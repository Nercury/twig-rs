//! Generator of the `structure` family: the tags that shape a template more than they print.
//!
//! Every case renders a template whose statements nest these tags at random, with text, prints,
//! `if` and `for` around them:
//!
//! - `{% types %}` with braces or without, optional entries (`?:`), any type strings, trailing
//!   commas, entries over several lines with inline (`#`) and documentation (`##`) comments
//!   between them, and now and then a syntax error (missing colon or comma, a string or number as
//!   the name, `? :`, which lexes as one operator, an unclosed mapping);
//! - `{% guard function|filter|test name %}` (also two-word tests) for callables that exist, exist
//!   only with an extension of the case (`dump`, `template_from_string`), or do not exist, with and
//!   without `{% else %}`; the dropped branch holds unknown callables, blocks, macros and nested
//!   guards (what it defines is discarded), and invalid guard types;
//! - `{% deprecated %}` with computed messages, `package` and `version` of every type (`'0'` and `''`
//!   drop the "Since" prefix), inside loops, macros, blocks, included and child templates;
//! - `{% with %}` with and without variables, `only`, non-mapping values, globals;
//! - `{% do %}` with assignments, destructuring, calls whose result is dropped, failing expressions;
//! - `{% flush %}` (with `use_yield` too);
//! - documentation comments (`{## ##}`, `{## #}`) before blocks, macros, prints and tags, and
//!   inline `##` comments inside tags;
//! - `{% apply %}` with chains of filters (escaping, case, `format`, `split`/`join`, `default`,
//!   `json_encode`, `spaceless`, `length`, ...), nested, under autoescaping;
//! - macros defined below the template root (deprecated) and blocks defined inside other tags;
//! - child templates whose root holds these tags next to their blocks.
//!
//! The data is JSON; `globals`, `strict_variables`, `autoescape`, `debug`, `use_yield`, the
//! `debug`/`string_loader` extensions and warnings turned into exceptions vary. Now and then the
//! environment is sandboxed globally with a policy that allows the tags, filters, functions and
//! tests the templates name (scanned from their sources, plus the `escape` filter the escaper
//! adds), sometimes minus one of them. No block calls a block that may enclose it (Twig recurses
//! until PHP runs out of memory, twig-rs stops at its depth limit: `reference/deviations.md`).
//!
//! [`generate`] is deterministic: case `i` of seed `s` depends only on `(s, i)` ([`case`]). Ids
//! are `struct-<seed>-<i>`. [`batch`] runs every case in a PHP process of its own.

use std::fmt::Write as _;

use super::casekit::{self, Builder, Rng};
use crate::kit::json::Json;

/// The family name.
pub const FAMILY: &str = "structure";

const SALT: u64 = 0x0053_5452_5543;

/// `count` cases of `seed`, as a batch (process isolation).
pub fn batch(seed: u64, count: usize) -> Json {
    casekit::batch(generate(seed, count))
}

/// `count` cases of `seed`.
pub fn generate(seed: u64, count: usize) -> Vec<Json> {
    (0..count as u64).map(|i| case(seed, i)).collect()
}

/// Case `index` of `seed` (id `struct-<seed>-<index>`).
pub fn case(seed: u64, index: u64) -> Json {
    let rng = Rng::new(casekit::case_seed(seed, index, SALT));
    Gen::new(rng).build(format!("struct-{seed}-{index}"))
}

/// Function names for `guard`: (name, an expression using it).
const FUNCTIONS: &[(&str, &str)] = &[
    ("range", "range(1, 3)|join"),
    ("max", "max(1, n)"),
    ("cycle", "cycle(['a', 'b'], n)"),
    ("include", "include('inc.twig')"),
    ("source", "source('inc.twig')|length"),
    ("constant", "constant('PHP_INT_SIZE')"),
    ("attribute", "attribute(h, 'a')"),
    // (never a block that may enclose the guard: Twig recurses until PHP runs out of memory)
    ("block", "block('missing') ?? 'nb'"),
    ("dump", "dump(n)"),
    (
        "template_from_string",
        "include(template_from_string('[{{ n }}]'))",
    ),
    ("parent", "'p'"),
    ("enum_cases", "'e'"),
    ("nope", "nope()"),
    ("rangex", "rangex(1)"),
    ("Range", "Range(1, 2)|join"),
    ("min", "min(l)"),
];

/// Filter names for `guard`.
const FILTERS: &[(&str, &str)] = &[
    ("upper", "s|upper"),
    ("escape", "s|escape('js')"),
    ("e", "html|e"),
    ("raw", "html|raw"),
    ("spaceless", "' <b> </b> '|spaceless"),
    ("invoke", "(x => x ~ '!')|invoke(n)"),
    ("default", "z|default('dflt')"),
    ("json_encode", "h|json_encode"),
    ("nope", "s|nope"),
    ("Upper", "s|Upper"),
    ("format", "'%s-%s'|format(n, s)"),
    ("column", "[h]|column('a')|join"),
];

/// Test names for `guard` (the guard text and an expression using the test).
const TESTS: &[(&str, &str)] = &[
    ("defined", "n is defined"),
    ("same as", "n is same as(3)"),
    ("divisible by", "n is divisible by(3)"),
    ("iterable", "l is iterable"),
    ("sequence", "l is sequence"),
    ("mapping", "h is mapping"),
    ("empty", "e is empty"),
    ("null", "z is null"),
    ("none", "z is none"),
    ("even", "n is even"),
    ("odd", "n is odd"),
    ("constant", "n is constant('PHP_INT_SIZE')"),
    ("same", "n is same(3)"),
    ("divisible", "n is divisible(3)"),
    ("nope", "n is nope"),
    ("Defined", "n is Defined"),
];

struct Gen {
    rng: Rng,
    counter: usize,
    blocks: Vec<String>,
    macros: Vec<String>,
    uses_inc: bool,
    has_debug: bool,
    has_loader: bool,
    in_child: bool,
}

impl Gen {
    fn new(rng: Rng) -> Gen {
        Gen {
            rng,
            counter: 0,
            blocks: Vec::new(),
            macros: Vec::new(),
            uses_inc: false,
            has_debug: false,
            has_loader: false,
            in_child: false,
        }
    }

    fn fresh(&mut self, prefix: &str) -> String {
        self.counter += 1;
        format!("{prefix}{}", self.counter)
    }

    fn build(mut self, id: String) -> Json {
        let mut b = Builder::new(id);
        self.has_debug = self.rng.chance(40);
        self.has_loader = self.rng.chance(50);
        let child = self.rng.chance(20);
        let main = if child {
            self.in_child = true;
            let source = self.child();
            b.template(
                "base.twig",
                "<base>{% block content %}[base content]{% endblock %}|{% block side %}[side]{% endblock %}</base>",
            );
            b.main = Some("child.twig".to_owned());
            source
        } else {
            self.body(0)
        };
        // an include at the end now and then (not in a child template: content outside blocks)
        let extra_include = !child && !self.uses_inc && self.rng.chance(20);
        let main = if extra_include {
            format!("{main}{{{{ include('inc.twig') }}}}")
        } else {
            main
        };
        if self.uses_inc || extra_include {
            let inc = self.inc();
            b.template("inc.twig", inc);
        }
        let main_name = b.main.clone().unwrap_or_else(|| "index.twig".to_owned());
        b.template(&main_name, main);
        b.data_json = vec![
            ("n".to_owned(), Json::int(3)),
            ("s".to_owned(), Json::str("str")),
            (
                "l".to_owned(),
                Json::Array(vec![Json::int(1), Json::int(2), Json::int(3)]),
            ),
            (
                "h".to_owned(),
                Json::Object(vec![
                    ("a".to_owned(), Json::int(1)),
                    ("b".to_owned(), Json::int(2)),
                ]),
            ),
            ("e".to_owned(), Json::str("")),
            ("z".to_owned(), Json::Null),
            ("html".to_owned(), Json::str("<b>x</b>")),
        ];
        if self.rng.chance(30) {
            b.globals.push(("g".to_owned(), Json::str("G")));
        }
        b.option("strict_variables", Json::Bool(self.rng.chance(50)));
        let autoescape = self
            .rng
            .weighted(&[(45, "false"), (45, "html"), (10, "js")]);
        b.option(
            "autoescape",
            if autoescape == "false" {
                Json::Bool(false)
            } else {
                Json::str(autoescape)
            },
        );
        if self.has_debug {
            b.extensions.push("debug");
            if self.rng.chance(70) {
                b.option("debug", Json::Bool(true));
            }
        }
        if self.has_loader {
            b.extensions.push("string_loader");
        }
        if self.rng.chance(15) {
            b.option("use_yield", Json::Bool(true));
        }
        if self.rng.chance(15) {
            let sources: Vec<String> = b
                .templates
                .iter()
                .map(|(_, s)| String::from_utf8_lossy(s).into_owned())
                .collect();
            let policy = self.policy(&sources);
            b.extensions.push("sandbox");
            b.sandbox = Some(Json::Object(vec![
                ("global".to_owned(), Json::Bool(true)),
                ("policy".to_owned(), policy),
            ]));
        }
        b.throw = self.rng.chance(15);
        b.build()
    }

    /// A security policy allowing what the templates use (scanned from their sources), now and
    /// then minus one name.
    fn policy(&mut self, sources: &[String]) -> Json {
        let (mut tags, mut filters, mut functions, tests) = used_names(sources);
        // the escaper inserts `escape` filters, which the sandbox checks too
        if !filters.iter().any(|f| f == "escape") {
            filters.push("escape".to_owned());
        }
        if self.rng.chance(35) {
            let list = match self.rng.below(3) {
                0 => &mut tags,
                1 => &mut filters,
                _ => &mut functions,
            };
            if !list.is_empty() {
                let i = self.rng.below(list.len());
                list.remove(i);
            }
        }
        let names = |list: Vec<String>| Json::Array(list.into_iter().map(Json::str).collect());
        Json::Object(vec![
            ("tags".to_owned(), names(tags)),
            ("filters".to_owned(), names(filters)),
            ("methods".to_owned(), Json::Object(Vec::new())),
            ("properties".to_owned(), Json::Object(Vec::new())),
            ("functions".to_owned(), names(functions)),
            ("tests".to_owned(), names(tests)),
            ("strict".to_owned(), Json::Bool(self.rng.chance(50))),
        ])
    }

    // ---- bodies ---------------------------------------------------------------------------

    fn body(&mut self, depth: usize) -> String {
        let mut out = String::new();
        let n = if depth == 0 {
            self.rng.range(2, 6)
        } else {
            self.rng.range(1, 3)
        };
        for _ in 0..n {
            if self.rng.chance(40) {
                out.push_str(self.rng.pick_str(&["\n", " ", "|", "\n  ", "-"]));
            }
            self.statement(&mut out, depth);
        }
        out
    }

    fn statement(&mut self, out: &mut String, depth: usize) {
        let nested = depth < 3;
        let kind = self.rng.weighted(&[
            (12, 0u8),
            (if nested { 12 } else { 2 }, 1),
            (10, 2),
            (if nested { 7 } else { 0 }, 3),
            (8, 4),
            (3, 5),
            (6, 6),
            (if nested { 9 } else { 0 }, 7),
            (if nested { 5 } else { 0 }, 8),
            (if nested { 4 } else { 0 }, 9),
            (10, 10),
            (if nested { 5 } else { 0 }, 11),
            (if nested { 4 } else { 0 }, 12),
            (3, 13),
            (3, 14),
        ]);
        match kind {
            0 => self.types(out),
            1 => self.guard(out, depth),
            2 => self.deprecated(out),
            3 => self.with(out, depth),
            4 => self.do_(out),
            5 => {
                let doc = self.maybe_doc();
                let _ = write!(out, "{doc}{{% flush %}}");
            }
            6 => self.doc_comment(out),
            7 => self.apply(out, depth),
            8 => self.block(out, depth),
            9 => self.macro_(out, depth),
            10 => self.print(out),
            11 => {
                let cond = self
                    .rng
                    .pick_str(&["n", "e", "false", "n > 1", "z is null", "l"]);
                let _ = write!(out, "{{% if {cond} %}}");
                let inner = self.body(depth + 1);
                out.push_str(&inner);
                if self.rng.chance(40) {
                    out.push_str("{% else %}");
                    let inner = self.body(depth + 1);
                    out.push_str(&inner);
                }
                out.push_str("{% endif %}");
            }
            12 => {
                let seq = self.rng.pick_str(&["l", "h", "[]", "1..2", "h|keys"]);
                let _ = write!(out, "{{% for item in {seq} %}}[{{{{ item }}}}]");
                let inner = self.body(depth + 1);
                out.push_str(&inner);
                out.push_str("{% endfor %}");
            }
            13 => {
                self.uses_inc = true;
                let form = self.rng.pick_str(&[
                    "{% include 'inc.twig' %}",
                    "{{ include('inc.twig') }}",
                    "{% include 'inc.twig' with {n: 7} only %}",
                    "{{ include('inc.twig', {s: 'inc'}) }}",
                ]);
                out.push_str(form);
            }
            _ => {
                let name = self.fresh("v");
                let expr = self.value();
                let _ = write!(out, "{{% set {name} = {expr} %}}");
            }
        }
    }

    fn value(&mut self) -> String {
        self.rng
            .pick_str(&[
                "n",
                "s",
                "l",
                "h",
                "n + 1",
                "s|upper",
                "l|join(',')",
                "h|keys",
                "'<i>'",
                "html",
                "z",
                "e",
            ])
            .to_owned()
    }

    fn print(&mut self, out: &mut String) {
        let doc = self.maybe_doc();
        if let Some(block) = self.blocks.last().cloned().filter(|_| self.rng.chance(15)) {
            let _ = write!(out, "{doc}[{{{{ block('{block}') }}}}]");
            return;
        }
        if let Some(name) = self.macros.first().cloned().filter(|_| self.rng.chance(15)) {
            let _ = write!(
                out,
                "{doc}[{{{{ _self.{name}('again') }}}}|{{{{ _self.{name} is defined ? 'D' : 'U' }}}}]"
            );
            return;
        }
        let expr = self.rng.pick_str(&[
            "n",
            "s",
            "html",
            "l|join(',')",
            "h|json_encode",
            "_context|keys|join(',')",
            "g ?? 'no-g'",
            "x ?? 'no-x'",
            "v1 ?? 'no-v1'",
            "v2 ?? 'no-v2'",
            "d ?? 'no-d'",
            "a ?? 'no-a'",
        ]);
        let _ = write!(out, "{doc}[{{{{ {expr} }}}}]");
    }

    fn maybe_doc(&mut self) -> String {
        if self.rng.chance(25) {
            self.doc_text()
        } else {
            String::new()
        }
    }

    fn doc_text(&mut self) -> String {
        let body = self.rng.pick_str(&[
            " the doc ",
            "\n * line one\n * line two\n ",
            "",
            " # ",
            " {{ not a print }} ",
            "@var int n",
            " ## ",
        ]);
        let close = self
            .rng
            .pick_str(&["##}", "#}", " ##}", "-##}", "~#}", "-#}"]);
        let open = self.rng.pick_str(&["{##", "{##", "{##-", "{##~"]);
        format!("{open}{body}{close}")
    }

    fn doc_comment(&mut self, out: &mut String) {
        if self.rng.chance(20) {
            out.push_str(
                self.rng
                    .pick_str(&["{##}", "{# plain #}", "{###}", "{## ##}"]),
            );
            return;
        }
        let doc = self.doc_text();
        out.push_str(&doc);
    }

    // ---- types ----------------------------------------------------------------------------

    fn types(&mut self, out: &mut String) {
        let doc = self.maybe_doc();
        let n = self.rng.range(0, 4);
        let mut entries = Vec::new();
        for _ in 0..n {
            entries.push(self.type_entry());
        }
        let broken = self.rng.chance(5);
        let sep = self.rng.pick_str(&[
            ", ",
            ",",
            ",\n",
            " ,\n  ",
            ", # comment\n",
            ",\n## doc of the next\n",
        ]);
        let mut inner = entries.join(sep);
        if !entries.is_empty() && self.rng.chance(25) {
            inner.push(',');
        }
        if broken {
            inner = match self.rng.below(9) {
                0 => "a 'int'".to_owned(),
                1 => "a: 'int' b: 'int'".to_owned(),
                2 => "'a': 'int'".to_owned(),
                3 => "1: 'int'".to_owned(),
                4 => "a: int".to_owned(),
                5 => "a?? 'int'".to_owned(),
                6 => "a: 'int',,".to_owned(),
                // `? :` lexes as one operator (the spaced form of `?:`), which `types` rejects
                7 => "a ? : 'int'".to_owned(),
                _ => "a: 'int', : 'x'".to_owned(),
            };
        }
        let enclosed = self.rng.chance(75);
        let body = if enclosed {
            let close = if broken && self.rng.chance(20) {
                ""
            } else {
                "}"
            };
            let pad = self.rng.pick_str(&["", " ", "\n"]);
            format!("{{{pad}{inner}{pad}{close}")
        } else {
            inner
        };
        let _ = write!(out, "{doc}{{% types {body} %}}");
    }

    fn type_entry(&mut self) -> String {
        let name = self.rng.pick_str(&[
            "n", "s", "foo", "_x", "a", "b", "true", "null", "foo_bar", "x1", "n",
        ]);
        let optional = self.rng.weighted(&[
            (65, ": "),
            (15, "?: "),
            (8, "?:"),
            (5, "?:\n"),
            (4, " :"),
            (3, " ?: "),
        ]);
        let ty = self.rng.pick_str(&[
            "'int'",
            "'string'",
            "'string|null'",
            "'\\\\App\\\\User'",
            "''",
            "\"bool\"",
            "'array<int, string>'",
            "'Σ'",
            "'?float'",
            "'mixed'",
        ]);
        let doc = if self.rng.chance(15) {
            "## doc of the entry\n"
        } else {
            ""
        };
        format!("{doc}{name}{optional}{ty}")
    }

    // ---- guard ----------------------------------------------------------------------------

    fn guard(&mut self, out: &mut String, depth: usize) {
        let kind = self
            .rng
            .weighted(&[(40, "function"), (30, "filter"), (29, "test"), (1, "")]);
        let (name, usage) = match kind {
            "function" => *self.rng.pick(FUNCTIONS),
            "filter" => *self.rng.pick(FILTERS),
            "test" => *self.rng.pick(TESTS),
            _ => *self
                .rng
                .pick(&[("macro", "'m'"), ("tag", "'t'"), ("Function", "'F'")]),
        };
        let (kind, name) = if kind.is_empty() {
            (name, "upper")
        } else {
            (kind, name)
        };
        let usage = if usage.contains("include(") || usage.contains("source(") {
            self.uses_inc = true;
            usage
        } else {
            usage
        };
        // whether the guarded callable exists (unknown callables go only into the dropped branch;
        // a few land in the taken one, which then fails to compile)
        let exists = !matches!(
            name,
            "nope" | "rangex" | "Range" | "Upper" | "same" | "divisible" | "Defined"
        ) && (name != "dump" || self.has_debug)
            && (name != "template_from_string" || self.has_loader);
        let unknown_here = |taken: bool, rng: &mut Rng| !taken || rng.chance(3);
        let doc = self.maybe_doc();
        let _ = write!(out, "{doc}{{% guard {kind} {name} %}}");
        // the branch taken when the callable exists
        let _ = write!(out, "<yes:{{{{ {usage} }}}}>");
        if self.rng.chance(40) {
            let inner = self.body(depth + 1);
            out.push_str(&inner);
        }
        if self.rng.chance(60) {
            out.push_str("{% else %}");
            out.push_str("<no>");
            if self.rng.chance(50) && unknown_here(!exists, &mut self.rng) {
                match self.rng.below(5) {
                    0 => out.push_str("{{ nope() }}"),
                    1 => out.push_str("{{ n|nope }}"),
                    2 => out.push_str("{% if n is nope %}{% endif %}"),
                    3 => out.push_str("{{ n|upper }}"),
                    _ => {
                        let name = self.fresh("dropped");
                        let _ = write!(
                            out,
                            "{{% block {name} %}}D{{% endblock %}}({{{{ block('{name}') ?? 'nb' }}}})"
                        );
                    }
                }
            }
            if self.rng.chance(40) {
                let inner = self.body(depth + 1);
                out.push_str(&inner);
            }
        } else if self.rng.chance(30) && unknown_here(exists, &mut self.rng) {
            // unknown callables in the taken branch fail to compile unless the branch is dropped
            match self.rng.below(3) {
                0 => out.push_str("{{ nope() }}"),
                1 => out.push_str("{{ n|nope }}"),
                _ => {
                    let name = self.fresh("guarded");
                    let _ = write!(out, "{{% block {name} %}}G{{% endblock %}}");
                }
            }
        }
        out.push_str("{% endguard %}");
    }

    // ---- deprecated -----------------------------------------------------------------------

    fn deprecated(&mut self, out: &mut String) {
        let message = self.rng.weighted(&[
            (20, "'msg'"),
            (8, "\"msg #{n}\""),
            (8, "'a' ~ n"),
            (4, "n"),
            (3, "1.5"),
            (3, "null"),
            (3, "true"),
            (2, "l"),
            (3, "html"),
            (2, "''"),
            (2, "'%s %d'"),
            (2, "s|upper"),
        ]);
        let mut tag = format!("deprecated {message}");
        let options = self.rng.below(5);
        let value = |rng: &mut Rng| {
            rng.weighted(&[
                (20, "'acme/pkg'"),
                (10, "'1.2'"),
                (5, "'0'"),
                (5, "''"),
                (4, "n"),
                (2, "null"),
                (2, "l"),
                (2, "1.5"),
                (2, "true"),
                (2, "s ~ '/x'"),
                (1, "h"),
            ])
        };
        match options {
            1 => {
                let p = value(&mut self.rng);
                let _ = write!(tag, " package={p}");
            }
            2 => {
                let v = value(&mut self.rng);
                let _ = write!(tag, " version={v}");
            }
            3 => {
                let (p, v) = (value(&mut self.rng), value(&mut self.rng));
                let _ = write!(tag, " package={p} version={v}");
            }
            4 => {
                let (p, v) = (value(&mut self.rng), value(&mut self.rng));
                if self.rng.chance(3) {
                    let _ = write!(tag, " since={p}");
                } else {
                    let _ = write!(tag, " version={v} package={p}");
                }
            }
            _ => {}
        }
        let doc = self.maybe_doc();
        let _ = write!(out, "{doc}{{% {tag} %}}");
    }

    // ---- with, do, apply ------------------------------------------------------------------

    fn with(&mut self, out: &mut String, depth: usize) {
        let vars = self.rng.weighted(&[
            (15, ""),
            (20, " {a: 1}"),
            (12, " {a: 1} only"),
            (8, " h"),
            (8, " h only"),
            (4, " l only"),
            (1, " n"),
            (1, " z"),
            (3, " {n: 'shadow', g: 'shadow-g'} only"),
            (3, " _context"),
            (3, " _context only"),
            (1, " 'abc'"),
            (2, " h|merge({c: 3})"),
        ]);
        let _ = write!(out, "{{% with{vars} %}}[{{{{ _context|keys|join(',') }}}}]");
        let inner = self.body(depth + 1);
        out.push_str(&inner);
        out.push_str("{% endwith %}");
    }

    fn do_(&mut self, out: &mut String) {
        let expr = self.rng.weighted(&[
            (10, "1 + 1"),
            (8, "x = 5"),
            (5, "[a, b] = [1, 2]"),
            (4, "{a, b} = h"),
            (6, "include('inc.twig')"),
            (5, "s|upper"),
            (4, "n.missing"),
            (3, "1 // 0"),
            (4, "range(1, 3)|map(x => x * 2)"),
            (3, "block('missing') ?? 'nb'"),
            (3, "dump(n)"),
            (3, "x = (d = 2) + 1"),
            (2, "[1, 2]|join(',')"),
            (2, "l[5]"),
            (2, "_self"),
            (2, "nope()"),
            (2, "m1 is defined"),
        ]);
        let expr = if expr.contains("dump(") && !self.has_debug {
            "n"
        } else {
            expr
        };
        if expr.contains("include(") {
            self.uses_inc = true;
        }
        let doc = self.maybe_doc();
        let _ = write!(out, "{doc}{{% do {expr} %}}");
        if expr.contains('=') && self.rng.chance(60) {
            out.push_str("[{{ x ?? '-' }},{{ a ?? '-' }},{{ b ?? '-' }},{{ d ?? '-' }}]");
        }
    }

    fn apply(&mut self, out: &mut String, depth: usize) {
        let n = self.rng.range(1, 3);
        let mut chain = Vec::new();
        for _ in 0..n {
            chain.push(self.rng.weighted(&[
                (10, "upper"),
                (6, "lower"),
                (6, "trim"),
                (6, "e"),
                (4, "e('html')"),
                (4, "escape('js')"),
                (6, "raw"),
                (4, "title"),
                (3, "capitalize"),
                (4, "replace({'A': 'a', 'x': 'X'})"),
                (3, "format('f1', 'f2')"),
                (3, "split(',')|join('-')"),
                (3, "default('dflt')"),
                (3, "json_encode"),
                (3, "spaceless"),
                (3, "length"),
                (2, "first"),
                (2, "reverse"),
                (2, "nl2br"),
                (2, "striptags"),
                (2, "url_encode"),
                (2, "slice(1, 3)"),
                (1, "abs"),
                (1, "number_format(1)"),
                (1, "join(',')"),
                (1, "keys"),
                (1, "batch(2)"),
                (1, "convert_encoding('UTF-16BE', 'UTF-8')|length"),
                (1, "nope"),
            ]));
        }
        let filters = chain.join("|");
        let _ = write!(out, "{{% apply {filters} %}}");
        let body = self.rng.pick_str(&[
            "a,b <x> %s ",
            " {{ html }} ",
            "{{ n }}",
            "",
            " Text with A and x ",
            "{{ s }},{{ n }}",
            "\n  line\n",
        ]);
        out.push_str(body);
        if self.rng.chance(50) {
            let inner = self.body(depth + 1);
            out.push_str(&inner);
        }
        out.push_str("{% endapply %}");
    }

    // ---- blocks and macros ----------------------------------------------------------------

    fn block(&mut self, out: &mut String, depth: usize) {
        let name = self.fresh("b");
        let doc = self.maybe_doc();
        let inline_doc = if self.rng.chance(15) {
            " ## inline doc\n"
        } else {
            ""
        };
        let _ = write!(out, "{doc}{{% block {name}{inline_doc} %}}<{name}>");
        let inner = self.body(depth + 1);
        out.push_str(&inner);
        out.push_str("{% endblock %}");
        self.blocks.push(name.clone());
        if self.rng.chance(40) {
            let _ = write!(out, "({{{{ block('{name}') }}}})");
        }
    }

    fn macro_(&mut self, out: &mut String, depth: usize) {
        let name = self.fresh("m");
        let doc = self.maybe_doc();
        let args = self
            .rng
            .pick_str(&["", "a", "a, b = 2", "a ## doc of a\n, b"]);
        let _ = write!(out, "{doc}{{% macro {name}({args}) %}}<{name}:");
        if !args.is_empty() {
            out.push_str("{{ a ?? '' }}");
        }
        if depth < 2 && self.rng.chance(50) {
            let inner = self.body(depth + 1);
            out.push_str(&inner);
        }
        out.push_str(">{% endmacro %}");
        self.macros.push(name.clone());
        let call = self.rng.pick_str(&["('x')", "()", "('y', 3)"]);
        let _ = write!(out, "{{{{ _self.{name}{call} }}}}");
    }

    // ---- templates ------------------------------------------------------------------------

    fn inc(&mut self) -> String {
        let mut out = String::from("<inc>");
        let n = self.rng.range(1, 3);
        for _ in 0..n {
            match self.rng.below(6) {
                0 => self.types(&mut out),
                1 => self.deprecated(&mut out),
                2 => self.do_(&mut out),
                3 => {
                    out.push_str("{% with {n: 'inner'} %}[{{ n }}]{% endwith %}");
                }
                4 => out.push_str("{% flush %}"),
                _ => out.push_str("[{{ n }}|{{ s }}]"),
            }
        }
        // inc.twig must not include itself
        out.replace("include('inc.twig')", "'inc'")
            .replace("{% include 'inc.twig' %}", "")
            + "</inc>"
    }

    fn child(&mut self) -> String {
        let mut out = String::new();
        if self.rng.chance(20) {
            let doc = self.doc_text();
            out.push_str(&doc);
        }
        out.push_str("{% extends 'base.twig' %}");
        let n = self.rng.range(1, 4);
        for _ in 0..n {
            match self.rng.below(9) {
                0 => self.types(&mut out),
                1 => self.deprecated(&mut out),
                2 => self.do_(&mut out),
                3 => out.push_str("{% flush %}"),
                4 => {
                    let doc = self.doc_text();
                    out.push_str(&doc);
                }
                5 => {
                    let name = self.rng.pick_str(&["range", "nope", "dump"]);
                    let _ = write!(
                        out,
                        "{{% guard function {name} %}}{{% set guarded = 'yes' %}}{{% else %}}{{% set guarded = 'no' %}}{{% endguard %}}"
                    );
                }
                6 => out.push_str("{% set x = 'child-x' %}"),
                7 => out.push_str("{% with {w: 1} %}{% set inner = 2 %}{% endwith %}"),
                _ => out.push_str(self.rng.pick_str(&["\n", " ", "{# c #}"])),
            }
            out.push('\n');
        }
        let content = self.body(1);
        let _ = write!(
            out,
            "{{% block content %}}<child:{{{{ x ?? '-' }}}}{{{{ guarded ?? '' }}}}>{content}{{% endblock %}}"
        );
        if self.rng.chance(50) {
            out.push_str("{% block side %}{{ parent() }}+{{ a ?? '' }}{% endblock %}");
        }
        out
    }
}

/// The tags (`{% name`), filters (`|name`, and the first filter of `apply`), functions (`name(` not
/// after `.` or `|`) and tests (`is [not] name [as|by]`) the sources name, sorted. Names that are
/// not callables (macro names, keywords) may be among them: a policy allows them harmlessly.
fn used_names(sources: &[String]) -> (Vec<String>, Vec<String>, Vec<String>, Vec<String>) {
    use std::collections::BTreeSet;
    fn ident(bytes: &[u8], mut i: usize) -> (String, usize) {
        let start = i;
        while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
            i += 1;
        }
        (String::from_utf8_lossy(&bytes[start..i]).into_owned(), i)
    }
    fn skip_spaces(bytes: &[u8], mut i: usize) -> usize {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        i
    }
    let (mut tags, mut filters, mut functions, mut tests) = (
        BTreeSet::new(),
        BTreeSet::new(),
        BTreeSet::new(),
        BTreeSet::new(),
    );
    for source in sources {
        let bytes = source.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            let c = bytes[i];
            if c == b'{' && bytes.get(i + 1) == Some(&b'%') {
                let mut j = i + 2;
                if matches!(bytes.get(j), Some(b'-' | b'~')) {
                    j += 1;
                }
                let (name, end) = ident(bytes, skip_spaces(bytes, j));
                if !name.is_empty() {
                    if name == "apply" {
                        let (filter, _) = ident(bytes, skip_spaces(bytes, end));
                        if !filter.is_empty() {
                            filters.insert(filter);
                        }
                    }
                    tags.insert(name);
                }
                i = end.max(i + 2);
                continue;
            }
            if c == b'|' {
                let (name, end) = ident(bytes, skip_spaces(bytes, i + 1));
                if !name.is_empty() {
                    filters.insert(name);
                }
                i = end.max(i + 1);
                continue;
            }
            if (c.is_ascii_alphabetic() || c == b'_')
                && (i == 0 || !(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_'))
            {
                let (name, end) = ident(bytes, i);
                let previous = bytes[..i].iter().rev().find(|b| !b.is_ascii_whitespace());
                if name == "is" {
                    let (mut test, mut after) = ident(bytes, skip_spaces(bytes, end));
                    if test == "not" {
                        (test, after) = ident(bytes, skip_spaces(bytes, after));
                    }
                    if !test.is_empty() {
                        let (second, _) = ident(bytes, skip_spaces(bytes, after));
                        if matches!(second.as_str(), "as" | "by") {
                            tests.insert(format!("{test} {second}"));
                        }
                        tests.insert(test);
                    }
                } else if bytes.get(end) == Some(&b'(') && !matches!(previous, Some(b'.' | b'|')) {
                    functions.insert(name);
                }
                i = end;
                continue;
            }
            i += 1;
        }
    }
    (
        tags.into_iter().collect(),
        filters.into_iter().collect(),
        functions.into_iter().collect(),
        tests.into_iter().collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kit::case::Case;

    #[test]
    fn scans_the_names_a_policy_needs() {
        let (tags, filters, functions, tests) = used_names(&[
            "{%- apply upper|trim %}{{ range(1, 2)|join }}{{ _self.m() }}{% endapply %}\
             {% if n is not divisible by(3) and x is defined %}{% endif %}"
                .to_owned(),
        ]);
        assert_eq!(tags, ["apply", "endapply", "endif", "if"]);
        assert_eq!(filters, ["join", "trim", "upper"]);
        assert_eq!(functions, ["by", "range"]);
        assert_eq!(tests, ["defined", "divisible", "divisible by"]);
    }

    #[test]
    fn deterministic_and_parseable() {
        let a = generate(9, 300);
        assert_eq!(a, generate(9, 300));
        assert_ne!(generate(10, 5), generate(9, 5));
        for json in &a {
            let case = Case::from_json(json).expect("a twig case");
            assert!(case.unsupported_reason().is_none(), "{json:?}");
            let main = case.template(&case.main).expect("the main template");
            assert!(!main.is_empty(), "{json:?}");
            if let Some(inc) = case.template("inc.twig") {
                let inc = String::from_utf8_lossy(inc);
                assert!(!inc.contains("inc.twig"), "{inc}");
            }
        }
        assert_eq!(case(9, 4), a[4]);
    }
}
