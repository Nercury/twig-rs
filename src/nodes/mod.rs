mod node;
mod token_parser;

pub use self::node::body::{ Body, ImportTarget };
pub use self::node::expr::{ Expr, ExprValue, ExprConstant, ExprCallType };
pub use self::node::module::Module;
pub use self::token_parser::{ TokenParser };

use parser::Context;
use tokens::TokenRef;
use Result;

#[derive(Debug)]
pub struct Block;

#[derive(Debug)]
pub struct Macro;

#[derive(Debug)]
pub struct Trait;

#[derive(Debug)]
pub struct EmbededTemplate;

pub trait TokenParserExtension
{
    fn get_tag(&self) -> &'static str;
    fn parse<'p, 'c>(&self, parser: &mut Context<'p, 'c>, token: TokenRef<'c>)
        -> Result<Option<Body<'c>>>;
}
