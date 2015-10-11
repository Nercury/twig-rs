use node::{ Expr };
use parser::{ Parse, Context };
use { Token, TokenValue };
use operator::{ OperatorOptions, OperatorKind, Associativity };
use error::{ Error, ErrorMessage };
use { Result, Expect };

impl<'c> Parse<'c> for Expr<'c> {
    type Output = Expr<'c>;

    fn parse<'r, I>(parser: &mut Context<'r, I>)
        -> Result<Expr<'c>>
    where
        I: Iterator<Item=Result<Token<'c>>>
    {
        parse_expression(parser, 0)
    }
}

fn parse_expression<'p, 'c, I>(parser: &mut Context<'p, I>, min_precedence: u16)
    -> Result<Expr<'c>>
    where
        I: Iterator<Item=Result<Token<'c>>>
{
    println!("parse_expression");

    let mut expr = try!(get_primary(parser));
    let mut token = try!(parser.current());

    loop {
        if let TokenValue::Operator(op_str) = token.value {
            if let OperatorOptions { kind: OperatorKind::Binary(associativity), precedence, .. } = *parser.get_operator_options(op_str) {
                if precedence >= min_precedence {
                    try!(parser.next());

                    // if callable ...
                        // TODO: Callable.
                    // else
                    let expr1 = try!(parse_expression(parser, match associativity {
                        Associativity::Left => precedence + 1,
                        Associativity::Right => precedence,
                    }));
                    expr = Expr::BinaryOperator {
                        value: op_str,
                        left: Box::new(expr.clone()),
                        right: Box::new(expr1),
                        line: token.line
                    };
                    // endif

                    token = try!(parser.current());

                    continue;
                }
            }
        }
        break;
    }

    if 0 == min_precedence {
        return parse_conditional_expression(parser, expr);
    }

    Ok(expr)
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
            let expr = try!(parse_expression(parser, precedence));
            let parsed_expr = Expr::UnaryOperator {
                value: op_str,
                expr: Box::new(expr),
                line: token.line
            };
            return parse_postfix_expression(parser, parsed_expr);
        }
    }

    if let TokenValue::Punctuation('(') = token.value {
        try!(parser.next());
        let parsed_expr = try!(parse_expression(parser, 0));
        if let Err(_) = parser.tokens.expect(TokenValue::Punctuation(')')) {
            return Err(Error::new_at(ErrorMessage::ParenthesisNotClosed, token.line));
        }
        return parse_postfix_expression(parser, parsed_expr);
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
    unimplemented!()
}

/// Parses expression and returns handle to one that should be executed first.
fn parse_postfix_expression<'p, 'c, I>(parser: &mut Context<'p, I>, expr: Expr<'c>)
    -> Result<Expr<'c>>
    where
        I: Iterator<Item=Result<Token<'c>>>
{
    println!("parse_postfix_expression");
    unimplemented!()
}

fn parse_conditional_expression<'p, 'c, I>(parser: &mut Context<'p, I>, expr: Expr<'c>)
    -> Result<Expr<'c>>
    where
        I: Iterator<Item=Result<Token<'c>>>
{
    println!("parse_conditional_expression");
    unimplemented!()
}
