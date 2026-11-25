//! String literal escapes. Twig: `Lexer::stripcslashes()` (TWIG/src/Lexer.php), Twig's own escape processing
//! (not PHP's `stripcslashes()`).

/// Processes the escapes of a string literal's content (after newline normalisation): `\f \n \r \t \v`, `\\`,
/// `\'` and `\"`, `\#{` (kept as `#{`), `\x` with one or two hex digits, octal `\0`-`\777` (modulo 256). For any
/// other escaped character the backslash is dropped. An escaped quote that is not the string's own quote and an
/// escaped character without meaning call `deprecated(character, position)` (the 1-based position of the escaped
/// character in `s`) for Twig's 3.12 deprecation.
///
/// Port of `Lexer::stripcslashes()` (TWIG/src/Lexer.php).
pub(crate) fn stripcslashes(s: &[u8], quote: u8, mut deprecated: impl FnMut(u8, usize)) -> Vec<u8> {
    let mut result = Vec::with_capacity(s.len());
    let length = s.len();
    let mut i = 0;
    while i < length {
        let Some(found) = s[i..].iter().position(|&b| b == b'\\') else {
            result.extend_from_slice(&s[i..]);
            break;
        };
        let pos = i + found;
        result.extend_from_slice(&s[i..pos]);
        i = pos + 1;

        if i >= length {
            result.push(b'\\');
            break;
        }

        let next = s[i];
        match next {
            b'f' => result.push(0x0C),
            b'n' => result.push(b'\n'),
            b'r' => result.push(b'\r'),
            b't' => result.push(b'\t'),
            b'v' => result.push(0x0B),
            b'\\' => result.push(b'\\'),
            b'\'' | b'"' => {
                if next != quote {
                    deprecated(next, i + 1);
                }
                result.push(next);
            }
            b'#' if s.get(i + 1) == Some(&b'{') => {
                result.extend_from_slice(b"#{");
                i += 1;
            }
            b'x' if s.get(i + 1).is_some_and(u8::is_ascii_hexdigit) => {
                i += 1;
                let mut value = hex_digit(s[i]);
                if s.get(i + 1).is_some_and(u8::is_ascii_hexdigit) {
                    i += 1;
                    value = value * 16 + hex_digit(s[i]);
                }
                result.push(value);
            }
            b'0'..=b'7' => {
                let mut value = u32::from(next - b'0');
                let mut digits = 1;
                while digits < 3 && s.get(i + 1).is_some_and(|b| (b'0'..=b'7').contains(b)) {
                    i += 1;
                    value = value * 8 + u32::from(s[i] - b'0');
                    digits += 1;
                }
                // `chr(octdec($octal) % 256)`
                result.push((value % 256) as u8);
            }
            _ => {
                deprecated(next, i + 1);
                result.push(next);
            }
        }
        i += 1;
    }
    result
}

fn hex_digit(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'f' => byte - b'a' + 10,
        _ => byte - b'A' + 10,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unescape(s: &str, quote: u8) -> (Vec<u8>, Vec<(char, usize)>) {
        let mut deprecations = Vec::new();
        let value = stripcslashes(s.as_bytes(), quote, |c, position| {
            deprecations.push((char::from(c), position))
        });
        (value, deprecations)
    }

    #[test]
    fn processes_twig_escapes() {
        // reference/twig/syntax.md §2.9 (verified on the oracle).
        assert_eq!(
            unescape(r"\a\b\e", b'\''),
            (b"abe".to_vec(), vec![('a', 2), ('b', 4), ('e', 6)])
        );
        assert_eq!(unescape(&["\\", "u0041"].concat(), b'"').0, b"u0041");
        assert_eq!(unescape(r"\X41", b'\'').0, b"X41");
        assert_eq!(unescape(r"\8", b'\'').0, b"8");
        assert_eq!(unescape(r"\x41\x4a\x4", b'\'').0, b"AJ\x04");
        assert_eq!(unescape(r"\x", b'\'').0, b"x");
        assert_eq!(unescape(r"\12a", b'\'').0, b"\na");
        assert_eq!(unescape(r"\1234", b'\'').0, b"S4");
        assert_eq!(unescape(r"\777", b'\'').0, b"\xff");
        assert_eq!(unescape(r"\400", b'\'').0, b"\x00");
        assert_eq!(unescape(r"\#{x}", b'\''), (b"#{x}".to_vec(), vec![]));
        assert_eq!(unescape(r"\#", b'\''), (b"#".to_vec(), vec![('#', 2)]));
        assert_eq!(
            unescape(r#"a\'b"#, b'"'),
            (b"a'b".to_vec(), vec![('\'', 3)])
        );
        assert_eq!(
            unescape(r#"a\"b"#, b'\''),
            (b"a\"b".to_vec(), vec![('"', 3)])
        );
        assert_eq!(unescape(r"a\'b", b'\''), (b"a'b".to_vec(), vec![]));
        assert_eq!(unescape(r"\f\n\r\t\v\\", b'\'').0, b"\x0C\n\r\t\x0B\\");
        assert_eq!(unescape("trailing\\", b'\'').0, b"trailing\\");
    }
}
