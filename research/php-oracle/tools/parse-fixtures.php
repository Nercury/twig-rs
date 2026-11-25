<?php
/*
 * Parse Twig .test fixtures EXACTLY like Twig\Test\IntegrationTestCase::assembleTests() (Twig 3.31.1,
 * src/Test/IntegrationTestCase.php lines 175-218 and 387-396) and dump them as JSON.
 *
 * Usage (inside the oracle image, or any PHP >= 8.1):
 *   php tools/parse-fixtures.php <fixturesDir> [--pretty] > fixtures.json
 *
 * Output: JSON array, one object per .test file (legacy and non-legacy, sorted by path):
 *   {
 *     "file":        "filters/abs.test",              // path relative to fixturesDir (key used by PHPUnit)
 *     "legacy":      false,                           // true when the real path contains ".legacy.test"
 *     "message":     "\"abs\" filter",                // --TEST-- text
 *     "condition":   "",                              // --CONDITION-- PHP expression ("" when absent)
 *     "deprecation": "",                              // --DEPRECATION-- text ("" when absent)
 *     "templates":   {"index.twig": "..."},           // insertion order == TEMPLATE section order
 *     "exception":   false | "Twig\\Error\\SyntaxError: ...",   // raw (untrimmed) --EXCEPTION-- text
 *     "outputs":     [ {"data": "...", "config": "...", "expect": "..."} ]  // raw, untrimmed
 *   }
 * For exception fixtures "outputs" has exactly one entry {data: <DATA text or "">, config: null, expect: ""},
 * mirroring `$outputs = [[null, $match[5], null, '']]`.
 */

$dir = $argv[1] ?? null;
if (!$dir || !is_dir($dir)) {
    fwrite(STDERR, "usage: php parse-fixtures.php <fixturesDir> [--pretty]\n");
    exit(2);
}
$pretty = in_array('--pretty', $argv, true);

function parseTemplates(string $test): array
{
    $templates = [];
    preg_match_all('/--TEMPLATE(?:\((.*?)\))?--(.*?)(?=\-\-TEMPLATE|$)/s', $test, $matches, \PREG_SET_ORDER);
    foreach ($matches as $match) {
        $templates[$match[1] ?: 'index.twig'] = $match[2];
    }

    return $templates;
}

$fixturesDir = realpath($dir);
$tests = [];
foreach (new \RecursiveIteratorIterator(new \RecursiveDirectoryIterator($fixturesDir), \RecursiveIteratorIterator::LEAVES_ONLY) as $file) {
    if (!preg_match('/\.test$/', $file)) {
        continue;
    }
    $legacy = str_contains($file->getRealpath(), '.legacy.test');
    $test = file_get_contents($file->getRealpath());
    $rel = str_replace($fixturesDir.'/', '', (string) $file);

    if (preg_match('/--TEST--\s*(.*?)\s*(?:--CONDITION--\s*(.*))?\s*(?:--DEPRECATION--\s*(.*?))?\s*((?:--TEMPLATE(?:\(.*?\))?--(?:.*?))+)\s*(?:--DATA--\s*(.*))?\s*--EXCEPTION--\s*(.*)/sx', $test, $match)) {
        $rec = [
            'file' => $rel,
            'legacy' => $legacy,
            'message' => $match[1],
            'condition' => $match[2],
            'deprecation' => $match[3],
            'templates' => parseTemplates($match[4]),
            'exception' => $match[6],
            'outputs' => [['data' => $match[5], 'config' => null, 'expect' => '']],
        ];
    } elseif (preg_match('/--TEST--\s*(.*?)\s*(?:--CONDITION--\s*(.*))?\s*(?:--DEPRECATION--\s*(.*?))?\s*((?:--TEMPLATE(?:\(.*?\))?--(?:.*?))+)--DATA--.*?--EXPECT--.*/s', $test, $match)) {
        preg_match_all('/--DATA--(.*?)(?:--CONFIG--(.*?))?--EXPECT--(.*?)(?=\-\-DATA\-\-|$)/s', $test, $outputs, \PREG_SET_ORDER);
        $outs = [];
        foreach ($outputs as $o) {
            $outs[] = ['data' => $o[1], 'config' => $o[2], 'expect' => $o[3]];
        }
        $rec = [
            'file' => $rel,
            'legacy' => $legacy,
            'message' => $match[1],
            'condition' => $match[2],
            'deprecation' => $match[3],
            'templates' => parseTemplates($match[4]),
            'exception' => false,
            'outputs' => $outs,
        ];
    } else {
        $rec = ['file' => $rel, 'legacy' => $legacy, 'invalid' => \sprintf('Test "%s" is not valid.', $rel)];
    }
    $tests[$rel] = $rec;
}
ksort($tests, SORT_STRING);
echo json_encode(array_values($tests), JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_INVALID_UTF8_SUBSTITUTE | ($pretty ? JSON_PRETTY_PRINT : 0)), "\n";
