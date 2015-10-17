mod parser;
mod node;
mod token_parser;

pub use self::node::body::{ Body, ImportTarget };
pub use self::node::expr::{ Expr, ExprValue, ExprConstant, ExprCallType };
pub use self::node::module::Module;
pub use self::token_parser::{ TokenParser };
pub use self::parser::{ Parser, Parse, ImportedFunction };
pub use self::parser::body as body_parser;
pub use self::parser::expr as expr_parser;
pub use self::parser::module as module_parser;

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
    fn parse<'p, 'c>(&self, parser: &mut Parser<'p, 'c>, token: TokenRef<'c>)
        -> Result<Option<Body<'c>>>;
}
