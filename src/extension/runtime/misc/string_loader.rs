//! The `template_from_string` function of the `StringLoaderExtension` (TWIG/src/Extension/StringLoaderExtension.php).

use twig_value::object::{FunctionKind, Param};
use twig_value::param::{DeclaredType, TypeMask, UserArg, verify_user_arg};
use twig_value::{Value, convert};

use super::bind_injected;
use crate::extension::runtime::{Call, CallResult};

/// The PHP name of the runtime, used in its argument errors.
const TEMPLATE_FROM_STRING: &str = "Twig\\Extension\\StringLoaderExtension::templateFromString";

/// Twig: `StringLoaderExtension::templateFromString(Environment $env, string|\Stringable $template, ?string $name =
/// null): TemplateWrapper`: `$env->createTemplate((string) $template, $name)`, a template named
/// `__string_template__<xxh128 of the source>` or `<name> (string template <hash>)`, as a `Twig\TemplateWrapper`
/// object ([`TemplateWrapper::to_value`](crate::runtime::TemplateWrapper::to_value)).
///
/// Errors: the `TypeError`s of the typed parameters (`Argument #2 ($template) must be of type Stringable|string`,
/// `Argument #3 ($name) must be of type ?string`) and the errors of compiling the template (a `SyntaxError` in the
/// generated name).
pub(super) fn template_from_string(call: &mut Call<'_>) -> CallResult {
    let args = bind_injected(
        call,
        TEMPLATE_FROM_STRING,
        1,
        &[
            Param::required("env"),
            Param::required("template"),
            Param::optional("name", Value::Null),
        ],
        FunctionKind::User,
    )?;
    let mut args = args.into_iter();
    let diag = call.diagnostics();
    let template = verify_user_arg(
        args.next().unwrap_or_default(),
        &DeclaredType::new(&["Stringable"], TypeMask::STRING),
        &UserArg::new(TEMPLATE_FROM_STRING, 2, "template"),
        diag,
    )?;
    let name = verify_user_arg(
        args.next().unwrap_or_default(),
        &DeclaredType::builtin(TypeMask::STRING | TypeMask::NULL),
        &UserArg::new(TEMPLATE_FROM_STRING, 3, "name"),
        diag,
    )?;
    let code = convert::to_string(&template, diag)?;
    let name = match &name {
        Value::Null => None,
        other => Some(convert::to_string(other, diag)?),
    };
    let name = name.as_ref().map(|name| name.to_string_lossy());
    let wrapper = call.env().create_template(&code, name.as_deref())?;
    Ok(wrapper.to_value())
}
