<?php

/*
 * Generator of tests/vectors/upstream/runtime/error_lines.json: a sweep of the subject of TWIG/tests/ErrorTest.php,
 * the template name and line Twig 3.31.1 attributes to an error: `foo.bar` failing (a missing variable in strict
 * mode, or ErrorTest_Foo::bar() throwing) inside every composition construct (include, embed, extends, parent(),
 * block(), traits, macros, captures, loops, filters with arrow functions, multi-line expressions, ...), with debug on
 * and off.
 *
 * Regenerate from the repository root (Git Bash on Windows needs MSYS_NO_PATHCONV=1):
 *
 *     MSYS_NO_PATHCONV=1 docker run --rm -i twig-oracle:3.31.1 php < tests/vectors/upstream/runtime/error_lines.gen.php > tests/vectors/upstream/runtime/error_lines.json
 *
 * Output: {"constructs": {name: templates}, "cases": [[construct, mode, debug, kind, text, line, source, previous]]}
 * where mode is "missing" (strict, no foo), "exception" (strict, foo = ErrorTest_Foo) or "lax_exception"; kind is
 * "value" (text: the output) or "error" (text: "Class: message", line: getTemplateLine(), source: the name of
 * getSourceContext(), previous: "Class: message" of getPrevious() or null).
 */

declare(strict_types=0);

require '/oracle/vendor/autoload.php';

use Twig\Environment;
use Twig\Loader\ArrayLoader;
use Twig\Tests\ErrorTest;
use Twig\Tests\ErrorTest_Foo;

class_exists(ErrorTest::class); // declares ErrorTest_Foo

$constructs = [
    'plain' => ['index' => "\n\n{{ foo.bar }}\n"],
    'multiline_expression' => ['index' => "{{\nfoo\n.\nbar\n}}"],
    'concat_lines' => ['index' => "{{ 'a'\n~ foo.bar\n~ 'b' }}"],
    'set' => ['index' => "a\n{% set x = foo.bar %}\n{{ x }}"],
    'set_capture' => ['index' => "{% set x %}\n\n{{ foo.bar }}{% endset %}{{ x }}"],
    'do' => ['index' => "\n{% do foo.bar %}"],
    'if' => ['index' => "{% if true %}\n{{ foo.bar }}\n{% endif %}"],
    'if_condition' => ['index' => "{% if\nfoo.bar %}x{% endif %}"],
    'for' => ['index' => "{% for i in [1, 2] %}\n{{ foo.bar }}{% endfor %}"],
    'for_sequence' => ['index' => "\n{% for i in foo.bar %}{{ i }}{% endfor %}"],
    'for_else' => ['index' => "{% for i in [] %}{% else %}\n\n{{ foo.bar }}{% endfor %}"],
    'with' => ['index' => "{% with {a: 1} %}\n{{ foo.bar }}{% endwith %}"],
    'with_only' => ['index' => "{% with {foo: foo} only %}\n{{ foo.bar }}{% endwith %}"],
    'apply' => ['index' => "{% apply upper %}\n{{ foo.bar }}{% endapply %}"],
    'filter_arrow' => ['index' => "{{ [1]\n|map(x =>\nfoo.bar)\n|join }}"],
    'filter_argument' => ['index' => "{{ 'x'|replace({\n'x': foo.bar}) }}"],
    'function_argument' => ['index' => "{{ max(1,\nfoo.bar) }}"],
    'ternary' => ['index' => "{{ true\n? foo.bar\n: 'no' }}"],
    'include' => ['index' => "{% include 'p' %}", 'p' => "\n{{ foo.bar }}"],
    'include_with' => ['index' => "x\n{% include 'p' with {a: 1} %}", 'p' => "{{ a }}\n\n{{ foo.bar }}"],
    'include_function' => ['index' => "\n{{ include('p') }}", 'p' => "\n\n\n{{ foo.bar }}"],
    'include_in_block' => ['index' => "{% extends 'base' %}{% block c %}\n{% include 'p' %}{% endblock %}", 'base' => '{% block c %}{% endblock %}', 'p' => "\n{{ foo.bar }}"],
    'extends_block' => ['index' => "{% extends 'base' %}\n{% block c %}\n\n{{ foo.bar }}{% endblock %}", 'base' => "{% block c %}{% endblock %}"],
    'parent_call' => ['index' => "{% extends 'base' %}{% block c %}\n{{ parent() }}{% endblock %}", 'base' => "\n{% block c %}{{ foo.bar }}{% endblock %}"],
    'parent_of_parent' => ['index' => "{% extends 'middle' %}{% block c %}{{ parent() }}{% endblock %}", 'middle' => "{% extends 'base' %}{% block c %}\n{{ parent() }}{% endblock %}", 'base' => "\n\n{% block c %}{{ foo.bar }}{% endblock %}"],
    'block_function' => ['index' => "{% block c %}\n{{ foo.bar }}{% endblock %}\n{{ block('c') }}"],
    'block_of_other_template' => ['index' => "\n{{ block('c', 'other') }}", 'other' => "{% block c %}\n\n{{ foo.bar }}{% endblock %}"],
    'nested_block' => ['index' => "{% block outer %}\n{% block inner %}\n{{ foo.bar }}{% endblock %}{% endblock %}"],
    'trait_block' => ['index' => "{% use 't' %}\n{{ block('c') }}", 't' => "{% block c %}\n{{ foo.bar }}{% endblock %}"],
    'trait_parent' => ['index' => "{% use 't' %}{% block c %}\n{{ parent() }}{% endblock %}", 't' => "{% block c %}\n\n{{ foo.bar }}{% endblock %}"],
    'embed' => ['index' => "\n{% embed 'e' %}{% endembed %}", 'e' => "{% block c %}\n{{ foo.bar }}{% endblock %}"],
    'embed_override' => ['index' => "{% embed 'e' %}\n{% block c %}\n{{ foo.bar }}{% endblock %}{% endembed %}", 'e' => "{% block c %}{% endblock %}"],
    'macro_self' => ['index' => "{% macro m(foo) %}\n{{ foo.bar }}{% endmacro %}\n\n{{ _self.m(foo) }}"],
    'macro_imported' => ['index' => "{% import 'macros' as h %}\n{{ h.m(foo) }}", 'macros' => "{% macro m(foo) %}\n\n{{ foo.bar }}{% endmacro %}"],
    'macro_from' => ['index' => "{% from 'macros' import m %}\n\n{{ m(foo) }}", 'macros' => "\n{% macro m(foo) %}{{ foo.bar }}{% endmacro %}"],
    'macro_in_block' => ['index' => "{% extends 'base' %}{% import 'macros' as h %}{% block c %}\n{{ h.m(foo) }}{% endblock %}", 'base' => '{% block c %}{% endblock %}', 'macros' => "{% macro m(foo) %}\n{{ foo.bar }}{% endmacro %}"],
    'dynamic_extends' => ['index' => "\n{% extends foo.bar %}"],
    'dynamic_include' => ['index' => "\n\n{% include foo.bar %}"],
    'deep_include' => ['index' => "{% include 'a' %}", 'a' => "\n{% include 'b' %}", 'b' => "\n\n{% include 'c' %}", 'c' => "{{ foo.bar }}"],
    'embed_in_include' => ['index' => "{% include 'a' %}", 'a' => "\n{% embed 'e' %}{% block c %}\n{{ foo.bar }}{% endblock %}{% endembed %}", 'e' => '{% block c %}{% endblock %}'],
];

$out = [];
foreach ($constructs as $name => $templates) {
    foreach (['missing', 'exception', 'lax_exception'] as $mode) {
        foreach ([false, true] as $debug) {
            $twig = new Environment(new ArrayLoader($templates), ['strict_variables' => 'lax_exception' !== $mode, 'debug' => $debug]);
            $context = 'missing' === $mode ? [] : ['foo' => new ErrorTest_Foo()];
            try {
                $output = $twig->render('index', $context);
                $out[] = [$name, $mode, $debug, 'value', $output, null, null, null];
            } catch (\Throwable $e) {
                $twigError = $e instanceof \Twig\Error\Error;
                $previous = $e->getPrevious();
                $out[] = [
                    $name, $mode, $debug, 'error', get_class($e).': '.$e->getMessage(),
                    $twigError ? $e->getTemplateLine() : null,
                    $twigError ? $e->getSourceContext()?->getName() : null,
                    $previous ? get_class($previous).': '.$previous->getMessage() : null,
                ];
            }
        }
    }
}

echo json_encode(['constructs' => $constructs, 'cases' => $out], JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_THROW_ON_ERROR), "\n";
