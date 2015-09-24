mod body;
mod body_node;
mod expr;

use error::Result;
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

    pub fn from_tokens<'code, I>(tokens: I)
        -> Result<Module<'code>>
            where I: IntoIterator<Item=Result<Token<'code>>>
    {
        let mut i = tokens.into_iter();
        i.next();
        Ok(Module::new())
    }
}
