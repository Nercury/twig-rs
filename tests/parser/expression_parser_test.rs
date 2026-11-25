//! Port of Twig 3.31.1's `tests/ExpressionParserTest.php` (TWIG/tests/ExpressionParserTest.php).
//!
//! Cases that go through tags (`set`, `do`, `with`, `macro`) or through compilation and rendering are ported with
//! the statement parser and the runtime; their expression parts are covered by the oracle vectors
//! (`tests/vectors/parser/expressions.json`). Tests about PHP node classes "ready" for first-class callables have
//! no counterpart: Rust node classes always receive the callable.

use std::sync::Arc;

use twig::ast::{
    ArgumentKey, Arguments, BinaryOp, ConstantValue, Expr, ExprKind, UnaryOp, dump::dump_expr,
};
use twig::extension::{
    CallableParameter, Extension, ExtensionSet, FilterNodeClass, FilterNodeFactory, FilterOptions,
    FunctionNodeClass, FunctionNodeFactory, FunctionOptions, PhpCallable, TestNodeClass,
    TestNodeFactory, TestOptions, TwigFilter, TwigFunction, TwigTest,
};
use twig::parser::expression::ExpressionParserEntry;
use twig::parser::expression::prefix::{LiteralExpressionParser, UnaryOperatorExpressionParser};
use twig::{Result, Source};

use super::support::{first_print_expr, parse, parse_error, parse_with};

fn constant(value: impl Into<ConstantValue>) -> Expr {
    Expr::constant(value, 1)
}

/// `new ArrayExpression([k1, v1, k2, v2, ...], 1)`.
fn array(elements: Vec<Expr>) -> Expr {
    let mut pairs = Vec::new();
    let mut iter = elements.into_iter();
    while let (Some(key), Some(value)) = (iter.next(), iter.next()) {
        pairs.push((key, value));
    }
    Expr::array_from_pairs(pairs, 1)
}

fn var(name: &str) -> Expr {
    Expr::context_variable(name, 1)
}

fn concat(left: Expr, right: Expr) -> Expr {
    Expr::binary(BinaryOp::Concat, left, right, 1).unwrap()
}

fn assert_expr(template: &str, expected: &Expr) {
    let (module, _) = parse(template);
    let module = module.unwrap_or_else(|e| panic!("{template:?}: {e}"));
    let actual = first_print_expr(&module);
    assert_eq!(dump_expr(actual), dump_expr(expected), "{template:?}");
    assert_eq!(actual, expected, "{template:?}");
}

/// Twig: `testSequenceExpression()` with `getTestsForSequence()`.
#[test]
fn sequence_expression() {
    let cases: Vec<(&str, Expr)> = vec![
        // simple sequence
        (
            "{{ [1, 2] }}",
            array(vec![constant(0), constant(1), constant(1), constant(2)]),
        ),
        // sequence with trailing ,
        (
            "{{ [1, 2, ] }}",
            array(vec![constant(0), constant(1), constant(1), constant(2)]),
        ),
        // simple mapping
        (
            r#"{{ {"a": "b", "b": "c"} }}"#,
            array(vec![
                constant("a"),
                constant("b"),
                constant("b"),
                constant("c"),
            ]),
        ),
        // mapping with trailing ,
        (
            r#"{{ {"a": "b", "b": "c", } }}"#,
            array(vec![
                constant("a"),
                constant("b"),
                constant("b"),
                constant("c"),
            ]),
        ),
        // mapping in a sequence
        (
            r#"{{ [1, {"a": "b", "b": "c"}] }}"#,
            array(vec![
                constant(0),
                constant(1),
                constant(1),
                array(vec![
                    constant("a"),
                    constant("b"),
                    constant("b"),
                    constant("c"),
                ]),
            ]),
        ),
        // sequence in a mapping
        (
            r#"{{ {"a": [1, 2], "b": "c"} }}"#,
            array(vec![
                constant("a"),
                array(vec![constant(0), constant(1), constant(1), constant(2)]),
                constant("b"),
                constant("c"),
            ]),
        ),
        (
            "{{ {a, b} }}",
            array(vec![constant("a"), var("a"), constant("b"), var("b")]),
        ),
        // sequence with spread operator
        (
            "{{ [1, 2, ...foo] }}",
            array(vec![
                constant(0),
                constant(1),
                constant(1),
                constant(2),
                constant(2),
                Expr::unary(UnaryOp::Spread, var("foo"), 1),
            ]),
        ),
        // mapping with spread operator
        (
            r#"{{ {"a": "b", "b": "c", ...otherLetters} }}"#,
            array(vec![
                constant("a"),
                constant("b"),
                constant("b"),
                constant("c"),
                constant(0),
                Expr::unary(UnaryOp::Spread, var("otherLetters"), 1),
            ]),
        ),
    ];
    for (template, expected) in &cases {
        assert_expr(template, expected);
    }
}

/// Twig: `testSequenceSyntaxError()` with `getFailingTestsForSequence()`.
#[test]
fn sequence_syntax_error() {
    for template in [
        r#"{{ [1, "a": "b"] }}"#,
        r#"{{ {"a": "b", 2} }}"#,
        r#"{{ {"a"} }}"#,
    ] {
        let (result, _) = parse(template);
        let error = result.expect_err(template);
        assert_eq!(error.kind(), twig::ErrorKind::Syntax, "{template:?}");
    }
}

/// Twig: `testMisplacedSpreadIsASyntaxError()` with the cases of `getMisplacedSpreadTests()` that need no tag.
#[test]
fn misplaced_spread_is_a_syntax_error() {
    for template in [
        "{{ ...foo }}",
        "{{ (...foo) }}",
        "{{ -...foo }}",
        "{{ 1 + ...foo }}",
        "{{ (bar => ...foo)(1) }}",
        "{{ [(...foo)] }}",
        "{{ [1, -...foo] }}",
        "{{ {a: ...foo} }}",
        "{{ {(...foo): 1} }}",
        "{{ bar(-...foo) }}",
        "{{ bar(a: ...foo) }}",
        "{{ bar(a = ...foo) }}",
    ] {
        assert_eq!(
            parse_error(template),
            "The spread operator can only be used on sequence elements, mapping elements, and call arguments in \"index\" at line 1.",
            "{template:?}"
        );
    }
}

/// Twig: `testStringExpressionDoesNotConcatenateTwoConsecutiveStrings()`.
#[test]
fn string_expression_does_not_concatenate_two_consecutive_strings() {
    assert_eq!(
        parse_error(r#"{{ "a" "b" }}"#),
        "Unexpected token \"string\" of value \"b\" (\"end of print statement\" expected) in \"index\" at line 1 column 8."
    );
}

/// Twig: `testSequenceDestructuringUsesAssignmentTargets()`.
#[test]
fn sequence_destructuring_uses_assignment_targets() {
    let (module, _) = parse("{{ ([first, , third] = values) }}");
    let module = module.unwrap();
    let node = first_print_expr(&module);
    let ExprKind::Binary(binary) = &node.kind else {
        panic!("{node:?}")
    };
    assert_eq!(binary.op, BinaryOp::SequenceDestructuringSet);
    let pairs = &binary.left.as_array().unwrap().elements;
    assert!(
        matches!(pairs[0].1.kind, ExprKind::AssignContextVariable(ref n) if n.name == b"first")
    );
    assert!(matches!(pairs[1].1.kind, ExprKind::Empty));
    assert!(
        matches!(pairs[2].1.kind, ExprKind::AssignContextVariable(ref n) if n.name == b"third")
    );
}

/// Twig: `testObjectDestructuringUsesAssignmentTargets()`.
#[test]
fn object_destructuring_uses_assignment_targets() {
    let (module, _) = parse("{{ ({name: user_name} = user) }}");
    let module = module.unwrap();
    let node = first_print_expr(&module);
    let ExprKind::Binary(binary) = &node.kind else {
        panic!("{node:?}")
    };
    assert_eq!(binary.op, BinaryOp::ObjectDestructuringSet);
    let (key, value) = &binary.left.as_array().unwrap().elements[0];
    assert_eq!(key.as_constant(), Some(&ConstantValue::string("name")));
    assert!(matches!(value.kind, ExprKind::AssignContextVariable(ref n) if n.name == b"user_name"));
}

/// Twig: `testStringExpression()` with `getTestsForString()`.
#[test]
fn string_expression() {
    assert_expr(
        r#"{{ "foo #{bar}" }}"#,
        &concat(constant("foo "), var("bar")),
    );
    assert_expr(
        r#"{{ "foo #{bar} baz" }}"#,
        &concat(concat(constant("foo "), var("bar")), constant(" baz")),
    );
    assert_expr(
        r#"{{ "foo #{"foo #{bar} baz"} baz" }}"#,
        &concat(
            concat(
                constant("foo "),
                concat(concat(constant("foo "), var("bar")), constant(" baz")),
            ),
            constant(" baz"),
        ),
    );
}

/// Twig: `testUnknownFunction()` and `testUnknownFunctionWithoutSuggestions()`.
#[test]
fn unknown_function() {
    assert_eq!(
        parse_error("{{ cycl() }}"),
        "Unknown \"cycl\" function. Did you mean \"cycle\" in \"index\" at line 1?"
    );
    assert_eq!(
        parse_error("{{ foobar() }}"),
        "Unknown \"foobar\" function in \"index\" at line 1."
    );
}

/// Twig: `testUnknownFilter()` and `testUnknownFilterWithoutSuggestions()`.
#[test]
fn unknown_filter() {
    assert_eq!(
        parse_error("{{ 1|lowe }}"),
        "Unknown \"lowe\" filter. Did you mean \"lower\" in \"index\" at line 1?"
    );
    assert_eq!(
        parse_error("{{ 1|foobar }}"),
        "Unknown \"foobar\" filter in \"index\" at line 1."
    );
}

/// Twig: `testUnknownTest()` and `testUnknownTestWithoutSuggestions()`.
#[test]
fn unknown_test() {
    assert_eq!(
        parse_error("{{ 1 is nul }}"),
        "Unknown \"nul\" test. Did you mean \"null\" in \"index\" at line 1?"
    );
    assert_eq!(
        parse_error("{{ 1 is foobar }}"),
        "Unknown \"foobar\" test in \"index\" at line 1."
    );
}

struct DynamicExtension;

impl Extension for DynamicExtension {
    fn class_name(&self) -> &str {
        "DynamicExtension"
    }

    fn filters(&self) -> Vec<TwigFilter> {
        vec![TwigFilter::new(
            "*_foo_*_bar",
            Some(three_arguments()),
            FilterOptions::default(),
        )]
    }

    fn functions(&self) -> Vec<TwigFunction> {
        vec![TwigFunction::new(
            "*_foo_*_bar",
            Some(three_arguments()),
            FunctionOptions::default(),
        )]
    }

    fn tests(&self) -> Vec<TwigTest> {
        vec![TwigTest::new(
            "*_foo_*_bar",
            Some(three_arguments()),
            TestOptions::default(),
        )]
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// `static function ($foo, $bar, $a) {}`.
fn three_arguments() -> PhpCallable {
    PhpCallable::closure(vec![
        CallableParameter::required("foo"),
        CallableParameter::required("bar"),
        CallableParameter::required("a"),
    ])
}

fn dynamic_set() -> ExtensionSet {
    let mut set = ExtensionSet::with_core_extensions();
    set.add_extension(Arc::new(DynamicExtension)).unwrap();
    set
}

/// Twig: `testCompiledCodeForDynamicTest()`, `testCompiledCodeForDynamicFunction()` and
/// `testCompiledCodeForDynamicFilter()`: the compiled call passes `"foo", "bar"` before the argument. The parser's
/// part is the dynamic callable: the name used in the template, the registered pattern and the captured parts.
#[test]
fn dynamic_callables() {
    let set = dynamic_set();
    for template in [
        r#"{{ "a" is foo_foo_bar_bar }}"#,
        r#"{{ foo_foo_bar_bar("a") }}"#,
        r#"{{ "a"|foo_foo_bar_bar }}"#,
    ] {
        let (module, _) = parse_with(&set, &Source::new(template, "index"));
        let module = module.unwrap();
        let expr = first_print_expr(&module);
        let (name, dynamic_name, arguments) = match &expr.kind {
            ExprKind::Test(t) => (t.test.name(), t.test.dynamic_name(), t.test.arguments()),
            ExprKind::Function(f) => (
                f.function.name(),
                f.function.dynamic_name(),
                f.function.arguments(),
            ),
            ExprKind::Filter(f) => (
                f.filter.name(),
                f.filter.dynamic_name(),
                f.filter.arguments(),
            ),
            _ => panic!("{expr:?}"),
        };
        assert_eq!(name, b"foo_foo_bar_bar", "{template}");
        assert_eq!(dynamic_name, b"*_foo_*_bar", "{template}");
        assert_eq!(arguments, [b"foo".to_vec(), b"bar".to_vec()], "{template}");
    }
}

struct Custom {
    class: &'static str,
}

impl FilterNodeFactory for Custom {
    fn class_name(&self) -> &str {
        self.class
    }

    fn create(
        &self,
        node: Expr,
        filter: TwigFilter,
        arguments: Arguments,
        line: i64,
    ) -> Result<Expr> {
        Ok(Expr::filter_node(
            node,
            filter,
            arguments,
            line,
            twig::ast::FilterKind::Filter,
        ))
    }
}

impl FunctionNodeFactory for Custom {
    fn class_name(&self) -> &str {
        self.class
    }

    fn create(&self, function: TwigFunction, arguments: Arguments, line: i64) -> Result<Expr> {
        let mut node = Expr::function(
            TwigFunction::new(
                function.name(),
                function.callable().cloned(),
                FunctionOptions::default(),
            ),
            arguments,
            line,
        )?;
        node.documentation = Some(b"custom".to_vec());
        Ok(node)
    }
}

impl TestNodeFactory for Custom {
    fn class_name(&self) -> &str {
        self.class
    }

    fn create(
        &self,
        node: Expr,
        test: TwigTest,
        arguments: Option<Arguments>,
        line: i64,
    ) -> Result<Expr> {
        Expr::test(node, test, arguments, line, twig::ast::TestKind::Null)
    }
}

/// Twig: `testReadyFunction()`, `testReadyFilter()`, `testReadyTest()` (and the "not ready" variants): a custom
/// node class receives the callable, the arguments and the line.
#[test]
fn custom_node_classes() {
    let mut set = ExtensionSet::with_core_extensions();
    let callable = Some(PhpCallable::function("foo", Vec::new()));
    set.add_function(TwigFunction::new(
        "foo",
        callable.clone(),
        FunctionOptions {
            node_class: FunctionNodeClass::Custom(Arc::new(Custom {
                class: "ReadyFunctionExpression",
            })),
            ..Default::default()
        },
    ))
    .unwrap();
    set.add_filter(TwigFilter::new(
        "foo",
        callable.clone(),
        FilterOptions {
            node_class: FilterNodeClass::Custom(Arc::new(Custom {
                class: "ReadyFilterExpression",
            })),
            ..Default::default()
        },
    ))
    .unwrap();
    set.add_test(TwigTest::new(
        "foo",
        callable,
        TestOptions {
            node_class: TestNodeClass::Custom(Arc::new(Custom {
                class: "ReadyTestExpression",
            })),
            ..Default::default()
        },
    ))
    .unwrap();
    for (template, check) in [
        ("{{ foo() }}", "custom"),
        ("{{ 1|foo }}", "FilterExpression"),
        ("{{ 1 is foo }}", "NullTest"),
    ] {
        let (module, _) = parse_with(&set, &Source::new(template, "index"));
        let module = module.unwrap();
        let dump = dump_expr(first_print_expr(&module));
        assert!(dump.contains(check), "{template}: {dump}");
    }
}

/// Twig: `testTwoWordTestPrecedence()`: a "empty element" test must have precedence over "empty".
#[test]
fn two_word_test_precedence() {
    let mut set = ExtensionSet::with_core_extensions();
    set.add_test(TwigTest::new(
        "empty element",
        Some(PhpCallable::function("foo", Vec::new())),
        TestOptions::default(),
    ))
    .unwrap();
    let (module, _) = parse_with(&set, &Source::new("{{ 1 is empty element }}", "index"));
    let module = module.unwrap();
    let ExprKind::Test(test) = &first_print_expr(&module).kind else {
        panic!()
    };
    assert_eq!(test.test.name(), b"empty element");
}

struct BangExtension;

impl Extension for BangExtension {
    fn class_name(&self) -> &str {
        "BangExtension"
    }

    fn expression_parsers(&self) -> Vec<ExpressionParserEntry> {
        vec![ExpressionParserEntry::prefix(
            UnaryOperatorExpressionParser::core(UnaryOp::Not, "!", 50),
        )]
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Twig: `testUnaryPrecedenceChange()`.
#[test]
fn unary_precedence_change() {
    let mut set = ExtensionSet::with_core_extensions();
    set.add_extension(Arc::new(BangExtension)).unwrap();
    let (module, _) = parse_with(&set, &Source::new(r#"{{ !false ? "OK" : "KO" }}"#, "index"));
    let module = module.unwrap();
    assert!(matches!(
        first_print_expr(&module).kind,
        ExprKind::ConditionalTernary(_)
    ));
}

/// Twig: `testBindingPower()`: the expression parses like its explicitly parenthesized form (compared on the tree,
/// ignoring the parentheses markers).
#[test]
fn binding_power() {
    fn strip(dump: String) -> String {
        dump.lines()
            .filter(|line| !line.trim_start().starts_with(".with_parentheses = true"))
            .collect::<Vec<_>>()
            .join("\n")
    }
    let mut cases: Vec<(String, String)> = Vec::new();
    // * / // % stronger than + -
    for op1 in ["*", "/", "//", "%"] {
        for op2 in ["+", "-"] {
            cases.push((
                format!("{{{{ 12 {op1} 6 {op2} 3 }}}}"),
                format!("{{{{ (12 {op1} 6) {op2} 3 }}}}"),
            ));
            cases.push((
                format!("{{{{ 12 {op2} 6 {op1} 3 }}}}"),
                format!("{{{{ 12 {op2} (6 {op1} 3) }}}}"),
            ));
        }
    }
    // + - * / // % stronger than == != <=> < > >= <=
    for op1 in ["+", "-", "*", "/", "//", "%"] {
        for op2 in ["==", "!=", "<=>", "<", ">", ">=", "<="] {
            cases.push((
                format!("{{{{ 12 {op1} 6 {op2} 3 }}}}"),
                format!("{{{{ (12 {op1} 6) {op2} 3 }}}}"),
            ));
        }
    }
    let pairs = [
        ("{{ 1 + 2 not in [3, 4] }}", "{{ (1 + 2) not in [3, 4] }}"),
        ("{{ 1 + 2 in [3, 4] }}", "{{ (1 + 2) in [3, 4] }}"),
        (
            r#"{{ 1 + 2 matches "/^3$/" }}"#,
            r#"{{ (1 + 2) matches "/^3$/" }}"#,
        ),
        // ~ stronger than `starts with` `ends with`
        (
            r#"{{ "a" ~ "b" starts with "a" }}"#,
            r#"{{ ("a" ~ "b") starts with "a" }}"#,
        ),
        (
            r#"{{ "a" ~ "b" ends with "b" }}"#,
            r#"{{ ("a" ~ "b") ends with "b" }}"#,
        ),
        // [] . stronger than anything else
        (r#"{{ -a["b"] + 3 }}"#, r#"{{ -(a["b"]) + 3 }}"#),
        (r#"{{ -a["c"]["d"] }}"#, r#"{{ -((a["c"])["d"]) }}"#),
        ("{{ -a.b }}", "{{ -(a.b) }}"),
        ("{{ -a.c.d }}", "{{ -((a.c).d) }}"),
        (r#"{{ -a.c["d"] }}"#, r#"{{ -((a.c)["d"]) }}"#),
        (r#"{{ -a["c"].d }}"#, r#"{{ -((a["c"]).d) }}"#),
        // () stronger than anything else
        ("{{ -random(1, 1) + 3 }}", "{{ -(random(1, 1)) + 3 }}"),
        // + - stronger than |
        ("{{ 10 + 2|length }}", "{{ 10 + (2|length) }}"),
        // = stronger than anything else (inside `do` in Twig's test)
        (r#"{{ c = "a" }}"#, r#"{{ c = ("a") }}"#),
        ("{{ c = a.b }}", "{{ c = (a.b) }}"),
        ("{{ a = 1 + 3 }}", "{{ a = (1 + 3) }}"),
        ("{{ a = false or true }}", "{{ a = (false or true) }}"),
        ("{{ c = 4 ? 0 : -1 }}", "{{ c = (4 ? 0 : -1) }}"),
    ];
    cases.extend(
        pairs
            .iter()
            .map(|(a, b)| ((*a).to_owned(), (*b).to_owned())),
    );
    for (expression, expected) in cases {
        let (a, _) = parse(&expression);
        let (b, _) = parse(&expected);
        let a = a.unwrap_or_else(|e| panic!("{expression}: {e}"));
        let b = b.unwrap_or_else(|e| panic!("{expected}: {e}"));
        assert_eq!(
            strip(dump_expr(first_print_expr(&a))),
            strip(dump_expr(first_print_expr(&b))),
            "{expression} vs {expected}"
        );
    }
}

/// Twig: `testLiteralExpressionParserGetOperatorTokensReturnsEmptyArray()`.
#[test]
fn literal_expression_parser_has_no_operator_token() {
    let set = ExtensionSet::with_core_extensions();
    let (_, entry) = set
        .expression_parsers()
        .by_class::<LiteralExpressionParser>()
        .expect("the literal parser is registered");
    assert!(entry.parser().operator_tokens().is_empty());
    assert_eq!(entry.parser().name(), "literal");
}

/// Twig: `testExpressionParserGetOperatorTokensDefaultBehavior()`.
#[test]
fn operator_tokens_are_name_and_aliases() {
    let set = ExtensionSet::with_core_extensions();
    for (_, entry) in set.expression_parsers().iter() {
        let parser = entry.parser();
        if parser.name() == "literal" {
            continue;
        }
        let mut expected = vec![parser.name().to_owned()];
        expected.extend(parser.aliases().iter().cloned());
        assert_eq!(expected, parser.operator_tokens(), "{}", entry.display());
    }
}

/// Twig: `testLiteralIsNotRegisteredAsOperator()`.
#[test]
fn literal_is_not_registered_as_operator() {
    let set = ExtensionSet::with_core_extensions();
    assert!(set.expression_parsers().prefix(b"literal").is_none());
    assert!(set.expression_parsers().infix(b"literal").is_none());
}

/// Named arguments keep the first position of a repeated name (`$args[$name] = $value`).
#[test]
fn repeated_named_arguments_keep_their_position() {
    let (module, _) = parse("{{ max(a = 1, b = 2, a = 3, 4) }}");
    let module = module.unwrap();
    let ExprKind::Function(function) = &first_print_expr(&module).kind else {
        panic!()
    };
    let keys: Vec<&ArgumentKey> = function
        .arguments
        .entries()
        .iter()
        .map(|(k, _)| k)
        .collect();
    assert_eq!(
        keys,
        [
            &ArgumentKey::Name(b"a".to_vec()),
            &ArgumentKey::Name(b"b".to_vec()),
            &ArgumentKey::Index(0)
        ]
    );
    assert_eq!(
        function
            .arguments
            .get(&ArgumentKey::Name(b"a".to_vec()))
            .and_then(Expr::as_constant),
        Some(&ConstantValue::Int(3))
    );
}
