//! Oracle vectors for `twig_value::builtins::string::charset_basic`, the clean-room `iconv()`: PHP 8.5.13 on glibc
//! 2.41 (`LC_CTYPE=C.UTF-8`) in `tests/vectors/string/charset_basic/*.json`, generated black-box by the PHP scripts
//! next to them (the case format is described in `common.inc.php`).
//!
//! Regenerate with the Docker oracle (`research/php-oracle`, image `twig-oracle:3.31.1`):
//!
//! ```text
//! cd crates/twig-value/tests/vectors
//! for t in pairs pairs_ignore names long sweep codepoints fuzz php codepages codepages_cases codepages_fuzz \
//!          multibytes multibytes_cases multibytes_fuzz; do
//!   MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//!       php /t/string/charset_basic/$t.gen.php > string/charset_basic/$t.json
//! done
//! ```
//!
//! The `codepages*` files cover the single-byte charsets with tables (`codepage.rs`): every byte and every code
//! point of each (`codepages.json`), conversions between them and everything else, the characters CP1255 and CP1258
//! combine and hold back, long inputs (`codepages_cases.json`) and random conversions (`codepages_fuzz.json`).
//!
//! The `multibytes*` files cover the multi-byte charsets (`multibyte.rs`) the same way: every byte sequence their
//! structure allows (also how many bytes //IGNORE skips and whether glibc reports it) and every code point of each
//! (`multibytes.json`); conversions between them and everything else, their irregular sequences, BIG5-HKSCS's pairs
//! and held characters, long inputs (`multibytes_cases.json`) and random conversions (`multibytes_fuzz.json`).
//!
//! The tables the module embeds have their own generators in `src/builtins/string/charset_basic/`; `probe.c` next
//! to the vectors is the black-box `iconv(3)` tracer used while modelling glibc's calls (no vector depends on it).
//!
//! A charset glibc knows but twig-rs does not convert must give
//! `Error: iconv(): conversion of charset "X" is not supported by twig-rs` where PHP converts (or fails on the
//! input); everything else must match PHP byte for byte, diagnostics included.

mod support;

use serde_json::Value as Json;
use support::values::{res_json, typed_value};
use support::{Mismatches, diag_list, hex, json_diags, load, s, unhex};
use twig_value::builtins::string::charset_basic::{iconv, iconv_bytes};
use twig_value::{Diagnostic, Level, PhpError, Value};

/// A vector file in the format of `common.inc.php`.
struct Vectors {
    names: Vec<Vec<u8>>,
    messages: Vec<Vec<u8>>,
    json: Json,
}

impl Vectors {
    fn load(file: &str) -> Self {
        let json = load(&format!("string/charset_basic/{file}"));
        let table = |key: &str| -> Vec<Vec<u8>> {
            json[key]
                .as_array()
                .unwrap_or_else(|| panic!("{file}: {key}"))
                .iter()
                .map(|h| unhex(s(h)))
                .collect()
        };
        Vectors {
            names: table("names"),
            messages: table("messages"),
            json,
        }
    }

    fn cases(&self) -> &[Json] {
        self.json["cases"].as_array().expect("cases")
    }

    fn name(&self, j: &Json) -> &[u8] {
        &self.names[j.as_u64().expect("name index") as usize]
    }

    /// The expected outcome of a case, messages resolved.
    fn expected(&self, j: &Json) -> Outcome {
        match j {
            Json::String(h) => Outcome::Converted(h.clone()),
            Json::Array(a) if a.len() == 2 && a[0].is_u64() => {
                Outcome::Converted(format!("{}:{}", a[0], s(&a[1])))
            }
            Json::Array(a) if a.len() == 2 => Outcome::False(
                s(&a[0]).to_string(),
                hex(&self.messages[a[1].as_u64().expect("message index") as usize]),
            ),
            Json::Array(a) if a.len() == 3 => Outcome::Exception(
                s(&a[1]).to_string(),
                hex(&self.messages[a[2].as_u64().expect("message index") as usize]),
            ),
            other => panic!("bad result {other}"),
        }
    }
}

/// What one conversion gave.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Outcome {
    /// The output in hex, or `length:fnv1a64` for long outputs.
    Converted(String),
    /// PHP's `false` after one diagnostic: level (`W`, `N`) and hex message.
    False(String, String),
    /// An exception: class and hex message.
    Exception(String, String),
    /// twig-rs' `Error` for a charset it does not convert (hex message).
    Unsupported(String),
    /// Anything else (a broken invariant).
    Other(String),
}

/// FNV-1a 64 as 16 hex digits (`hash('fnv1a64', ...)`), for long outputs.
fn fnv1a64(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

fn level(d: &Diagnostic) -> &'static str {
    match d.level {
        Level::Warning => "W",
        Level::Notice => "N",
        Level::Deprecated => "D",
        Level::UserDeprecated => "UD",
    }
}

/// `iconv_bytes()` with its outcome in the vector encoding.
fn convert(from: &[u8], to: &[u8], input: &[u8], long: bool) -> Outcome {
    let mut d: Vec<Diagnostic> = Vec::new();
    let r = iconv_bytes(from, to, input, &mut d);
    outcome(r, &d, long)
}

fn outcome(
    r: Result<Option<twig_value::PhpString>, PhpError>,
    d: &[Diagnostic],
    long: bool,
) -> Outcome {
    match (r, d) {
        (Ok(Some(out)), []) if long => {
            Outcome::Converted(format!("{}:{}", out.len(), fnv1a64(out.as_bytes())))
        }
        (Ok(Some(out)), []) => Outcome::Converted(hex(out.as_bytes())),
        (Ok(None), [one]) => Outcome::False(level(one).to_string(), hex(one.message.as_bytes())),
        (Err(e), [])
            if e.message()
                .as_bytes()
                .ends_with(b"\" is not supported by twig-rs") =>
        {
            Outcome::Unsupported(hex(e.message().as_bytes()))
        }
        (Err(e), []) => {
            Outcome::Exception(e.class().name().to_string(), hex(e.message().as_bytes()))
        }
        (r, d) => Outcome::Other(format!("{r:?} {d:?}")),
    }
}

/// Builds the input of a case: hex, or a recipe of segments (see `recipe()` in `common.inc.php`).
fn input(j: &Json) -> Vec<u8> {
    match j {
        Json::String(h) => unhex(h),
        Json::Array(segments) => {
            let mut out = Vec::new();
            for seg in segments {
                match s(&seg[0]) {
                    "hex" => out.extend(unhex(s(&seg[1]))),
                    "rep" => {
                        let unit = unhex(s(&seg[1]));
                        for _ in 0..seg[2].as_u64().expect("count") {
                            out.extend_from_slice(&unit);
                        }
                    }
                    other => panic!("bad segment {other}"),
                }
            }
            out
        }
        other => panic!("bad input {other}"),
    }
}

/// The text of a charset name for messages.
fn show(name: &[u8]) -> String {
    format!("{:?}", String::from_utf8_lossy(name))
}

/// Runs a file of conversions. `allow_unsupported`: a known-but-unsupported charset may give twig-rs' `Error` where
/// PHP did not report an unknown charset. Returns how many cases did.
fn run_conversions(file: &'static str, allow_unsupported: bool) -> usize {
    let v = Vectors::load(file);
    let mut m = Mismatches::new(file);
    let mut unsupported = 0;
    for case in v.cases() {
        let (from, to) = (v.name(&case[0]), v.name(&case[1]));
        let data = input(&case[2]);
        let long = case[3]
            .as_array()
            .is_some_and(|a| a.len() == 2 && a[0].is_u64());
        let expected = v.expected(&case[3]);
        let got = convert(from, to, &data, long);
        let what = || {
            let mut shown = hex(&data);
            shown.truncate(120);
            format!("iconv({}, {}, {shown})", show(from), show(to))
        };
        if let Outcome::Unsupported(message) = &got {
            unsupported += 1;
            let php_unknown = matches!(&expected, Outcome::False(_, m) if String::from_utf8_lossy(&unhex(m)).contains("Wrong encoding"));
            m.check(
                what,
                (allow_unsupported, php_unknown, message.is_empty()),
                (true, false, false),
            );
            continue;
        }
        m.check(what, got, expected);
    }
    m.finish();
    unsupported
}

#[test]
fn pairs() {
    assert_eq!(run_conversions("pairs.json", false), 0);
}

#[test]
fn pairs_ignore() {
    assert_eq!(run_conversions("pairs_ignore.json", false), 0);
}

#[test]
fn names() {
    let unsupported = run_conversions("names.json", true);
    assert!(
        unsupported > 1000,
        "{unsupported} cases use a charset twig-rs does not convert"
    );
}

#[test]
fn long() {
    assert_eq!(run_conversions("long.json", false), 0);
}

/// The token of one conversion in the sweeps (see `sweep.gen.php`).
fn token(from: &[u8], to: &[u8], input: &[u8]) -> Vec<u8> {
    let mut d: Vec<Diagnostic> = Vec::new();
    let r = iconv_bytes(from, to, input, &mut d);
    let mut t = Vec::new();
    match (r, d.as_slice()) {
        (Ok(Some(out)), []) => {
            t.extend_from_slice(format!("S{}:", out.len()).as_bytes());
            t.extend_from_slice(out.as_bytes());
        }
        (Ok(None), [one]) => {
            t.extend_from_slice(format!("{}{}:", level(one), one.message.len()).as_bytes());
            t.extend_from_slice(one.message.as_bytes());
        }
        (Err(e), []) => {
            t.extend_from_slice(format!("E{}:", e.class().name()).as_bytes());
            t.extend_from_slice(e.message().as_bytes());
            t.push(b'\n');
        }
        (r, d) => panic!("unexpected outcome {r:?} {d:?}"),
    }
    t
}

/// xorshift32, as in `sweep.gen.php`.
fn xorshift(x: &mut u32) -> u32 {
    *x ^= *x << 13;
    *x ^= *x >> 17;
    *x ^= *x << 5;
    *x
}

/// The 32-bit values of the `values32*` sweeps.
fn values() -> Vec<u32> {
    let mut v = Vec::new();
    for (lo, hi) in [
        (0, 0x100),
        (0xD7F0, 0xE010),
        (0xFFF0, 0x1_0010),
        (0x10_FFF0, 0x11_0010),
        (0x1F_FFF0, 0x20_0010),
        (0x3FF_FFF0, 0x400_0010),
        (0x7FFF_FFF0, 0x8000_0010),
        (0xFFFF_FFF0, 0xFFFF_FFFF),
    ] {
        v.extend(lo..=hi);
    }
    let mut x = 2_463_534_242u32;
    for i in 0..6000 {
        let r = xorshift(&mut x);
        v.push(match i % 4 {
            0 | 1 => r % 0x11_0000,
            2 => r & 0x7FFF_FFFF,
            _ => r,
        });
    }
    v
}

/// The inputs of a sweep kind, in blocks (see `inputs()` in `sweep.gen.php`).
fn sweep_inputs(kind: &str) -> Vec<Vec<Vec<u8>>> {
    let mut blocks = Vec::new();
    match kind {
        "bytes12" => {
            blocks.push((0..=255u8).map(|b| vec![b]).collect());
            for b0 in 0..=255u8 {
                blocks.push((0..=255u8).map(|b1| vec![b0, b1]).collect());
            }
        }
        "utf8x3" => {
            for b0 in 0xC0..=0xFFu8 {
                let mut block = Vec::new();
                for b1 in 0..=255u8 {
                    for b2 in [
                        0x00, 0x41, 0x7F, 0x80, 0x8F, 0x90, 0x9F, 0xA0, 0xBF, 0xC0, 0xC3, 0xE0,
                        0xF0, 0xFF,
                    ] {
                        block.push(vec![b0, b1, b2]);
                    }
                }
                blocks.push(block);
            }
        }
        "pairs16be" | "pairs16le" => {
            let be = kind == "pairs16be";
            let unit = |u: u16| if be { u.to_be_bytes() } else { u.to_le_bytes() };
            for hi in 0xD8..=0xDFu16 {
                let mut block = Vec::new();
                for lo in 0..=255u16 {
                    for u2 in [
                        0x0000, 0x0061, 0xD7FF, 0xD800, 0xDBFF, 0xDC00, 0xDFFF, 0xE000, 0xFEFF,
                        0xFFFE, 0xFFFF,
                    ] {
                        let mut input = unit((hi << 8) | lo).to_vec();
                        input.extend(unit(u2));
                        block.push(input);
                    }
                }
                blocks.push(block);
            }
        }
        "values32be" | "values32le" => {
            let be = kind == "values32be";
            for chunk in values().chunks(256) {
                blocks.push(
                    chunk
                        .iter()
                        .map(|&c| {
                            if be {
                                c.to_be_bytes().to_vec()
                            } else {
                                c.to_le_bytes().to_vec()
                            }
                        })
                        .collect(),
                );
            }
        }
        other => panic!("unknown sweep kind {other}"),
    }
    blocks
}

#[test]
fn sweep() {
    let v = load("string/charset_basic/sweep.json");
    let mut m = Mismatches::new("sweep.json");
    for sweep in v["sweeps"].as_array().expect("sweeps") {
        let (from, to, kind) = (s(&sweep["from"]), s(&sweep["to"]), s(&sweep["kind"]));
        let blocks = sweep_inputs(kind);
        let expected = sweep["blocks"].as_array().expect("blocks");
        assert_eq!(blocks.len(), expected.len(), "{from} -> {to} {kind}");
        for (i, (block, want)) in blocks.iter().zip(expected).enumerate() {
            let mut t = Vec::new();
            for input in block {
                t.extend(token(from.as_bytes(), to.as_bytes(), input));
            }
            m.check(
                || format!("{from} -> {to} {kind} block {i}"),
                fnv1a64(&t),
                s(want).to_string(),
            );
        }
    }
    m.finish();
}

/// The code points of a `codepoints.json` kind (see `code_points()` in `codepoints.gen.php`).
fn code_points(kind: &str) -> Vec<u32> {
    if kind == "bmp" {
        return (0..=0xFFFF).collect();
    }
    let mut v: Vec<u32> = (0x1_0000..=0x2_FFFF).collect();
    v.extend((0x3_0000..=0x10_FFFF).step_by(31));
    v.extend(0xE_0000..=0xE_01FF);
    v.extend(0x2_F800..=0x2_FA1F);
    v.extend(0x11_0000..=0x11_00FF);
    v
}

#[test]
fn codepoints() {
    let v = load("string/charset_basic/codepoints.json");
    let mut m = Mismatches::new("codepoints.json");
    for set in v["sets"].as_array().expect("sets") {
        let (to, kind) = (s(&set["to"]), s(&set["kind"]));
        let points = code_points(kind);
        let expected = set["blocks"].as_array().expect("blocks");
        assert_eq!(points.chunks(256).len(), expected.len(), "{to} {kind}");
        for (i, (block, want)) in points.chunks(256).zip(expected).enumerate() {
            let mut t = Vec::new();
            for &c in block {
                t.extend(token(b"UCS-4", to.as_bytes(), &c.to_be_bytes()));
            }
            m.check(
                || format!("UCS-4 -> {to} {kind} block {i} (from U+{:04X})", block[0]),
                fnv1a64(&t),
                s(want).to_string(),
            );
        }
    }
    m.finish();
}

#[test]
fn php_arguments() {
    let v = load("string/charset_basic/php.json");
    let mut m = Mismatches::new("php.json");
    for case in v["cases"].as_array().expect("cases") {
        let args: Vec<Value> = case["a"]
            .as_array()
            .expect("args")
            .iter()
            .map(typed_value)
            .collect();
        let mut d: Vec<Diagnostic> = Vec::new();
        let r = iconv(&args[0], &args[1], &args[2], &mut d)
            .map(|r| r.map_or(Value::Bool(false), Value::String));
        let expected_unsupported = r
            .as_ref()
            .is_err_and(|e| e.message_lossy().ends_with("is not supported by twig-rs"));
        if expected_unsupported {
            // twig-rs' Error where PHP knew the charset: PHP did not warn about an unknown name
            let php_unknown = case["d"].to_string().contains(&hex(b"Wrong encoding"));
            m.check(|| format!("iconv({})", case["a"]), php_unknown, false);
            continue;
        }
        m.check(
            || format!("iconv({})", case["a"]),
            (res_json(&r), diag_list(&d)),
            (case["r"].clone(), json_diags(&case["d"])),
        );
    }
    m.finish();
}

#[test]
fn fuzz() {
    assert_eq!(run_conversions("fuzz.json", false), 0);
}

/// The inputs of a `codepages.json` kind, in blocks (see `codepage_inputs()` in `codepages.gen.php`).
fn codepage_inputs(kind: &str) -> Vec<Vec<Vec<u8>>> {
    let mut blocks = Vec::new();
    match kind {
        "bytes1" => blocks.push((0..=255u8).map(|b| vec![b]).collect()),
        "strings" => {
            let up: Vec<u8> = (0..=255u8).collect();
            let down: Vec<u8> = up.iter().rev().copied().collect();
            let mut interleaved = Vec::new();
            for b in 0x80..=0xFFu8 {
                interleaved.extend([b, b ^ 0xC0, b'a']);
            }
            blocks.push(vec![up, down, interleaved]);
        }
        "bytes12" => blocks = sweep_inputs("bytes12"),
        "bmp" => {
            for chunk in (0..=0xFFFFu32).collect::<Vec<_>>().chunks(256) {
                blocks.push(chunk.iter().map(|c| c.to_be_bytes().to_vec()).collect());
            }
        }
        "translit" | "ignore" => {
            for start in (0..0x11_0000u32).step_by(4096) {
                let mut input = Vec::new();
                for c in start..start + 4096 {
                    if !(0xD800..=0xDFFF).contains(&c) {
                        input.extend(c.to_be_bytes());
                        if kind == "translit" {
                            input.extend(0x0Au32.to_be_bytes());
                        }
                    }
                }
                blocks.push(vec![input]);
            }
        }
        other => panic!("unknown codepage kind {other}"),
    }
    blocks
}

/// Runs the sets of `codepages.json` whose kind is one of `kinds`: of those, the ones whose index modulo `groups` is
/// `group` (the bigger kinds are split across tests, which run in parallel).
fn codepage_sets(kinds: &[&str], group: usize, groups: usize) {
    let v = load("string/charset_basic/codepages.json");
    let mut m = Mismatches::new("codepages.json");
    let mut sets = 0;
    let selected = v["sets"]
        .as_array()
        .expect("sets")
        .iter()
        .filter(|set| kinds.contains(&s(&set["kind"])))
        .enumerate()
        .filter(|(i, _)| i % groups == group);
    for (_, set) in selected {
        let (from, to, kind) = (s(&set["from"]), s(&set["to"]), s(&set["kind"]));
        sets += 1;
        let blocks = codepage_inputs(kind);
        let expected = set["blocks"].as_array().expect("blocks");
        assert_eq!(blocks.len(), expected.len(), "{from} -> {to} {kind}");
        for (i, (block, want)) in blocks.iter().zip(expected).enumerate() {
            let mut t = Vec::new();
            for input in block {
                t.extend(token(from.as_bytes(), to.as_bytes(), input));
            }
            m.check(
                || format!("{from} -> {to} {kind} block {i}"),
                fnv1a64(&t),
                s(want).to_string(),
            );
        }
    }
    assert!(sets > 0, "no sets of kinds {kinds:?}");
    m.finish();
}

#[test]
fn codepages_decode() {
    codepage_sets(&["bytes1", "strings", "bytes12"], 0, 1);
}

#[test]
fn codepages_bmp_0() {
    codepage_sets(&["bmp"], 0, 2);
}

#[test]
fn codepages_bmp_1() {
    codepage_sets(&["bmp"], 1, 2);
}

#[test]
fn codepages_translit_0() {
    codepage_sets(&["translit"], 0, 4);
}

#[test]
fn codepages_translit_1() {
    codepage_sets(&["translit"], 1, 4);
}

#[test]
fn codepages_translit_2() {
    codepage_sets(&["translit"], 2, 4);
}

#[test]
fn codepages_translit_3() {
    codepage_sets(&["translit"], 3, 4);
}

#[test]
fn codepages_ignore_0() {
    codepage_sets(&["ignore"], 0, 2);
}

#[test]
fn codepages_ignore_1() {
    codepage_sets(&["ignore"], 1, 2);
}

#[test]
fn codepages_cases() {
    assert_eq!(run_conversions("codepages_cases.json", false), 0);
}

#[test]
fn codepages_fuzz() {
    assert_eq!(run_conversions("codepages_fuzz.json", false), 0);
}

/// The follower of the `follow` and `bytes3` kinds of `multibytes.json`.
const FOLLOWER: &[u8] = b"\xa1\x41!";

/// The inputs of a `multibytes.json` kind, in blocks (see `multibyte_inputs()` in `multibytes.gen.php`).
fn multibyte_inputs(kind: &str) -> Vec<Vec<Vec<u8>>> {
    let mut blocks = Vec::new();
    match kind {
        "bytes1" | "bmp" | "translit" | "ignore" => blocks = codepage_inputs(kind),
        "bytes12" => blocks = sweep_inputs("bytes12"),
        "follow" => {
            for b0 in 0..=255u8 {
                blocks.push(
                    (0..=255u8)
                        .map(|b1| [&[b0, b1][..], FOLLOWER].concat())
                        .collect(),
                );
            }
        }
        "bytes3" => {
            for b1 in 0..=255u8 {
                let mut block = Vec::new();
                for b2 in 0..=255u8 {
                    block.push(vec![0x8F, b1, b2]);
                    block.push([&[0x8F, b1, b2][..], FOLLOWER].concat());
                }
                blocks.push(block);
            }
        }
        "bytes4" => {
            for p in 0xA0..=0xB1u8 {
                for b1 in 0xA0..=0xFFu8 {
                    blocks.push((0..=255u8).map(|b2| vec![0x8E, p, b1, b2]).collect());
                }
            }
        }
        "gb4" => {
            for b0 in [0x81, 0x82, 0x84, 0x85, 0x8F, 0x90, 0x99, 0xE3, 0xE4, 0xFE] {
                for b1 in 0x30..=0x39u8 {
                    let mut block = Vec::new();
                    for b2 in 0..=255u8 {
                        for b3 in [
                            0x2F, 0x30, 0x35, 0x39, 0x3A, 0x40, 0x7F, 0x80, 0x81, 0xFE, 0xFF,
                        ] {
                            block.push(vec![b0, b1, b2, b3]);
                        }
                    }
                    blocks.push(block);
                }
            }
        }
        "gb4all" => {
            for b0 in (0x81..=0x84u8).chain(0x90..=0xE3) {
                for b1 in 0x30..=0x39u8 {
                    let mut block = Vec::new();
                    for b2 in (0x81..=0xFEu8).step_by(if b0 >= 0x90 { 5 } else { 1 }) {
                        for b3 in 0x30..=0x39u8 {
                            block.push(vec![b0, b1, b2, b3]);
                        }
                    }
                    blocks.push(block);
                }
            }
        }
        "astral" => {
            let mut cps: Vec<u32> = (0x1_0000..=0x2_FFFF).collect();
            cps.extend((0x3_0000..=0x10_FFFF).step_by(31));
            cps.extend(0x11_0000..=0x11_00FF);
            for chunk in cps.chunks(256) {
                blocks.push(chunk.iter().map(|c| c.to_be_bytes().to_vec()).collect());
            }
        }
        other => panic!("unknown multibyte kind {other}"),
    }
    blocks
}

/// Runs the sets of `multibytes.json` whose kind is one of `kinds`: of those, the ones whose index modulo `groups` is
/// `group`.
fn multibyte_sets(kinds: &[&str], group: usize, groups: usize) {
    let v = load("string/charset_basic/multibytes.json");
    let mut m = Mismatches::new("multibytes.json");
    let mut sets = 0;
    let selected = v["sets"]
        .as_array()
        .expect("sets")
        .iter()
        .filter(|set| kinds.contains(&s(&set["kind"])))
        .enumerate()
        .filter(|(i, _)| i % groups == group);
    for (_, set) in selected {
        let (from, to, kind) = (s(&set["from"]), s(&set["to"]), s(&set["kind"]));
        sets += 1;
        let blocks = multibyte_inputs(kind);
        let expected = set["blocks"].as_array().expect("blocks");
        assert_eq!(blocks.len(), expected.len(), "{from} -> {to} {kind}");
        for (i, (block, want)) in blocks.iter().zip(expected).enumerate() {
            let mut t = Vec::new();
            for input in block {
                t.extend(token(from.as_bytes(), to.as_bytes(), input));
            }
            m.check(
                || format!("{from} -> {to} {kind} block {i}"),
                fnv1a64(&t),
                s(want).to_string(),
            );
        }
    }
    assert!(sets > 0, "no sets of kinds {kinds:?}");
    m.finish();
}

#[test]
fn multibytes_decode() {
    multibyte_sets(&["bytes1", "bytes12"], 0, 1);
}

#[test]
fn multibytes_follow() {
    multibyte_sets(&["follow"], 0, 1);
}

#[test]
fn multibytes_long_sequences() {
    multibyte_sets(&["bytes3", "bytes4", "gb4", "gb4all"], 0, 1);
}

#[test]
fn multibytes_bmp() {
    multibyte_sets(&["bmp", "astral"], 0, 1);
}

#[test]
fn multibytes_translit_0() {
    multibyte_sets(&["translit"], 0, 2);
}

#[test]
fn multibytes_translit_1() {
    multibyte_sets(&["translit"], 1, 2);
}

#[test]
fn multibytes_ignore() {
    multibyte_sets(&["ignore"], 0, 1);
}

#[test]
fn multibytes_cases() {
    // the conversions between GB 2312 and GBK or BIG5, which glibc converts with modules of their own
    assert_eq!(run_conversions("multibytes_cases.json", true), 84);
}

#[test]
fn multibytes_fuzz() {
    assert_eq!(run_conversions("multibytes_fuzz.json", false), 0);
}
