//! `nl2br()` and `strip_tags()` (reference/php/stdlib.md §5.5, §5.6).

use super::{bool_param, str_param, str_weak};
use crate::array::Array;
use crate::convert;
use crate::diag::Diagnostics;
use crate::error::PhpResult;
use crate::param::Arg;
use crate::string::PhpString;
use crate::value::Value;

/// Port of the body of `PHP_FUNCTION(nl2br)` (PHPSRC/ext/standard/string.c): inserts `<br />`
/// (`<br>` when `xhtml` is false) before every line break; `\r\n` and `\n\r` count as one break
/// and both bytes are kept after the tag. Returns `s` itself when there is no line break.
pub fn nl2br_bytes(s: &PhpString, xhtml: bool) -> PhpString {
    let bytes = s.as_bytes();
    if !bytes.iter().any(|&c| c == b'\r' || c == b'\n') {
        return s.clone();
    }
    let tag: &[u8] = if xhtml { b"<br />" } else { b"<br>" };
    // `*(tmp+1)` reads the terminating NUL past the end.
    let at = |i: usize| bytes.get(i).copied().unwrap_or(0);
    let mut out = Vec::with_capacity(bytes.len() + bytes.len() / 4 * tag.len());
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'\r' || c == b'\n' {
            out.extend_from_slice(tag);
            if (c == b'\r' && at(i + 1) == b'\n') || (c == b'\n' && at(i + 1) == b'\r') {
                out.push(c);
                i += 1;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    PhpString::from(out)
}

/// Port of `PHP_FUNCTION(nl2br)` (`nl2br(string $string, bool $use_xhtml = true)`).
pub fn nl2br(
    string: &Value,
    use_xhtml: Option<&Value>,
    diag: &mut dyn Diagnostics,
) -> PhpResult<PhpString> {
    let s = str_param(string, "nl2br", 1, "string", diag)?;
    let xhtml = match use_xhtml {
        Some(v) => bool_param(v, "nl2br", 2, "use_xhtml", diag)?,
        None => true,
    };
    Ok(nl2br_bytes(&s, xhtml))
}

/// C `isspace()` in the C/C.UTF-8 locale: space, `\t`, `\n`, `\v`, `\f`, `\r`.
fn c_isspace(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// Port of `php_tag_find()` (PHPSRC/ext/standard/string.c): normalises the collected tag (ASCII
/// lowercase, leading whitespace dropped, cut at the first whitespace after the name, a `/` right
/// after `<` or right before `>` dropped: `<a href=x>` becomes `<a>`, `</b>` becomes `<b>`) and
/// looks for it with C `strstr()` in the lowercased allow list (which stops at a NUL byte).
fn tag_find(tag: &[u8], set: &[u8]) -> bool {
    if tag.is_empty() {
        return false;
    }
    // The tag buffer always ends with '>', which stops the loop.
    let at = |i: usize| tag.get(i).copied().unwrap_or(0);
    let mut norm: Vec<u8> = Vec::with_capacity(tag.len() + 1);
    let mut state = 0;
    let mut t = 0usize;
    let mut c = at(0).to_ascii_lowercase();
    loop {
        match c {
            b'<' => norm.push(c),
            b'>' => break,
            _ => {
                if !c_isspace(c) {
                    if state == 0 {
                        state = 1;
                    }
                    if c != b'/' || (t > 0 && at(t - 1) != b'<' && at(t + 1) != b'>') {
                        norm.push(c);
                    }
                } else if state == 1 {
                    break;
                }
            }
        }
        t += 1;
        c = at(t).to_ascii_lowercase();
    }
    norm.push(b'>');
    let set = match memchr::memchr(0, set) {
        Some(nul) => &set[..nul],
        None => set,
    };
    memchr::memmem::find(set, &norm).is_some()
}

/// Port of `php_strip_tags_ex()` (PHPSRC/ext/standard/string.c) with `allow_tag_spaces = false`:
/// PHP's tag-stripping state machine (0 text, 1 HTML tag, 2 `<?...?>` processing instruction or
/// PHP code, 3 `<!...>` declaration, 4 `<!--...-->` comment), with its quote, nesting-depth and
/// `<?xml`/`<!DOCTYPE` quirks.
///
/// `allow` is the raw allow list (`<a><b>`), lowercased here; tags whose normalised form occurs in
/// it as a substring are kept. `None` (PHP's `NULL`) strips every tag.
pub fn strip_tags_bytes(s: &[u8], allow: Option<&[u8]>) -> PhpString {
    let buf = s;
    let end = buf.len();
    // C reads `p[1]` and `*(p-1)` on a NUL-terminated copy of the input.
    let at = |i: usize| buf.get(i).copied().unwrap_or(0);
    let allow_lower: Option<Vec<u8>> = allow.map(|a| a.to_ascii_lowercase());
    let mut tbuf: Option<Vec<u8>> = allow_lower.as_ref().map(|_| Vec::new());
    let mut out: Vec<u8> = Vec::with_capacity(end);
    let mut lc: u8 = 0;
    let mut br: i32 = 0;
    let mut depth: i32 = 0;
    let mut in_q: u8 = 0;
    let mut is_xml = false;
    let mut p = 0usize;
    let mut state = 0u8;

    macro_rules! tpush {
        ($c:expr) => {
            if let Some(t) = tbuf.as_mut() {
                t.push($c);
            }
        };
    }
    macro_rules! treset {
        () => {
            if let Some(t) = tbuf.as_mut() {
                t.clear();
            }
        };
    }

    while p < end {
        let c = buf[p];
        match state {
            0 => match c {
                0 => {}
                b'<' => {
                    if in_q != 0 {
                    } else if c_isspace(at(p + 1)) {
                        out.push(c);
                    } else {
                        lc = b'<';
                        state = 1;
                        tpush!(b'<');
                    }
                }
                b'>' => {
                    if depth != 0 {
                        depth -= 1;
                    } else if in_q == 0 {
                        out.push(c);
                    }
                }
                _ => out.push(c),
            },
            1 => match c {
                0 => {}
                b'<' => {
                    if in_q != 0 {
                    } else if c_isspace(at(p + 1)) {
                        tpush!(c);
                    } else {
                        depth += 1;
                    }
                }
                b'>' => {
                    if depth != 0 {
                        depth -= 1;
                    } else if in_q == 0 {
                        lc = b'>';
                        if is_xml && p >= 1 && at(p - 1) == b'-' {
                            // still inside the <?xml ... tag
                        } else {
                            in_q = 0;
                            state = 0;
                            is_xml = false;
                            if let (Some(t), Some(set)) = (tbuf.as_mut(), allow_lower.as_ref()) {
                                t.push(b'>');
                                if tag_find(t, set) {
                                    out.extend_from_slice(t);
                                }
                                t.clear();
                            }
                        }
                    }
                }
                b'"' | b'\'' => {
                    if p != 0 && (in_q == 0 || c == in_q) {
                        in_q = if in_q != 0 { 0 } else { c };
                    }
                    tpush!(c);
                }
                b'!' if p >= 1 && at(p - 1) == b'<' => {
                    // JavaScript & other HTML scripting languages
                    state = 3;
                    lc = c;
                }
                b'?' if p >= 1 && at(p - 1) == b'<' => {
                    br = 0;
                    state = 2;
                }
                _ => tpush!(c),
            },
            2 => match c {
                b'(' => {
                    if lc != b'"' && lc != b'\'' {
                        lc = b'(';
                        br += 1;
                    }
                }
                b')' => {
                    if lc != b'"' && lc != b'\'' {
                        lc = b')';
                        br -= 1;
                    }
                }
                b'>' => {
                    if depth != 0 {
                        depth -= 1;
                    } else if in_q == 0 && br == 0 && p >= 1 && lc != b'"' && at(p - 1) == b'?' {
                        in_q = 0;
                        state = 0;
                        treset!();
                    }
                }
                b'"' | b'\'' => {
                    if p >= 1 && at(p - 1) != b'\\' {
                        if lc == c {
                            lc = 0;
                        } else if lc != b'\\' {
                            lc = c;
                        }
                        if p != 0 && (in_q == 0 || c == in_q) {
                            in_q = if in_q != 0 { 0 } else { c };
                        }
                    }
                }
                b'l' | b'L' => {
                    // `<?xml` is not PHP code: back to an HTML tag.
                    if p > 4
                        && matches!(at(p - 1), b'm' | b'M')
                        && matches!(at(p - 2), b'x' | b'X')
                        && at(p - 3) == b'?'
                        && at(p - 4) == b'<'
                    {
                        state = 1;
                        is_xml = true;
                    }
                }
                _ => {}
            },
            3 => match c {
                b'>' => {
                    if depth != 0 {
                        depth -= 1;
                    } else if in_q == 0 {
                        in_q = 0;
                        state = 0;
                        treset!();
                    }
                }
                b'"' | b'\'' => {
                    if p != 0 && at(p - 1) != b'\\' && (in_q == 0 || c == in_q) {
                        in_q = if in_q != 0 { 0 } else { c };
                    }
                }
                b'-' => {
                    if p >= 2 && at(p - 1) == b'-' && at(p - 2) == b'!' {
                        state = 4;
                    }
                }
                b'E' | b'e' => {
                    // !DOCTYPE exception
                    if p > 6
                        && matches!(at(p - 1), b'p' | b'P')
                        && matches!(at(p - 2), b'y' | b'Y')
                        && matches!(at(p - 3), b't' | b'T')
                        && matches!(at(p - 4), b'c' | b'C')
                        && matches!(at(p - 5), b'o' | b'O')
                        && matches!(at(p - 6), b'd' | b'D')
                    {
                        state = 1;
                    }
                }
                _ => {}
            },
            _ => {
                if c == b'>' && in_q == 0 && p >= 2 && at(p - 1) == b'-' && at(p - 2) == b'-' {
                    in_q = 0;
                    state = 0;
                    treset!();
                }
            }
        }
        p += 1;
    }
    PhpString::from(out)
}

/// Port of `PHP_FUNCTION(strip_tags)` (`strip_tags(string $string, array|string|null
/// $allowed_tags = null)`).
///
/// An array allow list is turned into `<tag1><tag2>...` with PHP string conversion of each element
/// (`Array to string conversion` warnings, `__toString()`); an empty array strips every tag. Other
/// values go through the string rules (ints, floats, bools, `Stringable` objects); anything else
/// throws `TypeError: strip_tags(): Argument #2 ($allowed_tags) must be of type
/// array|string|null, X given`.
pub fn strip_tags(
    string: &Value,
    allowed_tags: Option<&Value>,
    diag: &mut dyn Diagnostics,
) -> PhpResult<PhpString> {
    let s = str_param(string, "strip_tags", 1, "string", diag)?;
    let allow: Option<Vec<u8>> = match allowed_tags {
        None | Some(Value::Null) => None,
        Some(Value::Array(tags)) => allow_list_from_array(tags, diag)?,
        Some(other) => {
            let arg = Arg::new("strip_tags", 2, "allowed_tags").declared("array|string|null");
            match str_weak(other, &arg, diag)? {
                Some(s) => Some(s.to_vec()),
                None => return Err(arg.type_error("of type array|string|null", other)),
            }
        }
    };
    Ok(strip_tags_bytes(&s, allow.as_deref()))
}

/// The array form of the allow list: `<` + `zval_get_string(tag)` + `>` per element; `None` for an
/// empty array (PHP passes `NULL` then).
fn allow_list_from_array(tags: &Array, diag: &mut dyn Diagnostics) -> PhpResult<Option<Vec<u8>>> {
    if tags.is_empty() {
        return Ok(None);
    }
    let mut out = Vec::new();
    for tag in tags.values() {
        let t = convert::to_string(tag, diag)?;
        out.push(b'<');
        out.extend_from_slice(&t);
        out.push(b'>');
    }
    Ok(Some(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(s: &str, allow: Option<&str>) -> String {
        strip_tags_bytes(s.as_bytes(), allow.map(str::as_bytes)).to_string()
    }

    #[test]
    fn strip_tags_examples() {
        assert_eq!(
            st(
                "<p>Hello <b>World</b><br/><!-- c --> <?php echo 1; ?>x</p> a < b",
                None
            ),
            "Hello World x a < b"
        );
        assert_eq!(st("a < b > c", None), "a < b > c");
        assert_eq!(st("1<2", None), "1");
        assert_eq!(st("a<\nb>c", None), "a<\nb>c");
        assert_eq!(st("<P>a</P>", Some("<p>")), "<P>a</P>");
        assert_eq!(st("<b>x</b><i>y</i>", Some("<b>")), "<b>x</b>y");
    }

    #[test]
    fn nl2br_examples() {
        assert_eq!(
            nl2br_bytes(&PhpString::from("a\nb\r\nc\n\rd\re"), true),
            "a<br />\nb<br />\r\nc<br />\n\rd<br />\re"
        );
        assert_eq!(nl2br_bytes(&PhpString::from("x\r"), false), "x<br>\r");
    }
}
