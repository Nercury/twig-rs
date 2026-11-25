<?php
// Generates iconv.json: iconv() on glibc 2.41 for the charsets twig-value supports (see the iconv module docs):
// charset-name parsing (suffixes, case, separators, aliases, length limit, unknown names), every charset in both
// directions on fixed and random input, invalid and incomplete input, unmappable characters, Unicode tag
// characters, byte order marks, //IGNORE (with PHP's ICONV_BROKEN_IGNORE handling across glibc's 8160-character
// rounds and output-buffer growth) and //TRANSLIT (the C.UTF-8 locale table, alone and with //IGNORE).
//
//   cd crates/twig-value/tests/vectors
//   MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//       php /t/string/iconv.gen.php > string/iconv.json

declare(strict_types=0);
require __DIR__.'/calls.inc.php';

mt_srand(2260);
$cases = [];
$supported = ['UTF-8', 'ASCII', 'ISO-8859-1', 'ISO-8859-2', 'ISO-8859-3', 'ISO-8859-4', 'ISO-8859-5', 'ISO-8859-6',
    'ISO-8859-7', 'ISO-8859-8', 'ISO-8859-9', 'ISO-8859-10', 'ISO-8859-11', 'ISO-8859-13', 'ISO-8859-14',
    'ISO-8859-15', 'ISO-8859-16', 'CP1250', 'CP1251', 'CP1252', 'CP1253', 'CP1254', 'CP1256', 'CP1257', 'KOI8-R',
    'KOI8-U', 'CP866', 'MACINTOSH', 'CP437', 'CP850', 'UCS-2', 'UCS-2LE', 'UCS-2BE', 'UCS-4', 'UCS-4LE', 'UNICODE',
    'UTF-16', 'UTF-16LE', 'UTF-16BE', 'UTF-32', 'UTF-32LE', 'UTF-32BE'];
$unicode = ['UTF-8', 'UCS-2', 'UCS-2BE', 'UCS-4', 'UCS-4LE', 'UNICODE', 'UTF-16', 'UTF-16LE', 'UTF-16BE', 'UTF-32',
    'UTF-32LE', 'UTF-32BE'];

// 1. names
$names = ['UTF-8', 'utf8', 'Utf-8', 'UTF_8', 'utf 8', 'UTF-8/', 'UTF-8//', 'UTF-8///', 'UTF-8////', 'UTF-8//TRANSLIT',
    'UTF-8//IGNORE', 'UTF-8//translit', 'utf-8//ignore', 'UTF-8//IGNORE//TRANSLIT', 'UTF-8//TRANSLIT//IGNORE',
    'UTF-8//TRANSLIT,IGNORE', 'UTF-8//IGNORE,', 'UTF-8//foo', 'UTF-8 //IGNORE', 'UTF-8/IGNORE', 'UTF-8,IGNORE',
    'ISO-10646/UTF8', 'ISO-10646/UTF8/', 'ISO-10646/UTF-8/', 'ISO-10646/UTF8/IGNORE', 'UTF-8@x', '.UTF-8', '-UTF-8',
    'UTF--8', 'U.T.F-8', '', '/', '//', '///', ' ', "UTF-8\0junk", "\0UTF-8", 'UTF-8 ', ' UTF-8', "UTF-8\t", 'LATIN1',
    'latin1', 'L1', 'ISO_8859-1:1987', 'ISO8859-1', 'ISO88591', 'CP819', 'IBM819', 'WINDOWS-1252', 'MS-ANSI',
    'CP-1252', 'US-ASCII', 'ANSI_X3.4-1968', 'ISO646-US', 'CSASCII', 'UCS-2LE', 'UNICODELITTLE', 'UNICODEBIG',
    'UCS-4BE', 'UTF16', 'UTF32BE', 'CSUNICODE', 'MAC', 'MACINTOSH', 'CSMACINTOSH', 'IBM866', 'CP866', '866', 'KOI8R',
    'CYRILLIC', 'ISO-IR-144', 'GREEK', 'HEBREW', 'ARABIC', 'LATIN-9', 'LATIN10', 'ISO-8859-12', 'bogus', 'INTERNAL',
    'WCHAR_T//IGNORE', str_repeat('A', 63), str_repeat('A', 64), 'UTF-8'.str_repeat('/', 58), 'utf-8:',
    'UTF-8//IGNORE//TRANSLIT//IGNORE', 'UTF-8//IGNORE/', 'utf-8//IGNORE', 'UTF8//IGNORE', 'X//IGNORE'];
foreach ($names as $n) {
    if (in_array($n, ['WCHAR_T//IGNORE'], true)) {
        continue;
    }
    $cases[] = ['iconv', ['ISO-8859-1', $n, "a\xE9"]];
    $cases[] = ['iconv', [$n, 'UTF-8', "a\xE9"]];
}
$cases[] = ['iconv', ['WCHAR_T', 'WCHAR_T', 'abcd']];
$cases[] = ['iconv', ['UTF-8', 'bogus', '']];
$cases[] = ['iconv', ['bogus', 'bogus2', 'x']];
foreach (odd_values() as $v) {
    if (is_object($v) && !($v instanceof S)) {
        $cases[] = ['iconv', [$v, 'UTF-8', 'x']];
        continue;
    }
    $cases[] = ['iconv', [$v, 'UTF-8', 'x']];
    $cases[] = ['iconv', ['UTF-8', $v, 'x']];
    $cases[] = ['iconv', ['UTF-8', 'ISO-8859-1', $v]];
}

// 2. fixed texts in every direction
$texts = ['', 'plain ASCII', "Latin-1: \u{E9}\u{E0}\u{FC}\u{DF}\u{FF}\u{A0}", "Euro \u{20AC} quotes \u{201C}x\u{201D}",
    "Cyrillic \u{416}\u{437}\u{44F}", "Greek \u{3A9}\u{3B1}", "Hebrew \u{5D0}", "Arabic \u{627}", "CJK \u{4E2D}\u{6587}",
    "emoji \u{1F600}", "tag \u{E0041}\u{E007F} end", "max \u{FFFD}\u{FFFF}\u{10FFFF}", "\0nul\0", "\u{FEFF}bom",
    "Thai \u{E01}", "Latin-2 \u{17C}\u{F3}\u{142}", "box \u{2500}\u{2591}", "mac \u{F8FF}\u{2122}"];
foreach ($texts as $t) {
    foreach ($supported as $cs) {
        $cases[] = ['iconv', ['UTF-8', $cs, $t]];
        $cases[] = ['iconv', ['UTF-8', $cs.'//IGNORE', $t]];
        $r = @iconv('UTF-8', $cs, $t);
        if ($r !== false) {
            $cases[] = ['iconv', [$cs, 'UTF-8', $r]];
            $cases[] = ['iconv', ['UTF-8', $cs.'//TRANSLIT', $t]];
        }
    }
}
// every byte through every single-byte charset and UTF-8
$all = '';
for ($b = 0; $b < 256; $b++) {
    $all .= chr($b);
}
foreach ($supported as $cs) {
    $cases[] = ['iconv', [$cs, 'UTF-8', $all]];
    $cases[] = ['iconv', [$cs, 'UTF-8//IGNORE', $all]];
    $cases[] = ['iconv', [$cs, 'UTF-16BE', 'abc']];
}
for ($b = 0; $b < 256; $b++) {
    foreach (['ASCII', 'ISO-8859-6', 'ISO-8859-11', 'CP1252', 'CP1253', 'KOI8-U', 'MACINTOSH', 'CP437'] as $cs) {
        $cases[] = ['iconv', [$cs, 'UCS-4LE', chr($b)]];
    }
}

// 3. invalid and incomplete input, odd code points
$bad = ["a\xFFb", "a\xC3", "a\xC3x", "\xE2\x82", "\xE2\x82x", "\xC0\x80", "\xE0\x80\x80", "\xED\xA0\x80", "\xF4\x90\x80\x80",
    "\xF7\xBF\xBF\xBF", "\xF8\x88\x80\x80\x80", "\xFC\x84\x80\x80\x80\x80", "\xFD\xBF\xBF\xBF\xBF\xBF", "\xFE", "\xFF\x80\x80",
    "\x80\x80\x80\x80\x80\x80\x80x", "a\xF0\x9F\x98", "\xF0\x9F\x98\x80\xF0", "\xC2", "x\xE0\xA0", "\xEF\xBF\xBF",
    "\xF8\x80\x80\x80\x80", "\xC3\xA9\xFF\xC3\xA9", "\xE2\x82\xAC\xC3", "a\xFFb\xE2\x82"];
foreach ($bad as $s) {
    foreach (['UTF-8', 'UTF-8//IGNORE', 'ISO-8859-1', 'ISO-8859-1//IGNORE', 'UTF-16', 'UCS-4', 'UTF-32BE', 'ASCII//IGNORE',
        'UCS-2BE//IGNORE', 'UTF-8//ignore'] as $to) {
        $cases[] = ['iconv', ['UTF-8', $to, $s]];
    }
}
$ucs4 = [0x41, 0xD800, 0xDFFF, 0xFFFE, 0xFFFF, 0x10000, 0x10FFFF, 0x110000, 0xE0001, 0xE0041, 0xE007F, 0xE0080,
    0x7FFFFFFF, 0x80000000, 0xFFFFFFFF, 0x20AC];
foreach ($ucs4 as $cp) {
    foreach (['UCS-4', 'UCS-4LE'] as $from) {
        $in = $from === 'UCS-4' ? pack('N', $cp) : pack('V', $cp);
        foreach (['UTF-8', 'ISO-8859-1', 'ASCII', 'CP1252', 'UCS-2', 'UCS-2BE', 'UNICODE', 'UTF-16', 'UTF-16BE', 'UTF-32',
            'UTF-32LE', 'UCS-4', 'UCS-4LE', 'ISO-8859-1//IGNORE', 'UCS-2//IGNORE', 'UCS-2BE//IGNORE', 'UTF-16//IGNORE',
            'UTF-32//IGNORE', 'UTF-8//IGNORE'] as $to) {
            $cases[] = ['iconv', [$from, $to, 'a'.$in.'b']];
            $cases[] = ['iconv', [$from.'//IGNORE', $to, $in.$in]];
        }
    }
    $cases[] = ['iconv', ['UCS-4', 'UTF-8//IGNORE', pack('N', $cp).'AB']];
}
foreach (["\xFF\xFEa\x00", "\xFE\xFF\x00a", "a\x00", "\x00a", "a", "", "\xFF", "\xFF\xFE", "\x00\xD8\x00\xDC", "\x00\xD8a\x00",
    "\x00\xDC", "\x00\xD8", "\xD8\x00\xDC\x00", "\x3D\xD8\x00\xDE"] as $s) {
    foreach (['UTF-16', 'UTF-16LE', 'UTF-16BE', 'UCS-2', 'UCS-2BE', 'UNICODE', 'UTF-16//IGNORE', 'UCS-2BE//IGNORE',
        'UCS-2//IGNORE', 'UNICODE//IGNORE'] as $from) {
        $cases[] = ['iconv', [$from, 'UTF-8', $s]];
        $cases[] = ['iconv', [$from, 'UTF-8//IGNORE', $s]];
    }
}
foreach (["\xFF\xFE\x00\x00a\x00\x00\x00", "\x00\x00\xFE\xFF\x00\x00\x00a", "a\x00\x00\x00", "\x00\x00\x00a", "abc",
    "\x00\xD8\x00\x00", "\x00\x00\x11\x00", "\xFF\xFF\x10\x00", ""] as $s) {
    foreach (['UTF-32', 'UTF-32LE', 'UTF-32BE', 'UCS-4', 'UCS-4LE', 'UTF-32//IGNORE', 'UCS-4//IGNORE'] as $from) {
        $cases[] = ['iconv', [$from, 'UTF-8', $s]];
        $cases[] = ['iconv', [$from, 'UTF-16LE', $s.$s]];
    }
}
foreach (['UTF-16', 'UTF-32', 'UNICODE', 'UTF-16LE', 'UCS-2'] as $to) {
    $cases[] = ['iconv', ['UTF-8', $to, '']];
    $cases[] = ['iconv', ['UTF-8', $to, "\xFF"]];
    $cases[] = ['iconv', ['UTF-8', $to.'//IGNORE', "\xFF"]];
    $cases[] = ['iconv', ['UTF-8', $to.'//IGNORE', "\xFFab"]];
}

// 4. random strings
$alpha = "aZ\x00\x7F\x80\x9F\xA0\xC2\xC3\xA9\xE2\x82\xAC\xF0\x9F\x98\xBF\xFE\xFF\xED\xA0\xF4\x90";
for ($k = 0; $k < 3000; $k++) {
    $from = $supported[mt_rand(0, count($supported) - 1)];
    $to = $supported[mt_rand(0, count($supported) - 1)];
    $suffix = ['', '', '//IGNORE', '//ignore', '//IGNORE//TRANSLIT', ''][mt_rand(0, 5)];
    $s = rand_str($alpha, 0, 14);
    if ($suffix === '//IGNORE//TRANSLIT') {
        $r = @iconv($from, $to, $s);
        if ($r === false) {
            $suffix = '//IGNORE';
        }
    }
    $cases[] = ['iconv', [$from, $to.$suffix, $s]];
}

// 5. //TRANSLIT: blocks of code points against several targets, and combinations
$blocks = [[0xA0, 0x24F], [0x250, 0x52F], [0x1E00, 0x1FFF], [0x2000, 0x22FF], [0x2300, 0x27FF], [0x2800, 0x2BFF],
    [0x2C00, 0x33FF], [0xA700, 0xA7FF], [0xFB00, 0xFFEF], [0x1D400, 0x1D7FF], [0x1F100, 0x1F2FF]];
foreach ($blocks as [$lo, $hi]) {
    for ($start = $lo; $start <= $hi; $start += 256) {
        $s = '';
        for ($cp = $start; $cp <= min($hi, $start + 255); $cp++) {
            if ($cp < 0xD800 || $cp > 0xDFFF) {
                $s .= mb_chr($cp, 'UTF-8');
            }
        }
        foreach (['ASCII', 'ISO-8859-1', 'CP1252', 'KOI8-R', 'UCS-2'] as $to) {
            $cases[] = ['iconv', ['UTF-8', $to.'//TRANSLIT', $s]];
        }
    }
}
foreach (['ASCII', 'ISO-8859-1', 'ISO-8859-5', 'UCS-2', 'UCS-2BE', 'UNICODE', 'UTF-16', 'UTF-32', 'UTF-8',
    'MACINTOSH', 'CP437'] as $to) {
    foreach (['', '//IGNORE', '//TRANSLIT', '//TRANSLIT//IGNORE', '//IGNORE//TRANSLIT', '//translit'] as $suffix) {
        foreach (["\u{1D400}x", "a\u{20AC}\xFFb\u{201C}", "\u{4E2D}\u{E0041}\u{20AC}", "\u{20AC}\xE2\x82", "x\u{1F600}y",
            "\xF8\x88\x80\x80\x80z", "\u{C6}\u{FB01}\u{2153}", "\xED\xA0\x80q"] as $s) {
            $cases[] = ['iconv', ['UTF-8', $to.$suffix, $s]];
        }
    }
    $cases[] = ['iconv', ['UCS-4', $to.'//TRANSLIT', pack('N*', 0x41, 0xD800, 0x20AC, 0x110000, 0x7FFFFFFF, 0x42)]];
}

// 6. long inputs (section "long", input = prefix repeated n times + tail, output as length and FNV-1a 64 hash):
// glibc's 8160-character rounds, PHP's output-buffer growth and its //IGNORE byte skipping.
$long = [];
foreach ([8158, 8159, 8160, 8161, 16319, 16320, 16321] as $n) {
    foreach (["\u{20AC}", "\u{E9}", "\xFF", "\u{E0041}"] as $special) {
        $long[] = ['UTF-8', 'ISO-8859-1//IGNORE', 'a', $n, $special.'bcd'.$special.'xyz'];
        $long[] = ['UTF-8', 'ASCII//IGNORE', 'a', $n, $special.'bcd'.$special.'xyz'];
    }
    $long[] = ['UTF-8', 'ISO-8859-1//IGNORE', "\u{E9}", $n, "\u{20AC}\u{E9}\u{E9}x"];
    $long[] = ['UTF-8', 'UTF-16//IGNORE', 'b', $n, "\xFF\u{1F600}z"];
    $long[] = ['ISO-8859-1', 'UTF-8', "\xE9", $n, 'a'];
    $long[] = ['UTF-8', 'UCS-2//IGNORE', 'c', $n, "\u{1F600}\u{1F600}de"];
    $long[] = ['UTF-8', 'UCS-2BE//IGNORE', 'c', $n, "\u{1F600}\u{1F600}de"];
    $long[] = ['UCS-4', 'UTF-8//IGNORE', "\0\0\0a", $n, "\xFF\xFF\xFF\xFF\0\0\0b"];
    $long[] = ['UTF-8', 'CP1252//IGNORE', "\u{20AC}", $n, "\u{4E2D}\u{20AC}"];
}
$long[] = ['UTF-8', 'ISO-8859-1//IGNORE', "a\u{20AC}", 9000, ''];
$long[] = ['UTF-8', 'ISO-8859-1', "a\u{E9}", 9000, "\u{20AC}"];
$long[] = ['UTF-8', 'UTF-32', "\u{1F600}", 5000, ''];
$long[] = ['UTF-16LE', 'UTF-8', "a\x00", 9000, "\x00\xD8"];
$long[] = ['UTF-8', 'UTF-16', "\u{1F600}x", 6000, ''];
$long[] = ['UTF-8', 'ISO-8859-1//IGNORE', "\u{20AC}\u{E9}", 12000, 'end'];
foreach ([8159, 8160, 8161] as $n) {
    $long[] = ['UTF-8', 'ASCII//TRANSLIT', 'a', $n, "\u{20AC}\u{4E2D}z"];
    $long[] = ['UTF-8', 'ASCII//IGNORE//TRANSLIT', 'b', $n, "\u{20AC}\xFF\u{E0041}z"];
    $long[] = ['UTF-8', 'UNICODE//TRANSLIT', 'c', $n, "\u{1D400}\u{1F600}d"];
    $long[] = ['UTF-8', 'ISO-8859-1//TRANSLIT//IGNORE', "\u{20AC}", $n, "\u{4E2D}\xFFx"];
}
$longRows = [];
foreach ($long as [$from, $to, $prefix, $n, $tail]) {
    [$r, $d] = capture(static fn () => iconv($from, $to, str_repeat($prefix, $n).$tail));
    $longRows[] = ['from' => $from, 'to' => $to, 'prefix' => bin2hex($prefix), 'n' => $n, 'tail' => bin2hex($tail),
        'r' => is_string($r) ? ['len' => strlen($r), 'fnv' => hash('fnv1a64', $r)] : ['false' => true], 'd' => $d];
}

emit('iconv.gen.php', ['cases' => run_cases($cases), 'long' => $longRows]);
