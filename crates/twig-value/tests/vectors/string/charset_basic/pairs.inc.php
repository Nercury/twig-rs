<?php
// Shared by pairs.gen.php and pairs_ignore.gen.php: every source/target pair of the charsets charset_basic converts
// (under their canonical names and the UCS-2LE/UCS-4BE aliases), each target with a set of suffixes, on a pool of
// inputs per source charset: edge characters, transliteration samples, values beyond Unicode, invalid and truncated
// sequences, byte order marks, and seeded random byte strings and character sequences.

require __DIR__ . '/common.inc.php';

/** The code points a charset can hold (what the oracle shows; used to build valid random inputs). */
function representable(string $cs, int $cp): bool
{
    $surrogate = $cp >= 0xD800 && $cp <= 0xDFFF;
    return match ($cs) {
        'ASCII' => $cp < 0x80,
        'ISO-8859-1' => $cp < 0x100,
        'UCS-2', 'UCS-2BE', 'UCS-2LE' => $cp < 0x10000 && !$surrogate,
        'UTF-16', 'UTF-16BE', 'UTF-16LE', 'UTF-32', 'UTF-32BE', 'UTF-32LE' => $cp <= 0x10FFFF && !$surrogate,
        'UTF-8' => !$surrogate,
        default => true, // UCS-4
    };
}

/** $cps in charset $cs (null when a code point cannot be represented). */
function encode_in(string $cs, array $cps): ?string
{
    foreach ($cps as $cp) {
        if (!representable($cs, $cp)) {
            return null;
        }
    }
    if ($cs === 'UTF-8') {
        return implode('', array_map('utf8', $cps));
    }
    $GLOBALS['__diags'] = [];
    $r = iconv('UCS-4', $cs, ucs4(...$cps));
    return $r === false ? null : $r;
}

function input_pool(string $cs): array
{
    $sequences = [
        [], [0x61], [0x61, 0x62, 0x63], [0x00], [0x7F], [0x80], [0xFF], [0x100], [0xE9], [0x20AC], [0xFEFF],
        [0xFFFE], [0xFFFF], [0xD7FF], [0xE000], [0x10000], [0x1F600], [0x10FFFF], [0x61, 0xE9, 0x62], [0xBD],
        [0x2018, 0x2019, 0x201C, 0x201D], [0x2026], [0xFB01], [0x1D400], [0x4E00], [0x200B, 0x61], [0x01C5],
        [0x339F], [0x22C5], [0x2F800], [0x1F1E6], [0xC6, 0xDF, 0xA0, 0xAD], [0x61, 0x20AC, 0x1F600, 0xE9, 0x4E00, 0x62],
        [0xFEFF, 0x61], [0xFFFE, 0x61], [0x61, 0xFEFF], [0x110000], [0x7FFFFFFF], [0x61, 0x110000, 0x62],
        [0x3FFFFFF, 0x4000000], [0xD800], [0x61, 0xDFFF, 0x62], [0x110000, 0x110000], [0x61, 0x10FFFF, 0x110000],
    ];
    $pool = [];
    foreach ($sequences as $cps) {
        $s = encode_in($cs, $cps);
        if ($s !== null) {
            $pool[] = $s;
        }
    }
    $raw = match ($cs) {
        'UTF-8' => ["\x80", "a\xffb", "\xc0\x80", "\xe0\x80\x80", "\xed\xa0\x80", "\xf4\x90\x80\x80", "\xf8\x88\x80\x80\x80",
            "\xfc\x84\x80\x80\x80\x80", "\xfe", "a\xc3", "a\xe2\x82", "a\xc3b", "\xe2\x82\xac\xe2\x82", "a\xf0\x9f\x98",
            "\xc2\xc2\x80", "a\xe0\x80", "\xc1\xbf", "a\xed\xa0", "\xe2\x82\xac\xff\xc3\xa9", "\xc3\xa9\xe0\x80"],
        'UTF-16', 'UTF-16BE', 'UTF-16LE', 'UCS-2', 'UCS-2BE', 'UCS-2LE' => ["\x00\xd8", "\xd8\x00", "\x00\xdc\x61\x00",
            "\xdc\x00\x00\x61", "\x3d\xd8\x00\xde", "\xd8\x3d\xde\x00", "\x61", "\x61\x00\x62", "\xfe\xff", "\xff\xfe",
            "\xfe\xff\x00\x61", "\xff\xfe\x61\x00", "\xfe\xff\xfe\xff", "\xff\xfe\xff\xfe\x61\x00", "\x00\xd8\x00\xd8\x00\xdc",
            "\xd8\x00\xd8\x00\xdc\x00", "\x61\x00\x00\xd8", "\x00\x61\xd8\x00", "\xfe", "\xe9\x00\xac\x20", "\x00\xe9\x20\xac",
            "\x00\xd8\x61\x00", "\xd8\x00\x00\x61\x00\x62"],
        'UTF-32', 'UTF-32BE', 'UTF-32LE', 'UCS-4', 'UCS-4BE', 'UCS-4LE' => ["\x00\x00\x00\x61", "\x61\x00\x00\x00",
            "\x00\x00\xfe\xff", "\xff\xfe\x00\x00", "\x00\x00\xfe\xff\x00\x00\x00\x61", "\xff\xfe\x00\x00\x61\x00\x00\x00",
            "\x00\x11\x00\x00", "\x00\x00\x11\x00", "\x80\x00\x00\x00", "\x00\x00\x00\x80", "\x7f\xff\xff\xff",
            "\xff\xff\xff\x7f", "\x00\x00\xd8\x00", "\x00\xd8\x00\x00", "\x00\x00\x00", "\x00\x00\x00\x61\x00",
            "\xff\xff\xff\xff", "\x00\x00\xfe\xff\x00\x00\xfe\xff", "\xff\xfe\x00\x00\xff\xfe\x00\x00",
            "\x00\x00\x00\x61\x80\x00\x00\x00\x00\x00\x00\x62", "\x61\x00\x00\x00\x00\x00\x00\x80\x62\x00\x00\x00"],
        'ASCII' => ["\x80", "a\xffb", "\x7f\x80\x7f", "\xe9"],
        default => ["\xff\xe9\x80", "\x00\x7f\x80\xa0"],
    };
    array_push($pool, ...$raw);
    mt_srand(crc32($cs));
    for ($i = 0; $i < 6; $i++) {
        $s = '';
        for ($n = mt_rand(1, 12); $n > 0; $n--) {
            $s .= chr(mt_rand(0, 255));
        }
        $pool[] = $s;
    }
    for ($i = 0; $i < 6; $i++) {
        $cps = [];
        for ($n = mt_rand(1, 8); $n > 0; $n--) {
            $r = mt_rand(0, 99);
            $cp = match (true) {
                $r < 40 => mt_rand(0x20, 0x7E),
                $r < 55 => mt_rand(0x80, 0xFF),
                $r < 80 => mt_rand(0x100, 0xFFFD),
                $r < 90 => mt_rand(0x10000, 0x10FFFF),
                default => [0xE9, 0x20AC, 0x2026, 0xBD, 0x1F600, 0x4E00, 0x200B][mt_rand(0, 6)],
            };
            if (representable($cs, $cp)) {
                $cps[] = $cp;
            }
        }
        $s = encode_in($cs, $cps);
        if ($s !== null) {
            $pool[] = $s;
        }
    }
    return array_values(array_unique($pool));
}

function pair_cases(array $suffixes): array
{
    $cases = [];
    foreach (CHARSETS as $from) {
        $pool = input_pool($from);
        foreach (CHARSETS as $to) {
            foreach ($suffixes as $suffix) {
                foreach ($pool as $input) {
                    $cases[] = conversion($from, $to . $suffix, $input);
                }
            }
        }
    }
    return $cases;
}
