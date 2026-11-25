<?php
/*
 * Oracle generator for substr_split.json (twig-rs, crates/twig-unicode, PHP 8.5.13 mbstring).
 *
 * - "substr": mb_substr($s, $start, $length, 'UTF-8') for every combination of the listed
 *   strings, starts and lengths (null = argument omitted), in two groups: short strings with
 *   many offsets, and long strings with offsets around mb_substr's 128-code-point decoding
 *   chunks. results[string][start][length] is bin2hex() of the result, or "!" followed by the
 *   message of the thrown ValueError.
 * - "split": mb_str_split($s, $length, 'UTF-8'); results[string][length] is the list of
 *   bin2hex() pieces, or "!" + ValueError message.
 * - "mblen": byte length of the first piece of mb_str_split(chr($b) . "AAAAAAA") for $b = 0..255.
 * - "ord": mb_ord($s, 'UTF-8') per string: the code point, false, or "!" + ValueError message.
 * - "convert_case_modes": mb_convert_case("aB", $mode, 'UTF-8') per mode: bin2hex() or "!" +
 *   ValueError message.
 *
 * Regenerate from the repository root with the oracle image (research/php-oracle):
 *   docker run --rm -i twig-oracle:3.31.1 php < crates/twig-unicode/tests/vectors/substr_split.gen.php \
 *       > crates/twig-unicode/tests/vectors/substr_split.json
 */

ini_set('memory_limit', '-1');
error_reporting(E_ALL);
set_error_handler(function ($no, $str) { fwrite(STDERR, "PHP error: $str\n"); exit(1); });

function attempt(callable $f)
{
    try {
        return $f();
    } catch (\ValueError $e) {
        return '!' . $e->getMessage();
    }
}

function substr_group(array $strings, array $starts, array $lengths): array
{
    $results = [];
    foreach ($strings as $s) {
        $perString = [];
        foreach ($starts as $start) {
            $perStart = [];
            foreach ($lengths as $length) {
                $perStart[] = attempt(fn() => bin2hex(mb_substr($s, $start, $length, 'UTF-8')));
            }
            $perString[] = $perStart;
        }
        $results[] = $perString;
    }
    return ['strings' => array_map('bin2hex', $strings), 'starts' => $starts, 'lengths' => $lengths, 'results' => $results];
}

$short = [
    '', 'a', 'abc', 'héllo wörld', "a\xFF\xC3\xA9\xE2\x82b", "\xF0\x9F\x98\x80x\xF0\x9F\x98",
    '日本語テキスト', "\xFF\xFE\xFD", "ab\xE2\x82\xACcd\xC0", "\xE0\x80\x80z", '12345',
];
$shortStarts = [0, 1, 2, 3, 5, 6, 10, 11, 12, 100, -1, -2, -3, -5, -6, -10, -11, -12, -100,
    PHP_INT_MAX, -PHP_INT_MAX, PHP_INT_MIN];
$shortLengths = [null, 0, 1, 2, 3, 5, 10, 11, 100, -1, -2, -3, -5, -10, -11, -100,
    PHP_INT_MAX, -PHP_INT_MAX, PHP_INT_MIN];

$long = [
    str_repeat("ab\xFFc\xC3\xA9", 50),
    str_repeat('Σ', 200),
    str_repeat('x', 130) . "\xE2\x82" . str_repeat('y', 130),
];
$longStarts = [0, 1, 126, 127, 128, 129, 249, 250, 255, 256, -1, -127, -128, -129, -200, -250, -261];
$longLengths = [null, 1, 2, 127, 128, 129, -1, -128, -129];

$splitStrings = array_merge($short, [str_repeat("ab\xFFc\xC3\xA9", 5), "\xE2\x82", "\xF0\x9F\x98", "a\xF4\x90\x80\x80b"]);
$splitLengths = [1, 2, 3, 4, 5, 7, 100, 0, -1, PHP_INT_MIN, 1073741823, 1073741824, PHP_INT_MAX];
$split = [];
foreach ($splitStrings as $s) {
    $perString = [];
    foreach ($splitLengths as $length) {
        $perString[] = attempt(fn() => array_map('bin2hex', mb_str_split($s, $length, 'UTF-8')));
    }
    $split[] = $perString;
}

$mblen = [];
for ($b = 0; $b < 256; $b++) {
    $mblen[] = strlen(mb_str_split(chr($b) . 'AAAAAAA', 1, 'UTF-8')[0]);
}

$ord = [];
foreach (array_merge($splitStrings, ["\x00", "\u{10FFFF}", "\xEF\xBF\xBD", "?"]) as $s) {
    $ord[] = [bin2hex($s), attempt(fn() => mb_ord($s, 'UTF-8'))];
}

$modes = [];
foreach ([-1, 0, 1, 2, 3, 4, 5, 6, 7, 8, PHP_INT_MAX, PHP_INT_MIN] as $mode) {
    $modes[] = [$mode, attempt(fn() => bin2hex(mb_convert_case('aB', $mode, 'UTF-8')))];
}

$data = [
    'generator' => 'substr_split.gen.php',
    'php_version' => PHP_VERSION,
    'substr' => [substr_group($short, $shortStarts, $shortLengths), substr_group($long, $longStarts, $longLengths)],
    'split' => ['strings' => array_map('bin2hex', $splitStrings), 'lengths' => $splitLengths, 'results' => $split],
    'mblen' => $mblen,
    'ord' => $ord,
    'convert_case_modes' => $modes,
];
echo json_encode($data, JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_PRETTY_PRINT), "\n";
