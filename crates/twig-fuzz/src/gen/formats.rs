//! Random cases for the "dates and number formatting" construct family: the `date` filter and
//! function with random formats (every letter, escapes, trailing backslashes), timezones (names,
//! links, case variants, offsets, abbreviations, `false`, `DateTimeZone` objects, invalid names)
//! and inputs (timestamps including negative and huge ones, digit strings, float strings,
//! absolute, relative, composed, mutated and token-soup date strings, `DateTime`,
//! `DateTimeImmutable` and `DateInterval` objects built by `data_php`), `date_modify`, local times
//! around DST gaps, overlaps and other offset changes, date comparisons, methods of date objects
//! (`modify`, `setDate`, `setTime`, `setISODate`, `setTimestamp`, `add`, `sub`, `setTimezone`,
//! `getTransitions`, `getOffset`, ...), `diff()` and interval formats, `number_format`, `round`,
//! `abs` and `format` (`sprintf`) with random arguments (halfway values included), `json_encode`
//! of random values with random flags and depths, and the string conversion of floats in output
//! (printing, concatenation, interpolation, `join`, `dump()`, arithmetic results, float ranges,
//! floats as array keys). Some cases also change the `CoreExtension` settings from `data_php`
//! (`setTimezone()`, `setDateFormat()`, `setNumberFormat()`), PHP's default timezone (the case's
//! `timezone`) or the `autoescape` strategy.
//!
//! [`generate`] / [`generate_kinds`] are reproducible: case `n` of seed `s` only depends on
//! `(s, n)` ([`case`]), its id is `formats-<s>-<n>`; [`batch`] wraps them as a batch file. Values
//! sit inline in the template or in the data (`data_json`, or `data_php` when the case needs
//! objects or settings). [`grid`] is a deterministic complement: every date format character for
//! special instants (year, leap and ISO week edges of signed and five-digit years, 32/53/64-bit
//! limits, DST changes) in odd zones, and every interval format character for their differences.
//!
//! What cannot match by design is avoided: microseconds of "now" (`u`/`v` date formats and
//! `%f`/`%F` interval formats are left out when the date depends on the current time, and
//! comparisons and `diff()` never take two now-dependent dates), unseeded randomness, `dump()` of
//! objects under escaping strategies that hide the object handles from twig-fuzz's deviation
//! rule, data values twig-fuzz cannot rebuild exactly from the oracle's typed data (fractional
//! seconds of data `DateInterval`s other than multiples of 1/64 s, the dynamic `days` property),
//! data code that throws (invalid zones and modifiers), and sizes that kill the oracle at its hard
//! memory limit (huge `number_format` decimals, `*` widths up to `INT_MAX`). The known
//! operand-order mismatch of commutative PHP opcodes (`+`, `*`, `b-or`, ...) is not provoked:
//! arithmetic only gets operands that raise no diagnostics.

use std::fmt::Write as _;

use crate::kit::json::Json;

/// The family name (the prefix of case ids).
pub const FAMILY: &str = "formats";

/// The groups of template shapes the generator draws from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// `date` filter and function, `date_modify`, date objects, their attributes and comparisons.
    Dates,
    /// `DateInterval` formats (`diff()` results and interval objects).
    Intervals,
    /// `number_format`, `round`, `abs`.
    Numbers,
    /// `format` (PHP's `sprintf`).
    Sprintf,
    /// `json_encode`.
    Json,
    /// Floats converted to strings: printing, concatenation, `join`, `dump()`, arithmetic results.
    Floats,
}

impl Kind {
    /// Every kind.
    pub const ALL: [Kind; 6] = [
        Kind::Dates,
        Kind::Intervals,
        Kind::Numbers,
        Kind::Sprintf,
        Kind::Json,
        Kind::Floats,
    ];

    /// The kind's name (`dates`, `intervals`, `numbers`, `sprintf`, `json`, `floats`).
    pub fn name(self) -> &'static str {
        match self {
            Kind::Dates => "dates",
            Kind::Intervals => "intervals",
            Kind::Numbers => "numbers",
            Kind::Sprintf => "sprintf",
            Kind::Json => "json",
            Kind::Floats => "floats",
        }
    }

    /// The kind called `name`.
    pub fn from_name(name: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.name() == name)
    }

    fn weight(self) -> u32 {
        match self {
            Kind::Dates => 32,
            Kind::Intervals => 8,
            Kind::Numbers => 20,
            Kind::Sprintf => 14,
            Kind::Json => 12,
            Kind::Floats => 14,
        }
    }
}

/// `count` cases of every kind for `seed`.
pub fn generate(seed: u64, count: usize) -> Vec<Json> {
    generate_kinds(seed, count, &Kind::ALL)
}

/// `count` cases for `seed` drawn from `kinds` (every kind when empty).
pub fn generate_kinds(seed: u64, count: usize, kinds: &[Kind]) -> Vec<Json> {
    (0..count as u64).map(|n| case(seed, n, kinds)).collect()
}

/// A batch file (`{"cases": [...]}`) of [`generate_kinds`].
pub fn batch(seed: u64, count: usize, kinds: &[Kind]) -> Json {
    Json::Object(vec![(
        "cases".to_owned(),
        Json::Array(generate_kinds(seed, count, kinds)),
    )])
}

/// Every format character of `DateTimeInterface::format()` | in a format with each one between
/// separators.
const ALL_FORMAT_CHARACTERS: &str =
    "d|D|j|l|N|S|w|z|W|F|m|M|n|t|L|o|X|x|Y|y|a|A|B|g|G|h|H|i|s|u|v|e|I|O|P|p|T|Z|c|r|U";

/// Every `DateInterval::format()` character, between separators.
const ALL_INTERVAL_CHARACTERS: &str = "%Y|%y|%M|%m|%D|%d|%a|%H|%h|%I|%i|%S|%s|%F|%f|%R|%r|%%";

/// Zones of the grid: identifiers with odd offsets and rules, offsets, an abbreviation.
const GRID_ZONES: &[&str] = &[
    "UTC",
    "Europe/Paris",
    "America/New_York",
    "Asia/Kathmandu",
    "Australia/Lord_Howe",
    "Pacific/Chatham",
    "America/St_Johns",
    "Asia/Tehran",
    "Africa/Casablanca",
    "Europe/Dublin",
    "Antarctica/Troll",
    "Pacific/Kiritimati",
    "Etc/GMT+12",
    "America/Sao_Paulo",
    "Europe/Moscow",
    "Asia/Kolkata",
    "Pacific/Apia",
    "+05:30",
    "-09:30",
    "EST",
];

/// Years of the grid: signs, centuries, leap rules, the 32-bit limits, five-digit years.
const GRID_YEARS: &[i64] = &[
    -10000, -1001, -101, -100, -5, -4, -1, 0, 1, 4, 99, 100, 400, 999, 1000, 1582, 1600, 1752,
    1800, 1883, 1899, 1900, 1901, 1945, 1969, 1970, 1971, 1999, 2000, 2004, 2008, 2009, 2010, 2015,
    2016, 2020, 2021, 2024, 2026, 2037, 2038, 2039, 2100, 2400, 3000, 9999, 10000, 99999,
];

/// Days since 1970-01-01 of a proleptic Gregorian date (Howard Hinnant's `days_from_civil`).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The special instants of the grid (Unix timestamps): around the start and end of every grid
/// year, the ISO week edges, leap days, mid-year, plus the 32-bit, 53-bit and 64-bit limits.
fn grid_instants() -> Vec<i64> {
    let mut out = Vec::new();
    for &y in GRID_YEARS {
        for (m, d, seconds) in [
            (1, 1, 0),
            (12, 31, 86_399),
            (2, 28, 43_200),
            (2, 29, 43_200),
            (6, 30, 45_296),
            (12, 28, 1),
            (1, 4, 86_000),
            (1, 3, 3_600),
        ] {
            out.push(days_from_civil(y, m, d) * 86_400 + seconds);
        }
    }
    out.extend([
        i64::MAX,
        i64::MIN,
        2_147_483_647,
        -2_147_483_648,
        9_007_199_254_740_993,
        -9_007_199_254_740_993,
        1_585_443_599,
        1_585_443_600,
        1_603_587_599,
        1_603_587_600,
        1_325_239_199,
        1_325_239_200,
    ]);
    out
}

/// A deterministic grid (no randomness): every date format character for every special instant
/// in every grid zone (`{{ ts|date(all characters, zone) }}`), then every interval format
/// character for differences between special instants in a few zones. Ids are
/// `formats-grid-<n>`.
pub fn grid() -> Vec<Json> {
    let mut cases = Vec::new();
    let mut push = |template: String| {
        let id = format!("{FAMILY}-grid-{}", cases.len());
        cases.push(Json::Object(vec![
            ("id".to_owned(), Json::str(id)),
            ("env".to_owned(), Json::str("plain")),
            (
                "templates".to_owned(),
                Json::Object(vec![("index.twig".to_owned(), Json::str(template))]),
            ),
        ]));
    };
    let instants = grid_instants();
    for &ts in &instants {
        for zone in GRID_ZONES {
            push(format!(
                "{{{{ {}|date('{ALL_FORMAT_CHARACTERS}', '{zone}') }}}}",
                twig_int(ts)
            ));
        }
    }
    // differences: pairs of instants in years that every zone handles
    let pairs: Vec<i64> = instants
        .iter()
        .copied()
        .filter(|ts| (-5_000_000_000..=5_000_000_000).contains(ts))
        .step_by(5)
        .collect();
    for (i, &a) in pairs.iter().enumerate() {
        for &b in pairs.iter().skip(i + 1).step_by(2) {
            let zone = GRID_ZONES[(i + pairs.len()) % 4];
            push(format!(
                "{{{{ date({}, '{zone}').diff(date({}, '{zone}'))|date('{ALL_INTERVAL_CHARACTERS}') }}}}",
                twig_int(a),
                twig_int(b)
            ));
        }
    }
    cases
}

/// Case `n` of `seed`: an oracle "twig" case in the plain environment.
pub fn case(seed: u64, n: u64, kinds: &[Kind]) -> Json {
    let kinds: &[Kind] = if kinds.is_empty() { &Kind::ALL } else { kinds };
    let mut g = Gen::new(Rng::new(mix(seed) ^ mix(n ^ 0x6a09_e667_f3bc_c908)));
    let weights: Vec<u32> = kinds.iter().map(|k| k.weight()).collect();
    let main = kinds[g.rng.weighted(&weights)];
    if g.rng.chance(22) {
        g.timezone = Some((*g.rng.pick(DEFAULT_TIMEZONES)).to_owned());
    }
    g.settings(main);
    // formatted strings through other escaping strategies (numbers are printed as they are)
    if g.rng.chance(14) {
        g.autoescape = Some(match g.rng.below(6) {
            0 | 1 => Json::Bool(false),
            2 => Json::str("js"),
            3 => Json::str("css"),
            4 => Json::str("url"),
            _ => Json::str("html_attr"),
        });
    }
    // mostly one statement: an exception in one statement hides the output of the others
    let statements = 1 + g.rng.weighted(&[70, 22, 8]);
    let mut body = String::new();
    for i in 0..statements {
        if i > 0 {
            body.push_str(if g.rng.chance(70) { "|" } else { "\n" });
        }
        let kind = if g.rng.chance(85) {
            main
        } else {
            kinds[g.rng.weighted(&weights)]
        };
        let statement = g.statement(kind);
        body.push_str(&statement);
    }
    g.finish(&format!("{FAMILY}-{seed}-{n}"), body)
}

// -------------------------------------------------------------------------------------------------
// random numbers

/// SplitMix64: small, seedable and good enough for test generation.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    /// A generator seeded with `seed`.
    pub fn new(seed: u64) -> Rng {
        Rng(seed)
    }

    /// The next 64 random bits.
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// A number in `0..n` (0 when `n` is 0).
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next_u64() % n as u64) as usize
        }
    }

    /// `true` with a probability of `percent` %.
    pub fn chance(&mut self, percent: u64) -> bool {
        self.next_u64() % 100 < percent
    }

    /// A random item.
    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }

    /// An index into `weights`, chosen by weight.
    pub fn weighted(&mut self, weights: &[u32]) -> usize {
        let total: u64 = weights.iter().map(|&w| u64::from(w)).sum();
        let mut x = self.next_u64() % total.max(1);
        for (i, &w) in weights.iter().enumerate() {
            let w = u64::from(w);
            if x < w {
                return i;
            }
            x -= w;
        }
        weights.len().saturating_sub(1)
    }

    /// A number in `lo..=hi`.
    pub fn range(&mut self, lo: i64, hi: i64) -> i64 {
        let span = hi.wrapping_sub(lo) as u64;
        if span == u64::MAX {
            return self.next_u64() as i64;
        }
        lo.wrapping_add((self.next_u64() % (span + 1)) as i64)
    }
}

fn mix(x: u64) -> u64 {
    Rng::new(x).next_u64()
}

// -------------------------------------------------------------------------------------------------
// values

/// A value that can be written as a Twig literal, as PHP code and (mostly) as JSON.
#[derive(Clone, Debug)]
enum V {
    Null,
    Bool(bool),
    Int(i64),
    /// Decimal text that PHP, JSON and Twig's lexer all read as the same float (`1.5`, `1.0e-7`,
    /// `-0.0`, `1e999` for INF, digit strings above `PHP_INT_MAX`).
    Float(String),
    Str(Vec<u8>),
    List(Vec<V>),
    Map(Vec<(Key, V)>),
    /// A value only PHP code can make (objects, NAN), with a Twig expression for it when there is one.
    Php {
        php: String,
        twig: Option<String>,
    },
}

#[derive(Clone, Debug, PartialEq)]
enum Key {
    Int(i64),
    Str(String),
}

impl V {
    fn s(text: &str) -> V {
        V::Str(text.as_bytes().to_vec())
    }

    fn f(text: &str) -> V {
        V::Float(text.to_owned())
    }

    fn php(code: impl Into<String>) -> V {
        V::Php {
            php: code.into(),
            twig: None,
        }
    }

    fn php_twig(code: impl Into<String>, twig: impl Into<String>) -> V {
        V::Php {
            php: code.into(),
            twig: Some(twig.into()),
        }
    }

    /// The value as a Twig expression usable as the operand of a filter.
    fn twig(&self) -> Option<String> {
        Some(match self {
            V::Null => "null".to_owned(),
            V::Bool(b) => b.to_string(),
            V::Int(i) => twig_int(*i),
            V::Float(text) => match text.strip_prefix('-') {
                Some(rest) => format!("(-{rest})"),
                None => text.clone(),
            },
            V::Str(bytes) => twig_str(bytes),
            V::List(items) => {
                let parts: Option<Vec<String>> = items.iter().map(V::twig).collect();
                format!("[{}]", parts?.join(", "))
            }
            V::Map(entries) => {
                let mut parts = Vec::new();
                for (key, value) in entries {
                    let key = match key {
                        Key::Int(i) if *i >= 0 => i.to_string(),
                        Key::Int(i) => format!("({i})"),
                        Key::Str(s) => twig_str(s.as_bytes()),
                    };
                    parts.push(format!("{key}: {}", value.twig()?));
                }
                format!("{{{}}}", parts.join(", "))
            }
            V::Php { twig, .. } => return twig.clone(),
        })
    }

    /// The value as PHP code.
    fn php_code(&self) -> String {
        match self {
            V::Null => "null".to_owned(),
            V::Bool(b) => b.to_string(),
            V::Int(i) if *i == i64::MIN => "PHP_INT_MIN".to_owned(),
            V::Int(i) => i.to_string(),
            V::Float(text) => text.clone(),
            V::Str(bytes) => php_str(bytes),
            V::List(items) => {
                let parts: Vec<String> = items.iter().map(V::php_code).collect();
                format!("[{}]", parts.join(", "))
            }
            V::Map(entries) => {
                let parts: Vec<String> = entries
                    .iter()
                    .map(|(key, value)| {
                        let key = match key {
                            Key::Int(i) => i.to_string(),
                            Key::Str(s) => php_str(s.as_bytes()),
                        };
                        format!("{key} => {}", value.php_code())
                    })
                    .collect();
                format!("[{}]", parts.join(", "))
            }
            V::Php { php, .. } => php.clone(),
        }
    }

    /// The value as JSON (`json_decode($json, true)` gives it back), if it has one.
    fn json(&self) -> Option<Json> {
        Some(match self {
            V::Null => Json::Null,
            V::Bool(b) => Json::Bool(*b),
            V::Int(i) => Json::int(*i),
            V::Float(text) => Json::Number(text.clone()),
            V::Str(bytes) => Json::String(String::from_utf8(bytes.clone()).ok()?),
            V::List(items) => Json::Array(items.iter().map(V::json).collect::<Option<_>>()?),
            V::Map(entries) => {
                if entries.is_empty() {
                    return Some(Json::Object(Vec::new()));
                }
                let mut fields = Vec::new();
                for (key, value) in entries {
                    let key = match key {
                        Key::Int(i) => i.to_string(),
                        Key::Str(s) => s.clone(),
                    };
                    fields.push((key, value.json()?));
                }
                Json::Object(fields)
            }
            V::Php { .. } => return None,
        })
    }
}

fn twig_int(i: i64) -> String {
    if i >= 0 {
        i.to_string()
    } else if i == i64::MIN {
        "(-9223372036854775807 - 1)".to_owned()
    } else {
        format!("(-{})", i.unsigned_abs())
    }
}

/// A single-quoted Twig string literal of `bytes` (the lexer's `stripcslashes()` gives them back).
fn twig_str(bytes: &[u8]) -> String {
    let mut out = String::from("'");
    for chunk in bytes.utf8_chunks() {
        for c in chunk.valid().chars() {
            match c {
                '\'' => out.push_str("\\'"),
                '\\' => out.push_str("\\\\"),
                c if (c as u32) < 0x20 || c == '\u{7f}' => {
                    let _ = write!(out, "\\x{:02x}", c as u32);
                }
                c => out.push(c),
            }
        }
        for b in chunk.invalid() {
            let _ = write!(out, "\\x{b:02x}");
        }
    }
    out.push('\'');
    out
}

/// A double-quoted PHP string literal of `bytes`.
fn php_str(bytes: &[u8]) -> String {
    let mut out = String::from("\"");
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
    out
}

/// A PHP single-line string literal for text known to be plain ASCII without quotes.
fn php_plain(text: &str) -> String {
    php_str(text.as_bytes())
}

// -------------------------------------------------------------------------------------------------
// pools

/// PHP default timezones of cases (`"timezone"`).
const DEFAULT_TIMEZONES: &[&str] = &[
    "UTC",
    "Europe/Paris",
    "America/New_York",
    "Asia/Tokyo",
    "Asia/Kathmandu",
    "Australia/Lord_Howe",
    "Pacific/Chatham",
    "America/St_Johns",
    "Europe/London",
    "Africa/Casablanca",
    "Europe/Dublin",
    "Antarctica/Troll",
    "Pacific/Kiritimati",
    "Etc/GMT+12",
    "America/Sao_Paulo",
    "Asia/Tehran",
    "Europe/Moscow",
    "Pacific/Apia",
];

/// Valid timezone identifiers, links and odd zones.
const TZ_NAMES: &[&str] = &[
    "UTC",
    "GMT",
    "Europe/Paris",
    "Europe/London",
    "Europe/Dublin",
    "Europe/Moscow",
    "Europe/Vilnius",
    "Europe/Lisbon",
    "Europe/Kyiv",
    "Europe/Kiev",
    "America/New_York",
    "America/Los_Angeles",
    "America/St_Johns",
    "America/Sao_Paulo",
    "America/Caracas",
    "America/Havana",
    "America/Santiago",
    "America/Argentina/Buenos_Aires",
    "America/Indiana/Indianapolis",
    "America/Noronha",
    "America/Scoresbysund",
    "Asia/Tokyo",
    "Asia/Kolkata",
    "Asia/Calcutta",
    "Asia/Kathmandu",
    "Asia/Tehran",
    "Asia/Shanghai",
    "Asia/Gaza",
    "Asia/Pyongyang",
    "Asia/Colombo",
    "Australia/Lord_Howe",
    "Australia/Adelaide",
    "Australia/Eucla",
    "Pacific/Chatham",
    "Pacific/Kiritimati",
    "Pacific/Apia",
    "Pacific/Pago_Pago",
    "Pacific/Marquesas",
    "Pacific/Honolulu",
    "Africa/Casablanca",
    "Africa/Cairo",
    "Africa/Monrovia",
    "Africa/Juba",
    "Antarctica/Troll",
    "Antarctica/Casey",
    "Atlantic/Azores",
    "Etc/GMT+12",
    "Etc/GMT-14",
    "Etc/GMT-0",
    "Etc/UTC",
    "US/Eastern",
    "Arctic/Longyearbyen",
    "Zulu",
    "Universal",
    "EST5EDT",
    "CET",
    "WET",
    "EET",
    "MST7MDT",
    "PST8PDT",
    "Navajo",
    "Iceland",
    "Japan",
    "Eire",
    "GB",
    "NZ-CHAT",
    "ROK",
    "W-SU",
    "Factory",
    "Etc/Unknown",
    "europe/paris",
    "EUROPE/PARIS",
    "utc",
    "America/new_york",
    "asia/KOLKATA",
];

/// UTC offsets as timezone names.
const TZ_OFFSETS: &[&str] = &[
    "+00:00",
    "-00:00",
    "+0000",
    "+05:30",
    "-03:30",
    "+05:45",
    "+14:00",
    "-12:00",
    "+12:45",
    "-0930",
    "+1",
    "-1",
    "+01",
    "+1:00",
    "+9",
    "+99:99",
    "+24:00",
    "-23:59",
    "+05:30:15",
    "GMT+2",
    "GMT-0530",
    "UTC+3",
    "+0130",
    "-01:30",
    "+13:45",
    "+15:00",
    "-15:00",
    "+100",
    "+1000",
    "+10000",
    "+5:3",
    "+",
];

/// Timezone abbreviations.
const TZ_ABBRS: &[&str] = &[
    "EST", "EDT", "CET", "CEST", "PST", "PDT", "BST", "IST", "JST", "AEST", "AEDT", "MSK", "NZDT",
    "z", "Z", "utc", "gmt", "est", "cest", "A", "Y", "HST", "AKDT", "WIB", "SAST", "MESZ",
];

/// Strings `new \DateTimeZone()` rejects.
const TZ_INVALID: &[&str] = &[
    "Mars/Phobos",
    "Europe/",
    "/",
    "",
    " ",
    "UTC ",
    " UTC",
    "Europe/Paris ",
    "Europe/Pariss",
    "-",
    "0",
    "abc",
    "Etc/GMT+15",
    "localtime",
    "Europe//Paris",
    "Europe/Paris/Extra",
    "+05:61",
];

/// Offset changes: zone, local date and hour of the change (DST gaps and overlaps, rule changes,
/// skipped days).
const TRANSITIONS: &[(&str, &str, u8)] = &[
    ("Europe/Paris", "2021-03-28", 2),
    ("Europe/Paris", "2021-10-31", 2),
    ("Europe/Paris", "1976-03-28", 1),
    ("Europe/Paris", "1945-09-16", 3),
    ("America/New_York", "2021-03-14", 2),
    ("America/New_York", "2021-11-07", 1),
    ("America/New_York", "1918-03-31", 2),
    ("Australia/Lord_Howe", "2021-10-03", 2),
    ("Australia/Lord_Howe", "2021-04-04", 1),
    ("America/Sao_Paulo", "2018-11-04", 0),
    ("America/Sao_Paulo", "2019-02-16", 23),
    ("Pacific/Apia", "2011-12-29", 23),
    ("Pacific/Apia", "2011-12-31", 0),
    ("Europe/Moscow", "2011-03-27", 2),
    ("Europe/Moscow", "2014-10-26", 1),
    ("Europe/Dublin", "2021-03-28", 1),
    ("Europe/Dublin", "2021-10-31", 1),
    ("Antarctica/Troll", "2021-03-28", 1),
    ("Antarctica/Troll", "2021-10-31", 3),
    ("Africa/Casablanca", "2021-04-11", 3),
    ("Africa/Casablanca", "2021-05-16", 2),
    ("America/Havana", "2021-03-14", 0),
    ("America/Havana", "2021-11-07", 0),
    ("America/Santiago", "2021-04-03", 23),
    ("America/Santiago", "2021-09-05", 0),
    ("Asia/Tehran", "2021-03-22", 0),
    ("Asia/Tehran", "2021-09-21", 23),
    ("America/St_Johns", "2021-03-14", 2),
    ("America/St_Johns", "2021-11-07", 1),
    ("Pacific/Chatham", "2021-09-26", 2),
    ("Pacific/Chatham", "2021-04-04", 3),
    ("Asia/Gaza", "2021-03-27", 0),
    ("Asia/Gaza", "2021-10-29", 1),
    ("Europe/London", "2021-03-28", 1),
    ("Europe/London", "2021-10-31", 1),
    ("America/Los_Angeles", "2021-03-14", 2),
    ("Europe/Vilnius", "2021-03-28", 3),
    ("Asia/Kolkata", "1945-10-15", 0),
    ("Asia/Kathmandu", "1986-01-01", 0),
    ("America/Caracas", "2016-05-01", 2),
    ("Asia/Pyongyang", "2015-08-15", 0),
    ("Asia/Pyongyang", "2018-05-04", 23),
    ("Pacific/Kiritimati", "1994-12-31", 0),
    ("America/Indiana/Indianapolis", "2006-04-02", 2),
    ("Europe/Kyiv", "1990-07-01", 3),
    ("America/Noronha", "1999-10-03", 0),
];

/// Modifiers that cross offset changes.
const DST_MODIFIERS: &[&str] = &[
    "+1 day",
    "-1 day",
    "+24 hours",
    "-24 hours",
    "+1 hour",
    "-1 hour",
    "+30 minutes",
    "+90 minutes",
    "+1 month",
    "-1 month",
    "midnight",
    "noon",
    "tomorrow",
    "yesterday",
    "+1 week",
    "next sunday",
    "+86400 sec",
    "-1 sec",
    "+3600 seconds",
    "today",
    "+1 day midnight",
    "00:00",
    "02:30",
    "03:00",
    "01:59:59",
    "+0 day",
    "first day of next month",
    "last day of this month 23:59",
    "+2 hours",
    "-2 hours",
    "+1 weekday",
    "@1616893200",
    "+1 day +1 hour",
    "-1 day -1 hour",
];

/// Prepared date formats (PHP strings; written into the template as literals).
const DATE_FORMATS: &[&str] = &[
    "Y-m-d",
    "Y-m-d H:i:s",
    "c",
    "r",
    "U",
    "D, d M Y H:i:s O",
    "l jS \\o\\f F Y h:i:s A",
    "N jS z t L o W",
    "g G h H i s e I O P p T Z",
    "d D j l N S w z W F m M n t L o X x Y y a A B g G h H i s e I O P p T Z c r U",
    "B",
    "X-x",
    "y n j",
    "\\Y\\m\\d",
    "Y-m-d\\TH:i:sP",
    "e T P p O Z",
    "jS",
    "o-\\WW-N",
    "L t",
    "z",
    "",
    "\\",
    "\\\\",
    "%Y-%m-%d",
    "Y\\",
    "a A g h",
];

/// Prepared date format constants (Twig expressions).
const DATE_FORMAT_CONSTANTS: &[&str] = &[
    "constant('DATE_ATOM')",
    "constant('DATE_RFC2822')",
    "constant('DATE_COOKIE')",
    "constant('DATE_RSS')",
    "constant('DATE_W3C')",
    "constant('DATE_RFC3339_EXTENDED')",
    "constant('DATE_RFC850')",
    "constant('DATE_RFC7231')",
    "constant('DateTimeInterface::ATOM')",
    "constant('DateTimeInterface::RFC3339_EXTENDED')",
    "constant('DateTime::COOKIE')",
    "constant('DateTimeImmutable::ISO8601')",
    "constant('DateTimeInterface::ISO8601_EXPANDED')",
];

/// Characters of `DateTimeInterface::format()` that print something.
const FORMAT_LETTERS: &[u8] = b"dDjlNSwzWFmMntLoXxYyaABgGhHisuveIOPpTZcrU";

/// Interval format characters after `%`.
const INTERVAL_LETTERS: &[u8] = b"YyMmDdaHhIiSsFfRr%";

const MONTHS: &[&str] = &[
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
    "jan",
    "feb",
    "mar",
    "apr",
    "may",
    "jun",
    "jul",
    "aug",
    "sep",
    "sept",
    "oct",
    "nov",
    "dec",
    "III",
    "XII",
    "iv",
];

const DAYS: &[&str] = &[
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
    "sunday",
    "mon",
    "tue",
    "tues",
    "wed",
    "wednes",
    "thu",
    "thur",
    "thurs",
    "fri",
    "sat",
    "sun",
    "Monday",
    "SUNDAY",
];

const UNITS: &[&str] = &[
    "sec",
    "secs",
    "second",
    "seconds",
    "min",
    "mins",
    "minute",
    "minutes",
    "hour",
    "hours",
    "day",
    "days",
    "week",
    "weeks",
    "fortnight",
    "fortnights",
    "forthnight",
    "month",
    "months",
    "year",
    "years",
    "msec",
    "millisecond",
    "milliseconds",
    "ms",
    "usec",
    "microsecond",
    "microseconds",
    "µs",
    "µsec",
    "weekday",
    "weekdays",
    "monday",
    "sat",
];

const ORDINALS: &[&str] = &[
    "first", "second", "third", "fourth", "fifth", "sixth", "seventh", "eighth", "ninth", "tenth",
    "eleventh", "twelfth", "last", "previous", "this", "next",
];

/// Absolute date strings (the result does not depend on "now").
const ABSOLUTE_DATES: &[&str] = &[
    "2020-01-01",
    "2020-02-29",
    "2021-02-29",
    "2020-02-30",
    "2020-13-01",
    "2020-00-10",
    "2020-01-00",
    "0000-00-00",
    "0000-00-00 00:00:00",
    "0001-01-01",
    "-0001-01-01",
    "9999-12-31 23:59:59",
    "1970-01-01 00:00:00",
    "1969-12-31 23:59:59",
    "2038-01-19 03:14:07",
    "2038-01-19 03:14:08",
    "1901-12-13 20:45:52",
    "1582-10-10",
    "2020-01-01 12:00:00 UTC",
    "2020-01-01T12:00:00Z",
    "2020-01-01T12:00:00+05:30",
    "2020-01-01T12:00:00.5+01:00",
    "2020-06-15 12:34:56.789012",
    "2020-06-15 12:34:56 Europe/Paris",
    "2020-06-15 12:34 EST",
    "2020-06-15 12:34 EDT",
    "2020-03-29 02:30:00 Europe/Paris",
    "2020-10-25 02:30:00 Europe/Paris",
    "2021-03-28 02:30:00",
    "2020-06-15 24:00:00",
    "2020-06-15 23:59:60",
    "15 June 2020",
    "June 15, 2020",
    "Jun 15 2020 10:00am",
    "15-Jun-2020",
    "15.06.2020",
    "06/15/2020",
    "2020/06/15",
    "20200615",
    "20200615T103000",
    "2020-W25",
    "2020-W25-3",
    "2020W253",
    "2020-167",
    "@0",
    "@-1",
    "@1700000000",
    "@1700000000.123456",
    "@-0.5",
    "@9223372036854775807",
    "Mon, 15 Jun 2020 10:00:00 +0000",
    "Monday, 15-Jun-20 10:00:00 UTC",
    "2020-06-15T10:00:00.000000Z",
    "1 January 1900",
    "+10000-01-01",
    "-10000-01-01",
    "+002020-01-01",
    "first day of January 2020",
    "last day of February 2024",
    "last day of feb 2023",
    "first monday of 2020-05",
    "last friday of december 2020",
    "2020-01-01 +1 month",
    "2020-01-31 +1 month",
    "2020-03-31 -1 month",
    "2020-01-01 next friday",
    "2020-01-01 noon",
    "2020-01-01 midnight",
    "2020-01-01 +1 weekday",
    "2020-01-03 +2 weekdays",
    "2020-01-04 -1 weekday",
    "2020-06-15 back of 7pm",
    "2020-06-15 front of 7am",
    "2020-06-15 10am",
    "2020-06-15 10:30pm",
    "2020-06-15 12am",
    "2020-06-15 12pm",
    "2020-06-15 10:00:00 America/Argentina/Buenos_Aires",
    "2020-06-15 10:00 +0530",
    "2020-06-15 10:00 GMT+05:30",
    "2020-06-15 10:00 UTC+5",
    "2010-03-13 08:00:00 (CET)",
    "1999-12-31T23:59:59.999999-12:00",
    "2000-01-01 00:00:00 +14:00",
    "2024-02-29 00:00:00 Pacific/Apia",
    "2011-12-30 00:00:00 Pacific/Apia",
    "2020-06-15 10:00:00 Antarctica/Troll",
];

/// Date strings relative to "now".
const RELATIVE_DATES: &[&str] = &[
    "now",
    "today",
    "tomorrow",
    "yesterday",
    "midnight",
    "noon",
    "+1 day",
    "-1 week",
    "+1 month",
    "1 year ago",
    "next monday",
    "last friday",
    "monday",
    "saturday this week",
    "monday next week",
    "first day of next month",
    "last day of this month",
    "first day of",
    "+1 weekday",
    "10:00",
    "10:00:00",
    "10am",
    "noon tomorrow",
    "tomorrow noon",
    "+90 minutes",
    "-1 sec",
    "+1 fortnight",
    "next year",
    "last year",
    "this month",
    "third tuesday of next month",
    "+2 hours 30 minutes",
    "back of 15",
    "front of 10",
    "",
    " ",
    "NOW",
    "Today",
    "+0 day",
    "1 day",
    "+1 day +1 day",
    "tomorrow +1 hour",
    "yesterday 14:00",
    "midnight +1 sec",
    "first day of this year",
    "last sat of this month",
    "+1 week 2 days 4 hours 2 seconds",
    "next week",
    "-3 weekdays",
    "6 weekdays ago",
];

/// Strings that are not dates (or barely).
const GARBAGE_DATES: &[&str] = &[
    // the format families of timelib's scanner, some of them broken
    "2008-07-01T22:35:17.02",
    "2008-07-01T22:35:17.03+08:00",
    "20080701T22:35:17",
    "20080701t223517",
    "2008.197",
    "2008197",
    "May-09-78",
    "78-Dec-22",
    "Dec 22 1978",
    "@1215282385 +1 day",
    "11 Oct",
    "11 Oct 2008 13:00 CET",
    "Sat, 25 Dec 2010 10:00:00 +0100 (CET)",
    "mon, 12 jan 2009",
    "tomorrow 11:00 next sat",
    "-1 week 3 days",
    "6 month 5 day 4 hour ago",
    "1 second ago 2 hours ago",
    "2004W101T05:00+0",
    "2004-W10-1",
    "2004-031",
    "2004031",
    "+1 month 2004-02-29",
    "first day of next year",
    "12/22/78",
    "1/17/2006",
    "1/17/06",
    "10/Oct/2000:13:55:36 -0700",
    "2008-08-07 18:11:31.0045",
    "Thu, 21 Dec 2000 16:01:07 +0200",
    "21:12:45 +0100",
    "8 Jan",
    "8/Jan",
    "Feb 29 2001",
    "2008-4-07",
    "08-04-07",
    "4-7-2008",
    "Sat Mar 10 17:16:18 MST 2001",
    "2001-03-10 17:16:18 America/Phoenix",
    "1978-12-22 +5 weeks",
    "eighth day of march 2020",
    "last wed of next month",
    "first sat of July 2008",
    "back of 7pm next week",
    "Mon Jul 21 20:42:04 2008",
    "2008-07-21 20:42:04 CEST+1",
    "20:42:04.123456 2008-07-21",
    "2008-07-21 20:42:04 -0000",
    "T22:35",
    "t22:35:17",
    "22.35.17",
    "22h35",
    "10.30pm",
    "0800",
    "+0800",
    "2008-W53-7",
    "1999-366",
    "2000-366",
    "1 year 2 months ago noon",
    "noon +2 weekdays",
    "third friday of january",
    "-3 fortnights",
    "+1 year -1 day midnight",
    "abc",
    "x",
    "2020-01-01 abc",
    "1e3",
    "1.5",
    "0.5",
    "+5",
    "-5 abc",
    "--1",
    "1-1-1-1",
    "2020-01-01 10:00:00 Mars/Phobos",
    "2020-01-01T",
    "T10:00",
    "10:",
    "25:00",
    "2020-02-30 25:61:61",
    "Feb 30th",
    "31st",
    "next",
    "last",
    "ago",
    "+",
    "-",
    "@",
    "@abc",
    "@1e3",
    "2020-01-01 2020-01-02",
    "10:00 11:00",
    "UTC UTC",
    "nownow",
    "tomorrowtomorrow",
    "0x1A",
    "1,5",
    "Ⅻ",
    "２０２０-01-01",
    "2020-01-01\u{0}",
    "\t2020-01-01\n",
    "@@1",
    "+1 day -",
    "12/25/2020/1",
    "+1 dayz",
    "pm",
    "first",
    "of",
];

/// Floats (decimal text, see [`V::Float`]).
const FLOATS: &[&str] = &[
    "0.0",
    "-0.0",
    "0.5",
    "1.5",
    "2.5",
    "-0.5",
    "-1.5",
    "-2.5",
    "0.1",
    "0.2",
    "0.3",
    "0.7",
    "1.005",
    "2.675",
    "1.955",
    "5.045",
    "5.055",
    "0.285",
    "1.45",
    "1.15",
    "8.325",
    "3.14159",
    "2.718281828459045",
    "1.0e15",
    "1.0E+25",
    "1.0e-7",
    "1.7976931348623157e308",
    "4.9e-324",
    "2.2250738585072014e-308",
    "1.0e22",
    "1.0e21",
    "1.0e20",
    "9.9999999999999e14",
    "123456789.12345678",
    "0.30000000000000004",
    "1.0000000000000002",
    "100.0",
    "1.0e100",
    "1.0e-100",
    "33.333333333333336",
    "1.0e16",
    "1234567.891",
    "9800.333",
    "-1234.567",
    "0.045",
    "1.0e-5",
    "0.0001",
    "0.00001",
    "999999999999999.9",
    "99999999999999.99",
    "1.25",
    "-1.25",
    "1.0e300",
    "6.02214076e23",
    "1.6e-19",
    "1.2345678901234567e19",
    "9.2233720368547758e18",
    "1.8446744073709552e19",
    "-9.2233720368547758e18",
    "9223372036854775808",
    "-9223372036854775809",
    "18446744073709551616",
    "1700000000.5",
    "0.49999999999999994",
    "5.0e-324",
    "1e999",
    "-1e999",
    "1.0e14",
    "123456.5",
    "-0.0001",
    "4503599627370496.5",
    "4503599627370497.0",
    "9007199254740993.0",
];

/// Integers.
const INTS: &[i64] = &[
    0,
    1,
    -1,
    2,
    7,
    10,
    59,
    60,
    99,
    100,
    255,
    1000,
    1024,
    3600,
    86400,
    999_999,
    1_000_000,
    1_234_567,
    2_147_483_647,
    2_147_483_648,
    -2_147_483_648,
    4_294_967_296,
    1_700_000_000,
    1_000_000_000_000_000,
    9_007_199_254_740_992,
    9_007_199_254_740_993,
    i64::MAX,
    -i64::MAX,
    i64::MIN,
    -999,
    -86400,
    123_456_789_012_345_678,
];

/// Unix timestamps.
const TIMESTAMPS: &[i64] = &[
    0,
    1,
    -1,
    59,
    60,
    86399,
    86400,
    -86400,
    951_782_400,
    1_000_000_000,
    1_234_567_890,
    1_700_000_000,
    1_585_443_599,
    1_585_443_600,
    1_603_587_599,
    1_603_587_600,
    2_147_483_647,
    2_147_483_648,
    -2_147_483_648,
    -2_147_483_649,
    4_102_444_800,
    253_402_300_799,
    253_402_300_800,
    -62_135_596_800,
    -62_135_596_801,
    -62_167_219_200,
    -62_198_755_200,
    100_000_000_000,
    -100_000_000_000,
    1_000_000_000_000,
    1_000_000_000_000_000,
    9_007_199_254_740_993,
    67_767_976_233_316_800,
    67_767_976_233_532_799,
    i64::MAX,
    -i64::MAX,
    i64::MIN,
    1_325_239_200,
    1_301_752_800,
];

/// Strings that look numeric (or nearly).
const NUMERIC_STRINGS: &[&str] = &[
    "0",
    "1",
    "-1",
    "1.5",
    " 1.5",
    "1.5 ",
    "1e3",
    "1E3",
    ".5",
    "5.",
    "-0",
    "+1",
    "0x1A",
    "0b11",
    "012",
    "1_000",
    "1,234.5",
    "1.234,5",
    "abc",
    "",
    " ",
    "12abc",
    "1e999",
    "-1e999",
    "nan",
    "INF",
    "9223372036854775808",
    "-9223372036854775809",
    "1e-400",
    "  42  ",
    "\t5",
    "5\n",
    "١٢٣",
    "１２３",
    "2.675",
    "1.005",
    "-0.0",
    "0.30000000000000004",
    "1e15",
    "123456789012345678901234567890",
];

/// Decimal points and thousands separators.
const SEPARATORS: &[&str] = &[
    ".", ",", "", " ", "'", "\u{a0}", "abc", "é", "٫", "\u{202f}", "--", "0", "1", ".,", "\\", "%",
    "日本",
];

/// Characters of `sprintf` conversions.
const SPRINTF_CONVERSIONS: &[u8] = b"bcdeEfFgGhHosuxX";

/// `json_encode()` flags.
const JSON_FLAGS: &[(&str, i64)] = &[
    ("JSON_HEX_TAG", 1),
    ("JSON_HEX_AMP", 2),
    ("JSON_HEX_APOS", 4),
    ("JSON_HEX_QUOT", 8),
    ("JSON_FORCE_OBJECT", 16),
    ("JSON_NUMERIC_CHECK", 32),
    ("JSON_UNESCAPED_SLASHES", 64),
    ("JSON_PRETTY_PRINT", 128),
    ("JSON_UNESCAPED_UNICODE", 256),
    ("JSON_PARTIAL_OUTPUT_ON_ERROR", 512),
    ("JSON_PRESERVE_ZERO_FRACTION", 1024),
    ("JSON_UNESCAPED_LINE_TERMINATORS", 2048),
    ("JSON_INVALID_UTF8_IGNORE", 1_048_576),
    ("JSON_INVALID_UTF8_SUBSTITUTE", 2_097_152),
    ("JSON_THROW_ON_ERROR", 4_194_304),
];

/// Strings for `json_encode()`.
const JSON_STRINGS: &[&str] = &[
    "",
    "abc",
    "é",
    "😀",
    "a/b",
    "<a href='x'>&amp;</a>",
    "\"quoted\"",
    "back\\slash",
    "line\nbreak\ttab\r",
    "\u{0}\u{1}\u{1f}\u{7f}",
    "\u{2028}\u{2029}",
    "\u{8}\u{c}",
    "日本語",
    "1.0",
    "1e3",
    "0x1A",
    " 12",
    "12 ",
    "1e999",
    "-0",
    "007",
    "+1",
    "\u{fffd}",
    "\u{feff}",
    "null",
];

// -------------------------------------------------------------------------------------------------
// the generator

/// A date expression and whether its value depends on the current time.
struct DateIn {
    expr: String,
    now: bool,
}

struct Gen {
    rng: Rng,
    vars: Vec<(String, V)>,
    /// PHP statements run before the data is returned (`data_php`).
    setup: Vec<String>,
    /// PHP's default timezone (`"timezone"`).
    timezone: Option<String>,
    debug: bool,
    /// The `autoescape` option (Twig's default `html` when `None`).
    autoescape: Option<Json>,
}

impl Gen {
    fn new(rng: Rng) -> Gen {
        Gen {
            rng,
            vars: Vec::new(),
            setup: Vec::new(),
            timezone: None,
            debug: false,
            autoescape: None,
        }
    }

    /// The case object.
    fn finish(self, id: &str, body: String) -> Json {
        let mut fields = vec![
            ("id".to_owned(), Json::str(id)),
            ("env".to_owned(), Json::str("plain")),
            (
                "templates".to_owned(),
                Json::Object(vec![("index.twig".to_owned(), Json::str(body))]),
            ),
        ];
        let mut config = Vec::new();
        if self.debug {
            config.push(("debug".to_owned(), Json::Bool(true)));
        }
        // `dump()` output escaped by another strategy hides the `#N` object handles from
        // twig-fuzz's object-handle deviation rule, so dumps keep the html strategy
        let autoescape = self
            .autoescape
            .as_ref()
            .filter(|a| !self.debug || matches!(a, Json::Bool(false)));
        if let Some(autoescape) = autoescape {
            config.push(("autoescape".to_owned(), autoescape.clone()));
        }
        if !config.is_empty() {
            fields.push(("config".to_owned(), Json::Object(config)));
        }
        if self.debug {
            fields.push((
                "extensions".to_owned(),
                Json::Array(vec![Json::str("debug")]),
            ));
        }
        if let Some(timezone) = &self.timezone {
            fields.push(("timezone".to_owned(), Json::str(timezone)));
        }
        let json: Option<Vec<(String, Json)>> = if self.setup.is_empty() {
            self.vars
                .iter()
                .map(|(name, value)| value.json().map(|j| (name.clone(), j)))
                .collect()
        } else {
            None
        };
        match json {
            Some(entries) if entries.is_empty() => {}
            Some(entries) => fields.push(("data_json".to_owned(), Json::Object(entries))),
            None => {
                let mut code = String::new();
                for statement in &self.setup {
                    code.push_str(statement);
                    code.push_str("; ");
                }
                let entries: Vec<String> = self
                    .vars
                    .iter()
                    .map(|(name, value)| format!("'{name}' => {}", value.php_code()))
                    .collect();
                let _ = write!(code, "return [{}]", entries.join(", "));
                fields.push(("data_php".to_owned(), Json::str(code)));
            }
        }
        Json::Object(fields)
    }

    /// The value inline (as a literal) or as a data variable.
    fn put(&mut self, value: V) -> String {
        match value.twig() {
            Some(literal) if !self.rng.chance(35) => literal,
            _ => self.var(value),
        }
    }

    /// The value as a data variable.
    fn var(&mut self, value: V) -> String {
        let name = format!("v{}", self.vars.len());
        self.vars.push((name.clone(), value));
        name
    }

    /// A random item of a list of static strings (a fixed signature: Rust 1.88 would infer
    /// `pick::<str>` from a `&str` parameter).
    fn pick_static(&mut self, items: &[&'static str]) -> &'static str {
        items[self.rng.below(items.len())]
    }

    fn pick_str(&mut self, items: &[&str]) -> String {
        (*self.rng.pick(items)).to_owned()
    }

    /// `CoreExtension` settings changed by the data code.
    fn settings(&mut self, kind: Kind) {
        const CORE: &str = "$twig->getExtension(\\Twig\\Extension\\CoreExtension::class)";
        if matches!(kind, Kind::Dates | Kind::Intervals) && self.rng.chance(8) {
            let timezone = if self.rng.chance(70) {
                php_plain(&self.pick_str(&TZ_NAMES[..60]))
            } else if self.rng.chance(50) {
                php_plain(&self.pick_str(&TZ_OFFSETS[..12]))
            } else {
                format!(
                    "new \\DateTimeZone({})",
                    php_plain(&self.pick_str(&TZ_NAMES[..40]))
                )
            };
            self.setup.push(format!("{CORE}->setTimezone({timezone})"));
        }
        if matches!(kind, Kind::Dates | Kind::Intervals) && self.rng.chance(6) {
            let format = php_str(&self.date_format_bytes(false));
            let interval = if self.rng.chance(60) {
                php_str(&self.interval_format_bytes(true))
            } else {
                "null".to_owned()
            };
            self.setup
                .push(format!("{CORE}->setDateFormat({format}, {interval})"));
        }
        if kind == Kind::Numbers && self.rng.chance(10) {
            let decimals = self.rng.range(-3, 8);
            let point = php_str(self.rng.pick(SEPARATORS).as_bytes());
            let thousands = php_str(self.rng.pick(SEPARATORS).as_bytes());
            self.setup.push(format!(
                "{CORE}->setNumberFormat({decimals}, {point}, {thousands})"
            ));
        }
    }

    fn statement(&mut self, kind: Kind) -> String {
        match kind {
            Kind::Dates => self.dates_statement(),
            Kind::Intervals => self.intervals_statement(),
            Kind::Numbers => self.numbers_statement(),
            Kind::Sprintf => self.sprintf_statement(),
            Kind::Json => self.json_statement(),
            Kind::Floats => self.floats_statement(),
        }
    }

    // ---- dates ----------------------------------------------------------------------------------

    fn dates_statement(&mut self) -> String {
        if self.rng.chance(14) {
            return self.transition_statement();
        }
        if self.rng.chance(14) {
            return self.methods_statement();
        }
        match self.rng.weighted(&[30, 14, 6, 12, 10, 7, 6, 4, 4, 4, 3]) {
            // D|date(F[, TZ])
            0 => {
                let date = self.date_input();
                let format = self.date_format(!date.now);
                if self.rng.chance(45) {
                    let timezone = self.timezone_arg();
                    if self.rng.chance(10) {
                        format!(
                            "{{{{ {}|date(timezone={timezone}, format={format}) }}}}",
                            date.expr
                        )
                    } else {
                        format!("{{{{ {}|date({format}, {timezone}) }}}}", date.expr)
                    }
                } else {
                    format!("{{{{ {}|date({format}) }}}}", date.expr)
                }
            }
            // date(D[, TZ])|date(F[, TZ|false])
            1 => {
                let date = self.date_input();
                let format = self.date_format(!date.now);
                let inner = if self.rng.chance(50) {
                    format!("date({}, {})", date.expr, self.timezone_arg())
                } else {
                    format!("date({})", date.expr)
                };
                let outer = match self.rng.below(3) {
                    0 => format!("date({format})"),
                    1 => format!("date({format}, false)"),
                    _ => format!("date({format}, {})", self.timezone_arg()),
                };
                format!("{{{{ {inner}|{outer} }}}}")
            }
            // the default format
            2 => {
                let date = self.date_input();
                if self.rng.chance(50) {
                    format!("{{{{ {}|date }}}}", date.expr)
                } else {
                    format!(
                        "{{{{ {}|date(null, {}) }}}}",
                        date.expr,
                        self.timezone_arg()
                    )
                }
            }
            // D|date_modify(M)[|date_modify(M)]|date(F[, TZ])
            3 => {
                let date = self.date_input();
                let mut expr = date.expr.clone();
                for _ in 0..1 + self.rng.weighted(&[80, 20]) {
                    let modifier = self.modifier();
                    let _ = write!(expr, "|date_modify({modifier})");
                }
                let format = self.date_format(!date.now);
                if self.rng.chance(30) {
                    format!("{{{{ {expr}|date({format}, {}) }}}}", self.timezone_arg())
                } else {
                    format!("{{{{ {expr}|date({format}) }}}}")
                }
            }
            // attributes and methods of date objects
            4 => {
                let date = self.date_input();
                let object = if self.rng.chance(50) {
                    format!("date({}, {})", date.expr, self.timezone_arg())
                } else {
                    format!("date({})", date.expr)
                };
                let member = match self.rng.weighted(&[20, 15, 10, 10, 10, 8, 8, 6, 6, 4, 3]) {
                    0 => format!("format({})", self.date_format(!date.now)),
                    1 => "timestamp".to_owned(),
                    2 => "offset".to_owned(),
                    3 => "timezone.name".to_owned(),
                    4 if !date.now => "date".to_owned(),
                    5 => "timezone_type".to_owned(),
                    6 => "timezone".to_owned(),
                    7 => "getTimestamp()".to_owned(),
                    8 if !date.now => "microsecond".to_owned(),
                    9 => "timezone.location.country_code".to_owned(),
                    _ => "getOffset()".to_owned(),
                };
                format!("{{{{ {object}.{member} }}}}")
            }
            // comparisons of dates
            5 => {
                let a = self.date_input();
                let b = self.fixed_date_input(a.now);
                let left = self.date_object(&a.expr);
                let right = self.date_object(&b.expr);
                match self.rng.below(4) {
                    0 => format!("{{{{ {left} <=> {right} }}}}"),
                    1 => format!(
                        "{{{{ {left} < {right} ? 'lt' : ({left} == {right} ? 'eq' : 'gt') }}}}"
                    ),
                    2 => format!("{{{{ {left} >= {right} ? 'ge' : 'lt' }}}}"),
                    _ => {
                        let format = self.date_format(!(a.now || b.now));
                        format!("{{{{ max({left}, {right})|date({format}) }}}}")
                    }
                }
            }
            // a date in a variable: modifying a copy leaves it alone
            6 => {
                let date = self.date_input();
                let modifier = self.modifier();
                let format = self.date_format(!date.now);
                format!(
                    "{{% set d = date({}) %}}{{{{ d|date({format}) }}}}/{{{{ d|date_modify({modifier})|date({format}) }}}}/{{{{ d|date({format}) }}}}",
                    date.expr
                )
            }
            // timestamps of parsed values
            7 => {
                let date = self.date_input();
                format!("{{{{ {}|date('U') }}}}", date.expr)
            }
            // json and dumps of date objects
            8 => {
                let date = self.date_fixed_object();
                if self.rng.chance(60) {
                    format!("{{{{ {date}|json_encode|raw }}}}")
                } else {
                    self.debug = true;
                    format!("{{{{ dump({date}) }}}}")
                }
            }
            // a date printed directly (an error unless it is a string)
            9 => {
                let date = self.date_input();
                format!("{{{{ date({}) ~ '' }}}}", date.expr)
            }
            // dates in other filters
            _ => {
                let date = self.date_input();
                let format = self.date_format(!date.now);
                match self.rng.below(3) {
                    0 => format!("{{{{ {}|date({format})|length }}}}", date.expr),
                    1 => format!("{{{{ {}|date({format})|upper }}}}", date.expr),
                    _ => format!(
                        "{{{{ [{}]|map(d => d|date({format}))|join(',') }}}}",
                        date.expr
                    ),
                }
            }
        }
    }

    /// A local time near an offset change of a zone (DST gaps and overlaps, zone rule changes,
    /// skipped days): the zone and the `Y-m-d H:i:s` text.
    fn transition_time(&mut self) -> (String, String) {
        let (zone, date, hour) = *self.rng.pick(TRANSITIONS);
        let (y, rest) = date.split_at(4);
        let m: i64 = rest[1..3].parse().unwrap_or(1);
        let mut d: i64 = rest[4..6].parse().unwrap_or(1);
        let mut h = i64::from(hour);
        match self.rng.weighted(&[70, 15, 15]) {
            0 => h = (h + self.rng.range(-3, 3)).clamp(0, 23),
            1 => d = (d - 1).max(1),
            _ => d = (d + 1).min(31),
        }
        let i = *self.rng.pick(&[0, 0, 15, 30, 30, 45, 59, 1]);
        let s = if self.rng.chance(20) {
            self.rng.range(0, 59)
        } else {
            0
        };
        (
            zone.to_owned(),
            format!("{y}-{m:02}-{d:02} {h:02}:{i:02}:{s:02}"),
        )
    }

    /// Dates near offset changes: parsing, conversion, modification, timestamps.
    fn transition_statement(&mut self) -> String {
        let (zone, local) = self.transition_time();
        let format = self.pick_str(&[
            "Y-m-d H:i:s T P I U",
            "c e I",
            "Y-m-d H:i T Z I",
            "U",
            "H:i:s O e",
            "G:i A T I",
            "D, d M Y H:i:s O",
        ]);
        let format = self.put(V::s(&format));
        let zoned = self.put(V::Str(format!("{local} {zone}").into_bytes()));
        match self.rng.weighted(&[20, 15, 30, 10, 10, 15]) {
            0 => format!("{{{{ {zoned}|date({format}, false) }}}}"),
            // parsed in PHP's default timezone
            1 => {
                self.timezone = Some(zone.clone());
                let plain = self.put(V::Str(local.into_bytes()));
                if self.rng.chance(50) {
                    format!("{{{{ {plain}|date({format}) }}}}")
                } else {
                    format!("{{{{ {plain}|date_modify('+0 sec')|date({format}, false) }}}}")
                }
            }
            2 => {
                let modifier = self.pick_str(DST_MODIFIERS);
                let modifier = self.put(V::s(&modifier));
                if self.rng.chance(30) {
                    let other = self.pick_str(DST_MODIFIERS);
                    let other = self.put(V::s(&other));
                    format!(
                        "{{{{ {zoned}|date_modify({modifier})|date_modify({other})|date({format}, false) }}}}"
                    )
                } else {
                    format!("{{{{ {zoned}|date_modify({modifier})|date({format}, false) }}}}")
                }
            }
            // converted to another zone
            3 => {
                let other = self.pick_str(&TZ_NAMES[..60]);
                let other = self.put(V::s(&other));
                format!("{{{{ {zoned}|date({format}, {other}) }}}}")
            }
            // a timestamp around the change, shown in the zone
            4 => {
                let shift = *self
                    .rng
                    .pick(&[-3600, -1800, -1, 0, 1, 1800, 3600, 7200, 86400]);
                let zone = self.put(V::s(&zone));
                format!("{{{{ ({zoned}|date('U') + {shift})|date({format}, {zone}) }}}}")
            }
            // a date object of the data
            _ => {
                let class = *self.rng.pick(&["\\DateTime", "\\DateTimeImmutable"]);
                let object = self.var(V::php(format!(
                    "new {class}({}, new \\DateTimeZone({}))",
                    php_plain(&local),
                    php_plain(&zone)
                )));
                let modifier = self.pick_str(DST_MODIFIERS);
                let modifier = self.put(V::s(&modifier));
                format!(
                    "{{{{ {object}|date({format}, false) }}}}/{{{{ {object}|date_modify({modifier})|date({format}, false) }}}}"
                )
            }
        }
    }

    /// A date object that does not depend on "now": one of the data or `date()` of a fixed input.
    fn method_target(&mut self) -> String {
        if self.rng.chance(50) {
            let object = self.datetime_object();
            self.var(object)
        } else {
            let date = self.fixed_date_input(true);
            self.date_object(&date.expr)
        }
    }

    /// A `DateTimeZone` object: of the data, or the zone of a date made by the template.
    fn timezone_target(&mut self) -> String {
        if self.rng.chance(50) {
            let object = self.timezone_object();
            self.var(V::php(object))
        } else {
            // `getTimezone()`: the `timezone` attribute is the zone's name (Twig reads the
            // array-cast property)
            let date = self.fixed_date_input(true);
            format!("date({}, {}).getTimezone()", date.expr, self.timezone_arg())
        }
    }

    /// Methods of date objects called by the template: setters, arithmetic, zone queries.
    fn methods_statement(&mut self) -> String {
        let object = self.method_target();
        let format = self.pick_str(&[
            "Y-m-d H:i:s.u T P",
            "c",
            "Y-m-d H:i:s e I U",
            "D, d M Y H:i:s O",
            "o-\\WW-N H:i",
        ]);
        let format = self.put(V::s(&format));
        let int = |g: &mut Gen, items: &[i64]| -> String { twig_int(*g.rng.pick(items)) };
        match self.rng.below(13) {
            0 => {
                let modifier = self.modifier();
                format!(
                    "{{% set d = {object} %}}{{% do d.modify({modifier}) %}}{{{{ d|date({format}, false) }}}}"
                )
            }
            1 => {
                let y = int(self, &[2020, -1, 0, 10000, 1970, 2024, 1582]);
                let m = int(self, &[-1, 0, 1, 2, 12, 13, 25]);
                let d = int(self, &[-1, 0, 1, 28, 29, 31, 32, 60]);
                format!("{{{{ {object}.setDate({y}, {m}, {d})|date({format}, false) }}}}")
            }
            2 => {
                let h = int(self, &[-1, 0, 23, 24, 25, 48]);
                let i = int(self, &[-1, 0, 59, 60, 90]);
                let s = int(self, &[-1, 0, 59, 60]);
                let micro = int(self, &[-1, 0, 999_999, 1_000_000, 2_500_000]);
                match self.rng.below(3) {
                    0 => format!("{{{{ {object}.setTime({h}, {i})|date({format}, false) }}}}"),
                    1 => format!("{{{{ {object}.setTime({h}, {i}, {s})|date({format}, false) }}}}"),
                    _ => format!(
                        "{{{{ {object}.setTime({h}, {i}, {s}, {micro})|date({format}, false) }}}}"
                    ),
                }
            }
            3 => {
                let y = int(self, &[2020, 2021, 2015, 1, 0, -5, 9999]);
                let w = int(self, &[0, 1, 52, 53, 54, -1]);
                let dow = int(self, &[0, 1, 7, 8, -1]);
                if self.rng.chance(50) {
                    format!("{{{{ {object}.setISODate({y}, {w})|date({format}, false) }}}}")
                } else {
                    format!("{{{{ {object}.setISODate({y}, {w}, {dow})|date({format}, false) }}}}")
                }
            }
            4 => {
                let ts = self.timestamp();
                let ts = self.put(V::Int(ts));
                format!("{{{{ {object}.setTimestamp({ts})|date({format}, false) }}}}")
            }
            5 | 6 => {
                let method = if self.rng.chance(50) { "add" } else { "sub" };
                let interval = if self.rng.chance(60) {
                    let interval = self.interval_object();
                    self.var(interval)
                } else {
                    let a = self.fixed_date_input(true);
                    let b = self.fixed_date_input(true);
                    let left = self.date_object(&a.expr);
                    let right = self.date_object(&b.expr);
                    format!("{left}.diff({right})")
                };
                format!("{{{{ {object}.{method}({interval})|date({format}, false) }}}}")
            }
            7 => {
                let begin = self.rng.range(-3_000_000_000, 3_000_000_000);
                let end = begin + self.rng.range(-1000, 320_000_000);
                format!(
                    "{{{{ {object}.getTimezone().getTransitions({}, {})|json_encode|raw }}}}",
                    twig_int(begin),
                    twig_int(end)
                )
            }
            8 => {
                let zone = self.timezone_target();
                match self.rng.below(4) {
                    0 => format!("{{{{ {zone}.location|json_encode|raw }}}}"),
                    1 => format!("{{{{ {zone}.getTransitions()|length }}}}"),
                    2 => format!("{{{{ {zone}.getTransitions()|last|json_encode|raw }}}}"),
                    _ => format!("{{{{ {zone}.name }}}}"),
                }
            }
            9 => {
                let zone = self.timezone_target();
                format!("{{{{ {zone}.getOffset({object}) }}}}")
            }
            10 => {
                let zone = self.timezone_target();
                format!("{{{{ {object}.setTimezone({zone})|date({format}, false) }}}}")
            }
            11 => {
                let micro = int(self, &[0, 1, 999_999, 1_000_000, -1]);
                format!("{{{{ {object}.setMicrosecond({micro})|date('s.u') }}}}")
            }
            _ => {
                let other = self.method_target();
                format!(
                    "{{{{ {object}.diff({other}).days }}}}|{{{{ {object} == {other} ? 'eq' : 'ne' }}}}"
                )
            }
        }
    }

    /// `date(expr)` (sometimes with a timezone).
    fn date_object(&mut self, expr: &str) -> String {
        if self.rng.chance(30) {
            format!("date({expr}, {})", self.timezone_arg())
        } else {
            format!("date({expr})")
        }
    }

    /// A date input that does not depend on "now" when `other_now` is set.
    fn fixed_date_input(&mut self, other_now: bool) -> DateIn {
        for _ in 0..20 {
            let date = self.date_input();
            if !(other_now && date.now) {
                return date;
            }
        }
        DateIn {
            expr: self.put(V::Int(1_700_000_000)),
            now: false,
        }
    }

    /// A date object fixed in time (a data object or `date()` of a fixed input).
    fn date_fixed_object(&mut self) -> String {
        if self.rng.chance(50) {
            let object = self.datetime_object();
            self.var(object)
        } else {
            let date = self.fixed_date_input(true);
            self.date_object(&date.expr)
        }
    }

    fn date_input(&mut self) -> DateIn {
        match self.rng.weighted(&[20, 8, 5, 33, 6, 16, 2, 6, 4]) {
            0 => {
                let ts = self.timestamp();
                DateIn {
                    expr: self.put(V::Int(ts)),
                    now: false,
                }
            }
            1 => {
                let (s, now) = self.timestamp_string();
                DateIn {
                    expr: self.put(V::Str(s.into_bytes())),
                    now,
                }
            }
            2 => {
                let value = if self.rng.chance(70) {
                    V::f(self.pick_static(&[
                        "0.0",
                        "-0.0",
                        "1.5",
                        "-1.5",
                        "0.5",
                        "1700000000.5",
                        "1.0e10",
                        "1.0E+25",
                        "5.0",
                        "-5.0",
                        "100.0",
                        "1e999",
                        "0.1",
                        "12.5",
                        "2020.5",
                        "20200101.0",
                        "1.0e15",
                        "86400.0",
                        "1.5e3",
                    ]))
                } else {
                    V::Float(self.random_float())
                };
                DateIn {
                    expr: self.put(value),
                    now: true,
                }
            }
            3 => {
                let (s, now) = self.date_string();
                DateIn {
                    expr: self.put(V::Str(s)),
                    now,
                }
            }
            4 => {
                let (value, now) = match self.rng.below(6) {
                    0 => (V::Null, true),
                    1 => (V::s("now"), true),
                    2 => (V::s(""), true),
                    3 => (V::Bool(false), true),
                    4 => (V::Bool(true), false),
                    _ => (V::s("NOW"), true),
                };
                DateIn {
                    expr: self.put(value),
                    now,
                }
            }
            5 => {
                let (value, now) = match self.rng.weighted(&[50, 18, 6, 6, 5, 5, 5, 5]) {
                    0 => (self.datetime_object(), false),
                    1 => (self.interval_object(), false),
                    2 => (V::php(self.timezone_object()), false),
                    3 => {
                        let (s, now) = self.date_string();
                        (
                            V::php(format!("new \\Twig\\Markup({}, 'UTF-8')", php_str(&s))),
                            now,
                        )
                    }
                    4 => (V::php("(object) ['date' => '2020-01-01']"), false),
                    5 => (V::php("new \\ArrayObject(['2020-01-01'])"), false),
                    6 => (V::php("new \\Twig\\Markup('1700000000', 'UTF-8')"), false),
                    _ => (
                        V::php(format!(
                            "new \\DateTimeZone({})",
                            php_plain(&self.pick_str(TZ_NAMES))
                        )),
                        false,
                    ),
                };
                DateIn {
                    expr: self.var(value),
                    now,
                }
            }
            6 => {
                let value = match self.rng.below(3) {
                    0 => V::List(Vec::new()),
                    1 => V::List(vec![V::Int(1)]),
                    _ => V::Map(vec![(Key::Str("a".to_owned()), V::s("2020-01-01"))]),
                };
                DateIn {
                    expr: self.put(value),
                    now: false,
                }
            }
            7 => {
                // a date object made by the template
                let inner = self.date_input();
                if self.rng.chance(50) {
                    DateIn {
                        expr: format!("date({}, {})", inner.expr, self.timezone_arg()),
                        now: inner.now,
                    }
                } else {
                    let modifier = self.modifier();
                    DateIn {
                        expr: format!("{}|date_modify({modifier})", inner.expr),
                        now: inner.now,
                    }
                }
            }
            _ => {
                // a near-timestamp string with junk
                let ts = self.timestamp();
                let s = match self.rng.below(5) {
                    0 => format!("@{ts}"),
                    1 => format!("{ts} "),
                    2 => format!(" {ts}"),
                    3 => format!("+{}", ts.unsigned_abs()),
                    _ => format!("{ts}.0"),
                };
                DateIn {
                    expr: self.put(V::Str(s.into_bytes())),
                    now: true,
                }
            }
        }
    }

    fn timestamp(&mut self) -> i64 {
        match self.rng.weighted(&[45, 20, 15, 10, 10]) {
            0 => *self.rng.pick(TIMESTAMPS),
            1 => self.rng.range(-2_000_000_000, 4_000_000_000),
            2 => self.rng.range(-100_000_000_000, 100_000_000_000),
            3 => self.rng.next_u64() as i64,
            _ => {
                // around DST transitions and year boundaries
                let base = *self.rng.pick(&[
                    1_585_443_600i64,
                    1_603_587_600,
                    1_616_893_200,
                    1_635_642_000,
                    1_577_836_800,
                    1_609_459_200,
                    946_684_800,
                    1_293_840_000,
                    -2_208_988_800,
                ]);
                base + self.rng.range(-7200, 7200)
            }
        }
    }

    /// A digit string (a timestamp for `convertDate()`) or a near miss; `true` when the near miss
    /// is parsed as a relative date.
    fn timestamp_string(&mut self) -> (String, bool) {
        match self.rng.weighted(&[30, 20, 20, 30]) {
            0 => (self.timestamp().to_string(), false),
            1 => {
                let s = (*self.rng.pick(&[
                    "0",
                    "-0",
                    "00",
                    "007",
                    "-007",
                    "1700000000",
                    "-1700000000",
                    "99999999999999999999",
                    "-99999999999999999999",
                    "9223372036854775808",
                    "-9223372036854775809",
                    "000000000000000000000001",
                    "0000",
                    "2020",
                    "20200101",
                    "202001011200",
                    "1",
                    "12",
                    "123",
                    "1234",
                    "12345",
                    "123456",
                    "1234567",
                ]))
                .to_owned();
                (s, false)
            }
            2 => {
                let len = 1 + self.rng.below(24);
                let mut s = String::new();
                if self.rng.chance(30) {
                    s.push('-');
                }
                for _ in 0..len {
                    s.push(char::from(b'0' + self.rng.below(10) as u8));
                }
                (s, false)
            }
            _ => {
                let s = (*self.rng.pick(&[
                    " 5", "5 ", "+5", "5.0", "1e3", "0x10", "-", "--5", "-+5", "- 5", "5-", "1 2",
                    "١", "0.", ".0", "-.5",
                ]))
                .to_owned();
                (s, true)
            }
        }
    }

    /// A date string and whether it depends on "now".
    fn date_string(&mut self) -> (Vec<u8>, bool) {
        match self.rng.weighted(&[20, 12, 14, 40, 8, 6]) {
            0 => (self.rng.pick(ABSOLUTE_DATES).as_bytes().to_vec(), false),
            1 => (self.rng.pick(RELATIVE_DATES).as_bytes().to_vec(), true),
            2 => (self.rng.pick(GARBAGE_DATES).as_bytes().to_vec(), true),
            3 => {
                let (s, now) = self.compose_date_string();
                (s.into_bytes(), now)
            }
            4 => (self.token_soup().into_bytes(), true),
            // a valid string with a few edits (the parser's error positions and messages)
            _ => {
                let mut s = if self.rng.chance(50) {
                    self.pick_str(ABSOLUTE_DATES)
                } else {
                    self.compose_date_string().0
                };
                for _ in 0..1 + self.rng.weighted(&[60, 30, 10]) {
                    self.mutate(&mut s);
                }
                (s.into_bytes(), true)
            }
        }
    }

    /// Random date tokens (numbers of every length, words, zones, signs) with random separators.
    fn token_soup(&mut self) -> String {
        let n = 1 + self.rng.weighted(&[20, 30, 25, 15, 10]);
        let mut s = String::new();
        for i in 0..n {
            if i > 0 && self.rng.chance(60) {
                s.push_str(
                    self.pick_static(&[
                        " ", " ", " ", "-", "/", ".", ":", ",", "T", "+", "  ", "\t",
                    ]),
                );
            }
            let token = self.date_token();
            s.push_str(&token);
        }
        s
    }

    fn date_token(&mut self) -> String {
        match self.rng.weighted(&[32, 22, 10, 6, 5, 5, 6, 4, 6, 4]) {
            0 => {
                let len = 1 + self.rng.weighted(&[25, 25, 10, 20, 5, 5, 4, 6]);
                let mut digits = String::new();
                for _ in 0..len {
                    digits.push(char::from(b'0' + self.rng.below(10) as u8));
                }
                digits
            }
            1 => self.pick_str(&[
                "next",
                "last",
                "previous",
                "this",
                "ago",
                "of",
                "day",
                "days",
                "week",
                "month",
                "year",
                "noon",
                "midnight",
                "today",
                "now",
                "tomorrow",
                "yesterday",
                "back",
                "front",
                "first",
                "second",
                "third",
                "twelfth",
                "weekday",
                "weekdays",
                "fortnight",
                "hour",
                "min",
                "sec",
                "msec",
                "usec",
                "eighth",
                "a",
                "and",
            ]),
            2 => self.pick_str(MONTHS),
            3 => self.pick_str(DAYS),
            4 => self.pick_str(TZ_ABBRS),
            5 => self.pick_str(&TZ_NAMES[..60]),
            6 => self.pick_str(&["am", "pm", "a.m.", "p.m.", "AM", "st", "nd", "rd", "th"]),
            7 => format!("@{}", self.rng.range(-100_000, 3_000_000_000)),
            8 => format!(
                "{}{}",
                self.rng.pick(&["+", "-"]),
                self.rng.range(0, 99_999)
            ),
            _ => self.pick_str(&["T", "t", "W", "w", "Z", "z", "X", "µ", "é", "_"]),
        }
    }

    /// One random edit of `s`: insert, delete, replace or swap characters, or repeat a part.
    fn mutate(&mut self, s: &mut String) {
        const INSERT: &[&str] = &[
            "x", " ", "-", "+", ":", "/", ".", "@", "0", "1", "9", "a", "T", "Z", "é", ",", "(",
            ")", "\t", "W", "_", "'", "#",
        ];
        if s.is_empty() {
            s.push_str(self.pick_static(INSERT));
            return;
        }
        let at = self.char_boundary(s);
        let len = s[at..].chars().next().map_or(0, char::len_utf8);
        match self.rng.below(5) {
            0 => s.insert_str(at, self.pick_static(INSERT)),
            1 => s.replace_range(at..at + len, ""),
            2 => {
                let with = *self.rng.pick(INSERT);
                s.replace_range(at..at + len, with);
            }
            3 => {
                let next = at + len;
                if let Some(c) = s[next..].chars().next() {
                    let first: String = s[at..next].to_owned();
                    s.replace_range(at..next + c.len_utf8(), &format!("{c}{first}"));
                }
            }
            _ => {
                let part: String = s[at..].chars().take(3).collect();
                s.insert_str(at, &part);
            }
        }
    }

    /// A date string made of random date, time, zone and relative parts.
    fn compose_date_string(&mut self) -> (String, bool) {
        let mut parts: Vec<String> = Vec::new();
        let mut absolute = false;
        if self.rng.chance(65) {
            let (part, is_absolute) = self.date_part();
            absolute = is_absolute;
            parts.push(part);
        }
        let mut has_time = false;
        if self.rng.chance(50) {
            parts.push(self.time_part());
            has_time = true;
        }
        if self.rng.chance(20) {
            parts.push(self.zone_part());
        }
        let relative = if self.rng.chance(35) {
            1 + self.rng.below(3)
        } else {
            0
        };
        for _ in 0..relative {
            parts.push(self.relative_part());
        }
        if parts.is_empty() {
            parts.push(self.relative_part());
        }
        if parts.len() > 1 && self.rng.chance(12) {
            let i = self.rng.below(parts.len());
            let j = self.rng.below(parts.len());
            parts.swap(i, j);
        }
        let sep = if absolute && has_time && parts.len() == 2 && self.rng.chance(25) {
            "T"
        } else {
            *self.rng.pick(&[" ", " ", " ", " ", "  ", ", ", "\t", ","])
        };
        let mut s = parts.join(sep);
        // an edit can turn the string into one that depends on "now"
        let mut edited = false;
        match self.rng.weighted(&[80, 5, 5, 8, 2]) {
            1 => s = s.to_uppercase(),
            2 => s = s.to_lowercase(),
            3 => {
                self.mutate(&mut s);
                edited = true;
            }
            4 => {
                s = format!("{s} {s}");
                edited = true;
            }
            _ => {}
        }
        (s, !absolute || edited)
    }

    fn char_boundary(&mut self, s: &str) -> usize {
        let boundaries: Vec<usize> = s.char_indices().map(|(i, _)| i).collect();
        *self.rng.pick(&boundaries)
    }

    fn year(&mut self) -> i64 {
        match self.rng.weighted(&[70, 15, 10, 5]) {
            0 => self.rng.range(1970, 2040),
            1 => self.rng.range(1900, 2100),
            2 => self.rng.range(0, 9999),
            _ => *self.rng.pick(&[0, 1, 1582, 1752, 9999, 10000, 99999]),
        }
    }

    /// A date part and whether it fixes the date (a month and day without a year do not).
    fn date_part(&mut self) -> (String, bool) {
        let y = self.year();
        let m = if self.rng.chance(95) {
            self.rng.range(1, 12)
        } else {
            *self.rng.pick(&[0, 13])
        };
        let d = match self.rng.weighted(&[80, 15, 5]) {
            0 => self.rng.range(1, 28),
            1 => self.rng.range(29, 31),
            _ => *self.rng.pick(&[0, 32]),
        };
        let month = self.pick_str(MONTHS);
        let day = self.pick_str(DAYS);
        let suffix = *self.rng.pick(&["st", "nd", "rd", "th"]);
        let part = match self.rng.below(24) {
            0..=2 => format!("{y:04}-{m:02}-{d:02}"),
            3 => format!("{y}-{m}-{d}"),
            4 => format!("{y:04}/{m:02}/{d:02}"),
            5 => format!("{m:02}/{d:02}/{y:04}"),
            6 => return (format!("{m}/{d}"), false),
            7 => format!("{d:02}.{m:02}.{y:04}"),
            8 => format!("{d}.{m}.{:02}", y % 100),
            9 => format!("{d:02}-{m:02}-{y:04}"),
            10 => format!("{y:04}{m:02}{d:02}"),
            11 => format!("{d} {month} {y}"),
            12 => format!("{month} {d}, {y}"),
            13 => format!("{month} {d}{suffix} {y}"),
            14 => format!("{d}-{month}-{y}"),
            15 => format!("{month} {y}"),
            16 => format!("{y:04}-{m:02}"),
            17 => format!("{y:04}-{:03}", self.rng.range(0, 367)),
            18 => format!("{y:04}-W{:02}", self.rng.range(0, 54)),
            19 => format!(
                "{y:04}-W{:02}-{}",
                self.rng.range(1, 53),
                self.rng.range(0, 8)
            ),
            20 => {
                let ts = self.timestamp();
                if self.rng.chance(30) {
                    format!("@{ts}.{}", self.rng.range(0, 999_999))
                } else {
                    format!("@{ts}")
                }
            }
            21 => format!(
                "{}{:05}-{m:02}-{d:02}",
                if self.rng.chance(50) { "+" } else { "-" },
                self.rng.range(0, 300_000)
            ),
            22 => format!("{day}, {d:02} {month} {y:04}"),
            _ => return (format!("{month} {d}"), false),
        };
        (part, true)
    }

    fn time_part(&mut self) -> String {
        let h = if self.rng.chance(90) {
            self.rng.range(0, 23)
        } else {
            *self.rng.pick(&[24, 25, 99])
        };
        let i = if self.rng.chance(95) {
            self.rng.range(0, 59)
        } else {
            60
        };
        let s = if self.rng.chance(95) {
            self.rng.range(0, 59)
        } else {
            *self.rng.pick(&[60, 61])
        };
        let h12 = self.rng.range(0, 13);
        let ampm = *self
            .rng
            .pick(&["am", "pm", "a.m.", "p.m.", "AM", "PM", "A.M."]);
        match self.rng.below(13) {
            0 | 1 => format!("{h:02}:{i:02}"),
            2 | 3 => format!("{h:02}:{i:02}:{s:02}"),
            4 => {
                let digits = 1 + self.rng.below(9);
                let mut frac = String::new();
                for _ in 0..digits {
                    frac.push(char::from(b'0' + self.rng.below(10) as u8));
                }
                format!("{h:02}:{i:02}:{s:02}.{frac}")
            }
            5 => format!("{h12}{ampm}"),
            6 => format!("{h12}:{i:02}{ampm}"),
            7 => format!("{h12}:{i:02} {ampm}"),
            8 => format!("T{h:02}:{i:02}:{s:02}"),
            9 => format!("t{h:02}{i:02}{s:02}"),
            10 => (*self.rng.pick(&["noon", "midnight", "NOON"])).to_owned(),
            11 => format!("{h}:{i}"),
            _ => format!("{h:02}:{i:02}:{s:02},{}", self.rng.range(0, 999)),
        }
    }

    fn zone_part(&mut self) -> String {
        let sign = *self.rng.pick(&["+", "-"]);
        let hh = self.rng.range(0, 15);
        let mm = *self.rng.pick(&[0, 0, 0, 30, 45, 15, 59, 60]);
        match self.rng.below(11) {
            0 => "Z".to_owned(),
            1 => (*self.rng.pick(&["UTC", "GMT", "utc"])).to_owned(),
            2 => format!("{sign}{hh:02}:{mm:02}"),
            3 => format!("{sign}{hh:02}{mm:02}"),
            4 => format!("{sign}{hh}"),
            5 => format!("GMT{sign}{hh}"),
            6 => format!("UTC{sign}{hh:02}:{mm:02}"),
            7 => self.pick_str(&TZ_NAMES[..60]),
            8 => self.pick_str(TZ_ABBRS),
            9 => format!("({})", self.pick_str(TZ_ABBRS)),
            _ => "Mars/Phobos".to_owned(),
        }
    }

    fn relative_part(&mut self) -> String {
        let n = match self.rng.weighted(&[80, 15, 5]) {
            0 => self.rng.range(0, 100),
            1 => self.rng.range(100, 100_000),
            _ => *self.rng.pick(&[
                1_000_000,
                2_147_483_648,
                9_223_372_036_854_775_807,
                99_999_999_999,
                12_345_678_901_234,
            ]),
        };
        let unit = self.pick_str(UNITS);
        let sign = *self.rng.pick(&["+", "-", "", "+", "-"]);
        let day = self.pick_str(DAYS);
        let month = self.pick_str(MONTHS);
        let ordinal = *self.rng.pick(ORDINALS);
        match self.rng.below(16) {
            0..=3 => format!("{sign}{n} {unit}"),
            4 => format!("{n} {unit} ago"),
            5 => format!(
                "{} {unit}",
                self.rng.pick(&["next", "last", "this", "previous"])
            ),
            6 => {
                let which = *self.rng.pick(&["first", "last"]);
                let tail = *self.rng.pick(&[
                    "",
                    " next month",
                    " this month",
                    " last month",
                    " january",
                    " next year",
                ]);
                format!("{which} day of{tail}")
            }
            7 => {
                let tail = *self
                    .rng
                    .pick(&["", " next month", " this month", " january 2021"]);
                format!("{ordinal} {day} of{tail}")
            }
            8 => day,
            9 => format!(
                "{} {day}",
                self.rng.pick(&["next", "last", "this", "previous"])
            ),
            10 => format!(
                "{day} {} week",
                self.rng.pick(&["next", "last", "this", "previous"])
            ),
            11 => (*self
                .rng
                .pick(&["tomorrow", "yesterday", "today", "now", "midnight", "noon"]))
            .to_owned(),
            12 => format!(
                "{} of {}{}",
                self.rng.pick(&["back", "front"]),
                self.rng.range(0, 24),
                self.rng.pick(&["", "am", "pm"])
            ),
            13 => format!("{sign}{n} weekdays"),
            14 => format!("{ordinal} {unit}"),
            _ => format!("{sign}{n} {month}"),
        }
    }

    /// A `date_modify` modifier (a Twig expression). `DateTime::modify()` changes the date's own
    /// fields and never reads the clock, so a modifier does not make a date depend on "now".
    fn modifier(&mut self) -> String {
        let value = match self.rng.weighted(&[60, 12, 10, 8, 10]) {
            0 => {
                let mut parts = Vec::new();
                for _ in 0..1 + self.rng.weighted(&[60, 30, 10]) {
                    parts.push(self.relative_part());
                }
                if self.rng.chance(15) {
                    parts.push(self.time_part());
                }
                V::Str(parts.join(" ").into_bytes())
            }
            1 => V::Str(self.compose_date_string().0.into_bytes()),
            2 => V::Str(self.rng.pick(GARBAGE_DATES).as_bytes().to_vec()),
            3 => V::Str(self.rng.pick(ABSOLUTE_DATES).as_bytes().to_vec()),
            _ => match self.rng.below(7) {
                0 => V::Int(5),
                1 => V::Null,
                2 => V::Bool(true),
                3 => V::f("1.5"),
                4 => V::List(vec![V::s("+1 day")]),
                5 => V::s(""),
                _ => V::php_twig("new \\Twig\\Markup('+1 day', 'UTF-8')", "'+1 day'|raw"),
            },
        };
        self.put(value)
    }

    /// A date format as a Twig expression (`micro_ok`: `u` and `v` may appear).
    fn date_format(&mut self, micro_ok: bool) -> String {
        match self.rng.weighted(&[12, 4, 74, 10]) {
            0 => {
                let format = self.pick_str(DATE_FORMATS);
                let format = if micro_ok {
                    format.into_bytes()
                } else {
                    strip_micro(format.as_bytes())
                };
                self.put(V::Str(format))
            }
            1 => self.pick_str(DATE_FORMAT_CONSTANTS),
            2 => {
                let bytes = self.date_format_bytes(micro_ok);
                self.put(V::Str(bytes))
            }
            _ => {
                let value = match self.rng.below(7) {
                    0 => V::Int(self.rng.range(-5, 100)),
                    1 => V::f("1.5"),
                    2 => V::Bool(true),
                    3 => V::Bool(false),
                    4 => V::Null,
                    5 => V::List(vec![V::s("Y")]),
                    _ => V::php_twig("new \\Twig\\Markup('Y-m-d', 'UTF-8')", "'Y-m-d'|raw"),
                };
                self.put(value)
            }
        }
    }

    /// Random `DateTimeInterface::format()` bytes.
    fn date_format_bytes(&mut self, micro_ok: bool) -> Vec<u8> {
        let len = 1 + self.rng.weighted(&[20, 20, 15, 12, 10, 8, 5, 4, 3, 3]);
        let mut out = Vec::new();
        for _ in 0..len {
            match self.rng.weighted(&[50, 18, 8, 12, 4, 4, 2, 2]) {
                // any letter
                0 | 1 => {
                    let letter = if self.rng.chance(70) {
                        *self.rng.pick(FORMAT_LETTERS)
                    } else {
                        let c = self.rng.below(52) as u8;
                        if c < 26 { b'a' + c } else { b'A' + c - 26 }
                    };
                    if micro_ok || !matches!(letter, b'u' | b'v') {
                        out.push(letter);
                    }
                }
                // an escaped letter
                2 => {
                    out.push(b'\\');
                    out.push(*self.rng.pick(FORMAT_LETTERS));
                }
                3 => out.push(*self.rng.pick(b" -:/.,T")),
                4 => out.extend_from_slice(self.rng.pick(&["é", "日", "\u{a0}"]).as_bytes()),
                5 => out.push(b'0' + self.rng.below(10) as u8),
                6 => out.extend_from_slice(
                    self.rng
                        .pick(&["\\\\", "%", "'", "\"", "<", "&"])
                        .as_bytes(),
                ),
                _ => out.push(b'\\'),
            }
        }
        // a lone backslash before an escaped letter escapes the backslash instead
        if micro_ok { out } else { strip_micro(&out) }
    }

    /// A timezone argument (a Twig expression).
    fn timezone_arg(&mut self) -> String {
        match self.rng.weighted(&[48, 10, 8, 5, 10, 6, 6, 4, 3]) {
            0 => {
                let name = self.pick_str(TZ_NAMES);
                self.put(V::s(&name))
            }
            1 => {
                let offset = self.pick_str(TZ_OFFSETS);
                self.put(V::s(&offset))
            }
            2 => {
                let abbr = self.pick_str(TZ_ABBRS);
                self.put(V::s(&abbr))
            }
            3 => {
                let invalid = self.pick_str(TZ_INVALID);
                self.put(V::s(&invalid))
            }
            4 => "false".to_owned(),
            5 => "null".to_owned(),
            6 => {
                let object = self.timezone_object();
                self.var(V::php(object))
            }
            7 => {
                let value = match self.rng.below(5) {
                    0 => V::Int(0),
                    1 => V::Int(5),
                    2 => V::Bool(true),
                    3 => V::List(vec![V::s("UTC")]),
                    _ => V::f("1.5"),
                };
                self.put(value)
            }
            _ => {
                // the zone of a date made by the template: its name (the `timezone` property)
                // or the `DateTimeZone` object
                let date = self.fixed_date_input(true);
                let member = if self.rng.chance(50) {
                    "timezone"
                } else {
                    "getTimezone()"
                };
                format!("date({}, {}).{member}", date.expr, self.timezone_arg())
            }
        }
    }

    fn timezone_object(&mut self) -> String {
        let name = match self.rng.weighted(&[70, 15, 15]) {
            0 => self.pick_str(&TZ_NAMES[..60]),
            // offsets `new \DateTimeZone()` accepts (the data code must not throw)
            1 => self.pick_str(&TZ_OFFSETS[..15]),
            _ => self.pick_str(&TZ_ABBRS[..13]),
        };
        format!("new \\DateTimeZone({})", php_plain(&name))
    }

    /// A `DateTime` or `DateTimeImmutable` built by the data code.
    fn datetime_object(&mut self) -> V {
        let class = if self.rng.chance(50) {
            "\\DateTime"
        } else {
            "\\DateTimeImmutable"
        };
        let local = {
            // LMT-era offsets before 1900, far-future rules after 2037
            let y = match self.rng.weighted(&[70, 15, 10, 5]) {
                0 => self.rng.range(1900, 2100),
                1 => self.rng.range(1600, 1899),
                2 => self.rng.range(2100, 3000),
                _ => self.rng.range(1, 999),
            };
            let m = self.rng.range(1, 12);
            let d = self.rng.range(1, 28);
            let h = self.rng.range(0, 23);
            let i = self.rng.range(0, 59);
            let s = self.rng.range(0, 59);
            let micro = if self.rng.chance(40) {
                format!(".{:06}", self.rng.range(0, 999_999))
            } else {
                String::new()
            };
            format!("{y:04}-{m:02}-{d:02} {h:02}:{i:02}:{s:02}{micro}")
        };
        let code = match self.rng.weighted(&[40, 15, 10, 10, 10, 8, 7]) {
            0 => format!(
                "new {class}({}, {})",
                php_plain(&local),
                self.timezone_object()
            ),
            1 => format!("new {class}({})", php_plain(&local)),
            2 => format!(
                "new {class}('@{}')",
                self.rng.range(-5_000_000_000, 5_000_000_000)
            ),
            3 => format!(
                "new {class}({})",
                php_plain(&format!(
                    "{local} {}",
                    self.pick_str(&["EST", "CEST", "+05:30", "-0800", "Z", "GMT+2", "PDT"])
                ))
            ),
            4 => format!(
                "(new {class}({}))->setTimezone({})",
                php_plain(&local),
                self.timezone_object()
            ),
            5 => format!(
                "{class}::createFromFormat('Y-m-d H:i:s', {}, {})",
                php_plain(&local[..19]),
                self.timezone_object()
            ),
            _ => {
                // a modifier PHP accepts (the data code must not throw)
                let modifier = format!(
                    "{}{} {}",
                    self.rng.pick(&["+", "-", ""]),
                    self.rng.range(0, 500),
                    self.rng.pick(&[
                        "sec",
                        "seconds",
                        "min",
                        "minutes",
                        "hour",
                        "hours",
                        "day",
                        "days",
                        "week",
                        "weeks",
                        "fortnight",
                        "month",
                        "months",
                        "year",
                        "years",
                        "weekday",
                        "weekdays",
                    ])
                );
                format!(
                    "(new {class}({}))->modify({})",
                    php_plain(&local),
                    php_plain(&modifier)
                )
            }
        };
        V::php(code)
    }

    /// A `DateInterval` built by the data code.
    fn interval_object(&mut self) -> V {
        let code = match self.rng.weighted(&[40, 35, 15, 10]) {
            0 => format!("new \\DateInterval('{}')", self.interval_spec()),
            1 => {
                let a = self.fixed_php_date();
                let b = self.fixed_php_date();
                let absolute = if self.rng.chance(25) { ", true" } else { "" };
                format!("(new \\DateTime({a}))->diff(new \\DateTime({b}){absolute})")
            }
            2 => {
                let spec = self.interval_spec();
                // not `days`: writing it adds a dynamic property (a deprecation) that the typed
                // data cannot carry over
                let field = *self.rng.pick(&["invert", "f", "d", "y", "h", "s"]);
                // fractions that survive `f * 1000000` exactly (the typed data records `f`, and
                // `__set_state()` truncates `f * 1000000` when twig-rs rebuilds the interval)
                let value = match field {
                    "f" => (*self.rng.pick(&["0.5", "0.015625", "1.5", "-0.25"])).to_owned(),
                    "invert" => self.rng.range(0, 1).to_string(),
                    _ => self.rng.range(-5, 400).to_string(),
                };
                format!(
                    "(function () {{ $i = new \\DateInterval('{spec}'); $i->{field} = {value}; return $i; }})()"
                )
            }
            _ => {
                let a = self.fixed_php_date();
                let b = self.fixed_php_date();
                format!("(new \\DateTimeImmutable({a}))->diff(new \\DateTimeImmutable({b}))")
            }
        };
        V::php(code)
    }

    /// A PHP string literal of a fixed date (with a zone sometimes) for intervals of the data:
    /// microseconds are multiples of 1/64 s, so that the `f` of a difference survives the typed
    /// data (see [`Gen::interval_object`]).
    fn fixed_php_date(&mut self) -> String {
        let y = self.rng.range(1960, 2060);
        let m = self.rng.range(1, 12);
        let d = self.rng.range(1, 31);
        let h = self.rng.range(0, 23);
        let i = self.rng.range(0, 59);
        let s = self.rng.range(0, 59);
        let micro = if self.rng.chance(30) {
            format!(".{:06}", self.rng.range(0, 63) * 15_625)
        } else {
            String::new()
        };
        let zone = if self.rng.chance(30) {
            format!(" {}", self.pick_str(&TZ_NAMES[..40]))
        } else {
            String::new()
        };
        php_plain(&format!(
            "{y:04}-{m:02}-{d:02} {h:02}:{i:02}:{s:02}{micro}{zone}"
        ))
    }

    fn interval_spec(&mut self) -> String {
        let mut spec = String::from("P");
        let mut any = false;
        for unit in ["Y", "M", "D"] {
            if self.rng.chance(45) {
                let _ = write!(spec, "{}{unit}", self.small_count());
                any = true;
            }
        }
        if !any && self.rng.chance(30) {
            let _ = write!(spec, "{}W", self.small_count());
            any = true;
        }
        let mut time = String::new();
        for unit in ["H", "M", "S"] {
            if self.rng.chance(40) {
                let _ = write!(time, "{}{unit}", self.small_count());
            }
        }
        if !time.is_empty() {
            spec.push('T');
            spec.push_str(&time);
        } else if !any {
            spec.push_str("0D");
        }
        spec
    }

    fn small_count(&mut self) -> i64 {
        match self.rng.weighted(&[70, 20, 10]) {
            0 => self.rng.range(0, 12),
            1 => self.rng.range(13, 400),
            _ => self.rng.range(400, 100_000),
        }
    }

    // ---- intervals ------------------------------------------------------------------------------

    fn intervals_statement(&mut self) -> String {
        if self.rng.chance(25) {
            // differences across offset changes, in one zone or between zones
            let (zone, a) = self.transition_time();
            let (other_zone, b) = if self.rng.chance(70) {
                let (_, b) = self.transition_time();
                let shift = self.transition_time().1;
                (zone.clone(), if self.rng.chance(50) { b } else { shift })
            } else {
                self.transition_time()
            };
            let left = self.put(V::Str(format!("{a} {zone}").into_bytes()));
            let right = self.put(V::Str(format!("{b} {other_zone}").into_bytes()));
            let absolute = if self.rng.chance(20) { ", true" } else { "" };
            let format = if self.rng.chance(60) {
                self.put(V::s("%R %y-%m-%d %h:%i:%s %a"))
            } else {
                self.interval_format(true)
            };
            // `false` keeps the parsed zones (the default converts both to the Twig timezone)
            let keep = if self.rng.chance(70) { ", false" } else { "" };
            return format!(
                "{{{{ date({left}{keep}).diff(date({right}{keep}){absolute})|date({format}) }}}}"
            );
        }
        match self.rng.weighted(&[40, 30, 15, 15]) {
            // date(A).diff(date(B)[, absolute])|date(IF)
            0 => {
                let a = self.date_input();
                let b = self.fixed_date_input(a.now);
                let absolute = match self.rng.below(5) {
                    0 => ", true",
                    1 => ", false",
                    _ => "",
                };
                let left = self.date_object(&a.expr);
                let right = self.date_object(&b.expr);
                let format = self.interval_format(!(a.now || b.now));
                format!("{{{{ {left}.diff({right}{absolute})|date({format}) }}}}")
            }
            // interval objects
            1 => {
                let interval = self.interval_object();
                let interval = self.var(interval);
                if self.rng.chance(20) {
                    format!("{{{{ {interval}|date }}}}")
                } else {
                    let format = self.interval_format(true);
                    format!("{{{{ {interval}|date({format}) }}}}")
                }
            }
            // attributes of intervals
            2 => {
                let interval = self.interval_object();
                let interval = self.var(interval);
                let member = *self.rng.pick(&[
                    "y",
                    "m",
                    "d",
                    "h",
                    "i",
                    "s",
                    "f",
                    "invert",
                    "days",
                    "from_string",
                    "format('%a')",
                ]);
                format!("{{{{ {interval}.{member} }}}}")
            }
            // intervals in json and dumps
            _ => {
                let interval = self.interval_object();
                let interval = self.var(interval);
                if self.rng.chance(60) {
                    format!("{{{{ {interval}|json_encode|raw }}}}")
                } else {
                    self.debug = true;
                    format!("{{{{ dump({interval}) }}}}")
                }
            }
        }
    }

    /// An interval format as a Twig expression (`micro_ok`: `%f` and `%F` may appear).
    fn interval_format(&mut self, micro_ok: bool) -> String {
        if self.rng.chance(6) {
            let value = match self.rng.below(4) {
                0 => V::Null,
                1 => V::Int(7),
                2 => V::List(Vec::new()),
                _ => V::Bool(true),
            };
            return self.put(value);
        }
        let bytes = self.interval_format_bytes(micro_ok);
        self.put(V::Str(bytes))
    }

    fn interval_format_bytes(&mut self, micro_ok: bool) -> Vec<u8> {
        let atoms = 1 + self.rng.weighted(&[25, 25, 20, 12, 8, 5, 5]);
        let mut out = Vec::new();
        for i in 0..atoms {
            match self.rng.weighted(&[70, 10, 15, 5]) {
                0 => {
                    let c = *self.rng.pick(INTERVAL_LETTERS);
                    if micro_ok || !matches!(c, b'f' | b'F') {
                        out.push(b'%');
                        out.push(c);
                    }
                }
                // any letter after `%` (unknown ones are copied)
                1 => {
                    let c = self.rng.below(52) as u8;
                    let c = if c < 26 { b'a' + c } else { b'A' + c - 26 };
                    if micro_ok || !matches!(c, b'f' | b'F') {
                        out.push(b'%');
                        out.push(c);
                    }
                }
                2 => out.extend_from_slice(
                    self.rng
                        .pick(&[" ", "-", ":", " days ", "x", "é", "\\", "%%", "T", "/"])
                        .as_bytes(),
                ),
                _ => {
                    if i + 1 == atoms {
                        out.push(b'%');
                    }
                }
            }
        }
        out
    }

    // ---- numbers --------------------------------------------------------------------------------

    fn numbers_statement(&mut self) -> String {
        if self.rng.chance(15) {
            // the rounding decision on the last digit, through every rounding path
            let (value, precision) = self.halfway();
            let value = self.put(value);
            return match self.rng.below(5) {
                0 => format!("{{{{ {value}|round({precision}) }}}}"),
                1 => format!("{{{{ {value}|number_format({precision}) }}}}"),
                2 => format!(
                    "{{{{ {value}|round({precision}, 'ceil') }}}}|{{{{ {value}|round({precision}, 'floor') }}}}"
                ),
                3 => format!("{{{{ '%.{precision}f|%.{precision}e'|format({value}, {value}) }}}}"),
                _ => format!("{{{{ ({value} * 10)|round({precision}) }}}}"),
            };
        }
        match self.rng.weighted(&[38, 30, 10, 10, 6, 6]) {
            0 => {
                let number = self.number_value();
                let number = self.put(number);
                let mut args: Vec<String> = Vec::new();
                let count = self.rng.weighted(&[15, 25, 15, 45]);
                if count >= 1 {
                    let decimals = self.decimals();
                    args.push(self.put(decimals));
                }
                if count >= 2 {
                    let point = self.separator();
                    args.push(self.put(point));
                }
                if count >= 3 {
                    let thousands = self.separator();
                    args.push(self.put(thousands));
                }
                if !args.is_empty() && self.rng.chance(10) {
                    let names = ["decimal", "decimal_point", "thousand_sep"];
                    let named: Vec<String> = args
                        .iter()
                        .zip(names)
                        .map(|(value, name)| format!("{name}={value}"))
                        .collect();
                    format!("{{{{ {number}|number_format({}) }}}}", named.join(", "))
                } else if args.is_empty() {
                    format!("{{{{ {number}|number_format }}}}")
                } else {
                    format!("{{{{ {number}|number_format({}) }}}}", args.join(", "))
                }
            }
            1 => {
                let number = self.number_value();
                let number = self.put(number);
                match self.rng.weighted(&[20, 35, 45]) {
                    0 => format!("{{{{ {number}|round }}}}"),
                    1 => {
                        let precision = self.precision();
                        let precision = self.put(precision);
                        format!("{{{{ {number}|round({precision}) }}}}")
                    }
                    _ => {
                        let precision = self.precision();
                        let precision = self.put(precision);
                        let method = self.round_method();
                        let method = self.put(method);
                        if self.rng.chance(10) {
                            format!(
                                "{{{{ {number}|round(method={method}, precision={precision}) }}}}"
                            )
                        } else {
                            format!("{{{{ {number}|round({precision}, {method}) }}}}")
                        }
                    }
                }
            }
            2 => {
                let number = self.number_value();
                let number = self.put(number);
                format!("{{{{ {number}|abs }}}}")
            }
            // chains
            3 => {
                let number = self.number_value();
                let number = self.put(number);
                let precision = self.precision();
                let precision = self.put(precision);
                let decimals = self.decimals();
                let decimals = self.put(decimals);
                match self.rng.below(3) {
                    0 => format!("{{{{ {number}|round({precision})|number_format({decimals}) }}}}"),
                    1 => {
                        let method = self.round_method();
                        let method = self.put(method);
                        format!("{{{{ {number}|abs|round({precision}, {method}) }}}}")
                    }
                    _ => format!("{{{{ ({number}|number_format({decimals}, '.', ''))|round }}}}"),
                }
            }
            // the defaults of setNumberFormat()
            4 => {
                let number = self.number_value();
                let number = self.put(number);
                format!("{{{{ {number}|number_format }}}}/{{{{ {number}|number_format(2) }}}}")
            }
            // results printed through other paths
            _ => {
                let number = self.number_value();
                let number = self.put(number);
                let precision = self.precision();
                let precision = self.put(precision);
                match self.rng.below(3) {
                    0 => format!("{{{{ {number}|round({precision})|json_encode }}}}"),
                    1 => format!("{{{{ ({number}|round({precision})) ~ '' }}}}"),
                    _ => format!("{{{{ [{number}|abs, {number}|round({precision})]|join(';') }}}}"),
                }
            }
        }
    }

    /// A random float as decimal text (see [`V::Float`]).
    fn random_float(&mut self) -> String {
        let negative = self.rng.chance(30);
        let body = match self.rng.weighted(&[35, 20, 20, 10, 10, 5, 12]) {
            0 => {
                // 1..17 significant digits with the point anywhere
                let digits = 1 + self.rng.below(17);
                let mut s = String::new();
                s.push(char::from(b'1' + self.rng.below(9) as u8));
                for _ in 1..digits {
                    s.push(char::from(b'0' + self.rng.below(10) as u8));
                }
                let point = self.rng.below(digits + 4);
                if point == 0 {
                    format!("0.{s}")
                } else if point < digits {
                    format!("{}.{}", &s[..point], &s[point..])
                } else if point == digits {
                    format!("{s}.0")
                } else {
                    format!("0.{}{s}", "0".repeat(point - digits))
                }
            }
            1 => {
                // a halfway case for rounding
                let int = self.rng.range(0, 100_000);
                let decimals = self.rng.below(5);
                let mut s = format!("{int}.");
                for _ in 0..decimals {
                    s.push(char::from(b'0' + self.rng.below(10) as u8));
                }
                s.push('5');
                s
            }
            2 => {
                let mut s = format!("{}.", self.rng.range(1, 9));
                for _ in 0..self.rng.below(16) {
                    s.push(char::from(b'0' + self.rng.below(10) as u8));
                }
                if s.ends_with('.') {
                    s.push('0');
                }
                let exponent = self.rng.range(-330, 310);
                let e = *self.rng.pick(&["e", "E"]);
                format!("{s}{e}{exponent}")
            }
            3 => format!("{}.0", self.rng.range(0, i64::MAX / 1000)),
            4 => {
                let k = self.rng.range(-20, 25);
                (*self.rng.pick(&[
                    format!("9.999999999999999e{k}"),
                    format!("1.0000000000000002e{k}"),
                    format!("4.999999999999999e{k}"),
                    format!("5.000000000000001e{k}"),
                    format!("1.0e{k}"),
                    format!("5.0e{k}"),
                ]))
                .clone()
            }
            5 => self.pick_str(FLOATS).trim_start_matches('-').to_owned(),
            _ => {
                // around the 14th significant digit (`precision` = 14 printing) or the 17th
                // (`serialize_precision` = -1): a digit run, then 5, 49999 or 50001
                let lead = self.rng.range(1, 9).to_string();
                let run = *self.rng.pick(&["9", "0", "3", "6"]);
                let length = *self.rng.pick(&[12usize, 13, 14, 15, 16]);
                let tail = *self.rng.pick(&["5", "49999", "50001", "4", "6", ""]);
                let digits = format!("{lead}{}{tail}", run.repeat(length - 1));
                let exponent = self.rng.range(-8, 20);
                format!("{}.{}e{exponent}", &digits[..1], &digits[1..])
            }
        };
        if negative { format!("-{body}") } else { body }
    }

    /// A halfway value and the precision that decides on its last digit (for `round` and
    /// `number_format`).
    fn halfway(&mut self) -> (V, i64) {
        let int = *self
            .rng
            .pick(&[0, 1, 2, 5, 12, 99, 1234, 99_999, 4_503_599]);
        let decimals = 1 + self.rng.below(6);
        let mut s = format!("{int}.");
        for _ in 0..decimals - 1 {
            s.push(char::from(b'0' + self.rng.below(10) as u8));
        }
        s.push('5');
        if self.rng.chance(30) {
            s.insert(0, '-');
        }
        (V::Float(s), decimals as i64 - 1)
    }

    /// A value for the number filters.
    fn number_value(&mut self) -> V {
        match self.rng.weighted(&[22, 22, 18, 14, 5, 5, 6, 4, 4]) {
            0 => {
                if self.rng.chance(70) {
                    V::Int(*self.rng.pick(INTS))
                } else {
                    V::Int(self.rng.range(-10_000_000, 10_000_000))
                }
            }
            1 => V::f(self.pick_static(FLOATS)),
            2 => V::Float(self.random_float()),
            3 => V::Str(self.rng.pick(NUMERIC_STRINGS).as_bytes().to_vec()),
            4 => {
                let text = self.random_float();
                V::Str(text.into_bytes())
            }
            5 => (*self.rng.pick(&[V::Null, V::Bool(true), V::Bool(false)])).clone(),
            6 => match self.rng.below(4) {
                0 => V::php_twig("NAN", "constant('NAN')"),
                1 => V::php_twig("INF", "constant('INF')"),
                2 => V::php_twig("-INF", "(-constant('INF'))"),
                _ => V::php_twig("PHP_FLOAT_EPSILON", "constant('PHP_FLOAT_EPSILON')"),
            },
            7 => match self.rng.below(3) {
                0 => V::List(Vec::new()),
                1 => V::List(vec![V::Int(1)]),
                _ => V::List(vec![V::Int(1), V::Int(2)]),
            },
            _ => match self.rng.below(4) {
                0 => V::php_twig("new \\Twig\\Markup('12.5', 'UTF-8')", "'12.5'|raw"),
                1 => V::php("new \\DateTime('2020-01-01')"),
                2 => V::php("(object) ['a' => 1]"),
                _ => V::php_twig("PHP_FLOAT_MAX", "constant('PHP_FLOAT_MAX')"),
            },
        }
    }

    /// `number_format()` decimals.
    fn decimals(&mut self) -> V {
        match self.rng.weighted(&[50, 12, 10, 8, 8, 6, 6]) {
            0 => V::Int(self.rng.range(0, 6)),
            1 => V::Int(
                *self
                    .rng
                    .pick(&[7, 10, 15, 16, 17, 20, 30, 50, 100, 300, 330, 400, 600]),
            ),
            2 => V::Int(self.rng.range(-25, -1)),
            3 => (*self.rng.pick(&[
                V::s("2"),
                V::s("2.5"),
                V::s("x"),
                V::s(""),
                V::s(" 3"),
                V::s("1e1"),
            ]))
            .clone(),
            4 => (*self.rng.pick(&[
                V::f("2.5"),
                V::f("2.0"),
                V::f("-1.5"),
                V::f("1.0e20"),
                V::f("1e999"),
            ]))
            .clone(),
            5 => (*self.rng.pick(&[V::Null, V::Bool(true), V::Bool(false)])).clone(),
            // huge positive decimals make PHP allocate that many digits (a fatal error at the hard
            // memory limit that kills the oracle), so they stay small
            _ => V::Int(
                *self
                    .rng
                    .pick(&[i64::MIN, -2_147_483_648, -400, -330, 999, 1000]),
            ),
        }
    }

    fn separator(&mut self) -> V {
        match self.rng.weighted(&[80, 8, 6, 6]) {
            0 => V::s(self.pick_static(SEPARATORS)),
            1 => V::Null,
            2 => (*self
                .rng
                .pick(&[V::Int(0), V::Int(12), V::Bool(true), V::f("1.5")]))
            .clone(),
            _ => V::List(vec![V::s(".")]),
        }
    }

    /// `round()` precision.
    fn precision(&mut self) -> V {
        match self.rng.weighted(&[50, 12, 12, 8, 8, 6, 4]) {
            0 => V::Int(self.rng.range(0, 6)),
            1 => V::Int(self.rng.range(-6, -1)),
            2 => V::Int(*self.rng.pick(&[
                10, 14, 15, 16, 17, 20, 22, 23, 50, 100, 308, 309, 320, 400, -20, -22, -23, -308,
                -309, -400,
            ])),
            3 => (*self
                .rng
                .pick(&[V::s("2"), V::s("2.5"), V::s("x"), V::s(""), V::s("-1")]))
            .clone(),
            4 => (*self
                .rng
                .pick(&[V::f("2.5"), V::f("2.0"), V::f("-1.5"), V::f("1.0e20")]))
            .clone(),
            5 => (*self.rng.pick(&[V::Null, V::Bool(true), V::Bool(false)])).clone(),
            _ => V::Int(*self.rng.pick(&[
                i64::MAX,
                i64::MIN,
                2_147_483_647,
                -2_147_483_648,
                2_147_483_648,
            ])),
        }
    }

    fn round_method(&mut self) -> V {
        match self.rng.weighted(&[40, 25, 25, 10]) {
            0 => V::s("common"),
            1 => V::s("ceil"),
            2 => V::s("floor"),
            _ => (*self.rng.pick(&[
                V::s("up"),
                V::s("CEIL"),
                V::s(""),
                V::Null,
                V::Int(1),
                V::Bool(true),
                V::s("ceil "),
            ]))
            .clone(),
        }
    }

    // ---- sprintf --------------------------------------------------------------------------------

    fn sprintf_statement(&mut self) -> String {
        let (format, needed) = self.sprintf_format();
        let given = match self.rng.weighted(&[80, 10, 10]) {
            0 => needed,
            1 => needed.saturating_sub(1 + self.rng.below(2)),
            _ => needed + 1 + self.rng.below(2),
        };
        let mut args = Vec::new();
        for _ in 0..given {
            let value = self.sprintf_arg();
            args.push(self.put(value));
        }
        let format = self.put(V::Str(format));
        if args.is_empty() {
            format!("{{{{ {format}|format }}}}")
        } else {
            format!("{{{{ {format}|format({}) }}}}", args.join(", "))
        }
    }

    /// A random `sprintf()` format and the number of arguments it consumes.
    fn sprintf_format(&mut self) -> (Vec<u8>, usize) {
        let mut out = String::new();
        let mut next = 0usize;
        let mut max_arg = 0usize;
        let specs = 1 + self.rng.weighted(&[40, 30, 15, 10, 5]);
        for _ in 0..specs {
            if self.rng.chance(40) {
                out.push_str(
                    self.pick_static(&["x", " ", "|", "a b", "é", "[", "]", "%%", "-", "0"]),
                );
            }
            out.push('%');
            let mut uses = 0usize;
            // an argument number
            let mut explicit: Option<usize> = None;
            if self.rng.chance(10) {
                let n = match self.rng.weighted(&[85, 15]) {
                    0 => 1 + self.rng.below(3),
                    _ => *self.rng.pick(&[0usize, 2_147_483_647, 9]),
                };
                let _ = write!(out, "{n}$");
                explicit = Some(n);
            }
            // flags
            for _ in 0..self.rng.weighted(&[50, 30, 15, 5]) {
                match self.rng.below(6) {
                    0 => out.push('-'),
                    1 => out.push('+'),
                    2 => out.push(' '),
                    3 => out.push('0'),
                    // a custom padding character (a digit one too: a bare `'` before the width
                    // would make the first width digit the padding and leave a huge width)
                    4 => {
                        out.push('\'');
                        out.push(*self.rng.pick(&['*', 'x', '0', '#', '-', ' ']));
                    }
                    _ => {
                        out.push('\'');
                        out.push(*self.rng.pick(&['1', '9', '\'', 'é']));
                    }
                }
            }
            // width
            match self.rng.weighted(&[55, 35, 5, 5]) {
                0 => {}
                1 => {
                    let _ = write!(out, "{}", self.rng.range(1, 30));
                }
                2 => {
                    out.push('*');
                    uses += 1;
                }
                _ => out.push_str(self.pick_static(&["2147483647", "2147483648", "0", "00", "99"])),
            }
            // precision
            match self.rng.weighted(&[50, 35, 5, 5, 5]) {
                0 => {}
                1 => {
                    let _ = write!(out, ".{}", self.rng.range(0, 20));
                }
                2 => {
                    let _ = write!(out, ".{}", self.rng.range(50, 60));
                }
                3 => {
                    out.push_str(".*");
                    uses += 1;
                }
                _ => out.push('.'),
            }
            if self.rng.chance(5) {
                out.push('l');
            }
            // the conversion
            match self.rng.weighted(&[86, 7, 4, 3]) {
                0 => out.push(char::from(*self.rng.pick(SPRINTF_CONVERSIONS))),
                1 => out.push(char::from(
                    *self.rng.pick(b"aijkmnpqrtvwyzADIJKLMNOPQRSTUVWYZ"),
                )),
                2 => out.push('%'),
                // the end of the format
                _ => return (out.into_bytes(), next.max(max_arg) + uses),
            }
            match explicit {
                Some(n) => max_arg = max_arg.max(n.min(9)),
                None => next += 1,
            }
            next += uses;
        }
        if self.rng.chance(30) {
            out.push_str(self.pick_static(&["!", " end", "%%", "%'", "%-"]));
        }
        (out.into_bytes(), next.max(max_arg))
    }

    fn sprintf_arg(&mut self) -> V {
        match self.rng.weighted(&[25, 25, 15, 15, 6, 5, 5, 4]) {
            0 => {
                let int = if self.rng.chance(60) {
                    *self.rng.pick(INTS)
                } else {
                    self.rng.range(-1000, 100_000)
                };
                // a `*` width up to INT_MAX makes PHP pad to that many bytes (a fatal error at
                // the oracle's hard memory limit); larger ones are a ValueError
                V::Int(if (10_000_001..=2_147_483_647).contains(&int) {
                    int % 10_000
                } else {
                    int
                })
            }
            1 => V::f(self.pick_static(FLOATS)),
            2 => V::Float(self.random_float()),
            3 => V::Str(self.rng.pick(NUMERIC_STRINGS).as_bytes().to_vec()),
            4 => V::s(self.pick_static(&["abc", "Hello World", "é", "日本語", "", "x"])),
            5 => (*self.rng.pick(&[V::Null, V::Bool(true), V::Bool(false)])).clone(),
            6 => match self.rng.below(3) {
                0 => V::php_twig("NAN", "constant('NAN')"),
                1 => V::php_twig("INF", "constant('INF')"),
                _ => V::php_twig("-INF", "(-constant('INF'))"),
            },
            _ => match self.rng.below(3) {
                0 => V::List(vec![V::Int(1)]),
                1 => V::php_twig("new \\Twig\\Markup('7.5', 'UTF-8')", "'7.5'|raw"),
                _ => V::List(Vec::new()),
            },
        }
    }

    // ---- json -----------------------------------------------------------------------------------

    fn json_statement(&mut self) -> String {
        let value = self.json_value(0);
        let value = self.put(value);
        let call = match self.rng.weighted(&[35, 40, 15, 10]) {
            0 => "json_encode".to_owned(),
            1 => format!("json_encode({})", self.json_flags()),
            2 => format!("json_encode({}, {})", self.json_flags(), self.json_depth()),
            _ => {
                if self.rng.chance(50) {
                    format!("json_encode(depth={})", self.json_depth())
                } else {
                    format!("json_encode(flags={})", self.json_flags())
                }
            }
        };
        if self.rng.chance(50) {
            format!("{{{{ {value}|{call}|raw }}}}")
        } else {
            if self.rng.chance(30) {
                self.autoescape = Some(Json::Bool(false));
            }
            format!("{{{{ {value}|{call} }}}}")
        }
    }

    fn json_flags(&mut self) -> String {
        match self.rng.weighted(&[35, 35, 15, 15]) {
            0 => {
                let mut flags = 0i64;
                for _ in 0..1 + self.rng.below(3) {
                    flags |= self.rng.pick(JSON_FLAGS).1;
                }
                flags.to_string()
            }
            1 => {
                let count = 1 + self.rng.weighted(&[60, 30, 10]);
                let names: Vec<String> = (0..count)
                    .map(|_| format!("constant('{}')", self.rng.pick(JSON_FLAGS).0))
                    .collect();
                names.join(" b-or ")
            }
            2 => (*self
                .rng
                .pick(&["0", "-1", "8388608", "2147483647", "null", "'128'", "true"]))
            .to_owned(),
            _ => {
                let a = self.rng.pick(JSON_FLAGS).1;
                let b = self.rng.pick(JSON_FLAGS).1;
                (a | b | 128).to_string()
            }
        }
    }

    fn json_depth(&mut self) -> String {
        (*self.rng.pick(&[
            "0",
            "1",
            "2",
            "3",
            "4",
            "512",
            "-1",
            "2147483647",
            "2147483648",
            "'2'",
            "1.5",
            "null",
        ]))
        .to_owned()
    }

    fn json_value(&mut self, depth: usize) -> V {
        let container = if depth >= 3 { 0 } else { 30 };
        match self
            .rng
            .weighted(&[14, 14, 10, 18, 6, container, container / 2, 6, 4])
        {
            0 => V::Int(*self.rng.pick(INTS)),
            1 => V::f(self.pick_static(FLOATS)),
            2 => V::Float(self.random_float()),
            3 => V::s(self.pick_static(JSON_STRINGS)),
            4 => (*self.rng.pick(&[V::Null, V::Bool(true), V::Bool(false)])).clone(),
            5 => {
                let n = self.rng.below(4);
                V::List((0..n).map(|_| self.json_value(depth + 1)).collect())
            }
            6 => {
                let n = self.rng.below(4);
                let mut entries: Vec<(Key, V)> = Vec::new();
                for i in 0..n {
                    let key = match self.rng.below(4) {
                        0 => Key::Int(i as i64),
                        1 => Key::Int(self.rng.range(-3, 10)),
                        2 => Key::Str(self.pick_str(&["a", "b", "é", "", "0", "a/b", "1.5"])),
                        _ => Key::Str(format!("k{i}")),
                    };
                    let normalized = normalize_key(&key);
                    if entries.iter().any(|(k, _)| normalize_key(k) == normalized) {
                        continue;
                    }
                    let value = self.json_value(depth + 1);
                    entries.push((key, value));
                }
                V::Map(entries)
            }
            7 => match self.rng.below(5) {
                0 => V::php_twig("NAN", "constant('NAN')"),
                1 => V::php_twig("INF", "constant('INF')"),
                2 => V::Str(b"\xff\xfeabc".to_vec()),
                3 => V::Str(b"a\xc3".to_vec()),
                _ => V::Str(b"\xed\xa0\x80".to_vec()),
            },
            _ => match self.rng.below(6) {
                0 => self.datetime_object(),
                1 => self.interval_object(),
                2 => V::php(self.timezone_object()),
                3 => V::php_twig("new \\Twig\\Markup('<b>m</b>', 'UTF-8')", "'<b>m</b>'|raw"),
                4 => V::php("(object) ['a' => 1.5, 'b' => [true, null], 'c' => (object) []]"),
                _ => V::php("new \\ArrayObject(['x' => 0.1, 'y' => 2])"),
            },
        }
    }

    // ---- floats ---------------------------------------------------------------------------------

    fn floats_statement(&mut self) -> String {
        match self
            .rng
            .weighted(&[30, 10, 10, 10, 8, 14, 8, 5, 5, 6, 5, 6, 6, 8])
        {
            0 => {
                let value = self.float_value();
                let value = self.put(value);
                format!("{{{{ {value} }}}}")
            }
            1 => {
                let a = self.float_value();
                let a = self.put(a);
                let b = self.float_value();
                let b = self.put(b);
                format!("{{{{ {a} ~ '|' ~ {b} }}}}")
            }
            2 => {
                let n = 1 + self.rng.below(4);
                let mut items = Vec::new();
                for _ in 0..n {
                    let value = self.float_value();
                    items.push(self.put(value));
                }
                format!("{{{{ [{}]|join(', ') }}}}", items.join(", "))
            }
            3 => {
                let value = self.float_value();
                let value = self.put(value);
                format!("{{{{ {value}|json_encode }}}}")
            }
            4 => {
                self.debug = true;
                let value = if self.rng.chance(70) {
                    self.float_value()
                } else {
                    V::List(vec![self.float_value(), self.float_value()])
                };
                let value = self.put(value);
                format!("{{{{ dump({value}) }}}}")
            }
            // arithmetic on numbers (no operand that raises a diagnostic)
            5 => {
                let a = self.arith_operand();
                let b = self.arith_operand();
                let op = *self.rng.pick(&["+", "-", "*", "/", "%", "**", "//"]);
                format!("{{{{ {a} {op} {b} }}}}")
            }
            6 => {
                let value = self.float_value();
                let value = self.put(value);
                let filter = *self.rng.pick(&[
                    "length",
                    "upper",
                    "url_encode",
                    "e('js')",
                    "first",
                    "last",
                    "reverse",
                    "split('.')|join('_')",
                    "slice(0, 4)",
                    "trim('0')",
                ]);
                format!("{{{{ {value}|{filter} }}}}")
            }
            7 => {
                // literals the lexer reads (underscores, exponents)
                let literal = self.number_literal();
                format!("{{{{ {literal} }}}}")
            }
            8 => {
                let a = self.float_value();
                let a = self.put(a);
                let b = self.float_value();
                let b = self.put(b);
                match self.rng.below(4) {
                    0 => format!("{{{{ max({a}, {b}) }}}}"),
                    1 => format!("{{{{ min([{a}, {b}]) }}}}"),
                    2 => format!("{{{{ {a} <=> {b} }}}}"),
                    _ => format!("{{{{ [{a}, {b}]|sort|join(' ') }}}}"),
                }
            }
            // unary minus of a number
            9 => {
                let value = self.arith_operand();
                if self.rng.chance(50) {
                    format!("{{{{ -{value} }}}}")
                } else {
                    format!("{{{{ -(-{value}) }}}}|{{{{ +{value} }}}}")
                }
            }
            // string interpolation
            10 => {
                let a = self.float_value();
                let a = self.put(a);
                let b = self.arith_operand();
                format!("{{{{ \"#{{{a}}}|#{{{b} * 1}}\" }}}}")
            }
            // ranges with float bounds or steps (small: at most 40 elements)
            11 => {
                let start = *self.rng.pick(&[
                    "0", "0.1", "0.5", "1", "1.5", "-1.5", "-0.3", "0.7", "1.0e-7", "2.5",
                ]);
                let step = *self
                    .rng
                    .pick(&["0.1", "0.25", "0.3", "0.5", "1.5", "0.7", "0.01", "1", "2"]);
                let count = self.rng.range(0, 30);
                let step_value: f64 = step.parse().unwrap_or(1.0);
                let start_value: f64 = start.parse().unwrap_or(0.0);
                let descending = self.rng.chance(25);
                let end = if descending {
                    start_value - step_value * count as f64
                } else {
                    start_value + step_value * count as f64
                };
                let end = format!("{end:.3}");
                let start = V::f(if start.contains(['.', 'e']) {
                    start
                } else {
                    "0.0"
                });
                let start = self.put(start);
                let end = self.put(V::Float(end));
                if self.rng.chance(25) {
                    format!("{{{{ ({start}..{end})|join(',') }}}}")
                } else {
                    format!("{{{{ range({start}, {end}, {step})|join(',') }}}}")
                }
            }
            // floats as array keys (PHP converts them to ints, Twig's getAttribute() casts them)
            12 => {
                let a = self.float_value();
                let a = self.put(a);
                let b = self.float_value();
                let b = self.put(b);
                match self.rng.below(4) {
                    0 => format!("{{{{ {{({a}): 'x', ({b}): 'y'}}|keys|join(',') }}}}"),
                    1 => format!("{{{{ {{({a}): 1}}|json_encode|raw }}}}"),
                    2 => format!("{{{{ [10, 20, 30][{a}] ?? 'none' }}}}"),
                    _ => format!("{{{{ attribute({{(1): 'one', (-1): 'minus'}}, {a}) }}}}"),
                }
            }
            // float conversions of sprintf
            _ => {
                let value = self.float_value();
                let value = self.put(value);
                let precision = self.rng.range(0, 20);
                let conversion = char::from(*self.rng.pick(b"eEfFgGhH"));
                let format = self.put(V::Str(
                    format!("%.{precision}{conversion}|%{conversion}").into_bytes(),
                ));
                format!("{{{{ {format}|format({value}, {value}) }}}}")
            }
        }
    }

    fn float_value(&mut self) -> V {
        match self.rng.weighted(&[40, 35, 10, 10, 5]) {
            0 => V::f(self.pick_static(FLOATS)),
            1 => V::Float(self.random_float()),
            2 => V::Int(*self.rng.pick(INTS)),
            3 => match self.rng.below(6) {
                0 => V::php_twig("NAN", "constant('NAN')"),
                1 => V::php_twig("INF", "constant('INF')"),
                2 => V::php_twig("-INF", "(-constant('INF'))"),
                3 => V::php_twig("M_PI", "constant('M_PI')"),
                4 => V::php_twig("PHP_FLOAT_MIN", "constant('PHP_FLOAT_MIN')"),
                _ => V::php_twig("PHP_FLOAT_MAX", "constant('PHP_FLOAT_MAX')"),
            },
            _ => V::Str(self.random_float().into_bytes()),
        }
    }

    /// An operand that `+`, `-`, `*`, `/`, `%`, `**` and `//` accept without a diagnostic.
    fn arith_operand(&mut self) -> String {
        let value = match self.rng.weighted(&[40, 30, 15, 15]) {
            0 => V::f(self.pick_static(FLOATS)),
            1 => V::Float(self.random_float()),
            2 => V::Int(*self.rng.pick(INTS)),
            _ => V::Int(self.rng.range(-100, 100)),
        };
        self.put(value)
    }

    /// A number literal in the template (underscores, exponents with and without a sign).
    fn number_literal(&mut self) -> String {
        let int = if self.rng.chance(15) {
            // past PHP_INT_MAX: the lexer makes a float
            let mut digits = String::from("9");
            for _ in 0..18 + self.rng.below(8) {
                digits.push(char::from(b'0' + self.rng.below(10) as u8));
            }
            digits
        } else {
            self.rng.range(0, 1_000_000_000).to_string()
        };
        let mut out = String::new();
        // underscores between digits
        for (i, c) in int.chars().enumerate() {
            if i > 0 && self.rng.chance(15) {
                out.push('_');
            }
            out.push(c);
        }
        if self.rng.chance(60) {
            out.push('.');
            for _ in 0..1 + self.rng.below(10) {
                out.push(char::from(b'0' + self.rng.below(10) as u8));
            }
        }
        if self.rng.chance(50) {
            out.push(*self.rng.pick(&['e', 'E']));
            out.push_str(self.pick_static(&["", "+", "-"]));
            let _ = write!(out, "{}", self.rng.range(0, 330));
        }
        out
    }
}

/// `u` and `v` (microseconds and milliseconds) removed from a format, keeping escaped ones: a
/// backslash escapes the next byte, as in `date_format()` (PHPSRC/ext/date/php_date.c).
fn strip_micro(format: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(format.len());
    let mut bytes = format.iter();
    while let Some(&b) = bytes.next() {
        match b {
            b'\\' => {
                out.push(b);
                if let Some(&next) = bytes.next() {
                    out.push(next);
                }
            }
            b'u' | b'v' => {}
            b => out.push(b),
        }
    }
    out
}

/// The array key PHP makes of `key` (numeric strings become integers).
fn normalize_key(key: &Key) -> Key {
    match key {
        Key::Str(s) => match s.parse::<i64>() {
            Ok(i) if i.to_string() == *s => Key::Int(i),
            _ => key.clone(),
        },
        Key::Int(_) => key.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kit::case::Case;

    #[test]
    fn cases_are_reproducible_and_parse() {
        let a = generate(7, 300);
        let b = generate(7, 300);
        assert_eq!(a, b);
        let mut ids = std::collections::BTreeSet::new();
        for json in &a {
            let case = Case::from_json(json).expect("a twig case");
            assert!(case.unsupported_reason().is_none(), "{json:?}");
            assert!(ids.insert(case.id.clone()), "duplicate id {}", case.id);
        }
        // case n does not depend on the batch size
        assert_eq!(generate(7, 5)[3], a[3]);
    }

    #[test]
    fn every_kind_generates() {
        for kind in Kind::ALL {
            let cases = generate_kinds(1, 50, &[kind]);
            assert_eq!(cases.len(), 50);
            assert_eq!(Kind::from_name(kind.name()), Some(kind));
        }
    }

    #[test]
    fn literals() {
        assert_eq!(twig_str(b"a'b\\c\xff\n"), "'a\\'b\\\\c\\xff\\x0a'");
        assert_eq!(php_str(b"a\"$\\\xff"), "\"a\\\"\\$\\\\\\xff\"");
        assert_eq!(twig_int(-5), "(-5)");
        assert_eq!(twig_int(i64::MIN), "(-9223372036854775807 - 1)");
        assert_eq!(V::f("-0.0").twig().as_deref(), Some("(-0.0)"));
        assert_eq!(strip_micro(b"Y-m-d u \\u v"), b"Y-m-d  \\u ");
        // a backslash escapes a backslash, so the `u` after it is a format character
        assert_eq!(strip_micro(b"\\\\u\\"), b"\\\\\\");
    }
}
