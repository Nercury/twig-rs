<?php
// Generates numeric_strings.json: how PHP 8.5.13 classifies and converts strings.
//
//   cd crates/twig-value/tests/vectors
//   MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//       php /t/numeric_strings.gen.php > numeric_strings.json
//
// Per input string (hex): is_numeric(), $s + 0 (+ diagnostics), (int), (float) bits, the array key
// [$s => 1] normalises to, and $s % 1000003 (+ diagnostics).

declare(strict_types=0);
require __DIR__.'/common.inc.php';
mt_srand(424242);

$inputs = ['', ' ', '0', '00', '007', '-0', '+0', '-00', '1', '-1', '+1', '+-5', '-+5', ' - 5', '- 5', '-', '+',
    '.', '.5', '5.', '-.5', '+.5', '.e5', '1.e5', '0.e5', '1e', '1e+', '1e-', '1E5', '1e+5', '1e-5', '1e5e5',
    '1.5.5', '0x1A', '0b11', '012', '1_000', "1\0", "\0", "1\0 ", 'INF', 'inf', 'NAN', 'nan', 'Infinity', '1e400',
    '-1e400', '1e-400', '1e1000', '9223372036854775807', '9223372036854775808', '9223372036854775809',
    '-9223372036854775808', '-9223372036854775809', '18446744073709551615', '18446744073709551616',
    '99999999999999999999', '999999999999999999999', '00000000000000000000001', '1e100', '-1e100',
    '9223372036854775808e+', '9300000000000000000e-', '-9223372036854775809e+', '9223372036854775808e',
    '9300000000000000000x', ' 9223372036854775808', '9223372036854775807 ', ' -9223372036854775808 ',
    '-9223372036854775808 ', '1.9999999999999999', '1.5e3abc', '12abc', 'abc', '1 x', ' 1 ', "\t\n123\r\x0b\x0c",
    "123\n", "\n123", "\x0b1", "\x0c1", '1x', '7.5x', '9.5e18', '1e3', '.5x', "\u{661}", "1\xff", "\xff",
    '0.0', '-0.0', '1.0', '01', '1 ', '1e0', '+1.5e+3', '-.5e-3', '   ', "\n", '1e19999', '1e20000',
    '0.000000000000000000000000000000000000000000001e45', '123456789012345678901234567890',
    '1234567890123456789', '12345678901234567890', '-1234567890123456789', '-12345678901234567890'];

// Random strings over a numeric-ish alphabet.
$alpha = ['0', '1', '2', '5', '9', '9', '.', 'e', 'E', '+', '-', ' ', "\t", "\n", "\r", "\x0b", "\x0c", 'x', 'a', "\0", '_'];
for ($i = 0; $i < 6000; $i++) {
    $len = mt_rand(0, 12);
    $s = '';
    for ($j = 0; $j < $len; $j++) {
        $s .= $alpha[mt_rand(0, count($alpha) - 1)];
    }
    $inputs[] = $s;
}
// Structured numbers: ws, sign, digits, fraction, exponent, trailing data.
$ws = ['', '', '', ' ', "\t", "\n "];
$tails = ['', '', '', ' ', "\n", 'x', 'e', 'e+', '.', '..', ' 1', "\0"];
for ($i = 0; $i < 6000; $i++) {
    $s = $ws[mt_rand(0, count($ws) - 1)];
    $s .= ['', '', '-', '+'][mt_rand(0, 3)];
    $nd = mt_rand(0, 25);
    for ($j = 0; $j < $nd; $j++) {
        $s .= (string) mt_rand(0, 9);
    }
    if (mt_rand(0, 2) === 0) {
        $s .= '.';
        $nf = mt_rand(0, 6);
        for ($j = 0; $j < $nf; $j++) {
            $s .= (string) mt_rand(0, 9);
        }
    }
    if (mt_rand(0, 3) === 0) {
        $s .= ['e', 'E'][mt_rand(0, 1)].['', '+', '-'][mt_rand(0, 2)].mt_rand(0, 400);
    }
    $s .= $tails[mt_rand(0, count($tails) - 1)];
    $inputs[] = $s;
}
// Integers around the int64 boundaries.
foreach ([PHP_INT_MAX, PHP_INT_MIN] as $b) {
    for ($d = -3; $d <= 3; $d++) {
        $inputs[] = (string) ($b + $d);
    }
}
for ($i = 0; $i < 300; $i++) {
    $inputs[] = '922337203685477'.mt_rand(0, 9999);
    $inputs[] = '-922337203685477'.mt_rand(0, 9999);
    $inputs[] = '92233720368547758'.mt_rand(0, 99).(mt_rand(0, 1) ? '' : ' ');
}

$cases = [];
foreach (array_values(array_unique($inputs, SORT_STRING)) as $s) {
    $c = ['s' => bin2hex($s), 'is_numeric' => is_numeric($s)];
    [$r, $d] = capture(static fn () => $s + 0);
    $c['add'] = $r instanceof \TypeError ? ['err' => 'TypeError'] : typed($r);
    $c['add_diag'] = $d;
    $c['int'] = (int) $s;
    $c['float'] = bin2hex(pack('E', (float) $s));
    $k = array_key_first([$s => 1]);
    $c['key'] = is_int($k) ? ['int' => $k] : 'str';
    [$r, $d] = capture(static fn () => $s % 1000003);
    $c['mod'] = $r instanceof \TypeError ? ['err' => 'TypeError'] : typed($r);
    $c['mod_diag'] = $d;
    $cases[] = $c;
}
emit('numeric_strings.gen.php', ['cases' => $cases]);
