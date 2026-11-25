//! The environment API: options, extension lifecycle, globals, template cache, string templates, sinks, host
//! callables and the limits of the core engine. Expected messages come from Twig 3.31.1 (quoted from its sources and
//! checked with the oracle where noted).

use std::sync::{Arc, Mutex};

use twig::ErrorKind;
use twig::deprecation::Deprecation;
use twig::escaper::DefaultStrategy;
use twig::extension::runtime::CallableRuntime;
use twig::extension::{
    CallableParameter, Extension, FilterOptions, FunctionOptions, PhpCallable, TwigFilter,
    TwigFunction,
};
use twig::loader::ArrayLoader;
use twig::runtime::{
    DiagnosticsPolicy, Environment, EnvironmentOptions, Markup, Notice, SandboxExtension,
    SecurityError, SecurityErrorKind, SecurityPolicy, SecurityPolicyInterface,
    SourcePolicyInterface, TemplateWrapper,
};
use twig::value::{Array, PhpString, Value};
use twig::vm::Sink;

fn options() -> EnvironmentOptions {
    EnvironmentOptions {
        autoescape: DefaultStrategy::Disabled,
        ..Default::default()
    }
}

fn env(templates: &[(&str, &str)]) -> Environment {
    Environment::with_options(
        ArrayLoader::new(
            templates
                .iter()
                .map(|(n, c)| (n.to_string(), c.to_string())),
        ),
        options(),
    )
}

fn ctx(pairs: &[(&str, Value)]) -> Array {
    let mut array = Array::new();
    for (key, value) in pairs {
        array.insert(*key, value.clone());
    }
    array
}

#[test]
fn renders_and_displays() {
    let env = env(&[("index", "Hello {{ name }}!")]);
    assert_eq!(
        env.render("index", ctx(&[("name", Value::from("World"))]))
            .unwrap(),
        "Hello World!"
    );
    let mut out: Vec<u8> = Vec::new();
    env.display("index", ctx(&[("name", Value::from("you"))]), &mut out)
        .unwrap();
    assert_eq!(out, b"Hello you!");
}

#[test]
fn missing_templates_are_loader_errors() {
    let env = env(&[]);
    let error = env.render("nope", Array::new()).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Loader);
    assert_eq!(error.to_string(), "Template \"nope\" is not defined.");
}

#[test]
fn charset_is_normalized() {
    let env = env(&[("index", "{{ _charset }}")]);
    assert_eq!(env.charset(), "UTF-8");
    env.set_charset("utf8");
    assert_eq!(env.charset(), "UTF-8");
    env.set_charset("iso-8859-1");
    assert_eq!(env.render("index", Array::new()).unwrap(), "ISO-8859-1");
}

#[test]
fn globals_follow_get_globals() {
    let env = env(&[("index", "{{ g }}|{{ _context|keys|join(',') }}")]);
    env.add_global("g", "global").unwrap();
    assert_eq!(
        env.render("index", ctx(&[("a", Value::Int(1))])).unwrap(),
        "global|a,g"
    );
    // the context wins over a global of the same name
    assert_eq!(
        env.render("index", ctx(&[("g", Value::from("ctx"))]))
            .unwrap(),
        "ctx|g"
    );
    // after initialization only existing globals can change
    env.add_global("g", "updated").unwrap();
    assert_eq!(env.render("index", Array::new()).unwrap(), "updated|g");
    let error = env.add_global("other", 1).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Logic);
    assert_eq!(
        error.to_string(),
        "Unable to add global \"other\" as the runtime or the extensions have already been initialized."
    );
}

#[derive(Debug)]
struct Late;

impl Extension for Late {
    fn class_name(&self) -> &str {
        "App\\LateExtension"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[test]
fn extensions_freeze_on_first_load() {
    let env = env(&[("index", "x")]);
    env.add_extension(Arc::new(Late)).unwrap();
    assert!(env.has_extension("App\\LateExtension"));
    assert_eq!(
        env.add_extension(Arc::new(Late)).unwrap_err().to_string(),
        "Unable to register extension \"App\\LateExtension\" as it is already registered."
    );
    env.render("index", Array::new()).unwrap();
    let error = env.add_extension(Arc::new(Late)).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Logic);
    assert_eq!(
        error.to_string(),
        "Unable to register extension \"App\\LateExtension\" as extensions have already been initialized."
    );
    let filter = TwigFilter::new("late", None, FilterOptions::default());
    assert_eq!(
        env.add_filter(filter).unwrap_err().to_string(),
        "Unable to add filter \"late\" as extensions have already been initialized."
    );
}

#[test]
fn template_instances_are_cached_by_cache_key() {
    let loader = Arc::new(ArrayLoader::new([("index", "one")]));
    let env = Environment::with_shared_loader(loader.clone(), options());
    let first = env.load("index").unwrap();
    let again = env.load("index").unwrap();
    assert!(Arc::ptr_eq(first.template(), again.template()));
    // a changed source is a new cache key: a new template
    loader.set_template("index", "two");
    let changed = env.load("index").unwrap();
    assert!(!Arc::ptr_eq(first.template(), changed.template()));
    assert_eq!(changed.render(Array::new()).unwrap(), "two");
    assert_eq!(first.render(Array::new()).unwrap(), "one");
}

#[test]
fn strict_variables_recompiles() {
    let env = env(&[("index", "[{{ undefined }}]")]);
    assert_eq!(env.render("index", Array::new()).unwrap(), "[]");
    env.enable_strict_variables();
    assert_eq!(
        env.render("index", Array::new()).unwrap_err().to_string(),
        "Variable \"undefined\" does not exist in \"index\" at line 1."
    );
    env.disable_strict_variables();
    assert_eq!(env.render("index", Array::new()).unwrap(), "[]");
}

#[test]
fn string_templates_are_named_after_their_hash() {
    // VERIFIED with the oracle (reference/twig/runtime.md §7; the strict-mode message from a direct oracle run)
    let env = env(&[]);
    let template = env.create_template(b"hello {{ name }}", None).unwrap();
    assert_eq!(
        template.template_name(),
        "__string_template__07592801e7ab6f3c2b0b6a04f5a254fc"
    );
    let named = env
        .create_template(b"hello {{ name }}", Some("foo.html"))
        .unwrap();
    assert_eq!(
        named.template_name(),
        "foo.html (string template 07592801e7ab6f3c2b0b6a04f5a254fc)"
    );
    assert_eq!(
        named.render(ctx(&[("name", Value::from("x"))])).unwrap(),
        "hello x"
    );
    env.enable_strict_variables();
    let strict = env.create_template(b"{{ x }}", None).unwrap();
    assert_eq!(
        strict.render(Array::new()).unwrap_err().to_string(),
        "Variable \"x\" does not exist in \"__string_template__c145ecd2804ae00fd8cf7eb0ee0ac2be\" at line 1."
    );
}

#[derive(Default)]
struct Recorder {
    chunks: Vec<Vec<u8>>,
    flushes: usize,
}

impl Sink for Recorder {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        self.chunks.push(bytes.to_vec());
        Ok(())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.flushes += 1;
        Ok(())
    }
}

#[test]
fn flush_reaches_displayed_sinks_only() {
    let env = env(&[(
        "index",
        "a{% flush %}b{% set x %}c{% flush %}{% endset %}{{ x }}",
    )]);
    let mut recorder = Recorder::default();
    env.display("index", Array::new(), &mut recorder).unwrap();
    assert_eq!(recorder.flushes, 1, "a flush inside a capture does nothing");
    assert_eq!(recorder.chunks.concat(), b"abc");
    // render() swallows flushes
    assert_eq!(env.render("index", Array::new()).unwrap(), "abc");
}

#[test]
fn errors_discard_rendered_output_but_not_displayed_output() {
    let env = env(&[("index", "before{{ 1 / 0 }}after")]);
    let error = env.render("index", Array::new()).unwrap_err();
    assert_eq!(
        error.to_string(),
        "An exception has been thrown during the rendering of a template (\"Division by zero\") in \"index\" at line 1."
    );
    let previous = std::error::Error::source(&error).map(ToString::to_string);
    assert_eq!(
        previous.as_deref(),
        Some("DivisionByZeroError: Division by zero")
    );
    let mut out: Vec<u8> = Vec::new();
    assert!(env.display("index", Array::new(), &mut out).is_err());
    assert_eq!(out, b"before");
}

#[test]
fn non_utf8_output_needs_bytes() {
    let env = env(&[("index", "{{ bytes }}")]);
    let context = ctx(&[("bytes", Value::String(PhpString::from(&b"\xff\xfe"[..])))]);
    assert_eq!(
        env.render_bytes("index", context.clone()).unwrap(),
        b"\xff\xfe"
    );
    let error = env.render("index", context).unwrap_err();
    assert!(error.to_string().contains("is not valid UTF-8"), "{error}");
}

#[test]
fn diagnostics_policies() {
    let env = env(&[("index", "{{ items }}")]);
    let context = ctx(&[("items", Value::Array(Array::from_values([Value::Int(1)])))]);
    // ignore (the default)
    assert_eq!(env.render("index", context.clone()).unwrap(), "Array");
    assert!(env.take_notices().is_empty());
    // collect
    env.set_diagnostics_policy(DiagnosticsPolicy::Collect);
    assert_eq!(env.render("index", context.clone()).unwrap(), "Array");
    let notices = env.take_notices();
    assert_eq!(notices.len(), 1);
    assert_eq!(
        notices[0].to_string(),
        "Warning: Array to string conversion"
    );
    // error: warnings become exceptions that Twig wraps
    env.set_diagnostics_policy(DiagnosticsPolicy::Error);
    assert_eq!(
        env.render("index", context).unwrap_err().to_string(),
        "An exception has been thrown during the rendering of a template (\"Array to string conversion\") in \"index\" at line 1."
    );
    assert!(matches!(env.take_notices().as_slice(), [Notice::Php(_)]));
}

#[test]
fn deprecations_are_reported() {
    let env = env(&[(
        "index",
        "{% deprecated 'Use other.' package='acme/pkg' version='1.2' %}ok",
    )]);
    env.set_diagnostics_policy(DiagnosticsPolicy::Collect);
    assert_eq!(env.render("index", Array::new()).unwrap(), "ok");
    let notices: Vec<String> = env.take_notices().iter().map(ToString::to_string).collect();
    assert_eq!(
        notices,
        ["Deprecated: Since acme/pkg 1.2: Use other. in \"index\" at line 1."]
    );
}

#[test]
fn callables_without_runtime_fail_when_called() {
    let env = env(&[("index", "{{ 'abc'|nort }}")]);
    env.add_filter(TwigFilter::new(
        "nort",
        Some(PhpCallable::closure(vec![CallableParameter::required(
            "value",
        )])),
        FilterOptions::default(),
    ))
    .unwrap();
    let error = env.render("index", Array::new()).unwrap_err();
    assert_eq!(
        error.to_string(),
        "An exception has been thrown during the rendering of a template (\"The filter \"nort\" has no runtime implementation.\") in \"index\" at line 1."
    );
}

#[test]
fn host_callables_render() {
    let env = env(&[(
        "index",
        "{{ 'x'|wrap('[', ']') }}|{{ ctx_keys() }}|{{ 'a'|dyn_hello }}",
    )]);
    let wrap = PhpCallable::closure(vec![
        CallableParameter::required("charset"),
        CallableParameter::required("value"),
        CallableParameter::required("left"),
        CallableParameter::required("right"),
    ]);
    CallableRuntime::new(|call| {
        let charset = call.charset();
        let args: Vec<String> = call
            .args()
            .iter()
            .map(|v| v.as_str().map(|s| s.to_string()).unwrap_or_default())
            .collect();
        Ok(Value::from(format!(
            "{}{}{}({})",
            args[1],
            args[0],
            args[2],
            charset.to_string_lossy()
        )))
    })
    .attach(&wrap);
    env.add_filter(TwigFilter::new(
        "wrap",
        Some(wrap),
        FilterOptions {
            needs_charset: true,
            ..Default::default()
        },
    ))
    .unwrap();
    let keys = PhpCallable::closure(vec![CallableParameter::required("context")]);
    CallableRuntime::new(|call| {
        let keys: Vec<String> = call
            .context()
            .keys()
            .map(|k| k.to_php_string().to_string_lossy().into_owned())
            .collect();
        Ok(Value::from(keys.join(",")))
    })
    .attach(&keys);
    env.add_function(TwigFunction::new(
        "ctx_keys",
        Some(keys),
        FunctionOptions {
            needs_context: true,
            ..Default::default()
        },
    ))
    .unwrap();
    let dynamic = PhpCallable::closure(vec![
        CallableParameter::required("name"),
        CallableParameter::required("value"),
    ]);
    CallableRuntime::new(|call| {
        let part = String::from_utf8_lossy(&call.dynamic_arguments()[0]).into_owned();
        let value = call
            .arg(0)
            .and_then(Value::as_str)
            .map(|s| s.to_string())
            .unwrap_or_default();
        Ok(Value::from(format!("{part}:{value}")))
    })
    .attach(&dynamic);
    env.add_filter(TwigFilter::new(
        "dyn_*",
        Some(dynamic),
        FilterOptions::default(),
    ))
    .unwrap();
    assert_eq!(
        env.render("index", ctx(&[("a", Value::Int(1))])).unwrap(),
        "[x](UTF-8)|a|hello:a"
    );
}

#[test]
fn composition_renders_blocks_and_parents() {
    let env = env(&[
        (
            "child",
            "{% extends 'base' %}{% block title %}Child - {{ parent() }}{% endblock %}",
        ),
        (
            "base",
            "<{% block title %}Base{% endblock %}|{% block body %}{{ name }}{% endblock %}>",
        ),
    ]);
    let template = env.load("child").unwrap();
    assert_eq!(
        template.render(ctx(&[("name", Value::from("n"))])).unwrap(),
        "<Child - Base|n>"
    );
    assert_eq!(
        template.render_block("title", Array::new()).unwrap(),
        "Child - Base"
    );
    assert!(template.has_block("body", Array::new()).unwrap());
    assert!(!template.has_block("nope", Array::new()).unwrap());
    assert_eq!(
        template.block_names(Array::new()).unwrap(),
        ["title", "body"]
    );
    // a missing block is reported without a line: no template handles the error (checked with the oracle)
    assert_eq!(
        template
            .render_block("nope", Array::new())
            .unwrap_err()
            .to_string(),
        "Block \"nope\" on template \"base\" does not exist in \"child\"."
    );
    let mut out: Vec<u8> = Vec::new();
    template
        .display_block("body", ctx(&[("name", Value::from("d"))]), &mut out)
        .unwrap();
    assert_eq!(out, b"d");
}

#[test]
fn template_wrappers_are_values() {
    let env = env(&[
        ("index", "{% include tpl %}|{{ tpl.templateName }}"),
        ("part", "P"),
    ]);
    let part = env.load("part").unwrap();
    let value = part.to_value();
    assert_eq!(
        TemplateWrapper::from_value(&value).map(|w| w.template_name().to_owned()),
        Some("part".to_owned())
    );
    assert_eq!(
        env.render("index", ctx(&[("tpl", value)])).unwrap(),
        "P|part"
    );
}

#[test]
fn template_recursion_is_bounded() {
    // PHP recurses until it crashes; twig-rs stops at the depth limit
    let env = env(&[
        ("a", "{% extends 'b' %}"),
        ("b", "{% extends 'a' %}"),
        ("t", "{% use 't' %}"),
    ]);
    env.set_max_depth(20);
    assert_eq!(
        env.render("a", Array::new()).unwrap_err().to_string(),
        "An exception has been thrown during the rendering of a template (\"Maximum template call depth of 20 reached (twig-rs limit).\") in \"a\" at line 1."
    );
    let error = env.load("t").unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Logic);
    assert_eq!(
        error.to_string(),
        "Maximum template call depth of 20 reached (twig-rs limit)."
    );
}

#[test]
fn deep_template_recursion_fits_the_native_stack() {
    // 150 levels of macro, include and block recursion on a 4 MiB thread (debug builds need about 1 MiB)
    let handle = std::thread::Builder::new()
        .stack_size(4 * 1024 * 1024)
        .spawn(|| {
            let env = env(&[
                (
                    "macro",
                    "{% macro r(n) %}{% if n > 0 %}{{ _self.r(n - 1) }}{% endif %}{% endmacro %}{{ _self.r(150) }}.",
                ),
                (
                    "include",
                    "{% set n = n ?? 150 %}{% if n > 0 %}{% include 'include' with {n: n - 1} %}{% else %}.{% endif %}",
                ),
                (
                    "block",
                    "{% set n = 150 %}{% block b %}{% if n > 0 %}{% set n = n - 1 %}{{ block('b') }}{% else %}.{% endif %}{% endblock %}",
                ),
            ]);
            env.set_max_depth(1000);
            ["macro", "include", "block"].map(|name| env.render(name, Array::new()).unwrap())
        })
        .unwrap();
    assert_eq!(handle.join().unwrap(), [".", ".", "."]);
}

fn sandboxed_env(
    templates: &[(&str, &str)],
    filters: &[&str],
    global: bool,
) -> (Environment, Arc<SandboxExtension>) {
    let env = env(templates);
    let policy = SecurityPolicy::new();
    policy.set_allowed_filters(filters.iter().copied());
    policy.set_allowed_tags(["sandbox", "include", "if"]);
    policy.set_allowed_functions(["wait"]);
    let extension = Arc::new(SandboxExtension::new(Arc::new(policy), global));
    env.add_extension(extension.clone()).unwrap();
    (env, extension)
}

#[test]
fn security_errors_carry_their_class() {
    let (env, _) = sandboxed_env(&[("index", "{{ 'a'|lower }}")], &["upper"], true);
    let error = env.render("index", Array::new()).unwrap_err();
    let violation = SecurityError::of(&error).expect("a sandbox violation");
    assert_eq!(
        violation.class_name(),
        "Twig\\Sandbox\\SecurityNotAllowedFilterError"
    );
    assert_eq!(
        violation.kind(),
        &SecurityErrorKind::Filter("lower".to_owned())
    );
    assert_eq!(
        error.to_string(),
        "Filter \"lower\" is not allowed in \"index\" at line 1."
    );
    assert_eq!(error.template_line(), 1);
}

#[test]
fn template_sandbox_toggles_end_with_the_render() {
    let (env, _) = sandboxed_env(
        &[
            ("index", "{% sandbox %}{% include 'bad' %}{% endsandbox %}"),
            ("bad", "{{ 'a'|lower }}"),
            ("free", "{{ 'a'|lower }}"),
        ],
        &[],
        false,
    );
    assert_eq!(
        env.render("index", Array::new()).unwrap_err().to_string(),
        "Filter \"lower\" is not allowed in \"bad\" at line 1."
    );
    // the failed render left the sandbox (the `finally` of the sandbox tag)
    assert_eq!(env.render("free", Array::new()).unwrap(), "a");
    assert_eq!(env.render("bad", Array::new()).unwrap(), "a");
}

#[test]
fn template_sandbox_toggles_stay_on_their_thread() {
    use std::sync::Barrier;
    let barrier = Arc::new(Barrier::new(2));
    let (env, _) = sandboxed_env(
        &[
            (
                "index",
                "{% sandbox %}{% include 'inner' %}{% endsandbox %}",
            ),
            ("inner", "{{ wait() }}"),
            ("free", "{{ 'a'|lower }}"),
        ],
        &[],
        false,
    );
    let callable = PhpCallable::closure(vec![]);
    let gate = Arc::clone(&barrier);
    CallableRuntime::new(move |_| {
        gate.wait();
        gate.wait();
        Ok(Value::from("waited"))
    })
    .attach(&callable);
    env.add_function(TwigFunction::new(
        "wait",
        Some(callable),
        FunctionOptions::default(),
    ))
    .unwrap();
    let sandboxed = {
        let env = env.clone();
        std::thread::spawn(move || env.render("index", Array::new()))
    };
    barrier.wait();
    // the other thread is inside its sandboxed include: this render is not sandboxed
    assert_eq!(env.render("free", Array::new()).unwrap(), "a");
    barrier.wait();
    assert_eq!(sandboxed.join().unwrap().unwrap(), "waited");
}

#[test]
fn the_deprecated_sandbox_switch_sandboxes_every_template() {
    let (env, extension) = sandboxed_env(&[("index", "{{ 'a'|lower }}")], &[], false);
    assert_eq!(env.render("index", Array::new()).unwrap(), "a");
    let mut deprecations: Vec<Deprecation> = Vec::new();
    extension.enable_sandbox(&mut deprecations);
    assert!(extension.is_sandboxed(None));
    assert_eq!(
        env.render("index", Array::new()).unwrap_err().to_string(),
        "Filter \"lower\" is not allowed in \"index\" at line 1."
    );
    extension.disable_sandbox(&mut deprecations);
    assert_eq!(env.render("index", Array::new()).unwrap(), "a");
    assert_eq!(
        deprecations
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        [
            "Since twig/twig 3.29: The \"Twig\\Extension\\SandboxExtension::enableSandbox()\" method is deprecated, use \"Twig\\Sandbox\\Sandbox\" to render untrusted templates instead.",
            "Since twig/twig 3.29: The \"Twig\\Extension\\SandboxExtension::disableSandbox()\" method is deprecated, use \"Twig\\Sandbox\\Sandbox\" to render untrusted templates instead.",
        ]
    );
}

struct UntrustedNames;

impl SourcePolicyInterface for UntrustedNames {
    fn enable_sandbox(&self, source: &twig::Source) -> bool {
        source.name().starts_with("untrusted")
    }
}

#[test]
fn source_policies_choose_the_sandboxed_templates() {
    let env = env(&[
        ("trusted", "{{ 'a'|lower }}{% include 'untrusted' %}"),
        ("untrusted", "{{ 'b'|lower }}"),
    ]);
    let mut deprecations: Vec<Deprecation> = Vec::new();
    let extension = SandboxExtension::with_source_policy(
        Arc::new(SecurityPolicy::new()),
        false,
        Arc::new(UntrustedNames),
        &mut deprecations,
    );
    env.add_extension(Arc::new(extension)).unwrap();
    assert_eq!(
        env.render("trusted", Array::new()).unwrap_err().to_string(),
        "Filter \"lower\" is not allowed in \"untrusted\" at line 1."
    );
    assert_eq!(deprecations.len(), 1);
}

/// A policy allowing everything but the `lower` filter.
struct NoLower;

impl SecurityPolicyInterface for NoLower {
    fn check_security(
        &self,
        _tags: &[&[u8]],
        filters: &[&[u8]],
        _functions: &[&[u8]],
        _tests: &[&[u8]],
        _deprecations: &mut dyn twig::deprecation::DeprecationSink,
    ) -> Result<(), SecurityError> {
        match filters.iter().find(|f| **f == b"lower") {
            Some(_) => Err(SecurityError::new(
                SecurityErrorKind::Filter("lower".to_owned()),
                "No lowering.",
            )),
            None => Ok(()),
        }
    }

    fn check_method_allowed(
        &self,
        _object: &twig::value::object::ObjectRef,
        _method: &[u8],
    ) -> Result<(), SecurityError> {
        Ok(())
    }

    fn check_property_allowed(
        &self,
        _object: &twig::value::object::ObjectRef,
        _property: &Value,
    ) -> Result<(), SecurityError> {
        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[test]
fn custom_security_policies() {
    let env = env(&[
        ("ok", "{% if true %}{{ 'a'|upper }}{% endif %}"),
        ("bad", "\n{{ 'a'|upper }}{{ 'a'|lower }}"),
    ]);
    env.add_extension(Arc::new(SandboxExtension::new(Arc::new(NoLower), true)))
        .unwrap();
    assert_eq!(env.render("ok", Array::new()).unwrap(), "A");
    // the line of the first use of what the violation names
    assert_eq!(
        env.render("bad", Array::new()).unwrap_err().to_string(),
        "No lowering in \"bad\" at line 2."
    );
}

#[test]
fn depth_is_bounded() {
    let env = env(&[(
        "index",
        "{{ [1]|map(a => [2]|map(b => [3]|map(c => c)|join)|join)|join }}",
    )]);
    assert_eq!(env.render("index", Array::new()).unwrap(), "3");
    env.set_max_depth(3);
    let error = env.render("index", Array::new()).unwrap_err();
    assert_eq!(
        error.to_string(),
        "An exception has been thrown during the rendering of a template (\"Maximum template call depth of 3 reached (twig-rs limit).\") in \"index\" at line 1."
    );
}

#[test]
fn arrow_functions_need_a_running_render() {
    let env = env(&[("index", "{% set f = x => x %}{{ capture(f) }}")]);
    let captured: Arc<Mutex<Option<Value>>> = Arc::default();
    let callable = PhpCallable::closure(vec![CallableParameter::required("f")]);
    let slot = Arc::clone(&captured);
    CallableRuntime::new(move |call| {
        *slot.lock().unwrap() = call.arg(0).cloned();
        let f = call.arg(0).cloned().unwrap_or_default();
        call.invoke(&f, vec![Value::from("called")])
    })
    .attach(&callable);
    env.add_function(TwigFunction::new(
        "capture",
        Some(callable),
        FunctionOptions::default(),
    ))
    .unwrap();
    assert_eq!(env.render("index", Array::new()).unwrap(), "called");
    let closure = captured.lock().unwrap().clone().unwrap();
    let Value::Object(closure) = closure else {
        panic!("a closure object")
    };
    let mut diag = twig::value::Ignore;
    let error = closure
        .invoke(twig::value::object::CallArgs::new(), &mut diag)
        .unwrap_err();
    assert_eq!(
        error.message(),
        "A Twig arrow function can only be called while a template is rendering."
    );
}

#[test]
fn markup_values_render_and_compare() {
    let env = env(&[(
        "index",
        "{{ m }}|{{ m|length }}|{{ m ? 'y' : 'n' }}|{{ e ? 'y' : 'n' }}",
    )]);
    let m = Value::Object(Markup::create(
        PhpString::from("<b>é</b>"),
        Some("UTF-8".into()),
        None,
    ));
    let e = Markup::new_value(PhpString::empty(), "UTF-8");
    assert_eq!(
        env.render("index", ctx(&[("m", m), ("e", e)])).unwrap(),
        "<b>é</b>|8|y|n"
    );
}

#[test]
fn environments_render_from_many_threads() {
    let env = env(&[("index", "{% for i in 1..n %}{{ i }}{% endfor %}")]);
    let handles: Vec<_> = (1..=4)
        .map(|n| {
            let env = env.clone();
            std::thread::spawn(move || env.render("index", ctx(&[("n", Value::Int(n))])).unwrap())
        })
        .collect();
    let outputs: Vec<String> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(outputs, ["1", "12", "123", "1234"]);
}

#[test]
fn programs_disassemble() {
    let env = env(&[("index", "{% for i in items %}\n{{ i }}{% endfor %}")]);
    let template = env.load("index").unwrap();
    let listing = template.template().program().disassemble();
    assert!(listing.contains("IterNext"), "{listing}");
    assert!(listing.contains("ForRestore"), "{listing}");
}

#[test]
fn twig_registers_the_same_extensions() {
    let env = env(&[]);
    for class in [
        "Twig\\Extension\\CoreExtension",
        "Twig\\Extension\\EscaperExtension",
        "Twig\\Extension\\YieldNotReadyExtension",
        "Twig\\Extension\\OptimizerExtension",
    ] {
        assert!(env.has_extension(class), "{class}");
    }
}

#[test]
fn the_escaper_keeps_the_charset_of_its_first_use() {
    // VERIFIED with the oracle: Twig creates `EscaperRuntime` with the charset of the first escape and keeps it
    let context = ctx(&[("s", Value::String(PhpString::from(&b"\xe9<"[..])))]);
    let first = env(&[("index", "{{ s|e }}|{{ _charset }}")]);
    assert_eq!(
        first.render_bytes("index", context.clone()).unwrap(),
        b"\xef\xbf\xbd&lt;|UTF-8"
    );
    first.set_charset("ISO-8859-1");
    assert_eq!(
        first.render_bytes("index", context.clone()).unwrap(),
        b"\xef\xbf\xbd&lt;|ISO-8859-1"
    );
    let changed_before = env(&[("index", "{{ s|e }}|{{ _charset }}")]);
    changed_before.set_charset("ISO-8859-1");
    assert_eq!(
        changed_before.render_bytes("index", context).unwrap(),
        b"\xe9&lt;|ISO-8859-1"
    );
}

#[test]
fn auto_escaping_with_the_escaper_visitor() {
    // `EscaperExtension` does not register its visitor yet: add it as Twig's extension would. VERIFIED with the
    // oracle (`'autoescape' => 'name'`).
    let template = "{{ s }}|{{ s|raw }}|{{ s|e('js') }}|{{ m }}|{{ hm }}|{% autoescape 'js' %}{{ s }}|{{ hm }}\
                    {% endautoescape %}|{{ s|upper }}|{{ s|upper|raw }}|{% set c %}<i>{% endset %}{{ c }}|{{ 5 }}|\
                    {{ n }}|{{ s ~ 'x' }}|{% autoescape false %}{{ s }}{% endautoescape %}";
    let env = Environment::with_options(
        ArrayLoader::new([
            ("index.html", template),
            ("index.js", "{{ s }}"),
            ("index.txt", "{{ s }}"),
        ]),
        EnvironmentOptions {
            autoescape: DefaultStrategy::Strategy("name".to_owned()),
            ..Default::default()
        },
    );
    env.add_node_visitor(Arc::new(twig::visitor::EscaperNodeVisitor))
        .unwrap();
    let context = ctx(&[
        ("s", Value::from("<b>")),
        (
            "m",
            Value::Object(Markup::create(
                PhpString::from("<b>bold</b>"),
                Some("UTF-8".into()),
                None,
            )),
        ),
        (
            "hm",
            Value::Object(Markup::create(
                PhpString::from("<i>"),
                Some("UTF-8".into()),
                Some(vec![PhpString::from("html")]),
            )),
        ),
        ("n", Value::Null),
    ]);
    assert_eq!(
        env.render("index.html", context.clone()).unwrap(),
        "&lt;b&gt;|<b>|\\u003Cb\\u003E|<b>bold</b>|<i>|\\u003Cb\\u003E|\\u003Ci\\u003E|&lt;B&gt;|<B>|<i>|5||&lt;b&gt;x|<b>"
    );
    assert_eq!(
        env.render("index.js", context.clone()).unwrap(),
        "\\u003Cb\\u003E"
    );
    assert_eq!(env.render("index.txt", context).unwrap(), "<b>");
}
