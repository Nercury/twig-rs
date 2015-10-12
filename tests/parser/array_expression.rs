extern crate twig;

use super::support;
use twig::node::{ Expr };

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
    ]
}
