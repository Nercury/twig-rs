<?php
// Generates array/sort.json: zend_sort parity and the sort family of ext/standard/array.c.
//
//   cd crates/twig-value/tests/vectors
//   MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//       php /t/array/sort.gen.php > array/sort.json
//
// zsort:    [n, input ints, keys after uasort() with an inconsistent comparator, comparator calls]
// stable:   [input ints, keys after asort()] (many ties: stability through the quicksort)
// arrays:   encn() arrays (values.inc.php enc() plus "next": the key `$a[] = x` would use, or null)
// flags:    [array index, function, flags, resn(result array), diags]
// user:     [array index, function, comparator name, resn(result array), diags, calls]
// natpairs: [a hex, b hex, strnatcmp, strnatcasecmp]
// messages: hex strings indexed by the message indices

declare(strict_types=0);
require __DIR__.'/../common.inc.php';
require __DIR__.'/../values.inc.php';
require __DIR__.'/array.inc.php';

mt_srand(20261125);

// --- zend_sort with an inconsistent comparator --------------------------------------------------
$sizes = array_merge(range(1, 20), [24, 31, 32, 33, 47, 48, 63, 64, 65, 100, 127, 128, 129, 200, 255,
    256, 257, 511, 512, 513, 1000, 1023, 1024, 1025, 1500, 2047, 2048, 2049, 3000, 4097]);
$zsort = [];
foreach ($sizes as $n) {
    $seeds = $n > 1000 ? 2 : 4;
    for ($s = 0; $s < $seeds; $s++) {
        $in = [];
        for ($i = 0; $i < $n; $i++) {
            $in[] = mt_rand(0, 999);
        }
        $calls = 0;
        $a = $in;
        uasort($a, static function ($x, $y) use (&$calls) {
            $calls++;
            return (($x * 31 + $y * 17 + ($x ^ $y)) % 5) - 2;
        });
        $zsort[] = [$n, $in, array_keys($a), $calls];
    }
}

$stable = [];
foreach ([17, 100, 1025, 3000] as $n) {
    $in = [];
    for ($i = 0; $i < $n; $i++) {
        $in[] = mt_rand(0, 9);
    }
    $a = $in;
    asort($a);
    $stable[] = [$in, array_keys($a)];
}

// --- arrays for the flag and comparator matrices -------------------------------------------------
$pool = [0, 1, -1, 2, 10, 9, 100, PHP_INT_MAX, PHP_INT_MIN, 0.0, -0.0, 1.5, -2.5, 9.0, 10.0, 1e100, INF, -INF,
    '0', '1', '-1', '10', '9', '9a', '1e1', ' 9', '9 ', '.5', '0x1A', '010', 'abc', 'ABC', 'b', 'B', '_', '',
    ' ', 'a1', 'a10', 'a2', 'A1', 'img12', 'img10', 'x 1', 'x  1', "\0a", 'a', true, false, null];
$arrays = [
    [],
    [42],
    [3, 1, 2],
    ['10', 9, '9a', 'abc', 1.5, true, null],
    ['b', 'B', 'a', 'A', '_', '1'],
    ['10', 9, '9a', 'abc', 1.5, true, null, '1e1', ' 9', 'ABC', 0, '', [], -1],
    ['x' => 3, 'y' => 1, 'z' => 2, 5 => 0, 'w' => 1],
    [5 => 'x', 3 => 'y', 10 => 'z'],
    ['a' => [1, 2], 'b' => [1], 'c' => [0, 5], 'd' => [], 'e' => 'str', 'f' => 1],
    [new S('b'), 'a', new S('c'), 'b', 1],
    [Suit::Hearts, 1, 'a', Suit::Hearts, null, 2],
    [NAN, 1, 0, NAN, -1],
    [1.5, '1.5', 1, '1', true],
    ['img12.png', 'img10.png', 'IMG2.png', 'img1.png', 'img 3.png', 'img003.png', 'img2.png', ''],
    ['10' => 'a', '9' => 'b', 'x' => 'c', '-1' => 'd', '01' => 'e', 'a' => 'f', '1e1' => 'g', ' 5' => 'h', '' => 'i'],
    ['b' => 1, 'B' => 2, 'a' => 3, 'A' => 4, 'a10' => 5, 'a9' => 6, 'a09' => 7, 7 => 8, -7 => 9],
    [PHP_INT_MAX => 1, -5 => 2, 0 => 3, 'k' => 4],
    [1.5, 1.2, 1.4, 0.3, 1.25, 2.75, -0.5],
    ['3', '20', '100', '1e2', 'abc', '2.5', '-1'],
    [[], [1], [0, 1], [1, 0], ['a' => 1], [2]],
];
// next free element after unset (packed without trailing holes)
$h = [7, 3, 5];
unset($h[2]);
$arrays[] = $h;
$h = ['x' => 2, 4 => 1, 9 => 0];
unset($h[9]);
$arrays[] = $h;
for ($i = 0; $i < 30; $i++) {
    $n = mt_rand(2, 40);
    $a = [];
    for ($j = 0; $j < $n; $j++) {
        $a[] = $pool[mt_rand(0, count($pool) - 1)];
    }
    if ($i % 3 === 1) {
        // random keys too
        $keyed = [];
        foreach ($a as $v) {
            $keyed[$pool[mt_rand(0, 46)] . ''] = $v;
        }
        $a = $keyed;
    }
    $arrays[] = $a;
}

$flagsList = [SORT_REGULAR, SORT_NUMERIC, SORT_STRING, SORT_STRING | SORT_FLAG_CASE, SORT_LOCALE_STRING,
    SORT_NATURAL, SORT_NATURAL | SORT_FLAG_CASE, 99];
$flags = [];
foreach ($arrays as $ai => $arr) {
    foreach (['sort', 'rsort', 'asort', 'arsort', 'ksort', 'krsort'] as $fn) {
        foreach ($flagsList as $flag) {
            $a = $arr;
            [$r, $d] = capture(static function () use (&$a, $fn, $flag) {
                $fn($a, $flag);
                return $a;
            });
            $flags[] = [$ai, $fn, $flag, resn($r), diags($d)];
        }
    }
    foreach (['natsort', 'natcasesort'] as $fn) {
        $a = $arr;
        [$r, $d] = capture(static function () use (&$a, $fn) {
            $fn($a);
            return $a;
        });
        $flags[] = [$ai, $fn, 0, resn($r), diags($d)];
    }
}

// --- user comparators ---------------------------------------------------------------------------
$calls = 0;
$comparators = comparators($calls);
$user = [];
foreach ($arrays as $ai => $arr) {
    if ($ai >= 22 && $ai % 4 !== 0) {
        continue;
    }
    foreach (['usort', 'uasort', 'uksort'] as $fn) {
        foreach ($comparators as $name => $cmp) {
            $a = $arr;
            $calls = 0;
            [$r, $d] = capture(static function () use (&$a, $fn, $cmp) {
                $fn($a, $cmp);
                return $a;
            });
            $user[] = [$ai, $fn, $name, resn($r), diags($d), $calls];
        }
    }
}

// --- natural order pairs ------------------------------------------------------------------------
$strs = ['', 'a', 'A', 'a1', 'a01', 'a001', 'a10', 'a2', 'a 2', 'a  2', "a\t2", 'a2 ', 'a2b', '1', '01', '001', '10',
    '0', '00', '0.5', '0.25', '.5', '1.5', '1.25', '12', '012', 'x1y2', 'x1y10', 'x01y2', 'img12', 'IMG12', 'img10',
    ' a', 'a ', '-1', '-2', '+1', '1e3', '1000', "\xff", "\xe9", 'abc', 'ABD', 'Abc', '123abc', '123ABC',
    'version 1.10', 'version 1.9', 'version 1.09', "a\0b", "a\0c", '9999999999999999999', '10000000000000000000'];
$natpairs = [];
foreach ($strs as $a) {
    foreach ($strs as $b) {
        $natpairs[] = [bin2hex($a), bin2hex($b), strnatcmp($a, $b), strnatcasecmp($a, $b)];
    }
}

emit('array/sort.gen.php', [
    'zsort' => $zsort,
    'stable' => $stable,
    'arrays' => array_map('encn', $arrays),
    'flags' => $flags,
    'user' => $user,
    'natpairs' => $natpairs,
    'messages' => messages(),
]);
