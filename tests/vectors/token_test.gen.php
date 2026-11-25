<?php

/*
 * Generator of tests/vectors/token_test.json: results of Twig 3.31.1's Token::test() on PHP 8.5.13.
 *
 * Regenerate from the repository root (Git Bash on Windows needs MSYS_NO_PATHCONV=1):
 *
 *     MSYS_NO_PATHCONV=1 docker run --rm -i twig-oracle:3.31.1 php < tests/vectors/token_test.gen.php > tests/vectors/token_test.json
 *
 * Output:
 * - "loose": PHP's loose `$token->getValue() == $value` inside Token::test($type, $value) for string, int and float
 *   token values against string test values. "strings" are the test values, "tokens" the [type, value] pairs and
 *   "results" one string per token with '1'/'0' per test value.
 * - "bc": Token::test() type matching, including the OPERATOR/PUNCTUATION backward compatibility: "tokens" are
 *   [type, value] pairs, "tests" [type, values] pairs (values null, a string or a list of strings), "results" one
 *   string per token with '1'/'0' per test.
 * Strings that are not valid UTF-8 are written as {"b64": "..."}; ints as {"int": "..."}; floats as {"float":
 * (string) $value, "bits": IEEE-754 big-endian hex}.
 */

declare(strict_types=0);

ini_set('display_errors', 'stderr');

require '/oracle/vendor/autoload.php';

use Twig\Token;

function enc_str(string $s)
{
    return mb_check_encoding($s, 'UTF-8') ? $s : ['b64' => base64_encode($s)];
}

function enc_value($value)
{
    if (is_int($value)) {
        return ['int' => (string) $value];
    }
    if (is_float($value)) {
        return ['float' => is_nan($value) ? 'NAN' : (string) $value, 'bits' => bin2hex(pack('E', $value))];
    }
    if (is_array($value)) {
        return array_map('enc_str', $value);
    }
    if (null === $value) {
        return null;
    }

    return enc_str($value);
}

$strings = [
    '', ' ', "\0", '0', '00', '-0', '+0', '0.0', '-0.0', '.0', '0.', '1', '01', '+1', '-1', ' 1', '1 ', "\t\n\r\v\f1\t\n\r\v\f",
    "1\0", '1.0', '1.', '.5', '0.5', '-.5', '+.5', '1e0', '1E0', '1e', '1e+', '1e-', '1e+0', '1e-0', '1.e1', '.5e1', '1e5',
    '1.5e3', '1500', '1500.0', '100000', '1e-5', '0.00001', '1e1000', '-1e1000', '1e-1000', '1_000', '0x1A', '0b11',
    '1a', 'a1', 'abc', 'INF', '-INF', 'NAN', 'inf', 'nan', '0.1', '0.10', '0.30000000000000004', '0.3',
    '9007199254740992', '9007199254740993', '9007199254740992.0',
    '9223372036854775807', '9223372036854775807 ', '09223372036854775807', '9223372036854775808', '9223372036854775808 ',
    '-9223372036854775808', '-9223372036854775808 ', '-9223372036854775809', '9223372036854775807.0', '9.2233720368548E+18',
    '92233720368547758070', '18446744073709551615', '18446744073709551616', '123456789012345678901',
    '123456789012345678901.5', '123456789012345678901.5000', '1234567890123456789012', '00000000000000000000001',
    '99999999999999999999', '100000000000000000000', '1.0E+20', '1.0E+25', '1e20', '1E+20', '-1', '- 1', '--1', '1-',
    "1\n\n", ' 1 2', '1 2', '１', "\xff", 'é', '1é', '+', '-', '.', 'e1', '%', '(', '?:',
];

$tokens = [];
foreach ($strings as $s) {
    $tokens[] = [Token::STRING_TYPE, $s];
}
foreach ([0, 1, -1, 1500, 100000, PHP_INT_MAX, PHP_INT_MIN, 9007199254740993, 123] as $i) {
    $tokens[] = [Token::NUMBER_TYPE, $i];
}
foreach ([0.0, -0.0, 1.0, 1.5, 0.1, 0.3, 0.1 + 0.2, 1e20, 1e15, 1e14, 1500.0, 1e-5, 9.2233720368547758E+18, INF, -INF, NAN, 1.2345678901234567e+21] as $f) {
    $tokens[] = [Token::NUMBER_TYPE, $f];
}

$loose = ['strings' => array_map('enc_str', $strings), 'tokens' => [], 'results' => []];
foreach ($tokens as [$type, $value]) {
    $token = new Token($type, $value, 1);
    $bits = '';
    foreach ($strings as $s) {
        $bits .= $token->test($type, $s) ? '1' : '0';
    }
    $loose['tokens'][] = [$type, enc_value($value)];
    $loose['results'][] = $bits;
}

$bcTokens = [
    [Token::PUNCTUATION_TYPE, '('], [Token::PUNCTUATION_TYPE, ')'], [Token::PUNCTUATION_TYPE, '['], [Token::PUNCTUATION_TYPE, '|'],
    [Token::PUNCTUATION_TYPE, '.'], [Token::PUNCTUATION_TYPE, '?'], [Token::PUNCTUATION_TYPE, '?:'], [Token::PUNCTUATION_TYPE, ','],
    [Token::OPERATOR_TYPE, '('], [Token::OPERATOR_TYPE, 'not'], [Token::NAME_TYPE, 'for'], [Token::NAME_TYPE, '0'],
    [Token::STRING_TYPE, ''], [Token::EOF_TYPE, ''], [Token::NUMBER_TYPE, 0], [Token::OPERATOR_TYPE, '=>'], [Token::OPERATOR_TYPE, '...'],
];
$bcTests = [];
foreach ([Token::OPERATOR_TYPE, Token::PUNCTUATION_TYPE, Token::NAME_TYPE, Token::STRING_TYPE, Token::EOF_TYPE, Token::NUMBER_TYPE] as $type) {
    foreach ([null, '', '0', '(', ')', '[', '|', '.', '?', '?:', ',', 'not', 'for', [], [''], ['('], [')'], [')', '('], ['for', 'if'], ['0']] as $values) {
        $bcTests[] = [$type, $values];
    }
}
$bc = ['tokens' => [], 'tests' => [], 'results' => []];
foreach ($bcTests as [$type, $values]) {
    $bc['tests'][] = [$type, enc_value($values)];
}
set_error_handler(fn () => true);
foreach ($bcTokens as [$type, $value]) {
    $token = new Token($type, $value, 1);
    $bits = '';
    foreach ($bcTests as [$testType, $values]) {
        $bits .= $token->test($testType, $values) ? '1' : '0';
    }
    $bc['tokens'][] = [$type, enc_value($value)];
    $bc['results'][] = $bits;
}
restore_error_handler();

echo json_encode(['loose' => $loose, 'bc' => $bc], JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_PRESERVE_ZERO_FRACTION | JSON_THROW_ON_ERROR), "\n";
