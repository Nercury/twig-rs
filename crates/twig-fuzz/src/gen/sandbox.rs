//! Generator of the `sandbox` family: random security policies and templates that exercise them.
//!
//! Every case renders in a plain environment with the `SandboxExtension` (sandboxed globally most of the time,
//! otherwise through `{% sandbox %}` and `include(..., sandboxed = true)`) and a random `SecurityPolicy`:
//!
//! - allowed tags, filters, functions and tests: built from the names the generated templates use, with some left
//!   out (the first one not allowed fails, at the line of its first use), case variants (never equal) and decoys;
//!   strict mode on or off (`extends`, `use`, `parent`, `block`, `attribute` and the tests are let through with a
//!   deprecation when it is off);
//! - allowed methods and properties per class: the exact class, its parents and interfaces (`Stringable`,
//!   `Traversable`, `Countable`, `ArrayAccess`, ...), case variants of class and member names (classes and methods
//!   are case-insensitive, properties are not), `__toString` in any case, a single member given as a string, decoys,
//!   rarely a numeric class key.
//!
//! The data (`data_php`) holds objects whose classes `twig-test-support` rebuilds: the helper classes of Twig's
//! `tests/IntegrationTest.php` (`"test_classes": true`), the classes declared inline by fixtures (declared here with
//! the fixtures' code, guarded by `class_exists()`), `ArrayObject` (also one containing itself), `ArrayIterator`,
//! `SplObjectStorage`, `SplFileInfo`, `CallbackFilterIterator` (constant callbacks only; its parents
//! `FilterIterator`/`IteratorIterator` serve as policy classes), `SimpleXMLElement`, `stdClass`, `Twig\Markup`,
//! dates, enum cases and a `TemplateWrapper`, plus nested arrays mixing objects with falsy values. No class of the
//! ports is a non-array-like `ArrayAccess`, so the sandbox's property check of array keys (`getAttribute()` on such
//! objects) is out of reach.
//!
//! The templates call methods (with string and object arguments, spreads), read properties and array keys (`.`,
//! `.(expr)`, `[]`, `?.` chains, `attribute()`, `is defined`, `??`, `default`, object destructuring), and convert
//! objects to strings in every place the sandbox visitor wraps (print, `~`, comparisons, `in`, `matches`, filter
//! inputs and arguments, function arguments, spreads, dynamic mapping keys, assignments, arrow function bodies,
//! template names of `include`/`extends`/`import`/`embed`/`block()`, dynamic macro names, `deprecated` and its
//! `package`/`version`, `_context` and `loop` walks), with string callables where the sandbox demands closures,
//! inside tags (`if`, `for`, `set`, `apply`, `autoescape`, `with`, `do`, `guard`, blocks, macros), across templates
//! (`include`/`include_only` with object variables, `embed`, `extends`, `use`, `import`/`from`, `_self` macros,
//! `template_from_string()`, sandboxed includes of templates that use traits or extend others) and over several
//! lines (error lines, `{% line %}`, newlines inside expressions and tags).
//!
//! Generated cases avoid what differs by design: no template recursion (`inc.twig` and the templates only it refers
//! to never include anything), no `dump()` of objects, no unseeded randomness. Known mismatches are generated only
//! rarely, so that new batches still show new clusters: numeric policy class keys, `{% deprecated %}` arguments
//! that are not strings (see `tests/vectors/fuzz/sandbox/known-mismatches.json`).
//!
//! [`generate`] is deterministic: case `i` of seed `s` only depends on `(s, i)`. Run its cases with
//! `"isolation": "process"` ([`batch`]).

use std::collections::BTreeSet;

use crate::kit::json::Json;

/// The family name.
pub const FAMILY: &str = "sandbox";

/// Generates `count` cases from `seed` (ids `sbx-<seed>-<i>`).
pub fn generate(seed: u64, count: usize) -> Vec<Json> {
    (0..count).map(|i| case(seed, i)).collect()
}

/// [`generate`] as a batch object whose cases run in a PHP process each (`"isolation": "process"`): with
/// `cache => false`, a template class compiled by an earlier case of the same process is reused, so its parse-time
/// deprecations are not raised again, and classes declared or loaded by one case would exist in the next.
pub fn batch(seed: u64, count: usize) -> Json {
    Json::Object(vec![
        ("cases".to_owned(), Json::Array(generate(seed, count))),
        ("isolation".to_owned(), Json::str("process")),
    ])
}

/// Case `index` of `seed`.
pub fn case(seed: u64, index: usize) -> Json {
    let mut rng = Rng::new(seed ^ (index as u64).wrapping_mul(0xD1B5_4A32_D192_ED03));
    Gen::new(&mut rng).case(format!("sbx-{seed}-{index}"))
}

// ---------------------------------------------------------------------------------------------------------------
// randomness

/// SplitMix64.
pub struct Rng(u64);

impl Rng {
    /// A generator for `seed`.
    pub fn new(seed: u64) -> Rng {
        let mut rng = Rng(seed);
        rng.next_u64();
        rng
    }

    /// The next 64 random bits.
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number in `0..n` (`n > 0`).
    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }

    /// True with probability `p`.
    pub fn chance(&mut self, p: f64) -> bool {
        ((self.next_u64() >> 11) as f64 / (1u64 << 53) as f64) < p
    }

    /// One of `items` (non-empty).
    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }

    /// One of `items`, weighted by the first field.
    fn weighted<'a, T>(&mut self, items: &'a [(u32, T)]) -> &'a T {
        let total: u32 = items.iter().map(|(w, _)| w).sum();
        let mut roll = self.below(total as usize) as u32;
        for (weight, item) in items {
            if roll < *weight {
                return item;
            }
            roll -= weight;
        }
        &items[items.len() - 1].1
    }
}

// ---------------------------------------------------------------------------------------------------------------
// the objects

/// Classes declared by the data code (the fixtures' inline classes).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Prelude {
    Stringy,
    Magic,
    MyObj,
    User,
    Items,
    ItemsCountable,
}

impl Prelude {
    /// The declaration, as the fixture writes it, guarded for a batch that shares one PHP process.
    fn code(self) -> &'static str {
        match self {
            // expressions/array_dynamic_keys.test
            Prelude::Stringy => {
                "if (!class_exists('TwigTestStringy', false)) {\n\
                 class TwigTestStringy implements \\Stringable {\n\
                 \x20   public function __construct(private string $v) {}\n\
                 \x20   public function __toString(): string { return $this->v; }\n\
                 }\n\
                 class TwigTestHolder {\n\
                 \x20   public string $name = 'attr';\n\
                 \x20   public TwigTestStringy $stringable;\n\
                 \x20   public function __construct() { $this->stringable = new TwigTestStringy('attr_obj'); }\n\
                 \x20   public function getStringable(): TwigTestStringy { return new TwigTestStringy('method_obj'); }\n\
                 }\n\
                 }\n"
            }
            // expressions/magic_call.test
            Prelude::Magic => {
                "if (!class_exists('TestClassForMagicCallAttributes', false)) {\n\
                 class TestClassForMagicCallAttributes\n\
                 {\n\
                 \x20   public function getBar()\n\
                 \x20   {\n\
                 \x20       return 'bar_from_getbar';\n\
                 \x20   }\n\
                 \n\
                 \x20   public function __call($method, $arguments)\n\
                 \x20   {\n\
                 \x20       if ('foo' === $method) {\n\
                 \x20           return 'foo_from_call';\n\
                 \x20       }\n\
                 \n\
                 \x20       return false;\n\
                 \x20   }\n\
                 }\n\
                 }\n"
            }
            // regression/block-inheritance-issue.test
            Prelude::MyObj => {
                "if (!class_exists('MyObj', false)) {\n\
                 class MyObj {\n\
                 \x20   public function __toString() {\n\
                 \x20       return 'foo';\n\
                 \x20   }\n\
                 }\n\
                 }\n"
            }
            // tags/autoescape/objects.test
            Prelude::User => {
                "if (!class_exists('UserForAutoEscapeTest', false)) {\n\
                 class UserForAutoEscapeTest\n\
                 {\n\
                 \x20 public function getName()\n\
                 \x20 {\n\
                 \x20   return 'Fabien<br />';\n\
                 \x20 }\n\
                 \n\
                 \x20 public function __toString()\n\
                 \x20 {\n\
                 \x20    return 'Fabien<br />';\n\
                 \x20 }\n\
                 }\n\
                 }\n"
            }
            // tags/for/objects.test
            Prelude::Items => {
                "if (!class_exists('ItemsIterator', false)) {\n\
                 class ItemsIterator implements \\Iterator\n\
                 {\n\
                 \x20 protected $values = ['foo' => 'bar', 'bar' => 'foo'];\n\
                 \x20 #[\\ReturnTypeWillChange]\n\
                 \x20 public function current() { return current($this->values); }\n\
                 \x20 #[\\ReturnTypeWillChange]\n\
                 \x20 public function key() { return key($this->values); }\n\
                 \x20 public function next(): void { next($this->values); }\n\
                 \x20 public function rewind(): void { reset($this->values); }\n\
                 \x20 public function valid(): bool { return false !== current($this->values); }\n\
                 }\n\
                 }\n"
            }
            // tags/for/objects_countable.test
            Prelude::ItemsCountable => {
                "if (!class_exists('ItemsIteratorCountable', false)) {\n\
                 class ItemsIteratorCountable implements \\Iterator, \\Countable\n\
                 {\n\
                 \x20 protected $values = ['foo' => 'bar', 'bar' => 'foo'];\n\
                 \x20 #[\\ReturnTypeWillChange]\n\
                 \x20 public function current() { return current($this->values); }\n\
                 \x20 #[\\ReturnTypeWillChange]\n\
                 \x20 public function key() { return key($this->values); }\n\
                 \x20 public function next(): void { next($this->values); }\n\
                 \x20 public function rewind(): void { reset($this->values); }\n\
                 \x20 public function valid(): bool { return false !== current($this->values); }\n\
                 \x20 public function count(): int { return count($this->values); }\n\
                 }\n\
                 }\n"
            }
        }
    }
}

/// An object kind of the data.
struct Kind {
    /// Variable name prefix.
    var: &'static str,
    /// `$obj::class`.
    class: &'static str,
    /// The other classes and interfaces it is an instance of.
    isa: &'static [&'static str],
    /// Its public methods.
    methods: &'static [&'static str],
    /// Names for `o.x`, `o['x']`, `attribute(o, 'x')`: methods, `get`/`is` aliases, properties, constants and
    /// names that do not exist.
    attrs: &'static [&'static str],
    /// Method calls with arguments (`o.x(...)`).
    calls: &'static [&'static str],
    /// Attribute snippets that return objects, with the kind (index into [`KINDS`]) they return.
    chains: &'static [(&'static str, usize)],
    /// Property names for the policy (some do not exist).
    props: &'static [&'static str],
    /// Has `__toString()`.
    stringable: bool,
    /// Is `Traversable`.
    traversable: bool,
    /// Declared by the data code.
    prelude: Option<Prelude>,
}

const FOO: usize = 0;
const STRINGY: usize = 6;
const MARKUP: usize = 13;
const DATETIME_ZONE: usize = 16;
const DATETIME_IMMUTABLE: usize = 15;
const ARRAY_ITERATOR: usize = 21;

/// The object kinds; the indexes above name some of them.
const KINDS: &[Kind] = &[
    // 0
    Kind {
        var: "foo",
        class: "Twig\\Tests\\TwigTestFoo",
        isa: &["Iterator", "Traversable"],
        methods: &[
            "bar",
            "getFoo",
            "getEmpty",
            "getNull",
            "getSelf",
            "is",
            "in",
            "not",
            "strToLower",
            "rewind",
            "current",
            "key",
            "next",
            "valid",
        ],
        attrs: &[
            "bar",
            "foo",
            "getFoo",
            "getfoo",
            "FOO",
            "empty",
            "null",
            "self",
            "is",
            "in",
            "not",
            "position",
            "array",
            "BAR_NAME",
            "bar_name",
            "key",
            "current",
            "valid",
            "missing",
            "0",
            "__toString",
            "Position",
            "getSelf",
        ],
        calls: &[
            "bar('x')",
            "bar('x', 'y')",
            "BAR(1)",
            "strToLower('ABC')",
            "strtolower('AbC')",
            "getFoo()",
            "getEmpty()",
            "is()",
            "not()",
            "current()",
            "key()",
        ],
        chains: &[("getSelf()", FOO), ("self", FOO)],
        props: &[
            "position", "array", "Position", "foo", "bar", "BAR_NAME", "self",
        ],
        stringable: false,
        traversable: true,
        prelude: None,
    },
    // 1
    Kind {
        var: "ts",
        class: "Twig\\Tests\\ToStringStub",
        isa: &["Stringable"],
        methods: &["__construct", "__toString"],
        attrs: &["string", "__toString", "__tostring", "toString", "missing"],
        calls: &["__toString()", "__TOSTRING()"],
        chains: &[],
        props: &["string", "String"],
        stringable: true,
        traversable: false,
        prelude: None,
    },
    // 2
    Kind {
        var: "cs",
        class: "Twig\\Tests\\CountableStub",
        isa: &["Countable", "Stringable"],
        methods: &["__construct", "count", "__toString"],
        attrs: &["count", "Count", "missing", "__toString"],
        calls: &["count()"],
        chains: &[],
        props: &["count"],
        stringable: true,
        traversable: false,
        prelude: None,
    },
    // 3
    Kind {
        var: "ias",
        class: "Twig\\Tests\\IteratorAggregateStub",
        isa: &["IteratorAggregate", "Traversable"],
        methods: &["__construct", "getIterator"],
        attrs: &["iterator", "data", "missing", "getIterator"],
        calls: &["getIterator()"],
        chains: &[
            ("getIterator()", ARRAY_ITERATOR),
            ("iterator", ARRAY_ITERATOR),
        ],
        props: &["data", "iterator"],
        stringable: false,
        traversable: true,
        prelude: None,
    },
    // 4
    Kind {
        var: "sit",
        class: "Twig\\Tests\\SimpleIteratorForTesting",
        isa: &["Iterator", "Traversable", "Stringable"],
        methods: &["current", "next", "key", "valid", "rewind", "__toString"],
        attrs: &["current", "key", "valid", "data", "missing", "__toString"],
        calls: &["current()", "key()", "valid()"],
        chains: &[],
        props: &["data", "key"],
        stringable: true,
        traversable: true,
        prelude: None,
    },
    // 5
    Kind {
        var: "mcs",
        class: "Twig\\Tests\\MagicCallStub",
        isa: &[],
        methods: &["__call"],
        attrs: &["anything", "foo", "__call"],
        calls: &["anything()", "foo(1)"],
        chains: &[],
        props: &["anything"],
        stringable: false,
        traversable: false,
        prelude: None,
    },
    // 6
    Kind {
        var: "st",
        class: "TwigTestStringy",
        isa: &["Stringable"],
        methods: &["__construct", "__toString"],
        attrs: &["v", "__toString", "missing"],
        calls: &["__toString()"],
        chains: &[],
        props: &["v"],
        stringable: true,
        traversable: false,
        prelude: Some(Prelude::Stringy),
    },
    // 7
    Kind {
        var: "h",
        class: "TwigTestHolder",
        isa: &[],
        methods: &["__construct", "getStringable"],
        attrs: &["name", "stringable", "getStringable", "Name", "missing"],
        calls: &["getStringable()"],
        chains: &[("stringable", STRINGY), ("getStringable()", STRINGY)],
        props: &["name", "stringable", "Name"],
        stringable: false,
        traversable: false,
        prelude: Some(Prelude::Stringy),
    },
    // 8
    Kind {
        var: "mc",
        class: "TestClassForMagicCallAttributes",
        isa: &[],
        methods: &["getBar", "__call"],
        attrs: &["foo", "bar", "getBar", "Foo", "baz", "__call"],
        calls: &["foo()", "getBar()", "baz(1)"],
        chains: &[],
        props: &["foo", "bar"],
        stringable: false,
        traversable: false,
        prelude: Some(Prelude::Magic),
    },
    // 9
    Kind {
        var: "mo",
        class: "MyObj",
        isa: &["Stringable"],
        methods: &["__toString"],
        attrs: &["__toString", "foo"],
        calls: &["__toString()"],
        chains: &[],
        props: &["foo"],
        stringable: true,
        traversable: false,
        prelude: Some(Prelude::MyObj),
    },
    // 10
    Kind {
        var: "u",
        class: "UserForAutoEscapeTest",
        isa: &["Stringable"],
        methods: &["getName", "__toString"],
        attrs: &["name", "getName", "NAME", "__toString", "missing"],
        calls: &["getName()", "getname()"],
        chains: &[],
        props: &["name"],
        stringable: true,
        traversable: false,
        prelude: Some(Prelude::User),
    },
    // 11
    Kind {
        var: "it",
        class: "ItemsIterator",
        isa: &["Iterator", "Traversable"],
        methods: &["current", "key", "next", "rewind", "valid"],
        attrs: &["current", "key", "values", "valid", "foo"],
        calls: &["current()", "key()"],
        chains: &[],
        props: &["values"],
        stringable: false,
        traversable: true,
        prelude: Some(Prelude::Items),
    },
    // 12
    Kind {
        var: "itc",
        class: "ItemsIteratorCountable",
        isa: &["Iterator", "Traversable", "Countable"],
        methods: &["current", "key", "next", "rewind", "valid", "count"],
        attrs: &["current", "count", "values", "foo"],
        calls: &["count()", "current()"],
        chains: &[],
        props: &["values"],
        stringable: false,
        traversable: true,
        prelude: Some(Prelude::ItemsCountable),
    },
    // 13
    Kind {
        var: "mk",
        class: "Twig\\Markup",
        isa: &["Countable", "JsonSerializable", "Stringable"],
        methods: &[
            "__construct",
            "__toString",
            "getCharset",
            "withContent",
            "isSafeFor",
            "count",
            "jsonSerialize",
        ],
        attrs: &[
            "count",
            "charset",
            "getCharset",
            "content",
            "__toString",
            "jsonSerialize",
            "safeStrategies",
            "missing",
        ],
        calls: &[
            "withContent('w')",
            "isSafeFor('html')",
            "getCharset()",
            "count()",
            "__toString()",
            "WITHCONTENT('x')",
            "__construct('n', 'UTF-8')",
        ],
        chains: &[("withContent('w')", MARKUP)],
        props: &["content", "charset", "safeStrategies"],
        stringable: true,
        traversable: false,
        prelude: None,
    },
    // 14
    Kind {
        var: "dt",
        class: "DateTime",
        isa: &["DateTimeInterface"],
        methods: &[
            "format",
            "getTimestamp",
            "getTimezone",
            "getOffset",
            "modify",
        ],
        attrs: &[
            "timestamp",
            "getTimestamp",
            "timezone",
            "date",
            "timezone_type",
            "offset",
            "missing",
        ],
        calls: &[
            "format('Y-m-d')",
            "getTimestamp()",
            "getOffset()",
            "modify('+1 day').format('d')",
        ],
        chains: &[
            ("getTimezone()", DATETIME_ZONE),
            ("timezone", DATETIME_ZONE),
        ],
        props: &["date", "timezone", "timezone_type", "timestamp"],
        stringable: false,
        traversable: false,
        prelude: None,
    },
    // 15
    Kind {
        var: "di",
        class: "DateTimeImmutable",
        isa: &["DateTimeInterface"],
        methods: &["format", "getTimestamp", "getTimezone", "modify"],
        attrs: &["timestamp", "timezone", "date", "missing"],
        calls: &["format('c')", "getTimestamp()"],
        chains: &[
            ("modify('+1 day')", DATETIME_IMMUTABLE),
            ("getTimezone()", DATETIME_ZONE),
        ],
        props: &["date", "timezone"],
        stringable: false,
        traversable: false,
        prelude: None,
    },
    // 16
    Kind {
        var: "tz",
        class: "DateTimeZone",
        isa: &[],
        methods: &["getName", "getLocation"],
        attrs: &["name", "getName", "missing", "timezone"],
        calls: &["getName()"],
        chains: &[],
        props: &["name", "timezone_type", "timezone"],
        stringable: false,
        traversable: false,
        prelude: None,
    },
    // 17
    Kind {
        var: "iv",
        class: "DateInterval",
        isa: &[],
        methods: &["format"],
        attrs: &["d", "y", "days", "invert", "f", "missing", "D"],
        calls: &["format('%d|%h')"],
        chains: &[],
        props: &["d", "y", "days", "invert", "f", "D"],
        stringable: false,
        traversable: false,
        prelude: None,
    },
    // 18
    Kind {
        var: "en",
        class: "Twig\\Tests\\DummyBackedEnum",
        isa: &["UnitEnum", "BackedEnum"],
        methods: &["cases", "from", "tryFrom"],
        attrs: &["name", "value", "cases", "missing", "Name"],
        calls: &[
            "from('bar').name",
            "tryFrom('x') is null ? 'n' : 'y'",
            "cases()|length",
        ],
        chains: &[],
        props: &["name", "value", "Name"],
        stringable: false,
        traversable: false,
        prelude: None,
    },
    // 19
    Kind {
        var: "ue",
        class: "Twig\\Tests\\DummyUnitEnum",
        isa: &["UnitEnum"],
        methods: &["cases"],
        attrs: &["name", "cases", "value"],
        calls: &["cases()|length"],
        chains: &[],
        props: &["name"],
        stringable: false,
        traversable: false,
        prelude: None,
    },
    // 20
    Kind {
        var: "ao",
        class: "ArrayObject",
        isa: &[
            "IteratorAggregate",
            "Traversable",
            "ArrayAccess",
            "Serializable",
            "Countable",
        ],
        methods: &[
            "offsetExists",
            "offsetGet",
            "count",
            "getArrayCopy",
            "getIterator",
            "getFlags",
        ],
        attrs: &[
            "a",
            "b",
            "0",
            "count",
            "getArrayCopy",
            "flags",
            "missing",
            "iterator",
        ],
        calls: &[
            "count()",
            "offsetGet('a')",
            "offsetExists('b') ? 'y' : 'n'",
            "getArrayCopy()|length",
        ],
        chains: &[("getIterator()", ARRAY_ITERATOR)],
        props: &["a", "b", "0", "count", "flags"],
        stringable: false,
        traversable: true,
        prelude: None,
    },
    // 21
    Kind {
        var: "ai",
        class: "ArrayIterator",
        isa: &[
            "SeekableIterator",
            "Iterator",
            "Traversable",
            "ArrayAccess",
            "Serializable",
            "Countable",
        ],
        methods: &[
            "offsetExists",
            "offsetGet",
            "count",
            "current",
            "key",
            "getArrayCopy",
        ],
        attrs: &["0", "1", "a", "count", "current", "key", "missing"],
        calls: &["count()", "current()", "offsetGet(0)"],
        chains: &[],
        props: &["0", "1", "a", "count"],
        stringable: false,
        traversable: true,
        prelude: None,
    },
    // 22
    Kind {
        var: "so",
        class: "SplObjectStorage",
        isa: &[
            "Countable",
            "Iterator",
            "Traversable",
            "Serializable",
            "ArrayAccess",
            "SeekableIterator",
        ],
        methods: &["count", "current", "key", "valid", "getInfo", "contains"],
        attrs: &["count", "current", "key", "info", "missing"],
        calls: &["count()", "key()"],
        chains: &[],
        props: &["count", "info"],
        stringable: false,
        traversable: true,
        prelude: None,
    },
    // 23
    Kind {
        var: "fi",
        class: "SplFileInfo",
        isa: &["Stringable"],
        methods: &[
            "getFilename",
            "getPath",
            "getExtension",
            "getBasename",
            "getPathname",
            "__toString",
        ],
        attrs: &[
            "filename",
            "path",
            "extension",
            "basename",
            "pathname",
            "__toString",
            "missing",
        ],
        calls: &["getBasename('.txt')", "getFilename()", "getExtension()"],
        chains: &[],
        props: &["filename", "path", "pathName"],
        stringable: true,
        traversable: false,
        prelude: None,
    },
    // 24
    Kind {
        var: "std",
        class: "stdClass",
        isa: &[],
        methods: &[],
        attrs: &["a", "b", "0", "A", "missing", "o"],
        calls: &["a()", "missing()"],
        chains: &[("o", STRINGY)],
        props: &["a", "b", "0", "A", "o"],
        stringable: false,
        traversable: false,
        prelude: Some(Prelude::Stringy),
    },
    // 25
    Kind {
        var: "tw",
        class: "Twig\\TemplateWrapper",
        isa: &[],
        methods: &[
            "render",
            "renderBlock",
            "hasBlock",
            "getBlockNames",
            "getTemplateName",
        ],
        attrs: &["templateName", "getTemplateName", "blockNames", "missing"],
        calls: &[
            "render({})",
            "render({x: 1})",
            "hasBlock('tb') ? 'y' : 'n'",
            "getBlockNames()|join",
            "renderBlock('tb', {})",
        ],
        chains: &[],
        props: &["templateName", "template"],
        stringable: false,
        traversable: false,
        prelude: None,
    },
    // 26
    Kind {
        var: "cf",
        class: "CallbackFilterIterator",
        isa: &[
            "FilterIterator",
            "IteratorIterator",
            "OuterIterator",
            "Iterator",
            "Traversable",
        ],
        methods: &[
            "accept",
            "rewind",
            "next",
            "getInnerIterator",
            "valid",
            "key",
            "current",
        ],
        attrs: &[
            "current",
            "key",
            "valid",
            "innerIterator",
            "accept",
            "missing",
        ],
        calls: &[
            "getInnerIterator().count()",
            "accept() ? 'y' : 'n'",
            "current()",
            "valid() ? 'v' : 'i'",
        ],
        chains: &[
            ("getInnerIterator()", ARRAY_ITERATOR),
            ("innerIterator", ARRAY_ITERATOR),
        ],
        props: &["current", "innerIterator"],
        stringable: false,
        traversable: true,
        prelude: None,
    },
    // 27
    Kind {
        var: "sx",
        class: "SimpleXMLElement",
        isa: &[
            "Stringable",
            "Countable",
            "RecursiveIterator",
            "Iterator",
            "Traversable",
        ],
        methods: &["getName", "count", "__toString"],
        attrs: &["getName", "count", "__toString", "missing"],
        calls: &["getName()", "count()", "__toString()"],
        chains: &[],
        props: &["c", "b"],
        stringable: true,
        traversable: true,
        prelude: None,
    },
];

/// The kinds a case picks objects from, weighted.
const KIND_WEIGHTS: &[(u32, usize)] = &[
    (6, 0),
    (6, 1),
    (3, 2),
    (3, 3),
    (3, 4),
    (2, 5),
    (5, 6),
    (3, 7),
    (3, 8),
    (5, 9),
    (4, 10),
    (2, 11),
    (2, 12),
    (5, 13),
    (2, 14),
    (1, 15),
    (1, 16),
    (1, 17),
    (2, 18),
    (1, 19),
    (3, 20),
    (2, 21),
    (2, 22),
    (2, 23),
    (3, 24),
    (2, 25),
    (2, 26),
    (1, 27),
];

/// Objects for arrays (PHP expression, prelude, kind).
const ELEMENT_OBJECTS: &[(&str, Option<Prelude>, usize)] = &[
    ("new MyObj()", Some(Prelude::MyObj), 9),
    ("new \\Twig\\Tests\\ToStringStub('e1')", None, 1),
    ("new \\Twig\\Tests\\ToStringStub('')", None, 1),
    ("new TwigTestStringy('e2')", Some(Prelude::Stringy), STRINGY),
    ("new \\Twig\\Markup('<i>m</i>', 'UTF-8')", None, MARKUP),
    ("new UserForAutoEscapeTest()", Some(Prelude::User), 10),
    ("new \\Twig\\Tests\\TwigTestFoo()", None, FOO),
    ("new \\Twig\\Tests\\CountableStub(2)", None, 2),
    (
        "new \\ArrayObject(['x' => new MyObj()])",
        Some(Prelude::MyObj),
        20,
    ),
    ("\\Twig\\Tests\\DummyBackedEnum::BAR", None, 18),
    (
        "new \\Twig\\Tests\\IteratorAggregateStub([new MyObj(), 0])",
        Some(Prelude::MyObj),
        3,
    ),
    ("new TwigTestHolder()", Some(Prelude::Stringy), 7),
];

const SCALARS: &[&str] = &[
    "0", "1", "''", "'0'", "'abc'", "null", "false", "true", "[]", "2.5", "'a,b'", "'<b>'",
];

const STRINGS: &[&str] = &["'abc'", "''", "'0'", "'<b>x</b>'", "'a,b'", "'ABC'", "'é'"];

// ---------------------------------------------------------------------------------------------------------------
// the generator

/// What a value is, as far as the generator knows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ty {
    /// An object of the kind (index into [`KINDS`]).
    Obj(usize),
    /// An array (maybe with objects).
    Arr,
    /// A scalar or anything else.
    Any,
}

/// An expression and what it gives.
#[derive(Clone, Debug)]
struct Val {
    e: String,
    ty: Ty,
}

impl Val {
    fn new(e: impl Into<String>, ty: Ty) -> Val {
        Val { e: e.into(), ty }
    }

    /// The expression, parenthesized unless it is a name or an attribute access.
    fn atom(&self) -> String {
        let simple = self
            .e
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'.' || b == b'?');
        if simple && !self.e.is_empty() {
            self.e.clone()
        } else {
            format!("({})", self.e)
        }
    }
}

/// A data variable.
#[derive(Clone, Debug)]
struct Var {
    name: String,
    ty: Ty,
}

/// The names the templates use.
#[derive(Default)]
struct Used {
    tags: BTreeSet<String>,
    filters: BTreeSet<String>,
    functions: BTreeSet<String>,
    tests: BTreeSet<String>,
}

/// How the templates are put together.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shape {
    Plain,
    Include,
    Extends,
    Embed,
    Use,
    Macros,
    SandboxTag,
    StringLoader,
    SelfMacro,
}

struct Gen<'r> {
    rng: &'r mut Rng,
    vars: Vec<Var>,
    used: Used,
    preludes: BTreeSet<Prelude>,
    /// Kinds present in the data (for the policy).
    kinds: BTreeSet<usize>,
    /// Kinds of objects inside arrays and other objects.
    element_kinds: BTreeSet<usize>,
    /// Templates other than the main one.
    extra: Vec<(String, String)>,
    /// Whether arrow functions may refer to the context.
    depth: usize,
    autoescape: bool,
    string_loader: bool,
    debug_ext: bool,
    global: bool,
    /// How often parts are separated by newlines.
    newline_bias: f64,
    /// Generating a macro body (no blocks there).
    in_macro: bool,
    /// Generating `inc.twig` (no includes there: no recursion).
    in_include_target: bool,
    /// Blocks named so far (`u1`, `u2`, ...).
    blocks: usize,
}

impl<'r> Gen<'r> {
    fn new(rng: &'r mut Rng) -> Gen<'r> {
        Gen {
            rng,
            vars: Vec::new(),
            used: Used::default(),
            preludes: BTreeSet::new(),
            kinds: BTreeSet::new(),
            element_kinds: BTreeSet::new(),
            extra: Vec::new(),
            depth: 0,
            autoescape: false,
            string_loader: false,
            debug_ext: false,
            global: true,
            newline_bias: 0.2,
            in_macro: false,
            in_include_target: false,
            blocks: 0,
        }
    }

    // ---- names ------------------------------------------------------------------------------------------------

    fn tag(&mut self, name: &str) -> String {
        self.used.tags.insert(name.to_owned());
        name.to_owned()
    }

    fn filter(&mut self, name: &str) -> String {
        self.used.filters.insert(name.to_owned());
        name.to_owned()
    }

    fn function(&mut self, name: &str) -> String {
        self.used.functions.insert(name.to_owned());
        name.to_owned()
    }

    fn test(&mut self, name: &str) -> String {
        self.used.tests.insert(name.to_owned());
        name.to_owned()
    }

    /// A separator between two template parts.
    fn sep(&mut self) -> &'static str {
        if self.rng.chance(self.newline_bias) {
            if self.rng.chance(0.2) { "\n\n" } else { "\n" }
        } else if self.rng.chance(0.3) {
            " "
        } else {
            ""
        }
    }

    /// Whitespace inside an expression (a newline now and then, for error lines).
    fn ws(&mut self) -> &'static str {
        if self.rng.chance(0.06) { "\n" } else { " " }
    }

    // ---- data -------------------------------------------------------------------------------------------------

    /// PHP code for an object of `kind`.
    fn php_object(&mut self, kind: usize) -> String {
        if let Some(p) = KINDS[kind].prelude {
            self.preludes.insert(p);
        }
        match kind {
            0 => "new \\Twig\\Tests\\TwigTestFoo()".to_owned(),
            1 => format!(
                "new \\Twig\\Tests\\ToStringStub({})",
                self.rng.pick(STRINGS)
            ),
            2 => format!("new \\Twig\\Tests\\CountableStub({})", self.rng.below(4)),
            3 => format!(
                "new \\Twig\\Tests\\IteratorAggregateStub({})",
                self.php_array(2)
            ),
            4 => "new \\Twig\\Tests\\SimpleIteratorForTesting()".to_owned(),
            5 => "new \\Twig\\Tests\\MagicCallStub()".to_owned(),
            6 => format!("new TwigTestStringy({})", self.rng.pick(STRINGS)),
            7 => "new TwigTestHolder()".to_owned(),
            8 => "new TestClassForMagicCallAttributes()".to_owned(),
            9 => "new MyObj()".to_owned(),
            10 => "new UserForAutoEscapeTest()".to_owned(),
            11 => "new ItemsIterator()".to_owned(),
            12 => "new ItemsIteratorCountable()".to_owned(),
            13 => {
                let content = self.rng.pick(STRINGS);
                if self.rng.chance(0.15) {
                    format!("new \\Twig\\Markup({content}, 'UTF-8', ['html'])")
                } else {
                    format!("new \\Twig\\Markup({content}, 'UTF-8')")
                }
            }
            14 => "new \\DateTime('2020-01-02 03:04:05', new \\DateTimeZone('UTC'))".to_owned(),
            15 => {
                "new \\DateTimeImmutable('2021-05-06 07:08:09', new \\DateTimeZone('Europe/Paris'))"
                    .to_owned()
            }
            16 => "new \\DateTimeZone('Europe/Vilnius')".to_owned(),
            17 => "new \\DateInterval('P1Y2M3DT4H5M6S')".to_owned(),
            18 => (*self.rng.pick(&[
                "\\Twig\\Tests\\DummyBackedEnum::FOO",
                "\\Twig\\Tests\\DummyBackedEnum::BAR",
            ]))
            .to_owned(),
            19 => (*self.rng.pick(&[
                "\\Twig\\Tests\\DummyUnitEnum::BAR",
                "\\Twig\\Tests\\DummyUnitEnum::BAZ",
            ]))
            .to_owned(),
            20 if self.rng.chance(0.12) => {
                // contains itself: the walk of the sandbox remembers the Traversables it has seen
                self.preludes.insert(Prelude::MyObj);
                "(static function () { $a = new \\ArrayObject(['x' => new MyObj()]); $a['me'] = $a; return $a; })()"
                    .to_owned()
            }
            20 => format!("new \\ArrayObject({})", self.php_map()),
            21 => format!("new \\ArrayIterator({})", self.php_array(2)),
            22 => {
                let (a, pa, ka) = *self.rng.pick(ELEMENT_OBJECTS);
                if let Some(p) = pa {
                    self.preludes.insert(p);
                }
                self.element_kinds.insert(ka);
                format!(
                    "(static function () {{ $s = new \\SplObjectStorage(); $s[{a}] = 'i1'; $s[new \\Twig\\Tests\\ToStringStub('k')] = 2; return $s; }})()"
                )
            }
            23 => format!(
                "new \\SplFileInfo({})",
                self.rng.pick(&["'/tmp/x.txt'", "'a/b.tar.gz'", "'plain'"])
            ),
            24 => {
                self.preludes.insert(Prelude::Stringy);
                format!(
                    "(object) ['a' => {}, 'b' => {}, 'o' => new TwigTestStringy('so'), '0' => 'zero']",
                    self.rng.pick(STRINGS),
                    self.php_element()
                )
            }
            25 => "$twig->load('tw.twig')".to_owned(),
            26 => format!(
                "new \\CallbackFilterIterator(new \\ArrayIterator({}), fn ($v) => {})",
                self.php_array(1),
                if self.rng.chance(0.85) {
                    "true"
                } else {
                    "false"
                }
            ),
            27 => (*self.rng.pick(&[
                "new \\SimpleXMLElement('<a b=\"1\"><c>t</c><c>u</c></a>')",
                "new \\SimpleXMLElement('<r>text</r>')",
            ]))
            .to_owned(),
            _ => unreachable!("kind {kind}"),
        }
    }

    /// A PHP value for an array element.
    fn php_element(&mut self) -> String {
        if self.rng.chance(0.45) {
            let (code, prelude, kind) = *self.rng.pick(ELEMENT_OBJECTS);
            if let Some(p) = prelude {
                self.preludes.insert(p);
            }
            self.element_kinds.insert(kind);
            code.to_owned()
        } else {
            (*self.rng.pick(SCALARS)).to_owned()
        }
    }

    /// A PHP list.
    fn php_array(&mut self, depth: usize) -> String {
        let n = self.rng.below(4);
        let mut items = Vec::new();
        for _ in 0..n {
            if depth > 0 && self.rng.chance(0.15) {
                items.push(self.php_array(depth - 1));
            } else {
                items.push(self.php_element());
            }
        }
        format!("[{}]", items.join(", "))
    }

    /// A PHP map.
    fn php_map(&mut self) -> String {
        let n = 1 + self.rng.below(3);
        let keys = ["'a'", "'b'", "'c'", "0", "'x y'"];
        let mut items = Vec::new();
        for key in keys.iter().take(n) {
            items.push(format!("{key} => {}", self.php_element()));
        }
        format!("[{}]", items.join(", "))
    }

    /// Picks the data: objects, arrays and scalars.
    fn data(&mut self) -> String {
        let mut entries: Vec<(String, String)> = Vec::new();
        let objects = 1 + self.rng.below(4);
        for i in 0..objects {
            let kind = *self.rng.weighted(KIND_WEIGHTS);
            let name = if i == 0 || !self.vars.iter().any(|v| v.name == KINDS[kind].var) {
                KINDS[kind].var.to_owned()
            } else {
                format!("{}{i}", KINDS[kind].var)
            };
            if self.vars.iter().any(|v| v.name == name) {
                continue;
            }
            let code = self.php_object(kind);
            self.kinds.insert(kind);
            self.vars.push(Var {
                name: name.clone(),
                ty: Ty::Obj(kind),
            });
            entries.push((name, code));
        }
        if self.rng.chance(0.6) {
            let code = self.php_array(2);
            self.vars.push(Var {
                name: "arr".to_owned(),
                ty: Ty::Arr,
            });
            entries.push(("arr".to_owned(), code));
        }
        if self.rng.chance(0.3) {
            let code = self.php_map();
            self.vars.push(Var {
                name: "map".to_owned(),
                ty: Ty::Arr,
            });
            entries.push(("map".to_owned(), code));
        }
        if self.rng.chance(0.4) {
            let code = (*self.rng.pick(SCALARS)).to_owned();
            self.vars.push(Var {
                name: "sc".to_owned(),
                ty: Ty::Any,
            });
            entries.push(("sc".to_owned(), code));
        }
        if self.rng.chance(0.3) {
            let leaf = self.php_element();
            let code = format!("['p' => ['q' => {leaf}, 'r' => ['s' => 'S']], 'a' => 1]");
            self.vars.push(Var {
                name: "nest".to_owned(),
                ty: Ty::Arr,
            });
            entries.push(("nest".to_owned(), code));
        }
        // the variable inc.twig prints when it is included without one
        let iv = if self.rng.chance(0.6) {
            self.php_element()
        } else {
            (*self.rng.pick(SCALARS)).to_owned()
        };
        entries.push(("iv".to_owned(), iv));
        let mut code = String::new();
        for p in &self.preludes {
            code.push_str(p.code());
        }
        code.push_str("return [\n");
        for (name, value) in &entries {
            code.push_str(&format!("    '{name}' => {value},\n"));
        }
        code.push_str("];\n");
        code
    }

    // ---- expressions ------------------------------------------------------------------------------------------

    /// A data variable (or the macro argument / loop variable in scope).
    fn var(&mut self) -> Val {
        if self.vars.is_empty() {
            return Val::new("'x'", Ty::Any);
        }
        let v = self.rng.pick(&self.vars).clone();
        Val::new(v.name, v.ty)
    }

    /// An object-valued expression.
    fn object(&mut self) -> Val {
        let mut objects: Vec<Var> = self
            .vars
            .iter()
            .filter(|v| matches!(v.ty, Ty::Obj(_)))
            .cloned()
            .collect();
        if self.rng.chance(0.5) {
            // most interesting: objects with __toString()
            let stringable: Vec<Var> = objects
                .iter()
                .filter(|v| matches!(v.ty, Ty::Obj(k) if KINDS[k].stringable))
                .cloned()
                .collect();
            if !stringable.is_empty() {
                objects = stringable;
            }
        }
        if objects.is_empty() {
            return self.var();
        }
        let v = self.rng.pick(&objects).clone();
        let Ty::Obj(kind) = v.ty else { unreachable!() };
        let mut val = Val::new(v.name, v.ty);
        let k = &KINDS[kind];
        if !k.chains.is_empty() && self.rng.chance(0.2) {
            let (chain, to) = *self.rng.pick(k.chains);
            let dot = if self.rng.chance(0.1) { "?." } else { "." };
            val = Val::new(format!("{}{dot}{chain}", val.e), Ty::Obj(to));
        }
        val
    }

    /// An attribute name of `kind`, sometimes with its case changed.
    fn attr_name(&mut self, kind: usize) -> String {
        let name = (*self.rng.pick(KINDS[kind].attrs)).to_owned();
        self.vary_case(&name, 0.15)
    }

    /// `name` with its case changed with probability `p`.
    fn vary_case(&mut self, name: &str, p: f64) -> String {
        if !self.rng.chance(p) {
            return name.to_owned();
        }
        match self.rng.below(3) {
            0 => name.to_ascii_lowercase(),
            1 => name.to_ascii_uppercase(),
            _ => {
                let mut out = String::new();
                for (i, c) in name.chars().enumerate() {
                    if i % 2 == 0 {
                        out.push(c.to_ascii_uppercase());
                    } else {
                        out.push(c.to_ascii_lowercase());
                    }
                }
                out
            }
        }
    }

    /// An attribute access on an object (method call, property, array key, ...).
    fn access(&mut self) -> Val {
        let obj = self.object();
        let Ty::Obj(kind) = obj.ty else {
            return Val::new(format!("{}.missing", obj.atom()), Ty::Any);
        };
        let o = obj.atom();
        let name = self.attr_name(kind);
        let quoted = format!("'{name}'");
        let plain = (name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
            && !name.as_bytes()[0].is_ascii_digit())
            || name.bytes().all(|b| b.is_ascii_digit());
        let nl = if self.rng.chance(0.05) {
            "
"
        } else {
            ""
        };
        let e = match self.rng.below(16) {
            0..=3 if plain => format!("{o}{nl}.{name}"),
            0..=3 => format!("{o}[{quoted}]"),
            4 | 5 if self.rng.chance(0.25) => {
                // an object argument: the arguments of a method call are checked for __toString()
                let methods = if KINDS[kind].methods.is_empty() {
                    &["missing"][..]
                } else {
                    KINDS[kind].methods
                };
                let mut method = *self.rng.pick(methods);
                if method == "__construct" {
                    // (re-running a constructor with an object stores it where a string is expected)
                    method = "__toString";
                }
                let arg = self.object();
                match self.rng.below(3) {
                    0 => format!("{o}.{method}({})", arg.e),
                    1 => format!("{o}.{method}(...[{}])", arg.e),
                    _ => {
                        self.function("attribute");
                        format!("attribute({o}, '{method}', [{}])", arg.e)
                    }
                }
            }
            4 | 5 => {
                let call = (*self.rng.pick(KINDS[kind].calls)).to_owned();
                format!("{o}.{}", self.vary_call_case(&call))
            }
            6 => format!("{o}[{quoted}]"),
            7 if plain => format!("{o}?.{name}"),
            7 | 8 => {
                self.function("attribute");
                if self.rng.chance(0.3) {
                    format!("attribute({o}, {quoted}, [])")
                } else {
                    format!("attribute({o}, {quoted})")
                }
            }
            9 if plain => format!("{o}.{name}()"),
            9 => format!("{o}[{quoted}]"),
            10 => {
                let t = self.test("defined");
                let target = self.definable();
                format!("{} is {t} ? 'Y' : 'N'", target.e)
            }
            11 => {
                let target = self.definable();
                format!("{} ?? 'dflt'", target.e)
            }
            12 => {
                let f = self.filter("default");
                let target = if plain {
                    format!("{o}.{name}")
                } else {
                    format!("{o}[{quoted}]")
                };
                format!("{target}|{f}('dflt')")
            }
            13 => {
                // a dynamic key: maybe a stringable object
                let key = if self.rng.chance(0.6) {
                    self.object()
                } else {
                    self.var()
                };
                format!("{o}[{}]", key.e)
            }
            14 if self.rng.chance(0.5) => {
                self.function("attribute");
                let key = self.object();
                format!("attribute({o}, {})", key.e)
            }
            14 => {
                // `.(expr)`: a dynamic attribute name, maybe an object
                let key = match self.rng.below(3) {
                    0 => self.object().e,
                    1 => format!("'{name}'"),
                    _ => format!("'{name}' ~ ''"),
                };
                let call = if self.rng.chance(0.3) { "()" } else { "" };
                format!("{o}.({key}){call}")
            }
            _ if plain => format!("{o}.{name}.{}", self.attr_name(kind)),
            _ => format!("{o}[{quoted}]"),
        };
        Val::new(e, Ty::Any)
    }

    /// An expression the `defined` test accepts (left operand of `??`): a name, an attribute access, an array.
    fn definable(&mut self) -> Val {
        match self.rng.below(5) {
            0 => self.var(),
            4 => {
                // a chain of null-safe accesses: `??` shares it with its null test
                let base = if self.vars.iter().any(|v| v.name == "nest") && self.rng.chance(0.6) {
                    "nest".to_owned()
                } else {
                    self.object().e
                };
                let first = *self.rng.pick(&[
                    "p",
                    "stringable",
                    "getSelf()",
                    "getIterator()",
                    "o",
                    "missing",
                ]);
                let second = *self.rng.pick(&[
                    "q",
                    "r",
                    "v",
                    "position",
                    "count()",
                    "__toString()",
                    "missing",
                    "0",
                ]);
                let third = if self.rng.chance(0.3) { "?.s" } else { "" };
                let dot = if self.rng.chance(0.8) { "?." } else { "." };
                let base = if base.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
                    base
                } else {
                    format!("({base})")
                };
                Val::new(format!("{base}?.{first}{dot}{second}{third}"), Ty::Any)
            }
            1 => {
                let v = self.var();
                Val::new(format!("[{}]", v.e), Ty::Arr)
            }
            _ => {
                let objects: Vec<Var> = self
                    .vars
                    .iter()
                    .filter(|v| matches!(v.ty, Ty::Obj(_)))
                    .cloned()
                    .collect();
                if objects.is_empty() {
                    return self.var();
                }
                let v = self.rng.pick(&objects).clone();
                let Ty::Obj(kind) = v.ty else { unreachable!() };
                let name = self.attr_name(kind);
                let plain = name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                    && !name.as_bytes()[0].is_ascii_digit();
                let chain = KINDS[kind].chains.first().filter(|_| self.rng.chance(0.2));
                let (base, ty) = match chain {
                    Some((c, to)) if !c.contains('(') => (format!("{}.{c}", v.name), Ty::Obj(*to)),
                    _ => (v.name.clone(), Ty::Any),
                };
                if plain {
                    Val::new(format!("{base}.{name}"), ty)
                } else {
                    Val::new(format!("{base}['{name}']"), ty)
                }
            }
        }
    }

    /// A method call snippet with the method name's case varied.
    fn vary_call_case(&mut self, call: &str) -> String {
        match call.find('(') {
            Some(i) if self.rng.chance(0.2) => {
                format!("{}{}", self.vary_case(&call[..i], 1.0), &call[i..])
            }
            _ => call.to_owned(),
        }
    }

    /// Any value: variables, attribute accesses, arrays of objects, conditionals, filter results.
    fn value(&mut self) -> Val {
        if self.depth > 3 {
            return self.var();
        }
        self.depth += 1;
        let v = match self.rng.below(20) {
            0..=4 => self.var(),
            5..=7 => self.object(),
            8..=10 => self.access(),
            11 => {
                let a = self.value();
                let b = self.value();
                Val::new(format!("[{}, {}]", a.e, b.e), Ty::Arr)
            }
            12 => {
                let a = self.value();
                let key = if self.rng.chance(0.5) {
                    "k".to_owned()
                } else {
                    "'k k'".to_owned()
                };
                Val::new(format!("{{{key}: {}}}", a.e), Ty::Arr)
            }
            13 => {
                let a = self.value();
                let b = self.value();
                let ty = if a.ty == b.ty { a.ty } else { Ty::Any };
                Val::new(format!("{} ?: {}", a.atom(), b.atom()), ty)
            }
            14 => {
                let a = self.definable();
                Val::new(format!("{} ?? 'n'", a.atom()), a.ty)
            }
            15 => {
                let a = self.value();
                let b = self.value();
                Val::new(format!("true ? {} : {}", a.atom(), b.atom()), a.ty)
            }
            16 => {
                let a = self.value();
                let name = *self.rng.pick(&["first", "last"]);
                let f = self.filter(name);
                Val::new(format!("[{}]|{f}", a.e), a.ty)
            }
            17 => {
                let a = self.value();
                let f = self.filter("default");
                Val::new(format!("{}|{f}('d')", a.atom()), a.ty)
            }
            18 => {
                let a = self.value();
                let f = self.filter("merge");
                let b = self.value();
                Val::new(format!("[{}]|{f}([{}])", a.e, b.e), Ty::Arr)
            }
            _ => {
                let a = self.value();
                Val::new(format!("({})", a.e), a.ty)
            }
        };
        self.depth -= 1;
        v
    }

    /// Something that converts `v` to a string, or walks it, the way the sandbox visitor wraps it.
    fn coerce(&mut self, v: &Val) -> String {
        let a = v.atom();
        let e = &v.e;
        let w = self.ws();
        match self.rng.below(57) {
            0..=3 => e.clone(),
            4 => format!("{a}{w}~ ''"),
            5 => format!("'<'{w}~ {a} ~ '>'"),
            6 => format!("{a} == 'abc'"),
            7 => format!("{a} != 'foo'"),
            8 => format!("{a} < 'b'"),
            9 => format!("{a} <=> 'abc'"),
            10 => format!("{a} in 'xabcfoox'"),
            11 => format!("'a' in {a}"),
            12 => format!("{a} not in ['foo']"),
            13 => format!("{a} matches '/o/'"),
            14 => format!("{a} starts with 'f'"),
            15 => format!("{a} ends with 'o'"),
            16 => {
                let t = self.test("empty");
                format!("{a} is {t} ? 'E' : 'F'")
            }
            17 => {
                let t = self.test("same as");
                format!("{a} is {t}('foo') ? 'S' : 'D'")
            }
            18 => {
                let name = *self
                    .rng
                    .pick(&["iterable", "null", "sequence", "mapping", "even", "true"]);
                let t = self.test(name);
                format!("{a} is {t} ? 'T' : 'F'")
            }
            19 => {
                let t = self.test("constant");
                format!("{a} is {t}('PHP_EOL') ? 'C' : 'N'")
            }
            20 => {
                let name = *self.rng.pick(&["max", "min"]);
                let f = self.function(name);
                format!("{f}({e}, 'm')")
            }
            21 => {
                let f = self.function("cycle");
                format!("{f}([{e}, 'b'], {})", self.rng.below(3))
            }
            22 => {
                self.function("range");
                let j = self.filter("join");
                if self.rng.chance(0.5) {
                    format!("({a}..'c')|{j}")
                } else {
                    format!("range({e}, 'c')|{j}")
                }
            }
            23 => {
                let f = self.function("date");
                let d = self.filter("date");
                format!("{f}({e})|{d}('Y')")
            }
            24 => {
                let j = self.filter("join");
                format!("[{e}, 1]|{j}('-')")
            }
            25 => {
                let j = self.filter("join");
                format!("{{k: {e}}}|{j}")
            }
            26 => {
                let j = self.filter("join");
                let k = self.filter("keys");
                format!("{{({e}): 1}}|{k}|{j}")
            }
            27 => {
                let j = self.filter("join");
                if self.spreadable(v) {
                    format!("[...{a}]|{j}")
                } else {
                    format!("[1, ...[{e}]]|{j}")
                }
            }
            28 => {
                let name = *self.rng.pick(&["max", "min"]);
                let f = self.function(name);
                if self.spreadable(v) {
                    format!("{f}(...{a})")
                } else {
                    format!("{f}(...[{e}, 'm'])")
                }
            }
            29 => {
                let m = self.filter("map");
                let j = self.filter("join");
                format!("{a}|{m}(v => v ~ '!')|{j}")
            }
            30 => {
                let m = self.filter("filter");
                let j = self.filter("join");
                format!("{a}|{m}(v => v)|{j}(',')")
            }
            31 => {
                let m = self.filter("reduce");
                format!("{a}|{m}((c, v) => c ~ v, '')")
            }
            32 => {
                let m = self.filter("sort");
                let j = self.filter("join");
                if self.rng.chance(0.5) {
                    format!("{a}|{m}|{j}")
                } else {
                    format!("{a}|{m}((x, y) => x <=> y)|{j}")
                }
            }
            33 => {
                let f = self.filter("find");
                format!("{a}|{f}(v => v)")
            }
            34 => {
                let f = self.filter("format");
                format!("'%s|%s'|{f}({e}, 'x')")
            }
            35 => {
                let f = self.filter("replace");
                format!("{a}|{f}({{'o': '0'}})")
            }
            36 => {
                let f = self.filter("join");
                format!("['p', 'q']|{f}({e})")
            }
            37 => {
                let f = self.filter("column");
                let j = self.filter("join");
                let key = self.rng.pick(&["'a'", "'name'", "'v'", "0"]);
                format!("{a}|{f}({key})|{j}")
            }
            38 => {
                let f = self.filter("split");
                let l = self.filter("length");
                format!("{a}|{f}('o')|{l}")
            }
            39 => {
                let f = self.filter("slice");
                format!("{a}|{f}(0, 2)")
            }
            40 => {
                let f = self.filter("abs");
                format!("{a}|{f}")
            }
            41 => {
                let f = self.filter("json_encode");
                format!("{a}|{f}")
            }
            42 => {
                let f = self.filter("escape");
                let s = self
                    .rng
                    .pick(&["'html'", "'js'", "'url'", "'html_attr'", "'css'"]);
                format!("{a}|{f}({s})")
            }
            43 => {
                let f = self.filter("raw");
                format!("{a}|{f}")
            }
            44 => {
                let f = self.filter("batch");
                let l = self.filter("length");
                format!("{a}|{f}(2)|{l}")
            }
            54 => {
                // an Iterator is replaced by the array it was materialised into when it is checked
                let needle = self.rng.pick(&["1", "2", "'bar'", "'foo'", "'a'"]);
                format!("{needle} in {a} ? 'I' : 'O'")
            }
            55 => {
                let other =
                    self.rng
                        .pick(&["{'a': 2}", "[1, 2]", "{'foo': 'bar', 'bar': 'foo'}", "[]"]);
                format!("{a} == {other} ? 'Q' : 'U'")
            }
            56 => {
                let m = self.filter("map");
                let j = self.filter("join");
                let kind = *self.rng.weighted(KIND_WEIGHTS);
                let attr = self.attr_name(kind);
                if attr.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                    && !attr.as_bytes()[0].is_ascii_digit()
                {
                    format!("[{e}]|{m}(v => v.{attr})|{j}")
                } else {
                    format!("[{e}]|{m}(v => v['{attr}'])|{j}")
                }
            }
            46 if self.global => {
                // not a Closure: rejected in sandbox mode
                let (f, callable) = *self.rng.pick(&[
                    ("map", "'upper'"),
                    ("filter", "'trim'"),
                    ("sort", "'strcmp'"),
                    ("reduce", "'max'"),
                    ("find", "'trim'"),
                ]);
                let f = self.filter(f);
                let j = self.filter("join");
                format!("{a}|{f}({callable})|{j}")
            }
            46 | 47 => {
                let op = self.rng.pick(&["has some", "has every"]);
                let body = if self.rng.chance(0.5) {
                    "v"
                } else {
                    "v ~ '' == 'foo'"
                };
                format!("{a} {op} v => {body} ? 'H' : 'N'")
            }
            48 if self.global => {
                let op = self.rng.pick(&["has some", "has every"]);
                format!("{a} {op} 'trim' ? 'H' : 'N'")
            }
            48 | 49 => {
                let f = self.filter("column");
                let j = self.filter("join");
                let key = self.rng.pick(&[
                    "'name'",
                    "'a'",
                    "'stringable'",
                    "'position'",
                    "'value'",
                    "0",
                ]);
                let b = self.object();
                format!("[{e}, {}]|{f}({key})|{j}", b.e)
            }
            50 => {
                let f = self.filter("invoke");
                format!("(x => x ~ '')|{f}({e})")
            }
            51 => {
                let f = self.filter("format");
                format!("{a}|{f}('p')")
            }
            52 => {
                let f = self.filter("merge");
                let j = self.filter("join");
                format!("[{e}]|{f}({a})|{j}")
            }
            53 => {
                // dump() of scalars only: object handles differ by design
                let f = self.function("dump");
                self.debug_ext = true;
                format!("{f}('d', 1) ~ {a}")
            }
            _ => {
                let f = *self.rng.pick(&[
                    "upper",
                    "lower",
                    "title",
                    "capitalize",
                    "trim",
                    "striptags",
                    "nl2br",
                    "url_encode",
                    "length",
                    "reverse",
                    "first",
                    "last",
                    "keys",
                    "number_format",
                    "round",
                    "e",
                    "spaceless",
                    "convert_encoding",
                ]);
                let f = self.filter(f);
                let p = self.pipe();
                if f == "convert_encoding" {
                    format!("{a}{p}{f}('UTF-8', 'ISO-8859-1')")
                } else if f == "keys" {
                    let j = self.filter("join");
                    format!("{a}{p}{f}|{j}")
                } else {
                    format!("{a}{p}{f}")
                }
            }
        }
    }

    /// Whether to spread `v` itself (an array or a `Traversable`, or now and then anything, for the `TypeError`).
    fn spreadable(&mut self, v: &Val) -> bool {
        match v.ty {
            Ty::Obj(kind) => KINDS[kind].traversable || self.rng.chance(0.2),
            Ty::Arr => true,
            Ty::Any => self.rng.chance(0.5),
        }
    }

    /// `|` with a newline before or after it now and then (the filter's line is its name's line).
    fn pipe(&mut self) -> &'static str {
        match self.rng.below(40) {
            0 => {
                "
|"
            }
            1 => {
                "|
"
            }
            _ => "|",
        }
    }

    /// A printable expression.
    fn expr(&mut self) -> String {
        match self.rng.below(10) {
            0..=1 => self.access().e,
            2..=7 => {
                let v = self.value();
                self.coerce(&v)
            }
            8 => {
                let v = self.access();
                self.coerce(&v)
            }
            _ => {
                let v = self.object();
                self.coerce(&v)
            }
        }
    }

    /// A template name expression for `include()` and friends: mostly a string, sometimes an object.
    fn template_name(&mut self, name: &str) -> String {
        if self.rng.chance(0.2) {
            let v = self.object();
            return v.e;
        }
        if self.rng.chance(0.1) {
            let f = self.filter("lower");
            return format!("'{name}'|{f}");
        }
        format!("'{name}'")
    }

    // ---- statements -------------------------------------------------------------------------------------------

    /// A statement (one "unit" of a template body).
    fn unit(&mut self, level: usize) -> String {
        let w = self.ws();
        let roll = if level > 1 {
            self.rng.below(8)
        } else {
            self.rng.below(36)
        };
        match roll {
            0..=5 => format!("{{{{ {} }}}}", self.expr()),
            6 => {
                let t = self.tag("set");
                let v = self.value();
                format!("{{% {t} v1 = {} %}}{{{{{w}v1 }}}}", v.e)
            }
            7 => {
                let t = self.tag("if");
                let cond = self.expr();
                let body = self.unit(level + 1);
                format!("{{% {t} {cond} %}}{body}{{% else %}}-{{% endif %}}")
            }
            8 | 9 => {
                let t = self.tag("for");
                let v = self.value();
                if self.rng.chance(0.5) {
                    format!("{{% {t} x in {} %}}{{{{ x{w}}}}},{{% endfor %}}", v.atom())
                } else {
                    let c = self.coerce(&Val::new("x", Ty::Any));
                    format!(
                        "{{% {t} k, x in {} %}}{{{{ k }}}}={{{{ {c} }}}};{{% endfor %}}",
                        v.atom()
                    )
                }
            }
            10 => {
                let t = self.tag("apply");
                let name = *self
                    .rng
                    .pick(&["upper", "lower", "escape", "trim", "title"]);
                let f = self.filter(name);
                let body = self.unit(level + 1);
                format!("{{% {t} {f} %}}{body}{{% endapply %}}")
            }
            11 => {
                let t = self.tag("autoescape");
                let strategy = self.rng.pick(&["'html'", "'js'", "false", "'url'"]);
                let body = self.unit(level + 1);
                format!("{{% {t} {strategy} %}}{body}{{% endautoescape %}}")
            }
            12 => {
                let t = self.tag("with");
                let v = self.value();
                let only = if self.rng.chance(0.3) { " only" } else { "" };
                format!(
                    "{{% {t} {{w1: {}}}{only} %}}{{{{ w1 }}}}{{% endwith %}}",
                    v.e
                )
            }
            13 => {
                let t = self.tag("do");
                format!("{{% {t} {} %}}", self.expr())
            }
            14 => {
                let t = self.tag("set");
                let body = self.unit(level + 1);
                format!("{{% {t} cap %}}{body}{{% endset %}}{{{{ cap }}}}")
            }
            15 => {
                let t = self.tag("deprecated");
                let v = self.value();
                let msg = if self.rng.chance(0.5) {
                    v.e
                } else {
                    "'dep'".to_owned()
                };
                format!("{{% {t} {msg} %}}")
            }
            16 if !self.in_macro => {
                let t = self.tag("block");
                let body = self.unit(level + 1);
                self.blocks += 1;
                format!("{{% {t} u{} %}}{body}{{% endblock %}}", self.blocks)
            }
            17..=19 if self.in_include_target => format!("{{{{ {} }}}}", self.expr()),
            17 => {
                let f = self.function("include");
                let target = self.template_name("inc.twig");
                self.ensure_include_target();
                let args = match self.rng.below(7) {
                    0 => String::new(),
                    1 => format!(", {{iv: {}}}", self.value().e),
                    2 => format!(", {{iv: {}}}, false", self.value().e),
                    3 => ", {}, true, true".to_owned(),
                    4 => ", sandboxed = false".to_owned(),
                    5 => {
                        let f = self.function("include_only");
                        return format!("{{{{ {f}({target}, {{iv: {}}}) }}}}", self.value().e);
                    }
                    6 if self.rng.chance(0.5) => {
                        // the variables argument is checked (an Iterator becomes an array)
                        format!(", {}", self.object().e)
                    }
                    _ => format!(", {{iv: {}}}, with_context = false", self.value().e),
                };
                format!("{{{{ {f}({target}{args}) }}}}")
            }
            18 => {
                let t = self.tag("include");
                let target = self.template_name("inc.twig");
                self.ensure_include_target();
                let tail = match self.rng.below(6) {
                    0 => String::new(),
                    1 => format!(" with {{iv: {}}}", self.value().e),
                    2 => format!(" with {{iv: {}}} only", self.value().e),
                    3 => " ignore missing".to_owned(),
                    4 => format!(" with {}", self.object().e),
                    _ => " only".to_owned(),
                };
                format!("{{% {t} {target}{tail} %}}")
            }
            19 => {
                let f = self.function("source");
                let target = self.template_name("inc.twig");
                self.ensure_include_target();
                format!("{{{{ {f}({target}) }}}}")
            }
            20 => {
                let t = self.tag("guard");
                let what = self.rng.pick(&[
                    "filter upper",
                    "function cycle",
                    "test constant",
                    "filter nope",
                ]);
                let body = self.unit(level + 1);
                format!("{{% {t} {what} %}}{body}{{% else %}}g{{% endguard %}}")
            }
            21 => {
                let t = self.tag("for");
                // `..` is the `range` function for the sandbox
                self.function("range");
                let body = self.unit(level + 1);
                format!("{{% {t} i in 1..2 %}}{body}{{% endfor %}}")
            }
            22 => {
                let body = self.unit(level + 1);
                format!("{{% line {} %}}{body}", self.rng.pick(&[0, 1, 7, 40]))
            }
            23 if !self.in_include_target => {
                // block() of another template: loads it (and checks its security) when called
                let f = self.function("block");
                self.defer("blk.twig");
                let target = self.template_name("blk.twig");
                if self.rng.chance(0.25) {
                    let t = self.test("defined");
                    format!("{{{{ {f}('x1', {target}) is {t} ? 'D' : 'U' }}}}")
                } else {
                    format!("{{{{ {f}('x1', {target}) }}}}")
                }
            }
            24 => {
                // object destructuring reads each property through getAttribute()
                let obj = self.object();
                let Ty::Obj(kind) = obj.ty else {
                    return format!("{{% do {{a: d1}} = {} %}}{{{{ d1 }}}}", obj.atom());
                };
                let names: Vec<&str> = KINDS[kind]
                    .attrs
                    .iter()
                    .copied()
                    .filter(|n| {
                        n.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                            && !n.as_bytes()[0].is_ascii_digit()
                    })
                    .collect();
                let first = if names.is_empty() {
                    "missing"
                } else {
                    *self.rng.pick(&names)
                };
                let t = self.tag("do");
                // operator names (`not`, `is`, `in`) cannot be variables: no shorthand for them
                let operator = matches!(first, "not" | "is" | "in" | "and" | "or" | "matches");
                if operator || self.rng.chance(0.5) {
                    let second = if names.is_empty() {
                        "a"
                    } else {
                        *self.rng.pick(&names)
                    };
                    format!(
                        "{{% {t} {{{first}: d1, {second}: d2}} = {} %}}{{{{ d1 }}}}|{{{{ d2 }}}}",
                        obj.atom()
                    )
                } else {
                    format!("{{% {t} {{{first}}} = {} %}}{{{{ {first} }}}}", obj.atom())
                }
            }
            25 => {
                let t = self.tag("do");
                let v = self.value();
                format!(
                    "{{% {t} [s1, s2] = {} %}}{{{{ s1 }}}}{{{{ s2 ?? '' }}}}",
                    v.atom()
                )
            }
            26 => {
                // an assignment expression is wrapped as a whole
                // (not as a call argument: `f((a1 = x))` is the named argument `a1`)
                let v = self.value();
                match self.rng.below(3) {
                    0 => format!("{{{{ (a1 = {}) }}}}", v.e),
                    1 => format!("{{{{ (a1 = {}) ~ '' }}}}", v.e),
                    _ => format!("{{{{ (a1 = {}) == 'foo' ? 'Q' : 'U' }}}}", v.e),
                }
            }
            27 => {
                // the whole context (data, globals) is walked for __toString()
                let f = *self.rng.pick(&["keys", "length", "json_encode", "first"]);
                let f = self.filter(f);
                if f == "keys" {
                    let j = self.filter("join");
                    format!("{{{{ _context|{f}|{j}(',') }}}}")
                } else if f == "json_encode" {
                    let l = self.filter("length");
                    format!("{{{{ _context|{f}|{l} }}}}")
                } else {
                    format!("{{{{ _context|{f} }}}}")
                }
            }
            28 => {
                let t = self.tag("for");
                let f = self.filter("length");
                format!(
                    "{{% {t} x in [1] %}}{{{{ loop|{f} }}}}{{{{ loop.parent|{f} }}}}{{% endfor %}}"
                )
            }
            29 => {
                // trigger_deprecation(string $package, string $version, ...): mostly strings and stringables (other
                // values are a known mismatch, rarely generated)
                let t = self.tag("deprecated");
                let (a, b) = if self.rng.chance(0.02) {
                    (self.value().atom(), self.value().atom())
                } else {
                    (self.deprecation_arg(), self.deprecation_arg())
                };
                format!("{{% {t} 'dep' package={a} version={b} %}}")
            }
            30 if !self.in_include_target => {
                let f = self.function("include");
                self.ensure_include_target();
                let first = if self.rng.chance(0.5) {
                    self.object().e
                } else {
                    "'missing.twig'".to_owned()
                };
                format!("{{{{ {f}([{first}, 'inc.twig']) }}}}")
            }
            32 => {
                // Markup values made by templates: their methods are always allowed, but `withContent()`
                let t = self.tag("set");
                let body = self.unit(level + 1);
                let call = *self.rng.pick(&[
                    "count",
                    "getCharset()",
                    "withContent('q')",
                    "__toString()",
                    "charset",
                    "content",
                ]);
                format!("{{% {t} cap2 %}}{body}{{% endset %}}{{{{ cap2.{call} }}}}")
            }
            33 if !self.in_include_target => {
                let f = self.function("include");
                self.ensure_include_target();
                let call = *self.rng.pick(&[
                    "count",
                    "getCharset()",
                    "withContent('q')",
                    "jsonSerialize()",
                ]);
                format!("{{{{ {f}('inc.twig').{call} }}}}")
            }
            31 => {
                let t = self.tag("apply");
                let a = *self
                    .rng
                    .pick(&["upper", "lower", "trim", "escape", "title", "striptags"]);
                let b = *self
                    .rng
                    .pick(&["upper", "lower", "trim", "escape", "nl2br", "spaceless"]);
                let a = self.filter(a);
                let b = self.filter(b);
                let body = self.unit(level + 1);
                format!("{{%{w}{t} {a}|{b} %}}{body}{{% endapply %}}")
            }
            _ => format!("{{{{ {} }}}}", self.expr()),
        }
    }

    /// A `package` or `version` of `{% deprecated %}`: a string or an object.
    fn deprecation_arg(&mut self) -> String {
        let stringable: Vec<Var> = self
            .vars
            .iter()
            .filter(|v| matches!(v.ty, Ty::Obj(k) if KINDS[k].stringable))
            .cloned()
            .collect();
        match self.rng.below(3) {
            2 if !stringable.is_empty() => self.rng.pick(&stringable).name.clone(),
            0 | 2 => "'twig/x'".to_owned(),
            _ => format!("'{}'", self.rng.below(9)),
        }
    }

    /// Several units.
    fn body(&mut self, min: usize, max: usize) -> String {
        let n = min + self.rng.below(max - min + 1);
        let mut out = String::new();
        for i in 0..n {
            if i > 0 {
                out.push_str(self.sep());
            }
            let unit = self.unit(0);
            out.push_str(&unit);
        }
        out
    }

    /// Adds `inc.twig` (the include target) once.
    fn ensure_include_target(&mut self) {
        self.defer("inc.twig");
    }

    /// Adds a template whose body is generated at the end (placeholder until then), once.
    fn defer(&mut self, name: &str) {
        if !self.extra.iter().any(|(n, _)| n == name) {
            self.extra.push((name.to_owned(), String::new()));
        }
    }

    /// The templates of a shape: (main template, extra templates).
    fn templates(&mut self, shape: Shape) -> String {
        match shape {
            Shape::Plain => self.body(1, 4),
            Shape::Include => {
                self.ensure_include_target();
                let mut out = self.body(0, 2);
                out.push_str(self.sep());
                let target = self.template_name("inc.twig");
                if self.rng.chance(0.5) {
                    let t = self.tag("include");
                    out.push_str(&format!("{{% {t} {target} %}}"));
                } else {
                    let f = self.function("include");
                    out.push_str(&format!("{{{{ {f}({target}) }}}}"));
                }
                out.push_str(self.sep());
                out.push_str(&self.body(0, 2));
                out
            }
            Shape::Extends => {
                let t = self.tag("extends");
                let parent = self.template_name("base.twig");
                let mut out = format!("{{% {t} {parent} %}}");
                let blocks = 1 + self.rng.below(2);
                for i in 0..blocks {
                    out.push_str(self.sep());
                    let b = self.tag("block");
                    let mut body = self.body(1, 2);
                    if self.rng.chance(0.3) {
                        let f = self.function("parent");
                        body.push_str(&format!("{{{{ {f}() }}}}"));
                    }
                    if i == 1 && self.rng.chance(0.3) {
                        // only `b2` renders `b1`: blocks calling each other would recurse forever
                        let f = self.function("block");
                        body.push_str(&format!("{{{{ {f}('b1') }}}}"));
                    }
                    out.push_str(&format!("{{% {b} b{} %}}{body}{{% endblock %}}", i + 1));
                }
                let mut base = String::new();
                base.push_str(&self.body(0, 1));
                for i in 0..2 {
                    let b = self.tag("block");
                    let body = self.body(0, 1);
                    base.push_str(&format!("[{{% {b} b{} %}}{body}{{% endblock %}}]", i + 1));
                }
                self.extra.push(("base.twig".to_owned(), base));
                out
            }
            Shape::Embed => {
                let t = self.tag("embed");
                let target = self.template_name("embed.twig");
                let b = self.tag("block");
                let inner = self.body(1, 2);
                let mut out = self.body(0, 1);
                out.push_str(self.sep());
                out.push_str(&format!(
                    "{{% {t} {target} %}}{{% {b} e1 %}}{inner}{{% endblock %}}{{% endembed %}}"
                ));
                let eb = self.tag("block");
                let ebody = self.body(0, 1);
                let base_body = self.body(0, 1);
                self.extra.push((
                    "embed.twig".to_owned(),
                    format!("<{{% {eb} e1 %}}{ebody}{{% endblock %}}{base_body}>"),
                ));
                out
            }
            Shape::Use => {
                let t = self.tag("use");
                let f = self.function("block");
                let mut out = format!("{{% {t} 'traits.twig' %}}");
                out.push_str(self.sep());
                out.push_str(&self.body(0, 2));
                out.push_str(&format!("{{{{ {f}('t1') }}}}"));
                let b = self.tag("block");
                let tbody = self.body(1, 2);
                self.extra.push((
                    "traits.twig".to_owned(),
                    format!("{{% {b} t1 %}}{tbody}{{% endblock %}}"),
                ));
                out
            }
            Shape::Macros => {
                let import_from = self.rng.chance(0.5);
                let target = self.template_name("macros.twig");
                let mut out = if import_from {
                    let t = self.tag("from");
                    format!("{{% {t} {target} import mac %}}")
                } else {
                    let t = self.tag("import");
                    format!("{{% {t} {target} as m %}}")
                };
                out.push_str(self.sep());
                out.push_str(&self.body(0, 1));
                let arg = self.value().e;
                let call = if import_from {
                    format!("mac({arg})")
                } else {
                    match self.rng.below(5) {
                        // macro names are case-insensitive, with a deprecation
                        0 => format!("m.MAC({arg})"),
                        // a dynamic name is coerced to a string: an object goes through the sandbox
                        1 => {
                            let name = self.object();
                            format!("m.({})({arg})", name.e)
                        }
                        2 => format!("m.('ma' ~ 'c')({arg})"),
                        _ => format!("m.mac({arg})"),
                    }
                };
                out.push_str(&format!("{{{{ {call} }}}}"));
                // the macro body sees only its argument
                let saved = std::mem::replace(
                    &mut self.vars,
                    vec![Var {
                        name: "a".to_owned(),
                        ty: Ty::Any,
                    }],
                );
                let mt = self.tag("macro");
                self.in_macro = true;
                let mbody = self.body(1, 2);
                self.in_macro = false;
                self.vars = saved;
                // `foo` is what MyObj and TwigTestStringy('foo') convert to
                self.extra.push((
                    "macros.twig".to_owned(),
                    format!("{{% {mt} mac(a) %}}{mbody}{{% endmacro %}}{{% {mt} foo(a) %}}F{{{{ a ?? '' }}}}{{% endmacro %}}"),
                ));
                out
            }
            Shape::SandboxTag => {
                self.ensure_include_target();
                let mut out = self.body(0, 1);
                out.push_str(self.sep());
                if self.rng.chance(0.5) {
                    let t = self.tag("sandbox");
                    let i = self.tag("include");
                    let target = self.template_name("inc.twig");
                    let tail = if self.rng.chance(0.3) {
                        " ignore missing"
                    } else {
                        ""
                    };
                    out.push_str(&format!(
                        "{{% {t} %}}{{% {i} {target}{tail} %}}{{% endsandbox %}}"
                    ));
                } else {
                    let f = self.function("include");
                    let target = self.template_name("inc.twig");
                    let args = if self.rng.chance(0.5) {
                        ", sandboxed = true".to_owned()
                    } else {
                        ", {}, true, false, true".to_owned()
                    };
                    out.push_str(&format!("{{{{ {f}({target}{args}) }}}}"));
                }
                out.push_str(self.sep());
                out.push_str(&self.body(0, 2));
                out
            }
            Shape::SelfMacro => {
                let mut out = self.body(0, 2);
                out.push_str(self.sep());
                let arg = self.value().e;
                if self.rng.chance(0.2) {
                    let name = self.object();
                    out.push_str(&format!("{{{{ _self.({})({arg}) }}}}", name.e));
                } else if self.rng.chance(0.3) {
                    let extra = self.object().e;
                    out.push_str(&format!("{{{{ _self.sm({arg}, {extra}) }}}}"));
                } else if self.rng.chance(0.3) {
                    out.push_str(&format!("{{{{ _self.sm(a = {arg}) }}}}"));
                } else if self.rng.chance(0.5) {
                    out.push_str(&format!("{{{{ _self.sm({arg}) }}}}"));
                } else {
                    let t = self.tag("import");
                    out.push_str(&format!("{{% {t} _self as me %}}{{{{ me.sm({arg}) }}}}"));
                }
                let saved = std::mem::replace(
                    &mut self.vars,
                    vec![Var {
                        name: "a".to_owned(),
                        ty: Ty::Any,
                    }],
                );
                let mt = self.tag("macro");
                self.in_macro = true;
                let mut mbody = self.body(1, 2);
                if self.rng.chance(0.3) {
                    let j = self.filter("join");
                    mbody.push_str(&format!("{{{{ varargs|{j}(',') }}}}"));
                }
                self.in_macro = false;
                self.vars = saved;
                out.push_str(self.sep());
                out.push_str(&format!(
                    "{{% {mt} sm(a) %}}{mbody}{{% endmacro %}}{{% {mt} foo(a) %}}F{{% endmacro %}}"
                ));
                out
            }
            Shape::StringLoader => {
                self.string_loader = true;
                let f = self.function("template_from_string");
                let src = if self.rng.chance(0.3) {
                    self.object().e
                } else {
                    let inner = self.rng.pick(&[
                        "'{{ 1 }}'",
                        "'{{ x|upper }}'",
                        "'{% if 1 %}i{% endif %}'",
                    ]);
                    (*inner).to_owned()
                };
                let mut out = self.body(0, 1);
                if self.rng.chance(0.5) {
                    let i = self.function("include");
                    out.push_str(&format!("{{{{ {i}({f}({src}), {{x: 'y'}}) }}}}"));
                } else {
                    out.push_str(&format!("{{{{ {f}({src}).render({{x: 'y'}}) }}}}"));
                }
                out
            }
        }
    }

    // ---- the policy -------------------------------------------------------------------------------------------

    /// An allow-list from the used names.
    fn allow_list(&mut self, used: &BTreeSet<String>, pool: &[&str], keep: f64) -> Json {
        let mut out: Vec<String> = Vec::new();
        for name in used {
            if self.rng.chance(keep) {
                out.push(name.clone());
            } else if self.rng.chance(0.3) {
                // a case variant never matches
                let v = self.vary_case(name, 1.0);
                out.push(v);
            }
        }
        let decoys = self.rng.below(3);
        for _ in 0..decoys {
            let d = (*self.rng.pick(pool)).to_owned();
            if !out.contains(&d) {
                out.push(d);
            }
        }
        // order does not matter to in_array(), but shuffle anyway
        for i in (1..out.len()).rev() {
            let j = self.rng.below(i + 1);
            out.swap(i, j);
        }
        Json::Array(out.into_iter().map(Json::str).collect())
    }

    /// A class key for the policy: the class, a parent or interface, a case or backslash variant, a decoy.
    fn class_key(&mut self, kind: usize) -> String {
        let k = &KINDS[kind];
        let roll = self.rng.below(100);
        if roll < 45 || (k.isa.is_empty() && roll < 75) {
            k.class.to_owned()
        } else if roll < 75 {
            (*self.rng.pick(k.isa)).to_owned()
        } else if roll < 83 {
            self.vary_case(k.class, 1.0)
        } else if roll < 91 {
            format!("\\{}", k.class)
        } else if roll < 94 {
            format!("{} ", k.class)
        } else if roll < 97 {
            (*self.rng.pick(&[
                "Twig\\Template",
                "Exception",
                "Closure",
                "Iterator",
                "Stringable",
                "",
            ]))
            .to_owned()
        } else {
            format!("\\\\{}", k.class)
        }
    }

    /// The methods of a policy entry for `kind`.
    fn method_members(&mut self, kind: usize, all: bool) -> Json {
        let k = &KINDS[kind];
        let mut out: Vec<String> = Vec::new();
        for m in k.methods {
            if all || self.rng.chance(0.55) {
                out.push(self.vary_case(m, 0.25));
            }
        }
        if k.stringable && (all || self.rng.chance(0.6)) {
            let v = (*self
                .rng
                .pick(&["__toString", "__tostring", "__TOSTRING", "__toSTRING"]))
            .to_owned();
            out.push(v);
        }
        if self.rng.chance(0.25) {
            // names that resolve through get/is/has or __call are not method names
            out.push((*self.rng.pick(k.attrs)).to_owned());
        }
        if out.len() == 1 && self.rng.chance(0.4) {
            return Json::str(out.remove(0));
        }
        Json::Array(out.into_iter().map(Json::str).collect())
    }

    /// The properties of a policy entry for `kind`.
    fn property_members(&mut self, kind: usize, all: bool) -> Json {
        let k = &KINDS[kind];
        let mut out: Vec<String> = Vec::new();
        for p in k.props {
            if all || self.rng.chance(0.5) {
                out.push(self.vary_case(p, 0.15));
            }
        }
        if self.rng.chance(0.2) {
            out.push((*self.rng.pick(k.attrs)).to_owned());
        }
        if out.len() == 1 && self.rng.chance(0.4) {
            return Json::str(out.remove(0));
        }
        Json::Array(out.into_iter().map(Json::str).collect())
    }

    /// The security policy.
    fn policy(&mut self) -> Json {
        let permissive = self.rng.chance(0.6);
        let keep = if permissive { 1.0 } else { 0.75 };
        let mut tags_used = std::mem::take(&mut self.used.tags);
        let mut filters_used = std::mem::take(&mut self.used.filters);
        let functions_used = std::mem::take(&mut self.used.functions);
        let tests_used = std::mem::take(&mut self.used.tests);
        if self.autoescape && (permissive || self.rng.chance(0.8)) {
            filters_used.insert("escape".to_owned());
        }
        if self.rng.chance(0.05) {
            tags_used.insert("verbatim".to_owned());
        }
        let tags = self.allow_list(
            &tags_used,
            &[
                "if",
                "for",
                "set",
                "IF",
                "spaceless",
                "verbatim",
                "filter",
                "macro",
                "sandbox",
            ],
            keep,
        );
        let filters = self.allow_list(
            &filters_used,
            &[
                "upper", "escape", "raw", "Upper", "e", "join", "date", "length", "default",
            ],
            keep,
        );
        let functions = self.allow_list(
            &functions_used,
            &[
                "include",
                "range",
                "block",
                "parent",
                "attribute",
                "source",
                "dump",
                "cycle",
                "Include",
            ],
            keep,
        );
        let tests = self.allow_list(
            &tests_used,
            &[
                "defined",
                "constant",
                "empty",
                "same as",
                "iterable",
                "Constant",
                "divisible by",
            ],
            if permissive { 1.0 } else { 0.6 },
        );

        // methods and properties
        let mut methods: Vec<(String, Json)> = Vec::new();
        let mut properties: Vec<(String, Json)> = Vec::new();
        let mut kinds: Vec<usize> = self.kinds.iter().copied().collect();
        for kind in self.element_kinds.clone() {
            if !kinds.contains(&kind) && self.rng.chance(0.5) {
                kinds.push(kind);
            }
        }
        let all_members = permissive && self.rng.chance(0.6);
        for kind in kinds {
            let entries = if all_members { 1 } else { self.rng.below(3) };
            for _ in 0..entries {
                let key = if all_members {
                    KINDS[kind].class.to_owned()
                } else {
                    self.class_key(kind)
                };
                let members = self.method_members(kind, all_members);
                if let Some(entry) = methods.iter_mut().find(|(k, _)| *k == key) {
                    entry.1 = members;
                } else {
                    methods.push((key, members));
                }
            }
            let entries = if all_members { 1 } else { self.rng.below(3) };
            for _ in 0..entries {
                let key = if all_members {
                    KINDS[kind].class.to_owned()
                } else {
                    self.class_key(kind)
                };
                let members = self.property_members(kind, all_members);
                if let Some(entry) = properties.iter_mut().find(|(k, _)| *k == key) {
                    entry.1 = members;
                } else {
                    properties.push((key, members));
                }
            }
        }
        if self.rng.chance(0.5) {
            // `Stringable` covers __toString() of every object that has one
            let key = (*self
                .rng
                .pick(&["Stringable", "stringable", "\\Stringable", "STRINGABLE"]))
            .to_owned();
            let member = self.rng.pick(&["__toString", "__tostring", "__TOSTRING"]);
            if !methods.iter().any(|(k, _)| *k == key) {
                methods.push((key, Json::Array(vec![Json::str(*member)])));
            }
        }
        if self.rng.chance(0.003) {
            // a numeric class key is an int key in PHP: `$obj instanceof 0` throws
            methods.insert(0, ("0".to_owned(), Json::str("count")));
        }
        if self.rng.chance(0.003) {
            properties.insert(0, ("1".to_owned(), Json::str("a")));
        }
        let mut policy = vec![
            ("tags".to_owned(), tags),
            ("filters".to_owned(), filters),
            ("methods".to_owned(), Json::Object(methods)),
            ("properties".to_owned(), Json::Object(properties)),
            ("functions".to_owned(), functions),
            ("tests".to_owned(), tests),
        ];
        if self.rng.chance(0.45) {
            policy.push(("strict".to_owned(), Json::Bool(true)));
        }
        Json::Object(policy)
    }

    // ---- the case ---------------------------------------------------------------------------------------------

    fn case(mut self, id: String) -> Json {
        let data = self.data();
        self.autoescape = self.rng.chance(0.35);
        self.newline_bias = *self.rng.pick(&[0.0, 0.2, 0.5]);
        self.global = self.rng.chance(0.75);
        self.debug_ext = self.rng.chance(0.15);

        let shape = if self.global {
            *self.rng.weighted(&[
                (8, Shape::Plain),
                (3, Shape::Include),
                (2, Shape::Extends),
                (1, Shape::Embed),
                (1, Shape::Use),
                (2, Shape::Macros),
                (1, Shape::SandboxTag),
                (1, Shape::StringLoader),
                (2, Shape::SelfMacro),
            ])
        } else {
            *self.rng.weighted(&[
                (5, Shape::SandboxTag),
                (1, Shape::Plain),
                (1, Shape::Include),
            ])
        };
        let main = self.templates(shape);
        // fill the include target with a body of its own
        self.in_include_target = true;
        if let Some(i) = self
            .extra
            .iter()
            .position(|(n, s)| n == "inc.twig" && s.is_empty())
        {
            let variant = self.rng.below(10);
            let body = if variant == 0 {
                // `use`: checked by the constructor when the template is loaded sandboxed
                // (templates of their own: the ones of the main template may include inc.twig)
                let u = self.tag("use");
                let f = self.function("block");
                let b = self.tag("block");
                let tbody = self.body(1, 1);
                self.extra.push((
                    "itraits.twig".to_owned(),
                    format!("{{% {b} it1 %}}{tbody}{{% endblock %}}"),
                ));
                let rest = self.body(0, 1);
                self.extra[i].1 = format!("{{% {u} 'itraits.twig' %}}<{rest}{{{{ {f}('it1') }}}}>");
                String::new()
            } else if variant == 1 {
                let e = self.tag("extends");
                let b = self.tag("block");
                let bbody = self.body(1, 1);
                let base_body = self.body(0, 1);
                self.extra.push((
                    "ibase.twig".to_owned(),
                    format!("[{{% {b} ib1 %}}{base_body}{{% endblock %}}]"),
                ));
                self.extra[i].1 =
                    format!("{{% {e} 'ibase.twig' %}}{{% {b} ib1 %}}{bbody}{{% endblock %}}");
                String::new()
            } else if self.rng.chance(0.3) {
                let iv = Val::new("iv", Ty::Any);
                let c = self.coerce(&iv);
                format!("[{{{{ {c} }}}}]")
            } else {
                self.body(1, 2)
            };
            if self.extra[i].1.is_empty() {
                self.extra[i].1 = format!("<{body}>");
            }
        }
        if let Some(i) = self
            .extra
            .iter()
            .position(|(n, s)| n == "blk.twig" && s.is_empty())
        {
            let b = self.tag("block");
            let body = self.body(1, 2);
            let outside = if self.rng.chance(0.3) {
                self.body(1, 1)
            } else {
                String::new()
            };
            self.extra[i].1 = format!("{outside}{{% {b} x1 %}}{body}{{% endblock %}}");
        }
        self.in_include_target = false;
        if self.kinds.contains(&25) {
            let b = self.tag("block");
            self.extra.push((
                "tw.twig".to_owned(),
                format!("T{{% {b} tb %}}B{{{{ x ?? '' }}}}{{% endblock %}}"),
            ));
        }

        let mut templates = vec![("index.twig".to_owned(), Json::str(main))];
        for (name, source) in std::mem::take(&mut self.extra) {
            if !templates.iter().any(|(n, _)| *n == name) {
                templates.push((name, Json::str(source)));
            }
        }

        let mut extensions = vec![Json::str("sandbox")];
        if self.debug_ext || self.used.functions.contains("dump") {
            extensions.push(Json::str("debug"));
        }
        if self.string_loader || self.rng.chance(0.1) {
            extensions.push(Json::str("string_loader"));
        }

        let mut config = vec![(
            "autoescape".to_owned(),
            if self.autoescape {
                Json::str(*self.rng.pick(&["html", "html", "js", "name"]))
            } else {
                Json::Bool(false)
            },
        )];
        if self.rng.chance(0.5) {
            config.push(("strict_variables".to_owned(), Json::Bool(true)));
        }
        if self.rng.chance(0.15) {
            config.push(("optimizations".to_owned(), Json::int(0)));
        }
        if self.rng.chance(0.1) {
            config.push(("debug".to_owned(), Json::Bool(true)));
        }

        let policy = self.policy();
        let mut fields = vec![
            ("id".to_owned(), Json::str(id)),
            ("env".to_owned(), Json::str("plain")),
            ("templates".to_owned(), Json::Object(templates)),
            ("data_php".to_owned(), Json::str(data)),
            ("test_classes".to_owned(), Json::Bool(true)),
            ("config".to_owned(), Json::Object(config)),
            ("extensions".to_owned(), Json::Array(extensions)),
            (
                "sandbox".to_owned(),
                Json::Object(vec![
                    ("global".to_owned(), Json::Bool(self.global)),
                    ("policy".to_owned(), policy),
                ]),
            ),
        ];
        if self.rng.chance(0.2) {
            fields.push((
                "globals".to_owned(),
                Json::Object(vec![
                    ("g".to_owned(), Json::str("gv")),
                    (
                        "garr".to_owned(),
                        Json::Array(vec![Json::int(1), Json::str("x")]),
                    ),
                ]),
            ));
        }
        if self.rng.chance(0.3) {
            fields.push(("load_all".to_owned(), Json::Bool(false)));
        }
        if self.rng.chance(0.08) {
            fields.push(("errors".to_owned(), Json::str("throw")));
        }
        Json::Object(fields)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_deterministic() {
        assert_eq!(generate(7, 20), generate(7, 20));
        assert_ne!(case(7, 1), case(7, 2));
    }

    #[test]
    fn cases_are_plain_sandbox_cases() {
        for c in generate(1, 200) {
            assert_eq!(c.get("env").and_then(Json::as_str), Some("plain"));
            let templates = c.get("templates").map(Json::entries).unwrap_or_default();
            assert!(templates.iter().any(|(n, _)| n == "index.twig"));
            let data = c.get("data_php").and_then(Json::as_str).unwrap_or_default();
            assert!(data.contains("return ["));
            assert!(c.get("sandbox").and_then(|s| s.get("policy")).is_some());
        }
    }
}
