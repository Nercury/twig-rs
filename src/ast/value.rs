//! Scalar values held by the AST: constants, temporary-variable names and node attribute values.

use std::fmt::{self, Write as _};

use crate::lexer::TokenValue;

/// A PHP value as held by `Twig\Node\Expression\ConstantExpression`: null, bool, int, float, string (bytes) or an
/// array of such values (default values of callable parameters, e.g. `variables = []` of `include()`).
#[derive(Debug, Clone, PartialEq)]
pub enum ConstantValue {
    /// PHP `null`.
    Null,
    /// PHP `true`/`false`.
    Bool(bool),
    /// A PHP int.
    Int(i64),
    /// A PHP float.
    Float(f64),
    /// A PHP string.
    String(Vec<u8>),
    /// A PHP array, in insertion order.
    Array(Vec<(ArrayKey, ConstantValue)>),
}

/// A PHP array key.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ArrayKey {
    /// An int key.
    Int(i64),
    /// A string key (never a canonical decimal integer string, PHP turns those into ints).
    String(Vec<u8>),
}

impl ConstantValue {
    /// A string constant.
    pub fn string(value: impl Into<Vec<u8>>) -> Self {
        ConstantValue::String(value.into())
    }

    /// The string bytes, for a string constant.
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            ConstantValue::String(bytes) => Some(bytes),
            _ => None,
        }
    }

    /// PHP's string conversion (`(string) $value`, `sprintf('%s', $value)`) of a scalar: `null` and `false` give
    /// `""`, `true` gives `"1"`, floats use `precision=14`. Arrays give `"Array"` (PHP also raises an "Array to
    /// string conversion" warning, which callers that need it must report themselves).
    pub fn to_php_string(&self) -> Vec<u8> {
        match self {
            ConstantValue::Null | ConstantValue::Bool(false) => Vec::new(),
            ConstantValue::Bool(true) => b"1".to_vec(),
            ConstantValue::Int(int) => int.to_string().into_bytes(),
            ConstantValue::Float(float) => TokenValue::Float(*float).to_php_string(),
            ConstantValue::String(bytes) => bytes.clone(),
            ConstantValue::Array(_) => b"Array".to_vec(),
        }
    }

    /// PHP truthiness.
    pub fn is_truthy(&self) -> bool {
        match self {
            ConstantValue::Null => false,
            ConstantValue::Bool(b) => *b,
            ConstantValue::Int(i) => *i != 0,
            ConstantValue::Float(f) => *f != 0.0,
            ConstantValue::String(s) => !(s.is_empty() || s.as_slice() == b"0"),
            ConstantValue::Array(items) => !items.is_empty(),
        }
    }

    /// The constant for a token value (a string, an int or a float).
    pub fn from_token_value(value: &TokenValue) -> Self {
        match value {
            TokenValue::Bytes(bytes) => ConstantValue::String(bytes.clone()),
            TokenValue::Int(int) => ConstantValue::Int(*int),
            TokenValue::Float(float) => ConstantValue::Float(*float),
        }
    }
}

impl From<&str> for ConstantValue {
    fn from(value: &str) -> Self {
        ConstantValue::String(value.as_bytes().to_vec())
    }
}

impl From<i64> for ConstantValue {
    fn from(value: i64) -> Self {
        ConstantValue::Int(value)
    }
}

impl From<bool> for ConstantValue {
    fn from(value: bool) -> Self {
        ConstantValue::Bool(value)
    }
}

/// The name of a `Twig\Node\Expression\TempNameExpression` (`LocalVariable`, `MacroVariable`): `null` (a variable
/// name generated at compile time), an int, or a string.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TempName {
    /// `null`: the compiler picks a name.
    Null,
    /// An int name (argument positions, digit-only names).
    Int(i64),
    /// A string name, with [`RESERVED_NAME_PREFIX`] for the [`RESERVED_NAMES`].
    String(Vec<u8>),
}

/// Twig: `TempNameExpression::RESERVED_NAMES`, the names of compiled PHP variables a temporary variable must not
/// clash with.
pub const RESERVED_NAMES: [&str; 5] = ["varargs", "context", "macros", "blocks", "this"];

/// Twig: `TempNameExpression::RESERVED_NAME_PREFIX`, `"\u{035C}"` (UTF-8 `CD 9C`).
pub const RESERVED_NAME_PREFIX: &str = "\u{035C}";

impl TempName {
    /// The normalised name `TempNameExpression::__construct()` stores: digit-only strings become ints (saturating
    /// like PHP's `(int)` cast), the [`RESERVED_NAMES`] get the [`RESERVED_NAME_PREFIX`].
    pub fn normalize(name: TempNameInput) -> TempName {
        match name {
            TempNameInput::Null => TempName::Null,
            TempNameInput::Int(int) => TempName::Int(int),
            TempNameInput::String(bytes) => {
                if !bytes.is_empty() && bytes.iter().all(u8::is_ascii_digit) {
                    let mut value: i64 = 0;
                    for &digit in &bytes {
                        match value
                            .checked_mul(10)
                            .and_then(|v| v.checked_add(i64::from(digit - b'0')))
                        {
                            Some(next) => value = next,
                            None => {
                                value = i64::MAX;
                                break;
                            }
                        }
                    }
                    TempName::Int(value)
                } else if RESERVED_NAMES
                    .iter()
                    .any(|r| r.as_bytes() == bytes.as_slice())
                {
                    let mut prefixed = RESERVED_NAME_PREFIX.as_bytes().to_vec();
                    prefixed.extend_from_slice(&bytes);
                    TempName::String(prefixed)
                } else {
                    TempName::String(bytes)
                }
            }
        }
    }

    /// The string name, if the name is a string.
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            TempName::String(bytes) => Some(bytes),
            _ => None,
        }
    }

    /// The name without the [`RESERVED_NAME_PREFIX`]. Twig: `MacroNode::stripReservedPrefix()`.
    pub fn strip_reserved_prefix(name: &[u8]) -> &[u8] {
        name.strip_prefix(RESERVED_NAME_PREFIX.as_bytes())
            .unwrap_or(name)
    }
}

/// The name passed to a `TempNameExpression` constructor (`string|int|null`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TempNameInput {
    /// `null`.
    Null,
    /// An int.
    Int(i64),
    /// A string.
    String(Vec<u8>),
}

impl From<&str> for TempNameInput {
    fn from(value: &str) -> Self {
        TempNameInput::String(value.as_bytes().to_vec())
    }
}

impl From<&[u8]> for TempNameInput {
    fn from(value: &[u8]) -> Self {
        TempNameInput::String(value.to_vec())
    }
}

impl From<Vec<u8>> for TempNameInput {
    fn from(value: Vec<u8>) -> Self {
        TempNameInput::String(value)
    }
}

impl From<i64> for TempNameInput {
    fn from(value: i64) -> Self {
        TempNameInput::Int(value)
    }
}

/// The value of a node attribute, as listed by [`Node::attributes`](super::Node::attributes) and
/// [`Expr::attributes`](super::Expr::attributes) in Twig's attribute order.
#[derive(Debug, Clone, PartialEq)]
pub enum AttrValue {
    /// PHP `null`.
    Null,
    /// A bool.
    Bool(bool),
    /// An int.
    Int(i64),
    /// A float.
    Float(f64),
    /// A string.
    String(Vec<u8>),
    /// An array.
    Array(Vec<(AttrValue, AttrValue)>),
    /// An object, by its string form (e.g. `Twig\TwigFilter(upper)` for a callable).
    Object(String),
}

impl AttrValue {
    /// A string attribute.
    pub fn string(value: impl AsRef<[u8]>) -> Self {
        AttrValue::String(value.as_ref().to_vec())
    }
}

impl From<&ConstantValue> for AttrValue {
    fn from(value: &ConstantValue) -> Self {
        match value {
            ConstantValue::Null => AttrValue::Null,
            ConstantValue::Bool(b) => AttrValue::Bool(*b),
            ConstantValue::Int(i) => AttrValue::Int(*i),
            ConstantValue::Float(f) => AttrValue::Float(*f),
            ConstantValue::String(s) => AttrValue::String(s.clone()),
            ConstantValue::Array(items) => AttrValue::Array(
                items
                    .iter()
                    .map(|(k, v)| {
                        let key = match k {
                            ArrayKey::Int(i) => AttrValue::Int(*i),
                            ArrayKey::String(s) => AttrValue::String(s.clone()),
                        };
                        (key, AttrValue::from(v))
                    })
                    .collect(),
            ),
        }
    }
}

impl From<&TempName> for AttrValue {
    fn from(value: &TempName) -> Self {
        match value {
            TempName::Null => AttrValue::Null,
            TempName::Int(i) => AttrValue::Int(*i),
            TempName::String(s) => AttrValue::String(s.clone()),
        }
    }
}

impl From<bool> for AttrValue {
    fn from(value: bool) -> Self {
        AttrValue::Bool(value)
    }
}

/// PHP's `var_export()` of a float: the shortest representation that round-trips (`serialize_precision=-1`),
/// E-style when the decimal exponent is below -4 or above 16, with `.0` appended to finite values without a `.`.
///
/// Port of `smart_str_append_double()` with `zend_gcvt()` in mode 0 (PHPSRC/Zend/zend_smart_str.c,
/// PHPSRC/Zend/zend_strtod.c).
pub fn php_var_export_float(value: f64) -> String {
    if value.is_nan() {
        return "NAN".to_owned();
    }
    if value.is_infinite() {
        return if value < 0.0 { "-INF" } else { "INF" }.to_owned();
    }
    let mut out = String::new();
    if value.is_sign_negative() {
        out.push('-');
    }
    let abs = value.abs();
    // Rust's `{:e}` gives the shortest round-trip digits: "d[.ddd]e<exp>".
    let formatted = format!("{abs:e}");
    let (mantissa, exponent) = formatted
        .split_once('e')
        .unwrap_or((formatted.as_str(), "0"));
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let exponent: i32 = exponent.parse().unwrap_or(0);
    // zend_dtoa() returns "0" with decpt 1 for zero.
    let decpt = if abs == 0.0 { 1 } else { exponent + 1 };
    let ndigit = 17;
    if (decpt >= 0 && decpt > ndigit) || decpt < -3 {
        // exponential format, e.g. 1.0E+25
        let mut chars = digits.chars();
        out.push(chars.next().unwrap_or('0'));
        out.push('.');
        let rest: String = chars.collect();
        if rest.is_empty() {
            out.push('0');
        } else {
            out.push_str(&rest);
        }
        let exp = decpt - 1;
        let _ = write!(out, "E{}{}", if exp < 0 { '-' } else { '+' }, exp.abs());
    } else if decpt < 0 {
        out.push_str("0.");
        for _ in decpt..0 {
            out.push('0');
        }
        out.push_str(&digits);
    } else {
        let decpt = decpt as usize;
        let bytes = digits.as_bytes();
        for i in 0..decpt {
            out.push(bytes.get(i).map_or('0', |&b| char::from(b)));
        }
        if bytes.len() > decpt {
            if decpt == 0 {
                out.push('0');
            }
            out.push('.');
            out.push_str(&digits[decpt..]);
        }
    }
    if !out.contains('.') {
        out.push_str(".0");
    }
    out
}

/// PHP `strtolower()` (ASCII only since PHP 8.2).
pub(crate) fn ascii_lowercase(bytes: &[u8]) -> Vec<u8> {
    bytes.to_ascii_lowercase()
}

/// A displayable byte string (lossy UTF-8), for messages.
pub(crate) struct Lossy<'a>(pub &'a [u8]);

impl fmt::Display for Lossy<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&String::from_utf8_lossy(self.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exports_floats_like_var_export() {
        assert_eq!(php_var_export_float(1.5), "1.5");
        assert_eq!(php_var_export_float(1000.0), "1000.0");
        assert_eq!(php_var_export_float(0.1), "0.1");
        assert_eq!(php_var_export_float(1e100), "1.0E+100");
        assert_eq!(php_var_export_float(f64::INFINITY), "INF");
        assert_eq!(php_var_export_float(f64::NEG_INFINITY), "-INF");
        assert_eq!(php_var_export_float(f64::NAN), "NAN");
        assert_eq!(php_var_export_float(1.0), "1.0");
        assert_eq!(php_var_export_float(0.0), "0.0");
        assert_eq!(php_var_export_float(-0.0), "-0.0");
        assert_eq!(
            php_var_export_float(9.223372036854776e18),
            "9.223372036854776E+18"
        );
        assert_eq!(php_var_export_float(1e17), "1.0E+17");
        assert_eq!(php_var_export_float(1e16), "10000000000000000.0");
        assert_eq!(php_var_export_float(0.0001), "0.0001");
        assert_eq!(php_var_export_float(0.00001), "1.0E-5");
        assert_eq!(
            php_var_export_float(0.30000000000000004),
            "0.30000000000000004"
        );
        assert_eq!(php_var_export_float(123.456), "123.456");
    }

    #[test]
    fn normalizes_temp_names() {
        assert_eq!(TempName::normalize("12".into()), TempName::Int(12));
        assert_eq!(
            TempName::normalize("context".into()),
            TempName::String("\u{035C}context".as_bytes().to_vec())
        );
        assert_eq!(
            TempName::normalize("contexts".into()),
            TempName::String(b"contexts".to_vec())
        );
        assert_eq!(
            TempName::normalize("99999999999999999999".into()),
            TempName::Int(i64::MAX)
        );
        assert_eq!(TempName::normalize(TempNameInput::Null), TempName::Null);
    }
}
