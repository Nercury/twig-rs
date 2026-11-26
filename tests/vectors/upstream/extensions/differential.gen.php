<?php

/*
 * Differential vectors around the ported extension tests (tests/upstream/extensions/differential.rs): the APIs the
 * PHPUnit tests exercise, with more inputs than their assertions use.
 *
 * - "compare": CoreExtension::compare($a, $b) for every pair of a list of scalars and arrays;
 * - "escape": EscaperRuntime::escape($value, $strategy, $charset, $autoescape) for values (strings, scalars, arrays,
 *   Stringable and plain objects, Markup with and without strategies) x strategies (the built-in ones, a custom one,
 *   an unknown one) x charsets x autoescape;
 * - "arguments": calls of host functions and filters (PHP closures with various signatures) through templates, the
 *   arguments the closure received (json_encode(func_get_args())) or the compile error;
 * - "templates": the CoreExtension helpers of CoreTest.php (include, source, cycle, random, reverse, first, last,
 *   slice, keys, in) through templates, with edge inputs: the output or the exception, and the deprecations.
 *
 * Warnings and notices are turned into exceptions, as PHPUnit does.
 *
 * Values are encoded as typed trees: {"t":"null"}, {"t":"bool","v":true}, {"t":"int","v":"42"},
 * {"t":"float","v":"<IEEE-754 bits, big-endian hex>"}, {"t":"string","b64":"..."}, {"t":"array","items":[[key, value],
 * ...]}, {"t":"object","id":"<name of the object in the Rust test>"} for inputs, {"t":"object","class":"...",
 * "string":<encoded string or null>} for results.
 *
 * Regenerate (from the repository root; Git Bash on Windows needs MSYS_NO_PATHCONV=1):
 *
 *     MSYS_NO_PATHCONV=1 docker run --rm -i twig-oracle:3.31.1 php < tests/vectors/upstream/extensions/differential.gen.php > tests/vectors/upstream/extensions/differential.json
 */

require '/oracle/vendor/autoload.php';
require_once '/oracle/vendor/twig/twig/tests/Runtime/EscaperRuntimeTest.php';

use Twig\Environment;
use Twig\Extension\CoreExtension;
use Twig\Loader\ArrayLoader;
use Twig\Markup;
use Twig\Runtime\EscaperRuntime;
use Twig\TwigFilter;
use Twig\TwigFunction;

set_error_handler(static function (int $type, string $message) {
    throw new \ErrorException($message, 0, $type);
});

function encode($value, array $objects = []): array
{
    if (null === $value) {
        return ['t' => 'null'];
    }
    if (\is_bool($value)) {
        return ['t' => 'bool', 'v' => $value];
    }
    if (\is_int($value)) {
        return ['t' => 'int', 'v' => (string) $value];
    }
    if (\is_float($value)) {
        return ['t' => 'float', 'v' => bin2hex(pack('E', $value))];
    }
    if (\is_string($value)) {
        return ['t' => 'string', 'b64' => base64_encode($value)];
    }
    if (\is_array($value)) {
        $items = [];
        foreach ($value as $k => $v) {
            $items[] = [encode($k), encode($v, $objects)];
        }

        return ['t' => 'array', 'items' => $items];
    }
    foreach ($objects as $id => $object) {
        if ($object === $value) {
            return ['t' => 'object', 'id' => $id];
        }
    }

    return ['t' => 'object', 'class' => $value::class, 'string' => $value instanceof \Stringable ? encode((string) $value) : null];
}

function failure(\Throwable $e): array
{
    return ['error' => $e::class, 'message' => $e->getMessage()];
}

$out = [];

// ---- CoreExtension::compare()

$values = [
    0, 1, -1, 42, \PHP_INT_MAX, \PHP_INT_MIN,
    0.0, -0.0, 1.5, 42.0, -42.5, 1e100, \NAN, \INF, -\INF,
    '', '0', '0.0', '-0', '+1', '.5', '5.', 'a', 'abc', 'A', '42', ' 42', '42 ', "\t42", "42\n", "42\v", "\v42", "42\f",
    '42abc', 'abc42', '1e3', '1e1000', '0x1A', 'INF', 'NAN', '9223372036854775808', '00042', '42.0', '١٢', "\0",
    null, true, false, [], [1], [1, 2], ['a' => 1], [0],
];
$results = [];
foreach ($values as $a) {
    $row = [];
    foreach ($values as $b) {
        try {
            $row[] = CoreExtension::compare($a, $b);
        } catch (\Throwable $e) {
            $row[] = failure($e);
        }
    }
    $results[] = $row;
}
$out['compare'] = ['values' => array_map('encode', $values), 'results' => $results];

// ---- EscaperRuntime::escape()

$objects = [
    'stringable' => new \Twig\Tests\Runtime\Extension_TestClass(),
    'plain' => new \stdClass(),
    'markup' => new Markup('<b>&amp;</b>', 'UTF-8'),
    'markup_html' => new Markup('<b>', 'UTF-8', ['html']),
    'markup_js' => new Markup('<b>', 'UTF-8', ['js']),
    'markup_none' => new Markup('<b>', 'UTF-8', []),
    'markup_empty' => new Markup('', 'UTF-8'),
];
$escapeValues = [
    '', 'plain', '<a href="x">\'&amp;</a>', 'é€😀', "\xC3\x28", "\x00\x1F\x7F", '123', ' ', 42, 0, -1.5, true, false, null, [1, 2],
    $objects['stringable'], $objects['plain'], $objects['markup'], $objects['markup_html'], $objects['markup_js'],
    $objects['markup_none'], $objects['markup_empty'],
];
$cases = [];
foreach ($escapeValues as $value) {
    foreach (['html', 'js', 'css', 'url', 'html_attr', 'html_attr_relaxed', 'foo', 'bar'] as $strategy) {
        foreach ([null, 'ISO-8859-1'] as $charset) {
            foreach ([false, true] as $autoescape) {
                $runtime = new EscaperRuntime();
                $runtime->setEscaper('foo', static fn ($s, $c) => get_debug_type($s).':'.(\is_scalar($s) || null === $s ? (string) $s : 'x').'@'.$c);
                $case = ['value' => encode($value, $objects), 'strategy' => $strategy, 'charset' => $charset, 'autoescape' => $autoescape];
                try {
                    $case['result'] = encode($runtime->escape($value, $strategy, $charset, $autoescape), $objects);
                } catch (\Throwable $e) {
                    $case += failure($e);
                }
                $cases[] = $case;
            }
        }
    }
}
$out['escape'] = $cases;

// ---- argument binding of host callables

$signatures = [
    // id => [type, callable, options]
    'abc' => ['function', static fn ($a, $b = 'B', $c = 'C') => json_encode(\func_get_args()), []],
    'rest' => ['function', static fn ($a, ...$rest) => json_encode(\func_get_args()), []],
    'bucket' => ['function', static fn ($a, $b = null, array $opts = []) => json_encode(\func_get_args()), ['is_variadic' => true]],
    'names' => ['function', static fn ($someName, $other_name = 2) => json_encode(\func_get_args()), []],
    'variadic_rest' => ['function', static fn ($a, $b = 'B', ...$rest) => json_encode(\func_get_args()), ['is_variadic' => true]],
    'only_rest' => ['function', static fn (...$values) => json_encode(\func_get_args()), []],
    'filter' => ['filter', static fn ($value, $x = 1, $y = 2) => json_encode(\func_get_args()), []],
    'filter_bucket' => ['filter', static fn ($value, $x = 1, array $more = []) => json_encode(\func_get_args()), ['is_variadic' => true]],
];
$calls = [
    '()', '(1)', '(1, 2)', '(1, 2, 3)', '(1, 2, 3, 4)', '(a: 1)', '(1, c: 3)', '(c: 3, a: 1)', '(1, b: 2, c: 3)',
    '(b: 2)', '(1, a: 2)', '(a: 1, 2)', '(1, d: 4)', '(1, d: 4, e: 5)', '(1, B: 2)', '(A: 1)', '(1, rest: 2)',
    '(1, opts: [2])', '(some_name: 1)', '(someName: 1)', '(somename: 1)', '(other_name: 3, some_name: 1)',
    '(otherName: 3, someName: 1)', '(1, other_name: 3)', '(1, x: 3)', '(y: 3)', '(1, more: 3)', '(x: 2, z: 3)',
    '(value: 1)', '(1, ...[2, 3])', '(...{a: 1, c: 3})', '(values: 1)', '(1, z: 3, b: 2)', '(1, 2, b: 3)',
    '(1, d_e: 4)', '(1, dE: 4)', '(1, 2, 3, d: 4)',
];
$cases = [];
foreach ($signatures as $id => [$type, $callable, $options]) {
    foreach ($calls as $call) {
        // the comment makes the source (and so the compiled template class, which PHP keeps for the whole process)
        // unique per signature: the argument binding is compiled into the class
        $template = '{# '.$id.' #}'.('filter' === $type ? '{{ 0|f'.$call.' }}' : '{{ f'.$call.' }}');
        $twig = new Environment(new ArrayLoader(['index' => $template]), ['autoescape' => false, 'cache' => false]);
        $twig->addFunction(new TwigFunction('f', $callable, $options));
        $twig->addFilter(new TwigFilter('f', $callable, $options));
        $deprecations = [];
        set_error_handler(static function (int $type, string $message) use (&$deprecations) {
            if (\E_USER_DEPRECATED === $type) {
                $deprecations[] = $message;

                return true;
            }
            throw new \ErrorException($message, 0, $type);
        });
        $case = ['signature' => $id, 'template' => $template];
        try {
            $case['output'] = $twig->load('index')->render([]);
        } catch (\Throwable $e) {
            $case += failure($e);
        } finally {
            restore_error_handler();
        }
        $case['deprecations'] = $deprecations;
        $cases[] = $case;
    }
}
$out['arguments'] = $cases;

// ---- the CoreExtension helpers of CoreTest.php through templates, with edge inputs

$templates = [
    "{{ include('x', sandboxed: false) }}", "{{ include('x', {}, true, false, false) }}", "{{ include('x', {}, true, false) }}",
    "{{ include(['missing', 'x']) }}", "{{ include('missing', ignore_missing: true) }}|", "{{ include('x')|length }}",
    "{{ include('empty') is same as('') ? 'empty' : 'other' }}", "{{ include('x') is same as('X') ? 'string' : 'markup' }}",
    "{{ include('missing') }}", "{{ source('x') }}", "{{ source('missing', ignore_missing: true) }}|", "{{ source('missing') }}",
    '{{ cycle([1, 2, 3], -1) }}', '{{ cycle({a: 1, b: 2}, 0) }}', "{{ cycle('abc', 1) }}", '{{ cycle([1, 2], 1.5) }}',
    '{{ cycle([1, 2], "1") }}', '{{ cycle([1, 2], true) }}', '{{ cycle(null, 0) }}', '{{ cycle([[1], 2], 0)|join }}',
    '{{ random(5, 5) }}', '{{ random(0) }}', "{{ random('a') }}", '{{ random(true) }}', '{{ random(null, 0) }}',
    '{{ random(10, 5) }}', '{{ random(-3, -3) }}', '{{ random(2.5, 2.9) }}', '{{ random([7]) }}', '{{ random({a: 8}) }}',
    "{{ random('é') }}", "{{ random('9', 9) }}",
    "{{ 'abc'|reverse }}", "{{ 'é😀a'|reverse }}", '{{ [1, 2]|reverse|join }}', "{{ {a: 1, 2: 'b'}|reverse(true)|json_encode }}",
    "{{ {a: 1, 2: 'b'}|reverse|json_encode }}", '{{ null|reverse }}|', '{{ 12|reverse }}', '{{ 1.5|reverse }}', '{{ true|reverse }}',
    "{{ 'abc'|first }}{{ 'abc'|last }}", '{{ 123|first }}{{ 123|last }}', "{{ [] |first is same as(false) ? 'false' : 'other' }}",
    "{{ {}|last is same as(false) ? 'false' : 'other' }}", '{{ {a: 1, b: 2}|first }}{{ {a: 1, b: 2}|last }}',
    "{{ 'é😀'|first }}|{{ 'é😀'|last }}", '{{ null|first }}|{{ null|last }}', '{{ 1.5|first }}|{{ 1.5|last }}',
    "{{ 'abcdef'|slice(-2) }}", "{{ 'abcdef'|slice(1, -1) }}", '{{ [1, 2, 3]|slice(-2, 1)|join }}',
    '{{ {a: 1, b: 2, 5: 3}|slice(1)|json_encode }}', '{{ {a: 1, b: 2, 5: 3}|slice(1, null, true)|json_encode }}',
    '{{ 12345|slice(1, 2) }}', '{{ null|slice(0) }}|', "{{ 'abc'[1:] }}", '{{ [1, 2, 3][:2]|join }}', "{{ 'é😀abc'|slice(1, 2) }}",
    "{{ 'abc'|slice(10) }}|", '{{ [1, 2]|slice(5)|json_encode }}', "{{ 'abc'|slice(1.5) }}", "{{ 'abc'|slice('1') }}",
    "{{ 'abc'|keys|json_encode }}", '{{ {a: 1, 3: 2}|keys|json_encode }}', '{{ null|keys|json_encode }}',
    "{{ 'a' in 'abc' ? 'y' : 'n' }}", "{{ '' in 'abc' ? 'y' : 'n' }}", "{{ 1 in '123' ? 'y' : 'n' }}", "{{ 1.5 in '1.5' ? 'y' : 'n' }}",
    "{{ null in 'abc' ? 'y' : 'n' }}", "{{ true in [1] ? 'y' : 'n' }}", "{{ '1e1' in [10] ? 'y' : 'n' }}", "{{ 'abc' in ['ABC'] ? 'y' : 'n' }}",
    "{{ [1] in [[1]] ? 'y' : 'n' }}", "{{ 'a' in {x: 'a'} ? 'y' : 'n' }}", "{{ 'x' in {x: 'a'} ? 'y' : 'n' }}", "{{ 0 in ['a'] ? 'y' : 'n' }}",
    "{{ '0' in [false] ? 'y' : 'n' }}", "{{ null in [false] ? 'y' : 'n' }}", "{{ 'abc' in null ? 'y' : 'n' }}", "{{ 1 in 1 ? 'y' : 'n' }}",
    "{{ ' 1' in [1] ? 'y' : 'n' }}", "{{ '1 ' in [1] ? 'y' : 'n' }}", "{{ 'INF' in [1e1000] ? 'y' : 'n' }}",
];
$cases = [];
foreach ($templates as $template) {
    $twig = new Environment(new ArrayLoader(['index' => $template, 'x' => 'X', 'empty' => '']), ['autoescape' => false, 'cache' => false]);
    $deprecations = [];
    set_error_handler(static function (int $type, string $message) use (&$deprecations) {
        if (\E_USER_DEPRECATED === $type || \E_DEPRECATED === $type) {
            $deprecations[] = $message;

            return true;
        }
        throw new \ErrorException($message, 0, $type);
    });
    mt_srand(1);
    $case = ['template' => $template];
    try {
        $case['output'] = $twig->load('index')->render([]);
    } catch (\Throwable $e) {
        $case += failure($e);
    } finally {
        restore_error_handler();
    }
    $case['deprecations'] = $deprecations;
    $cases[] = $case;
}
$out['templates'] = $cases;

echo json_encode($out, \JSON_UNESCAPED_SLASHES | \JSON_UNESCAPED_UNICODE | \JSON_INVALID_UTF8_SUBSTITUTE | \JSON_PRESERVE_ZERO_FRACTION), "\n";
