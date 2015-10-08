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
}
