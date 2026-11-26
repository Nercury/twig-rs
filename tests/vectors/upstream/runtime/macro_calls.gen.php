<?php

/*
 * Generator of tests/vectors/upstream/runtime/macro_calls.json: a sweep of TWIG/tests/TwigMacroTest.php's subject,
 * TwigMacro::callLegacy() (Twig 3.31.1): macro definitions (required, defaulted, reserved and variadic arguments)
 * crossed with calls (positional, named, unknown, duplicated, misordered and spread arguments). Each case is
 * "index.twig" declaring the macro, whose body passes its arguments to capture(), and calling it on line 7 with
 * `_self.test(...)`, exactly like the TwigMacroTest port.
 *
 * Regenerate from the repository root (Git Bash on Windows needs MSYS_NO_PATHCONV=1):
 *
 *     MSYS_NO_PATHCONV=1 docker run --rm -i twig-oracle:3.31.1 php < tests/vectors/upstream/runtime/macro_calls.gen.php > tests/vectors/upstream/runtime/macro_calls.json
 *
 * Output: [[template, kind, text, deprecations]]: kind "value" (text = the captured arguments, described like
 * get_attribute_sweep.gen.php does) or "error" (text = "Class: message").
 */

declare(strict_types=0);

require '/oracle/vendor/autoload.php';

use Twig\Environment;
use Twig\Loader\ArrayLoader;
use Twig\TwigFunction;

function describe($value): string
{
    if (is_object($value)) {
        return 'object('.get_class($value).')';
    }
    if (is_array($value)) {
        $parts = [];
        foreach ($value as $key => $item) {
            $parts[] = var_export($key, true).'=>'.describe($item);
        }

        return 'array('.implode(',', $parts).')';
    }

    return var_export($value, true);
}

// [definition, the variables the body captures]
$definitions = [
    ['test()', ['varargs']],
    ['test(a)', ['a', 'varargs']],
    ["test(a = 'da')", ['a', 'varargs']],
    ['test(a, b)', ['a', 'b', 'varargs']],
    ["test(a, b = 'db')", ['a', 'b', 'varargs']],
    ["test(a = 'da', b = 'db')", ['a', 'b', 'varargs']],
    ["test(a, b = 'db', c = 'dc')", ['a', 'b', 'c', 'varargs']],
    ['test(a, ...rest)', ['a', 'rest']],
    ["test(a = 'da', ...rest)", ['a', 'rest']],
    ['test(...rest)', ['rest']],
    ['test(context, macros)', ['context', 'macros', 'varargs']],
    ["test(blocks = 'dblocks', this = 'dthis')", ['blocks', 'this', 'varargs']],
    ['test(a, varargs)', ['a', 'varargs']],
    ['test(a, ...varargs)', ['a', 'varargs']],
    ['test(a, ...context)', ['a', 'context']],
];
$calls = [
    '()', "('x')", "('x', 'y')", "('x', 'y', 'z')", "('x', 'y', 'z', 'w')",
    "(a='x')", "(b='y')", "(a='x', b='y')", "(b='y', a='x')", "(c='z')", "(unknown='u')", "(a='x', unknown='u')",
    "(context='cx')", "(macros='m')", "(blocks='bl')", "(this='t')", "(varargs='va')", "(rest='r')",
    "('x', b='y')", "('x', a='y')", "('x', unknown='u')", "(a='x', 'y')", "('x', 'y', c='z')", "('x', c='z')",
    "(b='y', 'x')", "(a=null)", "('x', b=null)", "(...['x', 'y'])", "(...{a: 'x'})", "('x', ...{b: 'y'})",
    "(...{unknown: 'u'})", "(a: 'x', b: 'y')", "('x', 'y', unknown='u', other='o')",
];

$captured = [];
$capture = new TwigFunction('capture', static function ($value) use (&$captured) {
    $captured[] = describe($value);

    return '';
});

$out = [];
foreach ($definitions as [$definition, $variables]) {
    $body = '{{ capture(['.implode(', ', $variables).']) }}';
    foreach ($calls as $call) {
        $template = "{% macro $definition %}$body{% endmacro %}\n\n\n\n\n\n{{ _self.test$call }}";
        $twig = new Environment(new ArrayLoader(['index.twig' => $template]));
        $twig->addFunction($capture);
        $captured = [];
        $deprecations = [];
        set_error_handler(static function (int $type, string $message) use (&$deprecations): bool {
            $deprecations[] = (E_USER_DEPRECATED === $type ? 'UserDeprecated: ' : 'Other: ').$message;

            return true;
        });
        try {
            $twig->render('index.twig');
            $out[] = [$template, 'value', implode('|', $captured), $deprecations];
        } catch (\Throwable $e) {
            $out[] = [$template, 'error', get_class($e).': '.$e->getMessage(), $deprecations];
        } finally {
            restore_error_handler();
        }
    }
}

echo json_encode($out, JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_THROW_ON_ERROR), "\n";
