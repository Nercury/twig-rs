mod body;
mod body_node;
mod expr;

use error::Result;
use token;
use token::Token;

pub use self::body::Body;
pub use self::body_node::BodyNode;
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
    display_start: BodyNode<'a>,
    display_end: BodyNode<'a>,
    constructor_start: BodyNode<'a>,
    constructor_end: BodyNode<'a>,
    class_end: BodyNode<'a>,
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

            display_start: BodyNode::new(),
            display_end: BodyNode::new(),
            constructor_start: BodyNode::new(),
            constructor_end: BodyNode::new(),
            class_end: BodyNode::new(),
        }
    }

    pub fn from_tokens<'code, I>(mut tokens: I)
        -> Result<Module<'code>>
            where I: Iterator<Item=Result<Token<'code>>>
    {
        let mut module = Module::new();

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
                    token::Value::Text(t) => rv.push(BodyNode::Text(t, token.line_num)),
                    _ => (),
                },
                None => break,
                Some(Err(e)) => return Err(e),
            };

            maybe_token = tokens.next();
        }

        Ok(module)
    }
}
