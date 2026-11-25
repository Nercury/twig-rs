<?php
// Deterministic double set shared by float_format.gen.php and float_precision.gen.php.
// Returns [hex bits (big-endian) => float], sorted by bits.

declare(strict_types=1);

function float_values(int $scale): array
{
    mt_srand(20261125);
    $vals = [];
    $add = static function (float $f) use (&$vals): void {
        $vals[bin2hex(pack('E', $f))] = $f;
    };
    $bits = static fn (int $hi, int $lo): float => unpack('E', pack('NN', $hi, $lo))[1];
    $rnd32 = static fn (): int => (mt_rand(0, 0xFFFF) << 16) | mt_rand(0, 0xFFFF);
    $up = static fn (float $f): float => unpack('E', pack('J', unpack('J', pack('E', $f))[1] + 1))[1];
    $down = static fn (float $f): float => unpack('E', pack('J', unpack('J', pack('E', $f))[1] - 1))[1];

    // Specials and textbook values.
    foreach ([0.0, -0.0, INF, -INF, NAN, 1.0, -1.0, 0.1, 0.2, 0.3, 0.1 + 0.2, 1 / 3, 2 / 3, 10 / 3, M_PI, M_E,
        sqrt(2), 1 / 7, 7.0 / 3, 100.0, 1e14, 1e15, 1e16, 1e17, 1e25, 1e100, 0.0001, 0.00001, 0.000123, 1.5e-7,
        PHP_INT_MAX + 1, (float) PHP_INT_MAX, (float) PHP_INT_MIN, 1.7976931348623157E+308, 4.9E-324,
        2.2250738585072014E-308, 99999999999999.99, 999999999999999.9, 9007199254740993.0, 123456789012345.0,
        1234567890123456.0, 12345678901234567.0, 662936471232937.25, 1e22, 1e23, 0.5, 1.5, 2.5, 3.5, 0.25,
        0.125, 0.375, 1234.5, 1235.5, 23119411616519.5, 63151216125196.5, 100000000000001.0, 100000000000004.0,
        100000000000005.0, 100000000000006.0, 100000000000015.0, 120000000000001.0, 123456789012305.0,
        123456789012315.0, 900000000000005.0, 999999999999995.0, 566874425495005.0, 120005.0, 120015.0, 120004.0,
    ] as $f) {
        $add($f);
    }
    // Powers of two (all exponents) and of ten.
    for ($e = -1074; $e <= 1023; $e++) {
        $add(2.0 ** $e);
    }
    for ($e = -323; $e <= 308; $e++) {
        $add((float) ('1e'.$e));
        $add((float) ('5e'.$e));
    }
    // Random finite bit patterns.
    for ($i = 0; $i < 20 * $scale; $i++) {
        $f = $bits($rnd32(), $rnd32());
        if (is_finite($f)) {
            $add($f);
        }
    }
    // Random subnormals.
    for ($i = 0; $i < 2 * $scale; $i++) {
        $add($bits(mt_rand(0, 0x000FFFFF) | (mt_rand(0, 1) << 31), $rnd32()));
    }
    // Random short decimals (1..17 significant digits, wide exponent range).
    for ($i = 0; $i < 8 * $scale; $i++) {
        $nd = mt_rand(1, 17);
        $m = (string) mt_rand(1, 9);
        for ($j = 1; $j < $nd; $j++) {
            $m .= (string) mt_rand(0, 9);
        }
        $f = (float) ($m.'e'.mt_rand(-40, 40));
        $add(mt_rand(0, 3) === 0 ? -$f : $f);
    }
    // 15-digit integers ending in 5 (precision-14 ties), their /10 and /1000, and neighbours.
    for ($i = 0; $i < (int) (2.5 * $scale); $i++) {
        $n = mt_rand(10000000000000, 99999999999999) * 10 + 5;
        $add((float) $n);
        $add((float) $n / 10);
        $add((float) $n / 1000);
        $add((float) ($n - 1));
        $add((float) ($n + 1));
    }
    // Integer ties at other positions (keep-zeros quirk of dtoa's small-integer path).
    for ($i = 0; $i < 2 * $scale; $i++) {
        $digits = mt_rand(2, 15);
        $keep = mt_rand(1, $digits - 1);
        $head = mt_rand((int) (10 ** ($keep - 1)), (int) (10 ** $keep) - 1);
        $tail = $digits - $keep;
        $n = $head * (10 ** $tail) + 5 * (10 ** ($tail - 1));
        $add((float) $n);
        $add((float) ($n + 10 ** max(0, $tail - 2)));
    }
    // Values around the exponential-notation thresholds.
    foreach ([1e-5, 1e-4, 1e14, 1e15, 1e16, 1e17, 1e18] as $t) {
        for ($k = 0, $f = $t; $k < 40; $k++) {
            $add($f = $down($f));
        }
        for ($k = 0, $f = $t; $k < 40; $k++) {
            $add($f = $up($f));
        }
    }
    // Products and quotients as templates produce them.
    for ($i = 0; $i < (int) (2.5 * $scale); $i++) {
        $a = mt_rand(1, 100000) / 100;
        $b = mt_rand(1, 1000) / 10;
        $add($a * $b);
        $add($a / $b);
        $add($a + $b / 3);
    }
    ksort($vals, SORT_STRING);

    return $vals;
}

function emit_rows(array $rows): void
{
    $n = count($rows);
    foreach ($rows as $k => $r) {
        echo json_encode($r, JSON_UNESCAPED_SLASHES), $k + 1 < $n ? ",\n" : "\n";
    }
}
