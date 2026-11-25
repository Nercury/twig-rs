<?php
// Generates format/sprintf.json: sprintf() / vsprintf() (php_formatted_print) against PHP 8.5.13.
//
//   cd crates/twig-value/tests/vectors
//   MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//       php /t/format/sprintf.gen.php > format/sprintf.json
//
// values:  typed values (../values.inc.php enc(); classes S, stdClass, Thrower)
// matrix:  [format, [[value index, result, diags?], ...]] for sprintf(format, value)
// floats:  [format, double bits, result, diags?] for random doubles and float conversions
//          (formats with `.*` take precision -1 as the first argument)
// ints:    [format, int, result, diags?] for random ints and integer conversions
// cases:   [function, [argument value indices], result, diags?] for whole calls (errors included)
// result:  the output (JSON string when valid UTF-8, else {"hex"}) or {"err": [class, message],
//          "prev"?: [class, message]}; diags: [[level, message], ...]

declare(strict_types=0);
require __DIR__.'/../common.inc.php';
require __DIR__.'/../values.inc.php';
require __DIR__.'/classes.inc.php';
mt_srand(20261126);

$values = [];
$index = [];
/** Index of a value in the value table (scalars deduplicated by their typed form). */
function vi($v): int
{
    global $values, $index;
    $e = enc($v);
    $key = is_object($v) ? 'o' . count($values) : json_encode($e);
    if (!isset($index[$key])) {
        $index[$key] = count($values);
        $values[] = $e;
    }
    return $index[$key];
}

$ints = [0, 1, -1, 42, -42, 255, 65, 321, PHP_INT_MAX, PHP_INT_MIN];
$floats = [0.0, -0.0, 1.0, -1.5, 0.1, 2.5, 1234.5678, 1e-10, 1e20, 1e300, 5e-324, NAN, INF, -INF, 99.995];
$strings = ['', 'abc', 'abcdefghij', "a\0b", "\u{e9}t\u{e9}", '12abc', '0x1A', ' 7', '3.25', '-0', '1e3'];
$others = [true, false, null, [], [1, 2], new S('obj')];

$by_conv = [
    's' => array_merge([0, -42, 1.5, -0.0, 1e100, 0.1 + 0.2, NAN], $strings, $others),
    'd' => array_merge($ints, [1.9, -1.9, 1e30, NAN, INF], ['12abc', '0x1A', ' 7', '1e30', 'abc'], [true, false, null, [], [1]]),
    'u' => array_merge($ints, [-1.9, 1e19, NAN], ['-5', 'x'], [true, null]),
    'c' => [65, 321, -191, 0, 10, 1.9, '66', true],
    'o' => array_merge($ints, [8.9, '17']),
    'x' => array_merge($ints, [255.5, 'ff']),
    'X' => array_merge($ints, [255.5]),
    'b' => array_merge($ints, [5.5, '5']),
    '%' => [1],
];
foreach (str_split('eEfFgGhH') as $c) {
    $by_conv[$c] = array_merge($floats, [42, '3.25', 'abc', true, null]);
}

$flags = ['', '-', '+', '0', ' ', '-0', '+0', '-+0', "'#", "-'#", "'0"];
$widths = ['', '1', '8'];
$precisions = ['', '.', '.0', '.2', '.12'];

$matrix = [];
foreach ($by_conv as $conv => $vals) {
    $idx = array_map('vi', $vals);
    foreach ($flags as $fl) {
        foreach ($widths as $w) {
            foreach ($precisions as $p) {
                $fmt = '%' . $fl . $w . $p . $conv;
                $rows = [];
                foreach ($vals as $k => $v) {
                    [$r, $d] = capture(static fn () => sprintf($fmt, $v));
                    $rows[] = row([$idx[$k]], $r, $d, 'out_enc');
                }
                $matrix[] = [$fmt, $rows];
            }
        }
    }
}

// Random doubles through the float conversions.
$float_rows = [];
for ($i = 0; $i < 24000; $i++) {
    $x = humanfloat();
    $conv = 'eEfFgGhH'[mt_rand(0, 7)];
    $prec = match (mt_rand(0, 5)) {
        0 => '',
        1 => '.' . mt_rand(0, 60),
        default => '.' . mt_rand(0, 20),
    };
    $width = mt_rand(0, 3) === 0 ? (string) mt_rand(1, 40) : '';
    $flag = ['', '', '-', '+', '0', '+0', ' ', "'*"][mt_rand(0, 7)];
    $fmt = '%' . $flag . $width . $prec . $conv;
    [$r, $d] = capture(static fn () => sprintf($fmt, $x));
    $float_rows[] = row([$fmt, fbits($x)], $r, $d, 'out_enc');
}
// Shortest round-trip (precision -1) for %g %G %h %H.
for ($i = 0; $i < 3000; $i++) {
    $x = humanfloat();
    $fmt = '%.*' . 'gGhH'[mt_rand(0, 3)];
    [$r, $d] = capture(static fn () => sprintf($fmt, -1, $x));
    $float_rows[] = row([$fmt, fbits($x)], $r, $d, 'out_enc');
}

// Random ints through the integer conversions.
$int_rows = [];
for ($i = 0; $i < 6000; $i++) {
    $n = match (mt_rand(0, 3)) {
        0 => mt_rand(PHP_INT_MIN, PHP_INT_MAX),
        1 => mt_rand(-1000, 1000),
        2 => mt_rand(-PHP_INT_MAX, PHP_INT_MAX) >> mt_rand(0, 62),
        default => [PHP_INT_MIN, PHP_INT_MAX, 0, -1][mt_rand(0, 3)],
    };
    $conv = 'duoxXbc'[mt_rand(0, 6)];
    $flag = ['', '-', '+', '0', '-0', '+0', "'_", ' '][mt_rand(0, 7)];
    $width = mt_rand(0, 2) === 0 ? (string) mt_rand(1, 70) : '';
    $prec = mt_rand(0, 4) === 0 ? '.' . mt_rand(0, 5) : '';
    $fmt = '%' . $flag . $width . $prec . $conv;
    [$r, $d] = capture(static fn () => sprintf($fmt, $n));
    $int_rows[] = row([$fmt, $n], $r, $d, 'out_enc');
}

// Whole calls, errors included. Arguments are [values...] including the format.
$s = new S('abc');
$cases = [
    ['sprintf', []],
    ['sprintf', [[]]],
    ['sprintf', [null]],
    ['sprintf', [1.5]],
    ['sprintf', [true]],
    ['sprintf', [$s]],
    ['sprintf', [new stdClass()]],
    ['sprintf', ['%']],
    ['sprintf', ['%', 1]],
    ['sprintf', ['%%']],
    ['sprintf', ['100%']],
    ['sprintf', ['%5%']],
    ['sprintf', ['%5%', 1]],
    ['sprintf', ['%d %d', 1]],
    ['sprintf', ['%d %d %d']],
    ['sprintf', ['%*d %z']],
    ['sprintf', ['%z', 1]],
    ['sprintf', ['%z']],
    ['sprintf', ["abc\0def%\0", 1]],
    ['sprintf', ["%\0", 1]],
    ['sprintf', ["%\xe9", 1]],
    ['sprintf', ["a\0b%sc\0", 'x']],
    ['sprintf', ['%l', 1]],
    ['sprintf', ['%ld|%lf|%5.1lf|%-ls', 42, 1.5, 2.25, 'x']],
    ['sprintf', ['%2147483647d', 1]],
    ['sprintf', ['%99999999999999999999d', 1]],
    ['sprintf', ['%.2147483647f', 1.0]],
    ['sprintf', ['%.2147483646f', 1.0]],
    ['sprintf', ['%.54e|%.53e|%.99g', 1.0, 1.0, 0.1]],
    ['sprintf', ['%.2147483647s', 'abc']],
    ['sprintf', ['%0$s', 1]],
    ['sprintf', ['%$s', 1]],
    ['sprintf', ['%2147483647$s', 1]],
    ['sprintf', ['%2147483646$s', 1]],
    ['sprintf', ['%1$s %1$s|%2$s %s %s', 'a', 'b']],
    ['sprintf', ['%3$s', 'a', 'b']],
    ['sprintf', ['%1$s %3$s', 'a', 'b']],
    ['sprintf', ["%'", 1]],
    ['sprintf', ["%'x", 1]],
    ['sprintf', ["%'x5d|%-'x5d|%'\x005d|%''5s", 42, 42, 42, 'a']],
    ['sprintf', ['%*d|%-*d|%0*d', 5, 42, 5, 42, 5, -42]],
    ['sprintf', ['%*d', '5', 42]],
    ['sprintf', ['%*d', 5.0, 42]],
    ['sprintf', ['%*d', -1, 42]],
    ['sprintf', ['%*d', 2147483648, 42]],
    ['sprintf', ['%*d', 0, 42]],
    ['sprintf', ['%*d']],
    ['sprintf', ['%*d', 3]],
    ['sprintf', ['%.*f|%.*e', 2, 1.005, 3, 1234.5678]],
    ['sprintf', ['%.*f', '2', 1.0]],
    ['sprintf', ['%.*f', -1, 1.0]],
    ['sprintf', ['%.*s', -1, 'abc']],
    ['sprintf', ['%.*f', -2, 1.0]],
    ['sprintf', ['%.*f', 2147483648, 1.0]],
    ['sprintf', ['%.*g|%.*G|%.*h|%.*H', -1, 0.1, -1, 1e100, -1, 1 / 3, -1, -2.5e-7]],
    ['sprintf', ['%.*g', 0, 1234.5]],
    ['sprintf', ['%.*f', 60, 1.5]],
    ['sprintf', ['%1$*2$d|%1$.*2$f', 42, 6]],
    ['sprintf', ['%3$*2$.*1$f', 2, 10, 3.14159]],
    ['sprintf', ['%*1$d', 7]],
    ['sprintf', ['%.*1$f', 2]],
    ['sprintf', ['%*0$d', 7]],
    ['sprintf', ['%*3$d', 1, 2]],
    ['sprintf', ['%s', $s]],
    ['sprintf', ['%5.2s|%-5s|', $s, $s]],
    ['sprintf', ['%s', new stdClass()]],
    ['sprintf', ['%s%s', new stdClass(), [1]]],
    ['sprintf', ['%s%s', new Thrower(), $s]],
    ['sprintf', ['%s%s%s', new stdClass(), new Thrower(), new stdClass()]],
    ['sprintf', ['%s%d%f', new stdClass(), new stdClass(), new stdClass()]],
    ['sprintf', ['%s %z', new stdClass()]],
    ['sprintf', ['%s %s', new stdClass()]],
    ['sprintf', ['%d|%f|%x', new stdClass(), $s, new stdClass()]],
    ['sprintf', ['%s|%d|%f', [1, 2], [1], []]],
    ['sprintf', ['%s|%d|%e', NAN, NAN, NAN]],
    ['sprintf', ['%d|%d|%u|%x', INF, -INF, 1e30, -1e19]],
    ['sprintf', ['%d|%d|%f', '1e30', '9223372036854775808', '1e400']],
    ['sprintf', ['%c|%c|%c|%5c|%-5c', 321, -191, 0, 65, 66]],
    ['sprintf', ['%u|%u', -1, PHP_INT_MIN]],
    ['sprintf', ['%+d|%+d|%+05d|%+-5d|% d', 0, -0, 5, 5, 5]],
    ['sprintf', ['%+05f|%+05f|%010f|%-012f|%+.1e|%+g', NAN, INF, -INF, 1.5, 0.0, 0.0]],
    ['sprintf', ['%05s|%-05s|%05.1f|%-07.2f|%07.2f', 'ab', 'ab', -2.34, -1.5, -1.5]],
    ['sprintf', ['%f|%e|%g|%F|%G|%h|%E', -0.0, -0.0, -0.0, -0.0, -0.0, -0.0, -0.0]],
    ['sprintf', ['%.0f|%.0f|%.0f|%.0f|%.1f|%.2f|%.2f', 0.5, 1.5, 2.5, -0.5, 0.25, 0.125, 0.375]],
    ['sprintf', ['%.50f|%.53f', 0.1, 5e-324]],
    ['sprintf', ['%.0e|%.0e|%.1e|%e', 5.0, 15.0, 0.25, 9.9999995]],
    ['sprintf', ['%g|%g|%g|%g|%g|%g|%g', 100000.0, 1000000.0, 0.0001, 0.00001, 123456789.0, 1e15, 1e-5]],
    ['sprintf', ['%.3g|%.10g|%.17g|%.20G', 1234567.0, 1 / 3, 0.1, 2 / 3]],
    ['sprintf', ['%e|%E', 1e-300, 1e300]],
    ['sprintf', ["%'*10s|%'*-10s|%'010d", 'pad', 'pad', -5]],
    ['sprintf', ['%-+5d|%+-5d|%0-5d|%-05d', 3, 3, 3, 3]],
    ['sprintf', ['%5.3d|%.3d|%5.3u', 7, -7, 7]],
    ['sprintf', ['%.0x|%.x|%5.0b|%-5.1o|%.X', 255, 255, 5, 8, 255]],
    ['sprintf', ['%. 5d', 42]],
    ['sprintf', ['%-', 1]],
    ['sprintf', ['%5', 1]],
    ['sprintf', ['%.', 1]],
    ['sprintf', ['%5.', 1]],
    ['sprintf', ['%1$', 1]],
    ['sprintf', ['%1$-', 1]],
    ['sprintf', ['%*', 1]],
    ['sprintf', ['%.*', 1]],
    ['sprintf', ['%.*', 1, 2]],
    ['sprintf', ['%.*1$', 1]],
    ['vsprintf', []],
    ['vsprintf', ['%s']],
    ['vsprintf', ['%s', 'x']],
    ['vsprintf', [[], []]],
    ['vsprintf', [null, []]],
    ['vsprintf', ['%s', [], 1]],
    ['vsprintf', ['%s %s', ['a']]],
    ['vsprintf', ['%3$s', ['a']]],
    ['vsprintf', ['%2$s %1$s', ['x' => 'a', 'y' => 'b']]],
    ['vsprintf', ['%.60f', [1.5]]],
    ['vsprintf', ['%d-%s', [5 => 7, 2 => 'z']]],
    ['vsprintf', ['%*d', ['5', 1]]],
    ['vsprintf', ['%s', [new stdClass()]]],
];
$case_rows = [];
foreach ($cases as [$fn, $args]) {
    $idx = array_map('vi', $args);
    [$r, $d] = capture(static fn () => $fn(...$args));
    $case_rows[] = row([$fn, $idx], $r, $d, 'out_enc');
}

emit('format/sprintf.gen.php', [
    'values' => $values,
    'matrix' => $matrix,
    'floats' => $float_rows,
    'ints' => $int_rows,
    'cases' => $case_rows,
]);
