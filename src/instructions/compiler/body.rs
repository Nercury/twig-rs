use little::{ Template, Instruction, Mem };
use instructions::Compile;
use nodes::body::Body;
use value::Value;
use error::TemplateResult;
use mold::Staging;

impl<'c> Compile<'c> for Body<'c> {
    fn compile<'r>(&'r self, stage: &'r mut Staging<'c, Value>) -> TemplateResult<()> {
        trace!("Body::compile");
        match *self {
            Body::List { ref items } => {
                trace!("Body::List::compile");
                for item in items {
                    try!(item.compile(stage));
                }
                Ok(())
            },
            Body::Text { .. } => unreachable!("Body::Text::compile"),
            Body::Print { ref expr, .. } => {
                trace!("Body::Print::compile");

                // expr will place single value on stack.
                try!(expr.compile(stage));

                // we output it.
                let instruction = Instruction::Output(Mem::StackTop1);
                trace!("instr {:?}", instruction);
                stage.template.push_instruction(instruction);

                // and pop the stack.
                let instruction = Instruction::Pop(1);
                trace!("instr {:?}", instruction);

                Ok(())
            },
            Body::Import { .. } => unreachable!("Body::Import::compile"),
            Body::Macro { .. } => unreachable!("Body::Macro::compile"),
        }
    }
}
