//! `rawurlencode()`, `urlencode()` and `http_build_query()` (reference/php/stdlib.md §5.9): Twig's
//! `url_encode` filter (`http_build_query($array, '', '&', PHP_QUERY_RFC3986)` for arrays,
//! `rawurlencode()` otherwise) and the `url` escaping strategy.

use super::{long_param, str_param};
use crate::convert;
use crate::diag::Diagnostics;
use crate::error::{PhpError, PhpResult};
use crate::key::ArrayKey;
use crate::object::ObjectRef;
use crate::param::{self, Arg};
use crate::string::PhpString;
use crate::types;
use crate::value::Value;

/// `PHP_QUERY_RFC1738`: spaces as `+`, `~` encoded (`urlencode()`).
pub const PHP_QUERY_RFC1738: i64 = 1;
/// `PHP_QUERY_RFC3986`: spaces as `%20`, `~` kept (`rawurlencode()`).
pub const PHP_QUERY_RFC3986: i64 = 2;

const HEXCHARS: &[u8; 16] = b"0123456789ABCDEF";

/// Port of `php_url_encode_impl()` (PHPSRC/ext/standard/url.c), appending to `out`: bytes
/// `A-Z a-z 0-9 - _ .` (and `~` when `raw`) are kept, a space becomes `+` unless `raw`, every other
/// byte becomes `%XX` with uppercase hex digits.
pub fn url_encode_into(out: &mut Vec<u8>, s: &[u8], raw: bool) {
    out.reserve(s.len());
    for &c in s {
        if !raw && c == b' ' {
            out.push(b'+');
        } else if c.is_ascii_alphanumeric()
            || c == b'-'
            || c == b'.'
            || c == b'_'
            || (raw && c == b'~')
        {
            out.push(c);
        } else {
            out.extend_from_slice(&[
                b'%',
                HEXCHARS[usize::from(c >> 4)],
                HEXCHARS[usize::from(c & 15)],
            ]);
        }
    }
}

/// `php_raw_url_encode()` (`raw = true`) / `php_url_encode()` (`raw = false`).
pub fn url_encode_bytes(s: &[u8], raw: bool) -> PhpString {
    // [`url_encode_into`], sized first so that the string is built in one allocation
    let unreserved = |c: u8| {
        c.is_ascii_alphanumeric() || c == b'-' || c == b'.' || c == b'_' || (raw && c == b'~')
    };
    let len = s
        .iter()
        .map(|&c| {
            if unreserved(c) || (!raw && c == b' ') {
                1
            } else {
                3
            }
        })
        .sum();
    PhpString::build(len, |buf| {
        let mut at = 0;
        for &c in s {
            if !raw && c == b' ' {
                buf[at] = b'+';
                at += 1;
            } else if unreserved(c) {
                buf[at] = c;
                at += 1;
            } else {
                buf[at] = b'%';
                buf[at + 1] = HEXCHARS[usize::from(c >> 4)];
                buf[at + 2] = HEXCHARS[usize::from(c & 15)];
                at += 3;
            }
        }
    })
}

/// Port of `PHP_FUNCTION(rawurlencode)` (RFC 3986).
pub fn rawurlencode(string: &Value, diag: &mut dyn Diagnostics) -> PhpResult<PhpString> {
    let s = str_param(string, "rawurlencode", 1, "string", diag)?;
    Ok(url_encode_bytes(&s, true))
}

/// Port of `PHP_FUNCTION(urlencode)` (`application/x-www-form-urlencoded`).
pub fn urlencode(string: &Value, diag: &mut dyn Diagnostics) -> PhpResult<PhpString> {
    let s = str_param(string, "urlencode", 1, "string", diag)?;
    Ok(url_encode_bytes(&s, false))
}

/// The state of one `http_build_query()` run (`smart_str formstr` plus the recursion guard).
struct QueryBuilder<'a> {
    out: Vec<u8>,
    /// `formstr->s != NULL`: something was appended, so the next pair needs a separator.
    started: bool,
    arg_sep: &'a [u8],
    raw: bool,
    /// Objects whose property tables are being walked (`GC_PROTECT_RECURSION`).
    visiting: Vec<ObjectRef>,
}

/// A key of the table being encoded: an integer index or a (property) name.
enum QueryKey<'k> {
    Index(i64),
    Name(&'k [u8]),
}

impl QueryBuilder<'_> {
    /// Port of `php_url_encode_scalar()` (PHPSRC/ext/standard/http.c).
    fn scalar(
        &mut self,
        value: &Value,
        key: &QueryKey<'_>,
        num_prefix: &[u8],
        key_prefix: Option<&[u8]>,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<()> {
        if self.started {
            self.out.extend_from_slice(self.arg_sep);
        }
        self.started = true;
        if let Some(prefix) = key_prefix {
            self.out.extend_from_slice(prefix);
        }
        match key {
            QueryKey::Name(name) => url_encode_into(&mut self.out, name, self.raw),
            QueryKey::Index(idx) => {
                self.out.extend_from_slice(num_prefix);
                self.out.extend_from_slice(idx.to_string().as_bytes());
            }
        }
        if key_prefix.is_some() {
            self.out.extend_from_slice(b"%5D");
        }
        self.out.push(b'=');
        let mut value = value.clone();
        loop {
            match &value {
                Value::String(s) => url_encode_into(&mut self.out, s, self.raw),
                Value::Int(i) => self.out.extend_from_slice(i.to_string().as_bytes()),
                Value::Float(d) => {
                    let s = convert::double_to_string(*d, diag)?;
                    url_encode_into(&mut self.out, &s, self.raw);
                }
                Value::Bool(false) => self.out.push(b'0'),
                Value::Bool(true) => self.out.push(b'1'),
                Value::Object(o) => match o.enum_case_value() {
                    Some(backing) => {
                        value = backing.clone();
                        continue;
                    }
                    None => {
                        return Err(PhpError::value_error(format!(
                            "Unbacked enum {} cannot be converted to a string",
                            o.class().display_name()
                        )));
                    }
                },
                Value::Null | Value::Array(_) => unreachable!("handled by the caller"),
            }
            return Ok(());
        }
    }

    /// Port of `php_url_encode_hash_ex()` (PHPSRC/ext/standard/http.c) for one table: an array's
    /// elements or the properties of `object` visible from Twig's scope.
    fn table(
        &mut self,
        container: &Value,
        num_prefix: &[u8],
        key_prefix: Option<&[u8]>,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<()> {
        let entries: Vec<(ArrayKey, Value)> = match container {
            Value::Array(a) => a.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
            Value::Object(o) => {
                if self.visiting.iter().any(|v| ObjectRef::ptr_eq(v, o)) {
                    return Ok(());
                }
                o.visible_properties(diag)?
                    .into_iter()
                    .map(|(name, v)| (ArrayKey::Str(name), v))
                    .collect()
            }
            _ => unreachable!("tables are arrays or objects"),
        };
        let guard = matches!(container, Value::Object(_));
        if let Value::Object(o) = container {
            self.visiting.push(o.clone());
        }
        let result = self.entries(&entries, num_prefix, key_prefix, diag);
        if guard {
            self.visiting.pop();
        }
        result
    }

    fn entries(
        &mut self,
        entries: &[(ArrayKey, Value)],
        num_prefix: &[u8],
        key_prefix: Option<&[u8]>,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<()> {
        for (key, value) in entries {
            let key = match key {
                ArrayKey::Int(i) => QueryKey::Index(*i),
                ArrayKey::Str(s) => QueryKey::Name(s),
            };
            let nested = match value {
                Value::Array(_) => true,
                Value::Object(o) => o.enum_case_name().is_none(),
                _ => false,
            };
            if nested {
                let mut prefix: Vec<u8> = Vec::new();
                match &key {
                    QueryKey::Name(name) => {
                        if let Some(kp) = key_prefix {
                            prefix.extend_from_slice(kp);
                        }
                        url_encode_into(&mut prefix, name, self.raw);
                        prefix.extend_from_slice(if key_prefix.is_some() {
                            b"%5D%5B"
                        } else {
                            b"%5B"
                        });
                    }
                    QueryKey::Index(idx) => {
                        match key_prefix {
                            Some(kp) => prefix.extend_from_slice(kp),
                            None => prefix.extend_from_slice(num_prefix),
                        }
                        prefix.extend_from_slice(idx.to_string().as_bytes());
                        prefix.extend_from_slice(if key_prefix.is_some() {
                            b"%5D%5B"
                        } else {
                            b"%5B"
                        });
                    }
                }
                self.table(value, b"", Some(&prefix), diag)?;
            } else if value.is_null() {
                continue;
            } else {
                self.scalar(value, &key, num_prefix, key_prefix, diag)?;
            }
        }
        Ok(())
    }
}

/// The body of `PHP_FUNCTION(http_build_query)` (PHPSRC/ext/standard/http.c) on parsed arguments.
///
/// Walks `data` (an array, or an object's properties visible from outside its class) in order:
/// `null` values are skipped, nested arrays and non-enum objects recurse with `key%5B...%5D`
/// prefixes (an object already being walked is skipped), `true`/`false` become `1`/`0`, ints are
/// decimal, floats use PHP string conversion (precision 14), backed enums contribute their value
/// and unbacked ones throw `ValueError: Unbacked enum X cannot be converted to a string`. Integer
/// keys of the outermost table get `numeric_prefix`; `arg_separator` `None` means `&`
/// (`arg_separator.output`); keys and values are encoded with `rawurlencode()` when
/// `encoding_type` is [`PHP_QUERY_RFC3986`] (compared after truncation to a C `int`) and with
/// `urlencode()` otherwise.
pub fn build_query(
    data: &Value,
    numeric_prefix: &[u8],
    arg_separator: Option<&[u8]>,
    encoding_type: i64,
    diag: &mut dyn Diagnostics,
) -> PhpResult<PhpString> {
    let mut builder = QueryBuilder {
        out: Vec::new(),
        started: false,
        arg_sep: arg_separator.unwrap_or(b"&"),
        raw: encoding_type as i32 == PHP_QUERY_RFC3986 as i32,
        visiting: Vec::new(),
    };
    builder.table(data, numeric_prefix, None, diag)?;
    Ok(PhpString::from(builder.out))
}

/// Port of `PHP_FUNCTION(http_build_query)` (`http_build_query(array|object $data, string
/// $numeric_prefix = "", ?string $arg_separator = null, int $encoding_type = PHP_QUERY_RFC1738):
/// string`); see [`build_query`].
///
/// `data` must be an array or an object (`TypeError: http_build_query(): Argument #1 ($data) must
/// be of type array, X given`) but not an enum (`... must not be an enum, X given`).
pub fn http_build_query(
    data: &Value,
    numeric_prefix: Option<&Value>,
    arg_separator: Option<&Value>,
    encoding_type: Option<&Value>,
    diag: &mut dyn Diagnostics,
) -> PhpResult<PhpString> {
    const F: &str = "http_build_query";
    if !matches!(data, Value::Array(_) | Value::Object(_)) {
        return Err(Arg::new(F, 1, "data").type_error("of type array", data));
    }
    let prefix = match numeric_prefix {
        Some(p) => str_param(p, F, 2, "numeric_prefix", diag)?,
        None => PhpString::empty(),
    };
    let sep = match arg_separator {
        Some(s) => param::parse_str_or_null(
            s,
            &Arg::new(F, 3, "arg_separator").declared("?string"),
            diag,
        )?,
        None => None,
    };
    let enc_type = match encoding_type {
        Some(e) => long_param(e, F, 4, "encoding_type", diag)?,
        None => PHP_QUERY_RFC1738,
    };
    if let Value::Object(o) = data
        && o.enum_case_name().is_some()
    {
        return Err(PhpError::type_error(format!(
            "{F}(): Argument #1 ($data) must not be an enum, {} given",
            types::zend_zval_value_name(data)
        )));
    }
    build_query(data, &prefix, sep.as_deref(), enc_type, diag)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoded_strings_match_the_appending_encoder() {
        let all: Vec<u8> = (0..=255).collect();
        for raw in [true, false] {
            for s in [&all[..], b"", b"a b~c-d.e_f", b"%%  "] {
                let mut expected = Vec::new();
                url_encode_into(&mut expected, s, raw);
                assert_eq!(url_encode_bytes(s, raw).as_bytes(), &expected[..]);
            }
        }
    }
}
