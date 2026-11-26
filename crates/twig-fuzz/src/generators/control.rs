//! Seeded case generator for the control flow and scoping family.
//!
//! Every case is a random but meaningful template around the statements that move values in and
//! out of `$context`, with data and settings chosen to hit their edge cases:
//!
//! - `if` / `elseif` / `else` over values of every truthiness (`'0'`, `0.0`, `[]`, empty `Markup`,
//!   empty objects, ...), conditions with tests, comparisons, `??` and assignments;
//! - `for` with key and value targets (shadowing outer variables, globals, other targets, `loop`,
//!   `_key`, `_parent`, `_seq`, `_context`), `else`, every `loop` field, nested loops and
//!   `loop.parent`, sequences of every kind (lists, maps with numeric-string keys, strings,
//!   scalars, `null`, `Traversable` / `Countable` / `IteratorAggregate` objects, a generator, an
//!   `SplObjectStorage`, the context itself, `loop`, `_seq`, ranges, filtered sequences), mutation
//!   of the sequence, of outer variables and of the loop's own variables inside the body, the same
//!   iterator object iterated by nested loops;
//! - `set` (single, multi-target, capture), assignment (`=`) and destructuring expressions, `do`;
//! - `with` (with and without variables, `only`, non-mapping values, lists, `Traversable`s);
//! - `apply` with filter chains, sets inside capture bodies (they share the scope);
//! - probes of the exact contents and key order of `_context` after each construct
//!   (`_context|keys|join`, `dump(_context|keys)`, `dump()`, `_context|json_encode`), the
//!   optimizer's `loop` detection (`_context.loop`, `attribute(loop, ...)`, includes, blocks),
//!   `guard`, whitespace control on the tags;
//! - loops written the way templates usually write them (records with attributes, `loop.first`
//!   / `loop.last`, `cycle()`, totals, `filter`/`sort`/`batch`, a nested loop over a record's
//!   list), macros called from loops (their own context: arguments, `varargs`, globals);
//! - overwriting what `ForNode` manages (`loop`, `_parent`, `_seq`, `_iterated`) inside the body,
//!   so that the bookkeeping after the body (`++$context['loop']['index0']`, the restore with
//!   `array_intersect_key()`) runs on other values;
//! - globals shadowed by data and by template variables, `strict_variables` on and off, the
//!   `optimizations` and `use_yield` options, warnings collected or thrown, `guard` with unknown
//!   callables and blocks in the dropped branch, and syntax errors of these tags.
//!
//! [`generate`] is deterministic: case `n` of seed `s` depends only on `(s, n)` ([`case`]), so a
//! batch is reproduced from its seed and size. Ids are `ctl-<seed>-<n>`. Half of the cases are
//! "clean" (no undefined names, special names as targets, attribute access on arbitrary values,
//! non-mappings in `with` or syntax errors), so they usually render to the end; the other half
//! goes for errors.
//!
//! The generator avoids what cannot match by design (no `random()`, dates or `dump()` of objects;
//! `dump()` without arguments only with JSON data) and the operand-type errors of `+ * b-and b-or
//! b-xor` and `is not ... **` already known from `crates/twig-fuzz/vectors/round0-mismatches.json`.
//! It also avoids cases that would not end: inside loops an assigned value reads earlier
//! assignments at most once and never embeds the context (`loop.parent`, `_parent`, `_context`,
//! their JSON), so values grow linearly with the iterations; and operations that stop iterating
//! early (`first`, `slice`, `in`, sequence destructuring, `rewind()`) are only applied to literals
//! inside loops over objects, since rewinding the `Iterator` a loop traverses restarts it forever.

use crate::kit::json::Json;

/// The family this generator covers.
pub const FAMILY: &str = "control";

/// `count` cases of seed `seed` (case `n` is [`case`]`(seed, n)`).
pub fn generate(seed: u64, count: usize) -> Vec<Json> {
    (0..count).map(|index| case(seed, index)).collect()
}

/// Case `index` of seed `seed`: an oracle "twig" case (`"env": "plain"`) with `data_json`, or
/// `data_php` with the helper classes of Twig's test suite (`"test_classes": true`).
pub fn case(seed: u64, index: usize) -> Json {
    let mut rng = Rng(seed
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add((index as u64).wrapping_mul(0xD1B5_4A32_D192_ED03)));
    rng.next_u64();
    super::with_case_cache(Gen::new(rng).build(format!("ctl-{seed}-{index}")))
}

/// SplitMix64: small and seedable.
struct Rng(u64);

impl Rng {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: usize) -> usize {
        if n <= 1 {
            0
        } else {
            (self.next_u64() % n as u64) as usize
        }
    }

    fn percent(&mut self, p: usize) -> bool {
        self.below(100) < p
    }

    fn range(&mut self, lo: usize, hi: usize) -> usize {
        lo + self.below(hi - lo + 1)
    }

    fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        items[self.below(items.len())]
    }

    fn weighted(&mut self, weights: &[usize]) -> usize {
        let total: usize = weights.iter().sum();
        let mut r = self.below(total.max(1));
        for (i, w) in weights.iter().enumerate() {
            if r < *w {
                return i;
            }
            r -= w;
        }
        weights.len() - 1
    }
}

/// What a data variable is, for choosing sequences and conditions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// A list array.
    List,
    /// An array with string, numeric-string or sparse keys.
    Map,
    /// `[]`.
    Empty,
    /// A string, number, boolean or `null` (a loop over it runs zero times).
    Scalar,
    /// A `Traversable` that is not `Countable` (no `loop.length`).
    Iter,
    /// A `Traversable` and `Countable` object.
    IterCount,
    /// An object that is not `Traversable` (a loop over it runs zero times).
    Opaque,
}

/// A variable a case's data may contain.
struct DataVar {
    name: &'static str,
    kind: Kind,
    /// Iterations of a loop over it.
    size: usize,
    /// JSON text for `data_json` cases; `None`: only in `data_php` cases.
    json: Option<&'static str>,
    /// The PHP expression for `data_php` cases.
    php: &'static str,
    /// PHP statements the expression needs first (class declarations).
    needs: Option<&'static str>,
    /// Chance (percent) of being part of a case's data.
    weight: usize,
}

const ITEMS_ITERATOR: &str = r"if (!class_exists('ItemsIterator', false)) {
class ItemsIterator implements \Iterator
{
  protected $values = ['foo' => 'bar', 'bar' => 'foo'];
  #[\ReturnTypeWillChange]
  public function current() { return current($this->values); }
  #[\ReturnTypeWillChange]
  public function key() { return key($this->values); }
  public function next(): void { next($this->values); }
  public function rewind(): void { reset($this->values); }
  public function valid(): bool { return false !== current($this->values); }
}
}
";

const ITEMS_ITERATOR_COUNTABLE: &str = r"if (!class_exists('ItemsIteratorCountable', false)) {
class ItemsIteratorCountable implements \Iterator, \Countable
{
  protected $values = ['foo' => 'bar', 'bar' => 'foo'];
  #[\ReturnTypeWillChange]
  public function current() { return current($this->values); }
  #[\ReturnTypeWillChange]
  public function key() { return key($this->values); }
  public function next(): void { next($this->values); }
  public function rewind(): void { reset($this->values); }
  public function valid(): bool { return false !== current($this->values); }
  public function count(): int { return count($this->values); }
}
}
";

/// The `IteratorAggregate` with a generator `getIterator()` of expressions/set.test (twig-test-support
/// ports it under its anonymous class name).
const PAIR_TRAVERSABLE: &str = r"new class implements IteratorAggregate {
        public int $yielded = 0;

        public function getIterator(): Traversable
        {
            foreach ([5 => 'Fabien', 8 => 'Potencier', 13 => 'Ignored'] as $key => $value) {
                ++$this->yielded;
                yield $key => $value;
            }
        }
    }";

const SPL_STORAGE: &str = r"(static function () { $s = new \SplObjectStorage(); $s[(object) ['id' => 1]] = 'a'; $s[(object) ['id' => 2]] = 'b'; return $s; })()";

const fn var(
    name: &'static str,
    kind: Kind,
    size: usize,
    json: Option<&'static str>,
    php: &'static str,
    weight: usize,
) -> DataVar {
    DataVar {
        name,
        kind,
        size,
        json,
        php,
        needs: None,
        weight,
    }
}

const DATA: &[DataVar] = &[
    var("items", Kind::List, 3, Some("[1,2,3]"), "[1, 2, 3]", 100),
    var(
        "list",
        Kind::List,
        2,
        Some(r#"["a","b"]"#),
        "['a', 'b']",
        60,
    ),
    var("one", Kind::List, 1, Some("[7]"), "[7]", 40),
    var(
        "map",
        Kind::Map,
        2,
        Some(r#"{"x":1,"y":2}"#),
        "['x' => 1, 'y' => 2]",
        60,
    ),
    var(
        "mixed",
        Kind::Map,
        5,
        Some(r#"{"1":"one","a":"A","01":"z","-1":"m","1.5":"f"}"#),
        "['1' => 'one', 'a' => 'A', '01' => 'z', '-1' => 'm', '1.5' => 'f']",
        35,
    ),
    var(
        "nested",
        Kind::List,
        3,
        Some("[[1,2],[],[3]]"),
        "[[1, 2], [], [3]]",
        35,
    ),
    var(
        "tree",
        Kind::Map,
        2,
        Some(r#"{"a":{"b":[1,2]},"c":[]}"#),
        "['a' => ['b' => [1, 2]], 'c' => []]",
        25,
    ),
    var("empty", Kind::Empty, 0, Some("[]"), "[]", 40),
    var(
        "users",
        Kind::List,
        3,
        Some(
            r#"[{"name":"Ann","age":30,"tags":["a","b"],"active":true},{"name":"Bob","age":null,"tags":[],"active":false},{"name":"Cy","active":"0"}]"#,
        ),
        r"[(object) ['name' => 'Ann', 'age' => 30, 'tags' => ['a', 'b'], 'active' => true], new \ArrayObject(['name' => 'Bob', 'age' => null, 'tags' => [], 'active' => false]), ['name' => 'Cy', 'active' => '0']]",
        45,
    ),
    var(
        "holes",
        Kind::Map,
        3,
        Some(r#"{"0":"a","2":"c","5":"f"}"#),
        "[0 => 'a', 2 => 'c', 5 => 'f']",
        25,
    ),
    var(
        "bools",
        Kind::List,
        9,
        Some(r#"[true,false,null,0,"","0",[],"a",0.0]"#),
        "[true, false, null, 0, '', '0', [], 'a', 0.0]",
        30,
    ),
    var("s", Kind::Scalar, 0, Some(r#""abc""#), "'abc'", 40),
    var("es", Kind::Scalar, 0, Some(r#""""#), "''", 30),
    var("zs", Kind::Scalar, 0, Some(r#""0""#), "'0'", 30),
    var("n", Kind::Scalar, 0, Some("3"), "3", 40),
    var("z", Kind::Scalar, 0, Some("0"), "0", 30),
    var("neg", Kind::Scalar, 0, Some("-2"), "-2", 15),
    var("f", Kind::Scalar, 0, Some("1.5"), "1.5", 20),
    var("fz", Kind::Scalar, 0, Some("0.0"), "0.0", 20),
    var("t", Kind::Scalar, 0, Some("true"), "true", 30),
    var("fl", Kind::Scalar, 0, Some("false"), "false", 30),
    var("nul", Kind::Scalar, 0, Some("null"), "null", 30),
    var(
        "ao",
        Kind::IterCount,
        3,
        None,
        r"new \ArrayObject(['b' => 2, 'a' => 1, 3 => 'c'])",
        45,
    ),
    var("eao", Kind::IterCount, 0, None, r"new \ArrayObject([])", 25),
    var(
        "ai",
        Kind::IterCount,
        3,
        None,
        r"new \ArrayIterator([10, 20, 30])",
        40,
    ),
    var(
        "aon",
        Kind::IterCount,
        2,
        None,
        r"new \ArrayObject([[1, 2], ['k' => 'v']])",
        20,
    ),
    var(
        "obj",
        Kind::Opaque,
        0,
        None,
        "(object) ['p' => 1, 'q' => [2, 3]]",
        35,
    ),
    var("eobj", Kind::Opaque, 0, None, r"new \stdClass()", 20),
    var(
        "agg",
        Kind::Iter,
        2,
        None,
        r"new \Twig\Tests\IteratorAggregateStub(['x' => 1, 'y' => 2])",
        40,
    ),
    var(
        "sit",
        Kind::Iter,
        7,
        None,
        r"new \Twig\Tests\SimpleIteratorForTesting()",
        25,
    ),
    var(
        "foo",
        Kind::Iter,
        2,
        None,
        r"new \Twig\Tests\TwigTestFoo()",
        35,
    ),
    var(
        "cnt",
        Kind::Opaque,
        0,
        None,
        r"new \Twig\Tests\CountableStub(2)",
        25,
    ),
    var(
        "mk",
        Kind::Opaque,
        0,
        None,
        r"new \Twig\Markup('<b>m</b>', 'UTF-8')",
        30,
    ),
    var(
        "emk",
        Kind::Opaque,
        0,
        None,
        r"new \Twig\Markup('', 'UTF-8')",
        30,
    ),
    var(
        "str",
        Kind::Opaque,
        0,
        None,
        r"new \Twig\Tests\ToStringStub('ts')",
        25,
    ),
    var("gen", Kind::Iter, 3, None, PAIR_TRAVERSABLE, 40),
    var("sos", Kind::IterCount, 2, None, SPL_STORAGE, 25),
    DataVar {
        needs: Some(ITEMS_ITERATOR),
        ..var("ii", Kind::Iter, 2, None, r"new \ItemsIterator()", 30)
    },
    DataVar {
        needs: Some(ITEMS_ITERATOR_COUNTABLE),
        ..var(
            "iic",
            Kind::IterCount,
            2,
            None,
            r"new \ItemsIteratorCountable()",
            30,
        )
    },
];

/// Globals a case may define: `(name, JSON value)`; names of data variables, loop targets and
/// `loop` itself are among them so that data and template variables shadow them.
const GLOBALS: &[(&str, &str)] = &[
    ("g", r#""G""#),
    ("gl", "[1,2]"),
    ("gz", "0"),
    ("a", r#""ga""#),
    ("x", r#""gx""#),
    ("v", r#""gv""#),
    ("k", r#""gk""#),
    ("items", r#""gitems""#),
    ("map", r#"{"g":1}"#),
    ("loop", r#""gloop""#),
    ("_key", r#""g_key""#),
    ("_parent", r#"{"gp":1}"#),
    ("_seq", r#""g_seq""#),
];

/// Names templates assign to; data and global names are added per case.
const FRESH: &[&str] = &[
    "a", "b", "c", "x", "y", "acc", "tmp", "v", "k", "item", "key", "value", "i", "res", "A", "_",
    "é", "x_1", "__",
];

/// Names a template may read without assigning them first.
const MAYBE_UNDEFINED: &[&str] = &[
    "a",
    "b",
    "c",
    "x",
    "undef",
    "k",
    "v",
    "loop",
    "_key",
    "_parent",
    "_seq",
    "_iterated",
    "g",
    "acc",
    "item",
];

/// The special names of `NameExpression` and the private variables of `ForNode`.
const SPECIAL: &[&str] = &[
    "_key",
    "_seq",
    "_parent",
    "_iterated",
    "_context",
    "_self",
    "_charset",
];

const SCALARS: &[&str] = &[
    "0",
    "1",
    "-1",
    "2",
    "'0'",
    "''",
    "' '",
    "'a'",
    "'b'",
    "null",
    "true",
    "false",
    "0.0",
    "1.5",
    "'1'",
    "\"x#{1}\"",
    "'abc'",
    "-0.0",
    "'0.0'",
];

const ARRAYS: &[&str] = &[
    "[]",
    "[0]",
    "[1, 2, 3]",
    "['a', 'b']",
    "[null]",
    "[[1], [2, 3]]",
    "{a: 1, b: 2}",
    "{'1': 'x', '01': 'y', 2: 'z'}",
    "{x: [1], y: {z: 2}}",
    "[true, false, '', '0', 'x']",
    "{'-1': 'm', '1.0': 'f', 3: 't'}",
];

const RANGES: &[(&str, usize)] = &[
    ("1..3", 3),
    ("3..1", 3),
    ("0..0", 1),
    ("'a'..'c'", 3),
    ("range(0, 6, 3)", 3),
    ("range(5, 3)", 3),
    ("range('a', 'e', 2)", 3),
    ("-1..1", 3),
];

const LOOP_FIELDS: &[&str] = &[
    "index",
    "index0",
    "revindex",
    "revindex0",
    "first",
    "last",
    "length",
];

/// Filters applied to a data variable used as a loop sequence.
const SEQ_FILTERS: &[&str] = &[
    "reverse",
    "keys",
    "slice(1)",
    "merge([9])",
    "batch(2)",
    "sort",
    "filter(v => v)",
    "map(v => v ~ '!')",
    "slice(0, 2)",
    "first",
    "column('x')",
];

/// Filters for values that keep their size bounded.
const VALUE_FILTERS: &[&str] = &[
    "length",
    "upper",
    "first",
    "last",
    "default('d')",
    "keys",
    "reverse",
    "slice(0, 2)",
    "trim",
    "join(',')",
    "merge([1])",
    "capitalize",
    "e",
    "raw",
    "abs",
    "lower",
];

const APPLY_FILTERS: &[&str] = &[
    "upper",
    "lower",
    "title",
    "capitalize",
    "trim",
    "escape",
    "e('html')",
    "e('js')",
    "striptags",
    "nl2br",
    "raw",
    "spaceless",
    "replace({'1': 'one', a: 'A'})",
    "reverse",
    "length",
    "first",
    "last",
    "default('d')",
    "json_encode",
    "url_encode",
    "slice(1, 3)",
    "split('|')|join('+')",
    "trim('|')",
    "format('x')",
    "lower|upper",
    "spaceless|upper",
];

const TESTS: &[&str] = &[
    "defined",
    "empty",
    "null",
    "iterable",
    "odd",
    "even",
    "same as(1)",
    "same as('0')",
    "divisible by(2)",
    "sequence",
    "mapping",
    "true",
    "none",
    "same as(null)",
];

const COMPARISONS: &[&str] = &[
    "==",
    "!=",
    "<",
    ">=",
    "in",
    "not in",
    "===",
    "<=>",
    "starts with",
    "ends with",
    "matches",
];

/// Definitions of the macro `m` the macro probes call.
const MACROS: &[&str] = &[
    "{% macro m(a, b) %}[{{ _context|keys|join(',') }}]{% for i in a ?? [] %}{{ loop.index }}{{ loop.parent|keys|join('.') }}{% endfor %}{{ varargs|length }}{% endmacro %}",
    "{% macro m(a = 'd', loop = null) %}<{{ _context|keys|join(',') }}|{{ a|json_encode }}|{{ loop|json_encode }}>{% set a = 'm' %}{% endmacro %}",
    "{% macro m(...rest) %}{{ rest|json_encode }}[{{ _context|keys|join(',') }}]{% for k, v in _context %}{{ k }};{% endfor %}{% endmacro %}",
];

/// Syntax errors of the family's tags, inserted at a statement boundary.
const BROKEN: &[&str] = &[
    "{% set a, b = 1 %}",
    "{% set a, b %}x{% endset %}",
    "{% for x in %}{% endfor %}",
    "{% for x in items %}",
    "{% if x %}{% else %}{% else %}{% endif %}",
    "{% if x %}{% elseif %}{% endif %}",
    "{% for true in items %}{% endfor %}",
    "{% set null = 1 %}",
    "{% set None = 1 %}",
    "{% set FALSE = 1 %}",
    "{% with x only y %}{% endwith %}",
    "{% for k, v, w in items %}{% endfor %}",
    "{% endfor %}",
    "{% for x in items if x %}{% endfor %}",
    "{% do %}",
    "{% apply %}{% endapply %}",
    "{% apply upper %}x",
    "{% set x = 1, 2 %}",
    "{% do [a, b.c] = [1, 2] %}",
    "{% do [] = items %}",
    "{% do {} = map %}",
    "{% do 1 = 2 %}",
    "{% do {a: b.c} = map %}",
    "{% for x in items %}{% endif %}",
    "{% if %}{% endif %}",
    "{% with %}",
    "{% elseif x %}",
    "{% set x %}{% set y %}{% endset %}",
    "{% for x in items %}{% else %}{% else %}{% endfor %}",
    "{% set a = %}",
    "{% for in items %}{% endfor %}",
    "{% if x %}{% endfor %}",
    "{% with {a: 1} %}{% endif %}",
    "{% apply upper %}{% endwith %}",
    "{% guard filter %}{% endguard %}",
    "{% guard macro x %}{% endguard %}",
    "{% set x = [a, b] = [1, 2] %}",
    "{% do (a) = 1 %}",
    "{% for x.y in items %}{% endfor %}",
    "{% set a.b = 1 %}",
    "{% for k, in items %}{% endfor %}",
    "{% endset %}",
    "{% for x in items %}{% break %}{% endfor %}",
    "{% for x in items %}{% continue %}{% endfor %}",
    "{% if x %}{% elif y %}{% endif %}",
    "{% foreach x in items %}{% endforeach %}",
    "{% if x %}{% endwith %}",
    "{% apply uper %}x{% endapply %}",
    "{% if items is od %}{% endif %}",
    "{% for x in items|revers %}{% endfor %}",
    "{% set x = lenght(items) %}",
    "{% with {a: 1} %}{% endfor %}",
    "{% for x in items %}{% else x %}{% endfor %}",
    "{% if x %}{% else if y %}{% endif %}",
    "{% for x in items %}{% endfor x %}",
    "{% endif %}",
    "{% set %}",
    "{% for %}",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Theme {
    For,
    /// Loops the way templates usually write them: records, attributes, `loop.first`/`last`,
    /// `cycle()`, totals, `batch`, `filter`, `sort`, nested loops over a record's list.
    Idioms,
    If,
    Set,
    With,
    Apply,
    Mixed,
}

/// A lexical scope of the template being generated: the names it defines (an approximation used
/// to keep strict-mode cases running), `barrier` for `with ... only`.
struct Scope {
    names: Vec<String>,
    barrier: bool,
}

/// An enclosing `for`.
struct LoopCtx {
    key: String,
    value: String,
    /// Iterations (estimated).
    size: usize,
    /// The sequence expression when it is a plain data variable.
    seq_var: Option<&'static str>,
}

struct Gen {
    rng: Rng,
    strict: bool,
    /// Avoids the constructs that usually stop a render early (undefined names, special names as
    /// targets, attribute access on arbitrary values, non-mappings in `with`, syntax errors).
    clean: bool,
    php: bool,
    /// Adds integer keys to the data and the globals (`"7"`, `"5"`).
    numeric_keys: bool,
    data: Vec<&'static DataVar>,
    globals: Vec<(&'static str, &'static str)>,
    scopes: Vec<Scope>,
    loops: Vec<LoopCtx>,
    /// Names assigned so far (`set`, `=`, captures, destructuring).
    written: Vec<String>,
    /// Loop targets used so far.
    targets: Vec<String>,
    /// Reads of written variables still allowed in the expression being generated (`None`: any):
    /// inside loops an assigned value reads earlier assignments at most once, so values grow
    /// linearly with the iterations, never exponentially.
    written_reads: Option<usize>,
    budget: usize,
    depth: usize,
    loops_total: usize,
    blocks: usize,
    dump: bool,
    probe_template: bool,
    /// Calls of the macro `m` (defined at the end of the template when used).
    macro_calls: usize,
    theme: Theme,
}

impl Gen {
    fn new(mut rng: Rng) -> Gen {
        let strict = rng.percent(45);
        let clean = rng.percent(50);
        let php = rng.percent(30);
        let numeric_keys = rng.percent(8);
        let mut data: Vec<&'static DataVar> = Vec::new();
        for var in DATA {
            if (php || var.json.is_some()) && rng.percent(var.weight) {
                data.push(var);
            }
        }
        let mut globals = Vec::new();
        if rng.percent(45) {
            for global in GLOBALS {
                if rng.percent(18) {
                    globals.push(*global);
                }
            }
        }
        let theme = match rng.weighted(&[30, 12, 14, 10, 8, 20, 14]) {
            0 => Theme::For,
            6 => Theme::Idioms,
            1 => Theme::If,
            2 => Theme::Set,
            3 => Theme::With,
            4 => Theme::Apply,
            _ => Theme::Mixed,
        };
        let mut names: Vec<String> = data.iter().map(|d| d.name.to_owned()).collect();
        for (name, _) in &globals {
            if !names.iter().any(|n| n == name) {
                names.push((*name).to_owned());
            }
        }
        Gen {
            rng,
            strict,
            clean,
            php,
            numeric_keys,
            data,
            globals,
            scopes: vec![Scope {
                names,
                barrier: false,
            }],
            loops: Vec::new(),
            written: Vec::new(),
            targets: Vec::new(),
            written_reads: None,
            budget: 0,
            depth: 0,
            loops_total: 0,
            blocks: 0,
            dump: false,
            probe_template: false,
            macro_calls: 0,
            theme,
        }
    }

    fn build(mut self, id: String) -> Json {
        let small = self.rng.percent(40);
        self.budget = if small {
            self.rng.range(1, 3)
        } else {
            self.rng.range(4, 14)
        };
        let mut parts: Vec<String> = Vec::new();
        if self.rng.percent(30) {
            parts.push(self.prelude());
        }
        while self.budget > 0 {
            self.budget -= 1;
            let mut part = self.stmt();
            if self.rng.percent(40) {
                part.push_str(&self.probe());
            }
            part.push_str(self.sep());
            parts.push(part);
        }
        if self.rng.percent(55) {
            parts.push(self.final_probe());
        }
        if self.macro_calls > 0 {
            // macros run with a context of their own: their arguments, `varargs` and the globals
            parts.push(self.rng.pick(MACROS).to_owned());
        }
        if !self.clean && self.rng.percent(8) {
            let at = self.rng.below(parts.len() + 1);
            let broken = self.rng.pick(BROKEN).to_owned();
            parts.insert(at, broken);
        }
        let template = parts.concat();

        let mut case = vec![
            ("id".to_owned(), Json::str(id)),
            ("env".to_owned(), Json::str("plain")),
        ];
        let mut templates = vec![("index.twig".to_owned(), Json::str(template))];
        if self.probe_template {
            templates.push((
                "probe.twig".to_owned(),
                Json::str("<{{ _context|keys|join(',') }}>"),
            ));
        }
        case.push(("templates".to_owned(), Json::Object(templates)));
        if self.php {
            case.push(("data_php".to_owned(), Json::str(self.data_php())));
            case.push(("test_classes".to_owned(), Json::Bool(true)));
        } else {
            case.push(("data_json".to_owned(), self.data_json()));
        }
        let mut config = vec![(
            "autoescape".to_owned(),
            if self.rng.percent(50) {
                Json::Bool(false)
            } else {
                Json::str("html")
            },
        )];
        config.push(("strict_variables".to_owned(), Json::Bool(self.strict)));
        match self.rng.below(100) {
            0..=14 => config.push(("optimizations".to_owned(), Json::int(0))),
            15..=24 => config.push(("optimizations".to_owned(), Json::int(2))),
            _ => {}
        }
        if self.rng.percent(10) {
            config.push(("use_yield".to_owned(), Json::Bool(true)));
        }
        if self.dump {
            config.push(("debug".to_owned(), Json::Bool(self.rng.percent(85))));
        }
        case.push(("config".to_owned(), Json::Object(config)));
        if self.dump {
            case.push((
                "extensions".to_owned(),
                Json::Array(vec![Json::str("debug")]),
            ));
        }
        if !self.globals.is_empty() || self.numeric_keys {
            let mut globals: Vec<(String, Json)> = self
                .globals
                .iter()
                .map(|(name, value)| {
                    (
                        (*name).to_owned(),
                        Json::parse(value).expect("valid global JSON"),
                    )
                })
                .collect();
            if self.numeric_keys {
                globals.push(("5".to_owned(), Json::str("five")));
            }
            case.push(("globals".to_owned(), Json::Object(globals)));
        }
        if self.rng.percent(10) {
            case.push(("errors".to_owned(), Json::str("throw")));
        }
        Json::Object(case)
    }

    fn data_json(&self) -> Json {
        let mut entries: Vec<(String, Json)> = self
            .data
            .iter()
            .filter_map(|d| {
                d.json.map(|text| {
                    (
                        d.name.to_owned(),
                        Json::parse(text).expect("valid data JSON"),
                    )
                })
            })
            .collect();
        if self.numeric_keys {
            entries.insert(entries.len() / 2, ("7".to_owned(), Json::str("seven")));
        }
        Json::Object(entries)
    }

    fn data_php(&self) -> String {
        let mut code = String::new();
        for d in &self.data {
            if let Some(needs) = d.needs {
                code.push_str(needs);
            }
        }
        code.push_str("return [\n");
        for (i, d) in self.data.iter().enumerate() {
            if self.numeric_keys && i == self.data.len() / 2 {
                code.push_str("    '7' => 'seven',\n");
            }
            code.push_str(&format!("    '{}' => {},\n", d.name, d.php));
        }
        code.push(']');
        code
    }

    // ----------------------------------------------------------------------------------------
    // names and scopes

    fn defined(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        for scope in self.scopes.iter().rev() {
            for name in &scope.names {
                if !names.contains(name) {
                    names.push(name.clone());
                }
            }
            if scope.barrier {
                break;
            }
        }
        for (name, _) in &self.globals {
            if !names.iter().any(|n| n == name) {
                names.push((*name).to_owned());
            }
        }
        names
    }

    fn define(&mut self, name: &str) {
        if let Some(scope) = self.scopes.last_mut()
            && !scope.names.iter().any(|n| n == name)
        {
            scope.names.push(name.to_owned());
        }
    }

    fn wrote(&mut self, name: &str) {
        self.define(name);
        if !self.written.iter().any(|n| n == name) {
            self.written.push(name.to_owned());
        }
    }

    fn in_loop(&self) -> bool {
        !self.loops.is_empty()
    }

    fn loop_weight(&self) -> usize {
        self.loops
            .iter()
            .map(|l| l.size.max(1))
            .fold(1usize, usize::saturating_mul)
    }

    fn is_written(&self, name: &str) -> bool {
        self.written.iter().any(|n| n == name)
    }

    /// A variable name to read: usually a defined one (almost always in strict mode).
    fn read_name(&mut self) -> String {
        let mut defined = self.defined();
        if self.written_reads == Some(0) {
            defined.retain(|n| !self.is_written(n));
        }
        let p = match (self.clean, self.strict) {
            (true, true) => 100,
            (true, false) => 88,
            (false, true) => 90,
            (false, false) => 72,
        };
        let name = if !defined.is_empty() && self.rng.percent(p) {
            defined[self.rng.below(defined.len())].clone()
        } else {
            let name = self.rng.pick(MAYBE_UNDEFINED).to_owned();
            if self.written_reads == Some(0) && self.is_written(&name) {
                "undef".to_owned()
            } else {
                name
            }
        };
        if self.is_written(&name)
            && let Some(left) = self.written_reads.as_mut()
        {
            *left = left.saturating_sub(1);
        }
        name
    }

    /// A variable name to assign to: biased towards names that already mean something.
    fn write_name(&mut self) -> String {
        let defined = self.defined();
        match self.rng.below(100) {
            0..=29 if !defined.is_empty() => defined[self.rng.below(defined.len())].clone(),
            30..=44 if !self.data.is_empty() => {
                self.data[self.rng.below(self.data.len())].name.to_owned()
            }
            45..=52 if !self.globals.is_empty() => self.globals[self.rng.below(self.globals.len())]
                .0
                .to_owned(),
            53..=64 if self.in_loop() => {
                let l = &self.loops[self.rng.below(self.loops.len())];
                if self.rng.percent(50) {
                    l.value.clone()
                } else {
                    l.key.clone()
                }
            }
            65..=66 if self.in_loop() && !self.clean => "loop".to_owned(),
            67..=69 if !self.clean => self.rng.pick(SPECIAL).to_owned(),
            _ => self.rng.pick(FRESH).to_owned(),
        }
    }

    /// Whether a data variable is an `Iterator` with a position of its own: an operation that
    /// rewinds it without exhausting it (`first`, `slice(0, n)`, `in`, sequence destructuring,
    /// `rewind()`) inside a loop over it would never end.
    fn shared_position(name: &str) -> bool {
        matches!(name, "ai" | "sit" | "foo" | "ii" | "iic" | "sos")
    }

    /// Whether an operation that may stop iterating early is safe on `operand` here: always
    /// outside loops and without objects in the data, otherwise only on literals (a loop target,
    /// `_seq` or a context value may be the iterator the enclosing loop is traversing).
    fn early_exit_ok(&self, operand: &str) -> bool {
        !(self.php && self.in_loop())
            || operand.starts_with(['[', '{', '\'', '"', '-'])
            || operand.starts_with(|c: char| c.is_ascii_digit())
    }

    // ----------------------------------------------------------------------------------------
    // template text helpers

    fn open(&mut self) -> &'static str {
        if self.rng.percent(6) {
            self.rng.pick(&["{%-", "{%~"])
        } else {
            "{%"
        }
    }

    fn close(&mut self) -> &'static str {
        if self.rng.percent(6) {
            self.rng.pick(&["-%}", "~%}"])
        } else {
            "%}"
        }
    }

    fn tag(&mut self, inner: &str) -> String {
        let open = self.open();
        let close = self.close();
        if self.rng.percent(7) {
            // line breaks inside the tag move the lines errors are reported on
            let broken = inner.replacen(
                ' ',
                "
",
                self.rng.range(1, 2),
            );
            format!(
                "{open} {broken}
{close}"
            )
        } else {
            format!("{open} {inner} {close}")
        }
    }

    fn print(&mut self, expr: &str) -> String {
        match self.rng.below(100) {
            0..=3 => format!("{{{{- {expr} -}}}}"),
            4..=7 => format!(
                "{{{{
{expr}
}}}}"
            ),
            _ => format!("{{{{ {expr} }}}}"),
        }
    }

    fn sep(&mut self) -> &'static str {
        self.rng.pick(&["", "|", "\n", " ", ";", ",\n", "\n\n"])
    }

    // ----------------------------------------------------------------------------------------
    // expressions

    fn scalar(&mut self) -> String {
        self.rng.pick(SCALARS).to_owned()
    }

    fn array(&mut self) -> String {
        self.rng.pick(ARRAYS).to_owned()
    }

    /// A variable reference, maybe with an attribute or through `_context`.
    fn var_ref(&mut self) -> String {
        let name = self.read_name();
        if self.clean {
            return match self.rng.below(100) {
                0..=84 => name,
                85..=89 => format!("_context.{name}"),
                90..=94 => format!("_context['{name}']"),
                _ => format!("({name} ?? 'u')"),
            };
        }
        match self.rng.below(100) {
            0..=63 => name,
            64..=70 => {
                let attr = self
                    .rng
                    .pick(&["a", "x", "p", "0", "1", "length", "b", "q"]);
                format!("{name}.{attr}")
            }
            71..=75 => format!("{name}[0]"),
            76..=79 => format!("{name}['a']"),
            80..=86 => format!("_context.{name}"),
            87..=91 => format!("_context['{name}']"),
            _ => format!("({name} ?? 'u')"),
        }
    }

    fn atom(&mut self) -> String {
        match self.rng.below(100) {
            0..=49 => self.var_ref(),
            50..=79 => self.scalar(),
            80..=91 => self.array(),
            _ if self.in_loop() => self.loop_field(),
            _ => self.scalar(),
        }
    }

    fn loop_field(&mut self) -> String {
        let nested = self.loops.len() >= 2;
        match self.rng.below(100) {
            0..=44 => format!("loop.{}", self.rng.pick(LOOP_FIELDS)),
            45..=51 => "loop.parent|keys|join(',')".to_owned(),
            52..=56 => "loop|keys|join(',')".to_owned(),
            57..=63 if nested => format!(
                "loop.parent.loop.{}",
                self.rng
                    .pick(&["index", "index0", "first", "last", "length"])
            ),
            57..=63 => {
                let name = self.read_name();
                format!("loop.parent.{name}|json_encode")
            }
            64..=67 => format!("loop['{}']", self.rng.pick(LOOP_FIELDS)),
            68..=71 => format!("attribute(loop, '{}')", self.rng.pick(LOOP_FIELDS)),
            72..=75 => format!("loop.{} ?? 'nl'", self.rng.pick(LOOP_FIELDS)),
            76..=79 => format!("loop.{}|default('d')", self.rng.pick(LOOP_FIELDS)),
            80..=83 => "loop.last is same as(true) ? 'L' : 'NL'".to_owned(),
            84..=87 => format!("loop.{} is defined ? 'D' : 'U'", self.rng.pick(LOOP_FIELDS)),
            88..=90 => "_context.loop|json_encode".to_owned(),
            91..=93 => "loop|length".to_owned(),
            94..=95 => "loop.nope".to_owned(),
            96 => "loop.parent._parent is defined ? 'P' : 'NP'".to_owned(),
            97 => "\"i#{loop.index}-#{loop.parent|length}\"".to_owned(),
            _ => self.cycle(),
        }
    }

    /// `cycle()`, the loop helper, over sequences, mappings, objects and odd positions.
    fn cycle(&mut self) -> String {
        let values = if self.rng.percent(60) || self.data.is_empty() {
            self.rng
                .pick(&[
                    "['odd', 'even']",
                    "[1, 2, 3]",
                    "{a: 'x', b: 'y'}",
                    "[]",
                    "'ab'",
                    "1..3",
                ])
                .to_owned()
        } else {
            self.data[self.rng.below(self.data.len())].name.to_owned()
        };
        let position = self.rng.pick(&[
            "loop.index0",
            "loop.index",
            "loop.revindex ?? 0",
            "-1",
            "1.5",
            "null",
            "'1'",
            "_key",
        ]);
        format!("cycle({values}, {position})|json_encode")
    }

    /// The loop's own variables and the `ForNode` privates.
    fn loop_private(&mut self) -> String {
        let l = &self.loops[self.rng.below(self.loops.len())];
        let (key, value) = (l.key.clone(), l.value.clone());
        match self.rng.below(100) {
            0..=24 => key,
            25..=54 => format!("{value}|json_encode"),
            55..=64 => "_key|json_encode".to_owned(),
            65..=72 => "_seq|json_encode".to_owned(),
            73..=80 => "_parent|keys|join(',')".to_owned(),
            81..=86 => "_iterated ?? 'ni'".to_owned(),
            87..=92 => "_seq|length".to_owned(),
            _ => format!("{value} is defined ? 'D' : 'U'"),
        }
    }

    fn value(&mut self, depth: usize) -> String {
        let r = self.rng.below(100);
        if depth >= 2 || r < 30 {
            return self.atom();
        }
        match r {
            30..=39 => {
                let a = self.value(depth + 1);
                let b = self.value(depth + 1);
                format!("{a} ~ {b}")
            }
            40..=45 => {
                let n = self.rng.pick(&["1", "2", "-1", "10"]);
                if self.in_loop() {
                    format!("loop.index0 + {n}")
                } else {
                    format!("{n} + {n}")
                }
            }
            46..=54 => {
                let a = self.atom();
                let mut filter = self.rng.pick(VALUE_FILTERS);
                if (filter == "first" || filter.starts_with("slice")) && !self.early_exit_ok(&a) {
                    filter = "length";
                }
                format!("({a})|{filter}")
            }
            97..=99 => self.arrow(),
            55..=61 => {
                let a = self.var_ref();
                let b = self.atom();
                if a.contains("??") {
                    format!("{a} ~ {b}")
                } else {
                    format!("{a} ?? {b}")
                }
            }
            62..=67 => {
                let c = self.cond_atom();
                let a = self.atom();
                let b = self.atom();
                format!("{c} ? {a} : {b}")
            }
            68..=72 => {
                let a = self.value(depth + 1);
                let b = self.value(depth + 1);
                format!("[{a}, {b}]")
            }
            73..=77 => {
                let key = self.hash_key();
                let v = self.value(depth + 1);
                format!("{{{key}: {v}}}")
            }
            78..=82 => {
                let name = self.write_name();
                let v = if self.in_loop() {
                    self.rhs()
                } else {
                    self.value(depth + 1)
                };
                self.wrote(&name);
                format!("({name} = {v})")
            }
            83..=88 if self.in_loop() => self.loop_field(),
            83..=88 => self.atom(),
            89..=92 => {
                let a = self.array();
                let b = self.rng.pick(&["[1]", "{z: 1}", "['b']", "[]"]);
                format!("{a}|merge({b})")
            }
            93..=96 => {
                let a = self.atom();
                let b = self.atom();
                format!("{a} ?: {b}")
            }
            _ => self.atom(),
        }
    }

    /// An arrow function over a sequence, reading the loop and its targets when inside one.
    fn arrow(&mut self) -> String {
        let seq = if self.data.is_empty() || self.rng.percent(40) {
            self.rng
                .pick(&["[1, 2, 3]", "{a: 'x', b: 'y'}", "[]"])
                .to_owned()
        } else {
            let arrays: Vec<&'static DataVar> = self
                .data
                .iter()
                .copied()
                .filter(|d| matches!(d.kind, Kind::List | Kind::Map | Kind::Empty))
                .collect();
            if arrays.is_empty() {
                "[1, 2]".to_owned()
            } else {
                arrays[self.rng.below(arrays.len())].name.to_owned()
            }
        };
        let outer = if self.in_loop() {
            let l = &self.loops[self.rng.below(self.loops.len())];
            let target = l.value.clone();
            self.rng
                .pick(&["loop.index", "loop.first", "_key", "loop|length", "T"])
                .replace('T', &target)
        } else {
            let name = self.read_name();
            format!("{name}|json_encode")
        };
        let param = self.rng.pick(&["e", "v", "loop", "x", "_key"]);
        match self.rng.below(5) {
            0 => format!("({seq})|map({param} => {param}|json_encode ~ ({outer}))|join(',')"),
            1 => format!("({seq})|filter(({param}, kk) => kk is same as({outer}))|keys|join(',')"),
            2 => format!("({seq})|reduce((c, {param}) => c ~ ({outer}), '')"),
            3 => format!("({seq})|map({param} => _context|keys|join('.'))|join('|')"),
            _ => format!("({seq})|find({param} => {param} == ({outer}))|json_encode"),
        }
    }

    /// A value to assign; inside loops it reads earlier assignments at most once.
    fn rhs(&mut self) -> String {
        if !self.in_loop() {
            return self.value(0);
        }
        let saved = self.written_reads;
        self.written_reads = Some(saved.map_or(1, |n| n.min(1)));
        let v = match self.rng.below(100) {
            0..=24 if !self.written.is_empty() => {
                // accumulate into an earlier assignment
                let name = self.read_name();
                let item = {
                    let reads = self.written_reads;
                    self.written_reads = Some(0);
                    let item = if self.rng.percent(60) {
                        let l = &self.loops[self.rng.below(self.loops.len())];
                        l.value.clone()
                    } else {
                        self.atom()
                    };
                    self.written_reads = reads;
                    item
                };
                match self.rng.below(4) {
                    0 => format!("{name} ~ {item}"),
                    1 => format!("({name} ?? [])|merge([{item}])"),
                    2 => format!("{name}|default('') ~ loop.index"),
                    _ => format!("({name} ?? 0) + 1"),
                }
            }
            25..=44 => self.loop_field(),
            45..=59 => self.loop_private(),
            _ => self.value(0),
        };
        self.written_reads = saved;
        // values that embed the context (`loop.parent`, `_parent`, `_context`, JSON of them)
        // would contain the variable being assigned: each iteration would nest or escape it
        // again
        if Self::embeds_context(&v) {
            return self
                .rng
                .pick(&["loop.index", "loop.first", "loop.revindex ?? 0", "_key"])
                .to_owned();
        }
        v
    }

    fn embeds_context(expr: &str) -> bool {
        expr == "loop"
            || expr == "_seq"
            || [
                "json_encode",
                "_context",
                "_parent",
                "loop.parent",
                "loop|",
                "loop[",
            ]
            .iter()
            .any(|needle| expr.contains(needle))
    }

    fn cond_atom(&mut self) -> String {
        match self.rng.below(100) {
            0..=59 => self.var_ref(),
            60..=84 => self.scalar(),
            _ => self.array(),
        }
    }

    fn cond(&mut self) -> String {
        match self.rng.below(100) {
            0..=24 => self.cond_atom(),
            25..=33 => {
                let a = self.cond_atom();
                format!("not {a}")
            }
            34..=48 => {
                let a = self.cond_atom();
                let not = if self.rng.percent(30) { "not " } else { "" };
                let mut test = self.rng.pick(TESTS);
                if test == "defined" && a.contains("??") {
                    test = "null";
                }
                format!("{a} is {not}{test}")
            }
            49..=58 => {
                let a = self.cond_atom();
                let mut op = self.rng.pick(COMPARISONS);
                let b = if op == "matches" {
                    "'/^a/'".to_owned()
                } else {
                    self.cond_atom()
                };
                // `in` stops iterating at the first match: inside a loop over the same iterator
                // it would rewind it forever
                if op.ends_with("in") && !self.early_exit_ok(&b) {
                    op = "==";
                }
                format!("{a} {op} {b}")
            }
            59..=67 => {
                let a = self.cond_atom();
                let b = self.cond_atom();
                let op = self.rng.pick(&["and", "or", "and not", "or not"]);
                format!("{a} {op} {b}")
            }
            68..=72 => {
                let name = self.write_name();
                let v = self.atom();
                self.wrote(&name);
                format!("({name} = {v})")
            }
            73..=79 if self.in_loop() => {
                let f = self.rng.pick(&[
                    "loop.first",
                    "loop.last",
                    "loop.index is odd",
                    "loop.index > 1",
                    "loop.revindex0",
                    "loop.length is defined",
                    "not loop.first",
                ]);
                f.to_owned()
            }
            80..=85 => {
                let a = self.var_ref();
                let b = self.rng.pick(&["false", "true", "0", "'0'", "[]"]);
                if a.contains("??") {
                    format!("{a} == {b}")
                } else {
                    format!("{a} ?? {b}")
                }
            }
            86..=91 => {
                let a = self.var_ref();
                let mut op =
                    self.rng
                        .pick(&["|length", "|length > 1", "|first", "|default(false)"]);
                if op == "|first" && !self.early_exit_ok(&a) {
                    op = "|length";
                }
                format!("{a}{op}")
            }
            _ => {
                let name = self.read_name();
                format!("{name} is defined and {name}")
            }
        }
    }

    /// A key of a hash literal (sometimes numeric, quoted or dynamic).
    fn hash_key(&mut self) -> String {
        match self.rng.below(100) {
            0..=59 => self.write_name(),
            60..=69 => self.rng.pick(&["'1'", "2", "'02'", "'-3'", "0"]).to_owned(),
            70..=79 => self.rng.pick(&["'k k'", "'a'", "\"b\""]).to_owned(),
            80..=89 => {
                let v = self.scalar();
                format!("({v})")
            }
            _ => self.rng.pick(SPECIAL).to_owned(),
        }
    }

    /// A sequence for `for`, with its estimated number of iterations.
    fn seq(&mut self) -> (String, usize, Option<&'static str>) {
        let nested = self.in_loop();
        let heavy = self.loop_weight() >= 6;
        // reading assigned arrays as nested sequences can grow them geometrically
        let saved = self.written_reads;
        if nested {
            self.written_reads = Some(0);
        }
        let result = self.seq_inner(nested, heavy);
        self.written_reads = saved;
        result
    }

    fn seq_inner(&mut self, nested: bool, heavy: bool) -> (String, usize, Option<&'static str>) {
        let r = self.rng.below(100);
        match r {
            0..=37 if !self.data.is_empty() => {
                let iterable: Vec<&'static DataVar> = self
                    .data
                    .iter()
                    .copied()
                    .filter(|d| d.size > 0 && (!heavy || d.size <= 3))
                    .collect();
                let pool = if !iterable.is_empty() && self.rng.percent(80) {
                    iterable
                } else {
                    self.data.clone()
                };
                let d = pool[self.rng.below(pool.len())];
                (d.name.to_owned(), d.size, Some(d.name))
            }
            38..=51 => {
                let a = self.array();
                (a, 3, None)
            }
            52..=58 => {
                let (range, size) = self.rng.pick(RANGES);
                (range.to_owned(), size, None)
            }
            59..=67 if !self.data.is_empty() => {
                let d = self.data[self.rng.below(self.data.len())];
                let mut filter = self.rng.pick(SEQ_FILTERS);
                if (filter == "first" || filter.starts_with("slice")) && !self.early_exit_ok(d.name)
                {
                    filter = "reverse";
                }
                (format!("{}|{filter}", d.name), d.size.min(3), None)
            }
            68..=72 => {
                let s = self
                    .rng
                    .pick(&["_context", "_context|keys", "_context|slice(0, 2)"]);
                (s.to_owned(), if heavy { 2 } else { 8 }, None)
            }
            73..=81 if nested => {
                let l = &self.loops[self.rng.below(self.loops.len())];
                let (value, outer) = (l.value.clone(), l.seq_var);
                match self.rng.below(6) {
                    0 => ("loop".to_owned(), 8, None),
                    1 => ("loop.parent|keys".to_owned(), 8, None),
                    2 => ("_seq".to_owned(), 3, None),
                    3 if outer.is_some() => {
                        let o = outer.unwrap_or_default();
                        (o.to_owned(), 3, outer)
                    }
                    4 => ("_parent|keys".to_owned(), 8, None),
                    _ => (value, 3, None),
                }
            }
            82..=88 => {
                let s = self.scalar();
                (s, 0, None)
            }
            89..=93 => {
                let name = self.write_name();
                let a = self.rng.pick(&["[1, 2]", "{a: 'x'}", "[]", "'s'"]);
                self.wrote(&name);
                (format!("({name} = {a})"), 2, None)
            }
            94..=96 if !nested && !self.written.is_empty() => {
                let name = self.read_name();
                (name, 3, None)
            }
            _ => {
                let a = self.var_ref();
                (a, 2, None)
            }
        }
    }

    /// A mapping for `with`, and the keys it defines when they are known.
    fn mapping(&mut self) -> (String, Vec<String>) {
        let r = self.rng.below(100);
        let r = if self.clean && ((74..=78).contains(&r) || r >= 91) {
            r % 45
        } else {
            r
        };
        match r {
            0..=44 => {
                let n = self.rng.range(0, 3);
                let mut keys = Vec::new();
                let mut entries = Vec::new();
                for _ in 0..n {
                    if self.rng.percent(10) {
                        // shorthand `{a}` means `{a: a}`
                        let name = self.read_name();
                        if name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') {
                            keys.push(name.clone());
                            entries.push(name);
                            continue;
                        }
                    }
                    let key = self.hash_key();
                    let value = self.value(1);
                    if key.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') {
                        keys.push(key.clone());
                    }
                    entries.push(format!("{key}: {value}"));
                }
                (format!("{{{}}}", entries.join(", ")), keys)
            }
            45..=56 if !self.data.is_empty() => {
                let pool: Vec<&'static DataVar> = if self.clean {
                    self.data
                        .iter()
                        .copied()
                        .filter(|d| d.kind != Kind::Scalar && d.kind != Kind::Opaque)
                        .collect()
                } else {
                    self.data.clone()
                };
                match pool.get(self.rng.below(pool.len().max(1))) {
                    Some(d) => (d.name.to_owned(), Vec::new()),
                    None => ("{}".to_owned(), Vec::new()),
                }
            }
            57..=63 => ("_context".to_owned(), Vec::new()),
            64..=68 if self.in_loop() => {
                let m = self.rng.pick(&["loop", "loop.parent", "_parent"]);
                (m.to_owned(), Vec::new())
            }
            69..=73 => {
                let m = self.rng.pick(&["[1, 2]", "['a', 'b']", "[]", "{}"]);
                (m.to_owned(), Vec::new())
            }
            74..=78 => {
                let m = self.rng.pick(&["'str'", "null", "1", "true", "0.5"]);
                (m.to_owned(), Vec::new())
            }
            79..=84 => {
                let key = self.scalar();
                let value = self.atom();
                (format!("{{({key}): {value}}}"), Vec::new())
            }
            85..=90 => {
                let m = self.rng.pick(&[
                    "{a: 1}|merge(map ?? {})",
                    "(map ?? {})|merge({x: 9})",
                    "{'1': 'one', 2: 'two', '02': 'x'}",
                    "items ?? []",
                ]);
                (m.to_owned(), Vec::new())
            }
            _ => {
                let a = self.var_ref();
                (a, Vec::new())
            }
        }
    }

    // ----------------------------------------------------------------------------------------
    // statements

    fn body(&mut self, min: usize, max: usize) -> String {
        let n = self.rng.range(min, max);
        let mut out = String::new();
        for i in 0..n {
            if self.budget == 0 && i >= min {
                break;
            }
            self.budget = self.budget.saturating_sub(1);
            out.push_str(&self.stmt());
            out.push_str(self.sep());
        }
        out
    }

    fn stmt(&mut self) -> String {
        let can_nest = self.depth < 3;
        let can_loop = can_nest && self.loops.len() < 3 && self.loops_total < 5;
        // print, probe, set, multi, capture, assign, destructure, if, for, with, apply, do, text,
        // include, block, guard, accumulate, shared iterator, filtered iterator, deprecated
        let mut w: [usize; 23] = [
            16, 7, 10, 3, 5, 4, 3, 9, 12, 6, 4, 3, 3, 1, 1, 1, 3, 2, 2, 1, 2, 3, 2,
        ];
        if self.in_loop() {
            // includes and blocks inside a loop make the optimizer create `loop`
            w[13] += 2;
            w[14] += 2;
        }
        match self.theme {
            Theme::For => {
                w[8] += 16;
                w[16] += 4;
                w[17] += 3;
                w[18] += 2;
            }
            Theme::If => w[7] += 18,
            Theme::Set => {
                w[2] += 8;
                w[3] += 5;
                w[4] += 6;
                w[5] += 5;
                w[6] += 6;
            }
            Theme::With => w[9] += 14,
            Theme::Apply => {
                w[10] += 12;
                w[4] += 4;
            }
            Theme::Mixed => {}
            Theme::Idioms => {
                w[21] += 30;
                w[8] += 4;
            }
        }
        if !can_nest {
            for i in [4, 7, 9, 10, 15, 16, 17] {
                w[i] = 0;
            }
        }
        if !can_loop {
            w[8] = 0;
            w[16] = 0;
            w[17] = 0;
            w[18] = 0;
            w[21] = 0;
        }
        if !self.php {
            w[17] = 0;
            w[18] = 0;
        }
        if self.clean || !self.in_loop() {
            w[20] = 0;
        }
        match self.rng.weighted(&w) {
            0 => self.print_stmt(),
            1 => self.probe(),
            2 => self.set_stmt(),
            3 => self.multi_set(),
            4 => self.capture(),
            5 => self.assign_stmt(),
            6 => self.destructure(),
            7 => self.if_stmt(),
            8 => self.for_stmt(),
            9 => self.with_stmt(),
            10 => self.apply_stmt(),
            11 => self.do_stmt(),
            12 => self.text(),
            13 => self.include_probe(),
            14 => self.block_probe(),
            15 => self.guard(),
            16 => self.accumulate(),
            17 => self.shared_iteration(),
            18 => self.filtered_iteration(),
            20 => self.override_private(),
            21 => self.idiom(),
            22 => self.macro_call(),
            _ => {
                let message = self.rng.pick(&["'d'", "'loop ' ~ (loop.index ?? 0)"]);
                self.tag(&format!("deprecated {message}"))
            }
        }
    }

    fn text(&mut self) -> String {
        self.rng
            .pick(&["x", " ", "<b>", "&", "\n", "a|b", "%", "'q'"])
            .to_owned()
    }

    fn print_stmt(&mut self) -> String {
        let bounded = self.written_reads.is_some();
        let e = match self.rng.below(100) {
            0..=29 if self.in_loop() => self.loop_field(),
            30..=44 if self.in_loop() => self.loop_private(),
            _ => match self.rng.below(100) {
                0..=59 => {
                    let v = self.var_ref();
                    self.printable(&v, bounded)
                }
                _ => self.value(0),
            },
        };
        // a capture body inside a loop is assigned on every iteration (see `rhs`)
        if bounded && Self::embeds_context(&e) {
            return self.print("loop.index ?? 0");
        }
        self.print(&e)
    }

    /// Wraps a reference so that arrays and objects print something meaningful.
    fn printable(&mut self, v: &str, bounded: bool) -> String {
        let simple = !v.contains("??");
        match self.rng.below(100) {
            0..=34 if !bounded => format!("{v}|json_encode"),
            0..=34 => format!("{v}|length"),
            35..=49 if !self.clean => v.to_owned(),
            35..=49 => format!("{v}|json_encode"),
            50..=59 if simple => format!("{v} ?? 'u'"),
            60..=71 if simple => format!("{v} is defined ? 'D' : 'U'"),
            72..=80 => format!("{v}|default('d')"),
            81..=89 => format!("{v} is iterable ? {v}|length : {v}"),
            90..=94 => format!("{v} is null ? 'N' : 'V'"),
            _ => format!("{v}|length"),
        }
    }

    fn probe(&mut self) -> String {
        let unbounded = self.written_reads.is_none() && self.loops.len() <= 1;
        match self.rng.below(100) {
            0..=57 => "[{{ _context|keys|join(',') }}]".to_owned(),
            58..=64 => "{{ _context|length }}".to_owned(),
            65..=74 if unbounded => {
                "[{% for k, v in _context %}{{ k }}={{ v|json_encode }};{% endfor %}]".to_owned()
            }
            75..=81 if unbounded && !self.php => {
                self.dump = true;
                "{{ dump() }}".to_owned()
            }
            82..=90 => {
                self.dump = true;
                "{{ dump(_context|keys) }}".to_owned()
            }
            91..=95 if unbounded => "[{{ _context|json_encode }}]".to_owned(),
            _ => "[{{ _context|keys|json_encode }}]".to_owned(),
        }
    }

    fn final_probe(&mut self) -> String {
        let mut out = String::from("[{{ _context|keys|join(',') }}]");
        let mut names: Vec<String> = self.written.clone();
        for t in &self.targets {
            if !names.contains(t) {
                names.push(t.clone());
            }
        }
        let take = self.rng.range(1, 4);
        for name in names.iter().take(take) {
            out.push_str(&format!(
                "{{{{ {name} is defined ? {name}|json_encode : 'U' }}}}"
            ));
        }
        out
    }

    fn prelude(&mut self) -> String {
        let n = self.rng.range(1, 3);
        let mut out = String::new();
        for _ in 0..n {
            let name = self
                .rng
                .pick(&["a", "x", "v", "k", "acc", "loop", "item", "_key"]);
            let value = self.scalar();
            self.wrote(name);
            out.push_str(&self.tag(&format!("set {name} = {value}")));
        }
        out
    }

    fn set_stmt(&mut self) -> String {
        let name = self.write_name();
        let value = self.rhs();
        self.wrote(&name);
        self.tag(&format!("set {name} = {value}"))
    }

    fn multi_set(&mut self) -> String {
        let n = self.rng.range(2, 3);
        let names: Vec<String> = (0..n).map(|_| self.write_name()).collect();
        let values: Vec<String> = if n == 2 && self.rng.percent(30) {
            vec![names[1].clone(), names[0].clone()]
        } else {
            (0..n).map(|_| self.rhs()).collect()
        };
        for name in &names {
            self.wrote(name);
        }
        self.tag(&format!("set {} = {}", names.join(", "), values.join(", ")))
    }

    fn capture(&mut self) -> String {
        let name = self.write_name();
        let saved = self.written_reads;
        if self.in_loop() {
            self.written_reads = Some(saved.map_or(1, |n| n.min(1)));
        }
        let body = match self.rng.below(100) {
            0..=9 => String::new(),
            10..=19 => self.rng.pick(&["x", " ", "<b>", "0"]).to_owned(),
            20..=24 => self
                .rng
                .pick(&["{{ 'lit' }}", "{{ 1 }}", "{{ '' }}"])
                .to_owned(),
            _ => {
                self.depth += 1;
                let b = self.body(1, 3);
                self.depth -= 1;
                b
            }
        };
        self.written_reads = saved;
        self.wrote(&name);
        let open = self.tag(&format!("set {name}"));
        let close = self.tag("endset");
        format!("{open}{body}{close}")
    }

    fn assign_stmt(&mut self) -> String {
        let name = self.write_name();
        let value = self.rhs();
        self.wrote(&name);
        match self.rng.below(5) {
            0 => self.tag(&format!("do {name} = {value}")),
            1 => self.print(&format!("{name} = {value}")),
            2 => {
                let other = self.write_name();
                self.wrote(&other);
                self.tag(&format!("do {name} = {other} = {value}"))
            }
            3 => {
                let p = self.print(&format!("{name}|json_encode"));
                format!("{}{p}", self.tag(&format!("do {name} = {value}")))
            }
            _ => {
                let body = self.print("'T'");
                let open = self.tag(&format!("if ({name} = {value})"));
                let close = self.tag("endif");
                format!("{open}{body}{close}")
            }
        }
    }

    fn destructure(&mut self) -> String {
        let sequence = self.rng.percent(65);
        let pattern = if sequence {
            let n = self.rng.range(1, 3);
            let mut slots = Vec::new();
            for _ in 0..n {
                if self.rng.percent(12) {
                    slots.push(String::new());
                } else {
                    let name = self.write_name();
                    self.wrote(&name);
                    slots.push(name);
                }
            }
            if slots.iter().all(String::is_empty) {
                let name = self.write_name();
                self.wrote(&name);
                slots[0] = name;
            }
            format!("[{}]", slots.join(", "))
        } else {
            let n = self.rng.range(1, 3);
            let mut entries = Vec::new();
            for _ in 0..n {
                let attr = self.rng.pick(&[
                    "p", "q", "x", "y", "a", "c", "name", "0", "bar", "position", "yielded", "id",
                    "getFoo", "array",
                ]);
                if attr == "0" || self.rng.percent(40) {
                    let name = self.write_name();
                    self.wrote(&name);
                    entries.push(format!("{attr}: {name}"));
                } else {
                    self.wrote(attr);
                    entries.push(attr.to_owned());
                }
            }
            format!("{{{}}}", entries.join(", "))
        };
        let r = self.rng.below(100);
        let r = if self.clean && r >= 65 { r - 65 } else { r };
        let source = match r {
            0..=39 if !self.data.is_empty() => {
                let pool: Vec<&'static DataVar> = if self.clean {
                    self.data
                        .iter()
                        .copied()
                        .filter(|d| d.kind != Kind::Scalar && d.kind != Kind::Opaque)
                        .collect()
                } else {
                    self.data.clone()
                };
                match pool.get(self.rng.below(pool.len().max(1))) {
                    Some(d) => d.name.to_owned(),
                    None => self.array(),
                }
            }
            40..=64 => self.array(),
            65..=74 => self.scalar(),
            75..=84 if self.in_loop() => {
                let l = &self.loops[self.rng.below(self.loops.len())];
                l.value.clone()
            }
            _ => self.atom(),
        };
        // sequence destructuring stops iterating early (see `filtered_iteration`)
        let source = if sequence && !self.early_exit_ok(&source) {
            self.array()
        } else {
            source
        };
        match self.rng.below(4) {
            0 => self.print(&format!("({pattern} = {source})|json_encode")),
            1 => {
                let name = self.write_name();
                self.wrote(&name);
                self.tag(&format!("set {name} = ({pattern} = {source})"))
            }
            _ => self.tag(&format!("do {pattern} = {source}")),
        }
    }

    fn if_stmt(&mut self) -> String {
        let cond = self.cond();
        let mut out = self.tag(&format!("if {cond}"));
        self.depth += 1;
        self.scopes.push(Scope {
            names: Vec::new(),
            barrier: false,
        });
        out.push_str(&self.body(0, 3));
        self.scopes.pop();
        let elseifs = self.rng.weighted(&[60, 25, 15]);
        for _ in 0..elseifs {
            let cond = self.cond();
            out.push_str(&self.tag(&format!("elseif {cond}")));
            self.scopes.push(Scope {
                names: Vec::new(),
                barrier: false,
            });
            out.push_str(&self.body(0, 2));
            self.scopes.pop();
        }
        if self.rng.percent(45) {
            out.push_str(&self.tag("else"));
            self.scopes.push(Scope {
                names: Vec::new(),
                barrier: false,
            });
            out.push_str(&self.body(0, 2));
            self.scopes.pop();
        }
        self.depth -= 1;
        out.push_str(&self.tag("endif"));
        out
    }

    fn value_target(&mut self) -> String {
        let defined = self.defined();
        let r = self.rng.below(100);
        let r = if self.clean && r >= 81 { r - 81 } else { r };
        match r {
            0..=44 => self.rng.pick(&["v", "x", "item", "value", "i"]).to_owned(),
            45..=56 if self.in_loop() => {
                let l = &self.loops[self.rng.below(self.loops.len())];
                if self.rng.percent(60) {
                    l.value.clone()
                } else {
                    l.key.clone()
                }
            }
            57..=68 if !defined.is_empty() => defined[self.rng.below(defined.len())].clone(),
            69..=74 if !self.globals.is_empty() => self.globals[self.rng.below(self.globals.len())]
                .0
                .to_owned(),
            75..=80 => "k".to_owned(),
            81..=86 => "loop".to_owned(),
            87..=94 => self
                .rng
                .pick(&["_key", "_seq", "_parent", "_iterated"])
                .to_owned(),
            95..=99 => self.rng.pick(&["_context", "_self", "_charset"]).to_owned(),
            _ => "v".to_owned(),
        }
    }

    fn key_target(&mut self, value: &str) -> String {
        let r = self.rng.below(100);
        let r = if self.clean && (58..=74).contains(&r) {
            r - 58
        } else {
            r
        };
        match r {
            0..=49 => self.rng.pick(&["k", "key", "i", "kk"]).to_owned(),
            50..=57 => value.to_owned(),
            58..=63 => "_key".to_owned(),
            64..=69 => "loop".to_owned(),
            70..=74 => self.rng.pick(&["_parent", "_seq", "_iterated"]).to_owned(),
            75..=86 if self.in_loop() => {
                let l = &self.loops[self.rng.below(self.loops.len())];
                l.value.clone()
            }
            _ => {
                let defined = self.defined();
                if defined.is_empty() {
                    "k".to_owned()
                } else {
                    defined[self.rng.below(defined.len())].clone()
                }
            }
        }
    }

    fn for_stmt(&mut self) -> String {
        let value = self.value_target();
        let key = if self.rng.percent(45) {
            Some(self.key_target(&value))
        } else {
            None
        };
        let (seq, size, seq_var) = self.seq();
        let targets = match &key {
            Some(k) => format!("{k}, {value}"),
            None => value.clone(),
        };
        let key_name = key.clone().unwrap_or_else(|| "_key".to_owned());
        for t in [&value, &key_name] {
            if !self.targets.contains(t) {
                self.targets.push(t.clone());
            }
        }
        self.loops_total += 1;
        self.depth += 1;
        self.loops.push(LoopCtx {
            key: key_name.clone(),
            value: value.clone(),
            size,
            seq_var,
        });
        self.scopes.push(Scope {
            names: vec![
                value.clone(),
                key_name,
                "loop".to_owned(),
                "_parent".to_owned(),
                "_seq".to_owned(),
            ],
            barrier: false,
        });
        let head = self.tag(&format!("for {targets} in {seq}"));
        let body = self.body(1, 4);
        let else_part = if self.rng.percent(25) {
            let tag = self.tag("else");
            let b = self.body(1, 2);
            format!("{tag}{b}")
        } else {
            String::new()
        };
        self.scopes.pop();
        self.loops.pop();
        self.depth -= 1;
        let end = self.tag("endfor");
        format!("{head}{body}{else_part}{end}")
    }

    fn with_stmt(&mut self) -> String {
        let (head, keys, only) = if self.rng.percent(15) {
            ("with".to_owned(), Vec::new(), false)
        } else {
            let (expr, keys) = self.mapping();
            let only = self.rng.percent(35);
            let head = if only {
                format!("with {expr} only")
            } else {
                format!("with {expr}")
            };
            (head, keys, only)
        };
        self.scopes.push(Scope {
            names: keys,
            barrier: only,
        });
        self.depth += 1;
        let open = self.tag(&head);
        let body = self.body(1, 4);
        self.depth -= 1;
        self.scopes.pop();
        let close = self.tag("endwith");
        format!("{open}{body}{close}")
    }

    fn apply_stmt(&mut self) -> String {
        let n = self.rng.range(1, 2);
        let filters: Vec<&str> = (0..n).map(|_| self.rng.pick(APPLY_FILTERS)).collect();
        let saved = self.written_reads;
        if self.in_loop() {
            self.written_reads = Some(saved.map_or(1, |n| n.min(1)));
        }
        self.depth += 1;
        let open = self.tag(&format!("apply {}", filters.join("|")));
        let body = self.body(1, 3);
        self.depth -= 1;
        self.written_reads = saved;
        let close = self.tag("endapply");
        format!("{open}{body}{close}")
    }

    fn do_stmt(&mut self) -> String {
        let expr = match self.rng.below(100) {
            0..=29 if self.php => {
                // rewind() inside a loop over the same iterator would never end
                let call = if self.in_loop() {
                    self.rng.pick(&[
                        "ai.next()",
                        "foo.next()",
                        "sit.next()",
                        "ii.next()",
                        "sos.next()",
                        "foo.bar(1)",
                        "gen.getIterator()",
                        "ai.key()",
                        "sit.current()",
                        "attribute(ai, 'next')",
                        "attribute(foo, 'next')",
                    ])
                } else {
                    self.rng.pick(&[
                        "ai.next()",
                        "ai.rewind()",
                        "foo.next()",
                        "foo.rewind()",
                        "sit.next()",
                        "ii.next()",
                        "iic.rewind()",
                        "sos.next()",
                        "foo.bar(1)",
                        "gen.getIterator()",
                    ])
                };
                call.to_owned()
            }
            30..=59 => self.value(0),
            60..=79 => {
                let name = self.write_name();
                let value = self.rhs();
                self.wrote(&name);
                format!("{name} = {value}")
            }
            _ => self.cond(),
        };
        self.tag(&format!("do {expr}"))
    }

    fn include_probe(&mut self) -> String {
        self.probe_template = true;
        self.rng
            .pick(&[
                "{% include 'probe.twig' %}",
                "{% include 'probe.twig' only %}",
                "{% include 'probe.twig' with {z: 1} %}",
                "{{ include('probe.twig') }}",
                "{{ include('probe.twig', with_context = false) }}",
                "{% include 'probe.twig' with {loop: 'L'} only %}",
            ])
            .to_owned()
    }

    fn block_probe(&mut self) -> String {
        self.blocks += 1;
        let n = self.blocks;
        let body = if self.rng.percent(60) {
            "<{{ _context|keys|join(',') }}>".to_owned()
        } else {
            self.depth += 1;
            let b = self.body(1, 2);
            self.depth -= 1;
            b
        };
        format!("{{% block b{n} %}}{body}{{% endblock %}}")
    }

    /// `guard`: the branch for a callable that does not exist is parsed with unknown callables
    /// allowed and the parser state (blocks, macros, ...) restored, then dropped.
    fn guard(&mut self) -> String {
        let (head, exists) = self.rng.pick(&[
            ("guard filter upper", true),
            ("guard filter nope", false),
            ("guard function range", true),
            ("guard function nope", false),
            ("guard test odd", true),
            ("guard test divisible by", true),
            ("guard test nope", false),
            ("guard test same", false),
        ]);
        self.depth += 1;
        self.scopes.push(Scope {
            names: Vec::new(),
            barrier: false,
        });
        let open = self.tag(head);
        let mut body = self.body(1, 2);
        let has_else = self.rng.percent(60);
        let mut else_body = if has_else {
            self.body(1, 2)
        } else {
            String::new()
        };
        if self.rng.percent(60) {
            let dead = self.rng.pick(&[
                "{{ nope() }}",
                "{{ items|nope }}",
                "{% if 1 is nope %}{% endif %}",
                "{% apply nope %}x{% endapply %}",
                "{{ nope(1)|nope2 }}",
            ]);
            if exists {
                if has_else {
                    else_body.push_str(dead);
                }
            } else {
                body.push_str(dead);
            }
        }
        if self.rng.percent(25) {
            // a block defined in a dead branch is dropped with the restored parser state
            self.blocks += 1;
            let n = self.blocks;
            let block = format!("{{% block b{n} %}}B{n}{{% endblock %}}");
            if self.rng.percent(50) {
                body.push_str(&block);
            } else if has_else {
                else_body.push_str(&block);
            }
            body.push_str(&format!("{{{{ block('b{n}') is defined ? 'D' : 'U' }}}}"));
        }
        let else_part = if has_else {
            let tag = self.tag("else");
            format!("{tag}{else_body}")
        } else {
            String::new()
        };
        self.scopes.pop();
        self.depth -= 1;
        let close = self.tag("endguard");
        format!("{open}{body}{else_part}{close}")
    }

    /// The accumulator idiom: a variable assigned before a loop and extended in its body.
    fn accumulate(&mut self) -> String {
        let acc = self.rng.pick(&["acc", "out", "s", "items", "a"]).to_owned();
        let init = self.rng.pick(&["''", "[]", "0", "null", ""]);
        let mut out = String::new();
        if !init.is_empty() {
            out.push_str(&self.tag(&format!("set {acc} = {init}")));
            self.wrote(&acc);
        }
        let value = self.value_target();
        let (seq, size, seq_var) = {
            let saved = self.written_reads;
            self.written_reads = Some(0);
            let s = self.seq();
            self.written_reads = saved;
            s
        };
        let step = match init {
            "[]" => format!("{acc}|merge([{value}])"),
            "0" => format!("{acc} + loop.index"),
            _ => format!("{acc} ~ {value}"),
        };
        self.loops_total += 1;
        self.depth += 1;
        self.loops.push(LoopCtx {
            key: "_key".to_owned(),
            value: value.clone(),
            size,
            seq_var,
        });
        self.scopes.push(Scope {
            names: vec![value.clone(), "loop".to_owned(), "_key".to_owned()],
            barrier: false,
        });
        let head = self.tag(&format!("for {value} in {seq}"));
        let set = self.tag(&format!("set {acc} = {step}"));
        let local = if self.rng.percent(40) {
            let name = self.rng.pick(&["tmp", "inner", "y"]);
            self.wrote(name);
            self.tag(&format!("set {name} = loop.index"))
        } else {
            String::new()
        };
        let extra = if self.rng.percent(30) {
            self.body(1, 1)
        } else {
            String::new()
        };
        self.scopes.pop();
        self.loops.pop();
        self.depth -= 1;
        if !self.written.contains(&acc) {
            self.written.push(acc.clone());
        }
        let end = self.tag("endfor");
        out.push_str(&format!("{head}{set}{local}{extra}{end}"));
        out.push_str(&format!(
            "{{{{ {acc} is defined ? {acc}|json_encode : 'U' }}}}[{{{{ _context|keys|join(',') }}}}]"
        ));
        out
    }

    /// Nested loops over the same iterator object (shared position for `Iterator`s, a fresh
    /// iterator for `IteratorAggregate`s and arrays).
    fn shared_iteration(&mut self) -> String {
        let objects: Vec<&'static DataVar> = self
            .data
            .iter()
            .copied()
            .filter(|d| matches!(d.kind, Kind::Iter | Kind::IterCount))
            .collect();
        if objects.is_empty() {
            return self.for_stmt();
        }
        let o = objects[self.rng.below(objects.len())].name;
        self.loops_total += 2;
        let inner = match self.rng.below(3) {
            0 => format!("{{% for b in {o} %}}{{{{ b|json_encode }}}}{{% endfor %}}"),
            1 => format!(
                "{{% for kb, b in {o} %}}{{{{ kb }}}}{{{{ loop.index }}}}/{{{{ loop.length ?? 'nl' }}}}{{% endfor %}}"
            ),
            _ => format!("{{{{ {o}|length }}}}{{{{ {o}|join(',') }}}}"),
        };
        format!(
            "{{% for ka, a in {o} %}}[{{{{ ka }}}}={{{{ a|json_encode }}}}:{inner}]{{% else %}}E{{% endfor %}}{{{{ {o}|json_encode }}}}"
        )
    }

    /// A loop written the way templates usually write them, over records (`users`: arrays in
    /// `data_json`, a `stdClass`, an `ArrayObject` and an array in `data_php`).
    fn idiom(&mut self) -> String {
        self.loops_total += 2;
        let has_users = self.data.iter().any(|d| d.name == "users");
        let seq = if has_users {
            self.rng
                .pick(&[
                    "users",
                    "users|filter(u => u.active)",
                    "users|sort((a, b) => a.name <=> b.name)",
                    "users|reverse",
                    "users|slice(1)",
                    "users|batch(2)|first",
                    "users|column('name')",
                    "users|merge([{name: 'Dee'}])",
                ])
                .to_owned()
        } else {
            "[{name: 'Ann', tags: ['a']}, {name: 'Bob'}]".to_owned()
        };
        let user = self.rng.pick(&["user", "u", "item", "v", "users", "loop"]);
        let attr = |rng: &mut Rng| {
            rng.pick(&[
                "name",
                "age",
                "tags|join(',')",
                "tags|length",
                "active ? 'A' : 'I'",
                "name|upper",
                "age ?? 'n'",
                "nope ?? 'none'",
                "name is defined ? 'D' : 'U'",
                "['name']",
                "tags|first",
                "name ~ ':' ~ loop.index",
            ])
        };
        let a1 = attr(&mut self.rng);
        let a2 = attr(&mut self.rng);
        let field = |user: &str, a: &str| {
            if a.starts_with('[') {
                format!("{user}{a}")
            } else if let Some((first, rest)) = a.split_once(' ') {
                format!("{user}.{first} {rest}")
            } else if let Some((first, rest)) = a.split_once('|') {
                format!("{user}.{first}|{rest}")
            } else {
                format!("{user}.{a}")
            }
        };
        let f1 = field(user, a1);
        let f2 = field(user, a2);
        let inner = match self.rng.below(4) {
            0 => format!(
                "{{% for tag in {user}.tags ?? [] %}}{{{{ loop.parent.loop.index }}}}.{{{{ loop.index }}}}={{{{ tag }}}}{{% if not loop.last %}},{{% endif %}}{{% else %}}-{{% endfor %}}"
            ),
            1 => "{{ cycle(['odd', 'even'], loop.index0) }}".to_owned(),
            2 => format!("{{% set total = total + ({user}.age ?? 0) %}}"),
            _ => format!(
                "{{% if loop.first %}}<ul>{{% endif %}}<li>{{{{ {f2} }}}}</li>{{% if loop.last %}}</ul>{{% endif %}}"
            ),
        };
        let pre = if inner.contains("total") {
            "{% set total = 0 %}"
        } else {
            ""
        };
        let post = if inner.contains("total") {
            "={{ total }}"
        } else {
            ""
        };
        let else_part = if self.rng.percent(30) {
            "{% else %}none"
        } else {
            ""
        };
        format!(
            "{pre}{{% for {user} in {seq} %}}{{{{ loop.index }}}}/{{{{ loop.length ?? '?' }}}}:{{{{ {f1} }}}};{inner}{else_part}{{% endfor %}}{post}[{{{{ _context|keys|join(',') }}}}]"
        )
    }

    /// Calls the macro `m` (see [`MACROS`]) with arguments taken from the current scope.
    fn macro_call(&mut self) -> String {
        self.macro_calls += 1;
        let a = if self.in_loop() && self.rng.percent(50) {
            let l = &self.loops[self.rng.below(self.loops.len())];
            l.value.clone()
        } else {
            self.atom()
        };
        let call = match self.rng.below(5) {
            0 => format!("_self.m({a})"),
            1 => format!("_self.m({a}, loop ?? 'nl', 3)"),
            2 => "_self.m()".to_owned(),
            3 => format!("_self.m(a = {a})"),
            _ => format!("_self.m({a}, _context|keys)"),
        };
        self.print(&call)
    }

    /// Overwrites a variable `ForNode` manages (`loop`, `_parent`, `_seq`, `_iterated`, `_key`)
    /// inside the body: the bookkeeping that follows (`++$context['loop']['index0']`, the restore
    /// with `array_intersect_key()`) then runs on the new value.
    fn override_private(&mut self) -> String {
        let name = self.rng.pick(&[
            "loop",
            "loop",
            "loop",
            "_parent",
            "_parent",
            "_seq",
            "_iterated",
        ]);
        let mut values = vec![
            "false",
            "null",
            "[]",
            "''",
            "0",
            "true",
            "'abc'",
            "{index0: 'a'}",
            "{x: 1}",
            "{revindex0: 1, revindex: 2}",
            "{revindex0: null, revindex: 2}",
            "loop.parent",
            "{index0: '5', index: '1.5'}",
        ];
        if self.php {
            values.extend(["ao", "eao", "ai", "obj", "mk", "aon", "sos", "gen"]);
        }
        let value = self.rng.pick(&values);
        self.tag(&format!("set {name} = {value}"))
    }

    /// `filter` on a `Traversable` returns a `CallbackFilterIterator` over an `IteratorIterator`:
    /// over the generator of `gen` it can be traversed once only; iterate it twice, nested, or
    /// count it.
    fn filtered_iteration(&mut self) -> String {
        // destructuring stops iterating early: over an iterator with a position of its own,
        // inside a loop over the same object, the outer loop would never end
        let in_loop = self.in_loop();
        let objects: Vec<&'static DataVar> = self
            .data
            .iter()
            .copied()
            .filter(|d| matches!(d.kind, Kind::Iter | Kind::IterCount))
            .filter(|d| !in_loop || !Self::shared_position(d.name))
            .collect();
        if objects.is_empty() {
            return self.for_stmt();
        }
        let o = objects[self.rng.below(objects.len())].name;
        let name = self.rng.pick(&["fi", "items", "a"]);
        let arrow = self
            .rng
            .pick(&["v => v", "(v, k) => k != 8", "v => true", "v => false"]);
        self.wrote(name);
        self.loops_total += 2;
        let first = format!(
            "{{% for k, v in {name} %}}{{{{ k }}}}={{{{ v|json_encode }}}}{{{{ loop.length ?? '' }}}};{{% else %}}E{{% endfor %}}"
        );
        let second = match self.rng.below(5) {
            0 => first.clone(),
            1 => format!("{{{{ {name}|length }}}}"),
            2 => format!(
                "{{% for v in {name} %}}{{% for w in {name} %}}{{{{ w|json_encode }}}}{{% endfor %}}{{% endfor %}}"
            ),
            3 => format!("{{{{ {name}|join(',') }}}}{{{{ {o}|json_encode }}}}"),
            _ => {
                format!("{{% do [p, q] = {name} %}}{{{{ p|json_encode }}}}{{{{ q|json_encode }}}}")
            }
        };
        format!(
            "{{% set {name} = {o}|filter({arrow}) %}}{first}|{second}|{{{{ {o}|json_encode }}}}"
        )
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
        for case in &a {
            let parsed = Case::from_json(case).expect("a valid oracle case");
            assert!(!parsed.templates.is_empty());
            assert!(parsed.id.starts_with("ctl-7-"));
        }
        assert_ne!(generate(8, 5), generate(7, 5));
    }

    #[test]
    fn covers_the_family() {
        let text: String = generate(11, 2000)
            .iter()
            .map(|c| c.get("templates").map(Json::to_compact).unwrap_or_default())
            .collect();
        for needle in [
            "{% for ",
            "{% if ",
            "{% elseif ",
            "{% else %}",
            "{% set ",
            "{% endset %}",
            "{% with ",
            " only ",
            "{% apply ",
            "{% do ",
            "loop.parent",
            "_context|keys",
            "{% guard ",
        ] {
            assert!(text.contains(needle), "no case contains {needle:?}");
        }
    }
}
