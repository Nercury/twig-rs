//! Tag parsers: Twig's `Twig\TokenParser\*` (TWIG/src/TokenParser/).
//!
//! A [`TokenParser`] parses one tag. [`Parser::subparse`](crate::parser::Parser::subparse) dispatches every
//! `{% name ... %}` to the token parser registered for `name`, positioned after the name; the token parser consumes
//! the rest of the tag (and its body and end tag, for tags with a body) and returns the node.
//!
//! The core tags are registered by [`CoreExtension`](crate::extension::CoreExtension) (`apply`, `for`, `if`,
//! `extends`, `include`, `block`, `use`, `macro`, `import`, `from`, `set`, `types`, `flush`, `do`, `embed`, `with`,
//! `deprecated`, `guard`), [`EscaperExtension`](crate::extension::EscaperExtension) (`autoescape`) and
//! [`SandboxExtension`](crate::extension::SandboxExtension) (the deprecated `sandbox`). `verbatim` and `line` are
//! handled by the lexer.

mod apply;
mod autoescape;
mod block;
mod deprecated;
mod do_;
mod embed;
mod extends;
mod flush;
mod for_;
mod from;
mod guard;
mod if_;
mod import;
mod include;
mod macro_;
mod sandbox;
mod set;
mod types;
mod use_;
mod with;

pub use apply::ApplyTokenParser;
pub use autoescape::AutoEscapeTokenParser;
pub use block::BlockTokenParser;
pub use deprecated::DeprecatedTokenParser;
pub use do_::DoTokenParser;
pub use embed::EmbedTokenParser;
pub use extends::ExtendsTokenParser;
pub use flush::FlushTokenParser;
pub use for_::ForTokenParser;
pub use from::FromTokenParser;
pub use guard::GuardTokenParser;
pub use if_::IfTokenParser;
pub use import::ImportTokenParser;
pub use include::IncludeTokenParser;
pub use macro_::MacroTokenParser;
pub use sandbox::SandboxTokenParser;
pub use set::SetTokenParser;
pub use types::TypesTokenParser;
pub use use_::UseTokenParser;
pub use with::WithTokenParser;

use std::sync::Arc;

use crate::ast::{Expr, Node, Nodes};
use crate::error::Result;
use crate::lexer::{Token, TokenType, TokenValues, matches_name};
use crate::parser::{EndTagTest, Parser, add_expr_documentation};

/// A tag parser. Twig: `Twig\TokenParser\TokenParserInterface` (TWIG/src/TokenParser/TokenParserInterface.php).
///
/// Twig hands the parser to the token parser with `setParser()`; here it is an argument of [`parse`](Self::parse).
pub trait TokenParser: Send + Sync {
    /// The tag name. Twig: `getTag()`.
    fn tag(&self) -> &str;

    /// Parses the tag; `token` is the tag-name token and the stream is positioned after it. Returning `None` is
    /// Twig's deprecated `null` return (the parser reports the deprecation and adds no node).
    ///
    /// Twig: `parse(Token $token)`.
    ///
    /// # Errors
    ///
    /// The syntax errors of the tag.
    fn parse(&self, parser: &mut Parser<'_>, token: &Token) -> Result<Option<Node>>;

    /// Whether the tag is allowed in the sandbox without being listed in the policy.
    /// Twig: `isAlwaysAllowedInSandbox()`.
    fn is_always_allowed_in_sandbox(&self) -> bool {
        false
    }

    /// The PHP class name, used in the deprecation for a `null` return.
    fn class_name(&self) -> &str {
        std::any::type_name::<Self>()
    }
}

/// The token parsers of `CoreExtension::getTokenParsers()`, in Twig's order (which decides the order of "Did you
/// mean" suggestions for unknown tags).
pub fn core_token_parsers() -> Vec<Arc<dyn TokenParser>> {
    vec![
        Arc::new(ApplyTokenParser),
        Arc::new(ForTokenParser),
        Arc::new(IfTokenParser),
        Arc::new(ExtendsTokenParser),
        Arc::new(IncludeTokenParser),
        Arc::new(BlockTokenParser),
        Arc::new(UseTokenParser),
        Arc::new(MacroTokenParser),
        Arc::new(ImportTokenParser),
        Arc::new(FromTokenParser),
        Arc::new(SetTokenParser),
        Arc::new(TypesTokenParser),
        Arc::new(FlushTokenParser),
        Arc::new(DoTokenParser),
        Arc::new(EmbedTokenParser),
        Arc::new(WithTokenParser),
        Arc::new(DeprecatedTokenParser),
        Arc::new(GuardTokenParser),
    ]
}

/// Parses an assignment target list like `a, b`: names, or operator tokens that start like a name (`in`, `not in`,
/// `b-and`, ...) taken as variable names. Each target is an `AssignContextVariable` with the documentation of its
/// token.
///
/// Twig: `AbstractTokenParser::parseAssignmentExpression()` (TWIG/src/TokenParser/AbstractTokenParser.php).
///
/// # Errors
///
/// `Only variables can be assigned to. Unexpected token ...` and `You cannot assign a value to "%s".`.
pub fn parse_assignment_expression(parser: &mut Parser<'_>) -> Result<Nodes> {
    let mut targets = Nodes::new();
    loop {
        let token = parser.current_token().clone();
        if parser.stream().test(TokenType::Operator)
            && matches_name(token.value().as_bytes().unwrap_or_default())
        {
            // in this context, string operators are variable names
            parser.stream_mut().next()?;
        } else {
            parser.stream_mut().expect(
                TokenType::Name,
                None,
                Some("Only variables can be assigned to"),
            )?;
        }
        let mut target = Expr::assign_context_variable(token_value(&token), token.line())?;
        add_expr_documentation(&mut target, &[&token]);
        targets.push(Node::expression(target));
        if parser
            .stream_mut()
            .next_if_value(TokenType::Punctuation, ",")?
            .is_none()
        {
            break;
        }
    }
    Ok(targets)
}

/// `$token->getValue()` as bytes (names and strings; numbers in their PHP string form).
pub(crate) fn token_value(token: &Token) -> Vec<u8> {
    crate::parser::expression::prefix::token_bytes(token)
}

/// `$this->parser->subparse([$this, 'decideXxx'], $dropNeedle)` where the decision method is
/// `$token->test($values)` (a name token with one of `values`).
pub(crate) fn subparse_until(
    parser: &mut Parser<'_>,
    tag: &str,
    values: TokenValues<'_>,
    drop_needle: bool,
) -> Result<Node> {
    let test = move |token: &Token| token.test_value(TokenType::Name, values);
    parser.subparse(
        Some(&EndTagTest {
            tag: Some(tag),
            test: &test,
        }),
        drop_needle,
    )
}

/// `$this->parser->subparseIgnoreUnknownTwigCallables([$this, 'decideXxx'], $dropNeedle)` (see
/// [`subparse_until`]).
pub(crate) fn subparse_ignoring_unknown_callables_until(
    parser: &mut Parser<'_>,
    tag: &str,
    values: TokenValues<'_>,
    drop_needle: bool,
) -> Result<()> {
    let test = move |token: &Token| token.test_value(TokenType::Name, values);
    parser.subparse_ignore_unknown_twig_callables(
        Some(&EndTagTest {
            tag: Some(tag),
            test: &test,
        }),
        drop_needle,
    )
}

/// `$stream->expect(Token::BLOCK_END_TYPE)`.
pub(crate) fn expect_block_end(parser: &mut Parser<'_>) -> Result<Token> {
    parser.stream_mut().expect(TokenType::BlockEnd, None, None)
}

/// A `SyntaxError` at `line` with the stream's source (`new SyntaxError($message, $line,
/// $stream->getSourceContext())`).
pub(crate) fn syntax_error(
    parser: &Parser<'_>,
    message: impl Into<String>,
    line: i64,
) -> crate::Error {
    crate::Error::syntax(message)
        .with_line(line)
        .with_source(parser.stream().source_context().clone())
}

/// `ctype_space($text)`: non-empty and only ASCII whitespace (space, `\t`, `\n`, `\v`, `\f`, `\r`).
pub(crate) fn ctype_space(text: &[u8]) -> bool {
    !text.is_empty()
        && text
            .iter()
            .all(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0B | 0x0C))
}
