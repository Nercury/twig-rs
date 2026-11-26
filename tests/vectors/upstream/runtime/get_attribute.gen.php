<?php

/*
 * Generator of tests/vectors/upstream/runtime/get_attribute.json: the getAttribute() matrix of Twig 3.31.1's
 * tests/TemplateTest.php (TemplateTest::getGetAttributeTests(), the data provider of testGetAttribute(),
 * testGetAttributeStrict(), testGetAttributeDefined() and testGetAttributeDefinedStrict()) run the way the
 * twig-rs port runs it: through templates, because twig-rs has no public CoreExtension::getAttribute().
 *
 * Every data set is rendered with the template of its call type, in a lax and a strict environment, as a value
 * and as a defined test:
 *
 *     any     {{ capture(o.(i)) }}      {{ capture(o.(i) is defined) }}
 *     method  {{ capture(o.(i)()) }}    {{ capture(o.(i)() is defined) }}
 *     array   {{ capture(o[i]) }}       {{ capture(o[i] is defined) }}
 *
 * where "capture" records its argument. The direct CoreExtension::getAttribute() call of the PHPUnit test is
 * recorded too ("direct"), so differences between the PHPUnit test and the template path are visible.
 *
 * Regenerate from the repository root (Git Bash on Windows needs MSYS_NO_PATHCONV=1):
 *
 *     MSYS_NO_PATHCONV=1 docker run --rm -i twig-oracle:3.31.1 php < tests/vectors/upstream/runtime/get_attribute.gen.php > tests/vectors/upstream/runtime/get_attribute.json
 *
 * Output: [{"index", "class", "item" (var_export), "type", "defined", "value" (var_export, the data set's
 *           expectations), "lax", "strict", "defined_lax", "defined_strict", "direct", "direct_strict"}]
 * where every result is {"value": var_export} or {"error": [class, message]}.
 */

declare(strict_types=0);

require '/oracle/vendor/autoload.php';

use Twig\Environment;
use Twig\Extension\CoreExtension;
use Twig\Loader\ArrayLoader;
use Twig\Source;
use Twig\Tests\TemplateTest;
use Twig\TwigFunction;

error_reporting(E_ALL);
set_error_handler(static function (int $type, string $message): bool {
    if (!(error_reporting() & $type)) {
        return false; // silenced with @ (TemplatePropertyObjectDefinedWithUndefinedValue)
    }

    throw new \ErrorException($message, 0, $type);
});

$captured = null;
$capture = new TwigFunction('capture', static function ($value) use (&$captured) {
    $captured = $value;

    return '';
});

$templates = [
    'any' => ['{{ capture(o.(i)) }}', '{{ capture(o.(i) is defined) }}'],
    'method' => ['{{ capture(o.(i)()) }}', '{{ capture(o.(i)() is defined) }}'],
    'array' => ['{{ capture(o[i]) }}', '{{ capture(o[i] is defined) }}'],
];

function environment(bool $strict, string $template, TwigFunction $capture): Environment
{
    $twig = new Environment(new ArrayLoader(['index.twig' => $template]), ['strict_variables' => $strict]);
    $twig->addFunction($capture);

    return $twig;
}

function outcome(callable $run): array
{
    try {
        return ['value' => var_export($run(), true)];
    } catch (\Throwable $e) {
        return ['error' => [get_class($e), $e->getMessage()]];
    }
}

$out = [];
foreach (TemplateTest::getGetAttributeTests() as $index => $case) {
    [$defined, $value, $object, $item, $arguments, $type] = $case;
    [$valueTemplate, $definedTemplate] = $templates[$type];
    $row = [
        'index' => $index,
        'class' => is_object($object) ? get_class($object) : get_debug_type($object),
        'item' => var_export($item, true),
        'type' => $type,
        'defined' => $defined,
        'value' => var_export($value, true),
    ];
    foreach (['lax' => [false, $valueTemplate], 'strict' => [true, $valueTemplate], 'defined_lax' => [false, $definedTemplate], 'defined_strict' => [true, $definedTemplate]] as $mode => [$strict, $template]) {
        $row[$mode] = outcome(static function () use ($strict, $template, $capture, $object, $item, &$captured) {
            $captured = null;
            environment($strict, $template, $capture)->render('index.twig', ['o' => $object, 'i' => $item]);

            return $captured;
        });
    }
    foreach (['direct' => false, 'direct_strict' => true] as $mode => $strict) {
        $row[$mode] = outcome(static function () use ($strict, $object, $item, $arguments, $type) {
            $twig = new Environment(new ArrayLoader(), ['strict_variables' => $strict]);

            return CoreExtension::getAttribute($twig, new Source('', 'index.twig'), $object, $item, $arguments, $type);
        });
    }
    $out[] = $row;
}

echo json_encode($out, JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_THROW_ON_ERROR), "\n";
