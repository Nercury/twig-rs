<?php
// codepages_fuzz.json: seeded random conversions between the codepages and every other charset charset_basic
// converts: names and aliases in random case, suffix variants, inputs built from valid characters (with the letters
// and marks CP1255 and CP1258 combine), invalid bytes, raw bytes and long runs (so that rounds, E2BIG, PHP's //IGNORE
// byte skipping and held characters interact). Inputs are recipes (see common.inc.php); outputs longer than 256 bytes
// are [length, fnv1a64].

require __DIR__ . '/pairs.inc.php';

// arguments (exploratory rounds): seed and number of cases; codepages_fuzz.json uses the defaults
mt_srand((int) ($argv[1] ?? 1127));
$count = (int) ($argv[2] ?? 8000);

$aliases = [
    'ISO-8859-2' => ['latin2', 'ISO_8859-2:1987', 'ibm912'], 'ISO-8859-3' => ['L3', 'iso-ir-109'],
    'ISO-8859-4' => ['latin4', 'ISO88594'], 'ISO-8859-5' => ['cyrillic', 'ISO_8859-5'], 'ISO-8859-6' => ['arabic', 'ASMO-708'],
    'ISO-8859-7' => ['greek', 'ELOT_928', 'iso_8859-7:2003'], 'ISO-8859-8' => ['hebrew', 'cp916'],
    'ISO-8859-9' => ['latin5', 'TS-5881'], 'ISO-8859-10' => ['L6', 'latin6'], 'ISO-8859-11' => ['thai8', 'HP-THAI8'],
    'ISO-8859-13' => ['baltic', 'LATIN7', 'IBM-921'], 'ISO-8859-14' => ['iso-celtic', 'l8'],
    'ISO-8859-15' => ['latin-9', 'ISO_8859-15:1998'], 'ISO-8859-16' => ['latin10', 'ISO-IR-226'],
    'CP1250' => ['windows-1250', 'MS-EE'], 'CP1251' => ['windows-1251', 'ms-cyrl'], 'CP1252' => ['WINDOWS-1252', 'ms-ansi'],
    'CP1253' => ['windows-1253', 'MS-GREEK'], 'CP1254' => ['windows-1254', 'ms-turk'], 'CP1255' => ['windows-1255', 'MS-HEBR'],
    'CP1256' => ['windows-1256', 'ms-arab'], 'CP1257' => ['windows-1257', 'WINBALTRIM'], 'CP1258' => ['windows-1258', 'cp1258'],
    'KOI8-R' => ['koi8r', 'CSKOI8R'], 'KOI8-U' => ['koi8u'], 'KOI8-RU' => ['ibm1167', 'CP1167'],
    'CP437' => ['IBM437', '437', 'cspc8codepage437'], 'CP850' => ['ibm850', '850'], 'CP852' => ['IBM852', 'cspcp852'],
    'CP866' => ['ibm866', '866'], 'MACINTOSH' => ['mac', 'CSMACINTOSH'],
    'UTF-8' => ['utf8', ''], 'UTF-16' => ['utf16'], 'UTF-16LE' => ['UTF16LE'], 'UTF-32BE' => ['utf32be'],
    'UCS-2' => ['ucs2'], 'UCS-4' => ['ucs4'], 'ASCII' => ['us-ascii'], 'ISO-8859-1' => ['latin1', 'cp819'],
];
$others = ['UTF-8', 'UTF-16', 'UTF-16LE', 'UTF-32BE', 'UCS-2', 'UCS-4', 'ASCII', 'ISO-8859-1'];
$suffixes = ['', '', '//TRANSLIT', '//IGNORE', '//IGNORE', '//TRANSLIT//IGNORE', '//IGNORE//TRANSLIT', '//ignore',
    '//translit', '//TRANSLIT,IGNORE', '//IGNORE//', '/'];
$specials = [0xE9, 0x20AC, 0xBD, 0x2026, 0x1F600, 0x4E00, 0x200B, 0xFEFF, 0x10FFFF, 0x339F, 0x20BD, 0x1F00, 0x0401,
    0x05D0, 0x05E9, 0x05BC, 0x05C1, 0xFB2C, 0xFB49, 0x00C3, 0x1EA0, 0x0300, 0x0301, 0x0303, 0x0340, 0xE0041, 0xA0, 0x00];
$combining = ['CP1258' => ["A", "a", "F", "\xc2", "\xd5", "\xcc", "\xec", "\xde", "\xd2", "\xf2", "1", "\x80"],
    'CP1255' => ["\xf9", "\xe0", "\xe9", "\xf2", "\xcc", "\xd1", "\xd2", "\xc7", "\xc4", "A", "\x80"]];

/** The bytes charset $cs decodes on their own. */
function valid_bytes(string $cs): array
{
    static $cache = [];
    if (!isset($cache[$cs])) {
        $cache[$cs] = [];
        for ($b = 0; $b < 256; $b++) {
            $GLOBALS['__diags'] = [];
            if (iconv($cs, 'UCS-4BE', chr($b)) !== false) {
                $cache[$cs][] = $b;
            }
        }
    }
    return $cache[$cs];
}

/** $cps in $cs (codepages through iconv(), the others as pairs.inc.php builds them), null when not representable. */
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
    // one side at least is a codepage
    $fromCs = mt_rand(0, 2) > 0 ? CODEPAGES[mt_rand(0, count(CODEPAGES) - 1)] : $others[mt_rand(0, count($others) - 1)];
    $toCs = !in_array($fromCs, CODEPAGES, true) || mt_rand(0, 1) === 0
        ? CODEPAGES[mt_rand(0, count(CODEPAGES) - 1)] : $others[mt_rand(0, count($others) - 1)];
    $pick = static fn (string $cs): string => mt_rand(0, 2) === 0 && isset($aliases[$cs])
        ? $aliases[$cs][mt_rand(0, count($aliases[$cs]) - 1)] : $cs;
    $from = $pick($fromCs);
    $to = $pick($toCs) . $suffixes[mt_rand(0, count($suffixes) - 1)];
    if (mt_rand(0, 3) === 0) {
        $from = strtolower($from);
    }
    $long = mt_rand(0, 9) === 0;
    $isCodepage = in_array($fromCs, CODEPAGES, true);
    $segments = [];
    for ($n = mt_rand(1, $long ? 8 : 6); $n > 0; $n--) {
        $r = mt_rand(0, 99);
        if ($r < 40) {
            if ($isCodepage && mt_rand(0, 1) === 0) {
                $valid = valid_bytes($fromCs);
                $s = '';
                for ($k = mt_rand(1, 8); $k > 0; $k--) {
                    $s .= isset($combining[$fromCs]) && mt_rand(0, 1) === 0
                        ? $combining[$fromCs][mt_rand(0, count($combining[$fromCs]) - 1)]
                        : chr($valid[mt_rand(0, count($valid) - 1)]);
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
            $segments[] = $long && mt_rand(0, 2) === 0 ? ['rep', bin2hex($s), mt_rand(100, 9000)] : ['hex', bin2hex($s)];
        } elseif ($r < 60) {
            $invalid = $isCodepage ? array_values(array_diff(range(0, 255), valid_bytes($fromCs))) : [];
            $segments[] = ['hex', $invalid !== [] ? sprintf('%02x', $invalid[mt_rand(0, count($invalid) - 1)]) : 'ff'];
        } elseif ($r < 80) {
            $s = '';
            for ($k = mt_rand(1, 6); $k > 0; $k--) {
                $s .= chr(mt_rand(0, 255));
            }
            $segments[] = ['hex', bin2hex($s)];
        } else {
            $unit = $isCodepage ? chr(valid_bytes($fromCs)[mt_rand(0, count(valid_bytes($fromCs)) - 1)])
                : (string) encode_any($fromCs, [mt_rand(0x20, 0x7E)]);
            if ($fromCs === 'UTF-16' || $fromCs === 'UTF-32') {
                $unit = substr($unit, $fromCs === 'UTF-16' ? 2 : 4);
            }
            $segments[] = ['rep', bin2hex($unit), $long ? mt_rand(1000, 12000) : mt_rand(1, 40)];
        }
    }
    $input = recipe($segments);
    $cases[] = [name_index($from), name_index($to), $segments, convert($from, $to, $input, strlen($input) > 256)];
}
emit('codepages_fuzz.gen.php', $cases);
