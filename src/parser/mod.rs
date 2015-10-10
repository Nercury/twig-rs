use std::iter::Peekable;
use Token;
use environment::ParsingEnvironment;
use Result;
use error::{ Error, ErrorMessage };
use operator::OperatorOptions;

mod body;
mod expr;
mod module;

pub trait Parse<'c> {
    type Output;

    fn parse<'p, I>(parser: &mut Context<'p, I>)
        -> Result<Self::Output>
    where
        I: Iterator<Item=Result<Token<'c>>>;
}

pub struct Context<'p, I: 'p>
    where
        I: Iterator
{
    pub env: &'p ParsingEnvironment,
    pub tokens: Peekable<&'p mut I>,
}

impl<'p, I: 'p> Context<'p, I>
    where
        I: Iterator
{
    pub fn new<'r, 'c>(
        env: &'r ParsingEnvironment,
        tokens: &'r mut I
    ) -> Context<'r, I>
        where I: Iterator<Item=Result<Token<'c>>>
    {
        Context {
            env: env,
            tokens: tokens.peekable(),
        }
    }

    pub fn current<'r, 'c>(&'r mut self) -> Result<Token<'c>>
        where I: Iterator<Item=Result<Token<'c>>>
    {
        Ok(match self.tokens.peek() {
            Some(&Ok(ref t)) => t.clone(),
            None => return Err(Error::new(ErrorMessage::UnexpectedEndOfTemplate)),
            Some(&Err(ref e)) => return Err(e.clone()),
        })
    }

    pub fn next<'r, 'c>(&'r mut self) -> Result<Token<'c>>
        where I: Iterator<Item=Result<Token<'c>>>
    {
        Ok(match self.tokens.next() {
            Some(Ok(t)) => t,
            None => return Err(Error::new(ErrorMessage::UnexpectedEndOfTemplate)),
            Some(Err(e)) => return Err(e),
        })
    }

    pub fn get_operator_options<'r, 'c>(&'r self, op_str: &'c str) -> &'r OperatorOptions {
        self.env.operators
            .get(op_str)
            .expect("twig bug: operator that was lexed was not found when parsing")
    }
}
