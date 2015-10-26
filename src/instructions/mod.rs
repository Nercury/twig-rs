use nodes::Module;
use error::Result;
use value::Value;
use little::{ Template, Instruction, Mem, Constant };

pub fn compile(env: (), nodes: &Module) -> Result<Template<Value>> {
    trace!("compile");
    Ok(Template::empty()
        .push_instructions(vec![
            Instruction::Output(Mem::Const(Constant(1)))
        ])
        .push_constant(Constant(1), Value::Str("&lt;br /&gt;\n&lt;br /&gt;\n<br />".into()))
    )
}
