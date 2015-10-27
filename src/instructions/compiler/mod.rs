use little::Template;
use value::Value;
use error::TemplateResult;
use mold::Staging;

mod body;
mod expr;
mod module;

pub trait Compile<'c> {
    fn compile<'r>(&'r self, stage: &'r mut Staging<'c, Value>) -> TemplateResult<()>;
}
