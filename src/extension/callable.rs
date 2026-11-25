//! Filters, functions and tests: `Twig\TwigFilter`, `Twig\TwigFunction`, `Twig\TwigTest` and their base class
//! `Twig\AbstractTwigCallable` (TWIG/src/).
//!
//! A callable is the metadata the parser and the compiler need (name, options, node class, the PHP parameter list
//! used to bind named arguments) plus a runtime slot ([`PhpCallable::runtime`]) that the runtime phase fills.

use std::any::Any;
use std::fmt;
use std::sync::{Arc, OnceLock};

use crate::ast::{Arguments, ConstantValue, Expr};
use crate::deprecation::{Deprecation, DeprecationSink};
use crate::error::Result;
use crate::parser::Parser;

/// The callable behind a Twig filter, function or test, described the way PHP reflection sees it.
///
/// Twig keeps a PHP callable and reflects on it (`Twig\Util\ReflectionCallable`) to bind named arguments and to
/// report missing ones. twig-rs registers that reflection data explicitly: the parameter list (including the
/// parameters Twig injects: charset, environment, context, sandbox state and the filtered/tested value) and the
/// callable's reflection name. The implementation itself goes into [`runtime`](Self::runtime), filled by the
/// runtime phase.
#[derive(Clone)]
pub struct PhpCallable {
    inner: Arc<PhpCallableInner>,
}

struct PhpCallableInner {
    kind: CallableKind,
    parameters: Vec<CallableParameter>,
    runtime: OnceLock<Arc<dyn Any + Send + Sync>>,
}

/// What kind of PHP callable stands behind a Twig callable; it decides the name Twig's reflection reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallableKind {
    /// A plain PHP function such as `abs` or `range` (a string callable).
    Function(String),
    /// A class method, reported as `Class::method`.
    Method {
        /// The class name, e.g. `Twig\Extension\CoreExtension`.
        class: String,
        /// The method name.
        method: String,
    },
    /// A closure, reported as `Closure`.
    Closure,
}

/// One PHP parameter of a callable, as `ReflectionParameter` describes it.
#[derive(Debug, Clone, PartialEq)]
pub struct CallableParameter {
    /// The parameter name without `$`.
    pub name: String,
    /// Whether and how the parameter can be omitted.
    pub default: ParameterDefault,
    /// A PHP variadic parameter (`...$values`).
    pub variadic: bool,
    /// The parameter is declared with the `array` type (`array $arg = []` is the variadic bucket of Twig's
    /// `is_variadic` callables).
    pub array_type: bool,
    /// The declared PHP type proves that a value reaching the parameter cannot be converted to a string implicitly
    /// (Twig: `CallableParameters::isStringCoercionSafe()`: `int`, `float`, `bool`, `null`, `true`, `false`, a
    /// final class without `__toString()`, unions of such types); see
    /// [`string_coercion_safe`](Self::string_coercion_safe).
    pub coercion_safe: bool,
}

/// The default of a [`CallableParameter`].
#[derive(Debug, Clone, PartialEq)]
pub enum ParameterDefault {
    /// The parameter is required.
    Required,
    /// The parameter is optional without a default value PHP can reflect (variadics, some internal functions).
    Optional,
    /// The parameter has this default value.
    Value(ConstantValue),
}

impl CallableParameter {
    /// A required parameter.
    pub fn required(name: &str) -> Self {
        CallableParameter {
            name: name.to_owned(),
            default: ParameterDefault::Required,
            variadic: false,
            array_type: false,
            coercion_safe: false,
        }
    }

    /// A parameter with a default value.
    pub fn optional(name: &str, default: ConstantValue) -> Self {
        CallableParameter {
            name: name.to_owned(),
            default: ParameterDefault::Value(default),
            variadic: false,
            array_type: false,
            coercion_safe: false,
        }
    }

    /// A PHP variadic parameter (`...$name`).
    pub fn variadic(name: &str) -> Self {
        CallableParameter {
            name: name.to_owned(),
            default: ParameterDefault::Optional,
            variadic: true,
            array_type: false,
            coercion_safe: false,
        }
    }

    /// Marks the parameter as declared with the `array` type.
    #[must_use]
    pub fn array(mut self) -> Self {
        self.array_type = true;
        self
    }

    /// Marks the parameter as typed so that no value reaching it can be converted to a string implicitly (an
    /// `int`, `float`, `bool` or `null` parameter, a final class without `__toString()`): the sandbox then does not
    /// wrap the argument passed to it in a `__toString()` check, as Twig does after reflecting the PHP type
    /// (`CallableParameters::isStringCoercionSafe()`).
    #[must_use]
    pub fn string_coercion_safe(mut self) -> Self {
        self.coercion_safe = true;
        self
    }

    /// `ReflectionParameter::isOptional()`.
    pub fn is_optional(&self) -> bool {
        !matches!(self.default, ParameterDefault::Required)
    }

    /// `ReflectionParameter::isDefaultValueAvailable()`.
    pub fn default_value(&self) -> Option<&ConstantValue> {
        match &self.default {
            ParameterDefault::Value(value) => Some(value),
            _ => None,
        }
    }
}

impl PhpCallable {
    /// A callable of the given kind with its full PHP parameter list.
    pub fn new(kind: CallableKind, parameters: Vec<CallableParameter>) -> Self {
        PhpCallable {
            inner: Arc::new(PhpCallableInner {
                kind,
                parameters,
                runtime: OnceLock::new(),
            }),
        }
    }

    /// A plain PHP function (`'abs'`).
    pub fn function(name: &str, parameters: Vec<CallableParameter>) -> Self {
        Self::new(CallableKind::Function(name.to_owned()), parameters)
    }

    /// A class method (`[Class::class, 'method']`).
    pub fn method(class: &str, method: &str, parameters: Vec<CallableParameter>) -> Self {
        Self::new(
            CallableKind::Method {
                class: class.to_owned(),
                method: method.to_owned(),
            },
            parameters,
        )
    }

    /// A closure.
    pub fn closure(parameters: Vec<CallableParameter>) -> Self {
        Self::new(CallableKind::Closure, parameters)
    }

    /// The kind of callable.
    pub fn kind(&self) -> &CallableKind {
        &self.inner.kind
    }

    /// All PHP parameters, injected ones included. Twig: `ReflectionFunctionAbstract::getParameters()`.
    pub fn parameters(&self) -> &[CallableParameter] {
        &self.inner.parameters
    }

    /// Twig: `ReflectionFunctionAbstract::getNumberOfRequiredParameters()`: the parameters up to the last required
    /// one.
    pub fn number_of_required_parameters(&self) -> usize {
        self.inner
            .parameters
            .iter()
            .rposition(|p| !p.is_optional())
            .map_or(0, |i| i + 1)
    }

    /// The name Twig's `ReflectionCallable::getName()` reports: `Class::method`, the function name or `Closure`.
    pub fn reflection_name(&self) -> String {
        match &self.inner.kind {
            CallableKind::Function(name) => name.clone(),
            CallableKind::Method { class, method } => format!("{class}::{method}"),
            CallableKind::Closure => "Closure".to_owned(),
        }
    }

    /// Whether this is the PHP function `name` (Twig compares string callables, e.g. `'range' === $callable`).
    pub fn is_php_function(&self, name: &str) -> bool {
        matches!(&self.inner.kind, CallableKind::Function(f) if f == name)
    }

    /// The runtime implementation, once the runtime phase has set it.
    pub fn runtime(&self) -> Option<&Arc<dyn Any + Send + Sync>> {
        self.inner.runtime.get()
    }

    /// Sets the runtime implementation (once; later calls return the rejected value).
    ///
    /// # Errors
    ///
    /// Returns `value` back when the slot is already filled.
    pub fn set_runtime(
        &self,
        value: Arc<dyn Any + Send + Sync>,
    ) -> std::result::Result<(), Arc<dyn Any + Send + Sync>> {
        self.inner.runtime.set(value)
    }
}

impl fmt::Debug for PhpCallable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PhpCallable")
            .field("kind", &self.inner.kind)
            .field("parameters", &self.inner.parameters)
            .field("runtime", &self.inner.runtime.get().is_some())
            .finish()
    }
}

/// Deprecation information of a callable. Twig: `Twig\DeprecatedCallableInfo` (TWIG/src/DeprecatedCallableInfo.php).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DeprecatedCallableInfo {
    /// The package that deprecated the callable (`trigger_deprecation()`'s `$package`).
    pub package: String,
    /// The version since which the callable is deprecated.
    pub version: String,
    /// The name of the replacement, if any.
    pub alt_name: Option<String>,
    /// The package of the replacement, if any.
    pub alt_package: Option<String>,
    /// The version of the replacement package, if any.
    pub alt_version: Option<String>,
}

impl DeprecatedCallableInfo {
    /// Twig: `new DeprecatedCallableInfo($package, $version)`.
    pub fn new(package: &str, version: &str) -> Self {
        DeprecatedCallableInfo {
            package: package.to_owned(),
            version: version.to_owned(),
            ..Default::default()
        }
    }

    /// The deprecation `triggerDeprecation()` raises for a callable of type `ty` (`filter`, `function`, `test`)
    /// named `name`, used in `file` (the source path, or its name when the path is empty) at `line`.
    ///
    /// Twig: `DeprecatedCallableInfo::triggerDeprecation()`.
    pub fn deprecation(
        &self,
        ty: &str,
        name: &[u8],
        file: Option<&str>,
        line: Option<i64>,
    ) -> Deprecation {
        let truthy = |s: &str| !s.is_empty() && s != "0";
        let mut message = format!(
            "Twig {} \"{}\" is deprecated",
            ucfirst(ty),
            String::from_utf8_lossy(name)
        );
        if let Some(alt_name) = self.alt_name.as_deref().filter(|s| truthy(s)) {
            message.push_str(&format!("; use \"{alt_name}\""));
            if let Some(alt_package) = self.alt_package.as_deref().filter(|s| truthy(s)) {
                message.push_str(&format!(" from the \"{alt_package}\" package"));
            }
            if let Some(alt_version) = self.alt_version.as_deref().filter(|s| truthy(s)) {
                message.push_str(&format!(" (available since version {alt_version})"));
            }
            message.push_str(" instead");
        }
        if let Some(file) = file.filter(|s| truthy(s)) {
            message.push_str(&format!(" in {file}"));
            if let Some(line) = line.filter(|&l| l != 0) {
                message.push_str(&format!(" at line {line}"));
            }
        }
        message.push('.');
        Deprecation::new(self.package.clone(), self.version.clone(), message)
    }
}

/// PHP `ucfirst()` (ASCII).
fn ucfirst(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}

/// Computes the strategies a filter or function call is safe for from its arguments (Twig's `is_safe_callback`).
pub type SafeCallback = Arc<dyn Fn(&Arguments) -> Vec<ConstantValue> + Send + Sync>;

/// Builds the node of a function call at parse time (Twig's `parser_callable` option): receives the parser, the
/// parsed arguments and the line of the `(` token.
pub type ParserCallable =
    Arc<dyn Fn(&mut Parser<'_>, Arguments, i64) -> Result<Expr> + Send + Sync>;

/// Creates the node of a filter call for a custom node class.
pub trait FilterNodeFactory: Send + Sync {
    /// The class name shown in dumps and error messages.
    fn class_name(&self) -> &str;
    /// Twig: `new $nodeClass($node, $filter, $arguments, $lineno)`.
    ///
    /// # Errors
    ///
    /// Whatever the node constructor reports.
    fn create(
        &self,
        node: Expr,
        filter: TwigFilter,
        arguments: Arguments,
        line: i64,
    ) -> Result<Expr>;
}

/// Creates the node of a function call for a custom node class.
pub trait FunctionNodeFactory: Send + Sync {
    /// The class name shown in dumps and error messages.
    fn class_name(&self) -> &str;
    /// Twig: `new $nodeClass($function, $arguments, $lineno)`.
    ///
    /// # Errors
    ///
    /// Whatever the node constructor reports.
    fn create(&self, function: TwigFunction, arguments: Arguments, line: i64) -> Result<Expr>;
}

/// Creates the node of a test for a custom node class.
pub trait TestNodeFactory: Send + Sync {
    /// The class name shown in dumps and error messages.
    fn class_name(&self) -> &str;
    /// Twig: `new $nodeClass($node, $test, $arguments, $lineno)`.
    ///
    /// # Errors
    ///
    /// Whatever the node constructor reports.
    fn create(
        &self,
        node: Expr,
        test: TwigTest,
        arguments: Option<Arguments>,
        line: i64,
    ) -> Result<Expr>;
}

/// The node class of a filter (Twig's `node_class` option).
#[derive(Clone, Default)]
pub enum FilterNodeClass {
    /// `Twig\Node\Expression\FilterExpression` (the default).
    #[default]
    Filter,
    /// `Twig\Node\Expression\Filter\DefaultFilter`.
    Default,
    /// `Twig\Node\Expression\Filter\EscapeFilter`.
    Escape,
    /// `Twig\Node\Expression\Filter\RawFilter`.
    Raw,
    /// A node class defined by an extension.
    Custom(Arc<dyn FilterNodeFactory>),
}

/// The node class of a function (Twig's `node_class` option).
#[derive(Clone, Default)]
pub enum FunctionNodeClass {
    /// `Twig\Node\Expression\FunctionExpression` (the default).
    #[default]
    Function,
    /// `Twig\Node\Expression\FunctionNode\EnumFunction`.
    Enum,
    /// `Twig\Node\Expression\FunctionNode\EnumCasesFunction`.
    EnumCases,
    /// `Twig\Node\Expression\FunctionNode\RenderSandboxedFunction`.
    RenderSandboxed,
    /// A node class defined by an extension.
    Custom(Arc<dyn FunctionNodeFactory>),
}

/// The node class of a test (Twig's `node_class` option).
#[derive(Clone, Default)]
pub enum TestNodeClass {
    /// `Twig\Node\Expression\TestExpression` (the default).
    #[default]
    Test,
    /// `Twig\Node\Expression\Test\ConstantTest`.
    Constant,
    /// `Twig\Node\Expression\Test\DefinedTest`.
    Defined,
    /// `Twig\Node\Expression\Test\DivisiblebyTest`.
    DivisibleBy,
    /// `Twig\Node\Expression\Test\EvenTest`.
    Even,
    /// `Twig\Node\Expression\Test\NonStringCoercingInputTest`.
    NonStringCoercingInput,
    /// `Twig\Node\Expression\Test\NullTest`.
    Null,
    /// `Twig\Node\Expression\Test\OddTest`.
    Odd,
    /// `Twig\Node\Expression\Test\SameasTest`.
    SameAs,
    /// `Twig\Node\Expression\Test\TrueTest`.
    True,
    /// A node class defined by an extension.
    Custom(Arc<dyn TestNodeFactory>),
}

macro_rules! debug_node_class {
    ($ty:ident { $($variant:ident),* }) => {
        impl fmt::Debug for $ty {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                match self {
                    $($ty::$variant => f.write_str(stringify!($variant)),)*
                    $ty::Custom(factory) => write!(f, "Custom({})", factory.class_name()),
                }
            }
        }
    };
}

debug_node_class!(FilterNodeClass {
    Filter,
    Default,
    Escape,
    Raw
});
debug_node_class!(FunctionNodeClass {
    Function,
    Enum,
    EnumCases,
    RenderSandboxed
});
debug_node_class!(TestNodeClass {
    Test,
    Constant,
    Defined,
    DivisibleBy,
    Even,
    NonStringCoercingInput,
    Null,
    Odd,
    SameAs,
    True
});

/// The options of a filter. Twig: the `$options` of `new TwigFilter()` merged with the defaults of
/// `AbstractTwigCallable::__construct()` and `TwigFilter::__construct()`.
///
/// The legacy `deprecated`/`deprecating_package`/`alternative` options (deprecated since Twig 3.15) are not
/// supported; use `deprecation_info`.
#[derive(Clone, Default)]
pub struct FilterOptions {
    /// Pass the environment.
    pub needs_environment: bool,
    /// Pass the context.
    pub needs_context: bool,
    /// Pass the charset.
    pub needs_charset: bool,
    /// Pass whether the template is sandboxed.
    pub needs_is_sandboxed: bool,
    /// Collect extra arguments.
    pub is_variadic: bool,
    /// Allowed in the sandbox without being listed in the policy.
    pub always_allowed_in_sandbox: bool,
    /// Deprecation information.
    pub deprecation_info: Option<DeprecatedCallableInfo>,
    /// The strategies the output is safe for.
    pub is_safe: Option<Vec<ConstantValue>>,
    /// Computes the strategies the output is safe for from the arguments.
    pub is_safe_callback: Option<SafeCallback>,
    /// Escape the input with this strategy first, unless it is safe for it.
    pub pre_escape: Option<String>,
    /// The strategies for which the safety of the input carries over to the output.
    pub preserves_safety: Option<Vec<ConstantValue>>,
    /// The node class.
    pub node_class: FilterNodeClass,
}

/// The options of a function. Twig: the `$options` of `new TwigFunction()` with the defaults applied.
#[derive(Clone, Default)]
pub struct FunctionOptions {
    /// Pass the environment.
    pub needs_environment: bool,
    /// Pass the context.
    pub needs_context: bool,
    /// Pass the charset.
    pub needs_charset: bool,
    /// Pass whether the template is sandboxed.
    pub needs_is_sandboxed: bool,
    /// Collect extra arguments.
    pub is_variadic: bool,
    /// Allowed in the sandbox without being listed in the policy.
    pub always_allowed_in_sandbox: bool,
    /// Deprecation information.
    pub deprecation_info: Option<DeprecatedCallableInfo>,
    /// The strategies the output is safe for.
    pub is_safe: Option<Vec<ConstantValue>>,
    /// Computes the strategies the output is safe for from the arguments.
    pub is_safe_callback: Option<SafeCallback>,
    /// The node class.
    pub node_class: FunctionNodeClass,
    /// Builds the node at parse time instead of the node class.
    pub parser_callable: Option<ParserCallable>,
}

/// The options of a test. Twig: the `$options` of `new TwigTest()` with the defaults applied.
///
/// `needs_environment`, `needs_context` and `needs_charset` are kept because Twig counts them in
/// `getMinimalNumberOfRequiredArguments()`, but tests never receive these values (`TwigTest::needsCharset()` and
/// friends return `false`).
#[derive(Clone, Default)]
pub struct TestOptions {
    /// Counted in the minimal number of arguments, never passed.
    pub needs_environment: bool,
    /// Counted in the minimal number of arguments, never passed.
    pub needs_context: bool,
    /// Counted in the minimal number of arguments, never passed.
    pub needs_charset: bool,
    /// Pass whether the template is sandboxed.
    pub needs_is_sandboxed: bool,
    /// Collect extra arguments.
    pub is_variadic: bool,
    /// Allowed in the sandbox without being listed in the policy.
    pub always_allowed_in_sandbox: bool,
    /// Deprecation information.
    pub deprecation_info: Option<DeprecatedCallableInfo>,
    /// The node class.
    pub node_class: TestNodeClass,
    /// The test takes exactly one argument, which may be written without parentheses (`same as`).
    pub one_mandatory_argument: bool,
}

/// The type of a callable: `filter`, `function` or `test` (Twig's `getType()`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CallableType {
    /// `filter`.
    Filter,
    /// `function`.
    Function,
    /// `test`.
    Test,
}

impl CallableType {
    /// `filter`, `function` or `test`.
    pub fn as_str(self) -> &'static str {
        match self {
            CallableType::Filter => "filter",
            CallableType::Function => "function",
            CallableType::Test => "test",
        }
    }
}

macro_rules! callable_type {
    (
        $(#[$doc:meta])*
        $name:ident, $options:ident, $php_class:literal, $ty:expr, $extra_required:expr
    ) => {
        $(#[$doc])*
        #[derive(Clone)]
        pub struct $name {
            def: Arc<($options, Option<PhpCallable>)>,
            name: Vec<u8>,
            dynamic_name: Vec<u8>,
            arguments: Vec<Vec<u8>>,
        }

        impl $name {
            /// Twig: `new TwigXxx($name, $callable, $options)`.
            pub fn new(name: impl Into<Vec<u8>>, callable: Option<PhpCallable>, options: $options) -> Self {
                let name = name.into();
                $name {
                    def: Arc::new((options, callable)),
                    dynamic_name: name.clone(),
                    name,
                    arguments: Vec::new(),
                }
            }

            /// The name (for a dynamic callable matched through a `*` pattern: the name used in the template).
            /// Twig: `getName()`.
            pub fn name(&self) -> &[u8] {
                &self.name
            }

            /// The registered name, which may contain `*`. Twig: `getDynamicName()`.
            pub fn dynamic_name(&self) -> &[u8] {
                &self.dynamic_name
            }

            /// The parts of the name matched by the `*` of the registered name. Twig: `getArguments()`.
            pub fn arguments(&self) -> &[Vec<u8>] {
                &self.arguments
            }

            /// The options.
            pub fn options(&self) -> &$options {
                &self.def.0
            }

            /// The PHP callable, `None` for Twig's `null` callable. Twig: `getCallable()`.
            pub fn callable(&self) -> Option<&PhpCallable> {
                self.def.1.as_ref()
            }

            /// Twig: `getType()`.
            pub fn callable_type(&self) -> CallableType {
                $ty
            }

            /// Twig: `needsIsSandboxed()`.
            pub fn needs_is_sandboxed(&self) -> bool {
                self.def.0.needs_is_sandboxed
            }

            /// Twig: `isVariadic()`.
            pub fn is_variadic(&self) -> bool {
                self.def.0.is_variadic
            }

            /// Twig: `isAlwaysAllowedInSandbox()`.
            pub fn is_always_allowed_in_sandbox(&self) -> bool {
                self.def.0.always_allowed_in_sandbox
            }

            /// Twig: `isDeprecated()`.
            pub fn is_deprecated(&self) -> bool {
                self.def.0.deprecation_info.is_some()
            }

            /// The deprecation information, if the callable is deprecated.
            pub fn deprecation_info(&self) -> Option<&DeprecatedCallableInfo> {
                self.def.0.deprecation_info.as_ref()
            }

            /// Reports the deprecation of a deprecated callable (nothing otherwise).
            ///
            /// Twig: `triggerDeprecation($file, $line)`.
            pub fn trigger_deprecation(
                &self,
                file: Option<&str>,
                line: Option<i64>,
                deprecations: &mut dyn DeprecationSink,
            ) {
                if let Some(info) = &self.def.0.deprecation_info {
                    deprecations.report(info.deprecation($ty.as_str(), &self.name, file, line));
                }
            }

            /// A copy for a name matched through a `*` pattern. Twig: `withDynamicArguments()`.
            #[must_use]
            pub fn with_dynamic_arguments(
                &self,
                name: Vec<u8>,
                dynamic_name: Vec<u8>,
                arguments: Vec<Vec<u8>>,
            ) -> Self {
                $name {
                    def: Arc::clone(&self.def),
                    name,
                    dynamic_name,
                    arguments,
                }
            }

            /// The number of PHP arguments Twig passes before the template arguments: the injected values, the
            /// dynamic-name parts and, for filters and tests, the input. Twig: `getMinimalNumberOfRequiredArguments()`.
            pub fn minimal_number_of_required_arguments(&self) -> usize {
                let o = &self.def.0;
                usize::from(o.needs_charset)
                    + usize::from(o.needs_environment)
                    + usize::from(o.needs_context)
                    + usize::from(o.needs_is_sandboxed)
                    + self.arguments.len()
                    + $extra_required
            }

            /// Whether two handles are the same registered callable (same definition, same name).
            pub fn ptr_eq(&self, other: &Self) -> bool {
                Arc::ptr_eq(&self.def, &other.def) && self.name == other.name
            }
        }

        /// `Twig\TwigXxx(<name>)`, Twig's `__toString()`.
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}({})", $php_class, String::from_utf8_lossy(&self.name))
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{self}")
            }
        }

        /// Callables compare by identity of their definition and by name.
        impl PartialEq for $name {
            fn eq(&self, other: &Self) -> bool {
                self.ptr_eq(other)
            }
        }
    };
}

callable_type!(
    /// A filter. Twig: `Twig\TwigFilter` (TWIG/src/TwigFilter.php).
    TwigFilter,
    FilterOptions,
    "Twig\\TwigFilter",
    CallableType::Filter,
    1
);
callable_type!(
    /// A function. Twig: `Twig\TwigFunction` (TWIG/src/TwigFunction.php).
    TwigFunction,
    FunctionOptions,
    "Twig\\TwigFunction",
    CallableType::Function,
    0
);
callable_type!(
    /// A test. Twig: `Twig\TwigTest` (TWIG/src/TwigTest.php).
    TwigTest,
    TestOptions,
    "Twig\\TwigTest",
    CallableType::Test,
    1
);

impl TwigFilter {
    /// Twig: `needsCharset()`.
    pub fn needs_charset(&self) -> bool {
        self.options().needs_charset
    }

    /// Twig: `needsEnvironment()`.
    pub fn needs_environment(&self) -> bool {
        self.options().needs_environment
    }

    /// Twig: `needsContext()`.
    pub fn needs_context(&self) -> bool {
        self.options().needs_context
    }

    /// The node class. Twig: `getNodeClass()`.
    pub fn node_class(&self) -> &FilterNodeClass {
        &self.options().node_class
    }

    /// The strategies the output is safe for: `is_safe`, else `is_safe_callback($arguments)`, else none.
    /// Twig: `TwigFilter::getSafe()`.
    pub fn safe(&self, arguments: &Arguments) -> Vec<ConstantValue> {
        let o = self.options();
        if let Some(safe) = &o.is_safe {
            return safe.clone();
        }
        if let Some(callback) = &o.is_safe_callback {
            return callback(arguments);
        }
        Vec::new()
    }

    /// Twig: `getPreservesSafety()`.
    pub fn preserves_safety(&self) -> &[ConstantValue] {
        self.options().preserves_safety.as_deref().unwrap_or(&[])
    }

    /// Twig: `getPreEscape()`.
    pub fn pre_escape(&self) -> Option<&str> {
        self.options().pre_escape.as_deref()
    }
}

impl TwigFunction {
    /// Twig: `needsCharset()`.
    pub fn needs_charset(&self) -> bool {
        self.options().needs_charset
    }

    /// Twig: `needsEnvironment()`.
    pub fn needs_environment(&self) -> bool {
        self.options().needs_environment
    }

    /// Twig: `needsContext()`.
    pub fn needs_context(&self) -> bool {
        self.options().needs_context
    }

    /// The node class. Twig: `getNodeClass()`.
    pub fn node_class(&self) -> &FunctionNodeClass {
        &self.options().node_class
    }

    /// Twig: `getParserCallable()`.
    pub fn parser_callable(&self) -> Option<&ParserCallable> {
        self.options().parser_callable.as_ref()
    }

    /// The strategies the output is safe for. Twig: `TwigFunction::getSafe()`.
    pub fn safe(&self, arguments: &Arguments) -> Vec<ConstantValue> {
        let o = self.options();
        if let Some(safe) = &o.is_safe {
            return safe.clone();
        }
        if let Some(callback) = &o.is_safe_callback {
            return callback(arguments);
        }
        Vec::new()
    }
}

impl TwigTest {
    /// Always `false`: tests never receive the charset. Twig: `TwigTest::needsCharset()`.
    pub fn needs_charset(&self) -> bool {
        false
    }

    /// Always `false`. Twig: `TwigTest::needsEnvironment()`.
    pub fn needs_environment(&self) -> bool {
        false
    }

    /// Always `false`. Twig: `TwigTest::needsContext()`.
    pub fn needs_context(&self) -> bool {
        false
    }

    /// The node class. Twig: `getNodeClass()`.
    pub fn node_class(&self) -> &TestNodeClass {
        &self.options().node_class
    }

    /// Twig: `hasOneMandatoryArgument()`.
    pub fn has_one_mandatory_argument(&self) -> bool {
        self.options().one_mandatory_argument
    }
}

/// A callable of any type, as stored in an AST attribute or passed to the argument binder.
#[derive(Debug, Clone, PartialEq)]
pub enum TwigCallable {
    /// A filter.
    Filter(TwigFilter),
    /// A function.
    Function(TwigFunction),
    /// A test.
    Test(TwigTest),
}

impl TwigCallable {
    /// Twig: `getName()`.
    pub fn name(&self) -> &[u8] {
        match self {
            TwigCallable::Filter(c) => c.name(),
            TwigCallable::Function(c) => c.name(),
            TwigCallable::Test(c) => c.name(),
        }
    }

    /// Twig: `getType()`.
    pub fn callable_type(&self) -> CallableType {
        match self {
            TwigCallable::Filter(_) => CallableType::Filter,
            TwigCallable::Function(_) => CallableType::Function,
            TwigCallable::Test(_) => CallableType::Test,
        }
    }

    /// Twig: `getCallable()`.
    pub fn callable(&self) -> Option<&PhpCallable> {
        match self {
            TwigCallable::Filter(c) => c.callable(),
            TwigCallable::Function(c) => c.callable(),
            TwigCallable::Test(c) => c.callable(),
        }
    }

    /// Twig: `isVariadic()`.
    pub fn is_variadic(&self) -> bool {
        match self {
            TwigCallable::Filter(c) => c.is_variadic(),
            TwigCallable::Function(c) => c.is_variadic(),
            TwigCallable::Test(c) => c.is_variadic(),
        }
    }

    /// Twig: `getMinimalNumberOfRequiredArguments()`.
    pub fn minimal_number_of_required_arguments(&self) -> usize {
        match self {
            TwigCallable::Filter(c) => c.minimal_number_of_required_arguments(),
            TwigCallable::Function(c) => c.minimal_number_of_required_arguments(),
            TwigCallable::Test(c) => c.minimal_number_of_required_arguments(),
        }
    }

    /// Twig: `needsCharset()`.
    pub fn needs_charset(&self) -> bool {
        match self {
            TwigCallable::Filter(c) => c.needs_charset(),
            TwigCallable::Function(c) => c.needs_charset(),
            TwigCallable::Test(c) => c.needs_charset(),
        }
    }

    /// Twig: `needsEnvironment()`.
    pub fn needs_environment(&self) -> bool {
        match self {
            TwigCallable::Filter(c) => c.needs_environment(),
            TwigCallable::Function(c) => c.needs_environment(),
            TwigCallable::Test(c) => c.needs_environment(),
        }
    }

    /// Twig: `needsContext()`.
    pub fn needs_context(&self) -> bool {
        match self {
            TwigCallable::Filter(c) => c.needs_context(),
            TwigCallable::Function(c) => c.needs_context(),
            TwigCallable::Test(c) => c.needs_context(),
        }
    }

    /// Twig: `CallExpression::needsIsSandboxed()`.
    pub fn needs_is_sandboxed(&self) -> bool {
        match self {
            TwigCallable::Filter(c) => c.needs_is_sandboxed(),
            TwigCallable::Function(c) => c.needs_is_sandboxed(),
            TwigCallable::Test(c) => c.needs_is_sandboxed(),
        }
    }

    /// The dynamic-name arguments. Twig: `getArguments()`.
    pub fn dynamic_arguments(&self) -> &[Vec<u8>] {
        match self {
            TwigCallable::Filter(c) => c.arguments(),
            TwigCallable::Function(c) => c.arguments(),
            TwigCallable::Test(c) => c.arguments(),
        }
    }
}

impl fmt::Display for TwigCallable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TwigCallable::Filter(c) => c.fmt(f),
            TwigCallable::Function(c) => c.fmt(f),
            TwigCallable::Test(c) => c.fmt(f),
        }
    }
}

/// The tests node constructors create for themselves (`new TwigTest('true', null, ['always_allowed_in_sandbox' =>
/// true])` in `TrueTest::wrap()`, and the `defined`/`null` tests of `NullCoalesceBinary` and `DefaultFilter`): not
/// the registered core tests, but new instances with a `null` callable and the default node class.
pub(crate) fn internal_test(name: &'static str) -> TwigTest {
    static TRUE: OnceLock<TwigTest> = OnceLock::new();
    static DEFINED: OnceLock<TwigTest> = OnceLock::new();
    static NULL: OnceLock<TwigTest> = OnceLock::new();
    let cell = match name {
        "true" => &TRUE,
        "defined" => &DEFINED,
        _ => &NULL,
    };
    cell.get_or_init(|| {
        TwigTest::new(
            name,
            None,
            TestOptions {
                always_allowed_in_sandbox: true,
                ..Default::default()
            },
        )
    })
    .clone()
}

/// A dummy callable returning `''`, used for unknown callables while parsing a discarded `guard` branch
/// (`new TwigFilter($name, static fn () => '')`).
pub(crate) fn dummy_filter(name: &[u8]) -> TwigFilter {
    TwigFilter::new(
        name,
        Some(PhpCallable::closure(Vec::new())),
        FilterOptions::default(),
    )
}

/// See [`dummy_filter`].
pub(crate) fn dummy_function(name: &[u8]) -> TwigFunction {
    TwigFunction::new(
        name,
        Some(PhpCallable::closure(Vec::new())),
        FunctionOptions::default(),
    )
}

/// See [`dummy_filter`].
pub(crate) fn dummy_test(name: &[u8]) -> TwigTest {
    TwigTest::new(
        name,
        Some(PhpCallable::closure(Vec::new())),
        TestOptions::default(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_deprecations_like_deprecated_callable_info() {
        let info = DeprecatedCallableInfo {
            alt_name: Some("new".into()),
            alt_package: Some("acme/new".into()),
            alt_version: Some("3.0".into()),
            ..DeprecatedCallableInfo::new("acme/pkg", "2.0")
        };
        assert_eq!(
            info.deprecation("filter", b"old2", Some("some/path.twig"), Some(1))
                .to_string(),
            "Since acme/pkg 2.0: Twig Filter \"old2\" is deprecated; use \"new\" from the \"acme/new\" package \
             (available since version 3.0) instead in some/path.twig at line 1."
        );
        let info = DeprecatedCallableInfo::new("", "");
        assert_eq!(
            info.deprecation("filter", b"old", Some("index"), Some(0))
                .to_string(),
            "Twig Filter \"old\" is deprecated in index."
        );
    }

    #[test]
    fn counts_required_parameters() {
        let callable = PhpCallable::function(
            "range",
            vec![
                CallableParameter::required("start"),
                CallableParameter::required("end"),
                CallableParameter::optional("step", ConstantValue::Int(1)),
            ],
        );
        assert_eq!(callable.number_of_required_parameters(), 2);
        assert_eq!(callable.reflection_name(), "range");
        let filter = TwigFilter::new(
            "upper",
            None,
            FilterOptions {
                needs_charset: true,
                ..Default::default()
            },
        );
        assert_eq!(filter.minimal_number_of_required_arguments(), 2);
        assert_eq!(filter.to_string(), "Twig\\TwigFilter(upper)");
    }
}
