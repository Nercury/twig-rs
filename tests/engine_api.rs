//! The environment API: options, extension lifecycle, globals, template cache, string templates, sinks, host
//! callables and the limits of the core engine. Expected messages come from Twig 3.31.1 (quoted from its sources and
//! checked with the oracle where noted).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use twig::deprecation::Deprecation;
use twig::escaper::DefaultStrategy;
use twig::extension::runtime::CallableRuntime;
use twig::extension::{
    CallableParameter, Extension, FilterOptions, FunctionOptions, PhpCallable, TwigFilter,
    TwigFunction,
};
use twig::loader::ArrayLoader;
use twig::loader::Loader;
use twig::runtime::{
    DiagnosticsPolicy, Environment, EnvironmentOptions, Markup, Notice, SandboxExtension,
    SecurityError, SecurityErrorKind, SecurityPolicy, SecurityPolicyInterface,
    SourcePolicyInterface, TemplateWrapper,
};
use twig::value::{Array, PhpString, Value};
use twig::value::{PhpError, ThrowableClass};
use twig::vm::Sink;
use twig::{ErrorKind, Source};

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
fn security_policies_throw_what_php_throws() {
    use twig::value::object::StdClass;
    use twig::value::{Diagnostic, Level, ThrowableClass};
    // TWIG/src/Sandbox/SecurityPolicy.php, VERIFIED with the oracle (tests/runtime_parity.rs p21-*, the P23 sweeps of
    // tests/upstream/runtime): an integer class key throws when the loop reaches it, and an `ArrayAccess` key that is
    // not a string fails `SecurityNotAllowedPropertyError::__construct()` after `sprintf()` warned
    let object = StdClass::new().into_ref();
    let policy = SecurityPolicy::new();
    policy.set_allowed_methods([("stdClass", vec!["foo"]), ("7", vec!["bar"])]);
    assert_eq!(policy.check_method_allowed(&object, b"FOO"), Ok(()));
    let error = policy.check_method_allowed(&object, b"bar").unwrap_err();
    assert_eq!(error.kind(), &SecurityErrorKind::Php);
    assert_eq!(error.class_name(), "Error");
    assert_eq!(
        error.message(),
        "Class name must be a valid object or a string"
    );
    assert_eq!(error.error_kind(), ErrorKind::Php);
    let converted = error.into_error();
    assert_eq!(converted.class_name(), "Error");
    assert!(SecurityError::of(&converted).is_none());
    // "07" and "-0" are no integer keys: unknown classes
    policy.set_allowed_methods([("07", vec!["bar"]), ("-0", vec!["bar"])]);
    assert!(matches!(
        policy
            .check_method_allowed(&object, b"bar")
            .unwrap_err()
            .kind(),
        SecurityErrorKind::Method { .. }
    ));

    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let key = Value::Array(Array::from_values([Value::from("x")]));
    let error = policy
        .check_property_allowed_with_diagnostics(&object, &key, &mut diagnostics)
        .unwrap_err();
    assert_eq!(
        diagnostics,
        [Diagnostic::new(
            Level::Warning,
            "Array to string conversion"
        )]
    );
    assert_eq!(
        error.php_error().map(|e| e.class().clone()),
        Some(ThrowableClass::TypeError)
    );
    assert_eq!(
        error.message(),
        "Twig\\Sandbox\\SecurityNotAllowedPropertyError::__construct(): Argument #3 ($propertyName) must be of type string, array given"
    );
    let error = policy
        .check_property_allowed(&object, &Value::Int(1))
        .unwrap_err();
    assert_eq!(
        error.kind(),
        &SecurityErrorKind::Property {
            class: "stdClass".to_owned(),
            property: "1".to_owned()
        }
    );
    assert_eq!(
        error.message(),
        "Calling \"1\" property on a \"stdClass\" object is not allowed."
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
    // the `EscaperExtension` registers the `EscaperNodeVisitor`. VERIFIED with the oracle (`'autoescape' => 'name'`).
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

// ---- PHP throwables that are not Twig errors ----------------------------------------------------------------------

/// A loader that fails like `FilesystemLoader` does on PHP warnings (an `ErrorException`) or like `realpath()` with a
/// NUL byte (a `ValueError`), for two template names.
struct ThrowingLoader;

impl twig::loader::Loader for ThrowingLoader {
    fn get_source_context(&self, name: &str) -> twig::Result<twig::Source> {
        use twig::value::{PhpError, ThrowableClass};
        match name {
            "broken" => Err(twig::Error::php(PhpError::new(
                ThrowableClass::ErrorException,
                "file_get_contents(x): Failed to open stream: No such file or directory",
            ))),
            "value" => Err(twig::Error::php(PhpError::value_error(
                "realpath(): Argument #1 ($path) must not contain any null bytes",
            ))),
            _ => {
                let code = match name {
                    "tag" => "a\n{% include 'broken' %}",
                    "tag_missing" => "a\n{% include 'broken' ignore missing %}",
                    "func" => "{{ include('broken') }}",
                    "func_missing" => "{{ include('broken', ignore_missing = true) }}",
                    "extends" => "{% extends 'broken' %}",
                    "embed" => "\n\n{% embed 'broken' %}{% endembed %}",
                    "import" => "{% import 'broken' as m %}{{ m.x() }}",
                    "tag_value" => "{% include 'value' %}",
                    "source" => "{{ source('broken') }}",
                    _ => "",
                };
                Ok(twig::Source::new(code, name))
            }
        }
    }

    fn get_cache_key(&self, name: &str) -> twig::Result<Vec<u8>> {
        Ok(name.as_bytes().to_vec())
    }

    fn is_fresh(&self, _name: &str, _time: i64) -> twig::Result<bool> {
        Ok(true)
    }

    fn exists(&self, _name: &str) -> bool {
        true
    }
}

#[test]
fn php_throwables_of_loaders_are_wrapped_by_templates() {
    // VERIFIED with the oracle: a LoaderInterface throwing \ErrorException / \ValueError, `$twig->render($name)`
    // printed as `get_class($e).': '.$e->getMessage()` plus the class of the previous exception
    let env = Environment::new(ThrowingLoader);
    let wrapped = |name: &str, line: u32, previous: &str, message: &str| {
        format!(
            "{name} => Twig\\Error\\RuntimeError: An exception has been thrown during the rendering of a template (\"{message}\") in \"{name}\" at line {line}. [previous {previous}]"
        )
    };
    let stream = "file_get_contents(x): Failed to open stream: No such file or directory";
    let null_bytes = "realpath(): Argument #1 ($path) must not contain any null bytes";
    let expected = [
        format!("broken => ErrorException: {stream}"),
        format!("value => ValueError: {null_bytes}"),
        wrapped("tag", 2, "ErrorException", stream),
        wrapped("tag_missing", 2, "ErrorException", stream),
        wrapped("func", 1, "ErrorException", stream),
        wrapped("func_missing", 1, "ErrorException", stream),
        wrapped("extends", 1, "ErrorException", stream),
        wrapped("embed", 3, "ErrorException", stream),
        wrapped("import", 1, "ErrorException", stream),
        wrapped("tag_value", 1, "ValueError", null_bytes),
        wrapped("source", 1, "ErrorException", stream),
    ];
    let names = [
        "broken",
        "value",
        "tag",
        "tag_missing",
        "func",
        "func_missing",
        "extends",
        "embed",
        "import",
        "tag_value",
        "source",
    ];
    for (name, expected) in names.into_iter().zip(expected) {
        let error = env.render(name, Array::new()).unwrap_err();
        let previous = error
            .previous()
            .and_then(|previous| previous.downcast_ref::<twig::value::PhpError>())
            .map(|previous| format!(" [previous {}]", previous.class().name()))
            .unwrap_or_default();
        assert_eq!(
            format!("{name} => {}: {error}{previous}", error.class_name()),
            expected
        );
    }
    let error = env.load("broken").unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Php);
    assert!(!error.kind().is_twig_error());
}

// ---- sandbox error classes ---------------------------------------------------------------------------------------

#[test]
fn sandbox_violations_have_their_error_kinds() {
    let (env, _) = sandboxed_env(
        &[("index", "{{ 'a'|lower }}"), ("tag", "{% set x = 1 %}")],
        &["upper"],
        true,
    );
    let error = env.render("index", Array::new()).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::SecurityNotAllowedFilter);
    assert_eq!(
        error.class_name(),
        "Twig\\Sandbox\\SecurityNotAllowedFilterError"
    );
    assert!(error.previous().is_none());
    let error = env.render("tag", Array::new()).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::SecurityNotAllowedTag);
    assert_eq!(
        SecurityError::of(&error).map(SecurityError::kind),
        Some(&SecurityErrorKind::Tag("set".to_owned()))
    );
}

// ---- the escaper runtime, the CoreExtension settings, getSourceContext(), mt_srand() ----------------------------

#[test]
fn the_escaper_runtime_is_shared_and_keeps_its_first_charset() {
    let env = Environment::new(ArrayLoader::new([("index", "{{ v }}|{{ v|e('rot') }}")]));
    env.set_charset("ISO-8859-1");
    let runtime = env.escaper_runtime();
    assert_eq!(runtime.charset(), PhpString::from("ISO-8859-1"));
    env.set_charset("UTF-8");
    assert!(Arc::ptr_eq(&runtime, &env.escaper_runtime()));
    assert_eq!(
        env.escaper_runtime().charset(),
        PhpString::from("ISO-8859-1")
    );
    runtime.set_escaper(
        "rot",
        Arc::new(|value, charset, _diag| {
            let text = match value {
                Value::String(s) => s.to_string_lossy().into_owned(),
                _ => String::new(),
            };
            Ok(Value::from(format!(
                "{}:{}",
                text.len(),
                charset.to_string_lossy()
            )))
        }),
    );
    let runtime_value = env
        .runtime("Twig\\Runtime\\EscaperRuntime")
        .expect("the escaper runtime");
    assert!(
        runtime_value
            .downcast_ref::<twig::extension::runtime::EscaperRuntime>()
            .is_some()
    );
    let mut context = Array::new();
    context.insert("v", Value::from("<b>"));
    assert_eq!(
        env.render("index", context).unwrap(),
        "&lt;b&gt;|3:ISO-8859-1"
    );
}

#[test]
fn core_settings_configure_the_date_and_number_filters() {
    // `setDateFormat('d/m/Y', '%h hours')`, `setNumberFormat(2, ',', ' ')`, `setTimezone('Asia/Tokyo')`
    let env = Environment::new(ArrayLoader::new([(
        "index",
        "{{ 0|date }}|{{ 1234.5|number_format }}|{{ 0|date('H:i e') }}",
    )]));
    let settings = env.core_settings();
    settings.set_date_format(Some(Value::from("d/m/Y")), Some(Value::from("%h hours")));
    settings.set_number_format(Value::Int(2), Value::from(","), Value::from(" "));
    let mut diagnostics = Vec::new();
    settings
        .set_timezone(
            env.date_context(),
            &Value::from("Asia/Tokyo"),
            &mut diagnostics,
        )
        .unwrap();
    assert_eq!(
        env.render("index", Array::new()).unwrap(),
        "01/01/1970|1 234,50|09:00 Asia/Tokyo"
    );
    let core_format = env.with_extensions(|set| {
        set.extension::<twig::extension::CoreExtension>()
            .map(|core| core.date_format())
    });
    assert_eq!(
        core_format,
        Some([Value::from("d/m/Y"), Value::from("%h hours")])
    );
}

#[test]
fn template_wrappers_return_their_source_context() {
    // VERIFIED with the oracle (misc vectors string_loader-021..023): the code is '' unless debug is on
    let templates = [(
        "index",
        "{% set t = template_from_string('a{{ x }}b') %}[{{ t.sourceContext.code }}]",
    )];
    let env = Environment::new(ArrayLoader::new(templates));
    env.add_extension(Arc::new(twig::extension::StringLoaderExtension::new()))
        .unwrap();
    assert_eq!(env.render("index", Array::new()).unwrap(), "[]");
    let debug = Environment::with_options(
        ArrayLoader::new(templates),
        EnvironmentOptions {
            debug: true,
            ..Default::default()
        },
    );
    debug
        .add_extension(Arc::new(twig::extension::StringLoaderExtension::new()))
        .unwrap();
    assert_eq!(debug.render("index", Array::new()).unwrap(), "[a{{ x }}b]");
}

#[test]
fn mt_srand_seeds_random_and_shuffle() {
    // VERIFIED with the oracle: mt_srand(42); random(100), random(['a', 'b', 'c']), random()
    let env = env(&[(
        "index",
        "{{ random(100) }} {{ random(['a', 'b', 'c']) }} {{ random() }}",
    )]);
    twig::extension::runtime::mt_srand(42);
    assert_eq!(
        env.render("index", Array::new()).unwrap(),
        "38 c 2041643438"
    );
}

#[test]
fn literal_enum_names_are_checked_when_compiling() {
    let env = env(&[
        ("missing", "\n{{ enum('Nope') }}"),
        ("type", "{{ enum_cases(13) }}"),
        (
            "rounding",
            "{{ enum('RoundingMode').name }}|{{ enum_cases('\\\\RoundingMode')|length }}",
        ),
    ]);
    let error = env.load("missing").unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Syntax);
    assert_eq!(
        error.to_string(),
        "The first argument of the \"enum\" function must be the name of an enum, \"Nope\" given in \"missing\" at line 2."
    );
    assert_eq!(
        env.load("type").unwrap_err().to_string(),
        "The first argument of the \"enum_cases\" function must be a string in \"type\" at line 1."
    );
    assert_eq!(
        env.render("rounding", Array::new()).unwrap(),
        "HalfAwayFromZero|8"
    );
}

// ---- compiled sources, class keys, runtimes (plans/parity-fixes.md P24, P25, P26) ------------------------------

/// The source a failed render reports: kind, template name, code and line.
fn failure(env: &Environment, name: &str) -> (ErrorKind, String, String, i64) {
    let error = env.render(name, Array::new()).unwrap_err();
    let source = error.source_context().expect("a source");
    (
        error.kind(),
        source.name().to_owned(),
        String::from_utf8_lossy(source.code()).into_owned(),
        error.template_line(),
    )
}

#[test]
fn runtime_errors_carry_the_compiled_source() {
    // VERIFIED with the oracle (`$e->getSourceContext()->getCode()` after `$twig->render()`, and
    // `$twig->load($name)->getSourceContext()->getCode()`): a runtime error gets the source of the compiled template
    // (`ModuleNode::compileGetSourceContext()`), whose code is '' unless the template was compiled with debug on;
    // syntax errors keep the loader's source
    let templates = [
        ("index", "a\n{{ foo.bar }}"),
        ("include", "x{% include 'b' %}"),
        ("b", "b\n{{ foo.bar }}"),
        ("missing", "x\n{% include 'nope' %}"),
        ("wrapped", "x\n{{ 1 // 0 }}"),
        ("syntax", "x\n{{ foo( }}"),
        ("block", "{{ block('nope') }}"),
        ("sandboxed", "x\n{{ 'a'|upper }}"),
    ];
    let make = |debug: bool| {
        Environment::with_options(
            ArrayLoader::new(templates),
            EnvironmentOptions {
                debug,
                strict_variables: true,
                ..Default::default()
            },
        )
    };
    let env = make(false);
    let expected = [
        ("index", (ErrorKind::Runtime, "index", "", 2)),
        ("include", (ErrorKind::Runtime, "b", "", 2)),
        ("missing", (ErrorKind::Loader, "missing", "", 2)),
        ("wrapped", (ErrorKind::Runtime, "wrapped", "", 2)),
        ("syntax", (ErrorKind::Syntax, "syntax", "x\n{{ foo( }}", 2)),
        ("block", (ErrorKind::Runtime, "block", "", 1)),
    ];
    for (name, (kind, source, code, line)) in expected {
        assert_eq!(
            failure(&env, name),
            (kind, source.to_owned(), code.to_owned(), line),
            "{name}"
        );
    }
    assert_eq!(env.load("index").unwrap().source_context().code(), b"");
    let template = env.create_template(b"{{ foo }}", None).unwrap();
    let error = template.render(Array::new()).unwrap_err();
    assert_eq!(
        error.source_context().map(|s| s.code().to_vec()),
        Some(Vec::new())
    );
    assert_eq!(template.source_context().code(), b"");

    let debug = make(true);
    assert_eq!(
        failure(&debug, "index"),
        (
            ErrorKind::Runtime,
            "index".to_owned(),
            "a\n{{ foo.bar }}".to_owned(),
            2
        )
    );
    assert_eq!(
        debug.load("index").unwrap().source_context().code(),
        b"a\n{{ foo.bar }}"
    );

    // a violation of a template sandboxed by the extension: '' without debug, the code with it
    for (debug, code) in [(false, ""), (true, "x\n{{ 'a'|upper }}")] {
        let env = Environment::with_options(
            ArrayLoader::new(templates),
            EnvironmentOptions {
                debug,
                ..Default::default()
            },
        );
        let policy = SecurityPolicy::new();
        policy.set_strict(true);
        env.add_extension(Arc::new(SandboxExtension::new(Arc::new(policy), true)))
            .unwrap();
        assert_eq!(
            failure(&env, "sandboxed"),
            (
                ErrorKind::SecurityNotAllowedFilter,
                "sandboxed".to_owned(),
                code.to_owned(),
                2
            ),
            "debug={debug}"
        );
    }
}

#[test]
fn a_changed_default_strategy_compiles_templates_again() {
    // VERIFIED with the oracle: `{{ v }}|{% include "b" %}` renders `&lt;|&lt;`, then `<|<` after
    // `$twig->getExtension(EscaperExtension::class)->setDefaultStrategy(false)` (Environment::getTemplateClass() adds
    // the strategy resolved for the name to the class key)
    let env = Environment::new(ArrayLoader::new([
        ("index", "{{ v }}|{% include \"b\" %}"),
        ("b", "{{ v }}"),
    ]));
    let context = ctx(&[("v", Value::from("<"))]);
    assert_eq!(env.render("index", context.clone()).unwrap(), "&lt;|&lt;");
    env.with_extensions(|set| {
        set.extension::<twig::extension::EscaperExtension>()
            .unwrap()
            .set_default_strategy(DefaultStrategy::Disabled)
    });
    assert_eq!(env.render("index", context.clone()).unwrap(), "<|<");
    env.with_extensions(|set| {
        set.extension::<twig::extension::EscaperExtension>()
            .unwrap()
            .set_default_strategy(DefaultStrategy::html())
    });
    assert_eq!(env.render("index", context).unwrap(), "&lt;|&lt;");
}

#[test]
fn default_strategy_callbacks_run_when_a_template_class_is_resolved() {
    use twig::ast::ConstantValue;
    // VERIFIED with the oracle: the callback runs once per template name, in load order (`index,b`), and its
    // exception comes out of `load()` unwrapped (`RuntimeException: cb index`, no previous exception; a Twig
    // `RuntimeError` keeps line -1 and no source)
    let names = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&names);
    let env = Environment::with_options(
        ArrayLoader::new([("index", "{% include \"b\" %}{{ v }}"), ("b", "{{ v }}")]),
        EnvironmentOptions {
            autoescape: DefaultStrategy::Callback(Arc::new(move |name: &str| {
                seen.lock().unwrap().push(name.to_owned());
                Ok(ConstantValue::String(b"html".to_vec()))
            })),
            ..Default::default()
        },
    );
    let context = ctx(&[("v", Value::from("<"))]);
    let mut output = env.render("index", context.clone()).unwrap();
    output.push_str(&env.render("index", context).unwrap());
    assert_eq!(output, "&lt;&lt;&lt;&lt;");
    assert_eq!(*names.lock().unwrap(), ["index", "b"]);

    let throwing = Environment::with_options(
        ArrayLoader::new([("index", "{{ 1 }}")]),
        EnvironmentOptions {
            autoescape: DefaultStrategy::Callback(Arc::new(|name: &str| {
                Err(twig::Error::php(twig::value::PhpError::new(
                    twig::value::ThrowableClass::RuntimeException,
                    format!("cb {name}"),
                )))
            })),
            ..Default::default()
        },
    );
    let error = throwing.render("index", Array::new()).unwrap_err();
    assert_eq!(
        format!("{}: {error}", error.class_name()),
        "RuntimeException: cb index"
    );
    assert!(error.previous().is_none());

    let twig_error = Environment::with_options(
        ArrayLoader::new([("index", "{{ 1 }}")]),
        EnvironmentOptions {
            autoescape: DefaultStrategy::Callback(Arc::new(|name: &str| {
                Err(twig::Error::runtime(format!("cb {name}")))
            })),
            ..Default::default()
        },
    );
    let error = twig_error.render("index", Array::new()).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Runtime);
    assert_eq!(error.to_string(), "cb index");
    assert_eq!(error.template_line(), -1);
    assert!(error.source_context().is_none());
}

#[test]
fn the_sandbox_signature_initializes_the_extensions_when_loading() {
    // VERIFIED with the oracle (tests/vectors/visitors/optimizer_mode_env.json, opt-env-m16-sbx-*): with the
    // SandboxExtension, getTemplateClass()'s getSandboxSignature() initializes the extensions, so `load()` throws the
    // OptimizerNodeVisitor's InvalidArgumentException unwrapped (even before a syntax error); without it,
    // compileSource() wraps it
    for (template, sandbox) in [("{{ 1 }}", true), ("{{ 1 + }}", true), ("{{ 1 }}", false)] {
        let env = Environment::with_options(
            ArrayLoader::new([("index", template)]),
            EnvironmentOptions {
                optimizations: 16,
                ..Default::default()
            },
        );
        if sandbox {
            env.add_extension(Arc::new(SandboxExtension::default()))
                .unwrap();
        }
        let error = env.load("index").unwrap_err();
        let shown = format!("{}: {error}", error.class_name());
        if sandbox {
            assert_eq!(
                shown, "InvalidArgumentException: Optimizer mode \"16\" is not valid.",
                "{template}"
            );
        } else {
            assert_eq!(
                shown,
                "Twig\\Error\\SyntaxError: An exception has been thrown during the compilation of a template (\"Optimizer mode \"16\" is not valid.\") in \"index\".",
                "{template}"
            );
        }
    }
}

#[test]
fn runtimes_fall_back_to_registered_extensions() {
    // VERIFIED with the oracle: `$twig->getRuntime(StringLoaderExtension::class) === $extension` once the extension is
    // registered, and every call returns the same object
    let env = env(&[]);
    let extension: Arc<dyn Extension> = Arc::new(Late);
    env.add_extension(Arc::clone(&extension)).unwrap();
    let runtime = env.runtime("App\\LateExtension").unwrap();
    let held = runtime
        .downcast_ref::<Arc<dyn Extension>>()
        .expect("the extension");
    assert!(Arc::ptr_eq(held, &extension));
    assert!(held.as_any().downcast_ref::<Late>().is_some());
    assert!(Arc::ptr_eq(
        &runtime,
        &env.runtime("\\App\\LateExtension").unwrap()
    ));
    assert_eq!(
        env.runtime("App\\Missing").unwrap_err().to_string(),
        "Unable to load the \"App\\Missing\" runtime."
    );
}

// ---- lazily resolved source paths (P24) -------------------------------------------------------------------------

/// A loader serving `templates` with the paths `<prefix><name>` and recording the names it is asked for; when it is
/// down, it throws `RuntimeException('The storage is down.')` for the templates it knows. Clones share the state.
#[derive(Clone)]
struct RecordingLoader {
    templates: Arc<Vec<(&'static str, &'static str)>>,
    prefix: &'static str,
    down: Arc<AtomicBool>,
    reads: Arc<Mutex<Vec<String>>>,
}

impl RecordingLoader {
    fn new(templates: &[(&'static str, &'static str)], prefix: &'static str) -> Self {
        RecordingLoader {
            templates: Arc::new(templates.to_vec()),
            prefix,
            down: Arc::default(),
            reads: Arc::default(),
        }
    }

    fn go_down(&self) {
        self.down.store(true, Ordering::SeqCst);
    }

    /// The names asked so far, comma-separated (`implode(',', $loader->reads)`).
    fn reads(&self) -> String {
        self.reads.lock().unwrap().join(",")
    }
}

impl Loader for RecordingLoader {
    fn get_source_context(&self, name: &str) -> twig::Result<Source> {
        self.reads.lock().unwrap().push(name.to_owned());
        let Some((_, code)) = self.templates.iter().find(|(known, _)| *known == name) else {
            return Err(twig::Error::loader(format!(
                "Template \"{name}\" is not defined."
            )));
        };
        if self.down.load(Ordering::SeqCst) {
            return Err(twig::Error::php(PhpError::new(
                ThrowableClass::RuntimeException,
                "The storage is down.",
            )));
        }
        Ok(Source::with_path(
            *code,
            name,
            format!("{}{name}", self.prefix),
        ))
    }

    fn get_cache_key(&self, name: &str) -> twig::Result<Vec<u8>> {
        Ok(name.as_bytes().to_vec())
    }

    fn is_fresh(&self, _name: &str, _time: i64) -> twig::Result<bool> {
        Ok(true)
    }

    fn exists(&self, name: &str) -> bool {
        self.templates.iter().any(|(known, _)| *known == name)
    }
}

fn strict() -> EnvironmentOptions {
    EnvironmentOptions {
        strict_variables: true,
        ..options()
    }
}

/// `get_class($e).': '.$e->getMessage()` and `$e->getSourceContext()?->getPath()`.
fn error_and_path(result: twig::Result<String>) -> (String, String) {
    let error = result.unwrap_err();
    let path = error
        .source_context()
        .map(|source| source.path().to_owned())
        .unwrap_or_default();
    (format!("{}: {error}", error.class_name()), path)
}

// The scenarios of the next tests ran in the oracle (`php_code` cases, a PHP process each, with a PHP port of
// `RecordingLoader`); the expectations are its results.

#[test]
fn errors_resolve_the_lazy_path_of_their_template_when_they_get_its_source() {
    // the ignored LoaderError of `include ... ignore missing` gets the template's source (`Template::load()`), and its
    // `updateRepr()` asks the loader for the path while the loader works
    let loader = RecordingLoader::new(
        &[(
            "index.twig",
            "{% include 'missing.twig' ignore missing %}{{ foo }}",
        )],
        "/path/to/",
    );
    let twig = Environment::with_options(loader.clone(), strict());
    let template = twig.load("index.twig").unwrap();
    assert_eq!(loader.reads(), "index.twig");
    assert_eq!(
        template.render(ctx(&[("foo", Value::from("x"))])).unwrap(),
        "x"
    );
    assert_eq!(loader.reads(), "index.twig,missing.twig,index.twig");
    loader.go_down();
    assert_eq!(
        error_and_path(template.render(Array::new())),
        (
            "Twig\\Error\\RuntimeError: Variable \"foo\" does not exist in \"index.twig\" at line 1."
                .to_owned(),
            "/path/to/index.twig".to_owned()
        )
    );
    assert_eq!(
        loader.reads(),
        "index.twig,missing.twig,index.twig,missing.twig"
    );
}

#[test]
fn ignored_loader_errors_of_functions_leave_the_lazy_path_alone() {
    // `include()` and `source()` catch the LoaderError before any template gives it its source
    let loader = RecordingLoader::new(
        &[(
            "index.twig",
            "{{ include('missing.twig', ignore_missing = true) }}{{ source('missing.twig', ignore_missing = true) }}{{ foo }}",
        )],
        "/path/to/",
    );
    let twig = Environment::with_options(loader.clone(), strict());
    let template = twig.load("index.twig").unwrap();
    assert_eq!(
        template.render(ctx(&[("foo", Value::from("x"))])).unwrap(),
        "x"
    );
    assert_eq!(loader.reads(), "index.twig,missing.twig,missing.twig");
    loader.go_down();
    assert_eq!(
        error_and_path(template.render(Array::new())),
        (
            "Twig\\Error\\RuntimeError: Variable \"foo\" does not exist in \"index.twig\" at line 1."
                .to_owned(),
            String::new()
        )
    );
    assert_eq!(
        loader.reads(),
        "index.twig,missing.twig,missing.twig,missing.twig,missing.twig,index.twig"
    );
}

#[test]
fn embedded_templates_resolve_their_lazy_path_on_their_own() {
    let loader = RecordingLoader::new(
        &[
            (
                "index.twig",
                "{{ foo }}{% embed \"layout.twig\" %}{% block content %}{{ bar }}{% endblock %}{% endembed %}",
            ),
            ("layout.twig", "{% block content %}{% endblock %}"),
        ],
        "/path/to/",
    );
    let twig = Environment::with_options(loader.clone(), strict());
    let template = twig.load("index.twig").unwrap();
    assert_eq!(
        error_and_path(template.render(Array::new())),
        (
            "Twig\\Error\\RuntimeError: Variable \"foo\" does not exist in \"index.twig\" at line 1."
                .to_owned(),
            "/path/to/index.twig".to_owned()
        )
    );
    assert_eq!(loader.reads(), "index.twig,index.twig");
    loader.go_down();
    // the embedded template wraps the loader's exception with its own source, whose path is not resolved yet
    assert_eq!(
        error_and_path(template.render(ctx(&[("foo", Value::Int(1))]))),
        (
            "Twig\\Error\\RuntimeError: An exception has been thrown during the rendering of a template (\"The storage is down.\") in \"index.twig\" at line 1.".to_owned(),
            String::new()
        )
    );
    assert_eq!(
        loader.reads(),
        "index.twig,index.twig,layout.twig,index.twig"
    );
}

#[test]
fn templates_resolve_their_lazy_path_with_the_loader_of_their_instantiation() {
    let templates: &[(&'static str, &'static str)] = &[
        (
            "index.twig",
            "{% embed \"layout.twig\" %}{% block content %}{{ bar }}{% endblock %}{% endembed %}{{ foo }}",
        ),
        ("layout.twig", "{% block content %}{% endblock %}"),
    ];
    let loader = RecordingLoader::new(templates, "/path/to/");
    let twig = Environment::with_options(loader.clone(), strict());
    let template = twig.load("index.twig").unwrap();
    let other = RecordingLoader::new(templates, "/other/");
    twig.set_loader(other.clone());
    // the embedded template is instantiated after `setLoader()`, the main template before
    assert_eq!(
        error_and_path(template.render(ctx(&[("foo", Value::Int(1))]))),
        (
            "Twig\\Error\\RuntimeError: Variable \"bar\" does not exist in \"index.twig\" at line 1."
                .to_owned(),
            "/other/index.twig".to_owned()
        )
    );
    assert_eq!(
        error_and_path(template.render(ctx(&[("bar", Value::Int(1))]))),
        (
            "Twig\\Error\\RuntimeError: Variable \"foo\" does not exist in \"index.twig\" at line 1."
                .to_owned(),
            "/path/to/index.twig".to_owned()
        )
    );
    assert_eq!(loader.reads(), "index.twig,index.twig");
    assert_eq!(other.reads(), "layout.twig,index.twig");

    let loader = RecordingLoader::new(templates, "/path/to/");
    let twig = Environment::with_options(loader.clone(), strict());
    let template = twig.load("index.twig").unwrap();
    twig.set_loader(ArrayLoader::new(Vec::<(String, String)>::new()));
    assert_eq!(
        error_and_path(template.render(ctx(&[("foo", Value::Int(1))]))),
        (
            "Twig\\Error\\LoaderError: Template \"layout.twig\" is not defined in \"index.twig\" at line 1."
                .to_owned(),
            String::new()
        )
    );
    assert_eq!(loader.reads(), "index.twig");
}

#[test]
fn template_wrappers_in_templates_resolve_the_lazy_path_in_get_path() {
    for (code, output, path) in [
        (
            "{{ t.getSourceContext().getName() }}|{{ foo }}",
            "index.twig|1",
            "",
        ),
        (
            "{{ t.getSourceContext().getPath() }}|{{ foo }}",
            "/path/to/index.twig|1",
            "/path/to/index.twig",
        ),
    ] {
        let loader = RecordingLoader::new(&[("index.twig", code)], "/path/to/");
        let twig = Environment::with_options(loader.clone(), strict());
        let template = twig.load("index.twig").unwrap();
        let t = template.to_value();
        assert_eq!(
            template
                .render(ctx(&[("t", t.clone()), ("foo", Value::Int(1))]))
                .unwrap(),
            output
        );
        let reads = if path.is_empty() {
            "index.twig"
        } else {
            "index.twig,index.twig"
        };
        assert_eq!(loader.reads(), reads, "{code}");
        loader.go_down();
        assert_eq!(
            error_and_path(template.render(ctx(&[("t", t)]))),
            (
                "Twig\\Error\\RuntimeError: Variable \"foo\" does not exist in \"index.twig\" at line 1."
                    .to_owned(),
                path.to_owned()
            ),
            "{code}"
        );
        assert_eq!(loader.reads(), "index.twig,index.twig", "{code}");
    }
}

#[test]
fn debug_templates_keep_the_path_and_the_code_of_their_compilation() {
    let loader = RecordingLoader::new(&[("index.twig", "{{ foo }}")], "/path/to/");
    let twig = Environment::with_options(
        loader.clone(),
        EnvironmentOptions {
            debug: true,
            ..strict()
        },
    );
    let template = twig.load("index.twig").unwrap();
    loader.go_down();
    let error = template.render(Array::new()).unwrap_err();
    assert_eq!(
        error.to_string(),
        "Variable \"foo\" does not exist in \"index.twig\" at line 1."
    );
    let source = error.source_context().unwrap();
    assert_eq!(source.path(), "/path/to/index.twig");
    assert_eq!(source.code(), b"{{ foo }}");
    assert_eq!(loader.reads(), "index.twig");
}
