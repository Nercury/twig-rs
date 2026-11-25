//! Template sources.

use std::fmt;
use std::sync::Arc;

/// Holds information about a non-compiled Twig template: its code (bytes, as in PHP), its logical name (used in
/// error messages and by `_self`) and its filesystem path, if any.
///
/// Twig: `Twig\Source` (TWIG/src/Source.php). Cloning is cheap: the data is shared.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Source {
    inner: Arc<SourceInner>,
}

#[derive(PartialEq, Eq, Hash)]
struct SourceInner {
    code: Box<[u8]>,
    name: String,
    path: String,
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
        Source {
            inner: Arc::new(SourceInner {
                code: code.into().into_boxed_slice(),
                name: name.into(),
                path: path.into(),
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

    /// The filesystem path of the template, or `""`. Twig: `Source::getPath()`.
    pub fn path(&self) -> &str {
        &self.inner.path
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

impl fmt::Debug for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Source")
            .field("name", &self.name())
            .field("path", &self.path())
            .field("code", &String::from_utf8_lossy(self.code()))
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_columns_on_bytes() {
        let source = Source::new("{{ é + $ }}", "index");
        assert_eq!(source.column(8), 9);
        let source = Source::new("x\r\n{{ é + $ }}", "index");
        assert_eq!(source.column(11), 9);
        assert_eq!(Source::new("a\r", "index").column(2), 1);
        assert_eq!(Source::new("abc", "index").column(100), 4);
    }
}
