# Twig 3.31.1 — Nodes and the Compiled-Template Runtime Model

Reference for porting PHP Twig 3.31.1 (running on PHP 8.5.13) to Rust (twig-rs).
Scope: every `Twig\Node\*` class and what it compiles to, the compiled template class and its render/yield flow,
context passing, inheritance/blocks/traits/embed/include/macros at runtime, the runtime helpers that generated code
calls (`CoreExtension::getAttribute()` & co.), node visitors, `Environment`, errors, loaders, cache (brief), sandbox (brief).

All paths are relative to `ext/twig-3.31.1/` unless stated. "VERIFIED" means the behaviour was executed in
`php:8.5-cli` (PHP 8.5.13) against the local Twig 3.31.1 sources (harness directory
`<scratch>\rt-work`:
`bootstrap.php` (composer-less autoloader + `trigger_deprecation` collector), `run.sh <script>` (Docker runner),
`t*.php` probes, `gen_appendix.php` / `gen_exprs.php` (regenerate Appendix A); outputs are quoted verbatim).
"UNVERIFIED" marks statements derived from reading only, or not checked at all.

---

## Table of contents

- [0. Conventions and key facts at a glance](#0-conventions-and-key-facts-at-a-glance)
- [1. Pipeline overview](#1-pipeline-overview)
- [2. Structure of a compiled template](#2-structure-of-a-compiled-template)
  - [2.1 File layout](#21-file-layout)
  - [2.2 Class members and constructor](#22-class-members-and-constructor)
  - [2.3 doGetParent()](#23-dogetparent)
  - [2.4 doDisplay()](#24-dodisplay)
  - [2.5 block_NAME() methods](#25-block_name-methods)
  - [2.6 loadDeclaredMacros()](#26-loaddeclaredmacros)
  - [2.7 Trailing methods](#27-trailing-methods)
  - [2.8 Compiler mechanics that matter for porting](#28-compiler-mechanics-that-matter-for-porting)
- [3. Runtime model](#3-runtime-model)
  - [3.1 Entry points and the yield/display/render flow](#31-entry-points-and-the-yielddisplayrender-flow)
  - [3.2 Output model (use_yield, output buffering)](#32-output-model-use_yield-output-buffering)
  - [3.3 How $context travels (by-value copies, merges, restores)](#33-how-context-travels-by-value-copies-merges-restores)
  - [3.4 Globals](#34-globals)
  - [3.5 The for loop: _parent, _seq, _key, _iterated, loop](#35-the-for-loop-_parent-_seq-_key-_iterated-loop)
  - [3.6 Inheritance: parents and blocks](#36-inheritance-parents-and-blocks)
  - [3.7 Traits (use)](#37-traits-use)
  - [3.8 Embed](#38-embed)
  - [3.9 Include (tag, function, include_only, source)](#39-include-tag-function-include_only-source)
  - [3.10 Macros at runtime](#310-macros-at-runtime)
  - [3.11 Template::load() inside templates](#311-templateload-inside-templates)
  - [3.12 Exceptions during rendering: who handles them, which line is reported](#312-exceptions-during-rendering-who-handles-them-which-line-is-reported)
  - [3.13 Per-instance mutable state (observable)](#313-per-instance-mutable-state-observable)
- [4. Node reference](#4-node-reference)
  - [4.1 Base classes and interfaces](#41-base-classes-and-interfaces)
  - [4.2 Statement / structural nodes](#42-statement--structural-nodes)
  - [4.3 Expression nodes: variables and literals](#43-expression-nodes-variables-and-literals)
  - [4.4 GetAttrExpression](#44-getattrexpression)
  - [4.5 Call expressions (function, filter, test) and argument conventions](#45-call-expressions-function-filter-test-and-argument-conventions)
  - [4.6 Special filter / function / test nodes](#46-special-filter--function--test-nodes)
  - [4.7 Binary operators](#47-binary-operators)
  - [4.8 Unary operators](#48-unary-operators)
  - [4.9 Conditional operators: ?:, ??, ternary](#49-conditional-operators---ternary)
  - [4.10 Arrow functions](#410-arrow-functions)
  - [4.11 block(), parent(), attribute(), macro references](#411-block-parent-attribute-macro-references)
  - [4.12 Assignment and destructuring expressions](#412-assignment-and-destructuring-expressions)
  - [4.13 Deprecated / legacy expression nodes](#413-deprecated--legacy-expression-nodes)
- [5. Runtime helpers called by generated code](#5-runtime-helpers-called-by-generated-code)
  - [5.1 CoreExtension::getAttribute() — full algorithm](#51-coreextensiongetattribute--full-algorithm)
  - [5.2 Optimized `[]` access (non-strict mode)](#52-optimized--access-non-strict-mode)
  - [5.3 TrueTest truthiness](#53-truetest-truthiness)
  - [5.4 Other CoreExtension helpers](#54-other-coreextension-helpers)
  - [5.5 EscaperRuntime::escape() and Markup](#55-escaperruntimeescape-and-markup)
- [6. Node visitors](#6-node-visitors)
- [7. Environment](#7-environment)
- [8. Errors](#8-errors)
- [9. Loaders](#9-loaders)
- [10. Cache (brief)](#10-cache-brief)
- [11. Sandbox (brief)](#11-sandbox-brief)
- [12. Appendix A — generated PHP for representative templates](#12-appendix-a--generated-php-for-representative-templates)
- [13. Appendix B — porting notes, divergence traps, UNVERIFIED items](#13-appendix-b--porting-notes-divergence-traps-unverified-items)

---

## 0. Conventions and key facts at a glance

Terminology: *template object* = an instance of the compiled class `__TwigTemplate_<hash>` extending `Twig\Template`
(`src/Template.php`). One instance exists per (environment, template class); it is reused for every render
(`Environment::loadTemplate()` caches in `$this->loadedTemplates[$cls]`, `src/Environment.php:419-421,455`).

Facts that shape the Rust architecture (all VERIFIED unless noted):

1. Output is produced by PHP generators: every display method is `iterable` and `yield`s string chunks. `render()`
   concatenates them (or captures echo output when `use_yield=false`; same result). An exception discards everything
   produced by `render()`.
2. `$context` is a PHP array passed **by value** (copy-on-write) to every display method, block method, included template,
   and macro. Only three constructs share/mutate a caller's context: the capture closure of `{% set x %}…{% endset %}` /
   `{% apply %}` / macro bodies (captured **by reference**, `use (&$context, …)`), the `for` loop (mutates then restores),
   and `with` (saves/restores). Arrow functions capture `$context` **by value at creation time**.
3. PHP array semantics leak into observable output: key order (`_context|keys`), integer key renumbering by
   `array_merge` (include `with`), numeric-string key normalization (`"1"` ≡ `1`).
4. `strict_variables` changes the compiled code (not only the runtime): name access compiles to an existence check, and
   `x[...]` stops using the optimized array access path (§5.2) whose semantics differ from `getAttribute()` for
   `ArrayAccess` objects.
5. The OptimizerNodeVisitor is semantically visible: it removes the `loop` variable when unused (visible through
   `_context`), marks loop targets "always defined" (PHP warning instead of null/strict error in the `else` branch),
   turns `{{ "literal" }}` into text and `{{ block(...) }}` / `{{ parent() }}` into streamed output.
6. Macros: compiled into `Twig\TwigMacro` closures registered per template; called through `Twig\MacroNamespace`.
   The imported namespaces live in `$this->macros` **on the template instance** and persist across renders.
7. Error messages are `rawMessage` + ` in "<template name>"` + ` at line N` (+ ` column C`) with the final `.`/`?` moved
   to the end (§8.2). Lines of errors without an explicit line are *guessed* from the PHP backtrace (§3.12).
8. The integration-test corpus `tests/Fixtures/**/*.test` (488 files) is the conformance suite; harness in
   `src/Test/IntegrationTestCase.php:233-384` (defaults: `strict_variables => true`, `cache => false`, global
   `global => 'global'`, extensions from `tests/IntegrationTest.php:55-66`).

---

## 1. Pipeline overview

`Environment::render($name, $ctx)` → `load($name)` → `getTemplateClass($name)` → `loadTemplate($cls, $name)`:

```
if not class_exists($cls):
    key = cache->generateKey(name, mainCls)
    if !autoReload || isTemplateFresh(name, cache->getTimestamp(key)): cache->load(key)
    if still not loaded:
        source  = loader->getSourceContext(name)               # LoaderError if missing
        content = compileSource(source)                          # tokenize -> parse (+ node visitors) -> compile
        cache->write(key, content); cache->load(key)  (unless hotCache[name])
        if main class still missing: eval('?>'.content)
        if class still missing: RuntimeError('Failed to load Twig template "%s", index "%s": cache might be corrupted.', -1, source)
extensionSet->initRuntime()
return loadedTemplates[cls] = new cls(env)                     # constructor resolves traits eagerly (§3.7)
```
(`src/Environment.php:412-456`). `compileSource()` (`:614-624`): Twig `Error`s get `setSourceContext($source)` and are
rethrown; any other `\Exception` becomes
`SyntaxError('An exception has been thrown during the compilation of a template ("%s").', -1, $source, $e)`.

Parsing: `Parser::parse()` (`src/Parser.php:86-163`) builds
`ModuleNode(BodyNode([$body]), $parent, blocks, MacrosNode(macros), traits, embeddedTemplates, source)` and then runs
`NodeTraverser` with all registered visitors (§6). For child templates (`$parent` set), the body is cleaned:
`BlockReferenceNode`s and blank `TextNode`s at the top level are removed (`cleanupBodyForChildTemplates`, `:617-637`).
Each block is stored as `blocks[name] = BodyNode([BlockNode])` (`Parser::setBlock`, `:334-341`), and the place where the
block appeared in the body becomes a `BlockReferenceNode`.

Compilation: `Compiler::compile(ModuleNode)` (`src/Compiler.php:67-84`) → `ModuleNode::compile()` (§2).

---

## 2. Structure of a compiled template

### 2.1 File layout

One compiled PHP file per root template: `<?php`, a fixed `use` header, the main class, then one extra class per
embedded template (`{% embed %}`), named `<mainClass>___<index>` and compiled *after* the main class without `<?php`/`use`
header (`ModuleNode::compile()` `src/Node/ModuleNode.php:89-96`, `compileTemplate()` `:104-133`).

Fixed header (`compileClassHeader`, `:177-221`):
```php
use Twig\Environment;            use Twig\Error\LoaderError;      use Twig\Error\RuntimeError;
use Twig\Extension\CoreExtension; use Twig\Extension\SandboxExtension; use Twig\MacroNamespace;
use Twig\Markup;                 use Twig\Sandbox\SecurityError;  use Twig\Sandbox\SecurityNotAllowedTagError;
use Twig\Sandbox\SecurityNotAllowedFilterError; use Twig\Sandbox\SecurityNotAllowedFunctionError;
use Twig\Sandbox\SecurityNotAllowedTestError;   use Twig\Source; use Twig\Template; use Twig\TemplateWrapper;
```
then `/* <template name with "*/" replaced by "* /"> */` and `class <getTemplateClass(name, index)> extends Template`.

Method emission order inside a class: properties → `__construct` → `doGetParent` (only if the template has a parent) →
`doDisplay` → `block_*` methods (in block definition order) → `loadDeclaredMacros` (only if macros exist) →
`getTemplateName` → `isTraitable` (only when false) → `getDefaultEscapeStrategy` (only when a strategy is set) →
`getDebugInfo` → `getSourceContext` → `class_end` nodes (sandbox: `checkTraitsAllowed`, `ensureSecurityChecked`,
`checkSecurity`).

### 2.2 Class members and constructor

Properties emitted: `private Source $source;`, `private array $macros = [];` (doc `@var array<string, MacroNamespace>`),
and `private \Twig\Runtime\EscaperRuntime $escaper;` only when the EscaperNodeVisitor set attribute `escaper` (some
`EscapeFilter` uses the template escaper) (`:209-218`). Inherited from `Template` (`src/Template.php:36-47`):
`$parent`, `$parents = []`, `$blocks = []`, `$traits = []`, `$traitAliases = []`, `$extensions = []`, `$sandbox`,
private `$useYield`, `$macroNamespace`, `$loader`, `$sourcePath`.

Constructor (`compileConstructor`, `:223-363`), in order:
1. `constructor_start` nodes (sandbox: `$this->sandbox = $env->getExtension(SandboxExtension::class)->getChecker();`).
2. `parent::__construct($env);` — stores env, `useYield`, `extensions = $env->getExtensions()`, `loader`.
3. `$this->source = $this->getSourceContext();`
4. if escaper used: `$this->escaper = $env->getRuntime('Twig\Runtime\EscaperRuntime');`
5. if no parent: `$this->parent = false;` (so `getParent()` returns false without calling `doGetParent`).
6. if traits: `$this->ensureTraitsAllowed();` then, per trait `$i`, load + checks + alias handling (§3.7), then
   `$this->traits = $_trait_0_blocks;` (one trait) or `array_merge($_trait_0_blocks, …)`; then
   `$this->blocks = array_merge($this->traits, ['name' => [$this, 'block_name'], …]);`
   else `$this->blocks = ['name' => [$this, 'block_name'], …];`
7. `constructor_end` nodes.

`$this->blocks` maps block name → `[Template $owner, string $methodName]`.

### 2.3 doGetParent()

Only when `{% extends %}` (or embed) is present (`compileGetParent`, `:135-175`):
```php
protected function doGetParent(array $context): bool|string|Template|TemplateWrapper
{
    // constant parent:
    return $this->parent ??= $this->load("base.twig", 1);
    // dynamic parent (any non-ConstantExpression), e.g. {% extends layout %}:
    $macros = $this->macros;
    return $this->load(($context["layout"] ?? null), 1);
}
```
The parent expression is evaluated against the **context passed to getParent()**, every time (dynamic case); results
are cached by name in `$this->parents` (§3.6).

### 2.4 doDisplay()

```php
protected function doDisplay(array $context, array $blocks = []): iterable
{
    $macros = $this->macros;          // snapshot of the instance macro registry
    <display_start nodes>             // profiler only
    <body>
    // only when a parent exists:
    $this->parent = $this->load("base.twig", 1);       // only for a constant parent
    yield from $this->parent->unwrap()->yield($context, array_merge($this->blocks, $blocks));
    //   dynamic parent: yield from $this->getParent($context)->unwrap()->yield($context, array_merge($this->blocks, $blocks));
    <display_end nodes>
    return; yield;                    // only when there is no parent (keeps the method a generator)
}
```
(`compileDisplay`, `:365-408`). In a child template the body still executes before delegating to the parent: top-level
`set`, `import`, `from`, `do`, `with`, etc. in a child run first (VERIFIED: `{% extends 'parent' %}{% set x = 'X' %}`
makes `x` visible in blocks and in the parent body).

### 2.5 block_NAME() methods

```php
// line N
/**
 * @return iterable<null|scalar|\Stringable>
 */
public function block_NAME(array $context, array $blocks = []): iterable
{
    $macros = $this->macros;
    <body>
    return; yield;
}
```
(`src/Node/BlockNode.php:31-49`). The method name is `'block_'.$name` with the raw block name.

### 2.6 loadDeclaredMacros()

Emitted only when the template declares macros (`src/Node/MacrosNode.php:50-80`):
```php
protected function loadDeclaredMacros(): array
{
    return [
        "name" => new \Twig\TwigMacro("name", function (<params>, ...$varargs): string|Markup { ... }, <signature>, <variadic bool>),
        ...
    ];
}
```
Closure body: see §3.10 and `MacroNode` (§4.2). `Template::getMacroNamespace()` lazily wraps the array in a
`MacroNamespace` (`src/Template.php:549-552`).

### 2.7 Trailing methods

- `getTemplateName(): string` → `return "<name>";` (always).
- `isTraitable(): bool` → `return false;` only when the template is **not** traitable (§3.7 rule); base returns `true`.
- `getDefaultEscapeStrategy(): string|false` → emitted when the ModuleNode attribute `strategy` is not `false`
  (set by EscaperNodeVisitor to the default strategy string) (`:487-505`).
- `getDebugInfo(): array` → map PHP line → template line (sorted descending) (`:507-519`).
- `getSourceContext(): Source` → `new Source("<code if debug else empty>", "<name>", "<path>")`, or, outside debug mode
  when the path is non-empty, `Source::createWithLazyPath("", "<name>", $this->getSourcePath(...))` (`:521-547`).
  The source code is embedded only in debug mode.

### 2.8 Compiler mechanics that matter for porting

`src/Compiler.php`:
- `string($v)` (`:144-155`): `"` + `addcslashes($v, "\0\t\"\$\\")` with `'` → `\x27` + `"`. PHP-codegen only.
- `repr($v)` (`:162-195`): ints/floats via `var_export` with `serialize_precision=-1`; `null`, `true/false`; arrays as
  `[k => v, …]`; strings via `string()`.
- `getVarName()` → `_v0`, `_v1`, … (`:249-252`), reset per `compile()` (shared across embedded classes in the same file).
- `addDebugInfo($node)` (`:200-213`): writes `// line N` **only if N differs from the last emitted marker** (state shared
  across the whole file, in compile order). Used by: `BlockNode`, `BlockReferenceNode`, `DeprecatedNode`, `DoNode`,
  `ArrowFunctionExpression`, `ObjectDestructuringSetBinary`, `SequenceDestructuringSetBinary`,
  `BlockReferenceExpression` (output mode), `NameExpression`, `ParentExpression` (output mode), `AssignMacroVariable`,
  `FlushNode`, `ForElseNode`, `ForNode`, `IfNode`, `IncludeNode`, `MacroNode`, `ModuleNode` (parent/trait exprs),
  `PrintNode`, `SandboxNode`, `SetNode`, `TextNode`, `WithNode`. These markers drive line guessing (§3.12).
- `checkForEcho()`: deprecation when a node emits `echo`/`print` (legacy, irrelevant for Rust).
- `subcompile($node, $raw = true)`; `write()` indents by `4*indentation` spaces; cosmetic whitespace artefacts such as
  `yield from         $this->unwrap()…` appear in output and are irrelevant.

---

## 3. Runtime model

### 3.1 Entry points and the yield/display/render flow

`Environment` (`src/Environment.php`):
- `render($name, $ctx)` = `load($name)->render($ctx)` (`:351-354`); `display()` = `load($name)->display($ctx)` (`:365-368`).
- `load($name)` (`:379-395`): a `TemplateWrapper` is returned as is after `unwrap($this)` (ownership check); a raw
  `Template` is accepted with a deprecation; otherwise `$this->loadedWrappers[$cls] ??= new TemplateWrapper($this, $this->loadTemplate($cls, $name))`.

`TemplateWrapper` (`src/TemplateWrapper.php`), the public API:

| method | implementation |
|---|---|
| `stream($ctx = [])` (:38) | `yield from $template->yield($ctx)` |
| `streamBlock($name, $ctx = [])` (:46) | `yield from $template->yieldBlock($name, $ctx + globals)` |
| `render($ctx = [])` (:51) | `$template->render($ctx)` |
| `display($ctx = [])` (:56) | `$template->display($ctx, func_get_args()[1] ?? [])` (hidden 2nd arg = blocks) |
| `hasBlock($name, $ctx = [])` (:63) | `$template->hasBlock($name, $ctx + globals)` |
| `getBlockNames($ctx = [])` (:71) | `$template->getBlockNames($ctx + globals)` |
| `renderBlock($name, $ctx = [])` (:76) | `$template->renderBlock($name, $ctx + globals)` |
| `displayBlock($name, $ctx = [])` (:81) | `$ctx += globals; foreach ($template->yieldBlock($name, $ctx) as $d) echo $d;` |
| `unwrap(?Environment $env)` (:124) | throws `RuntimeError('A "Twig\TemplateWrapper" can only be used with the "Twig\Environment" that created it.')` when env differs (no line/source at creation; gets them from the template that catches it — VERIFIED message `... that created it in "unwrap" at line 1.`) |

`Template` (`src/Template.php`):

```php
// :471-482
public function yield(array $context, array $blocks = []): iterable {
    $context += $this->env->getGlobals();          // globals appended, context keys win
    $blocks = array_merge($this->blocks, $blocks);  // caller-supplied blocks override own blocks
    try {
        $this->ensureSecurityChecked();             // no-op unless sandbox (§11)
        yield from $this->doDisplay($context, $blocks);
    } catch (\Throwable $e) {
        $this->handleException($e);                 // §3.12
    }
}
// :431-436
public function display(array $context, array $blocks = []): void { foreach ($this->yield($context, $blocks) as $d) echo $d; }
// :438-466
public function render(array $context): string {
    if (!$this->useYield) {                      // default (use_yield=false)
        $level = ob_get_level();
        ob_start(debug ? null : static fn () => '');
        try { $this->display($context); }
        catch (\Throwable $e) { while (ob_get_level() > $level) ob_end_clean(); throw $e; }
        return ob_get_clean();
    }
    $content = ''; foreach ($this->yield($context) as $d) $content .= $d; return $content;
}
```
`renderBlock()` (:247-275) and `renderParentBlock()` (:204-232) follow the same pattern around `displayBlock()` /
`displayParentBlock()` (:167-190), which echo `yieldBlock()` / `yieldParentBlock()`.

Rust mapping: one evaluation entry point producing chunks into a sink; `render` = collect into a `String`, discarding on
error; `display`/`stream` = forward to the caller's sink.

### 3.2 Output model (use_yield, output buffering)

- Option `use_yield` (default `false`, `src/Environment.php:130`). With `true`, every node class must carry
  `#[YieldReady]` (else `LogicException` from `YieldNotReadyNodeVisitor`, §6.5). All core nodes are yield-ready.
- Generated code differences between the two modes (VERIFIED):
  - `CaptureNode`: `use_yield=false` → `\Twig\Extension\CoreExtension::captureOutput((function () use (&$context, $macros, $blocks) {...})())`
    (output buffering around a `foreach … echo`, `src/Extension/CoreExtension.php:2207-2225`);
    `use_yield=true` → `implode('', iterator_to_array((function () use (&$context, $macros, $blocks) {...})(), false))`.
  - `FlushNode`: `use_yield=true` emits `yield '';` before `flush();`.
- Both modes produce the same string for core nodes. Differences only arise with legacy nodes that `echo` directly.
- `render()` in non-debug mode installs an output callback returning `''` so that a `{% flush %}` inside `render()` does not
  leak buffered output to the client. In Rust: `flush` is a sink flush point; it never changes `render()`'s result.
- A print node always yields a string: `yield (string) <expr>;` except when the expression is known to be a string
  (§4.2 PrintNode). Text nodes yield string literals.

### 3.3 How `$context` travels (by-value copies, merges, restores)

PHP arrays are values (copy-on-write). Every call boundary below copies; objects inside the array are shared handles.

| Construct | Effect on `$context` | Code |
|---|---|---|
| `Template::yield()` (root render, include, embed, parent) | `$context += globals` | `src/Template.php:473` |
| block call `{% block %}` / `block()` / `parent()` | callee gets a **copy** of the caller's current `$context`; changes inside a block never leak out (VERIFIED `block-scope => q=`) | `BlockReferenceNode`, `Template::yieldBlock()` |
| `{% set a = … %}` | `$context["a"] = …` in the current scope | `SetNode` |
| `{% set a %}…{% endset %}`, `{% apply %}`, macro body | body runs in a closure `use (&$context, $macros, $blocks)`: **shares** the scope (VERIFIED `{% set x %}{% set y = 5 %}{% endset %}y={{ y }}` → `y=5`; `{% apply upper %}{% set q = 'in' %}{% endapply %}{{ q }}` → `in`) | `CaptureNode` |
| `{% for %}` | mutates the scope, then restores (§3.5) | `ForNode` |
| `{% with vars %}` / `{% with vars only %}` / `{% with %}` | `$_vN = $context;` … `$context = $_vN;` (VERIFIED: sets inside never leak) | `WithNode` |
| `{% include %}`, `{% embed %}` | callee receives a copy (`$context`, `CoreExtension::merge($context, vars)`, `CoreExtension::toArray(vars)` or `[]`); never leaks back (VERIFIED `leak => nos`) | `IncludeNode` |
| `include()` function | `array_merge($context, $variables)` (with_context) or `$variables` | `CoreExtension::include()` |
| macro call | brand-new context `[declared args…, "varargs"/variadic name => bucket] + globals` | `MacroNode` |
| arrow function | `function (...) use ($context, $macros)`: **snapshot at creation**; params are written into the copy; nothing leaks (VERIFIED: `{% set x = 1 %}{% set f = (y) => x + y %}{% set x = 10 %}` → `[1]\|map(f)` gives `2`, inline `[1]\|map(y => x + y)` gives `11`; a parameter named like an outer variable does not change it) | `ArrowFunctionExpression` |
| `{% extends %}` | the child's `doDisplay` runs its body, then passes its current `$context` to `parent->yield()` | `ModuleNode::compileDisplay()` |

Key order consequences (VERIFIED, initial context `['a'=>1,'i'=>'outer-i','b'=>2]`, global `gl`):
- after `{% for i in [1] %}…{% endfor %}`: keys `a,b,gl,i` — a variable that is also a loop target **moves to the end**
  (it is unset, then re-added by `$context += $_parent`).
- inside `{% with {z: 1, a: 9} %}`: `z,a,i,b,gl` (vars first, then old keys, then missing globals).
- included with `{z: 1, a: 9}`: `a,i,b,gl,z` (`array_merge`: existing keys keep position, values replaced).
- included `with {z: 1} only`: `z,gl`.
- macro `m(x, y)`: `x,y,varargs,gl`.
- `include 'k2' with {5: 'five'}`: integer key renumbered by `array_merge` → `0=five` (VERIFIED).

### 3.4 Globals

`Environment::getGlobals()` (`src/Environment.php:942-953`):
```
if extensionSet->isInitialized():          # after any getFilter/getFunction/... or after loadTemplate() (initRuntime)
    resolvedGlobals ??= array_merge(extensionSet->getGlobals(), this->globals)
    return resolvedGlobals
return array_merge(extensionSet->getGlobals(), this->globals)   # recomputed each call
```
- `ExtensionSet::getGlobals()` (`src/ExtensionSet.php:392-412`) merges `getGlobals()` of every `GlobalsInterface`
  extension in registration order; cached only once initialized. Env globals override extension globals.
- `addGlobal($name, $v)` (`:926-937`): after initialization only an existing name may be updated, else
  `LogicException('Unable to add global "%s" as the runtime or the extensions have already been initialized.')`
  (VERIFIED). Updating an existing global after initialization writes into `resolvedGlobals` (VERIFIED: G1 then G2).
- Globals are merged with `+` everywhere: the context always wins over a global of the same name (VERIFIED
  `global-override => ctx`).
- Merge points: `Template::yield()`, `TemplateWrapper` block APIs, `WithNode` (`$context += $this->env->getGlobals();`
  after building the scope, even with `only` — VERIFIED `with-only => 2global`), macro context (`] + $this->env->getGlobals()`),
  `BlockChain`.

### 3.5 The for loop: `_parent`, `_seq`, `_key`, `_iterated`, `loop`

Compiled shape (`src/Node/ForNode.php:52-111`, `ForLoopNode.php:30-50`, `ForElseNode.php:30-40`), VERIFIED output:
```php
$context['_parent'] = $context;                                         // (1) copy BEFORE evaluating the sequence
$context['_seq'] = CoreExtension::ensureTraversable(<seq expr>);        // (2) non-iterable -> []
$context['_iterated'] = false;                                          // only with {% else %}
$context['loop'] = [                                                    // only when with_loop (optimizer, §6.3)
  'parent' => $context['_parent'],
  'index0' => 0,
  'index'  => 1,
  'first'  => true,
];
if (is_array($context['_seq']) || (is_object($context['_seq']) && $context['_seq'] instanceof \Countable)) {
    $length = count($context['_seq']);
    $context['loop']['revindex0'] = $length - 1;
    $context['loop']['revindex'] = $length;
    $context['loop']['length'] = $length;
    $context['loop']['last'] = 1 === $length;
}
foreach ($context['_seq'] as $context["k"] => $context["v"]) {          // key target defaults to "_key"
    <body>
    $context['_iterated'] = true;                                       // only with else
    ++$context['loop']['index0'];                                       // only with_loop
    ++$context['loop']['index'];
    $context['loop']['first'] = false;
    if (isset($context['loop']['revindex0'], $context['loop']['revindex'])) {
        --$context['loop']['revindex0'];
        --$context['loop']['revindex'];
        $context['loop']['last'] = 0 === $context['loop']['revindex0'];
    }
}
if (!$context['_iterated']) { <else body> }                             // ForElseNode
$_parent = $context['_parent'];
unset($context['_seq'], $context['k'], $context['v'], $context['_parent'] [, $context['_iterated']] [, $context['loop']]);
$context = array_intersect_key($context, $_parent);                     // keep only keys that existed before (with inner values)
$context += $_parent;                                                   // re-add removed outer keys (outer values) at the end
```
Port-ready algorithm:
```
outer = ctx.clone()                      # snapshot (without "_parent")
ctx["_parent"] = outer.clone()
seq = eval(seq_expr)                     # evaluated AFTER _parent was set: `_context` as seq contains "_parent" (VERIFIED)
ctx["_seq"] = if is_iterable(seq) { seq } else { [] }   # strings/ints/null/plain objects iterate zero times
if has_else: ctx["_iterated"] = false
if with_loop:
    ctx["loop"] = {parent: ctx["_parent"], index0: 0, index: 1, first: true}
    if seq is array or Countable: n = count(seq) (Countable::count() called once); add revindex0=n-1, revindex=n, length=n, last=(n==1)
for (k, v) in iterate(seq):             # arrays: by-value iteration of a snapshot; Traversable: Iterator protocol / IteratorAggregate
    ctx[value_target] = v; ctx[key_target] = k          # value assigned first (key order: value key before key key, VERIFIED)
    run body
    if has_else: ctx["_iterated"] = true
    if with_loop: index0++, index++, first=false; if revindex0 and revindex both set and non-null: revindex0--, revindex--, last=(revindex0==0)
if has_else and !ctx["_iterated"]: run else body
parent = ctx["_parent"]
remove ctx keys: _seq, key_target, value_target, _parent, (_iterated), (loop)
ctx = ctx restricted to keys present in parent (order of ctx preserved)
for (k, v) in parent: if k not in ctx: ctx[k] = v      # appended in parent order
```
Observable consequences (VERIFIED):
- Variables assigned inside the loop that existed before keep the inner value (`x=2`); new ones vanish (`y=`); a loop
  target or `loop`/`_key`/`_parent` that existed outside is restored to its outer value (`restore => arrLKP`).
- `loop.parent` = the context at loop start, i.e. without `_parent` (`loop.parent|keys` → `x`), nested loops expose
  `loop.parent.loop`.
- Without a countable sequence (generators, iterators, `IteratorAggregate`): no `revindex0/revindex/length/last`
  (`for-gen => 1--2--`; strict `loop.revindex` → `Key "revindex" for sequence/mapping with keys "parent, index0, index, first" does not exist.`).
- The sequence is evaluated once (`{% set items = [] %}` inside the loop does not stop iteration, but persists after:
  `seq_once => 120`).
- Keys come out as PHP normalizes them: `{"a": 1, "2": "b", 3: "c"}` iterates keys `a`, `2` (int), `3` (int).
- With the optimizer, `loop` is not created unless referenced (§6.3): `{{ _context|keys|join(',') }}` inside the loop gives
  `a,g,_parent,_seq,i,_key` vs `a,g,_parent,_seq,loop,i,_key` when `loop.index` is also used (VERIFIED).
- The optimizer marks references to loop targets (and `loop`) inside the whole `for` node — including the `else`
  branch — as always defined, compiled as `$context["x"]`: `{% for x in [] %}{% else %}[{{ x }}]{% endfor %}` emits PHP
  warning `Undefined array key "x"` and prints `[]`, also in strict mode (VERIFIED). Rust: treat as null (and optionally
  record a warning); never a strict-variables error there.

### 3.6 Inheritance: parents and blocks

**getParent()** (`src/Template.php:109-145`):
```
if this.parent !== null: return this.parent          # false (no parent) or cached constant parent Template
ensureSecurityChecked()
try parent = doGetParent(context) catch Throwable e: handleException(e)
if !parent: return false                             # only reachable for the base doGetParent() (returns false)
if parent is TemplateWrapper: parent = load(parent, -1)
if parent is Template: return parents[parent.getSourceContext().getName()] = parent
return parents[parent] ??= load(parent, -1)           # string name; cached per name
```
`hasFixedParent()` = `null !== $this->parent` (true without parent or after a constant parent was loaded).
A dynamic `{% extends %}` expression may be a string, a `TemplateWrapper`, or an array of names (resolved by
`Environment::resolveTemplate()`: first existing name). VERIFIED `{% extends layout %}` with `layout = ['missing','l1']`.
The compiled `doGetParent()` always returns `$this->load(<expr>, LINE)`, so the value is coerced by PHP's parameter
type `string|TemplateWrapper|array` (VERIFIED): `false` → `""` → `Template "" is not defined in "i" at line 1.`;
`null` → PHP `TypeError` wrapped as `An exception has been thrown during the rendering of a template ("Twig\Template::load(): Argument #1 ($template) must be of type Twig\TemplateWrapper|array|string, null given, called in …")`
(PHP-internal text; a port must pick its own wording — UNVERIFIED target text). Ints become strings (`5` → `"5"`).

**Block table.** `Template::yield()` merges `$blocks = array_merge($this->blocks, $blocks)`; a child passes
`array_merge($this->blocks, $blocks)` to its parent's `yield()`. Net effect for a chain C → P → G rendered from C:
inside G's `doDisplay`, `$blocks` = G.blocks ⊕ P.blocks ⊕ C.blocks (rightmost wins). Each entry is
`[ownerTemplate, 'block_NAME']`.

**yieldBlock()** (`src/Template.php:487-521`):
```
yieldBlock(name, context, blocks = [], useBlocks = true, templateContext = null):
    if useBlocks and blocks[name] set:      (tpl, method) = blocks[name]
    elif this.blocks[name] set:             (tpl, method) = this.blocks[name]; blocks = array_merge(this.blocks, blocks)
    else:                                   tpl = null
    if tpl not null and not a Template: LogicException('A block must be a method on a \Twig\Template instance.')
    if tpl:
        try { tpl.ensureSecurityChecked(); yield from tpl.method(context, blocks) }
        catch Throwable e: tpl.handleException(e)          # errors inside a block are attributed to the block's owner
    elif parent = this.getParent(context):
        yield from parent.yieldBlock(name, context, array_merge(this.blocks, blocks), false, templateContext ?? this)
    elif blocks[name] set:
        throw RuntimeError(sprintf('Block "%s" should not call parent() in "%s" as the block does not exist in the parent template "%s".',
                                   name, blocks[name][0].getTemplateName(), this.getTemplateName()), -1, blocks[name][0].getSourceContext())
    else:
        throw RuntimeError(sprintf('Block "%s" on template "%s" does not exist.', name, this.getTemplateName()),
                           -1, (templateContext ?? this).getSourceContext())
```
**yieldParentBlock()** (`:535-544`) — `parent()` inside block `name` of the template that *owns* the running block method:
```
if this.traits[name] set: yield from this.traits[name][0].yieldBlock(this.traitAliases[name] ?? name, context, blocks, false)
elif parent = this.getParent(context): yield from parent.yieldBlock(name, context, blocks, false)
else throw RuntimeError(sprintf('The template has no parent and no traits defining the "%s" block.', name), -1, this.getSourceContext())
```
Compiled call sites:
- `{% block b %}` placeholder: `yield from $this->unwrap()->yieldBlock('b', $context, $blocks);`
- `{{ block('b') }}`: `yield from $this->unwrap()->yieldBlock("b", $context, $blocks);` (output mode, optimizer) or
  `$this->unwrap()->renderBlock("b", $context, $blocks)` inside expressions.
- `{{ block('b', 'tpl') }}`: `$this->load("tpl", LINE)->unwrap()->yieldBlock("b", $context)` — **no `$blocks`**: blocks of
  the other template are resolved in that template only.
- `block('b') is defined` → `$this->unwrap()->hasBlock("b", $context, $blocks)`; with template:
  `$this->load(...)->unwrap()->hasBlock("b", $context)`.
- `{{ parent() }}` → `yield from $this->yieldParentBlock("b", $context, $blocks);` or `$this->renderParentBlock(...)`.

**hasBlock()** (`:289-304`): `isset($blocks[$name])` → `$blocks[$name][0] instanceof self`; else own `$this->blocks`;
else `getParent($context)?->hasBlock($name, $context)` (blocks not forwarded); else false.
**getBlockNames()** (`:317-326`): `array_unique(array_merge(array_keys($blocks), array_keys($this->blocks), parent names))`.

VERIFIED messages:
- `Block "b" should not call parent() in "child_parent_missing" as the block does not exist in the parent template "parent" in "child_parent_missing" at line 1.`
- `Block "nope" on template "block_missing" does not exist in "block_missing" at line 1.`
- `{{ block('nope', 'parent') }}` → `Block "nope" on template "parent" does not exist in "parent".` (no line, see §3.12)
- `TemplateWrapper::renderBlock('zz')` on a child → `Block "zz" on template "parent" does not exist in "blocknames".` (no line)
- `{% use %}`'d block aliased away + `parent()` → `The template has no parent and no traits defining the "b1" block in "use.twig" at line 1.`

`_self` inside a block is the owning template's name (the method runs on the owner).

**BlockChain** (`src/BlockChain.php`, new userland API): composes blocks of several templates/chains (highest precedence
first) without their bodies; `renderBlock/streamBlock/displayBlock/hasBlock/getBlockNames`; resolves each template's
lineage via `getParent($context + $this->context + globals)`; unknown block → `RuntimeError('Block "%s" on template "%s" does not exist.', -1, entry source)`;
errors `LogicException('Circular template inheritance detected while building a block chain from "%s".')`,
`InvalidArgumentException('A block chain requires at least one template.')`, `TypeError('Block chain templates must be strings, "Twig\TemplateWrapper" or "Twig\BlockChain" instances, "%s" given.')`,
`LogicException('A block chain cannot contain templates from different Twig environments.')`. Low priority for a port.

### 3.7 Traits (use)

`{% use "t.twig" [with a as b, …] %}`: template must be a constant string (`SyntaxError('The template references in a "use" statement must be a string.')`).
Resolved **in the constructor** (VERIFIED generated code):
```php
$this->ensureTraitsAllowed();                       // sandbox: checks the "use" tag only
$_trait_0 = $this->load("blocks.twig", 1);
if (!$_trait_0->unwrap()->isTraitable()) {
    throw new RuntimeError('Template "'."blocks.twig".'" cannot be used as a trait.', 1, $this->source);
}
$_trait_0_blocks = $_trait_0->unwrap()->getBlocks();
if (!isset($_trait_0_blocks["b1"])) {
    throw new RuntimeError(sprintf('Block "%s" is not defined in trait "%s".', "b1", "blocks.twig"), 1, $this->source);
}
$_trait_0_blocks["alias"] = $_trait_0_blocks["b1"]; unset($_trait_0_blocks["b1"]); $this->traitAliases["alias"] = "b1";
...
$this->traits = array_merge($_trait_0_blocks, $_trait_1_blocks);   // later traits override earlier ones
$this->blocks = array_merge($this->traits, ['b1' => [$this, 'block_b1']]);   // own blocks override traits
```
Semantics:
- Aliasing **renames**: the original name disappears from the trait set (`parent()` in an own block of the original
  name then fails, VERIFIED). `traitAliases[alias] = original` lets `parent()` inside a block named `alias` call the
  trait's original block.
- Trait blocks keep their owner (`[$traitTemplate, 'block_x']`), so they run with the trait template as `$this`
  (errors inside are attributed to the trait template — VERIFIED `in "use_trait" at line 4`).
- `isTraitable()` rule (`ModuleNode::compileIsTraitable`, `:440-485`): traitable iff no parent, no macros, and every
  top-level body node has **zero child nodes**. Quirk: the test is `count($node)` (number of child nodes), so plain
  `TextNode`s and `BlockReferenceNode`s are fine but any print/tag with children makes it non-traitable. VERIFIED:
  `"body text{% block a %}…"` IS traitable (text ignored at use time); `"{{ x }}{% block a %}…"` → `Template "tprint" cannot be used as a trait in "trait_print" at line 1.`;
  a template with a macro → `Template "tmac" cannot be used as a trait`.
- Trait loading errors carry the `use` line (`Template::load($name, $line)`).

### 3.8 Embed

`{% embed "base.twig" [ignore missing] [with vars] [only] %}…{% endembed %}` is parsed as a separate `ModuleNode`
whose parent is the embed expression and whose body must not output content outside blocks (same rules as a child).
It gets an index (`Parser::embedTemplate`, per root template, starting at 1, nested embeds numbered first because they are
registered when their parse finishes — VERIFIED classes `___2` (outer) and `___1` (inner)). The embedded class has the
**same template name** as the enclosing template (`_self` = enclosing name, VERIFIED `e3 => <[X]e3>`).

Compiled call (`src/Node/EmbedNode.php:25-42` + `IncludeNode`):
```php
yield from $this->load("embed.twig", 1, 1)->unwrap()->yield(CoreExtension::merge($context, ["v" => 1]));
// ignore missing:
try {
    $_v0 = $this->load("embed.twig", 1, 1);
    $_v0->getParent($context);          // forces loading the embedded template's parent inside the try
} catch (LoaderError $e) { $_v0 = null; }
if ($_v0) { yield from $_v0->unwrap()->yield(...); }
```
`Template::load()` with the template's own name maps to the same class family (`static::class` minus `___N`) and
`Environment::loadTemplate($cls, $name, $index)` appends `___<index>` (`src/Template.php:342-351`).
Imports done outside the embed are **not** visible inside the embed's blocks (separate module; VERIFIED
`e => <[]>` lenient, strict `Variable "forms" does not exist in "e" at line 1.`); an `{% import %}` inside the embed
body (outside blocks) works.

### 3.9 Include (tag, function, include_only, source)

Tag (`src/Node/IncludeNode.php:37-103`):
```
{% include e %}                 -> yield from $this->load(e, LINE)->unwrap()->yield($context);
{% include e only %}            -> ...->yield([]);
{% include e with v %}          -> ...->yield(CoreExtension::merge($context, v));
{% include e with v only %}     -> ...->yield(CoreExtension::toArray(v));
ignore missing                   -> try { $_vN = $this->load(e, LINE); } catch (LoaderError $e) { $_vN = null; } if ($_vN) { yield from $_vN->unwrap()->yield(...); }
```
- `e` may be a string, a `TemplateWrapper` (same env), or an array of names (first existing, via
  `Environment::resolveTemplate()`; an array with all missing → `LoaderError('Unable to find one of the following templates: "missing", "missing2".')`;
  a single-element array behaves like a plain name).
- `ignore missing` only catches `LoaderError` thrown while **loading** that template (not errors while rendering it,
  not syntax errors).
- `with` non-iterable → `CoreExtension::merge()` → `RuntimeError('The "merge" filter expects a sequence or a mapping, got "int" for argument 2.')`
  (VERIFIED, attributed to the including template and line). `with v only` where `v` is not an array: PHP `TypeError`
  from `Template::yield(array $context)` wrapped as `An exception has been thrown during the rendering of a template ("Twig\Template::yield(): Argument #1 ($context) must be of type array, int given, called in …")`
  — message contains PHP internals, not reproducible verbatim (UNVERIFIED how a port should phrase it).
  A Traversable `with` value is accepted (converted by `toArray`, keys preserved; VERIFIED generator).
- Name type juggling: `{% include 5 %}` → name `"5"` (`Template "5" is not defined.`); `{% include null %}` → PHP
  `TypeError` (wrapped, PHP-internal message).

Function `include(template, variables = [], with_context = true, ignore_missing = false, sandboxed = false)`
(`src/Extension/CoreExtension.php:1541-1584`, `needs_environment`, `needs_context`, `is_safe: ['all']`):
```
if func_num_args >= 7: deprecation (sandboxed arg)
if withContext: variables = array_merge(context, variables)      # renumbers int keys
if sandboxed && SandboxExtension: enable sandbox temporarily (restored in finally)
try loaded = env.resolveTemplate(template) catch LoaderError: if !ignoreMissing rethrow; return ''
rendered = loaded.render(variables)                                # full render (globals merged inside)
return '' === rendered ? '' : new Markup(rendered, env.getCharset())
```
`include_only(template, variables = [], ignore_missing = false)` = `include(env, [], template, variables, false, ignoreMissing)`.
`source(name, ignore_missing = false)` (`:1610-1622`): `env.getLoader().getSourceContext(name).getCode()`; `LoaderError`
→ `''` if ignore_missing. Returns raw template source (no rendering), `is_safe: ['all']`.

Differences tag vs function: the tag streams the included template into the current output (`yield from`) using the
current `$context` (no array_merge unless `with`); the function renders to a string, returns `Markup`, and is subject to
the optimizer's `loop` analysis only through argument inspection (§6.3).

### 3.10 Macros at runtime

**Definition.** Each `{% macro name(args) %}` becomes a `MacroNode` in the module's `MacrosNode` (last definition wins,
with a deprecation when redefined, `Parser::setMacro`, `src/Parser.php:362-369`), and a `MacroDeclarationNode`
(`ConfigNode`, compiles to nothing) in the body. Compiled (VERIFIED):
```php
"input" => new \Twig\TwigMacro("input", function ($name = null, $value = "", $type = "text", $size = 20, ...$varargs): string|Markup {
    // line 1
    $macros = $this->macros;
    $context = [
        "name" => $name,
        "value" => $value,
        "type" => $type,
        "size" => $size,
        "varargs" => $varargs,
    ] + $this->env->getGlobals();

    $blocks = [];

    return ('' === $tmp = \Twig\Extension\CoreExtension::captureOutput((function () use (&$context, $macros, $blocks) {
        ...body...
        return; yield;
    })())) ? '' : new Markup($tmp, $this->env->getCharset());
}, ["name" => false, "value" => true, "type" => true, "size" => true], false),
```
- Parameters without an explicit default get `null` (`ConstantExpression(null)` with attribute `is_implicit`); the
  signature array maps name → "has explicit default". Defaults must be constant expressions (constants, arrays of
  constants, unary +/-).
- A non-variadic macro always gets an implicit `...$varargs` bucket exposed as context key `varargs`. An explicit
  variadic `...rest` replaces it (bucket var/key `rest`). Declaring an argument named `varargs` →
  `SyntaxError('The argument "varargs" in macro "r" cannot be defined because the variable "varargs" is reserved for arbitrary arguments.')`.
- Reserved PHP names `varargs, context, macros, blocks, this` (`TempNameExpression::RESERVED_NAMES`) are prefixed with
  `"\u{035C}"` in the PHP parameter names only; context keys keep the Twig name (VERIFIED `reserved => 12342|ct0`).
- Return value: `''` (empty string, not Markup) when the body output is empty, else `Markup(output, charset)`
  (VERIFIED `m.empty() is same as('')` → true).
- The macro body's context contains only its arguments, the bucket and globals (VERIFIED keys `varargs,gl`).
- `$this` inside the closure is the declaring template, so `_self`, `$this->macros` and the escaper are the declaring
  template's.

**Import.** `{% import expr as name %}` → `ImportNode` (`src/Node/ImportNode.php:44-61`):
```php
$macros["forms"] = $this->macros["forms"] = $this->getMacroNamespace();                         // import _self
$macros["forms"] = $this->macros["forms"] = $this->load("m.twig", LINE)->unwrap()->getMacroNamespace();
```
The `$this->macros[...] = ` part is emitted only when the import is in the template's main scope
(`AssignMacroVariable` attribute `global`, `Parser::isMainScope()`); imports inside blocks/macros only set the local
`$macros` copy. `{% from expr import a, b as c %}` registers an internal `MacroVariable` (auto-named `_vN`) and
function-call aliases; `a()` compiles to `($macros["_v0"] ?? $this->throwUninitializedMacroNamespace(6))->call("a", [...], $context, LINE, $this->source)`.
Duplicate names in one `from` overwrite each other (VERIFIED: `import hello as hi, hello` only registers `hello`).

**Call.** `forms.input(...)` compiles to (VERIFIED):
```php
($macros["forms"] ?? $this->throwUninitializedMacroNamespace(7))->call("input", ["pwd", "type" => "password", "extra" => 1], $context, 8, $this->source)
$this->getMacroNamespace()->call("input", ["x"], $context, 11, $this->source)       // _self.input('x')
(…)->has("list", $context)                                                         // forms.list is defined
```
Arguments: positional keys are ints, named keys are the Twig names **as written** (macro named args are
case-sensitive and not normalized, unlike filter/function args). Dynamic name `m.(expr)()` → `(string) <expr>`.
`throwUninitializedMacroNamespace(line)` (`src/Template.php:599-602`) →
`RuntimeError('Macros imported in the body of template "%s" are not available because the body was not rendered; move the "import" or "from" tag inside the block or the macro that uses it.', line, source)`
— happens e.g. when a block is rendered via `renderBlock()` before any full render of that template instance
(VERIFIED; after one full render the instance's `$this->macros` is populated and the same `renderBlock()` succeeds).

**MacroNamespace::call(name, args, context, line, source)** (`src/MacroNamespace.php:54-69`):
```
macro = getDeclared(name)                      # own macros: exact name, else case-insensitive (strcasecmp) with deprecation
if macro: return macro.callLegacy(args, source, line)
macro = getParent(context)?.resolve(name, context)   # walk the template's parent chain (getParent($context)), getDeclared at each level
if !macro:
    if name starts with "macro_" and resolve(substr(name, 6)) found: deprecation, use it
    else throw RuntimeError(sprintf('Macro "%s" is not defined in template "%s".', name, template.getTemplateName()), line, source)
return macro.callLegacy(args, source, line)
```
`getDeclared()` calls `$this->template->ensureSecurityChecked()` on a hit. `has(name, context)` (`:31-52`) searches the
same chain with case-insensitive matching (deprecation when case differs) and the `macro_` prefix fallback.
VERIFIED: `Macro "nope" is not defined in template "macros" in "macro_missing" at line 1.`

**TwigMacro::callLegacy(args, source, line)** (`src/TwigMacro.php:87-156`) — binding is delegated to PHP's native
argument unpacking; Twig only pre-checks:
```
if args is a list (all int keys 0..n-1):
    if n < requiredCount or (!variadic and n > count(declared)): emit deprecations (missing / too many)
    return body(...args)                       # extra positionals land in the bucket as 0,1,...
if count(args) == count(declared) and every key is a declared name: return body(...renamed(args))
scan args in order: positionalCount, sawNamed, misordered (int key after a named one), first duplicate
    (a named arg whose declared index < positionalCount), unknown named, namedRequired
if misordered: throw RuntimeError('Positional arguments cannot be used after named arguments for macro "%s".', line, source)
if duplicate:  throw RuntimeError('Argument "%s" is defined twice for macro "%s".', line, source)
deprecations for missing required / unknown named / too many positionals (non-variadic)
return body(...renamed(args))
```
PHP binding rules then apply: positional by order; named by exact (case-sensitive) parameter name; unspecified → default
(`null` for implicit); extra positionals then unknown named go to the bucket in call order (`{"0":3,"foo":4}`).
VERIFIED results for `m(a, b = 'B')`:
`m(1,2,3,foo=4)` → a=1,b=2,varargs `{"0":3,"foo":4}`; `m(b=2)` → a=null; `m(1,c=3)` → varargs `{"c":3}`;
`m(A=1)` → a=null, varargs `{"A":1}`; `n(a, ...rest)`: `n(1,2,x=3)` → rest `{"0":2,"x":3}`.
"Required" = declared without explicit default; missing ones only trigger the deprecation
`Not passing a value for the "%s" argument of macro "%s" is deprecated and the argument will be required in Twig 4.0; give it a default value in the macro definition or pass a value when calling it (in "%s" at line %d).`

**Escaping:** macro calls are always "safe" (`SafeAnalysisNodeVisitor`, §6.2): their output is not auto-escaped again;
and the result being `Markup` means it also passes runtime auto-escape.

**Errors inside a macro body** are not caught by the macro: non-Twig exceptions propagate to the caller's handler and are
reported against the **calling template and call line** (VERIFIED `in "user" at line 4` for a filter exception on line 6
of the macro template). Twig errors created with an explicit line inside the body keep the macro template's name/line
(`Variable "nope" does not exist in "macros" at line 3.`).

`CoreExtension::callMacro($namespace, 'macro_'.$name, …)` (`:1380-1383`) is a deprecated helper (to be removed).

### 3.11 Template::load() inside templates

`src/Template.php:331-369`:
```
load(template, line, index = null):
    try:
        if template is array:            return env.resolveTemplate(template).unwrap(env)
        if template is TemplateWrapper:  return template.unwrap(env)          # RuntimeError if another env
        cls = template === this.getTemplateName() ? static::class minus "___N" : env.getTemplateClass(template)
        return env.loadTemplate(cls, template, index)
    catch Error e:                                    # Twig errors only
        if !e.getSourceContext(): e.setSourceContext(this.getSourceContext())
        if e.getTemplateLine() > 0: rethrow           # e.g. syntax error inside the loaded template keeps its own location
        if line == -1: e.guess() else e.setTemplateLine(line)
        rethrow
```
So a missing included/extended/imported template reports the **current** template and the tag's line:
`Template "nope" is not defined in "missing_inc" at line 3.` (VERIFIED). `getParent()`/`hasBlock` resolution pass `-1`
(guess).

### 3.12 Exceptions during rendering: who handles them, which line is reported

`Template::handleException(\Throwable $e)` (`src/Template.php:614-631`):
```
if e is Twig Error:
    if !e.source: e.setSourceContext(this.getSourceContext())
    if e.lineno == -1: e.guess()
    throw e
wrap = new RuntimeError(sprintf('An exception has been thrown during the rendering of a template ("%s").', e.getMessage()), -1, this.getSourceContext(), e)
wrap.guess(); throw wrap
```
Handlers exist in exactly these places, so "this" (the template whose name appears in the message) is:
- `Template::yield()` — the template whose `doDisplay` was running (root, included, embedded, or parent template body);
- `Template::yieldBlock()` — the **owner** of the block being executed (child, parent or trait template);
- `Template::getParent()` — the template evaluating its `doGetParent`;
- `Template::ensureTraitsAllowed()` — sandbox only.
Macro bodies, arrow functions and capture closures have no handler of their own (errors bubble to the enclosing one).

`Error::guess()` (`src/Error/Error.php:115-123,162-207`), only when `lineno == -1`:
```
lineno = 0
template = first object in debug_backtrace() that is a Template whose getTemplateName() == this.source.getName()
if none: return                                  # lineno stays 0 -> message has no " at line"
walk exceptions from the innermost previous one to this one; for each, its throw site then its trace frames (innermost first):
    first frame whose file == file of template's compiled class:
        lineno = template line of the nearest `// line N` marker at or above that PHP line; return
```
Port semantics (VERIFIED on both `cache=false` (eval) and filesystem cache, identical results for the cases tested):
- reported line = the template line of the *statement-level* marker (§2.8 list) that textually precedes the code being
  executed **in the handling template** when the exception passed through it — in practice the line of the innermost
  `PrintNode`/`SetNode`/`IfNode`/`ForNode`/`IncludeNode`/`DoNode`/`TextNode`/`NameExpression`/… being evaluated in that
  template (filter exception on line 4 inside a block → `in "child" at line 4`; macro call on line 4 whose body throws
  → `in "user" at line 4`; exception inside an included template → `in "inc" at line 4`).
- If the error already has a source naming a different template than any template still on the stack, the line becomes 0
  and is omitted: `{{ block('nope', 'parent') }}` → `Block "nope" on template "parent" does not exist in "parent".`
- Errors propagated to the user without any handler keep line -1 (omitted): `TemplateWrapper::renderBlock('zz')` case.
- Because markers are emitted only when the line changes in compile order, a node that emits no marker of its own
  inherits the previous marker; for exact parity a port should precompute, per node, "the line of the last marker
  emitted at or before this node in compile order" (compile order: constructor (trait/parent exprs), doGetParent,
  doDisplay body, blocks in definition order, macros). Multi-line expressions: which PHP line a call is attributed to
  inside a multi-line compiled expression is PHP-engine specific (UNVERIFIED).

Twig errors thrown by callables keep their explicit line if > 0 (VERIFIED: `RuntimeError('twig error with line', 42)`
from a filter → `twig error with line in "twigerr_line" at line 42`); with line -1 they are guessed
(`twig error in "twigerr" at line 2.`).

### 3.13 Per-instance mutable state (observable)

Template objects are singletons per environment and keep state across renders:
- `$this->macros` (import results of the last body execution in main scope) — affects `renderBlock()` before/after a
  full render (§3.10).
- `$this->parent` (constant parent, set by `doGetParent`/`doDisplay`), `$this->parents` (dynamic parents by name).
- `$this->macroNamespace` (lazy), `$this->sourcePath` (lazy).
- `CoreExtension::getAttribute()` static caches per class (method maps, property checkers, constants) — pure caches.
- `EscaperRuntime::$safeClasses/$safeLookup` caches per runtime.

---

## 4. Node reference

Notation: "children" = named sub-nodes (`$nodes`), "attrs" = `$attributes`. `LINE` = node line. All node classes are
`#[YieldReady]`. Unless stated, `compile()` is shown as the PHP it emits; `<x>` = compiled child `x`.

### 4.1 Base classes and interfaces

- `Node` (`src/Node/Node.php`): `$nodes` (ordered map name→Node), `$attributes`, `$lineno`, `$tag` (set by the parser to
  the token parser's tag name — used by the sandbox and CorrectnessNodeVisitor), source context, documentation.
  Default `compile()` subcompiles all children in insertion order (`:123-128`). `__clone` deep-clones children.
  Internal `LogicException`s: `Attribute "%s" does not exist for Node "%s".`, `Node "%s" does not exist for Node "%s".`,
  `The tag of a node can only be set once.`; constructor: `Using "%s" for the value of node "%s" of "%s" is not supported. You must pass a \Twig\Node\Node instance.`
- `Nodes` (final list container), `BodyNode` (marker subclass), `EmptyNode` (no children allowed:
  `LogicException('EmptyNode cannot have children.')`).
- `AbstractExpression` (`src/Node/Expression/AbstractExpression.php`): `isGenerator()` (attr `is_generator`, never set by
  core), `setExplicitParentheses()`/`hasExplicitParentheses()` (attr `with_parentheses`, set by the parser for `( … )`).
- Marker interfaces: `NodeOutputInterface` (Text, Print, BlockReference, Include/Embed), `NodeCaptureInterface` (SetNode),
  `CoercesChildrenToStringInterface` (child names the sandbox wraps with `CheckToStringNode`, §6.4),
  `OperatorEscapeInterface` (`getOperandNamesToEscape()`: ternary/elvis/??, used by escaper & safe analysis),
  `SupportDefinedTestInterface` (`enableDefinedTest()`, `isDefinedTestEnabled()`),
  `ReturnPrimitiveTypeInterface` and sub-interfaces `ReturnBoolInterface`, `ReturnNumberInterface`, `ReturnStringInterface`,
  `ReturnArrayInterface` (used by `TrueTest::wrap()` and `PrintNode` to skip wrappers/casts).

### 4.2 Statement / structural nodes

**ModuleNode** (`src/Node/ModuleNode.php`, final). Children: `body` (BodyNode), `blocks` (Nodes of BodyNode([BlockNode])
or EmptyNode), `macros` (MacrosNode), `traits` (Nodes of `Nodes(['template' => ConstantExpression, 'targets' => Nodes(name => ConstantExpression(alias))])`),
`display_start`, `display_end`, `constructor_start`, `constructor_end`, `class_end` (hooks), optional `parent` (expression).
Attrs: `index` (embed index or null), `embedded_templates` (Node; kept as attribute so visitors traverse them separately
via their own parse), `strategy` (default escaping strategy string or false, set by EscaperNodeVisitor), `escaper`
(bool, set by EscaperNodeVisitor). Line is always 1. Compile: §2. String-coerced child: `parent`.

**TextNode** (`src/Node/TextNode.php`). Attr `data`. Compile: `// line N` + `yield "<data>";`.
`isBlank()`: `ctype_space(data)` or (contains UTF-8 BOM and the remainder after the first 3 bytes is empty or
`ctype_space`) — used to drop whitespace around blocks in child templates and by the correctness checks.

**PrintNode** (`src/Node/PrintNode.php:34-57`). Child `expr`. Compile:
```php
// line N
yield (string) <expr>;      // default
yield <expr>;               // when expr implements ReturnStringInterface (e.g. ConcatBinary) or is a string ConstantExpression (not in defined-test mode)
yield from <expr>;          // when expr->isGenerator() (custom nodes only)
```
`(string)` follows PHP string conversion (arrays → `"Array"` with warning `Array to string conversion`; `true` → `"1"`;
`false`/`null` → `""`; floats with PHP precision rules: `1.0` → `"1"`, `0.1+0.2` → `"0.3"`, `1e100` → `"1.0E+100"`,
`-0.0` → `"-0"`; non-Stringable object → `Error` "Object of class X could not be converted to string" wrapped as a
rendering exception) — VERIFIED. The optimizer replaces `PrintNode(ConstantExpression(string))` by a `TextNode` and
`PrintNode(BlockReferenceExpression|ParentExpression)` by the expression in output mode (§6.3).

**SetNode** (`src/Node/SetNode.php`, NodeCaptureInterface). Children `names` (Nodes of AssignContextVariable),
`values` (Nodes of expressions, or a single node for capture). Attrs `capture`, `safe`. Constructor rewrites capture
mode (`:22-38`):
- body empty → `values = ConstantExpression('')`, capture=false, safe=true;
- body is a single `TextNode` → `ConstantExpression(text)`, capture=false, safe=true;
- body is a single `PrintNode(ConstantExpression)` → that constant, capture=false, safe=true;
- otherwise `values = CaptureNode(body)`, capture=true, safe=true.
Compile (`:54-103`, VERIFIED):
```php
// line N
$context["a"] = <v>;                                    // single target
[$context["b"], $context["c"]] = [<v1>, <v2>];          // multi-target: count(names) must equal count(values) (parser: SyntaxError 'When using set, you must have the same number of variables and assignments.')
$context["d"] = ('' === $tmp = <CaptureNode>) ? '' : new Markup($tmp, $this->env->getCharset());   // capture
$context["e"] = new Markup("static", $this->env->getCharset());   // safe constant, non-empty
$context["f"] = "";                                                // safe constant ''
```
Note: the right-hand side of a multi-target set is a PHP array literal evaluated left to right before assignment
(`{% set a, b = 1, 2 %}{% set a, b = b, a %}{{ a }}{{ b }}` → `21`, VERIFIED). Capture with multiple targets:
`SyntaxError('When using set with a block, you cannot have a multi-target.')` (parser).

**CaptureNode** (`src/Node/CaptureNode.php:30-55`). Child `body`; attr `raw` (always false in core). Compile:
```php
('' === $tmp = \Twig\Extension\CoreExtension::captureOutput((function () use (&$context, $macros, $blocks) {
    <body>
    return; yield;
})())) ? '' : new Markup($tmp, $this->env->getCharset());
// use_yield=true: implode('', iterator_to_array((function () use (&$context, $macros, $blocks) {...})(), false))
```
Used by SetNode (capture), `apply`, macro bodies. The closure **shares** `$context` by reference.

**IfNode** (`src/Node/IfNode.php`). Children `tests` (Nodes: cond0, body0, cond1, body1, …; each cond wrapped with
`TrueTest::wrap()` in the constructor), optional `else`. Compile:
```php
// line N
if (<cond0>) {
    <body0>
} elseif (<cond1>) {
    <body1>
} else {
    <else>
}
```
Conditions that implement `ReturnPrimitiveTypeInterface` (comparisons, constants, arrays, `not`, `and`, tests…) are not
wrapped; everything else is `(($tmp = <x>) && $tmp instanceof Markup ? (string) $tmp : $tmp)` (§5.3).

**ForNode / ForLoopNode / ForElseNode** — §3.5. ForNode children `key_target`, `value_target` (AssignContextVariable;
key defaults to `_key`), `seq`, `body` (= `Nodes([userBody, ForLoopNode])`), optional `else` (ForElseNode). Attr
`with_loop` (default true; optimizer may set false). ForLoopNode attrs `with_loop`, `ifexpr` (unused), `else`
(copied from ForNode at compile time). Passing a non-null `ifexpr` to the constructor is deprecated (the `if` clause of
`for` is gone in 3.x).

**BlockNode** (`src/Node/BlockNode.php`). Attr `name`; child `body`. Compiles to the `block_NAME` method (§2.5).
Lives in `ModuleNode.blocks`, never in the body.

**BlockReferenceNode** (`src/Node/BlockReferenceNode.php`, NodeOutputInterface). Attr `name`. Compile:
`// line N` + `yield from $this->unwrap()->yieldBlock('NAME', $context, $blocks);`
Emitted where a `{% block %}` tag appears. In child templates top-level references are removed by the parser.

**IncludeNode** (`src/Node/IncludeNode.php`). Children `expr`, optional `variables`; attrs `only`, `ignore_missing`
(+ `sandboxed` set by the deprecated `{% sandbox %}` tag, unused by compile). Compile: §3.9. String-coerced child: `expr`.

**EmbedNode** (`src/Node/EmbedNode.php`, extends IncludeNode). Attrs `name` (enclosing template name), `index`;
`expr` is a dummy `ConstantExpression('not_used')`. Compile: §3.8.

**ImportNode** (`src/Node/ImportNode.php`). Children `expr` (template expression or ContextVariable `_self`), `var`
(AssignMacroVariable). Compile: `<var>` (`// line N` + `$macros["name"] = [$this->macros["name"] = ]`) then
`$this->getMacroNamespace();` for `_self`, else `$this->load(<expr>, LINE)->unwrap()->getMacroNamespace();`.
String-coerced child: `expr`.

**MacroNode** (`src/Node/MacroNode.php`). Attrs `name`, `variadic_name`; children `body` (BodyNode), `arguments`
(ArrayExpression of LocalVariable key → default value). Constructor checks (SyntaxError):
`The argument "%s" in macro "%s" cannot be defined because the variable "%s" is reserved for arbitrary arguments.`,
`The variadic argument "%s" in macro "%s" cannot have the same name as another argument.`,
`Argument "%s" is defined twice for macro "%s".` Parser-level: `The variadic argument "%s" in macro "%s" cannot have a default value.`,
`The variadic argument "%s" in macro "%s" must be the last one.`,
`A default value for an argument must be a constant (a boolean, a string, a number, a sequence, or a mapping).`
Compile: §3.10 (closure inside `new \Twig\TwigMacro(...)`).
**MacrosNode** — `loadDeclaredMacros()` (§2.6). **MacroDeclarationNode** — ConfigNode with attr `name`, no output.

**WithNode** (`src/Node/WithNode.php:35-75`). Child `body`, optional `variables`; attr `only`. Compile (VERIFIED):
```php
// line N
$_v0 = $context;
$_v1 = <variables>;
if (!is_iterable($_v1)) {
    throw new RuntimeError('Variables passed to the "with" tag must be a mapping.', LINE_OF_VARIABLES, $this->getSourceContext());
}
$_v1 = CoreExtension::toArray($_v1);
$context = [];                          // only with "only"
$context = $_v1 + $context;
$context += $this->env->getGlobals();
<body>
$context = $_v0;
```
Without variables: just save/body/restore. Note `with` mapping keys from a `Traversable` keep their keys.

**DoNode** — `// line N` + `<expr>;` (value discarded).

**FlushNode** — `// line N` + (`yield '';` if use_yield) + `flush();`.

**AutoEscapeNode** — attr `value` (strategy string, `false`; `true` is possible from `{% autoescape true %}` and ends up
as strategy `"1"` → runtime `Invalid escaping strategy "1" …`, VERIFIED); compiles its body only; consumed by the
EscaperNodeVisitor (§6.1). Parser: no argument → `'html'`; non-constant → `SyntaxError('An escaping strategy must be a string or false.')`.

**SandboxNode** (from the deprecated `{% sandbox %}` tag of SandboxExtension; body may only contain includes):
```php
// line N
if (!$alreadySandboxed = $this->sandbox->isSandboxed()) {
    $this->sandbox->setSandboxed(true);
}
try {
    <body>
} finally {
    if (!$alreadySandboxed) {
        $this->sandbox->setSandboxed(false);
    }
}
```

**DeprecatedNode** (`src/Node/DeprecatedNode.php:31-45`). Children `expr`, optional `package`, `version`. Compile
(VERIFIED):
```php
// line 2
trigger_deprecation("acme/pkg", "1.2", sprintf("%s in \"%s\" at line 2.", "The \"tags\" template is deprecated.", "tags"));
```
i.e. message `<expr> in "<template name>" at line <N>.`; package/version default to `''`. The resulting PHP deprecation
text (from symfony/deprecation-contracts) is `Since <package> <version>: <message>` when package or version is non-empty.
Rust: route to a deprecation callback; never affects output.

**TypesNode / TypeNode** (`{% types {name: 'type', opt?: 'type'} %}`): metadata only; compiles to nothing.

**ConfigNode**: base for nodes without code (`extends`, `use`, macro declarations). `{% extends %}` returns a ConfigNode and
sets the parser's parent expression.

**`guard` tag** (`src/TokenParser/GuardTokenParser.php`): resolved at parse time. `{% guard function|filter|test NAME %}A{% else %}B{% endguard %}`
returns `Nodes([A])` if the callable exists, else `Nodes([B])`; the discarded branch is parsed with unknown callables
ignored. Unsupported type → `SyntaxError('Supported guard types are function, filter and test, "%s" given.')`.

**`apply` tag** (`src/TokenParser/ApplyTokenParser.php`): `Nodes([SetNode(true, LocalVariable(null), body), PrintNode(<filters applied to LocalVariable>)])`.
Compiles to (VERIFIED):
```php
$_v0 = ('' === $tmp = \Twig\Extension\CoreExtension::captureOutput((function () use (&$context, $macros, $blocks) {...})())) ? '' : new Markup($tmp, $this->env->getCharset());
yield (string) $this->escaper->escape(Twig\Extension\CoreExtension::trim(Twig\Extension\CoreExtension::upper($this->env->getCharset(), $_v0)), "html", null, true);
```
The captured body is `Markup` (safe), but after a filter that is not `is_safe`/`preserves_safety` the result is
auto-escaped at runtime (a plain string result gets escaped; a `Markup` result is passed through, §5.5).

**Sandbox security nodes** (added by SandboxNodeVisitor, §6.4):
- `CheckSecurityCallNode` → `$this->sandbox = $env->getExtension(SandboxExtension::class)->getChecker();` at the very top
  of the constructor.
- `CheckSecurityNode(usedFilters, usedTags, usedFunctions, usedTests)` (maps name → first line) → methods
  `checkTraitsAllowed()` (only if the `use` tag is used), `ensureSecurityChecked()` and `checkSecurity()` (VERIFIED text
  in Appendix A.13). Errors get their line from the static maps.
- `CheckToStringNode(expr, spread = false)` (an expression) → `$this->sandbox->ensureToStringAllowed(<expr>, LINE, $this->source)`
  or `ensureSpreadAllowed(...)`.

**Profiler nodes** (`src/Profiler/Node/*`, added by `ProfilerNodeVisitor` of the ProfilerExtension): `EnterProfileNode`
→ `$__internal_<hash> = $this->extensions["<ext>"]; $__internal_<hash>->enter($__internal_<hash>_prof = new \Twig\Profiler\Profile($this->getTemplateName(), "template|block|macro", "<name>"));`;
`LeaveProfileNode` → `$__internal_<hash>->leave($__internal_<hash>_prof);`. No output effect.

**NodeDocumentation / NameDeprecation**: metadata (doc comments attached to nodes, deprecated node/attribute names). No
runtime effect.

### 4.3 Expression nodes: variables and literals

**NameExpression / ContextVariable** (`src/Node/Expression/NameExpression.php:38-82`; `Variable/ContextVariable.php`
is the non-deprecated subclass created by the parser for every bare name except `true/TRUE/false/FALSE/none/NONE/null/NULL`).
Attrs `name`, `ignore_strict_check` (false), `always_defined` (false); defined-test flag from `SupportDefinedTestTrait`.
Every compile first calls `addDebugInfo($this)` (a NameExpression on a new line inserts `// line N` + newline inside the
expression). Compile table (VERIFIED):

| situation | PHP |
|---|---|
| `_self` | `$this->getTemplateName()` |
| `_context` | `$context` (the whole current scope array) |
| `_charset` | `$this->env->getCharset()` |
| defined test, special name or always_defined | `true` |
| defined test | `array_key_exists("name", $context)` |
| always_defined | `$context["name"]` (no existence check: missing key → PHP warning `Undefined array key "name"` + null) |
| non-strict, or `ignore_strict_check` | `($context["name"] ?? null)` |
| strict_variables | `(isset($context["name"]) \|\| array_key_exists("name", $context) ? $context["name"] : (function () { throw new RuntimeError('Variable "name" does not exist.', LINE, $this->source); })())` (pipes escaped for the table) |

`_self` summary: as a value it is the **name string** of the template whose code is running (`getTemplateName()`:
the block owner inside blocks, the enclosing template inside an embed, the declaring template inside a macro body;
VERIFIED `{{ _self }}` → `index`, `e3`); `_self.m()` / `{% import _self as x %}` / `{% from _self import m %}` use
`$this->getMacroNamespace()` (a `MacroVariable` named `_self`, never a context lookup). `_context` is the live scope
array (passing it to a function passes a copy). The three special names cannot be shadowed: a context key `_self` /
`_charset` (passed in or created by `{% set _self = 'S' %}`) exists in `$context` but `{{ _self }}` / `{{ _charset }}`
still compile to the template name / charset (VERIFIED: `i|UTF-8|i|_self,_charset|S` for
`{{ _self }}|{{ _charset }}|{% set _self = 'S' %}{{ _self }}|{{ _context|keys|join(',') }}|{{ _context._self }}`).

`always_defined` is set by: the optimizer for `loop` and loop targets inside a `for` (§6.3); the left operand of `?:`
and `??` (the value position only; the test uses a clone); legacy `MethodCallExpression`. `ignore_strict_check` is set
by `GetAttrExpression` on its object node when the GetAttr itself ignores strict checks (defined-test chains, §4.4).
Note `null` stored in the context is "defined" (`array_key_exists`) and passes strict mode.

**AssignContextVariable** (final; extends deprecated `AssignNameExpression`): compiles to `$context["name"]`.
Constructor: names `true/false/none/null` (case-insensitive) → `SyntaxError('You cannot assign a value to "%s".')`.

**TempNameExpression / LocalVariable**: PHP local variable `$name`; `null` name → auto `_vN` at compile time;
numeric names become ints; reserved names (`varargs, context, macros, blocks, this`) prefixed with `"\u{035C}"`.
LocalVariable is used for macro argument keys, `apply`'s temporary, method-call argument keys.

**MacroVariable** (extends TempNameExpression; `Variable/MacroVariable.php:28-43`): name of an imported macro namespace.
`_self` → `$this->getMacroNamespace()`; otherwise `($macros["name"] ?? $this->throwUninitializedMacroNamespace(LINE))`.
**AssignMacroVariable** (`Variable/AssignMacroVariable.php:24-43`): `// line N` + `$macros["name"] = ` and, if attr
`global`, `$this->macros["name"] = `. `TemplateVariable`/`AssignTemplateVariable` are deprecated aliases.

**ConstantExpression** (`ConstantExpression.php:29-32`): `repr(value)` (string literal, int, float, bool, null, array);
defined-test mode → `true`. Implements `ReturnPrimitiveTypeInterface` (never TrueTest-wrapped). Always "safe for all"
for the escaper.

**ArrayExpression** (`ArrayExpression.php`; literal `[...]` and `{...}`; also argument lists). Children alternate key,
value. Constructor tracks the highest non-negative integer constant key for auto-indexing (`addElement` without key uses
`++index`). Compile (`:116-153`):
```
if definedTest: "true"
if any value is EmptyExpression: SyntaxError('Empty array elements are only allowed in destructuring assignments.', value line)
"[" + for i, (key, value):
    key TempNameExpression -> ConstantExpression(key name); ConstantExpression -> its value; else -> StringCastUnary(key)  " (string)<key>"
    isSequence stays true while key === i (strict int equality); once false it stays false
    emit "<key> => " unless isSequence or value is SpreadUnary
    emit <value>
"]"
```
VERIFIED: `{0: "a", 1: "b"}` → `["a", "b"]`; `{1: "a", 0: "b"}` → `[1 => "a", 0 => "b"]`;
`{a: 1, "b": 2, 3: c, (d): 4}` → `["a" => 1, "b" => 2, 3 => ($context["c"] ?? null),  (string)($context["d"] ?? null) => 4]`;
`[...a, 1]` → `[ ...($context["a"] ?? null), 1]`; `{...a, b: 1}` → `[ ...($context["a"] ?? null), "b" => 1]`.
Runtime = PHP array literal semantics (VERIFIED, `t35.php`): duplicate keys → last value wins, position of the first
occurrence (`{a: 1, b: 2, a: 3}` → `{"a":3,"b":2}`); canonical numeric-string keys → ints (`{'2': 'b'}` keys `[2]`);
**constant** keys (incl. parenthesized constants) are PHP literal keys: `(true)` → 1, `(1.5)` → 1 with deprecation
`Implicit conversion from float 1.5 to int loses precision`, `(null)` → `""` with PHP 8.5 deprecation
`Using null as an array offset is deprecated, use an empty string instead`; **non-constant** keys are `(string)`-cast
first (`([1])` → `"Array"` + warning `Array to string conversion`; an object without `__toString` → Error).
Spread: `[...[1, 2], ...{a: 3}, 4]` → `{"0":1,"1":2,"a":3,"2":4}` (int keys renumbered, string keys kept/overwritten:
`{...{a: 1}, a: 2}` → `{"a":2}`). A sequence literal cannot contain `key: value` pairs (parser error).
**VariadicExpression** = `...` + array (used for PHP-variadic callables). **EmptyExpression** compiles to nothing (only in
destructuring patterns). **ListExpression** (arrow parameters) → `$__a__, $__b__`.

### 4.4 GetAttrExpression

`src/Node/Expression/GetAttrExpression.php`. Created by `.` (`DotExpressionParser`: type `any`, or `method` when followed
by `(`; `?.` sets null_safe), `[...]` (`SquareBracketExpressionParser`: type `array`; `a[x:y]` is the `slice` filter
instead), and the `attribute()` function (type `any`). Children `node`, `attribute`, optional `arguments`
(ArrayExpression; for `.`-access always present, possibly empty). Attrs: `type` (`any`/`array`/`method` =
`Template::ANY_CALL/ARRAY_CALL/METHOD_CALL`), `ignore_strict_check` (false), `optimizable` (`!nullSafe`), `null_safe`,
`is_short_circuited`, `var_name`.

`enableDefinedTest()` sets definedTest and, recursively down the chain of nested GetAttr `node`s, `optimizable=false`
and `ignore_strict_check=true` (`changeIgnoreStrictCheck`, `:190-198`).

Compile (`:49-148`):
1. **Optimized array access** when `optimizable && (!strict_variables || ignore_strict_check) && !definedTest && type == array`
   (in practice: `x[...]` in non-strict mode):
   - without SandboxExtension: `(($_vN = <node>) && is_array($_vN) || $_vN instanceof ArrayAccess ? ($_vN[<key>] ?? null) : null)` — done.
   - with SandboxExtension: `(($_vN = <node>) && is_array($_vN) || $_vN instanceof ArrayAccess && in_array($_vN::class, CoreExtension::ARRAY_LIKE_CLASSES, true) ? ($_vN[<key>] ?? null) : <getAttribute call below>)`.
   `<key>`: a constant attribute is emitted literally; otherwise
   `(($_vM = <attr>) instanceof \Stringable && (is_array($_vN) || $_vN instanceof \ArrayObject || $_vN instanceof \ArrayIterator) ? (string) $_vM : $_vM)`.
   Semantics in §5.2.
2. If `ignore_strict_check`, set it on the `node` child too.
3. **Null-safe**: find the nearest null-safe GetAttr in `{this} ∪ node chain` (this first, then walking down `node`);
   if found and not yet short-circuited: emit `((null === ($_vK = <nullSafeNode.node>)) ? null : ` … `)` and mark it;
   the null-safe node later uses `$_vK` as its object. Effect: `a?.b.c` → `((null === ($_v8 = ($context["a"] ?? null))) ? null : getAttribute(getAttribute($_v8, "b"), "c"))`
   — the whole remaining chain is skipped when `a` is null (VERIFIED `n?.pub.foo` → null even in strict mode).
4. `CoreExtension::getAttribute($this->env, $this->source, <node or $_vK>, <attribute>, <arguments or []>, "<type>", <definedTest bool>, <ignore_strict_check bool>, <hasSandboxExtension bool>, <line of node child>)`.

VERIFIED compiled forms:
```
a.b              CoreExtension::getAttribute($this->env, $this->source, ($context["a"] ?? null), "b", [], "any", false, false, false, 88)
a.b()            ... "b", [], "method", false, false, false, 89)
a.b(1, 2)        ... "b", [1, 2], "method", false, false, false, 90)
a.0              ... 0, [], "any", ...                       (number token -> int attribute)
a.(b)            ... ($context["b"] ?? null), [], "any", ...
a.b is defined   ... "b", [], "any", true, true, false, 45)
a.b.c is defined getAttribute(getAttribute(($context["a"] ?? null), "b", [], "any", false, true, false, 46), "c", [], "any", true, true, false, 46)
a[b] is defined  ... ($context["b"] ?? null), [], "array", true, true, false, 47)
a["b"] (strict)  ... "b", [], "array", false, false, false, 3)
```
Method-call arguments are compiled as an ArrayExpression whose named keys become strings → PHP named arguments at
`$object->$method(...$arguments)` (VERIFIED `o.named(second="x", first="y")` → `y-x`). Reserved names used as named
method arguments get the `"\u{035C}"` prefix (UNVERIFIED consequence: PHP "Unknown named parameter").

String-coerced children (sandbox): `arguments` if present; `attribute` when type is `array`.

### 4.5 Call expressions (function, filter, test) and argument conventions

Classes: `CallExpression` (abstract), `FunctionExpression` (children `arguments`; attrs `name`, `type='function'`,
`twig_callable`), `FilterExpression` (children `node`, `filter` (deprecated ConstantExpression of the name), `arguments`;
attrs `name`, `type='filter'`, `twig_callable`), `TestExpression` (children `node`, optional `arguments`; attrs `name`,
`type='test'`). The node class is chosen by the callable's `node_class` option.

**Compiled call shape** (`CallExpression::compileCallable()` `:21-57`, `compileArguments()` `:59-128`):
```
<callee>(<charset?>, <env?>, <context?>, <isSandboxed?>, <dynamic-name args…>, <node (filter input / test subject)?>, <resolved template arguments…>)
```
- `<charset>` = `$this->env->getCharset()` if `needs_charset`; `<env>` = `$this->env` if `needs_environment`;
  `<context>` = `$context` (the **current scope array by value**) if `needs_context`; `<isSandboxed>` =
  `$this->env->hasExtension(\Twig\Extension\SandboxExtension::class) && $this->env->getExtension(\Twig\Extension\SandboxExtension::class)->getChecker()->isSandboxed($this->source)` if `needs_is_sandboxed`.
  Tests never get charset/env/context (`TwigTest` overrides those getters to false).
- Dynamic-name arguments: for callables registered with `*` in the name (e.g. `*_path`), the parts matched by
  `(.*?)` are passed as leading string arguments (`ExtensionSet::getFilter()` builds the regex
  `'#^'.str_replace('\\*', '(.*?)', preg_quote($name, '#')).'$#'`, `src/ExtensionSet.php:509-511`).
- `<callee>`: a plain function name (`max(`, `range(`, `abs(`, `json_encode(`, `is_iterable(`), a static method
  (`Twig\Extension\CoreExtension::upper(`), a runtime method (`$this->env->getRuntime('Class')->method(`), an extension
  instance method (`$this->extensions['Twig\Extension\CoreExtension']->convertDate(`), or for closures
  `$this->env->getFilter('name')->getCallable()(`. Irrelevant for Rust beyond "call the registered callable".
- VERIFIED: `random()` → `Twig\Extension\CoreExtension::random($this->env->getCharset())`; `date()` →
  `$this->extensions['Twig\Extension\CoreExtension']->convertDate()`; `a|map(x => x * 2)` →
  `Twig\Extension\CoreExtension::map($this->env, <isSandboxed>, ($context["a"] ?? null), function ($__x__) use ($context, $macros) { … })`.

**Template-argument resolution** (`src/Util/CallableArgumentsExtractor.php:29-161`), performed at compile time
(errors are SyntaxErrors with the call node's line and source). Arguments arrive as an ordered map: int keys for
positional, string keys for `name=value` / `name: value`.
```
normalize(n)   = strtolower(str_replace('_', '', n))            # so first_arg, firstArg, FIRSTARG, f_i_r_s_t_arg all match
toSnakeCase(n) = strtolower(preg_replace(['/([A-Z]+)([A-Z][a-z])/', '/([a-z0-9])([A-Z])/'], '\1_\2', n))

extract(args):
  extracted = ordered map; nameMap = {}; named = false
  for (k, node) in args:
      if k is string: named = true
      elif named: error 'Positional arguments cannot be used after named arguments for %s "%s".'      (type, callable name)
      extracted[normalize(k)] = node; nameMap[normalize(k)] = k                                      (int keys stay ints)
  if !named and !callable.is_variadic:
      min = (#injected: charset+env+context+is_sandboxed + #dynamic-name args) + (1 if filter or test)
      if count(extracted) < reflector.requiredParameterCount - min:
          error 'Value for argument "%s" is required for %s "%s".' with toSnakeCase(reflector.params[min + count(extracted)].name)
      return extracted values in order                         # extra positionals are passed through untouched
  if callable has no PHP callable (null): error named ? 'Named arguments are not supported for %s "%s".'
                                                     : 'Arbitrary positional arguments are not supported for %s "%s".'     (no line/source attached)
  params = PHP parameters minus [input if filter/test] minus injected ones minus dynamic-name args
  if is_variadic: last param must be `array $x = []` (dropped) or a PHP variadic `...$x` (dropped, isPhpVariadic) else
      error 'The last parameter of "%s" for %s "%s" must be an array with default value, eg. "array $arg = []".'   (reflection name e.g. Class::method or "Closure")
  result = []; names = []; missing = []; optional = []; pos = 0
  for p in params:
      pname = p.name; if callable is PHP 'range': start->low, end->high
      names.push(pname); key = normalize(pname)
      if key in extracted:
          if pos in extracted: error 'Argument "%s" is defined twice for %s "%s".'                     (pname as declared)
          if missing: error 'Argument "%s" could not be assigned for %s "%s(%s)" because it is mapped to an internal PHP function which cannot determine default value for optional argument%s "%s".'
                      (pname, type, name, join(', ', map(toSnakeCase, names)), 's' if count(missing) > 1, join('", "', missing))
          result += optional; result.push(extracted[key]); delete; optional = []
      elif pos in extracted:
          result += optional; result.push(extracted[pos]); delete; optional = []; pos++
      elif p has default: optional.push(Constant(default))          # defaults are materialized only if a later arg is given
      elif p optional (no reflectable default): if extracted empty: break; missing.push(pname)
      else: error 'Value for argument "%s" is required for %s "%s".' (toSnakeCase(pname))
  if is_variadic:
      bucket = isPhpVariadic ? VariadicExpression : ArrayExpression
      for (k, v) in remaining extracted: int k -> append; string k -> key = toSnakeCase(nameMap[k]) (deprecation if the original was not snake_case)
      if bucket non-empty: result += optional; result.push(bucket)
  if extracted not empty:
      error 'Unknown argument%s "%s" for %s "%s(%s)".' ('s' if >1, join('", "', normalized remaining keys), type, name, join(', ', map(toSnakeCase, names)))
            line/source of the first remaining argument node
  return result
```
VERIFIED messages (all suffixed ` in "index" at line 1.`):
`Value for argument "from" is required for filter "replace"`; `Positional arguments cannot be used after named arguments for filter "replace"`;
`Argument "start" is defined twice for filter "slice"`; `Value for argument "start" is required for filter "slice"` (for `slice(length=1)`);
`Unknown argument "foo" for filter "upper()"`; `Unknown arguments "foo", "bar" for filter "upper()"`;
`Unknown argument "foobar" for filter "upper()"` (for `Foo_Bar=1`: key normalized);
`Value for argument "low" is required for function "range"` (for `range(start=1, end=3)`);
`Value for argument "first_arg" is required for filter "my"`; `Value for argument "value" is required for function "max"`.
Variadic filter `my($value, $firstArg, $second_arg = 'S2', ...$rest)`: `my(1, 2, 3, foo=4)` → `["x",1,2,{"0":3,"foo":4}]`;
`myv($value, array $opts = [])`: `myv(1, 2, a=3, someKey=4)` → opts `{"0":1,"1":2,"a":3,"some_key":4}`.
Extra positional arguments are passed through: userland PHP callables ignore them (`'a'|upper(1, 2)` → `A`), internal
PHP functions throw (`abs() expects exactly 1 argument, 2 given`, wrapped as a rendering exception) — VERIFIED.

Parse-time lookup errors (`src/Parser.php:510-615`, VERIFIED): `Unknown "%s" function.`, `Unknown "%s" filter.`,
`Unknown "%s" test.`, `Unknown "%s" tag.`, each followed by `SyntaxError::addSuggestions()`: candidates with
`levenshtein(name, item) <= strlen(name)/3` or `str_contains(item, name)`, sorted by distance (stable), appended as
` Did you mean "a", "b"?` (e.g. `Unknown "cl" function. Did you mean "cycle", "include", "include_only" in "index" at line 1?`).
Callable lookup order: exact name, then dynamic `*` patterns in registration order, then undefined-callable callbacks
(`src/ExtensionSet.php:229-254, 285-310, 440-465`).

**FunctionExpression::compile** (`:50-70`): if the function is `constant` in defined-test mode, an argument
`checkDefined = true` is added (`constant('X') is defined` → `CoreExtension::constant("X", null, true)`).
**FilterExpression::compile** (`:48-68`): a FilterExpression named `raw` built by legacy code compiles to its node only.
**TestExpression::compile**: standard call; string-coerced children: `node` (only for the base class) and `arguments`.

### 4.6 Special filter / function / test nodes

**DefaultFilter** (`src/Node/Expression/Filter/DefaultFilter.php`, node_class of `default`): if the input is a
`ContextVariable` or `GetAttrExpression`, the node becomes
`ConditionalTernary(DefinedTest(clone input), FilterExpression(input, default, args), args[0] clone or ConstantExpression(''))`;
otherwise a plain call. VERIFIED compiled forms:
```
a|default            ((array_key_exists("a", $context)) ? (Twig\Extension\CoreExtension::default(($context["a"] ?? null))) : (""))
a|default("d")       ((array_key_exists("a", $context)) ? (Twig\Extension\CoreExtension::default(($context["a"] ?? null), "d")) : ("d"))
a.b|default("d")     ((CoreExtension::getAttribute(…, ($context["a"] ?? null), "b", [], "any", true, true, false, 64)) ? (Twig\Extension\CoreExtension::default(CoreExtension::getAttribute(…, "b", [], "any", false, false, false, 64), "d")) : ("d"))
(a ~ b)|default("d") Twig\Extension\CoreExtension::default((($context["a"] ?? null) . ($context["b"] ?? null)), "d")
```
`CoreExtension::default($value, $default = '')` returns `$default` when `testEmpty($value)` (§5.4). In strict mode the
value branch still strict-checks the base variable (the defined test protects it). The default argument expression is
evaluated only in the branch taken; the GetAttr is evaluated twice when defined (once as a defined test that never calls
the method, once for the value).

**EscapeFilter** (internal, node_class of `escape`/`e`): when flagged `template_escaper` by the EscaperNodeVisitor,
compiles to `$this->escaper->escape(<input>, <args…>)` instead of the generic runtime call. Autoescape inserts
`escape(<expr>, "<strategy>", null, true)`; explicit `|e` / `|e("js")` / `|escape("html","UTF-8")` compile to
`$this->escaper->escape(<x>)`, `…escape(<x>, "js")`, `…escape(<x>, "html", "UTF-8")` (VERIFIED).

**RawFilter** (node_class of `raw`, `is_safe: ['all']`): compiles to its input only (`a|raw` → `($context["a"] ?? null)`).

**EnumFunction / EnumCasesFunction** (`enum('X')`, `enum_cases('X')`): with exactly one constant string argument the
enum is checked at compile time and inlined (`X::FirstCase` / `X::cases()`), SyntaxErrors:
`The first argument of the "enum" function must be a string.`, `The first argument of the "enum" function must be the name of an enum, "%s" given.`,
`The first argument of the "enum" function must be a non-empty enum, "%s" given.` (and `enum_cases` variants). Otherwise
a runtime call (`CoreExtension::enum()` / `enumCases()`, §5.4). PHP enums have no Rust equivalent; implement via a host
enum registry or omit.

**RenderSandboxedFunction** (SandboxBridgeExtension's `render_sandboxed`): requires a literal `output_strategy`
argument (position 2 or named `output_strategy`, normalized) that is a non-empty string other than `all`; it is removed
from the runtime call and used as the "safe for" strategy. Errors: `Value for argument "output_strategy" is required for function "render_sandboxed".`,
`The "output_strategy" argument of the "render_sandboxed" function must be a non-empty literal string other than "all".`

**Test nodes** (all extend TestExpression; VERIFIED compiled forms):

| test | node class | PHP |
|---|---|---|
| `defined` | DefinedTest | `<node in defined-test mode>` — node must implement SupportDefinedTestInterface (Name, GetAttr, Constant, Array, BlockReference, Function `constant`, MacroReference, MethodCall) else `SyntaxError('The "defined" test only works with simple variables.')` |
| `null`, `none` | NullTest | `(null === <x>)` |
| `same as(y)` | SameasTest | `(<x> === <y>)` |
| `even` | EvenTest | `(<x> % 2 == 0)` |
| `odd` | OddTest | `(<x> % 2 != 0)` |
| `divisible by(y)` | DivisiblebyTest | `(0 == <x> % <y>)` (`% 0` → `DivisionByZeroError` "Modulo by zero", wrapped) |
| `constant('C')` / `constant('C', obj)` | ConstantTest | `(<x> === constant("C"))` / `(<x> === constant(get_class(<obj>)."::"."C"))` |
| `true` | TrueTest | `(($tmp = <x>) && $tmp instanceof Markup ? (string) $tmp : $tmp)` |
| `empty` | TestExpression | `Twig\Extension\CoreExtension::testEmpty(<x>)` |
| `iterable` | NonStringCoercingInputTest | `is_iterable(<x>)` |
| `sequence` / `mapping` | NonStringCoercingInputTest | `CoreExtension::testSequence(<x>)` / `testMapping(<x>)` |
| `is not T` | NotUnary(test) | ` !<test>` |

`x is defined` for a MacroReference: `($macros["m"] ?? …)->has("name", $context)`; for `constant('X')`:
`CoreExtension::constant("X", null, true)`; for a function name imported with `from`, the parser converts the
name into a MacroReferenceExpression. Note: the `true` test returns the **operand itself** (or its string form when it
is a truthy `Markup`), not a bool — VERIFIED `{{ 'abc' is true }}` → `abc`, `{{ 0 is true }}` → `0`, `{{ '' is true }}` → ``.
Only its truthiness matters in `if`/`and`/`or`/`not`.

### 4.7 Binary operators

All `AbstractBinary` subclasses compile by default to `(<left> OP <right>)` (`Binary/AbstractBinary.php:37-50`);
evaluation order left then right (PHP). Precedence/associativity are parser concerns (`CoreExtension::getExpressionParsers()`
`:340-409`). VERIFIED compiled forms:

| Twig | Node | PHP | returns-interface | string-coerced |
|---|---|---|---|---|
| `a + b` | AddBinary | `(a + b)` | Number | – |
| `a - b` | SubBinary | `(a - b)` | Number | – |
| `a * b` | MulBinary | `(a * b)` | Number | – |
| `a / b` | DivBinary | `(a / b)` | Number | – |
| `a // b` | FloorDivBinary | `(int) floor((a / b))` | Number | – |
| `a % b` | ModBinary | `(a % b)` | Number | – |
| `a ** b` | PowerBinary | `(a ** b)` | Number | – |
| `a ~ b` | ConcatBinary | `(a . b)` | String | left, right |
| `a and b` | AndBinary | `(T(a) && T(b))` | Bool | – |
| `a or b` | OrBinary | `(T(a) \|\| T(b))` | Bool | – |
| `a xor b` | XorBinary | `(T(a) xor T(b))` | Bool | – |
| `a b-and b` / `b-or` / `b-xor` | Bitwise*Binary | `(a & b)` / `(a \| b)` / `(a ^ b)` | Number | – |
| `==` `!=` `<` `>` `<=` `>=` | Equal/NotEqual/Less/Greater/LessEqual/GreaterEqualBinary | PHP `==` `!=` `<` `>` `<=` `>=` (PHP ≥ 8; older PHP used `CoreExtension::compare`) | Bool | left, right |
| `<=>` | SpaceshipBinary | `(a <=> b)` | Number | left, right |
| `===` / `!==` | SameAs/NotSameAsBinary | `(a === b)` / `(a !== b)` | Bool | – |
| `a in b` | InBinary | `CoreExtension::inFilter(a, b)` | Bool | left, right |
| `a not in b` | NotInBinary | `!CoreExtension::inFilter(a, b)` | Bool | left, right |
| `a matches b` | MatchesBinary | `CoreExtension::matches(b, a)` — **right evaluated first**; returns **int 0/1** (prints `0`/`1`, VERIFIED) | Bool | left, right |
| `a starts with b` | StartsWithBinary | `(is_string($_v0 = a) && is_string($_v1 = b) && str_starts_with($_v0, $_v1))` (b not evaluated when a is not a string; `1 starts with "1"` → false) | Bool | – |
| `a ends with b` | EndsWithBinary | same with `str_ends_with` | Bool | – |
| `a has some f` | HasSomeBinary | `CoreExtension::arraySome($this->env, a, f, <isSandboxed>)` | Bool | – |
| `a has every f` | HasEveryBinary | `CoreExtension::arrayEvery($this->env, a, f, <isSandboxed>)` | Bool | – |
| `a .. b` | RangeBinary | `range(a, b)` (PHP range semantics: `"a".."c"`, `3..1` descending, `1..2.5` → `[1,2]`) | Array | left, right |
| `a ?: b` | ElvisBinary | §4.9 | – | – |
| `a ?? b` | NullCoalesceBinary | §4.9 | – | – |
| `x = v` | SetBinary | `($context["x"] = v)` | – | – |
| `[x, y] = v` / `{x, y: z} = v` | Sequence/ObjectDestructuringSetBinary | §4.12 | – | – |

`T(x)` = `TrueTest::wrap(x)` (§5.3). Constant-regex `matches`: the regex is validated at parse time by running
`preg_match($regexp, '')` with an error handler →
`SyntaxError(sprintf('Regexp "%s" passed to "matches" is not valid: %s.', $regexp, substr($phpWarning, 14)))`
(VERIFIED `Regexp "/(/" passed to "matches" is not valid: Compilation failed: missing closing parenthesis at offset 1 in "index" at line 1.`).

Runtime semantics are PHP 8.5 operator semantics (another survey covers type juggling). VERIFIED error/edge samples
(messages appear inside `An exception has been thrown during the rendering of a template ("…")`):
`"abc" + 1` → `Unsupported operand types: string + int`; `"5 apples" + 1` → `6` with warning `A non-numeric value encountered`;
`[1] + 1` → `Unsupported operand types: array + int`; `[1] + [2, 3]` → `[1,3]` (union); `1 / 0`, `1 // 0` → `Division by zero`;
`1 % 0` → `Modulo by zero`; `-7 // 2` → `-4`; `7.5 // 2` → `3`; `2 ** 63` → float `9.223372036854776e+18`; `-2 ** 2` → `-4`;
`-"abc"` → `Unsupported operand types: string * int` (unary minus is multiplication by -1 in PHP);
`[1] ~ "x"` → `"Arrayx"` + warning; object without `__toString` in `~` → `Object of class ArrayObject could not be converted to string`;
`"a" b-and "b"` → `` "`" `` (PHP bytewise string AND); `"10" == "1e1"` → true; `"abc" == 0` → false; `null < -1` → true.

### 4.8 Unary operators

`AbstractUnary` (`Unary/AbstractUnary.php:33-46`): `(` OP `<node>` `)` when the expression had explicit parentheses,
otherwise ` ` OP `<node>` (leading space). VERIFIED: `-a` → ` -($context["a"] ?? null)`; `(-a)` → `(-($context["a"] ?? null))`;
`- -a` → ` - -(…)`.

| Twig | Node | PHP |
|---|---|---|
| `-x` | NegUnary | ` -x` |
| `+x` | PosUnary | ` +x` |
| `not x` | NotUnary (ReturnBool; operand TrueTest-wrapped) | ` !T(x)` |
| `...x` (in calls/arrays) | SpreadUnary | ` ...x` |
| (internal) | StringCastUnary | ` (string)x` (dynamic array keys) |

VERIFIED: `not "0"` → true, `not "0.0"` → false, `not []` → true, `not [0]` → false, `-null` → `0`, `-true` → `-1`.

### 4.9 Conditional operators: `?:`, `??`, ternary

**ConditionalTernary** (`Ternary/ConditionalTernary.php`): `((T(test)) ? (left) : (right))`; `a ? b` (no else) uses
`ConstantExpression('')` as right. Operands escaped individually (OperatorEscapeInterface `left`, `right`).

**ElvisBinary** (`Binary/ElvisBinary.php`): children `left`, `right`, `test` = `TrueTest::wrap(clone left)`; sets
`always_defined` on `left`. Compile `((<test>) ? (<left>) : (<right>))` → the left operand is **evaluated twice** when
truthy (VERIFIED `a ?: b` → `((T(($context["a"] ?? null))) ? ($context["a"]) : (($context["b"] ?? null)))`). In strict mode
an undefined left variable throws in the test (VERIFIED `Variable "foo" does not exist`).

**NullCoalesceBinary** (`Binary/NullCoalesceBinary.php`): `test` = `DefinedTest(clone left)`, AND-ed with
`NotUnary(NullTest(left))` unless `left` is a `BlockReferenceExpression`; sets `always_defined` on `left`. Compile
`((<test>) ? (<left>) : (<right>))`. VERIFIED:
```
a ?? b       (((array_key_exists("a", $context) &&  !(null === $context["a"]))) ? ($context["a"]) : (($context["b"] ?? null)))
a.b ?? c     (((getAttribute(…"b", [], "any", true, true, false, 42) &&  !(null === getAttribute(…"b", [], "any", false, false, false, 42)))) ? (getAttribute(…"b"…)) : (($context["c"] ?? null)))
```
Consequences: for GetAttr operands the attribute is fetched up to 3 times (methods called twice: null-test + value);
never throws strict errors for the tested chain; `e ?? "def"` with `e = ''` → `''` (only null/undefined fall through,
VERIFIED).

Escaping: the escaper descends into `left`/`right` of these three (and nested ones) and escapes each leaf separately
(`a ? b : c` → `((T(a)) ? (escape(b)) : (escape(c)))`, VERIFIED); safe analysis is the intersection of both operands.

### 4.10 Arrow functions

`ArrowFunctionExpression` (`ArrowFunctionExpression.php:40-51`), children `expr`, `names` (ListExpression of
AssignContextVariable). Compile (VERIFIED):
```php
function ($__v__, $__k__) use ($context, $macros) { $context["v"] = $__v__; $context["k"] = $__k__; return <expr>; }
```
`// line N` is emitted before it if the line changed. The closure snapshots `$context` and `$macros` at creation
(by value); parameters shadow context variables only inside. Arrow functions are PHP `\Closure`s; built-in filters
(`map`, `filter`, `reduce`, `sort`, `find`, `has some/every`, `column`) call them with `(value, key)` (reduce:
`(carry, value, key)`); surplus call arguments are ignored by PHP, missing ones raise an `ArgumentCountError` whose
message embeds PHP internals (VERIFIED `Too few arguments to function __TwigTemplate_…::{closure:…}(), 2 passed in … and exactly 3 expected`,
wrapped as a rendering exception — not reproducible verbatim). Non-closure
callables passed to these helpers are deprecated, and forbidden when sandboxed (`checkArrow`, §5.4).

### 4.11 block(), parent(), attribute(), macro references

Functions with `parser_callable` (`CoreExtension::getFunctions()` `:296-298`) are turned into nodes at parse time
(`FunctionExpressionParser` stores `sandboxed_function_name`/`sandboxed_function` attributes for the sandbox):
- `parent()` → `ParentExpression(blockName)`; SyntaxErrors `Calling the "parent" function outside of a block is forbidden.`
  and `Calling the "parent" function on a template that does not call "extends" or "use" is forbidden.` (`:2230-2241`).
  Compile: output mode `// line N` + `yield from $this->yieldParentBlock("NAME", $context, $blocks);`, else
  `$this->renderParentBlock("NAME", $context, $blocks)` (string).
- `block(name, template = null)` → `BlockReferenceExpression(name, template)` (`:2246-2252`, arguments resolved like a
  function `fn ($name, $template = null)`). Compile (`BlockReferenceExpression.php:46-89`): defined test →
  `hasBlock`; output mode → `// line N` + `yield from <target>->unwrap()->yieldBlock(<name>, $context[, $blocks]);`;
  otherwise `<target>->unwrap()->renderBlock(<name>, $context[, $blocks])` where `<target>` is `$this` or
  `$this->load(<template>, LINE)` and `$blocks` is passed only without template. String-coerced child: `template`.
- `attribute(obj, attr, args = null)` → `GetAttrExpression(obj, attr, args, 'any')` (`:2257-2272`).
- Macro references (`MacroReferenceExpression.php`): children `template` (MacroVariable), `arguments`, optional dynamic
  `name`; attr `name` (static). Compile: `<template>->call(<name>, <arguments>, $context, LINE, $this->source)`; defined
  test: `<template>->has(<name>, $context)`. Static name → `repr`, dynamic → `(string) <expr>`. `__clone` keeps the
  template node shared (its lazily generated `_vN` name must match the import). Calling without parentheses
  (`{{ forms.input }}`) still calls the macro (deprecated since 3.29, CorrectnessNodeVisitor).

### 4.12 Assignment and destructuring expressions

`=` inside expressions (`AssignmentExpressionParser`, precedence 0, right-assoc):
- `x = v` → `SetBinary` → `($context["x"] = <v>)`; value of the expression is `v` (VERIFIED `{% set x = (y = 5) + 1 %}` → x=6, y=5).
- `[a, , b] = v` → `SequenceDestructuringSetBinary` (VERIFIED compile):
  ```php
  [(($_v10 = <v>) instanceof \Traversable ? CoreExtension::destructureSequence($context, [0 => "a", 1 => null, 2 => "b"], $_v10) : ([$context["a"], , $context["b"]] = array_pad($_v10, 3, null))), $_v10 = null][0]
  ```
  arrays: PHP list assignment by **integer keys 0..n-1** after `array_pad` (a mapping without int keys gives null
  values + `Undefined array key 0` warnings, VERIFIED); non-array non-Traversable → `array_pad()` TypeError (wrapped);
  Traversable → first n yielded values (missing → null). Expression value = right-hand side.
- `{a, b: c} = v` → `ObjectDestructuringSetBinary`:
  `[[$context["a"], $context["c"]] = [CoreExtension::getAttribute($this->env, $this->source, ($_v11 = <v>), "a", [], \Twig\Template::ANY_CALL, false, false, <sandbox>, LINE), CoreExtension::getAttribute(…, $_v11, "b", …)], $_v11 = null][0]`
  i.e. each property fetched with full `getAttribute` (ANY_CALL, non-strict-ignoring → strict errors apply).
- Errors: `Cannot assign to "%s", only variables can be assigned.`, `Cannot destructure to an empty list of variables.`,
  `Cannot assign to "%s", only variables can be assigned in sequence destructuring.`, `… in object/mapping destructuring.`
- The `{% set %}` tag does not accept patterns (only names); destructuring is used via `{% do [a, b] = … %}` or inside
  expressions.

### 4.13 Deprecated / legacy expression nodes

- `ConditionalExpression` (3.17-deprecated): `((e1) ? (e2) : (e3))`, or `((e1) ?: (e3))` when e1 === e2 node.
- `NullCoalesceExpression` (deprecated): `((<name>) ?? (<right>))` for a ContextVariable left, else the conditional form.
- `MethodCallExpression` (deprecated): `$macros["n"]->call("m", …, $context, LINE, $this->getSourceContext())` / `->has()`.
- `InlinePrint` (deprecated): `yield <node>`.
- `NameExpression`, `AssignNameExpression`, `TempNameExpression`, `TemplateVariable`, `AssignTemplateVariable` used
  directly: deprecated aliases with identical compile output.

---

## 5. Runtime helpers called by generated code

### 5.1 CoreExtension::getAttribute() — full algorithm

`src/Extension/CoreExtension.php:1754-2010`. Signature:
`getAttribute(Environment $env, Source $source, $object, $item, array $arguments = [], $type = 'any', $isDefinedTest = false, $ignoreStrictCheck = false, $sandboxed = false, int $lineno = -1)`.
`$sandboxed` is a compile-time constant: "SandboxExtension is registered" (the checker itself decides whether the
sandbox is active). Port-ready pseudocode (messages verbatim; `dt(x)` = PHP `get_debug_type`: `null`, `bool`, `int`,
`float`, `string`, `array`, class name, `class@anonymous`, `resource (type)`; `%s` of scalars uses PHP string conversion):

```
propertyNotAllowedError = null
if sandboxed and item is Stringable: checker.ensureToStringAllowed(item, lineno, source)

# ---- (A) array-ish lookup: type 'any' or 'array' -------------------------------------------------------------
if type != 'method':
    arrayItem = (item is bool or float) ? (int) item : item          # true->1, 1.9->1 (truncation)
    if arrayItem is Stringable and object is (ArrayObject or ArrayIterator): arrayItem = (string) arrayItem
    if sandboxed and object is ArrayAccess and class(object) not in ARRAY_LIKE_CLASSES:
        try checker.checkPropertyAllowed(object, arrayItem, lineno, source)
        catch SecurityNotAllowedPropertyError e: propertyNotAllowedError = e; item = (string) item; goto METHOD_CHECK
    if object is array:        arrayItem = (string) arrayItem; exists = array_key_exists(arrayItem, object)   # "1" finds int key 1; null -> ""
    elif object is ArrayAccess: exists = object.offsetExists(arrayItem)
    else:                       exists = false
    if exists:
        return isDefinedTest ? true : object[arrayItem]               # ArrayAccess: offsetGet(arrayItem)
    if type == 'array' or object is not an object:
        if isDefinedTest: return false
        if ignoreStrictCheck or !env.isStrictVariables(): return null
        if object is ArrayAccess:
            msg = (arrayItem is object or array)
                ? sprintf('Key of type "%s" does not exist in ArrayAccess-able object of class "%s".', dt(arrayItem), dt(object))
                : sprintf('Key "%s" does not exist in ArrayAccess-able object of class "%s".', arrayItem, dt(object))
        elif object is an object:
            msg = sprintf('Impossible to access a key "%s" on an object of class "%s" that does not implement ArrayAccess interface.', item, dt(object))
        elif object is array:
            msg = empty(object) ? sprintf('Key "%s" does not exist as the sequence/mapping is empty.', arrayItem)
                                : sprintf('Key "%s" for sequence/mapping with keys "%s" does not exist.', arrayItem, implode(', ', array_keys(object)))
        elif type == 'array':
            msg = object === null ? sprintf('Impossible to access a key ("%s") on a null variable.', item)
                                  : sprintf('Impossible to access a key ("%s") on a %s variable ("%s").', item, dt(object), object)
        else:
            msg = object === null ? sprintf('Impossible to access an attribute ("%s") on a null variable.', item)
                                  : sprintf('Impossible to access an attribute ("%s") on a %s variable ("%s").', item, dt(object), object)
        throw RuntimeError(msg, lineno, source)
    # else: object that is not (or is but lacks the key) ArrayAccess and type 'any' -> continue

# ---- (B) object lookup ------------------------------------------------------------------------------------------
item = (string) item
if object is not an object:                                           # only reachable for type 'method'
    if isDefinedTest: return false
    if ignoreStrictCheck or !strict: return null
    msg = object === null ? sprintf('Impossible to invoke a method ("%s") on a null variable.', item)
        : object is array ? sprintf('Impossible to invoke a method ("%s") on a sequence/mapping.', item)
        : sprintf('Impossible to invoke a method ("%s") on a %s variable ("%s").', item, dt(object), object)
    throw RuntimeError(msg, lineno, source)
if object is Twig\Template: throw RuntimeError('Accessing \Twig\Template attributes is forbidden.', lineno, source)

if type != 'method':
    if sandboxed:
        try checker.checkPropertyAllowed(object, item, lineno, source)
        catch SecurityNotAllowedPropertyError e: propertyNotAllowedError = e; goto METHOD_CHECK
    pc = propertyChecker(class, item)    (cached):                    # getPropertyChecker, :2278-2298
        class has no declared property item       -> 'property_exists' closure (true for dynamic props, even when null)
        declared but non-public, or static        -> bool(class has method __isset)
        declared public, non-static               -> closure ReflectionProperty::isInitialized(object)
    if pc !== false and (isset(object->item) or (pc is closure and pc(object, item))):
        return isDefinedTest ? true : object->item                    # isset() triggers __isset, read triggers __get when inaccessible/undeclared
    if object is DateTimeInterface and item in ['date', 'timezone', 'timezone_type']:
        return isDefinedTest ? true : ((array) object)[item]          # "2020-01-02 03:04:05.000000", "UTC", 3
    if defined(class . '::' . item)  (cached):                         # class constants and enum cases, case-sensitive
        return isDefinedTest ? true : constant(class . '::' . item)

METHOD_CHECK:
map = methodMap(class) (cached):                                       # :1909-1952
    methods = public methods visible from outside (get_class_methods), plus '__invoke' for Closure; sort() them
    for m in methods:
        map[m] = m; lc = strtolower(m); map[lc] = m                    # unconditional (exact names override aliases)
        if lc starts with 'get': p = 3   elif lc starts with 'is': p = 2
        elif lc starts with 'has': p = 3; if ('is' . substr(lc, 3)) is also a method: continue
        else: continue
        stripped = substr(lc, p)
        if stripped != '':
            camel = stripped[0] . substr(m, p + 1)                     # getFooBar -> fooBar
            map[camel] ??= m; map[stripped] ??= m                      # first in sorted order wins: get* < has* < is*, has* skipped when is* exists
call = false
if item in map: method = map[item]
elif strtolower(item) in map: method = map[strtolower(item)]          # ASCII lowercase
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
if isDefinedTest: return true                                          # methods are never called by "is defined"
try: return object->method(...arguments)                               # string keys = PHP named arguments
catch BadMethodCallException e:
    if call and (ignoreStrictCheck or !strict): return null
    throw e
```
Notes / VERIFIED behaviours (`t7.php`):
- Resolution order for `obj.x` (type any): array key / `offsetExists` → accessible property (declared-public initialized,
  dynamic, or via `__isset`/`__get`) → DateTime pseudo-props → class constant → exact method `x` → method by lowercase name
  → `getX` → `isX` → `hasX` (has only if no isX) → `__call`. Examples: `o.priv` (private prop + `getPriv()`) → `getPriv`;
  `o.typed` (uninitialized typed prop) → null / strict "Neither the property…" / `is defined` false; `o.KONST` → class
  constant; `o.FOO`, `o.Foo` → `getFoo()`; `o.foo_bar` → not found (no snake→camel conversion); `o.get` → `get()`;
  `m.zz` with `__isset` false and `__call` → `__call(zz)`; `m.zz is defined` → true; `m.bad` where `__call` throws
  `BadMethodCallException` → null (lenient) / wrapped error (strict).
- Method calls receive `$arguments` spread (`o.args(1, "a", null)`); named arguments use PHP named-argument binding.
- Type `method` (`obj.x()`) skips (A) and the property/constant step.
- `aa.prop` on an ArrayAccess object: `offsetExists('prop')` false → falls through to the property (type any only).
- Strict messages VERIFIED: `Key "zz" for sequence/mapping with keys "a, b" does not exist.`;
  `Key "zz" does not exist as the sequence/mapping is empty.`; `Impossible to access an attribute ("zz") on a string variable ("str").`;
  `Impossible to access a key ("zz") on a string variable ("str").`; `Impossible to invoke a method ("zz") on a int variable ("5").`;
  `Impossible to invoke a method ("zz") on a null variable.`; `Impossible to invoke a method ("zz") on a sequence/mapping.`;
  `Neither the property "zz" nor one of the methods "zz()", "getzz()", "iszz()", "haszz()" or "__call()" exist and have public access in class "ArrayObject".`;
  `Key "zz" does not exist in ArrayAccess-able object of class "ArrayObject".`; `Impossible to access an attribute ("bar") on a null variable.`;
  `Key "-1" for sequence/mapping with keys "0, 1" does not exist.`
- `ARRAY_LIKE_CLASSES` (`:122-134`): `ArrayIterator, ArrayObject, CachingIterator, RecursiveArrayIterator, RecursiveCachingIterator, SplDoublyLinkedList, SplFixedArray, SplObjectStorage, SplQueue, SplStack, WeakMap`.

Rust object model implied: host objects need hooks for (1) offsetExists/offsetGet (ArrayAccess), (2) property lookup with
"declared public & initialized / dynamic / magic isset+get" semantics, (3) constants, (4) a public method list with
case-insensitive lookup and the get/is/has aliasing, (5) `__call`, (6) Stringable, Countable, Traversable.

### 5.2 Optimized `[]` access (non-strict mode)

When `strict_variables` is off, `x[k]` does **not** call `getAttribute()` (§4.4). Semantics:
```
v = eval(x)
if (truthy(v) and v is array) or v is ArrayAccess [and, when SandboxExtension is registered, class(v) in ARRAY_LIKE_CLASSES]:
    key = eval(k); if k is not a constant and key is Stringable and (v is array or ArrayObject or ArrayIterator): key = (string) key
    return v[key] ?? null
       # array: PHP offset rules: bool -> int, float -> int (fractional: deprecation "Implicit conversion from float 0.9 to int loses precision"),
       #        null -> "" (PHP 8.5 deprecation "Using null as an array offset is deprecated, use an empty string instead"),
       #        canonical numeric strings -> int; missing -> null
       # ArrayAccess: offsetExists(key) then offsetGet(key) (null result -> null); key passed RAW (no bool/float->int)
else:
    SandboxExtension registered ? getAttribute(... 'array' ...) : null
```
VERIFIED divergences from strict mode (getAttribute): `aa[true]` on an ArrayAccess → lenient passes `true`
(`offsetExists(true)`), strict passes `1`; `aa[1.5]` → `1.5` vs `1`. `str[0]`, `obj[k]` (non-ArrayAccess object) → null
in lenient mode (strict: errors above). Strings are never indexable by `[]` (no string-offset access).

### 5.3 TrueTest truthiness

`TrueTest::wrap($node)` (`src/Node/Expression/Test/TrueTest.php:24-36`) returns `$node` unchanged when it implements
`ReturnPrimitiveTypeInterface`, else a `TrueTest` compiling to
`(($tmp = <x>) && $tmp instanceof Markup ? (string) $tmp : $tmp)`. Applied to: `if`/`elseif` conditions, `and`/`or`/`xor`
operands, `not` operand, ternary test, elvis test. Effect: a `Markup` is truthy iff its string is truthy (`""` and `"0"`
are falsy), whereas PHP objects are otherwise always truthy. Everything else uses PHP truthiness: `false`, `0`, `0.0`,
`-0.0`, `""`, `"0"`, `[]`, `null` are falsy; `"0.0"`, `" "`, `[0]`, any non-Markup object are truthy (VERIFIED for
strings/arrays). `$tmp` is a single PHP local shared by nested wrappers; evaluation order makes this safe.

### 5.4 Other CoreExtension helpers

All in `src/Extension/CoreExtension.php`.

- `ensureTraversable($seq)` (`:1394-1401`): `is_iterable($seq) ? $seq : []`.
- `toArray($seq, $preserveKeys = true)` (`:1406-1417`): Traversable → `iterator_to_array($seq, $preserveKeys)`;
  non-array → returned unchanged (!); array → itself or `array_values`.
- `merge(...$arrays)` (`:734-747`): for each argument (1-based number N) not iterable →
  `RuntimeError(sprintf('The "merge" filter expects a sequence or a mapping, got "%s" for argument %d.', dt($a), N))`;
  else `$result = array_merge($result, toArray($a))` (int keys renumbered, string keys overwrite in place).
- `include()`, `includeOnly()`, `source()` — §3.9.
- `captureOutput(iterable $body)` (`:2207-2225`): output-buffer the iteration; on exception clean buffers and rethrow.
- `constant($constant, $object = null, bool $checkDefined = false)` (`:1681-1704`):
  ```
  if object !== null:
      if constant == 'class': return checkDefined ? true : get_class(object)
      constant = get_class(object) . '::' . constant
  if !defined(constant):
      if checkDefined: return false
      if strtolower(substr(constant, -7)) == '::class':
          throw RuntimeError(sprintf('You cannot use the Twig function "constant" to access "%s". You could provide an object and call constant("class", $object) or use the class name directly as a string.', constant))
      throw RuntimeError(sprintf('Constant "%s" is undefined.', constant))
  return checkDefined ? true : constant(constant)
  ```
  VERIFIED (`Constant "NOPE" is undefined in "index" at line 1.`; `constant('ATOM', d)` → `Y-m-d\TH:i:sP`). These
  RuntimeErrors carry no line → guessed (§3.12). PHP constants (global/class) need a host registry in Rust.
- `cycle($values, $position)` (`:421-444`): non-array and non-ArrayAccess →
  `RuntimeError('The "cycle" function expects an array or "ArrayAccess" as first argument.')`; non-countable ArrayAccess
  → deprecation + `toArray($values, false)`; `count == 0` → `RuntimeError('The "cycle" function expects a non-empty sequence.')`;
  returns `$values[$position % $count]` (PHP `%` on ints; a mapping is indexed by the integer key, missing → warning +
  null; UNVERIFIED for mappings).
- `inFilter($value, $compare)` (`:1071-1113`), used by `in` / `not in`:
  ```
  value/compare Markup -> (string)
  if compare is string:
      return (value is string|int|float) ? ('' === value or str_contains(compare, (string) value)) : false
  if compare not iterable: return false
  if value is object or resource:
      compare is array ? in_array(value, compare, true) : any(item === value for item in compare)
  return any(0 === compare(value, item) for item in compare)           # iterates values only (keys ignored)
  ```
- `compare($a, $b)` (`:1123-1175`), "PHP 8 non-strict comparison":
  ```
  int a, string b:  bt = trim(b, " \t\n\r\v\f"); if !is_numeric(bt): return (string) a <=> b
                    if (int) bt == bt: return a <=> (int) bt  else return (float) a <=> (float) bt
  string a, int b:  symmetric
  float a, string b: if is_nan(a): return 1; bt = trim(...); if !is_numeric(bt): return (string) a <=> b; return a <=> (float) bt
  string a, float b: if is_nan(b): return 1; symmetric
  else: return a <=> b
  ```
  VERIFIED: `"1" in [1, 2]` true; `null in [0]` true; `0 in ["a"]` false; `1 in {a: 1}` true (values); `"a" in {a: 1}`
  false; `1.0 in [1]` true; `"1e1" in ["10"]` true; `"" in "abc"` true; `1 in "123"` true.
- `matches(string $regexp, ?string $str): int` (`:1182-1196`): `preg_match($regexp, $str ?? '')` with a temporary error
  handler throwing `RuntimeError(sprintf('Regexp "%s" passed to "matches" is not valid', $regexp).substr($phpWarning, 12))`
  (no trailing period: VERIFIED `Regexp "/(/" passed to "matches" is not valid: Compilation failed: missing closing parenthesis at offset 1 in "i" at line 1`,
  `Regexp "nodelim" passed to "matches" is not valid: Delimiter must not be alphanumeric, backslash, or NUL byte in "i" at line 1`);
  `false` result → `RuntimeError(sprintf('Regexp "%s" passed to "matches" failed: %s.', $regexp, preg_last_error_msg()))`.
  Returns int 0/1. Arguments are coerced to `string` by PHP (arrays → TypeError).
- `arraySome($env, $array, $arrow, $isSandboxed = false)` / `arrayEvery` (`:2149-2186`): non-iterable →
  `RuntimeError('The "has some" test expects a sequence or a mapping, got "%s".')` / `'The "has every" test expects a sequence or a mapping, got "%s".'`;
  `checkArrow`; `some`: first truthy `$arrow($v, $k)` → true; `every`: first falsy → false.
- `checkArrow(bool $isSandboxed, $arrow, $thing, $type)` (`:2191-2202`): Closure → ok; sandboxed →
  `RuntimeError(sprintf('The callable passed to the "%s" %s must be a Closure in sandbox mode.', $thing, $type))`;
  else deprecation. (`$type` is `filter` or `operator`.)
- `destructureSequence(array &$context, array $names, \Traversable $sequence)` (`:1424-1450`): assigns the first
  `count($names)` yielded values to the non-null names (stops early), fills the rest with null, returns the sequence.
- `testEmpty($value)` (`:1464-1479`): Countable → `count == 0`; Traversable → `!iterator_count()` (consumes iterators);
  Stringable → `'' === (string)`; else `'' === $v || false === $v || null === $v || [] === $v` (so `0`, `"0"` are NOT empty).
- `testSequence($v)` (`:1491-1502`): ArrayObject → copy; Traversable → `iterator_to_array`; `is_array && array_is_list`.
  `testMapping($v)` (`:1514-1525`): same normalization; `(is_array && !array_is_list) || is_object` (note: any object
  that is not Traversable is a mapping; `[]` is a sequence).
- `default($value, $default = '')` (`:910-917`): `testEmpty($value) ? $default : $value`.
- `enum(string $enum)` / `enumCases(string $enum)` (`:1635-1666`): `RuntimeError('"%s" is not an enum.')`,
  `RuntimeError('"%s" is an empty enum.')`, `RuntimeError('Enum "%s" does not exist.')`.
- `callMacro()` — deprecated wrapper (§3.10).
- `src/Resources/core.php`: deprecated global functions `twig_*` (since 3.9) that forward to the static methods above
  (e.g. `twig_cycle`, `twig_include`, `twig_constant`, `twig_test_empty`); not used by generated code. Not needed in a port.

### 5.5 EscaperRuntime::escape() and Markup

`Twig\Markup` (`src/Markup.php`): holds `content` (string-cast in the constructor), `charset`, `safeStrategies`
(default `['all']`; core always uses the default). `__toString()` → content; `count()` → `mb_strlen(content, charset)`;
`jsonSerialize()` → content; `isSafeFor($s)`: true if any declared strategy is `all`, equals `$s`, or implies it
(`EscaperRuntime::IMPLIED_STRATEGIES`: `html_attr` ⇒ `html_attr_relaxed`, `html`; `html_attr_relaxed` ⇒ `html`);
`withContent()` (internal, used by `trim`). Markup objects are created by: capture (`set…endset`, `apply`, macros),
safe constant `set` captures, `include()`.

`EscaperRuntime::escape($string, string $strategy = 'html', ?string $charset = null, bool $autoescape = false)`
(`src/Runtime/EscaperRuntime.php:118-352`):
```
if autoescape and string is Markup:
    return string.isSafeFor(strategy) ? string : escape((string) string, strategy, charset)      # Markup passes autoescape untouched
if string is not a string:
    if string is Stringable:
        if autoescape:
            c = class(string); compute safeClasses[c] once from parents/interfaces registered via addSafeClass()
            if safeLookup[strategy][c] or safeLookup['all'][c]: return (string) string
        string = (string) string
    elif strategy in ['html', 'js', 'css', 'html_attr', 'html_attr_relaxed', 'url']:
        return string                                     # ints, floats, bools, null, arrays returned AS IS (printed later by "(string)")
if string === '': return ''
charset = charset ?: this.charset (env charset)
switch strategy:
  'html':   UTF-8 -> htmlspecialchars(s, ENT_QUOTES | ENT_SUBSTITUTE, 'UTF-8')
            charsets supported by htmlspecialchars (list at :166-181, also via strtoupper) -> htmlspecialchars with that charset
            else -> iconv(charset -> UTF-8), htmlspecialchars, iconv(UTF-8 -> charset)
  'js':     (convert to UTF-8 if needed) invalid UTF-8 -> RuntimeError('The string to escape is not a valid UTF-8 string.')
            replace every char not in [a-zA-Z0-9,._] by: \\ -> '\\\\', / -> '\/', \x08 -> '\b', \x0C -> '\f', \n -> '\n', \r -> '\r', \t -> '\t',
            else codepoint < 0x10000 -> sprintf('\u%04X'), else UTF-16 surrogate pair '\uHHHH\uLLLL'; convert back if needed
  'css':    (UTF-8 check as js) replace every char not in [a-zA-Z0-9] by sprintf('\\%X ', ord or codepoint)      # note trailing space
  'html_attr' / 'html_attr_relaxed':
            (UTF-8 check) regex '#[^a-zA-Z0-9,\.\-_]#Su' / '#[^a-zA-Z0-9,\.\-_:@\[\]]#Su'; for each match:
              ord <= 0x1F (except \t \n \r) or 0x7F..0x9F -> '&#xFFFD;'
              single byte: 34 '&quot;', 38 '&amp;', 60 '&lt;', 62 '&gt;', else sprintf('&#x%02X;', ord)
              multibyte: sprintf('&#x%04X;', codepoint)
  'url':    rawurlencode(s)
  default:  custom escaper registered via setEscaper(strategy, fn(string, charset)) -> call it
            else RuntimeError(sprintf('Invalid escaping strategy "%s" (valid ones: "%s").', strategy,
                 implode('", "', array_merge(['html', 'js', 'url', 'css', 'html_attr', 'html_attr_relaxed'], array_keys(escapers)))))
```
VERIFIED outputs for `<a href='x'>&é`: html `&lt;a href=&#039;x&#039;&gt;&amp;é`; (js, for `<'a'>`:
`<'a'>`); css `\3C a\20 href\3D \27 x\27 \3E \26 \E9 `; url `%3Ca%20href%3D%27x%27%3E%26%C3%A9`;
html_attr `&lt;a&#x20;href&#x3D;&#x27;x&#x27;&gt;&amp;&#x00E9;`. Explicit `|e` (autoescape=false) escapes Markup
too (`{{ m|e("js") }}` → `<b>`); autoescape leaves Markup alone (`{{ m }}` → `<b>`); a filter returning a
plain string from Markup gets escaped (`{{ m|upper }}` → `&lt;B&gt;`), `trim` keeps Markup (`{{ m|trim }}` → `<b>`).
Errors for unknown strategies are thrown even for non-string inputs with a custom strategy name (`{{ 5|e("bogus") }}`
→ error, VERIFIED) because the early return only covers the six built-in names.

---

## 6. Node visitors

**Traversal** (`src/NodeTraverser.php:48-75`): visitors are grouped by `getPriority()`, groups sorted ascending
(`ksort`), registration order inside a group. Each visitor performs its own complete depth-first pass over the module:
`enterNode(node)` → children in insertion order (a child returning `null` from `leaveNode` is removed; a different
node replaces it) → `leaveNode(node)`. Run once per parsed module by `Parser::parse()` (embedded templates are parsed —
and therefore visited — as separate modules; they are stored in a ModuleNode attribute so the outer pass does not visit
them again).

**Registration order** (`Environment::__construct`, `src/Environment.php:145-152`; `ExtensionSet::initExtensions`):
CoreExtension (`CorrectnessNodeVisitor`, −255) → EscaperExtension (`EscaperNodeVisitor`, 0) → YieldNotReadyExtension
(`YieldNotReadyNodeVisitor`, 255) → OptimizerExtension (`OptimizerNodeVisitor`, 255) → user extensions in `addExtension`
order (SandboxExtension: `SandboxNodeVisitor` 0; ProfilerExtension: `ProfilerNodeVisitor` 0) → visitors added via
`Environment::addNodeVisitor()` (staging extension, last).
Effective pass order: **Correctness → Escaper → (Sandbox / Profiler / user priority-0) → YieldNotReady → Optimizer →
(user priority-255)**. There is no `MacroAutoImportNodeVisitor` in 3.31.1 (`src/NodeVisitor/` contains only
Abstract, Correctness, Escaper, NodeVisitorInterface, Optimizer, SafeAnalysis, Sandbox, YieldNotReady): `_self.macro()`
is resolved by the parser into a `MacroReferenceExpression` on the `_self` `MacroVariable` (`$this->getMacroNamespace()`). Consequence: the Escaper sees the AST before the optimizer (e.g. it still sees
`PrintNode(BlockReferenceExpression)`), and the Sandbox visitor sees the escape filters inserted by the Escaper (the
`escape` filter appears in the sandbox's used-filters list, VERIFIED Appendix A.13).

### 6.1 EscaperNodeVisitor (priority 0)

`src/NodeVisitor/EscaperNodeVisitor.php`. State: `statusStack`, `blocks` (name → strategy at the block tag),
`defaultStrategy`, `usesEscaper`, an inner `SafeAnalysisNodeVisitor`.
```
enter ModuleNode:   defaultStrategy = EscaperExtension.getDefaultStrategy(templateName) or false   (string|false)
                    module.strategy = is_string(defaultStrategy) ? defaultStrategy : false; reset blocks, statusStack, usesEscaper
enter AutoEscapeNode: push node.value
enter BlockNode:      push blocks[name] ?? needEscaping()          # strategy recorded where the {% block %} tag appeared
leave ModuleNode:     module.escaper = usesEscaper
leave FilterExpression:
    if EscapeFilter: mark template_escaper (uses $this->escaper)
    preEscape: type = filter.pre_escape; if type and input not safeFor(type): input = escapeFilter(input, type)
leave PrintNode when (type = needEscaping()) !== false:
    expr is OperatorEscapeInterface ? escapeConditional(expr) : expr = escapeExpression(expr, type)
leave AutoEscapeNode / BlockNode: pop
leave BlockReferenceNode: blocks[name] = needEscaping()
needEscaping() = top(statusStack) if non-empty else (defaultStrategy ?: false)
escapeConditional(e): for each operand name in e.getOperandNamesToEscape(): operand OperatorEscapeInterface ? recurse : replace by escapeExpression(operand)
escapeExpression(e, type) = safeFor(type, e) ? e : EscapeFilter(e, args [Constant(type), Constant(null), Constant(true)])
safeFor(type, e): safe = safeAnalysis.getSafe(e); if empty: traverse e with SafeAnalysisNodeVisitor; safe = getSafe(e)
                  return type in safe or 'all' in safe
```
Rules that follow (VERIFIED, `t31.php`):
- Only `PrintNode` expressions are auto-escaped (and inputs of `pre_escape` filters such as `nl2br`); `set` values,
  filter arguments, macro arguments are not.
- `pre_escape` is applied to **every** FilterExpression of such a filter, independent of the autoescape state and of the
  context it appears in (VERIFIED: with `autoescape => false`, inside `{% autoescape false %}`, and in `{% set x = v|nl2br %}`,
  `"<b>\n"|nl2br` always gives `&lt;b&gt;<br />\n`); only a safe input (`|raw`, constants, safe filters) is left alone.
- A block's body uses the strategy in effect **at its `{% block %}` tag** (`{% autoescape false %}{% block b %}{{ v }}{% endblock %}{% endautoescape %}` → `<b>`);
  a `block('b')` call elsewhere does not change it.
- Macro bodies are compiled with the template's default strategy — an enclosing `{% autoescape %}` around a
  `{% macro %}` definition has **no effect** (`… → &lt;b&gt;`).
- `escape` of `(type, null, true)`: the runtime then skips `Markup` values safe for the strategy (§5.5).
- Strategy `true` from `{% autoescape true %}` is coerced to the string `"1"` (PHP `string` parameter) → runtime
  `Invalid escaping strategy "1" (valid ones: …)`.

### 6.2 SafeAnalysisNodeVisitor (helper of the escaper)

`src/NodeVisitor/SafeAnalysisNodeVisitor.php`. Computes, bottom-up (in `leaveNode`), the list of strategies an expression
is safe for; stored in a `WeakMap`. `getSafe()` adds implications: `html_attr` ⇒ `html`, `html_attr_relaxed`;
`html_attr_relaxed` ⇒ `html`.

| node | safe |
|---|---|
| ConstantExpression | `['all']` |
| BlockReferenceExpression, ParentExpression | `['all']` |
| MacroReferenceExpression, MethodCallExpression | `['all']` |
| OperatorEscapeInterface (ternary, `?:`, `??`) | intersection of the two operands (`LogicException` if more than 2) |
| FilterExpression | `filter.getSafe(arguments)` (`is_safe` or `is_safe_callback(args)`); if empty → `intersect(safe(input), filter.preserves_safety)` |
| FunctionExpression | `function.getSafe(arguments)` |
| anything else (names, GetAttr, arithmetic, concat, …) | `[]` |

`intersect(a, b)`: empty if either empty; `all` in a → b; `all` in b → a; else `array_intersect`.
Core safe declarations: `raw` (`['all']`), `escape`/`e` (`escapeFilterIsSafe`: first argument constant → `[value]`,
non-constant first arg → `[]`, no args → `['html']`), `nl2br` (`['html']`, pre_escape html), `spaceless` (`['html']`,
pre_escape html, deprecated), `include`/`include_only`/`source` (`['all']`), `render_sandboxed` (the literal strategy).
No core filter declares `preserves_safety`.

### 6.3 OptimizerNodeVisitor (priority 255)

`src/NodeVisitor/OptimizerNodeVisitor.php`. Option `optimizations` (default −1 = all; `0` none; `2` =
`OPTIMIZE_FOR`; `4`/`8` deprecated no-ops; > 14 → `InvalidArgumentException('Optimizer mode "%s" is not valid.')`).
1. **Always** (independent of the mode) in `leaveNode` (`optimizePrintNode`, `:99-121`):
   `PrintNode(ConstantExpression(string))` → `TextNode(string)`; `PrintNode(BlockReferenceExpression | ParentExpression)`
   → the expression with `output = true` (streamed `yield from …`). (VERIFIED with `optimizations => 0`.)
2. **OPTIMIZE_FOR** (`:126-224`): entering a `ForNode` sets `with_loop = false` and pushes the loop and its targets;
   the `seq` subtree is visited in the enclosing scope (loop suspended while inside it). Inside a loop:
   - `ContextVariable('loop')` → `always_defined = true`, current loop `with_loop = true`;
   - `ContextVariable(name ∈ targets of any enclosing loop)` → `always_defined = true` (also in the `else` branch);
   - `BlockReferenceNode` / `BlockReferenceExpression` → current loop `with_loop = true` (blocks may use `loop`);
   - `IncludeNode` without `only` → all enclosing loops `with_loop = true`;
   - `include()` function unless its `with_context` argument is literally `false` → all loops `with_loop = true`;
   - `GetAttrExpression` with a non-constant attribute or the constant attribute `parent`, when the current loop already
     has `with_loop` or the object is `loop` → all loops `with_loop = true` (for `loop.parent.loop`).
   Not detected: `_context`, `dump()`, macros receiving `_context`, `attribute(_context, 'loop')` → `loop` silently
   missing (VERIFIED via `_context|keys`).

### 6.4 SandboxNodeVisitor (priority 0, only with SandboxExtension)

`src/NodeVisitor/SandboxNodeVisitor.php`. Collects first-use lines of tags (`node.getNodeTag()`), filters, functions
(including parser-callable functions via the `sandboxed_function_name` attribute and `range` for `..`), tests, skipping
callables/tags whose `isAlwaysAllowedInSandbox()` is true; wraps every child listed by
`CoercesChildrenToStringInterface::getStringCoercedChildNames()` with `CheckToStringNode` (skipping `_self`,
skipping arguments/inputs whose PHP parameter type cannot string-coerce: `CallableParameters::isStringCoercionSafe()`:
int/float/bool/null/true/false/void/never, final non-Stringable non-Traversable classes and enums);
`wrapNode` descends into arrays, `Nodes`, spreads, ternary operands; wraps names, GetAttr, calls and assignments.
On leaving the module: prepends `CheckSecurityCallNode` to `constructor_start`, appends `CheckSecurityNode` to
`class_end`. Runtime: §11.

Shared node objects (VERIFIED with the visitor vectors, `tests/vectors/visitors/`): some constructors put the same PHP
node object in two places of the tree, and a traversal visits it once per place, so the sandbox visitor wraps it (or
the spread inside it) twice:
- `NullCoalesceBinary::__construct()` (`src/Node/Expression/Binary/NullCoalesceBinary.php`) gives its `left` operand
  itself to the `NullTest` of its `test` (`??` = `defined(clone left) and not (left is null) ? left : right`);
- `DefaultFilter::__construct()` (`src/Node/Expression/Filter/DefaultFilter.php`) gives the same `$arguments` node to
  the inner `FilterExpression` it builds and to itself.
`clone` makes deep copies that share nothing: the `DefinedTest` operand of `??` and of `default` and the `$false`
branch of `default` (`clone $arguments->getNode('0')`). Safe-analysis results are cached per node object
(`WeakMap`); recomputing them per visit gives the same result.

### 6.5 YieldNotReadyNodeVisitor (priority 255)

`src/NodeVisitor/YieldNotReadyNodeVisitor.php`: for every non-expression node class without `#[YieldReady]`:
`use_yield=true` → `LogicException('You cannot enable the "use_yield" option of Twig as node "%s" is not marked as ready for it; please make it ready and then flag it with the #[\Twig\Attribute\YieldReady] attribute.')`;
else a deprecation. Irrelevant for Rust.

### 6.6 CorrectnessNodeVisitor (priority −255, runs first)

`src/NodeVisitor/CorrectnessNodeVisitor.php` (moved from the parser; errors are SyntaxErrors with node line/source):
- Child template (module has `parent`): every root body node must be "empty output" (`isEmptyOutputNode`: capture nodes
  (`set`) are fine; `TextNode` must be blank; other `NodeOutputInterface` nodes except `BlockReferenceNode` are not;
  containers are checked recursively) else
  `A template that extends another one cannot include content outside Twig blocks. Did you forget to put the content inside a {% block %} tag?`
  (VERIFIED: `… inside a {% block %} tag in "content_child" at line 1?`).
- `extends` inside a block → `Cannot use "extend" in a block.`; inside a macro → `Cannot use "extend" in a macro.`;
  a second `extends` → `Multiple extends tags are forbidden.` (also thrown by `Parser::setParent`).
- ConfigNodes (`extends`, `use`, `macro` declarations) not at the root → deprecation.
- In a child template's root, a `{% block %}` nested under an output-wrapping tag (any tagged node other than block,
  e.g. `if`, `for`, `with`; not inside a block/macro/capture) →
  `A "block" tag cannot be under a "%s" tag (line %d).` (VERIFIED `A "block" tag cannot be under a "if" tag (line 1) in "block_in_if" at line 1.`;
  also `{% autoescape false %}{% block b %}…` at a child's root → `A "block" tag cannot be under a "autoescape" tag (line 2) in "c1" at line 2.`,
  while inside a block, or inside `{% set x %}…{% endset %}`, it is accepted).
- Macro reference without call parentheses (not a `defined` test) → deprecation (once per call site).

### 6.7 ProfilerNodeVisitor

Wraps module display (`display_start`/`display_end`), each block body and each macro body with Enter/LeaveProfileNode
(§4.2). No output effect.

---

## 7. Environment

`src/Environment.php`.

**Options** (`:122-139`, defaults):

| option | default | effect |
|---|---|---|
| `debug` | `false` | debug mode: `render()` uses plain `ob_start()`, compiled `getSourceContext()` embeds source; `auto_reload` defaults to it |
| `charset` | `'UTF-8'` | `setCharset()`: `strtoupper`; `UTF8` → `UTF-8` (VERIFIED `utf8` → `UTF-8`, `iso-8859-1` → `ISO-8859-1`, `''` → `''`) |
| `strict_variables` | `false` | compile-time: name checks (§4.3), GetAttr not optimized (§4.4), getAttribute throws |
| `autoescape` | `'html'` | EscaperExtension default strategy: string, `false`, `'name'` (→ `FileExtensionEscapingStrategy::guess`), or any non-string callable `fn(string $templateName): string\|false` |
| `cache` | `false` | `false` → NullCache; string → `FilesystemCache($dir, auto_reload ? FORCE_BYTECODE_INVALIDATION : 0)`; or a `CacheInterface`; else `LogicException('Cache can only be a string, false, or a \Twig\Cache\CacheInterface implementation.')` |
| `auto_reload` | `null` (= debug) | check `isTemplateFresh()` before using cached code |
| `optimizations` | `-1` | OptimizerNodeVisitor mode (§6.3) |
| `use_yield` | `false` | §3.2 |

Constructor (`:118-153`) registers, in order: `CoreExtension`, `EscaperExtension($options['autoescape'])`,
`YieldNotReadyExtension`, `OptimizerExtension`; a default runtime loader providing `EscaperRuntime($charset)`.

**FileExtensionEscapingStrategy::guess($name)** (`src/FileExtensionEscapingStrategy.php`): name ending with `/` or `\`
→ `'html'`; strip a trailing `.twig`; extension = `pathinfo($name, PATHINFO_EXTENSION)` (case-sensitive): `js`, `json` →
`'js'`; `css` → `'css'`; `txt` → `false`; else `'html'` (VERIFIED `a.TXT` → `html`, `a.json.twig` → `js`). Results are
cached per name (max 1000 entries, FIFO eviction) in `EscaperExtension::getDefaultStrategy()` (`src/Extension/EscaperExtension.php:122-140`).
String strategies are never treated as callables.

**Extension lifecycle** (`src/ExtensionSet.php`):
- `addExtension()` keyed by class (AttributeExtension: by its target class); duplicates →
  `LogicException('Unable to register extension "%s" as it is already registered.')`; after initialization →
  `LogicException('Unable to register extension "%s" as extensions have already been initialized.')` (VERIFIED).
- Initialization happens lazily on the first `getFilter/getFunction/getTest/getTokenParser/getNodeVisitors/getExpressionParsers/get*s()`
  (`initExtensions`, `:484-502`): iterates extensions in registration order, then the staging extension
  (`Environment::addFilter/addFunction/addTest/addTokenParser/addNodeVisitor`). Later definitions of the same name
  override earlier ones; names containing `*` are also registered as dynamic patterns. After that, `addFilter` etc. throw
  `LogicException('Unable to add filter "%s" as extensions have already been initialized.')` (function/test analogous),
  `'Unable to add a node visitor as extensions have already been initialized.'`, `'Unable to add a token parser as extensions have already been initialized.'`.
- `isInitialized()` is also true after `loadTemplate()` (`initRuntime()`), which freezes globals (§3.4).
- `getExtension($class)`: `RuntimeError('The "%s" extension is not enabled.')` (VERIFIED); `getRuntime($class)`:
  registered runtime loaders, then the default loader, then an extension instance of that class, else
  `RuntimeError('Unable to load the "%s" runtime.')` (VERIFIED).
- `registerUndefinedFilterCallback/FunctionCallback/TestCallback/TokenParserCallback`: consulted after exact and dynamic
  lookups; return `false` to decline.

**Template classes and caching by name**: `getTemplateClass($name, $index)` (`:329-340`) =
`'__TwigTemplate_'.xxh128(cacheKey . optionsHash [. ':autoescape:'.encodedStrategy] [. ':sandbox:'.signature])` plus
`'___'.$index` for embeds. `cacheKey = hotCache[$name] ?? loader->getCacheKey($name)` (ArrayLoader: `name:source`;
FilesystemLoader: resolved path relative to its root path). Two names resolving to the same cache key share one template
instance. `optionsHash` = `implode(':', [serialize(extension class names), PHP_MAJOR, PHP_MINOR, VERSION, (int) debug, (int) strict, use_yield?'1':'0', optimizations])`.
`removeCache($name)` forces a fresh class name (random suffix) and drops the wrapper cache; non-removable caches throw
`LogicException('The "%s" cache class does not support removing template cache as it does not implement the "RemovableCacheInterface" interface.')`.
`isTemplateFresh($name, $time)` = `extensionSet->getLastModified() <= $time && loader->isFresh($name, $time)`.

**createTemplate($source, ?$name)** (`:469-489`): name = `__string_template__<xxh128 hex of source>` or
`"<name> (string template <hash>)"`; temporarily installs `ChainLoader([ArrayLoader([name => source]), currentLoader])`,
loads, restores the loader. VERIFIED: `__string_template__07592801e7ab6f3c2b0b6a04f5a254fc` for `hello {{ name }}`,
`foo.html (string template 07592801e7ab6f3c2b0b6a04f5a254fc)`, and error messages use these names
(`Variable "x" does not exist in "__string_template__0f4bf23d1ec4213083ab0c66d398ab90" at line 1.`). Exact parity
requires XXH3-128 (`hash('xxh128')`, lowercase hex). `template_from_string(template, name = null)`
(StringLoaderExtension, not enabled by default) = `createTemplate((string) template, name)`.

**resolveTemplate($names)** (`:516-545`): non-array → `load()`; array: each element — `Template` (deprecated; ownership
check `RuntimeError('A "Twig\Template" can only be used with the "Twig\Environment" that created it.')`),
`TemplateWrapper` → load it; a string that `exists()` (skipped check when the array has exactly one element) → load;
none → `LoaderError(sprintf('Unable to find one of the following templates: "%s".', implode('", "', $names)))`
(`[]` → `… templates: "".`, VERIFIED).

**Other API**: `tokenize()`, `parse()`, `compile()`, `compileSource()`, `setLexer/Parser/Compiler`, `getLoader/setLoader`,
`getCharset`, `hasExtension`, `getExtensions`, `getFilters/getFunctions/getTests/getTokenParsers/getNodeVisitors`
(internal), `addGlobal/getGlobals/resetGlobals`, `enable/disableDebug`, `enable/disableAutoReload`,
`enable/disableStrictVariables` (these reset the options hash), `mergeGlobals()` (deprecated), `__clone` (deprecated).

---

## 8. Errors

### 8.1 Hierarchy and constructor

`Twig\Error\Error extends \Exception` (`src/Error/Error.php`), subclasses `LoaderError`, `RuntimeError`, `SyntaxError`
(adds `addSuggestions()`), and `Twig\Sandbox\SecurityError` (→ `SecurityNotAllowedTagError`, `…FilterError`,
`…FunctionError`, `…TestError`, `…MethodError`, `…PropertyError`, each carrying the offending name; method/property
errors also the class name).
`__construct(string $message, int $lineno = -1, ?Source $source = null, ?\Throwable $previous = null, ?int $columnno = null)`.
Accessors: `getRawMessage()`, `getTemplateLine()/setTemplateLine()`, `getTemplateColumn()/setTemplateColumn()`,
`getSourceContext()/setSourceContext()`, `guess()`, `appendMessage()`. Every setter recomputes the message.

When each is thrown:
- `LoaderError`: loaders (missing template, invalid names, namespaces), `resolveTemplate()` with no match.
- `SyntaxError`: lexer, parser, token parsers, node constructors and visitors at compile time, argument resolution
  (§4.5), constant regex in `matches`, non-Twig exceptions during compilation (wrapped by `compileSource`).
- `RuntimeError`: strict variables, getAttribute, missing blocks/macros/traits, runtime helpers (`merge`, `cycle`,
  `constant`, `matches`, escaping…), wrapper for any non-Twig exception during rendering (§3.12),
  `TemplateWrapper::unwrap()`, `getRuntime/getExtension`, corrupted cache.
- `SecurityError`s: sandbox policy (§11).
- Plain PHP exceptions (`LogicException`, `InvalidArgumentException`, `RuntimeException`) for API misuse (extension
  registration, cache directory, node construction).

### 8.2 Message formatting (exact rules)

`updateRepr()` (`:131-160`):
```
message = rawMessage
last = last byte of message
punct = (last == '.' or last == '?') ? last : ''
if punct: message = message without its last byte
if source and source.getName() != '': message .= sprintf(' in "%s"', source.getName())
if lineno > 0:
    message .= sprintf(' at line %d', lineno)
    if columnno !== null: message .= sprintf(' column %d', columnno)
if punct: message .= punct
```
(also sets the PHP `file`/`line` of the exception to the template path/line when the source has a path — irrelevant).
Examples (VERIFIED): `Variable "foo" does not exist.` → `Variable "foo" does not exist in "index" at line 1.`;
`Unknown "cl" function. Did you mean "cycle", "include", "include_only"?` → `… "include_only" in "index" at line 1?`;
`twig error with line` (line 42) → `twig error with line in "twigerr_line" at line 42` (no punctuation);
`Block "nope" on template "parent" does not exist.` with line 0 → `… does not exist in "parent".`;
syntax error with column → `… ("name" expected) in "syntax2" at line 3 column 8.`
Columns (1-based byte column, `Source::getColumn()`, CRLF/CR normalized to LF before counting) are only set by the
lexer (`Unexpected character "%s".`, `Unexpected "%s".`) and `TokenStream::expect()`.
`appendMessage()` appends to the raw message (used by suggestions and the parser's "expecting closing tag" hint).
`SyntaxError::addSuggestions($name, $items)`: §4.5.

### 8.3 guess()

§3.12. Only acts when `lineno == -1`; sets `lineno = 0` first, so failure to find the template leaves 0 (no line in the
message). Called by `Template::handleException()`, `Template::load()` (line −1 callers), and nowhere else.

### 8.4 Catalogue of runtime messages owned by the runtime model (verbatim)

| message | source |
|---|---|
| `Variable "%s" does not exist.` | NameExpression (strict) |
| `An exception has been thrown during the rendering of a template ("%s").` | Template::handleException |
| `An exception has been thrown during the compilation of a template ("%s").` | Environment::compileSource |
| `Block "%s" on template "%s" does not exist.` | Template::yieldBlock, BlockChain |
| `Block "%s" should not call parent() in "%s" as the block does not exist in the parent template "%s".` | Template::yieldBlock |
| `The template has no parent and no traits defining the "%s" block.` | Template::yieldParentBlock |
| `A block must be a method on a \Twig\Template instance.` (LogicException) | Template::yieldBlock |
| `Template "%s" cannot be used as a trait.` | compiled constructor |
| `Block "%s" is not defined in trait "%s".` | compiled constructor |
| `Macros imported in the body of template "%s" are not available because the body was not rendered; move the "import" or "from" tag inside the block or the macro that uses it.` | Template::throwUninitializedMacroNamespace |
| `Macro "%s" is not defined in template "%s".` | MacroNamespace::call |
| `Positional arguments cannot be used after named arguments for macro "%s".` | TwigMacro |
| `Argument "%s" is defined twice for macro "%s".` | TwigMacro (runtime), MacroNode (definition) |
| `Variables passed to the "with" tag must be a mapping.` | WithNode |
| `Accessing \Twig\Template attributes is forbidden.` | getAttribute |
| getAttribute messages | §5.1 |
| `Failed to load Twig template "%s", index "%s": cache might be corrupted.` | Environment::loadTemplate |
| `A "Twig\TemplateWrapper" can only be used with the "Twig\Environment" that created it.` | TemplateWrapper::unwrap |
| `A "Twig\Template" can only be used with the "Twig\Environment" that created it.` | Environment::resolveTemplate |
| `Unable to find one of the following templates: "%s".` (LoaderError) | Environment::resolveTemplate |
| `Unable to load the "%s" runtime.` / `The "%s" extension is not enabled.` | Environment / ExtensionSet |
| helper messages (`merge`, `cycle`, `constant`, `matches`, `has some/every`, `checkArrow`, enum, escaping) | §5.4, §5.5 |

---

## 9. Loaders

`LoaderInterface` (`src/Loader/LoaderInterface.php`): `getSourceContext(string $name): Source` (LoaderError when
missing), `getCacheKey(string $name): string`, `isFresh(string $name, int $time): bool`, `exists(string $name)`.
`Source` (`src/Source.php`): code, logical name (used in error messages, `_self`, `getTemplateName()`), path
(possibly lazy), `getColumn($offset)`.

### 9.1 ArrayLoader (`src/Loader/ArrayLoader.php`, final)

Map name → source (`setTemplate()` adds). Lookups use `isset($this->templates[$name])` (PHP array key semantics:
numeric-string names normalize to int keys; a `null` value counts as missing).
- `getSourceContext($name)` → `new Source($code, $name)` (no path) or `LoaderError(sprintf('Template "%s" is not defined.', $name))`.
- `getCacheKey($name)` → `$name.':'.$code` (same message when missing).
- `isFresh()` → `true` (same message when missing). `exists()` → isset.

### 9.2 ChainLoader (`src/Loader/ChainLoader.php`, final)

Ordered loaders (`addLoader()` appends; an iterable is materialized lazily). `exists()` is memoized per name
(`hasSourceCache`, reset by `addLoader`). `getSourceContext/getCacheKey/isFresh`: for each loader whose `exists($name)`
is true, try it; a `LoaderError` is collected (`getSourceContext`: message only; others: `Class: message`) and the next
loader is tried; if none succeeds:
`LoaderError(sprintf('Template "%s" is not defined%s.', $name, $exceptions ? ' ('.implode(', ', $exceptions).')' : ''))`
(VERIFIED `Template "zzz" is not defined.`). Loaders whose `exists()` is false are skipped silently.

### 9.3 FilesystemLoader (`src/Loader/FilesystemLoader.php`)

State: `paths[namespace] = list<path>` (`MAIN_NAMESPACE = '__main__'`), `cache[normalizedName] = resolved path`,
`errorCache[originalName] = message`, `rootPath` (`$rootPath ?? getcwd()` + `DIRECTORY_SEPARATOR`; `realpath`-ed when
given explicitly).
- `__construct($paths = [], ?string $rootPath = null)`: `setPaths($paths)` when non-empty.
- `setPaths($paths, $ns = '__main__')`: resets the namespace, `addPath` each. `getPaths($ns)` (`[]` for unknown),
  `getNamespaces()` (`array_keys($this->paths)`, insertion order). A loader constructed without paths has **no**
  `__main__` namespace (a plain name then fails with `There are no registered paths for namespace "__main__".`,
  VERIFIED; `getNamespaces()` → `[]`).
- `addPath($path, $ns = '__main__')` / `prependPath(...)`: clears both caches; unless a third `false` argument is
  passed, checks `is_dir(isAbsolutePath($path) ? $path : $rootPath.$path)` else
  `LoaderError(sprintf('The "%s" directory does not exist ("%s").', $path, $checkPath))` (VERIFIED); stores
  `rtrim($path, '/\\')`.
- `isAbsolutePath($f)`: starts with `/` or `\`, or `X:` + `/`/`\` with length > 3 and alpha drive letter, or
  `parse_url($f, PHP_URL_SCHEME) !== null` (stream wrappers / URLs).
- `normalizeName($name)` (`:246-276`):
  ```
  name = str_replace('\\', '/', name)
  if !preg_match('#(?:^|/)[/.]|^@__main__/#', name): return name           # fast path
  prefix = ''
  if name[0] == '@' and '/' in name: prefix = up to and including the first '/'; name = rest
  parts = []
  for part in explode('/', name):
      if part == '..' and parts non-empty and last(parts) != '..': pop
      elif part != '' and part != '.': push
  name = implode('/', parts)
  if prefix == '' or prefix == '@__main__/':
      return name starts with '@' ? '@__main__/'.name : name                 # keep "@x" files in the main namespace unambiguous
  return prefix.name
  ```
- `parseName($name)`: `@ns/rest` → `[ns, rest]`; `@` without `/` →
  `LoaderError(sprintf('Malformed namespaced template name "%s" (expecting "@namespace/template_name").', $name))`;
  else `['__main__', $name]`.
- `validateName($name, $originalName)`: NUL byte → `LoaderError('A template name cannot contain NUL bytes.')`;
  `ltrim('/')`, walk parts (`..` → level−1, `.` → unchanged, else level+1); level < 0 at any point →
  `LoaderError(sprintf('Looks like you try to load a template outside configured directories (%s).', $originalName))`.
- `findTemplate($name, $throw = true)` (`:184-244`):
  ```
  original = name; name = normalizeName(name)
  if cache[name]: return it
  if errorCache[original]: throw/return null with the cached message
  [ns, short] = parseName(name)       # LoaderError propagates (or null when !throw)
  validateName(short, original)
  if !paths[ns]: errorCache[original] = sprintf('There are no registered paths for namespace "%s".', ns); throw
  for path in paths[ns]:
      if !isAbsolutePath(path): path = rootPath.path
      if is_file(path.'/'.short): return cache[name] = realpath(...) ?: path.'/'.short
  errorCache[original] = sprintf('Unable to find template "%s" (looked into: %s).', original, implode(', ', paths[ns]))   # registered (rtrimmed) paths, not absolutized
  throw
  ```
- `getSourceContext($name)`: `new Source(file_get_contents($path), normalizeName($name), $path)` — the **logical name
  is normalized** (VERIFIED: `sub/../index.twig` → `_self` = `index.twig`; `./@at.twig` → `@__main__/@at.twig`;
  `sub\a.twig` → `sub/a.twig`; `/index.twig` → `index.twig`; `@admin/x.twig` keeps its prefix).
- `getCacheKey($name)`: resolved path with the `rootPath` prefix stripped when it starts with it.
- `exists($name)`: cached hit or `findTemplate($name, false) !== null` (never throws). It looks the **normalised**
  name up, so its failures are cached under the normalised name (`findTemplate()` caches failures under the name it is
  given) (VERIFIED).
- `isFresh($name, $time)`: `filemtime($path) < $time`.
- Path bookkeeping (VERIFIED, `tests/vectors/loader/scenarios.json`): `addPath('/')` stores `''` (`rtrim('/', '/\\')`),
  which then designates the root path; `setPaths([])` creates the namespace (`getNamespaces()` lists it) and clears
  neither cache, so a lookup then fails with `Unable to find template "x" (looked into: ).`; every `addPath()` /
  `prependPath()` clears both caches before checking the directory, even when the check fails.
- `file://` root paths and directories (VERIFIED): `isAbsolutePath()` accepts them (`parse_url()` scheme), and the
  URL form is kept in the template path (`Source::getPath()`) and in the cache key.
- PHP warnings on the way (VERIFIED, with the default error handler PHP carries on): a cached file deleted afterwards
  gives `file_get_contents(<path>): Failed to open stream: No such file or directory` and `Source('')`; a cached path
  that became a directory gives the notice `file_get_contents(): Read of <st_size + 8192> bytes failed with errno=21 Is
  a directory` and `Source('')`; `isFresh()` on a vanished file gives `filemtime(): stat failed for <path>` and
  compares `false < $time` (`$time != 0`). With warnings converted to exceptions (`ErrorException`), a template that
  includes such a template wraps it: `An exception has been thrown during the rendering of a template
  ("file_get_contents(…): …")` (`Template::load()` only catches `Twig\Error\Error`; VERIFIED for the tag, the
  function, `ignore missing`, `extends`, `embed`, `import` and `source()`). A root path containing a NUL byte throws
  `realpath()`'s `ValueError: realpath(): Argument #1 ($path) must not contain any null bytes`.
VERIFIED messages: `Looks like you try to load a template outside configured directories (../index.twig).`,
`… (sub/../../index.twig).`, `… (@admin/../x.twig).`, `There are no registered paths for namespace "nope".`,
`Malformed namespaced template name "@admin" (expecting "@namespace/template_name").`,
`Unable to find template "missing.twig" (looked into: <ROOT>/tpl, <ROOT>/tpl2).`, `A template name cannot contain NUL bytes.`,
`Unable to find template "" (looked into: …).` When raised through `{% include %}` etc., the including template and line
are appended (§3.11).

---

## 10. Cache (brief)

`CacheInterface` (`generateKey($name, $className)`, `write($key, $content)`, `load($key)`, `getTimestamp($key)`),
`RemovableCacheInterface::remove($name, $cls)`.
- `NullCache`: no-op (templates are compiled and `eval`'d on every new process; within a process they are cached in
  `Environment::$loadedTemplates`).
- `FilesystemCache($dir, $options)`: key = `$dir/<h[0..1]>/<h>.php` with `h = xxh128($className)`; atomic write via
  temp file + rename, chmod `0666 & ~umask()`, optional opcache invalidation; `RuntimeException`s:
  `Unable to create the cache directory (%s).`, `Unable to write in the cache directory (%s).`, `Failed to write cache file "%s".`,
  `Failed to delete cache file "%s".`
- `ReadOnlyFilesystemCache`: `write()` no-op. `ChainCache(iterable $caches)`: key `className#name`, writes to all, loads
  from the first that defines the class, timestamp from the first non-zero.
Rust: compiled PHP caching is irrelevant; an equivalent is caching parsed/optimized ASTs per (template cache key,
options hash) with `auto_reload` freshness checks via `loader.isFresh()`.

---

## 11. Sandbox (brief)

Pieces: `SandboxExtension($policy, $sandboxed = false)` (internal since 3.29; registers `SandboxTokenParser` (`{% sandbox %}`,
deprecated) and `SandboxNodeVisitor`), `SecurityChecker` (state: policy, `sandboxedGlobally`, toggle `sandboxed`,
deprecated source policy), `SecurityPolicy` (allow-lists; `setStrict(true)` = 4.0 behaviour), `Sandbox($env, $policy)`
(userland façade; requires a strict `SecurityPolicy`: `LogicException('The sandbox requires a strict security policy, call "setStrict(true)" on a dedicated policy for this sandbox.')`
and a fresh environment).

Runtime checks in compiled code (VERIFIED Appendix A.13):
- Constructor: `$this->sandbox = …->getChecker();` before `parent::__construct`; `ensureTraitsAllowed()` →
  `checkTraitsAllowed()` (only the `use` tag) before traits are loaded.
- `Template::yield()` / `yieldBlock()` / `getParent()` / macro lookup call `ensureSecurityChecked()` →
  if `isSandboxed($this->source)`: `checkSecurity()` = `$this->sandbox->checkSecurity(tags, filters, functions, tests, $this->source)`;
  `SecurityNotAllowed*Error` get the first-use line from the static maps.
- `CheckToStringNode` → `ensureToStringAllowed($v, LINE, $this->source)`: when sandboxed, every `Stringable` must be
  allowed to call `__toString` (`checkMethodAllowed($obj, '__toString')`); arrays and Traversables are walked
  recursively (iterators are materialized to arrays; `IteratorAggregate` walked and returned as is).
- `getAttribute(..., $sandboxed = true)`: `checkPropertyAllowed` / `checkMethodAllowed` (§5.1).
- `needs_is_sandboxed` callables receive `isSandboxed($this->source)`; arrow-taking filters then require real Closures.
Policy messages (`src/Sandbox/SecurityPolicy.php`): `Tag "%s" is not allowed.`, `Filter "%s" is not allowed.`,
`Function "%s" is not allowed.`, `Test "%s" is not allowed.` (strict mode only; non-strict: deprecation),
`Calling "%s" method on a "%s" object is not allowed.` (method lowercased), `Calling "%s" property on a "%s" object is not allowed.`
Non-strict policies implicitly allow (with deprecations) the `extends`/`use` tags, the `parent`/`block`/`attribute`
functions and all tests. `Template` instances and `Markup` methods (except `__construct`, `withContent`) are always
allowed. `include(…, sandboxed=true)` and `{% sandbox %}` toggle the checker's `sandboxed` flag for the duration.

---

## 12. Appendix A — generated PHP for representative templates

Produced by `scratchpad/rt-work/gen_appendix.php` with Twig 3.31.1 on PHP 8.5.13 (`cache => false`, default options unless
stated). Output is verbatim except the fixed `use` header and, after A.1, the five trailing boilerplate methods. Whitespace
quirks (e.g. `yield from         $this->…`) are genuine compiler output.

### A.1 Inheritance: blocks, parent(), block(), block() is defined, constant parent

Templates:

`base.twig`:
```twig
<html>{% block title %}Base title{% endblock %}|{% block content %}base content{% endblock %}</html>
```

`child.twig`:
```twig
{% extends 'base.twig' %}
{% block title %}Child - {{ parent() }}{% endblock %}
{% block content %}{{ block('title') }}/{{ block('title', 'base.twig') }}/{{ block('sidebar') is defined ? 'y' : 'n' }}{% endblock %}
```

Compiled `child.twig`:
```php
<?php
// [fixed `use` header omitted, see §2.1]
/* child.twig */
class __TwigTemplate_39847c2dda4747b65198a297e0171962 extends Template
{
    private Source $source;
    /**
     * @var array<string, MacroNamespace>
     */
    private array $macros = [];

    public function __construct(Environment $env)
    {
        parent::__construct($env);

        $this->source = $this->getSourceContext();

        $this->blocks = [
            'title' => [$this, 'block_title'],
            'content' => [$this, 'block_content'],
        ];
    }

    protected function doGetParent(array $context): bool|string|Template|TemplateWrapper
    {
        // line 1
        return $this->parent ??= $this->load("base.twig", 1);
    }

    protected function doDisplay(array $context, array $blocks = []): iterable
    {
        $macros = $this->macros;
        $this->parent = $this->load("base.twig", 1);
        yield from $this->parent->unwrap()->yield($context, array_merge($this->blocks, $blocks));
    }

    // line 2
    /**
     * @return iterable<null|scalar|\Stringable>
     */
    public function block_title(array $context, array $blocks = []): iterable
    {
        $macros = $this->macros;
        yield "Child - ";
        yield from $this->yieldParentBlock("title", $context, $blocks);
        return; yield;
    }

    // line 3
    /**
     * @return iterable<null|scalar|\Stringable>
     */
    public function block_content(array $context, array $blocks = []): iterable
    {
        $macros = $this->macros;
        yield from         $this->unwrap()->yieldBlock("title", $context, $blocks);
        yield "/";
        yield from         $this->load("base.twig", 3)->unwrap()->yieldBlock("title", $context);
        yield "/";
        yield (string) ((        $this->unwrap()->hasBlock("sidebar", $context, $blocks)) ? ("y") : ("n"));
        return; yield;
    }

    /**
     * @codeCoverageIgnore
     */
    public function getTemplateName(): string
    {
        return "child.twig";
    }

    /**
     * @codeCoverageIgnore
     */
    public function isTraitable(): bool
    {
        return false;
    }

    /**
     * @codeCoverageIgnore
     */
    public function getDefaultEscapeStrategy(): string|false
    {
        return "html";
    }

    /**
     * @codeCoverageIgnore
     */
    public function getDebugInfo(): array
    {
        return array (  66 => 3,  54 => 2,  43 => 1,);
    }

    public function getSourceContext(): Source
    {
        return new Source("", "child.twig", "");
    }
}
```

Compiled `base.twig`:
```php
<?php
// [fixed `use` header omitted, see §2.1]
/* base.twig */
class __TwigTemplate_32c24d7a6e64cad691ee92d0aabcf4d7 extends Template
{
    private Source $source;
    /**
     * @var array<string, MacroNamespace>
     */
    private array $macros = [];

    public function __construct(Environment $env)
    {
        parent::__construct($env);

        $this->source = $this->getSourceContext();

        $this->parent = false;

        $this->blocks = [
            'title' => [$this, 'block_title'],
            'content' => [$this, 'block_content'],
        ];
    }

    protected function doDisplay(array $context, array $blocks = []): iterable
    {
        $macros = $this->macros;
        // line 1
        yield "<html>";
        yield from $this->unwrap()->yieldBlock('title', $context, $blocks);
        yield "|";
        yield from $this->unwrap()->yieldBlock('content', $context, $blocks);
        yield "</html>";
        return; yield;
    }

    /**
     * @return iterable<null|scalar|\Stringable>
     */
    public function block_title(array $context, array $blocks = []): iterable
    {
        $macros = $this->macros;
        yield "Base title";
        return; yield;
    }

    /**
     * @return iterable<null|scalar|\Stringable>
     */
    public function block_content(array $context, array $blocks = []): iterable
    {
        $macros = $this->macros;
        yield "base content";
        return; yield;
    }

    /**
     * @codeCoverageIgnore
     */
    public function getTemplateName(): string
    {
        return "base.twig";
    }

    /**
     * @codeCoverageIgnore
     */
    public function getDefaultEscapeStrategy(): string|false
    {
        return "html";
    }

    /**
     * @codeCoverageIgnore
     */
    public function getDebugInfo(): array
    {
        return array (  46 => 1,);
    }

    public function getSourceContext(): Source
    {
        return new Source("", "base.twig", "");
    }
}
```

Render `child.twig` with context `[]` →
```text
<html>Child - Base title|Child - Base title/Base title/n</html>
```

### A.2 Dynamic parent

Templates:

`dyn.twig`:
```twig
{% extends layout %}{% block a %}A{% endblock %}
```

`l1`:
```twig
[{% block a %}{% endblock %}]
```

Compiled `dyn.twig`:
```php
<?php
// [fixed `use` header omitted, see §2.1]
/* dyn.twig */
class __TwigTemplate_560eede3b09417aa64335b11fdfa7b91 extends Template
{
    private Source $source;
    /**
     * @var array<string, MacroNamespace>
     */
    private array $macros = [];

    public function __construct(Environment $env)
    {
        parent::__construct($env);

        $this->source = $this->getSourceContext();

        $this->blocks = [
            'a' => [$this, 'block_a'],
        ];
    }

    protected function doGetParent(array $context): bool|string|Template|TemplateWrapper
    {
        $macros = $this->macros;
        // line 1
        return $this->load(($context["layout"] ?? null), 1);
    }

    protected function doDisplay(array $context, array $blocks = []): iterable
    {
        $macros = $this->macros;
        yield from $this->getParent($context)->unwrap()->yield($context, array_merge($this->blocks, $blocks));
    }

    /**
     * @return iterable<null|scalar|\Stringable>
     */
    public function block_a(array $context, array $blocks = []): iterable
    {
        $macros = $this->macros;
        yield "A";
        return; yield;
    }

    // [getTemplateName / isTraitable / getDefaultEscapeStrategy / getDebugInfo / getSourceContext omitted]
}
```

Render `dyn.twig` with context `{"layout":"l1"}` →
```text
[A]
```

Render `dyn.twig` with context `{"layout":["missing","l1"]}` →
```text
[A]
```

### A.3 Macros: defaults, implicit varargs, explicit variadic, import _self, from-import, defined test

Templates:

`macros.twig`:
```twig
{% macro input(name, value = '', type = 'text', size = 20) %}
<input type="{{ type }}" name="{{ name }}" value="{{ value|e }}" size="{{ size }}"/>{{ varargs|length }}
{% endmacro %}
{% macro list(title, ...items) %}{{ title }}:{% for i in items %}[{{ i }}]{% endfor %}{% endmacro %}
{% import _self as forms %}
{% from 'macros.twig' import list as l %}
{{ forms.input('user') }}
{{ forms.input('pwd', type='password', extra=1) }}
{{ l('T', 1, 2, 3) }}
{{ forms.list is defined ? 'def' : 'undef' }}
{{ _self.input('x') }}
```

Compiled `macros.twig`:
```php
<?php
// [fixed `use` header omitted, see §2.1]
/* macros.twig */
class __TwigTemplate_8f7cef9f6c0a550059627ac3bc3ba54a extends Template
{
    private Source $source;
    /**
     * @var array<string, MacroNamespace>
     */
    private array $macros = [];
    private \Twig\Runtime\EscaperRuntime $escaper;

    public function __construct(Environment $env)
    {
        parent::__construct($env);

        $this->source = $this->getSourceContext();

        $this->escaper = $env->getRuntime('Twig\Runtime\EscaperRuntime');

        $this->parent = false;

        $this->blocks = [
        ];
    }

    protected function doDisplay(array $context, array $blocks = []): iterable
    {
        $macros = $this->macros;
        // line 5
        $macros["forms"] = $this->macros["forms"] = $this->getMacroNamespace();
        // line 6
        $macros["_v0"] = $this->macros["_v0"] = $this->load("macros.twig", 6)->unwrap()->getMacroNamespace();
        // line 7
        yield (string) ($macros["forms"] ?? $this->throwUninitializedMacroNamespace(7))->call("input", ["user"], $context, 7, $this->source);
        yield "
";
        // line 8
        yield (string) ($macros["forms"] ?? $this->throwUninitializedMacroNamespace(8))->call("input", ["pwd", "type" => "password", "extra" => 1], $context, 8, $this->source);
        yield "
";
        // line 9
        yield (string) ($macros["_v0"] ?? $this->throwUninitializedMacroNamespace(6))->call("list", ["T", 1, 2, 3], $context, 9, $this->source);
        yield "
";
        // line 10
        yield (string) ((($macros["forms"] ?? $this->throwUninitializedMacroNamespace(10))->has("list", $context)) ? ("def") : ("undef"));
        yield "
";
        // line 11
        yield (string) $this->getMacroNamespace()->call("input", ["x"], $context, 11, $this->source);
        return; yield;
    }

    protected function loadDeclaredMacros(): array
    {
        return [
            "input" => new \Twig\TwigMacro("input", function ($name = null, $value = "", $type = "text", $size = 20, ...$varargs): string|Markup {
                // line 1
                $macros = $this->macros;
                $context = [
                    "name" => $name,
                    "value" => $value,
                    "type" => $type,
                    "size" => $size,
                    "varargs" => $varargs,
                ] + $this->env->getGlobals();

                $blocks = [];

                return ('' === $tmp = \Twig\Extension\CoreExtension::captureOutput((function () use (&$context, $macros, $blocks) {
                    // line 2
                    yield "<input type=\"";
                    yield (string) $this->escaper->escape(($context["type"] ?? null), "html", null, true);
                    yield "\" name=\"";
                    yield (string) $this->escaper->escape(($context["name"] ?? null), "html", null, true);
                    yield "\" value=\"";
                    yield (string) $this->escaper->escape(($context["value"] ?? null));
                    yield "\" size=\"";
                    yield (string) $this->escaper->escape(($context["size"] ?? null), "html", null, true);
                    yield "\"/>";
                    yield (string) $this->escaper->escape(Twig\Extension\CoreExtension::length($this->env->getCharset(), ($context["varargs"] ?? null)), "html", null, true);
                    yield "
";
                    return; yield;
                })())) ? '' : new Markup($tmp, $this->env->getCharset());
            }, ["name" => false, "value" => true, "type" => true, "size" => true], false),
            "list" => new \Twig\TwigMacro("list", function ($title = null, ...$items): string|Markup {
                // line 4
                $macros = $this->macros;
                $context = [
                    "title" => $title,
                    "items" => $items,
                ] + $this->env->getGlobals();

                $blocks = [];

                return ('' === $tmp = \Twig\Extension\CoreExtension::captureOutput((function () use (&$context, $macros, $blocks) {
                    yield (string) $this->escaper->escape(($context["title"] ?? null), "html", null, true);
                    yield ":";
                    $context['_parent'] = $context;
                    $context['_seq'] = CoreExtension::ensureTraversable(($context["items"] ?? null));
                    foreach ($context['_seq'] as $context["_key"] => $context["i"]) {
                        yield "[";
                        yield (string) $this->escaper->escape($context["i"], "html", null, true);
                        yield "]";
                    }
                    $_parent = $context['_parent'];
                    unset($context['_seq'], $context['_key'], $context['i'], $context['_parent']);
                    $context = array_intersect_key($context, $_parent);
                    $context += $_parent;
                    return; yield;
                })())) ? '' : new Markup($tmp, $this->env->getCharset());
            }, ["title" => false], true),
        ];
    }

    // [getTemplateName / isTraitable / getDefaultEscapeStrategy / getDebugInfo / getSourceContext omitted]
}
```

Render `macros.twig` with context `[]` →
```text
<input type="text" name="user" value="" size="20"/>0

<input type="password" name="pwd" value="" size="20"/>1

T:[1][2][3]
def
<input type="text" name="x" value="" size="20"/>0
```

### A.4 for loop with loop.*, key/value targets and else; for without loop usage

Templates:

`for.twig`:
```twig
{% for k, v in items %}{{ loop.index }}/{{ loop.length }}:{{ k }}={{ v }}{% if loop.last %}!{% endif %}{% else %}empty{% endfor %}
{% for x in 1..3 %}{{ x }}{% endfor %}
```

Compiled `for.twig`:
```php
<?php
// [fixed `use` header omitted, see §2.1]
/* for.twig */
class __TwigTemplate_3d620be372be29b52c7cfe78cc56864f extends Template
{
    private Source $source;
    /**
     * @var array<string, MacroNamespace>
     */
    private array $macros = [];
    private \Twig\Runtime\EscaperRuntime $escaper;

    public function __construct(Environment $env)
    {
        parent::__construct($env);

        $this->source = $this->getSourceContext();

        $this->escaper = $env->getRuntime('Twig\Runtime\EscaperRuntime');

        $this->parent = false;

        $this->blocks = [
        ];
    }

    protected function doDisplay(array $context, array $blocks = []): iterable
    {
        $macros = $this->macros;
        // line 1
        $context['_parent'] = $context;
        $context['_seq'] = CoreExtension::ensureTraversable(($context["items"] ?? null));
        $context['_iterated'] = false;
        $context['loop'] = [
          'parent' => $context['_parent'],
          'index0' => 0,
          'index'  => 1,
          'first'  => true,
        ];
        if (is_array($context['_seq']) || (is_object($context['_seq']) && $context['_seq'] instanceof \Countable)) {
            $length = count($context['_seq']);
            $context['loop']['revindex0'] = $length - 1;
            $context['loop']['revindex'] = $length;
            $context['loop']['length'] = $length;
            $context['loop']['last'] = 1 === $length;
        }
        foreach ($context['_seq'] as $context["k"] => $context["v"]) {
            yield (string) $this->escaper->escape(CoreExtension::getAttribute($this->env, $this->source, $context["loop"], "index", [], "any", false, false, false, 1), "html", null, true);
            yield "/";
            yield (string) $this->escaper->escape(CoreExtension::getAttribute($this->env, $this->source, $context["loop"], "length", [], "any", false, false, false, 1), "html", null, true);
            yield ":";
            yield (string) $this->escaper->escape($context["k"], "html", null, true);
            yield "=";
            yield (string) $this->escaper->escape($context["v"], "html", null, true);
            if ((($tmp = CoreExtension::getAttribute($this->env, $this->source, $context["loop"], "last", [], "any", false, false, false, 1)) && $tmp instanceof Markup ? (string) $tmp : $tmp)) {
                yield "!";
            }
            $context['_iterated'] = true;
            ++$context['loop']['index0'];
            ++$context['loop']['index'];
            $context['loop']['first'] = false;
            if (isset($context['loop']['revindex0'], $context['loop']['revindex'])) {
                --$context['loop']['revindex0'];
                --$context['loop']['revindex'];
                $context['loop']['last'] = 0 === $context['loop']['revindex0'];
            }
        }
        if (!$context['_iterated']) {
            yield "empty";
        }
        $_parent = $context['_parent'];
        unset($context['_seq'], $context['k'], $context['v'], $context['_parent'], $context['_iterated'], $context['loop']);
        $context = array_intersect_key($context, $_parent);
        $context += $_parent;
        // line 2
        $context['_parent'] = $context;
        $context['_seq'] = CoreExtension::ensureTraversable(range(1, 3));
        foreach ($context['_seq'] as $context["_key"] => $context["x"]) {
            yield (string) $this->escaper->escape($context["x"], "html", null, true);
        }
        $_parent = $context['_parent'];
        unset($context['_seq'], $context['_key'], $context['x'], $context['_parent']);
        $context = array_intersect_key($context, $_parent);
        $context += $_parent;
        return; yield;
    }

    // [getTemplateName / isTraitable / getDefaultEscapeStrategy / getDebugInfo / getSourceContext omitted]
}
```

Render `for.twig` with context `{"items":{"a":1,"b":2}}` →
```text
1/2:a=12/2:b=2!123
```

Render `for.twig` with context `{"items":[]}` →
```text
empty123
```

### A.5 set: simple, multi-target, capture, static capture, empty capture

Templates:

`set.twig`:
```twig
{% set a = 1 %}{% set b, c = 2, 3 %}{% set d %}<b>{{ a }}</b>{% endset %}{% set e %}static{% endset %}{% set f %}{% endset %}
{{ d }}{{ e }}{{ f }}
```

Compiled `set.twig`:
```php
<?php
// [fixed `use` header omitted, see §2.1]
/* set.twig */
class __TwigTemplate_ab085da4b57b6ed98c3530e605102897 extends Template
{
    private Source $source;
    /**
     * @var array<string, MacroNamespace>
     */
    private array $macros = [];
    private \Twig\Runtime\EscaperRuntime $escaper;

    public function __construct(Environment $env)
    {
        parent::__construct($env);

        $this->source = $this->getSourceContext();

        $this->escaper = $env->getRuntime('Twig\Runtime\EscaperRuntime');

        $this->parent = false;

        $this->blocks = [
        ];
    }

    protected function doDisplay(array $context, array $blocks = []): iterable
    {
        $macros = $this->macros;
        // line 1
        $context["a"] = 1;
        [$context["b"], $context["c"]] =         [2, 3];
        $context["d"] = ('' === $tmp = \Twig\Extension\CoreExtension::captureOutput((function () use (&$context, $macros, $blocks) {
            yield "<b>";
            yield (string) $this->escaper->escape(($context["a"] ?? null), "html", null, true);
            yield "</b>";
            return; yield;
        })())) ? '' : new Markup($tmp, $this->env->getCharset());
        $context["e"] = new Markup("static", $this->env->getCharset());
        $context["f"] = "";
        // line 2
        yield (string) $this->escaper->escape(($context["d"] ?? null), "html", null, true);
        yield (string) $this->escaper->escape(($context["e"] ?? null), "html", null, true);
        yield (string) $this->escaper->escape(($context["f"] ?? null), "html", null, true);
        return; yield;
    }

    // [getTemplateName / isTraitable / getDefaultEscapeStrategy / getDebugInfo / getSourceContext omitted]
}
```

Render `set.twig` with context `[]` →
```text
<b>1</b>static
```

### A.5b set capture with use_yield=true

Templates:

`set.twig`:
```twig
{% set d %}<b>{{ a }}</b>{% endset %}{{ d }}
```

Compiled `set.twig` (options: {"use_yield":true}):
```php
<?php
// [fixed `use` header omitted, see §2.1]
/* set.twig */
class __TwigTemplate_cffc2bcfa798eebf9ff8e9d5265dfaeb extends Template
{
    private Source $source;
    /**
     * @var array<string, MacroNamespace>
     */
    private array $macros = [];
    private \Twig\Runtime\EscaperRuntime $escaper;

    public function __construct(Environment $env)
    {
        parent::__construct($env);

        $this->source = $this->getSourceContext();

        $this->escaper = $env->getRuntime('Twig\Runtime\EscaperRuntime');

        $this->parent = false;

        $this->blocks = [
        ];
    }

    protected function doDisplay(array $context, array $blocks = []): iterable
    {
        $macros = $this->macros;
        // line 1
        $context["d"] = ('' === $tmp = implode('', iterator_to_array((function () use (&$context, $macros, $blocks) {
            yield "<b>";
            yield (string) $this->escaper->escape(($context["a"] ?? null), "html", null, true);
            yield "</b>";
            return; yield;
        })(), false))) ? '' : new Markup($tmp, $this->env->getCharset());
        yield (string) $this->escaper->escape(($context["d"] ?? null), "html", null, true);
        return; yield;
    }

    // [getTemplateName / isTraitable / getDefaultEscapeStrategy / getDebugInfo / getSourceContext omitted]
}
```

Render `set.twig` with context `{"a":1}` →
```text
<b>1</b>
```

### A.6 include: plain, with, with only, ignore missing, array of names, include() function

Templates:

`include.twig`:
```twig
{% include 'part.twig' %}{% include 'part.twig' with {x: 1} %}{% include 'part.twig' with {x: 2} only %}{% include 'missing.twig' ignore missing %}{% include ['missing.twig', 'part.twig'] only %}{{ include('part.twig', {x: 3}) }}
```

`part.twig`:
```twig
[{{ x|default("-") }}]
```

Compiled `include.twig`:
```php
<?php
// [fixed `use` header omitted, see §2.1]
/* include.twig */
class __TwigTemplate_a274c449ce3b5497168abce21dd650d6 extends Template
{
    private Source $source;
    /**
     * @var array<string, MacroNamespace>
     */
    private array $macros = [];

    public function __construct(Environment $env)
    {
        parent::__construct($env);

        $this->source = $this->getSourceContext();

        $this->parent = false;

        $this->blocks = [
        ];
    }

    protected function doDisplay(array $context, array $blocks = []): iterable
    {
        $macros = $this->macros;
        // line 1
        yield from $this->load("part.twig", 1)->unwrap()->yield($context);
        yield from $this->load("part.twig", 1)->unwrap()->yield(CoreExtension::merge($context, ["x" => 1]));
        yield from $this->load("part.twig", 1)->unwrap()->yield(CoreExtension::toArray(["x" => 2]));
        try {
            $_v0 = $this->load("missing.twig", 1);
        } catch (LoaderError $e) {
            // ignore missing template
            $_v0 = null;
        }
        if ($_v0) {
            yield from $_v0->unwrap()->yield($context);
        }
        yield from $this->load(["missing.twig", "part.twig"], 1)->unwrap()->yield([]);
        yield (string) Twig\Extension\CoreExtension::include($this->env, $context, "part.twig", ["x" => 3]);
        return; yield;
    }

    // [getTemplateName / isTraitable / getDefaultEscapeStrategy / getDebugInfo / getSourceContext omitted]
}
```

Render `include.twig` with context `{"x":"X"}` →
```text
[X][1][2][-][3]
```

### A.7 embed (main class + embedded class ___1)

Templates:

`embed.twig`:
```twig
{% embed 'base.twig' with {v: 1} %}{% block content %}E{{ v }}{{ parent() }}{% endblock %}{% endembed %}
```

`base.twig`:
```twig
<{% block content %}base{% endblock %}>
```

Compiled `embed.twig`:
```php
<?php
// [fixed `use` header omitted, see §2.1]
/* embed.twig */
class __TwigTemplate_f5fb60aea11f30f3c4eeafbc85c25249 extends Template
{
    private Source $source;
    /**
     * @var array<string, MacroNamespace>
     */
    private array $macros = [];

    public function __construct(Environment $env)
    {
        parent::__construct($env);

        $this->source = $this->getSourceContext();

        $this->parent = false;

        $this->blocks = [
        ];
    }

    protected function doDisplay(array $context, array $blocks = []): iterable
    {
        $macros = $this->macros;
        // line 1
        yield from $this->load("embed.twig", 1, 1)->unwrap()->yield(CoreExtension::merge($context, ["v" => 1]));
        return; yield;
    }

    // [getTemplateName / isTraitable / getDefaultEscapeStrategy / getDebugInfo / getSourceContext omitted]
}


/* embed.twig */
class __TwigTemplate_f5fb60aea11f30f3c4eeafbc85c25249___1 extends Template
{
    private Source $source;
    /**
     * @var array<string, MacroNamespace>
     */
    private array $macros = [];
    private \Twig\Runtime\EscaperRuntime $escaper;

    public function __construct(Environment $env)
    {
        parent::__construct($env);

        $this->source = $this->getSourceContext();

        $this->escaper = $env->getRuntime('Twig\Runtime\EscaperRuntime');

        $this->blocks = [
            'content' => [$this, 'block_content'],
        ];
    }

    protected function doGetParent(array $context): bool|string|Template|TemplateWrapper
    {
        return $this->parent ??= $this->load("base.twig", 1);
    }

    protected function doDisplay(array $context, array $blocks = []): iterable
    {
        $macros = $this->macros;
        $this->parent = $this->load("base.twig", 1);
        yield from $this->parent->unwrap()->yield($context, array_merge($this->blocks, $blocks));
    }

    /**
     * @return iterable<null|scalar|\Stringable>
     */
    public function block_content(array $context, array $blocks = []): iterable
    {
        $macros = $this->macros;
        yield "E";
        yield (string) $this->escaper->escape(($context["v"] ?? null), "html", null, true);
        yield from $this->yieldParentBlock("content", $context, $blocks);
        return; yield;
    }

    // [getTemplateName / isTraitable / getDefaultEscapeStrategy / getDebugInfo / getSourceContext omitted]
}
```

Render `embed.twig` with context `[]` →
```text
<E1base>
```

### A.8 use (traits) with alias, two traits, parent() into a trait block

Templates:

`use.twig`:
```twig
{% use 'blocks.twig' with b1 as alias %}{% use 'blocks2.twig' %}{% block b2 %}own{{ parent() }}{% endblock %}{{ block('alias') }}{{ block('b3') }}
```

`blocks.twig`:
```twig
{% block b1 %}B1{% endblock %}{% block b2 %}B2{% endblock %}
```

`blocks2.twig`:
```twig
{% block b3 %}B3{% endblock %}
```

Compiled `use.twig`:
```php
<?php
// [fixed `use` header omitted, see §2.1]
/* use.twig */
class __TwigTemplate_adccc8af93004e5e22eab49f3f0a3b98 extends Template
{
    private Source $source;
    /**
     * @var array<string, MacroNamespace>
     */
    private array $macros = [];

    public function __construct(Environment $env)
    {
        parent::__construct($env);

        $this->source = $this->getSourceContext();

        $this->parent = false;

        $this->ensureTraitsAllowed();

        // line 1
        $_trait_0 = $this->load("blocks.twig", 1);
        if (!$_trait_0->unwrap()->isTraitable()) {
            throw new RuntimeError('Template "'."blocks.twig".'" cannot be used as a trait.', 1, $this->source);
        }
        $_trait_0_blocks = $_trait_0->unwrap()->getBlocks();

        if (!isset($_trait_0_blocks["b1"])) {
            throw new RuntimeError(sprintf('Block "%s" is not defined in trait "%s".', "b1", "blocks.twig"), 1, $this->source);
        }

        $_trait_0_blocks["alias"] = $_trait_0_blocks["b1"]; unset($_trait_0_blocks["b1"]); $this->traitAliases["alias"] = "b1";

        $_trait_1 = $this->load("blocks2.twig", 1);
        if (!$_trait_1->unwrap()->isTraitable()) {
            throw new RuntimeError('Template "'."blocks2.twig".'" cannot be used as a trait.', 1, $this->source);
        }
        $_trait_1_blocks = $_trait_1->unwrap()->getBlocks();

        $this->traits = array_merge(
            $_trait_0_blocks,
            $_trait_1_blocks
        );

        $this->blocks = array_merge(
            $this->traits,
            [
                'b2' => [$this, 'block_b2'],
            ]
        );
    }

    protected function doDisplay(array $context, array $blocks = []): iterable
    {
        $macros = $this->macros;
        yield from $this->unwrap()->yieldBlock('b2', $context, $blocks);
        yield from         $this->unwrap()->yieldBlock("alias", $context, $blocks);
        yield from         $this->unwrap()->yieldBlock("b3", $context, $blocks);
        return; yield;
    }

    /**
     * @return iterable<null|scalar|\Stringable>
     */
    public function block_b2(array $context, array $blocks = []): iterable
    {
        $macros = $this->macros;
        yield "own";
        yield from $this->yieldParentBlock("b2", $context, $blocks);
        return; yield;
    }

    // [getTemplateName / isTraitable / getDefaultEscapeStrategy / getDebugInfo / getSourceContext omitted]
}
```

Render `use.twig` with context `[]` →
```text
ownB2B1B3
```

### A.9 apply

Templates:

`apply.twig`:
```twig
{% apply upper|trim %} hello {{ name }} {% endapply %}
```

Compiled `apply.twig`:
```php
<?php
// [fixed `use` header omitted, see §2.1]
/* apply.twig */
class __TwigTemplate_8deb858b31f78591e7799ec0ae6a6ddf extends Template
{
    private Source $source;
    /**
     * @var array<string, MacroNamespace>
     */
    private array $macros = [];
    private \Twig\Runtime\EscaperRuntime $escaper;

    public function __construct(Environment $env)
    {
        parent::__construct($env);

        $this->source = $this->getSourceContext();

        $this->escaper = $env->getRuntime('Twig\Runtime\EscaperRuntime');

        $this->parent = false;

        $this->blocks = [
        ];
    }

    protected function doDisplay(array $context, array $blocks = []): iterable
    {
        $macros = $this->macros;
        // line 1
        $_v0 = ('' === $tmp = \Twig\Extension\CoreExtension::captureOutput((function () use (&$context, $macros, $blocks) {
            yield " hello ";
            yield (string) $this->escaper->escape(($context["name"] ?? null), "html", null, true);
            yield " ";
            return; yield;
        })())) ? '' : new Markup($tmp, $this->env->getCharset());
        yield (string) $this->escaper->escape(Twig\Extension\CoreExtension::trim(Twig\Extension\CoreExtension::upper($this->env->getCharset(), $_v0)), "html", null, true);
        return; yield;
    }

    // [getTemplateName / isTraitable / getDefaultEscapeStrategy / getDebugInfo / getSourceContext omitted]
}
```

Render `apply.twig` with context `{"name":"bob"}` →
```text
HELLO BOB
```

### A.10 autoescape (js, false, raw) and default html

Templates:

`auto.twig`:
```twig
{% autoescape "js" %}{{ v }}{{ v|raw }}{% endautoescape %}{% autoescape false %}{{ v }}{% endautoescape %}{{ v }}
```

Compiled `auto.twig`:
```php
<?php
// [fixed `use` header omitted, see §2.1]
/* auto.twig */
class __TwigTemplate_84dc2f0fd6bebce9a231cc117b50185d extends Template
{
    private Source $source;
    /**
     * @var array<string, MacroNamespace>
     */
    private array $macros = [];
    private \Twig\Runtime\EscaperRuntime $escaper;

    public function __construct(Environment $env)
    {
        parent::__construct($env);

        $this->source = $this->getSourceContext();

        $this->escaper = $env->getRuntime('Twig\Runtime\EscaperRuntime');

        $this->parent = false;

        $this->blocks = [
        ];
    }

    protected function doDisplay(array $context, array $blocks = []): iterable
    {
        $macros = $this->macros;
        // line 1
        yield (string) $this->escaper->escape(($context["v"] ?? null), "js", null, true);
        yield (string) ($context["v"] ?? null);
        yield (string) ($context["v"] ?? null);
        yield (string) $this->escaper->escape(($context["v"] ?? null), "html", null, true);
        return; yield;
    }

    // [getTemplateName / isTraitable / getDefaultEscapeStrategy / getDebugInfo / getSourceContext omitted]
}
```

Render `auto.twig` with context `{"v":"<'a'>"}` →
```text
\u003C\u0027a\u0027\u003E<'a'><'a'>&lt;&#039;a&#039;&gt;
```

### A.11 with, do, deprecated, guard, types, flush, if/elseif/else

Templates:

`tags.twig`:
```twig
{% do a.b(1) %}
{% deprecated 'The "tags" template is deprecated.' package='acme/pkg' version='1.2' %}
{% guard filter upper %}U{% else %}noU{% endguard %}
{% guard function nope %}yes{{ nope() }}{% else %}no{% endguard %}
{% types {foo: 'string', bar?: 'int'} %}
{% flush %}
{% with {x: 1} only %}{{ x }}{% endwith %}
{% with %}{% set y = 2 %}{% endwith %}
{% if a %}A{% elseif b %}B{% else %}C{% endif %}
```

Compiled `tags.twig`:
```php
<?php
// [fixed `use` header omitted, see §2.1]
/* tags.twig */
class __TwigTemplate_2543cba0ec68d32215bdd7d06135299b extends Template
{
    private Source $source;
    /**
     * @var array<string, MacroNamespace>
     */
    private array $macros = [];
    private \Twig\Runtime\EscaperRuntime $escaper;

    public function __construct(Environment $env)
    {
        parent::__construct($env);

        $this->source = $this->getSourceContext();

        $this->escaper = $env->getRuntime('Twig\Runtime\EscaperRuntime');

        $this->parent = false;

        $this->blocks = [
        ];
    }

    protected function doDisplay(array $context, array $blocks = []): iterable
    {
        $macros = $this->macros;
        // line 1
        CoreExtension::getAttribute($this->env, $this->source, ($context["a"] ?? null), "b", [1], "method", false, false, false, 1);
        // line 2
        trigger_deprecation("acme/pkg", "1.2", sprintf("%s in \"%s\" at line 2.", "The \"tags\" template is deprecated.", "tags.twig"));
        // line 3
        yield "U";
        // line 4
        yield "no";
        // line 6
        flush();
        // line 7
        $_v0 = $context;
        $_v1 = ["x" => 1];
        if (!is_iterable($_v1)) {
            throw new RuntimeError('Variables passed to the "with" tag must be a mapping.', 7, $this->getSourceContext());
        }
        $_v1 = CoreExtension::toArray($_v1);
        $context = [];
        $context = $_v1 + $context;
        $context += $this->env->getGlobals();
        yield (string) $this->escaper->escape(($context["x"] ?? null), "html", null, true);
        $context = $_v0;
        // line 8
        $_v2 = $context;
        $context["y"] = 2;
        $context = $_v2;
        // line 9
        if ((($tmp = ($context["a"] ?? null)) && $tmp instanceof Markup ? (string) $tmp : $tmp)) {
            yield "A";
        } elseif ((($tmp = ($context["b"] ?? null)) && $tmp instanceof Markup ? (string) $tmp : $tmp)) {
            yield "B";
        } else {
            yield "C";
        }
        return; yield;
    }

    // [getTemplateName / isTraitable / getDefaultEscapeStrategy / getDebugInfo / getSourceContext omitted]
}
```

### A.12 strict_variables=true, autoescape=false

Templates:

`strict.twig`:
```twig
{{ a }}
{{ a.b }}
{{ a['b'] }}
{{ a.b|default('d') }}
{{ a ?? 'x' }}
{{ a.b.c is defined }}
{{ a ?: 'e' }}
```

Compiled `strict.twig` (options: {"strict_variables":true,"autoescape":false}):
```php
<?php
// [fixed `use` header omitted, see §2.1]
/* strict.twig */
class __TwigTemplate_bbcb2fc01e75d9653809a153e37cc0bb extends Template
{
    private Source $source;
    /**
     * @var array<string, MacroNamespace>
     */
    private array $macros = [];

    public function __construct(Environment $env)
    {
        parent::__construct($env);

        $this->source = $this->getSourceContext();

        $this->parent = false;

        $this->blocks = [
        ];
    }

    protected function doDisplay(array $context, array $blocks = []): iterable
    {
        $macros = $this->macros;
        // line 1
        yield (string) (isset($context["a"]) || array_key_exists("a", $context) ? $context["a"] : (function () { throw new RuntimeError('Variable "a" does not exist.', 1, $this->source); })());
        yield "
";
        // line 2
        yield (string) CoreExtension::getAttribute($this->env, $this->source, (isset($context["a"]) || array_key_exists("a", $context) ? $context["a"] : (function () { throw new RuntimeError('Variable "a" does not exist.', 2, $this->source); })()), "b", [], "any", false, false, false, 2);
        yield "
";
        // line 3
        yield (string) CoreExtension::getAttribute($this->env, $this->source, (isset($context["a"]) || array_key_exists("a", $context) ? $context["a"] : (function () { throw new RuntimeError('Variable "a" does not exist.', 3, $this->source); })()), "b", [], "array", false, false, false, 3);
        yield "
";
        // line 4
        yield (string) ((CoreExtension::getAttribute($this->env, $this->source, ($context["a"] ?? null), "b", [], "any", true, true, false, 4)) ? (Twig\Extension\CoreExtension::default(CoreExtension::getAttribute($this->env, $this->source, (isset($context["a"]) || array_key_exists("a", $context) ? $context["a"] : (function () { throw new RuntimeError('Variable "a" does not exist.', 4, $this->source); })()), "b", [], "any", false, false, false, 4), "d")) : ("d"));
        yield "
";
        // line 5
        yield (string) (((array_key_exists("a", $context) &&  !(null === $context["a"]))) ? ($context["a"]) : ("x"));
        yield "
";
        // line 6
        yield (string) CoreExtension::getAttribute($this->env, $this->source, CoreExtension::getAttribute($this->env, $this->source, ($context["a"] ?? null), "b", [], "any", false, true, false, 6), "c", [], "any", true, true, false, 6);
        yield "
";
        // line 7
        yield (string) (((($tmp = (isset($context["a"]) || array_key_exists("a", $context) ? $context["a"] : (function () { throw new RuntimeError('Variable "a" does not exist.', 7, $this->source); })())) && $tmp instanceof Markup ? (string) $tmp : $tmp)) ? ($context["a"]) : ("e"));
        return; yield;
    }

    // [getTemplateName / isTraitable / getDefaultEscapeStrategy / getDebugInfo / getSourceContext omitted]
}
```

Render `strict.twig` with context `[]` →
```text
EXCEPTION Twig\Error\RuntimeError: Variable "a" does not exist in "strict.twig" at line 1.
```

### A.13 SandboxExtension registered (policy allows include, upper, join, escape, range)

Templates:

`sb`:
```twig
{{ a.b }}{{ a['c'] }}{{ a|upper }}{{ a ~ 'x' }}{% include 'x' %}{{ range(1,2)|join }}
```

`x`:
```twig

```

Compiled `sb`:
```php
<?php
// [fixed `use` header omitted, see §2.1]
/* sb */
class __TwigTemplate_5cf63da0ddc2006ef641e1a37caefced extends Template
{
    private Source $source;
    /**
     * @var array<string, MacroNamespace>
     */
    private array $macros = [];
    private \Twig\Runtime\EscaperRuntime $escaper;

    public function __construct(Environment $env)
    {
        $this->sandbox = $env->getExtension(SandboxExtension::class)->getChecker();
        parent::__construct($env);

        $this->source = $this->getSourceContext();

        $this->escaper = $env->getRuntime('Twig\Runtime\EscaperRuntime');

        $this->parent = false;

        $this->blocks = [
        ];
    }

    protected function doDisplay(array $context, array $blocks = []): iterable
    {
        $macros = $this->macros;
        // line 1
        yield (string) $this->sandbox->ensureToStringAllowed($this->escaper->escape($this->sandbox->ensureToStringAllowed(CoreExtension::getAttribute($this->env, $this->source, ($context["a"] ?? null), "b", [], "any", false, false, true, 1), 1, $this->source), "html", null, true), 1, $this->source);
        yield (string) $this->sandbox->ensureToStringAllowed($this->escaper->escape($this->sandbox->ensureToStringAllowed((($_v0 = ($context["a"] ?? null)) && is_array($_v0) || $_v0 instanceof ArrayAccess && in_array($_v0::class, CoreExtension::ARRAY_LIKE_CLASSES, true) ? ($_v0["c"] ?? null) : CoreExtension::getAttribute($this->env, $this->source, ($context["a"] ?? null), "c", [], "array", false, false, true, 1)), 1, $this->source), "html", null, true), 1, $this->source);
        yield (string) $this->sandbox->ensureToStringAllowed($this->escaper->escape($this->sandbox->ensureToStringAllowed(Twig\Extension\CoreExtension::upper($this->env->getCharset(), $this->sandbox->ensureToStringAllowed(($context["a"] ?? null), 1, $this->source)), 1, $this->source), "html", null, true), 1, $this->source);
        yield (string) $this->sandbox->ensureToStringAllowed($this->escaper->escape(($this->sandbox->ensureToStringAllowed(($context["a"] ?? null), 1, $this->source) . "x"), "html", null, true), 1, $this->source);
        yield from $this->load("x", 1)->unwrap()->yield($context);
        yield (string) $this->sandbox->ensureToStringAllowed($this->escaper->escape($this->sandbox->ensureToStringAllowed(Twig\Extension\CoreExtension::join($this->sandbox->ensureToStringAllowed(range(1, 2), 1, $this->source)), 1, $this->source), "html", null, true), 1, $this->source);
        return; yield;
    }

    /**
     * @codeCoverageIgnore
     */
    public function getTemplateName(): string
    {
        return "sb";
    }

    /**
     * @codeCoverageIgnore
     */
    public function isTraitable(): bool
    {
        return false;
    }

    /**
     * @codeCoverageIgnore
     */
    public function getDefaultEscapeStrategy(): string|false
    {
        return "html";
    }

    /**
     * @codeCoverageIgnore
     */
    public function getDebugInfo(): array
    {
        return array (  48 => 1,);
    }

    public function getSourceContext(): Source
    {
        return new Source("", "sb", "");
    }
    
    public function ensureSecurityChecked(): void
    {
        if ($this->sandbox->isSandboxed($this->source)) {
            $this->checkSecurity();
        }
    }
    
    public function checkSecurity()
    {
        static $tags = ["include" => 1];
        static $filters = ["escape" => 1, "upper" => 1, "join" => 1];
        static $functions = ["range" => 1];
        static $tests = [];

        try {
            $this->sandbox->checkSecurity(
                [0 => "include"],
                [0 => "escape", 1 => "upper", 2 => "join"],
                [0 => "range"],
                [],
                $this->source
            );
        } catch (SecurityError $e) {
            if ($e instanceof SecurityNotAllowedTagError && isset($tags[$e->getTagName()])) {
                $e->setTemplateLine($tags[$e->getTagName()]);
            } elseif ($e instanceof SecurityNotAllowedFilterError && isset($filters[$e->getFilterName()])) {
                $e->setTemplateLine($filters[$e->getFilterName()]);
            } elseif ($e instanceof SecurityNotAllowedFunctionError && isset($functions[$e->getFunctionName()])) {
                $e->setTemplateLine($functions[$e->getFunctionName()]);
            } elseif ($e instanceof SecurityNotAllowedTestError && isset($tests[$e->getTestName()])) {
                $e->setTemplateLine($tests[$e->getTestName()]);
            }

            throw $e;
        }

    }
}
```

### A.14 Expression catalogue (each compiled as `{{ expr }}` alone; `autoescape => false`, non-strict)

The PHP shown is the argument of the `yield`/`yield (string)` statement (or the whole statement for streamed forms).

- `a + b` → `(($context["a"] ?? null) + ($context["b"] ?? null))`
- `a - b` → `(($context["a"] ?? null) - ($context["b"] ?? null))`
- `a * b` → `(($context["a"] ?? null) * ($context["b"] ?? null))`
- `a / b` → `(($context["a"] ?? null) / ($context["b"] ?? null))`
- `a // b` → `(int) floor((($context["a"] ?? null) / ($context["b"] ?? null)))`
- `a % b` → `(($context["a"] ?? null) % ($context["b"] ?? null))`
- `a ** b` → `(($context["a"] ?? null) ** ($context["b"] ?? null))`
- `a ~ b` → `yield (($context["a"] ?? null) . ($context["b"] ?? null));`
- `-a` → ` -($context["a"] ?? null)`
- `+a` → ` +($context["a"] ?? null)`
- `(-a)` → `(-($context["a"] ?? null))`
- `not a` → ` !(($tmp = ($context["a"] ?? null)) && $tmp instanceof Markup ? (string) $tmp : $tmp)`
- `a and b` → `((($tmp = ($context["a"] ?? null)) && $tmp instanceof Markup ? (string) $tmp : $tmp) && (($tmp = ($context["b"] ?? null)) && $tmp instanceof Markup ? (string) $tmp : $tmp))`
- `a or b` → `((($tmp = ($context["a"] ?? null)) && $tmp instanceof Markup ? (string) $tmp : $tmp) || (($tmp = ($context["b"] ?? null)) && $tmp instanceof Markup ? (string) $tmp : $tmp))`
- `a xor b` → `((($tmp = ($context["a"] ?? null)) && $tmp instanceof Markup ? (string) $tmp : $tmp) xor (($tmp = ($context["b"] ?? null)) && $tmp instanceof Markup ? (string) $tmp : $tmp))`
- `a b-and b` → `(($context["a"] ?? null) & ($context["b"] ?? null))`
- `a b-or b` → `(($context["a"] ?? null) | ($context["b"] ?? null))`
- `a b-xor b` → `(($context["a"] ?? null) ^ ($context["b"] ?? null))`
- `a == b` → `(($context["a"] ?? null) == ($context["b"] ?? null))`
- `a != b` → `(($context["a"] ?? null) != ($context["b"] ?? null))`
- `a < b` → `(($context["a"] ?? null) < ($context["b"] ?? null))`
- `a >= b` → `(($context["a"] ?? null) >= ($context["b"] ?? null))`
- `a <=> b` → `(($context["a"] ?? null) <=> ($context["b"] ?? null))`
- `a === b` → `(($context["a"] ?? null) === ($context["b"] ?? null))`
- `a !== b` → `(($context["a"] ?? null) !== ($context["b"] ?? null))`
- `a in b` → `CoreExtension::inFilter(($context["a"] ?? null), ($context["b"] ?? null))`
- `a not in b` → `!CoreExtension::inFilter(($context["a"] ?? null), ($context["b"] ?? null))`
- `a matches "/x/"` → `CoreExtension::matches("/x/", ($context["a"] ?? null))`
- `a starts with b` → `(is_string($_v0 = ($context["a"] ?? null)) && is_string($_v1 = ($context["b"] ?? null)) && str_starts_with($_v0, $_v1))`
- `a ends with b` → `(is_string($_v0 = ($context["a"] ?? null)) && is_string($_v1 = ($context["b"] ?? null)) && str_ends_with($_v0, $_v1))`
- `a has some (x => x)` → `CoreExtension::arraySome($this->env, ($context["a"] ?? null), function ($__x__) use ($context, $macros) { $context["x"] = $__x__; return ($context["x"] ?? null); }, $this->env->hasExtension(\Twig\Extension\SandboxExtension::class) && $this->env->getExtension(\Twig\Extension\SandboxExtension::class)->getChecker()->isSandboxed($this->source))`
- `a .. b` → `range(($context["a"] ?? null), ($context["b"] ?? null))`
- `a ? b : c` → `(((($tmp = ($context["a"] ?? null)) && $tmp instanceof Markup ? (string) $tmp : $tmp)) ? (($context["b"] ?? null)) : (($context["c"] ?? null)))`
- `a ? b` → `(((($tmp = ($context["a"] ?? null)) && $tmp instanceof Markup ? (string) $tmp : $tmp)) ? (($context["b"] ?? null)) : (""))`
- `a ?: b` → `(((($tmp = ($context["a"] ?? null)) && $tmp instanceof Markup ? (string) $tmp : $tmp)) ? ($context["a"]) : (($context["b"] ?? null)))`
- `a ?? b` → `(((array_key_exists("a", $context) &&  !(null === $context["a"]))) ? ($context["a"]) : (($context["b"] ?? null)))`
- `a.b ?? c` → `(((CoreExtension::getAttribute($this->env, $this->source, ($context["a"] ?? null), "b", [], "any", true, true, false, 1) &&  !(null === CoreExtension::getAttribute($this->env, $this->source, ($context["a"] ?? null), "b", [], "any", false, false, false, 1)))) ? (CoreExtension::getAttribute($this->env, $this->source, ($context["a"] ?? null), "b", [], "any", false, false, false, 1)) : (($context["c"] ?? null)))`
- `a ?? b ?? c` → `(((array_key_exists("a", $context) &&  !(null === $context["a"]))) ? ($context["a"]) : ((((array_key_exists("b", $context) &&  !(null === $context["b"]))) ? ($context["b"]) : (($context["c"] ?? null)))))`
- `a is defined` → `array_key_exists("a", $context)`
- `a.b is defined` → `CoreExtension::getAttribute($this->env, $this->source, ($context["a"] ?? null), "b", [], "any", true, true, false, 1)`
- `a.b.c is defined` → `CoreExtension::getAttribute($this->env, $this->source, CoreExtension::getAttribute($this->env, $this->source, ($context["a"] ?? null), "b", [], "any", false, true, false, 1), "c", [], "any", true, true, false, 1)`
- `a[b] is defined` → `CoreExtension::getAttribute($this->env, $this->source, ($context["a"] ?? null), ($context["b"] ?? null), [], "array", true, true, false, 1)`
- `a is not defined` → ` !array_key_exists("a", $context)`
- `a is null` → `(null === ($context["a"] ?? null))`
- `a is same as(b)` → `(($context["a"] ?? null) === ($context["b"] ?? null))`
- `a is even` → `(($context["a"] ?? null) % 2 == 0)`
- `a is odd` → `(($context["a"] ?? null) % 2 != 0)`
- `a is divisible by(3)` → `(0 == ($context["a"] ?? null) % 3)`
- `a is empty` → `Twig\Extension\CoreExtension::testEmpty(($context["a"] ?? null))`
- `a is iterable` → `is_iterable(($context["a"] ?? null))`
- `a is sequence` → `Twig\Extension\CoreExtension::testSequence(($context["a"] ?? null))`
- `a is mapping` → `Twig\Extension\CoreExtension::testMapping(($context["a"] ?? null))`
- `a is true` → `(($tmp = ($context["a"] ?? null)) && $tmp instanceof Markup ? (string) $tmp : $tmp)`
- `a is constant("X")` → `(($context["a"] ?? null) === constant("X"))`
- `a is constant("X", b)` → `(($context["a"] ?? null) === constant(get_class(($context["b"] ?? null))."::"."X"))`
- `constant("X") is defined` → `Twig\Extension\CoreExtension::constant("X", null, true)`
- `a|default` → `((array_key_exists("a", $context)) ? (Twig\Extension\CoreExtension::default(($context["a"] ?? null))) : (""))`
- `a|default("d")` → `((array_key_exists("a", $context)) ? (Twig\Extension\CoreExtension::default(($context["a"] ?? null), "d")) : ("d"))`
- `a.b|default("d")` → `((CoreExtension::getAttribute($this->env, $this->source, ($context["a"] ?? null), "b", [], "any", true, true, false, 1)) ? (Twig\Extension\CoreExtension::default(CoreExtension::getAttribute($this->env, $this->source, ($context["a"] ?? null), "b", [], "any", false, false, false, 1), "d")) : ("d"))`
- `(a ~ b)|default("d")` → `Twig\Extension\CoreExtension::default((($context["a"] ?? null) . ($context["b"] ?? null)), "d")`
- `a|map(x => x * 2)` → `Twig\Extension\CoreExtension::map($this->env, $this->env->hasExtension(\Twig\Extension\SandboxExtension::class) && $this->env->getExtension(\Twig\Extension\SandboxExtension::class)->getChecker()->isSandboxed($this->source), ($context["a"] ?? null), function ($__x__) use ($context, $macros) { $context["x"] = $__x__; return (($context["x"] ?? null) * 2); })`
- `a|reduce((c, x) => c + x, 0)` → `Twig\Extension\CoreExtension::reduce($this->env, $this->env->hasExtension(\Twig\Extension\SandboxExtension::class) && $this->env->getExtension(\Twig\Extension\SandboxExtension::class)->getChecker()->isSandboxed($this->source), ($context["a"] ?? null), function ($__c__, $__x__) use ($context, $macros) { $context["c"] = $__c__; $context["x"] = $__x__; return (($context["c"] ?? null) + ($context["x"] ?? null)); }, 0)`
- `[1, a, 3]` → `[1, ($context["a"] ?? null), 3]`
- `{a: 1, "b": 2, 3: c, (d): 4}` → `["a" => 1, "b" => 2, 3 => ($context["c"] ?? null),  (string)($context["d"] ?? null) => 4]`
- `{a}` → `["a" => ($context["a"] ?? null)]`
- `[...a, 1]` → `[ ...($context["a"] ?? null), 1]`
- `{...a, b: 1}` → `[ ...($context["a"] ?? null), "b" => 1]`
- `{0: "a", 1: "b"}` → `["a", "b"]`
- `{1: "a", 0: "b"}` → `[1 => "a", 0 => "b"]`
- `range(1, 3)` → `range(1, 3)`
- `max(a, b)` → `max(($context["a"] ?? null), ($context["b"] ?? null))`
- `cycle(a, 1)` → `Twig\Extension\CoreExtension::cycle(($context["a"] ?? null), 1)`
- `random()` → `Twig\Extension\CoreExtension::random($this->env->getCharset())`
- `date()` → `$this->extensions['Twig\Extension\CoreExtension']->convertDate()`
- `constant("A")` → `Twig\Extension\CoreExtension::constant("A")`
- `include("x")` → `Twig\Extension\CoreExtension::include($this->env, $context, "x")`
- `source("x")` → `Twig\Extension\CoreExtension::source($this->env, "x")`
- `a.b` → `CoreExtension::getAttribute($this->env, $this->source, ($context["a"] ?? null), "b", [], "any", false, false, false, 1)`
- `a.b()` → `CoreExtension::getAttribute($this->env, $this->source, ($context["a"] ?? null), "b", [], "method", false, false, false, 1)`
- `a.b(1, 2)` → `CoreExtension::getAttribute($this->env, $this->source, ($context["a"] ?? null), "b", [1, 2], "method", false, false, false, 1)`
- `a.b(x=1)` → `CoreExtension::getAttribute($this->env, $this->source, ($context["a"] ?? null), "b", ["x" => 1], "method", false, false, false, 1)`
- `a["b"]` → `(($_v0 = ($context["a"] ?? null)) && is_array($_v0) || $_v0 instanceof ArrayAccess ? ($_v0["b"] ?? null) : null)`
- `a[b]` → `(($_v0 = ($context["a"] ?? null)) && is_array($_v0) || $_v0 instanceof ArrayAccess ? ($_v0[(($_v1 = ($context["b"] ?? null)) instanceof \Stringable && (is_array($_v0) || $_v0 instanceof \ArrayObject || $_v0 instanceof \ArrayIterator) ? (string) $_v1 : $_v1)] ?? null) : null)`
- `a.0` → `CoreExtension::getAttribute($this->env, $this->source, ($context["a"] ?? null), 0, [], "any", false, false, false, 1)`
- `a?.b` → `((null === ($_v0 = ($context["a"] ?? null))) ? null : CoreExtension::getAttribute($this->env, $this->source, $_v0, "b", [], "any", false, false, false, 1))`
- `a?.b.c` → `((null === ($_v0 = ($context["a"] ?? null))) ? null : CoreExtension::getAttribute($this->env, $this->source, CoreExtension::getAttribute($this->env, $this->source, $_v0, "b", [], "any", false, false, false, 1), "c", [], "any", false, false, false, 1))`
- `a.b?.c` → `((null === ($_v0 = CoreExtension::getAttribute($this->env, $this->source, ($context["a"] ?? null), "b", [], "any", false, false, false, 1))) ? null : CoreExtension::getAttribute($this->env, $this->source, $_v0, "c", [], "any", false, false, false, 1))`
- `a.(b)` → `CoreExtension::getAttribute($this->env, $this->source, ($context["a"] ?? null), ($context["b"] ?? null), [], "any", false, false, false, 1)`
- `attribute(a, "b", [1])` → `CoreExtension::getAttribute($this->env, $this->source, ($context["a"] ?? null), "b", [1], "any", false, false, false, 1)`
- `a[1:2]` → `Twig\Extension\CoreExtension::slice($this->env->getCharset(), ($context["a"] ?? null), 1, 2)`
- `_self` → `$this->getTemplateName()`
- `_context` → `$context`
- `_charset` → `$this->env->getCharset()`
- `"a#{b}c"` → `yield (("a" . ($context["b"] ?? null)) . "c");`
- `1.5` → `1.5`
- `1e3` → `1000.0`
- `true` → `true`
- `none` → `null`
- `block("x")` → `yield from         $this->unwrap()->yieldBlock("x", $context, $blocks);`
- `block("x", "t.twig")` → `yield from         $this->load("t.twig", 1)->unwrap()->yieldBlock("x", $context);`
- `block("x") is defined` → `        $this->unwrap()->hasBlock("x", $context, $blocks)`
- `a = 1` → `($context["a"] = 1)`
- `[x, , y] = a` → `[(($_v0 = ($context["a"] ?? null)) instanceof \Traversable ? CoreExtension::destructureSequence($context, [0 => "x", 1 => null, 2 => "y"], $_v0) : ([$context["x"], , $context["y"]] = array_pad($_v0, 3, null))), $_v0 = null][0]`
- `{x, y: z} = a` → `[[$context["x"], $context["z"]] = [CoreExtension::getAttribute($this->env, $this->source, ($_v0 = ($context["a"] ?? null)), "x", [], \Twig\Template::ANY_CALL, false, false, false, 1), CoreExtension::getAttribute($this->env, $this->source, $_v0, "y", [], \Twig\Template::ANY_CALL, false, false, false, 1)], $_v0 = null][0]`
- `a|e` → `$this->escaper->escape(($context["a"] ?? null))`
- `a|e("js")` → `$this->escaper->escape(($context["a"] ?? null), "js")`
- `a|raw` → `($context["a"] ?? null)`
- `a|upper` → `Twig\Extension\CoreExtension::upper($this->env->getCharset(), ($context["a"] ?? null))`
- `a|nl2br` → `Twig\Extension\CoreExtension::nl2br($this->escaper->escape(($context["a"] ?? null), "html", null, true))`

---

## 13. Appendix B — porting notes, divergence traps, UNVERIFIED items

### 13.1 Model choices implied by the runtime (recommendations, with trade-offs)

1. **Interpret, don't generate.** Nothing observable depends on the PHP code itself except (a) line guessing (§3.12) and
   (b) which runtime path an expression takes (optimized `[]` vs `getAttribute`, strict checks, `always_defined`).
   Recommendation: keep the visitor passes (Correctness → Escaper → Sandbox → Optimizer) as AST transforms that set the
   same flags Twig sets (`always_defined`, `ignore_strict_check`, `optimizable`, `with_loop`, `output`, `strategy`,
   escape-filter insertion, `template_escaper`), and have the evaluator honour them. Risk: forgetting a flag silently
   changes edge-case output; mitigate by running `tests/Fixtures/**/*.test` (488 files) as the acceptance suite.
2. **Context = PHP array value.** Needs an insertion-ordered map with PHP key normalization (canonical decimal integer
   strings → int keys; bool/float/null key coercions), cheap copies (copy-on-write, e.g. `Rc<IndexMap>` +
   `make_mut`), and in-place mutation for the shared-scope constructs (capture bodies run in the *same* scope; arrow
   functions snapshot; blocks/includes/macros copy). Objects are shared handles.
3. **Template instances are stateful singletons per environment** (`$this->macros`, constant `$this->parent`, dynamic
   `$this->parents`). Exact parity for `renderBlock()`-before-`render()` needs a per-template mutable macro registry
   (interior mutability). Alternative (simpler, documented divergence): always resolve imports lazily.
4. **Output**: a sink trait (`write(&str)`); `render()` collects into a `String` and discards on error; `display`/`stream`
   forward. `{% flush %}` = sink flush.
5. **Errors**: carry `raw_message`, `line: i64` (−1 unknown, 0 "guess failed"), `column: Option<u32>`, `source name`,
   and format with §8.2. For guessed lines, precompute per node "the template line of the last debug marker emitted at
   or before it in compile order" and report the innermost executing node of the *handling* template (§3.12).
6. **PHP warnings** (array-to-string, undefined array key, non-numeric value, float-to-int precision loss, null array
   offset in PHP 8.5): PHP continues with a value; the Twig test-suite runs with warnings converted to exceptions
   (`phpunit.xml.dist`: `convertWarningsToExceptions="true"`, `convertNoticesToExceptions="true"`). A port needs a policy
   (e.g. a warning callback with an optional "strict PHP warnings" mode). No fixture expects a warning text (grep found
   none).

### 13.2 Divergence traps (all VERIFIED unless marked)

1. `for`: `_parent` is captured **before** the sequence is evaluated; value target assigned before key target;
   variables that are loop targets move to the end of the key order after the loop; only pre-existing keys survive.
2. `loop` exists only when used (optimizer) — visible through `_context`; loop targets are "always defined" even in
   `else` (warning + null instead of strict error).
3. Non-strict `x[k]` uses PHP array/ArrayAccess semantics directly (raw keys to `offsetExists/offsetGet`, strings never
   indexable); strict mode routes through `getAttribute` (bool/float keys → int).
4. `matches` yields int `0`/`1` (prints `0`); `starts with`/`ends with` are false for non-strings; `in` uses
   `CoreExtension::compare` with PHP-8 numeric-string rules and identity for objects.
5. `is true` returns the operand (prints it); `TrueTest` makes empty `Markup` falsy.
6. Double/triple evaluation: `a ?: b` evaluates `a` twice when truthy; `a.b ?? c` fetches up to 3 times (method called
   twice); `a.b|default(x)` fetches twice (defined test never calls methods).
7. Argument names: filters/functions/tests normalize (`lowercase`, underscores removed) and report unknown arguments with
   the **normalized** key; macros bind by exact, case-sensitive PHP parameter names.
8. Extra positional arguments: ignored by userland callables, errors for PHP internal functions (`abs`, `range`,
   `json_encode`, `max`, `min`, `is_iterable`) — the port must know which core callables are "internal" if it wants the
   exact PHP `ArgumentCountError` texts (e.g. `abs() expects exactly 1 argument, 2 given`).
9. `include … with` and `include()` use `array_merge` (integer keys renumbered); `with` tag uses `+` (no renumbering).
10. Errors thrown inside macro bodies are attributed to the caller; inside blocks to the block owner; a missing block
    requested via `block('x', 'tpl')` produces a message without a line (`… does not exist in "tpl".`).
11. `{% autoescape %}` around a macro definition has no effect; around a block definition it does; `pre_escape` filters
    always escape unsafe input.
12. `isTraitable()` ignores leaf nodes (plain text in a trait template is tolerated, `{{ x }}` is not).
13. FilesystemLoader normalizes logical names (`./a`, `sub/../a`, `a\b`, `/a`), which become the template name in
    messages and `_self`; `createTemplate` names need XXH3-128.
14. `{% extends false %}` → `Template "" is not defined.`; `{% include 5 %}` → name `"5"` (PHP scalar coercion).
15. `TemplateWrapper::renderBlock()` before a full render fails if the block uses body-level imports
    (`throwUninitializedMacroNamespace`), but succeeds after any full render of the same instance.
16. Messages containing PHP internals (TypeErrors, ArgumentCountErrors with file paths and generated class names) cannot be
    reproduced; choose stable approximations and document them.

### 13.3 UNVERIFIED items

- Exact line attribution for exceptions raised inside multi-line expressions (which PHP line a call opcode reports).
- `guess()` behaviour when the same template name appears several times on the stack (recursion), and with `yield from`
  chains through several generators (only the scenarios in §3.12 were executed).
- `cycle()` on mappings / non-list arrays (`$values[$position % $count]` on a string-keyed array → warning + null,
  inferred).
- `(int)` conversion of NaN/INF floats as attribute keys in `getAttribute` under PHP 8.5 (possible new warnings).
- Reserved names (`context`, `this`, …) used as named arguments of **method** calls (`obj.m(context=1)`): prefixed key →
  presumably PHP `Unknown named parameter` error.
- Behaviour when a template overwrites `loop` inside the loop body (`{% set loop = 'x' %}`) — the increments would operate
  on a string (PHP error text not checked).
- `ChainLoader` exception-list format with multiple failing loaders (only the empty-list form was executed).
- Sandbox details beyond the compiled shape (policy evaluation order for nested includes, `ensureSpreadAllowed`) were read,
  not executed.
- `Environment::isTemplateFresh()` / `auto_reload` timing behaviour with FilesystemCache (read only).
