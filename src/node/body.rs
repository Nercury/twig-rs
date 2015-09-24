use std::ops::Index;
use node::BodyNode;

pub struct Body<'a> {
    items: Vec<BodyNode<'a>>,
}

impl<'a> Body<'a> {
    pub fn new() -> Body<'a> {
        Body {
            items: Vec::new(),
        }
    }
}

impl<'a> Index<usize> for Body<'a> {
    type Output = BodyNode<'a>;

    fn index(&self, index: usize) -> &Self::Output {
        &self.items[index]
    }
}
