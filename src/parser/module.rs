use node::{ Module, Body };
use parser::{ Parse, Context };
use Result;

impl<'c> Parse<'c> for Module<'c> {
    type Output = Module<'c>;

    fn parse<'r>(parser: &mut Context<'r, 'c>)
        -> Result<Module<'c>>
    {
        println!("Module::parse");

        let mut module = Module::new();

        module.body = try!(Body::parse(parser));

        Ok(module)
    }
}
