<?php
// Generates array/range.json: range() over a grid of start/end/step values plus edge cases.
//
//   cd crates/twig-value/tests/vectors
//   MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//       php /t/array/range.gen.php > array/range.json
//
// values:   enc() values used as start/end; steps: enc() values used as step
// grid:     [start index, end index, step index or -1 (omitted), result, diags]
// extra:    [[start, end, step?] as enc() values, result, diags] for hand-picked cases
// arity:    [[args as enc() values], result, diags] for range(...$args)
// result:   {"ok": [tokens], "next": key of $r[] = x} with tokens "i<int>", "f<hex bits>",
//           "s<hex bytes>" (results are lists), or {"err": [class, message index]}
//
// The oracle runs with memory_limit=-1: no case may build a large array.

declare(strict_types=0);
require __DIR__.'/../common.inc.php';
require __DIR__.'/../values.inc.php';

function tokens($r): array
{
    if ($r instanceof \Throwable) {
        return ['err' => [get_class($r), msg(bin2hex($r->getMessage()))]];
    }
    if (!array_is_list($r)) {
        throw new LogicException('range() result is not a list');
    }
    $out = [];
    foreach ($r as $v) {
        $out[] = match (true) {
            is_int($v) => 'i' . $v,
            is_float($v) => 'f' . bin2hex(pack('E', $v)),
            is_string($v) => 's' . bin2hex($v),
        };
    }
    $t = $r;
    $t[] = 0;
    return ['ok' => $out, 'next' => array_key_last($t)];
}

$values = [0, 1, 5, -3, 10, 1.5, 2.0, -2.5, 0.1, -0.0, '1', '5', '10', '1.5', 'a', 'z', 'A', 'Z', 'aa', 'zz',
    '', ' ', '!', '~', "\xff", "\x00", '0', '9', ' 5', '5 ', 'e', '1e1', null, true, false, PHP_INT_MAX,
    PHP_INT_MIN, INF, NAN, [], new S('c'), new S('10'), new stdClass()];
$steps = [1, 2, 3, -1, 0, 0.5, 1.5, -2.5, 0.25, 2.0, -0.0, 100, '2', '0.5', 'x', null, true, INF, NAN,
    PHP_INT_MIN, -PHP_INT_MAX, []];

$grid = [];
foreach ($values as $si => $start) {
    foreach ($values as $ei => $end) {
        [$r, $d] = capture(static fn () => range($start, $end));
        $grid[] = [$si, $ei, -1, tokens($r), diags($d)];
        foreach ($steps as $ti => $step) {
            [$r, $d] = capture(static fn () => range($start, $end, $step));
            $grid[] = [$si, $ei, $ti, tokens($r), diags($d)];
        }
    }
}

$extraCases = [
    [0, 1e10],
    [1e10, 0],
    [-1e308, 1e308],
    [1e308, -1e308],
    [0, 1073741823],
    [1073741823, 0],
    [0, 1073741824],
    [-5, 1073741822, 1.5e300],
    [PHP_INT_MAX, 0],
    [0, PHP_INT_MAX],
    [PHP_INT_MIN, PHP_INT_MAX],
    [PHP_INT_MIN, PHP_INT_MAX, PHP_INT_MAX],
    [PHP_INT_MAX, PHP_INT_MIN, PHP_INT_MAX],
    [PHP_INT_MIN, PHP_INT_MAX, 2 ** 62],
    [PHP_INT_MAX - 2, PHP_INT_MAX],
    [PHP_INT_MIN, PHP_INT_MIN + 3],
    [PHP_INT_MAX, PHP_INT_MAX - 5, 2],
    [-0.0, 1e10],
    [0.5, 1e10, 1],
    [1e10, -0.01],
    [0, 1, 1e-300],
    [0, 1, 0.1],
    [1, 0, 0.1],
    [0, 1, 0.3],
    [0.1, 0.3, 0.1],
    [1, 2, 0.3],
    [1, 5, 2.0],
    [1, 5, '2.0'],
    [9007199254740993, 9007199254740995],
    [9007199254740993, 9007199254740995, 1.0],
    [9007199254740992.0, 9007199254740996.0],
    ['A', 'z', 10],
    ['a', 'e', -1],
    ['e', 'a', -1],
    ['e', 'a', 5],
    ['a', 'e', 5],
    ['a', 'e', 4],
    [' ', '~', 7],
    ["\x00", "\xff", 15],
    ["\xff", "\x00", 51],
    ['a', 'c', 1.5],
    ['1', '3', 1.5],
    ['a', '3', 1.5],
    ['0', 'a'],
    ['a', '0'],
    ['9', 'A', 3],
    ['1', '5'],
    ['1', '10'],
    ['5', 1],
    [1, '5'],
    ['b', 3],
    [3, 'b'],
    ['b', 3.5],
    ['ab', 'cd', 2],
    ['', ''],
    ['', 'c'],
    ['1e3', '1e3'],
    ['1.0', '3'],
    ['1e0', '3e0'],
    [' 1', '3'],
    ['0x10', '0x20'],
    [null, null],
    [true, false],
    [false, 3],
    [0, 0, -1],
    [5, 5, PHP_INT_MIN],
    [5, 5, 0],
    [1.5, 1.5, 7],
];
$extra = [];
foreach ($extraCases as $args) {
    [$r, $d] = capture(static fn () => range(...$args));
    $extra[] = [array_map('enc', $args), tokens($r), diags($d)];
}

$arity = [];
foreach ([[], [1], [1, 2, 3, 4], [1, 2, 1, 1, 1]] as $args) {
    [$r, $d] = capture(static fn () => range(...$args));
    $arity[] = [array_map('enc', $args), tokens($r), diags($d)];
}

emit('array/range.gen.php', [
    'values' => array_map('enc', $values),
    'steps' => array_map('enc', $steps),
    'grid' => $grid,
    'extra' => $extra,
    'arity' => $arity,
    'messages' => messages(),
]);
