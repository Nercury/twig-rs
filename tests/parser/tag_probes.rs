//! The tag probes of `reference/twig/syntax.md` §9 (Appendix A) and the examples of §4 and §6, verified on the
//! oracle: parse errors with their lines and columns, and the parse-level facts of the probes that render.

use std::sync::Arc;

use twig::ast::{NodeKey, NodeKind};
use twig::deprecation::Deprecation;
use twig::extension::{ExtensionSet, SandboxExtension};
use twig::parser::ParseContext;
use twig::{Error, Source};

fn context() -> ParseContext {
    ParseContext::new(ExtensionSet::with_core_extensions())
}

fn sandbox_context() -> ParseContext {
    let mut set = ExtensionSet::with_core_extensions();
    set.add_extension(Arc::new(SandboxExtension::new()))
        .unwrap();
    ParseContext::new(set)
}

/// Parses like `$env->loadTemplate()` does up to the compiler (`compileSource()`'s error handling).
fn load(context: &ParseContext, code: &str) -> (Result<twig::ast::Node, Error>, Vec<String>) {
    let mut deprecations: Vec<Deprecation> = Vec::new();
    let result = context.parse_template(&Source::new(code, "index"), &mut deprecations);
    (
        result,
        deprecations.iter().map(ToString::to_string).collect(),
    )
}

fn assert_errors(context: &ParseContext, cases: &[(&str, &str)]) {
    let mut failures = Vec::new();
    for (code, expected) in cases {
        match load(context, code).0 {
            Ok(_) => failures.push(format!("{code:?}: parsed, expected {expected}")),
            Err(error) if error.to_string() != *expected => {
                failures.push(format!(
                    "{code:?}\n  expected {expected}\n  actual   {error}"
                ));
            }
            Err(_) => {}
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn appendix_a_tag_errors() {
    assert_errors(
        &context(),
        &[
            (
                "{% do a.b = 2 %}",
                "Cannot assign to \"Twig\\Node\\Expression\\GetAttrExpression\", only variables can be assigned in \"index\" at line 1.",
            ),
            (
                "{% do [] = [] %}",
                "Cannot destructure to an empty list of variables in \"index\" at line 1.",
            ),
            (
                "{% if x %}\n{% endfor %}",
                "Unexpected \"endfor\" tag (expecting closing tag for the \"if\" tag defined near line 2) in \"index\" at line 2.",
            ),
            (
                "{% autoescape v %}{{ v }}{% endautoescape %}",
                "An escaping strategy must be a string or false in \"index\" at line 1.",
            ),
            (
                "{% block a %}x{% endblock b %}",
                "Expected endblock for block \"a\" (but \"b\" given) in \"index\" at line 1.",
            ),
            (
                "{% block a %}{% endblock %}\n{% block a %}{% endblock %}",
                "The block 'a' has already been defined line 1 in \"index\" at line 2.",
            ),
            (
                "{% extends 'b' %}{% if true %}\n{% block a %}{% endblock %}{% endif %}",
                "A \"block\" tag cannot be under a \"if\" tag (line 1) in \"index\" at line 2.",
            ),
            (
                "{% block a %}{{ parent() }}{% endblock %}{% extends 'b' %}",
                "Calling the \"parent\" function on a template that does not call \"extends\" or \"use\" is forbidden in \"index\" at line 1.",
            ),
            (
                "{{ parent() }}",
                "Calling the \"parent\" function outside of a block is forbidden in \"index\" at line 1.",
            ),
            (
                "{% deprecated 'x' foo='a' %}",
                "Unknown \"foo\" option in \"index\" at line 1.",
            ),
            (
                "{% embed 'b' %}\n  text\n{% endembed %}",
                "A template that extends another one cannot include content outside Twig blocks. Did you forget to put the content inside a {% block %} tag in \"index\" at line 2?",
            ),
            (
                "{% extends 'b' %}{% extends 'c' %}",
                "Multiple extends tags are forbidden in \"index\" at line 1.",
            ),
            (
                "{% extends 'b' %}\n{% block a %}{% extends 'c' %}{% endblock %}",
                "Cannot use \"extend\" in a block in \"index\" at line 2.",
            ),
            (
                "{% extends 'b' %}{% macro m() %}{% extends 'c' %}{% endmacro %}",
                "Cannot use \"extend\" in a macro in \"index\" at line 1.",
            ),
            (
                "{% for x in y if x %}{% endfor %}",
                "Unexpected token \"name\" of value \"if\" (\"end of statement block\" expected) in \"index\" at line 1 column 15.",
            ),
            (
                "{% for x in y %}",
                "Unexpected end of template in \"index\" at line 1.",
            ),
            (
                "{% from _self import m as n, m as o %}... {{ n() }}",
                "Unknown \"n\" function. Did you mean \"min\", \"enum\", \"range\", \"parent\", \"random\", \"include\", \"constant\", \"enum_cases\", \"include_only\" in \"index\" at line 1?",
            ),
            (
                "{% guard foo max %}{% endguard %}",
                "Supported guard types are function, filter and test, \"foo\" given in \"index\" at line 1.",
            ),
            (
                "{% include 'b' with {} ignore missing %}",
                "Unexpected token \"name\" of value \"ignore\" (\"end of statement block\" expected) in \"index\" at line 1 column 24.",
            ),
            (
                "{% macro m(...rest, a) %}{% endmacro %}",
                "The variadic argument \"rest\" in macro \"m\" must be the last one in \"index\" at line 1.",
            ),
            (
                "{% macro m(...rest = 1) %}{% endmacro %}",
                "The variadic argument \"rest\" in macro \"m\" cannot have a default value in \"index\" at line 1.",
            ),
            (
                "{% macro m(a, ...a) %}{% endmacro %}",
                "The variadic argument \"a\" in macro \"m\" cannot have the same name as another argument in \"index\" at line 1.",
            ),
            (
                "{% macro m(a, a) %}{% endmacro %}",
                "Argument \"a\" is defined twice for macro \"m\" in \"index\" at line 1.",
            ),
            (
                "{% macro m(varargs) %}{% endmacro %}",
                "The argument \"varargs\" in macro \"m\" cannot be defined because the variable \"varargs\" is reserved for arbitrary arguments in \"index\" at line 1.",
            ),
            (
                "{% macro m(a = b) %}{% endmacro %}",
                "A default value for an argument must be a constant (a boolean, a string, a number, a sequence, or a mapping) in \"index\" at line 1.",
            ),
            (
                "{% macro m() %}{% endmacro n %}",
                "Expected endmacro for macro \"m\" (but \"n\" given) in \"index\" at line 1.",
            ),
            (
                "{% set a, b = 1 %}",
                "When using set, you must have the same number of variables and assignments in \"index\" at line 1.",
            ),
            (
                "{% set a, b %}x{% endset %}",
                "When using set with a block, you cannot have a multi-target in \"index\" at line 1.",
            ),
            (
                "{% types a ? : 'x' %}",
                "A type name must be followed by a colon (:). Unexpected token \"operator\" of value \"? :\" (\"punctuation\" expected with value \":\") in \"index\" at line 1 column 12.",
            ),
            (
                "{% use x %}",
                "The template references in a \"use\" statement must be a string in \"index\" at line 1.",
            ),
            (
                "{% filter upper %}x{% endfilter %}",
                "Unknown \"filter\" tag in \"index\" at line 1.",
            ),
            (
                "{% includ %}",
                "Unknown \"includ\" tag. Did you mean \"include\" in \"index\" at line 1?",
            ),
            (
                "{% 1 %}",
                "A block must start with a tag name in \"index\" at line 1.",
            ),
            (
                "{% do {(a): b} = {x: 1} %}",
                "An exception has been thrown during the compilation of a template (\"Attribute \"value\" does not exist for Node \"Twig\\Node\\Expression\\Variable\\ContextVariable\".\") in \"index\".",
            ),
            (
                "{% macro n() %}N{% endmacro %}{% block a %}{% from _self import n %}{{ n() }}{% block b %}{{ n() }}{% endblock %}{% endblock %}",
                "Unknown \"n\" function. Did you mean \"min\", \"enum\", \"range\", \"parent\", \"random\", \"include\", \"constant\", \"enum_cases\", \"include_only\" in \"index\" at line 1?",
            ),
        ],
    );
}

#[test]
fn appendix_a_sandbox() {
    let (module, deprecations) = load(&sandbox_context(), "{% sandbox %}x{% endsandbox %}");
    assert!(module.is_ok());
    assert_eq!(
        deprecations,
        ["Since twig/twig 3.15: The \"sandbox\" tag is deprecated in \"index\" at line 1."]
    );
    assert_errors(
        &sandbox_context(),
        &[(
            "{% sandbox %}{{ x }}{% endsandbox %}",
            "Only \"include\" tags are allowed within a \"sandbox\" section in \"index\" at line 1.",
        )],
    );
    assert_errors(
        &context(),
        &[(
            "{% sandbox %}x{% endsandbox %}",
            "Unknown \"sandbox\" tag in \"index\" at line 1.",
        )],
    );
}

/// The probes that render: they parse, and their tree shows the parse-level part of the behaviour.
#[test]
fn appendix_a_rendering_probes_parse() {
    for code in [
        "{% autoescape true %}{{ v }}{% endautoescape %}",
        "{% guard function nope %}{{ nope() }}{% else %}N{% endguard %}",
        "{% with 1 %}{% endwith %}",
        "\u{feff}  {% extends 'b' %}",
        "{% block a-b %}",
        "{% do [x, y] = [1, 2] %}{{ x }}{{ y }}|{% do [, y2] = [1, 2] %}{{ y2 }}|{% do {a, b: c} = {a: 1, b: 2} %}{{ a }}{{ c }}",
        "{% if true %}\r\nX{% endif %}\r\nY|a\r\nb",
    ] {
        let (result, _) = load(&context(), code);
        assert!(result.is_ok(), "{code:?}: {result:?}");
    }

    // `%}` eats one newline: the if body is "X", the text after the tag starts with "Y"
    let (module, _) = load(&context(), "{% if true %}\nX{% endif %}\nY{{ 1 }}\nZ");
    let module = module.unwrap();
    let body = module
        .as_module()
        .unwrap()
        .body
        .as_nodes()
        .unwrap()
        .get(&NodeKey::Index(0))
        .unwrap()
        .as_nodes()
        .unwrap();
    let NodeKind::If(if_node) = &body.get(&NodeKey::Index(0)).unwrap().kind else {
        panic!("not an if");
    };
    let then = if_node
        .tests
        .as_nodes()
        .unwrap()
        .get(&NodeKey::Index(1))
        .unwrap();
    assert!(matches!(&then.kind, NodeKind::Text(text) if text.data == b"X"));
    assert!(
        matches!(&body.get(&NodeKey::Index(1)).unwrap().kind, NodeKind::Text(text) if text.data == b"Y")
    );
}

/// §6 introduction: unclosed bodies fail with `Unexpected end of template.` where the token parser calls `next()`,
/// and with an `expect()` error elsewhere.
#[test]
fn unclosed_bodies() {
    let end = |line: i64| format!("Unexpected end of template in \"index\" at line {line}.");
    let expect = |column: usize| {
        format!(
            "Unexpected token \"end of template\" (\"end of statement block\" expected) in \"index\" at line 1 column {column}."
        )
    };
    let cases = [
        ("{% for x in y %}", end(1)),
        ("{% if x %}{% else %}", end(1)),
        ("{% if x %}{% elseif y %}", end(1)),
        ("{% guard function nope %}", end(1)),
        ("{% if x %}\n\nabc", end(3)),
        ("{% if a %}{% if b %}{% endif %}", end(1)),
        ("{% for x in y %}{% else %}", expect(27)),
        ("{% guard function max %}{% else %}", expect(35)),
        ("{% apply upper %}x", expect(19)),
        ("{% block a %}x", expect(15)),
        ("{% macro m() %}x", expect(17)),
        ("{% set a %}x", expect(13)),
        ("{% with {a:1} %}", expect(17)),
        ("{% autoescape %}x", expect(18)),
    ];
    let cases: Vec<(&str, &str)> = cases.iter().map(|(c, e)| (*c, e.as_str())).collect();
    assert_errors(&context(), &cases);
    assert_errors(&sandbox_context(), &[("{% sandbox %}x", &expect(15))]);
}

/// §6 examples of each tag that are not in Appendix A.
#[test]
fn tag_examples() {
    assert_errors(
        &context(),
        &[
            (
                "{% apply %}",
                "Unexpected token \"end of statement block\" (\"name\" expected) in \"index\" at line 1 column 10.",
            ),
            (
                "{% apply upper x %}",
                "Unexpected token \"name\" of value \"x\" (\"end of statement block\" expected) in \"index\" at line 1 column 16.",
            ),
            (
                "{% block a 'x' %}{% endblock %}",
                "Unknown \"endblock\" tag in \"index\" at line 1.",
            ),
            (
                "{% block 1 %}",
                "Unexpected token \"number\" of value \"1\" (\"name\" expected) in \"index\" at line 1 column 10.",
            ),
            (
                "{% block a %}\n\nabc",
                "Unexpected token \"end of template\" (\"end of statement block\" expected) in \"index\" at line 3 column 4.",
            ),
            (
                "{% deprecated 'x' package 'a' %}",
                "Unexpected token \"string\" of value \"a\" (\"operator\" expected with value \"=\") in \"index\" at line 1 column 27.",
            ),
            (
                "{% deprecated %}",
                "Unexpected token \"end of statement block\" of value \"\" in \"index\" at line 1.",
            ),
            (
                "{% embed 'b' %}{% endif %}{% endembed %}",
                "Unexpected \"endif\" tag (expecting closing tag for the \"embed\" tag defined near line 1) in \"index\" at line 1.",
            ),
            (
                "{% extends 'b' 'c' %}",
                "Unexpected token \"string\" of value \"c\" (\"end of statement block\" expected) in \"index\" at line 1 column 16.",
            ),
            (
                "{% flush x %}",
                "Unexpected token \"name\" of value \"x\" (\"end of statement block\" expected) in \"index\" at line 1 column 10.",
            ),
            (
                "{% for x %}",
                "Unexpected token \"end of statement block\" (\"operator\" expected with value \"in\") in \"index\" at line 1 column 10.",
            ),
            (
                "{% for 1 in x %}",
                "Only variables can be assigned to. Unexpected token \"number\" of value \"1\" (\"name\" expected) in \"index\" at line 1 column 8.",
            ),
            (
                "{% for x in y %}{% else %}{% else %}{% endfor %}",
                "Unexpected \"else\" tag (expecting closing tag for the \"for\" tag defined near line 1) in \"index\" at line 1.",
            ),
            (
                "{% for true in x %}",
                "You cannot assign a value to \"true\" in \"index\" at line 1.",
            ),
            (
                "{% from _self %}",
                "Unexpected token \"end of statement block\" (\"name\" expected with value \"import\") in \"index\" at line 1 column 15.",
            ),
            (
                "{% from _self import m as true %}",
                "You cannot assign a value to \"true\" in \"index\" at line 1.",
            ),
            (
                "{% include 'b' ignore %}",
                "Unexpected token \"end of statement block\" (\"name\" expected with value \"missing\") in \"index\" at line 1 column 23.",
            ),
            (
                "{% if %}",
                "Unexpected token \"end of statement block\" of value \"\" in \"index\" at line 1.",
            ),
            (
                "{% if x %}{% else %}{% elseif y %}{% endif %}",
                "Unexpected \"elseif\" tag (expecting closing tag for the \"if\" tag defined near line 1) in \"index\" at line 1.",
            ),
            (
                "{% import _self %}",
                "Unexpected token \"end of statement block\" (\"name\" expected with value \"as\") in \"index\" at line 1 column 17.",
            ),
            (
                "{% import _self as m.n %}",
                "Unexpected token \"operator\" of value \".\" (\"end of statement block\" expected) in \"index\" at line 1 column 21.",
            ),
            (
                "{% macro m(a: 1) %}",
                "Arguments must be separated by a comma. Unexpected token \"punctuation\" of value \":\" (\"punctuation\" expected with value \",\") in \"index\" at line 1 column 13.",
            ),
            (
                "{% macro m %}",
                "A list of arguments must begin with an opening parenthesis. Unexpected token \"end of statement block\" (\"operator\" expected with value \"(\") in \"index\" at line 1 column 12.",
            ),
            (
                "{% macro m(\"a\") %}",
                "An argument must be a name. Unexpected token \"string\" of value \"a\" (\"name\" expected) in \"index\" at line 1 column 12.",
            ),
            (
                "{% macro m(...) %}",
                "A variadic argument must be a name. Unexpected token \"punctuation\" of value \")\" (\"name\" expected) in \"index\" at line 1 column 15.",
            ),
            (
                "{% set a.b = 1 %}",
                "Unexpected token \"operator\" of value \".\" (\"end of statement block\" expected) in \"index\" at line 1 column 9.",
            ),
            (
                "{% set %}",
                "Only variables can be assigned to. Unexpected token \"end of statement block\" (\"name\" expected) in \"index\" at line 1 column 8.",
            ),
            (
                "{% set a = 1 b = 2 %}",
                "Unexpected token \"name\" of value \"b\" (\"end of statement block\" expected) in \"index\" at line 1 column 14.",
            ),
            (
                "{% set none = 1 %}",
                "You cannot assign a value to \"none\" in \"index\" at line 1.",
            ),
            (
                "{% types a: b %}",
                "Unexpected token \"name\" of value \"b\" (\"string\" expected) in \"index\" at line 1 column 13.",
            ),
            (
                "{% types a: 'x' b: 'y' %}",
                "A type string must be followed by a comma. Unexpected token \"name\" of value \"b\" (\"punctuation\" expected with value \",\") in \"index\" at line 1 column 17.",
            ),
            (
                "{% use 'b' ~ 'c' %}",
                "The template references in a \"use\" statement must be a string in \"index\" at line 1.",
            ),
            (
                "{% if x %}{% foo %}{% endif %}",
                "Unexpected \"foo\" tag (expecting closing tag for the \"if\" tag defined near line 1) in \"index\" at line 1.",
            ),
            (
                "{% if x %}\n\n  {{ y }}{% endfor %}",
                "Unexpected \"endfor\" tag (expecting closing tag for the \"if\" tag defined near line 2) in \"index\" at line 3.",
            ),
            (
                "{% endif %}",
                "Unknown \"endif\" tag in \"index\" at line 1.",
            ),
            (
                "{% endif x %}",
                "Unknown \"endif\" tag in \"index\" at line 1.",
            ),
            (
                "{% if x %}{% endif x %}",
                "Unexpected token \"name\" of value \"x\" (\"end of statement block\" expected) in \"index\" at line 1 column 20.",
            ),
            (
                "{% e %}",
                "Unknown \"e\" tag. Did you mean \"use\", \"set\", \"types\", \"embed\", \"extends\", \"include\", \"deprecated\", \"autoescape\" in \"index\" at line 1?",
            ),
            (
                "{% line 0 %}{{ x",
                "Unexpected token \"end of template\" (\"end of print statement\" expected) in \"index\".",
            ),
            (
                "x{% extends 'b' %}{{ 1 2 }}",
                "Unexpected token \"number\" of value \"2\" (\"end of print statement\" expected) in \"index\" at line 1 column 24.",
            ),
        ],
    );
}

/// §6 examples that parse: the tag accepts names and operator words where Twig does.
#[test]
fn tags_accept_what_twig_accepts() {
    for code in [
        "{% block in%}{% endblock %}",
        "{% for in in [1,2] %}{{ in }}{% endfor %}",
        "{% set in = 1 %}",
        "{% set b-and = 1 %}",
        "{% set d = e = 3 %}",
        "{% types %}",
        "{% types {} %}",
        "{% with only %}{% endwith %}",
        "{% include 'b' with only %}",
        "{% use 1 %}",
        "{% for a, b, c in {x: 1} %}{{ a }}{% endfor %}",
        "{% guard test divisible by %}{% endguard %}",
        "{% macro m(a = -1, b = +2, c = [1, {c: 'd'}], d = null, e = 'x') %}{% endmacro %}",
        "{% do a = b = 'foo' %}",
        "{% block c 'z' %}",
    ] {
        let (result, _) = load(&context(), code);
        assert!(result.is_ok(), "{code:?}: {result:?}");
    }
}

/// A deviation: when `next()` fails at the end of the stream with an error at line 0 (only possible after
/// `{% line 0 %}`), `Parser::parse()` reads the current token past the end, where PHP raises an "Undefined array key"
/// warning and a `TypeError` (`TokenStream::getCurrent(): Return value must be of type Twig\Token, null returned`).
/// twig-rs keeps the syntax error (see `TokenStream::try_current()`).
#[test]
fn line_zero_error_past_the_end_of_the_stream() {
    assert_errors(
        &context(),
        &[(
            "{% line 0 %}{% if true %}x",
            "Unexpected end of template in \"index\".",
        )],
    );
}
