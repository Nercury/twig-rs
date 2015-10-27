use little::Template;
use value::Value;
use error::TemplateResult;

mod body;
mod module;

pub trait Compile {
    fn compile(&self, template: &mut Template<Value>) -> TemplateResult<()>;
}
