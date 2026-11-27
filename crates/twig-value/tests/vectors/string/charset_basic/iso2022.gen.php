<?php
// iso2022.json: every short byte sequence and every code point of the ISO-2022 charsets (iso2022.rs), from every
// designation and shift state, hashed per block of results like codepages.json (token() in common.inc.php):
//   state12   after the bytes "prefix" (a designation, a shift), every one- and two-byte input, alone and followed by
//             "!!" (into UCS-4BE, UCS-4BE//IGNORE);
//   esc       after "prefix", ESC and every two bytes, and ESC $ (, ESC $ ), ESC ., ESC N with every byte;
//   bmp       every BMP code point from UCS-4BE after the code points "prefix" (which select a set or a language);
//   translit  every code point U+0000..U+10FFFF (surrogates aside, and those of "skip": the characters whose
//             replacement twig-rs refuses, see iso2022_cases.json), 4096 per call, each followed by a line feed,
//             encoded with //TRANSLIT;
//   ignore    the same code points, 4096 per call, without line feeds, encoded with //IGNORE;
//   tags      after the code points "prefix", U+E0001 LANGUAGE TAG and every one or two tag characters
//             (U+E0000..U+E007F), and every three of TAG_ALPHABET, each followed by TAG_PROBE (ISO-2022-JP-2 reads the
//             language of a tag from its first two characters, letters of either case).
// A set is {"from", "to", "kind", "prefix" (hex), "blocks": [fnv1a64 of the concatenated tokens of each block]};
// tests/charset_basic.rs builds the same inputs and tokens. Mount the crate (the skip lists come from
// src/builtins/string/charset_basic/translit_table.rs):
//   cd crates/twig-value && MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/c" \
//       twig-oracle:3.31.1 php /c/tests/vectors/string/charset_basic/iso2022.gen.php \
//       > tests/vectors/string/charset_basic/iso2022.json

require __DIR__ . '/common.inc.php';

/**
 * The characters whose //TRANSLIT replacement in each ISO-2022 charset twig-rs refuses (translit.gen.php measures
 * them into translit_table.rs): left out of the translit sets, listed in their "skip".
 */
function unmodelled(string $cs): array
{
    $table = file_get_contents(__DIR__ . '/../../../../src/builtins/string/charset_basic/translit_table.rs');
    $name = strtoupper(str_replace('-', '_', $cs)) . '_UNMODELLED';
    if ($table === false || !preg_match('/static ' . $name . ': \[u32; \d+\] = \[(.*?)\];/s', $table, $m)) {
        throw new RuntimeException("no $name in translit_table.rs");
    }
    preg_match_all('/0x[0-9A-F]+/', $m[1], $cps);
    return array_map('hexdec', $cps[0]);
}

/**
 * The tag characters (minus U+E0000) of the three-character tags of the tags kind: U+E0000, U+E0001, controls, the
 * hyphen, a digit, the punctuation next to the letters, the letters of ja, ko and zh and x in both cases, CANCEL TAG.
 */
const TAG_ALPHABET = [0x00, 0x01, 0x02, 0x1F, 0x20, 0x2D, 0x30, 0x40, 0x41, 0x48, 0x4A, 0x4B, 0x4F, 0x58, 0x5A, 0x5B,
    0x5F, 0x60, 0x61, 0x68, 0x6A, 0x6B, 0x6F, 0x78, 0x7A, 0x7B, 0x7E, 0x7F];
/** What follows each tag of the tags kind: characters that each language (or none) writes in other sets. */
const TAG_PROBE = [0x4E2D, 0x6C49, 0xE9, 0xAC00, 0x3B1];

/** @return list<list<string>> the inputs of a kind after a prefix (without the code points $skip), in blocks */
function iso2022_inputs(string $kind, string $prefix, array $skip = []): array
{
    $skip = array_flip($skip);
    $blocks = [];
    switch ($kind) {
        case 'state12':
            $block = [];
            for ($b = 0; $b < 256; $b++) {
                $block[] = $prefix . chr($b);
                $block[] = $prefix . chr($b) . '!!';
            }
            $blocks[] = $block;
            for ($b0 = 0; $b0 < 256; $b0++) {
                $block = [];
                for ($b1 = 0; $b1 < 256; $b1++) {
                    $block[] = $prefix . chr($b0) . chr($b1);
                    $block[] = $prefix . chr($b0) . chr($b1) . '!!';
                }
                $blocks[] = $block;
            }
            break;
        case 'esc':
            for ($b0 = 0; $b0 < 256; $b0++) {
                $block = [];
                for ($b1 = 0; $b1 < 256; $b1++) {
                    $block[] = $prefix . "\x1b" . chr($b0) . chr($b1);
                }
                $blocks[] = $block;
            }
            foreach (["\x1b\$(", "\x1b\$)", "\x1b.", "\x1bN"] as $start) {
                $block = [];
                for ($b = 0; $b < 256; $b++) {
                    $block[] = $prefix . $start . chr($b);
                    $block[] = $prefix . $start . chr($b) . 'a';
                }
                $blocks[] = $block;
            }
            break;
        case 'bmp':
            foreach (array_chunk(range(0, 0xFFFF), 256) as $chunk) {
                $blocks[] = array_map(static fn (int $c): string => $prefix . pack('N', $c), $chunk);
            }
            break;
        case 'translit':
        case 'ignore':
            $sep = $kind === 'translit' ? pack('N', 0x0A) : '';
            for ($start = 0; $start < 0x110000; $start += 4096) {
                $in = '';
                for ($c = $start; $c < $start + 4096; $c++) {
                    if (($c < 0xD800 || $c > 0xDFFF) && !isset($skip[$c])) {
                        $in .= pack('N', $c) . $sep;
                    }
                }
                $blocks[] = [$in];
            }
            break;
        case 'tags':
            $tag = static fn (int ...$t): string => $prefix . implode('', array_map(static fn (int $c): string =>
                pack('N', 0xE0000 + $c), [0x01, ...$t])) . implode('', array_map(static fn (int $c): string =>
                pack('N', $c), TAG_PROBE));
            for ($t0 = 0; $t0 < 0x80; $t0++) {
                $block = [$tag($t0)];
                for ($t1 = 0; $t1 < 0x80; $t1++) {
                    $block[] = $tag($t0, $t1);
                }
                $blocks[] = $block;
            }
            foreach (TAG_ALPHABET as $t0) {
                $block = [];
                foreach (TAG_ALPHABET as $t1) {
                    foreach (TAG_ALPHABET as $t2) {
                        $block[] = $tag($t0, $t1, $t2);
                    }
                }
                $blocks[] = $block;
            }
            break;
        default:
            throw new RuntimeException("unknown kind $kind");
    }
    return $blocks;
}

/** A language tag in UCS-4BE. */
function tag(string $lang): string
{
    $s = pack('N', 0xE0001);
    foreach (str_split($lang) as $ch) {
        $s .= pack('N', 0xE0000 + ord($ch));
    }
    return $s;
}

$jp = ['', "\x1b(J", "\x1b\$@", "\x1b\$B"];
$jp2 = array_merge($jp, ["\x1b\$A", "\x1b\$(C", "\x1b\$(D", "\x1b(I", "\x1b.A", "\x1b.F", "\x1b\$B\x1b.F"]);
$kr = ['', "\x0e", "\x1b\$)C\x0e"];
$states = [
    'ISO-2022-JP' => ['', 0x61, 0xA5, 0x3042],
    'ISO-2022-JP-2' => ['', 0x61, 0xA5, 0x3042, 0x4E02, 0x2C9, 0xAC00, 0xFF61, 0xA0, 0x37A, 0x3042, [tag('zh')],
        [tag('ko')], [tag('ja'), 0xAC00], [tag('zh'), 0x3042], [tag('ko'), 0x3042], [tag('zh') . pack('N', 0xE007F), 0xAC00]],
    'ISO-2022-KR' => ['', 0x61, 0xAC00],
];
$sets = [];
foreach (['ISO-2022-JP' => $jp, 'ISO-2022-JP-2' => $jp2, 'ISO-2022-KR' => $kr] as $cs => $prefixes) {
    $plan = [];
    foreach ($prefixes as $prefix) {
        $plan[] = [$cs, 'UCS-4BE', 'state12', $prefix];
        $plan[] = [$cs, 'UCS-4BE//IGNORE', 'state12', $prefix];
        $plan[] = [$cs, 'UCS-4BE', 'esc', $prefix];
    }
    foreach ($states[$cs] as $x) {
        $prefix = is_array($x) ? $x[0] . (isset($x[1]) ? pack('N', $x[1]) : '') : ($x === '' ? '' : pack('N', $x));
        $plan[] = ['UCS-4BE', $cs, 'bmp', $prefix];
    }
    array_push($plan, ['UCS-4BE', "$cs//TRANSLIT", 'translit', ''], ['UCS-4BE', "$cs//IGNORE", 'ignore', '']);
    if ($cs === 'ISO-2022-JP-2') {
        // language tags after none, after one for ko (in capitals) and after one for zh and a line feed
        foreach (['', tag('KO') . pack('N', 0x4E2D), tag('zh') . pack('N', 0x0A)] as $prefix) {
            $plan[] = ['UCS-4BE', $cs, 'tags', $prefix];
        }
    }
    foreach ($plan as [$from, $to, $kind, $prefix]) {
        $skip = $kind === 'translit' ? unmodelled($cs) : [];
        $hashes = [];
        foreach (iso2022_inputs($kind, $prefix, $skip) as $block) {
            $t = '';
            foreach ($block as $input) {
                $t .= token($from, $to, $input);
            }
            $hashes[] = hash('fnv1a64', $t);
        }
        $sets[] = ['from' => $from, 'to' => $to, 'kind' => $kind, 'prefix' => bin2hex($prefix), 'skip' => $skip,
            'blocks' => $hashes];
    }
}

echo '{"generator":"iso2022.gen.php","php":"', PHP_VERSION, "\",\n\"sets\":[\n";
foreach ($sets as $k => $s) {
    echo json_encode($s, JSON_UNESCAPED_SLASHES | JSON_THROW_ON_ERROR), $k + 1 < count($sets) ? ",\n" : "\n";
}
echo "]}\n";
