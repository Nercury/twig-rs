/*!

Produces an abstract syntax tree from a stream of tokens.

*/

mod parser;
mod node;
mod token_parser;

pub use self::node::body;
pub use self::node::expr;
pub use self::node::module::Module;
pub use self::token_parser::{ TokenParser };
pub use self::parser::{ Parser, Parse, ImportedFunction };
pub use self::parser::body as body_parser;
pub use self::parser::expr as expr_parser;
pub use self::parser::module as module_parser;

use environment::ParsingEnvironment;
use tokens::{ TokenRef, TokenIter };
use error::TemplateResult;

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
        -> TemplateResult<Option<body::Body<'c>>>;
}

/// Parse given token stream into a node tree.
pub fn parse<'r, 'c>(env: &'r ParsingEnvironment, tokens: &'r mut TokenIter<'r, 'c>) -> TemplateResult<Module<'c>> {
    let mut parser = Parser::new(
        env, tokens
    );
    Module::parse(&mut parser)
}
