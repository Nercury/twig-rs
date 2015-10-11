use node::{ Body, Expr };
use parser::{ Parse, Context };
use TokenValue;
use { Result, Expect };
use error::{ Error, ErrorMessage };

impl<'c> Parse<'c> for Body<'c> {
    type Output = Body<'c>;

    fn parse<'r>(parser: &mut Context<'r, 'c>)
        -> Result<Body<'c>>
    {
        let mut maybe_line = None;
        let mut rv = Vec::new();

        while let Some(token) = try!(parser.maybe_current()) {
            if let None = maybe_line {
                maybe_line = Some(token.line);
            }
            match token.value {
                TokenValue::Text(t) => {
                    try!(parser.next());
                    rv.push(Body::Text { value: t, line: token.line })
                },
                TokenValue::VarStart => {
                    try!(parser.next());
                    let expr = try!(Expr::parse(parser));
                    try!(parser.expect(TokenValue::VarEnd));
                    rv.push(Body::Print { expr: Box::new(expr), line: token.line });
                },
                TokenValue::BlockStart => {
                    try!(parser.next());
                    let token = try!(parser.current());

                    let tag_name = match token.value {
                        TokenValue::Name(n) => n,
                        _ => return Err(Error::new_at(ErrorMessage::MustStartWithTagName, token.line)),
                    };

                    let subparser = match parser.env.handlers.get(tag_name) {
                        Some(sp) => sp,
                        None => {
                            unreachable!("errors when subparser not found not implemented")
                        }
                    };

                    try!(parser.next());
                    let maybe_node = try!(subparser.parse(parser, token));
                    if let Some(node) = maybe_node {
                         rv.push(node);
                    }
                },
                tv => { panic!("not implemented {:?}", tv) },
            };
        }

        if rv.len() == 1 {
            Ok(rv.remove(0))
        } else {
            Ok(Body::List { items: rv })
        }
    }
}
