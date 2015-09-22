use error::Result;
use token::Token;

pub struct Body;
pub struct Block;
pub struct Macro;
pub struct Trait;
pub struct AnyNode;

pub struct Module {
    // Sub nodes.
    body: Vec<Body>,
    blocks: Vec<Block>,
    macros: Vec<Macro>,
    traits: Vec<Trait>,

    // Attributes.
    file_id: Option<String>, // this must NOT be treated as file name
    index: i32, // TODO: wtf is this
    embedded_templates: Vec<()>, // TODO: should this be here

    // TODO: check usage of things bellow
    display_start: AnyNode,
    display_end: AnyNode,
    constructor_start: AnyNode,
    constructor_end: AnyNode,
    class_end: AnyNode,
}

/// Root Twig AST node.
impl Module {
    pub fn new() -> Module {
        Module {
            body: vec![],
            blocks: vec![],
            macros: vec![],
            traits: vec![],

            file_id: None,
            index: 0,
            embedded_templates: vec![],

            display_start: AnyNode,
            display_end: AnyNode,
            constructor_start: AnyNode,
            constructor_end: AnyNode,
            class_end: AnyNode,
        }
    }

    pub fn from_tokens<'code, I>(tokens: I)
        -> Result<Module>
            where I: IntoIterator<Item=Result<Token<'code>>>
    {
        let mut i = tokens.into_iter();
        i.next();
        Ok(Module::new())
    }
}
