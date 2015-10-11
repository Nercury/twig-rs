use std::iter::Peekable;
use { Token, TokenValue };
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

    pub fn maybe_current<'r, 'c>(&'r mut self) -> Result<Option<Token<'c>>>
        where I: Iterator<Item=Result<Token<'c>>>
    {
        Ok(match self.tokens.peek() {
            Some(&Ok(ref t)) => Some(t.clone()),
            None => None,
            Some(&Err(ref e)) => return Err(e.clone()),
        })
    }

    pub fn next<'r, 'c>(&'r mut self) -> Result<Token<'c>>
        where I: Iterator<Item=Result<Token<'c>>>
    {
        let token = match self.tokens.peek() {
            Some(&Ok(ref t)) => t.clone(),
            None => return Err(Error::new(ErrorMessage::UnexpectedEndOfTemplate)),
            Some(&Err(ref e)) => return Err(e.clone()),
        };

        match self.tokens.next() {
            None => return Err(Error::new(ErrorMessage::UnexpectedEndOfTemplate)),
            Some(Err(e)) => return Err(e),
            _ => (),
        };

        Ok(token)
    }

    pub fn skip_to_next_if<'r, 'c>(&'r mut self, expected: TokenValue<'c>) -> Result<bool>
        where I: Iterator<Item=Result<Token<'c>>>
    {
        let skip = match self.tokens.peek() {
            Some(&Ok(ref token)) if token.value == expected => true,
            _ => false,
        };
        if skip {
            match self.tokens.next() {
                Some(Ok(_)) => Ok(true),
                None => return Err(Error::new(ErrorMessage::UnexpectedEndOfTemplate)),
                Some(Err(e)) => return Err(e),
            }
        } else {
            Ok(false)
        }
    }

    pub fn expect<'r, 'c>(&'r mut self, expected: TokenValue<'c>) -> Result<Token<'c>>
        where I: Iterator<Item=Result<Token<'c>>>
    {
        let token = match self.tokens.peek() {
            Some(&Ok(ref t)) if t.value == expected => t.clone(),
            Some(&Ok(ref t)) => return Err(Error::new_at(
                ErrorMessage::ExpectedOtherTokenValue((t.value.into(), expected.into())),
                t.line
            )),
            None => return Err(Error::new(ErrorMessage::UnexpectedEndOfTemplate)),
            Some(&Err(ref e)) => return Err(e.clone()),
        };
        try!(self.next());

        Ok(token)
    }

    pub fn get_operator_options<'r, 'c>(&'r self, op_str: &'c str) -> &'r OperatorOptions {
        self.env.operators
            .get(op_str)
            .expect("twig bug: operator that was lexed was not found when parsing")
    }
}
