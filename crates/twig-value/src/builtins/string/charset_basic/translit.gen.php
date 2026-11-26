<?php
// Generates translit_table.rs: what //TRANSLIT turns a character into when the target charset cannot represent it.
//
// Black-box only: every code point is converted with PHP's iconv() in the oracle (glibc 2.41, LC_CTYPE=C.UTF-8,
// the locale PHP runs with there), one character between \x01 separators per slot of a batch, for every target with
// a smaller repertoire than Unicode: ANSI_X3.4-1968, ISO-8859-1, UCS-2 (U+10000 and up) and the codepages of
// codepages.inc.php. Characters that come out as "?" (glibc's replacement when it knows no transliteration) and the
// tag characters (which vanish) are left out. Pairs of characters were checked to transliterate like the two
// characters on their own (no multi-character rules in this locale).
//
// The targets disagree where a character has several alternatives (U+339F is "mm²" where the target has "²", else
// "mm^2"): the table lists, for each character, the replacements seen in the targets in an order under which taking
// the first one the target can represent gives every target's result; the generator checks that it does, byte for
// byte. The tests in tests/charset_basic.rs compare the table with oracle vectors.
//
// Regenerate (from crates/twig-value/src/builtins/string/charset_basic, Git Bash):
//   MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//       php /t/translit.gen.php > translit_table.rs

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

$targets = ['ANSI_X3.4-1968', 'ISO-8859-1', 'UCS-2LE'];
foreach (CODEPAGES as [$cs]) {
    $targets[] = $cs;
}

/** What each BMP code point encodes to in $target, null when it cannot be represented (nothing above the BMP is). */
function repertoire(string $target): array
{
    $enc = [];
    for ($cp = 0; $cp < 0x10000; $cp++) {
        $enc[$cp] = $cp >= 0xD800 && $cp <= 0xDFFF ? null : encode_cp($target, $cp);
    }
    return $enc;
}

/** The output of every character that $target cannot represent and does not turn into "?". */
function measure(string $target, array $enc): array
{
    $unit = $target === 'UCS-2LE' ? 2 : 1;
    $sep = $unit === 2 ? "\x01\x00" : "\x01";
    $out = [];
    for ($start = 0x80; $start <= 0x10FFFF; $start += 4096) {
        $cps = [];
        $in = '';
        for ($cp = $start; $cp < $start + 4096 && $cp <= 0x10FFFF; $cp++) {
            if (($cp >= 0xD800 && $cp <= 0xDFFF) || ($cp >= 0xE0000 && $cp <= 0xE007F)) {
                continue;
            }
            $cps[] = $cp;
            $in .= mb_chr($cp, 'UTF-8') . "\x01";
        }
        $converted = iconv('UTF-8', "$target//TRANSLIT", $in);
        if ($converted === false) {
            fail("batch $target $start failed");
        }
        $parts = [];
        $cur = '';
        for ($i = 0; $i < strlen($converted); $i += $unit) {
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
            if ($direct !== null) {
                if ($parts[$k] !== $direct) {
                    fail(sprintf('%s: U+%04X is %s alone, %s in a batch', $target, $cp, bin2hex($direct), bin2hex($parts[$k])));
                }
            } elseif ($parts[$k] !== ($unit === 2 ? "?\x00" : '?')) {
                $out[$cp] = $parts[$k];
            }
        }
    }
    return $out;
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
    $multi = false;
    foreach ($enc[$target] as $b) {
        $multi = $multi || ($b !== null && strlen($b) > 1 && $target !== 'UCS-2LE');
    }
    foreach ($measured[$target] as $cp => $bytes) {
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
    $results[$cp][$target] = $match ?? array_map(static fn (string $b): int => $decode[$target][$b], str_split($bytes));
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
        if ($cp < 0x10000 && $enc[$target][$cp] !== null) {
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
        if (($cp < 0x10000 && $enc[$target][$cp] !== null) || ($cp >= 0xD800 && $cp <= 0xDFFF)
            || ($cp >= 0xE0000 && $cp <= 0xE007F)) {
            continue;
        }
        $got = null;
        foreach ($table[$cp] ?? [] as $r) {
            $got = encode_all($enc[$target], $r);
            if ($got !== null) {
                break;
            }
        }
        if ($got !== ($measured[$target][$cp] ?? null)) {
            fail(sprintf('%s: U+%04X would be %s instead of %s', $target, $cp, bin2hex((string) $got), bin2hex($measured[$target][$cp] ?? '?')));
        }
    }
}

$keys = [];
$spans = [];
$data = [];
$index = [];
foreach ($table as $cp => $order) {
    foreach ($order as $r) {
        foreach ($r as $c) {
            if ($c > 0xFFFF) {
                fail(sprintf('U+%04X: a replacement beyond the BMP', $cp));
            }
        }
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

echo "//! Generated by translit.gen.php (black-box: PHP iconv() with //TRANSLIT in the oracle); do not edit.\n";
echo "//!\n";
echo "//! glibc 2.41 transliteration under LC_CTYPE=C.UTF-8, measured for every target with a smaller repertoire than\n";
echo "//! Unicode: the alternatives of each character in the order in which glibc tries them, as far as the targets show\n";
echo "//! it. Characters that are not listed become \"?\".\n\n";
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
printf("/// The alternatives, concatenated.\npub(super) static DATA: [u16; %d] = [\n", count($data));
foreach (array_chunk($data, 12) as $chunk) {
    echo '    ', implode(', ', array_map(static fn (int $c): string => sprintf('0x%04X', $c), $chunk)), ",\n";
}
echo "];\n";
