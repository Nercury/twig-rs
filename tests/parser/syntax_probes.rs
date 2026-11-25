//! The expression probes of `reference/twig/syntax.md` §9 (Appendix A, verified on the oracle) and the examples of
//! §5 that are decided by the parser alone (no tag, no rendering). The tag probes are in `tag_probes.rs`; probes that
//! render or compile come with the later stages, and `matches` patterns need the PCRE port.

use twig::ast::{BinaryOp, ConstantValue, ExprKind, UnaryOp};

use super::support::{first_print_expr, parse, parse_error};

#[test]
fn appendix_a_parse_errors() {
    let cases = [
        (
            "{{ (1+2) ?? 3 }}",
            "The \"defined\" test only works with simple variables in \"index\" at line 1.",
        ),
        (
            "{{ 1__2 }}",
            "Unexpected token \"name\" of value \"__2\" (\"end of print statement\" expected) in \"index\" at line 1 column 5.",
        ),
        (
            "{{ x",
            "Unexpected token \"end of template\" (\"end of print statement\" expected) in \"index\" at line 1 column 5.",
        ),
        (
            "{{ 'a' 'b' }}",
            "Unexpected token \"string\" of value \"b\" (\"end of print statement\" expected) in \"index\" at line 1 column 8.",
        ),
        (
            "{{- -}}",
            "Unexpected token \"end of print statement\" of value \"\" in \"index\" at line 1.",
        ),
        (
            "{% line 5 %}{{ x }}\n{{ y",
            "Unexpected token \"end of template\" (\"end of print statement\" expected) in \"index\" at line 6 column 5.",
        ),
        (
            "{{ true ?.5 : 1 }}",
            "Unexpected token \"punctuation\" of value \":\" (\"end of print statement\" expected) in \"index\" at line 1 column 13.",
        ),
        (
            "{{ 1...5 }}",
            "Unexpected token \"operator\" of value \"...\" (\"end of print statement\" expected) in \"index\" at line 1 column 5.",
        ),
        (
            "{{ 1. }}",
            "Expected name or number, got value \"\" of type \"end of print statement\" in \"index\" at line 1.",
        ),
        (
            "{{ 1 0 }}",
            "Unexpected token \"number\" (\"end of print statement\" expected) in \"index\" at line 1 column 6.",
        ),
        (
            "{{ 1 1.5 }}",
            "Unexpected token \"number\" of value \"1.5\" (\"end of print statement\" expected) in \"index\" at line 1 column 6.",
        ),
        (
            "{{ x is odd foo }}",
            "Unknown \"odd foo\" test in \"index\" at line 1.",
        ),
        (
            "{{ max(1,2)(3) }}",
            "Function name must be an identifier in \"index\" at line 1.",
        ),
        (
            "{{ {\"a\"} }}",
            "A mapping key must be followed by a colon (:). Unexpected token \"punctuation\" of value \"}\" (\"punctuation\" expected with value \":\") in \"index\" at line 1 column 8.",
        ),
        (
            "{{ [1, 2 3] }}",
            "A sequence element must be followed by a comma. Unexpected token \"number\" of value \"3\" (\"punctuation\" expected with value \",\") in \"index\" at line 1 column 10.",
        ),
        (
            "{{ ...a }}",
            "The spread operator can only be used on sequence elements, mapping elements, and call arguments in \"index\" at line 1.",
        ),
        (
            "{{ max(...[1, 5], 2) }}",
            "Normal arguments must be placed before argument unpacking in \"index\" at line 1.",
        ),
        (
            "{{ [1,2]|map((a, b) + 1)|join }}",
            "A list of variables must be followed by an arrow in \"index\" at line 1.",
        ),
        (
            "{{ [1,2]|map(1 => 2)|join }}",
            "The arrow function argument must be a list of variables or a single variable in \"index\" at line 1.",
        ),
        (
            "{{ x|join(a.b: 1) }}",
            "A parameter name must be a string, \"Twig\\Node\\Expression\\GetAttrExpression\" given in \"index\" at line 1.",
        ),
        (
            "{{ parent() }}",
            "Calling the \"parent\" function outside of a block is forbidden in \"index\" at line 1.",
        ),
        // §5.3
        (
            "{{ }}",
            "Unexpected token \"end of print statement\" of value \"\" in \"index\" at line 1.",
        ),
        (
            "{{ .5 }}",
            "Unexpected token \"operator\" of value \".\" in \"index\" at line 1.",
        ),
        (
            "{{ (() => 1) }}",
            "Unexpected token \"punctuation\" of value \")\" in \"index\" at line 1.",
        ),
        (
            "{{ not }}",
            "Unexpected token \"end of print statement\" of value \"\" in \"index\" at line 1.",
        ),
        (
            "{{ {[1]: 2} }}",
            "A mapping key must be a quoted string, a number, a name, or an expression enclosed in parentheses (unexpected token \"operator\" of value \"[\" in \"index\" at line 1.",
        ),
        (
            "{{ \"#{}\" }}",
            "Unexpected token \"end of string interpolation\" of value \"\" in \"index\" at line 1.",
        ),
        (
            "{{ \"#{ 1 2 }\" }}",
            "Unexpected token \"number\" of value \"2\" (\"end of string interpolation\" expected) in \"index\" at line 1 column 10.",
        ),
        (
            "{{ (a b) }}",
            "An opened parenthesis is not properly closed. Unexpected token \"name\" of value \"b\" (\"punctuation\" expected with value \")\") in \"index\" at line 1 column 7.",
        ),
        (
            "{{ (1, 2) }}",
            "Unexpected token \"number\" of value \"2\" (\"name\" expected) in \"index\" at line 1 column 8.",
        ),
        // §5.4
        (
            "{{ x| }}",
            "Unexpected token \"end of print statement\" (\"name\" expected) in \"index\" at line 1 column 7.",
        ),
        (
            "{{ x|'a' }}",
            "Unexpected token \"string\" of value \"a\" (\"name\" expected) in \"index\" at line 1 column 6.",
        ),
        (
            "{{ x.'y' }}",
            "Expected name or number, got value \"y\" of type \"string\" in \"index\" at line 1.",
        ),
        (
            "{{ x?.[1] }}",
            "Expected name or number, got value \"[\" of type \"operator\" in \"index\" at line 1.",
        ),
        (
            "{{ x.() }}",
            "Unexpected token \"punctuation\" of value \")\" in \"index\" at line 1.",
        ),
        (
            "{{ x[1:2:3] }}",
            "Unexpected token \"punctuation\" of value \":\" (\"punctuation\" expected with value \"]\") in \"index\" at line 1 column 9.",
        ),
        (
            "{{ x[] }}",
            "Unexpected token \"punctuation\" of value \"]\" in \"index\" at line 1.",
        ),
        (
            "{{ \"foo\"() }}",
            "Function name must be an identifier in \"index\" at line 1.",
        ),
        (
            "{{ in(1) }}",
            "Unknown \"in\" function. Did you mean \"min\", \"include\", \"include_only\" in \"index\" at line 1?",
        ),
        (
            "{{ 1 is divisible by }}",
            "Unexpected token \"end of print statement\" of value \"\" in \"index\" at line 1.",
        ),
        (
            "{{ 1 is same x }}",
            "Unknown \"same x\" test. Did you mean \"same as\" in \"index\" at line 1?",
        ),
        (
            "{{ 1 is divisible }}",
            "Unknown \"divisible\" test. Did you mean \"divisible by\" in \"index\" at line 1?",
        ),
        (
            "{{ 1 is nope(1 2) }}",
            "Unknown \"nope\" test. Did you mean \"none\" in \"index\" at line 1?",
        ),
        (
            "{{ 1|nope(1 2) }}",
            "Arguments must be separated by a comma. Unexpected token \"number\" of value \"2\" (\"punctuation\" expected with value \",\") in \"index\" at line 1 column 13.",
        ),
        (
            "{{ x|upper ?? 'd' }}",
            "The \"defined\" test only works with simple variables in \"index\" at line 1.",
        ),
        (
            "{{ 1 = 2 }}",
            "Cannot assign to \"Twig\\Node\\Expression\\ConstantExpression\", only variables can be assigned in \"index\" at line 1.",
        ),
        (
            "{{ a + b = 1 }}",
            "Cannot assign to \"Twig\\Node\\Expression\\Binary\\AddBinary\", only variables can be assigned in \"index\" at line 1.",
        ),
        (
            "{{ f(\"a\": 2) }}",
            "A parameter name must be a string, \"Twig\\Node\\Expression\\ConstantExpression\" given in \"index\" at line 1.",
        ),
        (
            "{{ (a + 1) => 2 }}",
            "A list must only contain variables in \"index\" at line 1.",
        ),
        (
            "{{ (a, b c) => 1 }}",
            "Unexpected token \"name\" of value \"c\" (\"punctuation\" expected with value \",\") in \"index\" at line 1 column 10.",
        ),
    ];
    for (template, expected) in cases {
        assert_eq!(parse_error(template), expected, "{template:?}");
    }
}

#[test]
fn appendix_a_trees() {
    // `(max)(1, 2)` calls the function max
    let (module, _) = parse("{{ (max)(1, 2) }}");
    assert!(matches!(
        first_print_expr(&module.unwrap()).kind,
        ExprKind::Function(_)
    ));

    // `2 is same as 2 + 1` is `(2 is same as 2) + 1`
    let (module, _) = parse("{{ 2 is same as 2 + 1 }}");
    let module = module.unwrap();
    let ExprKind::Binary(add) = &first_print_expr(&module).kind else {
        panic!()
    };
    assert_eq!(add.op, BinaryOp::Add);
    assert!(matches!(add.left.kind, ExprKind::Test(_)));

    // `2 ** 3 ** 2` is right associative, `-1**0` is `(-1) ** 0`
    let (module, _) = parse("{{ 2 ** 3 ** 2 }}");
    let module = module.unwrap();
    let ExprKind::Binary(power) = &first_print_expr(&module).kind else {
        panic!()
    };
    assert!(matches!(power.right.kind, ExprKind::Binary(ref b) if b.op == BinaryOp::Power));
    let (module, _) = parse("{{ -1**0 }}");
    let module = module.unwrap();
    let ExprKind::Binary(power) = &first_print_expr(&module).kind else {
        panic!()
    };
    assert!(matches!(power.left.kind, ExprKind::Unary(ref u) if u.op == UnaryOp::Neg));

    // `a ? b : c ? d : e` is `a ? b : (c ? d : e)`
    let (module, _) = parse("{{ true ? 'a' : false ? 'b' : 'c' }}");
    let module = module.unwrap();
    let ExprKind::ConditionalTernary(ternary) = &first_print_expr(&module).kind else {
        panic!()
    };
    assert!(matches!(
        ternary.right.kind,
        ExprKind::ConditionalTernary(_)
    ));

    // `{a, b: 2, 'c': 3, 4: 5, (1+1): 6, ...{z: 1}}`
    let (module, _) = parse("{{ {a, b: 2, 'c': 3, 4: 5, (1+1): 6, ...{z: 1}} }}");
    let module = module.unwrap();
    let array = first_print_expr(&module).as_array().unwrap();
    let keys: Vec<Option<&ConstantValue>> = array
        .elements
        .iter()
        .map(|(k, _)| k.as_constant())
        .collect();
    assert_eq!(
        keys[..4],
        [
            Some(&ConstantValue::string("a")),
            Some(&ConstantValue::string("b")),
            Some(&ConstantValue::string("c")),
            Some(&ConstantValue::Int(4)),
        ]
    );
    assert!(matches!(array.elements[4].0.kind, ExprKind::Binary(_)));
    assert_eq!(
        keys[5],
        Some(&ConstantValue::Int(0)),
        "the spread gets the first automatic key"
    );

    // `TRUE`, `FALSE`, `NULL`, `NONE` are constants, `True` is a variable
    let (module, _) = parse("{{ True }}");
    assert!(matches!(
        first_print_expr(&module.unwrap()).kind,
        ExprKind::ContextVariable(_)
    ));

    // operator words are variable names where no operator can follow
    for name in ["literal", "in", "is", "and"] {
        let (module, _) = parse(&format!("{{{{ {name} }}}}"));
        let module = module.unwrap();
        assert_eq!(
            first_print_expr(&module).context_variable_name(),
            Some(name.as_bytes())
        );
    }
}
