<?php

/*
 * twig-rs PHP oracle runner: reads a JSON batch on stdin, writes JSON results on stdout.
 * Runs Twig 3.31.1 on PHP 8.5 (see Dockerfile).  Usage inside the image:
 *
 *     php runner.php [--pretty] < batch.json > results.json
 *
 * Input: a JSON array of cases, or {"cases": [...], "isolation": "none"|"process"}, or a single case object.
 * Every case may carry an "id" (echoed back).  The case kind is "kind" or inferred from the keys:
 *
 * (a) twig render (default kind "twig"):
 *     {
 *       "templates": {"index.twig": "Hello {{ name }}"},  // required, name => source
 *       "main": "index.twig",            // template to render (default "index.twig", else first template)
 *       "data_json": {"name": "x"},      // context as JSON  ... or ...
 *       "data_php": "return ['name' => new \\ArrayObject([1])]",  // PHP statements evaluated like a fixture
 *                                        // --DATA-- section: eval($data_php.';') with $twig in scope; code without
 *                                        // any "return" keyword is treated as an expression ("['a' => 1]")
 *       "config": {"autoescape": "html"},// Environment options (JSON)  ... or "config_php": "return [...]"
 *       "env": "integration" | "plain",  // integration (default): exactly the IntegrationTest harness setup
 *                                        //   (cache=false, strict_variables=true unless overridden, global "global",
 *                                        //   Debug/Sandbox(non-global)/StringLoader/test extensions, callbacks)
 *                                        // plain: new Environment(ArrayLoader, config) with Twig defaults
 *       "extensions": ["debug","string_loader","sandbox"],  // plain env only
 *       "globals": {"k": "v"},           // plain env only
 *       "sandbox": {"global": true, "policy": {"tags":[], "filters":[], "methods":{}, "properties":{}, "functions":[]}},
 *       "timezone": "Europe/Paris",      // date_default_timezone_set() before data_php runs (restored afterwards)
 *       "load_all": true,                // load every template first, in order, like the harness (default true)
 *       "errors": "collect" | "throw"    // PHP warnings/notices: collect (default) or throw \ErrorException
 *                                        // (PHPUnit 9 converts them to exceptions inside Twig's test suite)
 *     }
 *     -> {"id","kind":"twig","ok":true,"output":"...","deprecations":[...],"diagnostics":[...]}
 *     -> {"id","kind":"twig","ok":false,"phase":"load"|"data"|"render","error":{class,message,formatted,rawMessage,
 *         templateName,templateLine,templateColumn,previous?},"deprecations":[...],"diagnostics":[...]}
 *     Strings that are not valid UTF-8 are returned with '?' substitutions plus a "<field>_b64" copy.
 *
 * (b) php eval (kind "php", inferred from "php" or "php_code"):
 *     {"php": "1.0 + '1e3'"}                // an expression (evaluated as `return (EXPR);`)
 *     {"php_code": "$a = [3,1]; sort($a); return $a;"}   // statements
 *     optional: "timezone", "errors": "collect"|"throw", "typed": true (adds a typed tree of the value)
 *     -> {"id","kind":"php","ok":true,"echo":"...","var_export":"...","var_dump":"...","json":"...","json_error":null,
 *         "type":"float","diagnostics":[...]}  or  {"ok":false,"error":{...},"echo":...,"diagnostics":[...]}
 *
 * (c) fixture (kind "fixture", inferred from "fixture"): runs one Twig .test file through a faithful copy of
 *     IntegrationTestCase::doIntegrationTest() and returns the ACTUAL PHP results per DATA/EXPECT pair.
 *     {"fixture": "filters/abs.test"}       // relative to vendor/twig/twig/tests/Fixtures (or absolute path)
 *     optional "typed": true -> every case also gets "data" (typed tree of the evaluated --DATA-- context),
 *     "config" (typed Environment options) and "state" (PHP default timezone + CoreExtension timezone/date
 *     format/number format right before rendering, i.e. after the DATA code ran)
 *     -> {"kind":"fixture","file","message","skipped":null|"reason","exception_expected":false|"...",
 *         "cases":[{"index":0,"pass":true,"output":"...","expected":"...","error":null|{...},"phase",
 *                   "deprecations":[...],"deprecation_expected":"..."}]}
 *
 * Batch option "isolation": "process" runs every case in its own PHP process (protects the batch from fatal
 * errors, redeclared classes and global state such as date_default_timezone_set()). Default "none": cases run
 * sequentially in one process; a fatal error still produces valid JSON (remaining cases are reported as lost).
 */

declare(strict_types=0);

require __DIR__.'/vendor/autoload.php';
require __DIR__.'/lib.php';

use TwigOracle\Diagnostics;

use function TwigOracle\describe_throwable;
use function TwigOracle\eval_data_code;
use function TwigOracle\integration_environment;
use function TwigOracle\plain_environment;
use function TwigOracle\put_string;
use function TwigOracle\run_fixture;
use function TwigOracle\typed;
use function TwigOracle\var_dump_string;

ini_set('display_errors', 'stderr');
error_reporting(\E_ALL);

$pretty = in_array('--pretty', $argv, true);
$single = in_array('--single', $argv, true);
$jsonFlags = JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_INVALID_UTF8_SUBSTITUTE | JSON_PRESERVE_ZERO_FRACTION | ($pretty ? JSON_PRETTY_PRINT : 0);

$input = stream_get_contents(STDIN);
try {
    $batch = json_decode($input, true, 512, JSON_THROW_ON_ERROR);
} catch (\JsonException $e) {
    echo json_encode([['ok' => false, 'error' => ['class' => 'JsonException', 'message' => 'Invalid JSON input: '.$e->getMessage()]]], $jsonFlags), "\n";
    exit(1);
}
$isolation = 'none';
if (is_array($batch) && array_is_list($batch)) {
    $cases = $batch;
} elseif (is_array($batch) && isset($batch['cases'])) {
    $cases = $batch['cases'];
    $isolation = $batch['isolation'] ?? 'none';
} else {
    $cases = [$batch];
}

$GLOBALS['__oracle_results'] = [];
$GLOBALS['__oracle_current'] = null;
$GLOBALS['__oracle_flags'] = $jsonFlags;
$GLOBALS['__oracle_total'] = count($cases);

register_shutdown_function(static function () {
    if (!isset($GLOBALS['__oracle_done'])) {
        // fatal error (redeclared class, timeout, memory...): still emit valid JSON
        while (ob_get_level() > 0) {
            ob_end_clean();
        }
        $err = error_get_last();
        $results = $GLOBALS['__oracle_results'];
        $cur = $GLOBALS['__oracle_current'];
        $results[] = ['id' => $cur['id'] ?? null, 'ok' => false, 'phase' => 'fatal', 'error' => ['class' => 'FatalError', 'message' => $err['message'] ?? 'unknown fatal error']];
        for ($i = count($results); $i < $GLOBALS['__oracle_total']; ++$i) {
            $results[] = ['ok' => false, 'phase' => 'lost', 'error' => ['class' => 'FatalError', 'message' => 'Not run: an earlier case in the batch died with a fatal error (use "isolation": "process").']];
        }
        echo json_encode($results, $GLOBALS['__oracle_flags']), "\n";
    }
});

$defaultTz = date_default_timezone_get();
foreach ($cases as $case) {
    $GLOBALS['__oracle_current'] = $case;
    set_time_limit(120);
    if ('process' === $isolation && !$single) {
        $GLOBALS['__oracle_results'][] = run_isolated($case);
    } else {
        try {
            $GLOBALS['__oracle_results'][] = run_case($case);
        } catch (\Throwable $e) {
            $GLOBALS['__oracle_results'][] = ['id' => $case['id'] ?? null, 'ok' => false, 'phase' => 'oracle', 'error' => describe_throwable($e)];
        }
    }
    date_default_timezone_set($defaultTz);
}
$GLOBALS['__oracle_done'] = true;
echo json_encode($single ? $GLOBALS['__oracle_results'][0] : $GLOBALS['__oracle_results'], $jsonFlags), "\n";

// ---------------------------------------------------------------------------------------------------------

function run_isolated(array $case): array
{
    $proc = proc_open([PHP_BINARY, __FILE__, '--single'], [0 => ['pipe', 'r'], 1 => ['pipe', 'w'], 2 => ['pipe', 'w']], $pipes);
    fwrite($pipes[0], json_encode($case, JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE));
    fclose($pipes[0]);
    $out = stream_get_contents($pipes[1]);
    $err = stream_get_contents($pipes[2]);
    fclose($pipes[1]);
    fclose($pipes[2]);
    proc_close($proc);
    $res = json_decode($out, true);
    if (!is_array($res)) {
        return ['id' => $case['id'] ?? null, 'ok' => false, 'phase' => 'fatal', 'error' => ['class' => 'FatalError', 'message' => trim($err."\n".$out)]];
    }
    if (array_is_list($res)) { // the child died: shutdown handler emitted a list
        $res = $res[0];
    }

    return $res;
}

function run_case(array $case): array
{
    $kind = $case['kind'] ?? (isset($case['fixture']) ? 'fixture' : (isset($case['php']) || isset($case['php_code']) ? 'php' : 'twig'));

    return match ($kind) {
        'twig' => run_twig($case),
        'php' => run_php($case),
        'fixture' => run_fixture($case),
        default => throw new \InvalidArgumentException(sprintf('Unknown case kind "%s".', $kind)),
    };
}

function run_twig(array $case): array
{
    $res = ['id' => $case['id'] ?? null, 'kind' => 'twig'];
    $templates = $case['templates'] ?? null;
    if (!is_array($templates) || !$templates) {
        throw new \InvalidArgumentException('A twig case needs a non-empty "templates" object.');
    }
    $main = $case['main'] ?? (array_key_exists('index.twig', $templates) ? 'index.twig' : array_key_first($templates));
    $diag = new Diagnostics();
    $mode = $case['errors'] ?? 'collect';
    if (isset($case['timezone'])) {
        date_default_timezone_set($case['timezone']);
    }

    $phase = 'setup';
    try {
        $twig = null;
        $config = $case['config'] ?? [];
        if (isset($case['config_php'])) {
            $config = array_merge($config, (array) eval_data_code($case['config_php'], null));
        }
        $twig = ('plain' === ($case['env'] ?? 'integration'))
            ? plain_environment($templates, $config, $case['extensions'] ?? [], $case['globals'] ?? [], $case['sandbox'] ?? null)
            : integration_environment($templates, $config);

        $phase = 'load';
        $loaded = $diag->run(static function () use ($twig, $templates, $main, $case) {
            $loaded = [];
            if ($case['load_all'] ?? true) {
                foreach (array_keys($templates) as $name) {
                    $loaded[$name] = $twig->load($name);
                }
            }

            return $loaded[$main] ?? $twig->load($main);
        }, $mode);

        $phase = 'data';
        $context = $diag->run(static function () use ($case, $twig) {
            if (isset($case['data_php'])) {
                return eval_data_code($case['data_php'], $twig);
            }

            return $case['data_json'] ?? [];
        }, $mode);
        if (!is_array($context)) {
            throw new \InvalidArgumentException(sprintf('The data must evaluate to an array, got "%s".', get_debug_type($context)));
        }

        $phase = 'render';
        $output = $diag->run(static fn () => $loaded->render($context), $mode);
        $res['ok'] = true;
        put_string($res, 'output', $output);
    } catch (\Throwable $e) {
        $res['ok'] = false;
        $res['phase'] = $phase;
        $res['error'] = describe_throwable($e);
    }
    $res['deprecations'] = $diag->deprecations;
    $res['diagnostics'] = $diag->diagnostics;

    return $res;
}

function run_php(array $case): array
{
    $res = ['id' => $case['id'] ?? null, 'kind' => 'php'];
    if (isset($case['timezone'])) {
        date_default_timezone_set($case['timezone']);
    }
    $code = isset($case['php_code']) ? $case['php_code'] : 'return ('.$case['php'].');';
    $diag = new Diagnostics();
    ob_start();
    try {
        $value = $diag->run(static fn () => (static function () use ($code) { return eval($code); })(), $case['errors'] ?? 'collect');
        $echo = ob_get_clean();
        $res['ok'] = true;
        put_string($res, 'echo', $echo);
        $res['type'] = get_debug_type($value);
        try {
            put_string($res, 'var_export', var_export($value, true));
        } catch (\Throwable $e) {
            $res['var_export'] = null;
            $res['var_export_error'] = describe_throwable($e);
        }
        put_string($res, 'var_dump', var_dump_string($value));
        $json = json_encode($value, JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_PRESERVE_ZERO_FRACTION | JSON_PARTIAL_OUTPUT_ON_ERROR);
        $res['json'] = false === $json ? null : $json;
        $res['json_error'] = JSON_ERROR_NONE === json_last_error() ? null : json_last_error_msg();
        if (is_string($value) || is_int($value) || is_float($value) || is_bool($value) || null === $value || $value instanceof \Stringable) {
            try {
                put_string($res, 'string', (string) $value); // PHP string conversion of the value
            } catch (\Throwable $e) {
                $res['string_error'] = describe_throwable($e);
            }
        }
        if ($case['typed'] ?? false) {
            $res['typed'] = typed($value);
        }
    } catch (\Throwable $e) {
        $echo = ob_get_clean();
        $res['ok'] = false;
        put_string($res, 'echo', $echo);
        $res['error'] = describe_throwable($e);
    }
    $res['diagnostics'] = $diag->diagnostics;

    return $res;
}
