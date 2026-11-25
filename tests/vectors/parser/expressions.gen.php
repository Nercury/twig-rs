<?php

/*
 * Generator of the parser vectors: the node tree Twig 3.31.1's Parser builds, or the parse error, plus the
 * deprecations raised while parsing. The built-in cases produce tests/vectors/parser/expressions.json (templates
 * without tags: text and print statements, so every expression parser is exercised); batches given in CASES_FILE
 * produce expressions_random.json, tags.json, statements.json and statements_random.json.
 *
 * Regenerate from the repository root (Git Bash on Windows needs MSYS_NO_PATHCONV=1):
 *
 *     MSYS_NO_PATHCONV=1 docker run --rm -i twig-oracle:3.31.1 php < tests/vectors/parser/expressions.gen.php > tests/vectors/parser/expressions.json
 *
 * For ad-hoc probes, pass the cases as base64-encoded JSON instead of the built-in list:
 *
 *     MSYS_NO_PATHCONV=1 docker run --rm -i -e CASES_B64="$(base64 -w0 cases.json)" twig-oracle:3.31.1 php < tests/vectors/parser/expressions.gen.php
 *
 * or, for larger batches, as a file in a mounted directory (expressions_random.json is generated this way):
 *
 *     MSYS_NO_PATHCONV=1 docker run --rm -i -v "$PWD/tests/vectors/parser:/v" -e CASES_FILE=/v/expressions_random.gen.json twig-oracle:3.31.1 php < tests/vectors/parser/expressions.gen.php > tests/vectors/parser/expressions_random.json
 *
 * `Environment::parse($env->tokenize($source))` runs with the parser's own node visitors only: the escaper,
 * optimizer, yield-ready and sandbox visitors are removed after the extensions are initialized, so the result is what
 * the parser and the CorrectnessNodeVisitor (priority -255) produce. The tree is captured by a node visitor that runs
 * before every other visitor (priority -100000); the CorrectnessNodeVisitor does not change the tree. Every
 * deprecation raised while lexing, parsing and visiting is recorded; an error (from the parser or the
 * CorrectnessNodeVisitor) replaces the dump.
 *
 * Case fields: id, source, optional name (template name, default "index"), path (source path, default ""),
 * ext (a test extension, see make_extension()), extensions (optional core extensions to add: "sandbox").
 * Output per case: id, source (or source_b64), name, path, ext, extensions, dump (or error: {class, message}),
 * deprecations.
 *
 * Dump format (mirrored by twig::ast::dump): one node per line, "<class> @<line>[ #<tag>][ doc=<string>]" where
 * <class> is the PHP class without the "Twig\Node\" prefix; then its attributes as ".<name> = <value>" in
 * attribute order, then pseudo attributes for state that is not an attribute (".(defined_test) = true" when the
 * defined test is enabled, ".(call_parentheses) = <bool>" for macro references), then its children as
 * "[<key>] <child>" indented by two spaces; node-valued attributes come last as "[.<name>] <node>".
 * Values: NULL, true, false, ints, float(<var_export>), "<string>" (\\, \" and \xHH escapes for bytes < 0x20 and
 * 0x7f), [k => v, ...] for arrays, the string form of Stringable objects (e.g. Twig\TwigFilter(upper)).
 */

declare(strict_types=0);

require '/oracle/vendor/autoload.php';

use Twig\Environment;
use Twig\Error\Error as TwigError;
use Twig\ExpressionParser\Infix\BinaryOperatorExpressionParser;
use Twig\ExpressionParser\InfixAssociativity;
use Twig\ExpressionParser\PrecedenceChange;
use Twig\ExpressionParser\Prefix\UnaryOperatorExpressionParser;
use Twig\Extension\AbstractExtension;
use Twig\Extension\SandboxExtension;
use Twig\Sandbox\SecurityPolicy;
use Twig\Loader\ArrayLoader;
use Twig\Node\Expression\MacroReferenceExpression;
use Twig\Node\Expression\SupportDefinedTestInterface;
use Twig\Node\ModuleNode;
use Twig\Node\Node;
use Twig\NodeVisitor\CorrectnessNodeVisitor;
use Twig\NodeVisitor\NodeVisitorInterface;
use Twig\Source;
use Twig\DeprecatedCallableInfo;
use Twig\TwigFilter;
use Twig\TwigFunction;
use Twig\TwigTest;

$cases = [
    // literals
    ['id' => 'lit-int', 'source' => '{{ 1 }}'],
    ['id' => 'lit-float', 'source' => '{{ 1.5 }}{{ 1e3 }}{{ 0.1 }}{{ 1e100 }}{{ 1e1000 }}{{ 1.0 }}'],
    ['id' => 'lit-big-int', 'source' => '{{ 9223372036854775807 }}{{ 9223372036854775808 }}'],
    ['id' => 'lit-string', 'source' => "{{ 'a' }}{{ \"b\" }}{{ '' }}"],
    ['id' => 'lit-string-escapes', 'source' => "{{ 'a\\nb\\\\c\\'d' }}{{ \"tab\\there\" }}"],
    ['id' => 'lit-string-binary', 'source' => "{{ 'caf\xe9' }}"],
    ['id' => 'lit-true-false-null', 'source' => '{{ true }}{{ TRUE }}{{ false }}{{ FALSE }}{{ null }}{{ NULL }}{{ none }}{{ NONE }}'],
    ['id' => 'lit-mixed-case-names', 'source' => '{{ True }}{{ Null }}{{ nOne }}'],
    ['id' => 'lit-name', 'source' => '{{ foo }}{{ _self }}{{ _context }}{{ _charset }}'],
    ['id' => 'lit-name-binary', 'source' => "{{ caf\xe9 }}"],
    ['id' => 'lit-operator-names', 'source' => '{{ in }}{{ is }}{{ and }}{{ or }}{{ matches }}{{ b-and }}'],
    ['id' => 'lit-not-name', 'source' => '{{ not }}'],
    ['id' => 'lit-starts-with-name', 'source' => '{{ starts }}'],
    ['id' => 'lit-empty-print', 'source' => '{{ }}'],
    ['id' => 'lit-dot-start', 'source' => '{{ .5 }}'],
    ['id' => 'lit-close-paren', 'source' => '{{ (() => 1) }}'],
    ['id' => 'lit-punctuation-colon', 'source' => '{{ : }}'],
    ['id' => 'lit-trim-empty', 'source' => '{{- -}}'],

    // text and multiple prints
    ['id' => 'text-only', 'source' => 'Hello'],
    ['id' => 'text-and-print', 'source' => "a\n{{ b }}\nc"],
    ['id' => 'empty-template', 'source' => ''],
    ['id' => 'prints-lines', 'source' => "{{ a }}\n\n{{\nb\n}}"],
    ['id' => 'doc-on-print', 'source' => '{## doc ##}{{ a }}'],
    ['id' => 'doc-inline-in-print', 'source' => "{{ a ## inline\n }}{{ b }}"],

    // strings and interpolation
    ['id' => 'interp-basic', 'source' => '{{ "foo #{bar}" }}'],
    ['id' => 'interp-tail', 'source' => '{{ "foo #{bar} baz" }}'],
    ['id' => 'interp-nested', 'source' => '{{ "foo #{"foo #{bar} baz"} baz" }}'],
    ['id' => 'interp-only', 'source' => '{{ "#{x}" }}'],
    ['id' => 'interp-two', 'source' => '{{ "#{a}#{b}" }}'],
    ['id' => 'interp-then-string', 'source' => '{{ "a#{1}" "b" }}'],
    ['id' => 'interp-lines', 'source' => "{{ \"a\n#{b}\nc#{\nd}\" }}"],
    ['id' => 'interp-empty', 'source' => '{{ "#{}" }}'],
    ['id' => 'interp-two-exprs', 'source' => '{{ "#{ 1 2 }" }}'],
    ['id' => 'string-string', 'source' => "{{ 'a' 'b' }}"],
    ['id' => 'string-string-double', 'source' => '{{ "a" "b" }}'],

    // sequences
    ['id' => 'seq-simple', 'source' => '{{ [1, 2] }}'],
    ['id' => 'seq-trailing-comma', 'source' => '{{ [1, 2, ] }}'],
    ['id' => 'seq-empty', 'source' => '{{ [] }}'],
    ['id' => 'seq-nested', 'source' => '{{ [1, {"a": "b", "b": "c"}] }}'],
    ['id' => 'seq-spread', 'source' => '{{ [1, 2, ...foo] }}'],
    ['id' => 'seq-empty-slots', 'source' => '{{ [1,,2] }}{{ [,1] }}{{ [,] }}{{ [1,,] }}'],
    ['id' => 'seq-missing-comma', 'source' => '{{ [1, 2 3] }}'],
    ['id' => 'seq-key-value', 'source' => '{{ [1, "a": "b"] }}'],
    ['id' => 'seq-lines', 'source' => "{{ [\n1,\n2\n] }}"],
    ['id' => 'seq-spread-lines', 'source' => "{{ [\n...a,\n...b] }}"],

    // mappings
    ['id' => 'map-simple', 'source' => '{{ {"a": "b", "b": "c"} }}'],
    ['id' => 'map-trailing-comma', 'source' => '{{ {"a": "b", "b": "c", } }}'],
    ['id' => 'map-empty', 'source' => '{{ {} }}'],
    ['id' => 'map-nested-seq', 'source' => '{{ {"a": [1, 2], "b": "c"} }}'],
    ['id' => 'map-shorthand', 'source' => '{{ {a, b} }}'],
    ['id' => 'map-all-keys', 'source' => "{{ {a, b: 2, 'c': 3, 4: 5, (1+1): 6, ...{z: 1}, 1.5: 7} }}"],
    ['id' => 'map-spread', 'source' => '{{ {"a": "b", "b": "c", ...otherLetters} }}'],
    ['id' => 'map-spread-first', 'source' => '{{ {...a, b: 1, ...c} }}'],
    ['id' => 'map-paren-key-full-expression', 'source' => '{{ {(a)|upper: 1, (1) + 1: 2} }}'],
    ['id' => 'map-value-only', 'source' => '{{ {"a"} }}'],
    ['id' => 'map-value-after-pair', 'source' => '{{ {"a": "b", 2} }}'],
    ['id' => 'map-bad-key', 'source' => '{{ {[1]: 2} }}'],
    ['id' => 'map-negative-key', 'source' => "{{ {-1: 'a'} }}"],
    ['id' => 'map-interpolated-key', 'source' => '{{ {"a#{1}": 2} }}'],
    ['id' => 'map-operator-key', 'source' => '{{ {and: 1} }}'],
    ['id' => 'map-operator-key-space', 'source' => '{{ {and : 1} }}'],
    ['id' => 'map-missing-comma', 'source' => '{{ {a: 1 b: 2} }}'],
    ['id' => 'map-lines', 'source' => "{{ {\na: 1,\n'b':\n2} }}"],
    ['id' => 'map-key-eof', 'source' => '{{ {a: 1, }}'],

    // grouping, arrow parameter lists
    ['id' => 'group', 'source' => '{{ (1 + 2) * 3 }}'],
    ['id' => 'group-name', 'source' => '{{ (a) }}{{ (max)(1, 2) }}'],
    ['id' => 'group-not-closed', 'source' => '{{ (a b) }}'],
    ['id' => 'group-tuple', 'source' => '{{ (1, 2) }}'],
    ['id' => 'group-list-no-arrow', 'source' => '{{ (a, b) + 1 }}'],
    ['id' => 'group-list-not-names', 'source' => '{{ (a + 1) => 2 }}'],
    ['id' => 'group-list-missing-comma', 'source' => '{{ (a, b c) => 1 }}'],
    ['id' => 'group-nested', 'source' => '{{ ((a)) }}'],
    ['id' => 'group-unary', 'source' => '{{ (-a) }}{{ (not a) }}'],
    ['id' => 'group-assign', 'source' => '{{ (a) = 1 }}'],

    // arrow functions
    ['id' => 'arrow-single', 'source' => '{{ x => x * 2 }}'],
    ['id' => 'arrow-paren-single', 'source' => '{{ (x) => x * 2 }}'],
    ['id' => 'arrow-list', 'source' => '{{ (a, b) => a ~ b }}'],
    ['id' => 'arrow-in-filter', 'source' => '{{ [1, 2]|map(v => v + 1) }}'],
    ['id' => 'arrow-in-filter-two', 'source' => '{{ [1, 2]|map((v, k) => v + k)|join }}'],
    ['id' => 'arrow-bad-left', 'source' => '{{ [1,2]|map(1 => 2) }}'],
    ['id' => 'arrow-empty-list', 'source' => '{{ () => 1 }}'],
    ['id' => 'arrow-body-precedence', 'source' => '{{ x => x ? 1 : 2 }}'],
    ['id' => 'arrow-reserved-name', 'source' => '{{ (true) => 1 }}'],
    ['id' => 'arrow-list-reserved-name', 'source' => '{{ (a, null) => 1 }}'],

    // unary operators and spread
    ['id' => 'unary-neg', 'source' => '{{ -a }}{{ - -2 }}{{ 1 + + 2 }}'],
    ['id' => 'unary-not', 'source' => '{{ not a }}{{ not a and b }}{{ not a * b }}{{ not x is defined }}'],
    ['id' => 'unary-neg-postfix', 'source' => '{{ -a.b }}{{ -a["b"] }}{{ -f() }}'],
    ['id' => 'unary-not-not', 'source' => '{{ not not a }}'],
    ['id' => 'unary-not-paren', 'source' => '{{ not(a) }}{{ not (a) b }}'],
    ['id' => 'unary-neg-power', 'source' => '{{ -1**0 }}{{ (-1)**0 }}{{ -2 ** 2 }}'],
    ['id' => 'spread-print', 'source' => '{{ ...foo }}'],
    ['id' => 'spread-group', 'source' => '{{ (...foo) }}'],
    ['id' => 'spread-neg', 'source' => '{{ -...foo }}'],
    ['id' => 'spread-binary', 'source' => '{{ 1 + ...foo }}'],
    ['id' => 'spread-arrow', 'source' => '{{ (bar => ...foo)(1) }}'],
    ['id' => 'spread-in-group-in-seq', 'source' => '{{ [(...foo)] }}'],
    ['id' => 'spread-neg-in-seq', 'source' => '{{ [1, -...foo] }}'],
    ['id' => 'spread-map-value', 'source' => '{{ {a: ...foo} }}'],
    ['id' => 'spread-map-key', 'source' => '{{ {(...foo): 1} }}'],
    ['id' => 'spread-arg-neg', 'source' => '{{ bar(-...foo) }}'],
    ['id' => 'spread-named-colon', 'source' => '{{ bar(a: ...foo) }}'],
    ['id' => 'spread-named-equal', 'source' => '{{ bar(a = ...foo) }}'],
    ['id' => 'spread-number', 'source' => '{{ 1...5 }}'],

    // binary operators and precedence
    ['id' => 'bin-all', 'source' => '{{ a or b xor c and d b-or e b-xor f b-and g }}'],
    ['id' => 'bin-comparisons', 'source' => '{{ a == b }}{{ a != b }}{{ a <=> b }}{{ a < b }}{{ a > b }}{{ a >= b }}{{ a <= b }}{{ a === b }}{{ a !== b }}'],
    ['id' => 'bin-word-operators', 'source' => '{{ a not in b }}{{ a in b }}{{ a starts with b }}{{ a ends with b }}{{ a has some b }}{{ a has every b }}'],
    ['id' => 'bin-arith', 'source' => '{{ 1 + 2 - 3 * 4 / 5 // 6 % 7 ** 8 ~ 9 }}'],
    ['id' => 'bin-range', 'source' => "{{ 1..3 }}{{ 1..3|join(',') }}"],
    ['id' => 'bin-right-assoc', 'source' => '{{ 2 ** 3 ** 2 }}{{ a ?: b ?: c }}{{ a ?? b ?? c }}'],
    ['id' => 'bin-left-assoc', 'source' => '{{ 1 - 2 - 3 }}{{ a ~ b ~ c }}'],
    ['id' => 'bin-lines', 'source' => "{{ a\n+\nb\n*\nc }}"],
    ['id' => 'bin-matches-valid', 'source' => '{{ a matches "/a/" }}{{ a matches b }}'],
    ['id' => 'bin-matches-invalid', 'source' => "{{ 'a' matches '/a' }}"],
    ['id' => 'bin-matches-number', 'source' => '{{ a matches 3 }}'],
    ['id' => 'bin-matches-null', 'source' => '{{ a matches null }}'],
    ['id' => 'bin-matches-compile', 'source' => '{{ a matches "/(/" }}'],
    ['id' => 'bin-elvis', 'source' => '{{ a ?: b }}{{ a ? : b }}{{ a.b ?: c }}'],
    ['id' => 'bin-null-coalesce', 'source' => '{{ a ?? b }}{{ a.b ?? c }}{{ block("x") ?? 1 }}{{ [1] ?? 2 }}{{ "s" ?? 3 }}'],
    ['id' => 'bin-null-coalesce-func', 'source' => '{{ max(1) ?? 2 }}{{ constant("A") ?? 3 }}'],
    ['id' => 'bin-null-coalesce-invalid', 'source' => '{{ (1 + 2) ?? 3 }}'],
    ['id' => 'bin-null-coalesce-filter', 'source' => "{{ x|upper ?? 'd' }}"],
    ['id' => 'bin-null-coalesce-self', 'source' => '{{ _self ?? 1 }}'],
    ['id' => 'bin-unknown-operator-char', 'source' => '{{ a ! b }}'],
    ['id' => 'bin-missing-right', 'source' => '{{ 1 + }}'],
    ['id' => 'bin-and-paren', 'source' => '{{ a and(b) }}{{ a or (b) }}'],

    // ternary
    ['id' => 'ternary', 'source' => '{{ a ? b : c }}{{ a ? b }}'],
    ['id' => 'ternary-nested', 'source' => '{{ a ? b : c ? d : e }}{{ 1 ? 2 ? 3 : 4 : 5 }}'],
    ['id' => 'ternary-lines', 'source' => "{{ a\n?\nb\n:\nc }}"],
    ['id' => 'ternary-null-safe-number', 'source' => '{{ true ?.5 : 1 }}'],
    ['id' => 'ternary-colon-only', 'source' => '{{ a ? : }}'],

    // assignment and destructuring
    ['id' => 'assign', 'source' => '{{ a = 1 }}{{ a = b = 2 }}'],
    ['id' => 'assign-paren', 'source' => '{{ (a = 1) + 1 }}'],
    ['id' => 'assign-to-constant', 'source' => '{{ 1 = 2 }}'],
    ['id' => 'assign-to-null', 'source' => '{{ none = 1 }}'],
    ['id' => 'assign-to-getattr', 'source' => '{{ a.b = 2 }}'],
    ['id' => 'assign-to-binary', 'source' => '{{ a + b = 1 }}'],
    ['id' => 'assign-chain-constant', 'source' => '{{ a = 1 = 2 }}'],
    ['id' => 'assign-reserved', 'source' => '{{ True = 1 }}'],
    ['id' => 'destructure-seq', 'source' => '{{ [x, y] = [1, 2] }}'],
    ['id' => 'destructure-seq-holes', 'source' => '{{ ([first, , third] = values) }}'],
    ['id' => 'destructure-map', 'source' => '{{ {a, b: c} = m }}'],
    ['id' => 'destructure-map-name', 'source' => '{{ ({name: user_name} = user) }}'],
    ['id' => 'destructure-seq-from-map-syntax', 'source' => '{{ {0: z} = x }}{{ {...a} = b }}'],
    ['id' => 'destructure-empty-seq', 'source' => '{{ [] = values }}'],
    ['id' => 'destructure-empty-map', 'source' => '{{ {} = values }}'],
    ['id' => 'destructure-only-holes', 'source' => '{{ [,] = values }}'],
    ['id' => 'destructure-only-holes-2', 'source' => '{{ [,,] = values }}'],
    ['id' => 'destructure-nested', 'source' => '{{ [a, [b]] = values }}'],
    ['id' => 'destructure-map-nested', 'source' => '{{ {a: [b]} = values }}'],
    ['id' => 'destructure-constant', 'source' => '{{ [1] = values }}'],
    ['id' => 'destructure-map-constant', 'source' => '{{ {a: 1} = values }}'],
    ['id' => 'destructure-reserved', 'source' => '{{ [true] = values }}'],
    ['id' => 'destructure-dynamic-key', 'source' => '{{ {(a): b} = x }}'],
    ['id' => 'destructure-spread-seq', 'source' => '{{ [...a] = b }}'],
    ['id' => 'destructure-lines', 'source' => "{{ [\na,\nb\n]\n=\nc }}"],

    // tests
    ['id' => 'test-simple', 'source' => '{{ a is defined }}{{ a is odd }}{{ a is null }}{{ a is none }}{{ a is empty }}{{ a is iterable }}'],
    ['id' => 'test-not', 'source' => '{{ a is not defined }}{{ a is not even }}'],
    ['id' => 'test-two-words', 'source' => '{{ a is same as b }}{{ a is divisible by 3 }}{{ a is same as(b) }}'],
    ['id' => 'test-two-words-unknown', 'source' => '{{ x is odd foo }}'],
    ['id' => 'test-same-x', 'source' => '{{ 1 is same x }}'],
    ['id' => 'test-divisible-alone', 'source' => '{{ 1 is divisible }}'],
    ['id' => 'test-unknown', 'source' => '{{ 1 is nul }}'],
    ['id' => 'test-unknown-no-suggestion', 'source' => '{{ 1 is foobar }}'],
    ['id' => 'test-args-after-lookup', 'source' => '{{ 1 is nope(1 2) }}'],
    ['id' => 'test-one-mandatory-precedence', 'source' => '{{ 2 is same as 2 + 1 }}{{ 3 is divisible by 2 + 1 }}{{ 1 is same as 1 is same as true }}'],
    ['id' => 'test-one-mandatory-missing', 'source' => '{{ 1 is divisible by }}'],
    ['id' => 'test-constant', 'source' => "{{ a is constant('A') }}{{ a is constant('A', b) }}"],
    ['id' => 'test-named-args', 'source' => '{{ a is same as(value = 1) }}'],
    ['id' => 'test-defined-getattr', 'source' => '{{ a.b.c is defined }}{{ a[b] is defined }}{{ a.b() is defined }}'],
    ['id' => 'test-defined-constant', 'source' => "{{ 'a' is defined }}{{ [1] is defined }}{{ {a: 1} is defined }}"],
    ['id' => 'test-defined-functions', 'source' => "{{ constant('A') is defined }}{{ max(1) is defined }}{{ block('a') is defined }}"],
    ['id' => 'test-defined-invalid', 'source' => '{{ (1 + 2) is defined }}'],
    ['id' => 'test-defined-filter', 'source' => '{{ a|upper is defined }}'],
    ['id' => 'test-defined-self-macro', 'source' => '{{ _self.m is defined }}{{ _self.m() is defined }}'],
    ['id' => 'test-name-missing', 'source' => '{{ a is }}'],
    ['id' => 'test-name-string', 'source' => "{{ a is 'b' }}"],
    ['id' => 'test-not-closing', 'source' => '{{ a is not}}'],
    ['id' => 'test-lines', 'source' => "{{ a\nis\nsame\nas\nb }}"],
    ['id' => 'test-lines-args', 'source' => "{{ a is divisible by(\n3\n)\n}}"],
    ['id' => 'test-not-lines', 'source' => "{{ a\nis not\nnull\n}}"],

    // filters
    ['id' => 'filter-simple', 'source' => '{{ a|upper }}{{ a|upper() }}{{ a|join(", ") }}'],
    ['id' => 'filter-chain', 'source' => '{{ a|upper|lower|trim }}'],
    ['id' => 'filter-named-args', 'source' => '{{ a|join(glue = ", ", and: " and ") }}'],
    ['id' => 'filter-default', 'source' => '{{ a|default }}{{ a|default("x") }}{{ a.b|default("y") }}{{ (a ~ b)|default("z") }}'],
    ['id' => 'filter-default-named', 'source' => '{{ a|default(default = "x") }}'],
    ['id' => 'filter-escape', 'source' => '{{ a|e }}{{ a|escape("js") }}{{ a|raw }}'],
    ['id' => 'filter-unknown', 'source' => '{{ 1|lowe }}'],
    ['id' => 'filter-unknown-no-suggestion', 'source' => '{{ 1|foobar }}'],
    ['id' => 'filter-unknown-suggestions', 'source' => '{{ x|f }}'],
    ['id' => 'filter-args-before-lookup', 'source' => '{{ 1|nope(1 2) }}'],
    ['id' => 'filter-missing-name', 'source' => '{{ x| }}'],
    ['id' => 'filter-string-name', 'source' => "{{ x|'a' }}"],
    ['id' => 'filter-operator-name', 'source' => "{{ 'a'|and }}"],
    ['id' => 'filter-deprecated', 'source' => "{{ 'a'|spaceless }}"],
    ['id' => 'filter-precedence-neg', 'source' => '{{ -1|abs }}{{ (-1)|abs }}{{ - (1|abs) }}'],
    ['id' => 'filter-lines', 'source' => "{{ a\n|\nupper\n(\n1\n) }}"],
    ['id' => 'filter-param-name-not-string', 'source' => '{{ x|join(a.b: 1) }}'],
    ['id' => 'filter-param-name-string', 'source' => '{{ f("a": 2) }}'],

    // functions
    ['id' => 'func-simple', 'source' => '{{ max(1, 2) }}{{ range(1, 3) }}{{ random() }}'],
    ['id' => 'func-named', 'source' => '{{ max(value = 1) }}{{ range(low: 1, high: 3) }}'],
    ['id' => 'func-spread', 'source' => '{{ max(...[1, 5]) }}{{ max(...[1,5], ...[7]) }}'],
    ['id' => 'func-spread-then-normal', 'source' => '{{ max(...[1, 5], 2) }}'],
    ['id' => 'func-trailing-comma', 'source' => '{{ max(1, 2, ) }}'],
    ['id' => 'func-missing-comma', 'source' => '{{ max(1 2) }}'],
    ['id' => 'func-unknown', 'source' => '{{ cycl() }}'],
    ['id' => 'func-unknown-no-suggestion', 'source' => '{{ foobar() }}'],
    ['id' => 'func-not-identifier', 'source' => '{{ max(1,2)(3) }}'],
    ['id' => 'func-string-call', 'source' => '{{ "foo"() }}'],
    ['id' => 'func-method-call-call', 'source' => '{{ x.y.z(1)(2) }}'],
    ['id' => 'func-operator-name', 'source' => '{{ in(1) }}'],
    ['id' => 'func-assign-named', 'source' => '{{ max((a = 1), 2) }}'],
    ['id' => 'func-assign-constant', 'source' => '{{ max(1 = 2) }}'],
    ['id' => 'func-duplicate-named', 'source' => '{{ max(a = 1, a = 2, 3) }}'],
    ['id' => 'func-enum', 'source' => "{{ enum('A') }}{{ enum_cases('A') }}"],
    ['id' => 'func-lines', 'source' => "{{ max\n(\n1,\n2\n) }}"],
    ['id' => 'func-parent-outside-block', 'source' => '{{ parent() }}'],
    ['id' => 'func-block', 'source' => "{{ block('a') }}{{ block('a', 'b') }}{{ block(name = 'c') }}{{ block('a', 'b', 'c') }}"],
    ['id' => 'func-block-missing', 'source' => '{{ block() }}'],
    ['id' => 'func-block-template-only', 'source' => "{{ block(template = 'x') }}"],
    ['id' => 'func-block-twice', 'source' => "{{ block('a', name = 'b') }}"],
    ['id' => 'func-block-unknown-arg', 'source' => "{{ block(foo = 'x', name = 'a') }}"],
    ['id' => 'func-block-unknown-args', 'source' => "{{ block(foo = 'x', bar = 'y', name = 'a') }}"],
    ['id' => 'func-block-positional-after-named', 'source' => "{{ block(name = 'a', 'b') }}"],
    ['id' => 'func-block-spread', 'source' => "{{ block(...['a']) }}"],
    ['id' => 'func-block-lines', 'source' => "{{ block(\n'a'\n,\nfoo\n=\n1\n) }}"],
    ['id' => 'func-attribute', 'source' => "{{ attribute(a, 'b') }}{{ attribute(a, 'b', [1]) }}{{ attribute(variable = a, attribute = 'b') }}"],
    ['id' => 'func-attribute-missing', 'source' => '{{ attribute(a) }}'],
    ['id' => 'func-attribute-named-missing', 'source' => '{{ attribute(variable = a) }}'],
    ['id' => 'func-attribute-in-group', 'source' => "{{ (attribute(a, 'b')) }}"],
    ['id' => 'func-block-in-group', 'source' => "{{ (block('a')) }}"],
    ['id' => 'func-named-args-normalized', 'source' => "{{ block(NAME = 'a') }}{{ block(n_a_m_e = 'b') }}"],

    // attribute access
    ['id' => 'dot-simple', 'source' => '{{ a.b }}{{ a.b.c }}{{ a.0 }}{{ a.0.1 }}{{ a.1e3 }}'],
    ['id' => 'dot-method', 'source' => '{{ a.b() }}{{ a.b(1, 2) }}{{ a.b(x = 1, y = 2) }}'],
    ['id' => 'dot-dynamic', 'source' => '{{ a.(b) }}{{ a.(b ~ "c")() }}'],
    ['id' => 'dot-operator-name', 'source' => '{{ foo.and }}{{ foo.not }}{{ foo.in }}{{ foo.is }}'],
    ['id' => 'dot-null-safe', 'source' => '{{ a?.b }}{{ a?.b.c }}{{ a?.b() }}'],
    ['id' => 'dot-string', 'source' => "{{ x.'y' }}"],
    ['id' => 'dot-trailing', 'source' => '{{ 1. }}'],
    ['id' => 'dot-null-safe-bracket', 'source' => '{{ x?.[1] }}'],
    ['id' => 'dot-empty-paren', 'source' => '{{ x.() }}'],
    ['id' => 'dot-self-macro', 'source' => '{{ _self.m() }}{{ _self.m }}{{ _self.m(1, a = 2) }}{{ _self.(n)() }}'],
    ['id' => 'dot-lines', 'source' => "{{ a\n.\nb\n(\n1\n)\n.\nc }}"],
    ['id' => 'dot-macro-lines', 'source' => "{{ _self\n.\nm\n(\n1\n) }}"],
    ['id' => 'dot-number-attribute-lines', 'source' => "{{ a.\n0 }}"],
    ['id' => 'dot-not-in-name', 'source' => '{{ a.not in }}'],

    // subscripts and slices
    ['id' => 'bracket-simple', 'source' => '{{ a[1] }}{{ a["b"] }}{{ a[b] }}{{ a[b][c] }}'],
    ['id' => 'bracket-slices', 'source' => '{{ a[1:2] }}{{ a[:2] }}{{ a[1:] }}{{ a[:] }}{{ a[-1:] }}'],
    ['id' => 'bracket-slice-three', 'source' => '{{ x[1:2:3] }}'],
    ['id' => 'bracket-empty', 'source' => '{{ x[] }}'],
    ['id' => 'bracket-lines', 'source' => "{{ a\n[\n1\n] }}{{ a\n[\n1\n:\n2\n] }}"],

    // precedence-change deprecations
    ['id' => 'dep-concat-add', 'source' => "{{ '42' ~ 1 + 41 }}{{ 1 + 41 ~ '42' }}"],
    ['id' => 'dep-concat-add-paren', 'source' => "{{ '42' ~ (1 + 41) }}{{ (1 + 41) ~ '42' }}"],
    ['id' => 'dep-null-coalesce', 'source' => "{{ 'notnull' ?? 'foo' ~ '_bar' }}"],
    ['id' => 'dep-not-mul', 'source' => '{{ not 1 * 2 }}{{ not (1 * 2) }}'],
    ['id' => 'dep-neg-filter', 'source' => '{{ -1|abs }}'],
    ['id' => 'dep-null-coalesce-chain', 'source' => '{{ a ?? b == c }}{{ a ?? b or c }}{{ a ?? not b }}'],
    ['id' => 'dep-null-coalesce-in-binary', 'source' => '{{ 1 + (a ?? b) }}{{ 1 + a ?? b }}'],
    ['id' => 'dep-lines', 'source' => "{{ '42'\n~\n1\n+\n41 }}"],
    ['id' => 'dep-name', 'name' => 'some/tpl.twig', 'source' => "{{ '42' ~ 1 + 41 }}"],
    ['id' => 'dep-grouping-resets', 'source' => '{{ (1 + 2) ~ 3 }}{{ not (a ~ b) }}'],

    // error lines and columns
    ['id' => 'err-unclosed-print', 'source' => '{{ x'],
    ['id' => 'err-two-values', 'source' => '{{ 1 0 }}{{ 1 "0" }}'],
    ['id' => 'err-two-values-float', 'source' => '{{ 1 1.5 }}'],
    ['id' => 'err-line-zero', 'source' => '{% line 0 %}{{ x'],
    ['id' => 'err-line-five', 'source' => "{% line 5 %}{{ x }}\n{{ y"],
    ['id' => 'err-multiline-column', 'source' => "\n\n  {{ a b }}"],
    ['id' => 'err-second-line', 'source' => "{{ a }}\n{{ (1 }}"],

    // more edge cases
    ['id' => 'err-line-zero-node-error', 'source' => '{% line 0 %}{{ (1+2) ?? 3 }}'],
    ['id' => 'err-line-zero-unknown-filter', 'source' => '{% line 0 %}{{ x|nope }}'],
    ['id' => 'err-in-interpolation-column', 'source' => "{{ \"a #{ b c }\" }}"],
    ['id' => 'attr-null-arguments', 'source' => "{{ attribute(a, 'b', null) }}{{ attribute(a, 'b', x ~ y) }}{{ attribute(a, 'b', c) }}{{ attribute(a, 'b', [1]) }}"],
    ['id' => 'method-args-reserved-names', 'source' => '{{ a.b(context: 1, this = 2, varargs: 3) }}'],
    ['id' => 'method-args-true-name', 'source' => '{{ a.b(True: 1) }}'],
    ['id' => 'method-args-none-name', 'source' => '{{ a.b(none: 1) }}'],
    ['id' => 'self-macro-true-name', 'source' => '{{ _self.m(True: 1) }}'],
    ['id' => 'dep-concat-chain', 'source' => "{{ 'a' ~ 'b' ~ 1 + 2 ~ 3 - 4 }}"],
    ['id' => 'dep-prefix-filter', 'source' => '{{ -a.b|abs }}{{ +a|abs }}{{ - (a|abs) }}{{ not a|abs }}'],
    ['id' => 'dep-null-coalesce-group', 'source' => "{{ ('a' ?? 'b') ~ 'c' }}{{ 'a' ?? ('b' ~ 'c') }}"],
    ['id' => 'dynamic-attribute-filter', 'source' => '{{ a.(b|upper) }}{{ a?.b?.c }}{{ a?.(b)() }}'],
    ['id' => 'null-coalesce-right-assoc', 'source' => '{{ a ?? b ?? c }}'],
    ['id' => 'defined-in-ternary', 'source' => '{{ x is defined ? 1 : 2 }}{{ not a is not defined }}'],
    ['id' => 'default-chain', 'source' => '{{ a|default(b)|default(c) }}{{ a|default(1, 2) }}{{ a|default(1, x: 2) }}'],
    ['id' => 'arrow-in-mapping', 'source' => '{{ {f: x => x, g: (a, b) => a ~ b} }}'],
    ['id' => 'test-not-same-as', 'source' => '{{ a is not same as b }}{{ a in b|keys }}'],
    ['id' => 'matches-dynamic-pattern', 'source' => "{{ a matches 'b' ~ c }}{{ a matches b ? '/x/' : '/y/' }}"],
    ['id' => 'destructure-then-use', 'source' => '{{ [a, b] = [1, 2] }}{{ a }}{{ {x: y} = z }}'],
    ['id' => 'keys-of-mapping', 'source' => '{{ {a: 1}|keys }}{{ [1, 2]|join(", ")|upper }}'],
    ['id' => 'string-number-keys', 'source' => "{{ {'1': a, 1: b, 1.0: c, '01': d} }}"],
    ['id' => 'empty-string-interpolation-concat', 'source' => '{{ "" ~ "#{a}" }}{{ "#{a}" "" }}'],
    ['id' => 'operator-attribute-names', 'source' => '{{ a.b-and }}{{ a.not in }}'],
    ['id' => 'named-arg-colon-space', 'source' => '{{ max(a : 1) }}{{ max(a :1) }}'],
    ['id' => 'call-on-group', 'source' => '{{ (max)(1) }}{{ (a.b)(1) }}'],
    ['id' => 'spread-trailing-comma', 'source' => '{{ max(1, ...a,) }}{{ [...a,] }}{{ {...a,} }}'],
    ['id' => 'nested-ternary-elvis', 'source' => '{{ a ?: b ? c : d }}{{ a ? b ?: c : d }}'],
    ['id' => 'power-unary-right', 'source' => '{{ 2 ** -1 }}{{ 2 ** not 1 }}'],
    ['id' => 'range-in-filter-args', 'source' => '{{ x|slice(1..2) }}{{ (1..3)|join }}'],
    ['id' => 'lines-across-everything', 'source' => "{{\na\n.\nb\n|\nupper\n~\n'x'\n?\n1\n:\n2\n}}"],

    // custom extensions: callables
    ['id' => 'ext-deprecated-filter', 'path' => 'some/path.twig', 'source' => '{{ a|old }}{{ a|old2 }}{{ old_fn() }}{{ a is old_test }}', 'ext' => [
        'filters' => [['old', ['deprecated' => ['acme/pkg', '1.2']]], ['old2', ['deprecated' => ['acme/pkg', '2.0', 'new', 'acme/new', '3.0']]]],
        'functions' => [['old_fn', ['deprecated' => ['acme/pkg', '1.3', 'new_fn']]]],
        'tests' => [['old_test', ['deprecated' => ['acme/pkg', '1.4', null, 'acme/x']]]],
    ]],
    ['id' => 'ext-deprecated-name-only', 'source' => "{{ a|old }}\n{{ a\nis\nold_test }}", 'ext' => [
        'filters' => [['old', ['deprecated' => ['', '']]]],
        'tests' => [['old_test', ['deprecated' => ['acme/pkg', '1.4']]]],
    ]],
    ['id' => 'ext-dynamic', 'source' => '{{ a|foo_x_bar }}{{ foo_y_bar() }}{{ a is foo_z_bar }}{{ a|x_path }}{{ a|x_y_path }}', 'ext' => [
        'filters' => [['foo_*_bar', []], ['*_path', []]],
        'functions' => [['foo_*_bar', []]],
        'tests' => [['foo_*_bar', []]],
    ]],
    ['id' => 'ext-dynamic-two-stars', 'source' => '{{ "a" is foo_foo_bar_bar }}{{ foo_a_b_bar_c_bar_d() }}', 'ext' => [
        'functions' => [['foo_*_bar_*', []]],
        'tests' => [['*_foo_*_bar', []]],
    ]],
    ['id' => 'ext-dynamic-exact-wins', 'source' => '{{ a|x_path }}', 'ext' => [
        'filters' => [['*_path', []], ['x_path', []]],
    ]],
    ['id' => 'ext-override-core', 'source' => '{{ a|upper }}{{ a|u }}', 'ext' => [
        'filters' => [['upper', ['node_class' => 'raw']]],
    ]],
    ['id' => 'ext-two-word-test', 'source' => '{{ 1 is empty element }}{{ 1 is empty }}', 'ext' => [
        'tests' => [['empty element', []]],
    ]],
    ['id' => 'ext-one-mandatory', 'source' => '{{ 1 is greater than 2 + 3 }}{{ 1 is greater than(2) }}', 'ext' => [
        'tests' => [['greater than', ['one_mandatory_argument' => true]]],
    ]],
    ['id' => 'ext-node-classes', 'source' => '{{ a|d }}{{ a|ee("js") }}{{ a|r }}', 'ext' => [
        'filters' => [['d', ['node_class' => 'default']], ['ee', ['node_class' => 'escape']], ['r', ['node_class' => 'raw']]],
    ]],
    ['id' => 'ext-default-like-not-default', 'source' => '{{ a|d("x") }}', 'ext' => [
        'filters' => [['d', ['node_class' => 'default']]],
    ]],
    ['id' => 'ext-test-node-classes', 'source' => '{{ a is t1 }}{{ a is t2(1) }}', 'ext' => [
        'tests' => [['t1', ['node_class' => 'null']], ['t2', ['node_class' => 'defined']]],
    ]],

    // custom extensions: operators
    ['id' => 'ext-unary-precedence', 'source' => '{{ !false ? "OK" : "KO" }}', 'ext' => [
        'unary' => [['!', 50, 'not']],
    ]],
    ['id' => 'ext-binary-custom', 'source' => '{{ 1 plus 2 * 3 }}{{ 1 * 2 plus 3 }}', 'ext' => [
        'binary' => [['plus', 30, 'left', 'add']],
    ]],
    ['id' => 'ext-binary-right', 'source' => '{{ 1 pow 2 pow 3 }}', 'ext' => [
        'binary' => [['pow', 200, 'right', 'power']],
    ]],
    ['id' => 'ext-binary-alias', 'source' => '{{ 1 plus 2 }}{{ 1 add 2 }}', 'ext' => [
        'binary' => [['plus', 30, 'left', 'add', null, ['add']]],
    ]],
    ['id' => 'ext-binary-override', 'source' => '{{ 1 + 2 * 3 }}', 'ext' => [
        'binary' => [['+', 70, 'left', 'sub']],
    ]],
    ['id' => 'ext-precedence-change-custom', 'source' => '{{ 1 + 2 bar 3 }}{{ 1 bar 2 + 3 }}{{ 1 * 2 bar 3 }}', 'ext' => [
        'binary' => [['bar', 20, 'left', 'concat', 40]],
    ]],
    ['id' => 'ext-unary-precedence-change', 'source' => '{{ neg 1 + 2 }}{{ neg 1 * 2 }}{{ neg (1 + 2) }}', 'ext' => [
        'unary' => [['neg', 25, 'neg', 35]],
    ]],
    ['id' => 'ext-precedence-out-of-range', 'source' => '{{ 1 big 2 }}', 'ext' => [
        'binary' => [['big', 600, 'left', 'add']],
    ]],
];

const NODE_CLASSES = [
    'filter' => [
        'default' => \Twig\Node\Expression\Filter\DefaultFilter::class,
        'escape' => \Twig\Node\Expression\Filter\EscapeFilter::class,
        'raw' => \Twig\Node\Expression\Filter\RawFilter::class,
    ],
    'function' => [],
    'test' => [
        'null' => \Twig\Node\Expression\Test\NullTest::class,
        'defined' => \Twig\Node\Expression\Test\DefinedTest::class,
    ],
    'unary' => [
        'not' => \Twig\Node\Expression\Unary\NotUnary::class,
        'neg' => \Twig\Node\Expression\Unary\NegUnary::class,
        'pos' => \Twig\Node\Expression\Unary\PosUnary::class,
    ],
    'binary' => [
        'add' => \Twig\Node\Expression\Binary\AddBinary::class,
        'sub' => \Twig\Node\Expression\Binary\SubBinary::class,
        'power' => \Twig\Node\Expression\Binary\PowerBinary::class,
        'concat' => \Twig\Node\Expression\Binary\ConcatBinary::class,
    ],
];

/**
 * A test extension: ext = {filters|functions|tests: [[name, options]], unary: [[name, precedence, class, new
 * precedence?]], binary: [[name, precedence, "left"|"right", class, new precedence?, aliases?]]}. Callable options:
 * needs_environment, needs_context, needs_charset, needs_is_sandboxed, is_variadic, one_mandatory_argument (tests),
 * is_safe, pre_escape, node_class (a key of NODE_CLASSES), deprecated: [package, version, alt name, alt package,
 * alt version]. The callable is a closure.
 */
function make_extension(array $ext): AbstractExtension
{
    $callables = [];
    foreach (['filter' => TwigFilter::class, 'function' => TwigFunction::class, 'test' => TwigTest::class] as $type => $class) {
        foreach ($ext[$type.'s'] ?? [] as [$name, $options]) {
            $phpOptions = [];
            foreach ($options as $key => $value) {
                if ('node_class' === $key) {
                    $phpOptions['node_class'] = NODE_CLASSES[$type][$value];
                } elseif ('deprecated' === $key) {
                    $phpOptions['deprecation_info'] = new DeprecatedCallableInfo(...$value);
                } else {
                    $phpOptions[$key] = $value;
                }
            }
            $callables[$type][] = new $class($name, static fn (...$args) => null, $phpOptions);
        }
    }
    $parsers = [];
    foreach ($ext['unary'] ?? [] as $u) {
        $parsers[] = new UnaryOperatorExpressionParser(NODE_CLASSES['unary'][$u[2]], $u[0], $u[1], isset($u[3]) ? new PrecedenceChange('acme/ops', '1.0', $u[3]) : null);
    }
    foreach ($ext['binary'] ?? [] as $b) {
        $parsers[] = new BinaryOperatorExpressionParser(NODE_CLASSES['binary'][$b[3]], $b[0], $b[1], 'left' === $b[2] ? InfixAssociativity::Left : InfixAssociativity::Right, isset($b[4]) ? new PrecedenceChange('acme/ops', '1.0', $b[4]) : null, aliases: $b[5] ?? []);
    }

    return new class($callables, $parsers) extends AbstractExtension {
        public function __construct(private array $callables, private array $parsers)
        {
        }

        public function getFilters(): array
        {
            return $this->callables['filter'] ?? [];
        }

        public function getFunctions(): array
        {
            return $this->callables['function'] ?? [];
        }

        public function getTests(): array
        {
            return $this->callables['test'] ?? [];
        }

        public function getExpressionParsers(): array
        {
            return $this->parsers;
        }
    };
}

final class DumpVisitor implements NodeVisitorInterface
{
    public ?string $dump = null;

    public function enterNode(Node $node, Environment $env): Node
    {
        if ($node instanceof ModuleNode) {
            // embedded templates are parsed (and visited) first; the outermost module is the last one
            $this->dump = rs_dump($node);
        }

        return $node;
    }

    public function leaveNode(Node $node, Environment $env): ?Node
    {
        return $node;
    }

    public function getPriority(): int
    {
        return -100000;
    }
}

function rs_quote(string $s): string
{
    $out = '"';
    $length = strlen($s);
    for ($i = 0; $i < $length; ++$i) {
        $c = $s[$i];
        $b = ord($c);
        if ('"' === $c || '\\' === $c) {
            $out .= '\\'.$c;
        } elseif ($b < 0x20 || 0x7F === $b) {
            $out .= sprintf('\\x%02x', $b);
        } else {
            $out .= $c;
        }
    }

    return $out.'"';
}

function rs_value($value): string
{
    if (null === $value) {
        return 'NULL';
    }
    if (is_bool($value)) {
        return $value ? 'true' : 'false';
    }
    if (is_int($value)) {
        return (string) $value;
    }
    if (is_float($value)) {
        return 'float('.var_export($value, true).')';
    }
    if (is_string($value)) {
        return rs_quote($value);
    }
    if (is_array($value)) {
        $parts = [];
        foreach ($value as $k => $v) {
            $parts[] = rs_value($k).' => '.rs_value($v);
        }

        return '['.implode(', ', $parts).']';
    }
    if ($value instanceof \Closure) {
        return '\Closure';
    }
    if ($value instanceof \Stringable) {
        return (string) $value;
    }

    return 'object('.get_class($value).')';
}

function rs_dump(Node $node, string $prefix = '', int $depth = 0): string
{
    $pad = str_repeat('  ', $depth);
    $class = get_class($node);
    if (str_starts_with($class, 'Twig\\Node\\')) {
        $class = substr($class, strlen('Twig\\Node\\'));
    }
    $line = $pad.$prefix.$class.' @'.$node->getTemplateLine();
    if (null !== $tag = $node->getNodeTag()) {
        $line .= ' #'.$tag;
    }
    if (null !== $doc = $node->getDocumentation()) {
        $line .= ' doc='.rs_quote($doc);
    }
    $lines = [$line];
    $nodeAttributes = [];
    foreach ((fn () => $this->attributes)->call($node) as $name => $value) {
        if ($value instanceof Node) {
            $nodeAttributes[$name] = $value;
            continue;
        }
        $lines[] = $pad.'  .'.$name.' = '.rs_value($value);
    }
    if ($node instanceof SupportDefinedTestInterface && $node->isDefinedTestEnabled()) {
        $lines[] = $pad.'  .(defined_test) = true';
    }
    if ($node instanceof MacroReferenceExpression) {
        $lines[] = $pad.'  .(call_parentheses) = '.($node->hasCallParentheses() ? 'true' : 'false');
    }
    foreach ($node->getNodes() as $key => $child) {
        $lines[] = rs_dump($child, '['.$key.'] ', $depth + 1);
    }
    foreach ($nodeAttributes as $name => $child) {
        $lines[] = rs_dump($child, '[.'.$name.'] ', $depth + 1);
    }

    return implode("\n", $lines);
}

/**
 * Initializes the extensions and keeps only the parser's own node visitors (the CorrectnessNodeVisitor and the
 * dump visitor): the escaper, optimizer, yield-ready and sandbox visitors belong to later stages.
 */
function keep_parser_visitors(Environment $env): void
{
    $env->getNodeVisitors();
    $set = (fn () => $this->extensionSet)->call($env);
    (function (): void {
        $this->visitors = array_values(array_filter(
            $this->visitors,
            static fn (NodeVisitorInterface $v): bool => $v instanceof CorrectnessNodeVisitor || $v instanceof DumpVisitor,
        ));
    })->call($set);
}

function enc_str(?string $s)
{
    if (null === $s) {
        return null;
    }

    return mb_check_encoding($s, 'UTF-8') ? $s : ['b64' => base64_encode($s)];
}

if (false !== $b64 = getenv('CASES_B64')) {
    $cases = json_decode(base64_decode($b64), true, 512, JSON_THROW_ON_ERROR);
}
if (false !== $file = getenv('CASES_FILE')) {
    $cases = json_decode(file_get_contents($file), true, 512, JSON_THROW_ON_ERROR);
}

$out = [];
foreach ($cases as $case) {
    $code = $case['source'];
    $name = $case['name'] ?? 'index';
    $path = $case['path'] ?? '';
    $deprecations = [];
    set_error_handler(function (int $type, string $message) use (&$deprecations): bool {
        if (E_USER_DEPRECATED === $type) {
            $deprecations[] = $message;

            return true;
        }

        throw new \ErrorException($message, 0, $type);
    });

    $res = ['id' => $case['id']];
    if (mb_check_encoding($code, 'UTF-8')) {
        $res['source'] = $code;
    } else {
        $res['source_b64'] = base64_encode($code);
    }
    if ('index' !== $name) {
        $res['name'] = $name;
    }
    if ('' !== $path) {
        $res['path'] = $path;
    }
    if (isset($case['ext'])) {
        $res['ext'] = $case['ext'];
    }
    if (isset($case['extensions'])) {
        $res['extensions'] = $case['extensions'];
    }
    $visitor = new DumpVisitor();
    $error = null;
    try {
        $env = new Environment(new ArrayLoader(), ['cache' => false, 'autoescape' => false]);
        foreach ($case['extensions'] ?? [] as $extension) {
            match ($extension) {
                'sandbox' => $env->addExtension(new SandboxExtension(new SecurityPolicy())),
            };
        }
        if (isset($case['ext'])) {
            $env->addExtension(make_extension($case['ext']));
        }
        $env->addNodeVisitor($visitor);
        keep_parser_visitors($env);
        $env->parse($env->tokenize(new Source($code, $name, $path)));
    } catch (\Throwable $e) {
        $error = ['class' => get_class($e), 'message' => enc_str($e->getMessage())];
    }
    restore_error_handler();
    if (null !== $error) {
        $res['error'] = $error;
    } else {
        $res['dump'] = enc_str($visitor->dump);
    }
    $res['deprecations'] = array_map('enc_str', $deprecations);
    $out[] = $res;
}

echo json_encode($out, JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_PRESERVE_ZERO_FRACTION | JSON_THROW_ON_ERROR), "\n";
