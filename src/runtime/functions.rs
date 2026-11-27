//! The template functions that load and render other templates: `include`, `include_only` and `source`
//! (`CoreExtension`) and `render_sandboxed` (`SandboxBridgeExtension`). `template_from_string`
//! (`StringLoaderExtension`) is one of the misc callables (`crate::extension::runtime::misc`).

use twig_value::object::{CallArgs, FunctionKind, Param};
use twig_value::param::{DeclaredType, TypeMask, UserArg, verify_user_arg};
use twig_value::{Array, PhpString, Value, convert};

use super::composition::{is_loader_error, resolve_template, yield_template};
use super::markup::Markup;
use super::sandbox::SandboxBridgeRuntime;
use super::template::Blocks;
use crate::deprecation::Deprecation;
use crate::error::Error;
use crate::extension::runtime::{Call, CallResult, RuntimeTable};
use crate::vm::{Exception, Output};

/// Registers the runtimes of the template functions.
pub(crate) fn register(table: &mut RuntimeTable) {
    table.register("Twig\\Extension\\CoreExtension::include", include);
    table.register("Twig\\Extension\\CoreExtension::includeOnly", include_only);
    table.register("Twig\\Extension\\CoreExtension::source", source);
    table.register(
        "Twig\\Runtime\\SandboxBridgeRuntime::render",
        render_sandboxed,
    );
}

/// A userland parameter in coercive typing mode.
fn typed(
    call: &mut Call<'_>,
    value: Value,
    function: &str,
    position: u32,
    name: &str,
    declared: DeclaredType<'_>,
) -> CallResult {
    Ok(verify_user_arg(
        value,
        &declared,
        &UserArg::new(function, position, name),
        call.diagnostics(),
    )?)
}

fn truthy(call: &mut Call<'_>, value: &Value) -> Result<bool, Exception> {
    Ok(convert::to_bool(value, call.diagnostics())?)
}

/// The names of the parameters Twig's injected arguments fill, in order (`Environment $env`, `$context`).
const INJECTED_NAMES: [&str; 2] = ["env", "context"];

/// The arguments of a call bound to a userland method as PHP binds them when the compiled template calls it: the
/// `injected` arguments Twig passes first (`needs_environment`, `needs_context`) take the first parameters, then the
/// positional template arguments, then the named arguments of spreads (`include(...{'template': 'b.twig'})`) by PHP
/// parameter name; missing optional parameters get their defaults and extra positional arguments are ignored.
///
/// Returns one value per parameter of `params` (the injected ones excluded) and `func_num_args()`: the number of
/// positional arguments, or one past the last parameter a named argument filled when that is larger
/// (`zend_handle_named_arg()`, PHPSRC/Zend/zend_execute.c).
///
/// # Errors
///
/// PHP's binding errors (`Unknown named parameter $ignore_missing`, `Named parameter $x overwrites previous
/// argument`, `Too few arguments to function X(), 1 passed and at least 2 expected` without PHP's ` in <file> on line
/// N`, reference/deviations.md).
fn bind_method(
    call: &Call<'_>,
    function: &str,
    injected: &[Value],
    params: &[Param],
) -> CallResult<(Vec<Value>, usize)> {
    let mut all: Vec<Param> = INJECTED_NAMES[..injected.len()]
        .iter()
        .map(|name| Param::required(name))
        .collect();
    all.extend_from_slice(params);
    let mut positional = injected.to_vec();
    positional.extend_from_slice(call.args());
    let args = CallArgs {
        positional,
        named: call.named_args().to_vec(),
    };
    let mut num_args = args.positional.len();
    for (name, _) in &args.named {
        if let Some(i) = all
            .iter()
            .position(|param| param.name.as_bytes() == name.as_bytes())
        {
            num_args = num_args.max(i + 1);
        }
    }
    let mut bound = args.bind(function, &all, FunctionKind::User)?;
    Ok((bound.split_off(injected.len()), num_args))
}

/// The bound values in parameter order.
fn take<const N: usize>(bound: Vec<Value>) -> [Value; N] {
    let mut values = bound.into_iter();
    std::array::from_fn(|_| values.next().unwrap_or_default())
}

/// Twig: `CoreExtension::include(Environment $env, $context, $template, $variables = [], $withContext = true,
/// $ignoreMissing = false, $sandboxed = false)` (TWIG/src/Extension/CoreExtension.php): renders the template (a name,
/// a `TemplateWrapper` or an array of names tried in turn) with the variables merged into the context
/// (`array_merge()`), and returns the output as a `Markup` (`''` when empty); a missing template is skipped with
/// `ignore_missing`; `sandboxed` (deprecated when `func_num_args() >= 7`) switches the sandbox on. The arguments are
/// bound to the PHP parameters as PHP binds them ([`bind_method`]).
fn include(call: &mut Call<'_>) -> CallResult {
    let context = call.context();
    let (bound, num_args) = bind_method(
        call,
        "Twig\\Extension\\CoreExtension::include",
        &[Value::Null, Value::Array(context.clone())],
        &[
            Param::required("template"),
            Param::optional("variables", Value::Array(Array::new())),
            Param::optional("withContext", true),
            Param::optional("ignoreMissing", false),
            Param::optional("sandboxed", false),
        ],
    )?;
    let [template, variables, with_context, ignore_missing, sandboxed] = take(bound);
    let sandboxed = truthy(call, &sandboxed)?;
    if num_args >= 7 {
        call.deprecate(Deprecation::twig(
            "3.29",
            if sandboxed {
                "The \"sandboxed\" argument of the \"include\" function is deprecated, use the \"render_sandboxed\" function to render untrusted templates instead."
            } else {
                "The \"sandboxed\" argument of the \"include\" function is deprecated, remove the argument as \"false\" has no effect."
            },
        ));
    }
    let with_context = truthy(call, &with_context)?;
    let ignore_missing = truthy(call, &ignore_missing)?;
    do_include(
        call,
        context,
        template,
        variables,
        with_context,
        ignore_missing,
        sandboxed,
    )
}

/// The body of `CoreExtension::include()`.
fn do_include(
    call: &mut Call<'_>,
    context: Array,
    template: Value,
    variables: Value,
    with_context: bool,
    ignore_missing: bool,
    sandboxed: bool,
) -> CallResult {
    let variables = if with_context {
        Value::Array(twig_value::builtins::array::array_merge(&[
            Value::Array(context),
            variables,
        ])?)
    } else {
        variables
    };
    let checker = if sandboxed
        && call
            .env()
            .has_extension("Twig\\Extension\\SandboxExtension")
    {
        call.env().sandbox_checker()
    } else {
        None
    };
    let already = checker.as_ref().map(|checker| {
        let already = checker.is_sandboxed(None);
        if !already {
            checker.set_template_sandboxed(true);
        }
        already
    });
    let result = render_included(call, &template, variables, ignore_missing);
    if let (Some(checker), Some(false)) = (&checker, already) {
        checker.set_template_sandboxed(false);
    }
    result
}

/// `$loaded = $env->resolveTemplate($template)` (a `LoaderError` is ignored with `ignore_missing`), `$rendered =
/// $loaded->render($variables)`, `'' === $rendered ? '' : new Markup($rendered, $env->getCharset())`.
fn render_included(
    call: &mut Call<'_>,
    template: &Value,
    variables: Value,
    ignore_missing: bool,
) -> CallResult {
    let loaded = match resolve_template(call.state(), template) {
        Ok(loaded) => loaded,
        Err(error) if ignore_missing && is_loader_error(&error) => {
            return Ok(Value::String(PhpString::empty()));
        }
        Err(error) => return Err(error),
    };
    // `TemplateWrapper::render(array $context = [])`
    let variables = match typed(
        call,
        variables,
        "Twig\\TemplateWrapper::render",
        1,
        "context",
        DeclaredType::builtin(TypeMask::ARRAY),
    )? {
        Value::Array(variables) => variables,
        _ => Array::new(),
    };
    let mut buffer: Vec<u8> = Vec::new();
    {
        let mut out = Output::buffer(&mut buffer);
        yield_template(
            call.state(),
            &mut out,
            loaded.template(),
            variables,
            &Blocks::default(),
        )?;
    }
    if buffer.is_empty() {
        return Ok(Value::String(PhpString::empty()));
    }
    Ok(Markup::new_value(PhpString::from(buffer), call.charset()))
}

/// Twig: `CoreExtension::includeOnly(Environment $env, $template, array $variables = [], bool $ignoreMissing =
/// false)`: `include()` without the context and without the `sandboxed` argument; the typed parameters are checked in
/// coercive mode, in order, once the arguments are bound ([`bind_method`]).
fn include_only(call: &mut Call<'_>) -> CallResult {
    const FUNCTION: &str = "Twig\\Extension\\CoreExtension::includeOnly";
    let (bound, _) = bind_method(
        call,
        FUNCTION,
        &[Value::Null],
        &[
            Param::required("template"),
            Param::optional("variables", Value::Array(Array::new())),
            Param::optional("ignoreMissing", false),
        ],
    )?;
    let [template, variables, ignore_missing] = take(bound);
    let variables = typed(
        call,
        variables,
        FUNCTION,
        3,
        "variables",
        DeclaredType::builtin(TypeMask::ARRAY),
    )?;
    let ignore_missing = typed(
        call,
        ignore_missing,
        FUNCTION,
        4,
        "ignoreMissing",
        DeclaredType::builtin(TypeMask::BOOL),
    )?;
    let ignore_missing = truthy(call, &ignore_missing)?;
    do_include(
        call,
        Array::new(),
        template,
        variables,
        false,
        ignore_missing,
        false,
    )
}

/// Twig: `CoreExtension::source(Environment $env, $name, $ignoreMissing = false): string`: the source code of a
/// template, not rendered (`''` for a missing template with `ignore_missing`); the arguments are bound as PHP binds
/// them ([`bind_method`]).
fn source(call: &mut Call<'_>) -> CallResult {
    let (bound, _) = bind_method(
        call,
        "Twig\\Extension\\CoreExtension::source",
        &[Value::Null],
        &[
            Param::required("name"),
            Param::optional("ignoreMissing", false),
        ],
    )?;
    let [name, ignore_missing] = take(bound);
    let ignore_missing = truthy(call, &ignore_missing)?;
    let loader = call.env().loader();
    // `LoaderInterface::getSourceContext(string $name)`
    let function = format!("{}::getSourceContext", loader.class_name());
    let name = match typed(
        call,
        name,
        &function,
        1,
        "name",
        DeclaredType::builtin(TypeMask::STRING),
    )? {
        Value::String(name) => name,
        _ => PhpString::empty(),
    };
    match loader.get_source_context(&name.to_string_lossy()) {
        Ok(source) => Ok(Value::String(PhpString::from(source.code()))),
        Err(error) if ignore_missing && error.kind() == crate::ErrorKind::Loader => {
            Ok(Value::String(PhpString::empty()))
        }
        Err(error) => Err(error.into()),
    }
}

/// Twig: `SandboxBridgeRuntime::render(string $name, array $context): string`, reached through
/// `$this->env->getRuntime(SandboxBridgeRuntime::class)`.
fn render_sandboxed(call: &mut Call<'_>) -> CallResult {
    const FUNCTION: &str = "Twig\\Runtime\\SandboxBridgeRuntime::render";
    let runtime = call.env().runtime(SandboxBridgeRuntime::CLASS)?;
    let Some(runtime) = runtime.downcast_ref::<SandboxBridgeRuntime>() else {
        return Err(Error::runtime(format!(
            "Unable to load the \"{}\" runtime.",
            SandboxBridgeRuntime::CLASS
        ))
        .into());
    };
    let runtime = runtime.clone();
    let (bound, _) = bind_method(
        call,
        FUNCTION,
        &[],
        &[Param::required("name"), Param::required("context")],
    )?;
    let [name, context] = take(bound);
    let name = typed(
        call,
        name,
        FUNCTION,
        1,
        "name",
        DeclaredType::builtin(TypeMask::STRING),
    )?;
    let context = typed(
        call,
        context,
        FUNCTION,
        2,
        "context",
        DeclaredType::builtin(TypeMask::ARRAY),
    )?;
    let name = match name {
        Value::String(name) => name.to_string_lossy().into_owned(),
        _ => String::new(),
    };
    let context = match context {
        Value::Array(context) => context,
        _ => Array::new(),
    };
    Ok(Value::String(runtime.render(&name, context)?))
}
