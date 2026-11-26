//! Node classes defined by extensions: tags and expressions that compile themselves with `twig::lltl::Compiler`, and
//! how the node visitors classify them.
//!
//! The oracle vectors `tests/vectors/custom_nodes.json` are the same cases rendered by Twig 3.31.1 with the PHP node
//! classes of `tests/vectors/custom_nodes.gen.php`, the counterparts of the classes below. Regenerate them from the
//! repository root with (Git Bash on Windows needs `MSYS_NO_PATHCONV=1`):
//!
//! ```text
//! MSYS_NO_PATHCONV=1 docker run --rm -i twig-oracle:3.31.1 php < tests/vectors/custom_nodes.gen.php > tests/vectors/custom_nodes.json
//! ```

mod common;

use std::any::Any;
use std::sync::{Arc, LazyLock, Mutex};

use common::Json;
use twig::ast::{
    Arguments, ChildKey, ConstantValue, CustomExpr, CustomNode, Expr, ExprKind, Node, NodeMut,
    NodeRef,
};
use twig::extension::{
    CallableParameter, Extension, FilterNodeClass, FilterNodeFactory, FilterOptions,
    FunctionNodeClass, FunctionNodeFactory, FunctionOptions, PhpCallable, SandboxExtension,
    TestNodeClass, TestNodeFactory, TestOptions, TokenParser, TwigCallable, TwigFilter,
    TwigFunction, TwigTest,
};
use twig::lexer::{Token, TokenType};
use twig::lltl::{BinaryOp, Compiler, Local, NameMode, NodeRuntime};
use twig::loader::ArrayLoader;
use twig::parser::expression::infix::{
    BinaryNodeClass, BinaryNodeFactory, BinaryOperatorExpressionParser,
};
use twig::parser::expression::prefix::{
    UnaryNodeClass, UnaryNodeFactory, UnaryOperatorExpressionParser,
};
use twig::parser::expression::{ExpressionParserEntry, InfixAssociativity};
use twig::parser::{EndTagTest, Parser};
use twig::runtime::{DiagnosticsPolicy, SecurityPolicy};
use twig::value::builtins::string::strtoupper;
use twig::value::object::{ObjectBuilder, ObjectRef};
use twig::value::{PhpError, ThrowableClass, Value, convert, numeric};
use twig::{Array, Environment, EnvironmentOptions, Error, context};

/// The methods every node class implements the same way.
macro_rules! node_boilerplate {
    ($trait:ident) => {
        fn clone_box(&self) -> Box<dyn $trait> {
            Box::new(self.clone())
        }

        fn eq_dyn(&self, other: &dyn $trait) -> bool {
            other.as_any().downcast_ref::<Self>() == Some(self)
        }

        fn as_any(&self) -> &dyn Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }
    };
}

// ---- runtimes -------------------------------------------------------------------------------------------------------

/// PHP's `strtoupper($value)`.
static STRTOUPPER: LazyLock<NodeRuntime> = LazyLock::new(|| {
    NodeRuntime::new(|call| {
        let value = call.arg(0).cloned().unwrap_or_default();
        Ok(Value::String(strtoupper(&value, call.diagnostics())?))
    })
});

/// `FailRuntime::fail($value)` / `fail_logic($value)`: the value, or an exception when it is falsy.
fn fail_runtime(logic: bool) -> NodeRuntime {
    NodeRuntime::new(move |call| {
        let value = call.arg(0).cloned().unwrap_or_default();
        if !convert::to_bool(&value, call.diagnostics())? {
            return Err(if logic {
                PhpError::new(ThrowableClass::LogicException, "Value is empty.").into()
            } else {
                Error::runtime("Value is empty.").into()
            });
        }
        Ok(value)
    })
}

static FAIL: LazyLock<NodeRuntime> = LazyLock::new(|| fail_runtime(false));
static FAIL_LOGIC: LazyLock<NodeRuntime> = LazyLock::new(|| fail_runtime(true));

// ---- {% unless cond %}...{% else %}...{% endunless %} ----------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
struct UnlessNode {
    condition: Expr,
    body: Node,
    otherwise: Option<Node>,
    line: i64,
}

impl CustomNode for UnlessNode {
    fn class_name(&self) -> &str {
        "UnlessNode"
    }

    fn children(&self) -> Vec<(ChildKey<'_>, NodeRef<'_>)> {
        let mut children = vec![
            (ChildKey::Name("condition"), NodeRef::Expr(&self.condition)),
            (ChildKey::Name("body"), NodeRef::Node(&self.body)),
        ];
        if let Some(otherwise) = &self.otherwise {
            children.push((ChildKey::Name("else"), NodeRef::Node(otherwise)));
        }
        children
    }

    fn children_mut(&mut self) -> Vec<(ChildKey<'_>, NodeMut<'_>)> {
        let mut children = vec![
            (
                ChildKey::Name("condition"),
                NodeMut::Expr(&mut self.condition),
            ),
            (ChildKey::Name("body"), NodeMut::Node(&mut self.body)),
        ];
        if let Some(otherwise) = &mut self.otherwise {
            children.push((ChildKey::Name("else"), NodeMut::Node(otherwise)));
        }
        children
    }

    /// `if (!(<condition>)) { <body> } else { <else> }`
    fn compile<'t>(&'t self, compiler: &mut Compiler<'t, '_>) -> twig::Result<()> {
        compiler.add_debug_info(self.line);
        compiler.subcompile_expr(&self.condition)?;
        let otherwise = compiler.jump_if_true()?;
        compiler.subcompile(&self.body)?;
        match &self.otherwise {
            Some(else_body) => {
                let end = compiler.jump();
                compiler.bind(otherwise)?;
                compiler.subcompile(else_body)?;
                compiler.bind(end)
            }
            None => compiler.bind(otherwise),
        }
    }

    node_boilerplate!(CustomNode);
}

struct UnlessTokenParser;

impl TokenParser for UnlessTokenParser {
    fn tag(&self) -> &str {
        "unless"
    }

    fn parse(&self, parser: &mut Parser<'_>, token: &Token) -> twig::Result<Option<Node>> {
        let line = token.line();
        let condition = parser.parse_expression(0)?;
        expect_block_end(parser)?;
        let fork = |token: &Token| token.test_value(TokenType::Name, &["else", "endunless"]);
        let fork = EndTagTest {
            tag: Some(self.tag()),
            test: &fork,
        };
        let body = parser.subparse(Some(&fork), false)?;
        let mut otherwise = None;
        if parser
            .stream_mut()
            .next()?
            .test_value(TokenType::Name, "else")
        {
            expect_block_end(parser)?;
            let end = |token: &Token| token.test_value(TokenType::Name, "endunless");
            let end = EndTagTest {
                tag: Some(self.tag()),
                test: &end,
            };
            otherwise = Some(parser.subparse(Some(&end), true)?);
        }
        expect_block_end(parser)?;
        let node = UnlessNode {
            condition,
            body,
            otherwise,
            line,
        };
        Ok(Some(Node::custom(node, line)))
    }
}

fn expect_block_end(parser: &mut Parser<'_>) -> twig::Result<()> {
    parser
        .stream_mut()
        .expect(TokenType::BlockEnd, None, None)?;
    Ok(())
}

/// The body of a tag up to `{% end %}`, which is consumed with the `%}` after it.
fn body_until(parser: &mut Parser<'_>, tag: &str, end: &'static str) -> twig::Result<Node> {
    expect_block_end(parser)?;
    let test = move |token: &Token| token.test_value(TokenType::Name, end);
    let test = EndTagTest {
        tag: Some(tag),
        test: &test,
    };
    let body = parser.subparse(Some(&test), true)?;
    expect_block_end(parser)?;
    Ok(body)
}

// ---- {% shout_set name = expr %}: $context['name'] = strtoupper(expr) -----------------------------------------------

#[derive(Debug, Clone, PartialEq)]
struct ShoutSetNode {
    name: Vec<u8>,
    value: Expr,
    line: i64,
}

impl CustomNode for ShoutSetNode {
    fn class_name(&self) -> &str {
        "ShoutSetNode"
    }

    fn children(&self) -> Vec<(ChildKey<'_>, NodeRef<'_>)> {
        vec![(ChildKey::Name("value"), NodeRef::Expr(&self.value))]
    }

    fn children_mut(&mut self) -> Vec<(ChildKey<'_>, NodeMut<'_>)> {
        vec![(ChildKey::Name("value"), NodeMut::Expr(&mut self.value))]
    }

    fn compile<'t>(&'t self, compiler: &mut Compiler<'t, '_>) -> twig::Result<()> {
        if self.name == b"_context" {
            return Err(Error::logic("Cannot assign to _context."));
        }
        compiler.add_debug_info(self.line);
        compiler.call_runtime(&STRTOUPPER, |c| c.subcompile_expr(&self.value))?;
        compiler.store_name(&self.name)
    }

    node_boilerplate!(CustomNode);
}

struct ShoutSetTokenParser;

impl TokenParser for ShoutSetTokenParser {
    fn tag(&self) -> &str {
        "shout_set"
    }

    fn parse(&self, parser: &mut Parser<'_>, token: &Token) -> twig::Result<Option<Node>> {
        let name = parser
            .stream_mut()
            .expect(TokenType::Name, None, None)?
            .value()
            .as_bytes()
            .unwrap_or_default()
            .to_vec();
        parser
            .stream_mut()
            .expect(TokenType::Operator, Some("="), None)?;
        let value = parser.parse_expression(0)?;
        expect_block_end(parser)?;
        let line = token.line();
        Ok(Some(Node::custom(ShoutSetNode { name, value, line }, line)))
    }
}

// ---- {% shout_block %}...{% endshout_block %}: yield strtoupper(<captured body>) ----------------------------------

#[derive(Debug, Clone, PartialEq)]
struct ShoutBlockNode {
    body: Node,
    line: i64,
}

impl CustomNode for ShoutBlockNode {
    fn class_name(&self) -> &str {
        "ShoutBlockNode"
    }

    fn children(&self) -> Vec<(ChildKey<'_>, NodeRef<'_>)> {
        vec![(ChildKey::Name("body"), NodeRef::Node(&self.body))]
    }

    fn children_mut(&mut self) -> Vec<(ChildKey<'_>, NodeMut<'_>)> {
        vec![(ChildKey::Name("body"), NodeMut::Node(&mut self.body))]
    }

    fn compile<'t>(&'t self, compiler: &mut Compiler<'t, '_>) -> twig::Result<()> {
        // `$_vN = <capture>; yield strtoupper($_vN);`
        compiler.local();
        compiler.add_debug_info(self.line);
        compiler.call_runtime(&STRTOUPPER, |c| {
            c.capture(true, |c| c.subcompile(&self.body))
        })?;
        compiler.print()
    }

    node_boilerplate!(CustomNode);
}

struct ShoutBlockTokenParser;

impl TokenParser for ShoutBlockTokenParser {
    fn tag(&self) -> &str {
        "shout_block"
    }

    fn parse(&self, parser: &mut Parser<'_>, token: &Token) -> twig::Result<Option<Node>> {
        let body = body_until(parser, self.tag(), "endshout_block")?;
        let line = token.line();
        Ok(Some(Node::custom(ShoutBlockNode { body, line }, line)))
    }
}

// ---- {% repeat count %}...{% endrepeat %}: for ($i = 0; $i < count; $i = $i + 1) { body } ----------------------

#[derive(Debug, Clone, PartialEq)]
struct RepeatNode {
    count: Expr,
    body: Node,
    line: i64,
}

impl CustomNode for RepeatNode {
    fn class_name(&self) -> &str {
        "RepeatNode"
    }

    fn children(&self) -> Vec<(ChildKey<'_>, NodeRef<'_>)> {
        vec![
            (ChildKey::Name("count"), NodeRef::Expr(&self.count)),
            (ChildKey::Name("body"), NodeRef::Node(&self.body)),
        ]
    }

    fn children_mut(&mut self) -> Vec<(ChildKey<'_>, NodeMut<'_>)> {
        vec![
            (ChildKey::Name("count"), NodeMut::Expr(&mut self.count)),
            (ChildKey::Name("body"), NodeMut::Node(&mut self.body)),
        ]
    }

    fn compile<'t>(&'t self, compiler: &mut Compiler<'t, '_>) -> twig::Result<()> {
        if let ExprKind::Constant(constant) = &self.count.kind
            && !is_numeric(&constant.value)
        {
            return Err(
                Error::syntax("The count of the \"repeat\" tag must be a number.")
                    .with_line(self.line),
            );
        }
        let i = compiler.local();
        compiler.add_debug_info(self.line);
        compiler.constant(0);
        compiler.store_local(i)?;
        let top = compiler.mark();
        compiler.load_local(i)?;
        compiler.subcompile_expr(&self.count)?;
        compiler.binary(BinaryOp::Less)?;
        let end = compiler.jump_if_false()?;
        compiler.subcompile(&self.body)?;
        compiler.load_local(i)?;
        compiler.constant(1);
        compiler.binary(BinaryOp::Add)?;
        compiler.store_local(i)?;
        compiler.jump_to(top)?;
        compiler.bind(end)
    }

    node_boilerplate!(CustomNode);
}

/// PHP's `is_numeric()` of a constant.
fn is_numeric(value: &ConstantValue) -> bool {
    match value {
        ConstantValue::Int(_) | ConstantValue::Float(_) => true,
        ConstantValue::String(s) => numeric::is_numeric_string(s, false).is_some(),
        _ => false,
    }
}

struct RepeatTokenParser;

impl TokenParser for RepeatTokenParser {
    fn tag(&self) -> &str {
        "repeat"
    }

    fn parse(&self, parser: &mut Parser<'_>, token: &Token) -> twig::Result<Option<Node>> {
        let count = parser.parse_expression(0)?;
        let body = body_until(parser, self.tag(), "endrepeat")?;
        let line = token.line();
        Ok(Some(Node::custom(RepeatNode { count, body, line }, line)))
    }
}

// ---- a FunctionExpression subclass: `strtoupper(<first argument>)`, or `FailRuntime::<name>(<first argument>)` ----

#[derive(Debug, Clone, PartialEq)]
struct CallFunction {
    function: TwigFunction,
    arguments: Arguments,
    runtime: NodeRuntime,
}

impl CustomExpr for CallFunction {
    fn class_name(&self) -> &str {
        "CallFunction"
    }

    fn children(&self) -> Vec<(ChildKey<'_>, NodeRef<'_>)> {
        vec![(
            ChildKey::Name("arguments"),
            NodeRef::Arguments(&self.arguments),
        )]
    }

    fn children_mut(&mut self) -> Vec<(ChildKey<'_>, NodeMut<'_>)> {
        vec![(
            ChildKey::Name("arguments"),
            NodeMut::Arguments(&mut self.arguments),
        )]
    }

    fn callable(&self) -> Option<TwigCallable> {
        Some(TwigCallable::Function(self.function.clone()))
    }

    fn compile<'t>(&'t self, compiler: &mut Compiler<'t, '_>) -> twig::Result<()> {
        let argument = &self.arguments.entries()[0].1;
        compiler.call_runtime(&self.runtime, |c| c.subcompile_expr(argument))
    }

    node_boilerplate!(CustomExpr);
}

struct CallFunctionFactory(&'static LazyLock<NodeRuntime>);

impl FunctionNodeFactory for CallFunctionFactory {
    fn class_name(&self) -> &str {
        "CallFunction"
    }

    fn create(
        &self,
        function: TwigFunction,
        arguments: Arguments,
        line: i64,
    ) -> twig::Result<Expr> {
        let runtime = NodeRuntime::clone(self.0);
        Ok(Expr::custom(
            CallFunction {
                function,
                arguments,
                runtime,
            },
            line,
        ))
    }
}

// ---- a FilterExpression subclass: `'<p>' . <input> . '</p>'` ------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
struct WrapFilter {
    node: Expr,
    filter: TwigFilter,
    arguments: Arguments,
}

impl CustomExpr for WrapFilter {
    fn class_name(&self) -> &str {
        "WrapFilter"
    }

    fn children(&self) -> Vec<(ChildKey<'_>, NodeRef<'_>)> {
        vec![
            (ChildKey::Name("node"), NodeRef::Expr(&self.node)),
            (
                ChildKey::Name("arguments"),
                NodeRef::Arguments(&self.arguments),
            ),
        ]
    }

    fn children_mut(&mut self) -> Vec<(ChildKey<'_>, NodeMut<'_>)> {
        vec![
            (ChildKey::Name("node"), NodeMut::Expr(&mut self.node)),
            (
                ChildKey::Name("arguments"),
                NodeMut::Arguments(&mut self.arguments),
            ),
        ]
    }

    fn callable(&self) -> Option<TwigCallable> {
        Some(TwigCallable::Filter(self.filter.clone()))
    }

    fn compile<'t>(&'t self, compiler: &mut Compiler<'t, '_>) -> twig::Result<()> {
        compiler.constant("<p>");
        compiler.subcompile_expr(&self.node)?;
        compiler.binary(BinaryOp::Concat)?;
        compiler.constant("</p>");
        compiler.binary(BinaryOp::Concat)
    }

    node_boilerplate!(CustomExpr);
}

struct WrapFilterFactory;

impl FilterNodeFactory for WrapFilterFactory {
    fn class_name(&self) -> &str {
        "WrapFilter"
    }

    fn create(
        &self,
        node: Expr,
        filter: TwigFilter,
        arguments: Arguments,
        line: i64,
    ) -> twig::Result<Expr> {
        Ok(Expr::custom(
            WrapFilter {
                node,
                filter,
                arguments,
            },
            line,
        ))
    }
}

// ---- a TestExpression subclass: `('' === <input>)` ----------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
struct EmptyStringTest {
    node: Expr,
    test: TwigTest,
    arguments: Option<Arguments>,
}

impl CustomExpr for EmptyStringTest {
    fn class_name(&self) -> &str {
        "EmptyStringTest"
    }

    fn children(&self) -> Vec<(ChildKey<'_>, NodeRef<'_>)> {
        let mut children = vec![(ChildKey::Name("node"), NodeRef::Expr(&self.node))];
        if let Some(arguments) = &self.arguments {
            children.push((ChildKey::Name("arguments"), NodeRef::Arguments(arguments)));
        }
        children
    }

    fn children_mut(&mut self) -> Vec<(ChildKey<'_>, NodeMut<'_>)> {
        let mut children = vec![(ChildKey::Name("node"), NodeMut::Expr(&mut self.node))];
        if let Some(arguments) = &mut self.arguments {
            children.push((ChildKey::Name("arguments"), NodeMut::Arguments(arguments)));
        }
        children
    }

    fn callable(&self) -> Option<TwigCallable> {
        Some(TwigCallable::Test(self.test.clone()))
    }

    fn compile<'t>(&'t self, compiler: &mut Compiler<'t, '_>) -> twig::Result<()> {
        compiler.constant("");
        compiler.subcompile_expr(&self.node)?;
        compiler.binary(BinaryOp::Identical)
    }

    node_boilerplate!(CustomExpr);
}

struct EmptyStringTestFactory;

impl TestNodeFactory for EmptyStringTestFactory {
    fn class_name(&self) -> &str {
        "EmptyStringTest"
    }

    fn create(
        &self,
        node: Expr,
        test: TwigTest,
        arguments: Option<Arguments>,
        line: i64,
    ) -> twig::Result<Expr> {
        Ok(Expr::custom(
            EmptyStringTest {
                node,
                test,
                arguments,
            },
            line,
        ))
    }
}

// ---- an OperatorEscapeInterface: `((left) ?: (right))` -------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
struct OrElse {
    left: Expr,
    right: Expr,
    /// The operands it names to escape.
    operands: Vec<String>,
}

impl CustomExpr for OrElse {
    fn class_name(&self) -> &str {
        "OrElseBinary"
    }

    fn children(&self) -> Vec<(ChildKey<'_>, NodeRef<'_>)> {
        vec![
            (ChildKey::Name("left"), NodeRef::Expr(&self.left)),
            (ChildKey::Name("right"), NodeRef::Expr(&self.right)),
        ]
    }

    fn children_mut(&mut self) -> Vec<(ChildKey<'_>, NodeMut<'_>)> {
        vec![
            (ChildKey::Name("left"), NodeMut::Expr(&mut self.left)),
            (ChildKey::Name("right"), NodeMut::Expr(&mut self.right)),
        ]
    }

    fn operand_names_to_escape(&self) -> Option<Vec<String>> {
        Some(self.operands.clone())
    }

    fn compile<'t>(&'t self, compiler: &mut Compiler<'t, '_>) -> twig::Result<()> {
        compiler.subcompile_expr(&self.left)?;
        compiler.dup()?;
        let done = compiler.jump_if_true()?;
        compiler.pop()?;
        compiler.subcompile_expr(&self.right)?;
        compiler.bind(done)
    }

    node_boilerplate!(CustomExpr);
}

struct OrElseFactory(&'static [&'static str]);

impl BinaryNodeFactory for OrElseFactory {
    fn class_name(&self) -> &str {
        "OrElseBinary"
    }

    fn create(&self, left: Expr, right: Expr, line: i64) -> twig::Result<Expr> {
        let operands = self.0.iter().map(|name| (*name).to_owned()).collect();
        Ok(Expr::custom(
            OrElse {
                left,
                right,
                operands,
            },
            line,
        ))
    }
}

// ---- a plain expression class `strtoupper(node)`, and one declaring its string coercion ---------------------------

#[derive(Debug, Clone, PartialEq)]
struct Up {
    node: Expr,
    coercing: bool,
}

impl CustomExpr for Up {
    fn class_name(&self) -> &str {
        "UpUnary"
    }

    fn children(&self) -> Vec<(ChildKey<'_>, NodeRef<'_>)> {
        vec![(ChildKey::Name("node"), NodeRef::Expr(&self.node))]
    }

    fn children_mut(&mut self) -> Vec<(ChildKey<'_>, NodeMut<'_>)> {
        vec![(ChildKey::Name("node"), NodeMut::Expr(&mut self.node))]
    }

    fn string_coerced_child_names(&self) -> Vec<String> {
        if self.coercing {
            vec!["node".to_owned()]
        } else {
            Vec::new()
        }
    }

    fn compile<'t>(&'t self, compiler: &mut Compiler<'t, '_>) -> twig::Result<()> {
        compiler.call_runtime(&STRTOUPPER, |c| c.subcompile_expr(&self.node))
    }

    node_boilerplate!(CustomExpr);
}

struct UpFactory {
    coercing: bool,
}

impl UnaryNodeFactory for UpFactory {
    fn class_name(&self) -> &str {
        "UpUnary"
    }

    fn create(&self, node: Expr, line: i64) -> twig::Result<Expr> {
        let coercing = self.coercing;
        Ok(Expr::custom(Up { node, coercing }, line))
    }
}

// ---- the extension ------------------------------------------------------------------------------------------------

struct CustomNodesExtension;

fn function(runtime: &'static LazyLock<NodeRuntime>) -> FunctionOptions {
    FunctionOptions {
        node_class: FunctionNodeClass::Custom(Arc::new(CallFunctionFactory(runtime))),
        ..Default::default()
    }
}

impl Extension for CustomNodesExtension {
    fn token_parsers(&self) -> Vec<Arc<dyn TokenParser>> {
        vec![
            Arc::new(UnlessTokenParser),
            Arc::new(ShoutSetTokenParser),
            Arc::new(ShoutBlockTokenParser),
            Arc::new(RepeatTokenParser),
        ]
    }

    fn functions(&self) -> Vec<TwigFunction> {
        let html = vec![ConstantValue::string("html")];
        // `static fn (string $s, int $n) => ''`
        let typed = PhpCallable::closure(vec![
            CallableParameter::required("s"),
            CallableParameter::required("n").string_coercion_safe(),
        ]);
        vec![
            TwigFunction::new("shout", None, function(&STRTOUPPER)),
            TwigFunction::new(
                "shout_safe",
                None,
                FunctionOptions {
                    is_safe: Some(html),
                    ..function(&STRTOUPPER)
                },
            ),
            TwigFunction::new("shout2", Some(typed), function(&STRTOUPPER)),
            TwigFunction::new("fail", None, function(&FAIL)),
            TwigFunction::new("fail_logic", None, function(&FAIL_LOGIC)),
        ]
    }

    fn filters(&self) -> Vec<TwigFilter> {
        let node_class = FilterNodeClass::Custom(Arc::new(WrapFilterFactory));
        vec![
            TwigFilter::new(
                "wrap",
                None,
                FilterOptions {
                    node_class: node_class.clone(),
                    pre_escape: Some("html".to_owned()),
                    is_safe: Some(vec![ConstantValue::string("html")]),
                    ..Default::default()
                },
            ),
            TwigFilter::new(
                "wrap2",
                None,
                FilterOptions {
                    node_class,
                    preserves_safety: Some(vec![ConstantValue::string("html")]),
                    ..Default::default()
                },
            ),
        ]
    }

    fn tests(&self) -> Vec<TwigTest> {
        vec![TwigTest::new(
            "empty_string",
            None,
            TestOptions {
                node_class: TestNodeClass::Custom(Arc::new(EmptyStringTestFactory)),
                ..Default::default()
            },
        )]
    }

    fn expression_parsers(&self) -> Vec<ExpressionParserEntry> {
        let binary = |name: &str, operands: &'static [&'static str]| {
            ExpressionParserEntry::infix(BinaryOperatorExpressionParser::new(
                BinaryNodeClass::Custom(Arc::new(OrElseFactory(operands))),
                name,
                15,
                InfixAssociativity::Left,
            ))
        };
        let unary = |name: &str, coercing: bool| {
            ExpressionParserEntry::prefix(UnaryOperatorExpressionParser::new(
                UnaryNodeClass::Custom(Arc::new(UpFactory { coercing })),
                name,
                500,
            ))
        };
        vec![
            binary("orelse", &["left", "right"]),
            binary("three", &["left", "right", "left"]),
            unary("up", false),
            unary("upc", true),
        ]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// The `Obj` class of the vectors: a Stringable object.
fn obj() -> ObjectRef {
    ObjectBuilder::new("Obj")
        .stringable(|_, _| Ok("obj"))
        .build()
}

fn strings(json: &Json) -> Vec<String> {
    json.as_array()
        .iter()
        .map(|item| item.as_str().expect("a string").to_owned())
        .collect()
}

/// The environment of a vector case: Twig's defaults, the extension, and a global strict sandbox when the case has
/// a policy.
fn vector_environment(case: &Json) -> Environment {
    let templates: Vec<(String, String)> = case
        .field("templates")
        .as_object()
        .iter()
        .map(|(name, code)| (name.clone(), code.as_str().expect("code").to_owned()))
        .collect();
    let options = EnvironmentOptions {
        strict_variables: case.field("strict_variables") == &Json::Bool(true),
        ..Default::default()
    };
    let env = Environment::with_options(ArrayLoader::new(templates), options);
    env.set_diagnostics_policy(DiagnosticsPolicy::Error);
    env.add_extension(Arc::new(CustomNodesExtension))
        .expect("registered");
    if let Some(sandbox) = case.get("sandbox").filter(|s| **s != Json::Null) {
        let policy = SecurityPolicy::new();
        policy.set_allowed_tags(strings(sandbox.field("tags")));
        policy.set_allowed_filters(strings(sandbox.field("filters")));
        policy.set_allowed_functions(strings(sandbox.field("functions")));
        policy.set_allowed_tests(strings(sandbox.field("tests")));
        policy.set_strict(true);
        env.add_extension(Arc::new(SandboxExtension::new(Arc::new(policy), true)))
            .expect("registered");
    }
    env
}

fn vector_context(case: &Json) -> Array {
    let mut context = Array::new();
    for (name, value) in case.field("context").as_object() {
        let value = match value {
            Json::Null => Value::Null,
            Json::Bool(b) => Value::Bool(*b),
            Json::String(s) => Value::from(s.as_str()),
            Json::Number(n) => match n.parse::<i64>() {
                Ok(i) => Value::Int(i),
                Err(_) => Value::Float(n.parse().expect("a number")),
            },
            other => panic!("unsupported context value {other:?}"),
        };
        context.insert(name.as_str(), value);
    }
    for name in strings(case.field("objects")) {
        context.insert(name.as_str(), Value::Object(obj()));
    }
    context
}

#[test]
fn renders_like_the_php_node_classes() {
    let cases = Json::load_vectors("custom_nodes.json");
    let cases = cases.as_array();
    assert!(cases.len() >= 45, "the vector file looks truncated");
    let mut failures = Vec::new();
    for case in cases {
        let name = case.field("name").as_str().expect("name");
        let env = vector_environment(case);
        let rendered = env.render("index.twig", vector_context(case));
        let got = match &rendered {
            Ok(output) => format!("output {output:?}"),
            Err(error) => format!("{}: {error}", error.class_name()),
        };
        let expected = match case.field("error") {
            Json::Null => format!(
                "output {:?}",
                case.field("output").as_str().expect("output")
            ),
            error => format!(
                "{}: {}",
                error.field("class").as_str().expect("class"),
                error.field("message").as_str().expect("message")
            ),
        };
        if got != expected {
            failures.push(format!("{name}:\n  expected {expected}\n  got      {got}"));
        }
        let deprecations: Vec<String> = env
            .take_notices()
            .iter()
            .map(|notice| notice.to_string())
            .collect();
        let expected_deprecations: Vec<String> = strings(case.field("deprecations"))
            .into_iter()
            .map(|message| format!("Deprecated: {message}"))
            .collect();
        if deprecations != expected_deprecations {
            failures.push(format!(
                "{name}: deprecations {deprecations:?}, expected {expected_deprecations:?}"
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} mismatches:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

// ---- the builder's checks -------------------------------------------------------------------------------------------

/// The code of a [`CodeNode`].
type CodeFn = dyn for<'t> Fn(&'t CodeNode, &mut Compiler<'t, '_>) -> twig::Result<()> + Send + Sync;

/// A statement node whose code is a closure, for the checks of the builder.
#[derive(Clone)]
struct CodeNode {
    children: Vec<Node>,
    code: Arc<CodeFn>,
}

impl std::fmt::Debug for CodeNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CodeNode")
    }
}

impl PartialEq for CodeNode {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.code, &other.code)
    }
}

impl CustomNode for CodeNode {
    fn class_name(&self) -> &str {
        "CodeNode"
    }

    fn children(&self) -> Vec<(ChildKey<'_>, NodeRef<'_>)> {
        self.children
            .iter()
            .enumerate()
            .map(|(i, node)| (ChildKey::Index(i as i64), NodeRef::Node(node)))
            .collect()
    }

    fn children_mut(&mut self) -> Vec<(ChildKey<'_>, NodeMut<'_>)> {
        self.children
            .iter_mut()
            .enumerate()
            .map(|(i, node)| (ChildKey::Index(i as i64), NodeMut::Node(node)))
            .collect()
    }

    fn compile<'t>(&'t self, compiler: &mut Compiler<'t, '_>) -> twig::Result<()> {
        (self.code)(self, compiler)
    }

    node_boilerplate!(CustomNode);
}

/// `{% code %}` (a body up to `{% endcode %}`) compiles with `code`.
struct CodeTokenParser {
    code: Arc<CodeFn>,
}

impl TokenParser for CodeTokenParser {
    fn tag(&self) -> &str {
        "code"
    }

    fn parse(&self, parser: &mut Parser<'_>, token: &Token) -> twig::Result<Option<Node>> {
        let body = body_until(parser, self.tag(), "endcode")?;
        let node = CodeNode {
            children: vec![body],
            code: Arc::clone(&self.code),
        };
        Ok(Some(Node::custom(node, token.line())))
    }
}

/// Renders `template` with `{% code %}` compiled by `code`.
fn render_code(
    template: &str,
    context: Array,
    code: impl for<'t> Fn(&'t CodeNode, &mut Compiler<'t, '_>) -> twig::Result<()>
    + Send
    + Sync
    + 'static,
) -> twig::Result<String> {
    let env = Environment::new(ArrayLoader::new([("index", template)]));
    env.add_token_parser(Arc::new(CodeTokenParser {
        code: Arc::new(code),
    }))?;
    env.render("index", context)
}

fn compile_error(message: &str) -> String {
    format!(
        "An exception has been thrown during the compilation of a template (\"{message}\") in \"index\"."
    )
}

#[test]
fn statements_leave_the_stack_as_they_found_it() {
    let error = render_code("{% code %}{% endcode %}", Array::new(), |_, c| {
        c.constant(1);
        Ok(())
    })
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        compile_error(
            "The \"CodeNode\" node class leaves 1 value on the stack where a statement node must leave 0 values."
        )
    );
    // the code cannot take what the enclosing code put on the stack
    let error = render_code("{% code %}{% endcode %}", Array::new(), |_, c| c.pop()).unwrap_err();
    assert_eq!(
        error.to_string(),
        compile_error(
            "The \"CodeNode\" node class takes a value from the stack that its code did not put there."
        )
    );
}

#[test]
fn jumps_are_checked() {
    let unbound = render_code("{% code %}{% endcode %}", Array::new(), |_, c| {
        let _ = c.jump();
        Ok(())
    })
    .unwrap_err();
    assert_eq!(
        unbound.to_string(),
        compile_error("The \"CodeNode\" node class does not bind 1 jump.")
    );
    let height = render_code("{% code %}{% endcode %}", Array::new(), |_, c| {
        let jump = c.jump();
        c.constant(1);
        c.bind(jump)?;
        c.pop()
    })
    .unwrap_err();
    // after an unconditional jump the code is dead: the bind takes the jump's height
    assert_eq!(
        height.to_string(),
        compile_error(
            "The \"CodeNode\" node class takes a value from the stack that its code did not put there."
        )
    );
    let height = render_code("{% code %}{% endcode %}", Array::new(), |_, c| {
        c.constant(true);
        let jump = c.jump_if_false()?;
        c.constant(1);
        c.bind(jump)
    })
    .unwrap_err();
    assert_eq!(
        height.to_string(),
        compile_error(
            "The \"CodeNode\" node class jumps with 0 values on the stack to a place where there is 1."
        )
    );
    let capture = render_code("{% code %}{% endcode %}", Array::new(), |_, c| {
        let mut out = None;
        c.capture(false, |c| {
            out = Some(c.jump());
            Ok(())
        })
        .map(|_| ())
    })
    .unwrap_err();
    assert_eq!(
        capture.to_string(),
        compile_error("The \"CodeNode\" node class does not bind 1 jump.")
    );
    let into_capture = render_code("{% code %}{% endcode %}", Array::new(), |_, c| {
        let jump = c.jump();
        c.capture(true, |c| c.bind(jump))?;
        c.pop()
    })
    .unwrap_err();
    assert_eq!(
        into_capture.to_string(),
        compile_error(
            "The \"CodeNode\" node class jumps into or out of a capture or the arguments of a runtime call."
        )
    );
}

#[test]
fn handles_belong_to_one_compilation() {
    static STASH: Mutex<Option<Local>> = Mutex::new(None);
    let env = Environment::new(ArrayLoader::new([
        ("a", "{% code %}{% endcode %}"),
        ("b", "{% code %}{% endcode %}"),
    ]));
    env.add_token_parser(Arc::new(CodeTokenParser {
        code: Arc::new(|_, c| {
            let mut stash = STASH.lock().unwrap();
            match *stash {
                Some(local) => c.load_local(local).and_then(|()| c.pop()),
                None => {
                    *stash = Some(c.local());
                    Ok(())
                }
            }
        }),
    }))
    .unwrap();
    assert_eq!(env.render("a", Array::new()).unwrap(), "");
    let error = env.render("b", Array::new()).unwrap_err();
    assert_eq!(
        error.to_string(),
        "An exception has been thrown during the compilation of a template (\"The \\\"CodeNode\\\" node class uses a \
         jump, a mark or a local of another compilation.\") in \"b\"."
            .replace("\\\"", "\"")
    );
}

#[test]
fn context_locals_and_output() {
    // `$_v = $context; $context['x'] = 'inner'; <body>; $context = $_v;`, then `$context['y']` read leniently
    let output = render_code(
        "{% code %}[{{ x }}]{% endcode %}{{ x }}",
        context! { x => "outer" },
        |node, c| {
            let saved = c.local();
            c.load_context();
            c.store_local(saved)?;
            c.constant("inner");
            c.store_name("x")?;
            c.subcompile(&node.children[0])?;
            c.load_local(saved)?;
            c.set_context()?;
            c.load_name("y", NameMode::Lenient);
            c.print()?;
            c.text("!");
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(output, "[inner]!outer");

    // a capture keeps its output and becomes a value; `markup()` makes the value safe
    let output = render_code(
        "{% code %}<{{ '<b>' }}>{% endcode %}",
        Array::new(),
        |node, c| {
            c.capture(false, |c| c.subcompile(&node.children[0]))?;
            c.string_cast()?;
            c.dup()?;
            c.binary(BinaryOp::Concat)?;
            c.markup()?;
            c.print()
        },
    )
    .unwrap();
    assert_eq!(output, "<<b>><<b>>");
}

#[test]
fn strict_names_fail_at_their_line() {
    let error = render_code("\n{% code %}{% endcode %}", Array::new(), |_, c| {
        c.add_debug_info(2);
        c.load_name("missing", NameMode::Strict { line: 7 });
        c.print()
    })
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "Variable \"missing\" does not exist in \"index\" at line 7."
    );
}

#[test]
fn runtimes_see_the_running_template() {
    let runtime = NodeRuntime::new(|call| {
        let facts = format!(
            "{} {} {} {}",
            call.template_name(),
            call.source().name(),
            call.charset().to_string_lossy(),
            call.is_sandboxed()
        );
        call.write(facts.as_bytes())?;
        // PHP's error for a value that is not callable
        call.invoke(&Value::Null, Vec::new())
    });
    let env = Environment::new(ArrayLoader::new([("index", "{% code %}{% endcode %}")]));
    env.add_token_parser(Arc::new(CodeTokenParser {
        code: Arc::new(move |_, c| {
            c.add_debug_info(1);
            c.call_runtime(&runtime, |_| Ok(()))?;
            c.print()
        }),
    }))
    .unwrap();
    let mut out = Vec::new();
    let error = env
        .load("index")
        .unwrap()
        .display(Array::new(), &mut out)
        .unwrap_err();
    assert_eq!(String::from_utf8(out).unwrap(), "index index UTF-8 false");
    // wrapped by the template, at the line of the call
    assert_eq!(
        error.to_string(),
        "An exception has been thrown during the rendering of a template (\"Value of type null is not callable\") in \
         \"index\" at line 1."
    );
}

/// `{{ apply_to(x, v => ...) }}`: a function node class calling its arrow argument through a runtime.
#[derive(Debug, Clone, PartialEq)]
struct ApplyTo {
    function: TwigFunction,
    arguments: Arguments,
}

static APPLY: LazyLock<NodeRuntime> = LazyLock::new(|| {
    NodeRuntime::new(|call| {
        let [value, arrow] = <[Value; 2]>::try_from(call.take_args()).expect("two arguments");
        call.write(b"(")?;
        let result = call.invoke(&arrow, vec![value])?;
        call.write(b")")?;
        call.context_mut().insert("applied", true);
        call.deprecate(twig::deprecation::Deprecation::new(
            "app/app",
            "1.0",
            format!("apply_to() in {}.", call.template_name()),
        ));
        Ok(result)
    })
});

impl CustomExpr for ApplyTo {
    fn class_name(&self) -> &str {
        "ApplyTo"
    }

    fn children(&self) -> Vec<(ChildKey<'_>, NodeRef<'_>)> {
        vec![(
            ChildKey::Name("arguments"),
            NodeRef::Arguments(&self.arguments),
        )]
    }

    fn children_mut(&mut self) -> Vec<(ChildKey<'_>, NodeMut<'_>)> {
        vec![(
            ChildKey::Name("arguments"),
            NodeMut::Arguments(&mut self.arguments),
        )]
    }

    fn callable(&self) -> Option<TwigCallable> {
        Some(TwigCallable::Function(self.function.clone()))
    }

    fn compile<'t>(&'t self, compiler: &mut Compiler<'t, '_>) -> twig::Result<()> {
        compiler.call_runtime(&APPLY, |c| {
            for (_, argument) in self.arguments.entries() {
                c.subcompile_expr(argument)?;
            }
            Ok(())
        })
    }

    node_boilerplate!(CustomExpr);
}

struct ApplyToFactory;

impl FunctionNodeFactory for ApplyToFactory {
    fn class_name(&self) -> &str {
        "ApplyTo"
    }

    fn create(
        &self,
        function: TwigFunction,
        arguments: Arguments,
        line: i64,
    ) -> twig::Result<Expr> {
        Ok(Expr::custom(
            ApplyTo {
                function,
                arguments,
            },
            line,
        ))
    }
}

#[test]
fn runtimes_write_output_change_the_context_and_call_back() {
    let env = Environment::new(ArrayLoader::new([(
        "index",
        "{{ apply_to('ab', v => v|upper ~ (applied ?? 'no')) }}|{{ applied ? 'yes' }}",
    )]));
    env.set_diagnostics_policy(DiagnosticsPolicy::Collect);
    env.add_function(TwigFunction::new(
        "apply_to",
        None,
        FunctionOptions {
            node_class: FunctionNodeClass::Custom(Arc::new(ApplyToFactory)),
            is_safe: Some(vec![ConstantValue::string("html")]),
            ..Default::default()
        },
    ))
    .unwrap();
    // the arrow function captured the context before the runtime changed it
    // the runtime writes before the template prints what it returns
    assert_eq!(env.render("index", Array::new()).unwrap(), "()ABno|yes");
    let notices: Vec<String> = env.take_notices().iter().map(ToString::to_string).collect();
    assert_eq!(
        notices,
        ["Deprecated: Since app/app 1.0: apply_to() in index."]
    );
}

#[test]
fn runtime_errors_keep_an_explicit_line() {
    let runtime = NodeRuntime::new(|_| Err(Error::runtime("Broken.").with_line(9).into()));
    let error = render_code("{% code %}{% endcode %}", Array::new(), move |_, c| {
        c.add_debug_info(1);
        c.call_runtime(&runtime, |_| Ok(()))?;
        c.pop()
    })
    .unwrap_err();
    assert_eq!(error.to_string(), "Broken in \"index\" at line 9.");
}

/// A node class without `compile()`: Twig's `Node::compile()` compiles the children in order.
#[derive(Debug, Clone, PartialEq)]
struct Children(Vec<Node>, Option<Expr>);

impl CustomNode for Children {
    fn class_name(&self) -> &str {
        "ChildrenNode"
    }

    fn children(&self) -> Vec<(ChildKey<'_>, NodeRef<'_>)> {
        let mut children: Vec<_> = self
            .0
            .iter()
            .enumerate()
            .map(|(i, node)| (ChildKey::Index(i as i64), NodeRef::Node(node)))
            .collect();
        if let Some(expr) = &self.1 {
            children.push((ChildKey::Name("expr"), NodeRef::Expr(expr)));
        }
        children
    }

    fn children_mut(&mut self) -> Vec<(ChildKey<'_>, NodeMut<'_>)> {
        let mut children: Vec<_> = self
            .0
            .iter_mut()
            .enumerate()
            .map(|(i, node)| (ChildKey::Index(i as i64), NodeMut::Node(node)))
            .collect();
        if let Some(expr) = &mut self.1 {
            children.push((ChildKey::Name("expr"), NodeMut::Expr(expr)));
        }
        children
    }

    node_boilerplate!(CustomNode);
}

struct ChildrenTokenParser;

impl TokenParser for ChildrenTokenParser {
    fn tag(&self) -> &str {
        "children"
    }

    fn parse(&self, parser: &mut Parser<'_>, token: &Token) -> twig::Result<Option<Node>> {
        let expr = if parser.stream().current().test(TokenType::BlockEnd) {
            None
        } else {
            Some(parser.parse_expression(0)?)
        };
        let body = body_until(parser, self.tag(), "endchildren")?;
        Ok(Some(Node::custom(
            Children(vec![body.clone(), body], expr),
            token.line(),
        )))
    }
}

#[test]
fn classes_without_compile_compile_their_children() {
    let env = Environment::new(ArrayLoader::new([
        ("nodes", "{% children %}<{{ x }}>{% endchildren %}"),
        ("expr", "{% children x %}<{{ x }}>{% endchildren %}"),
    ]));
    env.add_token_parser(Arc::new(ChildrenTokenParser)).unwrap();
    assert_eq!(
        env.render("nodes", context! { x => "&" }).unwrap(),
        "<&amp;><&amp;>"
    );
    let error = env.render("expr", Array::new()).unwrap_err();
    assert_eq!(
        error.to_string(),
        "An exception has been thrown during the compilation of a template (\"The \"ChildrenNode\" node class cannot \
         be compiled by twig-rs.\") in \"expr\"."
    );
}

#[test]
fn expression_classes_without_compile_compile_their_only_child() {
    #[derive(Debug, Clone, PartialEq)]
    struct Paren(Expr);

    impl CustomExpr for Paren {
        fn class_name(&self) -> &str {
            "ParenExpression"
        }

        fn children(&self) -> Vec<(ChildKey<'_>, NodeRef<'_>)> {
            vec![(ChildKey::Name("node"), NodeRef::Expr(&self.0))]
        }

        fn children_mut(&mut self) -> Vec<(ChildKey<'_>, NodeMut<'_>)> {
            vec![(ChildKey::Name("node"), NodeMut::Expr(&mut self.0))]
        }

        node_boilerplate!(CustomExpr);
    }

    struct ParenFactory;

    impl UnaryNodeFactory for ParenFactory {
        fn class_name(&self) -> &str {
            "ParenExpression"
        }

        fn create(&self, node: Expr, line: i64) -> twig::Result<Expr> {
            Ok(Expr::custom(Paren(node), line))
        }
    }

    struct Operators;

    impl Extension for Operators {
        fn expression_parsers(&self) -> Vec<ExpressionParserEntry> {
            vec![ExpressionParserEntry::prefix(
                UnaryOperatorExpressionParser::new(
                    UnaryNodeClass::Custom(Arc::new(ParenFactory)),
                    "same",
                    500,
                ),
            )]
        }

        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    let env = Environment::new(ArrayLoader::new([("index", "{{ same x }}")]));
    env.add_extension(Arc::new(Operators)).unwrap();
    assert_eq!(
        env.render("index", context! { x => "<b>" }).unwrap(),
        "&lt;b&gt;"
    );
}

#[test]
fn programs_list_their_runtimes() {
    let env = Environment::new(ArrayLoader::new([("index", "{{ up x }}{{ up y }}")]));
    env.add_extension(Arc::new(CustomNodesExtension)).unwrap();
    let source = twig::Source::new("{{ up x }}{{ up y }}", "index");
    let program = env.compile_source(&source).unwrap();
    // one runtime, called twice
    assert_eq!(program.runtimes().len(), 1);
    assert!(program.runtimes()[0].ptr_eq(&STRTOUPPER));
    let listing = program.disassemble();
    assert_eq!(
        listing.matches("CallRuntime #0 argc=1").count(),
        2,
        "{listing}"
    );
}

/// `{% filter_print value name(arguments) %}`: prints the call of the filter `name`, found by the token parser.
#[derive(Debug, Clone, PartialEq)]
struct FilterPrint {
    filter: TwigFilter,
    value: Expr,
    arguments: Arguments,
    line: i64,
}

impl CustomNode for FilterPrint {
    fn class_name(&self) -> &str {
        "FilterPrintNode"
    }

    fn children(&self) -> Vec<(ChildKey<'_>, NodeRef<'_>)> {
        vec![
            (ChildKey::Name("value"), NodeRef::Expr(&self.value)),
            (
                ChildKey::Name("arguments"),
                NodeRef::Arguments(&self.arguments),
            ),
        ]
    }

    fn children_mut(&mut self) -> Vec<(ChildKey<'_>, NodeMut<'_>)> {
        vec![
            (ChildKey::Name("value"), NodeMut::Expr(&mut self.value)),
            (
                ChildKey::Name("arguments"),
                NodeMut::Arguments(&mut self.arguments),
            ),
        ]
    }

    fn compile<'t>(&'t self, compiler: &mut Compiler<'t, '_>) -> twig::Result<()> {
        compiler.add_debug_info(self.line);
        let filter = TwigCallable::Filter(self.filter.clone());
        compiler.call(&filter, Some(&self.value), Some(&self.arguments), self.line)?;
        compiler.print()
    }

    node_boilerplate!(CustomNode);
}

struct FilterPrintTokenParser;

impl TokenParser for FilterPrintTokenParser {
    fn tag(&self) -> &str {
        "filter_print"
    }

    fn parse(&self, parser: &mut Parser<'_>, token: &Token) -> twig::Result<Option<Node>> {
        let line = token.line();
        let value = parser.parse_expression(0)?;
        let name = parser.stream_mut().expect(TokenType::Name, None, None)?;
        let filter = parser.get_filter(name.value().as_bytes().unwrap_or_default(), line)?;
        let arguments = if parser
            .stream()
            .current()
            .test_value(TokenType::Operator, "(")
        {
            parser.parse_named_arguments(true)?
        } else {
            Arguments::new()
        };
        expect_block_end(parser)?;
        let node = FilterPrint {
            filter,
            value,
            arguments,
            line,
        };
        Ok(Some(Node::custom(node, line)))
    }
}

#[test]
fn registered_callables_bind_their_arguments_while_compiling() {
    let render = |template: &str| {
        let env = Environment::new(ArrayLoader::new([("index", template)]));
        env.add_token_parser(Arc::new(FilterPrintTokenParser))
            .unwrap();
        env.render("index", context! { x => "<b> a,b ", y => "a" })
            .map_err(|error| error.to_string())
    };
    // printed as the node's code prints it: not escaped
    assert_eq!(render("{% filter_print x upper %}").unwrap(), "<B> A,B ");
    assert_eq!(
        render("{% filter_print x trim(side: 'right') %}|").unwrap(),
        "<b> a,b|"
    );
    assert_eq!(
        render("{% filter_print x replace({(y): 'c'}) %}").unwrap(),
        "<b> c,b "
    );
    // the binding errors of a call written in the template
    assert_eq!(
        render("{% filter_print x upper(nope: 1) %}"),
        render("{{ x|upper(nope: 1) }}")
    );
    assert!(render("{% filter_print x upper(nope: 1) %}").is_err());
}
