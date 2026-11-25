use crate::error::TemplateResult;
use crate::nodes::body::Body;
use crate::nodes::{Module, Parse, Parser};

impl<'c> Parse<'c> for Module<'c> {
    type Output = Module<'c>;

    fn parse<'r>(parser: &mut Parser<'r, 'c>) -> TemplateResult<Module<'c>> {
        let mut module = Module::new();
        let body = Body::parse(parser)?;

        module.body = body;

        Ok(module)
    }
}
