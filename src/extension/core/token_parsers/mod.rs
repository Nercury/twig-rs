mod parser_for;
mod parser_if;
mod parser_extends;
mod parser_include;
mod parser_block;
mod parser_use;
mod parser_filter;
mod parser_macro;
mod parser_import;
mod parser_from;
mod parser_set;
mod parser_spaceless;
mod parser_flush;
mod parser_do;
mod parser_embed;

pub use self::parser_for::For;
pub use self::parser_if::If;
pub use self::parser_extends::Extends;
pub use self::parser_include::Include;
pub use self::parser_block::Block;
pub use self::parser_use::Use;
pub use self::parser_filter::Filter;
pub use self::parser_macro::Macro;
pub use self::parser_import::Import;
pub use self::parser_from::From;
pub use self::parser_set::Set;
pub use self::parser_spaceless::Spaceless;
pub use self::parser_flush::Flush;
pub use self::parser_do::Do;
pub use self::parser_embed::Embed;

use nodes::Parser;
use nodes::expr::{ Expr, ExprValue };
use tokens::TokenValueRef;
use super::error::*;
use error::Result;

const INVALID_LVALUES: [&'static str; 3] = ["true", "false", "none"];

pub fn parse_assignment_expression<'p, 'c>(parser: &mut Parser<'p, 'c>)
    -> Result<Vec<Expr<'c>>>
{
    trace!("parse_assignment_expression");

    let mut targets = Vec::new();
    loop {
        let token = try!(parser.current());
        let name = match token.value {
            TokenValueRef::Name(name) => {
                try!(parser.next());
                name
            },
            _ => return Err(
                CoreError::new_at(CoreErrorMessage::OnlyVariablesCanBeAssignedTo, token.line)
                    .into()
            ),
        };

        if INVALID_LVALUES.contains(&name) {
            return Err(
                CoreError::new_at(CoreErrorMessage::CanNotAssignTo(name.into()), token.line)
                    .into()
            )
        }

        targets.push(Expr::new_at(ExprValue::AssignName(name), token.line));

        if !try!(parser.skip_to_next_if(TokenValueRef::Punctuation(','))) {
            break;
        }
    }

    Ok(targets)
}
