<?php
// Generates codepage_table.rs: the tables of the single-byte charsets in codepages.inc.php, as glibc's iconv converts
// them in the oracle, and how they compare with the Unicode Consortium's mapping files.
//
// Black-box only: every byte is decoded and every BMP code point encoded with one PHP iconv() call each, the astral
// planes are checked to hold nothing the charset can represent, and the compositions of the decoder are found by
// decoding every sequence that extends a character the decoder may compose (breadth first: a byte, then every byte
// after it, then every byte after each sequence that gave one character). Which characters a combining decoder holds
// back is seen through the public iconv() interface without PHP, whose iconv() always flushes: a small C program
// (compiled in the oracle) converts each byte and each composing sequence with one iconv() call and reports whether
// the call wrote nothing. The tables are what the oracle shows; the unicode.org files (fetched from
// https://www.unicode.org/Public/MAPPINGS/) only serve the comparison written into the documentation of each charset.
//
// Regenerate (from crates/twig-value/src/builtins/string/charset_basic, Git Bash; needs network access):
//   MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//       php /t/codepage.gen.php > codepage_table.rs

require __DIR__ . '/codepages.inc.php';
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

function u(int $cp): string
{
    return sprintf('U+%04X', $cp);
}

/**
 * Whether one iconv() call (no flush) from $cs into UCS-4BE consumes each of $inputs and writes nothing: the decoder
 * holds the character back.
 */
function holds_back(string $cs, array $inputs): array
{
    static $binary = null;
    if ($binary === null) {
        $binary = sys_get_temp_dir() . '/codepage_holds';
        file_put_contents("$binary.c", <<<'C'
            #include <iconv.h>
            #include <locale.h>
            #include <stdio.h>
            #include <string.h>
            /* stdin: "CHARSET HEXINPUT" per line; stdout: 1 when one iconv() call consumed the input and wrote nothing */
            int main(void) {
                char line[4096], cs[256], hex[2048];
                setlocale(LC_CTYPE, "C.UTF-8");
                while (fgets(line, sizeof line, stdin) && sscanf(line, "%255s %2047s", cs, hex) == 2) {
                    unsigned char in[1024], out[4096];
                    size_t n = 0;
                    for (const char *h = hex; h[0] && h[1]; h += 2) {
                        unsigned v;
                        sscanf(h, "%2x", &v);
                        in[n++] = (unsigned char)v;
                    }
                    iconv_t cd = iconv_open("UCS-4BE", cs);
                    char *ip = (char *)in, *op = (char *)out;
                    size_t il = n, ol = sizeof out;
                    size_t r = iconv(cd, &ip, &il, &op, &ol);
                    printf("%d\n", r != (size_t)-1 && il == 0 && op == (char *)out);
                    iconv_close(cd);
                }
                return 0;
            }
            C);
        exec("gcc -O1 -o $binary $binary.c", $ignored, $status);
        if ($status !== 0) {
            fail('cannot compile the iconv() helper');
        }
    }
    $lines = '';
    foreach ($inputs as $input) {
        $lines .= "$cs " . bin2hex($input) . "\n";
    }
    file_put_contents("$binary.in", $lines);
    $out = explode("\n", trim((string) shell_exec("$binary < $binary.in")));
    if (count($out) !== count($inputs)) {
        fail("$cs: the iconv() helper failed");
    }
    return array_map(static fn (string $r): bool => $r === '1', $out);
}

/** A unicode.org mapping file: byte => code points (a list: a few files map a byte to a sequence), or null. */
function unicode_file(string $path): array
{
    $text = file_get_contents("https://www.unicode.org/Public/MAPPINGS/$path");
    if ($text === false) {
        fail("cannot fetch $path");
    }
    $map = array_fill(0, 256, null);
    foreach (explode("\n", $text) as $line) {
        $line = trim(explode('#', $line, 2)[0]);
        if ($line === '') {
            continue;
        }
        $cols = preg_split('/\s+/', $line);
        if (count($cols) < 2) {
            continue; // an undefined byte
        }
        $map[hexdec($cols[0])] = array_map('hexdec', explode('+', $cols[1]));
    }
    return $map;
}

/** Runs of differences, merged where bytes and code points advance together: "0x00..=0x1F: U+0000..=U+001F". */
function describe_differences(array $glibc, array $file): array
{
    $show = static fn (?array $cps): string => $cps === null ? 'none' : implode(' ', array_map('u', $cps));
    $runs = [];
    for ($b = 0; $b < 256; $b++) {
        if ($glibc[$b] === $file[$b]) {
            continue;
        }
        $last = count($runs) - 1;
        if ($last >= 0) {
            [$start, $end] = $runs[$last];
            $step = $b - $start;
            $advances = static fn (?array $now, ?array $first): bool => $now === null ? $first === null
                : ($first !== null && count($now) === 1 && count($first) === 1 && $now[0] === $first[0] + $step);
            if ($end === $b - 1 && $advances($glibc[$b], $glibc[$start]) && $advances($file[$b], $file[$start])) {
                $runs[$last][1] = $b;
                continue;
            }
        }
        $runs[] = [$b, $b];
    }
    $out = [];
    foreach ($runs as [$start, $end]) {
        $range = static function (?array $first, ?array $last) use ($show): string {
            if ($first === null || $first === $last) {
                return $show($first);
            }
            return $show($first) . '..=' . $show($last);
        };
        $bytes = $start === $end ? sprintf('0x%02X', $start) : sprintf('0x%02X..=0x%02X', $start, $end);
        $out[] = sprintf('%s is %s (the file: %s)', $bytes, $range($glibc[$start], $glibc[$end]),
            $range($file[$start], $file[$end]));
    }
    return $out;
}

$tables = [];
foreach (CODEPAGES as $variant => [$cs, $file]) {
    // decoding: one byte per call
    $decoded = [];
    $high = [];
    for ($b = 0; $b < 256; $b++) {
        $cps = decode_byte($cs, $b);
        $decoded[$b] = $cps;
        if ($b < 0x80) {
            if ($cps !== [$b]) {
                fail("$cs: byte $b is not ASCII");
            }
        } elseif ($cps === null) {
            $high[] = null;
        } elseif (count($cps) === 1 && $cps[0] >= 0x80 && $cps[0] < 0xFFFF) {
            $high[] = $cps[0];
        } else {
            fail("$cs: byte $b decodes to " . json_encode($cps));
        }
    }

    // encoding: one BMP code point per call
    $single = [];
    $multi = [];
    for ($cp = 0; $cp < 0x10000; $cp++) {
        if ($cp >= 0xD800 && $cp <= 0xDFFF) {
            continue;
        }
        $r = encode_cp($cs, $cp);
        if ($cp < 0x80) {
            if ($r !== chr($cp)) {
                fail("$cs: " . u($cp) . ' is not ASCII');
            }
        } elseif ($r === null) {
            continue;
        } elseif (strlen($r) === 1) {
            $single[$cp] = ord($r);
        } elseif (strlen($r) <= 3 && !str_contains($r, "\x00")) {
            $multi[$cp] = $r;
        } else {
            fail("$cs: " . u($cp) . ' encodes to ' . bin2hex($r));
        }
    }
    // the astral planes hold nothing (the tag characters vanish, as for every charset)
    for ($start = 0x10000; $start < 0x110000; $start += 4096) {
        $in = '';
        for ($cp = $start; $cp < $start + 4096; $cp++) {
            $in .= pack('N', $cp);
        }
        if (iconv('UCS-4BE', "$cs//IGNORE", $in) !== '') {
            fail("$cs: something from " . u($start) . ' up is representable');
        }
    }
    // single-byte encodings and decodings are each other's inverse, but for a few more code points the encoder takes
    $oneWay = [];
    foreach ($single as $cp => $b) {
        if ($high[$b - 0x80] !== $cp) {
            if ($high[$b - 0x80] === null) {
                fail("$cs: " . u($cp) . " encodes to byte $b, which does not decode");
            }
            $oneWay[] = sprintf('%s as 0x%02X (%s)', u($cp), $b, u($high[$b - 0x80]));
        }
    }
    foreach ($high as $i => $cp) {
        if ($cp !== null && ($single[$cp] ?? null) !== 0x80 + $i) {
            fail("$cs: byte " . (0x80 + $i) . ' does not round-trip');
        }
    }

    // compositions: extend every sequence that decodes to one character by every byte
    $compose = [];
    $queue = [];
    for ($b = 0; $b < 256; $b++) {
        if ($decoded[$b] !== null) {
            $queue[] = [chr($b), $decoded[$b][0]];
        }
    }
    $seen = [];
    while ($queue !== []) {
        [$s, $x] = array_shift($queue);
        for ($b = 0; $b < 256; $b++) {
            if ($decoded[$b] === null) {
                continue;
            }
            $c = $decoded[$b][0];
            $r = iconv($cs, 'UCS-4BE', $s . chr($b));
            $cps = $r === false ? null : array_values(unpack('N*', $r) ?: []);
            if ($cps === [$x, $c]) {
                continue;
            }
            if ($cps === null || count($cps) !== 1) {
                fail("$cs: " . bin2hex($s . chr($b)) . ' decodes to ' . json_encode($cps));
            }
            $key = sprintf('%04X%04X', $x, $c);
            if (isset($compose[$key]) && $compose[$key][2] !== $cps[0]) {
                fail("$cs: " . u($x) . ' + ' . u($c) . ' composes differently');
            }
            $compose[$key] = [$x, $c, $cps[0]];
            if (!isset($seen[$s . chr($b)])) {
                $seen[$s . chr($b)] = true;
                $queue[] = [$s . chr($b), $cps[0]];
            }
        }
    }
    ksort($compose, SORT_STRING);

    // which characters a combining decoder holds back: those of a set of bytes (recorded as ranges of their code
    // points) and, of the combined characters, exactly those that combine further
    $hold = [];
    $holdCombined = [];
    if ($compose !== []) {
        $valid = array_keys(array_filter($decoded, static fn (?array $cps): bool => $cps !== null));
        $held = array_combine($valid, holds_back($cs, array_map('chr', $valid)));
        $byCp = [];
        foreach ($held as $b => $h) {
            $byCp[$decoded[$b][0]] = $h;
        }
        ksort($byCp);
        // a range runs as long as no character of the charset in between is written at once
        $open = false;
        foreach ($byCp as $cp => $h) {
            if (!$h) {
                $open = false;
            } elseif ($open) {
                $hold[count($hold) - 1][1] = $cp;
            } else {
                $hold[] = [$cp, $cp];
                $open = true;
            }
        }
        $sequences = array_map('strval', array_keys($seen));
        $combinedHeld = [];
        foreach (holds_back($cs, $sequences) as $k => $h) {
            $x = unpack('N', iconv($cs, 'UCS-4BE', $sequences[$k]))[1];
            if (isset($combinedHeld[$x]) && $combinedHeld[$x] !== $h) {
                fail("$cs: " . u($x) . ' is held back after some sequences only');
            }
            $combinedHeld[$x] = $h;
        }
        $holdCombined = array_keys(array_filter($combinedHeld));
        sort($holdCombined);
    }

    // the comparison with the published mapping
    $glibc = [];
    for ($b = 0; $b < 256; $b++) {
        $glibc[$b] = $b < 0x80 ? [$b] : ($high[$b - 0x80] === null ? null : [$high[$b - 0x80]]);
    }
    $doc = "`$cs`: ";
    if ($file !== null) {
        $diffs = describe_differences($glibc, unicode_file($file));
        $doc .= "unicode.org $file" . ($diffs === [] ? ', the same mapping' : ', except: ' . implode('; ', $diffs)) . '.';
    } else {
        // KOI8-RU: the closest published mapping
        $diffs = describe_differences($glibc, unicode_file('VENDORS/MISC/KOI8-U.TXT'));
        $doc .= 'no unicode.org mapping file; unicode.org VENDORS/MISC/KOI8-U.TXT (KOI8-U), except: ' . implode('; ', $diffs) . '.';
    }
    if ($compose !== [] || $multi !== []) {
        $ranges = implode(', ', array_map(static fn (array $r): string => u($r[0]) . '..=' . u($r[1]), $hold));
        $combined = $holdCombined === [] ? 'no combined character' : implode(', ', array_map('u', $holdCombined));
        $doc .= sprintf(' Not in the file: decoding composes %d pairs of characters (holding back %s, and %s) and encoding'
            . ' writes %d code points as two or three bytes (a letter and its marks).', count($compose), $ranges,
            $combined, count($multi));
    }
    if ($oneWay !== []) {
        $doc .= ' Encoding also takes ' . implode(', ', $oneWay) . '.';
    }
    $tables[$variant] = [$cs, $doc, $high, $single, $multi, $compose, $hold, $holdCombined];
}

function wrap(string $text, string $indent): string
{
    return $indent . '/// ' . str_replace("\n", "\n$indent/// ", wordwrap($text, 116 - strlen($indent), "\n", true)) . "\n";
}

function static_name(string $cs): string
{
    return strtoupper(str_replace('-', '_', $cs));
}

echo "//! Generated by codepage.gen.php (black-box: PHP iconv() in the oracle, compared with the Unicode Consortium's\n";
echo "//! mapping files); do not edit.\n";
echo "//!\n";
echo "//! The single-byte charsets of `codepage.rs` as glibc 2.41's iconv converts them in the oracle: every byte decoded\n";
echo "//! and every code point encoded with one iconv() call each, and the compositions of the decoders found by decoding\n";
echo "//! every sequence that extends a character. Bytes and code points below 0x80 are ASCII in all of them. Each\n";
echo "//! charset's documentation compares its bytes with the unicode.org mapping file (glibc's mapping is the table).\n\n";
echo "use super::codepage::{INVALID, Table};\n\n";
echo "/// A single-byte charset converted with tables: glibc's name for it and how it compares with the published mapping.\n";
echo "#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]\n";
echo "pub(crate) enum Codepage {\n";
foreach ($tables as $variant => [$cs, $doc]) {
    echo wrap($doc, '    ');
    echo "    $variant,\n";
}
echo "}\n\n";
printf("/// Every codepage.\n#[cfg(test)]\npub(super) const ALL: [Codepage; %d] = [\n", count($tables));
foreach (array_chunk(array_keys($tables), 5) as $chunk) {
    echo '    ', implode(', ', array_map(static fn (string $v): string => "Codepage::$v", $chunk)), ",\n";
}
echo "];\n\n";
echo "impl Codepage {\n";
echo "    /// The charset's tables.\n";
echo "    pub(super) fn table(self) -> &'static Table {\n";
echo "        match self {\n";
foreach ($tables as $variant => [$cs]) {
    printf("            Codepage::%s => &%s,\n", $variant, static_name($cs));
}
echo "        }\n";
echo "    }\n\n";
echo "    /// glibc's name for the charset.\n";
echo "    #[cfg(test)]\n";
echo "    pub(super) fn name(self) -> &'static str {\n";
echo "        match self {\n";
foreach ($tables as $variant => [$cs]) {
    printf("            Codepage::%s => %s,\n", $variant, json_encode($cs));
}
echo "        }\n";
echo "    }\n";
echo "}\n";

foreach ($tables as $variant => [$cs, $doc, $high, $single, $multi, $compose, $hold, $holdCombined]) {
    printf("\n/// `%s`.\nstatic %s: Table = Table {\n", $cs, static_name($cs));
    echo "    high: [\n";
    foreach (array_chunk($high, 8) as $chunk) {
        echo '        ', implode(', ', array_map(static fn (?int $c): string => $c === null ? 'INVALID' : sprintf('0x%04X', $c), $chunk)), ",\n";
    }
    echo "    ],\n";
    echo "    encode: &[\n";
    ksort($single);
    $pairs = [];
    foreach ($single as $cp => $b) {
        $pairs[] = sprintf('(0x%04X, 0x%02X)', $cp, $b);
    }
    foreach (array_chunk($pairs, 6) as $chunk) {
        echo '        ', implode(', ', $chunk), ",\n";
    }
    echo "    ],\n";
    ksort($multi);
    if ($multi === []) {
        echo "    decompose: &[],\n";
    } else {
        echo "    decompose: &[\n";
        $items = [];
        foreach ($multi as $cp => $bytes) {
            $b = array_pad(array_map('ord', str_split($bytes)), 3, 0);
            $items[] = sprintf('(0x%04X, [0x%02X, 0x%02X, 0x%02X])', $cp, ...$b);
        }
        foreach (array_chunk($items, 3) as $chunk) {
            echo '        ', implode(', ', $chunk), ",\n";
        }
        echo "    ],\n";
    }
    if ($compose === []) {
        echo "    compose: &[],\n";
    } else {
        echo "    compose: &[\n";
        $items = array_map(static fn (array $t): string => vsprintf('(0x%04X, 0x%04X, 0x%04X)', $t), array_values($compose));
        foreach (array_chunk($items, 4) as $chunk) {
            echo '        ', implode(', ', $chunk), ",\n";
        }
        echo "    ],\n";
    }
    printf("    hold: &[%s],\n", implode(', ', array_map(static fn (array $r): string => vsprintf('(0x%04X, 0x%04X)', $r), $hold)));
    printf("    hold_combined: &[%s],\n", implode(', ', array_map(static fn (int $c): string => sprintf('0x%04X', $c), $holdCombined)));
    echo "};\n";
}
