<?php
// Generates iconv_tables.rs: the single-byte charset tables of glibc 2.41 (as used by PHP 8.5.13's iconv() in
// the oracle image), measured through iconv() itself, and the charset names glibc's gconv cache knows.
//
//   cd crates/twig-value/src/builtins/string
//   MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//       php /t/iconv_tables.gen.php > iconv_tables.rs
//
// Tables: for each charset, the code point of every byte (decoding through UCS-4LE) and the bytes the encoder
// produces for every code point 0..=0xFFFE that differ from the inverse of the decoding table ("extra"), or code
// points the encoder rejects although some byte decodes to them ("missing").
// Names: every alias and module name of /usr/lib/x86_64-linux-gnu/gconv/gconv-modules{,.d/*.conf} plus glibc's
// builtin names (iconv/gconv_builtin.h, little-endian), each with the module name it resolves to.

declare(strict_types=0);
error_reporting(E_ALL & ~E_NOTICE);

const CHARSETS = [
    // canonical gconv name => Rust identifier
    'ISO-8859-2//' => 'ISO_8859_2', 'ISO-8859-3//' => 'ISO_8859_3', 'ISO-8859-4//' => 'ISO_8859_4',
    'ISO-8859-5//' => 'ISO_8859_5', 'ISO-8859-6//' => 'ISO_8859_6', 'ISO-8859-7//' => 'ISO_8859_7',
    'ISO-8859-8//' => 'ISO_8859_8', 'ISO-8859-9//' => 'ISO_8859_9', 'ISO-8859-10//' => 'ISO_8859_10',
    'ISO-8859-11//' => 'ISO_8859_11', 'ISO-8859-13//' => 'ISO_8859_13', 'ISO-8859-14//' => 'ISO_8859_14',
    'ISO-8859-15//' => 'ISO_8859_15', 'ISO-8859-16//' => 'ISO_8859_16', 'CP1250//' => 'CP1250',
    'CP1251//' => 'CP1251', 'CP1252//' => 'CP1252', 'CP1253//' => 'CP1253', 'CP1254//' => 'CP1254',
    'CP1256//' => 'CP1256', 'CP1257//' => 'CP1257', 'KOI8-R//' => 'KOI8_R', 'KOI8-U//' => 'KOI8_U',
    'IBM866//' => 'IBM866', 'MACINTOSH//' => 'MACINTOSH', 'IBM437//' => 'IBM437', 'IBM850//' => 'IBM850',
];

function name_of(string $canonical): string
{
    return rtrim($canonical, '/');
}

echo "//! Single-byte charset tables and charset names of glibc 2.41, measured in the oracle image by\n";
echo "//! `iconv_tables.gen.php` (next to this file); regenerate with the command in its header. Do not edit.\n\n";
echo "use super::iconv::ByteTable;\n\n";

$idents = [];
foreach (CHARSETS as $canonical => $ident) {
    $cs = name_of($canonical);
    $dec = [];
    for ($b = 0; $b < 256; $b++) {
        $r = iconv($cs, 'UCS-4LE', chr($b));
        $dec[$b] = $r === false ? 0xFFFF : unpack('V', $r)[1];
    }
    $inverse = [];
    for ($b = 255; $b >= 0; $b--) {
        if ($dec[$b] !== 0xFFFF) {
            $inverse[$dec[$b]] = $b;
        }
    }
    $extra = [];
    $missing = [];
    for ($cp = 0; $cp <= 0xFFFE; $cp++) {
        $r = iconv('UCS-4LE', $cs, pack('V', $cp));
        $want = $inverse[$cp] ?? null;
        $got = ($r === false || $r === '') ? null : ord($r);
        if ($r !== false && strlen($r) > 1) {
            throw new \RuntimeException("$cs: multi-byte output for U+".dechex($cp));
        }
        if ($got !== $want) {
            if ($got === null) {
                $missing[] = $cp;
            } else {
                $extra[] = [$cp, $got];
            }
        }
    }
    // Nothing beyond the BMP maps (tag characters are silently dropped by the encoder).
    foreach ([0xFFFF, 0x10000, 0x1F600, 0xE0041, 0x10FFFF, 0x110000, 0x7FFFFFFF] as $cp) {
        $r = iconv('UCS-4LE', $cs, pack('V', $cp));
        $tag = ($cp >> 7) === (0xE0000 >> 7);
        if (($tag && $r !== '') || (!$tag && $r !== false)) {
            throw new \RuntimeException("$cs: unexpected result for U+".dechex($cp));
        }
    }
    $idents[$canonical] = $ident;
    echo "/// `$cs`.\n";
    echo "pub(super) static $ident: ByteTable = ByteTable {\n";
    echo "    decode: [\n";
    foreach (array_chunk($dec, 8) as $row) {
        echo '        ', implode(', ', array_map(static fn ($c) => sprintf('0x%04X', $c), $row)), ",\n";
    }
    echo "    ],\n";
    echo '    extra: &[', implode(', ', array_map(static fn ($p) => sprintf('(0x%04X, 0x%02X)', $p[0], $p[1]), $extra)), "],\n";
    echo '    missing: &[', implode(', ', array_map(static fn ($c) => sprintf('0x%04X', $c), $missing)), "],\n";
    echo "};\n\n";
}

// Charset names: alias => module, module => itself.
$names = [];
$files = array_merge(['/usr/lib/x86_64-linux-gnu/gconv/gconv-modules'],
    glob('/usr/lib/x86_64-linux-gnu/gconv/gconv-modules.d/*.conf'));
foreach ($files as $file) {
    foreach (file($file) as $line) {
        $line = trim(preg_replace('/#.*/', '', $line));
        if ($line === '') {
            continue;
        }
        $f = preg_split('/\s+/', $line);
        if ($f[0] === 'alias') {
            $names[strtoupper($f[1])] ??= strtoupper($f[2]);
        } elseif ($f[0] === 'module') {
            foreach ([strtoupper($f[1]), strtoupper($f[2])] as $m) {
                if ($m !== 'INTERNAL') {
                    $names[$m] ??= $m;
                }
            }
        }
    }
}
$builtin = [
    'UCS4//' => 'ISO-10646/UCS4/', 'UCS-4//' => 'ISO-10646/UCS4/', 'UCS-4BE//' => 'ISO-10646/UCS4/',
    'CSUCS4//' => 'ISO-10646/UCS4/', 'ISO-10646//' => 'ISO-10646/UCS4/', '10646-1:1993//' => 'ISO-10646/UCS4/',
    '10646-1:1993/UCS4/' => 'ISO-10646/UCS4/', 'OSF00010104//' => 'ISO-10646/UCS4/',
    'OSF00010105//' => 'ISO-10646/UCS4/', 'OSF00010106//' => 'ISO-10646/UCS4/',
    'ISO-10646/UCS4/' => 'ISO-10646/UCS4/', 'UCS-4LE//' => 'UCS-4LE//', 'WCHAR_T//' => 'INTERNAL',
    'UTF8//' => 'ISO-10646/UTF8/', 'UTF-8//' => 'ISO-10646/UTF8/', 'ISO-IR-193//' => 'ISO-10646/UTF8/',
    'OSF05010001//' => 'ISO-10646/UTF8/', 'ISO-10646/UTF-8/' => 'ISO-10646/UTF8/',
    'ISO-10646/UTF8/' => 'ISO-10646/UTF8/', 'UCS2//' => 'ISO-10646/UCS2/', 'UCS-2//' => 'ISO-10646/UCS2/',
    'OSF00010100//' => 'ISO-10646/UCS2/', 'OSF00010101//' => 'ISO-10646/UCS2/',
    'OSF00010102//' => 'ISO-10646/UCS2/', 'ISO-10646/UCS2/' => 'ISO-10646/UCS2/',
    'ANSI_X3.4//' => 'ANSI_X3.4-1968//', 'ISO-IR-6//' => 'ANSI_X3.4-1968//',
    'ANSI_X3.4-1986//' => 'ANSI_X3.4-1968//', 'ISO_646.IRV:1991//' => 'ANSI_X3.4-1968//',
    'ASCII//' => 'ANSI_X3.4-1968//', 'ISO646-US//' => 'ANSI_X3.4-1968//', 'US-ASCII//' => 'ANSI_X3.4-1968//',
    'US//' => 'ANSI_X3.4-1968//', 'IBM367//' => 'ANSI_X3.4-1968//', 'CP367//' => 'ANSI_X3.4-1968//',
    'CSASCII//' => 'ANSI_X3.4-1968//', 'OSF00010020//' => 'ANSI_X3.4-1968//',
    'ANSI_X3.4-1968//' => 'ANSI_X3.4-1968//', 'UNICODELITTLE//' => 'ISO-10646/UCS2/',
    'UCS-2LE//' => 'ISO-10646/UCS2/', 'UCS-2BE//' => 'UNICODEBIG//', 'UNICODEBIG//' => 'UNICODEBIG//',
];
foreach ($builtin as $alias => $module) {
    $names[$alias] = $module;
}
// Resolve alias chains and keep only names whose module exists.
ksort($names, SORT_STRING);
echo "/// Every charset name glibc's gconv cache resolves (after `strip()`: uppercase, two `/`), with the\n";
echo "/// module it names, sorted for binary search.\n";
echo 'pub(super) static NAMES: [(&str, &str); ', count($names), "] = [\n";
foreach ($names as $alias => $module) {
    $seen = 0;
    while (isset($names[$module]) && $names[$module] !== $module && $seen++ < 8) {
        $module = $names[$module];
    }
    printf("    (%s, %s),\n", json_encode($alias, JSON_UNESCAPED_SLASHES), json_encode($module, JSON_UNESCAPED_SLASHES));
}
echo "];\n";

// Transliteration table of the C.UTF-8 locale (LC_CTYPE items _NL_CTYPE_TRANSLIT_*, glibc locale/langinfo.h:
// TAB_SIZE 61, FROM_IDX 62, FROM_TBL 63, TO_IDX 64, TO_TBL 65, DEFAULT_MISSING_LEN 66, DEFAULT_MISSING 67,
// IGNORE_LEN 68).
$f = file_get_contents('/usr/lib/locale/C.utf8/LC_CTYPE');
$h = unpack('Vmagic/Vn', $f);
$off = array_values(unpack('V'.$h['n'], $f, 8));
$off[] = strlen($f);
$item = static fn ($i) => substr($f, $off[$i], $off[$i + 1] - $off[$i]);
$word = static fn ($i) => unpack('V', $item($i))[1];
$u32 = static fn ($i) => array_values(unpack('V*', $item($i)));
$size = $word(61);
[$fromIdx, $fromTbl, $toIdx, $toTbl] = [$u32(62), $u32(63), $u32(64), $u32(65)];
if ($word(68) !== 0) {
    throw new \RuntimeException('translit_ignore ranges are not supported');
}
$missing = array_slice($u32(67), 0, $word(66));
$from = [];
$to = [];
$prev = -1;
for ($k = 0; $k < $size; $k++) {
    $i = $fromIdx[$k];
    if ($fromTbl[$i + 1] !== 0 || $fromTbl[$i] <= $prev) {
        throw new \RuntimeException('multi-character or unsorted transliteration source');
    }
    $prev = $from[] = $fromTbl[$i];
    $j = $toIdx[$k];
    $first = true;
    do {
        if (!$first) {
            $to[] = '\u{1}';
        }
        $first = false;
        while ($toTbl[$j] !== 0) {
            $cp = $toTbl[$j++];
            if ($cp === 1) {
                throw new \RuntimeException('U+0001 in a transliteration');
            }
            $to[] = ($cp >= 0x20 && $cp < 0x7F && $cp !== 0x22 && $cp !== 0x5C) ? chr($cp) : sprintf('\u{%X}', $cp);
        }
        $j++;
    } while ($toTbl[$j] !== 0);
    $to[] = '\0';
}
echo "\n/// Transliteration sources of the C.UTF-8 locale (`translit_neutral` and `translit_combining`), sorted.\n";
echo 'pub(super) static TRANSLIT_FROM: [u32; ', count($from), "] = [\n";
foreach (array_chunk($from, 10) as $row) {
    echo '    ', implode(', ', array_map(static fn ($c) => sprintf('0x%X', $c), $row)), ",\n";
}
echo "];\n\n";
echo "/// The replacement candidates of each source in order: candidates separated by U+0001, entries\n";
echo "/// terminated by U+0000.\n";
echo "pub(super) static TRANSLIT_TO: &str = concat!(\n";
$line = '';
foreach ($to as $token) {
    // tokens are single characters or whole escape sequences, so lines never split an escape
    if (strlen($line) + strlen($token) > 90) {
        echo '    "', $line, "\",\n";
        $line = '';
    }
    $line .= $token;
}
echo '    "', $line, "\",\n";
echo ");\n\n";
echo "/// `default_missing` of the C.UTF-8 locale.\n";
echo 'pub(super) static TRANSLIT_DEFAULT_MISSING: &[u32] = &[', implode(', ', array_map(static fn ($c) => sprintf('0x%X', $c), $missing)), "];\n";
