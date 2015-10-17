mod body;
mod expr;
mod module;

pub use self::body::{ Body, ImportTarget };
pub use self::expr::{ Expr, ExprValue, ExprConstant, ExprCallType };
pub use self::module::Module;

#[derive(Debug)]
pub struct Block;

#[derive(Debug)]
pub struct Macro;

#[derive(Debug)]
pub struct Trait;

#[derive(Debug)]
pub struct EmbededTemplate; // reference to another module
