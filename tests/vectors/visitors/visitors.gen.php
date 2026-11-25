<?php

/*
 * Generator of the node-visitor vectors (tests/vectors/visitors/visitors.json): the node tree Twig 3.31.1 returns
 * from Environment::parse() after all its node visitors ran (CorrectnessNodeVisitor -255, EscaperNodeVisitor 0,
 * SandboxNodeVisitor 0 with the sandbox extension, YieldNotReadyNodeVisitor 255, OptimizerNodeVisitor 255), or the
 * error, the deprecations raised on the way, and the tag/filter/function/test lists of the CheckSecurityNodes the
 * sandbox visitor added (private properties, not shown by the dump).
 *
 * Regenerate from the repository root (Git Bash on Windows needs MSYS_NO_PATHCONV=1):
 *
 *     MSYS_NO_PATHCONV=1 docker run --rm -i twig-oracle:3.31.1 php < tests/vectors/visitors/visitors.gen.php > tests/vectors/visitors/visitors.json
 *
 * A batch of cases given as a JSON file in a mounted directory replaces the built-in list (random.json is generated
 * this way from random.gen.json, see random.gen.py):
 *
 *     MSYS_NO_PATHCONV=1 docker run --rm -i -v "$PWD/tests/vectors/visitors:/v" -e CASES_FILE=/v/random.gen.json twig-oracle:3.31.1 php < tests/vectors/visitors/visitors.gen.php > tests/vectors/visitors/random.json
 *
 * Case fields: id, source, optional name (template name, default "index"), config (Environment options:
 * autoescape, optimizations), autoescape_map (the autoescape option is a callable returning map[name] for the names
 * of the map, false otherwise), extensions (["sandbox"]: new SandboxExtension(new SecurityPolicy()), ["debug"]),
 * ext (a test extension, see make_extension()).
 * Output per case: the case fields, then dump (or error: {class, message}), security (a list per module, the main
 * one first, then the embedded templates in index order: {tags, filters, functions, tests} as [name, line] pairs),
 * deprecations.
 *
 * The dump format is the one of tests/vectors/parser/expressions.gen.php (mirrored by twig::ast::dump).
 */

declare(strict_types=0);

require '/oracle/vendor/autoload.php';

use Twig\DeprecatedCallableInfo;
use Twig\Environment;
use Twig\Extension\AbstractExtension;
use Twig\Extension\DebugExtension;
use Twig\Extension\SandboxExtension;
use Twig\Loader\ArrayLoader;
use Twig\Node\CheckSecurityNode;
use Twig\Node\Expression\MacroReferenceExpression;
use Twig\Node\Expression\SupportDefinedTestInterface;
use Twig\Node\ModuleNode;
use Twig\Node\Node;
use Twig\Sandbox\SecurityPolicy;
use Twig\Source;
use Twig\TwigFilter;
use Twig\TwigFunction;
use Twig\TwigTest;

const NODE_CLASSES = [
    'filter' => [
        'default' => \Twig\Node\Expression\Filter\DefaultFilter::class,
        'escape' => \Twig\Node\Expression\Filter\EscapeFilter::class,
        'raw' => \Twig\Node\Expression\Filter\RawFilter::class,
    ],
    'function' => [],
    'test' => [],
];

/**
 * A test extension: ext = {filters|functions|tests: [[name, options]]}. Callable options: needs_environment,
 * needs_context, needs_charset, needs_is_sandboxed, is_variadic, is_safe, pre_escape, preserves_safety,
 * always_allowed_in_sandbox, node_class (a key of NODE_CLASSES), deprecated: [package, version]. The callable is a
 * closure with one untyped variadic parameter.
 */
function make_extension(array $ext): AbstractExtension
{
    $callables = [];
    foreach (['filter' => TwigFilter::class, 'function' => TwigFunction::class, 'test' => TwigTest::class] as $type => $class) {
        foreach ($ext[$type.'s'] ?? [] as [$name, $options]) {
            $phpOptions = [];
            foreach ($options as $key => $value) {
                if ('node_class' === $key) {
                    $phpOptions['node_class'] = NODE_CLASSES[$type][$value];
                } elseif ('deprecated' === $key) {
                    $phpOptions['deprecation_info'] = new DeprecatedCallableInfo(...$value);
                } else {
                    $phpOptions[$key] = $value;
                }
            }
            $callables[$type][] = new $class($name, static fn (...$args) => null, $phpOptions);
        }
    }

    return new class($callables) extends AbstractExtension {
        public function __construct(private array $callables)
        {
        }

        public function getFilters(): array
        {
            return $this->callables['filter'] ?? [];
        }

        public function getFunctions(): array
        {
            return $this->callables['function'] ?? [];
        }

        public function getTests(): array
        {
            return $this->callables['test'] ?? [];
        }
    };
}

function rs_quote(string $s): string
{
    $out = '"';
    $length = strlen($s);
    for ($i = 0; $i < $length; ++$i) {
        $c = $s[$i];
        $b = ord($c);
        if ('"' === $c || '\\' === $c) {
            $out .= '\\'.$c;
        } elseif ($b < 0x20 || 0x7F === $b) {
            $out .= sprintf('\\x%02x', $b);
        } else {
            $out .= $c;
        }
    }

    return $out.'"';
}

function rs_value($value): string
{
    if (null === $value) {
        return 'NULL';
    }
    if (is_bool($value)) {
        return $value ? 'true' : 'false';
    }
    if (is_int($value)) {
        return (string) $value;
    }
    if (is_float($value)) {
        return 'float('.var_export($value, true).')';
    }
    if (is_string($value)) {
        return rs_quote($value);
    }
    if (is_array($value)) {
        $parts = [];
        foreach ($value as $k => $v) {
            $parts[] = rs_value($k).' => '.rs_value($v);
        }

        return '['.implode(', ', $parts).']';
    }
    if ($value instanceof \Closure) {
        return '\Closure';
    }
    if ($value instanceof \Stringable) {
        return (string) $value;
    }

    return 'object('.get_class($value).')';
}

function rs_dump(Node $node, string $prefix = '', int $depth = 0): string
{
    $pad = str_repeat('  ', $depth);
    $class = get_class($node);
    if (str_starts_with($class, 'Twig\\Node\\')) {
        $class = substr($class, strlen('Twig\\Node\\'));
    }
    $line = $pad.$prefix.$class.' @'.$node->getTemplateLine();
    if (null !== $tag = $node->getNodeTag()) {
        $line .= ' #'.$tag;
    }
    if (null !== $doc = $node->getDocumentation()) {
        $line .= ' doc='.rs_quote($doc);
    }
    $lines = [$line];
    $nodeAttributes = [];
    foreach ((fn () => $this->attributes)->call($node) as $name => $value) {
        if ($value instanceof Node) {
            $nodeAttributes[$name] = $value;
            continue;
        }
        $lines[] = $pad.'  .'.$name.' = '.rs_value($value);
    }
    if ($node instanceof SupportDefinedTestInterface && $node->isDefinedTestEnabled()) {
        $lines[] = $pad.'  .(defined_test) = true';
    }
    if ($node instanceof MacroReferenceExpression) {
        $lines[] = $pad.'  .(call_parentheses) = '.($node->hasCallParentheses() ? 'true' : 'false');
    }
    foreach ($node->getNodes() as $key => $child) {
        $lines[] = rs_dump($child, '['.$key.'] ', $depth + 1);
    }
    foreach ($nodeAttributes as $name => $child) {
        $lines[] = rs_dump($child, '[.'.$name.'] ', $depth + 1);
    }

    return implode("\n", $lines);
}

/** The used tags/filters/functions/tests of the CheckSecurityNode of a module and of its embedded templates. */
function security(ModuleNode $module): array
{
    $out = [];
    foreach ($module->getNode('class_end') as $node) {
        if ($node instanceof CheckSecurityNode) {
            [$tags, $filters, $functions, $tests] = (fn () => [$this->usedTags, $this->usedFilters, $this->usedFunctions, $this->usedTests])->call($node);
            $pairs = static function (array $used): array {
                $list = [];
                foreach ($used as $name => $line) {
                    $list[] = [(string) $name, $line];
                }

                return $list;
            };
            $out[] = ['tags' => $pairs($tags), 'filters' => $pairs($filters), 'functions' => $pairs($functions), 'tests' => $pairs($tests)];
        }
    }
    $embedded = $module->getAttribute('embedded_templates');
    if ($embedded instanceof Node) {
        foreach ($embedded as $template) {
            array_push($out, ...security($template));
        }
    }

    return $out;
}

function enc_str(?string $s)
{
    if (null === $s) {
        return null;
    }

    return mb_check_encoding($s, 'UTF-8') ? $s : ['b64' => base64_encode($s)];
}

/** A case for every configuration: [id suffix => case fields]. */
function each_config(string $id, string $source, array $configs, array $extra = []): array
{
    $cases = [];
    foreach ($configs as $suffix => $fields) {
        $cases[] = ['id' => $id.'@'.$suffix, 'source' => $source] + $fields + $extra;
    }

    return $cases;
}

$autoescapes = [
    'html' => ['config' => ['autoescape' => 'html']],
    'false' => ['config' => ['autoescape' => false]],
    'js' => ['config' => ['autoescape' => 'js']],
];

$cases = [];

// ---- escaper: what gets escaped -----------------------------------------------------------------------------------
$escaperTemplates = [
    'print-name' => '{{ a }}',
    'print-raw' => '{{ a|raw }}',
    'print-constant' => "{{ 'x' }}{{ 1 }}{{ 1.5 }}{{ true }}{{ null }}",
    'print-concat' => "{{ a ~ 'x' }}",
    'print-filter' => '{{ a|upper }}',
    'print-escape' => '{{ a|e }}{{ a|escape }}',
    'print-escape-js' => "{{ a|e('js') }}",
    'print-escape-html-attr' => "{{ a|e('html_attr') }}{{ a|e('html_attr_relaxed') }}",
    'print-escape-dynamic' => '{{ a|e(b) }}',
    'print-escape-non-string-constant' => '{{ a|e(5) }}{{ a|e(true) }}{{ a|e(null) }}',
    'print-escape-twice' => '{{ a|e|e }}',
    'print-ternary' => '{{ a ? b : c }}',
    'print-ternary-safe' => "{{ a ? 'x' : b|raw }}{{ a ? 'x' : 'y' }}",
    'print-ternary-nested' => '{{ a ? (b ? c : d) : e }}{{ a ? b : (c ?: d) }}',
    'print-ternary-filtered' => '{{ (a ? b : c)|upper }}',
    'print-elvis' => "{{ a ?: b }}{{ a ?: 'x' }}",
    'print-null-coalesce' => "{{ a ?? b }}{{ a ?? 'x' }}{{ 'x' ?? 'y' }}",
    'print-null-coalesce-nested' => '{{ a ?? b ?? c }}{{ (a ?? b) ?: c }}',
    'print-block' => "{% block x %}{% endblock %}{{ block('x') }}{{ block('x', 'y') }}",
    'print-include' => "{{ include('x') }}{{ source('x') }}",
    'print-macro' => '{% macro m() %}{% endmacro %}{{ _self.m() }}',
    'print-default' => "{{ a|default('x') }}{{ a.b|default(c) }}",
    'print-arithmetic' => '{{ a + 1 }}{{ -a }}{{ not a }}{{ a ~ b ~ c }}',
    'print-getattr' => '{{ a.b }}{{ a[b] }}{{ a.b(c) }}',
    'print-function' => "{{ max(a, b) }}{{ range(1, 3)|join(',') }}{{ constant('E_ALL') }}",
    'print-tests' => '{{ a is defined }}{{ a is empty }}{{ a is same as(b) }}',
    'print-array' => '{{ [a, b] }}{{ {a: b} }}',
    'print-arrow' => '{{ a|map(x => x ~ 1)|join }}',
    'pre-escape-nl2br' => "{{ a|nl2br }}{{ a|raw|nl2br }}{{ 'x'|nl2br }}{{ a|e|nl2br }}{{ a|nl2br|nl2br }}",
    'pre-escape-in-set' => '{% set x = a|nl2br %}{% set y %}{{ a|nl2br }}{% endset %}',
    'pre-escape-spaceless' => '{{ a|spaceless }}',
    'pre-escape-js-input' => "{{ a|e('js')|nl2br }}{{ a|e('html_attr')|nl2br }}",
    'pre-escape-in-argument' => "{{ a|join(b|nl2br) }}",
    'set-not-escaped' => '{% set x = a %}{% set y = a ~ b %}{{ x }}',
    'capture' => '{% set x %}{{ a }}{% endset %}{{ x }}',
    'apply' => '{% apply upper %}{{ a }}{% endapply %}',
    'autoescape-tag' => "{% autoescape %}{{ a }}{% endautoescape %}{% autoescape 'js' %}{{ a }}{% endautoescape %}{% autoescape false %}{{ a }}{% endautoescape %}{{ a }}",
    'autoescape-nested' => "{% autoescape 'css' %}{{ a }}{% autoescape 'url' %}{{ a }}{% endautoescape %}{{ a }}{% endautoescape %}",
    'autoescape-safe-for-js' => "{% autoescape 'js' %}{{ a|e('js') }}{{ a|e }}{{ a|raw }}{% endautoescape %}",
    'autoescape-html-attr' => "{% autoescape 'html_attr' %}{{ a|e('html') }}{{ a|e('html_attr') }}{% endautoescape %}{% autoescape 'html' %}{{ a|e('html_attr') }}{{ a|e('html_attr_relaxed') }}{% endautoescape %}",
    'autoescape-true' => '{% autoescape true %}{{ a }}{% endautoescape %}',
    'autoescape-int' => '{% autoescape 1 %}{{ a }}{% endautoescape %}{% autoescape 0 %}{{ a }}{% endautoescape %}',
    'autoescape-float' => '{% autoescape 1.5 %}{{ a }}{% endautoescape %}',
    'autoescape-unknown' => "{% autoescape 'foo' %}{{ a }}{% endautoescape %}",
    'autoescape-null-text' => '{% autoescape null %}text{% endautoescape %}',
    'autoescape-null-print' => '{% autoescape null %}{{ a }}{% endautoescape %}',
    'autoescape-null-block' => '{% autoescape null %}{% block b %}x{% endblock %}{% endautoescape %}',
    'autoescape-ternary' => "{% autoescape 'js' %}{{ a ? b : c|e('js') }}{% endautoescape %}",
    'block-strategy-at-tag' => "{% autoescape false %}{% block b %}{{ a }}{% endblock %}{% endautoescape %}{{ block('b') }}",
    'block-strategy-js' => "{% autoescape 'js' %}{% block b %}{{ a }}{% block c %}{{ a }}{% endblock %}{% endblock %}{% endautoescape %}",
    'block-in-block-autoescape' => "{% block b %}{% autoescape 'css' %}{% block c %}{{ a }}{% endblock %}{% endautoescape %}{{ a }}{% endblock %}",
    'macro-default-strategy' => "{% autoescape 'js' %}{% macro m(x) %}{{ x }}{% endmacro %}{% endautoescape %}",
    'macro-autoescape-inside' => "{% macro m(x) %}{% autoescape 'css' %}{{ x }}{% endautoescape %}{{ x }}{% endmacro %}",
    'embed' => "{% embed 'x' %}{% block b %}{{ a }}{% endblock %}{% endembed %}{{ a }}",
    'embed-escape-only-inside' => "{% embed 'x' %}{% block b %}{{ a|e }}{% endblock %}{% endembed %}",
    'child-template' => "{% extends 'p' %}{% block b %}{{ a }}{{ parent() }}{% endblock %}",
    'with-and-if' => '{% with %}{{ a }}{% endwith %}{% if a %}{{ b }}{% else %}{{ c }}{% endif %}{% for x in a %}{{ x }}{% else %}{{ b }}{% endfor %}',
    'deprecated-tag' => "{% deprecated 'x' %}{{ a }}",
    'do-and-types' => '{% do a|nl2br %}{% types {a: "string"} %}{{ a }}',
];
foreach ($escaperTemplates as $id => $source) {
    array_push($cases, ...each_config('esc-'.$id, $source, $autoescapes));
}

// strategies of other defaults
foreach (['css', 'url', 'html_attr', 'html_attr_relaxed', 'foo', '0', ''] as $strategy) {
    $cases[] = ['id' => 'default-strategy-'.($strategy ?: 'empty'), 'source' => "{{ a }}{{ a|e('".$strategy."') }}{{ a ? b : c }}", 'config' => ['autoescape' => $strategy]];
}

// the 'name' strategy
foreach (['index.html', 'index.html.twig', 'index.js', 'index.js.twig', 'index.json', 'index.css', 'index.txt', 'index.txt.twig', 'index', 'dir/', 'a.txt/b', 'foo.twig'] as $name) {
    $cases[] = ['id' => 'name-strategy-'.$name, 'name' => $name, 'source' => '{{ a }}{% embed "x" %}{% block b %}{{ a }}{% endblock %}{% endembed %}', 'config' => ['autoescape' => 'name']];
}

// callable strategies (the callable returns map[name], false for other names)
foreach ([
    'string' => 'js',
    'false' => false,
    'true' => true,
    'int' => 5,
    'zero-string' => '0',
    'empty-string' => '',
    'float' => 2.5,
    'null' => null,
    'array' => ['x'],
] as $id => $value) {
    $cases[] = ['id' => 'callable-strategy-'.$id, 'source' => '{{ a }}', 'autoescape_map' => ['index' => $value], 'config' => []];
    $cases[] = ['id' => 'callable-strategy-'.$id.'-block', 'source' => '{% block b %}{{ a }}{% endblock %}', 'autoescape_map' => ['index' => $value], 'config' => []];
}
$cases[] = ['id' => 'callable-strategy-other-name', 'source' => '{{ a }}', 'autoescape_map' => ['other' => 'js'], 'config' => []];

// custom callables with escaping options
$escExt = ['filters' => [
    ['safe_html', ['is_safe' => ['html']]],
    ['safe_all', ['is_safe' => ['all']]],
    ['safe_js', ['is_safe' => ['js']]],
    ['safe_attr', ['is_safe' => ['html_attr']]],
    ['safe_attr_relaxed', ['is_safe' => ['html_attr_relaxed']]],
    ['preserves', ['preserves_safety' => ['html']]],
    ['preserves_all', ['preserves_safety' => ['all']]],
    ['preserves_js', ['preserves_safety' => ['js', 'html']]],
    ['pre_js', ['pre_escape' => 'js']],
    ['pre_html_safe_js', ['pre_escape' => 'html', 'is_safe' => ['js']]],
    ['my_raw', ['node_class' => 'raw', 'is_safe' => ['all']]],
    ['my_escape', ['node_class' => 'escape']],
], 'functions' => [
    ['fsafe', ['is_safe' => ['html']]],
    ['fsafe_js', ['is_safe' => ['js']]],
    ['fplain', []],
]];
$extTemplates = [
    'is-safe' => '{{ a|safe_html }}{{ a|safe_all }}{{ a|safe_js }}{{ a|safe_attr }}{{ a|safe_attr_relaxed }}',
    'preserves-safety' => "{{ a|preserves }}{{ a|raw|preserves }}{{ a|e|preserves }}{{ a|e('js')|preserves }}{{ 'x'|preserves }}{{ a|preserves_all }}{{ a|safe_attr|preserves }}{{ a|safe_js|preserves_js }}{{ a|raw|preserves_js }}",
    'pre-escape-custom' => "{{ a|pre_js }}{{ a|e('js')|pre_js }}{{ a|pre_html_safe_js }}{{ a|raw|pre_js }}",
    'custom-node-classes' => "{{ a|my_raw }}{{ a|my_escape }}{{ a|my_escape('js') }}",
    'functions' => '{{ fsafe() }}{{ fsafe_js() }}{{ fplain() }}{{ fsafe() ? fsafe() : fplain() }}',
    'conditional-intersections' => "{{ a ? a|safe_attr : a|safe_attr_relaxed }}{{ a ? a|safe_js : a|safe_html }}{{ a ?: a|safe_all }}{{ (a ?? b)|safe_html }}{{ a ?? b|safe_html }}{{ b|safe_html ?: c|safe_html }}",
];
foreach ($extTemplates as $id => $source) {
    array_push($cases, ...each_config('ext-'.$id, $source, $autoescapes + ['html_attr' => ['config' => ['autoescape' => 'html_attr']]], ['ext' => $escExt]));
}

// ---- optimizer ----------------------------------------------------------------------------------------------------
$optimizations = [
    'all' => ['config' => ['autoescape' => false, 'optimizations' => -1]],
    'none' => ['config' => ['autoescape' => false, 'optimizations' => 0]],
    'for' => ['config' => ['autoescape' => false, 'optimizations' => 2]],
];
$optimizerTemplates = [
    'print-text' => "{{ 'a' }}{{ \"b\" }}{{ 1 }}{{ 'a' ~ 'b' }}{{ '' }}",
    'print-block' => "{% block foo %}{% endblock %}{{ block('foo') }}{{ block('foo', 'x') }}{{ block('foo') ~ '' }}",
    'print-parent' => "{% extends 'x' %}{% block content %}{{ parent() }}{% endblock %}",
    'for-unused' => '{% for i in foo %}{% endfor %}',
    'for-loop' => '{% for i in foo %}{{ loop.index }}{% endfor %}',
    'for-nested-unused' => '{% for i in foo %}{% for j in foo %}{% endfor %}{% endfor %}',
    'for-include' => '{% for i in foo %}{% include "foo" %}{% endfor %}',
    'for-include-only' => '{% for i in foo %}{% include "foo" only %}{% endfor %}',
    'for-include-with-only' => '{% for i in foo %}{% include "foo" with { "foo": "bar" } only %}{% endfor %}',
    'for-include-with-loop-only' => '{% for i in foo %}{% include "foo" with { "foo": loop.index } only %}{% endfor %}',
    'for-nested-inner-loop' => '{% for i in foo %}{% for j in foo %}{{ loop.index }}{% endfor %}{% endfor %}',
    'for-nested-parent-loop' => '{% for i in foo %}{% for j in foo %}{{ loop.parent.loop.index }}{% endfor %}{% endfor %}',
    'for-loop-in-seq' => '{% for i in foo %}{% for j in [loop.index] %}{% endfor %}{% endfor %}',
    'for-loop-as-seq' => '{% for i in foo %}{% for j in loop %}{% endfor %}{% endfor %}',
    'for-set-loop' => '{% for i in foo %}{% set l = loop %}{% for j in foo %}{{ l.index }}{% endfor %}{% endfor %}',
    'for-foo-parent' => '{% for i in foo %}{% for j in foo %}{{ foo.parent.loop.index }}{% endfor %}{% endfor %}',
    'for-loop-bracket-parent' => '{% for i in foo %}{% for j in foo %}{{ loop["parent"].loop.index }}{% endfor %}{% endfor %}',
    'for-include-function' => '{% for i in foo %}{{ include("foo") }}{% endfor %}',
    'for-include-function-no-context' => '{% for i in foo %}{{ include("foo", with_context = false) }}{% endfor %}',
    'for-include-function-context' => '{% for i in foo %}{{ include("foo", with_context = true) }}{% endfor %}',
    'for-include-function-vars-no-context' => '{% for i in foo %}{{ include("foo", { "foo": "bar" }, with_context = false) }}{% endfor %}',
    'for-include-function-loop-no-context' => '{% for i in foo %}{{ include("foo", { "foo": loop.index }, with_context = false) }}{% endfor %}',
    'for-include-function-dynamic-context' => '{% for i in foo %}{{ include("foo", with_context = x) }}{% endfor %}',
    'for-include-function-camel-context' => '{% for i in foo %}{{ include("foo", withContext = false) }}{% endfor %}',
    'for-include-function-zero-context' => '{% for i in foo %}{{ include("foo", with_context = 0) }}{% endfor %}',
    'include-function-dynamic-context-outside' => '{{ include("foo", with_context = x) }}',
    'for-targets' => '{% for k, v in foo %}{{ k }}{{ v }}{{ _key }}{{ loop }}{% else %}{{ k }}{{ v }}{% endfor %}',
    'for-key-default' => '{% for v in foo %}{{ _key }}{{ v }}{% endfor %}',
    'for-targets-nested' => '{% for a in x %}{% for b in a %}{{ a }}{{ b }}{% endfor %}{{ b }}{% endfor %}{{ a }}',
    'for-seq-uses-targets' => '{% for a in x %}{% for b in a %}{% endfor %}{% endfor %}',
    'for-assign-loop' => '{% for i in foo %}{% set loop = 1 %}{% set i = 2 %}{% endfor %}',
    'for-block' => '{% for i in foo %}{% block b %}{% endblock %}{% endfor %}',
    'for-block-function' => "{% for i in foo %}{{ block('b') }}{% endfor %}{% block b %}{% endblock %}",
    'for-embed' => "{% for i in foo %}{% embed 'x' %}{% endembed %}{% endfor %}",
    'for-embed-only' => "{% for i in foo %}{% embed 'x' only %}{% endembed %}{% endfor %}",
    'for-dynamic-attribute' => '{% for i in foo %}{{ foo[i] }}{{ foo.(i) }}{% endfor %}',
    'for-dynamic-attribute-after-loop' => '{% for i in foo %}{{ loop.index }}{{ foo[i] }}{% endfor %}',
    'for-nested-dynamic-attribute' => '{% for i in foo %}{% for j in foo %}{{ loop.first }}{{ foo[j] }}{% endfor %}{% endfor %}',
    'for-in-macro' => '{% macro m() %}{% for i in foo %}{{ loop.index }}{% endfor %}{% endmacro %}',
    'for-in-block' => '{% block b %}{% for i in foo %}{{ i }}{% endfor %}{% endblock %}',
    'for-seq-loop-outside' => '{% for i in loop %}{{ loop }}{% endfor %}',
    'for-else-loop' => '{% for i in foo %}{% else %}{{ loop }}{% endfor %}',
    'for-print-constant' => "{% for i in foo %}{{ 'x' }}{{ block('b') }}{% endfor %}{% block b %}{% endblock %}",
    'for-ternary-with-loop' => '{% for i in foo %}{{ i ? loop.index : 0 }}{% endfor %}',
    'for-null-coalesce-loop' => '{% for i in foo %}{{ loop.index ?? 1 }}{{ i ?? 2 }}{% endfor %}',
    'for-arrow' => '{% for i in foo %}{{ foo|map(x => x ~ i)|join }}{% endfor %}',
];
foreach ($optimizerTemplates as $id => $source) {
    array_push($cases, ...each_config('opt-'.$id, $source, $optimizations));
}
foreach ([4, 8, 12, 6, 14, 15, 16, -2, 1, 3, 10] as $mode) {
    $cases[] = ['id' => 'opt-mode-'.$mode, 'source' => "{% for i in foo %}{{ loop.index }}{% endfor %}{{ 'x' }}", 'config' => ['autoescape' => false, 'optimizations' => $mode]];
}
array_push($cases, ...each_config('opt-escaped', "{{ 'x' }}{{ block('b') }}{% block b %}{{ a }}{% endblock %}{% for i in foo %}{{ i }}{% endfor %}", ['html' => ['config' => ['autoescape' => 'html']], 'js' => ['config' => ['autoescape' => 'js']]]));

// ---- sandbox ------------------------------------------------------------------------------------------------------
$sandboxConfigs = [
    'html' => ['config' => ['autoescape' => 'html'], 'extensions' => ['sandbox']],
    'false' => ['config' => ['autoescape' => false], 'extensions' => ['sandbox']],
];
$sandboxTemplates = [
    'appendix-a13' => "{{ a.b }}{{ a['c'] }}{{ a|upper }}{{ a ~ 'x' }}{% include 'x' %}{{ range(1,2)|join }}",
    'names' => '{{ a }}{{ _self }}{{ _context }}{{ _charset }}',
    'getattr' => '{{ a.b }}{{ a[b] }}{{ a.b(c, d) }}{{ a.b(c ~ d) }}{{ a?.b }}{{ a[b ~ c] }}',
    'filters' => '{{ a|upper }}{{ a|lower(b) }}{{ a|join(b) }}{{ a|replace({(b): c}) }}{{ a|raw }}',
    'typed-filters' => '{{ a|abs }}{{ a|json_encode(b, c) }}{{ a|json_encode(flags = b, depth = c) }}{{ a|invoke(b) }}{{ a|e(b, c, d) }}',
    'typed-functions' => "{{ range(a, b, c) }}{{ range(a, b, step = c) }}{{ constant(a, b, c) }}{{ include_only(a, b, c) }}{{ max(a, b) }}",
    'spread' => '{{ max(...a) }}{{ range(1, ...a) }}{{ range(1, 2, ...a) }}{{ [...a] }}{{ {...a} }}{{ a|merge(...b) }}{{ a|json_encode(1, ...b) }}{{ a|json_encode(...b) }}',
    'named-arguments' => '{{ a|json_encode(depth = b) }}{{ a|join(glue = b) }}{{ a|join(and = b, glue = c) }}{{ range(start = a, end = b) }}',
    'variadic' => '{{ a|format(b, c) }}{{ a|merge(b, c) }}{{ max(a, b, c) }}',
    'conditional' => "{{ (a ? b : c)|upper }}{{ a ?? b }}{{ a ?: b }}{{ max(a ? b : c) }}{{ (a ? b|raw : 'x')|upper }}",
    'binary' => "{{ a ~ b }}{{ a == b }}{{ a != b }}{{ a < b }}{{ a <= b }}{{ a > b }}{{ a >= b }}{{ a <=> b }}{{ a in b }}{{ a not in b }}{{ a matches '/x/' }}{{ a starts with b }}{{ a ends with b }}{{ a + b }}{{ a .. b }}{{ a === b }}{{ a has some b }}",
    'binary-nested' => "{{ (a ~ b) ~ (c|upper) }}{{ a.b == c.d }}{{ max(a) ~ min(b) }}",
    'range-operator' => '{{ 1..3 }}{{ a..b }}{% for i in 1..2 %}{% endfor %}',
    'tests' => "{{ a is empty }}{{ a is iterable }}{{ a is sequence }}{{ a is defined }}{{ a is null }}{{ a is even }}{{ a is odd }}{{ a is divisible by(b) }}{{ a is same as(b) }}{{ a is constant('E_ALL') }}{{ a is not empty }}",
    'tests-arguments' => "{{ a is constant(b) }}{{ a is constant(b, c) }}{{ a is iterable(b) }}",
    'constant-test-without-arguments' => '{{ a is constant }}',
    'functions' => "{{ block('b') }}{% block b %}{% endblock %}{{ attribute(a, 'b') }}{{ attribute(a, b, [c]) }}{{ cycle(a, b) }}{{ random(a) }}{{ date(a) }}{{ include(a) }}{{ source(a) }}",
    'parent' => "{% extends a %}{% block b %}{{ parent() }}{% endblock %}",
    'parent-constant' => "{% extends 'x' %}{% block b %}{{ parent() }}{% endblock %}",
    'parent-filtered' => "{% extends a|first %}",
    'tags' => "{% if a %}{% for b in c %}{% set d = e %}{% endfor %}{% endif %}{% set f %}x{% endset %}{% with %}{% endwith %}{% do a %}{% apply upper %}x{% endapply %}{% flush %}",
    'tags-lines' => "\n{% if a %}\n{% if b %}{% endif %}{% endif %}\n{% for x in y %}{% endfor %}",
    'include-tag' => "{% include a %}{% include 'x' with b %}{% include a ~ b %}{% include [a, b] %}",
    'import' => "{% import a as m %}{% import _self as s %}{% from a import x %}{% from _self import y %}{% macro y() %}{% endmacro %}",
    'macro-calls' => "{% import 'x' as m %}{{ m.foo(a) }}{{ m.(a)() }}{{ m.(a ~ b)() }}",
    'deprecated' => "{% deprecated a package=b version=c %}{% deprecated 'x' ~ a %}",
    'array-keys' => '{{ {(a): b, c: d, "e": f, 1: g, (h ~ i): j} }}{{ [a, b] }}',
    'array-in-arguments' => '{{ max([a, b]) }}{{ max({(a): b}) }}{{ a|join([b]) }}',
    'set-binary' => '{{ max(a = b) }}{{ max([a, b] = c) }}{{ max({a: b} = c) }}',
    'nested-calls' => '{{ max(min(a)) }}{{ a|upper|lower }}{{ max(a|upper) }}{{ max(a is empty) }}',
    'block-template' => "{{ block('b', a) }}{{ block('b', 'x') }}{{ block(a) }}{% block b %}{% endblock %}",
    'escape-then-sandbox' => "{% autoescape 'js' %}{{ a }}{{ a|nl2br }}{% endautoescape %}",
    'embed' => "{% embed a %}{% block b %}{{ c|upper }}{% endblock %}{% endembed %}{{ d|lower }}",
    'embed-two' => "{% embed 'x' %}{% endembed %}{% embed 'y' %}{% block c %}{{ range(1, 2) }}{% endblock %}{% endembed %}",
    'arrow-functions' => "{{ a|map(x => x ~ b)|join }}{{ a|filter(x => x)|join }}{{ a|sort((x, y) => x <=> y)|join }}",
    'text-only' => 'hello',
    'print-constant' => "{{ 'x' }}{{ 1 }}{{ block('b') }}{% block b %}{% endblock %}",
    'self-in-coerced' => "{{ _self ~ a }}{{ max(_self) }}{{ _self|upper }}",
    'use-tag' => "{% use 'x' %}{% use 'y' with b as c %}",
    'guard' => "{% guard function upper %}{% endguard %}{% guard filter upper %}{{ a|upper }}{% endguard %}",
];
foreach ($sandboxTemplates as $id => $source) {
    array_push($cases, ...each_config('sandbox-'.$id, $source, $sandboxConfigs));
}
$sandboxExt = [
    'filters' => [['always', ['always_allowed_in_sandbox' => true]], ['envf', ['needs_environment' => true, 'needs_charset' => true]], ['dyn_*', []]],
    'functions' => [['alwaysf', ['always_allowed_in_sandbox' => true]], ['plain', []], ['ctx', ['needs_context' => true, 'needs_is_sandboxed' => true]]],
    'tests' => [['alwayst', ['always_allowed_in_sandbox' => true]], ['plaint', []]],
];
array_push($cases, ...each_config('sandbox-ext', "{{ a|always }}{{ alwaysf(a) }}{{ a is alwayst }}{{ a|envf(b) }}{{ plain(a, b) }}{{ ctx(a) }}{{ a is plaint }}{{ a is plaint(b) }}{{ a|dyn_x(b) }}", $sandboxConfigs, ['ext' => $sandboxExt]));
array_push($cases, ...each_config('sandbox-with-debug', '{{ dump(a) }}{{ dump() }}', ['html' => ['config' => ['autoescape' => 'html'], 'extensions' => ['sandbox', 'debug']]]));
$cases[] = ['id' => 'sandbox-optimizer-none', 'source' => "{{ 'x' }}{% for i in foo %}{{ loop.index }}{% endfor %}", 'config' => ['autoescape' => 'html', 'optimizations' => 0], 'extensions' => ['sandbox']];

// ---- node objects PHP shares (the left operand of ??, the arguments of default) and the deep clones that do not ---
$sharingTemplates = [
    'nc-array' => '{{ [a, b] ?? c }}',
    'nc-array-spread' => '{{ [...a] ?? b }}',
    'nc-function-spread' => '{{ max(...a) ?? b }}',
    'nc-array-function-spread' => '{{ [max(...a)] ?? b }}',
    'nc-dynamic-key' => '{{ {(a): b} ?? c }}',
    'nc-escaped-array' => '{{ [x, ...a] ?? b }}',
    'nc-getattr-arguments' => '{{ a.b(c, ...d) ?? e }}',
    'nc-block' => "{{ block('x') ?? 'y' }}{% block x %}{% endblock %}",
    'nc-in-elvis' => '{{ ([...a] ?? b) ?: c }}',
    'nc-chain' => '{{ [...a] ?? [...b] ?? c }}',
    'default-spread' => '{{ x|default(...a) }}',
    'default-array-spread' => '{{ x|default([a, ...b]) }}',
    'default-function-spread' => '{{ x|default(max(...a)) }}',
    'default-no-rewrite' => '{{ (x ~ y)|default(max(...a)) }}',
    'default-nc' => '{{ x|default([...a] ?? b) }}',
    'default-nested' => '{{ x|default(max(...a))|default(min(...b)) }}',
    'default-in-default' => '{{ x|default(y|default(max(...a))) }}',
    'default-getattr-node' => '{{ x[max(...a)]|default(b) }}',
    'nc-in-default-node' => '{{ x[[...a] ?? b]|default(c) }}',
];
foreach ($sharingTemplates as $id => $source) {
    array_push($cases, ...each_config('sharing-'.$id, $source, [
        'sandbox' => ['config' => ['autoescape' => false], 'extensions' => ['sandbox']],
        'sandbox-html' => ['config' => ['autoescape' => 'html'], 'extensions' => ['sandbox']],
        'html' => ['config' => ['autoescape' => 'html']],
    ]));
}

// ---- apply, embed and include interactions ------------------------------------------------------------------------
$interactionTemplates = [
    'apply-nl2br' => '{% apply nl2br %}{{ a }}{% endapply %}',
    'apply-escape' => "{% apply e('js') %}{{ a }}{% endapply %}{% apply upper|e %}x{% endapply %}",
    'apply-raw' => '{% apply raw %}{{ a }}{% endapply %}',
    'apply-in-autoescape' => "{% autoescape 'js' %}{% apply upper %}{{ a }}{% endapply %}{% endautoescape %}",
    'embed-autoescape' => "{% autoescape 'js' %}{% embed 'x' %}{% block b %}{{ a }}{% endblock %}{% endembed %}{% endautoescape %}",
    'embed-with-loop' => "{% for i in x %}{% embed 'y' %}{% block b %}{{ loop.index }}{% endblock %}{% endembed %}{% endfor %}",
    'include-in-loop-sandbox' => "{% for i in x %}{{ include(i) }}{% include i %}{% endfor %}",
    'use-blocks' => "{% use 'x' %}{% block b %}{{ a }}{{ parent() }}{% endblock %}",
    'macro-escape' => "{% macro m(a) %}{{ a }}{{ a|nl2br }}{% endmacro %}{{ _self.m(b) }}",
    'set-capture-autoescape' => "{% autoescape 'js' %}{% set x %}{{ a }}{% endset %}{% endautoescape %}{{ x }}",
];
foreach ($interactionTemplates as $id => $source) {
    array_push($cases, ...each_config('interaction-'.$id, $source, [
        'html' => ['config' => ['autoescape' => 'html']],
        'sandbox' => ['config' => ['autoescape' => 'html'], 'extensions' => ['sandbox']],
    ]));
}

if (false !== $b64 = getenv('CASES_B64')) {
    $cases = json_decode(base64_decode($b64), true, 512, JSON_THROW_ON_ERROR);
}
if (false !== $file = getenv('CASES_FILE')) {
    $cases = json_decode(file_get_contents($file), true, 512, JSON_THROW_ON_ERROR);
}

$out = [];
foreach ($cases as $case) {
    $code = $case['source'];
    $name = $case['name'] ?? 'index';
    $deprecations = [];
    set_error_handler(function (int $type, string $message) use (&$deprecations): bool {
        if (E_USER_DEPRECATED === $type) {
            $deprecations[] = $message;

            return true;
        }

        throw new \ErrorException($message, 0, $type);
    });

    $res = $case;
    unset($res['source']);
    $res = ['id' => $case['id']] + (mb_check_encoding($code, 'UTF-8') ? ['source' => $code] : ['source_b64' => base64_encode($code)]) + $res;
    $error = null;
    $module = null;
    try {
        $config = ['cache' => false] + ($case['config'] ?? []);
        if (isset($case['autoescape_map'])) {
            $map = $case['autoescape_map'];
            $config['autoescape'] = static fn (string $name) => array_key_exists($name, $map) ? $map[$name] : false;
        }
        $env = new Environment(new ArrayLoader(), $config);
        foreach ($case['extensions'] ?? [] as $extension) {
            match ($extension) {
                'sandbox' => $env->addExtension(new SandboxExtension(new SecurityPolicy())),
                'debug' => $env->addExtension(new DebugExtension()),
            };
        }
        if (isset($case['ext'])) {
            $env->addExtension(make_extension($case['ext']));
        }
        $module = $env->parse($env->tokenize(new Source($code, $name)));
    } catch (\Throwable $e) {
        $error = ['class' => get_class($e), 'message' => enc_str($e->getMessage())];
    }
    restore_error_handler();
    if (null !== $error) {
        $res['error'] = $error;
    } else {
        $res['dump'] = enc_str(rs_dump($module));
        if (in_array('sandbox', $case['extensions'] ?? [], true)) {
            $res['security'] = security($module);
        }
    }
    $res['deprecations'] = array_map('enc_str', $deprecations);
    $out[] = $res;
}

echo json_encode($out, JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_PRESERVE_ZERO_FRACTION | JSON_THROW_ON_ERROR), "\n";
