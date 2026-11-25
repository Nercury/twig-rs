//! The environment `doIntegrationTest()` builds for `Twig\Tests\IntegrationTest` (TWIG/tests/IntegrationTest.php,
//! `reference/twig/test-suite.md` §3), as far as twig-rs has it: the extensions in registration order and the
//! undefined-callable callbacks, for the parser. The test callables carry their PHP reflection data (names and
//! parameters, checked with the oracle) so that the compiler can bind arguments; their implementations, the
//! `global` global, the runtime loader and the security policy come with the runtime stages.

use std::any::Any;
use std::sync::Arc;

use twig::Error;
use twig::ast::{ConstantValue, Expr, Node};
use twig::extension::{
    CallableParameter as P, DebugExtension, DeprecatedCallableInfo, Extension, ExtensionSet,
    FilterOptions, FunctionOptions, PhpCallable, SandboxExtension, StringLoaderExtension,
    TestOptions, TokenParser, TwigFilter, TwigFunction, TwigTest,
};
use twig::lexer::{Token, TokenType};
use twig::parser::{ParseContext, Parser};

const TEST_EXTENSION: &str = "Twig\\Tests\\TwigTestExtension";

/// A parse context with the extensions and callbacks of the harness: `new Environment($loader, $config)` (Core and
/// Escaper; the yield-ready and optimizer extensions only add visitors), then `getExtensions()` in order (Debug,
/// Sandbox, StringLoader, `TwigTestExtension`, `TwigTestExtensionWithAttributes('*')`,
/// `AttributeExtension(TwigTestLazyExtensionWithAttributes::class)`), then the undefined filter, function and test
/// callbacks. Every case gets a fresh one, as every case gets a fresh `Environment`.
pub fn parse_context() -> Result<ParseContext, Error> {
    let mut set = ExtensionSet::with_core_extensions();
    set.add_extension(Arc::new(DebugExtension::new()))?;
    // `new SandboxExtension(new SecurityPolicy([], [], [], [], ['dump']), false)`: not global; the policy comes
    // with the sandbox stage
    set.add_extension(Arc::new(SandboxExtension::new()))?;
    set.add_extension(Arc::new(StringLoaderExtension::new()))?;
    set.add_extension(Arc::new(TwigTestExtension))?;
    set.add_extension(Arc::new(AttributeCallables {
        class: "Twig\\Tests\\TwigTestExtensionWithAttributes",
        prefix: "",
    }))?;
    set.add_extension(Arc::new(AttributeCallables {
        class: "Twig\\Extension\\AttributeExtension",
        prefix: "lazy_",
    }))?;
    set.register_undefined_filter_callback(Arc::new(|name: &[u8]| {
        if name == b"throwing_undefined_filter" {
            return Err(Error::syntax("This filter is undefined in the tests."));
        }
        Ok(None)
    }));
    set.register_undefined_function_callback(Arc::new(|name: &[u8]| {
        if name == b"throwing_undefined_function" {
            return Err(Error::syntax("This function is undefined in the tests."));
        }
        Ok(None)
    }));
    set.register_undefined_test_callback(Arc::new(|name: &[u8]| {
        if name == b"throwing_undefined_test" || name == b"throwing_undefined_two words_test" {
            return Err(Error::syntax("This test is undefined in the tests."));
        }
        // "Ensure this does not conflict with `divisible by` and `same as`."
        if name == b"divisible" || name == b"same" {
            return Ok(Some(TwigTest::new(
                name.to_vec(),
                Some(PhpCallable::closure(Vec::new())),
                TestOptions::default(),
            )));
        }
        Ok(None)
    }));
    Ok(ParseContext::new(set))
}

/// A method of `TwigTestExtension`.
fn method(name: &str, parameters: Vec<P>) -> Option<PhpCallable> {
    Some(PhpCallable::method(TEST_EXTENSION, name, parameters))
}

fn value() -> Vec<P> {
    vec![P::required("value")]
}

fn safe(strategies: &[&str]) -> Option<Vec<ConstantValue>> {
    Some(strategies.iter().map(|s| (*s).into()).collect())
}

/// `TwigTestTokenParser_§`: `{% § %}` prints `§`.
struct SectionTokenParser;

impl TokenParser for SectionTokenParser {
    fn tag(&self) -> &str {
        "§"
    }

    /// `$this->parser->getStream()->expect(Token::BLOCK_END_TYPE); return new PrintNode(new
    /// ConstantExpression('§', -1), -1);`
    fn parse(&self, parser: &mut Parser<'_>, _token: &Token) -> twig::Result<Option<Node>> {
        parser
            .stream_mut()
            .expect(TokenType::BlockEnd, None, None)?;
        Ok(Some(Node::print(Expr::constant("§", -1), -1)))
    }

    fn class_name(&self) -> &str {
        "Twig\\Tests\\TwigTestTokenParser_§"
    }
}

/// `Twig\Tests\TwigTestExtension`.
struct TwigTestExtension;

impl Extension for TwigTestExtension {
    fn class_name(&self) -> &str {
        TEST_EXTENSION
    }

    fn token_parsers(&self) -> Vec<Arc<dyn TokenParser>> {
        vec![Arc::new(SectionTokenParser)]
    }

    fn filters(&self) -> Vec<TwigFilter> {
        let d = FilterOptions::default;
        let sep = || P::optional("sep", "<br />".into());
        // `[$this, 'magicCall']` and `'...::magicStaticCall'` go through __call / __callStatic: the trampoline
        // closures reflect no parameter
        let magic = || method("magicCall", Vec::new());
        let magic_static = || method("magicStaticCall", Vec::new());
        vec![
            TwigFilter::new("§", method("§Filter", value()), d()),
            TwigFilter::new(
                "escape_and_nl2br",
                method(
                    "escape_and_nl2br",
                    vec![P::required("env"), P::required("value"), sep()],
                ),
                FilterOptions {
                    needs_environment: true,
                    is_safe: safe(&["html"]),
                    ..d()
                },
            ),
            TwigFilter::new(
                "nl2br",
                method("nl2br", vec![P::required("value"), sep()]),
                FilterOptions {
                    pre_escape: Some("html".to_owned()),
                    is_safe: safe(&["html"]),
                    ..d()
                },
            ),
            TwigFilter::new(
                "escape_something",
                method("escape_something", value()),
                FilterOptions {
                    is_safe: safe(&["something"]),
                    ..d()
                },
            ),
            TwigFilter::new(
                "preserves_safety",
                method("preserves_safety", value()),
                FilterOptions {
                    preserves_safety: safe(&["html"]),
                    ..d()
                },
            ),
            TwigFilter::new("static_call_string", method("staticCall", value()), d()),
            TwigFilter::new("static_call_array", method("staticCall", value()), d()),
            TwigFilter::new("magic_call", magic(), d()),
            TwigFilter::new("magic_call_closure", magic(), d()),
            TwigFilter::new("magic_call_string", magic_static(), d()),
            TwigFilter::new("magic_call_array", magic_static(), d()),
            TwigFilter::new(
                "*_path",
                method(
                    "dynamic_path",
                    vec![P::required("element"), P::required("item")],
                ),
                d(),
            ),
            TwigFilter::new(
                "*_foo_*_bar",
                method(
                    "dynamic_foo",
                    vec![P::required("foo"), P::required("bar"), P::required("item")],
                ),
                d(),
            ),
            TwigFilter::new("not", method("notFilter", value()), d()),
            TwigFilter::new(
                "anon_foo",
                Some(PhpCallable::closure(vec![P::required("name")])),
                d(),
            ),
        ]
    }

    fn functions(&self) -> Vec<TwigFunction> {
        let d = FunctionOptions::default;
        vec![
            TwigFunction::new("§", method("§Function", value()), d()),
            TwigFunction::new(
                "safe_br",
                method("br", Vec::new()),
                FunctionOptions {
                    is_safe: safe(&["html"]),
                    ..d()
                },
            ),
            TwigFunction::new("unsafe_br", method("br", Vec::new()), d()),
            TwigFunction::new("static_call_string", method("staticCall", value()), d()),
            TwigFunction::new("static_call_array", method("staticCall", value()), d()),
            TwigFunction::new(
                "*_path",
                method(
                    "dynamic_path",
                    vec![P::required("element"), P::required("item")],
                ),
                d(),
            ),
            TwigFunction::new(
                "*_foo_*_bar",
                method(
                    "dynamic_foo",
                    vec![P::required("foo"), P::required("bar"), P::required("item")],
                ),
                d(),
            ),
            TwigFunction::new(
                "anon_foo",
                Some(PhpCallable::closure(vec![P::required("name")])),
                d(),
            ),
            TwigFunction::new(
                "deprecated_function",
                Some(PhpCallable::closure(Vec::new())),
                FunctionOptions {
                    deprecation_info: Some(DeprecatedCallableInfo {
                        alt_name: Some("not_deprecated_function".to_owned()),
                        ..DeprecatedCallableInfo::new("foo/bar", "1.1")
                    }),
                    ..d()
                },
            ),
        ]
    }

    fn tests(&self) -> Vec<TwigTest> {
        let d = TestOptions::default;
        vec![
            TwigTest::new("multi word", method("is_multi_word", value()), d()),
            TwigTest::new(
                "test_*",
                method(
                    "dynamic_test",
                    vec![P::required("element"), P::required("item")],
                ),
                d(),
            ),
        ]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// The attribute-declared callables: `TwigTestExtensionWithAttributes('*')` (`attribute_*`, methods of that class)
/// and `AttributeExtension(TwigTestLazyExtensionWithAttributes::class)` (`lazy_attribute_*`, methods of the lazily
/// loaded runtime class).
struct AttributeCallables {
    class: &'static str,
    prefix: &'static str,
}

impl AttributeCallables {
    fn callable(&self, method: &str) -> Option<PhpCallable> {
        let class = if self.prefix.is_empty() {
            "Twig\\Tests\\TwigTestExtensionWithAttributes"
        } else {
            "Twig\\Tests\\TwigTestLazyExtensionWithAttributes"
        };
        Some(PhpCallable::method(class, method, value()))
    }
}

impl Extension for AttributeCallables {
    fn class_name(&self) -> &str {
        self.class
    }

    fn filters(&self) -> Vec<TwigFilter> {
        vec![TwigFilter::new(
            format!("{}attribute_filter", self.prefix),
            self.callable("attributeFilter"),
            FilterOptions::default(),
        )]
    }

    fn functions(&self) -> Vec<TwigFunction> {
        vec![TwigFunction::new(
            format!("{}attribute_function", self.prefix),
            self.callable("attributeFunction"),
            FunctionOptions::default(),
        )]
    }

    fn tests(&self) -> Vec<TwigTest> {
        vec![TwigTest::new(
            format!("{}attribute_test", self.prefix),
            self.callable("attributeTest"),
            TestOptions::default(),
        )]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
