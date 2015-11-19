use instructions::{ CompileExpression, CompiledExpression };
use nodes::expr::{ Expr, ExprValue };
use value::Value;
use error::TemplateResult;
use mold::Staging;

impl<'c> CompileExpression<'c> for Expr<'c> {
    fn compile<'r>(&'r self, stage: &'r mut Staging<'c, Value>) -> TemplateResult<CompiledExpression> {
        trace!("Expr::compile");
        Ok(match self.value {
            ExprValue::Constant(_) => unreachable!("ExprValue::Constant::compile"),
            ExprValue::Name(name) => {
                let maybe_mem = stage.use_name(name);

                let name_mem = match maybe_mem {
                    Some(mem) => {
                        trace!("use mem {:?} for name {:?}", mem, name);
                        mem
                    },
                    None => {
                        stage.include_const(Value::Str(name.into()))
                    }
                };

                CompiledExpression::with_result("ExprValue::Name", name_mem)
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
            ExprValue::FunctionCall { name, ref arguments } => {
                CompiledExpression::empty("function call")
            },
        })
    }
}
