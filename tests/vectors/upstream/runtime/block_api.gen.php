<?php

/*
 * Generator of tests/vectors/upstream/runtime/block_api.json: a sweep of the TemplateWrapper block API of Twig 3.31.1
 * (the subject of TWIG/tests/TemplateWrapperTest.php and the block tests of TemplateTest.php/CallMacroTest.php):
 * renderBlock(), hasBlock() and getBlockNames() on template families (inheritance, dynamic parents, traits with
 * aliases, nested blocks, embeds, macros, parent() and block() calls, strict variables), with several block names
 * and contexts, in lax and strict environments with a global. One environment serves the operations of a family,
 * mode and context, in the order of the output.
 *
 * Regenerate from the repository root (Git Bash on Windows needs MSYS_NO_PATHCONV=1):
 *
 *     MSYS_NO_PATHCONV=1 docker run --rm -i twig-oracle:3.31.1 php < tests/vectors/upstream/runtime/block_api.gen.php > tests/vectors/upstream/runtime/block_api.json
 *
 * Output: [{"family", "strict", "op", "block", "context", "kind", "text", "line", "source", "deprecations"}] where
 * kind is "value" (text: the rendered block, "true"/"false", or the block names joined by ",") or "error" (text:
 * "Class: message", line: getTemplateLine(), source: the name of getSourceContext() or null).
 */

declare(strict_types=0);

require '/oracle/vendor/autoload.php';

use Twig\Environment;
use Twig\Loader\ArrayLoader;

$families = [
    'simple' => [
        'index' => "{% block a %}A{{ v }}{% endblock %}\n{% block b %}B{{ g }}{% endblock %}",
    ],
    'extends' => [
        'index' => "{% extends 'base' %}\n{% block a %}childA[{{ parent() }}]{% endblock %}",
        'base' => "{% block a %}baseA{% endblock %}{% block b %}baseB{{ block('a') }}{% endblock %}",
    ],
    'three_levels' => [
        'index' => "{% extends 'middle' %}{% block a %}1[{{ parent() }}]{% endblock %}",
        'middle' => "{% extends 'base' %}{% block a %}2[{{ parent() }}]{% endblock %}{% block m %}M{% endblock %}",
        'base' => "{% block a %}3{% endblock %}{% block b %}B{% endblock %}",
    ],
    'dynamic_parent' => [
        'index' => "{% extends layout %}{% block a %}A{{ parent() }}{% endblock %}",
        'layout1' => "{% block x %}X1{% endblock %}{% block a %}LA1{% endblock %}",
        'layout2' => "{% block a %}LA2{% endblock %}{% block y %}Y2{% endblock %}",
    ],
    'traits' => [
        'index' => "{% use 't' with a as ta %}{% use 'u' %}{% block a %}A[{{ parent() }}]{% endblock %}",
        't' => "{% block a %}TA{% endblock %}{% block c %}TC{% endblock %}",
        'u' => "{% block d %}UD{{ block('ta') }}{% endblock %}{% block c %}UC{% endblock %}",
    ],
    'nested' => [
        'index' => "{% block outer %}O{% block inner %}I{{ v }}{% endblock %}{% endblock %}",
    ],
    'parent_without_parent' => [
        'index' => "\n{% block a %}{{ parent() }}{% endblock %}",
    ],
    'missing_block_call' => [
        'index' => "{% block a %}\n{{ block('missing') }}{% endblock %}{% block b %}{{ block('missing') is defined ? 'y' : 'n' }}{% endblock %}",
    ],
    'embed' => [
        'index' => "{% embed 'e' %}{% block a %}EA{% endblock %}{% endembed %}{% block z %}Z{% endblock %}",
        'e' => "{% block a %}ea{% endblock %}{% block f %}ef{% endblock %}",
    ],
    'strict_error' => [
        'index' => "{% block a %}\n\n{{ missing.attr }}{% endblock %}{% block b %}{{ v.attr }}{% endblock %}",
    ],
    'macros' => [
        'index' => "{% macro m(x) %}<{{ x }}>{% endmacro %}{% block a %}{{ _self.m(v) }}{% endblock %}{% block b %}{% import _self as s %}{{ s.m('i') }}{% endblock %}",
    ],
    'dynamic_parent_error' => [
        'index' => "{% extends layout|upper %}{% block a %}A{% endblock %}",
        'LAYOUT1' => "{% block x %}X{% endblock %}",
    ],
    'parent_block_call' => [
        'index' => "{% extends 'base' %}{% block a %}{{ block('b') }}{% endblock %}{% block b %}childB{% endblock %}",
        'base' => "{% block a %}baseA{% endblock %}{% block b %}baseB{% endblock %}{% block c %}{{ block('a') }}{% endblock %}",
    ],
    'set_capture' => [
        'index' => "{% set x %}{% block a %}A{% endblock %}{% endset %}{% block b %}[{{ x|default('nox') }}]{% endblock %}",
    ],
    'globals_and_context' => [
        'index' => "{% block a %}{{ g }}|{{ v|default('none') }}|{{ _context|keys|sort|join(',') }}{% endblock %}",
    ],
];

$blocks = ['a', 'b', 'c', 'd', 'ta', 'x', 'y', 'm', 'outer', 'inner', 'z', 'f', 'missing'];
$contexts = [
    'empty' => [],
    'v' => ['v' => 'V'],
    'layout1' => ['layout' => 'layout1', 'v' => 'V'],
    'layout2' => ['layout' => 'layout2'],
    'layout_missing' => ['layout' => 'nope'],
    'g_override' => ['g' => 'local'],
];

$out = [];
foreach ($families as $family => $templates) {
    foreach ([false, true] as $strict) {
        foreach ($contexts as $contextName => $context) {
            // one environment per family, mode and context: the operations run in this order on it
            $twig = new Environment(new ArrayLoader($templates), ['strict_variables' => $strict]);
            $twig->addGlobal('g', 'G');
            foreach (['blocks', 'has', 'render'] as $op) {
                foreach ($op === 'blocks' ? [null] : $blocks as $block) {
                    $deprecations = [];
                    set_error_handler(static function (int $type, string $message) use (&$deprecations): bool {
                        $deprecations[] = (E_USER_DEPRECATED === $type ? 'UserDeprecated: ' : 'Other: ').$message;

                        return true;
                    });
                    $row = ['family' => $family, 'strict' => $strict, 'op' => $op, 'block' => $block, 'context' => $contextName];
                    try {
                        $wrapper = $twig->load('index');
                        $result = match ($op) {
                            'blocks' => implode(',', $wrapper->getBlockNames($context)),
                            'has' => $wrapper->hasBlock($block, $context) ? 'true' : 'false',
                            'render' => $wrapper->renderBlock($block, $context),
                        };
                        $row += ['kind' => 'value', 'text' => $result, 'line' => null, 'source' => null];
                    } catch (\Throwable $e) {
                        $row += [
                            'kind' => 'error',
                            'text' => get_class($e).': '.$e->getMessage(),
                            'line' => $e instanceof \Twig\Error\Error ? $e->getTemplateLine() : null,
                            'source' => $e instanceof \Twig\Error\Error ? $e->getSourceContext()?->getName() : null,
                        ];
                    } finally {
                        restore_error_handler();
                    }
                    $row['deprecations'] = $deprecations;
                    $out[] = $row;
                }
            }
        }
    }
}

echo json_encode(['families' => $families, 'contexts' => $contexts, 'cases' => $out], JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_THROW_ON_ERROR), "\n";
