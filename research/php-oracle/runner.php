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
 *       // optional, used by twig-fuzz (crates/twig-fuzz); without them nothing changes:
 *       "templates_b64": {"x.twig": "/w=="}, // templates given as base64 (sources that are not valid UTF-8),
 *                                        //   added after "templates", replacing a template of the same name
 *       "test_classes": true,            // load tests/IntegrationTest.php first (declares TwigTestFoo & co.)
 *       "typed_data": true,              // also return "data" (TwigOracle\typed_rich() of the evaluated data,
 *                                        //   taken before rendering), "state" (default timezone and the
 *                                        //   CoreExtension timezone/date format/number format after the data
 *                                        //   phase) and "data_diagnostics"/"data_deprecations" (what the data
 *                                        //   code raised)
 *       "time_limit": 120,               // set_time_limit() for the case (default 120)
 *       "memory_limit": "512M",          // ini memory_limit for the case (the image default is -1)
 *       "clock": true,                   // also return "clock": [sec, usec] (gettimeofday() right before
 *                                        //   rendering), so that a runner with a settable clock can use the
 *                                        //   same "now"
 *       // and in "sandbox": "policy": {"tests": [...], "strict": true}: SecurityPolicy's allowed tests and
 *       // setStrict(), applied when the environment is built
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
 * With process isolation:
 *   - a child gets its case's own text from the batch, not json_encode() of the decoded case (json_encode()
 *     cannot write the INF that json_decode() makes of a number like 1e400);
 *   - batch option "jobs": N runs up to N children at a time (default 1); cases with the same filesystem cache
 *     directory never run at the same time; results keep the batch order;
 *   - a child that has not answered after "wall_limit" seconds of wall-clock time (a case option, default
 *     2 * time_limit + 30, at least 60) is killed and reported as "phase": "fatal": time_limit counts CPU time
 *     only, so a child blocked on a pipe or stuck after a fatal error would otherwise hold the batch forever.
 *
 * A "twig" case whose config "cache" is a directory under /tmp/ gets that directory emptied before it runs, so
 * that every run compiles its templates (a retry, or a second run of a case in the same container, would
 * otherwise load the files the first run compiled, without their compile-time deprecations).
 *
 * --heartbeat: write "twig-oracle heartbeat" to stderr at start and "." after every case, so that a client can
 * tell a slow batch from a stuck one (twig-fuzz's oracle client does).
 *
 * Server mode (twig-fuzz's minimiser): `php runner.php --serve` reads one batch per line and writes its results
 * as one line, until stdin closes. A fatal error ends the server after it printed that batch's results (the case
 * that died has "phase": "fatal", the cases after it "lost"). --pretty is ignored in server mode.
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
use function TwigOracle\typed_rich;
use function TwigOracle\var_dump_string;

ini_set('display_errors', 'stderr');
error_reporting(\E_ALL);

$pretty = in_array('--pretty', $argv, true);
$single = in_array('--single', $argv, true);
$serve = in_array('--serve', $argv, true);
$jsonFlags = JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_INVALID_UTF8_SUBSTITUTE | JSON_PRESERVE_ZERO_FRACTION | ($pretty && !$serve ? JSON_PRETTY_PRINT : 0);
$GLOBALS['__oracle_flags'] = $jsonFlags;
$GLOBALS['__oracle_heartbeat'] = in_array('--heartbeat', $argv, true);
if ($GLOBALS['__oracle_heartbeat']) {
    fwrite(STDERR, "twig-oracle heartbeat\n");
    fflush(STDERR);
}

if ($serve) {
    serve($jsonFlags);
    exit(0);
}

$input = stream_get_contents(STDIN);
try {
    $batch = json_decode($input, true, 512, JSON_THROW_ON_ERROR);
} catch (\JsonException $e) {
    echo json_encode([['ok' => false, 'phase' => 'oracle', 'error' => ['class' => 'JsonException', 'message' => 'Invalid JSON input: '.$e->getMessage()]]], $jsonFlags), "\n";
    exit(1);
}
[$cases, $isolation, $jobs] = batch_cases($batch);

// a closure, as before the runner was split into functions: object ids (spl_object_id, var_dump handles) stay put
register_shutdown_function(static function () {
    shutdown_report();
});
$results = run_batch($cases, $isolation, $single, 'process' === $isolation && !$single ? raw_cases($input, $cases) : null, $jobs);
echo json_encode($single ? $results[0] : $results, $jsonFlags), "\n";

// ---------------------------------------------------------------------------------------------------------

/** The cases, the isolation mode and the number of parallel children ("jobs") of a decoded batch. */
function batch_cases($batch): array
{
    if (is_array($batch) && array_is_list($batch)) {
        return [$batch, 'none', 1];
    }
    if (is_array($batch) && isset($batch['cases'])) {
        return [array_values((array) $batch['cases']), $batch['isolation'] ?? 'none', max(1, (int) ($batch['jobs'] ?? 1))];
    }

    return [[$batch], 'none', 1];
}

/**
 * Runs the cases of one batch in order (with "isolation": "process" in child processes, see run_isolated());
 * shutdown_report() covers a batch that dies. $raw: the source text of each case (raw_cases()).
 */
function run_batch($cases, $isolation, bool $single, ?array $raw = null, int $jobs = 1): array
{
    $GLOBALS['__oracle_results'] = [];
    $GLOBALS['__oracle_current'] = null;
    $GLOBALS['__oracle_total'] = count($cases);
    unset($GLOBALS['__oracle_done']);

    if ('process' === $isolation && !$single) {
        $GLOBALS['__oracle_results'] = run_isolated(array_values($cases), $raw, $jobs);
        $GLOBALS['__oracle_done'] = true;

        return $GLOBALS['__oracle_results'];
    }
    $defaultTz = date_default_timezone_get();
    foreach ($cases as $case) {
        $GLOBALS['__oracle_current'] = $case;
        set_time_limit((int) ($case['time_limit'] ?? 120));
        $memory = isset($case['memory_limit']) ? ini_set('memory_limit', (string) $case['memory_limit']) : false;
        try {
            $GLOBALS['__oracle_results'][] = run_case($case);
        } catch (\Throwable $e) {
            $GLOBALS['__oracle_results'][] = ['id' => $case['id'] ?? null, 'ok' => false, 'phase' => 'oracle', 'error' => describe_throwable($e)];
        }
        if (false !== $memory) {
            ini_set('memory_limit', $memory);
        }
        date_default_timezone_set($defaultTz);
        heartbeat();
    }
    $GLOBALS['__oracle_done'] = true;

    return $GLOBALS['__oracle_results'];
}

/** --heartbeat: one line on stderr per finished case. */
function heartbeat(): void
{
    if ($GLOBALS['__oracle_heartbeat'] ?? false) {
        fwrite(STDERR, ".\n");
        fflush(STDERR);
    }
}

/** Shutdown function: a fatal error (redeclared class, timeout, memory...) still emits valid JSON. */
function shutdown_report(): void
{
    if (!isset($GLOBALS['__oracle_done'])) {
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
}

/** --serve: one batch per input line, its results as one output line. */
function serve(int $jsonFlags): void
{
    $registered = false;
    while (false !== ($line = fgets(STDIN))) {
        if ('' === trim($line)) {
            continue;
        }
        try {
            $batch = json_decode($line, true, 512, JSON_THROW_ON_ERROR);
        } catch (\JsonException $e) {
            echo json_encode([['ok' => false, 'phase' => 'oracle', 'error' => ['class' => 'JsonException', 'message' => 'Invalid JSON input: '.$e->getMessage()]]], $jsonFlags), "\n";
            fflush(STDOUT);
            continue;
        }
        [$cases, $isolation, $jobs] = batch_cases($batch);
        if (!$registered) {
            register_shutdown_function('shutdown_report');
            $registered = true;
        }
        $raw = 'process' === $isolation ? raw_cases($line, $cases) : null;
        echo json_encode(run_batch($cases, $isolation, false, $raw, $jobs), $jsonFlags), "\n";
        fflush(STDOUT);
    }
    $GLOBALS['__oracle_done'] = true;
}

/**
 * The source text of every case of a batch, in the order batch_cases() returns them, or null when the text does
 * not split that way. A case that runs in a child process gets its own text rather than json_encode() of the
 * decoded case: json_encode() fails on the INF/-INF that json_decode() makes of numbers such as 1e400, so the
 * child would decode something else, or nothing.
 */
function raw_cases(string $json, array $cases): ?array
{
    try {
        $scanner = new RawJson($json);
        $spans = null;
        if ('[' === $scanner->peek()) {
            $spans = $scanner->elements();
        } elseif ('{' === $scanner->peek()) {
            foreach ($scanner->members() as [$key, $from, $to]) {
                if ('cases' === $key) { // json_decode() keeps the last of repeated keys
                    $inner = (new RawJson(substr($json, $from, $to - $from)))->elements();
                    $spans = array_map(static fn (array $span) => [$span[0] + $from, $span[1] + $from], $inner);
                }
            }
        }
    } catch (\Throwable) {
        return null;
    }
    if (null === $spans || count($spans) !== count($cases)) {
        return null;
    }

    return array_map(static fn (array $span) => substr($json, $span[0], $span[1] - $span[0]), $spans);
}

/** Byte offsets of the members and elements of JSON text that json_decode() already accepted. */
final class RawJson
{
    private int $p = 0;
    private int $n;

    public function __construct(private string $s)
    {
        $this->n = strlen($s);
    }

    public function peek(): string
    {
        $this->p += strspn($this->s, " \t\n\r", $this->p);

        return $this->p < $this->n ? $this->s[$this->p] : '';
    }

    /** Skips one value and returns its [start, end) offsets. */
    public function value(): array
    {
        $c = $this->peek();
        $start = $this->p;
        if ('{' === $c) {
            $this->members();
        } elseif ('[' === $c) {
            $this->elements();
        } elseif ('"' === $c) {
            $this->string();
        } elseif ('' === $c) {
            throw new \RuntimeException('unexpected end');
        } else {
            $this->p += strcspn($this->s, ",]} \t\n\r", $this->p);
        }

        return [$start, $this->p];
    }

    private function string(): string
    {
        $start = $this->p++;
        while (true) {
            $this->p += strcspn($this->s, "\"\\", $this->p);
            if ($this->p >= $this->n) {
                throw new \RuntimeException('unterminated string');
            }
            if ('\\' === $this->s[$this->p]) {
                $this->p += 2;
                continue;
            }
            ++$this->p;

            return (string) json_decode(substr($this->s, $start, $this->p - $start), false, 512, JSON_THROW_ON_ERROR);
        }
    }

    /** After a member or an element: true at ",", false at $close. */
    private function next(string $close): bool
    {
        $c = $this->peek();
        ++$this->p;
        if ($c === $close) {
            return false;
        }
        if (',' !== $c) {
            throw new \RuntimeException(sprintf('unexpected "%s" at %d', $c, $this->p - 1));
        }

        return true;
    }

    /** @return list<array{string, int, int}> key, value start, value end */
    public function members(): array
    {
        $out = [];
        $this->peek();
        ++$this->p; // {
        if ('}' === $this->peek()) {
            ++$this->p;

            return $out;
        }
        do {
            $this->peek();
            $key = $this->string();
            if (':' !== $this->peek()) {
                throw new \RuntimeException('expected ":"');
            }
            ++$this->p;
            [$from, $to] = $this->value();
            $out[] = [$key, $from, $to];
        } while ($this->next('}'));

        return $out;
    }

    /** @return list<array{int, int}> */
    public function elements(): array
    {
        $out = [];
        $this->peek();
        ++$this->p; // [
        if (']' === $this->peek()) {
            ++$this->p;

            return $out;
        }
        do {
            $out[] = $this->value();
        } while ($this->next(']'));

        return $out;
    }
}

/** The filesystem cache directory of a twig case ("config": {"cache": "<dir>"}), if any. */
function cache_dir($case): ?string
{
    $dir = is_array($case) && is_array($case['config'] ?? null) ? ($case['config']['cache'] ?? null) : null;

    return is_string($dir) && '' !== $dir ? rtrim($dir, '/') : null;
}

/** Empties the filesystem cache directory of a case before it runs, when it is under /tmp/ (see the top). */
function clear_cache_dir(?string $dir): void
{
    if (null === $dir || !str_starts_with($dir, '/tmp/') || str_contains($dir, '/..') || !is_dir($dir)) {
        return;
    }
    $files = new \RecursiveIteratorIterator(
        new \RecursiveDirectoryIterator($dir, \FilesystemIterator::SKIP_DOTS),
        \RecursiveIteratorIterator::CHILD_FIRST
    );
    foreach ($files as $file) {
        $file->isDir() && !$file->isLink() ? @rmdir($file->getPathname()) : @unlink($file->getPathname());
    }
}

/** The wall-clock limit of a case run in a child process (see the top). */
function wall_limit($case): float
{
    $case = is_array($case) ? $case : [];
    if (isset($case['wall_limit'])) {
        return (float) $case['wall_limit'];
    }

    return (float) max(60, 2 * (int) ($case['time_limit'] ?? 120) + 30);
}

/**
 * Runs every case in a PHP process of its own (`php runner.php --single`), up to $jobs at a time, and returns
 * their results in batch order. A child gets the case's text from the batch ($raw); a child that does not answer
 * within wall_limit() is killed. A child that dies without printing a result (a fatal error PHP could not report,
 * the hard memory limit, a crash) is reported as "phase": "fatal" with what it printed.
 */
function run_isolated(array $cases, ?array $raw, int $jobs): array
{
    set_time_limit(0); // the children have their limits; this loop mostly waits
    $results = [];
    $pending = array_keys($cases);
    $running = [];
    $busy = [];
    while ($pending || $running) {
        // start children, never two with the same cache directory
        foreach ($pending as $k => $i) {
            if (count($running) >= $jobs) {
                break;
            }
            $dir = cache_dir($cases[$i]);
            if (null !== $dir && isset($busy[$dir])) {
                continue;
            }
            unset($pending[$k]);
            $child = start_child($cases[$i], $raw[$i] ?? null);
            if (isset($child['proc'])) {
                $child['dir'] = $dir;
                if (null !== $dir) {
                    $busy[$dir] = true;
                }
                $running[$i] = $child;
            } else {
                $results[$i] = $child;
                heartbeat();
            }
        }
        if (!$running) {
            continue;
        }
        // wait for output, at most until the nearest deadline
        $read = [];
        foreach ($running as $child) {
            foreach ($child['pipes'] as $pipe) {
                $read[] = $pipe;
            }
        }
        $wait = max(0.0, min(1.0, min(array_column($running, 'deadline')) - microtime(true)));
        $write = $except = null;
        if ($read) {
            @stream_select($read, $write, $except, (int) $wait, (int) (($wait - (int) $wait) * 1e6));
        }
        foreach (array_keys($running) as $i) {
            $child = &$running[$i];
            foreach ($child['pipes'] as $fd => $pipe) {
                $chunk = fread($pipe, 1 << 16);
                if (false !== $chunk && '' !== $chunk) {
                    $child[1 === $fd ? 'out' : 'err'] .= $chunk;
                }
                if (feof($pipe)) {
                    fclose($pipe);
                    unset($child['pipes'][$fd]);
                }
            }
            $killed = false;
            if ($child['pipes'] && microtime(true) > $child['deadline']) {
                proc_terminate($child['proc'], 9);
                foreach ($child['pipes'] as $pipe) {
                    fclose($pipe);
                }
                $child['pipes'] = [];
                $killed = true;
            }
            if (!$child['pipes']) {
                $status = proc_close($child['proc']);
                $results[$i] = child_result($cases[$i], $child, $status, $killed);
                if (null !== $child['dir']) {
                    unset($busy[$child['dir']]);
                }
                unset($child, $running[$i]);
                heartbeat();
                continue;
            }
            unset($child);
        }
    }
    ksort($results);

    return array_values($results);
}

/** Starts the child of one case; returns its state, or a result when it cannot be started. */
function start_child($case, ?string $text): array
{
    $id = is_array($case) ? ($case['id'] ?? null) : null;
    if (null === $text) {
        $text = json_encode($case, JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_PRESERVE_ZERO_FRACTION);
        if (false === $text) {
            return ['id' => $id, 'ok' => false, 'phase' => 'oracle', 'error' => ['class' => 'JsonException', 'message' => 'The case cannot be passed to a child process: '.json_last_error_msg()]];
        }
    }
    $proc = proc_open([PHP_BINARY, __FILE__, '--single'], [0 => ['pipe', 'r'], 1 => ['pipe', 'w'], 2 => ['pipe', 'w']], $pipes);
    if (!is_resource($proc)) {
        return ['id' => $id, 'ok' => false, 'phase' => 'oracle', 'error' => ['class' => 'RuntimeException', 'message' => 'Cannot start a child process.']];
    }
    @fwrite($pipes[0], $text);
    fclose($pipes[0]);
    stream_set_blocking($pipes[1], false);
    stream_set_blocking($pipes[2], false);

    return ['proc' => $proc, 'pipes' => [1 => $pipes[1], 2 => $pipes[2]], 'out' => '', 'err' => '', 'deadline' => microtime(true) + wall_limit($case)];
}

/** The result of a finished (or killed) child. */
function child_result($case, array $child, int $status, bool $killed): array
{
    $id = is_array($case) ? ($case['id'] ?? null) : null;
    if ($killed) {
        return ['id' => $id, 'ok' => false, 'phase' => 'fatal', 'error' => ['class' => 'FatalError', 'message' => sprintf('Killed: no result after %s seconds of wall-clock time (wall_limit).', wall_limit($case))]];
    }
    $res = json_decode($child['out'], true);
    if (!is_array($res)) {
        $message = trim($child['err']."\n".$child['out']);

        return ['id' => $id, 'ok' => false, 'phase' => 'fatal', 'error' => ['class' => 'FatalError', 'message' => '' !== $message ? $message : sprintf('The child process exited with status %d without a result.', $status)]];
    }
    if (array_is_list($res)) { // the child died: shutdown handler emitted a list
        $res = $res[0];
    }
    if (!isset($res['phase']) && !($res['ok'] ?? false) && 'JsonException' === ($res['error']['class'] ?? null)) {
        $res['phase'] = 'oracle'; // the child could not decode its input: says nothing about Twig
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
    if (isset($case['templates_b64']) && is_array($case['templates_b64'])) {
        $templates = is_array($templates) ? $templates : [];
        foreach ($case['templates_b64'] as $name => $code) {
            $templates[$name] = base64_decode((string) $code);
        }
    }
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
    $typed = null;
    $clock = null;
    try {
        if ($case['test_classes'] ?? false) {
            class_exists(\Twig\Tests\IntegrationTest::class); // declares the helper classes of the test suite
        }
        $twig = null;
        $config = $case['config'] ?? [];
        if (isset($case['config_php'])) {
            $config = array_merge($config, (array) eval_data_code($case['config_php'], null));
        }
        clear_cache_dir(cache_dir(['config' => $config]));
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
        $diagnosticsBefore = count($diag->diagnostics);
        $deprecationsBefore = count($diag->deprecations);
        $context = $diag->run(static function () use ($case, $twig) {
            if (isset($case['data_php'])) {
                return eval_data_code($case['data_php'], $twig);
            }

            return $case['data_json'] ?? [];
        }, $mode);
        if ($case['typed_data'] ?? false) {
            $core = $twig->getExtension(\Twig\Extension\CoreExtension::class);
            $typed = [
                'data' => typed_rich($context),
                'state' => [
                    'php_default_timezone' => date_default_timezone_get(),
                    'core_timezone' => $core->getTimezone()->getName(),
                    'core_date_format' => $core->getDateFormat(),
                    'core_number_format' => $core->getNumberFormat(),
                ],
                'data_diagnostics' => array_slice($diag->diagnostics, $diagnosticsBefore),
                'data_deprecations' => array_slice($diag->deprecations, $deprecationsBefore),
            ];
        }
        if (!is_array($context)) {
            throw new \InvalidArgumentException(sprintf('The data must evaluate to an array, got "%s".', get_debug_type($context)));
        }

        $phase = 'render';
        if ($case['clock'] ?? false) {
            $now = gettimeofday();
            $clock = [$now['sec'], $now['usec']];
        }
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
    if (null !== $typed) {
        $res += $typed;
    }
    if (null !== $clock) {
        $res['clock'] = $clock;
    }

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
