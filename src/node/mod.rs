use error::Result;
use token::Token;

pub struct Module;

/// Root Twig AST node.
impl Module {
    pub fn from_tokens<'code, I>(tokens: I)
        -> Result<Module>
            where I: IntoIterator<Item=Result<Token<'code>>>
    {
        let mut i = tokens.into_iter();
        i.next();
        Ok(Module)
    }
}
