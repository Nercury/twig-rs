/*!

Produces an abstract syntax tree from a stream of tokens.

*/

mod node;
mod parser;
mod token_parser;

pub use self::node::body;
pub use self::node::expr;
pub use self::node::module::Module;
pub use self::parser::body as body_parser;
pub use self::parser::expr as expr_parser;
pub use self::parser::module as module_parser;
pub use self::parser::{ImportedFunction, Parse, Parser};
pub use self::token_parser::TokenParser;

use crate::environment::ParsingEnvironment;
use crate::error::TemplateResult;
use crate::tokens::{TokenIter, TokenRef};

#[derive(Debug)]
pub struct Block;

#[derive(Debug)]
pub struct Macro;

#[derive(Debug)]
pub struct Trait;

#[derive(Debug)]
pub struct EmbededTemplate;

pub trait TokenParserExtension {
    fn get_tag(&self) -> &'static str;
    fn parse<'p, 'c>(
        &self,
        parser: &mut Parser<'p, 'c>,
        token: TokenRef<'c>,
    ) -> TemplateResult<Option<body::Body<'c>>>;
}

/// Parse given token stream into a node tree.
pub fn parse<'r, 'c>(
    env: &'r ParsingEnvironment,
    tokens: &'r mut TokenIter<'r, 'c>,
) -> TemplateResult<Module<'c>> {
    let mut parser = Parser::new(env, tokens);
    Module::parse(&mut parser)
}
