<?php
// multibytes_fuzz.json: seeded random conversions between the multi-byte charsets (multibyte.rs) and every other
// charset charset_basic converts: names and aliases in random case, suffix variants, inputs built from valid
// characters of every kind of sequence (with BIG5-HKSCS's pairs and the characters its encoder holds back), invalid
// and cut-off sequences, raw bytes and long runs (so that rounds, E2BIG, PHP's //IGNORE byte skipping and the
// characters held between calls interact). The conversions glibc does with modules of their own (GB 2312 and GBK or
// BIG5) are left out. Inputs are recipes (see common.inc.php); outputs longer than 256 bytes are [length, fnv1a64].

require __DIR__ . '/pairs.inc.php';

// arguments (exploratory rounds): seed and number of cases; multibytes_fuzz.json uses the defaults
mt_srand((int) ($argv[1] ?? 1128));
$count = (int) ($argv[2] ?? 8000);

$aliases = [
    'SJIS' => ['shift_jis', 'MS_KANJI', 'csShiftJIS'], 'CP932' => ['windows-31j', 'MS932', 'sjis-win'],
    'EUC-JP' => ['eucjp', 'UJIS', 'csEUCPkdFmtJapanese'], 'EUC-JP-MS' => ['eucjp-ms', 'EUCJP-WIN'],
    'GBK' => ['cp936', 'ms936', 'windows-936'], 'GB18030' => ['gb18030'], 'EUC-CN' => ['gb2312', 'EUCCN', 'csGB2312'],
    'BIG5' => ['big-5', 'CP950', 'cn-big5'], 'BIG5-HKSCS' => ['big5hkscs'], 'EUC-KR' => ['euckr', 'csEUCKR'],
    'UHC' => ['cp949', 'msCP949'], 'JOHAB' => ['cp1361'], 'EUC-TW' => ['euctw', 'OSF0005000A'],
    'UTF-8' => ['utf8', ''], 'UTF-16' => ['utf16'], 'UCS-2' => ['ucs2'], 'ISO-8859-1' => ['latin1'], 'CP1252' => ['windows-1252'],
];
$others = ['UTF-8', 'UTF-16', 'UTF-16LE', 'UTF-32BE', 'UCS-2', 'UCS-4', 'ASCII', 'ISO-8859-1', 'CP1252', 'CP1258'];
$direct = static fn (string $a, string $b): bool => ($a === 'EUC-CN' && ($b === 'GBK' || $b === 'BIG5'))
    || ($b === 'EUC-CN' && ($a === 'GBK' || $a === 'BIG5'));
$suffixes = ['', '', '//TRANSLIT', '//IGNORE', '//IGNORE', '//TRANSLIT//IGNORE', '//IGNORE//TRANSLIT', '//ignore',
    '//translit', '//TRANSLIT,IGNORE', '//IGNORE//', '/'];
$specials = [0xE9, 0x20AC, 0xA5, 0x203E, 0x5C, 0x7E, 0xFF5E, 0x301C, 0x2014, 0x2015, 0x2212, 0xFF0D, 0xA2, 0xFFE0, 0x3042,
    0xFF61, 0x4E00, 0x4E02, 0xAC00, 0x3131, 0x20A9, 0x2460, 0x2160, 0x3300, 0xF900, 0xFA6C, 0xCA, 0xEA, 0x304, 0x30C,
    0xE000, 0x80, 0x1F600, 0x20089, 0xE5E5, 0xFFFD, 0x200B, 0xFEFF, 0xE0041, 0x10FFFF, 0x00];
$extra = [
    'BIG5-HKSCS' => ["\x88\x62", "\x88\x64", "\x88\xa3", "\x88\xa5", "\x88\x66", "\x88\xa7"],
    'UHC' => ["\xa2\xe8", "\xa2\xe7"], 'EUC-JP-MS' => ["\x8e\xe0", "\x8e\xff"], 'GB18030' => ["\x81\x30\x81\x30", "\x84\x31\xa4\x39"],
    'EUC-TW' => ["\x8e\xa2\xa1\xa1", "\x8e\xa1\xa4\xa1"], 'EUC-JP' => ["\x8f\xb0\xa1", "\x8e\xb1"],
];

/** Valid sequences of a multi-byte charset (cached): single bytes and a sample of longer ones. */
function mb_valid(string $cs): array
{
    static $cache = [];
    if (!isset($cache[$cs])) {
        $seqs = [];
        for ($b = 0; $b < 256; $b++) {
            $GLOBALS['__diags'] = [];
            if (iconv($cs, 'UCS-4BE', chr($b)) !== false) {
                $seqs[] = chr($b);
            }
        }
        for ($cp = 0x80; $cp < 0x30000; $cp += $cp < 0x10000 ? 3 : 97) {
            $GLOBALS['__diags'] = [];
            $r = iconv('UCS-4BE', $cs, pack('N', $cp));
            if ($r !== false && strlen($r) > 1) {
                $seqs[] = $r;
            }
        }
        $cache[$cs] = $seqs;
    }
    return $cache[$cs];
}

/** $cps in $cs, null when not representable. */
function encode_any(string $cs, array $cps): ?string
{
    if (in_array($cs, CHARSETS, true)) {
        return encode_in($cs, array_values(array_filter($cps, static fn (int $c): bool => representable($cs, $c))));
    }
    $GLOBALS['__diags'] = [];
    $r = iconv('UCS-4BE', "$cs//IGNORE", ucs4(...$cps));
    return $r === false ? null : $r;
}

$cases = [];
for ($i = 0; $i < $count; $i++) {
    // one side at least is a multi-byte charset
    do {
        $fromCs = mt_rand(0, 2) > 0 ? MULTIBYTES[mt_rand(0, count(MULTIBYTES) - 1)] : $others[mt_rand(0, count($others) - 1)];
        $toCs = !in_array($fromCs, MULTIBYTES, true) || mt_rand(0, 1) === 0
            ? MULTIBYTES[mt_rand(0, count(MULTIBYTES) - 1)] : $others[mt_rand(0, count($others) - 1)];
    } while ($direct($fromCs, $toCs));
    $pick = static fn (string $cs): string => mt_rand(0, 2) === 0 && isset($aliases[$cs])
        ? $aliases[$cs][mt_rand(0, count($aliases[$cs]) - 1)] : $cs;
    $from = $pick($fromCs);
    $to = $pick($toCs) . $suffixes[mt_rand(0, count($suffixes) - 1)];
    if (mt_rand(0, 3) === 0) {
        $from = strtolower($from);
    }
    $long = mt_rand(0, 9) === 0;
    $isMb = in_array($fromCs, MULTIBYTES, true);
    $segments = [];
    for ($n = mt_rand(1, $long ? 8 : 6); $n > 0; $n--) {
        $r = mt_rand(0, 99);
        if ($r < 45) {
            if ($isMb && mt_rand(0, 1) === 0) {
                $valid = mb_valid($fromCs);
                $s = '';
                for ($k = mt_rand(1, 8); $k > 0; $k--) {
                    $s .= isset($extra[$fromCs]) && mt_rand(0, 4) === 0
                        ? $extra[$fromCs][mt_rand(0, count($extra[$fromCs]) - 1)]
                        : $valid[mt_rand(0, count($valid) - 1)];
                }
            } else {
                $cps = [];
                for ($k = mt_rand(1, 6); $k > 0; $k--) {
                    $cps[] = match (mt_rand(0, 4)) {
                        0, 1 => mt_rand(0x20, 0x7E),
                        2 => mt_rand(0x80, 0x2FF),
                        3 => mt_rand(0x300, 0xFFFD),
                        default => $specials[mt_rand(0, count($specials) - 1)],
                    };
                }
                $s = (string) encode_any($fromCs, array_values(array_filter($cps, static fn (int $c): bool => $c < 0xD800 || $c > 0xDFFF)));
            }
            if ($s === '') {
                continue;
            }
            if (($fromCs === 'UTF-16' || $fromCs === 'UTF-32') && $segments !== []) {
                $s = substr($s, $fromCs === 'UTF-16' ? 2 : 4); // the mark only at the start
            }
            $segments[] = $long && mt_rand(0, 2) === 0 ? ['rep', bin2hex($s), mt_rand(100, 6000)] : ['hex', bin2hex($s)];
        } elseif ($r < 60) {
            // a lead byte alone or with a bad trail, or an invalid byte
            $s = chr(mt_rand(0x80, 0xFF));
            if (mt_rand(0, 1) === 0) {
                $s .= chr([mt_rand(0, 0x3F), mt_rand(0x40, 0x7F), mt_rand(0x80, 0xA0), 0xFF, mt_rand(0x30, 0x39)][mt_rand(0, 4)]);
            }
            $segments[] = ['hex', bin2hex($s)];
        } elseif ($r < 80) {
            $s = '';
            for ($k = mt_rand(1, 6); $k > 0; $k--) {
                $s .= chr(mt_rand(0, 255));
            }
            $segments[] = ['hex', bin2hex($s)];
        } else {
            $unit = $isMb ? mb_valid($fromCs)[mt_rand(0, count(mb_valid($fromCs)) - 1)]
                : (string) encode_any($fromCs, [mt_rand(0x20, 0x7E)]);
            if ($fromCs === 'UTF-16' || $fromCs === 'UTF-32') {
                $unit = substr($unit, $fromCs === 'UTF-16' ? 2 : 4);
            }
            $segments[] = ['rep', bin2hex($unit), $long ? mt_rand(1000, 12000) : mt_rand(1, 40)];
        }
    }
    $input = recipe($segments);
    if ($fromCs === 'UHC' && str_contains(substr($input, -3), "\xa2\xe8")) {
        // A2 E8 near the end under //IGNORE: glibc reads past the input (what PHP shows depends on the memory there)
        $i--;
        continue;
    }
    $cases[] = [name_index($from), name_index($to), $segments, convert($from, $to, $input, strlen($input) > 256)];
}
emit('multibytes_fuzz.gen.php', $cases);
