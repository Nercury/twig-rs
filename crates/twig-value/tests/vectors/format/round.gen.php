<?php
// Generates format/round.json: round() (PHP 8.4+ algorithm), floor(), ceil(), abs() and Twig's
// round filter (CoreExtension::round) against PHP 8.5.13 / Twig 3.31.1.
//
//   cd crates/twig-value/tests/vectors
//   MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//       php /t/format/round.gen.php > format/round.json
//
// floats: [double bits, places, mode, result bits] for round(float, places, mode)
// ints:   [int, places, result bits] for round(int, places)
// values: typed values (../values.inc.php enc()) used by cases and twig
// cases:  [function, [argument value indices], result, diags?] (round/floor/ceil/abs calls)
// twig:   [[value, precision, method value indices], result, diags?] for CoreExtension::round()
// result: typed value (enc()) or {"err": [class, message]}

declare(strict_types=0);
require __DIR__.'/../common.inc.php';
require __DIR__.'/../values.inc.php';
require __DIR__.'/classes.inc.php';
require '/oracle/vendor/autoload.php';
mt_srand(20261128);

$specials = [0.0, -0.0, 0.5, -0.5, 1.5, 2.5, -2.5, 3.5, 0.285, 1.955, 5.045, 5.055, 1.45, 1.005, 1.255,
    0.1 + 0.2, 1234567.891, 123456.0, 1.23456789, 1.5e-20, 4503599627370495.5, 4503599627370497.0,
    9.2233720368547758E+18, 1e15, 1e16, 1e22, 1e23, 1e300, -1e300, 5e-324, 2.2250738585072014E-308,
    0.49999999999999994, -0.49999999999999994, NAN, INF, -INF, 1 / 3, 2 / 3, 999.995, 0.045, 0.055];
$places = [-400, -309, -308, -30, -23, -22, -17, -16, -15, -10, -5, -3, -2, -1, 0, 1, 2, 3, 5, 10, 14, 15,
    16, 17, 22, 23, 25, 30, 308, 309, 330, 400, PHP_INT_MAX, PHP_INT_MIN, 2147483648, -2147483649];

$floats = [];
foreach ($specials as $x) {
    foreach ($places as $p) {
        foreach ([1, 2, 3, 4, 5, 6, 7, 8] as $mode) {
            [$r, $d] = capture(static fn () => round($x, $p, $mode));
            $floats[] = [fbits($x), $p, $mode, fbits($r)];
        }
    }
}
for ($i = 0; $i < 30000; $i++) {
    $x = humanfloat();
    $p = match (mt_rand(0, 5)) {
        0 => mt_rand(-30, 30),
        1 => mt_rand(-330, 330),
        default => mt_rand(-4, 8),
    };
    $mode = mt_rand(1, 8);
    [$r, $d] = capture(static fn () => round($x, $p, $mode));
    $floats[] = [fbits($x), $p, $mode, fbits($r)];
}

$ints = [];
foreach ([0, 1, -1, 5, 15, -15, 35, 45, 49, 50, 99, 1234, -1234, 2 ** 53 + 1, PHP_INT_MAX, PHP_INT_MIN] as $n) {
    foreach ([-20, -19, -16, -10, -3, -2, -1, 0, 1, 2] as $p) {
        [$r, $d] = capture(static fn () => round($n, $p));
        $ints[] = [$n, $p, fbits($r)];
    }
}

$values = [];
function vi($v): int
{
    global $values;
    $values[] = enc($v);
    return count($values) - 1;
}

$cases = [
    ['round', []],
    ['round', [1.5, 0, 1, 4]],
    ['round', [2.5]],
    ['round', [42]],
    ['round', ['2.5']],
    ['round', ['3']],
    ['round', [' 2.5']],
    ['round', ['2.5 ']],
    ['round', ['1e3']],
    ['round', ['abc']],
    ['round', ['12abc']],
    ['round', [null]],
    ['round', [true]],
    ['round', [false]],
    ['round', [[]]],
    ['round', [new stdClass()]],
    ['round', [new S('5')]],
    ['round', [1.955, '2']],
    ['round', [1.955, 2.7]],
    ['round', [1.955, '2.7']],
    ['round', [1.955, null]],
    ['round', [1.955, 'x']],
    ['round', [1.955, true]],
    ['round', [1.955, 1e20]],
    ['round', [1.955, NAN]],
    ['round', [2.5, 0, 3]],
    ['round', [2.5, 0, '3']],
    ['round', [2.5, 0, 3.5]],
    ['round', [2.5, 0, 0]],
    ['round', [2.5, 0, 9]],
    ['round', [2.5, 0, -1]],
    ['round', [2.5, 0, null]],
    ['round', [2.5, 0, 'x']],
    ['round', [2.5, 0, []]],
    ['round', [2.5, 0, new stdClass()]],
    ['round', [2.5, 0, Suit::Hearts]],
    ['round', [2.5, 0, RoundingMode::HalfAwayFromZero]],
    ['round', [2.5, 0, RoundingMode::HalfTowardsZero]],
    ['round', [2.5, 0, RoundingMode::HalfEven]],
    ['round', [3.5, 0, RoundingMode::HalfOdd]],
    ['round', [-2.5, 0, RoundingMode::TowardsZero]],
    ['round', [-2.1, 0, RoundingMode::AwayFromZero]],
    ['round', [-2.1, 0, RoundingMode::NegativeInfinity]],
    ['round', [-2.9, 0, RoundingMode::PositiveInfinity]],
    ['round', [35, -1]],
    ['round', [35, 1]],
    ['round', [PHP_INT_MAX, -1]],
    ['floor', []],
    ['floor', [1.5, 2]],
    ['floor', [-1.5]],
    ['floor', [7]],
    ['floor', ['7.9']],
    ['floor', ['x']],
    ['floor', [null]],
    ['floor', [true]],
    ['floor', [[]]],
    ['floor', [-0.0]],
    ['floor', [NAN]],
    ['floor', [-INF]],
    ['floor', [PHP_INT_MAX]],
    ['ceil', []],
    ['ceil', [-0.5]],
    ['ceil', [1.0000001]],
    ['ceil', ['-0.5']],
    ['ceil', [new S('1.5')]],
    ['ceil', [false]],
    ['ceil', [PHP_INT_MIN]],
    ['abs', []],
    ['abs', [1, 2]],
    ['abs', [-5]],
    ['abs', [PHP_INT_MIN]],
    ['abs', [PHP_INT_MIN + 1]],
    ['abs', [-0.0]],
    ['abs', [-1.5]],
    ['abs', [-INF]],
    ['abs', [NAN]],
    ['abs', ['-3']],
    ['abs', ['-3.5']],
    ['abs', ['1e3']],
    ['abs', [' 12']],
    ['abs', ['12 ']],
    ['abs', ['-9223372036854775808']],
    ['abs', ['9223372036854775808']],
    ['abs', [null]],
    ['abs', [false]],
    ['abs', [true]],
    ['abs', ['12abc']],
    ['abs', ['x']],
    ['abs', ['']],
    ['abs', [[]]],
    ['abs', [new stdClass()]],
    ['abs', [Suit::Hearts]],
];
$case_rows = [];
foreach ($cases as [$fn, $args]) {
    $idx = array_map('vi', $args);
    [$r, $d] = capture(static fn () => $fn(...$args));
    $case_rows[] = row([$fn, $idx], $r, $d, 'enc');
}

$twig_values = [1.255, '1.255', 42, -1.5, 2.5, 'abc', null, true, [1], 1e20, -0.5, 5.055, '12abc'];
$precisions = [0, 1, 2, -1, -2, '2', 2.5, null, true, 400, -400, 'x', []];
$methods = ['common', 'ceil', 'floor', 'round', 'CEIL', null, 1];
$twig = [];
foreach ($twig_values as $x) {
    foreach ($precisions as $p) {
        foreach ($methods as $mth) {
            [$r, $d] = capture(static fn () => \Twig\Extension\CoreExtension::round($x, $p, $mth));
            $twig[] = row([[vi($x), vi($p), vi($mth)]], $r, $d, 'enc');
        }
    }
}

emit('format/round.gen.php', [
    'floats' => $floats,
    'ints' => $ints,
    'values' => $values,
    'cases' => $case_rows,
    'twig' => $twig,
]);
