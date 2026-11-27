<?php
// calls.json: single iconv() calls, for the model of one call (src/builtins/string/charset_basic/engine.rs), measured
// in the oracle with translit_probe.c ("seq", public iconv API only), which sets the output space of each call as PHP's
// iconv() cannot. Two kinds of cases, all from UCS-4BE:
//
//   ["seq", to, input, sizes, result]   successive calls with the listed output spaces ("f": the flush with 64 bytes
//                                       left); result: " [STATUS c=CONSUMED out=HEX]" (" [flush STATUS out=HEX]")
//                                       per call, STATUS being OK, E2BIG, EILSEQ or EINVAL;
//   ["room", to, input, first, result]  a call with FIRST bytes (which stops before the character after the prefix)
//                                       and then, on a descriptor of its own for each K = 1, 2, ..., the same call
//                                       followed by one with K bytes, up to the first K that converts the character:
//                                       result " first=STATUS,CONSUMED,HEX" and " K:STATUS:CONSUMED:HEX" for each K
//                                       whose second call writes something or does not end with E2BIG.
//
// to indexes "names"; results longer than 64 characters are "#" and their fnv1a64. What the cases cover: //TRANSLIT
// into the ISO-2022 charsets at every output space (designations that stay, ISO-2022-KR's header, the default "?"
// against an alternative), the room glibc wants before a character in every target and with every suffix, and the
// alternatives of a sample of the transliteration table (and of those no target shows) where the room runs out. The
// characters whose transliteration into an ISO-2022 charset twig-rs refuses are left out.
//
// Regenerate (from crates/twig-value, Git Bash):
//   MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/c" twig-oracle:3.31.1 \
//       php /c/tests/vectors/string/charset_basic/calls.gen.php > tests/vectors/string/charset_basic/calls.json

require __DIR__ . '/common.inc.php';

$dir = __DIR__ . '/../../../../src/builtins/string/charset_basic';
$probe = '/tmp/translit_probe';
shell_exec('gcc -O2 -o ' . $probe . ' ' . escapeshellarg("$dir/translit_probe.c"));
if (!is_executable($probe)) {
    throw new RuntimeException('cannot build translit_probe.c');
}
$table = (string) file_get_contents("$dir/translit_table.rs");

/** A static array of code points of translit_table.rs. */
function table_array(string $table, string $name): array
{
    if (!preg_match('/static ' . $name . ': \[u32; \d+\] = \[(.*?)\];/s', $table, $m)) {
        throw new RuntimeException("no $name in translit_table.rs");
    }
    preg_match_all('/0x[0-9A-F]+/', $m[1], $cps);
    return array_map('hexdec', $cps[0]);
}

$refused = [];
foreach (['ISO-2022-JP', 'ISO-2022-JP-2', 'ISO-2022-KR'] as $cs) {
    $refused[$cs] = array_flip(table_array($table, strtoupper(str_replace('-', '_', $cs)) . '_UNMODELLED'));
}
/** Whether twig-rs refuses the //TRANSLIT replacement of one of $cps into $to. */
function refused(string $to, array $cps): bool
{
    $base = explode('//', $to)[0];
    foreach ($cps as $c) {
        if (isset($GLOBALS['refused'][$base][$c]) && str_contains($to, '//TRANSLIT')) {
            return true;
        }
    }
    return false;
}

/** Runs seq lines [to, input, sizes] through the probe: the result of each. */
function run(array $lines): array
{
    $text = '';
    foreach ($lines as [$to, $input, $sizes]) {
        $text .= bin2hex('UCS-4BE') . ' ' . bin2hex($to) . ' ' . ($input === '' ? '-' : bin2hex($input)) . " $sizes\n";
    }
    file_put_contents('/tmp/calls.txt', $text);
    $out = explode("\n", trim((string) shell_exec($GLOBALS['probe'] . ' seq < /tmp/calls.txt')));
    if (count($out) !== count($lines)) {
        throw new RuntimeException('the probe gave ' . count($out) . ' results for ' . count($lines));
    }
    return array_map(static fn (string $l): string => substr($l, strpos($l, ':') + 1), $out);
}

function compact_result(string $r): string
{
    return strlen($r) > 64 ? '#' . hash('fnv1a64', $r) : $r;
}

$cases = [];

// //TRANSLIT into the ISO-2022 charsets at every output space, then the rest and the flush
$seq = [];
$isoTranslit = [
    ['ISO-2022-KR//TRANSLIT', [[0x61], [0xAC00]]],
    ['ISO-2022-JP//TRANSLIT', [[0x61], [0x3042], [0xA5]]],
    ['ISO-2022-JP-2//TRANSLIT', [[0x3042], [0xE9], [0xAC00], [0xA0], [0x20AC]]],
];
$xs = [0xFFFF, 0x2C9, 0xFB01, 0xE9, 0xFE16, 0x226D, 0x3200, 0x3395, 0x20AB, 0x1F600, 0x3231, 0x2026, 0x300, 0x2460];
foreach ($isoTranslit as [$to, $prefixes]) {
    foreach ($prefixes as $pre) {
        foreach ($xs as $x) {
            if (refused($to, [$x])) {
                continue;
            }
            foreach (array_merge([''], range(1, 16)) as $k) {
                $seq[] = [$to, ucs4(...[...$pre, $x, 0x61, 0x3042]), $k === '' ? '200,f' : "$k,200,f"];
            }
        }
    }
}
// the byte order mark of UTF-16 and UTF-32 before a replacement during the first run
foreach (['UTF-16//TRANSLIT', 'UTF-32//TRANSLIT', 'UCS-2//TRANSLIT'] as $to) {
    foreach ([0xD800, 0x110000, 0x1F600] as $x) {
        foreach (range(1, 12) as $k) {
            $seq[] = [$to, ucs4(0x61, $x, 0x61), "$k,200,f"];
            $seq[] = [$to, ucs4($x, 0x61), "$k,200,f"];
        }
    }
}
foreach (run($seq) as $i => $r) {
    [$to, $input, $sizes] = $seq[$i];
    $cases[] = ['seq', name_index($to), bin2hex($input), $sizes, compact_result($r)];
}

// the room glibc wants before a character: every target, every suffix, characters of every kind
$chars = [0xFFFF, 0x300, 0xA0, 0x3226, 0x1F600, 0x20AC, 0xE9, 0x3042, 0xAC00, 0x4E00, 0x2460, 0xFF61, 0x10000, 0x80,
    0x85, 0x8E, 0x8F, 0x9F, 0xA5, 0x203E, 0xFF71, 0x4E02, 0x554A, 0x3B1, 0xE0041, 0x226D, 0x3395, 0x20AB, 0x309B];
$rooms = [];
// (every fourth codepage: they all want a byte)
$someCodepages = array_values(array_filter(CODEPAGES, static fn (int $i): bool => $i % 4 === 0, ARRAY_FILTER_USE_KEY));
foreach (array_merge(['ASCII', 'ISO-8859-1', 'UTF-8'], $someCodepages, MULTIBYTES) as $t) {
    $rooms[] = [$t, [0x61], 1];
}
foreach (['UTF-16' => 4, 'UTF-16LE' => 2, 'UCS-2' => 4, 'UTF-32' => 8, 'UCS-4' => 4] as $t => $first) {
    $rooms[] = [$t, [0x61], $first];
}
$rooms = array_merge($rooms, [
    ['ISO-2022-JP', [0x61], 1], ['ISO-2022-JP', [0x3042], 5], ['ISO-2022-JP', [0xA5], 4],
    ['ISO-2022-JP-2', [0x61], 1], ['ISO-2022-JP-2', [0x3042], 5], ['ISO-2022-JP-2', [0xE9], 6],
    ['ISO-2022-JP-2', [0x4E02], 6], ['ISO-2022-JP-2', [0xAC00], 6], ['ISO-2022-JP-2', [0x3B1], 5],
    ['ISO-2022-JP-2', [0xA0], 7], ['ISO-2022-KR', [0x61], 5], ['ISO-2022-KR', [0xAC00], 7],
]);
$sweeps = [];
foreach ($rooms as [$t, $pre, $first]) {
    foreach (['', '//TRANSLIT', '//IGNORE'] as $suffix) {
        foreach ($chars as $x) {
            if (!refused($t . $suffix, [$x])) {
                $sweeps[] = [$t . $suffix, ucs4(...[...$pre, $x, 0x61]), $first];
            }
        }
    }
}
// the alternatives of a sample of the table where the room runs out
$keys = array_values(array_unique(table_array($table, 'KEYS')));
$data = table_array($table, 'DATA');
// (and characters with alternatives no target shows)
$sample = [0x33A8, 0x20AB, 0x3395, 0x3396, 0xFE16, 0x226D, 0x3200, 0x2280, 0x309B, 0x321D];
foreach ($keys as $i => $cp) {
    if ($i % 16 === 0 && !in_array($cp, $sample, true)) {
        $sample[] = $cp;
    }
}
foreach ([['ASCII', [0x61], 1], ['ISO-8859-1', [0x61], 1], ['EUC-JP', [0x61], 1], ['EUC-KR', [0x61], 1],
    ['SJIS', [0x61], 1], ['BIG5', [0x61], 1], ['ISO-2022-JP', [0x3042], 5], ['ISO-2022-JP-2', [0x3042], 5],
    ['ISO-2022-KR', [0xAC00], 7]] as [$t, $pre, $first]) {
    foreach ($sample as $x) {
        if (!refused("$t//TRANSLIT", [$x])) {
            $sweeps[] = ["$t//TRANSLIT", ucs4(...[...$pre, $x, 0x61]), $first];
        }
    }
}
$sweeps = array_values(array_unique($sweeps, SORT_REGULAR));
$lines = [];
foreach ($sweeps as [$to, $input, $first]) {
    foreach (range(1, 12) as $k) {
        $lines[] = [$to, $input, "$first,$k"];
    }
}
$results = run($lines);
foreach ($sweeps as $i => [$to, $input, $first]) {
    $r = '';
    foreach (range(1, 12) as $k) {
        preg_match_all('/\[(\w+) c=(\d+) out=([0-9a-f]*)\]/', $results[12 * $i + $k - 1], $m, PREG_SET_ORDER);
        if ($k === 1) {
            $r .= " first={$m[0][1]},{$m[0][2]},{$m[0][3]}";
        }
        [$status, $consumed, $out] = [$m[1][1], (int) $m[1][2], $m[1][3]];
        if ($consumed > 0 || $status !== 'E2BIG' || $out !== '') {
            $r .= " $k:$status:$consumed:$out";
        }
        if ($consumed > 0 || $status !== 'E2BIG') {
            break;
        }
    }
    $cases[] = ['room', name_index($to), bin2hex($input), $first, compact_result($r)];
}
emit('calls.gen.php', $cases);
