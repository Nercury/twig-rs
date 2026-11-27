<?php
// Generates name_table.rs: every charset name the oracle's iconv knows, mapped to the charset_basic converter that
// behaves exactly like it, or marked as not supported by twig-rs.
//
// Black-box only: the names come from `iconv -l` and each name is classified by a behavioural fingerprint (PHP
// iconv() calls in both directions, flags, invalid input, long inputs, every BMP code point encoded with //TRANSLIT,
// the compositions of CP1255 and CP1258) compared with the fingerprints of the canonical names of the charsets
// charset_basic implements. A name that matches a single-byte charset of codepages.inc.php must also match it on a
// deeper print: the astral planes, every two-byte input and every byte after each composing pair. A name whose
// fingerprint matches no canonical name (for example WCHAR_T, which differs from UCS-4LE in what it lets through, or
// TIS-620, which lacks the no-break space of ISO-8859-11) is "not supported". A name that matches a multi-byte charset
// of multibytes.inc.php must match its deeper print too: every one- and two-byte input, the three- and four-byte
// inputs of EUC-JP, EUC-TW and GB18030, and the astral planes.
//
// Regenerate (from crates/twig-value/src/builtins/string/charset_basic, Git Bash):
//   MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//       php /t/names.gen.php > name_table.rs

declare(strict_types=1);

require __DIR__ . '/codepages.inc.php';
require __DIR__ . '/multibytes.inc.php';
ini_set('memory_limit', '-1');

const BASIC = [
    'Utf8' => 'UTF-8', 'Utf16' => 'UTF-16', 'Utf16Be' => 'UTF-16BE', 'Utf16Le' => 'UTF-16LE',
    'Utf32' => 'UTF-32', 'Utf32Be' => 'UTF-32BE', 'Utf32Le' => 'UTF-32LE',
    'Ucs2' => 'UCS-2', 'Ucs2Be' => 'UCS-2BE', 'Ucs4' => 'UCS-4', 'Ucs4Le' => 'UCS-4LE',
    'Ascii' => 'ANSI_X3.4-1968', 'Latin1' => 'ISO-8859-1',
];

set_error_handler(static function (int $no, string $msg): bool {
    $GLOBALS['diag'][] = $msg;
    return true;
});

function probe(string $from, string $to, string $s): string
{
    $GLOBALS['diag'] = [];
    $r = iconv($from, $to, $s);
    if ($r === false) {
        $m = $GLOBALS['diag'][0] ?? '?';
        return str_contains($m, 'Wrong') ? 'W' : (str_contains($m, 'illegal') ? 'S' : (str_contains($m, 'incomplete') ? 'I' : 'F'));
    }
    return strlen($r) > 64 ? strlen($r) . ':' . md5($r) : bin2hex($r);
}

function u4(int ...$cps): string
{
    return implode('', array_map(static fn (int $c): string => pack('N', $c), $cps));
}

/** The byte sequences that CP1255 and CP1258 decode to one character (their compositions). */
function composing_inputs(): array
{
    static $inputs = null;
    if ($inputs === null) {
        $inputs = [];
        foreach (['CP1255', 'CP1258'] as $cs) {
            for ($a = 0; $a < 256; $a++) {
                for ($b = 0; $b < 256; $b++) {
                    $r = iconv($cs, 'UCS-4BE', chr($a) . chr($b));
                    if ($r !== false && strlen($r) === 4) {
                        $inputs[] = chr($a) . chr($b);
                    }
                }
            }
        }
    }
    return $inputs;
}

function fingerprint(string $name): string
{
    $r = [];
    $forward = [];
    for ($cp = 0; $cp < 0x100; $cp++) {
        $forward[] = mb_chr($cp, 'UTF-8');
    }
    foreach ([0x100, 0x20AC, 0xD7FF, 0xE000, 0xFEFF, 0xFFFE, 0xFFFF, 0x10000, 0x1F600, 0x10FFFF] as $cp) {
        $forward[] = mb_chr($cp, 'UTF-8');
    }
    array_push($forward, "\xf4\x90\x80\x80", "\xfd\xbf\xbf\xbf\xbf\xbf", 'ab', '');
    foreach ($forward as $s) {
        $r[] = probe('UTF-8', $name, $s);
    }
    foreach (["a\u{e9}b", "a\u{20ac}b", "a\u{1F600}b", "a\xf4\x90\x80\x80b", "a\xffb"] as $s) {
        foreach (['//TRANSLIT', '//IGNORE', '//TRANSLIT//IGNORE'] as $suffix) {
            $r[] = probe('UTF-8', $name . $suffix, $s);
        }
    }
    $backward = [];
    for ($b = 0; $b < 0x100; $b++) {
        $backward[] = chr($b);
    }
    array_push($backward, "\x00a", "a\x00", "\x00\x00\x00a", "a\x00\x00\x00", "\xfe\xff\x00a", "\xff\xfea\x00",
        "\x00\x00\xfe\xff\x00\x00\x00a", "\xff\xfe\x00\x00a\x00\x00\x00", "\xd8\x3d\xde\x00", "\x3d\xd8\x00\xde",
        "\x00\x00\xd8\x00", "\x00\xd8\x00\x00", "\x00\x11\x00\x00", "\x00\x00\x11\x00", "\x7f\xff\xff\xff",
        "\xff\xff\xff\x7f", "\x80\x00\x00\x00", "\x00\x00\x00\x80", "\xc3\xa9", "\xf4\x90\x80\x80", "\xed\xa0\x80",
        "\xd8\x00", "\x00\xd8", "\xdc\x00\x00a", "a\x00\x00\xdc", "\xfe\xff", "\xff\xfe", "a\x80b\x00");
    foreach ($backward as $s) {
        $r[] = probe($name, 'UTF-8', $s);
        $r[] = probe($name, 'UCS-4LE//IGNORE', $s);
        $r[] = probe($name, 'UTF-16BE//ignore', $s);
    }
    foreach (composing_inputs() as $s) {
        $r[] = probe($name, 'UCS-4BE', $s);
    }
    foreach ([0xD800, 0x110000, 0x7FFFFFFF, 0x10000, 0x20AC, 0xE9] as $cp) {
        foreach (['', '//TRANSLIT', '//IGNORE', '//TRANSLIT//IGNORE', '//ignore'] as $suffix) {
            $r[] = probe('UCS-4', $name . $suffix, u4(0x61, $cp, 0x62));
        }
    }
    // every BMP code point, each followed by a line feed
    for ($start = 0; $start < 0x10000; $start += 4000) {
        $in = '';
        for ($cp = $start; $cp < min($start + 4000, 0x10000); $cp++) {
            if ($cp < 0xD800 || $cp > 0xDFFF) {
                $in .= pack('N', $cp) . "\x00\x00\x00\x0a";
            }
        }
        $r[] = probe('UCS-4BE', $name . '//TRANSLIT', $in);
    }
    // long inputs: glibc converts in chunks, and PHP's //IGNORE handling depends on where a call stops
    $long = str_repeat('a', 9000);
    foreach (["\u{e9}", "\u{20ac}", "\u{1F600}", "\xf4\x90\x80\x80", "\xff"] as $x) {
        $r[] = probe('UTF-8', $name . '//IGNORE', $x . $long);
        $r[] = probe('UTF-8', $name . '//ignore', $x . $long);
        $r[] = probe('UTF-8', $name . '//TRANSLIT', $x . $long);
    }
    $encoded = probe('UTF-8', $name, $long) === 'W' ? '' : (string) iconv('UTF-8', $name, $long);
    foreach (["\x80", "\x80\x00", "\x00\xd8", "\x00\x00\x00\x80", "\x00\x00\x11\x00"] as $x) {
        $r[] = probe($name, 'ISO-8859-1//IGNORE', $x . $encoded);
        $r[] = probe($name, 'UTF-16LE//ignore', $x . $encoded);
        $r[] = probe($name, 'ISO-8859-1//IGNORE', $encoded . $x . $encoded);
    }
    return md5(implode(',', $r));
}

/** The deeper print of a single-byte charset: the astral planes, every two-byte input, every byte after a pair that
 * gave one character. */
function deep_fingerprint(string $name): string
{
    $r = [];
    for ($start = 0x10000; $start < 0x110000; $start += 4096) {
        $in = '';
        for ($cp = $start; $cp < $start + 4096; $cp++) {
            $in .= pack('N', $cp);
        }
        $r[] = probe('UCS-4BE', $name . '//IGNORE', $in);
    }
    for ($a = 0; $a < 256; $a++) {
        $row = '';
        for ($b = 0; $b < 256; $b++) {
            $s = chr($a) . chr($b);
            $out = probe($name, 'UCS-4BE', $s);
            $row .= $out . ',';
            if (strlen($out) === 8) {
                for ($c = 0; $c < 256; $c++) {
                    $row .= probe($name, 'UCS-4BE', $s . chr($c)) . ';';
                }
            }
        }
        $r[] = md5($row);
    }
    return md5(implode(',', $r));
}

/** The deeper print of a multi-byte charset. */
function deep_multibyte_fingerprint(string $name): string
{
    $r = [];
    for ($start = 0x10000; $start < 0x110000; $start += 4096) {
        $in = '';
        for ($cp = $start; $cp < $start + 4096; $cp++) {
            $in .= pack('N', $cp);
        }
        $r[] = probe('UCS-4BE', $name . '//IGNORE', $in);
    }
    $row = static function (string $prefix, int $lo, int $hi) use ($name): string {
        $out = '';
        for ($b = $lo; $b <= $hi; $b++) {
            $out .= probe($name, 'UCS-4BE', $prefix . chr($b)) . ',';
            $out .= probe($name, 'UCS-4BE//IGNORE', $prefix . chr($b) . '!!') . ';';
        }
        return md5($out);
    };
    for ($a = 0; $a < 256; $a++) {
        $r[] = $row(chr($a), 0, 255);
    }
    foreach ([0x8E, 0x8F] as $lead) {
        for ($b = 0xA1; $b <= 0xFE; $b++) {
            $r[] = $row(chr($lead) . chr($b), 0xA1, 0xFE);
        }
    }
    for ($p = 0xA1; $p <= 0xB0; $p++) {
        for ($b = 0xA1; $b <= 0xFE; $b += 7) {
            $r[] = $row("\x8e" . chr($p) . chr($b), 0xA1, 0xFE);
        }
    }
    foreach ([0x81, 0x82, 0x84, 0x85, 0x90, 0xA0, 0xE3, 0xE4, 0xFE] as $l) {
        for ($d = 0x30; $d <= 0x39; $d++) {
            for ($b = 0x81; $b <= 0xFE; $b += 5) {
                $r[] = $row(chr($l) . chr($d) . chr($b), 0x2F, 0x3A);
            }
        }
    }
    return md5(implode(',', $r));
}

/** The deeper print of an ISO-2022 charset: every two-byte input after each designation, and every BMP code point
 * after a character of each set. */
function deep_iso2022_fingerprint(string $name): string
{
    $r = [];
    foreach (['', "\x1b(B", "\x1b(J", "\x1b\$@", "\x1b\$B", "\x1b\$A", "\x1b\$(C", "\x1b\$(D", "\x1b(I", "\x1b.A\x1bN",
        "\x1b.F\x1bN", "\x0e", "\x1b\$)C\x0e"] as $prefix) {
        for ($a = 0; $a < 256; $a++) {
            $row = '';
            for ($b = 0; $b < 256; $b++) {
                $row .= probe($name, 'UCS-4BE', $prefix . chr($a) . chr($b)) . ',';
            }
            $r[] = md5($row);
        }
    }
    foreach ([0x61, 0xA5, 0x3042, 0x4E02, 0x554A, 0xAC00, 0xFF61, 0xA0, 0x386] as $x) {
        $row = '';
        for ($cp = 0; $cp < 0x10000; $cp++) {
            if ($cp < 0xD800 || $cp > 0xDFFF) {
                $row .= probe('UCS-4BE', $name, pack('N', $x) . pack('N', $cp)) . ',';
            }
        }
        $r[] = md5($row);
    }
    return md5(implode(',', $r));
}

$canonical = [];
foreach (BASIC as $variant => $name) {
    $canonical[$variant] = $name;
}
foreach (CODEPAGES as $variant => [$name]) {
    $canonical["Codepage(Codepage::$variant)"] = $name;
}
foreach (MULTIBYTES as $variant => $name) {
    $canonical["Multibyte(Multibyte::$variant)"] = $name;
}
foreach (ISO2022S as $variant => $name) {
    $canonical["Iso2022(Iso2022::$variant)"] = $name;
}
$canonicalPrints = [];
$deepPrints = [];
foreach ($canonical as $variant => $name) {
    $canonicalPrints[fingerprint($name)] = $variant;
    if (str_starts_with($variant, 'Codepage')) {
        $deepPrints[$variant] = deep_fingerprint($name);
    } elseif (str_starts_with($variant, 'Multibyte')) {
        $deepPrints[$variant] = deep_multibyte_fingerprint($name);
    } elseif (str_starts_with($variant, 'Iso2022')) {
        $deepPrints[$variant] = deep_iso2022_fingerprint($name);
    }
}
if (count($canonicalPrints) !== count($canonical)) {
    fwrite(STDERR, "two canonical charsets have the same fingerprint\n");
    exit(1);
}

$entries = [];
foreach (preg_split('/\s+/', trim((string) shell_exec('iconv -l'))) as $listed) {
    // `iconv -l` prints NAME// or, for names with a slash inside, A/B/
    $key = str_ends_with($listed, '//') ? substr($listed, 0, -2) : rtrim($listed, '/');
    $variant = $canonicalPrints[fingerprint($key)] ?? null;
    $deep = static fn (string $v, string $key): string => match (true) {
        str_starts_with($v, 'Multibyte') => deep_multibyte_fingerprint($key),
        str_starts_with($v, 'Iso2022') => deep_iso2022_fingerprint($key),
        default => deep_fingerprint($key),
    };
    if ($variant !== null && isset($deepPrints[$variant]) && $deep($variant, $key) !== $deepPrints[$variant]) {
        fwrite(STDERR, "$key: only the deeper print differs from $canonical[$variant]\n");
        $variant = null;
    }
    $entries[$key] = $variant;
}
ksort($entries, SORT_STRING);

echo "//! Generated by names.gen.php (black-box: `iconv -l` and behavioural fingerprints in the oracle); do not edit.\n";
echo "//!\n";
echo "//! Every charset name glibc 2.41's iconv knows in the oracle image, as `iconv -l` lists it without the trailing\n";
echo "//! slashes, sorted bytewise, with the charset_basic converter it behaves exactly like (`None`: a charset twig-rs\n";
echo "//! does not convert).\n\n";
echo "use super::codec::Charset;\n";
echo "use super::codepage::Codepage;\n";
echo "use super::iso2022::Iso2022;\n";
echo "use super::multibyte::Multibyte;\n\n";
echo "/// The known charset names (upper case, `A` or `A/B`) and their converters.\n";
printf("pub(super) static NAMES: [(&str, Option<Charset>); %d] = [\n", count($entries));
foreach ($entries as $key => $variant) {
    printf("    (%s, %s),\n", json_encode((string) $key, JSON_UNESCAPED_SLASHES), $variant === null ? 'None' : "Some(Charset::$variant)");
}
echo "];\n";
