<?php
// multibytes.json: every byte sequence and every code point of each multi-byte charset (multibyte.rs), hashed per
// block of results like codepages.json (token() in common.inc.php):
//   bytes1    every byte on its own, decoded (into UTF-8, UTF-16LE//IGNORE, ASCII//TRANSLIT);
//   bytes12   every one- and two-byte input, decoded (into UCS-4BE, UCS-4BE//IGNORE);
//   follow    every two-byte input followed by A1 41 21, which shows how many bytes //IGNORE skips (into
//             UCS-4BE//IGNORE, and UTF-8//ignore, a suffix PHP does not know, which shows whether glibc reports the
//             skip);
//   bytes3    EUC-JP and EUC-JP-MS: 8F and every two-byte input, alone and followed by A1 41 21;
//   bytes4    EUC-TW: 8E, a plane byte A0..B1 and every two-byte input A0..FF x 00..FF;
//   gb4       GB18030: four-byte inputs: the first byte 81, 82, 84, 85, 8F, 90, 99, E3, E4 or FE, the second 30..39,
//             every third byte, the fourth 2F 30 35 39 3A 40 7F 80 81 FE FF;
//   gb4all    GB18030: every four-byte sequence 81..84 30..39 81..FE 30..39 (the BMP) and every fifth third byte
//             after 90..E3 30..39 (the supplementary planes);
//   bmp       every BMP code point on its own, encoded from UCS-4BE (one call each, no flags);
//   astral    BIG5-HKSCS, EUC-TW and GB18030: U+10000..U+2FFFF, every 31st code point up to U+10FFFF and
//             U+110000..U+1100FF, encoded from UCS-4BE;
//   translit  every code point U+0000..U+10FFFF (surrogates aside), 4096 per call, each followed by a line feed,
//             encoded with //TRANSLIT;
//   ignore    the same code points, 4096 per call, without line feeds, encoded with //IGNORE.
// A set is {"from", "to", "kind", "blocks": [fnv1a64 of the concatenated tokens of each block]}; tests/charset_basic.rs
// builds the same inputs and tokens.

require __DIR__ . '/common.inc.php';

const FOLLOWER = "\xa1\x41!";

/** @return list<list<string>> the inputs of a kind, in blocks */
function multibyte_inputs(string $kind): array
{
    $blocks = [];
    switch ($kind) {
        case 'bytes1':
            $blocks[] = array_map('chr', range(0, 255));
            break;
        case 'bytes12':
        case 'follow':
            if ($kind === 'bytes12') {
                $blocks[] = array_map('chr', range(0, 255));
            }
            for ($b0 = 0; $b0 < 256; $b0++) {
                $block = [];
                for ($b1 = 0; $b1 < 256; $b1++) {
                    $block[] = chr($b0) . chr($b1) . ($kind === 'follow' ? FOLLOWER : '');
                }
                $blocks[] = $block;
            }
            break;
        case 'bytes3':
            for ($b1 = 0; $b1 < 256; $b1++) {
                $block = [];
                for ($b2 = 0; $b2 < 256; $b2++) {
                    $block[] = "\x8f" . chr($b1) . chr($b2);
                    $block[] = "\x8f" . chr($b1) . chr($b2) . FOLLOWER;
                }
                $blocks[] = $block;
            }
            break;
        case 'bytes4':
            for ($p = 0xA0; $p <= 0xB1; $p++) {
                for ($b1 = 0xA0; $b1 <= 0xFF; $b1++) {
                    $block = [];
                    for ($b2 = 0; $b2 < 256; $b2++) {
                        $block[] = "\x8e" . chr($p) . chr($b1) . chr($b2);
                    }
                    $blocks[] = $block;
                }
            }
            break;
        case 'gb4':
            foreach ([0x81, 0x82, 0x84, 0x85, 0x8F, 0x90, 0x99, 0xE3, 0xE4, 0xFE] as $b0) {
                for ($b1 = 0x30; $b1 <= 0x39; $b1++) {
                    $block = [];
                    for ($b2 = 0; $b2 < 256; $b2++) {
                        foreach ([0x2F, 0x30, 0x35, 0x39, 0x3A, 0x40, 0x7F, 0x80, 0x81, 0xFE, 0xFF] as $b3) {
                            $block[] = chr($b0) . chr($b1) . chr($b2) . chr($b3);
                        }
                    }
                    $blocks[] = $block;
                }
            }
            break;
        case 'gb4all':
            for ($b0 = 0x81; $b0 <= 0xE3; $b0 = $b0 === 0x84 ? 0x90 : $b0 + 1) {
                for ($b1 = 0x30; $b1 <= 0x39; $b1++) {
                    $block = [];
                    for ($b2 = 0x81; $b2 <= 0xFE; $b2 += $b0 >= 0x90 ? 5 : 1) {
                        for ($b3 = 0x30; $b3 <= 0x39; $b3++) {
                            $block[] = chr($b0) . chr($b1) . chr($b2) . chr($b3);
                        }
                    }
                    $blocks[] = $block;
                }
            }
            break;
        case 'bmp':
            foreach (array_chunk(range(0, 0xFFFF), 256) as $chunk) {
                $blocks[] = array_map(static fn (int $c): string => pack('N', $c), $chunk);
            }
            break;
        case 'astral':
            $cps = range(0x10000, 0x2FFFF);
            for ($c = 0x30000; $c <= 0x10FFFF; $c += 31) {
                $cps[] = $c;
            }
            array_push($cps, ...range(0x110000, 0x1100FF));
            foreach (array_chunk($cps, 256) as $chunk) {
                $blocks[] = array_map(static fn (int $c): string => pack('N', $c), $chunk);
            }
            break;
        case 'translit':
        case 'ignore':
            $sep = $kind === 'translit' ? pack('N', 0x0A) : '';
            for ($start = 0; $start < 0x110000; $start += 4096) {
                $in = '';
                for ($c = $start; $c < $start + 4096; $c++) {
                    if ($c < 0xD800 || $c > 0xDFFF) {
                        $in .= pack('N', $c) . $sep;
                    }
                }
                $blocks[] = [$in];
            }
            break;
        default:
            throw new RuntimeException("unknown kind $kind");
    }
    return $blocks;
}

$sets = [];
foreach (MULTIBYTES as $cs) {
    $plan = [
        [$cs, 'UTF-8', 'bytes1'], [$cs, 'UTF-16LE//IGNORE', 'bytes1'], [$cs, 'ASCII//TRANSLIT', 'bytes1'],
        [$cs, 'UCS-4BE', 'bytes12'], [$cs, 'UCS-4BE//IGNORE', 'bytes12'],
        [$cs, 'UCS-4BE//IGNORE', 'follow'], [$cs, 'UTF-8//ignore', 'follow'],
        ['UCS-4BE', $cs, 'bmp'], ['UCS-4BE', "$cs//TRANSLIT", 'translit'], ['UCS-4BE', "$cs//IGNORE", 'ignore'],
    ];
    if ($cs === 'EUC-JP' || $cs === 'EUC-JP-MS') {
        array_push($plan, [$cs, 'UCS-4BE', 'bytes3'], [$cs, 'UCS-4BE//IGNORE', 'bytes3']);
    }
    if ($cs === 'EUC-TW') {
        array_push($plan, [$cs, 'UCS-4BE', 'bytes4'], [$cs, 'UCS-4BE//IGNORE', 'bytes4']);
    }
    if ($cs === 'GB18030') {
        array_push($plan, [$cs, 'UCS-4BE', 'gb4'], [$cs, 'UCS-4BE//IGNORE', 'gb4'], [$cs, 'UTF-8', 'gb4all']);
    }
    if (in_array($cs, ['BIG5-HKSCS', 'EUC-TW', 'GB18030'], true)) {
        $plan[] = ['UCS-4BE', $cs, 'astral'];
    }
    foreach ($plan as [$from, $to, $kind]) {
        $hashes = [];
        foreach (multibyte_inputs($kind) as $block) {
            $t = '';
            foreach ($block as $input) {
                $t .= token($from, $to, $input);
            }
            $hashes[] = hash('fnv1a64', $t);
        }
        $sets[] = ['from' => $from, 'to' => $to, 'kind' => $kind, 'blocks' => $hashes];
    }
}

echo '{"generator":"multibytes.gen.php","php":"', PHP_VERSION, "\",\n\"sets\":[\n";
foreach ($sets as $k => $s) {
    echo json_encode($s, JSON_UNESCAPED_SLASHES | JSON_THROW_ON_ERROR), $k + 1 < count($sets) ? ",\n" : "\n";
}
echo "]}\n";
