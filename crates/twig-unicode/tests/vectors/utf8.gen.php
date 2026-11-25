<?php
/*
 * Oracle generator for utf8.json (twig-rs, crates/twig-unicode, PHP 8.5.13 mbstring).
 *
 * How mbstring's UTF-8 decoding treats every byte string in three input sets:
 *
 * - "exhaustive": every string of 0 to 3 bytes over a 21-byte alphabet with a byte on each
 *   side of every boundary in UTF-8's well-formedness table (Unicode Table 3-7);
 * - "exhaustive4": every 4-byte string over an 11-byte alphabet focused on 4-byte sequences;
 * - "random": 3000 pseudo-random strings of 1 to 24 bytes (mt_srand(20261125)), never
 *   containing "?" so that substitutions stay recognisable.
 *
 * Exhaustive sets list their alphabet and lengths instead of inputs; inputs are enumerated by
 * ascending length, then lexicographically by alphabet position (first byte most significant).
 * Per input, in input order:
 *   "substr": bin2hex(mb_substr($s, 0)), i.e. the decoded string re-encoded with "?" for every
 *             ill-formed sequence;
 *   "strlen": mb_strlen($s);
 *   "check":  mb_check_encoding($s, 'UTF-8') as a string of 0/1;
 *   "split":  the byte lengths of the pieces of mb_str_split($s), as one digit each;
 *   "ord":    mb_ord($s), -1 for false, null for the empty string (ValueError);
 *   "case":   for MB_CASE_UPPER/LOWER/TITLE/FOLD, the inputs (by index) whose conversion is not
 *             equal to "substr", with bin2hex() of the conversion.
 *
 * "corruptions" checks longer strings, where mb_check_encoding() runs PHP's SIMD UTF-8
 * validation in 16/32-byte blocks: for each base string and each byte offset $p from 0 to its
 * length, each corruption kind is applied at $p (truncate: substr($b, 0, $p); replace_ff: byte
 * $p replaced by FF, only for $p < length; insert_*: the bytes inserted before byte $p), in the
 * order base string, kind, offset. Per resulting string: "check" (0/1) and "strlen".
 *
 * Regenerate from the repository root with the oracle image (research/php-oracle):
 *   docker run --rm -i twig-oracle:3.31.1 php < crates/twig-unicode/tests/vectors/utf8.gen.php \
 *       > crates/twig-unicode/tests/vectors/utf8.json
 */

ini_set('memory_limit', '-1');
error_reporting(E_ALL);
set_error_handler(function ($no, $str) { fwrite(STDERR, "PHP error: $str\n"); exit(1); });

function enumerate(array $alphabet, array $lengths): array
{
    $inputs = [];
    foreach ($lengths as $len) {
        $strings = [''];
        for ($i = 0; $i < $len; $i++) {
            $next = [];
            foreach ($strings as $prefix) {
                foreach ($alphabet as $b) {
                    $next[] = $prefix . chr($b);
                }
            }
            $strings = $next;
        }
        array_push($inputs, ...$strings);
    }
    return $inputs;
}

function describe(array $inputs): array
{
    $modes = ['upper' => MB_CASE_UPPER, 'lower' => MB_CASE_LOWER, 'title' => MB_CASE_TITLE, 'fold' => MB_CASE_FOLD];
    $r = ['substr' => [], 'strlen' => [], 'check' => '', 'split' => [], 'ord' => [], 'case' => []];
    foreach ($modes as $name => $mode) {
        $r['case'][$name] = new stdClass();
    }
    foreach ($inputs as $i => $s) {
        $sub = mb_substr($s, 0, null, 'UTF-8');
        $r['substr'][] = bin2hex($sub);
        $r['strlen'][] = mb_strlen($s, 'UTF-8');
        $r['check'] .= mb_check_encoding($s, 'UTF-8') ? '1' : '0';
        $r['split'][] = implode('', array_map('strlen', mb_str_split($s, 1, 'UTF-8')));
        if ($s === '') {
            $r['ord'][] = null;
        } else {
            $ord = mb_ord($s, 'UTF-8');
            $r['ord'][] = $ord === false ? -1 : $ord;
        }
        foreach ($modes as $name => $mode) {
            $c = mb_convert_case($s, $mode, 'UTF-8');
            if ($c !== $sub) {
                $r['case'][$name]->{$i} = bin2hex($c);
            }
        }
        if (mb_strtoupper($s, 'UTF-8') !== mb_convert_case($s, MB_CASE_UPPER, 'UTF-8')
            || mb_strtolower($s, 'UTF-8') !== mb_convert_case($s, MB_CASE_LOWER, 'UTF-8')) {
            fwrite(STDERR, 'mb_strtoupper/mb_strtolower disagree with mb_convert_case for ' . bin2hex($s) . "\n");
            exit(1);
        }
    }
    return $r;
}

$alphabet3 = [0x30, 0x7F, 0x80, 0x8F, 0x90, 0x9F, 0xA0, 0xBF, 0xC0, 0xC1, 0xC2, 0xDF, 0xE0, 0xE1, 0xED, 0xEF, 0xF0, 0xF1, 0xF4, 0xF5, 0xFF];
$alphabet4 = [0x30, 0x80, 0x8F, 0x90, 0xBF, 0xC2, 0xE0, 0xF0, 0xF1, 0xF4, 0xFF];

mt_srand(20261125);
$random = [];
for ($n = 0; $n < 3000; $n++) {
    $len = mt_rand(1, 24);
    $s = '';
    while (strlen($s) < $len) {
        $b = mt_rand(0, 1) ? $alphabet3[mt_rand(0, count($alphabet3) - 1)] : mt_rand(0, 255);
        if ($b !== 0x3F) {
            $s .= chr($b);
        }
    }
    $random[] = $s;
}

$sets = [
    'exhaustive' => ['alphabet' => $alphabet3, 'lengths' => [0, 1, 2, 3]]
        + describe(enumerate($alphabet3, [0, 1, 2, 3])),
    'exhaustive4' => ['alphabet' => $alphabet4, 'lengths' => [4]]
        + describe(enumerate($alphabet4, [4])),
    'random' => ['inputs' => array_map('bin2hex', $random)] + describe($random),
];

$bases = [str_repeat("aé€😀", 7), str_repeat('x', 40) . '日本語' . str_repeat('y', 20), str_repeat('😀', 20)];
$kinds = [
    'truncate' => null,
    'replace_ff' => "\xFF",
    'insert_80' => "\x80",
    'insert_c0af' => "\xC0\xAF",
    'insert_surrogate' => "\xED\xA0\x80",
    'insert_f490' => "\xF4\x90\x80\x80",
    'insert_e282' => "\xE2\x82",
];
$corruptions = ['bases' => array_map('bin2hex', $bases), 'kinds' => array_keys($kinds), 'check' => '', 'strlen' => []];
foreach ($bases as $b) {
    foreach ($kinds as $kind => $bytes) {
        for ($p = 0; $p <= strlen($b); $p++) {
            if ($kind === 'truncate') {
                $s = substr($b, 0, $p);
            } elseif ($kind === 'replace_ff') {
                if ($p === strlen($b)) {
                    continue;
                }
                $s = substr($b, 0, $p) . $bytes . substr($b, $p + 1);
            } else {
                $s = substr($b, 0, $p) . $bytes . substr($b, $p);
            }
            $corruptions['check'] .= mb_check_encoding($s, 'UTF-8') ? '1' : '0';
            $corruptions['strlen'][] = mb_strlen($s, 'UTF-8');
        }
    }
}
$sets['corruptions'] = $corruptions;

$flags = JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE;
$out = "{\n  \"generator\": \"utf8.gen.php\",\n  \"php_version\": " . json_encode(PHP_VERSION);
foreach ($sets as $name => $set) {
    $out .= ",\n  " . json_encode($name) . ": {";
    $first = true;
    foreach ($set as $key => $value) {
        $out .= ($first ? "\n" : ",\n") . '    ' . json_encode($key) . ': ' . json_encode($value, $flags);
        $first = false;
    }
    $out .= "\n  }";
}
$out .= "\n}\n";
echo $out;
