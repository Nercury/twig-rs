//! The parse-time validation of constant `matches` patterns, backed by the PCRE port.

use crate::extension::RegexValidator;

/// Validates the constant right operand of `matches` like `MatchesBinary::__construct()`: `preg_match($regexp, '')`
/// with an error handler turning the first PHP warning into `SyntaxError(sprintf('Regexp "%s" passed to "matches" is
/// not valid: %s.', $regexp, substr($m, 14)))` (TWIG/src/Node/Expression/Binary/MatchesBinary.php). The environment
/// installs it in its extension set.
#[derive(Debug, Clone, Copy, Default)]
pub struct PcreRegexValidator;

impl RegexValidator for PcreRegexValidator {
    fn validate(&self, pattern: &[u8]) -> Option<String> {
        let mut first: Option<twig_pcre::Diagnostic> = None;
        twig_pcre::with_thread_state(|pcre| {
            pcre.preg_match(pattern, b"", &mut |diagnostic| {
                first.get_or_insert(diagnostic);
            });
        });
        first.map(|diagnostic| {
            // substr($m, 14): the message without "preg_match(): "
            let text = diagnostic.text();
            String::from_utf8_lossy(text.get(14..).unwrap_or_default()).into_owned()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_the_preg_match_warning() {
        let validator = PcreRegexValidator;
        assert_eq!(validator.validate(b"/^a+$/"), None);
        assert_eq!(
            validator.validate(b"/(/").as_deref(),
            Some("Compilation failed: missing closing parenthesis at offset 1")
        );
        assert_eq!(
            validator.validate(b"abc").as_deref(),
            Some("Delimiter must not be alphanumeric, backslash, or NUL byte")
        );
    }
}
