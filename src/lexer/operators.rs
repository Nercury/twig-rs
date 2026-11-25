//! The operator pattern. Twig: `Lexer::getOperatorRegex()` (TWIG/src/Lexer.php).

use super::php::is_pcre_space;

/// The operator tokens of a default Twig 3.31.1 environment, in the order `Lexer::getOperatorRegex()` collects
/// them: `ExpressionParsers::getOperatorTokensFor($parser)` for every parser of `Environment::getExpressionParsers()`
/// (prefix parsers first, then infix parsers, by registration order; duplicates included).
///
/// Dumped from the oracle. Later stages derive this list from the expression-parser registry instead.
pub const DEFAULT_OPERATORS: &[&str] = &[
    "not",
    "...",
    "-",
    "+",
    "(",
    "?:",
    "? :",
    "??",
    "or",
    "xor",
    "and",
    "b-or",
    "b-xor",
    "b-and",
    "==",
    "!=",
    "<=>",
    "<",
    ">",
    ">=",
    "<=",
    "not in",
    "in",
    "matches",
    "starts with",
    "ends with",
    "has some",
    "has every",
    "===",
    "!==",
    "..",
    "+",
    "-",
    "~",
    "*",
    "/",
    "//",
    "%",
    "**",
    "?",
    "=",
    "is",
    "is not",
    "|",
    "(",
    ".",
    "?.",
    "[",
    "=>",
];

/// One alternative of the operator regex.
#[derive(Debug, Clone)]
struct OperatorPattern {
    /// Literal parts; consecutive parts are separated by `\s+`.
    parts: Vec<Vec<u8>>,
    /// The operator starts with a letter: `(?<![\.\|]\s|.[\.\|])` before it.
    lookbehind: bool,
    /// The operator ends with a letter: `(?=[\s()\[{])` after it.
    lookahead: bool,
}

/// Matches the operator alternation Twig builds in `Lexer::getOperatorRegex()`, anchored at a position.
///
/// The PCRE pattern needs look-behind and look-ahead, so it is matched by hand with the same semantics: the
/// alternatives are tried in pattern order and the first one that matches wins.
#[derive(Debug, Clone)]
pub(crate) struct OperatorMatcher {
    patterns: Vec<OperatorPattern>,
    /// For every byte, the indices (in pattern order) of the patterns whose match can start with it.
    by_first_byte: Vec<Vec<usize>>,
}

impl OperatorMatcher {
    /// Builds the matcher from the operator tokens in registry order.
    ///
    /// Port of `Lexer::getOperatorRegex()`: `array_combine()` keeps the first position of a repeated token,
    /// `arsort()` orders by byte length descending (stable), an operator ending with an ASCII letter must be
    /// followed by whitespace, `(`, `)`, `[` or `{`, an operator starting with an ASCII letter must not follow
    /// `.`/`|` (optionally followed by one whitespace character), and a whitespace run inside an operator matches
    /// any whitespace run. Empty tokens are ignored (Twig's pattern would match the empty string and loop forever).
    pub(crate) fn new<I>(operators: I) -> Self
    where
        I: IntoIterator,
        I::Item: AsRef<[u8]>,
    {
        let mut tokens: Vec<Vec<u8>> = Vec::new();
        for operator in operators {
            let operator = operator.as_ref();
            if !operator.is_empty() && !tokens.iter().any(|t| t == operator) {
                tokens.push(operator.to_vec());
            }
        }
        tokens.sort_by_key(|token| std::cmp::Reverse(token.len()));

        let patterns: Vec<OperatorPattern> = tokens
            .iter()
            .map(|token| OperatorPattern {
                parts: split_on_whitespace_runs(token),
                // `ctype_alpha()` of a single byte in the C/C.UTF-8 locale: ASCII letters only.
                lookbehind: token[0].is_ascii_alphabetic(),
                lookahead: token[token.len() - 1].is_ascii_alphabetic(),
            })
            .collect();

        // Patterns are visited in order, so every list is in pattern order.
        let mut by_first_byte = vec![Vec::new(); 256];
        for (index, pattern) in patterns.iter().enumerate() {
            match pattern.parts.first().and_then(|part| part.first()) {
                Some(&byte) => by_first_byte[usize::from(byte)].push(index),
                // The pattern starts with `\s+`.
                None => {
                    for byte in 0..=255u8 {
                        if is_pcre_space(byte) {
                            by_first_byte[usize::from(byte)].push(index);
                        }
                    }
                }
            }
        }

        OperatorMatcher {
            patterns,
            by_first_byte,
        }
    }

    /// The end offset of the operator matching at `start` in `code`, if any.
    pub(crate) fn match_at(&self, code: &[u8], start: usize) -> Option<usize> {
        let &first = code.get(start)?;
        self.by_first_byte[usize::from(first)]
            .iter()
            .find_map(|&index| self.patterns[index].match_at(code, start))
    }
}

impl OperatorPattern {
    fn match_at(&self, code: &[u8], start: usize) -> Option<usize> {
        if self.lookbehind && is_after_dot_or_pipe(code, start) {
            return None;
        }
        let mut position = start;
        for (i, part) in self.parts.iter().enumerate() {
            if i > 0 {
                let run = code[position..]
                    .iter()
                    .take_while(|&&b| is_pcre_space(b))
                    .count();
                if run == 0 {
                    return None;
                }
                position += run;
            }
            if !code[position..].starts_with(part) {
                return None;
            }
            position += part.len();
        }
        if self.lookahead
            && !code
                .get(position)
                .is_some_and(|&b| is_pcre_space(b) || b"()[{".contains(&b))
        {
            return None;
        }
        Some(position)
    }
}

/// The literal parts of an operator between its whitespace runs (`preg_replace('/\s+/', '\s+', $r)`), e.g.
/// `not  in` gives `["not", "in"]` and ` x` gives `["", "x"]`.
fn split_on_whitespace_runs(token: &[u8]) -> Vec<Vec<u8>> {
    let mut parts = vec![Vec::new()];
    let mut in_run = false;
    for &byte in token {
        if is_pcre_space(byte) {
            if !in_run {
                parts.push(Vec::new());
                in_run = true;
            }
        } else {
            in_run = false;
            if let Some(part) = parts.last_mut() {
                part.push(byte);
            }
        }
    }
    parts
}

/// The look-behind `(?<![\.\|]\s|.[\.\|])`: true when the two bytes before `position` are `.`/`|` followed by a
/// whitespace byte, or any byte but `\n` (PCRE's `.` without the `s` flag) followed by `.`/`|`.
fn is_after_dot_or_pipe(code: &[u8], position: usize) -> bool {
    if position < 2 {
        return false;
    }
    let (before, last) = (code[position - 2], code[position - 1]);
    let is_dot_or_pipe = |b: u8| b == b'.' || b == b'|';
    (is_dot_or_pipe(before) && is_pcre_space(last)) || (before != b'\n' && is_dot_or_pipe(last))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matched(matcher: &OperatorMatcher, code: &str, start: usize) -> Option<&'static str> {
        let end = matcher.match_at(code.as_bytes(), start)?;
        Some(code[start..end].to_owned().leak())
    }

    #[test]
    fn follows_the_regex_alternation_order() {
        let m = OperatorMatcher::new(DEFAULT_OPERATORS);
        assert_eq!(matched(&m, "not in x", 0), Some("not in"));
        assert_eq!(matched(&m, "not   in\n x", 0), Some("not   in"));
        assert_eq!(matched(&m, "not(x)", 0), Some("not"));
        assert_eq!(matched(&m, "notx", 0), None);
        assert_eq!(matched(&m, "is not}}", 0), Some("is"));
        assert_eq!(matched(&m, "...5", 0), Some("..."));
        assert_eq!(matched(&m, "..5", 0), Some(".."));
        assert_eq!(matched(&m, "? : b", 0), Some("? :"));
        assert_eq!(matched(&m, "?.5", 0), Some("?."));
        assert_eq!(matched(&m, "b-and2", 0), None);
        assert_eq!(matched(&m, "x.and ", 2), None);
        assert_eq!(matched(&m, "x. and ", 3), None);
        assert_eq!(matched(&m, "x.  and ", 4), Some("and"));
        assert_eq!(matched(&m, "x\n.and ", 3), Some("and"));
        assert_eq!(matched(&m, "and", 0), None);
    }

    #[test]
    fn orders_custom_operators_by_length() {
        let m = OperatorMatcher::new(["custom", "shared", "custom not", "shared", "not"]);
        assert_eq!(matched(&m, "custom not 1", 0), Some("custom not"));
        assert_eq!(matched(&m, "custom 1", 0), Some("custom"));
        assert_eq!(matched(&m, "shared 1", 0), Some("shared"));
    }

    #[test]
    fn ignores_empty_operators() {
        let m = OperatorMatcher::new(["", "+"]);
        assert_eq!(matched(&m, "x", 0), None);
        assert_eq!(matched(&m, "+", 0), Some("+"));
    }

    #[test]
    fn orders_the_default_operators_like_the_dumped_regex() {
        // The alternation order of the operator regex dumped from the oracle (reference/twig/syntax.md §2.4).
        let expected = [
            "starts with",
            "ends with",
            "has every",
            "has some",
            "matches",
            "not in",
            "is not",
            "b-xor",
            "b-and",
            "b-or",
            "not",
            "...",
            "? :",
            "xor",
            "and",
            "<=>",
            "===",
            "!==",
            "?:",
            "??",
            "or",
            "==",
            "!=",
            ">=",
            "<=",
            "in",
            "..",
            "//",
            "**",
            "is",
            "?.",
            "=>",
            "-",
            "+",
            "(",
            "<",
            ">",
            "~",
            "*",
            "/",
            "%",
            "?",
            "=",
            "|",
            ".",
            "[",
        ];
        let matcher = OperatorMatcher::new(DEFAULT_OPERATORS);
        let order: Vec<String> = matcher
            .patterns
            .iter()
            .map(|p| String::from_utf8(p.parts.join(&b' ')).unwrap())
            .collect();
        assert_eq!(order, expected);
        let letters: Vec<(bool, bool)> = matcher
            .patterns
            .iter()
            .map(|p| (p.lookbehind, p.lookahead))
            .collect();
        assert_eq!(letters[0], (true, true));
        assert_eq!(letters[11], (false, false));
    }

    #[test]
    fn splits_operators_on_whitespace_runs() {
        let parts = |op: &str| split_on_whitespace_runs(op.as_bytes());
        assert_eq!(parts("not  in"), [b"not".to_vec(), b"in".to_vec()]);
        assert_eq!(parts(" x"), [b"".to_vec(), b"x".to_vec()]);
        assert_eq!(parts("x \t"), [b"x".to_vec(), b"".to_vec()]);
        assert_eq!(parts("?:"), [b"?:".to_vec()]);
    }
}
