//! Generator of the `unicode` family: Twig's string filters and functions on random Unicode.
//!
//! Every case puts one to four random strings in the context and prints a few results of the
//! callables that look at characters rather than bytes, or at bytes where a character would be
//! expected: `upper`, `lower`, `title`, `capitalize` (mbstring's full case mapping: final sigma,
//! `ß`, `İ`, ligatures, titlecase digraphs, Cherokee, Georgian, astral scripts), `length`, `first`,
//! `last`, `slice` and `[a:b]`, `reverse`, `split` (empty and multi-byte delimiters, limits),
//! `trim` with multi-byte masks and `..` ranges, `replace`, `format` with widths, precisions and
//! padding characters, `nl2br`, `striptags`, `spaceless`, `url_encode` (strings and mappings with
//! Unicode keys), `json_encode` with the Unicode and invalid-UTF-8 flags, every escaping strategy
//! with and without a charset argument, `convert_encoding` between UTF-8, UTF-16/32, the
//! single-byte and the CJK charsets (`//TRANSLIT`, `//IGNORE`), comparisons and sorting of
//! strings, `matches` with `/u` patterns, `starts with`/`ends with`/`in`, per-character loops and
//! arrows over `split('')`, interpolation and autoescaped output.
//!
//! Strings mix ASCII (with control characters and HTML specials), Latin-1, the characters with
//! special case mappings, combining marks, zero-width and other format characters, CJK, fullwidth
//! forms, astral planes, any code point of the blocks with case pairs (also the Unicode 16 and 17
//! additions), and (in a quarter of the cases) invalid UTF-8: lone continuation bytes,
//! truncated sequences, overlong forms, surrogates, code points above U+10FFFF and the bytes
//! `F5`-`FF`. Valid strings travel as `data_json`, invalid ones as `data_php`; arguments are
//! single-quoted Twig literals with `\xHH` escapes. Settings vary: the `charset` option (UTF-8,
//! ISO-8859-1, ASCII and other single-byte and multi-byte charsets), the autoescape strategy,
//! `strict_variables` and warnings turned into exceptions.
//!
//! Template literals never hold a CR byte: a CR that an escape puts into compiled PHP moves the
//! lines PHP reports (accepted deviation D2, `reference/deviations.md`); data strings may. No
//! unseeded randomness (`random()`, `shuffle`).
//!
//! [`generate`] is deterministic: case `i` of seed `s` depends only on `(s, i)` ([`case`]). Ids
//! are `uni-<seed>-<i>`. [`batch`] runs every case in a PHP process of its own.

use std::fmt::Write as _;

use super::casekit::{self, Builder, Rng, php_bytes};
use crate::kit::json::Json;

/// The family name.
pub const FAMILY: &str = "unicode";

const SALT: u64 = 0x0055_4E49_434F;

/// `count` cases of `seed`, as a batch (process isolation).
pub fn batch(seed: u64, count: usize) -> Json {
    casekit::batch(generate(seed, count))
}

/// `count` cases of `seed`.
pub fn generate(seed: u64, count: usize) -> Vec<Json> {
    (0..count as u64).map(|i| case(seed, i)).collect()
}

/// Case `index` of `seed` (id `uni-<seed>-<index>`).
pub fn case(seed: u64, index: u64) -> Json {
    let rng = Rng::new(casekit::case_seed(seed, index, SALT));
    Gen::new(rng).build(format!("uni-{seed}-{index}"))
}

/// Groups of characters, each with a weight.
const GROUPS: &[(u32, &[&str])] = &[
    // ASCII letters and digits
    (
        20,
        &[
            "a", "b", "z", "A", "B", "Z", "i", "I", "s", "S", "k", "K", "0", "1", "9", "x", "y",
        ],
    ),
    // ASCII punctuation, HTML specials, whitespace and controls
    (
        12,
        &[
            " ", " ", "  ", "\t", "\n", "\r", "\r\n", "\0", "\x0b", "\x0c", "<", ">", "&", "\"",
            "'", "/", "\\", "%", "_", "-", ".", ":", ",", ";", "=", "?", "#", "+", "*", "~", "`",
            "\x7f", "\x1b",
        ],
    ),
    // Latin-1 supplement
    (
        12,
        &[
            "é", "É", "ß", "ÿ", "µ", "à", "ç", "ñ", "ö", "Ø", "þ", "Þ", "ð", "ª", "º", "·", "¡",
            "¿", "«", "»", "\u{a0}", "\u{ad}", "×", "÷", "Å", "å",
        ],
    ),
    // special case mappings
    (
        14,
        &[
            "İ", "ı", "ŉ", "ǅ", "ǆ", "Ǆ", "ǈ", "ǉ", "ǋ", "ǌ", "ǲ", "ǳ", "Ǳ", "ǰ", "ſ", "ẞ", "Ÿ",
            "ĳ", "Ĳ", "œ", "Œ", "ł", "Ł", "ﬁ", "ﬂ", "ﬀ", "ﬃ", "ﬄ", "ﬅ", "ﬆ", "\u{212a}",
            "\u{2126}", "\u{212b}", "ẖ", "ẗ", "ẘ", "ẙ", "ẚ", "ᾀ", "ᾈ", "ᾳ", "ᾼ", "ῳ", "ῼ", "և",
            "ﬓ", "Ꭰ", "ꭰ", "Ᏸ", "ᏸ", "ა", "Ა", "ⓐ", "Ⓐ", "Ω", "ꞵ", "Ʞ", "ʞ",
        ],
    ),
    // Greek, with the sigmas
    (
        12,
        &[
            "Σ", "Σ", "σ", "ς", "Α", "α", "Ω", "ω", "ΐ", "ΰ", "ά", "Ϊ", "ϐ", "ϑ", "ϕ", "ϖ", "ϰ",
            "ϱ", "ϵ", "Ϳ", "ϳ", "Ͱ", "ͱ",
        ],
    ),
    // Cyrillic, Armenian and others
    (
        6,
        &[
            "Д", "д", "Ё", "ё", "Ѐ", "ѐ", "ӏ", "Ӏ", "Ա", "ա", "Ꙁ", "ꙁ", "Ꚛ", "ꚛ", "Ȿ", "ȿ",
        ],
    ),
    // combining marks
    (
        10,
        &[
            "\u{300}", "\u{301}", "\u{308}", "\u{327}", "\u{345}", "\u{20dd}", "\u{307}",
            "\u{36f}", "\u{1ab0}",
        ],
    ),
    // format characters and other spaces
    (
        6,
        &[
            "\u{200b}",
            "\u{200c}",
            "\u{200d}",
            "\u{feff}",
            "\u{2028}",
            "\u{2029}",
            "\u{85}",
            "\u{3000}",
            "\u{2019}",
            "\u{2bc}",
            "\u{180e}",
            "\u{e0001}",
        ],
    ),
    // CJK, fullwidth and halfwidth forms
    (
        6,
        &[
            "中", "文", "日", "本", "한", "ｶ", "Ａ", "ａ", "Ｚ", "ｚ", "㈱", "㍻", "〇",
        ],
    ),
    // astral planes
    (
        8,
        &[
            "😀",
            "🇱🇹",
            "👍🏽",
            "𐐀",
            "𐐨",
            "𞤀",
            "𞤢",
            "𝔄",
            "𑢠",
            "𑣀",
            "𖹀",
            "𖹠",
            "𐒰",
            "𐓘",
            "𐲀",
            "𐳀",
            "𝟘",
            "\u{10ffff}",
            "\u{1f600}\u{fe0f}",
        ],
    ),
];

/// Invalid UTF-8.
const INVALID: &[&[u8]] = &[
    b"\x80",
    b"\xbf",
    b"\xc0\x80",
    b"\xc1\xbf",
    b"\xc3",
    b"\xe2\x82",
    b"\xed\xa0\x80",
    b"\xed\xbf\xbf",
    b"\xf4\x90\x80\x80",
    b"\xf0\x9f\x98",
    b"\xf8\x88\x80\x80\x80",
    b"\xfe",
    b"\xff",
    b"\xe0\x80\xaf",
    b"\xf0\x80\x80\xaf",
    b"\xc3\x28",
];

/// Charsets for the `charset` option, `escape`'s charset argument and `convert_encoding`.
const CHARSETS: &[&str] = &[
    "UTF-8",
    "utf-8",
    "ISO-8859-1",
    "iso-8859-1",
    "ISO-8859-15",
    "Windows-1252",
    "CP1251",
    "KOI8-R",
    "ASCII",
    "US-ASCII",
    "UTF-16",
    "UTF-16LE",
    "UTF-16BE",
    "UTF-32",
    "UCS-2",
    "SJIS",
    "Shift_JIS",
    "EUC-JP",
    "GBK",
    "GB2312",
    "BIG5",
    "BIG5-HKSCS",
    "ISO-8859-5",
    "MacRoman",
    "CP866",
    "8bit",
    "nope",
];

struct Gen {
    rng: Rng,
    /// The context strings, as bytes.
    strings: Vec<Vec<u8>>,
    /// Whether `m` (a mapping with string keys) is in the context.
    mapping: Option<Vec<(Vec<u8>, i64)>>,
    invalid: bool,
    counter: usize,
    uses_dump: bool,
}

impl Gen {
    fn new(rng: Rng) -> Gen {
        Gen {
            rng,
            strings: Vec::new(),
            mapping: None,
            invalid: false,
            counter: 0,
            uses_dump: false,
        }
    }

    fn build(mut self, id: String) -> Json {
        let mut b = Builder::new(id);
        self.invalid = self.rng.chance(25);
        let n = self.rng.range(1, 4);
        for _ in 0..n {
            let s = self.string();
            self.strings.push(s);
        }
        if self.rng.chance(20) {
            let keys = self.rng.range(1, 4);
            let mut entries = Vec::new();
            for i in 0..keys {
                let key = self.short_string();
                entries.push((key, i as i64 + 1));
            }
            self.mapping = Some(entries);
        }
        let mut template = String::new();
        let statements = self.rng.range(1, 4);
        for _ in 0..statements {
            self.statement(&mut template);
            template.push('\n');
        }
        b.template("index.twig", template);
        self.data(&mut b);
        let charset =
            self.rng
                .weighted(&[(68, "UTF-8"), (12, "ISO-8859-1"), (5, "ASCII"), (15, "")]);
        let charset = if charset.is_empty() {
            *self.rng.pick(CHARSETS)
        } else {
            charset
        };
        if charset != "UTF-8" {
            b.option("charset", Json::str(charset));
        }
        let autoescape = self.rng.weighted(&[
            (45, "false"),
            (30, "html"),
            (8, "js"),
            (5, "css"),
            (4, "url"),
            (4, "html_attr"),
            (4, "html_attr_relaxed"),
        ]);
        b.option(
            "autoescape",
            if autoescape == "false" {
                Json::Bool(false)
            } else {
                Json::str(autoescape)
            },
        );
        b.option("strict_variables", Json::Bool(self.rng.chance(50)));
        if self.uses_dump {
            // dump() prints only in debug mode
            b.option("debug", Json::Bool(true));
            b.extensions.push("debug");
        }
        b.throw = self.rng.chance(15);
        b.build()
    }

    fn data(&mut self, b: &mut Builder) {
        let all_valid = self.strings.iter().all(|s| std::str::from_utf8(s).is_ok())
            && self
                .mapping
                .as_ref()
                .is_none_or(|m| m.iter().all(|(k, _)| std::str::from_utf8(k).is_ok()));
        if all_valid {
            for (i, s) in self.strings.iter().enumerate() {
                let text = String::from_utf8(s.clone()).expect("checked");
                b.data_json.push((format!("s{i}"), Json::str(text)));
            }
            if let Some(m) = &self.mapping {
                let fields = m
                    .iter()
                    .map(|(k, v)| {
                        (
                            String::from_utf8(k.clone()).expect("checked"),
                            Json::int(*v),
                        )
                    })
                    .collect();
                b.data_json.push(("m".to_owned(), Json::Object(fields)));
            }
            b.data_json.push(("n".to_owned(), Json::int(2)));
        } else {
            let mut php = String::from("return [");
            for (i, s) in self.strings.iter().enumerate() {
                let _ = write!(php, "'s{i}' => {}, ", php_bytes(s));
            }
            if let Some(m) = &self.mapping {
                php.push_str("'m' => [");
                for (k, v) in m {
                    let _ = write!(php, "{} => {v}, ", php_bytes(k));
                }
                php.push_str("], ");
            }
            php.push_str("'n' => 2];");
            b.data_php = Some(php);
        }
    }

    // ---- strings --------------------------------------------------------------------------

    fn unit(&mut self, out: &mut Vec<u8>) {
        if self.invalid && self.rng.chance(15) {
            out.extend_from_slice(self.rng.pick_bytes(INVALID));
            return;
        }
        if self.rng.chance(10) {
            // any code point of a range with case mappings, recent additions or odd properties
            let (low, high) = *self.rng.pick(&[
                (0x80u32, 0x24f),
                (0x250, 0x2ff),
                (0x370, 0x52f),
                (0x531, 0x587),
                (0x10a0, 0x10ff),
                (0x13a0, 0x13fd),
                (0x1c80, 0x1cbf),
                (0x1e00, 0x1fff),
                (0x2c00, 0x2c7f),
                (0x2d00, 0x2d2f),
                (0xa640, 0xa69f),
                (0xa720, 0xa7ff),
                (0xab70, 0xabbf),
                (0xff00, 0xff5f),
                (0x10400, 0x104ff),
                (0x10570, 0x105bf),
                (0x10c80, 0x10cff),
                (0x10d50, 0x10d8f),
                (0x118a0, 0x118ff),
                (0x16e40, 0x16e9f),
                (0x1e900, 0x1e95f),
                (0x1f100, 0x1f1ff),
                (0xe0000, 0xe007f),
            ]);
            let cp = low + self.rng.below((high - low + 1) as usize) as u32;
            if let Some(c) = char::from_u32(cp) {
                let mut buf = [0u8; 4];
                out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                return;
            }
        }
        let total: u32 = GROUPS.iter().map(|(w, _)| w).sum();
        let mut roll = self.rng.below(total as usize) as u32;
        for (weight, chars) in GROUPS {
            if roll < *weight {
                out.extend_from_slice(self.rng.pick_str(chars).as_bytes());
                return;
            }
            roll -= weight;
        }
    }

    fn string(&mut self) -> Vec<u8> {
        let mut out = Vec::new();
        let len = self
            .rng
            .weighted(&[(5, 0usize), (30, 3), (35, 6), (20, 10), (10, 16)]);
        let len = if len == 0 { 0 } else { self.rng.range(1, len) };
        // a word-like run now and then, for title() and capitalize()
        let wordy = self.rng.chance(30);
        for i in 0..len {
            if wordy && i % 4 == 3 {
                out.push(b' ');
            } else {
                self.unit(&mut out);
            }
        }
        out
    }

    fn short_string(&mut self) -> Vec<u8> {
        let mut out = Vec::new();
        let len = self.rng.range(1, 3);
        for _ in 0..len {
            self.unit(&mut out);
        }
        out
    }

    /// A short string, often a piece of one of the context strings (so that it occurs in them).
    fn piece(&mut self) -> Vec<u8> {
        if self.rng.chance(60) {
            let s = self.rng.pick(&self.strings).clone();
            if let Ok(text) = std::str::from_utf8(&s)
                && !text.is_empty()
                && self.rng.chance(75)
            {
                // whole characters
                let chars: Vec<char> = text.chars().collect();
                let start = self.rng.below(chars.len());
                let len = self.rng.range(1, 2.min(chars.len() - start));
                return chars[start..start + len]
                    .iter()
                    .collect::<String>()
                    .into_bytes();
            }
            if !s.is_empty() {
                // bytes, possibly cutting a character
                let start = self.rng.below(s.len());
                let len = self.rng.range(1, 3.min(s.len() - start));
                return s[start..start + len].to_vec();
            }
        }
        self.short_string()
    }

    /// A single-quoted Twig literal of `bytes` (`\xHH` for controls and invalid UTF-8; CR bytes are
    /// left out, see the module documentation).
    fn lit(bytes: &[u8]) -> String {
        let mut out = String::from("'");
        let mut rest = bytes;
        while !rest.is_empty() {
            let (valid, invalid) = match std::str::from_utf8(rest) {
                Ok(valid) => (valid, &b""[..]),
                Err(e) => {
                    let (v, after) = rest.split_at(e.valid_up_to());
                    let bad = e.error_len().unwrap_or(after.len()).max(1);
                    (std::str::from_utf8(v).expect("valid prefix"), &after[..bad])
                }
            };
            for c in valid.chars() {
                match c {
                    '\'' => out.push_str("\\'"),
                    '\\' => out.push_str("\\\\"),
                    '\r' => {}
                    c if (c as u32) < 0x20 || c == '\x7f' => {
                        let _ = write!(out, "\\x{:02x}", c as u32);
                    }
                    c => out.push(c),
                }
            }
            for &byte in invalid {
                let _ = write!(out, "\\x{byte:02x}");
            }
            rest = &rest[valid.len() + invalid.len()..];
        }
        out.push('\'');
        out
    }

    /// A context string (`s0`, ...), now and then a literal.
    fn svar(&mut self) -> String {
        if self.rng.chance(12) {
            let s = self.string();
            return Self::lit(&s);
        }
        format!("s{}", self.rng.below(self.strings.len()))
    }

    fn int_arg(&mut self) -> String {
        self.rng
            .weighted(&[
                (8, "0"),
                (8, "1"),
                (8, "2"),
                (6, "3"),
                (8, "-1"),
                (6, "-2"),
                (4, "-3"),
                (5, "5"),
                (4, "10"),
                (4, "-10"),
                (4, "null"),
                (1, "9223372036854775807"),
                (1, "-9223372036854775807"),
                (1, "'2'"),
                (1, "1.0"),
                (1, "true"),
            ])
            .to_owned()
    }

    // ---- statements -----------------------------------------------------------------------

    fn statement(&mut self, out: &mut String) {
        match self.rng.below(20) {
            0 => {
                let s = self.svar();
                let body = self
                    .rng
                    .pick_str(&["<{{ c }}>", "{{ c|upper }}|", "{{ c|length }}"]);
                let delimiter = self.rng.pick_str(&["''", "''", "' '", "''|default('')"]);
                let _ = write!(
                    out,
                    "{{% for c in {s}|split({delimiter}) %}}{body}{{% endfor %}}"
                );
            }
            1 => {
                self.counter += 1;
                let name = format!("x{}", self.counter);
                let expr = self.expr();
                let _ = write!(
                    out,
                    "{{% set {name} = {expr} %}}[{{{{ {name}|length }}}}:{{{{ {name} }}}}]"
                );
            }
            2 => {
                let filter = self.case_filter();
                let s = self.svar();
                let _ = write!(
                    out,
                    "{{% apply {filter} %}}{{{{ {s} }}}}|{{{{ {s}|raw }}}}{{% endapply %}}"
                );
            }
            3 => {
                self.uses_dump = true;
                let s = self.svar();
                let f = self.case_filter();
                let _ = write!(out, "{{{{ dump({s}|{f}, {s}|length) }}}}");
            }
            _ => {
                let expr = self.expr();
                let _ = write!(out, "[{{{{ {expr} }}}}]");
            }
        }
    }

    fn case_filter(&mut self) -> &'static str {
        self.rng
            .pick_str(&["upper", "lower", "title", "capitalize"])
    }

    fn expr(&mut self) -> String {
        let s = self.svar();
        match self.rng.below(27) {
            0..=3 => {
                let f = self.case_filter();
                if self.rng.chance(20) {
                    let g = self.case_filter();
                    format!("{s}|{f}|{g}")
                } else {
                    format!("{s}|{f}")
                }
            }
            4 => {
                let f = self.case_filter();
                let tail =
                    self.rng
                        .pick_str(&["length", "first", "last", "reverse", "json_encode"]);
                format!("{s}|{f}|{tail}")
            }
            5 | 6 => {
                let f = self
                    .rng
                    .pick_str(&["length", "first", "last", "reverse", "length"]);
                format!("{s}|{f}")
            }
            7 | 8 => {
                let (a, l) = (self.int_arg(), self.int_arg());
                match self.rng.below(5) {
                    0 => format!("{s}|slice({a})"),
                    1 => format!("{s}|slice({a}, {l})"),
                    2 => format!("{s}[{a}:{l}]"),
                    3 => format!("{s}[{a}:]"),
                    _ => format!("{s}[:{l}]"),
                }
            }
            9 | 10 => {
                let delimiter = if self.rng.chance(40) {
                    "''".to_owned()
                } else {
                    let p = self.piece();
                    Self::lit(&p)
                };
                let join = self
                    .rng
                    .pick_str(&["join('|')", "json_encode", "length", "join(', ')"]);
                if self.rng.chance(50) {
                    let limit = self.int_arg();
                    format!("{s}|split({delimiter}, {limit})|{join}")
                } else {
                    format!("{s}|split({delimiter})|{join}")
                }
            }
            11 | 12 => {
                let mask = match self.rng.below(6) {
                    0 => None,
                    1 => Some("'a..z'".to_owned()),
                    2 => Some(Self::lit(b"\xc2\xa0 ")),
                    3 => Some(
                        self.rng
                            .pick_str(&["'z..a'", "'..a'", "'a..'", "'a..b..c'", "'\\x00..\\x1f'"])
                            .to_owned(),
                    ),
                    _ => {
                        let p = self.piece();
                        Some(Self::lit(&p))
                    }
                };
                let side = self.rng.pick_str(&["", "'left'", "'right'", "'both'"]);
                match (mask, side) {
                    (None, "") => format!("{s}|trim"),
                    (None, side) => format!("{s}|trim(side={side})"),
                    (Some(mask), "") => format!("{s}|trim({mask})"),
                    (Some(mask), side) => format!("{s}|trim({mask}, {side})"),
                }
            }
            13 => {
                let (from, to) = (self.piece(), self.short_string());
                let (from2, to2) = (self.piece(), self.piece());
                if self.rng.chance(50) {
                    format!("{s}|replace({{{}: {}}})", Self::lit(&from), Self::lit(&to))
                } else {
                    format!(
                        "{s}|replace({{({}): {}, ({}): {}}})",
                        Self::lit(&from),
                        Self::lit(&to),
                        Self::lit(&from2),
                        Self::lit(&to2)
                    )
                }
            }
            14 | 15 => {
                let format = self.rng.pick_str(&[
                    "'%s'",
                    "'[%5s]'",
                    "'[%-6s]'",
                    "'[%05s]'",
                    "'[%.2s]'",
                    "'[%3.1s]'",
                    "'[%\\'*8s]'",
                    "'[%\\'é8s]'",
                    "'%c%c'",
                    "'%u|%x|%o|%b'",
                    "'%2$s %1$s'",
                    "'%10.4f'",
                    "'%e'",
                    "'%s %s'",
                    "'%%|%s'",
                ]);
                let s2 = self.svar();
                if format.contains("%c") || format.contains("%u") {
                    let (a, b2) = (self.rng.below(300), self.rng.below(70_000));
                    format!("{format}|format({a}, {b2}, {a}, {b2})")
                } else if format.contains('f') || format.contains("%e") {
                    format!("{format}|format({s}|length / 3)")
                } else {
                    format!("{format}|format({s}, {s2})")
                }
            }
            16 => {
                let f = self.rng.pick_str(&[
                    "nl2br",
                    "striptags",
                    "striptags('<b><p>')",
                    "nl2br|striptags",
                    "spaceless",
                ]);
                format!("{s}|{f}")
            }
            17 => match self.rng.below(4) {
                0 => format!("{s}|url_encode"),
                1 => {
                    let key = self.piece();
                    format!("{{({}): {s}, k: [{s}, 1]}}|url_encode", Self::lit(&key))
                }
                2 if self.mapping.is_some() => "m|url_encode".to_owned(),
                _ => format!("[{s}, null, true, 1.5]|url_encode"),
            },
            18 | 19 => {
                let flags = self.rng.pick_str(&[
                    "", "256", "320", "1048576", "2097152", "2097408", "15", "512", "4194304",
                    "128", "1048832", "32", "2304", "1024", "16", "2097664", "1049088",
                ]);
                let value = if self.mapping.is_some() && self.rng.chance(30) {
                    "m".to_owned()
                } else if self.rng.chance(20) {
                    format!("{{({s}): {s}}}")
                } else {
                    s
                };
                if flags.is_empty() {
                    format!("{value}|json_encode")
                } else {
                    format!("{value}|json_encode({flags})")
                }
            }
            20 | 21 => {
                let strategy = self.rng.pick_str(&[
                    "html",
                    "js",
                    "css",
                    "url",
                    "html_attr",
                    "html_attr_relaxed",
                ]);
                if self.rng.chance(35) {
                    let charset = self.rng.pick_str(CHARSETS);
                    format!("{s}|e('{strategy}', '{charset}')")
                } else {
                    format!("{s}|escape('{strategy}')")
                }
            }
            22 | 23 => {
                let to = self.rng.pick_str(CHARSETS);
                let to = match self.rng.below(8) {
                    0 => format!("{to}//TRANSLIT"),
                    1 => format!("{to}//IGNORE"),
                    _ => to.to_owned(),
                };
                let from = self.rng.weighted(&[(70, "UTF-8"), (30, "")]);
                let from = if from.is_empty() {
                    self.rng.pick_str(CHARSETS)
                } else {
                    from
                };
                let tail = self
                    .rng
                    .pick_str(&["", "|length", "|url_encode", "|json_encode"]);
                format!("{s}|convert_encoding('{to}', '{from}'){tail}")
            }
            24 => {
                let t = self.svar();
                match self.rng.below(11) {
                    0 => format!("{s} == {t}"),
                    1 => format!("{s} < {t}"),
                    2 => format!("{s} <=> {t}"),
                    3 => format!("{s} starts with {t}"),
                    4 => format!("{t} in {s}"),
                    5 => format!("max({s}, {t})"),
                    6 => format!("[{s}, {t}, 'a', 'Z']|sort|join('|')"),
                    7 => format!("[{s}, {t}]|sort((a, b) => a|lower <=> b|lower)|join('|')"),
                    8 => format!("{s}|lower == {t}|lower"),
                    9 => format!("{s} ends with {t}"),
                    _ => format!("{s}|upper is same as({t}|upper)"),
                }
            }
            25 => {
                let pattern = self.rng.pick_str(&[
                    "'/^\\\\w+$/u'",
                    "'/\\\\p{Lu}/u'",
                    "'/\\\\p{Ll}/u'",
                    "'/./su'",
                    "'/\\\\X{3}/u'",
                    "'/\\\\s/u'",
                    "'/\\\\s/'",
                    "'/é/i'",
                    "'/É/iu'",
                    "'/[\\\\x{1F600}-\\\\x{1F64F}]/u'",
                    "'/^.{0,4}$/u'",
                    "'/^.{0,4}$/'",
                    "'/\\\\bσ/u'",
                ]);
                format!("{s} matches {pattern}")
            }
            _ => {
                let t = self.svar();
                match self.rng.below(8) {
                    0 => format!("\"<#{{{s}}}|#{{{t}|upper}}>\""),
                    1 => format!("{s}|split('')|map(c => c|upper)|join"),
                    2 => format!("{s}|split('')|reverse|join == {s}|reverse"),
                    3 => format!("{s}|split('', 2)|map(p => p|length)|join(',')"),
                    4 => format!("cycle({s}|split(''), n)"),
                    5 => format!("{s}|default('d')|title"),
                    6 => "_charset".to_owned(),
                    _ => format!("{s}|split('')|filter(c => c|upper != c|lower)|join"),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kit::case::Case;

    #[test]
    fn deterministic_and_parseable() {
        let a = generate(5, 300);
        assert_eq!(a, generate(5, 300));
        assert_ne!(generate(6, 5), generate(5, 5));
        for json in &a {
            let case = Case::from_json(json).expect("a twig case");
            assert!(case.unsupported_reason().is_none(), "{json:?}");
            let source = case.template("index.twig").expect("index.twig");
            assert!(!source.contains(&b'\r'), "{json:?}");
        }
        assert_eq!(case(5, 9), a[9]);
    }

    #[test]
    fn literals_escape_what_twig_would_read_otherwise() {
        assert_eq!(Gen::lit(b"a'b\\c"), "'a\\'b\\\\c'");
        assert_eq!(Gen::lit(b"\x00a\xffb\r\n"), "'\\x00a\\xffb\\x0a'");
        assert_eq!(Gen::lit("é".as_bytes()), "'é'");
    }
}
