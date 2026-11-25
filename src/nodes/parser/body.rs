use crate::error::{TemplateError, TemplateResult};
use crate::nodes::body::Body;
use crate::nodes::expr::Expr;
use crate::nodes::{Parse, Parser};
use crate::tokens::{TokenRef, TokenValueRef};

impl<'c> Parse<'c> for Body<'c> {
    type Output = Body<'c>;

    fn parse<'r>(parser: &mut Parser<'r, 'c>) -> TemplateResult<Body<'c>> {
        subparse(parser, |_| None)
    }
}

pub struct BlockEnd {
    pub drop_needle: bool,
}

pub fn subparse<'p, 'c, D>(parser: &mut Parser<'p, 'c>, test: D) -> TemplateResult<Body<'c>>
where
    D: Fn(&TokenRef<'c>) -> Option<BlockEnd>,
{
    let mut maybe_line = None;
    let mut rv = Vec::new();

    while let Some(token) = parser.maybe_current()? {
        if maybe_line.is_none() {
            maybe_line = Some(token.line);
        }
        match token.value {
            TokenValueRef::Text(t) => {
                parser.next()?;
                rv.push(Body::Text {
                    value: t,
                    line: token.line,
                })
            }
            TokenValueRef::VarStart => {
                parser.next()?;
                let expr = Expr::parse(parser)?;
                parser.expect(TokenValueRef::VarEnd)?;
                rv.push(Body::Print {
                    expr: Box::new(expr),
                    line: token.line,
                });
            }
            TokenValueRef::BlockStart => {
                parser.next()?;
                let token = parser.current()?;

                let tag_name = match token.value {
                    TokenValueRef::Name(n) => n,
                    _ => return Err(TemplateError::MustStartWithTagName.at(token.line)),
                };

                if let Some(end) = test(&token) {
                    if end.drop_needle {
                        parser.next()?;
                    }
                    return if 1 == rv.len() {
                        Ok(rv.remove(0))
                    } else {
                        Ok(Body::List { items: rv })
                    };
                }

                let subparser = match parser.env.handlers.get(tag_name) {
                    Some(sp) => sp,
                    None => {
                        unreachable!("errors when subparser not found not implemented")
                    }
                };

                parser.next()?;
                let maybe_node = subparser.parse(parser, token)?;
                if let Some(node) = maybe_node {
                    rv.push(node);
                }
            }
            tv => {
                panic!("not implemented {:?}", tv)
            }
        };
    }

    if rv.len() == 1 {
        Ok(rv.remove(0))
    } else {
        Ok(Body::List { items: rv })
    }
}
