<?php
// Generates iconv_charsets.json: iconv() on glibc 2.41 for the charsets beyond the UTF variants and the common
// single-byte ones (iconv.gen.php covers those): the CJK multi-byte charsets, the stateful ISO-2022 family, the
// composing single-byte charsets CP1255/CP1258/TCVN5712-1 and every other single-byte module. Aliases, texts in
// both directions with //IGNORE and //TRANSLIT, invalid and incomplete input around every lead-byte class, shift
// sequences, flushing, combining sequences, random strings, and long inputs around glibc's conversion rounds and
// PHP's output-buffer growth (section "long", outputs as length and FNV-1a 64 hash).
//
// The cases go through twig-value's call table (crates/twig-value/tests/vectors/string/calls.inc.php and the files
// it includes), so the container sees the whole crates/ directory:
//
//   cd crates
//   MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/c" twig-oracle:3.31.1 \
//       php /c/twig-iconv/tests/vectors/iconv_charsets.gen.php > twig-iconv/tests/vectors/iconv_charsets.json

declare(strict_types=0);
require __DIR__.'/../../../twig-value/tests/vectors/string/calls.inc.php';

mt_srand(2262);
$cases = [];
$add = static function (string $from, string $to, string $s) use (&$cases) {
    $cases[] = ['iconv', [$from, $to, $s]];
};

$mb = ['EUC-JP', 'EUC-JP-MS', 'SJIS', 'CP932', 'EUC-CN', 'GBK', 'GB18030', 'EUC-KR', 'UHC', 'BIG5', 'BIG5-HKSCS',
    'EUC-JISX0213', 'SHIFT_JISX0213'];
$iso2022 = ['ISO-2022-JP', 'ISO-2022-JP-2', 'ISO-2022-JP-3', 'ISO-2022-KR'];
$composing = ['CP1255', 'CP1258', 'TCVN5712-1'];
$new = array_merge($mb, $iso2022, $composing);

// 1. names
foreach (['SHIFT_JIS', 'shift-jis', 'MS_KANJI', 'CSSHIFTJIS', 'WINDOWS-31J', 'MS932', 'SJIS-OPEN', 'SJIS-WIN',
    'CSWINDOWS31J', 'EUCJP', 'eucJP', 'UJIS', 'GB2312', 'csGB2312', 'CN-GB', 'EUCCN', 'CP936', 'WINDOWS-936', 'GB18030',
    'EUCKR', 'CSEUCKR', 'CP949', 'MSCP949', 'BIG-5', 'BIGFIVE', 'BIG-FIVE', 'CN-BIG5', 'CP950', 'BIG5HKSCS',
    'big5-hkscs', 'CSISO2022JP', 'ISO2022JP', 'CSISO2022JP2', 'ISO2022JP2', 'ISO-2022-JP-1', 'ISO-2022-JP-3',
    'ISO2022KR', 'CSISO2022KR', 'WINDOWS-1255', 'MS-HEBR', 'WINDOWS-1258', 'TCVN', 'TCVN-5712', 'TCVN5712-1:1993',
    'EUC-JISX0213', 'SHIFT_JISX0213', 'ShiftJISX0213', 'EUCJP-WIN', 'eucJP-ms', 'EUCJP-OPEN', 'KOI8-RU',
    'WINDOWS-874', 'CP874', 'IBM-1047', 'EBCDIC-US',
    'MAC-CYRILLIC', 'MACCYRILLIC', 'CP1125', 'ARMSCII-8', 'GEORGIAN-PS', 'TIS-620', 'VISCII', 'ISO646-DE', 'DIN_66003',
    'CP737', 'CP775', 'IBM1140', 'EUC-TW', 'UTF-7', 'JOHAB', 'BIG5-HKSCS//IGNORE', 'GBK//TRANSLIT'] as $n) {
    if (in_array($n, ['EUC-TW', 'UTF-7', 'JOHAB'], true)) {
        continue; // not converted by twig-iconv; the name parsing of these is covered by iconv.json
    }
    $add('UTF-8', $n, "a\u{E9}\u{20AC}\u{611B}");
    $add($n, 'UTF-8', 'abc');
}

// 2. texts in every direction
$texts = [
    "plain ASCII ~\\ text",
    "\u{65E5}\u{672C}\u{8A9E}\u{306E}\u{30C6}\u{30AD}\u{30B9}\u{30C8}\u{3001}\u{FF76}\u{FF80}\u{FF76}\u{FF85}\u{3002}",
    "\u{A5}\u{203E}\u{FF5E}\u{FF0D}\u{FF3C}\u{301C}\u{2225}\u{2015}\u{2014}\u{2016}",
    "\u{7B80}\u{4F53}\u{4E2D}\u{6587}\u{4EEC}\u{8FD9}",
    "\u{7E41}\u{9AD4}\u{4E2D}\u{6587}\u{5011}\u{9019}",
    "\u{D55C}\u{AD6D}\u{C5B4} \u{6F22}\u{5B57} \u{3131}\u{314F}",
    "\u{9999}\u{6E2F} \u{CA}\u{CA}\u{304}\u{CA}\u{30C} \u{EA}\u{EA}\u{304}\u{EA}\u{30C}x",
    "Latin-1: \u{E9}\u{E0}\u{FC}\u{DF} \u{FF} \u{B1}",
    "\u{395}\u{3BB}\u{3BB}\u{3B7}\u{3BD}\u{3B9}\u{3BA}\u{3AC} \u{3AC}\u{3AD}",
    "\u{41A}\u{438}\u{440}\u{438}\u{43B}\u{43B}\u{438}\u{446}\u{430} \u{451}\u{401}",
    "\u{5E2}\u{5D1}\u{5E8}\u{5D9}\u{5EA} \u{5E9}\u{5B8}\u{5C1}\u{5DC}\u{5D5}\u{5B9}\u{5DD} \u{FB2C}\u{FB4B}\u{FB1D}",
    "Ti\u{1EBF}ng Vi\u{1EC7}t: \u{1EB3} \u{1ED1} \u{1EF1} \u{102}\u{103}",
    "\u{20AC}\u{A3}\u{A9}\u{AE}\u{2122}\u{2026}\u{2022}\u{201C}\u{201D}\u{2018}\u{2019}",
    "emoji \u{1F600} ext-B \u{2000B}\u{20089}\u{2A6B2}",
    "tag \u{E0001}\u{E006A}\u{E0061}x\u{E007F}y\u{E0041}",
    "\0\x01\x0e\x0f\x1b\x7f\n\r\t",
    "\u{E000}\u{E757}\u{F8F0}\u{F8FF}",
    "\u{304B}\u{309A}\u{30B1}\u{309A}\u{E6}\u{300}\u{254}\u{301}\u{2E9}\u{2E5}\u{2E5}\u{2E9} A\u{300}a\u{301}",
    "\u{FF21}\u{FF22}\u{FF41}\u{FF10}\u{FFE5}\u{FFE3}",
    "\u{F929}\u{FA2D}\u{FA0C}\u{FA0D}\u{2170}\u{2160}\u{2460}\u{3231}",
    "a\u{304B}\nb\u{D55C}\nc\u{4E00}\n\u{E9}\n\u{3B1}\n",
    "\u{1E00}\u{1EA0}\u{1EF9}\u{300}\u{301}\u{303}\u{309}\u{323}",
];
foreach ($texts as $t) {
    foreach ($new as $cs) {
        foreach (['', '//IGNORE', '//TRANSLIT', '//TRANSLIT//IGNORE'] as $suffix) {
            $add('UTF-8', $cs.$suffix, $t);
        }
        foreach (['', '//IGNORE'] as $suffix) {
            $r = @iconv('UTF-8', $cs.$suffix, $t);
            if ($r !== false && $r !== '') {
                $add($cs, 'UTF-8', $r);
                $add($cs, 'UTF-16LE', $r);
            }
        }
    }
}

// 3. every byte, and invalid or incomplete input around each lead-byte class
$all = implode('', array_map('chr', range(0, 255)));
foreach (array_merge($new, ['KOI8-RU', 'CP737', 'IBM1047', 'MACINTOSH']) as $cs) {
    $add($cs, 'UTF-8', $all);
    $add($cs, 'UTF-8//IGNORE', $all);
    $add($cs, 'UCS-4LE//IGNORE', $all);
}
$leads = [0x0E, 0x0F, 0x1B, 0x5C, 0x7E, 0x80, 0x81, 0x84, 0x85, 0x87, 0x88, 0x8E, 0x8F, 0x9F, 0xA0, 0xA1, 0xA4, 0xA8, 0xB0,
    0xC6, 0xC8, 0xC9, 0xD7, 0xD8, 0xDF, 0xE0, 0xEA, 0xEB, 0xED, 0xF0, 0xF8, 0xF9, 0xFA, 0xFC, 0xFD, 0xFE, 0xFF];
// (iconv_sweep.json covers every byte pair; these are the readable cases)
foreach ($mb as $cs) {
    foreach ($leads as $l) {
        $add($cs, 'UTF-8', 'a'.chr($l));
        foreach ([0x00, 0x40, 0x7F, 0xA1, 0xFF] as $t) {
            $add($cs, 'UTF-8', chr($l).chr($t));
        }
        foreach ([0x30, 0x41, 0x80, 0xFE] as $t) {
            $add($cs, 'UTF-8//IGNORE', 'a'.chr($l).chr($t).'bc');
        }
    }
}
foreach (['EUC-JP', 'EUC-JP-MS', 'EUC-JISX0213'] as $cs) {
    foreach (['', "\xA1", "\xA2", "\xB0", "\xED", "\xEE", "\xFE", "\x41", "\xA1\xA1", "\xA2\xF1", "\xB0\xA1", "\xED\xE3", "\xEE\xEC",
        "\xFE\xFE", "\xA1\x41", "\xA2\xAF", "\xA6\xE1", "\xF3\xF3", "\xA8\xA1", "\xAC\xA1", "\xEE\xA1", "\xFE\xF6"] as $tail) {
        $add($cs, 'UTF-8', "\x8F".$tail);
        $add($cs, 'UTF-8//IGNORE', "x\x8F".$tail."yz");
        $add($cs, 'UTF-8', "\x8E".$tail);
    }
}
foreach (["\x81\x30\x81\x30", "\x81\x30\x81", "\x81\x30", "\x84\x31\xA4\x39", "\x84\x31\xA5\x30", "\x8F\x39\xFE\x39",
    "\x90\x30\x81\x30", "\xE3\x32\x9A\x35", "\xE3\x32\x9A\x36", "\xFE\x39\xFE\x39", "\x81\x30\x7F\x30", "\x81\x30\x81\x3A",
    "\x81\x30\xFF\x30", "\x82\x35\x8F\x33", "\x95\x32\x82\x36", "\x81\x39\xEE\x39", "\xA8\xBF", "\xFE\x50", "\xFE\xFF",
    "\x81\x7F", "\x80", "\xFF", "\x81\x29", "\x81\x3A"] as $s) {
    $add('GB18030', 'UTF-8', $s);
    $add('GB18030', 'UTF-8//IGNORE', "a".$s."bcde");
    $add('GB18030', 'UTF-16BE', $s.$s);
}
foreach (["\x88\x62", "\x88\x64", "\x88\xA3", "\x88\xA5", "\x88\x66", "\x88\xA7", "\x88\x62\x88\x64", "\x87\x40", "\x81\x40",
    "\x88\x62a", "\xA1\x40", "\xC6\xA1", "\xC8\x40", "\xF9\xFE", "\xFE\xFE"] as $s) {
    foreach (['UTF-8', 'UTF-16LE', 'ASCII//TRANSLIT', 'ISO-8859-1//IGNORE', 'BIG5-HKSCS', 'BIG5'] as $to) {
        $add('BIG5-HKSCS', $to, $s);
    }
}

// 4. stateful decoders
$jpIn = ["\x1b\$B0l\x1b(B", "\x1b\$B0l", "\x1b", "a\x1b", "\x1b\$", "a\x1b\$B", "\x1b\$B0", "\x1b(Z", "\x1b(Zab", "\x1b\$Aab",
    "\x1b\$(Cab", "\x1b\$(Dab", "\x1b\$(", "\x1b\$(Q", "\x1b(J\\~a", "\x1b(I12", "\x1b(I\x60\x20", "\x1b\$B\x21\x21\x0a\x21",
    "\x1b\$B\x7f\x21", "\x1b\$B\x21\x7f", "\x1b\$B\x74\x26\x74\x27", "\x1b\$B\x80\x80", "\x1b.A\x1bNi", "\x1b.F\x1bNa",
    "\x1b.F\x1bN\x7f", "\x1b.F\x1bN\x1f", "\x1bNa", "\x1b\$@0l\x1b\$B0l", "\x1b\$A0l", "\x1b\$(C0!", "\x1b\$(D+1",
    "\x1b\$(C\x21\x21\x0e", "\x1b\$(O\$w\x1b\$(P!!", "\x1b\$(Q\$w", "\x1b\$(Q\$", "\x1b\$(Q.!", "\x1b\$(P\$w",
    "\x1b\$(Q+:\x1b\$(Q+e", "\x1b\$B\$+\$w", "\x1b(B\x1b\$B", "\xa4", "\x1b\$B\x1b(B\x1b", "\x0e\x0f"];
foreach (['ISO-2022-JP', 'ISO-2022-JP-2', 'ISO-2022-JP-3'] as $cs) {
    foreach ($jpIn as $s) {
        $add($cs, 'UTF-8', $s);
        $add($cs, 'UTF-8//IGNORE', $s.'z');
        $add($cs, 'UTF-16LE', $s);
    }
}
foreach (["\x1b\$)C", "\x1b\$)Ca\x0e0!\x0fb", "\x0e0!", "\x0e0", "\x0e0!0", "\x0e\x0a\x0f", "\x0e\x0ax", "\x1b", "\x1b\$",
    "\x1b\$)", "\x1b\$)C\x1b\$)C", "\x1b\$)Dab", "\x1b(Bab", "\xa1\xa1", "a\x80", "\x0e!!\x0f", "\x0e\x7e\x21", "\x0e\x49\x21",
    "\x0e\x30\x7f", "\x0e\x1b\$)C0!", "\x0f\x0f\x0e\x0e0!"] as $s) {
    $add('ISO-2022-KR', 'UTF-8', $s);
    $add('ISO-2022-KR', 'UTF-8//IGNORE', $s.'z');
    $add('ISO-2022-KR', 'EUC-KR', $s);
}
foreach (['CP1255', 'CP1258', 'TCVN5712-1'] as $cs) {
    $b = [];
    for ($i = 0; $i < 256; $i++) {
        $b[] = chr($i);
    }
    foreach (['A', "A\xCC", "a\xEC", "\xC3\xDE", "\xC3\xD2", "Ab\xCC", "A\xCC\xCC", "\xE0\xC7", "\xF9\xD1\xCC", "\xF9\xCC\xD1",
        "\xF9\xCC", "\xE9\xC4", "\xD6\xC7", "\xE0", "\xE0\xE1", "\xE0\xC4\xC7", "\xE0\xCA", "\xE0\xDB", "A\xFF", "\xFFA",
        "A\x0a", "\x41\xEC\xEC", "\x01\xB0", "\xA1\xB3", "\xC3\xB3", "aA\xB3\xB4"] as $s) {
        foreach (['UTF-8', 'UTF-8//IGNORE', 'ASCII', 'ASCII//IGNORE', 'ASCII//TRANSLIT', 'ISO-8859-1//TRANSLIT', $cs,
            'UTF-16'] as $to) {
            $add($cs, $to, $s);
        }
    }
    for ($i = 0; $i < 20; $i++) {
        $add($cs, 'UTF-8//IGNORE', rand_str("A\x41\x61\x45\xC3\xCC\xEC\xDE\xD2\xF2\xE0\xE9\xF9\xC4\xC7\xD1\xCC\xFF\x0a ", 1, 12));
    }
}

// 5. stateful and composing encoders
$jpOut = ["a\u{65E5}b", "\u{65E5}\n\u{65E5}", "\u{65E5}", "\u{FF76}", "\u{A5}\u{203E}a", "\u{A5}\u{65E5}\u{A5}",
    "\u{E9}\u{3B1}\u{E9}\u{3AC}", "\u{E9}\n\u{E9}", "\u{3AC}\n\u{3AC}", "\u{D55C}a\u{D55C}", "\u{4EEC}\u{65E5}\u{4EEC}",
    "\u{E0001}\u{E006B}\u{E006F}\u{4E00}\u{D55C}\u{4E00}", "\u{E0001}\u{E007A}\u{E0068}\u{4E00}\u{65E5}x",
    "\u{E0001}\u{E006A}\u{E0061}\u{4E00}\u{D55C}", "\u{E0001}\u{E006A}\u{E0062}\u{4E00}", "\u{E0001}\u{E007F}\u{4E00}",
    "\u{E0001}\u{E004B}\u{E004F}\u{D55C}\u{3B1}", "\x1b\x0e\x0f", "\u{304B}\u{309A}", "\u{304B}\u{309A}\u{304B}",
    "\u{304B}x", "\u{304B}", "\u{E6}\u{300}", "\u{E6}x", "\u{E6}", "\u{2E9}\u{2E5}", "\u{2E5}\u{2E9}\u{2E9}",
    "a\u{304B}\u{65E5}\u{309A}", "\u{4FF1}\u{4FF1}", "\u{20089}\u{4E00}", "\u{4E00}\u{20089}a", "\u{FF76}\u{FF61}a",
    "\u{2000B}\u{304B}\u{309A}", "\u{304B}\u{1F600}", "\u{304B}\u{20AC}", "\u{6F22}\u{D55C}\u{AC00}", "\u{3231}\u{2160}",
    "\u{E0041}a\u{E0041}", "\u{20AC}\u{1F600}x"];
foreach ($iso2022 as $cs) {
    foreach ($jpOut as $s) {
        foreach (['', '//IGNORE', '//TRANSLIT', '//IGNORE//TRANSLIT'] as $suffix) {
            $add('UTF-8', $cs.$suffix, $s);
        }
    }
    $add('UTF-8', $cs, '');
    $add('UTF-8', $cs.'//TRANSLIT', "\u{1E9E}a");
    $add('UTF-8', $cs.'//TRANSLIT', "\u{65E5}\u{1E9E}\u{65E5}");
    $add('UTF-8', $cs.'//TRANSLIT', "\u{D55C}\u{FB01}\u{D55C}");
    $add('UTF-8', $cs.'//IGNORE', "\xFF\u{65E5}");
    $add('UTF-8', $cs, "\u{65E5}\xE6\x97");
}
foreach (['BIG5-HKSCS', 'EUC-JISX0213', 'SHIFT_JISX0213'] as $cs) {
    foreach (["\u{CA}", "\u{CA}\u{304}", "\u{CA}\u{30C}x", "\u{CA}\u{CA}\u{304}", "\u{EA}\u{20AC}", "\u{EA}\u{1F600}",
        "\u{CA}\u{E0041}", "\u{CA}\xFF", "\u{304B}\u{309A}", "\u{304B}\u{304B}\u{309A}\u{309A}", "\u{E6}\u{301}",
        "\u{254}\u{300}\u{254}\u{301}", "\u{2E9}\u{2E5}\u{2E9}", "\u{30C8}\u{309A}", "\u{31F7}\u{309A}", "\u{304B}\u{FB01}",
        "\u{304B}", "\u{304B}\u{3099}"] as $s) {
        foreach (['', '//IGNORE', '//TRANSLIT'] as $suffix) {
            $add('UTF-8', $cs.$suffix, $s);
        }
    }
}
foreach ($composing as $cs) {
    foreach (["\u{FB2C}", "\u{FB2C}\u{FB2D}a", "\u{FB1D}\u{FB4E}", "\u{5E9}\u{5BC}\u{5C1}", "\u{1EA0}\u{1EF9}\u{1EA1}",
        "\u{102}\u{300}", "\u{C0}\u{C1}\u{C3}\u{1EA2}\u{1EA0}", "\u{E0}\u{301}", "\u{1E9E}\u{20AC}", "\u{1F600}a",
        "a\u{E0041}b", "\u{FB30}\u{FB31}\u{FB32}", "\u{1EDB}\u{1EDD}\u{1EDF}\u{1EE1}\u{1EE3}", "\u{1B0}\u{1AF}\u{1A1}\u{1A0}",
        "\u{110}\u{111}\u{20AB}"] as $s) {
        foreach (['', '//IGNORE', '//TRANSLIT', '//TRANSLIT//IGNORE'] as $suffix) {
            $add('UTF-8', $cs.$suffix, $s);
        }
    }
}

// 5b. code points beyond the BMP and beyond Unicode through every encoder
foreach ($new as $cs) {
    foreach ([0x10000, 0x1F600, 0x20089, 0x2A6D6, 0x2F800, 0x30000, 0xE0001, 0xE0041, 0xE007F, 0xE0080, 0xF0000,
        0x10FFFF, 0x110000, 0x7FFFFFFF, 0xD800, 0xFFFE, 0xFFFF] as $cp) {
        foreach (['', '//IGNORE', '//TRANSLIT'] as $suffix) {
            $add('UCS-4', $cs.$suffix, pack('N3', 0x41, $cp, 0x42));
        }
    }
}

// 6. every single-byte module: all bytes, an invalid byte, encodings of a few characters
$sbcsModules = ['ARMSCII-8', 'ASMO_449', 'BRF', 'BS_4730', 'CP10007', 'CP1125', 'CP737', 'CP770', 'CP771', 'CP772', 'CP773',
    'CP774', 'CP775', 'CSA_Z243.4-1985-1', 'CSA_Z243.4-1985-2', 'CSN_369103', 'CWI', 'DEC-MCS', 'DIN_66003', 'DS_2089',
    'EBCDIC-AT-DE-A', 'EBCDIC-AT-DE', 'EBCDIC-CA-FR', 'EBCDIC-DK-NO-A', 'EBCDIC-DK-NO', 'EBCDIC-ES-A', 'EBCDIC-ES-S',
    'EBCDIC-ES', 'EBCDIC-FI-SE-A', 'EBCDIC-FI-SE', 'EBCDIC-FR', 'EBCDIC-IS-FRISS', 'EBCDIC-IT', 'EBCDIC-PT', 'EBCDIC-UK',
    'EBCDIC-US', 'ECMA-CYRILLIC', 'ES', 'ES2', 'GB_1988-80', 'GEORGIAN-ACADEMY', 'GEORGIAN-PS', 'GOST_19768-74',
    'GREEK-CCITT', 'GREEK7-OLD', 'GREEK7', 'HP-GREEK8', 'HP-ROMAN8', 'HP-ROMAN9', 'HP-THAI8', 'HP-TURKISH8', 'IBM037',
    'IBM038', 'IBM1004', 'IBM1008', 'IBM1025', 'IBM1026', 'IBM1046', 'IBM1047', 'IBM1097', 'IBM1112', 'IBM1122', 'IBM1123',
    'IBM1124', 'IBM1129', 'IBM1130', 'IBM1132', 'IBM1133', 'IBM1137', 'IBM1140', 'IBM1141', 'IBM1142', 'IBM1143',
    'IBM1144', 'IBM1145', 'IBM1146', 'IBM1147', 'IBM1148', 'IBM1149', 'IBM1153', 'IBM1154', 'IBM1155', 'IBM1156',
    'IBM1157', 'IBM1158', 'IBM1160', 'IBM1161', 'IBM1162', 'IBM1163', 'IBM1164', 'IBM1166', 'IBM1167', 'IBM12712',
    'IBM16804', 'IBM256', 'IBM273', 'IBM274', 'IBM275', 'IBM277', 'IBM278', 'IBM280', 'IBM281', 'IBM284', 'IBM285',
    'IBM290', 'IBM297', 'IBM420', 'IBM423', 'IBM424', 'IBM4517', 'IBM4899', 'IBM4909', 'IBM4971', 'IBM500', 'IBM5347',
    'IBM803', 'IBM851', 'IBM852', 'IBM855', 'IBM856', 'IBM857', 'IBM858', 'IBM860', 'IBM861', 'IBM862', 'IBM863',
    'IBM864', 'IBM865', 'IBM866NAV', 'IBM868', 'IBM869', 'IBM870', 'IBM871', 'IBM874', 'IBM875', 'IBM880', 'IBM891',
    'IBM901', 'IBM902', 'IBM903', 'IBM9030', 'IBM904', 'IBM905', 'IBM9066', 'IBM918', 'IBM921', 'IBM922', 'IBM9448',
    'IEC_P27-1', 'INIS-8', 'INIS-CYRILLIC', 'INIS', 'ISIRI-3342', 'ISO-8859-9E', 'ISO-IR-197', 'ISO-IR-209',
    'ISO_10367-BOX', 'ISO_11548-1', 'ISO_2033', 'ISO_5427-EXT', 'ISO_5427', 'ISO_5428', 'IT', 'JIS_C6220-1969-RO',
    'JIS_C6229-1984-B', 'JUS_I.B1.002', 'KOI-8', 'KOI8-RU', 'KOI8-T', 'KSC5636', 'LATIN-GREEK-1', 'LATIN-GREEK',
    'MAC-CENTRALEUROPE', 'MAC-IS', 'MAC-SAMI', 'MAC-UK', 'MIK', 'MSZ_7795.3', 'NATS-DANO', 'NATS-SEFI', 'NC_NC00-10',
    'NF_Z_62-010', 'NF_Z_62-010_1973', 'NS_4551-1', 'NS_4551-2', 'PT', 'PT154', 'PT2', 'RK1048', 'SEN_850200_B',
    'SEN_850200_C', 'TIS-620', 'VISCII', 'WIN-SAMI-2'];
$sample = "Az09 \u{A3}\u{A5}\u{E9}\u{C4}\u{F6}\u{20AC}\u{416}\u{3B1}\u{5D0}\u{627}\u{E01}\u{10D0}\u{2800}\u{2591}\u{E0041}\u{1F600}~\\[]{}@#\$";
foreach ($sbcsModules as $cs) {
    $add($cs, 'UTF-8', $all);
    $add($cs, 'UTF-8//IGNORE', $all);
    foreach (['', '//IGNORE', '//TRANSLIT'] as $suffix) {
        $add('UTF-8', $cs.$suffix, $sample);
    }
    $r = @iconv('UTF-8', $cs.'//IGNORE', $sample);
    if ($r !== false) {
        $add($cs, 'UTF-8', $r);
    }
}

// 6b. the direct modules (GB2312/BIG5, GBK/GB2312, IBM1008/IBM420)
foreach ([['GB2312', 'BIG5'], ['BIG5', 'GB2312'], ['GBK', 'GB2312'], ['GB2312', 'GBK'], ['CP936', 'EUC-CN'],
    ['EUC-CN', 'BIG-5'], ['IBM1008', 'IBM420'], ['IBM420', 'IBM1008']] as [$from, $to]) {
    foreach (["a\xB0\xA1b", "\xB0\xA1", "\xB0\xA1a", "\xA1\xA1\xD7\xFAx", "\xD7\xFA", "\x81\x40xy", "\xA8\x44z", "\xFF\xFFa\x80b",
        "\xC6\xA1a\xA1\x40", "\xA1\xFF\xA1", "\xF7\xFE\xF6\xFFq", "\xF9\xFE\xF9\xD5r", "\xA2\xA1\xA6\xE0\xA8\xBBs", "\x80\x80t",
        "\xB0", "a\xB0", $all] as $s) {
        foreach (['', '//IGNORE', '//TRANSLIT'] as $suffix) {
            $add($from, $to.$suffix, $s);
        }
    }
}

// 7. random strings
$alphabets = [
    'mb' => "aZ\x00\x7F\x80\x81\x8E\x8F\x9F\xA0\xA1\xA4\xB0\xC6\xC8\xC9\xDF\xE0\xEA\xF9\xFC\xFE\xFF\x30\x39\x40\x7E\xA1\xFE\x62",
    'jp' => "\x1b\$B(J)I@A.FN0l!\x21\x7E\x0a\x0e\x0fab\x80\xA4",
    'kr' => "\x1b\$)C\x0e\x0f0!\x21\x7E\x0a\x49ab\x80",
];
$utf8Pool = ["a", "\n", "\u{65E5}", "\u{304B}", "\u{309A}", "\u{FF76}", "\u{A5}", "\u{203E}", "\u{E9}", "\u{3B1}", "\u{D55C}",
    "\u{4EEC}", "\u{20AC}", "\u{1F600}", "\u{E0041}", "\u{CA}", "\u{304}", "\u{E6}", "\u{300}", "\u{2000B}", "\xFF", "\u{FB2C}",
    "\u{1EA0}", "\u{5E9}", "\u{5BC}", "\u{102}"];
for ($k = 0; $k < 2500; $k++) {
    $cs = $new[mt_rand(0, count($new) - 1)];
    $suffix = ['', '', '//IGNORE', '//TRANSLIT', '//IGNORE//TRANSLIT'][mt_rand(0, 4)];
    if (mt_rand(0, 1) === 0) {
        $alpha = in_array($cs, ['ISO-2022-JP', 'ISO-2022-JP-2', 'ISO-2022-JP-3'], true) ? $alphabets['jp']
            : ($cs === 'ISO-2022-KR' ? $alphabets['kr'] : $alphabets['mb']);
        $add($cs, 'UTF-8'.($suffix === '//TRANSLIT' ? '' : $suffix), rand_str($alpha, 0, 16));
    } else {
        $add('UTF-8', $cs.$suffix, rand_pieces($utf8Pool, 0, 10));
    }
}

// 7b. random text between two of the charsets (stateful decoders with stateful encoders, flushing)
$crossPool = array_merge($utf8Pool, ["\u{4E00}", "\u{AC00}", "\u{3042}", "\u{30AB}\u{309A}", "\u{1EA0}", "b", "\u{A0}"]);
$targets = array_merge($new, ['UTF-16', 'ASCII', 'ISO-8859-1', 'KOI8-RU', 'UTF-8']);
for ($k = 0; $k < 1500; $k++) {
    $from = $new[mt_rand(0, count($new) - 1)];
    $to = $targets[mt_rand(0, count($targets) - 1)];
    $s = @iconv('UTF-8', $from.'//IGNORE', rand_pieces($crossPool, 1, 12));
    if ($s === false) {
        continue;
    }
    if (mt_rand(0, 4) === 0 && $s !== '') {
        $s = substr($s, 0, mt_rand(0, strlen($s) - 1)); // cut inside a sequence or shift state
    }
    $add($from, $to.['', '//IGNORE', '//TRANSLIT', '//TRANSLIT//IGNORE'][mt_rand(0, 3)], $s);
}

// 8. long inputs (section "long", input = prefix repeated n times + tail)
$long = [];
foreach ([8159, 8160, 8161, 16319, 16320, 16321] as $n) {
    $long[] = ['BIG5-HKSCS', 'UTF-8', "\x88\x62", $n, "a"];
    $long[] = ['BIG5-HKSCS', 'UTF-16LE', "a", $n, "\x88\x62\x88\x64"];
    $long[] = ['BIG5-HKSCS', 'ASCII//IGNORE', "\x88\x62", $n, "ab"];
    $long[] = ['EUC-JISX0213', 'UTF-8', "\xA4\xF7", $n, "a"];
    $long[] = ['SHIFT_JISX0213', 'UTF-16', "a", $n, "\x82\xF5\x82\xF5"];
    $long[] = ['ISO-2022-JP-3', 'UTF-8', "\x1b\$(Q\$w", intdiv($n, 2), "a"];
    $long[] = ['CP1258', 'UTF-8', "A\xEC", $n, "B"];
    $long[] = ['CP1258', 'UTF-8', "a", $n, "A\xEC"];
    $long[] = ['CP1258', 'ASCII//IGNORE', "\xC3", $n, "x"];
    $long[] = ['CP1255', 'UTF-8', "\xF9\xCC\xD1", $n, "z"];
    $long[] = ['ISO-2022-JP', 'UTF-8', "\x1b\$B0l\x1b(Ba", $n, "\x1b\$B"];
    $long[] = ['UTF-8', 'ISO-2022-JP', "\u{65E5}a", $n, "\u{65E5}"];
    $long[] = ['UTF-8', 'ISO-2022-JP//IGNORE', "\u{65E5}", $n, "\u{1F600}\u{65E5}\u{20AC}a"];
    $long[] = ['UTF-8', 'ISO-2022-KR//IGNORE', "\u{D55C}a", $n, "\u{1F600}\u{D55C}"];
    $long[] = ['UTF-8', 'EUC-JP//IGNORE', "a", $n, "\u{1F600}b\u{1F600}c"];
    $long[] = ['UTF-8', 'SJIS//IGNORE', "\u{65E5}", $n, "\u{20AC}\u{65E5}x"];
    $long[] = ['UTF-8', 'BIG5-HKSCS', "\u{CA}", $n, "\u{304}"];
    $long[] = ['UTF-8', 'BIG5-HKSCS//IGNORE', "\u{CA}x", $n, "\u{1F600}\u{CA}"];
    $long[] = ['CP932', 'UTF-8//IGNORE', "a", $n, "\x81\x20\x81\x20b"];
    $long[] = ['EUC-KR', 'UTF-8//IGNORE', "\xB0\xA1", $n, "\xA1\x41\xB0\xA1"];
    $long[] = ['GB18030', 'UTF-8//IGNORE', "\x81\x30\x81\x30", intdiv($n, 2), "\x81\x30\xFF\x30z"];
}
foreach ([10, 100, 1000, 5000] as $n) {
    $long[] = ['ISO-8859-1', 'EUC-JP', "\xE9", $n, "a"];
    $long[] = ['ISO-8859-1', 'EUC-JP', "a\xE9", $n, ""];
    $long[] = ['ISO-8859-1', 'ISO-2022-JP-2', "\xE9", $n, "a"];
    $long[] = ['ISO-8859-1', 'ISO-2022-JP-2', "a\xE9", $n, "b"];
    $long[] = ['ISO-8859-1', 'GBK', "\xB0", $n + 1, ""];
    $long[] = ['ISO-8859-1', 'GB18030', "\xE9a", $n, ""];
    $long[] = ['ISO-8859-1', 'ISO-2022-KR', "a", $n, ""];
    $long[] = ['KOI8-R', 'ISO-2022-JP', "\xC1a", $n, ""];
    $long[] = ['UTF-8', 'ISO-2022-JP-3', "a\u{304B}", $n, "\u{309A}"];
    $long[] = ['UTF-8', 'CP1258', "\u{1EA0}", $n, "x"];
    $long[] = ['UTF-8', 'CP1255', "\u{FB2C}", $n, ""];
    $long[] = ['UTF-8', 'ISO-2022-JP//TRANSLIT', "a\u{65E5}", $n, "\u{1E9E}"];
}
// the output buffer runs full while an encoder holds a buffered character or between escape sequences
foreach ([37, 38, 39, 64, 65, 66, 101, 1000, 1001] as $n) {
    $long[] = ['ISO-8859-1', 'ISO-2022-JP-3', "\xE6", $n, ""];
    $long[] = ['ISO-8859-1', 'ISO-2022-JP-3', "\xE6a", $n, "\xE6"];
    $long[] = ['ISO-8859-1', 'ISO-2022-JP-3//TRANSLIT', "\xE6\xA4", $n, ""];
    $long[] = ['ISO-8859-1', 'EUC-JISX0213', "\xE6", $n, ""];
    $long[] = ['ISO-8859-1', 'SHIFT_JISX0213', "\xE6\xB0", $n, ""];
    $long[] = ['ISO-8859-1', 'BIG5-HKSCS', "\xCA", $n, ""];
    $long[] = ['ISO-8859-1', 'BIG5-HKSCS//IGNORE', "\xCA\xE6", $n, "\xEA"];
    $long[] = ['ISO-8859-1', 'ISO-2022-JP-2', "\xE9\xA5", $n, ""];
    $long[] = ['ISO-8859-1', 'ISO-2022-KR', "a\xA1", $n, ""];
    $long[] = ['ISO-8859-1', 'TCVN5712-1', "\xC0\xC1\xC3", $n, ""];
    $long[] = ['ISO-8859-1', 'EUC-JP//TRANSLIT', "\xE9\xA4", $n, ""];
    $long[] = ['ISO-8859-1', 'GB18030', "\xB7", $n, "a"];
    $long[] = ['UCS-2LE', 'ISO-2022-JP', "\x00\x4E\x41\x00", $n, ""];
}
$longRows = [];
foreach ($long as [$from, $to, $prefix, $n, $tail]) {
    [$r, $d] = capture(static fn () => iconv($from, $to, str_repeat($prefix, $n).$tail));
    $longRows[] = ['from' => $from, 'to' => $to, 'prefix' => bin2hex($prefix), 'n' => $n, 'tail' => bin2hex($tail),
        'r' => is_string($r) ? ['len' => strlen($r), 'fnv' => hash('fnv1a64', $r)] : ['false' => true], 'd' => $d];
}

emit('iconv_charsets.gen.php', ['cases' => run_cases($cases), 'long' => $longRows]);
