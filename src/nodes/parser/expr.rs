use crate::error::{TemplateError, TemplateResult};
use crate::nodes::expr::{Expr, ExprCallType, ExprConstant, ExprValue};
use crate::nodes::{ImportedFunction, Parse, Parser};
use crate::operator::{Associativity, OperatorKind, OperatorOptions};
use crate::tokens::{ConstNumberRef, ConstRef, TokenValueRef};
use std::collections::VecDeque;

impl<'c> Parse<'c> for Expr<'c> {
    type Output = Expr<'c>;

    fn parse<'r>(parser: &mut Parser<'r, 'c>) -> TemplateResult<Expr<'c>> {
        parse_expression(parser, 0)
    }
}

pub fn parse_expression<'p, 'c>(
    parser: &mut Parser<'p, 'c>,
    min_precedence: u16,
) -> TemplateResult<Expr<'c>> {
    let mut expr = get_primary(parser)?;
    let mut token = parser.current()?;

    loop {
        if let TokenValueRef::Operator(op_str) = token.value
            && let OperatorOptions {
                kind: OperatorKind::Binary { associativity, .. },
                precedence: Some(precedence),
                ..
            } = parser.get_operator_options(op_str)
            && precedence >= min_precedence
        {
            parser.next()?;

            // if callable ...
            // TODO: Callable.
            // else
            let expr1 = parse_expression(
                parser,
                match associativity {
                    Associativity::Left => precedence + 1,
                    Associativity::Right => precedence,
                },
            )?;
            expr = Expr::new_at(
                ExprValue::BinaryOperator {
                    value: op_str,
                    left: Box::new(expr.clone()),
                    right: Box::new(expr1),
                },
                token.line,
            );
            // endif

            token = parser.current()?;

            continue;
        }
        break;
    }

    if 0 == min_precedence {
        return parse_conditional_expression(parser, expr);
    }

    Ok(expr)
}

pub fn get_primary<'p, 'c>(parser: &mut Parser<'p, 'c>) -> TemplateResult<Expr<'c>> {
    let token = parser.current()?;

    if let TokenValueRef::Operator(op_str) = token.value
        && let OperatorOptions {
            kind: OperatorKind::Unary { .. },
            precedence: Some(precedence),
            ..
        } = parser.get_operator_options(op_str)
    {
        parser.next()?;
        let expr = parse_expression(parser, precedence)?;
        let parsed_expr = Expr::new_at(
            ExprValue::UnaryOperator {
                value: op_str,
                expr: Box::new(expr),
            },
            token.line,
        );
        return parse_postfix_expression(parser, parsed_expr);
    }

    if let TokenValueRef::Punctuation('(') = token.value {
        parser.next()?;
        let parsed_expr = parse_expression(parser, 0)?;
        if parser.expect(TokenValueRef::Punctuation(')')).is_err() {
            return Err(TemplateError::ParenthesisNotClosed.at(token.line));
        }
        return parse_postfix_expression(parser, parsed_expr);
    }

    parse_primary_expression(parser)
}

/// Parses expression and returns handle to one that should be executed first.
pub fn get_function_node<'p, 'c>(
    parser: &mut Parser<'p, 'c>,
    name: &'c str,
    line: usize,
) -> TemplateResult<Expr<'c>> {
    match name {
        "parent" => unreachable!("function node parent"),
        "block" => unreachable!("function node block"),
        "attribute" => unreachable!("function node attribute"),
        _ => {
            if let Some(ImportedFunction { id, alias, .. }) = parser.get_imported_function(name) {
                return Ok(Expr::new_at(
                    ExprValue::ImportedFunctionCall {
                        id,
                        alias,
                        arguments: parse_unnamed_arguments(parser, false)?,
                    },
                    line,
                ));
            }

            return Ok(Expr::new_at(
                ExprValue::FunctionCall {
                    name,
                    arguments: parse_named_arguments(parser, false)?,
                },
                line,
            ));
        }
    }
}

pub fn parse_primary_expression<'p, 'c>(parser: &mut Parser<'p, 'c>) -> TemplateResult<Expr<'c>> {
    let token = parser.current()?;

    let expr = match token.value {
        TokenValueRef::Name(name) => {
            parser.next()?;
            match name {
                "true" | "TRUE" => Expr::new_bool(true, token.line),
                "false" | "FALSE" => Expr::new_bool(false, token.line),
                "none" | "NONE" | "null" | "NULL" => Expr::new_null(token.line),
                name => {
                    let current_token = parser.current()?;
                    match current_token.value {
                        TokenValueRef::Punctuation('(') => {
                            get_function_node(parser, name, token.line)?
                        }
                        _ => Expr::new_name(name, token.line),
                    }
                }
            }
        }
        TokenValueRef::Value(ref value) => match *value {
            ConstRef::Num(num) => {
                parser.next()?;
                get_number_expr(num, token.line)
            }
            ConstRef::Str(_) => parse_string_expression(parser)?,
        },
        TokenValueRef::InterpolationStart => parse_string_expression(parser)?,
        TokenValueRef::Operator(_) => unreachable!("TokenValueRef::Operator"),
        TokenValueRef::Punctuation('[') => parse_array_expression(parser)?,
        TokenValueRef::Punctuation('{') => parse_hash_expression(parser)?,
        other => return Err(TemplateError::UnexpectedTokenValue(other.into()).at(token.line)),
    };

    parse_postfix_expression(parser, expr)
}

pub fn get_number_expr<'c>(num: ConstNumberRef<'c>, line: usize) -> Expr<'c> {
    Expr::new_at(
        ExprValue::Constant(match num {
            ConstNumberRef::Big(v) => ExprConstant::Big(v),
            ConstNumberRef::Float(v) => ExprConstant::Float(v),
            ConstNumberRef::Int(v) => ExprConstant::Int(v),
        }),
        line,
    )
}

pub fn parse_string_expression<'p, 'c>(parser: &mut Parser<'p, 'c>) -> TemplateResult<Expr<'c>> {
    let mut nodes = VecDeque::new();
    let mut next_can_be_string = true;

    loop {
        let token = parser.current()?;

        if let (true, TokenValueRef::Value(ConstRef::Str(value))) =
            (next_can_be_string, token.value)
        {
            parser.next()?;
            nodes.push_back(Expr::new_str_constant(value, token.line));
            next_can_be_string = false;
            continue;
        }

        if let TokenValueRef::InterpolationStart = token.value {
            parser.next()?;
            nodes.push_back(parse_expression(parser, 0)?);
            parser.expect(TokenValueRef::InterpolationEnd)?;
            next_can_be_string = true;
            continue;
        }

        break;
    }

    let mut expr = nodes
        .pop_front()
        .expect("twig bug: expected first node to be string when in parse_string_expression state");

    for node in nodes {
        let line = node.line;
        expr = Expr::new_at(
            ExprValue::Concat {
                left: Box::new(expr),
                right: Box::new(node),
            },
            line,
        );
    }

    Ok(expr)
}

pub fn parse_array_expression<'p, 'c>(parser: &mut Parser<'p, 'c>) -> TemplateResult<Expr<'c>> {
    parser.expect_or_error(
        TokenValueRef::Punctuation('['),
        TemplateError::ExpectedArrayElement,
    )?;

    let mut items = Vec::new();

    let mut token = parser.current()?;
    let start_line = token.line;
    let mut first = true;

    while token.value != TokenValueRef::Punctuation(']') {
        if !first {
            parser.expect_or_error(
                TokenValueRef::Punctuation(','),
                TemplateError::ArrayValueMustBeFollowedByComma,
            )?;
            token = parser.current()?;

            // trailing ,?
            if token.value == TokenValueRef::Punctuation(']') {
                break;
            }
        }
        first = false;

        items.push(parse_expression(parser, 0)?);
        token = parser.current()?;
    }
    parser.expect_or_error(
        TokenValueRef::Punctuation(']'),
        TemplateError::ArrayNotClosed,
    )?;

    Ok(Expr::new_array(items, start_line))
}

pub fn parse_hash_expression<'p, 'c>(parser: &mut Parser<'p, 'c>) -> TemplateResult<Expr<'c>> {
    parser.expect_or_error(
        TokenValueRef::Punctuation('{'),
        TemplateError::ExpectedHashElement,
    )?;

    let mut items = Vec::new();

    let mut token = parser.current()?;
    let start_line = token.line;
    let mut first = true;

    while token.value != TokenValueRef::Punctuation('}') {
        if !first {
            parser.expect_or_error(
                TokenValueRef::Punctuation(','),
                TemplateError::HashValueMustBeFollowedByComma,
            )?;
            token = parser.current()?;

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
            TokenValueRef::Value(ConstRef::Str(v)) => {
                parser.next()?;
                Expr::new_str_constant(v, token.line)
            }
            TokenValueRef::Name(v) => {
                parser.next()?;
                Expr::new_str_constant(v, token.line)
            }
            TokenValueRef::Value(ConstRef::Num(num)) => {
                parser.next()?;
                get_number_expr(num, token.line)
            }
            TokenValueRef::Punctuation('(') => parse_expression(parser, 0)?,
            _ => {
                return Err(TemplateError::InvalidHashKey {
                    unexpected: token.value.into(),
                }
                .at(token.line));
            }
        };

        parser.expect_or_error(
            TokenValueRef::Punctuation(':'),
            TemplateError::HashKeyMustBeFollowedByColon,
        )?;

        let value = parse_expression(parser, 0)?;
        token = parser.current()?;

        items.push((key, value));
    }
    parser.expect_or_error(
        TokenValueRef::Punctuation('}'),
        TemplateError::HashNotClosed,
    )?;

    Ok(Expr::new_hash(items, start_line))
}

pub fn parse_postfix_expression<'p, 'c>(
    parser: &mut Parser<'p, 'c>,
    mut node: Expr<'c>,
) -> TemplateResult<Expr<'c>> {
    loop {
        let token = parser.current()?;
        if let TokenValueRef::Punctuation(ch) = token.value {
            node = match ch {
                '.' | '[' => parse_subscript_expression(parser, node)?,
                '|' => parse_filter_expression(parser, node)?,
                _ => break,
            };

            continue;
        }

        break;
    }

    Ok(node)
}

pub fn parse_subscript_expression<'p, 'c>(
    parser: &mut Parser<'p, 'c>,
    _node: Expr<'c>,
) -> TemplateResult<Expr<'c>> {
    let token = parser.next()?;
    let line = token.line;

    match token.value {
        TokenValueRef::Punctuation('.') => {
            let token = parser.next()?;
            let _arg = match token.value {
                TokenValueRef::Name(v) => Expr::new_str_constant(v, line),
                TokenValueRef::Value(ConstRef::Num(num)) => get_number_expr(num, line),
                // OMG the hack here is _hilarious_:
                // TODO: ($token->getType() == Twig_tokens::OPERATOR_TYPE && preg_match(Twig_Lexer::REGEX_NAME, $token->getValue()))
                _ => return Err(TemplateError::ExpectedNameOrNumber.at(line)),
            };

            let (_call_type, _arguments) = match parser.current()?.value {
                TokenValueRef::Punctuation('(') => (
                    ExprCallType::Method,
                    parse_unnamed_arguments(parser, false)?,
                ),
                _ => (ExprCallType::Any, Vec::new()),
            };

            // TODO: Block of bad code, then return `ExprValue::GetAttr`.

            unimplemented!()
        }
        _ => {
            // TODO: `ExprCallType::Array` access.

            unimplemented!()
        }
    }
}

pub fn parse_filter_expression<'p, 'c>(
    _parser: &mut Parser<'p, 'c>,
    _expr: Expr<'c>,
) -> TemplateResult<Expr<'c>> {
    unimplemented!()
}

pub fn parse_unnamed_arguments<'p, 'c>(
    parser: &mut Parser<'p, 'c>,
    definition: bool,
) -> TemplateResult<Vec<Expr<'c>>> {
    let mut args = Vec::new();

    parser.expect_or_error(
        TokenValueRef::Punctuation('('),
        TemplateError::ListOfArgumentsMustBeginWithParenthesis,
    )?;

    while !parser.test(TokenValueRef::Punctuation(')'))? {
        if !args.is_empty() {
            parser.expect_or_error(
                TokenValueRef::Punctuation(','),
                TemplateError::ArgumentsMustBeSeparatedByComma,
            )?;
        }

        let value = if definition {
            unreachable!("argument definition parsing not implemented");
        } else {
            parse_expression(parser, 0)?
        };

        if definition {
            unreachable!("argument definition parsing not implemented");
        } else {
            args.push(value);
        }
    }
    parser.expect_or_error(
        TokenValueRef::Punctuation(')'),
        TemplateError::ListOfArgumentsMustCloseWithParenthesis,
    )?;

    Ok(args)
}

pub fn parse_named_arguments<'p, 'c>(
    parser: &mut Parser<'p, 'c>,
    definition: bool,
) -> TemplateResult<Vec<(Option<&'c str>, Expr<'c>)>> {
    let mut args = Vec::new();

    parser.expect_or_error(
        TokenValueRef::Punctuation('('),
        TemplateError::ListOfArgumentsMustBeginWithParenthesis,
    )?;

    while !parser.test(TokenValueRef::Punctuation(')'))? {
        if !args.is_empty() {
            parser.expect_or_error(
                TokenValueRef::Punctuation(','),
                TemplateError::ArgumentsMustBeSeparatedByComma,
            )?;
        }

        let (name_expr, _token) = if definition {
            let name = parser.expect_name()?;
            let token = parser.current()?;
            (Expr::new_name(name, token.line), token)
        } else {
            (parse_expression(parser, 0)?, parser.current()?)
        };

        let (name, value) = if parser.skip_to_next_if(TokenValueRef::Operator("="))? {
            let token = parser.current()?;

            let name = match name_expr {
                Expr {
                    value: ExprValue::Name(n),
                    ..
                } => n,
                other => {
                    return Err(TemplateError::ParameterNameMustBeAString {
                        given: format!("{:?}", other),
                    }
                    .at(token.line));
                }
            };

            let value = if definition {
                let value = parse_primary_expression(parser)?;

                if !value.is_constant() {
                    return Err(TemplateError::DefaultValueForArgumentMustBeConstant
                        .at(parser.current()?.line));
                }

                value
            } else {
                parse_expression(parser, 0)?
            };

            (Some(name), value)
        } else {
            (None, name_expr)
        };

        args.push(if definition {
            match name {
                None => (
                    Some(match value {
                        Expr {
                            value: ExprValue::Name(n),
                            ..
                        } => n,
                        _ => unreachable!("twig bug: expected that expression is a name"),
                    }),
                    Expr::new_null(parser.current()?.line),
                ),
                Some(name) => (Some(name), value),
            }
        } else {
            (name, value)
        })
    }
    parser.expect_or_error(
        TokenValueRef::Punctuation(')'),
        TemplateError::ListOfArgumentsMustCloseWithParenthesis,
    )?;

    Ok(args)
}

pub fn parse_conditional_expression<'p, 'c>(
    parser: &mut Parser<'p, 'c>,
    mut expr: Expr<'c>,
) -> TemplateResult<Expr<'c>> {
    while parser.skip_to_next_if(TokenValueRef::Punctuation('?'))? {
        let (expr2, expr3) = if !parser.skip_to_next_if(TokenValueRef::Punctuation(':'))? {
            let expr2 = parse_expression(parser, 0)?;
            if parser.skip_to_next_if(TokenValueRef::Punctuation(':'))? {
                (expr2, parse_expression(parser, 0)?)
            } else {
                (expr2, Expr::new_str_constant("", parser.current()?.line))
            }
        } else {
            (expr.clone(), parse_expression(parser, 0)?)
        };
        expr = Expr::new_at(
            ExprValue::Conditional {
                expr: Box::new(expr),
                yay: Box::new(expr2),
                nay: Box::new(expr3),
            },
            parser.current()?.line,
        );
    }

    Ok(expr)
}
