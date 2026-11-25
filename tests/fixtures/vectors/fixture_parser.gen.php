<?php

/*
 * Generator of tests/fixtures/vectors/fixture_parser.json: how Twig 3.31.1's IntegrationTestCase parses `.test`
 * files, for hand-written edge cases and seeded random inputs. Every input is written to its own temporary
 * directory and parsed by the real upstream code, the private static IntegrationTestCase::assembleTests() called
 * through reflection (TWIG/src/Test/IntegrationTestCase.php), so the vectors hold PHP's answer, not a re-implementation.
 *
 * Regenerate from the repository root (Git Bash on Windows needs MSYS_NO_PATHCONV=1):
 *
 *     MSYS_NO_PATHCONV=1 docker run --rm -i twig-oracle:3.31.1 php < tests/fixtures/vectors/fixture_parser.gen.php > tests/fixtures/vectors/fixture_parser.json
 *
 * Output: {"generator", "twig", "php", "cases": [{"id", "text", "result"}]} where "result" is null when upstream
 * throws 'Test "x.test" is not valid.', else {"message", "condition", "deprecation", "templates": [[name, source]...]
 * (in order), "exception": false or the raw --EXCEPTION-- text, "outputs": [[data, config or null, expect]...]}.
 * Random inputs on which one of the two assembleTests() patterns fails with a PCRE error (backtrack limit) are left
 * out: the harness only claims PCRE's result for inputs PCRE can match.
 */

declare(strict_types=1);

require '/oracle/vendor/autoload.php';

const PATTERN_EXCEPTION = '/--TEST--\s*(.*?)\s*(?:--CONDITION--\s*(.*))?\s*(?:--DEPRECATION--\s*(.*?))?\s*((?:--TEMPLATE(?:\(.*?\))?--(?:.*?))+)\s*(?:--DATA--\s*(.*))?\s*--EXCEPTION--\s*(.*)/sx';
const PATTERN_OUTPUT = '/--TEST--\s*(.*?)\s*(?:--CONDITION--\s*(.*))?\s*(?:--DEPRECATION--\s*(.*?))?\s*((?:--TEMPLATE(?:\(.*?\))?--(?:.*?))+)--DATA--.*?--EXPECT--.*/s';

$assemble = new ReflectionMethod(Twig\Test\IntegrationTestCase::class, 'assembleTests');

/** The parse of one input by the upstream code; null when it is "not valid". */
function upstream_parse(ReflectionMethod $assemble, string $text): ?array
{
    $dir = sys_get_temp_dir().'/fixture-parser-'.bin2hex(random_bytes(8));
    mkdir($dir);
    file_put_contents($dir.'/x.test', $text);
    try {
        $tests = $assemble->invoke(null, false, $dir);
    } catch (InvalidArgumentException $e) {
        if ('Test "x.test" is not valid.' !== $e->getMessage()) {
            throw $e;
        }

        return null;
    } finally {
        unlink($dir.'/x.test');
        rmdir($dir);
    }
    [, $message, $condition, $templates, $exception, $outputs, $deprecation] = $tests['x.test'];

    return [
        'message' => $message,
        'condition' => $condition,
        'deprecation' => $deprecation,
        'templates' => array_map(static fn ($name, $source) => [(string) $name, $source], array_keys($templates), $templates),
        'exception' => $exception,
        'outputs' => array_map(static fn ($o) => [$o[1], $o[2] ?? null, $o[3] ?? ''], $outputs),
    ];
}

/** Whether PCRE can evaluate both assembleTests() patterns on the input (no backtrack-limit error). */
function pcre_ok(string $text): bool
{
    foreach ([PATTERN_EXCEPTION, PATTERN_OUTPUT] as $pattern) {
        if (false === preg_match($pattern, $text)) {
            return false;
        }
    }

    return true;
}

$cases = [];
$add = static function (string $id, string $text) use (&$cases, $assemble): void {
    if (!pcre_ok($text)) {
        return;
    }
    $cases[] = ['id' => $id, 'text' => $text, 'result' => upstream_parse($assemble, $text)];
};

// --- hand-written edge cases -------------------------------------------------------------------------------------

$fixed = [
    'basic-output' => "--TEST--\nmsg\n--TEMPLATE--\n{{ 1 }}\n--DATA--\nreturn []\n--EXPECT--\n1\n",
    'basic-exception' => "--TEST--\nmsg\n--TEMPLATE--\n{{ 1 }}\n--EXCEPTION--\nTwig\\Error\\SyntaxError: x.\n",
    'exception-with-data' => "--TEST--\nmsg\n--TEMPLATE--\nA\n--DATA--\nreturn []\n--EXCEPTION--\nE\n",
    'exception-trailing-ws-template' => "--TEST--\nm\n--TEMPLATE--\nA\n \t\n\n--EXCEPTION--\nE",
    'condition' => "--TEST--\nm\n--CONDITION--\nPHP_VERSION_ID >= 80100\n--TEMPLATE--\nA\n--DATA--\nreturn []\n--EXPECT--\nA",
    'condition-and-deprecation' => "--TEST--\nmsg\n--CONDITION--\ntrue\n--DEPRECATION--\nfoo\n--TEMPLATE--\nA\n--DATA--\nreturn []\n--EXPECT--\nA\n",
    'condition-two-templates' => "--TEST--\nx\n--CONDITION--\ntrue\n--TEMPLATE--\nA\n--TEMPLATE(b.twig)--\nB\n--DATA--\nreturn []\n--EXPECT--\nok\n",
    'condition-template-in-expect' => "--TEST--\nx\n--CONDITION--\ntrue\n--TEMPLATE--\nA\n--DATA--\nreturn []\n--EXPECT--\nok --TEMPLATE-- z\n--DATA--\nreturn [1]\n--EXPECT--\nok2\n",
    'condition-exception' => "--TEST--\nx\n--CONDITION--\n1\n--TEMPLATE--\nA\n--TEMPLATE(b)--\nB\n--EXCEPTION--\nE\n",
    'condition-empty' => "--TEST--\nx\n--CONDITION----TEMPLATE--\nA\n--DATA--\nreturn []\n--EXPECT--\nok",
    'deprecation' => "--TEST--\nmsg\n--DEPRECATION--\n  one\n  two  \n\n--TEMPLATE--\nA\n--DATA--\nreturn []\n--EXPECT--\nA\n",
    'deprecation-then-condition' => "--TEST--\nm\n--DEPRECATION--\nd\n--CONDITION--\nc\n--TEMPLATE--\nA\n--DATA--\nreturn []\n--EXPECT--\nA\n",
    'deprecation-exception' => "--TEST--\nm\n--DEPRECATION--\nd\n--TEMPLATE--\nA\n--EXCEPTION--\nE\n",
    'two-exceptions-with-data' => "--TEST--\nx\n--TEMPLATE--\nA\n--DATA--\nreturn []\n--EXCEPTION--\nE1\n--EXCEPTION--\nE2\n",
    'two-exceptions-without-data' => "--TEST--\nx\n--TEMPLATE--\nA\n--EXCEPTION--\nE1\n--EXCEPTION--\nE2\n",
    'exception-in-expect' => "--TEST--\nx\n--TEMPLATE--\nA\n--DATA--\nreturn []\n--EXPECT--\nfoo --EXCEPTION-- bar\n",
    'config' => "--TEST--\nx\n--TEMPLATE--\nA\n--DATA--\nreturn []\n--CONFIG--\nreturn ['autoescape' => false]\n--EXPECT--\nA\n",
    'config-expect-overlap' => "--TEST--\nx\n--TEMPLATE--\nA\n--DATA--\nd\n--CONFIG--EXPECT--\nfoo\n--EXPECT--\nbar\n",
    'config-expect-overlap-last' => "--TEST--\nx\n--TEMPLATE--\nA\n--DATA--\nd\n--CONFIG--EXPECT--\nfoo\n",
    'config-without-expect-later' => "--TEST--\nx\n--TEMPLATE--\nA\n--DATA--\nd\n--EXPECT--\ne\n--DATA--\nd2\n--CONFIG--\nc\n",
    'three-pairs' => "--TEST--\nx\n--TEMPLATE--\nA\n--DATA--\nreturn [1]\n--EXPECT--\n1\n--DATA--\nreturn [2]\n--CONFIG--\nreturn []\n--EXPECT--\n2\n--DATA--\nreturn [3]\n--EXPECT--\n3\n\n",
    'data-without-expect-at-end' => "--TEST--\nx\n--TEMPLATE--\nA\n--DATA--\nd\n--EXPECT--\ne\n--DATA--\nlast\n",
    'template-data-overlap' => "--TEST--\nx\n--TEMPLATE--DATA--\nfoo\n--DATA--\nreturn []\n--EXPECT--\nok\n",
    'name-with-template' => "--TEST--\nx\n--TEMPLATE(--\nbody\n--TEMPLATE(b)--\nB\n--DATA--\nreturn []\n--EXPECT--\nok\n",
    'name-without-close' => "--TEST--\nx\n--TEMPLATE--\nA\n--TEMPLATE(b\nB\n--DATA--\nreturn []\n--EXPECT--\nok\n",
    'name-close-after-data' => "--TEST--\nx\n--TEMPLATE(a\n--DATA--\nd\n)--\nbody\n--DATA--\nreturn []\n--EXPECT--\nok\n",
    'name-parens' => "--TEST--\nx\n--TEMPLATE(a)b)--\nA\n--TEMPLATE((c))--\nC\n--DATA--\nreturn []\n--EXPECT--\nok\n",
    'bad-header' => "--TEST--\nx\n--TEMPLATE--\nA\n--TEMPLATEX--\nZ\n--TEMPLATE(c)--\nC\n--DATA--\nreturn []\n--EXPECT--\nok\n",
    'only-bad-headers' => "--TEST--\nx\n--TEMPLATEX--\nA\n--DATA--\nreturn []\n--EXPECT--\nok\n",
    'zero-and-empty-names' => "--TEST--\nx\n--TEMPLATE(0)--\nA\n--TEMPLATE()--\nB\n--TEMPLATE(12)--\nC\n--DATA--\nreturn []\n--EXPECT--\nok\n\n",
    'duplicate-names' => "--TEST--\nx\n--TEMPLATE(a)--1\n--TEMPLATE(b)--2\n--TEMPLATE(a)--3\n--DATA--\nreturn []\n--EXPECT--\nok",
    'empty-templates' => "--TEST--\nx\n--TEMPLATE(a)----TEMPLATE(b)--\n--DATA--\nreturn []\n--EXPECT--\n",
    'last-template-two-newlines' => "--TEST--\nx\n--TEMPLATE--\nA\n\n--DATA--\nreturn []\n--EXPECT--\nok",
    'data-in-message' => "--TEST--\nfoo --DATA-- bar\n--TEMPLATE--\nx\n--DATA--\nreturn []\n--EXPECT--\nok\n",
    'template-in-message' => "--TEST--\nsee --TEMPLATEX-- here\n--TEMPLATE--\nx\n--DATA--\nreturn []\n--EXPECT--\nok\n",
    'two-test-markers' => "junk --TEST-- a\n--TEST--\nb\n--TEMPLATE--\nA\n--DATA--\nreturn []\n--EXPECT--\nok",
    'two-line-message' => "--TEST--\n  line one\n  line two  \n--TEMPLATE--\nA\n--DATA--\nreturn []\n--EXPECT--\nok",
    'empty-message' => "--TEST----TEMPLATE--\nA\n--DATA--\nreturn []\n--EXPECT--\nok",
    'whitespace-kinds' => "--TEST--\t\x0B\x0C\r\n m \r\n--TEMPLATE--\r\nA\r\n--DATA--\r\nreturn []\r\n--EXPECT--\r\nok\r\n",
    'no-test' => "--TEMPLATE--\nA\n--DATA--\nreturn []\n--EXPECT--\nok",
    'no-template' => "--TEST--\nx\n--DATA--\nreturn []\n--EXPECT--\nok",
    'no-expect' => "--TEST--\nx\n--TEMPLATE--\nA\n--DATA--\nreturn []\n",
    'no-data' => "--TEST--\nx\n--TEMPLATE--\nA\n--EXPECT--\nok",
    'expect-before-data' => "--TEST--\nx\n--TEMPLATE--\nA\n--EXPECT--\nok\n--DATA--\nreturn []\n",
    'empty-file' => '',
    'utf8' => "--TEST--\nété\n--TEMPLATE(é.twig)--\n{{ 'à' }}\n--DATA--\nreturn []\n--EXPECT--\nà\n",
];
foreach ($fixed as $id => $text) {
    $add($id, $text);
}

// --- seeded random inputs ----------------------------------------------------------------------------------------

mt_srand(31_1_2026);

function pick(array $items)
{
    return $items[mt_rand(0, count($items) - 1)];
}

function ws(): string
{
    return pick(['', '', "\n", "\n", "\n\n", ' ', "\t", " \n", "\r\n", "\x0B", "\x0C", "\n \n"]);
}

function filler(): string
{
    $out = '';
    for ($i = mt_rand(0, 3); $i > 0; --$i) {
        $out .= pick(['a', 'foo', '0', '12', 'index.twig', 'é', '{{ x }}', '-', '--', '(', ')', ')--', 'return []', ' ', "\n"]);
    }

    return $out;
}

function marker(): string
{
    return pick([
        '--TEST--', '--CONDITION--', '--DEPRECATION--', '--TEMPLATE--', '--TEMPLATE(a)--', '--TEMPLATE(b.twig)--',
        '--TEMPLATE()--', '--TEMPLATE(0)--', '--TEMPLATE(', '--TEMPLATE', '--DATA--', '--CONFIG--', '--EXPECT--',
        '--EXCEPTION--', '--DATA', '--EXPECT', ')--', '--',
    ]);
}

/** A file shaped like a fixture, with random variations and noise. */
function structured(): string
{
    $parts = [];
    if (0 === mt_rand(0, 7)) {
        $parts[] = filler();
    }
    if (0 !== mt_rand(0, 15)) {
        $parts[] = '--TEST--';
    }
    $parts[] = ws().filler().ws();
    if (0 === mt_rand(0, 3)) {
        $parts[] = '--CONDITION--'.ws().filler().ws();
    }
    if (0 === mt_rand(0, 3)) {
        $parts[] = '--DEPRECATION--'.ws().filler().ws();
    }
    for ($i = mt_rand(0, 3); $i > 0; --$i) {
        $parts[] = pick(['--TEMPLATE--', '--TEMPLATE(a)--', '--TEMPLATE(b.twig)--', '--TEMPLATE()--', '--TEMPLATE(0)--', '--TEMPLATE(a)--']).ws().filler().ws();
    }
    if (mt_rand(0, 1)) {
        if (mt_rand(0, 1)) {
            $parts[] = '--DATA--'.ws().filler().ws();
        }
        $parts[] = '--EXCEPTION--'.ws().filler().ws();
    } else {
        for ($i = mt_rand(0, 3); $i > 0; --$i) {
            $parts[] = '--DATA--'.ws().filler().ws();
            if (0 === mt_rand(0, 3)) {
                $parts[] = '--CONFIG--'.ws().filler().ws();
            }
            if (0 !== mt_rand(0, 7)) {
                $parts[] = '--EXPECT--'.ws().filler().ws();
            }
        }
    }
    for ($i = mt_rand(0, 2); $i > 0; --$i) {
        array_splice($parts, mt_rand(0, count($parts)), 0, [ws().marker().ws()]);
    }

    return implode('', $parts);
}

/** Random tokens, mostly markers. */
function soup(): string
{
    $out = '';
    for ($i = mt_rand(1, 14); $i > 0; --$i) {
        $out .= mt_rand(0, 1) ? marker() : (mt_rand(0, 1) ? ws() : filler());
    }

    return $out;
}

for ($n = 0; $n < 1500; ++$n) {
    $add('structured-'.$n, structured());
}
for ($n = 0; $n < 500; ++$n) {
    $add('soup-'.$n, soup());
}

echo json_encode([
    'generator' => 'tests/fixtures/vectors/fixture_parser.gen.php',
    'twig' => Twig\Environment::VERSION,
    'php' => PHP_VERSION,
    'cases' => $cases,
], JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_PRETTY_PRINT | JSON_THROW_ON_ERROR), "\n";
