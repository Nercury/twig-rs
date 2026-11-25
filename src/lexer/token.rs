//! Tokens. Twig: `Twig\Token` (TWIG/src/Token.php).

use std::fmt;

use super::php;

/// The type of a token, with Twig's numeric values.
///
/// Twig: the `Token::*_TYPE` constants (TWIG/src/Token.php).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i8)]
pub enum TokenType {
    /// `EOF_TYPE`: the end of the template (always the last token of a lexed stream).
    Eof = -1,
    /// `TEXT_TYPE`: template text outside tags.
    Text = 0,
    /// `BLOCK_START_TYPE`: `{%`.
    BlockStart = 1,
    /// `VAR_START_TYPE`: `{{`.
    VarStart = 2,
    /// `BLOCK_END_TYPE`: `%}`.
    BlockEnd = 3,
    /// `VAR_END_TYPE`: `}}`.
    VarEnd = 4,
    /// `NAME_TYPE`: a name.
    Name = 5,
    /// `NUMBER_TYPE`: a number literal; the value is an int or a float.
    Number = 6,
    /// `STRING_TYPE`: a string literal (or a part of an interpolated one), escapes already processed.
    String = 7,
    /// `OPERATOR_TYPE`: an operator, including `(`, `[`, `.`, `?` and `|`.
    Operator = 8,
    /// `PUNCTUATION_TYPE`: in practice one of `) ] { } : ,`.
    Punctuation = 9,
    /// `INTERPOLATION_START_TYPE`: `#{` inside a double-quoted string.
    InterpolationStart = 10,
    /// `INTERPOLATION_END_TYPE`: the `}` closing an interpolation.
    InterpolationEnd = 11,
    /// `ARROW_TYPE`, deprecated since Twig 3.21 ("arrow" is now an operator); never produced by the lexer.
    Arrow = 12,
    /// `SPREAD_TYPE`, deprecated since Twig 3.21 ("spread" is now an operator); never produced by the lexer.
    Spread = 13,
}

impl TokenType {
    /// The PHP constant value.
    pub fn as_int(self) -> i8 {
        self as i8
    }

    /// The type for a PHP constant value.
    pub fn from_int(value: i64) -> Option<TokenType> {
        Some(match value {
            -1 => TokenType::Eof,
            0 => TokenType::Text,
            1 => TokenType::BlockStart,
            2 => TokenType::VarStart,
            3 => TokenType::BlockEnd,
            4 => TokenType::VarEnd,
            5 => TokenType::Name,
            6 => TokenType::Number,
            7 => TokenType::String,
            8 => TokenType::Operator,
            9 => TokenType::Punctuation,
            10 => TokenType::InterpolationStart,
            11 => TokenType::InterpolationEnd,
            12 => TokenType::Arrow,
            13 => TokenType::Spread,
            _ => return None,
        })
    }

    /// The constant name, e.g. `EOF_TYPE`. Twig: `Token::typeToString($type, true)`; the long form is
    /// `Twig\Token::` followed by this name.
    pub fn name(self) -> &'static str {
        match self {
            TokenType::Eof => "EOF_TYPE",
            TokenType::Text => "TEXT_TYPE",
            TokenType::BlockStart => "BLOCK_START_TYPE",
            TokenType::VarStart => "VAR_START_TYPE",
            TokenType::BlockEnd => "BLOCK_END_TYPE",
            TokenType::VarEnd => "VAR_END_TYPE",
            TokenType::Name => "NAME_TYPE",
            TokenType::Number => "NUMBER_TYPE",
            TokenType::String => "STRING_TYPE",
            TokenType::Operator => "OPERATOR_TYPE",
            TokenType::Punctuation => "PUNCTUATION_TYPE",
            TokenType::InterpolationStart => "INTERPOLATION_START_TYPE",
            TokenType::InterpolationEnd => "INTERPOLATION_END_TYPE",
            TokenType::Arrow => "ARROW_TYPE",
            TokenType::Spread => "SPREAD_TYPE",
        }
    }

    /// The English name used in error messages, e.g. `end of template`. Twig: `Token::typeToEnglish()`.
    pub fn english(self) -> &'static str {
        match self {
            TokenType::Eof => "end of template",
            TokenType::Text => "text",
            TokenType::BlockStart => "begin of statement block",
            TokenType::VarStart => "begin of print statement",
            TokenType::BlockEnd => "end of statement block",
            TokenType::VarEnd => "end of print statement",
            TokenType::Name => "name",
            TokenType::Number => "number",
            TokenType::String => "string",
            TokenType::Operator => "operator",
            TokenType::Punctuation => "punctuation",
            TokenType::InterpolationStart => "begin of string interpolation",
            TokenType::InterpolationEnd => "end of string interpolation",
            TokenType::Arrow => "arrow function",
            TokenType::Spread => "spread operator",
        }
    }
}

/// The value of a token: a PHP string (bytes) for every type except numbers, which hold a PHP int or float.
/// Tokens without a value (delimiters, interpolation markers, EOF) hold the empty string.
#[derive(Clone, PartialEq)]
pub enum TokenValue {
    /// A PHP string.
    Bytes(Vec<u8>),
    /// A PHP int.
    Int(i64),
    /// A PHP float.
    Float(f64),
}

impl TokenValue {
    /// The empty string, the value of delimiter, interpolation and EOF tokens.
    pub fn empty() -> Self {
        TokenValue::Bytes(Vec::new())
    }

    /// The bytes of a string value; `None` for numbers.
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            TokenValue::Bytes(bytes) => Some(bytes),
            TokenValue::Int(_) | TokenValue::Float(_) => None,
        }
    }

    /// PHP truthiness: `''`, `'0'`, `0` and `0.0` are false.
    pub fn is_truthy(&self) -> bool {
        match self {
            TokenValue::Bytes(bytes) => php::is_truthy_str(bytes),
            TokenValue::Int(int) => *int != 0,
            TokenValue::Float(float) => *float != 0.0,
        }
    }

    /// PHP's string conversion of the value (`sprintf('%s', $value)`), as bytes.
    pub fn to_php_string(&self) -> Vec<u8> {
        match self {
            TokenValue::Bytes(bytes) => bytes.clone(),
            TokenValue::Int(int) => int.to_string().into_bytes(),
            TokenValue::Float(float) => php::float_to_string(*float).into_bytes(),
        }
    }

    /// PHP's loose comparison `$this->value == $value` with a string `$value`, as used by `Token::test()`: numeric
    /// strings compare as numbers (`'1.0' == '1'`, `1 == ' 1'`), anything else byte for byte (an int or float through
    /// its PHP string form).
    pub fn loosely_equals(&self, value: &[u8]) -> bool {
        match self {
            TokenValue::Bytes(bytes) => php::loose_equals_strings(bytes, value),
            TokenValue::Int(int) => php::loose_equals_int_string(*int, value),
            TokenValue::Float(float) => php::loose_equals_float_string(*float, value),
        }
    }
}

impl fmt::Debug for TokenValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TokenValue::Bytes(bytes) => match std::str::from_utf8(bytes) {
                Ok(text) => write!(f, "{text:?}"),
                Err(_) => write!(f, "b\"{}\"", bytes.escape_ascii()),
            },
            TokenValue::Int(int) => write!(f, "{int}"),
            TokenValue::Float(float) => write!(f, "{float:?}"),
        }
    }
}

/// PHP's `%s` formatting of the value (bytes that are not valid UTF-8 are replaced by U+FFFD).
impl fmt::Display for TokenValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TokenValue::Bytes(bytes) => f.write_str(&String::from_utf8_lossy(bytes)),
            TokenValue::Int(int) => write!(f, "{int}"),
            TokenValue::Float(float) => f.write_str(&php::float_to_string(*float)),
        }
    }
}

impl From<&str> for TokenValue {
    fn from(value: &str) -> Self {
        TokenValue::Bytes(value.as_bytes().to_vec())
    }
}

impl From<String> for TokenValue {
    fn from(value: String) -> Self {
        TokenValue::Bytes(value.into_bytes())
    }
}

impl From<&[u8]> for TokenValue {
    fn from(value: &[u8]) -> Self {
        TokenValue::Bytes(value.to_vec())
    }
}

impl From<Vec<u8>> for TokenValue {
    fn from(value: Vec<u8>) -> Self {
        TokenValue::Bytes(value)
    }
}

impl From<i64> for TokenValue {
    fn from(value: i64) -> Self {
        TokenValue::Int(value)
    }
}

impl From<f64> for TokenValue {
    fn from(value: f64) -> Self {
        TokenValue::Float(value)
    }
}

/// The value constraint of [`Token::test_value`]: one value (PHP loose `==`) or a list of values (PHP strict
/// `in_array()`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenValues<'a> {
    /// `$token->test($type, 'value')`.
    One(&'a [u8]),
    /// `$token->test($type, ['a', 'b'])`.
    AnyOf(&'a [&'a str]),
}

impl TokenValues<'_> {
    /// PHP truthiness of the `$values` argument.
    fn is_truthy(&self) -> bool {
        match self {
            TokenValues::One(value) => php::is_truthy_str(value),
            TokenValues::AnyOf(values) => !values.is_empty(),
        }
    }

    /// Whether one of the values is in `list` (`(array) $values` searched with strict `in_array()`).
    fn any_in(&self, list: &[&[u8]]) -> bool {
        match self {
            TokenValues::One(value) => list.contains(value),
            TokenValues::AnyOf(values) => {
                values.iter().any(|value| list.contains(&value.as_bytes()))
            }
        }
    }
}

impl<'a> From<&'a str> for TokenValues<'a> {
    fn from(value: &'a str) -> Self {
        TokenValues::One(value.as_bytes())
    }
}

impl<'a> From<&'a String> for TokenValues<'a> {
    fn from(value: &'a String) -> Self {
        TokenValues::One(value.as_bytes())
    }
}

impl<'a> From<&'a [u8]> for TokenValues<'a> {
    fn from(value: &'a [u8]) -> Self {
        TokenValues::One(value)
    }
}

impl<'a> From<&'a [&'a str]> for TokenValues<'a> {
    fn from(values: &'a [&'a str]) -> Self {
        TokenValues::AnyOf(values)
    }
}

impl<'a, const N: usize> From<&'a [&'a str; N]> for TokenValues<'a> {
    fn from(values: &'a [&'a str; N]) -> Self {
        TokenValues::AnyOf(values)
    }
}

/// The values for which `Token::test()` lets an `OPERATOR_TYPE` test match a `PUNCTUATION_TYPE` token (backward
/// compatibility with Twig < 3.21, where these were punctuation).
const BC_OPERATOR_PUNCTUATION: &[&[u8]] = &[b"(", b"[", b"|", b".", b"?", b"?:"];

/// A token: a type, a value, the template line and the byte offset of its first byte, plus optional documentation
/// attached by `{## ... ##}` / `## ...` documentation comments.
///
/// Twig: `Twig\Token` (TWIG/src/Token.php).
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    ty: TokenType,
    value: TokenValue,
    line: i64,
    offset: Option<usize>,
    documentation: Option<Vec<u8>>,
}

impl Token {
    /// A token that is not tied to a source position (no offset), such as the tokens a token parser injects.
    ///
    /// Twig: `new Token($type, $value, $lineno)`. Unlike Twig, constructing an `ARROW_TYPE` or `SPREAD_TYPE` token
    /// raises no deprecation.
    pub fn new(ty: TokenType, value: impl Into<TokenValue>, line: i64) -> Self {
        Token {
            ty,
            value: value.into(),
            line,
            offset: None,
            documentation: None,
        }
    }

    /// Sets the 0-based byte offset of the token in the source code.
    #[must_use]
    pub fn with_offset(mut self, offset: Option<usize>) -> Self {
        self.offset = offset;
        self
    }

    /// Sets the documentation attached to the token.
    #[must_use]
    pub fn with_documentation(mut self, documentation: Option<Vec<u8>>) -> Self {
        self.documentation = documentation;
        self
    }

    /// The token type. Twig: `Token::getType()` (deprecated in Twig, which prefers `test()`).
    pub fn ty(&self) -> TokenType {
        self.ty
    }

    /// Twig: `Token::getValue()`.
    pub fn value(&self) -> &TokenValue {
        &self.value
    }

    /// Twig: `Token::getLine()`.
    pub fn line(&self) -> i64 {
        self.line
    }

    /// The 0-based byte offset of the token in the source code, `None` for tokens that are not tied to a source
    /// position. Twig: `Token::getOffset()`.
    pub fn offset(&self) -> Option<usize> {
        self.offset
    }

    /// Twig: `Token::getDocumentation()`.
    pub fn documentation(&self) -> Option<&[u8]> {
        self.documentation.as_deref()
    }

    /// The English name of the token type. Twig: `Token::toEnglish()`.
    pub fn to_english(&self) -> &'static str {
        self.ty.english()
    }

    /// Tests the token type, like `$token->test($type)`.
    ///
    /// For backward compatibility an `OPERATOR_TYPE` test also matches `PUNCTUATION_TYPE` tokens, exactly as in
    /// Twig; an `ARROW_TYPE`/`SPREAD_TYPE` test matches the `=>`/`...` operator (without Twig's deprecation).
    ///
    /// Twig: `Token::test()` (TWIG/src/Token.php).
    pub fn test(&self, ty: TokenType) -> bool {
        self.test_impl(ty, None)
    }

    /// Tests the token type and value, like `$token->test($type, $values)`; Twig's one-argument form
    /// `$token->test('name')` is `test_value(TokenType::Name, "name")`.
    ///
    /// Twig: `Token::test()` (TWIG/src/Token.php). See [`test`](Self::test) for the backward-compatibility rules;
    /// Twig's 3.21 deprecation for testing punctuation-typed `( [ | . ? ?:` tokens (which the lexer never produces)
    /// is not raised.
    pub fn test_value<'a>(&self, ty: TokenType, values: impl Into<TokenValues<'a>>) -> bool {
        self.test_impl(ty, Some(values.into()))
    }

    fn test_impl(&self, ty: TokenType, values: Option<TokenValues<'_>>) -> bool {
        match ty {
            TokenType::Arrow => {
                return self.ty == TokenType::Operator && self.value.as_bytes() == Some(b"=>");
            }
            TokenType::Spread => {
                return self.ty == TokenType::Operator && self.value.as_bytes() == Some(b"...");
            }
            _ => {}
        }
        let mut type_matches = self.ty == ty;
        if !type_matches && ty == TokenType::Operator && self.ty == TokenType::Punctuation {
            type_matches = match &values {
                Some(values) if values.is_truthy() => values.any_in(BC_OPERATOR_PUNCTUATION),
                _ => true,
            };
        }
        type_matches
            && match values {
                None => true,
                Some(TokenValues::One(value)) => self.value.loosely_equals(value),
                Some(TokenValues::AnyOf(values)) => self
                    .value
                    .as_bytes()
                    .is_some_and(|bytes| values.iter().any(|value| value.as_bytes() == bytes)),
            }
    }
}

/// `TYPE_NAME(value)`, e.g. `NAME_TYPE(foo)`. Twig: `Token::__toString()`.
impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}({})", self.ty.name(), self.value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tests_types_and_values() {
        let name = Token::new(TokenType::Name, "endfor", 1);
        assert!(name.test(TokenType::Name));
        assert!(name.test_value(TokenType::Name, "endfor"));
        assert!(name.test_value(TokenType::Name, &["else", "endfor"]));
        assert!(!name.test_value(TokenType::Name, &["else"]));
        assert!(!name.test(TokenType::String));

        // Backward compatibility: OPERATOR tests match punctuation.
        let punctuation = Token::new(TokenType::Punctuation, ")", 1);
        assert!(punctuation.test(TokenType::Operator));
        assert!(!punctuation.test_value(TokenType::Operator, ")"));
        let synthesized = Token::new(TokenType::Punctuation, "(", 1);
        assert!(synthesized.test_value(TokenType::Operator, "("));
        assert!(synthesized.test_value(TokenType::Punctuation, "("));

        let arrow = Token::new(TokenType::Operator, "=>", 1);
        assert!(arrow.test(TokenType::Arrow));
        assert!(!arrow.test(TokenType::Spread));

        let number = Token::new(TokenType::Number, 1i64, 1);
        assert!(number.test_value(TokenType::Number, "1"));
        assert!(!number.test_value(TokenType::Number, &["1"]));
    }

    #[test]
    fn displays_like_php() {
        assert_eq!(
            Token::new(TokenType::Name, "foo", 1).to_string(),
            "NAME_TYPE(foo)"
        );
        assert_eq!(
            Token::new(TokenType::Number, 1e20, 1).to_string(),
            "NUMBER_TYPE(1.0E+20)"
        );
        assert_eq!(Token::new(TokenType::Eof, "", 1).to_string(), "EOF_TYPE()");
    }

    #[test]
    fn evaluates_php_truthiness() {
        assert!(!TokenValue::empty().is_truthy());
        assert!(!TokenValue::from("0").is_truthy());
        assert!(TokenValue::from("00").is_truthy());
        assert!(!TokenValue::Int(0).is_truthy());
        assert!(!TokenValue::Float(-0.0).is_truthy());
        assert!(TokenValue::Float(f64::NAN).is_truthy());
    }
}
