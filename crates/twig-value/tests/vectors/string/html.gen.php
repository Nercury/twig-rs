<?php
// Generates html.json: htmlspecialchars() with every flag combination, every charset name of html.c (and
// unknown ones), double_encode on and off, systematic invalid-sequence tables for UTF-8 and the multi-byte
// charsets, and random byte strings.
//
//   cd crates/twig-value/tests/vectors
//   MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//       php /t/string/html.gen.php > string/html.json

declare(strict_types=0);
require __DIR__.'/calls.inc.php';

mt_srand(1378);
$cases = [];
$charsets = ['ISO-8859-1', 'ISO8859-1', 'ISO-8859-15', 'ISO8859-15', 'utf-8', 'cp1252', 'Windows-1252', '1252', 'BIG5',
    '950', 'GB2312', '936', 'BIG5-HKSCS', 'Shift_JIS', 'SJIS', '932', 'SJIS-win', 'CP932', 'EUCJP', 'EUC-JP',
    'eucJP-win', 'KOI8-R', 'koi8-ru', 'koi8r', 'cp1251', 'Windows-1251', 'win-1251', 'iso8859-5', 'iso-8859-5',
    'cp866', '866', 'ibm866', 'MacRoman'];
$names = array_merge($charsets, ['UTF-8', 'Utf-8', 'macroman', 'big5-hkscs', 'shift_jis', 'bogus', 'UTF8', 'latin1',
    '', null, "utf-8\0junk", "\0", 'ISO-8859-1 ', 'KOI8-U', 'ASCII']);
$flagsets = [];
foreach ([ENT_COMPAT, ENT_QUOTES, ENT_NOQUOTES] as $q) {
    foreach ([0, ENT_IGNORE, ENT_SUBSTITUTE, ENT_IGNORE | ENT_SUBSTITUTE] as $e) {
        foreach ([ENT_HTML401, ENT_XML1, ENT_XHTML, ENT_HTML5] as $dt) {
            foreach ([0, ENT_DISALLOWED] as $dis) {
                $flagsets[] = $q | $e | $dt | $dis;
            }
        }
    }
}
$basic = "<a href=\"x\">it's & \"q\" </a>\x00\x01\x0B\x0C\x0D\x7F";
foreach ($names as $cs) {
    $cases[] = ['htmlspecialchars3', [$basic, ENT_QUOTES | ENT_SUBSTITUTE, $cs]];
    $cases[] = ['htmlspecialchars3', ['', ENT_QUOTES, $cs]];
}
$all = '';
for ($b = 0; $b < 256; $b++) {
    $all .= chr($b);
}
foreach ($charsets as $cs) {
    foreach ([ENT_QUOTES | ENT_SUBSTITUTE, ENT_QUOTES | ENT_SUBSTITUTE | ENT_DISALLOWED | ENT_HTML5,
        ENT_COMPAT | ENT_IGNORE | ENT_DISALLOWED | ENT_XML1, ENT_NOQUOTES | ENT_DISALLOWED, ENT_QUOTES] as $f) {
        $cases[] = ['htmlspecialchars3', [$all, $f, $cs]];
    }
}
$fixed = ['', 'plain', '<>&\'"', "a\xFFb\xC3", "\xE2\x82\xAC\xF0\x9F\x98\x80", "\xED\xA0\x80", "\xF4\x90\x80\x80",
    "\xC0\x80", "\xE0\x80\x80", "\xF0\x80\x80\x80", "\xEF\xBF\xBE\xEF\xBF\xBF\xEF\xB7\x90", "\xC2\x80\xC2\x9F\xC2\xA0",
    "\x7F\x00\x0B\x0C\x0D", "\xF3\xBF\xBF\xBF\xF4\x8F\xBF\xBF", '&amp; &lt; &#039; &apos;'];
foreach ($fixed as $s) {
    $cases[] = ['htmlspecialchars1', [$s]];
    foreach ($flagsets as $f) {
        $cases[] = ['htmlspecialchars2', [$s, $f]];
    }
}
// double_encode = false
$ents = '&amp; &#65; &#x41; &#X41; &#x0x41; &#0x41; &#x0X41; &#x0x; &copy; &apos; &NotNestedGreaterGreater; &bogus; '
    .'&#1114111; &#1114112; &#xD800; &#0; &#13; &#12; &#x; &#; &; &AMP; &a1b; &#99999999999999999999; &#x10FFFF; '
    .'&#x110000; &#xFFFE; &#xFDD0; &#127; &#128; &#159; &#9; &#10; &#xfffff; &Aacute; &aacute &amp &#65 &lt;;'
    .' &#-1; &#+1; &# 1; &#x 1; &zwnj; &ZeroWidthSpace; &apos &quot; &QUOT; &nbsp; &#x00; &#0065; &';
foreach ($flagsets as $f) {
    $cases[] = ['htmlspecialchars4', [$ents, $f, 'UTF-8', false]];
}
foreach (['ISO-8859-1', 'SJIS', 'BIG5', 'KOI8-R', 'EUC-JP'] as $cs) {
    $cases[] = ['htmlspecialchars4', [$ents, ENT_QUOTES | ENT_DISALLOWED, $cs, false]];
}
// systematic UTF-8 sequences: every lead byte with interesting continuation bytes, separated by '|'
$s2 = [0x41, 0x7F, 0x80, 0x8F, 0x9F, 0xA0, 0xBF, 0xC0, 0xC2, 0xE0, 0xF0, 0xF4, 0xFF];
$s4 = [0x41, 0x80, 0x8F, 0x90, 0xBF, 0xC2, 0xF5];
for ($c = 0x80; $c <= 0xFF; $c++) {
    $a = $b = '';
    foreach ($s2 as $x) {
        $a .= chr($c).chr($x).'|';
        foreach ($s2 as $y) {
            $a .= chr($c).chr($x).chr($y).'|';
        }
    }
    foreach ($s4 as $x) {
        foreach ($s4 as $y) {
            foreach ($s4 as $z) {
                $b .= chr($c).chr($x).chr($y).chr($z).'|';
            }
        }
    }
    $cases[] = ['htmlspecialchars3', [$a.chr($c), ENT_QUOTES | ENT_SUBSTITUTE, 'UTF-8']];
    $cases[] = ['htmlspecialchars3', [$b.chr($c).chr(0x80).chr(0x80), ENT_QUOTES | ENT_SUBSTITUTE, 'UTF-8']];
}
// multi-byte charsets: every lead byte with interesting second (and third) bytes
$t2 = [0x00, 0x26, 0x3C, 0x40, 0x41, 0x7E, 0x7F, 0x80, 0x8E, 0x8F, 0xA0, 0xA1, 0xDF, 0xE0, 0xFC, 0xFD, 0xFE, 0xFF];
foreach (['BIG5', 'BIG5-HKSCS', 'GB2312', 'SJIS', 'EUC-JP'] as $cs) {
    for ($c = 0x80; $c <= 0xFF; $c++) {
        $a = '';
        foreach ($t2 as $x) {
            $a .= chr($c).chr($x).'|';
            if ($cs === 'EUC-JP') {
                foreach ([0x41, 0xA0, 0xA1, 0xFF] as $y) {
                    $a .= chr($c).chr($x).chr($y).'|';
                }
            }
        }
        $cases[] = ['htmlspecialchars3', [$a.chr($c), ENT_QUOTES | ENT_SUBSTITUTE, $cs]];
    }
}
// random strings
$alpha = "<>&'\"#;ax|\x00\x7F";
for ($b = 0x80; $b < 0x100; $b++) {
    $alpha .= chr($b);
}
for ($k = 0; $k < 2500; $k++) {
    $cs = $charsets[mt_rand(0, count($charsets) - 1)];
    $f = $flagsets[mt_rand(0, count($flagsets) - 1)];
    $cases[] = ['htmlspecialchars4', [rand_str($alpha, 0, 16), $f, $cs, (bool) mt_rand(0, 1)]];
}
// argument coercion
foreach (odd_values() as $v) {
    $cases[] = ['htmlspecialchars1', [$v]];
    $cases[] = ['htmlspecialchars2', ['<a>', $v]];
    $cases[] = ['htmlspecialchars3', ['<a>', ENT_QUOTES, $v]];
    $cases[] = ['htmlspecialchars4', ['&amp;<', ENT_QUOTES, 'UTF-8', $v]];
}
$cases[] = ['htmlspecialchars2', ['<a\'">', ENT_QUOTES + (1 << 32)]];
$cases[] = ['htmlspecialchars2', ['<a\'">', -1]];
$cases[] = ['htmlspecialchars2', ["a\xFF<", PHP_INT_MAX]];
$cases[] = ['htmlspecialchars2', ["a\xFF<", '11']];

emit('html.gen.php', ['cases' => run_cases($cases)]);
