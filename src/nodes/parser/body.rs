use nodes::{ Parser, Parse };
use nodes::body::Body;
use nodes::expr::Expr;
use tokens::{ TokenRef, TokenValueRef };
use Expect;
use error::{ Result, ErrorAt, ErrorMessage };

impl<'c> Parse<'c> for Body<'c> {
    type Output = Body<'c>;

    fn parse<'r>(parser: &mut Parser<'r, 'c>)
        -> Result<Body<'c>>
    {
        trace!("Body::parse");

        subparse(parser, |_| None)
    }
}

pub struct BlockEnd {
    pub drop_needle: bool,
}

pub fn subparse<'p, 'c, D>(parser: &mut Parser<'p, 'c>, test: D)
    -> Result<Body<'c>>
        where D: Fn(&TokenRef<'c>) -> Option<BlockEnd>
{
    let mut maybe_line = None;
    let mut rv = Vec::new();

    while let Some(token) = try!(parser.maybe_current()) {
        if let None = maybe_line {
            maybe_line = Some(token.line);
        }
        match token.value {
            TokenValueRef::Text(t) => {
                try!(parser.next());
                rv.push(Body::Text { value: t, line: token.line })
            },
            TokenValueRef::VarStart => {
                try!(parser.next());
                let expr = try!(Expr::parse(parser));
                try!(parser.expect(TokenValueRef::VarEnd));
                rv.push(Body::Print { expr: Box::new(expr), line: token.line });
            },
            TokenValueRef::BlockStart => {
                try!(parser.next());
                let token = try!(parser.current());

                let tag_name = match token.value {
                    TokenValueRef::Name(n) => n,
                    _ => return Err(ErrorAt::new_at(ErrorMessage::MustStartWithTagName, token.line)),
                };

                if let Some(end) = test(&token) {
                    if end.drop_needle {
                        try!(parser.next());
                    }
                    return if 1 == rv.len() {
                        Ok(rv.remove(0))
                    } else {
                        Ok(Body::List { items: rv })
                    }
                }

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
