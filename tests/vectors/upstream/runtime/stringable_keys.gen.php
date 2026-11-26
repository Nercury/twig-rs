<?php

/*
 * Generator of tests/vectors/upstream/runtime/stringable_keys.json: a sweep of the Stringable-key handling of
 * getAttribute() and of the optimized array access (TWIG/tests/TemplateTest.php, the testStringableKey* and
 * testArrayAccess* tests): keys (a fresh TemplateStringableKey, a plain object, an array, null, an int, a float, true)
 * used as the item on containers (arrays, the internal SPL array classes and subclasses, SplObjectStorage, tracking
 * ArrayAccess objects, objects, scalars) with every call type, lax and strict, as a defined test, and sandboxed with
 * and without the right to call __toString().
 *
 * Regenerate from the repository root (Git Bash on Windows needs MSYS_NO_PATHCONV=1):
 *
 *     MSYS_NO_PATHCONV=1 docker run --rm -i twig-oracle:3.31.1 php < tests/vectors/upstream/runtime/stringable_keys.gen.php > tests/vectors/upstream/runtime/stringable_keys.json
 *
 * Output: [[key, container, type, mode, kind, text, toStringCalls, offsetExists calls, offsetGet calls,
 * diagnostics]] where kind is "value" (text: the captured value, described like get_attribute_sweep.gen.php does) or
 * "error" ("Class: message"); the counts are -1 for keys and containers that do not track them; diagnostics lists the
 * PHP warnings and deprecations raised ("Warning: ...", "Deprecated: ...").
 */

declare(strict_types=0);

require '/oracle/vendor/autoload.php';

use Twig\Environment;
use Twig\Extension\SandboxExtension;
use Twig\Loader\ArrayLoader;
use Twig\Sandbox\SecurityPolicy;
use Twig\Tests\TemplateArrayIteratorSubclass;
use Twig\Tests\TemplateArrayObjectSubclass;
use Twig\Tests\TemplateStringableKey;
use Twig\Tests\TemplateTest;
use Twig\Tests\TemplateTrackingArrayAccess;
use Twig\TwigFunction;

class_exists(TemplateTest::class); // declares the helper classes

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

$containers = [
    'array' => static fn ($key) => ['string' => 'value'],
    'ArrayObject' => static fn ($key) => new \ArrayObject(['string' => 'value']),
    'ArrayIterator' => static fn ($key) => new \ArrayIterator(['string' => 'value']),
    'RecursiveArrayIterator' => static fn ($key) => new \RecursiveArrayIterator(['string' => 'value']),
    'TemplateArrayObjectSubclass' => static fn ($key) => new TemplateArrayObjectSubclass(['string' => 'value']),
    'TemplateArrayIteratorSubclass' => static fn ($key) => new TemplateArrayIteratorSubclass(['string' => 'value']),
    'SplObjectStorage' => static function ($key) {
        $storage = new \SplObjectStorage();
        if (is_object($key)) {
            $storage[$key] = 'value';
        }

        return $storage;
    },
    'TemplateTrackingArrayAccess' => static fn ($key) => new TemplateTrackingArrayAccess(is_object($key) ? $key : new \stdClass(), false),
    'TemplateTrackingArrayAccessRejecting' => static fn ($key) => new TemplateTrackingArrayAccess(is_object($key) ? $key : new \stdClass(), true),
    'TemplateArrayAccessObject' => static fn ($key) => new Twig\Tests\TemplateArrayAccessObject(),
    'stdClass' => static fn ($key) => (object) ['string' => 'value'],
    'TemplateMethodObject' => static fn ($key) => new Twig\Tests\TemplateMethodObject(),
    'TemplateMagicMethodObject' => static fn ($key) => new Twig\Tests\TemplateMagicMethodObject(),
    'null' => static fn ($key) => null,
    'string' => static fn ($key) => 'str',
];
$types = ['any' => 'o.(k)', 'method' => 'o.(k)()', 'array' => 'o[k]'];

$allow = new SecurityPolicy([], [], [TemplateStringableKey::class => ['__toString']], [
    TemplateArrayObjectSubclass::class => ['string'],
    TemplateArrayIteratorSubclass::class => ['string'],
    TemplateTrackingArrayAccess::class => ['string'],
    'Twig\Tests\TemplateArrayAccessObject' => ['string'],
    'stdClass' => ['string'],
], ['capture']);
$modes = [
    'lax' => [false, false, null],
    'strict' => [true, false, null],
    'defined_lax' => [false, true, null],
    'defined_strict' => [true, true, null],
    'sandbox_allow' => [false, false, $allow],
    'sandbox_deny' => [false, false, new SecurityPolicy([], [], [], [], ['capture'])],
];

$captured = null;
$capture = new TwigFunction('capture', static function ($value) use (&$captured) {
    $captured = $value;

    return '';
});

$environments = [];
foreach ($types as $type => $expression) {
    foreach ($modes as $mode => [$strict, $defined, $policy]) {
        $template = $defined ? "{{ capture($expression is defined) }}" : "{{ capture($expression) }}";
        $twig = new Environment(new ArrayLoader(['index.twig' => $template]), ['strict_variables' => $strict, 'autoescape' => false]);
        $twig->addFunction($capture);
        if (null !== $policy) {
            $twig->addExtension(new SandboxExtension($policy, true));
        }
        $environments[$type][$mode] = $twig;
    }
}

$diagnostics = [];
set_error_handler(static function (int $type, string $message) use (&$diagnostics): bool {
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

$keys = [
    'stringable' => static fn () => new TemplateStringableKey(),
    'object' => static fn () => new \stdClass(),
    'array' => static fn () => ['x'],
    'null' => static fn () => null,
    'int' => static fn () => 1,
    'float' => static fn () => 1.5,
    'true' => static fn () => true,
];

$out = [];
foreach ($keys as $keyName => $keyFactory) {
    foreach ($containers as $name => $factory) {
        foreach (array_keys($types) as $type) {
            foreach (array_keys($modes) as $mode) {
                $key = $keyFactory();
                $container = $factory($key);
                $captured = null;
                $diagnostics = [];
                try {
                    $environments[$type][$mode]->render('index.twig', ['o' => $container, 'k' => $key]);
                    [$kind, $text] = ['value', describe($captured)];
                } catch (\Throwable $e) {
                    [$kind, $text] = ['error', get_class($e).': '.$e->getMessage()];
                }
                $tracking = $container instanceof TemplateTrackingArrayAccess;
                $calls = $key instanceof TemplateStringableKey ? $key->toStringCalls : -1;
                $out[] = [$keyName, $name, $type, $mode, $kind, $text, $calls, $tracking ? count($container->offsetExistsCalls) : -1, $tracking ? count($container->offsetGetCalls) : -1, $diagnostics];
            }
        }
    }
}

restore_error_handler();
echo json_encode($out, JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_THROW_ON_ERROR), "\n";
