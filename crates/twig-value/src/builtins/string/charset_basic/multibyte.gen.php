<?php
// Generates multibyte_table.rs: the multi-byte charsets of multibyte.rs as glibc 2.41's iconv converts them in the
// oracle, and how they compare with the published mapping files.
//
// Black-box only. multibyte_probe.c (compiled in the oracle image, public iconv_open()/iconv() calls only) decodes
// every byte sequence each charset's structure allows (every byte, every lead and trail byte pair, the three- and
// four-byte spaces) and encodes every code point U+0000..U+10FFFF; invalid sequences also tell how many bytes
// //IGNORE skips. This generator turns that into the tables and checks them:
//
// - the coded character sets several charsets share (JIS X 0208 and JIS X 0212 from EUC-JP, GB 2312 from EUC-CN,
//   KS X 1001 from EUC-KR, Big5 from BIG5, the CNS 11643 planes from EUC-TW) are stored once, compressed (see
//   `Stream` in multibyte.rs), and every charset's grid of two-byte cells is assembled from them, from streams of
//   its own for the cells they lack, and from patches, exactly as multibyte.rs assembles it at run time; the
//   assembled grids must equal the measurements cell for cell;
// - the encoders are the inverse of the decoders (the first or the last of several sequences that decode to the
//   same character, whichever needs fewer exceptions) plus the exceptions measured; every code point must come out
//   as measured;
// - how many bytes an invalid sequence makes //IGNORE skip (one or the whole pair, as a set of trail byte ranges
//   per lead byte), and the few irregular cases (written as checks below);
// - PHP's iconv() must agree with the probe on samples of every kind of sequence.
//
// The comparison with the published mapping files (fetched from https://www.unicode.org/Public/MAPPINGS/ and
// https://encoding.spec.whatwg.org/) is written into the documentation of each charset; the tables are what the
// oracle shows.
//
// Regenerate (from crates/twig-value/src/builtins/string/charset_basic, Git Bash; needs network access; a few
// minutes):
//   MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//       php /t/multibyte.gen.php > multibyte_table.rs

declare(strict_types=1);
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

function note(string $message): void
{
    fwrite(STDERR, $message . "\n");
}

function u(int $cp): string
{
    return sprintf('U+%04X', $cp);
}

// ---------------------------------------------------------------------------------------------------------------
// Measurement

/** Runs multibyte_probe.c: the lines of `dec`/`enc` for a charset (cached for the run). */
function probe(string ...$args): array
{
    static $binary = null;
    if ($binary === null) {
        $binary = sys_get_temp_dir() . '/multibyte_probe';
        exec('gcc -O2 -o ' . escapeshellarg($binary) . ' ' . escapeshellarg(__DIR__ . '/multibyte_probe.c'), $o, $status);
        if ($status !== 0) {
            fail('cannot compile multibyte_probe.c');
        }
    }
    $out = shell_exec(escapeshellarg($binary) . ' ' . implode(' ', array_map('escapeshellarg', $args)));
    if (!is_string($out) || $out === '') {
        fail('multibyte_probe ' . implode(' ', $args) . ' failed');
    }
    return explode("\n", rtrim($out, "\n"));
}

/**
 * What every byte sequence of $cs decodes to (after $prefix): hex => ['C', [cps]] | ['I'] | ['X', k1, k2, out].
 */
function measure_decode(string $cs, string $prefix = '-', bool $sample = false): array
{
    $seq = [];
    foreach (probe('dec', $cs, $prefix, $sample ? '1' : '0') as $line) {
        $p = explode(' ', $line);
        $seq[$p[0]] = match ($p[1]) {
            '=' => ['C', ($p[2] ?? '') === '' ? [] : array_map('hexdec', explode(',', $p[2]))],
            'I' => ['I'],
            'X' => ['X', ...array_map('intval', explode('-', $p[2])), $p[3]],
            default => fail("$cs: probe line $line"),
        };
    }
    return $seq;
}

/** What every code point encodes to in $cs: cp => bytes. */
function measure_encode(string $cs): array
{
    $enc = [];
    foreach (probe('enc', $cs) as $line) {
        $p = explode(' ', $line);
        $enc[hexdec($p[0])] = hex2bin($p[1] ?? '');
    }
    return $enc;
}

// ---------------------------------------------------------------------------------------------------------------
// The compressed streams (the format `Stream` in multibyte.rs reads)
//
// A stream lists cells, each a code point or empty; `prev` starts at 0 and is the last code point listed:
//   0x00..=0x3F          one cell: prev + 1 + t
//   0x40..=0x7F          a run of t - 0x40 + 2 cells: prev + 1, prev + 2, ...
//   0x80..=0x9F          t - 0x80 + 1 empty cells
//   0xA0..=0xDF b        one cell: prev + d, d the 14-bit two's complement ((t - 0xA0) << 8 | b)
//   0xE0..=0xFF b1 b2    one cell: (t - 0xE0) << 16 | b1 << 8 | b2
// Cells after the end of a stream are empty.

/** @param list<int|null> $cells */
function compress(array $cells): string
{
    while ($cells !== [] && end($cells) === null) {
        array_pop($cells);
    }
    $out = '';
    $prev = 0;
    $n = count($cells);
    for ($i = 0; $i < $n;) {
        $c = $cells[$i];
        if ($c === null) {
            $j = $i;
            while ($j < $n && $cells[$j] === null && $j - $i < 32) {
                $j++;
            }
            $out .= chr(0x80 + $j - $i - 1);
            $i = $j;
            continue;
        }
        $j = $i;
        while ($j < $n && $cells[$j] === $prev + 1 + $j - $i && $j - $i < 65) {
            $j++;
        }
        if ($j - $i >= 2) {
            $out .= chr(0x40 + $j - $i - 2);
            $prev = $cells[$j - 1];
            $i = $j;
            continue;
        }
        $d = $c - $prev;
        if ($d >= 1 && $d <= 64) {
            $out .= chr($d - 1);
        } elseif ($d >= -8192 && $d < 8192) {
            $v = $d & 0x3FFF;
            $out .= chr(0xA0 + ($v >> 8)) . chr($v & 0xFF);
        } elseif ($c <= 0x1FFFFF) {
            $out .= chr(0xE0 | ($c >> 16)) . chr(($c >> 8) & 0xFF) . chr($c & 0xFF);
        } else {
            fail('a code point beyond the stream format');
        }
        $prev = $c;
        $i++;
    }
    if (decompress($out, $n) !== $cells) {
        fail('a stream does not decompress to its cells');
    }
    return $out;
}

/** The cells of a stream (the inverse of compress(), as multibyte.rs reads it); $count cells. */
function decompress(string $s, int $count): array
{
    $cells = [];
    $prev = 0;
    $len = strlen($s);
    for ($i = 0; $i < $len;) {
        $t = ord($s[$i++]);
        if ($t < 0x40) {
            $prev += 1 + $t;
            $cells[] = $prev;
        } elseif ($t < 0x80) {
            for ($k = 0; $k < $t - 0x40 + 2; $k++) {
                $cells[] = ++$prev;
            }
        } elseif ($t < 0xA0) {
            for ($k = 0; $k < $t - 0x80 + 1; $k++) {
                $cells[] = null;
            }
        } elseif ($t < 0xE0) {
            $v = (($t - 0xA0) << 8) | ord($s[$i++]);
            $prev += $v >= 0x2000 ? $v - 0x4000 : $v;
            $cells[] = $prev;
        } else {
            $prev = (($t - 0xE0) << 16) | (ord($s[$i]) << 8) | ord($s[$i + 1]);
            $i += 2;
            $cells[] = $prev;
        }
    }
    return array_pad($cells, $count, null);
}

// ---------------------------------------------------------------------------------------------------------------
// Grids
//
// A plane is the grid of the two-byte cells of a charset (and of its three- and four-byte sequences, by prefix):
// lead bytes [lo, hi], trail bytes in ranges, cells in lead-major order. multibyte.rs assembles each plane's grid
// from steps:
//   ['set', SET, LAYOUT]               the cells of a shared coded character set (a stream of rows x cols cells)
//                                      placed by LAYOUT ('offset' at lead/trail bytes, 'sjis', 'johab');
//   ['holes', STREAM, leads, trails]   a stream over the cells of a region that are still empty;
//   ['hangul', leads, trails]          the Hangul syllables U+AC00..U+D7A3 the grid does not hold yet, in order,
//                                      into the empty cells of a region;
//   ['patch']                          cells set to the code point measured (or emptied, or a pair of code points).

/** The grid position of cell ($r, $c) of a set placed with $layout (0-based), null when the layout leaves it out. */
function place(array $layout, int $r, int $c): ?array
{
    switch ($layout[0]) {
        case 'offset':
            return [$layout[1] + $r, $layout[2] + $c];
        case 'sjis':
            // JIS X 0208 row/cell to Shift_JIS
            $lead = intdiv($r, 2) + ($r < 62 ? 0x81 : 0xC1);
            $trail = $r % 2 === 0 ? $c + ($c < 63 ? 0x40 : 0x41) : $c + 0x9F;
            return [$lead, $trail];
        case 'johab':
            // KS X 1001 rows 1-12 (symbols) and 42-93 (hanja) to Johab
            if ($r < 12) {
                $lead = 0xD9 + intdiv($r, 2);
                $odd = $r % 2;
            } elseif ($r >= 41 && $r < 93) {
                $lead = 0xE0 + intdiv($r - 41, 2);
                $odd = ($r - 41) % 2;
            } else {
                return null;
            }
            $trail = $odd === 1 ? $c + 0xA1 : ($c < 78 ? $c + 0x31 : $c + 0x43);
            return [$lead, $trail];
    }
    fail('unknown layout ' . $layout[0]);
}

/** The bytes of a list of ranges, in order. */
function range_bytes(array $ranges): array
{
    $bytes = [];
    foreach ($ranges as [$lo, $hi]) {
        for ($b = $lo; $b <= $hi; $b++) {
            $bytes[] = $b;
        }
    }
    return $bytes;
}

/** The cells of a region in grid order (lead ranges and trail ranges): list of lead << 8 | trail. */
function region_cells(array $leads, array $trails): array
{
    $cells = [];
    foreach (range_bytes($leads) as $l) {
        foreach (range_bytes($trails) as $t) {
            $cells[] = $l << 8 | $t;
        }
    }
    return $cells;
}

final class Plane
{
    /** @var array<int, int|array> lead << 8 | trail => code point, or [cp, cp] for a pair (after assemble()) */
    public array $cells = [];
    /** @var list<array> the steps, streams and patch filled in (after assemble()) */
    public array $steps = [];

    /** @param list<array{int, int}> $leads @param list<array{int, int}> $trails */
    public function __construct(public string $name, public string $prefix, public array $leads, public array $trails)
    {
    }

    public static function in(array $ranges, int $b): bool
    {
        foreach ($ranges as [$lo, $hi]) {
            if ($b >= $lo && $b <= $hi) {
                return true;
            }
        }
        return false;
    }

    public function inside(int $l, int $t): bool
    {
        return self::in($this->leads, $l) && self::in($this->trails, $t);
    }
}

/**
 * Assembles a plane from its steps as multibyte.rs does, deriving the content of new 'holes' streams and the patch
 * from the measured cells $want (lead << 8 | trail => cp or [cp, cp]), and checks the result against $want.
 */
function assemble(Plane $plane, array $steps, array $want, array $sets, array &$streams): void
{
    $what = $plane->name;
    $grid = [];
    foreach ($steps as $step) {
        switch ($step[0]) {
            case 'set':
                [$rows, $cols] = $sets[$step[1]];
                foreach (decompress($streams[$step[1]], $rows * $cols) as $i => $cp) {
                    if ($cp === null) {
                        continue;
                    }
                    $pos = place($step[2], intdiv($i, $cols), $i % $cols);
                    if ($pos === null) {
                        continue;
                    }
                    if (!$plane->inside($pos[0], $pos[1])) {
                        fail("$what: set {$step[1]} places a cell outside the plane");
                    }
                    $grid[$pos[0] << 8 | $pos[1]] = $cp;
                }
                break;
            case 'holes':
            case 'hangul':
                [$leads, $trails] = $step[0] === 'holes' ? [$step[2], $step[3]] : [$step[1], $step[2]];
                $empty = [];
                foreach (region_cells([$leads], $trails) as $key) {
                    if (!Plane::in($plane->leads, $key >> 8)) {
                        continue;
                    }
                    if (!$plane->inside($key >> 8, $key & 0xFF)) {
                        fail("$what: a region outside the plane");
                    }
                    if (!isset($grid[$key])) {
                        $empty[] = $key;
                    }
                }
                if ($step[0] === 'hangul') {
                    $held = [];
                    foreach ($grid as $cp) {
                        if (is_int($cp)) {
                            $held[$cp] = true;
                        }
                    }
                    $k = 0;
                    for ($cp = 0xAC00; $cp <= 0xD7A3 && $k < count($empty); $cp++) {
                        if (!isset($held[$cp])) {
                            $grid[$empty[$k++]] = $cp;
                        }
                    }
                    break;
                }
                // a new stream is what the measurement has in these cells (pairs are patched afterwards)
                $name = $step[1];
                if (!isset($streams[$name])) {
                    $cells = [];
                    foreach ($empty as $key) {
                        $v = $want[$key] ?? null;
                        $cells[] = is_int($v) ? $v : null;
                    }
                    $streams[$name] = compress($cells);
                }
                foreach (decompress($streams[$name], count($empty)) as $k => $cp) {
                    if ($cp !== null) {
                        $grid[$empty[$k]] = $cp;
                    }
                }
                break;
            case 'patch':
                $patch = [];
                foreach ($want + $grid as $key => $ignored) {
                    $have = $grid[$key] ?? null;
                    $w = $want[$key] ?? null;
                    if ($have !== $w) {
                        $patch[$key] = $w;
                    }
                }
                ksort($patch);
                foreach ($patch as $key => $v) {
                    if ($v === null) {
                        unset($grid[$key]);
                    } else {
                        $grid[$key] = $v;
                    }
                }
                $step = ['patch', $patch];
                note(sprintf('%s: %d patched cells', $what, count($patch)));
                break;
            default:
                fail("$what: unknown step {$step[0]}");
        }
        $plane->steps[] = $step;
    }
    ksort($grid);
    ksort($want);
    if ($grid !== $want) {
        fail("$what: the assembled grid differs from the measurement");
    }
    $plane->cells = $grid;
}

/** The cells (lead << 8 | trail => cp or [cp, cp]) of the measured sequences prefix + lead + trail of a plane. */
function measured_cells(array $seq, Plane $plane): array
{
    $prefix = $plane->prefix;
    $cells = [];
    foreach (region_cells($plane->leads, $plane->trails) as $key) {
        $v = $seq[$prefix . sprintf('%04x', $key)] ?? null;
        if ($v !== null && $v[0] === 'C') {
            $cells[$key] = count($v[1]) === 1 ? $v[1][0] : $v[1];
        }
    }
    // nothing valid outside the plane
    $plen = strlen($prefix) + 4;
    foreach ($seq as $hex => $v) {
        $hex = (string) $hex;
        if ($v[0] === 'C' && strlen($hex) === $plen && str_starts_with($hex, $prefix)) {
            $key = hexdec(substr($hex, -4));
            if (!isset($cells[$key])) {
                fail("{$plane->name}: $hex decodes outside the plane");
            }
        }
    }
    return $cells;
}

/** A coded character set from a measured plane: rows x cols cells from (lead0, trail0). */
function extract_set(array $cells, int $rows, int $cols, int $lead0, int $trail0): array
{
    $list = [];
    for ($r = 0; $r < $rows; $r++) {
        for ($c = 0; $c < $cols; $c++) {
            $v = $cells[($lead0 + $r) << 8 | ($trail0 + $c)] ?? null;
            $list[] = is_int($v) ? $v : null;
        }
    }
    return $list;
}

/**
 * The skip windows of a plane: for each lead byte, the trail bytes at which an invalid pair makes //IGNORE skip both
 * bytes (elsewhere it skips the lead byte only), as ranges extended over the trails where both lengths behave alike
 * or that are valid, so that leads share windows. Pairs that skip neither one nor two bytes are returned in
 * $irregular (hex => measurement) for the family's checks.
 */
function windows(Plane $plane, array $seq, array $skipTrails, array &$irregular): array
{
    $windows = [];
    foreach (range_bytes($plane->leads) as $l) {
        $class = [];
        for ($t = 0; $t < 256; $t++) {
            $hex = $plane->prefix . sprintf('%02x%02x', $l, $t);
            $v = $seq[$hex] ?? null;
            if (in_array($t, $skipTrails, true) || $v === null || $v[0] !== 'X') {
                $class[$t] = '?';
                continue;
            }
            [, $k1, $k2] = $v;
            $one = $k1 <= 1 && 1 <= $k2;
            $two = $k1 <= 2 && 2 <= $k2;
            if (!$one && !$two) {
                $irregular[$hex] = $v;
                $class[$t] = '?';
            } else {
                $class[$t] = $one && $two ? '?' : ($two ? '2' : '1');
            }
        }
        $ranges = [];
        $open = null;
        for ($t = 0; $t <= 256; $t++) {
            $c = $t < 256 ? $class[$t] : '1';
            if ($c === '1') {
                if ($open !== null) {
                    $ranges[] = $open;
                    $open = null;
                }
            } elseif ($open === null) {
                $open = [$t, $t, $c === '2'];
            } else {
                $open[1] = $t;
                $open[2] = $open[2] || $c === '2';
            }
        }
        // ranges without a pair that must skip two bytes are left out
        $windows[$l] = array_values(array_map(static fn (array $r): array => [$r[0], $r[1]],
            array_filter($ranges, static fn (array $r): bool => $r[2])));
    }
    return $windows;
}

// ---------------------------------------------------------------------------------------------------------------
// The charsets

require __DIR__ . '/multibytes.inc.php';

const CHARSETS = MULTIBYTES;

const SJIS_TRAILS = [[0x40, 0x7E], [0x80, 0xFC]];
const GBK_TRAILS = [[0x40, 0x7E], [0x80, 0xFE]];
const BIG5_TRAILS = [[0x40, 0x7E], [0xA1, 0xFE]];
const UHC_TRAILS = [[0x41, 0x5A], [0x61, 0x7A], [0x81, 0xFE]];
const EUC = [[0xA1, 0xFE]];

/**
 * The planes of each charset: [name, prefix, lead ranges, trail ranges, steps]. Every lead byte of a plane gets a
 * row (also those without characters, for their skip windows); the shared sets come from the first plane that
 * defines them (see SETS).
 */
function plane_definitions(): array
{
    $sjis = [['set', 'JIS0208', ['sjis']]];
    $eucjp = [['set', 'JIS0208', ['offset', 0xA1, 0xA1]], ['holes', 'EUCJP_KANA', [0x8E, 0x8E], EUC]];
    $gbk = [['set', 'GB2312', ['offset', 0xA1, 0xA1]], ['holes', 'GBK', [0x81, 0xFE], GBK_TRAILS]];
    $big5 = [['holes', 'BIG5', [0xA1, 0xF9], BIG5_TRAILS]];
    $defs = [
        'Sjis' => [['SJIS', '', [[0x81, 0x9F], [0xE0, 0xEA]], SJIS_TRAILS, [...$sjis, ['patch']]]],
        'Cp932' => [['CP932', '', [[0x81, 0x9F], [0xE0, 0xFC]], SJIS_TRAILS,
            [...$sjis, ['holes', 'CP932', [0x81, 0xFC], SJIS_TRAILS], ['patch']]]],
        'EucJp' => [
            ['EUCJP', '', [[0x8E, 0x8E], [0xA0, 0xFE]], EUC, [...$eucjp, ['patch']]],
            ['EUCJP_3', '8f', EUC, EUC, [['set', 'JIS0212', ['offset', 0xA1, 0xA1]], ['patch']]],
        ],
        'EucJpMs' => [
            ['EUCJPMS', '', [[0x8E, 0x8E], [0xA0, 0xFE]], EUC, [...$eucjp, ['holes', 'EUCJPMS', [0xA0, 0xFE], EUC], ['patch']]],
            ['EUCJPMS_3', '8f', EUC, EUC,
                [['set', 'JIS0212', ['offset', 0xA1, 0xA1]], ['holes', 'EUCJPMS_3', [0xA1, 0xFE], EUC], ['patch']]],
        ],
        'Gbk' => [['GBK', '', [[0x81, 0xFE]], GBK_TRAILS, [...$gbk, ['patch']]]],
        'Gb18030' => [['GB18030', '', [[0x81, 0xFE]], GBK_TRAILS,
            [...$gbk, ['holes', 'GB18030', [0x81, 0xFE], GBK_TRAILS], ['patch']]]],
        'EucCn' => [['EUCCN', '', [[0x8E, 0x8F], [0xA1, 0xFE]], EUC, [['set', 'GB2312', ['offset', 0xA1, 0xA1]], ['patch']]]],
        'Big5' => [['BIG5', '', [[0xA1, 0xF9]], BIG5_TRAILS, [...$big5, ['patch']]]],
        'Big5Hkscs' => [['BIG5HKSCS', '', [[0x81, 0xFE]], BIG5_TRAILS,
            [...$big5, ['holes', 'BIG5HKSCS', [0x81, 0xFE], BIG5_TRAILS], ['patch']]]],
        'EucKr' => [['EUCKR', '', [[0xA1, 0xFF]], EUC, [['set', 'KSX1001', ['offset', 0xA1, 0xA1]], ['patch']]]],
        'Uhc' => [['UHC', '', [[0x81, 0xC8], [0xCA, 0xFD]], UHC_TRAILS, [['set', 'KSX1001', ['offset', 0xA1, 0xA1]],
            ['hangul', [0x81, 0xA0], UHC_TRAILS], ['hangul', [0xA1, 0xC6], [[0x41, 0x5A], [0x61, 0x7A], [0x81, 0xA0]]],
            ['patch']]]],
        'Johab' => [['JOHAB', '', [[0x84, 0xD3], [0xD9, 0xDE], [0xE0, 0xF9]], [[0x31, 0x7E], [0x81, 0xFE]],
            [['set', 'KSX1001', ['johab']], ['holes', 'JOHAB', [0x84, 0xD3], [[0x31, 0x7E], [0x81, 0xFE]]], ['patch']]]],
        'EucTw' => [['EUCTW', '', EUC, EUC, [['set', 'CNS1', ['offset', 0xA1, 0xA1]], ['patch']]]],
    ];
    // the four-byte planes of EUC-TW (8E A1 is plane 1 again, the plane of the two-byte sequences)
    foreach ([2, 3, 4, 5, 6, 7, 15] as $p) {
        $defs['EucTw'][] = ["EUCTW_$p", sprintf('8e%02x', 0xA0 + $p), EUC, EUC,
            [['set', "CNS$p", ['offset', 0xA1, 0xA1]], ['patch']]];
    }
    return $defs;
}

/** The shared sets: name => [rows, cols, the plane that defines it, lead0, trail0]. */
const SETS = [
    'JIS0208' => [94, 94, 'EUCJP', 0xA1, 0xA1], 'JIS0212' => [94, 94, 'EUCJP_3', 0xA1, 0xA1],
    'GB2312' => [94, 94, 'EUCCN', 0xA1, 0xA1], 'KSX1001' => [94, 94, 'EUCKR', 0xA1, 0xA1],
    'CNS1' => [94, 94, 'EUCTW', 0xA1, 0xA1], 'CNS2' => [94, 94, 'EUCTW_2', 0xA1, 0xA1],
    'CNS3' => [94, 94, 'EUCTW_3', 0xA1, 0xA1], 'CNS4' => [94, 94, 'EUCTW_4', 0xA1, 0xA1],
    'CNS5' => [94, 94, 'EUCTW_5', 0xA1, 0xA1], 'CNS6' => [94, 94, 'EUCTW_6', 0xA1, 0xA1],
    'CNS7' => [94, 94, 'EUCTW_7', 0xA1, 0xA1], 'CNS15' => [94, 94, 'EUCTW_15', 0xA1, 0xA1],
];

$dec = [];
$enc = [];
foreach (CHARSETS as $variant => $cs) {
    $dec[$variant] = measure_decode($cs, '-', $cs === 'GB18030');
    $enc[$variant] = measure_encode($cs);
    note("$cs: " . count($dec[$variant]) . ' sequences, ' . count($enc[$variant]) . ' code points encode');
}

$planes = [];
foreach (plane_definitions() as $variant => $list) {
    foreach ($list as [$name, $prefix, $leads, $trails, $steps]) {
        $planes[$variant][$name] = [new Plane($name, $prefix, $leads, $trails), $steps];
    }
}
$streams = [];
$sets = [];
foreach (SETS as $set => [$rows, $cols, $from, $lead0, $trail0]) {
    foreach ($planes as $variant => $list) {
        if (isset($list[$from])) {
            $cells = measured_cells($dec[$variant], $list[$from][0]);
            $streams[$set] = compress(extract_set($cells, $rows, $cols, $lead0, $trail0));
            $sets[$set] = [$rows, $cols];
        }
    }
    if (!isset($sets[$set])) {
        fail("set $set: no plane $from");
    }
}
foreach ($planes as $variant => $list) {
    foreach ($list as [$plane, $steps]) {
        assemble($plane, $steps, measured_cells($dec[$variant], $plane), $sets, $streams);
    }
}
$total = 0;
foreach ($streams as $name => $s) {
    $total += strlen($s);
    note(sprintf('stream %s: %d bytes', $name, strlen($s)));
}
note("streams: $total bytes");

// ---------------------------------------------------------------------------------------------------------------
// Decoding: single bytes, lead bytes, skip windows and the longer sequences of each family

/** How multibyte.rs decodes each charset. */
const FAMILY = ['Sjis' => 'dbcs', 'Cp932' => 'dbcs', 'EucJp' => 'eucjp', 'EucJpMs' => 'eucjp', 'Gbk' => 'dbcs',
    'Gb18030' => 'gb18030', 'EucCn' => 'dbcs', 'Big5' => 'dbcs', 'Big5Hkscs' => 'dbcs', 'EucKr' => 'dbcs',
    'Uhc' => 'dbcs', 'Johab' => 'dbcs', 'EucTw' => 'euctw'];

/**
 * The irregular two-byte sequences, checked against the measurement: UHC's A2 E8 makes //IGNORE skip four bytes
 * and needs them in the input (glibc reads past the end of the input otherwise; the oracle's PHP then reports an
 * incomplete character, see the vectors); EUC-JP-MS decodes 8E E0..8E FF to U+FFFD under //IGNORE (EILSEQ
 * without it).
 */
const QUIRKS = ['Uhc' => 'skip4', 'EucJpMs' => 'fffd'];

/** The byte ranges of a list of bytes. */
function to_ranges(array $bytes): array
{
    sort($bytes);
    $ranges = [];
    foreach ($bytes as $b) {
        $last = count($ranges) - 1;
        if ($last >= 0 && $ranges[$last][1] === $b - 1) {
            $ranges[$last][1] = $b;
        } else {
            $ranges[] = [$b, $b];
        }
    }
    return $ranges;
}

$specs = [];
foreach (CHARSETS as $variant => $cs) {
    $seq = $dec[$variant];
    $family = FAMILY[$variant];
    [$main] = reset($planes[$variant]);
    // single bytes: characters (as ranges of consecutive code points), lead bytes, invalid bytes (one skipped)
    $single = [];
    $leads = [];
    for ($b = 0; $b < 256; $b++) {
        $v = $seq[sprintf('%02x', $b)];
        if ($v[0] === 'C') {
            if (count($v[1]) !== 1) {
                fail("$cs: byte $b decodes to " . count($v[1]) . ' characters');
            }
            $last = count($single) - 1;
            if ($last >= 0 && $single[$last][1] === $b - 1 && $single[$last][2] + $b - $single[$last][0] === $v[1][0]) {
                $single[$last][1] = $b;
            } else {
                $single[] = [$b, $b, $v[1][0]];
            }
        } elseif ($v[0] === 'I') {
            $leads[] = $b;
        } elseif (!($v[1] <= 1 && 1 <= $v[2])) {
            fail("$cs: invalid byte $b skips {$v[1]}-{$v[2]}");
        }
    }
    $longLead = ['eucjp' => 0x8F, 'euctw' => 0x8E][$family] ?? null;
    foreach ($leads as $l) {
        if ($l !== $longLead && !Plane::in($main->leads, $l)) {
            fail("$cs: lead byte $l has no row");
        }
    }
    foreach (range_bytes($main->leads) as $l) {
        if (!in_array($l, $leads, true)) {
            fail("$cs: row $l is not a lead byte");
        }
    }
    // the two-byte sequences of the main plane
    $irregular = [];
    $prefixTrails = $family === 'gb18030' ? range(0x30, 0x39) : [];
    $windows = windows($main, $seq, $prefixTrails, $irregular);
    foreach (range_bytes($main->leads) as $l) {
        for ($t = 0; $t < 256; $t++) {
            $hex = sprintf('%02x%02x', $l, $t);
            $v = $seq[$hex];
            if ($v[0] === 'I') {
                if (!in_array($t, $prefixTrails, true)) {
                    fail("$cs: $hex is incomplete");
                }
            } elseif (in_array($t, $prefixTrails, true)) {
                fail("$cs: $hex is not incomplete");
            } elseif ($v[0] === 'X' && !isset($irregular[$hex])) {
                $k = Plane::in($windows[$l], $t) ? 2 : 1;
                if ($k < $v[1] || $k > $v[2]) {
                    fail("$cs: $hex skips {$v[1]}-{$v[2]}, the window says $k");
                }
            }
        }
    }
    $quirk = null;
    $expected = [];
    switch (QUIRKS[$variant] ?? null) {
        case 'skip4':
            // the probe's output for A2 E8 alone reads past its input (PHP's is checked below)
            $expected = ['a2e8' => ['X', 4, 4, $irregular['a2e8'][3] ?? '']];
            $quirk = ['skip4', 0xA2E8];
            set_error_handler(static function (int $no, string $msg) use (&$notice): bool {
                $notice = $msg;
                return true;
            });
            foreach (["\xA2\xE8" => false, "\xA2\xE8A" => false, "\xA2\xE8AB" => '', "\xA2\xE8ABC" => 'C'] as $in => $out) {
                $notice = null;
                $r = iconv('UHC', 'UTF-8//IGNORE', $in);
                if ($r !== $out || ($out === false && !str_contains((string) $notice, 'incomplete'))) {
                    fail('UHC: A2 E8 under //IGNORE in PHP: ' . bin2hex($in) . ' gives ' . var_export($r, true));
                }
            }
            // and glibc reports the skip
            if (iconv('UHC', 'UTF-8//ignore', "\xA2\xE8ABC") !== false) {
                fail('UHC: A2 E8 skips silently');
            }
            restore_error_handler();
            break;
        case 'fffd':
            for ($t = 0xE0; $t <= 0xFF; $t++) {
                $expected[sprintf('8e%02x', $t)] = ['X', 0, 0, '=fffd'];
            }
            $quirk = ['fffd', 0x8E, 0xE0, 0xFF, 0xFFFD];
            break;
    }
    if ($irregular != $expected) {
        fail("$cs: irregular sequences " . json_encode($irregular));
    }

    // the longer sequences
    $extra = [];
    switch ($family) {
        case 'eucjp':
            // 8F, a second byte in a range that waits for the third, else one byte skipped; an invalid three-byte
            // sequence skips one byte (EUC-JP) or all three (EUC-JP-MS)
            $wait = [];
            for ($t = 0; $t < 256; $t++) {
                $v = $seq[sprintf('8f%02x', $t)];
                if ($v[0] === 'I') {
                    $wait[] = $t;
                } elseif ($v[0] !== 'X' || !($v[1] <= 1 && 1 <= $v[2])) {
                    fail("$cs: 8f" . sprintf('%02x', $t) . ' ' . json_encode($v));
                }
            }
            $wait = to_ranges($wait);
            if (count($wait) !== 1) {
                fail("$cs: the second bytes after 8F are not one range");
            }
            $skip3 = null;
            [$third] = array_values(array_slice($planes[$variant], 1, 1));
            foreach (range_bytes($wait) as $b1) {
                for ($b2 = 0; $b2 < 256; $b2++) {
                    $hex = sprintf('8f%02x%02x', $b1, $b2);
                    $v = $seq[$hex];
                    if ($v[0] === 'C') {
                        if (!isset($third[0]->cells[$b1 << 8 | $b2])) {
                            fail("$cs: $hex outside the three-byte plane");
                        }
                        continue;
                    }
                    if ($v[0] !== 'X') {
                        fail("$cs: $hex " . json_encode($v));
                    }
                    $k = $variant === 'EucJp' ? 1 : 3;
                    if ($k < $v[1] || $k > $v[2]) {
                        fail("$cs: $hex skips {$v[1]}-{$v[2]}, not $k");
                    }
                    $skip3 = $k;
                }
            }
            $extra = ['wait' => $wait[0], 'skip3' => $skip3];
            break;
        case 'euctw':
            // 8E, a plane byte A1..B0 and two more bytes; an invalid sequence skips one byte
            for ($t = 0; $t < 256; $t++) {
                $hex = sprintf('8e%02x', $t);
                $v = $seq[$hex];
                if ($t >= 0xA1 && $t <= 0xB0) {
                    if ($v[0] !== 'I') {
                        fail("$cs: $hex is not incomplete");
                    }
                    for ($x = 0; $x < 256; $x++) {
                        if ($seq[$hex . sprintf('%02x', $x)][0] !== 'I') {
                            fail("$cs: {$hex}$x is not incomplete");
                        }
                    }
                } elseif ($v[0] !== 'X' || !($v[1] <= 1 && 1 <= $v[2])) {
                    fail("$cs: $hex " . json_encode($v));
                }
            }
            $byPrefix = [];
            foreach ($planes[$variant] as [$plane]) {
                $byPrefix[$plane->prefix] = $plane;
            }
            foreach ($seq as $hex => $v) {
                $hex = (string) $hex;
                if (strlen($hex) !== 8) {
                    continue;
                }
                $p = substr($hex, 0, 4);
                $plane = $p === '8ea1' ? $byPrefix[''] : ($byPrefix[$p] ?? null);
                $key = hexdec(substr($hex, 4));
                if ($v[0] === 'C') {
                    if ($plane === null || ($plane->cells[$key] ?? null) !== (count($v[1]) === 1 ? $v[1][0] : $v[1])) {
                        fail("$cs: $hex differs from its plane");
                    }
                } elseif ($v[0] !== 'X' || !($v[1] <= 1 && 1 <= $v[2])) {
                    fail("$cs: $hex " . json_encode($v));
                } elseif ($plane !== null && isset($plane->cells[$key])) {
                    fail("$cs: $hex is invalid but in its plane");
                }
            }
            break;
        case 'gb18030':
            // four bytes [81-FE][30-39][81-FE][30-39]: the three-byte prefixes all wait for the fourth byte; a third
            // byte outside 81..FE makes //IGNORE skip three bytes, else four; the valid ones map linearly to the
            // BMP code points the two-byte sequences lack (ranges) and to U+10000.. from 90 30 81 30
            $linear = static fn (string $b): int => (((ord($b[0]) - 0x81) * 10 + ord($b[1]) - 0x30) * 126
                + ord($b[2]) - 0x81) * 10 + ord($b[3]) - 0x30;
            $runs = [];
            $sampled = 0;
            foreach ($seq as $hex => $v) {
                $hex = (string) $hex;
                if (strlen($hex) === 6 && $v[0] !== 'I') {
                    fail("$cs: $hex is not incomplete");
                }
                if (strlen($hex) !== 8) {
                    continue;
                }
                $b = hex2bin($hex);
                if ($v[0] === 'C') {
                    if (count($v[1]) !== 1) {
                        fail("$cs: $hex decodes to several characters");
                    }
                    $runs[$linear($b)] = $v[1][0];
                    continue;
                }
                $k = ord($b[2]) < 0x81 || ord($b[2]) > 0xFE ? 3 : 4;
                if ($v[0] !== 'X' || $k < $v[1] || $k > $v[2]) {
                    fail("$cs: $hex " . json_encode($v) . ", not $k");
                }
                $sampled++;
            }
            ksort($runs);
            $bmp = [];
            $supplementary = null;
            foreach ($runs as $i => $cp) {
                if ($cp >= 0x10000) {
                    if ($supplementary === null) {
                        $supplementary = [$i, $cp];
                    } elseif ($cp - $supplementary[1] !== $i - $supplementary[0]) {
                        fail("$cs: the supplementary planes are not linear");
                    }
                    continue;
                }
                $last = count($bmp) - 1;
                if ($last >= 0 && $bmp[$last][2] === $i - 1 && $bmp[$last][1] + $i - $bmp[$last][0] === $cp) {
                    $bmp[$last][2] = $i;
                } else {
                    $bmp[] = [$i, $cp, $i];
                }
            }
            if ($supplementary !== [189000, 0x10000] || $runs[array_key_last($runs)] !== 0x10FFFF) {
                fail("$cs: the supplementary planes are not at 90 30 81 30");
            }
            note("$cs: " . count($bmp) . " four-byte BMP ranges, $sampled invalid four-byte sequences checked");
            $extra = ['ranges' => $bmp];
            break;
    }
    // whether //IGNORE skips invalid input silently: glibc's EILSEQ after a skip shows through a suffix PHP does not
    // know (//ignore); every invalid sequence of a charset must agree (every 97th of them is tried, the irregular ones
    // aside)
    $silent = null;
    $k = 0;
    set_error_handler(static fn (): bool => true);
    foreach ($seq as $hex => $v) {
        $hex = (string) $hex;
        if ($v[0] !== 'X' || isset($irregular[$hex]) || $k++ % 97 !== 0) {
            continue;
        }
        $r = iconv($cs, 'UTF-8//ignore', hex2bin($hex) . 'zz');
        if ($silent !== null && $silent !== ($r !== false)) {
            fail("$cs: //IGNORE skips $hex " . ($silent ? 'loudly' : 'silently') . ', unlike the others');
        }
        $silent = $r !== false;
    }
    restore_error_handler();
    $specs[$variant] = ['single' => $single, 'leads' => $leads, 'windows' => $windows, 'quirk' => $quirk,
        'silent' => $silent] + $extra;
}

// ---------------------------------------------------------------------------------------------------------------
// Encoding: the inverse of the decoder and the exceptions

/** The planes in the order the encoder's inverse lists them, with the bytes before lead and trail. */
function encode_planes(string $variant, array $planes): array
{
    $list = [];
    foreach ($planes[$variant] as [$plane]) {
        $list[] = [$plane, $plane->prefix];
        if ($variant === 'EucTw' && $plane->prefix === '') {
            $list[] = [$plane, '8ea1'];
        }
    }
    return $list;
}

/** The inverse of a decoder: cp => code (the bytes as an integer), the first or the last sequence winning. */
function inverse(array $single, array $encodePlanes, bool $last): array
{
    $inv = [];
    $put = static function (int $cp, int $code) use (&$inv, $last): void {
        if ($last || !isset($inv[$cp])) {
            $inv[$cp] = $code;
        }
    };
    foreach ($single as [$lo, $hi, $base]) {
        for ($b = $lo; $b <= $hi; $b++) {
            $put($base + $b - $lo, $b);
        }
    }
    foreach ($encodePlanes as [$plane, $prefix]) {
        $p = $prefix === '' ? 0 : hexdec($prefix);
        foreach ($plane->cells as $key => $cp) {
            if (is_int($cp)) {
                $put($cp, $p << 16 | $key);
            }
        }
    }
    return $inv;
}

const REMOVE = 0xFFFFFFFF;

foreach (CHARSETS as $variant => $cs) {
    $measured = [];
    foreach ($enc[$variant] as $cp => $bytes) {
        if ($cp >= 0xE0000 && $cp <= 0xE007F && $bytes === '') {
            // the tag characters vanish where the charset cannot represent them (engine.rs)
            continue;
        }
        if ($bytes === '' || strlen($bytes) > 4) {
            fail("$cs: " . u($cp) . ' encodes to ' . bin2hex($bytes));
        }
        $measured[$cp] = hexdec(bin2hex($bytes));
    }
    if ($variant === 'Gb18030') {
        // the four-byte sequences are computed, the generator checks them below
        foreach ($measured as $cp => $code) {
            if ($code > 0xFFFF) {
                unset($measured[$cp]);
            }
        }
    }
    $best = null;
    foreach ([false, true] as $last) {
        $inv = inverse($specs[$variant]['single'], encode_planes($variant, $planes), $last);
        $patch = [];
        foreach ($measured + $inv as $cp => $ignored) {
            $want = $measured[$cp] ?? REMOVE;
            if (($inv[$cp] ?? REMOVE) !== $want) {
                $patch[$cp] = $want;
            }
        }
        ksort($patch);
        if ($best === null || count($patch) < count($best[1])) {
            $best = [$last, $patch];
        }
    }
    $specs[$variant]['encode_last'] = $best[0];
    $specs[$variant]['encode'] = $best[1];
    note(sprintf('%s: encoder %s, %d exceptions', $cs, $best[0] ? 'last' : 'first', count($best[1])));
}

// GB18030: the computed four-byte sequences of every code point the two-byte table lacks
$ranges = $specs['Gb18030']['ranges'];
foreach ($enc['Gb18030'] as $cp => $bytes) {
    if (strlen($bytes) !== 4) {
        continue;
    }
    if ($cp >= 0x10000) {
        $i = 189000 + $cp - 0x10000;
    } else {
        $i = null;
        foreach ($ranges as [$first, $start, $end]) {
            if ($cp >= $start && $cp <= $start + $end - $first) {
                $i = $first + $cp - $start;
            }
        }
        if ($i === null) {
            fail('GB18030: ' . u($cp) . ' is in no four-byte range');
        }
    }
    $b = chr(0x81 + intdiv($i, 12600)) . chr(0x30 + intdiv($i, 1260) % 10) . chr(0x81 + intdiv($i, 10) % 126)
        . chr(0x30 + $i % 10);
    if ($b !== $bytes) {
        fail('GB18030: ' . u($cp) . ' encodes to ' . bin2hex($bytes) . ', not ' . bin2hex($b));
    }
}

// ---------------------------------------------------------------------------------------------------------------
// The comparison with the published mapping files

/** A published file, cached for the run. */
function fetch(string $url): string
{
    static $cache = [];
    if (!isset($cache[$url])) {
        $text = file_get_contents($url);
        if ($text === false) {
            fail("cannot fetch $url");
        }
        $cache[$url] = $text;
    }
    return $cache[$url];
}

/** The data lines of a mapping file as lists of integers (the columns before a comment). */
function columns(string $url): array
{
    $rows = [];
    foreach (explode("\n", fetch($url)) as $line) {
        $line = trim(explode('#', $line, 2)[0]);
        if ($line !== '') {
            $rows[] = array_map(static fn (string $c): int => intval($c, 0), preg_split('/\s+/', $line));
        }
    }
    return $rows;
}

const UNICODE_ORG = 'https://www.unicode.org/Public/MAPPINGS/';
const WHATWG = 'https://encoding.spec.whatwg.org/';

/**
 * A published mapping as code => [cp] (codes are the bytes as an integer), from a unicode.org file whose first
 * column is the code ($map turns it into the bytes) and second the code point.
 */
function unicode_map(string $path, ?callable $map = null, int $codeColumn = 0, int $cpColumn = 1): array
{
    $m = [];
    foreach (columns(UNICODE_ORG . $path) as $cols) {
        if (count($cols) > $cpColumn) {
            $code = $map === null ? $cols[$codeColumn] : $map($cols[$codeColumn]);
            if ($code !== null) {
                $m[$code] = [$cols[$cpColumn]];
            }
        }
    }
    return $m;
}

/** A WHATWG index as code => [cp]: $pointer turns a pointer into the bytes. */
function whatwg_map(string $index, callable $pointer): array
{
    $m = [];
    foreach (columns(WHATWG . "index-$index.txt") as [$p, $cp]) {
        $m[$pointer($p)] = [$cp];
    }
    return $m;
}

function ascii(int $last = 0x7F): array
{
    $m = [];
    for ($b = 0; $b <= $last; $b++) {
        $m[$b] = [$b];
    }
    return $m;
}

/** What glibc decodes, code => [cps] ($maxBytes: the longest sequences included). */
function glibc_map(array $seq, int $maxBytes): array
{
    $m = [];
    foreach ($seq as $hex => $v) {
        $hex = (string) $hex;
        if ($v[0] === 'C' && strlen($hex) <= 2 * $maxBytes) {
            $m[hexdec($hex)] = $v[1];
        }
    }
    return $m;
}

/** The differences, merged into runs where codes and code points advance together. */
function differences(array $glibc, array $file): array
{
    $show = static fn (?array $cps): string => $cps === null ? 'none' : implode(' ', array_map('u', $cps));
    $codes = array_keys($glibc + $file);
    sort($codes);
    $runs = [];
    foreach ($codes as $code) {
        $g = $glibc[$code] ?? null;
        $f = $file[$code] ?? null;
        if ($g === $f) {
            continue;
        }
        $last = count($runs) - 1;
        if ($last >= 0) {
            [$start, $end, $g0, $f0] = $runs[$last];
            $step = $code - $start;
            $advances = static fn (?array $now, ?array $first): bool => $now === null ? $first === null
                : ($first !== null && count($now) === 1 && count($first) === 1 && $now[0] === $first[0] + $step);
            if ($end === $code - 1 && $advances($g, $g0) && $advances($f, $f0)) {
                $runs[$last][1] = $code;
                continue;
            }
        }
        $runs[] = [$code, $code, $g, $f];
    }
    $hex = static fn (int $code): string => sprintf($code > 0xFFFFFF ? '%08X' : ($code > 0xFFFF ? '%06X' : ($code > 0xFF ? '%04X' : '%02X')), $code);
    $out = [];
    foreach ($runs as [$start, $end, $g, $f]) {
        $range = static function (?array $first, int $n) use ($show): string {
            if ($first === null || $n === 0) {
                return $show($first);
            }
            return $show($first) . '..=' . u($first[0] + $n);
        };
        $codes = $start === $end ? $hex($start) : $hex($start) . '..=' . $hex($end);
        $out[] = sprintf('%s is %s (the file: %s)', $codes, $range($g, $end - $start), $range($f, $end - $start));
    }
    return $out;
}

$sjisPointer = static function (int $p): int {
    $lead = intdiv($p, 188);
    $trail = $p % 188;
    return ($lead + ($lead < 0x1F ? 0x81 : 0xC1)) << 8 | ($trail + ($trail < 0x3F ? 0x40 : 0x41));
};
$eucPointer = static fn (int $p): int => (intdiv($p, 94) + 0xA1) << 8 | ($p % 94 + 0xA1);
$gbkPointer = static fn (int $p): int => (intdiv($p, 190) + 0x81) << 8 | ($p % 190 + ($p % 190 < 0x3F ? 0x40 : 0x41));
$kana = [];
for ($b = 0xA1; $b <= 0xDF; $b++) {
    $kana[$b] = [0xFF61 + $b - 0xA1];
}
$plus8080 = static fn (int $c): int => $c + 0x8080;
$jis = unicode_map('OBSOLETE/EASTASIA/JIS/JIS0208.TXT', $plus8080, 1, 2)
    + array_combine(array_map(static fn (int $b): int => 0x8E00 | $b, array_keys($kana)), $kana)
    + unicode_map('OBSOLETE/EASTASIA/JIS/JIS0212.TXT', static fn (int $c): int => 0x8F8080 + $c);
$whatwgEucJp = ascii() + array_combine(array_map(static fn (int $b): int => 0x8E00 | $b, array_keys($kana)), $kana)
    + whatwg_map('jis0208', $eucPointer) + whatwg_map('jis0212', static fn (int $p): int => 0x8F0000 | $eucPointer($p));
$cp936 = unicode_map('VENDORS/MICSFT/WINDOWS/CP936.TXT');
$gb2312 = ascii();
foreach ($cp936 as $code => $cp) {
    if ($code >= 0xA1A1 && ($code & 0xFF) >= 0xA1) {
        $gb2312[$code] = $cp;
    }
}
$gbkIndex = whatwg_map('gb18030', $gbkPointer);
$gb18030 = ascii() + $gbkIndex;
$big5Whatwg = ascii(0x80);
foreach (columns(WHATWG . 'index-big5.txt') as [$p, $cp]) {
    $trail = $p % 157;
    $big5Whatwg[(intdiv($p, 157) + 0x81) << 8 | ($trail + ($trail < 0x3F ? 0x40 : 0x62))] = [$cp];
}
foreach ([1133 => [0xCA, 0x304], 1135 => [0xCA, 0x30C], 1164 => [0xEA, 0x304], 1166 => [0xEA, 0x30C]] as $p => $cps) {
    $trail = $p % 157;
    $big5Whatwg[(intdiv($p, 157) + 0x81) << 8 | ($trail + ($trail < 0x3F ? 0x40 : 0x62))] = $cps;
}
$cns = [];
foreach (columns(UNICODE_ORG . 'OBSOLETE/EASTASIA/OTHER/CNS11643.TXT') as [$code, $cp]) {
    $plane = $code >> 16;
    $cell = ($code & 0xFFFF) + 0x8080;
    if ($plane === 1) {
        $cns[$cell] = [$cp];
    }
    $cns[(0x8EA0 + $plane) << 16 | $cell] = [$cp];
}

/** variant => list of [description of the file, its mapping] (the first is the one the documentation leads with). */
$references = [
    'Sjis' => [['unicode.org OBSOLETE/EASTASIA/JIS/SHIFTJIS.TXT', unicode_map('OBSOLETE/EASTASIA/JIS/SHIFTJIS.TXT') + ascii(0x1F) + [0x7F => [0x7F]]]],
    'Cp932' => [['unicode.org VENDORS/MICSFT/WINDOWS/CP932.TXT', unicode_map('VENDORS/MICSFT/WINDOWS/CP932.TXT')],
        ['the WHATWG Encoding Standard\'s Shift_JIS (index-jis0208)', ascii(0x80) + $kana + whatwg_map('jis0208', $sjisPointer)]],
    'EucJp' => [['unicode.org OBSOLETE/EASTASIA/JIS/JIS0208.TXT, JIS0212.TXT and JIS0201.TXT (EUC-JP forms, ASCII below 0x80)', ascii() + $jis],
        ['the WHATWG Encoding Standard\'s EUC-JP (index-jis0208, index-jis0212)', $whatwgEucJp]],
    'EucJpMs' => [['no published mapping file; unicode.org OBSOLETE/EASTASIA/JIS/JIS0208.TXT, JIS0212.TXT and JIS0201.TXT (EUC-JP forms, ASCII below 0x80)', ascii() + $jis]],
    'Gbk' => [['unicode.org VENDORS/MICSFT/WINDOWS/CP936.TXT', $cp936],
        ['the WHATWG Encoding Standard\'s GBK (index-gb18030)', ascii() + [0x80 => [0x20AC]] + $gbkIndex]],
    'Gb18030' => [['the WHATWG Encoding Standard\'s gb18030 (index-gb18030; the four-byte sequences are compared with index-gb18030-ranges below)', $gb18030]],
    'EucCn' => [['unicode.org no longer publishes GB2312.TXT; unicode.org VENDORS/MICSFT/WINDOWS/CP936.TXT, bytes below 0x80 and A1A1..FEFE', $gb2312]],
    'Big5' => [['unicode.org OBSOLETE/EASTASIA/OTHER/BIG5.TXT (ASCII below 0x81)', unicode_map('OBSOLETE/EASTASIA/OTHER/BIG5.TXT') + ascii(0x80)],
        ['unicode.org VENDORS/MICSFT/WINDOWS/CP950.TXT', unicode_map('VENDORS/MICSFT/WINDOWS/CP950.TXT')]],
    'Big5Hkscs' => [['the WHATWG Encoding Standard\'s Big5 (index-big5, HKSCS)', $big5Whatwg]],
    'EucKr' => [['unicode.org OBSOLETE/EASTASIA/KSC/KSX1001.TXT (EUC-KR forms, ASCII below 0x80)', unicode_map('OBSOLETE/EASTASIA/KSC/KSX1001.TXT', $plus8080) + ascii()]],
    'Uhc' => [['unicode.org VENDORS/MICSFT/WINDOWS/CP949.TXT', unicode_map('VENDORS/MICSFT/WINDOWS/CP949.TXT')],
        ['the WHATWG Encoding Standard\'s EUC-KR (index-euc-kr)', ascii() + whatwg_map('euc-kr', static fn (int $p): int => (intdiv($p, 190) + 0x81) << 8 | ($p % 190 + 0x41))]],
    'Johab' => [['unicode.org OBSOLETE/EASTASIA/KSC/JOHAB.TXT', unicode_map('OBSOLETE/EASTASIA/KSC/JOHAB.TXT') + ascii(0x1F) + [0x7F => [0x7F]]]],
    'EucTw' => [['unicode.org OBSOLETE/EASTASIA/OTHER/CNS11643.TXT, planes 1 and 2 (plane 1 also as two bytes; ASCII below 0x80)', $cns + ascii()]],
];

$docs = [];
$listings = [];
foreach (CHARSETS as $variant => $cs) {
    $glibc = glibc_map($dec[$variant], $variant === 'Gb18030' ? 2 : 4);
    $parts = [];
    foreach ($references[$variant] as $k => [$file, $map]) {
        if ($variant === 'EucTw') {
            // planes 1 and 2 (the file has plane 14 too, glibc planes 3 to 7 and 15: counted below)
            $keep = static fn (int $code): bool => $code <= 0xFFFF || ($code >> 16) === 0x8EA1 || ($code >> 16) === 0x8EA2;
            $glibc = array_filter($glibc, $keep, ARRAY_FILTER_USE_KEY);
            $map = array_filter($map, $keep, ARRAY_FILTER_USE_KEY);
        }
        $diffs = differences($glibc, $map);
        $parts[] = $diffs === [] ? "$file: the same mapping" : sprintf('%s: %d difference%s', $file, count($diffs), count($diffs) === 1 ? '' : 's');
        if ($diffs !== []) {
            $listings[] = ["`$cs` and $file", $diffs];
        }
    }
    if ($variant === 'EucTw') {
        $counts = [];
        foreach (glibc_map($dec[$variant], 4) as $code => $cps) {
            $plane = $code >> 16;
            if ($plane > 0x8EA2) {
                $counts[$plane - 0x8EA0] = ($counts[$plane - 0x8EA0] ?? 0) + 1;
            }
        }
        ksort($counts);
        $fileOnly = count(array_filter($cns, static fn (int $code): bool => ($code >> 16) === 0x8EAE, ARRAY_FILTER_USE_KEY));
        $parts[] = 'the file has no planes ' . implode(', ', array_keys($counts)) . ' ('
            . implode(', ', array_map(static fn (int $p, int $n): string => "$n characters in plane $p", array_keys($counts), $counts))
            . "), and glibc no plane 14 (the file: $fileOnly characters)";
    }
    $docs[$variant] = $parts;
}
// the four-byte sequences of GB18030 and the WHATWG ranges
$whatwgRanges = columns(WHATWG . 'index-gb18030-ranges.txt');
$glibcRanges = array_map(static fn (array $r): array => [$r[0], $r[1]], $specs['Gb18030']['ranges']);
$rangeDiffs = [];
$byIndex = [];
foreach ($whatwgRanges as [$p, $cp]) {
    $byIndex[$p] = $cp;
}
foreach ($glibcRanges as [$p, $cp]) {
    if (($byIndex[$p] ?? null) !== $cp) {
        $rangeDiffs[] = sprintf('glibc starts a range at index %d (%s)', $p, u($cp));
    }
    unset($byIndex[$p]);
}
foreach ($byIndex as $p => $cp) {
    $rangeDiffs[] = sprintf('the file starts a range at index %d (%s)', $p, u($cp));
}
$docs['Gb18030'][] = 'its four-byte BMP sequences: ' . count($glibcRanges) . ' ranges, index-gb18030-ranges has '
    . count($whatwgRanges) . ($rangeDiffs === [] ? ' (the same)' : sprintf(' (%d differences, listed below)', count($rangeDiffs)));
if ($rangeDiffs !== []) {
    $listings[] = ['`GB18030`\'s four-byte ranges and index-gb18030-ranges', $rangeDiffs];
}
foreach ($listings as [$title, $diffs]) {
    note($title . ': ' . count($diffs) . ' differences');
}
// ---------------------------------------------------------------------------------------------------------------
// The Rust tables

function wrap(string $text, string $indent): string
{
    return $indent . '/// ' . str_replace("\n", "\n$indent/// ", wordwrap($text, 116 - strlen($indent), "\n", true)) . "\n";
}

/** A byte string literal, wrapped. */
function rust_bytes(string $s, string $indent): string
{
    $lines = [];
    $line = '';
    for ($i = 0, $n = strlen($s); $i < $n; $i++) {
        $c = ord($s[$i]);
        $e = $c > 0x20 && $c < 0x7F && $c !== 0x22 && $c !== 0x5C ? chr($c) : sprintf('\x%02x', $c);
        if (strlen($line) + strlen($e) > 112 - strlen($indent)) {
            $lines[] = $line;
            $line = '';
        }
        $line .= $e;
    }
    $lines[] = $line;
    return 'b"' . implode("\\\n" . $indent, $lines) . '"';
}

function hex2(int $b): string
{
    return sprintf('0x%02X', $b);
}

function ranges_rust(array $ranges): string
{
    return '&[' . implode(', ', array_map(static fn (array $r): string => '(' . hex2($r[0]) . ', ' . hex2($r[1]) . ')', $ranges)) . ']';
}

function chunked(array $items, int $perLine, string $indent): string
{
    $out = '';
    foreach (array_chunk($items, $perLine) as $chunk) {
        $out .= $indent . implode(', ', $chunk) . ",\n";
    }
    return $out;
}

function const_name(string $s): string
{
    return strtoupper((string) preg_replace('/[^A-Za-z0-9]/', '_', $s));
}

// every plane gets an index; pairs of code points get indices too
$planeIndex = [];
$pairs = [];
$pairIndex = static function (array $cps) use (&$pairs): int {
    $key = implode(',', $cps);
    if (!isset($pairs[$key])) {
        $pairs[$key] = count($pairs);
    }
    return $pairs[$key];
};
foreach ($planes as $variant => $list) {
    foreach ($list as $name => [$plane]) {
        $planeIndex[$name] = count($planeIndex);
    }
}

$out = '';
$out .= "//! Generated by multibyte.gen.php (black-box: multibyte_probe.c and PHP iconv() in the oracle, compared with the\n";
$out .= "//! published mapping files); do not edit.\n";
$out .= "//!\n";
$out .= "//! The multi-byte charsets of `multibyte.rs` as glibc 2.41's iconv converts them in the oracle: every byte sequence\n";
$out .= "//! their structure allows decoded and every code point encoded. Each charset's documentation compares its sequences\n";
$out .= "//! with the published mapping files (glibc's mapping is the table); the differences are listed in\n";
$out .= "//! multibyte_differences.md.\n\n";
$out .= "use super::multibyte::{Family, Layout, Plane, Quirk, Spec, Step};\n\n";
$out .= "/// A multi-byte charset: glibc's name for it and how it compares with the published mappings.\n";
$out .= "#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]\n";
$out .= "pub(crate) enum Multibyte {\n";
foreach (CHARSETS as $variant => $cs) {
    $doc = "`$cs`: " . implode('; ', $docs[$variant]) . '.';
    $exceptions = $specs[$variant]['encode'];
    if ($exceptions !== []) {
        $items = [];
        foreach ($exceptions as $cp => $code) {
            $items[] = $code === REMOVE ? u($cp) . ' not at all' : sprintf('%s as %X', u($cp), $code);
        }
        $doc .= sprintf(' Encoding (the %s of several sequences of a character) also takes %s.',
            $specs[$variant]['encode_last'] ? 'last' : 'first', implode(', ', $items));
    }
    $out .= wrap($doc, '    ');
    $out .= "    $variant,\n";
}
$out .= "}\n\n";
$out .= sprintf("/// Every multi-byte charset, in the order of their discriminants.\npub(super) const ALL: [Multibyte; %d] = [\n", count(CHARSETS));
$out .= chunked(array_map(static fn (string $v): string => "Multibyte::$v", array_keys(CHARSETS)), 5, '    ');
$out .= "];\n\n";
$out .= "impl Multibyte {\n";
$out .= "    /// How the charset converts.\n";
$out .= "    pub(super) fn spec(self) -> &'static Spec {\n";
$out .= "        match self {\n";
foreach (CHARSETS as $variant => $cs) {
    $out .= sprintf("            Multibyte::%s => &%s_SPEC,\n", $variant, const_name($cs));
}
$out .= "        }\n    }\n\n";
$out .= "    /// glibc's name for the charset.\n";
$out .= "    #[cfg(test)]\n";
$out .= "    pub(super) fn name(self) -> &'static str {\n";
$out .= "        match self {\n";
foreach (CHARSETS as $variant => $cs) {
    $out .= sprintf("            Multibyte::%s => %s,\n", $variant, json_encode($cs));
}
$out .= "        }\n    }\n}\n";

foreach (CHARSETS as $variant => $cs) {
    $spec = $specs[$variant];
    $family = FAMILY[$variant];
    $planeIds = array_map(static fn (string $name): int => $planeIndex[$name], array_keys($planes[$variant]));
    $out .= sprintf("\n/// `%s`.\nstatic %s_SPEC: Spec = Spec {\n", $cs, const_name($cs));
    switch ($family) {
        case 'dbcs':
            $out .= "    family: Family::Dbcs,\n";
            break;
        case 'eucjp':
            $out .= sprintf("    family: Family::EucJp { plane: %d, wait: (%s, %s), skip3: %d },\n", $planeIds[1],
                hex2($spec['wait'][0]), hex2($spec['wait'][1]), $spec['skip3']);
            break;
        case 'euctw':
            $items = [];
            foreach (array_keys($planes[$variant]) as $k => $name) {
                if ($k > 0) {
                    $items[] = sprintf('(%s, %d)', hex2(hexdec(substr($planes[$variant][$name][0]->prefix, 2))), $planeIndex[$name]);
                }
            }
            $out .= sprintf("    family: Family::EucTw { planes: &[%s] },\n", implode(', ', $items));
            break;
        case 'gb18030':
            $out .= "    family: Family::Gb18030,\n";
            break;
    }
    $out .= '    single: &[' . implode(', ', array_map(static fn (array $r): string => sprintf('(%s, %s, 0x%04X)', hex2($r[0]), hex2($r[1]), $r[2]), $spec['single'])) . "],\n";
    $bits = array_fill(0, 8, 0);
    foreach ($spec['leads'] as $l) {
        $bits[$l >> 5] |= 1 << ($l & 31);
    }
    $out .= '    lead: [' . implode(', ', array_map(static fn (int $w): string => sprintf('0x%08X', $w), $bits)) . "],\n";
    $out .= sprintf("    plane: %d,\n", $planeIds[0]);
    $out .= sprintf("    silent: %s,\n", $spec['silent'] ? 'true' : 'false');
    $quirk = $spec['quirk'];
    $out .= '    quirk: ' . match ($quirk[0] ?? null) {
        null => 'Quirk::None',
        'skip4' => sprintf('Quirk::IgnoreSkip4(%s, %s)', hex2($quirk[1] >> 8), hex2($quirk[1] & 0xFF)),
        'fffd' => sprintf('Quirk::IgnoreReplacement { lead: %s, trails: (%s, %s), cp: 0x%04X }', hex2($quirk[1]), hex2($quirk[2]), hex2($quirk[3]), $quirk[4]),
    } . ",\n";
    $encodePlanes = [];
    foreach (encode_planes($variant, $planes) as [$plane, $prefix]) {
        $encodePlanes[] = sprintf('(%d, 0x%X)', $planeIndex[$plane->name], $prefix === '' ? 0 : hexdec($prefix));
    }
    $out .= '    encode_planes: &[' . implode(', ', $encodePlanes) . "],\n";
    $out .= sprintf("    encode_last: %s,\n", $spec['encode_last'] ? 'true' : 'false');
    $items = [];
    foreach ($spec['encode'] as $cp => $code) {
        $items[] = sprintf('(0x%04X, %s)', $cp, $code === REMOVE ? 'NONE' : sprintf('0x%X', $code));
    }
    $out .= '    encode: &[' . implode(', ', $items) . "],\n";
    $out .= "};\n";
}

$usesNone = false;
foreach ($specs as $spec) {
    $usesNone = $usesNone || in_array(REMOVE, $spec['encode'], true);
}
if ($usesNone) {
    $out .= "\n/// A code the encoder does not write (in `Spec::encode`).\nconst NONE: u32 = u32::MAX;\n";
}
$out .= sprintf("\n/// The number of planes.\npub(super) const PLANE_COUNT: usize = %d;\n", count($planeIndex));
$out .= "\n/// The planes (`PLANES[i]` is plane `i`).\npub(super) static PLANES: [Plane; PLANE_COUNT] = [\n";
foreach ($planes as $variant => $list) {
    foreach ($list as $name => [$plane]) {
        $out .= "    // $name\n";
        $out .= "    Plane {\n";
        $out .= '        leads: ' . ranges_rust($plane->leads) . ",\n";
        $out .= '        trails: ' . ranges_rust($plane->trails) . ",\n";
        $out .= "        steps: &[\n";
        foreach ($plane->steps as $step) {
            switch ($step[0]) {
                case 'set':
                    [$rows, $cols] = $sets[$step[1]];
                    $layout = match ($step[2][0]) {
                        'offset' => sprintf('Layout::Offset(%s, %s)', hex2($step[2][1]), hex2($step[2][2])),
                        'sjis' => 'Layout::Sjis',
                        'johab' => 'Layout::Johab',
                    };
                    $out .= sprintf("            Step::Set { stream: &%s, rows: %d, cols: %d, layout: %s },\n", $step[1], $rows, $cols, $layout);
                    break;
                case 'holes':
                    $out .= sprintf("            Step::Holes { stream: &%s, leads: (%s, %s), trails: %s },\n", const_name($step[1]),
                        hex2($step[2][0]), hex2($step[2][1]), ranges_rust($step[3]));
                    break;
                case 'hangul':
                    $out .= sprintf("            Step::Hangul { leads: (%s, %s), trails: %s },\n", hex2($step[1][0]), hex2($step[1][1]), ranges_rust($step[2]));
                    break;
                case 'patch':
                    if ($step[1] === []) {
                        break;
                    }
                    $items = [];
                    foreach ($step[1] as $key => $v) {
                        $items[] = sprintf('(0x%04X, %s)', $key, $v === null ? '0' : (is_array($v) ? ($pairIndex($v) === 0 ? 'PAIR' : sprintf('PAIR | %d', $pairIndex($v))) : sprintf('0x%04X', $v)));
                    }
                    $out .= "            Step::Patch(&[\n";
                    $out .= chunked($items, 6, '                ');
                    $out .= "            ]),\n";
                    break;
            }
        }
        $out .= "        ],\n";
        // windows of the main plane only (the longer sequences follow their family's rule)
        $windows = $plane->prefix === '' ? ($specs[$variant]['windows'] ?? []) : [];
        $distinct = [];
        $of = [];
        foreach (range_bytes($plane->leads) as $l) {
            $w = $windows[$l] ?? [];
            $key = json_encode($w);
            if (!isset($distinct[$key])) {
                $distinct[$key] = [count($distinct), $w];
            }
            $of[] = $distinct[$key][0];
        }
        if (count($distinct) === 1) {
            $of = [];
        }
        $out .= '        window_of: &[' . implode(', ', $of) . "],\n";
        $out .= '        windows: &[' . implode(', ', array_map(static fn (array $d): string => ranges_rust($d[1]), $distinct)) . "],\n";
        $out .= "    },\n";
    }
}
$out .= "];\n";

$out .= "\n/// A cell that decodes to a pair of code points (`PAIRS[cell & !PAIR]`).\npub(super) const PAIR: u32 = 0x8000_0000;\n";
$out .= sprintf("\n/// The pairs of code points of cells marked `PAIR`.\npub(super) static PAIRS: [(u32, u32); %d] = [\n", count($pairs));
$out .= chunked(array_map(static fn (string $k): string => vsprintf('(0x%04X, 0x%04X)', explode(',', $k)), array_keys($pairs)), 6, '    ');
$out .= "];\n";

$ranges = $specs['Gb18030']['ranges'];
$out .= sprintf("\n/// GB18030's four-byte sequences of BMP code points: (linear index of the first sequence, its code point) of each\n/// range of consecutive code points, sorted by index (not by code point: glibc swaps a few code points with the\n/// two-byte table).\npub(super) static GB18030_RANGES: [(u16, u16); %d] = [\n", count($ranges));
$out .= chunked(array_map(static fn (array $r): string => sprintf('(%d, 0x%04X)', $r[0], $r[1]), $ranges), 8, '    ');
$out .= "];\n";
$out .= sprintf("\n/// The last linear index of GB18030's four-byte BMP sequences.\npub(super) const GB18030_BMP_END: u32 = %d;\n", $ranges[array_key_last($ranges)][2]);

foreach ($streams as $name => $data) {
    $setDoc = isset($sets[$name]) ? sprintf('The coded character set %s (%d x %d cells).', $name, $sets[$name][0], $sets[$name][1])
        : sprintf('The cells of %s that the steps before have left empty.', $name);
    $out .= sprintf("\n/// %s\nstatic %s: [u8; %d] = *%s;\n", $setDoc, const_name($name), strlen($data), rust_bytes($data, ''));
}

echo $out;

$md = "<!-- Generated by multibyte.gen.php; do not edit. -->\n\n";
$md .= "# The multi-byte charsets and the published mapping files\n\n";
$md .= "Every difference between glibc 2.41's iconv in the oracle (the tables of `multibyte_table.rs`) and the published\n";
$md .= "mapping files the generator compares them with, merged into runs where codes and code points advance together.\n";
$md .= "Codes are the bytes in hexadecimal; \"none\": that side does not decode the code.\n";
foreach ($listings as [$title, $diffs]) {
    $md .= "\n## " . str_replace('`', '', $title) . "\n\n";
    foreach ($diffs as $d) {
        $md .= "- $d\n";
    }
}
file_put_contents(__DIR__ . '/multibyte_differences.md', $md);
