//! The static helpers of `Twig\Extension\CoreExtension` (TWIG/src/Extension/CoreExtension.php) for host code:
//! `getAttribute()`, the attribute access of templates, and the helpers the core filters, functions and tests compile
//! to (`cycle()`, `random()`, `first()`, `include()`, ...), as associated functions of
//! [`CoreExtension`](crate::extension::CoreExtension).
//!
//! The helpers run the implementations templates use. Where Twig's helper takes what a compiled template injects
//! (`string $charset`, `Environment $env`, `bool $isSandboxed`), the Rust function takes the environment, which
//! gives them: its charset (what templates pass as `$charset`), and whether its sandbox is on (`$isSandboxed`). PHP
//! warnings and deprecations go to the environment, as when a template calls the helper.

use std::sync::{Arc, LazyLock};

use twig_value::object::CallArgs;
use twig_value::{Array, PhpError, Value};

use super::attribute::get_attribute_in;
use super::core::{compare_values, in_filter};
use super::environment::Environment;
use super::macros::MacroNamespace;
use crate::error::{Error, Result};
use crate::extension::callable::{FunctionOptions, PhpCallable, TwigCallable, TwigFunction};
use crate::extension::runtime::{CallableRuntime, call_callable, install_runtimes};
use crate::extension::{CoreExtension, ExtensionSet};
use crate::lltl::{GetAttrOp, Program};
use crate::loader::ArrayLoader;
use crate::source::Source;
use crate::vm::{ArgList, Exception, VmState};

/// The class of the helpers.
const CORE_EXTENSION: &str = "Twig\\Extension\\CoreExtension";

/// The static methods of `CoreExtension` the core callables run (and the instance methods `formatDate()`,
/// `modifyDate()`, `convertDate()` and `formatNumber()`, which use the settings of the environment's extension).
const HELPERS: &[&str] = &[
    "batch",
    "capitalize",
    "column",
    "constant",
    "convertDate",
    "convertEncoding",
    "cycle",
    "default",
    "enum",
    "enumCases",
    "filter",
    "find",
    "first",
    "formatDate",
    "formatNumber",
    "include",
    "includeOnly",
    "invoke",
    "join",
    "keys",
    "last",
    "length",
    "lower",
    "map",
    "merge",
    "modifyDate",
    "random",
    "reduce",
    "replace",
    "reverse",
    "round",
    "shuffle",
    "slice",
    "sort",
    "source",
    "spaceless",
    "split",
    "splitIntoCharacters",
    "sprintf",
    "striptags",
    "testEmpty",
    "testMapping",
    "testSequence",
    "titleCase",
    "trim",
    "upper",
    "urlencode",
];

/// A function per helper, named after it, whose PHP callable is `CoreExtension::<helper>` and whose runtime is the
/// core runtime of that callable.
static HELPER_SET: LazyLock<ExtensionSet> = LazyLock::new(|| {
    let mut set = ExtensionSet::new();
    for &helper in HELPERS {
        let callable = PhpCallable::method(CORE_EXTENSION, helper, Vec::new());
        // a fresh set accepts every name once
        let _ = set.add_function(TwigFunction::new(
            helper,
            Some(callable),
            FunctionOptions::default(),
        ));
    }
    set.initialize();
    install_runtimes(&set);
    set
});

/// The program a helper is called from: the core runtimes ask the calling template for its source (whether it runs
/// sandboxed), and a helper called by host code has none. An empty template without a name.
static HOST_PROGRAM: LazyLock<Option<Arc<Program>>> = LazyLock::new(|| {
    let env = Environment::new(ArrayLoader::new(Vec::<(String, String)>::new()));
    env.compile_source(&Source::new("", "")).ok()
});

/// Calls the core runtime of `CoreExtension::<method>` with `arguments` (after the injected ones) and the
/// context `context` (`needs_context`).
fn call_helper(
    env: &Environment,
    method: &str,
    context: Option<Array>,
    arguments: CallArgs,
) -> Result<Value> {
    // method names are case-insensitive
    let function = HELPERS
        .iter()
        .find(|helper| helper.eq_ignore_ascii_case(method))
        .and_then(|helper| HELPER_SET.get_function(helper.as_bytes()).ok().flatten())
        .filter(|function| function.callable().and_then(CallableRuntime::of).is_some());
    let (Some(function), Some(program)) = (function, HOST_PROGRAM.as_ref()) else {
        return Err(Error::php(PhpError::error(format!(
            "Call to undefined method {CORE_EXTENSION}::{method}()"
        ))));
    };
    let callable = TwigCallable::Function(function);
    let mut state = VmState::new(env.clone());
    let list = ArgList {
        args: arguments,
        context,
        unpacked: false,
    };
    call_callable(&mut state, program, &callable, list).map_err(Exception::into_error)
}

/// Positional arguments.
fn args<const N: usize>(values: [Value; N]) -> CallArgs {
    CallArgs::from(Vec::from(values))
}

impl CoreExtension {
    /// The attribute `item` of `object`, as a template reads it: `object.item` ([`CallType::Any`]), `object[item]`
    /// ([`CallType::Array`]) or `object.item(...arguments)` ([`CallType::Method`]), on behalf of the template whose
    /// source is `source`. Twig: `CoreExtension::getAttribute(Environment $env, Source $source, $object, $item, array
    /// $arguments = [], $type = Template::ANY_CALL, $isDefinedTest = false, $ignoreStrictCheck = false, $sandboxed =
    /// false, int $lineno = -1)`; `op` holds the last five (`line` `-1`: none).
    ///
    /// The lookup order is Twig's: array keys and `ArrayAccess` offsets, properties (and `__isset()`/`__get()`),
    /// class constants, then methods (`item()`, `getItem()`, `isItem()`, `hasItem()`, `__call()`); see the
    /// [guide](crate#exposing-rust-types-to-templates). A missing attribute is `null`, or with `strict_variables`
    /// the error a template gets (unless `op.ignore_strict_check`); with `op.defined_test` the result is whether
    /// the attribute exists. With `op.sandboxed` the environment's sandbox checks the methods and properties read.
    ///
    /// ```
    /// use twig::ast::CallType;
    /// use twig::extension::CoreExtension;
    /// use twig::lltl::GetAttrOp;
    /// use twig::{Array, Environment, EnvironmentOptions, Source, Value, context, loader::ArrayLoader};
    ///
    /// let options = EnvironmentOptions { strict_variables: true, ..Default::default() };
    /// let env = Environment::with_options(ArrayLoader::new([("index", "")]), options);
    /// let source = Source::new("", "index");
    /// let op = GetAttrOp {
    ///     call_type: CallType::Any,
    ///     defined_test: false,
    ///     ignore_strict_check: false,
    ///     sandboxed: false,
    ///     line: 3,
    /// };
    /// let user = Value::from(context! { name => "Fabien" });
    /// let name = CoreExtension::get_attribute(&env, &source, user.clone(), "name".into(), Array::new(), op)?;
    /// assert_eq!(name, Value::from("Fabien"));
    /// let error = CoreExtension::get_attribute(&env, &source, user, "age".into(), Array::new(), op).unwrap_err();
    /// assert_eq!(
    ///     error.to_string(),
    ///     "Key \"age\" for sequence/mapping with keys \"name\" does not exist in \"index\" at line 3.",
    /// );
    /// # Ok::<(), twig::Error>(())
    /// ```
    ///
    /// [`CallType::Any`]: crate::ast::CallType::Any
    /// [`CallType::Array`]: crate::ast::CallType::Array
    /// [`CallType::Method`]: crate::ast::CallType::Method
    ///
    /// # Errors
    ///
    /// The `RuntimeError`s of strict mode (with `op.line` and `source`), the sandbox's violations, and what a method
    /// or a property handler of `object` throws.
    pub fn get_attribute(
        env: &Environment,
        source: &Source,
        object: Value,
        item: Value,
        arguments: Array,
        op: GetAttrOp,
    ) -> Result<Value> {
        let mut state = VmState::new(env.clone());
        get_attribute_in(
            &mut state,
            source,
            object,
            item,
            Value::Array(arguments),
            &op,
        )
        .map_err(Exception::into_error)
    }

    /// Calls the static helper `CoreExtension::<method>()` with `arguments` (the arguments after those Twig injects,
    /// positional or named, bound to the PHP parameters as PHP binds them). The helpers are the PHP callables of the
    /// core filters, functions and tests: `batch`, `capitalize`, `column`, `constant`, `convertEncoding`, `cycle`,
    /// `default`, `enum`, `enumCases`, `filter`, `find`, `first`, `include`, `includeOnly`, `invoke`, `join`, `keys`,
    /// `last`, `length`, `lower`, `map`, `merge`, `random`, `reduce`, `replace`, `reverse`, `round`, `shuffle`,
    /// `slice`, `sort`, `source`, `spaceless`, `split`, `sprintf`, `striptags`, `testEmpty`, `testMapping`,
    /// `testSequence`, `titleCase`, `trim`, `upper`, `urlencode`, and the methods `formatDate`, `modifyDate`,
    /// `convertDate` and `formatNumber` of the environment's extension. What Twig injects comes from `env`: its
    /// charset, itself, whether its sandbox is on, and an empty context (see [`include`](Self::include) for a
    /// context).
    ///
    /// ```
    /// use twig::extension::CoreExtension;
    /// use twig::value::object::CallArgs;
    /// use twig::{Environment, Value, loader::ArrayLoader};
    ///
    /// let env = Environment::new(ArrayLoader::new([("index", "")]));
    /// // CoreExtension::join(['a', 'b', 'c'], ', ', ' and ')
    /// let arguments = CallArgs::new().arg(Value::from(vec!["a", "b", "c"])).arg(", ").arg(" and ");
    /// assert_eq!(CoreExtension::call_static(&env, "join", arguments)?, Value::from("a, b and c"));
    /// # Ok::<(), twig::Error>(())
    /// ```
    ///
    /// # Errors
    ///
    /// `Call to undefined method Twig\Extension\CoreExtension::%s()` (a PHP `Error`) for another name, PHP's
    /// binding errors, and what the helper throws.
    pub fn call_static(env: &Environment, method: &str, arguments: CallArgs) -> Result<Value> {
        call_helper(env, method, None, arguments)
    }

    /// Twig: `CoreExtension::cycle($values, $position)`: `$values[$position % count($values)]`.
    ///
    /// # Errors
    ///
    /// `The "cycle" function expects an array or "ArrayAccess" as first argument.`, `The "cycle" function expects a
    /// non-empty sequence.` (runtime errors), and the errors of reading the element.
    pub fn cycle(env: &Environment, values: Value, position: Value) -> Result<Value> {
        call_helper(env, "cycle", None, args([values, position]))
    }

    /// Twig: `CoreExtension::random(string $charset, $values = null, $max = null)`, with the environment's charset:
    /// a random integer, character or element (`null`, `null` for `mt_rand()`).
    ///
    /// # Errors
    ///
    /// `The "random" function cannot pick from an empty sequence or mapping.` and the errors of the conversions.
    pub fn random(env: &Environment, values: Value, max: Value) -> Result<Value> {
        call_helper(env, "random", None, args([values, max]))
    }

    /// Twig: `CoreExtension::first(string $charset, $item)`, with the environment's charset.
    ///
    /// # Errors
    ///
    /// The errors of iterating `item` or converting it to a string.
    pub fn first(env: &Environment, item: Value) -> Result<Value> {
        call_helper(env, "first", None, args([item]))
    }

    /// Twig: `CoreExtension::last(string $charset, $item)`, with the environment's charset.
    ///
    /// # Errors
    ///
    /// As [`first`](Self::first).
    pub fn last(env: &Environment, item: Value) -> Result<Value> {
        call_helper(env, "last", None, args([item]))
    }

    /// Twig: `CoreExtension::keys($array)`.
    ///
    /// # Errors
    ///
    /// The errors of iterating `array`.
    pub fn keys(env: &Environment, array: Value) -> Result<Value> {
        call_helper(env, "keys", None, args([array]))
    }

    /// Twig: `CoreExtension::slice(string $charset, $item, $start, $length = null, $preserveKeys = false)`, with the
    /// environment's charset.
    ///
    /// # Errors
    ///
    /// The errors of iterating `item`, converting it to a string, and of the PHP functions slicing it.
    pub fn slice(
        env: &Environment,
        item: Value,
        start: Value,
        length: Value,
        preserve_keys: Value,
    ) -> Result<Value> {
        call_helper(
            env,
            "slice",
            None,
            args([item, start, length, preserve_keys]),
        )
    }

    /// Twig: `CoreExtension::reverse(string $charset, $item, $preserveKeys = false)`, with the environment's charset.
    ///
    /// # Errors
    ///
    /// The errors of iterating `item` or converting it to a string.
    pub fn reverse(env: &Environment, item: Value, preserve_keys: Value) -> Result<Value> {
        call_helper(env, "reverse", None, args([item, preserve_keys]))
    }

    /// Twig: `CoreExtension::shuffle(string $charset, $item)`, with the environment's charset.
    ///
    /// # Errors
    ///
    /// As [`reverse`](Self::reverse).
    pub fn shuffle(env: &Environment, item: Value) -> Result<Value> {
        call_helper(env, "shuffle", None, args([item]))
    }

    /// Twig: `CoreExtension::length(string $charset, $thing)`, with the environment's charset.
    ///
    /// # Errors
    ///
    /// The errors of counting or iterating `thing`.
    pub fn length(env: &Environment, thing: Value) -> Result<Value> {
        call_helper(env, "length", None, args([thing]))
    }

    /// Twig: `CoreExtension::include(Environment $env, $context, $template, $variables = [], $withContext = true,
    /// $ignoreMissing = false)`: the output of the template (a name, a `Twig\TemplateWrapper` object or a list of
    /// names), rendered with `variables` merged into `context`, as a `Twig\Markup` (`''` when empty).
    ///
    /// # Errors
    ///
    /// The errors of loading and rendering the template (none for a missing template with `ignore_missing`).
    pub fn include(
        env: &Environment,
        context: Array,
        template: Value,
        variables: Value,
        with_context: bool,
        ignore_missing: bool,
    ) -> Result<Value> {
        call_helper(
            env,
            "include",
            Some(context),
            args([
                template,
                variables,
                Value::Bool(with_context),
                Value::Bool(ignore_missing),
            ]),
        )
    }

    /// Twig: `CoreExtension::source(Environment $env, $name, $ignoreMissing = false)`: the code of a template.
    ///
    /// # Errors
    ///
    /// The loader's errors (none for a missing template with `ignore_missing`).
    pub fn source(env: &Environment, name: Value, ignore_missing: bool) -> Result<Value> {
        call_helper(
            env,
            "source",
            None,
            args([name, Value::Bool(ignore_missing)]),
        )
    }

    /// Twig: `CoreExtension::compare($a, $b)`: PHP 8's comparison of two values (`-1`, `0` or `1`, and `1` for
    /// values that cannot be compared), with Twig's rules for numbers against numeric strings
    /// ([`compare_values`](super::compare_values)).
    ///
    /// # Errors
    ///
    /// The errors of comparing objects.
    pub fn compare(env: &Environment, a: &Value, b: &Value) -> Result<i64> {
        let mut state = VmState::new(env.clone());
        compare_values(a, b, &mut state).map_err(Exception::into_error)
    }

    /// Twig: `CoreExtension::inFilter($value, $compare)`: the `in` operator ([`in_filter`](super::in_filter)).
    ///
    /// # Errors
    ///
    /// The errors of iterating `compare` or comparing.
    pub fn in_filter(env: &Environment, value: &Value, compare: &Value) -> Result<bool> {
        let mut state = VmState::new(env.clone());
        in_filter(value, compare, &mut state).map_err(Exception::into_error)
    }

    /// Twig: `CoreExtension::callMacro(MacroNamespace $namespace, string $method, array $args, int $lineno, array
    /// $context, Source $source)` (to be removed in Twig 4.0): [`MacroNamespace::call`] with the name `method`
    /// without its `macro_` prefix (its first six bytes).
    ///
    /// # Errors
    ///
    /// As [`MacroNamespace::call`].
    pub fn call_macro(
        namespace: &MacroNamespace,
        method: &str,
        arguments: Array,
        line: i64,
        context: &Array,
        source: &Source,
    ) -> Result<Value> {
        // `substr($method, strlen('macro_'))`
        let name = method.as_bytes().get(6..).unwrap_or_default();
        namespace.call(name, arguments, context, line, source)
    }
}
