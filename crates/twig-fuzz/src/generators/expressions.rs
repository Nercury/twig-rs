//! Cases for the "expressions and operators" family of differential fuzzing (roadmap phase 5).
//!
//! Every case is an oracle "twig" case (`"env": "plain"`) whose template prints one to three
//! expressions. A case stresses one [`Profile`]:
//!
//! | profile | stresses |
//! |---|---|
//! | `arith` | `+ - * / // % **`, unary `-`/`+`, ints near the int64 limits, floats, numeric strings |
//! | `concat` | `~` and string interpolation with operands of every type |
//! | `compare` | `== != < > <= >= <=> === !==` across mixed types |
//! | `strops` | `starts with`, `ends with`, `matches`, `in`, `not in` |
//! | `range` | `..` over ints, letters, floats, numeric strings, bools, null and int64 edges |
//! | `logic` | `and or xor not`, `b-and b-or b-xor`, `?:`, `??`, ternaries, truthiness |
//! | `arrays` | sequence and mapping literals: spread, key types, computed and duplicate keys |
//! | `arrows` | arrow functions in `map`/`filter`/`reduce`/`sort`/`find` and `has some`/`has every` |
//! | `attrs` | attribute chains (`.`, `[]`, `.(expr)`, `?.`, method calls) on every value type |
//! | `literals` | number literals (overflow, underscores, exponents, precision-14 edges), string escapes |
//! | `tostring` | the string conversion of every value type under every autoescape strategy |
//! | `mixed` | free combinations of all of the above |
//! | `coalesce` | `??`, `?:`, `is defined`, `is null` over null-safe chains and undefined names, also as `x[...]` keys |
//! | `assign` | assignment and destructuring expressions (`=`, 3.23), arrow functions as values |
//! | `overflow` | integer arithmetic and bitwise operators at the int64 limits |
//! | `loose` | loose and strict comparisons over curated operands (PHP 8 string/number rules) |
//!
//! Operands are biased towards edge values: `PHP_INT_MAX`/`PHP_INT_MIN`, floats whose `precision`
//! 14 rendering switches to exponent notation, `-0.0`, `INF`, `NAN`, numeric strings with leading
//! or trailing whitespace, leading-numeric and non-numeric strings, `null`, booleans, arrays,
//! `Twig\Markup`, `Stringable` and plain objects, `ArrayAccess` and `Traversable` objects, dates
//! and the helper classes of Twig's `tests/IntegrationTest.php`, plus undefined variables under
//! both `strict_variables` settings, the special names (`_self`, `_charset`, `_context`) and the
//! spellings of `true`/`false`/`null`.
//!
//! Settings vary too: every autoescape strategy, `strict_variables`, `"errors": "throw"`, the
//! `SandboxExtension` (registered, not sandboxing: it changes how `x[...]` compiles) and
//! expressions spread over several lines (runtime error lines follow Twig's `// line N` markers).
//!
//! The context holds only the variables the template uses: `data_json` when JSON can express
//! them, `data_php` otherwise (objects, `NAN`), with `"test_classes": true` when a helper class
//! of `IntegrationTest.php` is used; `TwigTestStringy` is declared by the data code as in the
//! fixture `expressions/array_dynamic_keys.test`.
//!
//! What cannot match by design is left out (`reference/deviations.md`): unseeded randomness, the
//! current time, string callables, ranges large enough to reach a memory limit. `dump()` of
//! objects is fine under every escaping strategy: the comparison ignores object handles also in
//! escaped dumps.
//!
//! [`generate`] is deterministic: case `i` of seed `s` depends only on `s` and `i`. Batches go
//! through `twig-fuzz drive` like any other batch file.

use std::fmt::Write as _;

use crate::kit::json::Json;

/// The family name (case ids, `tests/vectors/fuzz/expressions/`).
pub const FAMILY: &str = "expressions";

/// The cases `0..count` of `seed`.
pub fn generate(seed: u64, count: usize) -> Vec<Json> {
    (0..count).map(|index| generate_case(seed, index)).collect()
}

/// Case `index` of `seed`.
pub fn generate_case(seed: u64, index: usize) -> Json {
    let mut rng = Rng::new(seed.wrapping_mul(0x2545_F491_4F6C_DD1D) ^ index as u64);
    let profile = rng.weighted(PROFILES);
    let mut generator = Generator::new(&mut rng);
    super::with_case_cache(
        generator.case(profile, &format!("x{seed}-{index:05}-{}", profile.name())),
    )
}

// ---- random numbers ----------------------------------------------------------------------------

/// SplitMix64: small, fast and the same on every platform.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    /// A generator seeded with `seed`.
    pub fn new(seed: u64) -> Rng {
        Rng(seed)
    }

    /// The next 64 random bits.
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number in `0..n` (`n > 0`; the modulo bias is irrelevant for these small `n`).
    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }

    /// `true` with a probability of `percent` / 100.
    pub fn percent(&mut self, percent: u64) -> bool {
        self.next_u64() % 100 < percent
    }

    /// A random element of a non-empty slice.
    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }

    /// An element chosen by weight (`items` non-empty, weights not all zero).
    pub fn weighted<T: Copy>(&mut self, items: &[(u64, T)]) -> T {
        let total: u64 = items.iter().map(|(weight, _)| weight).sum();
        let mut roll = self.next_u64() % total;
        for &(weight, item) in items {
            if roll < weight {
                return item;
            }
            roll -= weight;
        }
        items[items.len() - 1].1
    }
}

// ---- profiles ----------------------------------------------------------------------------------

/// The operator group a case stresses (see the module documentation).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Profile {
    /// Arithmetic.
    Arith,
    /// Concatenation and interpolation.
    Concat,
    /// Comparisons.
    Compare,
    /// `starts with`, `ends with`, `matches`, `in`, `not in`.
    StringOps,
    /// Ranges.
    Range,
    /// Logic, bitwise and conditional operators.
    Logic,
    /// Sequence and mapping literals.
    Arrays,
    /// Arrow functions.
    Arrows,
    /// Attribute access.
    Attributes,
    /// Number and string literals.
    Literals,
    /// String conversion in output.
    ToString,
    /// Everything.
    Mixed,
    /// `??`, `?:`, `is defined`, `is null` over null-safe attribute chains and undefined names.
    Coalesce,
    /// Assignment and destructuring expressions (`=`, 3.23), arrow functions as values.
    Assign,
    /// Integer arithmetic at the int64 limits.
    Overflow,
    /// Loose and strict comparisons over curated operands.
    Loose,
}

impl Profile {
    /// The name used in case ids.
    pub fn name(self) -> &'static str {
        match self {
            Profile::Arith => "arith",
            Profile::Concat => "concat",
            Profile::Compare => "compare",
            Profile::StringOps => "strops",
            Profile::Range => "range",
            Profile::Logic => "logic",
            Profile::Arrays => "arrays",
            Profile::Arrows => "arrows",
            Profile::Attributes => "attrs",
            Profile::Literals => "literals",
            Profile::ToString => "tostring",
            Profile::Mixed => "mixed",
            Profile::Coalesce => "coalesce",
            Profile::Assign => "assign",
            Profile::Overflow => "overflow",
            Profile::Loose => "loose",
        }
    }
}

const PROFILES: &[(u64, Profile)] = &[
    (14, Profile::Arith),
    (8, Profile::Concat),
    (12, Profile::Compare),
    (9, Profile::StringOps),
    (8, Profile::Range),
    (11, Profile::Logic),
    (10, Profile::Arrays),
    (7, Profile::Arrows),
    (12, Profile::Attributes),
    (6, Profile::Literals),
    (6, Profile::ToString),
    (9, Profile::Mixed),
    (9, Profile::Coalesce),
    (6, Profile::Assign),
    (6, Profile::Overflow),
    (8, Profile::Loose),
];

// ---- the context palette -----------------------------------------------------------------------

const INT: u32 = 1;
const FLOAT: u32 = 1 << 1;
const NUMSTR: u32 = 1 << 2;
const STR: u32 = 1 << 3;
const BOOL: u32 = 1 << 4;
const NULL: u32 = 1 << 5;
const ARR: u32 = 1 << 6;
const OBJ: u32 = 1 << 7;
/// Small enough to be a range bound.
const SMALL: u32 = 1 << 8;
/// Iterable (`is_iterable()`).
const ITER: u32 = 1 << 9;
/// Has `__toString()`.
const STRINGABLE: u32 = 1 << 10;
/// A helper class of Twig's `tests/IntegrationTest.php` (`"test_classes": true`).
const TEST_CLASS: u32 = 1 << 11;
/// A `TwigTestStringy` (declared by the data code).
const STRINGY: u32 = 1 << 12;
/// Has methods worth calling with arguments.
const METHODS: u32 = 1 << 13;

const NUMERIC: u32 = INT | FLOAT | NUMSTR;
const SCALAR: u32 = INT | FLOAT | NUMSTR | STR | BOOL | NULL;

/// A context variable: its name, kind, JSON text (`None`: JSON cannot express it), PHP
/// expression and the attribute names worth accessing on it.
struct Var {
    name: &'static str,
    kind: u32,
    json: Option<&'static str>,
    php: &'static str,
    attrs: &'static [&'static str],
}

const fn var(
    name: &'static str,
    kind: u32,
    json: &'static str,
    php: &'static str,
    attrs: &'static [&'static str],
) -> Var {
    Var {
        name,
        kind,
        json: if json.is_empty() { None } else { Some(json) },
        php,
        attrs,
    }
}

const SCALAR_ATTRS: &[&str] = &["0", "x"];
const STRING_ATTRS: &[&str] = &["0", "1", "length", "x"];
const DATE_ATTRS: &[&str] = &["timestamp", "offset", "timezone", "missing"];
const DEFAULT_ATTRS: &[&str] = &["a", "b", "0", "1", "x", "c"];

#[rustfmt::skip]
static VARS: &[Var] = &[
    // ints
    var("zero", INT | SMALL, "0", "0", SCALAR_ATTRS),
    var("one", INT | SMALL, "1", "1", SCALAR_ATTRS),
    var("two", INT | SMALL, "2", "2", SCALAR_ATTRS),
    var("neg", INT | SMALL, "-1", "-1", SCALAR_ATTRS),
    var("seven", INT | SMALL, "7", "7", SCALAR_ATTRS),
    var("n255", INT, "255", "255", SCALAR_ATTRS),
    var("imax", INT, "9223372036854775807", "PHP_INT_MAX", SCALAR_ATTRS),
    var("imin", INT, "-9223372036854775808", "PHP_INT_MIN", SCALAR_ATTRS),
    var("p62", INT, "4611686018427387904", "4611686018427387904", SCALAR_ATTRS),
    var("p53", INT, "9007199254740993", "9007199254740993", SCALAR_ATTRS),
    // floats
    var("fzero", FLOAT | SMALL, "0.0", "0.0", SCALAR_ATTRS),
    var("fnegz", FLOAT | SMALL, "-0.0", "-0.0", SCALAR_ATTRS),
    var("half", FLOAT | SMALL, "0.5", "0.5", SCALAR_ATTRS),
    var("f15", FLOAT | SMALL, "1.5", "1.5", SCALAR_ATTRS),
    var("fneg", FLOAT | SMALL, "-2.5", "-2.5", SCALAR_ATTRS),
    var("f3", FLOAT | SMALL, "3.0", "3.0", SCALAR_ATTRS),
    var("tenth", FLOAT, "0.1", "0.1", SCALAR_ATTRS),
    var("fsum", FLOAT, "0.30000000000000004", "0.30000000000000004", SCALAR_ATTRS),
    var("f1e15", FLOAT, "1e15", "1e15", SCALAR_ATTRS),
    var("f1e14", FLOAT, "123456789012345.67", "123456789012345.67", SCALAR_ATTRS),
    var("fbig", FLOAT, "1.0e25", "1.0e25", SCALAR_ATTRS),
    var("p63f", FLOAT, "9.2233720368547758e18", "9.2233720368547758E+18", SCALAR_ATTRS),
    var("ftiny", FLOAT, "1.5e-7", "1.5e-7", SCALAR_ATTRS),
    var("fsub", FLOAT, "5e-324", "5e-324", SCALAR_ATTRS),
    var("fmax", FLOAT, "1.7976931348623157e308", "1.7976931348623157e308", SCALAR_ATTRS),
    var("inf", FLOAT, "1e400", "INF", SCALAR_ATTRS),
    var("ninf", FLOAT, "-1e400", "-INF", SCALAR_ATTRS),
    var("nan", FLOAT, "", "NAN", SCALAR_ATTRS),
    // numeric strings
    var("s0", NUMSTR | SMALL, r#""0""#, "'0'", STRING_ATTRS),
    var("s1", NUMSTR | SMALL, r#""1""#, "'1'", STRING_ATTRS),
    var("s10", NUMSTR | SMALL, r#""10""#, "'10'", STRING_ATTRS),
    var("sneg", NUMSTR | SMALL, r#""-3""#, "'-3'", STRING_ATTRS),
    var("sexp", NUMSTR, r#""1e3""#, "'1e3'", STRING_ATTRS),
    var("sflt", NUMSTR | SMALL, r#""2.5""#, "'2.5'", STRING_ATTRS),
    var("slead", NUMSTR | SMALL, r#"" 5""#, "' 5'", STRING_ATTRS),
    var("strail", NUMSTR | SMALL, r#""5 ""#, "'5 '", STRING_ATTRS),
    var("sws", NUMSTR | SMALL, r#""\t1\n""#, r#""\t1\n""#, STRING_ATTRS),
    var("szero2", NUMSTR | SMALL, r#""0.0""#, "'0.0'", STRING_ATTRS),
    var("snegz", NUMSTR | SMALL, r#""-0""#, "'-0'", STRING_ATTRS),
    var("splus", NUMSTR | SMALL, r#""+1""#, "'+1'", STRING_ATTRS),
    var("soct", NUMSTR | SMALL, r#""012""#, "'012'", STRING_ATTRS),
    var("sdot", NUMSTR | SMALL, r#"".5""#, "'.5'", STRING_ATTRS),
    var("sdot2", NUMSTR | SMALL, r#""5.""#, "'5.'", STRING_ATTRS),
    var("sbig", NUMSTR, r#""9223372036854775808""#, "'9223372036854775808'", STRING_ATTRS),
    var("sinf", NUMSTR, r#""1e400""#, "'1e400'", STRING_ATTRS),
    // leading-numeric and other strings
    var("shex", STR, r#""0x1A""#, "'0x1A'", STRING_ATTRS),
    var("sund", STR, r#""1_000""#, "'1_000'", STRING_ATTRS),
    var("smix", STR, r#""12abc""#, "'12abc'", STRING_ATTRS),
    var("abc", STR | SMALL, r#""abc""#, "'abc'", STRING_ATTRS),
    var("empty", STR | SMALL, r#""""#, "''", STRING_ATTRS),
    var("space", STR | SMALL, r#"" ""#, "' '", STRING_ATTRS),
    var("lower", STR | SMALL, r#""a""#, "'a'", STRING_ATTRS),
    var("upper", STR | SMALL, r#""Z""#, "'Z'", STRING_ATTRS),
    var("utf", STR, r#""é""#, "'é'", STRING_ATTRS),
    var("html", STR, r#""<b>&amp;</b>""#, "'<b>&amp;</b>'", STRING_ATTRS),
    var("quote", STR, r#""a'b\"c""#, r#"'a\'b"c'"#, STRING_ATTRS),
    var("snull", STR, r#""null""#, "'null'", STRING_ATTRS),
    var("sinfw", STR, r#""INF""#, "'INF'", STRING_ATTRS),
    // bools and null
    var("yes", BOOL | SMALL, "true", "true", SCALAR_ATTRS),
    var("no", BOOL | SMALL, "false", "false", SCALAR_ATTRS),
    var("nul", NULL | SMALL, "null", "null", SCALAR_ATTRS),
    // arrays
    var("elist", ARR | ITER, "[]", "[]", &["0", "x"]),
    var("list", ARR | ITER, "[1,2,3]", "[1, 2, 3]", &["0", "1", "2", "3", "-1"]),
    var("letters", ARR | ITER, r#"["a","b"]"#, "['a', 'b']", &["0", "1", "a"]),
    var("map", ARR | ITER, r#"{"a":1,"b":2}"#, "['a' => 1, 'b' => 2]", &["a", "b", "c", "0"]),
    var("zlist", ARR | ITER, "[0]", "[0]", &["0", "1"]),
    var("nlist", ARR | ITER, "[null]", "[null]", &["0", "1"]),
    var("nested", ARR | ITER, "[[1,2],[3]]", "[[1, 2], [3]]", &["0", "1", "2"]),
    var("numkeys", ARR | ITER, r#"{"1":"x","2":"y"}"#, "[1 => 'x', 2 => 'y']", &["1", "2", "0", "x"]),
    var("deep", ARR | ITER, r#"{"x":{"y":{"z":1}}}"#, "['x' => ['y' => ['z' => 1]]]", &["x", "y"]),
    var("mixed", ARR | ITER, r#"[1,"1",1.0,true,null]"#, "[1, '1', 1.0, true, null]", &["0", "2", "4", "5"]),
    var("strnums", ARR | ITER, r#"["10",9,"9a"]"#, "['10', 9, '9a']", &["0", "1", "2"]),
    var("holes", ARR | ITER, r#"{"3":"c","1":"a"}"#, "[3 => 'c', 1 => 'a']", &["3", "1", "0"]),
    var("nmap", ARR | ITER, r#"{"k":null,"e":""}"#, "['k' => null, 'e' => '']", &["k", "e", "x"]),
    var("floats", ARR | ITER, "[1.5,-0.0,0.1]", "[1.5, -0.0, 0.1]", &["0", "1", "2"]),
    var("assoc", ARR | ITER, r#"{"0":"z","x":"y"}"#, "[0 => 'z', 'x' => 'y']", &["0", "x", "1"]),
    // objects
    var("obj", OBJ, "", "(object) ['a' => 1, 'b' => ['c' => 2], 'n' => null, 's' => 'str']", &["a", "b", "n", "s", "missing"]),
    var("eobj", OBJ, "", "new \\stdClass()", &["a", "missing"]),
    var("aobj", OBJ | ITER | METHODS, "", "new \\ArrayObject(['a' => 1, 'b' => 2])", &["a", "b", "count", "missing", "0"]),
    var("aiter", OBJ | ITER | METHODS, "", "new \\ArrayIterator([1, 2, 3])", &["0", "1", "count", "current", "missing"]),
    var("mk", OBJ | STRINGABLE | METHODS, "", "new \\Twig\\Markup('<b>x</b>', 'UTF-8')", &["count", "charset", "missing", "jsonSerialize"]),
    var("mkempty", OBJ | STRINGABLE, "", "new \\Twig\\Markup('', 'UTF-8')", &["count", "missing"]),
    var("mknum", OBJ | STRINGABLE, "", "new \\Twig\\Markup('10', 'UTF-8')", &["count", "missing"]),
    var("str7", OBJ | STRINGABLE | STRINGY, "", "new TwigTestStringy('7')", &["v", "missing"]),
    var("strabc", OBJ | STRINGABLE | STRINGY, "", "new TwigTestStringy('abc')", &["v", "missing"]),
    var("date", OBJ | METHODS, "", "new \\DateTime('2020-01-02 03:04:05')", DATE_ATTRS),
    var("idate", OBJ | METHODS, "", "new \\DateTimeImmutable('2001-02-03 04:05:06')", DATE_ATTRS),
    var("foo", OBJ | ITER | TEST_CLASS | METHODS, "", "new \\Twig\\Tests\\TwigTestFoo()", &["foo", "Foo", "getFoo", "bar", "self", "null", "empty", "is", "in", "not", "array", "position", "missing"]),
    var("tostr", OBJ | STRINGABLE | TEST_CLASS, "", "new \\Twig\\Tests\\ToStringStub('5')", &["string", "missing"]),
    var("countable", OBJ | TEST_CLASS, "", "new \\Twig\\Tests\\CountableStub(3)", &["count", "missing"]),
    var("agg", OBJ | ITER | TEST_CLASS, "", "new \\Twig\\Tests\\IteratorAggregateStub(['x' => 1, 'y' => 2])", &["x", "getIterator", "missing"]),
    var("simpleit", OBJ | ITER | STRINGABLE | TEST_CLASS, "", "new \\Twig\\Tests\\SimpleIteratorForTesting()", &["current", "key", "valid", "missing"]),
    var("magic", OBJ | TEST_CLASS, "", "new \\Twig\\Tests\\MagicCallStub()", &["foo", "missing"]),
];

const UNDEFINED: &[&str] = &["undef", "nope"];

// ---- literal pools (Twig source text) ----------------------------------------------------------

const INT_LITERALS: &[&str] = &[
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
    "1000",
    "65536",
    "2147483647",
    "2147483648",
    "4294967296",
    "9007199254740992",
    "9007199254740993",
    "4611686018427387904",
    "9223372036854775806",
    "9223372036854775807",
    "9223372036854775808",
    "18446744073709551616",
    "99999999999999999999",
    "1_000",
    "1_000_000",
    "007",
    "00",
    "0_1",
];

const FLOAT_LITERALS: &[&str] = &[
    "0.0",
    "1.0",
    "1.5",
    "2.5",
    "0.1",
    "0.2",
    "0.5",
    "0.7",
    "3.14",
    "1e3",
    "1E3",
    "1e+3",
    "1e-3",
    "1.5e-7",
    "1e15",
    "1e14",
    "1e16",
    "1.0e20",
    "1e21",
    "1e100",
    "1e308",
    "1e309",
    "1e-320",
    "4.9e-324",
    "123456789012345.678",
    "12345678901234567.0",
    "1234567890123456789.0",
    "0.30000000000000004",
    "3.14159265358979",
    "1_000.5",
    "1.0E+25",
    "9.2233720368547758E+18",
    "100000000000000.0",
    "99999999999999.99",
    "999999999999999.9",
    "0.1e1",
    "1.23456789012345e-5",
    "1e0",
    "2.675",
    "1.005",
    "0.000021",
    "0.0001",
    "1.5e300",
    "7.0E-10",
    "1e-5",
    "0.00001",
];

/// String literals: numeric, leading-numeric and other strings, Twig's escapes (valid, octal,
/// hexadecimal and the deprecated ones) and quotes.
const STRING_LITERALS: &[&str] = &[
    "''",
    "'0'",
    "'1'",
    "'-1'",
    "'10'",
    "'1e3'",
    "' 5'",
    "'5 '",
    "'0x1A'",
    "'1_000'",
    "'.5'",
    "'5.'",
    "'-0'",
    "'+1'",
    "'012'",
    "'1.0'",
    "'0.0'",
    "'abc'",
    "'ABC'",
    "'a'",
    "'b'",
    "'z'",
    "'Z'",
    "' '",
    "'12abc'",
    "'abc12'",
    "'null'",
    "'true'",
    "'Array'",
    "'é'",
    "'日本'",
    "'<br>'",
    "'&'",
    "'%d'",
    "'9223372036854775807'",
    "'9223372036854775808'",
    "'1e400'",
    "'INF'",
    "'NAN'",
    "'-INF'",
    "'1e'",
    "'e1'",
    "'١'",
    "'1 '",
    r"'\t1'",
    r#""\n2""#,
    r"'\x41'",
    r"'\101'",
    r#""A""#,
    r"'it\'s'",
    r#""a\"b""#,
    r#"'a\"b'"#,
    r"'\0'",
    r"'\777'",
    r"'\\'",
    r#""\f\v""#,
    r"'a\\nb'",
    r"'\x'",
    r"'\e'",
    r##""\#{x}""##,
    "'#{x}'",
    r#""tab\there""#,
    "'1e-7'",
    "'0.1'",
    "' 0x1'",
    "'1,5'",
    "'00'",
    "'0e5'",
    "'-'",
    r##""\x41#{1 + 1}\n""##,
    r##""#{'a'}\#{b}""##,
    r##""a\"#{null}\"b""##,
    r##""#{"\t"}|#{1.5e-7}""##,
    r##""\101#{-0.0}\x""##,
];

/// Special names and the literal spellings of `true`, `false` and `null` (`True` is a variable).
const SPECIAL_NAMES: &[&str] = &[
    "_self",
    "_charset",
    "(_context|length)",
    "TRUE",
    "FALSE",
    "NULL",
    "NONE",
    "none",
    "True",
];

const NUMERIC_STRING_LITERALS: &[&str] = &[
    "'0'",
    "'1'",
    "'-1'",
    "'10'",
    "'1e3'",
    "' 5'",
    "'5 '",
    "'.5'",
    "'5.'",
    "'-0'",
    "'+1'",
    "'012'",
    "'1.0'",
    "'0.0'",
    "'1e400'",
    "'9223372036854775807'",
    "'9223372036854775808'",
    "'0e5'",
    "'1e-7'",
    r"'\t1'",
    "'12abc'",
    "'0x1A'",
    "'1_000'",
    "'abc'",
    "''",
];

const REGEX_LITERALS: &[&str] = &[
    "'/^a/'",
    "'/b/i'",
    r"'/\\d+/'",
    "'/^$/'",
    "'/./u'",
    r"'/^\\s*$/'",
    "'/[a-z]+/'",
    "'#a#'",
    "'{a}'",
    "'/a/x'",
    "'/A/i'",
    "'~^1~'",
    "'/é/u'",
    r"'/\\bab/'",
    r"'/^-?\\d+$/'",
    "'/1/'",
    r"'/^\\d*\\.\\d+$/'",
    "'/[[:alpha:]]/'",
    "'/(a)(b)?/'",
    r"'/\\x{e9}/u'",
    "'/a'",
    "'abc'",
    "'/(/'",
    "'/a/k'",
    "''",
    "'(a'",
];

const DATE_FORMATS: &[&str] = &[
    "'Y-m-d'",
    "'U'",
    "'D, d M Y H:i'",
    "'u e P'",
    "'N jS z t L'",
];

/// Bounds of ranges by kind; a range stays small (`reference/deviations.md`, memory limits) or is
/// rejected by PHP before it allocates.
const RANGE_SMALL_INTS: &[&str] = &["0", "1", "2", "3", "4", "5", "7", "10", "12", "-2", "-1"];
const RANGE_LETTERS: &[&str] = &[
    "'a'", "'e'", "'A'", "'E'", "'z'", "'Z'", "'b'", "'aa'", "'é'", "'!'", "'~'", "'0'", "'9'",
    "' '", "''", "'ab'", "'Y'",
];
const RANGE_FLOATS: &[&str] = &[
    "0.5", "1.5", "2.5", "-1.5", "1.0", "4.0", "1e1", "0.1", "3.9", "2.25",
];
const RANGE_NUMERIC_STRINGS: &[&str] = &[
    "'1'", "'5'", "'2.5'", "' 3'", "'1e1'", "'-2'", "'3 '", "'0'", "'1.5'", "'+2'", "'07'",
];
const RANGE_EDGES: &[(&str, &str)] = &[
    ("9223372036854775805", "9223372036854775807"),
    ("9223372036854775807", "9223372036854775805"),
    ("(-9223372036854775807 - 1)", "(-9223372036854775806)"),
    ("9007199254740992.0", "9007199254740994.0"),
    ("9223372036854775806", "9223372036854775808"),
    ("1e15", "1e15 + 2"),
    ("0.1", "0.3"),
    ("-0.0", "2"),
    ("'9223372036854775806'", "'9223372036854775807'"),
];
const RANGE_WILD: &[(&str, &str)] = &[
    ("0", "9223372036854775807"),
    ("-9223372036854775807", "9223372036854775807"),
    ("0", "1e20"),
    ("1", "1e300"),
    ("0", "constant('INF')"),
    ("constant('NAN')", "3"),
    ("0", "-1e20"),
    ("'a'", "9223372036854775807"),
    ("1.5", "9.2233720368547758E+18"),
];

// ---- the generator -----------------------------------------------------------------------------

/// What an expression should mostly produce.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Want {
    Any,
    Num,
    Str,
    Bool,
    Iter,
    Key,
}

struct Generator<'r> {
    rng: &'r mut Rng,
    /// The palette indexes of the variables used, in first-use order.
    used: Vec<usize>,
    /// The template calls `dump()`.
    dump: bool,
    /// Names bound by enclosing arrow functions.
    params: Vec<&'static str>,
    /// The percentage of attribute accesses that use `?.`.
    null_safe: u64,
}

impl<'r> Generator<'r> {
    fn new(rng: &'r mut Rng) -> Self {
        Generator {
            rng,
            used: Vec::new(),
            dump: false,
            params: Vec::new(),
            null_safe: 18,
        }
    }

    fn percent(&mut self, percent: u64) -> bool {
        self.rng.percent(percent)
    }

    fn pick(&mut self, items: &[&'static str]) -> &'static str {
        items[self.rng.below(items.len())]
    }

    // ---- variables ----

    /// A variable whose kind has any of the bits of `mask`.
    fn var_index(&mut self, mask: u32) -> usize {
        let candidates: Vec<usize> = (0..VARS.len())
            .filter(|&i| VARS[i].kind & mask != 0)
            .collect();
        let index = if candidates.is_empty() {
            self.rng.below(VARS.len())
        } else {
            *self.rng.pick(&candidates)
        };
        if !self.used.contains(&index) {
            self.used.push(index);
        }
        index
    }

    fn var(&mut self, mask: u32) -> String {
        let index = self.var_index(mask);
        VARS[index].name.to_owned()
    }

    /// A variable, an arrow parameter in scope, or (rarely) an undefined name.
    fn name(&mut self, mask: u32) -> String {
        if !self.params.is_empty() && self.percent(45) {
            return self.pick(&self.params.clone()).to_owned();
        }
        if self.percent(4) {
            return self.pick(UNDEFINED).to_owned();
        }
        self.var(mask)
    }

    // ---- leaves ----

    fn leaf(&mut self, want: Want) -> String {
        match want {
            Want::Num => self.num_leaf(),
            Want::Str => self.str_leaf(),
            Want::Bool => self.bool_leaf(),
            Want::Iter => self.iter_leaf(),
            Want::Key => self.key_leaf(),
            Want::Any => match self.rng.below(40) {
                0..=11 => self.num_leaf(),
                12..=19 => self.str_leaf(),
                20..=23 => self.bool_leaf(),
                24..=27 => self.iter_leaf(),
                28 => self.pick(SPECIAL_NAMES).to_owned(),
                _ => self.name(!0),
            },
        }
    }

    fn num_leaf(&mut self) -> String {
        match self.rng.below(100) {
            0..=24 => self.pick(INT_LITERALS).to_owned(),
            25..=42 => self.pick(FLOAT_LITERALS).to_owned(),
            43..=50 => self.pick(NUMERIC_STRING_LITERALS).to_owned(),
            51..=84 => self.name(NUMERIC),
            85..=89 => self.pick(&["true", "false", "null"]).to_owned(),
            90..=94 => self.name(STR | BOOL | NULL),
            _ => self.name(ARR | OBJ),
        }
    }

    fn str_leaf(&mut self) -> String {
        match self.rng.below(100) {
            0..=34 => self.pick(STRING_LITERALS).to_owned(),
            35..=59 => self.name(STR | NUMSTR),
            60..=69 => self.name(STRINGABLE),
            70..=79 => self.num_leaf(),
            80..=84 => self.pick(&["true", "false", "null"]).to_owned(),
            _ => self.name(!0),
        }
    }

    fn bool_leaf(&mut self) -> String {
        match self.rng.below(100) {
            0..=39 => self
                .pick(&[
                    "true", "false", "null", "0", "1", "''", "'0'", "'0.0'", "' '", "[]", "[0]",
                    "'a'", "0.0", "-0.0", "{}", "'false'", "[null]", "0.1", "-1", "'00'",
                ])
                .to_owned(),
            _ => self.name(!0),
        }
    }

    fn iter_leaf(&mut self) -> String {
        match self.rng.below(100) {
            0..=54 => self.name(ITER),
            55..=79 => self
                .pick(&[
                    "[]",
                    "[1, 2]",
                    "['a', 'b', 'c']",
                    "{a: 1, b: 2}",
                    "{'1': 'x', '0': 'y'}",
                    "[null, false, 0, '']",
                    "[1, [2, 3]]",
                    "{x: {y: 1}}",
                    "['10', 9, '9a', 1.5]",
                    "{2: 'two', 1: 'one'}",
                    "[1.5, -0.0, '1.5']",
                    "{'': 'e', ' ': 's'}",
                ])
                .to_owned(),
            80..=89 => {
                let (low, high) = self.range_bounds();
                format!("({low}..{high})")
            }
            _ => self.name(SCALAR | OBJ),
        }
    }

    fn key_leaf(&mut self) -> String {
        match self.rng.below(100) {
            0..=19 => self
                .pick(&["0", "1", "2", "-1", "3", "9223372036854775807", "10"])
                .to_owned(),
            20..=44 => self
                .pick(&[
                    "'a'",
                    "'b'",
                    "'0'",
                    "'1'",
                    "'01'",
                    "'-0'",
                    "' 1'",
                    "'1.5'",
                    "''",
                    "'x'",
                    "'9223372036854775808'",
                    "'-1'",
                    "'c'",
                    "'k'",
                    "'e'",
                ])
                .to_owned(),
            45..=54 => self
                .pick(&[
                    "0.5",
                    "1.5",
                    "1.0",
                    "-1.5",
                    "1e3",
                    "1e20",
                    "0.9999999999999999",
                ])
                .to_owned(),
            55..=62 => self.pick(&["true", "false", "null"]).to_owned(),
            63..=66 => self.pick(&["[]", "[1]", "{a: 1}"]).to_owned(),
            67..=72 => self.name(STRINGABLE | OBJ),
            _ => self.name(SCALAR),
        }
    }

    // ---- composite expressions ----

    fn depth(&mut self, low: u32, high: u32) -> u32 {
        low + self.rng.below((high - low + 1) as usize) as u32
    }

    /// An expression, parenthesized now and then when it is not atomic.
    fn sub(&mut self, want: Want, depth: u32) -> String {
        let e = self.expr(want, depth);
        if atomic(&e) || self.percent(35) {
            e
        } else {
            format!("({e})")
        }
    }

    fn expr(&mut self, want: Want, depth: u32) -> String {
        if depth == 0 || self.percent(22) {
            return self.leaf(want);
        }
        match want {
            Want::Num => self.num_expr(depth),
            Want::Str => self.str_expr(depth),
            Want::Bool => self.bool_expr(depth),
            Want::Iter => self.iter_expr(depth),
            Want::Key => {
                if self.percent(50) {
                    self.num_expr(depth)
                } else {
                    self.str_expr(depth)
                }
            }
            Want::Any => match self.rng.below(20) {
                0..=5 => self.num_expr(depth),
                6..=8 => self.str_expr(depth),
                9..=12 => self.bool_expr(depth),
                13..=14 => self.iter_expr(depth),
                15..=16 => self.conditional(Want::Any, depth),
                _ => self.attribute_chain(depth),
            },
        }
    }

    fn arith_op(&mut self) -> &'static str {
        self.rng.weighted(&[
            (3, "+"),
            (3, "-"),
            (3, "*"),
            (2, "/"),
            (2, "//"),
            (2, "%"),
            (2, "**"),
        ])
    }

    fn num_expr(&mut self, depth: u32) -> String {
        match self.rng.below(100) {
            0..=54 => {
                let op = self.arith_op();
                let left = self.sub(Want::Num, depth - 1);
                let right = self.sub(Want::Num, depth - 1);
                format!("{left} {op} {right}")
            }
            55..=66 => {
                let op = self.pick(&["-", "+", "- -", "-", "+"]);
                let operand = self.sub(Want::Num, depth - 1);
                format!("{op}{operand}")
            }
            67..=74 => {
                let op = self.pick(&["b-and", "b-or", "b-xor"]);
                let left = self.sub(Want::Num, depth - 1);
                let right = self.sub(Want::Num, depth - 1);
                format!("{left} {op} {right}")
            }
            75..=81 => self.conditional(Want::Num, depth),
            82..=87 => self.attribute_chain(depth),
            88..=93 => {
                let operand = self.sub(Want::Num, depth - 1);
                match self.rng.below(6) {
                    0 => format!("{operand}|abs"),
                    1 => format!("{operand}|round"),
                    2 => format!("{operand}|length"),
                    3 => {
                        let other = self.sub(Want::Num, depth - 1);
                        format!("max({operand}, {other})")
                    }
                    4 => {
                        let other = self.sub(Want::Num, depth - 1);
                        format!("min({operand}, {other})")
                    }
                    _ => format!("{operand}|first"),
                }
            }
            _ => {
                let left = self.sub(Want::Any, depth - 1);
                let right = self.sub(Want::Any, depth - 1);
                format!("{left} <=> {right}")
            }
        }
    }

    fn str_expr(&mut self, depth: u32) -> String {
        match self.rng.below(100) {
            0..=44 => {
                let n = 2 + self.rng.below(2);
                let parts: Vec<String> = (0..n).map(|_| self.sub(Want::Any, depth - 1)).collect();
                parts.join(" ~ ")
            }
            45..=69 => self.interpolation(depth),
            70..=79 => self.conditional(Want::Str, depth),
            80..=89 => self.attribute_chain(depth),
            _ => {
                let operand = self.sub(Want::Any, depth - 1);
                let filter = self.pick(&[
                    "upper",
                    "default('d')",
                    "join(',')",
                    "trim",
                    "lower",
                    "capitalize",
                ]);
                format!("{operand}|{filter}")
            }
        }
    }

    fn interpolation(&mut self, depth: u32) -> String {
        let inner = self.expr(Want::Any, depth.saturating_sub(1));
        match self.rng.below(6) {
            0 => format!("\"<#{{{inner}}}>\""),
            1 => format!("\"#{{{inner}}}\""),
            2 => {
                let other = self.expr(Want::Any, depth.saturating_sub(1));
                format!("\"#{{ {inner} }}-#{{{other}}}\"")
            }
            3 => format!("\"a #{{ \"b#{{{inner}}}\" }} c\""),
            4 => format!("\"x#{{{inner}}}\" ~ 'y'"),
            _ => format!("\"#{{{inner}}}\" \"z\""),
        }
    }

    fn comparison_op(&mut self) -> &'static str {
        self.rng.weighted(&[
            (4, "=="),
            (3, "!="),
            (3, "<"),
            (3, ">"),
            (2, "<="),
            (2, ">="),
            (2, "<=>"),
            (2, "==="),
            (2, "!=="),
        ])
    }

    fn bool_expr(&mut self, depth: u32) -> String {
        match self.rng.below(100) {
            0..=34 => {
                let op = self.comparison_op();
                let left = self.sub(Want::Any, depth - 1);
                let right = self.sub(Want::Any, depth - 1);
                format!("{left} {op} {right}")
            }
            35..=49 => {
                let op = self.pick(&["and", "or", "xor", "and", "or"]);
                let left = self.sub(Want::Bool, depth - 1);
                let right = self.sub(Want::Bool, depth - 1);
                format!("{left} {op} {right}")
            }
            50..=56 => {
                let operand = self.sub(Want::Bool, depth - 1);
                format!("not {operand}")
            }
            57..=71 => self.string_op(depth),
            72..=78 => self.has_some_every(depth),
            79..=89 => {
                let operand = self.sub(Want::Any, depth - 1);
                let negation = if self.percent(40) { "not " } else { "" };
                let test = match self.rng.below(9) {
                    0 => "defined".to_owned(),
                    1 => "null".to_owned(),
                    2 => "empty".to_owned(),
                    3 => "iterable".to_owned(),
                    4 => format!("same as({})", self.leaf(Want::Any)),
                    5 => "odd".to_owned(),
                    6 => "even".to_owned(),
                    7 => format!("divisible by({})", self.leaf(Want::Num)),
                    _ => "sequence".to_owned(),
                };
                format!("{operand} is {negation}{test}")
            }
            _ => self.conditional(Want::Bool, depth),
        }
    }

    fn string_op(&mut self, depth: u32) -> String {
        let d = depth.saturating_sub(1);
        match self.rng.below(10) {
            0..=1 => {
                let left = self.sub(Want::Str, d);
                let right = self.sub(Want::Str, d);
                format!("{left} starts with {right}")
            }
            2..=3 => {
                let left = self.sub(Want::Str, d);
                let right = self.sub(Want::Str, d);
                format!("{left} ends with {right}")
            }
            4..=5 => {
                let subject = self.sub(Want::Str, d);
                let regex = if self.percent(75) {
                    self.pick(REGEX_LITERALS).to_owned()
                } else {
                    self.sub(Want::Str, d)
                };
                format!("{subject} matches {regex}")
            }
            _ => {
                let needle = self.sub(Want::Any, d);
                let haystack = match self.rng.below(5) {
                    0 => self.sub(Want::Str, d),
                    1 => self.name(STRINGABLE | ITER),
                    _ => self.sub(Want::Iter, d),
                };
                let op = if self.percent(70) { "in" } else { "not in" };
                format!("{needle} {op} {haystack}")
            }
        }
    }

    fn has_some_every(&mut self, depth: u32) -> String {
        let subject = if self.percent(85) {
            self.sub(Want::Iter, depth.saturating_sub(1))
        } else {
            self.sub(Want::Any, depth.saturating_sub(1))
        };
        let op = if self.percent(50) {
            "has some"
        } else {
            "has every"
        };
        let callable = if self.percent(92) {
            self.arrow(1, depth.saturating_sub(1))
        } else {
            // not callable (strings and arrays would name PHP functions: reference/deviations.md)
            self.name(INT | FLOAT | BOOL | NULL | OBJ)
        };
        format!("{subject} {op} {callable}")
    }

    fn conditional(&mut self, want: Want, depth: u32) -> String {
        let d = depth - 1;
        match self.rng.below(10) {
            0..=2 => {
                let test = self.sub(Want::Bool, d);
                let then = self.sub(want, d);
                let otherwise = self.sub(want, d);
                format!("{test} ? {then} : {otherwise}")
            }
            3 => {
                let test = self.sub(Want::Bool, d);
                let then = self.sub(want, d);
                format!("{test} ? {then}")
            }
            4..=5 => {
                let left = self.sub(want, d);
                let right = self.sub(want, d);
                format!("{left} ?: {right}")
            }
            6..=8 => {
                let left = self.coalesce_left(d);
                let right = self.sub(want, d);
                if self.percent(25) {
                    let last = self.sub(want, d);
                    format!("{left} ?? {right} ?? {last}")
                } else {
                    format!("{left} ?? {right}")
                }
            }
            _ => {
                let test = self.sub(Want::Bool, d);
                let then = self.sub(want, d);
                let test2 = self.sub(Want::Bool, d);
                let a = self.sub(want, d);
                let b = self.sub(want, d);
                format!("{test} ? {then} : {test2} ? {a} : {b}")
            }
        }
    }

    /// The left operand of `??`: something the `defined` test accepts.
    fn coalesce_left(&mut self, depth: u32) -> String {
        match self.rng.below(10) {
            0..=3 => self.name(!0),
            4 => self.pick(UNDEFINED).to_owned(),
            5..=7 => self.attribute_chain(depth.max(1)),
            8 => self
                .pick(&["null", "'x'", "[]", "[1]", "0", "false"])
                .to_owned(),
            _ => format!("max({}, {})", self.leaf(Want::Num), self.leaf(Want::Num)),
        }
    }

    // ---- iterables ----

    fn iter_expr(&mut self, depth: u32) -> String {
        match self.rng.below(10) {
            0..=2 => self.sequence(depth),
            3..=5 => self.mapping(depth),
            6..=7 => {
                let (low, high) = self.range_bounds();
                format!("{low}..{high}")
            }
            8 => self.attribute_chain(depth),
            _ => {
                let operand = self.sub(Want::Iter, depth - 1);
                match self.rng.below(5) {
                    0 => format!("{operand}|keys"),
                    1 => {
                        let other = self.sub(Want::Iter, depth - 1);
                        format!("{operand}|merge({other})")
                    }
                    2 => format!("{operand}|reverse"),
                    3 => {
                        let arrow = self.arrow(1, depth - 1);
                        format!("{operand}|map({arrow})")
                    }
                    _ => {
                        let arity = self.rng.below(2) + 1;
                        let arrow = self.arrow(arity, depth - 1);
                        format!("{operand}|filter({arrow})")
                    }
                }
            }
        }
    }

    fn sequence(&mut self, depth: u32) -> String {
        let n = self.rng.below(4);
        let mut items: Vec<String> = Vec::new();
        for _ in 0..n {
            if self.percent(22) {
                let spread = self.spread_operand(depth);
                items.push(format!("...{spread}"));
            } else {
                items.push(self.expr(Want::Any, depth.saturating_sub(1)));
            }
        }
        let trailing = if !items.is_empty() && self.percent(10) {
            ","
        } else {
            ""
        };
        format!("[{}{trailing}]", items.join(", "))
    }

    fn spread_operand(&mut self, depth: u32) -> String {
        match self.rng.below(10) {
            0..=4 => self.name(ITER),
            5..=6 => self.sequence(depth.saturating_sub(1)),
            7 => self.mapping(depth.saturating_sub(1)),
            8 => {
                let (low, high) = self.range_bounds();
                format!("({low}..{high})")
            }
            _ => self.name(SCALAR | OBJ),
        }
    }

    fn mapping(&mut self, depth: u32) -> String {
        let n = self.rng.below(4);
        let mut items: Vec<String> = Vec::new();
        for _ in 0..n {
            let value_depth = depth.saturating_sub(1);
            let item = match self.rng.below(20) {
                0..=4 => {
                    let key = self.pick(&["a", "b", "x", "k", "true", "null", "and", "e"]);
                    let value = self.expr(Want::Any, value_depth);
                    format!("{key}: {value}")
                }
                5..=7 => {
                    let key = self.pick(&[
                        "'a'",
                        "'1'",
                        "'01'",
                        "'-0'",
                        "' 1'",
                        "''",
                        "'9223372036854775808'",
                        "'a b'",
                        "\"b\"",
                        "'1.5'",
                        "'-1'",
                    ]);
                    let value = self.expr(Want::Any, value_depth);
                    format!("{key}: {value}")
                }
                8..=10 => {
                    let key = self.pick(&[
                        "0",
                        "1",
                        "2",
                        "5",
                        "1.5",
                        "0.5",
                        "1e3",
                        "9223372036854775807",
                        "9223372036854775808",
                        "007",
                    ]);
                    let value = self.expr(Want::Any, value_depth);
                    format!("{key}: {value}")
                }
                11..=14 => {
                    let key = self.expr(Want::Key, value_depth);
                    let value = self.expr(Want::Any, value_depth);
                    format!("({key}): {value}")
                }
                15..=17 => {
                    let spread = self.spread_operand(depth);
                    format!("...{spread}")
                }
                _ => {
                    // the shorthand {name} means {name: name}
                    self.name(!0)
                }
            };
            items.push(item);
        }
        format!("{{{}}}", items.join(", "))
    }

    /// Two range bounds of one regime.
    fn range_bounds(&mut self) -> (String, String) {
        match self.rng.below(20) {
            0..=5 => (
                self.range_operand(RANGE_SMALL_INTS, INT | BOOL),
                self.range_operand(RANGE_SMALL_INTS, INT),
            ),
            6..=8 => (
                self.range_operand(RANGE_LETTERS, STR),
                self.range_operand(RANGE_LETTERS, STR),
            ),
            9..=10 => (
                self.range_operand(RANGE_FLOATS, FLOAT),
                self.range_operand(RANGE_SMALL_INTS, FLOAT | INT),
            ),
            11..=12 => (
                self.range_operand(RANGE_NUMERIC_STRINGS, NUMSTR),
                self.range_operand(RANGE_SMALL_INTS, NUMSTR | INT),
            ),
            13..=14 => (
                self.range_operand(RANGE_LETTERS, STR),
                self.range_operand(RANGE_SMALL_INTS, INT | NUMSTR),
            ),
            15 => (
                self.pick(&["true", "false", "null", "''", "' '"])
                    .to_owned(),
                self.range_operand(RANGE_SMALL_INTS, INT | BOOL | NULL),
            ),
            16..=17 => {
                let (low, high) = *self.rng.pick(RANGE_EDGES);
                (low.to_owned(), high.to_owned())
            }
            _ => {
                let (low, high) = *self.rng.pick(RANGE_WILD);
                if self.percent(50) {
                    (low.to_owned(), high.to_owned())
                } else {
                    (high.to_owned(), low.to_owned())
                }
            }
        }
    }

    /// A literal of `pool` or a small variable of `kind`.
    fn range_operand(&mut self, pool: &[&'static str], kind: u32) -> String {
        if self.percent(30) {
            let candidates: Vec<usize> = (0..VARS.len())
                .filter(|&i| VARS[i].kind & SMALL != 0 && VARS[i].kind & kind != 0)
                .collect();
            if !candidates.is_empty() {
                let index = *self.rng.pick(&candidates);
                if !self.used.contains(&index) {
                    self.used.push(index);
                }
                return VARS[index].name.to_owned();
            }
        }
        self.pick(pool).to_owned()
    }

    // ---- arrow functions ----

    /// An arrow function of `arity` parameters (1 to 3), parenthesized.
    fn arrow(&mut self, arity: usize, depth: u32) -> String {
        const NAMES: &[&[&str]] = &[&["x"], &["v", "k"], &["c", "v", "k"]];
        let names: &[&'static str] = NAMES[arity.clamp(1, 3) - 1];
        let saved = self.params.len();
        self.params.extend_from_slice(names);
        let body = match self.rng.below(10) {
            0..=1 => {
                let op = self.arith_op();
                let param = self.pick(names);
                let other = self.leaf(Want::Num);
                format!("{param} {op} {other}")
            }
            2..=3 => {
                let op = self.comparison_op();
                let param = self.pick(names);
                let other = self.leaf(Want::Any);
                format!("{param} {op} {other}")
            }
            4 => {
                let param = self.pick(names);
                let other = self.leaf(Want::Any);
                format!("{param} ~ {other}")
            }
            5 => {
                let param = self.pick(names);
                format!("{param} ?? 'n'")
            }
            6 => {
                let param = self.pick(names);
                let attr = self.pick(&["a", "0", "x", "foo"]);
                format!("{param}.{attr} ?? {}", self.leaf(Want::Any))
            }
            _ => self.expr(Want::Any, depth.min(2)),
        };
        self.params.truncate(saved);
        let parameters = if names.len() == 1 && self.percent(60) {
            names[0].to_owned()
        } else {
            format!("({})", names.join(", "))
        };
        format!("({parameters} => {body})")
    }

    // ---- attribute access ----

    fn attribute_chain(&mut self, depth: u32) -> String {
        let (mut chain, base_index) = match self.rng.below(20) {
            0..=8 => {
                let index = self.var_index(OBJ | ARR);
                (VARS[index].name.to_owned(), Some(index))
            }
            9..=12 => {
                let index = self.var_index(!0);
                (VARS[index].name.to_owned(), Some(index))
            }
            13..=14 => (self.pick(UNDEFINED).to_owned(), None),
            15..=17 => (
                self.pick(&[
                    "{a: 1, b: {c: 'd'}}",
                    "[10, 20, 30]",
                    "{'0': 'z', '1': 'o'}",
                    "{a: null}",
                ])
                .to_owned(),
                None,
            ),
            _ if !self.params.is_empty() => (self.pick(&self.params.clone()).to_owned(), None),
            _ => {
                let index = self.var_index(OBJ);
                (VARS[index].name.to_owned(), Some(index))
            }
        };
        let attrs: &[&str] = base_index.map_or(DEFAULT_ATTRS, |i| VARS[i].attrs);
        let steps = 1 + self.rng.below(3);
        for step in 0..steps {
            let null_safe = self.percent(self.null_safe);
            let dot = if null_safe { "?." } else { "." };
            let accessor = match self.rng.below(20) {
                0..=8 => {
                    let attr = if step == 0 || self.percent(50) {
                        self.pick(attrs)
                    } else {
                        self.pick(&[
                            "a", "b", "c", "0", "1", "x", "y", "z", "self", "foo", "count",
                        ])
                    };
                    if attr.starts_with('-') {
                        format!("[{attr}]")
                    } else {
                        format!("{dot}{attr}")
                    }
                }
                9..=12 => {
                    let key = if self.percent(50) {
                        let attr = self.pick(attrs);
                        if attr.parse::<i64>().is_ok() {
                            attr.to_owned()
                        } else {
                            format!("'{attr}'")
                        }
                    } else {
                        self.expr(Want::Key, depth.saturating_sub(1).min(1))
                    };
                    format!("[{key}]")
                }
                13..=14 => {
                    let key = self.expr(Want::Key, depth.saturating_sub(1).min(1));
                    format!("{dot}({key})")
                }
                _ => self.method_call(base_index.filter(|_| step == 0), dot),
            };
            chain.push_str(&accessor);
        }
        chain
    }

    fn method_call(&mut self, base: Option<usize>, dot: &str) -> String {
        let name = base.map(|i| VARS[i].name);
        match name {
            Some("foo") => match self.rng.below(8) {
                0 => format!("{dot}bar()"),
                1 => format!("{dot}bar({})", self.leaf(Want::Any)),
                2 => format!(
                    "{dot}bar({}, {})",
                    self.leaf(Want::Any),
                    self.leaf(Want::Any)
                ),
                3 => format!("{dot}strToLower({})", self.leaf(Want::Str)),
                4 => format!("{dot}getSelf().getFoo()"),
                5 => format!("{dot}getNull()"),
                6 => format!("{dot}bar(param2 = {})", self.leaf(Want::Any)),
                _ => format!("{dot}not()"),
            },
            Some("date" | "idate") => match self.rng.below(3) {
                0 => format!("{dot}format({})", self.pick(DATE_FORMATS)),
                1 => format!("{dot}getTimestamp()"),
                _ => format!("{dot}format({})", self.leaf(Want::Str)),
            },
            Some("aobj" | "aiter") => match self.rng.below(4) {
                0 => format!("{dot}count()"),
                1 => format!("{dot}offsetGet({})", self.leaf(Want::Key)),
                2 => format!("{dot}offsetExists({})", self.leaf(Want::Key)),
                _ => format!("{dot}getArrayCopy()"),
            },
            Some("mk") => match self.rng.below(3) {
                0 => format!("{dot}count()"),
                1 => format!("{dot}isSafeFor('html')"),
                _ => format!("{dot}getCharset()"),
            },
            _ => {
                let method = self.pick(&["missing", "count", "a", "format", "x"]);
                if self.percent(50) {
                    format!("{dot}{method}()")
                } else {
                    format!("{dot}{method}({})", self.leaf(Want::Any))
                }
            }
        }
    }

    // ---- statements ----

    fn wrap_value(&mut self, e: &str) -> String {
        match self.rng.below(20) {
            0..=7 => format!("{{{{ {e} }}}}"),
            8..=12 => {
                self.dump = true;
                format!("{{{{ dump({e}) }}}}")
            }
            13..=14 => format!("{{% set r = {e} %}}{{{{ r }}}}"),
            15..=16 => format!("{{{{ ({e})|json_encode }}}}"),
            17 => format!("{{{{ \"[#{{{e}}}]\" }}}}"),
            _ => format!("{{% if {e} %}}T{{% else %}}F{{% endif %}}"),
        }
    }

    fn wrap_bool(&mut self, e: &str) -> String {
        match self.rng.below(10) {
            0..=2 => {
                self.dump = true;
                format!("{{{{ dump({e}) }}}}")
            }
            3..=5 => format!("{{{{ ({e}) ? 'T' : 'F' }}}}"),
            6..=7 => format!("{{% if {e} %}}T{{% else %}}F{{% endif %}}"),
            _ => format!("{{{{ {e} }}}}"),
        }
    }

    fn wrap_iter(&mut self, e: &str) -> String {
        match self.rng.below(10) {
            0..=2 => {
                self.dump = true;
                format!("{{{{ dump({e}) }}}}")
            }
            3..=4 => format!("{{{{ ({e})|json_encode }}}}"),
            5..=6 => format!("{{% for k, v in {e} %}}{{{{ k }}}}={{{{ v }}}};{{% endfor %}}"),
            7 => format!("{{{{ ({e})|join(',') }}}}"),
            8 => format!("{{{{ ({e})|length }}}}"),
            _ => format!("{{{{ {e} }}}}"),
        }
    }

    fn wrap_attr(&mut self, e: &str) -> String {
        match self.rng.below(10) {
            0..=3 => format!("{{{{ {e} }}}}"),
            4..=5 => {
                let default = self.leaf(Want::Any);
                format!("{{{{ {e} ?? {default} }}}}")
            }
            6 => {
                self.dump = true;
                format!("{{{{ dump({e}) }}}}")
            }
            7 => format!("{{{{ {e} is defined ? 'D' : 'U' }}}}"),
            8 => format!("{{{{ {e} is null ? 'N' : 'V' }}}}"),
            _ => format!("{{% if {e} %}}T{{% else %}}F{{% endif %}}"),
        }
    }

    fn statement(&mut self, profile: Profile) -> String {
        match profile {
            Profile::Arith => {
                let depth = self.depth(1, 3);
                let op = self.arith_op();
                let left = self.sub(Want::Num, depth - 1);
                let right = self.sub(Want::Num, depth - 1);
                let e = if self.percent(15) {
                    let unary = self.pick(&["-", "+", "-", "not "]);
                    format!("{unary}{left} {op} {right}")
                } else {
                    format!("{left} {op} {right}")
                };
                self.wrap_value(&e)
            }
            Profile::Concat => {
                let depth = self.depth(1, 2);
                let e = if self.percent(70) {
                    let n = 2 + self.rng.below(2);
                    let parts: Vec<String> = (0..n).map(|_| self.sub(Want::Any, depth)).collect();
                    parts.join(" ~ ")
                } else {
                    self.interpolation(depth)
                };
                if self.percent(80) {
                    format!("{{{{ {e} }}}}")
                } else {
                    self.wrap_value(&e)
                }
            }
            Profile::Compare => {
                let depth = self.depth(1, 2);
                let op = self.comparison_op();
                let left = self.sub(Want::Any, depth - 1);
                let right = self.sub(Want::Any, depth - 1);
                let e = format!("{left} {op} {right}");
                if op == "<=>" && self.percent(50) {
                    format!("{{{{ {e} }}}}")
                } else {
                    self.wrap_bool(&e)
                }
            }
            Profile::StringOps => {
                let depth = self.depth(1, 2);
                let e = self.string_op(depth);
                self.wrap_bool(&e)
            }
            Profile::Range => {
                let (low, high) = self.range_bounds();
                let e = format!("{low}..{high}");
                if self.percent(25) {
                    self.wrap_iter(&format!("({e})"))
                } else {
                    self.wrap_iter(&e)
                }
            }
            Profile::Logic => {
                let depth = self.depth(1, 3);
                let e = match self.rng.below(10) {
                    0..=3 => {
                        let op = self.pick(&["and", "or", "xor", "b-and", "b-or", "b-xor"]);
                        let left = self.sub(Want::Bool, depth - 1);
                        let right = self.sub(Want::Bool, depth - 1);
                        format!("{left} {op} {right}")
                    }
                    4 => {
                        let operand = self.sub(Want::Bool, depth - 1);
                        format!("not {operand}")
                    }
                    5..=6 => {
                        let op = self.pick(&["b-and", "b-or", "b-xor"]);
                        let left = self.sub(Want::Num, depth - 1);
                        let right = self.sub(Want::Num, depth - 1);
                        format!("{left} {op} {right}")
                    }
                    _ => self.conditional(Want::Any, depth),
                };
                self.wrap_value(&e)
            }
            Profile::Arrays => {
                let depth = self.depth(1, 3);
                let e = if self.percent(45) {
                    self.mapping(depth)
                } else {
                    self.sequence(depth)
                };
                self.wrap_iter(&e)
            }
            Profile::Arrows => self.arrow_statement(),
            Profile::Attributes => {
                let depth = self.depth(1, 2);
                let e = self.attribute_chain(depth);
                self.wrap_attr(&e)
            }
            Profile::Literals => self.literal_statement(),
            Profile::ToString => {
                let value = match self.rng.below(10) {
                    0..=5 => self.name(!0),
                    6..=7 => self.leaf(Want::Num),
                    8 => self.pick(STRING_LITERALS).to_owned(),
                    _ => self.leaf(Want::Iter),
                };
                match self.rng.below(6) {
                    0..=2 => format!("{{{{ {value} }}}}"),
                    3 => format!("{{{{ {value} ~ '' }}}}"),
                    4 => format!("{{{{ \"#{{{value}}}\" }}}}"),
                    _ => format!("{{% set s %}}{{{{ {value} }}}}{{% endset %}}{{{{ s }}}}"),
                }
            }
            Profile::Mixed => {
                let depth = self.depth(2, 3);
                let e = self.expr(Want::Any, depth);
                self.wrap_value(&e)
            }
            Profile::Coalesce => self.coalesce_statement(),
            Profile::Assign => self.assign_statement(),
            Profile::Overflow => {
                let n = 1 + self.rng.below(2);
                let mut e = self.overflow_operand();
                for _ in 0..n {
                    let op = self.pick(&[
                        "+", "-", "*", "/", "//", "%", "**", "b-and", "b-or", "b-xor", "+", "-",
                        "*",
                    ]);
                    let operand = self.overflow_operand();
                    e = format!("{e} {op} {operand}");
                }
                if self.percent(20) {
                    e = format!("-({e})");
                }
                match self.rng.below(4) {
                    0..=1 => {
                        self.dump = true;
                        format!("{{{{ dump({e}) }}}}")
                    }
                    2 => format!("{{{{ {e} }}}}"),
                    _ => format!("{{{{ ({e})|json_encode }}}}"),
                }
            }
            Profile::Loose => {
                let op = self.pick(&[
                    "==", "!=", "<", ">", "<=", ">=", "<=>", "===", "!==", "==", "<=>", "in",
                ]);
                let left = self.loose_operand();
                let right = self.loose_operand();
                let e = format!("{left} {op} {right}");
                match self.rng.below(3) {
                    0 => {
                        self.dump = true;
                        format!("{{{{ dump({e}) }}}}")
                    }
                    1 => format!("{{{{ ({e}) ? 'T' : 'F' }}}}"),
                    _ => format!("{{{{ {e} }}}}"),
                }
            }
        }
    }

    fn coalesce_statement(&mut self) -> String {
        self.null_safe = 45;
        let depth = self.depth(1, 2);
        let chain = self.attribute_chain(depth);
        let s = match self.rng.below(12) {
            0..=2 => {
                let default = self.leaf(Want::Any);
                format!("{{{{ {chain} ?? {default} }}}}")
            }
            3 => {
                let other = self.attribute_chain(depth);
                let default = self.leaf(Want::Any);
                format!("{{{{ {chain} ?? {other} ?? {default} }}}}")
            }
            4 => {
                let default = self.leaf(Want::Any);
                format!("{{{{ {chain} ?: {default} }}}}")
            }
            5 => format!("{{{{ {chain} is defined ? 'D' : 'U' }}}}"),
            6 => format!("{{{{ {chain} is not defined ? 'U' : 'D' }}}}"),
            7 => {
                let default = self.leaf(Want::Any);
                format!("{{{{ ({chain} ?? {default})|json_encode }}}}")
            }
            8 => {
                let container = self.name(ARR | OBJ | SCALAR);
                let default = self.leaf(Want::Any);
                format!("{{{{ {container}[{chain}] ?? {default} }}}}")
            }
            9 => {
                let container = self.name(ARR | OBJ | SCALAR);
                format!("{{{{ {container}[{chain}] }}}}")
            }
            10 => format!("{{{{ {chain} is null ? 'N' : 'V' }}}}"),
            _ => format!("{{% if {chain} is defined and {chain} %}}T{{% else %}}F{{% endif %}}"),
        };
        self.null_safe = 18;
        s
    }

    fn assign_statement(&mut self) -> String {
        let value = self.sub(Want::Any, 1);
        let source = if self.percent(75) {
            self.sub(Want::Iter, 1)
        } else {
            self.leaf(Want::Any)
        };
        match self.rng.below(9) {
            0 => format!("{{% set r = (a = {value}) %}}{{{{ r }}}}|{{{{ a }}}}"),
            1 => format!("{{{{ (a = {value}) ~ '|' ~ a }}}}"),
            2 => format!("{{% do [a, b] = {source} %}}{{{{ a ?? 'n' }}}}|{{{{ b ?? 'n' }}}}"),
            3 => format!("{{% do {{a, b: c}} = {source} %}}{{{{ a ?? 'n' }}}}|{{{{ c ?? 'n' }}}}"),
            4 => {
                self.dump = true;
                format!("{{{{ dump([a, , b] = {source}) }}}}|{{{{ a ?? 'n' }}}}")
            }
            5 => {
                self.dump = true;
                format!("{{{{ dump({{a, x: b}} = {source}) }}}}|{{{{ b ?? 'n' }}}}")
            }
            6 => {
                let arrow = self.arrow(1, 1);
                format!("{{% set f = {arrow} %}}{{{{ {source}|map(f)|json_encode }}}}")
            }
            7 if self.percent(70) => {
                format!("{{% do a = b = {value} %}}{{{{ a }}}}|{{{{ b }}}}")
            }
            // a Closure in output, in a concatenation and in a comparison
            7 => format!(
                "{{% set f = (x => x) %}}{{{{ f ~ '' }}}}|{{{{ f == {value} ? 'T' : 'F' }}}}"
            ),
            _ => format!("{{{{ ([a, b] = {source}) is iterable ? a ~ b : 'x' }}}}"),
        }
    }

    fn overflow_operand(&mut self) -> String {
        if self.percent(20) {
            return self.var(INT);
        }
        self.pick(&[
            "9223372036854775807",
            "(-9223372036854775807 - 1)",
            "9223372036854775806",
            "-9223372036854775807",
            "4611686018427387904",
            "-4611686018427387904",
            "3037000499",
            "3037000500",
            "1",
            "-1",
            "2",
            "0",
            "3",
            "1e19",
            "9.2233720368547758E+18",
            "-9.2233720368547758E+18",
            "'9223372036854775807'",
            "'9223372036854775808'",
            "' 9223372036854775807'",
            "'-9223372036854775808'",
            "64",
            "63",
            "-64",
            "0.5",
            "2147483648",
        ])
        .to_owned()
    }

    fn loose_operand(&mut self) -> String {
        match self.rng.below(10) {
            0..=6 => self
                .pick(&[
                    "0",
                    "1",
                    "-1",
                    "0.0",
                    "-0.0",
                    "1.0",
                    "'0'",
                    "'1'",
                    "''",
                    "' '",
                    "'0.0'",
                    "'00'",
                    "'1e0'",
                    "'1e3'",
                    "'1000'",
                    "' 1'",
                    "'1 '",
                    "'abc'",
                    "'ABC'",
                    "'1abc'",
                    "'0x0'",
                    "'INF'",
                    "'NAN'",
                    "'-0'",
                    "null",
                    "true",
                    "false",
                    "[]",
                    "[0]",
                    "[null]",
                    "[1, 2]",
                    "[2, 1]",
                    "{a: 1}",
                    "{b: 1}",
                    "{'0': 1}",
                    "[[]]",
                    "1e400",
                    "-1e400",
                    "constant('NAN')",
                    "'2020-01-02 03:04:05'",
                    "9223372036854775807",
                    "9223372036854775808",
                    "'9223372036854775807'",
                    "'9223372036854775808'",
                    "'1e1000'",
                    "'-1e1000'",
                ])
                .to_owned(),
            _ => self.name(!0),
        }
    }

    fn arrow_statement(&mut self) -> String {
        let subject = self.sub(Want::Iter, 1);
        let s = match self.rng.below(9) {
            0..=1 => {
                let arity = 1 + self.rng.below(2);
                let arrow = self.arrow(arity, 2);
                format!("{{{{ {subject}|map({arrow})|json_encode }}}}")
            }
            2 => {
                let arity = 1 + self.rng.below(2);
                let arrow = self.arrow(arity, 2);
                format!("{{{{ {subject}|filter({arrow})|json_encode }}}}")
            }
            3 => {
                let arity = 2 + self.rng.below(2);
                let arrow = self.arrow(arity, 2);
                let initial = self.leaf(Want::Any);
                format!("{{{{ {subject}|reduce({arrow}, {initial})|json_encode }}}}")
            }
            4 => {
                let arrow = self.arrow(2, 2);
                format!("{{{{ {subject}|sort({arrow})|json_encode }}}}")
            }
            5 => {
                let arity = 1 + self.rng.below(2);
                let arrow = self.arrow(arity, 2);
                format!("{{{{ {subject}|find({arrow})|json_encode }}}}")
            }
            _ => {
                let e = self.has_some_every(2);
                format!("{{{{ ({e}) ? 'Y' : 'N' }}}}")
            }
        };
        if self.percent(25) {
            // the arrow sees the context as it was when it was created; a parameter shadows a
            // variable of the same name only inside the arrow
            let value = self.leaf(Want::Any);
            let name = self.pick(&["x", "v", "k", "y"]);
            format!("{{% set {name} = {value} %}}{s}|{{{{ {name} ?? 'u' }}}}")
        } else {
            s
        }
    }

    fn literal_statement(&mut self) -> String {
        let literal = match self.rng.below(10) {
            0..=3 => self.pick(INT_LITERALS).to_owned(),
            4..=6 => self.pick(FLOAT_LITERALS).to_owned(),
            7..=8 => self.pick(STRING_LITERALS).to_owned(),
            _ => self.interpolation(1),
        };
        match self.rng.below(8) {
            0..=2 => format!("{{{{ {literal} }}}}"),
            3..=4 => {
                self.dump = true;
                format!("{{{{ dump({literal}) }}}}")
            }
            5 => format!("{{{{ -{literal} }}}}"),
            6 => format!("{{{{ {literal} ~ '|' ~ ({literal}) * 1 }}}}"),
            _ => {
                let other = self.pick(FLOAT_LITERALS);
                format!("{{{{ {literal} is same as({other}) ? 'S' : 'D' }}}}")
            }
        }
    }

    // ---- the case ----

    fn case(&mut self, profile: Profile, id: &str) -> Json {
        let statements = 1 + self.rng.below(3);
        let mut template = String::new();
        for n in 0..statements {
            if n > 0 {
                template.push('|');
            }
            let statement = self.statement(profile);
            template.push_str(&statement);
        }
        if self.percent(15) {
            // the line of a runtime error depends on Twig's `// line N` markers inside expressions
            template = spread_lines(&template, self.rng);
        }

        let mut fields: Vec<(String, Json)> = vec![
            ("id".to_owned(), Json::str(id)),
            ("env".to_owned(), Json::str("plain")),
            (
                "templates".to_owned(),
                Json::Object(vec![("index.twig".to_owned(), Json::str(template))]),
            ),
        ];

        let used: Vec<&Var> = self.used.iter().map(|&i| &VARS[i]).collect();
        if used.iter().all(|v| v.json.is_some()) {
            let entries = used
                .iter()
                .map(|v| {
                    let value = Json::parse(v.json.unwrap_or("null")).unwrap_or(Json::Null);
                    (v.name.to_owned(), value)
                })
                .collect();
            fields.push(("data_json".to_owned(), Json::Object(entries)));
        } else {
            fields.push(("data_php".to_owned(), Json::str(data_php(&used))));
            if used.iter().any(|v| v.kind & TEST_CLASS != 0) {
                fields.push(("test_classes".to_owned(), Json::Bool(true)));
            }
        }

        let mut config: Vec<(String, Json)> = Vec::new();
        let autoescape = if profile == Profile::ToString {
            *self.rng.pick(&[
                Autoescape::False,
                Autoescape::Html,
                Autoescape::Js,
                Autoescape::Css,
                Autoescape::Url,
                Autoescape::HtmlAttr,
            ])
        } else {
            self.rng.weighted(&[
                (40, Autoescape::False),
                (45, Autoescape::Html),
                (4, Autoescape::Js),
                (3, Autoescape::Css),
                (3, Autoescape::Url),
                (5, Autoescape::HtmlAttr),
            ])
        };
        config.push(("autoescape".to_owned(), autoescape.json()));
        let strict = if profile == Profile::Attributes {
            45
        } else {
            25
        };
        if self.percent(strict) {
            config.push(("strict_variables".to_owned(), Json::Bool(true)));
        }
        if self.dump {
            config.push(("debug".to_owned(), Json::Bool(true)));
        }
        fields.push(("config".to_owned(), Json::Object(config)));
        let mut extensions = Vec::new();
        if self.dump {
            extensions.push(Json::str("debug"));
        }
        // the SandboxExtension (not sandboxing) changes how `x[...]` compiles (ARRAY_LIKE_CLASSES)
        let sandbox = if profile == Profile::Attributes {
            20
        } else {
            5
        };
        if self.percent(sandbox) {
            extensions.push(Json::str("sandbox"));
        }
        if !extensions.is_empty() {
            fields.push(("extensions".to_owned(), Json::Array(extensions)));
        }
        if self.percent(12) {
            fields.push(("errors".to_owned(), Json::str("throw")));
        }
        Json::Object(fields)
    }
}

/// The `autoescape` option values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Autoescape {
    False,
    Html,
    Js,
    Css,
    Url,
    HtmlAttr,
}

impl Autoescape {
    fn json(self) -> Json {
        match self {
            Autoescape::False => Json::Bool(false),
            Autoescape::Html => Json::str("html"),
            Autoescape::Js => Json::str("js"),
            Autoescape::Css => Json::str("css"),
            Autoescape::Url => Json::str("url"),
            Autoescape::HtmlAttr => Json::str("html_attr"),
        }
    }
}

/// Whether an expression is one token (no parentheses needed around it).
fn atomic(e: &str) -> bool {
    e.bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'.')
        || (e.len() >= 2
            && e.starts_with('\'')
            && e.ends_with('\'')
            && !e[1..e.len() - 1].contains('\''))
}

/// The template with some of the spaces inside `{{ }}` and `{% %}` tags (outside string
/// literals) replaced by newlines.
fn spread_lines(template: &str, rng: &mut Rng) -> String {
    let mut out = String::with_capacity(template.len());
    let mut in_tag = false;
    let mut quote: Option<char> = None;
    let mut previous = '\0';
    for c in template.chars() {
        match quote {
            Some(q) => {
                if c == q && previous != '\\' {
                    quote = None;
                }
                out.push(c);
            }
            None if !in_tag => {
                if (c == '{' || c == '%') && previous == '{' {
                    in_tag = true;
                }
                out.push(c);
            }
            None => {
                match c {
                    '\'' | '"' => quote = Some(c),
                    '}' if previous == '}' || previous == '%' => in_tag = false,
                    _ => {}
                }
                if c == ' ' && rng.percent(35) {
                    out.push('\n');
                } else {
                    out.push(c);
                }
            }
        }
        previous = if previous == '\\' && c == '\\' {
            '\0'
        } else {
            c
        };
    }
    out
}

/// The data code: the classes it needs, then the array of the used variables.
fn data_php(vars: &[&Var]) -> String {
    let mut code = String::new();
    if vars.iter().any(|v| v.kind & STRINGY != 0) {
        // expressions/array_dynamic_keys.test declares it the same way
        code.push_str(
            "if (!class_exists('TwigTestStringy', false)) { class TwigTestStringy implements \\Stringable { \
             public function __construct(private string $v) {} \
             public function __toString(): string { return $this->v; } } } ",
        );
    }
    code.push_str("return [");
    for (n, v) in vars.iter().enumerate() {
        if n > 0 {
            code.push_str(", ");
        }
        let _ = write!(code, "'{}' => {}", v.name, v.php);
    }
    code.push(']');
    code
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kit::case::Case;

    #[test]
    fn deterministic_and_parseable() {
        let first = generate(7, 300);
        let second = generate(7, 300);
        assert_eq!(first, second);
        let mut ids = std::collections::BTreeSet::new();
        for case in &first {
            let parsed = Case::from_json(case).expect("a valid case");
            assert!(parsed.unsupported_reason().is_none(), "{case:?}");
            assert!(ids.insert(parsed.id.clone()), "duplicate id {}", parsed.id);
            // data_json must survive a round trip through the JSON text
            let text = case.to_compact();
            assert_eq!(&Json::parse(&text).unwrap(), case);
        }
        assert_ne!(generate(8, 10), first[..10].to_vec());
    }

    #[test]
    fn palette_is_consistent() {
        let mut names = std::collections::BTreeSet::new();
        for v in VARS {
            assert!(names.insert(v.name), "duplicate variable {}", v.name);
            if let Some(json) = v.json {
                assert!(Json::parse(json).is_ok(), "{}: {json}", v.name);
            } else {
                assert!(v.kind & (OBJ | FLOAT) != 0, "{} needs JSON", v.name);
            }
        }
        for name in UNDEFINED {
            assert!(!names.contains(name));
        }
    }

    #[test]
    fn every_profile_appears() {
        let cases = generate(1, 600);
        for (_, profile) in PROFILES {
            let tag = format!("-{}", profile.name());
            assert!(
                cases.iter().any(|c| c
                    .get("id")
                    .and_then(Json::as_str)
                    .is_some_and(|id| id.ends_with(&tag))),
                "{tag}"
            );
        }
    }
}
