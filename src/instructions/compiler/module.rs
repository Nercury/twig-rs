use little::{ Template };
use instructions::Compile;
use nodes::Module;
use value::Value;
use error::TemplateResult;

impl<'c> Compile for Module<'c> {
    fn compile(&self, template: &mut Template<Value>) -> TemplateResult<()> {
        trace!("Module::compile");
        self.body.compile(template)
    }
}
