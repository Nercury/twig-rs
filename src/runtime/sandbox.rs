//! The sandbox: security policies, the checker that applies them, the sandbox extension, the `Sandbox` facade and the
//! bridge behind `render_sandboxed()` (TWIG/src/Sandbox/, TWIG/src/Extension/SandboxExtension.php,
//! TWIG/src/Runtime/SandboxBridgeRuntime.php).
//!
//! A template compiled while a [`SandboxExtension`] is registered carries the security checks of the sandbox node
//! visitor: the tags, filters, functions and tests it uses are checked against the policy when it starts rendering
//! (`checkSecurity()`), and the methods, properties and `__toString()` calls it makes are checked as they happen. The
//! checks only apply while the template runs sandboxed: always for an extension created with `sandboxed = true`
//! (what [`Sandbox`] does), otherwise inside `{% sandbox %}`, an `include(..., sandboxed = true)` or after the
//! deprecated [`SandboxExtension::enable_sandbox`].
//!
//! ```
//! use std::sync::Arc;
//! use twig::loader::ArrayLoader;
//! use twig::runtime::{Environment, SandboxExtension, SecurityPolicy};
//! use twig::value::Array;
//!
//! let env = Environment::new(ArrayLoader::new([("index", "{{ 'a'|upper }}{{ 'b'|lower }}")]));
//! let policy = SecurityPolicy::new();
//! policy.set_allowed_filters(["upper", "escape"]);
//! env.add_extension(Arc::new(SandboxExtension::new(Arc::new(policy), true)))?;
//! let error = env.render("index", Array::new()).unwrap_err();
//! assert_eq!(error.to_string(), "Filter \"lower\" is not allowed in \"index\" at line 1.");
//! # Ok::<(), twig::Error>(())
//! ```

use std::any::Any;
use std::cell::RefCell;
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};

use twig_value::object::ObjectRef;
use twig_value::{Array, Ignore, PhpString, Value, convert};

use super::environment::Environment;
use super::markup::Markup;
use super::template::Template;
use crate::deprecation::{Deprecation, DeprecationSink};
use crate::error::{Error, Result};
use crate::extension::{Extension, NodeVisitor, TokenParser};
use crate::lltl::{Program, SecurityLists};
use crate::source::Source;
use crate::token_parser::SandboxTokenParser;
use crate::visitor::SandboxNodeVisitor;
use crate::vm::{Exception, Sink, VmState};

/// What a [`SecurityError`] stands for: the subclass of `Twig\Sandbox\SecurityError` and the names it carries.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SecurityErrorKind {
    /// `Twig\Sandbox\SecurityNotAllowedTagError` (`getTagName()`).
    Tag(String),
    /// `Twig\Sandbox\SecurityNotAllowedFilterError` (`getFilterName()`).
    Filter(String),
    /// `Twig\Sandbox\SecurityNotAllowedFunctionError` (`getFunctionName()`).
    Function(String),
    /// `Twig\Sandbox\SecurityNotAllowedTestError` (`getTestName()`).
    Test(String),
    /// `Twig\Sandbox\SecurityNotAllowedMethodError` (`getClassName()`, `getMethodName()`).
    Method {
        /// The class of the object.
        class: String,
        /// The method name (lower case).
        method: String,
    },
    /// `Twig\Sandbox\SecurityNotAllowedPropertyError` (`getClassName()`, `getPropertyName()`).
    Property {
        /// The class of the object.
        class: String,
        /// The property name.
        property: String,
    },
    /// `Twig\Sandbox\SecurityError` itself.
    Other,
}

/// A sandbox violation: Twig's `Twig\Sandbox\SecurityError` and its `SecurityNotAllowed*Error` subclasses, which
/// extend `Twig\Error\Error`.
///
/// Policies return it; the checker turns it into a Twig [`Error`] with the template source and line. That error is a
/// `RuntimeError` until [`ErrorKind`](crate::ErrorKind) gets the sandbox classes; the original `SecurityError` rides
/// along as its [`previous`](Error::previous) error, which [`SecurityError::of`] finds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecurityError {
    kind: SecurityErrorKind,
    message: String,
}

impl SecurityError {
    /// A violation with the given message.
    pub fn new(kind: SecurityErrorKind, message: impl Into<String>) -> Self {
        SecurityError {
            kind,
            message: message.into(),
        }
    }

    /// What the violation stands for.
    pub fn kind(&self) -> &SecurityErrorKind {
        &self.kind
    }

    /// The raw message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// The PHP class of the violation, e.g. `Twig\Sandbox\SecurityNotAllowedFilterError`.
    pub fn class_name(&self) -> &'static str {
        match self.kind {
            SecurityErrorKind::Tag(_) => "Twig\\Sandbox\\SecurityNotAllowedTagError",
            SecurityErrorKind::Filter(_) => "Twig\\Sandbox\\SecurityNotAllowedFilterError",
            SecurityErrorKind::Function(_) => "Twig\\Sandbox\\SecurityNotAllowedFunctionError",
            SecurityErrorKind::Test(_) => "Twig\\Sandbox\\SecurityNotAllowedTestError",
            SecurityErrorKind::Method { .. } => "Twig\\Sandbox\\SecurityNotAllowedMethodError",
            SecurityErrorKind::Property { .. } => "Twig\\Sandbox\\SecurityNotAllowedPropertyError",
            SecurityErrorKind::Other => "Twig\\Sandbox\\SecurityError",
        }
    }

    /// The sandbox violation a Twig error stands for, if it is one.
    pub fn of(error: &Error) -> Option<&SecurityError> {
        error.previous()?.downcast_ref::<SecurityError>()
    }

    /// The Twig error of the violation, with Twig's constructor defaults (no line, no source).
    pub fn into_error(self) -> Error {
        Error::runtime(self.message.clone()).with_previous(self)
    }
}

impl fmt::Display for SecurityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.class_name(), self.message)
    }
}

impl std::error::Error for SecurityError {}

/// A security policy: decides which tags, filters, functions, tests, methods and properties a sandboxed template may
/// use. Twig: `Twig\Sandbox\SecurityPolicyInterface`.
pub trait SecurityPolicyInterface: Send + Sync + 'static {
    /// Checks the tags, filters, functions and tests a template uses. Twig: `checkSecurity($tags, $filters,
    /// $functions, $tests)`.
    ///
    /// # Errors
    ///
    /// The first violation.
    fn check_security(
        &self,
        tags: &[&[u8]],
        filters: &[&[u8]],
        functions: &[&[u8]],
        tests: &[&[u8]],
        deprecations: &mut dyn DeprecationSink,
    ) -> std::result::Result<(), SecurityError>;

    /// Checks a method call (also `__toString` for string conversions). Twig: `checkMethodAllowed($obj, $method)`.
    ///
    /// # Errors
    ///
    /// A [`SecurityErrorKind::Method`] violation.
    fn check_method_allowed(
        &self,
        object: &ObjectRef,
        method: &[u8],
    ) -> std::result::Result<(), SecurityError>;

    /// Checks a property read (also an `ArrayAccess` offset, which is any PHP value). Twig:
    /// `checkPropertyAllowed($obj, $property)`.
    ///
    /// # Errors
    ///
    /// A [`SecurityErrorKind::Property`] violation.
    fn check_property_allowed(
        &self,
        object: &ObjectRef,
        property: &Value,
    ) -> std::result::Result<(), SecurityError>;

    /// The policy as `Any`, to reach the concrete type.
    fn as_any(&self) -> &dyn Any;
}

/// Decides whether a template runs sandboxed from its source. Twig: `Twig\Sandbox\SourcePolicyInterface`
/// (deprecated since Twig 3.27.0).
pub trait SourcePolicyInterface: Send + Sync + 'static {
    /// Twig: `enableSandbox(Source $source): bool`.
    fn enable_sandbox(&self, source: &Source) -> bool;
}

#[derive(Default)]
struct Lists {
    tags: Vec<String>,
    filters: Vec<String>,
    methods: Vec<(String, Vec<String>)>,
    properties: Vec<(String, Vec<String>)>,
    functions: Vec<String>,
    tests: Vec<String>,
}

/// The allow-list policy of Twig. Twig: `Twig\Sandbox\SecurityPolicy` (TWIG/src/Sandbox/SecurityPolicy.php).
///
/// The lists can be changed after the policy was handed to a sandbox (Twig's setters). Methods are allowed per class
/// (and its subclasses and interfaces) and matched case-insensitively; properties per class, case-sensitively. In
/// non-strict mode (the default, Twig 3 behaviour), the `extends` and `use` tags, the `parent`, `block` and
/// `attribute` functions and every test are allowed with a deprecation; [`set_strict`](Self::set_strict) opts in to
/// Twig 4.0's rules.
#[derive(Default)]
pub struct SecurityPolicy {
    lists: RwLock<Lists>,
    strict: AtomicBool,
}

impl fmt::Debug for SecurityPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SecurityPolicy")
            .field("strict", &self.is_strict())
            .finish_non_exhaustive()
    }
}

fn strings<I, S>(items: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    items.into_iter().map(Into::into).collect()
}

impl SecurityPolicy {
    /// A policy that allows nothing. Twig: `new SecurityPolicy()`.
    pub fn new() -> Self {
        SecurityPolicy::default()
    }

    fn lists(&self) -> std::sync::RwLockReadGuard<'_, Lists> {
        self.lists.read().unwrap_or_else(|e| e.into_inner())
    }

    fn lists_mut(&self) -> std::sync::RwLockWriteGuard<'_, Lists> {
        self.lists.write().unwrap_or_else(|e| e.into_inner())
    }

    /// Twig: `setAllowedTags()`.
    pub fn set_allowed_tags<I, S>(&self, tags: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.lists_mut().tags = strings(tags);
    }

    /// Twig: `setAllowedFilters()`.
    pub fn set_allowed_filters<I, S>(&self, filters: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.lists_mut().filters = strings(filters);
    }

    /// Twig: `setAllowedMethods()`: class => method names (lower-cased here, as Twig does).
    pub fn set_allowed_methods<I, C, M, S>(&self, methods: I)
    where
        I: IntoIterator<Item = (C, M)>,
        C: Into<String>,
        M: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.lists_mut().methods = methods
            .into_iter()
            .map(|(class, methods)| {
                let methods = methods
                    .into_iter()
                    .map(|m| m.into().to_ascii_lowercase())
                    .collect();
                (class.into(), methods)
            })
            .collect();
    }

    /// Twig: `setAllowedProperties()`: class => property names.
    pub fn set_allowed_properties<I, C, P, S>(&self, properties: I)
    where
        I: IntoIterator<Item = (C, P)>,
        C: Into<String>,
        P: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.lists_mut().properties = properties
            .into_iter()
            .map(|(class, properties)| (class.into(), strings(properties)))
            .collect();
    }

    /// Twig: `setAllowedFunctions()`.
    pub fn set_allowed_functions<I, S>(&self, functions: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.lists_mut().functions = strings(functions);
    }

    /// Twig: `setAllowedTests()`.
    pub fn set_allowed_tests<I, S>(&self, tests: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.lists_mut().tests = strings(tests);
    }

    /// Toggles strict mode (Twig 4.0's rules: nothing is implicitly allowed). Twig: `setStrict()`.
    pub fn set_strict(&self, strict: bool) {
        self.strict.store(strict, Ordering::Release);
    }

    /// Whether strict mode is on. Twig: `isStrict()`.
    pub fn is_strict(&self) -> bool {
        self.strict.load(Ordering::Acquire)
    }
}

/// `in_array($name, $allowed, true)`.
fn allowed(list: &[String], name: &[u8]) -> bool {
    list.iter().any(|item| item.as_bytes() == name)
}

fn lossy(name: &[u8]) -> String {
    String::from_utf8_lossy(name).into_owned()
}

/// `The "%s" %s is always allowed in sandboxes, but won't be in 4.0, ...`
fn always_allowed(version: &str, name: &[u8], what: &str) -> Deprecation {
    Deprecation::twig(
        version,
        format!(
            "The \"{}\" {what} is always allowed in sandboxes, but won't be in 4.0, please enable it explicitly in your sandbox policy if needed (or enable strict mode on the security policy to opt-in to the 4.0 behavior now).",
            lossy(name)
        ),
    )
}

impl SecurityPolicyInterface for SecurityPolicy {
    /// Twig: `SecurityPolicy::checkSecurity()`: tags, then filters, then functions, then tests; the first name that
    /// is not allowed fails (or is let through with a deprecation in non-strict mode).
    fn check_security(
        &self,
        tags: &[&[u8]],
        filters: &[&[u8]],
        functions: &[&[u8]],
        tests: &[&[u8]],
        deprecations: &mut dyn DeprecationSink,
    ) -> std::result::Result<(), SecurityError> {
        let strict = self.is_strict();
        let lists = self.lists();
        for &tag in tags {
            if !allowed(&lists.tags, tag) {
                if !strict && (tag == b"extends" || tag == b"use") {
                    deprecations.report(always_allowed("3.12", tag, "tag"));
                } else {
                    return Err(SecurityError::new(
                        SecurityErrorKind::Tag(lossy(tag)),
                        format!("Tag \"{}\" is not allowed.", lossy(tag)),
                    ));
                }
            }
        }
        for &filter in filters {
            if !allowed(&lists.filters, filter) {
                return Err(SecurityError::new(
                    SecurityErrorKind::Filter(lossy(filter)),
                    format!("Filter \"{}\" is not allowed.", lossy(filter)),
                ));
            }
        }
        for &function in functions {
            if !allowed(&lists.functions, function) {
                if !strict && matches!(function, b"parent" | b"block" | b"attribute") {
                    deprecations.report(always_allowed("3.27", function, "function"));
                } else {
                    return Err(SecurityError::new(
                        SecurityErrorKind::Function(lossy(function)),
                        format!("Function \"{}\" is not allowed.", lossy(function)),
                    ));
                }
            }
        }
        for &test in tests {
            if !allowed(&lists.tests, test) {
                if !strict {
                    deprecations.report(always_allowed("3.28", test, "test"));
                } else {
                    return Err(SecurityError::new(
                        SecurityErrorKind::Test(lossy(test)),
                        format!("Test \"{}\" is not allowed.", lossy(test)),
                    ));
                }
            }
        }
        Ok(())
    }

    /// Twig: `SecurityPolicy::checkMethodAllowed()`: the methods of `Markup` (except the constructor and
    /// `withContent()`) are always allowed.
    fn check_method_allowed(
        &self,
        object: &ObjectRef,
        method: &[u8],
    ) -> std::result::Result<(), SecurityError> {
        let method = method.to_ascii_lowercase();
        if Markup::is_markup(object) && method != b"__construct" && method != b"withcontent" {
            return Ok(());
        }
        let lists = self.lists();
        let is_allowed = lists.methods.iter().any(|(class, methods)| {
            object.instance_of(class) && methods.iter().any(|m| m.as_bytes() == method.as_slice())
        });
        if is_allowed {
            return Ok(());
        }
        let class = object.class_name().to_owned();
        let method = lossy(&method);
        Err(SecurityError::new(
            SecurityErrorKind::Method {
                class: class.clone(),
                method: method.clone(),
            },
            format!("Calling \"{method}\" method on a \"{class}\" object is not allowed."),
        ))
    }

    /// Twig: `SecurityPolicy::checkPropertyAllowed()`.
    fn check_property_allowed(
        &self,
        object: &ObjectRef,
        property: &Value,
    ) -> std::result::Result<(), SecurityError> {
        let lists = self.lists();
        let is_allowed = match property {
            Value::String(name) => lists.properties.iter().any(|(class, properties)| {
                object.instance_of(class)
                    && properties.iter().any(|p| p.as_bytes() == name.as_bytes())
            }),
            _ => false,
        };
        if is_allowed {
            return Ok(());
        }
        let class = object.class_name().to_owned();
        // sprintf('%s', $property)
        let property = convert::to_string(property, &mut Ignore)
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        Err(SecurityError::new(
            SecurityErrorKind::Property {
                class: class.clone(),
                property: property.clone(),
            },
            format!("Calling \"{property}\" property on a \"{class}\" object is not allowed."),
        ))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

thread_local! {
    /// The checkers the templates of this thread switched the sandbox on for (`{% sandbox %}`, `include(...,
    /// sandboxed = true)`): Twig's `SecurityChecker::setSandboxed()` from compiled code. Those toggles are paired
    /// within one render, which runs on one thread, so keeping them per thread keeps concurrent renders apart.
    static TEMPLATE_SANDBOXED: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
}

static NEXT_CHECKER: AtomicUsize = AtomicUsize::new(1);

/// Applies a policy to the templates that run sandboxed. Twig: `Twig\Sandbox\SecurityChecker`
/// (TWIG/src/Sandbox/SecurityChecker.php, internal).
pub struct SecurityChecker {
    id: usize,
    policy: RwLock<Arc<dyn SecurityPolicyInterface>>,
    sandboxed_globally: bool,
    sandboxed: AtomicBool,
    source_policy: Option<Arc<dyn SourcePolicyInterface>>,
}

impl fmt::Debug for SecurityChecker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SecurityChecker")
            .field("sandboxed_globally", &self.sandboxed_globally)
            .finish_non_exhaustive()
    }
}

/// Reports deprecations to an environment.
struct EnvSink<'a>(&'a Environment);

impl DeprecationSink for EnvSink<'_> {
    fn report(&mut self, deprecation: Deprecation) {
        self.0.report_deprecation(deprecation);
    }
}

impl SecurityChecker {
    fn new(
        policy: Arc<dyn SecurityPolicyInterface>,
        sandboxed_globally: bool,
        source_policy: Option<Arc<dyn SourcePolicyInterface>>,
    ) -> Self {
        SecurityChecker {
            id: NEXT_CHECKER.fetch_add(1, Ordering::Relaxed),
            policy: RwLock::new(policy),
            sandboxed_globally,
            sandboxed: AtomicBool::new(false),
            source_policy,
        }
    }

    /// Toggles the sandbox for every template of the environment (the deprecated `SandboxExtension::enableSandbox()`
    /// and `disableSandbox()`). Twig: `setSandboxed()`.
    pub fn set_sandboxed(&self, sandboxed: bool) {
        self.sandboxed.store(sandboxed, Ordering::Release);
    }

    /// `setSandboxed()` as compiled templates call it: for the templates of the current thread.
    pub(crate) fn set_template_sandboxed(&self, sandboxed: bool) {
        TEMPLATE_SANDBOXED.with(|ids| {
            let mut ids = ids.borrow_mut();
            ids.retain(|&id| id != self.id);
            if sandboxed {
                ids.push(self.id);
            }
        });
    }

    fn template_sandboxed(&self) -> bool {
        TEMPLATE_SANDBOXED.with(|ids| ids.borrow().contains(&self.id))
    }

    /// Whether templates (of `source`) run sandboxed: globally, after `set_sandboxed(true)`, inside a template's
    /// sandbox toggle, or by the (deprecated) source policy. Twig: `isSandboxed(?Source $source = null)`.
    pub fn is_sandboxed(&self, source: Option<&Source>) -> bool {
        self.sandboxed_globally
            || self.sandboxed.load(Ordering::Acquire)
            || self.template_sandboxed()
            || match (source, &self.source_policy) {
                (Some(source), Some(policy)) => policy.enable_sandbox(source),
                _ => false,
            }
    }

    /// Whether the extension was created sandboxed. Twig: `isSandboxedGlobally()`.
    pub fn is_sandboxed_globally(&self) -> bool {
        self.sandboxed_globally
    }

    /// Replaces the policy. Twig: `setSecurityPolicy()`.
    pub fn set_security_policy(&self, policy: Arc<dyn SecurityPolicyInterface>) {
        *self.policy.write().unwrap_or_else(|e| e.into_inner()) = policy;
    }

    /// The policy. Twig: `getSecurityPolicy()`.
    pub fn security_policy(&self) -> Arc<dyn SecurityPolicyInterface> {
        Arc::clone(&self.policy.read().unwrap_or_else(|e| e.into_inner()))
    }

    /// Twig: `SecurityChecker::checkSecurity()`: the policy check when sandboxed; a violation gets the source.
    fn check_security(
        &self,
        env: &Environment,
        lists: [&[&[u8]]; 4],
        source: &Source,
    ) -> std::result::Result<(), (SecurityError, Error)> {
        if !self.is_sandboxed(Some(source)) {
            return Ok(());
        }
        let [tags, filters, functions, tests] = lists;
        self.security_policy()
            .check_security(tags, filters, functions, tests, &mut EnvSink(env))
            .map_err(|violation| {
                let error = violation.clone().into_error().with_source(source.clone());
                (violation, error)
            })
    }

    /// Twig: `SecurityChecker::checkMethodAllowed()`: a method violation gets the source and the line (and is told
    /// apart, as `getAttribute()` catches it).
    pub(crate) fn check_method_allowed(
        &self,
        object: &ObjectRef,
        method: &[u8],
        line: i64,
        source: &Source,
    ) -> std::result::Result<(), Violation> {
        if !self.is_sandboxed(Some(source)) {
            return Ok(());
        }
        self.method_check(object, method, line, source)
    }

    fn method_check(
        &self,
        object: &ObjectRef,
        method: &[u8],
        line: i64,
        source: &Source,
    ) -> std::result::Result<(), Violation> {
        match self.security_policy().check_method_allowed(object, method) {
            Ok(()) => Ok(()),
            Err(violation) if matches!(violation.kind, SecurityErrorKind::Method { .. }) => {
                Err(Violation::NotAllowed(
                    violation
                        .into_error()
                        .with_source(source.clone())
                        .with_line(line)
                        .into(),
                ))
            }
            Err(other) => Err(Violation::Other(other.into_error().into())),
        }
    }

    /// Twig: `SecurityChecker::checkPropertyAllowed()`: a property violation gets the source and the line (and is
    /// told apart, as `getAttribute()` falls back to a method then).
    pub(crate) fn check_property_allowed(
        &self,
        object: &ObjectRef,
        property: &Value,
        line: i64,
        source: &Source,
    ) -> std::result::Result<(), Violation> {
        if !self.is_sandboxed(Some(source)) {
            return Ok(());
        }
        match self
            .security_policy()
            .check_property_allowed(object, property)
        {
            Ok(()) => Ok(()),
            Err(violation) if matches!(violation.kind, SecurityErrorKind::Property { .. }) => {
                Err(Violation::NotAllowed(
                    violation
                        .into_error()
                        .with_source(source.clone())
                        .with_line(line)
                        .into(),
                ))
            }
            Err(other) => Err(Violation::Other(other.into_error().into())),
        }
    }

    /// Twig: `SecurityChecker::ensureToStringAllowed()`: when sandboxed, a `Stringable` must be allowed to call
    /// `__toString()`, and arrays and `Traversable`s are searched for such objects (a single-pass iterator comes back
    /// as the array it was materialised into).
    pub(crate) fn ensure_to_string_allowed(
        &self,
        state: &mut VmState,
        value: Value,
        line: i64,
        source: &Source,
    ) -> std::result::Result<Value, Exception> {
        if !self.is_sandboxed(Some(source)) {
            // walking an array element by element does nothing observable outside the sandbox
            return Ok(value);
        }
        let mut seen = Vec::new();
        self.do_ensure(state, value, line, source, &mut seen)
    }

    /// Twig: `SecurityChecker::ensureSpreadAllowed(iterable $obj, ...)`: the spread operand is materialised (also
    /// outside the sandbox) and its elements are checked.
    pub(crate) fn ensure_spread_allowed(
        &self,
        state: &mut VmState,
        value: Value,
        line: i64,
        source: &Source,
    ) -> std::result::Result<Value, Exception> {
        let mut seen = Vec::new();
        let array = match &value {
            Value::Array(array) => array.clone(),
            Value::Object(object) if object.instance_of("Traversable") => {
                seen.push(object.id());
                twig_value::object::iterator_to_array(object, true, state)?
            }
            other => {
                return Err(twig_value::param::UserArg::new(
                    "Twig\\Sandbox\\SecurityChecker::ensureSpreadAllowed",
                    1,
                    "obj",
                )
                .type_error(&twig_value::param::DeclaredType::iterable(), other)
                .into());
            }
        };
        if self.is_sandboxed(Some(source)) {
            self.ensure_array(state, &array, line, source, &mut seen)?;
        }
        Ok(Value::Array(array))
    }

    /// Twig: `SecurityChecker::doEnsureToStringAllowed()` (sandboxed).
    fn do_ensure(
        &self,
        state: &mut VmState,
        value: Value,
        line: i64,
        source: &Source,
        seen: &mut Vec<u64>,
    ) -> std::result::Result<Value, Exception> {
        let object = match &value {
            Value::Array(array) => {
                self.ensure_array(state, array, line, source, seen)?;
                return Ok(value);
            }
            Value::Object(object) => object.clone(),
            _ => return Ok(value),
        };
        let stringable = object.instance_of("Stringable");
        if stringable {
            self.method_check(&object, b"__toString", line, source)
                .map_err(Violation::into_exception)?;
        }
        if object.instance_of("Traversable") {
            if seen.contains(&object.id()) {
                return Ok(value);
            }
            seen.push(object.id());
            if object.instance_of("IteratorAggregate") {
                // `foreach ($obj as $v)`: the values only
                if let Some(mut iter) = object.get_iterator(state)? {
                    iter.rewind(state)?;
                    while iter.valid(state)? {
                        let element = iter.current(state)?;
                        self.do_ensure(state, element, line, source, seen)?;
                        iter.next(state)?;
                    }
                }
                return Ok(value);
            }
            let array = twig_value::object::iterator_to_array(&object, true, state)?;
            self.ensure_array(state, &array, line, source, seen)?;
            if !stringable {
                return Ok(Value::Array(array));
            }
        }
        Ok(value)
    }

    /// Twig: `SecurityChecker::ensureToStringAllowedForArray()`: falsy elements are skipped, nested arrays searched.
    fn ensure_array(
        &self,
        state: &mut VmState,
        array: &Array,
        line: i64,
        source: &Source,
        seen: &mut Vec<u64>,
    ) -> std::result::Result<(), Exception> {
        for element in array.values() {
            if !convert::to_bool(element, state)? {
                continue;
            }
            match element {
                Value::Array(nested) => self.ensure_array(state, nested, line, source, seen)?,
                other => {
                    self.do_ensure(state, other.clone(), line, source, seen)?;
                }
            }
        }
        Ok(())
    }
}

/// A failed method or property check: the violation the check is about (`SecurityNotAllowedMethodError`,
/// `SecurityNotAllowedPropertyError`, which `getAttribute()` catches), or another exception of the policy.
pub(crate) enum Violation {
    NotAllowed(Exception),
    Other(Exception),
}

impl Violation {
    /// The exception, whatever it is.
    pub(crate) fn into_exception(self) -> Exception {
        match self {
            Violation::NotAllowed(exception) | Violation::Other(exception) => exception,
        }
    }
}

/// The names of a security list (`array_keys($usedTags)`).
fn names(list: &[(Vec<u8>, i64)]) -> Vec<&[u8]> {
    list.iter().map(|(name, _)| name.as_slice()).collect()
}

/// `isset($map[$name])` on a list filtered with `array_filter()` (lines `0` dropped): the line of the first use.
fn first_line(list: &[(Vec<u8>, i64)], name: &str) -> Option<i64> {
    list.iter()
        .find(|(used, line)| used.as_slice() == name.as_bytes() && *line != 0)
        .map(|(_, line)| *line)
}

/// Twig: `ensureSecurityChecked()` of a template compiled with the sandbox (`CheckSecurityNode`): when the template
/// runs sandboxed, `checkSecurity()` checks its tags, filters, functions and tests; a violation is reported at the line
/// of the first use of what it names.
pub(crate) fn ensure_security_checked(
    state: &mut VmState,
    template: &Template,
) -> std::result::Result<(), Exception> {
    let Some(lists) = template.program().security() else {
        return Ok(());
    };
    let Some(checker) = state.env.sandbox_checker() else {
        return Ok(());
    };
    let source = template.source();
    if !checker.is_sandboxed(Some(source)) {
        return Ok(());
    }
    check_security(&state.env, &checker, lists, source)
}

/// Twig: the compiled `checkSecurity()` (`CheckSecurityNode::compile()`).
fn check_security(
    env: &Environment,
    checker: &SecurityChecker,
    lists: &SecurityLists,
    source: &Source,
) -> std::result::Result<(), Exception> {
    let tags = names(&lists.tags);
    let filters = names(&lists.filters);
    let functions = names(&lists.functions);
    let tests = names(&lists.tests);
    match checker.check_security(env, [&tags, &filters, &functions, &tests], source) {
        Ok(()) => Ok(()),
        Err((violation, mut error)) => {
            let line = match violation.kind() {
                SecurityErrorKind::Tag(name) => first_line(&lists.tags, name),
                SecurityErrorKind::Filter(name) => first_line(&lists.filters, name),
                SecurityErrorKind::Function(name) => first_line(&lists.functions, name),
                SecurityErrorKind::Test(name) => first_line(&lists.tests, name),
                _ => None,
            };
            if let Some(line) = line {
                error.set_template_line(line);
            }
            Err(error.into())
        }
    }
}

/// Twig: `Template::ensureTraitsAllowed()` with the compiled `checkTraitsAllowed()` (when the template uses `use`):
/// the `use` tag is checked when the template is loaded, as its traits are resolved then; the violation is reported at
/// the line of the first `use`.
pub(crate) fn ensure_traits_allowed(env: &Environment, program: &Program) -> Result<()> {
    let Some(lists) = program.security() else {
        return Ok(());
    };
    let Some(use_line) = lists
        .tags
        .iter()
        .find(|(tag, _)| tag.as_slice() == b"use")
        .map(|(_, line)| *line)
    else {
        return Ok(());
    };
    let Some(checker) = env.sandbox_checker() else {
        return Ok(());
    };
    let source = program.source();
    if !checker.is_sandboxed(Some(source)) {
        return Ok(());
    }
    let use_tag: &[u8] = b"use";
    match checker.check_security(env, [&[use_tag], &[], &[], &[]], source) {
        Ok(()) => Ok(()),
        Err((violation, mut error)) => {
            if matches!(violation.kind(), SecurityErrorKind::Tag(_)) {
                error.set_template_line(use_line);
            } else if error.template_line() == -1 {
                // `handleException()` guesses from the constructor, which has no line marker
                error.set_template_line(0);
            }
            Err(error)
        }
    }
}

/// Twig: the `SandboxNode` toggle: `if (!$alreadySandboxed = $this->sandbox->isSandboxed()) {
/// $this->sandbox->setSandboxed(true); }`; returns `$alreadySandboxed`.
pub(crate) fn sandbox_start(env: &Environment) -> bool {
    match env.sandbox_checker() {
        Some(checker) => {
            let already = checker.is_sandboxed(None);
            if !already {
                checker.set_template_sandboxed(true);
            }
            already
        }
        None => true,
    }
}

/// Twig: `if (!$alreadySandboxed) { $this->sandbox->setSandboxed(false); }`.
pub(crate) fn sandbox_end(env: &Environment, already: bool) {
    if !already && let Some(checker) = env.sandbox_checker() {
        checker.set_template_sandboxed(false);
    }
}

/// `Twig\Extension\SandboxExtension`: registers the deprecated `sandbox` tag and the sandbox node visitor, and holds
/// the [`SecurityChecker`] of the environment. Twig: TWIG/src/Extension/SandboxExtension.php.
///
/// Its class name is Twig's, so it stands for `SandboxExtension` everywhere Twig checks for it (compiled templates,
/// `hasExtension()`).
pub struct SandboxExtension {
    checker: Arc<SecurityChecker>,
}

impl fmt::Debug for SandboxExtension {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SandboxExtension")
            .field("checker", &self.checker)
            .finish()
    }
}

impl SandboxExtension {
    /// Twig: `new SandboxExtension($policy, $sandboxed)`: with `sandboxed`, every template of the environment runs
    /// sandboxed.
    pub fn new(policy: Arc<dyn SecurityPolicyInterface>, sandboxed: bool) -> Self {
        SandboxExtension {
            checker: Arc::new(SecurityChecker::new(policy, sandboxed, None)),
        }
    }

    /// Twig: `new SandboxExtension($policy, $sandboxed, $sourcePolicy)`, deprecated since Twig 3.27.0: the source
    /// policy decides per template whether it runs sandboxed. The deprecation is reported to `deprecations`.
    pub fn with_source_policy(
        policy: Arc<dyn SecurityPolicyInterface>,
        sandboxed: bool,
        source_policy: Arc<dyn SourcePolicyInterface>,
        deprecations: &mut dyn DeprecationSink,
    ) -> Self {
        deprecations.report(Deprecation::twig(
            "3.27.0",
            "The \"Twig\\Sandbox\\SourcePolicyInterface\" interface is deprecated with no replacement, do not pass an instance to \"Twig\\Extension\\SandboxExtension\".",
        ));
        SandboxExtension {
            checker: Arc::new(SecurityChecker::new(policy, sandboxed, Some(source_policy))),
        }
    }

    /// The checker. Twig: `getChecker()`.
    pub fn checker(&self) -> &Arc<SecurityChecker> {
        &self.checker
    }

    fn deprecated_toggle(method: &str, deprecations: &mut dyn DeprecationSink) {
        deprecations.report(Deprecation::twig(
            "3.29",
            format!(
                "The \"Twig\\Extension\\SandboxExtension::{method}()\" method is deprecated, use \"Twig\\Sandbox\\Sandbox\" to render untrusted templates instead."
            ),
        ));
    }

    /// Sandboxes every template of the environment (deprecated since Twig 3.29). Twig: `enableSandbox()`.
    pub fn enable_sandbox(&self, deprecations: &mut dyn DeprecationSink) {
        Self::deprecated_toggle("enableSandbox", deprecations);
        self.checker.set_sandboxed(true);
    }

    /// Twig: `disableSandbox()` (deprecated since Twig 3.29).
    pub fn disable_sandbox(&self, deprecations: &mut dyn DeprecationSink) {
        Self::deprecated_toggle("disableSandbox", deprecations);
        self.checker.set_sandboxed(false);
    }

    /// Twig: `isSandboxed(?Source $source = null)`.
    pub fn is_sandboxed(&self, source: Option<&Source>) -> bool {
        self.checker.is_sandboxed(source)
    }

    /// Twig: `isSandboxedGlobally()` (deprecated since Twig 3.29).
    pub fn is_sandboxed_globally(&self, deprecations: &mut dyn DeprecationSink) -> bool {
        Self::deprecated_toggle("isSandboxedGlobally", deprecations);
        self.checker.is_sandboxed_globally()
    }

    /// Twig: `setSecurityPolicy()`.
    pub fn set_security_policy(&self, policy: Arc<dyn SecurityPolicyInterface>) {
        self.checker.set_security_policy(policy);
    }

    /// Twig: `getSecurityPolicy()`.
    pub fn security_policy(&self) -> Arc<dyn SecurityPolicyInterface> {
        self.checker.security_policy()
    }
}

impl Extension for SandboxExtension {
    fn class_name(&self) -> &str {
        "Twig\\Extension\\SandboxExtension"
    }

    /// Twig: `SandboxExtension::getTokenParsers()`.
    fn token_parsers(&self) -> Vec<Arc<dyn TokenParser>> {
        vec![Arc::new(SandboxTokenParser)]
    }

    /// Twig: `SandboxExtension::getNodeVisitors()`.
    fn node_visitors(&self) -> Vec<Arc<dyn NodeVisitor>> {
        vec![Arc::new(SandboxNodeVisitor)]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Renders untrusted templates in an environment dedicated to it, always sandboxed. Twig: `Twig\Sandbox\Sandbox`
/// (TWIG/src/Sandbox/Sandbox.php, Twig 3.29+).
#[derive(Debug, Clone)]
pub struct Sandbox {
    env: Environment,
}

impl Sandbox {
    /// Twig: `new Sandbox($env, $policy)`: registers a globally sandboxed [`SandboxExtension`] on `env`.
    ///
    /// # Errors
    ///
    /// `LogicException`s: `The sandbox requires a strict security policy, call "setStrict(true)" on a dedicated policy
    /// for this sandbox.` for a non-strict [`SecurityPolicy`]; `The environment passed to "Twig\Sandbox\Sandbox" must be
    /// dedicated to it: pass a freshly built environment that has no "Twig\Extension\SandboxExtension" registered and
    /// has not been used yet.` when the extension cannot be registered (the registration error is the previous one).
    pub fn new(env: Environment, policy: Arc<dyn SecurityPolicyInterface>) -> Result<Sandbox> {
        if let Some(policy) = policy.as_any().downcast_ref::<SecurityPolicy>()
            && !policy.is_strict()
        {
            return Err(Error::logic(
                "The sandbox requires a strict security policy, call \"setStrict(true)\" on a dedicated policy for this sandbox.",
            ));
        }
        if let Err(error) = env.add_extension(Arc::new(SandboxExtension::new(policy, true))) {
            return Err(Error::logic(
                "The environment passed to \"Twig\\Sandbox\\Sandbox\" must be dedicated to it: pass a freshly built environment that has no \"Twig\\Extension\\SandboxExtension\" registered and has not been used yet.",
            )
            .with_previous(error));
        }
        Ok(Sandbox { env })
    }

    /// The dedicated environment.
    pub fn environment(&self) -> &Environment {
        &self.env
    }

    /// Twig: `Sandbox::render()`.
    ///
    /// # Errors
    ///
    /// Loading and rendering errors, sandbox violations among them.
    pub fn render(&self, name: &str, context: Array) -> Result<String> {
        self.env.render(name, context)
    }

    /// [`render`](Self::render) to bytes.
    ///
    /// # Errors
    ///
    /// As [`render`](Self::render).
    pub fn render_bytes(&self, name: &str, context: Array) -> Result<Vec<u8>> {
        self.env.render_bytes(name, context)
    }

    /// Twig: `Sandbox::display()`.
    ///
    /// # Errors
    ///
    /// As [`render`](Self::render), and the errors of the sink.
    pub fn display(&self, name: &str, context: Array, sink: &mut dyn Sink) -> Result<()> {
        self.env.display(name, context, sink)
    }

    /// Twig: `Sandbox::renderBlock()`.
    ///
    /// # Errors
    ///
    /// As [`render`](Self::render).
    pub fn render_block(&self, name: &str, block: &str, context: Array) -> Result<String> {
        self.env.load(name)?.render_block(block, context)
    }

    /// Twig: `Sandbox::displayBlock()`.
    ///
    /// # Errors
    ///
    /// As [`display`](Self::display).
    pub fn display_block(
        &self,
        name: &str,
        block: &str,
        context: Array,
        sink: &mut dyn Sink,
    ) -> Result<()> {
        self.env.load(name)?.display_block(block, context, sink)
    }

    /// Twig: `Sandbox::createTemplate()`: a sandboxed template from source code.
    ///
    /// # Errors
    ///
    /// The errors of compiling the template.
    pub fn create_template(
        &self,
        template: &[u8],
        name: Option<&str>,
    ) -> Result<super::TemplateWrapper> {
        self.env.create_template(template, name)
    }
}

/// The runtime of `render_sandboxed()` (`SandboxBridgeExtension`): renders a template of a [`Sandbox`]. Twig:
/// `Twig\Runtime\SandboxBridgeRuntime`; register it with [`Environment::add_runtime`] under
/// [`SandboxBridgeRuntime::CLASS`].
#[derive(Debug, Clone)]
pub struct SandboxBridgeRuntime {
    sandbox: Sandbox,
}

impl SandboxBridgeRuntime {
    /// The runtime class name, `Twig\Runtime\SandboxBridgeRuntime`.
    pub const CLASS: &'static str = "Twig\\Runtime\\SandboxBridgeRuntime";

    /// Twig: `new SandboxBridgeRuntime($sandbox)`.
    pub fn new(sandbox: Sandbox) -> Self {
        SandboxBridgeRuntime { sandbox }
    }

    /// Twig: `SandboxBridgeRuntime::render(string $name, array $context): string`.
    ///
    /// # Errors
    ///
    /// As [`Sandbox::render_bytes`].
    pub fn render(&self, name: &str, context: Array) -> Result<PhpString> {
        Ok(PhpString::from(self.sandbox.render_bytes(name, context)?))
    }
}

/// Whether the error is a sandbox violation, with its class (for messages and tests).
pub fn security_error_class(error: &Error) -> Option<&'static str> {
    SecurityError::of(error).map(SecurityError::class_name)
}
