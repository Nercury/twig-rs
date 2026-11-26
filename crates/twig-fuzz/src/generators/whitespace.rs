//! Generator of the `whitespace` family: what the lexer does with the bytes between and around
//! tags (Twig's `Lexer`, TWIG/src/Lexer.php).
//!
//! Every case renders a template built from random text and tags, where each delimiter gets a
//! random whitespace control (`-` trims all whitespace, `~` only `" \t\0\x0B"`) and random
//! whitespace inside it:
//!
//! - text: spaces, tabs, LF, CRLF, lone CR, NUL, `\v`, `\f`, NBSP, U+0085, U+2028, U+3000, BOMs,
//!   raw bytes `0x85`/`0xA0`/`0xFF`, stray closing delimiters (`}}`, `%}`, `#}`, `-}}`), letters;
//! - prints with string literals that hold whitespace, nested interpolations and braces inside
//!   them (the lexer's string and interpolation states), inline comments (`#` and the
//!   documentation form `##`), operators spelled over several whitespace characters (`not   in`,
//!   `is\nnot`, `starts  with`, `divisible\tby`), multi-line ternaries; in a few cases NBSP,
//!   U+3000, U+0085 or NUL inside the tags (name characters or unexpected for PCRE without `/u`);
//! - tags (`if`, `for`, `set` and captures, `apply` with filters that make whitespace visible,
//!   `with`, blocks, macros, `include`, `source()`) with whitespace control on both delimiters
//!   and newlines inside them, so the newline a `%}` eats (`%}(?:\r\n?|\n)?`) and the one `-%}`
//!   eats are both exercised;
//! - comments: `{# #}` with trims, documentation comments (`{## ##}`, `{## #}`, `{##- -##}`), the
//!   empty comment `{##}`, `{###}`, comments holding tags, `#` and `}`;
//! - `{% verbatim %}` blocks with trims on all four delimiters (`lex_block_raw`, `lex_raw_data`:
//!   neither eats the newline after `%}`) and tags inside;
//! - `{% line N %}` directives (`lex_block_line`: no trim marker before `%}`, so `{% line 3 -%}`
//!   is an unknown `line` tag) followed by errors that show the line;
//! - child templates whose text outside blocks is blank or not for `TextNode::isBlank()`
//!   (`ctype_space()` knows `\v` and `\f` but not NUL; a leading BOM is allowed);
//! - whole templates converted to CRLF or CR line endings, a leading BOM, empty templates and
//!   huge runs of text, comments and verbatim content;
//! - in about one case of seven, a syntax of its own (the case's `"lexer"` options, `new
//!   \Twig\Lexer($twig, $options)`): other block, print and comment delimiters (also ones that
//!   share a prefix, hold a space or a `#`), other whitespace-control characters (an empty one
//!   trims on every tag), another set of `whitespace_line_chars` and another interpolation opener.
//!   Every delimiter the case writes uses that syntax; the text and the string literals keep
//!   Twig's delimiters, which are plain text then.
//!
//! The data is JSON; templates that are not valid UTF-8 go to `templates_b64`. Cases end with a
//! probe now and then (an undefined variable under `strict_variables`, a syntax error) whose
//! line shows how the lexer counted the lines before it. Left out (`reference/deviations.md`):
//! `{% line 0 %}`, since a syntax error at line 0 makes PHP read past the token stream, and `\r`
//! escapes in string literals, since a CR in the compiled PHP moves the lines PHP reports
//! (accepted deviation D2; CR bytes reach the filters through the data instead), and the lexer
//! options whose Twig patterns misbehave (`reference/deviations.md`: trim characters that are PCRE
//! metacharacters, `whitespace_line_chars` with escapes or class syntax, empty interpolation
//! delimiters, which make Twig loop forever).
//!
//! [`generate`] is deterministic: case `i` of seed `s` depends only on `(s, i)` ([`case`]). Ids
//! are `ws-<seed>-<i>`. [`batch`] runs every case in a PHP process of its own.

use super::casekit::{self, Builder, Rng};
use crate::kit::json::Json;

/// The family name.
pub const FAMILY: &str = "whitespace";

const SALT: u64 = 0x0057_5350_4143;

/// `count` cases of `seed`, as a batch (process isolation).
pub fn batch(seed: u64, count: usize) -> Json {
    casekit::batch(generate(seed, count))
}

/// `count` cases of `seed`.
pub fn generate(seed: u64, count: usize) -> Vec<Json> {
    (0..count as u64).map(|i| case(seed, i)).collect()
}

/// The salt of the syntax a case draws from a generator of its own, so that the cases with Twig's
/// syntax are the same as before lexer options existed.
const SYNTAX_SALT: u64 = 0x004C_4558_4552;

/// Case `index` of `seed` (id `ws-<seed>-<index>`).
pub fn case(seed: u64, index: u64) -> Json {
    let rng = Rng::new(casekit::case_seed(seed, index, SALT));
    let syntax = Syntax::pick(&mut Rng::new(casekit::case_seed(
        seed,
        index,
        SALT ^ SYNTAX_SALT,
    )));
    Gen::new(rng, syntax).build(format!("ws-{seed}-{index}"))
}

/// The template syntax of a case: Twig's, or one of its own given as lexer options.
///
/// The options avoid what `reference/deviations.md` lists for Twig's lexer: the trim characters
/// are no PCRE metacharacters (Twig puts them into the `endverbatim` pattern unquoted), contain no
/// `#` or whitespace (that pattern is in extended mode), `whitespace_line_chars` holds no escape,
/// range or class syntax and is not empty, and the interpolation delimiters are not empty. The
/// interpolation closer stays `}`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Syntax {
    block: [&'static str; 2],
    var: [&'static str; 2],
    comment: [&'static str; 2],
    trim: &'static str,
    line_trim: &'static str,
    /// `None`: Twig's `' \t\0\x0B'` (not written into the options).
    line_chars: Option<&'static str>,
    interpolation: &'static str,
}

impl Syntax {
    /// Twig's defaults (`Lexer::__construct()`).
    const TWIG: Syntax = Syntax {
        block: ["{%", "%}"],
        var: ["{{", "}}"],
        comment: ["{#", "#}"],
        trim: "-",
        line_trim: "~",
        line_chars: None,
        interpolation: "#{",
    };

    /// Twig's syntax in six cases of seven, otherwise one with some parts changed.
    fn pick(rng: &mut Rng) -> Syntax {
        let mut syntax = Syntax::TWIG;
        if !rng.chance(15) {
            return syntax;
        }
        while syntax == Syntax::TWIG {
            if rng.chance(55) {
                syntax.block = *rng.weighted(&[
                    (6, &["<%", "%>"]),
                    (4, &["[%", "%]"]),
                    (3, &["{@", "@}"]),
                    (2, &["<?", "?>"]),
                    (2, &["{%%", "%%}"]),
                    // a closer that starts like a comment closer, a space Twig's patterns ignore
                    (1, &["{%", "#}"]),
                    (1, &["{ %", "% }"]),
                    (1, &["{%", "%}"]),
                ]);
            }
            if rng.chance(55) {
                syntax.var = *rng.weighted(&[
                    (5, &["${", "}"]),
                    (4, &["[[", "]]"]),
                    (3, &["{=", "=}"]),
                    (2, &["{{{", "}}}"]),
                    // the block opener's prefix (`<%`, alternatives are tried in order)
                    (2, &["<%=", "%>"]),
                    (1, &["{{", "}}"]),
                ]);
            }
            if rng.chance(45) {
                syntax.comment = *rng.weighted(&[
                    (5, &["<#", "#>"]),
                    (4, &["[#", "#]"]),
                    (3, &["{*", "*}"]),
                    (2, &["/*", "*/"]),
                    (2, &["<!--", "-->"]),
                    (1, &["{#", "#}"]),
                ]);
            }
            if rng.chance(40) {
                syntax.trim = rng.weighted(&[(5, "="), (4, "!"), (3, ":"), (2, "@"), (1, "")]);
            }
            if rng.chance(35) {
                syntax.line_trim = rng.weighted(&[(5, "!"), (4, ":"), (3, "@"), (2, "=")]);
            }
            if syntax.line_trim == syntax.trim {
                syntax.line_trim = if syntax.trim == "~" { "!" } else { "~" };
            }
            if rng.chance(30) {
                syntax.line_chars =
                    Some(rng.pick_str(&[" ", "\t", " \t", " x", "\u{b}\u{c} ", "\t."]));
            }
            if rng.chance(25) {
                syntax.interpolation = rng.pick_str(&["${", "%{", "@{"]);
            }
        }
        syntax
    }

    /// The case's `"lexer"` field: the options that differ from Twig's.
    fn options(&self) -> Option<Json> {
        let pair = |p: [&str; 2]| Json::Array(vec![Json::str(p[0]), Json::str(p[1])]);
        let twig = Syntax::TWIG;
        let mut fields = Vec::new();
        if self.block != twig.block {
            fields.push(("tag_block".to_owned(), pair(self.block)));
        }
        if self.var != twig.var {
            fields.push(("tag_variable".to_owned(), pair(self.var)));
        }
        if self.comment != twig.comment {
            fields.push(("tag_comment".to_owned(), pair(self.comment)));
        }
        if self.trim != twig.trim {
            fields.push(("whitespace_trim".to_owned(), Json::str(self.trim)));
        }
        if self.line_trim != twig.line_trim {
            fields.push(("whitespace_line_trim".to_owned(), Json::str(self.line_trim)));
        }
        if let Some(chars) = self.line_chars {
            fields.push(("whitespace_line_chars".to_owned(), Json::str(chars)));
        }
        if self.interpolation != twig.interpolation {
            fields.push((
                "interpolation".to_owned(),
                Json::Array(vec![Json::str(self.interpolation), Json::str("}")]),
            ));
        }
        (!fields.is_empty()).then_some(Json::Object(fields))
    }

    /// The whitespace control `ctl` (`""`, `-` or `~`, as [`Gen::ctl`] gives it) in this syntax.
    fn control(&self, ctl: &str) -> &'static str {
        match ctl {
            "-" => self.trim,
            "~" => self.line_trim,
            _ => "",
        }
    }

    /// `fragment`, written with Twig's delimiters, in this syntax: openers (`{{`, `{%`, `{#`),
    /// closers (`}}`, `%}`, `#}`) and the `-` / `~` right after an opener or right before a closer
    /// (also before the `#` of a documentation comment's `##}`). Everything else stays.
    fn translate(&self, fragment: &[u8]) -> Vec<u8> {
        if *self == Syntax::TWIG {
            return fragment.to_vec();
        }
        let closer = |rest: &[u8]| -> Option<&'static str> {
            if rest.starts_with(b"}}") {
                Some(self.var[1])
            } else if rest.starts_with(b"%}") {
                Some(self.block[1])
            } else if rest.starts_with(b"#}") {
                Some(self.comment[1])
            } else {
                None
            }
        };
        let mut out = Vec::with_capacity(fragment.len() + 8);
        let mut i = 0;
        while i < fragment.len() {
            let rest = &fragment[i..];
            let opener = if rest.starts_with(b"{{") {
                Some(self.var[0])
            } else if rest.starts_with(b"{%") {
                Some(self.block[0])
            } else if rest.starts_with(b"{#") {
                Some(self.comment[0])
            } else {
                None
            };
            if let Some(opener) = opener {
                out.extend_from_slice(opener.as_bytes());
                i += 2;
                if let Some(&c @ (b'-' | b'~')) = fragment.get(i) {
                    out.extend_from_slice(
                        self.control(if c == b'-' { "-" } else { "~" }).as_bytes(),
                    );
                    i += 1;
                }
                continue;
            }
            if let [c @ (b'-' | b'~'), after @ ..] = rest {
                let after = after
                    .strip_prefix(b"#")
                    .filter(|a| a.starts_with(b"#}"))
                    .unwrap_or(after);
                if closer(after).is_some() {
                    out.extend_from_slice(
                        self.control(if *c == b'-' { "-" } else { "~" }).as_bytes(),
                    );
                    i += 1;
                    continue;
                }
            }
            if let Some(closer) = closer(rest) {
                out.extend_from_slice(closer.as_bytes());
                i += 2;
                continue;
            }
            out.push(fragment[i]);
            i += 1;
        }
        out
    }

    /// A string literal of [`Gen::expr`] with this syntax's interpolation opener in place of every
    /// `#{` that is not escaped.
    fn interpolate(&self, literal: &str) -> String {
        if self.interpolation == "#{" {
            return literal.to_owned();
        }
        let mut out = String::with_capacity(literal.len());
        let mut rest = literal;
        while let Some(at) = rest.find("#{") {
            out.push_str(&rest[..at]);
            if rest[..at].ends_with('\\') {
                out.push_str("#{");
            } else {
                out.push_str(self.interpolation);
            }
            rest = &rest[at + 2..];
        }
        out.push_str(rest);
        out
    }
}

/// Whitespace bytes of every kind the lexer distinguishes.
const SPACES: &[&[u8]] = &[
    b" ", b" ", b"  ", b"\t", b"\n", b"\n", b"\n", b"\r\n", b"\r", b"\0", b"\x0b", b"\x0c", b" \n",
    b"\n ", b"\t\n", b"\n\n",
];

/// Text that is not whitespace (never ending with `{`, which would merge with a following tag).
const WORDS: &[&str] = &[
    "a", "b", "xy", "Z", "<p>", "</p>", "&", ".", "-", "~", "}", "#", "%", "}}", "%}", "#}", "-}}",
    "~%}", "-#}", "{ {x", "{-y", "x}", "é", "1", "=", "|",
];

/// Characters that look like whitespace to someone, but not to PCRE without `/u`.
const ODD_SPACES: &[&[u8]] = &[
    "\u{a0}".as_bytes(),
    "\u{85}".as_bytes(),
    "\u{2028}".as_bytes(),
    "\u{3000}".as_bytes(),
    "\u{feff}".as_bytes(),
    "\u{200b}".as_bytes(),
    b"\x85",
    b"\xa0",
    b"\xff",
];

/// Whitespace inside a tag, between the delimiter and its content.
const INNER: &[(u32, &str)] = &[
    (50, " "),
    (14, ""),
    (8, "  "),
    (8, "\n"),
    (5, "\t"),
    (4, "\r\n"),
    (4, " \n "),
    (2, "\x0c"),
    (2, "\x0b"),
    (2, "\r"),
    (1, "\n\n"),
];

/// Characters inside a tag that are not whitespace for PCRE without `/u`: name characters (bytes
/// from 0x7f) or an unexpected character (a few cases only, they fail to compile).
const ODD_INNER: &[&str] = &["\u{a0}", "\u{3000}", "\0", "\u{85}"];

struct Gen {
    rng: Rng,
    syntax: Syntax,
    counter: usize,
    blocks: Vec<String>,
    captures: Vec<String>,
    loop_vars: Vec<String>,
    uses_inc: bool,
    strict: bool,
    odd_inner: bool,
}

impl Gen {
    fn new(rng: Rng, syntax: Syntax) -> Gen {
        Gen {
            rng,
            syntax,
            counter: 0,
            blocks: Vec::new(),
            captures: Vec::new(),
            loop_vars: Vec::new(),
            uses_inc: false,
            strict: false,
            odd_inner: false,
        }
    }

    fn fresh(&mut self, prefix: &str) -> String {
        self.counter += 1;
        format!("{prefix}{}", self.counter)
    }

    fn build(mut self, id: String) -> Json {
        let mut b = Builder::new(id);
        self.strict = self.rng.chance(50);
        self.odd_inner = self.rng.chance(5);
        let scenario = self
            .rng
            .weighted(&[(70, 0u8), (14, 1), (8, 2), (5, 3), (3, 4)]);
        let main = match scenario {
            // a child template: text outside blocks, blank or not
            1 => {
                let (child, base) = self.child_and_base();
                b.template("base.twig", base);
                b.main = Some("child.twig".to_owned());
                child
            }
            // huge text, comments or verbatim content
            2 => self.huge(),
            // an empty or nearly empty template
            3 => {
                let empty = self
                    .rng
                    .pick(&[
                        &b""[..],
                        b" ",
                        b"\n",
                        b"\r\n",
                        b"\0",
                        b"{##}",
                        b"{#-#}",
                        b"{{-''-}}",
                        b"{%- if true -%}{%- endif -%}",
                        b"\xef\xbb\xbf",
                    ])
                    .to_vec();
                self.syntax.translate(&empty)
            }
            // a template made of one construct in many spellings
            4 => self.spellings(),
            _ => self.body(0),
        };
        let mut main = main;
        // (not in a child template, where the text before it is content outside blocks)
        if scenario != 1 && scenario != 2 && self.rng.chance(30) {
            self.probe(&mut main);
        }
        let main = self.line_endings(main);
        let main = if self.rng.chance(4) {
            let mut with_bom = b"\xef\xbb\xbf".to_vec();
            with_bom.extend_from_slice(&main);
            with_bom
        } else {
            main
        };
        let main_name = b.main.clone().unwrap_or_else(|| "index.twig".to_owned());
        b.template(&main_name, main);
        if self.uses_inc {
            let inc = self.inc();
            let inc = self.line_endings(inc);
            b.template("inc.twig", inc);
        }
        // the main template last: base.twig and inc.twig are loaded first
        b.templates.sort_by_key(|(name, _)| *name == main_name);
        b.data_json = vec![
            ("s".to_owned(), Json::str(" a  b ")),
            ("t".to_owned(), Json::str("\t x\n")),
            ("r".to_owned(), Json::str("\r\n x \r\x0b\x0c\0")),
            ("n".to_owned(), Json::int(3)),
            ("e".to_owned(), Json::str("")),
            (
                "l".to_owned(),
                Json::Array(vec![Json::int(1), Json::int(2)]),
            ),
        ];
        let autoescape = self.rng.weighted(&[(60, 0u8), (30, 1), (6, 2), (4, 3)]);
        b.option(
            "autoescape",
            match autoescape {
                0 => Json::Bool(false),
                1 => Json::str("html"),
                2 => Json::str("js"),
                _ => Json::str("html_attr"),
            },
        );
        b.option("strict_variables", Json::Bool(self.strict));
        if self.rng.chance(10) {
            b.option("debug", Json::Bool(true));
        }
        if self.rng.chance(10) {
            b.option("use_yield", Json::Bool(true));
        }
        b.lexer = self.syntax.options();
        b.build()
    }

    // ---- text -----------------------------------------------------------------------------

    fn text(&mut self, out: &mut Vec<u8>) {
        let n = self.rng.range(1, 5);
        for _ in 0..n {
            match self.rng.below(100) {
                0..=54 => out.extend_from_slice(self.rng.pick_bytes(SPACES)),
                55..=91 => {
                    let word = self.rng.pick_str(WORDS);
                    out.extend_from_slice(&self.syntax.translate(word.as_bytes()));
                }
                _ => out.extend_from_slice(self.rng.pick_bytes(ODD_SPACES)),
            }
        }
    }

    /// Only whitespace (for the text around blocks of a child template).
    fn blank(&mut self, out: &mut Vec<u8>) {
        // `TextNode::isBlank()` allows a BOM only at the start of the text
        if self.rng.chance(12) {
            out.extend_from_slice(b"\xef\xbb\xbf");
        }
        let n = self.rng.range(0, 4);
        for _ in 0..n {
            match self.rng.below(100) {
                0..=96 => out.extend_from_slice(self.rng.pick_bytes(SPACES)),
                97 => out.extend_from_slice(b"\xef\xbb\xbf"),
                _ => out.extend_from_slice(self.rng.pick_bytes(ODD_SPACES)),
            }
        }
    }

    // ---- delimiters -----------------------------------------------------------------------

    fn ctl(&mut self) -> &'static str {
        self.rng.weighted(&[(50, ""), (30, "-"), (20, "~")])
    }

    fn inner(&mut self) -> &'static str {
        if self.odd_inner && self.rng.chance(10) {
            return self.rng.pick_str(ODD_INNER);
        }
        self.rng.weighted(INNER)
    }

    /// `{%` + trim + space + `content` + space + trim + `%}` (in the case's syntax).
    fn tag(&mut self, content: &str) -> String {
        let (l, sl, sr, r) = (self.ctl(), self.inner(), self.inner(), self.ctl());
        let [open, close] = self.syntax.block;
        let (l, r) = (self.syntax.control(l), self.syntax.control(r));
        format!("{open}{l}{sl}{content}{sr}{r}{close}")
    }

    /// `{{` + trim + space + `expr` + space + trim + `}}`.
    fn print_of(&mut self, expr: &str) -> String {
        let (l, sl, sr, r) = (self.ctl(), self.inner(), self.inner(), self.ctl());
        // a space keeps `-`/`~` of the delimiter apart from an operator of the expression
        let sl = if sl.is_empty() && expr.starts_with(['-', '~']) {
            " "
        } else {
            sl
        };
        let sr = if sr.is_empty() && expr.ends_with(['-', '~']) {
            " "
        } else {
            sr
        };
        let [open, close] = self.syntax.var;
        let (l, r) = (self.syntax.control(l), self.syntax.control(r));
        format!("{open}{l}{sl}{expr}{sr}{r}{close}")
    }

    // ---- bodies ---------------------------------------------------------------------------

    fn body(&mut self, depth: usize) -> Vec<u8> {
        let mut out = Vec::new();
        let n = if depth == 0 {
            self.rng.range(2, 7)
        } else {
            self.rng.range(1, 3)
        };
        for _ in 0..n {
            if self.rng.chance(75) {
                self.text(&mut out);
            }
            self.item(&mut out, depth);
        }
        if self.rng.chance(70) {
            self.text(&mut out);
        }
        out
    }

    fn item(&mut self, out: &mut Vec<u8>, depth: usize) {
        let nested = depth < 2;
        let kind = self.rng.weighted(&[
            (30, 0u8),
            (if nested { 10 } else { 0 }, 1),
            (if nested { 7 } else { 0 }, 2),
            (if nested { 6 } else { 0 }, 3),
            (4, 4),
            (if nested { 6 } else { 0 }, 5),
            (if nested { 3 } else { 0 }, 6),
            (if nested { 4 } else { 0 }, 7),
            (if depth == 0 { 3 } else { 0 }, 8),
            (9, 9),
            (5, 10),
            (5, 11),
            (3, 12),
            (4, 13),
            (3, 14),
            (1, 15),
        ]);
        match kind {
            0 => {
                let expr = self.expr();
                let print = self.print_of(&expr);
                out.extend_from_slice(print.as_bytes());
            }
            1 => self.if_tag(out, depth),
            2 => self.for_tag(out, depth),
            3 => self.capture(out, depth),
            4 => {
                let name = self.fresh("v");
                let expr = self.expr();
                let eq = self.rng.pick_str(&["=", " = ", "\n=\n", "  =\t"]);
                let tag = self.tag(&format!("set {name}{eq}{expr}"));
                out.extend_from_slice(tag.as_bytes());
                if self.rng.chance(60) {
                    let print = self.print_of(&format!("'[' ~ {name} ~ ']'"));
                    out.extend_from_slice(print.as_bytes());
                }
            }
            5 => self.apply(out, depth),
            6 => {
                let vars =
                    self.rng
                        .pick_str(&["", " {w: ' w '}", " {w: \"\\n\"} only", " {s: '  '}"]);
                let open = self.tag(&format!("with{vars}"));
                out.extend_from_slice(open.as_bytes());
                let inner = self.body(depth + 1);
                out.extend_from_slice(&inner);
                if vars.contains('w') {
                    let print = self.print_of("w ?? 'none'");
                    out.extend_from_slice(print.as_bytes());
                }
                let close = self.tag("endwith");
                out.extend_from_slice(close.as_bytes());
            }
            7 => {
                let name = self.fresh("b");
                let open = self.tag(&format!("block {name}"));
                out.extend_from_slice(open.as_bytes());
                let inner = self.body(depth + 1);
                out.extend_from_slice(&inner);
                let end = if self.rng.chance(30) {
                    format!("endblock {name}")
                } else {
                    "endblock".to_owned()
                };
                let close = self.tag(&end);
                out.extend_from_slice(close.as_bytes());
                self.blocks.push(name);
            }
            8 => self.macro_def(out),
            9 => self.comment(out),
            10 => self.doc_comment(out),
            11 => self.verbatim(out),
            12 => self.line(out),
            13 => self.include(out),
            14 => {
                // a reference to something defined earlier
                if let Some(name) = self.blocks.last().cloned().filter(|_| self.rng.chance(50)) {
                    let print = self.print_of(&format!("block('{name}')"));
                    out.extend_from_slice(print.as_bytes());
                } else if let Some(name) = self.captures.last().cloned() {
                    let print = self.print_of(&format!("'<' ~ {name} ~ '>'"));
                    out.extend_from_slice(print.as_bytes());
                } else {
                    let print = self.print_of("_charset");
                    out.extend_from_slice(print.as_bytes());
                }
            }
            _ => {
                // now and then a stray brace right before a print: `{{{`
                if self.rng.chance(30) {
                    out.push(b'{');
                }
                let expr = self.expr();
                let print = self.print_of(&expr);
                out.extend_from_slice(print.as_bytes());
            }
        }
    }

    fn expr(&mut self) -> String {
        let var = self.var();
        match self.rng.below(12) {
            0 | 1 => {
                let literal = self.rng.pick_str(&[
                    "'  a  '",
                    "' '",
                    "''",
                    "'a\\tb'",
                    "\"\\n x \"",
                    "'\\x0b|\\f|\\0'",
                    "\"#{ s }\"",
                    "\"#{s}|#{  n  }\"",
                    "\"#{\nn\n}\"",
                    "\"a\nb\"",
                    "'a\r\nb'",
                    "'a\rb'",
                    "\" #{ ' x ' } \"",
                    "'{{ x }}'",
                    "'{% if %}'",
                    "'#}'",
                    "'-}}'",
                    // the lexer's string and interpolation states
                    "\"#{ \"in #{ n } ner\" }\"",
                    "\"#{ '}' }|#{ \"}\" }\"",
                    "\"#{ {a: '}'}|length }\"",
                    "\"#{ [n, '#{'] |join }\"",
                    "\"\\#{n}|#{n}\"",
                    "\"a#b##{n}#\"",
                    "\"#{n}#{n}\"",
                    "\"#{\n\tn\r\n}\"",
                    "\"#{ n # comment }\n }\"",
                    "'#{n}'",
                    "\"\\101\\x41\\7\"",
                    "\"#{ n ~ \"#{ s|trim }\" }\"",
                    "\"#{ (n) }#{ n|abs }\"",
                ]);
                self.syntax.interpolate(literal)
            }
            2 | 3 => var,
            4 | 5 => {
                let filter = self.rng.pick_str(&[
                    "trim",
                    "trim(side='left')",
                    "trim(side = 'right')",
                    "trim(' ')",
                    "replace({' ': '_', \"\\n\": '/', \"\\t\": '>'})",
                    "nl2br",
                    "split(' ')|join('|')",
                    "length",
                    "upper",
                    "striptags",
                    "json_encode",
                    "e('js')",
                    "url_encode",
                    "spaceless",
                ]);
                // CR bytes come from the data (`r`): a `\r` escape puts a CR into the compiled PHP,
                // which moves the error lines PHP reports (accepted deviation D2)
                let input =
                    self.rng
                        .pick_str(&["s", "t", "e", "'  <a>  </a>  '", "\"\\n x \\t\"", "r"]);
                format!("{input}|{filter}")
            }
            6 => self
                .rng
                .pick_str(&[
                    "n-1",
                    "n - -1",
                    "-n",
                    "n~'x'",
                    "n ~ '|' ~ n",
                    "(n)",
                    "n\n+\n1",
                    "n\t*\t2",
                    "- - n",
                    "n--1",
                ])
                .to_owned(),
            7 => {
                let inner = self.rng.pick_str(&[
                    "n # a comment\n",
                    "n ## a doc comment\n",
                    "# c\n n",
                    "n # c }} %} #}\n",
                    "n #\n",
                    "(n # inside\n + 1)",
                    "[1, # c\n 2]|join",
                    "n # c\r\n",
                ]);
                inner.to_owned()
            }
            8 => self
                .rng
                .pick_str(&[
                    "'ab' starts  with 'a'",
                    "1 not   in [1, 2]",
                    "n is   not  null",
                    "n is divisible   by(3)",
                    "n is same\tas(3)",
                    "'ab' ends\nwith 'b'",
                    "n b-and\t1",
                    "not   n",
                    "n is\nnot\nempty",
                    "s matches  '/\\\\s/'",
                    "n\nis\ndefined",
                    "n is   same as(3)",
                    "1 not\nin [2]",
                    "not\t(n)",
                ])
                .to_owned(),
            9 => self
                .rng
                .pick_str(&[
                    "n\n?\n'y'\n:\n'z'",
                    "e ?: '<empty>'",
                    "x ?? '<undefined>'",
                    "n\r\n?\r\n'cr'\r\n:\r\n'lf'",
                    "{a: ' a ', 'b': \"\\tb\"}|join(',')",
                    "[\n' ',\n\"\\n\"\n]|join('.')",
                ])
                .to_owned(),
            10 => {
                if self.rng.chance(50) {
                    format!("{var} ~ '|'")
                } else {
                    format!("'|' ~ {var}")
                }
            }
            _ => self
                .rng
                .pick_str(&["s|length", "t|length", "s|trim|length", "l|length", "n"])
                .to_owned(),
        }
    }

    fn var(&mut self) -> String {
        if let Some(v) = self
            .loop_vars
            .last()
            .cloned()
            .filter(|_| self.rng.chance(40))
        {
            return v;
        }
        self.rng
            .pick_str(&["s", "t", "n", "e", "l|join(' ')", "l|join(\"\\n\")", "r"])
            .to_owned()
    }

    fn cond(&mut self) -> String {
        self.rng
            .pick_str(&[
                "n", "e", "s", "true", "false", "n > 2", "l|length", "not e", "n\n>\n1", "t|trim",
            ])
            .to_owned()
    }

    // ---- tags -----------------------------------------------------------------------------

    fn if_tag(&mut self, out: &mut Vec<u8>, depth: usize) {
        let cond = self.cond();
        let open = self.tag(&format!("if {cond}"));
        out.extend_from_slice(open.as_bytes());
        let body = self.body(depth + 1);
        out.extend_from_slice(&body);
        if self.rng.chance(30) {
            let cond = self.cond();
            let elseif = self.tag(&format!("elseif {cond}"));
            out.extend_from_slice(elseif.as_bytes());
            let body = self.body(depth + 1);
            out.extend_from_slice(&body);
        }
        if self.rng.chance(50) {
            let tag = self.tag("else");
            out.extend_from_slice(tag.as_bytes());
            let body = self.body(depth + 1);
            out.extend_from_slice(&body);
        }
        let close = self.tag("endif");
        out.extend_from_slice(close.as_bytes());
    }

    fn for_tag(&mut self, out: &mut Vec<u8>, depth: usize) {
        let var = self.fresh("x");
        let seq = self
            .rng
            .pick_str(&["l", "1..3", "[]", "[' ', \"\\n\"]", "'ab'|split('')"]);
        let open = self.tag(&format!("for {var} in {seq}"));
        out.extend_from_slice(open.as_bytes());
        self.loop_vars.push(var.clone());
        if self.rng.chance(50) {
            self.text(out);
        }
        let print = self.print_of(&var);
        out.extend_from_slice(print.as_bytes());
        let body = self.body(depth + 1);
        out.extend_from_slice(&body);
        if self.rng.chance(30) {
            let print = self.print_of("loop.index");
            out.extend_from_slice(print.as_bytes());
        }
        self.loop_vars.pop();
        if self.rng.chance(30) {
            let tag = self.tag("else");
            out.extend_from_slice(tag.as_bytes());
            let body = self.body(depth + 1);
            out.extend_from_slice(&body);
        }
        let close = self.tag("endfor");
        out.extend_from_slice(close.as_bytes());
    }

    fn capture(&mut self, out: &mut Vec<u8>, depth: usize) {
        let name = self.fresh("c");
        let open = self.tag(&format!("set {name}"));
        out.extend_from_slice(open.as_bytes());
        let body = self.body(depth + 1);
        out.extend_from_slice(&body);
        let close = self.tag("endset");
        out.extend_from_slice(close.as_bytes());
        let shown = self.rng.pick_str(&[
            "'[' ~ NAME ~ ']'",
            "NAME|json_encode",
            "NAME|length",
            "NAME|replace({' ': '.', \"\\n\": '/'})",
        ]);
        let print = self.print_of(&shown.replace("NAME", &name));
        out.extend_from_slice(print.as_bytes());
        self.captures.push(name);
    }

    fn apply(&mut self, out: &mut Vec<u8>, depth: usize) {
        let filter = self.rng.pick_str(&[
            "json_encode",
            "replace({' ': '.', \"\\n\": '/', \"\\t\": '>', \"\\0\": '0', \"\\x0b\": 'v', \"\\f\": 'f'})",
            "trim",
            "upper",
            "nl2br",
            "spaceless",
            "e('js')",
            "url_encode",
            "trim|json_encode",
            "split(\"\\n\")|join('/')",
            "length",
        ]);
        let open = self.tag(&format!("apply {filter}"));
        out.extend_from_slice(open.as_bytes());
        let body = self.body(depth + 1);
        out.extend_from_slice(&body);
        let close = self.tag("endapply");
        out.extend_from_slice(close.as_bytes());
    }

    fn macro_def(&mut self, out: &mut Vec<u8>) {
        let name = self.fresh("m");
        let open = self.tag(&format!("macro {name}(a, b = ' ')"));
        out.extend_from_slice(open.as_bytes());
        if self.rng.chance(60) {
            self.text(out);
        }
        let print = self.print_of("a ~ b");
        out.extend_from_slice(print.as_bytes());
        if self.rng.chance(60) {
            self.text(out);
        }
        let close = self.tag("endmacro");
        out.extend_from_slice(close.as_bytes());
        self.text(out);
        let arg = self.rng.pick_str(&["' x '", "\"\\n\"", "s", "t"]);
        let call = format!("_self.{name}({arg})");
        let print = self.print_of(&call);
        out.extend_from_slice(print.as_bytes());
    }

    fn comment(&mut self, out: &mut Vec<u8>) {
        if self.rng.chance(25) {
            // special spellings
            let raw = self.rng.pick_str(&[
                "{##}",
                "{###}",
                "{####}",
                "{#-#}",
                "{#~#}",
                "{#--#}",
                "{#~~#}",
                "{# # #}",
                "{#}#}",
                "{#-}-#}",
                "{##-##}",
                "{##~##}",
                "{#\n#}",
                "{#\r\n#}",
                "{##\n}#}",
                "{#{{ n }}#}",
                "{#{% if %}#}",
                "{#-\n\n-#}",
            ]);
            out.extend_from_slice(&self.syntax.translate(raw.as_bytes()));
            return;
        }
        let l = self.ctl();
        let r = self.ctl();
        let [open, close] = self.syntax.comment;
        out.extend_from_slice(format!("{open}{}", self.syntax.control(l)).as_bytes());
        self.comment_body(out);
        out.extend_from_slice(format!("{}{close}", self.syntax.control(r)).as_bytes());
    }

    fn comment_body(&mut self, out: &mut Vec<u8>) {
        let n = self.rng.range(0, 4);
        for _ in 0..n {
            let atom: &[u8] = match self.rng.below(10) {
                0..=3 => self.rng.pick_bytes(SPACES),
                4..=6 => self.rng.pick_str(&["c", "note", "a b"]).as_bytes(),
                7 => self
                    .rng
                    .pick_str(&["{{ n }}", "{% if true %}", "{#", "#", "}", "{", "%}", "-"])
                    .as_bytes(),
                8 => self.rng.pick_bytes(ODD_SPACES),
                _ => b"\r",
            };
            let atom = self.syntax.translate(atom);
            out.extend_from_slice(&atom);
        }
    }

    fn doc_comment(&mut self, out: &mut Vec<u8>) {
        let l = self.ctl();
        let open = self.syntax.comment[0];
        out.extend_from_slice(format!("{open}#{}", self.syntax.control(l)).as_bytes());
        self.comment_body(out);
        let close = self
            .rng
            .pick_str(&["##}", "#}", "-##}", "~##}", "-#}", "~#}", " ##}", "###}"]);
        out.extend_from_slice(&self.syntax.translate(close.as_bytes()));
        // documentation attaches to the next tag or print
        if self.rng.chance(50) {
            let expr = self.expr();
            let print = self.print_of(&expr);
            out.extend_from_slice(print.as_bytes());
        }
    }

    fn verbatim(&mut self, out: &mut Vec<u8>) {
        let name = self
            .rng
            .weighted(&[(89, "verbatim"), (1, "raw"), (10, "verbatim ")]);
        let open = self.tag(name);
        out.extend_from_slice(open.as_bytes());
        let n = self.rng.range(0, 5);
        for _ in 0..n {
            match self.rng.below(10) {
                0..=3 => out.extend_from_slice(self.rng.pick_bytes(SPACES)),
                4..=6 => {
                    let piece = self.rng.pick_str(&[
                        "{{ n }}",
                        "{%- if x -%}",
                        "{# c #}",
                        "{{",
                        "%}",
                        "{% endverbatim x %}",
                        "{%endverbatim",
                        "{% verbatim %}",
                    ]);
                    out.extend_from_slice(&self.syntax.translate(piece.as_bytes()));
                }
                _ => self.text(out),
            }
        }
        let end = self
            .rng
            .weighted(&[(80, "endverbatim"), (17, "endverbatim "), (3, "endraw")]);
        let close = self.tag(end);
        out.extend_from_slice(close.as_bytes());
    }

    fn line(&mut self, out: &mut Vec<u8>) {
        let n = *self.rng.pick(&[1u32, 1, 2, 3, 5, 10, 42, 100, 99_999]);
        let l = self.ctl();
        let sl = self.rng.pick_str(&[" ", "", "\n", "  ", "\t"]);
        let mid = self.rng.pick_str(&[" ", "  ", "\n", "\t", "\r\n"]);
        let sr = self.rng.pick_str(&[" ", "", "\n", "  "]);
        let r = if self.rng.chance(3) { self.ctl() } else { "" };
        let [open, close] = self.syntax.block;
        let (l, r) = (self.syntax.control(l), self.syntax.control(r));
        out.extend_from_slice(format!("{open}{l}{sl}line{mid}{n}{sr}{r}{close}").as_bytes());
    }

    fn include(&mut self, out: &mut Vec<u8>) {
        self.uses_inc = true;
        let form = self.rng.below(6);
        match form {
            0 => {
                let tag = self.tag("include 'inc.twig'");
                out.extend_from_slice(tag.as_bytes());
            }
            1 => {
                let tag = self.tag("include 'inc.twig' with {n: ' n '}");
                out.extend_from_slice(tag.as_bytes());
            }
            2 => {
                let print = self.print_of("include('inc.twig')");
                out.extend_from_slice(print.as_bytes());
            }
            3 => {
                let print = self.print_of("source('inc.twig')");
                out.extend_from_slice(print.as_bytes());
            }
            4 => {
                let print = self.print_of("source('inc.twig')|json_encode");
                out.extend_from_slice(print.as_bytes());
            }
            _ => {
                let print = self.print_of("include('inc.twig')|json_encode");
                out.extend_from_slice(print.as_bytes());
            }
        }
    }

    fn inc(&mut self) -> Vec<u8> {
        let mut out = Vec::new();
        self.text(&mut out);
        let expr = self.rng.pick_str(&["n", "s", "t|trim", "'i'"]);
        let print = self.print_of(expr);
        out.extend_from_slice(print.as_bytes());
        if self.rng.chance(50) {
            self.comment(&mut out);
        }
        self.text(&mut out);
        out
    }

    // ---- scenarios ------------------------------------------------------------------------

    /// `child.twig` (extends `base.twig`) and `base.twig`.
    fn child_and_base(&mut self) -> (Vec<u8>, Vec<u8>) {
        let mut child = Vec::new();
        self.blank(&mut child);
        let extends = self.tag("extends 'base.twig'");
        child.extend_from_slice(extends.as_bytes());
        for name in ["a", "b"] {
            match self.rng.below(20) {
                0..=14 => self.blank(&mut child),
                15 | 16 => self.comment(&mut child),
                17 | 18 => {
                    let set = self.tag("set z = 1");
                    child.extend_from_slice(set.as_bytes());
                }
                _ => self.text(&mut child),
            }
            if self.rng.chance(85) {
                let open = self.tag(&format!("block {name}"));
                child.extend_from_slice(open.as_bytes());
                let body = self.body(1);
                child.extend_from_slice(&body);
                let close = self.tag("endblock");
                child.extend_from_slice(close.as_bytes());
            }
        }
        self.blank(&mut child);
        let mut base = Vec::new();
        self.text(&mut base);
        for name in ["a", "b"] {
            let open = self.tag(&format!("block {name}"));
            base.extend_from_slice(open.as_bytes());
            base.extend_from_slice(format!("<{name}>").as_bytes());
            let close = self.tag("endblock");
            base.extend_from_slice(close.as_bytes());
            self.text(&mut base);
        }
        let base = self.line_endings(base);
        (child, base)
    }

    /// A template with a huge run of text, a huge comment or huge verbatim content.
    fn huge(&mut self) -> Vec<u8> {
        let unit = self
            .rng
            .pick(&[
                &b" "[..],
                b"\n",
                b"\r\n",
                b" \t",
                b"ab \n",
                b"-",
                b"#",
                b"}",
                b"\x0b\x0c",
            ])
            .to_vec();
        let times = self.rng.range(2_000, 20_000);
        let run: Vec<u8> = unit.repeat(times);
        let mut out = Vec::new();
        match self.rng.below(5) {
            0 => {
                out.extend_from_slice(b"a");
                out.extend_from_slice(&run);
                let print = self.print_of("n");
                out.extend_from_slice(print.as_bytes());
                out.extend_from_slice(&run);
            }
            1 => {
                out.extend_from_slice(&self.syntax.translate(b"a{#"));
                out.extend_from_slice(&run);
                let close = self.rng.pick_str(&["#}", "-#}", "~#}"]);
                out.extend_from_slice(&self.syntax.translate(close.as_bytes()));
                out.extend_from_slice(b"b");
            }
            2 => {
                let open = self.tag("verbatim");
                out.extend_from_slice(open.as_bytes());
                out.extend_from_slice(&run);
                let close = self.tag("endverbatim");
                out.extend_from_slice(close.as_bytes());
                out.extend_from_slice(b"|");
            }
            3 => {
                // whitespace inside a tag
                let [open, close] = self.syntax.var;
                out.extend_from_slice(format!("x{open}").as_bytes());
                out.extend_from_slice(&b" \n".repeat(times));
                out.extend_from_slice(b"n");
                out.extend_from_slice(&b"\t ".repeat(times / 2));
                let ctl = self.ctl();
                out.extend_from_slice(self.syntax.control(ctl).as_bytes());
                out.extend_from_slice(format!("{close}y").as_bytes());
            }
            _ => {
                out.extend_from_slice(&run);
                let ctl = self.ctl();
                let ctl = self.syntax.control(ctl);
                let [open, close] = self.syntax.var;
                out.extend_from_slice(format!("{open}{ctl} s {ctl}{close}").as_bytes());
                out.extend_from_slice(&run);
                self.probe(&mut out);
            }
        }
        out
    }

    /// One construct in many spellings of its delimiters.
    fn spellings(&mut self) -> Vec<u8> {
        let mut out = Vec::new();
        let n = self.rng.range(4, 10);
        let kind = self.rng.below(4);
        for _ in 0..n {
            out.extend_from_slice(self.rng.pick_bytes(SPACES));
            out.extend_from_slice(b"|");
            out.extend_from_slice(self.rng.pick_bytes(SPACES));
            match kind {
                0 => {
                    let print = self.print_of("n");
                    out.extend_from_slice(print.as_bytes());
                }
                1 => {
                    let open = self.tag("if true");
                    let close = self.tag("endif");
                    out.extend_from_slice(open.as_bytes());
                    out.extend_from_slice(self.rng.pick_bytes(SPACES));
                    out.extend_from_slice(b"i");
                    out.extend_from_slice(self.rng.pick_bytes(SPACES));
                    out.extend_from_slice(close.as_bytes());
                }
                2 => self.comment(&mut out),
                _ => self.verbatim(&mut out),
            }
        }
        out.extend_from_slice(b"|");
        out
    }

    /// Something at the end that shows a line number.
    fn probe(&mut self, out: &mut Vec<u8>) {
        self.text(out);
        let probe = self.rng.pick_str(&[
            "{{ undefined_name }}",
            "{{ n.missing }}",
            "{{ 1 + }}",
            "{{ (1 }}",
            "{% if n %}",
            "{{ n|nope }}",
            "{% endif %}",
            "{{ 1 // 0 }}",
            "{{ [1][5] }}",
            "{{ n.x.y }}",
        ]);
        out.extend_from_slice(&self.syntax.translate(probe.as_bytes()));
        if self.rng.chance(40) {
            self.text(out);
        }
    }

    /// LF line endings as they are, or converted to CRLF, CR or a mix.
    fn line_endings(&mut self, source: Vec<u8>) -> Vec<u8> {
        let mode = self.rng.weighted(&[(70, 0u8), (13, 1), (10, 2), (7, 3)]);
        if mode == 0 {
            return source;
        }
        let mut out = Vec::with_capacity(source.len() + source.len() / 8);
        for &b in &source {
            if b == b'\n' {
                let style = if mode == 3 {
                    self.rng.range(0, 2)
                } else {
                    mode as usize
                };
                match style {
                    1 => out.extend_from_slice(b"\r\n"),
                    2 => out.push(b'\r'),
                    _ => out.push(b'\n'),
                }
            } else {
                out.push(b);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kit::case::Case;

    #[test]
    fn deterministic_and_parseable() {
        let a = generate(7, 300);
        assert_eq!(a, generate(7, 300));
        assert_ne!(generate(8, 5), generate(7, 5));
        for json in &a {
            let case = Case::from_json(json).expect("a twig case");
            assert!(case.unsupported_reason().is_none(), "{json:?}");
            assert!(case.template(&case.main).is_some());
        }
        assert_eq!(case(7, 11), a[11]);
        let batch = batch(7, 3);
        assert_eq!(
            batch.get("isolation").and_then(Json::as_str),
            Some("process")
        );
    }

    #[test]
    fn fragments_in_another_syntax() {
        let custom = Syntax {
            block: ["<%", "%>"],
            var: ["${", "}"],
            comment: ["<#", "#>"],
            trim: "=",
            line_trim: "!",
            line_chars: Some(" "),
            interpolation: "%{",
        };
        let t = |s: &str| String::from_utf8(custom.translate(s.as_bytes())).unwrap();
        assert_eq!(t("{{ n }}"), "${ n }");
        assert_eq!(t("{%- if x -%}"), "<%= if x =%>");
        assert_eq!(t("{#~~#}"), "<#!!#>");
        assert_eq!(t("{##-##}"), "<##=##>");
        assert_eq!(t("-##}"), "=##>");
        assert_eq!(t("{#}#}"), "<#}#>");
        assert_eq!(t("a - b } { {x {-y"), "a - b } { {x {-y");
        assert_eq!(t("-}}"), "=}");
        assert_eq!(Syntax::TWIG.translate(b"{%- x -%}"), b"{%- x -%}");
        assert_eq!(
            custom.interpolate(r##""#{ n }|\#{n}|#{ '#{' }""##),
            r##""%{ n }|\#{n}|%{ '%{' }""##
        );
        assert_eq!(Syntax::TWIG.options(), None);
        let options = custom.options().unwrap();
        assert_eq!(
            options
                .get("interpolation")
                .map(Json::to_compact)
                .as_deref(),
            Some(r#"["%{","}"]"#)
        );
    }

    /// One case in seven gets lexer options, none of the values `reference/deviations.md` lists
    /// for Twig's lexer.
    #[test]
    fn lexer_options_avoid_the_deviations() {
        let cases = generate(9001, 2000);
        let custom: Vec<&Json> = cases.iter().filter(|c| c.get("lexer").is_some()).collect();
        assert!((200..400).contains(&custom.len()), "{}", custom.len());
        for case in custom {
            let lexer = case.get("lexer").unwrap();
            for key in ["whitespace_trim", "whitespace_line_trim"] {
                if let Some(trim) = lexer.get(key).and_then(Json::as_str) {
                    assert!(
                        !trim.contains(|c: char| "\\^$.[]|()?*+{}#".contains(c) || c.is_whitespace()),
                        "{trim:?}"
                    );
                }
            }
            if let Some(chars) = lexer.get("whitespace_line_chars").and_then(Json::as_str) {
                assert!(!chars.is_empty() && !chars.contains(['\\', ']', '[', '^', '-']));
            }
            if let Some(interpolation) = lexer.get("interpolation") {
                assert!(interpolation.items().iter().all(|d| d.as_str() != Some("")));
            }
        }
    }
}
