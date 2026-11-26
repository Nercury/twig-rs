//! The code generator: lowers a visited `ModuleNode` into a [`Program`], mirroring each node's `compile()` method.
//!
//! The generator walks the tree in the order Twig's compiler writes PHP (`ModuleNode::compileTemplate()`: the
//! constructor with the traits, `doGetParent()`, `doDisplay()`, the blocks in definition order, the macros, then the
//! embedded templates), so that:
//!
//! - expressions are evaluated in PHP's order (left to right, keys before values in array literals, the right operand
//!   of `matches` first, the double evaluations of `?:`, `??` and `default`);
//! - the line markers of `Compiler::addDebugInfo()` are emitted at the same points with the same file-wide
//!   "only when the line changed" state, which is what makes `Error::guess()` reproducible from the line tables;
//! - the compile-time decisions that change runtime behaviour are taken from the same flags (`strict_variables`, the
//!   `SandboxExtension`, `always_defined`, `ignore_strict_check`, `optimizable`, `with_loop`, ...), including the
//!   attribute mutations Twig performs while compiling (the `ignore_strict_check` a `GetAttrExpression` sets on its
//!   object node, the short-circuit marks of null-safe chains shared by `??`, the single temporary of `apply`).

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::{Arc, OnceLock};

use twig_value::object::{ClassRef, ObjectRef, StdClass};
use twig_value::{Array, ArrayKey, PhpError, PhpString, ThrowableClass, Value};

use super::builder::{Compiler, Produces};
use super::instr::{
    BinaryOp, BlockOp, ConstId, GetAttrOp, Instr, Label, LocalId, NameMode, UnitId, YieldArgs,
};
use super::php::{self, PhpModel, folds_children, is_php_literal};
use super::program::{
    BlockDef, FileId, LineTable, MacroDef, MacroParam, ParentDef, Program, SecurityLists, TraitDef,
    Unit, UnitKind,
};
use super::runtime::NodeRuntime;
use crate::ast::{
    self, ArgumentKey, Arguments, BinaryExpr, CallType, ConstantValue, Expr, ExprKind, FilterKind,
    ForNode, FunctionKind, GetAttrExpr, MacroName, Node, NodeKey, NodeKind, NodeRef, TempName,
    TestKind, UnaryOp,
};
use crate::deprecation::{Deprecation, DeprecationSink};
use crate::error::{Error, Result};
use crate::extension::arguments::CallableArgumentsExtractor;
use crate::extension::{CallableKind, TwigCallable};
use crate::source::Source;

/// The lookup of an enum by name at compile time: Twig's `enum_exists($name)` (the class, when it exists and is an
/// enum), which `EnumFunction` and `EnumCasesFunction` call while compiling.
pub type EnumResolver = Arc<dyn Fn(&[u8]) -> Option<ClassRef> + Send + Sync>;

/// The environment facts compiled code depends on (Twig reads them from the environment while compiling).
#[derive(Clone, Default)]
pub struct CompileOptions {
    /// `strict_variables`: names compile to existence checks and `[]` access is not optimized.
    pub strict_variables: bool,
    /// Whether the `SandboxExtension` is registered (`$env->hasExtension(SandboxExtension::class)`).
    pub sandboxed: bool,
    /// `debug`: the compiled `getSourceContext()` keeps the template code (`''` otherwise).
    pub debug: bool,
    /// The enums `enum()` and `enum_cases()` can name with a literal argument; without a resolver no enum exists
    /// at compile time.
    pub enums: Option<EnumResolver>,
}

impl fmt::Debug for CompileOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CompileOptions")
            .field("strict_variables", &self.strict_variables)
            .field("sandboxed", &self.sandboxed)
            .field("debug", &self.debug)
            .field("enums", &self.enums.is_some())
            .finish()
    }
}

/// Compiles a visited `ModuleNode` (and its embedded templates) into a [`Program`].
///
/// Twig: `Compiler::compile($node)` with `ModuleNode::compile()` (TWIG/src/Compiler.php,
/// TWIG/src/Node/ModuleNode.php), then what PHP does when `Environment::loadTemplate()` evaluates the generated
/// file: Twig compiles the whole file (the template and its embedded templates) before PHP sees any of it, so its own
/// errors come first; PHP then parses the whole file, so a `ParseError` anywhere in it comes next, and compiles it in
/// file order, so the first compile-time fatal error comes last.
///
/// # Errors
///
/// - The errors Twig raises while compiling: `SyntaxError`s with the template source (argument binding,
///   `Empty array elements are only allowed in destructuring assignments.`) and `LogicException`-kind errors
///   (callables that cannot be reflected, malformed trees, node classes defined by extensions). The caller wraps
///   them like `Environment::compileSource()` does.
/// - Then PHP's `ParseError` `syntax error, unexpected token "..."` (see `php::generated_parse_error`).
/// - Then the first `E_COMPILE_ERROR` of the generated PHP, as a `CompileError` with PHP's message (`Cannot use
///   positional argument after argument unpacking`, `Only arrays and Traversables can be unpacked, X given`; PHP
///   stops with an uncatchable fatal error, `reference/deviations.md`).
pub fn compile(
    module: &Node,
    options: &CompileOptions,
    file: FileId,
    deprecations: &mut dyn DeprecationSink,
) -> Result<Arc<Program>> {
    let mut codegen = Codegen {
        options: options.clone(),
        php: PhpModel {
            strict_variables: options.strict_variables,
        },
        deprecations: Vec::new(),
        file,
        last_line: 0,
        marker: None,
        php_marker: None,
        fold_marker: None,
        own_fold: None,
        deferred_lines: Vec::new(),
        in_folded: false,
        dead: false,
        php_fatal: None,
        program: None,
        units: Vec::new(),
        twig_vars: 0,
        null_safe: HashMap::new(),
        strict_ignored: HashSet::new(),
        aliases: HashMap::new(),
        named_macro_vars: HashSet::new(),
        temporaries: Vec::new(),
        apply_temps: Vec::new(),
    };
    let program = codegen.module(module);
    // Twig reports them while it compiles; nothing reads the sink before the compilation ends
    for deprecation in codegen.deprecations.drain(..) {
        deprecations.report(deprecation);
    }
    let program = program?;
    // PHP parses the whole generated file before compiling any of it
    if php::generated_parse_error(module) {
        return Err(Error::php(PhpError::new(
            ThrowableClass::ParseError,
            "syntax error, unexpected token \"...\"",
        )));
    }
    if let Some(fatal) = codegen.php_fatal.take() {
        return Err(compile_fatal(fatal));
    }
    Ok(program)
}

/// The line of an outermost array literal that is known only once part of its code is compiled: its first value
/// comes after the dynamic key of an element ([`php::FirstLine::AfterKey`]), and the constants PHP folds in that key
/// already take the line. Until the key is compiled, the code carries a placeholder line ([`placeholder_line()`]),
/// which [`Codegen::key_compiled`] replaces in the line table and [`Codegen::emit_at`] maps from then on.
#[derive(Debug, Clone, Copy)]
struct DeferredLine {
    /// The identity of the key (see [`expr_key()`]).
    key: usize,
    /// The unit being compiled (an index of [`Codegen::units`]) and its first instruction that may carry the
    /// placeholder.
    unit: usize,
    from: Label,
    /// The line, once the key is compiled.
    line: Option<Option<i64>>,
}

/// The first placeholder line: placeholders lie between [`LineTable::NO_MARKER`] and every real line (lines are
/// never negative).
const FIRST_PLACEHOLDER: i64 = LineTable::NO_MARKER + 1;

/// The placeholder line of the deferred line `i` (see [`DeferredLine`]).
fn placeholder_line(i: usize) -> i64 {
    FIRST_PLACEHOLDER + i as i64
}

/// What [`Codegen::begin_fold`] changed, for [`Codegen::end_fold`].
struct FoldScope {
    fold_marker: Option<Option<i64>>,
    /// The deferred line the literal's line is (an index of [`Codegen::deferred_lines`]).
    deferred: Option<usize>,
}

/// The short-circuit state of a null-safe `GetAttrExpression` (its `is_short_circuited` and `var_name` attributes).
#[derive(Debug, Clone, Copy)]
struct NullSafeState {
    short_circuited: bool,
    /// The local standing for `$_vN`, and `N`.
    var: Option<(LocalId, u32)>,
}

/// Compiled temporaries kept alive until the end of the compilation (see [`Codegen::temporaries`]).
#[allow(
    dead_code,
    reason = "the trees are only kept allocated, so that the addresses identifying their nodes stay unique"
)]
enum Temporary {
    Exprs(Vec<Expr>),
    Arguments(Box<Arguments>),
}

/// A constant-pool key for deduplication.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum ConstKey {
    Null,
    Bool(bool),
    Int(i64),
    Str(Vec<u8>),
}

struct ProgramBuilder {
    source: Source,
    index: Option<i64>,
    constants: Vec<Value>,
    const_index: HashMap<ConstKey, ConstId>,
    callables: Vec<TwigCallable>,
    runtimes: Vec<NodeRuntime>,
    units: Vec<Unit>,
    security: Option<SecurityLists>,
    /// The number of `ZEND_RECV_INIT` cache slots.
    recv_cache: u32,
}

/// The `$macros` slot a `MacroVariable` names.
enum MacroSlot {
    /// `_self`: `$this->getMacroNamespace()`.
    This,
    /// `$macros[key]`.
    Key(Vec<u8>),
}

struct UnitBuilder {
    kind: UnitKind,
    code: Vec<Instr>,
    lines: LineTable,
    current: Option<i64>,
    locals: u16,
    named_locals: HashMap<TempName, LocalId>,
}

pub(super) struct Codegen {
    pub(super) options: CompileOptions,
    /// What PHP's compiler makes of the generated code (constant folding, operand kinds).
    php: PhpModel,
    /// The deprecations raised while compiling, in order, reported to the caller's sink when the compilation ends.
    deprecations: Vec<Deprecation>,
    file: FileId,
    /// Twig's `Compiler::$lastLine` (`null` compares like `0`).
    last_line: i64,
    /// The line of the last emitted marker, `None` before the first one.
    marker: Option<i64>,
    /// The marker in effect at PHP's `CG(zend_lineno)`: the line of the last AST node whose compilation started
    /// (`zend_compile_expr_inner()` sets it on entry to every expression and nothing restores it), which is the line
    /// an opcode emitted after its operands carries. It is the current marker after a token written by Twig's
    /// compiler, and the line of the enclosing array literal after a constant PHP folded there (see
    /// [`Codegen::fold_marker`]).
    php_marker: Option<i64>,
    /// While the elements of an array literal are compiled (outside the calls, assignments and closures they
    /// contain): the marker of the outermost literal's line, which `zend_try_ct_eval_array()` gives every constant
    /// it folds in them (`zend_eval_const_expr()` stamps a folded value with the current `CG(zend_lineno)`).
    fold_marker: Option<Option<i64>>,
    /// The fold marker of the tokens of the expression being compiled itself: the enclosing literal's, also for an
    /// expression whose children are compiled outside it (the `null` that ends `x['k']`'s inline access).
    own_fold: Option<Option<i64>>,
    /// The lines of array literals that come after a key, in the order they were found (see [`DeferredLine`]).
    deferred_lines: Vec<DeferredLine>,
    /// The expression being compiled is part of one PHP folded into a constant.
    in_folded: bool,
    /// The code being compiled is code PHP's compiler never reaches: the right operand of `&&`/`||` when a constant
    /// left operand decides (`zend_compile_short_circuiting()`), or the branch `zend_eval_const_expr()` drops from a
    /// conditional inside an array literal. Twig still compiles it (its errors still come), but PHP raises no
    /// compile-time error in it.
    dead: bool,
    /// The first compile-time fatal error of the generated PHP that PHP's compiler reaches, in PHP's compile order
    /// (raised by [`compile`] once Twig's compilation finished).
    php_fatal: Option<php::CompileFatal>,
    program: Option<ProgramBuilder>,
    units: Vec<UnitBuilder>,
    /// Twig's `Compiler::$varNameSalt`: the `N` of the next `$_vN` (`Compiler::getVarName()`), counted per compiled
    /// file (a template and its embedded templates) in compile order.
    twig_vars: u32,
    /// The `is_short_circuited` and `var_name` attributes of the null-safe nodes compiled so far, by node identity.
    null_safe: HashMap<usize, NullSafeState>,
    /// The nodes whose `ignore_strict_check` attribute a compiled `GetAttrExpression` set, by node identity.
    strict_ignored: HashSet<usize>,
    /// Twig mutates node objects while compiling them, and some objects are compiled twice (the left operand of
    /// `??` is also the operand of its null test); twig-rs copies such subtrees. A node key (see [`expr_key()`])
    /// maps here to the key of the node that stands for the same PHP object.
    aliases: HashMap<usize, usize>,
    /// The `{% from %}` macro variables (`MacroVariable` without a name) already named by `getVarName()`.
    named_macro_vars: HashSet<u32>,
    /// Trees compiled from copies (the arguments `CallableArgumentsExtractor` binds): node identities are addresses,
    /// so they stay allocated until the end.
    temporaries: Vec<Temporary>,
    apply_temps: Vec<LocalId>,
}

/// `Node "%s" does not exist for Node "%s".` (Twig's `Node::getNode()`).
fn missing_node(name: &str, class: &str) -> Error {
    Error::logic(format!(
        "Node \"{name}\" does not exist for Node \"{class}\"."
    ))
}

impl Codegen {
    // ---- emission ------------------------------------------------------------------------------------------------

    fn program(&mut self) -> &mut ProgramBuilder {
        self.program
            .as_mut()
            .expect("a program is being compiled while code is emitted")
    }

    fn unit(&mut self) -> &mut UnitBuilder {
        self.units
            .last_mut()
            .expect("a unit is being compiled while code is emitted")
    }

    /// Twig: `Compiler::addDebugInfo()`: a marker is emitted when the line differs from the last marker's. The code
    /// written next starts after it.
    pub(super) fn add_debug_info(&mut self, line: i64) {
        if line != self.last_line {
            self.last_line = line;
            self.marker = Some(line);
        }
        self.php_marker = self.marker;
    }

    /// The marker `add_debug_info(line)` would leave in effect.
    fn marker_after(&self, line: Option<i64>) -> Option<i64> {
        match line {
            Some(line) if line != self.last_line => Some(line),
            _ => self.marker,
        }
    }

    /// A token Twig's compiler writes at the current position (a literal, a variable, `$this->source`, ...) was
    /// compiled: PHP's `CG(zend_lineno)` is its line.
    pub(super) fn leaf(&mut self) {
        self.php_marker = self.marker;
    }

    /// A token PHP folds into a constant (`null`, `true`, `false`, a constant operation) was compiled: inside an
    /// array literal its line is the literal's (`zend_eval_const_expr()`), elsewhere its own.
    fn folded(&mut self) {
        self.php_marker = self.own_fold.unwrap_or(self.marker);
    }

    /// Starts the folding of the elements of an outermost array literal (`zend_try_ct_eval_array()`): the constants
    /// it folds take the line of the literal, the line of the PHP node of its first value, which `first` locates. A
    /// line that comes after a key is a placeholder until the key is compiled ([`DeferredLine`]).
    fn begin_fold(&mut self, first: php::FirstLine<'_>) -> FoldScope {
        let mut deferred = None;
        let line = match first {
            php::FirstLine::Marker(line) => self.marker_after(Some(line)),
            php::FirstLine::Start => self.marker,
            php::FirstLine::AfterKey(key) => {
                deferred = Some(self.deferred_lines.len());
                let from = self.here();
                self.deferred_lines.push(DeferredLine {
                    key: expr_key(key),
                    unit: self.units.len() - 1,
                    from,
                    line: None,
                });
                Some(placeholder_line(self.deferred_lines.len() - 1))
            }
        };
        FoldScope {
            fold_marker: self.fold_marker.replace(line),
            deferred,
        }
    }

    /// Ends the folding [`begin_fold`](Self::begin_fold) started.
    fn end_fold(&mut self, scope: FoldScope) {
        // the key was not compiled only when the compilation failed before it: the placeholder gets the marker in
        // effect then
        if let Some(i) = scope.deferred
            && self.deferred_lines[i].line.is_none()
        {
            self.resolve_line(i);
        }
        self.fold_marker = scope.fold_marker;
    }

    /// The dynamic key `key` of an array literal was compiled (its `(string)` cast included): the line of a literal
    /// whose first value comes next is the marker now in effect.
    fn key_compiled(&mut self, key: &Expr) {
        let key = expr_key(key);
        if let Some(i) = self
            .deferred_lines
            .iter()
            .rposition(|deferred| deferred.key == key && deferred.line.is_none())
        {
            self.resolve_line(i);
        }
    }

    /// Gives the deferred line `i` the marker in effect, also in the code compiled so far.
    fn resolve_line(&mut self, i: usize) {
        let line = self.marker;
        let deferred = &mut self.deferred_lines[i];
        deferred.line = Some(line);
        let (unit, from) = (deferred.unit, deferred.from);
        let placeholder = placeholder_line(i);
        let unit = &mut self.units[unit];
        unit.lines
            .replace_from(from, placeholder, line.unwrap_or(LineTable::NO_MARKER));
        if unit.current == Some(placeholder) {
            unit.current = line;
        }
    }

    /// `line`, or the line a placeholder stands for once it is known (see [`DeferredLine`]).
    fn known_line(&self, line: Option<i64>) -> Option<i64> {
        let deferred = line
            .filter(|&line| {
                line >= FIRST_PLACEHOLDER && line < placeholder_line(self.deferred_lines.len())
            })
            .and_then(|line| self.deferred_lines[(line - FIRST_PLACEHOLDER) as usize].line);
        deferred.unwrap_or(line)
    }

    /// Compiles `f` outside the array literal being folded: the operands of calls, assignments, `isset()` and
    /// `instanceof`, which `zend_eval_const_expr()` does not descend into.
    fn without_fold<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        let saved = self.fold_marker.take();
        let result = f(self);
        self.fold_marker = saved;
        result
    }

    /// Whether the code being compiled is part of the elements of an array literal that `zend_try_ct_eval_array()`
    /// folds with `zend_eval_const_expr()` before compiling them (outside the calls, assignments and closures they
    /// contain): constant operands are then decided in eval mode ([`PhpModel::const_operand`]).
    fn folding(&self) -> bool {
        self.fold_marker.is_some()
    }

    /// Whether PHP's compiler reaches the code being compiled: not when it is [dead](Codegen::dead), nor when it is
    /// part of an expression `zend_eval_const_expr()` replaced by a constant.
    fn php_compiles(&self) -> bool {
        !self.dead && !self.in_folded
    }

    /// A compile-time fatal error of the generated PHP at this point of PHP's compile order: kept when PHP's
    /// compiler reaches it and no earlier one was recorded.
    fn php_fatal(&mut self, fatal: php::CompileFatal) {
        if self.php_compiles() && self.php_fatal.is_none() {
            self.php_fatal = Some(fatal);
        }
    }

    /// After compiling the argument `argument` of a call whose next argument is positional: when `argument` is a
    /// spread, `zend_compile_args()` fails there with `Cannot use positional argument after argument unpacking`
    /// (PHPSRC/Zend/zend_compile.c).
    fn positional_after(&mut self, argument: &Expr) {
        if is_spread(argument) {
            self.php_fatal(php::CompileFatal(
                "Cannot use positional argument after argument unpacking".to_owned(),
            ));
        }
    }

    /// Compiles `f` as [dead](Codegen::dead) code when `dead`.
    fn dead_if<T>(&mut self, dead: bool, f: impl FnOnce(&mut Self) -> T) -> T {
        let saved = self.dead;
        self.dead |= dead;
        let result = f(self);
        self.dead = saved;
        result
    }

    /// Emits an instruction at the line PHP gives an opcode emitted at this point: `CG(zend_lineno)`, the line of the
    /// last compiled token (see [`Codegen::php_marker`]).
    pub(super) fn emit(&mut self, instr: Instr) -> Label {
        let line = self.php_marker;
        self.emit_at(instr, line)
    }

    /// Emits an instruction attributed to `line` (the marker in effect where the PHP construct starts: calls report
    /// the line of their beginning).
    pub(super) fn emit_at(&mut self, instr: Instr, line: Option<i64>) -> Label {
        let line = self.known_line(line);
        let unit = self.unit();
        let ip = unit.code.len() as Label;
        if line != unit.current {
            unit.lines.push(ip, line.unwrap_or(LineTable::NO_MARKER));
            unit.current = line;
        }
        unit.code.push(instr);
        ip
    }

    /// The label of the next instruction.
    pub(super) fn here(&mut self) -> Label {
        self.unit().code.len() as Label
    }

    /// Points the jump at `at` to `target`.
    fn patch(&mut self, at: Label, target: Label) {
        let instr = &mut self.unit().code[at as usize];
        match instr {
            Instr::Jump(t) | Instr::JumpIfFalse(t) | Instr::JumpIfTrue(t) => *t = target,
            Instr::IterNext { exit, .. } => *exit = target,
            Instr::TryLoader { catch } => *catch = target,
            Instr::RecvInit { skip, .. } => *skip = target,
            _ => {}
        }
    }

    /// Points the jump at `at` to the next instruction.
    pub(super) fn patch_here(&mut self, at: Label) {
        let target = self.here();
        self.patch(at, target);
    }

    pub(super) fn local(&mut self) -> LocalId {
        let unit = self.unit();
        let id = unit.locals;
        unit.locals = unit.locals.saturating_add(1);
        id
    }

    /// The local of a `LocalVariable`: the temporary of the enclosing `apply` for a name generated at compile time,
    /// otherwise one slot per name.
    fn named_local(&mut self, name: &TempName) -> LocalId {
        if *name == TempName::Null {
            if let Some(&slot) = self.apply_temps.last() {
                return slot;
            }
            return self.local();
        }
        if let Some(&slot) = self.unit().named_locals.get(name) {
            return slot;
        }
        let slot = self.local();
        self.unit().named_locals.insert(name.clone(), slot);
        slot
    }

    pub(super) fn constant(&mut self, value: Value) -> ConstId {
        let key = match &value {
            Value::Null => Some(ConstKey::Null),
            Value::Bool(b) => Some(ConstKey::Bool(*b)),
            Value::Int(i) => Some(ConstKey::Int(*i)),
            Value::String(s) => Some(ConstKey::Str(s.as_bytes().to_vec())),
            _ => None,
        };
        let program = self.program();
        if let Some(key) = &key
            && let Some(&id) = program.const_index.get(key)
        {
            return id;
        }
        let id = program.constants.len() as ConstId;
        program.constants.push(value);
        if let Some(key) = key {
            program.const_index.insert(key, id);
        }
        id
    }

    pub(super) fn name_constant(&mut self, name: &[u8]) -> ConstId {
        self.constant(Value::String(PhpString::from(name)))
    }

    pub(super) fn emit_const(&mut self, value: Value) {
        let id = self.constant(value);
        self.emit(Instr::Const(id));
    }

    /// The index of a runtime closure in the program's runtimes (each closure once).
    pub(super) fn runtime(&mut self, runtime: &NodeRuntime) -> u32 {
        let program = self.program();
        if let Some(i) = program.runtimes.iter().position(|r| r.ptr_eq(runtime)) {
            return i as u32;
        }
        program.runtimes.push(runtime.clone());
        (program.runtimes.len() - 1) as u32
    }

    /// The line of the last emitted marker (`None` before the first one): where a call written now starts.
    pub(super) fn marker(&self) -> Option<i64> {
        self.marker
    }

    fn callable(&mut self, callable: &TwigCallable) -> u32 {
        let program = self.program();
        if let Some(i) = program.callables.iter().position(|c| c == callable) {
            return i as u32;
        }
        program.callables.push(callable.clone());
        (program.callables.len() - 1) as u32
    }

    fn source(&self) -> Source {
        self.program
            .as_ref()
            .map(|p| p.source.clone())
            .expect("a program is being compiled")
    }

    fn begin_unit(&mut self, kind: UnitKind) {
        self.units.push(UnitBuilder {
            kind,
            code: Vec::new(),
            lines: LineTable::default(),
            current: None,
            locals: 0,
            named_locals: HashMap::new(),
        });
    }

    fn finish_unit(&mut self) -> UnitId {
        let unit = self
            .units
            .pop()
            .expect("finish_unit() matches begin_unit()");
        let program = self.program();
        program.units.push(Unit {
            kind: unit.kind,
            code: unit.code,
            lines: unit.lines,
            locals: unit.locals,
        });
        (program.units.len() - 1) as UnitId
    }

    // ---- modules -------------------------------------------------------------------------------------------------

    /// Twig: `ModuleNode::compileTemplate()` and the subcompilation of the embedded templates.
    fn module(&mut self, node: &Node) -> Result<Arc<Program>> {
        let NodeKind::Module(module) = &node.kind else {
            return Err(Error::logic(format!(
                "Cannot compile a \"{}\" node as a template.",
                node.class_name()
            )));
        };
        let saved = self.program.replace(ProgramBuilder {
            source: module.source.clone(),
            index: module.index,
            constants: Vec::new(),
            const_index: HashMap::new(),
            callables: Vec::new(),
            runtimes: Vec::new(),
            units: Vec::new(),
            security: None,
            recv_cache: 0,
        });
        let result = self.module_body(module);
        let builder = std::mem::replace(&mut self.program, saved)
            .expect("the program compiled by module_body()");
        let parts = result?;
        Ok(Arc::new(Program {
            file: self.file,
            source: builder.source,
            index: builder.index,
            keys: Program::keys_of(&builder.constants),
            constants: builder.constants,
            callables: builder.callables,
            runtimes: builder.runtimes,
            units: builder.units,
            body: parts.body,
            blocks: parts.blocks,
            macros: parts.macros,
            parent: parts.parent,
            traits: parts.traits,
            embedded: parts.embedded,
            traitable: parts.traitable,
            strategy: module.strategy.clone(),
            escaper: module.escaper,
            security: builder.security,
            debug: self.options.debug,
            recv_cache: (0..builder.recv_cache).map(|_| OnceLock::new()).collect(),
        }))
    }

    fn module_body(&mut self, module: &ast::ModuleNode) -> Result<ModuleParts> {
        // compileConstructor(): constructor_start, the traits, constructor_end
        self.statement(&module.constructor_start)?;
        let mut traits = Vec::new();
        if let Some(nodes) = module.traits.as_nodes() {
            for trait_node in nodes.nodes() {
                traits.push(self.trait_def(trait_node)?);
            }
        }
        self.statement(&module.constructor_end)?;

        // compileGetParent()
        let parent = match &module.parent {
            None => None,
            Some(expr) => match &expr.kind {
                ExprKind::Constant(constant) => {
                    self.add_debug_info(expr.line);
                    Some(ParentDef::Constant {
                        name: const_value(&constant.value),
                        line: expr.line,
                    })
                }
                _ => {
                    // `$macros = $this->macros;` then `return $this->load(<parent>, line);`
                    self.begin_unit(UnitKind::Parent);
                    self.add_debug_info(expr.line);
                    let start = self.marker;
                    self.expr(expr)?;
                    self.emit_at(
                        Instr::LoadTemplate {
                            line: expr.line,
                            index: None,
                        },
                        start,
                    );
                    self.emit(Instr::Return);
                    let unit = self.finish_unit();
                    Some(ParentDef::Dynamic {
                        unit,
                        line: expr.line,
                    })
                }
            },
        };

        // compileDisplay()
        self.begin_unit(UnitKind::Body);
        self.statement(&module.display_start)?;
        self.statement(&module.body)?;
        if let Some(expr) = &module.parent {
            // `$this->parent = $this->load(<parent>, line);` (constant parent) or `$this->getParent($context)`, then
            // `yield from <parent>->unwrap()->yield($context, array_merge($this->blocks, $blocks));`
            self.add_debug_info(expr.line);
            match &expr.kind {
                ExprKind::Constant(constant) => {
                    self.emit_const(const_value(&constant.value));
                    self.emit(Instr::LoadTemplate {
                        line: expr.line,
                        index: None,
                    });
                    self.emit(Instr::SetParent);
                }
                _ => {
                    self.emit(Instr::GetParent { this: true });
                }
            }
            self.emit(Instr::YieldTemplate(YieldArgs::Parent));
        }
        self.statement(&module.display_end)?;
        self.emit(Instr::Return);
        let body = self.finish_unit();

        // the blocks
        let mut blocks = Vec::new();
        for (name, block) in module.blocks() {
            let NodeKind::Block(block_node) = &block.kind else {
                continue;
            };
            // BlockNode::compile()
            self.add_debug_info(block.line);
            self.begin_unit(UnitKind::Block(name.to_vec()));
            self.statement(&block_node.body)?;
            self.emit(Instr::Return);
            let unit = self.finish_unit();
            blocks.push(BlockDef {
                name: name.to_vec(),
                unit,
                line: block.line,
            });
        }

        // compileMacros()
        let mut macros = Vec::new();
        for (_, macro_node) in module.macros() {
            macros.push(self.macro_def(macro_node)?);
        }

        let traitable = is_traitable(module);

        // compileClassFooter()
        self.statement(&module.class_end)?;

        // the embedded templates
        let mut embedded = Vec::new();
        for template in module.embedded_templates() {
            embedded.push(self.module(template)?);
        }

        Ok(ModuleParts {
            body,
            blocks,
            macros,
            parent,
            traits,
            embedded,
            traitable,
        })
    }

    /// The trait part of `ModuleNode::compileConstructor()`.
    fn trait_def(&mut self, node: &Node) -> Result<TraitDef> {
        let nodes = node
            .as_nodes()
            .ok_or_else(|| missing_node("template", &node.class_name()))?;
        let template = nodes
            .get(&NodeKey::Name(b"template".to_vec()))
            .ok_or_else(|| missing_node("template", &node.class_name()))?;
        let expr = template
            .as_expr()
            .ok_or_else(|| missing_node("template", &node.class_name()))?;
        self.add_debug_info(expr.line);
        let mut targets = Vec::new();
        if let Some(targets_node) = nodes
            .get(&NodeKey::Name(b"targets".to_vec()))
            .and_then(Node::as_nodes)
        {
            for (key, value) in targets_node.entries() {
                let name = match key {
                    NodeKey::Name(name) => name.clone(),
                    NodeKey::Index(i) => i.to_string().into_bytes(),
                };
                let alias = value
                    .as_expr()
                    .and_then(Expr::as_constant)
                    .map_or(Value::Null, const_value);
                targets.push((name, alias));
            }
        }
        Ok(TraitDef {
            template: expr.as_constant().map_or(Value::Null, const_value),
            line: expr.line,
            targets,
        })
    }

    /// Twig: `MacroNode::compile()`.
    fn macro_def(&mut self, node: &Node) -> Result<MacroDef> {
        let NodeKind::Macro(macro_node) = &node.kind else {
            return Err(Error::logic(format!(
                "Cannot compile a \"{}\" node as a macro.",
                node.class_name()
            )));
        };
        let mut params = Vec::new();
        // the defaults PHP cannot fold: the parameter, its default and the value standing for it
        let mut deferred: Vec<(Vec<u8>, &Expr, Value)> = Vec::new();
        if let ExprKind::Array(arguments) = &macro_node.arguments.kind {
            for (key, value) in &arguments.elements {
                let name: Vec<u8> = match &key.kind {
                    ExprKind::LocalVariable(TempName::String(name)) => {
                        TempName::strip_reserved_prefix(name).to_vec()
                    }
                    ExprKind::LocalVariable(TempName::Int(i)) => i.to_string().into_bytes(),
                    _ => Vec::new(),
                };
                let explicit_default =
                    !matches!(&value.kind, ExprKind::Constant(c) if c.is_implicit);
                // MacroNode::compile() writes the default into the closure's signature (`function ($a = <default>,
                // ...)`); zend_compile_params() folds it with zend_const_expr_to_zval() (zend_eval_const_expr()),
                // and keeps a default that cannot be folded without an error as a constant expression (an
                // `IS_CONSTANT_AST` value), which ZEND_RECV_INIT evaluates when the argument is missing
                let default = match self.php.eval_const(value) {
                    Some(default) => default,
                    None => {
                        let constant_ast = Value::Object(ObjectRef::new(StdClass::new()));
                        deferred.push((name.clone(), value, constant_ast.clone()));
                        constant_ast
                    }
                };
                params.push(MacroParam {
                    name: PhpString::from(name),
                    default,
                    explicit_default,
                });
            }
        }
        self.begin_unit(UnitKind::Macro(macro_node.name.clone()));
        // ZEND_RECV_INIT: the first opcodes of the closure, on the line of its declaration, which Twig writes before
        // the macro's own marker (the marker in effect is the last one compiled before the macro)
        self.leaf();
        for (name, default, constant_ast) in deferred {
            let name = self.name_constant(&name);
            let constant_ast = self.constant(constant_ast);
            let cache = self.recv_cache_slot();
            let init = self.emit(Instr::RecvInit {
                name,
                constant_ast,
                cache,
                skip: 0,
            });
            self.constant_expression(default)?;
            self.emit(Instr::RecvStore { name, cache });
            self.patch_here(init);
        }
        self.add_debug_info(node.line);
        self.emit(Instr::CaptureStart);
        self.statement(&macro_node.body)?;
        self.emit(Instr::CaptureEnd { markup: true });
        self.emit(Instr::Return);
        let unit = self.finish_unit();
        Ok(MacroDef {
            name: macro_node.name.clone(),
            unit,
            params,
            variadic: macro_node.variadic_name.clone(),
            line: node.line,
        })
    }

    /// A runtime cache slot of `ZEND_RECV_INIT` (see [`Program::recv_cache`]).
    fn recv_cache_slot(&mut self) -> u32 {
        let program = self.program();
        program.recv_cache += 1;
        program.recv_cache - 1
    }

    /// The code evaluating a constant expression at runtime: `zend_ast_evaluate()` (PHPSRC/Zend/zend_ast.c) of the
    /// default of a macro argument that PHP could not fold, whose operands it evaluates in its own order: a unary
    /// minus is `mul_function(-1, value)`, a unary plus `add_function(0, value)`, an array literal adds its
    /// elements in order, each key before its value. The parts PHP folded while compiling are constants.
    fn constant_expression(&mut self, expr: &Expr) -> Result<()> {
        if let Some(value) = self.php.eval_const(expr) {
            self.emit_const(value);
            return Ok(());
        }
        match &expr.kind {
            ExprKind::Unary(unary) if matches!(unary.op, UnaryOp::Neg | UnaryOp::Pos) => {
                let (left, op) = if unary.op == UnaryOp::Neg {
                    (-1, BinaryOp::Mul)
                } else {
                    (0, BinaryOp::Add)
                };
                self.emit_const(Value::Int(left));
                self.constant_expression(&unary.node)?;
                self.emit(Instr::Binary(op));
            }
            ExprKind::Array(array) => {
                self.emit(Instr::NewArray);
                let mut is_sequence = true;
                for (i, (key, value)) in array.elements.iter().enumerate() {
                    let key_value = match &key.kind {
                        ExprKind::LocalVariable(name) => Some(match name {
                            TempName::Null => Value::Null,
                            TempName::Int(i) => Value::Int(*i),
                            TempName::String(s) => Value::String(PhpString::from(s.as_slice())),
                        }),
                        ExprKind::Constant(constant) => Some(const_value(&constant.value)),
                        _ => None,
                    };
                    if !matches!(&key_value, Some(Value::Int(k)) if *k == i as i64) {
                        is_sequence = false;
                    }
                    if is_sequence {
                        self.constant_expression(value)?;
                        self.emit(Instr::ArrayAppend);
                        continue;
                    }
                    match key_value {
                        Some(key) => self.emit_const(key),
                        None => {
                            self.constant_expression(key)?;
                            self.emit(Instr::StringCast);
                        }
                    }
                    self.constant_expression(value)?;
                    self.emit(Instr::ArrayInsert);
                }
            }
            // MacroTokenParser::checkConstantExpression() admits nothing else
            _ => self.expr(expr)?,
        }
        Ok(())
    }

    // ---- statements ----------------------------------------------------------------------------------------------

    pub(super) fn statement(&mut self, node: &Node) -> Result<()> {
        match &node.kind {
            NodeKind::Nodes(nodes) | NodeKind::Body(nodes) => {
                let apply = node.tag.as_deref() == Some("apply");
                if apply {
                    let slot = self.local();
                    self.apply_temps.push(slot);
                }
                let mut result = Ok(());
                for child in nodes.nodes() {
                    result = self.statement(child);
                    if result.is_err() {
                        break;
                    }
                }
                if apply {
                    self.apply_temps.pop();
                }
                result
            }
            NodeKind::Text(text) => {
                // TextNode::compile()
                self.add_debug_info(node.line);
                let id = self.constant(Value::String(PhpString::from(text.data.as_slice())));
                self.emit(Instr::Text(id));
                Ok(())
            }
            NodeKind::Print(print) => {
                // PrintNode::compile()
                self.add_debug_info(node.line);
                self.expr(&print.expr)?;
                self.emit(Instr::Print);
                Ok(())
            }
            NodeKind::If(if_node) => self.if_node(node, if_node),
            NodeKind::For(for_node) => self.for_node(node, for_node),
            NodeKind::Set(set) => self.set_node(node, set),
            NodeKind::Capture(capture) => {
                self.capture(capture)?;
                self.emit(Instr::Pop);
                Ok(())
            }
            NodeKind::With(with) => {
                // WithNode::compile()
                self.add_debug_info(node.line);
                let saved = self.local();
                self.twig_var();
                self.emit(Instr::LoadContext);
                self.emit(Instr::StoreLocal(saved));
                if let Some(variables) = &with.variables {
                    let local = self.local();
                    self.twig_var();
                    self.expr(variables)?;
                    self.emit(Instr::StoreLocal(local));
                    // `if (!is_iterable($_vN)) { throw ... }`, `$_vN = CoreExtension::toArray($_vN);`, `$context =
                    // $_vN + $context + ...;`: statements written after the markers of the variables
                    self.leaf();
                    self.emit(Instr::WithVars {
                        local,
                        only: with.only,
                        line: variables.line,
                    });
                }
                self.statement(&with.body)?;
                // `$context = $_vN;`
                self.leaf();
                self.emit(Instr::LoadLocal(saved));
                self.emit(Instr::SetContext);
                Ok(())
            }
            NodeKind::Do(do_node) => {
                // DoNode::compile()
                self.add_debug_info(node.line);
                self.expr(&do_node.expr)?;
                self.emit(Instr::Pop);
                Ok(())
            }
            NodeKind::Flush => {
                // FlushNode::compile()
                self.add_debug_info(node.line);
                self.emit(Instr::Flush);
                Ok(())
            }
            NodeKind::Deprecated(deprecated) => {
                // DeprecatedNode::compile(): `trigger_deprecation(package, version, sprintf("%s in \"name\" at line
                // N.", message, "name"))`: package, version, then the message
                self.add_debug_info(node.line);
                let start = self.marker;
                if let Some(package) = &deprecated.package {
                    self.expr(package)?;
                }
                if let Some(version) = &deprecated.version {
                    self.expr(version)?;
                }
                self.expr(&deprecated.expr)?;
                // zend_compile_func_sprintf() (PHPSRC/Zend/zend_compile.c) compiles a sprintf() with a constant
                // `%s`-only format into a rope: the arguments first (the template name literal last), then the rope,
                // which converts the message to a string at the line of that literal, before trigger_deprecation()
                // checks its parameters
                self.leaf();
                self.emit(Instr::StringCast);
                self.emit_at(
                    Instr::Deprecated {
                        has_package: deprecated.package.is_some(),
                        has_version: deprecated.version.is_some(),
                        line: node.line,
                    },
                    start,
                );
                Ok(())
            }
            NodeKind::AutoEscape(auto_escape) => self.statement(&auto_escape.body),
            NodeKind::BlockReference(block) => {
                // BlockReferenceNode::compile(): `yield from $this->unwrap()->yieldBlock('NAME', $context, $blocks);`
                self.add_debug_info(node.line);
                self.emit_const(Value::String(PhpString::from(block.name.as_slice())));
                self.emit(Instr::Block {
                    op: BlockOp::Yield,
                    template: false,
                });
                Ok(())
            }
            NodeKind::Include(include) => self.include(
                node.line,
                IncludeTarget::Expr(&include.expr),
                include.variables.as_ref(),
                include.only,
                include.ignore_missing,
            ),
            NodeKind::Embed(embed) => self.include(
                node.line,
                IncludeTarget::Embedded {
                    name: &embed.name,
                    index: embed.index,
                },
                embed.variables.as_ref(),
                embed.only,
                embed.ignore_missing,
            ),
            NodeKind::Import(import) => self.import(node, import),
            NodeKind::Sandbox(sandbox) => {
                // SandboxNode::compile(): the sandbox is switched on around the body, in a try/finally
                self.add_debug_info(node.line);
                let local = self.local();
                self.emit(Instr::SandboxStart { local });
                self.statement(&sandbox.body)?;
                self.emit(Instr::SandboxEnd { local });
                Ok(())
            }
            NodeKind::Expression(expr) => match &expr.kind {
                ExprKind::BlockReference(b) if b.output && !b.defined_test => self.expr(expr),
                ExprKind::Parent(p) if p.output => self.expr(expr),
                _ => {
                    self.expr(expr)?;
                    self.emit(Instr::Pop);
                    Ok(())
                }
            },
            NodeKind::Empty
            | NodeKind::Macros(_)
            | NodeKind::Block(_)
            | NodeKind::Macro(_)
            | NodeKind::MacroDeclaration(_)
            | NodeKind::Config
            | NodeKind::Types(_)
            | NodeKind::Type(_)
            | NodeKind::ForLoop(_)
            | NodeKind::ForElse(_)
            | NodeKind::CheckSecurityCall => Ok(()),
            NodeKind::CheckSecurity(check) => {
                // CheckSecurityNode::compile(): `ensureSecurityChecked()` and `checkSecurity()` with the four lists
                self.program().security = Some(SecurityLists {
                    tags: check.used_tags.clone(),
                    filters: check.used_filters.clone(),
                    functions: check.used_functions.clone(),
                    tests: check.used_tests.clone(),
                });
                Ok(())
            }
            NodeKind::Module(_) => Err(Error::logic(
                "A \"Twig\\Node\\ModuleNode\" cannot be compiled inside a template body.",
            )),
            NodeKind::Custom(custom) => {
                // Twig: the node's own `compile()`
                let mut compiler = Compiler::new(self, custom.class_name(), Produces::Nothing);
                custom.compile(&mut compiler)?;
                compiler.finish()
            }
        }
    }

    /// Twig: `IfNode::compile()`.
    fn if_node(&mut self, node: &Node, if_node: &ast::IfNode) -> Result<()> {
        self.add_debug_info(node.line);
        let tests = if_node.tests.as_nodes();
        let count = tests.map_or(0, ast::Nodes::len) as i64;
        let mut end_jumps = Vec::new();
        let mut i = 0;
        while i < count {
            let tests = if_node.tests.as_nodes();
            let condition = tests
                .and_then(|t| t.get(&NodeKey::Index(i)))
                .ok_or_else(|| missing_node(&i.to_string(), "Twig\\Node\\Nodes"))?;
            match condition.as_expr() {
                Some(expr) => self.expr(expr)?,
                None => self.statement(condition)?,
            }
            let skip = self.emit(Instr::JumpIfFalse(0));
            if let Some(body) = tests.and_then(|t| t.get(&NodeKey::Index(i + 1))) {
                self.statement(body)?;
            }
            end_jumps.push(self.emit(Instr::Jump(0)));
            self.patch_here(skip);
            i += 2;
        }
        if let Some(else_node) = &if_node.else_ {
            self.statement(else_node)?;
        }
        for jump in end_jumps {
            self.patch_here(jump);
        }
        Ok(())
    }

    /// Twig: `ForNode::compile()` with `ForLoopNode::compile()` and `ForElseNode::compile()`.
    fn for_node(&mut self, node: &Node, for_node: &ForNode) -> Result<()> {
        self.add_debug_info(node.line);
        let key_name = for_node
            .key_target
            .context_variable_name()
            .unwrap_or(b"_key")
            .to_vec();
        let value_name = for_node
            .value_target
            .context_variable_name()
            .unwrap_or_default()
            .to_vec();
        let key = self.name_constant(&key_name);
        let value = self.name_constant(&value_name);
        let has_else = for_node.else_.is_some();
        let with_loop = for_node.with_loop;

        self.emit(Instr::ForParent);
        let start = self.marker;
        self.expr(&for_node.seq)?;
        self.emit_at(Instr::EnsureTraversable, start);
        let seq = self.name_constant(b"_seq");
        self.emit(Instr::StoreName(seq));
        // the statements after `$context['_seq'] = ...;` (`count($context['_seq'])` of a Countable, the `foreach`
        // with its iterator calls) are compiled at the line they are written on (zend_compile_stmt()), after the
        // markers of the sequence
        self.leaf();
        if has_else {
            let iterated = self.name_constant(b"_iterated");
            self.emit_const(Value::Bool(false));
            self.emit(Instr::StoreName(iterated));
        }
        if with_loop {
            self.emit(Instr::LoopInit);
        }
        self.emit(Instr::IterStart);
        let head = self.emit(Instr::IterNext {
            exit: 0,
            key,
            value,
        });
        match &for_node.body.kind {
            NodeKind::Nodes(nodes) | NodeKind::Body(nodes) => {
                for child in nodes.nodes() {
                    if let NodeKind::ForLoop(_) = child.kind {
                        self.for_loop_end(has_else, with_loop);
                    } else {
                        self.statement(child)?;
                    }
                }
            }
            _ => self.statement(&for_node.body)?,
        }
        self.emit(Instr::Jump(head));
        self.patch_here(head);
        self.emit(Instr::IterEnd);
        if let Some(else_node) = &for_node.else_ {
            // ForElseNode::compile(): `if (!$context['_iterated']) { ... }`
            self.add_debug_info(else_node.line);
            let iterated = self.name_constant(b"_iterated");
            self.emit(Instr::LoadName {
                name: iterated,
                mode: NameMode::AlwaysDefined,
            });
            let skip = self.emit(Instr::JumpIfTrue(0));
            match &else_node.kind {
                NodeKind::ForElse(for_else) => self.statement(&for_else.body)?,
                _ => self.statement(else_node)?,
            }
            self.patch_here(skip);
        }
        // `$_parent = $context['_parent']; unset(...); $context = array_intersect_key($context, $_parent); ...`:
        // statements of their own, after the last marker of the body (and of the else branch)
        self.leaf();
        self.emit(Instr::ForRestore {
            key,
            value,
            has_else,
            with_loop,
        });
        Ok(())
    }

    /// Twig: `ForLoopNode::compile()` with the attributes `ForNode::compile()` copies to it: statements of their own
    /// (`$context['_iterated'] = true;`, `++$context['loop']['index0'];`, ...), which PHP compiles at the line they
    /// are written on (zend_compile_stmt(), PHPSRC/Zend/zend_compile.c): after the last marker of the body, also one
    /// in code PHP skips or replaces by a constant.
    fn for_loop_end(&mut self, has_else: bool, with_loop: bool) {
        self.leaf();
        if has_else {
            let iterated = self.name_constant(b"_iterated");
            self.emit_const(Value::Bool(true));
            self.emit(Instr::StoreName(iterated));
        }
        if with_loop {
            self.emit(Instr::LoopUpdate);
        }
    }

    /// Twig: `SetNode::compile()`.
    fn set_node(&mut self, node: &Node, set: &ast::SetNode) -> Result<()> {
        self.add_debug_info(node.line);
        let names: Vec<&Node> = match &set.names.kind {
            NodeKind::Nodes(nodes) | NodeKind::Body(nodes) => nodes.nodes().collect(),
            _ => vec![&set.names],
        };
        if names.len() > 1 {
            // [$context["a"], $context["b"]] = [<v1>, <v2>];
            if set.capture {
                self.statement_value(&set.values)?;
            } else {
                self.emit(Instr::NewArray);
                let values: Vec<&Node> = match &set.values.kind {
                    NodeKind::Nodes(nodes) | NodeKind::Body(nodes) => nodes.nodes().collect(),
                    _ => vec![&set.values],
                };
                // zend_compile_array() of `[v0, v1, ...]`: zend_try_ct_eval_array() folds the values first
                if self.php_compiles()
                    && let Some(fatal) = self
                        .php
                        .values_eval_fatal(values.iter().filter_map(|value| value.as_expr()))
                {
                    self.php_fatal(fatal);
                }
                // the array literal of the values folds its elements at its line, the line of its first value
                let first = values
                    .first()
                    .and_then(|value| value.as_expr())
                    .map_or(php::FirstLine::Start, php::first_line);
                let fold = self.begin_fold(first);
                let mut result = Ok(());
                for value in values {
                    result = self.statement_value(value);
                    if result.is_err() {
                        break;
                    }
                    self.emit(Instr::ArrayAppend);
                }
                self.end_fold(fold);
                result?;
            }
            let mut targets = Array::new();
            for name in &names {
                let target = name
                    .as_expr()
                    .and_then(Expr::context_variable_name)
                    .unwrap_or_default();
                let _ = targets.push(Value::String(PhpString::from(target)));
            }
            let targets = self.constant(Value::Array(targets));
            self.emit(Instr::ListAssign(targets));
            self.emit(Instr::Pop);
            return Ok(());
        }

        // SetNode::compile() writes the target first: the `LocalVariable` without a name of `apply` is named then
        if let Some(Some(ExprKind::LocalVariable(TempName::Null))) = names
            .first()
            .map(|target| target.as_expr().map(|e| &e.kind))
        {
            self.twig_var();
        }
        if set.capture {
            self.statement_value(&set.values)?;
        } else if set.safe {
            match set.values.as_expr().map(|e| &e.kind) {
                Some(ExprKind::Constant(constant)) => {
                    if constant.value == ConstantValue::String(Vec::new()) {
                        self.emit_const(Value::String(PhpString::empty()));
                    } else {
                        self.emit_const(const_value(&constant.value));
                        self.emit(Instr::MakeMarkup);
                    }
                }
                _ => {
                    self.statement_value(&set.values)?;
                    self.emit(Instr::MarkupUnlessEmpty);
                }
            }
        } else {
            self.statement_value(&set.values)?;
        }
        match names.first() {
            Some(target) => self.store_target(target),
            None => {
                self.emit(Instr::Pop);
                Ok(())
            }
        }
    }

    /// A node that compiles to a value (an expression, a `CaptureNode`, or a `Nodes` container holding one).
    fn statement_value(&mut self, node: &Node) -> Result<()> {
        match &node.kind {
            NodeKind::Expression(expr) => self.expr(expr),
            NodeKind::Capture(capture) => self.capture(capture),
            NodeKind::Nodes(nodes) | NodeKind::Body(nodes) if nodes.len() == 1 => {
                match nodes.nodes().next() {
                    Some(child) => self.statement_value(child),
                    None => Ok(()),
                }
            }
            _ => Err(Error::logic(format!(
                "A \"{}\" node does not compile to a value.",
                node.class_name()
            ))),
        }
    }

    /// Stores the top of the stack into an assignment target (`AssignContextVariable` or `LocalVariable`).
    fn store_target(&mut self, target: &Node) -> Result<()> {
        let Some(expr) = target.as_expr() else {
            return Err(Error::logic(format!(
                "Cannot assign to a \"{}\" node.",
                target.class_name()
            )));
        };
        match &expr.kind {
            ExprKind::AssignContextVariable(name) | ExprKind::ContextVariable(name) => {
                let id = self.name_constant(&name.name);
                self.emit(Instr::StoreName(id));
                Ok(())
            }
            ExprKind::LocalVariable(name) => {
                let slot = self.named_local(name);
                self.emit(Instr::StoreLocal(slot));
                Ok(())
            }
            _ => Err(Error::logic(format!(
                "Cannot assign to a \"{}\" node.",
                expr.class_name()
            ))),
        }
    }

    /// Twig: `CaptureNode::compile()`.
    fn capture(&mut self, capture: &ast::CaptureNode) -> Result<()> {
        self.emit(Instr::CaptureStart);
        self.statement(&capture.body)?;
        self.emit(Instr::CaptureEnd {
            markup: !capture.raw,
        });
        Ok(())
    }

    // ---- expressions ---------------------------------------------------------------------------------------------

    /// Compiles an expression.
    ///
    /// Besides the code, this tracks PHP's line state: `zend_compile_expr_inner()` sets `CG(zend_lineno)` to the
    /// line of every expression it starts compiling (PHPSRC/Zend/zend_compile.c), an expression PHP folds into a
    /// constant inside an array literal takes the literal's line unless the constant is a value of the parser, which
    /// keeps its own ([`PhpModel::keeps_parsed_line`]), and the operands of calls and assignments are not part of the
    /// literal's folding.
    pub(super) fn expr(&mut self, expr: &Expr) -> Result<()> {
        self.php_marker = self.marker;
        let saved_fold = self.fold_marker;
        let saved_own = self.own_fold;
        let saved_in_folded = self.in_folded;
        self.own_fold = self.fold_marker;
        let folded = self.fold_marker.is_some()
            && !self.in_folded
            && !is_php_literal(expr)
            && self.php.eval_const(expr).is_some();
        if folded {
            self.in_folded = true;
        }
        if !folds_children(expr) {
            self.fold_marker = None;
        }
        let result = self.expr_kind(expr);
        self.fold_marker = saved_fold;
        self.in_folded = saved_in_folded;
        // a value of the parser (a chosen branch included) is where its compilation left the line state
        if folded && !self.php.keeps_parsed_line(expr) {
            self.folded();
        }
        self.own_fold = saved_own;
        result
    }

    fn expr_kind(&mut self, expr: &Expr) -> Result<()> {
        match &expr.kind {
            ExprKind::Constant(constant) => {
                // ConstantExpression::compile(): `Compiler::repr()`, a literal or a constant expression PHP folds
                if constant.defined_test {
                    self.emit_const(Value::Bool(true));
                } else {
                    self.emit_const(const_value(&constant.value));
                }
                if !is_php_literal(expr) {
                    self.folded();
                }
                Ok(())
            }
            ExprKind::ContextVariable(name) => {
                self.name(expr, name);
                Ok(())
            }
            ExprKind::AssignContextVariable(name) => {
                // AssignContextVariable::compile(): `$context["name"]`
                let id = self.name_constant(&name.name);
                self.emit(Instr::LoadName {
                    name: id,
                    mode: NameMode::AlwaysDefined,
                });
                Ok(())
            }
            ExprKind::LocalVariable(name) => {
                let slot = self.named_local(name);
                self.emit(Instr::LoadLocal(slot));
                Ok(())
            }
            ExprKind::MacroVariable(_) => {
                // MacroVariable::compile()
                match self.macro_slot(expr)? {
                    MacroSlot::This => self.emit(Instr::SelfMacroNamespace),
                    MacroSlot::Key(key) => {
                        let name = self.name_constant(&key);
                        self.emit(Instr::MacroNamespace {
                            name,
                            line: expr.line,
                        })
                    }
                };
                Ok(())
            }
            ExprKind::AssignMacroVariable(_) => Err(Error::logic(
                "A \"Twig\\Node\\Expression\\Variable\\AssignMacroVariable\" only compiles as the target of an import.",
            )),
            ExprKind::Array(array) => self.array(expr, array),
            ExprKind::ArrowFunction(arrow) => self.arrow(expr, arrow),
            ExprKind::List(_) => Err(Error::logic(
                "A \"Twig\\Node\\Expression\\ListExpression\" only compiles as arrow function parameters.",
            )),
            ExprKind::Unary(unary) => {
                // AbstractUnary::compile()
                match unary.op {
                    UnaryOp::Spread => self.expr(&unary.node),
                    op => {
                        self.expr(&unary.node)?;
                        self.emit(match op {
                            UnaryOp::Neg => Instr::Neg,
                            UnaryOp::Pos => Instr::Pos,
                            UnaryOp::Not => Instr::Not,
                            UnaryOp::StringCast | UnaryOp::Spread => Instr::StringCast,
                        });
                        Ok(())
                    }
                }
            }
            ExprKind::Binary(binary) => self.binary(expr, binary),
            ExprKind::ConditionalTernary(ternary) => {
                // ConditionalTernary::compile(): ((test) ? (left) : (right))
                self.conditional(&ternary.test, &ternary.left, &ternary.right, None)
            }
            ExprKind::GetAttr(get_attr) => self.get_attr(expr, get_attr),
            ExprKind::MacroReference(reference) => {
                // MacroReferenceExpression::compile(): `<template>->call(<name>, <arguments>, $context, line,
                // $this->source)` or `<template>->has(<name>, $context)`
                let start = self.marker;
                self.expr(&reference.template)?;
                match &reference.name {
                    MacroName::Static(name) => {
                        self.emit_const(Value::String(PhpString::from(name.as_slice())));
                    }
                    MacroName::Dynamic(name) => {
                        // `(string) <name>`
                        self.expr(name)?;
                        self.emit(Instr::StringCast);
                    }
                }
                if reference.defined_test {
                    self.emit_at(Instr::MacroDefined, start);
                } else {
                    self.expr(&reference.arguments)?;
                    self.emit_at(Instr::MacroCall { line: expr.line }, start);
                }
                // `, $context)` / `, $context, line, $this->source)`
                self.leaf();
                Ok(())
            }
            ExprKind::BlockReference(block) => {
                // BlockReferenceExpression::compile(): `[yield from ]<target>->unwrap()->yieldBlock|renderBlock|hasBlock(
                // <name>, $context[, $blocks])` where the target is `$this` or `$this->load(<template>, line)`; a
                // spread name or template is followed by positional arguments, which PHP refuses to compile
                if block.output && !block.defined_test {
                    self.add_debug_info(expr.line);
                }
                let mut start = self.marker;
                if let Some(template) = &block.template {
                    self.expr(template)?;
                    self.positional_after(template);
                    self.emit_at(
                        Instr::LoadTemplate {
                            line: expr.line,
                            index: None,
                        },
                        start,
                    );
                    // `->unwrap()->renderBlock(` follows the template expression: PHP attributes a method call to
                    // the line of its name
                    start = self.marker;
                }
                self.expr(&block.name)?;
                self.positional_after(&block.name);
                if block.template.is_none() {
                    // `, $context, $blocks)`
                    self.leaf();
                    self.emit(Instr::CheckBlocks);
                }
                let op = if block.defined_test {
                    BlockOp::Has
                } else if block.output {
                    BlockOp::Yield
                } else {
                    BlockOp::Render
                };
                let block = Instr::Block {
                    op,
                    template: block.template.is_some(),
                };
                if op == BlockOp::Yield {
                    // the body of the yieldBlock() generator runs on `yield from`, at the end of the statement
                    self.leaf();
                    self.emit(block);
                } else {
                    self.emit_at(block, start);
                }
                // `, $context[, $blocks])`
                self.leaf();
                Ok(())
            }
            ExprKind::Parent(parent) => {
                // ParentExpression::compile(): `yield from $this->yieldParentBlock("name", $context, $blocks);` or
                // `$this->renderParentBlock("name", $context, $blocks)`
                if parent.output {
                    self.add_debug_info(expr.line);
                }
                let name = self.name_constant(&parent.name);
                self.leaf();
                self.emit(Instr::ParentBlock {
                    name,
                    render: !parent.output,
                });
                Ok(())
            }
            ExprKind::Filter(filter) => self.filter(expr, filter),
            ExprKind::Function(function) => self.function(expr, function),
            ExprKind::Test(test) => self.test(expr, test),
            ExprKind::Empty => {
                // EmptyExpression::compile() writes nothing; only destructuring patterns contain it
                self.emit_const(Value::Null);
                Ok(())
            }
            ExprKind::CheckToString(check) => {
                // CheckToStringNode::compile(): `$this->sandbox->ensureToStringAllowed(expr, line, $this->source)`
                let start = self.marker;
                self.expr(&check.expr)?;
                self.emit_at(
                    Instr::CheckToString {
                        spread: check.spread,
                        line: check.expr.line,
                    },
                    start,
                );
                self.leaf();
                Ok(())
            }
            ExprKind::Custom(custom) => {
                // Twig: the expression's own `compile()`
                let mut compiler = Compiler::new(self, custom.class_name(), Produces::Value);
                custom.compile(&mut compiler)?;
                compiler.finish()
            }
        }
    }

    /// `((test) ? (left) : (right))` (ConditionalTernary, ElvisBinary, NullCoalesceBinary); `share` names the
    /// operand of the null test inside `test` and the node it is the same PHP object as (`NullCoalesceBinary`
    /// passes its `$left` to the `NullTest`).
    ///
    /// Inside an array literal, `zend_eval_const_expr()` replaces a conditional whose test folds by the branch the
    /// test selects, so PHP's line state is the one that branch leaves (also when the branch is a constant: a value
    /// of the parser keeps its own line), and PHP never compiles the other branch.
    fn conditional(
        &mut self,
        test: &Expr,
        left: &Expr,
        right: &Expr,
        share: Option<(&Expr, &Expr)>,
    ) -> Result<()> {
        let choice = if self.folding() {
            self.php.eval_const(test).map(|t| php::truthy(&t))
        } else {
            None
        };
        if let Some((copy, original)) = share {
            self.alias_tree(copy, original);
        }
        self.expr(test)?;
        let otherwise = self.emit(Instr::JumpIfFalse(0));
        self.dead_if(choice == Some(false), |this| this.expr(left))?;
        let left_marker = self.php_marker;
        let end = self.emit(Instr::Jump(0));
        self.patch_here(otherwise);
        self.dead_if(choice == Some(true), |this| this.expr(right))?;
        self.patch_here(end);
        if choice == Some(true) {
            self.php_marker = left_marker;
        }
        Ok(())
    }

    /// Twig: `NameExpression::compile()`.
    fn name(&mut self, expr: &Expr, name: &ast::NameExpr) {
        self.add_debug_info(expr.line);
        let special = matches!(name.name.as_slice(), b"_self" | b"_context" | b"_charset");
        if name.defined_test {
            if special || expr.always_defined {
                // `true`
                self.emit_const(Value::Bool(true));
                self.folded();
            } else {
                let id = self.name_constant(&name.name);
                self.emit(Instr::LoadName {
                    name: id,
                    mode: NameMode::Defined,
                });
            }
            return;
        }
        if special {
            self.emit(match name.name.as_slice() {
                b"_self" => Instr::LoadTemplateName,
                b"_context" => Instr::LoadContext,
                _ => Instr::LoadCharset,
            });
            return;
        }
        let mode = if expr.always_defined {
            NameMode::AlwaysDefined
        } else if name.ignore_strict_check
            || self.strict_ignored.contains(&self.identity(expr_key(expr)))
            || !self.options.strict_variables
        {
            NameMode::Lenient
        } else {
            NameMode::Strict { line: expr.line }
        };
        let id = self.name_constant(&name.name);
        self.emit(Instr::LoadName { name: id, mode });
        if mode == NameMode::Lenient {
            // `($context["name"] ?? null)` ends with `null`
            self.folded();
        }
    }

    /// Twig: `ArrayExpression::compile()` (and `VariadicExpression::compile()`, whose `...` the caller adds).
    fn array(&mut self, expr: &Expr, array: &ast::ArrayExpr) -> Result<()> {
        if array.defined_test {
            self.emit_const(Value::Bool(true));
            return Ok(());
        }
        for (_, value) in &array.elements {
            if matches!(value.kind, ExprKind::Empty) {
                return Err(Error::syntax(
                    "Empty array elements are only allowed in destructuring assignments.",
                )
                .with_line(value.line)
                .with_source(self.source()));
            }
        }
        // zend_compile_array() folds the elements of an outermost literal at the literal's line, the line of its first
        // element's value; nested literals are folded with it
        let mut fold = None;
        if self.fold_marker.is_none() {
            // zend_try_ct_eval_array() fails the compilation of a constant literal unpacking a constant that is not an
            // array: the nested literals it builds while folding the elements, then this one
            if self.php_compiles()
                && let Some(fatal) = self.php.array_eval_fatal(array)
            {
                self.php_fatal(fatal);
            }
            fold = Some(self.begin_fold(php::first_line(expr)));
        }
        let result = self.array_elements(array);
        if let Some(fold) = fold {
            self.end_fold(fold);
        }
        result
    }

    /// The elements of an `ArrayExpression`: `[v0, v1, ...]` while the keys are `0, 1, ...`, then `key => value`
    /// (a dynamic key cast with `(string)`), and `...value` for spreads.
    fn array_elements(&mut self, array: &ast::ArrayExpr) -> Result<()> {
        self.emit(Instr::NewArray);
        let mut is_sequence = true;
        for (i, (key, value)) in array.elements.iter().enumerate() {
            let key_value: Option<Value> = match &key.kind {
                ExprKind::LocalVariable(name)
                | ExprKind::MacroVariable(ast::MacroVariableExpr { name, .. }) => {
                    Some(match name {
                        TempName::Null => Value::Null,
                        TempName::Int(i) => Value::Int(*i),
                        TempName::String(s) => Value::String(PhpString::from(s.as_slice())),
                    })
                }
                ExprKind::Constant(constant) => Some(const_value(&constant.value)),
                _ => None,
            };
            // `$key !== $i`
            if !matches!(&key_value, Some(Value::Int(k)) if *k == i as i64) {
                is_sequence = false;
            }
            let spread = match &value.kind {
                ExprKind::Unary(unary) if unary.op == UnaryOp::Spread => Some(&unary.node),
                _ => None,
            };
            if let Some(spread) = spread {
                self.expr(spread)?;
                self.emit(Instr::ArraySpread);
                continue;
            }
            if is_sequence {
                self.expr(value)?;
                self.emit(Instr::ArrayAppend);
                continue;
            }
            match key_value {
                Some(key_value) => self.emit_const(key_value),
                None => {
                    // a dynamic key: StringCastUnary
                    self.expr(key)?;
                    self.emit(Instr::StringCast);
                    self.key_compiled(key);
                }
            }
            self.expr(value)?;
            self.emit(Instr::ArrayInsert);
        }
        Ok(())
    }

    /// Twig: `ArrowFunctionExpression::compile()`.
    fn arrow(&mut self, expr: &Expr, arrow: &ast::ArrowFunctionExpr) -> Result<()> {
        self.add_debug_info(expr.line);
        let start = self.marker;
        let mut params = Vec::new();
        if let ExprKind::List(list) = &arrow.names.kind {
            for item in &list.items {
                if let Some(name) = item.context_variable_name() {
                    params.push(PhpString::from(name));
                }
            }
        }
        self.begin_unit(UnitKind::Arrow(params));
        self.emit_at(Instr::Recv, start);
        let result = self.expr(&arrow.expr);
        if result.is_ok() {
            self.emit(Instr::Return);
        }
        let unit = self.finish_unit();
        result?;
        self.emit_at(Instr::MakeArrow(unit), start);
        // the closure ends with `; }`
        self.leaf();
        Ok(())
    }

    fn binary(&mut self, expr: &Expr, binary: &BinaryExpr) -> Result<()> {
        use ast::BinaryOp as B;
        let simple = |op: B| -> Option<BinaryOp> {
            Some(match op {
                B::Add => BinaryOp::Add,
                B::Sub => BinaryOp::Sub,
                B::Mul => BinaryOp::Mul,
                B::Div => BinaryOp::Div,
                B::FloorDiv => BinaryOp::FloorDiv,
                B::Mod => BinaryOp::Mod,
                B::Power => BinaryOp::Pow,
                B::Concat => BinaryOp::Concat,
                B::BitwiseAnd => BinaryOp::BitAnd,
                B::BitwiseOr => BinaryOp::BitOr,
                B::BitwiseXor => BinaryOp::BitXor,
                B::Equal => BinaryOp::Equal,
                B::NotEqual => BinaryOp::NotEqual,
                B::Less => BinaryOp::Less,
                B::Greater => BinaryOp::Greater,
                B::LessEqual => BinaryOp::LessEqual,
                B::GreaterEqual => BinaryOp::GreaterEqual,
                B::Spaceship => BinaryOp::Spaceship,
                B::SameAs => BinaryOp::Identical,
                B::NotSameAs => BinaryOp::NotIdentical,
                _ => return None,
            })
        };
        if binary.op == B::Power {
            return self.power(binary);
        }
        if let Some(op) = simple(binary.op) {
            // AbstractBinary::compile(): (left OP right); inside an array literal the operands are what
            // zend_eval_const_expr() left of them (see PhpModel::const_operand())
            let folding = self.folding();
            self.expr(&binary.left)?;
            self.expr(&binary.right)?;
            match op {
                BinaryOp::Concat => {
                    // zend_compile_binary_op() (PHPSRC/Zend/zend_compile.c, ZEND_CONCAT): a constant operand is
                    // converted while compiling, except a constant array, which gets a `ZEND_CAST` of its own (the
                    // left operand's first), run before the concatenation converts the other operand
                    if let Some(Value::Array(_)) = self.php.const_operand(&binary.left, folding) {
                        self.emit(Instr::StringCastBelow);
                    }
                    if let Some(Value::Array(_)) = self.php.const_operand(&binary.right, folding) {
                        self.emit(Instr::StringCast);
                    }
                    self.emit(Instr::Binary(op));
                }
                BinaryOp::Mul | BinaryOp::BitAnd | BinaryOp::BitOr | BinaryOp::BitXor => {
                    // ZEND_MUL and ZEND_BW_* are SPEC(COMMUTATIVE): zend_vm_set_opcode_handler_ex() swaps the
                    // operands when op1's kind is lower than op2's (IS_CONST < IS_TMP_VAR < IS_VAR < IS_CV), so the
                    // operation converts and reports the right operand first (PHPSRC/Zend/zend_vm_execute.h,
                    // zend_vm_def.h); the operands are still evaluated left to right
                    let commuted = self.php.operand_kind(&binary.left, folding)
                        < self.php.operand_kind(&binary.right, folding);
                    self.emit(if commuted {
                        Instr::CommutedBinary(op)
                    } else {
                        Instr::Binary(op)
                    });
                }
                _ => {
                    self.emit(Instr::Binary(op));
                }
            }
            return Ok(());
        }
        match binary.op {
            B::And | B::Or => {
                // (T(left) && T(right)) / (T(left) || T(right)); zend_compile_short_circuiting() does not compile the
                // right operand when a constant left operand decides (inside an array literal such an operation is
                // folded as a whole)
                let and = binary.op == B::And;
                let decided = self
                    .php
                    .compile_const(&binary.left)
                    .is_some_and(|left| php::truthy(&left) != and);
                self.expr(&binary.left)?;
                let left_marker = self.php_marker;
                let short = self.emit(if and {
                    Instr::JumpIfFalse(0)
                } else {
                    Instr::JumpIfTrue(0)
                });
                self.dead_if(decided, |this| this.expr(&binary.right))?;
                self.emit(Instr::ToBool);
                let end = self.emit(Instr::Jump(0));
                self.patch_here(short);
                self.emit_const(Value::Bool(!and));
                self.patch_here(end);
                if decided {
                    self.php_marker = left_marker;
                }
            }
            B::Xor => {
                // (T(left) xor T(right))
                self.expr(&binary.left)?;
                self.expr(&binary.right)?;
                self.emit(Instr::Binary(BinaryOp::Xor));
            }
            B::In | B::NotIn => {
                // [!]CoreExtension::inFilter(left, right)
                let start = self.marker;
                self.expr(&binary.left)?;
                self.expr(&binary.right)?;
                self.emit_at(
                    Instr::In {
                        negate: binary.op == B::NotIn,
                    },
                    start,
                );
            }
            B::Matches => {
                // CoreExtension::matches(right, left)
                let start = self.marker;
                self.expr(&binary.right)?;
                self.expr(&binary.left)?;
                self.emit_at(Instr::Matches, start);
            }
            B::StartsWith | B::EndsWith => {
                // (is_string($a = left) && is_string($b = right) && str_starts_with($a, $b))
                let a = self.local();
                let b = self.local();
                self.twig_var();
                self.twig_var();
                self.expr(&binary.left)?;
                self.emit(Instr::StoreLocal(a));
                self.emit(Instr::LoadLocal(a));
                self.emit(Instr::IsString);
                let fail_a = self.emit(Instr::JumpIfFalse(0));
                self.expr(&binary.right)?;
                self.emit(Instr::StoreLocal(b));
                self.emit(Instr::LoadLocal(b));
                self.emit(Instr::IsString);
                let fail_b = self.emit(Instr::JumpIfFalse(0));
                self.emit(Instr::LoadLocal(a));
                self.emit(Instr::LoadLocal(b));
                self.emit(if binary.op == B::StartsWith {
                    Instr::StartsWith
                } else {
                    Instr::EndsWith
                });
                let end = self.emit(Instr::Jump(0));
                self.patch_here(fail_a);
                self.patch_here(fail_b);
                self.emit_const(Value::Bool(false));
                self.patch_here(end);
                // `str_starts_with($a, $b))`
                self.leaf();
            }
            B::HasSome | B::HasEvery => {
                // CoreExtension::arraySome($this->env, left, right, <isSandboxed>)
                let start = self.marker;
                self.expr(&binary.left)?;
                self.expr(&binary.right)?;
                self.emit_at(
                    if binary.op == B::HasSome {
                        Instr::HasSome
                    } else {
                        Instr::HasEvery
                    },
                    start,
                );
                // the `<isSandboxed>` literal
                self.leaf();
            }
            B::Range => {
                // range(left, right)
                let start = self.marker;
                self.expr(&binary.left)?;
                self.expr(&binary.right)?;
                self.emit_at(Instr::Range, start);
            }
            B::Elvis => {
                // ((test) ? (left) : (right)), test = TrueTest::wrap(clone left)
                let test = binary
                    .test
                    .as_ref()
                    .ok_or_else(|| missing_node("test", &expr.class_name()))?;
                self.conditional(test, &binary.left, &binary.right, None)?;
            }
            B::NullCoalesce => {
                // ((test) ? (left) : (right)); the null test of `test` shares its operand with `left`
                let test = binary
                    .test
                    .as_ref()
                    .ok_or_else(|| missing_node("test", &expr.class_name()))?;
                let share = null_test_operand(test).map(|shared| (shared, &binary.left));
                self.conditional(test, &binary.left, &binary.right, share)?;
            }
            B::Set => {
                // ($context["name"] = right)
                self.expr(&binary.right)?;
                self.emit(Instr::Dup);
                let name = binary.left.context_variable_name().unwrap_or_default();
                let id = self.name_constant(name);
                self.emit(Instr::StoreName(id));
            }
            B::SequenceDestructuringSet => {
                // SequenceDestructuringSetBinary::compile()
                self.add_debug_info(expr.line);
                self.twig_var();
                let mut names = Array::new();
                if let ExprKind::Array(array) = &binary.left.kind {
                    for (_, value) in &array.elements {
                        let _ = names.push(match value.context_variable_name() {
                            Some(name) => Value::String(PhpString::from(name)),
                            None => Value::Null,
                        });
                    }
                }
                let names = self.constant(Value::Array(names));
                self.expr(&binary.right)?;
                // The destructuring assignment is a statement of its own after the right operand: PHP reports its
                // warnings at the marker in effect there, not at a folded literal's line.
                self.leaf();
                self.emit(Instr::SeqDestructure(names));
                self.leaf();
            }
            B::ObjectDestructuringSet => {
                // ObjectDestructuringSetBinary::compile()
                self.add_debug_info(expr.line);
                self.twig_var();
                let mut mappings: Vec<(Value, Vec<u8>)> = Vec::new();
                if let ExprKind::Array(array) = &binary.left.kind {
                    for (key, value) in &array.elements {
                        let property = key.as_constant().map_or(Value::Null, const_value);
                        let variable = value.context_variable_name().unwrap_or_default().to_vec();
                        mappings.push((property, variable));
                    }
                }
                let object = self.local();
                let empty_arguments = self.constant(Value::Array(Array::new()));
                self.emit(Instr::NewArray);
                for (i, (property, _)) in mappings.iter().enumerate() {
                    let start = self.marker;
                    if i == 0 {
                        self.expr(&binary.right)?;
                        self.emit(Instr::Dup);
                        self.emit(Instr::StoreLocal(object));
                    } else {
                        self.emit(Instr::LoadLocal(object));
                    }
                    self.emit_const(property.clone());
                    self.emit(Instr::Const(empty_arguments));
                    self.emit_at(
                        Instr::GetAttr(GetAttrOp {
                            call_type: CallType::Any,
                            defined_test: false,
                            ignore_strict_check: false,
                            sandboxed: self.options.sandboxed,
                            line: binary.right.line,
                        }),
                        start,
                    );
                    self.emit(Instr::ArrayAppend);
                }
                let mut names = Array::new();
                for (_, variable) in &mappings {
                    let _ = names.push(Value::String(PhpString::from(variable.as_slice())));
                }
                let names = self.constant(Value::Array(names));
                self.emit(Instr::ListAssign(names));
                self.leaf();
            }
            _ => unreachable!("every simple operator is handled above"),
        }
        Ok(())
    }

    /// Twig: `PowerBinary` (`AbstractBinary::compile()`: `(left ** right)`).
    ///
    /// The PHP of the left operand may begin with prefix operators, which bind more loosely than `**` in PHP, so PHP
    /// applies the power to what follows them and the prefixes to the power: an unparenthesized unary ` -x`, ` +x`
    /// or ` !x` (`AbstractUnary::compile()` writes parentheses only for `with_parentheses`), `(int) floor((a / b))`
    /// (`FloorDivBinary::compile()`, whatever its parentheses) and `!CoreExtension::inFilter(a, b)`
    /// (`NotInBinary::compile()`): `-x ** y` is `-(x ** y)`, `(a // b) ** y` is `(int) (floor(a / b) ** y)` and
    /// `(a not in b) ** y` is `!(inFilter(a, b) ** y)`. Chains combine, the first prefix applied last.
    fn power(&mut self, binary: &BinaryExpr) -> Result<()> {
        enum Core<'a> {
            Plain(&'a Expr),
            Floor(&'a BinaryExpr),
            In(&'a BinaryExpr),
        }
        let mut prefixes = Vec::new();
        let mut left = &binary.left;
        let core = loop {
            match &left.kind {
                ExprKind::Unary(unary)
                    if !left.with_parentheses
                        && matches!(unary.op, UnaryOp::Neg | UnaryOp::Pos | UnaryOp::Not) =>
                {
                    prefixes.push(match unary.op {
                        UnaryOp::Neg => Instr::Neg,
                        UnaryOp::Pos => Instr::Pos,
                        _ => Instr::Not,
                    });
                    left = &unary.node;
                }
                ExprKind::Binary(inner) if inner.op == ast::BinaryOp::FloorDiv => {
                    prefixes.push(Instr::IntCast);
                    break Core::Floor(inner);
                }
                ExprKind::Binary(inner) if inner.op == ast::BinaryOp::NotIn => {
                    prefixes.push(Instr::Not);
                    break Core::In(inner);
                }
                _ => break Core::Plain(left),
            }
        };
        match core {
            Core::Plain(left) => self.expr(left)?,
            Core::Floor(inner) => {
                // `floor((a / b))`: the operands are arguments of a call
                self.without_fold(|this| {
                    this.expr(&inner.left)?;
                    this.expr(&inner.right)
                })?;
                self.emit(Instr::Binary(BinaryOp::FloorOfDiv));
            }
            Core::In(inner) => {
                // `CoreExtension::inFilter(a, b)`
                let start = self.marker;
                self.without_fold(|this| {
                    this.expr(&inner.left)?;
                    this.expr(&inner.right)
                })?;
                self.emit_at(Instr::In { negate: false }, start);
            }
        }
        self.expr(&binary.right)?;
        self.emit(Instr::Binary(BinaryOp::Pow));
        for prefix in prefixes.into_iter().rev() {
            self.emit(prefix);
        }
        Ok(())
    }

    /// Twig: `GetAttrExpression::compile()`.
    fn get_attr(&mut self, expr: &Expr, get_attr: &GetAttrExpr) -> Result<()> {
        // a GetAttrExpression compiled with `ignore_strict_check` sets it on its node object
        let ignore_strict_check = get_attr.ignore_strict_check
            || self.strict_ignored.contains(&self.identity(expr_key(expr)));
        let mut sandbox_end = None;

        // optimize array calls
        if get_attr.optimizable
            && (!self.options.strict_variables || ignore_strict_check)
            && !get_attr.defined_test
            && get_attr.call_type == CallType::Array
        {
            // `(($_vN = node) && is_array($_vN) || $_vN instanceof ArrayAccess ? ($_vN[key] ?? null) : null)`
            let var = self.local();
            self.twig_var();
            let start = self.marker;
            self.expr(&get_attr.node)?;
            self.emit(Instr::StoreLocal(var));
            self.emit(Instr::LoadLocal(var));
            // zend_compile_assign() leaves `CG(zend_lineno)` at the line of `$_vN`, where `&&` converts the value
            self.emit_at(
                Instr::ArrayLike {
                    sandboxed: self.options.sandboxed,
                },
                start,
            );
            self.leaf();
            let otherwise = self.emit(Instr::JumpIfFalse(0));
            self.emit(Instr::LoadLocal(var));
            match &get_attr.attribute.kind {
                ExprKind::Constant(constant) => self.emit_const(const_value(&constant.value)),
                _ => {
                    // compileArrayKey(): `(($_vM = attribute) instanceof \Stringable && ...)`
                    self.twig_var();
                    self.expr(&get_attr.attribute)?;
                    self.emit(Instr::StringableKey { container: var });
                }
            }
            // after the key (a literal or `$_vM`)
            self.leaf();
            self.emit(Instr::DimIsset);
            let end = self.emit(Instr::Jump(0));
            self.patch_here(otherwise);
            if !self.options.sandboxed {
                // `: null)`
                self.emit_const(Value::Null);
                self.folded();
                self.patch_here(end);
                return Ok(());
            }
            sandbox_end = Some(end);
        }

        if ignore_strict_check {
            let node = self.identity(expr_key(&get_attr.node));
            self.strict_ignored.insert(node);
        }

        // the null-safe node: this one, or the nearest null-safe node of the object chain
        let mut null_safe_node: Option<(&Expr, &GetAttrExpr)> = None;
        if get_attr.null_safe {
            null_safe_node = Some((expr, get_attr));
        } else {
            let mut node = &get_attr.node;
            while let ExprKind::GetAttr(inner) = &node.kind {
                if inner.null_safe {
                    null_safe_node = Some((node, inner));
                    break;
                }
                node = &inner.node;
            }
        }
        let mut short_circuit_end = None;
        if let Some((ns_expr, ns)) = null_safe_node
            && !self.null_safe_state(ns_expr).short_circuited
        {
            // ((null === ($_vK = <ns.node>)) ? null : ...): written before `ns` compiles itself, so the node of a
            // deeper `ns` has only the attributes it was given so far
            let (var, _) = self.null_safe_var(ns_expr);
            self.expr(&ns.node)?;
            self.emit(Instr::StoreLocal(var));
            self.emit(Instr::LoadLocal(var));
            self.emit(Instr::IsNull);
            let go = self.emit(Instr::JumpIfFalse(0));
            self.emit_const(Value::Null);
            short_circuit_end = Some(self.emit(Instr::Jump(0)));
            self.patch_here(go);
            self.mark_short_circuited(ns_expr);
        }

        // `CoreExtension::getAttribute($this->env, $this->source, node, attribute, arguments, type, ...)`: positional
        // arguments follow a spread child (`attribute(x, ...y)`), which zend_compile_args() refuses when it reaches
        // the first of them, once the spread's own expression is compiled
        let start = self.marker;
        if get_attr.null_safe {
            // `$_vK`, unassigned when the code that assigns it did not run (a key compiled twice under the
            // SandboxExtension, whose first copy sits in a branch that was not taken)
            let (var, n) = self.null_safe_var(expr);
            self.leaf();
            self.emit(Instr::LoadVar {
                local: var,
                name: n,
            });
        } else {
            self.expr(&get_attr.node)?;
            self.positional_after(&get_attr.node);
        }
        self.expr(&get_attr.attribute)?;
        self.positional_after(&get_attr.attribute);
        match &get_attr.arguments {
            Some(arguments) => {
                self.expr(arguments)?;
                self.positional_after(arguments);
            }
            None => self.emit_const(Value::Array(Array::new())),
        }
        self.emit_at(
            Instr::GetAttr(GetAttrOp {
                call_type: get_attr.call_type,
                defined_test: get_attr.defined_test,
                ignore_strict_check,
                sandboxed: self.options.sandboxed,
                line: get_attr.node.line,
            }),
            start,
        );
        if let Some(end) = sandbox_end {
            self.patch_here(end);
        }
        if let Some(end) = short_circuit_end {
            self.patch_here(end);
        }
        // `, line)` of the getAttribute() call
        self.leaf();
        Ok(())
    }

    /// The identity of a node: its key (see [`expr_key()`]), or the key of the node it is the same PHP object as.
    fn identity(&self, key: usize) -> usize {
        self.aliases.get(&key).copied().unwrap_or(key)
    }

    fn null_safe_state(&self, node: &Expr) -> NullSafeState {
        self.null_safe
            .get(&self.identity(expr_key(node)))
            .copied()
            .unwrap_or(NullSafeState {
                short_circuited: false,
                var: None,
            })
    }

    /// Twig: `GetAttrExpression::getVarName()`: the variable is named once per node (`Compiler::getVarName()`).
    fn null_safe_var(&mut self, node: &Expr) -> (LocalId, u32) {
        let key = self.identity(expr_key(node));
        if let Some(var) = self.null_safe.get(&key).and_then(|s| s.var) {
            return var;
        }
        let var = (self.local(), self.twig_var());
        let state = self.null_safe.entry(key).or_insert(NullSafeState {
            short_circuited: false,
            var: None,
        });
        state.var = Some(var);
        var
    }

    fn mark_short_circuited(&mut self, node: &Expr) {
        let key = self.identity(expr_key(node));
        self.null_safe
            .entry(key)
            .or_insert(NullSafeState {
                short_circuited: false,
                var: None,
            })
            .short_circuited = true;
    }

    /// Twig: `Compiler::getVarName()`: the `N` of a new `$_vN`.
    pub(super) fn twig_var(&mut self) -> u32 {
        let n = self.twig_vars;
        self.twig_vars = self.twig_vars.wrapping_add(1);
        n
    }

    /// Records that the nodes of `copy` are the PHP objects of the corresponding nodes of `original`: twig-rs
    /// copied a subtree that Twig shares, so compiling one must change the state of the other. A wrapper a visitor
    /// put around the original (`CheckToStringNode`, the escaper's escape filter) is skipped: its node is the shared
    /// object.
    fn alias_tree(&mut self, copy: &Expr, original: &Expr) {
        let mut original = original;
        while !same_node_class(copy, original) {
            original = match &original.kind {
                ExprKind::CheckToString(check) => &check.expr,
                ExprKind::Filter(filter) if matches!(filter.kind, FilterKind::Escape { .. }) => {
                    &filter.node
                }
                _ => return,
            };
        }
        let canonical = self.identity(expr_key(original));
        let key = expr_key(copy);
        if key != canonical {
            self.aliases.insert(key, canonical);
        }
        let copies = copy.children();
        let originals = original.children();
        for ((_, c), (_, o)) in copies.iter().zip(originals.iter()) {
            match (c, o) {
                (NodeRef::Expr(c), NodeRef::Expr(o)) => self.alias_tree(c, o),
                (NodeRef::Arguments(c), NodeRef::Arguments(o)) => {
                    for ((_, c), (_, o)) in c.entries().iter().zip(o.entries().iter()) {
                        self.alias_tree(c, o);
                    }
                }
                _ => {}
            }
        }
    }

    /// Twig: `FilterExpression::compile()` and the special filter classes.
    fn filter(&mut self, expr: &Expr, filter: &ast::FilterExpr) -> Result<()> {
        match filter.kind {
            // DefaultFilter::compile() and RawFilter::compile(): the node only
            FilterKind::Default | FilterKind::Raw => self.expr(&filter.node),
            FilterKind::Filter if filter.filter.name() == b"raw" => {
                self.deprecations.push(Deprecation::twig(
                    "3.11",
                    "Creating the \"raw\" filter via \"FilterExpression\" is deprecated; use \"RawFilter\" instead.",
                ));
                self.expr(&filter.node)
            }
            FilterKind::Filter | FilterKind::Escape { .. } => self.call(
                &TwigCallable::Filter(filter.filter.clone()),
                Some(&filter.node),
                Some(&filter.arguments),
                expr.line,
            ),
        }
    }

    /// Twig: `FunctionExpression::compile()`.
    fn function(&mut self, expr: &Expr, function: &ast::FunctionExpr) -> Result<()> {
        if function.kind == FunctionKind::RenderSandboxed {
            return self.render_sandboxed(expr, function);
        }
        if matches!(function.kind, FunctionKind::Enum | FunctionKind::EnumCases)
            && let Some(inlined) = self.enum_function(expr, function)?
        {
            self.emit_const(inlined);
            return Ok(());
        }
        if function.function.name() == b"constant" && function.defined_test {
            let mut arguments = function.arguments.clone();
            arguments.set_named(b"checkDefined".to_vec(), Expr::constant(true, expr.line));
            let result = self.call(
                &TwigCallable::Function(function.function.clone()),
                None,
                Some(&arguments),
                expr.line,
            );
            self.temporaries
                .push(Temporary::Arguments(Box::new(arguments)));
            return result;
        }
        self.call(
            &TwigCallable::Function(function.function.clone()),
            None,
            Some(&function.arguments),
            expr.line,
        )
    }

    /// Twig: `TestExpression::compile()` and the special test classes.
    fn test(&mut self, expr: &Expr, test: &ast::TestExpr) -> Result<()> {
        let argument = |i: i64| -> Result<&Expr> {
            let Some(arguments) = &test.arguments else {
                return Err(missing_node("arguments", test.kind.class_name()));
            };
            arguments
                .get(&ast::ArgumentKey::Index(i))
                .ok_or_else(|| missing_node(&i.to_string(), arguments.class_name()))
        };
        match test.kind {
            TestKind::Test | TestKind::NonStringCoercingInput => self.call(
                &TwigCallable::Test(test.test.clone()),
                Some(&test.node),
                test.arguments.as_ref(),
                expr.line,
            ),
            TestKind::Defined => self.expr(&test.node),
            TestKind::Null => {
                self.expr(&test.node)?;
                self.emit(Instr::IsNull);
                Ok(())
            }
            TestKind::SameAs => {
                self.expr(&test.node)?;
                self.expr(argument(0)?)?;
                self.emit(Instr::Binary(BinaryOp::Identical));
                Ok(())
            }
            TestKind::Even | TestKind::Odd => {
                self.expr(&test.node)?;
                self.emit_const(Value::Int(2));
                self.emit(Instr::Binary(BinaryOp::Mod));
                self.emit_const(Value::Int(0));
                self.emit(Instr::Binary(if test.kind == TestKind::Even {
                    BinaryOp::Equal
                } else {
                    BinaryOp::NotEqual
                }));
                Ok(())
            }
            TestKind::DivisibleBy => {
                // (0 == node % arguments[0])
                self.emit_const(Value::Int(0));
                self.expr(&test.node)?;
                self.expr(argument(0)?)?;
                self.emit(Instr::Binary(BinaryOp::Mod));
                self.emit(Instr::Binary(BinaryOp::Equal));
                Ok(())
            }
            TestKind::Constant => {
                // (node === constant([get_class(arguments[1])."::".]arguments[0]))
                self.expr(&test.node)?;
                let start = self.marker;
                let with_object = test
                    .arguments
                    .as_ref()
                    .is_some_and(|a| a.get(&ast::ArgumentKey::Index(1)).is_some());
                // the arguments of constant() and get_class() are not part of an enclosing literal's folding
                let name = argument(0)?;
                let spread = match &name.kind {
                    ExprKind::Unary(unary) if unary.op == UnaryOp::Spread && !with_object => {
                        Some(&unary.node)
                    }
                    _ => None,
                };
                self.without_fold(|this| {
                    if with_object {
                        this.expr(argument(1)?)?;
                    }
                    // `constant(...value)` (valid PHP) unpacks the value into constant()'s arguments
                    this.expr(spread.unwrap_or(name))
                })?;
                self.emit_at(
                    Instr::PhpConstant {
                        with_object,
                        spread: spread.is_some(),
                    },
                    start,
                );
                self.emit(Instr::Binary(BinaryOp::Identical));
                Ok(())
            }
            TestKind::True => {
                // TrueTest::compile(): `(($tmp = node) && $tmp instanceof Markup ? (string) $tmp : $tmp)`.
                // zend_compile_assign() sets `CG(zend_lineno)` back to the line of `$tmp` after compiling the value
                // (PHPSRC/Zend/zend_compile.c), so the `JMPZ_EX` of `&&`, which converts the value to bool (PHP 8.5's
                // NAN warning), reports the line `$tmp` is written on: the marker in effect before the node's own
                let start = self.marker;
                self.expr(&test.node)?;
                self.emit_at(Instr::TrueTest, start);
                // `: $tmp)`
                self.leaf();
                Ok(())
            }
        }
    }

    /// Twig: `EnumFunction::compile()` and `EnumCasesFunction::compile()`
    /// (TWIG/src/Node/Expression/FunctionNode/EnumFunction.php, EnumCasesFunction.php): with exactly one argument (named
    /// `enum` or the first positional one) that is a constant, the name is checked while compiling (`is_string()`,
    /// `enum_exists()`, and for `enum` a non-empty `$value::cases()`) and the call is replaced by the case
    /// (`Enum::FIRST_CASE`) or the cases (`Enum::cases()`), returned here as the constant they evaluate to; `None`
    /// compiles the regular function call (`parent::compile()`).
    ///
    /// # Errors
    ///
    /// The `SyntaxError`s `The first argument of the "%s" function must be a string.`, `... must be the name of an
    /// enum, "%s" given.` and `... must be a non-empty enum, "%s" given.`, at the line of the call.
    fn enum_function(
        &mut self,
        expr: &Expr,
        function: &ast::FunctionExpr,
    ) -> Result<Option<Value>> {
        let arguments = &function.arguments;
        let first = arguments
            .get(&ArgumentKey::Name(b"enum".to_vec()))
            .or_else(|| arguments.get(&ArgumentKey::Index(0)));
        let Some(first) = first else {
            return Ok(None);
        };
        let ExprKind::Constant(constant) = &first.kind else {
            return Ok(None);
        };
        if arguments.len() != 1 {
            return Ok(None);
        }
        let cases = function.kind == FunctionKind::EnumCases;
        let name = if cases { "enum_cases" } else { "enum" };
        let ConstantValue::String(value) = &constant.value else {
            return Err(Error::syntax(format!(
                "The first argument of the \"{name}\" function must be a string."
            ))
            .with_line(expr.line)
            .with_source(self.source()));
        };
        let class = self
            .options
            .enums
            .as_ref()
            .and_then(|resolve| resolve(value));
        let Some(class) = class else {
            return Err(Error::syntax(format!(
                "The first argument of the \"{name}\" function must be the name of an enum, \"{}\" given.",
                String::from_utf8_lossy(value)
            ))
            .with_line(expr.line)
            .with_source(self.source()));
        };
        let all = class.enum_cases();
        if cases {
            // `$value::cases()`
            return Ok(Some(Value::Array(Array::from_values(
                all.iter().cloned().map(Value::Object),
            ))));
        }
        match all.first() {
            // `$value::FIRST_CASE`
            Some(case) => Ok(Some(Value::Object(case.clone()))),
            None => Err(Error::syntax(format!(
                "The first argument of the \"enum\" function must be a non-empty enum, \"{}\" given.",
                String::from_utf8_lossy(value)
            ))
            .with_line(expr.line)
            .with_source(self.source())),
        }
    }

    /// Twig: `RenderSandboxedFunction::compile()`: the literal `output_strategy` argument (position 2 or a name that
    /// normalises to `outputstrategy`) is checked and removed, the rest compiles as a plain function call.
    fn render_sandboxed(&mut self, expr: &Expr, function: &ast::FunctionExpr) -> Result<()> {
        let strategy_key = function
            .arguments
            .entries()
            .iter()
            .map(|(key, _)| key)
            .find(|key| match key {
                ArgumentKey::Index(i) => *i == 2,
                ArgumentKey::Name(name) => {
                    let normalized: Vec<u8> = name
                        .iter()
                        .filter(|&&b| b != b'_')
                        .map(u8::to_ascii_lowercase)
                        .collect();
                    normalized == b"outputstrategy"
                }
            })
            .cloned();
        let Some(strategy_key) = strategy_key else {
            return Err(Error::syntax(
                "Value for argument \"output_strategy\" is required for function \"render_sandboxed\".",
            )
            .with_line(expr.line)
            .with_source(self.source()));
        };
        let mut arguments = function.arguments.clone();
        let strategy = arguments
            .remove(&strategy_key)
            .ok_or_else(|| missing_node("output_strategy", "Twig\\Node\\Nodes"))?;
        let valid = matches!(
            &strategy.kind,
            ExprKind::Constant(constant)
                if matches!(&constant.value, ConstantValue::String(s) if !s.is_empty() && s.as_slice() != b"all")
        );
        if !valid {
            return Err(Error::syntax(
                "The \"output_strategy\" argument of the \"render_sandboxed\" function must be a non-empty literal string other than \"all\".",
            )
            .with_line(strategy.line)
            .with_source(self.source()));
        }
        let result = self.call(
            &TwigCallable::Function(function.function.clone()),
            None,
            Some(&arguments),
            expr.line,
        );
        self.temporaries
            .push(Temporary::Arguments(Box::new(arguments)));
        result
    }

    /// The `$macros` slot of a `MacroVariable`: `_self`, its name, or the name Twig's compiler gives a nameless one
    /// (`{% from %}`) the first time it compiles it (`Compiler::getVarName()`); twig-rs derives that name from the
    /// variable's identity, so the import and the calls it enables agree.
    fn macro_slot(&mut self, var: &Expr) -> Result<MacroSlot> {
        let ExprKind::MacroVariable(variable) = &var.kind else {
            return Err(Error::logic(format!(
                "A \"{}\" node cannot name a macro namespace.",
                var.class_name()
            )));
        };
        self.name_macro_var(variable);
        Ok(match &variable.name {
            TempName::String(name) if name.as_slice() == b"_self" => MacroSlot::This,
            other => MacroSlot::Key(macro_key(other, variable.id)),
        })
    }

    /// Twig: `MacroVariable::getName()`: a variable without a name is named by `Compiler::getVarName()` when it is
    /// first compiled.
    fn name_macro_var(&mut self, variable: &ast::MacroVariableExpr) {
        if variable.name == TempName::Null && self.named_macro_vars.insert(variable.id) {
            self.twig_var();
        }
    }

    /// Twig: `ImportNode::compile()`: `$macros["name"] = [$this->macros["name"] = ]` (AssignMacroVariable, with its
    /// marker), then `$this->getMacroNamespace()` for `_self` or `$this->load(<template>,
    /// line)->unwrap()->getMacroNamespace()`.
    fn import(&mut self, node: &Node, import: &ast::ImportNode) -> Result<()> {
        let ExprKind::AssignMacroVariable(assign) = &import.var.kind else {
            return Err(missing_node("var", "Twig\\Node\\ImportNode"));
        };
        let ExprKind::MacroVariable(variable) = &assign.var.kind else {
            return Err(missing_node(
                "var",
                "Twig\\Node\\Expression\\Variable\\AssignMacroVariable",
            ));
        };
        self.add_debug_info(import.var.line);
        // AssignMacroVariable::compile() names a `{% from %}` variable first
        self.name_macro_var(variable);
        let start = self.marker;
        if import.expr.context_variable_name() == Some(b"_self".as_slice()) {
            self.emit(Instr::SelfMacroNamespace);
        } else {
            self.expr(&import.expr)?;
            self.emit_at(
                Instr::LoadTemplate {
                    line: node.line,
                    index: None,
                },
                start,
            );
            self.emit(Instr::TemplateMacroNamespace);
        }
        // the assignment target keeps the variable's own name, `_self` included
        let name = self.name_constant(&macro_key(&variable.name, variable.id));
        self.emit(Instr::SetMacroNamespace {
            name,
            global: assign.global,
        });
        Ok(())
    }

    /// Twig: `IncludeNode::compile()` with `addGetTemplate()` (`EmbedNode::addGetTemplate()` for an embed) and
    /// `addTemplateArguments()`: the template is loaded, then the variables are evaluated, then the template is
    /// rendered; with `ignore missing`, a `LoaderError` while loading (an embed also resolves its parent there)
    /// skips the template.
    fn include(
        &mut self,
        line: i64,
        target: IncludeTarget<'_>,
        variables: Option<&Expr>,
        only: bool,
        ignore_missing: bool,
    ) -> Result<()> {
        self.add_debug_info(line);
        let start = self.marker;
        let var = ignore_missing.then(|| {
            self.twig_var();
            self.local()
        });
        let try_at = ignore_missing.then(|| self.emit(Instr::TryLoader { catch: 0 }));
        match target {
            IncludeTarget::Expr(expr) => {
                self.expr(expr)?;
                self.emit_at(Instr::LoadTemplate { line, index: None }, start);
            }
            IncludeTarget::Embedded { name, index } => {
                self.emit_const(Value::from(name));
                self.emit_at(
                    Instr::LoadTemplate {
                        line,
                        index: Some(index),
                    },
                    start,
                );
                if ignore_missing {
                    // `$_vN->getParent($context);`
                    self.emit(Instr::Dup);
                    self.emit(Instr::GetParent { this: false });
                    self.emit(Instr::Pop);
                }
            }
        }
        let mut skip = None;
        // PHP attributes a method call to the line of its name: `->unwrap()->yield(` follows the template
        // expression (and, with `ignore missing`, starts a statement of its own after the try/catch)
        let mut call = self.marker;
        if let (Some(var), Some(try_at)) = (var, try_at) {
            // `} catch (LoaderError $e) { $_vN = null; } if ($_vN) { yield from $_vN->unwrap()->yield(...); }`
            self.emit(Instr::EndTry);
            self.patch_here(try_at);
            self.emit(Instr::StoreLocal(var));
            self.emit(Instr::LoadLocal(var));
            skip = Some(self.emit(Instr::JumpIfFalse(0)));
            self.emit(Instr::LoadLocal(var));
            call = self.marker;
        }
        let args = match variables {
            None if only => YieldArgs::Empty,
            None => YieldArgs::Context,
            Some(variables) => {
                // `CoreExtension::merge($context, <variables>)` or `CoreExtension::toArray(<variables>)`
                let before = self.marker;
                self.expr(variables)?;
                self.emit_at(Instr::IncludeContext { merge: !only }, before);
                if only {
                    self.emit_at(Instr::CheckContext, call);
                }
                YieldArgs::Given
            }
        };
        self.emit(Instr::YieldTemplate(args));
        if let Some(skip) = skip {
            self.patch_here(skip);
        }
        Ok(())
    }

    /// Twig: `CallExpression::compileCallable()` and `compileArguments()`.
    pub(super) fn call(
        &mut self,
        callable: &TwigCallable,
        node: Option<&Expr>,
        arguments: Option<&Arguments>,
        line: i64,
    ) -> Result<()> {
        // compileCallable(): callables other than plain PHP functions are reflected first
        let plain_function = matches!(
            callable.callable().map(|c| c.kind()),
            Some(CallableKind::Function(name)) if !name.contains("::")
        );
        if !plain_function && callable.callable().is_none() {
            return Err(Error::logic(format!(
                "Callback for {} \"{}\" is not callable in the current scope.",
                callable.callable_type().as_str(),
                String::from_utf8_lossy(callable.name())
            )));
        }
        let start = self.marker;
        let index = self.callable(callable);
        self.emit(Instr::ArgsStart {
            with_context: callable.needs_context(),
        });
        if let Some(node) = node {
            self.expr(node)?;
            self.emit(Instr::Arg);
        }
        if let Some(arguments) = arguments {
            let source = self.source();
            let extractor =
                CallableArgumentsExtractor::new(callable, line, Some(&source), node.is_some())?;
            let bound = extractor.extract_arguments(arguments, &mut self.deprecations)?;
            // the extractor returns copies; Twig compiles the argument objects themselves
            self.alias_arguments(&bound, arguments);
            let result = self.bound_arguments(&bound, index);
            self.temporaries.push(Temporary::Exprs(bound));
            result?;
        }
        self.emit_at(Instr::Call(index), start);
        Ok(())
    }

    /// Compiles the arguments `CallableArgumentsExtractor` bound for the callable `callable`.
    fn bound_arguments(&mut self, bound: &[Expr], callable: u32) -> Result<()> {
        for argument in bound {
            match &argument.kind {
                ExprKind::Array(array) if array.variadic => {
                    self.expr(argument)?;
                    self.emit(Instr::ArgSpread { callable });
                }
                ExprKind::Unary(unary) if unary.op == UnaryOp::Spread => {
                    self.expr(&unary.node)?;
                    self.emit(Instr::ArgSpread { callable });
                }
                _ => {
                    self.expr(argument)?;
                    self.emit(Instr::Arg);
                }
            }
        }
        Ok(())
    }

    /// Aliases the copies `CallableArgumentsExtractor` returned to the argument nodes they copy (each to the first
    /// unused argument equal to it; the elements of a variadic array likewise).
    fn alias_arguments(&mut self, bound: &[Expr], arguments: &Arguments) {
        let mut used = vec![false; arguments.len()];
        let mut alias = |this: &mut Self, copy: &Expr| {
            let found = arguments
                .entries()
                .iter()
                .enumerate()
                .find(|(i, (_, original))| !used[*i] && *original == *copy);
            if let Some((i, (_, original))) = found {
                used[i] = true;
                this.alias_tree(copy, original);
            }
        };
        for copy in bound {
            match &copy.kind {
                ExprKind::Array(array) if array.variadic => {
                    for (_, value) in &array.elements {
                        alias(self, value);
                    }
                }
                _ => alias(self, copy),
            }
        }
    }
}

/// What an `IncludeNode` loads.
enum IncludeTarget<'a> {
    /// `{% include <expr> %}`.
    Expr(&'a Expr),
    /// `{% embed %}`: the embedded template `index` of the template `name` (the template containing the embed).
    Embedded { name: &'a str, index: i64 },
}

/// The `$macros` key of a macro variable: its name, or for a nameless one (`{% from %}`) a name derived from its
/// identity (Twig's compiler names it `_vN`; the `"\0"` prefix keeps it apart from every import alias).
fn macro_key(name: &TempName, id: u32) -> Vec<u8> {
    match name {
        TempName::String(name) => name.clone(),
        TempName::Int(i) => i.to_string().into_bytes(),
        TempName::Null => format!("\0_v{id}").into_bytes(),
    }
}

/// The parts of a program [`Codegen::module_body`] produces.
struct ModuleParts {
    body: UnitId,
    blocks: Vec<BlockDef>,
    macros: Vec<MacroDef>,
    parent: Option<ParentDef>,
    traits: Vec<TraitDef>,
    embedded: Vec<Arc<Program>>,
    traitable: bool,
}

/// Whether an expression is a `SpreadUnary` (`...expr`).
fn is_spread(expr: &Expr) -> bool {
    matches!(&expr.kind, ExprKind::Unary(unary) if unary.op == UnaryOp::Spread)
}

/// A compile-time fatal error of the generated PHP (`E_COMPILE_ERROR`): uncatchable in PHP; twig-rs fails the
/// compilation with a `CompileError` carrying PHP's message.
fn compile_fatal(fatal: php::CompileFatal) -> Error {
    Error::php(PhpError::new(ThrowableClass::CompileError, fatal.0))
}

/// The key of a node for the compile-time state Twig keeps on node objects: the address of the expression. The AST
/// is borrowed for the whole compilation and compiled copies are kept alive ([`Codegen::temporaries`]), so
/// addresses are unique.
fn expr_key(expr: &Expr) -> usize {
    std::ptr::from_ref(expr) as usize
}

/// Whether two nodes are of the same class, as far as telling a copy from a visitor's wrapper needs.
fn same_node_class(a: &Expr, b: &Expr) -> bool {
    match (&a.kind, &b.kind) {
        (ExprKind::Filter(a), ExprKind::Filter(b)) => {
            std::mem::discriminant(&a.kind) == std::mem::discriminant(&b.kind)
                && a.filter.name() == b.filter.name()
        }
        (ExprKind::Unary(a), ExprKind::Unary(b)) => a.op == b.op,
        (ExprKind::Binary(a), ExprKind::Binary(b)) => a.op == b.op,
        (ExprKind::Test(a), ExprKind::Test(b)) => a.kind == b.kind,
        (a, b) => std::mem::discriminant(a) == std::mem::discriminant(b),
    }
}

/// The operand of the null test inside a `NullCoalesceBinary` test (`defined(clone) && !(null === left)`).
fn null_test_operand(test: &Expr) -> Option<&Expr> {
    let ExprKind::Binary(and) = &test.kind else {
        return None;
    };
    if and.op != ast::BinaryOp::And {
        return None;
    }
    let ExprKind::Unary(not) = &and.right.kind else {
        return None;
    };
    if not.op != UnaryOp::Not {
        return None;
    }
    match &not.node.kind {
        ExprKind::Test(null_test) if null_test.kind == TestKind::Null => Some(&null_test.node),
        _ => None,
    }
}

/// Twig: `ModuleNode::compileIsTraitable()`: no parent, no macros, and every node of the body has no children (the
/// check counts child nodes, so text and block references are fine).
fn is_traitable(module: &ast::ModuleNode) -> bool {
    if module.parent.is_some() || module.macros.as_nodes().is_some_and(|m| !m.is_empty()) {
        return false;
    }
    let body = match &module.body.kind {
        NodeKind::Body(nodes) => match nodes.get(&NodeKey::Index(0)) {
            Some(first) => first,
            None => return true,
        },
        _ => &module.body,
    };
    let children = body.children();
    if children.is_empty() {
        // `new Nodes([$nodes])`: the node itself has no children
        return true;
    }
    children
        .iter()
        .all(|(_, child)| child.children().is_empty())
}

/// The PHP value of a constant expression's value (`Compiler::repr()`).
pub(crate) fn const_value(value: &ConstantValue) -> Value {
    match value {
        ConstantValue::Null => Value::Null,
        ConstantValue::Bool(b) => Value::Bool(*b),
        ConstantValue::Int(i) => Value::Int(*i),
        ConstantValue::Float(f) => Value::Float(*f),
        ConstantValue::String(s) => Value::String(PhpString::from(s.as_slice())),
        ConstantValue::Array(items) => {
            let mut array = Array::new();
            for (key, value) in items {
                let key = match key {
                    ast::ArrayKey::Int(i) => ArrayKey::Int(*i),
                    ast::ArrayKey::String(s) => {
                        ArrayKey::from_string(PhpString::from(s.as_slice()))
                    }
                };
                array.insert(key, const_value(value));
            }
            Value::Array(array)
        }
    }
}
