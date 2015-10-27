use nodes::body::Body;

#[derive(Debug)]
pub struct Module<'c> {
    // Sub nodes.
    pub body: Body<'c>,
    // pub blocks: Vec<Block>,
    // pub macros: Vec<Macro>,
    // pub traits: Vec<Trait>,

    // Attributes.
    // file_id: Option<String>, // this must NOT be treated as file name
    // index: i32, // TODO: wtf is this
    // embedded_templates: Vec<EmbededTemplate>,

    // TODO: check usage of things bellow
    // display_start: Body<'c>,
    // display_end: Body<'c>,
    // constructor_start: Body<'c>,
    // constructor_end: Body<'c>,
    // class_end: Body<'c>,
}

/// Root Twig AST node.
impl<'c> Module<'c> {
    pub fn new() -> Module<'c> {
        Module {
            body: Body::new(),
            // blocks: vec![],
            // macros: vec![],
            // traits: vec![],

            // file_id: None,
            // index: 0,
            // embedded_templates: vec![],
            //
            // display_start: Body::new(),
            // display_end: Body::new(),
            // constructor_start: Body::new(),
            // constructor_end: Body::new(),
            // class_end: Body::new(),
        }
    }
}
