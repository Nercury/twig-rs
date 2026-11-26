//! Port of `TWIG/tests/FactoryRuntimeLoaderTest.php`, plus the runtime loader behaviour of `Environment::getRuntime()`
//! the oracle reports (`tests/vectors/upstream/runtime/host_api.json`: `runtime-factory-cached`,
//! `runtime-loader-null-falls-back`, `runtime-factory-throws`, `escaper-factory-*`).

use std::any::Any;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use twig::extension::Extension;
use twig::extension::runtime::EscaperRuntime;
use twig::runtime::{FactoryRuntimeLoader, RuntimeLoader};
use twig::value::{Array, PhpError, ThrowableClass};
use twig::{Environment, ErrorKind};

use super::support::loader;

/// `new \stdClass()`, the runtime of the upstream test (`'\Twig\Tests\getRuntime'`).
struct StdClass;

#[test]
fn test_load() {
    let loader = FactoryRuntimeLoader::new().factory("stdClass", || Ok(Arc::new(StdClass)));
    let runtime = loader.load("stdClass").unwrap().expect("a runtime");
    assert!(runtime.downcast_ref::<StdClass>().is_some());
}

#[test]
fn test_load_returns_null_for_unmapped_runtime() {
    let loader = FactoryRuntimeLoader::new();
    assert!(loader.load("stdClass").unwrap().is_none());
}

/// `FactoryRuntimeLoader::load()` calls the factory every time and matches the class exactly; the environment keeps
/// the first runtime it gets.
#[test]
fn factories_run_on_every_load_and_the_environment_keeps_the_runtime() {
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    let factories = FactoryRuntimeLoader::new().factory("stdClass", || {
        CALLS.fetch_add(1, Ordering::SeqCst);
        Ok(Arc::new(StdClass))
    });
    let a = factories.load("stdClass").unwrap().unwrap();
    let b = factories.load("stdClass").unwrap().unwrap();
    assert!(!Arc::ptr_eq(&a, &b));
    assert_eq!(CALLS.load(Ordering::SeqCst), 2);
    assert!(factories.load("\\stdClass").unwrap().is_none());

    let env = Environment::new(loader(&[]));
    env.add_runtime_loader(Arc::new(factories));
    let first = env.runtime("stdClass").unwrap();
    assert!(Arc::ptr_eq(&first, &env.runtime("stdClass").unwrap()));
    assert_eq!(CALLS.load(Ordering::SeqCst), 3);
}

/// The runtime loaders are asked in order, the first runtime wins, `None` asks the next one, and the registered
/// extension of the class comes after the loaders.
#[test]
fn runtime_loaders_are_asked_in_order_before_the_extensions() {
    struct ExtRt;

    impl Extension for ExtRt {
        fn class_name(&self) -> &str {
            "ExtRt"
        }

        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    let env = Environment::new(loader(&[]));
    env.add_extension(Arc::new(ExtRt)).unwrap();
    env.add_runtime_loader(Arc::new(FactoryRuntimeLoader::new()));
    let runtime = env.runtime("ExtRt").unwrap();
    assert!(runtime.downcast_ref::<Arc<dyn Extension>>().is_some());
    let error = env.runtime("Nope").unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Runtime);
    assert_eq!(error.to_string(), "Unable to load the \"Nope\" runtime.");

    let env = Environment::new(loader(&[]));
    env.add_runtime_loader(Arc::new(
        FactoryRuntimeLoader::new().factory("A", || Ok(Arc::new("first"))),
    ));
    env.add_runtime_loader(Arc::new(
        FactoryRuntimeLoader::new()
            .factory("A", || Ok(Arc::new("second")))
            .factory("B", || Ok(Arc::new("b"))),
    ));
    assert_eq!(
        env.runtime("A").unwrap().downcast_ref::<&str>(),
        Some(&"first")
    );
    assert_eq!(env.runtime("B").unwrap().downcast_ref::<&str>(), Some(&"b"));
}

/// A factory that throws: `getRuntime()` throws its exception and asks again next time.
#[test]
fn a_failing_factory_is_asked_again() {
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    let env = Environment::new(loader(&[]));
    env.add_runtime_loader(Arc::new(FactoryRuntimeLoader::new().factory(
        "MyRuntime",
        || -> twig::Result<Arc<StdClass>> {
            CALLS.fetch_add(1, Ordering::SeqCst);
            Err(twig::Error::php(PhpError::new(
                ThrowableClass::LogicException,
                "No runtime.",
            )))
        },
    )));
    for _ in 0..2 {
        let error = env.runtime("MyRuntime").unwrap_err();
        assert_eq!(
            (error.class_name(), error.to_string().as_str()),
            ("LogicException", "No runtime.")
        );
    }
    assert_eq!(CALLS.load(Ordering::SeqCst), 2);
}

/// The escaper runtime comes from the runtime loaders too; a failing factory makes loading a template that escapes
/// fail with its exception, as the compiled constructor fetches the runtime (oracle: `escaper-factory-throws`).
#[test]
fn the_escaper_runtime_comes_from_the_runtime_loaders() {
    let env = Environment::new(loader(&[
        ("index", "{{ \"<\" }}"),
        ("raw", "{{ \"<\"|raw }}"),
        (
            "explicit",
            "{% autoescape false %}{{ '<'|e }}{% endautoescape %}",
        ),
    ]));
    env.add_runtime_loader(Arc::new(FactoryRuntimeLoader::new().factory(
        "Twig\\Runtime\\EscaperRuntime",
        || -> twig::Result<Arc<EscaperRuntime>> {
            Err(twig::Error::php(PhpError::new(
                ThrowableClass::LogicException,
                "No escaper.",
            )))
        },
    )));
    assert_eq!(env.render("raw", Array::new()).unwrap(), "<");
    assert_eq!(env.render("index", Array::new()).unwrap(), "<");
    let error = env.render("explicit", Array::new()).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Logic);
    assert_eq!(error.to_string(), "No escaper.");
}
