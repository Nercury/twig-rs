//! The ergonomic API of roadmap phase 6: the top-level re-exports, the `From` conversions of Rust data, the `context!`
//! macro, render methods that take any data convertible into a context, `render_string`, writers as sinks, callables
//! made of closures (`TwigFilter::from_fn` & co.), host objects made with `ObjectBuilder` and, with the `serde`
//! feature, `to_value`/`to_context`.
//!
//! Expected outputs and messages come from the oracle (Twig 3.31.1 on PHP 8.5.13) rendering the same templates with
//! the PHP equivalent of the data (`json_decode()` for the integers beyond PHP's `int` range and for the maps), with
//! PHP closures registered with the same options, e.g. `new TwigFilter('repeat', fn ($value, $times = 2, $sep = '')
//! => implode($sep, array_fill(0, $times, $value)))` for the `repeat` filter below, and with the PHP classes that the
//! built host objects stand for (written out above each builder). Where twig-rs cannot match (the name PHP gives a
//! closure in an `ArgumentCountError`, the file PHP names in one), the test says so.

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

/// Maps are contexts as they are; their keys are normalised (oracle: `render('index', ['1' => 'x', 'name' =>
/// 'y'])`).
#[test]
fn maps_are_contexts() {
    let env = env("{{ 1 }}{{ _context|keys|json_encode }}");
    let map = BTreeMap::from([("1", "x"), ("name", "y")]);
    assert_eq!(env.render("index", map).unwrap(), "1[1,&quot;name&quot;]");

    let env = self::env("Hello {{ name }}!");
    let map = HashMap::from([("name".to_owned(), "World".to_owned())]);
    assert_eq!(env.render("index", map).unwrap(), "Hello World!");
    let pairs = [("name", PhpString::from(&b"bytes"[..]))];
    assert_eq!(env.render("index", pairs).unwrap(), "Hello bytes!");
}

/// Every method that takes a context takes anything that converts into an `Array`.
#[test]
fn render_methods_take_any_context_data() {
    let env = Environment::new(ArrayLoader::new([(
        "index",
        "{{ greeting }} {{ name }}{% block end %}{{ end ?? '!' }}{% endblock %}",
    )]));
    let map = BTreeMap::from([("greeting", "Hi"), ("name", "you")]);
    assert_eq!(env.render("index", map.clone()).unwrap(), "Hi you!");
    assert_eq!(env.render_bytes("index", map.clone()).unwrap(), b"Hi you!");
    let mut out = Vec::new();
    env.display("index", map.clone(), &mut out).unwrap();
    assert_eq!(out, b"Hi you!");

    let template = env.load("index").unwrap();
    let pairs = [("greeting", "Yo"), ("name", "me"), ("end", "?")];
    assert_eq!(template.render(pairs).unwrap(), "Yo me?");
    assert_eq!(template.render_bytes(pairs).unwrap(), b"Yo me?");
    assert_eq!(template.render_block("end", pairs).unwrap(), "?");
    assert_eq!(
        template.render_block_bytes("end", map.clone()).unwrap(),
        b"!"
    );
    let mut out = Vec::new();
    template.display_block("end", pairs, &mut out).unwrap();
    template.stream_block("end", map.clone(), &mut out).unwrap();
    template
        .display(context! { greeting => 1, name => 2 }, &mut out)
        .unwrap();
    template.stream(Array::new(), &mut out).unwrap();
    assert_eq!(out, b"?!1 2! !");
    assert!(template.has_block("end", map.clone()).unwrap());
    assert_eq!(template.block_names(map).unwrap(), ["end"]);
    // a list is a context too (the keys 0, 1, ...)
    let env = self::env("{{ _context|join(',') }}");
    assert_eq!(env.render("index", vec!["a", "b"]).unwrap(), "a,b");
}

/// Globals take any convertible value.
#[test]
fn globals_take_converted_values() {
    let env = env("{{ site.name }} {{ site.langs|join('/') }}");
    env.add_global("site", context! { name => "Docs", langs => ["en", "lt"] })
        .unwrap();
    assert_eq!(render(&env, Array::new()), "Docs en/lt");
}

// ---- rendering conveniences ------------------------------------------------------------------------------------------

/// Template source rendered without registering it: Twig's `createTemplate($source)->render($context)` (oracle: the
/// names, the loader error and the deprecation reported once for two `createTemplate()` calls of one source).
#[test]
fn render_string_renders_sources_no_loader_knows() {
    let env = Environment::new(ArrayLoader::new([
        ("layout", "[{% block body %}{% endblock %}]"),
        ("index", "{{ include('__string_template__' ~ hash) }}"),
    ]));
    let source = "{% extends 'layout' %}{% block body %}Hello {{ name }}!{% endblock %}";
    assert_eq!(
        env.render_string(source, context! { name => "Fabien" })
            .unwrap(),
        "[Hello Fabien!]"
    );
    let error = env
        .render_string("{{ name|nope }}", [("name", "Fabien")])
        .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Syntax);
    assert_eq!(
        error.to_string(),
        "Unknown \"nope\" filter in \"__string_template__e447569389deda7aacdfc7238ab4517c\" at line 1."
    );
    // the name is not registered with the loader
    assert_eq!(env.render_string(b"x", Array::new()).unwrap(), "x");
    let error = env
        .render("index", [("hash", "5c7401c0ec22eeeeeaf06c6480b2cd11")])
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "Template \"__string_template__5c7401c0ec22eeeeeaf06c6480b2cd11\" is not defined in \"index\" at line 1."
    );

    // compiled once per source, like the templates of a loader
    env.set_diagnostics_policy(DiagnosticsPolicy::Collect);
    for _ in 0..2 {
        assert_eq!(
            env.render_string("{{ 'a'|spaceless }}", Array::new())
                .unwrap(),
            "a"
        );
    }
    let notices: Vec<String> = env.take_notices().iter().map(ToString::to_string).collect();
    assert_eq!(
        notices,
        [
            "Deprecated: Since twig/twig 3.12: Twig Filter \"spaceless\" is deprecated in \
             __string_template__d325a5821eae967c6f2625380e868d8a at line 1."
        ]
    );
    let first = env.create_template(b"{{ x }}", None).unwrap();
    let again = env.create_template(b"{{ x }}", None).unwrap();
    assert!(std::sync::Arc::ptr_eq(first.template(), again.template()));
}

/// Any `io::Write` is a sink: `{% flush %}` flushes it; the `IoSink` wrapper of older code still works.
#[test]
fn display_writes_into_any_writer() {
    #[derive(Default)]
    struct Recorder {
        bytes: Vec<u8>,
        flushes: usize,
    }

    impl std::io::Write for Recorder {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            self.flushes += 1;
            Ok(())
        }
    }

    let env = env("{% for i in 1..3 %}{{ i }}{% endfor %}{% flush %}");
    let mut recorder = Recorder::default();
    env.display("index", Array::new(), &mut recorder).unwrap();
    assert_eq!(
        (recorder.bytes.as_slice(), recorder.flushes),
        (&b"123"[..], 1)
    );

    let mut cursor = std::io::Cursor::new(Vec::new());
    env.display("index", Array::new(), &mut cursor).unwrap();
    let mut writer: Box<dyn std::io::Write> = Box::new(Vec::new());
    env.display("index", Array::new(), &mut writer).unwrap();
    let mut buffer = std::io::BufWriter::new(Vec::new());
    env.load("index")
        .unwrap()
        .display(Array::new(), &mut buffer)
        .unwrap();
    assert_eq!(buffer.into_inner().unwrap(), b"123");
    env.display("index", Array::new(), &mut twig::vm::IoSink(&mut cursor))
        .unwrap();
    cursor.flush().unwrap();
    assert_eq!(cursor.into_inner(), b"123123");
    // a borrowed sink lends on
    let lend = |mut sink: &mut dyn twig::vm::Sink| env.display("index", Array::new(), &mut sink);
    let mut out = Vec::new();
    lend(&mut out).unwrap();
    assert_eq!(out, b"123");
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

// ---- host objects made with ObjectBuilder --------------------------------------------------------------------------

mod host_objects {
    use twig::runtime::{Sandbox, SecurityPolicy};
    use twig::value::FetchMode;
    use twig::value::object::{ObjectBuilder, ObjectRef, Param};

    use super::*;

    /// `$a.$b...`: PHP's string conversion of each part.
    fn concat(
        parts: &[&Value],
        diag: &mut dyn twig::value::Diagnostics,
    ) -> twig::value::PhpResult<PhpString> {
        let mut bytes = Vec::new();
        for part in parts {
            bytes.extend_from_slice(convert::to_string(part, diag)?.as_bytes());
        }
        Ok(PhpString::from(bytes))
    }

    /// ```php
    /// namespace App;
    ///
    /// class User
    /// {
    ///     public $name;
    ///     public $roles;
    ///
    ///     public function __construct($name, $roles) { $this->name = $name; $this->roles = $roles; }
    ///     public function isAdmin() { return in_array('admin', $this->roles, true); }
    ///     public function hasRole($role) { return in_array($role, $this->roles, true); }
    ///     public function greet($greeting, $end = '!') { return $greeting.' '.$this->name.$end; }
    ///     public function __toString(): string { return $this->name; }
    /// }
    /// ```
    fn user(name: &str, roles: &[&str]) -> ObjectRef {
        let roles: Vec<Value> = roles.iter().map(|role| Value::from(*role)).collect();
        let (admin, has) = (roles.clone(), roles.clone());
        ObjectBuilder::new("App\\User")
            .property("name", name)
            .property("roles", roles)
            .method("isAdmin", [], move |_, _, _| {
                Ok(admin.contains(&Value::from("admin")))
            })
            .method("hasRole", [Param::required("role")], move |_, args, _| {
                Ok(has.contains(&args[0]))
            })
            .method(
                "greet",
                [Param::required("greeting"), Param::optional("end", "!")],
                |this, args, diag| {
                    let name = this.read_property(b"name", FetchMode::Read, diag)?;
                    concat(&[&args[0], &Value::from(" "), &name, &args[1]], diag)
                },
            )
            .stringable(|this, diag| this.read_property(b"name", FetchMode::Read, diag))
            .build()
    }

    fn strict_env(templates: &[(&'static str, &'static str)]) -> Environment {
        Environment::with_options(
            ArrayLoader::new(templates.iter().copied()),
            EnvironmentOptions {
                strict_variables: true,
                ..Default::default()
            },
        )
    }

    /// The outputs (or the error messages) of the templates, in order.
    fn render_all(env: &Environment, names: &[&str], context: &Array) -> Vec<String> {
        names
            .iter()
            .map(|name| match env.render(name, context.clone()) {
                Ok(output) => output,
                Err(error) => error.to_string(),
            })
            .collect()
    }

    /// Methods are found like Twig finds those of a PHP object: by name, case-insensitively, through the `is` prefix;
    /// arguments bind like a PHP method's.
    #[test]
    fn built_objects_resolve_like_php_objects() {
        let env = strict_env(&[
            (
                "methods",
                "{{ user.name }}|{{ user.admin ? 'admin' : 'user' }}|{{ user.ISADMIN ? 'y' : 'n' }}|\
                 {{ user.isAdmin() ? 'y' : 'n' }}|{{ user.hasRole('dev') ? 'dev' : 'not dev' }}|\
                 {{ user.hasRole(1) ? 'one' : 'not one' }}|{{ user.greet('Hi') }}|\
                 {{ user.Greet(end: '?', greeting: 'Yo') }}|{{ user.greet('Hey', '.', 'extra') }}",
            ),
            (
                "strings",
                "{{ user }}|{{ user ~ '!' }}|{{ user|upper }}|{{ user|length }}|{{ user.roles|join(',') }}|\
                 {{ user|json_encode }}",
            ),
            (
                "defined",
                "{{ user.missing is defined ? 'y' : 'n' }}|{{ user.name is defined ? 'y' : 'n' }}|\
                 {{ user.greet is defined ? 'y' : 'n' }}|{{ user.missing ?? 'none' }}",
            ),
            (
                "compare",
                "{{ user == same ? 'equal' : 'different' }}|{{ user == other ? 'equal' : 'different' }}|\
                 {{ user is same as(same) ? 'same' : 'not same' }}|{{ user is same as(user) ? 'same' : 'not same' }}",
            ),
            ("unknown named", "{{ user.greet(nope: 1) }}"),
            ("too few", "{{ user.greet() }}"),
            ("missing", "{{ user.missing }}"),
        ]);
        let context = context! {
            user => user("Fabien", &["admin", "dev"]),
            same => user("Fabien", &["admin", "dev"]),
            other => user("Nerijus", &["dev"]),
        };
        let names = [
            "methods",
            "strings",
            "defined",
            "compare",
            "unknown named",
            "too few",
            "missing",
        ];
        assert_eq!(
            render_all(&env, &names, &context),
            [
                "Fabien|admin|y|y|dev|not one|Hi Fabien!|Yo Fabien?|Hey Fabien.",
                "Fabien|Fabien!|FABIEN|6|admin,dev|\
                 {&quot;name&quot;:&quot;Fabien&quot;,&quot;roles&quot;:[&quot;admin&quot;,&quot;dev&quot;]}",
                "n|y|y|none",
                // objects of one class compare by their properties
                "equal|different|not same|same",
                "An exception has been thrown during the rendering of a template (\"Unknown named parameter $nope\") \
                 in \"unknown named\" at line 1.",
                // PHP: `0 passed in /.../CoreExtension.php on line 2001 and ...` (reference/deviations.md)
                "An exception has been thrown during the rendering of a template (\"Too few arguments to function \
                 App\\User::greet(), 0 passed and at least 1 expected\") in \"too few\" at line 1.",
                "Neither the property \"missing\" nor one of the methods \"missing()\", \"getmissing()\", \
                 \"ismissing()\", \"hasmissing()\" or \"__call()\" exist and have public access in class \"App\\User\" \
                 in \"missing\" at line 1.",
            ]
        );
    }

    /// ```php
    /// namespace App;
    ///
    /// class Config implements \ArrayAccess
    /// {
    ///     public $name = 'property';
    ///
    ///     public function __construct(private array $data) {}
    ///     public function getName() { return 'getter'; }
    ///     public function getOther() { return 'other getter'; }
    ///     public function offsetExists($offset): bool { return is_string($offset) && array_key_exists($offset, $this->data); }
    ///     public function offsetGet($offset): mixed { return $this->offsetExists($offset) ? $this->data[$offset] : null; }
    ///     public function offsetSet($offset, $value): void { throw new \LogicException('App\Config is read-only.'); }
    ///     public function offsetUnset($offset): void { throw new \LogicException('App\Config is read-only.'); }
    /// }
    /// ```
    ///
    /// with `new Config(['title' => 'offset', 'theme' => 'dark', 'empty' => null])`.
    fn config() -> ObjectRef {
        let data = context! { title => "offset", theme => "dark", empty => None::<i64> };
        ObjectBuilder::new("App\\Config")
            .property("name", "property")
            .method("getName", [], |_, _, _| Ok("getter"))
            .method("getOther", [], |_, _, _| Ok("other getter"))
            .array_access(move |_, offset, _| {
                Ok(match offset {
                    Value::String(key) => data.get_str(key.as_bytes()).cloned(),
                    _ => None,
                })
            })
            .build()
    }

    /// `ArrayAccess` offsets come before properties, properties before methods (unless called with parentheses).
    #[test]
    fn offsets_come_before_properties_and_properties_before_methods() {
        let env = strict_env(&[
            (
                "resolution",
                "{{ config.title }}|{{ config.name }}|{{ config.name() }}|{{ config.getName }}|{{ config.other }}|\
                 {{ config['theme'] }}|{{ config.nothing ?? 'none' }}|{{ config.theme is defined ? 'y' : 'n' }}|\
                 {{ config.absent is defined ? 'y' : 'n' }}|{{ config.empty is null ? 'null' : 'not null' }}|\
                 {{ config.empty ?? 'default' }}|{{ config[0] ?? 'no 0' }}",
            ),
            ("set", "{{ config.offsetSet('a', 1) }}"),
            ("missing", "{{ config.nothing }}"),
            ("missing key", "{{ config['nothing'] }}"),
        ]);
        let context = context! { config => config() };
        assert_eq!(
            render_all(
                &env,
                &["resolution", "set", "missing", "missing key"],
                &context
            ),
            [
                "offset|property|getter|getter|other getter|dark|none|y|n|null|default|no 0",
                "An exception has been thrown during the rendering of a template (\"App\\Config is read-only.\") in \
                 \"set\" at line 1.",
                "Neither the property \"nothing\" nor one of the methods \"nothing()\", \"getnothing()\", \
                 \"isnothing()\", \"hasnothing()\" or \"__call()\" exist and have public access in class \
                 \"App\\Config\" in \"missing\" at line 1.",
                "Key \"nothing\" does not exist in ArrayAccess-able object of class \"App\\Config\" in \"missing key\" \
                 at line 1.",
            ]
        );
    }

    /// ```php
    /// namespace App;
    ///
    /// class Magic
    /// {
    ///     public function __isset($name) { return 'shown' === $name; }
    ///     public function __get($name) { return 'got '.$name; }
    ///     public function __call($name, $arguments) { return $name.'('.implode(',', $arguments).')'; }
    /// }
    ///
    /// class Steps implements \IteratorAggregate
    /// {
    ///     public function __construct(private array $steps) {}
    ///     public function getIterator(): \Iterator { return new \ArrayIterator($this->steps); }
    /// }
    /// ```
    ///
    /// with `new Steps(['lex' => 'lexer', 'parse' => 'parser', 'run' => 'vm'])`.
    #[test]
    fn magic_methods_and_iteration() {
        let magic = ObjectBuilder::new("App\\Magic")
            .method("__isset", [Param::required("name")], |_, args, _| {
                Ok(args[0] == Value::from("shown"))
            })
            .method("__get", [Param::required("name")], |_, args, diag| {
                concat(&[&Value::from("got "), &args[0]], diag)
            })
            .method(
                "__call",
                [Param::required("name"), Param::required("arguments")],
                |_, args, diag| {
                    let mut parts = Vec::new();
                    for (_, argument) in args[1].as_array().into_iter().flatten() {
                        parts.push(
                            convert::to_string(argument, diag)?
                                .to_string_lossy()
                                .into_owned(),
                        );
                    }
                    let name = convert::to_string(&args[0], diag)?;
                    Ok(format!("{}({})", name.to_string_lossy(), parts.join(",")))
                },
            )
            .build();
        let steps = ObjectBuilder::new("App\\Steps")
            .iterable(|_, _| Ok(context! { lex => "lexer", parse => "parser", run => "vm" }))
            .build();
        let env = strict_env(&[
            (
                "magic",
                "{{ magic.shown }}|{{ magic.hidden }}|{{ magic.run(1, 2) }}|{{ magic.run(a: 1) }}|\
                 {{ magic.hidden is defined ? 'y' : 'n' }}|{{ magic.shown is defined ? 'y' : 'n' }}",
            ),
            (
                "steps",
                "{{ steps|length }}|{% for key, step in steps %}{{ key }}={{ step }}{{ loop.first ? '(first)' }} \
                 {% endfor %}|{{ steps|keys|join(',') }}|{{ steps|join('/') }}|{{ steps is iterable ? 'iterable' }}|\
                 {{ steps is empty ? 'empty' : 'not empty' }}",
            ),
            ("steps ends", "{{ steps|first }}|{{ steps|last }}"),
        ]);
        let context = context! { magic, steps };
        assert_eq!(
            render_all(&env, &["magic", "steps", "steps ends"], &context),
            [
                "got shown|hidden()|run(1,2)|run(1)|y|y",
                "3|lex=lexer(first) parse=parser run=vm |lex,parse,run|lexer/parser/vm|iterable|not empty",
                "lexer|vm",
            ]
        );
    }

    /// Sandbox policies name the class and the members of a built object like those of a PHP object.
    #[test]
    fn sandbox_policies_apply_to_built_objects() {
        let env = Environment::new(ArrayLoader::new([
            ("allowed", "{{ user.name }} {{ user.admin ? 'admin' }}"),
            ("method", "{{ user.greet('Hi') }}"),
            ("property", "{{ user.roles }}"),
            ("to string", "{{ user }}"),
        ]));
        let policy = SecurityPolicy::new();
        policy.set_allowed_filters(["escape"]);
        policy.set_allowed_methods([("App\\User", ["isAdmin"])]);
        policy.set_allowed_properties([("App\\User", ["name"])]);
        policy.set_strict(true);
        let sandbox = Sandbox::new(env, std::sync::Arc::new(policy)).unwrap();
        let context = context! { user => user("Fabien", &["admin", "dev"]) };
        let results: Vec<String> = ["allowed", "method", "property", "to string"]
            .iter()
            .map(|name| match sandbox.render(name, context.clone()) {
                Ok(output) => output,
                Err(error) => format!("{}: {error}", error.class_name()),
            })
            .collect();
        assert_eq!(
            results,
            [
                "Fabien admin",
                "Twig\\Sandbox\\SecurityNotAllowedMethodError: Calling \"greet\" method on a \"App\\User\" object is \
                 not allowed in \"method\" at line 1.",
                "Twig\\Sandbox\\SecurityNotAllowedPropertyError: Calling \"roles\" property on a \"App\\User\" object \
                 is not allowed in \"property\" at line 1.",
                "Twig\\Sandbox\\SecurityNotAllowedMethodError: Calling \"__tostring\" method on a \"App\\User\" object \
                 is not allowed in \"to string\" at line 1.",
            ]
        );
    }
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
