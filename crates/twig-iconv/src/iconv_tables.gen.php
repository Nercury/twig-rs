<?php
// Generates iconv_tables.rs and iconv/data/sbcs.bin: the single-byte charset tables of glibc 2.41 (as used by
// PHP 8.5.13's iconv() in the oracle image), measured through iconv() itself, the charset names glibc's gconv
// cache knows, and the transliteration table of the C.UTF-8 locale.
//
//   cd crates/twig-iconv/src
//   MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//       php /t/iconv_tables.gen.php > iconv_tables.rs
//
// Tables: every gconv module built from glibc's iconvdata/8bit-gap.c or 8bit-generic.c, plus ARMSCII-8,
// ISO_11548-1 and the ISO646 variants (all stateless, one byte per character). For each, the code point of every
// byte (decoding through UCS-4LE; 0xFFFF for bytes the decoder rejects) goes to sbcs.bin (256 little-endian u16
// per charset, in the order of SBCS); the bytes the encoder produces for code points 0..=0xFFFE that differ from
// the inverse of the decoding table (lowest byte first) go to "extra", code points the encoder rejects although
// some byte decodes to them to "missing".
// Names: every alias and module name of /usr/lib/x86_64-linux-gnu/gconv/gconv-modules{,.d/*.conf} plus glibc's
// builtin names (iconv/gconv_builtin.h, little-endian), each with the module name it resolves to.

declare(strict_types=0);
error_reporting(E_ALL & ~E_NOTICE);

$sbcs = [
    'ARMSCII-8//', 'ASMO_449//', 'BRF//', 'BS_4730//', 'CP10007//', 'CP1125//', 'CP1250//', 'CP1251//', 'CP1252//',
    'CP1253//', 'CP1254//', 'CP1256//', 'CP1257//', 'CP737//', 'CP770//', 'CP771//', 'CP772//', 'CP773//',
    'CP774//', 'CP775//', 'CSA_Z243.4-1985-1//', 'CSA_Z243.4-1985-2//', 'CSN_369103//', 'CWI//', 'DEC-MCS//',
    'DIN_66003//', 'DS_2089//', 'EBCDIC-AT-DE-A//', 'EBCDIC-AT-DE//', 'EBCDIC-CA-FR//', 'EBCDIC-DK-NO-A//',
    'EBCDIC-DK-NO//', 'EBCDIC-ES-A//', 'EBCDIC-ES-S//', 'EBCDIC-ES//', 'EBCDIC-FI-SE-A//', 'EBCDIC-FI-SE//',
    'EBCDIC-FR//', 'EBCDIC-IS-FRISS//', 'EBCDIC-IT//', 'EBCDIC-PT//', 'EBCDIC-UK//', 'EBCDIC-US//',
    'ECMA-CYRILLIC//', 'ES//', 'ES2//', 'GB_1988-80//', 'GEORGIAN-ACADEMY//', 'GEORGIAN-PS//', 'GOST_19768-74//',
    'GREEK-CCITT//', 'GREEK7-OLD//', 'GREEK7//', 'HP-GREEK8//', 'HP-ROMAN8//', 'HP-ROMAN9//', 'HP-THAI8//',
    'HP-TURKISH8//', 'IBM037//', 'IBM038//', 'IBM1004//', 'IBM1008//', 'IBM1025//', 'IBM1026//', 'IBM1046//',
    'IBM1047//', 'IBM1097//', 'IBM1112//', 'IBM1122//', 'IBM1123//', 'IBM1124//', 'IBM1129//', 'IBM1130//',
    'IBM1132//', 'IBM1133//', 'IBM1137//', 'IBM1140//', 'IBM1141//', 'IBM1142//', 'IBM1143//', 'IBM1144//',
    'IBM1145//', 'IBM1146//', 'IBM1147//', 'IBM1148//', 'IBM1149//', 'IBM1153//', 'IBM1154//', 'IBM1155//',
    'IBM1156//', 'IBM1157//', 'IBM1158//', 'IBM1160//', 'IBM1161//', 'IBM1162//', 'IBM1163//', 'IBM1164//',
    'IBM1166//', 'IBM1167//', 'IBM12712//', 'IBM16804//', 'IBM256//', 'IBM273//', 'IBM274//', 'IBM275//',
    'IBM277//', 'IBM278//', 'IBM280//', 'IBM281//', 'IBM284//', 'IBM285//', 'IBM290//', 'IBM297//', 'IBM420//',
    'IBM423//', 'IBM424//', 'IBM437//', 'IBM4517//', 'IBM4899//', 'IBM4909//', 'IBM4971//', 'IBM500//', 'IBM5347//',
    'IBM803//', 'IBM850//', 'IBM851//', 'IBM852//', 'IBM855//', 'IBM856//', 'IBM857//', 'IBM858//', 'IBM860//',
    'IBM861//', 'IBM862//', 'IBM863//', 'IBM864//', 'IBM865//', 'IBM866//', 'IBM866NAV//', 'IBM868//', 'IBM869//',
    'IBM870//', 'IBM871//', 'IBM874//', 'IBM875//', 'IBM880//', 'IBM891//', 'IBM901//', 'IBM902//', 'IBM903//',
    'IBM9030//', 'IBM904//', 'IBM905//', 'IBM9066//', 'IBM918//', 'IBM921//', 'IBM922//', 'IBM9448//',
    'IEC_P27-1//', 'INIS-8//', 'INIS-CYRILLIC//', 'INIS//', 'ISIRI-3342//', 'ISO-8859-10//', 'ISO-8859-11//',
    'ISO-8859-13//', 'ISO-8859-14//', 'ISO-8859-15//', 'ISO-8859-16//', 'ISO-8859-2//', 'ISO-8859-3//',
    'ISO-8859-4//', 'ISO-8859-5//', 'ISO-8859-6//', 'ISO-8859-7//', 'ISO-8859-8//', 'ISO-8859-9//', 'ISO-8859-9E//',
    'ISO-IR-197//', 'ISO-IR-209//', 'ISO_10367-BOX//', 'ISO_11548-1//', 'ISO_2033//', 'ISO_5427-EXT//',
    'ISO_5427//', 'ISO_5428//', 'IT//', 'JIS_C6220-1969-RO//', 'JIS_C6229-1984-B//', 'JUS_I.B1.002//', 'KOI-8//',
    'KOI8-R//', 'KOI8-RU//', 'KOI8-T//', 'KOI8-U//', 'KSC5636//', 'LATIN-GREEK-1//', 'LATIN-GREEK//',
    'MAC-CENTRALEUROPE//', 'MAC-IS//', 'MAC-SAMI//', 'MAC-UK//', 'MACINTOSH//', 'MIK//', 'MSZ_7795.3//',
    'NATS-DANO//', 'NATS-SEFI//', 'NC_NC00-10//', 'NF_Z_62-010//', 'NF_Z_62-010_1973//', 'NS_4551-1//',
    'NS_4551-2//', 'PT//', 'PT154//', 'PT2//', 'RK1048//', 'SEN_850200_B//', 'SEN_850200_C//', 'TIS-620//',
    'VISCII//', 'WIN-SAMI-2//',
];
sort($sbcs, SORT_STRING);

echo "//! Single-byte charset index, charset names and transliteration table of glibc 2.41, measured in the\n";
echo "//! oracle image by `iconv_tables.gen.php` (next to this file; it also writes `iconv/data/sbcs.bin`).\n";
echo "//! Regenerate with the command in its header. Do not edit.\n\n";
echo "use super::iconv::Sbcs;\n\n";

$blob = '';
echo "/// The single-byte charsets (gconv module names, sorted) with the encoder's deviations from the inverse\n";
echo "/// of the decoding table; the decoding table of entry `i` is `SBCS_DATA[512 * i..512 * (i + 1)]`.\n";
echo 'pub(super) static SBCS: [Sbcs; ', count($sbcs), "] = [\n";
foreach ($sbcs as $module) {
    $cs = rtrim($module, '/');
    $dec = [];
    for ($b = 0; $b < 256; $b++) {
        $r = iconv($cs, 'UCS-4LE', chr($b));
        if ($r !== false && strlen($r) !== 4) {
            throw new \RuntimeException("$cs: byte $b decodes to several characters");
        }
        $dec[$b] = $r === false ? 0xFFFF : unpack('V', $r)[1];
        if ($dec[$b] > 0xFFFF || ($r !== false && $dec[$b] === 0xFFFF)) {
            throw new \RuntimeException("$cs: byte $b decodes to U+FFFF or beyond");
        }
        $blob .= pack('v', $dec[$b]);
    }
    $all = @iconv($cs, 'UCS-4LE//IGNORE', implode('', array_map('chr', range(0, 255))));
    $want = implode('', array_map(static fn ($c) => $c === 0xFFFF ? '' : pack('V', $c), $dec));
    if ($all !== $want) {
        throw new \RuntimeException("$cs: decoding is not stateless");
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
    // Nothing beyond the BMP maps; tag characters are dropped silently.
    foreach ([0xFFFF, 0x10000, 0x1F600, 0xE0000, 0xE0041, 0xE007F, 0xE0080, 0x10FFFF, 0x110000, 0x7FFFFFFF] as $cp) {
        $r = iconv('UCS-4LE', $cs, pack('V', $cp));
        $tag = ($cp >> 7) === (0xE0000 >> 7);
        if (($tag && $r !== '') || (!$tag && $r !== false)) {
            throw new \RuntimeException("$cs: unexpected result for U+".dechex($cp));
        }
    }
    echo '    Sbcs {', "\n";
    echo '        module: ', json_encode($module, JSON_UNESCAPED_SLASHES), ",\n";
    echo '        extra: &[', implode(', ', array_map(static fn ($p) => sprintf('(0x%04X, 0x%02X)', $p[0], $p[1]), $extra)), "],\n";
    echo '        missing: &[', implode(', ', array_map(static fn ($c) => sprintf('0x%04X', $c), $missing)), "],\n";
    echo "    },\n";
}
echo "];\n\n";
@mkdir(__DIR__.'/iconv/data', 0777, true);
file_put_contents(__DIR__.'/iconv/data/sbcs.bin', $blob);
echo "/// The decoding tables of [`SBCS`]: 256 little-endian `u16` code points per charset, `0xFFFF` for\n";
echo "/// bytes the decoder rejects.\n";
echo 'pub(super) static SBCS_DATA: &[u8; ', strlen($blob), "] = include_bytes!(\"iconv/data/sbcs.bin\");\n\n";

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
