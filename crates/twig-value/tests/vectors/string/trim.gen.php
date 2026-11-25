<?php
// Generates trim.json: trim/ltrim/rtrim with and without character lists, php_charmask() ranges and their
// warnings, argument coercion; random strings against random masks.
//
//   cd crates/twig-value/tests/vectors
//   MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//       php /t/string/trim.gen.php > string/trim.json

declare(strict_types=0);
require __DIR__.'/calls.inc.php';

mt_srand(1209);
$strings = ['', ' ', "  a b  ", "\t\n\r\0\x0B\x0Cab\x0C\x0B\0\r\n\t", 'abcxcba', '..a..', "\xE9a\xE9", 'aaa', 'xyz',
    "\x00\x01 mid \x7F\xFF", 'a..c', '-a-', 'cba.abc', "0123456789", " \t\n\r\v\f"];
$masks = ['', ' ', 'a', '.', '..', '...', '....', 'a..c', '..c', 'a..', 'z..a', 'a..b..c', 'a..a', "\x00..\x20",
    "\xE9", " \t\n\r\0\x0B", " \t\n\r\v\f", 'a-c', '0..9', 'c..a', "a..\xFF", 'a..b..', '..a..', 'ab..', '..ab',
    'a...c', 'a....c', '.-.', "\x7F..\x80", 'x..z..', '-..-', 'b..a..c', "\xFF..\x00"];
$cases = [];
foreach (['trim', 'ltrim', 'rtrim'] as $f) {
    foreach ($strings as $s) {
        $cases[] = [$f.'1', [$s]];
        foreach ($masks as $m) {
            $cases[] = [$f.'2', [$s, $m]];
        }
    }
    foreach (odd_values() as $v) {
        $cases[] = [$f.'1', [$v]];
        $cases[] = [$f.'2', [$v, 'a']];
        $cases[] = [$f.'2', ['a1.5a', $v]];
    }
}
$alphabet = "abc.. \t\x00z-\xFF";
for ($k = 0; $k < 1500; $k++) {
    $f = ['trim2', 'ltrim2', 'rtrim2'][mt_rand(0, 2)];
    $cases[] = [$f, [rand_str($alphabet, 0, 12), rand_str($alphabet, 0, 8)]];
}

emit('trim.gen.php', ['cases' => run_cases($cases)]);
