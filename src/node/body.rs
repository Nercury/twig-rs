use node::Expr;
use Token;
use TokenValue;
use node::Module;
use Result;
use Expect;

#[derive(Debug)]
pub enum Body<'a> {
    List(Vec<Body<'a>>),
    Text(&'a str, usize),
    Print(Expr<'a>, usize),
}

impl<'a> Body<'a> {
    pub fn new() -> Body<'a> {
        Body::List(Vec::new())
    }

    pub fn from_tokens<'code, I>(tokens: &mut I)
        -> Result<Body<'code>>
            where I: Iterator<Item=Result<Token<'code>>>
    {
        let mut maybe_token = tokens.next();
        let mut line_num = match maybe_token {
            Some(Ok(ref token)) => token.line_num,
            None => 1,
            Some(Err(e)) => return Err(e),
        };
        let mut rv = Vec::new();

        loop {
            match maybe_token {
                Some(Ok(ref token)) => match token.value {
                    TokenValue::Text(t) => rv.push(Body::Text(t, token.line_num)),
                    TokenValue::VarStart => {
                        let expr = try!(Module::parse_expr(tokens));
                        try!(tokens.expect(TokenValue::VarEnd));
                        rv.push(Body::Print(expr, token.line_num));
                    },
                    _ => unimplemented!(),
                },
                None => break,
                Some(Err(e)) => return Err(e),
            };

            maybe_token = tokens.next();
        }

        if rv.len() == 1 {
            Ok(rv.remove(0))
        } else {
            Ok(Body::List(rv))
        }
    }

    pub fn expect_print<'r>(&'a self) -> &'r Expr<'a> {
        match *self {
            Body::Print(ref e, _) => e,
            ref what => panic!("Expected expect_print to return Expr but received {:?}", what),
        }
    }

    pub fn expect_list<'r>(&'a self) -> &'r Vec<Body<'a>> {
        match *self {
            Body::List(ref list) => list,
            ref what => panic!("Expected expect_list to return Vec but received {:?}", what),
        }
    }
}
