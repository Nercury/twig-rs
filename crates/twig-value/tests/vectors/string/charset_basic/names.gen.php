<?php
// names.json: charset names. Every name `iconv -l` lists (as listed, lower-cased, and in the forms glibc prints),
// and many spellings of a few names: case, dropped bytes, white space, commas and slashes, the suffix list
// (//TRANSLIT, //IGNORE in every combination and syntax PHP and glibc treat differently), the empty name (the
// locale's charset), NUL bytes and the 64-byte limit, as source and as target.

require __DIR__ . '/common.inc.php';

$cases = [];
$probe = static function (string $from, string $to) use (&$cases): void {
    foreach (["a\u{e9}b", "a\xffb", "a\u{4e00}b", "a\u{1F600}"] as $s) {
        $cases[] = conversion($from, $to, $s);
    }
};

// every name glibc knows, as `iconv -l` prints it and without the slashes
foreach (preg_split('/\s+/', trim((string) shell_exec('iconv -l'))) as $listed) {
    $name = str_ends_with($listed, '//') ? substr($listed, 0, -2) : $listed;
    $cases[] = conversion('UTF-8', $name, "a\u{e9}\u{20ac}\u{1F600}");
    $cases[] = conversion($name, 'UTF-8', "a\x00b\x00\xc3\xa9");
    $cases[] = conversion($name, 'UTF-16BE//IGNORE', "\xfe\xff\x00a\xd8\x00");
    $cases[] = conversion('UCS-4', strtolower($name) . '//TRANSLIT', ucs4(0x61, 0xE9, 0x20AC, 0x1F600));
    $cases[] = conversion('UTF-8', $listed, 'a');
}

$suffixes = ['', '/', '//', '///', '////', '//TRANSLIT', '//IGNORE', '//TRANSLIT//IGNORE', '//IGNORE//TRANSLIT',
    '//translit', '//ignore', '//Ignore', '//TRANSLIT,IGNORE', '//IGNORE,TRANSLIT', '//TRANSLIT/IGNORE',
    '//IGNORE/TRANSLIT', '///IGNORE', '///TRANSLIT', '/ /IGNORE', '// IGNORE', '//IGNORE ', "//IGNORE\t", "//IGNORE\n",
    "//IGNORE\r\x0b\x0c", '//IG NORE', '//FOO', '//FOO,IGNORE', '//IGNORE,FOO', '//FOO//IGNORE', '//IGNORE//FOO',
    '//IGNOREX', '//XIGNORE', '//TRANSLITX', '//,IGNORE', '//IGNORE,', '//,', '//TRANSLIT//', '//IGNORE//', '//IGNORE/',
    '/IGNORE', '/TRANSLIT', "//IGNORE\x00junk", ',', ',,', ',//IGNORE', '//IGNORE X', '//IGNORE.', '/X', '/X/',
    '/X//IGNORE', '//IGNORE//IGNORE', '/,/IGNORE', '/, /TRANSLIT', '//TRANSLIT IGNORE', '//ignore//translit',
    "//IGNORE \x01", '//IGNORE,IGNORE', '/////IGNORE', '//X/Y/IGNORE'];
foreach (['UTF-8', 'ISO-8859-1', 'ASCII', 'UCS-2', 'ISO-10646/UCS4', 'UTF-16', 'UCS-2BE', 'EUC-JP', 'NOPE', ''] as $base) {
    foreach ($suffixes as $suffix) {
        $probe('UTF-8', $base . $suffix);
    }
    $cases[] = conversion($base . '//IGNORE', 'UTF-16BE', "a\xffb");
    $cases[] = conversion($base . '//TRANSLIT', 'ASCII', "a\u{e9}b");
    $cases[] = conversion(strtolower($base), 'UTF-8', "a\u{e9}b");
}

$spellings = ['utf-8', 'Utf-8', 'UTF8', 'utf8', 'UTF_8', 'UTF 8', 'UTF.8', 'UTF:8', 'UTF--8', ' UTF-8', 'UTF-8 ',
    "UTF-8\t", "UTF-8\n", 'U-T-F-8', 'UTF-08', '/UTF-8', '//UTF-8', 'UTF-8/X', 'UTF-8/X/', 'UTF-8/X//', 'UTF-8//X', '',
    '/', '//', '///', '-', 'ISO-10646/UTF8', 'ISO-10646/UTF8/', 'iso-10646/utf8/', 'ISO-10646', 'ISO-10646/',
    'ISO-10646//', 'ISO-10646/UCS4', 'ISO-10646/UCS4/', 'ISO-10646/UCS4/TRANSLIT', 'ISO-10646/UCS4/IGNORE',
    'ISO-10646/UCS2', 'ISO-10646/UCS2/IGNORE', "UTF8\x00junk", "\x00", "\x00UTF-16", 'ISO8859-1', 'ISO 8859-1',
    'ISO-8859-1:1987', 'ISO_8859-1:1987', 'LATIN1', 'latin1', 'Latin-1', 'L1', 'CP819', 'IBM-819', 'ISO88591',
    'ISO-88591', '8859_1', '8859-1', 'ansi_x3.4-1968', 'US', 'us', 'USASCII', 'US_ASCII', 'ucs-2', 'UCS-4-BE',
    'UCS4BE', 'UTF-16-LE', 'utf16le', 'UNICODELITTLE', 'UNICODEBIG', 'WCHAR_T', 'UNICODE', 'INTERNAL', 'CHAR', 'UTF-7',
    'CP1252', 'ASCII,', 'ASCII,,', ',ASCII', 'AS,CII', 'ASCII,X', 'ASCII,/', 'ASCII,//IGNORE', 'ASCII,/IGNORE',
    'ASCII/,/IGNORE', 'ISO-10646/UCS2,', 'ISO-10646/UCS2/,', 'ISO-10646/UCS2,/', 'ISO-10646/UCS4,/IGNORE',
    'ISO-10646,/UCS4/IGNORE', 'ISO-10646/,UCS4/IGNORE', 'ISO-10646/UCS4,,/IGNORE', ',', ',,', ',//IGNORE', ' ',
    ' //IGNORE', 'AS CII//IGNORE', "ASCII\x01//IGNORE", 'ascii,,//ignore', 'ASCII/, /IGNORE', 'UTF-8/ /',
    "UTF-8/\x80/", 'ASCII,/,', 'ASCII,/,/IGNORE', '/,', '/,/', '/,//IGNORE', ',/', ',/,', '/ /', ' / / ', '/ //IGNORE',
    'ISO-10646/UCS4,/,', 'ISO-10646/UCS4/,/', 'UTF-8,/,', ',,/,,', 'ASCII/,,', 'ASCII/,,/TRANSLIT', 'ISO-10646/,',
    'ISO-10646/,/IGNORE', 'ISO-10646/,UCS4', ',UTF-8', 'UTF-8,,/', "\x80", "\x80//IGNORE", "ASCII\x00/IGNORE",
    'ISO-10646//UCS4', 'UCS-4/UCS4', 'UCS-4/X', 'ISO-10646/UTF16', 'ISO-10646/X', '10646-1:1993/UCS4/IGNORE',
    'ISO/TR_11548-1', 'ISO', 'ISO/', 'NF_Z_62-010_(1973)', 'NF_Z_62-010_1973', 'IBM891', 'cp903', 'OSF00010101',
    "ISO-10646/UCS 4/IGNORE", "ISO- 10646/UCS4/IGNORE", 'ISO-10646/UCS4/ IGNORE', "UTF-8\xc3\xa9", "\xc3\xa9UTF-8"];
foreach ($spellings as $name) {
    $probe('UTF-8', $name);
    $probe($name, 'UTF-16BE');
    $cases[] = conversion('UTF-16LE', $name, "a\x00\xe9\x00\x00\xd8");
}

// commas, white space, dropped bytes and slashes around a name: which of them glibc strips before dropping bytes
$alphabet = [',', ' ', '(', '/', 'X', "\t", "\x80"];
$tails = [''];
$level = [''];
for ($len = 1; $len <= 3; $len++) {
    $next = [];
    foreach ($level as $p) {
        foreach ($alphabet as $a) {
            $next[] = $p . $a;
        }
    }
    array_push($tails, ...$next);
    $level = $next;
}
foreach ($tails as $tail) {
    $cases[] = conversion('UTF-8', 'US' . $tail, "a\u{e9}");
    $cases[] = conversion('ISO-10646/UCS4' . $tail, 'UTF-8', "\x00\x00\x00a");
}
foreach (['', ',', ' ', '(', ', ', ',(', ' ,', '(,', ',,', "\x80,", ",\x80"] as $s1) {
    foreach (['', ',', ' ', '(', ', ', ',(', ' ,', '(,', ',,'] as $s2) {
        $cases[] = conversion('UTF-8', "ISO-10646{$s1}/UCS4{$s2}", 'a');
        $cases[] = conversion('UTF-8', "{$s1}/{$s2}", 'a');
        $cases[] = conversion('UTF-8', "{$s1}US{$s2}//IGNORE", "a\xff");
    }
}
for ($b = 1; $b < 256; $b++) {
    $cases[] = conversion('UTF-8', 'US,' . chr($b), 'a');
    $cases[] = conversion('UTF-8', 'UTF' . chr($b) . '-8', 'a');
    $cases[] = conversion('UTF-8', 'ASCII//IGNORE' . chr($b), "a\xff");
}

// seeded random names built from fragments
mt_srand(77);
$fragments = ['UTF', 'utf', '-', '8', '16', '32', 'BE', 'le', 'UCS', '2', '4', 'ISO', '10646', '8859', '1', 'LATIN', 'ASCII',
    'US', 'ANSI_X3.4', '-1968', 'UNICODE', 'BIG', 'LITTLE', 'CP', '819', '367', 'IBM', '891', 'L1', '/', '//', ',', ' ', "\t",
    'TRANSLIT', 'IGNORE', 'translit', 'ignore', '_', '.', ':', 'X', "\x80", "\x00", 'EUC-JP', 'WCHAR_T', '(', ')'];
for ($i = 0; $i < 3000; $i++) {
    $name = '';
    for ($n = mt_rand(1, 7); $n > 0; $n--) {
        $name .= $fragments[mt_rand(0, count($fragments) - 1)];
    }
    $cases[] = conversion('UTF-8', $name, "a\u{e9}\xff\u{4e00}");
    $cases[] = conversion($name, 'UTF-16BE', "a\xc3\xa9");
}

// PHP's 64-byte limit counts every byte, the NUL and what follows included; glibc drops the spaces
foreach ([62, 63, 64, 65] as $len) {
    foreach (['UTF-8', "UTF-8\x00", 'UTF-8//IGNORE'] as $start) {
        $name = str_pad($start, $len, ' ');
        $probe('UTF-8', $name);
        $probe($name, 'ISO-8859-1');
    }
    $cases[] = conversion(str_repeat('A', $len), str_repeat('B', $len), 'a');
}

emit('names.gen.php', $cases);
