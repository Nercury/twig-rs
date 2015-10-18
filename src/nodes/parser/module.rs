use nodes::{ Parse, Parser, Module };
use nodes::body::Body;
use error::TemplateResult;

impl<'c> Parse<'c> for Module<'c> {
    type Output = Module<'c>;

    fn parse<'r>(parser: &mut Parser<'r, 'c>)
        -> TemplateResult<Module<'c>>
    {
        trace!("Module::parse");

        let mut module = Module::new();
        let body = try!(Body::parse(parser));

        module.body = body;

        Ok(module)
    }
}
