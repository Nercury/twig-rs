<?php
// Generates returns.json: the declared return types of the ported test classes of TWIG/tests/IntegrationTest.php.
//
//   cd crates/twig-test-support/tests/vectors
//   MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//       php /t/returns.gen.php > returns.json
//
// Each case stores a value with the constructor and reads it back through the method that declares the return type:
// "string" is `(string) new ToStringStub(...)` (`__toString(): string`), "count" is `(new CountableStub(...))->count()`
// (`count(): int`). Results are the value or the chain of thrown exceptions (outermost first). "throw" cases run with
// an error handler that throws ErrorException for every diagnostic.

declare(strict_types=0);

require '/oracle/vendor/autoload.php';
class_exists(\Twig\Tests\IntegrationTest::class); // declares the helper classes of the test suite

use Twig\Tests\CountableStub;
use Twig\Tests\ToStringStub;
use Twig\Tests\TwigTestFoo;

$values = [
    'null' => static fn () => null,
    'true' => static fn () => true,
    'int' => static fn () => 7,
    'float' => static fn () => 1.5,
    'nan' => static fn () => NAN,
    'string' => static fn () => 's',
    'numeric' => static fn () => '1.5',
    'array' => static fn () => [1],
    'countable' => static fn () => new CountableStub(1),
    'tostring' => static fn () => new ToStringStub('inner'),
    'foo' => static fn () => new TwigTestFoo(),
];

$ops = [
    'string' => static function ($v) {
        $o = new ToStringStub('t');
        $o->__construct($v);

        return (string) $o;
    },
    'count' => static function ($v) {
        $o = new CountableStub(0);
        $o->__construct($v);

        return $o->count();
    },
];

function chain(\Throwable $e): array
{
    $chain = [];
    for (; null !== $e; $e = $e->getPrevious()) {
        $chain[] = [get_class($e), $e->getMessage()];
    }

    return $chain;
}

function result($r): array
{
    return match (true) {
        is_string($r) => ['string' => $r],
        is_int($r) => ['int' => $r],
        default => ['other' => get_debug_type($r)],
    };
}

$cases = [];
foreach (['collect', 'throw'] as $mode) {
    foreach ($ops as $op => $run) {
        foreach ($values as $label => $make) {
            $diagnostics = [];
            set_error_handler(static function (int $no, string $msg) use ($mode, &$diagnostics): bool {
                if ('throw' === $mode) {
                    throw new \ErrorException($msg, 0, $no);
                }
                $diagnostics[] = $msg;

                return true;
            });
            $value = $make();
            try {
                $result = ['ok' => result($run($value))];
            } catch (\Throwable $e) {
                $result = ['err' => chain($e)];
            }
            restore_error_handler();
            $cases[] = ['mode' => $mode, 'op' => $op, 'value' => $label, 'result' => $result, 'diagnostics' => $diagnostics];
        }
    }
}

echo json_encode(['generator' => 'returns.gen.php', 'php' => PHP_VERSION, 'cases' => $cases],
    JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES | JSON_THROW_ON_ERROR), "\n";
