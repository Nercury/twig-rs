<?php
// Generates format/number_format.json: number_format() against PHP 8.5.13.
//
//   cd crates/twig-value/tests/vectors
//   MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//       php /t/format/number_format.gen.php > format/number_format.json
//
// floats: [double bits, decimals, result] with the default separators
// seps:   [double bits, decimals, decimal separator, thousands separator, result]
// ints:   [int, decimals, result]
// cases:  [[argument value indices], result, diags?] for whole calls (coercions and errors)
// values: typed values (../values.inc.php enc()) of the cases
// result: the output (JSON string when valid UTF-8, else {"hex"}) or {"err": [class, message]}

declare(strict_types=0);
require __DIR__.'/../common.inc.php';
require __DIR__.'/../values.inc.php';
require __DIR__.'/classes.inc.php';
mt_srand(20261127);

$specials = [0.0, -0.0, 0.5, -0.5, 1.5, 2.5, -2.5, 0.05, 0.005, 1.005, 1.255, 0.285, 1.955, 5.045, 5.055,
    1234.5, -1234.567, 1234567.891, 0.123456789, 1e15, 1e16, 1e20, 1e22, 1e23, 1e100, 1e308, -1e308,
    4503599627370495.5, 4503599627370496.0, -4503599627370496.0, 9007199254740993.0, 9.2233720368547758E+18,
    -9.2233720368547758E+18, 1.8446744073709552E+19, 5e-324, 2.2250738585072014E-308, 0.1 + 0.2, 1 / 3,
    999.995, 999999.9999, 0.0001, 0.00001, 0.49999999999999994, NAN, INF, -INF, 123456789012345.67,
    -0.001, -0.004, -0.005, 99.5, 999.5];
$decs = [-25, -20, -19, -18, -17, -16, -10, -5, -3, -2, -1, 0, 1, 2, 3, 5, 10, 14, 15, 16, 17, 20, 30, 53];

$floats = [];
foreach ($specials as $x) {
    foreach ($decs as $dec) {
        [$r, $d] = capture(static fn () => number_format($x, $dec));
        $floats[] = row([fbits($x), $dec], $r, $d, 'out_enc');
    }
}
for ($i = 0; $i < 30000; $i++) {
    $x = humanfloat();
    $dec = mt_rand(0, 3) === 0 ? mt_rand(-20, 0) : mt_rand(0, 25);
    [$r, $d] = capture(static fn () => number_format($x, $dec));
    $floats[] = row([fbits($x), $dec], $r, $d, 'out_enc');
}
// Precision beyond PHP's 318 printf digits, and huge places.
foreach ([0.1, 1.5, 5e-324, 1e300, -2.5, 123.456] as $x) {
    foreach ([300, 317, 318, 319, 320, 330, 400, 2147483647] as $dec) {
        if ($dec > 1000) {
            continue;
        }
        [$r, $d] = capture(static fn () => number_format($x, $dec));
        $floats[] = row([fbits($x), $dec], $r, $d, 'out_enc');
    }
}

$sep_list = [['.', ','], [',', '.'], ['', ''], ['dp', 'ts'], ["\u{2009}", "\u{a0}"], [' ', "'"], ['', ','], ['.', '']];
$seps = [];
for ($i = 0; $i < 3000; $i++) {
    $x = humanfloat();
    $dec = mt_rand(-5, 12);
    [$dp, $ts] = $sep_list[mt_rand(0, count($sep_list) - 1)];
    [$r, $d] = capture(static fn () => number_format($x, $dec, $dp, $ts));
    $seps[] = row([fbits($x), $dec, $dp, $ts], $r, $d, 'out_enc');
}

$ints = [];
$int_specials = [0, 1, -1, 5, -5, 49, 50, -50, 99, 999, 1000, -1000, 1234, -1234, 999999, 1000000, 4999,
    5000, PHP_INT_MAX, PHP_INT_MIN, PHP_INT_MAX - 1, PHP_INT_MIN + 1, 4999999999999999999, 5000000000000000000,
    -5000000000000000000, 9223372036854775000];
foreach ($int_specials as $n) {
    foreach ([-21, -20, -19, -18, -10, -4, -3, -2, -1, 0, 1, 2, 5] as $dec) {
        [$r, $d] = capture(static fn () => number_format($n, $dec));
        $ints[] = row([$n, $dec], $r, $d, 'out_enc');
    }
}
for ($i = 0; $i < 3000; $i++) {
    $n = mt_rand(0, 1) ? mt_rand(PHP_INT_MIN, PHP_INT_MAX) : mt_rand(-10 ** 7, 10 ** 7);
    $dec = mt_rand(-20, 6);
    [$r, $d] = capture(static fn () => number_format($n, $dec));
    $ints[] = row([$n, $dec], $r, $d, 'out_enc');
}

$values = [];
function vi($v): int
{
    global $values;
    $values[] = enc($v);
    return count($values) - 1;
}

$cases = [
    [],
    [1234.5],
    [1234],
    [null],
    [true],
    ['1234.5'],
    [' 12'],
    ['12 '],
    ['1e3'],
    ['abc'],
    ['12abc'],
    [[]],
    [new stdClass()],
    [new S('5')],
    [1234.5, null],
    [1234.5, '2'],
    [1234.5, 1.7],
    [1234.5, '1.7'],
    [1234.5, 'x'],
    [1234.5, true],
    [1234.5, 2.0],
    [1234.5, 1e20],
    [1234.5, NAN],
    [1234.5, 1000],
    [1234.5, PHP_INT_MIN],
    [1234.5, -2147483649],
    [12, 1000],
    [12, -2147483649],
    [12, PHP_INT_MIN],
    [1234.5, 2, null, null],
    [1234.5, 2, ',', null],
    [1234.5, 2, null, ' '],
    [1234.5, 2, 1, 2],
    [1234.5, 2, 1.5, true],
    [1234.5, 2, [], '.'],
    [1234.5, 2, '.', []],
    [1234.5, 2, new S(','), new S('.')],
    [1234.5, 2, '.', ',', 'extra'],
    [12345678, 2, '', ''],
    [-0.0, 2],
    [-1234567.891, -3, '.', ' '],
    [9007199254740993, 2],
];
$case_rows = [];
foreach ($cases as $args) {
    $idx = array_map('vi', $args);
    [$r, $d] = capture(static fn () => number_format(...$args));
    $case_rows[] = row([$idx], $r, $d, 'out_enc');
}

emit('format/number_format.gen.php', [
    'floats' => $floats,
    'seps' => $seps,
    'ints' => $ints,
    'values' => $values,
    'cases' => $case_rows,
]);
