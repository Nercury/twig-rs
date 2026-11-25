# Twig 3.31.1 core callables: filters, functions, tests, operator callables, globals

Reference for porting every callable that Twig 3.31.1 core ships (`src/`) to Rust (twig-rs), so that templates
produce byte-identical output and identical error messages to PHP Twig 3.31.1 on PHP 8.5.13.

Paths in this document:

- `TWIG/` = `<scratch>\ext\twig-3.31.1\`
- `PHP/` = `<scratch>\ext\php-src-8.5.13\`
- Oracle scripts used to verify behaviour: `<scratch>\oracle\*.php`
  (run with `oracle/run.sh <file>`: Docker `php:8.5-cli` = PHP 8.5.13, Twig source mounted read-only, no composer;
  `boot.php` defines a `trigger_deprecation()` shim printing `Since <pkg> <ver>: <msg>`).

Evidence markers used throughout:

- **[ORACLE]** observed by running PHP 8.5.13 + Twig 3.31.1 (Docker, CLI SAPI, no php.ini: `precision=14`,
  `serialize_precision=-1`, `date.timezone` unset → UTC unless the script sets it, C locale).
- **[SRC]** derived from reading the cited source lines.
- **UNVERIFIED** neither run nor fully traced; treat as a hypothesis.

Unless stated otherwise, every error message shown is the *raw* message; Twig appends the template location
(section 1.7). Quoted messages are verbatim, including punctuation.

---

## Table of contents

1. [Callable model: registration, resolution, compilation, argument binding, errors](#1-callable-model)
   1. [Extensions and what they register](#11-extensions-and-what-they-register)
   2. [TwigFilter / TwigFunction / TwigTest options](#12-twigfilter--twigfunction--twigtest-options)
   3. [Name resolution, dynamic names, unknown callables](#13-name-resolution-dynamic-names-unknown-callables)
   4. [How a call is compiled (argument order)](#14-how-a-call-is-compiled-argument-order)
   5. [Argument binding (named / positional / variadic)](#15-argument-binding-callableargumentsextractor)
   6. [PHP parameter names of every core callable](#16-php-parameter-names-of-every-core-callable)
   7. [Error message format and wrapping](#17-error-message-format-and-wrapping)
   8. [PHP value conversions the callables rely on](#18-php-value-conversions-the-callables-rely-on)
   9. [Safety model: is_safe, is_safe_callback, pre_escape, preserves_safety, Markup](#19-safety-model)
   10. [Deprecation messages](#110-deprecation-messages)
2. [Globals and special variables](#2-globals-and-special-variables)
3. [Escaping: `escape`/`e`, `raw`, EscaperRuntime, strategies](#3-escaping)
4. [Filters (alphabetical)](#4-filters)
5. [Functions (alphabetical)](#5-functions)
6. [Tests (alphabetical)](#6-tests)
7. [Operator callables (`in`, `matches`, `starts with`, `has some`, `..`, ...)](#7-operator-callables)
8. [Attribute access algorithm (`.`, `[]`, `attribute()`)](#8-attribute-access-algorithm-coreextensiongetattribute)
9. [Sandbox: SecurityPolicy, SecurityChecker, node visitor, Sandbox, render_sandboxed](#9-sandbox)
10. [Debug extension: `dump()` and the var_dump format](#10-debug-extension-dump)
11. [StringLoader extension: `template_from_string()`](#11-stringloader-extension-template_from_string)
12. [PHP builtin reference (port notes)](#12-php-builtin-reference-port-notes)
13. [Non-portable / environment-dependent behaviour](#13-non-portable--environment-dependent-behaviour)
14. [Summary table](#14-summary-table)
15. [Appendix: remaining classes in scope](#15-appendix-remaining-classes-in-scope)

Filter index (§4): [abs](#41-abs) · [batch](#42-batch) · [capitalize](#43-capitalize) · [column](#44-column) ·
[convert_encoding](#45-convert_encoding) · [date](#46-date) · [date_modify](#47-date_modify) · [default](#48-default) ·
[e/escape](#31-filters-escape-e-raw) · [filter](#49-filter) · [find](#410-find) · [first](#411-first) · [format](#412-format) ·
[invoke](#413-invoke) · [join](#414-join) · [json_encode](#415-json_encode) · [keys](#416-keys) · [last](#417-last) ·
[length](#418-length) · [lower](#419-lower) · [map](#420-map) · [merge](#421-merge) · [nl2br](#422-nl2br) ·
[number_format](#423-number_format) · [raw](#31-filters-escape-e-raw) · [reduce](#425-reduce) · [replace](#426-replace) ·
[reverse](#427-reverse) · [round](#428-round) · [shuffle](#429-shuffle) · [slice](#430-slice) · [sort](#431-sort) ·
[spaceless](#432-spaceless-deprecated-since-312) · [split](#433-split) · [striptags](#434-striptags) · [title](#435-title) ·
[trim](#436-trim) · [upper](#437-upper) · [url_encode](#438-url_encode).
Function index (§5): attribute, block, constant, cycle, date, enum, enum_cases, include, include_only, max/min, parent,
random, range, source (+ dump §10, template_from_string §11, render_sandboxed §9.8).
Test index (§6): constant, defined, divisible by, empty, even/odd, iterable, mapping, none/null, same as, sequence, true.

---

## 1. Callable model

### 1.1 Extensions and what they register

`Environment::__construct` (`TWIG/src/Environment.php:118-153`) always registers, in this order:

1. `CoreExtension` (filters, functions, tests, token parsers, expression parsers, `CorrectnessNodeVisitor`)
2. `EscaperExtension` (filters `escape`, `e`, `raw`; `autoescape` tag; `EscaperNodeVisitor`)
3. `YieldNotReadyExtension` (only a node visitor; PHP >= 8.0 → always)
4. `OptimizerExtension` (only `OptimizerNodeVisitor`)

Optional core extensions (user must `addExtension`):

| Extension | File | Registers |
|---|---|---|
| `DebugExtension` | `src/Extension/DebugExtension.php` | function `dump` |
| `StringLoaderExtension` | `src/Extension/StringLoaderExtension.php` | function `template_from_string` |
| `SandboxExtension` (internal since 3.29) | `src/Extension/SandboxExtension.php` | `sandbox` tag (deprecated), `SandboxNodeVisitor`; no callables |
| `SandboxBridgeExtension` | `src/Extension/SandboxBridgeExtension.php` | function `render_sandboxed` (needs runtime `SandboxBridgeRuntime`) |
| `ProfilerExtension` | `src/Extension/ProfilerExtension.php` | node visitor only |
| `AttributeExtension` | `src/Extension/AttributeExtension.php` | user filters/functions/tests declared with `#[AsTwigFilter]`, `#[AsTwigFunction]`, `#[AsTwigTest]` |
| `StagingExtension` (internal) | `src/Extension/StagingExtension.php` | holds callables added via `Environment::addFilter/addFunction/addTest` |

**Override order** [SRC `TWIG/src/ExtensionSet.php:484-528`]: `initExtensions()` walks extensions in registration order,
then the staging extension last, doing `$this->filters[$name] = $filter` — a later registration with the same name
**silently replaces** an earlier one (no error), except inside `StagingExtension` itself where duplicates throw
`\LogicException('Filter "%s" is already registered.')` / `'Function "%s" is already registered.'` /
`'Test "%s" is already registered.'` / `'Tag "%s" is already registered.'` (`StagingExtension.php:31-95`).

Adding after initialisation throws `\LogicException`:
`'Unable to add filter "%s" as extensions have already been initialized.'`, same for `function`, `test`;
`'Unable to register extension "%s" as extensions have already been initialized.'`,
`'Unable to register extension "%s" as it is already registered.'` (`ExtensionSet.php:189-216, 264-271, 419-426`).
`getExtension()` of a missing one: `RuntimeError('The "%s" extension is not enabled.')` (`ExtensionSet.php:88-97`).

### 1.2 TwigFilter / TwigFunction / TwigTest options

`AbstractTwigCallable::__construct` (`TWIG/src/AbstractTwigCallable.php:26-67`) merges defaults:

| option | default | meaning |
|---|---|---|
| `needs_environment` | false | pass `Environment` |
| `needs_context` | false | pass the context array (by value) |
| `needs_charset` | false | pass `$env->getCharset()` |
| `needs_is_sandboxed` | false | pass bool: `hasExtension(SandboxExtension) && getChecker()->isSandboxed($this->source)` |
| `is_variadic` | false | extra args collected (see 1.5) |
| `always_allowed_in_sandbox` | false | skip sandbox allow-list |
| `deprecation_info` | null | `DeprecatedCallableInfo` |
| `deprecated`, `deprecating_package`, `alternative` | false, '', null | legacy (triggers 3.15 deprecation, converted to `deprecation_info`) |

Errors: `\LogicException('The "deprecation_info" option must be an instance of "Twig\DeprecatedCallableInfo".')`,
`\LogicException('When setting the "deprecation_info" option, you need to remove the obsolete deprecated options.')`.

`TwigFilter` adds (`src/TwigFilter.php:28-39`): `is_safe` null, `is_safe_callback` null, `pre_escape` null,
`preserves_safety` null, `node_class` `FilterExpression`.
`TwigFunction` adds (`src/TwigFunction.php:28-39`): `is_safe` null, `is_safe_callback` null, `node_class`
`FunctionExpression`, `parser_callable` null.
`TwigTest` adds (`src/TwigTest.php:28-37`): `node_class` `TestExpression`, `one_mandatory_argument` false;
`TwigTest::needsCharset/needsEnvironment/needsContext()` always return **false** (options ignored for tests);
`needs_is_sandboxed` is honoured for tests.

`getSafe(Node $args)` (filter and function): `is_safe` if not null, else `is_safe_callback($args)` if not null, else `[]`.
`getMinimalNumberOfRequiredArguments()` = count of needs_charset/env/context/is_sandboxed flags + count of dynamic
arguments, **+1 for filters and tests** (the piped input) (`AbstractTwigCallable.php:195-198`, `TwigFilter.php:69-72`,
`TwigTest.php:399-402`).

`__toString()` of a callable: `sprintf('%s(%s)', static::class, $name)`.

### 1.3 Name resolution, dynamic names, unknown callables

`ExtensionSet::getFilter/getFunction/getTest($name)` (`ExtensionSet.php:229-310, 440-465`):

1. exact name in the map → return it;
2. else iterate *dynamic* callables in registration order; a callable whose name contains `*` is registered with the
   regex `'#^'.str_replace('\\*', '(.*?)', preg_quote($name, '#')).'$#'` (`ExtensionSet.php:509-527`). First match wins;
   captured groups (lazy `(.*?)`) become the callable's *dynamic arguments*:
   `withDynamicArguments($name, $filter->getName(), $matches)` (clone with `name`=actual name,
   `dynamicName`=pattern name, `arguments`=captures);
3. else call each registered "undefined callback" (`registerUndefinedFilterCallback` …) in order; first non-`false` result wins;
4. else `null`.

`Parser::getFilter/getFunction/getTest` (`TWIG/src/Parser.php:510-620`) turn `null` into a `SyntaxError` at parse
time (message then gets suggestions):

- `'Unknown "%s" filter.'`, `'Unknown "%s" function.'`, `'Unknown "%s" test.'`
- Test names: after `is`/`is not`, the parser reads a NAME token; **if the next token is also a NAME**, it forms the
  two-word name `"<a> <b>"`, looks only that up, and consumes the second token (no fallback to the one-word test).
  [ORACLE] `{{ 1 is iterable foo }}` → `Unknown "iterable foo" test. Did you mean "iterable" in "index" at line 1?`
- Suggestions (`SyntaxError::addSuggestions`, `src/Error/SyntaxError.php:26-44`): for each registered name `$item`,
  `$lev = levenshtein($name, $item)`; keep it if `$lev <= strlen($name) / 3` **or** `str_contains($item, $name)`;
  `asort($alternatives)` (by distance, stable), then append `' Did you mean "%s"?'` with names joined by `'", "'`.
  Because the raw message then ends with `?`, the location is inserted before it (1.7):
  [ORACLE] `Unknown "upperx" filter. Did you mean "upper" in "index" at line 1?`; `Unknown "joni" filter in "index" at line 1.` (no suggestion).
  Note levenshtein compares bytes; `strlen($name)/3` is a float comparison.
- When parsing inside `{% guard %}`-style "ignore unknown" mode (`shouldIgnoreUnknownTwigCallables`), unknown callables
  become dummies returning `''` (Parser.php:521, 545, 591).
- If the resolved callable is deprecated, `triggerDeprecation($src->getPath() ?: $src->getName(), $line)` is called at
  parse time (see 1.10).

### 1.4 How a call is compiled (argument order)

`CallExpression::compileArguments` (`TWIG/src/Node/Expression/CallExpression.php:70-139`). The PHP call receives,
in this exact order, only those that apply:

1. `$this->env->getCharset()` if `needs_charset`
2. `$this->env` if `needs_environment`
3. `$context` if `needs_context` (the current context array, copied)
4. `$this->env->hasExtension(SandboxExtension::class) && $this->env->getExtension(SandboxExtension::class)->getChecker()->isSandboxed($this->source)` if `needs_is_sandboxed`
5. each dynamic argument (string captured from a `*` name)
6. the piped input (`node`) — filters and tests only
7. the template arguments after binding (1.5)

Callable dispatch (`CallExpression::compileCallable`, lines 32-68): a plain function-name string is called directly
(e.g. `abs(...)`, `json_encode(...)`, `max(...)`, `range(...)`, `is_iterable(...)`); `[ClassName, method]` with a static
method → `ClassName::method(...)`; non-static → `$this->env->getRuntime('ClassName')->method(...)` (runtime loader);
`[$extensionInstance, method]` → `$this->extensions['Class']->method(...)`.

Consequences for porting:
- PHP *internal* functions called directly (abs, json_encode, max, min, range, is_iterable) **reject extra
  positional arguments** with `ArgumentCountError` (`'abs() expects exactly 1 argument, 2 given'`,
  `'json_encode() expects at most 3 arguments, 4 given'`, `'is_iterable() expects exactly 1 argument, 2 given'`)
  [ORACLE], while *userland* methods (CoreExtension::…, EscaperRuntime::escape) silently ignore extra positional
  arguments [ORACLE `{{ s|e("html", null, true, 4) }}` → works; `{{ "a"|upper(1) }}` → `A`].
- Calls with exactly 2 arguments to `min`/`max` compile to PHP *frameless* calls whose tie-breaking differs from
  the variadic form (see §5 `max`/`min`).

### 1.5 Argument binding (CallableArgumentsExtractor)

`TWIG/src/Util/CallableArgumentsExtractor.php:40-172`, run at **compile time** (errors are `SyntaxError` with the
call's line). Input: the parsed argument list (ordered; positional items have int keys, named items have string keys).

```
extractArguments(arguments):
    extracted = {}         # normalized-name|int -> node
    nameMap = {}           # normalized -> original name
    named = false
    for (name, node) in arguments:
        if name is string: named = true
        elif named: throw SyntaxError('Positional arguments cannot be used after named arguments for %s "%s".' % (type, callableName))
        n = normalizeName(name)            # strtolower(str_replace('_', '', name)) for strings; ints unchanged
        extracted[n] = node; nameMap[n] = name
    if !named and !callable.isVariadic():
        min = callable.getMinimalNumberOfRequiredArguments()
        required = reflector.getNumberOfRequiredParameters()      # of the real PHP callable, all params
        if count(extracted) < required - min:
            argName = toSnakeCase(reflector.getParameters()[min + count(extracted)].name)
            throw SyntaxError('Value for argument "%s" is required for %s "%s".' % (argName, type, callableName))
        return extracted (as-is, positional)                       # no reflection mapping!
    if callable.getCallable() is null:
        throw named ? SyntaxError('Named arguments are not supported for %s "%s".')
                    : SyntaxError('Arbitrary positional arguments are not supported for %s "%s".')
    (params, isPhpVariadic) = getCallableParameters()             # see below
    args = []; paramNames = []; missing = []; optional = []; pos = 0
    for p in params:
        pname = p.name
        if callable === 'range' (the string):  pname 'start' -> 'low', 'end' -> 'high'
        paramNames[] = pname
        np = normalizeName(pname)
        if np in extracted:
            if pos in extracted: throw SyntaxError('Argument "%s" is defined twice for %s "%s".' % (pname, type, callableName))
            if missing: throw SyntaxError('Argument "%s" could not be assigned for %s "%s(%s)" because it is mapped to an internal PHP function which cannot determine default value for optional argument%s "%s".'
                          % (pname, type, callableName, join(', ', map(toSnakeCase, paramNames)), 's' if count(missing)>1 else '', join('", "', missing)))
            args += optional; args[] = extracted[np]; unset extracted[np]; optional = []
        elif pos in extracted:
            args += optional; args[] = extracted[pos]; unset extracted[pos]; optional = []; pos++
        elif p.isDefaultValueAvailable():
            optional[] = ConstantExpression(p.defaultValue)
        elif p.isOptional():                     # e.g. a variadic or internal param without reflectable default
            if extracted is empty: break
            missing[] = pname
        else:
            throw SyntaxError('Value for argument "%s" is required for %s "%s".' % (toSnakeCase(pname), type, callableName))
    if callable.isVariadic():
        arb = isPhpVariadic ? VariadicExpression([]) : ArrayExpression([])
        for (key, value) in extracted:
            if key is int: arb.add(value)
            else:
                orig = nameMap[key]
                if orig != toSnakeCase(orig): trigger_deprecation('twig/twig','3.15','Using "snake_case" for variadic arguments is required for a smooth upgrade with Twig 4.0; rename "%s" to "%s" in "%s" at line %d.')
                arb.add(value, key = toSnakeCase(orig))
            unset extracted[key]
        if arb.count(): args += optional; args[] = arb
    if extracted not empty:
        throw SyntaxError('Unknown argument%s "%s" for %s "%s(%s)".' % ('s' if count>1 else '', join('", "', keys(extracted)),
                          type, callableName, join(', ', map(toSnakeCase, paramNames))))       # line of first unknown node
    return args

getCallableParameters():
    params = ReflectionCallable.getTwigParameters(stripInput = node has 'node' child)  # strips: input, charset, env, context, is_sandboxed, each dynamic arg (in that order)
    if callable.isVariadic():
        last = end(params)
        if last has type 'array' and default [] : pop it; isPhpVariadic = false
        elif last.isVariadic(): pop it; isPhpVariadic = true
        else throw SyntaxError('The last parameter of "%s" for %s "%s" must be an array with default value, eg. "array $arg = []".')
    return (params, isPhpVariadic)

toSnakeCase(s) = strtolower(preg_replace(['/([A-Z]+)([A-Z][a-z])/', '/([a-z0-9])([A-Z])/'], '\1_\2', s))
```

Notes [ORACLE]:
- Keys in the error list are the *normalized* names (`strtolower`, underscores removed):
  `Unknown arguments "x", "y" for filter "lower()".` ; `Unknown argument "strateg" for filter "escape(strategy, charset, autoescape)".`
- Duplicate named arguments in the template (`replace(from=a, from=b)`) are not an error: the parser stores
  `$args[$name] = $value`, the last one wins [ORACLE → `c`].
- Missing-arg check in the positional path uses the raw PHP parameter name (snake-cased): `range(1)` →
  `Value for argument "end" is required for function "range".` (not `high`).
- A non-variadic PHP-variadic parameter (e.g. `format`'s `...$values`, `merge`'s `...$arrays`, `invoke`'s `...$arguments`)
  is still visible by name: `"%s-%s"|format("a", values="b")` → `Argument "values" is defined twice for filter "format".`;
  `"%s"|format(format="x")` → `Unknown argument "format" for filter "format(values)".` (input param stripped);
  `[1]|merge(arrays=[2])` → `Unknown argument "arrays" for filter "merge()".` (the stripped input *is* `$arrays`).
- Default values of PHP internal functions are reflectable in PHP 8 (`json_encode(depth=1)` → `json_encode($v, 0, 1)`).
- Special callables with custom parser/node classes bypass parts of this (see `parent`, `block`, `attribute`,
  `default`, `defined`, `same as`, `divisible by`, `constant` test, `render_sandboxed`, `enum`, `enum_cases`).
  E.g. `1 is same as(value=1)` and `undef|default(default="d")` fail at compile time with
  `An exception has been thrown during the compilation of a template ("Node "0" does not exist for Node "Twig\Node\Nodes".").`
  (no line number) [ORACLE].
- Spread arguments `...expr` are allowed in calls (`ArgumentsTrait.php:45-55`); "Normal arguments must be placed before
  argument unpacking." if a normal argument follows a spread. [ORACLE] `"%s %s"|format(...["a","b"])` → `a b`.

### 1.6 PHP parameter names of every core callable

Needed for named arguments, for the "required" messages and for `Unknown argument … (list)` texts. The list in
parentheses is what the error message prints (snake_case, input/injected params stripped).

| Twig callable | PHP callable | PHP parameters (after injected ones) | Printed list |
|---|---|---|---|
| filter `abs` | `abs` | `num` (= input) | `abs()` |
| filter `batch` | `CoreExtension::batch` | `items`, `size`, `fill = null`, `preserveKeys = true` | `batch(size, fill, preserve_keys)` |
| filter `capitalize` | `CoreExtension::capitalize` | (charset), `string` | `capitalize()` |
| filter `column` | `CoreExtension::column` | (env, isSandboxed), `array`, `name`, `index = null` | `column(name, index)` |
| filter `convert_encoding` | `CoreExtension::convertEncoding` | `string`, `to`, `from` | `convert_encoding(to, from)` |
| filter `date` | `$core->formatDate` | `date`, `format = null`, `timezone = null` | `date(format, timezone)` |
| filter `date_modify` | `$core->modifyDate` | `date`, `modifier` | `date_modify(modifier)` |
| filter `default` | `CoreExtension::default` (node `DefaultFilter`) | `value`, `default = ''` | — |
| filter `escape`, `e` | `EscaperRuntime::escape` (runtime, non-static) | `string`, `strategy = 'html'`, `charset = null`, `autoescape = false` | `escape(strategy, charset, autoescape)` |
| filter `filter` | `CoreExtension::filter` | (env, isSandboxed), `array`, `arrow` | `filter(arrow)` |
| filter `find` | `CoreExtension::find` | (env, isSandboxed), `array`, `arrow` | `find(arrow)` |
| filter `first` | `CoreExtension::first` | (charset), `item` | `first()` |
| filter `format` | `CoreExtension::sprintf` | `format`, `...values` | `format(values)` |
| filter `invoke` | `CoreExtension::invoke` | `\Closure arrow`, `...arguments` | `invoke(arguments)` |
| filter `join` | `CoreExtension::join` | `value`, `glue = ''`, `and = null` | `join(glue, and)` |
| filter `json_encode` | `json_encode` | `value`, `flags = 0`, `depth = 512` | `json_encode(flags, depth)` |
| filter `keys` | `CoreExtension::keys` | `array` | `keys()` |
| filter `last` | `CoreExtension::last` | (charset), `item` | `last()` |
| filter `length` | `CoreExtension::length` | (charset), `thing` | `length()` |
| filter `lower` | `CoreExtension::lower` | (charset), `string` | `lower()` |
| filter `map` | `CoreExtension::map` | (env, isSandboxed), `array`, `arrow` | `map(arrow)` |
| filter `merge` | `CoreExtension::merge` | `...arrays` (input is `arrays[0]`) | `merge()` |
| filter `nl2br` | `CoreExtension::nl2br` | `string` | `nl2br()` |
| filter `number_format` | `$core->formatNumber` | `number`, `decimal = null`, `decimalPoint = null`, `thousandSep = null` | `number_format(decimal, decimal_point, thousand_sep)` |
| filter `raw` | none (node `RawFilter`) | arguments ignored | — |
| filter `reduce` | `CoreExtension::reduce` | (env, isSandboxed), `array`, `arrow`, `initial = null` | `reduce(arrow, initial)` |
| filter `replace` | `CoreExtension::replace` | `str`, `from` | `replace(from)` |
| filter `reverse` | `CoreExtension::reverse` | (charset), `item`, `preserveKeys = false` | `reverse(preserve_keys)` |
| filter `round` | `CoreExtension::round` | `value`, `precision = 0`, `method = 'common'` | `round(precision, method)` |
| filter `shuffle` | `CoreExtension::shuffle` | (charset), `item` | `shuffle()` |
| filter `slice` | `CoreExtension::slice` | (charset), `item`, `start`, `length = null`, `preserveKeys = false` | `slice(start, length, preserve_keys)` |
| filter `sort` | `CoreExtension::sort` | (env, isSandboxed), `array`, `arrow = null` | `sort(arrow)` |
| filter `spaceless` | `CoreExtension::spaceless` | `content` | `spaceless()` |
| filter `split` | `CoreExtension::split` | (charset), `value`, `delimiter`, `limit = null` | `split(delimiter, limit)` |
| filter `striptags` | `CoreExtension::striptags` | `string`, `allowable_tags = null` | `striptags(allowable_tags)` |
| filter `title` | `CoreExtension::titleCase` | (charset), `string` | `title()` |
| filter `trim` | `CoreExtension::trim` | `string`, `characterMask = null`, `side = 'both'` | `trim(character_mask, side)` |
| filter `upper` | `CoreExtension::upper` | (charset), `string` | `upper()` |
| filter `url_encode` | `CoreExtension::urlencode` | `url` | `url_encode()` |
| function `attribute` | parser callable; fake `fn ($variable, $attribute, $arguments = null)` | | |
| function `block` | parser callable; fake `fn ($name, $template = null)` | | |
| function `constant` | `CoreExtension::constant` | `constant`, `object = null`, `bool checkDefined = false` | `constant(constant, object, check_defined)` |
| function `cycle` | `CoreExtension::cycle` | `values`, `position` | `cycle(values, position)` |
| function `date` | `$core->convertDate` | `date = null`, `timezone = null` | `date(date, timezone)` |
| function `dump` | `DebugExtension::dump` (is_variadic) | (env, context), `...vars` | |
| function `enum` | `CoreExtension::enum` (node `EnumFunction`) | `string enum` | |
| function `enum_cases` | `CoreExtension::enumCases` (node `EnumCasesFunction`) | `string enum` | |
| function `include` | `CoreExtension::include` | (env, context), `template`, `variables = []`, `withContext = true`, `ignoreMissing = false`, `sandboxed = false` | `include(template, variables, with_context, ignore_missing, sandboxed)` |
| function `include_only` | `CoreExtension::includeOnly` | (env), `template`, `array variables = []`, `bool ignoreMissing = false` | |
| function `max` / `min` | `max` / `min` | `value`, `...values` | `max(value, values)` |
| function `parent` | parser callable | arguments ignored | |
| function `random` | `CoreExtension::random` | (charset), `values = null`, `max = null` | `random(values, max)` |
| function `range` | `range` | `start`→`low`, `end`→`high`, `step = 1` | `range(low, high, step)` |
| function `render_sandboxed` | `SandboxBridgeRuntime::render` (node `RenderSandboxedFunction`) | `string name`, `array context` (+ compile-time `output_strategy`) | |
| function `source` | `CoreExtension::source` | (env), `name`, `ignoreMissing = false` | |
| function `template_from_string` | `StringLoaderExtension::templateFromString` | (env), `string\|\Stringable template`, `?string name = null` | |
| test `empty` | `CoreExtension::testEmpty` | `value` | |
| test `iterable` | `is_iterable` | `value` | |
| test `sequence` / `mapping` | `CoreExtension::testSequence` / `testMapping` | `value` | |
| tests `even`, `odd`, `defined`, `same as`, `none`, `null`, `divisible by`, `constant`, `true` | none (node classes) | positional `arguments[0]`, `[1]` | |

### 1.7 Error message format and wrapping

`Twig\Error\Error::updateRepr` (`TWIG/src/Error/Error.php`): final message =
raw message with a trailing `.` or `?` removed, then ` in "<template name>"` if the error has a source with a name,
then ` at line <N>` if line > 0 (and ` column <C>` if a column is set — lexer/parser errors), then the removed
punctuation re-appended. Examples [ORACLE]:
`The "merge" filter expects a sequence or a mapping, got "string" for argument 1 in "index" at line 1.`;
`Regexp "/(/" passed to "matches" is not valid: Compilation failed: missing closing parenthesis at offset 1 in "index" at line 1`
(no trailing punctuation because the raw message has none);
`Block "nope" on template "blocks" does not exist in "blocks".` (line unknown → no ` at line`).

Wrapping (`Template::handleException`, `TWIG/src/Template.php:614-631`): any non-`Twig\Error\Error` throwable thrown
while rendering becomes
`RuntimeError(sprintf('An exception has been thrown during the rendering of a template ("%s").', $e->getMessage()), -1, $source, $e)`
followed by line guessing. Hence every PHP `TypeError`/`ValueError`/`ArgumentCountError`/`DivisionByZeroError`/
`DateMalformedStringException` from a builtin appears as e.g.
`An exception has been thrown during the rendering of a template ("abs(): Argument #1 ($num) must be of type int|float, string given") in "index" at line 1.`
Exceptions thrown during *compilation* (node compile) become
`SyntaxError('An exception has been thrown during the compilation of a template ("%s").')` without a line (see 1.5).

PHP warnings/notices/deprecations raised by builtins (e.g. `Array to string conversion`, `iconv(): Detected an illegal
character in input string`, `strtr(): Ignoring replacement of empty string`) are **not** converted to exceptions by Twig;
rendering continues with PHP's fallback value. They go to PHP's error handler. A port should expose them through a
diagnostics hook, not fail.

Messages that include PHP-internal identifiers cannot be reproduced exactly — see §13 (e.g. closure names, compiled
class names, PHP file paths in `TypeError`s raised for *userland* Twig methods:
`Twig\Extension\CoreExtension::invoke(): Argument #1 ($arrow) must be of type Closure, string given, called in /twig/src/Environment.php(444) : eval()'d code on line 44`).

### 1.8 PHP value conversions the callables rely on

None of Twig's PHP files declare `strict_types`, and compiled templates don't either [SRC grep], so all builtin calls use
PHP's *coercive* typing. Facts used throughout this document (all [ORACLE] on PHP 8.5.13 unless noted):

- **Float → string** (echo, concatenation, `(string)`, string parameters, `implode`, `strtr` values, `http_build_query`):
  `zend_double_to_str` = `zend_gcvt(value, precision=14, '.', 'E')` (`PHP/Zend/zend_operators.c:3616-3628`,
  `PHP/Zend/zend_strtod.c:4509-4600`): 14 significant digits, E-style when the decimal exponent `decpt > 14` or
  `decpt < -3`; mantissa always has a `.` in E-style (`1.0E+25`), exponent without leading zeros (`E+25`, `E-5`).
  Examples: `0.1+0.2`→`0.3`, `1.0`→`1`, `-0.0`→`-0`, `1e14`→`1.0E+14`, `1e13`→`10000000000000`, `0.0001`→`0.0001`,
  `0.00001`→`1.0E-5`, `1/3`→`0.33333333333333`, `(float)PHP_INT_MAX`→`9.2233720368548E+18`, `INF`→`INF`,
  `-INF`→`-INF`, `NAN`→`NAN` **plus** PHP 8.5 warning `unexpected NAN value was coerced to string`.
- **Float → JSON / var_dump**: `serialize_precision=-1` → shortest round-trip digits (`zend_gcvt` mode 0, 17 digits max):
  `0.30000000000000004`, `1.0e+25` (JSON, lowercase `e`), `float(1.0E+25)` (var_dump), `100.0`→`100` (JSON) / `float(100)`.
- **bool/null → string**: `true`→`"1"`, `false`→`""`, `null`→`""`.
- **array → string**: `"Array"` + warning `Array to string conversion` (not an exception).
- **object → string**: `__toString()` if `Stringable`; else `Error("Object of class X could not be converted to string")`.
- **Internal function, string parameter**: int/float/bool coerced as above; `null` → deprecation
  `<fn>(): Passing null to parameter #<n> ($<name>) of type <type> is deprecated` then treated as `""`/0;
  array → `TypeError('<fn>(): Argument #<n> ($<name>) must be of type string, array given')`;
  non-Stringable object → `... must be of type string, <Class> given`.
- **int parameter**: numeric strings accepted incl. leading/trailing whitespace (`" 5"`, `"5 "`, `"1e3"`→1000);
  `"5abc"`, `"abc"`, `""`, `"0x1A"` → `TypeError ... must be of type int, string given`; float with fractional part →
  deprecation `Implicit conversion from float 2.5 to int loses precision` and truncation; float-string `"2.5"` →
  `Implicit conversion from float-string "2.5" to int loses precision`; `true`→1.
- **`int|float` parameter** (`abs`, `round`'s value is pre-cast by Twig, `ceil`): same acceptance as above.
- **Arithmetic / `%`** (used by `even`, `odd`, `divisible by`, `cycle`): numeric strings OK; leading-numeric `"5abc"` →
  value 5 + warning `A non-numeric value encountered`; non-numeric `"abc"`/`""` →
  `TypeError('Unsupported operand types: string % int')`; arrays → `Unsupported operand types: array % int`;
  `null`→0; fractional floats → `Implicit conversion from float 9.5 to int loses precision`; `% 0` →
  `DivisionByZeroError('Modulo by zero')`; `/ 0` → `Division by zero`.
- **Explicit casts** `(float)`/`(int)` never fail: `(float)"abc"`=0, `(float)"12abc"`=12, `(float)[]`=0, `(float)[1]`=1,
  `(float)$obj` = 1 + warning `Object of class X could not be converted to float`.
- **Array keys**: int; numeric-decimal strings → int (`"2"`→2, not `"02"`/`" 2"`); `bool`→int; `null`→`""` (PHP 8.5:
  deprecation `Using null as an array offset is deprecated, use an empty string instead`); float → truncated int
  (deprecation if fractional); arrays/objects as keys → `TypeError('Cannot access offset of type array on array')`.
- **`get_debug_type()`** names used in messages: `null`, `bool`, `int`, `float`, `string`, `array`, the class name
  (`Twig\Markup`, `ArrayObject`), `Closure`, `class@anonymous` [ORACLE].
- **Iteration of Traversable**: `IteratorAggregate::getIterator()` is followed (recursively by PHP); generators can only
  be iterated once (`Cannot traverse an already closed generator` on reuse) [ORACLE].

### 1.9 Safety model

Compile-time safety analysis (`TWIG/src/NodeVisitor/SafeAnalysisNodeVisitor.php`) decides whether an auto-escaped
`{{ … }}` (or a filter input with `pre_escape`) needs an `escape` filter:

- `ConstantExpression`, `BlockReferenceExpression` (`block()`), `ParentExpression` (`parent()`),
  `MethodCallExpression`, `MacroReferenceExpression` → safe for `['all']`.
- `?:` and `??` (OperatorEscapeInterface) → intersection of both operands' safety (the operands themselves are escaped
  individually: `EscaperNodeVisitor::escapeConditional`).
- `FilterExpression` → `filter.getSafe(arguments)`; if that is `[]`, intersection of the input's safety with
  `getPreservesSafety()` (`preserves_safety`, default `[]` → not safe). No core filter uses `preserves_safety`.
- `FunctionExpression` → `function.getSafe(arguments)`.
- everything else (variables, attribute access, operators, tests…) → `[]`.
- `getSafe()` post-processing: if `html_attr` ∈ safe → add `html`, `html_attr_relaxed`; if `html_attr_relaxed` ∈ safe →
  add `html`.
- `intersectSafe(a, b)`: if either empty → `[]`; if `all` ∈ a → b; if `all` ∈ b → a; else `array_intersect`.

`EscaperNodeVisitor` (`TWIG/src/NodeVisitor/EscaperNodeVisitor.php`):
- default strategy per template = `EscaperExtension::getDefaultStrategy($templateName)` (section 3.8);
- `{% autoescape %}` pushes a strategy; blocks inherit;
- every `PrintNode` whose expression is not safe for the active strategy (and not `all`) gets wrapped in
  `escape(expr, '<strategy>', null, true)` — i.e. **autoescape=true** call of `EscaperRuntime::escape`;
- for filters with `pre_escape` (core: `nl2br`, `spaceless`, both `'html'`), the *input* gets wrapped in
  `escape(input, 'html', null, true)` unless already safe for `html`, regardless of whether autoescape is on.

Safety declarations in core:

| callable | safety |
|---|---|
| `escape`, `e` | `is_safe_callback` = `EscaperExtension::escapeFilterIsSafe`: no args → `['html']`; first arg a constant → `[that constant value]`; first arg non-constant → `[]` |
| `raw` | `['all']` |
| `nl2br`, `spaceless` | `['html']` + `pre_escape` `'html'` |
| `include`, `include_only`, `source` | `['all']` |
| `dump` | `['html']` if (`xdebug` loaded and `xdebug.mode` contains `develop` and `html_errors` on) **or `PHP_SAPI === 'cli'`**, else `[]` (`DebugExtension.php:24-33`) |
| `render_sandboxed` | `[output_strategy]` (compile-time literal) |
| everything else | `[]` (output is escaped; a `Markup` input does not make the output safe) |

`Twig\Markup` (`TWIG/src/Markup.php`): `__construct($content, $charset, array $safeStrategies = ['all'])`
(content cast to string); `__toString()` returns content; `count()` = `mb_strlen(content, charset)`;
`jsonSerialize()` = content; `isSafeFor($s)`: true if any declared strategy is `all`, equals `$s`, or implies `$s`
via `EscaperRuntime::IMPLIED_STRATEGIES` (`html_attr` ⇒ `html_attr_relaxed`, `html`; `html_attr_relaxed` ⇒ `html`);
`withContent()` clones with new content (internal). Markup is `Countable`, `JsonSerializable`, `Stringable`.

Runtime truthiness: Twig wraps operands of `and`, `or`, `xor`, `not`, `?:` (test part) and `if`-like conditions with the
internal `true` test (§6 `true`): a `Markup` value is converted to its string before PHP truthiness, so
`Markup('')` and `Markup('0')` are **falsy** [ORACLE]. Callables that test truthiness themselves (`filter`, `find`,
`has some`, `has every`, `default` uses `testEmpty`) use raw PHP truthiness: any object, including `Markup('')`, is truthy
[ORACLE `[1,2]|filter(v => m)|length` with `m = Markup('')` → 2].

### 1.10 Deprecation messages

`DeprecatedCallableInfo::triggerDeprecation($file, $line)` (`TWIG/src/DeprecatedCallableInfo.php:44-69`), constructor
`(string $package, string $version, ?string $altName = null, ?string $altPackage = null, ?string $altVersion = null)`:

```
msg = sprintf('Twig %s "%s" is deprecated', ucfirst(type), name)     # type = filter|function|test
if altName: msg .= '; use "%s"' (+ ' from the "%s" package' if altPackage) (+ ' (available since version %s)' if altVersion) . ' instead'
if file: msg .= ' in %s' (+ ' at line %d' if line)
msg .= '.'
trigger_deprecation(package, version, msg)    # symfony/deprecation-contracts → E_USER_DEPRECATED "Since <package> <version>: <msg>"
```

Core deprecated callable: filter `spaceless` (`DeprecatedCallableInfo('twig/twig', '3.12')`) →
[ORACLE] `Since twig/twig 3.12: Twig Filter "spaceless" is deprecated in index at line 1.` (file = source path or name).
Other runtime deprecations are listed with each callable.

---

## 2. Globals and special variables

Core extensions implement no `GlobalsInterface`, so **core registers no global values**. Globals come only from
`Environment::addGlobal()` and extensions implementing `GlobalsInterface` (`ExtensionSet::getGlobals`, merged with
`array_merge`, environment globals win: `Environment::getGlobals` = `array_merge(extensionGlobals, envGlobals)`).
Globals are merged *under* the render context at the start of every template render
(`Template::yield`: `$context += $this->env->getGlobals()`, `Template.php:471-482`) — context keys win, and globals
are visible in `_context` and in templates included with `include_only` (they are re-added per template) [ORACLE].
`addGlobal` after initialisation for a *new* name: `\LogicException('Unable to add global "%s" as the runtime or the extensions have already been initialized.')`.

Special names compiled by `NameExpression` (`TWIG/src/Node/Expression/NameExpression.php:22-26`), documented in
`doc/templates.rst:122-129`:

| name | compiles to | value |
|---|---|---|
| `_self` | `$this->getTemplateName()` | current template name (string) [ORACLE `index`] |
| `_context` | `$context` | the whole current context array (incl. globals and loop/set variables) |
| `_charset` | `$this->env->getCharset()` | environment charset, upper-cased (`setCharset`: `strtoupper`, `'UTF8'`→`'UTF-8'`) [ORACLE `utf8` → `UTF-8`] |

`x is defined` on any special name compiles to `true`. `_self.foo` / `_self.foo()` is a macro reference
(`DotExpressionParser.php:62-80`), not attribute access. In the sandbox, `_self` is never wrapped in `CheckToStringNode`.

---

## 3. Escaping

### 3.1 Filters `escape`, `e`, `raw`

Registered by `EscaperExtension::getFilters` (`TWIG/src/Extension/EscaperExtension.php:56-63`):

```php
new TwigFilter('escape', [EscaperRuntime::class, 'escape'], ['is_safe_callback' => [self::class, 'escapeFilterIsSafe'], 'node_class' => EscapeFilter::class]),
new TwigFilter('e',      [EscaperRuntime::class, 'escape'], ['is_safe_callback' => [self::class, 'escapeFilterIsSafe'], 'node_class' => EscapeFilter::class]),
new TwigFilter('raw', null, ['is_safe' => ['all'], 'node_class' => RawFilter::class]),
```

- Signature: `x|escape(strategy = 'html', charset = null, autoescape = false)`; `e` is an alias (separate filter
  name — the sandbox treats `e` and `escape` as different names [ORACLE `Filter "e" is not allowed`]).
- `EscapeFilter` node (`src/Node/Expression/Filter/EscapeFilter.php`) only changes code generation: when flagged by the
  escaper visitor it calls the template's cached `$this->escaper->escape(...)`. Same semantics.
- `raw` (`RawFilter`): compiles to just its input; ignores arguments [ORACLE `{{ "x"|raw(1) }}` → `x`]; safe `['all']`.
  `raw` only prevents escaping if it is the *last* filter (`{{ v|raw|upper }}` is escaped) [ORACLE].
- Strategy safety: `{{ v|e }}`/`{{ v|e('html') }}` inside html autoescape → not double-escaped; `{{ v|e(var) }}` → escaped
  twice (non-constant strategy ⇒ unsafe) [ORACLE `{% set st = "html" %}{{ s|e(st) }}` with `<b>` → `&amp;lt;b&amp;gt;`].

### 3.2 `EscaperRuntime::escape` — port-ready algorithm

`TWIG/src/Runtime/EscaperRuntime.php:118-352`. The runtime is created per environment with the env charset
(`Environment.php:141-143`).

```
escape(string, strategy = 'html', charset = null, autoescape = false):
    if autoescape and string is Markup:
        return string.isSafeFor(strategy) ? string : escape((string) string, strategy, charset)   # recursive, autoescape=false
    if string is not a PHP string:
        if string is Stringable:
            if autoescape:
                c = class(string)
                if c not in safeClasses:                      # lazily inherit safe strategies from parents/interfaces
                    safeClasses[c] = []
                    for cls in class_parents(string) + class_implements(string):   # PHP order: parents (nearest first) then interfaces
                        if cls in safeClasses:
                            safeClasses[c] = array_unique(array_merge(safeClasses[c], safeClasses[cls]))
                            addSafeLookup(c, safeClasses[cls])
                if safeLookup[strategy][c] or safeLookup['all'][c]: return (string) string
            string = (string) string                          # __toString()
        elif strategy in ['html','js','css','html_attr','html_attr_relaxed','url']:
            return string                                     # unchanged: int stays int, null stays null, array stays array, non-Stringable object unchanged
        # else: custom/unknown strategy -> value is passed on unconverted
    if string === '': return ''
    charset = charset ?: this.charset                         # '' and null both fall back
    switch strategy:
        'html' -> 3.3, 'js' -> 3.4, 'css' -> 3.5, 'html_attr'/'html_attr_relaxed' -> 3.6, 'url' -> rawurlencode(string)
        default:
            if strategy in escapers: return escapers[strategy](string, charset)     # string may be a non-string here (int, array...)
            valid = implode('", "', array_merge(['html','js','url','css','html_attr','html_attr_relaxed'], array_keys(escapers)))
            throw RuntimeError(sprintf('Invalid escaping strategy "%s" (valid ones: "%s").', strategy, valid))
```

Consequences [ORACLE]:
- `{{ 5|e }}` → `5`; `{{ 1.5e25|e }}` → `1.5E+25` (printed by echo); `{{ true|e }}` → `1`; `{{ null|e }}` → ``.
- `{{ [1,2] }}` → `Array` + warning (escape returns the array, echo converts).
- Non-Stringable object printed: `An exception has been thrown during the rendering of a template ("Object of class NS could not be converted to string")`.
- Invalid strategy (also when the value is an array): `Invalid escaping strategy "foo" (valid ones: "html", "js", "url", "css", "html_attr", "html_attr_relaxed").`
  With a custom escaper `csv` registered: `... (valid ones: "html", "js", "url", "css", "html_attr", "html_attr_relaxed", "csv").`
- `''` returns `''` before strategy dispatch, so even an invalid strategy is accepted for `''` and custom escapers are not called for `''`.
- The strategy is not validated at compile time.

Other runtime API: `setEscaper($strategy, callable $callable)` (callable `(string $string, string $charset): string`;
built-in names cannot be overridden — the switch handles them first), `getEscapers()`,
`setSafeClasses(array $map)` (resets), `addSafeClass(string $class, array $strategies)` (`ltrim($class, '\\')`,
merges, and records implied strategies: `html_attr` ⇒ `html_attr_relaxed`,`html`; `html_attr_relaxed` ⇒ `html`).
Safe classes apply **only on the autoescape path** (`{{ obj }}`), never to an explicit `|e` [ORACLE: safe class `S`
with `['html']`: `{{ o }}` → raw; `{{ o|e }}` → escaped; in `{% autoescape "js" %}` → escaped].
Deprecated `EscaperExtension::setEscaper($strategy, $callable)` (3.10) wraps a callable receiving `(Environment, string, charset)`.

### 3.3 Strategy `html`

```
if charset === 'UTF-8': return htmlspecialchars(string, ENT_QUOTES | ENT_SUBSTITUTE, 'UTF-8')
static list (exact-case keys): 'ISO-8859-1','ISO8859-1','ISO-8859-15','ISO8859-15','utf-8','UTF-8','CP866','IBM866','866',
   'CP1251','WINDOWS-1251','WIN-1251','1251','CP1252','WINDOWS-1252','1252','KOI8-R','KOI8-RU','KOI8R','BIG5','950',
   'GB2312','936','BIG5-HKSCS','SHIFT_JIS','SJIS','932','EUC-JP','EUCJP','ISO8859-5','ISO-8859-5','MACROMAN'
if charset in list: return htmlspecialchars(string, ENT_QUOTES | ENT_SUBSTITUTE, charset)
if strtoupper(charset) in list: list[charset] = true; return htmlspecialchars(string, ENT_QUOTES | ENT_SUBSTITUTE, charset)
string = iconv(charset, 'UTF-8', string)          # convertEncoding(string, 'UTF-8', charset); throws RuntimeError if iconv() missing
string = htmlspecialchars(string, ENT_QUOTES | ENT_SUBSTITUTE, 'UTF-8')
return iconv('UTF-8', charset, string)
```

`htmlspecialchars` with these flags (doctype HTML 4.01, `double_encode = true`) — port rules (`PHP/ext/standard/html.c:1131-1350`):
- replace `&`→`&amp;` (always, existing entities are re-encoded: `&amp;`→`&amp;amp;`), `"`→`&quot;`, `'`→`&#039;`,
  `<`→`&lt;`, `>`→`&gt;`; every other valid character is copied unchanged (including NUL, control chars, DEL).
- UTF-8 decoding is strict (`get_next_char`, html.c:89-200): lead `< 0x80` 1 byte; `0x80–0xC1` invalid (1 byte);
  `0xC2–0xDF` needs 1 continuation; `0xE0–0xEF` needs 2 continuations, value ≥ 0x800, not D800–DFFF;
  `0xF0–0xF4` needs 3 continuations, value 0x10000–0x10FFFF; `≥ 0xF5` invalid. On failure the number of bytes
  consumed follows UTR#36 strategy 2: stop *before* the first byte that is not a continuation or that is a valid
  lead byte; if all continuation bytes are present but the value is overlong/surrogate/out of range, consume the whole
  sequence. Each failure emits U+FFFD (`EF BF BD`) for UTF-8, or `&#xFFFD;` for every other charset.
  [ORACLE] `"a\xffb\xc3"` → `a\u{FFFD}b\u{FFFD}`; `"a\xc0\xafb"` → two FFFD; `"a\xed\xa0\x80b"` → one FFFD;
  `"a\xf4\x90\x80\x80b"` → one FFFD.
- Charset names accepted by `htmlspecialchars` (case-insensitive compare, `PHP/ext/standard/html_tables.h:35-75`):
  ISO-8859-1, ISO8859-1, ISO-8859-15, ISO8859-15, utf-8, cp1252, Windows-1252, 1252, BIG5, 950, GB2312, 936,
  BIG5-HKSCS, Shift_JIS, SJIS, 932, SJIS-win, CP932, EUCJP, EUC-JP, eucJP-win, KOI8-R, koi8-ru, koi8r, cp1251,
  Windows-1251, win-1251, iso8859-5, iso-8859-5, cp866, 866, ibm866, MacRoman. Single-byte charsets never fail;
  BIG5/GB2312/BIG5-HKSCS/SJIS/EUC-JP have their own multi-byte validity rules in `get_next_char` (html.c:200-345).
- [ORACLE] charset `ISO-8859-1`, input `"\xe9<"` → `"\xe9&lt;"`; charset `KOI8-U` (not in list) → iconv round-trip → `"\xe9&lt;"`.

### 3.4 Strategy `js`

```
if charset !== 'UTF-8': string = iconv(charset, 'UTF-8', string)
if !preg_match('//u', string): throw RuntimeError('The string to escape is not a valid UTF-8 string.')
string = preg_replace_callback('#[^a-zA-Z0-9,\._]#Su', cb, string)      # each non-matching *code point*
cb(char):
    short = { '\\' => '\\\\', '/' => '\\/', "\x08" => '\b', "\x0C" => '\f', "\x0A" => '\n', "\x0D" => '\r', "\x09" => '\t' }
    if char in short: return short[char]
    cp = mb_ord(char, 'UTF-8')
    if cp < 0x10000: return sprintf('\u%04X', cp)                       # uppercase hex, 4 digits
    u = cp - 0x10000; return sprintf('\u%04X\u%04X', 0xD800 | (u >> 10), 0xDC00 | (u & 0x3FF))
if charset !== 'UTF-8': string = iconv('UTF-8', charset, string)
```

Unescaped set: `A–Z a–z 0–9 , . _`. Note `\0` is not a short escape (→ `\u0000`), `"` and `'` → `\u0022`, `\u0027`.
[ORACLE] input `<>&"' \t\n\r\0\x01\x7f-_.,~:@[]/\=+%é€😀\u{80}\u{9f}\u{a0}` →
`\u003C\u003E\u0026\u0022\u0027\u0020\t\n\r\u0000\u0001\u007F\u002D_.,\u007E\u003A\u0040\u005B\u005D\/\\\u003D\u002B\u0025\u00E9\u20AC\uD83D\uDE00\u0080\u009F\u00A0`.

### 3.5 Strategy `css`

```
(same UTF-8 conversion and validity check / message as js)
string = preg_replace_callback('#[^a-zA-Z0-9]#Su', cb, string)
cb(char) = sprintf('\\%X ', strlen(char) === 1 ? ord(char) : mb_ord(char, 'UTF-8'))   # backslash, uppercase hex (no padding), ONE space
(convert back with iconv if charset !== 'UTF-8')
```

[ORACLE] `<` → `\3C `, tab → `\9 `, NUL → `\0 `, `é` → `\E9 `, `€` → `\20AC `, `😀` → `\1F600 `.

### 3.6 Strategies `html_attr` and `html_attr_relaxed`

```
(same UTF-8 conversion and validity check / message as js)
regex = strategy == 'html_attr' ? '#[^a-zA-Z0-9,\.\-_]#Su' : '#[^a-zA-Z0-9,\.\-_:@\[\]]#Su'
cb(chr):
    ord = ord(chr[0])                                   # FIRST BYTE only
    if (ord <= 0x1F and chr not in ["\t", "\n", "\r"]) or (0x7F <= ord <= 0x9F): return '&#xFFFD;'
    if strlen(chr) == 1:
        return {34: '&quot;', 38: '&amp;', 60: '&lt;', 62: '&gt;'}[ord] ?? sprintf('&#x%02X;', ord)
    return sprintf('&#x%04X;', mb_ord(chr, 'UTF-8'))   # at least 4 uppercase hex digits
(convert back with iconv if charset !== 'UTF-8')
```

Because only the first byte is tested, U+0080–U+009F (UTF-8 `C2 80`–`C2 9F`) are **not** replaced by FFFD; they become
`&#x0080;`… [ORACLE]. Unescaped sets: `html_attr`: `A–Z a–z 0–9 , . - _`; `html_attr_relaxed` additionally `: @ [ ]`.
[ORACLE] `' '`→`&#x20;`, `'`→`&#x27;`, tab→`&#x09;`, LF→`&#x0A;`, CR→`&#x0D;`, NUL/0x01/DEL→`&#xFFFD;`,
`~`→`&#x7E;`, `é`→`&#x00E9;`, `😀`→`&#x1F600;`.
Safety: a value safe for `html_attr` is also safe for `html_attr_relaxed` and `html`; `html_attr_relaxed` ⇒ `html`.

### 3.7 Strategy `url`

`rawurlencode($string)` (no charset conversion): bytes `A–Z a–z 0–9 - _ . ~` kept, every other byte → `%XX` uppercase hex
(`PHP/ext/standard/url.c:450-538`). Invalid UTF-8 passes byte-wise [ORACLE `"a\xffb"` → `a%FFb`].

### 3.8 Default strategy, `FileExtensionEscapingStrategy`

`EscaperExtension::__construct($defaultStrategy = 'html')` (from the `autoescape` environment option).
`setDefaultStrategy('name')` ⇒ `[FileExtensionEscapingStrategy::class, 'guess']`. `getDefaultStrategy($templateName)`
(`EscaperExtension.php:122-140`): memoised per name (max 1000 entries, oldest evicted); if the strategy is not a string and
not `false` it is a callable invoked with the template name (string strategies are never treated as function names);
returns the string or `false` (disabled).

`FileExtensionEscapingStrategy::guess(string $name)` (`TWIG/src/FileExtensionEscapingStrategy.php:28-54`):
```
if last char of name in ['/', '\\']: return 'html'
if name ends with '.twig': name = name without the 5 chars
ext = pathinfo(name, PATHINFO_EXTENSION)
'js','json' -> 'js';  'css' -> 'css';  'txt' -> false;  otherwise 'html'
```
(`pathinfo` extension = text after the last `.` in the basename; basename splits on `/` (and `\` on Windows builds) — UNVERIFIED for
Windows-specific separators.)


---

## 4. Filters

All core filters are registered in `CoreExtension::getFilters()` (`TWIG/src/Extension/CoreExtension.php:239-291`)
unless noted (`escape`, `e`, `raw` → §3). Shared helpers referenced below:

```
toArray(seq, preserveKeys = true)          # CoreExtension::toArray, CoreExtension.php:1406-1417
    if seq is Traversable: return iterator_to_array(seq, preserveKeys)
    if seq is not array:   return seq                       # unchanged (NOT wrapped)
    return preserveKeys ? seq : array_values(seq)

splitIntoCharacters(string, name)          # CoreExtension.php:1273-1280 (private)
    if preg_match_all('/./us', string, m) === false:
        throw RuntimeError(sprintf('Unable to split the string passed to "%s" into characters: %s.', name, preg_last_error_msg()))
    return m[0] ?: ['']                     # empty string -> ['']; '/./us' = every UTF-8 code point incl. "\n"

convertEncoding(string, to, from)          # CoreExtension.php:1257-1264 (also the convert_encoding filter)
    if !function_exists('iconv'): throw RuntimeError('Unable to convert encoding: required function iconv() does not exist. You should install ext-iconv or symfony/polyfill-iconv.')
    return iconv(from, to, string ?? '')    # declared return type string: iconv false -> '' (coercion), plus iconv warning
```

`preg_last_error_msg()` for invalid UTF-8 is `Malformed UTF-8 characters, possibly incorrectly encoded`.

### 4.1 `abs`

- Registration: `new TwigFilter('abs', 'abs')` — PHP's internal `abs(int|float $num): int|float`, called directly.
- Behaviour: int → `|n|` (int), except `PHP_INT_MIN` → float `9.2233720368547758E+18` (printed `9.2233720368548E+18`);
  float → `fabs`. `PHP/ext/standard/math.c:252-271`.
- Coercion (`int|float` param, 1.8): numeric strings OK (`" 5"`, `"5 "`, `" 5 "`→5, `"1e3"`→1000, `"1.5"`→1.5, `"-0"`→0);
  `true`→1, `false`→0; `null` → 0 + deprecation `abs(): Passing null to parameter #1 ($num) of type int|float is deprecated`;
  `"abc"`, `"5abc"`, `""`, `"0x1A"`, arrays, objects → `TypeError`:
  `abs(): Argument #1 ($num) must be of type int|float, string given` (`array given`, `<Class> given`).
- Extra argument → `abs() expects exactly 1 argument, 2 given`. Named argument → compile-time
  `Unknown argument "num" for filter "abs()".` [ORACLE all].
- `(-0.0)|abs` prints `0` (fabs(-0.0) = 0.0) [ORACLE].

### 4.2 `batch`

- Registration: `new TwigFilter('batch', [self::class, 'batch'])`. Signature `items|batch(size, fill = null, preserve_keys = true)`.
- Source: `CoreExtension.php:1715-1735`.

```
batch(items, size, fill = null, preserveKeys = true):
    if !is_iterable(items):
        throw RuntimeError(sprintf('The "batch" filter expects a sequence or a mapping, got "%s".', get_debug_type(items)))
    size = (int) ceil(size)                          # PHP ceil(int|float): strings coerced, "abc" -> TypeError 'ceil(): Argument #1 ($num) must be of type int|float, string given'
    result = array_chunk(toArray(items, preserveKeys), size, preserveKeys)   # size < 1 -> ValueError 'array_chunk(): Argument #2 ($length) must be greater than 0'
    if fill !== null and result not empty:
        last = count(result) - 1
        fillCount = size - count(result[last])
        repeat fillCount times: result[last][] = fill    # appends with next integer key (max int key + 1, or 0)
    return result
```

`array_chunk($a, $n, $preserve)`: consecutive chunks of `n` entries in order; with `$preserve` each chunk keeps original
keys, otherwise each chunk is re-indexed 0..n-1. Chunks themselves are indexed 0..k.
[ORACLE]: `['a','b','c','d']|batch(3)` → `[["a","b","c"],{"3":"d"}]`; `|batch(3,'x')` → `[["a","b","c"],{"3":"d","4":"x","5":"x"}]`;
`|batch(3,'x',false)` → `[["a","b","c"],["d","x","x"]]`; `{a:1,b:2,c:3}|batch(2,'z')` rows `a=1 b=2` / `c=3 0=z`;
`[1,2,3]|batch(2.1)` → `[[1,2,3]]` (ceil); `batch("2")` OK; `[]|batch(2,'x')` → `[]`; `batch(0)`/`batch(-1)` →
`An exception has been thrown during the rendering of a template ("array_chunk(): Argument #2 ($length) must be greater than 0")`;
`"abc"|batch(2)` → `The "batch" filter expects a sequence or a mapping, got "string".`

### 4.3 `capitalize`

- Registration: `new TwigFilter('capitalize', [self::class, 'capitalize'], ['needs_charset' => true])`.
- Source `CoreExtension.php:1370-1373`:
  `mb_strtoupper(mb_substr($string ?? '', 0, 1, $charset), $charset) . mb_strtolower(mb_substr($string ?? '', 1, null, $charset), $charset)`
- Uses *full* case mapping (§12.12): first code point upper-cased (may expand: `ß`→`SS`), rest lower-cased with
  final-sigma rule. Note upper, not title case: `ǆa` → `Ǆa` (U+01C4), not `ǅa` [ORACLE].
- `$string` is passed to `mb_substr(string $string, ...)`: int/float/bool coerced, `null`→`''`, arrays/non-Stringable
  objects → `TypeError` (`mb_substr(): Argument #1 ($string) must be of type string, array given`).
- Invalid UTF-8 bytes become `?` (mbstring substitution) [ORACLE `"a\xffb"` → `A?b`].
- Unknown charset → `mb_substr(): Argument #4 ($encoding) must be a valid encoding, "X" given` (UNVERIFIED exact arg number;
  verified form for upper: `mb_strtoupper(): Argument #2 ($encoding) must be a valid encoding, "NOPE-1" given`).
- [ORACLE] `'my first car'` → `My first car`; `"ßa BC"` → `SSa bc`; `"élan VITAL"` → `Élan vital`; `""` → ``.

### 4.4 `column`

- Registration: `new TwigFilter('column', [self::class, 'column'], ['needs_environment' => true, 'needs_is_sandboxed' => true])`.
  Signature `array|column(name, index = null)`. Source `CoreExtension.php:2031-2057`.

```
column(env, isSandboxed, array, name, index = null):
    if !is_iterable(array): throw RuntimeError(sprintf('The "column" filter expects a sequence or a mapping, got "%s".', get_debug_type(array)))
    if array is Traversable: array = iterator_to_array(array)          # keys preserved
    if isSandboxed:
        policy = env.getExtension(SandboxExtension).getChecker().getSecurityPolicy()
        for item in array: if item is object:
            policy.checkPropertyAllowed(item, (string) name)                  # SecurityNotAllowedPropertyError (no line/source set here)
            if index !== null: policy.checkPropertyAllowed(item, (string) index)
    return array_column(array, name, index)
```

`array_column` (`PHP/ext/standard/array.c:4695-4800`): `name`/`index` are `string|int|null`. For each row: arrays →
`row[name]` (symtable lookup: numeric strings act as int keys); objects → property `name` if it exists or `__isset` says so
(public/visible properties; `__get` honoured); rows lacking the column are skipped. `name === null` → whole row.
With `index`: key = `row[index]` (array-key coercion of the value), rows without the index column are appended with the
next integer key. Result keys are otherwise 0..n-1.
[ORACLE] `[{a:1,b:"x"},{a:2,b:"y"},{b:"z"}]|column("a")` → `[1,2]`; `column("b","a")` → `{"1":"x","2":"y"}`;
`column(null,"b")` → rows keyed by `b`; objects with public prop `v` → values.

### 4.5 `convert_encoding`

- Registration: `new TwigFilter('convert_encoding', [self::class, 'convertEncoding'])`. Signature `s|convert_encoding(to, from)`.
- Behaviour: `convertEncoding()` helper above = `iconv($from, $to, $string ?? '')`. On failure iconv returns `false`,
  which the `string` return type coerces to `''`; PHP emits a warning:
  `iconv(): Wrong encoding, conversion from "UTF-8" to "NOPE" is not allowed`,
  `iconv(): Detected an illegal character in input string`, or
  `iconv(): Detected an incomplete multibyte character in input string` [ORACLE first two].
- `//TRANSLIT`, `//IGNORE` suffixes are handled by the platform iconv (glibc in the oracle: `"€"` → `ASCII//TRANSLIT` → `EUR`) — §13.
- [ORACLE] `"é"|convert_encoding("ISO-8859-1","UTF-8")` → byte `E9`; `"é"|convert_encoding("UTF-16BE","UTF-8")` → `00 E9`.

### 4.6 `date`

- Registration: `new TwigFilter('date', [$this, 'formatDate'])` (bound to the CoreExtension *instance*: defaults are per
  environment). Signature `d|date(format = null, timezone = null)`. Source `CoreExtension.php:522-534`.

```
formatDate(date, format = null, timezone = null):
    if format === null:
        format = date instanceof DateInterval ? dateFormats[1] : dateFormats[0]     # defaults 'F j, Y H:i' and '%d days'
    if date instanceof DateInterval: return date.format(format)                  # DateInterval::format, timezone ignored (§12.16)
    return convertDate(date, timezone).format(format)                          # DateTimeInterface::format (§12.16)
```

`convertDate` is the `date()` function (§5.5). Configuration (`CoreExtension.php:138-213`):
`setDateFormat($format = null, $dateIntervalFormat = null)` (null keeps the current one), `getDateFormat()`,
`setTimezone($tz)` (string → `new \DateTimeZone($tz)`), `getTimezone()` (lazy: `new \DateTimeZone(date_default_timezone_get())`).

[ORACLE] (PHP default timezone UTC): `DateTime('2024-01-11 15:17:05', Europe/Paris)|date` → `January 11, 2024 14:17`
(converted to the default timezone!); `0|date` → `January 1, 1970 00:00`; `d|date("Y-m-d H:i T", false)` keeps `CET`;
`d|date("Y-m-d H:i T","America/New_York")` → `2024-01-11 09:17 EST`; interval `(2024-01-01)->diff(2024-03-05 10:20:30)|date`
→ `4 days` (`%d` = days component, not total), with `"%y-%m-%d %h:%i:%s %R %a"` → `0-2-4 10:20:30 + 64`;
after `setDateFormat('d/m/Y', '%d jours')`: `"2024-03-05"|date` → `05/03/2024`, `DateInterval('P2D')|date` → `2 jours`.
Escaping in format: a backslash before a format character outputs that character literally (`\Y` → `Y`); in a Twig string literal the backslash is written doubled (template source `"\\Y"` gives the format `\Y`) [ORACLE].
Errors are those of `convertDate` / PHP (`Failed to parse time string (notadate) at position 0 (n): The timezone could not be found in the database`,
`DateTimeZone::__construct(): Unknown or bad timezone (Nowhere/City)`) wrapped as "An exception has been thrown…".

### 4.7 `date_modify`

- Registration: `new TwigFilter('date_modify', [$this, 'modifyDate'])`. Signature `d|date_modify(modifier)`.
- Source `CoreExtension.php:548-551`: `return $this->convertDate($date, false)->modify($modifier);`
  - `false` timezone: the value keeps its own timezone (a string date is parsed in PHP's default timezone;
    a timestamp gets `+00:00`); `DateTime` input is cloned first (original untouched); `DateTimeImmutable` returns a new object.
  - Returns a `DateTime`/`DateTimeImmutable` object (print it through `date`).
  - Invalid modifier (PHP 8.3+) throws `DateMalformedStringException`:
    `DateTime::modify(): Failed to parse time string (blah) at position 0 (b): The timezone could not be found in the database`
    (`DateTimeImmutable::modify(): …` for immutables) → wrapped [ORACLE].
- [ORACLE] `"2024-01-31"|date_modify("+1 month")|date("Y-m-d")` → `2024-03-02` (overflow semantics of timelib).

### 4.8 `default`

- Registration: `new TwigFilter('default', [self::class, 'default'], ['node_class' => DefaultFilter::class])`.
  Signature `v|default(default = '')`. Runtime: `CoreExtension::default($value, $default = '')` (`CoreExtension.php:910-917`):
  `return testEmpty($value) ? $default : $value;` (testEmpty = §6.4 `empty`).
- Compile-time rewrite (`TWIG/src/Node/Expression/Filter/DefaultFilter.php:39-63`): if the filtered node is a plain
  variable (`ContextVariable`, incl. parenthesised `(undef)`) or an attribute access (`GetAttrExpression`: `a.b`, `a['b']`,
  `a.b()`, nested), it compiles to
  `(<node> is defined) ? default(<node>, args...) : <default-arg-or ''>`
  where the `defined` test suppresses strict-variable errors and the ternary's false branch is the first argument
  *node* (`$arguments->getNode('0')`) or `''`. Otherwise (filters, operators, literals…) it is a plain call.
  Hence, with `strict_variables`, `undef|default('x')`, `a.missing|default('x')`, `a.b.c|default`, `x.y()|default` never
  throw, but `(undef ~ '')|default('x')` throws `Variable "undef" does not exist.` [ORACLE].
- A *named* argument breaks the rewrite: `undef|default(default="d")` →
  compile-time `An exception has been thrown during the compilation of a template ("Node "0" does not exist for Node "Twig\Node\Nodes".").`
  For a non-variable input named args work: `"x"|default(default="d")` → `x` [ORACLE].
- Empty values: `''`, `false`, `null`, `[]`, Countable with count 0 (incl. `Markup('')`, empty `ArrayObject`), Traversable
  with no elements (consumes generators!), Stringable whose string is `''`. **Not** empty: `0`, `'0'`, `0.0`, `' '`
  [ORACLE `[{{ 0|default("d") }}]` → `[0]`].

### 4.9 `filter`

- Registration: `new TwigFilter('filter', [self::class, 'filter'], ['needs_environment' => true, 'needs_is_sandboxed' => true])`.
  Signature `seq|filter(arrow)`. Source `CoreExtension.php:2064-2078`.

```
filter(env, isSandboxed, array, arrow):
    if !is_iterable(array): throw RuntimeError(sprintf('The "filter" filter expects a sequence/mapping or "Traversable", got "%s".', get_debug_type(array)))
    checkArrow(isSandboxed, arrow, 'filter', 'filter')           # §4.40
    if array is array: return array_filter(array, arrow, ARRAY_FILTER_USE_BOTH)   # arrow(value, key); keys preserved; PHP truthiness of result
    return new CallbackFilterIterator(new IteratorIterator(array), arrow)          # LAZY; arrow(current, key, iterator); keys preserved
```

- [ORACLE] `[1,2,3,4]|filter(v => v > 2)` → `{"2":3,"3":4}` (keys kept); mapping with `(v,k)` works;
  generator input returns an iterator object (iterable; `json_encode` gives `{}`; iterating yields `1=2 2=3 …`).
- Missing arrow → compile-time `Value for argument "arrow" is required for filter "filter".`

### 4.10 `find`

- Registration: like `filter` (`needs_environment`, `needs_is_sandboxed`). Signature `seq|find(arrow)`. Source `CoreExtension.php:2085-2100`.

```
find(env, isSandboxed, array, arrow):
    if !is_iterable(array): throw RuntimeError(sprintf('The "find" filter expects a sequence or a mapping, got "%s".', get_debug_type(array)))
    checkArrow(isSandboxed, arrow, 'find', 'filter')
    for (k, v) in array: if arrow(v, k): return v      # PHP truthiness
    return null
```

[ORACLE] `[34,36,38,40]|find(v => v > 38)` → `40`; no match → `` (null).

### 4.11 `first`

- Registration: `new TwigFilter('first', [self::class, 'first'], ['needs_charset' => true])`. Source `CoreExtension.php:795-800`.

```
first(charset, item):
    elements = slice(charset, item, 0, 1, false)        # §4.30
    return is_string(elements) ? elements : current(elements)   # current([]) === false
```

- [ORACLE] `[1,2]|first` → `1`; `{a:1,b:2}|first` → `1`; `'1234'|first` → `1`; `5|first` → `5` (mb_substr of "5");
  `null|first` → `''`; **`[]|first` returns `false`** (prints ``, `is same as(false)` is true);
  generators work (LimitIterator path).

### 4.12 `format`

- Registration: `new TwigFilter('format', [self::class, 'sprintf'])`. Signature `fmt|format(...values)`.
- Source `CoreExtension.php:560-563`: `return \sprintf($format ?? '', ...$values);` → PHP `sprintf` (§12.8).
- Errors from PHP (wrapped): `3 arguments are required, 2 given` (`ArgumentCountError`, counts include the format),
  `Unknown format specifier "y"`, `Missing format specifier at end of string`,
  `Argument number specifier must be greater than zero and less than 2147483647`, `Precision must be ...`, `Width must be ...`;
  notice `sprintf(): Requested precision of 60 digits was truncated to PHP maximum of 53 digits`.
  Format must be a string: `sprintf(): Argument #1 ($format) must be of type string, array given`.
- [ORACLE] `"%s|%d|%05.2f|%x|%X|%o|%b|%c|%e|%E|%g|%G|%u|%%|%'*10s|%-5s|%+d|%5.1s|%1$s"` with
  `("ab", 42.9, 3.14159, 255, 255, 8, 5, 65, 1234.5678, 0.00012, 0.00001234, 1e20, -1, "x", "ab", 7, "xyz")` →
  `ab|42|03.14|ff|FF|10|101|A|1.234568e+3|1.200000E-4|1.234e-5|1.0E+20|18446744073709551615|%|*********x|ab   |+7|    x|ab`;
  `null|format("x")` → ``; `5|format` → `5`; `"%s"|format([1])` → `Array` + warning; `"%s|%d|%s"|format(true,true,false)` → `1|1|`.

### 4.13 `invoke`

- Registration: `new TwigFilter('invoke', [self::class, 'invoke'])`. Signature `arrow|invoke(...arguments)`.
- Source `CoreExtension.php:967-970`: `public static function invoke(\Closure $arrow, ...$arguments): mixed { return $arrow(...$arguments); }`
- Input must be a `Closure` (Twig arrow functions compile to closures): otherwise PHP TypeError
  `Twig\Extension\CoreExtension::invoke(): Argument #1 ($arrow) must be of type Closure, string given, called in … on line …`
  (non-portable suffix, §13). Too few arguments for the arrow → PHP `ArgumentCountError` with compiled closure name (§13);
  extra arguments are ignored.
- Twig arrow functions (`ArrowFunctionExpression`, `src/Node/Expression/ArrowFunctionExpression.php:42-61`) compile to
  `function ($__a__, $__b__) use ($context, $macros) { $context["a"] = $__a__; ...; return <expr>; }` — all parameters are
  required, the context is captured **by value** at creation (assignments inside never leak) [ORACLE].
- [ORACLE] `{% set f = (a, b) => a ~ "-" ~ b %}{{ f|invoke(1, 2) }}` → `1-2`; `f|invoke(arguments=1)` binds to the variadic.

### 4.14 `join`

- Registration: `new TwigFilter('join', [self::class, 'join'])`. Signature `v|join(glue = '', and = null)`. Source `CoreExtension.php:838-859`.

```
join(value, glue = '', and = null):
    if !is_iterable(value): value = (value === null) ? [] : [value]
    value = toArray(value, false)                     # values only
    if count(value) == 0: return ''
    if and === null or and === glue: return implode(glue, value)
    if count(value) == 1: return value[0]             # returned AS IS (not stringified) -> the return type string coerces it
    return implode(glue, array_slice(value, 0, -1)) . and . value[count - 1]
```

- `implode` (§12.6) converts elements: string as is, int decimal, float precision-14 form, `true`→`1`, `false`/`null`→``,
  arrays → `Array` + warning, Stringable → `__toString`, other objects → Error. `glue` is coerced to string; `null` glue →
  deprecation `implode(): Passing null to parameter #1 ($separator) of type array|string is deprecated` [ORACLE].
- Declared return type `string`: the single-element `$value[0]` is coerced (int→"5"; array → `TypeError`… UNVERIFIED for arrays).
- [ORACLE] `[1,2,3]|join` → `123`; `join(", ", " and ")` → `1, 2 and 3`; `[1]|join(", "," and ")` → `1`;
  `[1,2]|join(", ",", ")` → `1, 2`; `"abc"|join(",")` → `abc`; `null|join` → ``; `5|join` → `5`; `{a:1,b:2}|join(",")` → `1,2`;
  `[true,false,null,1.5,1.0]|join(",")` → `1,,,1.5,1`.

### 4.15 `json_encode`

- Registration: `new TwigFilter('json_encode', 'json_encode')` — PHP internal `json_encode(mixed $value, int $flags = 0, int $depth = 512): string|false`.
- Not safe: output is HTML-escaped by autoescape [ORACLE `{{ {a:"<"}|json_encode }}` → `{&quot;a&quot;:&quot;&lt;&quot;}`].
- Failure (invalid UTF-8, NAN/INF, depth exceeded, recursion…) returns `false` → prints `` (no exception, no warning)
  [ORACLE]. Named args are `flags`, `depth` (the doc's `options` name is wrong: `Unknown argument "options" for filter "json_encode(flags, depth)".`).
- Default encoding rules: §12.14. [ORACLE] `{a:"é/<>&'\"", b:[1, 1.0, 0.1, true, null], c:{}, d:[]}` →
  `{"a":"\u00e9\/<>&'\"","b":[1,1,0.1,true,null],"c":[],"d":[]}` (the `é` is emitted as the six ASCII characters
  backslash, `u`, `0`, `0`, `e`, `9`); `{0:"a",1:"b"}` → `["a","b"]`; `{1:"a",2:"b"}` →
  `{"1":"a","2":"b"}`; `Markup` → its string; objects → public props.

### 4.16 `keys`

- Registration: `new TwigFilter('keys', [self::class, 'keys'])`. Source `CoreExtension.php:930-960`.

```
keys(array):
    if array is Traversable:
        while array is IteratorAggregate: array = array.getIterator()
        keys = []
        if array is Iterator: array.rewind(); while array.valid(): keys[] = array.key(); array.next()
        else: for (key, _) in array: keys[] = key          # internal Traversable
        return keys                                         # raw keys (any type, duplicates kept)
    if !is_array(array): return []
    return array_keys(array)
```

[ORACLE] `[5,6]|keys` → `[0,1]`; `{a:1,"2":2}|keys` → `["a",2]` (numeric-string key is int); `"abc"|keys` → `[]`;
generator keys `a,b,a` → `["a","b","a"]`; keys `1.5,true,null` kept raw → `[1.5,true,null,…]`.

### 4.17 `last`

- Registration: `new TwigFilter('last', [self::class, 'last'], ['needs_charset' => true])`. Source `CoreExtension.php:811-816`:
  `elements = slice(charset, item, -1, 1, false); return is_string(elements) ? elements : current(elements);`
- Traversables with negative start are materialised (`iterator_to_array`, keys dropped) then sliced.
- [ORACLE] `[1,2]|last` → `2`; `'1234'|last` → `4`; `[]|last` → `false`; `"été"|last` → `é`.

### 4.18 `length`

- Registration: `new TwigFilter('length', [self::class, 'length'], ['needs_charset' => true])`. Source `CoreExtension.php:1289-1312`.

```
length(charset, thing):
    if thing === null: return 0
    if is_scalar(thing): return mb_strlen(thing, charset)            # int/float/bool coerced to string first: 12345->5, 1.5->3, true->1, false->0
    if thing is Countable or is_array(thing) or thing is SimpleXMLElement: return count(thing)
    if thing is Traversable: return iterator_count(thing)            # consumes generators
    if thing is Stringable: return mb_strlen((string) thing, charset)
    return 1                                                          # any other object
```

- `Markup` is Countable → `mb_strlen(content, markupCharset)`.
- `mb_strlen` with invalid UTF-8 counts each bad byte-group as one char (§12.12) [ORACLE `"a\xed\xa0\x80b"` → 5].
- Unknown charset → `mb_strlen(): Argument #2 ($encoding) must be a valid encoding, "NOPE-1" given` [ORACLE].

### 4.19 `lower`

- Registration: `new TwigFilter('lower', [self::class, 'lower'], ['needs_charset' => true])`. Source `CoreExtension.php:1333-1336`:
  `return mb_strtolower($string ?? '', $charset);` (full lower-case mapping incl. final sigma, §12.12).
- [ORACLE] `"İSTANBUL ǅ ΣΑΣ ÉA"` → `i̇stanbul ǆ σας éa`; `"ǳ"` → `ǳ`.

### 4.20 `map`

- Registration: `new TwigFilter('map', [self::class, 'map'], ['needs_environment' => true, 'needs_is_sandboxed' => true])`.
  Source `CoreExtension.php:2107-2121`.

```
map(env, isSandboxed, array, arrow):
    if !is_iterable(array): throw RuntimeError(sprintf('The "map" filter expects a sequence or a mapping, got "%s".', get_debug_type(array)))
    checkArrow(isSandboxed, arrow, 'map', 'filter')
    r = []
    for (k, v) in array: r[k] = arrow(v, k)          # keys preserved; duplicate Traversable keys overwrite (first position kept)
    return r
```

[ORACLE] `[1,2,3]|map(v => v * 2)` → `[2,4,6]`; generator `a=>1,b=>2,a=>3` with `(v,k) => k ~ v` → `{"a":"a3","b":"b2"}`.

### 4.21 `merge`

- Registration: `new TwigFilter('merge', [self::class, 'merge'])`. Signature `a|merge(b, c, ...)`. Source `CoreExtension.php:734-747`.

```
merge(...arrays):                     # arrays[0] is the piped input
    result = []
    for (argNumber, array) in arrays:
        if !is_iterable(array):
            throw RuntimeError(sprintf('The "merge" filter expects a sequence or a mapping, got "%s" for argument %d.', get_debug_type(array), argNumber + 1))
        result = array_merge(result, toArray(array))      # keys preserved from Traversable then merged
    return result
```

`array_merge`: string keys — later values overwrite, position of first occurrence kept; integer keys — always appended and
renumbered 0..n. [ORACLE] `[1,2]|merge(["a","b"])` → `[1,2,"a","b"]`; `{a:1,b:2}|merge({b:3,c:4})` → `{"a":1,"b":3,"c":4}`;
`{1:"a",2:"b"}|merge({1:"c"})` → `["a","b","c"]`; `[1]|merge({x:1},[2])` → `{"0":1,"x":1,"1":2}`; `null|merge([1])` →
`... got "null" for argument 1.`; `[1]|merge([2], "x")` → `... got "string" for argument 3.`; `[1]|merge` → `[1]`.

### 4.22 `nl2br`

- Registration: `new TwigFilter('nl2br', [self::class, 'nl2br'], ['pre_escape' => 'html', 'is_safe' => ['html']])`.
  Source `CoreExtension.php:1233-1236`: `return nl2br($string ?? '');`
- PHP `nl2br($s, true)` (`PHP/ext/standard/string.c:4876-4958`): before each line break insert `<br />`; a line break is
  `\r\n`, `\n\r`, `\r` or `\n` (two-char pairs count once and are both copied after the `<br />`).
- `pre_escape: html`: the input is html-escaped first unless it is already safe for html by *compile-time* analysis
  (§1.9: string literals and `raw`/safe-filter outputs are not escaped) — this happens even when autoescape is off
  [ORACLE: env `autoescape=false`: `{{ "<a>\n"|nl2br }}` → `<a><br />` (literal is safe), but
  `{% set x = s|nl2br %}{{ x }}` with `s="<a>\n"` → `&lt;a&gt;<br />`]. A `Markup` *variable* is escaped at runtime
  only if not safe for html (autoescape path of `escape`). Output is safe for html.
- [ORACLE] `"<a>\nb"` → `&lt;a&gt;<br />\nb`; `|raw|nl2br` → `<a><br />\nb`; `"a\nb\r\nc\rd\n\re"` →
  `a<br />\nb<br />\r\nc<br />\rd<br />\n\re`; `"a\r\nb\n\rc\r\rd"` (php) → `a<br />\r\nb<br />\n\rc<br />\r<br />\rd`.

### 4.23 `number_format`

- Registration: `new TwigFilter('number_format', [$this, 'formatNumber'])`. Signature
  `n|number_format(decimal = null, decimal_point = null, thousand_sep = null)`. Source `CoreExtension.php:687-703`.

```
formatNumber(number, decimal = null, decimalPoint = null, thousandSep = null):
    defaults = this.numberFormat                         # [0, '.', ','] ; setNumberFormat($decimal, $decimalPoint, $thousandSep)
    decimal ??= defaults[0]; decimalPoint ??= defaults[1]; thousandSep ??= defaults[2]
    return number_format((float) number, decimal, decimalPoint, thousandSep)
```

- `(float)` cast never fails (`"abc"`→0, `"12abc"`→12, `"1e3"`→1000, `[]`→0, `[1]`→1, object → 1 + warning
  `Object of class S could not be converted to float`).
- PHP `number_format(float, int $decimals, ?string, ?string)` — algorithm §12.10. `decimal` param is `int`: `"2"` OK,
  `2.7` → 2 + deprecation `Implicit conversion from float 2.7 to int loses precision`, `"x"` →
  `number_format(): Argument #2 ($decimals) must be of type int, string given` (wrapped).
- [ORACLE] `200.35` → `200`; `9800.333|number_format(2, ".", ",")` → `9,800.33`; `1234567.891|number_format(2, ",", ".")` →
  `1.234.567,89`; `0.5`→`1`, `1.5`→`2`, `2.5`→`3`; `(-0.4)` → `0` (no `-0`); `(-1234.567)|number_format(1)` → `-1,234.6`;
  `1234|number_format(2,"dec","thou")` → `1thou234dec00`; `1234.5|number_format(-1)` → `1,230`;
  `1234.5|number_format(2, "", "")` → `123450`; `1e20` → `100,000,000,000,000,000,000`; `1e15|number_format(2)` →
  `1,000,000,000,000,000.00`; `NAN` → `nan`; `-INF` → `inf` (sign lost); `1.005|number_format(2)` → `1.01`.
- `{{ -9800.333|number_format(2) }}` parses as `-(9800.333|number_format(2))` → string `"9,800.33"` negated →
  `-9` + warning `A non-numeric value encountered` + Twig 3.21 precedence deprecation [ORACLE for `-1|date`].

### 4.24 `raw`

See §3.1.

### 4.25 `reduce`

- Registration: `new TwigFilter('reduce', [self::class, 'reduce'], ['needs_environment' => true, 'needs_is_sandboxed' => true])`.
  Signature `seq|reduce(arrow, initial = null)`. Source `CoreExtension.php:2128-2142`.

```
reduce(env, isSandboxed, array, arrow, initial = null):
    if !is_iterable(array): throw RuntimeError(sprintf('The "reduce" filter expects a sequence or a mapping, got "%s".', get_debug_type(array)))
    checkArrow(isSandboxed, arrow, 'reduce', 'filter')
    acc = initial
    for (key, value) in array: acc = arrow(acc, value, key)
    return acc
```

[ORACLE] `[1,2,3]|reduce((carry, v, k) => carry + v * k)` → `8`; with initial `10` and `(c, v) => c + v` → `16`; `[]|reduce(...)` → null.

### 4.26 `replace`

- Registration: `new TwigFilter('replace', [self::class, 'replace'])`. Signature `s|replace(from)`. Source `CoreExtension.php:640-647`.

```
replace(str, from):
    if !is_iterable(from): throw RuntimeError(sprintf('The "replace" filter expects a sequence or a mapping, got "%s".', get_debug_type(from)))
    return strtr(str ?? '', toArray(from))           # array form of strtr (§12.5)
```

- `strtr($str, array)`: longest matching key first at each position; replaced text is not re-scanned; integer keys
  are used as their decimal strings (so a *sequence* replaces `"0"`, `"1"`…); empty-string key ignored with warning
  `strtr(): Ignoring replacement of empty string`; replacement values converted to string (arrays → `Array` + warning).
  `$str` coerced to string (`TypeError` for arrays).
- [ORACLE] `"I like %this% and %that%."|replace({"%this%": "apples", "%that%": "oranges"})` → `I like apples and oranges.`;
  `"abcabc"|replace({"a":"1","ab":"2","abc":"3"})` → `33`; `"0123"|replace(["x","y"])` → `xy23`;
  `"abc"|replace({"a":1.5,"b":true,"c":null})` → `1.51`; `"abc"|replace("a")` → `The "replace" filter expects a sequence or a mapping, got "string".`

### 4.27 `reverse`

- Registration: `new TwigFilter('reverse', [self::class, 'reverse'], ['needs_charset' => true])`. Signature
  `v|reverse(preserve_keys = false)`. Source `CoreExtension.php:982-1005`.

```
reverse(charset, item, preserveKeys = false):
    if item is Traversable: return array_reverse(iterator_to_array(item), preserveKeys)    # NOTE: iterator_to_array preserves keys first
    if is_array(item): return array_reverse(item, preserveKeys)
    string = (string) item                         # Stringable OK; arrays impossible here; non-Stringable object -> Error
    if charset !== 'UTF-8': string = convertEncoding(string, 'UTF-8', charset)
    string = implode('', array_reverse(splitIntoCharacters(string, 'reverse')))
    if charset !== 'UTF-8': string = convertEncoding(string, charset, 'UTF-8')
    return string
```

- `array_reverse($a, $preserve)`: string keys are always preserved; integer keys preserved only if `$preserve`, else renumbered.
- [ORACLE] `{1:"a",2:"b",3:"c"}|reverse` → `0:c 1:b 2:a`; `|reverse(true)` → `3:c 2:b 1:a`; `{x:"a",2:"b",y:"c"}|reverse` →
  `y:c 0:b x:a`; `"été"|reverse` → `été`; `123|reverse` → `321`; `null|reverse` → ``; generator with duplicate keys
  `a,b,a` → `{"b":2,"a":3}`; invalid UTF-8 → `Unable to split the string passed to "reverse" into characters: Malformed UTF-8 characters, possibly incorrectly encoded.`

### 4.28 `round`

- Registration: `new TwigFilter('round', [self::class, 'round'])`. Signature `n|round(precision = 0, method = 'common')`.
  Source `CoreExtension.php:660-673`.

```
round(value, precision = 0, method = 'common'):
    value = (float) value
    if method === 'common': return round(value, precision)          # PHP round, HALF_UP, §12.9
    if method !== 'ceil' and method !== 'floor':
        throw RuntimeError('The "round" filter only supports the "common", "ceil", and "floor" methods.')
    return method(value * 10 ** precision) / 10 ** precision         # PHP ceil()/floor() of the scaled float, then divide
```

- Always returns a float (prints via precision-14: `3.0`→`3`, `1e20`→`1.0E+20`, `(-0.4)|round` → `-0`).
- `precision` for `common` goes to `round(int $precision)`: `2.9` → deprecation + 2; `"2"` OK; `"abc"` →
  `round(): Argument #2 ($precision) must be of type int, string given`. For ceil/floor `10 ** precision` is PHP `**`
  (int ** int → int if it fits, negative exponent → float).
- ceil/floor operate on the binary product: `1.005|round(2, "ceil")` → `1.01` (100.49999999999999 → 101);
  `1.1|round(1, "floor")` → `1.1`.
- [ORACLE] `42.55|round` → `43`; `42.55|round(1,"floor")` → `42.5`; `42.55|round(1,"ceil")` → `42.6`; `2.5`→`3`; `-2.5`→`-3`;
  `1.955|round(2)` → `1.96`; `5.045|round(2)` → `5.05`; `5.055|round(2)` → `5.06`; `1234.5|round(-2)` → `1200`;
  `0.285|round(2)` → `0.29`; `1.45|round(1)` → `1.5`; `"3.7"`→`4`; `null`→`0`; `"abc"`→`0`; `true`→`1`; `[1]`→`1`;
  `(-3.21)|round(1,"ceil")` → `-3.2`, `"floor"` → `-3.3`; `3.21|round(-1,"ceil")` → `10`;
  `3.5|round(0,"up")` → `The "round" filter only supports the "common", "ceil", and "floor" methods.`
- Note the `//` operator compiles to `(int) floor(a / b)` (int result), unlike `round(0,'floor')` (float) [SRC `FloorDivBinary`].

### 4.29 `shuffle`

- Registration: `new TwigFilter('shuffle', [self::class, 'shuffle'], ['needs_charset' => true])`. Source `CoreExtension.php:1015-1039`.

```
shuffle(charset, item):
    if is_string(item):
        if charset !== 'UTF-8': item = convertEncoding(item, 'UTF-8', charset)
        chars = splitIntoCharacters(item, 'shuffle'); shuffle(chars); item = implode('', chars)
        if charset !== 'UTF-8': item = convertEncoding(item, charset, 'UTF-8')
        return item
    if is_iterable(item): item = toArray(item, false); shuffle(item)      # keys NOT preserved
    return item                                                         # other types unchanged
```

- PHP `shuffle` = Fisher–Yates with the MT19937 engine (§12.15). Output is random; `""|shuffle` → `` (splits to `['']`).
- [ORACLE] `[1]|shuffle` → `[1]`; `{a:1}|shuffle` → `[1]`; `5|shuffle` → `5`; `null|shuffle` → ``; invalid UTF-8 string →
  `Unable to split the string passed to "shuffle" into characters: …`.

### 4.30 `slice`

- Registration: `new TwigFilter('slice', [self::class, 'slice'], ['needs_charset' => true])`. Signature
  `v|slice(start, length = null, preserve_keys = false)`; also the `v[start:length]` syntax. Source `CoreExtension.php:761-784`.

```
slice(charset, item, start, length = null, preserveKeys = false):
    if item is Traversable:
        while item is IteratorAggregate: item = item.getIterator()
        if start >= 0 and length >= 0 and item is Iterator:        # NOTE: null >= 0 is true in PHP
            try: return iterator_to_array(new LimitIterator(item, start, length ?? -1), preserveKeys)
            catch OutOfBoundsException: return []                  # start beyond the end
        item = iterator_to_array(item, preserveKeys)
    if is_array(item): return array_slice(item, start, length, preserveKeys)
    return mb_substr((string) item, start, length, charset)
```

- `array_slice` (int `offset`, `?int length`, bool `preserve_keys`): negative offset counts from the end; negative length
  stops that many from the end; string keys always preserved, integer keys renumbered unless `preserve_keys`.
- `mb_substr` (§12.12) for strings, also for scalars cast to string (`12345|slice(1,2)` → `23`), `null` → `''`.
  `start`/`length` are `int` params (`"1"` OK, `1.7` → 1 + deprecation, `"x"` → `mb_substr(): Argument #2 ($start) must be of type int, string given`,
  `null` start → deprecation `mb_substr(): Passing null to parameter #2 ($start) of type int is deprecated` → 0).
- [ORACLE] `[1,2,3,4,5]|slice(1,2)` → `[2,3]`; `…|slice(1,2,true)` → `{"1":2,"2":3}`; `{a:1,b:2,3:3,4:4}|slice(1)` →
  `{"b":2,"0":3,"1":4}`; `|slice(1,null,true)` → `{"b":2,"3":3,"4":4}`; `[1,2,3]|slice(-2)` → `[2,3]`; `slice(5)` → `[]`;
  `"12345"[1:2]` → `23`, `[:2]` → `12`, `[2:]` → `345`, `[:-2]` → `123`; `"abcdef"|slice(-3,-1)` → `de`;
  `"abc"|slice(-10,2)` → `ab`; generator `slice(1,2)` → `[2,3]`, `slice(-2)` → `[4,5]`, `slice(10,2)` → `[]`,
  `slice(1,-1)` → `[2]` (materialised); IteratorAggregate `{x:1,y:2}|slice(1,1,true)` → `{"y":2}`.

### 4.31 `sort`

- Registration: `new TwigFilter('sort', [self::class, 'sort'], ['needs_environment' => true, 'needs_is_sandboxed' => true])`.
  Signature `seq|sort(arrow = null)`. Source `CoreExtension.php:1049-1066`.

```
sort(env, isSandboxed, array, arrow = null):
    if array is Traversable: array = iterator_to_array(array)          # keys preserved
    elif !is_array(array): throw RuntimeError(sprintf('The "sort" filter expects a sequence or a mapping, got "%s".', get_debug_type(array)))
    if arrow !== null:
        checkArrow(isSandboxed, arrow, 'sort', 'filter')
        uasort(array, arrow)              # keys preserved, user comparator
    else:
        asort(array)                      # keys preserved, PHP standard comparison (SORT_REGULAR)
    return array
```

- Both sorts are **stable** (original order as tie-breaker) and use `zend_sort` (hybrid insertion sort / quicksort, §12.11).
  For inconsistent comparisons (PHP 8 mixed-type `<=>` is not transitive) the exact algorithm matters.
- `asort` comparator = `zend_compare` (PHP 8 semantics: numeric strings compare numerically, `"abc" <=> 0` compares as
  strings, `null <=> x` compares as bool, …) — see the expressions/operators reference for `zend_compare`.
- `uasort` comparator: the arrow's return value converted with `zval_get_long` (**floats truncated toward zero**: a
  comparator `a - b` returning `0.3` counts as equal [ORACLE `[3.5,3.2,3.8]|sort((a,b) => a - b)` → unchanged order]);
  a bool return triggers deprecation `uasort(): Returning bool from comparison function is deprecated, return an integer less than, equal to, or greater than zero`
  (once per sort) and `false` is retried with swapped operands (`array.c:837-872`).
- [ORACLE] `[3,1,2]|sort` → `{"1":1,"2":2,"0":3}` (keys kept); `{b:2,a:3,c:1}|sort` → `{"c":1,"b":2,"a":3}`;
  `["b","a","B","10","9","1e1"]|sort` → `{"4":"9","3":"10","5":"1e1","2":"B","1":"a","0":"b"}`;
  `["10",9,"9a",1.5,"abc",true,null]|sort` → `{"6":null,"3":1.5,"1":9,"0":"10","2":"9a","4":"abc","5":true}`;
  arrow `(a, b) => a.q <=> b.q` sorts; non-Closure callable `"strcmp"` works with deprecation
  `Passing a callable that is not a PHP \Closure as an argument to the "sort" filter is deprecated.`

### 4.32 `spaceless` (deprecated since 3.12)

- Registration: `new TwigFilter('spaceless', [self::class, 'spaceless'], ['pre_escape' => 'html', 'is_safe' => ['html'], 'deprecation_info' => new DeprecatedCallableInfo('twig/twig', '3.12')])`.
- Source `CoreExtension.php:1245-1248`: `return trim(preg_replace('/>\s+</', '><', $content ?? ''));`
  (`\s` = PCRE whitespace bytes `\t \n \v \f \r` and space, no `/u`; `trim` default mask `" \t\n\r\0\x0B"`).
- Using it triggers at compile time: `Since twig/twig 3.12: Twig Filter "spaceless" is deprecated in <path-or-name> at line <n>.`
- [ORACLE] `"  <div>\n  <b> x </b>  </div>  "|spaceless` → `<div><b> x </b></div>`.

### 4.33 `split`

- Registration: `new TwigFilter('split', [self::class, 'split'], ['needs_charset' => true])`. Signature
  `s|split(delimiter, limit = null)`. Source `CoreExtension.php:882-905`.

```
split(charset, value, delimiter, limit = null):
    value = value ?? ''
    if '' !== delimiter:                                  # strict: null, 0, false are "non-empty" here
        return limit === null ? explode(delimiter, value) : explode(delimiter, value, limit)
    if limit <= 1: return splitIntoCharacters(value, 'split')      # null <= 1 is true
    length = mb_strlen(value, charset)
    if length < limit: return [value]
    r = []
    for (i = 0; i < length; i += limit): r[] = mb_substr(value, i, limit, charset)
    return r
```

- `explode` (§12.6): limit > 0 → at most `limit` parts (last holds the rest); limit < 0 → all but the last `-limit`;
  limit 0 → treated as 1; empty input with limit >= 0 → `['']`. `delimiter = null` → deprecation
  `explode(): Passing null to parameter #1 ($separator) of type string is deprecated` then ValueError
  `explode(): Argument #1 ($separator) must not be empty` (wrapped). `limit = "x"` → `explode(): Argument #3 ($limit) must be of type int, string given`.
- Non-string `value` coerced (`12345|split(3)` → `["12","45"]`).
- [ORACLE] `"one,two,three"|split(",")` → `[one,two,three]`; `split(",", 2)` → `[one, "two,three,four"]`; `split(",", -1)` →
  `[one,two,three]`; `"one,two"|split(",", 0)` → `["one,two"]`; `"123"|split("")` → `[1,2,3]`; `"aabbcc"|split("", 2)` →
  `[aa,bb,cc]`; `"aabbc"|split("",2)` → `[aa,bb,c]`; `"ab"|split("",5)` → `[ab]`; `"éàü"|split("")` → 3 chars;
  `""|split("")` → `[""]` (length 1); `"abc"|split("", -1)` → chars; `null|split(",")` → `[""]`; invalid UTF-8 with `""` →
  `Unable to split the string passed to "split" into characters: …`.

### 4.34 `striptags`

- Registration: `new TwigFilter('striptags', [self::class, 'striptags'])`. Signature `s|striptags(allowable_tags = null)`.
- Source `CoreExtension.php:1346-1349`: `return strip_tags($string ?? '', $allowable_tags);`
- PHP `strip_tags(string, array|string|null)` state machine: §12.4. Despite the doc ("replaces adjacent whitespace characters
  by one space") **no whitespace is collapsed** [ORACLE `"a  \n <p>b</p>"` → `a  \n b`].
- Allowed tags: string `"<b><br>"` or array `["b","br"]` (each element `<name>`-wrapped).
- [ORACLE] `"<p>Hello <b>World</b></p><br/><!-- c --> &amp; <?php x ?> a < b"` → `Hello World &amp;  a < b`;
  `striptags("<b><br>")` keeps `<b>…</b>` and `<br/>`.

### 4.35 `title`

- Registration: `new TwigFilter('title', [self::class, 'titleCase'], ['needs_charset' => true])`.
- Source `CoreExtension.php:1358-1361`: `return mb_convert_case($string ?? '', \MB_CASE_TITLE, $charset);` (§12.12 title algorithm).
- [ORACLE] `"hello wORLD o'neil mary-jane ǆemal 3rd x_y a.b ÉCOLE ﬁsh"` →
  `Hello World O'neil Mary-Jane ǅemal 3Rd X_Y A.b École Fish`; `"hello 2nd world_wide-web o'reilly ŉ"` →
  `Hello 2Nd World_Wide-Web O'reilly ʼN`; `"ß"|title` → `Ss`; `"ǳ"|title` → `ǲ`; whitespace kept as is.

### 4.36 `trim`

- Registration: `new TwigFilter('trim', [self::class, 'trim'])`. Signature `s|trim(character_mask = null, side = 'both')`.
  Source `CoreExtension.php:1209-1224`.

```
trim(string, characterMask = null, side = 'both'):         # returns string|Stringable
    if characterMask === null: characterMask = " \t\n\r\0\x0B"      # DEFAULT_TRIM_CHARS (no \f)
    trimmed = match side:
        'both'  -> trim(string ?? '', characterMask)
        'left'  -> ltrim(string ?? '', characterMask)
        'right' -> rtrim(string ?? '', characterMask)
        default -> throw RuntimeError('Trimming side must be "left", "right" or "both".')
    return (string is Markup and characterMask === DEFAULT_TRIM_CHARS) ? string.withContent(trimmed) : trimmed
```

- Character mask supports `a..z` ranges (php_charmask, §12.7); invalid ranges emit warnings.
- A `Markup` input trimmed with the default mask stays a `Markup` (keeps its safety); with any other mask it becomes a plain
  (unsafe) string [ORACLE `m|trim` → `<b>x</b>` raw; `m|trim("<")` → escaped].
- [ORACLE] `"  a  "|trim` → `a`; `"xxaxx"|trim("x")` → `a`; `trim(side="left")` → `a  `; `trim(" ", "right")` → `  a`;
  `"abcxcba"|trim("a..c")` → `x`; `12.50|trim("0")` → `12.5` (float printed `12.5` first); `"\x0Ca"` keeps `\f`.

### 4.37 `upper`

- Registration: `new TwigFilter('upper', [self::class, 'upper'], ['needs_charset' => true])`. Source `CoreExtension.php:1321-1324`:
  `return mb_strtoupper($string ?? '', $charset);` (full mapping, §12.12).
- [ORACLE] `"straße ǆ ﬁ éa"` → `STRASSE Ǆ FI ÉA`; `1.5|upper` → `1.5`; `true|upper` → `1`; `null|upper` → ``;
  `[1]|upper` → `mb_strtoupper(): Argument #1 ($string) must be of type string, array given`; non-Stringable object →
  `… must be of type string, NS given`; Stringable → its string upper-cased; `"a\xffb"` → `A?B`.

### 4.38 `url_encode`

- Registration: `new TwigFilter('url_encode', [self::class, 'urlencode'])`. Source `CoreExtension.php:712-719`.

```
urlencode(url):
    if is_array(url): return http_build_query(url, '', '&', PHP_QUERY_RFC3986)    # §12.3
    return rawurlencode(url ?? '')                                               # §3.7; Traversable/objects -> must be Stringable
```

- [ORACLE] `"path-seg*ment ~é"` → `path-seg%2Ament%20~%C3%A9`; `{name:"Fabien","a b":"c&d",arr:[1,2],n:null,t:true,f:false,nested:{x:"y"}}`
  → `name=Fabien&a%20b=c%26d&arr%5B0%5D=1&arr%5B1%5D=2&t=1&f=0&nested%5Bx%5D=y` (null skipped);
  `{a: 0.1+0.2, b: 1e25, c: 1.0, d: -0.0}` → `a=0.3&b=1.0E%2B25&c=1&d=-0` (precision-14 floats);
  `[5,6]` → `0=5&1=6`; `true` → `1`; `false` → ``; `"-_.~!*'();:@&=+$,/?#[]"` → `-_.~%21%2A%27%28%29%3B%3A%40%26%3D%2B%24%2C%2F%3F%23%5B%5D`.

### 4.39 Escape-related filters

`escape`, `e`, `raw` — §3.

### 4.40 `checkArrow` (shared by `filter`, `find`, `map`, `reduce`, `sort`, `has some`, `has every`)

`CoreExtension::checkArrow(bool $isSandboxed, $arrow, $thing, $type)` (`CoreExtension.php:2191-2202`):

```
if arrow is Closure: return
if isSandboxed: throw RuntimeError(sprintf('The callable passed to the "%s" %s must be a Closure in sandbox mode.', thing, type))
trigger_deprecation('twig/twig', '3.15', 'Passing a callable that is not a PHP \Closure as an argument to the "%s" %s is deprecated.', thing, type)
# then the callable (e.g. a function-name string) IS called
```

`type` is `filter` for filters and `operator` for `has some`/`has every`. [ORACLE]
`The callable passed to the "map" filter must be a Closure in sandbox mode.`;
`The callable passed to the "has some" operator must be a Closure in sandbox mode.`

---

## 5. Functions

Core functions are registered in `CoreExtension::getFunctions()` (`TWIG/src/Extension/CoreExtension.php:293-312`);
`dump`, `template_from_string`, `render_sandboxed` come from optional extensions (§10, §11, §9.8).

### 5.1 `attribute`

- Registration: `new TwigFunction('attribute', null, ['parser_callable' => [self::class, 'parseAttributeFunction']])`.
- Parse time (`CoreExtension.php:2257-2272`): arguments are bound against the fake signature
  `fn ($variable, $attribute, $arguments = null)` (named args allowed; extra positional args silently dropped [ORACLE]),
  then replaced by `new GetAttrExpression($args[0], $args[1], $args[2] ?? null, Template::ANY_CALL, $line)` — i.e. exactly
  the `.` operator with a dynamic name (§8). `attribute(o, "name", ["x"])` passes method arguments.
- `attribute(o, x) is defined` works (GetAttr supports the defined test). Missing argument →
  `Value for argument "attribute" is required for function "attribute".` [ORACLE].
- Deprecated in docs since 3.15 (use `o.(expr)`) but no runtime deprecation (commented out, `CoreExtension.php:2262-2269`).
- Sandbox: the node keeps `sandboxed_function_name = 'attribute'`; non-strict policies allow it with a 3.27 deprecation (§9.2).
- [ORACLE] `attribute(o,"p")` → public prop; `attribute(o,"bar")` → `getBar()`; `"baz"` → `isBaz()`; `"qux"` → `hasQux()`;
  `attribute(o,"name",["x"])` → `name:x`; `attribute(a,0)` on arrays; `attribute(o,"FOO")` → class constant.

### 5.2 `block`

- Registration: `new TwigFunction('block', null, ['parser_callable' => [self::class, 'parseBlockFunction']])`.
- Parse time (`CoreExtension.php:2246-2252`): bound against `fn ($name, $template = null)`; produces
  `BlockReferenceExpression($args[0], $args[1] ?? null, $line)` (`TWIG/src/Node/Expression/BlockReferenceExpression.php`).
- Runtime (compile output):
  - `block(name)` → `$this->unwrap()->renderBlock(name, $context, $blocks)`;
  - `block(name, template)` → `$this->load(template, line)->unwrap()->renderBlock(name, $context)` (template may be a name,
    a `TemplateWrapper`, or an array of names tried in order — `Environment::resolveTemplate`);
  - `block(...) is defined` → `hasBlock(name, $context[, $blocks])` (no rendering, no error).
- Result is a plain **string**, but compile-time safe (`['all']`): `{{ block('x') }}` is not escaped, while
  `{% set b = block('x') %}{{ b }}` **is** escaped [ORACLE].
- `Template::renderBlock` / `yieldBlock` (`TWIG/src/Template.php:247-277, 487-522`) errors (raw messages):
  - `Block "%s" on template "%s" does not exist.` (template = name of the template asked)
  - `Block "%s" should not call parent() in "%s" as the block does not exist in the parent template "%s".`
  - `A block must be a method on a \Twig\Template instance.` (\LogicException)
  [ORACLE] `{{ block("nope") }}` → `Block "nope" on template "index" does not exist in "index" at line 1.`;
  `block("nope", "blocks")` → `Block "nope" on template "blocks" does not exist in "blocks".`;
  missing template → `Template "nope" is not defined in "index" at line 1.` (LoaderError).
- Sandbox: `sandboxed_function_name = 'block'` (non-strict: allowed with 3.27 deprecation).

### 5.3 `constant`

- Registration: `new TwigFunction('constant', [self::class, 'constant'])`. Signature `constant(constant, object = null)`.
  Source `CoreExtension.php:1681-1704`.

```
constant(constant, object = null, checkDefined = false):
    if object !== null:
        if constant === 'class': return checkDefined ? true : object::class        # object must be an object (PHP Error otherwise)
        constant = object::class . '::' . constant
    if !defined(constant):
        if checkDefined: return false
        if strtolower(substr(constant, -7)) === '::class':
            throw RuntimeError(sprintf('You cannot use the Twig function "constant" to access "%s". You could provide an object and call constant("class", $object) or use the class name directly as a string.', constant))
        throw RuntimeError(sprintf('Constant "%s" is undefined.', constant))
    return checkDefined ? true : constant(constant)
```

- `constant(...) is defined` → `FunctionExpression::enableDefinedTest` (only for `constant`) adds the argument
  `checkDefined = true` (`FunctionExpression.php:51-56, 76-78`) → returns bool.
- Class constants include enum cases (`constant("Suit::Hearts")` → the enum object).
- [ORACLE] `constant("MYCONST")`, `constant("C::FOO")`, `constant("FOO", o)`, `constant("class", o)` → `C`,
  `constant("PHP_INT_MAX")` → `9223372036854775807`, `constant("M_PI")` → `3.1415926535898` (precision 14),
  `constant("NOPE")` → `Constant "NOPE" is undefined.`, `constant("C::class")`/`constant("C::CLASS")` → the "You cannot use…" message,
  `constant("MYCONST", null)` → global constant.
- The port needs a registry of "PHP constants" (global + class constants) — at least `PHP_EOL`, `PHP_INT_MAX`,
  `PHP_INT_MIN`, `PHP_INT_SIZE`, `PHP_FLOAT_*`, `M_*`, `E_*`, `JSON_*`, `ENT_*`, `DATE_*` / `DateTimeInterface::*` are
  commonly used from templates (UNVERIFIED which subset a port should ship).

### 5.4 `cycle`

- Registration: `new TwigFunction('cycle', [self::class, 'cycle'])`. Signature `cycle(values, position)`. Source `CoreExtension.php:421-443`.

```
cycle(values, position):
    if !is_array(values):
        if values is not ArrayAccess: throw RuntimeError('The "cycle" function expects an array or "ArrayAccess" as first argument.')
        if !is_countable(values):
            trigger_deprecation('twig/twig', '3.12', 'Passing a non-countable sequence of values to "%s()" is deprecated.', 'Twig\Extension\CoreExtension::cycle')
            values = toArray(values, false)
    if !(count = count(values)): throw RuntimeError('The "cycle" function expects a non-empty sequence.')
    return values[position % count]                # PHP % : int conversion of both sides, sign of dividend
```

- `values[...]` is a raw PHP array read: mappings with non-0..n-1 keys yield `null` + warning `Undefined array key 0`;
  negative positions → negative remainder → `Undefined array key -1` → null [ORACLE]. Float positions → deprecation +
  truncation; `"1"` OK; `"x"` → `Unsupported operand types: string % int`.
- [ORACLE] `cycle(["odd","even"], 5)` → `even`; `cycle({5:"x",1:"y"}, 1)` → `y`; `cycle(ArrayObject(['p','q']), 3)` → `q`;
  `cycle("abc", 0)` → `The "cycle" function expects an array or "ArrayAccess" as first argument.`;
  `cycle([], 0)` → `The "cycle" function expects a non-empty sequence.`; `cycle([1])` → compile-time
  `Value for argument "position" is required for function "cycle".`

### 5.5 `date` (function) — `convertDate`

- Registration: `new TwigFunction('date', [$this, 'convertDate'])`. Signature `date(date = null, timezone = null)`.
  Source `CoreExtension.php:585-630`. Also used by the `date` and `date_modify` filters.

```
convertDate(date = null, timezone = null):
    if timezone !== false:
        if timezone === null: timezone = this.getTimezone()           # Twig default tz (setTimezone) or PHP date_default_timezone_get()
        elif timezone is not DateTimeZone: timezone = new DateTimeZone(timezone)     # string coerced; bad -> 'DateTimeZone::__construct(): Unknown or bad timezone (X)'
    if date is DateTimeImmutable: return timezone !== false ? date.setTimezone(timezone) : date
    if date is DateTime:
        date = clone date
        if timezone !== false: date.setTimezone(timezone)
        return date
    if date === null or date === 'now':
        return new DateTime('now', timezone !== false ? timezone : this.getTimezone())
    asString = (string) date                    # arrays: "Array" + warning, then DateTime ctor TypeError (see below)
    if ctype_digit(asString) or (asString !== '' and asString[0] === '-' and ctype_digit(substr(asString, 1))):
        date = new DateTime('@' . date)         # unix timestamp; resulting zone is the offset "+00:00"
    else:
        date = new DateTime(date)               # NOTE: the ORIGINAL value, parsed by timelib in PHP's default timezone
    if timezone !== false: date.setTimezone(timezone)
    return date
```

Port-critical consequences [ORACLE / fixture `TWIG/tests/Fixtures/filters/date_time_zone_conversion.test`]:
- Only all-digit strings/ints (optionally one leading `-`) are timestamps. Floats are *not*: `1700000000.5` → string
  `"1700000000.5"` → parsed by timelib as a date/time expression → `1699-11-30 00:05:00` [ORACLE]. `true` → `"1"` →
  timestamp 1; `false` → `""` → `new DateTime("")` = now; `""` → now.
- A timezone-less date string is interpreted in **PHP's default timezone** (`date_default_timezone_get()`), *not* the
  Twig timezone, then converted to the Twig timezone. With PHP default `UTC` and Twig tz `Asia/Tokyo`:
  `"2024-01-01 12:00"|date("Y-m-d H:i T")` → `2024-01-01 21:00 JST` [ORACLE].
- With `timezone = false`: a timestamp keeps zone `+00:00` (`T` → `GMT+0000`, `e` → `+00:00`); a plain string keeps PHP's
  default zone; a string with an explicit offset keeps that offset (`T` → `GMT+0500`, `e` → `+05:00`) [ORACLE].
- `'now'`/`null` are created directly in the target zone.
- Return value is a `DateTime`/`DateTimeImmutable` object usable in comparisons (`date(a) < date('-2days')` — PHP compares
  DateTime objects by instant) and attribute access (`date("2024-01-01").timestamp` → `getTimestamp()`;
  `.timezone`, `.date`, `.timezone_type` read the internal properties via the special-case in §8).
- Errors (wrapped): `Failed to parse time string (notadate) at position 0 (n): The timezone could not be found in the database`
  (`DateMalformedStringException`, PHP 8.3+), `DateTime::__construct(): Argument #1 ($datetime) must be of type string, array given`
  (preceded by an `Array to string conversion` warning from the `(string)` cast) [ORACLE].
- Date string grammar, relative formats and DST rules are timelib's (`PHP/ext/date/lib/parse_date.re`) — out of this
  document's scope.

### 5.6 `enum`

- Registration: `new TwigFunction('enum', [self::class, 'enum'], ['node_class' => EnumFunction::class])`. Signature `enum(enum)`.
- Compile time (`TWIG/src/Node/Expression/FunctionNode/EnumFunction.php:21-52`): if the call has exactly one argument
  (positional `0` or named `enum`) and it is a literal:
  - non-string literal → `SyntaxError('The first argument of the "enum" function must be a string.')`
  - not an existing enum → `SyntaxError(sprintf('The first argument of the "enum" function must be the name of an enum, "%s" given.', $value))`
  - enum without cases → `SyntaxError(sprintf('The first argument of the "enum" function must be a non-empty enum, "%s" given.', $value))`
  - else compiles to `\Enum::FirstCase` directly.
  Otherwise compiles to the runtime call.
- Runtime `CoreExtension::enum(string $enum): \UnitEnum` (`CoreExtension.php:1655-1666`):
  `if !enum_exists(enum): throw RuntimeError(sprintf('"%s" is not an enum.', enum))`;
  `if !(cases = enum::cases()): throw RuntimeError(sprintf('"%s" is an empty enum.', enum))`; `return cases[0]`.
- The returned case object is only a handle: `enum('X').Name` (constant lookup via §8 property step 3: class constants),
  `enum('X').cases()`/`.cases` (static method `cases()`), `.from('v')`, `.tryFrom`, custom static methods.
  Printing an enum case → `Object of class Suit could not be converted to string` [ORACLE].

### 5.7 `enum_cases`

- Registration: `new TwigFunction('enum_cases', [self::class, 'enumCases'], ['node_class' => EnumCasesFunction::class])`.
- Compile time (`EnumCasesFunction.php:21-48`): single literal argument → non-string:
  `The first argument of the "enum_cases" function must be a string.`; unknown:
  `The first argument of the "enum_cases" function must be the name of an enum, "%s" given.`; else compiles to `\Enum::cases()`.
- Runtime (`CoreExtension.php:1635-1642`): `if !enum_exists(enum): throw RuntimeError(sprintf('Enum "%s" does not exist.', enum)); return enum::cases();`
- [ORACLE] all messages above verified.

### 5.8 `include`

- Registration: `new TwigFunction('include', [self::class, 'include'], ['needs_environment' => true, 'needs_context' => true, 'is_safe' => ['all']])`.
  Signature `include(template, variables = [], with_context = true, ignore_missing = false, sandboxed = false)`.
  Source `CoreExtension.php:1541-1584`.

```
include(env, context, template, variables = [], withContext = true, ignoreMissing = false, sandboxed = false):
    if func_num_args() >= 7:              # i.e. the template passed the 5th Twig argument (positional or named, explicitly)
        trigger_deprecation('twig/twig', '3.29', sandboxed
            ? 'The "sandboxed" argument of the "include" function is deprecated, use the "render_sandboxed" function to render untrusted templates instead.'
            : 'The "sandboxed" argument of the "include" function is deprecated, remove the argument as "false" has no effect.')
    alreadySandboxed = false; sandbox = null
    if withContext: variables = array_merge(context, variables)    # variables must be an array: else TypeError 'array_merge(): Argument #2 must be of type array, string given'
    if (isSandboxed = sandboxed and env.hasExtension(SandboxExtension)):
        sandbox = env.getExtension(SandboxExtension).getChecker()
        if !(alreadySandboxed = sandbox.isSandboxed()): sandbox.setSandboxed(true)
    try:
        try: loaded = env.resolveTemplate(template)
        catch LoaderError e: if !ignoreMissing: throw e; return ''
        rendered = loaded.render(variables)
        return rendered === '' ? '' : new Markup(rendered, env.getCharset())
    finally:
        if isSandboxed and !alreadySandboxed: sandbox.setSandboxed(false)
```

- `Environment::resolveTemplate($names)` (`TWIG/src/Environment.php:516-548`): non-array → `load($names)` (string name or
  `TemplateWrapper`); array → for each: `TemplateWrapper` → use it; if the array has more than one element and
  `!loader.exists(name)` → skip; else `load(name)`. None found →
  `LoaderError(sprintf('Unable to find one of the following templates: "%s".', implode('", "', $names)))`.
  A one-element array that does not exist raises the loader's own error (`Template "nope" is not defined.`) [ORACLE].
- `with_context = false` → only `variables` (plus globals, re-added by the included template).
- `ignore_missing` only catches `LoaderError` from *resolving* the template — errors inside the included template propagate.
- Return value: `''` (plain string) for empty output, else `Markup` (safe for `all`): `{% set i = include('x') %}{{ i }}`
  is not re-escaped [ORACLE].
- `include(null)` → `Twig\Environment::getTemplateClass(): Argument #1 ($name) must be of type string, null given, called in …` (non-portable suffix).
- [ORACLE] `include("inc")` sees context; `include("inc", {b: 2}, false)` / `with_context = false` only `b`;
  `include("nope")` → `Template "nope" is not defined in "index" at line 1.`; `include("nope", ignore_missing = true)` → ``;
  `include(["nope","inc"])` → renders `inc`; `include(["nope","nope2"])` → `Unable to find one of the following templates: "nope", "nope2" in "index" at line 1.`

### 5.9 `include_only` (3.29+)

- Registration: `new TwigFunction('include_only', [self::class, 'includeOnly'], ['needs_environment' => true, 'is_safe' => ['all']])`.
  Signature `include_only(template, variables = [], ignore_missing = false)`.
- Source `CoreExtension.php:1597-1600`: `return self::include($env, [], $template, $variables, false, $ignoreMissing);`
  (no sandbox arg → no deprecation). `variables` is typed `array`: `include_only("inc", "x")` → PHP TypeError
  `Twig\Extension\CoreExtension::includeOnly(): Argument #3 ($variables) must be of type array, string given, called in …` (non-portable).
- Globals remain visible [ORACLE].

### 5.10 `max` / `min`

- Registration: `new TwigFunction('max', 'max')`, `new TwigFunction('min', 'min')` — PHP internals called directly.
- Semantics (`PHP/ext/standard/array.c:1197-1446`):
  - one argument: must be an array (`max(): Argument #1 ($value) must be of type array, int given`, also for
    `ArrayObject`/Traversable); empty array → `ValueError('max(): Argument #1 ($value) must contain at least one element')`;
    returns the first maximal/minimal element by `zend_compare` (`zend_hash_minmax`, keys ignored).
  - 2 arguments (compiled as PHP *frameless* calls): `max` returns `lhs` when `lhs >= rhs` else `rhs` (tie → **lhs**);
    `min` returns `lhs` when `lhs < rhs` else `rhs` (tie → **rhs**). Int/float fast paths compare numerically when the int is
    exactly representable as double, else generic `zend_compare`.
  - 3+ arguments: start with the first; replace only when strictly greater (max) / strictly smaller (min) (tie → first).
  - no argument → compile-time `Value for argument "value" is required for function "max".`
- [ORACLE] `max(1,3,2)` → `3`; `max([1,3,2])` → `3`; `max({2:"e",1:"a",3:"b"})` → `e`; `max("apple","banana")` → `banana`;
  `max([1,2],[1,3])` → `[1,3]`; `max("10", 9)` → `10`; `max("abc", 0)` → `abc`; `min("abc", 0)` → `0`; `min("a","A")` → `A`;
  ties: `min(null,false)` → `false`, `min(false,null)` → `null`, `min(0,"0")` → `"0"`, `max(0,"0")` → `0`,
  `max(null,false)` → `null`, `min(null,false,null)` → `null`, `min(0,"0",5)` → `0`, `min([0,"0"])` → `0`.

### 5.11 `parent`

- Registration: `new TwigFunction('parent', null, ['parser_callable' => [self::class, 'parseParentFunction']])`.
- Parse time (`CoreExtension.php:2230-2241`); arguments are **ignored** [ORACLE `parent(1, 2)` works]:
  - not inside a block → `SyntaxError('Calling the "parent" function outside of a block is forbidden.')`
  - template without `extends`/`use` → `SyntaxError('Calling the "parent" function on a template that does not call "extends" or "use" is forbidden.')`
  - else `ParentExpression(currentBlockName, line)` → `$this->renderParentBlock(name, $context, $blocks)` (string, compile-time safe `all`).
- Runtime (`Template::yieldParentBlock`, `Template.php:535-545`): traits (`use`) first, then the parent template; else
  `RuntimeError(sprintf('The template has no parent and no traits defining the "%s" block.', $name))`.
- Sandbox: `sandboxed_function_name = 'parent'` (non-strict: allowed with 3.27 deprecation).

### 5.12 `random`

- Registration: `new TwigFunction('random', [self::class, 'random'], ['needs_charset' => true])`. Signature
  `random(values = null, max = null)`. Source `CoreExtension.php:460-511`.

```
random(charset, values = null, max = null):
    if values === null: return max === null ? mt_rand() : mt_rand(0, (int) max)
    if is_int(values) or is_float(values):
        if max === null:
            if values < 0: max = 0; min = values
            else:          max = values; min = 0
        else: min = values
        return mt_rand((int) min, (int) max)         # min > max -> ValueError 'mt_rand(): Argument #2 ($max) must be greater than or equal to argument #1 ($min)'
    if is_string(values):
        if values === '': return ''
        if charset !== 'UTF-8': values = convertEncoding(values, 'UTF-8', charset)
        values = splitIntoCharacters(values, 'random')
        if charset !== 'UTF-8': for (i, v) in values: values[i] = convertEncoding(v, charset, 'UTF-8')
    if !is_iterable(values): return values            # e.g. true -> true (prints "1"), objects returned as-is
    values = toArray(values)                          # keys preserved
    if count(values) === 0: throw RuntimeError('The "random" function cannot pick from an empty sequence or mapping.')
    return values[array_rand(values, 1)]
```

- Not cryptographically secure; uses the per-request MT19937 state (§12.15) — deterministic only after `mt_srand(seed)`:
  [ORACLE] after `mt_srand(42)`: `random(100)` → `38`, then `random(["a","b","c"])` → `c`, then `random()` → `2041643438`.
- [ORACLE] `random(0)` → `0`; `random(5,5)` → `5`; `random(-3,-3)` → `-3`; `random("a")` → `a`; `random({a:9})` → `9`;
  `random("")` → ``; `random([])` → error above; `random(10,5)` → wrapped ValueError; `random(2.9, 2.9)` → `2`.

### 5.13 `range`

- Registration: `new TwigFunction('range', 'range')` — PHP internal `range(string|int|float $start, string|int|float $end, int|float $step = 1): array`.
  The `..` operator compiles to `range(left, right)` (§7.6).
- Named arguments use `low`, `high`, `step` (CallableArgumentsExtractor renames `start`/`end` only for the callable
  string `'range'`); `range(start=1, end=3)` → `Value for argument "low" is required for function "range".` [ORACLE].
- Full PHP 8.3+ algorithm: §12.13. [ORACLE] `range(0,3)` → `[0,1,2,3]`; `range(0,6,2)`; `range(3,0)` → `[3,2,1,0]`;
  `range("a","e",2)` → `[a,c,e]`; `range(0,1,0.25)` → `[0,0.25,0.5,0.75,1]`; `range(5,1,2)` → `[5,3,1]`;
  `range("1","3")` → `["1","2","3"]` (single-byte digit strings give *strings*); `range("10","8")` → `[10,9,8]` (ints);
  `range(" 1","3")` → `[1,2,3]`; `range("A","z",10)` → `["A","K","U","_","i","s"]`; `range("5","a")` → characters `5`…`a`;
  `range(1,2.5)` → `[1.0,2.0]`; `range(1,2,-1)` → `range(): Argument #3 ($step) must be greater than 0 for increasing ranges`;
  `range(1,3,0)` → `range(): Argument #3 ($step) cannot be 0`; `range(1,3,5)` →
  `range(): Argument #3 ($step) must be less than the range spanned by argument #1 ($start) and argument #2 ($end)`;
  `range("","c")` → `[0]` + 2 warnings; `range("aa","cc")` → `[a,b,c]` + 2 warnings; `range("a","c",1.5)` → `[0]` + warning;
  `range(null, 2)` → `[0,1,2]` + null deprecation.

### 5.14 `source`

- Registration: `new TwigFunction('source', [self::class, 'source'], ['needs_environment' => true, 'is_safe' => ['all']])`.
  Signature `source(name, ignore_missing = false)`. Source `CoreExtension.php:1610-1622`:

```
source(env, name, ignoreMissing = false):
    try: return env.getLoader().getSourceContext(name).getCode()     # raw template code, not rendered
    catch LoaderError e: if !ignoreMissing: throw e; return ''
```

- Output is **not escaped** (`is_safe: all`) [ORACLE `source("raw.txt")` → `Hello {{ name }} <b>`]. `name` reaches the loader's
  `string` parameter (TypeError for arrays/null under the loader's signature).
- Loader messages (raw): ArrayLoader `Template "%s" is not defined.`; ChainLoader `Template "%s" is not defined%s.` (with
  ` (<loader errors joined by ", ">)` when inner loaders failed); FilesystemLoader `Unable to find template "%s" (looked into: %s).`,
  `There are no registered paths for namespace "%s".`, `Malformed namespaced template name "%s" (expecting "@namespace/template_name").`,
  `A template name cannot contain NUL bytes.`, `Looks like you try to load a template outside configured directories (%s).`
  (`TWIG/src/Loader/*.php`).

### 5.15 `template_from_string`, `dump`, `render_sandboxed`

See §11, §10, §9.8.

---

## 6. Tests

Registered in `CoreExtension::getTests()` (`TWIG/src/Extension/CoreExtension.php:314-331`). Syntax: `x is name`,
`x is name(args)`, `x is not name` (= `not (x is name)`). Tests with `one_mandatory_argument` (`same as`, `divisible by`)
accept a bare argument expression parsed at precedence 100 (`x is divisible by 3`). Generic tests compile to
`callable(x, args...)` (`TestExpression`); most core tests use custom node classes that inline PHP code. All are
`always_allowed_in_sandbox` **except `constant`**.

| test | node class / callable | compiled PHP | string-coercion note |
|---|---|---|---|
| `constant` | `ConstantTest` | `(x === constant(ARG0))` or `(x === constant(get_class(ARG1)."::".ARG0))` | args checked |
| `defined` | `DefinedTest` | the node compiled in "defined" mode | — |
| `divisible by` | `DivisiblebyTest` | `(0 == x % ARG0)` | — |
| `empty` | `CoreExtension::testEmpty` | `CoreExtension::testEmpty(x)` | — |
| `even` | `EvenTest` | `(x % 2 == 0)` | — |
| `iterable` | `is_iterable` + `NonStringCoercingInputTest` | `is_iterable(x)` | — |
| `mapping` | `CoreExtension::testMapping` + `NonStringCoercingInputTest` | | — |
| `none`, `null` | `NullTest` | `(null === x)` | — |
| `odd` | `OddTest` | `(x % 2 != 0)` | — |
| `same as` | `SameasTest` | `(x === ARG0)` | — |
| `sequence` | `CoreExtension::testSequence` + `NonStringCoercingInputTest` | | — |
| `true` | `TrueTest` | `(($tmp = x) && $tmp instanceof Markup ? (string) $tmp : $tmp)` | — |

Arguments of node-class tests are read **positionally** (`getNode('0')`, `getNode('1')`): named arguments make compilation
fail with `An exception has been thrown during the compilation of a template ("Node "0" does not exist for Node "Twig\Node\Nodes".").`
[ORACLE `9 is divisible by(num=3)`, `1 is same as(value=1)`]. Extra arguments to node-class tests are ignored
[ORACLE `2 is even(2)` → true]; extra arguments to `is_iterable` → `is_iterable() expects exactly 1 argument, 2 given`.
There is **no `false` test** (`Unknown "false" test.`).

### 6.1 `constant`

- `TwigTest('constant', null, ['node_class' => ConstantTest::class])` — the only core test requiring sandbox allow-listing
  (non-strict policy: allowed with deprecation `The "constant" test is always allowed in sandboxes, but won't be in 4.0, …`, §9.2).
- `TWIG/src/Node/Expression/Test/ConstantTest.php:27-48`. PHP `constant()` throws `Error("Undefined constant C::NOPE")`
  → wrapped `An exception has been thrown during the rendering of a template ("Undefined constant C::NOPE")` [ORACLE].
  `get_class()` of a non-object → TypeError (wrapped). Strict identity: `'3' is constant('C::NUM')` (NUM=3) → false [ORACLE].

### 6.2 `defined`

- `TwigTest('defined', null, ['node_class' => DefinedTest::class, 'always_allowed_in_sandbox' => true])`.
- `DefinedTest::__construct` (`DefinedTest.php:37-58`): if the tested node does not implement `SupportDefinedTestInterface`
  → `SyntaxError('The "defined" test only works with simple variables.', $lineno)` [ORACLE `(1 + 1) is defined`,
  `a|upper is defined`]. Supported nodes and their compiled "defined" form:
  - variable (`NameExpression`/`ContextVariable`): `array_key_exists('name', $context)`; special names → `true`;
    `always_defined` vars → `true` (null values count as defined [ORACLE `n is defined` with `n=null` → true]);
  - attribute access (`GetAttrExpression`, all nested parts switched to ignore-strict, non-optimised): `CoreExtension::getAttribute(..., isDefinedTest=true, ignoreStrictCheck=true)` (§8);
  - `block(...)` → `hasBlock(...)`;
  - `constant(...)` → `constant(..., checkDefined=true)`;
  - any other function call → `enableDefinedTest` is a no-op, the *function result* is returned (truthy/falsy!)
    [ORACLE `range(1, 2) is defined` → true because `[1,2]` is truthy];
  - literals (`ConstantExpression`, `ArrayExpression`) → `true` [ORACLE `"a" is defined` prints `1`];
  - macro references (`_self.m`, imported macros) → whether the macro exists; `_self.m() is defined` (with parentheses) is
    deprecated since 3.29 (`IsExpressionParser.php:41-51`).
- With `strict_variables`, `undef is defined` never throws.

### 6.3 `divisible by`

- `TwigTest('divisible by', null, ['node_class' => DivisiblebyTest::class, 'one_mandatory_argument' => true, 'always_allowed_in_sandbox' => true])`.
- `(0 == x % y)`: PHP `%` converts both operands to int (1.8): `9.5 is divisible by(3)` → true + deprecation;
  `"9"` OK; `"abc"` → `Unsupported operand types: string % int`; `y = 0` → `Modulo by zero` [ORACLE].
  `0 == int` is a plain int comparison.

### 6.4 `empty`

- `TwigTest('empty', [self::class, 'testEmpty'], ['always_allowed_in_sandbox' => true])`. Source `CoreExtension.php:1464-1479`:

```
testEmpty(value):
    if value is Countable:    return 0 === count(value)
    if value is Traversable:  return !iterator_count(value)        # consumes generators / iterators
    if value is Stringable:   return '' === (string) value
    return '' === value or false === value or null === value or [] === value
```

[ORACLE] empty: `''`, `false`, `null`, `[]`, Countable(0), Stringable(''), empty IteratorAggregate, empty generator,
`Markup('')`, `ArrayObject([])`, undefined variable (non-strict). Not empty: `0`, `'0'`, `0.0`, `' '`, `[1]`, plain object.
`undef is empty` with strict_variables → `Variable "undef" does not exist.` (no defined-test magic, unlike `default`).

### 6.5 `even` / `odd`

- `TwigTest('even', null, ['node_class' => EvenTest::class, ...])` → `(x % 2 == 0)`; `odd` → `(x % 2 != 0)`.
- [ORACLE] `-3 is odd` → true; `"4" is even` → true; `2.5 is even` → true (+deprecation, int 2); `null is even` → true;
  `0 is odd` → false; `"abc" is even` / `[1] is even` → `Unsupported operand types: string % int` / `array % int`.

### 6.6 `iterable`

- `TwigTest('iterable', 'is_iterable', ['node_class' => NonStringCoercingInputTest::class, 'always_allowed_in_sandbox' => true])`.
  `is_iterable(x)`: true for arrays and `Traversable` objects only (not strings, not plain objects, not `Markup`) [ORACLE].
  (`NonStringCoercingInputTest` only affects sandbox `__toString` wrapping: the input is not wrapped.)

### 6.7 `mapping`

- `TwigTest('mapping', [self::class, 'testMapping'], ['node_class' => NonStringCoercingInputTest::class, 'always_allowed_in_sandbox' => true])`.
  Source `CoreExtension.php:1514-1525`:

```
testMapping(value):
    if value is ArrayObject: value = value.getArrayCopy()
    if value is Traversable: value = iterator_to_array(value)     # consumes generators
    return (is_array(value) and !array_is_list(value)) or is_object(value)
```

`array_is_list`: keys are exactly 0,1,2,… in order (`[]` is a list). [ORACLE] mapping: `{a:1}`, `{1:'a',2:'b'}`,
`{1:'a',0:'b'}`, `ArrayObject(['a'=>1])`, IteratorAggregate with string keys, **any non-Traversable object** (plain object,
Stringable, `Markup`); not mapping: lists, `[]`, `{}` (empty literal is an empty array → list), strings, ints, null.

### 6.8 `none` / `null`

- Both `TwigTest(..., null, ['node_class' => NullTest::class, 'always_allowed_in_sandbox' => true])` → `(null === x)`.
  Undefined variable in non-strict mode is `null` → true; with strict_variables → `Variable "undef" does not exist.` [ORACLE].

### 6.9 `same as`

- `TwigTest('same as', null, ['node_class' => SameasTest::class, 'one_mandatory_argument' => true, ...])` → `(x === y)`.
  PHP identity: same type and value; arrays: same key/value pairs in the same order with `===` values; objects: same instance.
  [ORACLE] `1 is same as("1")` → false; `[1] is same as([1])` → true; `1.0 is same as(1)` → false; `1 is same as 1` works.
  `1 is same as` (no arg) → `Unexpected token "end of print statement" of value "" in "index" at line 1.`

### 6.10 `sequence`

- `TwigTest('sequence', [self::class, 'testSequence'], ['node_class' => NonStringCoercingInputTest::class, ...])`.
  Source `CoreExtension.php:1491-1502`: same conversions as `mapping`, then `return is_array(value) and array_is_list(value);`.
  [ORACLE] `[1,2]`, `[]`, `{0:1,1:2}`, `ArrayObject([1,2])`, IteratorAggregate over a list → true; `{1:'a',2:'b'}` → false.

### 6.11 `true` (internal truthiness test)

- `TwigTest('true', null, ['node_class' => TrueTest::class, 'always_allowed_in_sandbox' => true])` — undocumented; exists so
  that `TrueTest::wrap()` can wrap operands of `and`/`or`/`xor`/`not`/`?:` (`TrueTest.php:23-36`). `wrap($node)` returns the node
  unchanged if it implements `ReturnPrimitiveTypeInterface` (bool/number/string/array-returning nodes, constants).
- `x is true` **returns the value itself** (or the Markup's string), not a boolean: [ORACLE] `[{{ 1 is true }}]` → `[1]`,
  `[{{ 0 is true }}]` → `[0]`, `[{{ "a" is true }}]` → `[a]`, `Markup('')` → `[]`, `Markup('x')` → `[x]`, `null` → `[]`;
  `0 is not true` → true. Port it as "truthiness-normalising identity" (`Markup → string`, else unchanged).

---

## 7. Operator callables

Binary operators whose compiled form calls Twig runtime helpers or PHP functions (registered in
`CoreExtension::getExpressionParsers`, `CoreExtension.php:340-409`). Precedences are listed for reference; parsing is
covered elsewhere.

| operator | prec. | node | compiled form |
|---|---|---|---|
| `in` | 20 | `InBinary` | `CoreExtension::inFilter(left, right)` |
| `not in` | 20 | `NotInBinary` | `!CoreExtension::inFilter(left, right)` |
| `matches` | 20 | `MatchesBinary` | `CoreExtension::matches(right, left)` |
| `starts with` | 20 | `StartsWithBinary` | `(is_string($a = left) && is_string($b = right) && str_starts_with($a, $b))` |
| `ends with` | 20 | `EndsWithBinary` | `(is_string($a = left) && is_string($b = right) && str_ends_with($a, $b))` |
| `has some` | 20 | `HasSomeBinary` | `CoreExtension::arraySome($this->env, left, right, <isSandboxed>)` |
| `has every` | 20 | `HasEveryBinary` | `CoreExtension::arrayEvery($this->env, left, right, <isSandboxed>)` |
| `..` | 25 | `RangeBinary` | `range(left, right)` |
| `==` `!=` `<` `>` `<=` `>=` `<=>` | 20 | `*Binary` | native PHP operators on PHP >= 8 (`CoreExtension::compare` only for PHP < 8) |
| `===` `!==` | 20 | | native |
| `~` | 40 (27 in 4.0) | `ConcatBinary` | PHP `.` (string conversion of both sides, §1.8) |
| `+ - * / % **` `b-and b-or b-xor` | | | native PHP arithmetic (`//` → `(int) floor(a / b)`) |
| `and` `or` `xor` `not` | | | `&&` `\|\|` `xor` `!` with `TrueTest::wrap` on operands |
| `?:` | 5 | `ElvisBinary` | `((TrueTest(left)) ? (left) : (right))` |
| `??` | 300 (5 in 4.0) | `NullCoalesceBinary` | `((left is defined and not (left is null)) ? left : right)` (no null test for `block()`) |

`<isSandboxed>` = `$this->env->hasExtension(SandboxExtension::class) && …->getChecker()->isSandboxed($this->source)`.

### 7.1 `in` / `not in` — `CoreExtension::inFilter`

`CoreExtension.php:1071-1113`:

```
inFilter(value, compare):
    if value is Markup:   value = (string) value
    if compare is Markup: compare = (string) compare
    if is_string(compare):
        if is_string(value) or is_int(value) or is_float(value):
            return '' === value or str_contains(compare, (string) value)      # float -> precision-14 string
        return false                                                          # null, bool, array, objects
    if !is_iterable(compare): return false
    if is_object(value) or is_resource(value):
        if !is_array(compare):
            for item in compare: if item === value: return true
            return false
        return in_array(value, compare, true)                                # identity
    for item in compare:                     # values only (keys never matched)
        if 0 === compare(value, item): return true
    return false
```

`CoreExtension::compare($a, $b)` (`CoreExtension.php:1123-1175`) — Twig's "stricter loose equality":

```
compare(a, b):
    if is_int(a) and is_string(b):
        bTrim = trim(b, " \t\n\r\v\f")
        if !is_numeric(bTrim): return (string) a <=> b            # NOTE: untrimmed b
        if (int) bTrim == bTrim: return a <=> (int) bTrim
        return (float) a <=> (float) bTrim
    if is_string(a) and is_int(b):  (mirror: aTrim; !is_numeric -> a <=> (string) b; (int) aTrim == aTrim -> (int) aTrim <=> b; else floats)
    if is_float(a) and is_string(b):
        if is_nan(a): return 1
        bTrim = trim(b, " \t\n\r\v\f")
        if !is_numeric(bTrim): return (string) a <=> b
        return a <=> (float) bTrim
    if is_string(a) and is_float(b):  (mirror; is_nan(b) -> 1)
    return a <=> b                                                 # PHP 8 spaceship for everything else
```

[ORACLE] true: `1 in [1,2]`, `"1" in [1,2]`, `1 in ["1","2"]`, `"a" in "abc"`, `"" in "abc"`, `"" in ""`, `1 in "123"`,
`1.5 in "x1.5y"`, `1 in {a:1}`, `"1e1" in ["10"]`, `" 1" in [1]`, `"1 " in [1]`, `1 in [" 1"]`, `1 in ["1 "]`,
`null in [0]`, `null in [false]`, `null in [""]`, `false in [0]`, `true in ["a"]`, `[1] in [[1]]`, `[1] in [["1"]]`,
`o in [o]` (same instance), `"x" in [o]` (o Stringable "x", via `<=>`), `1 in generator`, `Markup("b") in "abc"`,
`1.0 in [1]`, `"1.0" in ["1"]`, `"INF" in [INF]`, `"9223372036854775808" in [PHP_INT_MAX]`.
false: `true in "1"`, `null in "abc"`, `"a" in {a:1}` (keys not searched), `"abc" in ["ABC"]`, `0 in ["a"]`, `"a" in [0]`,
`"abc" in [0]`, `o in [o2]` (equal but different instance), `o in ["x"]`, `"a" in 5`, `"a" in null`, `"a" in Markup("b")`,
`NAN in [NAN]`, `"abc" in ["abc "]`, `0.1+0.2 in ["0.3"]`, `"0.3" in [0.1+0.2]`.

### 7.2 `matches` — `CoreExtension::matches`

- Compile time (`TWIG/src/Node/Expression/Binary/MatchesBinary.php:21-37`): if the right operand is a literal, it is test-compiled
  with `preg_match($regexp, '')`; a PHP warning becomes
  `SyntaxError(sprintf('Regexp "%s" passed to "matches" is not valid: %s.', $regexp, substr($m, 14)), $lineno)` (`substr` drops `"preg_match(): "`).
  [ORACLE] `"abc" matches "/(/"` → `Regexp "/(/" passed to "matches" is not valid: Compilation failed: missing closing parenthesis at offset 1 in "index" at line 1.`;
  `"abc" matches "abc"` → `… is not valid: Delimiter must not be alphanumeric, backslash, or NUL byte in "index" at line 1.`
- Runtime (`CoreExtension.php:1182-1196`), signature `matches(string $regexp, ?string $str): int`:

```
set_error_handler(fn(t, m) => throw RuntimeError(sprintf('Regexp "%s" passed to "matches" is not valid', regexp) . substr(m, 12)))
try:
    if false === (result = preg_match(regexp, str ?? '')):
        throw RuntimeError(sprintf('Regexp "%s" passed to "matches" failed: %s.', regexp, preg_last_error_msg()))
    return result                       # 0 or 1 (int)
finally: restore_error_handler()
```

  `substr($m, 12)` keeps `": <reason>"`, and **no final period** is added. [ORACLE]
  `Regexp "/(/" passed to "matches" is not valid: Compilation failed: missing closing parenthesis at offset 1 in "index" at line 1`;
  `Regexp "" passed to "matches" is not valid: Empty regular expression in "index" at line 1`;
  backtracking limit → `Regexp "/(?:\D+|<\d+>)*[!?]/" passed to "matches" failed: Backtrack limit exhausted in "index" at line 1.`;
  invalid UTF-8 subject with `/u` → `… failed: Malformed UTF-8 characters, possibly incorrectly encoded in "index" at line 1.`;
  non-string left operand: int/Stringable coerced; array → PHP TypeError with file/line (non-portable).
- Regex dialect: PHP PCRE2 with delimiters and modifiers (`i m s x u U D A J n` …) — see the PCRE reference survey.

### 7.3 `starts with` / `ends with`

Both operands must be PHP strings, otherwise false (no coercion): [ORACLE] `123 starts with "1"` → false,
`"123" starts with 1` → false, `Markup("abc") starts with "a"` → false, `null starts with ""` → false;
`"abc" starts with ""` → true, `"abc" ends with ""` → true. Byte-wise comparison.

### 7.4 `has some` / `has every`

`CoreExtension.php:2149-2186`:

```
arraySome(env, array, arrow, isSandboxed = false):
    if !is_iterable(array): throw RuntimeError(sprintf('The "has some" test expects a sequence or a mapping, got "%s".', get_debug_type(array)))
    checkArrow(isSandboxed, arrow, 'has some', 'operator')
    for (k, v) in array: if arrow(v, k): return true
    return false
arrayEvery: same with 'has every'; returns false on the first falsy result, true otherwise (true for empty input)
```

Note precedence: in `seq has some v => v > 2 ? "T" : "F"` the ternary belongs to the arrow body [ORACLE].
A non-Closure right operand (e.g. `"is_int"`) is called with `(v, k)` after the deprecation → internal functions reject the
extra argument (`is_int() expects exactly 1 argument, 2 given`) [ORACLE].

### 7.5 Comparison helper `CoreExtension::compare`

Only used by `inFilter` on PHP 8 (see 7.1). The comparison operators compile to PHP operators (`zend_compare` semantics).

### 7.6 `..` (range)

`RangeBinary` compiles to `range(left, right)` (PHP, §12.13). In the sandbox, `..` requires the `range` *function*
to be allowed (`SandboxNodeVisitor.php:96-99`) [ORACLE `Function "range" is not allowed`].

---

## 8. Attribute access algorithm (`CoreExtension::getAttribute`)

Used by `a.b`, `a['b']`, `a.b(args)`, `a?.b` and the `attribute()` function. Source `TWIG/src/Extension/CoreExtension.php:1754-2010`.
Compile-time shortcuts (`TWIG/src/Node/Expression/GetAttrExpression.php:46-160`): an `a[b]` access (ARRAY_CALL) in non-strict
mode (or ignore-strict) without defined test compiles to `(is_array($v) || $v instanceof ArrayAccess ? ($v[key] ?? null) : null)`;
with a sandbox registered, only `ARRAY_LIKE_CLASSES` take the shortcut; a Stringable key is cast to string for arrays,
`ArrayObject`, `ArrayIterator`. `?.` short-circuits to `null` when the object is `null`.

```
getAttribute(env, source, object, item, arguments = [], type = ANY_CALL, isDefinedTest = false, ignoreStrictCheck = false, sandboxed = false, lineno = -1):
    propertyNotAllowedError = null
    if sandboxed and item is Stringable: checker.ensureToStringAllowed(item, lineno, source)
    # --- array / ArrayAccess step (ANY_CALL or ARRAY_CALL)
    if type !== METHOD_CALL:
        arrayItem = (is_bool(item) or is_float(item)) ? (int) item : item
        if arrayItem is Stringable and (object is ArrayObject or ArrayIterator): arrayItem = (string) arrayItem
        if sandboxed and object is ArrayAccess and class(object) not in ARRAY_LIKE_CLASSES:
            try checker.checkPropertyAllowed(object, arrayItem, lineno, source)
            catch SecurityNotAllowedPropertyError as propertyNotAllowedError: item = (string) item; goto methodCheck
        exists = is_array(object) ? array_key_exists(arrayItem = (string) arrayItem, object)
               : object is ArrayAccess ? object.offsetExists(arrayItem) : false
        if exists: return isDefinedTest ? true : object[arrayItem]
        if type === ARRAY_CALL or !is_object(object):
            if isDefinedTest: return false
            if ignoreStrictCheck or !env.isStrictVariables(): return null
            message =
              object is ArrayAccess ? (is_object/array(arrayItem) ? 'Key of type "%s" does not exist in ArrayAccess-able object of class "%s".' % (debug(arrayItem), debug(object))
                                                                : 'Key "%s" does not exist in ArrayAccess-able object of class "%s".' % (arrayItem, debug(object)))
              : is_object(object) ? 'Impossible to access a key "%s" on an object of class "%s" that does not implement ArrayAccess interface.' % (item, debug(object))
              : is_array(object) ? (empty ? 'Key "%s" does not exist as the sequence/mapping is empty.' % arrayItem
                                          : 'Key "%s" for sequence/mapping with keys "%s" does not exist.' % (arrayItem, implode(', ', array_keys(object))))
              : type === ARRAY_CALL ? (object === null ? 'Impossible to access a key ("%s") on a null variable.' % item
                                                       : 'Impossible to access a key ("%s") on a %s variable ("%s").' % (item, debug(object), object))
              : object === null ? 'Impossible to access an attribute ("%s") on a null variable.' % item
              : 'Impossible to access an attribute ("%s") on a %s variable ("%s").' % (item, debug(object), object)
            throw RuntimeError(message, lineno, source)
    item = (string) item
    if !is_object(object):
        if isDefinedTest: return false
        if ignoreStrictCheck or !strict: return null
        message = object === null ? 'Impossible to invoke a method ("%s") on a null variable.'
                : is_array(object) ? 'Impossible to invoke a method ("%s") on a sequence/mapping.'
                : 'Impossible to invoke a method ("%s") on a %s variable ("%s").' % (item, debug(object), object)
        throw RuntimeError(message, lineno, source)
    if object is Template: throw RuntimeError('Accessing \Twig\Template attributes is forbidden.', lineno, source)
    # --- property step (ANY_CALL only reaches here; METHOD_CALL skips)
    if type !== METHOD_CALL:
        if sandboxed:
            try checker.checkPropertyAllowed(object, item, lineno, source)
            catch SecurityNotAllowedPropertyError as propertyNotAllowedError: goto methodCheck
        checker = getPropertyChecker(class, item)          # cached per class/item:
            # class has no declared property `item` -> property_exists() closure (dynamic props)
            # declared but not public or static -> class has __isset ? true : false
            # declared public -> closure checking ReflectionProperty::isInitialized
        if checker !== false and (isset(object->item) or (checker is Closure and checker(object, item))):
            return isDefinedTest ? true : object->item      # public prop (even null, if initialized/declared)
        if object is DateTimeInterface and item in ['date','timezone','timezone_type']:
            return isDefinedTest ? true : ((array) object)[item]
        if defined(class . '::' . item): return isDefinedTest ? true : constant(class . '::' . item)   # class constants, enum cases
    methodCheck:
    cache[class] (built once per class): methods = get_class_methods(object) (+ '__invoke' for Closure); sort(methods);
        for each method m (sorted), lc = strtolower(m):
            map[m] = m; map[lc] = m
            if lc starts 'get': name = substr(lc, 3)
            elif lc starts 'is': name = substr(lc, 2)
            elif lc starts 'has': name = substr(lc, 3); if 'is' . name in lcMethods: continue
            else continue
            if name !== '':
                camel = name[0] . substr(m, prefixLen + 1)          # getFooBar -> fooBar
                map[camel] ??= m; map[name] ??= m
    call = false
    if item in map: method = map[item]
    elif strtolower(item) in map: method = map[strtolower(item)]
    elif '__call' in map: method = item; call = true
    else:
        if isDefinedTest: return false
        if propertyNotAllowedError: throw propertyNotAllowedError
        if ignoreStrictCheck or !strict: return null
        throw RuntimeError(sprintf('Neither the property "%1$s" nor one of the methods "%1$s()", "get%1$s()", "is%1$s()", "has%1$s()" or "__call()" exist and have public access in class "%2$s".', item, class), lineno, source)
    if sandboxed:
        try checker.checkMethodAllowed(object, method, lineno, source)
        catch SecurityNotAllowedMethodError e:
            if isDefinedTest: return false
            if propertyNotAllowedError: throw propertyNotAllowedError
            throw e
    if isDefinedTest: return true
    try: return object->method(...arguments)
    catch BadMethodCallException e: if call and (ignoreStrictCheck or !strict): return null; throw e
```

Precedence summary for `a.b`: array key / ArrayAccess offset → public property → DateTime special props → class constant →
method `b()` → `getB()` → `isB()` → `hasB()` → `__call`. Mechanics: method names are processed in `sort()` order
(case-sensitive byte order); exact names (`$classCache[$method]`, `$classCache[strtolower($method)]`) are assigned
unconditionally (so an exact method always wins), prefixed aliases only if not yet set; `get…` sorts before `is…`, and a
`has…` method is ignored entirely when the matching `is…` exists — giving get > is > has (comment at `CoreExtension.php:1908`).
Lookup tries the item as given, then lower-cased.
[ORACLE] error texts: `Impossible to access an attribute ("y") on a null variable.`; `Impossible to access a key ("y") on a null variable.`;
`Impossible to access an attribute ("y") on a string variable ("str").`; `Impossible to access a key ("y") on a int variable ("5").`;
`Key "y" does not exist as the sequence/mapping is empty.`; `Key "y" for sequence/mapping with keys "a, b" does not exist.`;
`Impossible to invoke a method ("y") on a null variable.`; `Impossible to invoke a method ("y") on a sequence/mapping.`;
`Impossible to invoke a method ("y") on a string variable ("abc").`;
`Impossible to access a key "y" on an object of class "C" that does not implement ArrayAccess interface.`;
`Neither the property "y" nor one of the methods "y()", "gety()", "isy()", "hasy()" or "__call()" exist and have public access in class "ArrayObject".`;
`Impossible to access an attribute ("y") on a bool variable ("1").`; `… on a float variable ("1.5").`;
`x[1.5]`/`x[true]` read key `1`. Private properties are invisible (non-strict → null).

---

## 9. Sandbox

Files: `TWIG/src/Sandbox/*.php`, `TWIG/src/Extension/SandboxExtension.php`, `TWIG/src/NodeVisitor/SandboxNodeVisitor.php`,
`TWIG/src/Node/CheckSecurityNode.php`, `CheckSecurityCallNode.php`, `CheckToStringNode.php`, `TWIG/src/Util/CallableParameters.php`,
`TWIG/src/Extension/SandboxBridgeExtension.php`, `TWIG/src/Runtime/SandboxBridgeRuntime.php`,
`TWIG/src/Node/Expression/FunctionNode/RenderSandboxedFunction.php`, docs `TWIG/doc/sandbox.rst`.

### 9.1 Exceptions (all `Twig\Sandbox\SecurityError extends Twig\Error\Error`)

| class | ctor | getter |
|---|---|---|
| `SecurityNotAllowedTagError` | `(string $message, string $tagName)` | `getTagName()` |
| `SecurityNotAllowedFilterError` | `(string $message, string $functionName)` | `getFilterName()` |
| `SecurityNotAllowedFunctionError` | `(string $message, string $functionName)` | `getFunctionName()` |
| `SecurityNotAllowedTestError` | `(string $message, string $testName)` | `getTestName()` |
| `SecurityNotAllowedMethodError` | `(string $message, string $className, string $methodName)` | `getClassName()`, `getMethodName()` |
| `SecurityNotAllowedPropertyError` | `(string $message, string $className, string $propertyName)` | `getClassName()`, `getPropertyName()` |

They are thrown without source/line; `SecurityChecker`/`CheckSecurityNode` set the source and the line (first line where the
tag/filter/function/test is used; method/property: the access line).

### 9.2 `SecurityPolicy` (`TWIG/src/Sandbox/SecurityPolicy.php`)

Constructor `(array $allowedTags = [], array $allowedFilters = [], array $allowedMethods = [], array $allowedProperties = [], array $allowedFunctions = [], array $allowedTests = [])`;
setters `setAllowedTags/Filters/Methods/Properties/Functions/Tests`, `setStrict(bool)`, `isStrict()` (internal).
`setAllowedMethods`: `[class => method|methods[]]`, method names lower-cased.

```
checkSecurity(tags, filters, functions, tests = []):          # 4th param missing at call -> deprecation 3.28:
    #   'Not passing the "$tests" argument to "%s::checkSecurity()" is deprecated; it will be required in 4.0.'
    for tag in tags: if tag not in allowedTags (strict in_array):
        if !strict and tag === 'extends': trigger_deprecation('twig/twig','3.12','The "extends" tag is always allowed in sandboxes, but won\'t be in 4.0, please enable it explicitly in your sandbox policy if needed (or enable strict mode on the security policy to opt-in to the 4.0 behavior now).')
        elif !strict and tag === 'use':   (same text with "use")
        else: throw SecurityNotAllowedTagError(sprintf('Tag "%s" is not allowed.', tag), tag)
    for filter in filters: if not allowed: throw SecurityNotAllowedFilterError(sprintf('Filter "%s" is not allowed.', filter), filter)
    for function in functions: if not allowed:
        if !strict and function in ['parent','block','attribute']:
            trigger_deprecation('twig/twig','3.27','The "<name>" function is always allowed in sandboxes, but won\'t be in 4.0, please enable it explicitly in your sandbox policy if needed (or enable strict mode on the security policy to opt-in to the 4.0 behavior now).')
        else: throw SecurityNotAllowedFunctionError(sprintf('Function "%s" is not allowed.', function), function)
    for test in tests: if not allowed:
        if !strict: trigger_deprecation('twig/twig','3.28','The "%s" test is always allowed in sandboxes, but won\'t be in 4.0, please enable it explicitly in your sandbox policy if needed (or enable strict mode on the security policy to opt-in to the 4.0 behavior now).', test)
        else: throw SecurityNotAllowedTestError(sprintf('Test "%s" is not allowed.', test), test)

checkMethodAllowed(obj, method):
    method = strtolower(method)
    if obj is Template or (obj is Markup and method not in ['__construct', 'withcontent']): return
    for (class, methods) in allowedMethods: if obj instanceof class and method in methods: return
    throw SecurityNotAllowedMethodError(sprintf('Calling "%s" method on a "%s" object is not allowed.', method, obj::class), obj::class, method)

checkPropertyAllowed(obj, property):
    for (class, props) in allowedProperties: if obj instanceof class and property in (is_array(props) ? props : [props]) (strict): return
    throw SecurityNotAllowedPropertyError(sprintf('Calling "%s" property on a "%s" object is not allowed.', property, obj::class), obj::class, property)
```

Order of checks is tags, filters, functions, tests; the first failure wins [ORACLE `{% if %}…|upper…range()` → tag error].
Method names in messages are lower-cased (`Calling "getbody" method on a "Art" object is not allowed.`); property names are
case-sensitive; `instanceof` covers subclasses and interfaces [ORACLE `Child extends Art` allowed by `Art` entry].
`Markup` methods (except constructor/`withContent`) are always allowed.

### 9.3 `SecurityChecker` (internal) and `SandboxExtension`

`SandboxExtension::__construct(SecurityPolicyInterface $policy, $sandboxed = false, ?SourcePolicyInterface $sourcePolicy = null)`
(passing a source policy: deprecation 3.27.0 `The "Twig\Sandbox\SourcePolicyInterface" interface is deprecated with no replacement, do not pass an instance to "Twig\Extension\SandboxExtension".`).
`enableSandbox()`, `disableSandbox()`, `isSandboxedGlobally()` → deprecations 3.29
(`The "%s()" method is deprecated, use "Twig\Sandbox\Sandbox" to render untrusted templates instead.`).
`checkSecurity($tags, $filters, $functions, $tests = [], $source = null)` with a `Source` as 4th arg → 3.28 deprecation
`Passing a "Twig\Source" as the 4th argument of "%s()" is deprecated; pass an array of tests instead.`

`SecurityChecker` (`TWIG/src/Sandbox/SecurityChecker.php`):

```
isSandboxed(source = null) = sandboxedGlobally or sandboxed(flag set by include(sandboxed=true)/enableSandbox) or (source and sourcePolicy and sourcePolicy.enableSandbox(source))
checkSecurity(tags, filters, functions, tests, source):
    if !isSandboxed(source): return
    policy.checkSecurity(...)   # if the policy method declares < 4 params: deprecation 3.28 'The "%s::checkSecurity()" method will take a 4th "array $tests" argument in 4.0; not declaring it is deprecated.' and call with 3
    catch SecurityError e: e.setSourceContext(source); rethrow
checkMethodAllowed / checkPropertyAllowed(obj, name, lineno, source): if isSandboxed(source): policy check; on error set source + line, rethrow
ensureToStringAllowed(obj, lineno, source) = doEnsure(obj, seen = new SplObjectStorage)
doEnsure(obj):
    if is_array(obj): ensureArray(obj); return obj               # walked before the sandbox check; element checks below return early when not sandboxed
    if !isSandboxed(source): return obj
    if obj is Stringable: policy.checkMethodAllowed(obj, '__toString')   (error gets source + line)
    if obj is Traversable:
        if obj in seen: return obj; seen.add(obj)
        if obj is IteratorAggregate: for v in obj: doEnsure(v); return obj        # iterates (getIterator called again later)
        array = iterator_to_array(obj); ensureArray(array)
        if obj is not Stringable: return array                   # single-pass iterators are replaced by the materialised array
    return obj
ensureArray(arr, stack): for (k, v) in arr: if !v: continue; if !is_array(v): doEnsure(v); continue
                         if v is a PHP reference already on the stack: continue (cycle guard); recurse
ensureSpreadAllowed(iterable): materialise Traversable (iterator_to_array) then ensureArray; return the array
```

### 9.4 What the `SandboxNodeVisitor` collects and wraps

`TWIG/src/NodeVisitor/SandboxNodeVisitor.php:57-128`, for every node inside a module:

- **tags**: `node.getNodeTag()` unless the token parser `isAlwaysAllowedInSandbox()` (no core tag returns true;
  `AbstractTokenParser::isAlwaysAllowedInSandbox()` → false);
- **filters**: each `FilterExpression` name (including the `escape` filters inserted by auto-escaping — so a sandboxed,
  auto-escaped template needs `escape` allowed: [ORACLE `{{ a }}` → `Filter "escape" is not allowed.`]) unless the filter is
  `always_allowed_in_sandbox` (no core filter is);
- **functions**: each `FunctionExpression` name, plus `parent`/`block`/`attribute` via the `sandboxed_function_name`
  attribute set by `FunctionExpressionParser`, plus `range` for every `..` operator; minus `always_allowed_in_sandbox`
  (no core function is);
- **tests**: each `TestExpression` name minus always-allowed (all core tests except `constant`);
- each name is recorded with the line of its first use (used for the error line).
- **string-coercion wrapping**: for nodes implementing `CoercesChildrenToStringInterface` it wraps the children listed by
  `getStringCoercedChildNames()` in `CheckToStringNode` (→ `$this->sandbox->ensureToStringAllowed(expr, line, $this->source)`;
  spread operands use `ensureSpreadAllowed`). For filter/function/test calls it consults the PHP parameter types
  (`CallableParameters::isStringCoercionSafe`): a parameter typed `int`, `float`, `bool`, `null`, `true`, `false`, `void`,
  `never`, an enum, or a `final` class that is neither `Stringable` nor `Traversable` is "safe" and not wrapped; untyped,
  `string`, `array`, `iterable`, `mixed`, `object`, interfaces and non-final classes are wrapped. Only variables
  (`ContextVariable` except `_self`), attribute accesses (non-generator), filter/function/test calls, assignments, arrays
  (element-wise) and `?:`/`??` operands are wrapped.
  Coercing children: filters `node`+`arguments`; functions `arguments`; generic tests `node`(only for the base class)+`arguments`;
  `ConstantTest` `arguments`; `NonStringCoercingInputTest` `arguments`; `defined`, `divisible by`, `even`, `odd`, `null`, `same as`,
  `true` → none; `in`, `not in`, `matches`, `..`, `~`, `==`, `!=`, `<`, `>`, `<=`, `>=`, `<=>` → both operands;
  `starts with`/`ends with`/`has some`/`has every` → none; `GetAttr` → `arguments` and (for `[]`) the key; `block(…, tpl)` → template.
  Consequence [ORACLE]: `{{ a }}`, `{{ a|upper }}`, `{{ a ~ "" }}`, `{{ [a]|join }}`, `{{ [...arts]|length }}`, and even
  `arts|column("secret")` (array input deep-checked) raise `Calling "__tostring" method on a "Art" object is not allowed.`

At the end of the module it injects `CheckSecurityCallNode` (constructor: `$this->sandbox = $env->getExtension(SandboxExtension::class)->getChecker();`)
and `CheckSecurityNode` (`ensureSecurityChecked()`, `checkSecurity()` with the four name lists in first-use order; if
`use` is among the tags, `checkTraitsAllowed()` is generated and run from the constructor). `checkSecurity` runs when a
template starts rendering (`Template::yield`, `yieldBlock`, `getParent`), so part of the output of an outer template may
already be streamed before an included template fails. Error line = the recorded line for that name.

### 9.5 Runtime checks inside callables

- `checkArrow`: non-Closure callables rejected in sandbox (`filter`, `find`, `map`, `reduce`, `sort`, `has some`, `has every`) — §4.40.
- `column`: property allow-list checked for each object row (§4.4).
- `getAttribute`: property/method allow-lists, ArrayAccess offsets checked as properties except for `ARRAY_LIKE_CLASSES`
  (`ArrayIterator`, `ArrayObject`, `CachingIterator`, `RecursiveArrayIterator`, `RecursiveCachingIterator`,
  `SplDoublyLinkedList`, `SplFixedArray`, `SplObjectStorage`, `SplQueue`, `SplStack`, `WeakMap` — exact class match, not
  subclasses) (§8) [ORACLE `aa.foo` on custom ArrayAccess → `Calling "foo" property on a "AA" object is not allowed.`;
  `ArrayObject` → allowed].
- `include(…, sandboxed=true)` toggles the legacy flag (§5.8).

### 9.6 Always-allowed list and Twig 4 changes

Always allowed in 3.31: tests `defined`, `divisible by`, `empty`, `even`, `iterable`, `mapping`, `none`, `null`, `odd`,
`same as`, `sequence`, `true`. Non-strict policies additionally allow (with deprecations) the `extends`/`use` tags, the
`parent`/`block`/`attribute` functions and every test. `doc/sandbox.rst` lists items that become always-allowed in 4.0
(tags apply/block/do/for/if/macro/set/types/with; filters abs, batch, capitalize, convert_encoding, default, e, escape,
first, format, join, keys, last, length, lower, merge, nl2br, number_format, replace, reverse, round, slice, split,
striptags, title, trim, upper, url_encode; functions cycle, max, min) — not yet in 3.31.1 code.

### 9.7 `Sandbox` class (3.29+)

`new Sandbox(Environment $env, SecurityPolicyInterface $policy)` (`TWIG/src/Sandbox/Sandbox.php:32-45`):
- a non-strict `SecurityPolicy` → `\LogicException('The sandbox requires a strict security policy, call "setStrict(true)" on a dedicated policy for this sandbox.')` [ORACLE];
- registers `new SandboxExtension($policy, true)` on the env; if that throws `\LogicException` (extensions already
  initialised or already registered) → `\LogicException('The environment passed to "Twig\Sandbox\Sandbox" must be dedicated to it: pass a freshly built environment that has no "Twig\Extension\SandboxExtension" registered and has not been used yet.')`.
- methods delegate to the env: `render`, `display`, `stream`, `renderBlock`, `displayBlock`, `streamBlock`, `createTemplate`.

### 9.8 `render_sandboxed` function (SandboxBridgeExtension)

- Registration (`TWIG/src/Extension/SandboxBridgeExtension.php:23-28`):
  `new TwigFunction('render_sandboxed', [SandboxBridgeRuntime::class, 'render'], ['is_safe_callback' => [RenderSandboxedFunction::class, 'getSafe'], 'node_class' => RenderSandboxedFunction::class])`.
  Requires a runtime loader providing `SandboxBridgeRuntime($sandbox)`; otherwise `RuntimeError('Unable to load the "Twig\Runtime\SandboxBridgeRuntime" runtime.')` (`Environment::getRuntime`).
- Signature `render_sandboxed(name, context, output_strategy)`; runtime method `render(string $name, array $context): string`
  → `$sandbox->render($name, $context)` (context does not inherit the caller's variables).
- Compile time (`RenderSandboxedFunction.php:24-49`): the argument named `output_strategy` (normalised `outputstrategy`)
  or positional index 2 is required: `Value for argument "output_strategy" is required for function "render_sandboxed".`;
  it must be a literal non-empty string other than `all`:
  `The "output_strategy" argument of the "render_sandboxed" function must be a non-empty literal string other than "all".`
  It is removed from the runtime call and becomes the safety declaration `[strategy]`.
- The result string is safe only for that strategy (with implications): [ORACLE] `"html"` → raw in html;
  `"js"` → html-escaped when printed in html; `"html_attr"` → raw in html (implied). Missing `context` →
  `Value for argument "context" is required for function "render_sandboxed".`; violations inside the sandboxed template report
  the sandboxed template name (`Filter "lower" is not allowed in "bad" at line 1.`).

---

## 10. Debug extension: `dump`

- Registration (`TWIG/src/Extension/DebugExtension.php:21-34`):
  `new TwigFunction('dump', [self::class, 'dump'], ['is_safe' => $isDumpOutputHtmlSafe ? ['html'] : [], 'needs_context' => true, 'needs_environment' => true, 'is_variadic' => true])`
  where `$isDumpOutputHtmlSafe = (extension_loaded('xdebug') && str_contains(ini_get('xdebug.mode'), 'develop') && (false === ini_get('html_errors') || ini_get('html_errors'))) || 'cli' === PHP_SAPI`.
  Under the CLI SAPI the output is **not** escaped [ORACLE `dump("<b>")` → `string(3) "<b>"`]; under a web SAPI without
  xdebug it is HTML-escaped. A port must pick one (configuration flag) — §13.
- Runtime (`DebugExtension.php:39-61`):

```
dump(env, context, ...vars):
    if !env.isDebug(): return null                    # prints nothing
    ob_start()
    if vars is empty:
        vars = {}
        for (key, value) in context: if value is not Template and not TemplateWrapper: vars[key] = value
        var_dump(vars)
    else:
        var_dump(...vars)                             # one dump per argument, each ends with "\n"
    return ob_get_clean()
```

- `dump()` with no argument dumps the context array incl. globals and set variables (insertion order: render context first,
  then globals added by `+=`, then variables set later) [ORACLE].
- Named arguments are collected into the variadic with string keys and spread into `var_dump` → PHP named-argument
  semantics (`var_dump() expects at least 1 argument, 0 given` for only-named) + 3.15 snake_case deprecation [ORACLE].
- `var_dump` format (`PHP/ext/standard/var.c:44-117, 118-250`), indentation = 2 spaces per nesting level:
  - `NULL`, `bool(true)`, `bool(false)`, `int(42)`, `float(<serialize_precision -1 repr, 'E' exponent>)` e.g.
    `float(1.5)`, `float(1)`, `float(-0)`, `float(1.0E+25)`, `float(1.0E-5)`, `float(0.30000000000000004)`,
    `float(NAN)`, `float(INF)`, `float(-INF)`;
  - `string(<byte length>) "<raw bytes>"`;
  - `array(<count>) {` newline, per element `  [<int>]=>` or `  ["<key>"]=>` newline, value dumped at +2 indent, closing `}`;
    recursion → `*RECURSION*`;
  - objects `object(<Class>)#<handle> (<prop count>) {`; props `["name"]=>`, `["name":protected]=>`,
    `["name":"Class":private]=>`, uninitialised typed props `uninitialized(int)`; recursion `*RECURSION*`;
    enums `enum(Suit::Hearts)`; closures dump `name`/`file`/`line`/`parameter` pseudo-properties (PHP 8.4+);
    lazy objects prefixed `lazy ghost `/`lazy proxy `.
  - [ORACLE] `dump(1, "s", 1.5, true, null, [1, {a: "b"}], o)` →
    ```
    int(1)
    string(1) "s"
    float(1.5)
    bool(true)
    NULL
    array(2) {
      [0]=>
      int(1)
      [1]=>
      array(1) {
        ["a"]=>
        string(1) "b"
      }
    }
    object(C)#3403 (3) {
      ["p"]=>
      string(2) "pp"
      ["n"]=>
      NULL
      ["priv":"C":private]=>
      int(1)
    }
    ```
  Object handle numbers (`#3403`) depend on PHP's allocation history and are not reproducible (§13).

---

## 11. StringLoader extension: `template_from_string`

- Registration (`TWIG/src/Extension/StringLoaderExtension.php:20-25`):
  `new TwigFunction('template_from_string', [self::class, 'templateFromString'], ['needs_environment' => true])`.
  Not safe; returns a `TemplateWrapper` (printing it directly → `Object of class Twig\TemplateWrapper could not be converted to string` [ORACLE]);
  meant for `include(template_from_string(...))`, `embed`/`extends` targets, `block(name, tpl)`.
- `templateFromString(Environment $env, string|\Stringable $template, ?string $name = null): TemplateWrapper` →
  `$env->createTemplate((string) $template, $name)`.
- `Environment::createTemplate($template, $name)` (`TWIG/src/Environment.php:469-490`):

```
hash = hash('xxh128', template)            # PHP >= 8.1; lowercase hex of XXH3-128 (big-endian canonical form)
name = name !== null ? sprintf('%s (string template %s)', name, hash) : sprintf('__string_template__%s', hash)
loader = new ChainLoader([new ArrayLoader([name => template]), current = env.getLoader()])
env.setLoader(loader)
try: return new TemplateWrapper(env, env.loadTemplate(env.getTemplateClass(name), name))
finally: env.setLoader(current)
```

  The generated name appears in error messages: [ORACLE] `template_from_string("{{ x", "mytpl")` →
  `Unexpected token "end of template" ("end of print statement" expected) in "mytpl (string template 9ff43002e15bfa7f71e7f19f9dcd8e68)" at line 1 column 5.`;
  without name → `… in "__string_template__9ff43002e15bfa7f71e7f19f9dcd8e68" at line 1 column 5.`
  Test vectors: `hash('xxh128', '')` = `99aa06d3014798d86001c324468d497f`; `hash('xxh128', 'Hello {{ name }}')` =
  `b82f37db51c94f1c005847c9210f86bb` [ORACLE]. A port must implement XXH3-128 to match names.
- The created template sees the enclosing environment's loader (for its own includes) only during compilation/loading.
- `template_from_string` is never sandbox-safe (doc `template_from_string.rst` "danger" note).


---

## 12. PHP builtin reference (port notes)

Only the behaviour reachable from core callables is described. Line numbers refer to `PHP/` (php-src 8.5.13).

### 12.1 `htmlspecialchars`

See §3.3.

### 12.2 `rawurlencode`

See §3.7.

### 12.3 `http_build_query($data, '', '&', PHP_QUERY_RFC3986)` (used by `url_encode` on arrays)

`PHP/ext/standard/http.c:25-222`:

```
encode_hash(ht, out, key_prefix = null, objectScope = null):
    for (key, value) in ht:                                   # insertion order
        if objectScope and key is a property name not accessible from outside: skip   # only public props of objects
        name = (key is string) ? unmangled key : null
        if value is array or (object and not enum):
            sub = name !== null ? rawurlencode(name) : (string) intKey
            new_prefix = key_prefix ? key_prefix . sub . '%5D%5B' : sub . '%5B'
            encode_hash(HASH_OF(value), out, new_prefix, value if object)      # recursion-protected: a recursive array is silently skipped
        elif value is null (or resource): skip
        else:
            if out not empty: out .= '&'
            if key_prefix: out .= key_prefix
            out .= name !== null ? rawurlencode(name) : (string) intKey        # numeric_prefix is '' in Twig
            if key_prefix: out .= '%5D'
            out .= '='
            out .= string  -> rawurlencode(string)
                   int     -> decimal
                   float   -> rawurlencode(zend_double_to_str(f))              # precision-14 form, e.g. "1.0E%2B25"
                   false   -> '0' ; true -> '1'
                   backed enum -> its value (re-dispatched); unit enum -> ValueError 'Unbacked enum %s cannot be converted to a string'
```

Top-level enum → `TypeError('http_build_query(): Argument #1 ($data) must not be an enum, %s given')` — not reachable from
Twig (only arrays go there). Empty array → `''`.

### 12.4 `strip_tags($str, $allowed)` — state machine

`PHP/ext/standard/string.c:4961-5003` (argument handling) and `php_strip_tags_ex` 5257-5576 (machine), `php_tag_find`
5174-5226. Allowed tags given as an array are turned into `"<a><b>"`. With `allow_tag_spaces = false`. Port the machine
verbatim; summary:

- state 0 (text): `\0` dropped; `<` followed by whitespace → literal `<` output (stays in state 0); `<` otherwise
  (unless inside quotes `in_q`) → state 1, buffer `<` (if allowed tags given); `>` → if `depth > 0` decrement, else output
  (unless `in_q`); other bytes output.
- state 1 (inside tag): `<` (not in quotes, next byte not whitespace) → `depth++`; `>` → if depth, decrement; else (not in
  quotes): end of tag, if allowed tags present and `php_tag_find(buffer, allowed)` → emit the buffered tag verbatim; back to
  state 0; `"`/`'` toggle quote state (only if `p != buf` and matching quote char); `!` right after `<` → state 3; `?` right
  after `<` → state 2 (PHP); other bytes buffered.
- state 2 (`<?…?>`): parentheses depth tracking outside quotes; `>` ends when depth 0, previous byte `?` and not in a
  double-quote context; `<?xml` (case-insensitive `xml` then `l`) switches to state 1 with `is_xml`.
- state 3 (`<!…>`): `>` ends (if not in quotes); `<!--` → state 4 (comment, ends at `-->`); `<!DOCTYPE` → state 1.
- state 4: ends at `-->` (not in quotes).
- `php_tag_find`: normalises the buffered tag: lower-case, keep `<`, drop leading whitespace, stop at the first
  whitespace after the name or at `>`, drop a `/` that is directly after `<` or directly before `>` (so `</b>` → `<b>`,
  `<br/>` → `<br>`), append `>`; then checks `strstr(allowedLowercased, "<name>")` (substring match!).
- [ORACLE] `"a < b <br>c"` → `a < b c`; with `"<br>"` allowed → `a < b <br>c`; `"x<a href='>'>y</a>z"` → `xyz`;
  `"1 <2 3>4"` → `1 4`.

### 12.5 `strtr($str, array $pairs)`

`PHP/ext/standard/string.c:3475-3533` (`php_strtr_array`), `2977-3121` (`php_strtr_array_ex`), `3536-3567` (`strtr`).

```
strtr(str, pairs):
    if str === '': return ''
    if count(pairs) == 0: return str
    if count(pairs) == 1:
        (key, value) = the pair; key = int key -> decimal string
        if key === '': warning 'strtr(): Ignoring replacement of empty string'; return str
        return str_replace-like replace of all non-overlapping occurrences of key by (string) value (left to right)
    # general case
    collect keys: int keys -> decimal strings; '' keys -> warning (once per empty key) and ignored; keys longer than str ignored
    minlen/maxlen = min/max key length; if none: return str
    pos = 0; out = ''
    while pos <= len(str) - minlen:
        for len from min(maxlen, len(str) - pos) down to minlen:     # longest match first
            if str[pos:pos+len] is a key: out .= pending text . (string) pairs[key]; pos += len; continue outer
        pos++
    return out . rest
```

Replacement values are converted with `zval_get_string` (float precision-14, `true`→`1`, `null`→`''`, arrays → `Array` +
warning). Replaced output is never re-scanned.

### 12.6 `explode($sep, $str, $limit = PHP_INT_MAX)` and `implode($glue, $array)`

`PHP/ext/standard/string.c:847-969`:

```
explode(sep, str, limit):
    if sep === '': throw ValueError('explode(): Argument #1 ($separator) must not be empty')
    if str === '': return limit >= 0 ? [''] : []
    if limit > 1:  split on each occurrence (left to right, non-overlapping) until limit-1 pieces, the rest is the last piece
    elif limit < 0: split fully, drop the last -limit pieces (no occurrence -> [])
    else (0 or 1): [str]
```

`implode(glue, pieces)` (string.c:974-1083): 0 elements → `''`; 1 element → `zval_get_string(element)`; else elements joined:
strings as is, ints in decimal, others via `zval_get_string` (float precision 14, bool, null, `Array` + warning, Stringable).

### 12.7 `trim`/`ltrim`/`rtrim` and character masks

`PHP/ext/standard/string.c:475-516` (`php_charmask`), `524-608` (`php_trim_int`). Mask of length 1 → compare that byte;
otherwise build a 256-entry mask: `X..Y` (with `Y >= X`) adds the byte range; malformed ranges emit warnings and are
skipped: `Invalid '..'-range, no character to the left of '..'`, `Invalid '..'-range, no character to the right of '..'`,
`Invalid '..'-range, '..'-range needs to be incrementing`, `Invalid '..'-range` (all prefixed `trim(): ` etc. by
`php_error_docref`). Trimming is byte-wise (multi-byte characters in the mask match their individual bytes).

### 12.8 `sprintf` (format filter)

`PHP/ext/standard/formatted_print.c:72-700`. Port-ready:

```
sprintf(format, args):
    out = ''; currarg = 0; maxMissing = -1
    loop over format:
        copy text up to next '%'; skip '%'
        if next == '%': out += '%'; continue
        align = RIGHT; pad = ' '; alwaysSign = false; expprec = false; adjusting = 0; width = 0; precision = 0
        if isalpha(next): argnum = NEXT
        else:
            argnum = NEXT; if digits followed by '$': n = number; if n <= 0 or n >= INT_MAX:
                throw ValueError('Argument number specifier must be greater than zero and less than 2147483647'); argnum = n-1; skip '$'
            loop modifiers: ' ' or '0' -> pad = it; '-' -> align = LEFT; '+' -> alwaysSign = true;
                            '\'' -> pad = following byte (none: ValueError 'Missing padding character'); else break
            width: '*' [n$] -> takes an int argument (non-int: ValueError 'Width must be an integer'; <0 or >INT_MAX: 'Width must be between 0 and 2147483647')
                   digits -> number (>= INT_MAX: ValueError 'Width must be between 0 and 2147483647'); adjusting |= WIDTH
            if '.': precision: '*' [n$] -> int arg (non-int: 'Precision must be an integer'; < -1 or > INT_MAX: 'Precision must be between -1 and 2147483647'); expprec = true
                               digits -> number (>= INT_MAX: 'Precision must be between 0 and 2147483647'); expprec = true
                               nothing -> precision 0 (expprec false); adjusting |= PRECISION
        if next == 'l': skip
        if argnum == NEXT: argnum = currarg++
        if argnum >= argc: maxMissing = max(maxMissing, argnum); continue        # error raised at the end
        if expprec and precision == -1 and conv not in 'gGhH': throw ValueError('Precision -1 is only supported for %g, %G, %h and %H')
        switch conv:
            's': appendString(zval_get_string(arg), width, precision, pad, align, expprec)
            'd': appendInt(zval_get_long(arg))       'u': appendUInt((uint64) zval_get_long(arg))
            'e','E','f','F','g','G','h','H': appendDouble(zval_get_double(arg), ...)
            'c': out += chr(zval_get_long(arg) & 0xFF)          # width/padding ignored
            'o','x','X','b': append2n(zval_get_long(arg), bits 3/4/4/1, lower/lower/UPPER/lower, expprec)
            '%': out += '%'                                       # NOTE: still consumed an argument slot above
            end of string: throw ValueError('Missing format specifier at end of string')
            default: throw ValueError(sprintf('Unknown format specifier "%c"', conv))
    if maxMissing >= 0: throw ArgumentCountError(sprintf('%d arguments are required, %d given', maxMissing + 2, argc + 1))   # counts include the format

appendString(s, minWidth, maxWidth, pad, align, expprec, neg = false, alwaysSign = false):
    copyLen = expprec ? min(maxWidth, len(s)) : len(s)
    npad = max(0, minWidth - copyLen)
    if align == RIGHT:
        if (neg or alwaysSign) and pad == '0': out += s[0]; s = s[1:]; copyLen--      # sign before zeros
        out += pad * npad
    out += s[0:copyLen]
    if align == LEFT: out += pad * npad            # strings/floats/hex CAN be right-padded with '0'

appendInt(n): if align == LEFT and pad == '0': pad = ' '; s = decimal(|n|) with '-' if n<0 else ('+' if alwaysSign); appendString(s, width, 0, pad, align, expprec=false, neg=n<0, alwaysSign)
appendUInt(n): same without sign
append2n(n, bits): s = unsigned base-2^bits digits of n (two's complement for negatives); appendString(s, width, maxWidth=0, pad, align, expprec)
                   # => any explicit precision on %b/%o/%x/%X prints NO digits, only padding [ORACLE "%.2x" -> "", "%5.1x" -> "     "]
appendDouble(x, conv):
    if !(adjusting & PRECISION): precision = 6
    elif precision > 53: notice 'Requested precision of %d digits was truncated to PHP maximum of 53 digits'; precision = 53
    if isnan(x): appendString("NaN", minWidth=3, ...)                      # width ignored
    if isinf(x): s = x < 0 ? "-INF" : "INF"; appendString(s, minWidth=len(s), ..., neg=x<0)   # width ignored
    'e','E','f','F': s = php_conv_fp(conv == 'f' ? 'F' : conv, |x|, precision, '.')   (§ below); prefix '-' if negative else '+' if alwaysSign
    'g','G','h','H': if precision == 0: precision = 1
                     s = zend_gcvt(x, precision, '.', conv in 'GH' ? 'E' : 'e')   # precision -1 -> shortest repr
                     neg = s starts with '-'; if !neg and alwaysSign: '+' prefix
    appendString(s, width, 0, pad, align, expprec=false, neg, alwaysSign)
```

`zval_get_long` of a string: leading-numeric prefix without warnings (`"12abc"`→12, `"abc"`→0, overflowing integer strings
saturate to `PHP_INT_MAX`/`MIN`); floats truncate (NAN → 0 with PHP 8.5 warning `The float NAN is not representable as an int, cast occurred`);
arrays → 0/1; `zval_get_double` of a string: numeric prefix via strtod (`"1e3"`→1000). Decimal point: C locale `.` (PHP
uses the locale's for `f`, `g`, `G`; Twig never sets a locale).

`php_conv_fp` (`PHP/main/snprintf.c:271-360`) with `__cvt` (62-125): `F` → `zend_dtoa(value, mode 3, ndigit=precision)`
(correctly rounded decimal of the exact binary value; exact ties → even), zero-padded to `precision` decimals; leading
`0.` + zeros when the decimal point is ≤ 0. `e`/`E` → `zend_dtoa(value, mode 2, ndigit=precision+1)`; output
`d[.ddd]e±X` with the exponent in decimal **without zero padding** (`1.234568e+3`, `1.200000E-4`, `0.000000e+0`).
`zend_gcvt` (`PHP/Zend/zend_strtod.c:4509-4600`): `zend_dtoa(value, mode 2, ndigit)` (mode 0, 17 digits if ndigit < 0);
E-style if `decpt > ndigit` or `decpt < -3`, mantissa `d.ddd` (adds `.0` when single digit), exponent `e+N`/`e-N` no padding;
fixed style otherwise (trailing zeros removed because dtoa strips them).
[ORACLE] `%g` of 100000 → `100000`, 1000000 → `1.0e+6`, 0.0001 → `0.0001`, 0.00001 → `1.0e-5`; `%G` 1e-10 → `1.0E-10`;
`%.3g` 3.14159 → `3.14`; `%.0e` 12345 → `1e+4`; `%E` 0.000123 → `1.230000E-4`; `%.2f` 1.005 → `1.00`; `%.1f` 0.05 → `0.1`;
`%.0f` 0.5 → `0`, 1.5 → `2`, 2.5 → `2`; `%-05d` 42 → `42   `; `%05.1f` 3.14159 → `003.1`; `%+05d` -7 → `-0007`;
`%'#10.3f` 2.5 → `#####2.500`; `%c` 321 → `A`; `%5.1s` "x" → `    x`; `%.3s` "abcdef" → `abc`; `%2$s %1$s` (255,"x"…) works;
`%f` of NAN → `NaN`, `%e` INF → `INF`, `%5.1f` -INF → `-INF`, `%d` NAN → `0` (+warning).

### 12.9 `round($num, $precision)` (HALF_UP)

`PHP/ext/standard/math.c:39-50` (`php_intpow10`), `53-65`, `69-160` (`php_round_helper`), `168-238` (`_php_math_round`),
`339-393` (`round`). The Twig filter always passes a float.

```
_php_math_round(value, places):                     # mode HALF_UP
    if !finite(value) or value == 0.0: return value
    if places == 0 and value == trunc(value): return value
    places = max(places, INT_MIN + 1)
    exponent = |places| <= 22 ? EXACT_POW10[|places|] : pow(10.0, |places|)
    if value >= 0: tmp = floor(places > 0 ? value * exponent : value / exponent); tmp2 = tmp + 1.0
    else:          tmp = ceil (places > 0 ? value * exponent : value / exponent); tmp2 = tmp - 1.0
    if (places > 0 ? tmp2 / exponent : tmp2 * exponent) == value: tmp = tmp2     # pre-rounding fix (e.g. 0.285)
    if |tmp| >= 1e16: return value
    edge = places > 0 ? |(tmp + copysign(0.5, tmp)) / exponent| : |(tmp + copysign(0.5, tmp)) * exponent|
    if |value| >= edge: tmp = tmp + copysign(1.0, tmp)                          # away from zero
    if |places| < 23: return places > 0 ? tmp / exponent : tmp * exponent
    buf = snprintf("%15fe%d", tmp, -places); tmp = zend_strtod(buf); return finite(tmp) ? tmp : value
```

`round()` itself: precision `int` (`places` clamped to int range); an int argument with `places >= 0` returns it as float.
[ORACLE] `round(2.675, 2)` → 2.68; `round(1.005, 2)` → 1.01; `round(-1.5)` → -2; `round(0.49999999999999994)` → 0;
`round(5.0449999999999999, 2)` → 5.05; `round(1234567.891, -3)` → 1235000; `round(-0.4)` → `-0`.
Twig's `ceil`/`floor` methods use PHP `ceil()`/`floor()` (`math.c:275-310`; int input → float) on `value * 10 ** precision`.

### 12.10 `number_format($num, $decimals, $dec_point, $thousands_sep)`

`PHP/ext/standard/math.c:1108-1224` (float), `1226-1343` (int), `1345-1395` (entry). Twig always passes a float, so:

```
number_format(d, dec, dp, ts):                      # dp/ts: arbitrary byte strings, may be '' or multi-byte
    if (d >= 2^52 or d <= -2^52) and d fits in int64: return format_long((int) d, dec, dp, ts)
    dec = clamp to int
    neg = d < 0; d = |d|
    d = _php_math_round(d, dec)                      # §12.9 (also for negative dec)
    dec = max(0, dec)
    s = spprintf("%.{dec}F", d)                       # php_conv_fp 'F'; NAN -> "nan", INF -> "inf"
    if !isdigit(s[0]): return s                       # "nan"/"inf": sign dropped
    if neg and d == 0: neg = false                    # no "-0"
    int part = s up to '.', insert ts every 3 digits from the right; if dec: dp . fraction (zero-padded to dec)
    return (neg ? '-' : '') . result

format_long(n, dec, dp, ts):
    neg = n < 0; t = |n| (unsigned)
    if dec < 0: if -dec > 19: t = 0 else: p = 10^-dec; rest = t % p; t = (t / p) * p + (rest >= p/2 ? p : 0); if t == 0: neg = false
    digits grouped with ts; if dec > 0: dp . '0' * dec; sign
```

### 12.11 `asort` / `uasort` and `zend_sort`

`PHP/ext/standard/array.c:89-104` (stable fallback), `286-309` (`php_array_data_compare_unstable_i` = `zend_compare`
plus enum grouping), `649-658` (`php_get_long`), `754-770` (`asort`), `837-872` (user compare), `894-939` (`php_usort`/`uasort`);
`PHP/Zend/zend_hash.c:3201-3270` (`zend_hash_sort_internal`); `PHP/Zend/zend_sort.c:24-306`.

- Before sorting, each element's original position is stored (`Z_EXTRA = i`); the comparator falls back to that position when
  the user/standard comparison returns 0 → **stable**. Keys are kept (no renumbering) for both `asort` and `uasort`.
- `uasort` sorts a copy of the array; comparator result → `php_get_long` (`zval_get_long`: floats truncated, strings
  numeric-prefix, `null`→0), normalised to -1/0/1. Bool result → deprecation once per call + retry with swapped args when false.
- `zend_sort(base, n, cmp)`: if `n <= 16` → `zend_insert_sort` (special networks for 2–5 elements, `zend_sort_2..5`; for 6–16
  plain insertion for the first 6 elements, then insertion with a stride-2 backward search — note one branch calls
  `cmp(i, j)` with swapped arguments; port verbatim); else quicksort: pivot = middle element; median of
  (first, pivot, last) via `zend_sort_3`, or of 5 elements (first, first+delta, pivot, pivot+delta, last) via `zend_sort_5`
  when `n >= 1024` (`delta = (n/2 >> 1)` elements); move pivot to index 1; Hoare-style partition with `cmp(pivot, i) > 0` /
  `cmp(j, pivot) > 0`; recurse on the smaller side, loop on the larger. Port exactly if results for inconsistent comparators
  (mixed types) must match.

### 12.12 mbstring: `mb_strtoupper`, `mb_strtolower`, `mb_convert_case(MB_CASE_TITLE)`, `mb_substr`, `mb_strlen`, `mb_ord`

`PHP/ext/mbstring/php_unicode.c:284-475` (`php_unicode_convert_case`), `PHP/ext/mbstring/mbstring.c:1813-1878`
(`mb_get_strlen`), `2131-2187` (`mb_get_substr`), `2360-2412` (`mb_substr`), `PHP/ext/mbstring/libmbfl/filters/mbfilter_utf8.c:446-530`
(UTF-8 decoder).

- Decoding UTF-8 (`mb_utf8_to_wchar`): each malformed sequence yields **one** error code point, consuming: lone
  `0x80–0xC1`/`≥0xF5` byte → 1 byte; 2-byte lead + non-continuation → 1 byte; 3-byte lead: second byte must be a continuation and
  respect `E0 → A0–BF`, `ED → 80–9F`, else error consuming only the lead; valid 2nd but bad 3rd → error consuming 2 bytes;
  truncated at end → error consuming the lead plus available valid continuation bytes; 4-byte leads analogous (`F0 → 90–BF`,
  `F4 → 80–8F`). Error code points are re-encoded as the substitution character `?` (0x3F) by
  `mb_strtoupper/lower/convert_case/substr`.
  [ORACLE] `"a\xe2\x82b"|upper` → `A?B`; `"a\xc0\xafb"` → `A??B`; `"a\xed\xa0\x80b"` → `A???B`; `"a\xf4\x90\x80\x80b"` → `A????B`;
  `"\xf0\x9f\x98"` → `?`.
- `mb_strlen` (UTF-8): fast path counts non-continuation bytes when the string is known-valid; otherwise counts decoded code
  points incl. error markers [ORACLE `"a\xed\xa0\x80b"` → 5, `"a\xc0\xafb"` → 4].
- `mb_substr(str, start, length, enc)`: if `start < 0` or `length < 0`, compute `mblen`; `start < 0` → `max(0, mblen + start)`;
  `length === null` → to the end; `length < 0` → `(mblen - start) + length` if positive else 0; result built by decoding and
  re-encoding the selected code points (invalid bytes inside the range become `?`). `start == PHP_INT_MIN` →
  `mb_substr(): Argument #2 ($start) must be between -9223372036854775807 and 9223372036854775807`.
- Case conversion (full mappings from UnicodeData + SpecialCasing (unconditional entries) + CaseFolding; properties
  `Cased`/`Case_Ignorable` from DerivedCoreProperties; Unicode version of the tables UNVERIFIED):
  - UPPER/LOWER: per code point, may expand to up to 3 code points (`ß`→`SS`, `ﬁ`→`FI`, `İ`→`i̇`, `ŉ`→`ʼN`).
  - LOWER final sigma: `Σ` (U+03A3) → `ς` (U+03C2) when preceded (skipping case-ignorables) by a cased letter and **not**
    followed (skipping case-ignorables) by a cased letter; otherwise `σ` [ORACLE `ΣΑΣ` → `σας`].
  - TITLE: `title_mode = false`; for each code point `w`: output `title_mode ? lower(w) : title(w)` (title mapping, e.g.
    `ǆ`→`ǅ`, `ß`→`Ss`), final-sigma rule applies in lower mode; then if `w` is not Case_Ignorable: `title_mode = is_cased(w)`.
    So after a digit/space/`_`/`-` (not cased, not ignorable) the next letter is title-cased; after `'`, `.`, `:` and other
    case-ignorables the mode is unchanged (`o'neil` → `O'neil`, `a.b` → `A.b`, `3rd` → `3Rd`, `x_y` → `X_Y`) [ORACLE].
- `mb_ord($char, 'UTF-8')` → code point (used by the escapers on validated UTF-8).
- Invalid encoding name → `ValueError('mb_strtoupper(): Argument #2 ($encoding) must be a valid encoding, "X" given')` (arg number
  per function: `mb_strlen` #2, `mb_strtolower` #2, `mb_substr` #4, `mb_convert_case` #3) [ORACLE first two].

### 12.13 `range($start, $end, $step = 1)` (PHP 8.3+ semantics)

`PHP/ext/standard/array.c:2891-3249`:

```
range(start, end, step = 1):                       # start/end: int|float|string; step: int|float
    stepDouble = false; stepNeg = false
    if step is float: reject INF/NAN (ValueError 'range(): Argument #3 ($step) must be a finite number, INF provided' / 'NAN provided');
                      if step < 0: stepNeg = true; step = -step
                      stepLong = (int) step; if (float) stepLong != step: stepDouble = true
    else: if step < 0: (PHP_INT_MIN -> ValueError 'range(): Argument #3 ($step) must be greater than -9223372036854775808'); stepNeg = true; step = -step
    if step == 0: throw ValueError('range(): Argument #3 ($step) cannot be 0')
    classify(input, argNo):                       # -> LONG | DOUBLE | STRING | DIGITCHAR (single-byte digit string)
        int -> LONG; float -> DOUBLE (INF/NAN -> ValueError 'range(): Argument #N ($start|$end) must be a finite number, INF provided')
        string '' -> warning 'range(): Argument #N ($x) must not be empty, casted to 0' -> LONG 0
        numeric string (PHP rules, whitespace allowed) -> float form: DOUBLE; integer form: length 1 ? DIGITCHAR : LONG
        other string: if length != 1: warning 'range(): Argument #N ($x) must be a single byte, subsequent bytes are ignored'; -> STRING (first byte)
    if either is STRING or DIGITCHAR:
        if one side is LONG/DOUBLE (not string-ish):
            if the numeric side is start and end is not DIGITCHAR: warning 'range(): Argument #1 ($start) must be a single byte string if argument #2 ($end) is a single byte string, argument #2 ($end) converted to 0'
            (mirror for end: 'range(): Argument #2 ($end) must be a single byte string if argument #1 ($start) is a single byte string, argument #1 ($start) converted to 0')
            treat both as numbers (string side = its numeric value or 0) -> numeric path
        elif stepDouble:
            if either is STRING (non-digit): warning 'range(): Argument #3 ($step) must be of type int when generating an array of characters, inputs converted to 0'
            treat both as numbers -> numeric path
        else: CHARACTER range on the first bytes low/high (unsigned):
            low > high: if low - high < step -> boundary error; emit low, low-step, … while >= high (stop if next < 0)
            high > low: if stepNeg -> negative-step error; if high - low < step -> boundary error; emit low, low+step, … while <= high (stop if > 255)
            equal: [chr(low)]
            elements are 1-byte STRINGS (also for digits: range("1","3") = ["1","2","3"])
    numeric path:
        if either DOUBLE or stepDouble: float range
            start > end: if start - end < step -> boundary; size = round((start-end)/step + 1); elements start - i*step while >= end
            end > start: stepNeg -> error; if end - start < step -> boundary; elements start + i*step while <= end (i < size)
            equal: [start (float)]
        else int range: analogous with exact integer arithmetic; size check against HT_MAX_SIZE:
            ValueError 'The supplied range exceeds the maximum array size by %d elements: start=%d, end=%d, step=%d. Calculated size: %d. Maximum size: %d.'
    errors: boundary -> ValueError('range(): Argument #3 ($step) must be less than the range spanned by argument #1 ($start) and argument #2 ($end)')
            negative step on increasing range -> ValueError('range(): Argument #3 ($step) must be greater than 0 for increasing ranges')
```

Note decreasing ranges accept positive *or* negative steps; increasing ranges reject negative steps. Float element values are
computed as `start ± i * step` (not accumulated).

### 12.14 `json_encode` (flags 0)

`PHP/ext/json/json_encoder.c` (double: `79-92`):
- `null` → `null`; bools → `true`/`false`; ints decimal; floats `zend_gcvt(d, serialize_precision=-1 → 17/shortest, '.', 'e')`
  (`0.1`, `1.0e+25`, `-0`, `100`); NAN/INF → failure (`false`, error `Inf and NaN cannot be JSON encoded`).
- strings: must be valid UTF-8 else failure; escapes `"`→`\"`, `\`→`\\`, `/`→`\/`, `\b \f \n \r \t`, other `< 0x20` →
  `\u00XX` (lowercase hex), non-ASCII → `\uXXXX` lowercase hex (surrogate pairs above BMP), `0x7F` raw. U+2028/2029 escaped
  (`\u2028`) unless `JSON_UNESCAPED_LINE_TERMINATORS` is combined with `JSON_UNESCAPED_UNICODE` [ORACLE].
- arrays: list (keys exactly 0..n-1 in order, incl. empty) → `[…]`; otherwise object `{"k":v}` with int keys as strings.
- objects: `JsonSerializable` → `jsonSerialize()` result; enums: backed → value, unit → failure; other objects → public
  properties as an object (empty → `{}`).
- options seen in templates: `JSON_PRETTY_PRINT` (4-space indent, `": "`, empty arrays `[]`, empty objects `{}`),
  `JSON_UNESCAPED_SLASHES`, `JSON_UNESCAPED_UNICODE`, `JSON_HEX_TAG/AMP/APOS/QUOT` (`\u003C`, `\u0026`, `\u0027`, `\u0022`
  uppercase hex), `JSON_FORCE_OBJECT`, `JSON_PRESERVE_ZERO_FRACTION`, `JSON_PARTIAL_OUTPUT_ON_ERROR`, `JSON_THROW_ON_ERROR`
  (throws `JsonException`, wrapped by Twig). Default depth 512.
- [ORACLE] input bytes `00 01 1F 7F 09 0A 0D 08 0C 22 5C 2F` followed by `é`, `😀`, U+2028 → `"\u0000\u0001\u001f<DEL>\t\n\r\b\f\"\\\/\u00e9\ud83d\ude00\u2028"` (`<DEL>` = the raw byte 7F);
  `[[[1]]]|json_encode(0, 2)` → `` (depth failure).

### 12.15 Randomness: `mt_rand`, `array_rand`, `shuffle`

`PHP/ext/random/random.c:80-133` (`php_random_range32`), `292-301`, `454-479`, `523-545`; `PHP/ext/standard/array.c:3253-3330`
(shuffle), `6355-6500` (`php_array_pick_keys`).
- Engine: MT19937 (correct variant, `MT_RAND_MT19937`), seeded lazily from a CSPRNG unless `mt_srand()` was called.
- `mt_rand()` → `next32() >> 1`. `mt_rand(min, max)` (`max < min` → ValueError
  `mt_rand(): Argument #2 ($max) must be greater than or equal to argument #1 ($min)`) → `min + range32(max - min)` where
  `range32(umax)`: `r = next32()`; if `umax == 0xFFFFFFFF` return r; `umax++`; if power of two return `r & (umax-1)`;
  `limit = 0xFFFFFFFF - (0xFFFFFFFF % umax) - 1`; while `r > limit`: `r = next32()` (up to 50 attempts, then
  `BrokenRandomEngineError('Failed to generate an acceptable random number in 50 attempts')`); return `r % umax`
  (64-bit variant for spans > 2^32).
- `array_rand($a, 1)`: if fewer than half the hash slots are used → `i = range(0, count-1)` then linear scan; else repeatedly
  pick a random slot index until a non-empty one is found; returns the key.
- `shuffle`: re-indexes, then Fisher–Yates from the end: for `n_left = n-1 … 1`: `j = range(0, n_left)`; swap.
- Exact sequences can only be reproduced with an explicit seed and an identical MT19937 port; Twig templates never seed.

### 12.16 Dates: `DateTime::format`, `DateInterval::format`, construction

`PHP/ext/date/php_date.c:636-648` (`english_suffix`), `672-853` (`date_format`), `4957-5030` (`date_interval_format`).

`DateTimeInterface::format` characters (anything else is copied; `\` copies the next byte literally):

| char | output | char | output |
|---|---|---|---|
| `d` | day `%02d` | `D` | `Mon`…`Sun` |
| `j` | day | `l` | `Monday`… |
| `S` | `st`/`nd`/`rd`/`th` (day 10–19 → `th`, else by last digit 1/2/3) | `w` | 0 (Sun)…6 |
| `N` | 1 (Mon)…7 | `z` | day of year 0-based |
| `W` | ISO week `%02d` | `o` | ISO year |
| `F` | `January`… | `m` | month `%02d` |
| `M` | `Jan`… | `n` | month |
| `t` | days in month | `L` | leap 1/0 |
| `y` | `%02d` of year % 100 | `Y` | `-`? + at least 4 digits |
| `x` | like `Y`, `+` prefix if ≥ 10000 | `X` | always sign (`+2024`) |
| `a`/`A` | `am`/`pm`, `AM`/`PM` | `B` | Swatch beat `%03d` |
| `g`/`G` | 12h / 24h hour | `h`/`H` | `%02d` 12h / 24h |
| `i` | minutes `%02d` | `s` | seconds `%02d` |
| `u` | microseconds `%06d` | `v` | milliseconds `%03d` |
| `e` | zone id (`Europe/Paris`), abbreviation for abbr-type zones, `+05:30` for offset zones | `I` | DST 1/0 |
| `O` | `+0200` | `P` | `+02:00` |
| `p` | like `P` but `Z` for UTC/`+00:00` | `T` | abbreviation (`CET`, `EST`), `GMT+0530` for offset zones |
| `Z` | offset seconds | `c` | `2024-07-04T09:06:07+00:00` |
| `r` | `Thu, 04 Jul 2024 09:06:07 +0000` | `U` | Unix timestamp |

[ORACLE] `DateTime('2024-07-04 05:06:07.123456', America/New_York)` converted to UTC with
`"d D j l N S w z W F m M n t L o X x Y y a A B g G h H i s u v e I O P p T Z c r U"` →
`04 Thu 4 Thursday 4 th 4 185 27 July 07 Jul 7 31 1 2024 +2024 2024 2024 24 am AM 420 9 9 09 09 06 07 123456 123 UTC 0 +0000 +00:00 Z UTC 0 2024-07-04T09:06:07+00:00 Thu, 04 Jul 2024 09:06:07 +0000 1720083967`.

`DateInterval::format` (`%` + char; unknown `%x` → copied as `%x`; trailing `%` → dropped):
`%Y` years `%02d`, `%y` years, `%M`/`%m` months, `%D`/`%d` days, `%H`/`%h` hours, `%I`/`%i` minutes, `%S`/`%s` seconds,
`%F` microseconds `%06d`, `%f` microseconds, `%a` total days (or `(unknown)`), `%R` `+`/`-`, `%r` `-` or ``, `%%` `%`.

Construction (`new DateTime($s[, $tz])`, `setTimezone`, `modify`) is timelib (`PHP/ext/date/lib/`); errors used above:
`Failed to parse time string (%s) at position %d (%c): %s` (`DateMalformedStringException`, PHP 8.3+, `php_date.c:2433`),
`DateTimeZone::__construct(): Unknown or bad timezone (%s)`, `DateTime::modify(): Failed to parse time string …`.
`@<ts>` creates an offset-type zone `+00:00`. The internal properties exposed by `(array) $dt` (used by Twig's attribute
special case): `date` (`Y-m-d H:i:s.u`), `timezone_type` (1 offset, 2 abbreviation, 3 identifier), `timezone`
[ORACLE `d.date` → `2024-01-11 15:17:05.000000`, `d.timezone` → `Europe/Paris`, `d.timezone_type` → `3`].

### 12.17 Array helpers

- `array_slice($a, $offset, $length = null, $preserve = false)`: `offset > count` → `[]`; negative offset → `max(0, count+offset)`;
  `length === null` → rest; negative length → stop `|length|` before the end (empty if not positive); string keys always kept,
  int keys renumbered unless `$preserve`.
- `array_chunk($a, $size, $preserve)`: `size < 1` → `ValueError('array_chunk(): Argument #2 ($length) must be greater than 0')`.
- `array_merge(...)`: int keys renumbered/appended, string keys overwritten in place.
- `array_reverse($a, $preserve)`: reverses order; string keys kept; int keys renumbered unless `$preserve`.
- `array_filter($a, $cb, ARRAY_FILTER_USE_BOTH)`: calls `$cb($value, $key)`, keeps entries whose result is truthy (PHP
  `zend_is_true`), keys preserved.
- `array_keys($a)`: keys in order (numeric-string keys are ints).
- `array_is_list($a)`: true iff keys are `0..n-1` in order.
- `iterator_to_array($it, $preserve)`: see 1.8 key rules; non-preserve appends.
- `in_array($v, $a, true)`: identity comparison.
- `CallbackFilterIterator(IteratorIterator($t), $cb)`: lazy; `$cb($current, $key, $iterator)`; keys preserved.
- `LimitIterator($it, $offset, $count = -1)`: seeks to `$offset` (throws `OutOfBoundsException` if past the end — caught by
  Twig's `slice`), yields up to `$count` items (-1 = all).

### 12.18 `iconv($from, $to, $s)`

Platform iconv (glibc in the oracle image). On failure returns `false` + warning (`iconv(): Wrong encoding, conversion from "%s" to "%s" is not allowed`,
`iconv(): Detected an illegal character in input string`, `iconv(): Detected an incomplete multibyte character in input string`).
Charset aliases, `//TRANSLIT` and `//IGNORE` behaviour are platform-specific (§13).


---

## 13. Non-portable / environment-dependent behaviour

Items a Rust port cannot reproduce byte-for-byte without emulating PHP internals, or that depend on PHP configuration.
Each needs an explicit design decision.

1. **PHP-internal names in error messages** — any PHP `TypeError`/`ArgumentCountError` raised for a *userland* method or a
   compiled closure contains `called in <php file> on line <n>` or the closure name
   `__TwigTemplate_<xxh128>::{closure:…::doDisplay():44}()` and PHP file paths: e.g. `invoke` with a non-Closure
   (`Twig\Extension\CoreExtension::invoke(): Argument #1 ($arrow) must be of type Closure, string given, called in /twig/src/Environment.php(444) : eval()'d code on line 44`),
   arrow functions called with too few arguments, `matches` with an array operand, `include_only` with non-array variables,
   `include(null)`, `block(name, null)`, `render_sandboxed` with a non-array context [ORACLE]. Internal-function TypeErrors
   (`abs(): Argument #1 ($num) must be of type int|float, string given`) *are* portable.
2. **`dump()`**: object handle numbers (`object(C)#3403`), closure pseudo-properties (`name`, `file`, `line`), and the HTML-safety
   decision that depends on `PHP_SAPI`/xdebug (`is_safe = ['html']` under CLI) [SRC DebugExtension.php:24-29].
3. **Randomness** (`random`, `shuffle`): unseeded MT19937 state; reproducible only with `mt_srand` + exact MT19937/range ports.
4. **iconv** (`convert_encoding`, non-UTF-8 charsets in `escape`, `reverse`, `shuffle`, `random`): charset names/aliases,
   `//TRANSLIT`, `//IGNORE`, and error detection are platform (glibc) behaviour.
5. **Timezone database and "now"**: `date`, `date_modify`, `date()` depend on the tz database version (DST rules), PHP's
   `date.timezone` default (UTC when unset, with no warning in PHP 8), the current time, and timelib's free-form parser
   (`"1700000000.5"` → `1699-11-30 00:05:00`; `"1.5"` → today 01:05) [ORACLE].
6. **`precision` / `serialize_precision` ini** (14 / -1) shape every float-to-string conversion; PHP defaults assumed.
7. **Locale**: `sprintf` `%f/%g/%G` use the C locale decimal point; Twig never calls `setlocale`.
8. **Unicode tables**: mbstring case mapping/properties are from php-src's generated `unicode_data.h` (version UNVERIFIED);
   PCRE2 version for `matches`, `/./us` splitting and escaper regexes.
9. **Generated names**: `template_from_string` uses `xxh128` of the source → port XXH3-128; compiled class names
   (`__TwigTemplate_…`) only matter for (1).
10. **PHP warnings/deprecations** are not errors in Twig (rendering continues with PHP's fallback value); the port should
    surface them via a side channel if it wants parity with logs.
11. **Object model**: `getAttribute` relies on PHP reflection (public/initialised properties, `__isset`, `__get`, `__call`,
    method name case-insensitivity, `get_class_methods` order + `sort`). A Rust port needs an equivalent "object" trait.
12. **Frameless `min`/`max` tie-breaking** for exactly two arguments (§5.10) — easy to miss.
13. **Sort algorithm**: results for inconsistent comparators (mixed-type `zend_compare`, user arrows) depend on `zend_sort`.

---

## 14. Summary table

Legend — Opt: `env` needs_environment, `ctx` needs_context, `cs` needs_charset, `sb` needs_is_sandboxed, `var` is_variadic,
`pe` pre_escape, `dep` deprecated, `nc` custom node_class, `pc` parser_callable. Safe: compile-time safety of the output.
SB: always allowed in sandbox (core 3.31.1).

### 14.1 Filters

| name | PHP callable | Opt | Safe | SB | Key behaviour / errors |
|---|---|---|---|---|---|
| `abs` | `abs` | — | — | no | int\|float param; `TypeError` for non-numeric |
| `batch` | `CoreExtension::batch` | — | — | no | `ceil(size)`; `array_chunk`; fill last row; `expects a sequence or a mapping` |
| `capitalize` | `CoreExtension::capitalize` | cs | — | no | upper(first cp) . lower(rest) (mbstring full mapping) |
| `column` | `CoreExtension::column` | env sb | — | no | `array_column`; sandbox property checks |
| `convert_encoding` | `CoreExtension::convertEncoding` | — | — | no | `iconv(from, to, s)`; false → `''` |
| `date` | `$core->formatDate` | — | — | no | default `F j, Y H:i` / `%d days`; `convertDate` |
| `date_modify` | `$core->modifyDate` | — | — | no | `convertDate(d, false)->modify(m)` |
| `default` | `CoreExtension::default` | nc `DefaultFilter` | — | no | `testEmpty`; defined-test rewrite for variables/attributes |
| `e`, `escape` | `EscaperRuntime::escape` | nc `EscapeFilter` | callback: `[strategy]`/`['html']`/`[]` | no | strategies html, js, css, html_attr, html_attr_relaxed, url, custom; `Invalid escaping strategy …` |
| `filter` | `CoreExtension::filter` | env sb | — | no | `array_filter(..., USE_BOTH)` or lazy `CallbackFilterIterator`; closure check |
| `find` | `CoreExtension::find` | env sb | — | no | first value with truthy arrow(v,k), else null |
| `first` | `CoreExtension::first` | cs | — | no | `slice(0,1)`; `[]` → `false` |
| `format` | `CoreExtension::sprintf` | — | — | no | PHP `sprintf(format ?? '', ...)` |
| `invoke` | `CoreExtension::invoke` | — | — | no | `\Closure` input, `arrow(...args)` |
| `join` | `CoreExtension::join` | — | — | no | non-iterables wrapped; `and` separator; `implode` |
| `json_encode` | `json_encode` | — | — | no | flags/depth; failure → `''` |
| `keys` | `CoreExtension::keys` | — | — | no | `array_keys` / iterator keys; non-iterable → `[]` |
| `last` | `CoreExtension::last` | cs | — | no | `slice(-1,1)`; `[]` → `false` |
| `length` | `CoreExtension::length` | cs | — | no | null 0; scalars `mb_strlen`; Countable/array count; Traversable `iterator_count`; Stringable; else 1 |
| `lower` | `CoreExtension::lower` | cs | — | no | `mb_strtolower` (final sigma) |
| `map` | `CoreExtension::map` | env sb | — | no | `r[k] = arrow(v, k)` |
| `merge` | `CoreExtension::merge` | — | — | no | `array_merge` over `toArray` of each; `… for argument %d.` |
| `nl2br` | `CoreExtension::nl2br` | pe `html` | `['html']` | no | PHP `nl2br` (`<br />`) |
| `number_format` | `$core->formatNumber` | — | — | no | defaults `0`, `.`, `,`; `number_format((float) n, …)` |
| `raw` | — | nc `RawFilter` | `['all']` | no | identity; args ignored |
| `reduce` | `CoreExtension::reduce` | env sb | — | no | `acc = arrow(acc, v, k)` from `initial` |
| `replace` | `CoreExtension::replace` | — | — | no | `strtr(str ?? '', toArray(from))` |
| `reverse` | `CoreExtension::reverse` | cs | — | no | `array_reverse`; strings per code point |
| `round` | `CoreExtension::round` | — | — | no | common/ceil/floor; returns float |
| `shuffle` | `CoreExtension::shuffle` | cs | — | no | random; strings per code point; keys dropped |
| `slice` | `CoreExtension::slice` | cs | — | no | `LimitIterator` / `array_slice` / `mb_substr` |
| `sort` | `CoreExtension::sort` | env sb | — | no | `asort` / `uasort` (stable, keys kept) |
| `spaceless` | `CoreExtension::spaceless` | pe `html`, dep 3.12 | `['html']` | no | `trim(preg_replace('/>\s+</', '><', s))` |
| `split` | `CoreExtension::split` | cs | — | no | `explode` or character chunks |
| `striptags` | `CoreExtension::striptags` | — | — | no | PHP `strip_tags` |
| `title` | `CoreExtension::titleCase` | cs | — | no | `mb_convert_case(MB_CASE_TITLE)` |
| `trim` | `CoreExtension::trim` | — | — | no | sides both/left/right; Markup kept for default mask |
| `upper` | `CoreExtension::upper` | cs | — | no | `mb_strtoupper` |
| `url_encode` | `CoreExtension::urlencode` | — | — | no | arrays → `http_build_query(…, RFC3986)`, else `rawurlencode` |

### 14.2 Functions

| name | PHP callable | Opt | Safe | SB | Key behaviour / errors |
|---|---|---|---|---|---|
| `attribute` | — | pc | — | non-strict only (dep 3.27) | → `GetAttrExpression` (ANY_CALL) |
| `block` | — | pc | `['all']` (node) | non-strict only (dep 3.27) | `renderBlock` / `hasBlock`; string result |
| `constant` | `CoreExtension::constant` | — | — | no | global/class constants, `('class', obj)`, `is defined` support |
| `cycle` | `CoreExtension::cycle` | — | — | no | `values[position % count]` |
| `date` | `$core->convertDate` | — | — | no | DateTime conversion rules (§5.5) |
| `dump` | `DebugExtension::dump` | env ctx var | `['html']` under CLI | no | debug only; `var_dump` |
| `enum` | `CoreExtension::enum` | nc `EnumFunction` | — | no | first case; compile-time validation of literals |
| `enum_cases` | `CoreExtension::enumCases` | nc `EnumCasesFunction` | — | no | `Enum::cases()` |
| `include` | `CoreExtension::include` | env ctx | `['all']` | no | `Markup` result; `sandboxed` arg deprecated 3.29 |
| `include_only` | `CoreExtension::includeOnly` | env | `['all']` | no | no context |
| `max` | `max` | — | — | no | PHP semantics; frameless 2-arg tie → lhs |
| `min` | `min` | — | — | no | PHP semantics; frameless 2-arg tie → rhs |
| `parent` | — | pc | `['all']` (node) | non-strict only (dep 3.27) | `renderParentBlock`; args ignored |
| `random` | `CoreExtension::random` | cs | — | no | `mt_rand` / `array_rand` |
| `range` | `range` | — | — | no | PHP 8.3+ `range`; named `low`/`high`/`step` |
| `render_sandboxed` | `SandboxBridgeRuntime::render` | nc `RenderSandboxedFunction` | `[output_strategy]` | no | SandboxBridgeExtension |
| `source` | `CoreExtension::source` | env | `['all']` | no | loader source code |
| `template_from_string` | `StringLoaderExtension::templateFromString` | env | — | no | `createTemplate`, xxh128 names |

### 14.3 Tests

| name | implementation | one_mandatory_argument | SB | Semantics |
|---|---|---|---|---|
| `constant` | `ConstantTest` | no | **no** | `x === constant(name)` / `constant(get_class(obj)."::".name)` |
| `defined` | `DefinedTest` | no | yes | existence check (SyntaxError for unsupported nodes) |
| `divisible by` | `DivisiblebyTest` | yes | yes | `0 == x % y` |
| `empty` | `CoreExtension::testEmpty` | no | yes | Countable/Traversable/Stringable/`'' false null []` |
| `even` | `EvenTest` | no | yes | `x % 2 == 0` |
| `iterable` | `is_iterable` (`NonStringCoercingInputTest`) | no | yes | array or Traversable |
| `mapping` | `CoreExtension::testMapping` | no | yes | non-list array or any (non-Traversable) object |
| `none`, `null` | `NullTest` | no | yes | `null === x` |
| `odd` | `OddTest` | no | yes | `x % 2 != 0` |
| `same as` | `SameasTest` | yes | yes | `x === y` |
| `sequence` | `CoreExtension::testSequence` | no | yes | list array (`array_is_list`) |
| `true` | `TrueTest` (internal) | no | yes | returns `x` (Markup → string); used for truthiness |

### 14.4 Operator callables

| operator | runtime | Notes |
|---|---|---|
| `in` / `not in` | `CoreExtension::inFilter` | substring for strings; `compare()` loose-equality for iterables; identity for objects |
| `matches` | `CoreExtension::matches` | `preg_match`; compile-time validation for literals |
| `starts with` / `ends with` | `str_starts_with` / `str_ends_with` | both operands must be strings |
| `has some` / `has every` | `CoreExtension::arraySome` / `arrayEvery` | closure check, sandbox-aware |
| `..` | `range(a, b)` | sandbox: requires function `range` |

### 14.5 Globals

| name | value |
|---|---|
| `_self` | current template name |
| `_context` | current context array |
| `_charset` | environment charset |
| (none registered by core extensions) | user globals via `addGlobal` / `GlobalsInterface` |

---

## 15. Appendix: remaining classes in scope

- `AttributeExtension` (`TWIG/src/Extension/AttributeExtension.php`): builds `TwigFilter`/`TwigFunction`/`TwigTest` from
  `#[AsTwigFilter(name, needsCharset, needsEnvironment, needsContext, needsIsSandboxed, isSafe, isSafeCallback, preEscape, preservesSafety, deprecationInfo, alwaysAllowedInSandbox)]`,
  `#[AsTwigFunction(name, needsCharset, needsEnvironment, needsContext, needsIsSandboxed, isSafe, isSafeCallback, deprecationInfo, alwaysAllowedInSandbox)]`,
  `#[AsTwigTest(name, needsCharset, needsEnvironment, needsContext, needsIsSandboxed, deprecationInfo, alwaysAllowedInSandbox)]`
  on methods (repeatable). `needs_environment` defaults to "first parameter typed `Twig\Environment` and not variadic";
  `is_variadic` = the PHP method is variadic. Errors: `\InvalidArgumentException('"Twig\Extension\AttributeExtension" requires the name of a class with attributes.')`,
  `\InvalidArgumentException('Do not call the parent constructor when extending "Twig\Extension\AttributeExtension", the attributes are read from the extension class itself.')`,
  `\LogicException('"%s::%s()" needs at least %d arguments to be used AsTwigFilter, but only %d defined.')` (also AsTwigFunction/AsTwigTest).
  The registered key is the target class name (`getClass()`); `Environment::getExtension()` of such a class returns its runtime.
- `OptimizerExtension(int $optimizers = -1)`, `ProfilerExtension(Profile $profile)` (`enter`/`leave` maintain a stack of
  profiles), `YieldNotReadyExtension(bool $useYield)` — node visitors only, no callables.
- `StagingExtension` — container for `Environment::addFilter/addFunction/addTest/addTokenParser/addNodeVisitor`
  (duplicate errors in §1.1).
- `Util/CallableArgumentsExtractor` (§1.5), `Util/ReflectionCallable` (resolves `'Class::method'` strings and arrays to a
  reflector; `getTwigParameters()` strips injected parameters; non-callable → `\LogicException('Callback for %s "%s" is not callable in the current scope.')`),
  `Util/CallableParameters` (§9.4), `Util/DeprecationCollector` (`collectDir($dir, $ext = '.twig')`, `collect(\Traversable $iterator)`:
  parses each template collecting `E_USER_DEPRECATED` messages, ignoring `SyntaxError`s), `Util/TemplateDirIterator`
  (yields `file_get_contents(path)` keyed by `(string) path`).
- `Resources/core.php`, `debug.php`, `escaper.php`, `string_loader.php`: legacy global functions (`twig_cycle`, `twig_random`,
  `twig_date_format_filter`, … `twig_var_dump`, `twig_raw_filter`, `twig_escape_filter`, `twig_escape_filter_is_safe`,
  `twig_template_from_string`), all deprecated since 3.9 (`Using the internal "%s" function is deprecated.`) and delegating to
  the static methods above; plus the internal helpers `twig_resolve_caller_source()` (walks the backtrace for the calling
  `Template`'s Source) and `twig_resolve_is_sandboxed(Environment $env)`. Not needed by compiled templates in 3.31.1.
- `CoreExtension` public non-callable API: `setDateFormat`, `getDateFormat`, `setTimezone`, `getTimezone`,
  `setNumberFormat`, `getNumberFormat`, constants `ARRAY_LIKE_CLASSES`; internal helpers `toArray`, `ensureTraversable`
  (`is_iterable ? seq : []`, used by `for`), `destructureSequence`, `captureOutput`, `callMacro`, `getAttribute`, `inFilter`,
  `compare`, `matches`, `arraySome`, `arrayEvery`, `checkArrow`, `parseParentFunction`, `parseBlockFunction`, `parseAttributeFunction`.
