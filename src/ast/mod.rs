//! The abstract syntax tree: a static mirror of Twig 3.31.1's node classes (TWIG/src/Node/).
//!
//! - [`Node`] / [`NodeKind`]: statement and structural nodes (`ModuleNode`, `PrintNode`, `IfNode`, ...).
//! - [`Expr`] / [`ExprKind`]: expression nodes (`ConstantExpression`, `GetAttrExpression`, the binary, unary,
//!   filter, function and test families, ...).
//!
//! Attributes are typed fields, including the ones later passes set (`always_defined`, `ignore_strict_check`,
//! `optimizable`, `with_loop`, `output`, `strategy`, ...). Line numbers are the ones Twig assigns. Constructors
//! port the PHP constructors with their checks and rewrites (`SetNode`, `IfNode`, `NullCoalesceBinary`,
//! `DefaultFilter`, ...).
//!
//! [`Node::children`], [`Expr::children`] and their `_mut` variants list children with Twig's keys and in Twig's
//! order; the node traverser ([`crate::visitor`]) walks the tree through them. Extensions add node classes through
//! [`CustomNode`] and [`CustomExpr`]. [`dump`] prints a tree in the format the oracle vectors use.
//!
//! Deprecated Twig classes that only legacy PHP code creates (`NameExpression`, `AssignNameExpression`,
//! `TempNameExpression`, `TemplateVariable`, `AssignTemplateVariable`, `ConditionalExpression`,
//! `NullCoalesceExpression`, `MethodCallExpression`, `InlinePrint`) have no counterpart; the profiler nodes come
//! with the profiler extension.

pub mod dump;
mod expr;
mod node;
mod value;
mod visit;

pub use expr::{
    ArgumentKey, Arguments, ArrayExpr, ArrowFunctionExpr, AssignMacroVariableExpr, BinaryExpr,
    BinaryOp, BlockReferenceExpr, CallType, CheckToStringExpr, ConditionalTernaryExpr,
    ConstantExpr, Expr, ExprKind, FilterExpr, FilterKind, FunctionExpr, FunctionKind, GetAttrExpr,
    ListExpr, MacroName, MacroReferenceExpr, MacroVariableExpr, NameExpr, ParentExpr,
    SandboxedFunction, TestExpr, TestKind, UnaryExpr, UnaryOp,
};
pub use node::{
    AutoEscapeNode, BlockNode, BlockReferenceNode, CaptureNode, CheckSecurityNode, DeprecatedNode,
    DoNode, EmbedNode, ForElseNode, ForLoopNode, ForNode, IfNode, ImportNode, IncludeNode,
    MacroDeclarationNode, MacroNode, ModuleNode, Node, NodeKey, NodeKind, Nodes, PrintNode,
    SandboxNode, SetNode, TextNode, TypeNode, TypesNode, VARARGS_NAME, WithNode,
};
pub use value::{
    ArrayKey, AttrValue, ConstantValue, RESERVED_NAME_PREFIX, RESERVED_NAMES, TempName,
    TempNameInput, php_var_export_float,
};
pub use visit::{ChildKey, CustomExpr, CustomNode, NodeMut, NodeRef};

pub(crate) use value::Lossy;
