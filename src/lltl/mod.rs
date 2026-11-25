//! LLTL, the low-level template language: what a visited Twig syntax tree is compiled into before the
//! [virtual machine](crate::vm) runs it.
//!
//! Twig compiles a template into a PHP class; twig-rs compiles it into a [`Program`]: one [`Unit`] of
//! [instructions](Instr) per method Twig would generate (`doDisplay()`, each `block_NAME()`, each macro closure, a
//! dynamic `doGetParent()`, each arrow-function closure), a constant pool of [`twig_value`] values, the table of
//! filters, functions and tests the code calls, and the compile-time facts the runtime needs (blocks, macros, parent,
//! traits, embedded templates). The [code generator](compile) mirrors every node's `compile()` method, in Twig's compile
//! order, so that evaluation order, compile-time flags and line attribution come out identical; see `plans/engine.md`.
//!
//! Every unit has a run-length [`LineTable`] with entries only where Twig's `Compiler::addDebugInfo()` emits a
//! `// line N` marker. An error without an explicit line takes the line of the instruction that failed, in the
//! innermost frame of the template that handles it, which is what Twig's `Error::guess()` computes from the PHP
//! backtrace.

mod codegen;
mod instr;
mod program;

pub use codegen::{CompileOptions, EnumResolver, compile};
pub use instr::{
    BinaryOp, BlockOp, ConstId, GetAttrOp, Instr, Label, LocalId, NameMode, UnitId, YieldArgs,
};
pub use program::{
    BlockDef, FileId, LineTable, MacroDef, MacroParam, ParentDef, Program, SecurityLists, TraitDef,
    Unit, UnitKind,
};
