//! Templates from a list of loaders.
//!
//! Twig: `Twig\Loader\ChainLoader` (TWIG/src/Loader/ChainLoader.php).

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use super::Loader;
use crate::{Error, ErrorKind, Result, Source};

/// Loads templates from other loaders, trying them in order.
///
/// Twig: `Twig\Loader\ChainLoader` (TWIG/src/Loader/ChainLoader.php). For every method but `exists`, a loader is
/// asked only when its [`exists`](Loader::exists) says so; the first answer wins. When every asked loader fails with
/// a loader error, the error is `Template "<name>" is not defined (<messages>).`, listing the failures (prefixed
/// with the failing loader's [`class_name`](Loader::class_name) for [`get_cache_key`](Loader::get_cache_key) and
/// [`is_fresh`](Loader::is_fresh)); without failures it is `Template "<name>" is not defined.` Errors of other kinds
/// are returned at once.
///
/// [`exists`](Loader::exists) answers are remembered per name until [`add_loader`](Self::add_loader) is called, so
/// a template added to an inner loader later is not seen by `exists` (but is found by the other methods).
///
/// ```
/// use std::sync::Arc;
/// use twig::loader::{ArrayLoader, ChainLoader, Loader};
///
/// let loader = ChainLoader::new([
///     Arc::new(ArrayLoader::new([("index.twig", "first")])) as Arc<dyn Loader>,
///     Arc::new(ArrayLoader::new([("index.twig", "second"), ("other.twig", "other")])),
/// ]);
/// assert_eq!(loader.get_source_context("index.twig")?.code(), b"first");
/// assert_eq!(loader.get_cache_key("other.twig")?, b"other.twig:other");
/// assert_eq!(
///     loader.get_source_context("missing.twig").unwrap_err().to_string(),
///     "Template \"missing.twig\" is not defined.",
/// );
/// # Ok::<(), twig::Error>(())
/// ```
#[derive(Default)]
pub struct ChainLoader {
    state: RwLock<State>,
}

#[derive(Default)]
struct State {
    loaders: Vec<Arc<dyn Loader>>,
    /// `$hasSourceCache`: name => what `exists()` answered.
    has_source_cache: HashMap<String, bool>,
    /// Bumped by `add_loader()`, so that an `exists()` running concurrently does not store an answer computed for
    /// the previous list of loaders.
    generation: u64,
}

impl ChainLoader {
    /// A chain of the given loaders. Twig: `new ChainLoader(iterable $loaders = [])`.
    pub fn new<I>(loaders: I) -> Self
    where
        I: IntoIterator<Item = Arc<dyn Loader>>,
    {
        ChainLoader {
            state: RwLock::new(State {
                loaders: loaders.into_iter().collect(),
                ..State::default()
            }),
        }
    }

    /// Appends a loader and forgets the remembered [`exists`](Loader::exists) answers.
    ///
    /// Twig: `ChainLoader::addLoader(LoaderInterface $loader)`.
    pub fn add_loader(&self, loader: Arc<dyn Loader>) {
        let mut state = self.write();
        state.loaders.push(loader);
        state.has_source_cache.clear();
        state.generation += 1;
    }

    /// The loaders, in order. Twig: `ChainLoader::getLoaders()`.
    pub fn get_loaders(&self) -> Vec<Arc<dyn Loader>> {
        self.read().loaders.clone()
    }

    fn read(&self) -> RwLockReadGuard<'_, State> {
        // A poisoned lock only means another thread panicked; the state is updated in single steps and stays valid.
        self.state.read().unwrap_or_else(|e| e.into_inner())
    }

    fn write(&self) -> RwLockWriteGuard<'_, State> {
        self.state.write().unwrap_or_else(|e| e.into_inner())
    }

    /// The loop shared by `getSourceContext()`, `getCacheKey()` and `isFresh()`: asks every loader that has the
    /// template, collecting the messages of loader errors (via `describe`) until one succeeds.
    fn first_success<T>(
        &self,
        name: &str,
        call: impl Fn(&dyn Loader) -> Result<T>,
        describe: impl Fn(&dyn Loader, &Error) -> String,
    ) -> Result<T> {
        let mut exceptions = Vec::new();
        // The list is copied so that no lock is held while the loaders run (they may use this chain again).
        for loader in self.get_loaders() {
            if !loader.exists(name) {
                continue;
            }
            match call(&*loader) {
                Ok(value) => return Ok(value),
                Err(error) if error.kind() == ErrorKind::Loader => {
                    exceptions.push(describe(&*loader, &error));
                }
                Err(error) => return Err(error),
            }
        }
        let details = if exceptions.is_empty() {
            String::new()
        } else {
            format!(" ({})", exceptions.join(", "))
        };
        Err(Error::loader(format!(
            "Template \"{name}\" is not defined{details}."
        )))
    }
}

impl fmt::Debug for ChainLoader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let loaders: Vec<String> = self
            .read()
            .loaders
            .iter()
            .map(|loader| loader.class_name().to_owned())
            .collect();
        f.debug_struct("ChainLoader")
            .field("loaders", &loaders)
            .finish_non_exhaustive()
    }
}

impl Loader for ChainLoader {
    /// Twig: `ChainLoader::getSourceContext()`; failures are listed by message.
    fn get_source_context(&self, name: &str) -> Result<Source> {
        self.first_success(
            name,
            |loader| loader.get_source_context(name),
            |_, error| error.to_string(),
        )
    }

    /// Twig: `ChainLoader::getCacheKey()`; failures are listed as `<class name>: <message>`.
    fn get_cache_key(&self, name: &str) -> Result<Vec<u8>> {
        self.first_success(
            name,
            |loader| loader.get_cache_key(name),
            |loader, error| format!("{}: {error}", loader.class_name()),
        )
    }

    /// Twig: `ChainLoader::isFresh()`; failures are listed as `<class name>: <message>`.
    fn is_fresh(&self, name: &str, time: i64) -> Result<bool> {
        self.first_success(
            name,
            |loader| loader.is_fresh(name, time),
            |loader, error| format!("{}: {error}", loader.class_name()),
        )
    }

    /// Twig: `ChainLoader::exists()`: whether any loader has the template, remembered per name.
    fn exists(&self, name: &str) -> bool {
        let (loaders, generation) = {
            let state = self.read();
            if let Some(&exists) = state.has_source_cache.get(name) {
                return exists;
            }
            (state.loaders.clone(), state.generation)
        };
        let exists = loaders.iter().any(|loader| loader.exists(name));
        let mut state = self.write();
        if state.generation == generation {
            state.has_source_cache.insert(name.to_owned(), exists);
        }
        exists
    }

    fn class_name(&self) -> &str {
        "Twig\\Loader\\ChainLoader"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loader::{ArrayLoader, FilesystemLoader};

    #[test]
    fn class_names_are_twigs() {
        assert_eq!(
            ChainLoader::default().class_name(),
            "Twig\\Loader\\ChainLoader"
        );
        assert_eq!(
            ArrayLoader::default().class_name(),
            "Twig\\Loader\\ArrayLoader"
        );
        assert_eq!(
            FilesystemLoader::default().class_name(),
            "Twig\\Loader\\FilesystemLoader"
        );
    }

    #[test]
    fn debug_lists_the_loaders() {
        let chain = ChainLoader::new([Arc::new(ArrayLoader::default()) as Arc<dyn Loader>]);
        assert_eq!(
            format!("{chain:?}"),
            "ChainLoader { loaders: [\"Twig\\\\Loader\\\\ArrayLoader\"], .. }"
        );
    }
}
