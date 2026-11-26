//! Cases for the "filters, functions and tests" family of differential fuzzing (roadmap phase 5).
//!
//! Every case is an oracle "twig" case (`"env": "plain"`) whose template calls Twig's core
//! callables (`reference/twig/core-extension.md`): the filters of `CoreExtension` and
//! `EscaperExtension`, the functions of `CoreExtension`, `DebugExtension` (`dump`) and
//! `StringLoaderExtension` (`template_from_string`), every core test, and `has some`/`has every`
//! (they call arrows like the filters do). One to three statements print their results through
//! plain `{{ }}`, `dump()` (exact PHP types), `json_encode`, a `for` loop, `{% apply %}` or a test.
//!
//! A case follows one profile, named by the last letter of its id (`c<seed>-<index><profile>`):
//!
//! | profile | share | arguments |
//! |---|---|---|
//! | `m`ixed | 45% | anything: wrong types, nested calls, named/spread/missing/extra arguments, misspelled callable names |
//! | `v`alid | 40% | well-typed values from per-parameter pools, positional or correctly named: results are compared |
//! | `f`ocus | 15% | like `valid`, one callable called by two to four statements |
//!
//! Arguments come from a pool per parameter ("meaningful" values: strategies, charsets, date
//! formats, sprintf formats, trim masks, ...) mixed with values of any type: `null`, booleans,
//! ints near the int64 limits, `-0.0`, `INF`, `NAN`, numeric and leading-numeric strings,
//! unicode that case mapping treats specially, invalid UTF-8, sequences, mappings, rows, nested
//! calls, `Twig\Markup` (variables, macro results, `include()`), `Traversable` objects
//! (`ArrayIterator`, `ArrayObject`, the iterators of Twig's `tests/IntegrationTest.php`),
//! `Stringable` and `Countable` objects, dates, intervals, timezones and enums. Argument lists are
//! positional, named (correct, camelCase, unknown and duplicate names, positional after named),
//! spread (`...[a, b]`, `...{name: v}`, `...var`), short of a required argument or with extra
//! arguments.
//!
//! The context holds only the variables the template uses: `data_json` when JSON can express them,
//! `data_php` otherwise, with `"test_classes": true` when a helper class of `IntegrationTest.php`
//! is used. Settings vary too: autoescape strategies, `strict_variables`, `debug`, charsets,
//! `"errors": "throw"`, the PHP default timezone and, rarely, a global sandbox.
//!
//! What cannot match by design is left out (`reference/deviations.md`): unseeded randomness
//! (`random()` and `shuffle` only get inputs whose result does not depend on the generator),
//! microsecond formats of the current time, string callables that name PHP functions, sizes that
//! reach a memory limit (`number_format()` with a huge `decimals` allocates past PHP's hard memory
//! limit and kills the oracle process with its whole batch), `dump()` of `TemplateWrapper`s and
//! closures, and `dump()` under strategies that escape its object handles. Each main template ends
//! with a comment naming its case: PHP compiles an identical source only once per process, and a
//! second copy would not raise the compile-time deprecations again.
//!
//! [`generate`] is deterministic: case `i` of seed `s` depends only on `s` and `i`.

use std::collections::BTreeSet;

use crate::kit::json::Json;

/// The family name (case ids, `tests/vectors/fuzz/callables/`).
pub const FAMILY: &str = "callables";

/// The cases `0..count` of `seed`.
pub fn generate(seed: u64, count: usize) -> Vec<Json> {
    (0..count).map(|index| generate_case(seed, index)).collect()
}

/// Case `index` of `seed`.
pub fn generate_case(seed: u64, index: usize) -> Json {
    let mut rng = Rng::new(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (index as u64));
    Generator::new(&mut rng).case(&format!("c{seed}-{index:05}"))
}

// ---- random numbers ----------------------------------------------------------------------------

/// SplitMix64: small, fast and the same on every platform.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Rng {
        let mut rng = Rng(seed ^ 0xD1B5_4A32_D192_ED03);
        rng.next();
        rng
    }

    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number in `0..n` (`0` when `n` is 0).
    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }

    /// True with `percent` % probability.
    fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }

    fn weighted<T: Copy>(&mut self, items: &[(u32, T)]) -> T {
        let total: u64 = items.iter().map(|(w, _)| u64::from(*w)).sum();
        let mut x = self.next() % total.max(1);
        for (weight, item) in items {
            let weight = u64::from(*weight);
            if x < weight {
                return *item;
            }
            x -= weight;
        }
        items[items.len() - 1].1
    }
}

// ---- the variables a template can use ------------------------------------------------------------

const NULLISH: u32 = 1;
const BOOL: u32 = 1 << 1;
const INT: u32 = 1 << 2;
const FLOAT: u32 = 1 << 3;
const NUMSTR: u32 = 1 << 4;
const STR: u32 = 1 << 5;
const HTML: u32 = 1 << 6;
const NL: u32 = 1 << 7;
const SEQ: u32 = 1 << 8;
const MAP: u32 = 1 << 9;
const TRAV: u32 = 1 << 10;
const ROWS: u32 = 1 << 11;
const OBJ: u32 = 1 << 12;
const MARKUP: u32 = 1 << 13;
const DATE: u32 = 1 << 14;
const INTERVAL: u32 = 1 << 15;
const TZ: u32 = 1 << 16;
/// Needs `"test_classes": true` (a helper class of Twig's `tests/IntegrationTest.php`).
const TCLS: u32 = 1 << 17;
/// Never dumped and never picked as an arbitrary value (`TemplateWrapper`: its dump is the whole
/// environment).
const NODUMP: u32 = 1 << 18;
const ENUM: u32 = 1 << 19;
/// Not valid UTF-8.
const BAD: u32 = 1 << 20;
const FMT: u32 = 1 << 21;

const NUM: u32 = INT | FLOAT | NUMSTR;
const ITER: u32 = SEQ | MAP | TRAV;
const ANYVAR: u32 = !NODUMP;

/// A context variable: JSON text (`data_json`, also turned into PHP for `data_php`) or PHP code.
struct Var {
    name: &'static str,
    json: Option<&'static str>,
    php: &'static str,
    tags: u32,
}

const fn j(name: &'static str, json: &'static str, tags: u32) -> Var {
    Var {
        name,
        json: Some(json),
        php: "",
        tags,
    }
}

const fn p(name: &'static str, php: &'static str, tags: u32) -> Var {
    Var {
        name,
        json: None,
        php,
        tags,
    }
}

const VARS: &[Var] = &[
    j("n", "null", NULLISH),
    j("t", "true", BOOL),
    j("f", "false", BOOL),
    j("i0", "0", INT),
    j("i1", "1", INT),
    j("i2", "2", INT),
    j("i3", "3", INT),
    j("i7", "7", INT),
    j("in1", "-1", INT),
    j("in7", "-7", INT),
    j("i255", "255", INT),
    j("imil", "1234567", INT),
    j("ibig", "9223372036854775807", INT),
    j("imin", "-9223372036854775808", INT),
    j("i53", "9007199254740993", INT),
    j("fz", "0.0", FLOAT),
    j("fnz", "-0.0", FLOAT),
    j("fh", "0.5", FLOAT),
    j("f15", "1.5", FLOAT),
    j("f25", "2.5", FLOAT),
    j("fn25", "-2.5", FLOAT),
    j("f1005", "1.005", FLOAT),
    j("fpi", "3.14159265358979", FLOAT),
    j("f3", "3.0", FLOAT),
    j("f1e15", "1.0e15", FLOAT),
    j("f1e20", "1.0e20", FLOAT),
    j("fmax", "1.7976931348623157e308", FLOAT),
    j("fden", "5e-324", FLOAT),
    j("fsm", "1.0e-7", FLOAT),
    j("fneg", "-1234.5678", FLOAT),
    j("finf", "1e400", FLOAT),
    j("fninf", "-1e400", FLOAT),
    p("fnan", "NAN", FLOAT),
    j("s0", r#""""#, STR),
    j("sa", r#""abc""#, STR),
    j("sw", r#""Hello World""#, STR),
    j("spad", r#""  pad me  ""#, STR),
    j("szero", r#""0""#, STR | NUMSTR),
    j("s42", r#""42""#, STR | NUMSTR),
    j("sneg", r#""-17""#, STR | NUMSTR),
    j("sfl", r#""1.5""#, STR | NUMSTR),
    j("sexp", r#""1e3""#, STR | NUMSTR),
    j("slead", r#""12abc""#, STR | NUMSTR),
    j("sws", r#"" 7 ""#, STR | NUMSTR),
    j("shex", r#""0x1A""#, STR),
    j("sutf", r#""été à Paris""#, STR),
    j("ssz", r#""Straße ß""#, STR),
    j("sturk", r#""İstanbul ıi""#, STR),
    j("sdz", r#""ǅemal ǆ ǈ""#, STR),
    j("sgreek", r#""ΣΑΣ ὈΔΥΣΣΕΎΣ σ""#, STR),
    j("scjk", r#""日本語のテキスト""#, STR),
    j("semoji", r#""a😀b👍🏽c""#, STR),
    j("scomb", r#""é ñ""#, STR),
    j("slig", r#""ﬁ ﬂ ŉ ǰ""#, STR),
    j("shtml", r#""<p class=\"x\">Tom & 'Jerry'</p>""#, STR | HTML),
    j(
        "stags",
        r#""<b>bold</b> <i>it</i><br/><!-- c --><script>x</script> a < b""#,
        STR | HTML,
    ),
    j("snl", r#""a\nb\r\nc\rd\n\re""#, STR | NL),
    j("scsv", r#""a,b,,c,""#, STR),
    j(
        "sfmt",
        r#""%s|%d|%05.2f|%x|%'*8s|%-4s|%+d|%e|%b|%c|%%""#,
        STR | FMT,
    ),
    j("sfmt2", r#""%2$s %1$s %2$s""#, STR | FMT),
    j("sdate", r#""2020-02-29 13:45:10""#, STR | DATE),
    j(
        "sdate2",
        r#""2021-03-28 02:30:00 Europe/Paris""#,
        STR | DATE,
    ),
    j("sts", r#""1700000000""#, STR | NUMSTR | DATE),
    j("stsn", r#""-86400""#, STR | NUMSTR | DATE),
    j("stz", r#""Asia/Kathmandu""#, STR),
    j("surl", r#""a b&c=d/é?x#y~z*'()""#, STR),
    j("sjs", r#""</script>\"'\\\n\t ""#, STR),
    j("snul", r#""a\u0000b""#, STR),
    j("sctrl", r#""\u0001\u001f\u007f\u0080\u009f""#, STR),
    j(
        "slong",
        r#""The quick brown fox jumps over the lazy dog""#,
        STR,
    ),
    j("sspace", r#"" \t\n\r\u000b\u000c ""#, STR | NL),
    j("snbsp", r#"" x ""#, STR),
    j("sbom", r#""﻿bom""#, STR),
    p("sbad", r#""a\xffb\xc3""#, STR | BAD),
    p("slat", r#""caf\xe9 cr\xe8me""#, STR | BAD),
    p(
        "sover",
        r#""x\xc0\xafy\xed\xa0\x80z\xf4\x90\x80\x80""#,
        STR | BAD,
    ),
    j("a0", "[]", SEQ),
    j("al", "[1,2,3]", SEQ),
    j("al6", "[5,3,8,1,9,2]", SEQ),
    j("as", r#"["b","a","C","10","9","1e1",""]"#, SEQ),
    j("am", r#"{"a":1,"b":2,"c":3}"#, MAP),
    j("amix", r#"{"0":"z","x":"y","1":"w"}"#, MAP),
    j("aholes", r#"{"0":"a","2":"b","5":"c"}"#, MAP),
    j("aneg", r#"{"-5":"m","3":"x"}"#, MAP),
    j("anest", r#"[[1,2],[3,[4,5]],{"k":"v"}]"#, SEQ),
    j(
        "arows",
        r#"[{"id":1,"name":"ab","v":null},{"id":2,"name":"cd","v":1.5},{"id":"3","v":true}]"#,
        SEQ | ROWS,
    ),
    j("amixed", r#"[1,"1",1.5,true,null,"a",[1],0,""]"#, SEQ),
    j("afl", "[2.5,-1.5,0.1,1e100,-0.0]", SEQ),
    j("astrk", r#"{"10":"ten","9":"nine","a":"A","1.5":"x"}"#, MAP),
    j("abool", r#"[true,false,null,0,"","0",[]]"#, SEQ),
    j(
        "a20",
        "[0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19]",
        SEQ,
    ),
    j("autf", r#"["é","😀","a","Z","ß"]"#, SEQ),
    j("adeep", r#"{"a":{"b":{"c":[1,2]}}}"#, MAP),
    j("astrs", r#"["one","two","three"]"#, SEQ),
    p("abad", r#"['ok', "\xff", 'k' => "\xc3"]"#, MAP | BAD),
    p("ainf", "[INF, -INF, NAN, 0.1]", SEQ),
    p(
        "aobj",
        r#"['o' => (object) ['id' => 1], 'm' => new \Twig\Markup('<i>m</i>', 'UTF-8'), 'ai' => new \ArrayIterator([1, 2])]"#,
        MAP,
    ),
    p(
        "rowso",
        r#"[(object) ['id' => 1, 'name' => 'a'], (object) ['id' => 2, 'name' => 'b'], (object) ['name' => 'c']]"#,
        SEQ | ROWS,
    ),
    p(
        "amarks",
        r#"[new \Twig\Markup('b', 'UTF-8'), new \Twig\Markup('a', 'UTF-8'), 'c']"#,
        SEQ,
    ),
    p(
        "m",
        r#"new \Twig\Markup('<b>bold</b> & co', 'UTF-8')"#,
        OBJ | MARKUP | STR | HTML,
    ),
    p("me", r#"new \Twig\Markup('', 'UTF-8')"#, OBJ | MARKUP | STR),
    p(
        "m0",
        r#"new \Twig\Markup('0', 'UTF-8')"#,
        OBJ | MARKUP | STR,
    ),
    p(
        "mu",
        r#"new \Twig\Markup('été ΣΑΣ', 'UTF-8')"#,
        OBJ | MARKUP | STR,
    ),
    p(
        "ml",
        r#"new \Twig\Markup("  a,b\n ", 'UTF-8')"#,
        OBJ | MARKUP | STR | NL,
    ),
    p(
        "mlat",
        r#"new \Twig\Markup("caf\xe9", 'ISO-8859-1')"#,
        OBJ | MARKUP | STR | BAD,
    ),
    p(
        "ai",
        r#"new \ArrayIterator([3, 1, 'k' => 2, 10 => 'x'])"#,
        OBJ | TRAV,
    ),
    p("ai0", r#"new \ArrayIterator([])"#, OBJ | TRAV),
    p("ail", r#"new \ArrayIterator(['b', 'a', 'c'])"#, OBJ | TRAV),
    p(
        "ao",
        r#"new \ArrayObject(['x' => 1, 'y' => [2, 3], 0 => 'z'])"#,
        OBJ | TRAV,
    ),
    p("aol", r#"new \ArrayObject([3, 1, 2])"#, OBJ | TRAV),
    p(
        "agg",
        r#"new \Twig\Tests\IteratorAggregateStub(['a' => 1, 'b' => 2, 'c' => 3])"#,
        OBJ | TRAV | TCLS,
    ),
    p(
        "aggl",
        r#"new \Twig\Tests\IteratorAggregateStub([5, 6, 7, 8])"#,
        OBJ | TRAV | TCLS,
    ),
    p(
        "sit",
        r#"new \Twig\Tests\SimpleIteratorForTesting()"#,
        OBJ | TRAV | STR | TCLS,
    ),
    p("foo", r#"new \Twig\Tests\TwigTestFoo()"#, OBJ | TRAV | TCLS),
    p(
        "ts",
        r#"new \Twig\Tests\ToStringStub('to<str> ß')"#,
        OBJ | STR | TCLS,
    ),
    p(
        "tse",
        r#"new \Twig\Tests\ToStringStub('')"#,
        OBJ | STR | TCLS,
    ),
    p(
        "tsn",
        r#"new \Twig\Tests\ToStringStub('12')"#,
        OBJ | STR | NUMSTR | TCLS,
    ),
    p("cs", r#"new \Twig\Tests\CountableStub(3)"#, OBJ | TCLS),
    p("cs0", r#"new \Twig\Tests\CountableStub(0)"#, OBJ | TCLS),
    p("mc", r#"new \Twig\Tests\MagicCallStub()"#, OBJ | TCLS),
    p(
        "obj",
        r#"(object) ['a' => 1, 'b' => 'x', 'c' => [1, 2]]"#,
        OBJ,
    ),
    p("oe", r#"new \stdClass()"#, OBJ),
    p(
        "dt",
        r#"new \DateTime('2021-03-04 05:06:07', new \DateTimeZone('Asia/Tokyo'))"#,
        OBJ | DATE,
    ),
    p(
        "dti",
        r#"new \DateTimeImmutable('2000-01-01 00:00:00', new \DateTimeZone('UTC'))"#,
        OBJ | DATE,
    ),
    p(
        "dtu",
        r#"new \DateTime('1969-12-31 23:59:59.25', new \DateTimeZone('America/New_York'))"#,
        OBJ | DATE,
    ),
    p(
        "di",
        r#"new \DateInterval('P1Y2M3DT4H5M6S')"#,
        OBJ | DATE | INTERVAL,
    ),
    p(
        "did",
        r#"(new \DateTime('2020-01-01 00:00:00', new \DateTimeZone('UTC')))->diff(new \DateTime('2021-03-05 10:20:30', new \DateTimeZone('UTC')))"#,
        OBJ | DATE | INTERVAL,
    ),
    p(
        "din",
        r#"(new \DateTime('2021-03-05', new \DateTimeZone('UTC')))->diff(new \DateTime('2020-01-01', new \DateTimeZone('UTC')))"#,
        OBJ | DATE | INTERVAL,
    ),
    p("tz", r#"new \DateTimeZone('America/New_York')"#, OBJ | TZ),
    p("tzo", r#"new \DateTimeZone('+05:30')"#, OBJ | TZ),
    p("en", r#"\Twig\Tests\DummyBackedEnum::FOO"#, OBJ | ENUM),
    p("enu", r#"\Twig\Tests\DummyUnitEnum::BAR"#, OBJ | ENUM),
    p("tw", r#"$twig->load('b.twig')"#, OBJ | NODUMP),
];

fn var(name: &str) -> Option<&'static Var> {
    VARS.iter().find(|v| v.name == name)
}

/// The PHP expression that builds the same value as `json_decode($json, true)`.
fn php_of_json(json: &Json) -> String {
    match json {
        Json::Null => "null".to_owned(),
        Json::Bool(b) => b.to_string(),
        // `-9223372036854775808` in PHP code is a float (unary minus on an overflowing literal)
        Json::Number(n) if n == "-9223372036854775808" => "PHP_INT_MIN".to_owned(),
        Json::Number(n) => n.clone(),
        Json::String(s) => php_string(s.as_bytes()),
        Json::Array(items) => {
            let items: Vec<String> = items.iter().map(php_of_json).collect();
            format!("[{}]", items.join(", "))
        }
        Json::Object(fields) => {
            let fields: Vec<String> = fields
                .iter()
                .map(|(k, v)| format!("{} => {}", php_string(k.as_bytes()), php_of_json(v)))
                .collect();
            format!("[{}]", fields.join(", "))
        }
    }
}

/// A PHP double-quoted string literal of `bytes`.
fn php_string(bytes: &[u8]) -> String {
    let mut out = String::from("\"");
    for &b in bytes {
        match b {
            b'\\' => out.push_str("\\\\"),
            b'"' => out.push_str("\\\""),
            b'$' => out.push_str("\\$"),
            0x20..=0x7e => out.push(char::from(b)),
            _ => out.push_str(&format!("\\x{b:02x}")),
        }
    }
    out.push('"');
    out
}

/// A Twig single-quoted string literal of `text`.
fn twig_string(text: &str) -> String {
    let mut out = String::from("'");
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                out.push_str(&format!("\\x{:02x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('\'');
    out
}

// ---- literal pools (Twig source) ----------------------------------------------------------------

const INTS: &[&str] = &[
    "0",
    "1",
    "2",
    "3",
    "5",
    "7",
    "10",
    "42",
    "100",
    "255",
    "256",
    "1000",
    "65535",
    "2147483647",
    "2147483648",
    "4294967296",
    "9007199254740993",
    "9223372036854775807",
    "(-1)",
    "(-2)",
    "(-7)",
    "(-128)",
    "(-1000)",
    "(-2147483649)",
    "(-9223372036854775807)",
];

const FLOATS: &[&str] = &[
    "0.0",
    "0.5",
    "1.5",
    "2.5",
    "0.1",
    "1.005",
    "2.675",
    "1.955",
    "5.055",
    "1.45",
    "0.285",
    "4.35",
    "3.0",
    "9.99",
    "1e3",
    "1.0e15",
    "1.0e20",
    "1e100",
    "1e308",
    "1e309",
    "1e-7",
    "5e-324",
    "123456.789",
    "0.30000000000000004",
    "9223372036854775808",
    "(-0.0)",
    "(-0.5)",
    "(-1.5)",
    "(-2.5)",
    "(-1234.5678)",
    "(-1e309)",
    "0.125",
    "0.375",
    "2.5e-5",
    "1.15",
    "1.25",
    "1.35",
    "0.045",
    "99.995",
    "8.325",
    "1.4999999999999999",
    "1000000000000000.5",
    "4.4E+15",
    "123456789012345678.0",
    "(-1.005)",
    "(-0.0001)",
    "1.0E-10",
];

const STRINGS: &[&str] = &[
    "''",
    "' '",
    "'a'",
    "'abc'",
    "'ABC'",
    "'Hello World'",
    r"'hello wORLD foo-bar o\'neil x_y 3rd'",
    "'0'",
    "'1'",
    "'-1'",
    "'2'",
    "'1.5'",
    "'1e3'",
    "' 5'",
    "'5 '",
    "'5abc'",
    "'abc5'",
    "'0x1A'",
    "'.5'",
    "'1_000'",
    "'1,5'",
    "'été'",
    "'ÉTÉ'",
    "'ß'",
    "'İ'",
    "'ı'",
    "'ǅ'",
    "'ΣΑΣ'",
    "'😀'",
    "'日本'",
    "'ﬁ'",
    r#""\xff""#,
    r#""a\x80b""#,
    r#""\xc3\x28""#,
    r#""\xe2\x82""#,
    r#""\xf0\x9f\x98""#,
    r#""\xed\xa0\x80""#,
    r#""\xc0\xaf""#,
    "'<b>x</b>'",
    "'a & b'",
    r#"'"q"'"#,
    r#""it's""#,
    "'%s'",
    "'%d%%'",
    r#""a\nb""#,
    r#""\r\n""#,
    r#""\t x \t""#,
    "'null'",
    "'true'",
    "'a,b,,c'",
    "'  a  b  '",
    r"'x\\y'",
    r"'\x00'",
    "'Y-m-d'",
    "'UTC'",
    "'html'",
    "'ŉ'",
    "'ǰ'",
    "'ΐ'",
    "'ﬓ'",
    "'ᾳ ᾼ'",
    "'Ǆ ǈ'",
    "'ẞ'",
    "'ΣΑΣ. aΣ Σ'",
    "'ⓐⓑ'",
    "'Ꭰꭰ'",
    "'i̇ ı̇'",
    "'٣①'",
    r"'o\'neil mcdonald-smith 3rd x.y'",
    "'ÉCOLE élève'",
    "'ǅungla ǈubljana'",
    "'ꞵꞴ'",
    "'𐐀𐐨'",
    "'ἀΐ'",
    r#""Ã©ÿÃ""#,
    r#""été""#,
];

const ARRAYS: &[&str] = &[
    "[]",
    "[1, 2, 3]",
    "[3, 1, 2]",
    "['a', 'b', 'c']",
    "['b', 'a', 'C', '10', '9']",
    "{'a': 1, 'b': 2}",
    "{'b': 2, 'a': 1, 'c': 0}",
    "{0: 'x', 2: 'y', 5: 'z'}",
    "{'x': 'y', 0: 'z'}",
    "[1, [2, 3], [4, [5]]]",
    "[null, false, '', 0, '0', []]",
    "{'k': {'n': 1}}",
    "['10', 9, '9a', 1.5, true, null]",
    "[[1, 2], [3, 4], [5]]",
    "[{'id': 1, 'n': 'a'}, {'id': 2, 'n': 'b'}, {'n': 'c'}]",
    "{'1': 'a', '01': 'b', '1.5': 'c'}",
    "{(-1): 'neg', 3: 'pos'}",
    "['é', 'a', 'Z', '😀']",
    "[1.5, 2.5, -0.5, 1e100]",
    "[true, false]",
    "[1, '1', 1.0]",
    "{'a': 'A', 'b': null}",
    "[0]",
    "['x']",
    "{'only': 1}",
    "[...[1, 2], 3]",
    "{...{'a': 1}, 'b': 2}",
];

const ARROW1: &[&str] = &[
    "v => v",
    "v => v > 1",
    "v => not v",
    "v => v is even",
    "v => v ~ '!'",
    "v => v|upper",
    "(v, k) => k",
    "(v, k) => k ~ '=' ~ v",
    "v => null",
    "v => [v]",
    "v => v * 2",
    "v => v.id",
    "v => v|length",
    "v => v is iterable",
    "(v, k) => k is even",
    "v => v is same as(1)",
    "v => '0'",
    "v => v ?? 'd'",
    "(v, k) => v ~ k",
    "v => v|json_encode",
    "x => x",
    "v => v is not null",
    "v => v|default('d')",
    "(v, k, it) => k",
    "() => 1",
    "v => v|round",
    "v => v|first",
    "v => v.name ?? '-'",
];

const ARROW2: &[&str] = &[
    "(a, b) => a <=> b",
    "(a, b) => b <=> a",
    "(a, b) => a - b",
    "(a, b) => b - a",
    "(a, b) => a > b",
    "(a, b) => a < b",
    "(a, b) => 0",
    "(a, b) => -1",
    "(a, b) => 1.5",
    "(a, b) => null",
    "(a, b) => 'x'",
    "(a, b) => '1'",
    "(a, b) => a|length <=> b|length",
    "(a, b) => a.id <=> b.id",
    "(a, b) => (a ~ '') <=> (b ~ '')",
    "(a, b) => [a] <=> [b]",
    "(a, b) => a|lower <=> b|lower",
    "a => a",
];

const ARROW3: &[&str] = &[
    "(c, v) => c + v",
    "(c, v) => c ~ v",
    "(c, v, k) => c ~ k",
    "(c, v) => c|merge([v])",
    "(c, v) => max(c, v)",
    "(c, v) => v",
    "(c, v) => c ?? v",
    "(c, v, k) => c ~ k ~ '=' ~ v ~ ';'",
    "(c, v) => c * v",
    "(c, v) => [c, v]",
    "c => c",
    "(c, v) => c|default(0) + v|length",
    "(c, v) => c is null ? v : c",
];

/// Closures for `invoke` and how many arguments they need (fewer is an `ArgumentCountError` whose
/// closure name is a known deviation).
const CLOSURES: &[(&str, usize)] = &[
    ("(v => v ~ '!')", 1),
    ("((a, b) => a ~ '-' ~ b)", 2),
    ("(v => v * 2)", 1),
    ("(v => v|upper)", 1),
    ("(() => 'z')", 0),
    ("((a, b, c) => c)", 3),
    ("(x => x)", 1),
    ("(v => [v, v])", 1),
    ("((a, b) => b)", 2),
];

/// Values that are not closures and do not name a PHP function (see `reference/deviations.md`,
/// "String callables").
const NON_CALLABLES: &[&str] = &["null", "'nope'", "''", "1", "[1, 2]", "['a', 'b']", "true"];

const BOOLS: &[&str] = &[
    "true", "false", "null", "0", "1", "'yes'", "''", "[]", "'0'", "2",
];

const BATCH_SIZES: &[&str] = &[
    "1", "2", "3", "2.1", "0.5", "'2'", "0", "(-1)", "'a'", "null", "100", "true", "[]", "(-2.5)",
    "1.0", "' 3'", "'1e1'", "1e309",
];

const COLUMN_KEYS: &[&str] = &[
    "'id'", "'name'", "'v'", "'n'", "0", "1", "null", "'nope'", "1.5", "true", "[]", "'0'", "'a'",
];

const CHARSETS: &[&str] = &[
    "'UTF-8'",
    "'utf-8'",
    "'utf8'",
    "'ISO-8859-1'",
    "'iso-8859-15'",
    "'Windows-1252'",
    "'cp1251'",
    "'KOI8-R'",
    "'KOI8-U'",
    "'ASCII'",
    "'US-ASCII'",
    "'UTF-16'",
    "'UTF-16BE'",
    "'UTF-16LE'",
    "'UTF-32'",
    "'UCS-2'",
    "'SJIS'",
    "'Shift_JIS'",
    "'EUC-JP'",
    "'BIG5'",
    "'GB2312'",
    "'GB18030'",
    "'CP850'",
    "'MACINTOSH'",
    "'ISO-2022-JP'",
    "'BIG5-HKSCS'",
    "'cp866'",
    "'MacRoman'",
    "'koi8-ru'",
    "'EUCJP'",
    "'SJIS-win'",
    "'CP932'",
    "'eucJP-win'",
    "'Windows-1251'",
    "'win-1251'",
    "'936'",
    "'950'",
    "'1252'",
    "'866'",
    "'ISO8859-5'",
    "'ASCII//TRANSLIT'",
    "'ASCII//IGNORE'",
    "'UTF-8//IGNORE'",
    "'ISO-8859-1//TRANSLIT'",
    "'nope'",
    "''",
    "null",
];

const STRATEGIES: &[&str] = &[
    "'html'",
    "'js'",
    "'css'",
    "'url'",
    "'html_attr'",
    "'html_attr_relaxed'",
    "'HTML'",
    "'foo'",
    "''",
    "null",
    "1",
    "false",
    "true",
];

const DATE_PIECES: &[&str] = &[
    "d", "D", "j", "l", "N", "S", "w", "z", "W", "F", "m", "M", "n", "t", "L", "o", "X", "x", "Y",
    "y", "a", "A", "B", "g", "G", "h", "H", "i", "s", "e", "I", "O", "P", "p", "T", "Z", "c", "r",
    "U", " ", "-", ":", "/", ",", "\\Y", "\\d", "é", "%", "\\\\",
];

const INTERVAL_FORMATS: &[&str] = &[
    "'%d days'",
    "'%a'",
    "'%R%a'",
    "'%y-%m-%d %h:%i:%s'",
    "'%H:%I:%S'",
    "'%f %F'",
    "'%%'",
    "'%x %r'",
    "'%Y %M %D'",
    "'%R %a days'",
    "''",
];

const TIMEZONES: &[&str] = &[
    "'UTC'",
    "'Europe/Paris'",
    "'America/New_York'",
    "'Asia/Kathmandu'",
    "'Australia/Lord_Howe'",
    "'Pacific/Chatham'",
    "'Asia/Tokyo'",
    "'+05:45'",
    "'-03:30'",
    "'+14:00'",
    "'EST'",
    "'CET'",
    "'utc'",
    "'europe/paris'",
    "false",
    "null",
    "'Nope/Zone'",
    "''",
    "true",
    "0",
    "'Z'",
    "'GMT+2'",
    "'Etc/GMT+2'",
    "'America/Argentina/Buenos_Aires'",
    "'Antarctica/Troll'",
    "'+25:00'",
    "'Europe/Kyiv'",
    "'Europe/Kiev'",
];

/// Date inputs that do not depend on the current time.
const DATES_FIXED: &[&str] = &[
    "0",
    "(-1)",
    "1700000000",
    "2147483648",
    "(-62135596800)",
    "'1700000000'",
    "'-86400'",
    "'@86400'",
    "'@-1'",
    "'@1.5'",
    "'2020-02-29'",
    "'2020-02-30'",
    "'2021-03-28 02:30:00'",
    "'2021-10-31 02:30:00'",
    "'2020-01-01T10:00:00+05:45'",
    "'2020-01-01 10:00:00 America/New_York'",
    "'garbage'",
    "true",
    "'2020-13-45'",
    "'0000-00-00'",
    "'-0001-11-30'",
    "'9999-12-31 23:59:59'",
    "'1700000000.5'",
    "1700000000.5",
    "'2020-06-15 12:00:00.123456'",
    "'Sat, 01 Jan 2022 10:00:00 +0100'",
    "'2022W05'",
    "'2022-002'",
    "[]",
    "'20200101'",
    "'10/12/2020'",
    "'12.10.2020'",
    "'1 January 2021'",
    "'2021-06-01 12:00 PDT'",
];

/// Date inputs relative to the current time (the oracle's clock is replayed, microseconds are not).
const DATES_NOW: &[&str] = &[
    "null",
    "'now'",
    "'tomorrow'",
    "'yesterday noon'",
    "'last day of february'",
    "'first monday of next month'",
    "'+1 week 2 days'",
    "''",
    "'1.5'",
    "1.5",
    "false",
    "'noon'",
    "'midnight'",
    "' 12'",
    "'1e3'",
];

const MODIFIERS: &[&str] = &[
    "'+1 day'",
    "'-1 month'",
    "'+1 month'",
    "'last day of this month'",
    "'first day of next year'",
    "'noon'",
    "'midnight'",
    "'+90 minutes'",
    "'next monday'",
    "'+1 week 2 days 4 hours 2 seconds'",
    "'-1 year'",
    "'@0'",
    "'garbage'",
    "''",
    "'+0 day'",
    "'tomorrow'",
    "'+1 sec'",
    "'saturday this week'",
    "'+2147483648 days'",
    "'last day of'",
    "'+1.5 days'",
    "'UTC'",
    "'Europe/Paris'",
    "'+1 weekday'",
    "'-3 weekdays'",
    "null",
    "1",
    "'1 fortnight ago'",
    "'2020-01-01'",
    "'+1 usec'",
];

const GLUES: &[&str] = &[
    "''", "','", "', '", "' and '", "null", "1", "true", "[]", "'é'", r#""\n""#, "' | '", "1.5",
];

const JSON_FLAGS: &[&str] = &[
    "0",
    "1",
    "2",
    "4",
    "8",
    "16",
    "32",
    "64",
    "128",
    "256",
    "512",
    "1024",
    "2048",
    "15",
    "4194304",
    "(-1)",
    "'2'",
    "null",
    "1.5",
    "constant('JSON_PRETTY_PRINT')",
    "constant('JSON_PRESERVE_ZERO_FRACTION')",
    "constant('JSON_PARTIAL_OUTPUT_ON_ERROR')",
    "constant('JSON_INVALID_UTF8_SUBSTITUTE')",
    "constant('JSON_INVALID_UTF8_IGNORE')",
    "constant('JSON_FORCE_OBJECT')",
    "constant('JSON_NUMERIC_CHECK')",
    "constant('JSON_HEX_TAG')",
    "constant('JSON_UNESCAPED_UNICODE')",
    "constant('JSON_UNESCAPED_SLASHES')",
    "constant('JSON_THROW_ON_ERROR')",
];

const DEPTHS: &[&str] = &[
    "1",
    "2",
    "3",
    "512",
    "0",
    "(-1)",
    "2147483647",
    "2147483648",
    "'3'",
    "null",
    "1.5",
];

const DECIMALS: &[&str] = &[
    "0", "1", "2", "3", "10", "20", "50", "100", "(-1)", "(-2)", "(-20)", "'2'", "2.7", "null",
    "'x'", "true", "' 1'",
];

const SEPARATORS: &[&str] = &[
    "'.'",
    "','",
    "' '",
    "''",
    "'dec'",
    "'é'",
    "null",
    "1",
    "true",
    "'::'",
    r#""\xff""#,
    "[]",
];

const PRECISIONS: &[&str] = &[
    "0", "1", "2", "3", "(-1)", "(-2)", "(-3)", "10", "15", "16", "20", "308", "309", "400",
    "(-400)", "'2'", "2.9", "null", "'abc'", "true", "' 1'",
];

const METHODS: &[&str] = &[
    "'common'", "'ceil'", "'floor'", "'up'", "'COMMON'", "''", "null", "0", "true",
];

const REPLACE_MAPS: &[&str] = &[
    "{'a': 'X'}",
    "{'a': 'b', 'b': 'a'}",
    "{'': 'X'}",
    "{'abc': 1, 'ab': 2, 'a': 3}",
    "['x', 'y']",
    "{'%this%': 'that'}",
    "{'é': 'e', 'ß': 'ss'}",
    "{'a': [1]}",
    "{'a': null, 'b': true, 'c': 1.5}",
    "[]",
    "'a'",
    "null",
    "{1: 'one', 2: 'two'}",
    "{' ': '_'}",
    r#"{"\xff": '?'}"#,
];

const OFFSETS: &[&str] = &[
    "0",
    "1",
    "2",
    "3",
    "(-1)",
    "(-2)",
    "(-10)",
    "10",
    "100",
    "null",
    "'1'",
    "1.7",
    "'x'",
    "true",
    "false",
    "9223372036854775807",
    "(-9223372036854775807)",
    "[]",
    "' 2'",
];

const DELIMITERS: &[&str] = &[
    "','", "''", "' '", "'b'", "'ab'", "'é'", r#""\n""#, "null", "0", "1", "'aa'", "'😀'", "[]",
];

const LIMITS: &[&str] = &[
    "null",
    "0",
    "1",
    "2",
    "3",
    "(-1)",
    "(-2)",
    "100",
    "'2'",
    "1.5",
    "'x'",
    "true",
    "9223372036854775807",
    "(-9223372036854775807)",
];

const TAGS: &[&str] = &[
    "null",
    "'<b>'",
    "'<b><i>'",
    "'<p><br>'",
    "['b', 'i']",
    "['<b>']",
    "''",
    "'b'",
    "'<script>'",
    "'<b><br/>'",
    "['br']",
    "[1]",
    "'bi'",
    "'<B>'",
];

const TRIM_MASKS: &[&str] = &[
    "null",
    "' '",
    "'a'",
    "'abc'",
    "'a..z'",
    "'A..Z0..9'",
    "'z..a'",
    "'..a'",
    "'a..'",
    "'..'",
    "'a..b..c'",
    r#""\x00..\x1f""#,
    r#"" \t\n""#,
    "'<>'",
    "'é'",
    "'0'",
    "''",
    "'ab..'",
    "'xy'",
];

const SIDES: &[&str] = &[
    "'both'", "'left'", "'right'", "'BOTH'", "'middle'", "''", "null", "1", "true",
];

const TEMPLATE_NAMES: &[&str] = &[
    "'b.twig'",
    "'e.twig'",
    "'p.twig'",
    "'nope.twig'",
    "['nope.twig', 'b.twig']",
    "['nope', 'nope2']",
    "['nope']",
    "[]",
    "''",
    "'B.twig'",
    "'b.twig '",
];

const TEMPLATE_VARS: &[&str] = &[
    "{}",
    "{'v': 'V'}",
    "{'v': [1, 2]}",
    "[]",
    "[1, 2]",
    "null",
    "'x'",
    "{'_context': 1}",
    "{(1): 'one'}",
    "{'v': '<b>'}",
];

const BLOCK_NAMES: &[&str] = &["'blk'", "'other'", "'nope'", "''", "'BLK'", "null", "1"];

const ATTRIBUTE_NAMES: &[&str] = &[
    "'a'",
    "'b'",
    "'c'",
    "'id'",
    "'nope'",
    "0",
    "1",
    "'getFoo'",
    "'foo'",
    "'bar'",
    "'BAR_NAME'",
    "'array'",
    "'position'",
    "'self'",
    "'getSelf'",
    "'count'",
    "'x'",
    "'name'",
    "'value'",
    "'timestamp'",
    "'timezone'",
];

/// Method arguments of `attribute()`: arrays, rarely something else (PHP's `TypeError` for a non-array is a known
/// mismatch that would mask others).
const ATTRIBUTE_ARGS: &[&str] = &[
    "[]",
    "['p1']",
    "['p1', 'p2']",
    "[1, 2, 3]",
    "{'a': 1}",
    "['p1', null]",
    "[[1]]",
    "[]",
    "['x']",
    "null",
];

const TEMPLATE_SOURCES: &[&str] = &[
    "'{{ 1 + 1 }}'",
    r#"'Hello {{ v|default("x") }}'"#,
    "'{{ _self }}'",
    "'{% if %}'",
    "'{{ undef }}'",
    "''",
    "'{% block b %}BB{% endblock %}'",
    "'{{ v }}'",
    r#"'{{ "é"|upper }}'"#,
    "'{{ v|join(\",\") }}'",
    "'{{ include(\"b.twig\") }}'",
    "'{% extends \"p.twig\" %}'",
    "'{{ parent() }}'",
];

const ENUM_NAMES: &[&str] = &[
    r"'Twig\\Tests\\DummyBackedEnum'",
    r"'Twig\\Tests\\DummyUnitEnum'",
    r"'\\Twig\\Tests\\DummyBackedEnum'",
    "'stdClass'",
    "'Nope'",
    "''",
    "1",
    "null",
];

const CONSTANT_NAMES: &[&str] = &[
    "'PHP_EOL'",
    "'PHP_INT_MAX'",
    "'PHP_INT_MIN'",
    "'PHP_INT_SIZE'",
    "'PHP_FLOAT_EPSILON'",
    "'PHP_FLOAT_MAX'",
    "'PHP_FLOAT_MIN'",
    "'PHP_FLOAT_DIG'",
    "'M_PI'",
    "'M_E'",
    "'M_SQRT2'",
    "'E_ALL'",
    "'E_USER_DEPRECATED'",
    "'E_STRICT'",
    "'JSON_PRETTY_PRINT'",
    "'JSON_THROW_ON_ERROR'",
    "'ENT_QUOTES'",
    "'SORT_STRING'",
    "'COUNT_RECURSIVE'",
    "'PHP_VERSION'",
    "'PHP_MAJOR_VERSION'",
    "'PHP_OS_FAMILY'",
    "'DIRECTORY_SEPARATOR'",
    "'MB_CASE_TITLE'",
    "'PREG_SPLIT_NO_EMPTY'",
    "'PHP_ROUND_HALF_UP'",
    "'DATE_ATOM'",
    "'DATE_RFC7231'",
    "'DateTimeInterface::ATOM'",
    "'DateTime::RFC3339'",
    r"'Twig\\Tests\\TwigTestFoo::BAR_NAME'",
    r"'Twig\\Tests\\DummyBackedEnum::FOO'",
    "'NOPE'",
    "'php_eol'",
    "'true'",
    "'TRUE'",
    "'NULL'",
    "'INF'",
    "'NAN'",
    "'PHP_EOL::class'",
    "'stdClass::class'",
    "'stdClass::CLASS'",
    "'ArrayObject::ARRAY_AS_PROPS'",
    r"'Twig\\Environment::VERSION'",
    r"'Twig\\Environment::MAJOR_VERSION'",
    "''",
    "'::'",
];

/// Constant names looked up on an object (`constant(name, object)`).
const OBJECT_CONSTANT_NAMES: &[&str] = &[
    "'class'",
    "'BAR_NAME'",
    "'FOO'",
    "'BAR'",
    "'ARRAY_AS_PROPS'",
    "'ATOM'",
    "'nope'",
    "'CLASS'",
];

const DIVISORS: &[&str] = &[
    "1",
    "2",
    "3",
    "0",
    "(-1)",
    "(-2)",
    "0.5",
    "1.5",
    "'2'",
    "'0'",
    "'a'",
    "null",
    "true",
    "[]",
    "9223372036854775807",
    "(-9223372036854775807)",
];

const POSITIONS: &[&str] = &[
    "0",
    "1",
    "2",
    "5",
    "(-1)",
    "(-3)",
    "1.5",
    "'1'",
    "'x'",
    "null",
    "true",
    "9223372036854775807",
    "(-9223372036854775807)",
];

const SPRINTF_SPECS: &[&str] = &[
    "%s",
    "%d",
    "%u",
    "%f",
    "%F",
    "%.2f",
    "%10.4f",
    "%-10s|",
    "%05d",
    "%x",
    "%X",
    "%o",
    "%b",
    "%c",
    "%e",
    "%.3e",
    "%E",
    "%g",
    "%G",
    "%h",
    "%H",
    "%%",
    "%'*10s",
    "%'010d",
    "%+d",
    "%+.1f",
    "%1$s",
    "%2$s",
    "%3$d",
    "% d",
    "%5.1s",
    "%-5d",
    "%.0f",
    "%.15g",
    "%.60f",
    "%.54e",
    "%y",
    "%0$s",
    "%2147483648$s",
    "%'",
    "%-'x8.3f",
    "%010.3e",
    "%2147483648s",
    "%.2147483648f",
    "%1$'#10.3f",
];

const SPRINTF_TEXT: &[&str] = &["a", " ", "|", "-", "é", "x=", "[", "]"];

/// Inputs of `shuffle` whose result does not depend on the random generator.
const SHUFFLE_INPUTS: &[&str] = &[
    "[]",
    "[1]",
    "{'a': 1}",
    "'a'",
    "''",
    "'é'",
    "5",
    "null",
    "true",
    "1.5",
    r#""\xff""#,
    "{'k': [1, 2]}",
];

/// `random()` arguments whose result does not depend on the random generator.
const RANDOM_ARGS: &[&str] = &[
    "0",
    "5, 5",
    "(-3), (-3)",
    "'a'",
    "''",
    "'é'",
    "[]",
    "{}",
    "['x']",
    "{'a': 9}",
    "10, 5",
    "2.9, 2.9",
    "null, 0",
    "true",
    "null, (-1)",
    "'a', 1",
    "[7], 3",
    r#""\xff""#,
    "values=[3]",
    "max=0",
    "9223372036854775807, 9223372036854775807",
    "(-9223372036854775807), (-9223372036854775807)",
    "1e309, 1e309",
];

// ---- well-typed values ([`Profile::Valid`], [`Profile::Focus`]) -----------------------------------

const GOOD_STRINGS: &[&str] = &[
    "''",
    "' '",
    "'a'",
    "'abc'",
    "'Hello World'",
    r"'hello wORLD foo-bar o\'neil x_y 3rd'",
    "'0'",
    "'1.5'",
    "'été'",
    "'ÉTÉ'",
    "'ß'",
    "'İ'",
    "'ǅ'",
    "'ΣΑΣ. aΣ Σ'",
    "'😀'",
    "'日本'",
    "'ﬁ'",
    "'ŉ ǰ ΐ'",
    "'ﬓ'",
    "'ᾳ ᾼ'",
    "'Ǆ ǈ'",
    "'ẞ'",
    "'Ꭰꭰ'",
    "'i̇ ı̇'",
    r"'o\'neil mcdonald-smith 3rd x.y'",
    "'ÉCOLE élève'",
    "'𐐀𐐨'",
    "'<b>x</b> & y'",
    r#""a\nb\r\nc""#,
    r#""\t x \t""#,
    "'a,b,,c'",
    "'  a  b  '",
    r#""\xff""#,
    r#""a\x80b""#,
    r#""\xc3\x28""#,
    r#""\xe9t\xe9""#,
    "'%s'",
    "'1e3'",
    "' 5'",
    "'5abc'",
];

const GOOD_ARRAYS: &[&str] = &[
    "[]",
    "[1, 2, 3]",
    "[3, 1, 2]",
    "['a', 'b', 'c']",
    "['b', 'a', 'C', '10', '9']",
    "{'a': 1, 'b': 2}",
    "{'b': 2, 'a': 1, 'c': 0}",
    "{0: 'x', 2: 'y', 5: 'z'}",
    "{'x': 'y', 0: 'z'}",
    "[1, [2, 3], [4, [5]]]",
    "[null, false, '', 0, '0', []]",
    "['10', 9, '9a', 1.5, true, null]",
    "[[1, 2], [3, 4], [5]]",
    "[{'id': 1, 'n': 'a'}, {'id': 2, 'n': 'b'}, {'n': 'c'}]",
    "{'1': 'a', '01': 'b', '1.5': 'c'}",
    "['é', 'a', 'Z', '😀']",
    "[1.5, 2.5, -0.5, 1e100]",
    "[1, '1', 1.0]",
    "{'a': 'A', 'b': null}",
    "[0]",
];

const GOOD_STRATEGIES: &[&str] = &[
    "'html'",
    "'js'",
    "'css'",
    "'url'",
    "'html_attr'",
    "'html_attr_relaxed'",
];

const GOOD_CHARSETS: &[&str] = &[
    "'UTF-8'",
    "'utf-8'",
    "'ISO-8859-1'",
    "'iso-8859-15'",
    "'Windows-1252'",
    "'cp1251'",
    "'KOI8-R'",
    "'BIG5'",
    "'SJIS'",
    "'EUC-JP'",
    "'GB2312'",
    "'cp866'",
    "'MacRoman'",
    "'UTF-16'",
    "'UTF-16BE'",
    "'UTF-32'",
    "'ASCII//TRANSLIT'",
    "'ASCII//IGNORE'",
    "'GB18030'",
    "'KOI8-U'",
    "'CP850'",
    "'UCS-2'",
];

const GOOD_TIMEZONES: &[&str] = &[
    "'UTC'",
    "'Europe/Paris'",
    "'America/New_York'",
    "'Asia/Kathmandu'",
    "'Australia/Lord_Howe'",
    "'Pacific/Chatham'",
    "'Asia/Tokyo'",
    "'+05:45'",
    "'-03:30'",
    "'+14:00'",
    "'EST'",
    "'CET'",
    "'utc'",
    "false",
    "null",
    "'Etc/GMT+2'",
    "'America/Argentina/Buenos_Aires'",
    "'Antarctica/Troll'",
    "'Europe/Kyiv'",
    "'Europe/Kiev'",
    "'Pacific/Kiritimati'",
    "'America/St_Johns'",
];

const GOOD_DATES: &[&str] = &[
    "0",
    "(-1)",
    "1700000000",
    "2147483648",
    "(-62135596800)",
    "'1700000000'",
    "'-86400'",
    "'@86400'",
    "'@-1'",
    "'@1.5'",
    "'2020-02-29'",
    "'2020-02-30'",
    "'2021-03-28 02:30:00'",
    "'2021-10-31 02:30:00'",
    "'2020-01-01T10:00:00+05:45'",
    "'2020-01-01 10:00:00 America/New_York'",
    "true",
    "'0000-00-00'",
    "'-0001-11-30'",
    "'9999-12-31 23:59:59'",
    "'2020-06-15 12:00:00.123456'",
    "'Sat, 01 Jan 2022 10:00:00 +0100'",
    "'2022W05'",
    "'2022-002'",
    "'20200101'",
    "'10/12/2020'",
    "'12.10.2020'",
    "'1 January 2021'",
    "'2021-06-01 12:00 PDT'",
    "'2000-01-01 00:00:00.5 UTC'",
    "'1969-12-31 23:59:59'",
    "'2038-01-19 03:14:08'",
];

const GOOD_MODIFIERS: &[&str] = &[
    "'+1 day'",
    "'-1 month'",
    "'+1 month'",
    "'last day of this month'",
    "'first day of next year'",
    "'noon'",
    "'midnight'",
    "'+90 minutes'",
    "'next monday'",
    "'+1 week 2 days 4 hours 2 seconds'",
    "'-1 year'",
    "'@0'",
    "'+0 day'",
    "'tomorrow'",
    "'+1 sec'",
    "'saturday this week'",
    "'last day of'",
    "'UTC'",
    "'Europe/Paris'",
    "'+1 weekday'",
    "'-3 weekdays'",
    "'1 fortnight ago'",
    "'2020-01-01'",
    "'+1 usec'",
    "'last sunday of march'",
    "'+25 hours'",
];

const GOOD_GLUES: &[&str] = &[
    "''", "','", "', '", "' and '", "'é'", r#""\n""#, "' | '", "'0'",
];

const GOOD_JSON_FLAGS: &[&str] = &[
    "0",
    "1",
    "2",
    "4",
    "8",
    "16",
    "32",
    "64",
    "128",
    "256",
    "512",
    "1024",
    "2048",
    "15",
    "1572864",
    "constant('JSON_PRETTY_PRINT')",
    "constant('JSON_PRESERVE_ZERO_FRACTION')",
    "constant('JSON_PARTIAL_OUTPUT_ON_ERROR')",
    "constant('JSON_INVALID_UTF8_SUBSTITUTE')",
    "constant('JSON_INVALID_UTF8_IGNORE')",
    "constant('JSON_FORCE_OBJECT')",
    "constant('JSON_NUMERIC_CHECK')",
    "constant('JSON_UNESCAPED_UNICODE')",
    "constant('JSON_UNESCAPED_SLASHES')",
    "constant('JSON_THROW_ON_ERROR')",
];

const GOOD_DEPTHS: &[&str] = &["1", "2", "3", "512", "'3'"];

const GOOD_DECIMALS: &[&str] = &[
    "0", "1", "2", "3", "4", "6", "10", "15", "20", "(-1)", "(-2)", "(-3)", "'2'",
];

const GOOD_SEPARATORS: &[&str] = &[
    "'.'", "','", "' '", "''", "'dec'", "'é'", "'::'", "'\u{a0}'",
];

const GOOD_PRECISIONS: &[&str] = &[
    "0", "1", "2", "3", "4", "(-1)", "(-2)", "(-3)", "10", "14", "15", "16", "17", "20", "'2'",
];

const GOOD_METHODS: &[&str] = &["'common'", "'ceil'", "'floor'"];

const GOOD_REPLACE_MAPS: &[&str] = &[
    "{'a': 'X'}",
    "{'a': 'b', 'b': 'a'}",
    "{'abc': 1, 'ab': 2, 'a': 3}",
    "['x', 'y']",
    "{'%this%': 'that'}",
    "{'é': 'e', 'ß': 'ss'}",
    "{'a': null, 'b': true, 'c': 1.5}",
    "[]",
    "{1: 'one', 2: 'two'}",
    "{' ': '_'}",
    "{'l': 'L', 'll': '--', 'lo': '0'}",
];

const GOOD_OFFSETS: &[&str] = &[
    "0",
    "1",
    "2",
    "3",
    "(-1)",
    "(-2)",
    "(-10)",
    "10",
    "null",
    "'1'",
    "9223372036854775807",
    "(-9223372036854775807)",
];

const GOOD_DELIMITERS: &[&str] = &[
    "','", "''", "' '", "'b'", "'ab'", "'é'", r#""\n""#, "'😀'", "'l'",
];

const GOOD_LIMITS: &[&str] = &["null", "0", "1", "2", "3", "(-1)", "(-2)", "100", "'2'"];

const GOOD_TAGS: &[&str] = &[
    "null",
    "'<b>'",
    "'<b><i>'",
    "'<p><br>'",
    "['b', 'i']",
    "['<b>']",
    "''",
    "'<script>'",
    "'<b><br/>'",
    "['br']",
    "'<B>'",
];

const GOOD_TRIM_MASKS: &[&str] = &[
    "null",
    "' '",
    "'a'",
    "'abc'",
    "'a..z'",
    "'A..Z0..9'",
    r#"" \t\n""#,
    "'<>'",
    "'é'",
    "'0'",
    "''",
    "'xy'",
    "'z..a'",
    "'a..b..c'",
];

const GOOD_SIDES: &[&str] = &["'both'", "'left'", "'right'"];

const GOOD_BATCH_SIZES: &[&str] = &["1", "2", "3", "4", "2.1", "0.5", "'2'", "100"];

const GOOD_COLUMN_KEYS: &[&str] = &["'id'", "'name'", "'v'", "'n'", "0", "1", "null", "'nope'"];

const GOOD_TEMPLATE_NAMES: &[&str] = &[
    "'b.twig'",
    "'e.twig'",
    "'p.twig'",
    "['nope.twig', 'b.twig']",
    "'nope.twig'",
];

const GOOD_TEMPLATE_VARS: &[&str] = &["{}", "{'v': 'V'}", "{'v': [1, 2]}", "[]", "{'v': '<b>'}"];

const GOOD_BLOCK_NAMES: &[&str] = &["'blk'", "'other'", "'nope'"];

const GOOD_ENUM_NAMES: &[&str] = &[
    r"'Twig\\Tests\\DummyBackedEnum'",
    r"'Twig\\Tests\\DummyUnitEnum'",
];

const GOOD_CONSTANT_NAMES: &[&str] = &[
    "'PHP_EOL'",
    "'PHP_INT_MAX'",
    "'PHP_INT_MIN'",
    "'PHP_INT_SIZE'",
    "'PHP_FLOAT_EPSILON'",
    "'PHP_FLOAT_MAX'",
    "'PHP_FLOAT_MIN'",
    "'PHP_FLOAT_DIG'",
    "'M_PI'",
    "'M_E'",
    "'M_SQRT2'",
    "'E_ALL'",
    "'E_USER_DEPRECATED'",
    "'JSON_PRETTY_PRINT'",
    "'ENT_QUOTES'",
    "'SORT_STRING'",
    "'PHP_VERSION'",
    "'PHP_OS_FAMILY'",
    "'DIRECTORY_SEPARATOR'",
    "'MB_CASE_TITLE'",
    "'DATE_ATOM'",
    "'DateTimeInterface::ATOM'",
    "'DateTime::RFC3339'",
    r"'Twig\\Tests\\DummyBackedEnum::FOO'",
    "'NAN'",
    "'INF'",
    "'PHP_FLOAT_MIN'",
    "'M_LN2'",
    "'PREG_SPLIT_NO_EMPTY'",
    "'E_STRICT'",
    "'DATE_RFC7231'",
];

const GOOD_DIVISORS: &[&str] = &[
    "1",
    "2",
    "3",
    "(-1)",
    "(-2)",
    "'2'",
    "7",
    "9223372036854775807",
];

const GOOD_POSITIONS: &[&str] = &["0", "1", "2", "5", "(-1)", "(-3)", "'1'", "1.5"];

const GOOD_BOOLS: &[&str] = &["true", "false"];

// ---- callables ------------------------------------------------------------------------------------

/// What a parameter (or a filter's input) prefers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Fl {
    Any,
    Str,
    Num,
    Iter,
    Seqish,
    Rows,
    Date,
    Html,
    Nl,
    Maybe,
    Closure,
    Arrow1,
    Arrow2,
    Arrow3,
    Bool,
    BatchSize,
    ColumnKey,
    Charset,
    DateFormat,
    Timezone,
    Modifier,
    Strategy,
    Glue,
    JsonFlags,
    Depth,
    Decimals,
    Separator,
    Precision,
    Method,
    ReplaceMap,
    Offset,
    Delimiter,
    Limit,
    Tags,
    TrimMask,
    Side,
    Format,
    TemplateName,
    TemplateVars,
    BlockName,
    AttributeName,
    AttributeArgs,
    TemplateSource,
    EnumName,
    ConstantName,
    Object,
    Divisor,
    Position,
    UrlInput,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Param {
    name: &'static str,
    fl: Fl,
}

const fn pa(name: &'static str, fl: Fl) -> Param {
    Param { name, fl }
}

/// A filter or function: its parameters after the input, how many are required, and the
/// variadic tail (its PHP name and what it prefers).
#[derive(Clone, Copy, Debug)]
struct Spec {
    name: &'static str,
    input: Fl,
    params: &'static [Param],
    required: usize,
    variadic: Option<Param>,
    weight: u32,
}

const fn sp(
    name: &'static str,
    input: Fl,
    params: &'static [Param],
    required: usize,
    weight: u32,
) -> Spec {
    Spec {
        name,
        input,
        params,
        required,
        variadic: None,
        weight,
    }
}

const fn spv(
    name: &'static str,
    input: Fl,
    params: &'static [Param],
    required: usize,
    variadic: Param,
    weight: u32,
) -> Spec {
    Spec {
        name,
        input,
        params,
        required,
        variadic: Some(variadic),
        weight,
    }
}

const ESCAPE_PARAMS: &[Param] = &[
    pa("strategy", Fl::Strategy),
    pa("charset", Fl::Charset),
    pa("autoescape", Fl::Bool),
];

const FILTERS: &[Spec] = &[
    sp("abs", Fl::Num, &[], 0, 5),
    sp(
        "batch",
        Fl::Iter,
        &[
            pa("size", Fl::BatchSize),
            pa("fill", Fl::Any),
            pa("preserve_keys", Fl::Bool),
        ],
        1,
        5,
    ),
    sp("capitalize", Fl::Str, &[], 0, 4),
    sp(
        "column",
        Fl::Rows,
        &[pa("name", Fl::ColumnKey), pa("index", Fl::ColumnKey)],
        1,
        4,
    ),
    sp(
        "convert_encoding",
        Fl::Str,
        &[pa("to", Fl::Charset), pa("from", Fl::Charset)],
        2,
        4,
    ),
    sp(
        "date",
        Fl::Date,
        &[pa("format", Fl::DateFormat), pa("timezone", Fl::Timezone)],
        0,
        8,
    ),
    sp(
        "date_modify",
        Fl::Date,
        &[pa("modifier", Fl::Modifier)],
        1,
        3,
    ),
    sp("default", Fl::Maybe, &[pa("default", Fl::Any)], 0, 5),
    sp("escape", Fl::Str, ESCAPE_PARAMS, 0, 6),
    sp("e", Fl::Str, ESCAPE_PARAMS, 0, 2),
    sp("filter", Fl::Iter, &[pa("arrow", Fl::Arrow1)], 1, 4),
    sp("find", Fl::Iter, &[pa("arrow", Fl::Arrow1)], 1, 3),
    sp("first", Fl::Seqish, &[], 0, 4),
    spv("format", Fl::Format, &[], 0, pa("values", Fl::Any), 6),
    spv("invoke", Fl::Closure, &[], 0, pa("arguments", Fl::Any), 2),
    sp(
        "join",
        Fl::Iter,
        &[pa("glue", Fl::Glue), pa("and", Fl::Glue)],
        0,
        5,
    ),
    sp(
        "json_encode",
        Fl::Any,
        &[pa("flags", Fl::JsonFlags), pa("depth", Fl::Depth)],
        0,
        5,
    ),
    sp("keys", Fl::Iter, &[], 0, 3),
    sp("last", Fl::Seqish, &[], 0, 3),
    sp("length", Fl::Any, &[], 0, 5),
    sp("lower", Fl::Str, &[], 0, 4),
    sp("map", Fl::Iter, &[pa("arrow", Fl::Arrow1)], 1, 4),
    spv("merge", Fl::Iter, &[], 0, pa("arrays", Fl::Iter), 5),
    sp("nl2br", Fl::Nl, &[], 0, 3),
    sp(
        "number_format",
        Fl::Num,
        &[
            pa("decimal", Fl::Decimals),
            pa("decimal_point", Fl::Separator),
            pa("thousand_sep", Fl::Separator),
        ],
        0,
        6,
    ),
    sp("raw", Fl::Any, &[], 0, 1),
    sp(
        "reduce",
        Fl::Iter,
        &[pa("arrow", Fl::Arrow3), pa("initial", Fl::Any)],
        1,
        3,
    ),
    sp("replace", Fl::Str, &[pa("from", Fl::ReplaceMap)], 1, 4),
    sp(
        "reverse",
        Fl::Seqish,
        &[pa("preserve_keys", Fl::Bool)],
        0,
        4,
    ),
    sp(
        "round",
        Fl::Num,
        &[pa("precision", Fl::Precision), pa("method", Fl::Method)],
        0,
        6,
    ),
    sp("shuffle", Fl::Any, &[], 0, 1),
    sp(
        "slice",
        Fl::Seqish,
        &[
            pa("start", Fl::Offset),
            pa("length", Fl::Offset),
            pa("preserve_keys", Fl::Bool),
        ],
        1,
        6,
    ),
    sp("sort", Fl::Iter, &[pa("arrow", Fl::Arrow2)], 0, 5),
    sp("spaceless", Fl::Html, &[], 0, 1),
    sp(
        "split",
        Fl::Str,
        &[pa("delimiter", Fl::Delimiter), pa("limit", Fl::Limit)],
        1,
        5,
    ),
    sp(
        "striptags",
        Fl::Html,
        &[pa("allowable_tags", Fl::Tags)],
        0,
        3,
    ),
    sp("title", Fl::Str, &[], 0, 4),
    sp(
        "trim",
        Fl::Str,
        &[pa("character_mask", Fl::TrimMask), pa("side", Fl::Side)],
        0,
        5,
    ),
    sp("upper", Fl::Str, &[], 0, 4),
    sp("url_encode", Fl::UrlInput, &[], 0, 4),
];

const FUNCTIONS: &[Spec] = &[
    sp(
        "range",
        Fl::Any,
        &[pa("low", Fl::Any), pa("high", Fl::Any), pa("step", Fl::Any)],
        2,
        6,
    ),
    spv(
        "max",
        Fl::Any,
        &[pa("value", Fl::Any)],
        1,
        pa("values", Fl::Any),
        5,
    ),
    spv(
        "min",
        Fl::Any,
        &[pa("value", Fl::Any)],
        1,
        pa("values", Fl::Any),
        5,
    ),
    sp(
        "cycle",
        Fl::Any,
        &[pa("values", Fl::Iter), pa("position", Fl::Position)],
        2,
        3,
    ),
    sp(
        "constant",
        Fl::Any,
        &[
            pa("constant", Fl::ConstantName),
            pa("object", Fl::Object),
            pa("check_defined", Fl::Bool),
        ],
        1,
        5,
    ),
    sp(
        "date",
        Fl::Any,
        &[pa("date", Fl::Date), pa("timezone", Fl::Timezone)],
        0,
        5,
    ),
    sp("random", Fl::Any, &[], 0, 2),
    sp(
        "include",
        Fl::Any,
        &[
            pa("template", Fl::TemplateName),
            pa("variables", Fl::TemplateVars),
            pa("with_context", Fl::Bool),
            pa("ignore_missing", Fl::Bool),
            pa("sandboxed", Fl::Bool),
        ],
        1,
        3,
    ),
    sp(
        "include_only",
        Fl::Any,
        &[
            pa("template", Fl::TemplateName),
            pa("variables", Fl::TemplateVars),
            pa("ignore_missing", Fl::Bool),
        ],
        1,
        2,
    ),
    sp(
        "source",
        Fl::Any,
        &[pa("name", Fl::TemplateName), pa("ignore_missing", Fl::Bool)],
        1,
        2,
    ),
    sp(
        "block",
        Fl::Any,
        &[pa("name", Fl::BlockName), pa("template", Fl::TemplateName)],
        1,
        2,
    ),
    sp(
        "attribute",
        Fl::Any,
        &[
            pa("variable", Fl::Object),
            pa("attribute", Fl::AttributeName),
            pa("arguments", Fl::AttributeArgs),
        ],
        2,
        2,
    ),
    sp(
        "template_from_string",
        Fl::Any,
        &[pa("template", Fl::TemplateSource), pa("name", Fl::Str)],
        1,
        2,
    ),
    sp("enum", Fl::Any, &[pa("enum", Fl::EnumName)], 1, 2),
    sp("enum_cases", Fl::Any, &[pa("enum", Fl::EnumName)], 1, 2),
];

/// A test: its name, what its input prefers and its argument (for `same as`, `divisible by`,
/// `constant`).
#[derive(Clone, Copy, Debug)]
struct TestSpec {
    name: &'static str,
    input: Fl,
    arg: Option<Fl>,
    weight: u32,
}

const fn ts(name: &'static str, input: Fl, arg: Option<Fl>, weight: u32) -> TestSpec {
    TestSpec {
        name,
        input,
        arg,
        weight,
    }
}

const TESTS: &[TestSpec] = &[
    ts("constant", Fl::Any, Some(Fl::ConstantName), 4),
    ts("defined", Fl::Any, None, 5),
    ts("divisible by", Fl::Num, Some(Fl::Divisor), 4),
    ts("empty", Fl::Maybe, None, 5),
    ts("even", Fl::Num, None, 3),
    ts("odd", Fl::Num, None, 3),
    ts("iterable", Fl::Any, None, 3),
    ts("mapping", Fl::Iter, None, 4),
    ts("none", Fl::Maybe, None, 1),
    ts("null", Fl::Maybe, None, 2),
    ts("same as", Fl::Any, Some(Fl::Any), 4),
    ts("sequence", Fl::Iter, None, 4),
    ts("true", Fl::Any, None, 2),
];

/// Where a value comes from.
#[derive(Clone, Copy)]
enum Src {
    Pool(&'static [&'static str]),
    Vars(u32),
    Nested,
    Any,
    Special,
}

/// How a case is put together.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Profile {
    /// Anything goes: wrong types, named/spread/missing/extra arguments, nested calls.
    Mixed,
    /// Well-typed arguments (positional or correctly named) from the pools of each parameter: the
    /// results, not the errors, are compared.
    Valid,
    /// One callable, called by every statement with other well-typed arguments.
    Focus,
}

/// The callable of a [`Profile::Focus`] case.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Focus {
    Filter(usize),
    Function(usize),
    Test(usize),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Layout {
    /// The statements are the template.
    Plain,
    /// `{% extends 'p.twig' %}` with the statements in block `blk` (`parent()` works).
    Child,
}

#[derive(Clone, Copy)]
enum Statement {
    Print,
    Dump,
    Json,
    For,
    Test,
    If,
    Set,
    Apply,
}

const MAX_DEPTH: usize = 3;

/// The helper templates every case has.
const B_TWIG: &str = "B[{{ v|default('-') }}]{% block blk %}in-b{% endblock %}";
const P_TWIG: &str =
    "<{% block blk %}P{{ v|default('') }}{% endblock %}|{% block other %}O{% endblock %}>";
const MACROS: &str = "{% macro mk(v) %}{{ v }}{% endmacro %}{% macro mkb() %}<b>b&amp;</b>{% endmacro %}{% macro mke() %}{% endmacro %}";

// ---- the generator --------------------------------------------------------------------------------

struct Generator<'r> {
    rng: &'r mut Rng,
    layout: Layout,
    vars: BTreeSet<&'static str>,
    macros: bool,
    own_block: bool,
    debug: bool,
    /// The date input of the current `date` call is known not to depend on the current time
    /// (microsecond formats allowed).
    fixed_date: bool,
    /// `dump()` may be used (see [`Generator::case`]).
    dump_allowed: bool,
    profile: Profile,
    focus: Option<Focus>,
}

impl<'r> Generator<'r> {
    fn new(rng: &'r mut Rng) -> Generator<'r> {
        Generator {
            rng,
            layout: Layout::Plain,
            vars: BTreeSet::new(),
            macros: false,
            own_block: false,
            debug: false,
            fixed_date: false,
            dump_allowed: true,
            profile: Profile::Mixed,
            focus: None,
        }
    }

    /// Whether arguments are well-typed in this case.
    fn valid(&self) -> bool {
        self.profile != Profile::Mixed
    }

    fn case(&mut self, id: &str) -> Json {
        self.layout = self
            .rng
            .weighted(&[(90, Layout::Plain), (10, Layout::Child)]);
        let autoescape = self.rng.weighted(&[
            (55, ""),
            (25, "false"),
            (4, "js"),
            (3, "css"),
            (3, "url"),
            (3, "html_attr"),
            (3, "name"),
            (4, "html"),
        ]);
        // `dump()` output is escaped by these strategies, and escaped object handles are not
        // recognised as the known deviation
        self.dump_allowed = matches!(autoescape, "" | "false" | "name" | "html");
        self.profile = self.rng.weighted(&[
            (45, Profile::Mixed),
            (40, Profile::Valid),
            (15, Profile::Focus),
        ]);
        let mut count = self.rng.weighted(&[(55, 1), (35, 2), (10, 3)]);
        if self.profile == Profile::Focus {
            self.focus = Some(self.pick_focus());
            count = 2 + self.rng.below(3);
        }
        // the id names the profile: `m`ixed, `v`alid, `f`ocus
        let id = &format!(
            "{id}{}",
            match self.profile {
                Profile::Mixed => "m",
                Profile::Valid => "v",
                Profile::Focus => "f",
            }
        );
        let mut body = String::new();
        for _ in 0..count {
            body.push_str(&self.statement());
        }
        // `dump()` of the whole context, unless it holds a TemplateWrapper
        if self.dump_allowed && self.rng.chance(2) && !self.vars.iter().any(|v| *v == "tw") {
            self.debug = true;
            body.push_str("{{ dump() }}");
        }
        let mut index = String::new();
        if self.macros {
            index.push_str(MACROS);
        }
        match self.layout {
            Layout::Plain => {
                index.push_str(&body);
                if self.own_block {
                    index.push_str("{% block own %}OWN{% endblock %}");
                }
            }
            Layout::Child => {
                index.push_str("{% extends 'p.twig' %}{% block blk %}");
                index.push_str(&body);
                index.push_str("{% endblock %}");
            }
        }
        // PHP compiles an identical source once per process: later copies would not raise the
        // compile-time deprecations again
        index.push_str(&format!("{{# {id} #}}"));
        let templates = Json::Object(vec![
            ("index.twig".to_owned(), Json::str(index)),
            ("b.twig".to_owned(), Json::str(B_TWIG)),
            ("e.twig".to_owned(), Json::str("")),
            ("p.twig".to_owned(), Json::str(P_TWIG)),
        ]);

        let mut fields = vec![
            ("id".to_owned(), Json::str(id)),
            ("env".to_owned(), Json::str("plain")),
            ("templates".to_owned(), templates),
        ];
        let used: Vec<&'static Var> = self.vars.iter().filter_map(|n| var(n)).collect();
        let needs_php = used.iter().any(|v| v.json.is_none()) || self.rng.chance(5);
        if needs_php {
            let entries: Vec<String> = used
                .iter()
                .map(|v| {
                    let value = match v.json {
                        Some(text) => php_of_json(&Json::parse(text).expect("valid JSON")),
                        None => v.php.to_owned(),
                    };
                    format!("'{}' => {value}", v.name)
                })
                .collect();
            fields.push((
                "data_php".to_owned(),
                Json::str(format!("return [{}];", entries.join(", "))),
            ));
        } else {
            let entries: Vec<(String, Json)> = used
                .iter()
                .map(|v| {
                    (
                        v.name.to_owned(),
                        Json::parse(v.json.expect("JSON variable")).expect("valid JSON"),
                    )
                })
                .collect();
            fields.push(("data_json".to_owned(), Json::Object(entries)));
        }
        // the classes of tests/IntegrationTest.php, also when a template names one: an earlier case
        // of the same PHP process may have declared them
        if used.iter().any(|v| v.tags & TCLS != 0) || index_source_names_test_class(&fields) {
            fields.push(("test_classes".to_owned(), Json::Bool(true)));
        }

        let mut config = Vec::new();
        match autoescape {
            "" => {}
            "false" => config.push(("autoescape".to_owned(), Json::Bool(false))),
            strategy => config.push(("autoescape".to_owned(), Json::str(strategy))),
        }
        if self.rng.chance(30) {
            config.push(("strict_variables".to_owned(), Json::Bool(true)));
        }
        if self.debug || self.rng.chance(5) {
            config.push(("debug".to_owned(), Json::Bool(true)));
        }
        if self.rng.chance(8) {
            let charset = *self.rng.pick(&[
                "ISO-8859-1",
                "ISO-8859-1",
                "ISO-8859-1",
                "utf-8",
                "ASCII",
                "UTF8",
                "Windows-1252",
            ]);
            config.push(("charset".to_owned(), Json::str(charset)));
        }
        if self.rng.chance(2) {
            config.push(("optimizations".to_owned(), Json::int(0)));
        }
        fields.push(("config".to_owned(), Json::Object(config)));
        fields.push((
            "extensions".to_owned(),
            Json::Array(vec![Json::str("debug"), Json::str("string_loader")]),
        ));
        if self.rng.chance(10) {
            fields.push(("errors".to_owned(), Json::str("throw")));
        }
        if self.rng.chance(10) {
            let timezone = *self.rng.pick(&[
                "Europe/Paris",
                "America/New_York",
                "Asia/Kathmandu",
                "Australia/Lord_Howe",
                "Pacific/Apia",
                "UTC",
            ]);
            fields.push(("timezone".to_owned(), Json::str(timezone)));
        }
        if self.rng.chance(4) {
            fields.push(("sandbox".to_owned(), self.sandbox()));
        }
        Json::Object(fields)
    }

    /// A global sandbox whose policy allows most of what the templates use.
    fn sandbox(&mut self) -> Json {
        let mut filters: Vec<&str> = FILTERS.iter().map(|s| s.name).collect();
        filters.push("escape");
        let mut functions: Vec<&str> = FUNCTIONS.iter().map(|s| s.name).collect();
        functions.extend(["dump", "parent"]);
        let mut tests: Vec<&str> = TESTS.iter().map(|t| t.name).collect();
        let tags = vec![
            "for", "if", "set", "apply", "macro", "block", "extends", "import",
        ];
        // sometimes one name is missing
        match self.rng.below(6) {
            0 => {
                let i = self.rng.below(filters.len());
                filters.remove(i);
            }
            1 => {
                let i = self.rng.below(functions.len());
                functions.remove(i);
            }
            2 => {
                let i = self.rng.below(tests.len());
                tests.remove(i);
            }
            _ => {}
        }
        let list = |names: &[&str]| Json::Array(names.iter().map(|n| Json::str(*n)).collect());
        let properties = Json::Object(vec![(
            "stdClass".to_owned(),
            Json::Array(vec![Json::str("a"), Json::str("id"), Json::str("name")]),
        )]);
        let methods = Json::Object(vec![(
            "Twig\\Tests\\ToStringStub".to_owned(),
            Json::Array(vec![Json::str("__toString")]),
        )]);
        let mut policy = vec![
            ("tags".to_owned(), list(&tags)),
            ("filters".to_owned(), list(&filters)),
            ("functions".to_owned(), list(&functions)),
            ("tests".to_owned(), list(&tests)),
            ("methods".to_owned(), methods),
            ("properties".to_owned(), properties),
        ];
        if self.rng.chance(30) {
            policy.push(("strict".to_owned(), Json::Bool(true)));
        }
        Json::Object(vec![
            ("global".to_owned(), Json::Bool(true)),
            ("policy".to_owned(), Json::Object(policy)),
        ])
    }

    fn statement(&mut self) -> String {
        let kind = match self.focus {
            Some(Focus::Test(_)) => *self.rng.pick(&[
                Statement::Test,
                Statement::Test,
                Statement::Dump,
                Statement::If,
            ]),
            Some(_) => self.rng.weighted(&[
                (30, Statement::Print),
                (40, Statement::Dump),
                (20, Statement::Json),
                (10, Statement::For),
            ]),
            None if self.profile == Profile::Valid => self.rng.weighted(&[
                (30, Statement::Print),
                (30, Statement::Dump),
                (16, Statement::Json),
                (8, Statement::For),
                (10, Statement::Test),
                (3, Statement::If),
                (3, Statement::Apply),
            ]),
            None => self.rng.weighted(&[
                (34, Statement::Print),
                (20, Statement::Dump),
                (12, Statement::Json),
                (8, Statement::For),
                (12, Statement::Test),
                (6, Statement::If),
                (4, Statement::Set),
                (4, Statement::Apply),
            ]),
        };
        match kind {
            Statement::Print => format!("{{{{ {} }}}}", self.call(0)),
            Statement::Dump if !self.dump_allowed => format!("{{{{ {} }}}}", self.call(0)),
            Statement::Dump => {
                self.debug = true;
                match self.rng.weighted(&[(80, 0), (12, 1), (8, 2)]) {
                    1 => format!("{{{{ dump({}, {}) }}}}", self.call(0), self.any(1)),
                    // named arguments of the variadic `dump()` (snake case is required)
                    2 if !self.valid() => {
                        let name = *self.rng.pick(&["fooBar", "foo_bar", "x", "Value", "a_b_c"]);
                        format!("{{{{ dump({name}={}) }}}}", self.call(0))
                    }
                    _ => format!("{{{{ dump({}) }}}}", self.call(0)),
                }
            }
            Statement::Json => format!("{{{{ {}|json_encode }}}}", self.call(0)),
            Statement::For => format!(
                "{{% for k, v in {} %}}[{{{{ k }}}}={{{{ v }}}}]{{% else %}}E{{% endfor %}}",
                self.call(0)
            ),
            Statement::Test if self.focus.is_none() && self.rng.chance(12) => {
                // the operators that call arrows like the filters do
                let op = *self.rng.pick(&["has some", "has every"]);
                let subject = self.input(Fl::Iter, 0);
                let arrow = self.flavored(Fl::Arrow1, 1);
                format!("{{{{ ({subject} {op} {arrow}) ? 'T' : 'F' }}}}")
            }
            Statement::Test => {
                let test = self.test(0);
                if self.rng.chance(50) {
                    format!("{{{{ {test} ? 'T' : 'F' }}}}")
                } else {
                    format!("{{{{ {test} }}}}")
                }
            }
            Statement::If => format!(
                "{{% if {} %}}T{{% else %}}F{{% endif %}}",
                self.callable_any(0)
            ),
            Statement::Set => format!("{{% set r = {} %}}{{{{ r }}}}", self.call(0)),
            Statement::Apply => {
                let spec = *self.rng.pick(&[
                    "upper",
                    "lower",
                    "title",
                    "capitalize",
                    "escape",
                    "escape('js')",
                    "trim",
                    "spaceless",
                    "nl2br",
                    "striptags",
                    "url_encode",
                    "reverse",
                    "length",
                    "first",
                    "split(',')|join('|')",
                    "slice(1, 3)",
                    "replace({'a': 'b'})",
                    "format(1, 2)",
                    "number_format(2)",
                    "round(1)",
                    "json_encode",
                    "default('d')",
                    "convert_encoding('ISO-8859-1', 'UTF-8')",
                    "date('Y')",
                    "abs",
                    "raw",
                    "upper|lower",
                ]);
                let inner = if self.rng.chance(50) {
                    format!("{{{{ {} }}}}", self.call(1))
                } else {
                    let text = *self.rng.pick(&[
                        " a,b <b>x</b> ",
                        "été\n",
                        "12.5",
                        "",
                        "  <p> x </p>  ",
                        "a&b",
                    ]);
                    text.to_owned()
                };
                format!("{{% apply {spec} %}}{inner}{{% endapply %}}")
            }
        }
    }

    /// A filter, function or test call (tests parenthesized).
    fn callable_any(&mut self, depth: usize) -> String {
        if self.rng.chance(20) {
            self.test(depth)
        } else {
            self.call(depth)
        }
    }

    /// A filter, function or test whose name is misspelled (`Unknown "x" filter. Did you mean ...?`).
    fn misspelled(&mut self, depth: usize) -> String {
        const FILTERS: &[&str] = &[
            "upperr",
            "lenght",
            "revers",
            "joni",
            "escpae",
            "number_formatt",
            "sortt",
            "slise",
            "datee",
            "e_",
            "raw_",
            "Upper",
            "json",
            "url",
            "spacless",
            "trim_",
            "rounds",
        ];
        const FUNCTIONS: &[&str] = &[
            "rang", "maxx", "constan", "cycl", "dat", "includ", "Range", "sourc", "blocks", "enums",
        ];
        const TESTS: &[&str] = &[
            "evn",
            "iterabl",
            "same",
            "divisible",
            "nul",
            "definedd",
            "same as as",
            "empy",
            "Even",
            "sequenc",
        ];
        let value = self.any(depth + 1);
        let value = Self::operand(value);
        match self.rng.below(3) {
            0 => format!("{value}|{}", self.rng.pick(FILTERS)),
            1 => format!("{}({value})", self.rng.pick(FUNCTIONS)),
            _ => format!("({value} is {})", self.rng.pick(TESTS)),
        }
    }

    /// A filter or function call.
    fn call(&mut self, depth: usize) -> String {
        if !self.valid() && self.rng.chance(2) {
            return self.misspelled(depth);
        }
        match self.focus {
            Some(Focus::Filter(i)) if depth == 0 => return self.filter_spec(FILTERS[i], depth),
            Some(Focus::Function(i)) if depth == 0 => {
                return self.function_spec(FUNCTIONS[i], depth);
            }
            _ => {}
        }
        if self.rng.chance(70) {
            self.filter(depth)
        } else {
            self.function(depth)
        }
    }

    /// The callable of a [`Profile::Focus`] case.
    fn pick_focus(&mut self) -> Focus {
        match self.rng.weighted(&[(65, 0), (22, 1), (13, 2)]) {
            0 => {
                let weights: Vec<(u32, usize)> = FILTERS
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| s.name != "shuffle")
                    .map(|(i, s)| (s.weight, i))
                    .collect();
                Focus::Filter(self.rng.weighted(&weights))
            }
            1 => {
                let weights: Vec<(u32, usize)> = FUNCTIONS
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| s.name != "random")
                    .map(|(i, s)| (s.weight, i))
                    .collect();
                Focus::Function(self.rng.weighted(&weights))
            }
            _ => {
                let weights: Vec<(u32, usize)> = TESTS
                    .iter()
                    .enumerate()
                    .map(|(i, t)| (t.weight, i))
                    .collect();
                Focus::Test(self.rng.weighted(&weights))
            }
        }
    }

    fn filter(&mut self, depth: usize) -> String {
        let weights: Vec<(u32, usize)> = FILTERS
            .iter()
            .enumerate()
            .map(|(i, s)| (s.weight, i))
            .collect();
        let spec = FILTERS[self.rng.weighted(&weights)];
        self.filter_spec(spec, depth)
    }

    fn filter_spec(&mut self, spec: Spec, depth: usize) -> String {
        let weights: Vec<(u32, usize)> = FILTERS
            .iter()
            .enumerate()
            .map(|(i, s)| (s.weight, i))
            .collect();
        if spec.name == "shuffle" {
            let input = *self.rng.pick(SHUFFLE_INPUTS);
            return format!("{input}|shuffle");
        }
        if spec.name == "invoke" && self.rng.chance(85) {
            // as many arguments as the closure needs, sometimes more
            let (closure, arity) = *self.rng.pick(CLOSURES);
            let count = arity + self.rng.weighted(&[(80, 0), (20, 1)]);
            let args: Vec<String> = (0..count).map(|_| self.any(depth + 1)).collect();
            return format!("{closure}|invoke({})", args.join(", "));
        }
        // nested calls restore the flag of their caller
        let entry = self.fixed_date;
        self.fixed_date = false;
        let input = self.input(spec.input, depth);
        let args = self.arguments(&spec, depth, true);
        let mut out = format!("{input}|{}{args}", spec.name);
        // a date that depends on "now": its microseconds are not replayed
        if spec.name == "date_modify" && !self.fixed_date {
            out.push_str("|date('Y-m-d H:i:s T')");
        }
        // a second filter on the result now and then
        if depth < MAX_DEPTH && self.rng.chance(12) {
            let next = FILTERS[self.rng.weighted(&weights)];
            // (`date_modify` of a date that may depend on "now" is only printed through `date`)
            if next.name != "shuffle" && next.name != "date_modify" {
                let args = self.arguments(&next, depth + 1, true);
                out = format!("{out}|{}{args}", next.name);
            }
        }
        self.fixed_date = entry;
        out
    }

    fn function(&mut self, depth: usize) -> String {
        let weights: Vec<(u32, usize)> = FUNCTIONS
            .iter()
            .enumerate()
            .map(|(i, s)| (s.weight, i))
            .collect();
        let spec = FUNCTIONS[self.rng.weighted(&weights)];
        self.function_spec(spec, depth)
    }

    fn function_spec(&mut self, spec: Spec, depth: usize) -> String {
        if self.layout == Layout::Child && self.rng.chance(15) {
            return self.wrap_string("parent()");
        }
        match spec.name {
            "random" => {
                let args = *self.rng.pick(RANDOM_ARGS);
                format!("random({args})")
            }
            "range" => self.range(),
            "date" => {
                let entry = self.fixed_date;
                self.fixed_date = false;
                let args = self.arguments(&spec, depth, false);
                // a spread may leave out the date: "now"
                if args.contains("...") {
                    self.fixed_date = false;
                }
                let call = format!("date{args}");
                let out = self.wrap_date(&call);
                self.fixed_date = entry;
                out
            }
            "enum" => {
                let call = format!("enum{}", self.arguments(&spec, depth, false));
                match self.rng.below(6) {
                    0 => format!("{call}.name"),
                    1 => format!("{call}.value"),
                    2 => {
                        self.use_var("en");
                        format!("({call} is same as(en))")
                    }
                    3 => format!("{call}|json_encode"),
                    4 => format!("{call}.cases|length"),
                    _ => call,
                }
            }
            "enum_cases" => {
                let call = format!("enum_cases{}", self.arguments(&spec, depth, false));
                match self.rng.below(5) {
                    0 => format!("{call}|length"),
                    1 => format!("{call}|map(c => c.name)|join(',')"),
                    2 => format!("{call}|first"),
                    3 => format!("{call}|json_encode"),
                    _ => call,
                }
            }
            "template_from_string" => {
                let call = format!(
                    "template_from_string{}",
                    self.arguments(&spec, depth, false)
                );
                match self.rng.below(4) {
                    0 | 1 => format!("include({call})"),
                    2 => format!("{call}.render({})", self.flavored(Fl::TemplateVars, depth)),
                    _ => format!("block('b', {call})"),
                }
            }
            "block" => {
                let name = match (self.layout, self.rng.below(5)) {
                    (Layout::Plain, 0) => {
                        self.own_block = true;
                        "'own'".to_owned()
                    }
                    (Layout::Child, 0) => "'other'".to_owned(),
                    _ => {
                        let name = *self.rng.pick(BLOCK_NAMES);
                        if name == "'blk'" && self.layout == Layout::Child {
                            "'other'".to_owned()
                        } else {
                            name.to_owned()
                        }
                    }
                };
                let call = if self.rng.chance(55) || self.layout == Layout::Child {
                    let template = self.flavored(Fl::TemplateName, depth + 1);
                    if self.rng.chance(10) {
                        format!("block(name={name}, template={template})")
                    } else {
                        format!("block({name}, {template})")
                    }
                } else if name == "'blk'" || name == "'other'" {
                    format!("block({name}, 'b.twig')")
                } else {
                    format!("block({name})")
                };
                self.wrap_string(&call)
            }
            "max" | "min" if self.valid() => {
                let args = if self.rng.chance(40) {
                    self.flavored(Fl::Iter, depth + 1)
                } else {
                    let n = 2 + self.rng.below(2);
                    let values: Vec<String> = (0..n)
                        .map(|_| {
                            self.from(
                                &[
                                    (30, Src::Pool(INTS)),
                                    (25, Src::Pool(FLOATS)),
                                    (25, Src::Pool(GOOD_STRINGS)),
                                    (10, Src::Pool(&["null", "true", "false", "[]", "[1, 2]"])),
                                    (10, Src::Vars(NUM | STR | BOOL | NULLISH)),
                                ],
                                depth + 1,
                            )
                        })
                        .collect();
                    values.join(", ")
                };
                format!("{}({args})", spec.name)
            }
            "include" | "include_only" | "source" | "constant" | "cycle" | "attribute" | "max"
            | "min" => {
                let args = self.arguments(&spec, depth, false);
                let call = format!("{}{args}", spec.name);
                if matches!(spec.name, "include" | "include_only" | "source") {
                    self.wrap_string(&call)
                } else {
                    call
                }
            }
            _ => {
                let args = self.arguments(&spec, depth, false);
                format!("{}{args}", spec.name)
            }
        }
    }

    /// Sometimes a filter on a string-valued call.
    fn wrap_string(&mut self, call: &str) -> String {
        match self.rng.below(8) {
            0 => format!("{call}|upper"),
            1 => format!("{call}|length"),
            2 => format!("{call}|trim"),
            3 => format!("{call}|escape"),
            _ => call.to_owned(),
        }
    }

    /// What to do with a `DateTime` returned by `date()`: its microseconds are only printed when it
    /// does not depend on the current time.
    fn wrap_date(&mut self, call: &str) -> String {
        if !self.fixed_date {
            return match self.rng.below(4) {
                0 => format!("{call}.timestamp"),
                _ => {
                    let format = self.date_format();
                    format!("{call}|date({format})")
                }
            };
        }
        match self.rng.below(10) {
            0..=4 => {
                let format = self.date_format();
                format!("{call}|date({format})")
            }
            5 => format!("{call}.timestamp"),
            6 => format!("{call}.timezone.name"),
            7 => format!("{call}|date_modify({})|date('c')", self.pool(MODIFIERS)),
            8 => format!("{call}|json_encode"),
            _ => call.to_owned(),
        }
    }

    fn range(&mut self) -> String {
        const SMALL: &[&str] = &[
            "0", "1", "2", "3", "5", "10", "(-1)", "(-3)", "20", "100", "'2'", "'10'", "' 1'",
        ];
        const CHARS: &[&str] = &[
            "'a'", "'e'", "'z'", "'A'", "'Z'", "'0'", "'9'", "'5'", "''", "'aa'", "'cc'", "'é'",
            "'1'", "'3'", "'%'", "'~'", "'1.5'",
        ];
        const FLOATS: &[&str] = &[
            "0.0", "0.5", "1.5", "2.5", "0.1", "(-1.5)", "1e3", "1.0e-3", "1.0", "'0.5'",
        ];
        const STEPS: &[&str] = &[
            "1",
            "2",
            "3",
            "0",
            "(-1)",
            "(-2)",
            "0.5",
            "0.25",
            "1.5",
            "100",
            "'1'",
            "'2'",
            "'a'",
            "null",
            "true",
            "0.1",
            "9223372036854775807",
            "1e-300",
            "1.0",
            "(-0.5)",
        ];
        const HUGE: &[&str] = &[
            "9223372036854775807",
            "(-9223372036854775807)",
            "1e10",
            "1e19",
            "1e309",
            "4294967296",
        ];
        const ODD: &[&str] = &[
            "null", "true", "false", "[]", "[1]", "'abc'", "' '", "'5a'", "1e309", "(-1e309)",
        ];
        // with a huge bound only steps that fail fast (or count past the maximum array size)
        const HUGE_STEPS: &[&str] = &["1", "0.5", "'1'", "1e-300", "9223372036854775807", "0"];
        let pattern = self.rng.below(10);
        let (low, high) = match pattern {
            0..=3 => (self.pool(SMALL), self.pool(SMALL)),
            4 | 5 => (self.pool(CHARS), self.pool(CHARS)),
            6 => (self.pool(FLOATS), self.pool(FLOATS)),
            7 => (self.pool(SMALL), self.pool(HUGE)),
            8 => (self.pool(ODD), self.pool(SMALL)),
            _ => (self.pool(SMALL), self.pool(CHARS)),
        };
        let mut args = vec![low, high];
        if self.rng.chance(45) {
            args.push(self.pool(if pattern == 7 { HUGE_STEPS } else { STEPS }));
        }
        match self.rng.below(12) {
            0 => {
                let names = ["low", "high", "step"];
                let named: Vec<String> = args
                    .iter()
                    .zip(names)
                    .map(|(v, n)| format!("{n}={v}"))
                    .collect();
                format!("range({})", named.join(", "))
            }
            1 => format!("range(start={}, end={})", args[0], args[1]),
            2 => format!("range(...[{}])", args.join(", ")),
            3 => format!("range({})", args[0]),
            4 => format!("({}..{})", args[0], args[1]),
            _ => format!("range({})", args.join(", ")),
        }
    }

    /// A test expression, parenthesized.
    fn test(&mut self, depth: usize) -> String {
        let weights: Vec<(u32, usize)> = TESTS
            .iter()
            .enumerate()
            .map(|(i, t)| (t.weight, i))
            .collect();
        let spec = match self.focus {
            Some(Focus::Test(i)) if depth == 0 => TESTS[i],
            _ => TESTS[self.rng.weighted(&weights)],
        };
        let not = if self.rng.chance(30) { "not " } else { "" };
        let subject = if spec.name == "defined" {
            self.defined_subject(depth)
        } else {
            self.input(spec.input, depth)
        };
        let args = match spec.arg {
            Some(fl) => {
                let value = if spec.name == "constant" {
                    self.constant_test_args(depth)
                } else {
                    self.flavored(fl, depth + 1)
                };
                let forms: &[(u32, u8)] = if self.valid() {
                    &[(80, 0), (20, 1)]
                } else {
                    &[(72, 0), (12, 1), (4, 2), (3, 3), (4, 4), (5, 5)]
                };
                match self.rng.weighted(forms) {
                    // `x is divisible by 3`: a bare argument parsed at precedence 100
                    1 if spec.name != "constant" => format!(" {value}"),
                    2 => "()".to_owned(),
                    3 => String::new(),
                    4 => format!("(value={value})"),
                    5 => format!("({value}, {})", self.any(depth + 1)),
                    _ => format!("({value})"),
                }
            }
            None if self.valid() => String::new(),
            None => match self.rng.weighted(&[(88, 0), (6, 1), (3, 2), (3, 3)]) {
                1 => format!("({})", self.any(depth + 1)),
                2 => "()".to_owned(),
                3 => format!("(x={})", self.any(depth + 1)),
                _ => String::new(),
            },
        };
        format!("({subject} is {not}{}{args})", spec.name)
    }

    fn constant_test_args(&mut self, depth: usize) -> String {
        if self.rng.chance(25) {
            let name = self.pool(OBJECT_CONSTANT_NAMES);
            let object = self.flavored(Fl::Object, depth + 1);
            format!("{name}, {object}")
        } else {
            self.pool(CONSTANT_NAMES)
        }
    }

    fn defined_subject(&mut self, depth: usize) -> String {
        const SUBJECTS: &[&str] = &[
            "undef",
            "undef.x",
            "am.a",
            "am.nope",
            "am['b']",
            "al[5]",
            "al.0",
            "obj.a",
            "obj.nope",
            "foo.bar",
            "foo.getFoo",
            "foo.getNull",
            "adeep.a.b.c",
            "adeep.a.x.c",
            "n",
            "al|first",
            "(1 + 1)",
            "'a'",
            "range(1, 2)",
            "range(1, 0)",
            "constant('PHP_EOL')",
            "constant('NOPE')",
            "constant('BAR_NAME', foo)",
            "block('nope', 'b.twig')",
            "block('blk', 'b.twig')",
            "_self",
            "_context",
            "_charset",
            "[]",
            "null",
            "attribute(am, 'a')",
            "attribute(obj, 'nope')",
            "_self.mk",
            "_self.nope",
            "dt.timestamp",
            "m.nope",
        ];
        let subject = *self.rng.pick(SUBJECTS);
        if subject.starts_with("_self.m") {
            self.macros = true;
        }
        if subject == "range(1, 2)" && depth < MAX_DEPTH && self.rng.chance(30) {
            return format!("max({})", self.any(depth + 1));
        }
        // the variables of the subject
        let mut name = String::new();
        for c in subject.chars().chain(std::iter::once(' ')) {
            if c.is_ascii_alphanumeric() || c == '_' {
                name.push(c);
            } else {
                if !name.is_empty() && var(&name).is_some() {
                    self.use_var(&name);
                }
                name.clear();
            }
        }
        subject.to_owned()
    }

    /// The input of a filter or test: preferred kinds of values, sometimes anything.
    fn input(&mut self, fl: Fl, depth: usize) -> String {
        let value = self.flavored(fl, depth + 1);
        Self::operand(value)
    }

    /// `value` usable as the operand of a filter or test.
    fn operand(value: String) -> String {
        let primary = value.starts_with('(')
            || value.starts_with('[')
            || value.starts_with('{')
            || value.starts_with('\'')
            || value.starts_with('"')
            || !value.contains([' ', '-', '|', '.', '?', '=']);
        if primary || is_simple_chain(&value) {
            value
        } else {
            format!("({value})")
        }
    }

    /// The argument list of a call: positional, named, mixed, spread, missing or extra arguments.
    fn arguments(&mut self, spec: &Spec, depth: usize, filter: bool) -> String {
        let mut params: Vec<Param> = spec.params.to_vec();
        if let Some(variadic) = spec.variadic {
            for _ in 0..self.rng.weighted(&[(30, 0), (35, 1), (20, 2), (15, 3)]) {
                params.push(variadic);
            }
        }
        let n = params.len();
        let required = spec.required.min(n);
        let modes: &[(u32, u8)] = if self.valid() {
            &[(78, 0), (12, 1), (6, 2), (4, 3)]
        } else {
            &[
                (64, 0), // positional
                (12, 1), // named
                (6, 2),  // positional then named
                (5, 3),  // spread
                (6, 4),  // one required argument missing
                (6, 5),  // extra arguments
                (1, 6),  // positional after named
            ]
        };
        let mut mode = self.rng.weighted(modes);
        // `default`'s compiled fallback branch is its first argument node: a spread there is PHP code that does
        // not parse (a known mismatch)
        if mode == 3 && spec.name == "default" {
            mode = 0;
        }
        let count = |g: &mut Self| -> usize {
            if n == required || g.rng.chance(40) {
                required
            } else {
                required + 1 + g.rng.below(n - required)
            }
        };
        let values = |g: &mut Self, params: &[Param]| -> Vec<String> {
            params.iter().map(|p| g.flavored(p.fl, depth + 1)).collect()
        };
        let list = match mode {
            1 | 2 if n > 0 => {
                let k = count(self).max(1);
                let positional = if mode == 2 { self.rng.below(k) } else { 0 };
                let mut parts = values(self, &params[..positional]);
                let mut named: Vec<(String, String)> = params[positional..k]
                    .iter()
                    .map(|p| {
                        let (name, fl) = self.argument_name(*p, spec);
                        (name, self.flavored(fl, depth + 1))
                    })
                    .collect();
                // named arguments may come in any order
                if named.len() > 1 && self.rng.chance(30) {
                    named.reverse();
                }
                let colon = self.rng.chance(20);
                for (name, value) in named {
                    parts.push(if colon {
                        format!("{name}: {value}")
                    } else {
                        format!("{name}={value}")
                    });
                }
                parts
            }
            3 => {
                let k = count(self);
                let vs = values(self, &params[..k]);
                match self.rng.below(5) {
                    0 if k > 0 => {
                        let named: Vec<String> = params[..k]
                            .iter()
                            .zip(&vs)
                            .map(|(p, v)| format!("'{}': {v}", p.name))
                            .collect();
                        vec![format!("...{{{}}}", named.join(", "))]
                    }
                    1 => {
                        // mostly sequences: keys that name parameters are rarer
                        let tags = if self.rng.chance(80) { SEQ } else { MAP | TRAV };
                        let v = self.vars_of(tags);
                        vec![format!("...{v}")]
                    }
                    2 if k > 1 => {
                        let mut parts = vs[..1].to_vec();
                        parts.push(format!("...[{}]", vs[1..].join(", ")));
                        parts
                    }
                    3 if k > 0 => {
                        // a normal argument after the spread
                        vec![
                            format!("...[{}]", vs[..k - 1].join(", ")),
                            vs[k - 1].clone(),
                        ]
                    }
                    _ => vec![format!("...[{}]", vs.join(", "))],
                }
            }
            4 if required > 0 => values(self, &params[..required - 1]),
            5 => {
                let mut vs = values(self, &params);
                for _ in 0..1 + self.rng.below(2) {
                    vs.push(self.any(depth + 1));
                }
                vs
            }
            6 if n > 1 => {
                let first = format!(
                    "{}={}",
                    params[0].name,
                    self.flavored(params[0].fl, depth + 1)
                );
                vec![first, self.flavored(params[1].fl, depth + 1)]
            }
            _ => {
                let k = count(self);
                values(self, &params[..k])
            }
        };
        if filter && list.is_empty() && self.rng.chance(55) {
            String::new()
        } else {
            format!("({})", list.join(", "))
        }
    }

    /// The name a named argument is given (usually the PHP name in snake case) and what its value
    /// prefers (another parameter's name takes that parameter's kind of value).
    fn argument_name(&mut self, param: Param, spec: &Spec) -> (String, Fl) {
        let name = param.name;
        let variants: &[(u32, u8)] = if self.valid() {
            &[(88, 0), (12, 1)]
        } else {
            &[(80, 0), (7, 1), (3, 2), (4, 3), (3, 4), (3, 5)]
        };
        match self.rng.weighted(variants) {
            1 => (camel_case(name), param.fl),
            2 => (name.to_ascii_uppercase(), param.fl),
            3 => (format!("{name}x"), param.fl),
            4 if !spec.params.is_empty() => {
                let other = *self.rng.pick(spec.params);
                (other.name.to_owned(), other.fl)
            }
            4 | 5 => ("nope".to_owned(), param.fl),
            _ => (name.to_owned(), param.fl),
        }
    }

    fn pool(&mut self, pool: &[&str]) -> String {
        (*self.rng.pick(pool)).to_owned()
    }

    fn use_var(&mut self, name: &str) {
        if let Some(v) = var(name) {
            self.vars.insert(v.name);
        }
    }

    /// A variable with one of `tags` (any variable when none has them).
    fn vars_of(&mut self, tags: u32) -> String {
        let candidates: Vec<&'static Var> = VARS
            .iter()
            .filter(|v| v.tags & tags != 0 && (tags & NODUMP != 0 || v.tags & NODUMP == 0))
            .collect();
        let chosen = if candidates.is_empty() {
            &VARS[self.rng.below(VARS.len())]
        } else {
            candidates[self.rng.below(candidates.len())]
        };
        self.vars.insert(chosen.name);
        chosen.name.to_owned()
    }

    fn from(&mut self, sources: &[(u32, Src)], depth: usize) -> String {
        match self.rng.weighted(sources) {
            Src::Pool(pool) => self.pool(pool),
            Src::Vars(tags) => self.vars_of(tags),
            Src::Nested if depth < MAX_DEPTH => self.call(depth + 1),
            Src::Nested | Src::Any => self.any(depth),
            Src::Special => self.special(),
        }
    }

    /// A value of any kind.
    fn any(&mut self, depth: usize) -> String {
        match self.rng.weighted(&[
            (10, 0),
            (8, 1),
            (12, 2),
            (11, 3),
            (34, 4),
            (12, 5),
            (5, 6),
            (3, 7),
            (2, 8),
            (3, 9),
        ]) {
            0 => self.pool(INTS),
            1 => self.pool(FLOATS),
            2 => self.pool(STRINGS),
            3 => self.pool(ARRAYS),
            4 => self.vars_of(ANYVAR),
            5 if depth < MAX_DEPTH => self.callable_any(depth + 1),
            6 => self.special(),
            7 => self.pool(&["true", "false", "null", "none"]),
            8 => {
                let v = self.vars_of(MAP | SEQ | OBJ);
                let key = self.pool(&["a", "id", "nope", "0", "x", "name", "getFoo", "c"]);
                if key.starts_with(|c: char| c.is_ascii_digit()) {
                    format!("{v}[{key}]")
                } else {
                    format!("{v}.{key}")
                }
            }
            9 => "undef".to_owned(),
            _ => self.vars_of(ANYVAR),
        }
    }

    /// Markup and other values only templates make.
    fn special(&mut self) -> String {
        match self.rng.below(7) {
            0 | 1 => {
                self.macros = true;
                let text = self.pool(&["'<x>'", "'a&b'", "'été'", "''", "'0'", "1.5", "null"]);
                format!("_self.mk({text})")
            }
            2 => {
                self.macros = true;
                "_self.mkb()".to_owned()
            }
            3 => {
                self.macros = true;
                "_self.mke()".to_owned()
            }
            4 => "include('b.twig')".to_owned(),
            5 => "_self".to_owned(),
            _ => "_charset".to_owned(),
        }
    }

    /// A value for a parameter or input that prefers `fl`.
    fn flavored(&mut self, fl: Fl, depth: usize) -> String {
        // sizes never get arbitrary values: `number_format()` with a huge `decimals` makes one
        // allocation past PHP's hard memory limit (the oracle process dies with the whole batch),
        // `batch()` with a huge size and a fill value appends until the memory limit
        let exempt = matches!(
            fl,
            Fl::Arrow1 | Fl::Arrow2 | Fl::Arrow3 | Fl::Decimals | Fl::BatchSize | Fl::AttributeArgs
        );
        if fl == Fl::Any || (!exempt && !self.valid() && self.rng.chance(15)) {
            return self.any(depth);
        }
        if self.valid()
            && self.rng.chance(92)
            && let Some(value) = self.good(fl, depth)
        {
            return value;
        }
        use Src::{Any, Nested, Pool, Special, Vars};
        match fl {
            Fl::Any => self.any(depth),
            Fl::Str => self.from(
                &[
                    (40, Pool(STRINGS)),
                    (36, Vars(STR | MARKUP | HTML | NL)),
                    (6, Pool(INTS)),
                    (4, Pool(FLOATS)),
                    (8, Nested),
                    (6, Special),
                ],
                depth,
            ),
            Fl::Num => self.from(
                &[
                    (28, Pool(INTS)),
                    (28, Pool(FLOATS)),
                    (30, Vars(NUM)),
                    (6, Pool(STRINGS)),
                    (8, Nested),
                ],
                depth,
            ),
            Fl::Iter => self.from(
                &[
                    (36, Pool(ARRAYS)),
                    (44, Vars(ITER)),
                    (12, Nested),
                    (8, Any),
                ],
                depth,
            ),
            Fl::Seqish => self.from(
                &[
                    (25, Pool(ARRAYS)),
                    (30, Vars(ITER)),
                    (15, Pool(STRINGS)),
                    (15, Vars(STR | MARKUP)),
                    (8, Nested),
                    (7, Any),
                ],
                depth,
            ),
            Fl::Rows => self.from(
                &[
                    (40, Vars(ROWS)),
                    (
                        30,
                        Pool(&[
                            "[{'id': 1, 'n': 'a'}, {'id': 2, 'n': 'b'}, {'n': 'c'}]",
                            "[[1, 2], [3, 4], [5]]",
                            "{'x': {'id': 9, 'name': 'nine'}, 'y': {'id': 8}}",
                            "[{'id': 'k', 'v': 1}, {'id': 'k', 'v': 2}]",
                            "[{'id': 1.5}, {'id': true}, {'id': null}]",
                            "[]",
                        ]),
                    ),
                    (20, Vars(ITER)),
                    (10, Any),
                ],
                depth,
            ),
            Fl::Date => match self.rng.weighted(&[(25, 0), (38, 1), (27, 2), (10, 3)]) {
                0 => self.pool(DATES_NOW),
                1 => {
                    self.fixed_date = true;
                    self.pool(DATES_FIXED)
                }
                2 => {
                    self.fixed_date = true;
                    self.vars_of(DATE | INT)
                }
                _ => self.any(depth),
            },
            Fl::Html => self.from(
                &[
                    (40, Vars(HTML | MARKUP)),
                    (
                        40,
                        Pool(&[
                            "'<p> <b>x</b> </p>'",
                            r#""  <div>\n  <b> x </b>  </div>  ""#,
                            r#"'<a href="x">l</a><br />'"#,
                            "'<?php echo 1; ?>t'",
                            "'<!-- c -->a<b'",
                            "'a < b > c'",
                            "'<b>été</b>'",
                            "'<script>alert(1)</script>'",
                            "''",
                            "'plain'",
                        ]),
                    ),
                    (20, Any),
                ],
                depth,
            ),
            Fl::Nl => self.from(
                &[
                    (40, Vars(NL)),
                    (
                        40,
                        Pool(&[
                            r#""a\nb""#,
                            r#""\r\n""#,
                            r#""x\n\ry\r\rz""#,
                            r#""<a>\n""#,
                            "''",
                            r#""\n\n""#,
                        ]),
                    ),
                    (20, Any),
                ],
                depth,
            ),
            Fl::Maybe => self.from(
                &[
                    (
                        40,
                        Pool(&[
                            "undef", "undef.x", "(undef)", "''", "[]", "0", "'0'", "null", "false",
                            "' '", "0.0",
                        ]),
                    ),
                    (35, Vars(NULLISH | BOOL | STR | SEQ | OBJ)),
                    (25, Any),
                ],
                depth,
            ),
            Fl::Closure => {
                if self.rng.chance(85) {
                    self.rng.pick(CLOSURES).0.to_owned()
                } else {
                    self.any(depth)
                }
            }
            Fl::Arrow1 | Fl::Arrow2 | Fl::Arrow3 => {
                if self.valid() || self.rng.chance(96) {
                    let pool = match fl {
                        Fl::Arrow1 => ARROW1,
                        Fl::Arrow2 => ARROW2,
                        _ => ARROW3,
                    };
                    self.pool(pool)
                } else {
                    self.pool(NON_CALLABLES)
                }
            }
            Fl::Bool => self.pool(BOOLS),
            Fl::BatchSize => self.pool(BATCH_SIZES),
            Fl::ColumnKey => self.pool(COLUMN_KEYS),
            Fl::Charset => self.pool(CHARSETS),
            Fl::DateFormat => self.date_format(),
            Fl::Timezone => self.from(&[(85, Pool(TIMEZONES)), (15, Vars(TZ | STR))], depth),
            Fl::Modifier => self.pool(MODIFIERS),
            Fl::Strategy => self.pool(STRATEGIES),
            Fl::Glue => self.pool(GLUES),
            Fl::JsonFlags => self.pool(JSON_FLAGS),
            Fl::Depth => self.pool(DEPTHS),
            Fl::Decimals => self.pool(DECIMALS),
            Fl::Separator => self.pool(SEPARATORS),
            Fl::Precision => self.pool(PRECISIONS),
            Fl::Method => self.pool(METHODS),
            Fl::ReplaceMap => self.from(
                &[(70, Pool(REPLACE_MAPS)), (20, Vars(ITER)), (10, Any)],
                depth,
            ),
            Fl::Offset => self.pool(OFFSETS),
            Fl::Delimiter => self.pool(DELIMITERS),
            Fl::Limit => self.pool(LIMITS),
            Fl::Tags => self.pool(TAGS),
            Fl::TrimMask => self.pool(TRIM_MASKS),
            Fl::Side => self.pool(SIDES),
            Fl::Format => {
                if self.rng.chance(20) {
                    self.vars_of(FMT | STR)
                } else {
                    self.sprintf_format()
                }
            }
            Fl::TemplateName => {
                if self.rng.chance(12) {
                    self.vars_of(NODUMP)
                } else {
                    self.pool(TEMPLATE_NAMES)
                }
            }
            Fl::TemplateVars => {
                self.from(&[(80, Pool(TEMPLATE_VARS)), (20, Vars(MAP | OBJ))], depth)
            }
            Fl::BlockName => self.pool(BLOCK_NAMES),
            Fl::AttributeName => self.pool(ATTRIBUTE_NAMES),
            Fl::AttributeArgs => self.pool(ATTRIBUTE_ARGS),
            Fl::TemplateSource => self.pool(TEMPLATE_SOURCES),
            Fl::EnumName => self.pool(ENUM_NAMES),
            Fl::ConstantName => self.pool(CONSTANT_NAMES),
            Fl::Object => self.from(&[(70, Vars(OBJ)), (30, Any)], depth),
            Fl::Divisor => self.pool(DIVISORS),
            Fl::Position => self.from(&[(80, Pool(POSITIONS)), (20, Vars(INT))], depth),
            Fl::UrlInput => self.from(
                &[
                    (30, Vars(STR | MAP | SEQ)),
                    (
                        30,
                        Pool(&[
                            "{'a b': 'c&d', 'arr': [1, 2], 'n': null, 't': true, 'f': false, 'nested': {'x': 'y'}}",
                            "{'a': 0.1, 'b': 1e25, 'c': 1.0, 'd': -0.0}",
                            "[5, 6]",
                            "'path-seg*ment ~é'",
                            r#""-_.~!*'();:@&=+$,/?#[]""#,
                            "{'é': 'ü', (1): [[]]}",
                            "[]",
                        ]),
                    ),
                    (20, Pool(STRINGS)),
                    (20, Any),
                ],
                depth,
            ),
        }
    }

    /// A well-typed value for `fl` ([`Profile::Valid`]), `None` where the general pools are fine.
    fn good(&mut self, fl: Fl, depth: usize) -> Option<String> {
        use Src::{Nested, Pool, Vars};
        let pool: &[&str] = match fl {
            Fl::Str => {
                return Some(self.from(
                    &[
                        (50, Pool(GOOD_STRINGS)),
                        (35, Vars(STR | MARKUP | HTML | NL)),
                        (5, Pool(INTS)),
                        (5, Pool(FLOATS)),
                        (5, Src::Special),
                    ],
                    depth,
                ));
            }
            Fl::Num => {
                return Some(self.from(
                    &[(35, Pool(INTS)), (35, Pool(FLOATS)), (30, Vars(NUM))],
                    depth,
                ));
            }
            Fl::Iter => {
                return Some(self.from(
                    &[(45, Pool(GOOD_ARRAYS)), (45, Vars(ITER)), (10, Nested)],
                    depth,
                ));
            }
            Fl::Seqish => {
                return Some(self.from(
                    &[
                        (30, Pool(GOOD_ARRAYS)),
                        (30, Vars(ITER)),
                        (20, Pool(GOOD_STRINGS)),
                        (20, Vars(STR | MARKUP)),
                    ],
                    depth,
                ));
            }
            Fl::Date => {
                return Some(match self.rng.weighted(&[(15, 0), (45, 1), (40, 2)]) {
                    0 => self.pool(DATES_NOW),
                    1 => {
                        self.fixed_date = true;
                        self.pool(GOOD_DATES)
                    }
                    _ => {
                        self.fixed_date = true;
                        self.vars_of(DATE | INT)
                    }
                });
            }
            Fl::Strategy => GOOD_STRATEGIES,
            Fl::Charset => GOOD_CHARSETS,
            Fl::Timezone => GOOD_TIMEZONES,
            Fl::Modifier => GOOD_MODIFIERS,
            Fl::Glue => GOOD_GLUES,
            Fl::JsonFlags => GOOD_JSON_FLAGS,
            Fl::Depth => GOOD_DEPTHS,
            Fl::Decimals => GOOD_DECIMALS,
            Fl::Separator => GOOD_SEPARATORS,
            Fl::Precision => GOOD_PRECISIONS,
            Fl::Method => GOOD_METHODS,
            Fl::ReplaceMap => GOOD_REPLACE_MAPS,
            Fl::Offset => GOOD_OFFSETS,
            Fl::Delimiter => GOOD_DELIMITERS,
            Fl::Limit => GOOD_LIMITS,
            Fl::Tags => GOOD_TAGS,
            Fl::TrimMask => GOOD_TRIM_MASKS,
            Fl::Side => GOOD_SIDES,
            Fl::BatchSize => GOOD_BATCH_SIZES,
            Fl::ColumnKey => GOOD_COLUMN_KEYS,
            Fl::TemplateName => GOOD_TEMPLATE_NAMES,
            Fl::TemplateVars => GOOD_TEMPLATE_VARS,
            Fl::BlockName => GOOD_BLOCK_NAMES,
            Fl::EnumName => GOOD_ENUM_NAMES,
            Fl::ConstantName => GOOD_CONSTANT_NAMES,
            Fl::Divisor => GOOD_DIVISORS,
            Fl::Position => GOOD_POSITIONS,
            Fl::Bool => GOOD_BOOLS,
            _ => return None,
        };
        Some(self.pool(pool))
    }

    /// A PHP date format (or, now and then, a `DateInterval` format).
    fn date_format(&mut self) -> String {
        if self.rng.chance(12) {
            return self.pool(INTERVAL_FORMATS);
        }
        if self.rng.chance(8) {
            return self.pool(&["null", "''", "1", "true", "[]"]);
        }
        let mut format = String::new();
        for _ in 0..1 + self.rng.below(6) {
            if self.fixed_date && self.rng.chance(4) {
                format.push_str(&self.pool(&["u", "v"]));
            } else {
                format.push_str(&self.pool(DATE_PIECES));
            }
        }
        twig_string(&format)
    }

    /// A `sprintf()` format.
    fn sprintf_format(&mut self) -> String {
        let mut format = String::new();
        for _ in 0..1 + self.rng.below(4) {
            if self.rng.chance(35) {
                format.push_str(&self.pool(SPRINTF_TEXT));
            }
            format.push_str(&self.pool(SPRINTF_SPECS));
        }
        twig_string(&format)
    }
}

/// Whether the main template names a helper class of `tests/IntegrationTest.php`.
fn index_source_names_test_class(fields: &[(String, Json)]) -> bool {
    fields
        .iter()
        .find(|(k, _)| k == "templates")
        .and_then(|(_, t)| t.get("index.twig"))
        .and_then(Json::as_str)
        .is_some_and(|s| s.contains("TwigTestFoo"))
}

/// `name` in camel case (`preserve_keys` -> `preserveKeys`).
fn camel_case(name: &str) -> String {
    let mut out = String::new();
    let mut upper = false;
    for c in name.chars() {
        if c == '_' {
            upper = true;
        } else if upper {
            out.push(c.to_ascii_uppercase());
            upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// A postfix chain (`x|f(a)`, `f(a)|g`, `x.y`) that needs no parentheses as an operand: no
/// space outside brackets and strings, and no operator characters at the top level.
fn is_simple_chain(value: &str) -> bool {
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for c in value.chars() {
        if let Some(q) = quote {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '\'' | '"' => quote = Some(c),
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            ' ' | '-' | '+' | '*' | '/' | '?' | '=' | '<' | '>' | '~' | '%' | ':' if depth == 0 => {
                return false;
            }
            _ => {}
        }
    }
    depth == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kit::case::Case;

    #[test]
    fn deterministic_and_parseable() {
        let a = generate(7, 200);
        let b = generate(7, 200);
        assert_eq!(a, b);
        for json in &a {
            let case = Case::from_json(json).expect("a valid case");
            assert!(case.unsupported_reason().is_none());
            assert!(case.template("index.twig").is_some());
        }
        assert_ne!(generate(8, 5), generate(7, 5));
    }

    #[test]
    fn data_round_trips() {
        for v in VARS {
            if let Some(text) = v.json {
                let json = Json::parse(text).expect("valid JSON");
                assert!(!php_of_json(&json).is_empty(), "{}", v.name);
            }
        }
        assert_eq!(php_string(b"a\"$\\\xff"), r#""a\"\$\\\xff""#);
        assert_eq!(twig_string("it's\\\n"), r"'it\'s\\\x0a'");
        assert_eq!(camel_case("preserve_keys"), "preserveKeys");
    }

    #[test]
    fn operands() {
        assert!(is_simple_chain("x|f(a, b)|g"));
        assert!(!is_simple_chain("a ~ b"));
        assert_eq!(Generator::operand("x|f(1, 2)".to_owned()), "x|f(1, 2)");
        assert_eq!(Generator::operand("v => v".to_owned()), "(v => v)");
        assert_eq!(Generator::operand("(-1)".to_owned()), "(-1)");
    }
}
