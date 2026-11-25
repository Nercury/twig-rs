"""Writes the oracle batches tests/vectors/callables/arrays/<topic>.gen.json of the array and iteration callables
(tests/callables_arrays.rs; the `random` topic is read by the unit tests of src/extension/runtime/arrays.rs):
templates rendered by a plain Twig 3.31.1 environment (autoescape off) with their outputs, error messages with lines,
deprecations and PHP diagnostics.

Values are shown with `json_encode(1024)` (JSON_PRESERVE_ZERO_FRACTION); the Rust harness gives `json_encode` a
runtime calling `twig_value::builtins::format::json_encode`, so the vectors do not depend on the string group.
Object cases build their values in `data_php` with the PHP classes below; tests/callables_arrays.rs has the same
classes in Rust and builds every object from the case's `objects` specs.

Regenerate from the repository root (Git Bash on Windows needs MSYS_NO_PATHCONV=1):

    python -I tests/vectors/callables/arrays/arrays.gen.py [topic ...]            # writes <topic>.gen.json
    python -I tests/vectors/callables/arrays/arrays.gen.py --oracle [topic ...]   # and runs them: <topic>.json

`--oracle` pipes each batch through `docker run --rm -i twig-oracle:3.31.1 php runner.php --pretty`.
"""

import base64
import json
import os
import subprocess
import sys

OUT = os.path.dirname(os.path.abspath(__file__))

PHP_CLASSES = r"""
if (!function_exists('twig_rs_gen')) {
    function twig_rs_gen(array $pairs) { foreach ($pairs as $p) { yield $p[0] => $p[1]; } }
    class TwigRsLogIter implements \Iterator {
        private $i = 0;
        public $log = [];
        public function __construct(private array $pairs) {}
        public function current(): mixed { $this->log[] = 'current'; return $this->pairs[$this->i][1]; }
        public function key(): mixed { $this->log[] = 'key'; return $this->pairs[$this->i][0]; }
        public function next(): void { $this->log[] = 'next'; $this->i++; }
        public function rewind(): void { $this->log[] = 'rewind'; $this->i = 0; }
        public function valid(): bool { $this->log[] = 'valid'; return $this->i < count($this->pairs); }
    }
    class TwigRsAgg implements \IteratorAggregate {
        public function __construct(private array $data) {}
        public function getIterator(): \Iterator { return new \ArrayIterator($this->data); }
    }
    class TwigRsAggGen implements \IteratorAggregate {
        public function __construct(private array $pairs) {}
        public function getIterator(): \Iterator { return twig_rs_gen($this->pairs); }
    }
    class TwigRsCountAccess implements \ArrayAccess, \Countable {
        public function __construct(private array $data) {}
        public function offsetExists($o): bool { return isset($this->data[$o]); }
        public function offsetGet($o): mixed { return $this->data[$o]; }
        public function offsetSet($o, $v): void {}
        public function offsetUnset($o): void {}
        public function count(): int { return count($this->data); }
    }
    class TwigRsAccess implements \ArrayAccess {
        public function __construct(private array $data) {}
        public function offsetExists($o): bool { return isset($this->data[$o]); }
        public function offsetGet($o): mixed { return $this->data[$o]; }
        public function offsetSet($o, $v): void {}
        public function offsetUnset($o): void {}
    }
    class TwigRsAccessAgg implements \ArrayAccess, \IteratorAggregate {
        public function __construct(private array $data) {}
        public function offsetExists($o): bool { return isset($this->data[$o]); }
        public function offsetGet($o): mixed { return $this->data[$o]; }
        public function offsetSet($o, $v): void {}
        public function offsetUnset($o): void {}
        public function getIterator(): \Iterator { return new \ArrayIterator($this->data); }
    }
    class TwigRsStr {
        public function __construct(private string $s) {}
        public function __toString(): string { return $this->s; }
    }
    class TwigRsCountable implements \Countable {
        public function __construct(private int $n) {}
        public function count(): int { return $this->n; }
    }
    class TwigRsPlain {}
    class TwigRsInvoke {
        public function __invoke(...$args) { return json_encode($args); }
    }
    class TwigRsCmp {
        public function __invoke($a, $b) { return $b <=> $a; }
    }
}
"""


def b64(text):
    return base64.b64encode(text.encode("utf-8")).decode("ascii")


def php_json(value):
    return "json_decode(base64_decode('%s'), true)" % b64(json.dumps(value, ensure_ascii=False))


def php_string(value):
    return "base64_decode('%s')" % b64(value)


def php_object(spec):
    kind = spec["kind"]
    if kind == "gen":
        return "twig_rs_gen(%s)" % php_json(spec["pairs"])
    if kind == "log_iter":
        return "new TwigRsLogIter(%s)" % php_json(spec["pairs"])
    if kind == "agg":
        return "new TwigRsAgg(%s)" % php_json(spec["data"])
    if kind == "agg_gen":
        return "new TwigRsAggGen(%s)" % php_json(spec["pairs"])
    if kind == "array_object":
        return "new \\ArrayObject(%s)" % php_json(spec["data"])
    if kind == "array_iterator":
        return "new \\ArrayIterator(%s)" % php_json(spec["data"])
    if kind == "count_access":
        return "new TwigRsCountAccess(%s)" % php_json(spec["data"])
    if kind == "access":
        return "new TwigRsAccess(%s)" % php_json(spec["data"])
    if kind == "access_agg":
        return "new TwigRsAccessAgg(%s)" % php_json(spec["data"])
    if kind == "str":
        return "new TwigRsStr(%s)" % php_string(spec["value"])
    if kind == "markup":
        return "new \\Twig\\Markup(%s, 'UTF-8')" % php_string(spec["value"])
    if kind == "std":
        return "(object) %s" % php_json(spec["data"])
    if kind == "countable":
        return "new TwigRsCountable(%d)" % spec["n"]
    if kind == "plain":
        return "new TwigRsPlain()"
    if kind == "invoke":
        return "new TwigRsInvoke()"
    if kind == "cmp":
        return "new TwigRsCmp()"
    if kind == "list":
        return "[%s]" % ", ".join(php_object(item) for item in spec["items"])
    if kind == "bytes":
        return "hex2bin('%s')" % spec["hex"]
    raise ValueError(kind)


def case(tpl, data=None, objects=None, strict=False, config=None, errors="collect", seed=None, sandbox=None):
    cfg = {"autoescape": False, "strict_variables": strict}
    if config:
        cfg.update(config)
    c = {"templates": {"index.twig": tpl}, "env": "plain", "config": cfg, "errors": errors}
    if sandbox is not None:
        c["sandbox"] = sandbox
    if objects or seed is not None:
        php = PHP_CLASSES
        if seed is not None:
            php += "mt_srand(%d);\n" % seed
            c["seed"] = seed
        parts = ", ".join("'%s' => %s" % (name, php_object(spec)) for name, spec in (objects or {}).items())
        c["data_php"] = php + "return array_merge(%s, [%s]);" % (php_json(data or {}), parts)
        c["data_rust"] = data or {}
        if objects:
            c["objects"] = objects
    elif data is not None:
        c["data_json"] = data
    return c


def write(topic, cases, oracle):
    for i, c in enumerate(cases):
        c["id"] = "%s-%03d" % (topic, i + 1)
    batch = {"cases": cases, "isolation": "none"}
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
    failures = [r for r in results if r.get("phase") in ("fatal", "lost", "setup", "data", "oracle")]
    print(topic, len(cases), "cases", "problems:" if failures else "", failures[:1])


def j(expr):
    """`{{ (expr)|json_encode(1024) }}`: the value with its type (JSON_PRESERVE_ZERO_FRACTION)."""
    return "{{ (%s)|json_encode(1024) }}" % expr


def each(expr):
    """A `for` loop printing `key=json(value);` for every element (works for any iterable)."""
    return "{%% for k, v in %s %%}{{ k|json_encode }}={{ v|json_encode(1024) }};{%% endfor %%}" % expr


DATA = {"list": [3, 1, 2], "map": {"b": 2, "a": 3, "c": 1}, "nums": {"1": "a", "2": "b", "3": "c"},
        "mixed": {"x": "a", "2": "b", "y": "c"}, "empty": [], "nul": None, "s": "abc", "n": 42, "f": 1.5,
        "t": True, "rows": [{"id": 1, "name": "a", "g": "x"}, {"id": 2, "name": "b", "g": "y"}, {"id": 3, "g": "x"}],
        "words": ["b", "a", "B", "10", "9", "1e1"], "messy": ["10", 9, "9a", 1.5, "abc", True, None]}


def exprs(items, data=DATA, **kw):
    return [case(j(e), data, **kw) for e in items]


def filters():
    c = []
    # join
    c += exprs([
        "list|join", "[1, 2, 3]|join(', ')", "[1, 2, 3]|join(', ', ' and ')", "[1]|join(', ', ' and ')",
        "[1, 2]|join(', ', ', ')", "s|join(',')", "nul|join", "n|join", "map|join(',')",
        "[true, false, null, 1.5, 1.0]|join(',')", "[[1], 2]|join(',')", "[1, 2]|join(null)",
        "[1, [2]]|join(', ', ' and ')", "[[1]]|join(',', '-')", "[null]|join(',', '-')", "[1.5]|join(',', '-')",
        "[true]|join(',', '-')", "[false]|join(',', '-')", "[1, 2, 3]|join(',', [1])", "{a: 1, b: 2}|join(glue='-')",
        "[1, 2]|join(...[','])", "[1, 2]|join(...{glue: '+'})", "[1, 2]|join(...{nope: 1})",
        "[1, 2]|join(...{value: 1})", "f|join", "t|join", "[1, 2]|join(1)", "[1, 2, 3]|join(and='&')",
    ])
    # split
    c += exprs([
        "'one,two,three'|split(',')", "'one,two,three,four'|split(',', 2)", "'one,two,three'|split(',', -1)",
        "'one,two'|split(',', 0)", "'123'|split('')", "'aabbcc'|split('', 2)", "'aabbc'|split('', 2)",
        "'ab'|split('', 5)", "'éàü'|split('')", "''|split('')", "'abc'|split('', -1)", "nul|split(',')",
        "12345|split(3)", "'a,b'|split(null)", "'a,b'|split(false)", "'a,b'|split(',', 'x')", "'abc'|split('', 'x')",
        "[1]|split(',')", "[1]|split('', 2)", "1.5|split('')", "true|split('')", "false|split('')",
        "'abcdef'|split('', 2.5)", "'abcdef'|split('', '2')", "'éàüö'|split('', 3)", "12345|split('', 2)",
        "12|split('', 5)", "'a,b,,c'|split(',')", "''|split(',')", "''|split(',', -1)", "'abc'|split('', null)",
        "'abc'|split('', 1)", "'abc'|split('', 0)", "'a'|split(delimiter='', limit=2)",
    ])
    # sort
    c += exprs([
        "list|sort", "map|sort", "words|sort", "messy|sort", "[3.5, 3.2, 3.8]|sort((a, b) => a - b)",
        "[3, 1, 2]|sort((a, b) => b <=> a)", "[3, 1, 2]|sort((a, b) => a > b)", "{b: 1, a: 1}|sort", "nul|sort",
        "s|sort", "[]|sort", "[1, 2]|sort(null)", "[2, 1]|sort('nope')", "[2, 1]|sort([1, 2])",
        "[5, '5', 'a', 3, null, true, 2.5, '10', 1, 0, 'b', false, -1, '1e1', [1], 7, 8, 9]|sort",
        "[5, '5', 'a', 3, null, true, 2.5, '10', 1, 0, 'b', false, -1, '1e1', 7, 8, 9]|sort((a, b) => a <=> b)",
        "rows|sort((a, b) => b.id <=> a.id)|column('id')", "[2, 1]|sort(...{arrow: (a, b) => a <=> b})",
        "['b', 'a']|sort((a, b) => 0)", "[3, 1, 2]|sort((a, b) => a < b)", "[1, 2]|sort((a, b) => '1')",
    ])
    # merge
    c += exprs([
        "[1, 2]|merge(['a', 'b'])", "{a: 1, b: 2}|merge({b: 3, c: 4})", "nums|merge({1: 'z'})",
        "[1]|merge({x: 1}, [2])", "nul|merge([1])", "[1]|merge([2], 'x')", "[1]|merge", "{a: 1}|merge([])",
        "[1]|merge(...[[2], [3]])", "[1]|merge(...{x: [2]})", "[1]|merge(...{x: 5})", "[1]|merge(...{'1x': 5})",
        "[]|merge({a: 1})", "mixed|merge(mixed)", "[1]|merge(nul)", "s|merge([1])",
    ])
    # batch
    c += exprs([
        "['a', 'b', 'c', 'd']|batch(3)", "['a', 'b', 'c', 'd']|batch(3, 'x')", "['a', 'b', 'c', 'd']|batch(3, 'x', false)",
        "{a: 1, b: 2, c: 3}|batch(2, 'z')", "[1, 2, 3]|batch(2.1)", "[1, 2, 3]|batch('2')", "[]|batch(2, 'x')",
        "[1]|batch(0)", "[1]|batch(-1)", "s|batch(2)", "[1]|batch('abc')", "[1, 2]|batch(null)",
        "[1, 2, 3]|batch(2, null)", "[1, 2, 3]|batch(2, 0)", "nums|batch(2, 'f')", "[1, 2, 3]|batch(2, 'x', [])",
        "[1, 2, 3]|batch(2, 'x', 'yes')", "[1, 2, 3]|batch(1e100)", "[1, 2, 3]|batch(true)", "[1, 2, 3, 4]|batch(2, 'x')",
        "[1, 2, 3]|batch(' 2')", "[1, 2, 3]|batch('2.5')", "[1, 2, 3]|batch(size=2, fill='y')",
        "[1, 2, 3]|batch(2, 'x', preserve_keys=false)", "{5: 'a', 9: 'b', 7: 'c'}|batch(2, 'x')",
    ])
    # column
    c += exprs([
        "rows|column('name')", "rows|column('name', 'id')", "rows|column(null, 'g')", "rows|column('g', 'g')",
        "[[1, 2], [3, 4]]|column(1)", "[[1, 2], [3, 4]]|column(1, 0)", "s|column('a')", "rows|column([])",
        "rows|column('id', 1.5)", "rows|column('id', 'nope')", "[[1, 2], 'x', [3]]|column(0)", "rows|column(true)",
        "[{a: 1}, {a: 2}]|column('a', [])", "[{a: null}, {b: 1}]|column('a')", "[{'1': 'x'}, {1: 'y'}]|column('1')",
    ])
    # filter
    c += [
        case(each("[1, 5, 3, 4, 5]|filter(v => v > 3)"), DATA), case(j("map|filter((v, k) => k != 'a')"), DATA),
    ]
    c += exprs([
        "[0, 1, '', 'a', null]|filter(null)", "[1, 2]|filter('nope')", "s|filter(v => v)", "[]|filter(v => v)",
        "[1, 2, 3]|filter(v => v is odd)|keys", "[1, 2]|filter(v => 'x')", "[1, 2]|filter(v => [])",
        "nul|filter(v => v)", "[1, 2]|filter(v => v, 'x')", "[1, 2]|filter([1])",
    ])
    # map, reduce, find
    c += exprs([
        "[1, 2, 3]|map(v => v * 2)", "map|map((v, k) => k ~ v)", "s|map(v => v)", "[1, 2]|map(v => v, 'extra')",
        "[1, 2]|map(...[v => v + 1])", "[1, 2]|map(...{arrow: v => v + 1})", "[1, 2]|map(...{nope: 1})",
        "{1: 'a', '01': 'b'}|map((v, k) => k)", "[]|map(v => v)", "[1, 2]|map('nope')", "nul|map(v => v)",
        "[1, 2, 3]|reduce((carry, v, k) => carry + v * k)", "[1, 2, 3]|reduce((c, v) => c + v, 10)",
        "[]|reduce((c, v) => c + v)", "s|reduce((c, v) => c)", "map|reduce((c, v, k) => c ~ k ~ v, '')",
        "[1, 2]|reduce((c, v) => c ~ v, initial='>')",
        "[34, 36, 38, 40]|find(v => v > 38)", "[1, 2]|find(v => v > 5)", "map|find((v, k) => k == 'a')",
        "s|find(v => v)", "[0, '', 'x']|find(v => v)", "[null, false, 0]|find(v => true)",
    ])
    # reverse, shuffle
    c += exprs([
        "nums|reverse", "nums|reverse(true)", "mixed|reverse", "'été'|reverse", "123|reverse", "nul|reverse",
        "1.5|reverse", "true|reverse", "[1, 2]|reverse(null)", "[1, 2]|reverse([])", "''|reverse", "'a'|reverse",
        "list|reverse(preserve_keys=true)", "'😀ab'|reverse",
        "[1]|shuffle", "{a: 1}|shuffle", "n|shuffle", "nul|shuffle", "''|shuffle", "'a'|shuffle",
        "[3, 1, 2]|shuffle|sort|join(',')", "'abc'|shuffle|length", "t|shuffle", "1.5|shuffle", "[]|shuffle",
        "'é'|shuffle",
    ])
    # length
    c += exprs([
        "nul|length", "'abc'|length", "'été'|length", "12345|length", "1.5|length", "t|length", "false|length",
        "list|length", "map|length", "''|length", "(0.1 + 0.2)|length", "[[1, 2]]|length", "-1|length",
    ])
    # slice, first, last
    c += exprs([
        "[1, 2, 3, 4, 5]|slice(1, 2)", "[1, 2, 3, 4, 5]|slice(1, 2, true)", "{a: 1, b: 2, 3: 3, 4: 4}|slice(1)",
        "{a: 1, b: 2, 3: 3, 4: 4}|slice(1, null, true)", "[1, 2, 3]|slice(-2)", "[1, 2, 3]|slice(5)",
        "'12345'[1:2]", "'12345'[:2]", "'12345'[2:]", "'12345'[:-2]", "'abcdef'|slice(-3, -1)", "'abc'|slice(-10, 2)",
        "12345|slice(1, 2)", "nul|slice(0, 1)", "'été'|slice(1)", "[1, 2, 3]|slice('1')", "[1, 2, 3]|slice(1.5)",
        "[1, 2, 3]|slice('x')", "'abc'|slice('x')", "'abc'|slice(null)", "[1, 2, 3]|slice(null)",
        "[1, 2, 3]|slice(0, 'x')", "'abc'|slice(1, 'x')", "[1, 2, 3]|slice(1, 1, 'yes')", "s[1:]",
        "list[1:1]", "map[1:]", "nums[0:2]", "'abc'|slice(1, null)", "1.5|slice(1)", "t|slice(0)",
        "[1, 2, 3]|slice(-1, -5)", "'abc'|slice(0, -5)", "[1, 2, 3]|slice(start=1)", "'abc'|slice(...[1, 1])",
        "[1, 2]|first", "{a: 1, b: 2}|first", "'1234'|first", "5|first", "nul|first", "[]|first", "'Ä€é'|first",
        "''|first", "[1, 2]|last", "'1234'|last", "[]|last", "'été'|last", "map|last", "1.5|first", "t|last",
        "[null]|first", "[false]|last",
    ])
    # keys, invoke
    c += exprs([
        "[5, 6]|keys", "{a: 1, '2': 2}|keys", "s|keys", "nul|keys", "map|keys", "[]|keys", "n|keys",
    ])
    inv = "{% set f = (a, b) => a ~ '-' ~ b %}{% set g = a => a * 2 %}"
    c += [case(inv + j(e), DATA) for e in [
        "f|invoke(1, 2)", "g|invoke(arguments=21)", "f|invoke(1, 2, 3)", "f|invoke(...[3, 4])",
        "f|invoke(...{a: 1, b: 2})", "f|invoke(...{__a__: 1, __b__: 2})", "f|invoke(1, ...{__b__: 2})",
        "g|invoke(...{arguments: 3})",
    ]]
    return c


def functions():
    c = []
    c += exprs([
        "max(1, 3, 2)", "max([1, 3, 2])", "max({2: 'e', 1: 'a', 3: 'b'})", "max('apple', 'banana')",
        "max([1, 2], [1, 3])", "max('10', 9)", "max('abc', 0)", "min('abc', 0)", "min('a', 'A')", "min(null, false)",
        "min(false, null)", "min(0, '0')", "max(0, '0')", "max(null, false)", "min(null, false, null)",
        "min(0, '0', 5)", "min([0, '0'])", "max(...[0, '0'])", "min(...[0, '0'])", "min(...[0, '0'], ...[])",
        "max(1)", "max([])", "max(...[])", "max(...{value: [1, 5]})", "max(...{values: 1})", "max(...{value: 1, x: 2})",
        "max(1, ...{value: 2})", "min('1.0', 1)", "min(1, '1.0')", "max(1.0, 1)", "max(1, 1.0)", "min(1.0, 1)",
        "min(1, 1.0)", "max(value=1, values=2)", "max([1, 2], 3)", "max(nul, 0)", "max('', null)",
        "max(max(1, 2), 0)", "min(...[[0, '0']|first, '0'])", "max(0, min(...[0, '0']))",
        "min(max(0, '0'), ...['0'])", "min(min(0, '0'), '0')", "max(...['0', 0])", "max('0', 0)",
        "min(...{value: ['b', 'a']})", "max(1, 2, ...[3])", "min(...[3], ...[2])", "min({a: 2, b: 1})",
        "max(1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17)",
    ])
    c += exprs([
        "range(0, 3)", "range(0, 6, 2)", "range(3, 0)", "range('a', 'e', 2)", "range(0, 1, 0.25)", "range(5, 1, 2)",
        "range('1', '3')", "range('10', '8')", "range(1, 2, -1)", "range(1, 3, 0)", "range(1, 3, 5)", "range('', 'c')",
        "range('aa', 'cc')", "range(null, 2)", "range(low=1, high=3)", "range(...[1, 3])", "range(...{start: 1, end: 3})",
        "range(...{low: 1, high: 3})", "range(1, ...{step: 2, end: 5})", "range(1, 2, 3, 4)", "range(1, 2.5)",
        "range(' 1', '3')", "range('A', 'z', 10)", "range('5', 'a')", "range('a', 'c', 1.5)", "range(1, 3, step=2)",
    ])
    c += exprs([
        "cycle(['odd', 'even'], 5)", "cycle({5: 'x', 1: 'y'}, 1)", "cycle({5: 'x', 1: 'y'}, 0)", "cycle([1, 2], -1)",
        "cycle([1, 2], 1.5)", "cycle([1, 2], '1')", "cycle([1, 2], 'x')", "cycle('abc', 0)", "cycle([], 0)",
        "cycle([1, 2], null)", "cycle([1, 2], true)", "cycle(nul, 0)", "cycle(n, 0)", "cycle([1, 2], '5abc')",
        "cycle([1, 2, 3], 9223372036854775807)", "cycle(values=['a'], position=0)", "cycle(...[['a', 'b'], 1])",
        "cycle([1, 2], [])",
    ])
    c += exprs([
        "random(0)", "random(5, 5)", "random(-3, -3)", "random('a')", "random({a: 9})", "random('')", "random([])",
        "random(10, 5)", "random(2.9, 2.9)", "random(t)", "random(nul, 0)", "random([7])", "random('é')",
        "random(nul, -1)", "random(0, 'x')", "random(-0.5)", "random(f, 1)", "random({})", "random(values=[3])",
        "random([nul])", "random(false)", "random(1, 1.9)", "random(-1.5, -1)",
    ])
    return c


def tests():
    c = []
    values = ["list", "s", "nul", "map", "[]", "{}", "[1, 2]", "{0: 1, 1: 2}", "{1: 'a', 2: 'b'}", "{1: 'a', 0: 'b'}",
              "n", "f", "t", "''", "'0'", "0", "[0]", "false", "' '", "0.0", "undef", "{a: 1}"]
    for test in ["iterable", "empty", "sequence", "mapping"]:
        c += exprs(["%s is %s" % (v, test) for v in values])
    c += exprs(["1 is iterable(2)", "[] is empty(1)", "[] is sequence(1)", "[] is mapping(1)",
                "list is not iterable", "nul is not empty", "[] is empty(...{value: 1})",
                "[] is sequence(...{nope: 1})"])
    c += exprs(["undef is empty", "undef is iterable"], strict=True)
    return c


GEN = {"kind": "gen", "pairs": [["a", 1], ["b", 2], ["a", 3]]}
LIST_GEN = {"kind": "gen", "pairs": [[0, 1], [1, 2], [2, 3], [3, 4], [4, 5]]}
LOG = {"kind": "log_iter", "pairs": [["a", 1], ["b", 2], ["c", 3]]}
AGG = {"kind": "agg", "data": {"a": 1, "b": 2, "c": 5, "d": 8}}
AGG_GEN = {"kind": "agg_gen", "pairs": [["x", 1], ["y", 2], ["z", 3]]}
AO = {"kind": "array_object", "data": [1, 2, 3, 4]}
AO_MAP = {"kind": "array_object", "data": {"a": 1}}
AI = {"kind": "array_iterator", "data": {"p": 1, "q": 2, "r": 3}}
CA = {"kind": "count_access", "data": ["p", "q"]}
ACC = {"kind": "access", "data": ["p", "q"]}
AA = {"kind": "access_agg", "data": ["p", "q"]}
ST = {"kind": "str", "value": "héllo"}
MK = {"kind": "markup", "value": "<b>é</b>"}
PL = {"kind": "plain"}
OBJS = {"kind": "list", "items": [{"kind": "std", "data": {"id": 1, "name": "a"}},
                                  {"kind": "std", "data": {"id": 2, "name": "b"}},
                                  {"kind": "std", "data": {"id": 3}}]}
BAD = {"kind": "bytes", "hex": "61ff62"}


def objects():
    c = []

    def obj(tpl, **objs):
        return case(tpl, {"nul": None}, objects=objs)

    for e in ["gen|map((v, k) => k ~ v)", "gen|keys", "gen|length", "gen|reverse", "gen|sort", "gen|merge([9])",
              "gen|batch(2)", "gen|join(',')", "gen|first", "gen|last", "gen|reduce((c, v) => c + v, 0)",
              "gen|find(v => v > 1)", "gen|column('x')", "gen is empty", "gen is sequence", "gen is mapping",
              "gen is iterable", "gen|shuffle|length", "gen|reverse(true)", "gen|batch(2, 'f', false)",
              "gen|keys|length", "gen|map(v => v)|keys", "gen|merge(gen2)"]:
        c.append(obj(j(e), gen=GEN, gen2=LIST_GEN))
    for e in ["lg|slice(1, 2)", "lg|slice(1, 2, true)", "lg|slice(-2)", "lg|slice(10, 2)", "lg|slice(1, 0)",
              "lg|slice(1, -1)", "lg|slice(0, 1)", "lg|slice(3)", "lg|slice(null, 2)", "lg|slice('1', '1')",
              "lg|slice(1.5)", "lg|slice('x')", "lg|slice(0, 'x')", "lg|slice(1, 2, [])", "lg|slice(true, 1)",
              "lg|slice(1, 2, 'yes')", "lg[1:]", "lg[:2]"]:
        c.append(obj(j(e), lg=LIST_GEN))
    for tpl in ["{{ gen|first }}|{{ gen|first }}", "{{ gen|length }}|{{ gen|length }}",
                "{{ gen|first }}{% for v in gen %}{{ v }}{% endfor %}",
                "{% set f = gen|filter(v => v > 1) %}{{ f|length }}|{{ f|length }}",
                "{% set f = gen|filter(v => v > 1) %}" + each("f") + "|" + each("f"),
                each("gen|filter(v => v > 1)"), each("gen|filter((v, k) => k == 'a')"),
                "{{ gen|filter(v => v)|json_encode }}", "{{ gen|filter(v => v) is iterable ? 'yes' }}",
                "{{ gen|filter(v => v)|keys|json_encode }}", "{{ gen|filter(v => v)|first }}",
                "{{ gen|filter(v => v > 1)|last }}", "{{ gen|filter(v => v)|length }}|{{ gen|length }}",
                "{{ gen|filter(v => v)|map(v => v * 10)|json_encode }}"]:
        c.append(obj(tpl, gen=GEN))
    log = "|{{ lg.log|join(',') }}"
    for e in ["lg|first", "lg|last", "lg|slice(1, 1)", "lg|keys", "lg|length", "lg|map(v => v)", "lg is empty",
              "lg is sequence", "lg|find(v => v == 2)", "lg|slice(0, 2)", "lg|slice(5)", "lg|reverse", "lg|sort",
              "lg|merge([])", "lg|batch(2)", "lg|join(',')", "lg|column('x')", "lg|slice(1)", "lg|slice(2, 1)",
              "lg|reduce((c, v) => c + v)", "lg is mapping", "lg|slice(1, 0)", "lg|slice(-1)"]:
        c.append(obj(j(e) + log, lg=LOG))
    for tpl in [each("lg|filter(v => v > 1)") + log, "{{ lg|filter(v => v > 1)|first }}" + log,
                "{{ lg|filter(v => v == 3)|keys|json_encode }}" + log,
                "{% set f = lg|filter(v => v != 2) %}{{ f|length }}{{ f|length }}" + log,
                "{{ cycle(lg, 1) }}", "{{ lg|filter(v => false)|length }}" + log]:
        c.append(obj(tpl, lg=LOG))
    for e in ["agg|slice(1, 2)", "agg|slice(1, 2, true)", "agg|slice(4, 1)", "agg|slice(3, 2)", "agg|first",
              "agg|last", "agg|keys", "agg|length", "agg is empty", "agg is sequence", "agg is mapping",
              "agg|map(v => v * 2)", "agg|merge({e: 9})", "agg|reverse(true)", "agg|sort", "agg|column('x')",
              "agg|slice(0, 2)", "agg|slice(2)", "agg[1:1]", "agg|slice(1, 10, true)", "agg|batch(3, 0)",
              "agg|join('+')", "agg|reduce((c, v, k) => c ~ k, '')", "agg|find((v, k) => k == 'c')"]:
        c.append(obj(j(e), agg=AGG))
    c.append(obj(each("agg|filter(v => v > 3)"), agg=AGG))
    c.append(obj(each("agg|filter((v, k, it) => it.valid)"), agg=AGG))
    for e in ["ag|first", "ag|slice(1)", "ag|keys", "ag|length", "ag is sequence", "ag|last"]:
        c.append(obj(j(e), ag=AGG_GEN))
    c.append(obj("{{ ag|first }}{{ ag|first }}{{ ag|join }}", ag=AGG_GEN))
    c.append(obj(each("ag|filter(v => v != 2)"), ag=AGG_GEN))
    for e in ["ao|first", "ao|last", "ao|slice(1, 2)", "ao[4:1]", "ao[3:2]", "ao|length", "ao is sequence",
              "ao is mapping", "ao|keys", "ao|reverse", "ao|sort", "ao|merge([5])", "cycle(ao, 3)", "ao|join(',')",
              "ao is empty", "ao|batch(3)", "ao|map(v => v + 1)", "ao|slice(-1)", "ao|slice(1, 1, true)",
              "ao|column(0)", "random(ao1)", "ao1 is mapping", "ao1 is sequence", "ao|shuffle|length",
              "ao[0:0]", "ao|slice(5, 1)", "ao|slice(4)"]:
        c.append(obj(j(e), ao=AO, ao1=AO_MAP))
    c.append(obj(each("ao|filter(v => v > 2)"), ao=AO))
    for e in ["ai|slice(1)", "ai|keys", "ai|first", "ai|length", "ai is sequence", "ai|slice(1, 1, true)",
              "ai|slice(3)", "ai|reverse"]:
        c.append(obj(j(e), ai=AI))
    c.append(obj(each("ai|filter(v => v > 1)"), ai=AI))
    c.append(obj("{% for v in ai %}{{ v }}{% endfor %}" + each("ai|filter(v => v != 2)"), ai=AI))
    for e in ["cycle(ca, 3)", "cycle(acc, 1)", "cycle(aa, 1)", "cycle(ca, -1)", "cycle(aa, 5)"]:
        c.append(obj(j(e), ca=CA, acc=ACC, aa=AA))
    for e in ["st|length", "st|reverse", "st|first", "st|last", "st|slice(1, 2)", "st is empty", "st is mapping",
              "st is sequence", "st|join(',')", "st|shuffle", "random(st)", "[st]|join", "st|split('l')",
              "st|split('')", "st|keys", "st is iterable", "st|split('', 2)", "[st, 'x']|sort", "max(st, 'z')",
              "mk|length", "mk|reverse", "mk is empty", "mk|first", "mk|join", "mk|split('')"]:
        c.append(obj(j(e), st=ST, mk=MK))
    for e in ["objs|column('name')", "objs|column('name', 'id')", "objs|column(null, 'id')|keys",
              "objs|column('nope')", "objs|map(o => o.id)", "objs|filter(o => o.id > 1)|keys",
              "objs|sort((a, b) => b.id <=> a.id)|map(o => o.id)", "objs|find(o => o.name is not defined)"]:
        c.append(obj(j(e), objs=OBJS))
    for e in ["pl|length", "pl is empty", "pl is mapping", "pl|join", "pl|reverse", "pl|first", "random(pl)",
              "pl|shuffle", "pl|keys", "pl|map(v => v)", "pl is iterable", "pl is sequence", "[pl]|join",
              "pl|split(',')", "cycle(pl, 0)", "pl|batch(1)", "max(pl, 1)"]:
        c.append(obj(j(e), pl=PL))
    for e in ["cnt0 is empty", "cnt3|length", "cnt3 is empty", "cnt3 is mapping", "cnt3|keys"]:
        c.append(obj(j(e), cnt0={"kind": "countable", "n": 0}, cnt3={"kind": "countable", "n": 3}))
    for e in ["[1, 2]|map(inv)", "[1, 2]|filter(inv)", "[1, 2]|find(inv)", "[1, 2]|reduce(inv, 0)", "[3, 1, 2]|sort(cmp)",
              "[1, 2]|map([pl, 'nope'])", "[2, 1]|sort([pl, 'nope'])", "[1, 2]|filter([pl, 'nope'])",
              "[1]|map([inv, '__invoke'])", "[2, 1]|sort([cmp, '__invoke'])"]:
        c.append(obj(j(e), inv={"kind": "invoke"}, cmp={"kind": "cmp"}, pl=PL))
    c.append(obj(each("agg|filter(inv)"), agg=AGG, inv={"kind": "invoke"}))
    for e in ["bad|split('')", "bad|reverse", "bad|shuffle", "random(bad)", "bad|length", "bad|slice(1)",
              "bad|first", "bad|split('', 2)"]:
        c.append(obj(j(e), bad=BAD))
    odd = {"kind": "gen", "pairs": [[1.5, "a"], [None, "b"], [True, "c"], ["07", "d"], ["7", "e"]]}
    for e in ["odd|map(v => v)", "odd|keys", "odd|reverse", "odd|merge([])", "odd is sequence", "odd|slice(1, 3, true)",
              "odd|slice(0, 5)", "odd|sort", "odd|batch(2)", "odd|reduce((c, v, k) => c ~ k, '')", "odd|first",
              "odd|last", "odd|column(0)", "odd|filter(v => true)|keys"]:
        c.append(obj(j(e), odd=odd))
    bad_keys = {"kind": "gen", "pairs": [[[1], "a"], ["b", "b"]]}
    for e in ["bk|map(v => v)", "bk|keys", "bk|reverse", "bk|slice(0, 1, true)", "bk|slice(0, 1)", "bk is sequence"]:
        c.append(obj(j(e), bk=bad_keys))
    return c


def errors():
    c = []
    c.append(case("line1\n{{ s|batch(2) }}", DATA))
    c.append(case("{{ [1]|merge(\n  [2],\n  'x'\n) }}", DATA))
    c.append(case("{% for x in 1..3 %}{{ cycle(['a', 'b'], x) }}{% endfor %}", DATA))
    c.append(case("a\n\n{{ s|sort }}", DATA))
    c.append(case("{{ list|map(v =>\n  v.nope) }}", DATA, strict=True))
    c.append(case("{{ list|map(v => v ~ undef) }}", DATA, strict=True))
    c.append(case("{{ list|sort((a, b) => undef) }}", DATA, strict=True))
    c.append(case("{{ list|filter(v => undef) }}", DATA, strict=True))
    c.append(case("{{ list|reduce((c, v) => undef) }}", DATA, strict=True))
    c.append(case("{{ list|find(v => undef) }}", DATA, strict=True))
    c.append(case("{{ list|sort((a, b) => 1 // 0) }}", DATA))
    c.append(case("x\n{{ 'a'|split('') }}{{ max(\n1) }}", DATA))
    c.append(case("{{ [1, 2]|map(v => v / 0) }}", DATA))
    c.append(case("{% set a = [3, 1]|sort %}{% for k, v in a %}{{ k }}{{ v }}{% endfor %}", DATA))
    c.append(case("{{ random([]) }}", DATA))
    c.append(case("{{ [1, 2]|map(v => v)|join }}{{ cycle([], 1) }}", DATA))
    return c


def random_topic():
    c = []
    data = {"nul": None}
    for seed in [42, 1, 12345, 0]:
        for tpl in [
            "{{ random(100) }}|{{ random(['a', 'b', 'c']) }}|{{ random() }}",
            "{{ random() }}|{{ random(10) }}|{{ random(-10) }}|{{ random(5, 10) }}|{{ random(nul, 3) }}",
            "{{ [1, 2, 3, 4, 5]|shuffle|join(',') }}|{{ 'abcdef'|shuffle }}|{{ random('héllo') }}",
            "{{ random({a: 1, b: 2, c: 3}) }}|{{ random(1.9, 5.1) }}|{{ {x: 1, y: 2}|shuffle|join(',') }}",
            "{{ random(0, 9223372036854775807) }}|{{ random(-9223372036854775807 - 1, 9223372036854775807) }}",
            "{{ (1..20)|shuffle|join(',') }}|{{ random(1..1000) }}|{{ random(4294967295) }}",
            "{{ random(-4294967296, 4294967296) }}|{{ 'aé😀'|shuffle }}|{{ random({5: 'x', 7: 'y'}) }}",
        ]:
            c.append(case(tpl, data, seed=seed))
    return c


def sandbox():
    c = []

    def sb(tpl, filters=(), functions=(), properties=None, global_=True, objects=None):
        policy = {"tags": [], "filters": ["json_encode"] + list(filters), "methods": {},
                  "properties": properties or {}, "functions": list(functions)}
        return case(tpl, DATA, objects=objects, sandbox={"global": global_, "policy": policy})

    objs = {"objs": OBJS}
    c.append(sb(j("objs|column('name')"), ["column"], properties={"stdClass": ["name", "id"]}, objects=objs))
    c.append(sb(j("objs|column('name', 'id')"), ["column"], properties={"stdClass": ["name", "id"]}, objects=objs))
    c.append(sb(j("objs|column('name', 'id')"), ["column"], properties={"stdClass": ["name"]}, objects=objs))
    c.append(sb(j("objs|column('name')"), ["column"], objects=objs))
    c.append(sb(j("objs|column(1)"), ["column"], properties={"stdClass": ["name"]}, objects=objs))
    c.append(sb(j("objs|column(null, 'id')|keys"), ["column", "keys"], properties={"stdClass": ["id"]}, objects=objs))
    c.append(sb(j("rows|column('name')"), ["column"]))
    c.append(sb(j("objs|column('name')"), ["column"], global_=False, objects=objs))
    for e in ["[1, 2]|filter('nope')", "[1, 2]|map('nope')", "[2, 1]|sort('x')", "[1, 2]|find('x')",
              "[1, 2]|reduce('x')", "[1, 2]|map(v => v * 2)", "[2, 1]|sort((a, b) => a <=> b)"]:
        c.append(sb(j(e), ["filter", "map", "sort", "find", "reduce"]))
    c.append(sb(j("[1, 2]|map('nope')"), ["map"], global_=False))
    c.append(sb(j("max(1, 2)") + j("range(1, 2)"), functions=["max", "range"]))
    return c


def charset():
    c = []
    for cs, items in [
        ("ISO-8859-1", ["'é'|length", "'é'|reverse", "'abc'|reverse", "'é'|slice(1)", "'é'|first", "'é'|last",
                        "'aé'|split('', 2)|join('|')", "'é'|split('')|length", "'é'|shuffle|length", "'x'|shuffle",
                        "random('a')", "'é'|reverse|length", "'abc'|slice(-2)", "'aé'|split('', 1)|length",
                        "'abcd'|split('', 3)|join('|')", "st|length", "st|reverse", "'é'|reverse|reverse"]),
        ("UTF8", ["'é'|length", "'é'|reverse", "'éa'|split('', 1)|join('|')"]),
        ("utf-8", ["'é'|length", "'é'|first", "'é'|shuffle"]),
        ("NOPE-1", ["'abc'|reverse", "'abc'|shuffle", "'a'|split('')|join", "[1]|length"]),
        ("NOPE-1", ["'abc'|length"]),
        ("NOPE-1", ["'abc'|slice(1)"]),
        ("NOPE-1", ["'abc'|split('', 2)"]),
        ("NOPE-1", ["'abc'|first"]),
    ]:
        tpl = "".join("[{{ %s }}]" % e for e in items)
        c.append(case(tpl, {"nul": None}, objects={"st": ST}, config={"charset": cs}))
    return c


TOPICS = {
    "charset": charset,
    "sandbox": sandbox,
    "filters": filters,
    "functions": functions,
    "tests": tests,
    "objects": objects,
    "errors": errors,
    "random": random_topic,
}


if __name__ == "__main__":
    args = sys.argv[1:]
    oracle = "--oracle" in args
    names = [a for a in args if not a.startswith("--")] or list(TOPICS)
    for name in names:
        write(name, TOPICS[name](), oracle)
