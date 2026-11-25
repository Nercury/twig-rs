# Engine plan (roadmap phases 3–4)

How the visited AST becomes output. Type and module names are provisional until the parser (`src/ast`,
`src/extension`) and `twig-value` APIs land; the decisions below are not.

## Modules of the root crate

| Module | Contents |
|---|---|
| `runtime/` | `Environment`, template cache, template instances (with Twig's per-instance mutable state), `TemplateWrapper`, `Markup`, `getAttribute`, compile-time argument binding (`CallableArgumentsExtractor` port), include/source helpers, runtime error helpers |
| `lltl/` | instruction set, per-template program (units for body, blocks, macros, dynamic parent; constant pool; run-length line tables), codegen mirroring `Node::compile()` |
| `vm/` | interpreter: frames, operand stack, output sink with capture stack, block tables, macro calls, for-loop opcodes |
| `visitor/` | correctness (exists), escaper + safe analysis, sandbox, optimizer |
| `loader/` | `Loader` trait, `ArrayLoader`, `FilesystemLoader` (name normalisation, namespaces, security checks), `ChainLoader` |
| `extension/<name>/` | runtime callables of core, escaper, sandbox, debug, string loader, attribute extensions |

## Decisions

- **Program shape**: one compiled program per template (and per embedded template, indexed like Twig's
  `___N` classes): units = body (`doDisplay`), one per block, one per macro, one for a dynamic parent expression; plus
  traits, constant parent, sandbox usage lists (tags/filters/functions with lines for `checkSecurity`), flags
  (`isTraitable`, body imports).
- **Instructions are high level** (minijinja-sized): load/store name with Twig's five name modes, `GetAttr` mirroring
  `CoreExtension::getAttribute($env, $source, $object, $item, $arguments, $type, $isDefinedTest, $ignoreStrictCheck,
  $sandboxed, $lineno)`, optimised `[]` access, operators delegating to `twig-value`, call filter/function/test with
  pre-bound arguments, jumps, `ForBegin/ForNext/ForLoopUpdate/ForEnd` reproducing `ForNode`'s exact statements,
  `BeginCapture/EndCapture`, `YieldBlock/YieldParentBlock`, `Include`, `Embed`, `CallMacro`, `With`, sandbox checks,
  `Deprecated`, `Guard`, `Flush`. Every instruction that Twig gives an explicit `$lineno` carries it as an operand.
- **Line table** entries only where `Compiler::addDebugInfo` emits a marker, in Twig's compile order; errors without an
  explicit line take the last entry at or before the failing instruction of the frame whose template handles the
  exception (`Template::yield`, `yieldBlock` of the block owner, `getParent`, `ensureTraitsAllowed`).
- **Frames** hold the context as a `twig-value` array (copy-on-write). Copies happen exactly where PHP copies: block
  calls, includes/embeds/parents (`+= globals` in `yield`), macros (fresh context), `with` (save/restore); capture
  bodies share the frame; arrow functions snapshot context and macros at creation.
- **Output** is a byte sink trait with a capture stack; `render()` collects into a buffer and discards it on error,
  `display`/`stream` forward to the caller's sink. `{% flush %}` is a sink flush.
- **Callables** (filters, functions, tests) are `Arc<dyn Fn(&mut Call) -> Result<Value>>` (`Send + Sync`). `Call`
  gives access to the environment, charset, context (for `needs_context`), sandbox flag, diagnostics, current template
  name/line, and the arguments already bound at compile time by the `CallableArgumentsExtractor` port (named-argument
  normalisation, defaults from the declared signature, variadics, verbatim SyntaxErrors).
- **Environment** is `Send + Sync`: extension set frozen at first load (Twig's `LogicException`s for late
  registration), globals resolved like `getGlobals()`, template cache keyed by name, template instances shared with
  interior mutability for Twig's observable per-instance state (imported macros, resolved parents).
- **Errors**: one `Error` type (from `src/error.rs`) for syntax/loader/runtime with `updateRepr` formatting; non-Twig
  errors (PHP exceptions from `twig-value`, host callables) are wrapped as
  `An exception has been thrown during the rendering of a template ("%s").` with the previous error kept.
- **PHP diagnostics** from `twig-value` operations flow into the environment's handler (ignore / collect / error).
- **Recursion** is bounded by a configurable depth (documented deviation).

## Work split

1. **Core VM** (sequential, one agent): LLTL + codegen + VM + `Environment`/`ArrayLoader` + text/print/if/for/set/do/
   with/apply/capture + all expressions + `getAttribute` + argument binding + `Markup` + error integration.
2. **Templates** (after 1): inheritance (constant and dynamic parents), blocks, `parent()`, `block()`, traits, embed,
   include tag/function, macros (`TwigMacro`/`MacroNamespace` semantics, `_self`, imports scopes, varargs), exception
   handling and line guessing.
3. **Visitors** (parallel with 1, AST only): escaper + safe analysis, optimizer, sandbox visitor.
4. **Loaders** (parallel): filesystem and chain loaders with verbatim messages.
5. **Harness data** (parallel, needs `twig-value`): corpus typed trees → values, Rust versions of the PHP test classes
   and inline classes, the `IntegrationTest` environment (test extension, sandbox policy, debug, string loader,
   undefined-callable callbacks).
6. **Extension callables** (after 1, parallel by group): strings, arrays/iteration, numbers/formatting, escaping,
   dates (after timelib), misc functions/tests, sandbox runtime, `dump`, `template_from_string`.

Exit of phase 3: the ≈262 "pure data" fixtures pass through the ratchet harness.
