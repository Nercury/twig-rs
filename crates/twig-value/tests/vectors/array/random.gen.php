<?php
// Generates array/random.json: seeded MT19937 (mt_srand/mt_rand), its range mapping, the legacy
// MT_RAND_PHP mode, shuffle() and array_rand(), plus argument errors (random_int included).
//
//   cd crates/twig-value/tests/vectors
//   MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//       php /t/array/random.gen.php > array/random.json
//
// seq:     [seed, mode, diags of mt_srand, [mt_rand() x n]]
// ranges:  [seed, mode, [[min, max, [results...]], ...]] (draws in order, one generator per row)
// shuffle: [seed, encn(input), [encn(after shuffle #1), encn(after shuffle #2)]]
// rand:    [seed, encn(input), num, res() of array_rand, then mt_rand() to show the state]
// errors:  [function, [args as enc()], res(), diags]

declare(strict_types=0);
require __DIR__.'/../common.inc.php';
require __DIR__.'/../values.inc.php';
require __DIR__.'/array.inc.php';

$seeds = [0, 1, 42, 12345, 4294967295, -1, PHP_INT_MAX, 4294967296 + 5, PHP_INT_MIN, 20261125];

$seq = [];
foreach ($seeds as $seed) {
    foreach ([MT_RAND_MT19937, MT_RAND_PHP] as $mode) {
        [, $d] = capture(static fn () => mt_srand($seed, $mode));
        $out = [];
        $n = $seed === 42 ? 1500 : 30;
        for ($i = 0; $i < $n; $i++) {
            $out[] = mt_rand();
        }
        $seq[] = [$seed, $mode, diags($d), $out];
    }
}

$bounds = [[0, 0], [0, 1], [1, 6], [-10, 10], [0, 99], [0, 255], [0, 3], [0, 2147483647], [0, 4294967295],
    [0, 4294967296], [-2147483648, 2147483647], [-1099511627776, 1099511627776], [0, PHP_INT_MAX],
    [PHP_INT_MIN, -1], [PHP_INT_MIN, PHP_INT_MAX], [5, 5], [PHP_INT_MAX - 3, PHP_INT_MAX], [0, 8589934591],
    [1, 3000000000], [-5, 6000000000000], [PHP_INT_MIN, 0], [PHP_INT_MIN + 1, PHP_INT_MAX]];
$ranges = [];
foreach ([0, 1, 42, 777] as $seed) {
    foreach ([MT_RAND_MT19937, MT_RAND_PHP] as $mode) {
        @mt_srand($seed, $mode);
        $rows = [];
        foreach ($bounds as [$min, $max]) {
            $r = [];
            for ($i = 0; $i < 8; $i++) {
                $r[] = mt_rand($min, $max);
            }
            $rows[] = [$min, $max, $r];
        }
        $ranges[] = [$seed, $mode, $rows];
    }
}

$inputs = [[], [1], [1, 2], range(0, 9), range('a', 'z'), ['a' => 1, 'b' => 2, 'c' => 3, 5 => 4, 'e' => 5],
    [5 => 'x', 3 => 'y', 10 => 'z'], array_fill(0, 40, 'same'), range(100, 160)];
$h = range(0, 7);
unset($h[2], $h[5]);
$inputs[] = $h;

$shuffle = [];
foreach ([0, 3, 99] as $seed) {
    foreach ($inputs as $in) {
        mt_srand($seed);
        $a = $in;
        shuffle($a);
        $first = encn($a);
        shuffle($a);
        $shuffle[] = [$seed, encn($in), [$first, encn($a)]];
    }
}

$rand = [];
foreach ([0, 5] as $seed) {
    foreach ($inputs as $in) {
        $nums = array_unique([1, 2, 3, count($in) - 1, count($in), count($in) + 1, 0, -1, intdiv(count($in), 2),
            intdiv(count($in), 2) + 1]);
        foreach ($nums as $num) {
            if ($num === 1 && $in === $h) {
                continue; // holes: num == 1 samples hash slots (documented deviation)
            }
            mt_srand($seed);
            [$r, $d] = capture(static fn () => array_rand($in, $num));
            $rand[] = [$seed, encn($in), $num, resn($r), diags($d), mt_rand()];
        }
    }
}

$errors = [];
$errorCalls = [
    ['mt_rand', [5, 1]],
    ['mt_rand', [1]],
    ['mt_rand', [1, 2, 3]],
    ['mt_rand', ['a', 5]],
    ['mt_rand', [1, 'b']],
    ['mt_rand', [1.5, 10]],
    ['mt_rand', [null, 10]],
    ['mt_rand', [0, 1e30]],
    ['mt_rand', ['3', '3']],
    ['mt_rand', [true, true]],
    ['random_int', [5, 1]],
    ['random_int', [7, 7]],
    ['random_int', ['a', 1]],
    ['random_int', [1.5, 1]],
    ['random_int', [null, 0]],
    ['array_rand', [[]]],
    ['array_rand', [[1, 2], 0]],
    ['array_rand', [[1, 2], 3]],
    ['array_rand', [[1, 2], -1]],
    ['array_rand', [[1, 2], 'x']],
    ['array_rand', [[1, 2], 1.5]],
    ['array_rand', ['x']],
];
foreach ($errorCalls as [$fn, $args]) {
    mt_srand(1);
    [$r, $d] = capture(static fn () => $fn(...$args));
    $errors[] = [$fn, array_map('enc', $args), res($r), diags($d)];
}

emit('array/random.gen.php', [
    'seq' => $seq,
    'ranges' => $ranges,
    'shuffle' => $shuffle,
    'rand' => $rand,
    'errors' => $errors,
    'messages' => messages(),
]);
