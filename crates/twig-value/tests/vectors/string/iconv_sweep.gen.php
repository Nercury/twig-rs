<?php
// Generates iconv_sweep.json: exhaustive sweeps of iconv() on glibc 2.41 over the multi-byte, stateful and composing
// charsets, recorded as FNV-1a 64 hashes so that the file stays small.
//
// Section "bytes": each row converts prefix . chr(x) . chr(y) . suffix for every x in [x0, x1] and y in [y0, y1]
// (in that order) from "from" to "to"; section "chars": each row converts the UCS-4LE string ctx . pack('V', cp)
// for every cp in [c0, c1] to "to". The hash covers, per conversion, the result ("S" and hex, or "F") followed by
// "|" plus level and hex message per diagnostic, conversions separated by "\n".
//
//   cd crates/twig-value/tests/vectors
//   MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//       php /t/string/iconv_sweep.gen.php > string/iconv_sweep.json

declare(strict_types=0);
require __DIR__.'/../common.inc.php';

function token($r, array $d): string
{
    $t = is_string($r) ? 'S'.bin2hex($r) : 'F';
    foreach ($d as [$level, $msg]) {
        $t .= '|'.$level.$msg;
    }
    return $t;
}

$bytes = [];
$byteRow = static function (string $from, string $to, string $prefix, int $x0, int $x1, int $y0, int $y1,
    string $suffix) use (&$bytes) {
    $h = hash_init('fnv1a64');
    $first = true;
    for ($x = $x0; $x <= $x1; $x++) {
        for ($y = $y0; $y <= $y1; $y++) {
            [$r, $d] = capture(static fn () => iconv($from, $to, $prefix.chr($x).chr($y).$suffix));
            hash_update($h, ($first ? '' : "\n").token($r, $d));
            $first = false;
        }
    }
    $bytes[] = ['from' => $from, 'to' => $to, 'prefix' => bin2hex($prefix), 'x' => [$x0, $x1], 'y' => [$y0, $y1],
        'suffix' => bin2hex($suffix), 'fnv' => hash_final($h)];
};
$chars = [];
$charRow = static function (string $to, array $ctx, int $c0, int $c1) use (&$chars) {
    $h = hash_init('fnv1a64');
    $prefix = $ctx ? pack('V*', ...$ctx) : '';
    for ($cp = $c0; $cp <= $c1; $cp++) {
        [$r, $d] = capture(static fn () => iconv('UCS-4LE', $to, $prefix.pack('V', $cp)));
        hash_update($h, ($cp === $c0 ? '' : "\n").token($r, $d));
    }
    $chars[] = ['to' => $to, 'ctx' => $ctx, 'c' => [$c0, $c1], 'fnv' => hash_final($h)];
};

// 1. every two-byte sequence of the stateless multi-byte charsets, per first byte
$mb = ['EUC-JP', 'EUC-JP-MS', 'SJIS', 'CP932', 'EUC-CN', 'GBK', 'GB18030', 'EUC-KR', 'UHC', 'BIG5', 'BIG5-HKSCS',
    'EUC-JISX0213', 'SHIFT_JISX0213'];
foreach ($mb as $cs) {
    for ($x = 0; $x < 256; $x++) {
        $byteRow($cs, 'UTF-8', '', $x, $x, 0, 255, '');
        $byteRow($cs, 'UTF-8//IGNORE', '', $x, $x, 0, 255, 'AB');
    }
}
// 2. three-byte EUC sequences
foreach (['EUC-JP', 'EUC-JP-MS', 'EUC-JISX0213'] as $cs) {
    for ($x = 0; $x < 256; $x++) {
        $byteRow($cs, 'UTF-8', "\x8F", $x, $x, 0, 255, '');
        $byteRow($cs, 'UTF-8//IGNORE', "\x8F", $x, $x, 0, 255, 'AB');
    }
}
// 3. GB18030 four-byte sequences
foreach ([0x81, 0x82, 0x83, 0x84, 0x85, 0x8F, 0x90, 0x95, 0xE3, 0xE4, 0xFD, 0xFE] as $b1) {
    for ($b2 = 0x30; $b2 <= 0x39; $b2++) {
        $byteRow('GB18030', 'UTF-8', chr($b1).chr($b2), 0x80, 0xFF, 0x2F, 0x3A, '');
        $byteRow('GB18030', 'UTF-8//IGNORE', chr($b1).chr($b2), 0x80, 0xFF, 0x2F, 0x3A, 'AB');
    }
}
// 4. the ISO-2022 decoders in each designation
$designations = [
    'ISO-2022-JP' => ['', "\x1b(J", "\x1b\$@", "\x1b\$B"],
    'ISO-2022-JP-2' => ['', "\x1b(J", "\x1b(I", "\x1b\$B", "\x1b\$A", "\x1b\$(C", "\x1b\$(D", "\x1b.A\x1bN", "\x1b.F\x1bN"],
    'ISO-2022-JP-3' => ['', "\x1b(J", "\x1b(I", "\x1b\$B", "\x1b\$(O", "\x1b\$(Q", "\x1b\$(P"],
    'ISO-2022-KR' => ["\x0e", "\x1b\$)C\x0e"],
];
foreach ($designations as $cs => $prefixes) {
    foreach ($prefixes as $prefix) {
        $byteRow($cs, 'UTF-8', $prefix, 0, 0x80, 0, 0x80, '');
        $byteRow($cs, 'UTF-8//IGNORE', $prefix, 0, 0x80, 0, 0x80, 'AB');
    }
}
// 4b. the direct modules
foreach ([['EUC-CN', 'BIG5'], ['BIG5', 'EUC-CN'], ['GBK', 'EUC-CN'], ['EUC-CN', 'GBK']] as [$from, $to]) {
    for ($x = 0x80; $x < 256; $x++) {
        $byteRow($from, $to, '', $x, $x, 0, 255, '');
        $byteRow($from, $to.'//IGNORE', '', $x, $x, 0, 255, 'AB');
    }
}
// 5. byte pairs of the composing charsets
foreach (['CP1255', 'CP1258', 'TCVN5712-1'] as $cs) {
    for ($x = 0; $x < 256; $x++) {
        $byteRow($cs, 'UTF-8', '', $x, $x, 0, 255, '');
        $byteRow($cs, 'UTF-16LE//IGNORE', '', $x, $x, 0, 255, "\xCC");
    }
}
// 6. every character alone, and the ISO-2022 encoders after characters that select each set
$encoders = array_merge($mb, ['ISO-2022-JP', 'ISO-2022-JP-2', 'ISO-2022-JP-3', 'ISO-2022-KR', 'CP1255', 'CP1258',
    'TCVN5712-1']);
foreach ($encoders as $cs) {
    $top = in_array($cs, ['GB18030', 'BIG5-HKSCS', 'EUC-JISX0213', 'SHIFT_JISX0213', 'ISO-2022-JP-3'], true) ? 0x2FFFF : 0xFFFF;
    for ($c = 0; $c <= $top; $c += 0x1000) {
        $charRow($cs, [], $c, $c + 0xFFF);
    }
}
$contexts = [
    'ISO-2022-JP' => [[0x65E5], [0xA5]],
    'ISO-2022-JP-2' => [[0xE9], [0xE0001, 0xE007A, 0xE0068, 0x4E00], [0xE0001, 0xE006B, 0xE006F, 0xAC00], [0x37A], [0xFF76]],
    'ISO-2022-JP-3' => [[0x4FF1], [0x20089], [0x304B], [0xFF76], [0xA5]],
    'ISO-2022-KR' => [[0xD55C]],
];
foreach ($contexts as $cs => $ctxs) {
    foreach ($ctxs as $ctx) {
        for ($c = 0; $c <= 0xFFFF; $c += 0x1000) {
            $charRow($cs, $ctx, $c, $c + 0xFFF);
        }
    }
}

emit('iconv_sweep.gen.php', ['bytes' => $bytes, 'chars' => $chars]);
