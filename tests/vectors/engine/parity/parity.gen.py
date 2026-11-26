"""Writes the oracle batches tests/vectors/engine/parity/<topic>.gen.json and records them as regression vectors
(<topic>.json, the format of tests/vectors/fuzz/README.md) for tests/engine_parity.rs: what PHP's compiler does with
the code Twig generates (constant folding and the lines of folded constants in array literals, operand order of
commutative opcodes, casts of constant arrays in `~`, prefix operators before `**`), the compile-time state of
null-safe chains, macro defaults evaluated when the macro is called, the lines of a few runtime errors, the `loop`
bookkeeping writes, named arguments while unpacking, callable values, spreads in special node classes, the
operands the folding of array literals makes constant, the lines of the constants that folding leaves, of the
statements Twig writes after an expression and of TrueTest's bool conversion (plans/parity-fixes.md, items P1 to P16,
P31 and P35 to P37).

Regenerate from the repository root (Git Bash on Windows needs MSYS_NO_PATHCONV=1):

    python -I tests/vectors/engine/parity/parity.gen.py              # writes <topic>.gen.json
    python -I tests/vectors/engine/parity/parity.gen.py --record     # and records <topic>.json

`--record` runs `twig-fuzz record` (TWIG_FUZZ, default `cargo run -q -p twig-fuzz --release --`), which sends each
batch through the oracle (Docker image twig-oracle:3.31.1), one PHP process per case, and keeps the cases twig-rs
can run; a case twig-rs renders differently is marked `known_mismatch` and needs a reason written by hand.
"""

import json
import os
import shlex
import subprocess
import sys

OUT = os.path.dirname(os.path.abspath(__file__))
OFF = {"autoescape": False}
STRICT = {"autoescape": False, "strict_variables": True}
DEBUG = {"autoescape": False, "debug": True}
STD = "return ['o' => new \\stdClass(), 'n' => 5, 'zero' => 0, 's' => 'x', 'nan' => NAN, 'ao' => new \\ArrayObject([1])];"


def case(template, **kw):
    c = {"env": "plain", "templates": template if isinstance(template, dict) else {"index.twig": template}}
    c.update(kw)
    return c


def lines():
    """P1 (folded constants inside array literals take the literal's line), P10, P11, P12."""
    d = {"n": 5, "zero": 0, "a": {"b": 1}, "x": {"a": 0}, "f": False, "k": "kk"}
    c = [
        case("{{ [n\n/ zero] }}", data_json=d),
        case("{{ [1, [\nn,\nn / zero]] }}", data_json=d),
        case("{{ [\na.b,\nn / zero] }}", data_json=d),
        case("{{ [\nn,\nn / zero] }}", data_json=d),
        case("{{ {a:\nn,\nb: n / zero} }}", data_json=d),
        case("{{ [1,\nn / 0] }}", data_json=d),
        case("{{ [1,\nn / -0] }}", data_json=d),
        case("{{ [1,\nn / zero] }}", data_json=d, config={"strict_variables": True}),
        case("{{ [1,\nmax(n / zero)] }}", data_json=d),
        case("{{ [1, max([\nn,\nn / zero])] }}", data_json=d),
        case("{{ [1, max([\n2,\nn / zero])] }}", data_json=d),
        case("{{ [1,\nn / x['a']] }}", data_json=d),
        case("{{ [1,\nn ~ o] }}", data_php=STD),
        case("{{ [1,\nn ~ o] }}", data_php=STD, extensions=["sandbox"]),
        case("{{ [1,\nf] }}", data_json=d, config=OFF, errors="throw"),
        case("{{ {a: 1, (\no): 2}|length }}", data_php=STD),
        case("{{ {a: 1,\n1.5: n}|length }}", data_json=d),
        case("{{ {a: 1,\n1.5: n}|length }}", data_json=d, errors="throw"),
        case("{{ [1,\n(true ? n / zero : 0)]|length }}", data_json=d),
        case("{{ [1,\n(false ? 0 : n / zero)]|length }}", data_json=d),
        case("{{ [1,\nn / (null ?? zero)]|length }}", data_json=d),
        case("{{ [1,\nn / (1 - 1)]|length }}", data_json=d),
        case("{{ [1,\nn / (zero ?: 0)]|length }}", data_json=d),
        case("{{ [1,\nn / (true and false)]|length }}", data_json=d),
        case("{{ [1,\nn / (0 is null)]|length }}", data_json=d),
        case("{{ [1,\nn % (4 is even)]|length }}", data_json=d),
        case("{{ [1,\nn / not 1]|length }}", data_json=d),
        case("{% set a, b = 1,\nn / zero %}", data_json=d),
        case("{% set a, b = 1,\nn / zero %}", data_json=d, config={"strict_variables": True}),
        case("{% macro m(a, b) %}{% endmacro %}{{ _self.m(1,\nn / zero) }}", data_json=d),
        case({"index.twig": "{% include 'x.twig' with {a: 1,\nb: n / zero} %}", "x.twig": "X"}, data_json=d),
        case("{% with {a: 1,\nb: n / zero} %}{% endwith %}", data_json=d),
        case("{{ dump(1,\nn / zero) }}", data_json=d, config={"debug": True}, extensions=["debug"]),
        case("{{ a.b(1,\nn / zero) }}", data_json=d),
        case("{{ [1, a.b(\nn / zero)]|length }}", data_json=d),
        case("{{ [1, 2 ~ [\nn,\nn / zero]|length]|length }}", data_json=d),
        case("{{ o ~ ([1,\nn]|length) }}", data_php=STD),
        case("{{ o ~ [1,\nn]|join }}", data_php=STD),
        case("{{ o\n~ ([1,\nn]|join) }}", data_php=STD),
        case("{{ [1,\n2] ~ o }}", data_php=STD),
        case("{{ o ~\n[1,\n2] }}", data_php=STD),
        case("{{ o ~ (false and\nn) }}", data_php=STD),
        case("{{ o ~ (true or\nn) }}", data_php=STD),
        case("{{ o ~ (true and\nn) }}", data_php=STD),
        case("{{ [1, -\no]|length }}", data_php=STD),
        case("{{ [1, not\no]|length }}", data_php=STD),
        case("{{ [1, ...\nn] }}", data_json=d),
        case("{{ b.p(n,\n...t) }}"),
        case("{{ [y ~\na] }}", data_json={"a": [1], "y": 1}, config=OFF, errors="throw"),
        # P10: the string conversion of the message of {% deprecated %}
        case("{% deprecated\nitems %}", data_json={"items": [1, 2]}, errors="throw"),
        case("{% deprecated\nitems %}", data_json={"items": [1, 2]}),
        case("{% deprecated\no %}", data_php=STD),
        case("{% deprecated 'a' ~\no %}", data_php=STD),
        case("{% deprecated\n[1] %}", data_php=STD, errors="throw"),
        case("{% deprecated\nitems package='p' version=\n'1' %}", data_json={"items": [1, 2]}, errors="throw"),
        # P11: an arrow function called with too few arguments fails on the line of `function (`
        case("{% do\n[1, 2]|map((a, b, c) => a)|join %}"),
        case("{{ [1, 2]|map((a, b,\nc) =>\na)|join }}"),
        case("{{ [1]|filter((a, b,\nc) => a)|join }}"),
        case("{{ [1]|reduce((c, v, k,\nx) => c, 0) }}"),
        case("{% macro m(f) %}{{ [1]|map(f)|join }}{% endmacro %}{{ _self.m((a, b,\nc) => a) }}"),
        case("{{ [1] has some (a,\nb, c) => a }}"),
        # P12: the read of `$blocks` in an arrow function
        case("{{ [1, 2]|find(x =>\nblock(e)) }}", errors="throw"),
        case("{{ [1, 2]|find(x =>\nblock(e)) }}"),
        case("{{ [1, 2]|find(x =>\nblock('b'\n)) }}{% block b %}{% endblock %}"),
        case("{{ [1, 2]|find(x =>\nblock('b') is defined) }}"),
        case("{{ [1, 2]|find(x =>\nblock('b', 'index.twig')) }}{% block b %}B{% endblock %}"),
    ]
    return c


def operands():
    """P2 (operand order of `*`, `b-and`, `b-or`, `b-xor`), P3 (constant arrays in `~`), P4 (prefixes before `**`)."""
    d = {"a": [1], "s": "abc", "f": 2.5, "n": 5, "m": {"k": 1}}
    kinds = ["1", "(1 + 2)", "-1", "(not 0)", "[1]", "(1 / 0)", "n", "(n + 1)", "-n", "(not n)", "(n ?? 1)",
             "(n ?: 1)", "(true ? n : 1)", "(n // 2)", "(n not in [1])", "(n is null)", "(n is even)", "[n]",
             "\"#{n}\"", "(x => x)", "max(1, 2)", "max(1, 2, 3)", "m.k", "(n|abs)", "(n in [1])", "(1..1)", "_self",
             "_charset", "_context", "m['k']"]
    c = []
    for k in kinds:
        c.append(case("{{ %s * a }}" % k, data_json=d, config=OFF))
        c.append(case("{{ a b-or %s }}" % k, data_json=d, config=OFF))
        c.append(case("{{ m.k b-and %s }}" % k, data_json={"m": {"k": [1]}, "n": 5, "a": [1]}, config=OFF))
    c.append(case("{{ m['k'] * a }}", data_json=d, config=STRICT))
    for t in ["{{ 2 * 3 * a }}", "{{ a * 2 * 3 }}", "{{ 0.5 b-and s }}", "{{ s b-and 0.5 }}", "{{ 'x' b-or 1.5 }}",
              "{{ 1.5 b-or 'x' }}", "{{ 1 + a }}", "{{ 1 - a }}", "{{ 1 % a }}", "{{ 1 ** a }}", "{{ 1 / a }}",
              "{{ -a }}", "{{ +a }}", "{{ 'abc' * 1 }}", "{{ 1 * 'abc' }}", "{{ a * 'abc' }}", "{{''*b}}",
              "{{0*a/(9)}}", "{{0.4b-or('')|date('c')}}", "{{1E25b-or range(5,4)}}"]:
        c.append(case(t, data_json=dict(d, b=True), config=OFF))
    dd = {"x": 7, "y": None, "z": 0, "items": [1]}
    for t in ["(7 // 2) ** -1", "(x // 2) ** 2", "-(x // 2) ** 2", "(-(x // 2)) ** 2", "not x ** 2", "(not x) ** 2",
              "not -x ** 2", "(x not in items) ** 2", "x is not null ** 2", "y is not null ** 2", "(x // z) ** 2",
              "(10 // 5) ** 2000", "(x // 2) ** 0.5", "-x ** 2", "(x in items) ** 2", "2 ** (x // 2) ** 2",
              "(1 not in [1]) ** 2"]:
        c.append(case("{{ dump(%s) }}" % t, data_json=dd, config=DEBUG, extensions=["debug"]))
    c.append(case("{%if f is not iterable**-1%}{%endif%}", data_json={}))
    for t, kw in [("{{ o ~ [] }}", {}), ("{{ [] ~ o }}", {}), ("{{ [] ~ [] }}", {"errors": "throw"}),
                  ("{{ nan ~ [1] }}", {"config": OFF}), ("{{ [1] ~ nan }}", {"config": OFF}),
                  ("{{ o ~ ([1] + [2]) }}", {}), ("{{ o ~ [s] }}", {}), ("{{ o ~ {(1 + 1): 1} }}", {}),
                  ("{{ o ~ {1.5: 1} }}", {}), ("{{ o ~ [true ? 1 : 2] }}", {}),
                  ("{{ \"#{o}#{[1]}\" }}", {"config": OFF}), ("{{ \"#{[1]}#{o}\" }}", {"config": OFF}),
                  ("{% set s = ao ~ [1] %}", {}), ("{{ o ~ [null] }}", {"errors": "throw"})]:
        c.append(case(t, data_php=STD, **kw))
    return c


def null_safe():
    """P5 to P8: the attributes Twig sets on node objects while compiling null-safe chains."""
    a = {"a": {"b": "B"}, "c": {"d": {"e": "E"}}, "arr": {"a": 1, "p": {"q": 2}}, "nested": [[1, 2], [3]],
         "n": None, "l": None, "u": {"c": None}}
    c = []
    for t in ["{{ undef?.x.y ?? 'd' }}", "{{ undef?.x.y is defined ? 'y' : 'n' }}", "{{ undef?.x.y|default('d') }}",
              "{{ undef?.x().y ?? 'd' }}", "{{ undef?.x[0] ?? 'd' }}", "{{ u?.c ?? 'd' }}", "{{ u?.c?.z.w ?? 'd' }}",
              "{{ u.b?.c.z ?? 'd' }}", "{{ undef?.x ?? 'd' }}", "{{ undef?.x is defined ? 'y' : 'n' }}",
              "{{ u?.c.z is defined ? 'y' : 'n' }}", "{{ undef?.x.y }}", "{{ n?.x.y ?? 'd' }}"]:
        c.append(case(t, data_json=a, config=STRICT))
    c.append(case("{{ [1,2]|join(a?.b) }}|{{ [1,2]|join(c?.d?.e) }}|" * 5, data_json=a, config=OFF))
    c.append(case("{{ [1,2]|join(a?.b ~ c?.d?.e) }}{{ max(a?.b, c?.d?.e) }}{{ [1,2]|join(c?.d?.e) }}" * 3,
                  data_json=a, config=OFF))
    c.append(case("{{ arr?.p?.q ?? 'd' }}", data_json=a, extensions=["sandbox"], config=OFF))
    c.append(case("{{ arr?.p?.q ?? 'd' }}", data_json=a, extensions=["sandbox"], config=STRICT))
    c.append(case("{{ arr?.p?.q ?? 'd' }}", data_json=a, config={"autoescape": "html"}))
    c.append(case("{{ nested[0.5]?.1 ?? 7 }}", data_json=a, config={"autoescape": "html"}))
    c.append(case("{{ arr?.p?.q ?? arr?.p?.q }}", data_json=a, config={"autoescape": "html"}))
    for t in ["{{ n[l?.a.b] }}", "{% set z = 1 %}{{ n[l?.a.b] }}", "{{ 'x' starts with 'x' }}{{ n[l?.a.b] }}",
              "{% with {} %}{{ n[l?.a.b] }}{% endwith %}", "{% for i in [1, 2] %}{{ n[l?.a.b] }}{% endfor %}",
              "{{ arr[l?.a.b] }}|{{ n[l?.a.b] }}"]:
        c.append(case(t, data_json=a, extensions=["sandbox"], config=OFF))
    c.append(case("{{ n[l?.a.b] }}", data_json=a, extensions=["sandbox"], config=OFF, errors="throw"))
    for t in ["{{ x|default(a?.b) }}", "{{ a.zz|default(a?.b) }}", "{{ a|default(a?.b)|json_encode }}"]:
        c.append(case(t, data_json=a, config=OFF))
    return c


def macro_defaults():
    """P9: macro defaults PHP cannot fold are evaluated when the macro is called without the argument."""
    c = []
    for d in ["-'a'", "+'a'", "-[1]", "[-'a']", "- -'a'", "-'1a'", "+'1a'", "[1, -'1a']", "{a: -'1a', b: 2}", "-'1'",
              "-null", "-true", "[-1, +2]", "-1.5", "{(1): -'1a'}", "{1.5: 2}", "-'1e2'"]:
        c.append(case("{%% macro m(a = %s) %%}[{{ a|json_encode }}]{%% endmacro %%}{{ _self.m() }}{{ _self.m() }}"
                      "{{ _self.m(7) }}" % d, config=OFF))
    c += [
        case("{% macro m(a = -'a') %}\n[{{ a }}]\n{% endmacro %}\nx\n{{ _self.m() }}\ny\n{{ 'z' }}\n", config=OFF),
        case("{% block b %}\nB\n{% endblock %}\n{% macro m(a = -'a') %}\n[{{ a }}]\n{% endmacro %}\n{{ _self.m() }}\n",
             config=OFF),
        case({"index.twig": "{% import 'm.twig' as m %}\n{{ m.m() }}\n",
              "m.twig": "{% macro m(a = -'a') %}{{ a }}{% endmacro %}"}, config=OFF),
        case("{% macro m(a = -'1a') %}{{ a }}{% endmacro %}{{ _self.m() }}{{ _self.m() }}", config=OFF, errors="throw"),
        case("{% macro m(a = -'1a', b = 2, c = +'2b') %}{{ a }}{{ b }}{{ c }}{% endmacro %}{{ _self.m(b: 5) }}|"
             "{{ _self.m(1) }}|{{ _self.m(c: 3) }}", config=OFF),
        case("{% macro m(a = -'a', b = -'b') %}{{ a }}{% endmacro %}{{ _self.m(1) }}", config=OFF),
        case("{% macro m(a = [-'1a']) %}{{ a|json_encode }}{% endmacro %}{{ _self.m() }}{{ _self.m() }}", config=OFF),
        case("{% macro m(a = -'1a') %}{{ a }}{% endmacro %}{% macro n(a = -'1a') %}{{ a }}{% endmacro %}"
             "{{ _self.m() }}{{ _self.n() }}{{ _self.m() }}", config=OFF),
        case("{% macro m(a = -'a') %}{{ a }}{% endmacro %}{{ _self.m(...[]) }}", config=OFF),
        case("{% macro m(a, b = -'a') %}{{ a }}{% endmacro %}{{ _self.m() }}", config=OFF),
    ]
    return c


def loops():
    """P13: the `loop` bookkeeping writes of ForLoopNode (PHP's dimension fetches for writing)."""
    c = [
        case("{% for x in [1] %}{{ loop.index }}{% for t in [1] %}{% set _parent = [] %}{% endfor %}{% endfor %}"),
        case("{% for x in [1] %}{{ loop.index }}{% for t in [1] %}{% set _parent = [] %}{% endfor %}{% endfor %}",
             errors="throw"),
    ]
    for v in ["false", "null", "[]", "'str'", "5", "true", "o"]:
        c.append(case("{%% for x in [1, 2] %%}{%% set loop = %s %%}{%% endfor %%}" % v, data_php=STD))
    c += [
        case("{% for x in [1, 2] %}{% set loop = ao %}{{ ao|json_encode }}{% endfor %}{{ ao.index0 }}",
             data_php="return ['ao' => new \\ArrayObject(['index0' => 5])];"),
        case("{% for x in [1, 2] %}{% set loop = ai %}{% endfor %}{{ ai|json_encode }}",
             data_php="return ['ai' => new \\ArrayIterator([1])];"),
        case("{% for x in [1, 2] %}{% set loop = ao %}{% endfor %}{{ ao|json_encode }}",
             data_php="return ['ao' => new \\ArrayObject(['revindex0' => 3, 'revindex' => 4])];"),
        case("{% for loop in users %}{{ loop.name }}{% endfor %}",
             data_php="return ['users' => [new \\ArrayObject(['name' => 'Bob']), new \\ArrayObject(['name' => 'Cy'])]];"),
    ]
    return c


def calls():
    """P14 (named arguments checked while unpacking), P15 (callable values), P16 (spreads in special node
    classes)."""
    x = {"x": {"a": 1}}
    c = [case(t) for t in [
        "{{ 'x'|first(...{'k': 1, 0: 2}) }}", "{{ 1|abs(...{0: 1, 'a': 2}) }}", "{{ '%s'|format(...{a: 1, b: 2}) }}",
        "{{ 'abc'|slice(...{length: 1, start: 1}) }}", "{{ 'abc'|slice(...{start: 1, start2: 1}) }}",
        "{{ 'abc'|slice(1, ...{start: 2}) }}", "{{ range(...{low: 1, high: 3})|join }}",
        "{{ [1] has some no }}", "{{[2]has every b}}", "{{ [1] has some ['a', 'b'] }}", "{{ [1] has some [1, 2] }}",
        "{{ [1] has some ['stdClass', 'x'] }}", "{{ [1] has every 5 }}", "{{ [1]|map(true) }}",
        "{{ [1]|map(['a', 'b']) }}", "{{ [1]|map([1, 2]) }}", "{{ [1]|find(['stdClass', 'x']) }}",
    ]]
    c += [
        case("{{ x is constant(...['PHP_EOL']) ? 'y' : 'n' }}", data_json=x, config=OFF),
        case("{{ \"\\n\" is constant(...['PHP_EOL']) ? 'y' : 'n' }}", config=OFF),
        case("{{ x is constant(...{name: 'PHP_EOL'}) ? 'y' : 'n' }}", data_json=x, config=OFF),
        case("{{ x is constant(...[]) ? 'y' : 'n' }}", data_json=x, config=OFF),
        case("{{ x is constant(...{k: 1}) ? 'y' : 'n' }}", data_json=x, config=OFF),
        case("{{ 'a'|default(...[]) }}", config=OFF),
        case("{{ [n, ...null] }}", data_json={"n": 1}, config=OFF),
        case("{{ max(1, ...null) }}", config=OFF),
        case("{{ x is null(...[]) ? 'y' : 'n' }}", data_json=x, config=OFF),
        case("{{ [...[1, 2], 3]|join }}", config=OFF),
    ]
    return c


def folding():
    """P31: the operands of `*`, `b-and`, `b-or`, `b-xor` and `~` inside array literals, whose elements
    zend_try_ct_eval_array() folds with zend_eval_const_expr() before compiling them: a conditional (`? :`, `?:`, `??`)
    with a constant test is replaced by the branch it chooses, so it is a constant (P2's operand swap, P3's cast of a
    constant array) or what that branch compiles to; calls, closures and the code outside literals are not folded."""
    d = {"a": [1], "m": {"k": 1}, "z": 0}
    objs = "return ['o' => new \\stdClass(), 'nan' => NAN, 'a' => [1]];"
    c = [case(t, data_json=d, config=OFF) for t in [
        "{{ [(false ?? '') * a]|length }}", "{{ [(true ? false : 1) * a]|length }}",
        "{{ [(0 ?: false) * a]|length }}", "{{ [(null ?? false) * a]|length }}",
        "{{ [(false ?? 0) b-and a]|length }}", "{{ [(true ? 1.5 : 0) b-or a]|length }}",
        "{{ [(false ?? 0) b-xor a]|length }}", "{{ max([(false ?? '') * a]) }}",
        "{{ {k: (false ?? '') * a}|length }}", "{{ a.x((false ?? '') * a) }}",
        "{% macro m(x) %}{% endmacro %}{{ _self.m((false ?? '') * a) }}", "{{ [[(false ?? '') * a]]|length }}",
        "{{ {((false ?? '') * a): 1}|length }}", "{{ [...[(false ?? '') * a]]|length }}",
        "{{ [max((false ?? '') * a, 1)]|length }}", "{{ [[a]|map(x => (false ?? '') * x)]|length }}",
        "{{ [((false ?? '') * 2) * a]|length }}", "{{ [-(true ? false : 1) * a]|length }}",
        "{{ [((false ?? '')|raw) * a]|length }}", "{{ [((false ?? '')|default(1)) * a]|length }}",
        "{{ [(true ? _context : 1) * m.k]|length }}", "{{ [(null ?? _context) * m.k]|length }}",
        "{{ [(0 ?: _context) * m.k]|length }}", "{{ [(_context ?: 1) * m.k]|length }}",
        "{{ (true ? _context : 1) * m.k }}", "{{ [(true ? m.k : 1) * _context]|length }}",
        "{{ [(true ? _context : 1) b-or m.k]|length }}", "{% with {k: (false ?? '') * a} %}{% endwith %}",
        "{% set x = [(false ?? '') * a] %}", "{% set x, y = (false ?? '') * a, 1 %}",
        "{% for x in [(false ?? '') * a] %}{% endfor %}", "{{ [((false ?? '') * a) is same as(1)]|length }}",
        "{{ [(1 == 1) and ((false ?? '') * a)]|length }}", "{{ [((false ?? '') * a) ? 1 : 2]|length }}",
        "{{ [true ? 1 : (false ?? '') * a]|length }}", "{{ [false ? 1 : (false ?? '') * a]|length }}",
        "{{ [(z is defined ? false : 1) * a]|length }}", "{{ (false ?? '') * a }}", "{{ (true ? false : 1) * a }}",
    ]]
    c.append(case({"index.twig": "{% include 't.twig' with {k: (false ?? '') * a} %}", "t.twig": "T"}, data_json=d,
                  config=OFF))
    c.append(case("{{ [(false ?? '') * a]|length }}", data_json=d, config=STRICT))
    c.append(case("{{ dump((false ?? '') * a) }}", data_json=d, config=DEBUG, extensions=["debug"]))
    for t in ["{{ [o ~ (true ? [] : 1)]|length }}", "{{ [(true ? [] : 1) ~ o]|length }}", "{{ o ~ (true ? [] : 1) }}",
              "{{ [nan ~ ([1] ?? 2)]|length }}", "{{ [([1] ?? 2) ~ nan]|length }}", "{{ [o ~ (null ?? [])]|length }}"]:
        c.append(case(t, data_php=objs, config=OFF))
    return c


def fold_lines():
    """P35: the lines of the constants zend_try_ct_eval_array() leaves inside array literals. A conditional whose test
    folds is replaced by the branch it chooses; a value of the parser (a literal, a concatenation the parser folds)
    keeps the line it is written on, a value zend_eval_const_expr() creates takes the literal's line. The literal's
    line is its first value's, which comes after the dynamic key of its first element."""
    c = []
    # the operand of `*` after the marker of `r` (line 2): its own line, or the literal's (line 1)
    for y in ["''?''", "1?'x':''", "''?:''", "null??''", "''?'':'a'~'b'", "1?'a'~1:''", "'a'~'b'", "'a'~1",
              "'a'~'b'~'c'", "1?(0?'a':'b'):'c'", "1?('a'|raw):''", "1?1.5:''", "1?0:''", "1?1E25:''", "1?0.0:''",
              "1?1~2:''", "1?1.5~'a':''", "1?('a'~'b')|raw:''", "true?'a':''", "'a'?:''", "'a'??''",
              "('a'~'b')?:''", "1?'':'a'", "('a'|raw)", "'a'~'b'|raw", "1?'a':\nzz", "\n_self is defined?'a':''",
              "1?(\n_self is defined?'a':''):''", "'a'|raw|raw", "'a' ~ 'b' ~ 1.5 ~ 0", "1 ? (0 ?: 'x') : ''",
              "1 ? (null ?? 'x') : ''", "1 ? ('x' ?? 'y') : ''", "('a' ~ 'b') ?: ('c' ~ 'd')", "(0 ~ '') ?: 'x'",
              "(1 ? 'a' : '')|raw", "'a' ~ ('b'|raw)", "1 ? ('a' ~ 'b')|raw ~ 'c' : ''", "1 ? 12345678901234567890 : ''",
              "1 ? 1.0E+25 ~ '' : ''", "1 ? (1 ? (1 ? 'a' : 0) : 1) : 2", "1 ? ''|default('x') : 0",
              # values zend_eval_const_expr() creates
              "''", "1?null:''", "1?false:''", "1?true:''", "1?[]:''", "1?-1:''", "1?-'1':''", "1?+1:''",
              "-1~'b'", "1?'a'~-1:''", "(0?'':'a')~'b'", "1?'a'~(1?'b':''):''", "1 ? (1 ? -1 : '') : ''"]:
        c.append(case("{{([[...{},\nr]*(%s)])}}" % y))
    c += [
        case("{{([[1,\nr]*(''?'')])}}"),
        case("{{[[r,\nr]*(0?'':'k')]}}"),
        # outside array literals PHP folds no conditional
        case("{{[...{},\nr]*(1?'a':'')}}"),
        case("{{[r\n*(''?'')]}}"),
        case("{{r\n*(1?'a':'')}}"),
    ]
    # the first element's dynamic key: the marker after it decides the literal's line
    s = {"s": [1]}
    for t, kw in [
        ("{{({(('')\n~s):([])+e})}}", {}),
        ("{{({(('')\n~s):([])+e})}}", {"data_json": {"s": "a", "e": 1}}),
        ("{{({(\ns):([])+e})}}", {}),
        ("{{ {('' ~\ns):\n\n\n1}|length }}", {"data_json": s, "errors": "throw"}),
        ("{{ {('' ~\ns):\n\n\ne}|length }}", {"data_json": s, "errors": "throw"}),
        ("{{ [{('' ~\ns): 1}]|length }}", {"data_json": s, "errors": "throw"}),
        ("{{ [[{('' ~\ns): 1}]]|length }}", {"data_json": s, "errors": "throw"}),
        ("{% set a, b = {('' ~\ns): 1}, 2 %}", {"data_json": s, "errors": "throw"}),
        ("{{ {(s\n~ t): [] + e} }}", {}),
        ("{{ [{(s\n~ t): 1} + e] }}", {}),
        ("{{ {('' ~\ns): 1, ('' ~ \n\nt): 2}|length }}", {"data_json": {"s": 1, "t": [1]}, "errors": "throw"}),
        ("{{ {('' ~\ns ~ ''): 1}|length }}", {"data_json": s, "errors": "throw"}),
        ("{{ [...{('' ~\ns): 1}]|length }}", {"data_json": s, "errors": "throw"}),
        ("{{ [({('' ~\ns): 1}) ?: 2]|length }}", {"data_json": s, "errors": "throw"}),
        ("{{ [{('' ~\ns): 1}|raw]|length }}", {"data_json": s, "errors": "throw"}),
        ("{{ [{(('' ~\ns) ~ ''): 1} ~ '']|length }}", {"data_json": s, "errors": "throw"}),
        ("{{ {(\ns ~ t ~\n''): 1}|length }}", {"data_json": {"s": [1], "t": 1}, "errors": "throw"}),
        ("{{ {('' ~ s|length ~\nt): 1}|length }}", {"data_json": {"s": [1], "t": [1]}, "errors": "throw"}),
        ("{{ {(\n'' ~ s): [1] ~ (1\n~ '')}|length }}", {"data_json": {"s": 1}, "errors": "throw"}),
        ("{{ {('' ~ max([1,\ns], 1)):\n2}|length }}", {"data_json": {"s": [1]}, "errors": "throw"}),
        # literals of their own inside the key (call arguments, arrow functions), with their own lines
        ("{{ {(('' ~ max({('' ~\nt): 1}|length, 0)) ~\n\n s): 1}|length }}", {"data_json": {"s": [1], "t": "a"},
                                                                          "errors": "throw"}),
        ("{{ {(('' ~ max({('' ~\nt): 1}|length, 0)) ~\n\n s): 1}|length }}", {"data_json": {"s": "b", "t": [1]},
                                                                          "errors": "throw"}),
        ("{{ {(('' ~ max([1,\n\nt]|length, 0)) ~\n s): 1}|length }}", {"data_json": {"s": [1], "t": "a"},
                                                                    "errors": "throw"}),
        ("{{ [1]|map(v => {('' ~\ns): 1})|length }}", {"data_json": s, "errors": "throw"}),
        ("{{ [1]|map(v => {('' ~\ns):\n\n1, (''\n~ s): 2})|length }}", {"data_json": s, "errors": "throw"}),
        ("{% set a, b = {('' ~\ns):\n\nx}, 2 %}", {"data_json": {"s": [1], "x": 1}, "errors": "throw"}),
        ("{{ {(\n('' ~ s) ~ (\n\nt ~ '')): 1}|length }}", {"data_json": {"s": [1], "t": "a"}, "errors": "throw"}),
        ("{{ {(\n('' ~ s) ~ (\n\nt ~ '')): 1}|length }}", {"data_json": {"s": "a", "t": [1]}, "errors": "throw"}),
        ("{{ {(\ns ~ ''):\n\nr * (1 ? 'a' : '')} }}", {"data_json": {"s": 1}}),
        ("{% with {(\n'' ~ s): 1} %}{% endwith %}", {"data_json": s, "errors": "throw"}),
        ("{% for x in {('' ~\ns): 1} %}{% endfor %}", {"data_json": s, "errors": "throw"}),
        # the literal's line comes from its first value
        ("{{ {(s ~\n''): [] + e} }}", {}),
        ("{{ {a: 1, ('' ~\ns): [] + e} }}", {}),
        ("{{ {a: 1, (\ns): ([])+e} }}", {}),
        ("{{ {(s~''):([])+\ne} }}", {}),
        ("{{ {('a'\n~'b'):([])+e} }}", {}),
    ]:
        c.append(case(t, **kw))
    return c


def statement_lines():
    """P36: the statements Twig writes after an expression without a marker of their own (the bookkeeping of
    ForLoopNode and ForNode, the end of `with`) are compiled at the line they are written on: after the last marker,
    also one in code PHP skips (a decided `or`) or replaces by a constant (the folding of an array literal)."""
    m = {"map": {"x": 1}}
    c = [
        case("{%for _parent in map%}{%set\nr,v=(5),y%}{%endfor%}", data_json=m),
        case("{%for _parent in map%}{%set r = [1,\ny]%}{%endfor%}", data_json=m),
        case("{%for loop in items%}{%if {'':''} or\nc%}{%endif%}{%endfor%}", data_json={"items": [1]}),
        case("{%for _parent in map%}{%set r = \ny%}{%endfor%}", data_json=m),
        case("{%for _parent in map%}{%set r,v=(5),y%}{%endfor%}", data_json=m),
        case("{% for x in [1, 2] %}{% set loop = [1,\ny] %}{% endfor %}", errors="throw"),
        case("{% for x in [1, 2] %}{% set loop = [1,\ny] %}{% endfor %}"),
        case("{% for _parent in map %}{% else %}{% set a = [1,\ny] %}{% endfor %}", data_json=m),
        case("{% for _parent in map %}{% set a = [1,\ny] %}{% else %}e{% endfor %}", data_json=m),
        case("{% for x in [1] %}{% set _parent = [1,\ny] %}{% set _parent = 5 %}{% endfor %}"),
        case("{% for x in [1] %}{% for _parent in [1] %}{% set a = [1,\ny] %}{% endfor %}{% endfor %}"),
    ]
    for body in ["{% do [1,\ny] %}", "{{ [1,\ny]|length }}", "{% if 1 or\nc %}{% endif %}", "{% if 0 and\nc %}{% endif %}",
                 "{% set a = (1 ? 2 :\nc) %}", "{% set a = [1 ? 2 :\nc] %}",
                 "{% set a %}{{ [1,\ny]|length }}{% endset %}", "{% with {a: [1,\ny]} %}{% endwith %}",
                 "{% with {a: 1} %}{% set b = [1,\ny] %}{% endwith %}", "{% set a = {(\nk): 1,\n\nb: y} %}"]:
        c.append(case("{% for x in [1, 2] %}{% set loop = 5 %}" + body + "{% endfor %}"))
    return c


def true_test():
    """P37: TrueTest converts its operand to bool (`(($tmp = x) && ...)`) at the line of `$tmp`, written before the
    operand's markers (zend_compile_assign() restores CG(zend_lineno)); PHP 8.5 warns for NAN."""
    c = []
    for t in ["{%if\nnan%}{%endif%}", "{%if 0%}{%elseif\nnan%}{%endif%}", "{{ not\nnan }}", "{{ (\nnan) ? 1 : 2 }}",
              "{{ (\nnan) ?: 2 }}", "{{ 1 xor (\nnan) }}", "{{ (\nnan) and 1 }}", "{%if 1 and\n('' or nan)%}{%endif%}",
              "{{ [\n1, (\nnan) ? 1 : 2]|length }}", "{{ [(\nnan) and 1]|length }}", "{{ 1 and (\nnan) }}",
              "{% if (\nnan) or 1 %}{% endif %}", "{{ (\nnan) ?: (\nnan) }}", "{{ [1, (\nnan) ?: 2]|length }}",
              "{% if 0 %}{% elseif 0 %}{% elseif\nnan %}{% endif %}", "{{ not not\nnan }}",
              "{% set a = (\nnan) ? 1 : 2 %}", "{{ ((\nnan) ? 1 : 2) ? 3 : 4 }}", "{%if nan%}{%endif%}",
              "{%if(3)\nand(nan)!=x%}{%endif%}"]:
        c.append(case(t, data_php=STD, errors="throw"))
    for t in ["{%if (''or\nnan)%}{%endif%}", "{%if\nnan%}{%endif%}", "{{ (\nnan) ?: 2 }}", "{{ not\nnan }}"]:
        c.append(case(t, data_php=STD))
    for t in ["{% set m %}x{% endset %}{{ m ? 'y' : 'n' }}|{{ m ?: 'n' }}|{{ not m ? 'y' : 'n' }}",
              "{% set m %}{% endset %}{{ m ? 'y' : 'n' }}|{{ m ?: 'n' }}|{{ m and 1 ? 'y' : 'n' }}"]:
        c.append(case(t))
    return c


TOPICS = {"lines": lines, "operands": operands, "null_safe": null_safe, "macro_defaults": macro_defaults,
          "loops": loops, "calls": calls, "folding": folding, "fold_lines": fold_lines,
          "statement_lines": statement_lines, "true_test": true_test}

DESCRIPTIONS = {
    "lines": "Lines of runtime errors: constants PHP folds inside array literals take the literal's line (P1), the "
             "message conversion of {% deprecated %} (P10), missing arguments of arrow functions (P11), $blocks in "
             "arrow functions (P12).",
    "operands": "Operand order of the commutative opcodes (P2), casts of constant arrays in `~` (P3), prefix operators "
                "before `**` (P4).",
    "null_safe": "Compile-time state of null-safe chains: strictness of the hoisted object (P5), node identity (P6, "
                 "P7), the variable a key compiled twice reads (P8).",
    "macro_defaults": "Macro defaults PHP cannot fold, evaluated when the argument is missing (P9).",
    "loops": "The loop bookkeeping writes of ForLoopNode (P13).",
    "calls": "Named arguments while unpacking (P14), callable values (P15), spreads in special node classes (P16).",
    "folding": "Operands of `*`, `b-and`, `b-or`, `b-xor` and `~` inside array literals, which zend_try_ct_eval_array() "
               "folds before compiling them: conditionals with a constant test become the branch they choose (P31).",
    "fold_lines": "Lines of the constants zend_try_ct_eval_array() leaves inside array literals: a chosen branch or a "
                  "concatenation the parser folded keeps its own line, other folded values take the literal's line, "
                  "which comes after the dynamic key of its first element (P35).",
    "statement_lines": "Lines of the statements Twig writes after an expression: the loop bookkeeping of ForLoopNode "
                       "and ForNode, the end of `with` (P36).",
    "true_test": "Line of the bool conversion of TrueTest: the line of `$tmp`, before the operand's markers (P37).",
}


def main():
    record = "--record" in sys.argv
    wanted = [a for a in sys.argv[1:] if not a.startswith("--")] or list(TOPICS)
    fuzz = shlex.split(os.environ.get("TWIG_FUZZ", "cargo run -q -p twig-fuzz --release --"))
    for topic in wanted:
        cases = TOPICS[topic]()
        for i, c in enumerate(cases):
            c["id"] = "%s-%03d" % (topic, i + 1)
        gen = os.path.join(OUT, topic + ".gen.json")
        with open(gen, "w", encoding="utf-8", newline="\n") as f:
            json.dump({"cases": cases, "isolation": "process"}, f, indent=1, ensure_ascii=False)
            f.write("\n")
        print(topic, len(cases), "cases")
        if record:
            out = os.path.join(OUT, topic + ".json")
            subprocess.run(fuzz + ["record", gen, "--out", out, "--description", DESCRIPTIONS[topic]], check=True)


if __name__ == "__main__":
    main()
