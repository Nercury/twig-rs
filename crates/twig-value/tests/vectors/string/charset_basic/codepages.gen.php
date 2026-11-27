<?php
// codepages.json: every byte and every code point of each codepage (the single-byte charsets with tables), hashed per
// block of results like sweep.json (token() in common.inc.php):
//   bytes1    every byte on its own, decoded (into UTF-8, UTF-16LE//IGNORE, ASCII//TRANSLIT);
//   strings   all bytes in a row (up, down, interleaved), decoded with //IGNORE;
//   bytes12   every one- and two-byte input (CP1255 and CP1258, whose decoders combine characters);
//   bmp       every BMP code point on its own, encoded from UCS-4BE (one call each, no flags);
//   translit  every code point U+0000..U+10FFFF (surrogates aside), 4096 per call, each followed by a line feed,
//             encoded with //TRANSLIT;
//   ignore    the same code points, 4096 per call, without line feeds, encoded with //IGNORE;
//   translit_ignore
//             every code point (surrogates aside), 2048 per call (one round), each followed by a line feed, encoded
//             with //TRANSLIT//IGNORE: for the codepages without "?", where //TRANSLIT fails at a character none of
//             whose alternatives is representable.
// A set is {"from", "to", "kind", "blocks": [fnv1a64 of the concatenated tokens of each block]}; tests/charset_basic.rs
// builds the same inputs and tokens.

require __DIR__ . '/common.inc.php';

/** @return list<list<string>> the inputs of a kind, in blocks */
function codepage_inputs(string $kind): array
{
    $blocks = [];
    switch ($kind) {
        case 'bytes1':
            $blocks[] = array_map('chr', range(0, 255));
            break;
        case 'strings':
            $up = implode('', array_map('chr', range(0, 255)));
            $interleaved = '';
            for ($b = 0x80; $b < 0x100; $b++) {
                $interleaved .= chr($b) . chr($b ^ 0xC0) . 'a';
            }
            $blocks[] = [$up, strrev($up), $interleaved];
            break;
        case 'bytes12':
            $blocks[] = array_map('chr', range(0, 255));
            for ($b0 = 0; $b0 < 256; $b0++) {
                $block = [];
                for ($b1 = 0; $b1 < 256; $b1++) {
                    $block[] = chr($b0) . chr($b1);
                }
                $blocks[] = $block;
            }
            break;
        case 'bmp':
            foreach (array_chunk(range(0, 0xFFFF), 256) as $chunk) {
                $blocks[] = array_map(static fn (int $c): string => pack('N', $c), $chunk);
            }
            break;
        case 'translit':
        case 'ignore':
        case 'translit_ignore':
            $sep = $kind === 'ignore' ? '' : pack('N', 0x0A);
            $per = $kind === 'translit_ignore' ? 2048 : 4096;
            for ($start = 0; $start < 0x110000; $start += $per) {
                $in = '';
                for ($c = $start; $c < $start + $per; $c++) {
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
foreach (CODEPAGES as $cs) {
    $plan = [
        [$cs, 'UTF-8', 'bytes1'], [$cs, 'UTF-16LE//IGNORE', 'bytes1'], [$cs, 'ASCII//TRANSLIT', 'bytes1'],
        [$cs, 'UCS-4BE//IGNORE', 'strings'], [$cs, 'UTF-8//IGNORE', 'strings'],
        ['UCS-4BE', $cs, 'bmp'], ['UCS-4BE', "$cs//TRANSLIT", 'translit'], ['UCS-4BE', "$cs//IGNORE", 'ignore'],
    ];
    if (in_array($cs, COMPOSING, true)) {
        array_push($plan, [$cs, 'UTF-8', 'bytes12'], [$cs, 'UCS-4BE//IGNORE', 'bytes12'],
            [$cs, 'ISO-8859-1//TRANSLIT', 'bytes12']);
    }
    foreach ($plan as [$from, $to, $kind]) {
        $hashes = [];
        foreach (codepage_inputs($kind) as $block) {
            $t = '';
            foreach ($block as $input) {
                $t .= token($from, $to, $input);
            }
            $hashes[] = hash('fnv1a64', $t);
        }
        $sets[] = ['from' => $from, 'to' => $to, 'kind' => $kind, 'blocks' => $hashes];
    }
}

// the codepages without "?"
foreach (CODEPAGES as $cs) {
    if (iconv('UCS-4BE', $cs, pack('N', 0x3F)) !== false) {
        continue;
    }
    $hashes = [];
    foreach (codepage_inputs('translit_ignore') as $block) {
        $t = '';
        foreach ($block as $input) {
            $t .= token('UCS-4BE', "$cs//TRANSLIT//IGNORE", $input);
        }
        $hashes[] = hash('fnv1a64', $t);
    }
    $sets[] = ['from' => 'UCS-4BE', 'to' => "$cs//TRANSLIT//IGNORE", 'kind' => 'translit_ignore', 'blocks' => $hashes];
}

echo '{"generator":"codepages.gen.php","php":"', PHP_VERSION, "\",\n\"sets\":[\n";
foreach ($sets as $k => $s) {
    echo json_encode($s, JSON_UNESCAPED_SLASHES | JSON_THROW_ON_ERROR), $k + 1 < count($sets) ? ",\n" : "\n";
}
echo "]}\n";
