mod parser_block;
mod parser_do;
mod parser_embed;
mod parser_extends;
mod parser_filter;
mod parser_flush;
mod parser_for;
mod parser_from;
mod parser_if;
mod parser_import;
mod parser_include;
mod parser_macro;
mod parser_set;
mod parser_spaceless;
mod parser_use;

pub use self::parser_block::Block;
pub use self::parser_do::Do;
pub use self::parser_embed::Embed;
pub use self::parser_extends::Extends;
pub use self::parser_filter::Filter;
pub use self::parser_flush::Flush;
pub use self::parser_for::For;
pub use self::parser_from::From;
pub use self::parser_if::If;
pub use self::parser_import::Import;
pub use self::parser_include::Include;
pub use self::parser_macro::Macro;
pub use self::parser_set::Set;
pub use self::parser_spaceless::Spaceless;
pub use self::parser_use::Use;

use super::error::*;
use crate::error::TemplateResult;
use crate::nodes::Parser;
use crate::nodes::expr::{Expr, ExprValue};
use crate::tokens::TokenValueRef;

const INVALID_LVALUES: [&str; 3] = ["true", "false", "none"];

pub fn parse_assignment_expression<'p, 'c>(
    parser: &mut Parser<'p, 'c>,
) -> TemplateResult<Vec<Expr<'c>>> {
    let mut targets = Vec::new();
    loop {
        let token = parser.current()?;
        let name = match token.value {
            TokenValueRef::Name(name) => {
                parser.next()?;
                name
            }
            _ => {
                return Err(CoreTemplateError::OnlyVariablesCanBeAssignedTo
                    .at(token.line)
                    .into());
            }
        };

        if INVALID_LVALUES.contains(&name) {
            return Err(CoreTemplateError::CanNotAssignTo(name.into())
                .at(token.line)
                .into());
        }

        targets.push(Expr::new_at(ExprValue::AssignName(name), token.line));

        if !parser.skip_to_next_if(TokenValueRef::Punctuation(','))? {
            break;
        }
    }

    Ok(targets)
}
