//! Ports of Twig 3.31.1's behaviour-level PHPUnit tests about node visitors, token parsers, the expression parser
//! (the parts that compile or render), node classes, the default escaping strategy of templates and the dedicated
//! sandbox (TWIG/tests/, classified in `reference/twig/test-suite.md` §6). Upstream test names are kept (snake case),
//! one module per upstream class:
//!
//! | upstream file | port |
//! |---|---|
//! | `NodeVisitor/EscaperTest.php` | `upstream/visitors/escaper_test.rs` |
//! | `NodeVisitor/SandboxTest.php` | `upstream/visitors/sandbox_test.rs` |
//! | `TokenParser/BlockTokenParserTest.php` (rendering parts) | `upstream/visitors/block_token_parser_test.rs` |
//! | `TokenParser/GuardTokenParserTest.php` (through the environment) | `upstream/visitors/guard_token_parser_test.rs` |
//! | `ExpressionParserTest.php` (tags, compilation, rendering) | `upstream/visitors/expression_parser_test.rs` |
//! | `TemplateEscapeStrategyTest.php` | `upstream/visitors/template_escape_strategy_test.rs` |
//! | `Node/TextTest.php` | `upstream/visitors/node_text_test.rs` |
//! | `Node/Expression/CallTest.php` | `upstream/visitors/node_expression_call_test.rs` |
//! | `Node/Expression/FilterTest.php` | `upstream/visitors/node_expression_filter_test.rs` |
//! | `Node/Expression/MacroReferenceTest.php` | `upstream/visitors/node_expression_macro_reference_test.rs` |
//! | `Extension/SandboxBridgeExtensionTest.php` | `upstream/visitors/extension_sandbox_bridge_extension_test.rs` |
//! | `Extension/SandboxDeprecationsTest.php` | `upstream/visitors/extension_sandbox_deprecations_test.rs` |
//! | `Extension/SandboxStateChangeTest.php` | `upstream/visitors/extension_sandbox_state_change_test.rs` |
//! | `Sandbox/SandboxTest.php` | `upstream/visitors/sandbox_sandbox_test.rs` |
//!
//! Expected values come from the upstream tests; where PHPUnit only checks an exception class, a message substring
//! or the equality of two renders, the exact result Twig produces is compared too, from the oracle vectors
//! `tests/vectors/upstream/visitors/<topic>.json`. Regenerate them (from the repository root; Git Bash on Windows
//! needs MSYS_NO_PATHCONV=1):
//!
//!     python -I tests/vectors/upstream/visitors/upstream_visitors.gen.py --oracle
//!
//! Tests marked `#[ignore = "twig-rs mismatch: ..."]` reproduce differences from Twig found around the ported tests
//! (`cargo test --test upstream_visitors -- --ignored` runs them).
//!
//! Ported elsewhere: `NodeTraverserTest` and `NodeVisitor/CorrectnessTest` (tests/parser/traverser.rs),
//! `NodeVisitor/OptimizerTest` (tests/visitors.rs), the parse-level parts of `TokenParser/*TokenParserTest` and of
//! `ExpressionParserTest` (tests/parser/token_parser_test.rs, tests/parser/expression_parser_test.rs),
//! `FileExtensionEscapingStrategyTest` (tests/escaper.rs).
//!
//! Not ported (no counterpart in twig-rs; each module lists the methods it leaves out):
//!
//! - `NodeVisitor/SafeAnalysisTest::testCompilingDoesNotRetainTheAst`: PHP garbage collection of node objects
//!   (`WeakReference`s after `gc_collect_cycles()`); Rust drops the tree when the compilation returns.
//! - `NodeVisitor/SandboxTest::testGeneratorExpression`: the `is_generator` attribute (no Twig node sets it, only
//!   hand-built trees) has no counterpart on `twig::ast::Expr`, and the assertion is on generated PHP.
//! - `NodeVisitor/EscaperTest::testTemplatesFetchTheEscaperOnlyWhenTheyEscape` compares generated PHP: ported on
//!   `Program::uses_template_escaper()`, the attribute the generated `$escaper` property comes from.
//! - `NodeVisitor/EscaperTest::testCompilationErrorInsideAnAutoescapeTagDoesNotAffectTheNextTemplate`: an
//!   `is_safe_callback` cannot fail in twig-rs (`SafeCallback` returns the strategies); ported with another
//!   compilation error raised by the escaper visitor inside an `autoescape` tag.
//! - `ExpressionParserTest::testCompiledCodeFor{Test,Function,Filter}` compare generated PHP: ported as renders that
//!   show the arguments the dynamic callable receives.
//! - `TemplateEscapeStrategyTest::testEmbeddedTemplatesExposeTheStrategyOfTheirTemplate` counts methods of
//!   generated PHP: ported on the `Program`s of the template and of its embedded template.
//! - `TemplateEscapeStrategyTest::testTemplatesCompiledBeforeTheStrategyWasExposedReportNoStrategy`: a hand-written
//!   PHP subclass of `Twig\Template`; twig-rs templates are always compiled.
//! - `Node/NodeTest.php`: `Node::__toString()` (twig-rs dumps trees in the oracle generators' format, not Twig's) and
//!   `deprecateAttribute()`/`deprecateNode()` (attributes and children are typed fields).
//! - `Node/MacrosTest.php`: `MacrosNode` is a plain container in twig-rs, without the construction checks of Twig's
//!   class (the parser only puts `MacroNode`s in it).
//! - The constructor and compile tests of the other `NodeTestCase` subclasses (`Node/**`): typed constructors, and
//!   generated PHP.
//! - `Profiler/**`: twig-rs has no profiler.

mod common;

#[path = "upstream/visitors/mod.rs"]
mod visitors;
