//! The test-only extensions and callbacks of `Twig\Tests\IntegrationTest` (TWIG/tests/IntegrationTest.php,
//! `reference/twig/test-suite.md` §3.4-§3.5), ported: `TwigTestExtension` (the `§` tag and its filters, functions and
//! tests), `TwigTestExtensionWithAttributes('*')`, `AttributeExtension(TwigTestLazyExtensionWithAttributes::class)` with
//! the runtime of its `FactoryRuntimeLoader`, and the undefined filter, function and test callbacks.
//!
//! Every callable carries the reflection data of its PHP callable (names and parameters, checked with the oracle),
//! which the compiler uses to bind arguments, and a [`CallableRuntime`] running the body of the PHP method with PHP's
//! semantics (userland argument binding, string conversions and builtins from `twig-value`). The file declares no
//! node class: the `§` token parser returns a core `PrintNode`.

use std::any::Any;
use std::sync::Arc;

use twig::Error;
use twig::ast::{ConstantValue, Expr, Node};
use twig::extension::runtime::{Call, CallResult, CallableRuntime};
use twig::extension::{
    CallableParameter as P, DeprecatedCallableInfo, Extension, FilterOptions, FunctionOptions,
    PhpCallable, TestOptions, TokenParser, TwigFilter, TwigFunction, TwigTest,
    UndefinedFilterCallback, UndefinedFunctionCallback, UndefinedTestCallback,
};
use twig::lexer::{Token, TokenType};
use twig::parser::Parser;
use twig::runtime::Environment;
use twig::value::builtins::string::{str_contains, str_replace, strtoupper};
use twig::value::object::{CallArgs, FunctionKind, Param};
use twig::value::param::{DeclaredType, TypeMask, UserArg, verify_user_arg};
use twig::value::{Diagnostics, PhpResult, PhpString, Value, compare, ops};

/// `Twig\Tests\TwigTestExtension`.
pub const TEST_EXTENSION: &str = "Twig\\Tests\\TwigTestExtension";
/// `Twig\Tests\TwigTestExtensionWithAttributes`: an `AttributeExtension` subclass, its own runtime.
pub const ATTRIBUTE_EXTENSION: &str = "Twig\\Tests\\TwigTestExtensionWithAttributes";
/// `Twig\Tests\TwigTestLazyExtensionWithAttributes`: the runtime class of the lazily loaded attribute callables.
pub const LAZY_ATTRIBUTE_RUNTIME: &str = "Twig\\Tests\\TwigTestLazyExtensionWithAttributes";

/// Attaches `body` as the runtime of `callable`.
fn with_runtime(
    callable: PhpCallable,
    body: impl Fn(&mut Call<'_>) -> CallResult + Send + Sync + 'static,
) -> Option<PhpCallable> {
    CallableRuntime::new(body).attach(&callable);
    Some(callable)
}

/// A method of `TwigTestExtension` with its runtime.
fn method(
    name: &str,
    parameters: Vec<P>,
    body: impl Fn(&mut Call<'_>) -> CallResult + Send + Sync + 'static,
) -> Option<PhpCallable> {
    with_runtime(PhpCallable::method(TEST_EXTENSION, name, parameters), body)
}

/// A closure with its runtime.
fn closure(
    parameters: Vec<P>,
    body: impl Fn(&mut Call<'_>) -> CallResult + Send + Sync + 'static,
) -> Option<PhpCallable> {
    with_runtime(PhpCallable::closure(parameters), body)
}

fn value() -> Vec<P> {
    vec![P::required("value")]
}

fn safe(strategies: &[&str]) -> Option<Vec<ConstantValue>> {
    Some(strategies.iter().map(|s| (*s).into()).collect())
}

/// The PHP arguments of a call bound to the parameters of a userland function: `injected` placeholders for the
/// parameters Twig injects first (`$env`), the parts of a dynamic name (`*_path`), then the call's arguments
/// (`zend_handle_named_arg()` and the userland `Too few arguments` error; extra arguments are ignored).
fn bind(
    call: &mut Call<'_>,
    function: &str,
    injected: usize,
    params: &[Param],
) -> CallResult<Vec<Value>> {
    let mut args: CallArgs = call.take_args();
    let mut leading = vec![Value::Null; injected];
    leading.extend(
        call.dynamic_arguments()
            .iter()
            .map(|part| Value::String(PhpString::from(part.as_slice()))),
    );
    if !leading.is_empty() {
        args.positional.splice(0..0, leading);
    }
    Ok(args.bind(function, params, FunctionKind::User)?)
}

/// [`bind`] for a function without variadic parameter: one value per parameter.
fn bind_n<const N: usize>(
    call: &mut Call<'_>,
    function: &str,
    injected: usize,
    params: [Param; N],
) -> CallResult<[Value; N]> {
    let values = bind(call, function, injected, &params)?;
    Ok(<[Value; N]>::try_from(values)
        .unwrap_or_else(|_| unreachable!("binding returns one value per parameter")))
}

/// PHP's `.` over several operands, left to right.
fn concat(parts: &[&Value], diag: &mut dyn Diagnostics) -> PhpResult<Value> {
    let mut out = Value::String(PhpString::empty());
    for part in parts {
        out = Value::String(ops::concat(&out, part, diag)?);
    }
    Ok(out)
}

fn text(s: &str) -> Value {
    Value::from(s)
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

/// `TwigTestExtension::§Filter($value)` / `§Function($value)`: `"§{$value}§"`.
fn section(function: &'static str) -> impl Fn(&mut Call<'_>) -> CallResult + Send + Sync {
    move |call| {
        let [value] = bind_n(call, function, 0, [Param::required("value")])?;
        Ok(concat(
            &[&text("§"), &value, &text("§")],
            call.diagnostics(),
        )?)
    }
}

/// `TwigTestExtension::nl2br($value, $sep = '<br />')`: `str_replace("\n", "$sep\n", $value ?? '')`.
fn nl2br(value: Value, sep: &Value, diag: &mut dyn Diagnostics) -> PhpResult<Value> {
    let replace = concat(&[sep, &text("\n")], diag)?;
    let subject = if value.is_null() { text("") } else { value };
    str_replace(&text("\n"), &replace, &subject, diag)
}

/// `$env->getRuntime(EscaperRuntime::class)->escape($value, 'html')` (TWIG/src/Runtime/EscaperRuntime.php, with
/// `$autoescape = false` and no charset): the environment's escaper runtime.
fn escape_html(env: &Environment, value: Value, diag: &mut dyn Diagnostics) -> CallResult {
    env.escaper_runtime()
        .escape(value, b"html", None, false, diag)
}

/// `TwigTestExtension::__call($method, $arguments)` (for `magicCall`) and `__callStatic()` (for `magicStaticCall`):
/// `$prefix.$arguments[0]`.
fn magic(prefix: &'static str) -> impl Fn(&mut Call<'_>) -> CallResult + Send + Sync {
    move |call| {
        let args = call.take_args().to_array();
        let first = match args.get_int(0) {
            Some(first) => first.clone(),
            None => {
                call.diagnostics().warn("Undefined array key 0")?;
                Value::Null
            }
        };
        Ok(concat(&[&text(prefix), &first], call.diagnostics())?)
    }
}

/// `TwigTestExtension::staticCall($value)`: `"*$value*"`.
fn static_call(call: &mut Call<'_>) -> CallResult {
    let [value] = bind_n(
        call,
        "Twig\\Tests\\TwigTestExtension::staticCall",
        0,
        [Param::required("value")],
    )?;
    Ok(concat(
        &[&text("*"), &value, &text("*")],
        call.diagnostics(),
    )?)
}

/// `TwigTestExtension::dynamic_path($element, $item)`: `$element.'/'.$item`.
fn dynamic_path(call: &mut Call<'_>) -> CallResult {
    let [element, item] = bind_n(
        call,
        "Twig\\Tests\\TwigTestExtension::dynamic_path",
        0,
        [Param::required("element"), Param::required("item")],
    )?;
    Ok(concat(&[&element, &text("/"), &item], call.diagnostics())?)
}

/// `TwigTestExtension::dynamic_foo($foo, $bar, $item)`: `$foo.'/'.$bar.'/'.$item`.
fn dynamic_foo(call: &mut Call<'_>) -> CallResult {
    let [foo, bar, item] = bind_n(
        call,
        "Twig\\Tests\\TwigTestExtension::dynamic_foo",
        0,
        [
            Param::required("foo"),
            Param::required("bar"),
            Param::required("item"),
        ],
    )?;
    Ok(concat(
        &[&foo, &text("/"), &bar, &text("/"), &item],
        call.diagnostics(),
    )?)
}

/// `static function ($name) { return '*'.$name.'*'; }` (the `anon_foo` filter and function, declared on lines 259
/// and 273 of IntegrationTest.php). `name` is the closure's name in PHP's messages: its scope, then
/// `{closure:<enclosing method>:<line>}` (PHP 8.4+, checked with the oracle).
fn anon_foo(name: &'static str) -> impl Fn(&mut Call<'_>) -> CallResult + Send + Sync {
    move |call| {
        let [value] = bind_n(call, name, 0, [Param::required("name")])?;
        Ok(concat(
            &[&text("*"), &value, &text("*")],
            call.diagnostics(),
        )?)
    }
}

/// `TwigTestExtension::escape_something($value)` / `preserves_safety($value)`: `strtoupper($value)`.
fn upper(function: &'static str) -> impl Fn(&mut Call<'_>) -> CallResult + Send + Sync {
    move |call| {
        let [value] = bind_n(call, function, 0, [Param::required("value")])?;
        Ok(Value::String(strtoupper(&value, call.diagnostics())?))
    }
}

/// `Twig\Tests\TwigTestExtension`.
pub struct TwigTestExtension;

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
        // `[$this, 'magicCall']`, `\Closure::fromCallable([$this, 'magicCall'])` and `'...::magicStaticCall'` go
        // through __call / __callStatic: the trampoline closures reflect no parameter
        let magic_call = || method("magicCall", Vec::new(), magic("magic_"));
        let magic_static_call = || method("magicStaticCall", Vec::new(), magic("static_magic_"));
        vec![
            TwigFilter::new(
                "§",
                method(
                    "§Filter",
                    value(),
                    section("Twig\\Tests\\TwigTestExtension::§Filter"),
                ),
                d(),
            ),
            TwigFilter::new(
                "escape_and_nl2br",
                method(
                    "escape_and_nl2br",
                    vec![P::required("env"), P::required("value"), sep()],
                    |call| {
                        // `$this->nl2br($env->getRuntime(EscaperRuntime::class)->escape($value, 'html'), $sep)`
                        let [_env, value, sep] = bind_n(
                            call,
                            "Twig\\Tests\\TwigTestExtension::escape_and_nl2br",
                            1,
                            [
                                Param::required("env"),
                                Param::required("value"),
                                Param::optional("sep", "<br />"),
                            ],
                        )?;
                        let env = call.env().clone();
                        let escaped = escape_html(&env, value, call.diagnostics())?;
                        Ok(nl2br(escaped, &sep, call.diagnostics())?)
                    },
                ),
                FilterOptions {
                    needs_environment: true,
                    is_safe: safe(&["html"]),
                    ..d()
                },
            ),
            TwigFilter::new(
                "nl2br",
                method("nl2br", vec![P::required("value"), sep()], |call| {
                    let [value, sep] = bind_n(
                        call,
                        "Twig\\Tests\\TwigTestExtension::nl2br",
                        0,
                        [Param::required("value"), Param::optional("sep", "<br />")],
                    )?;
                    Ok(nl2br(value, &sep, call.diagnostics())?)
                }),
                FilterOptions {
                    pre_escape: Some("html".to_owned()),
                    is_safe: safe(&["html"]),
                    ..d()
                },
            ),
            TwigFilter::new(
                "escape_something",
                method(
                    "escape_something",
                    value(),
                    upper("Twig\\Tests\\TwigTestExtension::escape_something"),
                ),
                FilterOptions {
                    is_safe: safe(&["something"]),
                    ..d()
                },
            ),
            TwigFilter::new(
                "preserves_safety",
                method(
                    "preserves_safety",
                    value(),
                    upper("Twig\\Tests\\TwigTestExtension::preserves_safety"),
                ),
                FilterOptions {
                    preserves_safety: safe(&["html"]),
                    ..d()
                },
            ),
            TwigFilter::new(
                "static_call_string",
                method("staticCall", value(), static_call),
                d(),
            ),
            TwigFilter::new(
                "static_call_array",
                method("staticCall", value(), static_call),
                d(),
            ),
            TwigFilter::new("magic_call", magic_call(), d()),
            TwigFilter::new("magic_call_closure", magic_call(), d()),
            TwigFilter::new("magic_call_string", magic_static_call(), d()),
            TwigFilter::new("magic_call_array", magic_static_call(), d()),
            TwigFilter::new(
                "*_path",
                method(
                    "dynamic_path",
                    vec![P::required("element"), P::required("item")],
                    dynamic_path,
                ),
                d(),
            ),
            TwigFilter::new(
                "*_foo_*_bar",
                method(
                    "dynamic_foo",
                    vec![P::required("foo"), P::required("bar"), P::required("item")],
                    dynamic_foo,
                ),
                d(),
            ),
            TwigFilter::new(
                "not",
                method("notFilter", value(), |call| {
                    // `'not '.$value`
                    let [value] = bind_n(
                        call,
                        "Twig\\Tests\\TwigTestExtension::notFilter",
                        0,
                        [Param::required("value")],
                    )?;
                    Ok(concat(&[&text("not "), &value], call.diagnostics())?)
                }),
                d(),
            ),
            TwigFilter::new(
                "anon_foo",
                closure(
                    vec![P::required("name")],
                    anon_foo(
                        "Twig\\Tests\\TwigTestExtension::{closure:Twig\\Tests\\TwigTestExtension::getFilters():259}",
                    ),
                ),
                d(),
            ),
        ]
    }

    fn functions(&self) -> Vec<TwigFunction> {
        let d = FunctionOptions::default;
        // `TwigTestExtension::br()`: `'<br />'`
        let br = || method("br", Vec::new(), |_call| Ok(text("<br />")));
        vec![
            TwigFunction::new(
                "§",
                method(
                    "§Function",
                    value(),
                    section("Twig\\Tests\\TwigTestExtension::§Function"),
                ),
                d(),
            ),
            TwigFunction::new(
                "safe_br",
                br(),
                FunctionOptions {
                    is_safe: safe(&["html"]),
                    ..d()
                },
            ),
            TwigFunction::new("unsafe_br", br(), d()),
            TwigFunction::new(
                "static_call_string",
                method("staticCall", value(), static_call),
                d(),
            ),
            TwigFunction::new(
                "static_call_array",
                method("staticCall", value(), static_call),
                d(),
            ),
            TwigFunction::new(
                "*_path",
                method(
                    "dynamic_path",
                    vec![P::required("element"), P::required("item")],
                    dynamic_path,
                ),
                d(),
            ),
            TwigFunction::new(
                "*_foo_*_bar",
                method(
                    "dynamic_foo",
                    vec![P::required("foo"), P::required("bar"), P::required("item")],
                    dynamic_foo,
                ),
                d(),
            ),
            TwigFunction::new(
                "anon_foo",
                closure(
                    vec![P::required("name")],
                    anon_foo(
                        "Twig\\Tests\\TwigTestExtension::{closure:Twig\\Tests\\TwigTestExtension::getFunctions():273}",
                    ),
                ),
                d(),
            ),
            TwigFunction::new(
                "deprecated_function",
                // `static function () { return 'foo'; }`
                closure(Vec::new(), |_call| Ok(text("foo"))),
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
            TwigTest::new(
                "multi word",
                method("is_multi_word", value(), |call| {
                    // `str_contains($value, ' ')`
                    let [value] = bind_n(
                        call,
                        "Twig\\Tests\\TwigTestExtension::is_multi_word",
                        0,
                        [Param::required("value")],
                    )?;
                    Ok(Value::Bool(str_contains(
                        &value,
                        &text(" "),
                        call.diagnostics(),
                    )?))
                }),
                d(),
            ),
            TwigTest::new(
                "test_*",
                method(
                    "dynamic_test",
                    vec![P::required("element"), P::required("item")],
                    |call| {
                        // `$element === $item`
                        let [element, item] = bind_n(
                            call,
                            "Twig\\Tests\\TwigTestExtension::dynamic_test",
                            0,
                            [Param::required("element"), Param::required("item")],
                        )?;
                        Ok(Value::Bool(compare::identical(&element, &item)))
                    },
                ),
                d(),
            ),
        ]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// The state of `TwigTestExtensionWithAttributes` and `TwigTestLazyExtensionWithAttributes` (`private string
/// $wrapper`), the runtime object their callables are called on.
#[derive(Debug)]
pub struct AttributeRuntime {
    wrapper: PhpString,
}

impl AttributeRuntime {
    /// `new TwigTestExtensionWithAttributes($wrapper)` / `new TwigTestLazyExtensionWithAttributes($wrapper)`.
    pub fn new(wrapper: &str) -> AttributeRuntime {
        AttributeRuntime {
            wrapper: PhpString::from(wrapper),
        }
    }
}

/// The attribute-declared callables of a class: `TwigTestExtensionWithAttributes('*')` (`attribute_*`, called on
/// the extension, its own runtime) and `AttributeExtension(TwigTestLazyExtensionWithAttributes::class)`
/// (`lazy_attribute_*`, called on the instance the runtime loader creates). `AttributeExtension` builds them from
/// `[$class, $method]` callables, which compile to `$this->env->getRuntime($class)->method(...)`.
pub struct AttributeCallables {
    /// The extension class (`Twig\Tests\TwigTestExtensionWithAttributes` or `Twig\Extension\AttributeExtension`).
    pub extension: &'static str,
    /// The class declaring the methods, whose runtime the callables are called on.
    pub runtime: &'static str,
    /// The prefix of the callable names (`""` or `lazy_`).
    pub prefix: &'static str,
}

impl AttributeCallables {
    /// `public function $method(string $value)`: the runtime of `$this->env->getRuntime($class)->$method($value)`,
    /// with the coercion of the `string` parameter; `body` computes the result from the wrapper and the value.
    fn callable(
        &self,
        method: &'static str,
        body: fn(&PhpString, &PhpString) -> Value,
    ) -> Option<PhpCallable> {
        let class = self.runtime;
        with_runtime(PhpCallable::method(class, method, value()), move |call| {
            let runtime = call.env().runtime(class)?;
            let Some(runtime) = runtime.downcast_ref::<AttributeRuntime>() else {
                return Err(Error::runtime(format!(
                    "The \"{class}\" runtime is not an attribute extension."
                ))
                .into());
            };
            let function = format!("{class}::{method}");
            let [value] = bind_n(call, &function, 0, [Param::required("value")])?;
            let value = verify_user_arg(
                value,
                &DeclaredType::builtin(TypeMask::STRING),
                &UserArg::new(&function, 1, "value"),
                call.diagnostics(),
            )?;
            let Value::String(value) = value else {
                unreachable!("a value accepted by the string type is a string")
            };
            Ok(body(&runtime.wrapper, &value))
        })
    }
}

/// `$this->wrapper.$value.$this->wrapper`.
fn wrap(wrapper: &PhpString, value: &PhpString) -> Value {
    Value::String(PhpString::concat(&[
        wrapper.as_bytes(),
        value.as_bytes(),
        wrapper.as_bytes(),
    ]))
}

/// `$this->wrapper === $value`.
fn is_wrapper(wrapper: &PhpString, value: &PhpString) -> Value {
    Value::Bool(wrapper == value)
}

impl Extension for AttributeCallables {
    fn class_name(&self) -> &str {
        self.extension
    }

    fn filters(&self) -> Vec<TwigFilter> {
        vec![TwigFilter::new(
            format!("{}attribute_filter", self.prefix),
            self.callable("attributeFilter", wrap),
            FilterOptions::default(),
        )]
    }

    fn functions(&self) -> Vec<TwigFunction> {
        vec![TwigFunction::new(
            format!("{}attribute_function", self.prefix),
            self.callable("attributeFunction", wrap),
            FunctionOptions::default(),
        )]
    }

    fn tests(&self) -> Vec<TwigTest> {
        vec![TwigTest::new(
            format!("{}attribute_test", self.prefix),
            self.callable("attributeTest", is_wrapper),
            TestOptions::default(),
        )]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// `getUndefinedFilterCallbacks()`.
pub fn undefined_filter_callback() -> UndefinedFilterCallback {
    Arc::new(|name: &[u8]| {
        if name == b"throwing_undefined_filter" {
            return Err(Error::syntax("This filter is undefined in the tests."));
        }
        Ok(None)
    })
}

/// `getUndefinedFunctionCallbacks()`.
pub fn undefined_function_callback() -> UndefinedFunctionCallback {
    Arc::new(|name: &[u8]| {
        if name == b"throwing_undefined_function" {
            return Err(Error::syntax("This function is undefined in the tests."));
        }
        Ok(None)
    })
}

/// `getUndefinedTestCallbacks()`.
pub fn undefined_test_callback() -> UndefinedTestCallback {
    Arc::new(|name: &[u8]| {
        if name == b"throwing_undefined_test" || name == b"throwing_undefined_two words_test" {
            return Err(Error::syntax("This test is undefined in the tests."));
        }
        // "Ensure this does not conflict with `divisible by` and `same as`."
        if name == b"divisible" || name == b"same" {
            // `new TwigTest($name, static fn () => '')`
            return Ok(Some(TwigTest::new(
                name.to_vec(),
                closure(Vec::new(), |_call| Ok(text(""))),
                TestOptions::default(),
            )));
        }
        Ok(None)
    })
}
