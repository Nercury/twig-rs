//! Seeded case generator for the "errors, lines and deprecations" family.
//!
//! Every case places one *fault* at a random position of a set of multi-line templates and checks
//! what Twig reports for it: the error class and exact message, the template name and line, the
//! previous exception, and the deprecations and PHP diagnostics with their order.
//!
//! The path from the main template (`index.twig`) to the fault takes up to three random *hops*:
//! `include` (tag, `include()`, `include_only()`, `with`/`only`/`ignore missing`), `embed` (fault in
//! an overriding block or in the embedded template), inline blocks, child templates (fault in a
//! child block, in a parent block reached through `parent()` or not overridden, in the parent's
//! body, in a grandparent, in the `extends` expression, at the child's root), macros (imported with
//! `import`/`from`, `_self`, nested), traits (`use` with aliases), `block()` of another template,
//! `template_from_string()`, and control structures (`for`, `if`, `with`, `set` captures, `apply`,
//! `autoescape`). Random fillers (text, prints, sets, loops, comments, `verbatim`, `{% line %}`,
//! whitespace control) shift the fault's line, and expressions are spread over several lines with
//! variables on other lines, which moves the debug markers `Error::guess()` reads.
//!
//! Faults:
//! - runtime errors: undefined variables, keys, attributes and methods (strict and non-strict),
//!   calling non-callables (also arrays as callables and arrows with too few parameters),
//!   division and modulo by zero, invalid arguments to filters and functions (PHP
//!   `TypeError`/`ValueError`/`ArgumentCountError`, Twig `RuntimeError`s), operand type errors,
//!   spreads, regular expressions, destructuring, enums, bad includes/embeds/imports/sources,
//!   missing templates and blocks, macro errors (rich call shapes), recursion failing at a depth,
//!   exceptions thrown by objects' methods and by lazily filtered iterators, `TemplateWrapper`
//!   `render()`/`renderBlock()`, warnings turned into exceptions (`"errors": "throw"`);
//! - line probes: a failing operand among benign ones spread over several lines, faults in
//!   argument, key, interpolation and arrow-body positions, and evaluation-order probes (two
//!   different faults in one expression: hash keys and values, named arguments that Twig
//!   reorders, filter input and arguments, ...);
//! - compile-time errors: argument binding of filters, functions and tests (too few/many
//!   arguments, unknown, duplicated and misplaced named arguments, arguments on other lines),
//!   macro definitions, misplaced `extends`/`block`/`parent()`, unknown enums;
//! - deprecations: the `spaceless` filter, the `deprecated` tag, string escapes, operator
//!   precedence changes, macro calls (arguments, names, parentheses, `defined`), macro
//!   redefinitions, tags outside the root, `include(sandboxed)`, the `sandbox` tag, `snake_case`
//!   variadic arguments, deprecated PHP constants and PHP's own deprecations (implicit float to
//!   int conversions, `null` arguments, ...), alone or several in several templates.
//!
//! Settings vary too: `strict_variables`, `autoescape` (also the file-extension strategy),
//! `charset`, `use_yield`, `optimizations` (rarely deprecated or invalid modes), `debug`, globals,
//! warnings collected or thrown, `load_all` (templates compiled up front or when first used), the
//! order of the templates, template names with spaces, quotes, `%`, directories and non-ASCII
//! characters, CRLF/CR line endings, deprecation "storms", and objects in the data (`data_php`
//! with the helper classes of Twig's `tests/IntegrationTest.php`, `stdClass`, `ArrayObject`,
//! `Markup`, dates). Every case uses a filesystem cache of its own, so that `Error::guess()` tells
//! the compiled templates apart as twig-rs does (the `cache => false` line guessing is a listed
//! deviation).
//!
//! The generator avoids what cannot match by design (`reference/deviations.md`): unseeded
//! randomness, the current time, `dump()` of objects, known PHP function names as string callables
//! and unbounded recursion; it does not provoke the operand order of commutative operators already
//! known from `crates/twig-fuzz/vectors/round0-mismatches.json` on purpose.
//!
//! [`generate`] is deterministic: case `i` of seed `s` depends only on `(s, i)` ([`case`]). Ids are
//! `errors-<seed>-<i>-<labels>`, the labels naming the hops and the kind of fault.

use crate::kit::json::Json;

/// The family name.
pub const FAMILY: &str = "errors";

/// `count` cases of seed `seed`.
pub fn generate(seed: u64, count: usize) -> Vec<Json> {
    (0..count).map(|index| case(seed, index)).collect()
}

/// Case `index` of seed `seed`.
pub fn case(seed: u64, index: usize) -> Json {
    Gen::new(seed, index).case()
}

// ---------------------------------------------------------------------------------------------
// randomness

/// SplitMix64.
#[derive(Clone, Debug)]
struct Rng(u64);

impl Rng {
    fn new(seed: u64, index: usize) -> Rng {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ (index as u64).wrapping_mul(0xD1B5_4A32_D192_ED03)
            ^ 0x5EED_E770_0000_0001);
        rng.next();
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

    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }

    fn range(&mut self, low: usize, high: usize) -> usize {
        low + self.below(high - low + 1)
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }

    fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        items[self.below(items.len())]
    }

    fn weighted<T: Copy>(&mut self, items: &[(u32, T)]) -> T {
        let total: u64 = items.iter().map(|(w, _)| u64::from(*w)).sum();
        let mut roll = self.next() % total.max(1);
        for (weight, item) in items {
            let weight = u64::from(*weight);
            if roll < weight {
                return *item;
            }
            roll -= weight;
        }
        items[items.len() - 1].1
    }

    fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = self.below(i + 1);
            items.swap(i, j);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// data

/// The type of a data variable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ty {
    Int,
    Float,
    Str,
    Bool,
    Null,
    List,
    Map,
    Obj,
}

/// The data of every case: name, type, whether benign expressions may use it, JSON text.
const DATA: &[(&str, Ty, bool, &str)] = &[
    ("n", Ty::Int, true, "5"),
    ("one", Ty::Int, true, "1"),
    ("neg", Ty::Int, true, "-3"),
    ("zero", Ty::Int, false, "0"),
    ("big", Ty::Int, false, "9223372036854775807"),
    ("fl", Ty::Float, true, "2.5"),
    ("fzero", Ty::Float, false, "0.0"),
    ("s", Ty::Str, true, "\"abc\""),
    ("e", Ty::Str, true, "\"\""),
    ("html", Ty::Str, true, "\"<b>x</b> & y\""),
    ("num", Ty::Str, true, "\"10\""),
    ("word", Ty::Str, true, "\"a,b,c\""),
    ("key", Ty::Str, true, "\"a\""),
    ("bad_key", Ty::Str, false, "\"zz\""),
    ("fmt", Ty::Str, false, "\"%s-%d\""),
    ("tpl", Ty::Str, false, "\"part.twig\""),
    ("missing", Ty::Str, false, "\"nope.twig\""),
    ("date_s", Ty::Str, false, "\"2020-01-02 03:04:05\""),
    ("bad_date", Ty::Str, false, "\"not a date\""),
    ("bad_tz", Ty::Str, false, "\"Mars/Phobos\""),
    ("pattern", Ty::Str, false, "\"/(/\""),
    ("t", Ty::Bool, true, "true"),
    ("f", Ty::Bool, true, "false"),
    ("nul", Ty::Null, true, "null"),
    ("items", Ty::List, true, "[1, 2, 3]"),
    ("strs", Ty::List, true, "[\"a\", \"b\"]"),
    ("empty", Ty::List, true, "[]"),
    (
        "rows",
        Ty::List,
        false,
        "[{\"id\": 1, \"name\": \"x\"}, {\"id\": 2}]",
    ),
    ("tpls", Ty::List, false, "[\"nope.twig\", \"part.twig\"]"),
    ("map", Ty::Map, true, "{\"a\": 1, \"b\": 2}"),
    (
        "nested",
        Ty::Map,
        false,
        "{\"a\": {\"b\": [1, {\"c\": \"d\"}]}}",
    ),
];

/// The objects of `data_php` cases: name, PHP expression.
const OBJECTS: &[(&str, &str)] = &[
    ("obj", "(object) ['a' => 1, 'list' => [1, 2]]"),
    ("ao", "new \\ArrayObject(['a' => 1, 'b' => 2])"),
    ("foo", "new \\Twig\\Tests\\TwigTestFoo()"),
    ("magic", "new \\Twig\\Tests\\MagicCallStub()"),
    ("str_obj", "new \\Twig\\Tests\\ToStringStub('abc')"),
    ("countable", "new \\Twig\\Tests\\CountableStub(2)"),
    ("iter", "new \\Twig\\Tests\\SimpleIteratorForTesting()"),
    (
        "agg",
        "new \\Twig\\Tests\\IteratorAggregateStub(['x' => 1, 'y' => 2])",
    ),
    ("markup", "new \\Twig\\Markup('<i>m</i>', 'UTF-8')"),
    (
        "dt",
        "new \\DateTimeImmutable('2020-01-02 03:04:05', new \\DateTimeZone('UTC'))",
    ),
];

/// Names that are never defined.
const UNDEFINED: &[&str] = &[
    "undef",
    "nope",
    "missing_var",
    "user",
    "fooBar",
    "x1",
    "items2",
];

/// The variables visible at one position of a template.
#[derive(Clone, Debug, Default)]
struct Scope {
    /// Defined variables and their types.
    vars: Vec<(String, Ty, bool)>,
    /// The value variable of the innermost `for` (`loop` is defined too).
    loop_var: Option<String>,
    /// Inside a macro body.
    in_macro: bool,
    /// Inside a block body.
    in_block: bool,
    /// `parent()` may be called here.
    parent_ok: bool,
    /// The current template.
    template: String,
}

impl Scope {
    fn has(&self, name: &str) -> bool {
        self.vars.iter().any(|(n, _, _)| n == name)
    }

    fn of(&self, ty: Ty, benign_only: bool) -> Vec<&str> {
        self.vars
            .iter()
            .filter(|(_, t, b)| *t == ty && (*b || !benign_only))
            .map(|(n, _, _)| n.as_str())
            .collect()
    }

    fn with_var(&self, name: &str, ty: Ty) -> Scope {
        let mut scope = self.clone();
        scope.vars.retain(|(n, _, _)| n != name);
        scope.vars.push((name.to_owned(), ty, true));
        scope
    }
}

// ---------------------------------------------------------------------------------------------
// the generator

/// What the innermost position holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FaultKind {
    Expr,
    Stmt,
    Binding,
    Deprecation,
    Compile,
    DeprecationThenExpr,
    DeprecationThenCompile,
    Benign,
}

/// One hop on the way from the main template to the fault.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Hop {
    Include,
    EmbedOverride,
    EmbedBase,
    Block,
    MacroLib,
    MacroSelf,
    Trait,
    BlockFn,
    StringTemplate,
    Loop,
    If,
    With,
    Capture,
    Apply,
    Autoescape,
}

struct Gen {
    rng: Rng,
    seed: u64,
    index: usize,
    strict: bool,
    objects: bool,
    string_loader: bool,
    debug_ext: bool,
    sandbox_ext: bool,
    /// Templates other than `index.twig`, in creation order.
    templates: Vec<(String, String)>,
    counter: usize,
    /// Root-level additions (macros, `use`, `import`) of the templates being built.
    roots: Vec<Vec<String>>,
    labels: Vec<String>,
    /// `part.twig` is referenced.
    part: bool,
    /// Percentage of fillers that are deprecation triggers.
    deprecation_rate: u64,
    /// Templates get names with spaces, quotes, `%`, non-ASCII characters, directories.
    fancy: bool,
    /// Globals `g`, `gl` and `n` (shadowed by the data) are registered.
    globals: bool,
}

impl Gen {
    fn new(seed: u64, index: usize) -> Gen {
        Gen {
            rng: Rng::new(seed, index),
            seed,
            index,
            strict: false,
            objects: false,
            string_loader: false,
            debug_ext: false,
            sandbox_ext: false,
            templates: Vec::new(),
            counter: 0,
            roots: Vec::new(),
            labels: Vec::new(),
            part: false,
            deprecation_rate: 7,
            fancy: false,
            globals: false,
        }
    }

    fn label(&mut self, label: &str) {
        if self.labels.len() < 6 {
            self.labels.push(label.to_owned());
        }
    }

    fn fresh(&mut self, prefix: &str) -> String {
        self.counter += 1;
        format!("{prefix}{}", self.counter)
    }

    fn fresh_template(&mut self, prefix: &str) -> String {
        let name = self.fresh(prefix);
        if !self.fancy {
            return format!("{name}.twig");
        }
        match self.rng.below(9) {
            0 => format!("{prefix} {name}.twig"),
            1 => format!("dir/sub/{name}.twig"),
            2 => format!("{name}'q.twig"),
            3 => format!("{name}\"dq.twig"),
            4 => format!("{name}%s%d.html"),
            5 => format!("é{name}ü.twig"),
            6 => format!("@ns/{name}.twig"),
            7 => name.clone(),
            _ => format!("{name}\\back.twig"),
        }
    }

    fn top_scope(&self) -> Scope {
        let mut scope = self.data_scope();
        if self.globals {
            scope.vars.push(("g".to_owned(), Ty::Str, true));
            scope.vars.push(("gl".to_owned(), Ty::List, true));
        }
        scope
    }

    fn data_scope(&self) -> Scope {
        let mut vars: Vec<(String, Ty, bool)> = DATA
            .iter()
            .map(|(n, t, b, _)| ((*n).to_owned(), *t, *b))
            .collect();
        if self.objects {
            vars.extend(
                OBJECTS
                    .iter()
                    .map(|(n, _)| ((*n).to_owned(), Ty::Obj, false)),
            );
        }
        Scope {
            vars,
            template: "index.twig".to_owned(),
            ..Scope::default()
        }
    }

    fn case(mut self) -> Json {
        self.strict = self.rng.chance(60);
        self.objects = self.rng.chance(30);
        self.string_loader = self.rng.chance(30);
        self.debug_ext = self.rng.chance(12);
        self.sandbox_ext = self.rng.chance(5);
        if self.rng.chance(15) {
            self.deprecation_rate = 40;
            self.label("storm");
        }
        if self.rng.chance(15) {
            self.fancy = true;
            self.label("fancy");
        }
        if self.rng.chance(12) {
            self.globals = true;
            self.label("glob");
        }
        let scope = self.top_scope();
        let depth = self
            .rng
            .weighted(&[(28, 0usize), (40, 1), (22, 2), (10, 3)]);
        let index = self.template_to_fault("index.twig", &scope, depth);
        let mut templates = vec![("index.twig".to_owned(), index)];
        templates.append(&mut self.templates);
        let all: String = templates.iter().map(|(_, s)| s.as_str()).collect();
        if self.part || all.contains("tpl") || all.contains("part.twig") {
            templates.push(("part.twig".to_owned(), "[part {{ n ?? 0 }}]".to_owned()));
        }
        if all.contains("blocks.twig") {
            templates.push((
                "blocks.twig".to_owned(),
                "{% block shared %}shared{% endblock %}".to_owned(),
            ));
        }
        if templates.len() > 1 && self.rng.chance(35) {
            self.rng.shuffle(&mut templates);
        }
        let ending = self.rng.weighted(&[(90, "\n"), (6, "\r\n"), (4, "\r")]);
        if ending != "\n" {
            self.label(if ending == "\r" { "cr" } else { "crlf" });
            for (_, source) in &mut templates {
                *source = source.replace('\n', ending);
            }
        }

        let mut fields: Vec<(String, Json)> = Vec::new();
        let labels = if self.labels.is_empty() {
            String::new()
        } else {
            format!("-{}", self.labels.join("."))
        };
        fields.push((
            "id".to_owned(),
            Json::str(format!("{FAMILY}-{}-{}{labels}", self.seed, self.index)),
        ));
        fields.push(("env".to_owned(), Json::str("plain")));
        fields.push((
            "templates".to_owned(),
            Json::Object(
                templates
                    .into_iter()
                    .map(|(name, source)| (name, Json::str(source)))
                    .collect(),
            ),
        ));
        let id = format!("{FAMILY}-{}-{}{labels}", self.seed, self.index);
        let mut config = self.config();
        // a filesystem cache tells the compiled templates apart in `Error::guess()`, as twig-rs
        // does (reference/deviations.md, "Line guessing with `cache => false`")
        config.set("cache", Json::str(format!("/tmp/twig-fuzz-errors/{id}")));
        fields.push(("config".to_owned(), config));
        let mut extensions = Vec::new();
        if self.debug_ext {
            extensions.push(Json::str("debug"));
        }
        if self.string_loader {
            extensions.push(Json::str("string_loader"));
        }
        if self.sandbox_ext {
            extensions.push(Json::str("sandbox"));
        }
        if !extensions.is_empty() {
            fields.push(("extensions".to_owned(), Json::Array(extensions)));
        }
        if self.globals {
            fields.push((
                "globals".to_owned(),
                Json::parse("{\"g\": \"glob\", \"gl\": [1, 2], \"n\": 99}").expect("valid JSON"),
            ));
        }
        if self.objects {
            fields.push(("data_php".to_owned(), Json::str(data_php())));
            fields.push(("test_classes".to_owned(), Json::Bool(true)));
        } else {
            fields.push(("data_json".to_owned(), data_json()));
        }
        fields.push(("timezone".to_owned(), Json::str("UTC")));
        if self.rng.chance(30) {
            fields.push(("errors".to_owned(), Json::str("throw")));
        }
        if self.rng.chance(30) {
            fields.push(("load_all".to_owned(), Json::Bool(false)));
        }
        Json::Object(fields)
    }

    fn config(&mut self) -> Json {
        let mut config: Vec<(String, Json)> = Vec::new();
        if self.strict {
            config.push(("strict_variables".to_owned(), Json::Bool(true)));
        } else if self.rng.chance(50) {
            config.push(("strict_variables".to_owned(), Json::Bool(false)));
        }
        match self
            .rng
            .weighted(&[(40, 0), (30, 1), (18, 2), (8, 3), (4, 4)])
        {
            1 => config.push(("autoescape".to_owned(), Json::Bool(false))),
            2 => config.push(("autoescape".to_owned(), Json::str("html"))),
            3 => config.push(("autoescape".to_owned(), Json::str("js"))),
            4 => {
                // the strategy of the file extension (`.twig` names escape nothing)
                self.label("aename");
                config.push(("autoescape".to_owned(), Json::str("name")));
            }
            _ => {}
        }
        if self.rng.chance(4) {
            let charset = self.rng.pick(&["ISO-8859-1", "utf-8", "nope", "ASCII"]);
            self.label("charset");
            config.push(("charset".to_owned(), Json::str(charset)));
        }
        if self.rng.chance(25) {
            config.push(("use_yield".to_owned(), Json::Bool(true)));
        }
        match self.rng.weighted(&[(83, 0), (12, 1), (4, 2), (1, 3)]) {
            1 => config.push(("optimizations".to_owned(), Json::int(0))),
            2 => config.push(("optimizations".to_owned(), Json::int(2))),
            3 => {
                // deprecated optimizer modes
                self.label("optmode");
                let mode = self.rng.pick(&[4, 8, 12, -2, 16]);
                config.push(("optimizations".to_owned(), Json::int(mode)));
            }
            _ => {}
        }
        if self.rng.chance(10) {
            config.push(("debug".to_owned(), Json::Bool(true)));
        }
        Json::Object(config)
    }

    // -----------------------------------------------------------------------------------------
    // layout

    /// The source of a whole template whose rendering reaches the fault (`depth` more hops).
    fn template_to_fault(&mut self, name: &str, scope: &Scope, depth: usize) -> String {
        let mut scope = scope.clone();
        scope.template = name.to_owned();
        scope.loop_var = None;
        scope.in_macro = false;
        scope.in_block = false;
        scope.parent_ok = false;
        self.roots.push(Vec::new());
        let (chunks, child) = if depth > 0 && self.rng.chance(35) {
            (self.child_template(&scope, depth - 1), true)
        } else {
            (self.body_to_fault(&scope, depth), false)
        };
        let extras = self.roots.pop().unwrap_or_default();
        self.assemble(chunks, extras, child)
    }

    /// Joins root chunks and root additions (imports, macros, `use`) into a template source.
    fn assemble(&mut self, mut chunks: Vec<String>, extras: Vec<String>, child: bool) -> String {
        for extra in extras {
            let at = if extra.starts_with("{% macro") || self.rng.chance(20) {
                self.rng.below(chunks.len() + 1)
            } else {
                0
            };
            chunks.insert(at, extra);
        }
        if self.rng.chance(7) {
            let at = self.rng.below(chunks.len() + 1);
            let line = self.rng.pick(&[1, 7, 40, 1000]);
            chunks.insert(at, format!("{{% line {line} %}}"));
            self.label("line");
        }
        self.join(&chunks, child)
    }

    /// Concatenates chunks with random separators (only blank ones at the root of a child).
    fn join(&mut self, chunks: &[String], child: bool) -> String {
        let mut out = String::new();
        for (i, chunk) in chunks.iter().enumerate() {
            if i > 0 || self.rng.chance(15) {
                out.push_str(&self.separator(child));
            }
            out.push_str(chunk);
        }
        if self.rng.chance(50) {
            out.push('\n');
        }
        out
    }

    fn separator(&mut self, child: bool) -> String {
        let blank = ["", "\n", "\n", "\n\n", " ", "\n    ", "\n\n\n"];
        if child {
            return match self.rng.below(10) {
                0 => "\n{# a comment\n   over two lines #}\n".to_owned(),
                _ => self.rng.pick(&blank).to_owned(),
            };
        }
        match self.rng.below(14) {
            0 => "\ntext line\n".to_owned(),
            1 => "\n{# comment #}\n".to_owned(),
            2 => "\n{# a comment\n   over two lines #}".to_owned(),
            3 => "\nhéllo wörld\n".to_owned(),
            4 => "{% verbatim %}{{ not parsed }}\n{% endverbatim %}".to_owned(),
            _ => self.rng.pick(&blank).to_owned(),
        }
    }

    /// A space, or sometimes a line break with indentation.
    fn sp(&mut self) -> String {
        match self.rng.below(10) {
            0 | 1 => "\n    ".to_owned(),
            2 => "\n".to_owned(),
            _ => " ".to_owned(),
        }
    }

    /// Body chunks with the way to the fault somewhere among fillers.
    fn body_to_fault(&mut self, scope: &Scope, depth: usize) -> Vec<String> {
        let before = self.rng.weighted(&[(35, 0usize), (40, 1), (20, 2), (5, 3)]);
        let after = self.rng.weighted(&[(45, 0usize), (40, 1), (15, 2)]);
        let mut chunks = Vec::new();
        for _ in 0..before {
            chunks.push(self.filler(scope));
        }
        let path = if depth == 0 || self.rng.chance(15) {
            self.fault(scope)
        } else {
            self.hop(scope, depth - 1)
        };
        chunks.push(path);
        for _ in 0..after {
            chunks.push(self.filler(scope));
        }
        chunks
    }

    fn body_text(&mut self, scope: &Scope, depth: usize) -> String {
        let chunks = self.body_to_fault(scope, depth);
        self.join(&chunks, false)
    }

    /// The statement that leads one hop further.
    fn hop(&mut self, scope: &Scope, depth: usize) -> String {
        let mut hops = vec![
            (16, Hop::Include),
            (8, Hop::EmbedOverride),
            (4, Hop::EmbedBase),
            (12, Hop::MacroLib),
            (6, Hop::MacroSelf),
            (5, Hop::BlockFn),
            (8, Hop::Loop),
            (5, Hop::If),
            (4, Hop::With),
            (4, Hop::Capture),
            (3, Hop::Apply),
            (2, Hop::Autoescape),
        ];
        if !scope.in_macro {
            hops.push((8, Hop::Block));
            hops.push((5, Hop::Trait));
        }
        if self.string_loader {
            hops.push((5, Hop::StringTemplate));
        }
        match self.rng.weighted(&hops) {
            Hop::Include => self.hop_include(scope, depth),
            Hop::EmbedOverride => self.hop_embed_override(scope, depth),
            Hop::EmbedBase => self.hop_embed_base(scope, depth),
            Hop::Block => self.hop_block(scope, depth),
            Hop::MacroLib => self.hop_macro(scope, depth, false),
            Hop::MacroSelf => self.hop_macro(scope, depth, true),
            Hop::Trait => self.hop_trait(scope, depth),
            Hop::BlockFn => self.hop_block_fn(scope, depth),
            Hop::StringTemplate => self.hop_string_template(scope, depth),
            Hop::Loop => self.hop_loop(scope, depth),
            Hop::If => self.hop_if(scope, depth),
            Hop::With => self.hop_with(scope, depth),
            Hop::Capture => self.hop_capture(scope, depth),
            Hop::Apply => self.hop_apply(scope, depth),
            Hop::Autoescape => self.hop_autoescape(scope, depth),
        }
    }

    /// `{k: k, ...}` for a few variables of `scope`, and the scope an `only` target sees.
    fn pass_vars(&mut self, scope: &Scope) -> (String, Vec<(String, Ty, bool)>) {
        let mut names: Vec<(String, Ty, bool)> = scope.vars.clone();
        self.rng.shuffle(&mut names);
        names.truncate(self.rng.range(1, 4));
        let mut parts = Vec::new();
        for (name, _, _) in &names {
            if self.rng.chance(15) {
                parts.push(format!("'{name}': {name}"));
            } else {
                parts.push(format!("{name}: {name}"));
            }
        }
        let sp = self.sp();
        (format!("{{{}}}", parts.join(&format!(",{sp}"))), names)
    }

    fn hop_include(&mut self, scope: &Scope, depth: usize) -> String {
        let name = self.fresh_template("inc");
        let form = self.rng.weighted(&[(50, 0), (35, 1), (15, 2)]);
        let with = self.rng.chance(50) || form == 2;
        let only = form == 2 || (with && self.rng.chance(40));
        let (vars, passed) = if with {
            self.pass_vars(scope)
        } else {
            (String::new(), Vec::new())
        };
        let mut inner = scope.clone();
        if only {
            inner.vars = passed;
        }
        inner.loop_var = None;
        inner.in_macro = false;
        let source = self.template_to_fault(&name, &inner, depth);
        self.templates.push((name.clone(), source));
        let ignore = self.rng.chance(15);
        let sp = self.sp();
        match form {
            0 => {
                self.label("inc");
                let q = twig_string(&name);
                let mut tag = format!("{{% include{sp}{q}");
                if ignore {
                    tag.push_str(" ignore missing");
                }
                if with {
                    tag.push_str(&format!(" with {vars}"));
                }
                if only {
                    tag.push_str(" only");
                }
                tag.push_str(" %}");
                tag
            }
            1 => {
                self.label("incf");
                let mut args = vec![twig_string(&name)];
                if with {
                    args.push(vars);
                }
                if only {
                    args.push("with_context = false".to_owned());
                }
                if ignore {
                    args.push("ignore_missing = true".to_owned());
                }
                format!("{{{{ include({}) }}}}", args.join(&format!(",{sp}")))
            }
            _ => {
                self.label("inco");
                let q = twig_string(&name);
                format!("{{{{ include_only({q},{sp}{vars}) }}}}")
            }
        }
    }

    fn hop_embed_override(&mut self, scope: &Scope, depth: usize) -> String {
        self.label("emb");
        let base = self.fresh_template("embed");
        let block = self.fresh("b");
        let other = self.fresh("b");
        let base_scope = Scope {
            template: base.clone(),
            loop_var: None,
            in_macro: false,
            in_block: false,
            parent_ok: false,
            ..scope.clone()
        };
        let base_source = self.benign_with_blocks(&base_scope, &[block.clone(), other.clone()]);
        self.templates.push((base.clone(), base_source));
        let with = self.rng.chance(30);
        let only = with && self.rng.chance(40);
        let (vars, passed) = if with {
            self.pass_vars(scope)
        } else {
            (String::new(), Vec::new())
        };
        let mut inner = scope.clone();
        if only {
            inner.vars = passed;
        }
        inner.in_block = true;
        inner.parent_ok = true;
        inner.loop_var = None;
        inner.in_macro = false;
        let body = self.body_text(&inner, depth);
        let q = twig_string(&base);
        let mut tag = format!("{{% embed {q}");
        if with {
            tag.push_str(&format!(" with {vars}"));
        }
        if only {
            tag.push_str(" only");
        }
        tag.push_str(" %}");
        let mut blocks = vec![format!("{{% block {block} %}}{body}{{% endblock %}}")];
        if self.rng.chance(40) {
            let filler = self.filler(&inner);
            blocks.push(format!("{{% block {other} %}}{filler}{{% endblock %}}"));
        }
        self.rng.shuffle(&mut blocks);
        let sep = self.separator(true);
        format!("{tag}{sep}{}{sep}{{% endembed %}}", blocks.join("\n"))
    }

    fn hop_embed_base(&mut self, scope: &Scope, depth: usize) -> String {
        self.label("embb");
        let base = self.fresh_template("embed");
        let block = self.fresh("b");
        let mut source = self.template_to_fault(&base, scope, depth);
        source.push_str(&format!("\n{{% block {block} %}}base{{% endblock %}}"));
        self.templates.push((base.clone(), source));
        let filler = self.benign(scope);
        let sep = self.separator(true);
        let q = twig_string(&base);
        if self.rng.chance(50) {
            format!(
                "{{% embed {q} %}}{sep}{{% block {block} %}}{{{{ {filler} }}}}{{% endblock %}}{sep}{{% endembed %}}"
            )
        } else {
            format!("{{% embed {q} %}}{sep}{{% endembed %}}")
        }
    }

    fn hop_block(&mut self, scope: &Scope, depth: usize) -> String {
        self.label("blk");
        let block = self.fresh("b");
        let mut inner = scope.clone();
        inner.in_block = true;
        let body = self.body_text(&inner, depth);
        let def = format!("{{% block {block} %}}{body}{{% endblock %}}");
        if self.rng.chance(25) {
            let sep = self.separator(false);
            format!("{def}{sep}{{{{ block('{block}') }}}}")
        } else {
            def
        }
    }

    fn hop_macro(&mut self, scope: &Scope, depth: usize, own: bool) -> String {
        let macro_name = self.fresh("m");
        // the macro receives a few variables under their own names, so they stay defined
        let mut passed: Vec<(String, Ty, bool)> = scope.vars.clone();
        self.rng.shuffle(&mut passed);
        passed.truncate(self.rng.range(0, 3));
        let mut params: Vec<String> = passed.iter().map(|(n, _, _)| n.clone()).collect();
        let mut inner_vars = passed.clone();
        let with_default = self.rng.chance(40);
        if with_default {
            let default = self
                .rng
                .pick(&["1", "'d'", "null", "[1, 2]", "{k: 'v'}", "-1"]);
            params.push(format!("opt = {default}"));
            inner_vars.push(("opt".to_owned(), Ty::Str, false));
        }
        let variadic = self.rng.chance(15);
        if variadic {
            params.push("...rest".to_owned());
            inner_vars.push(("rest".to_owned(), Ty::List, true));
        }
        let lib = if own {
            scope.template.clone()
        } else {
            self.fresh_template("macros")
        };
        if self.globals {
            inner_vars.push(("g".to_owned(), Ty::Str, true));
            inner_vars.push(("gl".to_owned(), Ty::List, true));
        }
        let inner = Scope {
            vars: inner_vars,
            loop_var: None,
            in_macro: true,
            in_block: false,
            parent_ok: false,
            template: lib.clone(),
        };
        let body = self.body_text(&inner, depth);
        let sp = self.sp();
        let def = format!(
            "{{% macro {macro_name}({}) %}}{body}{{% endmacro %}}",
            params.join(&format!(",{sp}"))
        );
        // positional or named arguments, sometimes with a deprecated shape
        let mut args: Vec<String> = Vec::new();
        let named = self.rng.chance(25);
        for (name, _, _) in &passed {
            if named {
                args.push(format!("{name} = {name}"));
            } else {
                args.push(name.clone());
            }
        }
        if self.rng.chance(10) && !passed.is_empty() {
            args.pop();
            self.label("dmiss");
        }
        if self.rng.chance(8) {
            args.push("'extra'".to_owned());
            self.label("dextra");
        }
        if self.rng.chance(6) {
            args.push("zz = 1".to_owned());
            self.label("dnamed");
        }
        let sp = self.sp();
        let args = args.join(&format!(",{sp}"));
        let mut called = macro_name.clone();
        if self.rng.chance(6) {
            called = called.to_uppercase();
            self.label("dcase");
        } else if self.rng.chance(4) {
            called = format!("macro_{called}");
            self.label("dprefix");
        }
        let call_form = self.rng.weighted(&[(70, 0), (20, 1), (10, 2)]);
        let (import, call) = if own {
            self.label("mself");
            self.roots
                .last_mut()
                .expect("a template is being built")
                .push(def);
            match call_form {
                0 => (None, format!("_self.{called}({args})")),
                1 => {
                    let alias = self.fresh("me");
                    (
                        Some(format!("{{% import _self as {alias} %}}")),
                        format!("{alias}.{called}({args})"),
                    )
                }
                _ => (
                    Some(format!("{{% from _self import {macro_name} %}}")),
                    format!("{macro_name}({args})"),
                ),
            }
        } else {
            self.label("mlib");
            let mut lib_chunks = vec![def];
            if self.rng.chance(40) {
                let other = self.fresh("m");
                lib_chunks.push(format!(
                    "{{% macro {other}(a) %}}[{{{{ a }}}}]{{% endmacro %}}"
                ));
            }
            if self.rng.chance(4) {
                // a redefinition (deprecated since 3.29)
                lib_chunks.push(format!("{{% macro {macro_name}() %}}first{{% endmacro %}}"));
                self.label("dredef");
            }
            self.rng.shuffle(&mut lib_chunks);
            let lib_source = self.join(&lib_chunks, true);
            self.templates.push((lib.clone(), lib_source));
            let q = twig_string(&lib);
            match call_form {
                0 => {
                    let alias = self.fresh("lib");
                    (
                        Some(format!("{{% import {q} as {alias} %}}")),
                        format!("{alias}.{called}({args})"),
                    )
                }
                1 => (
                    Some(format!("{{% from {q} import {macro_name} %}}")),
                    format!("{macro_name}({args})"),
                ),
                _ => {
                    let alias = self.fresh("f");
                    (
                        Some(format!("{{% from {q} import {macro_name} as {alias} %}}")),
                        format!("{alias}({args})"),
                    )
                }
            }
        };
        let statement = match self.rng.weighted(&[(60, 0), (15, 1), (15, 2), (10, 3)]) {
            0 => format!("{{{{ {call} }}}}"),
            1 => format!("{{% do {call} %}}"),
            2 => {
                let v = self.fresh("r");
                format!("{{% set {v} = {call} %}}{{{{ {v} }}}}")
            }
            _ => format!("{{{{ {call}|upper }}}}"),
        };
        match import {
            Some(import) if self.rng.chance(60) => {
                self.roots
                    .last_mut()
                    .expect("a template is being built")
                    .push(import);
                statement
            }
            Some(import) => {
                let sep = self.separator(false);
                format!("{import}{sep}{statement}")
            }
            None => statement,
        }
    }

    fn hop_trait(&mut self, scope: &Scope, depth: usize) -> String {
        self.label("trait");
        let name = self.fresh_template("trait");
        let block = self.fresh("t");
        let inner = Scope {
            template: name.clone(),
            in_block: true,
            parent_ok: false,
            loop_var: None,
            ..scope.clone()
        };
        let body = self.body_text(&inner, depth);
        let mut blocks = vec![format!("{{% block {block} %}}{body}{{% endblock %}}")];
        if self.rng.chance(40) {
            let other = self.fresh("t");
            blocks.push(format!("{{% block {other} %}}other{{% endblock %}}"));
        }
        self.rng.shuffle(&mut blocks);
        let source = self.join(&blocks, true);
        self.templates.push((name.clone(), source));
        let q = twig_string(&name);
        let (use_tag, rendered) = if self.rng.chance(30) {
            let alias = self.fresh("alias");
            (format!("{{% use {q} with {block} as {alias} %}}"), alias)
        } else {
            (format!("{{% use {q} %}}"), block)
        };
        self.roots
            .last_mut()
            .expect("a template is being built")
            .push(use_tag);
        format!("{{{{ block('{rendered}') }}}}")
    }

    fn hop_block_fn(&mut self, scope: &Scope, depth: usize) -> String {
        self.label("bfn");
        let name = self.fresh_template("other");
        let block = self.fresh("b");
        let inner = Scope {
            template: name.clone(),
            in_block: true,
            parent_ok: false,
            loop_var: scope.loop_var.clone(),
            ..scope.clone()
        };
        let body = self.body_text(&inner, depth);
        let before = self.filler(&inner);
        let source = format!("{before}\n{{% block {block} %}}{body}{{% endblock %}}\n");
        self.templates.push((name.clone(), source));
        let q = twig_string(&name);
        match self.rng.below(4) {
            0 => format!(
                "{{% if block('{block}', {q}) is defined %}}{{{{ block('{block}', {q}) }}}}{{% endif %}}"
            ),
            1 => format!("{{% set v = block('{block}', {q}) %}}{{{{ v }}}}"),
            _ => format!("{{{{ block('{block}', {q}) }}}}"),
        }
    }

    fn hop_string_template(&mut self, scope: &Scope, depth: usize) -> String {
        self.label("str");
        let inner_name = "__string_template__".to_owned();
        let source = self.template_to_fault(&inner_name, scope, depth);
        let literal = twig_string(&source);
        match self.rng.below(5) {
            0 => format!("{{% include template_from_string({literal}) %}}"),
            1 => format!("{{{{ include(template_from_string({literal}, 'custom.twig')) }}}}"),
            2 => format!("{{{{ template_from_string({literal}).render(_context) }}}}"),
            _ => format!("{{{{ include(template_from_string({literal})) }}}}"),
        }
    }

    fn hop_loop(&mut self, scope: &Scope, depth: usize) -> String {
        self.label("for");
        let var = self.fresh("x");
        let mut inner = scope.with_var(&var, Ty::Int);
        inner.loop_var = Some(var.clone());
        let seq = if scope.has("items") && self.rng.chance(70) {
            "items".to_owned()
        } else {
            self.rng
                .pick(&["[1, 2]", "range(1, 3)", "[10, 20, 30]"])
                .to_owned()
        };
        let body = self.body_text(&inner, depth);
        let body = if self.rng.chance(30) {
            format!("{{% if loop.index == 2 %}}{body}{{% endif %}}")
        } else {
            body
        };
        let sp = self.sp();
        if self.rng.chance(20) {
            format!("{{% for k, {var} in{sp}{seq} %}}{body}{{% else %}}none{{% endfor %}}")
        } else {
            format!("{{% for {var} in{sp}{seq} %}}{body}{{% endfor %}}")
        }
    }

    fn hop_if(&mut self, scope: &Scope, depth: usize) -> String {
        self.label("if");
        let body = self.body_text(scope, depth);
        let cond = self.truthy(scope);
        let sp = self.sp();
        match self.rng.below(4) {
            0 => format!("{{% if not{sp}{cond} %}}no{{% else %}}{body}{{% endif %}}"),
            1 => format!(
                "{{% if not {cond} %}}no{{% elseif{sp}{cond} %}}{body}{{% else %}}never{{% endif %}}"
            ),
            _ => format!("{{% if{sp}{cond} %}}{body}{{% endif %}}"),
        }
    }

    fn hop_with(&mut self, scope: &Scope, depth: usize) -> String {
        self.label("with");
        let value = self.benign(scope);
        let var = self.fresh("w");
        let only = self.rng.chance(30);
        let mut inner = scope.with_var(&var, Ty::Str);
        if only {
            inner.vars = vec![(var.clone(), Ty::Str, true)];
        }
        let body = self.body_text(&inner, depth);
        let sp = self.sp();
        format!(
            "{{% with {{{var}:{sp}{value}}}{} %}}{body}{{% endwith %}}",
            if only { " only" } else { "" }
        )
    }

    fn hop_capture(&mut self, scope: &Scope, depth: usize) -> String {
        self.label("cap");
        let var = self.fresh("c");
        let body = self.body_text(scope, depth);
        format!("{{% set {var} %}}{body}{{% endset %}}{{{{ {var} }}}}")
    }

    fn hop_apply(&mut self, scope: &Scope, depth: usize) -> String {
        self.label("apply");
        let filter = self.rng.pick(&[
            "upper",
            "lower|trim",
            "escape('js')",
            "replace({'a': 'b'})",
            "spaceless",
            "nl2br",
        ]);
        let body = self.body_text(scope, depth);
        format!("{{% apply {filter} %}}{body}{{% endapply %}}")
    }

    fn hop_autoescape(&mut self, scope: &Scope, depth: usize) -> String {
        self.label("ae");
        let strategy = self
            .rng
            .pick(&["'js'", "false", "'html'", "'css'", "'url'"]);
        let body = self.body_text(scope, depth);
        format!("{{% autoescape {strategy} %}}{body}{{% endautoescape %}}")
    }

    /// Root chunks of a child template whose rendering reaches the fault.
    fn child_template(&mut self, scope: &Scope, depth: usize) -> Vec<String> {
        let base = self.fresh_template("base");
        let blocks: Vec<String> = (0..self.rng.range(1, 3)).map(|_| self.fresh("b")).collect();
        let target = blocks[self.rng.below(blocks.len())].clone();
        let mut extends = format!("{{% extends {} %}}", twig_string(&base));
        let base_scope = Scope {
            template: base.clone(),
            ..scope.clone()
        };
        let block_scope = Scope {
            in_block: true,
            parent_ok: true,
            ..scope.clone()
        };
        let mut chunks: Vec<String> = Vec::new();
        let place = self.rng.weighted(&[
            (30, 0), // child block
            (14, 1), // parent block through parent()
            (12, 2), // parent block not overridden
            (10, 3), // parent body
            (8, 4),  // grandparent
            (9, 5),  // extends expression
            (9, 6),  // child root
            (6, 7),  // trait block overriding the parent's
            (2, 8),  // compile-time structure errors
        ]);
        match place {
            0 => {
                self.label("cblk");
                let base_source = self.benign_with_blocks(&base_scope, &blocks);
                self.templates.push((base.clone(), base_source));
                for block in &blocks {
                    if *block == target {
                        let body = self.body_text(&block_scope, depth);
                        chunks.push(format!("{{% block {block} %}}{body}{{% endblock %}}"));
                    } else if self.rng.chance(50) {
                        let body = self.filler(&block_scope);
                        chunks.push(format!("{{% block {block} %}}{body}{{% endblock %}}"));
                    }
                }
            }
            1 | 2 => {
                self.label(if place == 1 { "pcall" } else { "pblk" });
                let inner = Scope {
                    template: base.clone(),
                    in_block: true,
                    parent_ok: false,
                    ..scope.clone()
                };
                let mut base_chunks = Vec::new();
                base_chunks.push(self.filler(&base_scope));
                for block in &blocks {
                    let body = if *block == target {
                        self.body_text(&inner, depth)
                    } else {
                        self.filler(&inner)
                    };
                    base_chunks.push(format!("{{% block {block} %}}{body}{{% endblock %}}"));
                }
                let base_source = self.join(&base_chunks, false);
                self.templates.push((base.clone(), base_source));
                for block in &blocks {
                    if *block == target && place == 1 {
                        let before = self.filler(&block_scope);
                        let call = if self.rng.chance(20) {
                            "{% set p = parent() %}{{ p }}".to_owned()
                        } else {
                            "{{ parent() }}".to_owned()
                        };
                        chunks.push(format!(
                            "{{% block {block} %}}{before}\n{call}{{% endblock %}}"
                        ));
                    } else if *block != target && self.rng.chance(50) {
                        let body = self.filler(&block_scope);
                        chunks.push(format!("{{% block {block} %}}{body}{{% endblock %}}"));
                    }
                }
            }
            3 => {
                self.label("pbody");
                let mut base_chunks = self.body_to_fault(&base_scope, depth);
                for block in &blocks {
                    let at = self.rng.below(base_chunks.len() + 1);
                    let filler = self.filler(&base_scope);
                    base_chunks
                        .insert(at, format!("{{% block {block} %}}{filler}{{% endblock %}}"));
                }
                let base_source = self.join(&base_chunks, false);
                self.templates.push((base.clone(), base_source));
                for block in &blocks {
                    if self.rng.chance(50) {
                        let body = self.filler(&block_scope);
                        chunks.push(format!("{{% block {block} %}}{body}{{% endblock %}}"));
                    }
                }
            }
            4 => {
                self.label("grand");
                let base_source = self.template_to_fault(&base, &base_scope, depth);
                self.templates.push((base.clone(), base_source));
                for block in &blocks {
                    let body = self.filler(&block_scope);
                    chunks.push(format!("{{% block {block} %}}{body}{{% endblock %}}"));
                }
            }
            5 => {
                self.label("extx");
                let base_source = self.benign_with_blocks(&base_scope, &blocks);
                self.templates.push((base.clone(), base_source));
                let expr = self.rng.pick(&[
                    "missing",
                    "'nope.twig'",
                    "nul",
                    "n",
                    "undef",
                    "items",
                    "map",
                    "['nope.twig', 'BASE']",
                    "['nope1.twig', 'nope2.twig']",
                    "[]",
                    "t ? 'BASE' : 'nope.twig'",
                    "f ? 'BASE' : missing",
                    "'BASE'|upper",
                    "tpls",
                    "'BA' ~ undef",
                    "include('BASE')",
                    "1 / zero",
                    "map.zz",
                ]);
                let expr = expr.replace("'BASE'", &twig_string(&base));
                let sp = self.sp();
                extends = format!("{{% extends{sp}{expr} %}}");
                for block in &blocks {
                    let body = self.filler(&block_scope);
                    chunks.push(format!("{{% block {block} %}}{body}{{% endblock %}}"));
                }
            }
            6 => {
                self.label("croot");
                let base_source = self.benign_with_blocks(&base_scope, &blocks);
                self.templates.push((base.clone(), base_source));
                let fault = self.fault_expr(scope);
                let v = self.fresh("v");
                let sp = self.sp();
                chunks.push(match self.rng.below(3) {
                    0 => format!("{{% do{sp}{fault} %}}"),
                    _ => format!("{{% set {v} ={sp}{fault} %}}"),
                });
                for block in &blocks {
                    if self.rng.chance(60) {
                        let body = self.filler(&block_scope);
                        chunks.push(format!("{{% block {block} %}}{body}{{% endblock %}}"));
                    }
                }
            }
            7 => {
                self.label("ctrait");
                let base_source = self.benign_with_blocks(&base_scope, &blocks);
                self.templates.push((base.clone(), base_source));
                let trait_name = self.fresh_template("trait");
                let inner = Scope {
                    template: trait_name.clone(),
                    in_block: true,
                    parent_ok: true,
                    ..scope.clone()
                };
                let body = self.body_text(&inner, depth);
                let trait_source = format!("{{% block {target} %}}{body}{{% endblock %}}");
                self.templates.push((trait_name.clone(), trait_source));
                chunks.push(format!("{{% use {} %}}", twig_string(&trait_name)));
            }
            _ => {
                self.label("cstruct");
                let base_source = self.benign_with_blocks(&base_scope, &blocks);
                self.templates.push((base.clone(), base_source));
                let broken = self.rng.pick(&[
                    "{% if t %}{% block BLOCK %}x{% endblock %}{% endif %}",
                    "outside text",
                    "{{ 'printed' }}",
                    "{% block BLOCK %}a{% endblock %}\n{% block BLOCK %}b{% endblock %}",
                    "{% extends 'BASE' %}",
                    "{% block BLOCK %}{% extends 'BASE' %}{% endblock %}",
                    "{% block BLOCK %}{{ parent() }}{{ parent() }}{% endblock %}",
                    "{% include 'BASE' %}",
                    "{% for i in items %}{% block BLOCK %}{{ i }}{% endblock %}{% endfor %}",
                    "{% set x %}{% block BLOCK %}captured{% endblock %}{% endset %}",
                    "{% macro mm() %}{% extends 'BASE' %}{% endmacro %}",
                    "{% block BLOCK %}{% use 'BASE' %}{% endblock %}",
                    "{% block nope_in_parent %}{{ parent() }}{% endblock %}{% block BLOCK %}{{ block('nope_in_parent') }}{% endblock %}",
                    "{% block BLOCK %}{{ parent() }}{{ block('nope') }}{% endblock %}",
                    "{% block BLOCK %}{{ block('BLOCK', 'BASE') }}{% endblock %}",
                ]);
                chunks.push(
                    broken
                        .replace("BLOCK", &target)
                        .replace("'BASE'", &twig_string(&base)),
                );
            }
        }
        let at = if self.rng.chance(85) {
            0
        } else {
            self.rng.below(chunks.len() + 1)
        };
        chunks.insert(at, extends);
        chunks
    }

    /// A benign template defining `blocks`.
    fn benign_with_blocks(&mut self, scope: &Scope, blocks: &[String]) -> String {
        let inner = Scope {
            in_block: true,
            ..scope.clone()
        };
        let mut chunks = Vec::new();
        if self.rng.chance(60) {
            chunks.push(self.filler(scope));
        }
        for block in blocks {
            let body = if self.rng.chance(70) {
                self.filler(&inner)
            } else {
                String::new()
            };
            chunks.push(format!("{{% block {block} %}}{body}{{% endblock %}}"));
            if self.rng.chance(30) {
                chunks.push(self.filler(scope));
            }
        }
        self.join(&chunks, false)
    }

    // -----------------------------------------------------------------------------------------
    // fillers

    /// A benign statement (sometimes a deprecation trigger).
    fn filler(&mut self, scope: &Scope) -> String {
        if self.rng.chance(self.deprecation_rate) {
            return self.deprecation(scope);
        }
        let e = self.benign(scope);
        let sp = self.sp();
        match self.rng.below(16) {
            0..=3 => format!("{{{{ {e} }}}}"),
            4 => format!("{{{{-{sp}{e} }}}}"),
            5 => format!("{{{{ {e}{sp}-}}}}"),
            6 => {
                let v = self.fresh("v");
                format!("{{% set {v} = {e} %}}")
            }
            7 => format!("{{% if {e} %}}yes{{% else %}}no{{% endif %}}"),
            8 => {
                let v = self.fresh("i");
                format!("{{% for {v} in [1, 2] %}}{{{{ {v} }}}}{{% endfor %}}")
            }
            9 => format!("{{% do {e} %}}"),
            10 => "plain text".to_owned(),
            11 => "{# comment #}".to_owned(),
            12 => format!("{{%- if true -%}}\n  {{{{ {e} }}}}\n{{%- endif %}}"),
            13 => {
                let v = self.fresh("c");
                format!("{{% set {v} %}}\ncaptured {{{{ {e} }}}}\n{{% endset %}}")
            }
            14 => "line one\nline two".to_owned(),
            _ => format!("{{{{{sp}{e}{sp}}}}}"),
        }
    }

    /// A benign expression over the variables of `scope`.
    fn benign(&mut self, scope: &Scope) -> String {
        if let Some(var) = &scope.loop_var
            && self.rng.chance(30)
        {
            return match self.rng.below(3) {
                0 => var.clone(),
                1 => format!("{var} * 2"),
                _ => "loop.index".to_owned(),
            };
        }
        let pattern = self.rng.pick(&[
            "$I",
            "$S",
            "$S|upper",
            "$L|join(', ')",
            "$L|length",
            "$M|keys|join",
            "$I + 1",
            "'x' ~ $S",
            "$S ~ $I",
            "$B ? 'y' : 'n'",
            "$N ?? 'dflt'",
            "$L|first",
            "$M|length",
            "$I * 2",
            "'lit'",
            "$F",
            "\"interp #{$I}\"",
        ]);
        self.fill(pattern, scope)
    }

    /// A truthy condition.
    fn truthy(&mut self, scope: &Scope) -> String {
        let pattern = self.rng.pick(&[
            "true",
            "1",
            "$L|length > 0 or true",
            "not false",
            "'x'",
            "[1]",
            "$I is defined or true",
        ]);
        self.fill(pattern, scope)
    }

    // -----------------------------------------------------------------------------------------
    // placeholders

    /// Replaces the placeholders of `pattern` (`$I` int, `$F` float, `$S` string, `$L` list,
    /// `$M` map, `$B` bool, `$N` null, `$A` any, `$Z` zero, `$U` undefined name, `$O` object,
    /// `$K` missing key, `$Q` missing template, `$T` existing template, `$D` bad date, `$W` bad
    /// timezone, `$R` bad regex, `$P` format string, `$X` loop value, `$V` defined variable,
    /// `$_` space or line break, `$$` a dollar sign).
    fn fill(&mut self, pattern: &str, scope: &Scope) -> String {
        let mut out = String::with_capacity(pattern.len() + 16);
        let mut chars = pattern.chars().peekable();
        while let Some(c) = chars.next() {
            if c != '$' {
                out.push(c);
                continue;
            }
            let Some(code) = chars.next() else {
                out.push('$');
                break;
            };
            let value = match code {
                '$' => "$".to_owned(),
                '_' => self.sp(),
                'I' => self.value(scope, Ty::Int),
                'F' => self.value(scope, Ty::Float),
                'S' => self.value(scope, Ty::Str),
                'L' => self.value(scope, Ty::List),
                'M' => self.value(scope, Ty::Map),
                'B' => self.value(scope, Ty::Bool),
                'N' => self.value(scope, Ty::Null),
                'A' => {
                    let ty = self.rng.pick(&[
                        Ty::Int,
                        Ty::Float,
                        Ty::Str,
                        Ty::Bool,
                        Ty::Null,
                        Ty::List,
                        Ty::Map,
                    ]);
                    self.value(scope, ty)
                }
                'Z' => self.zero(scope),
                'U' => self.rng.pick(UNDEFINED).to_owned(),
                'O' => {
                    let objects = scope.of(Ty::Obj, false);
                    if objects.is_empty() {
                        self.value(scope, Ty::Map)
                    } else {
                        objects[self.rng.below(objects.len())].to_owned()
                    }
                }
                'K' => self.special(scope, "bad_key", "'zz'"),
                'Q' => {
                    if self.rng.chance(50) {
                        self.special(scope, "missing", "'nope.twig'")
                    } else {
                        "'nope.twig'".to_owned()
                    }
                }
                'T' => {
                    self.part = true;
                    if self.rng.chance(30) {
                        self.special(scope, "tpl", "'part.twig'")
                    } else {
                        "'part.twig'".to_owned()
                    }
                }
                'D' => self.special(scope, "bad_date", "'not a date'"),
                'W' => self.special(scope, "bad_tz", "'Mars/Phobos'"),
                'R' => self.special(scope, "pattern", "'/(/'"),
                'P' => self.special(scope, "fmt", "'%s-%d'"),
                'j' => {
                    let ints = scope.of(Ty::Int, true);
                    if ints.is_empty() {
                        "_context|length".to_owned()
                    } else {
                        ints[self.rng.below(ints.len())].to_owned()
                    }
                }
                's' => {
                    let strs = scope.of(Ty::Str, true);
                    if strs.is_empty() {
                        "_charset".to_owned()
                    } else {
                        strs[self.rng.below(strs.len())].to_owned()
                    }
                }
                'X' => match &scope.loop_var {
                    Some(var) => var.clone(),
                    None => self.value(scope, Ty::Int),
                },
                'V' => {
                    if scope.vars.is_empty() {
                        "_self".to_owned()
                    } else {
                        let i = self.rng.below(scope.vars.len());
                        scope.vars[i].0.clone()
                    }
                }
                other => {
                    out.push('$');
                    other.to_string()
                }
            };
            out.push_str(&value);
        }
        out
    }

    /// A variable of `ty` defined in `scope`, or a literal.
    fn value(&mut self, scope: &Scope, ty: Ty) -> String {
        let vars = scope.of(ty, true);
        if !vars.is_empty() && self.rng.chance(65) {
            return vars[self.rng.below(vars.len())].to_owned();
        }
        let literals: &[&str] = match ty {
            Ty::Int => &["1", "2", "7", "-1", "100", "3"],
            Ty::Float => &["1.5", "-2.5", "0.5"],
            Ty::Str => &["'abc'", "''", "'x y'", "\"q\"", "'10'", "'é'"],
            Ty::Bool => &["true", "false"],
            Ty::Null => &["null", "none"],
            Ty::List => &["[1, 2]", "[]", "['a', 'b']", "[3, 1, 2]"],
            Ty::Map => &["{a: 1}", "{'k': 'v', 'n': 2}", "{(1): 'one'}"],
            Ty::Obj => &["{a: 1}"],
        };
        self.rng.pick(literals).to_owned()
    }

    /// The variable `name` when defined, else `literal`.
    fn special(&mut self, scope: &Scope, name: &str, literal: &str) -> String {
        if scope.has(name) && self.rng.chance(60) {
            name.to_owned()
        } else {
            literal.to_owned()
        }
    }

    /// A zero divisor.
    fn zero(&mut self, scope: &Scope) -> String {
        let mut options: Vec<&str> = vec!["0", "0.0", "'0'", "null", "false", "(1 - 1)"];
        for name in ["zero", "fzero", "nul", "f"] {
            if scope.has(name) {
                options.push(name);
                options.push(name);
            }
        }
        if scope.has("n") {
            options.push("(n - n)");
        }
        if scope.has("items") {
            options.push("(items|length - 3)");
        }
        self.rng.pick(&options).to_owned()
    }

    // -----------------------------------------------------------------------------------------
    // faults

    fn fault(&mut self, scope: &Scope) -> String {
        let kind = self.rng.weighted(&[
            (44, FaultKind::Expr),
            (14, FaultKind::Stmt),
            (12, FaultKind::Binding),
            (15, FaultKind::Deprecation),
            (8, FaultKind::Compile),
            (6, FaultKind::DeprecationThenExpr),
            (3, FaultKind::DeprecationThenCompile),
            (4, FaultKind::Benign),
        ]);
        match kind {
            FaultKind::Expr => {
                let e = self.fault_expr(scope);
                self.expr_statement(&e, scope)
            }
            FaultKind::Stmt => self.statement_fault(scope),
            FaultKind::Binding => self.binding_fault(scope),
            FaultKind::Deprecation => {
                let mut out = self.deprecation(scope);
                if self.rng.chance(30) {
                    let sep = self.separator(false);
                    let second = self.deprecation(scope);
                    out = format!("{out}{sep}{second}");
                }
                out
            }
            FaultKind::Compile => {
                if self.rng.chance(40) {
                    self.recursion_fault(scope)
                } else {
                    self.compile_fault(scope)
                }
            }
            FaultKind::DeprecationThenExpr => {
                let first = self.deprecation(scope);
                let e = self.fault_expr(scope);
                let second = self.expr_statement(&e, scope);
                let sep = self.separator(false);
                if self.rng.chance(70) {
                    format!("{first}{sep}{second}")
                } else {
                    format!("{second}{sep}{first}")
                }
            }
            FaultKind::DeprecationThenCompile => {
                let first = self.deprecation(scope);
                let second = if self.rng.chance(50) {
                    self.compile_fault(scope)
                } else {
                    self.binding_fault(scope)
                };
                let sep = self.separator(false);
                format!("{first}{sep}{second}")
            }
            FaultKind::Benign => {
                self.label("benign");
                self.filler(scope)
            }
        }
    }

    /// A statement evaluating the expression `e`.
    fn expr_statement(&mut self, e: &str, scope: &Scope) -> String {
        let sp = self.sp();
        let sp2 = self.sp();
        match self.rng.weighted(&[
            (38, 0),
            (8, 1),
            (7, 2),
            (7, 3),
            (3, 4),
            (5, 5),
            (6, 6),
            (4, 7),
            (4, 8),
            (3, 9),
            (2, 10),
            (3, 11),
            (3, 12),
            (3, 13),
            (2, 14),
        ]) {
            0 => format!("{{{{{sp}{e}{sp2}}}}}"),
            1 => {
                let v = self.fresh("v");
                format!("{{% set {v} ={sp}{e} %}}{{{{ {v} }}}}")
            }
            2 => format!("{{% do{sp}{e} %}}"),
            3 => format!("{{% if{sp}{e} %}}yes{{% else %}}no{{% endif %}}"),
            4 => {
                let cond = self.truthy(scope);
                format!("{{% if not {cond} %}}a{{% elseif{sp}{e} %}}b{{% endif %}}")
            }
            5 => {
                let v = self.fresh("i");
                format!("{{% for {v} in{sp}{e} %}}[{{{{ {v} }}}}]{{% endfor %}}")
            }
            6 => {
                let v = self.fresh("i");
                let inner = scope.with_var(&v, Ty::Int);
                let inner = Scope {
                    loop_var: Some(v.clone()),
                    ..inner
                };
                let e2 = if self.rng.chance(50) {
                    self.fault_expr(&inner)
                } else {
                    e.to_owned()
                };
                format!("{{% for {v} in [1, 2, 3] %}}\n  {{{{ {e2} }}}}\n{{% endfor %}}")
            }
            7 => {
                let v = self.fresh("w");
                format!("{{% with {{{v}:{sp}{e}}} %}}{{{{ {v} }}}}{{% endwith %}}")
            }
            8 => {
                let v = self.fresh("c");
                format!("{{% set {v} %}}\n{{{{ {e} }}}}\n{{% endset %}}{{{{ {v} }}}}")
            }
            9 => format!("{{% apply upper %}}{{{{{sp}{e} }}}}{{% endapply %}}"),
            10 => format!("{{% autoescape 'html' %}}{{{{ {e} }}}}{{% endautoescape %}}"),
            11 => format!("{{% deprecated{sp}{e} %}}"),
            12 => {
                self.part = true;
                format!("{{{{ include('part.twig',{sp}{{v: {e}}}) }}}}")
            }
            13 => {
                self.part = true;
                format!("{{% include 'part.twig' with{sp}{{v: {e}}} %}}")
            }
            _ => format!("{{{{ ({e}){sp}|e }}}}"),
        }
    }

    /// An expression that fails (or raises a diagnostic), wrapped in benign expressions.
    fn fault_expr(&mut self, scope: &Scope) -> String {
        let leaf = self.leaf(scope);
        let levels = self.rng.weighted(&[(40, 0usize), (35, 1), (18, 2), (7, 3)]);
        let mut e = leaf;
        for _ in 0..levels {
            e = self.wrap(&e, scope);
        }
        e
    }

    /// Wraps `inner` in a benign expression.
    fn wrap(&mut self, inner: &str, scope: &Scope) -> String {
        let pattern = self.rng.pick(&[
            "($E)",
            "$S$_~$_$E",
            "$E$_~$_$S",
            "[$A,$_$E]|length",
            "{k:$_$E}|keys|join",
            "($E)|length",
            "($E)|e",
            "($E)|default('d')",
            "$E$_??$_'d'",
            "($E) is empty ? 'E' : 'N'",
            "$B$_?$_$E$_:$_$A",
            "true ? $E : 0",
            "false ? 0 : $E",
            "true and $E",
            "false or $E",
            "[1, 2]|map(x =>$_$E)|join(',')",
            "[1, 2]|filter(x =>$_$E)|length",
            "[1, 2]|reduce((c, x) =>$_c ~ $E, '')",
            "[1, 2]|find(x =>$_$E)",
            "max(1,$_$E)",
            "($E)$_|upper",
            "($E ~ 'x')|trim",
            "'a' ~$_($E)$_~ 'b'",
            "{(key): $E}",
            "$E$_?:$_'f'",
            "($E)|json_encode",
            "not ($E)",
        ]);
        let filled = self.fill(pattern, scope);
        filled.replace("$E", inner)
    }

    /// The failing expression itself; leaves calling `template_from_string()` or `dump()` without
    /// their extension are drawn again (an unknown function is a plain syntax error).
    fn leaf(&mut self, scope: &Scope) -> String {
        let mut leaf = self.leaf_once(scope);
        for _ in 0..8 {
            let missing = (leaf.contains("template_from_string") && !self.string_loader)
                || (leaf.contains("dump(") && !self.debug_ext);
            if !missing {
                break;
            }
            leaf = self.leaf_once(scope);
        }
        leaf
    }

    fn leaf_once(&mut self, scope: &Scope) -> String {
        let mut kinds: Vec<(u32, &'static str)> = vec![
            (10, "undef"),
            (14, "attr"),
            (6, "call"),
            (11, "div"),
            (16, "filter"),
            (10, "func"),
            (6, "tpl"),
            (5, "block"),
            (4, "macro"),
            (6, "ops"),
            (3, "spread"),
            (3, "regex"),
            (7, "phpdep"),
            (3, "const"),
            (8, "warn"),
            (10, "probe"),
            (8, "pos"),
            (10, "order"),
            (5, "edge"),
            (8, "macrocall"),
            (4, "enum"),
            (6, "arrow"),
        ];
        if self.objects {
            kinds.push((10, "obj"));
            kinds.push((4, "lazy"));
        }
        if self.string_loader {
            kinds.push((4, "wrapper"));
        }
        let kind = self.rng.weighted(&kinds);
        self.label(kind);
        if kind == "probe" {
            return self.line_probe(scope);
        }
        if kind == "order" {
            return self.order_probe(scope);
        }
        if kind == "macrocall" {
            return self.macro_call(scope);
        }
        if kind == "arrow" {
            let inner = self.small_leaf(scope);
            let pattern = self.rng.pick(&[
                "$L|map(x =>$_$L|map(y =>$_$E)|join)|join",
                "$L|map(x =>$_x$_~$_$E)|join",
                "$L|filter(x =>$_$V$_and$_$E)|join",
                "$L|reduce((c, x) =>$_c$_~$_$V$_~$_$E,$_'')",
                "$L|sort((a, b) =>$_(a$_<=>$_b)$_~$_$E)|join",
                "$L|find(x =>$_x > 1$_and$_$E)",
                "$L has some x =>$_$V$_?$_$E$_:$_0",
                "[$_1,$_2$_]|map(x =>$_[$_x,$_$E$_])|length",
                "{a: 1}|map((v, k) =>$_k$_~$_$E)|join",
                "$L|map(x => x)$_|map(x =>$_$E)|join",
                "$L|find(x =>$_block($S))",
                "$L|map(x =>$_block($_$S$_, $T))|join",
                "$L|map(x =>$_block('nope'))|join",
                "$L|filter(x =>$_attribute($M,$_$S))|join",
                "$L|map(x =>$_x ~ block($V))|join",
                "$L|map(x =>$__self.nope(x))|join",
                "$L|map(x =>$_include($T))|join",
                "$L|map(x =>$_include($Q))|join",
                "$L|map(x =>$_source($Q))|join",
            ]);
            let filled = self.fill(pattern, scope);
            return filled.replace("$E", &inner);
        }
        if kind == "pos" {
            let inner = self.small_leaf(scope);
            let pattern = self.rng.pick(&[
                "$L|join($E)",
                "$S|replace({(key): $E})",
                "$S|replace({'a':$_$E})",
                "$I is divisible by($E)",
                "$I is same as($E)",
                "{($E): 1}|keys|join",
                "\"a #{$E} b\"",
                "\"line\n#{$E}\"",
                "1..$E",
                "$E..3",
                "$B ? $E : 1",
                "$S|date(format = $E)",
                "range(1,$_$E)",
                "max($_1,$_$E$_)",
                "[$_1,$_$E$_]",
                "{a:$_1,$_b:$_$E}",
                "$L|map(x => x ~ $E)|join",
                "$L|slice(0,$_$E)",
                "$S|trim($_$E$_)",
                "$S|default($E)",
                "$E|default($S)",
                "($E)|default",
                "attribute($M,$_$E)",
                "$M[$E]",
                "$M.($E)",
                "$L|sort((a, b) =>$_$E)|join",
                "$L|reduce((c, v) => c ~ $E, 0)",
                "$L has some x =>$_$E",
                "$E ?? ($E)",
                "cycle($L, $E)",
                "include('part.twig', {v: $E})",
                "constant($E)",
            ]);
            self.part = true;
            let filled = self.fill(pattern, scope);
            return filled.replace("$E", &inner);
        }
        let patterns: &[&str] = match kind {
            "undef" => &[
                "$U",
                "$U.x",
                "$U[0]",
                "$U|upper",
                "$U ~ 'x'",
                "$U + 1",
                "$U.a.b",
                "$U is null ? 1 : 2",
                "$U|default('d')",
                "$U ?? 'd'",
                "$U is defined",
                "not $U",
                "-$U",
                "[$U]",
                "{k: $U}",
                "max($U, 1)",
                "loop.index",
                "_context.$U",
                "attribute($U, 'x')",
                "$U$_|length",
                "$U()",
                "$U.x()",
                "$U|default",
                "$U.y ?? $U",
            ],
            "attr" => &[
                "$M.zz",
                "$M['zz']",
                "$M[$K]",
                "$M.($K)",
                "$M.zz.yy",
                "nested.a.zz",
                "nested.a.b[5]",
                "nested.a.b[1].zz",
                "nested$_.a$_.zz",
                "rows[0].zz",
                "rows[5].id",
                "rows[1]$_.name",
                "$L[9]",
                "$L[-1]",
                "$L.9",
                "$L[$I]",
                "empty[0]",
                "empty.first",
                "$L[1.5]",
                "$L[true]",
                "$L[$N]",
                "$M[$L]",
                "$S.x",
                "$S[0]",
                "$S.length",
                "$I.x",
                "$F.x",
                "$B.x",
                "$N.x",
                "$N[0]",
                "$L.x",
                "$L.length",
                "$M.a.b",
                "$M.keys()",
                "$S.upper()",
                "$N.x()",
                "$I.x(1)",
                "attribute($M, 'zz')",
                "attribute($S, 'x')",
                "attribute($N, 'x')",
                "attribute($M, 'a', [1])",
                "attribute($L, 9)",
                "attribute($M, $L)",
                "$M?.zz",
                "$N?.x",
                "$M.zz is defined ? 1 : $M.zz",
                "_self.zz",
                "_charset.x",
                "loop.zz",
                "$M$_[$_'zz'$_]",
                "$M.a$_.b$_.c",
            ],
            "obj" => &[
                "obj.zz",
                "obj.zz()",
                "obj.a.zz",
                "obj.list[5]",
                "obj.list.zz",
                "ao.zz",
                "ao['zz']",
                "ao.zz()",
                "foo.zz",
                "foo.zz()",
                "foo.getSelf.zz",
                "foo.getSelf$_.getNull$_.x",
                "foo.getNull.x()",
                "foo.strToLower()",
                "foo.strToLower(1, 2)",
                "foo.bar(1, 2, 3)",
                "foo.array[5]",
                "foo.position.x",
                "magic.x",
                "magic.x()",
                "magic.getX",
                "magic$_.anything(1)",
                "str_obj.zz",
                "str_obj ~ $S",
                "countable ~ ''",
                "countable|upper",
                "countable|length",
                "'' ~ countable$_ ~ ''",
                "iter.zz",
                "agg.zz",
                "agg[0]",
                "markup.zz",
                "dt.zz",
                "dt.format()",
                "dt.format('Y', 1)",
                "dt.modify('garbage')",
                "dt.setDate(1)",
                "dt$_.format$_('Y')$_.x",
                "obj ~ ''",
                "obj|upper",
                "obj + 1",
                "ao + 1",
                "foo|join",
                "obj|join(',')",
                "constant('NOPE', foo)",
                "constant('BAR_NAME', obj)",
                "constant('RFC7231', dt)",
                "$O.zz",
                "$O.zz()",
                "$O|first",
                "$O|keys|join",
                "$O|merge([1])",
                "$O|batch(1)",
                "$O|column('a')",
                "$O|reduce((c, v) => c ~ v)",
                "$O|slice(1)|join",
                "[...$O]",
            ],
            "call" => &[
                "$L|map(5)",
                "$L|map($N)",
                "$L|map('no_such_fn')",
                "$L|filter(true)",
                "$L|filter('no_such_fn')",
                "$L|sort(1)",
                "$L|sort('no_such_fn')",
                "$L|reduce(2)",
                "$L|reduce('no_such_fn', 0)",
                "$L|find(3)",
                "$L has some 1",
                "$L has every $N",
                "$L has some 'no_such_fn'",
                "5|invoke",
                "$S|invoke(1)",
                "$N|invoke",
                "$L|map([1, 2])",
                "[1, 2]|map((a, b, c) =>$_a)|join",
                "[1, 2]|map(x => x.zz)|join",
                "[1, 2]|map(x =>$_10 / (x - 1))|join",
                "[1]|map(x => $U)|join",
                "{a: 1}|map((v, k) => k.zz)|join",
            ],
            "div" => &[
                "$I$_/$_$Z",
                "$I$_//$_$Z",
                "$I$_%$_$Z",
                "$F % $Z",
                "0 ** -1",
                "$Z ** -1",
                "$I is divisible by($Z)",
                "($I / $Z)|round",
                "[1, 2, 3]|map(x => 6 / (x - 2))|join(',')",
                "$I / $Z ?? 1",
                "($I$_+$_1)$_/$_$Z",
                "$S|length / $Z",
                "$I // 0.5 // $Z",
                "10 % 3 % $Z",
                "-9223372036854775807 - 1 // -1",
                "big // 0.0001",
                "1e400 // 1",
                "$L|length % $Z",
                "range(1, 3)|reduce((c, x) => c / (x - 3), 1)",
            ],
            "filter" => &[
                "$S|round",
                "$I|round(1, 'up')",
                "$I|round(1, $S)",
                "$I|round('x')",
                "$I|round($F)",
                "$L|batch(0)",
                "$L|batch(-1)",
                "$L|batch('x')",
                "$I|batch(2)",
                "$S|batch(1)",
                "$S|trim(' ', 'middle')",
                "$S|trim($N, $S)",
                "$S|escape('nope')",
                "$S|escape($S)",
                "$S|e('html', 'BAD-CHARSET')",
                "$S|e('js', 'ISO-8859-1')",
                "$S|e('css', 'UTF-16')",
                "$S|e('html_attr_relaxed')",
                "$L|e",
                "$I|date('Y', $W)",
                "$D|date",
                "$D|date('Y')",
                "$S|date_modify('+1 day')",
                "'2020-01-01'|date_modify($D)|date('Y')",
                "$L|date",
                "'@x'|date('Y')",
                "$S|replace(5)",
                "$S|replace($S)",
                "$S|replace($N)",
                "$I|merge([1])",
                "$L|merge(5)",
                "$L|merge($S)",
                "$N|merge($L)",
                "$L|merge($M, $I)",
                "$S|slice('x')",
                "$S|slice(1, 'y')",
                "$L|slice($S)",
                "$I|slice(1)",
                "rows|column('id')|join",
                "$L|column('a')",
                "$S|column('a')",
                "rows|column($L)",
                "$P|format()",
                "'%z'|format(1)",
                "'%2$$s'|format(1)",
                "$P|format($L, 1)",
                "'%d %d %d'|format(1, 2)",
                "$L|json_encode($S)",
                "$L|json_encode(0, 0)",
                "$I|number_format(2, $L)",
                "$I|number_format($S)",
                "$L|join(', ', $L)",
                "$L|join($N)",
                "$S|split('')",
                "$S|split($N)",
                "$S|split(',', $S)",
                "$L|split(',')",
                "$L|abs",
                "$S|abs",
                "$N|abs",
                "$B|abs",
                "$L|upper",
                "$L|lower",
                "$L|title",
                "$L|capitalize",
                "$L|nl2br",
                "$L|striptags",
                "$M|url_encode|upper",
                "$L|convert_encoding('UTF-8', 'BAD')",
                "$S|convert_encoding('BAD', 'UTF-8')",
                "$S|convert_encoding('UTF-8', 'nope')",
                "$L|spaceless",
                "$L|reverse|join|upper",
                "$S|sort",
                "$I|keys",
                "$S|first",
                "$L|last|upper",
                "$L|filter(x => x)|map(x => x.zz)|join",
                "$S|default($U)",
                "$L|format",
                "$M|join(', ')|upper",
                "[[1]]|join",
                "{a: [1]}|join(',')",
            ],
            "func" => &[
                "range(1, 2, 0)",
                "range(1, $I, -1)",
                "range('a', 'z', 0)",
                "range(1, 'x')",
                "range($L, 2)",
                "range(0, 1, 0.0)",
                "range(1, 3, 1.5)|join",
                "range($N, 2)|join",
                "range(1, 2, $Z)",
                "cycle([], 1)",
                "cycle(empty, $I)",
                "cycle($S, 1)",
                "cycle($L, $S)",
                "cycle($L, $N)",
                "cycle($L, $F)",
                "cycle($M, 1)",
                "max()",
                "max([])",
                "max($I)",
                "min(empty)",
                "max($L, $S)",
                "min($M)",
                "max(1, [])",
                "constant('NOPE')",
                "constant('Foo::BAR')",
                "constant($N)",
                "constant($L)",
                "constant('PHP_EOL') ~ constant('NOPE_' ~ $I)",
                "constant('Twig\\\\Token::NOPE')",
                "date($D)",
                "date($I, $W)",
                "date($L)",
                "date('2020-01-01', 5)",
                "date('2020-01-01', $B)|date('e')",
                "random([])",
                "random(empty)",
                "random(5, 1)",
                "random([], $I)",
                "source($Q)",
                "source($N)",
                "source($Q, ignore_missing = true)",
                "source($T)",
                "enum($S)",
                "enum_cases($S)",
                "enum('NoSuchEnum')",
                "enum_cases('stdClass')",
                "template_from_string($S) ~ ''",
            ],
            "tpl" => &[
                "include($Q)",
                "include($N)",
                "include($I)",
                "include($L)",
                "include([])",
                "include(tpls)",
                "include(['nope1.twig', 'nope2.twig'])",
                "include($B)",
                "include($M)",
                "include($Q, ignore_missing = true)",
                "include($T, $S)",
                "include($T, $I)",
                "include($T, $L)",
                "include($T, with_context = false)",
                "include_only($T, $S)",
                "include_only($Q)",
                "include($T, sandboxed = false)",
                "include($T$_,$_{v: 1}$_,$_false)",
                "include(template_from_string('{{ 1 / 0 }}'))",
                "include(template_from_string(\"a\\n{{ $U }}\"))",
                "include(template_from_string('{% if %}'))",
                "include(template_from_string('x', 'named.twig'))",
                "include(template_from_string(\"{{ 'a'\\n|nope }}\"))",
                "include(template_from_string(\"{% include 'nope.twig' %}\"))",
            ],
            "block" => &[
                "block('nope')",
                "block($N)",
                "block($I)",
                "block($L)",
                "block($S)",
                "block('nope', $T)",
                "block('b', $Q)",
                "block('nope') is defined ? 'y' : block('nope')",
                "block($N, $T)",
                "block('nope', 'part.twig') is defined",
                "parent()",
                "block('b', template_from_string('{% block b %}{{ 1 / 0 }}{% endblock %}'))",
            ],
            "macro" => &[
                "_self.nope()",
                "_self.nope",
                "_self.nope() is defined",
                "_self.nope is defined ? 1 : _self.nope()",
                "_self.Nope(1)",
                "_self.macro_nope()",
            ],
            "ops" => &[
                "$M - 1",
                "$L - 1",
                "$S - $I",
                "$I - $L",
                "$S / 2",
                "$L % 2",
                "$N - $L",
                "-$L",
                "$L ~ 'x'",
                "{a: 1} ~ ''",
                "$S..5",
                "$L..3",
                "'a'..'zz'",
                "1..'a'",
                "1 <=> $L",
                "big + 1",
                "-big - 2",
                "$S ** 2",
                "$L ** 2",
                "'5 apples' + 1",
                "$S < $L",
                "$M in 5",
                "$L starts with 'a'",
                "$S ends with $L",
                "'abc' matches $L",
            ],
            "spread" => &[
                "[1, ...$j]",
                "[...$s]",
                "{...$L}",
                "{...$j}",
                "range(...$j)",
                "max(...$s)",
                "[...nul]",
                "[...$M]|join",
                "{...$M, ...$L}|keys|join",
                "range(...[1, 3])|join",
                "[1,$_...$s,$_2]",
            ],
            "regex" => &[
                "$S matches $R",
                "$S matches $S",
                "$S matches $N",
                "$S matches '/(/'",
                "$S matches 'abc'",
                "$S matches '/a/z'",
                "$L matches '/a/'",
                "$S$_matches$_$R",
            ],
            "warn" => &[
                "$L ~ ''",
                "''$_~$_$L",
                "[[1]]|join",
                "{a: {b: 1}}|join(',')",
                "$S matches $R",
                "$S matches $S",
                "'5 apples' - 1",
                "'3x' * 1 - 1",
                "$S|e('html', 'BAD-CHARSET')",
                "$S|convert_encoding('BAD', 'UTF-8')",
                "$S|convert_encoding('UTF-8', 'nope')",
                "\"\\xff\"|convert_encoding('UTF-8', 'ASCII')",
                "rows|join(', ')",
                "rows|first ~ ''",
                "nested ~ ''",
                "$L|map(x => [x])|join",
                "$L|join(', ')$_~$_$L",
                "{(1.5): 1}|keys|join",
                "$S|format(1, 2)",
                "'%s %s'|format($L, $L)",
                "$L|url_encode ~ $L",
                "$L|batch(2)|join",
                "_context|join",
                "loop ?? $L ~ ''",
            ],
            "edge" => &[
                "'1.5' % 1",
                "'2.5' // 1 % 2",
                "$L|sort((a, b) => a > b)|join",
                "$L|sort((a, b) => b < a)|join(',')",
                "nested|json_encode(constant('JSON_THROW_ON_ERROR'), 1)",
                "\"\\xff\"|json_encode(constant('JSON_THROW_ON_ERROR'))",
                "$M.(1)",
                "$M.($N)",
                "$M.($L)",
                "$M.(1.5)",
                "$S matches '/a/e'",
                "$S matches '/(?<n>a)(?<n>b)/'",
                "$S matches '/a{2,1}/'",
                "$S matches '#a'",
                "$S matches ''",
                "cycle($L, -1)",
                "cycle($L, -5)",
                "cycle($L, 1.5)",
                "5 % 0.5",
                "$I % 0.5",
                "big * 2",
                "big + big",
                "(-big - 1) % -1",
                "(-big - 1) // -1",
                "big ** big",
                "-big ** 0.5",
                "range(1, 50)[99]",
                "range(1, 50)|map(x => x.zz)|join",
                "$M['é']",
                "$M.é",
                "{'é': 1, 'ü': 2}.zz",
                "{(1): 'a', '2': 'b', 'c': 3}.zz",
                "{'a.b': 1}.zz",
                "'héllo'.x",
                "'héllo'[9]",
                "map.zz|default('d')",
                "map.zz.yy|default('d')",
                "map.zz(1)|default('d')",
                "$U.x()|default('d')",
                "$L[$U]|default('d')",
                "($M.zz ~ 'a')|default('d')",
                "$M[$K]|default($U)",
                "(1 / $Z) is defined",
                "$M.zz is defined",
                "$M.zz.yy is defined",
                "$U.x is defined",
                "$U[0] is defined",
                "$L[9] is defined",
                "$M?.zz?.yy",
                "$N?.x.y",
                "$U?.x",
                "_context.$U",
                "_context[$K]",
                "_self ~ $U",
                "_charset|upper ~ $U",
                "[1, 2, 3][1:]",
                "'abc'[1:5]",
                "$L[1:$U]",
                "$S[$U:]",
                "\"a #{$M.zz} b\"",
                "'%s'|format($U)",
                "$U|length",
                "$U is empty",
                "$U is null",
                "$U is iterable",
                "$U is same as(null)",
                "not $U is defined",
            ],
            "enum" => &[
                "enum('Twig\\\\Tests\\\\DummyBackedEnum').zz",
                "enum('Twig\\\\Tests\\\\DummyBackedEnum').value ~ enum('Twig\\\\Tests\\\\DummyUnitEnum').value",
                "enum('Twig\\\\Tests\\\\DummyUnitEnum').name.zz",
                "enum('Twig\\\\Tests\\\\DummyBackedEnum') ~ ''",
                "enum('Twig\\\\Tests\\\\DummyBackedEnum')|upper",
                "enum('Twig\\\\Tests\\\\DummyBackedEnum')|json_encode",
                "enum_cases('Twig\\\\Tests\\\\DummyBackedEnum')|join",
                "enum_cases('Twig\\\\Tests\\\\DummyUnitEnum')|map(c => c.zz)|join",
                "enum_cases('Twig\\\\Tests\\\\DummyUnitEnum')|map(c =>$_c.name)|join(',')",
                "constant('Twig\\\\Tests\\\\DummyBackedEnum::FOO').zz",
                "constant('Twig\\\\Tests\\\\DummyBackedEnum::NOPE')",
                "constant('NOPE', enum('Twig\\\\Tests\\\\DummyUnitEnum'))",
                "enum('Twig\\Tests\\DummyBackedEnum')",
                "enum($S)",
                "enum_cases($S ~ 'x')",
                "enum('Twig\\\\Tests\\\\DummyBackedEnum').from('zz')",
                "enum('Twig\\\\Tests\\\\DummyBackedEnum').tryFrom()",
                "enum('Twig\\\\Tests\\\\DummyBackedEnum').cases()|length / $Z",
                "enum('Twig\\\\Tests\\\\DummyUnitEnum') in [1]",
                "enum('Twig\\\\Tests\\\\DummyUnitEnum') + 1",
            ],
            "lazy" => &[
                "agg|filter(v => 1 / $Z)|join",
                "iter|filter(v =>$_v.zz)|join",
                "agg|map(v => v.zz)|join",
                "iter|filter(v => v > 2)|map(v => 10 / (v - 4))|join",
                "agg|find(v => v / $Z)",
                "agg|reduce((c, v) => c ~ v.zz, '')",
                "iter has some v => v.zz",
                "agg has every v =>$_1 / $Z",
                "agg|sort((a, b) => a.zz)|join",
                "foo|filter(v => v.zz)|join",
            ],
            "wrapper" => &[
                "template_from_string('{% block b %}{{ 1 / 0 }}{% endblock %}').renderBlock('b')",
                "template_from_string('{% block b %}x{% endblock %}').renderBlock('nope')",
                "template_from_string('{{ $U }}').render({})",
                "template_from_string('{{ v.zz }}').render({v: {a: 1}})",
                "template_from_string(\"a\n{{ include('nope.twig') }}\").render()",
                "template_from_string('{% block b %}{{ parent() }}{% endblock %}').renderBlock('b')",
                "template_from_string('x').renderBlock()",
                "template_from_string('x').render(5)",
                "template_from_string('x').zz",
                "template_from_string('x').hasBlock()",
                "template_from_string('{% block b %}{{ block(\"c\") }}{% endblock %}').renderBlock('b')",
                "template_from_string('{% macro m() %}{{ 1 / 0 }}{% endmacro %}{{ _self.m() }}').render()",
                "include(template_from_string('{% extends \"nope.twig\" %}'))",
                "include(template_from_string(\"{% block a %}{% endblock %}\\n{{ $U }}\"))",
            ],
            "phpdep" => &[
                "$L[1.5]",
                "$L[$N]",
                "'abc'|slice($F)",
                "$N|abs",
                "range($N, 2)|join",
                "$I % $F",
                "$F is even",
                "$F is odd",
                "$F is divisible by(2)",
                "'abc'|split('', $F)|join",
                "$L|batch($F)|length",
                "$N|round",
                "$S|trim($N)",
                "$N|split(',')|join",
                "$S|split($N)|join",
                "$L|join($N)",
                "$N|nl2br",
                "$N|upper",
                "$N|striptags",
                "$N|title",
                "$N|capitalize",
                "$N|replace({})",
                "$N|json_encode",
                "$N|convert_encoding('UTF-8', 'UTF-8')",
                "$N|url_encode",
                "$N|slice(1)",
                "$N|first",
                "$N|format",
                "$N|number_format",
                "0 ** -1",
                "$Z ** -2",
                "$L|slice(0, $F)|join",
                "{(1.5): 'a'}|keys|join",
                "{($N): 'a'}|keys|join",
                "'%d'|format($F)",
                "$S[$F]",
                "$S|batch($F)|length",
            ],
            _ => &[
                "constant('E_STRICT')",
                "constant('MT_RAND_PHP')",
                "constant('DATE_RFC7231')",
                "constant('DateTimeInterface::RFC7231')",
                "constant('DateTime::RFC7231')",
                "constant('DateTimeImmutable::RFC7231')",
                "constant('SUNFUNCS_RET_STRING')",
                "constant('ASSERT_ACTIVE')",
                "constant('FILE_TEXT')",
                "$I is constant('E_STRICT')",
                "constant('E_STRICT') is defined",
                "constant('E_STRICT')$_ ~ constant('MT_RAND_PHP')",
                "'D, d M Y'|date(constant('DATE_RFC7231'))",
                "constant('E_ALL') ~ constant('E_STRICT')",
            ],
        };
        let pattern = self.rng.pick(patterns);
        self.fill(pattern, scope)
    }

    /// A short failing expression, for argument and operand positions.
    fn small_leaf(&mut self, scope: &Scope) -> String {
        let pattern = self.rng.pick(&[
            "$U",
            "$U.x",
            "$M.zz",
            "$L[9]",
            "$S.x",
            "$N.x",
            "$I / $Z",
            "$I % $Z",
            "$I // $Z",
            "$L ~ ''",
            "range(1, 2, 0)",
            "max([])",
            "cycle([], 1)",
            "constant('NOPE')",
            "$S|round(1, 'up')",
            "$L|batch(0)",
            "$S|escape('nope')",
            "$S|trim(' ', 'middle')",
            "$I|merge([1])",
            "block('nope')",
            "include($Q)",
            "source($Q)",
            "$L|map(5)|join",
            "$S|split('')|join",
            "$P|format()",
            "date($D)",
            "$D|date",
            "_self.nope()",
            "-$L",
            "$M - 1",
            "$S matches '/a/z'",
        ]);
        let filled = self.fill(pattern, scope);
        if filled.contains("part.twig") {
            self.part = true;
        }
        filled
    }

    /// A call of a macro defined in the current template with a random call shape (deprecated
    /// argument shapes, runtime binding errors, case variants, `macro_` prefix, no parentheses,
    /// `defined`), through `_self`, `import` or `from`.
    fn macro_call(&mut self, scope: &Scope) -> String {
        let name = self.fresh("m");
        let params: Vec<&str> = match self.rng.below(5) {
            0 => vec![],
            1 => vec!["a"],
            2 => vec!["a", "b = 2"],
            3 => vec!["a = 'x'", "...rest"],
            _ => vec!["a", "b", "c = null"],
        };
        let body = self.rng.pick(&[
            "[{{ a ?? '-' }}]",
            "{{ varargs|length }}",
            "{{ a|default('d') }}{{ _context|keys|join(',') }}",
            "{{ (rest ?? [])|length }}",
            "{{ a.zz }}",
            "{{ b ?? 'nb' }}{{ c ?? 'nc' }}",
            "{{ _self }}",
        ]);
        let def = format!(
            "{{% macro {name}({}) %}}{body}{{% endmacro %}}",
            params.join(", ")
        );
        self.roots
            .last_mut()
            .expect("a template is being built")
            .push(def);
        let args = self.rng.pick(&[
            "",
            "1",
            "1, 2",
            "1, 2, 3, 4",
            "a = 1",
            "b = 1",
            "zz = 1",
            "a = 1, zz = 2, yy = 3",
            "b = 1, 2",
            "1, a = 2",
            "a = 1, a = 2",
            "...[1, 2]",
            "...{a: 1}",
            "...{zz: 1}",
            "1,$_...[2, 3]",
            "rest = 1",
            "varargs = 1",
            "A = 1",
            "$U",
            "1 / $Z",
        ]);
        let args = self.fill(args, scope);
        let called = match self.rng.below(10) {
            0 => name.to_uppercase(),
            1 => format!("macro_{name}"),
            2 => {
                let mut c = name.clone();
                c.replace_range(0..1, "M");
                c
            }
            _ => name.clone(),
        };
        let form = self.rng.weighted(&[(50, 0), (20, 1), (15, 2), (15, 3)]);
        let (prefix, target) = match form {
            0 => (String::new(), format!("_self.{called}")),
            1 => {
                let alias = self.fresh("me");
                (
                    format!("{{% import _self as {alias} %}}"),
                    format!("{alias}.{called}"),
                )
            }
            2 => (format!("{{% from _self import {name} %}}"), called.clone()),
            _ => {
                let alias = self.fresh("f");
                (
                    format!("{{% from _self import {name} as {alias} %}}"),
                    alias,
                )
            }
        };
        if !prefix.is_empty() {
            self.roots
                .last_mut()
                .expect("a template is being built")
                .push(prefix);
        }
        match self.rng.below(8) {
            0 => target.clone(),
            1 => format!("{target} is defined"),
            2 => format!("{target}({args}) is defined"),
            _ => format!("{target}({args})"),
        }
    }

    /// Two different faults in one expression: the one PHP evaluates first must be reported.
    fn order_probe(&mut self, scope: &Scope) -> String {
        let first = self.small_leaf(scope);
        let mut second = self.small_leaf(scope);
        for _ in 0..4 {
            if second != first {
                break;
            }
            second = self.small_leaf(scope);
        }
        let pattern = self.rng.pick(&[
            "$1 ~ $2",
            "$1$_~$_$2",
            "[$1, $2]",
            "{a: $1, b: $2}",
            "{($1): $2}",
            "{($1): 1, ($2): 2}",
            "max($1, $2)",
            "range(high = $1, low = $2)",
            "range(low = $1, high = $2)",
            "range(step = $1, low = 1, high = $2)",
            "'2020-01-01'|date(timezone = $1, format = $2)",
            "'2020-01-01'|date(format = $1, timezone = $2)",
            "$S|trim(side = $1, character_mask = $2)",
            "$L|join(and = $1, glue = $2)",
            "$L|slice(length = $1, start = $2)",
            "$I|number_format(thousand_sep = $1, decimal = $2)",
            "$1|default($2)",
            "$1|replace({(key): $2})",
            "($1)|join($2)",
            "$1 is same as($2)",
            "$1 is divisible by($2)",
            "$1 in $2",
            "$1 starts with $2",
            "$1 matches $2",
            "$1..$2",
            "$1[$2]",
            "attribute($1, $2)",
            "$1 ? $2 : 0",
            "\"#{$1}-#{$2}\"",
            "cycle($1, $2)",
            "include($1, {v: $2})",
            "_self.nope($1, $2)",
            "[...$1, ...$2]",
            "($1) - ($2)",
            "($1) / ($2)",
            "$1 == $2",
            "$1 <=> $2",
            "$1 and $2",
            "$1 ?: $2",
            "$L|map(x => $1)|join($2)",
            "$L|reduce((c, v) => $1, $2)",
            "($1)|batch($2)",
            "($1)|merge($2)",
        ]);
        self.part = true;
        let filled = self.fill(pattern, scope);
        filled.replace("$1", &first).replace("$2", &second)
    }

    /// A failing operand among benign operands spread over several lines: the markers of the
    /// variables on other lines decide which line `Error::guess()` reports.
    fn line_probe(&mut self, scope: &Scope) -> String {
        let count = self.rng.range(2, 4);
        let at = self.rng.below(count);
        let mut operands: Vec<String> = Vec::new();
        for i in 0..count {
            if i == at {
                operands.push(self.small_leaf(scope));
            } else {
                let pattern = self.rng.pick(&[
                    "$V",
                    "$S",
                    "$I",
                    "'lit'",
                    "$L|length",
                    "$S|upper",
                    "max($I, $V|length)",
                    "($V)",
                    "\"#{$V|length}\"",
                    "$L|map(x =>$_x)|join",
                ]);
                operands.push(self.fill(pattern, scope));
            }
        }
        let mut out = operands[0].clone();
        for operand in &operands[1..] {
            let op = self
                .rng
                .pick(&["~", "~", "==", "!=", "and", "or", "?:", "??", "-", "/"]);
            let before = if self.rng.chance(45) { "\n    " } else { " " };
            let after = if self.rng.chance(25) { "\n    " } else { " " };
            out = if self.rng.chance(30) {
                format!("({out}){before}{op}{after}({operand})")
            } else {
                format!("{out}{before}{op}{after}{operand}")
            };
        }
        out
    }

    /// A statement that fails by itself.
    fn statement_fault(&mut self, scope: &Scope) -> String {
        self.label("stmt");
        self.part = true;
        let sp = self.sp();
        let pattern = self.rng.pick(&[
            "{% with $S %}w{% endwith %}",
            "{% with $I %}w{% endwith %}",
            "{% with $L %}{{ _context|length }}{% endwith %}",
            "{% with $N %}w{% endwith %}",
            "{% with $B only %}w{% endwith %}",
            "{% include $Q %}",
            "{% include $Q ignore missing %}",
            "{% include$_$N %}",
            "{% include $I %}",
            "{% include $L %}",
            "{% include [] %}",
            "{% include tpls %}",
            "{% include ['nope1.twig', 'nope2.twig'] %}",
            "{% include ['nope1.twig', 'nope2.twig'] ignore missing %}",
            "{% include $T with $S %}",
            "{% include $T with $I %}",
            "{% include $T with $L only %}",
            "{% include $T with $N %}",
            "{% include $T with$_{v: $U} %}",
            "{% embed $Q %}{% endembed %}",
            "{% embed $Q ignore missing %}{% endembed %}",
            "{% embed $T with $S %}{% endembed %}",
            "{% import $Q as lib %}{{ lib.f() }}",
            "{% from $Q import f %}{{ f() }}",
            "{% import $T as lib %}{{ lib.nope() }}",
            "{% from $T import nope %}{{ nope() }}",
            "{% from $T import nope as alias %}{{$_alias(1) }}",
            "{% import _self as me %}{{ me.nope() }}",
            "{% import _self as me %}{{ me() }}",
            "{% import _self as me %}{{ me ~ 'x' }}",
            "{% import _self as me %}{{ me.nope.zz }}",
            "{% import _self as me %}{% set me = 1 %}{{ me.nope() }}",
            "{% import $T as lib %}{{ lib|length }}",
            "{% from _self import nope %}{{ nope() }}",
            "{{ block('nope') }}",
            "{{ block($N) }}",
            "{{ block('b', $Q) }}",
            "{{ block('nope', $T) }}",
            "{% for i in $U %}{{ i }}{% endfor %}",
            "{% for k, v in $M %}{{ k.zz }}{% endfor %}",
            "{% for i in $L %}{{ loop.zz }}{% endfor %}",
            "{% for i in $L %}{{ loop.parent.zz }}{% endfor %}",
            "{% set a, b = $L, $U %}{{ a }}",
            "{% do [a, b] = $I %}",
            "{% do {zz} = $M %}{{ zz }}",
            "{% do {a} = $S %}",
            "{% do [a, b] = $S %}",
            "{% do {a, zz: c} = $M %}",
            "{% deprecated $L %}",
            "{% deprecated 'msg' package = $B version = $B %}",
            "{% deprecated 'msg' package = $O %}",
            "{% deprecated $O %}",
            "{% deprecated $M %}",
            "{% deprecated 'msg' version = $I package = 'p' %}",
            "{% deprecated $N %}",
            "{% deprecated $U %}",
            "{% deprecated 'a'$_~$_$U %}",
            "{% set v = $U %}{{ v }}",
            "{% if $U %}a{% endif %}",
            "{% if f %}a{% elseif $U %}b{% endif %}",
            "{{ _self.nope() }}",
            "{% apply escape($S) %}x{% endapply %}",
            "{% apply round(1, 'up') %}5{% endapply %}",
            "{% autoescape 'nope' %}{{ $S }}{% endautoescape %}",
            "{% types {x: 'string'} %}{{ $U }}",
            "{{ $L|map(x =>$_x / 0)|join }}",
            "{% set m = $L|map(x =>$_x.zz) %}{{ m|join }}",
            "{% do include($Q) %}",
            "{% set x = include($Q) %}",
            "{% sandbox %}{% include $Q %}{% endsandbox %}",
            "{% include$_$U %}",
            "{% include 'part' ~$_$U %}",
            "{% include$_'part.twig' with {v: 1 / $Z} %}",
            "{% embed$_$U %}{% endembed %}",
            "{% import$_$U as lib %}",
            "{% from$_$U import f %}",
            "{{ block($_$U$_) }}",
            "{{ block('b',$_$U) }}",
            "{% include $T with {a: 1}$_~$_$U %}",
            "{% set a, b = 1,$_1 / $Z %}{{ a }}",
            "{% set a, b =$_$U, 2 %}",
            "{% for i in 1..$U %}{{ i }}{% endfor %}",
            "{% for i in [1, 2] %}{{ i }}{% else %}{{ $U }}{% endfor %}",
            "{% for i in [] %}{{ i }}{% else %}{{ $U }}{% endfor %}",
            "{% for i in $L %}{% if loop.last %}{{ $U }}{% endif %}{% endfor %}",
            "{% for k, v in {a: 1, b: 0} %}{{ 1 / v }}{% endfor %}",
            "{% for i in $O %}{{ loop.revindex }}{% endfor %}",
            "{% for i in $O %}{{ loop.last ? 'L' : '' }}{{ loop.length }}{% endfor %}",
            "{% for i in $O %}{{ loop.parent.zz }}{% endfor %}",
            "{% for i in $L %}{% for j in $O %}{{ loop.parent.loop.zz }}{% endfor %}{% endfor %}",
            "{% for i in $L %}{{ loop.revindex0 }}{{ loop.zz }}{% endfor %}",
            "{% if f %}a{% elseif f %}b{% elseif$_$U %}c{% endif %}",
            "{% guard function range %}{{ range(1, 2, 0) }}{% endguard %}",
            "{% guard filter nope %}x{% else %}{{ $U }}{% endguard %}",
            "{% apply replace({'a': $U}) %}abc{% endapply %}",
            "{% autoescape$_'js' %}{{ $L ~ '' }}{% endautoescape %}",
            "{{ \"a #{$_$U$_} b\" }}",
            "{{ \"x\n#{1 / $Z}\n\" }}",
            "{{ ('a'\n~ $U) }}",
        ]);
        let filled = self.fill(pattern, scope);
        if self.rng.chance(30) {
            filled.replacen("{% ", &format!("{{%{sp}"), 1)
        } else {
            filled
        }
    }

    /// A recursive macro, block or template that fails at a depth.
    fn recursion_fault(&mut self, scope: &Scope) -> String {
        self.label("rec");
        let leaf = self.small_leaf(scope);
        match self.rng.below(4) {
            0 => {
                let name = self.fresh("r");
                let def = format!(
                    "{{% macro {name}(d) %}}[{{{{ d }}}}\n{{% if d > 0 %}}{{{{ _self.{name}(d - 1) }}}}{{% else %}}{{{{ {leaf} }}}}{{% endif %}}]{{% endmacro %}}"
                );
                self.roots
                    .last_mut()
                    .expect("a template is being built")
                    .push(def);
                format!("{{{{ _self.{name}({}) }}}}", self.rng.range(0, 3))
            }
            1 => {
                let block = self.fresh("rb");
                let d = self.fresh("d");
                format!(
                    "{{% set {d} = {} %}}{{% block {block} %}}<{{% if {d} > 0 %}}{{% set {d} = {d} - 1 %}}\n{{{{ block('{block}') }}}}{{% else %}}{{{{ {leaf} }}}}{{% endif %}}>{{% endblock %}}",
                    self.rng.range(0, 3)
                )
            }
            _ => {
                let name = self.fresh_template("rec");
                let q = twig_string(&name);
                let source = format!(
                    "({{{{ d }}}}\n{{% if d > 0 %}}{{% include {q} with {{d: d - 1}} %}}{{% else %}}{{{{ {leaf} }}}}{{% endif %}})"
                );
                self.templates.push((name, source));
                format!("{{% include {q} with {{d: {}}} %}}", self.rng.range(0, 3))
            }
        }
    }

    /// A compile-time error of a definition or a misplaced tag.
    fn compile_fault(&mut self, scope: &Scope) -> String {
        self.label("compile");
        let name = self.fresh("m");
        let block = self.fresh("b");
        let pattern = self.rng.pick(&[
            "{% macro MACRO(a = $U) %}{% endmacro %}",
            "{% macro MACRO(a = 1 + 1) %}{% endmacro %}",
            "{% macro MACRO(a, a) %}{% endmacro %}",
            "{% macro MACRO(...varargs) %}{% endmacro %}",
            "{% macro MACRO(...rest, b) %}{% endmacro %}",
            "{% block BLOCK %}a{% endblock %}{% block BLOCK %}b{% endblock %}",
            "{{ parent() }}",
            "{% block BLOCK %}{{ parent() }}{% endblock %}",
            "{% block BLOCK %}{% extends 'part.twig' %}{% endblock %}",
            "{% macro MACRO() %}{% extends 'part.twig' %}{% endmacro %}",
            "{% use $S %}",
            "{% use 'part.twig' %}{{ block('nope') }}",
            "{% use 'nope.twig' %}",
            "{% embed 'part.twig' %}text{% endembed %}",
            "{% embed 'part.twig' %}{{ 'x' }}{% endembed %}",
            "{{ enum('NoSuchEnum') }}",
            "{{ enum('stdClass') }}",
            "{{ enum_cases('NoSuchEnum') }}",
            "{% set true = 1 %}",
            "{{ $S|nope_filter }}",
            "{{ nope_function() }}",
            "{{ $S is nope_test }}",
            "{{ 'x' matches '/(/' }}",
            "{% if t %}{% macro MACRO() %}x{% endmacro %}{% endif %}{{ _self.MACRO() }}",
            "{% for i in [1] %}{% import 'part.twig' as p %}{% endfor %}",
            "{% if t %}{% use 'part.twig' %}{% endif %}",
            "{{ _self.MACRO }}{% macro MACRO() %}m{% endmacro %}",
            "{{ _self.MACRO() is defined }}{% macro MACRO() %}m{% endmacro %}",
            "{% macro MACRO() %}a{% endmacro %}{% macro MACRO() %}b{% endmacro %}{{ _self.MACRO() }}",
        ]);
        self.part = true;
        let filled = self.fill(pattern, scope);
        filled.replace("MACRO", &name).replace("BLOCK", &block)
    }

    // -----------------------------------------------------------------------------------------
    // deprecations

    fn deprecation(&mut self, scope: &Scope) -> String {
        let kind = self.rng.weighted(&[
            (10, "spaceless"),
            (14, "tag"),
            (8, "escape"),
            (10, "prec"),
            (6, "macrodep"),
            (4, "root"),
            (4, "incsb"),
            (3, "sbtag"),
            (3, "snake"),
            (10, "phpdep"),
            (6, "const"),
            (4, "guard"),
        ]);
        self.label(&format!("d{kind}"));
        let sp = self.sp();
        match kind {
            "spaceless" => {
                let pattern = self.rng.pick(&[
                    "{{ $S|spaceless }}",
                    "{{ '<b> x </b>'$_|spaceless }}",
                    "{% apply spaceless %}<b>\n  x  </b>{% endapply %}",
                    "{% apply upper|spaceless %}<i> y </i>{% endapply %}",
                    "{{ ($S|spaceless) ~ ('<p> </p>'|spaceless) }}",
                    "{% set v = $S|spaceless %}",
                    "{{ $L|map(x => x|spaceless)|join }}",
                ]);
                self.fill(pattern, scope)
            }
            "tag" => {
                let pattern = self.rng.pick(&[
                    "{% deprecated 'The thing is deprecated.' %}",
                    "{% deprecated 'msg' package = 'acme/pkg' version = '1.2' %}",
                    "{% deprecated 'msg' version = '2.0' %}",
                    "{% deprecated 'msg' package = 'only/package' %}",
                    "{% deprecated$_'a' ~ $S %}",
                    "{% deprecated \"x #{$S}\" %}",
                    "{% deprecated $I %}",
                    "{% deprecated $S %}",
                    "{% deprecated 'percent %s %d' %}",
                    "{% deprecated 'two'$_package = 'p/q'$_version = '3.4' %}",
                    "{% deprecated 'no punctuation' %}",
                    "{% deprecated 'question?' %}",
                    "{% deprecated '' %}",
                    "{% deprecated 'msg' package = $I version = $F %}",
                ]);
                self.fill(pattern, scope)
            }
            "escape" => {
                let pattern = self.rng.pick(&[
                    "{# 'a\\q' #}{{ 'ok' }}",
                    "{% verbatim %}{{ 'a\\q' }}{% endverbatim %}",
                    "{{ \"#{$S}\\q\" }}",
                    "{{ \"\\q#{$S}\\w\" }}",
                    "{{ 'a\\qb\\wc' }}",
                    "{{ '\\\\q' }}",
                    "{{ '\\\\\\q' }}",
                    "{{ \"\\#{$S}\" }}",
                    "{{ \"x\\\ny\" }}",
                    "{{ {'k\\q': 1}|keys|join }}",
                    "{% deprecated 'dep\\q' %}",
                    "{{ $S|replace({'\\q': 'x'}) }}",
                    "{{ 'a\\qb' }}",
                    "{{ \"x\\y\" }}",
                    "{{ 'l1\nl2\\zl3' }}",
                    "{{ '\\a' ~ '\\e' }}",
                    "{{ \"\\8\\9\" }}",
                    "{{ 'ok\\'s' ~ 'bad\\%' }}",
                    "{% set v = 'a\\b' %}",
                    "{{ 'x'$_~$_'\\Q' }}",
                    "{{ \"#{$S}\\w\" }}",
                    "{{ \"a\\'b\" }}",
                    "{{ 'a\\\"b' }}",
                ]);
                self.fill(pattern, scope)
            }
            "prec" => {
                let pattern = self.rng.pick(&[
                    "{{ $N ?? $S ~ $N ?? $S }}",
                    "{{ ($N ?? 'a') ~ 'b' }}",
                    "{{ not $N ?? true }}",
                    "{{ -$I ** 2 }}",
                    "{{ 2 * 3 ~ 'x' }}",
                    "{{ $L|length + 1 ~ 'x' }}",
                    "{{ '42' ~\n1\n+\n41 }}",
                    "{{ -\n1|abs }}",
                    "{{ not\n1 * 2 }}",
                    "{% set v = 1 + 2 ~ 'x' %}",
                    "{% if $N ?? 1 ~ 'a' %}y{% endif %}",
                    "{{ [1 + 1 ~ 'a']|join }}",
                    "{{ max(1 + 1 ~ 'a', 2) }}",
                    "{{ '42' ~ 1 + 41 }}",
                    "{{ 1 + 41 ~ '42' }}",
                    "{{ '42'$_~$_1$_+$_41 }}",
                    "{{ $N ?? 'foo' ~ '_bar' }}",
                    "{{ not 1 * 2 }}",
                    "{{ -1|abs }}",
                    "{{ -$I|abs }}",
                    "{{ $N ?? 1 + 2 }}",
                    "{{ 1 - 2 ~ 'x' }}",
                    "{{ 'x' ~ 3 - 1 }}",
                    "{{ $N ?? $I * 2 }}",
                    "{{ not $B == true }}",
                    "{{ $I$_+$_$I$_~$_$S }}",
                    "{% if not 1 * 0 %}z{% endif %}",
                    "{{ -$F|round }}",
                    "{{ +$I|abs }}",
                ]);
                self.fill(pattern, scope)
            }
            "macrodep" => {
                let name = self.fresh("m");
                let def = format!(
                    "{{% macro {name}(a, b = 2) %}}[{{{{ a }}}}{{{{ b }}}}]{{% endmacro %}}"
                );
                let call = self.rng.pick(&[
                    "_self.NAME(1, 2, 3)",
                    "_self.NAME(1, zz = 3)",
                    "_self.NAME()",
                    "_self.NAME(b = 1)",
                    "_self.NAME(b = 1, 2)",
                    "_self.NAME(1, a = 2)",
                    "_self.NAME_UPPER(1)",
                    "_self.macro_NAME(1)",
                    "_self.NAME",
                    "_self.NAME is defined",
                    "_self.NAME() is defined",
                    "_self.NAME_UPPER is defined",
                    "_self.macro_NAME is defined",
                    "_self.NAME(1,$_2,$_3,$_4)",
                    "_self.NAME($_zz = 1,$_yy = 2)",
                ]);
                let call = call
                    .replace("NAME_UPPER", &name.to_uppercase())
                    .replace("NAME", &name);
                let call = self.fill(&call, scope);
                self.roots
                    .last_mut()
                    .expect("a template is being built")
                    .push(def);
                format!("{{{{{sp}{call} }}}}")
            }
            "root" => {
                let pattern = self.rng.pick(&[
                    "{% if true %}{% macro MACRO() %}x{% endmacro %}{% endif %}",
                    "{% block BLOCK %}{% macro MACRO() %}x{% endmacro %}{% endblock %}",
                    "{% for i in [1] %}{% use 'blocks.twig' %}{% endfor %}",
                    "{% if t %}{% use 'blocks.twig' %}{% endif %}{{ block('shared') }}",
                    "{% if false %}{% extends 'part.twig' %}{% endif %}",
                ]);
                self.part = true;
                let name = self.fresh("m");
                let block = self.fresh("b");
                pattern.replace("MACRO", &name).replace("BLOCK", &block)
            }
            "incsb" => {
                self.part = true;
                if self.sandbox_ext && self.rng.chance(50) {
                    format!("{{{{ include('part.twig',{sp}sandboxed = true) }}}}")
                } else {
                    let pattern = self.rng.pick(&[
                        "{{ include('part.twig', sandboxed = false) }}",
                        "{{ include('part.twig', {}, true, false, false) }}",
                        "{{ include($_'part.twig',$_sandboxed = false$_) }}",
                        "{{ include('part.twig', sandboxed = true) }}",
                    ]);
                    self.fill(pattern, scope)
                }
            }
            "sbtag" => {
                self.part = true;
                if self.sandbox_ext {
                    "{% sandbox %}\n{% include 'part.twig' %}\n{% endsandbox %}".to_owned()
                } else {
                    "{{ 'no sandbox' }}".to_owned()
                }
            }
            "snake" => {
                if self.debug_ext && self.rng.chance(60) {
                    self.rng
                        .pick(&[
                            "{{ dump(fooBar = 1) }}",
                            "{{ dump(1, someValue = 2, other_value = 3) }}",
                            "{{ dump(\n  camelCase = 'x'\n) }}",
                        ])
                        .to_owned()
                } else {
                    let pattern = self.rng.pick(&[
                        "{{ '%s'|format(fooBar = 1) }}",
                        "{{ max(1, fooBar = 2) }}",
                        "{{ [1]|merge([2], fooBar = [3])|join }}",
                        "{{ $L|join(glueValue = ',') }}",
                    ]);
                    self.fill(pattern, scope)
                }
            }
            "guard" => {
                let pattern = self.rng.pick(&[
                    "{% guard filter spaceless %}{{ $S|spaceless }}{% endguard %}",
                    "{% guard filter upper %}a{% else %}{{ $S|spaceless }}{% endguard %}",
                    "{% guard filter nope %}{{ $S|spaceless }}{% else %}b{% endguard %}",
                    "{% guard function nope %}{{ nope($S|spaceless) }}{% else %}{{ 'x'|spaceless }}{% endguard %}",
                    "{% guard test nope %}{{ 'a\\q' }}{% endguard %}",
                    "{% guard filter nope %}{{ '42' ~ 1 + 41 }}{% endguard %}",
                    "{% guard filter upper %}u{% else %}{% deprecated 'never' %}{% endguard %}",
                    "{% guard function range %}{{ _self.nope }}{% endguard %}",
                ]);
                self.fill(pattern, scope)
            }
            "phpdep" => {
                self.label("phpdep");
                let pattern = self.rng.pick(&[
                    "{{ $L[1.5] }}",
                    "{{ $L[$N] }}",
                    "{{ 'abc'|slice($F) }}",
                    "{{ $N|abs }}",
                    "{{ range($N, 2)|join }}",
                    "{{ $I % $F }}",
                    "{{ $F is even ? 'e' : 'o' }}",
                    "{{ 'abc'|split('', $F)|join(',') }}",
                    "{{ $N|upper }}",
                    "{{ $N|nl2br }}",
                    "{{ 0 ** -1 }}",
                    "{{ {(1.5): 'a'}|keys|join }}",
                    "{{ $S|trim($N) }}",
                    "{{ $L|batch($F)|length }}",
                ]);
                self.fill(pattern, scope)
            }
            _ => {
                let pattern = self.rng.pick(&[
                    "{{ constant('E_STRICT') }}",
                    "{{ constant('MT_RAND_PHP') }}",
                    "{{ constant('DATE_RFC7231') }}",
                    "{{ constant('DateTimeInterface::RFC7231') }}",
                    "{{ constant('SUNFUNCS_RET_DOUBLE') }}",
                    "{{ constant('ASSERT_BAIL') }}",
                    "{{ constant('FILE_BINARY') }}",
                    "{{ $I is constant('E_STRICT') ? 'y' : 'n' }}",
                    "{% if constant('E_STRICT') is defined %}d{% endif %}",
                ]);
                self.fill(pattern, scope)
            }
        }
    }

    // -----------------------------------------------------------------------------------------
    // argument binding

    /// A call whose arguments Twig rejects (or accepts) while compiling.
    fn binding_fault(&mut self, scope: &Scope) -> String {
        self.label("bind");
        let mut sigs: Vec<&Sig> = SIGS.iter().collect();
        sigs.retain(|s| {
            (s.name != "template_from_string" || self.string_loader)
                && (s.name != "dump" || self.debug_ext)
        });
        let sig = sigs[self.rng.below(sigs.len())];
        let args = self.binding_args(sig, scope);
        let input = self.binding_input(sig, scope);
        let sp = self.sp();
        let expr = match sig.kind {
            Kind::Filter => {
                if args.is_empty() && self.rng.chance(50) {
                    format!("{input}|{}", sig.name)
                } else {
                    format!("{input}{sp}|{}({args})", sig.name)
                }
            }
            Kind::Function => format!("{}({args})", sig.name),
            Kind::Test => {
                if args.is_empty() {
                    format!("{input} is {}", sig.name)
                } else {
                    format!("{input} is {}({args})", sig.name)
                }
            }
        };
        let expr = if matches!(sig.kind, Kind::Test) {
            format!("({expr}) ? 'yes' : 'no'")
        } else {
            expr
        };
        self.expr_statement(&expr, scope)
    }

    fn binding_input(&mut self, sig: &Sig, scope: &Scope) -> String {
        let pattern = match sig.input {
            Input::Str => "$S",
            Input::List => "$L",
            Input::Int => "$I",
            Input::Date => "'2020-01-02'",
            Input::Any => "$A",
        };
        self.fill(pattern, scope)
    }

    fn binding_args(&mut self, sig: &Sig, scope: &Scope) -> String {
        let values: Vec<String> = sig
            .params
            .iter()
            .map(|p| self.arg_value(p, scope))
            .collect();
        let mut args: Vec<String> = Vec::new();
        let shape = self.rng.weighted(&[
            (12, "fewer"),
            (10, "more"),
            (14, "named"),
            (12, "unknown"),
            (8, "after"),
            (8, "twice"),
            (6, "unknowns"),
            (8, "camel"),
            (6, "upper"),
            (6, "skip"),
            (10, "exact"),
        ]);
        self.label(shape);
        let n = sig.params.len();
        match shape {
            "fewer" => {
                let count = sig.required.saturating_sub(1).min(n);
                args.extend(values.iter().take(count).cloned());
            }
            "more" => {
                args.extend(values.iter().cloned());
                for _ in 0..self.rng.range(1, 2) {
                    args.push(self.fill("$A", scope));
                }
            }
            "named" | "camel" | "upper" => {
                let mut picked: Vec<usize> = (0..n).collect();
                self.rng.shuffle(&mut picked);
                picked.truncate(self.rng.range(sig.required.min(n), n));
                if picked.is_empty() && n > 0 {
                    picked.push(0);
                }
                for i in picked {
                    let name = match shape {
                        "camel" => camel(sig.params[i]),
                        "upper" => sig.params[i].to_uppercase(),
                        _ => sig.params[i].to_owned(),
                    };
                    args.push(format!("{name} = {}", values[i]));
                }
            }
            "unknown" => {
                args.extend(values.iter().take(sig.required).cloned());
                args.push(format!(
                    "{} = 1",
                    self.rng.pick(&["foo", "bar_baz", "zzQux"])
                ));
            }
            "unknowns" => {
                args.extend(values.iter().take(sig.required).cloned());
                args.push("foo = 1".to_owned());
                args.push("bar = 2".to_owned());
            }
            "after" => {
                if n > 0 {
                    let last = n - 1;
                    args.push(format!("{} = {}", sig.params[last], values[last]));
                    args.push(values[0].clone());
                } else {
                    args.push("a = 1".to_owned());
                    args.push("2".to_owned());
                }
            }
            "twice" => {
                if n > 0 {
                    args.push(values[0].clone());
                    args.push(format!("{} = {}", sig.params[0], values[0]));
                } else {
                    args.push("1".to_owned());
                    args.push("value = 2".to_owned());
                }
            }
            "skip" => {
                // a later parameter by name, an earlier optional one left out
                if n >= 2 {
                    args.extend(values.iter().take(sig.required).cloned());
                    let last = n - 1;
                    args.push(format!("{} = {}", sig.params[last], values[last]));
                } else {
                    args.extend(values.iter().cloned());
                }
            }
            _ => {
                args.extend(
                    values
                        .iter()
                        .take(self.rng.range(sig.required.min(n), n))
                        .cloned(),
                );
            }
        }
        // arguments on their own lines move the line of an unknown argument
        if self.rng.chance(30) && !args.is_empty() {
            format!("\n    {}\n", args.join(",\n    "))
        } else {
            let sp = self.sp();
            args.join(&format!(",{sp}"))
        }
    }

    fn arg_value(&mut self, param: &str, scope: &Scope) -> String {
        let pattern = match param {
            "format" => "'Y-m-d'",
            "timezone" => "'UTC'",
            "modifier" => "'+1 day'",
            "from" => "{'a': 'b'}",
            "decimal" | "precision" | "size" | "position" | "limit" | "depth" | "start"
            | "length" | "flags" => "1",
            "decimal_point" | "thousand_sep" => "'.'",
            "method" => "'floor'",
            "to" => "'UTF-8'",
            "allowable_tags" => "'<b>'",
            "character_mask" => "' '",
            "side" => "'left'",
            "glue" | "delimiter" => "','",
            "and" => "' and '",
            "arrow" => "x => x",
            "initial" => "0",
            "fill" | "default" => "'d'",
            "preserve_keys" | "with_context" | "ignore_missing" | "sandboxed" | "check_defined"
            | "autoescape" => "true",
            "name" | "index" => "'id'",
            "strategy" => "'html'",
            "charset" => "'UTF-8'",
            "values" | "arrays" | "variables" | "vars" => "[1, 2]",
            "low" => "1",
            "high" => "3",
            "step" => "1",
            "constant" => "'PHP_EOL'",
            "object" => "null",
            "template" => "'part.twig'",
            "date" => "'2020-01-02'",
            "enum" => "'NoSuchEnum'",
            "value" | "num" => "$I",
            "arguments" => "1",
            _ => "$A",
        };
        if param == "template" {
            self.part = true;
        }
        self.fill(pattern, scope)
    }
}

/// Where a binding fault's callable gets its input.
#[derive(Clone, Copy, Debug)]
enum Input {
    Str,
    List,
    Int,
    Date,
    Any,
}

#[derive(Clone, Copy, Debug)]
enum Kind {
    Filter,
    Function,
    Test,
}

/// A core callable as Twig binds its arguments: parameters after the injected ones and the
/// filtered/tested value, and how many of them are required.
struct Sig {
    kind: Kind,
    name: &'static str,
    params: &'static [&'static str],
    required: usize,
    input: Input,
}

const fn sig(
    kind: Kind,
    name: &'static str,
    params: &'static [&'static str],
    required: usize,
    input: Input,
) -> Sig {
    Sig {
        kind,
        name,
        params,
        required,
        input,
    }
}

const SIGS: &[Sig] = &[
    sig(
        Kind::Filter,
        "date",
        &["format", "timezone"],
        0,
        Input::Date,
    ),
    sig(Kind::Filter, "date_modify", &["modifier"], 1, Input::Date),
    sig(Kind::Filter, "format", &["values"], 0, Input::Str),
    sig(Kind::Filter, "replace", &["from"], 1, Input::Str),
    sig(
        Kind::Filter,
        "number_format",
        &["decimal", "decimal_point", "thousand_sep"],
        0,
        Input::Int,
    ),
    sig(Kind::Filter, "abs", &[], 0, Input::Int),
    sig(
        Kind::Filter,
        "round",
        &["precision", "method"],
        0,
        Input::Int,
    ),
    sig(Kind::Filter, "url_encode", &[], 0, Input::Str),
    sig(
        Kind::Filter,
        "json_encode",
        &["flags", "depth"],
        0,
        Input::Any,
    ),
    sig(
        Kind::Filter,
        "convert_encoding",
        &["to", "from"],
        2,
        Input::Str,
    ),
    sig(Kind::Filter, "title", &[], 0, Input::Str),
    sig(Kind::Filter, "upper", &[], 0, Input::Str),
    sig(
        Kind::Filter,
        "striptags",
        &["allowable_tags"],
        0,
        Input::Str,
    ),
    sig(
        Kind::Filter,
        "trim",
        &["character_mask", "side"],
        0,
        Input::Str,
    ),
    sig(Kind::Filter, "nl2br", &[], 0, Input::Str),
    sig(Kind::Filter, "spaceless", &[], 0, Input::Str),
    sig(Kind::Filter, "join", &["glue", "and"], 0, Input::List),
    sig(
        Kind::Filter,
        "split",
        &["delimiter", "limit"],
        1,
        Input::Str,
    ),
    sig(Kind::Filter, "sort", &["arrow"], 0, Input::List),
    sig(Kind::Filter, "merge", &["arrays"], 0, Input::List),
    sig(
        Kind::Filter,
        "batch",
        &["size", "fill", "preserve_keys"],
        1,
        Input::List,
    ),
    sig(Kind::Filter, "column", &["name", "index"], 1, Input::List),
    sig(Kind::Filter, "filter", &["arrow"], 1, Input::List),
    sig(Kind::Filter, "map", &["arrow"], 1, Input::List),
    sig(
        Kind::Filter,
        "reduce",
        &["arrow", "initial"],
        1,
        Input::List,
    ),
    sig(Kind::Filter, "find", &["arrow"], 1, Input::List),
    sig(Kind::Filter, "reverse", &["preserve_keys"], 0, Input::List),
    sig(Kind::Filter, "length", &[], 0, Input::List),
    sig(
        Kind::Filter,
        "slice",
        &["start", "length", "preserve_keys"],
        1,
        Input::Str,
    ),
    sig(Kind::Filter, "first", &[], 0, Input::List),
    sig(Kind::Filter, "default", &["default"], 0, Input::Any),
    sig(Kind::Filter, "keys", &[], 0, Input::List),
    sig(Kind::Filter, "invoke", &["arguments"], 0, Input::Any),
    sig(
        Kind::Filter,
        "escape",
        &["strategy", "charset", "autoescape"],
        0,
        Input::Str,
    ),
    sig(
        Kind::Filter,
        "e",
        &["strategy", "charset", "autoescape"],
        0,
        Input::Str,
    ),
    sig(Kind::Filter, "raw", &[], 0, Input::Str),
    sig(Kind::Function, "max", &["value", "values"], 1, Input::Any),
    sig(Kind::Function, "min", &["value", "values"], 1, Input::Any),
    sig(
        Kind::Function,
        "range",
        &["low", "high", "step"],
        2,
        Input::Any,
    ),
    sig(
        Kind::Function,
        "constant",
        &["constant", "object", "check_defined"],
        1,
        Input::Any,
    ),
    sig(
        Kind::Function,
        "cycle",
        &["values", "position"],
        2,
        Input::Any,
    ),
    sig(Kind::Function, "date", &["date", "timezone"], 0, Input::Any),
    sig(
        Kind::Function,
        "include",
        &[
            "template",
            "variables",
            "with_context",
            "ignore_missing",
            "sandboxed",
        ],
        1,
        Input::Any,
    ),
    sig(
        Kind::Function,
        "include_only",
        &["template", "variables", "ignore_missing"],
        1,
        Input::Any,
    ),
    sig(
        Kind::Function,
        "source",
        &["name", "ignore_missing"],
        1,
        Input::Any,
    ),
    sig(Kind::Function, "enum", &["enum"], 1, Input::Any),
    sig(Kind::Function, "enum_cases", &["enum"], 1, Input::Any),
    sig(
        Kind::Function,
        "template_from_string",
        &["template", "name"],
        1,
        Input::Any,
    ),
    sig(Kind::Function, "dump", &["vars"], 0, Input::Any),
    sig(Kind::Test, "empty", &[], 0, Input::Any),
    sig(Kind::Test, "iterable", &[], 0, Input::Any),
    sig(Kind::Test, "sequence", &[], 0, Input::Any),
    sig(Kind::Test, "divisible by", &["num"], 1, Input::Int),
    sig(Kind::Test, "same as", &["value"], 1, Input::Any),
    sig(Kind::Test, "even", &[], 0, Input::Int),
    sig(Kind::Test, "null", &[], 0, Input::Any),
    sig(Kind::Test, "defined", &[], 0, Input::Any),
    sig(
        Kind::Test,
        "constant",
        &["constant", "object"],
        1,
        Input::Any,
    ),
];

/// `snake_case` to `camelCase`.
fn camel(name: &str) -> String {
    let mut out = String::new();
    let mut upper = false;
    for c in name.chars() {
        if c == '_' {
            upper = true;
        } else if upper {
            out.extend(c.to_uppercase());
            upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// A Twig single-quoted string literal of `text`.
fn twig_string(text: &str) -> String {
    let mut out = String::from("'");
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            other => out.push(other),
        }
    }
    out.push('\'');
    out
}

/// The data of a `data_json` case.
fn data_json() -> Json {
    let text = format!(
        "{{{}}}",
        DATA.iter()
            .map(|(name, _, _, json)| format!("\"{name}\": {json}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    Json::parse(&text).expect("the data table is valid JSON")
}

/// The data of a `data_php` case: the JSON data plus the objects.
fn data_php() -> String {
    let json = format!(
        "{{{}}}",
        DATA.iter()
            .map(|(name, _, _, json)| format!("\"{name}\": {json}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let objects: Vec<String> = OBJECTS
        .iter()
        .map(|(name, php)| format!("'{name}' => {php}"))
        .collect();
    format!(
        "return array_merge(json_decode('{}', true), [{}]);",
        json.replace('\\', "\\\\").replace('\'', "\\'"),
        objects.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_and_well_formed() {
        let a = generate(7, 50);
        let b = generate(7, 50);
        assert_eq!(a, b);
        for case in &a {
            let templates = case.get("templates").expect("templates");
            assert!(templates.get("index.twig").is_some());
            assert_eq!(case.get("env").and_then(Json::as_str), Some("plain"));
            let id = case.get("id").and_then(Json::as_str).unwrap_or_default();
            assert!(id.starts_with("errors-7-"), "{id}");
        }
        assert_ne!(generate(8, 5), generate(7, 5));
    }

    #[test]
    fn placeholders() {
        let mut g = Gen::new(1, 1);
        let scope = g.top_scope();
        let filled = g.fill("$$ $U $Z", &scope);
        assert!(filled.starts_with("$ "), "{filled}");
        assert_eq!(camel("decimal_point"), "decimalPoint");
        assert_eq!(twig_string("a'b\\c"), "'a\\'b\\\\c'");
    }
}
