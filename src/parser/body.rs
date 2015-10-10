use node::{ Body, Expr };
use parser::{ Parse, Context };
use { Token, TokenValue };
use { Result, Error, Expect };
use error::{ ErrorMessage };

impl<'c> Parse<'c> for Body<'c> {
    type Output = Body<'c>;

    fn parse<'r, I>(parser: &mut Context<'r, I>)
        -> Result<Body<'c>>
    where
        I: Iterator<Item=Result<Token<'c>>>
    {
        let mut maybe_token = parser.tokens.next();
        let _line_num = match maybe_token {
            Some(Ok(ref token)) => token.line,
            None => return Err(Error::new(ErrorMessage::UnexpectedEndOfTemplate)),
            Some(Err(e)) => return Err(e),
        };
        let mut rv = Vec::new();

        loop {
            match maybe_token {
                Some(Ok(ref token)) => match token.value {
                    TokenValue::Text(t) => rv.push(Body::Text { value: t, line: token.line }),
                    TokenValue::VarStart => {
                        let expr = try!(Expr::parse(parser));
                        try!(parser.tokens.expect(TokenValue::VarEnd));
                        rv.push(Body::Print { expr: Box::new(expr), line: token.line });
                    },
                    _ => unimplemented!(),
                },
                None => break,
                Some(Err(e)) => return Err(e),
            };

            maybe_token = parser.tokens.next();
        }

        if rv.len() == 1 {
            Ok(rv.remove(0))
        } else {
            Ok(Body::List { items: rv })
        }
    }
}
