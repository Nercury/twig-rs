//! `CallableArgumentsExtractor` (TWIG/src/Util/CallableArgumentsExtractor.php) with the core callables: the
//! oracle-verified messages of `reference/twig/core-extension.md` §1.5 and `reference/twig/runtime.md` §4.5.

use std::sync::Arc;

use twig::Source;
use twig::ast::{ArgumentKey, Arguments, ConstantValue, Expr, ExprKind, dump::dump_expr};
use twig::deprecation::Deprecation;
use twig::extension::arguments::{CallableArgumentsExtractor, to_snake_case};
use twig::extension::{DebugExtension, ExtensionSet, TwigCallable};

use super::support::keep_parser_visitors;

fn set() -> ExtensionSet {
    let mut set = ExtensionSet::with_core_extensions();
    set.add_extension(Arc::new(DebugExtension::new())).unwrap();
    keep_parser_visitors(set)
}

fn callable(set: &ExtensionSet, kind: &str, name: &str) -> TwigCallable {
    match kind {
        "filter" => TwigCallable::Filter(set.get_filter(name.as_bytes()).unwrap().unwrap()),
        "function" => TwigCallable::Function(set.get_function(name.as_bytes()).unwrap().unwrap()),
        _ => TwigCallable::Test(set.get_test(name.as_bytes()).unwrap().unwrap()),
    }
}

enum Arg {
    Positional(i64),
    Named(&'static str, i64),
}

fn arguments(args: &[Arg]) -> Arguments {
    let mut arguments = Arguments::new();
    for arg in args {
        match arg {
            Arg::Positional(v) => arguments.push(Expr::constant(*v, 1)),
            Arg::Named(name, v) => {
                arguments.set_named(name.as_bytes().to_vec(), Expr::constant(*v, 1))
            }
        }
    }
    arguments
}

fn extract(kind: &str, name: &str, args: &[Arg]) -> Result<Vec<Expr>, String> {
    let set = set();
    let callable = callable(&set, kind, name);
    let source = Source::new("", "index");
    let extractor =
        CallableArgumentsExtractor::new(&callable, 1, Some(&source), kind != "function")
            .map_err(|e| e.to_string())?;
    let mut deprecations: Vec<Deprecation> = Vec::new();
    extractor
        .extract_arguments(&arguments(args), &mut deprecations)
        .map_err(|e| e.to_string())
}

fn error(kind: &str, name: &str, args: &[Arg]) -> String {
    extract(kind, name, args).expect_err("an error")
}

#[test]
fn reports_missing_arguments() {
    use Arg::*;
    assert_eq!(
        error("filter", "replace", &[]),
        "Value for argument \"from\" is required for filter \"replace\" in \"index\" at line 1."
    );
    assert_eq!(
        error("filter", "slice", &[Named("length", 1)]),
        "Value for argument \"start\" is required for filter \"slice\" in \"index\" at line 1."
    );
    assert_eq!(
        error("function", "range", &[Named("start", 1), Named("end", 3)]),
        "Value for argument \"low\" is required for function \"range\" in \"index\" at line 1."
    );
    assert_eq!(
        error("function", "range", &[Positional(1)]),
        "Value for argument \"end\" is required for function \"range\" in \"index\" at line 1."
    );
    assert_eq!(
        error("function", "max", &[]),
        "Value for argument \"value\" is required for function \"max\" in \"index\" at line 1."
    );
}

#[test]
fn reports_misplaced_and_duplicate_arguments() {
    use Arg::*;
    assert_eq!(
        error("filter", "replace", &[Named("from", 1), Positional(2)]),
        "Positional arguments cannot be used after named arguments for filter \"replace\" in \"index\" at line 1."
    );
    assert_eq!(
        error("filter", "slice", &[Positional(1), Named("start", 2)]),
        "Argument \"start\" is defined twice for filter \"slice\" in \"index\" at line 1."
    );
    assert_eq!(
        error("filter", "format", &[Positional(1), Named("values", 2)]),
        "Argument \"values\" is defined twice for filter \"format\" in \"index\" at line 1."
    );
}

#[test]
fn reports_unknown_arguments_with_normalized_names() {
    use Arg::*;
    assert_eq!(
        error("filter", "upper", &[Named("foo", 1)]),
        "Unknown argument \"foo\" for filter \"upper()\" in \"index\" at line 1."
    );
    assert_eq!(
        error("filter", "upper", &[Named("foo", 1), Named("bar", 2)]),
        "Unknown arguments \"foo\", \"bar\" for filter \"upper()\" in \"index\" at line 1."
    );
    assert_eq!(
        error("filter", "upper", &[Named("Foo_Bar", 1)]),
        "Unknown argument \"foobar\" for filter \"upper()\" in \"index\" at line 1."
    );
    assert_eq!(
        error("filter", "format", &[Named("format", 1)]),
        "Unknown argument \"format\" for filter \"format(values)\" in \"index\" at line 1."
    );
    assert_eq!(
        error("filter", "merge", &[Named("arrays", 1)]),
        "Unknown argument \"arrays\" for filter \"merge()\" in \"index\" at line 1."
    );
    assert_eq!(
        error("filter", "escape", &[Named("strateg", 1)]),
        "Unknown argument \"strateg\" for filter \"escape(strategy, charset, autoescape)\" in \"index\" at line 1."
    );
    assert_eq!(
        error("filter", "join", &[Named("sep", 1)]),
        "Unknown argument \"sep\" for filter \"join(glue, and)\" in \"index\" at line 1."
    );
}

#[test]
fn binds_named_arguments_and_fills_defaults() {
    use Arg::*;
    // json_encode(depth=1): flags gets its default
    let args = extract("filter", "json_encode", &[Named("depth", 1)]).unwrap();
    let values: Vec<_> = args.iter().map(|a| a.as_constant().cloned()).collect();
    assert_eq!(
        values,
        [Some(ConstantValue::Int(0)), Some(ConstantValue::Int(1))]
    );
    // names are normalized: case and underscores are ignored
    let args = extract(
        "filter",
        "slice",
        &[Positional(1), Named("PRESERVE_KEYS", 2)],
    )
    .unwrap();
    let values: Vec<_> = args.iter().map(|a| a.as_constant().cloned()).collect();
    assert_eq!(
        values,
        [
            Some(ConstantValue::Int(1)),
            Some(ConstantValue::Null),
            Some(ConstantValue::Int(2))
        ]
    );
    // positional only: passed through untouched, extra ones included
    let args = extract("filter", "upper", &[Positional(1), Positional(2)]).unwrap();
    assert_eq!(args.len(), 2);
}

#[test]
fn collects_variadic_arguments() {
    let set = set();
    let dump = callable(&set, "function", "dump");
    let source = Source::new("", "index");
    let extractor = CallableArgumentsExtractor::new(&dump, 3, Some(&source), false).unwrap();
    let mut arguments = Arguments::new();
    arguments.push(Expr::constant(1, 3));
    arguments.set_named(b"fooBar".to_vec(), Expr::constant(2, 3));
    let mut deprecations: Vec<Deprecation> = Vec::new();
    let args = extractor
        .extract_arguments(&arguments, &mut deprecations)
        .unwrap();
    assert_eq!(args.len(), 1);
    let ExprKind::Array(bucket) = &args[0].kind else {
        panic!()
    };
    assert!(
        bucket.variadic,
        "a PHP variadic collects into a VariadicExpression"
    );
    assert_eq!(
        bucket.elements[1].0.as_constant(),
        Some(&ConstantValue::string("foo_bar"))
    );
    assert_eq!(
        deprecations
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        [
            "Since twig/twig 3.15: Using \"snake_case\" for variadic arguments is required for a smooth upgrade with Twig 4.0; rename \"fooBar\" to \"foo_bar\" in \"index\" at line 3."
        ]
    );
    let text = dump_expr(&args[0]);
    assert!(
        text.starts_with("Expression\\VariadicExpression @3"),
        "{text}"
    );
}

#[test]
fn rejects_callables_without_php_callable() {
    let set = set();
    let even = callable(&set, "test", "even");
    let error = CallableArgumentsExtractor::new(&even, 1, None, true)
        .err()
        .unwrap();
    assert_eq!(
        error.to_string(),
        "Callback for test \"even\" is not callable in the current scope."
    );
    assert_eq!(error.kind(), twig::ErrorKind::Logic);
}

#[test]
fn converts_names_to_snake_case() {
    assert_eq!(to_snake_case("preserveKeys"), "preserve_keys");
    assert_eq!(to_snake_case("decimalPoint"), "decimal_point");
    assert_eq!(to_snake_case("thousandSep"), "thousand_sep");
    assert_eq!(to_snake_case("characterMask"), "character_mask");
    assert_eq!(to_snake_case("ignoreMissing"), "ignore_missing");
}

#[test]
fn argument_keys_are_positions_or_names() {
    let mut arguments = Arguments::new();
    arguments.set_named(b"a".to_vec(), Expr::constant(1, 1));
    arguments.push(Expr::constant(2, 1));
    arguments.push(Expr::constant(3, 1));
    let keys: Vec<_> = arguments.entries().iter().map(|(k, _)| k.clone()).collect();
    assert_eq!(
        keys,
        [
            ArgumentKey::Name(b"a".to_vec()),
            ArgumentKey::Index(0),
            ArgumentKey::Index(1)
        ]
    );
}
