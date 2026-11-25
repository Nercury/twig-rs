<?php

/*
 * Generator of tests/vectors/lexer.json: Twig 3.31.1 tokenization results (tokens with type, value, line, offset
 * and documentation; or the error) plus deprecations, and optionally the result of one TokenStream::expect() call.
 *
 * Regenerate from the repository root (Git Bash on Windows needs MSYS_NO_PATHCONV=1):
 *
 *     MSYS_NO_PATHCONV=1 docker run --rm -i twig-oracle:3.31.1 php < tests/vectors/lexer.gen.php > tests/vectors/lexer.json
 *
 * Case fields: id, source (or parts: [[text, repeat count], ...] concatenated), optional name (template name,
 * default "index"), options (Lexer options), parsers (extra expression parsers: ["unary"|"binary", name, [aliases]]),
 * expect ([skip, type, value, message]).
 * Output per case: id, source (or source_b64 when not UTF-8, or source_parts), name, options, operators (when
 * parsers were added), tokens ([type, value, line, offset, documentation]) or error, deprecations, expect (input +
 * result). Strings that are not valid UTF-8 are written as {"b64": "..."}, strings longer than 512 bytes as
 * {"len": n, "crc32": "8 hex digits"}; ints as {"int": "..."}; floats as {"float": (string) $value, "bits":
 * IEEE-754 big-endian hex}.
 */

declare(strict_types=0);

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

const TK_EOF = -1;
const TK_TEXT = 0;
const TK_BLOCK_START = 1;
const TK_VAR_START = 2;
const TK_BLOCK_END = 3;
const TK_VAR_END = 4;
const TK_NAME = 5;
const TK_NUMBER = 6;
const TK_STRING = 7;
const TK_OPERATOR = 8;
const TK_PUNCTUATION = 9;

$cases = [
    // basics
    ['id' => 'empty', 'source' => ''],
    ['id' => 'text-only', 'source' => 'Hello world'],
    ['id' => 'hello-name', 'source' => 'Hello {{ name }}!'],
    ['id' => 'if-block', 'source' => '{% if a %}x{% endif %}'],
    ['id' => 'if-else-newline', 'source' => "{% if a %}x{% else %}y{% endif %}\n"],
    ['id' => 'attributes', 'source' => '{{ a.b.c }}'],
    ['id' => 'float-attribute', 'source' => '{{ a.0.1 }}'],
    ['id' => 'subscripts', 'source' => '{{ a[0]["b"] }}'],
    ['id' => 'ternary', 'source' => '{{ a ? b : c }}'],
    ['id' => 'elvis', 'source' => '{{ a ?: b }}'],
    ['id' => 'elvis-spaced', 'source' => '{{ a ? : b }}'],
    ['id' => 'elvis-spaced-newline', 'source' => "{{ a ?\n\t: b }}"],
    ['id' => 'null-coalesce', 'source' => '{{ a ?? b }}'],
    ['id' => 'null-safe', 'source' => '{{ a?.b }}'],
    ['id' => 'null-safe-number', 'source' => '{{ true ?.5 : 1 }}'],
    ['id' => 'call-arguments', 'source' => '{{ f(a, b = 1, c: 2) }}'],
    ['id' => 'filters', 'source' => '{{ x|upper|default("y") }}'],
    ['id' => 'arrow', 'source' => '{{ (a) => a * 2 }}'],
    ['id' => 'spread', 'source' => '{{ [1, ...a] }}'],
    ['id' => 'mapping', 'source' => "{{ {a, b: 2, 'c': 3, 4: 5, (1+1): 6, ...{z: 1}} }}"],
    ['id' => 'spread-number', 'source' => '{{ 1...5 }}'],
    ['id' => 'set', 'source' => '{% set a = 1 %}'],
    ['id' => 'destructuring', 'source' => '{% do [x, y] = [1, 2] %}'],
    ['id' => 'multiline-expression', 'source' => "{{\n  a\n  +\n  b\n}}"],
    ['id' => 'two-prints', 'source' => '{{ x }}{{ y }}'],

    // operators
    ['id' => 'op-not-in', 'source' => '{{ a not in b }}'],
    ['id' => 'op-not-in-whitespace', 'source' => "{{ a not   in\n b }}"],
    ['id' => 'op-not-in-tab', 'source' => "{{ a not\tin b }}"],
    ['id' => 'op-is-not', 'source' => '{{ a is not b }}'],
    ['id' => 'op-is-not-closing', 'source' => '{{ a is not}}'],
    ['id' => 'op-starts-with', 'source' => '{{ a starts with b }}'],
    ['id' => 'op-ends-with', 'source' => '{{ a ends  with b }}'],
    ['id' => 'op-has-some-every', 'source' => '{{ a has some b }}{{ a has every b }}'],
    ['id' => 'op-matches', 'source' => '{{ a matches b }}{{ a matches(b) }}'],
    ['id' => 'op-bitwise', 'source' => '{{ 1 b-and 2 b-or 3 b-xor 4 }}'],
    ['id' => 'op-b-and-digit', 'source' => '{{ 1 b-and2 }}'],
    ['id' => 'op-b-and-paren', 'source' => '{{ 1 b-and(2) }}'],
    ['id' => 'op-logical', 'source' => '{{ a and b or c xor d }}'],
    ['id' => 'op-not', 'source' => '{{ not a }}{{ not(a) }}{{ not[1] }}{{ not{a:1} }}'],
    ['id' => 'op-not-prefix-names', 'source' => '{{ notx }}{{ notable }}'],
    ['id' => 'op-comparisons', 'source' => '{{ 1 <=> 2 }}{{ 1 === 2 }}{{ 1 !== 2 }}{{ 1 == 2 }}{{ 1 != 2 }}{{ 1 >= 2 }}{{ 1 <= 2 }}{{ 1 < 2 }}{{ 1 > 2 }}'],
    ['id' => 'op-range', 'source' => '{{ 1..5 }}'],
    ['id' => 'op-arithmetic', 'source' => '{{ 7 // 2 ** 3 ~ 1 * 2 / 3 % 4 }}'],
    ['id' => 'op-unary', 'source' => '{{ 1 - -2 + +3 }}'],
    ['id' => 'op-filter-named-and', 'source' => "{{ 'a'|and }}"],
    ['id' => 'op-filter-two-spaces-and', 'source' => "{{ 'x' |  and }}"],
    ['id' => 'op-attribute-named-and', 'source' => '{{ foo.and }}{{ foo . and }}{{ foo .and }}'],
    ['id' => 'op-attribute-two-spaces-and', 'source' => '{{ x.  and }}'],
    ['id' => 'op-newline-dot-and', 'source' => "{{ x\n.and }}"],
    ['id' => 'op-newline-pipe-and', 'source' => "{{ x\n|and }}"],
    ['id' => 'op-dot-newline-and', 'source' => "{{ x.\nand }}"],
    ['id' => 'op-in', 'source' => '{{ a in b }}{{ a in(b) }}{{ a in[1] }}{{ a in{} }}'],
    ['id' => 'op-in-closing', 'source' => '{{ a in}}'],
    ['id' => 'op-is-defined', 'source' => '{{ x is defined }}{{ x isdefined }}'],
    ['id' => 'op-uppercase', 'source' => '{{ AND }}{{ TRUE }}'],
    ['id' => 'op-words-as-names', 'source' => '{{ literal }}{{ in }}{{ is }}{{ and }}'],
    ['id' => 'op-and-end-of-line', 'source' => "{{ 1 and\n0}}"],
    ['id' => 'op-block-in', 'source' => '{% block in%}'],
    ['id' => 'op-or-paren', 'source' => '{{ a or(b) }}{{ a|or }}{{ a or }}'],
    ['id' => 'op-startswith', 'source' => '{{ a startswith b }}'],
    ['id' => 'op-compact', 'source' => '{{ a?:b }}{{ a ?b:c }}{{ a=>b }}{{ a = b }}{{ a==b }}'],
    ['id' => 'op-bang', 'source' => '{{ a ! b }}'],
    ['id' => 'op-question-end', 'source' => '{{ a? }}'],
    ['id' => 'op-not-in-paren', 'source' => '{{ a not in(b) }}'],
    ['id' => 'op-is-not-paren', 'source' => '{{ a is not(b) }}'],
    ['id' => 'op-not-not', 'source' => '{{ not not a }}'],

    // names
    ['id' => 'name-utf8', 'source' => '{{ café }}{{ é }}{{ 日本 }}'],
    ['id' => 'name-tag-section-sign', 'source' => '{% § %}'],
    ['id' => 'name-function-section-sign', 'source' => '{{ §() }}'],
    ['id' => 'name-nbsp', 'source' => "{{ a\xc2\xa0}}"],
    ['id' => 'name-underscore', 'source' => '{{ _a1 }}{{ a_b.c_d }}'],
    ['id' => 'name-del', 'source' => "{{ \x7f }}"],
    ['id' => 'name-latin1', 'source' => "{{ caf\xe9 }}"],

    // numbers
    ['id' => 'num-integers', 'source' => '{{ 0 }}{{ 1 }}{{ 007 }}{{ 00 }}'],
    ['id' => 'num-floats', 'source' => '{{ 1.0 }}{{ 1.5 }}{{ 0.0 }}{{ 0.1 }}{{ 3.14159265358979 }}'],
    ['id' => 'num-exponents', 'source' => '{{ 1e3 }}{{ 1E2 }}{{ 1.5e3 }}{{ 1e+3 }}{{ 1e-3 }}{{ 1.0e20 }}'],
    ['id' => 'num-underscores', 'source' => '{{ 1_000 }}{{ 1_000.000_1e1_0 }}{{ 1_0.5_0 }}{{ 1e3_0 }}'],
    ['id' => 'num-double-underscore', 'source' => '{{ 1__2 }}'],
    ['id' => 'num-trailing-underscore', 'source' => '{{ 1_ }}'],
    ['id' => 'num-bare-exponent', 'source' => '{{ 1e }}{{ 1e+ }}{{ 1e_3 }}'],
    ['id' => 'num-trailing-dot', 'source' => '{{ 1. }}'],
    ['id' => 'num-leading-dot', 'source' => '{{ .5 }}'],
    ['id' => 'num-hex-binary', 'source' => '{{ 0x1A }}{{ 0b11 }}'],
    ['id' => 'num-int-max', 'source' => '{{ 9223372036854775807 }}'],
    ['id' => 'num-int-overflow', 'source' => '{{ 9223372036854775808 }}{{ 922337203685477580700 }}{{ 123456789012345678 }}'],
    ['id' => 'num-infinite', 'source' => '{{ 1e1000 }}{{ 1e-400 }}'],
    ['id' => 'num-precision', 'source' => '{{ 100000000000005.0 }}{{ 0.30000000000000004 }}{{ 0.000012345 }}{{ 1.5.3 }}'],
    ['id' => 'num-negative', 'source' => '{{ -1 }}'],

    // strings and escapes
    ['id' => 'str-basic', 'source' => "{{ 'abc' }}{{ \"abc\" }}{{ '' }}{{ \"\" }}"],
    ['id' => 'str-esc-unknown', 'source' => <<<'T'
{{ '\a\b\e' }}
T],
    ['id' => 'str-esc-u', 'source' => '{{ "\\' . 'u0041" }}'],
    ['id' => 'str-esc-upper-x', 'source' => <<<'T'
{{ '\X41' }}{{ '\8' }}{{ '\9' }}
T],
    ['id' => 'str-esc-hex', 'source' => <<<'T'
{{ '\x41\x4a\x4' }}{{ '\x' }}{{ '\xfg' }}{{ '\xF' }}{{ '\xFFF' }}{{ '\x6' }}
T],
    ['id' => 'str-esc-octal', 'source' => <<<'T'
{{ '\12a' }}{{ '\1234' }}{{ '\777' }}{{ '\400' }}{{ '\065\x64' }}{{ '\0' }}{{ '\00' }}{{ '\0000' }}
T],
    ['id' => 'str-esc-interpolation', 'source' => <<<'T'
{{ '\#{x}' }}{{ '\#' }}{{ "App\#{var}" }}{{ "\#" }}{{ "a\#{b}c" }}
T],
    ['id' => 'str-esc-quotes', 'source' => <<<'T'
{{ "a\'b" }}{{ 'a\"b' }}{{ "foo \" bar" }}{{ 'foo \' bar' }}
T],
    ['id' => 'str-esc-specials', 'source' => <<<'T'
{{ '\f\n\r\t\v' }}{{ '\\f\\n\\r\\t\\v' }}
T],
    ['id' => 'str-esc-backslashes', 'source' => <<<'T'
{{ 'App\\Test' }}{{ 'App\Test' }}{{ 'Ymd\\THis' }}
T],
    ['id' => 'str-esc-utf8', 'source' => <<<'T'
{{ 'a\é' }}
T],
    ['id' => 'str-hash', 'source' => '{{ "#" }}{{ "a # b" }}'],
    ['id' => 'str-nul', 'source' => "{{ \"\0\" }}"],
    ['id' => 'str-crlf', 'source' => "{{ 'a\r\nb' }}{{ \"a\rb\" }}{{ x }}"],
    ['id' => 'str-esc-position-after-crlf', 'source' => "{{ 'a\r\n\\q' }}"],
    ['id' => 'str-esc-line', 'source' => "\n\n{{ 'x\\q' }}"],
    ['id' => 'str-esc-multiline-line', 'source' => "{{ 'a\nb\\q' }}"],
    ['id' => 'str-esc-interpolated-parts', 'source' => "{{ \"a\\q#{b}\\w\" }}"],
    ['id' => 'str-unterminated-single', 'source' => "{{ 'abc }}"],
    ['id' => 'str-unterminated-escaped-quote', 'source' => <<<'T'
{{ 'a\' }}
T],
    ['id' => 'str-unterminated-double', 'source' => '{{ "abc }}'],
    ['id' => 'str-trailing-backslash', 'source' => '{{ "abc\\'],
    ['id' => 'str-unterminated-interpolation', 'source' => '{{ "#{" }}'],
    ['id' => 'str-unterminated-interpolation-2', 'source' => '{{ "a#{1" }}'],
    ['id' => 'str-unterminated-eof-single', 'source' => "{{ 'abc"],
    ['id' => 'str-unterminated-eof-double', 'source' => '{{ "abc'],
    ['id' => 'str-quote-only', 'source' => '{{ "'],
    ['id' => 'str-utf8-long-ja', 'source' => '{{ "日本では、春になると桜の花が咲きます。" }}{{ \'日本では、春になると桜の花が咲きます。\' }}'],

    // interpolation
    ['id' => 'interp-basic', 'source' => '{{ "foo #{bar} baz" }}'],
    ['id' => 'interp-only', 'source' => '{{ "#{x}" }}'],
    ['id' => 'interp-nested', 'source' => '{{ "bar #{ "foo#{bar}" }" }}'],
    ['id' => 'interp-nested-block', 'source' => '{% foo "bar #{ "foo#{bar}" }" %}'],
    ['id' => 'interp-mapping', 'source' => '{{ "a#{ {x: 1}|length }b" }}'],
    ['id' => 'interp-concat', 'source' => '{{ "a#{1}" "b" }}'],
    ['id' => 'interp-hash-text', 'source' => '{{ "bar # baz" }}{{ "me # this is NOT an inline comment" }}'],
    ['id' => 'interp-unclosed-paren', 'source' => '{{ "#{ 1 )" }}'],
    ['id' => 'interp-paren-brace', 'source' => '{{ "#{ (1 }" }}'],
    ['id' => 'interp-multiline', 'source' => "{{ \"#{\n x }\" }}"],
    ['id' => 'interp-unclosed-variable', 'source' => "\n{% set a = \"#{ x "],
    ['id' => 'interp-end-line', 'source' => "{{ \"#{ x \n}\" }}"],
    ['id' => 'interp-deep', 'source' => '{{ "#{ "#{ "x" }" }" }}'],
    ['id' => 'interp-several', 'source' => '{{ "a #{b} #{c}" }}{{ "#{a}#{b}" }}'],
    ['id' => 'interp-concat-inside', 'source' => '{{ "#{ a ~ "b" }" }}'],
    ['id' => 'interp-double-hash', 'source' => '{{ "##{a}" }}'],
    ['id' => 'interp-empty', 'source' => '{{ "#{}" }}'],
    ['id' => 'interp-crlf', 'source' => "{{ \"a\r\n#{x}\" }}"],

    // brackets and punctuation
    ['id' => 'br-nested-mappings', 'source' => '{{ {"a":{"b":"c"}} }}{{ {a: {b: 1}} }}'],
    ['id' => 'br-unclosed-paren', 'source' => '{{ (1 + 3 }}'],
    ['id' => 'br-unclosed-bracket', 'source' => '{{ obj["a" }}'],
    ['id' => 'br-unclosed-brace', 'source' => '{{ ({ a: 1) }}'],
    ['id' => 'br-unclosed-nested', 'source' => '{{ (([1]) + 3 }}'],
    ['id' => 'br-unexpected-paren', 'source' => '{{ 1 + 3) }}'],
    ['id' => 'br-unexpected-bracket', 'source' => '{{ obj] }}'],
    ['id' => 'br-unexpected-brace', 'source' => '{{ { a: 1 }}'],
    ['id' => 'br-unexpected-paren-2', 'source' => '{{ ([1] + 3)) }}'],
    ['id' => 'br-print-single-brace', 'source' => '{{ x }'],
    ['id' => 'br-unclosed-at-eof', 'source' => '{{ [1, 2'],
    ['id' => 'br-unclosed-multiline', 'source' => "{{ [1,\n2"],
    ['id' => 'br-punctuation', 'source' => '{{ a, b: c }}'],
    ['id' => 'br-paren-multiline', 'source' => "{{ (\n1\n }}"],
    ['id' => 'br-block-unclosed', 'source' => '{% if (a %}'],
    ['id' => 'br-unexpected-second-line', 'source' => "{{ x }}\n{{ ) }}"],
    ['id' => 'br-mismatch-line', 'source' => "{{ [\n1,\n(2\n] }}"],
    ['id' => 'br-empty-mapping', 'source' => '{{ {} }}'],

    // unexpected characters
    ['id' => 'char-dollar', 'source' => '{{ $x }}'],
    ['id' => 'char-at', 'source' => '{{ @ }}'],
    ['id' => 'char-semicolon', 'source' => '{{ ; }}'],
    ['id' => 'char-nul', 'source' => "{{ \0 }}"],
    ['id' => 'char-backtick', 'source' => '{{ ` }}'],
    ['id' => 'char-backslash', 'source' => '{{ \\ }}'],
    ['id' => 'char-column-bytes', 'source' => '{{ é + $ }}'],
    ['id' => 'char-column-crlf', 'source' => "x\r\n{{ é + \$ }}"],
    ['id' => 'char-third-line', 'source' => "\n\n{{ & }}"],
    ['id' => 'char-caret', 'source' => "{{ 'a' ^ }}"],
    ['id' => 'char-tab-dollar', 'source' => "{{\t\$ }}"],
    ['id' => 'char-nul-before-close', 'source' => "{{ x\0}}"],

    // unclosed tags
    ['id' => 'unclosed-var-no-space', 'source' => '{{ x'],
    ['id' => 'unclosed-var-space', 'source' => '{{ x '],
    ['id' => 'unclosed-block-newline', 'source' => "\n{% if x\n"],
    ['id' => 'unclosed-block-open', 'source' => '{%'],
    ['id' => 'unclosed-block-open-space', 'source' => '{% '],
    ['id' => 'unclosed-var-open', 'source' => '{{'],
    ['id' => 'unclosed-var-open-space', 'source' => '{{ '],
    ['id' => 'unclosed-var-lexertest', 'source' => "\n\n{{\n\nbar\n\n\n"],
    ['id' => 'unclosed-block-lexertest', 'source' => "\n\n{%\n\nbar\n\n\n"],
    ['id' => 'unclosed-interpolation-space', 'source' => '{{ "#{ x '],
    ['id' => 'unclosed-inline-comment-swallows', 'source' => '{{ x #}}'],
    ['id' => 'unclosed-inline-comment-number', 'source' => '{{ 1 #}}'],
    ['id' => 'unclosed-block-crlf', 'source' => "{% if x\r\n"],
    ['id' => 'unclosed-trim-openers', 'source' => '{%-'],
    ['id' => 'unclosed-trim-var', 'source' => '{{-'],
    ['id' => 'unclosed-trim-var-space', 'source' => '{{- '],
    ['id' => 'unclosed-var-newline', 'source' => "{{ x\n"],
    ['id' => 'unclosed-second-var', 'source' => '{{ x }} {{ y'],

    // comments
    ['id' => 'comment-basic', 'source' => '{# a #}b'],
    ['id' => 'comment-trim-both', 'source' => "{#- a -#}  b"],
    ['id' => 'comment-trim-left', 'source' => 'a  {#- x #}'],
    ['id' => 'comment-eats-newline', 'source' => "a\n{# x #}\nb"],
    ['id' => 'comment-eats-crlf', 'source' => "a\n{# x #}\r\nb"],
    ['id' => 'comment-eats-cr', 'source' => "a\n{# x #}\rb"],
    ['id' => 'comment-line-trim-right', 'source' => "{# a ~#}  \n b"],
    ['id' => 'comment-line-trim-left', 'source' => "  {#~ a #}"],
    ['id' => 'comment-hash-brace', 'source' => '{#}#}after'],
    ['id' => 'comment-unclosed-after-print', 'source' => '{{ x }}{#}'],
    ['id' => 'comment-unclosed', 'source' => "line1\n{# a "],
    ['id' => 'comment-with-tags', 'source' => '{# {{ x }} #}'],
    ['id' => 'comment-empty-doc-like', 'source' => '{##}'],
    ['id' => 'comment-empty-doc-like-text', 'source' => '{##}b'],
    ['id' => 'doc-mixed', 'source' => '{## doc ##}a{##}b{## d2 -##}  c'],
    ['id' => 'doc-hash-closer', 'source' => '{## doc #}{{ x }}'],
    ['id' => 'doc-skips-newline-text', 'source' => "{## a ##}\n{{ x }}"],
    ['id' => 'doc-skips-space-text', 'source' => '{## a ##} {{ x }}'],
    ['id' => 'doc-attaches-to-text', 'source' => '{## a ##}text{{ x }}'],
    ['id' => 'doc-crlf', 'source' => "{## multi\r\n line ##}{{ x }}"],
    ['id' => 'doc-blank', 'source' => '{##  ##}{{ x }}'],
    ['id' => 'doc-joined', 'source' => '{## a ##}{## b ##}{{ x }}'],
    ['id' => 'doc-cleared-by-verbatim', 'source' => '{## x ##}{% verbatim %}v{% endverbatim %}{{ y }}'],
    ['id' => 'doc-cleared-by-line', 'source' => '{## x ##}{% line 3 %}{{ y }}'],
    ['id' => 'doc-inline', 'source' => "{{ x ## inline doc\n }}"],
    ['id' => 'doc-inline-two', 'source' => "{{ x ## d1\n ## d2\n }}"],
    ['id' => 'inline-comment', 'source' => "{{ x # c\n }}"],
    ['id' => 'inline-comment-cr', 'source' => "{{ x # c\r y }}"],
    ['id' => 'doc-trim-open', 'source' => '{##- a ##}'],
    ['id' => 'doc-line-trim-open', 'source' => '{##~ a ##}'],
    ['id' => 'doc-trim-both', 'source' => 'a {##- doc -##} b'],
    ['id' => 'doc-line-trim-close', 'source' => "{## a ~##}  b"],
    ['id' => 'comment-inline-hash', 'source' => "{#\n            Some regular comment # this is an inline comment\n        #}"],
    ['id' => 'doc-unclosed', 'source' => '{## a'],
    ['id' => 'doc-opener-only', 'source' => '{##'],
    ['id' => 'comment-opener-only', 'source' => '{#'],
    ['id' => 'comment-hash-only', 'source' => '{# #'],
    ['id' => 'comment-lines', 'source' => "{# a #}\n\n{{ x }}"],
    ['id' => 'doc-at-eof', 'source' => '{## a ##}'],
    ['id' => 'doc-in-if', 'source' => '{% if x %}{## doc ##}{% endif %}'],
    ['id' => 'doc-hashes', 'source' => '{## ## ##}'],
    ['id' => 'doc-newline-closer', 'source' => "{## a\n#}"],
    ['id' => 'doc-triple-hash', 'source' => '{### a ###}'],
    ['id' => 'comment-trim-doc-closer', 'source' => '{# a -##}'],
    ['id' => 'comment-line-trim-both', 'source' => "{#~ a ~#}\t \nb"],
    ['id' => 'doc-after-print', 'source' => '{{ 1 }}{## d ##}'],
    ['id' => 'comment-multiline-lines', 'source' => "{# a\r\nb #}{{ x }}"],

    // whitespace control
    ['id' => 'ws-trim-both', 'source' => 'a  {{- 1 -}}  b'],
    ['id' => 'ws-line-trim-both', 'source' => "a \n {{~ 1 ~}} \n b"],
    ['id' => 'ws-form-feed', 'source' => "a\f{{- 1 -}}\fb"],
    ['id' => 'ws-nul', 'source' => "a\0{{- 1 -}}\0b"],
    ['id' => 'ws-if-trim', 'source' => "{%- if true -%}\n  a  \n{%- endif -%}"],
    ['id' => 'ws-block-eats-newline', 'source' => "x {% if true %}\ny{% endif %}\nz"],
    ['id' => 'ws-print-keeps-newline', 'source' => "{{ 1 }}\n{{ 2 }}"],
    ['id' => 'ws-crlf-text', 'source' => "a\r\nb"],
    ['id' => 'ws-crlf-prints', 'source' => "{{ 1 }}\r\n{{ 2 }}"],
    ['id' => 'ws-crlf-blocks', 'source' => "{% if true %}\r\nX{% endif %}\r\nY"],
    ['id' => 'ws-cr-blocks', 'source' => "{% if true %}\rX{% endif %}\rY"],
    ['id' => 'ws-line-trim-block', 'source' => "a {%~ if true ~%} \n b"],
    ['id' => 'ws-trim-then-number', 'source' => '{{-1}}'],
    ['id' => 'ws-number-trim-close', 'source' => '{{ 5-}}{{ 5 -}}'],
    ['id' => 'ws-empty-trimmed-print', 'source' => '{{- -}}'],
    ['id' => 'ws-line-trim-comment', 'source' => "a {#~ c ~#} \n b"],
    ['id' => 'ws-line-trim-tabs', 'source' => "a\t{%~ x ~%}\t\nb"],
    ['id' => 'ws-vertical-tab', 'source' => "a\x0b{{~ 1 ~}}\x0bb"],
    ['id' => 'ws-line-trim-keeps-crlf', 'source' => "a \r\n{{~ 1 }}"],
    ['id' => 'ws-trim-crlf', 'source' => "{{ 1 -}}\r\n\r\nb"],
    ['id' => 'ws-block-trim-newlines', 'source' => "{% x -%}\n\n b"],
    ['id' => 'ws-block-one-newline', 'source' => "{% x %}\n\nb"],
    ['id' => 'ws-block-line-trim', 'source' => "{% x ~%}  \n b"],
    ['id' => 'ws-line-chars', 'source' => "{{ 1 ~}}\t\0\x0b\n"],
    ['id' => 'ws-trim-before-block', 'source' => "a\n\n{%- x %}"],
    ['id' => 'ws-plain', 'source' => 'a {% x %} b'],
    ['id' => 'ws-print-newline-end', 'source' => "{{ x }}\n"],
    ['id' => 'ws-comment-newlines', 'source' => "{# x #}\n\n"],
    ['id' => 'ws-comment-trim-newlines', 'source' => "{#- x -#}\n\n y"],
    ['id' => 'ws-form-feed-close', 'source' => "{{ x \f}}"],
    ['id' => 'ws-form-feed-newline-close', 'source' => "{{ x \f\n}}"],
    ['id' => 'ws-vertical-tab-close', 'source' => "{{ x\x0b}}"],

    // verbatim
    ['id' => 'verbatim-newlines', 'source' => "{% verbatim %}\nx{{ y }}{% endverbatim %}\nY"],
    ['id' => 'verbatim-line-trim', 'source' => "A {%~ verbatim ~%}  x  {%~ endverbatim ~%}  \n B"],
    ['id' => 'verbatim-trim', 'source' => "A {%- verbatim -%}  x  {%- endverbatim -%}  \n B"],
    ['id' => 'verbatim-unclosed', 'source' => '{% verbatim %}abc'],
    ['id' => 'verbatim-unclosed-line', 'source' => "\n{% verbatim %}\nabc"],
    ['id' => 'verbatim-not-nested', 'source' => '{% verbatim %}{% verbatim %}x{% endverbatim %}y{% endverbatim %}'],
    ['id' => 'verbatim-with-argument', 'source' => '{% verbatim x %}'],
    ['id' => 'raw-is-a-tag', 'source' => '{% raw %}x{% endraw %}'],
    ['id' => 'verbatim-compact', 'source' => '{%verbatim%}x{%endverbatim%}'],
    ['id' => 'verbatim-crlf', 'source' => "{% verbatim %}a\r\nb{% endverbatim %}"],
    ['id' => 'verbatim-line-quirk', 'source' => "{% verbatim %}\na\nb\n{% endverbatim %}{{ x }}"],
    ['id' => 'verbatim-trim-inner', 'source' => '{% verbatim -%}  x  {%- endverbatim %}'],
    ['id' => 'verbatim-comment-inside', 'source' => '{% verbatim %}{# x #}{% endverbatim %}'],
    ['id' => 'verbatim-multiline-tags', 'source' => "{%\nverbatim\n%}x{%\nendverbatim\n%}y"],
    ['id' => 'verbatim-trim-close', 'source' => "{% verbatim %}x{% endverbatim -%}\n\ny"],
    ['id' => 'verbatim-line-trim-close', 'source' => "{% verbatim %}x{% endverbatim ~%} \t\ny"],
    ['id' => 'verbatim-mixed-trims', 'source' => '{%- verbatim %}x{%~ endverbatim %}'],
    ['id' => 'verbatim-then-print', 'source' => '{% verbatim %}{{ x }}{%endverbatim%}{{ y }}'],
    ['id' => 'verbatim-unterminated-closer', 'source' => '{% verbatim %}x{% endverbatim'],
    ['id' => 'verbatim-trim-text', 'source' => '{% verbatim %}x  {%- endverbatim %}'],
    ['id' => 'verbatim-trim-before', 'source' => 'a  {%- verbatim %}x{% endverbatim %}'],
    ['id' => 'verbatim-line-trim-compact', 'source' => '{% verbatim %}  x  {%~endverbatim%}'],
    ['id' => 'verbatim-string-inside', 'source' => '{% verbatim %}{{ "#{" }}{% endverbatim %}'],

    // line directive
    ['id' => 'line-lexertest', 'source' => "foo\nbar\n{% line 10 %}\n{{\nbaz\n}}\n"],
    ['id' => 'line-lexertest-inline', 'source' => "foo\nbar{% line 10 %}{{\nbaz\n}}\n"],
    ['id' => 'line-then-parse-error', 'source' => "{% line 5 %}{{ x }}\n{{ y"],
    ['id' => 'line-zero', 'source' => '{% line 0 %}{{ x'],
    ['id' => 'line-saturates', 'source' => '{% line 99999999999999999999 %}{{ x }}'],
    ['id' => 'line-int-max', 'source' => '{% line 9223372036854775807 %}{{ x }}'],
    ['id' => 'line-trim-right-not-recognized', 'source' => '{% line 5 -%}'],
    ['id' => 'line-trim-left', 'source' => '{%- line 7 %}'],
    ['id' => 'line-compact', 'source' => '{%line 3%}x'],
    ['id' => 'line-trims-text', 'source' => "a {%- line 7 %}\nb"],
    ['id' => 'line-keeps-newline', 'source' => "{% line 5 %}\n"],
    ['id' => 'line-multiline-tag', 'source' => "{%\nline\n5\n%}{{ x }}"],
    ['id' => 'line-leading-zeros', 'source' => '{% line 007 %}{{ x }}'],
    ['id' => 'line-negative', 'source' => '{% line -5 %}'],
    ['id' => 'line-garbage', 'source' => '{% line 5x %}'],
    ['id' => 'line-missing-number', 'source' => '{% line %}'],
    ['id' => 'line-line-trim', 'source' => '{% line 5 ~%}'],
    ['id' => 'line-error-line', 'source' => "{% line 2 %}\n{{ x \$ }}"],
    ['id' => 'line-deprecation-line', 'source' => "{% line 3 %}{{ 'a\\q' }}"],
    ['id' => 'line-zero-error', 'source' => "{% line 0 %}{{ x \$ }}"],
    ['id' => 'line-unclosed-bracket', 'source' => '{% line 10 %}{{ (1 }}'],

    // carriage returns and line counting
    ['id' => 'cr-line-count', 'source' => "a\rb{{ x }}"],
    ['id' => 'crlf-line-count', 'source' => "a\r\n\r\nb{{ x }}"],
    ['id' => 'crlf-before-close', 'source' => "{{ x\r\n}}"],
    ['id' => 'crlf-inside-print', 'source' => "{{\r\nx\r\n}}"],
    ['id' => 'crlf-block', 'source' => "{% x\r\n%}\r\ny"],
    ['id' => 'crlf-unexpected', 'source' => "\r\n\r\n{{ \$ }}"],

    // misc
    ['id' => 'bom', 'source' => "\xef\xbb\xbf  {% extends 'b' %}"],
    ['id' => 'open-at-end', 'source' => 'a{{'],
    ['id' => 'block-open-at-end', 'source' => 'a{%'],
    ['id' => 'comment-open-at-end', 'source' => 'a{#'],
    ['id' => 'empty-print', 'source' => '{{}}'],
    ['id' => 'empty-block', 'source' => '{%%}'],
    ['id' => 'empty-print-space', 'source' => '{{ }}'],
    ['id' => 'empty-block-space', 'source' => '{% %}'],
    ['id' => 'extra-closers', 'source' => '{{ x }}}}'],
    ['id' => 'triple-braces', 'source' => '{{{ x }}}'],
    ['id' => 'block-open-percent', 'source' => '{%%'],
    ['id' => 'filter-call', 'source' => '{{ x|e("html") }}'],
    ['id' => 'long-text', 'source' => str_repeat('ab', 500)],
    ['id' => 'long-name', 'source' => '{{ '.str_repeat('x', 1000).' }}'],

    // PCRE JIT stack limits of PHP's preg_match() (192 KiB JIT stack)
    ['id' => 'jit-sq-string-8190', 'parts' => [["{{ '", 1], ['\\n', 8190], ["' }}", 1]]],
    ['id' => 'jit-sq-string-8191', 'parts' => [["{{ '", 1], ['\\n', 8191], ["' }}", 1]]],
    ['id' => 'jit-sq-string-quotes-8191', 'parts' => [["{{ 'a", 1], ["\\'", 8191], ["' }}", 1]]],
    ['id' => 'jit-dq-string-8190', 'parts' => [['{{ "', 1], ['\\n', 8190], ['" }}', 1]]],
    ['id' => 'jit-dq-string-8191', 'parts' => [['{{ "', 1], ['\\n', 8191], ['" }}', 1]]],
    ['id' => 'jit-dq-part-4914', 'parts' => [['{{ "', 1], ['\\n', 4913], ['#" }}', 1]]],
    ['id' => 'jit-dq-part-4915', 'parts' => [['{{ "', 1], ['\\n', 4914], ['#" }}', 1]]],
    ['id' => 'jit-dq-part-hashes-4915', 'parts' => [['{{ "', 1], ['a#', 4915], ['" }}', 1]]],
    ['id' => 'jit-dq-part-before-interpolation', 'parts' => [['{{ "', 1], ['#', 4914], ['#{x}" }}', 1]]],
    ['id' => 'jit-dq-part-after-interpolation', 'parts' => [['{{ "#{x}', 1], ['#', 4915], ['" }}', 1]]],
    ['id' => 'jit-number-4911', 'parts' => [['{{ 1', 1], ['_1', 4911], [' }}', 1]]],
    ['id' => 'jit-number-4912', 'parts' => [['{{ 1', 1], ['_1', 4912], [' }}', 1]]],
    ['id' => 'jit-number-fraction-4909', 'parts' => [['{{ 1.1', 1], ['_1', 4909], [' }}', 1]]],
    ['id' => 'jit-number-fraction-4910', 'parts' => [['{{ 1.1', 1], ['_1', 4910], [' }}', 1]]],
    ['id' => 'jit-number-exponent-attempt-4910', 'parts' => [['{{ 1', 1], ['_1', 4910], ['e }}', 1]]],
    ['id' => 'jit-number-exponent-attempt-4911', 'parts' => [['{{ 1', 1], ['_1', 4911], ['e }}', 1]]],
    ['id' => 'jit-number-fraction-exponent-4907', 'parts' => [['{{ 1', 1], ['_1', 4907], ['.1e1 }}', 1]]],
    ['id' => 'jit-number-fraction-exponent-4908', 'parts' => [['{{ 1', 1], ['_1', 4908], ['.1e1 }}', 1]]],
    ['id' => 'jit-number-signed-exponent-attempt', 'parts' => [['{{ 1.1', 1], ['_1', 4909], ['e+ }}', 1]]],

    // TokenStream::expect()
    ['id' => 'expect-float-value', 'source' => '{{ 1 1e20 }}', 'expect' => [2, TK_VAR_END, null, null]],
    ['id' => 'expect-zero', 'source' => '{{ 1 0 }}', 'expect' => [2, TK_VAR_END, null, null]],
    ['id' => 'expect-zero-string', 'source' => '{{ 1 "0" }}', 'expect' => [2, TK_VAR_END, null, null]],
    ['id' => 'expect-float', 'source' => '{{ 1 1.5 }}', 'expect' => [2, TK_VAR_END, null, null]],
    ['id' => 'expect-zero-float', 'source' => '{{ 1 0.0 }}', 'expect' => [2, TK_VAR_END, null, null]],
    ['id' => 'expect-eof', 'source' => '{{ x', 'expect' => [2, TK_VAR_END, null, null]],
    ['id' => 'expect-string', 'source' => "{{ 'a' 'b' }}", 'expect' => [2, TK_VAR_END, null, null]],
    ['id' => 'expect-name-after-number', 'source' => '{{ 1__2 }}', 'expect' => [2, TK_VAR_END, null, null]],
    ['id' => 'expect-column-crlf', 'source' => "x\r\n{{ 1__2 }}", 'expect' => [3, TK_VAR_END, null, null]],
    ['id' => 'expect-closing-line', 'source' => "{% from 'forms.twig'\n  %}", 'expect' => [3, TK_NAME, 'import', null]],
    ['id' => 'expect-message', 'source' => '{{ {"a"} }}', 'expect' => [3, TK_PUNCTUATION, ':', 'A mapping key must be followed by a colon (:)']],
    ['id' => 'expect-message-2', 'source' => '{{ [1, 2 3] }}', 'expect' => [5, TK_PUNCTUATION, ',', 'A sequence element must be followed by a comma']],
    ['id' => 'expect-success', 'source' => '{% if x %}', 'expect' => [3, TK_BLOCK_END, null, null]],
    ['id' => 'expect-eof-end', 'source' => '{{ x }}', 'expect' => [3, TK_EOF, null, null]],
    ['id' => 'expect-line-zero', 'source' => '{% line 0 %}{{ x', 'expect' => [2, TK_VAR_END, null, null]],
    ['id' => 'expect-value-mismatch', 'source' => '{{ x }}', 'expect' => [1, TK_NAME, 'y', null]],
    ['id' => 'expect-number-zero', 'source' => '{{ 0 }}', 'expect' => [1, TK_NAME, null, null]],
    ['id' => 'expect-string-zero', 'source' => '{{ "0" }}', 'expect' => [1, TK_NAME, null, null]],
    ['id' => 'expect-infinite', 'source' => '{{ 1e1000 }}', 'expect' => [1, TK_NAME, null, null]],
    ['id' => 'expect-custom-message', 'source' => '{{ x }}', 'expect' => [1, TK_OPERATOR, null, 'Custom']],
    ['id' => 'expect-falsy-message', 'source' => '{{ x }}', 'expect' => [1, TK_OPERATOR, '0', '0']],
    ['id' => 'expect-bc-operator-value', 'source' => '{{ 1, 2 }}', 'expect' => [2, TK_OPERATOR, '(', null]],
    ['id' => 'expect-bc-operator-type', 'source' => '{{ 1, 2 }}', 'expect' => [2, TK_OPERATOR, null, null]],
    ['id' => 'expect-line-column', 'source' => "{{\n\n  x }}", 'expect' => [1, TK_NUMBER, null, null]],
    ['id' => 'expect-column-utf8', 'source' => '{{ é x }}', 'expect' => [2, TK_VAR_END, null, null]],
    ['id' => 'expect-verbatim-text', 'source' => '{% verbatim %}x{% endverbatim %}', 'expect' => [0, TK_BLOCK_START, null, null]],
    ['id' => 'expect-verbatim-text-line', 'source' => "a\n{% verbatim %}\nx\n{% endverbatim %}", 'expect' => [1, TK_NAME, null, null]],
    ['id' => 'expect-text-value', 'source' => "  \n x", 'expect' => [0, TK_NAME, null, null]],
    ['id' => 'expect-name-zero-template', 'source' => '{{ x }}', 'name' => '0', 'expect' => [1, TK_NUMBER, null, null]],
    ['id' => 'expect-empty-template-name', 'source' => '{{ x }}', 'name' => '', 'expect' => [1, TK_NUMBER, null, null]],
    ['id' => 'expect-line-directive', 'source' => "{% line 5 %}{{ x }}\n{{ y", 'expect' => [6, TK_VAR_END, null, null]],
    ['id' => 'expect-null-safe-number', 'source' => '{{ true ?.5 : 1 }}', 'expect' => [4, TK_VAR_END, null, null]],
    ['id' => 'expect-spread-number', 'source' => '{{ 1...5 }}', 'expect' => [2, TK_VAR_END, null, null]],
    ['id' => 'expect-types-elvis', 'source' => "{% types a ? : 'x' %}", 'expect' => [3, TK_PUNCTUATION, ':', 'A type name must be followed by a colon (:)']],
    ['id' => 'appendix-blocks-newlines', 'source' => "{% if true %}\nX{% endif %}\nY{{ 1 }}\nZ"],
    ['id' => 'appendix-block-name-minus', 'source' => '{% block a-b %}'],
    ['id' => 'error-template-name-zero', 'source' => '{{ $ }}', 'name' => '0'],
    ['id' => 'error-template-name-quote', 'source' => '{{ $ }}', 'name' => 'dir/a "b".twig'],

    // custom options
    ['id' => 'opt-lexertest-overriding', 'source' => '[# comment #]{# variable #}/# if true #/true/# endif #/', 'options' => ['tag_comment' => ['[#', '#]'], 'tag_block' => ['/#', '#/'], 'tag_variable' => ['{#', '#}']]],
    ['id' => 'opt-custom-doc-comment', 'source' => '[## doc ##][##]x{# y #}', 'options' => ['tag_comment' => ['[#', '#]']]],
    ['id' => 'opt-space-in-delimiter', 'source' => 'A{{ x }}B', 'options' => ['tag_variable' => ['{ {', '}}']]],
    ['id' => 'opt-space-in-delimiter-2', 'source' => 'A{ { x }}B', 'options' => ['tag_variable' => ['{ {', '}}']]],
    ['id' => 'opt-nel-in-delimiter', 'source' => "A{\x85{ x }}B{{ y }}", 'options' => ['tag_variable' => ["{\x85{", '}}']]],
    ['id' => 'opt-space-in-closer', 'source' => '{{ x } }{{ y }}', 'options' => ['tag_variable' => ['{{', '} }']]],
    ['id' => 'opt-space-in-line-closer', 'source' => "{% line 4 % }\n{{ x }}{% line 7 %}{{ y }}", 'options' => ['tag_block' => ['{%', '% }']]],
    ['id' => 'opt-empty-trim', 'source' => '{{ 1 }}a {{- 1 }}', 'options' => ['whitespace_trim' => '']],
    ['id' => 'opt-dollar-interpolation', 'source' => '{{ "a${b}c" }}{{ "a#{b}c" }}', 'options' => ['interpolation' => ['${', '}']]],
    ['id' => 'opt-double-brace-interpolation-end', 'source' => '{{ "a#{ b }}c" }}{{ "a#{ {x: 1} }}c" }}', 'options' => ['interpolation' => ['#{', '}}']]],
    ['id' => 'opt-space-interpolation', 'source' => '{{ "a#{ b } c" }}', 'options' => ['interpolation' => ['#{', ' }']]],
    ['id' => 'opt-erb-style', 'source' => '<%= x %><% if y %>z<% endif %>', 'options' => ['tag_block' => ['<%', '%>'], 'tag_variable' => ['<%=', '%>']]],
    ['id' => 'opt-same-block-and-var', 'source' => '{% x %}', 'options' => ['tag_variable' => ['{%', '%}']]],
    ['id' => 'opt-line-chars', 'source' => "{{ 1 ~}} xx\ny{{~ 2 }}ax {{~ 3 }}", 'options' => ['whitespace_line_chars' => ' x']],
    ['id' => 'opt-trim-chars', 'source' => 'a {{+ 1 +}} b {{* 2 *}} c', 'options' => ['whitespace_trim' => '+', 'whitespace_line_trim' => '*']],
    ['id' => 'opt-hash-comment-closer', 'source' => '{# a #b{## c #d', 'options' => ['tag_comment' => ['{#', '#']]],
    ['id' => 'opt-empty-variable-opener', 'source' => 'a{%b%}c', 'options' => ['tag_variable' => ['', '}}']]],
    ['id' => 'opt-long-delimiters', 'source' => "<?twig x ?>\n<?twig- y -?>", 'options' => ['tag_variable' => ['<?twig', '?>']]],
    ['id' => 'opt-verbatim-custom', 'source' => '[% verbatim %]{{ x }}[%- endverbatim -%] y', 'options' => ['tag_block' => ['[%', '%]']]],

    // custom expression parsers (operator list)
    ['id' => 'ops-lexertest-overlapping', 'source' => '{{ custom not 1 }}{{ custom 1 }}{{ shared 1 }}{{ not false }}{{ 1 not in [2] }}', 'parsers' => [['unary', 'custom', ['shared']], ['unary', 'custom not', ['shared']]]],
    ['id' => 'ops-non-ascii', 'source' => '{{ нет a }}{{ aнет }}{{ a ≠ b }}', 'parsers' => [['unary', 'нет', []], ['binary', '≠', []]]],
    ['id' => 'ops-letter-digit', 'source' => '{{ a x2 b }}{{ a x2(b) }}{{ x2 }}', 'parsers' => [['binary', 'x2', []]]],
    ['id' => 'ops-spaced-symbols', 'source' => "{{ a <  > b }}{{ a <\n> b }}{{ a <> b }}", 'parsers' => [['binary', '< >', []]]],

    // operator look-around details
    ['id' => 'op-cr-dot-and', 'source' => "{{ x\r.and }}{{ x\r|and }}"],
    ['id' => 'op-dot-tab-and', 'source' => "{{ x.\tand }}{{ x|\x0cand }}{{ x.\x0band }}"],
    ['id' => 'op-dot-newline-twice-and', 'source' => "{{ x.\n\nand }}"],
    ['id' => 'op-not-vt-ff-in', 'source' => "{{ a not\x0bin b }}{{ a not\x0cin b }}{{ a not \r\n in b }}"],
    ['id' => 'op-lookahead-vt-ff', 'source' => "{{ not\x0bx }}{{ not\x0cx }}{{ not\rx }}"],
    ['id' => 'op-lookahead-close-paren', 'source' => '{{ (not) }}'],
    ['id' => 'op-lookahead-nul', 'source' => "{{ a and\0 }}"],
    ['id' => 'opt-operator-at-start', 'source' => '<not x><in x><|in x><.in x>', 'options' => ['tag_variable' => ['<', '>']]],
    // Lexer::lexData() picks the delimiter with a PHP switch, i.e. the loose ==: "1.0" == "1"
    ['id' => 'opt-numeric-delimiters', 'source' => 'a1.0 x %}b1 y %}c', 'options' => ['tag_block' => ['1', '%}'], 'tag_variable' => ['1.0', '}}']]],

    // reference/twig/syntax.md §2.16 and §2.17 as encoded in tests/syntax_reference_test.rs
    ['id' => 'ref-ws-rtrim-set', 'source' => "a \t\n\r\0\x0b{{- 1 }}"],
    ['id' => 'ref-ws-rtrim-form-feed', 'source' => "a\x0c{{- 1 }}"],
    ['id' => 'ref-ws-trim-block-open', 'source' => "a \n{%- x %}"],
    ['id' => 'ref-ws-trim-comment-open', 'source' => "a \n{#- c #}"],
    ['id' => 'ref-ws-trim-doc-open', 'source' => "a \n{##- d ##}"],
    ['id' => 'ref-ws-line-trim-set', 'source' => "a\n \t\0\x0b{{~ 1 }}"],
    ['id' => 'ref-ws-line-trim-keeps-crlf', 'source' => "a \r\n{{~ 1 }}"],
    ['id' => 'ref-ws-line-trim-block-open', 'source' => "a\n \t{%~ x %}"],
    ['id' => 'ref-ws-line-trim-comment-open', 'source' => "a\n \t{#~ c #}"],
    ['id' => 'ref-ws-trim-close-set', 'source' => "{{ 1 -}} \t\n\x0b\x0c\r b"],
    ['id' => 'ref-ws-trim-close-nul', 'source' => "{{ 1 -}}\0b"],
    ['id' => 'ref-ws-trim-block-close', 'source' => "{% x -%}\n\n b"],
    ['id' => 'ref-ws-trim-comment-close', 'source' => "{# c -#}\n\n b"],
    ['id' => 'ref-ws-trim-doc-close', 'source' => "{## d -##}\n\n b"],
    ['id' => 'ref-ws-line-trim-close-set', 'source' => "{{ 1 ~}} \t\0\x0b\nb"],
    ['id' => 'ref-ws-line-trim-block-close', 'source' => "{% x ~%}  \n b"],
    ['id' => 'ref-ws-line-trim-comment-close', 'source' => "{# c ~#}\t \nb"],
    ['id' => 'ref-ws-line-trim-doc-close', 'source' => '{## d ~##}  b'],
    ['id' => 'ref-ws-block-crlf', 'source' => "{% x %}\r\n\r\nb"],
    ['id' => 'ref-ws-block-cr', 'source' => "{% x %}\r\rb"],
    ['id' => 'ref-ws-comment-newline', 'source' => "{# c #}\n\nb"],
    ['id' => 'ref-ws-print-keeps-newline', 'source' => "{{ x }}\nb"],
    ['id' => 'ref-ws-verbatim-plain', 'source' => "{% verbatim %}\nx{% endverbatim %}\nb"],
    ['id' => 'ref-ws-line-plain', 'source' => "{% line 5 %}\nb"],
    ['id' => 'ref-ws-empty-comment-trims', 'source' => 'a {#--#} b'],
    ['id' => 'ref-err-unclosed-bracket', 'source' => "{{ [1,\n2"],
    ['id' => 'ref-err-unclosed-paren', 'source' => "{{ (\n1"],
    ['id' => 'ref-err-unclosed-brace', 'source' => "{{ {a:\n1"],
    ['id' => 'ref-err-unclosed-string-line', 'source' => "\n{{ \"abc }}"],
    ['id' => 'ref-err-unclosed-interpolation', 'source' => '{{ "#{ x'],
    ['id' => 'ref-err-unclosed-block', 'source' => "\n{%\n\nx\n"],
    ['id' => 'ref-err-unclosed-variable', 'source' => "{{\nx\n"],
    ['id' => 'ref-err-bang', 'source' => '{{ ! }}'],
    ['id' => 'ref-err-verbatim-line', 'source' => "{%\nverbatim\n%}abc"],
    ['id' => 'ref-err-comment-line', 'source' => "a\n\n{# x\n"],
    ['id' => 'ref-err-unexpected-bracket', 'source' => '{{ ] }}'],
    ['id' => 'ref-err-mismatch', 'source' => '{{ [1) }}'],
    ['id' => 'ref-err-mismatch-line', 'source' => "{{ (\n[1\n) }}"],
    ['id' => 'ref-unclosed-if', 'source' => "{% if x %}\n\nabc"],
];

function enc_str(?string $s)
{
    if (null === $s) {
        return null;
    }

    if (strlen($s) > 512) {
        return ['len' => strlen($s), 'crc32' => hash('crc32b', $s)];
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

function token_type(Token $token): int
{
    return (fn () => $this->type)->call($token);
}

function enc_token(Token $token): array
{
    return [token_type($token), enc_value($token->getValue()), $token->getLine(), $token->getOffset(), enc_str($token->getDocumentation())];
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

$out = [];
foreach ($cases as $case) {
    $code = $case['source'] ?? implode('', array_map(fn ($part) => str_repeat($part[0], $part[1]), $case['parts']));
    $name = $case['name'] ?? 'index';
    $env = new Environment(new ArrayLoader());
    $operators = null;
    if (isset($case['parsers'])) {
        $parsers = [];
        foreach ($case['parsers'] as [$kind, $opName, $aliases]) {
            $parsers[] = 'unary' === $kind
                ? new UnaryOperatorExpressionParser(NotUnary::class, $opName, 50, aliases: $aliases)
                : new BinaryOperatorExpressionParser(AddBinary::class, $opName, 30, aliases: $aliases);
        }
        $env->getExpressionParsers()->add($parsers);
        $operators = [];
        foreach ($env->getExpressionParsers() as $parser) {
            foreach (ExpressionParsers::getOperatorTokensFor($parser) as $token) {
                $operators[] = $token;
            }
        }
    }
    $options = $case['options'] ?? [];
    $lexer = new Lexer($env, $options);

    $deprecations = [];
    $diagnostics = [];
    set_error_handler(function (int $type, string $message) use (&$deprecations, &$diagnostics): bool {
        if (E_USER_DEPRECATED === $type) {
            $deprecations[] = enc_str($message);
        } else {
            $diagnostics[] = enc_str($message);
        }

        return true;
    });

    $res = ['id' => $case['id']];
    if (isset($case['parts'])) {
        $res['source_parts'] = $case['parts'];
    } elseif (mb_check_encoding($code, 'UTF-8')) {
        $res['source'] = $code;
    } else {
        $res['source_b64'] = base64_encode($code);
    }
    if ('index' !== $name) {
        $res['name'] = $name;
    }
    if ($options) {
        $res['options'] = array_map('enc_str_deep', $options);
    }
    if (null !== $operators) {
        $res['operators'] = array_map('enc_str', $operators);
    }
    try {
        $stream = $lexer->tokenize(new Source($code, $name));
        $res['tokens'] = array_map('enc_token', (fn () => $this->tokens)->call($stream));
        if (isset($case['expect'])) {
            [$skip, $type, $value, $message] = $case['expect'];
            for ($i = 0; $i < $skip; ++$i) {
                $stream->next();
            }
            $expect = ['skip' => $skip, 'type' => $type, 'value' => $value, 'message' => $message];
            try {
                $expect['token'] = enc_token($stream->expect($type, $value, $message));
            } catch (Throwable $e) {
                $expect['error'] = enc_error($e);
            }
            $res['expect'] = $expect;
        }
    } catch (Throwable $e) {
        $res['error'] = enc_error($e);
    }
    restore_error_handler();
    $res['deprecations'] = $deprecations;
    if ($diagnostics) {
        $res['diagnostics'] = $diagnostics;
    }
    $out[] = $res;
}

function enc_str_deep($value)
{
    return is_array($value) ? array_map('enc_str', $value) : enc_str($value);
}

// One case per block, one token per line.
$lines = [];
foreach ($out as $res) {
    $fields = [];
    foreach ($res as $key => $value) {
        if ('tokens' === $key) {
            $tokens = array_map(fn ($t) => '    '.j($t), $value);
            $fields[] = '  "tokens": ['.($tokens ? "\n".implode(",\n", $tokens)."\n  " : '').']';
        } else {
            $fields[] = '  '.j($key).': '.j($value);
        }
    }
    $lines[] = "{\n".implode(",\n", $fields)."\n}";
}
echo "[\n", implode(",\n", $lines), "\n]\n";
