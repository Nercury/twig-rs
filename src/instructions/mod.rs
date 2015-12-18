pub use self::compiler::{
    Compile, CompileExpression, CompiledExpression
};

use environment::CompilingEnvironment;
use nodes::Module;
use error::Result;
use value::Value;
use little::{ Template };
use mold::Staging;

mod compiler;

pub fn compile(env: &CompilingEnvironment, nodes: &Module) -> Result<Template<Value>> {
    let mut stage = Staging::new();
    try!(nodes.compile(&mut stage));
    Ok(stage.into())
}
