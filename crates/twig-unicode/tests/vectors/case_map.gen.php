<?php
/*
 * Oracle generator for case_map.json (twig-rs, crates/twig-unicode, PHP 8.5.13 mbstring).
 *
 * For every code point U+0000..U+10FFFF except the surrogates, taken as a one-character UTF-8
 * string:
 *
 * - "upper", "lower", "title", "fold", "upper_simple", "lower_simple", "title_simple",
 *   "fold_simple": mb_convert_case() with MB_CASE_UPPER .. MB_CASE_FOLD_SIMPLE. Only results
 *   that differ from the input are listed, as [code point, [result code points]].
 *   mb_strtoupper()/mb_strtolower() are checked to agree with MB_CASE_UPPER/MB_CASE_LOWER.
 *
 * - "cased", "case_ignorable": the derived properties as mbstring's tables have them, as
 *   inclusive [first, last] ranges. PHP has no function that returns them, so they are read
 *   off the case conversion (php_unicode_convert_case() in ext/mbstring/php_unicode.c):
 *     P1 = mb_strtolower(63 x "A" . "Σ" . X): the sigma is the last code point of the first
 *          64-code-point chunk, so the look-ahead runs scan_ahead_for_cased_letter() on X,
 *          which tests Cased first: the sigma stays "σ" iff X is Cased.
 *     P2 = mb_strtolower("AΣ" . X): inside the chunk the look-ahead skips Case_Ignorable code
 *          points first: "σ" iff X is Cased and not Case_Ignorable.
 *     P3 = mb_convert_case("A" . X . "a", MB_CASE_TITLE): X keeps title mode (the final "a"
 *          stays lower case) iff X is Case_Ignorable or Cased.
 *   Cased = P1; Case_Ignorable = Cased ? !P2 : P3. Contradicting probe results are listed in
 *   "anomalies" (expected empty).
 *
 * Regenerate from the repository root with the oracle image (research/php-oracle):
 *   docker run --rm -i twig-oracle:3.31.1 php < crates/twig-unicode/tests/vectors/case_map.gen.php \
 *       > crates/twig-unicode/tests/vectors/case_map.json
 */

ini_set('memory_limit', '-1');
error_reporting(E_ALL);
set_error_handler(function ($no, $str) { fwrite(STDERR, "PHP error: $str\n"); exit(1); });

$modes = [
    'upper' => MB_CASE_UPPER,
    'lower' => MB_CASE_LOWER,
    'title' => MB_CASE_TITLE,
    'fold' => MB_CASE_FOLD,
    'upper_simple' => MB_CASE_UPPER_SIMPLE,
    'lower_simple' => MB_CASE_LOWER_SIMPLE,
    'title_simple' => MB_CASE_TITLE_SIMPLE,
    'fold_simple' => MB_CASE_FOLD_SIMPLE,
];

function code_points(string $s): array
{
    return array_values(unpack('N*', mb_convert_encoding($s, 'UTF-32BE', 'UTF-8')));
}

$maps = array_fill_keys(array_keys($modes), []);
$ranges = ['cased' => [], 'case_ignorable' => []];
$anomalies = [];
$sigma = "\u{3A3}";
$p1Prefix = str_repeat('A', 63) . $sigma;

function add_to_ranges(array &$list, int $cp): void
{
    $n = count($list);
    if ($n > 0 && $list[$n - 1][1] === $cp - 1) {
        $list[$n - 1][1] = $cp;
    } else {
        $list[] = [$cp, $cp];
    }
}

for ($cp = 0; $cp <= 0x10FFFF; $cp++) {
    if ($cp >= 0xD800 && $cp <= 0xDFFF) {
        continue;
    }
    $ch = mb_chr($cp, 'UTF-8');
    foreach ($modes as $name => $mode) {
        $r = mb_convert_case($ch, $mode, 'UTF-8');
        if ($r !== $ch) {
            $maps[$name][] = [$cp, code_points($r)];
        }
        if ($mode === MB_CASE_UPPER && $r !== mb_strtoupper($ch, 'UTF-8')) {
            $anomalies[] = sprintf('U+%04X: mb_strtoupper differs from MB_CASE_UPPER', $cp);
        }
        if ($mode === MB_CASE_LOWER && $r !== mb_strtolower($ch, 'UTF-8')) {
            $anomalies[] = sprintf('U+%04X: mb_strtolower differs from MB_CASE_LOWER', $cp);
        }
    }

    $p1 = mb_substr(mb_strtolower($p1Prefix . $ch, 'UTF-8'), 63, 1, 'UTF-8');
    $p2 = mb_substr(mb_strtolower('A' . $sigma . $ch, 'UTF-8'), 1, 1, 'UTF-8');
    $p3 = mb_substr(mb_convert_case('A' . $ch . 'a', MB_CASE_TITLE, 'UTF-8'), -1, 1, 'UTF-8');
    foreach (['P1' => $p1, 'P2' => $p2] as $probe => $value) {
        if ($value !== "\u{3C3}" && $value !== "\u{3C2}") {
            $anomalies[] = sprintf('U+%04X: %s gave %s', $cp, $probe, bin2hex($value));
        }
    }
    if ($p3 !== 'a' && $p3 !== 'A') {
        $anomalies[] = sprintf('U+%04X: P3 gave %s', $cp, bin2hex($p3));
    }
    $cased = $p1 === "\u{3C3}";
    $caseIgnorable = $cased ? $p2 === "\u{3C2}" : $p3 === 'a';
    if ($cased && $p3 !== 'a') {
        $anomalies[] = sprintf('U+%04X: Cased but P3 ended title mode', $cp);
    }
    if (!$cased && $p2 !== "\u{3C2}") {
        $anomalies[] = sprintf('U+%04X: not Cased but P2 kept the sigma medial', $cp);
    }
    if ($cased) {
        add_to_ranges($ranges['cased'], $cp);
    }
    if ($caseIgnorable) {
        add_to_ranges($ranges['case_ignorable'], $cp);
    }
}

// Output: one list entry per line, to keep diffs readable.
$flags = JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE;
$out = "{\n";
$out .= '  "generator": "case_map.gen.php",' . "\n";
$out .= '  "php_version": ' . json_encode(PHP_VERSION) . ",\n";
$out .= '  "anomalies": ' . json_encode($anomalies, $flags) . ",\n";
$sections = $maps + $ranges;
$i = 0;
foreach ($sections as $name => $entries) {
    $out .= '  ' . json_encode($name) . ": [\n";
    $lines = array_map(fn($e) => '    ' . json_encode($e, $flags), $entries);
    $out .= implode(",\n", $lines) . "\n  ]" . (++$i < count($sections) ? ',' : '') . "\n";
}
$out .= "}\n";
echo $out;
