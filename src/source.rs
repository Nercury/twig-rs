//! Template sources.

use std::any::Any;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, OnceLock};

/// Holds information about a non-compiled Twig template: its code (bytes, as in PHP), its logical name (used in
/// error messages and by `_self`) and its filesystem path, if any.
///
/// Twig: `Twig\Source` (TWIG/src/Source.php). Cloning is cheap: the data is shared, and so is a lazily resolved path
/// ([`Source::with_lazy_path`]) once it is resolved.
#[derive(Clone)]
pub struct Source {
    inner: Arc<SourceInner>,
}

struct SourceInner {
    code: Box<[u8]>,
    name: String,
    path: Path,
}

/// What a lazily resolved path asks for the path when the path is first needed: the closure of
/// [`Source::with_lazy_path`], or a resolver of the engine whose state it reaches with [`Source::path_resolver`].
pub(crate) trait PathResolver: Any + Send + Sync {
    /// The path.
    fn resolve(&self) -> String;
}

/// The resolver of [`Source::with_lazy_path`].
struct FnResolver<F>(F);

impl<F: Fn() -> String + Send + Sync + 'static> PathResolver for FnResolver<F> {
    fn resolve(&self) -> String {
        (self.0)()
    }
}

/// `Source::$path`: `string|\Closure`.
enum Path {
    Known(String),
    /// The closure, and the path it returned (`$this->path = ($this->path)()`).
    Lazy {
        resolver: Box<dyn PathResolver>,
        resolved: OnceLock<String>,
    },
}

impl Source {
    /// A source without a filesystem path. Twig: `new Source($code, $name)`.
    pub fn new(code: impl Into<Vec<u8>>, name: impl Into<String>) -> Self {
        Self::with_path(code, name, String::new())
    }

    /// Twig: `new Source($code, $name, $path)`.
    pub fn with_path(
        code: impl Into<Vec<u8>>,
        name: impl Into<String>,
        path: impl Into<String>,
    ) -> Self {
        Self::build(code, name, Path::Known(path.into()))
    }

    /// A source whose path `resolve` returns when the path is first needed ([`path`](Self::path)); the path is then
    /// kept, by this source and its clones. Twig: `Source::createWithLazyPath($code, $name, $path)`, what a
    /// template compiled without `debug` returns from `getSourceContext()`.
    ///
    /// `resolve` runs at most once per source, unless several threads ask for the path at the same time: the
    /// first result is kept.
    pub fn with_lazy_path(
        code: impl Into<Vec<u8>>,
        name: impl Into<String>,
        resolve: impl Fn() -> String + Send + Sync + 'static,
    ) -> Self {
        Self::with_path_resolver(code, name, FnResolver(resolve))
    }

    /// [`with_lazy_path`](Self::with_lazy_path) with a resolver of the engine.
    pub(crate) fn with_path_resolver(
        code: impl Into<Vec<u8>>,
        name: impl Into<String>,
        resolver: impl PathResolver,
    ) -> Self {
        Self::build(
            code,
            name,
            Path::Lazy {
                resolver: Box::new(resolver),
                resolved: OnceLock::new(),
            },
        )
    }

    /// The resolver of a lazily resolved path, when it is a `T` (resolved or not).
    pub(crate) fn path_resolver<T: PathResolver>(&self) -> Option<&T> {
        match &self.inner.path {
            Path::Known(_) => None,
            Path::Lazy { resolver, .. } => {
                let resolver: &dyn Any = &**resolver;
                resolver.downcast_ref::<T>()
            }
        }
    }

    fn build(code: impl Into<Vec<u8>>, name: impl Into<String>, path: Path) -> Self {
        Source {
            inner: Arc::new(SourceInner {
                code: code.into().into_boxed_slice(),
                name: name.into(),
                path,
            }),
        }
    }

    /// The template source code. Twig: `Source::getCode()`.
    pub fn code(&self) -> &[u8] {
        &self.inner.code
    }

    /// The template logical name. Twig: `Source::getName()`.
    pub fn name(&self) -> &str {
        &self.inner.name
    }

    /// The filesystem path of the template, or `""`. A lazily resolved path is resolved now if it is not yet.
    /// Twig: `Source::getPath()`.
    pub fn path(&self) -> &str {
        match &self.inner.path {
            Path::Known(path) => path,
            Path::Lazy { resolver, resolved } => match resolved.get() {
                Some(path) => path,
                None => {
                    // The resolver runs outside the cell, so a resolver that asks for this path again recurses
                    // (as `getPath()` does in PHP) instead of deadlocking.
                    let path = resolver.resolve();
                    resolved.get_or_init(|| path)
                }
            },
        }
    }

    /// The 1-based column of a 0-based byte offset in the code. Columns count bytes; `"\r\n"`, `"\r"` and `"\n"`
    /// all end a line. An offset past the end of the code is treated as the end of the code.
    ///
    /// Twig: `Source::getColumn()` (TWIG/src/Source.php) for a known (non-negative) offset; Twig's `null` for a
    /// negative offset is the caller's `None` offset.
    pub fn column(&self, offset: usize) -> usize {
        let before = &self.code()[..offset.min(self.code().len())];
        // Normalising "\r\n" and "\r" to "\n" only changes bytes up to the last line break, so the column is the
        // number of bytes after the last "\n" or "\r", plus one.
        match before.iter().rposition(|&b| b == b'\n' || b == b'\r') {
            Some(line_break) => before.len() - line_break,
            None => before.len() + 1,
        }
    }
}

/// Sources are equal when their code, name and path are; a lazily resolved path is resolved to compare it.
impl PartialEq for Source {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
            || (self.code() == other.code()
                && self.name() == other.name()
                && self.path() == other.path())
    }
}

impl Eq for Source {}

/// Hashes the code, the name and the path (a lazily resolved path is resolved).
impl Hash for Source {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.code().hash(state);
        self.name().hash(state);
        self.path().hash(state);
    }
}

/// Shows a lazily resolved path only once it is resolved (formatting never resolves it).
impl fmt::Debug for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = f.debug_struct("Source");
        debug.field("name", &self.name());
        match &self.inner.path {
            Path::Known(path) => debug.field("path", path),
            Path::Lazy { resolved, .. } => match resolved.get() {
                Some(path) => debug.field("path", path),
                None => debug.field("path", &format_args!("<not resolved>")),
            },
        };
        debug
            .field("code", &String::from_utf8_lossy(self.code()))
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn computes_columns_on_bytes() {
        let source = Source::new("{{ é + $ }}", "index");
        assert_eq!(source.column(8), 9);
        let source = Source::new("x\r\n{{ é + $ }}", "index");
        assert_eq!(source.column(11), 9);
        assert_eq!(Source::new("a\r", "index").column(2), 1);
        assert_eq!(Source::new("abc", "index").column(100), 4);
    }

    #[test]
    fn resolves_a_lazy_path_once_when_it_is_first_needed() {
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&calls);
        let source = Source::with_lazy_path("", "index", move || {
            counter.fetch_add(1, Ordering::SeqCst);
            "/path/to/index".to_owned()
        });
        let clone = source.clone();
        assert_eq!(
            format!("{source:?}"),
            "Source { name: \"index\", path: <not resolved>, code: \"\" }"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(clone.path(), "/path/to/index");
        assert_eq!(source.path(), "/path/to/index");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            format!("{source:?}"),
            "Source { name: \"index\", path: \"/path/to/index\", code: \"\" }"
        );
        assert_eq!(
            source,
            Source::with_path("", "index", "/path/to/index"),
            "a lazy path compares by its value"
        );
        assert_ne!(source, Source::new("", "index"));
    }
}
