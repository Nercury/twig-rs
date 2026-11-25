//! The template functions that load and render other templates: `include`, `include_only` and `source`
//! (`CoreExtension`) and `render_sandboxed` (`SandboxBridgeExtension`). `template_from_string`
//! (`StringLoaderExtension`) is one of the misc callables (`crate::extension::runtime::misc`).

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

/// Twig: `CoreExtension::include($env, $context, $template, $variables = [], $withContext = true, $ignoreMissing =
/// false, $sandboxed = false)`: renders the template (a name, a `TemplateWrapper` or an array of names tried in turn)
/// with the variables merged into the context (`array_merge()`), and returns the output as a `Markup` (`''` when
/// empty); a missing template is skipped with `ignore_missing`; `sandboxed` (deprecated) switches the sandbox on.
fn include(call: &mut Call<'_>) -> CallResult {
    let args = call.args().to_vec();
    let template = args.first().cloned().unwrap_or_default();
    let variables = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| Value::Array(Array::new()));
    let with_context = match args.get(2) {
        Some(value) => truthy(call, value)?,
        None => true,
    };
    let ignore_missing = match args.get(3) {
        Some(value) => truthy(call, value)?,
        None => false,
    };
    let sandboxed = match args.get(4) {
        Some(value) => {
            let sandboxed = truthy(call, value)?;
            // `func_num_args() >= 7`
            call.deprecate(Deprecation::twig(
                "3.29",
                if sandboxed {
                    "The \"sandboxed\" argument of the \"include\" function is deprecated, use the \"render_sandboxed\" function to render untrusted templates instead."
                } else {
                    "The \"sandboxed\" argument of the \"include\" function is deprecated, remove the argument as \"false\" has no effect."
                },
            ));
            sandboxed
        }
        None => false,
    };
    let context = call.context();
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
        let mut out = Output::new(&mut buffer, false);
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

/// Twig: `CoreExtension::includeOnly($env, $template, array $variables = [], bool $ignoreMissing = false)`:
/// `include()` without the context and without the `sandboxed` argument.
fn include_only(call: &mut Call<'_>) -> CallResult {
    const FUNCTION: &str = "Twig\\Extension\\CoreExtension::includeOnly";
    let args = call.args().to_vec();
    let template = args.first().cloned().unwrap_or_default();
    let variables = match args.get(1) {
        Some(value) => typed(
            call,
            value.clone(),
            FUNCTION,
            3,
            "variables",
            DeclaredType::builtin(TypeMask::ARRAY),
        )?,
        None => Value::Array(Array::new()),
    };
    let ignore_missing = match args.get(2) {
        Some(value) => {
            let value = typed(
                call,
                value.clone(),
                FUNCTION,
                4,
                "ignoreMissing",
                DeclaredType::builtin(TypeMask::BOOL),
            )?;
            truthy(call, &value)?
        }
        None => false,
    };
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

/// Twig: `CoreExtension::source($env, $name, $ignoreMissing = false)`: the source code of a template, not rendered
/// (`''` for a missing template with `ignore_missing`).
fn source(call: &mut Call<'_>) -> CallResult {
    let args = call.args().to_vec();
    let name = args.first().cloned().unwrap_or_default();
    let ignore_missing = match args.get(1) {
        Some(value) => truthy(call, value)?,
        None => false,
    };
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
    let args = call.args().to_vec();
    let name = typed(
        call,
        args.first().cloned().unwrap_or_default(),
        FUNCTION,
        1,
        "name",
        DeclaredType::builtin(TypeMask::STRING),
    )?;
    let context = typed(
        call,
        args.get(1).cloned().unwrap_or_default(),
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
