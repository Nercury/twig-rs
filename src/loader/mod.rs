//! Template loaders: where template sources come from.
//!
//! Twig: `Twig\Loader\LoaderInterface` and its implementations (TWIG/src/Loader/). Template names are the logical
//! names used in templates (`{% include 'base.html' %}`); a loader turns them into [`Source`]s.
//!
//! [`ArrayLoader`] keeps templates in memory, [`FilesystemLoader`] reads them from directories (with namespaces,
//! name normalisation and Twig's security checks) and [`ChainLoader`] asks several loaders in turn.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crate::{Error, Result, Source};

pub mod chain;
pub mod filesystem;

pub use chain::ChainLoader;
pub use filesystem::FilesystemLoader;

/// Loads template sources by logical name.
///
/// Twig: `Twig\Loader\LoaderInterface` (TWIG/src/Loader/LoaderInterface.php). Every method that receives an unknown
/// name fails with a loader error, except [`exists`](Loader::exists).
pub trait Loader: Send + Sync {
    /// The source of a template. Twig: `getSourceContext(string $name): Source`.
    fn get_source_context(&self, name: &str) -> Result<Source>;

    /// A key that identifies the template source for caching compiled templates.
    /// Twig: `getCacheKey(string $name): string` (bytes, because PHP strings are byte strings).
    fn get_cache_key(&self, name: &str) -> Result<Vec<u8>>;

    /// Whether the template is still fresh relative to a Unix timestamp. Twig: `isFresh(string $name, int $time)`.
    fn is_fresh(&self, name: &str, time: i64) -> Result<bool>;

    /// Whether the loader can load the template. Twig: `exists(string $name)`.
    fn exists(&self, name: &str) -> bool;

    /// The PHP class name of the loader (Twig's `$loader::class`), which [`ChainLoader`] puts in front of the
    /// errors it collects from [`get_cache_key`](Loader::get_cache_key) and [`is_fresh`](Loader::is_fresh).
    ///
    /// The core loaders return Twig's class names (`Twig\Loader\ArrayLoader`, ...); the default is the Rust type
    /// name.
    fn class_name(&self) -> &str {
        std::any::type_name::<Self>()
    }
}

/// Loads templates from an in-memory map of name to source code.
///
/// Twig: `Twig\Loader\ArrayLoader` (TWIG/src/Loader/ArrayLoader.php). Templates can be added after the loader has
/// been handed to an environment, like `ArrayLoader::setTemplate()`.
#[derive(Debug, Default)]
pub struct ArrayLoader {
    templates: RwLock<HashMap<String, Arc<[u8]>>>,
}

impl ArrayLoader {
    /// A loader for the given templates. Twig: `new ArrayLoader(array $templates)`.
    pub fn new<I, N, C>(templates: I) -> Self
    where
        I: IntoIterator<Item = (N, C)>,
        N: Into<String>,
        C: Into<Vec<u8>>,
    {
        let templates = templates
            .into_iter()
            .map(|(name, code)| (name.into(), Arc::from(code.into())))
            .collect();
        ArrayLoader {
            templates: RwLock::new(templates),
        }
    }

    /// Adds or replaces a template. Twig: `ArrayLoader::setTemplate(string $name, string $template)`.
    pub fn set_template(&self, name: impl Into<String>, code: impl Into<Vec<u8>>) {
        self.write().insert(name.into(), Arc::from(code.into()));
    }

    fn code(&self, name: &str) -> Result<Arc<[u8]>> {
        self.read()
            .get(name)
            .cloned()
            .ok_or_else(|| Error::loader(format!("Template \"{name}\" is not defined.")))
    }

    fn read(&self) -> std::sync::RwLockReadGuard<'_, HashMap<String, Arc<[u8]>>> {
        // A poisoned lock only means another thread panicked while inserting; the map itself stays valid.
        self.templates.read().unwrap_or_else(|e| e.into_inner())
    }

    fn write(&self) -> std::sync::RwLockWriteGuard<'_, HashMap<String, Arc<[u8]>>> {
        self.templates.write().unwrap_or_else(|e| e.into_inner())
    }
}

impl Loader for ArrayLoader {
    fn get_source_context(&self, name: &str) -> Result<Source> {
        Ok(Source::new(self.code(name)?.to_vec(), name))
    }

    fn get_cache_key(&self, name: &str) -> Result<Vec<u8>> {
        let code = self.code(name)?;
        // room for what the environment appends to make the class key, so that the whole template is not copied
        // once more
        let mut key = Vec::with_capacity(name.len() + 1 + code.len() + 64);
        key.extend_from_slice(name.as_bytes());
        key.push(b':');
        key.extend_from_slice(&code);
        Ok(key)
    }

    fn is_fresh(&self, name: &str, _time: i64) -> Result<bool> {
        self.code(name).map(|_| true)
    }

    fn exists(&self, name: &str) -> bool {
        self.read().contains_key(name)
    }

    fn class_name(&self) -> &str {
        "Twig\\Loader\\ArrayLoader"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn array_loader_matches_twig() {
        let loader = ArrayLoader::new([("index.twig", "Hello")]);
        assert!(loader.exists("index.twig"));
        assert!(!loader.exists("missing.twig"));
        let source = loader.get_source_context("index.twig").unwrap();
        assert_eq!(source.code(), b"Hello");
        assert_eq!(source.name(), "index.twig");
        assert_eq!(
            loader.get_cache_key("index.twig").unwrap(),
            b"index.twig:Hello"
        );
        assert!(loader.is_fresh("index.twig", 0).unwrap());

        for err in [
            loader.get_source_context("missing.twig").unwrap_err(),
            loader.is_fresh("missing.twig", 0).unwrap_err(),
            loader.get_cache_key("missing.twig").unwrap_err(),
        ] {
            assert_eq!(err.kind(), crate::ErrorKind::Loader);
            assert_eq!(err.to_string(), "Template \"missing.twig\" is not defined.");
        }

        loader.set_template("missing.twig", "now here");
        assert_eq!(
            loader.get_source_context("missing.twig").unwrap().code(),
            b"now here"
        );
    }
}
