<?php
// Generates cjk_tables.rs and data/*.bin: the multi-byte mapping tables of glibc 2.41 (as used by PHP 8.5.13's
// iconv() in the oracle image), measured exhaustively through iconv() itself.
//
//   cd crates/twig-iconv/src/iconv
//   MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//       php /t/cjk_tables.gen.php > cjk_tables.rs
//
// Table files (little endian): a header [width (2 or 4), lead_lo, lead_hi, trail_lo, trail_hi, 0, 0, 0], one u16
// row index per lead byte (0xFFFF: no row), then the rows of trail_hi - trail_lo + 1 values (0: no character).
// * jis0208, jis0212, gb2312, ksc5601: the 94x94 coded character sets (rows and columns 0x21..0x7E) as the EUC-JP,
//   EUC-CN and EUC-KR decoders read them;
// * sjis, cp932, gbk, gb18030, uhc, big5, big5hkscs: the two-byte sequences of each charset (one character each);
// * jisx0213_1, jisx0213_2: the JIS X 0213 planes as EUC-JISX0213 reads them; values below 0x80 are 1-based
//   indexes into JISX0213_PAIRS (positions that decode to two characters);
// * gb18030_4: GB18030's four-byte sequences of the BMP as runs [index, code point, length] (u32 each), the index
//   being ((b1 - 0x81) * 10 + b2 - 0x30) * 1260 + (b3 - 0x81) * 10 + b4 - 0x30.
// Encoders: for each stateless charset the bytes the module writes for every code point 0..=0x10FFFF, recorded as
// deviations from the inverse of the decoder (for each code point the shortest, then lowest, byte sequence that
// decodes to exactly that character; GB18030's supplementary planes count as their linear four-byte form), with
// b"" for code points the encoder rejects. Unicode tag characters are either all dropped or all treated like any
// other code point (TAGS_DROPPED).
// Coded character sets inside ISO-2022 (SET_*): the encoder functions ucs4_to_jisx0208() and friends measured in
// context (ISO-2022-JP after a JIS X 0208 character, ISO-2022-JP-2 after a JIS X 0212 character or under the zh/ko
// language tags, ISO-2022-JP-3 in JIS X 0213 plane 1 and plane 2), as deviations from the inverse of the table.

declare(strict_types=0);
error_reporting(E_ALL & ~E_NOTICE & ~E_WARNING);
set_error_handler(static fn () => true);

$dir = __DIR__.'/data';
@mkdir($dir, 0777, true);

/** The code point of $seq when $cs decodes it to exactly one character, else null. */
function dec1(string $cs, string $seq): ?int
{
    $r = iconv($cs, 'UCS-4LE', $seq);
    if ($r === false || strlen($r) !== 4) {
        return null;
    }
    return unpack('V', $r)[1];
}

/** The code points $cs decodes $seq to (empty on failure). */
function dec(string $cs, string $seq): array
{
    $r = iconv($cs, 'UCS-4LE', $seq);
    if ($r === false || $r === '') {
        return [];
    }
    return array_values(unpack('V*', $r));
}

/** Writes a table file; $value(lead, trail) returns the value or null. Returns [lead][trail] => value. */
function table(string $file, int $width, int $leadLo, int $leadHi, int $trailLo, int $trailHi, callable $value): array
{
    global $dir;
    $index = '';
    $rows = '';
    $n = 0;
    $all = [];
    for ($l = $leadLo; $l <= $leadHi; $l++) {
        $row = '';
        $any = false;
        for ($t = $trailLo; $t <= $trailHi; $t++) {
            $v = $value($l, $t);
            if ($v !== null) {
                $any = true;
                $all[$l][$t] = $v;
            }
            $row .= pack($width === 2 ? 'v' : 'V', $v ?? 0);
            if ($width === 2 && ($v ?? 0) > 0xFFFF) {
                throw new \RuntimeException("$file: value beyond 16 bits");
            }
        }
        if ($any) {
            $index .= pack('v', $n++);
            $rows .= $row;
        } else {
            $index .= pack('v', 0xFFFF);
        }
    }
    file_put_contents("$dir/$file.bin", pack('C8', $width, $leadLo, $leadHi, $trailLo, $trailHi, 0, 0, 0).$index.$rows);
    return $all;
}

function rs_bytes(string $s): string
{
    $o = 'b"';
    foreach (str_split($s) as $c) {
        if ($c === '') {
            continue;
        }
        $o .= sprintf('\x%02X', ord($c));
    }
    return $o.'"';
}

function emit_pairs(string $name, string $doc, array $pairs): void
{
    echo "/// $doc\n";
    echo "pub(super) static $name: &[(u32, &[u8])] = &[\n";
    foreach ($pairs as [$cp, $bytes]) {
        printf("    (0x%04X, %s),\n", $cp, rs_bytes($bytes));
    }
    echo "];\n\n";
}

echo "//! Multi-byte mapping tables of glibc 2.41, measured in the oracle image by `cjk_tables.gen.php` (next to\n";
echo "//! this file; it also writes `data/*.bin`). Regenerate with the command in its header. Do not edit.\n\n";

// 1. The 94x94 coded character sets.
$square = static fn (string $file, callable $f) => table($file, 2, 0x21, 0x7E, 0x21, 0x7E, $f);
$sets = [
    'JIS0208' => $square('jis0208', static fn ($r, $c) => dec1('EUC-JP', chr($r | 0x80).chr($c | 0x80))),
    'JIS0212' => $square('jis0212', static fn ($r, $c) => dec1('EUC-JP', "\x8F".chr($r | 0x80).chr($c | 0x80))),
    'GB2312' => $square('gb2312', static fn ($r, $c) => dec1('EUC-CN', chr($r | 0x80).chr($c | 0x80))),
    'KSC5601' => $square('ksc5601', static fn ($r, $c) => dec1('EUC-KR', chr($r | 0x80).chr($c | 0x80))),
];

// 2. Two-byte tables of the stateless charsets.
$dbcs = [
    // charset => [file, width, lead_lo, lead_hi, trail_lo, trail_hi]
    'SJIS' => ['sjis', 2, 0x81, 0xEA, 0x40, 0xFF],
    'CP932' => ['cp932', 2, 0x81, 0xFC, 0x40, 0xFC],
    'GBK' => ['gbk', 2, 0x81, 0xFE, 0x40, 0xFF],
    'GB18030' => ['gb18030', 2, 0x81, 0xFE, 0x40, 0xFF],
    'UHC' => ['uhc', 2, 0x81, 0xFD, 0x41, 0xFE],
    'BIG5' => ['big5', 2, 0xA1, 0xF9, 0x40, 0xFE],
    'BIG5-HKSCS' => ['big5hkscs', 4, 0x87, 0xFE, 0x40, 0xFE],
    'EUC-JP-MS' => ['eucjpms', 2, 0xA1, 0xFE, 0xA1, 0xFE],
];
foreach ($dbcs as $cs => [$file, $width, $ll, $lh, $tl, $th]) {
    table($file, $width, $ll, $lh, $tl, $th, static fn ($l, $t) => dec1($cs, chr($l).chr($t)));
}
// EUC-JP-MS's three-byte sequences (0x8F and two bytes)
table('eucjpms_3', 2, 0xA1, 0xFE, 0xA1, 0xFE, static fn ($l, $t) => dec1('EUC-JP-MS', "\x8F".chr($l).chr($t)));

// 3. JIS X 0213 (combining positions decode to two characters).
$pairs = [];
$jisx0213 = static function (string $prefix) use (&$pairs) {
    return static function ($r, $c) use ($prefix, &$pairs) {
        $cps = dec('EUC-JISX0213', $prefix.chr($r | 0x80).chr($c | 0x80));
        if (count($cps) === 1) {
            if ($cps[0] < 0x80) {
                throw new \RuntimeException('JIS X 0213 position decodes below 0x80');
            }
            return $cps[0];
        }
        if (count($cps) === 2) {
            $k = array_search($cps, $pairs, true);
            if ($k === false) {
                $pairs[] = $cps;
                $k = count($pairs) - 1;
            }
            return $k + 1;
        }
        return null;
    };
};
$x0213_1 = table('jisx0213_1', 4, 0x21, 0x7E, 0x21, 0x7E, $jisx0213(''));
$x0213_2 = table('jisx0213_2', 4, 0x21, 0x7E, 0x21, 0x7E, $jisx0213("\x8F"));
echo "/// The character pairs of the JIS X 0213 positions that decode to a base and a combining character.\n";
echo 'pub(super) static JISX0213_PAIRS: [(u32, u32); ', count($pairs), "] = [\n";
foreach ($pairs as [$a, $b]) {
    printf("    (0x%04X, 0x%04X),\n", $a, $b);
}
echo "];\n\n";

// 4. GB18030 four-byte sequences in the BMP.
$gb4 = static function (int $idx): string {
    $b4 = $idx % 10;
    $idx = intdiv($idx, 10);
    $b3 = $idx % 126;
    $idx = intdiv($idx, 126);
    $b2 = $idx % 10;
    $b1 = intdiv($idx, 10);
    return chr($b1 + 0x81).chr($b2 + 0x30).chr($b3 + 0x81).chr($b4 + 0x30);
};
$runs = [];
$gb4map = [];
for ($idx = 0; $idx < 0x99FC; $idx++) {
    $cp = dec1('GB18030', $gb4($idx));
    if ($cp === null) {
        continue;
    }
    $gb4map[$idx] = $cp;
    $last = count($runs) - 1;
    if ($last >= 0 && $runs[$last][0] + $runs[$last][2] === $idx && $runs[$last][1] + $runs[$last][2] === $cp) {
        $runs[$last][2]++;
    } else {
        $runs[] = [$idx, $cp, 1];
    }
}
file_put_contents("$dir/gb18030_4.bin", implode('', array_map(static fn ($r) => pack('V3', ...$r), $runs)));
// the supplementary planes are linear
foreach ([0x10000, 0x10001, 0x1F600, 0x20000, 0xE0041, 0x10FFFF] as $cp) {
    if (dec1('GB18030', $gb4($cp + 0x1E248)) !== $cp) {
        throw new \RuntimeException(sprintf('GB18030 four-byte form of U+%X is not linear', $cp));
    }
}

foreach (['jis0208', 'jis0212', 'gb2312', 'ksc5601', 'sjis', 'cp932', 'gbk', 'gb18030', 'uhc', 'big5', 'big5hkscs',
    'eucjpms', 'eucjpms_3', 'jisx0213_1', 'jisx0213_2', 'gb18030_4'] as $file) {
    $ident = strtoupper($file);
    $size = filesize("$dir/$file.bin");
    echo "pub(super) static $ident: &[u8; $size] = include_bytes!(\"data/$file.bin\");\n";
}
echo "\n";

// 5. Encoders of the stateless charsets: deviations from the inverse of the decoder.
$encoders = [
    // charset => [Rust identifier, three-byte lead (EUC), four-byte (GB18030)]
    'EUC-JP' => ['EUC_JP', true, false],
    'SJIS' => ['SJIS', false, false],
    'CP932' => ['CP932', false, false],
    'EUC-CN' => ['EUC_CN', false, false],
    'GBK' => ['GBK', false, false],
    'GB18030' => ['GB18030', false, true],
    'EUC-KR' => ['EUC_KR', false, false],
    'UHC' => ['UHC', false, false],
    'BIG5' => ['BIG5', false, false],
    'BIG5-HKSCS' => ['BIG5HKSCS', false, false],
    'EUC-JP-MS' => ['EUC_JP_MS', true, false],
];
$tagsDropped = [];
foreach ($encoders as $cs => [$ident, $euc3, $four]) {
    $inverse = [];
    $consider = static function (string $seq) use ($cs, &$inverse) {
        $cp = dec1($cs, $seq);
        if ($cp !== null && !isset($inverse[$cp])) {
            $inverse[$cp] = $seq;
        }
    };
    for ($b = 0; $b < 256; $b++) {
        $consider(chr($b));
    }
    for ($l = 0x80; $l < 256; $l++) {
        for ($t = 0; $t < 256; $t++) {
            $consider(chr($l).chr($t));
        }
    }
    if ($euc3) {
        for ($r = 0xA1; $r <= 0xFE; $r++) {
            for ($c = 0xA1; $c <= 0xFE; $c++) {
                $consider("\x8F".chr($r).chr($c));
            }
        }
    }
    if ($four) {
        foreach ($gb4map as $idx => $cp) {
            $inverse[$cp] ??= $gb4($idx);
        }
    }
    $tags = [];
    for ($cp = 0xE0000; $cp <= 0xE007F; $cp++) {
        $r = iconv('UCS-4LE', $cs, pack('V', $cp));
        $tags[$r === '' ? 'dropped' : 'other'] = true;
    }
    if (count($tags) !== 1) {
        throw new \RuntimeException("$cs: tag characters are not handled uniformly");
    }
    $dropped = isset($tags['dropped']);
    $tagsDropped[$ident] = $dropped;
    $dev = [];
    for ($cp = 0; $cp <= 0x10FFFF; $cp++) {
        if ($dropped && ($cp >> 7) === (0xE0000 >> 7)) {
            continue;
        }
        $r = iconv('UCS-4LE', $cs, pack('V', $cp));
        $got = ($r === false) ? '' : $r;
        if ($r === '') {
            throw new \RuntimeException(sprintf('%s: U+%X writes nothing', $cs, $cp));
        }
        $want = $inverse[$cp] ?? (($four && $cp >= 0x10000) ? $gb4($cp + 0x1E248) : '');
        if ($got !== $want) {
            $dev[] = [$cp, $got];
        }
    }
    emit_pairs("ENC_$ident", "`$cs`: the encoder's deviations from the inverse of the decoder.", $dev);
}
echo "/// Whether the encoder drops Unicode tag characters (`UNICODE_TAG_HANDLER`) it cannot map.\n";
foreach ($tagsDropped as $ident => $dropped) {
    echo "pub(super) const TAGS_DROPPED_$ident: bool = ", $dropped ? 'true' : 'false', ";\n";
}
echo "\n";

// 6. The coded character sets of the ISO-2022 encoders, measured in context.
/** For each code point, the two bytes the set encoder writes after the prefix, or '' when it has none. */
function in_context(string $cs, array $prefixCps, string $prefixOut, array $cps): array
{
    $out = [];
    foreach ($cps as $cp) {
        $r = iconv('UCS-4LE', $cs, pack('V*', ...[...$prefixCps, $cp]));
        if ($r === false || strncmp($r, $prefixOut, strlen($prefixOut)) !== 0) {
            $out[$cp] = '';
            continue;
        }
        $rest = substr($r, strlen($prefixOut), 2);
        $out[$cp] = (strlen($rest) === 2 && $rest[0] !== "\x1B" && ord($rest[0]) >= 0x21 && ord($rest[0]) <= 0x7E
            && ord($rest[1]) >= 0x21 && ord($rest[1]) <= 0x7E) ? $rest : '';
    }
    return $out;
}

/** The inverse of a 94x94 table: code point => two bytes of the lowest position. */
function set_inverse(array $table): array
{
    $inv = [];
    foreach ($table as $r => $row) {
        foreach ($row as $c => $cp) {
            $inv[$cp] ??= chr($r).chr($c);
        }
    }
    return $inv;
}

$bmp = range(0, 0xFFFF);
$contexts = [
    'JIS0208' => ['ISO-2022-JP', [0x4E00], "\x1B\$B0l"],
    'JIS0212' => ['ISO-2022-JP-2', [0xE9], "\x1B\$(D+1"],
    'GB2312' => ['ISO-2022-JP-2', [0xE0001, 0xE007A, 0xE0068, 0x4E00], "\x1B\$AR;"],
    'KSC5601' => ['ISO-2022-JP-2', [0xE0001, 0xE006B, 0xE006F, 0xAC00], "\x1B\$(C0!"],
];
foreach ($contexts as $set => [$cs, $prefixCps, $prefixOut]) {
    $measured = in_context($cs, $prefixCps, $prefixOut, $bmp);
    $inv = set_inverse($sets[$set]);
    $dev = [];
    foreach ($measured as $cp => $bytes) {
        if ($bytes !== ($inv[$cp] ?? '')) {
            $dev[] = [$cp, $bytes];
        }
    }
    emit_pairs("SET_$set", "`ucs4_to_".strtolower($set)."()` as measured in $cs: deviations from the inverse of the table.", $dev);
}

// ISO 8859-7 upper half as a G2 set of ISO-2022-JP-2 (iso8859-7jp.h).
$g2 = [];
for ($b = 0x20; $b < 0x80; $b++) {
    $g2[$b] = dec1('ISO-2022-JP-2', "\x1B.F\x1BN".chr($b));
}
echo "/// The G2 set ISO 8859-7 of ISO-2022-JP-2 (`ESC . F`, used with `ESC N`): the code point of each byte\n";
echo "/// 0x20..0x7F, 0 for none.\n";
echo "pub(super) static G2_GREEK: [u16; 96] = [\n";
foreach (array_chunk($g2, 8) as $row) {
    echo '    ', implode(', ', array_map(static fn ($c) => sprintf('0x%04X', $c ?? 0), $row)), ",\n";
}
echo "];\n\n";
$greekProbe = null;
foreach (array_merge(range(0xA0, 0x3FF), range(0x2000, 0x20FF)) as $cp) {
    $r = iconv('UCS-4LE', 'ISO-2022-JP-2', pack('V', $cp));
    if ($r !== false && str_starts_with($r, "\x1B.F\x1BN")) {
        $greekProbe = $cp;
        break;
    }
}
if ($greekProbe === null) {
    throw new \RuntimeException('no character selects ISO 8859-7 in ISO-2022-JP-2');
}
$probeOut = substr(iconv('UCS-4LE', 'ISO-2022-JP-2', pack('V', $greekProbe)), 0, 6);
$g2inv = [];
foreach ($g2 as $b => $cp) {
    if ($cp !== null) {
        $g2inv[$cp] ??= $b;
    }
}
$dev = [];
foreach ($bmp as $cp) {
    if ($cp < 0x80) {
        continue;
    }
    $r = iconv('UCS-4LE', 'ISO-2022-JP-2', pack('V2', $greekProbe, $cp));
    $got = null;
    if ($r !== false && strncmp($r, $probeOut, 6) === 0 && substr($r, 6, 2) === "\x1BN") {
        $got = ord($r[8]);
    }
    if ($got !== ($g2inv[$cp] ?? null)) {
        $dev[] = [$cp, $got === null ? '' : chr($got)];
    }
}
emit_pairs('G2_GREEK_ENC', 'The ISO 8859-7 G2 encoder of ISO-2022-JP-2: deviations from the inverse of [`G2_GREEK`].', $dev);

// JIS X 0213 in ISO-2022-JP-3: plane 1 (after a character added in 2004) and plane 2.
$probe1 = $x0213_1[0x2E][0x21];
$probe2 = $x0213_2[0x21][0x21];
$plane1 = in_context('ISO-2022-JP-3', [$probe1], "\x1B\$(Q.!", range(0, 0x2FFFF));
$plane2 = in_context('ISO-2022-JP-3', [$probe2], "\x1B\$(P!!", range(0, 0x2FFFF));
$inv = [];
foreach ([[$x0213_1, 0], [$x0213_2, 0x80]] as [$table, $hi]) {
    foreach ($table as $r => $row) {
        foreach ($row as $c => $cp) {
            if ($cp >= 0x80) {
                $inv[$cp] ??= chr($r | $hi).chr($c);
            }
        }
    }
}
$dev = [];
for ($cp = 0; $cp <= 0x2FFFF; $cp++) {
    $got = $plane1[$cp] !== ''
        ? $plane1[$cp]
        : ($plane2[$cp] !== '' ? chr(ord($plane2[$cp][0]) | 0x80).$plane2[$cp][1] : '');
    if ($got !== ($inv[$cp] ?? '')) {
        $dev[] = [$cp, $got];
    }
}
emit_pairs('SET_JISX0213', '`ucs4_to_jisx0213()` as measured in ISO-2022-JP-3 (plane 2 rows with bit 7 set): '
    .'deviations from the inverse of the tables.', $dev);

// 7. The composing single-byte charsets: decoding tables (characters the decoder buffers are flushed), the
// compositions of a buffered character with a following combining character (measured on every pair of bytes,
// and on the composed character with a third byte), and the encoder (direct bytes and decompositions) as
// deviations from the inverse of the decoding table.
foreach (['CP1255' => ['CP1255', [0x05D0, 0x05F2], [0x05B0, 0x05C4]], 'CP1258' => ['CP1258', [0x41, 0x1B0], [0x300, 0x33F]],
    'TCVN5712-1' => ['TCVN', [0x41, 0x1B0], [0x300, 0x33F]]] as $cs => [$ident, $buffered, $combining]) {
    $dec = [];
    for ($b = 0; $b < 256; $b++) {
        $dec[$b] = dec1($cs, chr($b));
    }
    $isBuffered = static fn ($c) => $c !== null && $c >= $buffered[0] && $c <= $buffered[1];
    $isCombining = static fn ($c) => $c !== null && $c >= $combining[0] && $c <= $combining[1];
    $comp = [];
    for ($b1 = 0; $b1 < 256; $b1++) {
        if (!$isBuffered($dec[$b1])) {
            continue;
        }
        for ($b2 = 0; $b2 < 256; $b2++) {
            if (!$isCombining($dec[$b2])) {
                continue;
            }
            $out = dec($cs, chr($b1).chr($b2));
            if (count($out) !== 1) {
                continue;
            }
            $comp[$dec[$b1].','.$dec[$b2]] = [$dec[$b1], $dec[$b2], $out[0]];
            for ($b3 = 0; $b3 < 256; $b3++) {
                if (!$isCombining($dec[$b3])) {
                    continue;
                }
                $out3 = dec($cs, chr($b1).chr($b2).chr($b3));
                if (count($out3) === 1) {
                    $comp[$out[0].','.$dec[$b3]] = [$out[0], $dec[$b3], $out3[0]];
                }
            }
        }
    }
    ksort($comp, SORT_NATURAL);
    $comp = array_values($comp);
    usort($comp, static fn ($a, $b) => [$a[0], $a[1]] <=> [$b[0], $b[1]]);
    echo "/// `$cs`: the code point of every byte (0xFFFF: invalid).\n";
    echo "pub(super) static {$ident}_DECODE: [u16; 256] = [\n";
    foreach (array_chunk($dec, 8) as $row) {
        echo '    ', implode(', ', array_map(static fn ($c) => sprintf('0x%04X', $c ?? 0xFFFF), $row)), ",\n";
    }
    echo "];\n\n";
    echo "/// `$cs`: the compositions `(buffered, combining, composed)` of the decoder, sorted.\n";
    echo "pub(super) static {$ident}_COMPOSE: &[(u32, u32, u32)] = &[\n";
    foreach ($comp as [$a, $b, $c]) {
        printf("    (0x%04X, 0x%04X, 0x%04X),\n", $a, $b, $c);
    }
    echo "];\n\n";
    $inverse = [];
    for ($b = 255; $b >= 0; $b--) {
        if ($dec[$b] !== null) {
            $inverse[$dec[$b]] = chr($b);
        }
    }
    $tags = [];
    for ($cp = 0xE0000; $cp <= 0xE007F; $cp++) {
        $tags[iconv('UCS-4LE', $cs, pack('V', $cp)) === '' ? 'dropped' : 'other'] = true;
    }
    if (array_keys($tags) !== ['dropped']) {
        throw new \RuntimeException("$cs: tag characters are not dropped");
    }
    $dev = [];
    for ($cp = 0; $cp <= 0x10FFFF; $cp++) {
        if (($cp >> 7) === (0xE0000 >> 7)) {
            continue;
        }
        $r = iconv('UCS-4LE', $cs, pack('V', $cp));
        $got = $r === false ? '' : $r;
        if ($got !== ($inverse[$cp] ?? '')) {
            $dev[] = [$cp, $got];
        }
    }
    emit_pairs("ENC_$ident", "`$cs`: the encoder's deviations (decompositions) from the inverse of the decoding table.", $dev);
}

// 8. The direct modules of the gconv cache's extra conversions: GB2312 <-> BIG5 (gbbig5.c) and IBM1008 <-> IBM420
// (ibm1008_420.c). Values are the two output bytes (gbbig5 maps unmapped positions to 0).
$direct = static function (string $from, string $to) {
    return static function ($l, $t) use ($from, $to) {
        $r = iconv($from, $to, chr($l).chr($t));
        if ($r === false) {
            return null;
        }
        if (strlen($r) !== 2) {
            throw new \RuntimeException("$from -> $to: one-byte output");
        }
        return (ord($r[0]) << 8) | ord($r[1]);
    };
};
table('gb2312_big5', 2, 0xA1, 0xF7, 0xA1, 0xFF, $direct('EUC-CN', 'BIG5'));
table('big5_gb2312', 2, 0xA1, 0xF9, 0x40, 0xFE, static function ($l, $t) use ($direct) {
    return ($t <= 0x7E || $t >= 0xA1) ? $direct('BIG5', 'EUC-CN')($l, $t) : null;
});
foreach (['gb2312_big5', 'big5_gb2312'] as $file) {
    $ident = strtoupper($file);
    $size = filesize("$dir/$file.bin");
    echo "pub(super) static $ident: &[u8; $size] = include_bytes!(\"data/$file.bin\");\n";
}
foreach (['IBM1008' => 'IBM420', 'IBM420' => 'IBM1008'] as $from => $to) {
    $map = iconv($from, $to, implode('', array_map('chr', range(0, 255))));
    echo "\n/// The direct `$from` to `$to` module: the output byte of every byte.\n";
    echo "pub(super) static {$from}_{$to}: [u8; 256] = [\n";
    foreach (array_chunk(str_split($map), 16) as $row) {
        echo '    ', implode(', ', array_map(static fn ($c) => sprintf('0x%02X', ord($c)), $row)), ",\n";
    }
    echo "];\n";
}
