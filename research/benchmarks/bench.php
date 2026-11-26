<?php
// PHP Twig counterpart of the criterion benchmarks in benches/: the same scenarios (benches/scenarios/<name>/: the
// templates, index.twig as the entry point, data.json as the context), the same micro templates (the `micro` group of
// benches/render.rs) and the same operations, timed with hrtime().
//
//   php bench.php <scenarios dir> [--cache=<dir>] [--dump=<dir>] [--time=<seconds>] [--only=<substring>]
//
// --cache   Twig's filesystem cache (so that OPcache can keep the compiled templates); default: no cache, compiled
//           templates are eval()'d once per process.
// --dump    write each scenario's rendered output to <dir>/<scenario>.html (compare with TWIG_BENCH_DUMP of
//           benches/render.rs).
// --time    measurement time per benchmark in seconds (default 5, after a warm-up of a fifth of it).
//
// Prints one JSON object per line and benchmark: group, name, median/mean/min nanoseconds per iteration, the number
// of samples and the iterations per sample. run.sh runs it in the oracle image (Twig 3.31.1 on PHP 8.5.13).

declare(strict_types=1);

require '/oracle/vendor/autoload.php';

use Twig\Environment;
use Twig\Extension\CoreExtension;
use Twig\Loader\ArrayLoader;
use Twig\Source;

// getopt() stops at the first operand, so options are parsed by hand and may follow the directory
$options = [];
$dir = null;
foreach (array_slice($argv, 1) as $arg) {
    if (preg_match('/^--(cache|dump|time|only)=(.*)$/s', $arg, $m)) {
        $options[$m[1]] = $m[2];
    } elseif ($dir === null && !str_starts_with($arg, '--')) {
        $dir = $arg;
    } else {
        $dir = null;
        break;
    }
}
if ($dir === null || !is_dir($dir)) {
    fwrite(STDERR, "usage: php bench.php <scenarios dir> [--cache=<dir>] [--dump=<dir>] [--time=<s>] [--only=<s>]\n");
    exit(2);
}
$cache = $options['cache'] ?? false;
$dump = $options['dump'] ?? null;
$time = (float) ($options['time'] ?? 5);
$only = $options['only'] ?? null;

/** @return array<string, array{templates: array<string, string>, context: array}> */
function scenarios(string $dir): array
{
    $scenarios = [];
    $names = array_filter(scandir($dir), fn ($n) => $n[0] !== '.' && is_dir("$dir/$n"));
    sort($names, SORT_STRING);
    foreach ($names as $name) {
        $templates = [];
        $files = scandir("$dir/$name");
        sort($files, SORT_STRING);
        foreach ($files as $file) {
            if (str_ends_with($file, '.twig')) {
                $templates[$file] = file_get_contents("$dir/$name/$file");
            }
        }
        $data = "$dir/$name/data.json";
        $context = is_file($data) ? json_decode(file_get_contents($data), true, 512, JSON_THROW_ON_ERROR) : [];
        $scenarios[$name] = ['templates' => $templates, 'context' => $context];
    }

    return $scenarios;
}

/** Twig's default options and the scenario's templates; the date filters use UTC, like benches/common/mod.rs. */
function environment(array $templates, string|false $cache): Environment
{
    $env = new Environment(new ArrayLoader($templates), ['cache' => $cache]);
    $env->getExtension(CoreExtension::class)->setTimezone('UTC');

    return $env;
}

/**
 * Warms up for a fifth of $seconds, sizes samples to about 10 ms, then takes samples for $seconds (at least 10).
 *
 * @return array{median_ns: float, mean_ns: float, min_ns: float, samples: int, iters: int}
 */
function measure(callable $fn, float $seconds): array
{
    $warmupEnd = hrtime(true) + (int) ($seconds * 2e8);
    $calls = 0;
    $start = hrtime(true);
    do {
        $fn();
        ++$calls;
    } while ($calls < 3 || hrtime(true) < $warmupEnd);
    $perCall = (hrtime(true) - $start) / $calls;
    $iters = max(1, (int) round(1e7 / max($perCall, 1)));

    $samples = [];
    $end = hrtime(true) + (int) ($seconds * 1e9);
    do {
        $t0 = hrtime(true);
        for ($i = 0; $i < $iters; ++$i) {
            $fn();
        }
        $samples[] = (hrtime(true) - $t0) / $iters;
    } while (count($samples) < 10 || hrtime(true) < $end);

    sort($samples);
    $n = count($samples);
    $median = $n % 2 ? $samples[intdiv($n, 2)] : ($samples[$n / 2 - 1] + $samples[$n / 2]) / 2;

    return [
        'median_ns' => round($median, 1),
        'mean_ns' => round(array_sum($samples) / $n, 1),
        'min_ns' => round($samples[0], 1),
        'samples' => $n,
        'iters' => $iters,
    ];
}

/** The templates of the `micro` group of benches/render.rs (keep them in sync). */
function micro_templates(): array
{
    return [
        'hello' => 'Hello {{ name }}!',
        'text' => str_repeat('x{# c #}', 1000),
        'print_int' => str_repeat('{{ 1 }}', 1000),
        'print_raw' => str_repeat('{{ a|raw }}', 1000),
        'print_escaped' => str_repeat('{{ a }}', 1000),
        'set' => str_repeat('{% set x = a %}', 1000),
        'set_add' => str_repeat('{% set x = a + 1 %}', 1000),
        'for' => '{% for i in items %}{{ i }}{% endfor %}',
        'attribute' => str_repeat('{{ user.name }}', 1000),
        'filter' => str_repeat('{{ name|length }}', 1000),
        'empty_loop' => str_repeat('{% for i in [1, 2] %}{% endfor %}', 1000),
        'macro' => '{% macro m(v) %}{{ v }}{% endmacro %}'.str_repeat('{{ _self.m(a) }}', 1000),
    ];
}

$results = [];
foreach (scenarios($dir) as $name => $scenario) {
    if ($only !== null && !str_contains($name, $only)) {
        continue;
    }

    // compile: Environment::compileSource() of every template (benches/compile.rs)
    $env = environment($scenario['templates'], false);
    $sources = [];
    foreach ($scenario['templates'] as $template => $code) {
        $sources[] = new Source($code, $template);
    }
    $stats = measure(function () use ($env, $sources) {
        foreach ($sources as $source) {
            $env->compileSource($source);
        }
    }, $time);
    echo json_encode(['group' => 'compile', 'name' => $name] + $stats), "\n";

    // render: Environment::render('index.twig') with a warm template cache (benches/render.rs)
    $env = environment($scenario['templates'], $cache === false ? false : "$cache/$name");
    $context = $scenario['context'];
    $output = $env->render('index.twig', $context);
    if ($dump !== null) {
        if (!is_dir($dump)) {
            mkdir($dump, 0777, true);
        }
        file_put_contents("$dump/$name.html", $output);
    }
    $stats = measure(fn () => $env->render('index.twig', $context), $time);
    echo json_encode(['group' => 'render', 'name' => $name, 'bytes' => strlen($output)] + $stats), "\n";
}

// micro: one-off templates repeating a single construct (the `micro` group of benches/render.rs)
$templates = micro_templates();
$env = environment($templates, $cache === false ? false : "$cache/micro");
$context = ['name' => 'World', 'a' => 1, 'items' => range(1, 1000), 'user' => ['name' => 'Ada']];
foreach (array_keys($templates) as $name) {
    if ($only !== null && !str_contains($name, $only)) {
        continue;
    }
    $env->render($name, $context);
    $stats = measure(fn () => $env->render($name, $context), $time);
    echo json_encode(['group' => 'micro', 'name' => $name] + $stats), "\n";
}
