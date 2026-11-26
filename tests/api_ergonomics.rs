//! The ergonomic API of roadmap phase 6 (`plans/product.md`, "API ergonomics"): the top-level re-exports, the `From`
//! conversions of Rust data, the `context!` macro, callables made of closures (`TwigFilter::from_fn` & co.) and, with
//! the `serde` feature, `to_value`/`to_context`.
//!
//! Expected outputs and messages come from the oracle (Twig 3.31.1 on PHP 8.5.13) rendering the same templates with
//! the PHP equivalent of the data (`json_decode()` for the integers beyond PHP's `int` range and for the maps) and
//! with PHP closures registered with the same options, e.g. `new TwigFilter('repeat', fn ($value, $times = 2, $sep =
//! '') => implode($sep, array_fill(0, $times, $value)))` for the `repeat` filter below. Where twig-rs cannot match
//! (the name PHP gives a closure in an `ArgumentCountError`), the test says so.

use std::collections::{BTreeMap, HashMap};
use std::io::Write as _;

use twig::extension::{CallableParameter, ConstantValue, DeprecatedCallableInfo};
use twig::loader::{ArrayLoader, ChainLoader, FilesystemLoader};
use twig::runtime::{DiagnosticsPolicy, Notice};
use twig::value::{ArrayKey, PhpString, convert};
use twig::{
    Array, Environment, EnvironmentOptions, ErrorKind, TwigFilter, TwigFunction, TwigTest, Value,
    context,
};

fn env(template: &str) -> Environment {
    Environment::new(ArrayLoader::new([("index", template)]))
}

fn render(env: &Environment, context: Array) -> String {
    env.render("index", context).unwrap()
}

fn render_error(env: &Environment, context: Array) -> twig::Error {
    env.render("index", context).unwrap_err()
}

fn keys(array: &Array) -> Vec<ArrayKey> {
    array.keys().cloned().collect()
}

/// A string argument converted the way PHP's string interpolation converts it.
fn string(
    call: &mut twig::extension::Call<'_>,
    value: &Value,
) -> twig::extension::CallResult<String> {
    Ok(convert::to_string(value, call.diagnostics())?
        .to_string_lossy()
        .into_owned())
}

// ---- re-exports -------------------------------------------------------------------------------------------------

/// The names a first program needs are at the crate root (and the loaders in `twig::loader`), as the very types of
/// the modules that define them.
#[test]
fn top_level_names_are_the_engine_types() {
    fn same<T>(_: &T, _: &T) {}

    let env: twig::Environment = twig::runtime::Environment::with_options(
        ArrayLoader::new([("index", "{{ x }}")]),
        twig::runtime::EnvironmentOptions::default(),
    );
    let options: EnvironmentOptions = twig::runtime::EnvironmentOptions::default();
    same(&options.charset, &String::new());
    let array: twig::Array = twig::value::Array::new();
    let value: twig::Value = twig::value::Value::from(array.clone());
    same(&value, &Value::Null);
    let source: twig::Source = twig::source::Source::new(b"x".to_vec(), "x".to_owned());
    same(&source, &twig::Source::new(Vec::new(), String::new()));
    let template: twig::TemplateWrapper = env.load("index").unwrap();
    same(&template, &env.load("index").unwrap());
    let result: twig::Result<String> = template.render(context! { x => 1 });
    same(&result, &Ok::<String, twig::error::Error>(String::new()));
    let error: twig::Error = env.render("missing", Array::new()).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Loader);
    let markup: Option<twig::Markup> = None::<twig::runtime::Markup>;
    assert!(markup.is_none());
    let filter: twig::TwigFilter =
        twig::extension::TwigFilter::new("f", None, twig::extension::FilterOptions::default());
    same(&filter.to_string(), &"Twig\\TwigFilter(f)".to_owned());
    let _: Option<twig::TwigFunction> = None::<twig::extension::TwigFunction>;
    let _: Option<twig::TwigTest> = None::<twig::extension::TwigTest>;
    let _: Option<FilesystemLoader> = None::<twig::loader::filesystem::FilesystemLoader>;
    let _: Option<ChainLoader> = None::<twig::loader::chain::ChainLoader>;
}

// ---- context! and From conversions ------------------------------------------------------------------------------

#[test]
fn context_macro_builds_arrays() {
    assert!(context! {}.is_empty());

    let title = "Users";
    let user = context! { name => "Fabien", roles => vec!["admin", "dev"] };
    let context = context! {
        title,
        users => vec![user],
        "page-size" => 20,
        next => None::<i64>,
    };
    assert_eq!(
        keys(&context),
        vec![
            ArrayKey::from("title"),
            ArrayKey::from("users"),
            ArrayKey::from("page-size"),
            ArrayKey::from("next"),
        ]
    );
    let env = env(
        "{{ title }}: {% for user in users %}{{ user.name }} ({{ user.roles|join(', ') }}){% endfor %}, \
         {{ _context['page-size'] }} per page, next: {{ next ?? 'none' }}",
    );
    assert_eq!(
        render(&env, context),
        "Users: Fabien (admin, dev), 20 per page, next: none"
    );

    // PHP's array-literal rules: "1" is the int key 1, a repeated key keeps its first position and the last value
    let context = context! { "1" => "a", b => 2, "1" => "c", 7 => 'x' };
    assert_eq!(
        keys(&context),
        vec![ArrayKey::Int(1), ArrayKey::from("b"), ArrayKey::Int(7)]
    );
    assert_eq!(context.get_int(1), Some(&Value::from("c")));
    assert_eq!(context.get_int(7), Some(&Value::from("x")));
}

/// Integers beyond PHP's `int` range become floats as with `json_decode()`, map keys are normalised like PHP array
/// keys, and the results print like the PHP values (oracle: the same template with `json_decode()`d data).
#[test]
fn rust_data_renders_like_the_php_equivalent() {
    let mut m = BTreeMap::new();
    m.insert("01", 1);
    m.insert("1", 2);
    m.insert("a", 3);
    let mut o = BTreeMap::new();
    o.insert(3u64, "small");
    o.insert(u64::MAX, "big");
    let context = context! {
        a => i64::MAX as u64,
        b => i64::MAX as u64 + 1,
        c => u64::MAX,
        d => i128::from(i64::MIN) - 1,
        f => 0.1f32,
        m,
        n => [(true, "t"), (false, "f")],
        o,
        e => None::<i32>,
        g => vec![Value::from(vec![1, 2]), Value::from(context! { x => 1.5, y => 'é' })],
    };
    let env = env(
        "{{ a }}|{{ b }}|{{ c }}|{{ d }}|{{ f }}|{% for k, v in m %}{{ k|json_encode }}={{ v }},{% endfor %}|\
         {% for k, v in n %}{{ k|json_encode }}={{ v }},{% endfor %}|\
         {% for k, v in o %}{{ k|json_encode }}={{ v }},{% endfor %}|{{ e is null ? \"null\" }}|{{ g|json_encode }}",
    );
    assert_eq!(
        render(&env, context),
        "9223372036854775807|9.2233720368548E+18|1.844674407371E+19|-9.2233720368548E+18|0.10000000149012|\
         &quot;01&quot;=1,1=2,&quot;a&quot;=3,|1=t,0=f,|3=small,&quot;18446744073709551615&quot;=big,|null|\
         [[1,2],{&quot;x&quot;:1.5,&quot;y&quot;:&quot;\\u00e9&quot;}]"
    );
}

/// Maps convert into whole contexts; their keys are normalised (oracle: `render('index', ['1' => 'x', 'name' =>
/// 'y'])`).
#[test]
fn maps_are_contexts() {
    let env = env("{{ 1 }}{{ _context|keys|json_encode }}");
    let map = BTreeMap::from([("1", "x"), ("name", "y")]);
    assert_eq!(render(&env, Array::from(map)), "1[1,&quot;name&quot;]");

    let env = self::env("Hello {{ name }}!");
    let map = HashMap::from([("name".to_owned(), "World".to_owned())]);
    assert_eq!(render(&env, Array::from(map)), "Hello World!");
    assert_eq!(
        render(
            &env,
            Array::from([("name", PhpString::from(&b"bytes"[..]))])
        ),
        "Hello bytes!"
    );
}

/// Globals take any convertible value.
#[test]
fn globals_take_converted_values() {
    let env = env("{{ site.name }} {{ site.langs|join('/') }}");
    env.add_global("site", context! { name => "Docs", langs => ["en", "lt"] })
        .unwrap();
    assert_eq!(render(&env, Array::new()), "Docs en/lt");
}

// ---- rendering conveniences that exist today -----------------------------------------------------------------------

/// Template source rendered without registering it (Twig's `createTemplate()`), and output into any `io::Write`
/// through `IoSink`.
#[test]
fn string_templates_and_io_writers() {
    let env = env("");
    let template = env
        .create_template(b"{{ greeting }}, {{ name }}!", None)
        .unwrap();
    assert_eq!(
        template
            .render(context! { greeting => "Hello", name => "you" })
            .unwrap(),
        "Hello, you!"
    );

    let env = self::env("{% for i in 1..3 %}{{ i }}{% endfor %}");
    let mut out = std::io::Cursor::new(Vec::new());
    env.display("index", Array::new(), &mut twig::vm::IoSink(&mut out))
        .unwrap();
    out.flush().unwrap();
    assert_eq!(out.into_inner(), b"123");
}

// ---- callables made of closures -------------------------------------------------------------------------------------

#[test]
fn filter_from_fn_with_safety_options() {
    let env = env("{{ text|wrap('em') }}");
    let mut wrap = TwigFilter::from_fn("wrap", ["tag".into()], |call, args| {
        let [text, tag] = args else { unreachable!() };
        let text = string(call, text)?;
        let tag = string(call, tag)?;
        Ok(format!("<{tag}>{text}</{tag}>"))
    });
    wrap.is_safe(["html"]).pre_escape("html");
    env.add_filter(wrap.build()).unwrap();
    assert_eq!(
        render(&env, context! { text => "<b>" }),
        "<em>&lt;b&gt;</em>"
    );

    // without `is_safe`, the output is escaped
    let env = self::env("{{ text|wrap('em') }}");
    env.add_filter(
        TwigFilter::from_fn("wrap", ["tag".into()], |call, args| {
            let text = string(call, &args[0])?;
            let tag = string(call, &args[1])?;
            Ok(format!("<{tag}>{text}</{tag}>"))
        })
        .build(),
    )
    .unwrap();
    assert_eq!(
        render(&env, context! { text => "x" }),
        "&lt;em&gt;x&lt;/em&gt;"
    );
}

/// The parameter list drives Twig's binding of named arguments and its errors.
#[test]
fn parameters_bind_like_php_closure_parameters() {
    let repeat = || {
        TwigFilter::from_fn(
            "repeat",
            [
                CallableParameter::optional("times", 2.into()),
                CallableParameter::optional("sep", "".into()),
            ],
            |call, args| {
                let [value, times, sep] = args else {
                    unreachable!()
                };
                let value = string(call, value)?;
                let times = convert::to_long(times, call.diagnostics())?;
                let sep = string(call, sep)?;
                Ok(vec![value; usize::try_from(times).unwrap_or(0)].join(sep.as_str()))
            },
        )
        .build()
    };
    let env = env("{{ 'ab'|repeat }}|{{ 'ab'|repeat(3) }}|{{ 'ab'|repeat(sep: '-', times: 2) }}");
    env.add_filter(repeat()).unwrap();
    assert_eq!(render(&env, Array::new()), "abab|ababab|ab-ab");

    let env = self::env("{{ 'x'|repeat(foo: 1) }}");
    env.add_filter(repeat()).unwrap();
    let error = render_error(&env, Array::new());
    assert_eq!(error.kind(), ErrorKind::Syntax);
    assert_eq!(
        error.to_string(),
        "Unknown argument \"foo\" for filter \"repeat(times, sep)\" in \"index\" at line 1."
    );

    let env = self::env("{{ 'x'|wrap }}");
    env.add_filter(TwigFilter::from_fn("wrap", ["tag".into()], |_, _| Ok("")).build())
        .unwrap();
    assert_eq!(
        render_error(&env, Array::new()).to_string(),
        "Value for argument \"tag\" is required for filter \"wrap\" in \"index\" at line 1."
    );
}

/// Defaults fill omitted arguments, also those skipped by a named argument and those an unpacked mapping leaves out;
/// an unpacked unknown name is PHP's `Error`, wrapped by the template.
#[test]
fn function_from_fn_with_defaults_and_unpacking() {
    let greet = || {
        TwigFunction::from_fn(
            "greet",
            [
                CallableParameter::optional("name", "nobody".into()),
                CallableParameter::optional("greeting", "Hello".into()),
            ],
            |call, args| {
                let name = string(call, &args[0])?;
                let greeting = string(call, &args[1])?;
                Ok(format!("{greeting} {name}"))
            },
        )
        .build()
    };
    let env = env(
        "{{ greet(...{name: 'Zed'}) }}|{{ greet() }}|{{ greet('Bob', 'Hi') }}|{{ greet(greeting: 'Yo') }}|\
         {{ greet('<b>') }}",
    );
    env.add_function(greet()).unwrap();
    assert_eq!(
        render(&env, Array::new()),
        "Hello Zed|Hello nobody|Hi Bob|Yo nobody|Hello &lt;b&gt;"
    );

    let env = self::env("{{ greet(...{nope: 1}) }}");
    env.add_function(greet()).unwrap();
    let error = render_error(&env, Array::new());
    assert_eq!(error.kind(), ErrorKind::Runtime);
    assert_eq!(
        error.to_string(),
        "An exception has been thrown during the rendering of a template (\"Unknown named parameter $nope\") in \
         \"index\" at line 1."
    );
}

/// Too few arguments (only possible by unpacking) is PHP's `ArgumentCountError`. PHP names the closure after its file
/// and line (`{closure:/app/src/Extension.php:12}`) and adds the calling file (` in <file> on line <n>` after
/// `passed`); twig-rs says `{closure}` and leaves the location out, as for every other call PHP reports that way.
#[test]
fn too_few_unpacked_arguments_are_an_argument_count_error() {
    let env = env("{{ need(...[]) }}");
    env.add_function(
        TwigFunction::from_fn("need", ["x".into()], |_, args| Ok(args[0].clone())).build(),
    )
    .unwrap();
    let error = render_error(&env, Array::new());
    assert_eq!(
        error.to_string(),
        "An exception has been thrown during the rendering of a template (\"Too few arguments to function \
         {closure}(), 0 passed and exactly 1 expected\") in \"index\" at line 1."
    );
}

/// A variadic parameter collects the extra positional and named arguments.
#[test]
fn variadic_functions_collect_the_rest() {
    let env = env("{{ pairs('a', 'b', x: 'c') }}|{{ pairs() }}|{{ pairs(...[1, 2]) }}");
    let mut pairs = TwigFunction::from_fn(
        "pairs",
        [CallableParameter::variadic("parts")],
        |call, args| {
            let parts = args[0].as_array().cloned().unwrap_or_default();
            let mut out = Vec::new();
            for (key, value) in &parts {
                out.push(format!("{key}={}", string(call, value)?));
            }
            Ok(out.join(","))
        },
    );
    pairs.is_variadic();
    env.add_function(pairs.build()).unwrap();
    assert_eq!(render(&env, Array::new()), "0=a,1=b,x=c||0=1,1=2");

    // `is_variadic` without a variadic parameter: Twig's compile-time error
    let env = self::env("{{ bad(1) }}");
    let mut bad = TwigFunction::from_fn("bad", ["x".into()], |_, args| Ok(args[0].clone()));
    bad.is_variadic();
    env.add_function(bad.build()).unwrap();
    assert_eq!(
        render_error(&env, Array::new()).to_string(),
        "The last parameter of \"Closure\" for function \"bad\" must be an array with default value, eg. \
         \"array $arg = []\" in \"index\"."
    );
}

/// The parts a `*` of a dynamic name matches come before the input in Twig; the closure reads them from the call.
#[test]
fn dynamic_filters_see_the_matched_parts() {
    let env = env("{{ 'x'|foo_suffix }}|{{ 'y'|bar_baz_suffix('!') }}");
    env.add_filter(
        TwigFilter::from_fn(
            "*_suffix",
            [CallableParameter::optional("end", "".into())],
            |call, args| {
                let suffix = String::from_utf8_lossy(&call.dynamic_arguments()[0]).into_owned();
                let value = string(call, &args[0])?;
                let end = string(call, &args[1])?;
                Ok(format!("{value}-{suffix}{end}"))
            },
        )
        .build(),
    )
    .unwrap();
    assert_eq!(render(&env, Array::new()), "x-foo|y-bar_baz!");
}

/// The injected values do not shift the template arguments: with `needs_charset`, `needs_environment` and
/// `needs_context`, `arg` still binds by name and position (oracle: `fn ($charset, $env, $context, $value, $arg =
/// 'd')`).
#[test]
fn injected_values_come_from_the_call() {
    let env = env("{{ 'v'|combo }}|{{ 'v'|combo(arg: 'a') }}|{{ 'v'|combo('b') }}");
    let mut combo = TwigFilter::from_fn(
        "combo",
        [CallableParameter::optional("arg", "d".into())],
        |call, args| {
            assert_eq!(call.env().charset(), "UTF-8");
            let user = call.context().get_str(b"user").cloned().unwrap_or_default();
            let user = string(call, &user)?;
            let value = string(call, &args[0])?;
            let arg = string(call, &args[1])?;
            Ok(format!("{}|{user}|{value}|{arg}", call.charset()))
        },
    );
    combo.needs_charset().needs_environment().needs_context();
    assert!(combo.options().needs_context);
    env.add_filter(combo.build()).unwrap();
    assert_eq!(
        render(&env, context! { user => "u" }),
        "UTF-8|u|v|d|UTF-8|u|v|a|UTF-8|u|v|b"
    );

    // without `needs_context`, the call has no context
    let env = self::env("{{ ctx() }}");
    env.add_function(
        TwigFunction::from_fn("ctx", [], |call, _| Ok(call.context().len() as i64)).build(),
    )
    .unwrap();
    assert_eq!(render(&env, context! { user => "u" }), "0");
}

#[test]
fn tests_from_fn() {
    let env = env(
        "{{ 3 is divisible_into(9) ? 'y' : 'n' }}{{ 2 is divisible_into(9) ? 'y' : 'n' }}|\
         {{ 'abc' is same_length 'xyz' ? 'y' : 'n' }}{{ 'ab' is not same_length 'xyz' ? 'y' : 'n' }}",
    );
    env.add_test(
        TwigTest::from_fn("divisible_into", ["n".into()], |call, args| {
            let value = convert::to_long(&args[0], call.diagnostics())?;
            let n = convert::to_long(&args[1], call.diagnostics())?;
            Ok(n % value == 0)
        })
        .build(),
    )
    .unwrap();
    let mut same_length = TwigTest::from_fn("same_length", ["other".into()], |call, args| {
        Ok(string(call, &args[0])?.len() == string(call, &args[1])?.len())
    });
    same_length.one_mandatory_argument();
    env.add_test(same_length.build()).unwrap();
    assert_eq!(render(&env, Array::new()), "yn|yy");
}

/// A parameter may share the input's usual name: the input parameter is renamed, so a named argument reaches the
/// declared one.
#[test]
fn a_parameter_named_value_is_a_template_argument() {
    let env = env("{{ 'in'|pick(value: 'arg') }}|{{ 'in'|pick(...{value: 'unpacked'}) }}");
    env.add_filter(
        TwigFilter::from_fn("pick", ["value".into()], |call, args| {
            Ok(format!(
                "{}<{}",
                string(call, &args[1])?,
                string(call, &args[0])?
            ))
        })
        .build(),
    )
    .unwrap();
    assert_eq!(render(&env, Array::new()), "arg&lt;in|unpacked&lt;in");
}

#[test]
fn deprecated_callables_report_their_deprecation() {
    let env = env("{{ 'x'|old }}");
    env.set_diagnostics_policy(DiagnosticsPolicy::Collect);
    let mut old = TwigFilter::from_fn("old", [], |_, args| Ok(args[0].clone()));
    old.deprecation_info(DeprecatedCallableInfo {
        alt_name: Some("new".to_owned()),
        ..DeprecatedCallableInfo::new("acme/pkg", "1.2")
    });
    env.add_filter(old.build()).unwrap();
    assert_eq!(render(&env, Array::new()), "x");
    let notices: Vec<String> = env
        .take_notices()
        .into_iter()
        .filter_map(|notice| match notice {
            Notice::Deprecation(deprecation) => Some(deprecation.to_string()),
            Notice::Php(_) => None,
        })
        .collect();
    assert_eq!(
        notices,
        vec![
            "Since acme/pkg 1.2: Twig Filter \"old\" is deprecated; use \"new\" instead in index at line 1."
                .to_owned()
        ]
    );
}

/// A builder can make several callables, and its result is an ordinary callable (reflection data included).
#[test]
fn builders_make_ordinary_callables() {
    let mut builder = TwigFunction::from_fn(
        "f",
        [
            "a".into(),
            CallableParameter::optional("b", ConstantValue::Null),
        ],
        |_, args| Ok(args.len() as i64),
    );
    builder.needs_context().is_safe(["html"]);
    let first = builder.build();
    let second = builder.build();
    assert!(!first.ptr_eq(&second));
    assert!(first.needs_context());
    assert_eq!(first.to_string(), "Twig\\TwigFunction(f)");
    let callable = first.callable().unwrap();
    assert_eq!(callable.reflection_name(), "Closure");
    let names: Vec<&str> = callable
        .parameters()
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    assert_eq!(names, ["context", "a", "b"]);
    assert_eq!(first.minimal_number_of_required_arguments(), 1);

    let env = env("{{ f(1) }}{{ g('x') }}");
    env.add_function(first).unwrap();
    env.add_function(
        TwigFunction::from_fn("g", [], |_, args| Ok(Value::from(args.len() as i64))).build(),
    )
    .unwrap();
    // `f` gets one value per declared parameter, `g` none (extra arguments are dropped, as PHP does for closures)
    assert_eq!(render(&env, Array::new()), "20");
}

// ---- serde -----------------------------------------------------------------------------------------------------------

#[cfg(feature = "serde")]
mod serde_feature {
    use serde::Serialize;

    use super::*;

    #[derive(Serialize)]
    struct Item {
        name: String,
        price: f64,
        tags: Vec<&'static str>,
        discount: Option<u8>,
    }

    #[derive(Serialize)]
    enum Status {
        Active,
        Suspended { reason: &'static str },
    }

    #[derive(Serialize)]
    struct Page {
        title: &'static str,
        items: Vec<Item>,
        status: Status,
        previous: Status,
    }

    fn page() -> Page {
        Page {
            title: "Shop",
            items: vec![
                Item {
                    name: "pen".to_owned(),
                    price: 1.5,
                    tags: vec!["office", "blue"],
                    discount: None,
                },
                Item {
                    name: "desk".to_owned(),
                    price: 10.0,
                    tags: vec![],
                    discount: Some(5),
                },
            ],
            status: Status::Active,
            previous: Status::Suspended { reason: "audit" },
        }
    }

    #[test]
    fn serializable_data_renders() {
        let env = env(
            "{{ title }}: {% for item in items %}{{ item.name }} {{ item.price }} [{{ item.tags|join('/') }}] \
             {{ item.discount ?? 'none' }}; {% endfor %}{{ status }}, {{ previous.Suspended.reason }}",
        );
        assert_eq!(
            render(&env, twig::to_context(&page()).unwrap()),
            "Shop: pen 1.5 [office/blue] none; desk 10 [] 5; Active, audit"
        );
        // inside context!
        let env = self::env("{{ page.items|length }} {{ page.items[0].tags[1] }}");
        let context = context! { page => twig::to_value(&page()).unwrap() };
        assert_eq!(render(&env, context), "2 blue");
    }

    /// JSON data renders like the same JSON `json_decode()`d by PHP, big integers included (oracle: `{{ c }}` of
    /// `json_decode('18446744073709551615')` above).
    #[test]
    fn json_data_renders_like_json_decode() {
        let data = serde_json::json!({
            "name": "x",
            "n": 18_446_744_073_709_551_615u64,
            "list": [1, "a", null, 2.5],
        });
        let env = env("{{ name }} {{ n }} {{ list|json_encode }}");
        assert_eq!(
            render(&env, twig::to_context(&data).unwrap()),
            "x 1.844674407371E+19 [1,&quot;a&quot;,null,2.5]"
        );
    }

    #[test]
    fn serialization_errors_are_twig_errors() {
        let error = twig::to_context(&"text").unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Logic);
        assert_eq!(
            error.to_string(),
            "the value must serialize to a map, a struct or a sequence to become an array, not to string"
        );
        let mut bad = BTreeMap::new();
        bad.insert(vec![1], 1);
        let error = twig::Error::from(twig::to_value(&bad).unwrap_err());
        assert_eq!(
            error.to_string(),
            "a map key must be a string, an integer, a bool, a char or a unit variant, not a sequence"
        );
        assert!(std::error::Error::source(&error).is_some());

        // in a closure, through `twig::Error`
        let env = self::env("{{ config().name }}");
        env.add_function(
            TwigFunction::from_fn("config", [], |_, _| {
                #[derive(Serialize)]
                struct Config {
                    name: &'static str,
                }
                Ok(twig::to_value(&Config { name: "site" }).map_err(twig::Error::from)?)
            })
            .build(),
        )
        .unwrap();
        assert_eq!(render(&env, Array::new()), "site");
    }
}
