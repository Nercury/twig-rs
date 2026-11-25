# Engine plan (roadmap phases 3–4)

How the visited AST becomes output. Stages 1 (core VM) and 2 (template composition and the sandbox runtime) are
done; this file records the names and APIs they chose and what the next stages plug into. Upstream citations live in
the doc comments of the code.

## Status

| Stage | State |
|---|---|
| 1. Core VM: LLTL + codegen + VM + `Environment` + every statement + every expression + `getAttribute` + argument binding + `Markup` + errors/lines | done |
| 2. Templates: inheritance, blocks, `parent()`, `block()`, traits, embed, include, `include()`/`include_only()`/`source()`/`template_from_string()`, macros and imports, `TemplateWrapper` block API, exception handling and line guessing across templates; sandbox runtime (policy, checker, `SandboxExtension`, `Sandbox`, `render_sandboxed()`) | done (`tests/engine.rs`: 1567 oracle cases in 18 topics, `tests/engine_api.rs`) |
| 3. Visitors (escaper, optimizer, sandbox) | landed by parallel agents; the `Environment` registers the optimizer, `runtime::SandboxExtension` registers the sandbox visitor. `EscaperExtension` does not register `EscaperNodeVisitor` yet, so templates are not auto-escaped |
| 6. Extension callables by group | the runtime API and the group modules exist; the template functions are implemented by `runtime::functions` (registered after the groups) |

## Pipeline

```text
Environment::load(name)
  -> load_template_index: class key = loader cache key + "\0{debug}:{strict}:{use_yield}:{optimizations}"
  -> load_class(key, name, index): instance cache (key, `___N` appended for an embedded template)
       -> programs cache (one compiled file per class key): compile_source = ParseContext::parse_template
          (extension set: core, escaper, yield-not-ready, optimizer, + sandbox, PcreRegexValidator) -> lltl::compile
       -> init_runtime; construct (the compiled constructor): sandbox checker check, escaper runtime, traits
          (ensureTraitsAllowed, load + isTraitable + getBlocks + renamed blocks per trait)
  -> Arc<Template> (one per environment and class, shared between renders and threads)
  -> TemplateWrapper::{render, render_bytes, display, stream, render_block(_bytes), display_block, stream_block,
     has_block, block_names}
       -> VmState::new(env), Output::new(sink, flushes)
       -> runtime::composition::{yield_template, yield_block, has_block, block_names}
```

## LLTL (`src/lltl`)

- `Program` (one per template and per embedded template): `name`, `source`, `file` (`FileId`, shared by a template
  and its embedded templates), `constants`, `callables`, `units`, `body`, `blocks: [BlockDef {name, unit, line}]`,
  `macros: [MacroDef {name, unit, params: [MacroParam {name, default, explicit_default}], variadic, line}]`,
  `parent: Option<ParentDef::{Constant {name, line}, Dynamic {unit, line}}>` (the dynamic unit returns the loaded
  parent), `traits: [TraitDef {template, line, targets: [(block, alias)]}]`, `embedded_template(index)` (searched in
  the whole tree: nested embeds are numbered first, Twig numbers per file), `is_traitable()`,
  `default_escape_strategy()`, `security(): Option<SecurityLists {tags, filters, functions, tests}>` (the
  `CheckSecurityNode` of the sandbox visitor, names with first-use lines), `disassemble()`.
- `Unit {kind, code, lines: LineTable, locals}`; `UnitKind::{Body, Block(name), Macro(name), Parent, Arrow(params)}`.
- `LineTable`: run-length `(ip, line)` entries pushed only where `Compiler::addDebugInfo` writes a marker, in Twig's
  compile order. An instruction is attributed to the marker in effect where its PHP construct is attributed:
  - a function or static call: the line where the call starts (`emit_at(start)`);
  - a method call: the line of the method name, so `->unwrap()->yield(`/`->renderBlock(` after a template
    expression takes the marker in effect after it;
  - `yield from <generator>` (output-mode `block()`, include/embed/parent yields): the body runs on `YIELD_FROM`,
    whose line is where the statement ends; call-time checks (`CheckContext`) keep the call's line.
- Composition instructions (stack effects in `instr.rs`):
  - templates: `LoadTemplate {line, index}` (`$this->load()`), `SetParent`, `GetParent {this}`,
    `IncludeContext {merge}` (`CoreExtension::merge($context, vars)` / `toArray(vars)`), `CheckContext`
    (`array $context` of `yield()`), `YieldTemplate(YieldArgs::{Context, Empty, Given, Parent})`,
    `Block {op: BlockOp::{Yield, Render, Has}, template}`, `ParentBlock {name, render}`;
  - macros: `MacroNamespace {name, line}` (`$macros[name] ?? throwUninitializedMacroNamespace(line)`),
    `SelfMacroNamespace`, `TemplateMacroNamespace`, `SetMacroNamespace {name, global}`, `MacroCall {line}`,
    `MacroDefined`; a `{% from %}` macro variable (no name, `_vN` in Twig) gets the key `"\0_v<id>"`;
  - regions: `TryLoader {catch}` / `EndTry` (`ignore missing`), `SandboxStart {local}` / `SandboxEnd {local}`;
  - `CheckToString {spread, line}`.
- `render_sandboxed()` compiles like `RenderSandboxedFunction` (the literal `output_strategy` is checked and dropped).

## VM (`src/vm`)

- `VmState {env, frames, max_depth}` (template stack, depth limit, diagnostics sink, `host()` re-entry).
- `Frame {context, macros, blocks: Option<Blocks>, template, stack, locals, iterators, args, saved_macros,
  handlers}`: `Frame::new(context, template, macros, blocks)` for compiled methods, `Frame::arrow(...)` without
  `$blocks` (PHP warns `Undefined variable $blocks` and the template method rejects `null`).
- `Handler::{Loader {catch, ...}, Sandbox {already}}`: `execute()` unwinds the regions of the unit on an exception
  (a `finally` runs, a `catch (LoaderError)` restores the frame, pushes `null` and continues).
- `step()` dispatches to one function per instruction group, so each frame stays small: template recursion (blocks,
  includes, macros) costs about 6 KiB of native stack per level in debug builds, 3 KiB in release.
- `handle_exception(state, exception, handler_program)` and `guess()` port `Template::handleException()` /
  `Error::guess()`: the handler's file first, then the outer frames' files; traces innermost first.

## Runtime (`src/runtime`)

- `Template`: per-instance state of a compiled class: `program`, `family` (root program of the file), `key` (class
  key), `env_id`, `$this->macros` (`set_macro`), `$this->parent` (unresolved / none / resolved), `$this->traits`,
  `$this->traitAliases`, `$this->blocks` (own units and trait `BlockRef`s; `block_table()` builds the `Blocks` with
  `Arc<Self>`). `Blocks`: `$blocks` arrays (`merge` = `array_merge`); `BlockRef {template, unit}`.
  `TemplateObject`: the handle values on the operand stack.
- `composition`: `yield_template` (`Template::yield()`), `get_parent`, `yield_block` (with `$templateContext`),
  `yield_parent_block`, `render_block`, `render_parent_block`, `has_block`, `block_names`, `load`
  (`Template::load()`: `string|TemplateWrapper|array` coercion, own class family, error source/line rules),
  `env_load`, `resolve_template`, `merge_include_variables`. Exception handlers sit exactly where Twig's are
  (`yield()`, `yieldBlock()` (block owner), `getParent()`); macros, arrows and captures have none.
- `macros`: `MacroNamespace` objects (weak references to their template, so imports never form cycles), `call`/`has`
  (`MacroNamespace`), `TwigMacro::callLegacy()` checks and deprecations, PHP's binding of spread arguments to the
  macro closure (positional, named, reserved-name renaming, bucket).
- `sandbox`: `SecurityPolicyInterface` (trait for custom policies), `SecurityPolicy` (allow-lists, strict mode),
  `SourcePolicyInterface` (deprecated), `SecurityChecker`, `SecurityError`/`SecurityErrorKind`,
  `SandboxExtension` (Twig's class name: registers the `sandbox` tag and the sandbox visitor, holds the checker),
  `Sandbox` (Twig 3.29 facade), `SandboxBridgeRuntime`, `ensure_security_checked` (compiled `checkSecurity()`),
  `ensure_traits_allowed`. A violation is a `RuntimeError`-kind `Error` whose `previous()` is the `SecurityError`
  (`SecurityError::of(&error)` gives the class) until `ErrorKind` has the sandbox classes.
- `functions` (registered after the group modules in `extension/runtime/mod.rs`): `include`, `include_only`,
  `source`, `template_from_string`, `render_sandboxed`.
- `Environment` additions: `resolve_template`, `sandbox_checker`, `is_sandboxed`, `add_runtime`/`runtime` (runtime
  objects by class, `Unable to load the "%s" runtime.`), `load_family`, `load_template_index`.
- `TemplateWrapper` as a value: `to_value()` / `from_value()` (`Twig\TemplateWrapper` object with `render`,
  `renderBlock`, `hasBlock`, `getBlockNames`, `getTemplateName`, `getDefaultEscapeStrategy`; ownership checked
  against the running environment).

## Callable runtimes (`src/extension/runtime`)

- As in stage 1: `CallableRuntime` in the runtime slot of a `PhpCallable`, `Call<'_>`, `install_runtimes`, the group
  modules (`strings`, `arrays`, `misc`); `crate::runtime::functions::register` runs last and replaces the misc
  group's placeholders for the template functions.

## Next stage hooks

1. `ErrorKind` sandbox variants: switch `SecurityError::into_error()` to them (one place) and drop the `previous`
   detail; `tests/engine/mod.rs::error_class` keeps working.
2. `extension::SandboxExtension` (the tag-only marker in `extension/optional.rs`) should become a re-export of
   `runtime::SandboxExtension`.
3. Auto-escaping: once `EscaperExtension` returns `EscaperNodeVisitor`, the sandbox topics can run with the default
   `autoescape` (the inserted `escape` filters are then checked by the policy, as in Twig).
4. A filesystem `Loader` with paths: `TemplateWrapper::getSourceContext()` from templates and paths in errors.

## Deviations introduced by stages 1 and 2

- A configurable call-depth limit (`max_depth`, default 200) with a twig-rs message instead of PHP's stack overflow;
  template constructors (traits loading traits) are limited to 64 nested levels (cycles of traits).
- `render()` returns `String`: non-UTF-8 output is an error (`render_bytes()`/`display()` return the bytes).
- A sink write failure is a `RuntimeError` (`Unable to write the template output: ...`); PHP's `echo` cannot fail.
- `Markup::count()` and the `upper`/`lower` runtimes support UTF-8 (and single-byte charsets for `count`) only.
- String callables (`'strtoupper'` as a callable value) cannot be resolved: `Call to undefined function X()`.
- Arrow-function `ArgumentCountError` messages name `{closure}` without PHP's class/file suffix.
- Userland `TypeError`s (`Template::load()`, `Template::yield()`, `renderBlock()` without `$blocks`, the typed
  parameters of the template functions) omit PHP's `, called in <compiled file> on line N` suffix.
- Line guessing keeps templates apart like Twig with a filesystem cache: with `cache => false` PHP eval()s every
  template from the same line of `Environment.php`, so `Error::guess()` also matches frames of other templates (an
  exception in the body of a macro imported from another template takes the handling template's marker at or before
  the macro's PHP line). The composition vectors run the oracle with a filesystem cache per case.
- Sandbox violations are `RuntimeError`-kind errors carrying a `SecurityError` (class and names) as `previous()`.
- `{% sandbox %}` and `include(..., sandboxed = true)` switch the sandbox on for the current thread (Twig's checker
  flag is per environment; renders share it in PHP), `SandboxExtension::enable_sandbox()` for every thread.
- `Twig\TemplateWrapper` objects in templates support `render`, `renderBlock`, `hasBlock`, `getBlockNames`,
  `getTemplateName` and `getDefaultEscapeStrategy`; `display`, `displayBlock`, `stream`, `streamBlock`,
  `getSourceContext`, `unwrap` and `isOwnedBy` throw a `LogicException` (they echo or return internal objects).
- Runtime-class callables (`render_sandboxed`) get their runtime when called, after the arguments are evaluated
  (Twig fetches it first).
- No auto-escaping by default yet (see Status); the escape runtime has no custom escapers or safe classes.

## Tests

- `tests/engine.rs` + `tests/engine/` run the oracle batches of `tests/vectors/engine/` (generator:
  `tests/vectors/engine/engine.gen.py`, which reproduces every committed batch byte for byte): stage 1's topics and
  `inheritance` (140), `traits` (58), `embed` (57), `include` (119), `macros` (119), `composition_errors` (60),
  `sandbox` (131), `api` (14 scripts of API calls: `TemplateWrapper` block methods, `resolveTemplate()`, `Sandbox`,
  `render_sandboxed()`). Composition topics run each case in its own PHP process with a filesystem cache, load only
  the main template first (`load_all: false`), and set strict policies and allowed tests after loading, as their
  `data_php` does.
- `tests/engine_api.rs`: environment lifecycle, cache, string templates, sinks, diagnostics policies, host callables,
  depth limits, threads, the block API, wrappers as values, template recursion limits and native stack use, sandbox
  toggles across errors and threads, custom and source policies.
