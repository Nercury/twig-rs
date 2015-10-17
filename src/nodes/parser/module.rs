use nodes::{ Parse, Parser, Module, Body };
use Result;

impl<'c> Parse<'c> for Module<'c> {
    type Output = Module<'c>;

    fn parse<'r>(parser: &mut Parser<'r, 'c>)
        -> Result<Module<'c>>
    {
        trace!("Module::parse");

        let module = Some(Module::new());

        // if parser.has_module() {
        //     unreachable!("Same context should not be used to parse multiple modules.");
        // }

        // swap(&mut module, &mut parser.module);
        let body = try!(Body::parse(parser));
        // swap(&mut module, &mut parser.module);

        if let Some(mut module) = module {
            module.body = body;
            return Ok(module);
        }

        unreachable!("Parser has consumed the module without giving it back.");
    }
}
