# Twig 3.31.1 syntax reference for the twig-rs port: lexer, parser, tags

Target: byte-for-byte identical behaviour with PHP Twig v3.31.1 on PHP 8.5.13 for everything that happens from template
source text up to the parsed node tree (plus the few compile-time checks that surface as `SyntaxError`s).

Sources (all paths relative to the Twig root `ext/twig-3.31.1/` unless stated otherwise):
`src/Lexer.php`, `src/Token.php`, `src/TokenStream.php`, `src/Parser.php`, `src/ExpressionParser.php`,
`src/ExpressionParser/**`, `src/TokenParser/**`, `src/OperatorPrecedenceChange.php`, `src/Error/{Error,SyntaxError}.php`,
`src/Source.php`, `src/Extension/CoreExtension.php` (registration parts), `src/ExtensionSet.php`, `src/NodeVisitor/CorrectnessNodeVisitor.php`,
the node classes that perform checks, `doc/templates.rst`, `doc/tags/*.rst`, `doc/deprecated.rst`, `doc/operators_precedence.rst`,
`tests/LexerTest.php`, `tests/ParserTest.php`, `tests/ExpressionParserTest.php`, `tests/Fixtures/**`.

How things were verified: a PHP oracle (`docker run php:8.5-cli`, PHP 8.5.13) loading the local Twig 3.31.1 `src/` through a
PSR-4 autoloader plus the real `symfony/deprecation-contracts` `trigger_deprecation()` body. Environment options were
`['autoescape' => false, 'cache' => false]` unless stated, loader `ArrayLoader`, template name `index`. About 450 probes were
run; every line marked **[verified]** was observed on the oracle. The harness lives in
`scratchpad/oracle/` (`run.php`, `bootstrap.php`, `*.json` probe files, `regexes.php`). Statements marked **UNVERIFIED** were
derived from reading code only and were not run.

---

## Table of contents

1. [Error model and message formatting](#1-error-model-and-message-formatting)
2. [Lexer](#2-lexer)
   - 2.1 Options and defaults
   - 2.2 Token types
   - 2.3 Regexes, verbatim
   - 2.4 Operator regex construction
   - 2.5 Main loop and states
   - 2.6 `lexData`: text, whitespace control, tag openers, `verbatim`, `line`
   - 2.7 `lexBlock` / `lexVar` and closing tokens
   - 2.8 `lexExpression`
   - 2.9 Strings and escape processing
   - 2.10 Double-quoted strings and interpolation
   - 2.11 Numbers
   - 2.12 Names
   - 2.13 Punctuation and bracket checking
   - 2.14 Comments, inline comments, documentation comments
   - 2.15 Line numbers, offsets, columns, newline normalization
   - 2.16 Whitespace control summary
   - 2.17 Lexer error catalogue
3. [TokenStream and Token::test](#3-tokenstream-and-tokentest)
4. [Parser core](#4-parser-core)
   - 4.1 `parse()` and Module assembly
   - 4.2 `subparse()`
   - 4.3 Unknown/unexpected tag errors and the suggestion algorithm
   - 4.4 Blocks, macros, traits, embedded templates, imported symbols
   - 4.5 Child templates: body cleanup and `CorrectnessNodeVisitor`
   - 4.6 Function / filter / test lookup
5. [Expression grammar](#5-expression-grammar)
   - 5.1 Precedence climbing (`Parser::parseExpression`)
   - 5.2 Complete operator table
   - 5.3 Prefix parsers (literals, grouping, unary, spread)
   - 5.4 Infix parsers
   - 5.5 Call arguments
   - 5.6 Errors raised while constructing nodes
   - 5.7 Precedence-change deprecations
   - 5.8 Line numbers given to expression nodes
   - 5.9 Compile semantics that leak from the AST (must be replicated)
6. [Tags](#6-tags)
7. [Deprecations relevant to syntax](#7-deprecations-relevant-to-syntax)
8. [Gap analysis against the original twig-rs code](#8-gap-analysis-against-the-original-twig-rs-code)
9. [Appendix A: verified probe transcript (selection)](#9-appendix-a-verified-probe-transcript-selection)

### Top pitfalls for the port (each verified on the oracle)

1. Everything is byte based (names accept any byte >= 0x7F, columns count bytes, `\s` is ASCII only).
2. `(`, `[`, `.`, `?`, `|` are OPERATOR tokens; only `) ] { } : ,` are punctuation.
3. Operator words need a following `[\s()\[{]` and must not follow `.`/`|` (+ optional single space): `foo.and`, `x|in` are names,
   `{{ x is not}}` is `is` + name `not`, `{% block in%}` is legal.
4. `{{-1}}` prints `1` (trim marker), `{{ 5-}}` prints `5`; `a.0.1` uses the float attribute `0.1`; `?.5` is the null-safe operator.
5. `{{ x` fails in the parser but `{{ x ` fails in the lexer (`Unclosed "variable"`), with different messages.
6. Whitespace trimming is asymmetric: `{{-` uses PHP `rtrim` (NUL yes, `\f` no), `-}}` uses `\s` (`\f` yes, NUL no); plain `%}`
   eats one newline, plain `}}` none, `verbatim`/`line` closers none.
7. CRLF/CR are normalized to LF in all text and string literals.
8. String escapes are Twig's own (`\f\n\r\t\v \\ \' \" \#{ \xH \xHH \0-\377`); everything else drops the backslash.
9. `"#{x}"` is the bare expression `x`; `"a#{1}" "b"` concatenates but `'a' 'b'` is an error.
10. Unary `-`/`+` bind tighter than everything but postfix; `not` has precedence 50; tests' single mandatory argument is parsed
    at precedence 100; `-1**0` evaluates to `-1` (PHP compile artefact).
11. `??` needs a left operand that supports the `defined` test (`(1+2) ?? 3` is a SyntaxError).
12. `=` is an expression operator (3.23): named arguments `f(a = 1)` are unwrapped `SetBinary`s, even `f((a = 1))`.
13. Unknown-name suggestions depend on registry ORDER (stable sort by levenshtein distance).
14. "defined near line N" is the line of the first token of the enclosing body, not of the opening tag.
15. Correctness checks (child-template content, block nesting, multiple extends) run after the whole template parsed.
16. Imports are visible only in the innermost scope and the main scope; `from x import m as a, m as b` keeps only `b`.

---

## 1. Error model and message formatting

### 1.1 Exception classes

- `Twig\Error\SyntaxError extends Twig\Error\Error extends \Exception` (`src/Error/SyntaxError.php:20`). All lexer and parser
  errors are `SyntaxError`, plus a few errors raised by node constructors and by `CorrectnessNodeVisitor`.
- Constructor: `Error::__construct(string $message, int $lineno = -1, ?Source $source = null, ?\Throwable $previous = null, ?int $columnno = null)`
  (`src/Error/Error.php:56`). The raw message is stored; the displayed message is recomputed by `updateRepr()` whenever line,
  column, source or message change.

### 1.2 Final message algorithm (`src/Error/Error.php:131-160`)

```
message = rawMessage
last = last byte of message
punct = (last == '.' or last == '?') ? last : ''
if punct != '': message = message without its last byte
if source != null and source.name != '':      message += ' in "' + source.name + '"'
if lineno > 0:
    message += ' at line ' + lineno
    if columnno != null:                       message += ' column ' + columnno
message += punct
```

Consequences [verified]:
- `Unknown "cycl" function. Did you mean "cycle"?` becomes `Unknown "cycl" function. Did you mean "cycle" in "index" at line 1?`
- Messages without trailing `.`/`?` get no punctuation at the end.
- `lineno <= 0` suppresses ` at line ...` (and the column): `{% line 0 %}{{ x` ->
  `Unexpected token "end of template" ("end of print statement" expected) in "index".` [verified]
- `appendMessage()` (`Error.php:125`) concatenates to the raw message and re-runs `updateRepr()`.

### 1.3 Where source/line come from

- `Parser::parse()` (`src/Parser.php:118-127`) catches `SyntaxError` thrown during `subparse()`: if the error has no source it
  gets the stream source; if `getTemplateLine()` is falsy (i.e. exactly `0`; `-1` is truthy in PHP) it gets the current
  token's line.
- `Environment::compileSource()` (`src/Environment.php:614-624`) catches every `Twig\Error\Error` and calls
  `setSourceContext($source)` (unconditionally); any other `\Exception` is wrapped as
  `SyntaxError('An exception has been thrown during the compilation of a template ("%s").', -1, $source, $e)` - note lineno `-1`,
  so the message has no line. PHP `\Error`s (e.g. `TypeError`) are NOT caught and escape raw. [verified: see 6.2 autoescape null]

### 1.4 Columns

Only these errors carry a column (all others have none) [verified]:
- lexer `Unexpected character "%s".` and `Unexpected "%s".` (column of the offending byte),
- every error produced by `TokenStream::expect()` (column of the offending token; tokens synthesized by a token parser, e.g. the
  four tokens injected by `embed`, have no offset and thus no column).

`Source::getColumn(int $offset)` (`src/Source.php:83-93`), byte based:

```
if offset < 0: return null
before = substr(code, 0, offset) with "\r\n" and then "\r" replaced by "\n"
lineStart = strrpos(before, "\n")
return lineStart === false ? strlen(before) + 1 : strlen(before) - lineStart
```

`{{ é + $ }}` -> column 9 (é counts 2 bytes); `x\r\n{{ é + $ }}` -> line 2 column 9. [verified]

### 1.5 Suggestions (`SyntaxError::addSuggestions`, `src/Error/SyntaxError.php:28-45`)

```
alternatives = ordered map
for item in items (in registry order):
    lev = levenshtein(name, item)               # PHP levenshtein: byte based, insert/delete/replace cost 1
    if lev <= strlen(name) / 3  (float division)  or  str_contains(item, name):
        alternatives[item] = lev
if empty: return
asort(alternatives)                             # ascending by lev, STABLE (ties keep registry order)
appendMessage(' Did you mean "' + join('", "', keys) + '"?')
```

Registry orders that therefore matter (dumped from the oracle, default `Environment`):
- tags: `apply, for, if, extends, include, block, use, macro, import, from, set, types, flush, do, embed, with, deprecated, guard, autoescape`
  (`sandbox` is appended only when `SandboxExtension` is added; user tags come after extension tags).
- filters: `date, date_modify, format, replace, number_format, abs, round, url_encode, json_encode, convert_encoding, title, capitalize, upper, lower, striptags, trim, nl2br, spaceless, join, split, sort, merge, batch, column, filter, map, reduce, find, reverse, shuffle, length, slice, first, last, default, keys, invoke, escape, e, raw`
- functions: `parent, block, attribute, max, min, range, constant, cycle, random, date, include, include_only, source, enum_cases, enum`
- tests: `even, odd, defined, same as, none, null, divisible by, constant, empty, iterable, sequence, mapping, true`

Examples [verified]: `{% e %}` -> `Unknown "e" tag. Did you mean "use", "set", "types", "embed", "extends", "include", "deprecated", "autoescape" in "index" at line 1?`;
`{{ x|f }}` -> `Unknown "f" filter. Did you mean "find", "first", "format", "filter", "shuffle", "default", "date_modify", "number_format" in "index" at line 1?`;
`{% fo %}` -> `... Did you mean "for" ...` (by `str_contains`, lev 1 > 0.67).

---

## 2. Lexer

### 2.1 Options and defaults (`src/Lexer.php:77-90`)

| option | default | notes |
|---|---|---|
| `tag_comment` | `['{#', '#}']` | |
| `tag_block` | `['{%', '%}']` | |
| `tag_variable` | `['{{', '}}']` | |
| `whitespace_trim` | `'-'` | |
| `whitespace_line_trim` | `'~'` | |
| `whitespace_line_chars` | `' \t\0\x0B'` | a PHP single-quoted literal: the backslashes are literal and only become TAB/NUL/VT inside the regex character class `[ \t\0\x0B]` |
| `interpolation` | `['#{', '}']` | |

Options are merged with `array_merge` (user keys override). Regexes are built once per `Lexer` instance on first `tokenize()`
(`initialize()`, `src/Lexer.php:92-208`). Behaviour with non-default delimiters is UNVERIFIED (beyond `tests/LexerTest.php:548`);
note that the delimiters are inserted with `preg_quote(..., '#')` into patterns using the `x` flag, so a delimiter containing a
space would have the space ignored (UNVERIFIED), and in `lex_raw_data` the trim characters are inserted without quoting.

### 2.2 Token types (`src/Token.php:20-40`, English names `src/Token.php:225-261`)

| const | value | `typeToEnglish` (used in error messages) | token value |
|---|---|---|---|
| `EOF_TYPE` | -1 | `end of template` | `''` |
| `TEXT_TYPE` | 0 | `text` | string |
| `BLOCK_START_TYPE` | 1 | `begin of statement block` | `''` |
| `VAR_START_TYPE` | 2 | `begin of print statement` | `''` |
| `BLOCK_END_TYPE` | 3 | `end of statement block` | `''` |
| `VAR_END_TYPE` | 4 | `end of print statement` | `''` |
| `NAME_TYPE` | 5 | `name` | string |
| `NUMBER_TYPE` | 6 | `number` | int or float (PHP) |
| `STRING_TYPE` | 7 | `string` | string (escapes already processed) |
| `OPERATOR_TYPE` | 8 | `operator` | string (whitespace runs collapsed to one space) |
| `PUNCTUATION_TYPE` | 9 | `punctuation` | one-byte string |
| `INTERPOLATION_START_TYPE` | 10 | `begin of string interpolation` | `''` |
| `INTERPOLATION_END_TYPE` | 11 | `end of string interpolation` | `''` |
| `ARROW_TYPE` (deprecated 3.21) | 12 | `arrow function` | never produced |
| `SPREAD_TYPE` (deprecated 3.21) | 13 | `spread operator` | never produced |

Each token also stores `lineno`, a 0-based byte `offset` (null for synthesized tokens) and an optional `documentation` string
(section 2.14). `typeToString()` gives `EOF_TYPE` etc. (or `Twig\Token::EOF_TYPE` in long form).

Important 3.x change: `(`, `[`, `.`, `?`, `|` are lexed as **OPERATOR** tokens (they are operator names, and operators are tried
before punctuation). PUNCTUATION tokens in practice are only `)`, `]`, `{`, `}`, `:`, `,`. [verified by token dumps]

### 2.3 Regexes, verbatim

Class constants (`src/Lexer.php:51-67`), PHP literal then effective PCRE pattern:

```php
public const REGEX_NAME = '/[a-zA-Z_\x7f-\xff][a-zA-Z0-9_\x7f-\xff]*/A';
public const REGEX_STRING = '/"([^#"\\\\]*(?:\\\\.[^#"\\\\]*)*)"|\'([^\'\\\\]*(?:\\\\.[^\'\\\\]*)*)\'/As';
//   effective: /"([^#"\\]*(?:\\.[^#"\\]*)*)"|'([^'\\]*(?:\\.[^'\\]*)*)'/As
public const REGEX_NUMBER = '/(?(DEFINE)
        (?<LNUM>[0-9]+(_[0-9]+)*)               # Integers (with underscores)   123_456
        (?<FRAC>\.(?&LNUM))                     # Fractional part               .456
        (?<EXPONENT>[eE][+-]?(?&LNUM))          # Exponent part                 E+10
        (?<DNUM>(?&LNUM)(?:(?&FRAC))?)          # Decimal number                123_456.456
    )(?:(?&DNUM)(?:(?&EXPONENT))?)              #                               123_456.456E+10
    /Ax';
public const REGEX_DQ_STRING_DELIM = '/"/A';
public const REGEX_DQ_STRING_PART = '/[^#"\\\\]*(?:(?:\\\\.|#(?!\{))[^#"\\\\]*)*/As';
//   effective: /[^#"\\]*(?:(?:\\.|#(?!\{))[^#"\\]*)*/As
public const REGEX_INLINE_COMMENT = '/#[^\n]*/A';        // public BC constant, NOT used by the 3.31.1 lexer
public const PUNCTUATION = '()[]{}?:.,|';
private const REGEX_RAW_INLINE_COMMENT = '/#[^\r\n]*/A'; // the one actually used (src/Lexer.php:415)
```

Inline (`src/Lexer.php:370`): whitespace `'/\s+/A'`.

Built regexes with default options - exact strings dumped from the oracle (`regexes.php`). Patterns with the `x` flag contain
newlines/indentation in the real string; PCRE ignores unescaped whitespace outside character classes in `x` mode (the space inside
`[ \t\0\x0B]` IS significant). Delimiters are `{`...`}` for these.

| key | pattern (whitespace-insignificant parts compacted) | flags | anchored? | source |
|---|---|---|---|---|
| `lex_var` | `\s*(?:\-\}\}\s*\|~\}\}[ \t\0\x0B]*\|\}\})` | `Ax` | yes | `Lexer.php:105-114` |
| `lex_block` | `\s*(?:\-%\}\s*\n?\|~%\}[ \t\0\x0B]*\|%\}(?:\r\n?\|\n)?)` | `Ax` | yes | `Lexer.php:117-126` |
| `lex_raw_data` | `\{%(-\|~)?\s*endverbatim\s*(?:\-%\}\s*\|~%\}[ \t\0\x0B]*\|%\})` | `sx` | **no** (searched from cursor) | `Lexer.php:129-143` |
| `operator` | see 2.4 | `A` | yes | `Lexer.php:614-646` |
| `lex_comment` | `(?:\-\#\}\s*\n?\|~\#\}[ \t\0\x0B]*\|\#\}(?:\r\n?\|\n)?)` | `sx` | **no** | `Lexer.php:148-156` |
| `lex_documentation_comment` | `(?:\-\#\#\}\s*\n?\|~\#\#\}[ \t\0\x0B]*\|\-\#\}\s*\n?\|~\#\}[ \t\0\x0B]*\|\#\}(?:\r\n?\|\n)?)` | `sx` | **no** | `Lexer.php:159-171` |
| `lex_block_raw` | `\s*verbatim\s*(?:\-%\}\s*\|~%\}[ \t\0\x0B]*\|%\})` | `Asx` | yes | `Lexer.php:174-183` |
| `lex_block_line` | `\s*line\s+(\d+)\s*%\}` | `As` (no `x`) | yes | `Lexer.php:185` |
| `lex_tokens_start` | `(\{\{\|\{%\|\{\#\#(?!\})\|\{\#)(\-\|~)?` | `sx` | no (`preg_match_all`) | `Lexer.php:188-202` |
| `interpolation_start` | `\#\{\s*` | `A` | yes | `Lexer.php:203` |
| `interpolation_end` | `\s*\}` | `A` | yes | `Lexer.php:204` |

(`\|` in the table means a literal regex alternation `|`.) The `(?!\})` lookahead in `lex_tokens_start` is
`$emptyCommentLookahead` (`Lexer.php:100`): it is only added because the comment closer starts with `#`; it keeps the empty comment
`{##}` a normal comment instead of a documentation comment.

PCRE facts the port must reproduce (no `u` flag anywhere, so everything is byte based):
- `\s` = `[\t\n\x0B\f\r ]` (ASCII only; bytes >= 0x80 are never whitespace).
- `\d` = `[0-9]`.
- `A` = anchored at the offset passed to `preg_match` (i.e. "match exactly at cursor").
- `s` = `.` also matches `\n` (only relevant for `.` uses; character classes match newlines anyway).

### 2.4 Operator regex construction (`src/Lexer.php:614-646`)

```
tokens = concat(ExpressionParsers::getOperatorTokensFor(p) for p in env.getExpressionParsers())   # name + aliases; literal -> []
map = array_combine(tokens, map(strlen, tokens))      # duplicate keys collapse, key keeps FIRST position
arsort(map)                                           # by length DESC, stable
parts = []
for op in map keys:
    r = preg_quote(op, '/')
    if ctype_alpha(last byte of op):  r = r + '(?=[\s()\[{])'          # must be followed by ws, ( ) [ or {
    if ctype_alpha(first byte of op): r = '(?<![\.\|]\s|.[\.\|])' + r  # not preceded by ". " "| " or "x." "x|"
    r = preg_replace('/\s+/', '\s+', r)              # a space inside an operator matches any whitespace run
    parts.push(r)
regex = '/' + join('|', parts) + '/A'
```

Exact default regex [verified, dumped]:

```
/(?<![\.\|]\s|.[\.\|])starts\s+with(?=[\s()\[{])|(?<![\.\|]\s|.[\.\|])ends\s+with(?=[\s()\[{])|(?<![\.\|]\s|.[\.\|])has\s+every(?=[\s()\[{])|(?<![\.\|]\s|.[\.\|])has\s+some(?=[\s()\[{])|(?<![\.\|]\s|.[\.\|])matches(?=[\s()\[{])|(?<![\.\|]\s|.[\.\|])not\s+in(?=[\s()\[{])|(?<![\.\|]\s|.[\.\|])is\s+not(?=[\s()\[{])|(?<![\.\|]\s|.[\.\|])b\-xor(?=[\s()\[{])|(?<![\.\|]\s|.[\.\|])b\-and(?=[\s()\[{])|(?<![\.\|]\s|.[\.\|])b\-or(?=[\s()\[{])|(?<![\.\|]\s|.[\.\|])not(?=[\s()\[{])|\.\.\.|\?\s+\:|(?<![\.\|]\s|.[\.\|])xor(?=[\s()\[{])|(?<![\.\|]\s|.[\.\|])and(?=[\s()\[{])|\<\=\>|\=\=\=|\!\=\=|\?\:|\?\?|(?<![\.\|]\s|.[\.\|])or(?=[\s()\[{])|\=\=|\!\=|\>\=|\<\=|(?<![\.\|]\s|.[\.\|])in(?=[\s()\[{])|\.\.|\/\/|\*\*|(?<![\.\|]\s|.[\.\|])is(?=[\s()\[{])|\?\.|\=\>|\-|\+|\(|\<|\>|~|\*|\/|%|\?|\=|\||\.|\[/A
```

Operator token list in that order: `starts with, ends with, has every, has some, matches, not in, is not, b-xor, b-and, b-or, not, ..., ? :, xor, and, <=>, ===, !==, ?:, ??, or, ==, !=, >=, <=, in, .., //, **, is, ?., =>, -, +, (, <, >, ~, *, /, %, ?, =, |, ., [`.
PCRE alternation takes the first alternative that matches (left to right), so longer operators win; the lookarounds make the
letter-operators context sensitive. Lexer-visible consequences [all verified]:

- `{{ not(x) }}`, `not[1]`, `not{a:1}` -> OPERATOR `not`; `notx`, `notable` -> NAME.
- `{{ x is not}}` -> `is` operator, then NAME `not` (because `}` is not in the lookahead set) -> parse error `Unknown "not" test.`.
- `{{ 1 b-and2 }}` -> NUMBER 1, NAME `b`, OPERATOR `-`, NAME `and2`.
- `x not   in\n y` -> one OPERATOR token with value `not in` (internal whitespace normalized to one space by
  `preg_replace('/\s+/', ' ', $match[0])`, `Lexer.php:380`). `a ? : b` -> OPERATOR `? :` (Elvis alias).
- `'a'|and`, `foo.and`, `foo . and`, `foo .and` -> NAME `and` (lookbehind). `'x' |  and` (two spaces) -> OPERATOR `and`.
  `x.  and` (two spaces) -> OPERATOR `and`, which the dot parser still accepts as an attribute name.
- `a ?.5 : 1` -> `?.` null-safe operator is taken, then `5` -> later `Unexpected token "punctuation" of value ":" ("end of print statement" expected)`.
- `1..5` -> NUMBER 1, OPERATOR `..`, NUMBER 5 (FRAC needs a digit after `.`). `a.0.1` -> NAME a, OPERATOR `.`, NUMBER `0.1` (float!).
- `{{-1}}` is `{{-` (trim) followed by `1`, prints `1`; `{{ 5-}}` and `{{ 5 -}}` are `5` followed by the `-}}` trim closer. [verified]

### 2.5 Main loop and states (`src/Lexer.php:210-264`)

States: `STATE_DATA=0`, `STATE_BLOCK=1`, `STATE_VAR=2`, `STATE_STRING=3`, `STATE_INTERPOLATION=4` (`Lexer.php:45-49`).

```
tokenize(source):
    initialize()                     # build regexes once
    code = source.code; cursor = 0; lineno = 1; end = strlen(code)
    tokens = []; state = DATA; states = []; brackets = []; position = -1; documentation = []
    positions = preg_match_all(lex_tokens_start, code, PREG_OFFSET_CAPTURE)
        # for every non-overlapping match, left to right: [0]=(full text, offset), [1]=(delimiter), [2]=(trim char or '' / unset)
    while cursor < end:
        switch state: DATA -> lexData(); BLOCK -> lexBlock(); VAR -> lexVar(); STRING -> lexString(); INTERPOLATION -> lexInterpolation()
    pushToken(EOF)                                   # value '', line = lineno, offset = cursor (== end)
    if brackets not empty:
        (expect, line) = pop(brackets)               # innermost
        throw SyntaxError('Unclosed "%s".' % expect, line)     # no column
    return TokenStream(tokens, source)

pushState(s): states.push(state); state = s
popState():   if states empty: throw LogicException('Cannot pop state without a previous state.'); state = states.pop()
```

`pushToken(type, value='', offset=null, lineno=null)` (`Lexer.php:565-581`): drops TEXT tokens whose value is `''`; attaches
pending documentation (2.14) to any token except whitespace-only TEXT; offset defaults to the current cursor and line to the
current lineno. Callers always push a token **before** moving the cursor over it, so offsets/lines point at the token start.

`moveCursor(text)` (`Lexer.php:590-596`): `cursor += strlen(text)`; `lineno += count of newlines in text` where `\r\n` and lone
`\r` each count as one newline.

### 2.6 `lexData` (`src/Lexer.php:266-345`)

```
lexData():
    if position == count(positions) - 1:                 # no further tag opener
        text = substr(code, cursor); pushToken(TEXT, normalizeNewlines(text)); moveCursor(text); return
    position += 1; pos = positions[position]
    while pos.offset < cursor:                            # skip openers swallowed by earlier constructs (strings, comments, verbatim...)
        if position == count(positions) - 1: return       # next lexData() call emits the rest as TEXT
        position += 1; pos = positions[position]
    text = textContent = substr(code, cursor, pos.offset - cursor)
    if pos has group 2:
        if group2 == '-':  text = rtrim(text)                     # PHP default set " \t\n\r\0\x0B"
        elif group2 == '~': text = rtrim(text, " \t\0\x0B")        # keeps \r and \n
    pushToken(TEXT, normalizeNewlines(text))               # dropped if empty
    moveCursor(textContent)
    switch pos.delimiter:                                  # group 1
      '{##': moveCursor(pos.fulltext); lexComment(true)
      '{#' : moveCursor(pos.fulltext); lexComment(false)
      '{%' : line0 = lineno; cur0 = cursor; moveCursor(pos.fulltext)      # fulltext includes the trim char
             if lex_block_raw matches at cursor:  documentation = []; moveCursor(m[0]); lexRawData()
             elif lex_block_line matches at cursor: documentation = []; moveCursor(m[0]); lineno = (int) m[1]
             else: pushToken(BLOCK_START, '', cur0, line0); pushState(BLOCK); currentVarBlockLine = lineno
      '{{' : line0 = lineno; cur0 = cursor; moveCursor(pos.fulltext)
             pushToken(VAR_START, '', cur0, line0); pushState(VAR); currentVarBlockLine = lineno
```

Notes:
- `(int) m[1]` saturates to `PHP_INT_MAX` on overflow: `{% line 99999999999999999999 %}` makes subsequent lines
  `9223372036854775807`. [verified]
- `{% line N %}` (lexer pseudo-tag, undocumented): no trim modifier allowed on the right (`{% line 5 -%}` is not recognized and
  becomes tag `line` -> `Unknown "line" tag.`), a left modifier works (`{%- line 7 %}`), no newline after `%}` is eaten; the text
  following it gets line `N`. `{%line 3%}` works. [verified]
- `{% verbatim %}` is recognized only in the exact form of `lex_block_raw`; `{% verbatim x %}` is a normal tag named `verbatim`
  -> `Unknown "verbatim" tag.` [verified]. There is no `raw` alias any more: `{% raw %}` -> `Unknown "raw" tag.` [verified].

`lexRawData()` (`Lexer.php:484-507`):

```
if lex_raw_data does not match searching from cursor:
    throw SyntaxError('Unexpected end of file: Unclosed "verbatim" block.', lineno)        # no column
text = substr(code, cursor, m.offset - cursor); start = cursor
moveCursor(text + m.fulltext)
if group1 == '-': text = rtrim(text)  elif group1 == '~': text = rtrim(text, " \t\0\x0B")
pushToken(TEXT, normalizeNewlines(text), offset = start)
```

Verbatim quirks [verified]: the plain `%}` of both `{% verbatim %}` and `{% endverbatim %}` does **not** eat the following newline
(`{% verbatim %}\nx{{ y }}{% endverbatim %}\nY` renders `\nx{{ y }}\nY`); nesting is not supported (the first `endverbatim`
closes); CRLF inside verbatim is normalized to LF; `-`/`~` work on all four sides.

### 2.7 `lexBlock` / `lexVar` and closing tokens (`src/Lexer.php:347-365`, `603-612`)

```
lexBlock(): if brackets empty and lex_block matches at cursor: pushClosingToken(BLOCK_END, m[0]); popState()  else lexExpression()
lexVar():   if brackets empty and lex_var   matches at cursor: pushClosingToken(VAR_END,   m[0]); popState()  else lexExpression()

pushClosingToken(type, match):
    lead = strlen(match) - strlen(ltrim(match))        # ltrim default set " \t\n\r\0\x0B"
    if lead: moveCursor(substr(match, 0, lead))
    pushToken(type)                                    # so the token line/offset is the "%}" / "}}" marker itself
    moveCursor(substr(match, lead))
```

The closing delimiter is only recognized while the bracket stack is empty: `{{ {a: {b: 1}} }}` lexes fine,
`{{ (1 + 3 }}` gives `Unclosed "(".` (the `}` is checked against `(`). [verified]

### 2.8 `lexExpression` (`src/Lexer.php:367-425`) - exact order

```
lexExpression():
    if /\s+/A matches at cursor:
        moveCursor(ws)
        if cursor >= end:
            throw SyntaxError('Unclosed "%s".' % (state == BLOCK ? 'block' : 'variable'), currentVarBlockLine)   # no column
    if operator regex matches at cursor:
        op = preg_replace('/\s+/', ' ', m[0])
        if op in ['{', '(', '[']: checkBrackets(op)          # in practice '(' and '['
        pushToken(OPERATOR, op); moveCursor(m[0])
    elif REGEX_NAME matches:   pushToken(NAME, m[0]); moveCursor(m[0])
    elif REGEX_NUMBER matches: pushToken(NUMBER, 0 + str_replace('_', '', m[0])); moveCursor(m[0])
    elif code[cursor] in PUNCTUATION:
        checkBrackets(code[cursor]); pushToken(PUNCTUATION, code[cursor]); moveCursor(code[cursor])
    elif REGEX_STRING matches:
        pushToken(STRING, stripcslashes(normalizeNewlines(substr(m[0], 1, -1)), m[0][0])); moveCursor(m[0])
    elif REGEX_DQ_STRING_DELIM matches:                      # an opening " that REGEX_STRING could not match (contains # or is unterminated)
        brackets.push(['"', lineno]); pushState(STRING); moveCursor('"')
    elif REGEX_RAW_INLINE_COMMENT matches:                   # '#[^\r\n]*'
        if m[0] starts with '##': addDocumentation(substr(m[0], 2))
        moveCursor(m[0])
    else:
        throw SyntaxError('Unexpected character "%s".' % code[cursor], lineno, column = getColumn(cursor))
```

Notes [verified unless stated]:
- The whitespace check means `{{ x` (no trailing blank) lexes successfully and fails in the parser
  (`Unexpected token "end of template" ("end of print statement" expected) in "index" at line 1 column 5.`) while `{{ x ` fails
  in the lexer (`Unclosed "variable" in "index" at line 1.`). Likewise `{{ [1, 2` -> `Unclosed "["` (end-of-tokenize check).
- Inside a string interpolation the state is INTERPOLATION, so the message says `variable` even inside a block:
  `\n{% set a = "#{ x ` -> `Unclosed "variable" in "index" at line 2.`
- The offending byte is printed raw (one byte, so a multi-byte UTF-8 char can never be "unexpected": bytes >= 0x80 start names).
  A NUL byte gives `Unexpected character "\0"` with the raw NUL in the message.
- `{{ 1 #}}` : `#}}` is an inline comment to end of line -> parser error at EOF (column 9).

### 2.9 Strings and escape processing (`src/Lexer.php:404-407`, `427-482`)

Single-quoted strings and double-quoted strings without `#` are matched whole by `REGEX_STRING`; the inner text (quotes stripped)
goes through `normalizeNewlines()` then the Twig-specific `stripcslashes($str, $quoteType)` (NOT PHP's `stripcslashes`):

```
stripcslashes(str, quote):
    out = ''; i = 0; n = strlen(str)
    loop:
        p = strpos(str, '\\', i); if p === false: out += substr(str, i); break
        out += substr(str, i, p - i); i = p + 1
        if i >= n: out += '\\'; break                    # unreachable from the lexer (regexes consume \\. pairs)
        c = str[i]
        if c in {f:"\f", n:"\n", r:"\r", t:"\t", v:"\v"}:   out += that char
        elif c == '\\':                                    out += '\\'
        elif c == "'" or c == '"':
            if c != quote: deprecation(c, i + 1)           # 3.12 deprecation, char kept
            out += c
        elif c == '#' and i + 1 < n and str[i+1] == '{':   out += '#{'; i += 1
        elif c == 'x' and i + 1 < n and ctype_xdigit(str[i+1]):
            hex = str[++i]; if i + 1 < n and ctype_xdigit(str[i+1]): hex += str[++i]
            out += chr(hexdec(hex))                        # 1 or 2 hex digits
        elif ctype_digit(c) and c < '8':                   # octal, up to 3 digits
            oct = c; while i + 1 < n and ctype_digit(str[i+1]) and str[i+1] < '8' and strlen(oct) < 3: oct += str[++i]
            out += chr(octdec(oct) % 256)
        else:
            deprecation(c, i + 1); out += c                # backslash dropped
        i += 1
    return out
```

Deprecation text (`Lexer.php:455` and `474`, emitted via `trigger_deprecation('twig/twig', '3.12', ...)`):
`Since twig/twig 3.12: Character "%s" should not be escaped; the "\" character is ignored in Twig 3 but will not be in Twig 4. Please remove the extra "\" character at position %d in "%s" at line %d.`
where position = 1-based index of the escaped character inside the (part of the) string content, name = source name, line =
current lexer line. Escapes NOT supported (backslash dropped + deprecation): `\a \b \e \u \X \8 \9` and any other char. [verified:
`'\a\b\e'` -> `abe`, `"\u0041"` -> `u0041`, `'\X41'` -> `X41`, `'\8'` -> `8`, `'\x41\x4a\x4'` -> `AJ\x04`, `'\x'` -> `x`,
`'\12a'` -> `"\na"`, `'\1234'` -> `S4`, `'\777'` -> `\xff`, `'\400'` -> `\x00`, `'\#{x}'` -> `#{x}` (no deprecation), `'\#'` -> `#`
(deprecation), `"a\'b"` -> `a'b` (deprecation), `'a\"b'` -> `a"b` (deprecation)].

Unterminated single-quoted string: `{{ 'abc }}` -> `Unexpected character "'" in "index" at line 1 column 4.`; `{{ 'a\' }}` same
(the `\'` is an escaped quote). Unterminated double-quoted string: `{{ "abc }}` -> `Unclosed """ in "index" at line 1.` (it enters
STATE_STRING and the end-of-input bracket check fires). [verified]

### 2.10 Double-quoted strings and interpolation (`src/Lexer.php:529-563`)

```
lexString():                                           # state STRING, the opening " already consumed
    if interpolation_start ('#{' + \s*) matches:
        brackets.push(['#{', lineno]); pushToken(INTERPOLATION_START); moveCursor(m[0]); pushState(INTERPOLATION)
    elif REGEX_DQ_STRING_PART matches and m[0] != '':
        pushToken(STRING, stripcslashes(normalizeNewlines(m[0]), '"')); moveCursor(m[0])
    elif REGEX_DQ_STRING_DELIM matches:                # closing "
        (expect, line) = brackets.pop()
        if '"' != code[cursor]: throw SyntaxError('Unclosed "%s".' % expect, line)   # unreachable
        popState(); moveCursor('"')
    else:
        throw SyntaxError('Unexpected character "%s".', lineno, column)  # only reachable for a lone trailing "\" at EOF
                                                                          # [verified: {{ "abc\  -> Unexpected character "\" ... column 8.]

lexInterpolation():
    top = last(brackets)
    if top[0] == '#{' and interpolation_end (\s* + '}') matches: brackets.pop(); pushClosingToken(INTERPOLATION_END, m[0]); popState()
    else lexExpression()
```

Token shapes [verified]: `"foo #{bar} baz"` -> STRING `foo `, INTERP_START, NAME bar, INTERP_END, STRING ` baz`;
`"#{x}"` -> INTERP_START, NAME x, INTERP_END (no STRING token at all). Nested interpolation works
(`"bar #{ "foo#{bar}" }"`). `"a#{1"` -> `Unclosed """`. `"#{"` -> `Unclosed """`. Interpolation is not processed in single quotes.
A `}` inside the interpolation closes it only when the bracket stack top is `#{`, so `"a#{ {x: 1}|length }b"` works.
The parser side (5.3.4) turns the parts into nested `ConcatBinary` nodes.

### 2.11 Numbers (`src/Lexer.php:54-60`, `393-396`)

- Grammar: `LNUM ( '.' LNUM )? ( [eE] [+-]? LNUM )?` with `LNUM = [0-9]+ ( '_' [0-9]+ )*`, anchored, PCRE backtracking:
  `1e` -> NUMBER 1 + NAME e; `1__2` -> NUMBER 1 + NAME `__2`; `1.` -> NUMBER 1 + OPERATOR `.`; `.5` -> OPERATOR `.` + NUMBER 5;
  `0x1A` -> NUMBER 0 + NAME `x1A`; no sign (a leading `-` is the unary operator). [verified]
- Value: `0 + str_replace('_', '', $match)` = PHP numeric-string conversion: a string with `.`, `e` or `E` becomes a float
  (`1.0` prints `1`, `1e3` prints `1000`, `1e1000` is `INF`); an integer string becomes an int, or a float when it overflows int64
  (`9223372036854775807` stays int, `9223372036854775808` prints `9.2233720368548E+18`, `922337203685477580700` prints
  `9.2233720368548E+20`); leading zeros are decimal (`007` -> 7). [verified]
- In error messages the value is formatted with PHP `%s`, i.e. PHP float-to-string (precision 14): `"1.5"`, `"1.0E+20"`
  (`{{ 1 1e20 }}` -> `Unexpected token "number" of value "1.0E+20" ("end of print statement" expected) ... column 6.`).
  A value of `0`/`0.0` (falsy) suppresses the ` of value "..."` part of `TokenStream::expect()` messages (section 3). [verified]

### 2.12 Names (`src/Lexer.php:51`, `388-391`)

`[a-zA-Z_\x7f-\xff][a-zA-Z0-9_\x7f-\xff]*` on **bytes**: every UTF-8 multi-byte character (all bytes >= 0x80) and DEL (0x7f)
are name characters, so `café`, `é`, `§`, `日本`, and even `a` followed by U+00A0 NBSP are names. [verified: `{{ café }}`,
`{{ é }}`, `{% § %}` (LexerTest), `{{ a\u00a0}}` is variable `a\u00a0`]. Names are tried after operators, so a name that is exactly
an operator word followed by `[\s()\[{]` becomes an OPERATOR token (`and`, `or`, `xor`, `not`, `in`, `is`, `matches`, ...).

### 2.13 Punctuation and bracket checking (`src/Lexer.php:398-402`, `663-679`)

```
checkBrackets(c):
    if c in ['{', '(', '[']: brackets.push([c, lineno])
    elif c in ['}', ')', ']']:
        if brackets empty: throw SyntaxError('Unexpected "%s".' % c, lineno, column = getColumn(cursor))
        (expect, line) = brackets.pop()
        if c != str_replace(['{','(','['], ['}',')',']'], expect):     # '"' and '#{' never equal any closer
            throw SyntaxError('Unclosed "%s".' % expect, line)             # no column
```

Bracket stack entries: `{`, `(`, `[` (from punctuation/operators), `"` (double-quoted string), `#{` (interpolation).
[verified: `{{ 1 + 3) }}` -> `Unexpected ")" in "index" at line 1 column 9.`; `{{ { a: 1 }}` -> `Unexpected "}" ... column 12`;
`{{ ({ a: 1) }}` -> `Unclosed "{" in "index" at line 1.`; `{{ "#{ 1 )" }}` -> `Unclosed "#{" in "index" at line 1.`].

### 2.14 Comments, inline comments, documentation comments

Regular comment (`lexComment(false)`, `src/Lexer.php:509-527`): after the opener, `lex_comment` is **searched** (not anchored) from
the cursor; no match -> `SyntaxError('Unclosed comment.', lineno)` where lineno is the line of the `{#` (only the opener was
consumed). The comment text is discarded; the closer alternatives are `-#}` + all whitespace, `~#}` + `[ \t\0\x0B]*`, or `#}` + one
optional newline (`\r\n`, `\r` or `\n`). Comments do not nest; `{# {{ x }} #}` is fine; `{#}#}after` renders `after`;
`{{ x }}{#}` -> `Unclosed comment.` [verified]

Documentation comments (3.29, experimental; metadata only, no effect on output): opener `{##` (not followed by `}`), closer `#}`
or symmetric `##}` with the same trim variants. The text (with one trailing `#` removed when the closer was `##}`) is
newline-normalized and trimmed and, if non-empty, queued; inline `## ...` comments inside `{{ }}`/`{% %}` are queued the same way.
Queued documentation is attached to the next pushed token that is not a whitespace-only TEXT token (`Lexer.php:575-578`);
`{% verbatim %}` and `{% line %}` clear the queue. The parser copies token documentation onto `PrintNode`s, tag nodes, set/for
targets, macro arguments and `types` entries (`NodeDocumentation`, `src/Node/NodeDocumentation.php`). A port that does not need
the metadata only has to lex `{##`/`##}` correctly. `{## doc ##}a{##}b{## d2 -##}  c` renders `abc`. [verified]

Inline comments (3.15): inside expressions `#` up to (not including) `\r` or `\n` is skipped (`REGEX_RAW_INLINE_COMMENT`). They
cannot appear inside strings (the string regexes consume `#`), and `{{ "x" # c }}` swallows the `}}`.

### 2.15 Line numbers, offsets, columns, newline normalization

- Line counting treats `\r\n`, `\r` and `\n` each as one line break (`moveCursor`). [verified: `a\rb{{ x }}` -> VAR_START at line 2]
- `normalizeNewlines()` (`str_replace(["\r\n", "\r"], "\n", ...)`) is applied to TEXT token values, string literal contents
  (before escape processing), verbatim text and documentation. Output of a CRLF template therefore contains only LF:
  `a\r\nb` renders `a\nb`; `{{ 1 }}\r\n{{ 2 }}` renders `1\n2`; `{{ 'a\r\nb' }}` renders `a\nb`. [verified]
- Token line = line of the first byte of the token; closing tokens get the line of the marker (after leading whitespace):
  `{{ x\n}}` -> VAR_END at line 2. Multi-line strings keep the start line. [verified]
- Offsets are byte offsets into the ORIGINAL source (not the normalized text). Columns: section 1.4.
- `TEXT` token offsets: start of the text; BLOCK/VAR_START: offset of `{%`/`{{`; EOF: `strlen(code)`.

### 2.16 Whitespace control summary [all verified]

| construct | removes |
|---|---|
| `{{-` `{%-` `{#-` `{##-` | trailing whitespace of the preceding TEXT with PHP `rtrim()` default set `" \t\n\r\0\x0B"` (includes NUL, excludes `\f`) |
| `{{~` `{%~` `{#~` | trailing `" \t\0\x0B"` of the preceding TEXT (keeps `\r`, `\n`) |
| `-}}` `-%}` `-#}` `-##}` | leading whitespace after the tag with PCRE `\s*` = `[\t\n\x0B\f\r ]` (includes `\f`, excludes NUL) |
| `~}}` `~%}` `~#}` `~##}` | leading `[ \t\0\x0B]` after the tag (keeps newlines) |
| plain `%}` and `#}` | exactly one following newline (`\r\n`, `\r` or `\n`) |
| plain `}}` | nothing |
| plain `%}` of `{% verbatim %}`, `{% endverbatim %}`, `{% line N %}` | nothing |

The `-`/rtrim vs `\s` asymmetry is real: `a\f{{- 1 -}}\fb` renders `a\f1b`; `a\0{{- 1 -}}\0b` renders `a1\0b`.
`{%- if true -%}\n  a  \n{%- endif -%}` renders `a`. To remove whitespace between tags use `{#--#}`.

### 2.17 Lexer error catalogue

| message (raw, verbatim) | trigger | line | column | source |
|---|---|---|---|---|
| `Unclosed "%s".` (`(` `[` `{` `"` `#{`) | bracket/string/interpolation still open at end of input | line where it was opened | no | `Lexer.php:260` |
| `Unclosed "%s".` (`block`/`variable`) | whitespace inside `{% %}`/`{{ }}`/interpolation runs to end of input | line of the `{%`/`{{` | no | `Lexer.php:374` |
| `Unexpected character "%s".` | no lexeme matches (e.g. `$`, `@`, `;`, `!` alone, `'` unterminated, NUL) | current | yes | `Lexer.php:423`, `549` |
| `Unexpected end of file: Unclosed "verbatim" block.` | no `endverbatim` | line after the opener | no | `Lexer.php:487` |
| `Unclosed comment.` | no comment closer | line of the opener | no | `Lexer.php:513` |
| `Unexpected "%s".` | closing bracket with empty stack | current | yes | `Lexer.php:671` |
| `Unclosed "%s".` | closing bracket does not match the top of stack | line of the opener | no | `Lexer.php:676` |

---

## 3. TokenStream and Token::test

`src/TokenStream.php`. The stream always ends with the EOF token.

- `next()` (`:50-57`): advances and returns the previous token; if there is no token after the advance it throws
  `SyntaxError('Unexpected end of template.', <line of the last token, i.e. EOF>)` (no column). This is what an unclosed
  `if` (any branch) or an unclosed main body of `for`/`guard` produces: `{% if x %}\n\nabc` ->
  `Unexpected end of template in "index" at line 3.` [verified] (see the introduction of section 6 for the other case).
- `nextIf($primary, $secondary = null)` (`:64-67`): `test()` then `next()`.
- `expect($type, $value = null, ?string $message = null)` (`:72-90`):

```
token = current
if !token.test(type, value):
    throw SyntaxError(sprintf('%sUnexpected token "%s"%s ("%s" expected%s).',
        message ? message + '. ' : '',
        token.toEnglish(),
        token.value ? sprintf(' of value "%s"', token.value) : '',      # PHP truthiness: '', '0', 0, 0.0 suppress it
        Token::typeToEnglish(type),
        value ? sprintf(' with value "%s"', value) : ''),
      token.line, source, column = source.getColumn(token.offset ?? -1))
next(); return token
```

  Examples [verified]: `A mapping key must be followed by a colon (:). Unexpected token "punctuation" of value "}" ("punctuation" expected with value ":") in "index" at line 1 column 8.`;
  `{{ 1 0 }}` -> `Unexpected token "number" ("end of print statement" expected) in "index" at line 1 column 6.`;
  `{{ 1 "0" }}` -> `Unexpected token "string" ("end of print statement" expected) ...`.
- `look()` (`:95-102`) is unused by core. `injectTokens()` (`:42-45`) inserts tokens at the current position (used by `embed`).

`Token::test($type, $values = null)` (`src/Token.php:76-125`):
- one argument that is not an int means `test(NAME_TYPE, $values)`;
- values: array -> strict `in_array`; scalar -> loose `==`; null -> type only;
- BC: `test(OPERATOR_TYPE)` (no value) also matches PUNCTUATION tokens, and `test(OPERATOR_TYPE, v)` matches a PUNCTUATION token
  when `v` is one of `( [ | . ? ?:`; testing a PUNCTUATION token of those values with a PUNCTUATION type triggers a 3.21
  deprecation. Irrelevant for lexer-produced tokens (those characters are always OPERATOR), relevant only for synthesized tokens.

---

## 4. Parser core

### 4.1 `parse()` and Module assembly (`src/Parser.php:86-163`)

```
parse(stream, test = null, dropNeedle = false) -> ModuleNode:
    embeddedParent = this.embeddedTemplateParent; this.embeddedTemplateParent = null
    if stack empty: lastEmbedIndex = 0                     # embed indices are unique across nested parses of one root parse
    stack.push(snapshot of all properties except stack, env, handlers, visitors, expressionParser, reservedMacroNames,
               lastEmbedIndex, varNameSalt)
    visitors ??= env.getNodeVisitors()
    this.stream = stream; parent = null; blocks = {}; documentationTargets = []; macros = {}; traits = []
    blockStack = []; importedSymbols = [{}]; embeddedTemplates = []; expressionRefs = new WeakMap
    try:   body = subparse(test, dropNeedle)
    catch SyntaxError e:
        if e.source == null: e.setSourceContext(stream.source)
        if !e.lineno (== 0): e.setTemplateLine(currentToken.line)
        rethrow
    finally: expressionRefs = null
    if embeddedParent: parent = embeddedParent              # overrides the injected {% extends "__parent__" %}
    if parent: body = cleanupBodyForChildTemplates(body)   # 4.5
    node = new ModuleNode(new BodyNode([body]), parent,
                          blocks ? new Nodes(blocks) : new EmptyNode(),
                          new MacrosNode(macros),
                          traits ? new Nodes(traits) : new EmptyNode(),
                          embeddedTemplates ? new Nodes(embeddedTemplates) : new EmptyNode(),
                          stream.source)
    node = new NodeTraverser(env, visitors).traverse(node)
    restore snapshot
    return node
```

ModuleNode (`src/Node/ModuleNode.php:37-80`): child nodes `body, blocks, macros, traits, display_start, display_end,
constructor_start, constructor_end, class_end` and `parent` (only if set); attributes `index` (null, set by `embedTemplate()`),
`embedded_templates` (an attribute so visitors do not traverse them again), `strategy`, `escaper`; line 1. `blocks` maps
name -> `BodyNode([BlockNode])`; `macros` maps name -> `MacroNode`; `traits` is a list of `Nodes(['template' => expr, 'targets' => Nodes(name => ConstantExpression(alias))])`.

Node visitors (`src/NodeTraverser.php:48-58`): grouped by `getPriority()`, `ksort` ascending, each visitor makes a complete pass
over the tree before the next one. Default visitors: `CorrectnessNodeVisitor` (-255), `EscaperNodeVisitor` (0),
`YieldNotReadyNodeVisitor` (255), `OptimizerNodeVisitor` (255); `SandboxNodeVisitor` (0) with the sandbox extension.
Consequence: ALL parse errors anywhere in the template win over correctness errors
(`x{% extends 'b' %}{{ 1 2 }}` reports the `1 2` parse error). [verified]

### 4.2 `subparse()` (`src/Parser.php:204-293`)

```
subparse(test, dropNeedle) -> Node:
    lineno = currentToken.line                      # used for "defined near line" and for the Nodes container
    rv = []
    while !stream.isEOF():
        tok = current
        if tok is TEXT:       next(); rv.push(TextNode(tok.value, tok.line))
        elif tok is VAR_START:
            next(); expr = parseExpression(); stream.expect(VAR_END)
            node = PrintNode(expr, tok.line); attach doc of tok; rv.push(node)
        elif tok is BLOCK_START:
            startToken = next()
            token = current
            if !token.test(NAME):
                throw SyntaxError('A block must start with a tag name.', token.line)          # no column
            if test != null and test(token):
                if dropNeedle: next()               # consume the end-tag NAME
                return count(rv) == 1 ? rv[0] : new Nodes(rv, lineno)
            subparser = env.getTokenParser(token.value)        # exact map lookup, then undefined-token-parser callbacks
            if !subparser: throw unknown/unexpected tag error (4.3)
            next()                                  # consume the tag NAME
            subparser.setParser(this)
            push documentation-target slot; node = subparser.parse(token); pop slot
            if node == null: deprecation 'Returning "null" from "%s" is deprecated and forbidden by "TokenParserInterface".'
            else: node.setNodeTag(subparser.getTag()); attach doc of startToken (or move it to the target); rv.push(node)
        else:
            throw SyntaxError('The lexer or the parser ended up in an unsupported state.', currentToken.line)   # unreachable with core
    return count(rv) == 1 ? rv[0] : new Nodes(rv, lineno)
```

Key properties:
- A body with exactly one node is that node (not wrapped); zero nodes give an empty `Nodes`.
- `token` passed to the token parser is the tag NAME token, so tag nodes get the line of the tag name (which differs from the
  `{%` line for `{%\nif x %}`).
- End tags are recognized by the caller-supplied `test`; the test never sees nested tags because nested tags are consumed by
  their own token parsers first.

### 4.3 Unknown/unexpected tag errors (`src/Parser.php:244-258`)

```
if test != null:
    e = SyntaxError(sprintf('Unexpected "%s" tag', token.value), token.line)
    callable = test (normalized)
    if callable is [TokenParserInterface obj, method]:
        e.appendMessage(sprintf(' (expecting closing tag for the "%s" tag defined near line %s).', obj.getTag(), lineno))
else:
    e = SyntaxError(sprintf('Unknown "%s" tag.', token.value), token.line)
    e.addSuggestions(token.value, array_keys(env.getTokenParsers()))
throw e
```

- No suggestions are offered inside a body: `{% if x %}{% foo %}{% endif %}` ->
  `Unexpected "foo" tag (expecting closing tag for the "if" tag defined near line 1) in "index" at line 1.` [verified]
- "near line" is the line of the **first token of the enclosing body**, not of the opening tag:
  `{% if x %}\n{% endfor %}` -> `... defined near line 2) in "index" at line 2.` (the newline after `%}` is eaten, so the body
  starts on line 2); `{% if x %}\n\n  {{ y }}{% endfor %}` -> `near line 2 ... at line 3`. [verified]
- All core end-tag tests are `[$this, 'decideXxx']` arrays, so the suffix is always present for core tags. A custom token parser
  using a closure would produce `Unexpected "x" tag in "index" at line N` (no period) - UNVERIFIED.
- Stray end tags at top level: `{% endif %}` -> `Unknown "endif" tag.`; `{% endblock %}`, `{% endfor %}`, `{% else %}`,
  `{% endverbatim %}` likewise (no suggestion qualifies). [verified]
- Removed/foreign tags: `filter`, `spaceless` (removed in 3.0), `raw`, `cache` (twig/cache-extra), `sandbox` (without the sandbox
  extension) -> `Unknown "<name>" tag.` [verified]

### 4.4 Blocks, macros, traits, embedded templates, imported symbols

- `setBlock(name, BlockNode)` (`Parser.php:334-341`): duplicate -> `SyntaxError(sprintf("The block '%s' has already been defined line %d.", $name, <first block line>), <current token line>, <first block source>)`
  (note the single quotes). Called when the second `{% block %}` tag name has been read, before its body. Stored as
  `BodyNode([BlockNode], [], blockLine)`.
- `blockStack` (`pushBlockStack/popBlockStack/peekBlockStack`, `Parser.php:305-318`): names of the blocks currently being
  parsed; used by `parent()`.
- `setMacro(name, MacroNode)` (`Parser.php:362-369`): a redefinition only triggers the 3.29 deprecation
  `Defining the macro "%s" more than once in "%s" is deprecated and will throw a SyntaxError in Twig 4.0 (previous definition at line %d, new definition at line %d). The last definition is used in Twig 3.`
  and the last definition wins. [verified]
- `addTrait()` (`:371-374`), `hasInheritance()` = `parent != null || traits not empty` (`:474-477`).
- `setParent(parent, throwOnMultiple = true)` (`:479-498`): `extends` calls it with `false`, so a second `extends` is silently
  ignored here (the first wins) and reported later by `CorrectnessNodeVisitor`.
- `embedTemplate(module)` (`:386-391`): `module.setIndex(++lastEmbedIndex)` then appended; indices are 1, 2, ... in source
  order (and across nesting). [ParserTest]
- Imported symbols (`:393-426`): `importedSymbols` is a list of scopes, index 0 = innermost, last = main scope.
  `pushLocalScope()` = `array_unshift(scopes, [])`, `popLocalScope()` = `array_shift`. Symbol types: `'template'` (from
  `import ... as x`), `'function'` (from `from ... import a as b`, value `['name' => macroName, 'node' => AssignMacroVariable]`).
  `getImportedSymbol(type, alias)` looks ONLY in scope 0 and in the main scope (intermediate scopes are skipped):
  an import made in `{% block a %}` is not visible in a nested `{% block b %}`; an import in the main scope is visible
  everywhere. `isMainScope()` = exactly one scope. Blocks and macros push a local scope. Embedded templates start with fresh
  scopes (imports of the enclosing template are not visible inside `embed`). [verified]
- `subparseIgnoreUnknownTwigCallables(test, dropNeedle)` (`:188-199`): used by `guard`; parses with unknown
  functions/filters/tests replaced by dummy callables returning `''`, then restores parent, blocks, documentationTargets, macros,
  importedSymbols, traits and embeddedTemplates (everything registered in the discarded branch disappears). Unknown TAGS still
  error. [verified]

### 4.5 Child templates: body cleanup and `CorrectnessNodeVisitor`

`cleanupBodyForChildTemplates(body)` (`Parser.php:617-637`), applied when the module has a parent (including embedded templates):

```
if body is BlockReferenceNode: return EmptyNode
if body is TextNode and body.isBlank(): return EmptyNode
for (k, node) in body (child nodes of whatever body is):
    if node is BlockReferenceNode or (node is TextNode and node.isBlank()): body.removeNode(k)      # keys are preserved
return body
```

`TextNode::isBlank()` (`src/Node/TextNode.php:42`): `ctype_space(data)` (C locale: space, `\t\n\r\v\f`), or, if the node's debug
string contains the UTF-8 BOM bytes `EF BB BF`, `t = substr(data, 3)` and `t === '' || ctype_space(t)`. So a leading BOM
(optionally followed by whitespace) before `{% extends %}` is allowed. [verified]

`CorrectnessNodeVisitor` (`src/NodeVisitor/CorrectnessNodeVisitor.php`, priority -255) then runs over the finished module:

1. `enterModule` (`:96-108`): `hasParent = module has 'parent'`; root nodes = `body.getNode('body').getNode('0')` itself if it is
   a single non-container node, else its children (`:125-133`). If `hasParent`, every root node must satisfy
   `isEmptyOutputNode()` (`:235-258`) or:
   `A template that extends another one cannot include content outside Twig blocks. Did you forget to put the content inside a {% block %} tag?`
   (line = line of that ROOT node, e.g. of the `if` that contains the text).
   `isEmptyOutputNode(n)`: `NodeCaptureInterface` (only `SetNode`) -> true; `TextNode` -> `isBlank()`; any other
   `NodeOutputInterface` (`PrintNode`, `IncludeNode`/`EmbedNode`; but not `BlockReferenceNode`) -> false; otherwise true iff all
   children are empty-output.
   Allowed at the root of a child template [verified]: `set` (even capturing text/prints), `do`, `deprecated`, `import`, `from`,
   `macro`, `types`, `flush`, `with`/`if`/`for`/`guard`/`autoescape` without output inside, `use`, `extends`. Rejected:
   text, `{{ }}`, `include`, `embed`, `apply` (contains a `PrintNode`), `for`/`if`/`with`/... containing text or prints.
2. Scope tracking (`:135-163`): `NodeCaptureInterface` increments `capturingNodeDepth`; `BlockNode` -> `blockDepth`;
   `MacroNode` -> `macroDepth`; any other node with a tag (except `BlockReferenceNode`) is pushed on `tagStack`.
3. `ConfigNode` (`extends`, `use`, `macro` declaration): if tag is `extends` (`:202-216`):
   in a block -> `Cannot use "extend" in a block.`; in a macro -> `Cannot use "extend" in a macro.`; second one ->
   `Multiple extends tags are forbidden.` (reported on the second `extends`, after the body has been traversed; blocks and macros
   are traversed after the body). Then, if the node is not a root node: 3.28 deprecation
   `Using the "%s" tag outside the root of a template is deprecated in %s at line %d.` (template name without quotes). [verified]
4. `BlockReferenceNode` (`:218-230`): if `hasParent` and not inside a block/macro/capture and `tagStack` is non-empty:
   `A "block" tag cannot be under a "%s" tag (line %d).` (tag and line of the innermost enclosing tag; reported at the block's line).
   Blocks under tags are fine in non-child templates, inside other blocks, and inside `set` captures. [verified]
5. `MacroReferenceExpression` (`:165-189`): macro called without parentheses (and not in a `defined` test) -> 3.29 deprecation
   `Omitting parentheses when calling the macro "%s" is deprecated and will throw a SyntaxError in Twig 4.0; add parentheses after the macro name in "%s" at line %d.`
   (dynamic names: `Omitting parentheses when calling a macro is deprecated ...`), reported once per (source, line, name).

### 4.6 Function / filter / test lookup (`src/Parser.php:510-615`)

- `getFunction(name, line)`: `env.getFunction(name)` = exact name, then dynamic `*` patterns
  (`'#^' + str_replace('\*', '(.*?)', preg_quote(name, '#')) + '$#'`, in registration order), then undefined-function callbacks.
  Missing -> `Unknown "%s" function.` + suggestions over `array_keys(env.getFunctions())`. In ignore mode (guard) a dummy is
  returned. Deprecated callables trigger their deprecation with `path ?: name` and the line.
- `getFilter(name, line)`: same with `Unknown "%s" filter.`.
- `getTest(line)`: reads `name = expect(NAME).value`; **if the next token is also a NAME**, `name = name + ' ' + next.value`,
  looks up that two-word name and consumes the second token (no fallback to the one-word test!); else looks up the one-word name.
  Missing -> `Unknown "%s" test.` + suggestions over test names. [verified: `{{ x is odd foo }}` -> `Unknown "odd foo" test.`;
  `{{ 1 is same x }}` -> `Unknown "same x" test. Did you mean "same as" ...?`; `{{ 1 is divisible }}` ->
  `Unknown "divisible" test. Did you mean "divisible by" ...?`]
- Error line: the line passed by the caller (filter-name token line for filters, `(` token line for functions, `is` token line
  for tests).

---

## 5. Expression grammar

### 5.1 Precedence climbing (`Parser::parseExpression`, `src/Parser.php:442-462`)

```
parseExpression(minPrec = 0):
    token = current
    if token.test(OPERATOR) and (ep = registry.prefix[token.value]):
        next(); expr = ep.parse(this, token); checkPrecedenceDeprecations(ep, expr)     # no precedence check for prefix
    else:
        expr = LiteralExpressionParser.parse(this, token)
    token = current
    while token.test(OPERATOR) and (ep = registry.infix[token.value]) and ep.precedence >= minPrec:
        next(); expr = ep.parse(this, expr, token); checkPrecedenceDeprecations(ep, expr)
        token = current
    return expr
```

Binary operators parse their right operand with `parseExpression(assoc == Left ? prec + 1 : prec)`
(`src/ExpressionParser/Infix/BinaryOperatorExpressionParser.php:44-49`). Registry: `ExpressionParsers`
(`src/ExpressionParser/ExpressionParsers.php`), separate prefix and infix maps keyed by operator token (name and aliases);
later registrations with the same token override earlier ones; precedences outside [0, 512] only deprecate (3.21).

### 5.2 Complete operator table (`src/Extension/CoreExtension.php:340-409`) [dumped from the oracle]

| prec | token(s) | kind | assoc | parser -> node | 4.0 change |
|---|---|---|---|---|---|
| 512 | `...` | prefix | - | `SpreadExpressionParser` -> always errors (5.3.6) | |
| 512 | `\|` | infix | Left | `FilterExpressionParser` -> filter node class | -> 300 (3.21) |
| 512 | `(` | infix | Left | `FunctionExpressionParser` -> function node class / `MacroReferenceExpression` | |
| 512 | `.` `?.` | infix | Left | `DotExpressionParser` -> `GetAttrExpression` / `MacroReferenceExpression` | |
| 512 | `[` | infix | Left | `SquareBracketExpressionParser` -> `GetAttrExpression` (array) / slice filter | |
| 500 | `-` | prefix | - | `UnaryOperatorExpressionParser` -> `NegUnary` | |
| 500 | `+` | prefix | - | -> `PosUnary` | |
| 300 | `??` | infix | Right | -> `NullCoalesceBinary` | -> 5 (3.15) |
| 250 | `=>` | infix | Left | `ArrowExpressionParser` -> `ArrowFunctionExpression` | |
| 200 | `**` | infix | Right | -> `PowerBinary` | |
| 100 | `is`, `is not` | infix | Left | `IsExpressionParser` / `IsNotExpressionParser` -> test node class (wrapped in `NotUnary` for `is not`) | |
| 60 | `*` `/` `//` `%` | infix | Left | `MulBinary` `DivBinary` `FloorDivBinary` `ModBinary` | |
| 50 | `not` | prefix | - | -> `NotUnary` | -> 70 (3.15) |
| 40 | `~` | infix | Left | -> `ConcatBinary` | -> 27 (3.15) |
| 30 | `+` `-` | infix | Left | `AddBinary` `SubBinary` | |
| 25 | `..` | infix | Left | `RangeBinary` | |
| 20 | `==` `!=` `<=>` `<` `>` `>=` `<=` `not in` `in` `matches` `starts with` `ends with` `has some` `has every` `===` `!==` | infix | Left | `EqualBinary` `NotEqualBinary` `SpaceshipBinary` `LessBinary` `GreaterBinary` `GreaterEqualBinary` `LessEqualBinary` `NotInBinary` `InBinary` `MatchesBinary` `StartsWithBinary` `EndsWithBinary` `HasSomeBinary` `HasEveryBinary` `SameAsBinary` `NotSameAsBinary` | |
| 18 | `b-and` | infix | Left | `BitwiseAndBinary` | |
| 17 | `b-xor` | infix | Left | `BitwiseXorBinary` | |
| 16 | `b-or` | infix | Left | `BitwiseOrBinary` | |
| 15 | `and` | infix | Left | `AndBinary` | |
| 12 | `xor` | infix | Left | `XorBinary` | |
| 10 | `or` | infix | Left | `OrBinary` | |
| 5 | `?:`, `? :` | infix | Right | -> `ElvisBinary` | |
| 0 | `(` | prefix | - | `GroupingExpressionParser` (parentheses / arrow parameter list) | |
| 0 | `?` | infix | Left | `ConditionalTernaryExpressionParser` -> `ConditionalTernary` | |
| 0 | `=` | infix | Right | `AssignmentExpressionParser` -> `SetBinary` / `SequenceDestructuringSetBinary` / `ObjectDestructuringSetBinary` | |
| - | (no token) | prefix | - | `LiteralExpressionParser` (fallback) | |

Operators are case sensitive (`AND` is a name).

### 5.3 Prefix parsers

#### 5.3.1 Literal parser (`src/ExpressionParser/Prefix/LiteralExpressionParser.php:34-95`)

```
parse(parser, token):
    switch:
    NAME:   next()
            'true' | 'TRUE'                    -> ConstantExpression(true)
            'false' | 'FALSE'                  -> ConstantExpression(false)
            'none' | 'NONE' | 'null' | 'NULL'  -> ConstantExpression(null)
            otherwise                          -> ContextVariable(name)        # True, Null, etc. are variables
    NUMBER: next() -> ConstantExpression(value)
    STRING or INTERPOLATION_START -> parseStringExpression()
    PUNCTUATION '{' -> parseMappingExpression()
    (fallthrough) OPERATOR:
            '[' -> parseSequenceExpression()
            value fully matches REGEX_NAME -> next(); ContextVariable(value)   # "in this context, string operators are variable names"
    default: throw SyntaxError(sprintf('Unexpected token "%s" of value "%s".', token.toEnglish(), token.value), token.line)   # no column, value always printed
```

Special names `_self`, `_context`, `_charset` are ordinary `ContextVariable`s at parse time; the compiler maps them
(`src/Node/Expression/NameExpression.php`). Assigning to them is accepted but reading them always yields the special value
(`{% set _self = 'x' %}{{ _self }}` prints the template name). [verified]
Variables named like (non-prefix) operator words work: `{{ in }}`, `{{ is }}`, `{{ and }}`, `{{ literal }}`; `{{ not }}` does not
(prefix `not` wins) -> `Unexpected token "end of print statement" of value "" in "index" at line 1.` [verified]
Examples of the default error: `{{ }}` -> `Unexpected token "end of print statement" of value "".`; `{{ .5 }}` ->
`Unexpected token "operator" of value ".".`; `{{ (() => 1) }}` -> `Unexpected token "punctuation" of value ")".` [verified]

#### 5.3.2 Sequences `[...]` (`LiteralExpressionParser.php:146-176`)

```
expect(OPERATOR '[', 'A sequence element was expected')        # cannot fail here
node = ArrayExpression([], current.line); first = true
while !test(PUNCTUATION ']'):
    if !first:
        expect(PUNCTUATION ',', 'A sequence element must be followed by a comma')
        if test(PUNCTUATION ']'): break                          # trailing comma
    first = false
    if test(PUNCTUATION ','):          node.addElement(EmptyExpression(current.line))   # empty slot (only valid for destructuring)
    elif (t = nextIf(OPERATOR '...')): node.addElement(SpreadUnary(parseExpression(), t.line))
    else:                              node.addElement(parseExpression())
expect(PUNCTUATION ']', 'An opened sequence is not properly closed')    # cannot fail (loop only exits on ']')
```

`addElement(value, key = null)`: without a key, key = `ConstantExpression(++index)` where `index` starts at -1 (re-scanned only
in the constructor). Empty slots outside destructuring fail at COMPILE time with
`Empty array elements are only allowed in destructuring assignments.` (`src/Node/Expression/ArrayExpression.php:127`):
`{{ [1,,2] }}`, `{{ [,1] }}`. [verified]

#### 5.3.3 Mappings `{...}` (`LiteralExpressionParser.php:178-235`)

```
expect(PUNCTUATION '{', 'A mapping element was expected')       # cannot fail here
node = ArrayExpression([], current.line); first = true
while !test(PUNCTUATION '}'):
    if !first:
        expect(PUNCTUATION ',', 'A mapping value must be followed by a comma')
        if test(PUNCTUATION '}'): break
    first = false
    if (t = nextIf(OPERATOR '...')): node.addElement(SpreadUnary(parseExpression(), t.line)); continue    # auto numeric key
    if (t = nextIf(NAME)):
        key = ConstantExpression(t.value)
        if test(PUNCTUATION, [',', '}']): node.addElement(ContextVariable(t.value), key); continue        # {a} == {'a': a}
    elif (t = nextIf(STRING)) or (t = nextIf(NUMBER)): key = ConstantExpression(t.value)
    elif test(OPERATOR '('): key = parseExpression()            # a FULL expression that merely starts with '(':
                                                                 # {(a)|upper: 1} -> key 'K', {(1) + 1: 2} -> key 2 [verified]
    else:
        c = current
        throw SyntaxError(sprintf('A mapping key must be a quoted string, a number, a name, or an expression enclosed in parentheses (unexpected token "%s" of value "%s".', c.toEnglish(), c.value), c.line)
    expect(PUNCTUATION ':', 'A mapping key must be followed by a colon (:)')
    node.addElement(parseExpression(), key)
expect(PUNCTUATION '}', 'An opened mapping is not properly closed')    # cannot fail
```

Notes [verified]: the "mapping key" message has an unbalanced parenthesis and ends with `".` (so the location is inserted before
the final `.`): `{{ {[1]: 2} }}` -> `A mapping key must be a quoted string, a number, a name, or an expression enclosed in parentheses (unexpected token "operator" of value "[" in "index" at line 1.`;
`{-1: 'a'}` fails the same way (no negative number keys); a string key with interpolation `{"a#{1}": 2}` -> `A mapping key must be followed by a colon (:). Unexpected token "begin of string interpolation" ("punctuation" expected with value ":") ...`;
`{and: 1}` works but `{and : 1}` fails (`and` becomes an operator token). Float keys are allowed (`{1.5: 'a'}` -> PHP truncates to
1 with an engine deprecation). Key order is preserved; numeric string keys behave like PHP array keys at runtime.

#### 5.3.4 Strings and interpolation (`LiteralExpressionParser.php:118-144`)

```
nodes = []; nextCanBeString = true
loop:
    if nextCanBeString and (t = nextIf(STRING)): nodes.push(ConstantExpression(t.value, t.line)); nextCanBeString = false
    elif nextIf(INTERPOLATION_START): nodes.push(parseExpression()); expect(INTERPOLATION_END); nextCanBeString = true
    else: break
expr = shift(nodes); for n in nodes: expr = ConcatBinary(expr, n, n.line)      # left-nested
return expr
```

Consequences [verified]: `{{ 'a' 'b' }}` -> `Unexpected token "string" of value "b" ("end of print statement" expected) ... column 8.`;
but after an interpolation a following string literal IS concatenated: `{{ "a#{1}" "b" }}` renders `a1b`; `"#{1 + 1}"` is the bare
expression (`"#{1 + 1}" is same as(2)` is true), not a string; `"#{}"` -> `Unexpected token "end of string interpolation" of value "".`;
`"#{ 1 2 }"` -> `Unexpected token "number" of value "2" ("end of string interpolation" expected) ... column 10.`

#### 5.3.5 Grouping and arrow parameter lists (`src/ExpressionParser/Prefix/GroupingExpressionParser.php:30-72`)

```
parse(parser, token):                                 # token = '('
    expr = parseExpression(0)
    if nextIf(PUNCTUATION ')'):
        if !test(OPERATOR '=>'): return expr.setExplicitParentheses()       # attribute with_parentheses = true
        return ListExpression([toAssign(expr)], token.line)
    if !test(PUNCTUATION ','): expect(PUNCTUATION ')', 'An opened parenthesis is not properly closed')
    names = [expr]
    loop:
        if nextIf(PUNCTUATION ')'): break
        expect(PUNCTUATION ',')
        t = expect(NAME); names.push(ContextVariable(t.value, t.line))
    if !test(OPERATOR '=>'):
        throw SyntaxError('A list of variables must be followed by an arrow.', current.line)
    return ListExpression(map(toAssign, names), t.line)

toAssign(e): if e is not ContextVariable: throw SyntaxError('A list must only contain variables.', e.line)
             return e is AssignContextVariable ? e : AssignContextVariable(e.name, e.line)   # may throw "You cannot assign a value to"
```

[verified] `{{ (a b) }}` -> `An opened parenthesis is not properly closed. Unexpected token "name" of value "b" ("punctuation" expected with value ")") ... column 7.`;
`{{ (1, 2) }}` -> `Unexpected token "number" of value "2" ("name" expected) ... column 8.`;
`(a, b) + 1` -> `A list of variables must be followed by an arrow.`; `(a + 1) => 2` -> `A list must only contain variables.`;
`(a, b c) => 1` -> `Unexpected token "name" of value "c" ("punctuation" expected with value ",") ...`.
Parenthesized names keep being `ContextVariable`s, so `(max)(1, 2)` calls the function `max`, and `(a) = 1` assigns. [verified]

#### 5.3.6 Unary operators and spread

- `UnaryOperatorExpressionParser::parse` (`src/ExpressionParser/Prefix/UnaryOperatorExpressionParser.php:43-46`):
  `new NodeClass(parseExpression(operandPrecedence ?? precedence), token.line)`. So `not` takes an operand of precedence >= 50
  (`not a and b` = `(not a) and b`; `not a * b` = `not (a * b)`; `not x is defined` = `not (x is defined)`), and `-`/`+` take only
  operands of precedence >= 500 (postfix `.`, `[`, `(`, `|` bind tighter: `-a.b` = `-(a.b)`, `-1|abs` = `-(1|abs)` = `-1`).
  `- - 2` = 2, `1 + + 2` = 3. [verified]
- `SpreadExpressionParser::parse` (`src/ExpressionParser/Prefix/SpreadExpressionParser.php:29-32`) always throws
  `The spread operator can only be used on sequence elements, mapping elements, and call arguments.` (no column). Sequences,
  mappings and argument lists consume `...` themselves before calling `parseExpression`. [verified, plus the 15 cases in
  `tests/ExpressionParserTest.php:143-162`]

### 5.4 Infix parsers

#### 5.4.1 Binary operators (`BinaryOperatorExpressionParser.php:44-49`)

`right = parseExpression(Left ? prec + 1 : prec); return new NodeClass(left, right, token.line)`.
Node-constructor side effects:
- `MatchesBinary` (`src/Node/Expression/Binary/MatchesBinary.php:24-40`): if the right operand is a `ConstantExpression`, runs
  `preg_match($regexp, '')` and converts the PHP warning into
  `SyntaxError(sprintf('Regexp "%s" passed to "matches" is not valid: %s.', $regexp, substr($phpWarning, 14)), $lineno)`
  (strips `preg_match(): `). `{{ 'a' matches '/a' }}` -> `Regexp "/a" passed to "matches" is not valid: No ending delimiter '/' found in "index" at line 1.`;
  `matches 3` -> `... is not valid: Delimiter must not be alphanumeric, backslash, or NUL byte ...`. The port needs PCRE2 pattern
  validation with PHP's exact warning texts. [verified]
- `NullCoalesceBinary` (`.../NullCoalesceBinary.php:31-47`): builds `DefinedTest(clone left)` (and `not (left is null)` unless
  left is `BlockReferenceExpression`) and sets `always_defined` on left. Therefore the LEFT operand of `??` must support the
  defined test (5.6): `{{ (1 + 2) ?? 3 }}`, `{{ x|upper ?? 'd' }}` -> `The "defined" test only works with simple variables in "index" at line 1.`;
  allowed: variables, attribute access, function calls, `block()`, macro references, constants, sequence/mapping literals
  (`[1] ?? 'd'`, `'a' ?? 'd'`). [verified]
- `ElvisBinary` (`.../ElvisBinary.php:26-32`): test node `TrueTest::wrap(clone left)`, `always_defined` on left; no restriction.
- `ConcatBinary` etc.: no checks.

#### 5.4.2 Conditional `?` (`src/ExpressionParser/Infix/ConditionalTernaryExpressionParser.php:29-41`)

```
then = parseExpression(0)
else = nextIf(PUNCTUATION ':') ? parseExpression(0) : ConstantExpression('', token.line)
return ConditionalTernary(left, then, else, token.line)
```
Both branches use precedence 0, so `a ? b : c ? d : e` = `a ? b : (c ? d : e)` and `1 ? 2 ? 3 : 4 : 5` = 3. `x ? 'y'` yields `''`
when false. [verified]

#### 5.4.3 Assignment `=` and destructuring (`src/ExpressionParser/Infix/AssignmentExpressionParser.php:42-74`, 3.23)

```
parse(parser, left, token):
    if left is not ContextVariable and not ArrayExpression:
        throw SyntaxError(sprintf('Cannot assign to "%s", only variables can be assigned.', left::class), token.line)
    right = parseExpression(0)                                # right associative
    if left is ArrayExpression:
        pairs = left.getKeyValuePairs(); seq = left.isSequence()
        if pairs empty or (seq and every value is EmptyExpression):
            throw SyntaxError('Cannot destructure to an empty list of variables.', token.line)
        for i, pair: if pair.value is ContextVariable (not Assign...): left.setNode(2*i+1, AssignContextVariable(name, line))
        return seq ? SequenceDestructuringSetBinary(left, right, token.line) : ObjectDestructuringSetBinary(left, right, token.line)
    return SetBinary(left, right, token.line)                 # SetBinary converts left to AssignContextVariable
```

- `left::class` is the PHP FQCN, e.g. `Cannot assign to "Twig\Node\Expression\ConstantExpression", only variables can be assigned.`
  (`{% do 1 = 2 %}`, `{% do none = 1 %}`, `{{ a = 1 = 2 }}`), `"...\GetAttrExpression"` (`a.b = 2`), `"...\Binary\AddBinary"` (`a + b = 1`). [verified]
- `isSequence()` (`ArrayExpression.php:69`): every key is a `TempNameExpression` or `ConstantExpression` whose value is
  identical (`===`) to its position. So `{0: z} = ['zero']` and even `{...a} = b` are SEQUENCE destructuring. [verified]
- Destructuring targets must be plain variables: `SequenceDestructuringSetBinary` (`:33-45`) ->
  `Cannot assign to "%s", only variables can be assigned in sequence destructuring.` (allowed: `AssignContextVariable`,
  `EmptyExpression`); `ObjectDestructuringSetBinary` (`:34-48`) ->
  `Cannot assign to "%s", only variables can be assigned in object/mapping destructuring.`; nested patterns are rejected
  (`...\ArrayExpression`); a non-constant object key (`{(a): b} = x`) escapes as a LogicException wrapped into
  `An exception has been thrown during the compilation of a template ("Attribute "value" does not exist for Node "Twig\Node\Expression\Variable\ContextVariable".") in "index".` [verified]
- `True`, `NULL`-like names in targets go through `AssignContextVariable` -> `You cannot assign a value to "True".`. [verified]
- `set`/`for` do not accept destructuring patterns (`{% set [a, b] = ... %}` -> `Only variables can be assigned to. Unexpected token "operator" of value "[" ("name" expected) ... column 8.`). [verified]
- Runtime value of the expression (compile semantics, [verified]): `x = 5` -> 5; sequence destructuring -> the right-hand array padded
  with nulls to the pattern length (`([a, b] = [1, 2, 3])` -> `[1,2,3]`, `([a, b, c] = [1])` -> `[1,null,null]`), a Traversable
  -> the iterator; object destructuring -> a PHP list of the extracted values (`({name} = user)` -> `["N"]`; the docs' claim that it
  returns the right-hand value is wrong for object destructuring).

#### 5.4.4 Tests `is` / `is not` (`src/ExpressionParser/Infix/IsExpressionParser.php:37-70`, `IsNotExpressionParser.php:24-27`)

```
parse(parser, left, token):
    test = parser.getTest(token.line)                          # 4.6, two-word rule
    args = null
    if test(OPERATOR '('):            args = parseNamedArguments()                     # 5.5
    elif test.hasOneMandatoryArgument(): args = Nodes([0 => parseExpression(100)])    # 'same as', 'divisible by'
    if test.name == 'defined':
        if left is NameExpression and a 'function' import named left.name exists:
            left = MacroReferenceExpression(alias var, macroName, ArrayExpression([]), left.line); left.hasCallParentheses = false
        if left is MacroReferenceExpression with call parentheses: 3.29 deprecation 'Using parentheses when testing a macro with the "defined" test is deprecated and will throw a SyntaxError in Twig 4.0; remove the parentheses after the macro name in "%s" at line %d.'
    return new test.nodeClass(left, test, args, current.line)    # is not: NotUnary(that, token.line)
```

- The one-mandatory-argument is parsed at precedence 100 and `is` itself has precedence 100: `2 is same as 2 + 1` =
  `(2 is same as 2) + 1` = 2; `3 is divisible by 2 + 1` = 1; `1 is same as 1 is same as true` = `1 is same as (1 is same as true)` = false;
  `1 is divisible by` -> `Unexpected token "end of print statement" of value "".` [verified]
- Test arguments are parsed AFTER the test lookup, so `{{ 1 is nope(1 2) }}` reports the unknown test; for filters and functions
  the arguments are parsed BEFORE the lookup, so `{{ nope(1 2) }}` / `{{ 1|nope(1 2) }}` report the comma error. [verified]
- `DefinedTest` constructor (5.6) restricts the tested expression.

#### 5.4.5 Filters `|` (`src/ExpressionParser/Infix/FilterExpressionParser.php:35-59`)

```
t = expect(NAME); line = t.line
args = test(OPERATOR '(') ? parseNamedArguments() : EmptyNode()
filter = parser.getFilter(t.value, line)
return new filter.nodeClass(left, filter, args, line)        # default FilterExpression; e.g. DefaultFilter, EscapeFilter, RawFilter
```
`{{ x| }}` -> `Unexpected token "end of print statement" ("name" expected) ... column 7.`; `{{ x|'a' }}` -> `Unexpected token "string" of value "a" ("name" expected) ...`. [verified]

#### 5.4.6 Function calls `(` (`src/ExpressionParser/Infix/FunctionExpressionParser.php:36-84`)

```
line = token.line                                            # line of '('
if left is not NameExpression: throw SyntaxError('Function name must be an identifier.', line)
name = left.name
if (alias = parser.getImportedSymbol('function', name)):     # macro imported with "from"
    args = parseCallableArguments(line, parseOpenParenthesis = false, preserveNames = true)
    node = MacroReferenceExpression(alias.node.var, alias.name, args, line); node.hasCallParentheses = true; return node
args = parseNamedArguments(parseOpenParenthesis = false)
function = parser.getFunction(name, line)
if function.parserCallable: node = callable(parser, EmptyNode(line), args, line); set attributes sandboxed_function_name/sandboxed_function; return node
return new function.nodeClass(function, args, line)
```
[verified] `{{ max(1,2)(3) }}`, `{{ "foo"() }}`, `{{ x.y.z(1)(2) }}` -> `Function name must be an identifier.`; `{{ in(1) }}` ->
`Unknown "in" function. ...` (operator-word variables can be called). Imported `from` macros shadow functions. Core parser
callables (`src/Extension/CoreExtension.php:2230-2272`):
- `parent()`: `Calling the "parent" function outside of a block is forbidden.` (empty block stack) and
  `Calling the "parent" function on a template that does not call "extends" or "use" is forbidden.` (checked at the moment
  `parent()` is parsed, so an `extends` that comes later in the source does not count) -> `ParentExpression(blockName)`. [verified]
- `block(name, template = null)` -> `BlockReferenceExpression`; `attribute(variable, attribute, arguments = null)` ->
  `GetAttrExpression(ANY_CALL)`; both bind their arguments at parse time through `CallableArgumentsExtractor`, whose errors
  (e.g. `Value for argument "name" is required for function "block".`) therefore appear at parse time.

#### 5.4.7 Attribute access `.` and null-safe `?.` (`src/ExpressionParser/Infix/DotExpressionParser.php:38-83`)

```
nullSafe = token.value == '?.'
tok = current; lineno = tok.line; args = ArrayExpression([], lineno); type = ANY_CALL
if nextIf(OPERATOR '('): attribute = parseExpression(); expect(PUNCTUATION ')')        # dynamic: a.(expr)
else:
    tok = next()
    if tok is NAME or NUMBER or (OPERATOR and preg_match(REGEX_NAME, value) /* prefix match */):
        attribute = ConstantExpression(tok.value, tok.line)
    else: throw SyntaxError(sprintf('Expected name or number, got value "%s" of type "%s".', tok.value, tok.toEnglish()), tok.line)
isMacroTarget = left is NameExpression and (getImportedSymbol('template', left.name) != null or left.name == '_self')
if test(OPERATOR '('): type = METHOD_CALL; args = parseCallableArguments(tok.line, preserveNames: isMacroTarget)
if isMacroTarget:
    name = attribute is ConstantExpression ? (string) attribute.value : attribute
    node = MacroReferenceExpression(MacroVariable(left.name, left.line), name, args, left.line)
    node.hasCallParentheses = (type == METHOD_CALL); return node
return GetAttrExpression(left, attribute, args, type, lineno, nullSafe)
```
[verified] `{{ x.'y' }}` -> `Expected name or number, got value "y" of type "string" in "index" at line 1.`; `{{ 1. }}` ->
`Expected name or number, got value "" of type "end of print statement" ...`; `{{ x?.[1] }}` -> `... got value "[" of type "operator" ...`;
`{{ x.() }}` -> `Unexpected token "punctuation" of value ")".`; `x.1` and `x.1e3` (float 1000) are numeric attributes; `a.0.1` uses
the float `0.1`. Null-safe short-circuits the rest of the chain at runtime (`n?.y.z` with n null -> `''`).

#### 5.4.8 Subscript and slice `[` (`src/ExpressionParser/Infix/SquareBracketExpressionParser.php:32-70`)

```
lineno = token.line; slice = false
if test(PUNCTUATION ':'): slice = true; attribute = ConstantExpression(0, token.line)
else: attribute = parseExpression()
if nextIf(PUNCTUATION ':'): slice = true
if slice:
    length = test(PUNCTUATION ']') ? ConstantExpression(null, token.line) : parseExpression()
    f = parser.getFilter('slice', token.line)
    node = new f.nodeClass(left, f, Nodes([attribute, length]), token.line)
    expect(PUNCTUATION ']'); return node
expect(PUNCTUATION ']')
return GetAttrExpression(left, attribute, ArrayExpression([], lineno), ARRAY_CALL, lineno)
```
`x[1:2]` = `x|slice(1, 2)`, `x[:2]`, `x[1:]`, `x[:]`; `x[1:2:3]` -> `Unexpected token "punctuation" of value ":" ("punctuation" expected with value "]") ... column 9.`;
`x[]` -> `Unexpected token "punctuation" of value "]".` [verified]

#### 5.4.9 Arrow functions `=>` (`src/ExpressionParser/Infix/ArrowExpressionParser.php:28-32`)

`ArrowFunctionExpression(parseExpression(0), left, token.line)`; the constructor (`src/Node/Expression/ArrowFunctionExpression.php:27-38`)
converts a `ContextVariable` left into a one-element `ListExpression` and otherwise requires a `ListExpression`:
`The arrow function argument must be a list of variables or a single variable.` (`map(1 => 2)`). The body is parsed at
precedence 0 regardless of context; `()` empty parameter lists are a syntax error. [verified]

### 5.5 Call arguments (`src/ExpressionParser/Infix/ArgumentsTrait.php:27-86`)

```
parseNamedArguments(parseOpenParenthesis = true) -> Nodes:
    args = []                                         # ordered map: int keys for positional, string keys for named
    if parseOpenParenthesis: expect(OPERATOR '(', 'A list of arguments must begin with an opening parenthesis')
    hasSpread = false
    while !test(PUNCTUATION ')'):
        if args not empty:
            expect(PUNCTUATION ',', 'Arguments must be separated by a comma')
            if test(PUNCTUATION ')'): break           # trailing comma
        if (t = nextIf(OPERATOR '...')): value = SpreadUnary(parseExpression(), t.line); hasSpread = true
        else:
            value = parseExpression()
            if hasSpread: throw SyntaxError('Normal arguments must be placed before argument unpacking.', current.line)
        name = null
        if value is SetBinary:                         # "name = value" was parsed by the assignment operator
            name = value.left.name; value = value.right
        elif (t = nextIf(OPERATOR '=')) or (t = nextIf(PUNCTUATION ':')):   # the '=' alternative is dead code since 3.23:
                                                                             # parseExpression() already consumed "= x" (or threw)
            if value is not ContextVariable:
                throw SyntaxError(sprintf('A parameter name must be a string, "%s" given.', value::class), t.line)
            name = value.name; value = parseExpression()
        if name == null: args[] = value  else: args[name] = value        # a repeated name overwrites, keeps first position
    expect(PUNCTUATION ')', 'A list of arguments must be closed by a parenthesis')   # cannot fail
    return Nodes(args)

parseCallableArguments(line, parseOpenParenthesis = true, preserveNames = false) -> ArrayExpression:
    for (k, n) in parseNamedArguments(parseOpenParenthesis):
        arguments.addElement(n, (is_int(k) or !preserveNames) ? LocalVariable(k, line) : ConstantExpression(k, line))
```

Notes [verified]:
- `name = value` and `name: value` are equivalent. Because `=` is an operator, `f(a = 1)` is parsed as `SetBinary` and then
  unwrapped; even `f((a = 1), 2)` counts as a NAMED argument (parentheses are ignored) ->
  `Positional arguments cannot be used after named arguments for function "max".` (binding error).
- `f(1 = 2)` -> `Cannot assign to "Twig\Node\Expression\ConstantExpression", only variables can be assigned.` (assignment parser);
  `f("a": 2)` -> `A parameter name must be a string, "Twig\Node\Expression\ConstantExpression" given.`; `f(a.b: 1)` ->
  `... "Twig\Node\Expression\GetAttrExpression" given.`
- `max(...[1, 5], 2)` -> `Normal arguments must be placed before argument unpacking.`; `max(...[1,5], ...[7])` is fine.
- Positional-after-named, duplicates, unknown and missing arguments are checked when the call is bound to the PHP callable
  (compile time, `CallExpression`/`CallableArgumentsExtractor`) - out of scope here; messages seen:
  `Positional arguments cannot be used after named arguments for function "date".`, `Argument "glue" is defined twice for filter "join".`,
  `Unknown argument "sep" for filter "join(glue, and)".`, `Value for argument "value" is required for function "max".`

### 5.6 Errors raised while constructing nodes

| message | class / trigger | source |
|---|---|---|
| `You cannot assign a value to "%s".` | `AssignContextVariable`/`AssignNameExpression` or `TempNameExpression` (`LocalVariable`, `MacroVariable`) with a name whose `strtolower` is `true`/`false`/`none`/`null` (set/for targets, from aliases, import alias, macro arguments, variadic name, destructuring targets, `=`) | `src/Node/Expression/AssignNameExpression.php:30`, `TempNameExpression.php:32` |
| `The "defined" test only works with simple variables.` | tested node does not implement `SupportDefinedTestInterface` (allowed: `NameExpression`/`ContextVariable`, `GetAttrExpression`, `ConstantExpression`, `ArrayExpression`, `FunctionExpression`, `BlockReferenceExpression`, `MacroReferenceExpression`, `MethodCallExpression`); also via `??` | `src/Node/Expression/Test/DefinedTest.php:46` |
| `Regexp "%s" passed to "matches" is not valid: %s.` | constant invalid regex on the right of `matches` | `MatchesBinary.php:35` |
| `The arrow function argument must be a list of variables or a single variable.` | `=>` with other left side | `ArrowFunctionExpression.php:34` |
| `Cannot assign to "%s", only variables can be assigned in sequence destructuring.` | | `SequenceDestructuringSetBinary.php:41` |
| `Cannot assign to "%s", only variables can be assigned in object/mapping destructuring.` | | `ObjectDestructuringSetBinary.php:41` |
| `The argument "varargs" in macro "%s" cannot be defined because the variable "varargs" is reserved for arbitrary arguments.` | macro argument named `varargs` | `src/Node/MacroNode.php:65` |
| `The variadic argument "%s" in macro "%s" cannot have the same name as another argument.` | | `MacroNode.php:68` |
| `Argument "%s" is defined twice for macro "%s".` | | `MacroNode.php:71` |
| `Empty array elements are only allowed in destructuring assignments.` | compile time | `ArrayExpression.php:127` |

The three MacroNode checks run when the macro NODE is built, i.e. after the macro body has been parsed (a syntax error in the
body is reported first). Lines: the defined-test error uses the `TestExpression` line; `MacroNode` errors use the argument's
default-value node line; the destructuring errors use the `=` line. [messages verified]

### 5.7 Precedence-change deprecations (`src/Parser.php:639-676`, `ExpressionParsers::getPrecedenceChanges` `:116-138`)

Only deprecations (no effect on parsing or output). Map construction: for every parser `ep` with a `PrecedenceChange(new)`,
`min = min(new, ep.prec)`, `max = max(...)`; every parser `e` with `min < e.prec < max` gets `ep` appended to `changes[e]`.
Resulting map (dumped): `*`, `/`, `//`, `%` -> [`not`, `??`]; `+`, `-` (binary) -> [`??`, `~`]; `-`, `+` (prefix) -> [`|`];
`not` and every other operator with 5 < prec < 300 -> [`??`].

```
checkPrecedenceDeprecations(ep, expr):
    expressionRefs[expr] = ep
    if ep not in changes or expr.hasExplicitParentheses(): return
    if ep is prefix:                                   # only "not" has its own change
        node = expr.node
        for (key, list) in changes: if ep in list and expressionRefs[node] === key: deprecate(ep, node.line)
    for other in changes[ep]:
        for child in expr.children:
            if expressionRefs[child] === other and !child.hasExplicitParentheses(): deprecate(other, child.line)

deprecate(p, line) = trigger_deprecation(p.change.package, p.change.version,
    'As the "%s" %s operator will change its precedence in the next major version, add explicit parentheses to avoid behavior change in "%s" at line %d.',
    p.name, 'prefix'|'infix', sourceName, line)
```
[verified] `'42' ~ 1 + 41`, `1 + 41 ~ '42'` (`~`), `'notnull' ?? 'foo' ~ '_bar'` (`??`), `not 1 * 2` (`not`), `-1|abs` (`|`, 3.21).

### 5.8 Line numbers given to expression nodes

Constant/variable: its token. Binary/unary/ternary/arrow/assignment: the operator token. `ArrayExpression`: the first token after
`[`/`{`. `ConcatBinary` from interpolation: the right part's line. `GetAttrExpression` from `.`: the token after the dot; from
`[`: the `[`. Filter node: the filter-name token. Function node: the `(` token. Test node: the token after the test
(+ arguments); `is not` wrapper: the `is not` token. Macro reference via `.`: the macro-variable token; via `from` import: `(`.
`PrintNode`: the `{{` token. Tag nodes: the tag-name token. `TextNode`: start of the text. These lines feed runtime error
messages (e.g. strict-variables `Variable "x" does not exist`).

### 5.9 Compile semantics that leak from the AST (must be replicated by an interpreter)

- `NegUnary`/`PosUnary`/`NotUnary` compile to ` -expr` without parentheses unless the group was explicit, and binaries compile to
  `(left op right)`. With `**` (PHP: `**` binds tighter than unary minus) this means `{{ -1**0 }}` = `-1` and `{{ -2 ** 2 }}` = `-4`
  although the AST is `(-1) ** 0`; `{{ (-1)**0 }}` = 1. [verified] (`src/Node/Expression/Unary/AbstractUnary.php:33-45`)
- `{{ 1..3|join(',') }}` is `1..(3|join(','))` (filters bind tighter) -> range(1, '3') printed as `Array` with a PHP warning. [verified]
- `if`, `?:`, `? :` wrap conditions in `TrueTest` (PHP truthiness).

---

## 6. Tags

General rules: a tag node gets `setNodeTag(tag)`; the token passed to `parse()` is the tag NAME token; after the tag-specific
syntax every tag ends with `expect(BLOCK_END)`, whose failure yields
`Unexpected token "<english>"[ of value "<v>"] ("end of statement block" expected) in "<tpl>" at line L column C.`
End tags are matched with `$token->test('endxxx')` (NAME type, loose equality) and, for body tags, the needle is dropped
(`dropNeedle = true`) unless stated. An end tag followed by anything but `%}` (other than the optional name for `endblock` and
`endmacro`) fails with that expect error: `{% endif x %}` -> `Unexpected token "name" of value "x" ("end of statement block" expected) ... column 20.`
Unclosed body tags give either `Unexpected end of template.` (when EOF is hit where the token parser calls `next()`: anywhere
inside `if`/`elseif`/`else` bodies of `if`, and in the main body of `for` and `guard`) or
`Unexpected token "end of template" ("end of statement block" expected) ... column C` (when it calls `expect(BLOCK_END)`: all other
body tags, and the `else` body of `for` and `guard`, which are parsed with a dropped needle). [verified: `{% for x in y %}`,
`{% if x %}{% else %}`, `{% if x %}{% elseif y %}`, `{% guard function nope %}` -> `Unexpected end of template in "index" at line 1.`;
`{% for x in y %}{% else %}` -> `... column 27.`, `{% guard function max %}{% else %}` -> `... column 35.`, `{% apply upper %}x`,
`{% block a %}x`, `{% macro m() %}x`, `{% set a %}x`, `{% with {a:1} %}`, `{% embed 'b' %}...`, `{% autoescape %}x`,
`{% sandbox %}x` -> `Unexpected token "end of template" ("end of statement block" expected) ...`]

Default tag set (`src/Extension/CoreExtension.php:215-237` + `EscaperExtension` + optional `SandboxExtension`):
`apply, for, if, extends, include, block, use, macro, import, from, set, types, flush, do, embed, with, deprecated, guard, autoescape, [sandbox]`;
lexer-level pseudo tags: `verbatim`/`endverbatim` (2.6) and `line N` (2.6).

### 6.1 `apply` (`src/TokenParser/ApplyTokenParser.php:33-55`)

Syntax: `{% apply filter[(args)] (| filter[(args)])* %} body {% endapply %}`

```
ref = LocalVariable(null, lineno); filter = ref
loop: filter = FilterExpressionParser.parse(parser, filter, current)    # parses NAME [args], not a full expression
      if !test(OPERATOR '|'): break
      next()
expect(BLOCK_END); body = subparse(decideApplyEnd 'endapply', dropNeedle); expect(BLOCK_END)
return Nodes([SetNode(true, ref, body, lineno), PrintNode(filter, lineno)], lineno)
```
Errors [verified]: `{% apply %}` -> `Unexpected token "end of statement block" ("name" expected) ... column 10.`;
`{% apply upper x %}` -> `Unexpected token "name" of value "x" ("end of statement block" expected) ... column 16.` (no other
operators allowed after the filters); unclosed -> `Unexpected token "end of template" ("end of statement block" expected) ...`.
`{% apply lower|escape('html') %}<B>{% endapply %}` -> `&lt;b&gt;`.

### 6.2 `autoescape` (`src/TokenParser/AutoEscapeTokenParser.php:27-47`, from `EscaperExtension`)

Syntax: `{% autoescape [constant] %} body {% endautoescape %}`

```
if test(BLOCK_END): value = 'html'
else: expr = parseExpression()
      if expr is not ConstantExpression: throw SyntaxError('An escaping strategy must be a string or false.', current.line)
      value = expr.value
expect(BLOCK_END); body = subparse('endautoescape', drop); expect(BLOCK_END)
return AutoEscapeNode(value, body, lineno)
```
Any constant is accepted at parse time (strings, numbers, true/false/null); sequences, concatenations and variables are rejected.
Runtime/compile consequences [verified]: `false` disables escaping; `'html'`, `'js'`, ... select a strategy; `true` and `1` both
become strategy `"1"` -> `Invalid escaping strategy "1" (valid ones: "html", "js", "url", "css", "html_attr", "html_attr_relaxed") in "index" at line 1.`
(RuntimeError); `'foo'` -> same message with `"foo"`; `null` -> uncaught PHP
`TypeError: Twig\NodeVisitor\EscaperNodeVisitor::needEscaping(): Return value must be of type string|bool, null returned`.

### 6.3 `block` (`src/TokenParser/BlockTokenParser.php:36-67`)

Syntax: `{% block NAME %} body {% endblock [NAME] %}` or shorthand `{% block NAME expr %}`

```
name = expect(NAME).value
parser.setBlock(name, block = BlockNode(name, EmptyNode(), lineno))     # duplicate check here (4.4)
parser.setDocumentationTarget(block); pushLocalScope(); pushBlockStack(name)
if nextIf(BLOCK_END):
    body = subparse('endblock', drop)
    if (t = nextIf(NAME)) and t.value != name:
        throw SyntaxError(sprintf('Expected endblock for block "%s" (but "%s" given).', name, t.value), current.line)
else:
    body = Nodes([PrintNode(parseExpression(), lineno)])
expect(BLOCK_END)
block.setNode('body', body); popBlockStack(); popLocalScope()
return BlockReferenceNode(name, lineno)
```
[verified] `{% block c 'z' %}` renders `z`; `{% block a 'x' %}{% endblock %}` -> `Unknown "endblock" tag.` (shorthand has no end tag);
`{% block a-b %}` is block `a` with shorthand `-b` (renders `0`); `{% block a.b %}` -> `Unexpected token "operator" of value ".".`;
`{% block 1 %}` -> `Unexpected token "number" of value "1" ("name" expected) ... column 10.`; `{% block and %}` ->
`Unexpected token "operator" of value "and" ("name" expected) ...` but `{% block in%}` works (no space before `%}`);
duplicates -> `The block 'a' has already been defined line 1 in "index" at line 2.`; mismatched end name ->
`Expected endblock for block "a" (but "b" given) in "index" at line 1.`; unclosed (`{% block a %}\n\nabc`) ->
`Unexpected token "end of template" ("end of statement block" expected) in "index" at line 3 column 4.`
Docs: block names are "alphanumeric characters, and underscores" - the lexer actually accepts any NAME token.

### 6.4 `deprecated` (`src/TokenParser/DeprecatedTokenParser.php:33-59`)

Syntax: `{% deprecated expr [package = expr] [version = expr] %}` (options in any order, repeatable; NAME keys)

```
node = DeprecatedNode(parseExpression(), token.line)
while test(NAME):
    k = current.value; next(); expect(OPERATOR '=')
    if k == 'package': node.setNode('package', parseExpression())
    elif k == 'version': node.setNode('version', parseExpression())
    else: throw SyntaxError(sprintf('Unknown "%s" option.', k), current.line)
expect(BLOCK_END); return node
```
[verified] `{% deprecated 'x' foo='a' %}` -> `Unknown "foo" option.`; `{% deprecated 'x' package 'a' %}` ->
`Unexpected token "string" of value "a" ("operator" expected with value "=") ... column 27.`; `{% deprecated %}` ->
`Unexpected token "end of statement block" of value "".`. Runtime deprecation text: `x in "index" at line 1.` /
`Since a/b 1.0: x in "index" at line 1.`

### 6.5 `do` (`src/TokenParser/DoTokenParser.php:25-32`)

`{% do expr %}` -> `DoNode(parseExpression(), token.line)`. Primary use in 3.23+: assignments/destructuring
(`{% do a = b = 'foo' %}`, `{% do [x, y] = [1, 2] %}`, `{% do {a, b: c} = map %}`). [verified]

### 6.6 `embed` (`src/TokenParser/EmbedTokenParser.php:25-48`)

Syntax: `{% embed expr [ignore missing] [with expr] [only] %} ... {% endembed %}`

```
parent = parseExpression()
[variables, only, ignoreMissing] = IncludeTokenParser::parseArguments()      # consumes BLOCK_END (6.11)
stream.injectTokens([BLOCK_START, NAME 'extends', STRING '__parent__', BLOCK_END])   # all with token.line, no offsets
module = parser.parseEmbeddedTemplate(stream, parent, decideBlockEnd 'endembed')   # Parser::parse(stream, test, dropNeedle=true)
parser.embedTemplate(module)                                                 # index = ++lastEmbedIndex
expect(BLOCK_END)
return EmbedNode(module.templateName, module.index, variables, only, ignoreMissing, token.line)
```
The embedded body is a full child template (own blocks, macros, imports; parent replaced by `parent` after parsing, so the
body cleanup and all `CorrectnessNodeVisitor` rules apply, run inside the nested `parse()` call). [verified] content outside blocks
-> `A template that extends another one cannot include content outside Twig blocks. ... line 2?`; an explicit
`{% extends %}` inside -> `Multiple extends tags are forbidden.`; `{% embed 'b' %}{% endif %}{% endembed %}` ->
`Unexpected "endif" tag (expecting closing tag for the "embed" tag defined near line 1) ...`; nested embeds work; `parent()` works
inside overridden blocks; imports of the outer template are not visible inside.

### 6.7 `extends` (`src/TokenParser/ExtendsTokenParser.php:28-35`)

`{% extends expr %}` -> `parser.setParent(parseExpression(), false)`; returns `ConfigNode(token.line)` (tag `extends`).
Rules (4.5): must not be in a block/macro, single per template, root-level only (deprecation otherwise), content outside blocks
forbidden, blocks must not be nested under tags at the root. [verified] `{% extends %}` -> `Unexpected token "end of statement block" of value "".`;
`{% extends 'b' 'c' %}` -> `Unexpected token "string" of value "c" ("end of statement block" expected) ... column 16.`

### 6.8 `flush` (`src/TokenParser/FlushTokenParser.php:27-32`)

`{% flush %}` -> `FlushNode(token.line)`. `{% flush x %}` -> `Unexpected token "name" of value "x" ("end of statement block" expected) ... column 10.` [verified]

### 6.9 `for` (`src/TokenParser/ForTokenParser.php:34-62`)

Syntax: `{% for target[, target ...] in expr %} body [{% else %} body] {% endfor %}`

```
targets = parseAssignmentExpression()          # AbstractTokenParser, see below
expect(OPERATOR 'in'); seq = parseExpression(); expect(BLOCK_END)
body = subparse(decideForFork ['else','endfor'])           # needle NOT dropped
if next().value == 'else':                                  # next() at EOF -> "Unexpected end of template."
    elseLine = current.line; expect(BLOCK_END)
    else = ForElseNode(subparse('endfor', drop), elseLine)
else: else = null
expect(BLOCK_END)
if count(targets) > 1: key = targets[0]; value = targets[1]          # a 3rd and later target are silently ignored
else: key = AssignContextVariable('_key', lineno); value = targets[0]
return ForNode(key, value, seq, null, body, else, lineno)
```

`parseAssignmentExpression()` (`src/TokenParser/AbstractTokenParser.php:46-68`):
```
loop:
    t = current
    if test(OPERATOR) and preg_match(REGEX_NAME, t.value):  next()    # operator words allowed; prefix match, so 'not in' or 'b-and' pass whole
    else: expect(NAME, null, 'Only variables can be assigned to')
    targets.push(AssignContextVariable(t.value, t.line))              # may throw "You cannot assign a value to"
    if !nextIf(PUNCTUATION ','): break
```
[verified] key/value, `else`, `loop.*`; `{% for x %}` -> `Unexpected token "end of statement block" ("operator" expected with value "in") ... column 10.`;
`{% for a, b, c in {x: 1} %}` uses a, b; `{% for 1 in x %}` -> `Only variables can be assigned to. Unexpected token "number" of value "1" ("name" expected) ... column 8.`;
the removed 2.x `if` clause: `{% for x in y if x %}` -> `Unexpected token "name" of value "if" ("end of statement block" expected) ... column 15.`;
unclosed main body -> `Unexpected end of template.`, unclosed `else` body -> `Unexpected token "end of template" ("end of statement block" expected) ...`;
double else -> `Unexpected "else" tag (expecting closing tag for the "for" tag defined near line 1) ...`;
`{% for true in x %}` -> `You cannot assign a value to "true".`; `{% for in in [1,2] %}{{ in }}` works; `{% set b-and = 1 %}` creates a
variable literally named `b-and`.

### 6.10 `from` (`src/TokenParser/FromTokenParser.php:30-68`)

Syntax: `{% from expr import NAME [as NAME] (, NAME [as NAME])* %}`

```
macro = parseExpression(); expect(NAME 'import')
targets = {}                                     # keyed by MACRO name: "import m as a, m as b" keeps only b
loop:
    name = expect(NAME).value
    alias = nextIf('as') ? AssignContextVariable(expect(NAME).value, token.line) : AssignContextVariable(name, token.line)
    targets[name] = alias
    if !nextIf(PUNCTUATION ','): break           # a trailing comma is an error (expects NAME)
expect(BLOCK_END)
ref = AssignMacroVariable(MacroVariable(null, token.line), parser.isMainScope())
node = ImportNode(macro, ref, token.line)
for (name, alias) in targets: parser.addImportedSymbol('function', alias.name, name, ref)
return node
```
[verified] `{% from _self %}` -> `Unexpected token "end of statement block" ("name" expected with value "import") ... column 15.`;
`import %}`, `as %}`, `m, %}` -> `Unexpected token "end of statement block" ("name" expected) ...`; `import and` ->
`Unexpected token "operator" of value "and" ("name" expected) ...`; `as true` -> `You cannot assign a value to "true".`;
`from _self import m as n, m as o` registers only `o` (`n()` -> `Unknown "n" function.`).

### 6.11 `include` (`src/TokenParser/IncludeTokenParser.php:31-67`)

Syntax (fixed order): `{% include expr [ignore missing] [with expr] [only] %}`

```
expr = parseExpression()
parseArguments():
    ignoreMissing = false; if nextIf(NAME 'ignore'): expect(NAME 'missing'); ignoreMissing = true
    variables = nextIf(NAME 'with') ? parseExpression() : null
    only = nextIf(NAME 'only') != null
    expect(BLOCK_END)
return IncludeNode(expr, variables, only, ignoreMissing, token.line)
```
[verified] wrong order `with {} ignore missing` -> `Unexpected token "name" of value "ignore" ("end of statement block" expected) ... column 24.`;
`only with {}` -> `... of value "with" ...`; `ignore %}` -> `Unexpected token "end of statement block" ("name" expected with value "missing") ... column 23.`;
`{% include ['nob', 'b'] %}` picks the first existing. Note `with only` treats `only` as the variables expression.

### 6.12 `if` (`src/TokenParser/IfTokenParser.php:36-74`)

```
expr = parseExpression(); expect(BLOCK_END)
body = subparse(decideIfFork ['elseif','else','endif']); tests = [expr, body]; else = null
loop:
    switch next().value:                          # next() at EOF -> "Unexpected end of template."
      'else':   expect(BLOCK_END); else = subparse(decideIfEnd ['endif'])          # needle not dropped
      'elseif': expr = parseExpression(); expect(BLOCK_END); body = subparse(decideIfFork); tests += [expr, body]
      'endif':  end
      default:  throw SyntaxError(sprintf('Unexpected end of template. Twig was looking for the following tags "else", "elseif", or "endif" to close the "if" block started at line %d).', lineno), current.line)   # unreachable
expect(BLOCK_END)
return IfNode(Nodes(tests), else, lineno)          # IfNode wraps each condition with TrueTest::wrap()
```
[verified] `{% if %}` / `{% elseif %}` -> `Unexpected token "end of statement block" of value "".`; `elseif` after `else` ->
`Unexpected "elseif" tag (expecting closing tag for the "if" tag defined near line 1) ...`; unclosed -> `Unexpected end of template.`;
nested unclosed `{% if a %}{% if b %}{% endif %}` -> `Unexpected end of template in "index" at line 1.`

### 6.13 `import` (`src/TokenParser/ImportTokenParser.php:29-39`)

`{% import expr as NAME %}`:
```
macro = parseExpression(); expect(NAME 'as'); name = expect(NAME).value
var = AssignMacroVariable(MacroVariable(name, token.line), parser.isMainScope())     # MacroVariable -> "You cannot assign" check
expect(BLOCK_END); parser.addImportedSymbol('template', name)
return ImportNode(macro, var, token.line)
```
After import, `name.macro(...)` and `name.(expr)(...)` (3.28) become `MacroReferenceExpression`; without parentheses a 3.29
deprecation. `_self.macro()` works without import. [verified] `{% import _self %}` ->
`Unexpected token "end of statement block" ("name" expected with value "as") ... column 17.`; `as m.n` ->
`Unexpected token "operator" of value "." ("end of statement block" expected) ... column 21.`; `as in` -> operator error; `as true`
-> `You cannot assign a value to "true".`

### 6.14 `macro` (`src/TokenParser/MacroTokenParser.php:39-146`)

Syntax: `{% macro NAME( [arg [= constant] (, arg [= constant])*] [, ...variadic] [,] ) %} body {% endmacro [NAME] %}`

```
name = expect(NAME).value
[arguments, variadicName] = parseDefinition(name)
expect(BLOCK_END); pushLocalScope()
body = subparse('endmacro', drop)
if (t = nextIf(NAME)) and t.value != name:
    throw SyntaxError(sprintf('Expected endmacro for macro "%s" (but "%s" given).', name, t.value), current.line)
popLocalScope(); expect(BLOCK_END)
parser.setMacro(name, macro = MacroNode(name, BodyNode([body]), arguments, lineno, variadicName))   # MacroNode checks (5.6)
parser.setDocumentationTarget(macro)
return MacroDeclarationNode(name, lineno)                          # a ConfigNode with attribute name

parseDefinition(macroName):
    arguments = ArrayExpression([], current.line); variadicName = null
    expect(OPERATOR '(', 'A list of arguments must begin with an opening parenthesis')
    while !test(PUNCTUATION ')'):
        if count(arguments):
            expect(PUNCTUATION ',', 'Arguments must be separated by a comma')
            if test(PUNCTUATION ')'): break
        if nextIf(OPERATOR '...'):
            t = expect(NAME, null, 'A variadic argument must be a name')
            variadicName = LocalVariable(t.value, t.line).name with the reserved prefix "\u{035C}" stripped
            if test(OPERATOR '='): throw SyntaxError(sprintf('The variadic argument "%s" in macro "%s" cannot have a default value.', variadicName, macroName), t.line)
            if nextIf(PUNCTUATION ',') and !test(PUNCTUATION ')'): throw SyntaxError(sprintf('The variadic argument "%s" in macro "%s" must be the last one.', variadicName, macroName), t.line)
            break
        t = expect(NAME, null, 'An argument must be a name')
        argName = LocalVariable(t.value, t.line)                    # "You cannot assign a value to" for true/false/none/null; reserved names get the prefix
        if (t = nextIf(OPERATOR '=')): default = parseExpression()
        else: default = ConstantExpression(null, current.line); default.is_implicit = true
        if !checkConstant(default): throw SyntaxError('A default value for an argument must be a constant (a boolean, a string, a number, a sequence, or a mapping).', t.line)
        arguments.addElement(default, argName)
    expect(PUNCTUATION ')', 'A list of arguments must be closed by a parenthesis')       # cannot fail
    return [arguments, variadicName]

checkConstant(n) = n is ConstantExpression|ArrayExpression|NegUnary|PosUnary and all children checkConstant
```
Reserved names (`TempNameExpression::RESERVED_NAMES = ['varargs', 'context', 'macros', 'blocks', 'this']`) are internally
prefixed with U+035C so they work as argument names, except `varargs` (MacroNode error). [verified] all variadic errors,
`{% macro m(a: 1) %}` -> `Arguments must be separated by a comma. Unexpected token "punctuation" of value ":" ("punctuation" expected with value ",") ... column 13.`;
`{% macro m %}` -> `A list of arguments must begin with an opening parenthesis. Unexpected token "end of statement block" ("operator" expected with value "(") ... column 12.`;
`{% macro m("a") %}` -> `An argument must be a name. Unexpected token "string" of value "a" ("name" expected) ... column 12.`;
`{% macro m(...) %}` -> `A variadic argument must be a name. Unexpected token "punctuation" of value ")" ("name" expected) ... column 15.`;
defaults `-1`, `+2`, `[1, {c: 'd'}]`, `null`, `'x'` accepted; `"x#{y}"`, `1 + 1`, `b` rejected; `{% macro and() %}` -> operator
error; macro inside a block -> 3.28 root deprecation; redefinition -> 3.29 deprecation. Runtime (3.29): calling with too many/unknown
named/missing arguments only deprecates.

### 6.15 `sandbox` (`src/TokenParser/SandboxTokenParser.php:34-61`, only with `SandboxExtension`; deprecated 3.15)

```
deprecation 'The "sandbox" tag is deprecated in "%s" at line %d.' (3.15)
expect(BLOCK_END); body = subparse('endsandbox', drop); expect(BLOCK_END)
if body is IncludeNode: body.sandboxed = true
else for node in body (children of body!):
    if node is TextNode and ctype_space(node.data): continue
    if node is not IncludeNode: throw SyntaxError('Only "include" tags are allowed within a "sandbox" section.', node.line)
    node.sandboxed = true
return SandboxNode(body, token.line)
```
Quirk [verified]: a body consisting of a SINGLE node is iterated over that node's children, so `{% sandbox %}x{% endsandbox %}`
passes (a TextNode has no children) while `{% sandbox %}{{ x }}{% endsandbox %}` fails (the PrintNode's child expression is not an
include). Error line = line of the offending node. Without the extension: `Unknown "sandbox" tag.`

### 6.16 `set` (`src/TokenParser/SetTokenParser.php:34-86`)

Syntax: `{% set t1[, t2 ...] = e1[, e2 ...] %}` or `{% set t %} body {% endset %}`

```
names = parseAssignmentExpression()
if nextIf(OPERATOR '='):
    values = parseMultitargetExpression()        # parseExpression() (, parseExpression())*  -> Nodes
    expect(BLOCK_END)
    if count(names) != count(values): throw SyntaxError('When using set, you must have the same number of variables and assignments.', current.line)
    capture = false
else:
    capture = true
    if count(names) > 1: throw SyntaxError('When using set with a block, you cannot have a multi-target.', current.line)
    expect(BLOCK_END); values = subparse('endset', drop); expect(BLOCK_END)
return SetNode(capture, names, values, lineno)
```
`SetNode` constructor (`src/Node/SetNode.php:26-45`) for captures: empty body -> `ConstantExpression('')` (capture off, safe);
single `TextNode` -> `ConstantExpression(text)` (safe); single `PrintNode` of a constant -> that constant (safe); otherwise
`CaptureNode(body)`; captured values are `Markup` (not re-escaped). Note the count check happens AFTER `expect(BLOCK_END)`, so
its line is that of the token after `%}`. [verified] `{% set a.b = 1 %}` -> `Unexpected token "operator" of value "." ("end of statement block" expected) ... column 9.`;
`{% set %}` -> `Only variables can be assigned to. Unexpected token "end of statement block" ("name" expected) ... column 8.`;
`{% set a = 1 b = 2 %}` -> `Unexpected token "name" of value "b" ... column 14.`; `{% set d = e = 3 %}` assigns both (the `=`
operator); `{% set in = 1 %}` works; `{% set none = 1 %}` -> `You cannot assign a value to "none".`; capture of ` <b> ` under
html autoescape prints ` <b> ` unescaped.

### 6.17 `types` (`src/TokenParser/TypesTokenParser.php:33-94`, experimental, 3.13)

Syntax: `{% types [{] NAME[?]: 'string' (, NAME[?]: 'string')* [,] [}] %}`

```
enclosed = nextIf(PUNCTUATION '{') != null; first = true
while !(test(PUNCTUATION '}') or test(BLOCK_END)):
    if !first:
        expect(PUNCTUATION ',', 'A type string must be followed by a comma')
        if test(PUNCTUATION '}') or test(BLOCK_END): break
    first = false
    nameToken = expect(NAME)
    if nextIf(OPERATOR '?:'): optional = true                       # "a?:" lexes as the ?: operator
    else: optional = nextIf(OPERATOR '?') != null; expect(PUNCTUATION ':', 'A type name must be followed by a colon (:)')
    valueToken = expect(STRING)
    types[name] = {type, optional}; nodes[name] = TypeNode(name, type, optional, nameToken.line)
if enclosed: expect(PUNCTUATION '}', 'An opened mapping is not properly closed')     # practically unreachable (lexer bracket check)
expect(BLOCK_END)
return TypesNode(types, line) with one child TypeNode per name
```
No effect on output. [verified] `{% types %}` and `{% types {} %}` are valid; `a ? : 'x'` -> `A type name must be followed by a colon (:). Unexpected token "operator" of value "? :" ...`;
`a: b` -> `Unexpected token "name" of value "b" ("string" expected) ...`; `'a': 'x'` -> `... ("name" expected) ...`;
`a: 'x' b: 'y'` -> `A type string must be followed by a comma. ...`; `{a: 'x' %}` -> lexer keeps `{` open, so
`A type string must be followed by a comma. Unexpected token "operator" of value "%" ...`.

### 6.18 `use` (`src/TokenParser/UseTokenParser.php:37-69`)

Syntax: `{% use 'template' [with NAME [as NAME] (, NAME [as NAME])*] %}`

```
template = parseExpression()
if template is not ConstantExpression: throw SyntaxError('The template references in a "use" statement must be a string.', current.line)
targets = {}
if nextIf('with'): loop: name = expect(NAME).value; alias = nextIf('as') ? expect(NAME).value : name
                         targets[name] = ConstantExpression(alias, -1); if !nextIf(PUNCTUATION ','): break
expect(BLOCK_END)
parser.addTrait(Nodes(['template' => template, 'targets' => Nodes(targets)]))
return ConfigNode(token.line)
```
Any constant passes the parse check; `use x`, `use 'b' ~ 'c'` fail. Root-level only (deprecation). `parent()` in blocks is
allowed in templates that only `use`. [verified] A numeric constant (`{% use 1 %}`) passes the parser but produces invalid
compiled PHP: PHP raises an uncaught `ParseError: syntax error, unexpected floating-point number ".1", expecting ")"` [verified] -
a PHP artefact a Rust port cannot reproduce literally; choose a deterministic behaviour and document it.

### 6.19 `with` (`src/TokenParser/WithTokenParser.php:27-45`)

Syntax: `{% with [expr [only]] %} body {% endwith %}`
```
variables = null; only = false
if !test(BLOCK_END): variables = parseExpression(); only = nextIf(NAME 'only') != null
expect(BLOCK_END); body = subparse('endwith', drop); expect(BLOCK_END)
return WithNode(body, variables, only, token.line)
```
[verified] scoping works; non-mapping -> runtime `Variables passed to the "with" tag must be a mapping.`; `{% with only %}` treats
`only` as the variables expression (runtime error if undefined).

### 6.20 `guard` (`src/TokenParser/GuardTokenParser.php:25-67`, 3.15)

Syntax: `{% guard function|filter|test NAME [NAME] %} body [{% else %} body] {% endguard %}`
```
typeToken = expect(NAME)
if typeToken.value not in ['function','filter','test']:
    throw SyntaxError(sprintf('Supported guard types are function, filter and test, "%s" given.', typeToken.value), typeToken.line)
name = expect(NAME).value
if type == 'test' and test(NAME): name += ' ' + current.value; next()      # always consumes a 2nd word
exists = env.get<Type>(name) != null   (SyntaxError during lookup -> false)
expect(BLOCK_END)
if exists: body = subparse(decideGuardFork ['else','endguard'])
else: body = EmptyNode(); subparseIgnoreUnknownTwigCallables(decideGuardFork)     # parsed, then discarded
else = EmptyNode()
if next().value === 'else':                      # next() at EOF -> "Unexpected end of template."
    expect(BLOCK_END)
    if exists: subparseIgnoreUnknownTwigCallables('endguard', drop)   # parsed, then discarded
    else: else = subparse('endguard', drop)
expect(BLOCK_END)
return Nodes([exists ? body : else], token.line)
```
[verified] unknown callables inside the discarded branch do not error, unknown tags do; blocks defined in the discarded branch
vanish; `{% guard test divisible %}` is false, `{% guard test divisible by %}` true.

### 6.21 `verbatim` and `line`

Lexer level, see 2.6. `{% verbatim %}...{% endverbatim %}` yields one TEXT token (no node of its own).

---

## 7. Deprecations relevant to syntax

All are emitted via `trigger_deprecation()` (`E_USER_DEPRECATED`, message prefixed `Since <package> <version>: `); none change
output. Listed with trigger: string escapes of non-special chars (3.12, 2.9); precedence changes `not`/`??`/`~` (3.15) and `|` (3.21)
(5.7); macro redefinition (3.29); macro call without parentheses (3.29); parentheses in `macro() is defined` (3.29);
`extends`/`use`/`macro` not at the template root (3.28); `sandbox` tag (3.15); deprecated callables (per callable); returning
null from a token parser (3.12); `Token::ARROW_TYPE`/`SPREAD_TYPE` (3.21); punctuation-typed `( [ | . ? ?:` tests (3.21);
`ExpressionParser` class and its methods (3.21); `getOperators()` extension API (3.21); precedence outside [0,512] (3.21); missing
`getOperatorTokens()` (3.24). Runtime macro deprecations (3.29): too many arguments, unknown named argument, missing argument
without default, case-insensitive macro name, `macro_` prefix. (`doc/deprecated.rst`.)

---

## 8. Gap analysis against the original twig-rs code

Original code: `ext/twig-rs-orig/src/tokens/**` (lexer) and `ext/twig-rs-orig/src/nodes/**` (parser), Twig 1.x era, Rust 2015,
`regex = 0.1.80` (no lookaround support).

### 8.1 Lexer (`src/tokens/lexer/{matchers,iter,options}.rs`, `src/tokens/{mod,token}.rs`)

| # | original | Twig 3.31.1 | action |
|---|---|---|---|
| L1 | `LexerOptions` has `tag_comment`, `tag_block`, `tag_variable`, `whitespace_trim`, `interpolation` (`options.rs`) | adds `whitespace_line_trim` (`~`) and `whitespace_line_chars` | add `~` everywhere (openers, `lex_var`, `lex_block`, `lex_comment`, `lex_raw_data`, `lex_block_raw`, rtrim set `" \t\0\x0B"`) |
| L2 | `lex_tokens_start` `({{\|{%\|{#)(-)?` | `({{\|{%\|{##(?!})\|{#)(-\|~)?` | add documentation-comment opener and `~` |
| L3 | `lex_block` `\A\s*(?:-%}\s*\|\s*%})\n?` - eats only `\n` | `%}(?:\r\n?\|\n)?`; `-%}\s*\n?`; `~%}[ \t\0\x0B]*` | eat `\r\n`/`\r`; do not eat a newline after `~%}` |
| L4 | `lex_comment` `(?:-#}\s*\|#})\n?` | `-#}\s*\n?` / `~#}[ \t\0\x0B]*` / `#}(?:\r\n?\|\n)?` + separate documentation-comment closer `##}` | |
| L5 | raw blocks: `(raw\|verbatim)` with `lex_raw_data`/`lex_verbatim_data` | only `verbatim`; `{% raw %}` is `Unknown "raw" tag.` | remove `raw`; implement `~` trims |
| L6 | operator regex appends `[\s()]` (consumes the following char!) and `trim_right()`s the value; no lookbehind | lookahead `(?=[\s()\[{])` (not consumed), lookbehind `(?<![\.\\|]\s\|.[\.\\|])` for operators starting with a letter | rewrite without consuming; add `[`/`{` and the lookbehind; the original swallows `(` after `not(`, never registers that bracket, and lexes `foo.and`/`x\|in` wrongly |
| L7 | operator set: 1.x list + `"="` hard-coded (`matchers.rs:207`) | 3.31.1 list (2.4) incl. `??`, `?:`, `? :`, `?.`, `...`, `=>`, `===`, `!==`, `<=>`, `xor`, `has some`, `has every`, `(`, `[`, `.`, `?`, `\|`; registry driven | build from the expression-parser registry, same ordering rule |
| L8 | operator value = raw slice trimmed right | whitespace runs collapsed to one space (`not   in` -> `not in`) | normalize |
| L9 | `(`, `[`, `.`, `?`, `\|` are `Punctuation(char)` | they are OPERATOR tokens; only `) ] { } : ,` are punctuation | change token kinds; brackets for `(`/`[` checked in the operator branch |
| L10 | `TokenValue::Value(Const)` merges numbers and strings; extra `CommentStart`; no EOF token (iterator ends) | separate NUMBER and STRING types; EOF token with line/offset; no comment token | match Twig types and English names (2.2) |
| L11 | number regex `[0-9]+(?:\.[0-9]+)?`; big ints kept as `Big(String)` | underscores, exponent; value via PHP numeric conversion (int, or float on overflow/`.`/`e`) | implement 2.11; drop `Big` |
| L12 | string tokens are raw slices (no escape processing, no newline normalization) | custom `stripcslashes` (2.9) after CRLF normalization; deprecations | implement |
| L13 | name regex is evaluated on `char`s, so `\x7F-\xFF` means U+007F..U+00FF only | byte based: DEL (0x7F) and every byte >= 0x80, i.e. every non-ASCII UTF-8 sequence, is a name char | operate on bytes (equivalently: U+007F and every char >= U+0080 are name chars) |
| L14 | no inline `#` comments or `##` doc lines in expressions | 3.15/3.29 features | add (`#[^\r\n]*`) |
| L15 | TEXT tokens not normalized; only `\n` counted as newline | `\r\n`/`\r` normalized to `\n` in TEXT/strings/verbatim; `\r` counted | implement |
| L16 | no byte offsets, no columns | offsets on tokens; columns in 3 error kinds | add offsets |
| L17 | `{% line %}` parse panics on overflow (`.expect`) | `(int)` saturates to `PHP_INT_MAX` | saturate |
| L18 | `Unclosed` check at end only via `cursor == end` + bracket pop; DQ-string closer pops any bracket | Twig error order and messages (2.17); `Unclosed "variable"` also in interpolation state | match exactly |
| L19 | error texts (e.g. `Unexpected character "x"`, `Unclosed comment`) lack the final `.` and location formatting | raw messages end with `.`; formatting per 1.2 | port the message table verbatim |
| L20 | `match_regex_dq_string_part` hand-written (works) | same semantics as `REGEX_DQ_STRING_PART` | keep, add escape processing per part |

### 8.2 Parser (`src/nodes/parser/{mod,expr,body,module}.rs`, `src/nodes/node/*.rs`, `src/extension/core/**`)

| # | original | Twig 3.31.1 | action |
|---|---|---|---|
| P1 | no end-of-stream token: `current()`/`expect()` return `UnexpectedEndOfTemplate.at(1)` | EOF token; `Unexpected token "end of template" (...)` or `Unexpected end of template.` with the EOF line | model EOF |
| P2 | `expect` messages: `Expected "..." but received ... with value {:?}` (Rust Debug quoting) | exact `TokenStream::expect` format (section 3), optional prefix message + `. `, PHP truthiness for the value part, columns | reimplement |
| P3 | expression parser: hard-coded `get_primary` (unary + `(`), function call only when a NAME is directly followed by `(`; postfix `.`/`[`/`\|` mostly `unimplemented!()`; ternary handled only when `min_precedence == 0` (1.x style) | registry of prefix/infix parsers (5.2); `(`, `.`, `?.`, `[`, `\|`, `?`, `=`, `=>`, `is`, `is not` are infix operators with precedences; `(max)(1,2)` is a call | rewrite as precedence climbing over a registry |
| P4 | operators missing: `??`, `?:` as binary, `? :`, `xor`, `<=>`, `===`, `!==`, `has some`, `has every`, `=`, `=>`, `...`, `?.`; `is`/`is not` registered as plain binaries | 5.2 | add |
| P5 | no tests: `is` parsed as binary operator | `getTest()` two-word rule, `one_mandatory_argument` parsed at precedence 100, `defined` special cases, `is not` = `NotUnary(test)` | implement 5.4.4 |
| P6 | no arrow functions, no grouping-as-parameter-list, no assignment/destructuring | 5.3.5, 5.4.3, 5.4.9 | implement |
| P7 | sequences/mappings: no spread, no empty slots, no `{a}` shorthand, error texts "array"/"hash" | `[...]`/`{...}` rules 5.3.2/5.3.3, texts "sequence"/"mapping" | implement and fix texts |
| P8 | arguments: `=` only, no `:`, no spread, no trailing comma handling in args; macro definitions via `parse_named_arguments(definition=true)` using `parse_primary_expression` (1.x) | 5.5; macro definitions in `MacroTokenParser::parseDefinition` (variadic `...name`, constant check incl. NegUnary/PosUnary) | implement |
| P9 | `INVALID_LVALUES = ["true","false","none"]` (case sensitive, no `null`) | `strtolower(name) in [true,false,none,null]` -> `You cannot assign a value to "%s".` | fix |
| P10 | unknown tag -> `unreachable!`; no unknown function/filter/test errors; no suggestions | `Unknown "x" tag/function/filter/test.` + levenshtein suggestions (1.5) and `Unexpected "x" tag (expecting closing tag ...)` | implement |
| P11 | `Module` holds only `body` | parent, blocks, macros, traits, embedded templates (with index), source; child-template cleanup; `CorrectnessNodeVisitor` rules | implement 4.1/4.5 |
| P12 | token parsers registered: for, if, extends, include, block, use, filter, macro, import, from, set, spaceless, flush, do, embed - only `from` and `macro` implemented, the rest `unreachable!()` | core set: apply, for, if, extends, include, block, use, macro, import, from, set, types, flush, do, embed, with, deprecated, guard (+ autoescape, + sandbox) | implement all of section 6; delete `filter` and `spaceless` (must yield `Unknown "filter" tag.`) |
| P13 | imported symbols: functions only, UUID-based, lookup through ALL scopes | 'function' and 'template' symbol types; lookup in innermost and main scope only; `_self`; `MacroReferenceExpression` with preserved argument names; method calls accept named arguments | reimplement 4.4 |
| P14 | `subparse` panics on unexpected token kinds | `The lexer or the parser ended up in an unsupported state.` | |
| P15 | no tag-name-token line semantics, no block stack (`parent()` checks), no `setBlock` duplicate check | 4.2/4.4/5.4.6 | |
| P16 | no parse-time node checks (defined test incl. `??`, matches regex, macro argument rules, destructuring) | 5.6 | |
| P17 | error message texts from 1.x (`A default value for an argument must be a constant (a boolean, a string, a number, or an array).`, `A hash key must be ...: unexpected token ...`, `An array element must be followed by a comma`, `Expected name or number`) | 3.x texts (sections 5/6) | replace |
| P18 | 1.x-only syntax: `{% raw %}`, `{% filter %}`, `{% spaceless %}`, `for ... if`, `autoescape true` meaning html | removed/changed | reject exactly like 3.31.1 |
| P19 | tests in `tests/parser/named_and_default_args.rs` assert that method and macro calls do NOT support named arguments | supported since 3.15 | update tests |

---

## 9. Appendix A: verified probe transcript (selection)

Format: template (JSON-escaped) -> output or `ERR <message>`. Environment as described at the top.

```
"{{ (1+2) ?? 3 }}"                         ERR The "defined" test only works with simple variables in "index" at line 1.
"{{ 1__2 }}"                               ERR Unexpected token "name" of value "__2" ("end of print statement" expected) in "index" at line 1 column 5.
"{{ 9223372036854775807 }}|{{ 9223372036854775808 }}|{{ 1_000 }}|{{ 1.5e3 }}|{{ 1E2 }}|{{ 007 }}|{{ 1.0 }}"
                                           9223372036854775807|9.2233720368548E+18|1000|1500|100|7|1
"{{ 1_000.000_1e1_0 }}|{{ 1e1000 }}"       10000001000000|INF
"{{ \"a\\nb\" }}|{{ 'a\\tb' }}|{{ 'a\\'b' }}|{{ \"a\\\"b\" }}"   a\nb|a\tb|a'b|a"b
"{{ \"#{\" }}"                             ERR Unclosed """ in "index" at line 1.
"a  {{- 1 -}}  b|a \n {{~ 1 ~}} \n b"      a1b|a \n1\n b
"{% if true %}\nX{% endif %}\nY{{ 1 }}\nZ" XY1\nZ
"{% if true %}\r\nX{% endif %}\r\nY|a\r\nb" XY|a\nb
"{% verbatim %}\nx{{ y }}{% endverbatim %}\nY"   \nx{{ y }}\nY
"A {%~ verbatim ~%}  x  {%~ endverbatim ~%}  \n B"   Ax\n B
"{% verbatim %}abc"                        ERR Unexpected end of file: Unclosed "verbatim" block in "index" at line 1.
"line1\n{# a "                             ERR Unclosed comment in "index" at line 2.
"{{ x"                                     ERR Unexpected token "end of template" ("end of print statement" expected) in "index" at line 1 column 5.
"{{ x "                                    ERR Unclosed "variable" in "index" at line 1.
"\n{% if x\n"                              ERR Unclosed "block" in "index" at line 2.
"{{ x }"                                   ERR Unexpected "}" in "index" at line 1 column 6.
"{{ $x }}"                                 ERR Unexpected character "$" in "index" at line 1 column 4.
"{{ (1 }}"                                 ERR Unclosed "(" in "index" at line 1.
"{{ 'a' 'b' }}"                            ERR Unexpected token "string" of value "b" ("end of print statement" expected) in "index" at line 1 column 8.
"{{ \"a#{1}\" \"b\" }}"                    a1b
"{{- -}}"                                  ERR Unexpected token "end of print statement" of value "" in "index" at line 1.
"{% line 5 %}{{ x }}\n{{ y"                ERR Unexpected token "end of template" ("end of print statement" expected) in "index" at line 6 column 5.
"{{ true ?.5 : 1 }}"                       ERR Unexpected token "punctuation" of value ":" ("end of print statement" expected) in "index" at line 1 column 13.
"{{ 1...5 }}"                              ERR Unexpected token "operator" of value "..." ("end of print statement" expected) in "index" at line 1 column 5.
"{{ 1. }}"                                 ERR Expected name or number, got value "" of type "end of print statement" in "index" at line 1.
"{{ 1 0 }}"                                ERR Unexpected token "number" ("end of print statement" expected) in "index" at line 1 column 6.
"{{ 1 1.5 }}"                              ERR Unexpected token "number" of value "1.5" ("end of print statement" expected) in "index" at line 1 column 6.
"{{ x is odd foo }}"                       ERR Unknown "odd foo" test in "index" at line 1.
"{{ 2 is same as 2 + 1 }}"                 2
"{{ (max)(1, 2) }}"                        2
"{{ max(1,2)(3) }}"                        ERR Function name must be an identifier in "index" at line 1.
"{{ max((a = 1), 2) }}"                    ERR Positional arguments cannot be used after named arguments for function "max" in "index" at line 1.
"{{ 1 ?: 2 ?: 3 }}|{{ 2 ** 3 ** 2 }}|{{ -1**0 }}|{{ (-1)**0 }}|{{ -2 ** 2 }}"   1|512|-1|1|-4
"{{ true ? 'a' : false ? 'b' : 'c' }}|{{ false ? 'a' : false ? 'b' : 'c' }}"   a|c
"{{ {a, b: 2, 'c': 3, 4: 5, (1+1): 6, ...{z: 1}}|json_encode }}"   {"a":1,"b":2,"c":3,"4":5,"2":6,"z":1}   (a = 1)
"{{ {\"a\"} }}"                            ERR A mapping key must be followed by a colon (:). Unexpected token "punctuation" of value "}" ("punctuation" expected with value ":") in "index" at line 1 column 8.
"{{ [1, 2 3] }}"                           ERR A sequence element must be followed by a comma. Unexpected token "number" of value "3" ("punctuation" expected with value ",") in "index" at line 1 column 10.
"{{ [1,,2] }}"                             ERR Empty array elements are only allowed in destructuring assignments in "index" at line 1.
"{{ ...a }}"                               ERR The spread operator can only be used on sequence elements, mapping elements, and call arguments in "index" at line 1.
"{{ max(...[1, 5], 2) }}"                  ERR Normal arguments must be placed before argument unpacking in "index" at line 1.
"{{ x[1:2]|join }}|{{ x[:2]|join }}|{{ x[1:]|join }}|{{ x[-1:]|join }}"  (x=[1,2,3,4])  23|12|234|4
"{% do [x, y] = [1, 2] %}{{ x }}{{ y }}|{% do [, y2] = [1, 2] %}{{ y2 }}|{% do {a, b: c} = {a: 1, b: 2} %}{{ a }}{{ c }}"   12|2|12
"{% do a.b = 2 %}"                         ERR Cannot assign to "Twig\Node\Expression\GetAttrExpression", only variables can be assigned in "index" at line 1.
"{% do [] = [] %}"                         ERR Cannot destructure to an empty list of variables in "index" at line 1.
"{{ [1,2]|map((a, b) + 1)|join }}"         ERR A list of variables must be followed by an arrow in "index" at line 1.
"{{ [1,2]|map(1 => 2)|join }}"             ERR The arrow function argument must be a list of variables or a single variable in "index" at line 1.
"{{ x|join(a.b: 1) }}"                     ERR A parameter name must be a string, "Twig\Node\Expression\GetAttrExpression" given in "index" at line 1.
"{{ 'a' matches '/a' }}"                   ERR Regexp "/a" passed to "matches" is not valid: No ending delimiter '/' found in "index" at line 1.
"{{ TRUE }}|{{ FALSE }}|{{ NULL is null }}|{{ NONE is null }}|{{ True }}"   (True="var")   1||1|1|var
"{{ literal }}|{{ in }}|{{ is }}|{{ and }}"   L|I|S|A
"{% if x %}\n{% endfor %}"                 ERR Unexpected "endfor" tag (expecting closing tag for the "if" tag defined near line 2) in "index" at line 2.
"{% autoescape true %}{{ v }}{% endautoescape %}"   ERR Invalid escaping strategy "1" (valid ones: "html", "js", "url", "css", "html_attr", "html_attr_relaxed") in "index" at line 1.
"{% autoescape v %}{{ v }}{% endautoescape %}"      ERR An escaping strategy must be a string or false in "index" at line 1.
"{% block a %}x{% endblock b %}"           ERR Expected endblock for block "a" (but "b" given) in "index" at line 1.
"{% block a %}{% endblock %}\n{% block a %}{% endblock %}"   ERR The block 'a' has already been defined line 1 in "index" at line 2.
"{% extends 'b' %}{% if true %}\n{% block a %}{% endblock %}{% endif %}"   ERR A "block" tag cannot be under a "if" tag (line 1) in "index" at line 2.
"{% block a %}{{ parent() }}{% endblock %}{% extends 'b' %}"   ERR Calling the "parent" function on a template that does not call "extends" or "use" is forbidden in "index" at line 1.
"{{ parent() }}"                           ERR Calling the "parent" function outside of a block is forbidden in "index" at line 1.
"{% deprecated 'x' foo='a' %}"             ERR Unknown "foo" option in "index" at line 1.
"{% embed 'b' %}\n  text\n{% endembed %}"  ERR A template that extends another one cannot include content outside Twig blocks. Did you forget to put the content inside a {% block %} tag in "index" at line 2?
"{% extends 'b' %}{% extends 'c' %}"       ERR Multiple extends tags are forbidden in "index" at line 1.
"{% extends 'b' %}\n{% block a %}{% extends 'c' %}{% endblock %}"   ERR Cannot use "extend" in a block in "index" at line 2.
"{% extends 'b' %}{% macro m() %}{% extends 'c' %}{% endmacro %}"   ERR Cannot use "extend" in a macro in "index" at line 1.
"{% for x in y if x %}{% endfor %}"        ERR Unexpected token "name" of value "if" ("end of statement block" expected) in "index" at line 1 column 15.
"{% for x in y %}"                         ERR Unexpected end of template in "index" at line 1.
"{% from _self import m as n, m as o %}... {{ n() }}"   ERR Unknown "n" function. Did you mean "min", "enum", "range", "parent", "random", "include", "constant", "enum_cases", "include_only" in "index" at line 1?
"{% guard foo max %}{% endguard %}"        ERR Supported guard types are function, filter and test, "foo" given in "index" at line 1.
"{% guard function nope %}{{ nope() }}{% else %}N{% endguard %}"   N
"{% include 'b' with {} ignore missing %}" ERR Unexpected token "name" of value "ignore" ("end of statement block" expected) in "index" at line 1 column 24.
"{% macro m(...rest, a) %}{% endmacro %}"  ERR The variadic argument "rest" in macro "m" must be the last one in "index" at line 1.
"{% macro m(...rest = 1) %}{% endmacro %}" ERR The variadic argument "rest" in macro "m" cannot have a default value in "index" at line 1.
"{% macro m(a, ...a) %}{% endmacro %}"     ERR The variadic argument "a" in macro "m" cannot have the same name as another argument in "index" at line 1.
"{% macro m(a, a) %}{% endmacro %}"        ERR Argument "a" is defined twice for macro "m" in "index" at line 1.
"{% macro m(varargs) %}{% endmacro %}"     ERR The argument "varargs" in macro "m" cannot be defined because the variable "varargs" is reserved for arbitrary arguments in "index" at line 1.
"{% macro m(a = b) %}{% endmacro %}"       ERR A default value for an argument must be a constant (a boolean, a string, a number, a sequence, or a mapping) in "index" at line 1.
"{% macro m() %}{% endmacro n %}"          ERR Expected endmacro for macro "m" (but "n" given) in "index" at line 1.
"{% sandbox %}x{% endsandbox %}"  (sandbox ext)    x  (+ 3.15 deprecation)
"{% sandbox %}{{ x }}{% endsandbox %}"  (sandbox ext)   ERR Only "include" tags are allowed within a "sandbox" section in "index" at line 1.
"{% set a, b = 1 %}"                       ERR When using set, you must have the same number of variables and assignments in "index" at line 1.
"{% set a, b %}x{% endset %}"              ERR When using set with a block, you cannot have a multi-target in "index" at line 1.
"{% types a ? : 'x' %}"                    ERR A type name must be followed by a colon (:). Unexpected token "operator" of value "? :" ("punctuation" expected with value ":") in "index" at line 1 column 12.
"{% use x %}"                              ERR The template references in a "use" statement must be a string in "index" at line 1.
"{% with 1 %}{% endwith %}"                ERR Variables passed to the "with" tag must be a mapping in "index" at line 1.   (RuntimeError)
"\ufeff  {% extends 'b' %}"                B
"{% filter upper %}x{% endfilter %}"       ERR Unknown "filter" tag in "index" at line 1.
"{% includ %}"                             ERR Unknown "includ" tag. Did you mean "include" in "index" at line 1?
"{% 1 %}"                                  ERR A block must start with a tag name in "index" at line 1.
"{% block a-b %}"                          0
"{{ (a, b) => a }}"                        ERR (RuntimeError) An exception has been thrown during the rendering of a template ("Object of class Closure could not be converted to string") in "index" at line 1.
"{{ ({name} = user)|json_encode }}"  (user={name:N,email:E})   ["N"]
"{{ ([a, b, c] = [1])|json_encode }}"      [1,null,null]
"{% do {(a): b} = {x: 1} %}"               ERR An exception has been thrown during the compilation of a template ("Attribute "value" does not exist for Node "Twig\Node\Expression\Variable\ContextVariable".") in "index".
"{% macro n() %}N{% endmacro %}{% block a %}{% from _self import n %}{{ n() }}{% block b %}{{ n() }}{% endblock %}{% endblock %}"
                                           ERR Unknown "n" function. Did you mean "min", "enum", "range", "parent", "random", "include", "constant", "enum_cases", "include_only" in "index" at line 1?
```
