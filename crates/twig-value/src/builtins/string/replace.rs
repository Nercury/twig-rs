//! `strtr()` (Twig's `replace` filter, reference/php/stdlib.md §5.3) and `str_replace()` (used by
//! Twig's lexer and compiler).

use std::collections::HashMap;

use super::{replace_all, str_param, str_weak};
use crate::array::Array;
use crate::convert;
use crate::diag::Diagnostics;
use crate::error::{PhpError, PhpResult};
use crate::key::ArrayKey;
use crate::param::Arg;
use crate::string::PhpString;
use crate::value::Value;

/// Port of `php_strtr_ex()` (PHPSRC/ext/standard/string.c): byte-for-byte translation of
/// `from[i]` to `to[i]` for the first `min(len(from), len(to))` bytes (a later duplicate in `from`
/// wins). Returns `s` itself when nothing changes.
pub fn strtr_bytes(s: &PhpString, from: &[u8], to: &[u8]) -> PhpString {
    let n = from.len().min(to.len());
    if n == 0 {
        return s.clone();
    }
    let mut xlat: [u8; 256] = std::array::from_fn(|i| i as u8);
    for i in 0..n {
        xlat[usize::from(from[i])] = to[i];
    }
    match s.iter().position(|&c| xlat[usize::from(c)] != c) {
        None => s.clone(),
        Some(first) => {
            let mut out = s.to_vec();
            for c in &mut out[first..] {
                *c = xlat[usize::from(*c)];
            }
            PhpString::from(out)
        }
    }
}

/// `strtr(): Ignoring replacement of empty string`.
fn empty_key_warning(diag: &mut dyn Diagnostics) -> PhpResult<()> {
    diag.warn("strtr(): Ignoring replacement of empty string")
}

/// Port of `php_strtr_array()` and `php_strtr_array_ex()` (PHPSRC/ext/standard/string.c): the
/// array form of `strtr()` on a parsed subject.
///
/// * No pairs: `s` unchanged. One pair: the key (ints in decimal) and the replacement (PHP string
///   conversion, even when it is never used) are converted first; an empty key warns `strtr():
///   Ignoring replacement of empty string` and returns `s`; otherwise every occurrence is replaced.
/// * Two or more pairs: empty string keys warn (once each, in order) and are skipped, keys longer
///   than `s` are skipped; the subject is scanned left to right and at each position the
///   **longest** key that matches wins; its replacement is converted at that moment (so conversion
///   diagnostics repeat per match) and the scan continues after the match (replacements are never
///   rescanned).
///
/// Callers handle the empty-subject shortcut of `PHP_FUNCTION(strtr)` (see [`strtr`]).
pub fn strtr_array(
    s: &PhpString,
    pairs: &Array,
    diag: &mut dyn Diagnostics,
) -> PhpResult<PhpString> {
    match pairs.len() {
        0 => Ok(s.clone()),
        1 => {
            let (key, value) = pairs.first().expect("one pair");
            let key = key.to_php_string();
            let replace = convert::to_string(value, diag)?;
            if key.is_empty() {
                empty_key_warning(diag)?;
                return Ok(s.clone());
            }
            Ok(replace_all(s, &key, &replace).0)
        }
        _ => strtr_array_ex(s, pairs, diag),
    }
}

/// Port of `php_strtr_array_ex()` (PHPSRC/ext/standard/string.c).
fn strtr_array_ex(
    input: &PhpString,
    pats: &Array,
    diag: &mut dyn Diagnostics,
) -> PhpResult<PhpString> {
    let s = input.as_bytes();
    let slen = s.len();
    let mut minlen = 128 * 1024;
    let mut maxlen = 0;
    let mut lengths = vec![false; slen + 1];
    let mut first_bytes = [false; 256];
    let mut has_num_keys = false;

    for key in pats.keys() {
        match key {
            ArrayKey::Int(_) => has_num_keys = true,
            ArrayKey::Str(k) => {
                let len = k.len();
                if len == 0 {
                    empty_key_warning(diag)?;
                    continue;
                }
                if len > slen {
                    continue;
                }
                maxlen = maxlen.max(len);
                minlen = minlen.min(len);
                lengths[len] = true;
                first_bytes[usize::from(k[0])] = true;
            }
        }
    }

    // With integer keys PHP rebuilds a string-keyed table (`zend_hash_add`: the first entry for a
    // key wins) without the patterns longer than the subject.
    let rebuilt: Option<HashMap<PhpString, &Value>> = if has_num_keys {
        let mut table = HashMap::with_capacity(pats.len());
        for (key, entry) in pats.iter() {
            let key_used = match key {
                ArrayKey::Int(i) => {
                    let k = convert::long_to_string(*i);
                    let len = k.len();
                    if len > slen {
                        continue;
                    }
                    maxlen = maxlen.max(len);
                    minlen = minlen.min(len);
                    lengths[len] = true;
                    first_bytes[usize::from(k[0])] = true;
                    k
                }
                ArrayKey::Str(k) => {
                    if k.len() > slen {
                        continue;
                    }
                    k.clone()
                }
            };
            table.entry(key_used).or_insert(entry);
        }
        Some(table)
    } else {
        None
    };

    if minlen > maxlen {
        return Ok(input.clone());
    }
    let lookup = |key: &[u8]| -> Option<&Value> {
        match &rebuilt {
            Some(table) => table.get(key).copied(),
            None => pats.get_str(key),
        }
    };

    let mut result: Option<Vec<u8>> = None;
    let mut old_pos = 0;
    let mut pos = 0;
    while pos <= slen - minlen {
        if first_bytes[usize::from(s[pos])] {
            let mut len = maxlen.min(slen - pos);
            while len >= minlen {
                if lengths[len]
                    && let Some(entry) = lookup(&s[pos..pos + len])
                {
                    let replacement = convert::to_string(entry, diag)?;
                    let out = result.get_or_insert_with(|| Vec::with_capacity(slen));
                    out.extend_from_slice(&s[old_pos..pos]);
                    out.extend_from_slice(&replacement);
                    old_pos = pos + len;
                    pos = old_pos - 1;
                    break;
                }
                len -= 1;
            }
        }
        pos += 1;
    }
    Ok(match result {
        Some(mut out) => {
            out.extend_from_slice(&s[old_pos..]);
            PhpString::from(out)
        }
        None => input.clone(),
    })
}

/// Port of `PHP_FUNCTION(strtr)` (`strtr(string $string, string|array $from, ?string $to = null)`)
/// and its frameless variants (same parsing).
///
/// With two arguments (`to` is `None`) `from` must be an array (`TypeError: strtr(): Argument #2
/// ($from) must be of type array, X given`) and [`strtr_array`] runs; with three, `from` and `to`
/// are strings (`null` deprecated, typed `string` and `?string`) and [`strtr_bytes`] runs. An empty
/// subject returns `""` before anything else.
pub fn strtr(
    string: &Value,
    from: &Value,
    to: Option<&Value>,
    diag: &mut dyn Diagnostics,
) -> PhpResult<PhpString> {
    let s = str_param(string, "strtr", 1, "string", diag)?;
    match to {
        None => {
            let Value::Array(pairs) = from else {
                return Err(Arg::new("strtr", 2, "from").type_error("of type array", from));
            };
            if s.is_empty() {
                return Ok(PhpString::empty());
            }
            strtr_array(&s, pairs, diag)
        }
        Some(to) => {
            let from = crate::param::parse_str(
                from,
                &Arg::new("strtr", 2, "from").declared("array|string"),
                diag,
            )?;
            let to =
                crate::param::parse_str(to, &Arg::new("strtr", 3, "to").declared("?string"), diag)?;
            if s.is_empty() {
                return Ok(PhpString::empty());
            }
            Ok(strtr_bytes(&s, &from, &to))
        }
    }
}

/// A `string|array` argument of `str_replace()`.
enum StrOrArray<'a> {
    Str(PhpString),
    Array(&'a Array),
}

/// `Z_PARAM_ARRAY_HT_OR_STR` (PHPSRC/Zend/zend_API.h) of `str_replace()` (declared
/// `array|string`).
fn array_or_str<'a>(
    v: &'a Value,
    position: u32,
    name: &str,
    diag: &mut dyn Diagnostics,
) -> PhpResult<StrOrArray<'a>> {
    if let Value::Array(a) = v {
        return Ok(StrOrArray::Array(a));
    }
    let arg = Arg::new("str_replace", position, name).declared("array|string");
    match str_weak(v, &arg, diag)? {
        Some(s) => Ok(StrOrArray::Str(s)),
        None => Err(arg.type_error("of type array|string", v)),
    }
}

/// Port of `php_str_replace_in_subject()` (PHPSRC/ext/standard/string.c), case-sensitive.
fn replace_in_subject(
    search: &StrOrArray<'_>,
    replace: &StrOrArray<'_>,
    subject: PhpString,
    count: &mut i64,
    diag: &mut dyn Diagnostics,
) -> PhpResult<PhpString> {
    if subject.is_empty() {
        return Ok(PhpString::empty());
    }
    match search {
        StrOrArray::Array(search) => {
            let mut subject = subject;
            let mut replace_values = match replace {
                StrOrArray::Array(r) => Some(r.values()),
                StrOrArray::Str(_) => None,
            };
            for entry in search.values() {
                let search_str = convert::to_string(entry, diag)?;
                let replace_str = match (&mut replace_values, replace) {
                    (Some(values), _) => match values.next() {
                        Some(v) => convert::to_string(v, diag)?,
                        None => PhpString::empty(),
                    },
                    (None, StrOrArray::Str(s)) => s.clone(),
                    (None, StrOrArray::Array(_)) => unreachable!("replace array has an iterator"),
                };
                if search_str.is_empty() {
                    continue;
                }
                let (result, n) = replace_all(&subject, &search_str, &replace_str);
                *count += n;
                if n > 0 {
                    subject = result;
                    if subject.is_empty() {
                        return Ok(PhpString::empty());
                    }
                }
            }
            Ok(subject)
        }
        StrOrArray::Str(search) => {
            let StrOrArray::Str(replace) = replace else {
                unreachable!("checked by the caller");
            };
            if search.is_empty() {
                return Ok(subject);
            }
            let (result, n) = replace_all(&subject, search, replace);
            *count += n;
            Ok(result)
        }
    }
}

/// Port of `PHP_FUNCTION(str_replace)` / `_php_str_replace_common()` (case-sensitive) and its
/// frameless variant, also returning the replacement count (the by-reference `$count`).
///
/// `search`, `replace` and `subject` are `array|string` (other values: `TypeError: str_replace():
/// Argument #N ($x) must be of type array|string, X given`); a string `search` with an array
/// `replace` throws `TypeError: str_replace(): Argument #2 ($replace) must be of type string when
/// argument #1 ($search) is a string`. An array `search` is applied entry by entry (each converted
/// to a string, empty ones skipped) with the matching entry of an array `replace` (`""` once it
/// runs out); an array `subject` returns an array with the same keys.
pub fn str_replace_count(
    search: &Value,
    replace: &Value,
    subject: &Value,
    diag: &mut dyn Diagnostics,
) -> PhpResult<(Value, i64)> {
    let search = array_or_str(search, 1, "search", diag)?;
    let replace = array_or_str(replace, 2, "replace", diag)?;
    let subject = array_or_str(subject, 3, "subject", diag)?;
    if matches!(search, StrOrArray::Str(_)) && matches!(replace, StrOrArray::Array(_)) {
        return Err(PhpError::type_error(
            "str_replace(): Argument #2 ($replace) must be of type string when argument #1 ($search) is a string",
        ));
    }
    let mut count = 0;
    let result = match subject {
        StrOrArray::Array(subjects) => {
            let mut out = Array::new();
            for (key, entry) in subjects.iter() {
                let s = convert::to_string(entry, diag)?;
                let r = replace_in_subject(&search, &replace, s, &mut count, diag)?;
                out.insert(key.clone(), Value::String(r));
            }
            Value::Array(out)
        }
        StrOrArray::Str(s) => {
            Value::String(replace_in_subject(&search, &replace, s, &mut count, diag)?)
        }
    };
    Ok((result, count))
}

/// [`str_replace_count`] without the count: `str_replace(array|string $search, array|string
/// $replace, string|array $subject): string|array`.
pub fn str_replace(
    search: &Value,
    replace: &Value,
    subject: &Value,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Value> {
    Ok(str_replace_count(search, replace, subject, diag)?.0)
}
