use Expect;
use Token;
use TokenValue;
use CompiledEnvironment;
use node::Module;
use node::Body;
use node::Expr;
use Result;

pub trait Parse<'code> {
    type Output;

    fn parse<'r, I>(parser: &mut Context<'r, I>)
        -> Result<Self::Output>
    where
        I: Iterator<Item=Result<Token<'code>>>;
}

pub struct Context<'a, I: 'a>
{
    pub env: &'a CompiledEnvironment,
    pub tokens: &'a mut I,
}

impl<'a, I: 'a> Context<'a, I> {
    pub fn new<'r>(
        env: &'r CompiledEnvironment,
        tokens: &'r mut I
    ) -> Context<'r, I> {
        Context {
            env: env,
            tokens: tokens,
        }
    }
}

impl<'a, 'code> Parse<'code> for Body<'a> {
    type Output = Body<'code>;

    fn parse<'r, I>(parser: &mut Context<'r, I>)
        -> Result<Body<'code>>
    where
        I: Iterator<Item=Result<Token<'code>>>
    {
        let mut maybe_token = parser.tokens.next();
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
                        let expr = try!(Expr::from_tokens(parser.tokens));
                        try!(parser.tokens.expect(TokenValue::VarEnd));
                        rv.push(Body::Print(expr, token.line_num));
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
            Ok(Body::List(rv))
        }
    }
}

impl<'a, 'code> Parse<'code> for Module<'a> {
    type Output = Module<'code>;

    fn parse<'r, I>(parser: &mut Context<'r, I>)
        -> Result<Module<'code>>
    where
        I: Iterator<Item=Result<Token<'code>>>
    {
        let mut module = Module::new();

        module.body = try!(Body::parse(parser));

        Ok(module)
    }
}
