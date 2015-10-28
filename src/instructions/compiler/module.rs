use instructions::Compile;
use nodes::Module;
use value::Value;
use error::TemplateResult;
use mold::Staging;

impl<'c> Compile<'c> for Module<'c> {
    fn compile<'r>(&'r self, stage: &'r mut Staging<'c, Value>) -> TemplateResult<()> {
        trace!("Module::compile");
        self.body.compile(stage)
    }
}
