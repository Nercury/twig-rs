"""Writes the oracle batches tests/vectors/upstream/visitors/<topic>.gen.json of tests/upstream_visitors.rs: the cases
of Twig 3.31.1's PHPUnit tests ported there (TWIG/tests/ExpressionParserTest.php, TokenParser/*,
NodeVisitor/EscaperTest.php, TemplateEscapeStrategyTest.php), run the way the upstream test runs them. The Rust
ports compare twig-rs with what Twig produced, so they also check what PHPUnit leaves open (the exact message where
the test only expects an exception class or a substring, the output where it compares two renders).

Twig-kind cases render with a plain environment (`new Environment(new ArrayLoader($templates), $config)`); php-kind
cases are the upstream test bodies (extensions, visitors and callbacks defined in PHP) returning their results.

Regenerate from the repository root (Git Bash on Windows needs MSYS_NO_PATHCONV=1):

    python -I tests/vectors/upstream/visitors/upstream_visitors.gen.py [topic ...]            # writes <topic>.gen.json
    python -I tests/vectors/upstream/visitors/upstream_visitors.gen.py --oracle [topic ...]   # and runs them: <topic>.json

`--oracle` pipes each batch through `docker run --rm -i twig-oracle:3.31.1 php runner.php --pretty`.
"""

import json
import os
import subprocess
import sys

OUT = os.path.dirname(os.path.abspath(__file__))


def php_str(value):
    """A PHP single-quoted string literal."""
    return "'" + value.replace("\\", "\\\\").replace("'", "\\'") + "'"


def php_value(value):
    if value is False:
        return "false"
    if value is True:
        return "true"
    if value is None:
        return "null"
    return php_str(value)


def plain(case_id, templates, main, config=None, data_php=None, data_json=None):
    case = {"id": case_id, "templates": templates, "main": main, "env": "plain"}
    if config is not None:
        case["config"] = config
    if data_php is not None:
        case["data_php"] = data_php
    if data_json is not None:
        case["data_json"] = data_json
    return case


def php(case_id, code):
    return {"id": case_id, "php_code": code}


# Records a throwable like the runner does (class and message, and the previous throwable).
DESCRIBE = r"""
$describe = static function (\Throwable $e) use (&$describe): array {
    $out = ['class' => $e::class, 'message' => $e->getMessage()];
    if ($e->getPrevious()) {
        $out['previous'] = $describe($e->getPrevious());
    }
    return $out;
};
"""

# ---- ExpressionParserTest ------------------------------------------------------------------------------------------

FAILING_ASSIGNMENTS = [
    '{% set false = "foo" %}',
    '{% set FALSE = "foo" %}',
    '{% set true = "foo" %}',
    '{% set TRUE = "foo" %}',
    '{% set none = "foo" %}',
    '{% set NONE = "foo" %}',
    '{% set null = "foo" %}',
    '{% set NULL = "foo" %}',
    '{% set 3 = "foo" %}',
    '{% set 1 + 2 = "foo" %}',
    '{% set "bar" = "foo" %}',
    "{% set %}{% endset %}",
]

MISPLACED_SPREADS = [
    "{{ ...foo }}",
    "{{ (...foo) }}",
    "{{ -...foo }}",
    "{{ 1 + ...foo }}",
    "{% set bar = ...foo %}",
    "{% do ...foo %}",
    "{% with ...foo %}{% endwith %}",
    "{{ (bar => ...foo)(1) }}",
    "{{ [(...foo)] }}",
    "{{ [1, -...foo] }}",
    "{{ {a: ...foo} }}",
    "{{ {(...foo): 1} }}",
    "{{ bar(-...foo) }}",
    "{{ bar(a: ...foo) }}",
    "{{ bar(a = ...foo) }}",
]

EMPTY_DESTRUCTURING = [
    "{% do [] = values %}",
    "{% do {} = values %}",
    "{% do [,] = values %}",
    "{% do [,,] = values %}",
]

NULL_SAFE = [
    ("{{ foo?.bar }}", "['foo' => (object) ['bar' => 'baz']]"),
    ("{{ foo?.bar }}", "['foo' => null]"),
    ("{{ foo?.bar?.baz }}", "['foo' => (object) ['bar' => (object) ['baz' => 'qux']]]"),
    ("{{ foo?.bar?.baz }}", "['foo' => (object) ['bar' => null]]"),
    ("{{ foo?.bar?.baz }}", "['foo' => null]"),
    ('{{ foo?.bar?.baz ?? "qux" }}', "['foo' => null]"),
    ('{{ foo?.bar ?? "qux" }}', "['foo' => (object) ['bar' => 0]]"),
    ('{{ foo?.bar ?? "qux" }}', "['foo' => (object) ['bar' => false]]"),
    ("{{ foo?.bar.baz }}", "['foo' => null]"),
    ("{{ foo?.bar.baz?.qux.corge }}", "['foo' => null]"),
    ("{{ foo?.bar.baz?.qux.corge }}", "['foo' => (object) ['bar' => (object) ['baz' => null]]]"),
    ("{{ foo?.bar|default(foo?.baz) }}", "['foo' => null]"),
    ("{{ foo?.bar|default(foo?.baz.qux) }}", "['foo' => null]"),
    (
        "{% for foo in foos %}[{{ foo?.bar|default(foo?.baz) }}]{% endfor %}",
        "['foos' => [(object) ['bar' => 'corge', 'baz' => 'grault'], (object) ['baz' => 'qux']]]",
    ),
]

INVALID_NULL_SAFE = [
    ("{{ foo?.bar.baz }}", "['foo' => (object) ['bar' => null]]"),
    ("{{ foo?.bar.baz?.qux.corge }}", "['foo' => (object) ['bar' => (object) ['baz' => (object) ['qux' => null]]]]"),
]

NON_CONSTANT_MACRO_DEFAULTS = [
    '{% macro foo(name = "a #{foo} a") %}{% endmacro %}',
    '{% macro foo(name = [["b", "a #{foo} a"]]) %}{% endmacro %}',
]

CONSTANT_MACRO_DEFAULTS = [
    '{% macro foo(name = "aa") %}{% endmacro %}',
    "{% macro foo(name = 12) %}{% endmacro %}",
    "{% macro foo(name = true) %}{% endmacro %}",
    '{% macro foo(name = ["a"]) %}{% endmacro %}',
    '{% macro foo(name = [["a"]]) %}{% endmacro %}',
    '{% macro foo(name = {a: "a"}) %}{% endmacro %}',
    '{% macro foo(name = {a: {b: "a"}}) %}{% endmacro %}',
]


def binding_power_tests():
    """`getBindingPowerTests()`: (name, expression, expected expression, context)."""
    tests = []
    for op1 in ["*", "/", "//", "%"]:
        for op2 in ["+", "-"]:
            tests.append((f"{op1} vs {op2}", f"{{{{ 12 {op1} 6 {op2} 3 }}}}", f"{{{{ (12 {op1} 6) {op2} 3 }}}}", None))
            tests.append((f"{op2} vs {op1}", f"{{{{ 12 {op2} 6 {op1} 3 }}}}", f"{{{{ 12 {op2} (6 {op1} 3) }}}}", None))
    for op1 in ["+", "-", "*", "/", "//", "%"]:
        for op2 in ["==", "!=", "<=>", "<", ">", ">=", "<="]:
            tests.append((f"{op1} vs {op2}", f"{{{{ 12 {op1} 6 {op2} 3 }}}}", f"{{{{ (12 {op1} 6) {op2} 3 }}}}", None))
    tests.append(("+ vs not in", "{{ 1 + 2 not in [3, 4] }}", "{{ (1 + 2) not in [3, 4] }}", None))
    tests.append(("+ vs in", "{{ 1 + 2 in [3, 4] }}", "{{ (1 + 2) in [3, 4] }}", None))
    tests.append(("+ vs matches", '{{ 1 + 2 matches "/^3$/" }}', '{{ (1 + 2) matches "/^3$/" }}', None))
    tests.append(("~ vs starts with", '{{ "a" ~ "b" starts with "a" }}', '{{ ("a" ~ "b") starts with "a" }}', None))
    tests.append(("~ vs ends with", '{{ "a" ~ "b" ends with "b" }}', '{{ ("a" ~ "b") ends with "b" }}', None))
    context = {"a": {"b": 1, "c": {"d": 2}}}
    tests.append(("[] vs unary -", '{{ -a["b"] + 3 }}', '{{ -(a["b"]) + 3 }}', context))
    tests.append(("[] vs unary - (multiple levels)", '{{ -a["c"]["d"] }}', '{{ -((a["c"])["d"]) }}', context))
    tests.append((". vs unary -", "{{ -a.b }}", "{{ -(a.b) }}", context))
    tests.append((". vs unary - (multiple levels)", "{{ -a.c.d }}", "{{ -((a.c).d) }}", context))
    tests.append((". [] vs unary -", '{{ -a.c["d"] }}', '{{ -((a.c)["d"]) }}', context))
    tests.append(("[] . vs unary -", '{{ -a["c"].d }}', '{{ -((a["c"]).d) }}', context))
    tests.append(("() vs unary -", "{{ -random(1, 1) + 3 }}", "{{ -(random(1, 1)) + 3 }}", None))
    tests.append(("+ vs |", "{{ 10 + 2|length }}", "{{ 10 + (2|length) }}", context))
    tests.append(("= same as literal", '{% do c = "a" %}{{ c }}', '{% do c = ("a") %}{{ c }}', None))
    tests.append(("= stronger than .", "{% do c = a.b %}{{ c }}", "{% do c = (a.b) %}{{ c }}", context))
    tests.append(("= stronger than math", "{% do a = 1 + 3 %}{{ a }}", "{% do a = (1 + 3) %}{{ a }}", None))
    tests.append(("= stronger than logical", "{% do a = false or true %}{{ a }}", "{% do a = (false or true) %}{{ a }}", None))
    tests.append(("= stronger than ternary", "{% do c = 4 ? 0 : -1 %}{{ c }}", "{% do c = (4 ? 0 : -1) %}{{ c }}", None))
    return tests


def expression_parser():
    no_escape = {"autoescape": False}
    cases = []
    for i, template in enumerate(FAILING_ASSIGNMENTS):
        cases.append(plain(f"can_only_assign_to_names/{i}", {"index": template}, "index", no_escape))
    for i, template in enumerate(MISPLACED_SPREADS):
        cases.append(plain(f"misplaced_spread_is_a_syntax_error/{i}", {"index": template}, "index", no_escape))
    cases.append(plain("sequence_compilation_error", {"index": "{{ [1,,2] }}"}, "index", no_escape))
    for i, template in enumerate(EMPTY_DESTRUCTURING):
        cases.append(plain(f"empty_destructuring_throws/{i}", {"index": template}, "index", no_escape))
    cases.append(php("object_destructuring_evaluates_right_hand_expression_once", r"""
$calls = 0;
$env = new \Twig\Environment(new \Twig\Loader\ArrayLoader(['template' => '{% do [result_first, result_second] = ({first, second} = next_value()) %}{{ first }} {{ second }} {{ result_first }} {{ result_second }}']));
$env->addFunction(new \Twig\TwigFunction('next_value', static function () use (&$calls): object {
    ++$calls;

    return (object) ['first' => $calls, 'second' => $calls];
}));
$output = $env->render('template');

return ['output' => $output, 'calls' => $calls];
"""))
    for i, (template, data) in enumerate(NULL_SAFE):
        cases.append(plain(f"null_safe_operator/{i}", {"template": template}, "template", {"strict_variables": True}, data_php=data))
    for i, (template, data) in enumerate(INVALID_NULL_SAFE):
        cases.append(plain(f"invalid_null_safe_operator_short_circuiting/{i}", {"template": template}, "template",
                           {"strict_variables": True}, data_php=data))
    cases.append(plain("macro_definition_does_not_support_non_name_variable_name",
                       {"index": '{% macro foo("a") %}{% endmacro %}'}, "index", no_escape))
    for i, template in enumerate(NON_CONSTANT_MACRO_DEFAULTS):
        cases.append(plain(f"macro_definition_does_not_support_non_constant_default_values/{i}", {"index": template},
                           "index", no_escape))
    for i, template in enumerate(CONSTANT_MACRO_DEFAULTS):
        cases.append(plain(f"macro_definition_supports_constant_default_values/{i}", {"index": template}, "index",
                           no_escape))
    # testCompiledCodeForDynamic{Test,Function,Filter}, rendered: what the compiled call passes to the callable
    for kind, template, method, cls in [
        ("test", '{{ "a" is foo_foo_bar_bar }}', "addTest", "TwigTest"),
        ("function", '{{ foo_foo_bar_bar("a") }}', "addFunction", "TwigFunction"),
        ("filter", '{{ "a"|foo_foo_bar_bar }}', "addFilter", "TwigFilter"),
    ]:
        cases.append(php(f"compiled_code_for_dynamic_callables/{kind}", f"""
$seen = [];
$env = new \\Twig\\Environment(new \\Twig\\Loader\\ArrayLoader(['index' => {php_str(template)}]), ['cache' => false, 'autoescape' => false]);
$env->{method}(new \\Twig\\{cls}('*_foo_*_bar', static function ($foo, $bar, $a) use (&$seen) {{
    $seen[] = "$foo,$bar,$a";

    return "$foo,$bar,$a";
}}));
$output = $env->render('index');

return ['output' => $output, 'seen' => $seen];
"""))
    for i, (name, expression, expected, context) in enumerate(binding_power_tests()):
        templates = {"expression": expression, "expected": expected}
        for main in ["expression", "expected"]:
            case = plain(f"binding_power/{i}/{main}", templates, main, data_json=context)
            case["name"] = name
            cases.append(case)
    # beyond the upstream tests: the mismatches found around them (see the tests named in the ids)
    for i, template in enumerate(NULL_SAFE_STRICT):
        cases.append(plain(f"null_safe_short_circuit_strict/{i}", {"template": template}, "template",
                           {"strict_variables": True}, data_php="['n' => null]"))
    for i, template in enumerate(MACRO_UNARY_DEFAULTS):
        cases.append(plain(f"macro_unary_defaults/{i}", {"index": template}, "index", no_escape))
    for i, template in enumerate(OPERAND_ORDER):
        cases.append(plain(f"operand_order/{i}", {"index": template}, "index", no_escape,
                           data_php="['arr' => [1], 'i' => 7]"))
    return cases


NULL_SAFE_STRICT = [
    "{{ u?.c.z ?? 'd' }}",
    "{{ u?.c.z is defined ? 'y' : 'n' }}",
    "{{ u?.c.z|default('d') }}",
    "{{ u?.c['z'] ?? 'd' }}",
    "{{ u?.c.z.w ?? 'd' }}",
    "{{ u?.c ?? 'd' }}",
    "{{ u?.c?.z.w ?? 'd' }}",
    "{{ n?.c.z ?? 'd' }}",
    "{{ u.b?.c.z ?? 'd' }}",
]

MACRO_UNARY_DEFAULTS = [
    "{% macro m(a = -'a') %}[{{ a|json_encode }}]{% endmacro %}{{ _self.m() }}",
    "{% macro m(a = +'a') %}[{{ a|json_encode }}]{% endmacro %}{{ _self.m() }}",
    "{% macro m(a = -[1]) %}[{{ a|json_encode }}]{% endmacro %}{{ _self.m() }}",
    "{% macro m(a = [-'a']) %}[{{ a|json_encode }}]{% endmacro %}{{ _self.m() }}",
    "{% macro m(a = {a: -''}) %}[{{ a|json_encode }}]{% endmacro %}{{ _self.m() }}",
    "{% macro m(a = - -'a') %}[{{ a|json_encode }}]{% endmacro %}{{ _self.m() }}",
    "{% macro m(a = -'a') %}[{{ a|json_encode }}]{% endmacro %}{{ _self.m(7) }}",
    "{% macro m(a = -'a') %}\n[{{ a }}]\n{% endmacro %}\nx\n{{ _self.m() }}\ny\n{{ 'z' }}\n",
    "{% macro m(a = -'1a') %}[{{ a|json_encode }}]{% endmacro %}{{ _self.m() }}{{ _self.m() }}",
]

OPERAND_ORDER = [
    "{{ 7 * arr }}",
    "{{ 7 b-and arr }}",
    "{{ 'a' b-or arr }}",
    "{{ null b-xor arr }}",
    "{{ (1 + 2) * arr }}",
    "{{ i * range(1, 2) }}",
    "{{ [1] * i }}",
    "{{ arr * 7 }}",
    "{{ i * arr }}",
    "{{ 7 + arr }}",
    "{{ range(1, 2) * i }}",
]


# ---- TokenParser/*TokenParserTest ----------------------------------------------------------------------------------

DOCUMENTATION_WHITESPACE_CONTROL = [
    ("opening whitespace trim marker", "A \n{##- The main content #}\n{% block content %}B{% endblock %}"),
    ("opening line whitespace trim marker", "A \n  {##~ The main content #}\n{% block content %}B{% endblock %}"),
    ("closing whitespace trim marker", "A{## The main content -##}\n  {% block content %}B{% endblock %}"),
    ("closing line whitespace trim marker", "A{## The main content ~##}  \n{% block content %}B{% endblock %}"),
]


def token_parser():
    cases = []
    for i, (name, template) in enumerate(DOCUMENTATION_WHITESPACE_CONTROL):
        case = plain(f"documentation_whitespace_control/{i}", {"index": template}, "index",
                     {"autoescape": False, "optimizations": 0})
        case["name"] = name
        cases.append(case)
    cases.append(plain("shortcut_assignment_named_docs_remains_valid",
                       {"index": '{% block title docs="The page title" %}'}, "index"))
    cases.append(plain("documentation_comment_requires_a_line_break",
                       {"index": "{% block ## Description %}Hello{% endblock %}"}, "index"))
    cases.append(php("undefined_handlers", DESCRIBE + r"""
$env = new \Twig\Environment(new \Twig\Loader\ArrayLoader(), ['cache' => false, 'autoescape' => false]);
$env->registerUndefinedFunctionCallback(static fn ($name) => throw new \Twig\Error\SyntaxError('boom.'));
try {
    (new \Twig\Parser($env))->parse($env->tokenize(new \Twig\Source('{% guard function boom %}{% endguard %}', '')));
    $parsed = null;
} catch (\Throwable $e) {
    $parsed = $describe($e);
}

// beyond the upstream test: the guard renders its else branch
$env = new \Twig\Environment(new \Twig\Loader\ArrayLoader(['index' => '{% guard function boom %}yes{% else %}no{% endguard %}']), ['cache' => false, 'autoescape' => false]);
$env->registerUndefinedFunctionCallback(static fn ($name) => throw new \Twig\Error\SyntaxError('boom.'));

return ['parse_error' => $parsed, 'render' => $env->render('index')];
"""))
    return cases


# ---- NodeVisitor/EscaperTest ---------------------------------------------------------------------------------------

ESCAPER_TEMPLATES = [
    ("autoescaped print", "html", "{{ foo }}"),
    ("print marked as safe", "html", "{{ foo|raw }}"),
    ("autoescaping disabled", False, "{{ foo }}{# not autoescaped #}"),
    ("explicit escape filter", False, "{{ foo|e }}"),
    ("autoescape tag", False, '{% autoescape "html" %}{{ foo }}{% endautoescape %}'),
    ("escaping in an embedded template only", False,
     '{% embed "embedded" %}{% block content %}{{ foo|e }}{% endblock %}{% endembed %}'),
]


def escaper():
    cases = []
    for i, (name, autoescape, template) in enumerate(ESCAPER_TEMPLATES):
        case = php(f"templates_fetch_the_escaper_only_when_they_escape/{i}", f"""
$env = new \\Twig\\Environment(new \\Twig\\Loader\\ArrayLoader(['index' => {php_str(template)}, 'embedded' => '{{% block content %}}{{% endblock %}}']), ['autoescape' => {php_value(autoescape)}]);
$compiled = $env->compileSource($env->getLoader()->getSourceContext('index'));

return [
    'escaper_classes' => substr_count($compiled, 'private \\Twig\\Runtime\\EscaperRuntime $escaper;'),
    'output' => $env->render('index', ['foo' => '<br>']),
];
""")
        case["name"] = name
        cases.append(case)
    # the upstream test, and the same with a compilation error twig-rs can raise (`{% autoescape null %}`: the
    # TypeError of `EscaperNodeVisitor::needEscaping()`), as twig-rs has no throwing `is_safe_callback`
    for case_id, broken in [
        ("compilation_error_inside_an_autoescape_tag_does_not_affect_the_next_template",
         "{% autoescape false %}{{ foo|failing|nl2br }}{% endautoescape %}"),
        ("compilation_error_inside_an_autoescape_tag_does_not_affect_the_next_template/null_strategy",
         "{% autoescape false %}{% autoescape null %}{{ foo|nl2br }}{% endautoescape %}{% endautoescape %}"),
    ]:
        cases.append(php(case_id, DESCRIBE + f"""
$env = new \\Twig\\Environment(new \\Twig\\Loader\\ArrayLoader([
    'broken.html' => {php_str(broken)},
    'index.html' => '{{{{ foo }}}}',
    'index.txt' => '{{{{ foo }}}}',
]), ['autoescape' => 'name']);
$env->addExtension(new class extends \\Twig\\Extension\\AbstractExtension {{
    public function getFilters(): array
    {{
        return [new \\Twig\\TwigFilter('failing', static fn ($value) => $value, ['is_safe_callback' => static function (): array {{
            throw new \\LogicException('Unable to compute the safety of the filter.');
        }}])];
    }}
}});

try {{
    $env->load('broken.html');
    $error = null;
}} catch (\\Throwable $e) {{
    $error = $describe($e);
}}

return [
    'error' => $error,
    'index.txt' => $env->render('index.txt', ['foo' => '<br>']),
    'index.html' => $env->render('index.html', ['foo' => '<br>']),
];
"""))
    cases.append(php("escape_filter_added_by_a_later_visitor_still_escapes", r"""
$env = new \Twig\Environment(new \Twig\Loader\ArrayLoader(['index' => '{{ "<br>" }}']), ['autoescape' => false]);
$env->addExtension(new class extends \Twig\Extension\AbstractExtension {
    public function getNodeVisitors(): array
    {
        return [new class implements \Twig\NodeVisitor\NodeVisitorInterface {
            public function enterNode(\Twig\Node\Node $node, \Twig\Environment $env): \Twig\Node\Node
            {
                return $node;
            }

            public function leaveNode(\Twig\Node\Node $node, \Twig\Environment $env): ?\Twig\Node\Node
            {
                if (!$node instanceof \Twig\Node\PrintNode) {
                    return $node;
                }

                $filter = $env->getFilter('escape');
                $class = $filter->getNodeClass();

                return new \Twig\Node\PrintNode(new $class($node->getNode('expr'), $filter, new \Twig\Node\Nodes([new \Twig\Node\Expression\ConstantExpression('html', 1)]), 1), 1);
            }

            public function getPriority(): int
            {
                return 10;
            }
        }];
    }
});

return $env->render('index');
"""))
    return cases


# ---- TemplateEscapeStrategyTest ------------------------------------------------------------------------------------

STRATEGY_TEMPLATES = [
    ("html", "guessed_html.html.twig", "name"),
    ("js", "guessed_js.js.twig", "name"),
    (False, "guessed_none.txt.twig", "name"),
    ("html", "forced_html.js.twig", "html"),
    (False, "disabled.html.twig", False),
]

AUTOESCAPE_TAGS = [
    ("another_strategy.html.twig", "'js'"),
    ("no_escaping.html.twig", "false"),
]


def template_escape_strategy():
    cases = []
    for i, (_, name, autoescape) in enumerate(STRATEGY_TEMPLATES):
        cases.append(php(f"compiled_templates_expose_their_strategy/{i}", f"""
$twig = new \\Twig\\Environment(new \\Twig\\Loader\\ArrayLoader([
    {php_str(name)} => '{{{{ value }}}}',
]), ['autoescape' => {php_value(autoescape)}, 'cache' => false]);

return $twig->load({php_str(name)})->getDefaultEscapeStrategy();
"""))
    for i, (name, tag) in enumerate(AUTOESCAPE_TAGS):
        cases.append(php(f"an_autoescape_tag_does_not_change_the_strategy_of_the_template/{i}", f"""
$twig = new \\Twig\\Environment(new \\Twig\\Loader\\ArrayLoader([
    {php_str(name)} => {php_str("{% autoescape " + tag + " %}{{ value }}{% endautoescape %}")},
]), ['autoescape' => 'name', 'cache' => false]);

return $twig->load({php_str(name)})->getDefaultEscapeStrategy();
"""))
    cases.append(php("the_strategy_describes_the_body_and_not_what_the_template_renders", r"""
$twig = new \Twig\Environment(new \Twig\Loader\ArrayLoader([
    'inherits.html.twig' => "{% extends 'inherited.txt.twig' %}",
    'inherited.txt.twig' => '{{ value }}',
]), ['autoescape' => 'name', 'cache' => false]);
$template = $twig->load('inherits.html.twig');

return ['strategy' => $template->getDefaultEscapeStrategy(), 'output' => $template->render(['value' => '<b>'])];
"""))
    cases.append(php("embedded_templates_expose_the_strategy_of_their_template", r"""
$twig = new \Twig\Environment(new \Twig\Loader\ArrayLoader([
    'index.js.twig' => "{% embed 'layout.html.twig' %}{% block content %}{{ value }}{% endblock %}{% endembed %}",
    'layout.html.twig' => '{% block content %}{% endblock %}',
]), ['autoescape' => 'name', 'cache' => false]);
$compiled = $twig->compileSource(new \Twig\Source($twig->getLoader()->getSourceContext('index.js.twig')->getCode(), 'index.js.twig'));

return [
    'methods' => substr_count($compiled, 'public function getDefaultEscapeStrategy(): string|false'),
    'js' => substr_count($compiled, 'return "js";'),
    'output' => $twig->render('index.js.twig', ['value' => '<b>\'']),
];
"""))
    return cases


# ---- Node/Expression/FilterTest, Node/Expression/MacroReferenceTest -----------------------------------------------

# The filters of `FilterTest::createEnvironment()`, returning what they receive (the environment and the context
# replaced by markers) instead of nothing.
FILTER_ENV = r"""
$describe = static function (\Throwable $e) use (&$describe): array {
    $out = ['class' => $e::class, 'message' => $e->getMessage()];
    if ($e->getPrevious()) {
        $out['previous'] = $describe($e->getPrevious());
    }
    return $out;
};
$env = new \Twig\Environment(new \Twig\Loader\ArrayLoader(), ['autoescape' => false]);
$env->addFilter(new \Twig\TwigFilter('anonymous', static function () {}));
$env->addFilter(new \Twig\TwigFilter('bar', static function (\Twig\Environment $env, ...$args) { return json_encode(['env', ...$args]); }, ['needs_environment' => true]));
$env->addFilter(new \Twig\TwigFilter('barbar', static function ($context, $string, $arg1 = null, $arg2 = null, array $args = []) { return json_encode(['context', $string, $arg1, $arg2, $args]); }, ['needs_context' => true, 'is_variadic' => true]));
$env->addFilter(new \Twig\TwigFilter('bar_sandbox', static function (bool $isSandboxed, $string) { return json_encode([$isSandboxed, $string]); }, ['needs_is_sandboxed' => true]));
$env->addFilter(new \Twig\TwigFilter('bar_all', static function (string $charset, \Twig\Environment $env, array $context, bool $isSandboxed, $string) { return json_encode([$charset, 'env', 'context', $isSandboxed, $string]); }, ['needs_charset' => true, 'needs_environment' => true, 'needs_context' => true, 'needs_is_sandboxed' => true]));
$render = static function (string $template) use ($env, $describe) {
    try {
        return ['output' => $env->createTemplate($template, 'index')->render([])];
    } catch (\Throwable $e) {
        return ['error' => $describe($e)];
    }
};
"""

FILTER_TEMPLATES = [
    '{{ "foo"|upper|number_format(2, ".", ",") }}',
    '{{ 0|date(timezone="America/Chicago", format="d/m/Y H:i:s P") }}',
    '{{ 0|date(timezone="America/Chicago") }}',
    '{{ "abc"|reverse(preserve_keys=true) }}',
    '{{ "abc"|reverse(preserveKeys=true) }}',
    '{{ "foo"|anonymous }}',
    '{{ "abc"|bar_sandbox }}',
    '{{ "abc"|bar_all }}',
    '{{ "abc"|bar }}',
    '{{ "abc"|bar("bar") }}',
    '{{ "abc"|barbar }}',
    '{{ "abc"|barbar(foo="bar") }}',
    '{{ "abc"|barbar(arg2="bar") }}',
    '{{ "abc"|barbar("1", "2", "3", foo="bar") }}',
]


def node():
    cases = []
    templates = ", ".join(php_str(t) for t in FILTER_TEMPLATES)
    cases.append(php("filter_test/provide_tests", FILTER_ENV + f"""
return array_map($render, [{templates}]);
"""))
    cases.append(php("filter_test/compile_errors", FILTER_ENV + r"""
$errors = [];
foreach (['{{ 0|date(foobar="America/Chicago") }}', '{{ 0|replace(to="foo") }}'] as $template) {
    try {
        // the bare node of the upstream test has no source: a template without a name compiles the same way
        $env->setLoader(new \Twig\Loader\ArrayLoader(['' => $template]));
        $env->compileSource(new \Twig\Source($template, ''));
        $errors[] = null;
    } catch (\Throwable $e) {
        $errors[] = $describe($e);
    }
}

return $errors;
"""))
    # CallTest exercises the deprecated CallExpression::getArguments(); templates go through
    # CallableArgumentsExtractor, whose arbitrary-arguments check throws a SyntaxError instead of a LogicException
    cases.append(php("call_test/arbitrary_arguments_in_templates", FILTER_ENV + r"""
if (!function_exists('twig_rs_custom_call_test_function')) {
    function twig_rs_custom_call_test_function($required) {}
}
$env->addFunction(new \Twig\TwigFunction('foo', static function () {}, ['is_variadic' => true]));
$env->addFunction(new \Twig\TwigFunction('bar', 'twig_rs_custom_call_test_function', ['is_variadic' => true]));

return [$render('{{ foo() }}'), $render('{{ bar() }}')];
"""))
    for i, (template, data) in enumerate(MACRO_REFERENCES):
        cases.append(plain(f"macro_reference_test/{i}", {"index": template}, "index", {"autoescape": False},
                           data_json=data))
    return cases


# `testDynamicNameResolvesMacroAtRuntime()` as renders: a macro called by an expression name.
MACRO_REFERENCES = [
    ("{% macro hello(who = 'you') %}hi {{ who }}{% endmacro %}{% import _self as mac %}{{ mac.(name)() }}",
     {"name": "hello"}),
    ("{% macro hello(who = 'you') %}hi {{ who }}{% endmacro %}{% import _self as mac %}{{ mac.(name)('me') }}",
     {"name": "hello"}),
    ("{% macro hello() %}hi{% endmacro %}{% import _self as mac %}{{ mac.(name)() }}", {"name": "missing"}),
    ("{% macro hello() %}hi{% endmacro %}{% import _self as mac %}{{ mac.(name ~ 'lo')() }}", {"name": "hel"}),
    ("{% macro hello() %}hi{% endmacro %}{{ _self.(name)() }}", {"name": "hello"}),
    ("{% macro hello() %}hi{% endmacro %}{% import _self as mac %}{{ mac.(name) is defined ? 'y' : 'n' }}",
     {"name": "hello"}),
    ("{% macro hello() %}hi{% endmacro %}{% import _self as mac %}{{ mac.(name) is defined ? 'y' : 'n' }}",
     {"name": "nope"}),
    ("{% macro hello() %}hi{% endmacro %}{% import _self as mac %}{{ mac.(name)() }}", {"name": "HELLO"}),
    ("{% macro hello() %}hi{% endmacro %}{% import _self as mac %}{{ mac.(name)() }}", {"name": 1}),
    ("{% macro hello() %}hi{% endmacro %}{% import _self as mac %}{{ mac.(name)() }}", {"name": None}),
    ("{% macro hello() %}hi{% endmacro %}{% import _self as mac %}{{ mac.(name)() }}", {"name": ["hello"]}),
    ("{% macro hello() %}hi{% endmacro %}{% import _self as mac %}{{ mac.(name) }}", {"name": "hello"}),
]


# ---- Extension/SandboxBridgeExtensionTest, Extension/SandboxStateChangeTest, Sandbox/SandboxTest ------------------

SANDBOX_PRELUDE = DESCRIBE + r"""
$try = static function (callable $f) use ($describe): array {
    try {
        return ['value' => $f()];
    } catch (\Throwable $e) {
        return ['error' => $describe($e)];
    }
};
$strict = static function (array $tags = [], array $filters = [], array $methods = [], array $properties = [], array $functions = [], array $tests = []): \Twig\Sandbox\SecurityPolicy {
    $policy = new \Twig\Sandbox\SecurityPolicy($tags, $filters, $methods, $properties, $functions, $tests);
    $policy->setStrict(true);

    return $policy;
};
$plain = static fn (array $templates = []) => new \Twig\Environment(new \Twig\Loader\ArrayLoader($templates), ['autoescape' => false]);
if (!class_exists('TwigRsSandboxTestObject', false)) {
    eval('final class TwigRsSandboxTestObject implements \Stringable { public $name = "fabien"; public function getName(): string { return "fabien"; } public function __toString(): string { return "object"; } }');
}
"""

BRIDGE_TEMPLATES = [
    '{{ render_sandboxed("content", {name: "Fabien"}, "html") }}',
    '{% autoescape "js" %}{{ render_sandboxed("content", {name: "Fabien"}, "html") }}{% endautoescape %}',
    '{{ render_sandboxed("content", {name: "Fabien"}, output_strategy: "html") }}',
    '{{ render_sandboxed("content", {name: "Fabien"}, outputStrategy: "html") }}',
    '{{ render_sandboxed("content", {name: "Fabien"}, strategy) }}',
    '{{ render_sandboxed("content", {name: "Fabien"}, "all") }}',
    '{{ render_sandboxed("content", {name: "Fabien"}) }}',
]

# Sandbox/SandboxTest: the cases whose upstream assertion is a message substring or an exception class only
SANDBOX_CASES = {
    "disallowed_tag": r"""(new \Twig\Sandbox\Sandbox($plain(['index' => '{% if 1 %}x{% endif %}']), $strict()))->render('index')""",
    "disallowed_filter": r"""(new \Twig\Sandbox\Sandbox($plain(['index' => '{{ "a"|upper }}']), $strict()))->render('index')""",
    "disallowed_function": r"""(new \Twig\Sandbox\Sandbox($plain(['index' => '{{ max(1, 2) }}']), $strict()))->render('index')""",
    "disallowed_test": r"""(function () use ($plain, $strict) { $env = $plain(['index' => '{{ 1 is funky ? "y" : "n" }}']); $env->addTest(new \Twig\TwigTest('funky', static fn ($value) => true)); return (new \Twig\Sandbox\Sandbox($env, $strict()))->render('index'); })()""",
    "rendering_directly_on_the_sandbox_environment": r"""(function () use ($plain, $strict) { $env = $plain(['index' => '{{ "a"|upper }}']); new \Twig\Sandbox\Sandbox($env, $strict()); return $env->render('index'); })()""",
    "method_denied": r"""(new \Twig\Sandbox\Sandbox($plain(['index' => '{{ obj.getName() }}']), $strict()))->render('index', ['obj' => new TwigRsSandboxTestObject()])""",
    "property_denied": r"""(new \Twig\Sandbox\Sandbox($plain(['index' => '{{ obj.name }}']), $strict()))->render('index', ['obj' => new TwigRsSandboxTestObject()])""",
    "to_string_denied": r"""(new \Twig\Sandbox\Sandbox($plain(['index' => '{{ obj }}']), $strict()))->render('index', ['obj' => new TwigRsSandboxTestObject()])""",
    "join_of_non_iterable_object": r"""(new \Twig\Sandbox\Sandbox($plain(['index' => '{{ obj|join(",") }}']), $strict(filters: ['join'])))->render('index', ['obj' => new class { private $secret = 'SECRET_PROPERTY'; private $nested; public function __construct() { $this->nested = new TwigRsSandboxTestObject(); } }])""",
    "join_of_stringable_denied": r"""(new \Twig\Sandbox\Sandbox($plain(['index' => '{{ obj|join(",") }}']), $strict(filters: ['join'])))->render('index', ['obj' => new TwigRsSandboxTestObject()])""",
    "markup_with_content_denied": r"""(new \Twig\Sandbox\Sandbox($plain(['index' => '{{ m.withContent("<b>b</b>") }}']), $strict()))->render('index', ['m' => new \Twig\Markup('a', 'UTF-8')])""",
    "included_templates_are_sandboxed": r"""(new \Twig\Sandbox\Sandbox($plain(['index' => '{{ include("partial") }}', 'partial' => '{{ "a"|upper }}']), $strict(functions: ['include'])))->render('index')""",
    "extends_denied": r"""(new \Twig\Sandbox\Sandbox($plain(['index' => '{% extends "layout" %}', 'layout' => 'layout content']), $strict()))->render('index')""",
    "use_denied": r"""(new \Twig\Sandbox\Sandbox($plain(['index' => '{% use "missing" with content as base_content %}']), $strict()))->render('index')""",
    "use_checked_before_the_trait_is_loaded": r"""(new \Twig\Sandbox\Sandbox($plain(['index' => '{{ block("b", "receiver") is defined ? "YES" : "NO" }}', 'receiver' => '{% use "missing" %}']), $strict(tags: ['block'], functions: ['block'])))->render('index')""",
    "auto_escaping_requires_the_escape_filter": r"""(new \Twig\Sandbox\Sandbox(new \Twig\Environment(new \Twig\Loader\ArrayLoader(['index' => '{{ v }}'])), $strict()))->render('index', ['v' => 'a&b'])""",
    "create_template_denied": r"""(new \Twig\Sandbox\Sandbox($plain(), $strict(filters: ['upper'])))->createTemplate('{{ name|lower }}')->render(['name' => 'fabien'])""",
    "already_used_environment": r"""(function () use ($plain, $strict) { $env = $plain(['index' => 'plain content']); $env->render('index'); new \Twig\Sandbox\Sandbox($env, $strict()); return 'created'; })()""",
    "environment_with_a_sandbox_extension": r"""(function () use ($plain, $strict) { $env = $plain(); $env->addExtension(new \Twig\Extension\SandboxExtension($strict())); new \Twig\Sandbox\Sandbox($env, $strict()); return 'created'; })()""",
}

FOREIGN_WRAPPERS = [
    ("include tag", "{% include foreign %}", "foreign content", ["include"], []),
    ("include function", "{{ include(foreign) }}", "foreign content", [], ["include"]),
    ("include function fallback", '{{ include(["missing", foreign]) }}', "foreign content", [], ["include"]),
    ("include_only function", "{{ include_only(foreign) }}", "foreign content", [], ["include_only"]),
    ("extends tag", "{% extends foreign %}", "foreign content", ["extends"], []),
    ("embed tag", "{% embed foreign %}{% endembed %}", "foreign content", ["embed", "extends"], []),
    ("import tag", "{% import foreign as macros %}{{ macros.foo() }}", "{% macro foo() %}foreign content{% endmacro %}",
     ["import"], []),
    ("from tag", "{% from foreign import foo %}{{ foo() }}", "{% macro foo() %}foreign content{% endmacro %}",
     ["from"], []),
    ("block function", '{{ block("content", foreign) }}', "{% block content %}foreign content{% endblock %}", [],
     ["block"]),
]

# Extension/SandboxStateChangeTest: the exceptions of the second render
STATE_CHANGE = {
    "enable_sandbox_after_first_render": r"""
$twig = $plain(['t' => '{{ "foo"|upper }}']);
$twig->addExtension($sandbox = new \Twig\Extension\SandboxExtension(new \Twig\Sandbox\SecurityPolicy(allowedFilters: []), false));
$first = $try(fn () => $twig->render('t'));
$sandbox->enableSandbox();
return [$first, $try(fn () => $twig->render('t'))];
""",
    "set_security_policy_tightening": r"""
$twig = $plain(['t' => '{{ "foo"|upper }}']);
$twig->addExtension($sandbox = new \Twig\Extension\SandboxExtension(new \Twig\Sandbox\SecurityPolicy(allowedFilters: ['upper', 'escape']), true));
$first = $try(fn () => $twig->render('t'));
$sandbox->setSecurityPolicy(new \Twig\Sandbox\SecurityPolicy(allowedFilters: ['escape']));
return [$first, $try(fn () => $twig->render('t'))];
""",
    "pre_warmed_parent_template_through_extends": r"""
$twig = $plain(['parent.twig' => '{% block c %}default{% endblock %}{{ "hi"|upper }}', 'child.twig' => '{% extends "parent.twig" %}{% block c %}child{% endblock %}']);
$twig->addExtension($sandbox = new \Twig\Extension\SandboxExtension(new \Twig\Sandbox\SecurityPolicy(allowedTags: ['extends', 'block'], allowedFilters: []), false));
$first = $try(fn () => $twig->render('child.twig'));
$sandbox->enableSandbox();
return [$first, $try(fn () => $twig->render('child.twig'))];
""",
    "pre_warmed_shared_via_sandboxed_include": r"""
$twig = $plain(['top' => '{{ include("user", sandboxed=true) }}', 'user' => '{% extends "shared" %}{% block c %}user{% endblock %}', 'shared' => '{% block c %}{% endblock %}{{ "ok"|upper }}']);
$twig->addExtension(new \Twig\Extension\SandboxExtension(new \Twig\Sandbox\SecurityPolicy(allowedTags: ['extends', 'block'], allowedFunctions: ['include'], allowedFilters: ['escape']), false));
$first = $try(fn () => $twig->render('shared'));
return [$first, $try(fn () => $twig->render('top'))];
""",
    "macro_from_pre_warmed_template": r"""
$twig = $plain(['macros.twig' => '{% macro greet(name) %}{{ name|upper }}{% endmacro %}', 'caller.twig' => '{% import "macros.twig" as m %}{{ m.greet("world") }}']);
$twig->addExtension($sandbox = new \Twig\Extension\SandboxExtension(new \Twig\Sandbox\SecurityPolicy(allowedTags: ['import', 'macro'], allowedFilters: []), false));
$first = $try(fn () => $twig->render('caller.twig'));
$sandbox->enableSandbox();
return [$first, $try(fn () => $twig->render('caller.twig'))];
""",
    "sandbox_tag_around_include_of_pre_warmed_template": r"""
$twig = $plain(['wrapper' => '{% sandbox %}{% include "user" %}{% endsandbox %}', 'user' => '{% extends "shared" %}{% block c %}user{% endblock %}', 'shared' => '{% block c %}{% endblock %}{{ "ok"|upper }}']);
$twig->addExtension(new \Twig\Extension\SandboxExtension(new \Twig\Sandbox\SecurityPolicy(allowedTags: ['extends', 'block', 'include'], allowedFilters: ['escape']), false));
$first = $try(fn () => $twig->render('shared'));
return [$first, $try(fn () => $twig->render('wrapper'))];
""",
    "tag_bypass_through_pre_warmed_parent": r"""
$twig = $plain(['parent.twig' => '{% block c %}{% for i in 1..2 %}{{ i }}{% endfor %}{% endblock %}', 'child.twig' => '{% extends "parent.twig" %}{% block c %}{{ parent() }}{% endblock %}']);
$twig->addExtension($sandbox = new \Twig\Extension\SandboxExtension(new \Twig\Sandbox\SecurityPolicy(allowedTags: ['extends', 'block'], allowedFilters: [], allowedFunctions: ['parent', 'range']), false));
$first = $try(fn () => $twig->render('child.twig'));
$sandbox->enableSandbox();
return [$first, $try(fn () => $twig->render('child.twig'))];
""",
    "function_bypass_through_pre_warmed_parent": r"""
$twig = $plain(['parent.twig' => '{% block c %}{{ range(1, 2)|first }}{% endblock %}', 'child.twig' => '{% extends "parent.twig" %}{% block c %}{{ parent() }}{% endblock %}']);
$twig->addExtension($sandbox = new \Twig\Extension\SandboxExtension(new \Twig\Sandbox\SecurityPolicy(allowedTags: ['extends', 'block'], allowedFilters: ['first'], allowedFunctions: ['parent']), false));
$first = $try(fn () => $twig->render('child.twig'));
$sandbox->enableSandbox();
return [$first, $try(fn () => $twig->render('child.twig'))];
""",
    "dynamic_parent_filter_rejected_when_reached_via_macro_import": r"""
$twig = $plain(['grandparent.twig' => '{% macro foo() %}grand{% endmacro %}', 'middle.twig' => '{% extends parent_name|evil %}', 'caller.twig' => '{% import "middle.twig" as m %}{{ m.foo() }}']);
$twig->addExtension(new \Twig\Extension\SandboxExtension(new \Twig\Sandbox\SecurityPolicy(allowedTags: ['extends', 'import', 'macro'], allowedFilters: []), true));
$calls = 0;
$twig->addFilter(new \Twig\TwigFilter('evil', static function ($v) use (&$calls) { ++$calls; return $v; }));
return [$try(fn () => $twig->render('caller.twig', ['parent_name' => 'grandparent.twig'])), $calls];
""",
    "dynamic_parent_function_rejected_when_reached_via_macro_import": r"""
$twig = $plain(['grandparent.twig' => '{% macro foo() %}grand{% endmacro %}', 'middle.twig' => '{% extends evil(parent_name) %}', 'caller.twig' => '{% import "middle.twig" as m %}{{ m.foo() }}']);
$twig->addExtension(new \Twig\Extension\SandboxExtension(new \Twig\Sandbox\SecurityPolicy(allowedTags: ['extends', 'import', 'macro'], allowedFunctions: []), true));
$calls = 0;
$twig->addFunction(new \Twig\TwigFunction('evil', static function ($v) use (&$calls) { ++$calls; return $v; }));
return [$try(fn () => $twig->render('caller.twig', ['parent_name' => 'grandparent.twig'])), $calls];
""",
}


def sandbox():
    cases = []
    templates = ", ".join(php_str(t) for t in BRIDGE_TEMPLATES)
    cases.append(php("sandbox_bridge_extension_test", SANDBOX_PRELUDE + f"""
$results = [];
foreach ([{templates}] as $template) {{
    $sandboxEnvironment = $plain(['content' => '<strong>{{{{ name }}}}</strong>']);
    $twig = new \\Twig\\Environment(new \\Twig\\Loader\\ArrayLoader(['index' => $template]));
    $twig->addExtension(new \\Twig\\Extension\\SandboxBridgeExtension());
    $policy = $strict();
    $twig->addRuntimeLoader(new \\Twig\\RuntimeLoader\\FactoryRuntimeLoader([
        \\Twig\\Runtime\\SandboxBridgeRuntime::class => static fn () => new \\Twig\\Runtime\\SandboxBridgeRuntime(new \\Twig\\Sandbox\\Sandbox($sandboxEnvironment, $policy)),
    ]));
    $results[] = $try(fn () => $twig->render('index'));
}}

return $results;
"""))
    for name, code in SANDBOX_CASES.items():
        cases.append(php(f"sandbox_test/{name}", SANDBOX_PRELUDE + f"\nreturn $try(fn () => {code});\n"))
    for i, (_, template, foreign, tags, functions) in enumerate(FOREIGN_WRAPPERS):
        cases.append(php(f"sandbox_test/foreign_template_wrapper/{i}", SANDBOX_PRELUDE + f"""
$foreign = $plain(['foreign' => {php_str(foreign)}]);
$sandbox = new \\Twig\\Sandbox\\Sandbox($plain(['index' => {php_str(template)}]), $strict(tags: [{", ".join(php_str(t) for t in tags)}], functions: [{", ".join(php_str(f) for f in functions)}]));

return $try(fn () => $sandbox->render('index', ['foreign' => $foreign->load('foreign')]));
"""))
    for name, code in STATE_CHANGE.items():
        cases.append(php(f"sandbox_state_change_test/{name}", SANDBOX_PRELUDE + code))
    return cases


TOPICS = {
    "sandbox": sandbox,
    "node": node,
    "expression_parser": expression_parser,
    "token_parser": token_parser,
    "escaper": escaper,
    "template_escape_strategy": template_escape_strategy,
}


def main(argv):
    oracle = "--oracle" in argv
    topics = [a for a in argv if not a.startswith("--")] or list(TOPICS)
    for topic in topics:
        batch = TOPICS[topic]()
        text = json.dumps(batch, indent=1, ensure_ascii=False) + "\n"
        path = os.path.join(OUT, f"{topic}.gen.json")
        with open(path, "w", encoding="utf-8", newline="\n") as f:
            f.write(text)
        print(f"{path}: {len(batch)} cases")
        if oracle:
            env = dict(os.environ, MSYS_NO_PATHCONV="1")
            result = subprocess.run(
                ["docker", "run", "--rm", "-i", "twig-oracle:3.31.1", "php", "runner.php", "--pretty"],
                input=text.encode("utf-8"), capture_output=True, env=env, check=True)
            out = os.path.join(OUT, f"{topic}.json")
            with open(out, "wb") as f:
                f.write(result.stdout.replace(b"\r\n", b"\n"))
            print(f"{out}: written")


if __name__ == "__main__":
    main(sys.argv[1:])
