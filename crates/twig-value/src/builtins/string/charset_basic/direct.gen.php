<?php
// Generates direct_table.rs: the pairs of single-byte charsets that glibc converts with a module of their own instead
// of through its internal UCS-4, and the byte each byte becomes there; and checks that no other pair of the charsets
// charset_basic converts with tables has one, but GB 2312 and GBK or BIG5 (which charset_basic.rs refuses).
//
// Black-box only, through PHP's iconv() in the oracle:
// - every byte of each single-byte charset (ANSI_X3.4-1968, ISO-8859-1 and the codepages of codepages.inc.php) is
//   converted into every other single-byte charset and into every multi-byte and ISO-2022 charset (multibytes.inc.php)
//   with one call, without a suffix and with //TRANSLIT, and compared with what the two charsets' own conversions give
//   it through UCS-4BE (one call each);
// - the characters of each multi-byte and ISO-2022 charset (what every BMP code point and every 97th beyond encodes
//   to), in batches of at most 4000 (one round of glibc's), are converted into every other charset with //IGNORE and
//   compared with their conversion through UCS-4BE.
// A pair where a result differs has a module of its own. Each such pair of single-byte charsets is checked to convert
// every byte into one byte under every suffix (none, //TRANSLIT, //IGNORE, //TRANSLIT//IGNORE), also in an input of
// every byte repeated beyond a round of 8160 characters, without a diagnostic; the table holds those bytes.
//
// Regenerate (from crates/twig-value/src/builtins/string/charset_basic, Git Bash):
//   MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//       php /t/direct.gen.php > direct_table.rs

require __DIR__ . '/codepages.inc.php';
require __DIR__ . '/multibytes.inc.php';
ini_set('memory_limit', '-1');

if (setlocale(LC_CTYPE, '0') !== 'C.UTF-8') {
    fwrite(STDERR, "expected LC_CTYPE=C.UTF-8\n");
    exit(1);
}

function fail(string $message): never
{
    fwrite(STDERR, $message . "\n");
    exit(1);
}

// the diagnostics of the last call
$GLOBALS['diags'] = [];
set_error_handler(static function (int $no, string $message): bool {
    $GLOBALS['diags'][] = "$no:$message";
    return true;
});

/** iconv() as a comparable result: "=" and the output, or "!" and the diagnostics of a false. */
function result(string $from, string $to, string $input): string
{
    $GLOBALS['diags'] = [];
    $r = iconv($from, $to, $input);
    if ($r === false) {
        return '!' . implode('|', $GLOBALS['diags']);
    }
    if ($GLOBALS['diags'] !== []) {
        fail("$from -> $to: " . bin2hex($input) . ' converts with ' . implode('|', $GLOBALS['diags']));
    }
    return "=$r";
}

/** The Rust value of a single-byte charset. */
function charset_value(string $name): string
{
    if ($name === 'ANSI_X3.4-1968') {
        return 'Charset::Ascii';
    }
    if ($name === 'ISO-8859-1') {
        return 'Charset::Latin1';
    }
    foreach (CODEPAGES as $variant => [$cs]) {
        if ($cs === $name) {
            return "Charset::Codepage(Codepage::$variant)";
        }
    }
    fail("no variant for $name");
}

$singles = ['ANSI_X3.4-1968', 'ISO-8859-1'];
foreach (CODEPAGES as [$cs]) {
    $singles[] = $cs;
}
$multis = array_merge(array_values(MULTIBYTES), array_values(ISO2022S));
$suffixes = ['', '//TRANSLIT'];

// each single-byte charset's decoding of each byte (UCS-4BE), and the encodings of what they decode to
$decoded = [];
$characters = [];
foreach ($singles as $cs) {
    for ($b = 0; $b < 256; $b++) {
        $decoded[$cs][$b] = result($cs, 'UCS-4BE', chr($b));
        if ($decoded[$cs][$b][0] === '=') {
            $characters[$decoded[$cs][$b]] = true;
        }
    }
}
$encoded = [];
foreach (array_merge($singles, $multis) as $cs) {
    foreach ($suffixes as $suffix) {
        foreach (array_keys($characters) as $u) {
            $encoded[$cs . $suffix][$u] = result('UCS-4BE', $cs . $suffix, substr((string) $u, 1));
        }
    }
}

// every byte of the single-byte charsets: the direct conversion against the composition
$direct = [];
foreach ($singles as $from) {
    foreach (array_merge($singles, $multis) as $to) {
        if ($from === $to) {
            continue;
        }
        foreach ($suffixes as $suffix) {
            for ($b = 0; $b < 256; $b++) {
                $u = $decoded[$from][$b];
                $composed = $u[0] === '!' ? $u : $encoded[$to . $suffix][$u];
                if (result($from, $to . $suffix, chr($b)) !== $composed) {
                    $direct[] = [$from, $to];
                    continue 3;
                }
            }
        }
    }
}

// the characters of the multi-byte and ISO-2022 charsets, in batches
$expected = ['EUC-CN -> GBK', 'EUC-CN -> BIG5', 'GBK -> EUC-CN', 'BIG5 -> EUC-CN'];
$refused = [];
foreach ($multis as $from) {
    $pieces = [];
    $cps = range(0, 0xFFFF);
    for ($cp = 0x10000; $cp < 0x110000; $cp += 97) {
        $cps[] = $cp;
    }
    foreach ($cps as $cp) {
        if ($cp < 0xD800 || $cp > 0xDFFF) {
            $r = result('UCS-4BE', $from, pack('N', $cp));
            if (strlen($r) > 1 && $r[0] === '=') {
                $pieces[] = substr($r, 1);
            }
        }
    }
    $batches = [];
    foreach (array_chunk($pieces, 4000) as $chunk) {
        $batch = implode('', $chunk);
        $ucs = result($from, 'UCS-4BE', $batch);
        if ($ucs[0] !== '=' || strlen($ucs) > 1 + 4 * 8160) {
            fail("$from: a batch of its characters does not decode in one round");
        }
        $batches[] = [$batch, substr($ucs, 1)];
    }
    foreach (array_merge($singles, $multis) as $to) {
        if ($from === $to) {
            continue;
        }
        foreach ($batches as [$batch, $ucs]) {
            if (result($from, "$to//IGNORE", $batch) !== result('UCS-4BE', "$to//IGNORE", $ucs)) {
                if (in_array($to, $singles, true)) {
                    $direct[] = [$from, $to];
                } else {
                    $refused[] = "$from -> $to";
                }
                continue 2;
            }
        }
    }
}
sort($refused);
sort($expected);
if ($refused !== $expected) {
    fail('the multi-byte charsets with modules of their own are ' . implode(', ', $refused) . ', not '
        . implode(', ', $expected) . ' (charset_basic.rs refuses those)');
}

// each pair with a module of its own: every byte becomes one byte, with every suffix, alone and in a long input
$all = '';
for ($b = 0; $b < 256; $b++) {
    $all .= chr($b);
}
$long = str_repeat($all, 40);
$tables = [];
foreach ($direct as [$from, $to]) {
    if (!in_array($from, $singles, true) || !in_array($to, $singles, true)) {
        fail("$from -> $to: a module of its own beyond the single-byte charsets (model it)");
    }
    $bytes = [];
    foreach (['', '//TRANSLIT', '//IGNORE', '//TRANSLIT//IGNORE'] as $suffix) {
        for ($b = 0; $b < 256; $b++) {
            $r = result($from, $to . $suffix, chr($b));
            if (strlen($r) !== 2 || $r[0] !== '=' || ($suffix !== '' && $r[1] !== $bytes[$b])) {
                fail(sprintf('%s -> %s%s: byte 0x%02X gives %s (a module of its own that does not map every byte to one '
                    . 'byte: model it)', $from, $to, $suffix, $b, $r[0] === '!' ? $r : bin2hex(substr($r, 1))));
            }
            $bytes[$b] = $r[1];
        }
        if (result($from, $to . $suffix, $long) !== '=' . strtr($long, $all, implode('', $bytes))) {
            fail("$from -> $to$suffix: a long input does not convert byte by byte");
        }
    }
    $tables[] = [$from, $to, array_map('ord', $bytes)];
}
fwrite(STDERR, sprintf("modules of their own: %s among the %d single-byte charsets; %s among the multi-byte ones\n",
    implode(', ', array_map(static fn (array $t): string => "$t[0] -> $t[1]", $tables)), count($singles),
    implode(', ', $refused)));

echo "//! Generated by direct.gen.php (black-box: PHP iconv() in the oracle); do not edit.\n";
echo "//!\n";
echo "//! The pairs of single-byte charsets glibc 2.41 converts with a module of their own instead of through its internal\n";
echo "//! UCS-4, found by converting every byte of each single-byte charset into every other charset (without a suffix and\n";
echo "//! with //TRANSLIT) and comparing with the two charsets' own conversions: what each byte becomes, the same under\n";
echo "//! every suffix (every byte converts, without a diagnostic).\n\n";
echo "use super::codec::Charset;\n";
echo "use super::codepage::Codepage;\n\n";
echo "/// `(from, to, bytes)`: byte `b` of `from` becomes `bytes[b]` in `to`.\n";
printf("pub(super) static DIRECT: [(Charset, Charset, [u8; 256]); %d] = [\n", count($tables));
foreach ($tables as [$from, $to, $bytes]) {
    printf("    // %s -> %s\n", $from, $to);
    printf("    (%s, %s, [\n", charset_value($from), charset_value($to));
    foreach (array_chunk($bytes, 16) as $chunk) {
        echo '        ', implode(', ', array_map(static fn (int $b): string => sprintf('0x%02X', $b), $chunk)), ",\n";
    }
    echo "    ]),\n";
}
echo "];\n";
