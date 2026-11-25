<?php
// Generates compare.json: == != < <= > >= <=> === !== over a value matrix (the 30x30 set of
// engine-semantics.md §7.10 plus edge cases: overflowing numeric strings, infinities, NaN, arrays,
// objects of several classes, enums, closures) and random number/string pairs.
//
//   cd crates/twig-value/tests/vectors
//   MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//       php /t/compare.gen.php > compare.json
//
// values:   typed values (values.inc.php enc())
// matrix:   [i, j, results] with one character per operator in the order of "ops":
//           t/f for booleans, -/0/+ for <=>, E for an exception; [i, j, results, extra] when an
//           operator raised diagnostics or an exception: extra = [[op index, err|null, diags], ...]
// random:   [a, b, <=> result, == result] for random scalar pairs
// messages: hex strings indexed by the message indices

declare(strict_types=0);
require __DIR__.'/common.inc.php';
require __DIR__.'/values.inc.php';
mt_srand(7102026);

$p2 = new P();
$p2->a = 2;
$p3 = new P();
$p3->dyn = 1;
$c1 = static fn () => 1;
$c2 = static fn () => 2;
$values = [
    // engine-semantics.md §7.10
    null, true, false, 0, 1, -1, 0.0, 1.5, NAN, INF, '', '0', '1', '-1', 'a', 'abc', '1abc', ' 1', '1 ', '1e0',
    '01', '0.0', '1.5', 'INF', [], [0], [1], [1, 2], new stdClass(), (object) ['a' => 1],
    // numbers
    -INF, -0.0, PHP_INT_MAX, PHP_INT_MIN, 2.0 ** 63, -(2.0 ** 63), 9007199254740993, 9007199254740992.0,
    0.1 + 0.2, 0.3, 1e100, 100, 42, 10, 2,
    // strings
    '9223372036854775807', '9223372036854775808', '9223372036854775809', '-9223372036854775808',
    '-9223372036854775809', '9.2233720368547758E+18', '1e1000', '2e1000', '-1e1000', '-2e1000', '1e3', '1000',
    '0x10', '16', '  1', ' ', "\0", "1\0", 'abd', 'ABC', 'B', 'Z', '10', '9', '9a', '1.0', '.5', '0.5', '-0',
    '-0.0', '+1', '1e-1000', 'NAN', '00', '1e2abc', '42abc', '1e2', '100', '9007199254740993',
    '9007199254740992', '9007199254740993.0', "\xff", "1 \n",
    // arrays
    [1 => 1, 0 => 0], ['a' => 1], ['b' => 1], ['a' => 1, 'b' => 2], ['b' => 2, 'a' => 1], [[1]], [null],
    [false], [NAN], ['1'], ['a'], [1, 3], [1 => 2, 0 => 9], [5], [1, [2]], [1, [3]],
    // objects
    (object) ['a' => 2], (object) ['a' => 1, 'b' => 2], (object) ['b' => 1], (object) ['a' => '1'],
    new P(), $p2, $p3, new T(), new S('abc'), new S('abd'), new S('1'), new S('1.5'), Suit::Hearts,
    Suit::Spades, U::A, Num::One, $c1, $c2,
];

$ops = [
    '==' => static fn ($a, $b) => $a == $b,
    '!=' => static fn ($a, $b) => $a != $b,
    '<' => static fn ($a, $b) => $a < $b,
    '<=' => static fn ($a, $b) => $a <= $b,
    '>' => static fn ($a, $b) => $a > $b,
    '>=' => static fn ($a, $b) => $a >= $b,
    '<=>' => static fn ($a, $b) => $a <=> $b,
    '===' => static fn ($a, $b) => $a === $b,
    '!==' => static fn ($a, $b) => $a !== $b,
];

function code($r): string
{
    if ($r instanceof \Throwable) {
        return 'E';
    }
    if (is_bool($r)) {
        return $r ? 't' : 'f';
    }
    return $r < 0 ? '-' : ($r > 0 ? '+' : '0');
}

$matrix = [];
foreach ($values as $i => $a) {
    foreach ($values as $j => $b) {
        $codes = '';
        $extra = [];
        $k = 0;
        foreach ($ops as $f) {
            [$r, $d] = capture(static fn () => $f($a, $b));
            $codes .= code($r);
            if ($d || $r instanceof \Throwable) {
                $extra[] = [$k, $r instanceof \Throwable ? res($r)['err'] : null, diags($d)];
            }
            $k++;
        }
        $matrix[] = $extra ? [$i, $j, $codes, $extra] : [$i, $j, $codes];
    }
}

/** A random numeric-looking (or almost numeric) string. */
function numstr(): string
{
    $ws = ['', '', '', '', ' ', "\t", "\n", '  ', "\r", "\v", "\f"];
    $s = $ws[mt_rand(0, count($ws) - 1)];
    $s .= ['', '', '', '-', '+'][mt_rand(0, 4)];
    $kind = mt_rand(0, 11);
    $digits = match (true) {
        $kind === 0 => str_repeat('9', mt_rand(17, 21)),
        $kind === 1 => (string) mt_rand(0, PHP_INT_MAX),
        $kind === 2 => '922337203685477580' . mt_rand(0, 9),
        $kind === 3 => '0' . mt_rand(0, 99),
        $kind === 4 => '',
        default => (string) mt_rand(0, 1000),
    };
    $s .= $digits;
    if (mt_rand(0, 2) === 0 || $digits === '') {
        $s .= '.' . (mt_rand(0, 3) ? mt_rand(0, 999) : '');
    }
    if (mt_rand(0, 3) === 0) {
        $s .= ['e', 'E'][mt_rand(0, 1)] . ['', '-', '+'][mt_rand(0, 2)] . (mt_rand(0, 5) ? mt_rand(0, 400) : '');
    }
    $s .= $ws[mt_rand(0, count($ws) - 1)];
    if (mt_rand(0, 6) === 0) {
        $s .= ['a', 'x', '1', '.', 'e', "\0", ' 1'][mt_rand(0, 6)];
    }
    return $s;
}

/** A random number (int or float) related to the strings above. */
function num()
{
    return match (mt_rand(0, 7)) {
        0 => mt_rand(-1000, 1000),
        1 => mt_rand(PHP_INT_MIN, PHP_INT_MAX),
        2 => mt_rand(-1000, 1000) / 8,
        3 => (float) mt_rand(-1000, 1000),
        4 => mt_rand(-1000000, 1000000) / 1000,
        5 => [INF, -INF, PHP_INT_MAX, PHP_INT_MIN, 2.0 ** 63, 0.0, -0.0, 1e300][mt_rand(0, 7)],
        6 => mt_rand() / mt_getrandmax() * 10 ** mt_rand(-20, 20),
        default => (float) mt_rand(PHP_INT_MIN, PHP_INT_MAX),
    };
}

$random = [];
for ($n = 0; $n < 16000; $n++) {
    switch ($n % 8) {
        case 0:
        case 1:
        case 2:
            $a = numstr();
            $b = mt_rand(0, 3) ? numstr() : (string) num();
            break;
        case 3:
            $a = num();
            $b = numstr();
            break;
        case 4:
            $a = numstr();
            $b = num();
            break;
        case 5:
            $a = num();
            $b = (string) num();
            break;
        case 6:
            $a = num();
            $b = num();
            break;
        default:
            $a = [null, true, false, '', '0'][mt_rand(0, 4)];
            $b = mt_rand(0, 1) ? numstr() : num();
    }
    [$r1, $d1] = capture(static fn () => $a <=> $b);
    [$r2, $d2] = capture(static fn () => $a == $b);
    $random[] = [enc($a), enc($b), $r1, $r2];
}

echo '{"generator":"compare.gen.php","php":"', PHP_VERSION, "\",\n";
echo '"ops":', json_encode(array_keys($ops)), ",\n";
echo '"values":', json_encode(array_map('enc', $values), JSON_UNESCAPED_SLASHES | JSON_THROW_ON_ERROR), ",\n";
foreach (['matrix' => $matrix, 'random' => $random] as $name => $rows) {
    echo '"', $name, "\":[\n";
    foreach ($rows as $k => $row) {
        echo json_encode($row, JSON_UNESCAPED_SLASHES | JSON_THROW_ON_ERROR), $k + 1 < count($rows) ? ",\n" : "\n";
    }
    echo "],\n";
}
echo '"messages":', json_encode(messages(), JSON_UNESCAPED_SLASHES | JSON_THROW_ON_ERROR), "}\n";
