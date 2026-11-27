<?php
// Generates translit_table.rs: what //TRANSLIT turns a character into when the target charset cannot represent it.
//
// Black-box only: every code point is converted with PHP's iconv() in the oracle (glibc 2.41, LC_CTYPE=C.UTF-8,
// the locale PHP runs with there), one character between \x01 separators per slot of a batch, for every target with
// a smaller repertoire than Unicode: ANSI_X3.4-1968, ISO-8859-1, UCS-2 (U+10000 and up), the codepages of
// codepages.inc.php, the multi-byte charsets of multibytes.inc.php (but GB18030, which represents every character)
// and the ISO-2022 charsets (whose results are compared as the characters they decode to: each one in a batch
// carries its designations). Characters that come out as "?" (glibc's replacement when it knows no transliteration) and the
// tag characters (which vanish) are left out. Pairs of characters were checked to transliterate like the two
// characters on their own (no multi-character rules in this locale).
//
// The targets disagree where a character has several alternatives (U+339F is "mm²" where the target has "²", else
// "mm^2"): the table lists, for each character, the replacements seen in the targets in an order under which taking
// the first one the target can represent gives every target's result; the generator checks that it does, byte for
// byte. Then what shows only at the level of single iconv() calls, measured with translit_probe.c (compiled here):
// alternatives that fail in every target, and the characters whose replacement into an ISO-2022 charset the table
// cannot give (see the end). The tests in tests/charset_basic.rs compare the table with oracle vectors.
//
// Regenerate (from crates/twig-value/src/builtins/string/charset_basic, Git Bash):
//   MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//       php /t/translit.gen.php > translit_table.rs

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

$targets = ['ANSI_X3.4-1968', 'ISO-8859-1', 'UCS-2LE'];
foreach (CODEPAGES as [$cs]) {
    $targets[] = $cs;
}
foreach (MULTIBYTES as $cs) {
    if ($cs !== 'GB18030') {
        $targets[] = $cs;
    }
}
// the ISO-2022 charsets are checked against the table afterwards (see the end)

/** The code points of an ISO-2022 result (the bytes of one character of a batch, with its designations). */
function iso_cps(string $target, string $bytes): array
{
    $r = iconv($target, 'UCS-4BE', $bytes);
    if ($r === false) {
        fail("$target: " . bin2hex($bytes) . ' does not decode');
    }
    return array_values(unpack('N*', $r) ?: []);
}

/** What each code point encodes to in $target, null when it cannot be represented. */
function repertoire(string $target): array
{
    $enc = [];
    for ($cp = 0; $cp < 0x10000; $cp++) {
        $enc[$cp] = $cp >= 0xD800 && $cp <= 0xDFFF ? null : encode_cp($target, $cp);
    }
    // above the BMP, blocks that hold nothing the target can represent are skipped (the tag characters vanish)
    for ($start = 0x10000; $start < 0x110000; $start += 4096) {
        $in = '';
        for ($cp = $start; $cp < $start + 4096; $cp++) {
            if ($cp < 0xE0000 || $cp > 0xE007F) {
                $in .= pack('N', $cp);
            }
        }
        if ($target === 'UCS-2LE' || iconv('UCS-4BE', "$target//IGNORE", $in) === '') {
            continue;
        }
        for ($cp = $start; $cp < $start + 4096; $cp++) {
            if ($cp < 0xE0000 || $cp > 0xE007F) {
                $enc[$cp] = encode_cp($target, $cp);
            }
        }
    }
    return $enc;
}

/** The output of every character that $target cannot represent and does not turn into "?". */
function measure(string $target, array $enc): array
{
    $iso = in_array($target, ISO2022S, true);
    $unit = $target === 'UCS-2LE' ? 2 : 1;
    // ISO-2022: a line feed, which also makes ISO-2022-JP-2 designate G2 again for each character; else a control
    // character no other character encodes to (some IBM codepages write U+263A and others as control bytes)
    $control = 1;
    while (!$iso && $unit === 1 && array_filter($enc, static fn (?string $b, int $cp): bool => $cp >= 0x80 && $b !== null
        && str_contains($b, chr($control)), ARRAY_FILTER_USE_BOTH) !== []) {
        $control++;
    }
    $sep = $unit === 2 ? "\x01\x00" : ($iso ? "\x0a" : chr($control));
    $out = [];
    for ($start = 0x80; $start <= 0x10FFFF; $start += 4096) {
        $cps = [];
        $in = '';
        for ($cp = $start; $cp < $start + 4096 && $cp <= 0x10FFFF; $cp++) {
            if (($cp >= 0xD800 && $cp <= 0xDFFF) || ($cp >= 0xE0000 && $cp <= 0xE007F)) {
                continue;
            }
            $cps[] = $cp;
            $in .= mb_chr($cp, 'UTF-8') . ($iso ? "\x0a" : ($unit === 2 ? "\x01" : $sep));
        }
        $converted = iconv('UTF-8', "$target//TRANSLIT", $in);
        if ($converted === false) {
            fail("batch $target $start failed");
        }
        $parts = [];
        $cur = '';
        if ($iso) {
            // the characters the batch decodes to, split at the line feeds (a G2 byte of ISO-2022-JP-2 may be 0A)
            $cur = [];
            foreach (iso_cps($target, $converted) as $c) {
                if ($c === 0x0A) {
                    $parts[] = $cur;
                    $cur = [];
                } else {
                    $cur[] = $c;
                }
            }
        }
        for ($i = 0; !$iso && $i < strlen($converted); $i += $unit) {
            $u = substr($converted, $i, $unit);
            if ($u === $sep) {
                $parts[] = $cur;
                $cur = '';
            } else {
                $cur .= $u;
            }
        }
        if (count($parts) !== count($cps)) {
            fail("batch $target $start: " . count($parts) . ' results for ' . count($cps));
        }
        foreach ($cps as $k => $cp) {
            $direct = $enc[$cp] ?? null;
            if ($direct === null && $target !== 'UCS-2LE' && $cp >= 0x10000 && isset($enc[$cp])) {
                $direct = $enc[$cp];
            }
            if ($iso && $direct !== null) {
                if ($parts[$k] !== [$cp]) {
                    fail(sprintf('%s: U+%04X is %s in a batch', $target, $cp, json_encode($parts[$k])));
                }
            } elseif ($direct !== null) {
                if ($parts[$k] !== $direct) {
                    fail(sprintf('%s: U+%04X is %s alone, %s in a batch', $target, $cp, bin2hex($direct), bin2hex($parts[$k])));
                }
            } elseif ($iso && $parts[$k] !== [0x3F]) {
                $out[$cp] = $parts[$k];
            } elseif (!$iso && $parts[$k] !== ($unit === 2 ? "?\x00" : '?')) {
                $out[$cp] = $parts[$k];
            }
        }
    }
    return $out;
}

/** Code points in UCS-4BE. */
function ucs4_of(array $cps): string
{
    return implode('', array_map(static fn (int $c): string => pack('N', $c), $cps));
}

/** The bytes of code points $cps in a target, null when one of them cannot be represented. */
function encode_all(array $enc, array $cps): ?string
{
    $s = '';
    foreach ($cps as $cp) {
        $b = $enc[$cp] ?? null;
        if ($b === null) {
            return null;
        }
        $s .= $b;
    }
    return $s;
}

$enc = [];
$measured = [];
foreach ($targets as $target) {
    $enc[$target] = repertoire($target);
    $measured[$target] = measure($target, $enc[$target]);
}

// the results as code points: one per byte (or UCS-2 unit), except where a charset writes a character as several
// bytes (CP1255, CP1258), where a result is one another target gave if it gives the same bytes
$decode = [];
foreach ($targets as $target) {
    if ($target === 'UCS-2LE') {
        continue;
    }
    foreach ($enc[$target] as $cp => $b) {
        if ($b !== null && strlen($b) === 1 && !isset($decode[$target][$b])) {
            $decode[$target][$b] = $cp;
        }
    }
}
$results = []; // code point => target => list of code points
$deferred = [];
foreach ($targets as $target) {
    // (also a codepage that writes several characters as one byte, U+2190 and U+001B as 0x1B: its bytes say less
    // than the other targets)
    $multi = in_array($target, MULTIBYTES, true);
    $writers = [];
    foreach ($enc[$target] as $b) {
        $multi = $multi || ($b !== null && strlen($b) > 1 && $target !== 'UCS-2LE');
        if ($b !== null && strlen($b) === 1) {
            $writers[$b] = ($writers[$b] ?? 0) + 1;
            $multi = $multi || $writers[$b] > 1;
        }
    }
    foreach ($measured[$target] as $cp => $bytes) {
        if (in_array($target, ISO2022S, true)) {
            $results[$cp][$target] = $bytes;
            continue;
        }
        if ($target === 'UCS-2LE') {
            $results[$cp][$target] = array_values(unpack('v*', $bytes));
            continue;
        }
        if ($multi) {
            $deferred[] = [$target, $cp, $bytes];
            continue;
        }
        $results[$cp][$target] = array_map(static fn (string $b): int => $decode[$target][$b], str_split($bytes));
    }
}
foreach ($deferred as [$target, $cp, $bytes]) {
    $match = null;
    foreach ($results[$cp] ?? [] as $alternative) {
        if (encode_all($enc[$target], $alternative) === $bytes) {
            $match = $alternative;
            break;
        }
    }
    if ($match === null && in_array($target, MULTIBYTES, true)) {
        // a replacement no byte-wise target gave: its code points as the target decodes them
        $match = array_values(unpack('N*', iconv($target, 'UCS-4BE', $bytes)) ?: []);
        if (encode_all($enc[$target], $match) !== $bytes) {
            fail(sprintf('%s: U+%04X becomes %s, which does not decode back', $target, $cp, bin2hex($bytes)));
        }
    }
    if ($match === null && !in_array($target, MULTIBYTES, true)) {
        // a byte several characters are written as: the one no target that gives "?" could write
        $match = [];
        foreach (str_split($bytes) as $byte) {
            $candidates = [];
            foreach ($enc[$target] as $c => $b) {
                if ($b !== $byte) {
                    continue;
                }
                foreach ($targets as $t) {
                    if (($enc[$t][$cp] ?? null) === null && !isset($measured[$t][$cp]) && ($enc[$t][$c] ?? null) !== null) {
                        continue 2;
                    }
                }
                $candidates[] = $c;
            }
            if ($candidates === []) {
                fail(sprintf('%s: U+%04X becomes %s, which no character explains', $target, $cp, bin2hex($bytes)));
            }
            $match[] = $candidates[0];
        }
    }
    $results[$cp][$target] = $match;
}
ksort($results);

// order the alternatives: a target's result comes before every other alternative the target can represent
$table = [];
foreach ($results as $cp => $byTarget) {
    $alternatives = [];
    foreach ($byTarget as $r) {
        $alternatives['u' . implode(',', $r)] = $r; // not numeric: PHP would turn such keys into integers
    }
    ksort($alternatives, SORT_STRING);
    $after = [];
    $before = array_fill_keys(array_keys($alternatives), 0);
    foreach ($targets as $target) {
        if (($enc[$target][$cp] ?? null) !== null) {
            continue;
        }
        $chosen = isset($byTarget[$target]) ? 'u' . implode(',', $byTarget[$target]) : null;
        foreach ($alternatives as $key => $r) {
            if ($key === $chosen || encode_all($enc[$target], $r) === null) {
                continue;
            }
            if ($chosen === null) {
                fail(sprintf('U+%04X: %s gives "?" but could write %s', $cp, $target, $key));
            }
            if (!isset($after[$chosen][$key])) {
                $after[$chosen][$key] = true;
                $before[$key]++;
            }
        }
    }
    $order = [];
    $ready = array_keys(array_filter($before, static fn (int $n): bool => $n === 0));
    while ($ready !== []) {
        $key = array_shift($ready);
        $order[] = $alternatives[$key];
        foreach (array_keys($after[$key] ?? []) as $next) {
            if (--$before[$next] === 0) {
                $ready[] = $next;
            }
        }
    }
    if (count($order) !== count($alternatives)) {
        fail(sprintf('U+%04X: the targets disagree on the order of its alternatives', $cp));
    }
    $table[$cp] = $order;
}

// check: the first representable alternative is every target's result
foreach ($targets as $target) {
    for ($cp = 0x80; $cp <= 0x10FFFF; $cp++) {
        if (($enc[$target][$cp] ?? null) !== null || ($cp >= 0xD800 && $cp <= 0xDFFF)
            || ($cp >= 0xE0000 && $cp <= 0xE007F)) {
            continue;
        }
        $got = null;
        foreach ($table[$cp] ?? [] as $r) {
            $got = encode_all($enc[$target], $r);
            if ($got !== null) {
                if (in_array($target, ISO2022S, true)) {
                    $got = $r;
                }
                break;
            }
        }
        if (in_array($target, ISO2022S, true)) {
            if ($got !== ($measured[$target][$cp] ?? null)) {
                fail(sprintf('%s: U+%04X would be %s instead of %s', $target, $cp, json_encode($got), json_encode($measured[$target][$cp] ?? null)));
            }
            continue;
        }
        if ($got !== ($measured[$target][$cp] ?? null)) {
            fail(sprintf('%s: U+%04X would be %s instead of %s', $target, $cp, bin2hex((string) $got), bin2hex($measured[$target][$cp] ?? '?')));
        }
    }
}

// Alternatives no target shows: glibc has alternatives for some characters that fail in every target (they hold a
// character none of them can represent). They matter where the output space runs out (glibc wants room for a byte
// before each character of an alternative, also before one it then cannot represent) and in the ISO-2022 charsets
// (an attempt that fails partway keeps the designations it reached). translit_probe.c measures what shows of them:
// the characters ISO-2022-KR cannot represent that have alternatives ("entries"), and the least room ASCII needs for
// each character ("kmin": two bytes where an alternative starts with a character ASCII has). The table stands for such
// an alternative by PHANTOM, a code point no charset has, after "(" in the second case (all that shows of its first
// character: one every ISO 646 variant has, and that JIS-Roman has in ISO-2022-JP, as it has no space).
const PHANTOM = 0xFFFFFFFF;
/** repertoire(), computed once per target. */
function repertoire_cached(string $target): array
{
    static $cache = [];
    return $cache[$target] ??= repertoire($target);
}
$probe = '/tmp/translit_probe';
shell_exec('gcc -O2 -o ' . $probe . ' ' . escapeshellarg(__DIR__ . '/translit_probe.c'));
if (!is_executable($probe)) {
    fail('cannot build translit_probe.c');
}
$phantoms = 0;
foreach (array_filter(explode("\n", (string) shell_exec("$probe entries"))) as $h) {
    $cp = (int) hexdec($h);
    if (!isset($table[$cp])) {
        $table[$cp] = [[PHANTOM]];
        $phantoms++;
    }
}
/**
 * The least output space in which the attempts at $alternatives write one of them (or the default "?"), glibc wanting
 * room for $room($c) bytes before each character $c, and $first before the character transliterated.
 */
function kmin_of(array $enc, array $alternatives, callable $room, int $first): int
{
    for ($k = $first; ; $k++) {
        foreach ($alternatives as $r) {
            $used = 0;
            foreach ($r as $c) {
                if ($k - $used < $room($c)) {
                    continue 3; // no room for the next character
                }
                if (($enc[$c] ?? null) === null) {
                    continue 2; // the next alternative
                }
                $used += strlen($enc[$c]);
                if ($used > $k) {
                    continue 3;
                }
            }
            return $k;
        }
        return $k;
    }
}
/** The least room glibc's probe measured for the characters of $target that need more than $base: code point => K. */
function measured_kmin(string $probe, string $target, int $base): array
{
    $kmin = [];
    foreach (array_filter(explode("\n", (string) shell_exec("$probe kmin $target"))) as $l) {
        [$h, $k] = explode(' ', $l);
        if ((int) $k > $base) {
            $kmin[(int) hexdec($h)] = (int) $k;
        }
    }
    return $kmin;
}
// ASCII wants a byte before every character; EUC-JP two before every one it does not write as one byte (see
// Charset::room in codec.rs). Where an alternative no target shows starts with characters a target has, the room the
// target needs shows them: ASCII shows one it has (taken as "("), EUC-JP longer ones, of which the first character of
// one of the character's alternatives is taken where it explains what EUC-JP needs.
$ascii = $enc['ANSI_X3.4-1968'];
$eucjp = $enc['EUC-JP'];
$asciiRoom = static fn (int $c): int => 1;
$eucjpRoom = static fn (int $c): int => strlen($eucjp[$c] ?? '') === 1 ? 1 : 2;
$asciiKmin = measured_kmin($probe, 'ANSI_X3.4-1968', 1);
$eucjpKmin = measured_kmin($probe, 'EUC-JP', 2);
$unresolved = [];
for ($cp = 0x80; $cp <= 0x10FFFF; $cp++) {
    if (($cp >= 0xD800 && $cp <= 0xDFFF) || ($cp >= 0xE0000 && $cp <= 0xE007F)) {
        continue;
    }
    $checks = [];
    if (($ascii[$cp] ?? null) === null) {
        $checks[] = [$ascii, $asciiRoom, 1, $asciiKmin[$cp] ?? 1];
    }
    if (($eucjp[$cp] ?? null) === null) {
        $checks[] = [$eucjp, $eucjpRoom, 2, $eucjpKmin[$cp] ?? 2];
    }
    $fits = static function (array $order) use ($checks): bool {
        foreach ($checks as [$e, $room, $first, $want]) {
            if (kmin_of($e, $order, $room, $first) !== $want) {
                return false;
            }
        }
        return true;
    };
    $order = $table[$cp] ?? [];
    if ($fits($order)) {
        continue;
    }
    if (($order[0] ?? null) === [PHANTOM]) {
        array_shift($order);
    }
    $candidates = [0x28];
    foreach ($order as $r) {
        if ($r !== [] && $r[0] !== PHANTOM && !in_array($r[0], $candidates, true)) {
            $candidates[] = $r[0];
        }
    }
    foreach ($candidates as $c) {
        $try = array_merge([[$c, PHANTOM]], $order);
        if ($fits($try)) {
            $table[$cp] = $try;
            $phantoms++;
            continue 2;
        }
    }
    $unresolved[] = sprintf('U+%04X', $cp);
}
// glibc's own "?" as an alternative of a character, which the targets show as its default "?": it is written whole
// or not at all, where the default "?" keeps what fits of it (translit_probe.c "whole": after a character of JIS X 0208,
// three bytes in ISO-2022-JP are enough for the escape sequence of ASCII but not for "?" as well)
$explicit = 0;
$jp = repertoire_cached('ISO-2022-JP');
foreach (array_filter(explode("\n", (string) shell_exec("$probe whole"))) as $h) {
    $cp = (int) hexdec($h);
    $order = $table[$cp] ?? [];
    if (in_array(sprintf('U+%04X', $cp), $unresolved, true)) {
        continue;
    }
    // only where every alternative fails at its first character: what an attempt writes first would explain it too
    foreach ($order as $r) {
        if ($r !== [] && ($jp[$r[0]] ?? null) !== null) {
            continue 2;
        }
    }
    $table[$cp] = $order === [[PHANTOM]] ? [[0x3F]] : array_merge($order, [[0x3F]]);
    $explicit++;
}
ksort($table);
fwrite(STDERR, sprintf("%d alternatives no target shows and %d \"?\"; the room needed unexplained for %s\n", $phantoms,
    $explicit, $unresolved === [] ? 'none' : implode(' ', $unresolved)));

// The ISO-2022 targets: glibc encodes the alternatives with the designations of the moment, and an attempt that fails
// partway is discarded but keeps the designations it reached, as the engine does. translit_probe.c ("stale") gives
// what that makes of the table's alternatives from a few states; every character whose replacement glibc writes
// otherwise from one of them (the table misses an alternative that changes designations) is listed: twig-rs refuses
// to transliterate it into that charset.
function tag_ucs4(string $lang): string
{
    $s = pack('N', 0xE0001);
    foreach (str_split($lang) as $ch) {
        $s .= pack('N', 0xE0000 + ord($ch));
    }
    return $s;
}
$isoStates = [
    'ISO-2022-JP' => ['', pack('N', 0x3042), pack('N', 0xA5)],
    'ISO-2022-JP-2' => ['', pack('N', 0x3042), pack('N', 0xAC00), pack('N', 0x4E02), pack('N', 0xA0), tag_ucs4('zh'),
        tag_ucs4('ko') . pack('N', 0x3042), pack('N', 0x3B1) . tag_ucs4('ja'), pack('N', 0x20AC)],
    // the header comes with the first character (a state of its own here)
    'ISO-2022-KR' => [pack('N', 0x61), pack('N', 0xAC00)],
];
$unmodelled = [];
foreach (ISO2022S as $target) {
    $enc[$target] = repertoire_cached($target);
    $unmodelled[$target] = [];
    // the attempts at each character: the characters before the first one the charset cannot represent of each
    // alternative that fails (glibc writes them and discards them), then the one that works (or the default "?")
    $attempts = [-1 => [[], [0x3F]]]; // -1: the characters without alternatives
    for ($cp = 0x80; $cp <= 0x10FFFF; $cp++) {
        if (($enc[$target][$cp] ?? null) !== null || !isset($table[$cp])) {
            continue;
        }
        $p = [];
        $w = [0x3F];
        foreach ($table[$cp] as $r) {
            $prefix = [];
            foreach ($r as $c) {
                if (($enc[$target][$c] ?? null) === null) {
                    array_push($p, ...$prefix);
                    continue 2;
                }
                $prefix[] = $c;
            }
            $w = $r;
            break;
        }
        $attempts[$cp] = [$p, $w];
    }
    $lines = '';
    $order = [];
    foreach ($isoStates[$target] as $si => $x) {
        foreach ($attempts as $cp => [$p, $w]) {
            $lines .= ($x === '' ? '-' : bin2hex($x)) . ' ' . ($p === [] ? '-' : bin2hex(ucs4_of($p))) . ' '
                . ($w === [] ? '-' : bin2hex(ucs4_of($w))) . "\n";
            $order[] = [$si, $cp];
        }
    }
    file_put_contents('/tmp/stale.txt', $lines);
    $predicted = [];
    foreach (explode("\n", trim((string) shell_exec("$probe stale $target < /tmp/stale.txt"))) as $i => $l) {
        [$si, $cp] = $order[$i];
        $bytes = array_map(static fn (string $h): string => $h === '-' ? '' : (string) hex2bin($h), explode(' ', $l));
        // ISO-2022-KR writes its header again before a replacement during the encoder's first run
        $predicted[$si][$cp] = $bytes[0] . ($target === 'ISO-2022-KR' ? "\x1b\$)C" : '') . $bytes[1] . $bytes[2];
    }
    for ($cp = 0x80; $cp <= 0x10FFFF; $cp++) {
        if (($enc[$target][$cp] ?? null) !== null || ($cp >= 0xD800 && $cp <= 0xDFFF) || ($cp >= 0xE0000 && $cp <= 0xE007F)) {
            continue;
        }
        foreach ($isoStates[$target] as $si => $x) {
            $got = iconv('UCS-4BE', "$target//TRANSLIT", $x . pack('N', $cp));
            if ($got !== $predicted[$si][isset($attempts[$cp]) ? $cp : -1]) {
                $unmodelled[$target][] = $cp;
                break;
            }
        }
    }
    fwrite(STDERR, sprintf("%s: %d characters whose transliteration the table does not model\n", $target, count($unmodelled[$target])));
}

$keys = [];
$spans = [];
$data = [];
$index = [];
foreach ($table as $cp => $order) {
    foreach ($order as $r) {
        $k = implode(',', $r);
        if (!isset($index[$k])) {
            // reuse an identical earlier replacement
            $index[$k] = count($data);
            array_push($data, ...$r);
        }
        $keys[] = $cp;
        $spans[] = sprintf('(%d, %d)', $index[$k], count($r));
    }
}

echo "//! Generated by translit.gen.php (black-box: PHP iconv() with //TRANSLIT and translit_probe.c in the oracle); do\n";
echo "//! not edit.\n";
echo "//!\n";
echo "//! glibc 2.41 transliteration under LC_CTYPE=C.UTF-8, measured for every target with a smaller repertoire than\n";
echo "//! Unicode: the alternatives of each character in the order in which glibc tries them, as far as the targets show\n";
echo "//! it; 0xFFFFFFFF stands for a character of an alternative no target can represent. Characters that are not listed\n";
echo "//! become \"?\".\n\n";
printf("/// The code points with replacements, sorted; one entry per alternative, in order.\npub(super) static KEYS: [u32; %d] = [\n", count($keys));
foreach (array_chunk($keys, 12) as $chunk) {
    echo '    ', implode(', ', array_map(static fn (int $c): string => sprintf('0x%04X', $c), $chunk)), ",\n";
}
echo "];\n\n";
printf("/// `(offset, length)` of the alternative of each `KEYS` entry in `DATA`.\npub(super) static SPANS: [(u16, u8); %d] = [\n", count($spans));
foreach (array_chunk($spans, 10) as $chunk) {
    echo '    ', implode(', ', $chunk), ",\n";
}
echo "];\n\n";
printf("/// The alternatives, concatenated.\npub(super) static DATA: [u32; %d] = [\n", count($data));
foreach (array_chunk($data, 12) as $chunk) {
    echo '    ', implode(', ', array_map(static fn (int $c): string => sprintf('0x%04X', $c), $chunk)), ",\n";
}
echo "];\n";
foreach ($unmodelled as $target => $cps) {
    printf("\n/// The characters whose //TRANSLIT replacement in %s the table does not model (glibc keeps the designations\n/// of an alternative it tries and rejects that no target shows), sorted.\npub(super) static %s: [u32; %d] = [\n", $target,
        strtoupper(str_replace('-', '_', $target)) . '_UNMODELLED', count($cps));
    foreach (array_chunk($cps, 12) as $chunk) {
        echo '    ', implode(', ', array_map(static fn (int $c): string => sprintf('0x%04X', $c), $chunk)), ",\n";
    }
    echo "];\n";
}
