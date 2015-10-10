use node::{ Expr };
use parser::{ Parse, Context };
use { Token, TokenValue };
use operator::{ OperatorOptions, OperatorKind };
use Result;

impl<'a, 'code> Parse<'code> for Expr<'a> {
    type Output = Expr<'code>;

    fn parse<'r, I>(parser: &mut Context<'r, I>)
        -> Result<Expr<'code>>
    where
        I: Iterator<Item=Result<Token<'code>>>
    {
        let token = try!(parser.next());
        parse_with_precedence(parser, token, 0)
    }
}

fn parse_with_precedence<'r, 'c, I>(parser: &mut Context<'r, I>, token: Token<'c>, precedence: u16)
    -> Result<Expr<'c>>
    where
        I: Iterator<Item=Result<Token<'c>>>
{
    println!("parse_with_precedence_0");
    parse_primary(parser, token)
}

/// Parses expression and returns handle to one that should be executed first.
fn parse_primary<'r, 'c, I>(parser: &mut Context<'r, I>, token: Token<'c>)
    -> Result<Expr<'c>>
    where
        I: Iterator<Item=Result<Token<'c>>>
{
    if let TokenValue::Operator(op_str) = token.value {
        if let OperatorOptions { kind: OperatorKind::Unary, precedence, .. } = *parser.get_operator_options(op_str) {
            let next_token = try!(parser.next());
        }
    }

    if let TokenValue::Punctuation('(') = token.value {

    }

    parse_primary_expression(parser, token)
}

/// Parses expression and returns handle to one that should be executed first.
fn parse_primary_expression<'r, 'c, I>(parser: &mut Context<'r, I>, token: Token<'c>)
    -> Result<Expr<'c>>
    where
        I: Iterator<Item=Result<Token<'c>>>
{
    println!("parse_primary");
    Ok(Expr::Constant { value: "", line: 1 })
}
