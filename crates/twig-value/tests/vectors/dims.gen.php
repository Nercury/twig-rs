<?php
// Generates dims.json: $container[$offset] (read), $container[$offset] ?? 'DFLT', isset() and empty() for
// arrays, strings, null, booleans, numbers and stdClass with offsets of every type, plus count().
//
//   cd crates/twig-value/tests/vectors
//   MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//       php /t/dims.gen.php > dims.json
//
// Containers are referenced by index into the CONTAINERS list below; tests/vectors.rs builds the same
// list. Offsets and results use the typed form of common.inc.php.

declare(strict_types=0);
require __DIR__.'/common.inc.php';

$containers = [
    ['k' => 'v', 5 => 'five', '' => 'empty', -1 => 'neg', 0 => 'zero', 1 => null, 'n' => null,
        PHP_INT_MAX => 'max', 'x' => 0, 'y' => '0', 'z' => NAN, 2 => 'two'],
    [],
    'abc',
    '',
    'a0',
    "0\0x",
    null,
    true,
    false,
    0,
    7,
    1.5,
    new stdClass(),
];

$offsets = [null, true, false, 0, 1, 2, 3, 4, 5, -1, -2, -3, -4, PHP_INT_MAX, PHP_INT_MIN, 0.0, -0.0, 1.0, 1.5,
    -1.5, 1.7, 2.0, 5.9, NAN, INF, -INF, 1e20, -1e20, 2.0 ** 63, '', '0', '1', '2', '5', '-1', '-3', '-4', ' 1',
    '1 ', "\n1", '1x', '1 x', 'x', 'k', 'n', 'K', '1.0', '1e0', '1e', '01', '-0', '+1', '00',
    '9223372036854775807', '9223372036854775808', "\0", "1\0", "k\0", '0x1', 'z', 'y', [], [1], new stdClass()];

$cases = [];
foreach ($containers as $ci => $c) {
    foreach ($offsets as $o) {
        $row = ['c' => $ci, 'o' => typed($o)];
        [$r, $d] = capture(static fn () => $c[$o]);
        $row['read'] = result($r);
        $row['read_diag'] = $d;
        [$r, $d] = capture(static fn () => $c[$o] ?? 'DFLT');
        $row['quiet'] = result($r);
        $row['quiet_diag'] = $d;
        [$r, $d] = capture(static fn () => isset($c[$o]));
        $row['isset'] = result($r);
        $row['isset_diag'] = $d;
        [$r, $d] = capture(static fn () => empty($c[$o]));
        $row['empty'] = result($r);
        $row['empty_diag'] = $d;
        $cases[] = $row;
    }
}

$counts = [];
foreach ([[], [1, 2], [1, [2, 3], [[4]], 'x' => [5, new stdClass()]], 'abc', null, 1, 1.5, true,
    new stdClass(), new ArrayObject([1, 2, 3])] as $v) {
    foreach ([0, 1, 2, -1] as $mode) {
        [$r, $d] = capture(static fn () => count($v, $mode));
        $counts[] = ['v' => typed($v), 'mode' => $mode, 'r' => result($r), 'diag' => $d];
    }
}
emit('dims.gen.php', ['cases' => $cases, 'counts' => $counts]);
