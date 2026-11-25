<?php

/*
 * Generator of tests/vectors/lexer_random.json: Twig 3.31.1 tokenization results for random templates built from
 * the byte sequences that matter to the lexer (a differential test against the hand-picked cases of lexer.json),
 * with random lexer options and, for some templates, extra operators. Same output format as
 * tests/vectors/lexer.gen.php; the templates are written into the vector file, so the Rust side does not need the
 * PRNG (mt_rand() with a fixed seed).
 *
 * Trim options containing PCRE metacharacters are left out on purpose: Twig inserts them unquoted into the
 * endverbatim pattern (a documented deviation of twig-rs, see LexerOptions).
 *
 * Regenerate from the repository root (Git Bash on Windows needs MSYS_NO_PATHCONV=1):
 *
 *     MSYS_NO_PATHCONV=1 docker run --rm -i twig-oracle:3.31.1 php < tests/vectors/lexer_random.gen.php > tests/vectors/lexer_random.json
 *
 * Environment variables SEED and COUNT override the seed and the number of templates for ad-hoc differential runs
 * (see lexer_matches_twig_on_external_vectors in tests/lexer_vectors.rs).
 */

declare(strict_types=0);

ini_set('display_errors', 'stderr');

require '/oracle/vendor/autoload.php';

use Twig\Environment;
use Twig\Error\Error as TwigError;
use Twig\ExpressionParser\ExpressionParsers;
use Twig\ExpressionParser\Infix\BinaryOperatorExpressionParser;
use Twig\ExpressionParser\Prefix\UnaryOperatorExpressionParser;
use Twig\Lexer;
use Twig\Loader\ArrayLoader;
use Twig\Node\Expression\Binary\AddBinary;
use Twig\Node\Expression\Unary\NotUnary;
use Twig\Source;
use Twig\Token;

const PIECES = [
    '{{', '}}', '{%', '%}', '{#', '#}', '{##', '##}', '-', '~', ' ', ' ', ' ', "\n", "\r", "\r\n", "\t", "\x0c", "\x0b", "\0",
    "'", '"', '\\', '\\\\', '#{', '}', '{', '(', ')', '[', ']', '|', '.', '?', ':', ',', '#', '##', '0', '1_2', '1e', '1.5', '_',
    'a', 'x', 'not', 'in', 'is not', 'and', 'verbatim', 'endverbatim', 'line 3', 'line', '$', "\x85", "\xc3\xa9", "\xff", "\x7f",
    '=>', '...', '??', '?:', '? :', '?.', '\\n', '\\x41', "\\'", '\\"', '\\#{', '\\q', '\\0', '\\777', 'foo.and', ' b-and', '|and',
    '. and', '{{ x }}', '{% if a %}', "'s'", '"#{a}"', '-}}', '~}}', '-%}', '~%}', '-#}', '~#}', '{{-', '{{~', '{%-', '{%~', '{#-',
    '{#~', '{% verbatim %}', '{% endverbatim %}', '{%- verbatim -%}', '{%~ endverbatim ~%}', '{% line 7 %}', '{% line 0 %}', '1e3',
    '1_000', '0.5', '9223372036854775808', '..', 'starts with', 'not  in', "not\nin", 'has some', 'matches', '!', '@', ';', '`',
    '^', '&', '*', '**', '//', '%', '+', '<=>', '===', '!=', 'b-and2', 'or(', 'in[', 'is{', "\r\n}}", "\r%}", '{## d ##}', '[#',
    '#]', '${', '<%', '%>', '<%=', '=', '!', '{=', '{!', '=}', '!}', '=%}', '{%=', '{%!', '!%}', 'custom', 'custom not', 'нет',
    '≠', '< >',
];

const OPTION_SETS = [
    [], [], [], [], [], [], [], [], [], [],
    ['interpolation' => ['${', '}']],
    ['tag_variable' => ['{ {', '}}']],
    ['whitespace_trim' => ''],
    ['tag_comment' => ['[#', '#]']],
    ['tag_block' => ['<%', '%>'], 'tag_variable' => ['<%=', '%>']],
    ['tag_comment' => ['{#', '#']],
    ['whitespace_line_chars' => ' x'],
    ['whitespace_trim' => '=', 'whitespace_line_trim' => '!'],
    ['whitespace_trim' => ' ', 'whitespace_line_trim' => '~'],
    ['interpolation' => ['', '}']],
    ['tag_comment' => ['{', '}']],
    ['tag_variable' => ['{', '}']],
    ['interpolation' => ['#{', '}}']],
    ['tag_variable' => ['{%', '%}']],
];

function enc_str(?string $s)
{
    if (null === $s) {
        return null;
    }

    return mb_check_encoding($s, 'UTF-8') ? $s : ['b64' => base64_encode($s)];
}

function enc_value($value)
{
    if (is_int($value)) {
        return ['int' => (string) $value];
    }
    if (is_float($value)) {
        return ['float' => (string) $value, 'bits' => bin2hex(pack('E', $value))];
    }

    return enc_str($value);
}

function enc_token(Token $token): array
{
    $type = (fn () => $this->type)->call($token);

    return [$type, enc_value($token->getValue()), $token->getLine(), $token->getOffset(), enc_str($token->getDocumentation())];
}

function enc_error(Throwable $e): array
{
    if ($e instanceof TwigError) {
        return [
            'class' => get_class($e),
            'raw' => enc_str($e->getRawMessage()),
            'line' => $e->getTemplateLine(),
            'column' => $e->getTemplateColumn(),
            'message' => enc_str($e->getMessage()),
        ];
    }

    return ['class' => get_class($e), 'message' => enc_str($e->getMessage())];
}

function j($value): string
{
    return json_encode($value, JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_PRESERVE_ZERO_FRACTION | JSON_THROW_ON_ERROR);
}

$seed = (int) (getenv('SEED') ?: 20261125);
$count = (int) (getenv('COUNT') ?: 3000);
mt_srand($seed);
$blocks = [];
for ($i = 0; $i < $count; ++$i) {
    $code = '';
    $pieces = mt_rand(0, 40);
    for ($p = 0; $p < $pieces; ++$p) {
        $code .= PIECES[mt_rand(0, count(PIECES) - 1)];
    }
    $options = OPTION_SETS[mt_rand(0, count(OPTION_SETS) - 1)];
    $env = new Environment(new ArrayLoader());
    $operators = null;
    if (0 === mt_rand(0, 4)) {
        $env->getExpressionParsers()->add([
            new UnaryOperatorExpressionParser(NotUnary::class, 'custom', 50, aliases: ['shared']),
            new UnaryOperatorExpressionParser(NotUnary::class, 'custom not', 50, aliases: ['shared']),
            new UnaryOperatorExpressionParser(NotUnary::class, 'нет', 50),
            new BinaryOperatorExpressionParser(AddBinary::class, '≠', 30),
            new BinaryOperatorExpressionParser(AddBinary::class, '< >', 30),
        ]);
        $operators = [];
        foreach ($env->getExpressionParsers() as $parser) {
            foreach (ExpressionParsers::getOperatorTokensFor($parser) as $token) {
                $operators[] = $token;
            }
        }
    }

    $deprecations = [];
    set_error_handler(function (int $type, string $message) use (&$deprecations): bool {
        if (E_USER_DEPRECATED === $type) {
            $deprecations[] = enc_str($message);
        }

        return true;
    });
    $res = ['id' => "random-$i"];
    if (mb_check_encoding($code, 'UTF-8')) {
        $res['source'] = $code;
    } else {
        $res['source_b64'] = base64_encode($code);
    }
    if ($options) {
        $res['options'] = $options;
    }
    if (null !== $operators) {
        $res['operators'] = array_map('enc_str', $operators);
    }
    try {
        $stream = (new Lexer($env, $options))->tokenize(new Source($code, 'index'));
        $res['tokens'] = array_map('enc_token', (fn () => $this->tokens)->call($stream));
    } catch (Throwable $e) {
        $res['error'] = enc_error($e);
    }
    restore_error_handler();
    $res['deprecations'] = $deprecations;

    $fields = [];
    foreach ($res as $key => $value) {
        if ('tokens' === $key) {
            $fields[] = '"tokens": ['.implode(', ', array_map('j', $value)).']';
        } else {
            $fields[] = j($key).': '.j($value);
        }
    }
    $blocks[] = '{'.implode(', ', $fields).'}';
}
echo "[\n", implode(",\n", $blocks), "\n]\n";
