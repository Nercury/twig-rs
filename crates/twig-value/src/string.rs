//! PHP byte strings.
//!
//! PHP strings are arbitrary byte sequences without an encoding (`zend_string`). [`PhpString`] is an
//! immutable, reference-counted byte string: cloning is an atomic increment, so values that contain
//! strings stay cheap to copy and are `Send + Sync`.

use std::borrow::{Borrow, Cow};
use std::fmt;
use std::ops::Deref;
use std::sync::{Arc, LazyLock};

/// An immutable PHP byte string (`zend_string`).
///
/// Equality, ordering and hashing are bytewise (`memcmp` order), which is what PHP's
/// `zend_binary_strcmp` and array-key hashing use. The bytes are not required to be UTF-8.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PhpString(Arc<[u8]>);

static EMPTY: LazyLock<PhpString> = LazyLock::new(|| PhpString(Arc::from(&[][..])));

impl PhpString {
    /// The empty string (`ZSTR_EMPTY_ALLOC()`); shares one allocation.
    pub fn empty() -> Self {
        EMPTY.clone()
    }

    /// Creates a string by copying `bytes`.
    pub fn from_bytes(bytes: &[u8]) -> Self {
        if bytes.is_empty() {
            Self::empty()
        } else {
            PhpString(Arc::from(bytes))
        }
    }

    /// The raw bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Length in bytes (`ZSTR_LEN`).
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the string has no bytes.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The string as `&str` when it is valid UTF-8.
    pub fn to_str(&self) -> Result<&str, std::str::Utf8Error> {
        std::str::from_utf8(&self.0)
    }

    /// The string as UTF-8, replacing invalid sequences with U+FFFD.
    pub fn to_string_lossy(&self) -> Cow<'_, str> {
        String::from_utf8_lossy(&self.0)
    }

    /// Whether both strings share the same allocation (the `zend_string` pointer-equality
    /// shortcut some engine functions take; never observable for strings, provided for
    /// completeness).
    pub fn ptr_eq(a: &PhpString, b: &PhpString) -> bool {
        Arc::ptr_eq(&a.0, &b.0)
    }

    /// The bytes up to (excluding) the first NUL byte, i.e. what C code printing the string with
    /// `%s` sees (anonymous class names embed a NUL).
    pub fn c_str(&self) -> &[u8] {
        match memchr::memchr(0, &self.0) {
            Some(pos) => &self.0[..pos],
            None => &self.0,
        }
    }

    /// Concatenates byte slices into a new string.
    pub fn concat(parts: &[&[u8]]) -> Self {
        let len = parts.iter().map(|p| p.len()).sum();
        let mut buf = Vec::with_capacity(len);
        for part in parts {
            buf.extend_from_slice(part);
        }
        PhpString::from(buf)
    }
}

impl Default for PhpString {
    fn default() -> Self {
        Self::empty()
    }
}

impl Deref for PhpString {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        &self.0
    }
}

impl AsRef<[u8]> for PhpString {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl Borrow<[u8]> for PhpString {
    fn borrow(&self) -> &[u8] {
        &self.0
    }
}

impl From<&[u8]> for PhpString {
    fn from(bytes: &[u8]) -> Self {
        PhpString::from_bytes(bytes)
    }
}

impl<const N: usize> From<&[u8; N]> for PhpString {
    fn from(bytes: &[u8; N]) -> Self {
        PhpString::from_bytes(bytes)
    }
}

impl From<Vec<u8>> for PhpString {
    fn from(bytes: Vec<u8>) -> Self {
        if bytes.is_empty() {
            PhpString::empty()
        } else {
            PhpString(Arc::from(bytes))
        }
    }
}

impl From<Box<[u8]>> for PhpString {
    fn from(bytes: Box<[u8]>) -> Self {
        PhpString::from(Vec::from(bytes))
    }
}

impl From<&str> for PhpString {
    fn from(s: &str) -> Self {
        PhpString::from_bytes(s.as_bytes())
    }
}

impl From<String> for PhpString {
    fn from(s: String) -> Self {
        PhpString::from(s.into_bytes())
    }
}

impl From<&String> for PhpString {
    fn from(s: &String) -> Self {
        PhpString::from_bytes(s.as_bytes())
    }
}

impl From<Cow<'_, str>> for PhpString {
    fn from(s: Cow<'_, str>) -> Self {
        PhpString::from_bytes(s.as_bytes())
    }
}

impl From<&PhpString> for PhpString {
    fn from(s: &PhpString) -> Self {
        s.clone()
    }
}

impl PartialEq<[u8]> for PhpString {
    fn eq(&self, other: &[u8]) -> bool {
        &*self.0 == other
    }
}

impl PartialEq<&[u8]> for PhpString {
    fn eq(&self, other: &&[u8]) -> bool {
        &*self.0 == *other
    }
}

impl<const N: usize> PartialEq<&[u8; N]> for PhpString {
    fn eq(&self, other: &&[u8; N]) -> bool {
        *self.0 == other[..]
    }
}

impl PartialEq<str> for PhpString {
    fn eq(&self, other: &str) -> bool {
        &*self.0 == other.as_bytes()
    }
}

impl PartialEq<&str> for PhpString {
    fn eq(&self, other: &&str) -> bool {
        &*self.0 == other.as_bytes()
    }
}

impl fmt::Display for PhpString {
    /// Lossy UTF-8 rendering (invalid sequences become U+FFFD).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_string_lossy())
    }
}

impl fmt::Debug for PhpString {
    /// Rendered like a Rust byte-string literal: `b"a\xff"`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("b\"")?;
        for &b in self.0.iter() {
            match b {
                b'"' => f.write_str("\\\"")?,
                b'\\' => f.write_str("\\\\")?,
                b'\n' => f.write_str("\\n")?,
                b'\r' => f.write_str("\\r")?,
                b'\t' => f.write_str("\\t")?,
                0x20..=0x7e => write!(f, "{}", b as char)?,
                _ => write!(f, "\\x{b:02x}")?,
            }
        }
        f.write_str("\"")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basics() {
        let s = PhpString::from("abc");
        assert_eq!(s.len(), 3);
        assert_eq!(s, "abc");
        assert_eq!(s.as_bytes(), b"abc");
        assert!(PhpString::empty().is_empty());
        assert!(PhpString::ptr_eq(&PhpString::empty(), &PhpString::from("")));
        assert_eq!(
            format!("{:?}", PhpString::from(&b"a\"\xff"[..])),
            "b\"a\\\"\\xff\""
        );
        assert_eq!(
            PhpString::from(&b"class@anonymous\0/x.php"[..]).c_str(),
            b"class@anonymous"
        );
        let (a, ab, b) = (
            PhpString::from("a"),
            PhpString::from("ab"),
            PhpString::from("b"),
        );
        assert!(a < b);
        assert!(ab > a);
        assert_eq!(PhpString::concat(&[b"a", b"", b"bc"]), "abc");
    }
}
