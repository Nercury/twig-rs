//! Twig: `Twig\NodeVisitor\SandboxNodeVisitor` (TWIG/src/NodeVisitor/SandboxNodeVisitor.php) and the parts of
//! `Twig\Util\CallableParameters` (TWIG/src/Util/CallableParameters.php) and
//! `Twig\Util\ReflectionCallable::getTwigParameters()` (TWIG/src/Util/ReflectionCallable.php) it uses.

use std::collections::HashMap;

use super::safe_analysis::is_operator_escape;
use super::{NodeVisitor, Visit, VisitContext, VisitResult, priority};
use crate::ast::{
    ArgumentKey, Arguments, BinaryOp, CallType, CheckSecurityNode, ChildKey, Expr, ExprKind,
    FilterExpr, FilterKind, MacroName, Node, NodeKind, NodeMut, NodeRef, Nodes, TestKind, UnaryOp,
};
use crate::error::{Error, Result};
use crate::extension::{CallableParameter, ExtensionSet, PhpCallable, TwigCallable, TwigFilter};

/// Adds the security checks of the sandbox (priority 0, registered by `SandboxExtension`, after the escaper):
///
/// - collects the tags, filters, functions (also the functions a `parser_callable` turned into another node, and
///   `range` for the `..` operator) and tests a template uses, with the line of their first use, skipping those
///   that are always allowed in the sandbox; they end up in a `CheckSecurityNode` appended to the module's
///   `class_end`, and a `CheckSecurityCallNode` is prepended to its `constructor_start`;
/// - wraps the children that a node coerces to a string at runtime (`CoercesChildrenToStringInterface`) in
///   `CheckToStringNode`s, so that an implicit `__toString()` call goes through the policy: variables, attribute
///   accesses, calls and assignments are wrapped, arrays, call arguments and the operands of `?:`, `??` and ternaries
///   are searched, spreads get a spread check, `_self` is never wrapped, and a filter, function or test argument is
///   not wrapped when the PHP parameter receiving it has a type that cannot be coerced to a string.
///
/// Twig: `Twig\NodeVisitor\SandboxNodeVisitor`.
///
/// The PHP parameter types come from reflection in Twig. twig-rs callables carry no PHP types, so only the
/// parameters of Twig's own callables whose type is string-coercion safe are known (the `num` of `abs`, the
/// `flags` and `depth` of `json_encode`, the `step` of `range`, the `arrow` of `invoke`, the `checkDefined` of
/// `constant`, the `ignoreMissing` of `include_only`, the `autoescape` of `escape`); the arguments of other callables
/// are wrapped as for untyped PHP parameters. Node classes of extensions are not wrapped either.
#[derive(Debug, Clone, Copy, Default)]
pub struct SandboxNodeVisitor;

impl NodeVisitor for SandboxNodeVisitor {
    fn class_name(&self) -> &str {
        "Twig\\NodeVisitor\\SandboxNodeVisitor"
    }

    /// 0. Twig: `SandboxNodeVisitor::getPriority()`.
    fn priority(&self) -> i64 {
        priority::SANDBOX
    }

    fn start<'a>(&'a self, extensions: &'a ExtensionSet) -> Box<dyn Visit + 'a> {
        Box::new(SandboxPass {
            extensions,
            in_a_module: false,
            tags: Used::default(),
            filters: Used::default(),
            functions: Used::default(),
            tests: Used::default(),
            clone_roots: Vec::new(),
            clone_depth: 0,
        })
    }
}

/// Names with the line of their first use, in order (a PHP `array<string, int>` filled with `isset()` checks).
#[derive(Default)]
struct Used(Vec<(Vec<u8>, i64)>);

impl Used {
    fn contains(&self, name: &[u8]) -> bool {
        self.0.iter().any(|(used, _)| used == name)
    }

    fn insert(&mut self, name: &[u8], line: i64) {
        self.0.push((name.to_vec(), line));
    }
}

/// The state of `SandboxNodeVisitor` during one traversal.
struct SandboxPass<'a> {
    extensions: &'a ExtensionSet,
    in_a_module: bool,
    tags: Used,
    filters: Used,
    functions: Used,
    tests: Used,
    /// The identities (never dereferenced) of the expressions PHP builds as deep clones (see [`clone_roots_of`]),
    /// registered when the pass enters their owner.
    clone_roots: Vec<*const Expr>,
    /// How many clone roots enclose the node being visited: inside a clone, PHP shares no node objects.
    clone_depth: usize,
}

impl Visit for SandboxPass<'_> {
    /// Twig: `SandboxNodeVisitor::enterNode()`.
    fn enter(&mut self, mut node: NodeMut<'_>, _cx: &mut VisitContext<'_>) -> Result<()> {
        if let NodeMut::Node(n) = &node
            && matches!(n.kind, NodeKind::Module(_))
        {
            self.in_a_module = true;
            self.tags = Used::default();
            self.filters = Used::default();
            self.functions = Used::default();
            self.tests = Used::default();
        } else if self.in_a_module {
            self.collect(&node)?;
        }

        // wrap children that the node itself will string-coerce at runtime; applies to ModuleNode (`parent` slot
        // for {% extends %}) too
        if self.in_a_module {
            coerce_children(&mut node)?;
        }

        if let Some(expr) = node.as_expr_mut() {
            if self.clone_roots.contains(&std::ptr::from_ref(&*expr)) {
                self.clone_depth += 1;
            }
            clone_roots_of(expr, &mut self.clone_roots);
        }
        Ok(())
    }

    /// Twig: `SandboxNodeVisitor::leaveNode()`, plus the merge of the copies of node objects PHP shares (see
    /// [`merge_shared_copies`]).
    fn leave(&mut self, mut node: NodeMut<'_>, _cx: &mut VisitContext<'_>) -> Result<VisitResult> {
        if let Some(expr) = node.as_expr_mut() {
            if self.clone_depth == 0 {
                merge_shared_copies(expr);
            }
            let id = std::ptr::from_ref(&*expr);
            if let Some(index) = self.clone_roots.iter().position(|root| *root == id) {
                self.clone_roots.swap_remove(index);
                self.clone_depth -= 1;
            }
        }
        if let NodeMut::Node(n) = node
            && let NodeKind::Module(module) = &mut n.kind
        {
            self.in_a_module = false;
            let constructor_start =
                std::mem::replace(&mut module.constructor_start, Node::empty(0));
            module.constructor_start = Node::nodes(
                Nodes::from_list([Node::new(NodeKind::CheckSecurityCall, 0), constructor_start]),
                0,
            );
            let check = CheckSecurityNode {
                used_filters: std::mem::take(&mut self.filters.0),
                used_tags: std::mem::take(&mut self.tags.0),
                used_functions: std::mem::take(&mut self.functions.0),
                used_tests: std::mem::take(&mut self.tests.0),
            };
            let class_end = std::mem::replace(&mut module.class_end, Node::empty(0));
            module.class_end = Node::nodes(
                Nodes::from_list([
                    Node::new(NodeKind::CheckSecurity(Box::new(check)), 0),
                    class_end,
                ]),
                0,
            );
        }
        Ok(VisitResult::Keep)
    }
}

impl SandboxPass<'_> {
    /// The collection part of `enterNode()` (inside a module): tags, filters, functions, tests, parser-callable
    /// functions and the `..` operator.
    fn collect(&mut self, node: &NodeMut<'_>) -> Result<()> {
        // look for tags
        if let NodeMut::Node(n) = node
            && let Some(tag) = n.tag.as_deref().filter(|t| !t.is_empty() && *t != "0")
            && !self.tags.contains(tag.as_bytes())
            && !self.is_tag_always_allowed(tag)?
        {
            self.tags.insert(tag.as_bytes(), n.template_line());
        }

        let Some(expr) = node.as_ref().as_expr() else {
            return Ok(());
        };
        let line = expr.line;
        match &expr.kind {
            // look for filters
            ExprKind::Filter(filter)
                if !self.filters.contains(filter.filter.name())
                    && !filter.filter.is_always_allowed_in_sandbox() =>
            {
                self.filters.insert(filter.filter.name(), line);
            }
            // look for functions
            ExprKind::Function(function)
                if !self.functions.contains(function.function.name())
                    && !function.function.is_always_allowed_in_sandbox() =>
            {
                self.functions.insert(function.function.name(), line);
            }
            // look for tests
            ExprKind::Test(test)
                if !self.tests.contains(test.test.name())
                    && !test.test.is_always_allowed_in_sandbox() =>
            {
                self.tests.insert(test.test.name(), line);
            }
            _ => {}
        }

        // look for functions whose parser callable replaced the FunctionExpression with a specialized node (e.g.
        // `parent`, `block`, `attribute`); the original function name was stashed by FunctionExpressionParser.
        if let Some(sandboxed) = &expr.sandboxed_function
            && !self.functions.contains(&sandboxed.name)
            && !sandboxed.function.is_always_allowed_in_sandbox()
        {
            self.functions.insert(&sandboxed.name, line);
        }

        // the .. operator is equivalent to the range() function
        if let ExprKind::Binary(binary) = &expr.kind
            && binary.op == BinaryOp::Range
            && !self.functions.contains(b"range")
            && !self.is_function_name_always_allowed(b"range")?
        {
            self.functions.insert(b"range", line);
        }
        Ok(())
    }

    /// Twig: `SandboxNodeVisitor::isTagAlwaysAllowedInSandbox()`.
    fn is_tag_always_allowed(&self, tag: &str) -> Result<bool> {
        Ok(self
            .extensions
            .get_token_parser(tag.as_bytes())?
            .is_some_and(|parser| parser.is_always_allowed_in_sandbox()))
    }

    /// Twig: `SandboxNodeVisitor::isFunctionNameAlwaysAllowedInSandbox()`.
    fn is_function_name_always_allowed(&self, name: &[u8]) -> Result<bool> {
        Ok(self
            .extensions
            .get_function(name)?
            .is_some_and(|function| function.is_always_allowed_in_sandbox()))
    }
}

/// The `CoercesChildrenToStringInterface` part of `enterNode()`: wraps the children the node coerces to a string
/// (`getStringCoercedChildNames()`), consulting the PHP parameters of filter, function and test calls.
fn coerce_children(node: &mut NodeMut<'_>) -> Result<()> {
    match node {
        NodeMut::Node(n) => match &mut n.kind {
            // the parent expression is resolved through the loader, which coerces it to a string
            NodeKind::Module(module) => {
                if let Some(parent) = &mut module.parent {
                    wrap_node(parent);
                }
            }
            NodeKind::Print(print) => wrap_node(&mut print.expr),
            // the loader resolves the template-name expression by coercing it to a string (an embed is an include)
            NodeKind::Include(include) => wrap_node(&mut include.expr),
            NodeKind::Embed(embed) => wrap_node(&mut embed.expr),
            NodeKind::Import(import) => wrap_node(&mut import.expr),
            // the message is formatted by `sprintf()`, and `package` / `version` are typed `string` on
            // trigger_deprecation()
            NodeKind::Deprecated(deprecated) => {
                let deprecated = &mut **deprecated;
                wrap_node(&mut deprecated.expr);
                if let Some(package) = &mut deprecated.package {
                    wrap_node(package);
                }
                if let Some(version) = &mut deprecated.version {
                    wrap_node(version);
                }
            }
            NodeKind::Expression(expr) => coerce_expr_children(expr)?,
            NodeKind::Custom(custom) => {
                let names = custom.string_coerced_child_names();
                if !names.is_empty() {
                    wrap_named_children(custom.children_mut(), &names);
                }
            }
            _ => {}
        },
        NodeMut::Expr(expr) => coerce_expr_children(expr)?,
        NodeMut::Arguments(_) => {}
    }
    Ok(())
}

/// Twig: `wrapNode($node, $name)` for each child a node class defined by an extension lists in
/// `getStringCoercedChildNames()`: an expression is wrapped like any other, a container (`Nodes`, an argument list)
/// element by element.
fn wrap_named_children(children: Vec<(ChildKey<'_>, NodeMut<'_>)>, names: &[String]) {
    for (key, child) in children {
        let key = key.to_string();
        if !names.contains(&key) {
            continue;
        }
        match child {
            NodeMut::Expr(expr) => wrap_node(expr),
            NodeMut::Arguments(arguments) => {
                for (_, argument) in arguments.entries_mut() {
                    wrap_node(argument);
                }
            }
            NodeMut::Node(node) => match &mut node.kind {
                NodeKind::Expression(expr) => wrap_node(expr),
                NodeKind::Nodes(nodes) => {
                    for (_, element) in nodes.entries_mut() {
                        if let Some(expr) = element.as_expr_mut() {
                            wrap_node(expr);
                        }
                    }
                }
                _ => {}
            },
        }
    }
}

/// [`coerce_children`] for the expression classes implementing `CoercesChildrenToStringInterface`.
fn coerce_expr_children(expr: &mut Expr) -> Result<()> {
    let class = expr.class_name();
    match &mut expr.kind {
        // dynamic mapping keys (computed at runtime) are coerced to string; static keys (constants or sequence
        // indexes) are emitted as PHP literals by compile() and never trigger a __toString() call
        ExprKind::Array(array) => {
            for (key, _) in &mut array.elements {
                if !matches!(
                    key.kind,
                    ExprKind::Constant(_) | ExprKind::LocalVariable(_) | ExprKind::MacroVariable(_)
                ) {
                    wrap_node(key);
                }
            }
        }
        ExprKind::Binary(binary) if coerces_operands(binary.op) => {
            let binary = &mut **binary;
            wrap_node(&mut binary.left);
            wrap_node(&mut binary.right);
        }
        // the template expression is resolved through the loader, which coerces it to a string
        ExprKind::BlockReference(reference) => {
            if let Some(template) = &mut reference.template {
                wrap_node(template);
            }
        }
        // a filter may coerce its input and arguments to string (e.g. `upper`, `replace`)
        ExprKind::Filter(filter) => {
            let filter = &mut **filter;
            let callable = TwigCallable::Filter(filter.filter.clone());
            let params = CallParameters::of(&callable);
            // The filter's input value maps to the first PHP parameter.
            if !params.as_ref().is_some_and(|p| p.is_safe(0)) {
                wrap_node(&mut filter.node);
            }
            coerce_arguments(&mut filter.arguments, params.as_ref(), true);
        }
        // a function may coerce its arguments to string (the host PHP code is opaque to Twig)
        ExprKind::Function(function) => {
            let callable = TwigCallable::Function(function.function.clone());
            let params = CallParameters::of(&callable);
            coerce_arguments(&mut function.arguments, params.as_ref(), false);
        }
        ExprKind::GetAttr(get_attr) => {
            let get_attr = &mut **get_attr;
            // the host PHP method may coerce any argument to string
            if let Some(arguments) = &mut get_attr.arguments {
                wrap_node(arguments);
            }
            // compileArrayKey() may coerce a Stringable key; expose it so the sandbox checks __toString()
            if get_attr.call_type == CallType::Array {
                wrap_node(&mut get_attr.attribute);
            }
        }
        // Dynamic macro names are string-coerced at runtime.
        ExprKind::MacroReference(reference) => {
            if let MacroName::Dynamic(name) = &mut reference.name {
                wrap_node(name);
            }
        }
        ExprKind::Test(test) => {
            let test = &mut **test;
            let (coerce_node, coerce_arguments_child) = match test.kind {
                // Custom node classes must declare their own coercion behavior.
                TestKind::Test => (true, test.arguments.is_some()),
                TestKind::NonStringCoercingInput => (false, test.arguments.is_some()),
                // `ConstantTest::getStringCoercedChildNames()` lists `arguments` unconditionally
                TestKind::Constant => (false, true),
                TestKind::Defined
                | TestKind::DivisibleBy
                | TestKind::Even
                | TestKind::Null
                | TestKind::Odd
                | TestKind::SameAs
                | TestKind::True => (false, false),
            };
            let callable = TwigCallable::Test(test.test.clone());
            let params = CallParameters::of(&callable);
            if coerce_node && !params.as_ref().is_some_and(|p| p.is_safe(0)) {
                wrap_node(&mut test.node);
            }
            if coerce_arguments_child {
                let Some(arguments) = &mut test.arguments else {
                    // `$node->getNode('arguments')` on a node without the child
                    return Err(Error::logic(format!(
                        "Node \"arguments\" does not exist for Node \"{class}\"."
                    )));
                };
                coerce_arguments(arguments, params.as_ref(), true);
            }
        }
        ExprKind::Custom(custom) => {
            let names = custom.string_coerced_child_names();
            if !names.is_empty() {
                wrap_named_children(custom.children_mut(), &names);
            }
        }
        _ => {}
    }
    Ok(())
}

/// Whether a binary class implements `CoercesChildrenToStringInterface` (both operands are coerced).
fn coerces_operands(op: BinaryOp) -> bool {
    matches!(
        op,
        BinaryOp::Concat
            | BinaryOp::Equal
            | BinaryOp::NotEqual
            | BinaryOp::Greater
            | BinaryOp::GreaterEqual
            | BinaryOp::Less
            | BinaryOp::LessEqual
            | BinaryOp::In
            | BinaryOp::NotIn
            | BinaryOp::Matches
            | BinaryOp::Range
            | BinaryOp::Spaceship
    )
}

/// The `arguments` child of a filter, function or test call: per-argument checks when the PHP parameters are
/// known (`wrapArguments()`), else every argument (`wrapNode()` on the container).
fn coerce_arguments(
    arguments: &mut Arguments,
    params: Option<&CallParameters<'_>>,
    has_node: bool,
) {
    match params {
        Some(params) => wrap_arguments(arguments, params, has_node),
        None => {
            for (_, argument) in arguments.entries_mut() {
                wrap_node(argument);
            }
        }
    }
}

/// The PHP parameters behind a filter, function or test call, without the parameters Twig injects.
///
/// Twig: `CallableParameters::fromNode()` / `ReflectionCallable::getTwigParameters()`.
struct CallParameters<'c> {
    callable: &'c PhpCallable,
    params: Vec<&'c CallableParameter>,
}

impl<'c> CallParameters<'c> {
    /// `None` for a callable without a PHP callable (Twig's `null`).
    fn of(callable: &'c TwigCallable) -> Option<Self> {
        let php = callable.callable()?;
        let mut params: Vec<&CallableParameter> = php.parameters().iter().collect();
        // array_shift() for each injected parameter, then for each dynamic-name argument
        let injected = usize::from(callable.needs_charset())
            + usize::from(callable.needs_environment())
            + usize::from(callable.needs_context())
            + usize::from(callable.needs_is_sandboxed())
            + callable.dynamic_arguments().len();
        params.drain(..injected.min(params.len()));
        Some(CallParameters {
            callable: php,
            params,
        })
    }

    /// Whether the parameter at `index` exists and has a string-coercion safe type.
    fn is_safe(&self, index: usize) -> bool {
        self.params
            .get(index)
            .is_some_and(|param| is_string_coercion_safe(self.callable, param))
    }
}

/// The parameters of Twig's callables whose PHP type proves that the value reaching them cannot implicitly
/// string-coerce (`int`, `float`, `bool`, `null`, `Closure`...), by reflection name and parameter name (dumped from
/// the oracle with `CallableParameters::isStringCoercionSafe()`).
const COERCION_SAFE_PARAMETERS: [(&str, &str); 8] = [
    ("abs", "num"),
    ("json_encode", "flags"),
    ("json_encode", "depth"),
    ("range", "step"),
    ("Twig\\Extension\\CoreExtension::invoke", "arrow"),
    ("Twig\\Extension\\CoreExtension::constant", "checkDefined"),
    (
        "Twig\\Extension\\CoreExtension::includeOnly",
        "ignoreMissing",
    ),
    ("Twig\\Runtime\\EscaperRuntime::escape", "autoescape"),
];

/// Twig: `CallableParameters::isStringCoercionSafe()` for a parameter of `callable`: a parameter declared safe
/// ([`CallableParameter::string_coercion_safe`]), or one of Twig's own safe parameters; parameters without a known
/// PHP type count as untyped (not safe).
fn is_string_coercion_safe(callable: &PhpCallable, param: &CallableParameter) -> bool {
    if param.coercion_safe {
        return true;
    }
    let name = callable.reflection_name();
    COERCION_SAFE_PARAMETERS
        .iter()
        .any(|(function, parameter)| *function == name && *parameter == param.name)
}

/// Twig: `SandboxNodeVisitor::wrapArguments()`: wraps each argument unless the PHP parameter receiving it has a
/// string-coercion safe type; the input of filters and tests is the first PHP parameter, so their template
/// arguments start at the second one.
fn wrap_arguments(arguments: &mut Arguments, params: &CallParameters<'_>, has_node: bool) {
    let positional: &[&CallableParameter] =
        params.params.get(usize::from(has_node)..).unwrap_or(&[]);
    let mut variadic: Option<&CallableParameter> = None;
    let mut by_name: HashMap<Vec<u8>, &CallableParameter> = HashMap::new();
    for param in positional {
        if param.variadic {
            variadic = Some(param);
            break;
        }
        by_name
            .entry(normalize_name(param.name.as_bytes()))
            .or_insert(param);
    }
    let safe = |param: &CallableParameter| is_string_coercion_safe(params.callable, param);

    let mut positional_index = 0;
    for (key, argument) in arguments.entries_mut() {
        let is_spread =
            matches!(&argument.kind, ExprKind::Unary(unary) if unary.op == UnaryOp::Spread);
        if matches!(key, ArgumentKey::Index(_)) && is_spread {
            // A spread can fill any parameter from its position on, so the check is skipped only when all of them
            // have a safe type.
            let remaining = positional.get(positional_index..).unwrap_or(&[]);
            let all_safe = !remaining.is_empty() && remaining.iter().all(|param| safe(param));
            if !all_safe {
                wrap_node(argument);
            }
            positional_index = positional.len();
            continue;
        }

        let param = match key {
            ArgumentKey::Index(_) => {
                let param = positional.get(positional_index).copied().or(variadic);
                if param.is_some_and(|param| !param.variadic) {
                    positional_index += 1;
                }
                param
            }
            ArgumentKey::Name(name) => by_name.get(&normalize_name(name)).copied().or(variadic),
        };
        if param.is_some_and(safe) {
            continue;
        }
        wrap_node(argument);
    }
}

/// Twig: `SandboxNodeVisitor::normalizeName()`: `strtolower(str_replace('_', '', $name))`.
fn normalize_name(name: &[u8]) -> Vec<u8> {
    name.iter()
        .filter(|&&b| b != b'_')
        .map(u8::to_ascii_lowercase)
        .collect()
}

/// Wraps an expression the parent coerces to a string: variables (but `_self`) and attribute accesses, calls and
/// assignments get a `CheckToStringNode`, the element of a spread gets a spread check, arrays and the operands of
/// `?:`, `??` and ternaries are searched.
///
/// Twig: `SandboxNodeVisitor::wrapNode()` (core expressions are never generators).
fn wrap_node(slot: &mut Expr) {
    match &mut slot.kind {
        // `_self` is internal: it compiles to `$this->getTemplateName()` and is always a string
        ExprKind::ContextVariable(name) | ExprKind::AssignContextVariable(name)
            if name.name == b"_self" => {}
        ExprKind::ContextVariable(_)
        | ExprKind::AssignContextVariable(_)
        | ExprKind::GetAttr(_)
        | ExprKind::Filter(_)
        | ExprKind::Function(_)
        | ExprKind::Test(_) => wrap_in_check(slot, false),
        ExprKind::Binary(binary)
            if matches!(
                binary.op,
                BinaryOp::Set
                    | BinaryOp::SequenceDestructuringSet
                    | BinaryOp::ObjectDestructuringSet
            ) =>
        {
            wrap_in_check(slot, false);
        }
        ExprKind::Unary(unary) if unary.op == UnaryOp::Spread => {
            wrap_in_check(&mut unary.node, true)
        }
        ExprKind::Array(array) => {
            for (key, value) in &mut array.elements {
                wrap_node(key);
                wrap_node(value);
            }
        }
        ExprKind::ConditionalTernary(ternary) => {
            let ternary = &mut **ternary;
            wrap_node(&mut ternary.left);
            wrap_node(&mut ternary.right);
        }
        ExprKind::Binary(binary) if is_operator_escape(binary.op) => {
            let binary = &mut **binary;
            wrap_node(&mut binary.left);
            wrap_node(&mut binary.right);
        }
        _ => {}
    }
}

/// `new CheckToStringNode($expr, $spread)` in place of `$expr`.
fn wrap_in_check(slot: &mut Expr, spread: bool) {
    let expr = std::mem::replace(slot, Expr::empty(0));
    *slot = Expr::check_to_string(expr, spread);
}

/// Reproduces PHP's sharing of node objects that the twig-rs AST holds as two copies, once the pass leaves the node
/// owning both copies:
///
/// - `NullCoalesceBinary` uses its left operand object both as `left` and as the operand of the `null` test of its
///   `test` child;
/// - `DefaultFilter` passes its `arguments` object both to itself and to the inner `default` filter of its `node`.
///
/// In PHP the visitor processes the one object at each of its places, on top of what earlier places did, and the
/// result shows in both places. Every step of the visitor is idempotent except the wrapping of a spread operand
/// (`wrapNode()` wraps the operand of a `SpreadUnary` again each time), and the copy that is not reached by the
/// wrapping of the parent (the operand of the null test) misses the wrappers added there. So the merged object is the
/// copy at the first place with, for each spread, the spread checks of both copies added up; both places get it.
/// Nodes inside a deep clone made by a node constructor share nothing ([`clone_roots_of`]) and are not merged.
fn merge_shared_copies(expr: &mut Expr) {
    match &mut expr.kind {
        ExprKind::Binary(binary) if binary.op == BinaryOp::NullCoalesce => {
            let binary = &mut **binary;
            // `test` is `defined(clone left) and not (left is null)` (no null test for `block()`)
            let Some(test) = &mut binary.test else {
                return;
            };
            let ExprKind::Binary(and) = &mut test.kind else {
                return;
            };
            let ExprKind::Unary(not) = &mut and.right.kind else {
                return;
            };
            let ExprKind::Test(null) = &mut not.node.kind else {
                return;
            };
            if null.kind != TestKind::Null {
                return;
            }
            // the left operand supports the `defined` test, so it is not a filter: a filter around it is the
            // escaper's
            let Some(left) = shared_left_operand(&mut binary.left) else {
                return;
            };
            if add_spread_checks(NodeRef::Expr(&null.node), NodeMut::Expr(left)) {
                null.node = left.clone();
            }
        }
        ExprKind::Filter(filter) if filter.kind == FilterKind::Default => {
            let filter = &mut **filter;
            let Some(inner) = inner_default_filter(&mut filter.node, &filter.filter) else {
                return;
            };
            if add_spread_checks(
                NodeRef::Arguments(&inner.arguments),
                NodeMut::Arguments(&mut filter.arguments),
            ) {
                inner.arguments = filter.arguments.clone();
            }
        }
        _ => {}
    }
}

/// Registers the children of `expr` that PHP builds as deep clones (`clone $node`, which does not keep the sharing
/// of the copied subtree): the `test` of `?:` (`TrueTest::wrap(clone $left)`), the operand of the `defined` test of
/// `??`, and the operand of the `defined` test and the fallback branch of the ternary a `default` filter builds for a
/// variable or an attribute (`clone $node`, `clone $arguments->getNode('0')`).
fn clone_roots_of(expr: &Expr, roots: &mut Vec<*const Expr>) {
    let defined_operand = |test: &Expr| match &test.kind {
        ExprKind::Test(defined) if defined.kind == TestKind::Defined => {
            Some(std::ptr::from_ref(&defined.node))
        }
        _ => None,
    };
    match &expr.kind {
        ExprKind::Binary(binary) if binary.op == BinaryOp::Elvis => {
            roots.extend(binary.test.as_ref().map(std::ptr::from_ref));
        }
        ExprKind::Binary(binary) if binary.op == BinaryOp::NullCoalesce => {
            // `defined(clone left) and not (left is null)`, or `defined(clone left)` for `block()`
            let defined = match binary.test.as_ref().map(|test| &test.kind) {
                Some(ExprKind::Binary(and)) if and.op == BinaryOp::And => {
                    defined_operand(&and.left)
                }
                Some(_) => binary.test.as_ref().and_then(defined_operand),
                None => None,
            };
            roots.extend(defined);
        }
        ExprKind::Filter(filter) if filter.kind == FilterKind::Default => {
            if let ExprKind::ConditionalTernary(ternary) = &filter.node.kind
                && let Some(defined) = defined_operand(&ternary.test)
            {
                roots.push(defined);
                roots.push(std::ptr::from_ref(&ternary.right));
            }
        }
        _ => {}
    }
}

/// The left operand object of a `??` under the wrappers added to the `left` slot since parsing (`CheckToStringNode`s
/// and the escaper's filter).
fn shared_left_operand(slot: &mut Expr) -> Option<&mut Expr> {
    match slot.kind {
        ExprKind::CheckToString(_) | ExprKind::Filter(_) => {}
        ExprKind::Custom(_) => return None,
        _ => return Some(slot),
    }
    match &mut slot.kind {
        ExprKind::CheckToString(check) => shared_left_operand(&mut check.expr),
        ExprKind::Filter(filter) => shared_left_operand(&mut filter.node),
        _ => None,
    }
}

/// The inner filter node of a `DefaultFilter` (`$default`: the same filter, as its `node` or as the `left` of the
/// ternary of the rewrite), under the wrappers added since parsing.
fn inner_default_filter<'e>(slot: &'e mut Expr, filter: &TwigFilter) -> Option<&'e mut FilterExpr> {
    let is_inner = matches!(
        &slot.kind,
        ExprKind::Filter(inner) if inner.kind == FilterKind::Filter && inner.filter == *filter
    );
    match &mut slot.kind {
        ExprKind::CheckToString(check) => inner_default_filter(&mut check.expr, filter),
        ExprKind::ConditionalTernary(ternary) => inner_default_filter(&mut ternary.left, filter),
        ExprKind::Filter(inner) => {
            if is_inner {
                Some(inner)
            } else {
                inner_default_filter(&mut inner.node, filter)
            }
        }
        _ => None,
    }
}

/// Adds the spread checks of `other` to those of `merged` (spreads matched in order): returns whether both copies
/// have the same spreads, in which case `merged` holds the result.
fn add_spread_checks(other: NodeRef<'_>, merged: NodeMut<'_>) -> bool {
    let mut others = Vec::new();
    spread_check_counts(other, &mut others);
    let mut counts = Vec::new();
    spread_check_counts(merged.as_ref(), &mut counts);
    if counts.len() != others.len() {
        return false;
    }
    let mut sums = counts.iter().zip(&others).map(|(a, b)| a + b);
    set_spread_check_counts(merged, &mut sums);
    true
}

/// The number of spread checks (`CheckToStringNode`s with `spread`) around the operand of every `SpreadUnary`, in
/// tree order.
fn spread_check_counts(node: NodeRef<'_>, out: &mut Vec<usize>) {
    if let Some(expr) = node.as_expr()
        && let ExprKind::Unary(unary) = &expr.kind
        && unary.op == UnaryOp::Spread
    {
        let mut count = 0;
        let mut operand = &unary.node;
        while let ExprKind::CheckToString(check) = &operand.kind
            && check.spread
        {
            count += 1;
            operand = &check.expr;
        }
        out.push(count);
    }
    for (_, child) in node.children() {
        spread_check_counts(child, out);
    }
}

/// Sets the number of spread checks around the operand of every `SpreadUnary`, in tree order.
fn set_spread_check_counts(mut node: NodeMut<'_>, counts: &mut impl Iterator<Item = usize>) {
    if let Some(expr) = node.as_expr_mut()
        && let ExprKind::Unary(unary) = &mut expr.kind
        && unary.op == UnaryOp::Spread
    {
        let count = counts.next().unwrap_or(0);
        let mut operand = std::mem::replace(&mut unary.node, Expr::empty(0));
        loop {
            match operand.kind {
                ExprKind::CheckToString(check) if check.spread => operand = check.expr,
                kind => {
                    operand.kind = kind;
                    break;
                }
            }
        }
        for _ in 0..count {
            operand = Expr::check_to_string(operand, true);
        }
        unary.node = operand;
    }
    for (_, child) in node.children_mut() {
        set_spread_check_counts(child, counts);
    }
}
