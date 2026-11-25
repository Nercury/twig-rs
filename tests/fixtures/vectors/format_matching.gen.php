<?php

/*
 * Generator of tests/fixtures/vectors/format_matching.json: the verdicts of PHPUnit 9.6's
 * assertStringMatchesFormat() (PHPUnit\Framework\Constraint\StringMatchesFormatDescription, the constraint
 * IntegrationTestCase uses to compare render-phase exceptions) for format/string pairs.
 *
 * Regenerate from the repository root (Git Bash on Windows needs MSYS_NO_PATHCONV=1):
 *
 *     MSYS_NO_PATHCONV=1 docker run --rm -i twig-oracle:3.31.1 php < tests/fixtures/vectors/format_matching.gen.php > tests/fixtures/vectors/format_matching.json
 *
 * Output: {"generator", "phpunit", "cases": [{"format", "string", "matches"}]}.
 */

declare(strict_types=1);

require '/oracle/vendor/autoload.php';

use PHPUnit\Framework\Constraint\StringMatchesFormatDescription;

$pairs = [
    // the only placeholder use in the fixtures (expressions/matches_error_runtime.test)
    ['Twig\Error\RuntimeError: Regexp "3" passed to "matches" is not valid: Delimiter must not be alphanumeric%sbackslash%sin "index.twig" at line 2',
        'Twig\Error\RuntimeError: Regexp "3" passed to "matches" is not valid: Delimiter must not be alphanumeric, backslash, or NUL byte in "index.twig" at line 2'],
    ['Twig\Error\RuntimeError: Regexp "3" passed to "matches" is not valid: Delimiter must not be alphanumeric%sbackslash%sin "index.twig" at line 2',
        'Twig\Error\RuntimeError: Regexp "3" passed to "matches" is not valid: Delimiter must not be alphanumeric or backslash in "index.twig" at line 2'],
    // `{% block %}` in load-phase expectations: "% " is not a placeholder
    ['Did you forget to put the content inside a {% block %} tag?', 'Did you forget to put the content inside a {% block %} tag?'],
    ['a{% block %}b', 'a{X block %}b'],
    // every placeholder
    ['%%', '%'], ['%%', '%%'], ['%%s', '%s'], ['%%s', 'x'], ['%%%s', '%abc'],
    ['%e', '/'], ['%e', '\\'], ['a%eb', 'a/b'],
    ['%s', 'abc'], ['%s', ''], ['%s', "a\nb"], ['%s', "a\rb"], ['x%sy', 'x y'], ['x%sy', 'xy'],
    ['%S', ''], ['%S', 'abc'], ['%S', "a\nb"], ['x%Sy', 'xy'],
    ['%a', ''], ['%a', "a\nb"], ['%a', 'x'], ['x%ay', "x\n\ny"],
    ['%A', ''], ['%A', "a\nb"],
    ['%w', ''], ['%w', " \t\n\r\x0B\x0C"], ['a%wb', 'a  b'], ['a%wb', 'a-b'],
    ['%i', '42'], ['%i', '-42'], ['%i', '+42'], ['%i', '4.2'], ['%i', ''],
    ['%d', '42'], ['%d', '-42'], ['%d', '007'],
    ['%x', 'deadBEEF'], ['%x', 'xyz'], ['%x', ''],
    ['%f', '1.5'], ['%f', '-1.5e10'], ['%f', '.5'], ['%f', '5.'], ['%f', '1E-3'], ['%f', 'abc'], ['%f', '+.5E+3'],
    ['%c', 'a'], ['%c', ''], ['%c', 'ab'], ['%c', "\n"],
    // not placeholders
    ['%z', '%z'], ['%z', 'z'], ['%', '%'], ['100%', '100%'], ['%.', '%.'], ['%\\s', '%\\s'], ['\\%s', '\\abc'],
    // regex metacharacters are literal
    ['a.b', 'a.b'], ['a.b', 'axb'], ['(a|b)*', '(a|b)*'], ['(a|b)*', 'a'], ['[x]{2}', '[x]{2}'], ['^$', '^$'],
    ['a/b#c', 'a/b#c'], ['\\d', '\\d'], ['\\d', '1'], ["NUL\0byte", "NUL\0byte"],
    // anchors and the final newline (`$` without D)
    ['abc', 'abc'], ['abc', 'abcd'], ['abc', 'xabc'], ['abc', "abc\n"], ['abc', "abc\n\n"], ['abc', "abc\r\n"],
    ['abc%s', "abcd\n"], ["abc\n", 'abc'], ["abc\n", "abc\n"],
    // "\r\n" is normalised in both strings
    ["a\r\nb", "a\nb"], ["a\nb", "a\r\nb"], ["a\rb", "a\nb"], ["a\r\r\nb", "a\r\nb"],
    // bytes, not characters
    ['é', 'é'], ['%c', 'é'], ['%c%c', 'é'], ['%s', "\xC3\x28"], ['%a', "\xFF"],
    // case sensitivity
    ['ABC', 'abc'],
];

$cases = [];
foreach ($pairs as [$format, $string]) {
    $cases[] = [
        'format' => $format,
        'string' => $string,
        'matches' => (new StringMatchesFormatDescription($format))->evaluate($string, '', true),
    ];
}

$encode = static function (array $case): array {
    foreach (['format', 'string'] as $key) {
        if (!mb_check_encoding($case[$key], 'UTF-8')) {
            $case[$key.'_b64'] = base64_encode($case[$key]);
            $case[$key] = mb_convert_encoding($case[$key], 'UTF-8', 'UTF-8');
        }
    }

    return $case;
};

echo json_encode([
    'generator' => 'tests/fixtures/vectors/format_matching.gen.php',
    'phpunit' => PHPUnit\Runner\Version::id(),
    'cases' => array_map($encode, $cases),
], JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_PRETTY_PRINT | JSON_THROW_ON_ERROR), "\n";
