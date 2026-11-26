//! Runtime loaders: Twig's `RuntimeLoaderInterface` and `FactoryRuntimeLoader` (TWIG/src/RuntimeLoader/), which
//! create the runtime objects of filters, functions and tests when they are first needed.

use std::any::Any;
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use crate::error::Result;

/// A runtime object, as [`Environment::runtime`](super::Environment::runtime) returns it.
pub type Runtime = Arc<dyn Any + Send + Sync>;

/// Creates the runtime objects of runtime classes, for [`Environment::runtime`](super::Environment::runtime). Twig:
/// `Twig\RuntimeLoader\RuntimeLoaderInterface` (TWIG/src/RuntimeLoader/RuntimeLoaderInterface.php).
///
/// [`Environment::add_runtime_loader`](super::Environment::add_runtime_loader) registers a loader. The environment
/// asks its loaders in the order they were added, the first time a runtime class is needed, and keeps the first
/// runtime one of them creates; a loader that does not know the class answers `None` and the next one is asked.
pub trait RuntimeLoader: Send + Sync + 'static {
    /// The runtime object of `class`, or `None` when this loader does not know how to create it. Twig:
    /// `load(string $class)`.
    ///
    /// # Errors
    ///
    /// What creating the runtime throws; [`Environment::runtime`](super::Environment::runtime) passes it on, and
    /// the runtime is asked for again next time.
    fn load(&self, class: &str) -> Result<Option<Runtime>>;
}

/// A runtime factory of a [`FactoryRuntimeLoader`].
type Factory = dyn Fn() -> Result<Runtime> + Send + Sync;

/// Runtimes made by factories when they are first needed. Twig: `Twig\RuntimeLoader\FactoryRuntimeLoader`
/// (TWIG/src/RuntimeLoader/FactoryRuntimeLoader.php).
///
/// Each [`load`](RuntimeLoader::load) of a class calls its factory again (the environment keeps the runtime it got
/// first); classes are matched exactly, as PHP array keys (`App\Runtime`, not `\App\Runtime`).
///
/// ```
/// use std::sync::Arc;
/// use std::sync::atomic::{AtomicUsize, Ordering};
/// use twig::extension::runtime::CallableRuntime;
/// use twig::extension::{FunctionOptions, PhpCallable, TwigFunction};
/// use twig::runtime::{Environment, FactoryRuntimeLoader};
/// use twig::{Array, Value, loader::ArrayLoader};
///
/// /// An expensive service: created only when a template calls `site_name()`.
/// struct SiteRuntime {
///     name: String,
/// }
///
/// static CREATED: AtomicUsize = AtomicUsize::new(0);
///
/// let env = Environment::new(ArrayLoader::new([
///     ("plain", "no runtime here"),
///     ("index", "{{ site_name() }} / {{ site_name() }}"),
/// ]));
/// // TwigFunction('site_name', [SiteRuntime::class, 'getName'])
/// let callable = PhpCallable::method("App\\SiteRuntime", "getName", Vec::new());
/// CallableRuntime::new(|call| {
///     let runtime = call.env().runtime("App\\SiteRuntime")?;
///     let runtime = runtime.downcast_ref::<SiteRuntime>().expect("the runtime of App\\SiteRuntime");
///     Ok(Value::from(runtime.name.as_str()))
/// })
/// .attach(&callable);
/// env.add_function(TwigFunction::new("site_name", Some(callable), FunctionOptions::default()))?;
/// env.add_runtime_loader(Arc::new(FactoryRuntimeLoader::new().factory("App\\SiteRuntime", || {
///     CREATED.fetch_add(1, Ordering::SeqCst);
///     Ok(Arc::new(SiteRuntime { name: "Example".to_owned() }))
/// })));
///
/// assert_eq!(env.render("plain", Array::new())?, "no runtime here");
/// assert_eq!(CREATED.load(Ordering::SeqCst), 0);
/// assert_eq!(env.render("index", Array::new())?, "Example / Example");
/// assert_eq!(CREATED.load(Ordering::SeqCst), 1);
/// # Ok::<(), twig::Error>(())
/// ```
#[derive(Clone, Default)]
pub struct FactoryRuntimeLoader {
    map: HashMap<String, Arc<Factory>>,
}

impl fmt::Debug for FactoryRuntimeLoader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut classes: Vec<&String> = self.map.keys().collect();
        classes.sort();
        f.debug_struct("FactoryRuntimeLoader")
            .field("classes", &classes)
            .finish()
    }
}

impl FactoryRuntimeLoader {
    /// A loader without factories (`new FactoryRuntimeLoader()`).
    pub fn new() -> Self {
        FactoryRuntimeLoader::default()
    }

    /// Adds the factory of the runtime class `class` (an entry `$class => $factory` of Twig's map; a class given
    /// twice keeps the last factory).
    pub fn factory<R: Any + Send + Sync>(
        mut self,
        class: &str,
        factory: impl Fn() -> Result<Arc<R>> + Send + Sync + 'static,
    ) -> Self {
        let factory = move || factory().map(|runtime| runtime as Runtime);
        self.map.insert(class.to_owned(), Arc::new(factory));
        self
    }
}

impl RuntimeLoader for FactoryRuntimeLoader {
    /// Twig: `FactoryRuntimeLoader::load()`: `null` for a class without a factory, else what the factory returns.
    fn load(&self, class: &str) -> Result<Option<Runtime>> {
        match self.map.get(class) {
            None => Ok(None),
            Some(factory) => factory().map(Some),
        }
    }
}
