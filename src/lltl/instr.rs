//! The LLTL instruction set.
//!
//! Instructions are high level: one instruction stands for one PHP construct of the code Twig's compiler generates
//! (an operator, a `CoreExtension::getAttribute()` call, a `foreach` step, a `yield` of a string, ...). They work on
//! the operand stack of the running frame; operands that index tables (constants, callables, units, locals) are
//! `u32`/`u16` indexes into the [`Program`](super::Program) or the frame.
//!
//! Lines: an instruction carries a template line only where Twig passes an explicit line to the runtime
//! (`new RuntimeError(..., LINE, ...)`, the `$lineno` of `getAttribute()`). Every other error is attributed through
//! the unit's [`LineTable`](super::LineTable), Twig's `// line N` markers.

use crate::ast::CallType;

/// Index of a constant in the program's constant pool.
pub type ConstId = u32;
/// Index of a unit in the program.
pub type UnitId = u32;
/// Index of a local variable slot of a frame (Twig's compiler variables `$_vN`).
pub type LocalId = u16;
/// Index of an instruction in a unit.
pub type Label = u32;

/// How a context variable is read. Twig: `NameExpression::compile()` (TWIG/src/Node/Expression/NameExpression.php).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NameMode {
    /// `($context["name"] ?? null)`: non-strict mode, or the `ignore_strict_check` attribute.
    Lenient,
    /// `isset($context["name"]) || array_key_exists("name", $context) ? $context["name"] : throw new
    /// RuntimeError('Variable "name" does not exist.', line, $this->source)`: `strict_variables`.
    Strict {
        /// The line of the variable (the explicit line of the error).
        line: i64,
    },
    /// `$context["name"]` without existence check (`always_defined`): a missing key raises PHP's warning
    /// `Undefined array key "name"` and reads as `null`.
    AlwaysDefined,
    /// `array_key_exists("name", $context)` (defined test).
    Defined,
}

/// A binary operator that maps to one PHP operator. Twig: the `operator()` of the `AbstractBinary` subclasses
/// (TWIG/src/Node/Expression/Binary/), evaluated with `twig-value`'s PHP 8.5 operator semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BinaryOp {
    /// `+` (AddBinary).
    Add,
    /// `-` (SubBinary).
    Sub,
    /// `*` (MulBinary).
    Mul,
    /// `/` (DivBinary).
    Div,
    /// `(int) floor(a / b)` (FloorDivBinary).
    FloorDiv,
    /// `floor(a / b)`: FloorDivBinary as the left operand of `**`, where PHP applies the `(int)` cast to the power
    /// (`(int) floor(a / b) ** c` is `(int) (floor(a / b) ** c)`).
    FloorOfDiv,
    /// `%` (ModBinary).
    Mod,
    /// `**` (PowerBinary).
    Pow,
    /// `.` (ConcatBinary, Twig `~`).
    Concat,
    /// `&` (BitwiseAndBinary, Twig `b-and`).
    BitAnd,
    /// `|` (BitwiseOrBinary, Twig `b-or`).
    BitOr,
    /// `^` (BitwiseXorBinary, Twig `b-xor`).
    BitXor,
    /// `==` (EqualBinary).
    Equal,
    /// `!=` (NotEqualBinary).
    NotEqual,
    /// `<` (LessBinary).
    Less,
    /// `>` (GreaterBinary).
    Greater,
    /// `<=` (LessEqualBinary).
    LessEqual,
    /// `>=` (GreaterEqualBinary).
    GreaterEqual,
    /// `<=>` (SpaceshipBinary).
    Spaceship,
    /// `===` (SameAsBinary, SameasTest).
    Identical,
    /// `!==` (NotSameAsBinary).
    NotIdentical,
    /// `xor` on the truthiness of both operands (XorBinary).
    Xor,
}

impl BinaryOp {
    /// The PHP operator.
    pub fn symbol(self) -> &'static str {
        match self {
            BinaryOp::Add => "+",
            BinaryOp::Sub => "-",
            BinaryOp::Mul => "*",
            BinaryOp::Div => "/",
            BinaryOp::FloorDiv => "//",
            BinaryOp::FloorOfDiv => "floor(/)",
            BinaryOp::Mod => "%",
            BinaryOp::Pow => "**",
            BinaryOp::Concat => ".",
            BinaryOp::BitAnd => "&",
            BinaryOp::BitOr => "|",
            BinaryOp::BitXor => "^",
            BinaryOp::Equal => "==",
            BinaryOp::NotEqual => "!=",
            BinaryOp::Less => "<",
            BinaryOp::Greater => ">",
            BinaryOp::LessEqual => "<=",
            BinaryOp::GreaterEqual => ">=",
            BinaryOp::Spaceship => "<=>",
            BinaryOp::Identical => "===",
            BinaryOp::NotIdentical => "!==",
            BinaryOp::Xor => "xor",
        }
    }
}

/// The operands of a `CoreExtension::getAttribute()` call that are known at compile time. Twig:
/// `GetAttrExpression::compile()` (TWIG/src/Node/Expression/GetAttrExpression.php).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GetAttrOp {
    /// `$type`: `any`, `array` or `method`.
    pub call_type: CallType,
    /// `$isDefinedTest`.
    pub defined_test: bool,
    /// `$ignoreStrictCheck`.
    pub ignore_strict_check: bool,
    /// `$sandboxed`: whether the `SandboxExtension` was registered when the template was compiled.
    pub sandboxed: bool,
    /// `$lineno`: the line of the object expression.
    pub line: i64,
}

/// What `Template::yield()` receives (`IncludeNode::addTemplateArguments()`, `ModuleNode::compileDisplay()`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum YieldArgs {
    /// `->yield($context)`: `[handle] -> []`.
    Context,
    /// `->yield([])` (`only` without variables): `[handle] -> []`.
    Empty,
    /// `->yield(<context>)` with the context computed by [`Instr::IncludeContext`]: `[handle context] -> []`.
    Given,
    /// `->yield($context, array_merge($this->blocks, $blocks))`, the parent of `{% extends %}`: `[handle] -> []`.
    Parent,
}

/// The block method a `block()` call or a `{% block %}` placeholder calls on a template
/// (`BlockReferenceExpression::compileTemplateCall()`, `BlockReferenceNode::compile()`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BlockOp {
    /// `yield from ...->yieldBlock(...)`: writes the block (`{% block %}`, `block()` in output mode).
    Yield,
    /// `...->renderBlock(...)`: the block as a string (`block()` in an expression).
    Render,
    /// `...->hasBlock(...)`: `block() is defined`.
    Has,
}

/// One instruction. The stack effect is given as `[before] -> [after]` (top of the stack last).
///
/// New instructions may be added (fused instructions among them), so matches on instructions outside this crate need
/// a wildcard arm.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Instr {
    // ---- stack, constants, locals -------------------------------------------------------------------------------
    /// `[] -> [constant]`.
    Const(ConstId),
    /// `[v] -> []`.
    Pop,
    /// `[v] -> [v v]`.
    Dup,
    /// `[] -> [local]`.
    LoadLocal(LocalId),
    /// `$_vN` read as a variable: `[] -> [local]`, where a local nothing assigned yet raises PHP's warning
    /// `Undefined variable $_vN` and reads as `null`.
    LoadVar {
        /// The local.
        local: LocalId,
        /// The `N` of `$_vN` (`Compiler::getVarName()`).
        name: u32,
    },
    /// `[v] -> []`, stores into the local.
    StoreLocal(LocalId),

    // ---- output -------------------------------------------------------------------------------------------------
    /// `yield "<data>";` (TextNode): writes the constant string.
    Text(ConstId),
    /// `yield (string) <expr>;` (PrintNode): `[v] -> []`, PHP string conversion then write.
    Print,
    /// `flush();` (FlushNode).
    Flush,
    /// `CoreExtension::captureOutput((function () use (&$context, ...) { ... })())`: starts capturing the output of
    /// the inline body (which shares the frame's context) and saves the frame's `$macros` (captured by value).
    CaptureStart,
    /// Ends the capture started by [`Instr::CaptureStart`]: `[] -> [captured]`; with `markup`,
    /// `('' === $tmp = ...) ? '' : new Markup($tmp, $this->env->getCharset())` (CaptureNode without `raw`).
    CaptureEnd {
        /// Wrap a non-empty result in a `Markup`.
        markup: bool,
    },

    // ---- context ------------------------------------------------------------------------------------------------
    /// Reads `$context[name]`: `[] -> [value]` (`[] -> [bool]` for [`NameMode::Defined`]).
    LoadName {
        /// The constant holding the name.
        name: ConstId,
        /// How it is read.
        mode: NameMode,
    },
    /// `_self`: `$this->getTemplateName()`.
    LoadTemplateName,
    /// `_context`: the whole context array.
    LoadContext,
    /// `_charset`: `$this->env->getCharset()`.
    LoadCharset,
    /// `$context["name"] = v`: `[v] -> []`.
    StoreName(ConstId),
    /// `$context = $_vN`: `[array] -> []`.
    SetContext,

    // ---- control flow -------------------------------------------------------------------------------------------
    /// Jumps to the label.
    Jump(Label),
    /// `[v] -> []`, jumps when `v` is falsy (PHP truthiness).
    JumpIfFalse(Label),
    /// `[v] -> []`, jumps when `v` is truthy.
    JumpIfTrue(Label),
    /// Leaves the unit; arrow-function units return the top of the stack.
    Return,

    // ---- conversions, tests and operators -----------------------------------------------------------------------
    /// `[v] -> [(bool) v]`.
    ToBool,
    /// TrueTest: `(($tmp = v) && $tmp instanceof Markup ? (string) $tmp : $tmp)`.
    TrueTest,
    /// `!v` (NotUnary): `[v] -> [bool]`.
    Not,
    /// `null === v` (NullTest): `[v] -> [bool]`.
    IsNull,
    /// `[a b] -> [a op b]`.
    Binary(BinaryOp),
    /// `[a b] -> [a op b]` with PHP's operands swapped: a commutative opcode (`*`, `&`, `|`, `^`) whose op1 kind is
    /// lower than op2's runs as `b op a` (`zend_vm_set_opcode_handler_ex()`), which converts and reports `b` first.
    CommutedBinary(BinaryOp),
    /// ` -v` (NegUnary; PHP compiles it to `v * -1`).
    Neg,
    /// ` +v` (PosUnary; PHP compiles it to `v * 1`).
    Pos,
    /// ` (string)v` (StringCastUnary), or the `ZEND_CAST` PHP gives a constant array operand of `.`.
    StringCast,
    /// `[a b] -> [(string) a, b]`: the `ZEND_CAST` PHP gives a constant array left operand of `.`, run after the
    /// right operand is evaluated (`zend_compile_binary_op()`).
    StringCastBelow,
    /// `(int) v` (the cast of FloorDivBinary applied after `**`): `[v] -> [int]`.
    IntCast,
    /// `CoreExtension::inFilter(a, b)`, negated for `not in`: `[a b] -> [bool]`.
    In {
        /// `not in`.
        negate: bool,
    },
    /// `CoreExtension::matches(regexp, subject)` (the right operand is evaluated first): `[regexp subject] -> [int]`.
    Matches,
    /// `is_string(v)`: `[v] -> [bool]`.
    IsString,
    /// `str_starts_with(a, b)`: `[a b] -> [bool]`.
    StartsWith,
    /// `str_ends_with(a, b)`: `[a b] -> [bool]`.
    EndsWith,
    /// `range(a, b)` (RangeBinary): `[a b] -> [array]`.
    Range,
    /// `CoreExtension::arraySome($this->env, a, f, <isSandboxed>)`: `[a f] -> [bool]`.
    HasSome,
    /// `CoreExtension::arrayEvery($this->env, a, f, <isSandboxed>)`: `[a f] -> [bool]`.
    HasEvery,
    /// `CoreExtension::ensureTraversable(v)`: `[v] -> [v or []]`.
    EnsureTraversable,
    /// PHP's `constant(name)` or `constant(get_class(object)."::".name)` (ConstantTest): `[object? name] -> [value]`.
    PhpConstant {
        /// The class of an object argument prefixes the name.
        with_object: bool,
        /// The name is a spread argument, `constant(...value)`: the value is unpacked into `constant()`'s
        /// arguments.
        spread: bool,
    },

    // ---- arrays -------------------------------------------------------------------------------------------------
    /// `[] -> [[]]`.
    NewArray,
    /// `[array v] -> [array]`: appends (`ZEND_ADD_ARRAY_ELEMENT` without key).
    ArrayAppend,
    /// `[array key v] -> [array]`: inserts with PHP's key normalisation.
    ArrayInsert,
    /// `[array v] -> [array]`: unpacks `...v` (`ZEND_ADD_ARRAY_UNPACK`).
    ArraySpread,

    // ---- attribute access ---------------------------------------------------------------------------------------
    /// `CoreExtension::getAttribute(...)`: `[object attribute arguments] -> [value]`.
    GetAttr(GetAttrOp),
    /// The condition of the optimized `[]` access: `($v && is_array($v)) || $v instanceof ArrayAccess` (with the
    /// `SandboxExtension`: `&& in_array($v::class, CoreExtension::ARRAY_LIKE_CLASSES, true)` on the second operand).
    /// `[v] -> [bool]`.
    ArrayLike {
        /// The `SandboxExtension` was registered when the template was compiled.
        sandboxed: bool,
    },
    /// `(($k = key) instanceof \Stringable && (is_array($v) || $v instanceof \ArrayObject || $v instanceof
    /// \ArrayIterator) ? (string) $k : $k)` where `$v` is the local: `[key] -> [key']`.
    StringableKey {
        /// The local holding the container.
        container: LocalId,
    },
    /// `($v[$k] ?? null)`: `[container key] -> [value]`.
    DimIsset,

    // ---- calls --------------------------------------------------------------------------------------------------
    /// Starts an argument list; with `with_context`, snapshots the context for a `needs_context` callable (PHP passes
    /// `$context` by value before the other arguments are evaluated).
    ArgsStart {
        /// The callable needs the context.
        with_context: bool,
    },
    /// `[v] -> []`: appends a positional argument to the current list.
    Arg,
    /// `[v] -> []`: unpacks `...v` into the current list (`ZEND_SEND_UNPACK`), checking each named element against
    /// the parameters of the callable it is sent to (`zend_handle_named_arg()`).
    ArgSpread {
        /// The callable called (an index into the program's callables).
        callable: u32,
    },
    /// Calls the filter, function or test `callables[i]` with the current argument list (the injected arguments
    /// are added by the runtime): `[] -> [result]`.
    Call(u32),
    /// Calls the runtime closure of a node class defined by an extension (an index into the program's runtimes) with
    /// the top `argc` values, in stack order: `[arg1 ... argN] -> [result]`.
    CallRuntime {
        /// The runtime (an index into the program's runtimes).
        runtime: u32,
        /// The number of arguments.
        argc: u32,
    },

    // ---- assignment ---------------------------------------------------------------------------------------------
    /// Sequence destructuring (SequenceDestructuringSetBinary): `[value] -> [result]`. The constant is an array of
    /// target names (`null` for a skipped slot).
    SeqDestructure(ConstId),
    /// List assignment `[$context["a"], $context["b"]] = $array`: `[array] -> [array]`. The constant is an array of
    /// target names.
    ListAssign(ConstId),

    // ---- for loops (ForNode, ForLoopNode, ForElseNode) ----------------------------------------------------------
    /// `$context['_parent'] = $context;`.
    ForParent,
    /// `$context['loop'] = [...]` and the countable part.
    LoopInit,
    /// `foreach ($context['_seq'] as ...)`: starts iterating (`FE_RESET_R`).
    IterStart,
    /// The `FE_FETCH_R` step: next element, or jump to `exit`; assigns the value then the key.
    IterNext {
        /// Where to go when the sequence is exhausted.
        exit: Label,
        /// The constant holding the key target name.
        key: ConstId,
        /// The constant holding the value target name.
        value: ConstId,
    },
    /// Ends the iteration (`FE_FREE`).
    IterEnd,
    /// The `loop` bookkeeping at the end of an iteration (ForLoopNode with `with_loop`).
    LoopUpdate,
    /// The restore sequence after the loop: `$_parent = $context['_parent']; unset(...); $context =
    /// array_intersect_key($context, $_parent); $context += $_parent;`.
    ForRestore {
        /// The constant holding the key target name.
        key: ConstId,
        /// The constant holding the value target name.
        value: ConstId,
        /// The loop has an `else` (`_iterated` is unset).
        has_else: bool,
        /// The loop has `with_loop` (`loop` is unset).
        with_loop: bool,
    },

    // ---- with ---------------------------------------------------------------------------------------------------
    /// The variables part of WithNode: checks `is_iterable($_v)` (else `RuntimeError('Variables passed to the "with"
    /// tag must be a mapping.', line)`), `CoreExtension::toArray()`, then `[$context = [];] $context = $_v + $context;
    /// $context += globals`.
    WithVars {
        /// The local holding the variables.
        local: LocalId,
        /// `only`.
        only: bool,
        /// The line of the variables expression.
        line: i64,
    },

    // ---- markup -------------------------------------------------------------------------------------------------
    /// `new Markup(v, $this->env->getCharset())`: `[v] -> [markup]`.
    MakeMarkup,
    /// `('' === $tmp = v) ? '' : new Markup($tmp, $this->env->getCharset())`: `[v] -> [v']`.
    MarkupUnlessEmpty,

    // ---- arrow functions ----------------------------------------------------------------------------------------
    /// `function ($__a__, ...) use ($context, $macros) { ... }`: `[] -> [closure]`, snapshotting the context and the
    /// macros of the frame.
    MakeArrow(UnitId),
    /// The `RECV` opcodes that start the closure of an arrow function, on the line of `function (`: nothing to do
    /// (the arguments are bound when the closure is called), but the line of the `ArgumentCountError` PHP raises
    /// there for missing arguments.
    Recv,

    // ---- deprecations -------------------------------------------------------------------------------------------
    /// `trigger_deprecation(package, version, sprintf('%s in "<name>" at line N.', message))` (DeprecatedNode):
    /// `[package? version? message] -> []`.
    Deprecated {
        /// A package expression was compiled.
        has_package: bool,
        /// A version expression was compiled.
        has_version: bool,
        /// The line of the tag.
        line: i64,
    },

    // ---- sandbox ------------------------------------------------------------------------------------------------
    /// `$this->sandbox->ensureToStringAllowed(v, line, $this->source)` (or `ensureSpreadAllowed`, CheckToStringNode):
    /// `[v] -> [v']` (a single-pass iterator comes back as the array it was materialised into).
    CheckToString {
        /// `ensureSpreadAllowed`.
        spread: bool,
        /// The line of the checked expression.
        line: i64,
    },
    /// `if (!$alreadySandboxed = $this->sandbox->isSandboxed()) { $this->sandbox->setSandboxed(true); } try {`
    /// (SandboxNode): stores `$alreadySandboxed` in the local and opens the `finally` that restores the flag.
    SandboxStart {
        /// The local holding `$alreadySandboxed`.
        local: LocalId,
    },
    /// `} finally { if (!$alreadySandboxed) { $this->sandbox->setSandboxed(false); } }`: closes the region opened by
    /// [`Instr::SandboxStart`].
    SandboxEnd {
        /// The local holding `$alreadySandboxed`.
        local: LocalId,
    },

    // ---- templates ----------------------------------------------------------------------------------------------
    /// `$this->load(<template>, line[, index])` (`Template::load()`): `[template] -> [handle]`, where the handle is
    /// the loaded template object.
    LoadTemplate {
        /// The line passed to `load()` (`-1` makes Twig guess it).
        line: i64,
        /// The embedded-template index (`{% embed %}`).
        index: Option<i64>,
    },
    /// `$this->parent = <handle>` (`doDisplay()` of a template with a constant parent): `[handle] -> [handle]`.
    SetParent,
    /// `$this->getParent($context)` (`this`) or `$handle->getParent($context)`: `[] -> [parent]` /
    /// `[handle] -> [parent]`; `false` when there is no parent.
    GetParent {
        /// Ask the running template rather than a loaded one.
        this: bool,
    },
    /// The variables of an include: `CoreExtension::merge($context, <variables>)` (`merge`) or
    /// `CoreExtension::toArray(<variables>)`: `[variables] -> [context]`.
    IncludeContext {
        /// `with` without `only`.
        merge: bool,
    },
    /// The `array $context` parameter of `Template::yield()`: `[context] -> [context]`, a `TypeError` for anything
    /// but an array (`with <not iterable> only`).
    CheckContext,
    /// `yield from <handle>->unwrap()->yield(...)`: renders a template into the output. PHP runs the body of the
    /// template on the `YIELD_FROM` opcode, whose line is the line where the statement ends: the instruction is
    /// attributed to the marker in effect after the arguments, the call-time checks before it to the start.
    YieldTemplate(YieldArgs),
    /// `<target>->unwrap()->yieldBlock|renderBlock|hasBlock(<name>, $context[, $blocks])`: `[handle? name] ->
    /// [result?]`; the target is the loaded template on the stack (`template`, called without `$blocks`) or
    /// `$this`.
    Block {
        /// The block method.
        op: BlockOp,
        /// A loaded template is the target (`block(name, template)`).
        template: bool,
    },
    /// `yield from $this->yieldParentBlock("name", $context, $blocks)` or `$this->renderParentBlock(...)`
    /// (ParentExpression): `[] -> []` / `[] -> [string]`.
    ParentBlock {
        /// The constant holding the block name.
        name: ConstId,
        /// `renderParentBlock()` (an expression) rather than the output mode.
        render: bool,
    },

    // ---- macros -------------------------------------------------------------------------------------------------
    /// `($macros["name"] ?? $this->throwUninitializedMacroNamespace(line))` (MacroVariable): `[] -> [namespace]`.
    MacroNamespace {
        /// The constant holding the `$macros` key.
        name: ConstId,
        /// The line of the macro variable.
        line: i64,
    },
    /// `$this->getMacroNamespace()` (`_self` macros): `[] -> [namespace]`.
    SelfMacroNamespace,
    /// `<handle>->unwrap()->getMacroNamespace()` (ImportNode): `[handle] -> [namespace]`.
    TemplateMacroNamespace,
    /// `$macros["name"] = [$this->macros["name"] = ]<namespace>` (AssignMacroVariable): `[namespace] -> []`.
    SetMacroNamespace {
        /// The constant holding the `$macros` key.
        name: ConstId,
        /// The import is in the main scope of the template (`$this->macros` is set too).
        global: bool,
    },
    /// `<namespace>->call(<name>, <arguments>, $context, line, $this->source)` (MacroReferenceExpression):
    /// `[namespace name arguments] -> [result]`.
    MacroCall {
        /// The line of the call.
        line: i64,
    },
    /// `<namespace>->has(<name>, $context)` (MacroReferenceExpression in a `defined` test): `[namespace name] ->
    /// [bool]`.
    MacroDefined,

    // ---- exception regions --------------------------------------------------------------------------------------
    /// `try {` of `try { ... } catch (LoaderError $e) { $v = null; }` (`ignore missing`): a `LoaderError` raised
    /// before the matching [`Instr::EndTry`] unwinds the frame to its state here, pushes `null` and jumps to `catch`.
    TryLoader {
        /// Where execution continues after a caught `LoaderError` (with `null` on the stack).
        catch: Label,
    },
    /// Closes the region of the innermost [`Instr::TryLoader`].
    EndTry,

    // ---- variables ----------------------------------------------------------------------------------------------
    /// The read of `$blocks` passed to a block method of `$this`, after the block name: in an arrow function, whose
    /// closure does not capture `$blocks`, PHP's warning `Undefined variable $blocks` (the method then rejects the
    /// `null`).
    CheckBlocks,

    // ---- macro parameters ---------------------------------------------------------------------------------------
    /// `ZEND_RECV_INIT` of a parameter whose default PHP could not fold (an `IS_CONSTANT_AST` default): when the
    /// argument was not passed (the context holds the value `constant_ast` stands for), the value cached by an
    /// earlier call is stored, or execution goes on with the evaluation of the default and [`Instr::RecvStore`];
    /// otherwise jumps to `skip`.
    RecvInit {
        /// The constant holding the parameter name.
        name: ConstId,
        /// The constant holding the value standing for the unevaluated default.
        constant_ast: ConstId,
        /// The runtime cache slot.
        cache: u32,
        /// Where to go when nothing is left to evaluate.
        skip: Label,
    },
    /// Stores the default evaluated after [`Instr::RecvInit`] in the context: `[v] -> []`. A value that is not
    /// refcounted (`null`, a bool, an int, a float) is cached for later calls (`ZEND_RECV_INIT` keeps such values in
    /// the runtime cache of the compiled closure).
    RecvStore {
        /// The constant holding the parameter name.
        name: ConstId,
        /// The runtime cache slot.
        cache: u32,
    },

    // ---- fused instructions -------------------------------------------------------------------------------------
    // The code generator does not emit these: the fusion pass that ends the compilation of every unit replaces
    // frequent sequences of the instructions above with them (see `plans/engine.md`, "Instruction fusion").
    /// A call of the filter, function or test `callables[callable]` whose arguments are on the stack: `ArgsStart`
    /// (without context), the arguments each followed by `Arg`, and `Call` fused. The `argc` values on top of the
    /// stack are the first positional arguments, the elements of the constant array `consts` the ones after them
    /// (trailing constant arguments fused); with `print`, the result is printed (`Print` fused): `[arg1 ... argN] ->
    /// [result]` (`[arg1 ... argN] -> []` with `print`).
    CallStack {
        /// The callable called (an index into the program's callables).
        callable: u32,
        /// The number of arguments on the stack.
        argc: u32,
        /// The constant array of the arguments after those on the stack.
        consts: Option<ConstId>,
        /// The result is printed.
        print: bool,
    },
    /// `LoadName { name, mode }` (lenient or always defined) and a [`Instr::CallStack`] of one stack argument fused:
    /// `[] -> [result]` (`[] -> []` with `print`). The `escape` filter of the core runtime gets the value where it is
    /// in the context, and a value it returns unchanged is printed in place.
    CallName {
        /// The constant holding the name read.
        name: ConstId,
        /// `NameMode::AlwaysDefined` (else `NameMode::Lenient`).
        always_defined: bool,
        /// The callable called (an index into the program's callables).
        callable: u32,
        /// The constant array of the arguments after the value.
        consts: Option<ConstId>,
        /// The result is printed.
        print: bool,
    },
    /// `[a] -> [a op constant]` (`[a] -> [constant op a]` when `commuted`): `Const(value)` and `Binary(op)` (or
    /// `CommutedBinary(op)`) fused.
    BinaryConst {
        /// The operator.
        op: BinaryOp,
        /// `CommutedBinary`.
        commuted: bool,
        /// The constant right operand.
        value: ConstId,
    },
    /// `$context[name] = a op b`: `Binary(op)` (or `CommutedBinary(op)`, or a [`Instr::BinaryConst`] when `value` is
    /// the constant right operand) and `StoreName(name)` fused: `[a b] -> []` (`[a] -> []`).
    BinaryStore {
        /// The operator.
        op: BinaryOp,
        /// `CommutedBinary`.
        commuted: bool,
        /// The constant right operand, when it is not on the stack.
        value: Option<ConstId>,
        /// The constant holding the name assigned.
        name: ConstId,
    },
    /// `$context[name] = ($context[left] ?? null) op value`: `LoadName { name: left, mode: Lenient }` and a
    /// [`Instr::BinaryStore`] with a constant operand fused: `[] -> []`. The operator reads the variable where it is
    /// in the context.
    NameBinaryStore {
        /// The operator.
        op: BinaryOp,
        /// `CommutedBinary`: the constant is the left operand.
        commuted: bool,
        /// The constant holding the name of the variable operand.
        left: ConstId,
        /// The constant operand.
        value: ConstId,
        /// The constant holding the name assigned.
        name: ConstId,
    },
    /// A comparison and the conditional jump on its result: `Binary(op)` (or a [`Instr::BinaryConst`] when `value` is
    /// the constant right operand) and `JumpIfTrue(target)` (`jump_if`) or `JumpIfFalse(target)` fused: `[a b] -> []`
    /// (`[a] -> []`). The comparisons give booleans, whose truthiness reports nothing.
    JumpIfCompare {
        /// The comparison (`==`, `!=`, `<`, `>`, `<=`, `>=`, `===`, `!==`).
        op: BinaryOp,
        /// The constant right operand, when it is not on the stack.
        value: Option<ConstId>,
        /// Jump when the comparison is true (`JumpIfTrue`) rather than false.
        jump_if: bool,
        /// Where to jump.
        target: Label,
    },
    /// `$context[to] = ($context[from] ?? null)`: `LoadName { name: from, mode: Lenient }` and `StoreName(to)` fused.
    CopyName {
        /// The constant holding the name read.
        from: ConstId,
        /// The constant holding the name assigned.
        to: ConstId,
    },
    /// `$context[name] = <constant>`: `Const(value)` and `StoreName(name)` fused.
    StoreConst {
        /// The constant stored.
        value: ConstId,
        /// The constant holding the name assigned.
        name: ConstId,
    },
    /// `CoreExtension::getAttribute()` of a constant attribute without arguments: `Const(item)`, `NewArray` and
    /// `GetAttr(op)` fused: `[object] -> [value]`.
    GetAttrConst {
        /// The compile-time operands.
        op: GetAttrOp,
        /// The constant attribute.
        item: ConstId,
    },
}

#[cfg(test)]
mod tests {
    /// Instructions stay four words long (`LoadTemplate` is): the fused instructions fit in the size of the others.
    #[test]
    fn instructions_are_four_words() {
        assert!(std::mem::size_of::<super::Instr>() <= 32);
    }
}
