<?php
// Independent oracle cross-check of the clean-room iconv() (twig-value's `charset_basic`, the default build's
// `BasicIconv`) for the charsets it gained with the multi-byte work: the 13 multi-byte charsets, ISO-2022-JP,
// ISO-2022-JP-2, ISO-2022-KR and the 187 single-byte charsets added with them, alone and converted from and to the
// charsets it had before.
//
// Written apart from the other generators of this directory and from the module's tables: what it knows about a
// charset is measured here, black-box, through PHP's iconv() (which bytes are characters, which start a longer
// sequence and which break one, which code points each charset encodes, which characters //TRANSLIT replaces), or
// taken from the standards and vendor documentation (the ISO-2022 escape sequences and shifts of RFC 1468, RFC 1554
// and RFC 1557, GB 18030's four-byte ranges, CP932's NEC and IBM rows, the user-defined areas, Big5-HKSCS's
// composed pairs, late additions such as the euro sign).
//
// Every input is converted four times: without a suffix, with //TRANSLIT, //IGNORE and //TRANSLIT//IGNORE (each is a
// case). The sections (named in the output):
// - random inputs biased to each charset's structure (valid and broken sequences, truncations, escape sequences and
//   shift states, mixed scripts, code points of every plane, invalid UTF-8, UTF-16 and UTF-32), between the charsets
//   under test and to and from the charsets before, under names drawn from each charset's aliases;
// - long inputs with such things around the 8160-character rounds, also periodic errors and unconvertible characters;
// - the output buffer of PHP's iconv() (its length plus 32, grown by its length) running out at every offset of a
//   repeated unit: ISO-2022 escapes and shifts, //TRANSLIT replacements longer than their input, also after a
//   two-byte set, BIG5-HKSCS's held characters and composed pairs, the combining decoders and encoders;
// - edge cases from the standards and vendor extensions, empty inputs, suffix variants, single-byte targets without
//   "?", Unicode language tags into ISO-2022-JP-2 (every case of their letters), single-byte charsets of one script
//   converted into each other (every byte), and //TRANSLIT replacements beyond ASCII in the single-byte charsets.
//
// Output: {"generator", "seed", "cases_total", "charsets": [name], "families": [charset under test or before],
//          "flags": [suffix], "messages": [[level, text]], "sections": [[first case, name]],
//          "cases": [[from, to, input, r0, r1, r2, r3]]}
//   from, to  indexes into "charsets" (and "families": which charset of the lists below the name was drawn for).
//   input     base64, or [[base64, count], ...]: each unit repeated count times, concatenated.
//   r0..r3    the results under the four "flags", in order; when r1, r2 and r3 equal r0, only r0 is written.
//             A string is iconv()'s output: hex when at most 12 bytes long, else "<length>:<fnv1a32 of the bytes>";
//             a number n >= 1 is false after the diagnostic messages[n - 1] (level as PHP's E_* value);
//             0 (in r1..r3) is the same result as r0.
//
// The output is the same from run to run (one seed, and the oracle answers alike). Regenerate with the Docker oracle
// (research/php-oracle, image twig-oracle:3.31.1), from crates/twig-value/tests/vectors (half a minute):
//
//   MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//       php /t/string/charset_basic/crosscheck.gen.php > string/charset_basic/crosscheck.json

declare(strict_types=1);

ini_set('memory_limit', '-1');
error_reporting(E_ALL);

const SEED = 20261127;
const FLAGS = ['', '//TRANSLIT', '//IGNORE', '//TRANSLIT//IGNORE'];

/** The multi-byte charsets under test. */
const MB = ['SJIS', 'CP932', 'EUC-JP', 'EUC-JP-MS', 'GBK', 'GB18030', 'EUC-CN', 'BIG5', 'BIG5-HKSCS', 'EUC-KR', 'UHC',
    'JOHAB', 'EUC-TW'];
/** The stateful charsets under test. */
const ST = ['ISO-2022-JP', 'ISO-2022-JP-2', 'ISO-2022-KR'];
/** The single-byte charsets under test (added with the multi-byte work). */
const NEW8 = ['ARABIC7', 'ARMSCII-8', 'BRF', 'CP10007', 'CP1004', 'CP1008', 'CP1046', 'CP1124', 'CP1125', 'CP1129',
    'CP1133', 'CP1162', 'CP1163', 'CP4909', 'CP5347', 'CP737', 'CP770', 'CP771', 'CP772', 'CP773', 'CP774', 'CP775',
    'CP851', 'CP855', 'CP857', 'CP858', 'CP860', 'CP861', 'CP862', 'CP863', 'CP865', 'CP866NAV', 'CP868', 'CP869',
    'CP874', 'CP901', 'CP902', 'CP904', 'CP922', 'CSISO139CSN369103', 'CSISO150', 'CSISO18GREEK7OLD',
    'CSISO19LATINGREEK', 'CSISO2033', 'CSISO27LATINGREEK1', 'CSISO49INIS', 'CSISO50INIS8', 'CSISO51INISCYRILLIC',
    'CSISO5427CYRILLIC', 'CSISO5427CYRILLIC1981', 'CSISO5428GREEK', 'CSISO88GREEK7', 'CSNATSDANO', 'CSNATSSEFI', 'CWI',
    'DEC-MCS', 'EBCDIC-AT-DE', 'EBCDIC-AT-DE-A', 'EBCDIC-CA-FR', 'EBCDIC-DK-NO', 'EBCDIC-DK-NO-A', 'EBCDIC-ES',
    'EBCDIC-ES-A', 'EBCDIC-ES-S', 'EBCDIC-FI-SE', 'EBCDIC-FI-SE-A', 'EBCDIC-FR', 'EBCDIC-IS-FRISS', 'EBCDIC-IT',
    'EBCDIC-PT', 'EBCDIC-UK', 'EBCDIC-US', 'ECMA-CYRILLIC', 'GEORGIAN-ACADEMY', 'GEORGIAN-PS', 'GOST_19768-74',
    'HP-GREEK8', 'HP-ROMAN8', 'HP-ROMAN9', 'HP-TURKISH8', 'IBM037', 'IBM038', 'IBM1025', 'IBM1026', 'IBM1047',
    'IBM1097', 'IBM1112', 'IBM1122', 'IBM1123', 'IBM1130', 'IBM1132', 'IBM1137', 'IBM1140', 'IBM1141', 'IBM1142',
    'IBM1143', 'IBM1144', 'IBM1145', 'IBM1146', 'IBM1147', 'IBM1148', 'IBM1149', 'IBM1153', 'IBM1154', 'IBM1155',
    'IBM1156', 'IBM1157', 'IBM1158', 'IBM1160', 'IBM1161', 'IBM1164', 'IBM1166', 'IBM12712', 'IBM16804', 'IBM256',
    'IBM273', 'IBM274', 'IBM275', 'IBM277', 'IBM278', 'IBM280', 'IBM281', 'IBM284', 'IBM285', 'IBM290', 'IBM297',
    'IBM420', 'IBM423', 'IBM424', 'IBM4517', 'IBM4899', 'IBM4971', 'IBM500', 'IBM803', 'IBM856', 'IBM864', 'IBM870',
    'IBM871', 'IBM875', 'IBM880', 'IBM9030', 'IBM905', 'IBM9066', 'IBM918', 'IEC_P27-1', 'ISIRI-3342', 'ISO-8859-9E',
    'ISO-IR-197', 'ISO-IR-209', 'ISO11548-1', 'ISO646-CA', 'ISO646-CA2', 'ISO646-CN', 'ISO646-CU', 'ISO646-DE',
    'ISO646-DK', 'ISO646-ES', 'ISO646-ES2', 'ISO646-FI', 'ISO646-FR', 'ISO646-FR1', 'ISO646-GB', 'ISO646-HU',
    'ISO646-IT', 'ISO646-JP', 'ISO646-JP-OCR-B', 'ISO646-KR', 'ISO646-NO', 'ISO646-NO2', 'ISO646-PT', 'ISO646-PT2',
    'ISO646-SE2', 'ISO646-YU', 'ISO_10367-BOX', 'KOI8', 'KOI8-T', 'MAC-CENTRALEUROPE', 'MAC-CYRILLIC', 'MAC-IS',
    'MAC-SAMI', 'MIK', 'PT154', 'RK1048', 'TCVN-5712', 'TIS-620', 'VISCII', 'WIN-SAMI-2'];
/** The Unicode charsets the converter had before. */
const UNI = ['UTF-8', 'UTF-16', 'UTF-16BE', 'UTF-16LE', 'UTF-32', 'UTF-32BE', 'UTF-32LE', 'UCS-2', 'UCS-2BE',
    'UCS-2LE', 'UCS-4', 'UCS-4BE', 'UCS-4LE'];
/** The other charsets the converter had before. */
const OLD8 = ['ASCII', 'ISO-8859-1', 'ISO-8859-2', 'ISO-8859-3', 'ISO-8859-4', 'ISO-8859-5', 'ISO-8859-6', 'ISO-8859-7',
    'ISO-8859-8', 'ISO-8859-9', 'ISO-8859-10', 'ISO-8859-11', 'ISO-8859-13', 'ISO-8859-14', 'ISO-8859-15',
    'ISO-8859-16', 'CP1250', 'CP1251', 'CP1252', 'CP1253', 'CP1254', 'CP1255', 'CP1256', 'CP1257', 'CP1258', 'KOI8-R',
    'KOI8-U', 'KOI8-RU', 'CP437', 'CP850', 'CP852', 'CP866', 'MACINTOSH'];

/** Malformed UTF-8: stray continuation bytes, overlong forms, surrogates, values past U+10FFFF, truncations. */
const BAD_UTF8 = ["\x80", "\xbf", "\xc0\x80", "\xc1\xbf", "\xe0\x80\x80", "\xe0\x9f\xbf", "\xed\xa0\x80",
    "\xed\xbf\xbf", "\xed\xa0\x80\xed\xb0\x80", "\xf0\x80\x80\x80", "\xf0\x8f\xbf\xbf", "\xf4\x90\x80\x80",
    "\xf5\x80\x80\x80", "\xf8\x88\x80\x80\x80", "\xfc\x84\x80\x80\x80\x80", "\xfe", "\xff", "\xc3", "\xe3\x81",
    "\xf0\x9f\x98", "\xc3\x28", "\xe3\x28\x81", "\xef\xbf"];

$GLOBALS['diag'] = [];
set_error_handler(static function (int $no, string $text): bool {
    $GLOBALS['diag'][] = [$no, $text];
    return true;
});

function fail(string $why): never
{
    fwrite(STDERR, "crosscheck.gen.php: $why\n");
    exit(1);
}

function note(string $what): void
{
    fwrite(STDERR, sprintf("[%6.1fs] %s\n", microtime(true) - $GLOBALS['t0'], $what));
}

$GLOBALS['t0'] = microtime(true);
mt_srand(SEED);

// ---------------------------------------------------------------------------------------------------------------
// iconv() and the randomness

/** iconv() and the diagnostics it raised. */
function call(string $from, string $to, string $in): array
{
    $GLOBALS['diag'] = [];
    $r = iconv($from, $to, $in);
    return [$r, $GLOBALS['diag']];
}

/** Decodes $bytes: ['v', code points], ['i'] (illegal), ['c'] (incomplete) or ['w'] (no conversion). */
function dec(string $cs, string $bytes): array
{
    [$r, $d] = call($cs, 'UTF-32BE', $bytes);
    if ($r !== false) {
        return ['v', $r === '' ? [] : array_values(unpack('N*', $r))];
    }
    $m = $d[0][1] ?? '';
    if (str_contains($m, 'incomplete')) {
        return ['c'];
    }
    return [str_contains($m, 'illegal') ? 'i' : 'w'];
}

/** Whether $cs encodes code point $cp. */
function encodes(string $cs, int $cp): bool
{
    [$r] = call('UTF-32BE', $cs, pack('N', $cp));
    return $r !== false;
}

function pct(int $p): bool
{
    return mt_rand(0, 99) < $p;
}

function pick(array $list): mixed
{
    if (!$list) {
        fail('pick from an empty list');
    }
    return $list[mt_rand(0, count($list) - 1)];
}

/** A key of $weights (non-numeric keys), drawn with the weights given. */
function wpick(array $weights): string
{
    $total = array_sum($weights);
    $x = mt_rand(0, $total - 1);
    foreach ($weights as $k => $w) {
        if ($x < $w) {
            return (string) $k;
        }
        $x -= $w;
    }
    fail('wpick');
}

/** A length: mostly short, sometimes longer. */
function len(): int
{
    return pct(70) ? mt_rand(1, 6) : (pct(80) ? mt_rand(7, 20) : mt_rand(21, 60));
}

// ---------------------------------------------------------------------------------------------------------------
// Unicode charsets written here (so that broken units can be placed exactly)

/** UTF-8, also for surrogates and the 5- and 6-byte forms of values past U+1FFFFF. */
function u8(int $cp): string
{
    if ($cp < 0x80) {
        return chr($cp);
    }
    if ($cp < 0x800) {
        return chr(0xc0 | ($cp >> 6)) . chr(0x80 | ($cp & 0x3f));
    }
    if ($cp < 0x10000) {
        return chr(0xe0 | ($cp >> 12)) . chr(0x80 | (($cp >> 6) & 0x3f)) . chr(0x80 | ($cp & 0x3f));
    }
    if ($cp < 0x200000) {
        return chr(0xf0 | ($cp >> 18)) . chr(0x80 | (($cp >> 12) & 0x3f)) . chr(0x80 | (($cp >> 6) & 0x3f))
            . chr(0x80 | ($cp & 0x3f));
    }
    if ($cp < 0x4000000) {
        return chr(0xf8 | ($cp >> 24)) . chr(0x80 | (($cp >> 18) & 0x3f)) . chr(0x80 | (($cp >> 12) & 0x3f))
            . chr(0x80 | (($cp >> 6) & 0x3f)) . chr(0x80 | ($cp & 0x3f));
    }
    return chr(0xfc | (($cp >> 30) & 1)) . chr(0x80 | (($cp >> 24) & 0x3f)) . chr(0x80 | (($cp >> 18) & 0x3f))
        . chr(0x80 | (($cp >> 12) & 0x3f)) . chr(0x80 | (($cp >> 6) & 0x3f)) . chr(0x80 | ($cp & 0x3f));
}

function unit16(int $u, bool $le): string
{
    return $le ? pack('v', $u) : pack('n', $u);
}

/** UTF-16 code units of $cp (a surrogate pair past U+FFFF). */
function u16(int $cp, bool $le): string
{
    if ($cp >= 0x10000 && $cp <= 0x10ffff) {
        $cp -= 0x10000;
        return unit16(0xd800 | ($cp >> 10), $le) . unit16(0xdc00 | ($cp & 0x3ff), $le);
    }
    return unit16($cp & 0xffff, $le);
}

function u32(int $cp, bool $le): string
{
    return $le ? pack('V', $cp) : pack('N', $cp);
}

/** A broken unit for a UTF-16 or UCS-2 input. */
function bad16(bool $le): string
{
    return match (mt_rand(0, 6)) {
        0 => unit16(0xd800 + mt_rand(0, 0x3ff), $le),
        1 => unit16(0xdc00 + mt_rand(0, 0x3ff), $le),
        2 => unit16(0xdc00 + mt_rand(0, 0x3ff), $le) . unit16(0xd800 + mt_rand(0, 0x3ff), $le),
        3 => unit16(0xfffe, $le),
        4 => unit16(0xfeff, $le),
        5 => unit16(0xffff, $le),
        default => chr(mt_rand(0, 255)),
    };
}

/** A value that is not a character, for a UTF-32 or UCS-4 input. */
function bad32(bool $le): string
{
    return match (mt_rand(0, 7)) {
        0 => u32(0x110000 + mt_rand(0, 0xffff), $le),
        1 => u32(0xd800 + mt_rand(0, 0x7ff), $le),
        2 => u32(0x7fffffff, $le),
        3 => u32(0x80000000 + mt_rand(0, 0xffff), $le),
        4 => u32(0xffffffff, $le),
        5 => u32(0xfffe, $le),
        6 => u32(0x1000000 + mt_rand(0, 0xffffff), $le),
        default => substr(u32(mt_rand(0x20, 0x10ffff), $le), 0, mt_rand(1, 3)),
    };
}

/**
 * $cps in the Unicode charset $cs; with $noise, broken units mixed in. The charsets without a byte order in their
 * name get one at random, with or without a byte order mark.
 */
function uni_bytes(string $cs, array $cps, bool $noise): string
{
    $out = [];
    $kind = match ($cs) {
        'UTF-8' => 8,
        'UTF-16', 'UTF-16BE', 'UTF-16LE', 'UCS-2', 'UCS-2BE', 'UCS-2LE' => 16,
        default => 32,
    };
    $le = str_ends_with($cs, 'LE');
    $bom = '';
    if (in_array($cs, ['UTF-16', 'UCS-2', 'UTF-32', 'UCS-4'], true)) {
        $le = pct(50);
        if (pct(60)) {
            $bom = $kind === 16 ? unit16(0xfeff, $le) : u32(0xfeff, $le);
        }
    }
    foreach ($cps as $cp) {
        if ($kind === 8) {
            $out[] = u8($cp);
        } elseif ($kind === 16) {
            $out[] = u16($cp, $le);
        } else {
            $out[] = u32($cp, $le);
        }
    }
    if ($noise) {
        $n = mt_rand(1, 2);
        for ($i = 0; $i < $n; $i++) {
            $bad = match ($kind) {
                8 => pick(BAD_UTF8),
                16 => bad16($le),
                default => bad32($le),
            };
            array_splice($out, mt_rand(0, count($out)), 0, [$bad]);
        }
        if (pct(15) && $out) {
            $last = array_pop($out);
            $out[] = substr($last, 0, max(1, strlen($last) - mt_rand(1, 3)));
        }
    }
    return $bom . implode('', $out);
}

// ---------------------------------------------------------------------------------------------------------------
// Charset names: every name iconv -l lists, grouped by how the charset converts a probe

/** The code points the name probe encodes. */
function probe_cps(): array
{
    $cps = [0x00, 0x0e, 0x1b, 0x20, 0x23, 0x24, 0x40, 0x41, 0x5b, 0x5c, 0x5d, 0x5e, 0x60, 0x7b, 0x7c, 0x7d, 0x7e,
        0x7f, 0x80, 0x85, 0x9f, 0xfeff, 0xfffd, 0x20ac, 0x2116, 0x2122, 0x10000, 0x1f600, 0x20000, 0x2a6d6, 0x10ffff,
        0xe0001, 0xe0041, 0x301c, 0xff5e, 0x2016, 0x2225, 0x2212, 0xff0d, 0xffe0, 0xffe1, 0xffe2, 0xffe4, 0x00a5,
        0x203e, 0x327e, 0x00ae, 0x3164, 0x20a9, 0xff61, 0xff9f, 0x2460, 0x2160, 0x2170, 0x3231, 0x3300, 0x33a8];
    $ranges = [[0xa0, 0xff, 3], [0x100, 0x17f, 7], [0x180, 0x24f, 17], [0x2b0, 0x36f, 11], [0x370, 0x3ff, 5],
        [0x400, 0x4ff, 5], [0x530, 0x58f, 9], [0x5d0, 0x5ea, 4], [0x600, 0x6ff, 7], [0xe01, 0xe5b, 6],
        [0xe81, 0xedd, 9], [0x10d0, 0x10f6, 5], [0x1e00, 0x1eff, 13], [0x2010, 0x2044, 2], [0x20a0, 0x20c0, 3],
        [0x2100, 0x214f, 5], [0x2190, 0x21ff, 13], [0x2200, 0x22ff, 7], [0x2300, 0x23ff, 31], [0x2500, 0x259f, 5],
        [0x25a0, 0x25ff, 11], [0x2600, 0x26ff, 29], [0x3000, 0x303f, 3], [0x3041, 0x30ff, 7], [0x3105, 0x312f, 9],
        [0x3131, 0x318e, 6], [0x3200, 0x32ff, 17], [0x3300, 0x33ff, 15], [0x3400, 0x4dbf, 409], [0x4e00, 0x9fff, 97],
        [0xac00, 0xd7a3, 211], [0xe000, 0xf8ff, 113], [0xf900, 0xfaff, 23], [0xfb00, 0xfb4f, 7], [0xfe10, 0xfe6f, 7],
        [0xff01, 0xffef, 3]];
    foreach ($ranges as [$a, $b, $step]) {
        for ($c = $a; $c <= $b; $c += $step) {
            $cps[] = $c;
        }
    }
    return $cps;
}

/** The byte strings the name probe decodes. */
function probe_bytes(): array
{
    $list = [];
    for ($b = 0; $b < 256; $b++) {
        $list[] = chr($b);
    }
    foreach ([0x81, 0x84, 0x87, 0x88, 0x8e, 0x8f, 0xa1, 0xa2, 0xa4, 0xad, 0xb0, 0xc6, 0xc9, 0xd9, 0xed, 0xee, 0xf0, 0xf9,
        0xfa, 0xfe] as $l) {
        foreach ([0x31, 0x40, 0x41, 0x5c, 0x62, 0x7e, 0x80, 0x81, 0x91, 0xa1, 0xa2, 0xc1, 0xdf, 0xe6, 0xe8, 0xfe] as $t) {
            $list[] = chr($l) . chr($t);
        }
    }
    foreach (['81308130', '8431a439', '90308130', 'e3329a35', '8ea2a1a1', '8ea1a1a1', '8fb0a1', '8fa2b7', 'a1a1a1',
        '1b2442302121', '1b244230211b2842', '1b2429430e30210f', '1b2441302121', '1b2e411b4e69', '2b41474520',
        'feff0041', 'fffe4100', '0000feff00000041', '1b2849312a', '1b2428443021'] as $h) {
        $list[] = hex2bin($h);
    }
    return $list;
}

/** For each charset name of iconv -l: a fingerprint of how it converts the probe. */
function fingerprints(): array
{
    $names = [];
    foreach (explode("\n", trim((string) shell_exec('iconv -l'))) as $line) {
        $names[] = rtrim(trim($line), '/');
    }
    $cps = probe_cps();
    $bytes = probe_bytes();
    $prints = [];
    foreach ($names as $name) {
        $h = hash_init('md5');
        foreach ($cps as $cp) {
            [$r, $d] = call('UTF-32BE', $name, pack('N', $cp));
            hash_update($h, ($r === false ? '!' . ($d[0][1] ?? '') : bin2hex($r)) . ',');
        }
        foreach ($bytes as $b) {
            [$r, $d] = call($name, 'UTF-32BE', $b);
            hash_update($h, ($r === false ? '!' . ($d[0][1] ?? '') : bin2hex($r)) . ',');
        }
        $prints[$name] = hash_final($h);
    }
    return $prints;
}

// ---------------------------------------------------------------------------------------------------------------
// What each charset decodes and encodes, measured

/**
 * Explores the sequences of $cs up to four bytes long: every byte, every extension of an incomplete prefix (at most
 * 2000 prefixes of a length, sampled beyond that). Returns ['valid' => [bytes => code points], 'bad' => [illegal
 * sequences, sampled], 'partial' => [incomplete prefixes]].
 */
function explore(string $cs): array
{
    $valid = [];
    $bad = [];
    $partial = [];
    $frontier = [''];
    for ($len = 1; $len <= 4 && $frontier; $len++) {
        if (count($frontier) > 2000) {
            $sample = [];
            for ($i = 0; $i < 2000; $i++) {
                $sample[$frontier[mt_rand(0, count($frontier) - 1)]] = true;
            }
            $frontier = array_map('strval', array_keys($sample));
        }
        $next = [];
        foreach ($frontier as $p) {
            for ($b = 0; $b < 256; $b++) {
                $s = $p . chr($b);
                $r = dec($cs, $s);
                if ($r[0] === 'v') {
                    $valid[$s] = $r[1];
                } elseif ($r[0] === 'c') {
                    $next[] = $s;
                } elseif ($r[0] === 'i') {
                    if ($len === 1 || mt_rand(0, 15) === 0) {
                        $bad[] = $s;
                    }
                } else {
                    fail("$cs does not decode");
                }
            }
        }
        array_push($partial, ...$next);
        $frontier = $next;
    }
    return ['valid' => $valid, 'bad' => $bad, 'partial' => $partial];
}

/** The tables a generator draws from, for a charset explored by explore(). */
function tables(array $x): array
{
    $single = [];
    $multi = [];
    $bycp = [];
    foreach ($x['valid'] as $s => $cps) {
        $s = (string) $s;
        if (strlen($s) === 1) {
            $single[] = $s;
        } else {
            $multi[strlen($s)][] = $s;
        }
        if (count($cps) === 1) {
            $bycp[$cps[0]][] = $s;
        }
    }
    ksort($multi);
    return ['single' => $single, 'multi' => array_values($multi), 'bycp' => $bycp,
        'bad' => array_map('strval', $x['bad']), 'partial' => array_map('strval', $x['partial'])];
}

/** Ranges of code points beyond the decoded tables that the encoders are tried with. */
function extra_cps(): array
{
    $out = [];
    foreach ([[0x00, 0x24f], [0x250, 0x36f], [0x370, 0x52f], [0x530, 0x6ff], [0xe00, 0xeff], [0x10a0, 0x11ff],
        [0x1e00, 0x1fff], [0x2000, 0x2bff], [0x2e80, 0x33ff], [0x3400, 0x4dbf], [0xa000, 0xa4cf], [0xac00, 0xd7ff],
        [0xe000, 0xf8ff], [0xf900, 0xffff], [0x1d400, 0x1d7ff], [0x1f100, 0x1f2ff], [0x1f600, 0x1f64f],
        [0xe0000, 0xe007f]] as [$a, $b]) {
        for ($c = $a; $c <= $b; $c++) {
            if ($c < 0xd800 || $c > 0xdfff) {
                $out[$c] = true;
            }
        }
    }
    return $out;
}

/** The code points of $universe that $cs encodes. */
function encodable(string $cs, array $universe): array
{
    $out = [];
    foreach ($universe as $cp => $_) {
        if (encodes($cs, $cp)) {
            $out[] = $cp;
        }
    }
    return $out;
}

// ---------------------------------------------------------------------------------------------------------------
// Inputs

/** An ASCII fragment. */
function ascii(): string
{
    return pick(['a', 'Az', '09', ' ', '\\', '~', '{|}', 'abc ', "\t", "\n", "\r\n", '$', '#@', 'Zz9', '.', '[]^_`']);
}

/**
 * Bytes for $cs (explored): valid characters, mostly multi-byte, and with $noise illegal bytes, sequences with a bad
 * byte, incomplete prefixes (also at the end), random bytes and control characters.
 */
function mb_bytes(array $t, int $n, bool $noise): string
{
    $s = '';
    for ($i = 0; $i < $n; $i++) {
        $what = $noise
            ? wpick(['multi' => 50, 'single' => 12, 'ascii' => 8, 'bad' => 10, 'partial' => 8, 'byte' => 6, 'ctl' => 6])
            : wpick(['multi' => 72, 'single' => 16, 'ascii' => 12]);
        if ($what === 'multi' && !$t['multi']) {
            $what = 'single';
        }
        if ($what === 'partial' && !$t['partial']) {
            $what = 'bad';
        }
        $s .= match ($what) {
            'multi' => pick(pick($t['multi'])),
            'single' => pick($t['single']),
            'ascii' => ascii(),
            'bad' => pick($t['bad']),
            'partial' => pick($t['partial']),
            'byte' => chr(mt_rand(0, 255)),
            default => pick(["\0", "\n", "\r", "\t", "\x1b", "\x0e", "\x0f", "\x7f", "\x1b(B", "\x1b\$B"]),
        };
    }
    if ($noise && pct(25)) {
        $s .= $t['partial'] ? pick($t['partial']) : pick($t['bad']);
    }
    return $s;
}

/** Bytes for $cs (explored) that decode to $cps, a random sequence for each code point that has several. */
function mb_encode(array $t, array $cps): string
{
    $s = '';
    foreach ($cps as $cp) {
        $s .= pick($t['bycp'][$cp]);
    }
    return $s;
}

/** A code point from a random plane (assigned or not, never a surrogate). */
function any_plane(): int
{
    $p = wpick(['p0' => 40, 'p1' => 15, 'p2' => 15, 'p3' => 6, 'p4' => 6, 'p14' => 10, 'p15' => 8]);
    $plane = match ($p) {
        'p0' => 0,
        'p1' => 1,
        'p2' => 2,
        'p3' => 3,
        'p4' => mt_rand(4, 13),
        'p14' => 14,
        default => mt_rand(15, 16),
    };
    do {
        $cp = ($plane << 16) | ($plane === 14 && pct(70) ? mt_rand(0, 0x7f) : mt_rand(0, 0xffff));
    } while ($cp >= 0xd800 && $cp <= 0xdfff);
    return $cp;
}

/**
 * Code points biased to $target: what it encodes, ASCII, what //TRANSLIT replaces, characters of other charsets,
 * any plane and the target's own edge cases.
 */
function cps_for(string $target, int $n): array
{
    global $ENC, $TRANSLIT, $OTHER, $SPECIAL;
    $out = [];
    for ($i = 0; $i < $n; $i++) {
        $what = wpick(['rep' => 58, 'ascii' => 10, 'translit' => 9, 'other' => 8, 'plane' => 6, 'special' => 9]);
        if ($what === 'rep' && !$ENC[$target]) {
            $what = 'ascii';
        }
        switch ($what) {
            case 'rep':
                $out[] = pick($ENC[$target]);
                break;
            case 'ascii':
                foreach (str_split(ascii()) as $c) {
                    $out[] = ord($c);
                }
                break;
            case 'translit':
                $out[] = pick($TRANSLIT);
                break;
            case 'other':
                $out[] = pick($OTHER);
                break;
            case 'plane':
                $out[] = any_plane();
                break;
            default:
                foreach (pick($SPECIAL[$target] ?? $SPECIAL['*']) as $cp) {
                    $out[] = $cp;
                }
        }
    }
    return $out;
}

/** An input in the source charset $from (Unicode, single-byte or explored) for code points $cps. */
function source_bytes(string $from, array $cps, bool $noise): string
{
    global $T;
    if (in_array($from, UNI, true)) {
        if (str_starts_with($from, 'UCS-2')) {
            foreach ($cps as &$cp) {
                if ($cp > 0xffff && pct(70)) {
                    $cp = mt_rand(0x4e00, 0x9fa5);
                }
            }
            unset($cp);
        }
        return uni_bytes($from, $cps, $noise);
    }
    $t = $T[$from];
    $s = '';
    foreach ($cps as $cp) {
        if (isset($t['bycp'][$cp])) {
            $s .= pick($t['bycp'][$cp]);
        } elseif ($noise && pct(30)) {
            $s .= pick($t['bad']);
        }
    }
    if ($noise) {
        $s .= pct(50) && $t['partial'] ? pick($t['partial']) : pick($t['bad']);
    }
    return $s;
}

/** ISO-2022 designations (G0 unless noted) as RFC 1468, RFC 1554 and RFC 1557 write them, and a few that are not. */
const ESC_JP = ['ascii' => "\x1b(B", 'roman' => "\x1b(J", 'jis78' => "\x1b\$@", 'jis83' => "\x1b\$B"];
const ESC_JP2 = ['gb' => "\x1b\$A", 'ksc' => "\x1b\$(C", 'jis212' => "\x1b\$(D"];
const ESC_G2 = ['l1' => "\x1b.A", 'gr' => "\x1b.F"];
const ESC_ODD = ["\x1b(I", "\x1b\$(B", "\x1b\$(@", "\x1b&@\x1b\$B", "\x1b\$)C", "\x1b(", "\x1b\$", "\x1b\$(", "\x1b",
    "\x1b(Z", "\x1b\$Z", "\x1b.Z", "\x1bN", "\x1bO", "\x1b(\x80", "\x1b\$)A", "\x1b-A", "\x1b,A", "\x0e", "\x0f",
    "\x1b\$(A", "\x1b\$(E", "\x1b\$+I"];

/** The pairs of bytes (0x21..0x7E) of the 94x94 sets, from the EUC tables measured. */
function pairs94(array $t, ?int $prefix): array
{
    $out = [];
    foreach ($t['multi'] as $list) {
        foreach ($list as $s) {
            if ($prefix === null && strlen($s) === 2 && ord($s[0]) >= 0xa1 && ord($s[1]) >= 0xa1) {
                $out[] = chr(ord($s[0]) - 0x80) . chr(ord($s[1]) - 0x80);
            } elseif ($prefix !== null && strlen($s) === 3 && ord($s[0]) === $prefix) {
                $out[] = chr(ord($s[1]) - 0x80) . chr(ord($s[2]) - 0x80);
            }
        }
    }
    return $out;
}

/** A random pair of 94-set bytes (assigned or not). */
function pair_any(): string
{
    return chr(mt_rand(0x21, 0x7e)) . chr(mt_rand(0x21, 0x7e));
}

/** An ISO-2022-JP or ISO-2022-JP-2 byte stream: designations, characters of the designated set, noise. */
function jp_bytes(string $cs, int $n, bool $noise): string
{
    global $PAIRS;
    $g0 = ESC_JP;
    if ($cs === 'ISO-2022-JP-2') {
        $g0 += ESC_JP2;
    }
    $mode = 'ascii';
    $g2 = false;
    $s = pct(15) ? pick($g0) : '';
    for ($i = 0; $i < $n; $i++) {
        $what = wpick(['text' => 55, 'switch' => 25, 'g2' => $cs === 'ISO-2022-JP-2' ? 8 : 0, 'nl' => 4,
            'noise' => $noise ? 12 : 0]);
        if ($what === 'switch') {
            $keys = array_keys($g0);
            $mode = $keys[mt_rand(0, count($keys) - 1)];
            $s .= $g0[$mode];
            $what = 'text';
        }
        if ($what === 'text') {
            $k = mt_rand(1, 5);
            for ($j = 0; $j < $k; $j++) {
                $s .= match ($mode) {
                    'ascii' => pct(90) ? chr(mt_rand(0x20, 0x7e)) : pick(["\t", "\r", "\0", "\x7f"]),
                    'roman' => chr(mt_rand(0x20, 0x7e)),
                    'jis78', 'jis83' => pct(92) ? pick($PAIRS['jis0208']) : pair_any(),
                    'gb' => pct(92) ? pick($PAIRS['gb2312']) : pair_any(),
                    'ksc' => pct(92) ? pick($PAIRS['ksc5601']) : pair_any(),
                    'jis212' => pct(92) ? pick($PAIRS['jis0212']) : pair_any(),
                };
            }
        } elseif ($what === 'g2') {
            if (!$g2 || pct(20)) {
                $s .= pick(ESC_G2);
                $g2 = true;
            }
            $s .= "\x1bN" . chr(mt_rand(0x20, 0x7f));
        } elseif ($what === 'nl') {
            $s .= pick(["\n", "\r\n", "\r"]);
        } else {
            $s .= match (mt_rand(0, 5)) {
                0 => pick(ESC_ODD),
                1 => chr(mt_rand(0x80, 0xff)),
                2 => chr(mt_rand(0x21, 0x7e)),
                3 => "\x1bN" . chr(mt_rand(0, 255)),
                4 => chr(mt_rand(0, 0x1f)),
                default => ' ',
            };
        }
    }
    if (pct(60)) {
        $s .= "\x1b(B";
    } elseif ($noise && pct(30)) {
        $s .= pick(["\x1b", "\x1b(", "\x1b\$", "\x1b\$(", "\x1bN", chr(mt_rand(0x21, 0x7e))]);
    }
    return $s;
}

/** An ISO-2022-KR byte stream: the header, SO/SI, KS C 5601 pairs, noise. */
function kr_bytes(int $n, bool $noise): string
{
    global $PAIRS;
    $s = '';
    $h = wpick(['start' => 85, 'none' => 7, 'twice' => 4, 'late' => 4]);
    if ($h === 'start' || $h === 'twice') {
        $s .= "\x1b\$)C";
    }
    if ($h === 'twice') {
        $s .= "\x1b\$)C";
    }
    $so = false;
    for ($i = 0; $i < $n; $i++) {
        if ($h === 'late' && $i === intdiv($n, 2)) {
            $s .= "\x1b\$)C";
        }
        $what = wpick(['text' => 55, 'shift' => 25, 'nl' => 6, 'noise' => $noise ? 14 : 0]);
        if ($what === 'shift') {
            $so = !$so;
            $s .= $so ? "\x0e" : "\x0f";
            $what = 'text';
        }
        if ($what === 'text') {
            $k = mt_rand(1, 5);
            for ($j = 0; $j < $k; $j++) {
                $s .= $so ? (pct(92) ? pick($PAIRS['ksc5601']) : pair_any()) : chr(mt_rand(0x20, 0x7e));
            }
        } elseif ($what === 'nl') {
            $s .= pick(["\n", "\r\n", "\r"]);
        } else {
            $s .= match (mt_rand(0, 5)) {
                0 => pick(ESC_ODD),
                1 => chr(mt_rand(0x80, 0xff)),
                2 => chr(mt_rand(0x21, 0x7e)),
                3 => pick(["\x0e", "\x0f", "\x0e\x0e", "\x0f\x0f"]),
                4 => chr(mt_rand(0, 0x1f)),
                default => pick(ESC_JP),
            };
        }
    }
    if ($so && pct(60)) {
        $s .= "\x0f";
    }
    return $s;
}

function st_bytes(string $cs, int $n, bool $noise): string
{
    return $cs === 'ISO-2022-KR' ? kr_bytes($n, $noise) : jp_bytes($cs, $n, $noise);
}

/** Bytes in any charset under test or before, valid or (with $noise) not, for code points biased to $toward. */
function bytes_in(string $cs, bool $noise, ?string $toward): string
{
    global $T;
    if (in_array($cs, ST, true)) {
        if ($toward !== null && pct(60)) {
            // Characters both can take, written by the oracle's encoder (its designations and shifts).
            $cps = [];
            $n = len();
            $common = $GLOBALS['COMMON'][$cs][$toward] ?? [];
            for ($i = 0; $i < $n; $i++) {
                $cps[] = $common && pct(85) ? pick($common) : pick($GLOBALS['ENC'][$cs]);
            }
            [$r] = call('UTF-32BE', $cs, pack('N*', ...$cps));
            if ($r !== false) {
                if ($noise) {
                    $pos = mt_rand(0, strlen($r));
                    $r = substr($r, 0, $pos) . pick([...ESC_ODD, "\x80", "\xff", "\n", "\x1b(B", "\x1b\$B"])
                        . substr($r, $pos);
                }
                return $r;
            }
        }
        return st_bytes($cs, len(), $noise);
    }
    if (in_array($cs, UNI, true)) {
        return uni_bytes($cs, cps_for($toward ?? 'UTF-8', len()), $noise);
    }
    $t = $T[$cs];
    if ($toward !== null && pct(60)) {
        $common = $GLOBALS['COMMON'][$cs][$toward] ?? [];
        if ($common) {
            $cps = [];
            $n = len();
            for ($i = 0; $i < $n; $i++) {
                $cps[] = pct(85) ? pick($common) : pick(array_keys($t['bycp']) ?: [0x41]);
            }
            $s = mb_encode($t, array_values(array_filter($cps, static fn ($c) => isset($t['bycp'][$c]))));
            if ($noise) {
                $s .= pick($t['bad']);
                if ($t['partial'] && pct(50)) {
                    $s .= pick($t['partial']);
                }
            }
            return $s;
        }
    }
    return mb_bytes($t, len(), $noise);
}

// ---------------------------------------------------------------------------------------------------------------
// Long inputs: recipes of repeated units

/** A recipe: [[bytes, count], ...], adjacent equal units merged. */
function recipe(array $parts): array
{
    $out = [];
    foreach ($parts as [$unit, $count]) {
        if ($count <= 0 || $unit === '') {
            continue;
        }
        $last = count($out) - 1;
        if ($last >= 0 && $out[$last][0] === $unit) {
            $out[$last][1] += $count;
        } else {
            $out[] = [$unit, $count];
        }
    }
    return $out;
}

function expand(string|array $input): string
{
    if (is_string($input)) {
        return $input;
    }
    $s = '';
    foreach ($input as [$unit, $count]) {
        $s .= str_repeat($unit, $count);
    }
    return $s;
}

/** Character positions for the special units of a long input: around the 8160-character rounds, or anywhere. */
function positions(int $total, int $n): array
{
    $out = [];
    for ($i = 0; $i < $n; $i++) {
        $out[] = pct(65)
            ? min($total, 8160 * mt_rand(1, max(1, intdiv($total, 8160))) + mt_rand(-3, 3))
            : mt_rand(0, $total);
    }
    sort($out);
    return $out;
}

/**
 * A long input: $total filler characters (units drawn from $fillers, one character each, in runs) with $specials
 * placed at the positions given (each counts as one character).
 */
function long_input(array $fillers, array $specials, int $total): array
{
    $pos = positions($total, count($specials));
    $parts = [];
    $at = 0;
    foreach ($pos as $i => $p) {
        while ($at < $p) {
            $run = min($p - $at, pct(50) ? $p - $at : mt_rand(1, 3000));
            $parts[] = [pick($fillers), $run];
            $at += $run;
        }
        $parts[] = [$specials[$i], 1];
        $at++;
    }
    while ($at < $total) {
        $run = pct(50) ? $total - $at : mt_rand(1, $total - $at);
        $parts[] = [pick($fillers), $run];
        $at += $run;
    }
    return recipe($parts);
}

/** How many characters a long input has: one, two or three rounds and a bit, or a round exactly. */
function long_total(): int
{
    return pct(25) ? 8160 * mt_rand(1, 3) + mt_rand(-2, 2) : mt_rand(8100, 24600);
}

// ---------------------------------------------------------------------------------------------------------------
// Measuring

note('fingerprinting the charset names');
$PRINTS = fingerprints();
$GROUP = [];
foreach ($PRINTS as $name => $print) {
    $GROUP[$print][] = (string) $name;
}

/** The names of $cs's group (itself first). */
function aliases(string $cs): array
{
    global $PRINTS, $GROUP;
    $print = $PRINTS[$cs] ?? null;
    if ($print === null) {
        // A name iconv -l does not list (ASCII is ANSI_X3.4-1968 there): find its group by the probe.
        foreach ($PRINTS as $name => $p) {
            if (strcasecmp((string) $name, $cs) === 0) {
                $print = $p;
            }
        }
    }
    $list = $print === null ? [] : $GROUP[$print];
    // WCHAR_T converts like UCS-4LE on the probe, but twig-rs refuses it on purpose (reference/deviations.md): it
    // is not a charset under test, so it is not drawn as another name of UCS-4LE.
    $list = array_filter($list, static fn (string $n): bool => strcasecmp($n, 'WCHAR_T') !== 0);
    return array_values(array_unique([$cs, ...$list]));
}

$ALIASES = [];
$FAMILY = [];
foreach ([...MB, ...ST, ...NEW8, ...UNI, ...OLD8] as $cs) {
    $ALIASES[$cs] = aliases($cs);
    foreach ($ALIASES[$cs] as $name) {
        $FAMILY[strtoupper($name)] ??= $cs;
    }
}
foreach ([...MB, ...ST] as $cs) {
    note(sprintf('  %s: %s', $cs, implode(' ', $ALIASES[$cs])));
}

/** A name for $cs: mostly its own, sometimes one of its aliases, in a random case. */
function nm(string $cs): string
{
    global $ALIASES;
    $name = pct(85) || count($ALIASES[$cs]) < 2 ? $cs : pick($ALIASES[$cs]);
    if (pct(5)) {
        $name = strtolower($name);
    }
    return $name;
}

note('exploring the multi-byte charsets');
$T = [];
foreach (MB as $cs) {
    $T[$cs] = tables(explore($cs));
    note(sprintf('  %s: %d single, %s multi-byte, %d code points', $cs, count($T[$cs]['single']),
        implode('+', array_map('count', $T[$cs]['multi'])), count($T[$cs]['bycp'])));
}
note('exploring the single-byte charsets');
foreach ([...NEW8, ...OLD8] as $cs) {
    $T[$cs] = tables(explore($cs));
    if (!$T[$cs]['bad']) {
        $T[$cs]['bad'] = ["\xff"];
    }
}

$PAIRS = [
    'jis0208' => pairs94($T['EUC-JP'], null),
    'jis0212' => pairs94($T['EUC-JP'], 0x8f),
    'gb2312' => pairs94($T['EUC-CN'], null),
    'ksc5601' => pairs94($T['EUC-KR'], null),
];

note('measuring what the targets encode');
$UNIVERSE = extra_cps();
foreach (MB as $cs) {
    foreach ($T[$cs]['bycp'] as $cp => $_) {
        $UNIVERSE[$cp] = true;
    }
}
$UNIVERSE8 = [];
foreach ([...NEW8, ...OLD8] as $cs) {
    foreach ($T[$cs]['bycp'] as $cp => $_) {
        $UNIVERSE8[$cp] = true;
        $UNIVERSE[$cp] = true;
    }
}
foreach ([[0, 0x24f], [0x370, 0x4ff], [0x2010, 0x203e], [0x20a0, 0x20c0], [0x2100, 0x2135], [0x2190, 0x2199],
    [0x2500, 0x25a0], [0xfb00, 0xfb06], [0xfffd, 0xfffd]] as [$a, $b]) {
    for ($c = $a; $c <= $b; $c++) {
        $UNIVERSE8[$c] = true;
    }
}
ksort($UNIVERSE);
ksort($UNIVERSE8);
$ENC = [];
foreach ([...MB, ...ST] as $cs) {
    $ENC[$cs] = encodable($cs, $UNIVERSE);
    note(sprintf('  %s encodes %d code points', $cs, count($ENC[$cs])));
}
foreach ([...NEW8, ...OLD8] as $cs) {
    $ENC[$cs] = encodable($cs, $UNIVERSE8);
}
$ENC['UTF-8'] = array_keys($UNIVERSE);
foreach (UNI as $cs) {
    $ENC[$cs] = $ENC['UTF-8'];
}
$ENCSET = [];
foreach ($ENC as $cs => $list) {
    $ENCSET[$cs] = array_flip($list);
}
$OTHER = array_keys($UNIVERSE);

note('measuring what //TRANSLIT replaces');
$TRANSLIT = [];
for ($cp = 0x80; $cp <= 0x1ffff; $cp++) {
    if (($cp >= 0xd800 && $cp <= 0xdfff) || ($cp >= 0x10000 && $cp < 0x1d400) || ($cp >= 0x1d800 && $cp < 0x1f100)
        || $cp >= 0x1f300) {
        continue;
    }
    [$r] = call('UTF-32BE', 'ASCII//TRANSLIT', pack('N', $cp));
    if ($r !== false && $r !== '?') {
        $TRANSLIT[] = $cp;
    } elseif (!isset($ENCSET['EUC-JP'][$cp])) {
        [$r] = call('UTF-32BE', 'EUC-JP//TRANSLIT', pack('N', $cp));
        if ($r !== false && $r !== '?') {
            $TRANSLIT[] = $cp;
        }
    }
}
note(sprintf('  %d code points have a replacement', count($TRANSLIT)));

/** Code points both charsets have: what $from decodes and $to encodes. */
function common(string $from, string $to): array
{
    global $T, $ENC, $ENCSET;
    $src = in_array($from, ST, true) ? $ENC[$from] : array_keys($T[$from]['bycp']);
    $out = [];
    foreach ($src as $cp) {
        if (isset($ENCSET[$to][$cp])) {
            $out[] = $cp;
        }
    }
    return $out;
}

$COMMON = [];
foreach ([...MB, ...ST] as $a) {
    foreach ([...MB, ...ST] as $b) {
        if ($a !== $b) {
            $COMMON[$a][$b] = common($a, $b);
        }
    }
}

// The edge cases of each target, as code point sequences.
$SPECIAL = [];
$euro = [[0x20ac], [0x00a4], [0x00a2], [0x00a3], [0x00a5], [0x20a9], [0x20aa], [0x20b9], [0x20bd], [0x00ae],
    [0x00a9], [0x2122], [0x2116], [0x00a0], [0x00ad], [0xfffd], [0xfeff], [0x0080], [0x009f], [0x00b5], [0x0152],
    [0x0153], [0x0160], [0x0178], [0x017d], [0x0192], [0x02c6], [0x02dc], [0x2013], [0x2014], [0x2018], [0x2019],
    [0x201a], [0x201c], [0x201d], [0x201e], [0x2020], [0x2021], [0x2022], [0x2026], [0x2030], [0x2039], [0x203a],
    [0x20b4], [0x20b8], [0x20ba], [0x20be], [0x2010], [0x00a6], [0x00a7], [0x00b0], [0x00b1], [0x00d7], [0x00f7]];
$SPECIAL['*'] = $euro;
$jp = [[0x5c], [0x7e], [0xa5], [0x203e], [0x301c], [0xff5e], [0x2016], [0x2225], [0x2212], [0xff0d], [0xa2], [0xffe0],
    [0xa3], [0xffe1], [0xac], [0xffe2], [0xa6], [0xffe4], [0x2014], [0x2015], [0xff3c], [0xff07], [0xff02], [0x2116],
    [0x2121], [0x3231], [0x2252], [0x2261], [0x222b], [0x221a], [0x22a5], [0x2220], [0x2235], [0x2229], [0x222a],
    [0x2460], [0x2473], [0x2160], [0x2169], [0x2170], [0x2179], [0x3349], [0x3314], [0x339c], [0x222e], [0x221f],
    [0x22bf], [0x2211], [0x3094], [0x30f7], [0x30fa], [0x3000], [0x20ac], [0xfa0e], [0xfa2d], [0x9ed1], [0x7e8a],
    [0xff61], [0xff71], [0xff9e], [0xff9f], [0xff76, 0xff9e], [0x304b, 0x3099], [0xe000], [0xe757], [0xe758],
    [0x00e9], [0x03b1], [0x0436], [0x2500], [0x4e02], [0x4e04], [0x9fa5]];
$cn = [[0x80], [0xa4], [0xa7], [0xb7], [0x30fb], [0x2014], [0x2015], [0x1e3f], [0x01f9], [0xe7c7], [0xe5e5], [0xe000],
    [0xe765], [0xe766], [0xe7ff], [0xe81e], [0xe826], [0xe82b], [0xe82c], [0xe832], [0xe843], [0xe854], [0xe864],
    [0x9fb4], [0x9fbb], [0xfe10], [0xfe19], [0x20087], [0x20089], [0x200cc], [0x215d7], [0x2298f], [0x241fe],
    [0xfffe], [0xffff], [0x10000], [0x10ffff], [0x20ac], [0x3000], [0x2e81], [0x2e84], [0x2fff], [0x303e], [0x4dae],
    [0x4db5], [0x4db6], [0x9fa6], [0x9fbc], [0xff5e], [0x2225], [0xe5e4], [0xe78d], [0xe796], [0xfa0c], [0xfa29],
    [0xe0001], [0x1f600]];
$tw = [[0x00ca], [0x00ea], [0x00ca, 0x0304], [0x00ca, 0x030c], [0x00ea, 0x0304], [0x00ea, 0x030c], [0x0304], [0x030c],
    [0x00ca, 0x0300], [0x00ca, 0x00ca, 0x0304], [0x00ca, 0x0301], [0x00ea, 0x00ea], [0x20000], [0x2a6d6], [0x27607],
    [0x43f0], [0x2027], [0x2022], [0xffe3], [0x02cd], [0x5140], [0x55c0], [0x20ac], [0x2460], [0x2554], [0x2566],
    [0x2557], [0x78a7], [0x92b9], [0x88cf], [0x58bb], [0x6052], [0x7ca7], [0x5afa], [0xe000], [0xf848], [0xf849],
    [0x3000], [0xfe5d], [0x2550], [0x256d], [0x2571], [0xff3f], [0x2574], [0x00af], [0x2215], [0xfe68], [0x5341],
    [0x5345], [0x8ff9], [0x2f00]];
$kr = [[0x327e], [0x20ac], [0xae], [0x3164], [0x20a9], [0xffe6], [0xac00], [0xd7a3], [0xac01], [0xd7a4], [0x1100],
    [0x3131], [0x3165], [0x318e], [0xb7], [0x2015], [0x223c], [0xff5e], [0x2016], [0x2225], [0xe000], [0x5c],
    [0x7e], [0xa5], [0x2116], [0x3200], [0x321c], [0x3260], [0x327b], [0x2160], [0x2170], [0x3380], [0x33dd],
    [0xf900], [0xfa0b], [0x4e00], [0x8a1b], [0x1161], [0x11a8], [0x115f], [0x1160]];
foreach (['SJIS', 'CP932', 'EUC-JP', 'EUC-JP-MS', 'ISO-2022-JP'] as $cs) {
    $SPECIAL[$cs] = [...$jp, ...$euro];
}
foreach (['GBK', 'GB18030', 'EUC-CN'] as $cs) {
    $SPECIAL[$cs] = [...$cn, ...$euro];
}
foreach (['BIG5', 'BIG5-HKSCS', 'EUC-TW'] as $cs) {
    $SPECIAL[$cs] = [...$tw, ...$euro];
}
foreach (['EUC-KR', 'UHC', 'JOHAB', 'ISO-2022-KR'] as $cs) {
    $SPECIAL[$cs] = [...$kr, ...$euro];
}
$tags = [[0xe0001, 0xe006a, 0xe0061], [0xe0001, 0xe006b, 0xe006f], [0xe0001, 0xe007a, 0xe0068],
    [0xe0001, 0xe0065, 0xe006e], [0xe0001, 0xe007f], [0xe0001], [0xe007f], [0xe0041], [0xe0020], [0xe0080]];
$SPECIAL['ISO-2022-JP-2'] = [...$jp, ...$kr, ...$cn, ...$tags, ...$tags, [0xe9], [0x3b1], [0xa0], [0xff],
    [0x384], [0x3a9]];

// ---------------------------------------------------------------------------------------------------------------
// The cases

mt_srand(SEED + 1);
$CASES = [];
$SECTIONS = [];

function section(string $name): void
{
    global $CASES, $SECTIONS;
    $SECTIONS[] = [count($CASES), $name];
    note("section $name");
}

function add(string $from, string $to, string|array $input): void
{
    global $CASES;
    $CASES[] = [$from, $to, $input];
}

$BEFORE_UNI = ['UTF-8', 'UTF-8', 'UTF-8', 'UTF-8', 'UTF-8', 'UTF-8', 'UTF-8', 'UTF-8', 'UTF-16', 'UTF-16LE',
    'UTF-16BE', 'UCS-2', 'UCS-2BE', 'UCS-2LE', 'UTF-32', 'UTF-32LE', 'UTF-32BE', 'UCS-4', 'UCS-4LE', 'UCS-4BE'];

/** A charset the converter had before, mostly UTF-8. */
function before(): string
{
    global $BEFORE_UNI;
    return pct(80) ? pick($BEFORE_UNI) : pick(OLD8);
}

section('multi-byte to the charsets before');
foreach (MB as $cs) {
    for ($i = 0; $i < 1380; $i++) {
        $to = before();
        add(nm($cs), nm($to), bytes_in($cs, pct(55), null));
    }
}

section('the charsets before to multi-byte');
foreach (MB as $cs) {
    for ($i = 0; $i < 1380; $i++) {
        $from = before();
        $noise = pct(20);
        $cps = cps_for($cs, len());
        if (!in_array($from, UNI, true)) {
            $cps = array_values(array_filter($cps, static fn ($c) => isset($GLOBALS['ENCSET'][$from][$c])));
            if (!$cps || pct(30)) {
                $cps[] = pick($ENC[$from]);
            }
        }
        add(nm($from), nm($cs), source_bytes($from, $cps, $noise));
    }
}

section('multi-byte to multi-byte');
foreach (MB as $a) {
    foreach (MB as $b) {
        if ($a !== $b) {
            for ($i = 0; $i < 30; $i++) {
                add(nm($a), nm($b), bytes_in($a, pct(30), $b));
            }
        }
    }
}

section('ISO-2022 to the charsets before');
foreach (ST as $cs) {
    for ($i = 0; $i < 1500; $i++) {
        add(nm($cs), nm(before()), st_bytes($cs, len(), pct(45)));
    }
}

section('the charsets before to ISO-2022');
foreach (ST as $cs) {
    for ($i = 0; $i < 1500; $i++) {
        $from = before();
        $cps = cps_for($cs, len());
        if (!in_array($from, UNI, true)) {
            $cps = array_values(array_filter($cps, static fn ($c) => isset($GLOBALS['ENCSET'][$from][$c])));
            if (!$cps || pct(30)) {
                $cps[] = pick($ENC[$from]);
            }
        }
        add(nm($from), nm($cs), source_bytes($from, $cps, pct(15)));
    }
}

section('ISO-2022 and multi-byte');
foreach (ST as $s) {
    foreach (MB as $m) {
        for ($i = 0; $i < 25; $i++) {
            add(nm($s), nm($m), bytes_in($s, pct(30), $m));
            add(nm($m), nm($s), bytes_in($m, pct(30), $s));
        }
    }
    foreach (ST as $s2) {
        if ($s !== $s2) {
            for ($i = 0; $i < 60; $i++) {
                add(nm($s), nm($s2), bytes_in($s, pct(30), $s2));
            }
        }
    }
}

section('single-byte to the charsets before');
foreach (NEW8 as $cs) {
    for ($i = 0; $i < 12; $i++) {
        add(nm($cs), nm(before()), mb_bytes($T[$cs], len(), pct(30)));
    }
}

section('the charsets before to single-byte');
foreach (NEW8 as $cs) {
    for ($i = 0; $i < 12; $i++) {
        $from = pct(85) ? pick($BEFORE_UNI) : pick(OLD8);
        $cps = [];
        $n = len();
        for ($j = 0; $j < $n; $j++) {
            $cps[] = match (wpick(['rep' => 70, 'translit' => 12, 'other' => 10, 'euro' => 8])) {
                'rep' => pick($ENC[$cs] ?: [0x41]),
                'translit' => pick($TRANSLIT),
                'other' => pick($OTHER),
                default => pick($euro)[0],
            };
        }
        if (!in_array($from, UNI, true)) {
            $cps = array_values(array_filter($cps, static fn ($c) => isset($GLOBALS['ENCSET'][$from][$c])));
            $cps[] = pick($ENC[$from]);
        }
        add(nm($from), nm($cs), source_bytes($from, $cps, pct(10)));
    }
}

section('single-byte and multi-byte or ISO-2022');
for ($i = 0; $i < 2000; $i++) {
    $sb = pick(NEW8);
    $other = pct(70) ? pick([...MB, ...ST]) : pick(NEW8);
    if (pct(50)) {
        // single-byte to the other: characters both have, mostly
        $common = [];
        foreach (array_keys($T[$sb]['bycp']) as $cp) {
            if (isset($ENCSET[$other][$cp])) {
                $common[] = $cp;
            }
        }
        $cps = [];
        $n = len();
        for ($j = 0; $j < $n; $j++) {
            $cps[] = $common && pct(80) ? pick($common) : pick(array_keys($T[$sb]['bycp']));
        }
        $s = mb_encode($T[$sb], $cps);
        if (pct(15)) {
            $s .= pick($T[$sb]['bad']);
        }
        add(nm($sb), nm($other), $s);
    } else {
        $cps = [];
        $n = len();
        $src = in_array($other, ST, true) ? $ENC[$other] : array_keys($T[$other]['bycp']);
        for ($j = 0; $j < $n; $j++) {
            $cps[] = pct(55) && $ENC[$sb] ? pick($ENC[$sb]) : pick($src);
        }
        if (in_array($other, ST, true)) {
            $cps = array_values(array_filter($cps, static fn ($c) => isset($GLOBALS['ENCSET'][$other][$c])));
            [$s] = call('UTF-32BE', $other, pack('N*', ...$cps));
            if ($s === false) {
                continue;
            }
        } else {
            $cps = array_values(array_filter($cps, static fn ($c) => isset($GLOBALS['T'][$other]['bycp'][$c])));
            $s = mb_encode($T[$other], $cps);
        }
        add(nm($other), nm($sb), $s);
    }
}

section('long inputs around the rounds');
$LONG_TARGETS = ['UTF-8', 'UTF-8', 'UTF-16LE', 'UCS-4BE', 'UTF-16', 'ISO-8859-1', 'CP1252'];
for ($i = 0; $i < 900; $i++) {
    $kind = wpick(['decode' => 40, 'encode' => 40, 'cross' => 20]);
    if ($kind === 'decode') {
        // A charset under test to one the converter had before.
        $from = pct(15) ? pick(ST) : (pct(85) ? pick(MB) : pick(NEW8));
        $to = pick($LONG_TARGETS);
        if (in_array($from, ST, true)) {
            [$fillers, $specials, $head, $tail] = long_st_parts($from);
            $input = long_input($fillers, $specials, long_total());
            array_unshift($input, [$head, 1]);
            $input[] = [$tail, 1];
            add(nm($from), nm($to), recipe($input));
            continue;
        }
        $t = $T[$from];
        $fillers = [];
        $nf = mt_rand(1, 3);
        for ($j = 0; $j < $nf; $j++) {
            $fillers[] = $t['multi'] && pct(75) ? pick(pick($t['multi'])) : pick($t['single']);
        }
        $specials = [];
        $ns = mt_rand(0, 4);
        for ($j = 0; $j < $ns; $j++) {
            $specials[] = match (wpick(['bad' => 30, 'partial' => 15, 'valid' => 40, 'byte' => 15])) {
                'bad' => pick($t['bad']),
                'partial' => $t['partial'] ? pick($t['partial']) : pick($t['bad']),
                'valid' => $t['multi'] ? pick(pick($t['multi'])) : pick($t['single']),
                default => chr(mt_rand(0, 255)),
            };
        }
        $input = long_input($fillers, $specials, long_total());
        if (pct(15) && $t['partial']) {
            $input[] = [pick($t['partial']), 1];
        }
        add(nm($from), nm($to), $input);
    } elseif ($kind === 'encode') {
        // One the converter had before to a charset under test.
        $to = pct(20) ? pick(ST) : (pct(80) ? pick(MB) : pick(NEW8));
        $from = pick(['UTF-8', 'UTF-8', 'UTF-8', 'UTF-16LE', 'UTF-16BE', 'UCS-4LE', 'UTF-32BE', 'UCS-2LE']);
        $le = str_ends_with($from, 'LE');
        $char = static function (int $cp) use ($from, $le): string {
            return match (true) {
                $from === 'UTF-8' => u8($cp),
                str_starts_with($from, 'UTF-16'), str_starts_with($from, 'UCS-2') => u16($cp, $le),
                default => u32($cp, $le),
            };
        };
        $rep = $ENC[$to] ?: [0x41];
        if (str_starts_with($from, 'UCS-2')) {
            $rep = array_values(array_filter($rep, static fn ($c) => $c <= 0xffff)) ?: [0x41];
        }
        $fillers = [];
        $nf = mt_rand(1, 3);
        for ($j = 0; $j < $nf; $j++) {
            $fillers[] = $char(pct(20) ? mt_rand(0x20, 0x7e) : pick($rep));
        }
        $specials = [];
        $ns = mt_rand(0, 4);
        for ($j = 0; $j < $ns; $j++) {
            $what = wpick(['translit' => 25, 'other' => 20, 'special' => 25, 'bad' => 15, 'rep' => 15]);
            $specials[] = match ($what) {
                'translit' => $char(pick($TRANSLIT)),
                'other' => $char(pick($OTHER)),
                'special' => implode('', array_map($char, pick($SPECIAL[$to] ?? $SPECIAL['*']))),
                'bad' => $from === 'UTF-8' ? pick(BAD_UTF8) : (str_starts_with($from, 'UTF-16')
                    || str_starts_with($from, 'UCS-2') ? bad16($le) : bad32($le)),
                default => $char(pick($rep)),
            };
        }
        add(nm($from), nm($to), long_input($fillers, $specials, long_total()));
    } else {
        // Between charsets under test.
        $from = pick(MB);
        $to = pct(25) ? pick(ST) : pick(MB);
        if ($to === $from) {
            $to = 'UTF-8';
        }
        $common = $COMMON[$from][$to] ?? array_keys($T[$from]['bycp']);
        if (!$common) {
            continue;
        }
        $t = $T[$from];
        $fillers = [];
        $nf = mt_rand(1, 3);
        for ($j = 0; $j < $nf; $j++) {
            $fillers[] = pick($t['bycp'][pick($common)]);
        }
        $specials = [];
        $ns = mt_rand(0, 3);
        for ($j = 0; $j < $ns; $j++) {
            $specials[] = pct(50) ? pick($t['bycp'][pick(array_keys($t['bycp']))]) : pick($t['bad']);
        }
        add(nm($from), nm($to), long_input($fillers, $specials, long_total()));
    }
}

/** Fillers, specials, head and tail of a long ISO-2022 input (one mode, with escapes and broken bytes around). */
function long_st_parts(string $cs): array
{
    global $PAIRS;
    if ($cs === 'ISO-2022-KR') {
        $fillers = [];
        for ($j = mt_rand(1, 3); $j > 0; $j--) {
            $fillers[] = pick($PAIRS['ksc5601']);
        }
        $specials = [];
        for ($j = mt_rand(0, 4); $j > 0; $j--) {
            $specials[] = pick(["\x0f\x0e", "\x0fa\x0e", "\n\x0e", pair_any(), "\x80", "\x1b\$)C", "\x0e", "\x21",
                pick($PAIRS['ksc5601'])]);
        }
        return [$fillers, $specials, "\x1b\$)C\x0e", pct(70) ? "\x0f" : ''];
    }
    $sets = ['jis0208' => "\x1b\$B"];
    if ($cs === 'ISO-2022-JP-2') {
        $sets += ['gb2312' => "\x1b\$A", 'ksc5601' => "\x1b\$(C", 'jis0212' => "\x1b\$(D"];
    }
    $keys = array_keys($sets);
    $set = $keys[mt_rand(0, count($keys) - 1)];
    $fillers = [];
    for ($j = mt_rand(1, 3); $j > 0; $j--) {
        $fillers[] = pick($PAIRS[$set]);
    }
    $specials = [];
    for ($j = mt_rand(0, 4); $j > 0; $j--) {
        $specials[] = pick(["\x1b(Ba" . $sets[$set], "\x1b(J\\" . $sets[$set], "\n", pair_any(), "\x80", "\x21",
            "\x1b\$B", "\x1b(B", "\x1b.A\x1bNi", "\x1b", pick($PAIRS[$set])]);
    }
    return [$fillers, $specials, $sets[$set], pct(70) ? "\x1b(B" : ''];
}

section('edge cases: code points into each charset under test');
foreach ([...MB, ...ST] as $cs) {
    foreach ($SPECIAL[$cs] ?? $SPECIAL['*'] as $cps) {
        add('UTF-8', $cs, 'A' . implode('', array_map('u8', $cps)) . 'B');
        if (in_array($cs, ST, true) || pct(30)) {
            $from = pick(['UTF-8', 'UTF-16LE', 'UCS-4BE', 'UTF-16']);
            add($from, $cs, uni_bytes($from, $cps, false));
        }
    }
}

section('edge cases: euro sign and late additions into the single-byte charsets');
foreach (NEW8 as $cs) {
    $list = [[0x20ac]];
    for ($j = 0; $j < 9; $j++) {
        $list[] = pick($euro);
    }
    foreach ($list as $cps) {
        add('UTF-8', $cs, implode('', array_map('u8', $cps)));
    }
}

section('edge cases: byte sequences from the standards and vendor extensions');
/** Hex strings for a range of two-byte codes (every $step-th). */
function codes(int $from, int $to, int $step = 1): array
{
    $out = [];
    for ($c = $from; $c <= $to; $c += $step) {
        $out[] = sprintf($c > 0xffffff ? '%08x' : ($c > 0xffff ? '%06x' : ($c > 0xff ? '%04x' : '%02x')), $c);
    }
    return $out;
}

$EDGE = [
    // NEC special characters (row 13), NEC-selected IBM extensions, IBM extensions, user-defined area, the
    // single bytes outside JIS X 0201, the codes Microsoft and JIS map differently.
    'CP932' => [...codes(0x8740, 0x879c), ...codes(0xed40, 0xeefc, 3), ...codes(0xfa40, 0xfc4b, 3),
        ...codes(0xf040, 0xf9fc, 97), 'f040', 'f07e', 'f080', 'f0fc', 'f9fc', 'f9fd', '80', 'a0', 'fd', 'fe', 'ff',
        '5c', '7e', '815f', '8160', '8161', '817c', '8191', '8192', '81ca', '879d', '879e', 'eeed', 'eeee', 'eeef',
        'eefc', 'eefd', 'fc4b', 'fc4c', 'fcfc', '8540', '86a2', '84bf', '889e', '889f', 'eaa4', 'eaa5', 'a1', 'df'],
    'SJIS' => ['5c', '7e', '815f', '8160', '8161', '817c', '8191', '8192', '81ca', '80', 'a0', 'fd', 'fe', 'ff',
        'f040', 'f9fc', 'fa40', 'ed40', ...codes(0x8740, 0x879c, 7), '8540', '84bf', '889e', '889f', 'eaa4', 'eaa5',
        'a1', 'df', 'e0', 'ef', 'effc', 'f0', '8140', '817f', '81fc', '9ffc', 'e040'],
    // Half-width katakana, JIS X 0212, the NEC row, user-defined areas, IBM extensions of the MS variant.
    'EUC-JP' => [...codes(0x8ea1, 0x8edf, 5), '8ee0', '8e80', '8efe', '8fa2af', '8fa2b7', '8fa2c3', '8fb0a1',
        '8fede3', '8fede4', '8ff3f3', '8ff5a1', '8ffefe', '8fa1a1', 'ada1', 'adfe', 'f5a1', 'fefe', 'a1c1', 'a1c2',
        'a1dd', 'a1f1', 'a1f2', 'a2cc', 'a9a1', 'a1a1', 'f4a6', 'f4a7', '8e', '8f', '8fa2', 'a1', 'ff', '80', '8ea0'],
    'EUC-JP-MS' => [...codes(0x8ea1, 0x8edf, 9), '8fa2b7', '8fa2c3', '8ff3f3', '8ff4fe', '8ff5a1', '8ffefe',
        '8fa1a1', ...codes(0xada1, 0xadfe, 5), 'f5a1', 'fefe', 'a1c1', 'a1c2', 'a1dd', 'a1f1', 'a1f2', 'a2cc',
        '8ff3fe', '8ff4a1', '8ff4ac', '8ff4ad', '8fa2f1', 'a9a1', '8e', '8f', 'a1'],
    // GBK: the euro byte, GB 18030's changed codes, user-defined areas.
    'GBK' => ['80', 'ff', 'a2e3', 'a6d9', 'a6df', 'a6ec', 'a6ed', 'a6f3', 'a8bc', 'a8bf', 'a989', 'a995', 'fe50',
        'fe51', 'fe52', 'fe53', 'fe59', 'fe61', 'fe66', 'fe67', 'fe6c', 'fe6d', 'fe76', 'fe7e', 'fe90', 'fe91', 'fea0',
        'a1a4', 'a1aa', 'aaa1', 'affe', 'f8a1', 'fefe', 'a140', 'a7a0', 'a2ab', 'a2e4', 'a2ef', 'a2fd', 'a4f4',
        'a5f7', 'a8bb', 'a8c1', 'a8c4', '8140', 'fe4f', 'a1a1', 'a3a0', '817f', '81ff', 'a8a1'],
    // GB 18030: the four-byte ranges' ends, the BMP's end, the planes beyond, what lies between and after.
    'GB18030' => ['81308130', '81308139', '81308230', '8130fe39', '81318130', '8431a439', '8431a530', '8431a439',
        '84318730', '84308130', '8431fe39', '85308130', '8f39fe39', '90308130', '9030fe39', 'e3329a35', 'e3329a36',
        'e3329b30', 'fd308130', 'fe308130', 'fe39fe39', '81358437', '81358438', '8135f437', '8336c739', '8336c830',
        '82358f33', '82359134', '84318236', '84318335', '81308140', '81307f30', '8130813a', '8130812f', '8139ee39',
        '8139ef30', '82358731', '95328236', '9a348431', 'a2e3', 'a8bc', 'a8bf', 'a6d9', 'fe51', 'fe52', 'fe6c', 'fe76',
        'fe91', '80', 'ff', '81', '8130', '813081', 'a1a1', 'a3a0', 'a1a4', 'a1aa', '84318131', '8431a438'],
    'EUC-CN' => ['a1a1', 'a1a4', 'a1aa', 'a2a1', 'a2aa', 'a2b0', 'a2e3', 'a6e0', 'a8bb', 'a8bc', 'a8bf', 'a8c0',
        'a9a4', 'a9ef', 'aaa1', 'b0a1', 'd7f9', 'd7fa', 'f7fe', 'f8a1', '80', 'a0', 'ff', 'a1', 'a1ff', 'a140'],
    // Big5: the duplicates, ETEN extensions, CP950's euro and user-defined areas.
    'BIG5' => ['a145', 'a14e', 'a1c3', 'a1c5', 'a1fe', 'a240', 'a2cc', 'a451', 'a2ce', 'a4ca', 'a3e1', 'a3c0',
        'a3fe', 'a15a', 'a1c2', ...codes(0xc6a1, 0xc7fc, 23), 'c879', 'c8d3', ...codes(0xf9d6, 0xf9fe, 3), 'f9fe',
        'fa40', 'fefe', '8e40', 'a0fe', '8140', '8dfe', 'c6a1', 'c8fe', 'f9d5', 'a140', 'a3bf', 'c87e', 'c8a1',
        'a1', '80', 'ff', 'a17f', 'a1a0'],
    // Big5-HKSCS: the composed pairs and their single forms, HKSCS-2008 rows, the planes beyond the BMP.
    'BIG5-HKSCS' => ['8862', '8864', '88a3', '88a5', '8866', '88a7', '8840', '8740', '87df', '87e0', '8740',
        ...codes(0x8840, 0x88aa, 5), ...codes(0x8940, 0x89fe, 17), 'fefe', 'c6a1', 'c8a4', 'c8fe', 'f9fe', 'a3e1',
        'a145', '8140', '8640', 'a0fe', '87', '88', '8862a1', '88628864', '886288a5', 'a3c0', 'c87e', '8bfe',
        '8e69', '8e6f', '8e7e', '8eab', '8eb4', '8ecd', '8fcb', '8ffe'],
    // KS X 1001: the euro and registered signs of 1998, U+327E of 2002, user-defined rows.
    'EUC-KR' => ['a2e6', 'a2e7', 'a2e8', 'a2e5', 'a1a4', 'a1aa', 'a1ad', 'a2a6', 'a4d4', 'c9a1', 'c9fe', 'fea1',
        'fefe', 'a1a1', 'b0a1', 'c8fe', 'cad0', 'fdfe', '80', 'a1', 'ff', 'a1ff', 'a141', 'a4a1', 'a4fe'],
    // UHC: its own Hangul rows, the A2 E8 code, trail bytes between the letter runs.
    'UHC' => ['8141', '8161', '8181', 'a041', 'c652', 'c653', 'c6a1', 'a2e8', 'a2e6', 'a2e7', '815b', '8160',
        '817b', '8180', 'fe41', 'c7a1', 'a1a1', 'a2e8a1', 'a2e8a2', 'a2e841', '8141a2e8', 'a2', 'a2e8a2e8', '80',
        'ff', '81', 'c8fe', 'fefe'],
    // Johab: Hangul by its bits, fillers, the symbol and Hanja areas, the won sign.
    'JOHAB' => ['8861', '8441', '8444', '8841', '8461', 'd3bd', 'd3b7', 'd931', 'd9e6', 'def1', 'e031', 'f9fe', 'd831',
        'd8fe', '5c', 'd4a1', 'd7fe', '8442', '8481', 'a1a1', '8000', '84', 'd9', 'ff', '80', 'dade', 'd97e',
        'd991', '9461', 'b4a1', 'c541'],
    // CNS 11643: plane 1 both ways, plane 2, planes 3 to 7, the planes beyond, truncations.
    'EUC-TW' => ['a1a1', 'fdcb', 'fdcc', 'c4a1', '8ea1a1a1', '8ea1fdcb', '8ea2a1a1', '8ea2f2c4', '8ea3a1a1',
        '8ea4a1a1', '8ea5a1a1', '8ea6a1a1', '8ea7a1a1', '8ea7f3e5', '8ea8a1a1', '8eb0a1a1', '8eb1a1a1', '8ea0a1a1',
        '8e', '8ea2', '8ea2a1', 'a1', '80', 'ff', 'fefe', 'a1fe', '8ea2a1fe', '8eaba1a1', '8eafa1a1'],
];
foreach ($EDGE as $cs => $list) {
    foreach (array_unique($list) as $h) {
        $seq = hex2bin($h);
        add($cs, 'UTF-8', 'A' . $seq . 'B');
        add($cs, pick(['UTF-16LE', 'UCS-4BE', 'UTF-8']), $seq);
        if (pct(25)) {
            $to = pick(MB);
            add($cs, $to === $cs ? 'UTF-8' : $to, $seq);
        }
    }
}
// The other names of these charsets resolve to them.
foreach ([...MB, ...ST] as $cs) {
    foreach ($ALIASES[$cs] as $name) {
        add($name, 'UTF-8', $cs === 'ISO-2022-KR' ? "\x1b\$)C\x0e\x30\x21\x0fa" : bytes_in($cs, false, null));
        add('UTF-8', $name, "a\xe3\x81\x82\xe4\xb8\x80\xea\xb0\x80\xc3\xa9\xe2\x82\xac");
    }
}

section('edge cases: ISO-2022');
$ISO = [
    // Encoding: the return to ASCII at the end, JIS X 0201 Roman and katakana, empty input, newlines.
    ['UTF-8', 'ISO-2022-JP', ["\xe3\x81\x82", "\xc2\xa5", "\xe2\x80\xbe", "\xc2\xa5a", "a\xc2\xa5", "\xc2\xa5\xe3\x81\x82",
        "\xef\xbd\xb1", "\xef\xbd\xa1", "\xef\xbe\x9f", "\xef\xbd\xb6\xef\xbe\x9e", "", "a", "\n", "\xe3\x81\x82\n",
        "\xe3\x81\x82\r\n\xe3\x81\x82", "\\~", "\xc2\xa5\\", "\xe2\x80\xbe~", "\xe3\x80\x9c", "\xef\xbd\x9e",
        "\xe2\x80\x96", "\xe2\x88\xa5", "\xe2\x88\x92", "\xef\xbc\x8d", "\xc3\xa9", "\xe4\xb8\x82", "\xe2\x91\xa0",
        "\xe3\x8d\x89", "\xf0\x9f\x98\x80", "\xe3\x81\x82\xf0\x9f\x98\x80", "\xc3\xa9\xe3\x81\x82", "\x1b",
        "\x1b(B", "\x0e", "\xe3\x8e\xa8", "a\xe3\x8e\xa8", "\xe3\x81\x82\xe3\x8e\xa8"]],
    ['UTF-8', 'ISO-2022-JP-2', ["\xe3\x81\x82", "\xc3\xa9", "\xce\xb1", "\xc3\xa9\xce\xb1", "\xc2\xa0", "\xc3\xbf",
        "\xce\x84", "\xe4\xb8\x82", "\xea\xb0\x80", "\xe4\xb8\x8e", "\xe4\xb8\x8e\xea\xb0\x80\xe3\x81\x82",
        "\xc2\xa5", "\xe2\x80\xbe", "\xef\xbd\xb1", "", "\xc3\xa9\n\xc3\xa9", "\xf3\xa0\x80\x81\xf3\xa0\x81\xab\xf3\xa0\x81\xaf\xe4\xb8\x80",
        "\xf3\xa0\x80\x81\xf3\xa0\x81\xba\xf3\xa0\x81\xa8\xe4\xb8\x80", "\xf3\xa0\x80\x81\xf3\xa0\x81\xaa\xf3\xa0\x81\xa1\xe4\xb8\x80",
        "\xf3\xa0\x80\x81\xf3\xa0\x81\xbf", "\xf3\xa0\x80\x81", "\xd0\x96", "\xe2\x82\xac", "\xe3\x8e\xa8"]],
    ['UTF-8', 'ISO-2022-KR', ["", "a", "\xea\xb0\x80", "\xea\xb0\x80a", "a\xea\xb0\x80", "\xea\xb0\x80\n\xea\xb0\x80",
        "\xea\xb0\x80\r\n", "\xc3\xa9", "\xe3\x81\x82", "\xe2\x82\xac", "\xe3\x89\xbe", "\xc2\xae", "\xe2\x82\xa9",
        "\x0e", "\x0f", "\x1b", "\x1b\$)C", "\xea\xb0\x81", "\xf0\x9f\x98\x80", "\xea\xb0\x80\xf0\x9f\x98\x80"]],
    // Decoding: designations, an end without the return to ASCII, katakana, the 1978 set, broken escapes.
    ['ISO-2022-JP', 'UTF-8', ["\x1b\$B\x24\x22\x1b(B", "\x1b\$B\x24\x22", "\x1b(J\x5c\x7e\x1b(B", "\x1b(J\x5c",
        "\x1b(I\x31", "\x1b(I\x31\x1b(B", "\x1b\$@\x30\x21\x1b(B", "\x1b&@\x1b\$B\x30\x21\x1b(B",
        "\x1b\$(B\x30\x21", "\x1b\$(D\x30\x21", "\x1b\$A\x30\x21", "\x1b\$B\x24", "\x1b\$B\x24\x22\x24",
        "\x1b\$B\n\x24\x22", "\x1b\$B\x24\x22\n", "\x1b", "\x1b(", "\x1b\$", "\x1bN", "\x0e\x0f", "\x80",
        "a\x1b(B", "\x1b(Ba", "\x1b\$B\x1b(B", "\x1b\$B\x7f\x21", "\x1b\$B\x21\x7f", "\x1b\$B\x20\x24\x22",
        "\x1b\$B\x22\x2f\x1b(B", "\x1b\$B\x2d\x21\x1b(B", "\x1b\$B\x74\x26\x1b(B", "\x1b\$B\x74\x27\x1b(B",
        "\x1b(J\x80", "\x1b.A\x1bNi", "\x1b\$B\x30\x21\x1b(J\x30\x1b(B"]],
    ['ISO-2022-JP-2', 'UTF-8', ["\x1b.A\x1bNi", "\x1b.F\x1bNa", "\x1bNi", "\x1b.A\x1bN\x20", "\x1b.A\x1bN\x7f",
        "\x1b.A\x1bN\x80", "\x1b.A\x1bN", "\x1b\$A\x30\x21\x1b(B", "\x1b\$(C\x30\x21\x1b(B", "\x1b\$(D\x30\x21\x1b(B",
        "\x1b\$(D\x22\x2f", "\x1b(I\x31", "\x1b\$(C\x30\x21\n\x30\x21", "\x1b.A\n\x1bNi", "\x1b.A\x1b.F\x1bNa",
        "\x1b\$B\x24\x22\x1b.A\x1bNi\x24\x22\x1b(B", "\x1b\$)C", "\x1b\$(A\x30\x21", "\x1b.B\x1bNi"]],
    ['ISO-2022-KR', 'UTF-8', ["\x1b\$)C\x0e\x30\x21\x0f", "\x1b\$)C\x0e\x30\x21", "\x0e\x30\x21\x0f", "\x1b\$)C",
        "\x1b\$)Ca", "a\x1b\$)C\x0e\x30\x21\x0f", "\x1b\$)C\x1b\$)C\x0e\x30\x21\x0f", "\x1b\$)C\x0e\x30\x21\n\x30\x21",
        "\x1b\$)C\x0e\x30\x21\n\x0f", "\x1b\$)C\x0e\x30", "\x1b\$)C\x0e\x0e\x30\x21", "\x1b\$)C\x0f\x0f",
        "\x1b\$)C\x80", "\x1b\$)C\x0e\x80\x0f", "\x1b\$)C\x0e\x22\x68\x0f", "\x1b\$)C\x0e\x22\x66\x22\x67\x0f",
        "\x1b\$)C\x0e\x7f\x21\x0f", "\x1b\$)", "\x1b", "", "\x1b(B", "\x1b\$)C\x0e\x20\x0f", "\x1b\$)C\x0e\x21\x20"]],
];
foreach ($ISO as [$from, $to, $inputs]) {
    foreach ($inputs as $in) {
        add($from, $to, $in);
        if (pct(40)) {
            add($from, $to === 'UTF-8' ? pick(['UTF-16BE', 'EUC-JP', 'SJIS', 'EUC-KR', 'UCS-4']) : $to, $in . $in);
        }
    }
}

/** $cps in a Unicode charset with its byte order in the name (or UTF-8), without a byte order mark. */
function chars(string $from, array $cps): string
{
    $le = str_ends_with($from, 'LE');
    $s = '';
    foreach ($cps as $cp) {
        $s .= match (true) {
            $from === 'UTF-8' => u8($cp),
            str_starts_with($from, 'UTF-16'), str_starts_with($from, 'UCS-2') => u16($cp, $le),
            default => u32($cp, $le),
        };
    }
    return $s;
}

/** Repeat counts for a unit of $len characters: a sweep of short ones (where PHP's buffer runs out), rounds, any. */
function repeats(int $len): int
{
    return match (wpick(['sweep' => 45, 'round' => 30, 'any' => 25])) {
        'sweep' => mt_rand(1, 120),
        'round' => max(1, intdiv(8160 * mt_rand(1, 3), $len) + mt_rand(-3, 3)),
        default => mt_rand(1, max(1, intdiv(24000, $len))),
    };
}

note('measuring //TRANSLIT replacements per target');
$XL_TARGETS = [...MB, ...ST, 'ASCII', 'ISO-8859-1', 'KOI8-T', 'TIS-620', 'IBM037', 'ISO646-DE', 'VISCII', 'CP437',
    'ARMSCII-8', 'GEORGIAN-PS', 'MAC-CYRILLIC', 'HP-ROMAN8', 'IBM1047', 'BRF', 'CSISO2033', 'TCVN-5712', 'CP1258'];
$REPLACED = [];
$EXPANDING = [];
foreach ($XL_TARGETS as $cs) {
    $REPLACED[$cs] = [];
    $EXPANDING[$cs] = [];
    foreach ($TRANSLIT as $cp) {
        if (isset($ENCSET[$cs][$cp])) {
            continue;
        }
        [$r] = call('UTF-32BE', $cs . '//TRANSLIT', pack('N', $cp));
        if ($r !== false && $r !== '' && $r !== '?') {
            $REPLACED[$cs][] = $cp;
            if (strlen($r) > strlen(u8($cp))) {
                $EXPANDING[$cs][] = $cp;
            }
        }
    }
}

// The character sets of the ISO-2022 targets, told apart by what each charset encodes.
$J = $ENCSET['ISO-2022-JP'];
$J2 = $ENCSET['ISO-2022-JP-2'];
$SETS = [
    'ascii' => range(0x21, 0x7e),
    'roman' => [0xa5, 0x203e],
    'jis0208' => array_values(array_filter($ENC['ISO-2022-JP'], static fn ($c) => $c >= 0x100 && $c !== 0x203e)),
    'gb' => array_values(array_filter($ENC['EUC-CN'],
        static fn ($c) => $c >= 0x100 && !isset($J[$c]) && isset($J2[$c]))),
    'ksc' => array_values(array_filter($ENC['EUC-KR'],
        static fn ($c) => $c >= 0x100 && !isset($J[$c]) && !isset($ENCSET['EUC-CN'][$c]) && isset($J2[$c]))),
    'jis212' => [],
    'latin1' => array_values(array_filter(range(0xa0, 0xff), static fn ($c) => !isset($J[$c]) && isset($J2[$c]))),
    'greek' => array_values(array_filter(range(0x370, 0x3ff), static fn ($c) => !isset($J[$c]) && isset($J2[$c]))),
    'hangul' => array_values(array_filter($ENC['ISO-2022-KR'], static fn ($c) => $c >= 0x100)),
    'kana' => range(0xff61, 0xff9f),
    'nope' => [0x1f600, 0x0e01, 0x10d0, 0x0531, 0x05d0, 0x0627, 0x1e9e, 0x2c00, 0xa000, 0x20bf, 0x10348],
];
foreach ($T['EUC-JP']['bycp'] as $cp => $seqs) {
    foreach ($seqs as $s) {
        if (strlen($s) === 3 && $s[0] === "\x8f" && !isset($J[$cp]) && isset($J2[$cp])) {
            $SETS['jis212'][] = $cp;
            break;
        }
    }
}
const TAGSEQ = [[0xe0001, 0xe006a, 0xe0061], [0xe0001, 0xe006b, 0xe006f], [0xe0001, 0xe007a, 0xe0068],
    [0xe0001, 0xe007a, 0xe0068, 0xe002d, 0xe0074, 0xe0077], [0xe0001, 0xe0065, 0xe006e], [0xe0001, 0xe007f],
    [0xe006a, 0xe0061], [0xe0001], [0xe0001, 0xe006a], [0xe0001, 0xe004a, 0xe0041], [0xe0001, 0xe006b, 0xe006f, 0xe007f],
    [0xe007f]];
$ST_SETS = [
    'ISO-2022-JP' => ['ascii', 'roman', 'jis0208', 'jis0208', 'kana', 'nope', 'translit'],
    'ISO-2022-JP-2' => ['ascii', 'roman', 'jis0208', 'gb', 'ksc', 'jis212', 'latin1', 'greek', 'kana', 'nope',
        'translit', 'tags'],
    'ISO-2022-KR' => ['ascii', 'hangul', 'hangul', 'nope', 'translit'],
];
foreach ($SETS as $k => $list) {
    note(sprintf('  set %s: %d', $k, count($list)));
}

/** Code points from one of the sets of an ISO-2022 target. */
function from_set(string $target, string $set): array
{
    global $SETS, $REPLACED;
    return match ($set) {
        'translit' => [pick($REPLACED[$target] ?: [0x2474])],
        'tags' => pick(TAGSEQ),
        default => [pick($SETS[$set] ?: [0x41])],
    };
}

section('ISO-2022 targets: alternating sets, the output buffer running out within escapes and shifts');
foreach (ST as $cs) {
    for ($i = 0; $i < 170; $i++) {
        $from = pick(['UTF-8', 'UTF-8', 'UTF-8', 'UTF-16LE', 'UCS-4BE']);
        $unit = [];
        for ($j = mt_rand(2, 4); $j > 0; $j--) {
            array_push($unit, ...from_set($cs, pick($ST_SETS[$cs])));
        }
        $pre = array_fill(0, mt_rand(0, 7), 0x61);
        $post = pct(40) ? from_set($cs, pick($ST_SETS[$cs])) : [];
        add($from, nm($cs), recipe([[chars($from, $pre), 1], [chars($from, $unit), repeats(count($unit))],
            [chars($from, $post), 1]]));
    }
}

section('ISO-2022: //TRANSLIT replacements after a two-byte set where the output buffer runs out');
foreach (['UTF-8', 'UTF-16LE', 'UCS-4BE'] as $from) {
    $units = [[0x3042, 0x33a8], [0x6f22, 0x33a8], [0x33a8, 0x3042], [0x3042, 0x33a8, 0x33a8], [0x3042, 0x2474],
        [0x3042, 0xfb03], [0x3042, 0x00c6], [0xa5, 0x33a8], [0x3042, 0xa5, 0x33a8], [0x3042, 0x3315]];
    foreach ($units as $unit) {
        for ($n = 1; $n <= ($from === 'UTF-8' ? 40 : 14); $n++) {
            add($from, 'ISO-2022-JP', recipe([[chars($from, $unit), $n]]));
        }
    }
}
foreach ([['ISO-2022-JP-2', [0x3042, 0x33a8]], ['ISO-2022-JP-2', [0x4e0e, 0xc6]], ['ISO-2022-JP-2', [0xac00, 0x2474]],
    ['ISO-2022-KR', [0xac00, 0x3315]], ['ISO-2022-KR', [0xac00, 0x337f]], ['ISO-2022-KR', [0x61, 0xfb03]],
    ['ISO-2022-KR', [0xac00, 0x1f600]]] as [$to, $unit]) {
    for ($n = 1; $n <= 30; $n++) {
        add('UTF-8', $to, recipe([[chars('UTF-8', $unit), $n]]));
    }
}

section('//TRANSLIT replacements longer than their input, with the output buffer running out');
foreach ($XL_TARGETS as $cs) {
    if (in_array($cs, ST, true) || !$EXPANDING[$cs]) {
        continue;
    }
    for ($i = 0; $i < 30; $i++) {
        $from = pick(['UTF-8', 'UTF-8', 'UTF-16LE']);
        $unit = [pick($EXPANDING[$cs])];
        if (pct(50)) {
            $unit[] = pct(50) ? pick($EXPANDING[$cs]) : pick($ENC[$cs] ?: [0x41]);
        }
        $pre = array_fill(0, mt_rand(0, 9), pick($ENC[$cs] ?: [0x41]));
        add($from, nm($cs), recipe([[chars($from, $pre), 1], [chars($from, $unit), repeats(count($unit))]]));
    }
}

section('BIG5-HKSCS: composed pairs and held characters around the rounds and the output buffer');
$hk = [[0xca], [0xca, 0x304], [0xea, 0x30c], [0xca, 0x41], [0x41, 0xca], [0xca, 0xca, 0x304], [0x4e2d, 0xca],
    [0xca, 0x300], [0xca, 0x1f600], [0xea], [0xea, 0x304, 0xca, 0x30c], [0xca, 0x4e2d], [0xca, 0x2474]];
for ($i = 0; $i < 260; $i++) {
    $unit = pick($hk);
    $from = pick(['UTF-8', 'UTF-8', 'UTF-16LE', 'UCS-4LE']);
    $len = count($unit);
    $n = match (wpick(['r1' => 30, 'r2' => 30, 'sweep' => 25, 'any' => 15])) {
        'r1' => intdiv(8160, $len) + mt_rand(-3, 3),
        'r2' => intdiv(16320 * mt_rand(1, 2), $len) + mt_rand(-3, 3),
        'sweep' => mt_rand(1, 60),
        default => mt_rand(1, intdiv(40000, $len)),
    };
    $pre = array_fill(0, mt_rand(0, 3), 0x41);
    $post = pct(50) ? [] : pick([[0x304], [0x41], [0xca], [0x30c], [0x1f600]]);
    add($from, 'BIG5-HKSCS', recipe([[chars($from, $pre), 1], [chars($from, $unit), $n], [chars($from, $post), 1]]));
}
for ($i = 0; $i < 140; $i++) {
    // One code, two characters.
    [$unit, $chars] = pick([["\x88\x62", 2], ["\x88\x62A", 3], ["A\x88\x64", 3], ["\x88\xa3\xa4\x40", 3],
        ["\x88\xa5\x88\xa5B", 5], ["\x88\x66", 1], ["\x88\x62\x88\x66", 3]]);
    $n = match (wpick(['half' => 25, 'r1' => 25, 'r2' => 25, 'sweep' => 25])) {
        'half' => intdiv(4080, $chars) + mt_rand(-3, 3),
        'r1' => intdiv(8160, $chars) + mt_rand(-3, 3),
        'r2' => intdiv(16320, $chars) + mt_rand(-3, 3),
        default => mt_rand(1, 60),
    };
    $pre = str_repeat('A', mt_rand(0, 3));
    $post = pick(['', '', "\x88", "\xff", 'Z']);
    add('BIG5-HKSCS', pick(['UTF-8', 'UTF-16LE', 'UCS-4BE', 'BIG5', 'UTF-16', 'ISO-8859-1']),
        recipe([[$pre, 1], [$unit, max(1, $n)], [$post, 1]]));
}

section('combining decoders and encoders (TCVN-5712, CP1258, CP1255) around the rounds');
foreach (['TCVN-5712', 'CP1258', 'CP1255'] as $cs) {
    $marks = [];
    $bases = [];
    foreach ($T[$cs]['bycp'] as $cp => $seqs) {
        if (($cp >= 0x300 && $cp <= 0x36f) || ($cp >= 0x591 && $cp <= 0x5c7)) {
            $marks[] = $seqs[0];
        } elseif (($cp >= 0x41 && $cp <= 0x5a) || ($cp >= 0x61 && $cp <= 0x7a) || ($cp >= 0xc0 && $cp <= 0x1b0)
            || ($cp >= 0x5d0 && $cp <= 0x5f2)) {
            $bases[] = $seqs[0];
        }
    }
    if (!$marks || !$bases) {
        fail("$cs: no marks or letters");
    }
    for ($i = 0; $i < 110; $i++) {
        $shape = pick(['bm', 'b', 'bmm', 'm', 'bbm', 'bm', 'bmb']);
        $unit = '';
        foreach (str_split($shape) as $c) {
            $unit .= $c === 'b' ? pick($bases) : pick($marks);
        }
        $n = repeats(strlen($shape));
        $to = pick(['UTF-8', 'UTF-8', 'UTF-16LE', 'UCS-4BE', 'VISCII', 'TCVN-5712', 'CP1258']);
        add(nm($cs), nm($to === $cs ? 'UTF-8' : $to), recipe([[pick($bases), mt_rand(0, 3)], [$unit, $n],
            [pick(['', pick($marks), pick($bases)]), 1]]));
    }
}
$viet = [];
for ($c = 0x1ea0; $c <= 0x1ef9; $c++) {
    $viet[] = [$c];
}
foreach ([0xc0, 0xc1, 0xc2, 0xc3, 0xc8, 0xc9, 0xca, 0xcc, 0xcd, 0xd2, 0xd3, 0xd4, 0xd5, 0xd9, 0xda, 0xdd, 0xe0, 0xe1,
    0xe2, 0xe3, 0x102, 0x103, 0x110, 0x111, 0x128, 0x129, 0x168, 0x169, 0x1a0, 0x1a1, 0x1af, 0x1b0] as $c) {
    $viet[] = [$c];
}
foreach ([[0x61, 0x301], [0x65, 0x323], [0xe2, 0x303], [0x1b0, 0x309], [0x41, 0x300, 0x301], [0x301], [0x61, 0x302]]
    as $seq) {
    $viet[] = $seq;
}
for ($i = 0; $i < 120; $i++) {
    $from = pick(['UTF-8', 'UTF-8', 'UTF-16LE']);
    $unit = [];
    for ($j = mt_rand(1, 3); $j > 0; $j--) {
        array_push($unit, ...pick($viet));
    }
    add($from, pick(['TCVN-5712', 'TCVN-5712', 'CP1258', 'VISCII']), recipe([[chars($from, [0x61]), mt_rand(0, 3)],
        [chars($from, $unit), repeats(count($unit))]]));
}

section('periodic errors and unconvertible characters in long inputs');
for ($i = 0; $i < 420; $i++) {
    if (pct(55)) {
        $from = pct(80) ? pick(MB) : pick(NEW8);
        $t = $T[$from];
        $m = mt_rand(0, 150);
        $filler = $t['multi'] && pct(75) ? pick(pick($t['multi'])) : pick($t['single']);
        $special = pct(55) ? pick($t['bad']) : ($t['partial'] && pct(60) ? pick($t['partial']) : chr(mt_rand(0x80, 0xff)));
        $unit = str_repeat($filler, $m) . $special;
        add(nm($from), nm(pick($LONG_TARGETS)), recipe([[$unit, max(1, intdiv(mt_rand(8100, 25000), $m + 1))]]));
    } else {
        $to = pct(75) ? pick([...MB, ...ST]) : pick(NEW8);
        $from = pick(['UTF-8', 'UTF-8', 'UTF-16LE', 'UCS-4BE']);
        $m = mt_rand(0, 150);
        $special = match (wpick(['translit' => 35, 'other' => 35, 'bad' => 30])) {
            'translit' => chars($from, [pick($TRANSLIT)]),
            'other' => chars($from, [pick($OTHER)]),
            default => $from === 'UTF-8' ? pick(BAD_UTF8) : ($from === 'UTF-16LE' ? bad16(true) : bad32(false)),
        };
        $unit = str_repeat(chars($from, [pick($ENC[$to] ?: [0x41])]), $m) . $special;
        add(nm($from), nm($to), recipe([[$unit, max(1, intdiv(mt_rand(8100, 25000), $m + 1))]]));
    }
}

section('empty inputs');
foreach ([...MB, ...ST, ...NEW8] as $cs) {
    add('UTF-8', $cs, '');
    add($cs, 'UTF-8', '');
}

section('suffix variants');
$SUFFIXES = ['//translit', '//ignore', '//IGNORE//TRANSLIT', '//TRANSLIT//IGNORE', '//Translit', '//', '/',
    '//IGNORE//', '//NONE', '//TRANSLIT,IGNORE', '//IGNORE,TRANSLIT', '//TRANSLIT//TRANSLIT', '//IGNORE//IGNORE',
    '// IGNORE', '//TRANSLIT/'];
foreach ([...MB, ...ST, 'KOI8-T', 'IBM037', 'TCVN-5712', 'BRF'] as $cs) {
    for ($i = 0; $i < 10; $i++) {
        $cps = [pick($ENC[$cs] ?: [0x41]), pick($TRANSLIT), pick($ENC[$cs] ?: [0x41]), pick($OTHER)];
        $in = implode('', array_map('u8', $cps));
        if (pct(40)) {
            $in .= pick(BAD_UTF8) . 'z';
        }
        add('UTF-8', $cs . pick($SUFFIXES), $in);
        if (pct(30)) {
            add($cs . pick(['//IGNORE', '//TRANSLIT', '//', '//ignore']), 'UTF-8', bytes_in($cs, true, null));
        }
    }
}

section('targets without "?"');
foreach (NEW8 as $cs) {
    if (encodes($cs, 0x3f)) {
        continue;
    }
    for ($i = 0; $i < 30; $i++) {
        $cps = [];
        for ($j = mt_rand(1, 6); $j > 0; $j--) {
            $cps[] = match (wpick(['rep' => 40, 'translit' => 30, 'other' => 30])) {
                'rep' => pick($ENC[$cs] ?: [0x20]),
                'translit' => pick($TRANSLIT),
                default => pick($OTHER),
            };
        }
        $from = pick(['UTF-8', 'UTF-16LE']);
        add($from, $cs, chars($from, $cps));
    }
}

section('single-byte charsets of one script converted into each other: every byte, in runs of 16 or 32');
// glibc may convert two charsets with a module of their own instead of going through Unicode (it does so for GB 2312
// and GBK or Big5); codepages of one script, EBCDIC and PC alike, are where such modules would be.
$SCRIPTS = [
    [16, ['IBM420', 'CP1008', 'CP1046', 'IBM864', 'ARABIC7', 'ISO-8859-6', 'CP1256']],
    [32, ['IBM424', 'IBM856', 'CP862', 'CP1255', 'ISO-8859-8']],
    [32, ['IBM423', 'IBM875', 'CP737', 'CP869', 'HP-GREEK8', 'ISO-8859-7', 'CP1253']],
    [32, ['IBM880', 'IBM1025', 'CP855', 'CP866', 'KOI8-R', 'CP1251', 'ISO-8859-5']],
    [32, ['TIS-620', 'CP874', 'IBM1160', 'IBM1161', 'IBM9066']],
    [32, ['TCVN-5712', 'VISCII', 'CP1258', 'CP1129', 'CP1163', 'IBM1164']],
];
foreach ($SCRIPTS as [$width, $family]) {
    foreach ($family as $a) {
        foreach ($family as $b) {
            if ($a === $b) {
                continue;
            }
            for ($run = 0; $run < 256; $run += $width) {
                $s = '';
                for ($byte = $run; $byte < $run + $width; $byte++) {
                    $s .= chr($byte);
                }
                add($a, $b, $s);
            }
        }
    }
}

section('//TRANSLIT into the single-byte charsets: replacements outside ASCII');
// Per charset, the code points (of the letters, punctuation and symbol blocks below) it does not encode and that
// //TRANSLIT replaces with characters of the charset beyond ASCII: all of them in one input, and each alone when
// there are few.
$XU = [];
foreach ([[0xa0, 0x24f], [0x370, 0x3ff], [0x400, 0x52f], [0x1e00, 0x1fff], [0x2000, 0x206f], [0x2100, 0x218f],
    [0x2460, 0x24ff], [0xfb00, 0xfb4f], [0xff01, 0xff5e]] as [$a, $b]) {
    for ($c = $a; $c <= $b; $c++) {
        $XU[] = $c;
    }
}
foreach (NEW8 as $cs) {
    $keep = [];
    foreach ($XU as $cp) {
        if (encodes($cs, $cp)) {
            continue;
        }
        [$r] = call('UTF-32BE', $cs . '//TRANSLIT', pack('N', $cp));
        if ($r === false || $r === '') {
            continue;
        }
        [$back] = call($cs, 'UTF-32BE', $r);
        if ($back === false || $back === '') {
            continue;
        }
        foreach (unpack('N*', $back) as $c) {
            if ($c >= 0x80) {
                $keep[] = $cp;
                break;
            }
        }
    }
    if ($keep) {
        add('UTF-8', $cs, chars('UTF-8', $keep));
        if (count($keep) <= 40) {
            foreach ($keep as $cp) {
                add('UTF-8', $cs, u8($cp));
            }
        }
    }
}

section('Unicode language tags');
$HAN = [0x4e00, 0x4e2d, 0x4eba, 0x5927, 0x5b66, 0x5b78, 0x56fd, 0x570b, 0x8a9e, 0x8bed, 0x6f22, 0x6c49, 0x9ad8,
    0x9ad9, 0x5c71, 0x4e0e, 0x8207];
$NOTHAN = [0x3042, 0x30a2, 0xac00, 0xe9, 0x3b1, 0x416, 0x41, 0xa5, 0x203e, 0xff71, 0x2460, 0x20ac];
for ($i = 0; $i < 360; $i++) {
    $cps = pick(TAGSEQ);
    for ($j = mt_rand(1, 6); $j > 0; $j--) {
        $cps[] = pct(65) ? pick($HAN) : pick($NOTHAN);
        if (pct(10)) {
            array_push($cps, ...pick(TAGSEQ));
        }
    }
    $to = pct(70) ? 'ISO-2022-JP-2' : pick([...MB, 'ISO-2022-JP', 'ISO-2022-KR']);
    $from = pick(['UTF-8', 'UTF-8', 'UTF-16BE', 'UCS-4LE']);
    add($from, $to, chars($from, $cps));
}

section('Unicode language tags: every case of the letters');
foreach (['ja', 'ko', 'zh'] as $lang) {
    foreach ([[0, 0], [1, 0], [0, 1], [1, 1]] as [$up0, $up1]) {
        $tag = [0xe0001, ord($lang[0]) - ($up0 ? 0x20 : 0) + 0xe0000, ord($lang[1]) - ($up1 ? 0x20 : 0) + 0xe0000];
        foreach ([[0x4e2d], [0x6c49, 0xe9], [0xe9], [0x4e2d, 0x4e2d], [0xac00, 0x4e2d], [0x3b1, 0x4e00],
            [0x4e2d, 0xe0001, 0xe007f, 0x4e2d], [0x6f22, 0xe9, 0x56fd]] as $tail) {
            add('UTF-8', 'ISO-2022-JP-2', chars('UTF-8', [...$tag, ...$tail]));
        }
        add('UTF-8', 'ISO-2022-JP-2', recipe([[chars('UTF-8', $tag), 1], [chars('UTF-8', [0x4e2d, 0xe9]), 4100]]));
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Running the cases

note(sprintf('converting %d inputs four times each', count($CASES)));
$names = [];
$messages = [];
$rows = [];

function name_index(string $name): int
{
    global $names;
    if (!isset($names[$name])) {
        $names[$name] = count($names);
    }
    return $names[$name];
}

/** The result of iconv($from, $to, $in) in the file's encoding. */
function outcome(string $from, string $to, string $in): string|int
{
    global $messages;
    [$r, $d] = call($from, $to, $in);
    if ($r === false) {
        if (count($d) !== 1) {
            fail(sprintf('iconv(%s, %s, %s): false with %d diagnostics', $from, $to, bin2hex($in), count($d)));
        }
        $key = $d[0][0] . ' ' . $d[0][1];
        if (!isset($messages[$key])) {
            $messages[$key] = [count($messages), $d[0][0], $d[0][1]];
        }
        return $messages[$key][0] + 1;
    }
    if ($d) {
        fail(sprintf('iconv(%s, %s, %s): a string with diagnostics', $from, $to, bin2hex($in)));
    }
    return strlen($r) <= 12 ? bin2hex($r) : strlen($r) . ':' . hash('fnv1a32', $r);
}

$total = 0;
foreach ($CASES as [$from, $to, $input]) {
    $bytes = expand($input);
    $results = [];
    foreach (FLAGS as $k => $flag) {
        $r = outcome($from, $to . $flag, $bytes);
        $results[] = ($k > 0 && $r === $results[0]) ? 0 : $r;
        $total++;
    }
    if ($results[1] === 0 && $results[2] === 0 && $results[3] === 0) {
        $results = [$results[0]];
    }
    $in = is_string($input) ? base64_encode($input)
        : array_map(static fn ($u) => [base64_encode($u[0]), $u[1]], $input);
    $rows[] = [name_index($from), name_index($to), $in, ...$results];
}
note("$total cases");

$msgList = [];
foreach ($messages as [$i, $level, $text]) {
    $msgList[$i] = [$level, $text];
}
ksort($msgList);

echo '{"generator":"crosscheck.gen.php","seed":', SEED, ',"cases_total":', $total, ",\n";
$list = array_map('strval', array_keys($names));
echo '"charsets":', json_encode($list, JSON_THROW_ON_ERROR | JSON_UNESCAPED_SLASHES), ",\n";
$families = array_map(static fn (string $n): string => $FAMILY[strtoupper($n)] ?? $n, $list);
echo '"families":', json_encode($families, JSON_THROW_ON_ERROR | JSON_UNESCAPED_SLASHES), ",\n";
echo '"flags":', json_encode(FLAGS, JSON_THROW_ON_ERROR | JSON_UNESCAPED_SLASHES), ",\n";
echo '"messages":', json_encode(array_values($msgList), JSON_THROW_ON_ERROR | JSON_UNESCAPED_SLASHES), ",\n";
echo '"sections":', json_encode($SECTIONS, JSON_THROW_ON_ERROR | JSON_UNESCAPED_SLASHES), ",\n";
echo "\"cases\":[\n";
$last = count($rows) - 1;
foreach ($rows as $i => $row) {
    echo json_encode($row, JSON_THROW_ON_ERROR | JSON_UNESCAPED_SLASHES), $i === $last ? "\n" : ",\n";
}
echo "]}\n";
note('done');
