//! `trim()`, `ltrim()`, `rtrim()` and `php_charmask()` (reference/php/stdlib.md §5.4).
//!
//! Twig's `trim` filter always passes a character list (`" \t\n\r\0\x0B"` when the template gives
//! none), `CoreExtension::compare()` trims `" \t\n\r\v\f"` and the lexer and `spaceless` use the
//! one-argument default (`" \n\r\t\v\0"`, no form feed).

use super::str_param;
use crate::diag::Diagnostics;
use crate::error::PhpResult;
use crate::string::PhpString;
use crate::value::Value;

/// Which side(s) [`php_trim`] strips: the `mode` argument of `php_trim()` (1 left, 2 right, 3
/// both).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TrimMode {
    /// `ltrim()`.
    Left = 1,
    /// `rtrim()`.
    Right = 2,
    /// `trim()`.
    Both = 3,
}

impl TrimMode {
    /// The PHP function of this mode (`ltrim`, `rtrim`, `trim`): the prefix of the
    /// `php_charmask()` warnings.
    pub fn function_name(self) -> &'static str {
        match self {
            TrimMode::Left => "ltrim",
            TrimMode::Right => "rtrim",
            TrimMode::Both => "trim",
        }
    }

    fn left(self) -> bool {
        self != TrimMode::Right
    }

    fn right(self) -> bool {
        self != TrimMode::Left
    }
}

/// Port of `php_charmask()` (PHPSRC/ext/standard/string.c): the 256-entry byte set described by
/// `input`, where `X..Y` (with `Y >= X`) adds a range.
///
/// A `..` that cannot form a range is skipped (its second `.` and the other bytes are still added)
/// after the warning `<function>(): Invalid '..'-range, no character to the left of '..'` (at the
/// start), `..., no character to the right of '..'` (at the end), `..., '..'-range needs to be
/// incrementing` (decreasing) or `Invalid '..'-range` (anything else, e.g. `a..b..c`).
pub fn charmask(
    input: &[u8],
    function: &str,
    diag: &mut dyn Diagnostics,
) -> PhpResult<[bool; 256]> {
    let mut mask = [false; 256];
    let len = input.len();
    let mut i = 0;
    while i < len {
        let c = input[i];
        if i + 3 < len && input[i + 1] == b'.' && input[i + 2] == b'.' && input[i + 3] >= c {
            for b in c..=input[i + 3] {
                mask[usize::from(b)] = true;
            }
            i += 4;
            continue;
        } else if i + 1 < len && input[i] == b'.' && input[i + 1] == b'.' {
            let detail = if i == 0 {
                ", no character to the left of '..'"
            } else if i + 2 >= len {
                ", no character to the right of '..'"
            } else if input[i - 1] > input[i + 2] {
                ", '..'-range needs to be incrementing"
            } else {
                ""
            };
            diag.warn(format!("{function}(): Invalid '..'-range{detail}"))?;
        } else {
            mask[usize::from(c)] = true;
        }
        i += 1;
    }
    Ok(mask)
}

/// The default set of `php_trim()` without a character list: `" \n\r\t\v\0"`.
fn is_default_trim_byte(c: u8) -> bool {
    matches!(c, b' ' | b'\n' | b'\r' | b'\t' | 0x0b | 0)
}

/// Port of `php_trim_int()` (PHPSRC/ext/standard/string.c): strips bytes of `what` (a
/// [`charmask`] description; a single byte is compared directly, so `"."` is not a range) or of the
/// default set `" \n\r\t\v\0"` when `what` is `None`. The mask (and its warnings) is computed even
/// for an empty string. Returns `s` itself when nothing is stripped.
pub fn php_trim(
    s: &PhpString,
    what: Option<&[u8]>,
    mode: TrimMode,
    diag: &mut dyn Diagnostics,
) -> PhpResult<PhpString> {
    let bytes = s.as_bytes();
    let mut start = 0;
    let mut end = bytes.len();
    let strip = |pred: &dyn Fn(u8) -> bool, start: &mut usize, end: &mut usize| {
        if mode.left() {
            while *start != *end && pred(bytes[*start]) {
                *start += 1;
            }
        }
        if mode.right() {
            while *start != *end && pred(bytes[*end - 1]) {
                *end -= 1;
            }
        }
    };
    match what {
        Some(&[p]) => strip(&|c| c == p, &mut start, &mut end),
        Some(w) => {
            let mask = charmask(w, mode.function_name(), diag)?;
            strip(&|c| mask[usize::from(c)], &mut start, &mut end);
        }
        None => strip(&is_default_trim_byte, &mut start, &mut end),
    }
    Ok(if end - start == bytes.len() {
        s.clone()
    } else {
        PhpString::from(&bytes[start..end])
    })
}

fn do_trim(
    string: &Value,
    characters: Option<&Value>,
    mode: TrimMode,
    diag: &mut dyn Diagnostics,
) -> PhpResult<PhpString> {
    let function = mode.function_name();
    let s = str_param(string, function, 1, "string", diag)?;
    let what = match characters {
        Some(c) => Some(str_param(c, function, 2, "characters", diag)?),
        None => None,
    };
    php_trim(&s, what.as_deref(), mode, diag)
}

/// Port of `PHP_FUNCTION(trim)` and its frameless variants (`trim(string $string, string
/// $characters = " \n\r\t\v\0")`).
pub fn trim(
    string: &Value,
    characters: Option<&Value>,
    diag: &mut dyn Diagnostics,
) -> PhpResult<PhpString> {
    do_trim(string, characters, TrimMode::Both, diag)
}

/// Port of `PHP_FUNCTION(ltrim)`.
pub fn ltrim(
    string: &Value,
    characters: Option<&Value>,
    diag: &mut dyn Diagnostics,
) -> PhpResult<PhpString> {
    do_trim(string, characters, TrimMode::Left, diag)
}

/// Port of `PHP_FUNCTION(rtrim)`.
pub fn rtrim(
    string: &Value,
    characters: Option<&Value>,
    diag: &mut dyn Diagnostics,
) -> PhpResult<PhpString> {
    do_trim(string, characters, TrimMode::Right, diag)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diag::Diagnostic;

    fn t(s: &str, what: Option<&str>, mode: TrimMode) -> (String, Vec<String>) {
        let mut d: Vec<Diagnostic> = Vec::new();
        let r = php_trim(&PhpString::from(s), what.map(str::as_bytes), mode, &mut d).unwrap();
        (
            r.to_string(),
            d.iter().map(|d| d.message.to_string()).collect(),
        )
    }

    #[test]
    fn ranges() {
        assert_eq!(t("abcxcba", Some("a..c"), TrimMode::Both).0, "x");
        assert_eq!(
            t("abc", Some("..c"), TrimMode::Both),
            (
                "ab".to_string(),
                vec!["trim(): Invalid '..'-range, no character to the left of '..'".to_string()]
            )
        );
        assert_eq!(
            t(".a.", Some("a.."), TrimMode::Left),
            (
                "".to_string(),
                vec!["ltrim(): Invalid '..'-range, no character to the right of '..'".to_string()]
            )
        );
        assert_eq!(
            t("ab", Some("z..a"), TrimMode::Right).1,
            vec!["rtrim(): Invalid '..'-range, '..'-range needs to be incrementing"]
        );
        assert_eq!(
            t("ab", Some("a..b..c"), TrimMode::Both).1,
            vec!["trim(): Invalid '..'-range"]
        );
        assert_eq!(t(" \x0c a \0", None, TrimMode::Both).0, "\x0c a");
    }
}
