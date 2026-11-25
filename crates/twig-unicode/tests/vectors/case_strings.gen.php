<?php
/*
 * Oracle generator for case_strings.json (twig-rs, crates/twig-unicode, PHP 8.5.13 mbstring).
 *
 * Case conversion of whole strings, where context matters: title-case word boundaries, the
 * final-sigma rule and its 64-code-point chunking (look-behind into the previous chunk's
 * converted output, look-ahead into the next chunk), special-casing expansions that fill the
 * conversion buffer, ill-formed UTF-8 between letters, and pseudo-random strings
 * (mt_srand(20261125)) over a token alphabet of such cases.
 *
 * Every case is {"name": ..., "in": hex, "modes": [hex of mb_convert_case($in, $mode) for
 * $mode = 0..7 (MB_CASE_UPPER .. MB_CASE_FOLD_SIMPLE)], "ucfirst": hex, "lcfirst": hex}.
 * mb_strtoupper()/mb_strtolower() are checked to agree with modes 0 and 1.
 *
 * Regenerate from the repository root with the oracle image (research/php-oracle):
 *   docker run --rm -i twig-oracle:3.31.1 php < crates/twig-unicode/tests/vectors/case_strings.gen.php \
 *       > crates/twig-unicode/tests/vectors/case_strings.json
 */

ini_set('memory_limit', '-1');
error_reporting(E_ALL);
set_error_handler(function ($no, $str) { fwrite(STDERR, "PHP error: $str\n"); exit(1); });

$cases = [];
function add(string $name, string $s): void
{
    global $cases;
    $cases[] = [$name, $s];
}

// Documented behaviour (reference/php/stdlib.md section 6.5).
add('doc title 1', "hello wORLD o'neil mc-donald ǆungla straße");
add('doc title 2', "a.b,c_d1e 2f\tg's x’y «z» ŉx");
add('doc upper', 'straße ǆ ﬁ ŉ');
add('doc lower', 'İSTANBUL ΣΑΣ ǅ');
add('empty', '');

// Final sigma, inside one chunk.
foreach ([
    'Σ', 'AΣ', 'ΑΣ', 'ΑΣΑ', 'ΑΣ Α', 'ΑΣ.', 'Α.Σ', "Α'Σ'", "ΑΣ'Α", "ΑΣ'", "Σ'Α", "'Σ", "Σ'",
    "ΑΣ\u{345}", "ΑΣ\u{345}Α", "Α\u{345}Σ", "1\u{345}Σ", 'ΑΣΣ', 'ΑΣΣΣ', 'ΣΣΣ', 'ΑΣ1', '1Σ', 'Α1Σ',
    "ΑΣ\xFF", "\xFFΣ", "Α\xFFΣ", "ΑΣ\xFFΑ", "ΣΑΣ ΣΑΣ", 'ΟΔΥΣΣΕΥΣ', 'ὈΔΥΣΣΕΎΣ', "Α\u{300}Σ",
    "ΑΣ\u{300}", "ΑΣ\u{300}Β", "ᾼΣ", "ǅΣ", "ΑΣ-Β", "ΑΣ\u{2019}Β", "Α\u{2019}Σ", "ΑΣ\u{200D}",
] as $s) {
    add('sigma', $s);
}

// Title case: word boundaries, digits, marks, title-case digraphs, expansions.
foreach ([
    'hello world', 'HELLO WORLD', "o'neil o’neil", 'mc-donald', 'a1b 2c', 'ǆungla ǅungla Ǆungla',
    'ßa ﬁx ﬄy ŉx', 'ΐa ΰb', "a\u{300}b c\u{301}D", 'ᾳ ᾼ ᾀ ᾈ', 'İi ıI', 'ǈ ǉ Ǉ', "\tx\ny\rz",
    "'abc", '.abc', 'a.b', 'a:b', "a\u{B7}b", 'a_b', 'a/b', "a\u{AD}b", "\u{10428}\u{10400}x",
    "\xFFab", "a\xFFb", "ab\xFF", "\xE2\x82ab", "a\xC0\xAFb", "a\xED\xA0\x80b", "a\xF4\x90\x80\x80b",
] as $s) {
    add('title', $s);
}

// Chunk boundaries: the input is decoded 64 code points at a time.
$ks = [1, 2, 3, 62, 63, 64, 65, 66, 127, 128, 129, 130, 191, 192, 193];
foreach ($ks as $k) {
    add("behind k=$k", 'a' . str_repeat("'", $k - 1) . 'Σ');
    add("behind expanded k=$k", 'İ' . str_repeat("'", $k - 1) . 'Σ');
    add("behind astral k=$k", "\u{10400}" . str_repeat("'", $k - 1) . 'Σ');
    add("ahead astral k=$k", str_repeat("\u{10428}", $k) . "Σ\u{345}");
    add("behind cased+ignorable k=$k", str_repeat('1', $k - 1) . "\u{345}Σ");
    add("behind bad k=$k", 'a' . str_repeat("'", max(0, $k - 2)) . "\xFFΣ");
    add("ahead cased+ignorable k=$k", 'a' . str_repeat("'", $k - 1) . "Σ\u{345}");
    add("ahead cased k=$k", 'a' . str_repeat("'", $k - 1) . "Σ'''b");
    add("ahead bad k=$k", 'a' . str_repeat('b', $k - 1) . "Σ\xFFb");
    add("sigmas k=$k", str_repeat('Σ', $k));
    add("sharp s k=$k", str_repeat('ß', $k));
    add("iota dialytika tonos k=$k", str_repeat('ΐ', $k));
    add("dotted I k=$k", str_repeat('İ', $k));
    add("title across k=$k", str_repeat("'", $k - 1) . 'ab cd');
    add("title bad k=$k", str_repeat('x', $k - 1) . "\xFFyz");
}

// Pseudo-random strings.
$tokens = [
    'a', 'A', 'z', 'Σ', 'σ', 'ς', 'ß', 'ẞ', 'İ', 'ı', 'i', 'I', 'ǅ', 'ǆ', 'Ǆ', 'ŉ', 'ﬁ', 'ΐ', 'ᾳ',
    'ᾼ', "\u{345}", "'", '’', '.', ':', '-', ' ', '1', "\u{300}", 'Ω', 'ω', 'Ö', 'ö', 'K', 'Å',
    'µ', 'և', 'ᾀ', "\u{10400}", "\u{10428}", "\u{13A0}", "\u{AB70}", "\u{1E9E}", 'ǰ', 'ẖ',
    "\xFF", "\xE2\x82", "\xC0", "\xED\xA0\x80", "\xF0\x9F\x98",
];
mt_srand(20261125);
for ($n = 0; $n < 80; $n++) {
    $len = mt_rand(0, 150);
    $s = '';
    for ($i = 0; $i < $len; $i++) {
        $s .= $tokens[mt_rand(0, count($tokens) - 1)];
    }
    add('random', $s);
}

$flags = JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE;
$lines = [];
foreach ($cases as [$name, $s]) {
    $modes = [];
    for ($mode = 0; $mode <= 7; $mode++) {
        $modes[] = bin2hex(mb_convert_case($s, $mode, 'UTF-8'));
    }
    if (bin2hex(mb_strtoupper($s, 'UTF-8')) !== $modes[0] || bin2hex(mb_strtolower($s, 'UTF-8')) !== $modes[1]) {
        fwrite(STDERR, "mb_strtoupper/mb_strtolower disagree with mb_convert_case for $name\n");
        exit(1);
    }
    $lines[] = '    ' . json_encode([
        'name' => $name,
        'in' => bin2hex($s),
        'modes' => $modes,
        'ucfirst' => bin2hex(mb_ucfirst($s, 'UTF-8')),
        'lcfirst' => bin2hex(mb_lcfirst($s, 'UTF-8')),
    ], $flags);
}
echo "{\n  \"generator\": \"case_strings.gen.php\",\n  \"php_version\": " . json_encode(PHP_VERSION)
    . ",\n  \"cases\": [\n" . implode(",\n", $lines) . "\n  ]\n}\n";
