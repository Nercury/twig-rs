<?php
// sweep.json: exhaustive and sampled short inputs, hashed per block of results: every 1- and 2-byte input of the
// decoders, 3-byte UTF-8 inputs, 16-bit surrogate pairs, 32-bit values (all surrogates, the neighbourhoods of every
// boundary, xorshift samples), and those values encoded into every target. A sweep is
// {"from", "to", "kind", "blocks": [fnv1a64 of the concatenated tokens of each block]} (token() in common.inc.php);
// tests/charset_basic.rs builds the same inputs and tokens.

require __DIR__ . '/common.inc.php';

/** xorshift32 */
function xorshift(int &$x): int
{
    $x ^= ($x << 13) & 0xFFFFFFFF;
    $x ^= $x >> 17;
    $x ^= ($x << 5) & 0xFFFFFFFF;
    return $x;
}

/** The 32-bit values of the "values" kinds: boundaries and their neighbourhoods, all surrogates, then samples. */
function values(): array
{
    $v = [];
    foreach ([[0, 0x100], [0xD7F0, 0xE010], [0xFFF0, 0x10010], [0x10FFF0, 0x110010], [0x1FFFF0, 0x200010],
        [0x3FFFFF0, 0x4000010], [0x7FFFFFF0, 0x80000010], [0xFFFFFFF0, 0xFFFFFFFF]] as [$lo, $hi]) {
        for ($c = $lo; $c <= $hi; $c++) {
            $v[] = $c;
        }
    }
    $x = 2463534242;
    for ($i = 0; $i < 6000; $i++) {
        $r = xorshift($x);
        $v[] = match ($i % 4) {
            0, 1 => $r % 0x110000,
            2 => $r & 0x7FFFFFFF,
            default => $r,
        };
    }
    return $v;
}

/** @return list<list<string>> the inputs of a sweep kind, in blocks */
function inputs(string $kind): array
{
    $blocks = [];
    switch ($kind) {
        case 'bytes12':
            $block = [];
            for ($b = 0; $b < 256; $b++) {
                $block[] = chr($b);
            }
            $blocks[] = $block;
            for ($b0 = 0; $b0 < 256; $b0++) {
                $block = [];
                for ($b1 = 0; $b1 < 256; $b1++) {
                    $block[] = chr($b0) . chr($b1);
                }
                $blocks[] = $block;
            }
            break;
        case 'utf8x3':
            foreach (range(0xC0, 0xFF) as $b0) {
                $block = [];
                for ($b1 = 0; $b1 < 256; $b1++) {
                    foreach ([0x00, 0x41, 0x7F, 0x80, 0x8F, 0x90, 0x9F, 0xA0, 0xBF, 0xC0, 0xC3, 0xE0, 0xF0, 0xFF] as $b2) {
                        $block[] = chr($b0) . chr($b1) . chr($b2);
                    }
                }
                $blocks[] = $block;
            }
            break;
        case 'pairs16be':
        case 'pairs16le':
            $format = $kind === 'pairs16be' ? 'n' : 'v';
            for ($hi = 0xD8; $hi <= 0xDF; $hi++) {
                $block = [];
                for ($lo = 0; $lo < 256; $lo++) {
                    foreach ([0x0000, 0x0061, 0xD7FF, 0xD800, 0xDBFF, 0xDC00, 0xDFFF, 0xE000, 0xFEFF, 0xFFFE, 0xFFFF] as $u2) {
                        $block[] = pack($format, ($hi << 8) | $lo) . pack($format, $u2);
                    }
                }
                $blocks[] = $block;
            }
            break;
        case 'values32be':
        case 'values32le':
            $format = $kind === 'values32be' ? 'N' : 'V';
            foreach (array_chunk(values(), 256) as $chunk) {
                $blocks[] = array_map(static fn (int $c): string => pack($format, $c), $chunk);
            }
            break;
        default:
            throw new RuntimeException("unknown kind $kind");
    }
    return $blocks;
}

$sweeps = [];
$sweep = static function (string $from, string $to, string $kind) use (&$sweeps): void {
    $hashes = [];
    foreach (inputs($kind) as $block) {
        $t = '';
        foreach ($block as $input) {
            $t .= token($from, $to, $input);
        }
        $hashes[] = hash('fnv1a64', $t);
    }
    $sweeps[] = ['from' => $from, 'to' => $to, 'kind' => $kind, 'blocks' => $hashes];
};

foreach (['UCS-4', 'UTF-16LE//IGNORE', 'ASCII//TRANSLIT//IGNORE', 'ISO-8859-1//TRANSLIT', 'UTF-8//ignore'] as $to) {
    $sweep('UTF-8', $to, 'bytes12');
}
$sweep('UTF-8', 'UCS-4', 'utf8x3');
$sweep('UTF-8', 'UTF-16BE//IGNORE', 'utf8x3');
foreach (['UTF-16', 'UTF-16BE', 'UTF-16LE', 'UCS-2', 'UCS-2BE', 'UCS-2LE', 'UTF-32', 'UCS-4LE', 'ASCII', 'ISO-8859-1'] as $from) {
    $sweep($from, 'UTF-8', 'bytes12');
    $sweep($from, 'UCS-4//IGNORE', 'bytes12');
}
foreach (['UTF-16', 'UTF-16BE', 'UTF-16LE', 'UCS-2', 'UCS-2BE'] as $from) {
    foreach (['pairs16be', 'pairs16le'] as $kind) {
        $sweep($from, 'UTF-8', $kind);
        $sweep($from, 'UTF-32BE//IGNORE', $kind);
    }
}
foreach (['UTF-32', 'UTF-32BE', 'UTF-32LE', 'UCS-4', 'UCS-4BE', 'UCS-4LE'] as $from) {
    foreach (['values32be', 'values32le'] as $kind) {
        $sweep($from, 'UTF-8', $kind);
        $sweep($from, 'UTF-16LE//IGNORE', $kind);
    }
}
foreach (CHARSETS as $to) {
    foreach (['', '//TRANSLIT', '//IGNORE'] as $suffix) {
        $sweep('UCS-4', $to . $suffix, 'values32be');
    }
}

echo '{"generator":"sweep.gen.php","php":"', PHP_VERSION, "\",\n\"sweeps\":[\n";
foreach ($sweeps as $k => $s) {
    echo json_encode($s, JSON_UNESCAPED_SLASHES | JSON_THROW_ON_ERROR), $k + 1 < count($sweeps) ? ",\n" : "\n";
}
echo "]}\n";
