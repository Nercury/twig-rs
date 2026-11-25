<?php

/*
 * Exports every Twig 3.31.1 integration fixture as one JSON corpus for twig-rs:
 * raw sections (parsed with the harness regexes), the ACTUAL PHP results of each DATA/EXPECT pair (run through a
 * faithful copy of IntegrationTestCase::doIntegrationTest(), one PHP process per fixture), the evaluated DATA as a
 * typed tree, and heuristic classification flags.
 *
 *   docker run --rm twig-oracle:3.31.1 php tools/export-corpus.php > corpus/fixtures-3.31.1.json
 *   docker run --rm twig-oracle:3.31.1 php tools/export-corpus.php --only filters/abs.test --pretty
 */

declare(strict_types=0);

require dirname(__DIR__).'/vendor/autoload.php';
require dirname(__DIR__).'/lib.php';

use function TwigOracle\parse_fixture;

$pretty = in_array('--pretty', $argv, true);
$only = null;
if (false !== ($k = array_search('--only', $argv, true))) {
    $only = $argv[$k + 1] ?? null;
}

$dir = realpath(TwigOracle\FIXTURES_DIR);
$files = [];
foreach (new RecursiveIteratorIterator(new RecursiveDirectoryIterator($dir), RecursiveIteratorIterator::LEAVES_ONLY) as $file) {
    if (preg_match('/\.test$/', (string) $file)) {
        $files[] = str_replace($dir.'/', '', (string) $file);
    }
}
sort($files, SORT_STRING);
if (null !== $only) {
    $files = array_values(array_filter($files, static fn ($f) => $f === $only || str_contains($f, $only)));
}

// Callables registered only by the test harness (tests/IntegrationTest.php), by kind.
const CUSTOM = [
    'filter' => ['§', 'escape_and_nl2br', 'nl2br', 'escape_something', 'preserves_safety', 'static_call_string', 'static_call_array',
        'magic_call', 'magic_call_closure', 'magic_call_string', 'magic_call_array', 'not', 'anon_foo', 'attribute_filter', 'lazy_attribute_filter'],
    'function' => ['§', 'safe_br', 'unsafe_br', 'static_call_string', 'static_call_array', 'anon_foo', 'deprecated_function',
        'attribute_function', 'lazy_attribute_function', 'throwing_undefined_function'],
    'test' => ['multi word', 'attribute_test', 'lazy_attribute_test', 'throwing_undefined_test', 'throwing_undefined_two words_test'],
];

function flags_for(array $p): array
{
    $tpl = implode("\n", $p['templates']);
    $data = implode("\n", array_map(static fn ($o) => (string) $o[1], $p['outputs']));
    $config = implode("\n", array_map(static fn ($o) => (string) ($o[2] ?? ''), $p['outputs']));
    $f = [];

    $custom = [];
    foreach (CUSTOM['filter'] as $n) {
        if (preg_match('/\|\s*'.preg_quote($n, '/').'(?![\w])/u', $tpl) || preg_match('/guard\s+filter\s+'.preg_quote($n, '/').'\b/u', $tpl)) {
            $custom[] = 'filter:'.$n;
        }
    }
    foreach (CUSTOM['function'] as $n) {
        if (preg_match('/(?<![\w.|])'.preg_quote($n, '/').'\s*\(/u', $tpl) || preg_match('/guard\s+function\s+'.preg_quote($n, '/').'\b/u', $tpl)) {
            $custom[] = 'function:'.$n;
        }
    }
    foreach (CUSTOM['test'] as $n) {
        if (preg_match('/\bis\s+(?:not\s+)?'.preg_quote($n, '/').'\b/u', $tpl) || preg_match('/guard\s+test\s+'.preg_quote($n, '/').'\b/u', $tpl)) {
            $custom[] = 'test:'.$n;
        }
    }
    if (preg_match('/\|\s*\w+_path\b|\b\w+_path\s*\(/', $tpl)) {
        $custom[] = 'filter|function:*_path';
    }
    if (preg_match('/\b\w+_foo_\w+_bar\b/', $tpl)) {
        $custom[] = 'filter|function:*_foo_*_bar';
    }
    if (preg_match('/\bis\s+(?:not\s+)?test_\w+/', $tpl)) {
        $custom[] = 'test:test_*';
    }
    if (preg_match('/\{%-?\s*§/u', $tpl)) {
        $custom[] = 'tag:§';
    }
    $f['custom_extension'] = $custom;

    $f['debug_extension'] = (bool) preg_match('/\bdump\s*\(/', $tpl);
    $f['string_loader_extension'] = (bool) preg_match('/\btemplate_from_string\s*\(/', $tpl);
    $f['sandbox'] = (bool) preg_match('/sandboxed\s*[=:]\s*true|\{%-?\s*sandbox\b/', $tpl);
    $f['harness_global'] = (bool) preg_match('/\bglobal\b|_context|\bdump\s*\(\s*\)/', $tpl);
    $f['php_constants'] = (bool) preg_match('/\bconstant\s*\(|is\s+(?:not\s+)?constant\s*\(/', $tpl);
    $f['enum'] = (bool) preg_match('/\benum(?:_cases)?\s*\(/', $tpl);
    $f['dates'] = (bool) (preg_match('/date_default_timezone_set|mktime|DateTime|DateInterval|setTimezone|setDateFormat/', $data)
        || preg_match('/\|\s*date\b|\|\s*date_modify\b|(?<![\w.|])date\s*\(/', $tpl));
    $f['data_uses_twig'] = str_contains($data, '$twig');
    $f['data_defines_classes'] = (bool) preg_match('/\bclass\s+\w+|new\s+class\b/', $data);
    preg_match_all('/new\s+\\\\?([A-Za-z_][\w\\\\]*)/', $data, $m);
    $f['data_new_classes'] = array_values(array_unique(array_map(static fn ($c) => ltrim($c, '\\'), $m[1])));
    preg_match_all('/[\'"](\w+)[\'"]\s*=>/', $config, $m);
    $f['config_keys'] = array_values(array_unique($m[1]));

    return $f;
}

$out = [
    'generated_by' => 'php-oracle/tools/export-corpus.php',
    'twig_version' => Twig\Environment::VERSION,
    'php_version' => PHP_VERSION,
    'fixtures' => [],
];
foreach ($files as $rel) {
    $p = parse_fixture(file_get_contents($dir.'/'.$rel));
    $entry = [
        'file' => $rel,
        'legacy' => str_contains($rel, '.legacy.test'),
        'message' => $p['message'],
        'condition' => $p['condition'],
        'deprecation_expected' => $p['deprecation'],
        'exception_expected' => $p['exception'],
        'templates' => $p['templates'],
        'outputs' => array_map(static fn ($o) => [
            'data_php' => $o[1],
            'config_php' => $o[2] ?? null,
            'expect_raw' => $o[3] ?? '',
        ], $p['outputs']),
        'flags' => flags_for($p),
    ];

    $proc = proc_open([PHP_BINARY, dirname(__DIR__).'/runner.php', '--single'], [0 => ['pipe', 'r'], 1 => ['pipe', 'w'], 2 => ['pipe', 'w']], $pipes);
    fwrite($pipes[0], json_encode(['fixture' => $rel, 'typed' => true]));
    fclose($pipes[0]);
    $json = stream_get_contents($pipes[1]);
    $err = stream_get_contents($pipes[2]);
    fclose($pipes[1]);
    fclose($pipes[2]);
    proc_close($proc);
    $actual = json_decode($json, true);
    if (is_array($actual) && array_is_list($actual)) {
        $actual = $actual[0];
    }
    $entry['actual'] = $actual ?? ['ok' => false, 'error' => trim($err."\n".$json)];
    $out['fixtures'][] = $entry;
}

echo json_encode($out, JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_INVALID_UTF8_SUBSTITUTE | JSON_PRESERVE_ZERO_FRACTION | ($pretty ? JSON_PRETTY_PRINT : 0)), "\n";
