//! Charset names the way glibc's `iconv_open()` reads them in the oracle (all of it observed black-box, see
//! tests/vectors/string/charset_basic/names.gen.php):
//!
//! - a name is `A`, `A/B` or `A/B/C`, split at the first two slashes; `A/B` names the charset (glibc lists a few as
//!   `A/B/`, for example `ISO-10646/UCS4/`), `C` is the suffix of error handlers (only read for the target);
//! - trailing commas and white space are removed from `B`, and from `A` when nothing else is left of `B`; then every
//!   byte other than ASCII letters, digits and `-.:_,` is dropped and letters are upper-cased (so a comma before a
//!   dropped byte stays, and a `B` of dropped bytes only leaves `A` as it is);
//! - an empty `A` and `B` mean the locale's charset, UTF-8 under the oracle's `LC_CTYPE=C.UTF-8`;
//! - the suffix is a list separated by `,` and `/`; an item `TRANSLIT` or `IGNORE` (any case, followed by nothing
//!   but white space) turns the handler on, anything else is ignored.

use super::codec::Charset;
use super::name_table::NAMES;

/// What a charset name refers to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Lookup {
    /// A charset this module converts.
    Supported(Charset),
    /// A charset glibc knows and twig-rs does not convert.
    Unsupported,
    /// A name glibc does not know (`iconv_open()` fails with `EINVAL`).
    Unknown,
}

/// A parsed charset name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Name {
    /// The charset.
    pub(crate) lookup: Lookup,
    /// The suffix asks for //TRANSLIT.
    pub(crate) translit: bool,
    /// The suffix asks for //IGNORE.
    pub(crate) ignore: bool,
}

/// Parses a charset name as glibc does (the bytes up to the first NUL, which is all C sees).
pub(crate) fn parse(name: &[u8]) -> Name {
    let (a, rest) = split_slash(name);
    let (b, suffix) = rest.map_or((&[][..], None), split_slash);
    let b = trim_end(b, b",");
    let key = if b.is_empty() {
        normalize(trim_end(a, b","))
    } else {
        let mut key = normalize(a);
        let b = normalize(b);
        if !b.is_empty() {
            key.push(b'/');
            key.extend_from_slice(&b);
        }
        key
    };
    let lookup = if key.is_empty() {
        Lookup::Supported(Charset::Utf8)
    } else {
        match NAMES.binary_search_by(|(known, _)| known.as_bytes().cmp(&key)) {
            Ok(i) => NAMES[i].1.map_or(Lookup::Unsupported, Lookup::Supported),
            Err(_) => Lookup::Unknown,
        }
    };
    let (mut translit, mut ignore) = (false, false);
    for item in suffix
        .unwrap_or_default()
        .split(|&b| b == b',' || b == b'/')
    {
        let item = trim_end(item, b"");
        translit |= item.eq_ignore_ascii_case(b"TRANSLIT");
        ignore |= item.eq_ignore_ascii_case(b"IGNORE");
    }
    Name {
        lookup,
        translit,
        ignore,
    }
}

fn split_slash(s: &[u8]) -> (&[u8], Option<&[u8]>) {
    match memchr::memchr(b'/', s) {
        Some(i) => (&s[..i], Some(&s[i + 1..])),
        None => (s, None),
    }
}

/// `s` without trailing white space (C's `isspace()`) and bytes of `also`.
fn trim_end<'a>(s: &'a [u8], also: &[u8]) -> &'a [u8] {
    let end = s
        .iter()
        .rposition(|b| {
            !matches!(b, b' ' | b'\t' | b'\n' | 0x0B | 0x0C | b'\r') && !also.contains(b)
        })
        .map_or(0, |i| i + 1);
    &s[..end]
}

fn normalize(part: &[u8]) -> Vec<u8> {
    part.iter()
        .filter(|&&b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b':' | b'_' | b','))
        .map(u8::to_ascii_uppercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::multibyte::Multibyte;
    use super::*;

    fn lookup(name: &[u8]) -> Lookup {
        parse(name).lookup
    }

    #[test]
    fn names() {
        assert_eq!(lookup(b"utf-8"), Lookup::Supported(Charset::Utf8));
        assert_eq!(lookup(b" UTF 8 "), Lookup::Supported(Charset::Utf8));
        assert_eq!(lookup(b""), Lookup::Supported(Charset::Utf8));
        assert_eq!(lookup(b"//IGNORE"), Lookup::Supported(Charset::Utf8));
        assert_eq!(lookup(b"UTF_8"), Lookup::Unknown);
        assert_eq!(lookup(b"/UTF-8"), Lookup::Unknown);
        assert_eq!(lookup(b"UTF-8/X"), Lookup::Unknown);
        assert_eq!(lookup(b"UTF-8//X"), Lookup::Supported(Charset::Utf8));
        assert_eq!(
            lookup(b"iso-10646/ucs4/ignore"),
            Lookup::Supported(Charset::Ucs4)
        );
        assert_eq!(lookup(b"ISO-10646,/UCS4"), Lookup::Unknown);
        assert_eq!(lookup(b"ASCII,/,"), Lookup::Supported(Charset::Ascii));
        assert_eq!(lookup(b"US, ,"), Lookup::Supported(Charset::Ascii));
        assert_eq!(lookup(b"US,("), Lookup::Unknown);
        assert_eq!(lookup(b"US,/("), Lookup::Unknown);
        assert_eq!(lookup(b"UTF-8/\x80/"), Lookup::Supported(Charset::Utf8));
        assert_eq!(lookup(b"latin1"), Lookup::Supported(Charset::Latin1));
        assert_eq!(lookup(b"UCS-2LE"), Lookup::Supported(Charset::Ucs2));
        assert_eq!(
            lookup(b"EUC-JP"),
            Lookup::Supported(Charset::Multibyte(Multibyte::EucJp))
        );
        assert_eq!(lookup(b"IBM930"), Lookup::Unsupported);
        assert_eq!(lookup(b"WCHAR_T"), Lookup::Unsupported);
        assert_eq!(lookup(b"NOPE"), Lookup::Unknown);
    }

    #[test]
    fn suffixes() {
        let flags = |name: &[u8]| {
            let n = parse(name);
            (n.translit, n.ignore)
        };
        assert_eq!(flags(b"ASCII//TRANSLIT//IGNORE"), (true, true));
        assert_eq!(flags(b"ASCII//ignore"), (false, true));
        assert_eq!(flags(b"ASCII//IGNORE,TRANSLIT"), (true, true));
        assert_eq!(flags(b"ASCII//IGNORE \t"), (false, true));
        assert_eq!(flags(b"ASCII// IGNORE"), (false, false));
        assert_eq!(flags(b"ASCII//IGNORE,"), (false, true));
        assert_eq!(flags(b"ASCII//IGNOREX"), (false, false));
        assert_eq!(flags(b"ISO-10646/UCS4/TRANSLIT"), (true, false));
        assert_eq!(flags(b"ASCII/IGNORE"), (false, false));
    }

    #[test]
    fn table_is_sorted() {
        assert!(
            NAMES
                .windows(2)
                .all(|w| w[0].0.as_bytes() < w[1].0.as_bytes())
        );
    }
}
