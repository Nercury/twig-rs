//! The parser callables of the core functions `parent()`, `block()` and `attribute()`
//! (`CoreExtension::parseParentFunction()` and friends, TWIG/src/Extension/CoreExtension.php).

use crate::ast::ConstantValue;
use crate::ast::{Arguments, CallType, Expr};
use crate::deprecation::Deprecation;
use crate::error::{Error, Result};
use crate::extension::arguments::{CallableArgumentsExtractor, is_array_or_variable};
use crate::extension::callable::{
    CallableParameter, FunctionOptions, PhpCallable, TwigCallable, TwigFunction,
};

use super::Parser;

/// Twig: `CoreExtension::parseParentFunction()`.
///
/// # Errors
///
/// `Calling the "parent" function outside of a block is forbidden.` and `Calling the "parent" function on a template
/// that does not call "extends" or "use" is forbidden.` (checked when `parent()` is parsed).
pub fn parse_parent_function(
    parser: &mut Parser<'_>,
    _arguments: Arguments,
    line: i64,
) -> Result<Expr> {
    // `if (!$blockName = $parser->peekBlockStack())`: PHP truthiness of the block name.
    let block_name = parser
        .peek_block_stack()
        .filter(|name| !(name.is_empty() || *name == b"0"))
        .map(<[u8]>::to_vec);
    let Some(block_name) = block_name else {
        return Err(Error::syntax(
            "Calling the \"parent\" function outside of a block is forbidden.",
        )
        .with_line(line)
        .with_source(parser.stream().source_context().clone()));
    };
    if !parser.has_inheritance() {
        return Err(Error::syntax(
            "Calling the \"parent\" function on a template that does not call \"extends\" or \"use\" is forbidden.",
        )
        .with_line(line)
        .with_source(parser.stream().source_context().clone()));
    }
    Ok(Expr::parent(block_name, line))
}

/// Twig: `CoreExtension::parseBlockFunction()`: the arguments are bound to `fn ($name, $template = null)`.
///
/// # Errors
///
/// The argument-binding errors (`Value for argument "name" is required for function "block".`, ...).
pub fn parse_block_function(
    parser: &mut Parser<'_>,
    arguments: Arguments,
    line: i64,
) -> Result<Expr> {
    let fake = TwigCallable::Function(TwigFunction::new(
        "block",
        Some(PhpCallable::closure(vec![
            CallableParameter::required("name"),
            CallableParameter::optional("template", ConstantValue::Null),
        ])),
        FunctionOptions::default(),
    ));
    let source = parser.stream().source_context().clone();
    let extractor = CallableArgumentsExtractor::new(&fake, line, Some(&source), false)?;
    let mut args = extractor
        .extract_arguments(&arguments, parser.deprecations())?
        .into_iter();
    let name = args
        .next()
        .ok_or_else(|| Error::logic("Undefined array key 0"))?;
    let template = args.next();
    Ok(Expr::block_reference(name, template, line))
}

/// Twig: `CoreExtension::parseAttributeFunction()`: the arguments are bound to
/// `fn ($variable, $attribute, $arguments = null)`.
///
/// # Errors
///
/// The argument-binding errors.
pub fn parse_attribute_function(
    parser: &mut Parser<'_>,
    arguments: Arguments,
    line: i64,
) -> Result<Expr> {
    let fake = TwigCallable::Function(TwigFunction::new(
        "attribute",
        Some(PhpCallable::closure(vec![
            CallableParameter::required("variable"),
            CallableParameter::required("attribute"),
            CallableParameter::optional("arguments", ConstantValue::Null),
        ])),
        FunctionOptions::default(),
    ));
    let source = parser.stream().source_context().clone();
    let extractor = CallableArgumentsExtractor::new(&fake, line, Some(&source), false)?;
    let mut args = extractor
        .extract_arguments(&arguments, parser.deprecations())?
        .into_iter();
    let variable = args
        .next()
        .ok_or_else(|| Error::logic("Undefined array key 0"))?;
    let attribute = args
        .next()
        .ok_or_else(|| Error::logic("Undefined array key 1"))?;
    let call_arguments = args.next();
    if let Some(call_arguments) = &call_arguments
        && !is_array_or_variable(call_arguments)
    {
        // GetAttrExpression::__construct()
        parser.report_deprecation(Deprecation::twig(
            "3.15",
            format!(
                "Not passing a \"Twig\\Node\\Expression\\ArrayExpression\" instance as the \"arguments\" argument of the \"Twig\\Node\\Expression\\GetAttrExpression\" constructor is deprecated (\"{}\" given).",
                call_arguments.class_name()
            ),
        ));
    }
    Ok(Expr::get_attr(
        variable,
        attribute,
        call_arguments,
        CallType::Any,
        line,
        false,
    ))
}
