use node::{ Module, Body };
use parser::{ Parse, Context };
use { Token };
use Result;

impl<'a, 'code> Parse<'code> for Module<'a> {
    type Output = Module<'code>;

    fn parse<'r, I>(parser: &mut Context<'r, I>)
        -> Result<Module<'code>>
    where
        I: Iterator<Item=Result<Token<'code>>>
    {
        let mut module = Module::new();

        module.body = try!(Body::parse(parser));

        Ok(module)
    }
}
