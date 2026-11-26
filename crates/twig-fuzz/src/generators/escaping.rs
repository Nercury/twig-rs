//! Generator of the escaping family: random but meaningful templates and data that exercise
//!
//! - the auto-escaping strategy of the environment (`html`, `js`, `css`, `url`, `html_attr`,
//!   `html_attr_relaxed`, `false`, `name` with template names of every shape, falsy and invalid
//!   strategy strings) and the `charset` option;
//! - `{% autoescape %}` nesting (strings, `false`, `true`, numbers, invalid strategies, non-constant
//!   arguments);
//! - the `raw`, `escape` and `e` filters with every strategy and charset, named arguments and the
//!   `autoescape` argument;
//! - `Twig\Markup` values in the data (charsets, declared safe strategies), Stringable and
//!   non-Stringable objects;
//! - safe filters (`is_safe`), `pre_escape` filters (`nl2br`, `spaceless`), filters that keep a
//!   `Markup` (`trim`, `default`, `first`/`last` of a list) and filter chains;
//! - the safety of `set` captures, `apply`, `include()`, `source()`, `block()`, `parent()` and macro
//!   calls, the strategy of blocks and macro bodies, `extends`, `embed` and `use` across templates;
//! - strings with invalid UTF-8 in the data, in string literals (`"\xff"`) and in raw template text.
//!
//! [`generate`] is deterministic: case `i` of a seed only depends on the seed and `i`. The cases use
//! the oracle's plain environment (format: the crate documentation), `data_json` when the data fits
//! JSON and `data_php` otherwise (Markup and other objects, invalid UTF-8).
//!
//! # Scenarios
//!
//! Each case picks a scenario that weights the statements and the data: prints, autoescape nesting,
//! captures (`set`/`apply`), macros (own and imported), blocks, inheritance (`extends`, `parent()`,
//! `block()`), names (the `name` strategy with includes, imports, traits and embeds across template
//! names), charsets, Markup data, invalid UTF-8, explicit strategies, micro (one value through
//! explicit strategies and charsets), sandbox (the escape filters the escaper inserts, checked by a
//! policy) and conversions (iconv notices turned into exceptions). [`charset_sweep`] adds strings of
//! random bytes through every strategy and charset.
//!
//! # What the cases avoid
//!
//! What cannot match by design, or what the twig-rs runner cannot rebuild: `{% autoescape null %}`
//! around a print (a listed deviation), callable `autoescape` options, Markup objects with a `null`
//! charset (twig-test-support cannot rebuild them), `dump()` of objects (handles), dates and
//! randomness. Every case has a filesystem cache of its own ([`super::with_case_cache`]): with
//! `cache => false` an error raised inside a macro imported from another template would get the
//! line Twig guesses from the frames of the wrong template (reference/deviations.md, "Line guessing
//! with `cache => false`").

use std::fmt::Write as _;

use crate::kit::json::{Json, base64_encode};

/// The family name (also the prefix of the case ids).
pub const FAMILY: &str = "escaping";

/// `count` cases generated from `seed`.
pub fn generate(seed: u64, count: usize) -> Vec<Json> {
    (0..count as u64).map(|index| case(seed, index)).collect()
}

/// Case `index` of the cases of `seed` (id `esc-<seed>-<index>`).
pub fn case(seed: u64, index: u64) -> Json {
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ index.wrapping_mul(0xD1B5_4A32_D192_ED03)
        ^ 0x00E5_CA9E);
    rng.next();
    super::with_case_cache(Gen::new(rng, format!("esc-{seed}-{index}")).build())
}

/// `count` cases of a charset sweep: every case prints three strings of random bytes (biased towards
/// the lead and trail bytes of the multi-byte charsets) through `escape` with random strategies and
/// charsets, under a random `charset` option (htmlspecialchars() charsets, iconv conversions,
/// `ENT_SUBSTITUTE`, invalid UTF-8).
pub fn charset_sweep(seed: u64, count: usize) -> Vec<Json> {
    (0..count as u64)
        .map(|index| super::with_case_cache(sweep_case(seed, index)))
        .collect()
}

/// Case `index` of the charset sweep of `seed` (id `esc-sweep-<seed>-<index>`).
pub fn sweep_case(seed: u64, index: u64) -> Json {
    let mut rng = Rng(seed.wrapping_mul(0xA24B_AED4_963E_E407)
        ^ index.wrapping_mul(0x9FB2_1C65_1E98_DF25)
        ^ 0x5EE9);
    rng.next();
    let mut data = Vec::new();
    for name in ["v", "w", "s"] {
        let len = rng.range(1, 12);
        let bytes: Vec<u8> = (0..len)
            .map(|_| match rng.below(10) {
                0..=2 => *rng.pick(b"<>&\"' a/\\\n\x00"),
                3..=4 => 0x81 + rng.below(0x7e) as u8,
                5..=6 => 0x40 + rng.below(0xbf) as u8,
                7 => *rng.pick(&[0x8e, 0x8f, 0x80, 0xa0, 0xff, 0xfe, 0x7f, 0x5c]),
                _ => 0xa1 + rng.below(0x5e) as u8,
            })
            .collect();
        let bytes = if rng.chance(20) {
            // valid UTF-8 now and then
            String::from_utf8_lossy(&bytes).into_owned().into_bytes()
        } else {
            bytes
        };
        data.push((name, bytes));
    }
    let mut php = String::from("return [");
    for (i, (name, bytes)) in data.iter().enumerate() {
        if i > 0 {
            php.push_str(", ");
        }
        let _ = write!(php, "'{name}' => ");
        php_string(bytes, &mut php);
    }
    php.push_str("];");
    let mut template = String::new();
    for i in 0..6 {
        if i > 0 {
            template.push('|');
        }
        let var = *rng.pick(&["v", "w", "s"]);
        let strategy = *rng.weighted(&[
            (40, "html"),
            (15, "js"),
            (15, "css"),
            (10, "html_attr"),
            (5, "html_attr_relaxed"),
            (5, "url"),
        ]);
        match rng.below(10) {
            0 => {
                let _ = write!(template, "{{{{ {var}|e('{strategy}') }}}}");
            }
            1 => {
                let _ = write!(template, "{{{{ {var} }}}}");
            }
            _ => {
                let charset = *rng.pick(CHARSETS);
                let _ = write!(template, "{{{{ {var}|e('{strategy}', '{charset}') }}}}");
            }
        }
    }
    let mut config = vec![(
        "autoescape".to_owned(),
        Json::str(*rng.pick(VALID_STRATEGIES)),
    )];
    if rng.chance(30) {
        config.push(("charset".to_owned(), Json::str(*rng.pick(ENV_CHARSETS))));
    }
    let mut fields = vec![
        (
            "id".to_owned(),
            Json::str(format!("esc-sweep-{seed}-{index}")),
        ),
        ("env".to_owned(), Json::str("plain")),
        (
            "templates".to_owned(),
            Json::Object(vec![("index.twig".to_owned(), Json::str(template))]),
        ),
        ("data_php".to_owned(), Json::str(php)),
        ("config".to_owned(), Json::Object(config)),
    ];
    if rng.chance(25) {
        fields.push(("errors".to_owned(), Json::str("throw")));
    }
    Json::Object(fields)
}

// ---- random numbers -------------------------------------------------------------------------------

/// SplitMix64.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// `0..n` (0 when `n` is 0).
    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }

    /// `lo..=hi`.
    fn range(&mut self, lo: usize, hi: usize) -> usize {
        lo + self.below(hi - lo + 1)
    }

    fn chance(&mut self, percent: u32) -> bool {
        self.next() % 100 < u64::from(percent)
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }

    fn weighted<'a, T>(&mut self, items: &'a [(u32, T)]) -> &'a T {
        let total: u32 = items.iter().map(|(w, _)| *w).sum();
        if total == 0 {
            return &items[0].1;
        }
        let mut x = (self.next() % u64::from(total)) as u32;
        for (w, item) in items {
            if x < *w {
                return item;
            }
            x -= w;
        }
        &items[items.len() - 1].1
    }
}

// ---- vocabulary -----------------------------------------------------------------------------------

const VALID_STRATEGIES: &[&str] = &["html", "js", "css", "url", "html_attr", "html_attr_relaxed"];

const INVALID_STRATEGIES: &[&str] = &[
    "HTML", "name", "bogus", "", "all", "html ", "1", "0", "Js", "htmlattr",
];

/// Charsets for the `escape` filter's second argument and the `charset` option.
const CHARSETS: &[&str] = &[
    "UTF-8",
    "utf-8",
    "ISO-8859-1",
    "iso-8859-1",
    "ISO8859-15",
    "Windows-1252",
    "cp1252",
    "CP1251",
    "KOI8-R",
    "ASCII",
    "BIG5",
    "SJIS",
    "EUC-JP",
    "GB2312",
    "UTF-16",
    "UTF-16LE",
    "MacRoman",
    "latin1",
    "utf8",
    "CP866",
    "ISO-8859-5",
    "BIG5-HKSCS",
    "big5-hkscs",
    "936",
    "1252",
    "UCS-2",
    "bogus-charset",
    "",
    // the rest of EscaperRuntime's htmlspecialchars() table, exact and upper-cased
    "ISO8859-1",
    "iso8859-15",
    "ISO-8859-15",
    "IBM866",
    "866",
    "WINDOWS-1251",
    "WIN-1251",
    "win-1251",
    "1251",
    "KOI8-RU",
    "koi8r",
    "950",
    "SHIFT_JIS",
    "shift_jis",
    "932",
    "EUCJP",
    "MACROMAN",
    "iso8859-5",
    "cp1251",
    "Big5",
];

/// The `charset` option of the environment.
const ENV_CHARSETS: &[&str] = &[
    "utf-8",
    "ISO-8859-1",
    "iso-8859-1",
    "Windows-1252",
    "CP1251",
    "KOI8-R",
    "ASCII",
    "ISO-8859-15",
    "latin1",
    "utf8",
    "BIG5",
    "SJIS",
    "EUC-JP",
    "UTF-16",
];

/// Template names for the `name` strategy (`FileExtensionEscapingStrategy::guess()`).
const TEMPLATE_NAMES: &[&str] = &[
    "page.html",
    "page.html.twig",
    "app.js",
    "app.js.twig",
    "data.json",
    "data.json.twig",
    "style.css",
    "style.css.twig",
    "notes.txt",
    "notes.txt.twig",
    "dir/",
    "dir\\",
    "x.JS",
    "x.Js.twig",
    "a.js.twig.twig",
    "a.twig",
    ".js",
    ".css.twig",
    "js",
    "a.b/c.js",
    "a.js/b",
    "a.",
    "a.txt.",
    "a\\b.css",
    "x.css\\y",
    "noext",
    "a.tar.gz",
    "x.json.txt.twig",
    "x.txt.js",
    "x.TWIG",
    "x.js.TWIG",
    "sub/dir/y.css.twig",
    ".twig",
    "twig",
    "x.html.js",
    "x.js.html",
    "x..js",
    "x.js.",
    "y.json/",
    "z.txt\\",
    "0.js",
    "q.css.twig/",
    "a.js\\",
    "foo.jsx",
    "a.json5",
    "a.cssx",
    "a.html.txt",
    ".txt",
    "txt",
    "a.Txt",
    "a.txt.twig.twig",
    "x.css.txt",
];

/// Plain names of helper templates.
const PLAIN_NAMES: &[&str] = &[
    "part.twig",
    "layout.twig",
    "macros.twig",
    "card.twig",
    "traits.twig",
    "inc.html.twig",
    "other.twig",
];

/// Pieces of data strings and literals (valid UTF-8).
const PIECES: &[&str] = &[
    "<",
    ">",
    "&",
    "\"",
    "'",
    "/",
    "\\",
    "`",
    "=",
    " ",
    "\t",
    "\n",
    "\r",
    "\u{0}",
    "\u{8}",
    "\u{c}",
    "\u{b}",
    "\u{1b}",
    "\u{1f}",
    "\u{7f}",
    ":",
    "@",
    "[",
    "]",
    ",",
    ".",
    "-",
    "_",
    "%",
    "+",
    "#",
    ";",
    "(",
    ")",
    "{",
    "}",
    "*",
    "~",
    "!",
    "?",
    "^",
    "|",
    "$",
    "a",
    "Z",
    "0",
    "9",
    "abc",
    "Hello",
    "x y",
    "é",
    "ü",
    "ß",
    "\u{a0}",
    "\u{80}",
    "\u{85}",
    "\u{9f}",
    "\u{ad}",
    "\u{100}",
    "\u{7ff}",
    "\u{800}",
    "\u{200b}",
    "\u{2028}",
    "\u{2029}",
    "\u{d7ff}",
    "\u{e000}",
    "\u{feff}",
    "\u{fffd}",
    "\u{fffe}",
    "\u{ffff}",
    "€",
    "日本",
    "😀",
    "\u{10ffff}",
    "e\u{301}",
    "İ",
    "ﬁ",
    "Ω",
    "ǅ",
    "&amp;",
    "&#39;",
    "&lt;",
    "&quot;",
    "<br />",
    "<b>",
    "</b>",
    "<script>alert('x')</script>",
    "]]>",
    "<!--",
    "-->",
    "{{",
    "}}",
    "{%",
    "%}",
    "javascript:",
    "x=y&z=1",
    "100%",
    "C:\\path",
    "line1\nline2",
    "  padded  ",
    "\r\n",
    "a href='x'",
    "\"quoted\"",
    "1.5",
    "-0",
    "0x1F",
    "%3C",
    "\\u003C",
    "</script><script>",
];

/// Byte sequences that are not valid UTF-8.
const INVALID_PIECES: &[&[u8]] = &[
    b"\xff",
    b"\xfe",
    b"\xc3",
    b"\x80",
    b"\xc0\xaf",
    b"\xed\xa0\x80",
    b"\xf4\x90\x80\x80",
    b"\xe2\x82",
    b"\xf8\x88\x80\x80\x80",
    b"\xc3\x28",
    b"\xe9",
    b"\xc2",
    b"\xef\xbf",
    b"\xf0\x9f\x98",
    b"\xa0",
    b"\xc1\xbf",
    b"\xe0\x80\xaf",
    b"<\xff>",
];

/// Pieces of raw template text (no `{`, so that no tag starts by accident).
const TEXT_PIECES: &[&str] = &[
    "<p>",
    "</p>",
    "<b>",
    "&amp;",
    "&",
    "<",
    ">",
    "\"",
    "'",
    " ",
    "\n",
    "x",
    "é",
    "日",
    "<a href=\"#\">",
    "</a>",
    "\t",
    "%",
    "}",
    "#",
    "\\",
    "😀",
    "&nbsp;",
    "<br>",
    "|",
];

/// Tags, filters and functions a sandbox policy may allow.
const SANDBOX_TAGS: &[&str] = &[
    "autoescape",
    "set",
    "apply",
    "if",
    "for",
    "with",
    "block",
    "include",
    "macro",
    "import",
    "from",
    "extends",
    "embed",
    "use",
    "verbatim",
    "do",
    "sandbox",
];
const SANDBOX_FILTERS: &[&str] = &[
    "escape",
    "e",
    "raw",
    "nl2br",
    "spaceless",
    "upper",
    "lower",
    "title",
    "capitalize",
    "trim",
    "striptags",
    "replace",
    "format",
    "default",
    "first",
    "last",
    "join",
    "slice",
    "reverse",
    "url_encode",
    "json_encode",
    "convert_encoding",
    "length",
    "abs",
    "round",
    "number_format",
    "map",
    "filter",
    "reduce",
    "find",
    "split",
    "keys",
    "merge",
];
const SANDBOX_FUNCTIONS: &[&str] = &[
    "include",
    "source",
    "block",
    "parent",
    "dump",
    "attribute",
    "cycle",
    "max",
    "constant",
    "template_from_string",
];

/// Floats: PHP literal, JSON text.
const FLOATS: &[(&str, &str)] = &[
    ("1.5", "1.5"),
    ("-0.0", "-0.0"),
    ("1.0E+25", "1.0e25"),
    ("0.1", "0.1"),
    ("100.0", "100.0"),
    ("-1.25", "-1.25"),
    ("1.0E-10", "1.0e-10"),
];

// ---- values ---------------------------------------------------------------------------------------

/// A data value.
#[derive(Clone, Debug)]
enum Val {
    Null,
    Bool(bool),
    Int(i64),
    /// PHP literal, JSON text.
    Float(&'static str, &'static str),
    Str(Vec<u8>),
    List(Vec<Val>),
    Map(Vec<(String, Val)>),
    /// `new \Twig\Markup($content, $charset[, $safe])`.
    Markup {
        content: Vec<u8>,
        charset: Option<&'static str>,
        safe: Option<Vec<&'static str>>,
    },
    /// `new \Twig\Tests\ToStringStub($s)` (needs `test_classes`).
    Stringable(Vec<u8>),
    /// `new \stdClass()`.
    Object,
}

/// What a variable holds (steers the expressions that use it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Str,
    Markup,
    Scalar,
    List,
    Map,
    Object,
    Any,
}

impl Val {
    fn kind(&self) -> Kind {
        match self {
            Val::Null | Val::Bool(_) | Val::Int(_) | Val::Float(..) => Kind::Scalar,
            Val::Str(_) => Kind::Str,
            Val::List(_) => Kind::List,
            Val::Map(_) => Kind::Map,
            Val::Markup { .. } => Kind::Markup,
            Val::Stringable(_) | Val::Object => Kind::Object,
        }
    }

    /// Whether the value needs `data_php` (JSON cannot carry it).
    fn needs_php(&self) -> bool {
        match self {
            Val::Null | Val::Bool(_) | Val::Int(_) | Val::Float(..) => false,
            Val::Str(s) => std::str::from_utf8(s).is_err(),
            Val::List(items) => items.iter().any(Val::needs_php),
            Val::Map(entries) => entries.iter().any(|(_, v)| v.needs_php()),
            Val::Markup { .. } | Val::Stringable(_) | Val::Object => true,
        }
    }

    fn uses_test_classes(&self) -> bool {
        match self {
            Val::Stringable(_) => true,
            Val::List(items) => items.iter().any(Val::uses_test_classes),
            Val::Map(entries) => entries.iter().any(|(_, v)| v.uses_test_classes()),
            _ => false,
        }
    }

    /// The JSON form (only for values that do not [need PHP](Val::needs_php)).
    fn json(&self) -> Json {
        match self {
            Val::Null | Val::Object => Json::Null,
            Val::Bool(b) => Json::Bool(*b),
            Val::Int(i) => Json::int(*i),
            Val::Float(_, json) => Json::Number((*json).to_owned()),
            Val::Str(s) | Val::Stringable(s) => Json::str(String::from_utf8_lossy(s)),
            Val::List(items) => Json::Array(items.iter().map(Val::json).collect()),
            Val::Map(entries) => {
                Json::Object(entries.iter().map(|(k, v)| (k.clone(), v.json())).collect())
            }
            Val::Markup { content, .. } => Json::str(String::from_utf8_lossy(content)),
        }
    }

    /// The PHP expression.
    fn php(&self, out: &mut String) {
        match self {
            Val::Null => out.push_str("null"),
            Val::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Val::Int(i) if *i == i64::MIN => out.push_str("PHP_INT_MIN"),
            Val::Int(i) => {
                let _ = write!(out, "{i}");
            }
            Val::Float(php, _) => out.push_str(php),
            Val::Str(s) => php_string(s, out),
            Val::List(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    item.php(out);
                }
                out.push(']');
            }
            Val::Map(entries) => {
                out.push('[');
                for (i, (key, value)) in entries.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    php_string(key.as_bytes(), out);
                    out.push_str(" => ");
                    value.php(out);
                }
                out.push(']');
            }
            Val::Markup {
                content,
                charset,
                safe,
            } => {
                out.push_str("new \\Twig\\Markup(");
                php_string(content, out);
                out.push_str(", ");
                match charset {
                    Some(c) => php_string(c.as_bytes(), out),
                    None => out.push_str("null"),
                }
                if let Some(safe) = safe {
                    out.push_str(", [");
                    for (i, s) in safe.iter().enumerate() {
                        if i > 0 {
                            out.push_str(", ");
                        }
                        php_string(s.as_bytes(), out);
                    }
                    out.push(']');
                }
                out.push(')');
            }
            Val::Stringable(s) => {
                out.push_str("new \\Twig\\Tests\\ToStringStub(");
                php_string(s, out);
                out.push(')');
            }
            Val::Object => out.push_str("new \\stdClass()"),
        }
    }
}

/// A PHP double-quoted string literal of arbitrary bytes.
fn php_string(bytes: &[u8], out: &mut String) {
    out.push('"');
    for &b in bytes {
        match b {
            b'"' => out.push_str("\\\""),
            b'\\' => out.push_str("\\\\"),
            b'$' => out.push_str("\\$"),
            0x20..=0x7e => out.push(b as char),
            _ => {
                let _ = write!(out, "\\x{b:02x}");
            }
        }
    }
    out.push('"');
}

// ---- generator state -------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scenario {
    Prints,
    Nesting,
    Capture,
    Macros,
    Blocks,
    Inheritance,
    Names,
    Charsets,
    Markup,
    Invalid,
    Strategies,
    /// One to four prints of one value with explicit strategies and charsets.
    Micro,
    /// The escaping filters the escaper inserts, seen by a sandbox policy.
    Sandbox,
    /// Charset conversions (iconv notices) with warnings turned into exceptions.
    Conversions,
}

/// Statement weights and data biases of a scenario.
#[derive(Clone, Copy, Debug)]
struct Profile {
    text: u32,
    print: u32,
    autoescape: u32,
    set: u32,
    capture: u32,
    apply: u32,
    cond: u32,
    loops: u32,
    with: u32,
    block: u32,
    include: u32,
    macro_call: u32,
    verbatim: u32,
    misc: u32,
    /// Percent of string pieces that are not valid UTF-8.
    invalid: u32,
    /// Percent chance of Markup variables.
    markup: u32,
    /// Percent chance of a `charset` option other than UTF-8.
    charset: u32,
    /// Percent chance that an `escape` filter gets a charset argument.
    escape_charset: u32,
    /// Percent chance that a string is made of random bytes.
    random_bytes: u32,
}

const BASE: Profile = Profile {
    text: 10,
    print: 45,
    autoescape: 10,
    set: 5,
    capture: 5,
    apply: 4,
    cond: 3,
    loops: 3,
    with: 2,
    block: 3,
    include: 4,
    macro_call: 4,
    verbatim: 1,
    misc: 4,
    invalid: 2,
    markup: 35,
    charset: 4,
    escape_charset: 12,
    random_bytes: 3,
};

impl Scenario {
    fn profile(self) -> Profile {
        match self {
            Scenario::Prints => BASE,
            Scenario::Nesting => Profile {
                autoescape: 35,
                print: 40,
                ..BASE
            },
            Scenario::Capture => Profile {
                capture: 25,
                apply: 18,
                set: 12,
                ..BASE
            },
            Scenario::Macros => Profile {
                macro_call: 30,
                ..BASE
            },
            Scenario::Blocks => Profile {
                block: 25,
                include: 10,
                ..BASE
            },
            Scenario::Inheritance => Profile { block: 10, ..BASE },
            Scenario::Names => Profile {
                include: 25,
                macro_call: 12,
                block: 8,
                ..BASE
            },
            Scenario::Charsets => Profile {
                charset: 60,
                escape_charset: 45,
                random_bytes: 25,
                ..BASE
            },
            Scenario::Markup => Profile {
                markup: 100,
                ..BASE
            },
            Scenario::Invalid => Profile {
                invalid: 30,
                text: 18,
                random_bytes: 15,
                ..BASE
            },
            Scenario::Strategies => Profile {
                print: 70,
                autoescape: 15,
                ..BASE
            },
            Scenario::Conversions => Profile {
                charset: 50,
                escape_charset: 70,
                random_bytes: 40,
                invalid: 20,
                ..BASE
            },
            Scenario::Sandbox => Profile {
                include: 8,
                apply: 6,
                ..BASE
            },
            Scenario::Micro => Profile {
                markup: 30,
                charset: 45,
                escape_charset: 60,
                random_bytes: 45,
                invalid: 8,
                ..BASE
            },
        }
    }
}

/// A variable in scope.
#[derive(Clone, Debug)]
struct Local {
    name: String,
    kind: Kind,
    /// The keys of a mapping.
    keys: Vec<String>,
}

/// A macro of the main template (`_self`).
#[derive(Clone, Debug)]
struct MacroDef {
    name: String,
    /// Parameter names with an optional default (Twig source).
    params: Vec<(String, Option<String>)>,
    body: Vec<u8>,
    /// `{% autoescape <arg> %}` around the definition.
    wrap: Option<String>,
}

/// A template of macros imported by the main template.
#[derive(Clone, Debug)]
struct Lib {
    /// `{% import ... as <alias> %}`.
    alias: String,
    /// Macro names and parameter counts.
    macros: Vec<(String, usize)>,
    /// Local names of macros imported with `{% from ... import m as f %}`.
    from: Vec<(String, usize)>,
}

/// The `SandboxExtension` of a case: global flag, allowed tags, filters and functions.
#[derive(Clone, Debug)]
struct Sandbox {
    global: bool,
    tags: Vec<&'static str>,
    filters: Vec<&'static str>,
    functions: Vec<&'static str>,
}

/// An expression and whether a filter, an attribute or an operator can follow it without parentheses.
struct Expr {
    src: String,
    atomic: bool,
}

impl Expr {
    fn atom(src: impl Into<String>) -> Expr {
        Expr {
            src: src.into(),
            atomic: true,
        }
    }

    fn compound(src: impl Into<String>) -> Expr {
        Expr {
            src: src.into(),
            atomic: false,
        }
    }

    /// The source, parenthesized unless atomic.
    fn wrapped(&self) -> String {
        if self.atomic {
            self.src.clone()
        } else {
            format!("({})", self.src)
        }
    }
}

struct Gen {
    rng: Rng,
    id: String,
    scenario: Scenario,
    p: Profile,
    // settings
    autoescape: Option<Json>,
    charset: Option<&'static str>,
    strict: bool,
    debug: bool,
    errors_throw: bool,
    uses_dump: bool,
    uses_string_loader: bool,
    /// `use_yield` and `optimizations` options.
    use_yield: Option<bool>,
    optimizations: Option<i64>,
    /// Globals of the environment (JSON strings).
    globals: Vec<(String, String)>,
    /// The sandbox.
    sandbox: Option<Sandbox>,
    // data
    data: Vec<(String, Val)>,
    scope: Vec<Local>,
    // templates
    main: String,
    names_used: Vec<String>,
    extra: Vec<(String, Vec<u8>)>,
    partials: Vec<String>,
    macros: Vec<MacroDef>,
    self_alias: Option<String>,
    libs: Vec<Lib>,
    header: Vec<u8>,
    fresh: usize,
    // where the statements being generated go
    in_macro: bool,
    in_aux: bool,
    block_ok: bool,
}

impl Gen {
    fn new(mut rng: Rng, id: String) -> Gen {
        let scenario = *rng.weighted(&[
            (16, Scenario::Prints),
            (12, Scenario::Nesting),
            (10, Scenario::Capture),
            (8, Scenario::Macros),
            (8, Scenario::Blocks),
            (6, Scenario::Inheritance),
            (8, Scenario::Names),
            (7, Scenario::Charsets),
            (10, Scenario::Markup),
            (7, Scenario::Invalid),
            (8, Scenario::Strategies),
            (20, Scenario::Micro),
            (5, Scenario::Sandbox),
            (6, Scenario::Conversions),
        ]);
        Gen {
            rng,
            id,
            scenario,
            p: scenario.profile(),
            autoescape: None,
            charset: None,
            strict: false,
            debug: false,
            errors_throw: false,
            uses_dump: false,
            uses_string_loader: false,
            use_yield: None,
            optimizations: None,
            sandbox: None,
            globals: Vec::new(),
            data: Vec::new(),
            scope: Vec::new(),
            main: "index.twig".to_owned(),
            names_used: Vec::new(),
            extra: Vec::new(),
            partials: Vec::new(),
            macros: Vec::new(),
            self_alias: None,
            libs: Vec::new(),
            header: Vec::new(),
            fresh: 0,
            in_macro: false,
            in_aux: false,
            block_ok: true,
        }
    }

    fn build(mut self) -> Json {
        self.settings();
        self.data();
        self.names_used.push(self.main.clone());
        let body = if self.scenario == Scenario::Inheritance {
            self.child_template()
        } else if matches!(self.scenario, Scenario::Micro | Scenario::Conversions) {
            self.micro_template()
        } else {
            let mut out = Vec::new();
            self.block_ok = true;
            let n = *self
                .rng
                .weighted(&[(30, 1usize), (35, 2), (20, 3), (15, 4)]);
            for _ in 0..n {
                self.stmt(0, &mut out);
            }
            out
        };
        let mut source = std::mem::take(&mut self.header);
        source.extend(body);
        for def in std::mem::take(&mut self.macros) {
            render_macro(&def, &mut source);
        }
        self.finish(source)
    }

    // ---- settings and data ------------------------------------------------------------------

    fn settings(&mut self) {
        let autoescape: Option<Json> = if self.scenario == Scenario::Names {
            Some(Json::str("name"))
        } else {
            match *self
                .rng
                .weighted(&[(34, 0u8), (33, 1), (8, 2), (14, 3), (8, 4), (3, 5)])
            {
                0 => None,
                1 => Some(Json::str(*self.rng.pick(VALID_STRATEGIES))),
                2 => Some(Json::Bool(false)),
                3 => Some(Json::str("name")),
                4 => Some(Json::str(*self.rng.pick(&["", "0", "html", "js"]))),
                _ => Some(Json::str(
                    *self.rng.pick(&["bogus", "HTML", "all", "name "]),
                )),
            }
        };
        self.autoescape = autoescape;
        if self.rng.chance(self.p.charset) {
            self.charset = Some(*self.rng.pick(ENV_CHARSETS));
        }
        self.strict = self.rng.chance(15);
        self.errors_throw = self.rng.chance(if self.scenario == Scenario::Conversions {
            60
        } else {
            8
        });
        if self.rng.chance(10) {
            self.use_yield = Some(self.rng.chance(50));
        }
        if self.rng.chance(8) {
            self.optimizations = Some(*self.rng.pick(&[0, 2, -1]));
        }
        if self.scenario == Scenario::Sandbox {
            let global = self.rng.chance(70);
            let mut tags = Vec::new();
            for tag in SANDBOX_TAGS {
                if self.rng.chance(92) {
                    tags.push(*tag);
                }
            }
            let mut filters = Vec::new();
            for filter in SANDBOX_FILTERS {
                let percent = if matches!(*filter, "escape" | "raw") {
                    60
                } else {
                    88
                };
                if self.rng.chance(percent) {
                    filters.push(*filter);
                }
            }
            let mut functions = Vec::new();
            for function in SANDBOX_FUNCTIONS {
                if self.rng.chance(90) {
                    functions.push(*function);
                }
            }
            self.sandbox = Some(Sandbox {
                global,
                tags,
                filters,
                functions,
            });
        }
        let names = self.scenario == Scenario::Names
            || self.autoescape.as_ref().and_then(Json::as_str) == Some("name")
            || self.rng.chance(15);
        if names && self.rng.chance(85) {
            self.main = (*self.rng.pick(TEMPLATE_NAMES)).to_owned();
        }
    }

    fn data(&mut self) {
        if self.rng.chance(12) {
            let value = String::from_utf8_lossy(&self.string()).into_owned();
            self.globals.push(("g".to_owned(), value));
            self.add_local("g".to_owned(), Kind::Str);
        }
        let strings = self.rng.range(1, 3);
        for name in &["v", "w", "s"][..strings] {
            let value = Val::Str(self.string());
            self.add_data(name, value);
        }
        if self.rng.chance(self.p.markup) {
            let count = self.rng.range(1, 3);
            for name in &["m", "mk", "mj"][..count] {
                let value = self.markup();
                self.add_data(name, value);
            }
        }
        if self.rng.chance(30) {
            let value = Val::Int(*self.rng.pick(&[0, 1, -1, 42, 255, i64::MAX, -7]));
            self.add_data("n", value);
        }
        if self.rng.chance(15) {
            let (php, json) = *self.rng.pick(FLOATS);
            self.add_data("f", Val::Float(php, json));
        }
        if self.rng.chance(15) {
            let value = Val::Bool(self.rng.chance(50));
            self.add_data("b", value);
        }
        if self.rng.chance(12) {
            self.add_data("z", Val::Null);
        }
        if self.rng.chance(35) {
            let count = self.rng.range(1, 4);
            let items = (0..count).map(|_| self.element()).collect();
            self.add_data("arr", Val::List(items));
        }
        if self.rng.chance(30) {
            let count = self.rng.range(1, 3);
            let keys = ["k", "h", "lt", "amp"];
            let entries = (0..count)
                .map(|i| (keys[i].to_owned(), self.element()))
                .collect();
            self.add_data("map", Val::Map(entries));
        }
        if self.rng.chance(8) {
            let value = Val::Stringable(self.string());
            self.add_data("o", value);
        }
        if self.rng.chance(4) {
            self.add_data("so", Val::Object);
        }
        if self.rng.chance(18) {
            let strategy = self.strategy();
            self.add_data("strat", Val::Str(strategy.into_bytes()));
        }
        if self.rng.chance(8) {
            let charset = *self.rng.pick(CHARSETS);
            self.add_data("cs", Val::Str(charset.as_bytes().to_vec()));
        }
    }

    fn add_data(&mut self, name: &str, value: Val) {
        let keys = match &value {
            Val::Map(entries) => entries.iter().map(|(k, _)| k.clone()).collect(),
            _ => Vec::new(),
        };
        self.scope.push(Local {
            name: name.to_owned(),
            kind: value.kind(),
            keys,
        });
        self.data.push((name.to_owned(), value));
    }

    /// An element of a list or mapping.
    fn element(&mut self) -> Val {
        match self.rng.below(10) {
            0..=4 => Val::Str(self.string()),
            5..=6 => self.markup(),
            7 => Val::Int(*self.rng.pick(&[0, 3, -1])),
            8 => Val::Null,
            _ => Val::List(vec![Val::Str(self.string())]),
        }
    }

    fn markup(&mut self) -> Val {
        let content = self.string();
        // (a `null` charset is valid PHP, but twig-test-support cannot rebuild such a Markup)
        let charset = *self.rng.weighted(&[
            (80, Some("UTF-8")),
            (8, Some("ISO-8859-1")),
            (5, Some("utf-8")),
            (4, Some("Windows-1252")),
            (3, Some("KOI8-R")),
        ]);
        let safe = match self.rng.below(17) {
            0..=5 => None,
            6 => Some(vec!["html"]),
            7 => Some(vec!["js"]),
            8 => Some(vec!["css"]),
            9 => Some(vec!["url"]),
            10 => Some(vec!["html_attr"]),
            11 => Some(vec!["html_attr_relaxed"]),
            12 => Some(vec![]),
            13 => Some(vec!["all"]),
            14 => Some(vec!["HTML", "Js"]),
            15 => Some(vec!["js", "css"]),
            _ => Some(vec!["html_attr", "url"]),
        };
        Val::Markup {
            content,
            charset,
            safe,
        }
    }

    /// A string made of interesting pieces (or of random bytes).
    fn string(&mut self) -> Vec<u8> {
        if self.rng.chance(self.p.random_bytes) {
            let len = self.rng.range(1, 8);
            let high = self.rng.chance(50);
            return (0..len)
                .map(|_| {
                    let b = self.rng.below(if high { 128 } else { 256 }) as u8;
                    if high { b | 0x80 } else { b }
                })
                .collect();
        }
        let count = *self
            .rng
            .weighted(&[(5, 0usize), (30, 1), (30, 2), (20, 3), (10, 4), (5, 6)]);
        let mut out = Vec::new();
        for _ in 0..count {
            if self.p.invalid > 0 && self.rng.chance(self.p.invalid) {
                let piece = INVALID_PIECES[self.rng.below(INVALID_PIECES.len())];
                out.extend_from_slice(piece);
            } else {
                out.extend_from_slice(self.rng.pick(PIECES).as_bytes());
            }
        }
        out
    }

    fn strategy(&mut self) -> String {
        if self.rng.chance(95) {
            (*self.rng.pick(VALID_STRATEGIES)).to_owned()
        } else {
            (*self.rng.pick(INVALID_STRATEGIES)).to_owned()
        }
    }

    // ---- names ------------------------------------------------------------------------------

    /// A template name not used yet: one of [`TEMPLATE_NAMES`] when names matter, else a plain one.
    fn fresh_name(&mut self) -> String {
        let names_matter = self.scenario == Scenario::Names
            || self.autoescape.as_ref().and_then(Json::as_str) == Some("name");
        for _ in 0..8 {
            let candidate = if names_matter || self.rng.chance(25) {
                *self.rng.pick(TEMPLATE_NAMES)
            } else {
                *self.rng.pick(PLAIN_NAMES)
            };
            if !self.names_used.iter().any(|n| n == candidate) {
                self.names_used.push(candidate.to_owned());
                return candidate.to_owned();
            }
        }
        self.fresh += 1;
        let name = format!("t{}.twig", self.fresh);
        self.names_used.push(name.clone());
        name
    }

    fn fresh_var(&mut self, prefix: &str) -> String {
        self.fresh += 1;
        format!("{prefix}{}", self.fresh)
    }

    // ---- statements ---------------------------------------------------------------------------

    fn stmt(&mut self, depth: u32, out: &mut Vec<u8>) {
        if self.rng.chance(20) {
            push(out, "\n");
        }
        #[derive(Clone, Copy)]
        enum St {
            Text,
            Print,
            Autoescape,
            Set,
            Capture,
            Apply,
            If,
            For,
            With,
            Block,
            Include,
            MacroCall,
            Verbatim,
            Misc,
        }
        let p = self.p;
        let nested = depth < 3;
        let mut choices: Vec<(u32, St)> = vec![
            (p.text, St::Text),
            (p.print, St::Print),
            (p.set, St::Set),
            (p.verbatim, St::Verbatim),
            (p.misc, St::Misc),
        ];
        if nested {
            choices.extend([
                (p.autoescape, St::Autoescape),
                (p.capture, St::Capture),
                (p.apply, St::Apply),
                (p.cond, St::If),
                (p.loops, St::For),
                (p.with, St::With),
            ]);
        }
        if nested && self.block_ok && !self.in_macro && !self.in_aux {
            choices.push((p.block, St::Block));
        }
        if !self.in_macro && !self.in_aux {
            choices.push((p.include, St::Include));
            choices.push((p.macro_call, St::MacroCall));
        }
        match *self.rng.weighted(&choices) {
            St::Text => {
                let text = self.text();
                out.extend(text);
            }
            St::Print => {
                let expr = self.expr(0);
                self.print(&expr.src, out);
            }
            St::Autoescape => {
                let arg = self.autoescape_arg();
                push(out, &format!("{{% autoescape{arg} %}}"));
                self.body(depth + 1, true, out);
                push(out, "{% endautoescape %}");
            }
            St::Set => self.set_stmt(out),
            St::Capture => self.capture_stmt(depth, out),
            St::Apply => {
                let chain = self.apply_chain();
                push(out, &format!("{{% apply {chain} %}}"));
                self.body(depth + 1, false, out);
                push(out, "{% endapply %}");
            }
            St::If => {
                let cond = self.condition();
                push(out, &format!("{{% if {cond} %}}"));
                self.body(depth + 1, true, out);
                if self.rng.chance(40) {
                    push(out, "{% else %}");
                    self.body(depth + 1, true, out);
                }
                push(out, "{% endif %}");
            }
            St::For => self.for_stmt(depth, out),
            St::With => {
                let name = self.fresh_var("w");
                let value = self.expr(1);
                let only = if self.rng.chance(20) { " only" } else { "" };
                push(
                    out,
                    &format!("{{% with {{{name}: {}}}{only} %}}", value.src),
                );
                let saved = if only.is_empty() {
                    self.scope.clone()
                } else {
                    std::mem::take(&mut self.scope)
                };
                self.scope.push(Local {
                    name,
                    kind: Kind::Any,
                    keys: Vec::new(),
                });
                self.body(depth + 1, false, out);
                self.scope = saved;
                push(out, "{% endwith %}");
            }
            St::Block => {
                let name = self.fresh_var("b");
                push(out, &format!("{{% block {name} %}}"));
                self.body(depth + 1, true, out);
                push(out, "{% endblock %}");
                if self.rng.chance(45) {
                    let call = self.block_call(&name);
                    self.print(&call, out);
                }
            }
            St::Include => self.include_stmt(depth, out),
            St::MacroCall => {
                let call = self.macro_call();
                let call = if self.rng.chance(25) {
                    format!("{}|{}", call, self.filter(2))
                } else {
                    call
                };
                self.print(&call, out);
            }
            St::Verbatim => {
                let text = self.text();
                push(out, "{% verbatim %}{{ v }}");
                out.extend(text);
                push(out, "{% endverbatim %}");
            }
            St::Misc => self.misc_stmt(out),
        }
    }

    /// The body of a tag: one to three statements; `keeps_blocks` says whether a `{% block %}`
    /// may appear in it.
    fn body(&mut self, depth: u32, keeps_blocks: bool, out: &mut Vec<u8>) {
        let saved_block_ok = self.block_ok;
        self.block_ok = saved_block_ok && keeps_blocks;
        let saved_scope = self.scope.len();
        let n = *self.rng.weighted(&[(45, 1usize), (40, 2), (15, 3)]);
        for _ in 0..n {
            self.stmt(depth, out);
        }
        self.scope.truncate(saved_scope);
        self.block_ok = saved_block_ok;
    }

    fn print(&mut self, expr: &str, out: &mut Vec<u8>) {
        match self.rng.below(100) {
            0..=7 => push(out, &format!("{{{{- {expr} -}}}}")),
            8..=11 => push(out, &format!("{{{{\n{expr}\n}}}}")),
            12..=17 => {
                // a line break before the last filter (not inside a string literal)
                match expr
                    .rfind('|')
                    .filter(|&i| !expr[i..].contains(['\'', '"']))
                {
                    Some(i) => push(out, &format!("{{{{ {}\n{} }}}}", &expr[..i], &expr[i..])),
                    None => push(out, &format!("{{{{ {expr} }}}}")),
                }
            }
            _ => push(out, &format!("{{{{ {expr} }}}}")),
        }
    }

    fn text(&mut self) -> Vec<u8> {
        let mut out = Vec::new();
        let n = self.rng.range(1, 3);
        for _ in 0..n {
            if self.p.invalid > 0 && self.rng.chance(self.p.invalid / 2 + 1) {
                let piece = INVALID_PIECES[self.rng.below(INVALID_PIECES.len())];
                out.extend_from_slice(piece);
            } else {
                out.extend_from_slice(self.rng.pick(TEXT_PIECES).as_bytes());
            }
        }
        out
    }

    /// The argument part of `{% autoescape ... %}`, with its leading space.
    fn autoescape_arg(&mut self) -> String {
        match self.rng.below(100) {
            0..=9 => String::new(),
            10..=54 => {
                let strategy = (*self.rng.pick(VALID_STRATEGIES)).to_owned();
                format!(" {}", self.literal(strategy.as_bytes()))
            }
            55..=71 => " false".to_owned(),
            72..=74 => " true".to_owned(),
            75..=78 => {
                let strategy = (*self.rng.pick(INVALID_STRATEGIES)).to_owned();
                format!(" {}", self.literal(strategy.as_bytes()))
            }
            79..=80 => format!(
                " {}",
                self.rng.pick(&["0", "1", "1.5", "1e100", "''", "'0'"])
            ),
            81..=86 => " \"html\"".to_owned(),
            87..=91 => " 'html_attr_relaxed'".to_owned(),
            92..=98 => " 'js'".to_owned(),
            _ => format!(
                " {}",
                self.rng.pick(&["v", "'js' ~ ''", "-1", "[]", "('css')"])
            ),
        }
    }

    fn set_stmt(&mut self, out: &mut Vec<u8>) {
        if self.rng.chance(15) {
            // multi-target
            let a = self.fresh_var("x");
            let b = self.fresh_var("x");
            let ea = self.expr(1);
            let eb = self.expr(1);
            push(
                out,
                &format!("{{% set {a}, {b} = {}, {} %}}", ea.src, eb.src),
            );
            self.print(&a, out);
            self.print(&b, out);
            self.add_local(a, Kind::Any);
            self.add_local(b, Kind::Any);
            return;
        }
        let name = self.fresh_var("x");
        let value = self.expr(0);
        push(out, &format!("{{% set {name} = {} %}}", value.src));
        if self.rng.chance(75) {
            let use_ = self.use_of(&name);
            self.print(&use_, out);
        }
        self.add_local(name, Kind::Any);
    }

    fn capture_stmt(&mut self, depth: u32, out: &mut Vec<u8>) {
        let name = self.fresh_var("c");
        push(out, &format!("{{% set {name} %}}"));
        match self.rng.below(12) {
            0 => {}
            1 => {
                let text = self.text();
                out.extend(text);
            }
            2 => {
                let lit = self.constant();
                push(out, &format!("{{{{ {lit} }}}}"));
            }
            3 => push(
                out,
                &format!(
                    "{{{{ {} }}}}",
                    self.rng.pick(&["null", "5", "true", "false", "''", "1.5"])
                ),
            ),
            4 => push(out, " "),
            _ => self.body(depth + 1, true, out),
        }
        push(out, "{% endset %}");
        let uses = self.rng.range(1, 2);
        for _ in 0..uses {
            let use_ = self.use_of(&name);
            self.print(&use_, out);
        }
        self.add_local(name, Kind::Markup);
    }

    /// A print of a freshly set variable: as is, through filters, tested.
    fn use_of(&mut self, name: &str) -> String {
        match self.rng.below(14) {
            0..=4 => name.to_owned(),
            5 => format!("{name}|upper"),
            6 => format!("{name}|e"),
            7 => format!("{name} ~ ''"),
            8 => format!("{name}|raw"),
            9 => format!("{name} is empty ? 'E' : 'N'"),
            10 => format!("{name}|length"),
            11 => format!("{name}|trim"),
            12 => format!("{name}|e('js')"),
            _ => format!("{name}|{}", self.filter(2)),
        }
    }

    fn add_local(&mut self, name: String, kind: Kind) {
        self.scope.push(Local {
            name,
            kind,
            keys: Vec::new(),
        });
    }

    fn apply_chain(&mut self) -> String {
        let n = *self.rng.weighted(&[(65, 1usize), (28, 2), (7, 3)]);
        let mut parts = Vec::new();
        for _ in 0..n {
            let f = match self.rng.below(16) {
                0 => "upper".to_owned(),
                1 => "lower".to_owned(),
                2 => "e".to_owned(),
                3 => "escape('js')".to_owned(),
                4 => "raw".to_owned(),
                5 => "nl2br".to_owned(),
                6 => "spaceless".to_owned(),
                7 => "trim".to_owned(),
                8 => "striptags".to_owned(),
                9 => "title".to_owned(),
                10 => "e('url')".to_owned(),
                11 => "replace({'<': '[', '&': '+'})".to_owned(),
                12 => "e('html_attr')".to_owned(),
                13 => "e('css')".to_owned(),
                14 => self.escape_call(),
                _ => "default('<d>')".to_owned(),
            };
            parts.push(f);
        }
        parts.join("|")
    }

    fn condition(&mut self) -> String {
        let var = self.var_name();
        match self.rng.below(8) {
            0 => var,
            1 => format!("{var} is empty"),
            2 => format!("{var} is defined"),
            3 => format!("not {var}"),
            4 => format!("{var} == '<b>'"),
            5 => format!("{var} is same as({var})"),
            6 => format!("{var}|length > 1"),
            _ => self.expr(1).src,
        }
    }

    fn for_stmt(&mut self, depth: u32, out: &mut Vec<u8>) {
        let item = self.fresh_var("it");
        let seq = match self.list_source() {
            Some(name) => name,
            None => {
                let a = self.constant();
                let b = self.expr(2);
                format!("[{a}, {}]", b.src)
            }
        };
        if self.rng.chance(20) {
            let key = self.fresh_var("k");
            push(out, &format!("{{% for {key}, {item} in {seq} %}}"));
            self.add_local(key, Kind::Scalar);
        } else {
            push(out, &format!("{{% for {item} in {seq} %}}"));
        }
        self.add_local(item.clone(), Kind::Any);
        if self.rng.chance(50) {
            let use_ = self.use_of(&item);
            self.print(&use_, out);
        }
        self.body(depth + 1, false, out);
        if self.rng.chance(15) {
            push(out, "{% else %}");
            self.body(depth + 1, false, out);
        }
        push(out, "{% endfor %}");
    }

    /// A list or mapping variable in scope.
    fn list_source(&mut self) -> Option<String> {
        let lists: Vec<String> = self
            .scope
            .iter()
            .filter(|l| matches!(l.kind, Kind::List | Kind::Map))
            .map(|l| l.name.clone())
            .collect();
        if lists.is_empty() || self.rng.chance(25) {
            None
        } else {
            Some(self.rng.pick(&lists).clone())
        }
    }

    fn include_stmt(&mut self, depth: u32, out: &mut Vec<u8>) {
        if self.sandbox.is_some() && self.rng.chance(35) {
            let name = self.partial();
            let lit = self.literal(name.as_bytes());
            let inner = if self.rng.chance(70) {
                format!("{{% include {lit} %}}")
            } else {
                format!("{{{{ include({lit}) }}}}")
            };
            push(out, &format!("{{% sandbox %}}{inner}{{% endsandbox %}}"));
            return;
        }
        match self.rng.below(13) {
            0..=2 => {
                let name = self.partial();
                let lit = self.literal(name.as_bytes());
                push(out, &format!("{{% include {lit} %}}"));
            }
            3 => {
                let name = self.partial();
                let lit = self.literal(name.as_bytes());
                let value = self.expr(1);
                let only = if self.rng.chance(40) { " only" } else { "" };
                push(
                    out,
                    &format!("{{% include {lit} with {{v: {}}}{only} %}}", value.src),
                );
            }
            4..=5 => {
                let name = self.partial();
                let lit = self.literal(name.as_bytes());
                let call = match self.rng.below(4) {
                    0 => format!("include({lit})|{}", self.filter(2)),
                    1 => format!("include({lit}, {{v: '<i>'}})"),
                    2 => format!("include({lit}, with_context = false)"),
                    _ => format!("include({lit})"),
                };
                self.print(&call, out);
            }
            6 => {
                let name = self.partial();
                let lit = self.literal(name.as_bytes());
                let var = self.fresh_var("i");
                push(out, &format!("{{% set {var} = include({lit}) %}}"));
                let use_ = self.use_of(&var);
                self.print(&use_, out);
            }
            7..=8 => self.embed_stmt(depth, out),
            9 => {
                let name = self.partial();
                let lit = self.literal(name.as_bytes());
                let call = match self.rng.below(3) {
                    0 => format!("source({lit})"),
                    1 => format!("source({lit})|e"),
                    _ => format!("source({lit})|{}", self.filter(2)),
                };
                self.print(&call, out);
            }
            10 => {
                // a trait
                let block = self.trait_block();
                let call = self.block_call(&block);
                self.print(&call, out);
            }
            _ => {
                self.uses_string_loader = true;
                let inner = match self.rng.below(4) {
                    0 => "{{ v }}<b>".to_owned(),
                    1 => "{% autoescape 'js' %}{{ v }}{% endautoescape %}".to_owned(),
                    2 => "<{{ v|raw }}>".to_owned(),
                    _ => "{{ '<i>' }}{{ v|upper }}".to_owned(),
                };
                let lit = self.literal(inner.as_bytes());
                let call = match self.rng.below(5) {
                    0 => format!("include(template_from_string({lit}))"),
                    4 => {
                        let name = (*self.rng.pick(TEMPLATE_NAMES)).to_owned();
                        let name = self.literal(name.as_bytes());
                        format!("template_from_string({lit}, {name}).getDefaultEscapeStrategy()")
                    }
                    1 => {
                        let name = (*self.rng.pick(TEMPLATE_NAMES)).to_owned();
                        let name = self.literal(name.as_bytes());
                        format!("include(template_from_string({lit}, {name}))")
                    }
                    2 => format!("template_from_string({lit}).render({{v: '<x>'}})"),
                    _ => format!("include(template_from_string({lit}))|upper"),
                };
                self.print(&call, out);
            }
        }
    }

    /// A partial template (new or existing) whose body uses the data.
    fn partial(&mut self) -> String {
        if !self.partials.is_empty() && (self.partials.len() >= 3 || self.rng.chance(50)) {
            return self.rng.pick(&self.partials).clone();
        }
        let name = self.fresh_name();
        let body = self.aux_body(|g, out| {
            let n = g.rng.range(1, 3);
            for _ in 0..n {
                g.stmt(1, out);
            }
        });
        self.extra.push((name.clone(), body));
        self.partials.push(name.clone());
        name
    }

    /// Runs `f` to generate the body of another template that sees the data variables.
    fn aux_body(&mut self, f: impl FnOnce(&mut Gen, &mut Vec<u8>)) -> Vec<u8> {
        let data_scope = self.data_scope();
        let saved_scope = std::mem::replace(&mut self.scope, data_scope);
        let saved = (self.in_aux, self.block_ok);
        self.in_aux = true;
        self.block_ok = false;
        let mut out = Vec::new();
        f(self, &mut out);
        (self.in_aux, self.block_ok) = saved;
        self.scope = saved_scope;
        out
    }

    fn data_scope(&self) -> Vec<Local> {
        self.data
            .iter()
            .map(|(name, value)| Local {
                name: name.clone(),
                kind: value.kind(),
                keys: match value {
                    Val::Map(entries) => entries.iter().map(|(k, _)| k.clone()).collect(),
                    _ => Vec::new(),
                },
            })
            .collect()
    }

    fn embed_stmt(&mut self, depth: u32, out: &mut Vec<u8>) {
        let name = self.fresh_name();
        let block = self.fresh_var("eb");
        let mut target = Vec::new();
        let text = self.text();
        target.extend(text);
        push(&mut target, &format!("{{% block {block} %}}"));
        let default = self.aux_body(|g, out| g.stmt(1, out));
        target.extend(default);
        push(&mut target, "{% endblock %}");
        self.extra.push((name.clone(), target));
        let lit = self.literal(name.as_bytes());
        push(out, &format!("{{% embed {lit} %}}{{% block {block} %}}"));
        if self.rng.chance(30) {
            self.print("parent()", out);
        }
        let saved = (self.block_ok, self.in_aux);
        self.block_ok = false;
        self.in_aux = true;
        self.body(depth + 1, false, out);
        (self.block_ok, self.in_aux) = saved;
        push(out, "{% endblock %}{% endembed %}");
    }

    /// A block of a trait template used by the main template (`{% use %}`).
    fn trait_block(&mut self) -> String {
        let name = self.fresh_name();
        let block = self.fresh_var("tb");
        let mut source = Vec::new();
        if self.rng.chance(30) {
            push(&mut source, "\n");
        }
        push(&mut source, &format!("{{% block {block} %}}"));
        let body = self.aux_body(|g, out| {
            let n = g.rng.range(1, 2);
            for _ in 0..n {
                g.stmt(1, out);
            }
        });
        source.extend(body);
        push(&mut source, "{% endblock %}");
        self.extra.push((name.clone(), source));
        let lit = self.literal(name.as_bytes());
        push(&mut self.header, &format!("{{% use {lit} %}}"));
        block
    }

    /// `block('name')`, sometimes through a filter.
    fn block_call(&mut self, name: &str) -> String {
        match self.rng.below(6) {
            0 => format!("block('{name}')|upper"),
            1 => format!("block('{name}')|e"),
            2 => format!("block('{name}') ~ ''"),
            _ => format!("block('{name}')"),
        }
    }

    fn misc_stmt(&mut self, out: &mut Vec<u8>) {
        match self.rng.below(12) {
            0..=1 => {
                self.uses_dump = true;
                if self.rng.chance(60) {
                    self.debug = true;
                }
                // strings and scalars only (an `include ... with` may still pass an object), and
                // only html-safe filters here
                let vars: Vec<String> = self
                    .scope
                    .iter()
                    .filter(|l| matches!(l.kind, Kind::Str | Kind::Scalar))
                    .map(|l| l.name.clone())
                    .collect();
                let arg = if vars.is_empty() {
                    self.constant()
                } else {
                    self.rng.pick(&vars).clone()
                };
                let call = match self.rng.below(5) {
                    0 => format!("dump({arg})|nl2br"),
                    1 => format!("dump({arg})|e"),
                    _ => format!("dump({arg})"),
                };
                self.print(&call, out);
            }
            2 => self.print("_self", out),
            3 => {
                let map = self.var_of(&[Kind::Map]);
                self.print(&format!("attribute({map}, 'k')"), out);
            }
            4 => {
                let var = self.var_of(&[Kind::Str]);
                push(out, &format!("{{% do {var}|e('bogus') %}}"));
            }
            5..=7 => {
                let m = self.var_of(&[Kind::Markup]);
                let call = match self.rng.below(8) {
                    0 => format!("{m}.isSafeFor('js') ? 'Y' : 'N'"),
                    1 => format!("{m}.withContent('<x>')"),
                    2 => format!("{m}.charset"),
                    3 => format!("{m}.count"),
                    4 => format!("{m}.jsonSerialize()"),
                    5 => format!("{m}|json_encode"),
                    6 => format!("{m} == '<b>' ? 'eq' : 'ne'"),
                    _ => format!("{m}|length"),
                };
                self.print(&call, out);
            }
            8 => {
                let cycle = format!("cycle(['<a>', {}], 1)", self.expr(2).src);
                self.print(&cycle, out);
            }
            9 => {
                let e = self.expr(1);
                self.print(&format!("max({}, '<z>')", e.src), out);
            }
            10 => {
                let e = self.expr(1);
                self.print(&format!("\"<#{{{}}}>\"", e.src), out);
            }
            _ => {
                let e = self.expr(1);
                self.print(&format!("[{}]|join", e.src), out);
            }
        }
    }

    // ---- macros -------------------------------------------------------------------------------

    /// A macro call expression (creating the macro when needed).
    fn macro_call(&mut self) -> String {
        if self.rng.chance(35) {
            return self.lib_call();
        }
        let index = if self.macros.is_empty() || (self.macros.len() < 2 && self.rng.chance(30)) {
            self.new_self_macro()
        } else {
            self.rng.below(self.macros.len())
        };
        let name = self.macros[index].name.clone();
        let params = self.macros[index].params.len();
        let args = self.call_args(params);
        if self.rng.chance(25) {
            let alias = match &self.self_alias {
                Some(alias) => alias.clone(),
                None => {
                    push(&mut self.header, "{% import _self as me %}");
                    self.self_alias = Some("me".to_owned());
                    "me".to_owned()
                }
            };
            format!("{alias}.{name}({args})")
        } else {
            format!("_self.{name}({args})")
        }
    }

    fn call_args(&mut self, params: usize) -> String {
        let count = if self.rng.chance(20) {
            self.rng.below(params + 1)
        } else {
            params
        };
        let mut args = Vec::new();
        for _ in 0..count {
            args.push(self.expr(2).src);
        }
        args.join(", ")
    }

    fn new_self_macro(&mut self) -> usize {
        let name = format!("m{}", self.macros.len() + 1);
        let (params, body) = self.macro_body();
        let wrap = if self.rng.chance(25) {
            Some(self.autoescape_arg())
        } else {
            None
        };
        self.macros.push(MacroDef {
            name,
            params,
            body,
            wrap,
        });
        self.macros.len() - 1
    }

    /// Parameters and body of a macro: the body prints its arguments.
    fn macro_body(&mut self) -> (Vec<(String, Option<String>)>, Vec<u8>) {
        let count = self.rng.range(0, 2);
        let mut params = Vec::new();
        for name in &["a", "bb"][..count] {
            let default = if self.rng.chance(25) {
                Some(self.constant())
            } else {
                None
            };
            params.push(((*name).to_owned(), default));
        }
        let locals: Vec<Local> = params
            .iter()
            .map(|(name, _)| Local {
                name: name.clone(),
                kind: Kind::Any,
                keys: Vec::new(),
            })
            .collect();
        let saved_scope = std::mem::replace(&mut self.scope, locals);
        let saved = (self.in_macro, self.block_ok);
        self.in_macro = true;
        self.block_ok = false;
        let mut body = Vec::new();
        let n = self.rng.range(1, 3);
        for _ in 0..n {
            self.stmt(1, &mut body);
        }
        (self.in_macro, self.block_ok) = saved;
        self.scope = saved_scope;
        (params, body)
    }

    /// A call of a macro of another template (`{% import %}` or `{% from %}`).
    fn lib_call(&mut self) -> String {
        if self.libs.is_empty() || (self.libs.len() < 2 && self.rng.chance(20)) {
            self.new_lib();
        }
        let index = self.rng.below(self.libs.len());
        let lib = self.libs[index].clone();
        if !lib.from.is_empty() && self.rng.chance(40) {
            let (local, params) = self.rng.pick(&lib.from).clone();
            let args = self.call_args(params);
            return format!("{local}({args})");
        }
        let (name, params) = self.rng.pick(&lib.macros).clone();
        let args = self.call_args(params);
        format!("{}.{name}({args})", lib.alias)
    }

    fn new_lib(&mut self) {
        let template = self.fresh_name();
        let alias = format!("lib{}", self.libs.len() + 1);
        let mut source = Vec::new();
        let mut macros = Vec::new();
        let count = self.rng.range(1, 2);
        for i in 0..count {
            let name = format!("mm{}", i + 1);
            let saved = self.in_aux;
            self.in_aux = true;
            let (params, body) = self.macro_body();
            self.in_aux = saved;
            let wrap = if self.rng.chance(20) {
                Some(self.autoescape_arg())
            } else {
                None
            };
            macros.push((name.clone(), params.len()));
            render_macro(
                &MacroDef {
                    name,
                    params,
                    body,
                    wrap,
                },
                &mut source,
            );
        }
        self.extra.push((template.clone(), source));
        let lit = self.literal(template.as_bytes());
        push(
            &mut self.header,
            &format!("{{% import {lit} as {alias} %}}"),
        );
        let mut from = Vec::new();
        if self.rng.chance(40) {
            let (name, params) = macros[0].clone();
            let local = format!("f{}", self.libs.len() + 1);
            push(
                &mut self.header,
                &format!("{{% from {lit} import {name} as {local} %}}"),
            );
            from.push((local, params));
        }
        self.libs.push(Lib {
            alias,
            macros,
            from,
        });
    }

    // ---- micro ------------------------------------------------------------------------------------

    /// One to four prints of the data with explicit strategies and charsets.
    fn micro_template(&mut self) -> Vec<u8> {
        let mut out = Vec::new();
        let n = self.rng.range(1, 4);
        for _ in 0..n {
            let var = self.var_of(&[Kind::Str, Kind::Markup]);
            let print = match self.rng.below(14) {
                0..=1 => var,
                2..=5 => format!("{var}|{}", self.escape_call()),
                6 => {
                    let a = self.escape_call();
                    let b = self.escape_call();
                    format!("{var}|{a}|{b}")
                }
                7 => format!("{var}|nl2br"),
                8 => format!("{var}|raw"),
                9 => {
                    let lit = self.constant();
                    format!("{lit}|{}", self.escape_call())
                }
                10 => {
                    let arg = self.autoescape_arg();
                    push(&mut out, &format!("{{% autoescape{arg} %}}"));
                    self.print(&var, &mut out);
                    push(&mut out, "{% endautoescape %}");
                    continue;
                }
                11 => format!("{var}|{}|{}", self.filter(2), self.escape_call()),
                12 => format!("{var} ~ ''"),
                _ => format!("{var}|trim|{}", self.escape_call()),
            };
            self.print(&print, &mut out);
        }
        out
    }

    // ---- inheritance ----------------------------------------------------------------------------

    /// The main template as a child of a layout (blocks, `parent()`, `block()`).
    fn child_template(&mut self) -> Vec<u8> {
        let layout = self.fresh_name();
        let content = "content";
        let side = "side";
        let mut parent = Vec::new();
        let text = self.text();
        parent.extend(text);
        let with_side = self.rng.chance(50);
        let side_wrap = if self.rng.chance(50) {
            Some(self.autoescape_arg())
        } else {
            None
        };
        let layout_body = self.aux_body(|g, out| {
            push(out, &format!("{{% block {content} %}}"));
            g.stmt(1, out);
            push(out, "{% endblock %}");
            if with_side {
                if let Some(arg) = &side_wrap {
                    push(out, &format!("{{% autoescape{arg} %}}"));
                }
                push(out, &format!("{{% block {side} %}}"));
                g.stmt(1, out);
                push(out, "{% endblock %}");
                if side_wrap.is_some() {
                    push(out, "{% endautoescape %}");
                }
            }
        });
        parent.extend(layout_body);
        self.extra.push((layout.clone(), parent));

        let mut child = Vec::new();
        let lit = self.literal(layout.as_bytes());
        push(&mut child, &format!("{{% extends {lit} %}}"));
        let wrap_child = self.rng.chance(6);
        if wrap_child {
            push(&mut child, "{% autoescape 'js' %}");
        }
        push(&mut child, &format!("{{% block {content} %}}"));
        if self.rng.chance(50) {
            let call = match self.rng.below(4) {
                0 => "parent()|upper",
                1 => "parent()|e",
                _ => "parent()",
            };
            self.print(call, &mut child);
        }
        self.block_ok = true;
        self.body(1, true, &mut child);
        if with_side && self.rng.chance(40) {
            let call = self.block_call(side);
            self.print(&call, &mut child);
        }
        push(&mut child, "{% endblock %}");
        if wrap_child {
            push(&mut child, "{% endautoescape %}");
        }
        if with_side && self.rng.chance(50) {
            push(&mut child, &format!("{{% block {side} %}}"));
            if self.rng.chance(50) {
                self.print("parent()", &mut child);
            }
            self.body(1, false, &mut child);
            push(&mut child, "{% endblock %}");
        }
        child
    }

    // ---- expressions --------------------------------------------------------------------------

    fn expr(&mut self, depth: u32) -> Expr {
        let leaf_chance = 30 + depth * 22;
        if depth >= 3 || self.rng.chance(leaf_chance) {
            return self.leaf(depth);
        }
        match self.rng.below(100) {
            0..=47 => {
                let inner = self.expr(depth + 1);
                let filter = self.filter(depth + 1);
                Expr::atom(format!("{}|{filter}", inner.wrapped()))
            }
            48..=57 => {
                let a = self.expr(depth + 1);
                let b = self.expr(depth + 1);
                Expr::compound(format!("{} ~ {}", a.wrapped(), b.wrapped()))
            }
            58..=67 => {
                let c = self.expr(depth + 1);
                let a = self.expr(depth + 1);
                let b = self.expr(depth + 1);
                Expr::compound(format!(
                    "{} ? {} : {}",
                    c.wrapped(),
                    a.wrapped(),
                    b.wrapped()
                ))
            }
            68..=72 => {
                let a = self.expr(depth + 1);
                let b = self.expr(depth + 1);
                Expr::compound(format!("{} ?: {}", a.wrapped(), b.wrapped()))
            }
            73..=80 => {
                let a = self.coalesce_left();
                let b = self.expr(depth + 1);
                Expr::compound(format!("{a} ?? {}", b.wrapped()))
            }
            81..=84 => {
                let c = self.expr(depth + 1);
                let a = self.expr(depth + 1);
                Expr::compound(format!("{} ? {}", c.wrapped(), a.wrapped()))
            }
            85..=88 => {
                let a = self.expr(depth + 1);
                Expr::atom(format!("({})", a.src))
            }
            89..=92 => {
                let a = self.expr(depth + 1);
                let b = self.expr(depth + 1);
                let op = *self
                    .rng
                    .pick(&["==", "!=", "in", "not in", "starts with", "<"]);
                Expr::compound(format!("{} {op} {}", a.wrapped(), b.wrapped()))
            }
            93..=95 => {
                let a = self.expr(depth + 1);
                let test = *self
                    .rng
                    .pick(&["empty", "null", "iterable", "same as(true)"]);
                Expr::compound(format!("{} is {test}", a.wrapped()))
            }
            _ => {
                let a = self.expr(depth + 1);
                Expr::atom(format!("\"<#{{{}}}>\"", a.src))
            }
        }
    }

    /// The left operand of `??` (it must support the `defined` test).
    fn coalesce_left(&mut self) -> String {
        match self.rng.below(6) {
            0 => "undef".to_owned(),
            1 => {
                let map = self.var_of(&[Kind::Map]);
                format!("{map}.k")
            }
            2 => "'<c>'".to_owned(),
            _ => self.var_name(),
        }
    }

    fn leaf(&mut self, depth: u32) -> Expr {
        match self.rng.below(100) {
            0..=44 => Expr::atom(self.var_name()),
            45..=59 => Expr::atom(self.constant()),
            60..=64 => Expr::atom(
                (*self
                    .rng
                    .pick(&["42", "0", "1.5", "true", "false", "null", "(-1)", "1e3"]))
                .to_owned(),
            ),
            65..=71 => {
                if let Some(attr) = self.attribute() {
                    Expr::atom(attr)
                } else {
                    Expr::atom(self.var_name())
                }
            }
            72..=73 => {
                let a = self.constant();
                let b = self.var_name();
                Expr::atom(format!("[{a}, {b}]"))
            }
            74..=75 => Expr::atom(if self.rng.chance(50) {
                "_charset"
            } else {
                "_self"
            }),
            76..=81 if !self.in_macro && !self.in_aux && depth < 3 => Expr::atom(self.macro_call()),
            82..=86 if !self.in_macro && !self.in_aux => {
                let name = self.partial();
                let lit = self.literal(name.as_bytes());
                Expr::atom(if self.rng.chance(70) {
                    format!("include({lit})")
                } else {
                    format!("source({lit})")
                })
            }
            87..=89 => Expr::atom("undef"),
            90..=93 => {
                let lit = self.constant();
                Expr::atom(format!("{{'k': {lit}}}"))
            }
            _ => Expr::atom(self.var_name()),
        }
    }

    /// A variable in scope (any kind), or `undef` when there is none.
    fn var_name(&mut self) -> String {
        if self.scope.is_empty() {
            return "undef".to_owned();
        }
        let weights: Vec<(u32, usize)> = self
            .scope
            .iter()
            .enumerate()
            .map(|(i, l)| {
                let w = match l.kind {
                    Kind::Str => 30,
                    Kind::Markup => 28,
                    Kind::Any => 20,
                    Kind::Scalar => 8,
                    Kind::List | Kind::Map => 7,
                    Kind::Object => 6,
                };
                (w, i)
            })
            .collect();
        let index = *self.rng.weighted(&weights);
        self.scope[index].name.clone()
    }

    /// A variable of one of the kinds (or any variable).
    fn var_of(&mut self, kinds: &[Kind]) -> String {
        let names: Vec<String> = self
            .scope
            .iter()
            .filter(|l| kinds.contains(&l.kind))
            .map(|l| l.name.clone())
            .collect();
        if names.is_empty() {
            self.var_name()
        } else {
            self.rng.pick(&names).clone()
        }
    }

    /// `map.k`, `map['h']`, `arr[0]`, `arr.1`, `map.missing`.
    fn attribute(&mut self) -> Option<String> {
        let candidates: Vec<Local> = self
            .scope
            .iter()
            .filter(|l| matches!(l.kind, Kind::List | Kind::Map))
            .cloned()
            .collect();
        if candidates.is_empty() {
            return None;
        }
        let local = self.rng.pick(&candidates).clone();
        Some(match local.kind {
            Kind::Map if !local.keys.is_empty() && self.rng.chance(85) => {
                let key = self.rng.pick(&local.keys).clone();
                if self.rng.chance(50) {
                    format!("{}.{key}", local.name)
                } else {
                    format!("{}['{key}']", local.name)
                }
            }
            Kind::Map => format!("{}.missing", local.name),
            _ => match self.rng.below(3) {
                0 => format!("{}[0]", local.name),
                1 => format!("{}.1", local.name),
                _ => format!("{}|first", local.name),
            },
        })
    }

    /// A string literal of an interesting string.
    fn constant(&mut self) -> String {
        let bytes = self.string();
        self.literal(&bytes)
    }

    /// A Twig string literal of arbitrary bytes (escape sequences for control characters, the quote,
    /// backslashes, `#` in double quotes and bytes that are not valid UTF-8).
    fn literal(&mut self, bytes: &[u8]) -> String {
        let double = self.rng.chance(35);
        let raw_unicode = self.rng.chance(85);
        let raw_newline = self.rng.chance(50);
        let quote = if double { '"' } else { '\'' };
        let mut out = String::with_capacity(bytes.len() + 2);
        out.push(quote);
        for chunk in bytes.utf8_chunks() {
            for c in chunk.valid().chars() {
                match c {
                    '\\' => out.push_str("\\\\"),
                    '\'' if !double => out.push_str("\\'"),
                    '"' if double => out.push_str("\\\""),
                    '#' if double => out.push_str("\\x23"),
                    '\n' if raw_newline => out.push('\n'),
                    '\n' => out.push_str("\\n"),
                    '\t' => out.push_str("\\t"),
                    c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                        let _ = write!(out, "\\x{:02x}", c as u32);
                    }
                    c if !c.is_ascii() && !raw_unicode => {
                        let mut buf = [0u8; 4];
                        for b in c.encode_utf8(&mut buf).bytes() {
                            let _ = write!(out, "\\x{b:02x}");
                        }
                    }
                    c => out.push(c),
                }
            }
            for b in chunk.invalid() {
                let _ = write!(out, "\\x{b:02x}");
            }
        }
        out.push(quote);
        out
    }

    // ---- filters ------------------------------------------------------------------------------

    fn filter(&mut self, depth: u32) -> String {
        match self.rng.below(100) {
            0..=29 => self.escape_call(),
            30..=37 if self.rng.chance(4) => "raw(1)".to_owned(),
            30..=37 => "raw".to_owned(),
            38..=43 => "nl2br".to_owned(),
            44..=46 => "spaceless".to_owned(),
            47..=52 => (*self.rng.pick(&["upper", "lower", "title", "capitalize"])).to_owned(),
            53..=59 => (*self.rng.pick(&[
                "trim",
                "trim",
                "trim('<>')",
                "trim(side='left')",
                "trim(null, 'right')",
                "trim(\" \\t\\n\\r\\0\\x0B\")",
                "trim(' ')",
                "trim(character_mask=null)",
            ]))
            .to_owned(),
            60..=62 => (*self
                .rng
                .pick(&["striptags", "striptags('<b>')", "striptags('<b><i>')"]))
            .to_owned(),
            63..=65 => (*self.rng.pick(&[
                "replace({'<': '[', '>': ']'})",
                "replace({'&': '&amp;'})",
                "replace({'a': '<a>'})",
            ]))
            .to_owned(),
            66..=67 => {
                let arg = self.expr(depth + 1);
                format!("format({})", arg.src)
            }
            68..=72 => {
                let arg = self.expr(depth + 1);
                format!("default({})", arg.src)
            }
            73..=76 => (*self.rng.pick(&["first", "last"])).to_owned(),
            77..=79 => (*self.rng.pick(&["join", "join(', ')", "join('<br>')"])).to_owned(),
            80..=81 => (*self.rng.pick(&["slice(1)", "slice(0, 2)", "slice(-2)"])).to_owned(),
            82 => "reverse".to_owned(),
            83..=84 => "url_encode".to_owned(),
            85..=86 => (*self.rng.pick(&[
                "json_encode",
                "json_encode(constant('JSON_HEX_TAG'))",
                "json_encode(constant('JSON_UNESCAPED_UNICODE'))",
            ]))
            .to_owned(),
            87..=88 => (*self.rng.pick(&[
                "convert_encoding('UTF-8', 'ISO-8859-1')",
                "convert_encoding('ISO-8859-1', 'UTF-8')",
            ]))
            .to_owned(),
            89 => "length".to_owned(),
            90 => (*self.rng.pick(&["abs", "round", "number_format(2)"])).to_owned(),
            91 => (*self.rng.pick(&[
                "map(x => x)",
                "map(x => x|upper)",
                "map(x => x|e)",
                "filter(x => x)",
                "reduce((c, x) => c ~ x, '')",
                "find(x => x)",
            ]))
            .to_owned(),
            92..=93 => self.escape_call(),
            94 => "split(',')".to_owned(),
            95 => "keys".to_owned(),
            96 => "merge(['<m>'])".to_owned(),
            _ => self.escape_call(),
        }
    }

    /// `escape`/`e` with any arguments.
    fn escape_call(&mut self) -> String {
        let name = if self.rng.chance(55) { "e" } else { "escape" };
        // strategy only up to here, strategy and charset up to 80
        let strategy_only = 15 + 60 - self.p.escape_charset.min(55) as usize;
        match self.rng.below(100) {
            0..=14 => name.to_owned(),
            n if n < strategy_only => {
                let strategy = self.strategy();
                format!("{name}({})", self.literal(strategy.as_bytes()))
            }
            n if n < 80 => {
                let strategy = self.strategy();
                let strategy = self.literal(strategy.as_bytes());
                let charset = self.charset_arg();
                format!("{name}({strategy}, {charset})")
            }
            80..=85 => {
                let strategy = self.strategy();
                let strategy = self.literal(strategy.as_bytes());
                match self.rng.below(4) {
                    0 => format!("{name}(strategy={strategy})"),
                    1 => format!("{name}(strategy: {strategy})"),
                    2 => {
                        let charset = self.charset_arg();
                        format!("{name}(charset={charset}, strategy={strategy})")
                    }
                    _ => {
                        let charset = self.charset_arg();
                        format!("{name}(charset={charset})")
                    }
                }
            }
            86..=89 => {
                let strategy = self.strategy();
                let strategy = self.literal(strategy.as_bytes());
                let flag = *self.rng.pick(&["true", "false", "1", "autoescape=true"]);
                if flag == "autoescape=true" {
                    format!("{name}({strategy}, autoescape=true)")
                } else {
                    format!("{name}({strategy}, null, {flag})")
                }
            }
            90..=94 => {
                let var = if self.scope.iter().any(|l| l.name == "strat") {
                    "strat".to_owned()
                } else {
                    self.var_name()
                };
                format!("{name}({var})")
            }
            _ => format!(
                "{name}({})",
                self.rng.pick(&[
                    "null",
                    "1",
                    "true",
                    "false",
                    "[]",
                    "'html', 'UTF-8', true, 1",
                    "'js', cs",
                    "autoescape=true",
                    "charset='UTF-8', autoescape=true",
                    "'html_attr', autoescape: true",
                ])
            ),
        }
    }

    fn charset_arg(&mut self) -> String {
        match self.rng.below(20) {
            0..=3 => "null".to_owned(),
            4 if self.scope.iter().any(|l| l.name == "cs") => "cs".to_owned(),
            5 => (*self.rng.pick(&["'0'", "false", "0", "''"])).to_owned(),
            _ => {
                let charset = *self.rng.pick(CHARSETS);
                self.literal(charset.as_bytes())
            }
        }
    }

    // ---- the case -----------------------------------------------------------------------------

    fn finish(self, source: Vec<u8>) -> Json {
        let mut fields: Vec<(String, Json)> = vec![
            ("id".to_owned(), Json::str(self.id.clone())),
            ("env".to_owned(), Json::str("plain")),
        ];
        let mut templates = vec![(self.main.clone(), source)];
        templates.extend(self.extra);
        let mut text = Vec::new();
        let mut binary = Vec::new();
        for (name, source) in templates {
            match String::from_utf8(source) {
                Ok(s) => text.push((name, Json::str(s))),
                Err(error) => {
                    text.push((name.clone(), Json::str("")));
                    binary.push((name, Json::str(base64_encode(error.as_bytes()))));
                }
            }
        }
        fields.push(("templates".to_owned(), Json::Object(text)));
        if !binary.is_empty() {
            fields.push(("templates_b64".to_owned(), Json::Object(binary)));
        }
        if self.main != "index.twig" {
            fields.push(("main".to_owned(), Json::str(self.main.clone())));
        }
        if !self.data.is_empty() {
            if self.data.iter().any(|(_, v)| v.needs_php()) {
                let mut php = String::from("return [");
                for (i, (name, value)) in self.data.iter().enumerate() {
                    if i > 0 {
                        php.push_str(", ");
                    }
                    php_string(name.as_bytes(), &mut php);
                    php.push_str(" => ");
                    value.php(&mut php);
                }
                php.push_str("];");
                fields.push(("data_php".to_owned(), Json::str(php)));
            } else {
                fields.push((
                    "data_json".to_owned(),
                    Json::Object(
                        self.data
                            .iter()
                            .map(|(name, value)| (name.clone(), value.json()))
                            .collect(),
                    ),
                ));
            }
        }
        if !self.globals.is_empty() {
            fields.push((
                "globals".to_owned(),
                Json::Object(
                    self.globals
                        .iter()
                        .map(|(k, v)| (k.clone(), Json::str(v.clone())))
                        .collect(),
                ),
            ));
        }
        let mut config = Vec::new();
        if let Some(autoescape) = self.autoescape {
            config.push(("autoescape".to_owned(), autoescape));
        }
        if let Some(charset) = self.charset {
            config.push(("charset".to_owned(), Json::str(charset)));
        }
        if self.strict {
            config.push(("strict_variables".to_owned(), Json::Bool(true)));
        }
        if self.debug {
            config.push(("debug".to_owned(), Json::Bool(true)));
        }
        if let Some(use_yield) = self.use_yield {
            config.push(("use_yield".to_owned(), Json::Bool(use_yield)));
        }
        if let Some(optimizations) = self.optimizations {
            config.push(("optimizations".to_owned(), Json::int(optimizations)));
        }
        if !config.is_empty() {
            fields.push(("config".to_owned(), Json::Object(config)));
        }
        let mut extensions = Vec::new();
        if self.uses_dump {
            extensions.push(Json::str("debug"));
        }
        if self.uses_string_loader {
            extensions.push(Json::str("string_loader"));
        }
        if !extensions.is_empty() {
            fields.push(("extensions".to_owned(), Json::Array(extensions)));
        }
        if self.data.iter().any(|(_, v)| v.uses_test_classes()) {
            fields.push(("test_classes".to_owned(), Json::Bool(true)));
        }
        if self.errors_throw {
            fields.push(("errors".to_owned(), Json::str("throw")));
        }
        if let Some(Sandbox {
            global,
            tags,
            filters,
            functions,
        }) = self.sandbox
        {
            let names = |list: Vec<&str>| Json::Array(list.into_iter().map(Json::str).collect());
            fields.push((
                "sandbox".to_owned(),
                Json::Object(vec![
                    ("global".to_owned(), Json::Bool(global)),
                    (
                        "policy".to_owned(),
                        Json::Object(vec![
                            ("tags".to_owned(), names(tags)),
                            ("filters".to_owned(), names(filters)),
                            ("functions".to_owned(), names(functions)),
                            ("methods".to_owned(), Json::Object(Vec::new())),
                            ("properties".to_owned(), Json::Object(Vec::new())),
                        ]),
                    ),
                ]),
            ));
        }
        Json::Object(fields)
    }
}

fn push(out: &mut Vec<u8>, text: &str) {
    out.extend_from_slice(text.as_bytes());
}

/// `{% macro name(params) %}body{% endmacro %}`, optionally inside `{% autoescape %}`.
fn render_macro(def: &MacroDef, out: &mut Vec<u8>) {
    if let Some(arg) = &def.wrap {
        push(out, &format!("{{% autoescape{arg} %}}"));
    }
    let params: Vec<String> = def
        .params
        .iter()
        .map(|(name, default)| match default {
            Some(default) => format!("{name} = {default}"),
            None => name.clone(),
        })
        .collect();
    push(
        out,
        &format!("{{% macro {}({}) %}}", def.name, params.join(", ")),
    );
    out.extend_from_slice(&def.body);
    push(out, "{% endmacro %}");
    if def.wrap.is_some() {
        push(out, "{% endautoescape %}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kit::case::Case;

    #[test]
    fn deterministic_and_parseable() {
        let a = generate(7, 300);
        let b = generate(7, 300);
        assert_eq!(a, b);
        for json in &a {
            let case = Case::from_json(json).expect("a twig case");
            assert!(case.unsupported_reason().is_none(), "{json:?}");
        }
        assert_ne!(generate(8, 5), generate(7, 5));
    }

    #[test]
    fn literals_round_trip_through_twig_escapes() {
        let mut g = Gen::new(Rng(1), "t".to_owned());
        for _ in 0..200 {
            let bytes = g.string();
            let lit = g.literal(&bytes);
            assert!(lit.len() >= 2);
            assert!(!lit[1..lit.len() - 1].contains("#{"), "{lit}");
        }
        let mut php = String::new();
        php_string(b"a\"$\\\xff", &mut php);
        assert_eq!(php, "\"a\\\"\\$\\\\\\xff\"");
    }
}
