//! The metadata of `Twig\Extension\CoreExtension` and `Twig\Extension\EscaperExtension`
//! (TWIG/src/Extension/CoreExtension.php, TWIG/src/Extension/EscaperExtension.php).
//!
//! Every filter, function and test with its options and the full PHP parameter list of its callable (dumped from
//! the oracle with `Twig\Util\ReflectionCallable`), every operator, the core tags, the `CorrectnessNodeVisitor`
//! and the escaper's default strategy; runtime implementations fill the callables' runtime slots later.

use std::any::Any;
use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::sync::{Arc, Mutex, PoisonError, RwLock};

use super::callable::{
    CallableParameter as P, DeprecatedCallableInfo, FilterNodeClass, FilterOptions,
    FunctionNodeClass, FunctionOptions, PhpCallable, TestNodeClass, TestOptions, TwigFilter,
    TwigFunction, TwigTest,
};
use super::{Extension, NodeVisitor, TokenParser};
use crate::ast::{Arguments, BinaryOp, ConstantValue as V, ExprKind, UnaryOp};
use crate::error::Result;
use crate::escaper::{self, DefaultStrategy, DefaultStrategyCallback};
use crate::parser::callables;
use crate::parser::expression::infix::{
    ArrowExpressionParser, AssignmentExpressionParser, BinaryOperatorExpressionParser,
    ConditionalTernaryExpressionParser, DotExpressionParser, FilterExpressionParser,
    FunctionExpressionParser, IsExpressionParser, SquareBracketExpressionParser,
};
use crate::parser::expression::prefix::{
    GroupingExpressionParser, LiteralExpressionParser, SpreadExpressionParser,
    UnaryOperatorExpressionParser,
};
use crate::parser::expression::{ExpressionParserEntry, InfixAssociativity, PrecedenceChange};
use crate::token_parser::{AutoEscapeTokenParser, core_token_parsers};
use crate::visitor::CorrectnessNodeVisitor;

const CORE: &str = "Twig\\Extension\\CoreExtension";

fn core_method(method: &str, parameters: Vec<P>) -> Option<PhpCallable> {
    Some(PhpCallable::method(CORE, method, parameters))
}

fn null() -> V {
    V::Null
}

/// `Twig\Extension\CoreExtension`: the core filters, functions, tests and operators.
#[derive(Debug, Default)]
pub struct CoreExtension {
    _private: (),
}

impl CoreExtension {
    /// The core extension.
    pub fn new() -> Self {
        CoreExtension::default()
    }
}

impl Extension for CoreExtension {
    fn class_name(&self) -> &str {
        CORE
    }

    /// Twig: `CoreExtension::getTokenParsers()`.
    fn token_parsers(&self) -> Vec<Arc<dyn TokenParser>> {
        core_token_parsers()
    }

    /// Twig: `CoreExtension::getNodeVisitors()`.
    fn node_visitors(&self) -> Vec<Arc<dyn NodeVisitor>> {
        vec![Arc::new(CorrectnessNodeVisitor)]
    }

    /// Twig: `CoreExtension::getFilters()`.
    fn filters(&self) -> Vec<TwigFilter> {
        let charset = || FilterOptions {
            needs_charset: true,
            ..Default::default()
        };
        let env_sandboxed = || FilterOptions {
            needs_environment: true,
            needs_is_sandboxed: true,
            ..Default::default()
        };
        let d = FilterOptions::default;
        vec![
            // formatting filters
            TwigFilter::new(
                "date",
                core_method(
                    "formatDate",
                    vec![
                        P::required("date"),
                        P::optional("format", null()),
                        P::optional("timezone", null()),
                    ],
                ),
                d(),
            ),
            TwigFilter::new(
                "date_modify",
                core_method(
                    "modifyDate",
                    vec![P::required("date"), P::required("modifier")],
                ),
                d(),
            ),
            TwigFilter::new(
                "format",
                core_method(
                    "sprintf",
                    vec![P::required("format"), P::variadic("values")],
                ),
                d(),
            ),
            TwigFilter::new(
                "replace",
                core_method("replace", vec![P::required("str"), P::required("from")]),
                d(),
            ),
            TwigFilter::new(
                "number_format",
                core_method(
                    "formatNumber",
                    vec![
                        P::required("number"),
                        P::optional("decimal", null()),
                        P::optional("decimalPoint", null()),
                        P::optional("thousandSep", null()),
                    ],
                ),
                d(),
            ),
            TwigFilter::new(
                "abs",
                Some(PhpCallable::function("abs", vec![P::required("num")])),
                d(),
            ),
            TwigFilter::new(
                "round",
                core_method(
                    "round",
                    vec![
                        P::required("value"),
                        P::optional("precision", V::Int(0)),
                        P::optional("method", "common".into()),
                    ],
                ),
                d(),
            ),
            // encoding
            TwigFilter::new(
                "url_encode",
                core_method("urlencode", vec![P::required("url")]),
                d(),
            ),
            TwigFilter::new(
                "json_encode",
                Some(PhpCallable::function(
                    "json_encode",
                    vec![
                        P::required("value"),
                        P::optional("flags", V::Int(0)),
                        P::optional("depth", V::Int(512)),
                    ],
                )),
                d(),
            ),
            TwigFilter::new(
                "convert_encoding",
                core_method(
                    "convertEncoding",
                    vec![
                        P::required("string"),
                        P::required("to"),
                        P::required("from"),
                    ],
                ),
                d(),
            ),
            // string filters
            TwigFilter::new(
                "title",
                core_method(
                    "titleCase",
                    vec![P::required("charset"), P::required("string")],
                ),
                charset(),
            ),
            TwigFilter::new(
                "capitalize",
                core_method(
                    "capitalize",
                    vec![P::required("charset"), P::required("string")],
                ),
                charset(),
            ),
            TwigFilter::new(
                "upper",
                core_method("upper", vec![P::required("charset"), P::required("string")]),
                charset(),
            ),
            TwigFilter::new(
                "lower",
                core_method("lower", vec![P::required("charset"), P::required("string")]),
                charset(),
            ),
            TwigFilter::new(
                "striptags",
                core_method(
                    "striptags",
                    vec![P::required("string"), P::optional("allowable_tags", null())],
                ),
                d(),
            ),
            TwigFilter::new(
                "trim",
                core_method(
                    "trim",
                    vec![
                        P::required("string"),
                        P::optional("characterMask", null()),
                        P::optional("side", "both".into()),
                    ],
                ),
                d(),
            ),
            TwigFilter::new(
                "nl2br",
                core_method("nl2br", vec![P::required("string")]),
                FilterOptions {
                    pre_escape: Some("html".to_owned()),
                    is_safe: Some(vec!["html".into()]),
                    ..Default::default()
                },
            ),
            TwigFilter::new(
                "spaceless",
                core_method("spaceless", vec![P::required("content")]),
                FilterOptions {
                    pre_escape: Some("html".to_owned()),
                    is_safe: Some(vec!["html".into()]),
                    deprecation_info: Some(DeprecatedCallableInfo::new("twig/twig", "3.12")),
                    ..Default::default()
                },
            ),
            // array helpers
            TwigFilter::new(
                "join",
                core_method(
                    "join",
                    vec![
                        P::required("value"),
                        P::optional("glue", "".into()),
                        P::optional("and", null()),
                    ],
                ),
                d(),
            ),
            TwigFilter::new(
                "split",
                core_method(
                    "split",
                    vec![
                        P::required("charset"),
                        P::required("value"),
                        P::required("delimiter"),
                        P::optional("limit", null()),
                    ],
                ),
                charset(),
            ),
            TwigFilter::new(
                "sort",
                core_method(
                    "sort",
                    vec![
                        P::required("env"),
                        P::required("isSandboxed"),
                        P::required("array"),
                        P::optional("arrow", null()),
                    ],
                ),
                env_sandboxed(),
            ),
            TwigFilter::new(
                "merge",
                core_method("merge", vec![P::variadic("arrays")]),
                d(),
            ),
            TwigFilter::new(
                "batch",
                core_method(
                    "batch",
                    vec![
                        P::required("items"),
                        P::required("size"),
                        P::optional("fill", null()),
                        P::optional("preserveKeys", V::Bool(true)),
                    ],
                ),
                d(),
            ),
            TwigFilter::new(
                "column",
                core_method(
                    "column",
                    vec![
                        P::required("env"),
                        P::required("isSandboxed"),
                        P::required("array"),
                        P::required("name"),
                        P::optional("index", null()),
                    ],
                ),
                env_sandboxed(),
            ),
            TwigFilter::new(
                "filter",
                core_method(
                    "filter",
                    vec![
                        P::required("env"),
                        P::required("isSandboxed"),
                        P::required("array"),
                        P::required("arrow"),
                    ],
                ),
                env_sandboxed(),
            ),
            TwigFilter::new(
                "map",
                core_method(
                    "map",
                    vec![
                        P::required("env"),
                        P::required("isSandboxed"),
                        P::required("array"),
                        P::required("arrow"),
                    ],
                ),
                env_sandboxed(),
            ),
            TwigFilter::new(
                "reduce",
                core_method(
                    "reduce",
                    vec![
                        P::required("env"),
                        P::required("isSandboxed"),
                        P::required("array"),
                        P::required("arrow"),
                        P::optional("initial", null()),
                    ],
                ),
                env_sandboxed(),
            ),
            TwigFilter::new(
                "find",
                core_method(
                    "find",
                    vec![
                        P::required("env"),
                        P::required("isSandboxed"),
                        P::required("array"),
                        P::required("arrow"),
                    ],
                ),
                env_sandboxed(),
            ),
            // string/array filters
            TwigFilter::new(
                "reverse",
                core_method(
                    "reverse",
                    vec![
                        P::required("charset"),
                        P::required("item"),
                        P::optional("preserveKeys", V::Bool(false)),
                    ],
                ),
                charset(),
            ),
            TwigFilter::new(
                "shuffle",
                core_method("shuffle", vec![P::required("charset"), P::required("item")]),
                charset(),
            ),
            TwigFilter::new(
                "length",
                core_method("length", vec![P::required("charset"), P::required("thing")]),
                charset(),
            ),
            TwigFilter::new(
                "slice",
                core_method(
                    "slice",
                    vec![
                        P::required("charset"),
                        P::required("item"),
                        P::required("start"),
                        P::optional("length", null()),
                        P::optional("preserveKeys", V::Bool(false)),
                    ],
                ),
                charset(),
            ),
            TwigFilter::new(
                "first",
                core_method("first", vec![P::required("charset"), P::required("item")]),
                charset(),
            ),
            TwigFilter::new(
                "last",
                core_method("last", vec![P::required("charset"), P::required("item")]),
                charset(),
            ),
            // iteration and runtime
            TwigFilter::new(
                "default",
                core_method(
                    "default",
                    vec![P::required("value"), P::optional("default", "".into())],
                ),
                FilterOptions {
                    node_class: FilterNodeClass::Default,
                    ..Default::default()
                },
            ),
            TwigFilter::new("keys", core_method("keys", vec![P::required("array")]), d()),
            TwigFilter::new(
                "invoke",
                core_method(
                    "invoke",
                    vec![P::required("arrow"), P::variadic("arguments")],
                ),
                d(),
            ),
        ]
    }

    /// Twig: `CoreExtension::getFunctions()`.
    fn functions(&self) -> Vec<TwigFunction> {
        let d = FunctionOptions::default;
        let parser_callable = |callable: super::callable::ParserCallable| FunctionOptions {
            parser_callable: Some(callable),
            ..Default::default()
        };
        vec![
            TwigFunction::new(
                "parent",
                None,
                parser_callable(Arc::new(callables::parse_parent_function)),
            ),
            TwigFunction::new(
                "block",
                None,
                parser_callable(Arc::new(callables::parse_block_function)),
            ),
            TwigFunction::new(
                "attribute",
                None,
                parser_callable(Arc::new(callables::parse_attribute_function)),
            ),
            TwigFunction::new(
                "max",
                Some(PhpCallable::function(
                    "max",
                    vec![P::required("value"), P::variadic("values")],
                )),
                d(),
            ),
            TwigFunction::new(
                "min",
                Some(PhpCallable::function(
                    "min",
                    vec![P::required("value"), P::variadic("values")],
                )),
                d(),
            ),
            TwigFunction::new(
                "range",
                Some(PhpCallable::function(
                    "range",
                    vec![
                        P::required("start"),
                        P::required("end"),
                        P::optional("step", V::Int(1)),
                    ],
                )),
                d(),
            ),
            TwigFunction::new(
                "constant",
                core_method(
                    "constant",
                    vec![
                        P::required("constant"),
                        P::optional("object", null()),
                        P::optional("checkDefined", V::Bool(false)),
                    ],
                ),
                d(),
            ),
            TwigFunction::new(
                "cycle",
                core_method(
                    "cycle",
                    vec![P::required("values"), P::required("position")],
                ),
                d(),
            ),
            TwigFunction::new(
                "random",
                core_method(
                    "random",
                    vec![
                        P::required("charset"),
                        P::optional("values", null()),
                        P::optional("max", null()),
                    ],
                ),
                FunctionOptions {
                    needs_charset: true,
                    ..Default::default()
                },
            ),
            TwigFunction::new(
                "date",
                core_method(
                    "convertDate",
                    vec![P::optional("date", null()), P::optional("timezone", null())],
                ),
                d(),
            ),
            TwigFunction::new(
                "include",
                core_method(
                    "include",
                    vec![
                        P::required("env"),
                        P::required("context"),
                        P::required("template"),
                        P::optional("variables", V::Array(Vec::new())),
                        P::optional("withContext", V::Bool(true)),
                        P::optional("ignoreMissing", V::Bool(false)),
                        P::optional("sandboxed", V::Bool(false)),
                    ],
                ),
                FunctionOptions {
                    needs_environment: true,
                    needs_context: true,
                    is_safe: Some(vec!["all".into()]),
                    ..Default::default()
                },
            ),
            TwigFunction::new(
                "include_only",
                core_method(
                    "includeOnly",
                    vec![
                        P::required("env"),
                        P::required("template"),
                        P::optional("variables", V::Array(Vec::new())).array(),
                        P::optional("ignoreMissing", V::Bool(false)),
                    ],
                ),
                FunctionOptions {
                    needs_environment: true,
                    is_safe: Some(vec!["all".into()]),
                    ..Default::default()
                },
            ),
            TwigFunction::new(
                "source",
                core_method(
                    "source",
                    vec![
                        P::required("env"),
                        P::required("name"),
                        P::optional("ignoreMissing", V::Bool(false)),
                    ],
                ),
                FunctionOptions {
                    needs_environment: true,
                    is_safe: Some(vec!["all".into()]),
                    ..Default::default()
                },
            ),
            TwigFunction::new(
                "enum_cases",
                core_method("enumCases", vec![P::required("enum")]),
                FunctionOptions {
                    node_class: FunctionNodeClass::EnumCases,
                    ..Default::default()
                },
            ),
            TwigFunction::new(
                "enum",
                core_method("enum", vec![P::required("enum")]),
                FunctionOptions {
                    node_class: FunctionNodeClass::Enum,
                    ..Default::default()
                },
            ),
        ]
    }

    /// Twig: `CoreExtension::getTests()`.
    fn tests(&self) -> Vec<TwigTest> {
        let class = |node_class: TestNodeClass| TestOptions {
            node_class,
            always_allowed_in_sandbox: true,
            ..Default::default()
        };
        let mandatory = |node_class: TestNodeClass| TestOptions {
            node_class,
            one_mandatory_argument: true,
            always_allowed_in_sandbox: true,
            ..Default::default()
        };
        vec![
            TwigTest::new("even", None, class(TestNodeClass::Even)),
            TwigTest::new("odd", None, class(TestNodeClass::Odd)),
            TwigTest::new("defined", None, class(TestNodeClass::Defined)),
            TwigTest::new("same as", None, mandatory(TestNodeClass::SameAs)),
            TwigTest::new("none", None, class(TestNodeClass::Null)),
            TwigTest::new("null", None, class(TestNodeClass::Null)),
            TwigTest::new("divisible by", None, mandatory(TestNodeClass::DivisibleBy)),
            TwigTest::new(
                "constant",
                None,
                TestOptions {
                    node_class: TestNodeClass::Constant,
                    ..Default::default()
                },
            ),
            TwigTest::new(
                "empty",
                core_method("testEmpty", vec![P::required("value")]),
                class(TestNodeClass::Test),
            ),
            TwigTest::new(
                "iterable",
                Some(PhpCallable::function(
                    "is_iterable",
                    vec![P::required("value")],
                )),
                class(TestNodeClass::NonStringCoercingInput),
            ),
            TwigTest::new(
                "sequence",
                core_method("testSequence", vec![P::required("value")]),
                class(TestNodeClass::NonStringCoercingInput),
            ),
            TwigTest::new(
                "mapping",
                core_method("testMapping", vec![P::required("value")]),
                class(TestNodeClass::NonStringCoercingInput),
            ),
            TwigTest::new("true", None, class(TestNodeClass::True)),
        ]
    }

    /// Twig: `CoreExtension::getExpressionParsers()`.
    fn expression_parsers(&self) -> Vec<ExpressionParserEntry> {
        use InfixAssociativity::{Left, Right};
        let binary = |op: BinaryOp, name: &str, precedence: i64| {
            ExpressionParserEntry::infix(BinaryOperatorExpressionParser::core(
                op, name, precedence, Left,
            ))
        };
        vec![
            // unary operators
            ExpressionParserEntry::prefix(
                UnaryOperatorExpressionParser::core(UnaryOp::Not, "not", 50)
                    .with_precedence_change(PrecedenceChange::new("twig/twig", "3.15", 70)),
            ),
            ExpressionParserEntry::prefix(SpreadExpressionParser),
            ExpressionParserEntry::prefix(UnaryOperatorExpressionParser::core(
                UnaryOp::Neg,
                "-",
                500,
            )),
            ExpressionParserEntry::prefix(UnaryOperatorExpressionParser::core(
                UnaryOp::Pos,
                "+",
                500,
            )),
            // binary operators
            ExpressionParserEntry::infix(
                BinaryOperatorExpressionParser::core(BinaryOp::Elvis, "?:", 5, Right)
                    .with_description("Elvis operator (a ?: b)")
                    .with_aliases(&["? :"]),
            ),
            ExpressionParserEntry::infix(
                BinaryOperatorExpressionParser::core(BinaryOp::NullCoalesce, "??", 300, Right)
                    .with_precedence_change(PrecedenceChange::new("twig/twig", "3.15", 5))
                    .with_description("Null coalescing operator (a ?? b)"),
            ),
            binary(BinaryOp::Or, "or", 10),
            binary(BinaryOp::Xor, "xor", 12),
            binary(BinaryOp::And, "and", 15),
            binary(BinaryOp::BitwiseOr, "b-or", 16),
            binary(BinaryOp::BitwiseXor, "b-xor", 17),
            binary(BinaryOp::BitwiseAnd, "b-and", 18),
            binary(BinaryOp::Equal, "==", 20),
            binary(BinaryOp::NotEqual, "!=", 20),
            binary(BinaryOp::Spaceship, "<=>", 20),
            binary(BinaryOp::Less, "<", 20),
            binary(BinaryOp::Greater, ">", 20),
            binary(BinaryOp::GreaterEqual, ">=", 20),
            binary(BinaryOp::LessEqual, "<=", 20),
            binary(BinaryOp::NotIn, "not in", 20),
            binary(BinaryOp::In, "in", 20),
            binary(BinaryOp::Matches, "matches", 20),
            binary(BinaryOp::StartsWith, "starts with", 20),
            binary(BinaryOp::EndsWith, "ends with", 20),
            binary(BinaryOp::HasSome, "has some", 20),
            binary(BinaryOp::HasEvery, "has every", 20),
            binary(BinaryOp::SameAs, "===", 20),
            binary(BinaryOp::NotSameAs, "!==", 20),
            binary(BinaryOp::Range, "..", 25),
            binary(BinaryOp::Add, "+", 30),
            binary(BinaryOp::Sub, "-", 30),
            ExpressionParserEntry::infix(
                BinaryOperatorExpressionParser::core(BinaryOp::Concat, "~", 40, Left)
                    .with_precedence_change(PrecedenceChange::new("twig/twig", "3.15", 27)),
            ),
            binary(BinaryOp::Mul, "*", 60),
            binary(BinaryOp::Div, "/", 60),
            ExpressionParserEntry::infix(
                BinaryOperatorExpressionParser::core(BinaryOp::FloorDiv, "//", 60, Left)
                    .with_description("Floor division"),
            ),
            binary(BinaryOp::Mod, "%", 60),
            ExpressionParserEntry::infix(
                BinaryOperatorExpressionParser::core(BinaryOp::Power, "**", 200, Right)
                    .with_description("Exponentiation operator"),
            ),
            // ternary operator
            ExpressionParserEntry::infix(ConditionalTernaryExpressionParser),
            // assignment operator
            ExpressionParserEntry::infix(AssignmentExpressionParser::new("=")),
            // Twig callables
            ExpressionParserEntry::infix(IsExpressionParser::is()),
            ExpressionParserEntry::infix(IsExpressionParser::is_not()),
            ExpressionParserEntry::infix(FilterExpressionParser),
            ExpressionParserEntry::infix(FunctionExpressionParser),
            // get attribute operators
            ExpressionParserEntry::infix(DotExpressionParser),
            ExpressionParserEntry::infix(SquareBracketExpressionParser),
            // group expression
            ExpressionParserEntry::prefix(GroupingExpressionParser),
            // arrow function
            ExpressionParserEntry::infix(ArrowExpressionParser),
            // all literals
            ExpressionParserEntry::prefix(LiteralExpressionParser),
        ]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// `Twig\Extension\EscaperExtension`: the `autoescape` tag, the `escape`, `e` and `raw` filters and the default
/// escaping strategy of templates, which the [`EscaperNodeVisitor`](crate::visitor::EscaperNodeVisitor) reads.
///
/// The default strategy can be changed at any time ([`set_default_strategy`](Self::set_default_strategy)); it is
/// resolved per template name when a template is compiled ([`default_strategy`](Self::default_strategy)).
///
/// Twig's extension also registers the `EscaperNodeVisitor` (`getNodeVisitors()`). The parser stage's extension set
/// ([`ExtensionSet::with_core_extensions`](super::ExtensionSet::with_core_extensions)) does not run it yet; until
/// then the visitor is added to an extension set explicitly.
pub struct EscaperExtension {
    /// Twig's `$defaultStrategy`.
    default_strategy: RwLock<ConfiguredStrategy>,
    /// Twig's `$resolvedDefaultStrategies`.
    resolved: Mutex<ResolvedStrategies>,
}

/// Twig: `EscaperExtension::MAX_RESOLVED_DEFAULT_STRATEGIES`.
const MAX_RESOLVED_DEFAULT_STRATEGIES: usize = 1000;

/// The default strategy as `setDefaultStrategy()` stores it.
#[derive(Clone)]
enum ConfiguredStrategy {
    /// A string or `false`.
    Value(V),
    /// `'name'`: `[FileExtensionEscapingStrategy::class, 'guess']`.
    Guess,
    /// Any other callable.
    Callback(DefaultStrategyCallback),
}

/// The strategies resolved per template name, in insertion order.
#[derive(Default)]
struct ResolvedStrategies {
    values: HashMap<String, V>,
    order: VecDeque<String>,
}

impl EscaperExtension {
    /// The escaper extension with Twig's default strategy, `html`. Twig: `new EscaperExtension()`.
    pub fn new() -> Self {
        EscaperExtension::with_default_strategy(DefaultStrategy::html())
    }

    /// The escaper extension with the given default strategy (the `autoescape` option of the environment).
    /// Twig: `new EscaperExtension($defaultStrategy)`.
    pub fn with_default_strategy(strategy: DefaultStrategy) -> Self {
        EscaperExtension {
            default_strategy: RwLock::new(configure(strategy)),
            resolved: Mutex::new(ResolvedStrategies::default()),
        }
    }

    /// Sets the default strategy and forgets the strategies already resolved; `DefaultStrategy::Strategy("name")`
    /// selects [`escaper::guess`]. Twig: `EscaperExtension::setDefaultStrategy()`.
    pub fn set_default_strategy(&self, strategy: DefaultStrategy) {
        let configured = configure(strategy);
        *self
            .default_strategy
            .write()
            .unwrap_or_else(PoisonError::into_inner) = configured;
        let mut resolved = self.resolved.lock().unwrap_or_else(PoisonError::into_inner);
        resolved.values.clear();
        resolved.order.clear();
    }

    /// The default strategy of the template `name`: a strategy name, `false`, or whatever a callback returned.
    /// Results are remembered per name (the 1000 most recent names), so a callback runs once per template name.
    ///
    /// Twig: `EscaperExtension::getDefaultStrategy()`.
    ///
    /// # Errors
    ///
    /// The error of a callback.
    pub fn default_strategy(&self, name: &str) -> Result<V> {
        {
            let mut resolved = self.resolved.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(strategy) = resolved.values.get(name) {
                return Ok(strategy.clone());
            }
            if resolved.values.len() >= MAX_RESOLVED_DEFAULT_STRATEGIES
                && let Some(first) = resolved.order.pop_front()
            {
                resolved.values.remove(&first);
            }
        }
        let configured = self
            .default_strategy
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        // string callables are disabled: only `'name'` or a callback is called
        let strategy = match configured {
            ConfiguredStrategy::Value(value) => value,
            ConfiguredStrategy::Guess => escaper::guess(name).map_or(V::Bool(false), V::from),
            ConfiguredStrategy::Callback(callback) => callback(name)?,
        };
        let mut resolved = self.resolved.lock().unwrap_or_else(PoisonError::into_inner);
        if resolved
            .values
            .insert(name.to_owned(), strategy.clone())
            .is_none()
        {
            resolved.order.push_back(name.to_owned());
        }
        Ok(strategy)
    }
}

impl Default for EscaperExtension {
    fn default() -> Self {
        EscaperExtension::new()
    }
}

impl fmt::Debug for EscaperExtension {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let strategy = match &*self
            .default_strategy
            .read()
            .unwrap_or_else(PoisonError::into_inner)
        {
            ConfiguredStrategy::Value(value) => format!("{value:?}"),
            ConfiguredStrategy::Guess => "name".to_owned(),
            ConfiguredStrategy::Callback(_) => "callback".to_owned(),
        };
        f.debug_struct("EscaperExtension")
            .field("default_strategy", &strategy)
            .finish_non_exhaustive()
    }
}

/// The conversion of `setDefaultStrategy()`: `'name'` becomes the file-extension guesser.
fn configure(strategy: DefaultStrategy) -> ConfiguredStrategy {
    match strategy {
        DefaultStrategy::Strategy(name) if name == "name" => ConfiguredStrategy::Guess,
        DefaultStrategy::Strategy(name) => ConfiguredStrategy::Value(V::String(name.into_bytes())),
        DefaultStrategy::Disabled => ConfiguredStrategy::Value(V::Bool(false)),
        DefaultStrategy::Callback(callback) => ConfiguredStrategy::Callback(callback),
    }
}

/// Twig: `EscaperExtension::escapeFilterIsSafe()`: the first argument if it is a constant (whatever its value),
/// nothing if it is not, `html` without arguments.
pub fn escape_filter_is_safe(arguments: &Arguments) -> Vec<V> {
    if let Some((_, first)) = arguments.entries().first() {
        if let ExprKind::Constant(constant) = &first.kind {
            return vec![constant.value.clone()];
        }
        return Vec::new();
    }
    vec!["html".into()]
}

impl Extension for EscaperExtension {
    fn class_name(&self) -> &str {
        "Twig\\Extension\\EscaperExtension"
    }

    /// Twig: `EscaperExtension::getTokenParsers()`.
    fn token_parsers(&self) -> Vec<Arc<dyn TokenParser>> {
        vec![Arc::new(AutoEscapeTokenParser)]
    }

    /// Twig: `EscaperExtension::getFilters()`.
    fn filters(&self) -> Vec<TwigFilter> {
        let escape = || {
            Some(PhpCallable::method(
                "Twig\\Runtime\\EscaperRuntime",
                "escape",
                vec![
                    P::required("string"),
                    P::optional("strategy", "html".into()),
                    P::optional("charset", null()),
                    P::optional("autoescape", V::Bool(false)),
                ],
            ))
        };
        let options = || FilterOptions {
            is_safe_callback: Some(Arc::new(escape_filter_is_safe)),
            node_class: FilterNodeClass::Escape,
            ..Default::default()
        };
        vec![
            TwigFilter::new("escape", escape(), options()),
            TwigFilter::new("e", escape(), options()),
            TwigFilter::new(
                "raw",
                None,
                FilterOptions {
                    is_safe: Some(vec!["all".into()]),
                    node_class: FilterNodeClass::Raw,
                    ..Default::default()
                },
            ),
        ]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
