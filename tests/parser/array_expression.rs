extern crate twig;

use super::support;
use twig::nodes::expr::Expr;

#[test]
fn test_array_expression() {
    for (template, expected) in get_tests_for_array() {
        let module = support::expect_parsed(template);
        assert_eq!(module.body.expect_print(), &expected);
    }
}

fn get_tests_for_array<'r>() -> Vec<(&'static str, Expr<'r>)> {
    vec![
        // simple array
        (r#"{{ [1, 2] }}"#, Expr::new_array(vec![
            Expr::new_int_constant(1, 1),
            Expr::new_int_constant(2, 1),
        ], 1)),
        // array with trailing ,
        (r#"{{ [1, 2, ] }}"#, Expr::new_array(vec![
            Expr::new_int_constant(1, 1),
            Expr::new_int_constant(2, 1),
        ], 1)),
        // simple hash
        (r#"{{ {"a": "b", "b": "c"} }}"#, Expr::new_hash(vec![
            (
                Expr::new_str_constant("a", 1),
                Expr::new_str_constant("b", 1),
            ),
            (
                Expr::new_str_constant("b", 1),
                Expr::new_str_constant("c", 1),
            ),
        ], 1)),
        // hash with trailing ,
        (r#"{{ {"a": "b", "b": "c", } }}"#, Expr::new_hash(vec![
            (
                Expr::new_str_constant("a", 1),
                Expr::new_str_constant("b", 1),
            ),
            (
                Expr::new_str_constant("b", 1),
                Expr::new_str_constant("c", 1),
            ),
        ], 1)),
        // hash with unquoted keys
        (r#"{{ {a: "b", b: "c" } }}"#, Expr::new_hash(vec![
            (
                Expr::new_str_constant("a", 1),
                Expr::new_str_constant("b", 1),
            ),
            (
                Expr::new_str_constant("b", 1),
                Expr::new_str_constant("c", 1),
            ),
        ], 1)),
        // hash with number keys
        (r#"{{ {2: "b", 3: "c" } }}"#, Expr::new_hash(vec![
            (
                Expr::new_int_constant(2, 1),
                Expr::new_str_constant("b", 1),
            ),
            (
                Expr::new_int_constant(3, 1),
                Expr::new_str_constant("c", 1),
            ),
        ], 1)),
        // hash in an array
        (r#"{{ [1, {"a": "b", "b": "c"}] }}"#, Expr::new_array(vec![
            Expr::new_int_constant(1, 1),
            Expr::new_hash(vec![
                (
                    Expr::new_str_constant("a", 1),
                    Expr::new_str_constant("b", 1),
                ),
                (
                    Expr::new_str_constant("b", 1),
                    Expr::new_str_constant("c", 1),
                ),
            ], 1),
        ], 1)),
        // array in a hash
        (r#"{{ {"a": [1, 2], "b": "c"} }}"#, Expr::new_hash(vec![
            (
                Expr::new_str_constant("a", 1),
                Expr::new_array(vec![
                    Expr::new_int_constant(1, 1),
                    Expr::new_int_constant(2, 1),
                ], 1),
            ),
            (
                Expr::new_str_constant("b", 1),
                Expr::new_str_constant("c", 1),
            ),
        ], 1)),
    ]
}

#[test]
fn test_array_syntax_error() {
    for template in get_failing_tests_for_array() {
        match support::maybe_parsed(template) {
            Ok(_) => panic!("expected {:?} to produce error", template),
            Err(e) => { println!("tmp {} produces {}", template, e); },
        }
    }
}

fn get_failing_tests_for_array<'r>() -> Vec<&'static str> {
    vec![
        r#"{{ [1, "a": "b"] }}"#,
        r#"{{ {"a": "b", 2} }}"#,
    ]
}
