//! Twig: `Twig\TokenParser\GuardTokenParser` (TWIG/src/TokenParser/GuardTokenParser.php).

use super::{
    TokenParser, expect_block_end, subparse_ignoring_unknown_callables_until, subparse_until,
    syntax_error, token_value,
};
use crate::ast::{Node, Nodes};
use crate::error::{ErrorKind, Result};
use crate::lexer::{Token, TokenType, TokenValues};
use crate::parser::Parser;

/// A compile-time condition on the availability of a callable:
/// `{% guard function|filter|test name [second word] %}...[{% else %}...]{% endguard %}`.
///
/// Both branches are parsed; the one that is not taken is parsed while ignoring unknown functions, filters and tests,
/// and everything it registers (blocks, macros, imports, ...) is discarded.
#[derive(Debug, Clone, Copy, Default)]
pub struct GuardTokenParser;

impl TokenParser for GuardTokenParser {
    fn tag(&self) -> &str {
        "guard"
    }

    /// Twig: `GuardTokenParser::parse()`.
    fn parse(&self, parser: &mut Parser<'_>, token: &Token) -> Result<Option<Node>> {
        let type_token = parser.stream_mut().expect(TokenType::Name, None, None)?;
        let callable_type = token_value(&type_token);
        if !matches!(callable_type.as_slice(), b"function" | b"filter" | b"test") {
            return Err(syntax_error(
                parser,
                format!(
                    "Supported guard types are function, filter and test, \"{}\" given.",
                    String::from_utf8_lossy(&callable_type)
                ),
                type_token.line(),
            ));
        }

        let name_token = parser.stream_mut().expect(TokenType::Name, None, None)?;
        let mut name = token_value(&name_token);
        if callable_type == b"test" && parser.stream().test(TokenType::Name) {
            // try 2-words tests
            name.push(b' ');
            name.extend_from_slice(&token_value(parser.current_token()));
            parser.stream_mut().next()?;
        }

        // `$this->parser->getEnvironment()->$method($name)`: a SyntaxError of an undefined-callable callback means
        // that the callable does not exist.
        let env = parser.environment();
        let lookup = match callable_type.as_slice() {
            b"function" => env.get_function(&name).map(|f| f.is_some()),
            b"filter" => env.get_filter(&name).map(|f| f.is_some()),
            _ => env.get_test(&name).map(|t| t.is_some()),
        };
        let exists = match lookup {
            Ok(exists) => exists,
            Err(error) if error.kind() == ErrorKind::Syntax => false,
            Err(error) => return Err(error),
        };

        expect_block_end(parser)?;

        let fork = TokenValues::AnyOf(&["else", "endguard"]);
        let body = if exists {
            subparse_until(parser, self.tag(), fork, false)?
        } else {
            subparse_ignoring_unknown_callables_until(parser, self.tag(), fork, false)?;
            Node::empty(0)
        };

        let mut else_ = Node::empty(0);
        if parser.stream_mut().next()?.value().as_bytes() == Some(b"else") {
            expect_block_end(parser)?;
            let end = TokenValues::AnyOf(&["endguard"]);
            if exists {
                subparse_ignoring_unknown_callables_until(parser, self.tag(), end, true)?;
            } else {
                else_ = subparse_until(parser, self.tag(), end, true)?;
            }
        }

        expect_block_end(parser)?;

        Ok(Some(Node::nodes(
            Nodes::from_list([if exists { body } else { else_ }]),
            token.line(),
        )))
    }

    fn class_name(&self) -> &str {
        "Twig\\TokenParser\\GuardTokenParser"
    }
}
