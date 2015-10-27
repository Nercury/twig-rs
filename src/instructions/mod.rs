pub use self::compiler::{ Compile };

use nodes::Module;
use error::Result;
use value::Value;
use little::{ Template };
use mold::Staging;

mod compiler;

pub fn compile(env: (), nodes: &Module) -> Result<Template<Value>> {
    trace!("compile");
    let mut stage = Staging::new();
    try!(nodes.compile(&mut stage));
    Ok(Template::empty())
}
