//! The nesting limit of the recursive parser (a documented deviation: PHP Twig's parser recursion is bounded only by
//! memory, the Rust parser by the thread's stack).

use twig::Source;
use twig::deprecation::Deprecation;
use twig::extension::ExtensionSet;
use twig::parser::{DEFAULT_MAX_NESTING_LEVEL, Parser};

use super::support::parser_set;
use super::tags::tag_set;

/// Runs `f` on a thread with a large stack (debug builds need about 8 KiB per nesting level).
fn with_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(f)
        .unwrap()
        .join()
        .unwrap()
}

fn parse(set: &ExtensionSet, code: &str, max: Option<usize>) -> Result<(), String> {
    let mut deprecations: Vec<Deprecation> = Vec::new();
    let source = Source::new(code, "index");
    let stream = set
        .lexer(Default::default())
        .tokenize(&source, &mut deprecations)
        .map_err(|e| e.to_string())?;
    let mut parser = Parser::new(set, stream, &mut deprecations);
    if let Some(max) = max {
        parser.set_max_nesting_level(max);
    }
    parser
        .parse(None, false)
        .map(drop)
        .map_err(|e| e.to_string())
}

fn parens(n: usize) -> String {
    format!("{{{{ {}1{} }}}}", "(".repeat(n), ")".repeat(n))
}

#[test]
fn nested_expressions_stop_at_the_limit_instead_of_overflowing_the_stack() {
    with_stack(|| {
        let set = parser_set();
        // the body and the print statement's expression take one level each
        assert_eq!(
            parse(&set, &parens(DEFAULT_MAX_NESTING_LEVEL - 2), None),
            Ok(())
        );
        let message = "Maximum nesting level of 128 reached in \"index\" at line 1.".to_owned();
        assert_eq!(
            parse(&set, &parens(DEFAULT_MAX_NESTING_LEVEL - 1), None),
            Err(message.clone())
        );
        assert_eq!(parse(&set, &parens(100_000), None), Err(message));
        let deep = format!("{{{{ {}1 }}}}", "-".repeat(100_000));
        assert!(
            parse(&set, &deep, None)
                .unwrap_err()
                .starts_with("Maximum nesting level")
        );
        let deep = format!("{{{{ {}1{} }}}}", "[".repeat(100_000), "]".repeat(100_000));
        assert!(
            parse(&set, &deep, None)
                .unwrap_err()
                .starts_with("Maximum nesting level")
        );
    });
}

#[test]
fn the_limit_is_configurable() {
    with_stack(|| {
        let set = parser_set();
        assert_eq!(parse(&set, &parens(500), Some(1000)), Ok(()));
        assert_eq!(
            parse(&set, "{{ ((1)) }}", Some(3)),
            Err("Maximum nesting level of 3 reached in \"index\" at line 1.".to_owned())
        );
    });
}

#[test]
fn nested_tags_count_too() {
    with_stack(|| {
        let set = tag_set();
        let code = format!(
            "{}x{}",
            (0..200)
                .map(|i| format!("{{% block b{i} %}}"))
                .collect::<String>(),
            "{% endblock %}".repeat(200)
        );
        assert!(
            parse(&set, &code, None)
                .unwrap_err()
                .starts_with("Maximum nesting level of 128 reached")
        );
        let code = format!(
            "{}x{}",
            (0..50)
                .map(|i| format!("{{% block b{i} %}}"))
                .collect::<String>(),
            "{% endblock %}".repeat(50)
        );
        assert_eq!(parse(&set, &code, None), Ok(()));
    });
}
