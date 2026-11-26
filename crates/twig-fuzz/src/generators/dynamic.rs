//! Generator of the `dynamic` family: names and callables computed at run time.
//!
//! Every case renders `index.twig` with two to five statements that pick what they use while the
//! template runs:
//!
//! - `include` (tag, `include()`, `include_only()`) and `source()` of computed names: concatenations,
//!   conditionals, arrays of candidates (`ignore missing`, `ignore_missing`), `Stringable` names,
//!   names with directories, dots, spaces and wrong case, non-string names (`null`, numbers, empty
//!   arrays), and `TemplateWrapper`s from `template_from_string()`;
//! - `template_from_string()` with computed sources (also failing ones), optional names, and the
//!   `TemplateWrapper` methods (`render`, `renderBlock`, `hasBlock`, `getBlockNames`,
//!   `templateName`), the wrappers passed to `include` and `embed`;
//! - `attribute()` with computed names of every type (strings that differ in case or are numeric,
//!   ints, floats, booleans, `null`, a `Stringable`) on arrays, `ArrayObject`, `stdClass` (also
//!   numeric property names), the helper objects of Twig's `tests/IntegrationTest.php`, scalars
//!   and `null`; computed arguments (lists, named arguments as mappings, a variable, a filter
//!   expression or a constant, which deprecate, a non-array), `is defined`, `??`, `default`, and
//!   the argument binding of `attribute()` itself (named, missing, unknown and extra arguments);
//! - macros referenced through computed names: `m.(name)()` and `_self.(name)()` with names that
//!   differ in case or carry the `macro_` prefix (deprecated), unknown and non-string names,
//!   `is defined` on them, macros of a parent template reached through the child's `_self`,
//!   macros taking arrow functions and variadic arguments, imports from computed template names;
//! - arrow functions passed around: stored in variables and mappings, picked by computed keys,
//!   called with `invoke`, given to `map`/`filter`/`reduce`/`sort`/`find`, to macros and to
//!   included templates, returning arrows, capturing the context when they are created;
//! - the context itself: `_context[name]`, `attribute(_context, name)`, key lists after `set`
//!   and `with`, the context passed on with `merge` and `filter`, a variable named `_context`;
//! - `block()` with computed names and templates, `{% embed %}` of computed names, and
//!   `template_from_string()` sources that extend, include or import other templates.
//!
//! The data comes from `data_php` with `"test_classes": true`. Settings vary: `strict_variables`,
//! `autoescape`, `debug` and warnings turned into exceptions. Spreads in `attribute()` arguments
//! are left out (PHP rejects some of them with an uncatchable compile-time fatal error that cannot be
//! compared, deviation D3 in `reference/deviations.md`), and so are string callables
//! (`reference/deviations.md`).
//!
//! [`generate`] is deterministic: case `i` of seed `s` depends only on `(s, i)` ([`case`]). Ids
//! are `dyn-<seed>-<i>`. [`batch`] runs every case in a PHP process of its own.

use std::fmt::Write as _;

use super::casekit::{self, Builder, Rng};
use crate::kit::json::Json;

/// The family name.
pub const FAMILY: &str = "dynamic";

const SALT: u64 = 0x0044_594E_414D;

/// `count` cases of `seed`, as a batch (process isolation).
pub fn batch(seed: u64, count: usize) -> Json {
    casekit::batch(generate(seed, count))
}

/// `count` cases of `seed`.
pub fn generate(seed: u64, count: usize) -> Vec<Json> {
    (0..count as u64).map(|i| case(seed, i)).collect()
}

/// Case `index` of `seed` (id `dyn-<seed>-<index>`).
pub fn case(seed: u64, index: u64) -> Json {
    let rng = Rng::new(casekit::case_seed(seed, index, SALT));
    Gen::new(rng).build(format!("dyn-{seed}-{index}"))
}

/// The helper templates every case has.
const HELPERS: &[(&str, &str)] = &[
    ("a.twig", "A({{ v ?? '-' }})"),
    ("b.twig", "B({{ _context|keys|join(',') }})"),
    ("dir/c.twig", "C[{{ v ?? '' }}]"),
    (
        "m.twig",
        "{% macro hello(name = 'World') %}Hello {{ name }}{% endmacro %}\
         {% macro Shout(x) %}{{ x|upper }}!{% endmacro %}\
         {% macro apply(f, v) %}{{ f|invoke(v) }}{% endmacro %}\
         {% macro vars() %}{{ varargs|join(',') }}{% endmacro %}\
         {% macro rest(a, ...more) %}{{ a }}:{{ more|join(',') }}{% endmacro %}\
         {% macro ctx() %}{{ _context|keys|join(',') }}{% endmacro %}",
    ),
    (
        "parent.twig",
        "{% macro pm(x) %}P{{ x }}{% endmacro %}<{% block content %}parent{% endblock %}>",
    ),
    ("fn.twig", "{{ f|invoke(v ?? 1) }}"),
    (
        "t.twig",
        "{% block x %}X{% endblock %}{% block y %}Y{{ v ?? '' }}{% endblock %}",
    ),
];

/// The context (PHP code; `"test_classes": true`).
const DATA: &str = "return [\
    'foo' => new \\Twig\\Tests\\TwigTestFoo(), \
    'stub' => new \\Twig\\Tests\\ToStringStub('a.twig'), \
    'stubName' => new \\Twig\\Tests\\ToStringStub('bar'), \
    'magic' => new \\Twig\\Tests\\MagicCallStub(), \
    'o' => (object) ['a' => 1, '0' => 'zero', 'b c' => 2, 'n' => null, 'Foo' => 'F'], \
    'ao' => new \\ArrayObject(['a' => 1, 2 => 'two', 'bar' => 'B']), \
    'h' => ['a' => 1, '1' => 'one', 'b c' => 3, '' => 'empty', 'bar' => ['x' => 'y']], \
    'l' => ['x', 'y', 'z'], \
    'names' => ['missing.twig', 'a.twig'], \
    'args' => ['A', 'B'], \
    'tpl' => 'a', 'mac' => 'hello', 'blk' => 'x', \
    'n' => 1, 'f' => 1.5, 't' => true, 'z' => null, 's' => 'foo', \
    'src' => '{{ v ?? \"src\" }}', \
    'srcBlock' => '{% block x %}SX{{ v ?? \"\" }}{% endblock %}', \
    'srcBad' => '{{ v ?? }}'\
]";

struct Gen {
    rng: Rng,
    counter: usize,
    uses_child: bool,
    uses_dump: bool,
    /// Arrow variables defined so far.
    arrows: Vec<String>,
}

impl Gen {
    fn new(rng: Rng) -> Gen {
        Gen {
            rng,
            counter: 0,
            uses_child: false,
            uses_dump: false,
            arrows: Vec::new(),
        }
    }

    fn fresh(&mut self, prefix: &str) -> String {
        self.counter += 1;
        format!("{prefix}{}", self.counter)
    }

    fn build(mut self, id: String) -> Json {
        let mut b = Builder::new(id);
        for (name, source) in HELPERS {
            b.template(name, *source);
        }
        let mut index = String::new();
        if self.rng.chance(50) {
            index.push_str("{% import 'm.twig' as m %}");
        }
        let n = self.rng.range(2, 5);
        for _ in 0..n {
            self.statement(&mut index);
            index.push('\n');
        }
        // `m` is used by many statements: import it when they did
        if index.contains("m.") && !index.starts_with("{% import 'm.twig' as m %}") {
            index.insert_str(0, "{% import 'm.twig' as m %}");
        }
        if self.uses_child {
            b.template(
                "child.twig",
                "{% extends 'parent.twig' %}{% import _self as me %}\
                 {% block content %}{{ me.pm(1) }}|{{ me.(name ?? 'pm')(2) }}|\
                 {{ me.(name ?? 'pm') is defined ? 'D' : 'U' }}{% endblock %}",
            );
        }
        if self.rng.chance(15) {
            index.push_str(&self.probe());
        }
        b.template("index.twig", index);
        b.data_php = Some(DATA.to_owned());
        b.test_classes = true;
        b.extensions.push("string_loader");
        let strict = self.rng.chance(55);
        b.option("strict_variables", Json::Bool(strict));
        let autoescape = self
            .rng
            .weighted(&[(55, "false"), (35, "html"), (10, "js")]);
        b.option(
            "autoescape",
            if autoescape == "false" {
                Json::Bool(false)
            } else {
                Json::str(autoescape)
            },
        );
        if self.uses_dump || self.rng.chance(10) {
            b.option("debug", Json::Bool(true));
            b.extensions.push("debug");
        }
        b.throw = self.rng.chance(15);
        if self.rng.chance(20) {
            b.load_all = Some(false);
        }
        b.build()
    }

    fn statement(&mut self, out: &mut String) {
        match self.rng.below(24) {
            0..=2 => self.include(out),
            3 => self.source(out),
            4..=6 => self.string_template(out),
            7..=10 => self.attribute(out),
            11 | 12 => self.context(out),
            13..=15 => self.macros(out),
            16..=19 => self.arrows_(out),
            20 => self.block(out),
            21 => self.embed(out),
            22 => {
                self.uses_child = true;
                let name = self.rng.pick_str(&[
                    "'pm'",
                    "'PM'",
                    "'macro_pm'",
                    "'nope'",
                    "null",
                    "1",
                    "'pM'",
                ]);
                let _ = write!(out, "{{{{ include('child.twig', {{name: {name}}}) }}}}");
            }
            _ => {
                let name = self.name_expr();
                let _ = write!(out, "{{{{ include('b.twig', {{({name}): 1}}) }}}}");
            }
        }
    }

    // ---- names ----------------------------------------------------------------------------

    /// A computed template name (sometimes not one that exists).
    fn template_name(&mut self) -> String {
        self.rng
            .weighted(&[
                (16, "'a' ~ '.twig'"),
                (16, "tpl ~ '.twig'"),
                (8, "names"),
                (6, "names|reverse"),
                (8, "['nope.twig', tpl ~ '.twig']"),
                (8, "'dir/' ~ 'c.twig'"),
                (8, "(n ? 'a' : 'b') ~ '.twig'"),
                (2, "names|first"),
                (8, "names|last"),
                (2, "[]"),
                (2, "''"),
                (2, "null"),
                (2, "n"),
                (2, "tpl"),
                (2, "'A.twig'"),
                (2, "'./a.twig'"),
                (2, "'dir//c.twig'"),
                (2, "'dir/../a.twig'"),
                (1, "' a.twig'"),
                (8, "template_from_string(src)"),
                (5, "[template_from_string(src), 'a.twig']"),
                (6, "stub"),
                (3, "[stub]"),
                (2, "{x: 'nope.twig', y: 'a.twig'}"),
                (1, "f"),
                (1, "t"),
            ])
            .to_owned()
    }

    /// A computed attribute, key or variable name of any type.
    fn name_expr(&mut self) -> String {
        self.rng
            .weighted(&[
                (6, "'bar'"),
                (4, "'b' ~ 'ar'"),
                (5, "'foo'"),
                (3, "'Foo'"),
                (3, "'getFoo'"),
                (3, "'FOO'|lower"),
                (2, "'self'"),
                (2, "'empty'"),
                (2, "'null'"),
                (2, "'position'"),
                (2, "'array'"),
                (3, "'BAR_NAME'"),
                (5, "'a'"),
                (2, "'b c'"),
                (3, "'0'"),
                (3, "0"),
                (3, "1"),
                (3, "'1'"),
                (2, "1.5"),
                (2, "true"),
                (2, "false"),
                (3, "null"),
                (2, "''"),
                (2, "'n'"),
                (2, "'__toString'"),
                (3, "stubName"),
                (1, "'is'"),
                (1, "'strToLower'"),
                (1, "'-1'"),
                (1, "' 1'"),
                (1, "'01'"),
                (3, "'missing'"),
                (2, "s"),
                (2, "n"),
                (1, "f"),
                (1, "l|first"),
                (1, "'s' ~ ''"),
            ])
            .to_owned()
    }

    fn object(&mut self) -> &'static str {
        self.rng.weighted(&[
            (8, "foo"),
            (6, "o"),
            (6, "ao"),
            (8, "h"),
            (4, "l"),
            (2, "s"),
            (2, "n"),
            (2, "z"),
            (2, "stub"),
            (1, "magic"),
            (4, "_context"),
            (3, "{a: 1, b: {c: 2}, 0: 'zero'}"),
            (2, "[10, 20]"),
            (2, "foo.self"),
            (1, "h.bar"),
        ])
    }

    // ---- statements -----------------------------------------------------------------------

    fn include(&mut self, out: &mut String) {
        let name = self.template_name();
        let form = self.rng.below(9);
        let _ = match form {
            0 => write!(out, "{{{{ include({name}) }}}}"),
            1 => write!(out, "{{% include {name} %}}"),
            2 => write!(out, "{{{{ include({name}, {{v: 1}}) }}}}"),
            3 => write!(out, "{{% include {name} with {{v: 2}} only %}}"),
            4 => write!(out, "{{{{ include({name}, ignore_missing = true) }}}}"),
            5 => write!(out, "{{% include {name} ignore missing %}}"),
            6 => write!(out, "{{{{ include_only({name}, {{v: 3}}) }}}}"),
            7 => write!(out, "{{{{ include({name}, with_context = false) }}}}"),
            _ => write!(out, "{{% include {name} ignore missing with {{v: 'w'}} %}}"),
        };
    }

    fn source(&mut self, out: &mut String) {
        let name = self.rng.weighted(&[
            (10, "'a' ~ '.twig'"),
            (10, "tpl ~ '.twig'"),
            (6, "'dir/c.twig'"),
            (6, "stub"),
            (4, "names|last"),
            (2, "'nope.twig'"),
            (2, "names"),
            (1, "null"),
            (1, "n"),
            (2, "'m.twig'"),
        ]);
        let _ = match self.rng.below(4) {
            0 => write!(out, "{{{{ source({name}) }}}}"),
            1 => write!(out, "{{{{ source({name}, ignore_missing = true) }}}}"),
            2 => write!(out, "{{{{ source({name})|length }}}}"),
            _ => write!(out, "{{{{ source({name}, true)|upper }}}}"),
        };
    }

    fn template_source(&mut self) -> String {
        self.rng
            .weighted(&[
                (8, "src"),
                (6, "'{{ v ?? 1 }}'"),
                (5, "srcBlock"),
                (4, "'[' ~ s ~ ']'"),
                (4, "'{{ ' ~ n ~ ' + 1 }}'"),
                (3, "'{{ undefined_x }}'"),
                (3, "'{% include \"a.twig\" %}'"),
                (3, "'{{ include(\"a.twig\", {v: 9}) }}'"),
                (
                    3,
                    "'{% extends \"parent.twig\" %}{% block content %}S{% endblock %}'",
                ),
                (3, "'{% import \"m.twig\" as mm %}{{ mm.hello() }}'"),
                (2, "srcBad"),
                (2, "'{{ '"),
                (2, "stub"),
                (2, "n"),
                (1, "null"),
                (1, "['a']"),
                (2, "'{{ _self }}'"),
                (2, "'{{ block(\"x\", \"t.twig\") }}'"),
                (2, "src ~ '\n{{ 1 // 0 }}'"),
                (1, "''"),
            ])
            .to_owned()
    }

    fn string_template(&mut self, out: &mut String) {
        let source = self.template_source();
        let name = self.rng.weighted(&[
            (60, ""),
            (15, "'n.twig'"),
            (8, "null"),
            (8, "tpl"),
            (5, "''"),
            (4, "s ~ '.twig'"),
        ]);
        let call = if name.is_empty() {
            format!("template_from_string({source})")
        } else {
            format!("template_from_string({source}, {name})")
        };
        let _ = match self.rng.below(10) {
            0 | 1 => write!(out, "{{{{ include({call}) }}}}"),
            2 => write!(out, "{{{{ {call}.render({{v: 'r'}}) }}}}"),
            3 => write!(out, "{{{{ {call}.render(_context) }}}}"),
            4 => {
                let t = self.fresh("t");
                write!(
                    out,
                    "{{% set {t} = {call} %}}{{{{ {t}.render() }}}}|{{{{ include({t}, {{v: 2}}) }}}}"
                )
            }
            5 => write!(
                out,
                "{{% set tw = {call} %}}{{{{ tw.hasBlock('x') ? tw.renderBlock('x', {{v: 'b'}}) : 'nob' }}}}|{{{{ tw.getBlockNames()|join(',') }}}}"
            ),
            6 => write!(
                out,
                "{{{{ {call}.templateName starts with '__string_template__' ? 'anon' : 'named' }}}}"
            ),
            7 => write!(
                out,
                "{{% embed {call} %}}{{% block x %}}EX{{% endblock %}}{{% endembed %}}"
            ),
            8 => write!(out, "{{{{ include([{call}, 'a.twig']) }}}}"),
            _ => write!(out, "{{{{ {call}.render({{v: 'q'}})|length }}}}"),
        };
    }

    fn arguments(&mut self) -> &'static str {
        self.rng.weighted(&[
            (8, "[]"),
            (6, "['x']"),
            (4, "['x', 'y']"),
            (4, "{param2: 'y'}"),
            (3, "{param1: 'p', param2: 'q'}"),
            (4, "args"),
            (2, "args|merge(['C'])"),
            (2, "null"),
            (2, "'str'"),
            (2, "[1, 2, 3]"),
            (1, "{0: 'a', param2: 'b'}"),
            (1, "{param2: 'b', 0: 'a'}"),
            (1, "{nope: 1}"),
        ])
    }

    fn attribute(&mut self, out: &mut String) {
        let object = self.object();
        let name = self.name_expr();
        let call = match self.rng.below(40) {
            0..=13 => {
                let args = self.arguments();
                format!("attribute({object}, {name}, {args})")
            }
            // named arguments, a fourth argument, wrong names (argument binding of `attribute()`)
            14 => format!("attribute(variable = {object}, attribute = {name})"),
            15 => format!("attribute({object}, attribute = {name}, arguments = ['n'])"),
            16 => format!("attribute({object}, {name}, [], 'method')"),
            17 => format!("attribute({object}, name = {name})"),
            18 => format!("attribute({object})"),
            19 => format!("attribute(arguments = [], variable = {object}, attribute = {name})"),
            _ => format!("attribute({object}, {name})"),
        };
        let _ = match self.rng.below(10) {
            0..=3 => write!(out, "[{{{{ {call} }}}}]"),
            4 => write!(out, "[{{{{ {call} is defined ? 'D' : 'U' }}}}]"),
            5 => write!(out, "[{{{{ {call}|default('dflt') }}}}]"),
            6 => write!(out, "[{{{{ {call} ?? 'nil' }}}}]"),
            7 => write!(out, "[{{{{ {call}|json_encode }}}}]"),
            8 => {
                let name2 = self.name_expr();
                write!(out, "[{{{{ attribute({call}, {name2}) ?? 'nil2' }}}}]")
            }
            _ => write!(out, "[{{{{ {object}.({name}) ?? 'dot' }}}}]"),
        };
    }

    fn context(&mut self, out: &mut String) {
        let name = self.name_expr();
        let _ = match self.rng.below(10) {
            0 | 1 => write!(out, "[{{{{ _context[{name}] ?? 'none' }}}}]"),
            2 => write!(out, "[{{{{ attribute(_context, {name}) ?? 'none' }}}}]"),
            3 => write!(out, "[{{{{ ({name}) in _context|keys ? 'in' : 'out' }}}}]"),
            4 => {
                let v = self.fresh("v");
                write!(
                    out,
                    "{{% set {v} = {name} %}}[{{{{ _context|keys|join(',') }}}}]"
                )
            }
            5 => write!(
                out,
                "{{% for key in [{name}, 'n', 's', 'missing'] %}}{{{{ key }}}}={{{{ _context[key] ?? '?' }}}};{{% endfor %}}"
            ),
            6 => write!(
                out,
                "{{% with {{inner: {name}}} %}}[{{{{ _context|keys|join(',') }}}}]{{% endwith %}}"
            ),
            7 => write!(
                out,
                "{{{{ include('b.twig', _context|merge({{extra: {name}}})) }}}}"
            ),
            8 => write!(
                out,
                "{{{{ include('b.twig', _context|filter((v, k) => k starts with 's')) }}}}"
            ),
            _ => write!(
                out,
                "{{% set _context = {{x: {name}}} %}}[{{{{ _context|keys|join(',') }}}}|{{{{ _context._context.x ?? '-' }}}}]"
            ),
        };
    }

    fn macro_name(&mut self) -> &'static str {
        self.rng.weighted(&[
            (14, "'hello'"),
            (10, "mac"),
            (4, "'HELLO'"),
            (3, "'Hello'"),
            (4, "'macro_hello'"),
            (2, "'macro_HELLO'"),
            (4, "'shout'"),
            (5, "'Shout'"),
            (2, "'nope'"),
            (1, "null"),
            (1, "1"),
            (1, "''"),
            (1, "true"),
            (4, "'vars'"),
            (4, "'rest'"),
            (4, "'ctx'"),
            (1, "stubName"),
            (3, "'hel' ~ 'lo'"),
        ])
    }

    fn macro_args(&mut self) -> &'static str {
        self.rng.weighted(&[
            (8, ""),
            (6, "'x'"),
            (3, "'x', 'y', 'z'"),
            (3, "name: 'N'"),
            (2, "x: 'X'"),
            (2, "...args"),
            (1, "...{name: 'S'}"),
            (2, "1, 2"),
        ])
    }

    fn macros(&mut self, out: &mut String) {
        let name = self.macro_name();
        let args = self.macro_args();
        let _ = match self.rng.below(11) {
            0..=3 => write!(out, "[{{{{ m.({name})({args}) }}}}]"),
            4 => write!(out, "[{{{{ m.({name}) is defined ? 'D' : 'U' }}}}]"),
            10 => {
                // the macro namespace as a value
                let probe = self.rng.pick_str(&[
                    "attribute(m, mac)",
                    "m is defined ? 'D' : 'U'",
                    "m.hello is defined ? 'D' : 'U'",
                    "m.Hello is defined ? 'D' : 'U'",
                    "m.macro_hello is defined ? 'D' : 'U'",
                    "_self.local is defined ? 'D' : 'U'",
                    "m.hello ?? 'none'",
                ]);
                write!(out, "[{{{{ {probe} }}}}]")
            }
            5 => {
                let local = self.rng.weighted(&[
                    (8, "'local'"),
                    (3, "'LOCAL'"),
                    (3, "'macro_local'"),
                    (2, "'Local'"),
                    (2, "name ?? 'local'"),
                    (1, "'nope'"),
                ]);
                write!(
                    out,
                    "{{% macro local(x = 'L') %}}local:{{{{ x }}}}{{% endmacro %}}[{{{{ _self.({local})({args}) }}}}|{{{{ _self.({local}) is defined ? 'D' : 'U' }}}}]"
                )
            }
            6 => {
                let template =
                    self.rng
                        .pick_str(&["'m' ~ '.twig'", "'m.twig'", "tpl ~ '.twig'", "'M.twig'"]);
                write!(
                    out,
                    "{{% import {template} as mm %}}[{{{{ mm.hello('I') }}}}]"
                )
            }
            7 => write!(
                out,
                "{{% from 'm.twig' import hello as h, Shout %}}[{{{{ h({args}) }}}}|{{{{ Shout('s') }}}}]"
            ),
            8 => write!(out, "[{{{{ m.({name}) }}}}]"),
            _ => {
                let arrow = self.arrow();
                write!(out, "[{{{{ m.apply({arrow}, 'v') }}}}]")
            }
        };
    }

    fn arrow(&mut self) -> String {
        if let Some(name) = self.arrows.last().cloned().filter(|_| self.rng.chance(40)) {
            return name;
        }
        self.rng
            .pick_str(&[
                "x => x|upper",
                "x => x ~ '!'",
                "(x) => [x, x]|join('-')",
                "x => x ~ x",
                "x => n ~ x",
                "(x, y) => x ~ y",
                "x => attribute(h, x) ?? 'nope'",
                "x => _context|keys|length",
                "x => x is defined ? x : 'undef'",
                "x => (x ~ 'q')|length",
            ])
            .to_owned()
    }

    fn arrows_(&mut self, out: &mut String) {
        let arrow = self.arrow();
        let _ = match self.rng.below(12) {
            0 => {
                let f = self.fresh("f");
                self.arrows.push(f.clone());
                write!(out, "{{% set {f} = {arrow} %}}[{{{{ {f}|invoke('a') }}}}]")
            }
            1 => write!(out, "[{{{{ (['a', 'b']|map({arrow}))|join(',') }}}}]"),
            2 => write!(
                out,
                "[{{{{ include('fn.twig', {{f: {arrow}, v: 'i'}}) }}}}]"
            ),
            3 => {
                let key = self
                    .rng
                    .pick_str(&["'up'", "'low'", "s", "'nope'", "('u' ~ 'p')"]);
                write!(
                    out,
                    "{{% set fs = {{up: s => s|upper, low: s => s|lower}} %}}[{{{{ (fs[{key}] ?? (x => 'none'))|invoke('aB') }}}}|{{{{ attribute(fs, {key})|default('d') is iterable ? 'it' : 'no' }}}}]"
                )
            }
            4 => write!(
                out,
                "{{% set k = 1 %}}{{% set g = x => x ~ k %}}{{% set k = 2 %}}[{{{{ g|invoke(0) }}}}|{{{{ k }}}}]"
            ),
            5 => write!(
                out,
                "{{% set curry = x => y => x ~ y %}}[{{{{ (curry|invoke('c'))|invoke('d') }}}}]"
            ),
            6 => write!(
                out,
                "[{{{{ ['b', 'a', 'c']|sort((a, b) => b <=> a)|join }}}}|{{{{ [1, 2, 3]|reduce((c, x) => c + x, 10) }}}}]"
            ),
            7 => write!(out, "[{{{{ l|filter({arrow})|join(',') }}}}]"),
            8 => write!(
                out,
                "[{{{{ l|find(x => x == 'y') }}}}|{{{{ (l has some (x => x == 'z')) ? 'some' : 'none' }}}}|{{{{ (l has every {arrow}) ? 'every' : 'not' }}}}]"
            ),
            9 => write!(out, "[{{{{ ({arrow})|invoke('i', 'j') }}}}]"),
            10 => write!(
                out,
                "{{% macro runner(fn) %}}<{{{{ fn|invoke('r') }}}}>{{% endmacro %}}[{{{{ _self.runner({arrow}) }}}}]"
            ),
            _ => {
                if self.rng.chance(30) {
                    write!(out, "[{{{{ s|invoke('x') }}}}]")
                } else {
                    write!(
                        out,
                        "[{{{{ (h|map(v => v is iterable ? v|keys|join : v))|join(',') }}}}]"
                    )
                }
            }
        };
    }

    fn block(&mut self, out: &mut String) {
        let name = self.rng.weighted(&[
            (5, "blk"),
            (5, "'x'"),
            (5, "'y'"),
            (1, "'X'"),
            (1, "'nope'"),
            (1, "null"),
            (3, "blk ~ ''"),
        ]);
        let template = self.rng.weighted(&[
            (5, ""),
            (5, "'t.twig'"),
            (1, "tpl ~ '.twig'"),
            (4, "template_from_string(srcBlock)"),
            (1, "'nope.twig'"),
        ]);
        let call = if template.is_empty() {
            format!("block({name}, 't.twig')")
        } else {
            format!("block({name}, {template})")
        };
        let _ = match self.rng.below(3) {
            0 => write!(out, "[{{{{ {call} }}}}]"),
            1 => write!(out, "[{{{{ {call} is defined ? 'D' : 'U' }}}}]"),
            _ => write!(
                out,
                "{{% with {{v: 'w'}} %}}[{{{{ {call} }}}}]{{% endwith %}}"
            ),
        };
    }

    fn embed(&mut self, out: &mut String) {
        let name = self.rng.pick_str(&[
            "'t.twig'",
            "'t' ~ '.twig'",
            "names|last",
            "['nope.twig', 't.twig']",
            "template_from_string(srcBlock)",
            "tpl ~ '.twig'",
        ]);
        let extra = self.rng.pick_str(&[
            "",
            " ignore missing",
            " with {v: 'e'}",
            " with {v: 'o'} only",
        ]);
        let _ = write!(
            out,
            "{{% embed {name}{extra} %}}{{% block y %}}EY{{{{ parent() }}}}{{{{ v ?? '' }}}}{{% endblock %}}{{% endembed %}}"
        );
    }

    fn probe(&mut self) -> String {
        if self.rng.chance(30) {
            self.uses_dump = true;
            return "{{ dump(_context|keys) }}".to_owned();
        }
        self.rng
            .pick_str(&[
                "{{ undefined_var }}",
                "{{ foo.nope }}",
                "{{ h.nope }}",
                "{{ 1 // 0 }}",
            ])
            .to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kit::case::Case;

    #[test]
    fn deterministic_and_parseable() {
        let a = generate(3, 300);
        assert_eq!(a, generate(3, 300));
        assert_ne!(generate(4, 5), generate(3, 5));
        for json in &a {
            let case = Case::from_json(json).expect("a twig case");
            assert!(case.unsupported_reason().is_none(), "{json:?}");
            assert_eq!(case.main, "index.twig");
        }
        assert_eq!(case(3, 17), a[17]);
    }
}
