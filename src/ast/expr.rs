//! Expression nodes: the classes under `Twig\Node\Expression` (TWIG/src/Node/Expression/).
//!
//! Every Twig expression class is an [`ExprKind`] variant (or a variant of an operator/kind enum for the binary,
//! unary, filter, function and test families). Attributes are typed fields; the attributes Twig may append to any
//! expression (`with_parentheses`, `always_defined`, `sandboxed_function_name`/`sandboxed_function`) live on
//! [`Expr`]. Constructors port the PHP constructors, including their checks and side effects.

use std::borrow::Cow;
use std::fmt;

use super::value::{AttrValue, ConstantValue, Lossy, TempName, TempNameInput, ascii_lowercase};
use super::visit::CustomExpr;
use crate::error::{Error, Result};
use crate::extension::callable::{
    FilterNodeClass, FunctionNodeClass, TestNodeClass, TwigFilter, TwigFunction, TwigTest,
    internal_test,
};

/// An expression node. Twig: `Twig\Node\Expression\AbstractExpression` and its subclasses.
pub struct Expr {
    /// The node class and its own attributes and children.
    pub kind: ExprKind,
    /// The template line (`getTemplateLine()`).
    pub line: i64,
    /// Twig's `with_parentheses` attribute: the expression was written in parentheses
    /// (`AbstractExpression::setExplicitParentheses()`). Unary nodes always have the attribute (default `false`).
    pub with_parentheses: bool,
    /// Twig's `always_defined` attribute. Variables always have it (default `false`); the left operand of `?:` and
    /// `??` gets it whatever its class (`ElvisBinary`, `NullCoalesceBinary`), and the optimizer sets it on loop
    /// variables.
    pub always_defined: bool,
    /// Twig's `sandboxed_function_name` and `sandboxed_function` attributes, set on the node a function's
    /// `parser_callable` returned (`parent()`, `block()`, `attribute()`), for the sandbox.
    pub sandboxed_function: Option<Box<SandboxedFunction>>,
    /// Documentation attached by `{## ... ##}` comments (Twig's `Node::getDocumentation()`).
    pub documentation: Option<Vec<u8>>,
    /// The expression parser that produced this node, for the precedence-change deprecations (Twig keeps it in the
    /// parser's `expressionRefs` weak map; a clone, like a PHP clone, has none).
    pub(crate) parsed_by: Option<u32>,
}

/// The function a `parser_callable` node was created for. Twig: the `sandboxed_function_name` and
/// `sandboxed_function` attributes.
#[derive(Debug, Clone, PartialEq)]
pub struct SandboxedFunction {
    /// The function name as written in the template.
    pub name: Vec<u8>,
    /// The function.
    pub function: TwigFunction,
}

impl Clone for Expr {
    /// Twig's `__clone()` (a deep clone). The parser's precedence bookkeeping is not copied, as a PHP clone is a new
    /// key of the parser's weak map.
    fn clone(&self) -> Self {
        Expr {
            kind: self.kind.clone(),
            line: self.line,
            with_parentheses: self.with_parentheses,
            always_defined: self.always_defined,
            sandboxed_function: self.sandboxed_function.clone(),
            documentation: self.documentation.clone(),
            parsed_by: None,
        }
    }
}

impl PartialEq for Expr {
    fn eq(&self, other: &Self) -> bool {
        self.kind == other.kind
            && self.line == other.line
            && self.with_parentheses == other.with_parentheses
            && self.always_defined == other.always_defined
            && self.sandboxed_function == other.sandboxed_function
            && self.documentation == other.documentation
    }
}

impl fmt::Debug for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&super::dump::dump_expr(self))
    }
}

/// The class of an expression node with its own attributes and children.
#[derive(Debug, Clone, PartialEq)]
pub enum ExprKind {
    /// `ConstantExpression`.
    Constant(ConstantExpr),
    /// `Variable\ContextVariable`: a variable read from the context.
    ContextVariable(NameExpr),
    /// `Variable\AssignContextVariable`: an assignment target (a subclass of `ContextVariable`).
    AssignContextVariable(NameExpr),
    /// `Variable\LocalVariable`: a compiled PHP local variable.
    LocalVariable(TempName),
    /// `Variable\MacroVariable`: an imported macro namespace.
    MacroVariable(MacroVariableExpr),
    /// `Variable\AssignMacroVariable`.
    AssignMacroVariable(Box<AssignMacroVariableExpr>),
    /// `ArrayExpression` (sequence and mapping literals) or its subclass `VariadicExpression`.
    Array(Box<ArrayExpr>),
    /// `ArrowFunctionExpression`.
    ArrowFunction(Box<ArrowFunctionExpr>),
    /// `ListExpression`: the parameters of an arrow function.
    List(Box<ListExpr>),
    /// A subclass of `Unary\AbstractUnary`.
    Unary(Box<UnaryExpr>),
    /// A subclass of `Binary\AbstractBinary`.
    Binary(Box<BinaryExpr>),
    /// `Ternary\ConditionalTernary`.
    ConditionalTernary(Box<ConditionalTernaryExpr>),
    /// `GetAttrExpression`.
    GetAttr(Box<GetAttrExpr>),
    /// `MacroReferenceExpression`.
    MacroReference(Box<MacroReferenceExpr>),
    /// `BlockReferenceExpression` (`block()`).
    BlockReference(Box<BlockReferenceExpr>),
    /// `ParentExpression` (`parent()`).
    Parent(ParentExpr),
    /// `FilterExpression` and its core subclasses.
    Filter(Box<FilterExpr>),
    /// `FunctionExpression` and its core subclasses.
    Function(Box<FunctionExpr>),
    /// `TestExpression` and its core subclasses.
    Test(Box<TestExpr>),
    /// `EmptyExpression`: an empty slot of a destructuring pattern.
    Empty,
    /// `Twig\Node\CheckToStringNode`, inserted by the sandbox.
    CheckToString(Box<CheckToStringExpr>),
    /// An expression node class defined by an extension.
    Custom(Box<dyn CustomExpr>),
}

/// `ConstantExpression`.
#[derive(Debug, Clone, PartialEq)]
pub struct ConstantExpr {
    /// The `value` attribute.
    pub value: ConstantValue,
    /// The defined-test mode (`SupportDefinedTestTrait`).
    pub defined_test: bool,
    /// The `is_implicit` attribute (appended after `value`): the `null` default of a macro argument declared without
    /// a default value.
    pub is_implicit: bool,
}

/// `ContextVariable`/`AssignContextVariable` (Twig's `NameExpression`); `always_defined` is [`Expr::always_defined`].
#[derive(Debug, Clone, PartialEq)]
pub struct NameExpr {
    /// The `name` attribute.
    pub name: Vec<u8>,
    /// The `ignore_strict_check` attribute.
    pub ignore_strict_check: bool,
    /// The defined-test mode.
    pub defined_test: bool,
}

/// `MacroVariable`.
///
/// A `from ... import` creates one macro variable without a name, shared (as the same PHP object) by the import
/// node and every macro call it enables; the compiler names it lazily. `id` stands for that identity: copies with
/// the same id are the same variable.
#[derive(Debug, Clone, PartialEq)]
pub struct MacroVariableExpr {
    /// The `name` attribute.
    pub name: TempName,
    /// The identity of the variable within one template.
    pub id: u32,
}

/// `AssignMacroVariable`.
#[derive(Debug, Clone, PartialEq)]
pub struct AssignMacroVariableExpr {
    /// The `var` child (a `MacroVariable`).
    pub var: Expr,
    /// The `global` attribute.
    pub global: bool,
}

/// `ArrayExpression`. The children are the key/value pairs; `index` is Twig's private auto-index counter.
#[derive(Debug, Clone, PartialEq)]
pub struct ArrayExpr {
    /// The key/value pairs in order.
    pub elements: Vec<(Expr, Expr)>,
    /// The last automatic key (`-1` initially).
    pub index: i64,
    /// The defined-test mode.
    pub defined_test: bool,
    /// The class is `VariadicExpression`.
    pub variadic: bool,
}

/// `ArrowFunctionExpression`.
#[derive(Debug, Clone, PartialEq)]
pub struct ArrowFunctionExpr {
    /// The `expr` child (the body).
    pub expr: Expr,
    /// The `names` child (a `ListExpression` of `AssignContextVariable`).
    pub names: Expr,
}

/// `ListExpression`.
#[derive(Debug, Clone, PartialEq)]
pub struct ListExpr {
    /// The items (`AssignContextVariable` nodes), keyed 0, 1, ...
    pub items: Vec<Expr>,
}

/// The unary operator classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnaryOp {
    /// `NegUnary` (`-`).
    Neg,
    /// `NotUnary` (`not`).
    Not,
    /// `PosUnary` (`+`).
    Pos,
    /// `SpreadUnary` (`...`).
    Spread,
    /// `StringCastUnary` (dynamic array keys).
    StringCast,
}

impl UnaryOp {
    /// The PHP class name.
    pub fn class_name(self) -> &'static str {
        match self {
            UnaryOp::Neg => "Twig\\Node\\Expression\\Unary\\NegUnary",
            UnaryOp::Not => "Twig\\Node\\Expression\\Unary\\NotUnary",
            UnaryOp::Pos => "Twig\\Node\\Expression\\Unary\\PosUnary",
            UnaryOp::Spread => "Twig\\Node\\Expression\\Unary\\SpreadUnary",
            UnaryOp::StringCast => "Twig\\Node\\Expression\\Unary\\StringCastUnary",
        }
    }
}

/// A unary operator node.
#[derive(Debug, Clone, PartialEq)]
pub struct UnaryExpr {
    /// The operator class.
    pub op: UnaryOp,
    /// The `node` child.
    pub node: Expr,
}

/// The binary operator classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(
    missing_docs,
    reason = "each variant is the binary class of the same name"
)]
pub enum BinaryOp {
    Add,
    And,
    BitwiseAnd,
    BitwiseOr,
    BitwiseXor,
    Concat,
    Div,
    Elvis,
    EndsWith,
    Equal,
    FloorDiv,
    Greater,
    GreaterEqual,
    HasEvery,
    HasSome,
    In,
    Less,
    LessEqual,
    Matches,
    Mod,
    Mul,
    NotEqual,
    NotIn,
    NotSameAs,
    NullCoalesce,
    ObjectDestructuringSet,
    Or,
    Power,
    Range,
    SameAs,
    SequenceDestructuringSet,
    Set,
    Spaceship,
    StartsWith,
    Sub,
    Xor,
}

impl BinaryOp {
    /// The PHP class name, e.g. `Twig\Node\Expression\Binary\AddBinary`.
    pub fn class_name(self) -> &'static str {
        match self {
            BinaryOp::Add => "Twig\\Node\\Expression\\Binary\\AddBinary",
            BinaryOp::And => "Twig\\Node\\Expression\\Binary\\AndBinary",
            BinaryOp::BitwiseAnd => "Twig\\Node\\Expression\\Binary\\BitwiseAndBinary",
            BinaryOp::BitwiseOr => "Twig\\Node\\Expression\\Binary\\BitwiseOrBinary",
            BinaryOp::BitwiseXor => "Twig\\Node\\Expression\\Binary\\BitwiseXorBinary",
            BinaryOp::Concat => "Twig\\Node\\Expression\\Binary\\ConcatBinary",
            BinaryOp::Div => "Twig\\Node\\Expression\\Binary\\DivBinary",
            BinaryOp::Elvis => "Twig\\Node\\Expression\\Binary\\ElvisBinary",
            BinaryOp::EndsWith => "Twig\\Node\\Expression\\Binary\\EndsWithBinary",
            BinaryOp::Equal => "Twig\\Node\\Expression\\Binary\\EqualBinary",
            BinaryOp::FloorDiv => "Twig\\Node\\Expression\\Binary\\FloorDivBinary",
            BinaryOp::Greater => "Twig\\Node\\Expression\\Binary\\GreaterBinary",
            BinaryOp::GreaterEqual => "Twig\\Node\\Expression\\Binary\\GreaterEqualBinary",
            BinaryOp::HasEvery => "Twig\\Node\\Expression\\Binary\\HasEveryBinary",
            BinaryOp::HasSome => "Twig\\Node\\Expression\\Binary\\HasSomeBinary",
            BinaryOp::In => "Twig\\Node\\Expression\\Binary\\InBinary",
            BinaryOp::Less => "Twig\\Node\\Expression\\Binary\\LessBinary",
            BinaryOp::LessEqual => "Twig\\Node\\Expression\\Binary\\LessEqualBinary",
            BinaryOp::Matches => "Twig\\Node\\Expression\\Binary\\MatchesBinary",
            BinaryOp::Mod => "Twig\\Node\\Expression\\Binary\\ModBinary",
            BinaryOp::Mul => "Twig\\Node\\Expression\\Binary\\MulBinary",
            BinaryOp::NotEqual => "Twig\\Node\\Expression\\Binary\\NotEqualBinary",
            BinaryOp::NotIn => "Twig\\Node\\Expression\\Binary\\NotInBinary",
            BinaryOp::NotSameAs => "Twig\\Node\\Expression\\Binary\\NotSameAsBinary",
            BinaryOp::NullCoalesce => "Twig\\Node\\Expression\\Binary\\NullCoalesceBinary",
            BinaryOp::ObjectDestructuringSet => {
                "Twig\\Node\\Expression\\Binary\\ObjectDestructuringSetBinary"
            }
            BinaryOp::Or => "Twig\\Node\\Expression\\Binary\\OrBinary",
            BinaryOp::Power => "Twig\\Node\\Expression\\Binary\\PowerBinary",
            BinaryOp::Range => "Twig\\Node\\Expression\\Binary\\RangeBinary",
            BinaryOp::SameAs => "Twig\\Node\\Expression\\Binary\\SameAsBinary",
            BinaryOp::SequenceDestructuringSet => {
                "Twig\\Node\\Expression\\Binary\\SequenceDestructuringSetBinary"
            }
            BinaryOp::Set => "Twig\\Node\\Expression\\Binary\\SetBinary",
            BinaryOp::Spaceship => "Twig\\Node\\Expression\\Binary\\SpaceshipBinary",
            BinaryOp::StartsWith => "Twig\\Node\\Expression\\Binary\\StartsWithBinary",
            BinaryOp::Sub => "Twig\\Node\\Expression\\Binary\\SubBinary",
            BinaryOp::Xor => "Twig\\Node\\Expression\\Binary\\XorBinary",
        }
    }

    /// Whether the class implements `ReturnPrimitiveTypeInterface` (through `ReturnBoolInterface`,
    /// `ReturnNumberInterface`, `ReturnStringInterface` or `ReturnArrayInterface`).
    pub fn returns_primitive(self) -> bool {
        !matches!(
            self,
            BinaryOp::Elvis
                | BinaryOp::NullCoalesce
                | BinaryOp::ObjectDestructuringSet
                | BinaryOp::SequenceDestructuringSet
                | BinaryOp::Set
        )
    }
}

/// A binary operator node. `test` is the extra child of `ElvisBinary` and `NullCoalesceBinary`.
#[derive(Debug, Clone, PartialEq)]
pub struct BinaryExpr {
    /// The operator class.
    pub op: BinaryOp,
    /// The `left` child.
    pub left: Expr,
    /// The `right` child.
    pub right: Expr,
    /// The `test` child of `?:` and `??`.
    pub test: Option<Expr>,
}

/// `Ternary\ConditionalTernary`.
#[derive(Debug, Clone, PartialEq)]
pub struct ConditionalTernaryExpr {
    /// The `test` child (wrapped by `TrueTest::wrap()`).
    pub test: Expr,
    /// The `left` child (the value when true).
    pub left: Expr,
    /// The `right` child (the value when false).
    pub right: Expr,
}

/// The `type` attribute of `GetAttrExpression` (Twig's `Template::ANY_CALL`, `ARRAY_CALL`, `METHOD_CALL`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CallType {
    /// `any` (`a.b`).
    Any,
    /// `array` (`a[b]`).
    Array,
    /// `method` (`a.b()`).
    Method,
}

impl CallType {
    /// `any`, `array` or `method`.
    pub fn as_str(self) -> &'static str {
        match self {
            CallType::Any => "any",
            CallType::Array => "array",
            CallType::Method => "method",
        }
    }
}

/// `GetAttrExpression`.
#[derive(Debug, Clone, PartialEq)]
pub struct GetAttrExpr {
    /// The `node` child (the object).
    pub node: Expr,
    /// The `attribute` child.
    pub attribute: Expr,
    /// The optional `arguments` child (an `ArrayExpression`).
    pub arguments: Option<Expr>,
    /// The `type` attribute.
    pub call_type: CallType,
    /// The `ignore_strict_check` attribute.
    pub ignore_strict_check: bool,
    /// The `optimizable` attribute.
    pub optimizable: bool,
    /// The `null_safe` attribute (`?.`).
    pub null_safe: bool,
    /// The `is_short_circuited` attribute (set at compile time).
    pub is_short_circuited: bool,
    /// The `var_name` attribute (set at compile time).
    pub var_name: Option<String>,
    /// The defined-test mode.
    pub defined_test: bool,
}

/// The macro name of a `MacroReferenceExpression`.
#[derive(Debug, Clone, PartialEq)]
pub enum MacroName {
    /// A static name (the `name` attribute).
    Static(Vec<u8>),
    /// A dynamic name (the `name` child), `forms.(expr)()`.
    Dynamic(Expr),
}

/// `MacroReferenceExpression`.
#[derive(Debug, Clone, PartialEq)]
pub struct MacroReferenceExpr {
    /// The `template` child (a `MacroVariable`).
    pub template: Expr,
    /// The macro name.
    pub name: MacroName,
    /// The `arguments` child (an `ArrayExpression`).
    pub arguments: Expr,
    /// Whether the macro was called with parentheses (`hasCallParentheses()`).
    pub has_call_parentheses: bool,
    /// The defined-test mode.
    pub defined_test: bool,
}

/// `BlockReferenceExpression`.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockReferenceExpr {
    /// The `name` child.
    pub name: Expr,
    /// The optional `template` child.
    pub template: Option<Expr>,
    /// The `output` attribute (set by the optimizer).
    pub output: bool,
    /// The defined-test mode.
    pub defined_test: bool,
}

/// `ParentExpression`.
#[derive(Debug, Clone, PartialEq)]
pub struct ParentExpr {
    /// The `name` attribute (the block name).
    pub name: Vec<u8>,
    /// The `output` attribute (set by the optimizer).
    pub output: bool,
}

/// The core filter node classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterKind {
    /// `FilterExpression`.
    Filter,
    /// `Filter\DefaultFilter`.
    Default,
    /// `Filter\EscapeFilter` with its `template_escaper` attribute.
    Escape {
        /// Set by the escaper visitor when the filter compiles to the template escaper.
        template_escaper: bool,
    },
    /// `Filter\RawFilter`.
    Raw,
}

/// `FilterExpression`.
#[derive(Debug, Clone, PartialEq)]
pub struct FilterExpr {
    /// The `node` child (the filtered value).
    pub node: Expr,
    /// The `twig_callable` attribute; `name` is its name.
    pub filter: TwigFilter,
    /// The deprecated `filter` child, a `ConstantExpression` of the filter name.
    pub filter_name: Expr,
    /// The `arguments` child.
    pub arguments: Arguments,
    /// The node class.
    pub kind: FilterKind,
}

/// The core function node classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FunctionKind {
    /// `FunctionExpression`.
    Function,
    /// `FunctionNode\EnumFunction`.
    Enum,
    /// `FunctionNode\EnumCasesFunction`.
    EnumCases,
    /// `FunctionNode\RenderSandboxedFunction`.
    RenderSandboxed,
}

/// `FunctionExpression`.
#[derive(Debug, Clone, PartialEq)]
pub struct FunctionExpr {
    /// The `twig_callable` attribute; `name` is its name.
    pub function: TwigFunction,
    /// The `arguments` child.
    pub arguments: Arguments,
    /// The node class.
    pub kind: FunctionKind,
    /// The defined-test mode (only ever enabled for `constant()`).
    pub defined_test: bool,
}

/// The core test node classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(
    missing_docs,
    reason = "each variant is the test class of the same name"
)]
pub enum TestKind {
    Test,
    Constant,
    Defined,
    DivisibleBy,
    Even,
    NonStringCoercingInput,
    Null,
    Odd,
    SameAs,
    True,
}

impl TestKind {
    /// The PHP class name.
    pub fn class_name(self) -> &'static str {
        match self {
            TestKind::Test => "Twig\\Node\\Expression\\TestExpression",
            TestKind::Constant => "Twig\\Node\\Expression\\Test\\ConstantTest",
            TestKind::Defined => "Twig\\Node\\Expression\\Test\\DefinedTest",
            TestKind::DivisibleBy => "Twig\\Node\\Expression\\Test\\DivisiblebyTest",
            TestKind::Even => "Twig\\Node\\Expression\\Test\\EvenTest",
            TestKind::NonStringCoercingInput => {
                "Twig\\Node\\Expression\\Test\\NonStringCoercingInputTest"
            }
            TestKind::Null => "Twig\\Node\\Expression\\Test\\NullTest",
            TestKind::Odd => "Twig\\Node\\Expression\\Test\\OddTest",
            TestKind::SameAs => "Twig\\Node\\Expression\\Test\\SameasTest",
            TestKind::True => "Twig\\Node\\Expression\\Test\\TrueTest",
        }
    }
}

/// `TestExpression`.
#[derive(Debug, Clone, PartialEq)]
pub struct TestExpr {
    /// The `node` child (the tested value).
    pub node: Expr,
    /// The `twig_callable` attribute; `name` is its name.
    pub test: TwigTest,
    /// The optional `arguments` child.
    pub arguments: Option<Arguments>,
    /// The node class.
    pub kind: TestKind,
}

/// `Twig\Node\CheckToStringNode`.
#[derive(Debug, Clone, PartialEq)]
pub struct CheckToStringExpr {
    /// The `expr` child.
    pub expr: Expr,
    /// The `spread` attribute.
    pub spread: bool,
}

/// The key of a call argument: its position or its name.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ArgumentKey {
    /// A positional argument (int key).
    Index(i64),
    /// A named argument (`name = value` or `name: value`).
    Name(Vec<u8>),
}

/// The arguments of a filter, function or test call: Twig's `Nodes` of argument expressions keyed by position or
/// name, or an `EmptyNode` when a filter is used without parentheses.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Arguments {
    entries: Vec<(ArgumentKey, Expr)>,
    empty_node: bool,
}

impl Arguments {
    /// An empty `Nodes` container (`()`).
    pub fn new() -> Self {
        Arguments::default()
    }

    /// Twig's `EmptyNode` in the arguments position (a filter without parentheses, the internal tests).
    pub fn empty_node() -> Self {
        Arguments {
            entries: Vec::new(),
            empty_node: true,
        }
    }

    /// Whether this is an `EmptyNode` rather than a `Nodes` container.
    pub fn is_empty_node(&self) -> bool {
        self.empty_node
    }

    /// Appends a positional argument: `$args[] = $value` (the key is the next int key).
    pub fn push(&mut self, value: Expr) {
        let next = self
            .entries
            .iter()
            .filter_map(|(key, _)| match key {
                ArgumentKey::Index(i) => Some(*i + 1),
                ArgumentKey::Name(_) => None,
            })
            .max()
            .unwrap_or(0);
        self.empty_node = false;
        self.entries.push((ArgumentKey::Index(next), value));
    }

    /// Sets a named argument: `$args[$name] = $value` (a repeated name keeps its first position).
    pub fn set_named(&mut self, name: Vec<u8>, value: Expr) {
        self.empty_node = false;
        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|(key, _)| matches!(key, ArgumentKey::Name(n) if *n == name))
        {
            entry.1 = value;
        } else {
            self.entries.push((ArgumentKey::Name(name), value));
        }
    }

    /// The arguments in order.
    pub fn entries(&self) -> &[(ArgumentKey, Expr)] {
        &self.entries
    }

    /// The arguments in order, mutably (keys cannot change).
    pub fn entries_mut(&mut self) -> impl Iterator<Item = (&ArgumentKey, &mut Expr)> {
        self.entries.iter_mut().map(|(k, v)| (&*k, v))
    }

    /// Consumes the container, returning the arguments in order.
    pub fn into_entries(self) -> Vec<(ArgumentKey, Expr)> {
        self.entries
    }

    /// Removes the argument with the given key (`removeNode()`).
    pub fn remove(&mut self, key: &ArgumentKey) -> Option<Expr> {
        let index = self.entries.iter().position(|(k, _)| k == key)?;
        Some(self.entries.remove(index).1)
    }

    /// The number of arguments (`count()`).
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether there is no argument.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The argument with the given key (`getNode()`).
    pub fn get(&self, key: &ArgumentKey) -> Option<&Expr> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    /// The PHP class of the container: `Twig\Node\EmptyNode` or `Twig\Node\Nodes`.
    pub fn class_name(&self) -> &'static str {
        if self.empty_node {
            "Twig\\Node\\EmptyNode"
        } else {
            "Twig\\Node\\Nodes"
        }
    }
}

impl Expr {
    /// An expression of the given kind with default common attributes.
    pub fn new(kind: ExprKind, line: i64) -> Self {
        Expr {
            kind,
            line,
            with_parentheses: false,
            always_defined: false,
            sandboxed_function: None,
            documentation: None,
            parsed_by: None,
        }
    }

    /// An expression of a class defined by an extension (`new MyExpression(..., $lineno)`), what the node class of a
    /// callable or an operator creates.
    pub fn custom(expr: impl CustomExpr, line: i64) -> Self {
        Expr::new(ExprKind::Custom(Box::new(expr)), line)
    }

    /// Twig: `new ConstantExpression($value, $lineno)`.
    pub fn constant(value: impl Into<ConstantValue>, line: i64) -> Self {
        Expr::new(
            ExprKind::Constant(ConstantExpr {
                value: value.into(),
                defined_test: false,
                is_implicit: false,
            }),
            line,
        )
    }

    /// Twig: `new ContextVariable($name, $lineno)`.
    pub fn context_variable(name: impl Into<Vec<u8>>, line: i64) -> Self {
        Expr::new(
            ExprKind::ContextVariable(NameExpr {
                name: name.into(),
                ignore_strict_check: false,
                defined_test: false,
            }),
            line,
        )
    }

    /// Twig: `new AssignContextVariable($name, $lineno)`.
    ///
    /// # Errors
    ///
    /// `You cannot assign a value to "%s".` for `true`, `false`, `none` and `null` in any case
    /// (`AssignNameExpression::__construct()`).
    pub fn assign_context_variable(name: impl Into<Vec<u8>>, line: i64) -> Result<Self> {
        let name = name.into();
        if is_reserved_constant_name(&name) {
            return Err(cannot_assign(&name, line));
        }
        Ok(Expr::new(
            ExprKind::AssignContextVariable(NameExpr {
                name,
                ignore_strict_check: false,
                defined_test: false,
            }),
            line,
        ))
    }

    /// Twig: `new LocalVariable($name, $lineno)`.
    ///
    /// # Errors
    ///
    /// `You cannot assign a value to "%s".` for a truthy name that is `true`, `false`, `none` or `null` in any case
    /// (`TempNameExpression::__construct()`).
    pub fn local_variable(name: impl Into<TempNameInput>, line: i64) -> Result<Self> {
        let name = check_temp_name(name.into(), line)?;
        Ok(Expr::new(ExprKind::LocalVariable(name), line))
    }

    /// Twig: `new MacroVariable($name, $lineno)`; `id` identifies the variable (see [`MacroVariableExpr`]).
    ///
    /// # Errors
    ///
    /// As [`local_variable`](Self::local_variable).
    pub fn macro_variable(name: impl Into<TempNameInput>, line: i64, id: u32) -> Result<Self> {
        let name = check_temp_name(name.into(), line)?;
        Ok(Expr::new(
            ExprKind::MacroVariable(MacroVariableExpr { name, id }),
            line,
        ))
    }

    /// Twig: `new AssignMacroVariable($var, $global)`; the line is the variable's.
    pub fn assign_macro_variable(var: Expr, global: bool) -> Self {
        let line = var.line;
        Expr::new(
            ExprKind::AssignMacroVariable(Box::new(AssignMacroVariableExpr { var, global })),
            line,
        )
    }

    /// Twig: `new ArrayExpression([], $lineno)`.
    pub fn array(line: i64) -> Self {
        Expr::new(
            ExprKind::Array(Box::new(ArrayExpr {
                elements: Vec::new(),
                index: -1,
                defined_test: false,
                variadic: false,
            })),
            line,
        )
    }

    /// Twig: `new ArrayExpression($elements, $lineno)` with key/value pairs: the auto-index starts after the
    /// highest non-negative int constant key.
    pub fn array_from_pairs(elements: Vec<(Expr, Expr)>, line: i64) -> Self {
        let mut index = -1;
        for (key, _) in &elements {
            if let ExprKind::Constant(ConstantExpr { value, .. }) = &key.kind {
                let candidate = match value {
                    ConstantValue::Int(i) if *i >= 0 => Some(*i),
                    ConstantValue::String(s)
                        if !s.is_empty() && s.iter().all(u8::is_ascii_digit) =>
                    {
                        std::str::from_utf8(s).ok().and_then(|s| s.parse().ok())
                    }
                    _ => None,
                };
                if let Some(candidate) = candidate
                    && candidate > index
                {
                    index = candidate;
                }
            }
        }
        Expr::new(
            ExprKind::Array(Box::new(ArrayExpr {
                elements,
                index,
                defined_test: false,
                variadic: false,
            })),
            line,
        )
    }

    /// Twig: `new ArrowFunctionExpression($expr, $names, $lineno)`: a single `ContextVariable` becomes a
    /// one-element `ListExpression` of `AssignContextVariable`.
    ///
    /// # Errors
    ///
    /// `The arrow function argument must be a list of variables or a single variable.` (at the line of `names`)
    /// when `names` is neither a variable nor a list; `You cannot assign a value to "%s".` for `true`/`null`-like
    /// names.
    pub fn arrow_function(expr: Expr, names: Expr, line: i64) -> Result<Self> {
        let names = if let Some(name) = names.context_variable_name() {
            let item = Expr::assign_context_variable(name.to_vec(), names.line)?;
            Expr::list(vec![item], line)
        } else {
            names
        };
        if !matches!(names.kind, ExprKind::List(_)) {
            return Err(Error::syntax(
                "The arrow function argument must be a list of variables or a single variable.",
            )
            .with_line(names.line));
        }
        Ok(Expr::new(
            ExprKind::ArrowFunction(Box::new(ArrowFunctionExpr { expr, names })),
            line,
        ))
    }

    /// Twig: `new ListExpression($items, $lineno)`.
    pub fn list(items: Vec<Expr>, line: i64) -> Self {
        Expr::new(ExprKind::List(Box::new(ListExpr { items })), line)
    }

    /// Twig: `new NegUnary($node, $lineno)` and the other unary classes; `NotUnary` wraps its operand with
    /// `TrueTest::wrap()`.
    pub fn unary(op: UnaryOp, node: Expr, line: i64) -> Self {
        let node = if op == UnaryOp::Not {
            Expr::true_test_wrap(node)
        } else {
            node
        };
        Expr::new(ExprKind::Unary(Box::new(UnaryExpr { op, node })), line)
    }

    /// Twig: `new XxxBinary($left, $right, $lineno)` with the constructor logic of the special classes:
    /// `AndBinary`, `OrBinary` and `XorBinary` wrap their operands with `TrueTest::wrap()`, `SetBinary` turns
    /// `left` into an `AssignContextVariable`, `ElvisBinary`/`NullCoalesceBinary` build their
    /// `test` child and mark `left` as always defined, the destructuring classes check their targets.
    ///
    /// `MatchesBinary` validates a constant regular expression in its constructor; that check needs the PCRE port
    /// and is done by the parser ([`Parser::check_matches_regexp`](crate::parser::Parser::check_matches_regexp)).
    ///
    /// # Errors
    ///
    /// The constructors' errors: `You cannot assign a value to "%s".`, `The "defined" test only works with simple
    /// variables.`, `Cannot assign to "%s", only variables can be assigned in sequence destructuring.`,
    /// `Cannot assign to "%s", only variables can be assigned in object/mapping destructuring.`, and the
    /// `LogicException`s of malformed operands.
    pub fn binary(op: BinaryOp, mut left: Expr, mut right: Expr, line: i64) -> Result<Self> {
        let mut test = None;
        match op {
            BinaryOp::And | BinaryOp::Or | BinaryOp::Xor => {
                left = Expr::true_test_wrap(left);
                right = Expr::true_test_wrap(right);
            }
            BinaryOp::Set => {
                // $name = $left->getAttribute('name'); if (!is_string($name)) throw ...
                let name = match &left.kind {
                    ExprKind::ContextVariable(n) | ExprKind::AssignContextVariable(n) => {
                        n.name.clone()
                    }
                    ExprKind::LocalVariable(TempName::String(name))
                    | ExprKind::MacroVariable(MacroVariableExpr {
                        name: TempName::String(name),
                        ..
                    }) => name.clone(),
                    ExprKind::LocalVariable(_) | ExprKind::MacroVariable(_) => {
                        return Err(Error::logic("The \"name\" attribute must be a string."));
                    }
                    _ => {
                        return Err(Error::logic(format!(
                            "Attribute \"name\" does not exist for Node \"{}\".",
                            left.class_name()
                        )));
                    }
                };
                left = Expr::assign_context_variable(name, left.line)?;
            }
            BinaryOp::Elvis => {
                test = Some(Expr::true_test_wrap(left.clone()));
                left.always_defined = true;
            }
            BinaryOp::NullCoalesce => {
                let left_line = left.line;
                let mut defined = Expr::test(
                    left.clone(),
                    internal_test("defined"),
                    Some(Arguments::empty_node()),
                    left_line,
                    TestKind::Defined,
                )?;
                // The null test shares the left operand object, which is marked as always defined afterwards.
                left.always_defined = true;
                if !matches!(left.kind, ExprKind::BlockReference(_)) {
                    let null_test = Expr::test(
                        left.clone(),
                        internal_test("null"),
                        Some(Arguments::empty_node()),
                        left_line,
                        TestKind::Null,
                    )?;
                    defined = Expr::new(
                        ExprKind::Binary(Box::new(BinaryExpr {
                            op: BinaryOp::And,
                            left: defined,
                            right: Expr::unary(UnaryOp::Not, null_test, left_line),
                            test: None,
                        })),
                        left_line,
                    );
                }
                test = Some(defined);
            }
            BinaryOp::SequenceDestructuringSet => {
                if let ExprKind::Array(array) = &left.kind {
                    for (_, value) in &array.elements {
                        if !matches!(
                            value.kind,
                            ExprKind::Empty | ExprKind::AssignContextVariable(_)
                        ) {
                            return Err(Error::syntax(format!(
                                "Cannot assign to \"{}\", only variables can be assigned in sequence destructuring.",
                                value.class_name()
                            ))
                            .with_line(line));
                        }
                    }
                } else {
                    return Err(Error::logic(format!(
                        "Call to undefined method {}::getKeyValuePairs()",
                        left.class_name()
                    )));
                }
            }
            BinaryOp::ObjectDestructuringSet => {
                let ExprKind::Array(array) = &left.kind else {
                    return Err(Error::logic(
                        "Left side must be ArrayExpression for object/mapping destructuring.",
                    ));
                };
                for (key, value) in &array.elements {
                    if !matches!(value.kind, ExprKind::AssignContextVariable(_)) {
                        return Err(Error::syntax(format!(
                            "Cannot assign to \"{}\", only variables can be assigned in object/mapping destructuring.",
                            value.class_name()
                        ))
                        .with_line(line));
                    }
                    if !matches!(key.kind, ExprKind::Constant(_)) {
                        return Err(Error::logic(format!(
                            "Attribute \"value\" does not exist for Node \"{}\".",
                            key.class_name()
                        )));
                    }
                }
            }
            _ => {}
        }
        Ok(Expr::new(
            ExprKind::Binary(Box::new(BinaryExpr {
                op,
                left,
                right,
                test,
            })),
            line,
        ))
    }

    /// Twig: `new ConditionalTernary($test, $left, $right, $lineno)`; the test is wrapped by `TrueTest::wrap()`.
    pub fn conditional_ternary(test: Expr, left: Expr, right: Expr, line: i64) -> Self {
        Expr::new(
            ExprKind::ConditionalTernary(Box::new(ConditionalTernaryExpr {
                test: Expr::true_test_wrap(test),
                left,
                right,
            })),
            line,
        )
    }

    /// Twig: `new GetAttrExpression($node, $attribute, $arguments, $type, $lineno, $nullSafe)`.
    pub fn get_attr(
        node: Expr,
        attribute: Expr,
        arguments: Option<Expr>,
        call_type: CallType,
        line: i64,
        null_safe: bool,
    ) -> Self {
        Expr::new(
            ExprKind::GetAttr(Box::new(GetAttrExpr {
                node,
                attribute,
                arguments,
                call_type,
                ignore_strict_check: false,
                optimizable: !null_safe,
                null_safe,
                is_short_circuited: false,
                var_name: None,
                defined_test: false,
            })),
            line,
        )
    }

    /// Twig: `new MacroReferenceExpression($template, $name, $arguments, $lineno)` (with call parentheses).
    pub fn macro_reference(template: Expr, name: MacroName, arguments: Expr, line: i64) -> Self {
        Expr::new(
            ExprKind::MacroReference(Box::new(MacroReferenceExpr {
                template,
                name,
                arguments,
                has_call_parentheses: true,
                defined_test: false,
            })),
            line,
        )
    }

    /// Twig: `new BlockReferenceExpression($name, $template, $lineno)`.
    pub fn block_reference(name: Expr, template: Option<Expr>, line: i64) -> Self {
        Expr::new(
            ExprKind::BlockReference(Box::new(BlockReferenceExpr {
                name,
                template,
                output: false,
                defined_test: false,
            })),
            line,
        )
    }

    /// Twig: `new ParentExpression($name, $lineno)`.
    pub fn parent(name: impl Into<Vec<u8>>, line: i64) -> Self {
        Expr::new(
            ExprKind::Parent(ParentExpr {
                name: name.into(),
                output: false,
            }),
            line,
        )
    }

    /// Twig: `new EmptyExpression($lineno)`.
    pub fn empty(line: i64) -> Self {
        Expr::new(ExprKind::Empty, line)
    }

    /// Twig: `new CheckToStringNode($expr, $spread)`; the line is the expression's.
    pub fn check_to_string(expr: Expr, spread: bool) -> Self {
        let line = expr.line;
        Expr::new(
            ExprKind::CheckToString(Box::new(CheckToStringExpr { expr, spread })),
            line,
        )
    }

    /// Twig: `new $nodeClass($node, $filter, $arguments, $lineno)` for the filter's node class, including the
    /// rewrite `DefaultFilter` performs and the line fallback of `RawFilter`.
    ///
    /// # Errors
    ///
    /// The `LogicException` of `DefaultFilter` when its arguments have no positional first argument
    /// (`Node "0" does not exist for Node "Twig\Node\Nodes".`), and whatever a custom node class reports.
    pub fn filter(node: Expr, filter: TwigFilter, arguments: Arguments, line: i64) -> Result<Self> {
        match filter.node_class().clone() {
            FilterNodeClass::Filter => Ok(Expr::filter_node(
                node,
                filter,
                arguments,
                line,
                FilterKind::Filter,
            )),
            FilterNodeClass::Escape => Ok(Expr::filter_node(
                node,
                filter,
                arguments,
                line,
                FilterKind::Escape {
                    template_escaper: false,
                },
            )),
            FilterNodeClass::Raw => {
                // `$lineno ?: $node->getTemplateLine()`
                let line = if line == 0 { node.line } else { line };
                Ok(Expr::filter_node(
                    node,
                    filter,
                    arguments,
                    line,
                    FilterKind::Raw,
                ))
            }
            FilterNodeClass::Default => {
                // Twig: DefaultFilter::__construct()
                let node_line = node.line;
                let is_default = filter.name() == b"default";
                let rewrite = is_default
                    && (node.context_variable_name().is_some()
                        || matches!(node.kind, ExprKind::GetAttr(_)));
                let test_node = rewrite.then(|| node.clone());
                let default = Expr::filter_node(
                    node,
                    filter.clone(),
                    arguments.clone(),
                    node_line,
                    FilterKind::Filter,
                );
                let node = if let Some(test_node) = test_node {
                    let test = Expr::test(
                        test_node,
                        internal_test("defined"),
                        Some(Arguments::empty_node()),
                        node_line,
                        TestKind::Defined,
                    )?;
                    let otherwise = if arguments.is_empty() {
                        Expr::constant("", node_line)
                    } else {
                        match arguments.get(&ArgumentKey::Index(0)) {
                            Some(first) => first.clone(),
                            None => {
                                return Err(Error::logic(format!(
                                    "Node \"0\" does not exist for Node \"{}\".",
                                    arguments.class_name()
                                )));
                            }
                        }
                    };
                    Expr::conditional_ternary(test, default, otherwise, node_line)
                } else {
                    default
                };
                Ok(Expr::filter_node(
                    node,
                    filter,
                    arguments,
                    line,
                    FilterKind::Default,
                ))
            }
            FilterNodeClass::Custom(factory) => factory.create(node, filter, arguments, line),
        }
    }

    /// A filter node of a core class without constructor logic (`FilterExpression::__construct()`).
    pub fn filter_node(
        node: Expr,
        filter: TwigFilter,
        arguments: Arguments,
        line: i64,
        kind: FilterKind,
    ) -> Self {
        let filter_name = Expr::constant(ConstantValue::String(filter.name().to_vec()), line);
        Expr::new(
            ExprKind::Filter(Box::new(FilterExpr {
                node,
                filter,
                filter_name,
                arguments,
                kind,
            })),
            line,
        )
    }

    /// Twig: `new $nodeClass($function, $arguments, $lineno)` for the function's node class.
    ///
    /// # Errors
    ///
    /// Whatever a custom node class reports.
    pub fn function(function: TwigFunction, arguments: Arguments, line: i64) -> Result<Self> {
        let kind = match function.node_class().clone() {
            FunctionNodeClass::Function => FunctionKind::Function,
            FunctionNodeClass::Enum => FunctionKind::Enum,
            FunctionNodeClass::EnumCases => FunctionKind::EnumCases,
            FunctionNodeClass::RenderSandboxed => FunctionKind::RenderSandboxed,
            FunctionNodeClass::Custom(factory) => {
                return factory.create(function, arguments, line);
            }
        };
        Ok(Expr::new(
            ExprKind::Function(Box::new(FunctionExpr {
                function,
                arguments,
                kind,
                defined_test: false,
            })),
            line,
        ))
    }

    /// Twig: `new $nodeClass($node, $test, $arguments, $lineno)` for a test of the given core class; see
    /// [`test_for`](Self::test_for) to use the test's own node class.
    ///
    /// # Errors
    ///
    /// `The "defined" test only works with simple variables.` for a `DefinedTest` whose node does not support the
    /// defined test (`DefinedTest::__construct()`).
    pub fn test(
        mut node: Expr,
        test: TwigTest,
        arguments: Option<Arguments>,
        line: i64,
        kind: TestKind,
    ) -> Result<Self> {
        if kind == TestKind::Defined {
            if !node.supports_defined_test() {
                return Err(Error::syntax(
                    "The \"defined\" test only works with simple variables.",
                )
                .with_line(line));
            }
            node.enable_defined_test();
        }
        Ok(Expr::new(
            ExprKind::Test(Box::new(TestExpr {
                node,
                test,
                arguments,
                kind,
            })),
            line,
        ))
    }

    /// Twig: `new $nodeClass($node, $test, $arguments, $lineno)` for the test's node class.
    ///
    /// # Errors
    ///
    /// As [`test`](Self::test), and whatever a custom node class reports.
    pub fn test_for(
        node: Expr,
        test: TwigTest,
        arguments: Option<Arguments>,
        line: i64,
    ) -> Result<Self> {
        let kind = match test.node_class().clone() {
            TestNodeClass::Test => TestKind::Test,
            TestNodeClass::Constant => TestKind::Constant,
            TestNodeClass::Defined => TestKind::Defined,
            TestNodeClass::DivisibleBy => TestKind::DivisibleBy,
            TestNodeClass::Even => TestKind::Even,
            TestNodeClass::NonStringCoercingInput => TestKind::NonStringCoercingInput,
            TestNodeClass::Null => TestKind::Null,
            TestNodeClass::Odd => TestKind::Odd,
            TestNodeClass::SameAs => TestKind::SameAs,
            TestNodeClass::True => TestKind::True,
            TestNodeClass::Custom(factory) => {
                return factory.create(node, test, arguments, line);
            }
        };
        Expr::test(node, test, arguments, line, kind)
    }

    /// Twig: `TrueTest::wrap($node)`: nodes returning a primitive type are kept, others are wrapped in a
    /// `TrueTest` (PHP truthiness with `Markup` objects converted to strings).
    pub fn true_test_wrap(node: Expr) -> Self {
        if node.returns_primitive() {
            return node;
        }
        let line = node.line;
        Expr::new(
            ExprKind::Test(Box::new(TestExpr {
                node,
                test: internal_test("true"),
                arguments: None,
                kind: TestKind::True,
            })),
            line,
        )
    }

    /// Marks the expression as written in parentheses. Twig: `AbstractExpression::setExplicitParentheses()`.
    #[must_use]
    pub fn with_explicit_parentheses(mut self) -> Self {
        self.with_parentheses = true;
        self
    }

    /// Twig: `AbstractExpression::hasExplicitParentheses()`.
    pub fn has_explicit_parentheses(&self) -> bool {
        self.with_parentheses
    }

    /// The fully qualified PHP class name, e.g. `Twig\Node\Expression\ConstantExpression` (used in error
    /// messages such as `Cannot assign to "%s", only variables can be assigned.`).
    pub fn class_name(&self) -> Cow<'static, str> {
        Cow::Borrowed(match &self.kind {
            ExprKind::Constant(_) => "Twig\\Node\\Expression\\ConstantExpression",
            ExprKind::ContextVariable(_) => "Twig\\Node\\Expression\\Variable\\ContextVariable",
            ExprKind::AssignContextVariable(_) => {
                "Twig\\Node\\Expression\\Variable\\AssignContextVariable"
            }
            ExprKind::LocalVariable(_) => "Twig\\Node\\Expression\\Variable\\LocalVariable",
            ExprKind::MacroVariable(_) => "Twig\\Node\\Expression\\Variable\\MacroVariable",
            ExprKind::AssignMacroVariable(_) => {
                "Twig\\Node\\Expression\\Variable\\AssignMacroVariable"
            }
            ExprKind::Array(a) if a.variadic => "Twig\\Node\\Expression\\VariadicExpression",
            ExprKind::Array(_) => "Twig\\Node\\Expression\\ArrayExpression",
            ExprKind::ArrowFunction(_) => "Twig\\Node\\Expression\\ArrowFunctionExpression",
            ExprKind::List(_) => "Twig\\Node\\Expression\\ListExpression",
            ExprKind::Unary(u) => u.op.class_name(),
            ExprKind::Binary(b) => b.op.class_name(),
            ExprKind::ConditionalTernary(_) => {
                "Twig\\Node\\Expression\\Ternary\\ConditionalTernary"
            }
            ExprKind::GetAttr(_) => "Twig\\Node\\Expression\\GetAttrExpression",
            ExprKind::MacroReference(_) => "Twig\\Node\\Expression\\MacroReferenceExpression",
            ExprKind::BlockReference(_) => "Twig\\Node\\Expression\\BlockReferenceExpression",
            ExprKind::Parent(_) => "Twig\\Node\\Expression\\ParentExpression",
            ExprKind::Filter(f) => match f.kind {
                FilterKind::Filter => "Twig\\Node\\Expression\\FilterExpression",
                FilterKind::Default => "Twig\\Node\\Expression\\Filter\\DefaultFilter",
                FilterKind::Escape { .. } => "Twig\\Node\\Expression\\Filter\\EscapeFilter",
                FilterKind::Raw => "Twig\\Node\\Expression\\Filter\\RawFilter",
            },
            ExprKind::Function(f) => match f.kind {
                FunctionKind::Function => "Twig\\Node\\Expression\\FunctionExpression",
                FunctionKind::Enum => "Twig\\Node\\Expression\\FunctionNode\\EnumFunction",
                FunctionKind::EnumCases => {
                    "Twig\\Node\\Expression\\FunctionNode\\EnumCasesFunction"
                }
                FunctionKind::RenderSandboxed => {
                    "Twig\\Node\\Expression\\FunctionNode\\RenderSandboxedFunction"
                }
            },
            ExprKind::Test(t) => t.kind.class_name(),
            ExprKind::Empty => "Twig\\Node\\Expression\\EmptyExpression",
            ExprKind::CheckToString(_) => "Twig\\Node\\CheckToStringNode",
            ExprKind::Custom(custom) => return Cow::Owned(custom.class_name().to_owned()),
        })
    }

    /// The name of a `ContextVariable` or `AssignContextVariable` (PHP's `instanceof ContextVariable`).
    pub fn context_variable_name(&self) -> Option<&[u8]> {
        match &self.kind {
            ExprKind::ContextVariable(n) | ExprKind::AssignContextVariable(n) => Some(&n.name),
            _ => None,
        }
    }

    /// Whether the class implements `ReturnPrimitiveTypeInterface` (which `TrueTest::wrap()` checks).
    pub fn returns_primitive(&self) -> bool {
        match &self.kind {
            ExprKind::Constant(_) | ExprKind::Array(_) | ExprKind::Test(_) => true,
            ExprKind::Binary(b) => b.op.returns_primitive(),
            ExprKind::Unary(u) => u.op == UnaryOp::Not,
            ExprKind::Custom(custom) => custom.returns_primitive(),
            _ => false,
        }
    }

    /// Whether the class implements `SupportDefinedTestInterface` (what the `defined` test accepts).
    pub fn supports_defined_test(&self) -> bool {
        match &self.kind {
            ExprKind::Constant(_)
            | ExprKind::ContextVariable(_)
            | ExprKind::AssignContextVariable(_)
            | ExprKind::Array(_)
            | ExprKind::GetAttr(_)
            | ExprKind::MacroReference(_)
            | ExprKind::BlockReference(_)
            | ExprKind::Function(_) => true,
            ExprKind::Custom(custom) => custom.supports_defined_test(),
            _ => false,
        }
    }

    /// Twig: `enableDefinedTest()`: `GetAttrExpression` also clears `optimizable` and sets `ignore_strict_check` down
    /// its chain of attribute accesses; `FunctionExpression` only enables it for `constant()`.
    pub fn enable_defined_test(&mut self) {
        match &mut self.kind {
            ExprKind::Constant(c) => c.defined_test = true,
            ExprKind::ContextVariable(n) | ExprKind::AssignContextVariable(n) => {
                n.defined_test = true;
            }
            ExprKind::Array(a) => a.defined_test = true,
            ExprKind::GetAttr(g) => {
                g.defined_test = true;
                // GetAttrExpression::changeIgnoreStrictCheck()
                let mut current: &mut GetAttrExpr = g;
                loop {
                    current.optimizable = false;
                    current.ignore_strict_check = true;
                    match &mut current.node.kind {
                        ExprKind::GetAttr(next) => current = next,
                        _ => break,
                    }
                }
            }
            ExprKind::MacroReference(m) => m.defined_test = true,
            ExprKind::BlockReference(b) => b.defined_test = true,
            ExprKind::Function(f) => {
                if f.function.name() == b"constant" {
                    f.defined_test = true;
                }
            }
            ExprKind::Custom(custom) => custom.enable_defined_test(),
            _ => {}
        }
    }

    /// Twig: `isDefinedTestEnabled()` (`false` for classes without the defined test).
    pub fn is_defined_test_enabled(&self) -> bool {
        match &self.kind {
            ExprKind::Constant(c) => c.defined_test,
            ExprKind::ContextVariable(n) | ExprKind::AssignContextVariable(n) => n.defined_test,
            ExprKind::Array(a) => a.defined_test,
            ExprKind::GetAttr(g) => g.defined_test,
            ExprKind::MacroReference(m) => m.defined_test,
            ExprKind::BlockReference(b) => b.defined_test,
            ExprKind::Function(f) => f.defined_test,
            ExprKind::Custom(custom) => custom.is_defined_test_enabled(),
            _ => false,
        }
    }

    /// The node's attributes in Twig's attribute order (`$node->attributes`), as the dump shows them: the class's
    /// own attributes, then the appended ones (`sandboxed_function_name`, `sandboxed_function`,
    /// `with_parentheses`, `always_defined`).
    pub fn attributes(&self) -> Vec<(Cow<'static, str>, AttrValue)> {
        let mut attrs: Vec<(Cow<'static, str>, AttrValue)> = Vec::new();
        let mut push =
            |name: &'static str, value: AttrValue| attrs.push((Cow::Borrowed(name), value));
        match &self.kind {
            ExprKind::Constant(c) => {
                push("value", AttrValue::from(&c.value));
                if c.is_implicit {
                    push("is_implicit", AttrValue::Bool(true));
                }
            }
            ExprKind::ContextVariable(n) | ExprKind::AssignContextVariable(n) => {
                push("name", AttrValue::string(&n.name));
                push("ignore_strict_check", n.ignore_strict_check.into());
                push("always_defined", self.always_defined.into());
            }
            ExprKind::LocalVariable(name) => push("name", AttrValue::from(name)),
            ExprKind::MacroVariable(m) => push("name", AttrValue::from(&m.name)),
            ExprKind::AssignMacroVariable(a) => push("global", a.global.into()),
            ExprKind::Unary(_) => push("with_parentheses", self.with_parentheses.into()),
            ExprKind::GetAttr(g) => {
                push("type", AttrValue::string(g.call_type.as_str()));
                push("ignore_strict_check", g.ignore_strict_check.into());
                push("optimizable", g.optimizable.into());
                push("null_safe", g.null_safe.into());
                push("is_short_circuited", g.is_short_circuited.into());
                push(
                    "var_name",
                    g.var_name
                        .as_ref()
                        .map_or(AttrValue::Null, AttrValue::string),
                );
            }
            ExprKind::MacroReference(m) => push(
                "name",
                match &m.name {
                    MacroName::Static(name) => AttrValue::string(name),
                    MacroName::Dynamic(_) => AttrValue::Null,
                },
            ),
            ExprKind::BlockReference(b) => push("output", b.output.into()),
            ExprKind::Parent(p) => {
                push("output", p.output.into());
                push("name", AttrValue::string(&p.name));
            }
            ExprKind::Filter(f) => {
                push("name", AttrValue::string(f.filter.name()));
                push("type", AttrValue::string("filter"));
                push("twig_callable", AttrValue::Object(f.filter.to_string()));
                if let FilterKind::Escape { template_escaper } = f.kind {
                    push("template_escaper", template_escaper.into());
                }
            }
            ExprKind::Function(f) => {
                push("name", AttrValue::string(f.function.name()));
                push("type", AttrValue::string("function"));
                push("twig_callable", AttrValue::Object(f.function.to_string()));
            }
            ExprKind::Test(t) => {
                push("name", AttrValue::string(t.test.name()));
                push("type", AttrValue::string("test"));
                push("twig_callable", AttrValue::Object(t.test.to_string()));
            }
            ExprKind::CheckToString(c) => push("spread", c.spread.into()),
            ExprKind::Custom(custom) => {
                for (name, value) in custom.attributes() {
                    attrs.push((Cow::Owned(name), value));
                }
            }
            ExprKind::Array(_)
            | ExprKind::ArrowFunction(_)
            | ExprKind::List(_)
            | ExprKind::Binary(_)
            | ExprKind::ConditionalTernary(_)
            | ExprKind::Empty => {}
        }
        if let Some(sandboxed) = &self.sandboxed_function {
            attrs.push((
                Cow::Borrowed("sandboxed_function_name"),
                AttrValue::string(&sandboxed.name),
            ));
            attrs.push((
                Cow::Borrowed("sandboxed_function"),
                AttrValue::Object(sandboxed.function.to_string()),
            ));
        }
        let is_unary = matches!(self.kind, ExprKind::Unary(_));
        if self.with_parentheses && !is_unary {
            attrs.push((Cow::Borrowed("with_parentheses"), AttrValue::Bool(true)));
        }
        let is_name = self.context_variable_name().is_some();
        if self.always_defined && !is_name {
            attrs.push((Cow::Borrowed("always_defined"), AttrValue::Bool(true)));
        }
        attrs
    }
}

impl Default for ArrayExpr {
    fn default() -> Self {
        ArrayExpr::new()
    }
}

impl ArrayExpr {
    /// An empty array (`new ArrayExpression([], $lineno)` without the line).
    pub fn new() -> Self {
        ArrayExpr {
            elements: Vec::new(),
            index: -1,
            defined_test: false,
            variadic: false,
        }
    }

    /// The `ArrayExpression` node at `line`.
    pub fn into_expr(self, line: i64) -> Expr {
        Expr::new(ExprKind::Array(Box::new(self)), line)
    }

    /// Twig: `ArrayExpression::addElement($value, $key)`: without a key, the key is the next auto-index (a
    /// `ConstantExpression` at the value's line).
    pub fn add_element(&mut self, value: Expr, key: Option<Expr>) {
        let key = key.unwrap_or_else(|| {
            self.index += 1;
            Expr::constant(self.index, value.line)
        });
        self.elements.push((key, value));
    }

    /// Twig: `ArrayExpression::isSequence()`: every key is a constant or temporary name identical (`===`) to its
    /// position.
    pub fn is_sequence(&self) -> bool {
        self.elements.iter().enumerate().all(|(i, (key, _))| {
            let expected = i as i64;
            match &key.kind {
                ExprKind::LocalVariable(name)
                | ExprKind::MacroVariable(MacroVariableExpr { name, .. }) => {
                    *name == TempName::Int(expected)
                }
                ExprKind::Constant(c) => c.value == ConstantValue::Int(expected),
                _ => false,
            }
        })
    }
}

impl Expr {
    /// The array data, for an `ArrayExpression`.
    pub fn as_array(&self) -> Option<&ArrayExpr> {
        match &self.kind {
            ExprKind::Array(array) => Some(array),
            _ => None,
        }
    }

    /// The array data, for an `ArrayExpression`.
    pub fn as_array_mut(&mut self) -> Option<&mut ArrayExpr> {
        match &mut self.kind {
            ExprKind::Array(array) => Some(array),
            _ => None,
        }
    }

    /// The constant value, for a `ConstantExpression`.
    pub fn as_constant(&self) -> Option<&ConstantValue> {
        match &self.kind {
            ExprKind::Constant(c) => Some(&c.value),
            _ => None,
        }
    }
}

/// `strtolower($name)` is `true`, `false`, `none` or `null`.
fn is_reserved_constant_name(name: &[u8]) -> bool {
    matches!(
        ascii_lowercase(name).as_slice(),
        b"true" | b"false" | b"none" | b"null"
    )
}

/// `You cannot assign a value to "%s".` (no source: `Parser::parse()` adds it).
fn cannot_assign(name: &[u8], line: i64) -> Error {
    Error::syntax(format!("You cannot assign a value to \"{}\".", Lossy(name))).with_line(line)
}

/// The checks and normalisation of `TempNameExpression::__construct()`.
fn check_temp_name(name: TempNameInput, line: i64) -> Result<TempName> {
    // if ($name && in_array(strtolower($name), ['true', 'false', 'none', 'null'], true))
    if let TempNameInput::String(bytes) = &name
        && !(bytes.is_empty() || bytes.as_slice() == b"0")
        && is_reserved_constant_name(bytes)
    {
        return Err(cannot_assign(bytes, line));
    }
    Ok(TempName::normalize(name))
}
