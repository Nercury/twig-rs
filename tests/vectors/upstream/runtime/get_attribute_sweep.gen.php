<?php

/*
 * Generator of tests/vectors/upstream/runtime/get_attribute_sweep.json: a wider sweep of the getAttribute() matrix of
 * Twig 3.31.1's tests/TemplateTest.php. Every helper object of the test (plus arrays, scalars and a closure) is
 * combined with a list of items (names, getter/isser/hasser names in several cases, numbers, numeric strings,
 * booleans, null) and every call type, then rendered through templates in six modes:
 *
 *     lax, strict                  {{ capture(o.(i)) }} / {{ capture(o.(i)()) }} / {{ capture(o[i]) }}
 *     defined_lax, defined_strict  the same expression followed by "is defined"
 *     sandbox_none                 lax, SandboxExtension (sandboxed) with a strict-off policy allowing only "capture"
 *     sandbox_some                 lax, sandboxed, a policy allowing some properties and methods (see $policy)
 *
 * The sandbox modes do not autoescape (no "escape" filter to allow). "capture" records a description of its
 * argument: var_export() for scalars, "object(Class)" for objects, arrays recursively.
 *
 * Regenerate from the repository root (Git Bash on Windows needs MSYS_NO_PATHCONV=1):
 *
 *     MSYS_NO_PATHCONV=1 docker run --rm -i twig-oracle:3.31.1 php < tests/vectors/upstream/runtime/get_attribute_sweep.gen.php > tests/vectors/upstream/runtime/get_attribute_sweep.json
 *
 * Output: {"objects": [names], "items": [var_export], "rows": [[object index, item index, type, {mode: result}]]}
 * where a result is [kind, text, diagnostics]: kind "value" (text = description) or "error" (text = "Class:
 * message"); diagnostics lists "Warning: ...", "Deprecated: ..." and "UserDeprecated: ..." in order.
 */

declare(strict_types=0);

require '/oracle/vendor/autoload.php';

use Twig\Environment;
use Twig\Extension\SandboxExtension;
use Twig\Loader\ArrayLoader;
use Twig\Sandbox\SecurityPolicy;
use Twig\Tests\TemplateTest;
use Twig\TwigFunction;

class_exists(TemplateTest::class); // declares the helper classes

$diagnostics = [];
set_error_handler(static function (int $type, string $message) use (&$diagnostics): bool {
    if (!(error_reporting() & $type)) {
        return false;
    }
    $label = match ($type) {
        E_WARNING => 'Warning',
        E_NOTICE => 'Notice',
        E_DEPRECATED => 'Deprecated',
        E_USER_DEPRECATED => 'UserDeprecated',
        default => 'Other',
    };
    $diagnostics[] = $label.': '.$message;

    return true;
});

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

$array = [
    'defined' => 'defined',
    'zero' => 0,
    'null' => null,
    '1' => 1,
    'bar' => true,
    'foo' => true,
    'baz' => 'baz',
    'baf' => 'baf',
    '09' => '09',
    '+4' => '+4',
];
$objects = [
    'array' => $array,
    'empty_array' => [],
    'list' => ['a', 'b'],
    'TemplateArrayAccessObject' => new Twig\Tests\TemplateArrayAccessObject(),
    'ArrayObject' => new \ArrayObject($array),
    'stdClass' => (object) $array,
    'TemplateMagicPropertyObject' => new Twig\Tests\TemplateMagicPropertyObject(),
    'TemplateMethodObject' => new Twig\Tests\TemplateMethodObject(),
    'TemplatePropertyObject' => new Twig\Tests\TemplatePropertyObject(),
    'TemplatePropertyObjectAndIterator' => new Twig\Tests\TemplatePropertyObjectAndIterator(),
    'TemplatePropertyObjectAndArrayAccess' => new Twig\Tests\TemplatePropertyObjectAndArrayAccess(),
    'TemplatePropertyObjectDefinedWithUndefinedValue' => new Twig\Tests\TemplatePropertyObjectDefinedWithUndefinedValue(),
    'TemplateMagicMethodObject' => new Twig\Tests\TemplateMagicMethodObject(),
    'TemplateMagicMethodExceptionObject' => new Twig\Tests\TemplateMagicMethodExceptionObject(),
    'TemplateMethodAndPropObject' => new Twig\Tests\TemplateMethodAndPropObject(),
    'TemplateArrayAccess' => new Twig\Tests\TemplateArrayAccess(),
    'TemplateGetIsMethods' => new Twig\Tests\TemplateGetIsMethods(),
    'TemplateAttributeResolution' => new Twig\Tests\TemplateAttributeResolution(),
    'TemplateMagicPrivatePropertyObject' => new Twig\Tests\TemplateMagicPrivatePropertyObject(),
    'closure' => static fn (): string => 'closure called',
    'null' => null,
    'int' => 42,
    'string' => 'string',
    'true' => true,
    'float' => 1.5,
];
$items = [
    'defined', 'undefined', 'protected', 'zero', 1, 1.0, 'null', 'bar', 'foo', 'baz', 'baf', '09', '+4',
    'BAR', 'Baz', 'DEFINED', 'getDefined', 'GETDEFINED', 'isBar', 'hasFoo', 'hasBaz', 'isBaz', 'getBaz', 'isBaf',
    'hasBaf', 'static', 'getStatic', 'getProtected', 'get1', 'get09', '__call', '__invoke', '__isset', '__get',
    'NAME', 'name', 'hidden', 'getHidden', 'secret', 'title', 'a', 'A', 'b', 'B', 'c', 'getC', 'vars', 'attributes',
    'data', 'children', 'get', 'is', 'has', 'offsetGet', 'offsetExists', 'getIterator', 0, '0', '', null, true,
    false, -1, -10, 1.5, '1.5', '01', ' 1', '1 ', 2,
];
$types = [
    'any' => 'o.(i)',
    'method' => 'o.(i)()',
    'array' => 'o[i]',
];

$policy = new SecurityPolicy([], [], [
    'Twig\Tests\TemplateMethodObject' => ['getDefined', 'isBar', 'baz', 'getStatic'],
    'Twig\Tests\TemplateMagicMethodObject' => ['undefined', 'bar'],
    'Twig\Tests\TemplateMethodAndPropObject' => ['getA'],
    'Twig\Tests\TemplateAttributeResolution' => ['getHidden'],
    'Closure' => ['__invoke'],
    'ArrayObject' => ['offsetGet'],
], [
    'Twig\Tests\TemplatePropertyObject' => ['defined', 'zero'],
    'Twig\Tests\TemplatePropertyObjectAndArrayAccess' => ['defined', 'bar'],
    'Twig\Tests\TemplateArrayAccessObject' => ['defined', 'zero', '1', 'attributes'],
    'Twig\Tests\TemplateMagicPropertyObject' => ['zero', 'defined'],
    'Twig\Tests\TemplateMethodAndPropObject' => ['b', 'a'],
    'Twig\Tests\TemplateMagicPrivatePropertyObject' => ['title'],
    'stdClass' => ['defined', 'bar', 'null'],
], ['capture']);

$captured = null;
$capture = new TwigFunction('capture', static function ($value) use (&$captured) {
    $captured = $value;

    return '';
});

$modes = [
    'lax' => [false, false, null],
    'strict' => [true, false, null],
    'defined_lax' => [false, true, null],
    'defined_strict' => [true, true, null],
    'sandbox_none' => [false, false, new SecurityPolicy([], [], [], [], ['capture'])],
    'sandbox_some' => [false, false, $policy],
];
$environments = [];
foreach ($types as $type => $expression) {
    foreach ($modes as $mode => [$strict, $defined, $sandbox]) {
        $template = $defined ? "{{ capture($expression is defined) }}" : "{{ capture($expression) }}";
        $options = ['strict_variables' => $strict];
        if (null !== $sandbox) {
            $options['autoescape'] = false;
        }
        $twig = new Environment(new ArrayLoader(['index.twig' => $template]), $options);
        $twig->addFunction($capture);
        if (null !== $sandbox) {
            $twig->addExtension(new SandboxExtension($sandbox, true));
        }
        $environments[$type][$mode] = $twig;
    }
}

$rows = [];
foreach (array_values($objects) as $o => $object) {
    foreach ($items as $i => $item) {
        foreach (array_keys($types) as $type) {
            $results = [];
            foreach (array_keys($modes) as $mode) {
                $captured = null;
                $diagnostics = [];
                try {
                    $environments[$type][$mode]->render('index.twig', ['o' => $object, 'i' => $item]);
                    $results[$mode] = ['value', describe($captured), $diagnostics];
                } catch (\Throwable $e) {
                    $results[$mode] = ['error', get_class($e).': '.$e->getMessage(), $diagnostics];
                }
            }
            $rows[] = [$o, $i, $type, $results];
        }
    }
}

restore_error_handler();
echo json_encode([
    'objects' => array_keys($objects),
    'items' => array_map(static fn ($item) => var_export($item, true), $items),
    'rows' => $rows,
], JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_THROW_ON_ERROR), "\n";
