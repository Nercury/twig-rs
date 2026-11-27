//! Binding of template call arguments to the PHP parameters of a callable.
//!
//! Port of `Twig\Util\CallableArgumentsExtractor` (TWIG/src/Util/CallableArgumentsExtractor.php) and of
//! `Twig\Util\ReflectionCallable::getTwigParameters()`, on top of the parameter lists registered with
//! [`PhpCallable`](super::PhpCallable).

use std::collections::HashMap;

use super::callable::{CallableParameter, TwigCallable};
use crate::ast::{ArgumentKey, Arguments, ArrayExpr, ConstantValue, Expr, ExprKind, Lossy};
use crate::deprecation::{Deprecation, DeprecationSink};
use crate::error::{Error, Result};
use crate::source::Source;

/// Extracts the arguments of a call for a callable.
///
/// Twig builds the extractor from the call node; what it uses of that node is its line, its source and whether it
/// has a `node` child (filters and tests: the input is the first PHP parameter).
pub struct CallableArgumentsExtractor<'a> {
    callable: &'a TwigCallable,
    line: i64,
    source: Option<&'a Source>,
    strip_input: bool,
}

/// A PHP-normalised argument key: `strtolower(str_replace('_', '', $name))` for names, the int for positions.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Normalized {
    Index(i64),
    Name(Vec<u8>),
}

impl<'a> CallableArgumentsExtractor<'a> {
    /// Twig: `new CallableArgumentsExtractor($node, $twigCallable)`; `line` and `source` are the call node's,
    /// `strip_input` is `$node->hasNode('node')`.
    ///
    /// # Errors
    ///
    /// `Callback for %s "%s" is not callable in the current scope.` (a `LogicException`) for a callable without a
    /// PHP callable, which `ReflectionCallable` cannot reflect.
    pub fn new(
        callable: &'a TwigCallable,
        line: i64,
        source: Option<&'a Source>,
        strip_input: bool,
    ) -> Result<Self> {
        if callable.callable().is_none() {
            return Err(Error::logic(format!(
                "Callback for {} \"{}\" is not callable in the current scope.",
                callable.callable_type().as_str(),
                Lossy(callable.name())
            )));
        }
        Ok(CallableArgumentsExtractor {
            callable,
            line,
            source,
            strip_input,
        })
    }

    fn error(&self, message: String) -> Error {
        let mut error = Error::syntax(message).with_line(self.line);
        if let Some(source) = self.source {
            error = error.with_source(source.clone());
        }
        error
    }

    /// Twig: `CallableArgumentsExtractor::extractArguments()`: positional arguments are passed through as long as
    /// no argument is named and the callable is not variadic; otherwise arguments are bound to the PHP parameters
    /// by name (normalised: case and underscores ignored) or position, defaults fill the gaps before a given
    /// argument, and variadic callables collect the rest.
    ///
    /// # Errors
    ///
    /// The binding errors with their verbatim messages: `Positional arguments cannot be used after named arguments
    /// for %s "%s".`, `Value for argument "%s" is required for %s "%s".`, `Argument "%s" is defined twice for %s
    /// "%s".`, `Unknown argument%s "%s" for %s "%s(%s)".`, ...
    pub fn extract_arguments(
        &self,
        arguments: &Arguments,
        deprecations: &mut dyn DeprecationSink,
    ) -> Result<Vec<Expr>> {
        let callable_type = self.callable.callable_type().as_str();
        let callable_name = Lossy(self.callable.name());
        let mut extracted: Vec<(Normalized, Expr)> = Vec::with_capacity(arguments.len());
        let mut name_map: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
        // `$extracted[$name] = $node`: where each normalised name is in `extracted`. The int keys of `Arguments` are
        // distinct and come before every name, so only names can repeat (`foo_bar` and `fooBar`), and a lookup in
        // this index stands for PHP's hash lookup instead of a scan of every argument.
        let mut named_at: HashMap<Vec<u8>, usize> = HashMap::new();
        let mut named = false;
        for (key, node) in arguments.entries() {
            match key {
                ArgumentKey::Name(name) => {
                    named = true;
                    let normalized = normalize_name(name);
                    set_name(&mut name_map, normalized.clone(), name.clone());
                    match named_at.get(&normalized) {
                        Some(&at) => extracted[at].1 = node.clone(),
                        None => {
                            named_at.insert(normalized.clone(), extracted.len());
                            extracted.push((Normalized::Name(normalized), node.clone()));
                        }
                    }
                }
                ArgumentKey::Index(i) => {
                    if named {
                        return Err(self.error(format!(
                            "Positional arguments cannot be used after named arguments for {callable_type} \"{callable_name}\"."
                        )));
                    }
                    extracted.push((Normalized::Index(*i), node.clone()));
                }
            }
        }

        // The constructor checked that there is a PHP callable.
        let php = self
            .callable
            .callable()
            .ok_or_else(|| Error::logic("The callable has no PHP callable."))?;

        if !named && !self.callable.is_variadic() {
            let min = self.callable.minimal_number_of_required_arguments() as i64;
            let required = php.number_of_required_parameters() as i64;
            if (extracted.len() as i64) < required - min {
                let index = (min + extracted.len() as i64) as usize;
                let name = php
                    .parameters()
                    .get(index)
                    .map(|p| p.name.as_str())
                    .unwrap_or_default();
                return Err(self.error(format!(
                    "Value for argument \"{}\" is required for {callable_type} \"{callable_name}\".",
                    to_snake_case(name)
                )));
            }
            return Ok(extracted.into_iter().map(|(_, node)| node).collect());
        }

        let (parameters, is_php_variadic) = self.callable_parameters()?;
        let mut result: Vec<Expr> = Vec::new();
        let mut parameter_names: Vec<String> = Vec::new();
        let mut missing: Vec<String> = Vec::new();
        let mut optional: Vec<Expr> = Vec::new();
        let mut pos: i64 = 0;
        for parameter in &parameters {
            let mut parameter_name = parameter.name.clone();
            if php.is_php_function("range") {
                if parameter_name == "start" {
                    parameter_name = "low".to_owned();
                } else if parameter_name == "end" {
                    parameter_name = "high".to_owned();
                }
            }
            parameter_names.push(parameter_name.clone());
            let normalized = Normalized::Name(normalize_name(parameter_name.as_bytes()));

            if let Some(value) = take(&mut extracted, &normalized) {
                if find(&extracted, &Normalized::Index(pos)).is_some() {
                    return Err(self.error(format!(
                        "Argument \"{parameter_name}\" is defined twice for {callable_type} \"{callable_name}\"."
                    )));
                }
                if !missing.is_empty() {
                    let list: Vec<String> =
                        parameter_names.iter().map(|n| to_snake_case(n)).collect();
                    return Err(self.error(format!(
                        "Argument \"{parameter_name}\" could not be assigned for {callable_type} \"{callable_name}({})\" because it is mapped to an internal PHP function which cannot determine default value for optional argument{} \"{}\".",
                        list.join(", "),
                        if missing.len() > 1 { "s" } else { "" },
                        missing.join("\", \"")
                    )));
                }
                result.append(&mut optional);
                result.push(value);
            } else if let Some(value) = take(&mut extracted, &Normalized::Index(pos)) {
                result.append(&mut optional);
                result.push(value);
                pos += 1;
            } else if let Some(default) = parameter.default_value() {
                optional.push(Expr::constant(default.clone(), self.line));
            } else if parameter.is_optional() {
                if extracted.is_empty() {
                    break;
                }
                missing.push(parameter_name);
            } else {
                return Err(self.error(format!(
                    "Value for argument \"{}\" is required for {callable_type} \"{callable_name}\".",
                    to_snake_case(&parameter_name)
                )));
            }
        }

        if self.callable.is_variadic() {
            let mut arbitrary = ArrayExpr::new();
            arbitrary.variadic = is_php_variadic;
            for (key, value) in std::mem::take(&mut extracted) {
                match key {
                    Normalized::Index(_) => arbitrary.add_element(value, None),
                    Normalized::Name(normalized) => {
                        let original = name_map
                            .iter()
                            .find(|(n, _)| *n == normalized)
                            .map(|(_, o)| o.clone())
                            .unwrap_or_default();
                        let original_str = String::from_utf8_lossy(&original).into_owned();
                        let snake = to_snake_case(&original_str);
                        if original_str != snake {
                            deprecations.report(Deprecation::twig(
                                "3.15",
                                format!(
                                    "Using \"snake_case\" for variadic arguments is required for a smooth upgrade with Twig 4.0; rename \"{original_str}\" to \"{snake}\" in \"{}\" at line {}.",
                                    self.source.map_or("", Source::name),
                                    self.line
                                ),
                            ));
                        }
                        arbitrary.add_element(
                            value,
                            Some(Expr::constant(
                                ConstantValue::String(snake.into_bytes()),
                                self.line,
                            )),
                        );
                    }
                }
            }
            if !arbitrary.elements.is_empty() {
                result.append(&mut optional);
                result.push(arbitrary.into_expr(self.line));
            }
        }

        if !extracted.is_empty() {
            let unknown: Vec<String> = extracted
                .iter()
                .map(|(key, _)| match key {
                    Normalized::Index(i) => i.to_string(),
                    Normalized::Name(name) => String::from_utf8_lossy(name).into_owned(),
                })
                .collect();
            let list: Vec<String> = parameter_names.iter().map(|n| to_snake_case(n)).collect();
            let first = &extracted[0].1;
            let mut error = Error::syntax(format!(
                "Unknown argument{} \"{}\" for {callable_type} \"{callable_name}({})\".",
                if unknown.len() > 1 { "s" } else { "" },
                unknown.join("\", \""),
                list.join(", ")
            ))
            .with_line(first.line);
            if let Some(source) = self.source {
                error = error.with_source(source.clone());
            }
            return Err(error);
        }

        Ok(result)
    }

    /// Twig: `CallableArgumentsExtractor::getCallableParameters()` with `ReflectionCallable::getTwigParameters()`:
    /// the PHP parameters minus the input (filters, tests), the injected values and the dynamic-name arguments;
    /// for variadic callables also minus the last parameter, which must be `array $x = []` or a PHP variadic.
    fn callable_parameters(&self) -> Result<(Vec<CallableParameter>, bool)> {
        let php = self
            .callable
            .callable()
            .ok_or_else(|| Error::logic("The callable has no PHP callable."))?;
        let mut parameters: Vec<CallableParameter> = php.parameters().to_vec();
        let mut shift = |condition: bool| {
            if condition && !parameters.is_empty() {
                parameters.remove(0);
            }
        };
        shift(self.strip_input);
        shift(self.callable.needs_charset());
        shift(self.callable.needs_environment());
        shift(self.callable.needs_context());
        shift(self.callable.needs_is_sandboxed());
        for _ in self.callable.dynamic_arguments() {
            shift(true);
        }
        let mut is_php_variadic = false;
        if self.callable.is_variadic() {
            let last = parameters.last();
            let is_array_bucket = last.is_some_and(|p| {
                p.array_type && p.default_value() == Some(&ConstantValue::Array(Vec::new()))
            });
            if is_array_bucket {
                parameters.pop();
            } else if last.is_some_and(|p| p.variadic) {
                parameters.pop();
                is_php_variadic = true;
            } else {
                return Err(Error::syntax(format!(
                    "The last parameter of \"{}\" for {} \"{}\" must be an array with default value, eg. \"array $arg = []\".",
                    php.reflection_name(),
                    self.callable.callable_type().as_str(),
                    Lossy(self.callable.name())
                )));
            }
        }
        Ok((parameters, is_php_variadic))
    }
}

/// Twig: `CallableArgumentsExtractor::normalizeName()`: `strtolower(str_replace('_', '', $name))`.
fn normalize_name(name: &[u8]) -> Vec<u8> {
    name.iter()
        .filter(|&&b| b != b'_')
        .map(u8::to_ascii_lowercase)
        .collect()
}

fn set_name(map: &mut Vec<(Vec<u8>, Vec<u8>)>, normalized: Vec<u8>, original: Vec<u8>) {
    match map.iter_mut().find(|(k, _)| *k == normalized) {
        Some(slot) => slot.1 = original,
        None => map.push((normalized, original)),
    }
}

fn find<'e>(extracted: &'e [(Normalized, Expr)], key: &Normalized) -> Option<&'e Expr> {
    extracted.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

/// `array_key_exists($key, $extracted)` then `unset()`: removes and returns the entry.
fn take(extracted: &mut Vec<(Normalized, Expr)>, key: &Normalized) -> Option<Expr> {
    let index = extracted.iter().position(|(k, _)| k == key)?;
    Some(extracted.remove(index).1)
}

/// Twig: `CallableArgumentsExtractor::toSnakeCase()`:
/// `strtolower(preg_replace(['/([A-Z]+)([A-Z][a-z])/', '/([a-z0-9])([A-Z])/'], '\1_\2', $name))`.
pub fn to_snake_case(name: &str) -> String {
    let bytes = name.as_bytes();
    // first pattern: ([A-Z]+)([A-Z][a-z])
    let mut first: Vec<u8> = Vec::with_capacity(bytes.len() + 4);
    let mut i = 0;
    while i < bytes.len() {
        let run_end = bytes[i..]
            .iter()
            .position(|b| !b.is_ascii_uppercase())
            .map_or(bytes.len(), |p| i + p);
        if run_end >= i + 2 && run_end < bytes.len() && bytes[run_end].is_ascii_lowercase() {
            first.extend_from_slice(&bytes[i..run_end - 1]);
            first.push(b'_');
            first.extend_from_slice(&bytes[run_end - 1..=run_end]);
            i = run_end + 1;
        } else {
            first.push(bytes[i]);
            i += 1;
        }
    }
    // second pattern: ([a-z0-9])([A-Z])
    let mut second: Vec<u8> = Vec::with_capacity(first.len() + 4);
    let mut i = 0;
    while i < first.len() {
        let c = first[i];
        if (c.is_ascii_lowercase() || c.is_ascii_digit())
            && first.get(i + 1).is_some_and(u8::is_ascii_uppercase)
        {
            second.push(c);
            second.push(b'_');
            second.push(first[i + 1]);
            i += 2;
        } else {
            second.push(c);
            i += 1;
        }
    }
    String::from_utf8_lossy(&second.to_ascii_lowercase()).into_owned()
}

/// Whether an expression is a `ContextVariable` (including `AssignContextVariable`), for the deprecation of
/// `GetAttrExpression` arguments that are neither arrays nor variables.
pub(crate) fn is_array_or_variable(expr: &Expr) -> bool {
    matches!(expr.kind, ExprKind::Array(_)) || expr.context_variable_name().is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_to_snake_case_like_twig() {
        assert_eq!(to_snake_case("preserveKeys"), "preserve_keys");
        assert_eq!(to_snake_case("firstArg"), "first_arg");
        assert_eq!(to_snake_case("HTMLParser"), "html_parser");
        assert_eq!(to_snake_case("allowable_tags"), "allowable_tags");
        assert_eq!(to_snake_case("someKey"), "some_key");
        assert_eq!(to_snake_case("ABCd"), "ab_cd");
        assert_eq!(to_snake_case("a1B"), "a1_b");
        assert_eq!(to_snake_case("FOO_Bar"), "foo_bar");
        assert_eq!(to_snake_case("aBcD"), "a_bc_d");
    }

    #[test]
    fn normalizes_names() {
        assert_eq!(normalize_name(b"Foo_Bar"), b"foobar");
    }
}
