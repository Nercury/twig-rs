use Token;
use CompiledEnvironment;
use Result;

mod body;
mod expr;
mod module;

pub trait Parse<'code> {
    type Output;

    fn parse<'r, I>(parser: &mut Context<'r, I>)
        -> Result<Self::Output>
    where
        I: Iterator<Item=Result<Token<'code>>>;
}

pub struct Context<'a, I: 'a>
{
    pub env: &'a CompiledEnvironment,
    pub tokens: &'a mut I,
}

impl<'a, I: 'a> Context<'a, I> {
    pub fn new<'r>(
        env: &'r CompiledEnvironment,
        tokens: &'r mut I
    ) -> Context<'r, I> {
        Context {
            env: env,
            tokens: tokens,
        }
    }
}
