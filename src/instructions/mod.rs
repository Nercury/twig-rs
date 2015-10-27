pub use self::compiler::{ Compile };

use nodes::Module;
use error::Result;
use value::Value;
use little::{ Template };

mod compiler;

pub fn compile(env: (), nodes: &Module) -> Result<Template<Value>> {
    trace!("compile");
    let mut template = Template::empty();
    try!(nodes.compile(&mut template));
    Ok(template)
}
