use little::{ Template };
use instructions::Compile;
use nodes::body::Body;
use value::Value;
use error::TemplateResult;

impl<'c> Compile for Body<'c> {
    fn compile(&self, template: &mut Template<Value>) -> TemplateResult<()> {
        match *self {
            Body::List { ref items } => {
                for item in items {
                    try!(item.compile(template));
                }
                Ok(())
            },
            _ => Ok(())
        }
    }
}
