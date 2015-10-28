use little::{ Instruction };
use instructions::{ Compile, CompileExpression };
use nodes::body::Body;
use value::Value;
use error::{ TemplateResult };
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

                let ce = try!(expr.compile(stage));
                if let Some(result) = ce.result() {
                    stage.instr(Instruction::Output(result));
                };
                try!(ce.finalize(stage));

                Ok(())
            },
            Body::Import { .. } => unreachable!("Body::Import::compile"),
            Body::Macro { .. } => unreachable!("Body::Macro::compile"),
        }
    }
}
