mod body;
mod expr;

use token;
use token::Token;
use Result;
use Expect;

pub use self::body::Body;
pub use self::expr::Expr;

pub struct Block;
pub struct Macro;
pub struct Trait;
pub struct EmbededTemplate; // reference to another module

pub struct Module<'a> {
    // Sub nodes.
    pub body: Body<'a>,
    pub blocks: Vec<Block>,
    pub macros: Vec<Macro>,
    pub traits: Vec<Trait>,

    // Attributes.
    file_id: Option<String>, // this must NOT be treated as file name
    index: i32, // TODO: wtf is this
    embedded_templates: Vec<EmbededTemplate>,

    // TODO: check usage of things bellow
    display_start: Body<'a>,
    display_end: Body<'a>,
    constructor_start: Body<'a>,
    constructor_end: Body<'a>,
    class_end: Body<'a>,
}

/// Root Twig AST node.
impl<'a> Module<'a> {
    pub fn new() -> Module<'a> {
        Module {
            body: Body::new(),
            blocks: vec![],
            macros: vec![],
            traits: vec![],

            file_id: None,
            index: 0,
            embedded_templates: vec![],

            display_start: Body::new(),
            display_end: Body::new(),
            constructor_start: Body::new(),
            constructor_end: Body::new(),
            class_end: Body::new(),
        }
    }

    pub fn from_tokens<'code, I>(mut tokens: I)
        -> Result<Module<'code>>
            where I: Iterator<Item=Result<Token<'code>>>
    {
        let mut module = Module::new();

        module.body = try!(Self::parse_body(&mut tokens));

        Ok(module)
    }

    fn parse_body<'code, I>(tokens: &mut I)
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
                    token::Value::Text(t) => rv.push(Body::Text(t, token.line_num)),
                    token::Value::VarStart => {
                        let expr = try!(Self::parse_expr(tokens));
                        try!(tokens.expect(token::Value::VarEnd));
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

    fn parse_expr<'code, I>(tokens: &mut I)
        -> Result<Expr<'code>>
            where I: Iterator<Item=Result<Token<'code>>>
    {
        unimplemented!();
        Ok(Expr::Constant("", 1))
    }
}
