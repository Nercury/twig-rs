use std::iter::Peekable;
use std::collections::HashMap;
use tokens::{ TokenRef, TokenValueRef, TokenValue, TokenIter };
use environment::ParsingEnvironment;
use error::{ TemplateResult, TemplateError, Received };
use operator::{ OperatorOptions, OperatorKind };
use uuid::Uuid;

pub mod body;
pub mod module;
pub mod expr;

#[derive(Copy, Clone)]
pub struct ImportedFunction<'c> {
    pub uuid: Uuid,
    pub name: &'c str,
    pub alias: &'c str,
}

impl<'c> ImportedFunction<'c> {
    pub fn new<'r>(uuid: Uuid, alias: &'r str, name: &'r str) -> ImportedFunction<'r> {
        ImportedFunction {
            uuid: uuid, name: name, alias: alias
        }
    }
}

pub struct ImportedSymbols<'c> {
    pub functions: HashMap<&'c str, ImportedFunction<'c>>
}

impl<'c> ImportedSymbols<'c> {
    pub fn new<'r>() -> ImportedSymbols<'r> {
        ImportedSymbols {
            functions: HashMap::new()
        }
    }
}

pub trait Parse<'c> {
    type Output;

    fn parse<'p>(parser: &mut Parser<'p, 'c>)
        -> TemplateResult<Self::Output>;
}

/// Helpers for manipulating and inspecting token iterators when creating AST.
///
/// Has methods to inspect state, like "current" token, and advance to next.
///
/// Current token is actually implemented as "peekable" next token. However,
/// in all parsing code this "peekable" becomes "current".
pub struct Parser<'p, 'c: 'p>
{
    /// Project options for parsing, containing data collected from all added
    /// extensions.
    pub env: &'p ParsingEnvironment,
    /// Token stream.
    pub tokens: Peekable<&'p mut TokenIter<'p, 'c>>,
    /// Imported symbol stack.
    pub imported_symbols: Vec<ImportedSymbols<'c>>,
}

impl<'p, 'c: 'p> Parser<'p, 'c>
{
    pub fn new<'r, 'z>(
        env: &'r ParsingEnvironment,
        tokens: &'r mut TokenIter<'r, 'z>
    ) -> Parser<'r, 'z>
    {
        Parser {
            env: env,
            tokens: tokens.peekable(),
            imported_symbols: vec![ImportedSymbols::new()],
        }
    }

    pub fn push_local_scope<'r>(&'r mut self) {
        self.imported_symbols.push(ImportedSymbols::new());
    }

    pub fn pop_local_scope<'r>(&'r mut self) {
        self.imported_symbols.pop();
    }

    /// Registers pecified alias as imported function, further parsing might
    /// depend on this (use this function).
    pub fn add_imported_function<'r>(&'r mut self, alias: &'c str, name: &'c str) -> Uuid {
        let uuid = Uuid::new_v4();
        self.imported_symbols
            .last_mut().unwrap()
            .functions
                .insert(alias, ImportedFunction::new(uuid.clone(), alias, name));
        uuid
    }

    /// Finds a function that was previosly imported in this or parent scope.
    pub fn get_imported_function<'r>(&'r self, name: &str) -> Option<ImportedFunction<'c>> {
        for symbols in &self.imported_symbols {
            if let Some(found) = symbols.functions.get(name) {
                return Some(*found);
            }
        }
        None
    }

    /// Get current token or fail.
    ///
    /// Returns current token, does not modify iterator position.
    /// Expects current token to exist, and if it is not (the end of file), returns
    /// UnexpectedEndOfTemplate error.
    pub fn current<'r>(&'r mut self) -> TemplateResult<TokenRef<'c>>
    {
        Ok(match self.tokens.peek() {
            Some(&Ok(ref t)) => t.clone(),
            None => return Err(TemplateError::UnexpectedEndOfTemplate.at(1)),
            Some(&Err(ref e)) => return Err(e.clone()),
        })
    }

    /// Get current token or the end of stream.
    ///
    /// Returns current token, does not modify iterator position.
    /// If the end of stream, returns None.
    pub fn maybe_current<'r>(&'r mut self) -> TemplateResult<Option<TokenRef<'c>>>
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
    pub fn next<'r>(&'r mut self) -> TemplateResult<TokenRef<'c>>
    {
        let token = match self.tokens.peek() {
            Some(&Ok(ref t)) => t.clone(),
            None => return Err(TemplateError::UnexpectedEndOfTemplate.at(1)),
            Some(&Err(ref e)) => return Err(e.clone()),
        };

        match self.tokens.next() {
            None => return Err(TemplateError::UnexpectedEndOfTemplate.at(token.line)),
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
    pub fn skip_to_next_if<'r>(&'r mut self, expected: TokenValueRef<'c>) -> TemplateResult<bool>
    {
        let (line, skip) = {
            let token = match self.tokens.peek() {
                Some(&Ok(ref token)) => token,
                _ => return Err(TemplateError::UnexpectedEndOfTemplate.at(1)),
            };
            (token.line, token.value == expected)
        };
        if skip {
            match self.tokens.next() {
                Some(Ok(_)) => Ok(true),
                None => return Err(TemplateError::UnexpectedEndOfTemplate.at(line)),
                Some(Err(e)) => return Err(e),
            }
        } else {
            Ok(false)
        }
    }

    /// Expects the current token to match value and advances to next token.
    ///
    /// Error condition same as `expect_match_or`.
    pub fn expect<'r>(&'r mut self, expected: TokenValueRef<'c>) -> TemplateResult<TokenRef<'c>>
    {
        self.expect_match_or(
            |token| if token.value == expected {
                Ok(token.clone())
            } else {
                Err(
                    TemplateError::ExpectedOtherTokenValue((token.value.into(), expected.into()))
                        .at(token.line)
                )
            }
        )
    }

    /// Expects the current token to be name type and advances to next token.
    ///
    /// Returns found name string.
    ///
    /// Error condition same as `expect_match_or`.
    pub fn expect_name<'r>(&'r mut self) -> TemplateResult<&'c str>
    {
        self.expect_match_or(
            |token| match token.value {
                TokenValueRef::Name(name) => Ok(name),
                _ => Err(
                    TemplateError::ExpectedTokenTypeButReceived(
                        (TokenValue::Name("".into()), Received::Token(token.value.into()))
                    ).at(token.line)
                )
            }
        )
    }

    /// Expects the current token to match value and advances to the next token.
    ///
    /// Error condition same as `expect_match_or`.
    pub fn expect_or_error<'r>(&'r mut self, expected: TokenValueRef<'c>, error_message: TemplateError) -> TemplateResult<TokenRef<'c>>
    {
        self.expect_match_or(
            |token| if token.value == expected {
                Ok(token.clone())
            } else {
                Err(error_message.at(token.line))
            }
        )
    }

    /// Expects the current token to pass `check` and advances to next token.
    ///
    /// Expects these tokens (current and next) to exist. If they do not exist (the end of file),
    /// returns `UnexpectedEndOfTemplate` error.
    pub fn expect_match_or<'r, C, T>(&'r mut self, check: C) -> TemplateResult<T>
        where
            C: for<'a> FnOnce(&'a TokenRef<'c>) -> TemplateResult<T>
    {
        let res = match self.tokens.peek() {
            Some(&Ok(ref t)) => {
                check(&t)
            },
            None => return Err(TemplateError::UnexpectedEndOfTemplate.at(1)),
            Some(&Err(ref e)) => return Err(e.clone()),
        };
        try!(self.next());

        res
    }

    /// Test the current token to match value.
    ///
    /// Expects these token to exist. If it does not exist (the end of file), returns
    /// UnexpectedEndOfTemplate error.
    pub fn test<'r>(&'r mut self, expected: TokenValueRef<'c>) -> TemplateResult<bool>
    {
        match self.tokens.peek() {
            Some(&Ok(ref t)) => {
                if t.value == expected {
                    Ok(true)
                } else {
                    Ok(false)
                }
            },
            None => Err(TemplateError::UnexpectedEndOfTemplate.at(1)),
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
