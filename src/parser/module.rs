use node::{ Module, Body };
use parser::{ Parse, Context };
use { Token };
use Result;

impl<'c> Parse<'c> for Module<'c> {
    type Output = Module<'c>;

    fn parse<'r, I>(parser: &mut Context<'r, I>)
        -> Result<Module<'c>>
    where
        I: Iterator<Item=Result<Token<'c>>>
    {
        let mut module = Module::new();

        module.body = try!(Body::parse(parser));

        Ok(module)
    }
}
