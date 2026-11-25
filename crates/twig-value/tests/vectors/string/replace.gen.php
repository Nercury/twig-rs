<?php
// Generates replace.json: strtr() (character form, array form with longest match, integer and empty keys,
// replacement conversion per match) and str_replace() (string/array search, replace and subject, counts);
// random subjects against random key sets.
//
//   cd crates/twig-value/tests/vectors
//   MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//       php /t/string/replace.gen.php > string/replace.json

declare(strict_types=0);
require __DIR__.'/calls.inc.php';

mt_srand(42);
$cases = [];
$subjects = ['', 'a', 'Hello %name%, %name%!', 'abcabc', 'a1b22c333', 'aaaa', "x\0y", '12345', 'the quick brown fox',
    'ababab', '-1-', 'ABab'];
$maps = [[], ['a' => 'x'], ['' => 'x'], ['ab' => ''], ['%name%' => 'Bob', 'Hello' => 'Hi', 'H' => 'X'],
    [1 => 'one', 22 => 'two', 333 => 'three'], ['a' => 'b', 'b' => 'a'], ['aa' => 'X', 'a' => 'Y', 'aaa' => 'Z'],
    ['' => 'x', 'a' => 'y', 'zzzzzzzzzzzzzzzzzzzzz' => 'long'], [-1 => 'm', 1 => 'p'], ['a' => 1.5, 'b' => true,
    'c' => null, 'x' => false], ['a' => [], 'b' => 'B'], ['a' => new S('S!')], ['a' => new stdClass(), 'b' => 'B'],
    ['a' => NAN, 'zz' => 'q'], ['a' => Suit::Hearts], ['' => 'x', '' . '' => 'y'], [0 => 'zero', '' => 'e'],
    ['fox' => 'dog', 'quick' => 'slow', 'the quick' => 'a slow', 'o' => '0'], ['12' => 'x', '123' => 'y', 1 => 'z'],
    [PHP_INT_MAX => 'max', PHP_INT_MIN => 'min'], ['b' => 'x', 'ab' => 'y', 'abab' => 'z'], ['A' => 'a']];
foreach ($subjects as $s) {
    foreach ($maps as $m) {
        $cases[] = ['strtr2', [$s, $m]];
    }
    foreach ([['a', 'b'], ['ab', 'xy'], ['abc', 'x'], ['', 'x'], ['a', ''], ['aa', 'xy'], ['ba', 'xy'], ['abc', 'cba'],
        ["\0", '0'], ['%', '#']] as [$from, $to]) {
        $cases[] = ['strtr3', [$s, $from, $to]];
    }
}
foreach (odd_values() as $v) {
    $cases[] = ['strtr2', [$v, ['1' => 'x']]];
    $cases[] = ['strtr2', ['abc', $v]];
    $cases[] = ['strtr3', ['abc', $v, 'x']];
    $cases[] = ['strtr3', ['abc', 'a', $v]];
    $cases[] = ['strtr3', [$v, '1', 'x']];
}
for ($k = 0; $k < 2000; $k++) {
    $map = [];
    $n = mt_rand(1, 6);
    for ($i = 0; $i < $n; $i++) {
        $key = rand_str('abc', 0, 4);
        if (mt_rand(0, 5) === 0) {
            $key = mt_rand(0, 30);
        }
        $map[$key] = rand_str('XYZ', 0, 3);
    }
    $cases[] = ['strtr2', [rand_str('abc0123', 0, 16), $map]];
}

$searches = ['a', 'ab', '', ['a', 'b'], ['ab', 'b', ''], ['a', 'aa'], [], ['x' => 'b', 'y' => 'a'], [1, 2.5, true],
    [[], 'a'], ['b', 'a']];
$replaces = ['X', '', ['1', '2'], ['1'], [], ['k' => 'K', 'l' => 'L', 'm' => 'M'], [[], 'Z'], [new S('s')]];
$rsubjects = ['', 'abcab', 'aaaa', 'banana', ['abc', 'k' => 'bab', 7 => 'x'], [], [1, 2.5, null, true], ['a' => []],
    'ab'];
foreach ($searches as $se) {
    foreach ($replaces as $re) {
        foreach ($rsubjects as $su) {
            $cases[] = ['str_replace', [$se, $re, $su]];
            $cases[] = ['str_replace_count', [$se, $re, $su]];
        }
    }
}
foreach (odd_values() as $v) {
    $cases[] = ['str_replace', [$v, 'x', 'a1b']];
    $cases[] = ['str_replace', ['1', $v, 'a1b']];
    $cases[] = ['str_replace', ['1', 'x', $v]];
    $cases[] = ['str_replace', [['1'], [$v], 'a1b']];
}
$cases[] = ['str_replace', [['a', 'b'], ['b', ''], 'ab']];
$cases[] = ['str_replace', [['ab', 'x'], ['', 'y'], 'ab']];
$cases[] = ['str_replace', [['a', []], 'Q', 'a']];

emit('replace.gen.php', ['cases' => run_cases($cases)]);
