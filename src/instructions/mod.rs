use nodes::Module;
use error::Result;
use value::Value;
use little::Template;

pub fn compile(env: (), nodes: &Module) -> Result<Template<Value>> {
    trace!("compile");
    Ok(Template::empty())
}
