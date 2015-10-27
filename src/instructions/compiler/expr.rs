use little::{ Template, Instruction };
use instructions::Compile;
use nodes::expr::{ Expr, ExprValue };
use value::Value;
use error::TemplateResult;
use mold::Staging;

impl<'c> Compile<'c> for Expr<'c> {
    fn compile<'r>(&'r self, stage: &'r mut Staging<'c, Value>) -> TemplateResult<()> {
        trace!("Expr::compile");
        match self.value {
            ExprValue::Constant(_) => unreachable!("ExprValue::Constant::compile"),
            ExprValue::Name(name) => {
                let name_mem = stage.use_name(name);
                trace!("use mem {:?} for name {:?}", name_mem, name);
                let instruction = Instruction::Push(name_mem);
                trace!("instr {:?}", instruction);
                stage.template.push_instruction(instruction);
            },
            ExprValue::AssignName(_) => unreachable!("ExprValue::AssignName::compile"),
            ExprValue::Array(_) => unreachable!("ExprValue::Array::compile"),
            ExprValue::Hash(_) => unreachable!("ExprValue::Hash::compile"),
            ExprValue::UnaryOperator { .. } => unreachable!("ExprValue::UnaryOperator::compile"),
            ExprValue::BinaryOperator { .. } => unreachable!("ExprValue::UnaryOperator::compile"),
            ExprValue::Concat { .. } => unreachable!("ExprValue::Concat::compile"),
            ExprValue::Conditional { .. } => unreachable!("ExprValue::Conditional::compile"),
            ExprValue::GetAttr { .. } => unreachable!("ExprValue::GetAttr::compile"),
            ExprValue::ImportedFunctionCall { .. } => unreachable!("ExprValue::ImportedFunctionCall::compile"),
            ExprValue::FunctionCall { .. } => unreachable!("ExprValue::FunctionCall::compile"),
        };

        Ok(())
    }
}
