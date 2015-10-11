use node::{ Expr, ExprValue };
use parser::{ Parse, Context };
use { Token, TokenValue };
use operator::{ OperatorOptions, OperatorKind, Associativity };
use error::{ Error, ErrorMessage };
use { Result, Expect };
use value::{ TwigValueRef };
use std::collections::VecDeque;

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
                    expr = Expr::new_at(ExprValue::BinaryOperator {
                        value: op_str,
                        left: Box::new(expr.clone()),
                        right: Box::new(expr1),
                    }, token.line);
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
            let parsed_expr = Expr::new_at(ExprValue::UnaryOperator {
                value: op_str,
                expr: Box::new(expr),
            }, token.line);
            return parse_postfix_expression(parser, parsed_expr);
        }
    }

    if let TokenValue::Punctuation('(') = token.value {
        try!(parser.next());
        let parsed_expr = try!(parse_expression(parser, 0));
        if let Err(_) = parser.expect(TokenValue::Punctuation(')')) {
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
    let token = try!(parser.current());

    let expr = match token.value {
        TokenValue::Name(_) => unreachable!("TokenValue::Name"),
        TokenValue::Value(ref value) => match *value {
            TwigValueRef::Num(_) => unreachable!("TwigValueRef::Num"),
            TwigValueRef::Str(_) => try!(parse_string_expression(parser)),
        },
        TokenValue::InterpolationStart => try!(parse_string_expression(parser)),
        TokenValue::Operator(_) => unreachable!("TokenValue::Operator"),
        TokenValue::Punctuation('[') => unreachable!("TokenValue::Punctuation('[')"),
        TokenValue::Punctuation('{') => unreachable!("TokenValue::Punctuation('{')"),
        other => return Err(Error::new_at(
            ErrorMessage::UnexpectedTokenValue(other.into()),
            token.line
        )),
    };

    parse_postfix_expression(parser, expr)
}

/// Parses expression and returns handle to one that should be executed first.
fn parse_string_expression<'p, 'c, I>(parser: &mut Context<'p, I>)
    -> Result<Expr<'c>>
    where
        I: Iterator<Item=Result<Token<'c>>>
{
    println!("parse_string_expression");

    let mut nodes = VecDeque::new();
    let mut next_can_be_string = true;

    loop {
        let token = try!(parser.current());

        if let (true, TokenValue::Value(TwigValueRef::Str(value))) = (next_can_be_string, token.value) {
            try!(parser.next());
            nodes.push_back(Expr::new_at(
                ExprValue::Constant(value),
                token.line
            ));
            next_can_be_string = false;
            continue;
        }

        if let TokenValue::InterpolationStart = token.value {
            try!(parser.next());
            nodes.push_back(try!(parse_expression(parser, 0)));
            try!(parser.expect(TokenValue::InterpolationEnd));
            next_can_be_string = true;
            continue;
        }

        break;
    }

    let mut expr = nodes.pop_front()
        .expect("twig bug: expected first node to be string when in parse_string_expression state");

    for node in nodes {
        let line = node.line;
        expr = Expr::new_at(
            ExprValue::Concat { left: Box::new(expr), right: Box::new(node) },
            line
        );
    }

    Ok(expr)
}

/// Parses expression and returns handle to one that should be executed first.
fn parse_postfix_expression<'p, 'c, I>(parser: &mut Context<'p, I>, mut node: Expr<'c>)
    -> Result<Expr<'c>>
    where
        I: Iterator<Item=Result<Token<'c>>>
{
    println!("parse_postfix_expression");

    loop {
        let token = try!(parser.current());
        if let TokenValue::Punctuation(ch) = token.value {
            node = match ch {
                '.' | '[' => try!(parse_subscript_expression(parser, node)),
                '|' => try!(parse_filter_expression(parser, node)),
                _ => break,
            };

            continue;
        }

        break;
    }

    Ok(node)
}

fn parse_subscript_expression<'p, 'c, I>(parser: &mut Context<'p, I>, expr: Expr<'c>)
    -> Result<Expr<'c>>
    where
        I: Iterator<Item=Result<Token<'c>>>
{
    println!("parse_subscript_expression");
    unimplemented!()
}

fn parse_filter_expression<'p, 'c, I>(parser: &mut Context<'p, I>, expr: Expr<'c>)
    -> Result<Expr<'c>>
    where
        I: Iterator<Item=Result<Token<'c>>>
{
    println!("parse_filter_expression");
    unimplemented!()
}

fn parse_conditional_expression<'p, 'c, I>(parser: &mut Context<'p, I>, mut expr: Expr<'c>)
    -> Result<Expr<'c>>
    where
        I: Iterator<Item=Result<Token<'c>>>
{
    println!("parse_conditional_expression");

    while try!(parser.skip_to_next_if(TokenValue::Punctuation('?'))) {
        let (expr2, expr3) =
            if !try!(parser.skip_to_next_if(TokenValue::Punctuation(':'))) {
                let expr2 = try!(parse_expression(parser, 0));
                if try!(parser.skip_to_next_if(TokenValue::Punctuation(':'))) {
                    (expr2, try!(parse_expression(parser, 0)))
                } else {
                    (expr2, Expr::new_at(
                        ExprValue::Constant(""),
                        try!(parser.current()).line
                    ))
                }
            } else {
                (expr.clone(), try!(parse_expression(parser, 0)))
            };
        expr = Expr::new_at(ExprValue::Conditional {
            expr: Box::new(expr),
            yay: Box::new(expr2),
            nay: Box::new(expr3)
        }, try!(parser.current()).line);
    }

    Ok(expr)
}
