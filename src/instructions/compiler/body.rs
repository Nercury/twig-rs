use little::{ Template };
use instructions::Compile;
use nodes::body::Body;
use value::Value;
use error::TemplateResult;

impl<'c> Compile for Body<'c> {
    fn compile(&self, template: &mut Template<Value>) -> TemplateResult<()> {
        trace!("Body::compile");
        match *self {
            Body::List { ref items } => {
                trace!("List::compile");
                for item in items {
                    try!(item.compile(template));
                }
                Ok(())
            },
            _ => Ok(())
        }
    }
}
