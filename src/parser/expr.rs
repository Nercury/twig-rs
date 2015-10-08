use node::{ Expr };
use parser::{ Parse, Context };
use { Token };
use Result;
use Error;
use error::ErrorMessage;

impl<'a, 'code> Parse<'code> for Expr<'a> {
    type Output = Expr<'code>;

    fn parse<'r, I>(parser: &mut Context<'r, I>)
        -> Result<Expr<'code>>
    where
        I: Iterator<Item=Result<Token<'code>>>
    {
        println!("parse");
        parse_with_precedence_0(parser)
    }
}

fn parse_with_precedence_0<'r, 'c, I>(parser: &mut Context<'r, I>)
    -> Result<Expr<'c>>
    where
        I: Iterator<Item=Result<Token<'c>>>
{
    println!("parse_with_precedence_0");
    parse_primary(parser)
}

/// Parses expression and returns handle to one that should be executed first.
fn parse_primary<'r, 'c, I>(parser: &mut Context<'r, I>)
    -> Result<Expr<'c>>
    where
        I: Iterator<Item=Result<Token<'c>>>
{
    let token = match parser.tokens.next() {
        Some(Ok(t)) => t,
        None => return Err(Error::new(ErrorMessage::UnexpectedEndOfTemplate)),
        Some(Err(e)) => return Err(e),
    };

    println!("parse_primary");
    Ok(Expr::Constant { value: "", line: 1 })
}
