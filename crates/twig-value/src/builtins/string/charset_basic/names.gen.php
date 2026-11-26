<?php
// Generates name_table.rs: every charset name the oracle's iconv knows, mapped to the charset_basic converter that
// behaves exactly like it, or marked as not supported by twig-rs.
//
// Black-box only: the names come from `iconv -l` and each name is classified by a behavioural fingerprint (PHP
// iconv() calls in both directions, flags, invalid input, long inputs) compared with the fingerprints of the canonical
// names of the charsets charset_basic implements. A name whose fingerprint matches no canonical name (for example
// WCHAR_T, which differs from UCS-4LE in what it lets through) is "not supported".
//
// Regenerate (from crates/twig-value/src/builtins/string/charset_basic, Git Bash):
//   MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//       php /t/names.gen.php > name_table.rs

declare(strict_types=1);

const CANONICAL = [
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
    foreach ([0xD800, 0x110000, 0x7FFFFFFF, 0x10000, 0x20AC, 0xE9] as $cp) {
        foreach (['', '//TRANSLIT', '//IGNORE', '//TRANSLIT//IGNORE', '//ignore'] as $suffix) {
            $r[] = probe('UCS-4', $name . $suffix, u4(0x61, $cp, 0x62));
        }
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

$canonicalPrints = [];
foreach (CANONICAL as $variant => $name) {
    $canonicalPrints[fingerprint($name)] = $variant;
}
if (count($canonicalPrints) !== count(CANONICAL)) {
    fwrite(STDERR, "two canonical charsets have the same fingerprint\n");
    exit(1);
}

$entries = [];
foreach (preg_split('/\s+/', trim((string) shell_exec('iconv -l'))) as $listed) {
    // `iconv -l` prints NAME// or, for names with a slash inside, A/B/
    $key = str_ends_with($listed, '//') ? substr($listed, 0, -2) : rtrim($listed, '/');
    $variant = $canonicalPrints[fingerprint($key)] ?? null;
    $entries[$key] = $variant;
}
ksort($entries, SORT_STRING);

echo "//! Generated by names.gen.php (black-box: `iconv -l` and behavioural fingerprints in the oracle); do not edit.\n";
echo "//!\n";
echo "//! Every charset name glibc 2.41's iconv knows in the oracle image, as `iconv -l` lists it without the trailing\n";
echo "//! slashes, sorted bytewise, with the charset_basic converter it behaves exactly like (`None`: a charset twig-rs\n";
echo "//! does not convert).\n\n";
echo "use super::codec::Charset;\n\n";
echo "/// The known charset names (upper case, `A` or `A/B`) and their converters.\n";
printf("pub(super) static NAMES: [(&str, Option<Charset>); %d] = [\n", count($entries));
foreach ($entries as $key => $variant) {
    printf("    (%s, %s),\n", json_encode((string) $key, JSON_UNESCAPED_SLASHES), $variant === null ? 'None' : "Some(Charset::$variant)");
}
echo "];\n";
