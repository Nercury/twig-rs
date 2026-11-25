<?php
// Generates pow.json: PHP `**` (pow_function_base + the C library pow() of glibc 2.41 with FMA).
//
//   cd crates/twig-value/tests/vectors
//   MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//       php /t/pow.gen.php > pow.json
//
// ints:   [base, exponent, result] for int ** int; result is an int or {"f": bits}.
// floats: [x bits, y bits, result bits] for float ** float (big-endian IEEE hex), i.e. C pow(x, y).
// diags:  [x bits, y bits, [diagnostics]] for the cases that emit diagnostics (0 ** negative).

declare(strict_types=0);
require __DIR__.'/common.inc.php';
mt_srand(20261125);

function fbits(float $f): string
{
    return bin2hex(pack('E', $f));
}

function frand(): float
{
    return mt_rand() / mt_getrandmax();
}

/** A random double from the full bit space (finite only). */
function anybits(): float
{
    do {
        $hi = mt_rand(0, 0xFFFFFFFF);
        $lo = mt_rand(0, 0xFFFFFFFF);
        $f = unpack('E', pack('NN', $hi, $lo))[1];
    } while (is_nan($f) || is_infinite($f));
    return $f;
}

$ints = [];
for ($b = -60; $b <= 60; $b++) {
    for ($e = 0; $e <= 80; $e++) {
        $ints[] = [$b, $e];
    }
}
for ($b = -20; $b <= 20; $b++) {
    for ($e = -20; $e <= -1; $e++) {
        $ints[] = [$b, $e];
    }
}
foreach ([PHP_INT_MAX, PHP_INT_MIN, PHP_INT_MAX - 1, PHP_INT_MIN + 1, 3037000499, 3037000500, -3037000500,
    2097151, 2097152, 4294967296, -4294967296, 10, 7, 3, 2, -2, 1, -1, 0] as $b) {
    foreach ([0, 1, 2, 3, 4, 5, 7, 8, 13, 16, 21, 31, 32, 33, 40, 62, 63, 64, 65, 100, 127, 1000, 1023, 1024,
        1075, 9999, PHP_INT_MAX, PHP_INT_MAX - 1, -1, -2, -63, -64, -1075, PHP_INT_MIN] as $e) {
        $ints[] = [$b, $e];
    }
}
for ($i = 0; $i < 3000; $i++) {
    $b = mt_rand(0, 3) === 0 ? mt_rand(PHP_INT_MIN, PHP_INT_MAX) : mt_rand(-100000, 100000);
    $e = mt_rand(0, 2) === 0 ? mt_rand(0, 200) : mt_rand(0, 12);
    $ints[] = [$b, $e];
}
$int_rows = [];
foreach ($ints as [$b, $e]) {
    [$r, $d] = capture(static fn () => $b ** $e);
    $int_rows[] = [$b, $e, is_int($r) ? $r : ['f' => fbits($r)]];
}

$pairs = [];
$specials = [0.0, -0.0, 1.0, -1.0, 2.0, -2.0, 0.5, -0.5, 3.0, 10.0, 1.5, -1.5, INF, -INF, NAN,
    PHP_FLOAT_MAX, -PHP_FLOAT_MAX, PHP_FLOAT_MIN, 4.9e-324, -4.9e-324, 2.0 ** 52, 2.0 ** 53, 2.0 ** 63,
    1e300, 1e-300, 0.1, 1.0000000000000002, 0.9999999999999999, 1e-17, 1e17, 1075.0, -1075.0, 1024.0,
    -1074.0, 1023.5, 0.3333333333333333, 2.0 ** 62, 2.0 ** 64, -(2.0 ** 63)];
foreach ($specials as $x) {
    foreach ($specials as $y) {
        $pairs[] = [$x, $y];
    }
}
// Moderate bases and exponents (results mostly in the normal range).
for ($i = 0; $i < 12000; $i++) {
    $pairs[] = [frand() * 100, (frand() - 0.5) * 40];
}
// Log-uniform bases, exponents scaled so the result stays representable.
for ($i = 0; $i < 8000; $i++) {
    $x = (0.5 + frand()) * 2.0 ** mt_rand(-1000, 1000);
    $lx = log($x);
    $y = $lx == 0.0 ? frand() : (frand() * 1400 - 700) / $lx;
    $pairs[] = [$x, $y];
}
// Results near overflow and in the subnormal range (specialcase paths).
for ($i = 0; $i < 4000; $i++) {
    $x = (0.5 + frand() * 4) ;
    $lx = log($x);
    if ($lx == 0.0) {
        continue;
    }
    $target = mt_rand(0, 1) ? 709.0 + frand() * 1.5 : -708.0 - frand() * 37.0;
    $pairs[] = [$x, $target / $lx];
    $pairs[] = [1.0 / $x, -$target / $lx];
}
// Bases near 1 with huge exponents.
for ($i = 0; $i < 3000; $i++) {
    $x = 1.0 + (frand() - 0.5) * 2.0 ** mt_rand(-52, -20);
    $pairs[] = [$x, (frand() - 0.5) * 2.0 ** mt_rand(20, 62)];
}
// Negative bases with integral exponents (odd/even sign handling).
for ($i = 0; $i < 3000; $i++) {
    $pairs[] = [-frand() * 50, (float) mt_rand(-300, 300)];
}
// Integer-valued doubles (int ** int overflow fallbacks use pow((double)l2, i)).
for ($i = 0; $i < 3000; $i++) {
    $pairs[] = [(float) mt_rand(-1000000, 1000000), (float) mt_rand(0, 60)];
}
// Arbitrary bit patterns.
for ($i = 0; $i < 4000; $i++) {
    $pairs[] = [anybits(), anybits()];
    $pairs[] = [abs(anybits()), (frand() - 0.5) * 8];
}

$float_rows = [];
$diag_rows = [];
foreach ($pairs as [$x, $y]) {
    [$r, $d] = capture(static fn () => $x ** $y);
    $float_rows[] = [fbits($x), fbits($y), fbits($r)];
    if ($d) {
        $diag_rows[] = [fbits($x), fbits($y), $d];
    }
}

$round_rows = [];
$xs = [0.0, -0.0, 0.5, -0.5, 1.5, -1.5, 2.5, 0.49999999999999994, -0.49999999999999994, 4503599627370495.5,
    -4503599627370495.5, 4503599627370496.0, 9007199254740993.0, 1e300, -1e-300, PHP_FLOAT_MIN, 4.9e-324,
    -4.9e-324, INF, -INF, NAN, PHP_FLOAT_MAX];
for ($i = 0; $i < 2000; $i++) {
    $xs[] = (frand() - 0.5) * 2.0 ** mt_rand(-60, 60);
    $xs[] = anybits();
}
foreach ($xs as $x) {
    $round_rows[] = [fbits($x), fbits(floor($x)), fbits(ceil($x))];
}
$fmod_rows = [];
foreach ($xs as $k => $x) {
    $y = $xs[($k * 7 + 3) % count($xs)];
    foreach ([$y, 2.0 ** 64, $k % 2 ? 3.0 : -0.75] as $yy) {
        $fmod_rows[] = [fbits($x), fbits($yy), fbits(fmod($x, $yy))];
    }
}

emit('math.gen.php', ['ints' => $int_rows, 'floats' => $float_rows, 'diags' => $diag_rows,
    'round' => $round_rows, 'fmod' => $fmod_rows]);
