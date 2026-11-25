#!/usr/bin/env python3
"""Writes test_extension.gen.json: oracle cases for the fixture harness's environment (tests/fixtures/environment.rs)
and its port of the test-only extensions of TWIG/tests/IntegrationTest.php (tests/fixtures/extension.rs).

Every case renders in the oracle's "integration" environment (exactly IntegrationTestCase's setup) with PHP warnings
and notices thrown, as PHPUnit throws them. The harness's self-test `harness::test_extension_matches_the_oracle`
runs the same cases through tests/fixtures/environment.rs and compares output, exception, deprecations and
diagnostics with test_extension.json.

`data_rust` is ignored by the oracle: it is the context of the case for the Rust side, a list of [name, value] pairs
(in context order) whose values are JSON values except `{"$markup": "<content>"}` (`new \\Twig\\Markup($content,
'UTF-8')`) and `{"$stdClass": {}}` (`new \\stdClass()`). `data_php` builds the same context in PHP. Template order
matters too (the templates are loaded in order): readers keep the order of the `templates` object.

Regenerate (Git Bash, from the repository root):

    python tests/fixtures/vectors/test_extension.gen.py > tests/fixtures/vectors/test_extension.gen.json
    research/php-oracle/oracle.sh < tests/fixtures/vectors/test_extension.gen.json > tests/fixtures/vectors/test_extension.json
"""

import json
import sys

CASES = []


def case(id, templates, data_php=None, data_rust=None, config=None):
    if isinstance(templates, str):
        templates = {"index.twig": templates}
    c = {"id": id, "env": "integration", "errors": "throw", "templates": templates}
    if config is not None:
        c["config"] = config
    if data_php is not None:
        c["data_php"] = data_php
        c["data_rust"] = [[name, value] for name, value in (data_rust or {}).items()]
    CASES.append(c)


# the § filter, function and tag
case("section/filter", "{{ 'a'|§ }}|{{ null|§ }}|{{ 1.5|§ }}|{{ true|§ }}")
case("section/function", "{{ §('a') }}")
case("section/array", "{{ [1]|§ }}")
case("section/object", "{{ o|§ }}", "return ['o' => new \\stdClass()]", {"o": {"$stdClass": {}}})
case("section/markup", "{{ m|§ }}", "return ['m' => new \\Twig\\Markup('<b>', 'UTF-8')]", {"m": {"$markup": "<b>"}})
case("section/missing_argument", "{{ §() }}")
case("section/named_argument", "{{ §(value='x') }}")
case("section/unknown_named_argument", "{{ §(foo='x') }}")
case("section/tag", "a{% § %}b")
case("section/tag_with_argument", "{% § foo %}")

# nl2br (pre_escape html, is_safe html)
case("nl2br/basic", "{{ \"a\\nb\"|nl2br }}")
case("nl2br/pre_escape", "{{ \"<a>\\n\"|nl2br }}")
case("nl2br/variable", "{{ v|nl2br }}", "return ['v' => \"<a>\\n&\"]", {"v": "<a>\n&"})
case("nl2br/separator", "{{ \"a\\nb\"|nl2br('|') }}")
case("nl2br/named_separator", "{{ \"a\\nb\"|nl2br(sep='|') }}")
case("nl2br/null", "[{{ null|nl2br }}]")
case("nl2br/array", "{{ v|nl2br|join(',') }}", "return ['v' => [\"a\\nb\", 'c']]", {"v": ["a\nb", "c"]})
case("nl2br/array_separator", "{{ \"a\\nb\"|nl2br([1]) }}")
case("nl2br/unknown_argument", "{{ \"a\\nb\"|nl2br(foo='x') }}")
case("nl2br/no_autoescape", "{{ \"<a>\\n\"|nl2br }}", config={"autoescape": False})

# escape_and_nl2br (needs_environment, is_safe html)
case("escape_and_nl2br/string", "{{ \"<b>\\nc\"|escape_and_nl2br }}")
case("escape_and_nl2br/markup", "{{ m|escape_and_nl2br }}", "return ['m' => new \\Twig\\Markup(\"<b>\\n\", 'UTF-8')]",
     {"m": {"$markup": "<b>\n"}})
case("escape_and_nl2br/int", "{{ 5|escape_and_nl2br }}")
case("escape_and_nl2br/array", "{{ [\"1\\n\"]|escape_and_nl2br|join }}")
case("escape_and_nl2br/empty", "[{{ ''|escape_and_nl2br }}]")
case("escape_and_nl2br/separator", "{{ \"a\\n&\"|escape_and_nl2br('!') }}")
case("escape_and_nl2br/null", "[{{ null|escape_and_nl2br }}]")
case("escape_and_nl2br/object", "{{ o|escape_and_nl2br }}", "return ['o' => new \\stdClass()]",
     {"o": {"$stdClass": {}}})

# escape_something (is_safe something) and preserves_safety (preserves_safety html)
case("escape_something/html", "{{ \"<b>\"|escape_something }}")
case("escape_something/autoescape_something",
     "{% autoescape 'something' %}{{ \"<b>\"|escape_something }}{% endautoescape %}")
case("escape_something/null", "[{{ null|escape_something }}]")
case("escape_something/array", "{{ [1]|escape_something }}")
case("preserves_safety/constant", "{{ \"<b>\"|preserves_safety }}")
case("preserves_safety/variable", "{{ v|preserves_safety }}", "return ['v' => '<b>']", {"v": "<b>"})
case("preserves_safety/raw", "{{ v|raw|preserves_safety }}", "return ['v' => '<b>']", {"v": "<b>"})

# static and magic calls
case("static_call/all",
     "{{ 'a'|static_call_string }}|{{ 'b'|static_call_array }}|{{ static_call_string('c') }}|{{ static_call_array('d') }}")
case("static_call/array", "{{ static_call_string([1]) }}")
case("magic_call/all",
     "{{ 'foo'|magic_call }}|{{ 'foo'|magic_call_closure }}|{{ 'foo'|magic_call_string }}|{{ 'foo'|magic_call_array }}")
case("magic_call/extra_argument", "{{ 'foo'|magic_call('bar') }}|{{ 'foo'|magic_call_string('bar') }}")
case("magic_call/array", "{{ [1]|magic_call }}")
case("magic_call/null", "{{ null|magic_call_closure }}")

# dynamic filters, functions and tests
case("dynamic/all", "{{ 'a'|foo_path }}|{{ bar_path('b') }}|{{ 'c'|x_foo_y_bar }}|{{ p_foo_q_bar('r') }}")
case("dynamic/missing_argument", "{{ foo_path() }}")
case("dynamic/named_argument", "{{ foo_path(item='y') }}")
case("dynamic/array", "{{ [1]|foo_path }}")
case("dynamic/test", "{{ 'bar' is test_bar ? 'y' : 'n' }}{{ 'baz' is test_bar ? 'y' : 'n' }}{{ 1 is test_1 ? 'y' : 'n' }}")
case("not/filter", "{{ 'value'|not }}|{{ null|not }}")

# closures
case("anon_foo/all", "{{ anon_foo('bar') }}|{{ 'bar'|anon_foo }}|{{ anon_foo(name='x') }}")
case("anon_foo/missing_argument", "{{ anon_foo() }}")
case("anon_foo/array", "{{ [1]|anon_foo }}")
case("br/all", "{{ safe_br() }}|{{ unsafe_br() }}|{{ safe_br(1, 2) }}")
case("deprecated_function/call", "{{ deprecated_function() }}")
case("deprecated_function/twice", "{{ deprecated_function() }}\n{{ deprecated_function() }}")

# tests
case("multi_word/strings", "{{ 'a b' is multi word ? 'y' : 'n' }}{{ 'ab' is multi word ? 'y' : 'n' }}")
case("multi_word/int", "{{ 1 is multi word ? 'y' : 'n' }}")
case("multi_word/null", "{{ null is multi word ? 'y' : 'n' }}")
case("multi_word/array", "{{ [1] is multi word ? 'y' : 'n' }}")
case("multi_word/not", "{{ 'a b' is not multi word ? 'y' : 'n' }}")

# attribute-declared callables
case("attributes/all",
     "{{ attribute_function('foo') }}|{{ 'foo'|attribute_filter }}|{{ '*' is attribute_test ? 'y' : 'n' }}|"
     "{{ lazy_attribute_function('foo') }}|{{ 'foo'|lazy_attribute_filter }}|{{ '+' is lazy_attribute_test ? 'y' : 'n' }}")
case("attributes/int", "{{ attribute_function(12) }}|{{ 1.5|lazy_attribute_filter }}")
case("attributes/bool", "{{ true is attribute_test ? 'y' : 'n' }}|{{ false|attribute_filter }}")
case("attributes/null", "{{ attribute_function(null) }}")
case("attributes/array", "{{ [1]|lazy_attribute_filter }}")
case("attributes/missing_argument", "{{ lazy_attribute_function() }}")
case("attributes/named_argument", "{{ attribute_function(value='v') }}")
case("attributes/markup", "{{ m|attribute_filter }}", "return ['m' => new \\Twig\\Markup('<b>', 'UTF-8')]",
     {"m": {"$markup": "<b>"}})

# undefined-callable callbacks
case("undefined/filter", "{{ 'a'|throwing_undefined_filter }}")
case("undefined/function", "{{ throwing_undefined_function() }}")
case("undefined/test", "{{ 1 is throwing_undefined_test }}")
case("undefined/two_word_test", "{{ 1 is throwing_undefined_two words_test }}")
case("undefined/guard", "{% guard filter throwing_undefined_filter %}yes{% else %}no{% endguard %}")
case("undefined/unknown_filter", "{{ 'a'|nope }}")
case("undefined/two_word_tests", "{{ 4 is divisible by(2) ? 'y' : 'n' }}{{ 1 is same as(1) ? 'y' : 'n' }}")
case("undefined/single_word_divisible", "{{ 1 is divisible ? 'y' : 'n' }}|{{ 1 is same ? 'y' : 'n' }}")

# spreads bypass the compile-time argument checks: PHP's binding errors of the methods and closures
case("spread/section_function", "{{ §(...[]) }}")
case("spread/section_filter_extra", "{{ 'a'|§(...[1, 2]) }}")
case("spread/anon_foo_function", "{{ anon_foo(...[]) }}")
case("spread/anon_foo_named", "{{ anon_foo(...{name: 'n'}) }}")
case("spread/anon_foo_unknown_named", "{{ anon_foo(...{nope: 'n'}) }}")
case("spread/dynamic_path", "{{ foo_path(...[]) }}")
case("spread/dynamic_foo", "{{ a_foo_b_bar(...[]) }}")
case("spread/nl2br", "{{ 'a'|nl2br(...{sep: '!'}) }}")
case("spread/escape_and_nl2br_named_value", "{{ 'a'|escape_and_nl2br(...{value: 'b'}) }}")
case("spread/attribute_function", "{{ attribute_function(...[]) }}")
case("spread/deprecated_function", "{{ deprecated_function(...[1]) }}")
case("spread/magic_call", "{{ 'a'|magic_call(...{x: 'y'}) }}")
case("spread/test", "{{ 'a b' is multi word(...[]) ? 'y' : 'n' }}")

# the rest of the harness environment
case("harness/global", "{{ global }}")
case("harness/global_in_context", "{{ _context|keys|join(',') }}", "return ['a' => 1]", {"a": 1})
case("harness/strict_variables", "{{ nope }}")
case("harness/not_strict", "[{{ nope }}]", config={"strict_variables": False})
case("harness/dump_without_debug", "[{{ dump(1) }}]")
case("harness/dump", "{{ dump('a', [1]) }}", config={"debug": True, "autoescape": False})
case("harness/dump_context", "{{ dump() }}", "return ['a' => 1]", {"a": 1}, config={"debug": True})
case("harness/sandboxed_include",
     {"index.twig": "{{ include('foo.twig', sandboxed=true) }}", "foo.twig": "{{ 'a'|upper }}"})
case("harness/sandboxed_include_dump",
     {"index.twig": "{{ include('foo.twig', sandboxed=true) }}", "foo.twig": "{{ dump(1) }}"},
     config={"debug": True})
case("harness/template_from_string", "{{ include(template_from_string('{{ 1 + 1 }}')) }}")
case("harness/autoescape_name", {"index.js": "{{ '\"' }}"}, config={"autoescape": "name"})
case("harness/warning", "{{ 'a' ~ [1] }}", config={"strict_variables": False})
case("harness/deprecated_tag",
     {"index.twig": "{% extends 'base.twig' %}{% deprecated 'Do not use.' %}", "base.twig": "base"})
case("harness/constant", "{{ constant('Twig\\\\Tests\\\\TwigTestFoo::BAR_NAME') }}")
case("harness/enum", "{{ enum('Twig\\\\Tests\\\\DummyBackedEnum').FOO.value }}")

sys.stdout.buffer.write((json.dumps({"cases": CASES}, indent=1, ensure_ascii=False) + "\n").encode("utf-8"))
