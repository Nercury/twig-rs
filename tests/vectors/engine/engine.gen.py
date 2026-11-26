"""Writes the oracle batches tests/vectors/engine/<topic>.gen.json of the engine tests (tests/engine.rs): templates
rendered by a plain Twig 3.31.1 environment (autoescape off, optimizations 0 unless a case says otherwise) in
non-strict and strict mode, with their outputs, error messages with lines and PHP diagnostics. Object cases build
the PHP classes below in `data_php`; tests/engine/objects.rs has their Rust versions.

The composition topics (inheritance, traits, embed, include, macros, composition_errors, sandbox) render several
templates, with template wrappers, globals and sandbox policies (see `case()`); the `api` topic runs PHP scripts of
API calls (`API_PHP`) whose JSON results tests/engine/api.rs compares call by call. The `compile_errors` topic
loads templates whose generated PHP fails to compile; PHP's fatal errors (`"phase": "fatal"`) are compared with
twig-rs's CompileError.

Regenerate from the repository root (Git Bash on Windows needs MSYS_NO_PATHCONV=1):

    python -I tests/vectors/engine/engine.gen.py [topic ...]            # writes <topic>.gen.json
    python -I tests/vectors/engine/engine.gen.py --oracle [topic ...]   # and runs them: <topic>.json

`--oracle` pipes each batch through `docker run --rm -i twig-oracle:3.31.1 php runner.php --pretty`.
"""

import json
import os
import subprocess
import sys

OUT = os.path.dirname(os.path.abspath(__file__))

BASE_CONFIG = {"autoescape": False, "optimizations": 0}

PHP_CLASSES = r"""
if (!class_exists('TwigRsBag', false)) {
    class TwigRsBag {
        public $pub = 'pub';
        public $nul = null;
        public ?int $typed;
        private $priv = 'priv';
        protected $prot = 'prot';
        const KONST = 'konst';
        public function getPriv() { return 'getPriv()'; }
        public function getFoo() { return 'getFoo()'; }
        public function isBar() { return 'isBar()'; }
        public function hasBaz() { return 'hasBaz()'; }
        public function isBaz() { return 'isBaz()'; }
        public function hasQux() { return 'hasQux()'; }
        public function get() { return 'get()'; }
        public function pub() { return 'pub()'; }
        public function args(...$a) { return json_encode($a); }
        public function named($first, $second = 'S') { return $first . '-' . $second; }
        public function getSelf() { return $this; }
        public function getNull() { return null; }
        public function __toString(): string { return 'bag'; }
    }
    class TwigRsMagic {
        public function __isset($n) { return $n === 'magic'; }
        public function __get($n) { return 'got ' . $n; }
        public function __call($n, $a) {
            if ($n === 'bad') { throw new \BadMethodCallException('bad call'); }
            return 'called ' . $n . '(' . count($a) . ')';
        }
    }
    class TwigRsAccess implements \ArrayAccess, \Countable, \IteratorAggregate {
        private $data;
        public function __construct($data) { $this->data = $data; }
        public function offsetExists($o): bool { return isset($this->data[$o]); }
        public function offsetGet($o): mixed { return $this->data[$o] ?? null; }
        public function offsetSet($o, $v): void {}
        public function offsetUnset($o): void {}
        public function count(): int { return count($this->data); }
        public function getIterator(): \Iterator { return new \ArrayIterator($this->data); }
    }
    class TwigRsIter implements \Iterator {
        private $i = 0;
        public $log = [];
        public function __construct(private array $items) {}
        public function current(): mixed { $this->log[] = 'current'; return $this->items[$this->i]; }
        public function key(): mixed { $this->log[] = 'key'; return 'k' . $this->i; }
        public function next(): void { $this->log[] = 'next'; $this->i++; }
        public function rewind(): void { $this->log[] = 'rewind'; $this->i = 0; }
        public function valid(): bool { $this->log[] = 'valid'; return $this->i < count($this->items); }
    }
    class TwigRsThrower {
        public function boom() { throw new \RuntimeException('boom!'); }
        public function getValue() { return 42; }
    }
}
"""

# object name -> PHP expression (the Rust side builds the same object, see tests/engine/objects.rs)
PHP_OBJECTS = {
    "bag": "new TwigRsBag()",
    "magic": "new TwigRsMagic()",
    "access": "new TwigRsAccess(['a' => 1, 'b' => null, 'c' => [1, 2]])",
    "iter": "new TwigRsIter(['x', 'y', 'z'])",
    "thrower": "new TwigRsThrower()",
    "arrobj": "new \\ArrayObject(['a' => 1, 'b' => 2])",
    "markup": "new \\Twig\\Markup('<b>bold</b>', 'UTF-8')",
    "empty_markup": "new \\Twig\\Markup('', 'UTF-8')",
    "zero_markup": "new \\Twig\\Markup('0', 'UTF-8')",
    "html_markup": "new \\Twig\\Markup('<i>x</i>', 'UTF-8', ['html'])",
}


def php_str(s):
    """A PHP single-quoted string literal."""
    return "'" + s.replace("\\", "\\\\").replace("'", "\\'") + "'"


def case(tpl, data=None, objects=(), strict=False, templates=None, config=None, errors="collect", main=None,
         load_all=None, globals=None, sandbox=None, wrappers=None, foreign_wrappers=None, extensions=None):
    """One render case. Composition cases add: `main` (the template to render), `load_all` (false: only `main` is
    loaded before rendering), `globals`, `extensions` (`string_loader`), `sandbox` ({"global", "policy"} as the
    oracle reads it, plus `strict` and `tests`, set from `data_php`), `wrappers` (variable => template of the
    environment, `$twig->load()`) and `foreign_wrappers` (variable => [name, source] of another environment)."""
    cfg = dict(BASE_CONFIG)
    cfg["strict_variables"] = strict
    if config:
        cfg.update(config)
    c = {"templates": templates or {"index.twig": tpl}, "env": "plain", "config": cfg, "errors": errors}
    if main is not None:
        c["main"] = main
    if load_all is not None:
        c["load_all"] = load_all
    if globals:
        c["globals"] = globals
    if extensions:
        c["extensions"] = extensions
    setup = ""
    if sandbox is not None:
        c["sandbox"] = sandbox
        if sandbox.get("strict") or sandbox.get("tests"):
            setup += "$p = $twig->getExtension('Twig\\\\Extension\\\\SandboxExtension')->getSecurityPolicy(); "
            if sandbox.get("strict"):
                setup += "$p->setStrict(true); "
            if sandbox.get("tests"):
                setup += "$p->setAllowedTests(json_decode(%s, true)); " % php_str(json.dumps(sandbox["tests"]))
    if objects or wrappers or foreign_wrappers or setup:
        parts = ["'%s' => %s" % (name, PHP_OBJECTS[name]) for name in objects]
        for variable, template in (wrappers or {}).items():
            parts.append("%s => $twig->load(%s)" % (php_str(variable), php_str(template)))
        for variable, (name, source) in (foreign_wrappers or {}).items():
            parts.append("%s => (new \\Twig\\Environment(new \\Twig\\Loader\\ArrayLoader([%s => %s])))->load(%s)" % (
                php_str(variable), php_str(name), php_str(source), php_str(name)))
        if wrappers:
            c["wrappers"] = wrappers
        if foreign_wrappers:
            c["foreign_wrappers"] = foreign_wrappers
        data_json = json.dumps(data or {})
        c["data_php"] = (PHP_CLASSES if objects else "") + setup + "return array_merge(json_decode(%s, true), [%s]);" % (
            json.dumps(data_json), ", ".join(parts))
        if objects:
            c["objects"] = list(objects)
        c["data_json_rust"] = data or {}
    elif data is not None:
        c["data_json"] = data
    return c


# Topics whose cases run in a PHP process each: PHP keeps compiled template classes for the whole process, so a later
# case with the same template and options would reuse the class without compiling it again (and without the
# compile-time deprecations); twig-rs compiles in every fresh environment.
PROCESS_ISOLATED = {"inheritance", "traits", "embed", "include", "macros", "composition_errors", "sandbox", "api",
                    "compile_errors"}


# With `cache => false` PHP eval()s every compiled template from the same line of Environment.php, so all templates
# share one file name and Error::guess() matches the frames of other templates (an imported macro's body) against the
# handling template's debug info. twig-rs keeps the templates apart like a filesystem cache does: the composition
# topics render with a filesystem cache of their own per case (reference/deviations.md).
def write(topic, cases, oracle):
    for i, c in enumerate(cases):
        c["id"] = "%s-%03d" % (topic, i + 1)
        if topic in PROCESS_ISOLATED and c.get("kind", "twig") == "twig":
            c["config"]["cache"] = "/tmp/twig-rs-cache/" + c["id"]
    batch = {"cases": cases, "isolation": "process" if topic in PROCESS_ISOLATED else "none"}
    gen_path = os.path.join(OUT, topic + ".gen.json")
    with open(gen_path, "w", encoding="utf-8", newline="\n") as f:
        json.dump(batch, f, indent=1, ensure_ascii=False)
        f.write("\n")
    if not oracle:
        print(topic, len(cases), "cases")
        return
    with open(gen_path, "rb") as f:
        data = f.read()
    p = subprocess.run(["docker", "run", "--rm", "-i", "twig-oracle:3.31.1", "php", "runner.php", "--pretty"],
                       input=data, capture_output=True)
    if p.returncode != 0:
        print(p.stderr.decode(), file=sys.stderr)
    results = json.loads(p.stdout.decode())
    out_path = os.path.join(OUT, topic + ".json")
    with open(out_path, "w", encoding="utf-8", newline="\n") as f:
        json.dump(results, f, indent=1, ensure_ascii=False)
        f.write("\n")
    failures = [r for r in results if r.get("phase") in ("fatal", "lost")]
    print(topic, len(cases), "cases", "fatal:" if failures else "", failures[:1])


ITEMS = {"items": [1, 2, 3], "map": {"a": 1, "b": 2}, "name": "bob", "empty": [], "nul": None,
         "t": True, "f": False, "zero": 0, "s": "str", "fl": 1.5, "nested": {"a": {"b": {"c": "deep"}}},
         "list": ["x", "y"], "num_keys": {"1": "one", "2": "two"}, "big": 9223372036854775807}


def output():
    c = []
    for tpl in [
        "plain text", "{{ 1 }}|{{ -1 }}|{{ 0 }}", "{{ 1.0 }}|{{ 0.1 + 0.2 }}|{{ 1e100 }}|{{ -0.0 }}|{{ 1/3 }}",
        "{{ 1e14 }}|{{ 1e15 }}|{{ 0.0001 }}|{{ 0.00001 }}|{{ 123456789012345678 }}",
        "{{ true }}|{{ false }}|{{ null }}|{{ none }}", "{{ 'a' ~ 'b' ~ 1 ~ 1.5 ~ true ~ false ~ null }}",
        "{{ [1, 2] }}", "{{ items }}", "{{ {a: 1} ~ 'x' }}", "a{# comment #}b",
        "{{ \"a#{name}b\" }}|{{ \"#{1 + 2}\" }}|{{ 'no #{interp}' }}",
        "{{ big }}|{{ big + 1 }}|{{ -big - 2 }}", "{{ 2 ** 63 }}|{{ 2 ** -1 }}|{{ 10 ** 20 }}",
        "line1\nline2 {{ name }}\n", "{{- ' a ' -}}  |  {{~ 'b' ~}}  x",
        "{% verbatim %}{{ x }}{% endverbatim %}", "{{ \"\\u00e9\" }}|{{ 'é' }}",
    ]:
        c.append(case(tpl, ITEMS))
    c.append(case("{% flush %}a{% flush %}b"))
    return c


def expressions():
    exprs = [
        # arithmetic
        "1 + 2 * 3", "(1 + 2) * 3", "10 / 4", "10 / 5", "7 // 2", "-7 // 2", "7.5 // 2", "7 % 3", "-7 % 3",
        "2 ** 3 ** 2", "-2 ** 2", "(-2) ** 2", "- -2 ** 2", "-2 ** -2", "+2 ** 2", "-zero ** 2", "2 ** -1 * 3", "'5' + 3", "'1.5' + 1", "null + 1", "true + 1", "[1] + [2, 3]",
        "'abc' ~ 1 + 2", "1 + 2 ~ 3", "'10' == '1e1'", "'abc' == 0", "null < -1", "1 <=> 2", "'a' <=> 'b'",
        "[1, 2] == [1, 2]", "[1, 2] === [2 => 1]", "1 === 1.0", "1 !== '1'", "1 != 2", "2 >= 2", "3 <= 2", "3 > 2",
        # bitwise
        "5 b-and 3", "5 b-or 3", "5 b-xor 3", "'a' b-and 'b'", "'ab' b-or 'c'",
        # logic
        "true and false", "true or false", "true xor true", "false xor true", "not true", "not 0", "not '0'",
        "not '0.0'", "not []", "not [0]", "1 and 'a'", "0 or ''", "not null", "not not 1",
        # unary
        "-'3'", "+'4.5'", "-null", "-true", "- -1", "-(1 + 2)",
        # strings
        "'abc' starts with 'a'", "'abc' starts with 'b'", "'abc' ends with 'c'", "1 starts with '1'",
        "'1' starts with 1", "'abc' matches '/^a.c$/'", "'abc' matches '/^b/'", "name matches '/o/'",
        # containment
        "1 in [1, 2]", "'1' in [1, 2]", "null in [0]", "0 in ['a']", "1 in {a: 1}", "'a' in {a: 1}",
        "1.0 in [1]", "'1e1' in ['10']", "'' in 'abc'", "1 in '123'", "'b' in 'abc'", "'d' not in 'abc'",
        "3 not in [1, 2]", "null in 'abc'", "[1] in [[1], [2]]", "'x' in null", "1 in 1",
        # range
        "(1..3)|join(',')", "(3..1)|join(',')", "('a'..'e')|join(',')", "(1..2.5)|join(',')", "(0..0)|join(',')",
        # ternary/elvis/coalesce
        "true ? 'y' : 'n'", "false ? 'y' : 'n'", "true ? 'y'", "false ? 'y'", "0 ?: 'e'", "'v' ?: 'e'",
        "null ?? 'd'", "nul ?? 'd'", "undef ?? 'd'", "'' ?? 'd'", "0 ?? 'd'", "undef ?? nul ?? 'last'",
        "map.a ?? 'x'", "map.z ?? 'x'", "items[5] ?? 'x'", "nested.a.b.c ?? 'x'", "nested.a.z.c ?? 'x'",
        # tests
        "null is null", "0 is null", "undef is not defined", "name is defined", "map.a is defined",
        "map.z is defined", "items.1 is defined", "items[7] is defined", "nested.a.b is defined",
        "nested.x.y is defined", "3 is odd", "3 is even", "4 is even", "'3' is odd", "9 is divisible by(3)",
        "9 is divisible by 4", "1 is same as(1)", "1 is same as('1')", "[] is empty", "'' is empty",
        "'0' is empty", "0 is empty", "null is empty", "{a: 1} is empty", "items is iterable", "s is iterable",
        "'abc' is true", "0 is true", "'' is true", "map is not empty", "_self is defined", "_context is defined",
        "1 is not null", "not (1 is null)",
        # literals
        "[1, 2, 3]|join(',')", "{a: 1, 'b': 2, 3: 'c'}|keys|join(',')", "{0: 'a', 1: 'b'}|join(',')",
        "{1: 'a', 0: 'b'}|keys|join(',')", "{a: 1, b: 2, a: 3}|join(',')", "{'2': 'b'}|keys|join(',')",
        "{(1 + 1): 'two'}|keys|join(',')", "{(name): 1}|keys|join(',')", "{name}|keys|join(',')",
        "[...items, 4]|join(',')", "[0, ...items]|join(',')", "{...map, c: 3}|join(',')",
        "{...map, a: 9}|join(',')", "[...[1, 2], ...{a: 3}, 4]|keys|join(',')", "{(true): 'x'}|keys|join(',')",
        "[1, [2, 3]]|length", "{a: {b: 'c'}}.a.b",
        # arrow functions
        "items|map(x => x * 2)|join(',')", "items|map((v, k) => k ~ ':' ~ v)|join(',')",
        "map|map(v => v + 10)|join(',')", "items|map(x => name)|join(',')", "items has some x => x > 2",
        "items has some x => x > 5", "items has every x => x > 0", "items has every x => x > 1",
        "[] has every x => false", "[] has some x => true", "items|map(x => x is odd ? 'o' : 'e')|join('')",
        # assignment expressions
        "(x = 5) + 1", "[a, b] = [1, 2]", "[a, , b] = [1, 2, 3]", "{a, c: d} = {a: 1, c: 2}",
        # special names
        "_self", "_charset", "_context|keys|join(',')",
        # filters and functions with arguments
        "name|upper", "name|upper|lower", "range(1, 3)|join(',')", "range(0, 10, 5)|join(',')",
        "items|join", "items|join(', ', ' and ')", "list|join(', ', ' and ')", "[]|join(',')", "'x'|join(',')",
        "null|join(',')", "items|length", "'é'|length", "null|length", "map|length", "5|length",
        "undef|default('d')", "nul|default('d')", "''|default('d')", "0|default('d')", "map.z|default('dz')",
        "map.a|default('dz')", "items|default", "[]|default('empty')", "name|default", "(undef ~ 'x')|default('d')",
        "items|keys|join(',')", "'abc'|keys|length",
    ]
    c = [case("{{ %s }}" % e, ITEMS) for e in exprs]
    # assignment side effects
    c.append(case("{% do [a, b] = [1, 2] %}{{ a }}{{ b }}|{% do [p, q] = [1] %}{{ p }}{{ q ?? 'null' }}", ITEMS))
    c.append(case("{% do {a, c: d} = {a: 1, c: 2} %}{{ a }}{{ d }}", ITEMS))
    c.append(case("{% set x = (y = 5) + 1 %}{{ x }}{{ y }}", ITEMS))
    c.append(case("{% set f = (v) => v ~ name %}{% set name = 'changed' %}{{ items|map(f)|join(',') }}|{{ items|map(v => v ~ name)|join(',') }}", ITEMS))
    c.append(case("{% set x = 1 %}{{ items|map(x => x + 1)|join(',') }}{{ x }}", ITEMS))
    c.append(case("{% do [a, b] = {x: 1, y: 2} %}{{ a ?? 'n' }}{{ b ?? 'n' }}", ITEMS))
    c.append(case("{{ (a = 1) ~ a }}", ITEMS))
    return c


def variables():
    c = []
    for strict in (False, True):
        for tpl in [
            "{{ name }}", "{{ undef }}", "{{ nul }}", "{{ map.a }}", "{{ map.z }}", "{{ map['a'] }}",
            "{{ map['z'] }}", "{{ items.1 }}", "{{ items[1] }}", "{{ items[9] }}", "{{ items.9 }}",
            "{{ s.foo }}", "{{ s['foo'] }}", "{{ s.foo() }}", "{{ nul.foo }}", "{{ nul['foo'] }}",
            "{{ nul.foo() }}", "{{ zero.foo }}", "{{ items.foo() }}", "{{ empty.x }}", "{{ empty['x'] }}",
            "{{ nested.a.b.c }}", "{{ nested.a.x.c }}", "{{ undef.a }}", "{{ num_keys.1 }}",
            "{{ num_keys['1'] }}", "{{ num_keys[1] }}", "{{ items[true] }}", "{{ items[1.7] }}",
            "{{ items['1'] }}", "{{ map[nul] }}", "{{ nul?.foo }}", "{{ nul?.foo.bar }}", "{{ map?.a }}",
            "{{ undef?.x }}", "{{ nested.a?.b.c }}", "{{ t.x }}", "{{ fl['x'] }}", "{{ items[-1] }}",
            "{{ attribute(map, 'a') }}", "{{ attribute(map, 'z') }}", "{{ attribute(items, 0) }}",
            "{{ map.(name) }}", "{{ undef|default('d') }}", "{{ map.z|default('d') }}",
            "{{ undef.x|default('d') }}", "{{ undef is defined ? 'y' : 'n' }}", "{{ nul is defined ? 'y' : 'n' }}",
            "{{ undef ?? 'd' }}", "{{ undef.x ?? 'd' }}", "{{ undef ?: 'e' }}",
            "{{ map.a.b }}", "{{ items|map(x => undef)|join }}", "{% if undef %}y{% else %}n{% endif %}",
            "{% for x in undef %}{{ x }}{% else %}E{% endfor %}", "{{ map[undef] }}",
        ]:
            c.append(case(tpl, ITEMS, strict=strict))
    return c


def statements():
    c = []
    for tpl in [
        # if
        "{% if t %}T{% endif %}{% if f %}F{% endif %}", "{% if zero %}1{% elseif nul %}2{% elseif name %}3{% else %}4{% endif %}",
        "{% if f %}1{% elseif f %}2{% else %}3{% endif %}", "{% if items %}{% if map.a == 1 %}nested{% endif %}{% endif %}",
        "{% if '0' %}y{% else %}n{% endif %}{% if '0.0' %}y{% else %}n{% endif %}{% if [] %}y{% else %}n{% endif %}",
        # for
        "{% for i in items %}{{ i }}{% endfor %}", "{% for k, v in map %}{{ k }}={{ v }},{% endfor %}",
        "{% for i in items %}{{ loop.index }}{{ loop.index0 }}{{ loop.revindex }}{{ loop.revindex0 }}{{ loop.first ? 'F' }}{{ loop.last ? 'L' }}{{ loop.length }};{% endfor %}",
        "{% for i in [] %}x{% else %}empty{% endfor %}", "{% for i in nul %}x{% else %}null-empty{% endfor %}",
        "{% for i in 's' %}x{% else %}string-empty{% endfor %}", "{% for i in 5 %}x{% endfor %}done",
        "{% for i in items %}{% for j in list %}{{ i }}{{ j }}{{ loop.parent.loop.index }} {% endfor %}{% endfor %}",
        "{% for i in items %}{{ loop.parent|keys|join(',') }}{% endfor %}",
        "{% set i = 'outer' %}{% for i in items %}{% endfor %}{{ i }}|{{ _context|keys|join(',') }}",
        "{% set x = 1 %}{% for i in items %}{% set x = i %}{% set y = i %}{% endfor %}{{ x }}{{ y ?? 'no-y' }}",
        "{% for i in items %}{{ _context|keys|join(',') }}{% endfor %}",
        "{% for i in items %}{{ _context|keys|join(',') }}{% else %}e{% endfor %}",
        "{% for k, v in {a: 1, '2': 'b', 3: 'c'} %}{{ k }}{{ k is same as(2) ? '(int)' }},{% endfor %}",
        "{% for i in items %}{% set items = [] %}{{ i }}{% endfor %}{{ items|length }}",
        "{% for loop in items %}{{ loop }}{% endfor %}", "{% for _key, v in list %}{{ _key }}{{ v }}{% endfor %}",
        "{% for i in 1..3 %}{{ i }}{% endfor %}", "{% for c in 'a'..'c' %}{{ c }}{% endfor %}",
        "{% for i in items %}{% if loop.index is odd %}{{ i }}{% endif %}{% endfor %}",
        "{% for i in items %}{% set loop = null %}{{ i }}{% endfor %}",
        "{% for i in items %}{{ i }}{% set loop = {index0: 'a'} %}{% endfor %}",
        "{% for v in map %}{{ v }}{% endfor %}{{ v ?? 'gone' }}{{ _key ?? 'nokey' }}",
        # set
        "{% set a = 1 %}{{ a }}", "{% set a, b = 1, 2 %}{{ a }}{{ b }}", "{% set a, b = 1, 2 %}{% set a, b = b, a %}{{ a }}{{ b }}",
        "{% set c %}<b>{{ name }}</b>{% endset %}{{ c }}|{{ c|length }}", "{% set e %}static{% endset %}{{ e }}",
        "{% set f %}{% endset %}[{{ f }}]{{ f is same as('') ? 'empty-string' }}",
        "{% set g %}{{ 'const' }}{% endset %}{{ g }}", "{% set h %}{{ 5 }}{% endset %}{{ h }}{{ h + 1 }}",
        "{% set x %}{% set y = 5 %}{% endset %}y={{ y }}", "{% set m %}0{% endset %}{{ m ? 'truthy' : 'falsy' }}",
        "{% set e2 %}{{ '' }}{% endset %}{{ e2 ? 'truthy' : 'falsy' }}",
        # capture and apply
        "{% apply upper %}hello {{ name }}{% endapply %}", "{% apply upper|lower %} Mixed {% endapply %}",
        "{% apply upper %}{% set q = 'in' %}{% endapply %}{{ q }}", "{% apply upper %}a{% apply lower %}B{% endapply %}c{% endapply %}",
        # with
        "{% with {a: 1, name: 'w'} %}{{ a }}{{ name }}{% endwith %}{{ a ?? 'x' }}{{ name }}",
        "{% with {a: 1} only %}{{ a }}{{ name ?? 'no-name' }}{% endwith %}", "{% with %}{% set y = 2 %}{{ y }}{% endwith %}{{ y ?? 'gone' }}",
        "{% with map %}{{ a }}{{ b }}{% endwith %}", "{% with {z: 1, items: 9} %}{{ _context|keys|join(',') }}{% endwith %}",
        "{% with {z: 1} only %}{{ _context|keys|join(',') }}{% endwith %}",
        # do, deprecated, types, guard
        "{% do 1 + 2 %}done", "{% do items|map(x => x) %}ok",
        "{% types {foo: 'string', bar?: 'int'} %}typed",
        "{% guard filter upper %}U{% else %}noU{% endguard %}", "{% guard function nope %}yes{{ nope() }}{% else %}no{% endguard %}",
        "{% guard test odd %}odd{% endguard %}", "{% autoescape 'js' %}{{ '<x>' }}{% endautoescape %}",
        "{% autoescape false %}{{ name }}{% endautoescape %}",
    ]:
        c.append(case(tpl, ITEMS))
    for strict in (False, True):
        c.append(case("{% for i in [] %}{% else %}[{{ i ?? 'q' }}]{% endfor %}", ITEMS, strict=strict))
        c.append(case("{% deprecated 'The X is deprecated.' %}{% deprecated 'D2' package='acme/pkg' version='1.2' %}ok", strict=strict))
    c.append(case("{% deprecated 'Dynamic ' ~ name %}ok", ITEMS))
    c.append(case("{% deprecated 'P' package='p' %}{% deprecated 'V' version='1.0' %}ok"))
    return c


def errors():
    c = []
    for strict in (False, True):
        for tpl in [
            "{{ 1 / 0 }}", "{{ 1 % 0 }}", "{{ 1 // 0 }}", "{{ 'abc' + 1 }}", "{{ [1] + 1 }}", "{{ -'abc' }}",
            "{{ 'x' matches '/(/' }}", "{{ 'x' matches name }}", "{{ items has some 5 }}", "{{ 5 has some x => x }}",
            "{% with 5 %}{% endwith %}", "{% with name %}{% endwith %}", "{{ [1, ...name] }}", "{{ range(...name) }}",
            "line1\n{{ undef }}", "\n\n{{ 1\n+\nundef }}", "{{ items|map(x =>\n undef) }}",
            "{% for i in items %}\n{{ i / 0 }}\n{% endfor %}", "{% set a = 1 / 0 %}", "{% if 1 / 0 %}{% endif %}",
            "\n{% do 1 / 0 %}", "{{ s.foo }}\n{{ 1 / 0 }}", "{% for i in 1..3 %}{% if i == 2 %}\n{{ 1 / 0 }}{% endif %}{% endfor %}",
            "{{ map.z.y }}", "{{ items|join(', ', 'and', 'x') }}",
            "{{ 'a'|upper(1, 2) }}", "{{ range(1) }}", "{{ range(1, 2, 3, 4) }}", "{{ items is iterable(1) }}",
            "{{ (5..'a')|join }}", "{{ 1 ~ {a: 1} }}", "{{ name ~ items }}",
        ]:
            c.append(case(tpl, ITEMS, strict=strict))
    # compile-time errors
    for tpl in [
        "{{ name|upper(foo=1) }}", "{{ name|upper(foo=1, bar=2) }}", "{{ range(start=1, end=3) }}", "{{ range(low=1) }}",
        "{{ [1, , 2] }}", "{{ name matches '/(/' }}", "{{ name matches 'nodelim' }}", "{{ name|join(glue=',', 1) }}",
        "{{ items|map(x => x, 1) }}", "{{ range(1, 2, step=1, low=1) }}", "{{ x is constant }}",
    ]:
        c.append(case(tpl, ITEMS))
    # warnings converted to exceptions (PHPUnit-like)
    for tpl in ["{{ items }}", "{{ 'a' ~ items }}", "{{ '5 apples' + 1 }}", "{{ items[1.5] }}",
                "{% for i in [1, 2] %}{% set loop = null %}{% endfor %}"]:
        c.append(case(tpl, ITEMS, errors="throw"))
    return c


def objects():
    c = []
    objs = ("bag", "magic", "access", "iter", "thrower", "arrobj", "markup", "empty_markup", "zero_markup")
    for strict in (False, True):
        for tpl in [
            "{{ bag.pub }}", "{{ bag.nul }}|{{ bag.nul is defined ? 'd' : 'u' }}", "{{ bag.typed }}",
            "{{ bag.typed is defined ? 'd' : 'u' }}", "{{ bag.priv }}", "{{ bag.prot }}", "{{ bag.KONST }}",
            "{{ bag.konst }}", "{{ bag.foo }}", "{{ bag.Foo }}", "{{ bag.FOO }}", "{{ bag.bar }}", "{{ bag.baz }}",
            "{{ bag.qux }}", "{{ bag.get }}", "{{ bag.pub() }}", "{{ bag.args(1, 'a', null) }}", "{{ bag.args() }}",
            "{{ bag.named('x') }}", "{{ bag.named(second='y', first='x') }}", "{{ bag.self.self.pub }}",
            "{{ bag.null.x }}", "{{ bag.null?.x }}", "{{ bag.nope }}", "{{ bag.nope() }}",
            "{{ bag.foo_bar }}", "{{ bag }}", "{{ bag ~ '!' }}", "{{ bag.pub is defined }}", "{{ bag.nope is defined }}",
            "{{ bag['pub'] }}", "{{ bag.getFoo }}", "{{ bag.isBar() }}",
            "{{ magic.magic }}", "{{ magic.other }}", "{{ magic.zz }}", "{{ magic.zz() }}", "{{ magic.zz(1, 2) }}",
            "{{ magic.bad }}", "{{ magic.zz is defined }}", "{{ magic.magic is defined }}",
            "{{ access.a }}", "{{ access['a'] }}", "{{ access.b }}", "{{ access['b'] }}", "{{ access.c|join(',') }}",
            "{{ access.zz }}", "{{ access['zz'] }}", "{{ access|length }}", "{% for k, v in access %}{{ k }}{% endfor %}",
            "{{ access.a is defined }}", "{{ access.zz is defined }}", "{{ 1 in access }}",
            "{% for k, v in iter %}{{ k }}={{ v }},{% endfor %}{{ iter.log|join(',') }}",
            "{% for v in iter %}{{ loop.index }}{{ loop.length ?? '-' }}{% endfor %}",
            "{{ iter|length }}", "{{ iter|keys|join(',') }}", "{{ 'y' in iter }}",
            "{{ thrower.value }}", "{{ thrower.boom }}", "{{ thrower.boom() }}",
            "{{ arrobj.a }}", "{{ arrobj['b'] }}", "{{ arrobj.zz }}", "{{ arrobj|length }}", "{{ arrobj.count }}",
            "{% for k, v in arrobj %}{{ k }}{{ v }}{% endfor %}",
            "{{ markup }}", "{{ markup|length }}", "{{ markup ? 'y' : 'n' }}|{{ empty_markup ? 'y' : 'n' }}|{{ zero_markup ? 'y' : 'n' }}",
            "{{ markup is empty }}{{ empty_markup is empty }}", "{{ markup.charset }}", "{{ 'bold' in markup }}",
            "{{ markup ~ '!' }}", "{% if empty_markup %}y{% else %}n{% endif %}{% if not zero_markup %}not{% endif %}",
            "{{ markup is true }}|{{ empty_markup is true }}", "{{ [markup]|length }}",
            "{{ bag.args(...[1, 2]) }}", "{{ bag.named(...{first: 'f'}) }}", "{{ attribute(bag, 'named', ['a', 'b']) }}",
            "{{ attribute(bag, 'pub') }}", "{{ bag.(name) }}",
        ]:
            c.append(case(tpl, {"name": "pub"}, objects=objs, strict=strict))
    return c


def lines():
    c = []
    objs = ("thrower", "iter")
    for strict in (False, True):
        for tpl in [
            "{{ 1\n/\n0 }}", "{{ 1\n/\nzero }}", "{{ zero\n/\n0 }}", "{{ range(\n1,\n2,\nzero) }}",
            "{{ range(\nzero, 2, 0) }}", "{{ 1 ~\n thrower.boom }}", "{{ thrower\n.boom }}", "{{ thrower.boom }}",
            "{{ 1 ~ \n thrower }}", "{{ thrower ~\n 'x' }}", "{{ items|map(x =>\n x / 0)|join }}",
            "{{ items|map(\nx =>\nx / zero\n)|join }}", "{{ items|map(x => x / 0)\n|join }}",
            "{% set x %}\n{{ 1 / 0 }}{% endset %}", "{% with {a: 1} %}\n{{ a / 0 }}{% endwith %}",
            "{% for i in [] %}{% else %}\n\n{{ 1 / 0 }}{% endfor %}",
            "{% for i in items %}\n{% endfor %}{{ 1 / 0 }}", "\n{% for i in items %}{{ i }}{% endfor %}\n\n{{ 1 / 0 }}",
            "{{ name ~\n undef }}", "{% if\nundef %}{% endif %}", "{{ nested\n.zz }}", "{{ nested.a\n.zz }}",
            "{{ map\n['zz'] }}", "{{\n\nundef\n}}", "{% set a = [\n1,\nundef\n] %}",
            "{{ {a:\n1,\nb: undef} }}", "{% for i in\nundef %}{% endfor %}", "{{ 'x'\nmatches\n'/(/' }}",
            "{{ items\nhas some\n5 }}", "{{ (1\n..\n'a')|join }}", "{{ 1 in\nthrower }}",
            "a\n{{ 'é'|upper }}\n{{ 1 / 0 }}", "{% apply upper %}\nx{{ 1 / 0 }}{% endapply %}",
            "{{ items|join(\n', ',\n'and',\nthrower.boom) }}", "{{ [1, 2]|map(v => [v]|map(w =>\n w / 0)|join)|join }}",
            "{% do\n1 / 0 %}", "{% set a,\nb = 1, 1 / 0 %}", "{{ undef\n?? 1 / 0 }}", "{{ 0\n?: 1 / 0 }}",
            "{{ true\n? 1 / 0\n: 2 }}", "{{ false or\n1 / 0 }}", "{{ -(\n1 / 0) }}", "{{ [\n...undef] }}",
            "{% for k, v in iter %}\n{{ v / 0 }}{% endfor %}",
        ]:
            c.append(case(tpl, ITEMS, objects=objs, strict=strict))
    return c


def diagnostics():
    c = []
    for errors in ("collect", "throw"):
        for tpl in [
            "{{ items }}", "{{ items ~ 'x' }}", "{{ '5 apples' + 1 }}", "{{ items[1.5] }}", "{{ items[nul] }}",
            "{{ {(1.5): 'x'}|keys|join }}", "{{ {(nul): 'x'}|keys|join }}", "{{ {(fl): 'x'}|keys|join }}",
            "{% for i in [1, 2] %}{% set loop = null %}{% endfor %}", "{{ 'abc' + 1 }}",
            "{{ 7.5 % 2 }}", "{{ 1 / 3 }}", "{{ {a: 1}|join }}", "{{ [[1]]|join }}", "{{ items[0.5] ?? 'x' }}",
            "{% for i in [1, 2] %}{% set loop = {index0: 'a'} %}{% endfor %}", "{{ nul.x }}", "{{ s.x }}",
            "{% deprecated 'dep' %}x", "{{ 9223372036854775807 + 1 }}", "{{ 10 // 0.5 }}",
        ]:
            c.append(case(tpl, ITEMS, errors=errors))
    return c


def optimized():
    c = []
    for strict in (False, True):
        for tpl in [
            "{% for i in items %}{{ _context|keys|join(',') }};{% endfor %}",
            "{% for i in items %}{{ loop.index }}{{ _context|keys|join(',') }};{% endfor %}",
            "{% for i in items %}{{ loop.parent.name }}{% endfor %}",
            "{% for i in items %}{% for j in [1] %}{{ loop.parent.loop.index }}{% endfor %}{% endfor %}",
            "{% for i in items %}{% for j in [1] %}{{ _context|keys|join(',') }}{% endfor %};{% endfor %}",
            "{% for i in items %}{{ attribute(loop, 'index') }}{% endfor %}",
            "{% for i in items %}{% set x = loop %}{% endfor %}{{ x.index }}",
            "{% for i in items %}{{ i }}{% endfor %}{{ i }}",
            "{% for k, v in map %}{{ k }}={{ v }},{% endfor %}",
            "{% for i in items %}{% for j in items %}{{ i }}{{ j }},{% endfor %}{% endfor %}",
            "{% for i in [] %}{% else %}[{{ i }}]{% endfor %}",
            "{% for i in [] %}{% else %}[{{ _key }}]{% endfor %}",
            "{% for i in [] %}{% else %}{{ i is defined ? 'y' : 'n' }}{% endfor %}",
            "{% for i in items %}{{ map.parent ?? 'n' }}{{ _context|keys|join(',') }};{% endfor %}",
            "{% for i in items %}{{ loop.index }}{% for j in [1] %}{{ map.parent ?? '' }}{{ _context|keys|join(',') }}{% endfor %};{% endfor %}",
            "{% for i in items %}{% set k = 'index' %}{{ loop[k] }}{% endfor %}",
            "{% for i in items %}{% set loop = 'mine' %}{{ loop }}{% endfor %}[{{ loop ?? 'gone' }}]",
            "{% for i in items %}{{ map[name] ?? 'n' }}{{ _context|keys|join(',') }};{% endfor %}",
            "{% set i = 'outer' %}{% for i in items %}{% endfor %}{{ i }}",
            "{% for i in items %}{{ i }}{% else %}none{% endfor %}{{ _context|keys|join(',') }}",
            "{{ 'constant' }}{{ 'a' ~ 'b' }}",
            "{% for i in items %}{{ items|map(x => loop.index)|join }};{% endfor %}",
            "{% for i in items %}{{ (x => i)(1) }}{% endfor %}",
            "{% for i in items %}{{ undef }}{% endfor %}",
            "{% for i in items|map(x => x * 2) %}{{ loop.index }}:{{ i }},{% endfor %}",
            "{% for i in 1..3 %}{{ loop.revindex }}{{ loop.last ? '.' : ',' }}{% endfor %}",
        ]:
            c.append(case(tpl, ITEMS, strict=strict, config={"optimizations": -1}))
    return c


def escape():
    """The `escape`/`e` filters called explicitly (the runtime of `EscaperRuntime::escape()`), autoescape off."""
    c = []
    objs = ("bag", "thrower", "markup", "html_markup", "empty_markup")
    for tpl in [
        "{{ '<a href=\"x\">&amp;\\'</a>'|e }}|{{ '<a>'|escape }}|{{ '<a>'|e('html') }}",
        "{{ '<a> \\'x\\' & é/\\u{2028}'|e('js') }}", "{{ '<a> \\'x\\' & é'|e('css') }}",
        "{{ '<a> \\'x\\' & é/?='|e('url') }}", "{{ '<a> \\'x\\' & é:@[]'|e('html_attr') }}",
        "{{ '<a> \\'x\\' & é:@[]'|e('html_attr_relaxed') }}",
        "{{ 5|e }}|{{ 1.5|e }}|{{ true|e }}|{{ null|e }}|{{ false|e('js') }}|{{ 0|e('url') }}",
        "{{ items|e }}", "{{ items|e('nope') }}", "{{ ''|e('nope') }}", "{{ 'x'|e('nope') }}", "{{ 'x'|e(1) }}",
        "{{ null|e('nope') }}", "{{ '<'|e('html', '0') }}|{{ '<'|e('html', '') }}|{{ '<'|e('html', null) }}",
        "{{ \"\\xe9<\"|e }}", "{{ \"\\xe9<\"|e('html', 'ISO-8859-1')|e('url') }}", "{{ \"\\xe9\"|e('js') }}",
        "{{ \"\\xe9\"|e('css') }}", "{{ \"\\xe9\"|e('html_attr') }}", "{{ \"\\xe9\"|e('url') }}",
        "{{ 'é<'|e('js', 'ISO-8859-1') }}", "{{ '<'|e('html', 'KOI8-R') }}|{{ '<'|e('html', 'koi8-r') }}",
        "{{ '<é>'|e('html', 'UTF8') }}", "{{ '<'|e('html', 'x-unknown') }}",
        "{{ markup|e }}|{{ markup|e('html', null, true) }}|{{ markup|e('js', null, true) }}",
        "{{ html_markup|e('html', null, true) }}|{{ html_markup|e('js', null, true) }}|{{ html_markup|e('url', null, true) }}",
        "{{ html_markup|e('html_attr', null, true) }}", "{{ empty_markup|e('js', null, true) }}|{{ empty_markup|e }}",
        "{{ bag|e }}|{{ bag|e('js', null, true) }}", "{{ thrower|e }}", "{{ thrower|e('nope') }}", "{{ bag|e('nope') }}",
        "{{ '<'|e(strategy='js') }}|{{ '<'|e(charset='UTF-8') }}|{{ '<'|e('html', null, 0) }}",
        "{{ '<'|e(autoescape=true) }}|{{ markup|e(autoescape=1) }}", "{{ '<'|e('js', 'UTF-8', false, 'extra') }}",
        "{{ items|map(v => v ~ '<')|join|e }}", "{% set s %}<b>{% endset %}{{ s|e }}|{{ s|e('html', null, true) }}",
        "{{ ('<' ~ name)|e('url') }}",
    ]:
        c.append(case(tpl, ITEMS, objects=objs))
    return c


# ---- template composition (stage 2) -------------------------------------------------------------------------------

def multi(main, templates, data=None, **kw):
    """A case rendering `main` of several templates; only `main` is loaded before rendering (`load_all` false), so
    the other templates load when the render needs them."""
    kw.setdefault("load_all", False)
    return case(None, data, templates=templates, main=main, **kw)


def variants(c, make, strict_modes=(False, True), optimizations=(0,)):
    """Appends `make(strict, opt)` for each mode."""
    for opt in optimizations:
        for strict in strict_modes:
            c.append(make(strict, opt))


CTX = {"name": "bob", "items": [1, 2], "layout": "l1", "layouts": ["missing", "l1"], "nope": ["n1", "n2"],
       "one": ["missing"], "five": 5, "nul": None, "f": False, "t": True, "fl": 1.5, "map": {"a": 1, "b": 2},
       "num": {"5": "five", "x": "ex"}}


def inheritance():
    """`extends` (constant and dynamic parents), blocks, `parent()`, `block()` and their errors."""
    c = []
    base = "<html>{% block title %}Base title{% endblock %}|{% block content %}base content{% endblock %}</html>"
    child = ("{% extends 'base.twig' %}\n{% block title %}Child - {{ parent() }}{% endblock %}\n"
             "{% block content %}{{ block('title') }}/{{ block('title', 'base.twig') }}/"
             "{{ block('sidebar') is defined ? 'y' : 'n' }}{% endblock %}")
    variants(c, lambda s, o: multi("child.twig", {"child.twig": child, "base.twig": base}, CTX, strict=s,
                                   config={"optimizations": o}), optimizations=(0, -1))
    # three levels, parent() chains, own blocks at every level
    g = "G[{% block a %}ga{% endblock %}|{% block b %}gb{% endblock %}|{% block c %}gc{% endblock %}]"
    p = "{% extends 'g' %}{% block a %}pa+{{ parent() }}{% endblock %}{% block c %}pc{% endblock %}"
    ch = "{% extends 'p' %}{% block a %}ca+{{ parent() }}{% endblock %}{% block b %}cb+{{ parent() }}{% endblock %}"
    variants(c, lambda s, o: multi("c", {"c": ch, "p": p, "g": g}, CTX, strict=s, config={"optimizations": o}),
             optimizations=(0, -1))
    for main, templates in [
        # the child body runs before the parent: top-level set, with and do are visible to the parent and blocks
        ("c", {"c": "{% extends 'p' %}{% set x = 'X' %}{% block b %}[{{ x }}]{% endblock %}",
               "p": "{{ x }}{% block b %}{% endblock %}{{ x }}"}),
        # a block gets a copy of the context: sets inside never leak out
        ("index.twig", {"index.twig": "{% set q = 1 %}{% block b %}{% set q = 2 %}{{ q }}{% endblock %}{{ q }}"}),
        ("index.twig", {"index.twig": "{% for i in items %}{% block b %}<{{ i }}>{% endblock %}{% endfor %}"}),
        ("index.twig", {"index.twig": "{% block outer %}[{% block inner %}in{% endblock %}]{% endblock %}|{{ block('inner') }}"}),
        ("c", {"c": "{% extends 'p' %}{% block inner %}IN{% endblock %}",
               "p": "{% block outer %}[{% block inner %}in{% endblock %}]{% endblock %}"}),
        ("c", {"c": "{% extends 'p' %}{% block outer %}<{{ parent() }}>{% endblock %}{% block inner %}IN{% endblock %}",
               "p": "{% block outer %}[{% block inner %}in{% endblock %}]{% endblock %}"}),
        # dynamic parents
        ("d", {"d": "{% extends layout %}{% block a %}A{% endblock %}", "l1": "[{% block a %}{% endblock %}]"}),
        ("d", {"d": "{% extends layouts %}{% block a %}A{% endblock %}", "l1": "[{% block a %}{% endblock %}]"}),
        ("d", {"d": "{% extends ['missing', 'l1'] %}{% block a %}A{% endblock %}", "l1": "[{% block a %}{% endblock %}]"}),
        ("d", {"d": "{% extends t ? 'l1' : 'l2' %}{% block a %}A{% endblock %}", "l1": "1{% block a %}{% endblock %}",
               "l2": "2{% block a %}{% endblock %}"}),
        ("d", {"d": "{% extends nope %}{% block a %}A{% endblock %}"}),
        ("d", {"d": "{% extends one %}{% block a %}A{% endblock %}"}),
        ("d", {"d": "{% extends [] %}{% block a %}A{% endblock %}"}),
        ("d", {"d": "{% extends five %}{% block a %}A{% endblock %}", "5": "five{% block a %}{% endblock %}"}),
        ("d", {"d": "{% extends nul %}{% block a %}A{% endblock %}"}),
        ("d", {"d": "{% extends f %}{% block a %}A{% endblock %}"}),
        ("d", {"d": "{% extends 'l' ~ 1 %}{% block a %}{{ parent() }}A{% endblock %}", "l1": "[{% block a %}a{% endblock %}]"}),
        ("d", {"d": "{% extends undefined_layout %}{% block a %}A{% endblock %}"}),
        ("d", {"d": "{% extends\n  layout\n%}\n{% block a %}A{% endblock %}", "l1": "{{ nonexistent.x.y }}[{% block a %}{% endblock %}]"}),
        # constant parents
        ("index.twig", {"index.twig": "{% extends false %}"}),
        ("index.twig", {"index.twig": "{% extends null %}"}),
        ("index.twig", {"index.twig": "{% extends 5 %}", "5": "five"}),
        ("index.twig", {"index.twig": "{% extends 'nope' %}"}),
        ("index.twig", {"index.twig": "\n\n{% extends 'nope' %}"}),
        ("index.twig", {"index.twig": "{% extends 'p' %}", "p": "parent {{ name }}"}),
        ("index.twig", {"index.twig": "{% extends 'p' %}  {# c #}\n  ", "p": "parent"}),
        ("index.twig", {"index.twig": "{% extends 'p' %}{% block x %}X{% endblock %}", "p": "no blocks"}),
        # block() and block(name, template)
        ("index.twig", {"index.twig": "{% block a %}A{% endblock %}{{ block('a') }}{{ block('a') }}"}),
        ("index.twig", {"index.twig": "{{ block('nope') }}"}),
        ("index.twig", {"index.twig": "x\n{{ block('a', 'b') }}{{ block('nope', 'b') }}", "b": "{% block a %}B{% endblock %}"}),
        ("index.twig", {"index.twig": "{{ block('a', 'missing') }}"}),
        ("index.twig", {"index.twig": "{{ block('a', layouts) }}", "l1": "{% block a %}L1{% endblock %}"}),
        ("index.twig", {"index.twig": "{{ block('a', ['nope', 'nope2']) }}"}),
        ("index.twig", {"index.twig": "{% block a %}A{% endblock %}{% set n = 'a' %}{{ block(n) }}{{ block(n ~ '') is defined ? 'y' : 'n' }}"}),
        ("index.twig", {"index.twig": "{% block a %}A{% endblock %}{{ block(5) }}"}),
        ("index.twig", {"index.twig": "{% block a %}A{% endblock %}{{ block(null) }}"}),
        ("index.twig", {"index.twig": "{% block a %}A{% endblock %}{{ block([1]) }}"}),
        ("index.twig", {"index.twig": "{% block a %}A{% endblock %}{{ block(1.5) }}"}),
        ("index.twig", {"index.twig": "{% block a %}A{% endblock %}{{ block(1.5) is defined ? 'y' : 'n' }}"}),
        ("index.twig", {"index.twig": "{% block a %}A{% endblock %}{{ [1]|map(x => block('a'))|join }}"}),
        ("index.twig", {"index.twig": "{% block a %}A{% endblock %}{{ [1]|map(x => block('a', 'b'))|join }}", "b": "{% block a %}B{% endblock %}"}),
        ("c", {"c": "{% extends 'b' %}{% block a %}A{{ [1]|map(x => parent())|join }}{% endblock %}", "b": "{% block a %}B{% endblock %}"}),
        ("index.twig", {"index.twig": "{% set b = block('x') %}{% block x %}<x>{% endblock %}|{{ b }}|{{ b|length }}"}),
        # block() is defined
        ("c", {"c": "{% extends 'p' %}{% block a %}{{ block('a') is defined ? 1 : 0 }}{{ block('b') is defined ? 1 : 0 }}{{ block('c') is defined ? 1 : 0 }}{{ block('z') is defined ? 1 : 0 }}{% endblock %}",
               "p": "{% block a %}{% endblock %}{% block b %}{% endblock %}{% if false %}{% block c %}{% endblock %}{% endif %}"}),
        ("index.twig", {"index.twig": "{{ block('a', 'b') is defined ? 1 : 0 }}{{ block('z', 'b') is defined ? 1 : 0 }}", "b": "{% block a %}{% endblock %}"}),
        # parent() errors
        ("c", {"c": "{% extends 'p' %}{% block b %}[{{ parent() }}]{% endblock %}", "p": "{% block a %}{% endblock %}"}),
        ("c", {"c": "{% extends 'p' %}\n{% block a %}\n[{{ parent() }}]{% endblock %}", "p": "{{ block('a') }}"}),
        ("c", {"c": "{% extends 'p' %}{% block a %}{% for i in items %}{{ parent() }}{{ i }}{% endfor %}{% endblock %}",
               "p": "{% block a %}<{{ i ?? 'none' }}>{% endblock %}"}),
        # _self is the template owning the running code
        ("c", {"c": "{% extends 'p' %}{% block a %}{{ _self }}/{{ parent() }}{% endblock %}",
               "p": "{{ _self }}:{% block a %}{{ _self }}{% endblock %}:{% block b %}{{ _self }}{% endblock %}"}),
        # the block table: a parent rendering a block the child defines but the parent does not
        ("c", {"c": "{% extends 'p' %}{% block extra %}E{% endblock %}", "p": "{{ block('extra') }}"}),
        ("c", {"c": "{% extends 'p' %}{% block extra %}E{% endblock %}", "p": "{{ block('extra') is defined ? 'y' : 'n' }}"}),
        # recursion through block()
        ("index.twig", {"index.twig": "{% set n = 3 %}{% block r %}{{ n }}{% if n > 0 %}{% set n = n - 1 %}{{ block('r') }}{% endif %}{% endblock %}"}),
        ("index.twig", {"index.twig": "{% block a %}A{{ name }}{% endblock %}{{ block('a', _self) }}"}),
        ("index.twig", {"index.twig": "x\n{{ block(\n  'nope',\n  'b'\n) }}", "b": "{% block a %}{% endblock %}"}),
        ("index.twig", {"index.twig": "x\n{{ block(\n  undefined_name\n) }}"}),
        ("c", {"c": "{% extends 'p' %}{% block a %}{{ forms.hi() }}{% endblock %}", "p": "{% import _self as forms %}{% macro hi() %}hi{% endmacro %}{% block a %}{{ forms.hi() }}{% endblock %}"}),
        ("c", {"c": "{% extends 'p' %}{% import _self as forms %}{% macro hi() %}chi{% endmacro %}{% block a %}{{ forms.hi() }}{% endblock %}", "p": "{% block a %}{% endblock %}|{% block b %}{{ forms.hi() }}{% endblock %}"}),
        # errors attributed to the owner of the block
        ("c", {"c": "{% extends 'p' %}\n{% block a %}\n{{ 1 // 0 }}{% endblock %}", "p": "x\n{% block a %}{% endblock %}"}),
        ("c", {"c": "{% extends 'p' %}\n{% block a %}\n{{ parent() }}{% endblock %}", "p": "x\n{% block a %}\n\n{{ 1 // 0 }}{% endblock %}"}),
        ("c", {"c": "{% extends 'p' %}\n{% block a %}A{% endblock %}", "p": "x\n{% block a %}{% endblock %}\n{{ undefined_var }}"}),
    ]:
        variants(c, lambda s, o, main=main, templates=templates: multi(main, templates, CTX, strict=s))
    # output mode (optimizer) for block() and parent()
    for main, templates in [
        ("c", {"c": "{% extends 'p' %}{% block a %}<{{ parent() }}>{% endblock %}", "p": "{% block a %}P{% endblock %}|{{ block('a') }}"}),
        ("index.twig", {"index.twig": "{{ block('nope') }}"}),
        ("index.twig", {"index.twig": "{% block a %}A{% endblock %}{{ block('a') }}{{ block('a', 'b') }}", "b": "{% block a %}B{% endblock %}"}),
        ("c", {"c": "{% extends 'p' %}{% block b %}[{{ parent() }}]{% endblock %}", "p": "{% block a %}{% endblock %}"}),
    ]:
        variants(c, lambda s, o, main=main, templates=templates: multi(main, templates, CTX, strict=s,
                                                                       config={"optimizations": -1}))
    # template wrappers as parents and block templates
    c.append(multi("c", {"c": "{% extends tpl %}{% block a %}A{% endblock %}", "p": "<{% block a %}{% endblock %}>"},
                   CTX, wrappers={"tpl": "p"}))
    c.append(multi("c", {"c": "{% extends tpl %}{% block a %}A{% endblock %}"}, CTX,
                   foreign_wrappers={"tpl": ["p", "<{% block a %}{% endblock %}>"]}))
    c.append(multi("index.twig", {"index.twig": "{{ block('a', tpl) }}", "b": "{% block a %}B{% endblock %}"}, CTX,
                   wrappers={"tpl": "b"}))
    c.append(multi("index.twig", {"index.twig": "{{ block('a', [tpl]) }}"}, CTX,
                   foreign_wrappers={"tpl": ["b", "{% block a %}B{% endblock %}"]}))
    return c


def traits():
    """`use`: trait blocks, aliases, `parent()` into traits, `isTraitable()` and the trait errors."""
    c = []
    for main, templates in [
        ("use.twig", {"use.twig": "{% use 'blocks.twig' with b1 as alias %}{% use 'blocks2.twig' %}{% block b2 %}own{{ parent() }}{% endblock %}{{ block('alias') }}{{ block('b3') }}",
                      "blocks.twig": "{% block b1 %}B1{% endblock %}{% block b2 %}B2{% endblock %}",
                      "blocks2.twig": "{% block b3 %}B3{% endblock %}"}),
        ("u", {"u": "{% use 't' %}{{ block('a') }}|{{ block('b') }}", "t": "{% block a %}TA{% endblock %}{% block b %}TB{% endblock %}"}),
        # an alias equal to the name removes the block
        ("u", {"u": "{% use 't' with a as a %}{{ block('a') is defined ? 'y' : 'n' }}", "t": "{% block a %}TA{% endblock %}"}),
        ("u", {"u": "{% use 't' with a as b %}{{ block('b') }}{{ block('a') is defined ? 'y' : 'n' }}", "t": "{% block a %}TA{% endblock %}"}),
        ("u", {"u": "{% use 't' with a as b, b as a %}{{ block('a') }}|{{ block('b') }}", "t": "{% block a %}TA{% endblock %}{% block b %}TB{% endblock %}"}),
        # parent() of an aliased block reaches the original trait block
        ("u", {"u": "{% use 't' with a as b %}{% block b %}own+{{ parent() }}{% endblock %}", "t": "{% block a %}TA{% endblock %}"}),
        ("u", {"u": "{% use 't' with a as b %}{% block a %}own+{{ parent() }}{% endblock %}", "t": "{% block a %}TA{% endblock %}"}),
        # traitable templates: text and block references only
        ("u", {"u": "{% use 't' %}{{ block('a') }}", "t": "text {% block a %}TA{% endblock %} text"}),
        ("u", {"u": "{% use 't' %}{{ block('a') }}", "t": "{{ x }}{% block a %}TA{% endblock %}"}),
        ("u", {"u": "{% use 't' %}{{ block('a') }}", "t": "{% macro m() %}{% endmacro %}{% block a %}TA{% endblock %}"}),
        ("u", {"u": "{% use 't' %}{{ block('a') }}", "t": "{% extends 'x' %}{% block a %}TA{% endblock %}", "x": "x"}),
        ("u", {"u": "{% use 't' %}{{ block('a') }}", "t": "{% set x = 1 %}{% block a %}TA{% endblock %}"}),
        ("u", {"u": "{% use 't' %}{{ block('a') }}", "t": "{# c #}{% block a %}TA{% endblock %}"}),
        # errors
        ("u", {"u": "\n{% use 't' with nope as x %}", "t": "{% block a %}{% endblock %}"}),
        ("u", {"u": "\n{% use 'missing' %}"}),
        ("u", {"u": "{% use 't' %}\n{% use 'missing' %}", "t": "{% block a %}{% endblock %}"}),
        ("u", {"u": "{% use 't' %}{{ block('a') }}", "t": "{% use 'missing' %}{% block a %}{% endblock %}"}),
        ("u", {"u": "{% use 't' %}{{ block('a') }}", "t": "{% block a %}{{ 1 // 0 }}{% endblock %}"}),
        ("u", {"u": "{% use 't' %}{% block b %}{{ parent() }}{% endblock %}", "t": "{% block a %}{% endblock %}"}),
        # nested traits and the order of the block tables
        ("u", {"u": "{% use 't1' %}{% use 't2' %}{{ block('a') }}{{ block('b') }}",
               "t1": "{% block a %}1a{% endblock %}{% block b %}1b{% endblock %}", "t2": "{% block a %}2a{% endblock %}"}),
        ("u", {"u": "{% use 't1' %}{{ block('a') }}{{ block('b') }}", "t1": "{% use 't2' %}{% block a %}1a{% endblock %}",
               "t2": "{% block a %}2a{% endblock %}{% block b %}2b{% endblock %}"}),
        ("u", {"u": "{% use 't1' %}{% block a %}u+{{ parent() }}{% endblock %}", "t1": "{% use 't2' %}{% block a %}1+{{ parent() }}{% endblock %}",
               "t2": "{% block a %}2{% endblock %}"}),
        ("u", {"u": "{% use 't1' with b as c %}{% block c %}u+{{ parent() }}{% endblock %}", "t1": "{% use 't2' with x as b %}",
               "t2": "{% block x %}2x{% endblock %}"}),
        # traits in a child template: own blocks, then traits, then the parent
        ("c", {"c": "{% extends 'p' %}{% use 't' %}{% block a %}c+{{ parent() }}{% endblock %}",
               "p": "{% block a %}pa{% endblock %}|{% block b %}pb{% endblock %}", "t": "{% block b %}tb{% endblock %}{% block a %}ta{% endblock %}"}),
        ("c", {"c": "{% extends 'p' %}{% use 't' %}", "p": "{% block a %}pa{% endblock %}", "t": "{% block a %}ta+{{ parent() }}{% endblock %}"}),
        # _self in a trait block is the trait template
        ("u", {"u": "{% use 't' %}{{ _self }}:{{ block('a') }}", "t": "{% block a %}{{ _self }}{% endblock %}"}),
        ("u", {"u": "{% use 't' %}{{ block('a') }}", "t": "{% block a %}{{ block('b') }}{% endblock %}{% block b %}tb{% endblock %}"}),
        ("u", {"u": "{% use 't' %}{% block b %}ub{% endblock %}{{ block('a') }}", "t": "{% block a %}{{ block('b') }}{% endblock %}{% block b %}tb{% endblock %}"}),
    ]:
        variants(c, lambda s, o, main=main, templates=templates: multi(main, templates, CTX, strict=s))
    # the trait of a template loaded alone
    c.append(multi("u", {"u": "{% use 't' %}{{ block('a') }}", "t": "{{ x }}{% block a %}{% endblock %}"}, CTX, load_all=False))
    c.append(multi("u", {"u": "{% use 't' %}{% block a %}{{ parent() }}{% endblock %}", "t": "{% block a %}T{% endblock %}"},
                   CTX, config={"optimizations": -1}))
    return c


def embed():
    """`embed`: embedded templates with their indexes, `with`/`only`/`ignore missing`, nesting and errors."""
    c = []
    for main, templates in [
        ("embed.twig", {"embed.twig": "{% embed 'base.twig' with {v: 1} %}{% block content %}E{{ v }}{{ parent() }}{% endblock %}{% endembed %}",
                        "base.twig": "<{% block content %}base{% endblock %}>"}),
        ("e", {"e": "{% embed 'b' %}{% endembed %}|{% embed 'b' %}{% block x %}X{% endblock %}{% endembed %}",
               "b": "[{% block x %}bx{% endblock %}]"}),
        ("e", {"e": "{% embed 'b' only %}{% block x %}{{ name ?? 'none' }}{% endblock %}{% endembed %}", "b": "[{% block x %}{% endblock %}]"}),
        ("e", {"e": "{% embed 'b' with {z: 1} only %}{% block x %}{{ _context|keys|join(',') }}{% endblock %}{% endembed %}", "b": "{% block x %}{% endblock %}"}),
        ("e", {"e": "{% embed 'b' with {z: 1} %}{% block x %}{{ _context|keys|join(',') }}{% endblock %}{% endembed %}", "b": "{% block x %}{% endblock %}"}),
        ("e", {"e": "{% embed 'b' with {5: 'five'} %}{% block x %}{{ _context|keys|join(',') }}{% endblock %}{% endembed %}", "b": "{% block x %}{% endblock %}"}),
        ("e", {"e": "{% embed 'missing' ignore missing %}{% block x %}X{% endblock %}{% endembed %}ok"}),
        ("e", {"e": "{% embed 'b' ignore missing %}{% block x %}X{% endblock %}{% endembed %}ok", "b": "[{% block x %}{% endblock %}]"}),
        ("e", {"e": "\n{% embed 'missing' %}{% block x %}X{% endblock %}{% endembed %}"}),
        ("e", {"e": "{% embed layout ignore missing %}{% block a %}A{% endblock %}{% endembed %}|", "l1": "[{% block a %}{% endblock %}]"}),
        ("e", {"e": "{% embed nope ignore missing %}{% block a %}A{% endblock %}{% endembed %}|"}),
        ("e", {"e": "{% embed layouts %}{% block a %}A{% endblock %}{% endembed %}", "l1": "[{% block a %}{% endblock %}]"}),
        ("e", {"e": "{% embed 'b' with 5 %}{% endembed %}", "b": "b"}),
        ("e", {"e": "{% embed 'b' with 5 only %}{% endembed %}", "b": "b"}),
        # nested embeds (indexes: the inner one is numbered first) and two embeds
        ("e", {"e": "{% embed 'b' %}{% block x %}({% embed 'c' %}{% block y %}Y{{ parent() }}{% endblock %}{% endembed %}){% endblock %}{% endembed %}",
               "b": "[{% block x %}{% endblock %}]", "c": "<{% block y %}cy{% endblock %}>"}),
        ("e", {"e": "{% embed 'b' %}{% block x %}1{% endblock %}{% endembed %}{% embed 'b' %}{% block x %}2{% endblock %}{% endembed %}",
               "b": "[{% block x %}{% endblock %}]"}),
        # _self, imports and context
        ("e", {"e": "{% embed 'b' %}{% block x %}{{ _self }}{% endblock %}{% endembed %}", "b": "{{ _self }}{% block x %}{% endblock %}"}),
        ("e", {"e": "{% import _self as m %}{% macro hi() %}hi{% endmacro %}{% embed 'b' %}{% block x %}{{ m.hi() }}{% endblock %}{% endembed %}", "b": "{% block x %}{% endblock %}"}),
        ("e", {"e": "{% macro hi() %}hi{% endmacro %}{% embed 'b' %}{% import _self as m %}{% block x %}{{ m.hi() }}{% endblock %}{% endembed %}", "b": "{% block x %}{% endblock %}"}),
        ("e", {"e": "{% embed 'b' %}{% macro hi() %}ehi{% endmacro %}{% block x %}{{ _self.hi() }}{% endblock %}{% endembed %}", "b": "{% block x %}{% endblock %}"}),
        ("e", {"e": "{% for i in items %}{% embed 'b' %}{% block x %}{{ i }}{{ loop.index }}{% endblock %}{% endembed %}{% endfor %}", "b": "{% block x %}{% endblock %}"}),
        ("e", {"e": "{% embed 'b' %}{% block x %}{{ block('y') }}{% endblock %}{% block y %}Y{% endblock %}{% endembed %}", "b": "{% block x %}{% endblock %}"}),
        # an embedded template extending a chain
        ("e", {"e": "{% embed 'p' %}{% block a %}E+{{ parent() }}{% endblock %}{% endembed %}", "p": "{% extends 'g' %}{% block a %}P+{{ parent() }}{% endblock %}",
               "g": "<{% block a %}G{% endblock %}>"}),
        # embed inside a block of a child and inside a macro
        ("c", {"c": "{% extends 'p' %}{% block a %}{% embed 'b' %}{% block x %}cx{% endblock %}{% endembed %}{% endblock %}", "p": "[{% block a %}{% endblock %}]",
               "b": "<{% block x %}{% endblock %}>"}),
        ("e", {"e": "{% macro m() %}{% embed 'b' %}{% block x %}mx{% endblock %}{% endembed %}{% endmacro %}{{ _self.m() }}", "b": "<{% block x %}{% endblock %}>"}),
        # errors in embedded blocks
        ("e", {"e": "{% embed 'b' %}\n{% block x %}\n{{ 1 // 0 }}{% endblock %}{% endembed %}", "b": "{% block x %}{% endblock %}"}),
        ("e", {"e": "{% embed 'b' %}{% block x %}\n{{ undefined_var }}{% endblock %}{% endembed %}", "b": "{% block x %}{% endblock %}"}),
        ("e", {"e": "x\n{% embed 'b' %}{% endembed %}", "b": "{% block x %}\n{{ undefined_var }}{% endblock %}"}),
    ]:
        variants(c, lambda s, o, main=main, templates=templates: multi(main, templates, CTX, strict=s))
    c.append(multi("e", {"e": "{% embed 'b' %}{% block x %}{{ parent() }}!{% endblock %}{% endembed %}", "b": "[{% block x %}bx{% endblock %}]"},
                   CTX, config={"optimizations": -1}))
    return c


def include():
    """The `include` tag, the `include()`, `include_only()` and `source()` functions and their errors."""
    c = []
    part = "[{{ x|default('-') }}]"
    for main, templates in [
        ("include.twig", {"include.twig": "{% include 'part.twig' %}{% include 'part.twig' with {x: 1} %}{% include 'part.twig' with {x: 2} only %}{% include 'missing.twig' ignore missing %}{% include ['missing.twig', 'part.twig'] only %}{{ include('part.twig', {x: 3}) }}",
                          "part.twig": part}),
        ("i", {"i": "{% include 'k' %}|{% include 'k' with {z: 1, a: 9} %}|{% include 'k' with {z: 1} only %}|{% include 'k' only %}",
               "k": "{{ _context|keys|join(',') }}"}),
        ("i", {"i": "{% include 'k' with {5: 'five', 'b': 2} %}", "k": "{{ _context|keys|join(',') }}"}),
        ("i", {"i": "{% set x = 'set' %}{% include 'k' %}{{ y ?? 'no-y' }}", "k": "{{ x }}{% set y = 'leak' %}"}),
        ("i", {"i": "{% for i in items %}{% include 'k' %}{% endfor %}", "k": "<{{ i }}:{{ loop.index }}>"}),
        ("i", {"i": "x\n\n{% include 'nope' %}"}),
        ("i", {"i": "{% include 'nope' ignore missing %}|{% include ['n1', 'n2'] ignore missing %}|"}),
        ("i", {"i": "{% include ['n1', 'n2'] %}"}),
        ("i", {"i": "{% include ['nope'] %}"}),
        ("i", {"i": "{% include [] %}"}),
        ("i", {"i": "{% include nope %}"}),
        ("i", {"i": "{% include layouts %}", "l1": "L1"}),
        ("i", {"i": "{% include five %}", "5": "five"}),
        ("i", {"i": "{% include nul %}"}),
        ("i", {"i": "{% include fl %}", "1.5": "onefive"}),
        ("i", {"i": "{% include 'k' with five %}", "k": "k"}),
        ("i", {"i": "{% include 'k' with five only %}", "k": "k"}),
        ("i", {"i": "{% include 'k' with map only %}", "k": "{{ _context|keys|join(',') }}"}),
        ("i", {"i": "{% include 'missing' ignore missing with five %}ok"}),
        ("i", {"i": "{% include source('nope') ignore missing %}ok"}),
        ("i", {"i": "{% include 'k' ignore missing %}", "k": "{{ 1 // 0 }}"}),
        ("i", {"i": "{% include 'k' ignore missing %}", "k": "{% include 'deeper' %}"}),
        ("i", {"i": "{% include 'k' ignore missing %}", "k": "{% use 'gone' %}"}),
        ("i", {"i": "{% include 'k' ignore missing %}", "k": "{{ x"}),
        ("i", {"i": "{% include 'k' %}", "k": "\n{{ x"}),
        ("i", {"i": "x\n{% include 'k' %}", "k": "a\n{{ 1 // 0 }}"}),
        ("i", {"i": "x\n{% include 'k' %}", "k": "a\n{{ undefined_var }}"}),
        ("i", {"i": "{% include 'k' %}", "k": "{% extends 'p' %}{% block a %}K{% endblock %}", "p": "<{% block a %}{% endblock %}>"}),
        ("i", {"i": "{% set n = 3 %}{% include 'r' %}", "r": "{{ n }}{% if n > 0 %}{% include 'r' with {n: n - 1} %}{% endif %}"}),
        ("i", {"i": "x\n{% include\n  'nope'\n%}"}),
        ("i", {"i": "x\n{% include\n  undefined_name\n%}"}),
        ("i", {"i": "x\n{% include 'k' with {\n  a: undefined_name\n} %}", "k": "k"}),
        ("i", {"i": "{% for n in ['k', 'nope', 'k'] %}{% include n ignore missing %}{{ loop.index }}{% endfor %}", "k": "<{{ n }}>"}),
        ("i", {"i": "{% for n in ['k', 'nope'] %}{% include [n, 'nope2'] ignore missing with {z: n} %}{% else %}none{% endfor %}", "k": "<{{ z }}>"}),
        ("i", {"i": "{% set c %}{% include 'k' %}{% include 'nope' ignore missing %}{% endset %}[{{ c }}]", "k": "K"}),
        ("i", {"i": "{{ [1, 2]|map(v => include('k', {v: v}))|join(',') }}", "k": "<{{ v }}>"}),
        # include()
        ("i", {"i": "{{ include('k') }}|{{ include('k', {b: 2}) }}|{{ include('k', {b: 2}, false) }}|{{ include('k', with_context = false) }}",
               "k": "{{ _context|keys|join(',') }}"}),
        ("i", {"i": "{{ include('nope') }}"}),
        ("i", {"i": "{{ include('nope', ignore_missing = true) }}|{{ include(['nope', 'k']) }}", "k": "K"}),
        ("i", {"i": "{{ include(['nope', 'nope2']) }}"}),
        ("i", {"i": "{{ include('k', 'string') }}", "k": "K"}),
        ("i", {"i": "{{ include('k', 'string', false) }}", "k": "K"}),
        ("i", {"i": "{{ include(nul) }}"}),
        ("i", {"i": "{% set i = include('k') %}{{ i }}|{{ include('empty') is same as('') ? 'empty' : 'not' }}|{{ include('k') is same as('<b>') ? 's' : 'm' }}",
               "k": "<b>", "empty": ""}),
        ("i", {"i": "{{ include('k', sandboxed = false) }}{{ include('k', {}, true, false, true) }}", "k": "K"}),
        ("i", {"i": "x\n{{ include('k') }}", "k": "a\n{{ 1 // 0 }}"}),
        ("i", {"i": "{{ include('k', {5: 'five'}) }}", "k": "{{ _context|keys|join(',') }}"}),
        ("i", {"i": "{{ include('k', map, ignore_missing = 'yes') }}", "k": "{{ a }}{{ b }}"}),
        # include_only()
        ("i", {"i": "{{ include_only('k', {b: 2}) }}|{{ include_only('nope', ignore_missing = true) }}|", "k": "{{ _context|keys|join(',') }}"}),
        ("i", {"i": "{{ include_only('k', 'x') }}", "k": "K"}),
        ("i", {"i": "{{ include_only('k', {}, 'yes') }}", "k": "K"}),
        # source()
        ("i", {"i": "{{ source('k') }}|{{ source('nope', ignore_missing = true) }}|{{ source('i')|length }}", "k": "Hello {{ name }} <b>"}),
        ("i", {"i": "x\n{{ source('nope') }}"}),
        ("i", {"i": "{{ source(nul) }}"}),
        ("i", {"i": "{{ source(five) }}", "5": "five src"}),
    ]:
        variants(c, lambda s, o, main=main, templates=templates: multi(main, templates, CTX, strict=s))
    c.append(multi("i", {"i": "{% include 'k' %}|{% include 'k' only %}", "k": "{{ g }}"}, CTX, globals={"g": "global"}))
    c.append(multi("i", {"i": "{{ include('k', {g: 'mine'}) }}|{{ include_only('k') }}", "k": "{{ g }}"}, CTX, globals={"g": "global"}))
    c.append(multi("i", {"i": "{% include tpl %}{{ include(tpl) }}{{ include([tpl]) }}", "k": "K{{ name }}"}, CTX, wrappers={"tpl": "k"}))
    c.append(multi("i", {"i": "{% include tpl %}"}, CTX, foreign_wrappers={"tpl": ["k", "K"]}))
    c.append(multi("i", {"i": "{{ include(tpl) }}"}, CTX, foreign_wrappers={"tpl": ["k", "K"]}))
    c.append(multi("i", {"i": "{{ include(template_from_string('Hello {{ name }}')) }}|{{ include(template_from_string('X', 'named')) }}"},
                   CTX, extensions=["string_loader"]))
    c.append(multi("i", {"i": "{{ include(template_from_string('{{ 1 // 0 }}')) }}"}, CTX, extensions=["string_loader"]))
    c.append(multi("i", {"i": "{{ include(template_from_string('{{ x')) }}"}, CTX, extensions=["string_loader"]))
    c.append(multi("i", {"i": "{% set t = template_from_string('<{{ name }}>') %}{{ t.render({name: 'r'}) }}|{{ t.templateName|slice(0, 19) }}"},
                   CTX, extensions=["string_loader"]))
    return c


def macros():
    """Macros: definitions, arguments (defaults, varargs, variadics, named), imports and scopes, errors and
    deprecations."""
    c = []
    m3 = ("{% macro input(name, value = '', type = 'text', size = 20) %}\n"
          "<input type=\"{{ type }}\" name=\"{{ name }}\" value=\"{{ value|e }}\" size=\"{{ size }}\"/>{{ varargs|length }}\n"
          "{% endmacro %}\n"
          "{% macro list(title, ...items) %}{{ title }}:{% for i in items %}[{{ i }}]{% endfor %}{% endmacro %}\n"
          "{% import _self as forms %}\n{% from 'macros.twig' import list as l %}\n"
          "{{ forms.input('user') }}\n{{ forms.input('pwd', type='password', extra=1) }}\n{{ l('T', 1, 2, 3) }}\n"
          "{{ forms.list is defined ? 'def' : 'undef' }}\n{{ _self.input('x') }}")
    variants(c, lambda s, o: multi("macros.twig", {"macros.twig": m3}, CTX, strict=s, config={"optimizations": o}),
             optimizations=(0, -1))
    sig = "{% macro m(a, b = 'B') %}a={{ a ?? 'null' }},b={{ b }},v={{ varargs|json_encode }}{% endmacro %}"
    for call in ["m(1, 2, 3, foo=4)", "m(b=2)", "m(1, c=3)", "m(A=1)", "m()", "m(1)", "m(1, 2, 3)", "m(a=1, b=2)",
                 "m(b=2, a=1)", "m(1, a=2)", "m(a=1, 2)", "m(b=1, 2)", "m(1, b=2, 3)", "m(...[1, 2])", "m(...{b: 9})",
                 "m(...{b: 9, a: 8})", "m(1, ...[2, 3])", "m(1, ...{x: 2})"]:
        variants(c, lambda s, o, call=call: case(sig + "{{ _self.%s }}" % call, CTX, strict=s))
    for main, templates in [
        ("index.twig", {"index.twig": "{% macro n(a, ...rest) %}{{ a }}|{{ rest|json_encode }}{% endmacro %}{{ _self.n(1, 2, x=3) }}{{ _self.n(1, rest=5) }}{{ _self.n() }}"}),
        ("index.twig", {"index.twig": "{% macro n(...rest) %}{{ rest|json_encode }}{{ varargs ?? 'novarargs' }}{% endmacro %}{{ _self.n(1, x=2) }}"}),
        ("index.twig", {"index.twig": "{% macro m(a) %}{{ a }}|{{ varargs|json_encode }}{% endmacro %}{{ _self.m(1, varargs=5) }}{{ _self.m(varargs=5, a=2) }}"}),
        ("index.twig", {"index.twig": "{% macro m(context, macros, blocks, this) %}{{ context }}{{ macros }}{{ blocks }}{{ this }}|{{ _context|keys|join(',') }}{% endmacro %}{{ _self.m(1, 2, 3, 4) }}{{ _self.m(this=8, context=5) }}"}),
        ("index.twig", {"index.twig": "{% macro m(x, y) %}{{ _context|keys|join(',') }}{% endmacro %}{{ _self.m(1, 2) }}"}),
        ("index.twig", {"index.twig": "{% macro e() %}{% endmacro %}{{ _self.e() is same as('') ? 'empty' : 'markup' }}|{% macro f() %}x{% endmacro %}{{ _self.f() is same as('x') ? 's' : 'm' }}"}),
        ("index.twig", {"index.twig": "{% macro m() %}{{ name ?? 'no-name' }}{% endmacro %}{{ _self.m() }}"}),
        ("index.twig", {"index.twig": "{% macro tree(items) %}{% for k, v in items %}<{{ k }}{% if v is iterable %}{{ _self.tree(v) }}{% endif %}>{% endfor %}{% endmacro %}{{ _self.tree({a: {b: {c: 1}}, d: 2}) }}"}),
        ("index.twig", {"index.twig": "{% macro a() %}A{{ _self.b() }}{% endmacro %}{% macro b() %}B{% endmacro %}{{ _self.a() }}"}),
        ("index.twig", {"index.twig": "{% macro a() %}{% import _self as me %}A{{ me.b() }}{% endmacro %}{% macro b() %}B{% endmacro %}{{ _self.a() }}"}),
        ("index.twig", {"index.twig": "{% macro a() %}{% set x = 1 %}{% for i in [1, 2] %}{{ i + x }}{% endfor %}{% endmacro %}{{ _self.a() }}{{ x ?? 'no-x' }}"}),
        ("index.twig", {"index.twig": "{% macro m() %}{{ block('b') }}{% endmacro %}{% block b %}B{% endblock %}{{ _self.m() }}"}),
        ("index.twig", {"index.twig": "{% set n = 'm' %}{% macro m() %}M{% endmacro %}{{ _self.(n)() }}{{ _self.(n ~ 'x')() }}"}),
        ("index.twig", {"index.twig": "{% macro hello() %}H{% endmacro %}{% from _self import hello as hi, hello %}{{ hello() }}{{ hi is defined ? 'y' : 'n' }}"}),
        ("index.twig", {"index.twig": "{% macro hello() %}H{% endmacro %}{% from _self import hello as hi %}{{ hi() }}"}),
        ("index.twig", {"index.twig": "{% macro w(v) %}<{{ v }}>{% endmacro %}{% import _self as m %}{{ items|map(v => m.w(v))|join }}{{ items|map(v => _self.w(v))|join }}"}),
        ("index.twig", {"index.twig": "{% macro w(v) %}<{{ v }}>{% endmacro %}{% from _self import w %}{% for i in items %}{{ w(i) }}{% endfor %}"}),
        ("index.twig", {"index.twig": "{% macro w(v) %}{{ v }}{% endmacro %}\n{{ _self.w(\n  undefined_name\n) }}"}),
        # imports from other templates and scopes
        ("u", {"u": "{% import 'm' as forms %}{{ forms.hi('x') }}|{% from 'm' import hi %}{{ hi('y') }}", "m": "{% macro hi(n) %}hi {{ n }}{% endmacro %}"}),
        ("u", {"u": "{% block a %}{% import 'm' as forms %}{{ forms.hi('a') }}{% endblock %}{% block b %}{{ forms.hi('b') }}{% endblock %}", "m": "{% macro hi(n) %}hi {{ n }}{% endmacro %}"}),
        ("u", {"u": "{% import 'm' as forms %}{% block b %}{{ forms.hi('b') }}{% endblock %}", "m": "{% macro hi(n) %}hi {{ n }}{% endmacro %}"}),
        ("u", {"u": "{% extends 'p' %}{% import 'm' as forms %}{% block b %}{{ forms.hi('b') }}{% endblock %}", "p": "{% block b %}{% endblock %}", "m": "{% macro hi(n) %}hi {{ n }}{% endmacro %}"}),
        ("u", {"u": "{% macro x() %}{{ forms.hi('m') }}{% endmacro %}{% import 'm' as forms %}{{ _self.x() }}", "m": "{% macro hi(n) %}hi {{ n }}{% endmacro %}"}),
        ("u", {"u": "{% set x %}{% import 'm' as forms %}{% endset %}{{ forms.hi('c') }}", "m": "{% macro hi(n) %}hi {{ n }}{% endmacro %}"}),
        ("u", {"u": "x\n{% import 'missing' as m %}"}),
        ("u", {"u": "x\n{% from 'missing' import a %}"}),
        ("u", {"u": "{% import 'm' as forms %}\n{{ forms.nope() }}", "m": "{% macro hi() %}{% endmacro %}"}),
        ("u", {"u": "{% from 'm' import nope %}\n{{ nope() }}", "m": "{% macro hi() %}{% endmacro %}"}),
        ("u", {"u": "{% import 'm' as forms %}{{ forms.HI() }}|{{ forms.HI is defined ? 'y' : 'n' }}|{{ forms.macro_hi() }}|{{ forms.macro_hi is defined ? 'y' : 'n' }}|{{ forms.macro_zz is defined ? 'y' : 'n' }}",
               "m": "{% macro hi() %}hi{% endmacro %}"}),
        ("u", {"u": "{% import 'm' as forms %}{{ forms.hi }}", "m": "{% macro hi() %}hi{% endmacro %}"}),
        ("u", {"u": "{% import 'c' as forms %}{{ forms.parentmacro() }}|{{ forms.childmacro() }}|{{ forms.parentmacro is defined ? 'y' : 'n' }}",
               "c": "{% extends 'p' %}{% macro childmacro() %}C{% endmacro %}", "p": "{% macro parentmacro() %}P{% endmacro %}"}),
        ("c", {"c": "{% extends 'p' %}{% block a %}{{ _self.parentmacro() }}{% endblock %}", "p": "{% macro parentmacro() %}P{% endmacro %}{% block a %}{% endblock %}"}),
        # errors and their lines
        ("u", {"u": "{% import 'm' as forms %}\n\n{{ forms.boom() }}", "m": "{% macro boom() %}\n\n{{ 1 // 0 }}{% endmacro %}"}),
        ("u", {"u": "{% import 'm' as forms %}\n\n{{ forms.boom() }}", "m": "{% macro boom() %}\n\n{{ undefined_var }}{% endmacro %}"}),
        ("u", {"u": "{% import _self as forms %}\n{% macro boom() %}\n\n{{ 1 // 0 }}{% endmacro %}\n{{ forms.boom() }}"}),
        ("u", {"u": "{% macro m(a, b) %}{% endmacro %}\n{{ _self.m(a=1, 2) }}"}),
        ("u", {"u": "{% macro m(a, b) %}{% endmacro %}\n{{ _self.m(1, a=2) }}"}),
        ("u", {"u": "{% macro m(a) %}{% endmacro %}\n{{ _self.nope() }}"}),
        ("u", {"u": "{% macro m(a) %}{% endmacro %}{{ _self.m(1, 2) }}{{ _self.m() }}{{ _self.m(z=1) }}"}),
    ]:
        variants(c, lambda s, o, main=main, templates=templates: multi(main, templates, CTX, strict=s))
    c.append(multi("u", {"u": "{% import 'm' as forms %}{{ forms.hi('g') }}|{{ _context|keys|join(',') }}", "m": "{% macro hi(n) %}{{ n }}{{ _context|keys|join(',') }}{% endmacro %}"},
                   CTX, globals={"gl": "G"}))
    return c


def composition_errors():
    """Which template and line an exception raised in composed templates is reported against (Template::handleException()
    and Error::guess())."""
    c = []
    objs = ("thrower",)
    for main, templates in [
        ("index.twig", {"index.twig": "a\n{% block b %}\nx\n{{ thrower.boom() }}{% endblock %}"}),
        ("c", {"c": "{% extends 'p' %}\n{% block b %}\n\n{{ thrower.boom() }}{% endblock %}", "p": "{% block b %}{% endblock %}"}),
        ("c", {"c": "{% extends 'p' %}\n{% block b %}\n\n{{ parent() }}{% endblock %}", "p": "\n{% block b %}\n{{ thrower.boom() }}{% endblock %}"}),
        ("c", {"c": "{% extends 'p' %}", "p": "\n\n{{ thrower.boom() }}"}),
        ("c", {"c": "{% extends 'p' %}\n{% set x = thrower.boom() %}", "p": "p"}),
        ("c", {"c": "\n{% extends thrower.boom() %}", "p": "p"}),
        ("c", {"c": "{% extends layout %}\n{% block a %}{{ thrower.boom() }}{% endblock %}", "l1": "\n{% block a %}{% endblock %}"}),
        ("i", {"i": "a\n{% include 'k' %}", "k": "\n\n{{ thrower.boom() }}"}),
        ("i", {"i": "a\n{% include 'k' with {t: thrower} %}", "k": "\n\n{{ t.boom() }}"}),
        ("i", {"i": "a\n{{ include('k') }}", "k": "\n\n{{ thrower.boom() }}"}),
        ("i", {"i": "a\n{% include 'k' %}", "k": "{% include 'k2' %}", "k2": "\n\n\n{{ thrower.boom() }}"}),
        ("u", {"u": "{% import 'm' as m %}\n\n\n{{ m.boom(thrower) }}", "m": "{% macro boom(t) %}\n\n{{ t.boom() }}{% endmacro %}"}),
        ("u", {"u": "{% macro boom(t) %}\n\n{{ t.boom() }}{% endmacro %}\n\n\n{{ _self.boom(thrower) }}"}),
        ("u", {"u": "{% use 't' %}\n{{ block('a') }}", "t": "\n{% block a %}\n{{ thrower.boom() }}{% endblock %}"}),
        ("e", {"e": "{% embed 'b' %}\n{% block x %}\n{{ thrower.boom() }}{% endblock %}{% endembed %}", "b": "{% block x %}{% endblock %}"}),
        ("e", {"e": "x\n{% embed 'b' %}{% endembed %}", "b": "\n{% block x %}\n{{ thrower.boom() }}{% endblock %}"}),
        ("index.twig", {"index.twig": "{% block a %}A{% endblock %}\n{{ block('a', 'b') }}", "b": "\n{% block a %}\n{{ thrower.boom() }}{% endblock %}"}),
        ("index.twig", {"index.twig": "x\n{{ [1]|map(v => thrower.boom())|join }}"}),
        ("index.twig", {"index.twig": "{% macro m() %}{{ [1]|map(v => thrower.boom())|join }}{% endmacro %}\nx\n{{ _self.m() }}"}),
    ]:
        variants(c, lambda s, o, main=main, templates=templates: multi(main, templates, CTX, objects=objs, strict=s))
    # the lines of multi-line composition calls: errors raised while calling use the first line of the call, errors
    # raised in a body run by `yield from` the line where the statement ends
    for main, templates in [
        ("i", {"i": "x\n{% include 'k' with\n  five %}", "k": "k"}),
        ("i", {"i": "x\n{% include\n  name\n  with five only %}", "bob": "b"}),
        ("i", {"i": "x\n{% include\n  name ignore missing\n  with five only %}", "bob": "b"}),
        ("i", {"i": "x\n{% include\n  name ignore missing\n  with\n five %}", "bob": "b"}),
        ("e", {"e": "x\n{% embed 'b' with\n  five %}{% endembed %}", "b": "b"}),
        ("e", {"e": "x\n{% embed 'b' with\n  five only %}{% endembed %}", "b": "b"}),
        ("c", {"c": "{% extends 'p' %}\n{% block a %}\n{{ block(\n  'nope'\n) }}{% endblock %}", "p": "{% block a %}{% endblock %}"}),
        ("c", {"c": "{% extends 'p' %}\n{% block a %}\n{{ block(\n  'a',\n  name\n) }}{% endblock %}", "p": "{% block a %}{% endblock %}", "bob": "x"}),
        ("index.twig", {"index.twig": "x\n{{ block(\n  name\n) is defined ? 'y' : 'n' }}{% if block(\n'nope',\n'k'\n) is defined %}{% endif %}{{ block(\n'nope',\n'k'\n) }}", "k": "k"}),
        ("i", {"i": "x\n{% include\n  'k'\n%}", "k": "{{ block('nope') }}"}),
        ("u", {"u": "{% import 'm' as m %}x\n{{ m.boom(\n  thrower\n) }}", "m": "{% macro boom(t) %}{{ t.boom() }}{% endmacro %}"}),
    ]:
        for opt in (0, -1):
            c.append(multi(main, templates, CTX, objects=objs, config={"optimizations": opt}))
    return c


SANDBOX_POLICY = {"tags": ["if", "for", "set", "block", "include", "import", "macro", "from", "embed", "apply", "with"],
                  "filters": ["upper", "join", "escape", "e", "length", "map", "default", "keys"],
                  "methods": {"TwigRsBag": ["getFoo", "isBar", "__toString", "args"], "TwigRsAccess": ["count"]},
                  "properties": {"TwigRsBag": ["pub"], "TwigRsAccess": ["a"]},
                  "functions": ["range", "include", "max"]}


def sandbox():
    """The sandbox: policy checks of tags, filters, functions and tests when a template starts, of methods, properties
    and `__toString()` as they run, sandboxed includes and their messages and lines."""
    c = []
    objs = ("bag", "access", "arrobj", "markup", "iter")

    def sb(templates, main="index.twig", data=None, glob=True, policy=None, strict=False, tests=None, objects=objs, **kw):
        box = {"global": glob, "policy": policy if policy is not None else SANDBOX_POLICY}
        if strict:
            box["strict"] = True
        if tests:
            box["tests"] = tests
        kw.setdefault("load_all", False)
        return case(None, dict(CTX, **(data or {})), templates=templates, main=main, objects=objects, sandbox=box, **kw)

    for tpl in [
        "{{ name|upper }}", "{{ name|lower }}", "{% if true %}{{ name|title }}{% endif %}", "{% do 1 %}",
        "{{ range(1, 2)|join }}", "{{ cycle([1, 2], 1) }}", "{{ name is defined ? 1 : 0 }}{{ 3 is odd ? 1 : 0 }}",
        "{{ name|upper }}{{ name|lower }}{{ cycle([1], 0) }}", "{% autoescape %}{{ name|lower }}{% endautoescape %}",
        "line1\n{{ name|upper }}\n{{ name|title }}\n{{ name|title }}", "{{ 1..3|join }}", "{{ max(1, 2) }}{{ min(1, 2) }}",
        "{% for i in items %}{{ loop.index }}{% endfor %}", "{% set x %}{{ name|capitalize }}{% endset %}",
        "{{ items|map(i => i * 2)|join }}", "{{ items|map('upper')|join }}",
        "{{ items|filter(i => i)|join }}",
        # objects
        "{{ bag.foo }}|{{ bag.bar }}|{{ bag.getFoo() }}", "{{ bag.baz }}", "{{ bag.pub }}", "{{ bag.priv }}",
        "{{ bag.KONST }}", "{{ bag.nul }}", "{{ bag }}", "{{ bag ~ '' }}", "{{ [bag]|join }}", "{{ bag|upper }}",
        "{{ bag.args(1, 2) }}", "{{ bag.self.foo }}", "{{ bag.foo is defined ? 'y' : 'n' }}|{{ bag.self is defined ? 'y' : 'n' }}",
        "{{ bag.nope is defined ? 'y' : 'n' }}", "{{ attribute(bag, 'foo') }}", "{{ markup }}|{{ markup|length }}|{{ markup.count }}",
        "{{ access.a }}|{{ access['a'] }}", "{{ access.b }}", "{{ access.count }}", "{{ access['c'] }}",
        "{{ arrobj.a }}|{{ arrobj['b'] }}", "{{ iter|join }}", "{{ [...items, ...iter]|join }}",
        "{{ {k: bag}|join }}", "{{ [[bag]]|length }}", "{{ name ~ markup }}",
        "{% set b = bag %}{{ b.foo }}{{ b }}", "{{ bag.foo ?? 'x' }}", "{{ (bag ?: 'x').foo }}",
    ]:
        c.append(sb({"index.twig": tpl}))
        c.append(sb({"index.twig": tpl}, strict=True, tests=["defined", "odd"]))
    for tpl in ["{{ name is defined ? 1 : 0 }}", "{{ 3 is odd }}{{ 3 is even }}", "{{ name is same as('x') }}",
                "{{ null is null }}{{ items is iterable }}{{ items is empty }}{{ 1 is constant('PHP_INT_SIZE') }}"]:
        c.append(sb({"index.twig": tpl}, strict=True, tests=["defined", "null"]))
        c.append(sb({"index.twig": tpl}, strict=True))
    # composition under the sandbox
    for templates, main in [
        ({"c": "{% extends 'p' %}{% block b %}C{% endblock %}", "p": "[{% block b %}{% endblock %}]"}, "c"),
        ({"c": "{% extends 'p' %}{% block b %}{{ parent() }}{{ block('x') }}{% endblock %}{% block x %}X{% endblock %}", "p": "[{% block b %}P{% endblock %}]"}, "c"),
        ({"u": "{% use 't' %}{{ block('a') }}", "t": "{% block a %}T{% endblock %}"}, "u"),
        ({"u": "x\n{% use 't' %}{{ block('a') }}", "t": "{% block a %}T{% endblock %}"}, "u"),
        ({"i": "{% include 'k' %}", "k": "{{ name|upper }}{{ name|lower }}"}, "i"),
        ({"i": "a{% include 'k' %}b", "k": "{{ name|lower }}"}, "i"),
        ({"i": "{{ include('k') }}", "k": "{{ name|lower }}"}, "i"),
        ({"u": "{% import 'm' as m %}{{ m.hi() }}", "m": "{% macro hi() %}{{ 'x'|lower }}{% endmacro %}"}, "u"),
        ({"u": "{% from 'm' import hi %}{{ hi() }}", "m": "{% macro hi() %}{{ 'x'|upper }}{% endmacro %}"}, "u"),
        ({"e": "{% embed 'b' %}{% block x %}{{ name|lower }}{% endblock %}{% endembed %}", "b": "[{% block x %}{% endblock %}]"}, "e"),
        ({"c": "{% extends layout %}{% block a %}A{% endblock %}", "l1": "[{% block a %}{% endblock %}]"}, "c"),
        ({"c": "{% extends 'p' %}", "p": "{{ name|lower }}"}, "c"),
    ]:
        c.append(sb(templates, main=main))
        c.append(sb(templates, main=main, strict=True))
    # sandboxed includes in a template that is not sandboxed
    for templates, main in [
        ({"i": "{% sandbox %}{% include 'k' %}{% endsandbox %}{{ name|lower }}", "k": "{{ name|upper }}"}, "i"),
        ({"i": "{% sandbox %}{% include 'k' %}{% endsandbox %}{{ name|lower }}", "k": "{{ name|lower }}"}, "i"),
        ({"i": "{{ include('k', sandboxed = true) }}{{ name|lower }}", "k": "{{ name|upper }}"}, "i"),
        ({"i": "{{ include('k', sandboxed = true) }}", "k": "{{ name|lower }}"}, "i"),
        ({"i": "{{ include('k', {}, true, false, true) }}{{ include('k') }}", "k": "{{ bag }}"}, "i"),
        ({"i": "{{ bag }}{% sandbox %}{% include 'k' %}{% endsandbox %}{{ bag }}", "k": "{{ bag.foo }}"}, "i"),
        ({"i": "{% sandbox %}{% include 'k' ignore missing %}{% include 'gone' ignore missing %}{% endsandbox %}{{ bag.baz }}", "k": "{{ bag.baz }}"}, "i"),
    ]:
        c.append(sb(templates, main=main, glob=False))
    return c

API_PHP = r"""
$spec = json_decode(%s, true);
$twig = new \Twig\Environment(new \Twig\Loader\ArrayLoader($spec['templates']), $spec['config'] + ['cache' => false]);
foreach ($spec['globals'] as $k => $v) {
    $twig->addGlobal($k, $v);
}
$policy = function (array $p, bool $strict) {
    $policy = new \Twig\Sandbox\SecurityPolicy($p['tags'] ?? [], $p['filters'] ?? [], $p['methods'] ?? [], $p['properties'] ?? [], $p['functions'] ?? [], $p['tests'] ?? []);
    $policy->setStrict($strict);
    return $policy;
};
$sandbox = null;
$results = [];
foreach ($spec['ops'] as $op) {
    try {
        $context = $op['context'] ?? [];
        switch ($op['op']) {
            case 'render': $r = $twig->load($op['template'])->render($context); break;
            case 'renderBlock': $r = $twig->load($op['template'])->renderBlock($op['block'], $context); break;
            case 'displayBlock':
                ob_start();
                try { $twig->load($op['template'])->displayBlock($op['block'], $context); } finally { $r = ob_get_clean(); }
                break;
            case 'stream': $r = implode('', iterator_to_array($twig->load($op['template'])->stream($context), false)); break;
            case 'streamBlock': $r = implode('', iterator_to_array($twig->load($op['template'])->streamBlock($op['block'], $context), false)); break;
            case 'hasBlock': $r = $twig->load($op['template'])->hasBlock($op['block'], $context); break;
            case 'getBlockNames': $r = array_values($twig->load($op['template'])->getBlockNames($context)); break;
            case 'resolveTemplate': $r = $twig->resolveTemplate($op['names'])->getTemplateName(); break;
            case 'sandbox':
                $env = new \Twig\Environment(new \Twig\Loader\ArrayLoader($op['templates']), ['cache' => false, 'autoescape' => false]);
                if ($op['used'] ?? false) { $env->load(array_key_first($op['templates'])); }
                $sandbox = new \Twig\Sandbox\Sandbox($env, $policy($op['policy'], $op['strict']));
                $r = null;
                break;
            case 'sandboxRender': $r = $sandbox->render($op['template'], $context); break;
            case 'sandboxRenderBlock': $r = $sandbox->renderBlock($op['template'], $op['block'], $context); break;
            case 'bridge':
                $env = new \Twig\Environment(new \Twig\Loader\ArrayLoader($op['templates']), ['cache' => false, 'autoescape' => false]);
                $sb = new \Twig\Sandbox\Sandbox($env, $policy($op['policy'], true));
                $twig->addExtension(new \Twig\Extension\SandboxBridgeExtension());
                if ($op['runtime'] ?? true) {
                    $twig->addRuntimeLoader(new \Twig\RuntimeLoader\FactoryRuntimeLoader([\Twig\Runtime\SandboxBridgeRuntime::class => fn () => new \Twig\Runtime\SandboxBridgeRuntime($sb)]));
                }
                $r = null;
                break;
            default: throw new \LogicException('unknown op');
        }
        $results[] = ['ok' => true, 'value' => $r];
    } catch (\Throwable $e) {
        $results[] = ['ok' => false, 'class' => get_class($e), 'message' => $e->getMessage()];
    }
}
return $results;
"""


def api_case(templates, ops, config=None, globals=None):
    """A sequence of API calls on one environment (`TemplateWrapper` methods, `resolveTemplate()`, `Sandbox`,
    `render_sandboxed()`); the results come back as JSON."""
    cfg = dict(BASE_CONFIG)
    cfg["strict_variables"] = False
    if config:
        cfg.update(config)
    spec = {"templates": templates, "config": cfg, "globals": globals or {}, "ops": ops}
    return {"kind": "php", "spec": spec, "php_code": API_PHP % php_str(json.dumps(spec))}


def api():
    """The block API of `TemplateWrapper`, `Environment::resolveTemplate()`, the `Sandbox` facade and
    `render_sandboxed()`."""
    c = []
    p = "P[{% block a %}pa{% endblock %}|{% block b %}pb{{ x ?? '' }}{% endblock %}]"
    ch = "{% extends 'p' %}{% use 't' %}{% block a %}ca+{{ parent() }}{% endblock %}{% block c %}cc{% endblock %}"
    t = "{% block t %}tt{% endblock %}{% block a %}ta{% endblock %}"
    T = {"p": p, "c": ch, "t": t}
    c.append(api_case(T, [
        {"op": "render", "template": "c"},
        {"op": "renderBlock", "template": "c", "block": "a"},
        {"op": "renderBlock", "template": "c", "block": "b", "context": {"x": "X"}},
        {"op": "renderBlock", "template": "c", "block": "t"},
        {"op": "renderBlock", "template": "c", "block": "zz"},
        {"op": "renderBlock", "template": "p", "block": "zz"},
        {"op": "displayBlock", "template": "c", "block": "a"},
        {"op": "streamBlock", "template": "c", "block": "c"},
        {"op": "stream", "template": "c"},
        {"op": "hasBlock", "template": "c", "block": "a"},
        {"op": "hasBlock", "template": "c", "block": "b"},
        {"op": "hasBlock", "template": "c", "block": "t"},
        {"op": "hasBlock", "template": "c", "block": "zz"},
        {"op": "hasBlock", "template": "p", "block": "c"},
        {"op": "getBlockNames", "template": "c"},
        {"op": "getBlockNames", "template": "p"},
        {"op": "getBlockNames", "template": "t"},
    ]))
    c.append(api_case({"g": "{% block a %}{{ g }}|{{ x ?? 'nox' }}{% endblock %}"}, [
        {"op": "renderBlock", "template": "g", "block": "a"},
        {"op": "renderBlock", "template": "g", "block": "a", "context": {"g": "ctx", "x": 1}},
    ], globals={"g": "global"}))
    # body imports and renderBlock before and after a full render (Template::$macros)
    m = {"u": "{% import 'm' as forms %}{% from 'm' import hi %}{% block a %}{{ forms.hi('a') }}{% endblock %}{% block b %}{{ hi('b') }}{% endblock %}{% block c %}{% import 'm' as local %}{{ local.hi('c') }}{% endblock %}",
         "m": "{% macro hi(n) %}hi {{ n }}{% endmacro %}"}
    c.append(api_case(m, [
        {"op": "renderBlock", "template": "u", "block": "a"},
        {"op": "renderBlock", "template": "u", "block": "b"},
        {"op": "renderBlock", "template": "u", "block": "c"},
        {"op": "render", "template": "u"},
        {"op": "renderBlock", "template": "u", "block": "a"},
        {"op": "renderBlock", "template": "u", "block": "b"},
    ]))
    # dynamic parents resolved with the given context
    d = {"d": "{% extends layout %}{% block a %}da{% endblock %}", "l1": "{% block a %}{% endblock %}{% block l %}l1{% endblock %}",
         "l2": "{% block a %}{% endblock %}{% block m %}l2{% endblock %}"}
    c.append(api_case(d, [
        {"op": "renderBlock", "template": "d", "block": "l", "context": {"layout": "l1"}},
        {"op": "renderBlock", "template": "d", "block": "m", "context": {"layout": "l2"}},
        {"op": "renderBlock", "template": "d", "block": "l", "context": {"layout": "l2"}},
        {"op": "hasBlock", "template": "d", "block": "m", "context": {"layout": "l2"}},
        {"op": "hasBlock", "template": "d", "block": "m", "context": {"layout": "l1"}},
        {"op": "getBlockNames", "template": "d", "context": {"layout": "l2"}},
        {"op": "renderBlock", "template": "d", "block": "l"},
        {"op": "renderBlock", "template": "d", "block": "a", "context": {"layout": "missing"}},
        {"op": "renderBlock", "template": "d", "block": "zz", "context": {"layout": "missing"}},
    ]))
    # constant parents: the child's own block, a parent block that calls block()
    k = {"c": "{% extends 'p' %}{% block x %}cx{% endblock %}", "p": "{% block y %}[{{ block('x') }}]{% endblock %}{% block x %}px{% endblock %}"}
    c.append(api_case(k, [
        {"op": "renderBlock", "template": "c", "block": "y"},
        {"op": "renderBlock", "template": "p", "block": "y"},
        {"op": "render", "template": "c"},
    ]))
    c.append(api_case({"e": "{% block a %}\n{{ 1 // 0 }}{% endblock %}", "s": "{% block a %}\n{{ undefined_var }}{% endblock %}"}, [
        {"op": "renderBlock", "template": "e", "block": "a"},
        {"op": "renderBlock", "template": "s", "block": "a"},
    ], config={"strict_variables": True}))
    # resolveTemplate()
    c.append(api_case({"a": "A", "b": "B"}, [
        {"op": "resolveTemplate", "names": ["missing", "b", "a"]},
        {"op": "resolveTemplate", "names": ["a"]},
        {"op": "resolveTemplate", "names": ["missing"]},
        {"op": "resolveTemplate", "names": ["n1", "n2"]},
        {"op": "resolveTemplate", "names": []},
    ]))
    # the Sandbox facade
    policy = {"tags": ["if"], "filters": ["upper"], "functions": []}
    sbt = {"ok": "{% if true %}{{ 'a'|upper }}{% endif %}", "bad": "{{ 'a'|lower }}", "blk": "{% block b %}{{ x|upper }}{% endblock %}",
           "ext": "{% extends 'ok' %}"}
    c.append(api_case({}, [
        {"op": "sandbox", "templates": sbt, "policy": policy, "strict": False},
        {"op": "sandbox", "templates": sbt, "policy": policy, "strict": True, "used": True},
        {"op": "sandbox", "templates": sbt, "policy": policy, "strict": True},
        {"op": "sandboxRender", "template": "ok"},
        {"op": "sandboxRender", "template": "bad"},
        {"op": "sandboxRender", "template": "ext"},
        {"op": "sandboxRenderBlock", "template": "blk", "block": "b", "context": {"x": "x"}},
    ]))
    # render_sandboxed() through the SandboxBridgeRuntime
    bridge = {"bad": "{{ 'a'|lower }}", "ok": "<{{ x|upper }}>{{ name ?? 'no-name' }}"}
    for tpl, strategy_ops in [
        ("{{ render_sandboxed('ok', {x: 'y'}, 'html') }}", None),
        ("{{ render_sandboxed('ok', {x: '<y>'}, output_strategy = 'js') }}", None),
        ("{{ render_sandboxed('bad', {}, 'html') }}", None),
        ("{{ render_sandboxed('missing', {}, 'html') }}", None),
        ("{{ render_sandboxed('ok', 'str', 'html') }}", None),
    ]:
        c.append(api_case({"index.twig": tpl}, [
            {"op": "bridge", "templates": bridge, "policy": {"filters": ["upper"]}},
            {"op": "render", "template": "index.twig", "context": {"name": "outer"}},
        ]))
    c.append(api_case({"index.twig": "{{ render_sandboxed('ok', {}, 'html') }}"}, [
        {"op": "bridge", "templates": bridge, "policy": {"filters": ["upper"]}, "runtime": False},
        {"op": "render", "template": "index.twig"},
    ]))
    return c

def parity():
    """Object cases of plans/parity-fixes.md (the other cases are in tests/vectors/engine/parity/): the `loop`
    bookkeeping writes of ForLoopNode into a userland ArrayAccess object (`offsetGet()` returns a copy, so `++` warns
    `Indirect modification of overloaded element of %s has no effect`; `offsetSet()` gets `first`) and into an object
    without ArrayAccess."""
    c = []
    for errors in ("collect", "throw"):
        c.append(case("{% for x in [1, 2] %}{% set loop = access %}{% endfor %}{{ access.a }}", objects=["access"],
                      errors=errors))
    c.append(case("{% for x in [1, 2] %}{% set loop = access %}{{ loop.a }}{% endfor %}", objects=["access"]))
    c.append(case("{% for x in [1, 2] %}{% set loop = magic %}{% endfor %}", objects=["magic"]))
    c.append(case("{% for x in [1, 2] %}{% set loop = bag %}{% endfor %}", objects=["bag"]))
    return c


def compile_errors():
    """When the generated PHP of a template fails to compile (plans/parity-fixes.md P16, P32): Twig compiles the whole
    file (the template and its embedded templates) before PHP sees it, so Twig's own errors come first; PHP parses the
    whole file, so a ParseError anywhere comes next (unwrapped by compileSource()); then PHP compiles it in file order
    and stops at the first E_COMPILE_ERROR (an uncatchable fatal error, `"phase": "fatal"`, for which twig-rs raises a
    CompileError with PHP's message, reference/deviations.md). Inside array literals zend_try_ct_eval_array() folds the
    elements first (building the nested constant literals, which may fail) and drops the branch a constant test does
    not choose; zend_compile_short_circuiting() does not compile the right operand a constant left operand decides."""
    d = {"a": [1], "z": 0, "n": None}
    parse = "{{ x|default(...[]) }}"
    embed = "{% block b %}{% endblock %}"
    c = [case(t, d) for t in [
        # Twig's errors come first, also from an embedded template; then the ParseError, then the first fatal error
        "{{ attribute(n, ...[]) }}{{ range() }}", "{{ x|default(...[]) }}{{ range() }}",
        "{{ [...true] }}{{ range() }}", "{% block b %}B{% endblock %}{{ block(...['b']) }}{{ range() }}",
        "{{ attribute(n, ...[]) }}{{ max(foo: 1) }}", "{{ range() }}{{ attribute(n, ...[]) }}",
        "{{ attribute(n, ...[]) }}{{ 1|nope }}", "{{ attribute(n, ...[]) }}{% macro m(a, a) %}{% endmacro %}",
        "{{ attribute(n, ...[]) }}{% macro m() %}{{ range() }}{% endmacro %}", "{{ false and range() }}",
        "{{ attribute(n, ...[]) }}{{ x|default(...[]) }}", "{{ false and x|default(...[]) }}",
        "{{ x|default(...[]) }}", "{{ x.y|default(...['d']) }}",
        "{{ attribute(n, ...[]) }}{{ [...true] }}", "{{ [...true] }}{{ attribute(n, ...[]) }}",
        "{% block b %}{{ [...true] }}{% endblock %}{{ attribute(n, ...[]) }}",
        "{% macro m() %}{{ [...true] }}{% endmacro %}{% block b %}{{ attribute(n, ...[]) }}{% endblock %}",
        # the elements of an array literal are folded before any of them is compiled
        "{{ [attribute(n, ...[]), [...true]]|length }}", "{{ [[...true], attribute(n, ...[])]|length }}",
        "{{ [[attribute(n, ...[])], [...true]]|length }}", "{{ [...true, attribute(n, ...[])]|length }}",
        "{{ [n and [...true]]|length }}", "{{ [(n ? [...true] : 1)]|length }}", "{{ [n ~ [...true]]|length }}",
        "{{ [false and [...true]]|length }}", "{{ [(true ? [...true] : 1)]|length }}",
        "{{ {([...true]): 1}|length }}", "{{ {(attribute(n, ...[])): [...true]}|length }}",
        "{{ {([...false]): attribute(n, ...[])}|length }}", "{% set a, b = 1, [...true] %}",
        "{% set a, b = attribute(n, ...[]), [...true] %}", "{{ attribute(n, ...[...true]) }}",
        "{% block b %}{% endblock %}{{ block(...[[...true]]) }}", "{{ a.b(...[...true]) }}",
        "{{ include(...[...true]) }}", "{% macro m(a = [1]) %}{% endmacro %}{{ _self.m(...[...true]) }}",
        "{% deprecated [...true] %}", "{% for x in [...true] %}{% endfor %}", "{% if [...true] %}{% endif %}",
        "{% with {a: [...true]} %}{% endwith %}",
        # code PHP never compiles: the dropped branch of a folded conditional, the right operand a constant decides
        "{{ false and attribute(n, ...[]) }}", "{{ true or attribute(n, ...[]) }}", "{{ 'a' or attribute(n, ...[]) }}",
        "{{ not (false and attribute(n, ...[])) }}", "{{ (false and true) and attribute(n, ...[]) }}",
        "{{ false and [...true] }}", "{% block b %}{% endblock %}{{ false and block(...['b']) }}",
        "{{ [true ? 1 : attribute(n, ...[])]|length }}", "{{ [(false ? [...true] : 1)]|length }}",
        "{{ [true or attribute(n, ...[])]|length }}", "{{ [1 ?? attribute(n, ...[])]|length }}",
        "{{ [1 ?: attribute(n, ...[])]|length }}", "{{ [false and attribute(n, ...[])]|length }}",
        "{{ [(true ? 1 : max([...true]))]|length }}", "{{ [...(true ? [1] : [...true])]|length }}",
        # ... and code it compiles
        "{{ true ? 1 : attribute(n, ...[]) }}", "{{ (false ? [...true] : 1) }}", "{{ 1 ?? attribute(n, ...[]) }}",
        "{{ 1 ?: attribute(n, ...[]) }}", "{{ n ?? attribute(n, ...[]) }}", "{{ [n ?? attribute(n, ...[])]|length }}",
        "{{ [true and attribute(n, ...[])]|length }}", "{{ [(false ? 1 : max([...true]))]|length }}",
        "{{ attribute(n, ...[]) is defined ? 1 : 2 }}", "{{ attribute(n, ...[]) ?? 'd' }}",
    ]]
    c.append(case("{{ [...true] }}", d, config={"autoescape": "html"}))
    c.append(case("{{ attribute(n, ...[]) }}{{ range() }}", d, config={"optimizations": -1}))
    c.append(case(None, d, templates={"index.twig": "{% extends x ?? [...true] %}{% block b %}"
                                                    "{{ attribute(n, ...[]) }}{% endblock %}", "a.twig": ""}))
    for main in ["{{ attribute(n, ...[]) }}{% embed 'e.twig' %}{% block b %}{{ range() }}{% endblock %}{% endembed %}",
                 "{{ x|default(...[]) }}{% embed 'e.twig' %}{% block b %}{{ range() }}{% endblock %}{% endembed %}",
                 "{{ attribute(n, ...[]) }}{% embed 'e.twig' %}{% block b %}{{ x|default(...[]) }}{% endblock %}"
                 "{% endembed %}",
                 "{% embed 'e.twig' %}{% block b %}{{ [...true] }}{% endblock %}{% endembed %}{{ attribute(n, ...[]) }}"]:
        c.append(case(None, d, templates={"index.twig": main, "e.twig": embed}))
    # the ParseError of another template, loaded while rendering: templates wrap it like any PHP throwable
    for main in ["{% include 'p.twig' %}", "{{ include('p.twig') }}", "{% extends 'p.twig' %}",
                 "{% import 'p.twig' as m %}{{ m.f() }}", "{{ source('p.twig') }}", "{% embed 'p.twig' %}{% endembed %}",
                 "{% include 'p.twig' ignore missing %}", "{% use 'p.twig' %}"]:
        c.append(multi("index.twig", {"index.twig": main, "p.twig": parse}, d))
    c.append(multi("index.twig", {"index.twig": "{{ block('b', 'p.twig') }}",
                                  "p.twig": "{% block b %}" + parse + "{% endblock %}"}, d))
    c.append(multi("index.twig", {"index.twig": "{% include 'p.twig' %}",
                                  "p.twig": "{{ attribute(n, ...[]) }}{{ range() }}"}, d))
    c.append(case("{{ include(template_from_string('{{ x|default(...[]) }}')) }}", d, extensions=["string_loader"]))
    return c


TOPICS = {"output": output, "expressions": expressions, "variables": variables, "statements": statements,
          "errors": errors, "objects": objects, "lines": lines, "diagnostics": diagnostics,
          "optimized": optimized, "escape": escape, "inheritance": inheritance, "traits": traits, "embed": embed,
          "include": include, "macros": macros, "composition_errors": composition_errors, "sandbox": sandbox,
          "api": api, "parity": parity, "compile_errors": compile_errors}

if __name__ == "__main__":
    args = sys.argv[1:]
    oracle = "--oracle" in args
    names = [a for a in args if a != "--oracle"]
    for name in names or TOPICS:
        write(name, TOPICS[name](), oracle)
