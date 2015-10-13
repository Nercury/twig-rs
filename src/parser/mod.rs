use std::iter::Peekable;
use { Token, TokenValue };
use environment::ParsingEnvironment;
use Result;
use error::{ Error, ErrorMessage, Received };
use operator::{ OperatorOptions, OperatorKind };
use lexer::iter::TokenIter;
use token::DebugValue;

pub mod body;
pub mod module;
pub mod expr;

pub trait Parse<'c> {
    type Output;

    fn parse<'p>(parser: &mut Context<'p, 'c>)
        -> Result<Self::Output>;
}

/// Helpers for manipulating and inspecting token iterators when creating AST.
///
/// Has methods to inspect state, like "current" token, and advance to next.
///
/// Current token is actually implemented as "peekable" next token. However,
/// in all parsing code this "peekable" becomes "current".
pub struct Context<'p, 'c: 'p>
{
    pub env: &'p ParsingEnvironment,
    pub tokens: Peekable<&'p mut TokenIter<'p, 'c>>,
}

impl<'p, 'c: 'p> Context<'p, 'c>
{
    pub fn new<'r, 'z>(
        env: &'r ParsingEnvironment,
        tokens: &'r mut TokenIter<'r, 'z>
    ) -> Context<'r, 'z>
    {
        Context {
            env: env,
            tokens: tokens.peekable(),
        }
    }

    /// Get current token or fail.
    ///
    /// Returns current token, does not modify iterator position.
    /// Expects current token to exist, and if it is not (the end of file), returns
    /// UnexpectedEndOfTemplate error.
    pub fn current<'r>(&'r mut self) -> Result<Token<'c>>
    {
        Ok(match self.tokens.peek() {
            Some(&Ok(ref t)) => t.clone(),
            None => return Err(Error::new(ErrorMessage::UnexpectedEndOfTemplate)),
            Some(&Err(ref e)) => return Err(e.clone()),
        })
    }

    /// Get current token or the end of stream.
    ///
    /// Returns current token, does not modify iterator position.
    /// If the end of stream, returns None.
    pub fn maybe_current<'r>(&'r mut self) -> Result<Option<Token<'c>>>
    {
        Ok(match self.tokens.peek() {
            Some(&Ok(ref t)) => Some(t.clone()),
            None => None,
            Some(&Err(ref e)) => return Err(e.clone()),
        })
    }

    /// Advances to the next token and returns previous.
    ///
    /// Expects the next token to exist. If it does not exist (the end of file), returns
    /// UnexpectedEndOfTemplate error.
    pub fn next<'r>(&'r mut self) -> Result<Token<'c>>
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

    /// Advances to the next token if expected token value is the same as current and
    /// returns current.
    ///
    /// Expects these tokens to exist. If they do not exist (the end of file), returns
    /// UnexpectedEndOfTemplate error.
    pub fn skip_to_next_if<'r>(&'r mut self, expected: TokenValue<'c>) -> Result<bool>
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

    /// Expects the current token to match value and advances to next token.
    ///
    /// Error condition same as `expect_match_or`.
    pub fn expect<'r>(&'r mut self, expected: TokenValue<'c>) -> Result<Token<'c>>
    {
        self.expect_match_or(
            |token| if token.value == expected {
                Ok(token.clone())
            } else {
                Err(Error::new_at(
                    ErrorMessage::ExpectedOtherTokenValue((token.value.into(), expected.into())),
                    token.line
                ))
            }
        )
    }

    /// Expects the current token to be name type and advances to next token.
    ///
    /// Returns found name string.
    ///
    /// Error condition same as `expect_match_or`.
    pub fn expect_name<'r>(&'r mut self) -> Result<&'c str>
    {
        self.expect_match_or(
            |token| match token.value {
                TokenValue::Name(name) => Ok(name),
                _ => Err(Error::new_at(
                    ErrorMessage::ExpectedTokenTypeButReceived(
                        (DebugValue::Name("".into()), Received::Token(token.value.into()))
                    ),
                    token.line
                ))
            }
        )
    }

    /// Expects the current token to match value and advances to the next token.
    ///
    /// Error condition same as `expect_match_or`.
    pub fn expect_or_error<'r>(&'r mut self, expected: TokenValue<'c>, error_message: ErrorMessage) -> Result<Token<'c>>
    {
        self.expect_match_or(
            |token| if token.value == expected {
                Ok(token.clone())
            } else {
                Err(Error::new_at(
                    error_message,
                    token.line
                ))
            }
        )
    }

    /// Expects the current token to pass `check` and advances to next token.
    ///
    /// Expects these tokens (current and next) to exist. If they do not exist (the end of file),
    /// returns `UnexpectedEndOfTemplate` error.
    pub fn expect_match_or<'r, C, T>(&'r mut self, check: C) -> Result<T>
        where
            C: for<'a> FnOnce(&'a Token<'c>) -> Result<T>
    {
        let res = match self.tokens.peek() {
            Some(&Ok(ref t)) => {
                check(&t)
            },
            None => return Err(Error::new(ErrorMessage::UnexpectedEndOfTemplate)),
            Some(&Err(ref e)) => return Err(e.clone()),
        };
        try!(self.next());

        res
    }

    /// Test the current token to match value.
    ///
    /// Expects these token to exist. If it does not exist (the end of file), returns
    /// UnexpectedEndOfTemplate error.
    pub fn test<'r>(&'r mut self, expected: TokenValue<'c>) -> Result<bool>
    {
        match self.tokens.peek() {
            Some(&Ok(ref t)) => {
                if t.value == expected {
                    Ok(true)
                } else {
                    Ok(false)
                }
            },
            None => Err(Error::new(ErrorMessage::UnexpectedEndOfTemplate)),
            Some(&Err(ref e)) => Err(e.clone()),
        }
    }

    /// Returns options structure for specified operator.
    ///
    /// Operator must exist in environment, otherwise panics.
    pub fn get_operator_options<'r>(&'r self, op_str: &'c str) -> OperatorOptions {
        self.env.operators
            .get(op_str)
            .cloned()
            .unwrap_or(OperatorOptions { precedence: None, kind: OperatorKind::Other })
    }
}
