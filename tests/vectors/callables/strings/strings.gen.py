"""Writes the oracle batches tests/vectors/callables/strings/<topic>.gen.json of the string and escaping callables:
templates rendered by a plain Twig 3.31.1 environment, with their outputs, error messages and PHP diagnostics.

- escape: the `escape`/`e` filters called explicitly (`EscaperRuntime::escape()`) and `raw`;
- autoescape: auto-escaping with the `html`, `js`, `name` and `false` default strategies, `{% autoescape %}`,
  `Markup` safety, safe filters and the `pre_escape` filters;
- case: `upper`, `lower`, `title`, `capitalize`;
- text: `trim`, `nl2br`, `spaceless`, `striptags`, `replace`, `format`, `url_encode`, `json_encode`,
  `convert_encoding`;
- split: `split`, `join`;
- runtime: custom escapers and safe classes registered on the environment's `EscaperRuntime` (the `setup` of a
  case); run by the unit tests of src/extension/runtime/strings/escaper/tests.rs.

tests/callables_strings.rs runs the first five topics. Object cases build the PHP classes below in `data_php`; the
Rust tests build the same objects. A case with a `twig_rs` field documents a deliberate deviation
(reference/deviations.md): the Rust test expects that result instead of the oracle's.

Regenerate from the repository root (Git Bash on Windows needs MSYS_NO_PATHCONV=1):

    python -I tests/vectors/callables/strings/strings.gen.py [topic ...]            # writes <topic>.gen.json
    python -I tests/vectors/callables/strings/strings.gen.py --oracle [topic ...]   # and runs them: <topic>.json

`--oracle` pipes each batch through `docker run --rm -i twig-oracle:3.31.1 php runner.php --pretty`.
"""

import json
import os
import re
import subprocess
import sys

OUT = os.path.dirname(os.path.abspath(__file__))

BASE_CONFIG = {"autoescape": False, "strict_variables": False}

PHP_CLASSES = r"""
if (!class_exists('TwigRsStr', false)) {
    interface TwigRsSafeIface {}
    class TwigRsStr implements TwigRsSafeIface {
        public function __construct(private string $s) {}
        public function __toString(): string { return $this->s; }
    }
    class TwigRsChild extends TwigRsStr {}
    class TwigRsCounter {
        private $n = 0;
        public function __toString(): string { $this->n++; return 'x' . $this->n . 'Y'; }
    }
    class TwigRsPlain { public $p = 'v'; }
    class TwigRsThrowStr {
        public function __toString(): string { throw new \RuntimeException('no string'); }
    }
    class TwigRsJson implements \JsonSerializable {
        public function jsonSerialize(): mixed { return ['j' => '<x>']; }
    }
}
"""

# object name -> PHP expression (the Rust tests build the same objects)
PHP_OBJECTS = {
    "str": "new TwigRsStr('<b>Str</b> é')",
    "child": "new TwigRsChild('<i>child</i>')",
    "counter": "new TwigRsCounter()",
    "plain": "new TwigRsPlain()",
    "thrower": "new TwigRsThrowStr()",
    "json": "new TwigRsJson()",
    "iter": "new \\ArrayIterator(['a' => '<x>', 'b' => 'y', 0 => 'z'])",
    "arrobj": "new \\ArrayObject(['k' => 'v', 1 => 'w'])",
    "markup": "new \\Twig\\Markup('<b>m</b>', 'UTF-8')",
    "markup_html": "new \\Twig\\Markup('<b>h</b>', 'UTF-8', ['html'])",
    "markup_js": "new \\Twig\\Markup('<b>j</b>', 'UTF-8', ['js'])",
    "markup_attr": "new \\Twig\\Markup('<b a=\"1\">a</b>', 'UTF-8', ['html_attr'])",
    "markup_none": "new \\Twig\\Markup('<b>n</b>', 'UTF-8', [])",
    "markup_empty": "new \\Twig\\Markup('', 'UTF-8')",
    "markup_ws": "new \\Twig\\Markup(\"  <b>w</b>\\n \", 'UTF-8')",
}

DATA = {
    "s": "<a href='x'>T&amp; \"q\"</a>",
    "u": "élan VITAL ǆ ß straße ﬁ İSTANBUL ΣΑΣ",
    "n": None,
    "i": 42,
    "neg": -7,
    "f": 1.5,
    "big": 1e25,
    "t": True,
    "fa": False,
    "arr": [1, "<b>", None],
    "nested": [[1], "x"],
    "map": {"a": "<1>", "b": 2},
    "empty": "",
    "zero": "0",
    "nl": "a\nb\r\nc\rd\n\re",
    "html": "  <div>\n  <b> x </b>  </div>  ",
}

# PHP custom escapers of the runtime topic (the unit tests have Rust versions)
PHP_ESCAPERS = {
    "star": "fn ($s, $c) => $s . '**' . $c",
    "type": "fn ($s, $c) => get_debug_type($s)",
    "wrap": "fn ($s, $c) => [$s]",
}

def used_objects(templates):
    """The objects a case's templates mention, in PHP_OBJECTS order."""
    text = " ".join(templates.values())
    return tuple(name for name in PHP_OBJECTS if re.search(r"(?<![\w.'])%s(?![\w'])" % name, text))


def case(tpl, data=None, objects=None, config=None, templates=None, main=None, errors="collect",
         setup=None, twig_rs=None):
    templates = templates or {"index.twig": tpl}
    if objects is None:
        objects = used_objects(templates)
    cfg = dict(BASE_CONFIG)
    if config:
        cfg.update(config)
    c = {"templates": templates, "env": "plain", "config": cfg, "errors": errors}
    if main:
        c["main"] = main
    data = DATA if data is None else data
    statements = []
    if setup:
        c["setup"] = setup
        statements.append("$r = $twig->getRuntime(\\Twig\\Runtime\\EscaperRuntime::class);")
        for cls, strategies in setup.get("safe_classes", []):
            statements.append("$r->addSafeClass(%s, %s);" % (json.dumps(cls), json.dumps(strategies)))
        for name in setup.get("escapers", []):
            statements.append("$r->setEscaper(%s, %s);" % (json.dumps(name), PHP_ESCAPERS[name]))
    if objects or statements:
        parts = ["'%s' => %s" % (name, PHP_OBJECTS[name]) for name in objects]
        c["data_php"] = PHP_CLASSES + "\n".join(statements) + "\nreturn array_merge(json_decode(%s, true), [%s]);" % (
            json.dumps(json.dumps(data)), ", ".join(parts))
        c["objects"] = list(objects)
        c["data_json_rust"] = data
    else:
        c["data_json"] = data
    if twig_rs:
        c["twig_rs"] = twig_rs
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
    failures = [r for r in results if r.get("phase") in ("fatal", "lost", "oracle", "setup", "data")]
    print(topic, len(cases), "cases", "bad:" if failures else "", failures[:1])


def wrapped(message):
    """The message of a non-Twig exception thrown while rendering index.twig at line 1."""
    return 'An exception has been thrown during the rendering of a template ("%s") in "index.twig" at line 1.' % message


def escape():
    c = []
    for tpl in [
        # strategies
        "{{ s|e }}|{{ s|escape }}|{{ s|e('html') }}|{{ s|escape('html') }}",
        "{{ s|e('js') }}", "{{ s|e('css') }}", "{{ s|e('url') }}", "{{ s|e('html_attr') }}",
        "{{ s|e('html_attr_relaxed') }}",
        "{{ u|e('js') }}|{{ u|e('css') }}|{{ u|e('url') }}|{{ u|e('html_attr') }}|{{ u|e('html_attr_relaxed') }}",
        "{{ \"\\x00\\x01\\x08\\t\\n\\x0b\\x0c\\r\\x1f\\x7f\"|e('js') }}",
        "{{ \"\\x00\\x01\\x08\\t\\n\\x0b\\x0c\\r\\x1f\\x7f\"|e('html_attr') }}|{{ \"\\x00\\t\\x7f\"|e('css') }}",
        "{{ \"\\xc2\\x80\\xc2\\x9f\\xc2\\xa0\\xe2\\x80\\xa8\\xf0\\x9f\\x98\\x80\"|e('html_attr') }}",
        "{{ \"\\xc2\\x80\\xc2\\x9f\\xe2\\x80\\xa8\\xf0\\x9f\\x98\\x80\"|e('js') }}|{{ \"\\xf0\\x9f\\x98\\x80\"|e('css') }}",
        "{{ '-_.~!*()'|e('url') }}|{{ ',.-_:@[]'|e('html_attr') }}|{{ ',.-_:@[]'|e('html_attr_relaxed') }}",
        "{{ ',._'|e('js') }}|{{ 'aZ09'|e('css') }}|{{ '&amp;'|e }}",
        # invalid UTF-8
        "{{ \"a\\xffb\\xc3\"|e }}", "{{ \"a\\xc0\\xafb\\xed\\xa0\\x80c\\xf4\\x90\\x80\\x80\"|e }}",
        "{{ \"a\\xffb\"|e('url') }}", "{{ \"a\\xffb\"|e('js') }}", "{{ \"a\\xffb\"|e('css') }}",
        "{{ \"a\\xffb\"|e('html_attr') }}", "{{ \"a\\xffb\"|e('html_attr_relaxed') }}",
        # values that are not strings
        "{{ i|e }}|{{ f|e }}|{{ big|e }}|{{ t|e }}|{{ fa|e }}|{{ n|e }}|{{ neg|e('js') }}|{{ f|e('url') }}",
        "{{ i|e('nope') }}", "{{ n|e('nope') }}", "{{ arr|e }}", "{{ arr|e('js')|join(',') }}", "{{ arr|e('nope') }}",
        "{{ empty|e('nope') }}|{{ zero|e('nope') }}", "{{ ''|e('html', 'nope') }}",
        "{{ str|e }}|{{ str|e('js') }}|{{ str|e('url') }}", "{{ child|e }}", "{{ str|e('nope') }}",
        "{{ plain|e }}", "{{ plain|e('js').p }}", "{{ plain|e('nope') }}", "{{ thrower|e }}", "{{ thrower|e('nope') }}",
        "{{ iter|e('html')|join(',') }}", "{{ iter|e }}", "{{ counter|e }}{{ counter|e }}",
        # Markup and the autoescape argument
        "{{ markup|e }}|{{ markup|e('html', null, true) }}|{{ markup|e('js', null, true) }}",
        "{{ markup_html|e('html', null, true) }}|{{ markup_html|e('js', null, true) }}|{{ markup_html|e('url', null, true) }}",
        "{{ markup_attr|e('html', null, true) }}|{{ markup_attr|e('html_attr_relaxed', null, true) }}|{{ markup_attr|e('css', null, true) }}",
        "{{ markup_none|e('html', null, true) }}|{{ markup_none|e('html') }}",
        "{{ markup_js|e('js', null, true) }}|{{ markup_js|e('html', null, true) }}",
        "{{ markup_empty|e('js', null, true) }}|{{ markup_empty|e }}|{{ markup_empty|e('nope') }}",
        "{{ markup_none|e('nope', null, true) }}", "{{ markup|e('nope', null, true) }}",
        "{{ str|e('html', null, true) }}|{{ child|e('js', null, true) }}",
        "{{ i|e('html', null, true) }}|{{ arr|e('html', null, true)|length }}",
        # charsets
        "{{ '<é>'|e('html', 'UTF-8') }}|{{ '<'|e('html', 'utf-8') }}|{{ '<'|e('html', '0') }}|{{ '<'|e('html', '') }}",
        "{{ \"\\xe9<\"|e('html', 'ISO-8859-1') }}|{{ \"\\xe9<\"|e('html', 'iso-8859-1')|e('url') }}",
        "{{ \"\\xe9<\"|e('js', 'ISO-8859-1') }}|{{ \"\\xe9<\"|e('css', 'ISO-8859-1') }}",
        "{{ \"\\xe9<\"|e('html_attr', 'ISO-8859-1') }}|{{ \"\\xe9<\"|e('url', 'ISO-8859-1') }}",
        "{{ \"\\xe9<\"|e('html', 'windows-1252')|e('url') }}|{{ \"\\xe9<\"|e('html', 'CP1252')|e('url') }}",
        "{{ '<'|e('html', 'KOI8-R') }}|{{ '<é>'|e('html', 'UTF8') }}|{{ '<é>'|e('js', 'utf-8') }}",
        "{{ '<é>'|e('html', 'x-unknown') }}", "{{ '<é>'|e('js', 'x-unknown') }}",
        "{{ 'é'|e('html', 'ISO-8859-2')|e('url') }}", "{{ '€<'|e('html', 'ISO-8859-1') }}",
        "{{ 'é'|e('js', 'ASCII') }}", "{{ 'a<'|e('html', 'UTF-16BE')|e('url') }}",
        # arguments
        "{{ '<'|e(strategy='js') }}|{{ '<'|e(charset='UTF-8') }}|{{ '<'|e('html', null, 0) }}",
        "{{ '<'|e(autoescape=true) }}|{{ markup|e(autoescape=1) }}|{{ markup|e('js', null, 'x') }}",
        "{{ '<'|e('js', 'UTF-8', false, 'extra', 5) }}", "{{ '<'|e(...['js']) }}|{{ '<'|e(...{strategy: 'url'}) }}",
        "{{ '<'|e(...{nope: 1}) }}", "{{ '<'|e('js', ...{strategy: 'url'}) }}", "{{ '<'|e(1) }}", "{{ '<'|e(1.5) }}",
        "{{ '<'|e(true) }}", "{{ '<'|e(null) }}", "{{ '<'|e(arr) }}", "{{ '<'|e('html', arr) }}",
        "{{ '<'|e('html', 5)|e('url') }}", "{{ '<'|e('html', null, arr) }}", "{{ '<'|e('html', null, null) }}",
        "{{ '<'|e('html', null, 'yes') }}|{{ markup|e('js', null, 0.0) }}",
        "{{ '<'|e(strateg='js') }}", "{{ '<'|escape(strategy=s) }}",
        # raw
        "{{ s|raw }}|{{ s|raw(1, 2) }}|{{ arr|raw|join(',') }}|{{ markup|raw }}",
        "{% set x = s|e %}{{ x|e }}",
    ]:
        c.append(case(tpl))
    c.append(case("{{ '<'|e }}", errors="throw"))
    c.append(case("{{ arr|e }}", errors="throw"))
    c.append(case("{{ '<é>'|e('html', 'x-unknown') }}", errors="throw"))
    c.append(case("{{ s|e }}", config={"charset": "ISO-8859-1"}))
    return c


def autoescape():
    c = []
    for strategy in ("html", "js", "css", "url", "html_attr", False):
        for tpl in [
            "{{ s }}|{{ s|raw }}|{{ s|e }}|{{ s|e('js') }}|{{ s|upper }}|{{ s|e|upper }}",
            "{{ markup }}|{{ markup_html }}|{{ markup_js }}|{{ markup_attr }}|{{ markup_none }}|{{ markup_empty }}",
            "{{ str }}|{{ child }}|{{ i }}|{{ f }}|{{ t }}|{{ n }}|{{ 'lit<' }}|{{ 'a' ~ s }}",
            "{{ s|nl2br }}|{{ markup_ws|nl2br }}|{{ s|raw|nl2br }}|{{ nl|nl2br }}|{{ 'x<\ny'|nl2br }}",
            "{{ html|spaceless }}|{{ s|spaceless }}|{{ s|raw|spaceless }}",
            "{{ markup_ws|trim }}|{{ markup_ws|trim('<') }}|{{ markup_ws|trim(side='left') }}",
            "{{ (t ? s : 'lit') }}|{{ (fa ? 'lit' : s) }}|{{ n ?? s }}|{{ s ?: 'x' }}|{{ s ?? 'x' }}",
            "{% set x %}<b>{{ s }}</b>{% endset %}{{ x }}|{{ x|upper }}|{{ x|raw }}",
            "{% autoescape 'js' %}{{ s }}|{{ markup_html }}|{{ markup_js }}{% endautoescape %}",
            "{% autoescape 'css' %}{{ s }}{% endautoescape %}|{% autoescape false %}{{ s }}{% endautoescape %}",
            "{% autoescape %}{{ s }}{% endautoescape %}|{% autoescape 'html_attr' %}{{ s }}|{{ markup_attr }}{% endautoescape %}",
            "{{ arr }}", "{{ plain }}", "{{ s|e('nope') }}", "{{ s|join }}|{{ s|title }}|{{ s|format }}",
            "{{ s|e('html')|e('html') }}|{{ s|e(strategy='html') }}|{% set st = 'html' %}{{ s|e(st) }}",
            "{{ s|default('x') }}|{{ undef|default(s) }}|{{ s|replace({'<': '['}) }}|{{ s|striptags }}",
        ]:
            c.append(case(tpl, config={"autoescape": strategy}))
    c.append(case("{% autoescape 'nope' %}{{ s }}{% endautoescape %}", config={"autoescape": "html"}))
    c.append(case("{% autoescape 'nope' %}{{ 'lit' }}{{ empty }}{% endautoescape %}", config={"autoescape": "html"}))
    for name in ["x.js.twig", "x.json.twig", "x.css", "x.css.twig", "x.txt", "x.txt.twig", "x.html", "x.twig",
                 "dir/", "dir\\", "noext", "a.b/c", "x.JS", ".js", "x.twig.js"]:
        c.append(case(None, templates={name: "{{ s }}|{{ markup_js }}|{{ s|nl2br }}"}, main=name,
                      config={"autoescape": "name"}))
    c.append(case("{{ s }}", config={"autoescape": "html", "charset": "ISO-8859-1"}))
    c.append(case("{{ \"\\xe9<\" }}", config={"autoescape": "js", "charset": "ISO-8859-1"}))
    return c


def case_filters():
    c = []
    for tpl in [
        "{{ u|upper }}|{{ u|lower }}|{{ u|title }}|{{ u|capitalize }}",
        "{{ 'hello wORLD o\\'neil mary-jane ǆemal 3rd x_y a.b ÉCOLE ﬁsh'|title }}",
        "{{ 'hello 2nd world_wide-web o\\'reilly ŉ ǳ ß'|title }}|{{ 'ǳ ǆa'|capitalize }}|{{ 'ßa BC'|capitalize }}",
        "{{ 'my first car'|capitalize }}|{{ ''|capitalize }}|{{ ''|title }}|{{ ''|upper }}",
        "{{ \"a\\xffb\\xc3\"|upper }}|{{ \"a\\xffB\\xc3\"|lower }}|{{ \"a\\xffb c\"|title }}|{{ \"\\xffAB\"|capitalize }}",
        "{{ \"a\\xe2\\x82b\\xc0\\xafc\\xed\\xa0\\x80d\\xf4\\x90\\x80\\x80e\\xf0\\x9f\\x98\"|upper }}",
        "{{ \"\\xc3\\xa9\\xff\"|capitalize }}|{{ \"\\xe2\\x82\"|capitalize }}",
        "{{ i|upper }}|{{ f|lower }}|{{ big|title }}|{{ t|capitalize }}|{{ fa|upper }}|{{ n|upper }}|{{ n|title }}",
        "{{ neg|capitalize }}|{{ n|capitalize }}|{{ n|lower }}",
        "{{ arr|upper }}", "{{ arr|lower }}", "{{ arr|title }}", "{{ arr|capitalize }}",
        "{{ str|upper }}|{{ str|lower }}|{{ str|title }}|{{ str|capitalize }}",
        "{{ plain|upper }}", "{{ plain|title }}", "{{ plain|capitalize }}", "{{ thrower|upper }}",
        "{{ thrower|capitalize }}", "{{ counter|upper }}|{{ counter|capitalize }}|{{ counter|title }}",
        "{{ markup|upper }}|{{ markup|capitalize }}|{{ iter|first }}",
        "{{ iter|upper }}", "{{ json|lower }}",
        "{{ 'a'|upper(1) }}|{{ 'a'|lower(1, 2) }}|{{ 'a b'|title(1) }}|{{ 'ab'|capitalize(1) }}",
        "{{ 'a'|upper(...{string: 'q'}) }}", "{{ 'a'|title(...{x: 1}) }}", "{{ 'a'|lower(...[1, 2]) }}",
        "{{ 'a'|capitalize(...{charset: 'x'}) }}", "{{ 'a'|upper(x=1) }}", "{{ 'a'|title(string='b') }}",
        "{{ 'ΣΑΣ ΣΑΣ.'|lower }}|{{ 'ΑΣ'|lower }}|{{ 'Σ'|lower }}|{{ 'aΣ b'|lower }}",
        "{{ 'ﬀ ﬃ ŉ'|upper }}|{{ 'İ'|lower|length }}|{{ 'ﬀ'|capitalize }}|{{ 'ǅ'|title }}",
    ]:
        c.append(case(tpl))
    for charset in ("NOPE", "utf8", "x\u00e9"):
        c.append(case("{{ 'a'|upper }}", config={"charset": charset}))
        c.append(case("{{ 'a'|lower }}", config={"charset": charset}))
        c.append(case("{{ 'a'|title }}", config={"charset": charset}))
        c.append(case("{{ 'a'|capitalize }}", config={"charset": charset}))
        c.append(case("{{ arr|capitalize }}", config={"charset": charset}))
    # single-byte charsets whose bytes are their code points (the case is converted in Unicode, `?` for what does not
    # fit) and ASCII (bytes from 0x80 are invalid)
    latin = "\\xe9t\\xe9 \\xdf\\xff\\xb5 x\\xc9"
    for charset in ("ISO-8859-1", "latin1", "8bit", "ASCII", "7bit"):
        c.append(case("{{ \"%s\"|upper|url_encode }}|{{ \"%s\"|lower|url_encode }}|{{ \"%s\"|title|url_encode }}|"
                      "{{ \"%s\"|capitalize|url_encode }}|{{ 'ab'|upper }}" % (latin, latin, latin, latin),
                      config={"charset": charset}))
    for charset, tpl, function in (("Windows-1252", "{{ 'a'|capitalize }}", "mb_strtoupper"),
                                   ("KOI8-R", "{{ 'a'|title }}", "mb_convert_case"),
                                   ("SJIS", "{{ 'a'|lower }}", "mb_strtolower"),
                                   ("UTF-16", "{{ 'a'|capitalize }}", "mb_substr")):
        c.append(case(tpl, config={"charset": charset}, twig_rs={"error": "Twig\\Error\\RuntimeError: " + wrapped(
            '%s(): encoding "%s" is not supported by twig-rs' % (function, charset.upper()))}))
    c.append(case("{{ 'a'|upper }}", config={"charset": "BASE64"}, twig_rs={"error": "Twig\\Error\\RuntimeError: " + wrapped(
        'mb_strtoupper(): encoding "BASE64" is not supported by twig-rs')}))
    return c


def text():
    c = []
    for tpl in [
        # trim
        "[{{ '  a  '|trim }}]|[{{ 'xxaxx'|trim('x') }}]|[{{ '  a  '|trim(side='left') }}]|[{{ '  a  '|trim(' ', 'right') }}]",
        "[{{ 'abcxcba'|trim('a..c') }}]|[{{ 12.50|trim('0') }}]|[{{ \"\\x0ca\\x0b\\x00\"|trim }}]|[{{ n|trim }}]",
        "[{{ i|trim('4') }}]|[{{ t|trim }}]|[{{ fa|trim }}]|[{{ '  a  '|trim(null, 'left') }}]",
        "[{{ 'a..b'|trim('..a') }}]", "[{{ 'ab'|trim('a..') }}]", "[{{ 'ab'|trim('z..a') }}]", "[{{ 'ab'|trim('..') }}]",
        "[{{ 'a.b'|trim('a.') }}]|[{{ 'a..b'|trim('a..b..') }}]",
        "[{{ ' x '|trim(side=true) }}]", "[{{ ' x '|trim(side=null) }}]", "[{{ arr|trim(side='foo') }}]",
        "[{{ arr|trim }}]", "[{{ arr|trim(side='left') }}]", "[{{ arr|trim(side='right') }}]",
        "[{{ 'x0x'|trim(0) }}]", "[{{ 'x'|trim(arr) }}]", "[{{ ' x '|trim(...{side: 'left'}) }}]",
        "[{{ str|trim('<>') }}]", "[{{ plain|trim }}]", "[{{ thrower|trim(side='x') }}]",
        "[{{ markup_ws|trim|e('html', null, true) }}]|[{{ markup_ws|trim('<')|e('html', null, true) }}]",
        "[{{ markup_ws|trim(\" \\t\\n\\r\\0\\x0B\")|e('html', null, true) }}]|[{{ markup_ws|trim(side='right')|e('html', null, true) }}]",
        "[{{ markup_ws|trim(null, 'left')|e('js', null, true) }}]|[{{ markup_none|trim|e('html', null, true) }}]",
        # nl2br
        "{{ nl|nl2br }}", "{{ 'a\\r\\nb\\n\\rc\\r\\rd'|nl2br }}|{{ n|nl2br }}|{{ i|nl2br }}|{{ ''|nl2br }}",
        "{{ arr|nl2br }}", "{{ str|nl2br }}|{{ markup_ws|nl2br }}", "{{ plain|nl2br }}", "{{ s|nl2br }}",
        "{% set x = s|nl2br %}{{ x }}", "{{ '<a>\\n'|nl2br }}",
        # spaceless
        "{{ html|spaceless }}", "[{{ \">\\x0b\\x0c<x> \\t\\r\\n <y>  \"|spaceless }}]", "[{{ '> <> <'|spaceless }}]",
        "[{{ '>  a <'|spaceless }}]|[{{ '><'|spaceless }}]|[{{ '>'|spaceless }}]|[{{ '   '|spaceless }}]",
        "[{{ n|spaceless }}]|[{{ i|spaceless }}]|[{{ f|spaceless }}]|[{{ t|spaceless }}]",
        "{{ arr|spaceless }}", "{{ nested|spaceless }}", "{{ [plain]|spaceless }}", "{{ [thrower]|spaceless }}",
        "{{ str|spaceless }}", "{{ plain|spaceless }}", "{{ thrower|spaceless }}", "{{ markup_ws|spaceless }}",
        "{{ s|spaceless }}", "{{ \"a>\\xff\\x20<b\"|spaceless|e('url') }}",
        # striptags
        "{{ '<p>Hello <b>World</b></p><br/><!-- c --> &amp; <?php x ?> a < b'|striptags }}",
        "{{ '<p>Hello <b>World</b></p><br/>'|striptags('<b><br>') }}|{{ '<p>a<b>b</b></p>'|striptags(['b', 'p']) }}",
        "{{ 'a  \\n <p>b</p>'|striptags }}|{{ 'x<a href=\\'>\\'>y</a>z'|striptags }}|{{ '1 <2 3>4'|striptags }}",
        "{{ 'a < b <br>c'|striptags }}|{{ 'a < b <br>c'|striptags('<br>') }}|{{ n|striptags }}|{{ i|striptags }}",
        "{{ '<b>x</b>'|striptags(null) }}|{{ '<b>x</b><i>y</i>'|striptags([]) }}|{{ '<1>x</1>'|striptags(1) }}",
        "{{ arr|striptags }}", "{{ '<b>x</b>'|striptags(plain) }}", "{{ '<b>x</b>'|striptags([[1], 'b']) }}",
        "{{ str|striptags }}|{{ markup|striptags }}|{{ '<b>x</b>'|striptags(str) }}",
        "{{ '<!DOCTYPE html><html><body>t</body></html>'|striptags }}|{{ '<?xml version=\"1.0\"?>x'|striptags }}",
        # replace
        "{{ 'I like %this% and %that%.'|replace({'%this%': 'apples', '%that%': 'oranges'}) }}",
        "{{ 'abcabc'|replace({'a': '1', 'ab': '2', 'abc': '3'}) }}|{{ '0123'|replace(['x', 'y']) }}",
        "{{ 'abc'|replace({'a': 1.5, 'b': true, 'c': null}) }}|{{ 'abc'|replace({}) }}|{{ ''|replace({'': 'x'}) }}",
        "{{ 'abc'|replace({'': 'x'}) }}", "{{ 'abc'|replace({'': 'x', 'b': 'y'}) }}",
        "{{ 'abc'|replace({'a': arr}) }}", "{{ 'abc'|replace({'a': plain}) }}", "{{ 'abc'|replace({'a': str}) }}",
        "{{ 'abc'|replace('a') }}", "{{ 'abc'|replace(n) }}", "{{ 'abc'|replace(i) }}", "{{ 'abc'|replace(str) }}",
        "{{ 'a<x>b'|replace(iter) }}|{{ 'kv'|replace(arrobj) }}", "{{ n|replace({'': 'x'}) }}|{{ i|replace({4: 'four'}) }}",
        "{{ arr|replace({'a': 'b'}) }}", "{{ str|replace({'<b>': '['}) }}", "{{ plain|replace({}) }}",
        "{{ 'abc'|replace({1: 'x', 'b': 'y', 'bc': 'z'}) }}|{{ 'aaa'|replace({'a': 'b', 'aa': 'c'}) }}",
        "{{ 'x'|replace(...[]) }}", "{{ 'a'|replace(...{from: {'a': 'b'}}) }}",
        # format
        "{{ '%s|%d|%05.2f|%x|%X|%o|%b|%c|%e|%E|%g|%G|%u|%%|%\\'*10s|%-5s|%+d|%5.1s|%1$s'|format('ab', 42.9, 3.14159, 255, 255, 8, 5, 65, 1234.5678, 0.00012, 0.00001234, 1e20, -1, 'x', 'ab', 7, 'xyz') }}",
        "{{ n|format('x') }}|{{ i|format }}|{{ '%s'|format(arr) }}|{{ '%s|%d|%s'|format(t, t, fa) }}",
        "{{ '%s %s'|format('a') }}", "{{ '%y'|format(1) }}", "{{ '%'|format }}", "{{ '%0$s'|format(1) }}",
        "{{ '%.60f'|format(1) }}", "{{ arr|format }}", "{{ str|format }}|{{ '%s'|format(str) }}",
        "{{ '%s'|format(plain) }}", "{{ '%d'|format('12abc') }}|{{ '%.2f'|format('1e3') }}|{{ '%d'|format(n) }}",
        "{{ '%s'|format(...{a: 1}) }}", "{{ '%s'|format(...{format: 1}) }}", "{{ '%s-%s'|format(...['a', 'b']) }}",
        "{{ '%s-%s'|format('a', values='b') }}", "{{ '%s'|format(values='b') }}", "{{ '%s'|format(format='x') }}",
        "{{ '%*d'|format(5, 42) }}|{{ '%-*d|'|format(4, 7) }}|{{ '%.*f'|format(2, 3.14159) }}",
        "{{ '%2$s %1$s'|format('a', 'b') }}|{{ '%f'|format(1e999) }}|{{ '%d'|format(1e999) }}",
        # url_encode
        "{{ 'path-seg*ment ~é'|url_encode }}|{{ '-_.~!*\\'();:@&=+$,/?#[]'|url_encode }}|{{ \"a\\xffb\"|url_encode }}",
        "{{ {name: 'Fabien', 'a b': 'c&d', arr: [1, 2], n: null, t: true, f: false, nested: {x: 'y'}}|url_encode }}",
        "{{ {a: 0.1 + 0.2, b: 1e25, c: 1.0, d: -0.0}|url_encode }}|{{ [5, 6]|url_encode }}|{{ []|url_encode }}",
        "{{ t|url_encode }}|{{ fa|url_encode }}|{{ n|url_encode }}|{{ i|url_encode }}|{{ f|url_encode }}",
        "{{ str|url_encode }}", "{{ plain|url_encode }}", "{{ iter|url_encode }}", "{{ markup|url_encode }}",
        "{{ {a: str, b: [str]}|url_encode }}", "{{ {(1): 'x', 'é': 'y'}|url_encode }}",
        # json_encode
        "{{ {a: 'é/<>&\\'\"', b: [1, 1.0, 0.1, true, null], c: {}, d: []}|json_encode }}",
        "{{ {0: 'a', 1: 'b'}|json_encode }}|{{ {1: 'a', 2: 'b'}|json_encode }}|{{ markup|json_encode }}",
        "{{ str|json_encode }}|{{ plain|json_encode }}|{{ json|json_encode }}|{{ iter|json_encode }}|{{ arrobj|json_encode }}",
        "{{ \"\\x00\\x01\\x1f\\x7f\\t\\n\\r\\x08\\x0c\\\"\\\\/\\xc3\\xa9\\xf0\\x9f\\x98\\x80\\xe2\\x80\\xa8\"|json_encode }}",
        "[{{ \"a\\xffb\"|json_encode }}]|[{{ 1e999|json_encode }}]|[{{ [[[1]]]|json_encode(0, 2) }}]",
        "{{ {a: [1, {}]}|json_encode(128) }}|{{ '/é'|json_encode(64 + 256) }}|{{ '<&\\'\"'|json_encode(1 + 2 + 4 + 8) }}",
        "{{ [1]|json_encode(16) }}|{{ 1.0|json_encode(1024) }}|{{ \"a\\xffb\"|json_encode(512) }}",
        "{{ \"a\\xffb\"|json_encode(4194304) }}", "{{ 'x'|json_encode(0, 512, 3) }}", "{{ 'x'|json_encode(flags='y') }}",
        "{{ [[1]]|json_encode(depth=1) }}|{{ [[1]]|json_encode(depth=2) }}", "{{ 'x'|json_encode(options=1) }}",
        "{{ [[1]]|json_encode(...{depth: 1}) }}|{{ 'é'|json_encode(...[256]) }}", "{{ 'x'|json_encode(...{nope: 1}) }}",
        "{{ i|json_encode }}|{{ f|json_encode }}|{{ big|json_encode }}|{{ n|json_encode }}|{{ t|json_encode }}",
        # convert_encoding
        "{{ 'é'|convert_encoding('ISO-8859-1', 'UTF-8')|url_encode }}|{{ 'é'|convert_encoding('UTF-16BE', 'UTF-8')|url_encode }}",
        "[{{ 'é'|convert_encoding('NOPE', 'UTF-8') }}]", "[{{ \"a\\xffb\"|convert_encoding('ISO-8859-1', 'UTF-8') }}]",
        "[{{ \"a\\xc3\"|convert_encoding('UTF-16', 'UTF-8') }}]", "[{{ '€'|convert_encoding('ISO-8859-1', 'UTF-8') }}]",
        "{{ \"\\xe9\"|convert_encoding('UTF-8', 'ISO-8859-1') }}|[{{ n|convert_encoding('UTF-8', 'ASCII') }}]",
        "{{ i|convert_encoding('UTF-8', 'ASCII') }}", "{{ arr|convert_encoding('UTF-8', 'ASCII') }}",
        "{{ str|convert_encoding('UTF-8', 'UTF-8') }}", "{{ 'x'|convert_encoding(arr, 'UTF-8') }}",
        "[{{ 'x'|convert_encoding(1, 'UTF-8') }}]", "{{ 'x'|convert_encoding('UTF-8') }}",
        "{{ 'x'|convert_encoding(...['UTF-8']) }}", "{{ 'é'|convert_encoding(from='UTF-8', to='ISO-8859-1')|url_encode }}",
    ]:
        c.append(case(tpl))
    c.append(case("{{ 'abc'|replace({'': 'x'}) }}", errors="throw"))
    c.append(case("[{{ 'é'|convert_encoding('NOPE', 'UTF-8') }}]", errors="throw"))
    c.append(case("{{ 'abc'|trim('z..a') }}", errors="throw"))
    return c


def split():
    c = []
    for tpl in [
        "{{ 'one,two,three'|split(',')|join('|') }}|{{ 'one,two,three,four'|split(',', 2)|join('|') }}",
        "{{ 'one,two,three'|split(',', -1)|join('|') }}|{{ 'one,two'|split(',', 0)|join('|') }}|{{ ''|split(',')|length }}",
        "{{ ''|split(',', -1)|length }}|{{ 'a'|split(',', -1)|length }}|{{ n|split(',')|length }}|{{ 12345|split(3)|join('|') }}",
        "{{ '123'|split('')|join('|') }}|{{ 'aabbcc'|split('', 2)|join('|') }}|{{ 'aabbc'|split('', 2)|join('|') }}",
        "{{ 'ab'|split('', 5)|join('|') }}|{{ 'éàü'|split('')|join('|') }}|{{ ''|split('')|length }}|{{ 'abc'|split('', -1)|join('|') }}",
        "{{ 'éàüöß'|split('', 2)|join('|') }}|{{ 'a\\nb'|split('')|length }}|{{ 'abc'|split('', 1)|join('|') }}",
        "{{ \"a\\xffb\"|split('')|join('|') }}", "{{ \"a\\xffb\"|split('', 2)|join('|') }}", "{{ \"a\\xffb\"|split(',')|join('|') }}",
        "{{ 'abc'|split('', 'abc')|join('|') }}|{{ 'abcde'|split('', '2')|join('|') }}|{{ 'abcde'|split('', true)|join('|') }}",
        "{{ 'abcde'|split('', 2.5)|join('|') }}", "{{ 'abcde'|split('', [2])|join('|') }}", "{{ 'abcde'|split('', '2abc')|join('|') }}",
        "{{ 'abcde'|split('', '2.5')|join('|') }}", "{{ 'abcde'|split('', 1e999)|join('|') }}", "{{ 'abcde'|split('', fa)|join('|') }}",
        "{{ 123|split('', 5)[0] is same as(123) ? 'int' : 'str' }}|{{ 123|split('')|join('|') }}|{{ 1.5|split('')|join('|') }}",
        "{{ t|split('')|join('|') }}|{{ str|split('', 3)|join('|') }}|{{ markup|split('', 4)|join('|') }}",
        "{{ arr|split('') }}", "{{ arr|split(',') }}", "{{ arr|split('', 2) }}", "{{ plain|split('') }}",
        "{{ plain|split(',') }}", "{{ counter|split('', 2)|join('|') }}", "{{ thrower|split('') }}",
        "{{ 'a'|split(null)|join('|') }}", "{{ 'a1b'|split(1)|join('|') }}", "{{ 'atrueb'|split(true)|join('|') }}",
        "{{ 'a'|split(fa)|join('|') }}", "{{ 'a'|split(arr) }}", "{{ 'a,b'|split(',', 'x') }}", "{{ 'a,b'|split(',', 1.5)|join('|') }}",
        "{{ 'a'|split(...[]) }}", "{{ 'a,b'|split(...{delimiter: ','})|join('|') }}", "{{ 'a,b'|split(',', ...{limit: 1})|join('|') }}",
        "{{ 'a'|split(',', 2, 3)|join('|') }}", "{{ 'a'|split(...{charset: 'x', delimiter: ','}) }}",
        # join
        "{{ [1, 2, 3]|join }}|{{ [1, 2, 3]|join(', ', ' and ') }}|{{ [1]|join(', ', ' and ') }}|{{ [1, 2]|join(', ', ', ') }}",
        "{{ 'abc'|join(',') }}|{{ n|join }}|{{ 5|join }}|{{ {a: 1, b: 2}|join(',') }}|{{ [t, fa, n, 1.5, 1.0]|join(',') }}",
        "{{ []|join(',', 'and') }}|{{ [[1]]|join(',') }}|{{ [1, [2]]|join(',') }}", "{{ [[1]]|join(',', ' and ') }}",
        "{{ [n]|join(',', ' and ') }}", "{{ [t]|join(',', ' and ') }}|{{ [1.5]|join(',', ' and ') }}|{{ [str]|join(',', ' and ') }}",
        "{{ [plain]|join(',', ' and ') }}", "{{ [plain]|join }}", "{{ [1, 2]|join(n) }}", "{{ [1, 2]|join(arr) }}",
        "{{ [1, 2, 3]|join(',', arr) }}", "{{ [1, 2, 3]|join(',', plain) }}", "{{ [1, 2]|join(1, 1) }}|{{ [1, 2]|join(1, '1') }}",
        "{{ iter|join(',') }}|{{ arrobj|join(',', '+') }}|{{ str|join }}|{{ markup|join }}|{{ t|join }}|{{ fa|join(',') }}",
        "{{ [str, markup]|join(' ', ' & ') }}|{{ [1, 2, 3]|join(glue='-') }}|{{ [1, 2, 3]|join(and='+') }}",
        "{{ [1, 2]|join(...{and: '&'}) }}|{{ [1, 2]|join('-', '+', 'extra') }}", "{{ [1, 2]|join(...{x: 1}) }}",
        "{{ [1, 2, 3]|join(', ', ...[' and ']) }}",
    ]:
        c.append(case(tpl))
    for charset in ("NOPE",):
        c.append(case("{{ 'abc'|split('', 2)|join('|') }}", config={"charset": charset}))
        c.append(case("{{ 'abc'|split('')|join('|') }}", config={"charset": charset}))
    for charset in ("ISO-8859-1", "koi8-r", "8bit"):
        c.append(case("{{ 'abc'|split('', 2)|join('|') }}|{{ 'aéb'|split('', 2)|join('|')|url_encode }}",
                      config={"charset": charset}))
    c.append(case("{{ 'abc'|split('', 2)|join('|') }}", config={"charset": "SJIS"},
                  twig_rs={"error": "Twig\\Error\\RuntimeError: " + wrapped(
                      'mb_strlen(): encoding "SJIS" is not supported by twig-rs')}))
    c.append(case("{{ [[1], 2]|join(',') }}", errors="throw"))
    return c


def runtime():
    c = []
    safe_str = {"safe_classes": [["TwigRsStr", ["html"]]]}
    for setup, tpls in [
        ({"escapers": ["star"]}, [
            "{{ 'foo'|e('star') }}|{{ n|e('star') }}|{{ i|e('star') }}|{{ f|e('star') }}|{{ t|e('star') }}",
            "{{ 'x'|e('star', 'ISO-8859-1') }}|{{ 'x'|e('star', '') }}|{{ ''|e('star') }}|{{ str|e('star') }}",
            "{{ arr|e('star') }}", "{{ plain|e('star') }}", "{{ markup|e('star') }}|{{ markup|e('star', null, true) }}",
            "{{ markup_none|e('star', null, true) }}", "{{ s|e('nope') }}", "{{ s|e('html') }}|{{ s|e('star')|e }}",
        ]),
        ({"escapers": ["type", "wrap", "star"]}, [
            "{{ 'x'|e('type') }}|{{ i|e('type') }}|{{ n|e('type') }}|{{ arr|e('type') }}|{{ plain|e('type') }}",
            "{{ str|e('type') }}|{{ iter|e('type') }}|{{ markup|e('type') }}|{{ markup|e('type', null, true) }}",
            "{{ 'x'|e('wrap')|join(',') }}", "{{ 'x'|e('wrap') }}", "{{ 'x'|e('nope') }}", "{{ i|e('nope') }}",
        ]),
        ({"escapers": ["star"]}, ["{% autoescape 'star' %}{{ s }}|{{ i }}|{{ n }}|{{ markup }}{% endautoescape %}"]),
        (safe_str, [
            "{{ str }}|{{ str|e }}|{{ str|e('html', null, true) }}|{{ str|e('js', null, true) }}",
            "{{ child }}|{% autoescape 'js' %}{{ str }}{% endautoescape %}|{% autoescape 'html_attr' %}{{ str }}{% endautoescape %}",
        ]),
        ({"safe_classes": [["\\TwigRsStr", ["js"]]]}, [
            "{{ str }}|{% autoescape 'js' %}{{ str }}|{{ child }}{% endautoescape %}",
        ]),
        ({"safe_classes": [["TwigRsSafeIface", ["html_attr"]]]}, [
            "{{ str }}|{{ child }}|{% autoescape 'html_attr_relaxed' %}{{ str }}{% endautoescape %}|{% autoescape 'js' %}{{ str }}{% endautoescape %}",
        ]),
        ({"safe_classes": [["TwigRsSafeIface", ["all"]]]}, [
            "{{ str }}|{% autoescape 'js' %}{{ child }}{% endautoescape %}|{% autoescape 'url' %}{{ str }}{% endautoescape %}",
        ]),
        ({"safe_classes": [["TwigRsStr", ["html_attr_relaxed"]], ["TwigRsChild", ["js"]]]}, [
            "{{ str }}|{{ child }}|{% autoescape 'js' %}{{ str }}|{{ child }}{% endautoescape %}",
            "{% autoescape 'html_attr' %}{{ str }}|{{ child }}{% endautoescape %}",
        ]),
        ({"safe_classes": [["Twig\\Markup", ["html"]]]}, [
            "{{ markup_js }}|{{ markup_none }}|{{ markup }}",
        ]),
        ({"safe_classes": [["twigrsstr", ["html"]]]}, ["{{ str }}"]),
        ({"safe_classes": [["TwigRsStr", ["html"]]], "escapers": ["star"]}, [
            "{% autoescape 'star' %}{{ str }}{% endautoescape %}|{{ str|e('star', null, true) }}",
        ]),
    ]:
        for tpl in tpls:
            c.append(case(tpl, setup=setup, config={"autoescape": "html"}))
    return c


TOPICS = {"escape": escape, "autoescape": autoescape, "case": case_filters, "text": text, "split": split,
          "runtime": runtime}

if __name__ == "__main__":
    args = sys.argv[1:]
    oracle = "--oracle" in args
    names = [a for a in args if a != "--oracle"]
    for name in names or TOPICS:
        write(name, TOPICS[name](), oracle)
