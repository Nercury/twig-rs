<?php
// Generates split.json: explode() with limits and the empty-separator ValueError; implode() in its frameless
// (direct call) and regular (call_user_func) forms with every element type and argument shape.
//
//   cd crates/twig-value/tests/vectors
//   MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//       php /t/string/split.gen.php > string/split.json

declare(strict_types=0);
require __DIR__.'/calls.inc.php';

mt_srand(31337);
$cases = [];
$subjects = ['', 'a', 'a,b,c', ',a,,b,', ',,,', 'a, b, c', 'abab', 'aaa', 'no-delim', "x\0y\0z"];
$delims = ['', ',', ', ', 'ab', 'a', ',,', "\0", 'zz'];
$limits = [0, 1, 2, 3, 4, 100, -1, -2, -3, -10, PHP_INT_MAX, PHP_INT_MIN, '2', 1.5, null, 'x', true];
foreach ($subjects as $s) {
    foreach ($delims as $d) {
        $cases[] = ['explode2', [$d, $s]];
        foreach ($limits as $l) {
            $cases[] = ['explode3', [$d, $s, $l]];
        }
    }
}
foreach (odd_values() as $v) {
    $cases[] = ['explode2', [$v, 'a,b']];
    $cases[] = ['explode2', [',', $v]];
}
for ($k = 0; $k < 500; $k++) {
    $cases[] = ['explode3', [rand_str('ab,', 1, 2), rand_str('ab,', 0, 10), mt_rand(-4, 4)]];
}

$arrays = [[], [1], ['x'], [1, 2.5, true, false, null, 'x'], [-0.0, 1e20, 0.1 + 0.2, 1e15, -1.5e-7],
    ['a' => 'b', 5 => 'c'], [[1], 'x'], [new S('q'), 'x'], [Suit::Hearts], [new stdClass()], [NAN, 'x'],
    [PHP_INT_MIN, PHP_INT_MAX, 0], [new S('only')], [[]], [null], [true], [1.0]];
$glues = ['', ',', ', ', '--', "\0"];
foreach ($arrays as $arr) {
    foreach ($glues as $g) {
        $cases[] = ['implode2', [$g, $arr]];
        $cases[] = ['implode2_dyn', [$g, $arr]];
        $cases[] = ['implode2', [$arr, $g]];
        $cases[] = ['implode2_dyn', [$arr, $g]];
    }
    $cases[] = ['implode1', [$arr]];
    $cases[] = ['implode1_dyn', [$arr]];
}
foreach (odd_values() as $v) {
    $cases[] = ['implode2', [$v, [1, 2]]];
    $cases[] = ['implode2_dyn', [$v, [1, 2]]];
    $cases[] = ['implode2', [',', $v]];
    $cases[] = ['implode2_dyn', [',', $v]];
    $cases[] = ['implode1', [$v]];
    $cases[] = ['implode1_dyn', [$v]];
    $cases[] = ['implode2', [[1, 2], $v]];
    $cases[] = ['implode2_dyn', [[1, 2], $v]];
}

emit('split.gen.php', ['cases' => run_cases($cases)]);
