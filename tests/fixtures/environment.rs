//! The environment `doIntegrationTest()` builds for `Twig\Tests\IntegrationTest` (TWIG/src/Test/IntegrationTestCase.php
//! and TWIG/tests/IntegrationTest.php, `reference/twig/test-suite.md` §3), in the same order:
//!
//! 1. `new Environment(new ArrayLoader($templates), array_merge(['cache' => false, 'strict_variables' => true],
//!    $config))`: the options come from the CONFIG tree of the corpus, resolved like `Environment::__construct()`
//!    by `twig-test-support` ([`options`]); the constructor registers Core, Escaper, YieldNotReady and Optimizer;
//! 2. `addGlobal('global', 'global')`;
//! 3. the runtime loaders: a `FactoryRuntimeLoader` creating `TwigTestLazyExtensionWithAttributes('+')`;
//! 4. the extensions: Debug, `Sandbox(new SecurityPolicy([], [], [], [], ['dump']), false)`, StringLoader,
//!    `TwigTestExtension`, `TwigTestExtensionWithAttributes('*')`,
//!    `AttributeExtension(TwigTestLazyExtensionWithAttributes::class)` (a later callable with the same name replaces
//!    an earlier one: the test `nl2br` replaces the core one);
//! 5. `getTwigFilters()`, `getTwigTests()`, `getTwigFunctions()` (empty);
//! 6. the undefined filter, function and test callbacks (the token-parser ones are empty).
//!
//! PHP warnings and notices become exceptions, as PHPUnit's `convertWarningsToExceptions` and
//! `convertNoticesToExceptions` make them ([`DiagnosticsPolicy::Error`]); deprecations are recorded.
//!
//! The classes the templates can name (`constant('Twig\\Tests\\TwigTestFoo::BAR_NAME')`, `enum(...)`) are registered
//! from `twig-test-support`'s class table: in PHP, loading IntegrationTest.php declares the helper classes and the
//! `Twig\Tests` enums are autoloaded.

use std::any::Any;
use std::sync::Arc;

use twig::escaper::DefaultStrategy;
use twig::extension::{
    DebugExtension, EscaperExtension, Extension, NodeVisitor, StringLoaderExtension,
};
use twig::loader::ArrayLoader;
use twig::runtime::{
    DiagnosticsPolicy, Environment, EnvironmentOptions, SandboxExtension, SecurityPolicy,
};
use twig::visitor::EscaperNodeVisitor;
use twig_test_support::{Autoescape, CacheOption};

use super::extension::{
    ATTRIBUTE_EXTENSION, AttributeCallables, AttributeRuntime, LAZY_ATTRIBUTE_RUNTIME,
    TwigTestExtension, undefined_filter_callback, undefined_function_callback,
    undefined_test_callback,
};
use super::parse::TemplateSection;

/// The options of a case: the CONFIG tree resolved by `twig-test-support` (Twig's defaults merged with the harness's
/// `cache => false, strict_variables => true` and CONFIG, then `Environment::__construct()`'s casts), with
/// warnings and notices turned into exceptions.
///
/// # Errors
///
/// An option value twig-rs cannot express (a callable `autoescape`, a compilation cache).
pub fn options(
    config: &twig_test_support::EnvironmentOptions,
) -> Result<EnvironmentOptions, String> {
    let autoescape = match &config.autoescape {
        Autoescape::Disabled => DefaultStrategy::Disabled,
        Autoescape::Strategy(name) => {
            DefaultStrategy::Strategy(name.to_string_lossy().into_owned())
        }
        // `'name'`: Twig replaces it with `FileExtensionEscapingStrategy::guess()`, which twig-rs selects by name
        Autoescape::FileExtension => DefaultStrategy::Strategy("name".to_owned()),
        Autoescape::Callable(value) => {
            return Err(format!("unsupported callable autoescape option {value:?}"));
        }
    };
    match &config.cache {
        CacheOption::Disabled => {}
        other => return Err(format!("unsupported cache option {other:?}")),
    }
    if !config.unknown.is_empty() {
        return Err(format!("unknown options {:?}", config.unknown));
    }
    Ok(EnvironmentOptions {
        debug: config.debug,
        charset: config.charset.to_string_lossy().into_owned(),
        strict_variables: config.strict_variables,
        autoescape,
        auto_reload: Some(config.auto_reload),
        optimizations: config.optimizations,
        use_yield: config.use_yield,
        diagnostics: DiagnosticsPolicy::Error,
        ..EnvironmentOptions::default()
    })
}

/// The environment of a case over its templates (with the case's padding), in the harness's registration order.
///
/// # Errors
///
/// A registration Twig would refuse (none for a fresh environment).
pub fn build(
    templates: &[TemplateSection],
    options: EnvironmentOptions,
) -> Result<Environment, String> {
    let loader = ArrayLoader::new(
        templates
            .iter()
            .map(|t| (t.name_lossy(), t.source.clone()))
            .collect::<Vec<_>>(),
    );
    let env = Environment::with_options(loader, options);
    let fail = |what: &str, error: twig::Error| format!("{what}: {error}");
    env.add_global("global", "global")
        .map_err(|e| fail("addGlobal", e))?;
    // `new FactoryRuntimeLoader([TwigTestLazyExtensionWithAttributes::class => static fn () => new
    // TwigTestLazyExtensionWithAttributes('+')])`
    env.add_runtime(LAZY_ATTRIBUTE_RUNTIME, Arc::new(AttributeRuntime::new("+")));
    if escaper_visitor_missing(&env) {
        env.add_extension(Arc::new(EscaperVisitorStopgap))
            .map_err(|e| fail("addExtension(stopgap)", e))?;
    }
    let policy = SecurityPolicy::new();
    policy.set_allowed_functions(["dump"]);
    let extensions: [Arc<dyn Extension>; 6] = [
        Arc::new(DebugExtension::new()),
        Arc::new(SandboxExtension::new(Arc::new(policy), false)),
        Arc::new(StringLoaderExtension::new()),
        Arc::new(TwigTestExtension),
        Arc::new(AttributeCallables {
            extension: ATTRIBUTE_EXTENSION,
            runtime: ATTRIBUTE_EXTENSION,
            prefix: "",
        }),
        Arc::new(AttributeCallables {
            extension: "Twig\\Extension\\AttributeExtension",
            runtime: LAZY_ATTRIBUTE_RUNTIME,
            prefix: "lazy_",
        }),
    ];
    for extension in extensions {
        let class = extension.class_name().to_owned();
        env.add_extension(extension)
            .map_err(|e| fail(&format!("addExtension({class})"), e))?;
    }
    // `Environment::getRuntime()` falls back to a registered extension that is an instance of the runtime class:
    // `TwigTestExtensionWithAttributes` is its own runtime
    env.add_runtime(ATTRIBUTE_EXTENSION, Arc::new(AttributeRuntime::new("*")));
    env.register_undefined_filter_callback(undefined_filter_callback());
    env.register_undefined_function_callback(undefined_function_callback());
    env.register_undefined_test_callback(undefined_test_callback());
    for class in twig_test_support::classes::fixture_class_table().iter() {
        if env.with_constants(|constants| constants.class(class.name()).is_none()) {
            env.add_class(class.clone())
                .map_err(|e| format!("cannot register class {}: {e}", class.name()))?;
        }
    }
    Ok(env)
}

/// Whether the `EscaperExtension` of the environment registers no node visitor. Twig's `getNodeVisitors()` returns
/// the `EscaperNodeVisitor`; twig-rs's extension does not yet (`src/extension/core.rs`), so the harness adds it.
fn escaper_visitor_missing(env: &Environment) -> bool {
    env.with_extensions(|set| {
        set.extension::<EscaperExtension>()
            .is_some_and(|escaper| escaper.node_visitors().is_empty())
    })
}

/// Registers the `EscaperNodeVisitor` where Twig's `EscaperExtension` does, as long as twig-rs's does not: right
/// after the extensions of the constructor, so that the visitor runs before the `SandboxNodeVisitor` of the same
/// priority, as in Twig. It switches itself off once the `EscaperExtension` registers the visitor.
struct EscaperVisitorStopgap;

impl Extension for EscaperVisitorStopgap {
    fn class_name(&self) -> &str {
        "TwigRs\\Fixtures\\EscaperNodeVisitorStopgap"
    }

    fn node_visitors(&self) -> Vec<Arc<dyn NodeVisitor>> {
        vec![Arc::new(EscaperNodeVisitor)]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
