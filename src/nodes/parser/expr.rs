use nodes::{ Parse, Parser, ImportedFunction };
use nodes::expr::{ Expr, ExprValue, ExprConstant, ExprCallType };
use tokens::TokenValueRef;
use operator::{ OperatorOptions, OperatorKind, Associativity };
use error::{ Result, ErrorAt, ErrorMessage };
use Expect;
use value::{ TwigValueRef, TwigNumberRef };
use std::collections::VecDeque;

impl<'c> Parse<'c> for Expr<'c> {
    type Output = Expr<'c>;

    fn parse<'r>(parser: &mut Parser<'r, 'c>)
        -> Result<Expr<'c>>
    {
        trace!("Expr::parse");
        parse_expression(parser, 0)
    }
}

pub fn parse_expression<'p, 'c>(parser: &mut Parser<'p, 'c>, min_precedence: u16)
    -> Result<Expr<'c>>
{
    trace!("parse_expression");

    let mut expr = try!(get_primary(parser));
    let mut token = try!(parser.current());

    loop {
        if let TokenValueRef::Operator(op_str) = token.value {
            if let OperatorOptions { kind: OperatorKind::Binary { associativity, .. }, precedence: Some(precedence), .. } = parser.get_operator_options(op_str) {
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

pub fn get_primary<'p, 'c>(parser: &mut Parser<'p, 'c>)
    -> Result<Expr<'c>>
{
    trace!("get_primary");

    let token = try!(parser.current());

    if let TokenValueRef::Operator(op_str) = token.value {
        if let OperatorOptions { kind: OperatorKind::Unary { .. }, precedence: Some(precedence), .. } = parser.get_operator_options(op_str) {
            try!(parser.next());
            let expr = try!(parse_expression(parser, precedence));
            let parsed_expr = Expr::new_at(ExprValue::UnaryOperator {
                value: op_str,
                expr: Box::new(expr),
            }, token.line);
            return parse_postfix_expression(parser, parsed_expr);
        }
    }

    if let TokenValueRef::Punctuation('(') = token.value {
        try!(parser.next());
        let parsed_expr = try!(parse_expression(parser, 0));
        if let Err(_) = parser.expect(TokenValueRef::Punctuation(')')) {
            return Err(ErrorAt::new_at(ErrorMessage::ParenthesisNotClosed, token.line));
        }
        return parse_postfix_expression(parser, parsed_expr);
    }

    parse_primary_expression(parser)
}

/// Parses expression and returns handle to one that should be executed first.
pub fn get_function_node<'p, 'c>(parser: &mut Parser<'p, 'c>, name: &'c str, line: usize)
    -> Result<Expr<'c>>
{
    trace!("get_function_node");

    match name {
        "parent" => unreachable!("function node parent"),
        "block" => unreachable!("function node block"),
        "attribute" => unreachable!("function node attribute"),
        _ => {
            if let Some(ImportedFunction { uuid, alias, .. }) = parser.get_imported_function(name) {
                return Ok(Expr::new_at(ExprValue::ImportedFunctionCall {
                    uuid: uuid,
                    alias: alias,
                    arguments: try!(parse_unnamed_arguments(parser, false))
                }, line));
            }

            unreachable!("other default");
        }
    };

    unimplemented!();
}

pub fn parse_primary_expression<'p, 'c>(parser: &mut Parser<'p, 'c>)
    -> Result<Expr<'c>>
{
    trace!("parse_primary_expression");
    let token = try!(parser.current());

    let expr = match token.value {
        TokenValueRef::Name(name) => {
            try!(parser.next());
            match name {
                "true" | "TRUE" =>
                    Expr::new_bool(true, token.line),
                "false" | "FALSE" =>
                    Expr::new_bool(false, token.line),
                "none" | "NONE" | "null" | "NULL" =>
                    Expr::new_null(token.line),
                name => {
                    let current_token = try!(parser.current());
                    match current_token.value {
                        TokenValueRef::Punctuation('(') => try!(get_function_node(parser, name, token.line)),
                        _ => Expr::new_name(name, token.line),
                    }
                },
            }
        },
        TokenValueRef::Value(ref value) => match *value {
            TwigValueRef::Num(num) => {
                try!(parser.next());
                get_number_expr(num, token.line)
            },
            TwigValueRef::Str(_) => try!(parse_string_expression(parser)),
        },
        TokenValueRef::InterpolationStart => try!(parse_string_expression(parser)),
        TokenValueRef::Operator(_) => unreachable!("TokenValueRef::Operator"),
        TokenValueRef::Punctuation('[') => try!(parse_array_expression(parser)),
        TokenValueRef::Punctuation('{') => try!(parse_hash_expression(parser)),
        other => return Err(ErrorAt::new_at(
            ErrorMessage::UnexpectedTokenValue(other.into()),
            token.line
        )),
    };

    parse_postfix_expression(parser, expr)
}

pub fn get_number_expr<'c>(num: TwigNumberRef<'c>, line: usize) -> Expr<'c> {
    Expr::new_at(ExprValue::Constant(match num {
        TwigNumberRef::Big(v) => ExprConstant::Big(v),
        TwigNumberRef::Float(v) => ExprConstant::Float(v),
        TwigNumberRef::Int(v) => ExprConstant::Int(v),
    }), line)
}

pub fn parse_string_expression<'p, 'c>(parser: &mut Parser<'p, 'c>)
    -> Result<Expr<'c>>
{
    trace!("parse_string_expression");

    let mut nodes = VecDeque::new();
    let mut next_can_be_string = true;

    loop {
        let token = try!(parser.current());

        if let (true, TokenValueRef::Value(TwigValueRef::Str(value))) = (next_can_be_string, token.value) {
            try!(parser.next());
            nodes.push_back(Expr::new_str_constant(value, token.line));
            next_can_be_string = false;
            continue;
        }

        if let TokenValueRef::InterpolationStart = token.value {
            try!(parser.next());
            nodes.push_back(try!(parse_expression(parser, 0)));
            try!(parser.expect(TokenValueRef::InterpolationEnd));
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

pub fn parse_array_expression<'p, 'c>(parser: &mut Parser<'p, 'c>)
    -> Result<Expr<'c>>
{
    trace!("parse_array_expression");

    try!(parser.expect_or_error(TokenValueRef::Punctuation('['), ErrorMessage::ExpectedArrayElement));

    let mut items = Vec::new();

    let mut token = try!(parser.current());
    let start_line = token.line;
    let mut first = true;

    while token.value != TokenValueRef::Punctuation(']') {
        if !first {
            try!(parser.expect_or_error(TokenValueRef::Punctuation(','), ErrorMessage::ArrayValueMustBeFollowedByComma));
            token = try!(parser.current());

            // trailing ,?
            if token.value == TokenValueRef::Punctuation(']') {
                break;
            }
        }
        first = false;

        items.push(try!(parse_expression(parser, 0)));
        token = try!(parser.current());
    }
    try!(parser.expect_or_error(TokenValueRef::Punctuation(']'), ErrorMessage::ArrayNotClosed));

    Ok(Expr::new_array(items, start_line))
}

pub fn parse_hash_expression<'p, 'c>(parser: &mut Parser<'p, 'c>)
    -> Result<Expr<'c>>
{
    trace!("parse_hash_expression");

    try!(parser.expect_or_error(TokenValueRef::Punctuation('{'), ErrorMessage::ExpectedHashElement));

    let mut items = Vec::new();

    let mut token = try!(parser.current());
    let start_line = token.line;
    let mut first = true;

    while token.value != TokenValueRef::Punctuation('}') {
        if !first {
            try!(parser.expect_or_error(TokenValueRef::Punctuation(','), ErrorMessage::HashValueMustBeFollowedByComma));
            token = try!(parser.current());

            // trailing ,?
            if token.value == TokenValueRef::Punctuation('}') {
                break;
            }
        }
        first = false;

        // a hash key can be:
        //
        //  * a number -- 12
        //  * a string -- 'a'
        //  * a name, which is equivalent to a string -- a
        //  * an expression, which must be enclosed in parentheses -- (1 + 2)
        let key = match token.value {
            TokenValueRef::Value(TwigValueRef::Str(v)) => {
                try!(parser.next());
                Expr::new_str_constant(v, token.line)
            },
            TokenValueRef::Name(v) => {
                try!(parser.next());
                Expr::new_str_constant(v, token.line)
            },
            TokenValueRef::Value(TwigValueRef::Num(num)) => {
                try!(parser.next());
                get_number_expr(num, token.line)
            },
            TokenValueRef::Punctuation('(') => {
                try!(parse_expression(parser, 0))
            }
            _ => return Err(ErrorAt::new_at(
                ErrorMessage::InvalidHashKey { unexpected: token.value.into() },
                token.line
            )),
        };

        try!(parser.expect_or_error(TokenValueRef::Punctuation(':'), ErrorMessage::HashKeyMustBeFollowedByColon));

        let value = try!(parse_expression(parser, 0));
        token = try!(parser.current());

        items.push((key, value));
    }
    try!(parser.expect_or_error(TokenValueRef::Punctuation('}'), ErrorMessage::HashNotClosed));

    Ok(Expr::new_hash(items, start_line))
}

pub fn parse_postfix_expression<'p, 'c>(parser: &mut Parser<'p, 'c>, mut node: Expr<'c>)
    -> Result<Expr<'c>>
{
    trace!("parse_postfix_expression");

    loop {
        let token = try!(parser.current());
        if let TokenValueRef::Punctuation(ch) = token.value {
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

pub fn parse_subscript_expression<'p, 'c>(parser: &mut Parser<'p, 'c>, node: Expr<'c>)
    -> Result<Expr<'c>>
{
    trace!("parse_subscript_expression");

    let mut token = try!(parser.next());
    let line = token.line;
    let mut arguments = Vec::<Expr<'c>>::new();
    let mut call_type = ExprCallType::Any;

    let arg = match token.value {
        TokenValueRef::Punctuation('.') => {
            token = try!(parser.next());
            let arg = match token.value {
                TokenValueRef::Name(v) => Expr::new_str_constant(v, line),
                TokenValueRef::Value(TwigValueRef::Num(num)) => get_number_expr(num, line),
                // OMG the hack here is _hilarious_:
                // TODO: ($token->getType() == Twig_tokens::OPERATOR_TYPE && preg_match(Twig_Lexer::REGEX_NAME, $token->getValue()))
                _ => return Err(ErrorAt::new_at(
                    ErrorMessage::ExpectedNameOrNumber,
                    line
                ))
            };

            token = try!(parser.current());
            if let TokenValueRef::Punctuation('(') = token.value {
                call_type = ExprCallType::Method;
                arguments = try!(parse_unnamed_arguments(parser, false));
            }

            // TODO: Block of bad code

            unimplemented!()

            //arg
        },
        _ => {
            call_type = ExprCallType::Array;

            unimplemented!()
        }
    };

    Ok(Expr::new_at(
        ExprValue::GetAttr {
            node: Box::new(node),
            arg: Box::new(arg),
            arguments: arguments,
            call_type: call_type
        },
        line
    ))
}

pub fn parse_filter_expression<'p, 'c>(parser: &mut Parser<'p, 'c>, expr: Expr<'c>)
    -> Result<Expr<'c>>
{
    trace!("parse_filter_expression");
    unimplemented!()
}

pub fn parse_unnamed_arguments<'p, 'c>(parser: &mut Parser<'p, 'c>, definition: bool)
    -> Result<Vec<Expr<'c>>>
{
    trace!("parse_unnamed_arguments, definition {:?}", definition);

    let mut args = Vec::new();

    try!(parser.expect_or_error(TokenValueRef::Punctuation('('), ErrorMessage::ListOfArgumentsMustBeginWithParenthesis));

    while !try!(parser.test(TokenValueRef::Punctuation(')'))) {
        if args.len() > 0 {
            try!(parser.expect_or_error(TokenValueRef::Punctuation(','), ErrorMessage::ArgumentsMustBeSeparatedByComma));
        }

        let value = if definition {
            unreachable!("argument definition parsing not implemented");
        } else {
            try!(parse_expression(parser, 0))
        };

        if definition {
            unreachable!("argument definition parsing not implemented");
        } else {
            args.push(value);
        }
    }
    try!(parser.expect_or_error(TokenValueRef::Punctuation(')'), ErrorMessage::ListOfArgumentsMustCloseWithParenthesis));

    Ok(args)
}

pub fn parse_named_arguments<'p, 'c>(parser: &mut Parser<'p, 'c>, definition: bool)
    -> Result<Vec<(&'c str, Expr<'c>)>>
{
    trace!("parse_named_arguments, definition {:?}", definition);

    let mut args = Vec::new();

    try!(parser.expect_or_error(TokenValueRef::Punctuation('('), ErrorMessage::ListOfArgumentsMustBeginWithParenthesis));

    while !try!(parser.test(TokenValueRef::Punctuation(')'))) {
        if args.len() > 0 {
            try!(parser.expect_or_error(TokenValueRef::Punctuation(','), ErrorMessage::ArgumentsMustBeSeparatedByComma));
        }

        let (name_expr, token) = if definition {
            let name = try!(parser.expect_name());
            let token = try!(parser.current());
            (Expr::new_name(name, token.line), token)
        } else {
            (try!(parse_expression(parser, 0)), try!(parser.current()))
        };

        try!(parser.expect(TokenValueRef::Operator("=")));

        let name = match name_expr {
            Expr { value: ExprValue::Name(n), .. } => n,
            other => return Err(ErrorAt::new_at(
                ErrorMessage::ParameterNameMustBeAString { given: format!("{:?}", other) },
                token.line
            )),
        };

        let value = if definition {
            let value = try!(parse_primary_expression(parser));

            if !value.is_constant() {
                return Err(ErrorAt::new_at(
                    ErrorMessage::DefaultValueForArgumentMustBeConstant,
                    try!(parser.current()).line
                ));
            }

            value
        } else {
            try!(parse_expression(parser, 0))
        };

        args.push((name, value))
    }
    try!(parser.expect_or_error(TokenValueRef::Punctuation(')'), ErrorMessage::ListOfArgumentsMustCloseWithParenthesis));

    Ok(args)
}

pub fn parse_conditional_expression<'p, 'c>(parser: &mut Parser<'p, 'c>, mut expr: Expr<'c>)
    -> Result<Expr<'c>>
{
    trace!("parse_conditional_expression");

    while try!(parser.skip_to_next_if(TokenValueRef::Punctuation('?'))) {
        let (expr2, expr3) =
            if !try!(parser.skip_to_next_if(TokenValueRef::Punctuation(':'))) {
                let expr2 = try!(parse_expression(parser, 0));
                if try!(parser.skip_to_next_if(TokenValueRef::Punctuation(':'))) {
                    (expr2, try!(parse_expression(parser, 0)))
                } else {
                    (expr2, Expr::new_str_constant("", try!(parser.current()).line))
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
