<?php
// Generates basic.json: strlen, substr, strpos, strrpos, substr_count, str_contains/starts_with/ends_with,
// ASCII case mapping, strcmp/strcasecmp/strncasecmp, ord, chr, str_repeat, levenshtein, ctype_*.
//
//   cd crates/twig-value/tests/vectors
//   MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//       php /t/string/basic.gen.php > string/basic.json

declare(strict_types=0);
require __DIR__.'/calls.inc.php';

mt_srand(20261125);
$cases = [];
$strings = ['', 'a', 'abc', 'Hello World', "a\0b", "\xC3\xA9t\xC3\xA9", 'ÀBC İ Ä', 'aaaa', 'abab', 'xYz', "\xFF\x80"];
$ints = [0, 1, 2, -1, -2, -3, 3, 4, 10, -10, PHP_INT_MAX, PHP_INT_MIN];

foreach (array_merge($strings, odd_values()) as $s) {
    $cases[] = ['strlen', [$s]];
    $cases[] = ['strtolower', [$s]];
    $cases[] = ['strtoupper', [$s]];
    $cases[] = ['ucfirst', [$s]];
    $cases[] = ['lcfirst', [$s]];
    $cases[] = ['ucwords1', [$s]];
    $cases[] = ['ord', [$s]];
}
foreach ($strings as $s) {
    foreach ($ints as $o) {
        $cases[] = ['substr2', [$s, $o]];
        foreach ([0, 1, 2, -1, -2, 5, PHP_INT_MAX, PHP_INT_MIN, null] as $l) {
            $cases[] = ['substr3', [$s, $o, $l]];
        }
    }
}
foreach (odd_values() as $v) {
    $cases[] = ['substr2', ['abcdef', $v]];
    $cases[] = ['substr3', ['abcdef', 1, $v]];
    $cases[] = ['substr2', [$v, 1]];
}
$needles = ['', 'a', 'b', 'ab', 'abc', 'x', "\0", 'bab'];
foreach (['', 'abcabc', 'ababab', "a\0b\0", 'xxx'] as $h) {
    foreach ($needles as $n) {
        $cases[] = ['strpos2', [$h, $n]];
        $cases[] = ['strrpos2', [$h, $n]];
        $cases[] = ['str_contains', [$h, $n]];
        $cases[] = ['str_starts_with', [$h, $n]];
        $cases[] = ['str_ends_with', [$h, $n]];
        $cases[] = ['substr_count2', [$h, $n]];
        foreach ([0, 1, 2, -1, -2, -6, 6, 7, -7] as $o) {
            $cases[] = ['strpos3', [$h, $n, $o]];
            $cases[] = ['strrpos3', [$h, $n, $o]];
            $cases[] = ['substr_count3', [$h, $n, $o]];
            foreach ([0, 1, 3, -1, -3, 9, null] as $l) {
                $cases[] = ['substr_count4', [$h, $n, $o, $l]];
            }
        }
    }
}
foreach (odd_values() as $v) {
    $cases[] = ['str_contains', [$v, 'a']];
    $cases[] = ['str_starts_with', ['abc', $v]];
    $cases[] = ['strpos3', ['abc', 'b', $v]];
}
foreach (['', 'a', 'A', 'abc', 'ABD', 'ab', "\xE9", 'Z', '[', 'abc'] as $a) {
    foreach (['', 'a', 'B', 'abc', 'abC', 'ab', "\xC9", 'z', '_'] as $b) {
        $cases[] = ['strcmp', [$a, $b]];
        $cases[] = ['strcasecmp', [$a, $b]];
        foreach ([0, 1, 2, 5, -1] as $l) {
            $cases[] = ['strncasecmp', [$a, $b, $l]];
        }
    }
}
foreach (['hello world', 'hello-world foo_bar', "a\tb\nc\x0Bd\x0Ce\rf", '  x', 'x  ', ''] as $s) {
    foreach ([' ', '-', '', 'a..z', '..', 'z..a', "\t-", ' _-'] as $d) {
        $cases[] = ['ucwords2', [$s, $d]];
    }
}
foreach (array_merge($ints, [255, 256, 300, -129, 65, '65', 65.5, '1e2'], odd_values()) as $c) {
    $cases[] = ['chr', [$c]];
}
foreach (['', 'ab', 'x'] as $s) {
    foreach ([0, 1, 3, -1, '2', 2.5, null] as $n) {
        $cases[] = ['str_repeat', [$s, $n]];
    }
}
// levenshtein
$lev = ['', 'a', 'ab', 'abc', 'kitten', 'sitting', 'flaw', 'lawn', 'saturday', 'sunday', "\xC3\xA9", 'trim', 'tirm'];
foreach ($lev as $a) {
    foreach ($lev as $b) {
        $cases[] = ['levenshtein2', [$a, $b]];
    }
}
foreach ([[1, 1, 1], [2, 1, 1], [1, 2, 1], [1, 1, 2], [0, 0, 0], [5, 3, 7], [-1, 2, -3], [PHP_INT_MAX, 1, 1], [1, PHP_INT_MAX, PHP_INT_MAX]] as [$i, $r, $d]) {
    foreach ([['kitten', 'sitting'], ['', 'abc'], ['abc', ''], ['ab', 'ba'], ['sunday', 'saturday']] as [$a, $b]) {
        $cases[] = ['levenshtein5', [$a, $b, $i, $r, $d]];
    }
}
for ($k = 0; $k < 300; $k++) {
    $cases[] = ['levenshtein5', [rand_str('abc', 0, 12), rand_str('abcd', 0, 12), mt_rand(0, 3), mt_rand(0, 3), mt_rand(0, 3)]];
}
foreach (odd_values() as $v) {
    $cases[] = ['levenshtein2', [$v, 'abc']];
    $cases[] = ['levenshtein5', ['a', 'b', $v, 1, 1]];
}
// ctype: every byte, a few strings, every type
$ctype = ['ctype_alnum', 'ctype_alpha', 'ctype_cntrl', 'ctype_digit', 'ctype_lower', 'ctype_graph', 'ctype_print',
    'ctype_punct', 'ctype_space', 'ctype_upper', 'ctype_xdigit'];
foreach ($ctype as $f) {
    for ($b = 0; $b < 256; $b++) {
        $cases[] = [$f, [chr($b)]];
    }
    foreach (['', 'abc', 'ABC', '123', 'a1', ' ', "\t\n", 'AbC', 'abc!', '0x1F', "1\n"] as $s) {
        $cases[] = [$f, [$s]];
    }
    foreach (array_merge(odd_values(), [-129, -128, -1, 48, 65, 97, 255, 256, 1000, -1000, 32, 9]) as $v) {
        $cases[] = [$f, [$v]];
    }
}

emit('basic.gen.php', ['cases' => run_cases($cases)]);
