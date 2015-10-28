use little::{ Mem, Instruction };
use value::Value;
use error::TemplateResult;
use mold::Staging;

mod body;
mod expr;
mod module;

pub trait Compile<'c> {
    fn compile<'r>(&'r self, stage: &'r mut Staging<'c, Value>) -> TemplateResult<()>;
}

/// Represents a mess created by a compiled expression.
///
/// It is up to the caller to clean up this mess by calling `finalize` on this struct.
pub struct CompiledExpression {
    origin: &'static str,
    stack_length: u16,
    result: Mem,
    finalized: bool,
}

impl CompiledExpression {
    pub fn with_result(origin: &'static str, result: Mem) -> CompiledExpression {
        CompiledExpression {
            origin: origin,
            stack_length: 0,
            result: result,
            finalized: false,
        }
    }

    pub fn new(origin: &'static str, result: Mem, stack_length: u16) -> CompiledExpression {
        CompiledExpression {
            origin: origin,
            stack_length: stack_length,
            result: result,
            finalized: false,
        }
    }

    pub fn result(&self) -> Mem {
        self.result.clone()
    }

    pub fn finalize<'c, 'r>(mut self, stage: &'r mut Staging<'c, Value>) -> TemplateResult<()> {
        if self.stack_length > 0 {
            trace!("finalize {}", self.origin);
            stage.instr(Instruction::Pop(self.stack_length));
        }
        self.finalized = true;
        Ok(())
    }
}

impl Drop for CompiledExpression {
    fn drop(&mut self) {
        if !self.finalized {
            panic!("finalize never called on {}!", self.origin);
        }
    }
}

pub trait CompileExpression<'c> {
    /// Compiles ast subnodes that return result in
    fn compile<'r>(&'r self, stage: &'r mut Staging<'c, Value>) -> TemplateResult<CompiledExpression>;
}
