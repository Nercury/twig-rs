use node::{ Expr };
use parser::{ Parse, Context };
use { Token, TokenValue };
use operator::{ OperatorOptions, OperatorKind };
use Result;

impl<'c> Parse<'c> for Expr<'c> {
    type Output = Expr<'c>;

    fn parse<'r, I>(parser: &mut Context<'r, I>)
        -> Result<Expr<'c>>
    where
        I: Iterator<Item=Result<Token<'c>>>
    {
        parse_with_precedence(parser, 0)
    }
}

fn parse_with_precedence<'p, 'c, I>(parser: &mut Context<'p, I>, precedence: u16)
    -> Result<Expr<'c>>
    where
        I: Iterator<Item=Result<Token<'c>>>
{
    println!("parse_with_precedence_0");
    get_primary(parser)
}

/// Parses expression and returns handle to one that should be executed first.
fn get_primary<'p, 'c, I>(parser: &mut Context<'p, I>)
    -> Result<Expr<'c>>
    where
        I: Iterator<Item=Result<Token<'c>>>
{
    let token = try!(parser.current());
    if let TokenValue::Operator(op_str) = token.value {
        if let OperatorOptions { kind: OperatorKind::Unary, precedence, .. } = *parser.get_operator_options(op_str) {
            try!(parser.next());
            let expr = try!(parse_with_precedence(parser, precedence));
            let parsed_expr = Expr::Operator {
                value: op_str,
                expr: Box::new(expr),
                line: token.line
            };
            return parse_postfix_expression(parser, parsed_expr);
        }
    }

    if let TokenValue::Punctuation('(') = token.value {

    }

    parse_primary_expression(parser)
}

/// Parses expression and returns handle to one that should be executed first.
fn parse_primary_expression<'p, 'c, I>(parser: &mut Context<'p, I>)
    -> Result<Expr<'c>>
    where
        I: Iterator<Item=Result<Token<'c>>>
{
    println!("parse_primary_expression");
    Ok(Expr::Constant { value: "", line: 1 })
}

/// Parses expression and returns handle to one that should be executed first.
fn parse_postfix_expression<'p, 'c, I>(parser: &mut Context<'p, I>, expr: Expr<'c>)
    -> Result<Expr<'c>>
    where
        I: Iterator<Item=Result<Token<'c>>>
{
    println!("parse_postfix_expression");
    Ok(Expr::Constant { value: "", line: 1 })
}
