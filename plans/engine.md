# Engine plan (roadmap phases 3–4)

How the visited AST becomes output. The engine is complete: the core VM, template composition, the sandbox runtime,
the node visitors and every bundled callable are in place and integrated (all 500 runnable fixture cases pass);
this file records the names and APIs the stages chose and the work that remains. Upstream citations live in the doc
comments of the code; deviations are listed in `reference/deviations.md`.

## Status

| Stage | State |
|---|---|
| 1. Core VM: LLTL + codegen + VM + `Environment` + every statement + every expression + `getAttribute` + argument binding + `Markup` + errors/lines | done |
| 2. Templates: inheritance, blocks, `parent()`, `block()`, traits, embed, include, `include()`/`include_only()`/`source()`/`template_from_string()`, macros and imports, `TemplateWrapper` block API, exception handling and line guessing across templates; sandbox runtime (policy, checker, `SandboxExtension`, `Sandbox`, `render_sandboxed()`) | done (`tests/engine.rs`: 1567 oracle cases in 18 topics, `tests/engine_api.rs`) |
| 3. Visitors: `EscaperNodeVisitor` (registered by `EscaperExtension`), `OptimizerNodeVisitor` (`OptimizerExtension`, registered by the `Environment`), `SandboxNodeVisitor` (`SandboxExtension`) | done (`tests/visitors.rs`: 551 + 1752 oracle cases) |
| 4. Loaders: `ArrayLoader`, `FilesystemLoader`, `ChainLoader` | done (`tests/loader_upstream.rs`, `tests/loader_vectors.rs`) |
| 5. Escaping strategies (`src/escaper`) and the `EscaperRuntime` of the environment | done (`tests/escaper.rs`, string-group vectors) |
| 6. Extension callables by group (strings, arrays, misc, template functions) | done (`tests/callables_*.rs`: 2413 oracle cases) |
| 7. Fixture suite through the `Environment` | 500 of 500 runnable cases (2 skipped by their CONDITION); the ratchet `tests/twig/known-failures.txt` is empty |

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
- `runtimes()`: the `NodeRuntime` closures the code of node classes of extensions calls (`CallRuntime {runtime,
  argc}`: `[args...] -> [result]`, attributed to the line where the call starts).
- `LineTable`: run-length `(ip, line)` entries pushed only where `Compiler::addDebugInfo` writes a marker, in Twig's
  compile order. An instruction is attributed to the marker in effect where its PHP construct is attributed:
  - a function or static call: the line where the call starts (`emit_at(start)`);
  - a method call: the line of the method name, so `->unwrap()->yield(`/`->renderBlock(` after a template
    expression takes the marker in effect after it;
  - `yield from <generator>` (output-mode `block()`, include/embed/parent yields): the body runs on `YIELD_FROM`,
    whose line is where the statement ends; call-time checks (`CheckContext`) keep the call's line;
  - any other instruction (`emit()`): PHP's `CG(zend_lineno)` after the operands, the marker in effect at the last
    compiled token (`Codegen::php_marker`): `leaf()` after a token Twig's compiler writes, `folded()` after one PHP
    folds into a constant (`null`, `true`, a constant operation), which inside an array literal takes the literal's
    line;
  - the constants `zend_try_ct_eval_array()` folds in an outermost array literal: the literal's line, the line of
    the PHP node of its first value (`php::first_line()`); a value PHP's parser makes (a literal, a concatenation of
    literals), also as the chosen branch of a folded conditional (`? :`, `?:`, `??`), keeps the line it is written on
    (P35, `PhpModel::keeps_parsed_line()`). When the first value comes after a dynamic key (`(string) key => value`),
    the literal's line is known only once the key is compiled: the code in between carries a placeholder line
    (`DeferredLine`), which `Codegen::key_compiled()` replaces in the line table (`LineTable::replace_from()`);
  - the statements Twig writes after an expression without a marker of their own (the bookkeeping of `for` loops and
    the end of `with`, P36; a sequence destructuring assignment, P40): the marker in effect (`leaf()`), not the line
    a folded literal or code PHP skips left behind;
  - the conversion to bool of `TrueTest` (the operands of `and`, `or`, `xor` and `not`, the tests of `if`, `elseif`
    and `? :`, the left operand of `?:`): the marker in effect where `(($tmp = ` is written, before the operand's
    own markers (P37).
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
- Compiled sources (`Template::source()`, `ModuleNode::compileGetSourceContext()`): what a template reports at
  runtime (its `getSourceContext()`, the source of its runtime errors and of the source policy check). With `debug` it
  is the loader's source. Without, `compile_source()` gives the program and its embedded programs a source without
  code, with the name and, for a template with a path, a path resolved when it is first needed (P24;
  `Source::with_lazy_path()`, Twig's `createWithLazyPath()`): `TemplatePath` ports `Template::getSourcePath()`, asking
  the loader of the program's first instance (taken in `Environment::construct()`, Twig's `$this->loader`) or, while
  the program has none, the loader it was compiled with, and giving `''` when that loader throws. The path is kept
  per program, where Twig keeps it per instance (`reference/deviations.md`, "Lazily resolved template paths").
- `composition`: `yield_template` (`Template::yield()`), `get_parent`, `yield_block` (with `$templateContext`),
  `yield_parent_block`, `render_block`, `render_parent_block`, `has_block`, `block_names`, `load`
  (`Template::load()`: `string|TemplateWrapper|array` coercion, own class family, error source/line rules),
  `env_load`, `resolve_template`, `merge_include_variables`. Exception handlers sit exactly where Twig's are
  (`yield()`, `yieldBlock()` (block owner), `getParent()`); macros, arrows and captures have none.
- `macros`: `MacroNamespace` objects (weak references to their template, so imports never form cycles), `call`/`has`
  (`MacroNamespace`), `TwigMacro::callLegacy()` checks and deprecations, PHP's binding of spread arguments to the
  macro closure (positional, named, reserved-name renaming, bucket).
- `attribute`: `get_attribute()` (`CoreExtension::getAttribute()`). A method call, `$object->$method(...$arguments)`
  in Twig, sends arguments with a named key the way `ZEND_SEND_UNPACK` does (P33): `ObjectRef::method_params()` finds
  the parameters of the method (those of the `__call()` trampoline for a missing or non-public method of a class
  with `__call()`), and `MethodParams::unpack()` checks each name as it is sent, so an unknown name fails at its
  element. Positional arguments, and objects that report no parameters, go through `CallArgs::from_spread()`.
  Classes report parameters with `Object::method_params()`: in twig-value the objects of `ObjectBuilder`,
  `ArrayObject`, `ArrayIterator` and enum cases; in twig-date the date classes, from the per-class tables of
  `args::method_params()` that their methods also bind with (P34); here `Markup`, `Twig\Source` and
  `Twig\TemplateWrapper`.
- `sandbox`: `SecurityPolicyInterface` (trait for custom policies), `SecurityPolicy` (allow-lists, strict mode),
  `SourcePolicyInterface` (deprecated), `SecurityChecker`, `SecurityError`/`SecurityErrorKind`,
  `SandboxExtension` (Twig's class name: registers the `sandbox` tag and the sandbox visitor, holds the checker),
  `Sandbox` (Twig 3.29 facade), `SandboxBridgeRuntime`, `ensure_security_checked` (compiled `checkSecurity()`),
  `ensure_traits_allowed`. A violation is an `Error` of its own class (`ErrorKind::SecurityNotAllowedFilter`, ...);
  `SecurityError::of(&error)` gives the violation and its names back.
- `functions` (registered after the group modules in `extension/runtime/mod.rs`): `include`, `include_only`,
  `source`, `render_sandboxed`.
- `Environment` additions: `resolve_template`, `sandbox_checker`, `is_sandboxed`, `add_runtime`/`runtime` (runtime
  objects by class, Twig's `getRuntime()`: the object registered for the class, the escaper runtime for
  `Twig\Runtime\EscaperRuntime`, else the registered extension of that class name, kept as the runtime of the class
  from then on (P26; the value holds the `Arc<dyn Extension>`), else `Unable to load the "%s" runtime.`),
  `escaper_runtime`, `core_settings`, `load_family`, `load_template_index`.
- `TemplateWrapper` as a value: `to_value()` / `from_value()` (`Twig\TemplateWrapper` object with `render`,
  `renderBlock`, `hasBlock`, `getBlockNames`, `getTemplateName`, `getDefaultEscapeStrategy`, `getSourceContext` (a
  `Twig\Source` object: code only when compiled with `debug`); ownership checked against the running
  environment).
- `Twig\Source` objects (`SourceObject`, P38): the three typed private properties of TWIG/src/Source.php in a
  `PropertyStore`, so dumps and casts see them as PHP does, and `__construct()`, `createWithLazyPath()`, `getCode()`,
  `getName()`, `getPath()`, `getColumn()` and `__serialize()`, bound and received like userland methods. The lazily
  resolved path of a template compiled without `debug` is a `SourcePathClosure`, the first-class callable
  `$this->getSourcePath(...)`, so the `Source` objects of one template instance compare equal
  (`zend_closure_compare()`) until `getPath()` (also called by `__serialize()`) replaces the closure with the path.
- `typed_property`: `verify_property_type()`, the port of `zend_verify_property_type()` in coercive typing mode, for
  the typed properties of the Twig classes implemented as host objects, which the value model leaves to each class
  (P39): `Markup::__construct()` assigns `?string $charset` through it (scalars are coerced, an array is a
  `TypeError`), and `Source::getPath()` the resolved `string|\Closure $path`.

## Callable runtimes (`src/extension/runtime`)

- `CallableRuntime` in the runtime slot of a `PhpCallable`, `Call<'_>` (arguments, injected values,
  `has_unpacked_arguments()` for calls with `...`), `install_runtimes`, the group modules (`strings`, `arrays`,
  `misc`) and `crate::runtime::functions` (`include`, `include_only`, `source`, `render_sandboxed`); each PHP callable
  has exactly one runtime entry.
- Shared state: the strings group's mbstring encodings (`strings::mb`, also used by the arrays group), the per-thread
  MT19937 of `random` and `shuffle` (`arrays::random`, seeded by the public `mt_srand()`).
- `callables` (P15): PHP callables as Twig's methods use them, in the class scope of `CoreExtension`:
  `check_callable()` checks a `callable` parameter (`zend_is_callable_ex()`: `uasort()` for `sort`, `array_filter()`
  and `CallbackFilterIterator` for `filter`), and `invoke()` calls a value (`ZEND_INIT_DYNAMIC_CALL`: `map`, `reduce`,
  `find`, the `has some` and `has every` operators, `Call::invoke()`). Closures, invokable objects,
  `[$object, 'method']`, `'Class::method'` and `['Class', 'method']` resolve like PHP (class lookup, `self`, `parent`
  and `static`, visibility, deprecations); a function name is never found, and a static method PHP would run fails
  with a `LogicException` when it is called (`reference/deviations.md`, "String callables").
- `misc/twig_classes.rs` (P27, generated by `twig_classes.gen.php` from the oracle's reflection): every class,
  interface, trait and enum of Twig 3.31.1 and the PHP classes they extend that twig-value lacks (`Exception`,
  `Throwable`, `IteratorIterator`, `OuterIterator`), with their constants, enum cases and methods.
  `misc::constants::builtin_classes()` builds a class table from it (with `RoundingMode`); `find_class()` looks a name
  up in the environment's registry, then in that table, for `constant()`, the `constant` test, `enum()`,
  `enum_cases()` and the class lookups of callables.
- `EscaperRuntime` (custom escapers, safe classes) is the environment's `escaper_runtime()` (Twig's
  `getRuntime(EscaperRuntime::class)`, charset fixed at first use); `CoreSettings` (date formats, timezone, number
  format) is held by `CoreExtension` and reached with `Environment::core_settings()`.

## Integration hooks (done)

- Error model: `ErrorKind` has the sandbox classes (`Security`, `SecurityNotAllowed{Tag,Filter,Function,Test,Method,
  Property}`, built by `SecurityError::into_error()`; `SecurityError::of()` reads the names back) and
  `ErrorKind::Php` for PHP throwables that are not Twig errors (`Error::php(PhpError)`, `Error::class_name()`):
  `vm::Exception::from(Error)` turns them back into PHP throwables, so templates wrap them (`An exception has been
  thrown during the rendering of a template ("...")`) and `compile_source()` wraps them while compiling. The
  `FilesystemLoader` reports PHP warnings as `ErrorException`s and `realpath()`'s `ValueError` this way.
- Visitors: `EscaperExtension::node_visitors()` and `SandboxExtension::node_visitors()` return their visitor;
  `NodeVisitor::class_name()` names them; `ExtensionSet::retain_node_visitors()` drops visitors after initialization
  (the parser tests keep the parser's own, like the oracle's `keep_parser_visitors()`).
- One `SandboxExtension` type (`runtime::sandbox`, re-exported by `twig::extension`, `Default` = a policy allowing
  nothing, not sandboxed globally).
- Codegen: `CompileOptions { debug, enums }`: `EnumFunction`/`EnumCasesFunction` check literal names at compile time
  (`enum_exists()` through the environment's classes) and inline the case(s) as constants; `Program::is_debug()`
  gives the code of `TemplateWrapper::getSourceContext()` (a `Twig\Source` object in templates).
- The `constant` test uses PHP's `constant()` over the shipped PHP constant table (`misc::constants`).
- Sandbox coercion: `CallableParameter::string_coercion_safe()` for host callables,
  `CustomExpr`/`CustomNode::string_coerced_child_names()` for node classes of extensions.
- Node classes of extensions (`ast::CustomNode`, `ast::CustomExpr`, made with `Node::custom`/`Expr::custom`):
  `compile()` writes the node's code into the unit being compiled through `lltl::Compiler` (Twig's `Compiler`):
  `add_debug_info`, `subcompile`/`subcompile_expr`, constants, locals (`getVarName()`), `$context` reads and writes,
  output and captures, PHP operators, forward `Jump`s and backward `Mark`s, `call` (`compileCallable()`, bound at
  compile time) and `call_runtime` (a `NodeRuntime` closure getting a `NodeCall`: the arguments, `$context`, the
  output, the environment, `invoke`). The builder verifies the stack discipline as the code is written (values
  taken only from what the node pushed in its region, statements stack-neutral, expressions one value, jumps landing
  at the height they left, no jump into or out of a capture or an argument list, handles of one builder only) and
  reports violations as `LogicException`s of the compilation. Without `compile()`, Twig's `Node::compile()`: statement
  children in order, the only child of an expression. The codegen buffers its deprecations (reported in order when
  it ends) so that the builder can borrow it. Classification: `callable()` makes a class a `FilterExpression`,
  `FunctionExpression` or `TestExpression` subclass for the safe analysis, the escaper's `pre_escape`, the sandbox
  (collection, `wrapNode()`, `CallableParameters`, the inherited `getStringCoercedChildNames()`) and the optimizer's
  `include()`; `operand_names_to_escape()` an `OperatorEscapeInterface` (`escapeConditional()`, the intersected
  safety, the sandbox's operand search). `safe_analysis::check_analysis()` raises the `LogicException`s Twig's
  analysis traversal throws for such classes (more than two operands, missing children) when the escaper pass has
  met one. `php::generated_parse_error()` lets the direct children of such classes be spreads (they write no PHP).

## Remaining work

1. Non-UTF-8 charsets beyond the single-byte ones in the mbstring functions (`upper`, `length`, `slice`, ...: tables
   for multi-byte and table-based encodings). iconv covers every charset only with the `glibc-iconv` feature.
2. Compiled-template caching across processes (optional, roadmap phase 6).
3. Public API review, `NOTICE` file and publishing (roadmap phase 6).

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
  toggles across errors and threads, custom and source policies, and the integration hooks (oracle-verified):
  PHP throwables of loaders wrapped by templates, sandbox error kinds, the escaper runtime, the `CoreExtension`
  settings, `getSourceContext()`, `mt_srand()`, compile-time enum checks, runtimes falling back to extensions (P26),
  lazily resolved source paths and the `Twig\Source` methods (P24, P38).
- `tests/engine_parity.rs`, `tests/runtime_parity.rs` and `tests/callables_parity.rs` replay the oracle vectors of
  phase 5's parity fixes with twig-fuzz's runner and diff (`tests/vectors/engine/parity/`, inline, and
  `tests/vectors/callables/parity/`): constant folding and the line rules above (P31, P35–P37), runtime fixes,
  callables and the class table (P15, P27). The fuzz regression vectors (`tests/fuzz_regressions.rs`,
  `tests/vectors/fuzz/`) cover the rest, the method parameters of P33 and P34 among them.
- `tests/fixtures.rs`: Twig's integration fixtures through the `Environment` (ratchet `tests/twig/known-failures.txt`).
- `tests/custom_nodes.rs`: node classes of extensions (tags compiling themselves, expression classes of callables and
  operators) against the oracle vectors `tests/vectors/custom_nodes.json`, whose generator
  `tests/vectors/custom_nodes.gen.php` holds the PHP counterparts of the Rust classes; the builder's checks; runtimes.
