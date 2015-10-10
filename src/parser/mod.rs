use Token;
use environment::ParsingEnvironment;
use Result;
use error::{ Error, ErrorMessage };
use operator::OperatorOptions;

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
    pub env: &'a ParsingEnvironment,
    pub tokens: &'a mut I,
}

impl<'a, I: 'a> Context<'a, I> {
    pub fn new<'r>(
        env: &'r ParsingEnvironment,
        tokens: &'r mut I
    ) -> Context<'r, I> {
        Context {
            env: env,
            tokens: tokens,
        }
    }

    pub fn next<'code>(&mut self) -> Result<Token<'code>>
        where
            I: Iterator<Item=Result<Token<'code>>>
    {
        Ok(match self.tokens.next() {
            Some(Ok(t)) => t,
            None => return Err(Error::new(ErrorMessage::UnexpectedEndOfTemplate)),
            Some(Err(e)) => return Err(e),
        })
    }

    pub fn get_operator_options(&'a self, op_str: &'a str) -> &'a OperatorOptions {
        self.env.operators
            .get(op_str)
            .expect("twig bug: operator that was lexed was not found when parsing")
    }
}
