//! Macros at runtime: `Twig\MacroNamespace` (the namespace an import binds, TWIG/src/MacroNamespace.php) and
//! `Twig\TwigMacro` (a macro of a template, TWIG/src/TwigMacro.php), with PHP's binding of the arguments to the
//! parameters of the compiled macro closure.

use std::fmt;
use std::sync::{Arc, LazyLock, Weak};

use twig_value::object::{ClassBuilder, ClassRef, Object, ObjectRef};
use twig_value::{Array, ArrayKey, PhpError, PhpString, Value};

use super::composition::get_parent;
use super::environment::Environment;
use super::sandbox::ensure_security_checked;
use super::template::{Blocks, Template};
use crate::ast::{RESERVED_NAME_PREFIX, RESERVED_NAMES, VARARGS_NAME};
use crate::deprecation::{Deprecation, DeprecationSink};
use crate::error::{Error, Result};
use crate::lltl::{MacroDef, MacroParam};
use crate::source::Source;
use crate::vm::{Exception, Frame, Output, VmState, run_unit};

/// `Twig\Node\Expression\MacroReferenceExpression`, named by the `macro_` prefix deprecations.
const MACRO_REFERENCE_CLASS: &str = "Twig\\Node\\Expression\\MacroReferenceExpression";

static CLASS: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::class("Twig\\MacroNamespace")
        .final_class()
        .build()
});

/// The name of the variadic bucket of a macro without a declared one.
static VARARGS: LazyLock<PhpString> = LazyLock::new(|| PhpString::from(VARARGS_NAME));

/// The `Twig\MacroNamespace` object of a template, as `{% import %}`, `{% from %}` and `_self` bind it in `$macros`.
/// It refers to its template weakly (templates import each other and themselves; the environment keeps the
/// templates alive, and each template keeps its namespace object), so `$this->macros` never forms a reference cycle.
struct NamespaceObject {
    template: Weak<Template>,
}

impl Object for NamespaceObject {
    fn class(&self) -> &ClassRef {
        &CLASS
    }
}

/// The macros of a template, as `{% import %}` binds them: Twig's `Twig\MacroNamespace`
/// (TWIG/src/MacroNamespace.php), from [`TemplateWrapper::macro_namespace`](super::TemplateWrapper::macro_namespace).
///
/// [`call`](Self::call) and [`has`](Self::has) are the calls a template compiles `forms.input(...)` and `forms.input
/// is defined` to: the macro is looked up in the template, then in its parents (resolved with the given context),
/// with Twig's deprecated fallbacks (a name differing in case, a `macro_` prefix), and the arguments are handled by
/// `TwigMacro::callLegacy()` (see [`TwigMacro`]). The macro body renders with its arguments and the globals, and
/// its PHP warnings and deprecations go to the environment, as when a template calls it.
#[derive(Clone)]
pub struct MacroNamespace {
    env: Environment,
    template: Arc<Template>,
}

impl fmt::Debug for MacroNamespace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MacroNamespace")
            .field("template", &self.template.name())
            .finish_non_exhaustive()
    }
}

impl MacroNamespace {
    /// The namespace of `template`, a template of `env`.
    pub(crate) fn new(env: Environment, template: Arc<Template>) -> Self {
        MacroNamespace { env, template }
    }

    /// Twig: `Template::getMacroNamespace()`: the namespace object of the template (`$this->macroNamespace ??= new
    /// MacroNamespace($this, $this->loadDeclaredMacros())`), the same object every time.
    pub(crate) fn value(template: &Arc<Template>) -> Value {
        template.macro_namespace_value(|| {
            Value::Object(ObjectRef::new(NamespaceObject {
                template: Arc::downgrade(template),
            }))
        })
    }

    /// The template of a namespace value.
    fn template_of(value: &Value) -> std::result::Result<Arc<Template>, Exception> {
        let template = match value {
            Value::Object(object) => object
                .downcast_ref::<NamespaceObject>()
                .and_then(|namespace| namespace.template.upgrade()),
            _ => None,
        };
        template.ok_or_else(|| {
            Error::logic("The template of a macro namespace is no longer loaded.").into()
        })
    }

    /// The template whose macros the namespace holds.
    pub fn template(&self) -> &Arc<Template> {
        &self.template
    }

    /// The namespace as a PHP object of the class `Twig\MacroNamespace` (the same object for every namespace of the
    /// template), for instance to check a [`SecurityPolicy`](super::SecurityPolicy) against it.
    pub fn to_value(&self) -> Value {
        MacroNamespace::value(&self.template)
    }

    /// Whether the macro `name` is defined in the template or its parents (the dynamic ones resolved with
    /// `context`). Twig: `MacroNamespace::has(string $name, array $context)`, what `macros.name is defined`
    /// compiles to: a name that differs from the macro's in case, or the macro's name with a `macro_` prefix, is
    /// found with a deprecation.
    ///
    /// # Errors
    ///
    /// The errors of resolving a dynamic parent.
    pub fn has(&self, name: impl AsRef<[u8]>, context: &Array) -> Result<bool> {
        let mut state = VmState::new(self.env.clone());
        let name = Value::String(PhpString::from(name.as_ref()));
        has(&mut state, &self.to_value(), &name, context).map_err(Exception::into_error)
    }

    /// Calls the macro `name` with `arguments` (positional arguments under their position, named arguments under
    /// their name) on behalf of a template whose source is `source`, at line `line`. Twig: `MacroNamespace::call(string
    /// $name, array $arguments, array $context, int $line, Source $source)`, what `macros.name(...)` compiles to.
    ///
    /// Returns the output of the macro: a `Twig\Markup` ([`Markup`](super::Markup)), or `''` when it is empty.
    ///
    /// # Errors
    ///
    /// `Macro "%s" is not defined in template "%s".` (at `line` in `source`), the errors of
    /// `TwigMacro::callLegacy()` (see [`TwigMacro::call_legacy`]) and what the macro body throws.
    pub fn call(
        &self,
        name: impl AsRef<[u8]>,
        arguments: Array,
        context: &Array,
        line: i64,
        source: &Source,
    ) -> Result<Value> {
        let mut state = VmState::new(self.env.clone());
        let name = Value::String(PhpString::from(name.as_ref()));
        call(
            &mut state,
            &self.to_value(),
            &name,
            Value::Array(arguments),
            context,
            line,
            source,
        )
        .map_err(Exception::into_error)
    }
}

/// Twig: `Template::throwUninitializedMacroNamespace()`: a macro namespace imported in the body of the running
/// template was used where the body did not run (`$macros["name"] ?? ...`).
pub(crate) fn uninitialized_namespace(template: &Template, line: i64) -> Exception {
    Error::runtime(format!(
        "Macros imported in the body of template \"{}\" are not available because the body was not rendered; move the \"import\" or \"from\" tag inside the block or the macro that uses it.",
        template.name()
    ))
    .with_line(line)
    .with_source(template.source().clone())
    .into()
}

/// A macro of a namespace: its template and its index in the template's macros.
type Found = (Arc<Template>, usize);

/// Twig: `MacroNamespace::getDeclared()`: the template's own macro of that name, else (deprecated) one whose name
/// differs only in case; the sandbox checks of the template run when one is found.
fn get_declared(
    state: &mut VmState,
    template: &Arc<Template>,
    name: &[u8],
) -> Result<Option<Found>, Exception> {
    let macros = template.program().macros();
    if let Some(i) = macros.iter().position(|m| m.name.as_slice() == name) {
        ensure_security_checked(state, template)?;
        return Ok(Some((Arc::clone(template), i)));
    }
    if let Some(i) = macros
        .iter()
        .position(|m| m.name.eq_ignore_ascii_case(name))
    {
        case_deprecation(state, &macros[i].name, template.name(), name);
        ensure_security_checked(state, template)?;
        return Ok(Some((Arc::clone(template), i)));
    }
    Ok(None)
}

#[cold]
fn case_deprecation(state: &mut VmState, declared: &[u8], template: &str, name: &[u8]) {
    state.env.report_deprecation(Deprecation::twig(
        "3.29",
        format!(
            "Calling the macro \"{}\" (defined in template \"{template}\") as \"{}\" is deprecated; macro names will be case-sensitive in Twig 4.0.",
            String::from_utf8_lossy(declared),
            String::from_utf8_lossy(name)
        ),
    ));
}

#[cold]
fn not_defined(name: &PhpString, template: &str, line: i64, source: &Source) -> Exception {
    Error::runtime(format!(
        "Macro \"{}\" is not defined in template \"{template}\".",
        name.to_string_lossy()
    ))
    .with_line(line)
    .with_source(source.clone())
    .into()
}

#[cold]
fn prefix_deprecation(state: &mut VmState, bare: &[u8], name: &PhpString) {
    state.env.report_deprecation(Deprecation::twig(
        "3.29",
        format!(
            "Calling the macro \"{}\" via the \"macro_\"-prefixed name \"{}\" is deprecated; pass the bare macro name to \"{}\" instead.",
            String::from_utf8_lossy(bare),
            name.to_string_lossy(),
            MACRO_REFERENCE_CLASS
        ),
    ));
}

#[cold]
fn misordered_error(macro_name: &[u8], line: i64, source: &Source) -> Exception {
    Error::runtime(format!(
        "Positional arguments cannot be used after named arguments for macro \"{}\".",
        String::from_utf8_lossy(macro_name)
    ))
    .with_line(line)
    .with_source(source.clone())
    .into()
}

#[cold]
fn duplicate_error(
    duplicate: &PhpString,
    macro_name: &[u8],
    line: i64,
    source: &Source,
) -> Exception {
    Error::runtime(format!(
        "Argument \"{}\" is defined twice for macro \"{}\".",
        duplicate.to_string_lossy(),
        String::from_utf8_lossy(macro_name)
    ))
    .with_line(line)
    .with_source(source.clone())
    .into()
}

#[cold]
fn overwrite_error(name: &[u8]) -> Exception {
    PhpError::error(format!(
        "Named parameter ${} overwrites previous argument",
        String::from_utf8_lossy(&parameter_name(name))
    ))
    .into()
}

/// Twig: `MacroNamespace::getParent()`: the namespace of the template's parent (resolved with the caller's context).
fn namespace_parent(
    state: &mut VmState,
    template: &Arc<Template>,
    context: &Array,
) -> Result<Option<Arc<Template>>, Exception> {
    get_parent(state, template, context)
}

/// Twig: `MacroNamespace::resolve()`: the declared macro of the namespace or of its parents.
fn resolve(
    state: &mut VmState,
    template: &Arc<Template>,
    name: &[u8],
    context: &Array,
) -> Result<Option<Found>, Exception> {
    let mut namespace = Arc::clone(template);
    loop {
        if let Some(found) = get_declared(state, &namespace, name)? {
            return Ok(Some(found));
        }
        match namespace_parent(state, &namespace, context)? {
            Some(parent) => namespace = parent,
            None => return Ok(None),
        }
    }
}

/// Twig: `MacroNamespace::findDeclaredName()`: the declared name (exact, else case-insensitive) and the template
/// name of the macro in the namespace or its parents.
fn find_declared_name(
    state: &mut VmState,
    template: &Arc<Template>,
    name: &[u8],
    context: &Array,
) -> Result<Option<(Vec<u8>, String)>, Exception> {
    let mut namespace = Arc::clone(template);
    loop {
        let macros = namespace.program().macros();
        if let Some(m) = macros.iter().find(|m| m.name.as_slice() == name) {
            return Ok(Some((m.name.clone(), namespace.name().to_owned())));
        }
        if let Some(m) = macros.iter().find(|m| m.name.eq_ignore_ascii_case(name)) {
            return Ok(Some((m.name.clone(), namespace.name().to_owned())));
        }
        match namespace_parent(state, &namespace, context)? {
            Some(parent) => namespace = parent,
            None => return Ok(None),
        }
    }
}

fn name_bytes(name: &Value) -> PhpString {
    match name {
        Value::String(name) => name.clone(),
        _ => PhpString::empty(),
    }
}

/// Twig: `MacroNamespace::has()`: the `is defined` test of a macro (case-insensitive and `macro_`-prefixed names are
/// deprecated matches).
pub(crate) fn has(
    state: &mut VmState,
    namespace: &Value,
    name: &Value,
    context: &Array,
) -> Result<bool, Exception> {
    let template = MacroNamespace::template_of(namespace)?;
    let name = name_bytes(name);
    if let Some((declared, template_name)) =
        find_declared_name(state, &template, name.as_bytes(), context)?
    {
        if declared.as_slice() != name.as_bytes() {
            state.env.report_deprecation(Deprecation::twig(
                "3.29",
                format!(
                    "Testing whether the macro \"{}\" (defined in template \"{}\") is defined as \"{}\" is deprecated; macro names will be case-sensitive in Twig 4.0 and this test will return false.",
                    String::from_utf8_lossy(&declared),
                    template_name,
                    name.to_string_lossy()
                ),
            ));
        }
        return Ok(true);
    }
    let Some(bare) = name.as_bytes().strip_prefix(b"macro_") else {
        return Ok(false);
    };
    if find_declared_name(state, &template, bare, context)?.is_none() {
        return Ok(false);
    }
    state.env.report_deprecation(Deprecation::twig(
        "3.29",
        format!(
            "Testing whether the macro \"{}\" is defined via the \"macro_\"-prefixed name \"{}\" is deprecated; pass the bare macro name to \"{}\" instead.",
            String::from_utf8_lossy(bare),
            name.to_string_lossy(),
            MACRO_REFERENCE_CLASS
        ),
    ));
    Ok(true)
}

/// Twig: `MacroNamespace::call()`: the macro of the namespace, else of its parents (with the caller's context),
/// else (deprecated) the macro named without a `macro_` prefix; then `TwigMacro::callLegacy()`.
///
/// # Errors
///
/// `Macro "%s" is not defined in template "%s".` (at the caller's line and source), the errors of the arguments and
/// whatever the macro body raises (a macro has no exception handler: the caller's template handles it).
pub(crate) fn call(
    state: &mut VmState,
    namespace: &Value,
    name: &Value,
    arguments: Value,
    context: &Array,
    line: i64,
    source: &Source,
) -> Result<Value, Exception> {
    let template = MacroNamespace::template_of(namespace)?;
    let name = name_bytes(name);
    let arguments = match arguments {
        Value::Array(arguments) => arguments,
        _ => Array::new(),
    };
    if let Some(found) = get_declared(state, &template, name.as_bytes())? {
        return call_legacy(state, found, arguments, source, line);
    }
    let mut found = None;
    if let Some(parent) = namespace_parent(state, &template, context)? {
        found = resolve(state, &parent, name.as_bytes(), context)?;
    }
    let found = match found {
        Some(found) => found,
        None => {
            let resolved = match name.as_bytes().strip_prefix(b"macro_") {
                Some(bare) => resolve(state, &template, bare, context)?.map(|found| (bare, found)),
                None => None,
            };
            let Some((bare, found)) = resolved else {
                return Err(not_defined(&name, template.name(), line, source));
            };
            prefix_deprecation(state, bare, &name);
            found
        }
    };
    call_legacy(state, found, arguments, source, line)
}

/// `array_is_list()`.
fn is_list(array: &Array) -> bool {
    array
        .keys()
        .enumerate()
        .all(|(i, key)| matches!(key, ArrayKey::Int(k) if *k == i as i64))
}

/// The signature `TwigMacro`'s constructor gets (the macro name, the arguments with their defaults, whether the
/// macro is variadic) and the facts it derives from it: `$argumentIndexes`, `$requiredNames`, `$requiredCount`.
struct Signature<'a> {
    name: &'a [u8],
    params: &'a [MacroParam],
    variadic: bool,
    required_count: usize,
}

impl<'a> Signature<'a> {
    fn new(name: &'a [u8], params: &'a [MacroParam], variadic: bool) -> Self {
        let required_count = params
            .iter()
            .rposition(|param| !param.explicit_default)
            .map_or(0, |i| i + 1);
        Signature {
            name,
            params,
            variadic,
            required_count,
        }
    }

    fn index_of(&self, name: &[u8]) -> Option<usize> {
        self.params
            .iter()
            .position(|param| param.name.as_bytes() == name)
    }

    fn is_required(&self, name: &[u8]) -> bool {
        self.params
            .iter()
            .any(|param| param.name.as_bytes() == name && !param.explicit_default)
    }
}

/// Reports deprecations to an environment.
struct EnvSink<'a>(&'a Environment);

impl DeprecationSink for EnvSink<'_> {
    fn report(&mut self, deprecation: Deprecation) {
        self.0.report_deprecation(deprecation);
    }
}

/// Twig: `TwigMacro::callLegacy()` of a macro of a template: [`legacy_arguments`], then the macro body runs with
/// its arguments ([`run_macro`]).
fn call_legacy(
    state: &mut VmState,
    (template, index): Found,
    arguments: Array,
    source: &Source,
    line: i64,
) -> Result<Value, Exception> {
    let program = Arc::clone(template.program());
    let def = &program.macros()[index];
    let signature = Signature::new(&def.name, &def.params, def.variadic.is_some());
    let (values, bucket) = legacy_arguments(
        &signature,
        &arguments,
        source,
        line,
        &mut EnvSink(&state.env),
    )?;
    let mut context = Array::new();
    for (param, value) in def.params.iter().zip(values) {
        context.insert(ArrayKey::from_string(param.name.clone()), value);
    }
    run_macro(state, &template, def, context, bucket)
}

/// Twig: `TwigMacro::callLegacy()` up to the call of the body: the lenient Twig 3 argument handling, with
/// deprecations for what Twig 4 rejects and errors for what PHP would reject with its own wording; then the
/// arguments are bound to the parameters of the body as `($this->body)(...$arguments)` binds them
/// ([`bind_arguments`]).
fn legacy_arguments(
    signature: &Signature<'_>,
    arguments: &Array,
    source: &Source,
    line: i64,
    deprecations: &mut dyn DeprecationSink,
) -> Result<(Vec<Value>, Array), Exception> {
    let declared = signature.params.len();
    if is_list(arguments) {
        let count = arguments.len();
        if count < signature.required_count || (!signature.variadic && count > declared) {
            trigger_legacy_deprecations(deprecations, signature, arguments, count, source, line);
        }
        return bind_arguments(signature.params, arguments);
    }

    // every declared argument is named: none can be missing, unknown, duplicated, or misordered
    let all_named = arguments.len() == declared
        && arguments.keys().all(|key| match key {
            ArrayKey::Str(name) => signature.index_of(name.as_bytes()).is_some(),
            ArrayKey::Int(_) => false,
        });
    if all_named {
        return bind_arguments(signature.params, arguments);
    }

    let mut positional_count = 0;
    let mut named_required = 0;
    let mut saw_named = false;
    let mut misordered = false;
    let mut duplicate: Option<PhpString> = None;
    let mut has_unknown_named = false;
    for key in arguments.keys() {
        match key {
            ArrayKey::Int(_) => {
                misordered = misordered || saw_named;
                positional_count += 1;
            }
            ArrayKey::Str(name) => {
                saw_named = true;
                match signature.index_of(name.as_bytes()) {
                    None => has_unknown_named = true,
                    Some(i) => {
                        if duplicate.is_none() && i < positional_count {
                            duplicate = Some(name.clone());
                        }
                        if signature.is_required(name.as_bytes()) {
                            named_required += 1;
                        }
                    }
                }
            }
        }
    }
    if misordered {
        return Err(misordered_error(signature.name, line, source));
    }
    if let Some(duplicate) = duplicate {
        return Err(duplicate_error(&duplicate, signature.name, line, source));
    }

    // for a fully named call, the coverage of the required arguments is exact; a mixed call falls back to the
    // precise (and slower) per-argument check
    let required_names = signature
        .params
        .iter()
        .filter(|p| !p.explicit_default)
        .count();
    let may_miss_required = if positional_count == 0 {
        named_required < required_names
    } else {
        positional_count < signature.required_count
    };
    if may_miss_required
        || (!signature.variadic && (has_unknown_named || positional_count > declared))
    {
        trigger_legacy_deprecations(
            deprecations,
            signature,
            arguments,
            positional_count,
            source,
            line,
        );
    }
    bind_arguments(signature.params, arguments)
}

/// Twig: `TwigMacro::triggerLegacyDeprecations()`.
fn trigger_legacy_deprecations(
    deprecations: &mut dyn DeprecationSink,
    signature: &Signature<'_>,
    arguments: &Array,
    positional_count: usize,
    source: &Source,
    line: i64,
) {
    let macro_name = String::from_utf8_lossy(signature.name).into_owned();
    if positional_count < signature.required_count {
        for (i, param) in signature.params.iter().enumerate() {
            if param.explicit_default {
                continue;
            }
            if i < positional_count
                || arguments.contains_key(&ArrayKey::from_string(param.name.clone()))
            {
                continue;
            }
            deprecations.report(Deprecation::twig(
                "3.29",
                format!(
                    "Not passing a value for the \"{}\" argument of macro \"{macro_name}\" is deprecated and the argument will be required in Twig 4.0; give it a default value in the macro definition or pass a value when calling it (in \"{}\" at line {line}).",
                    param.name.to_string_lossy(),
                    source.name()
                ),
            ));
        }
    }
    if signature.variadic {
        return;
    }
    if positional_count > signature.params.len() {
        deprecations.report(Deprecation::twig(
            "3.29",
            format!(
                "Passing more arguments than the macro \"{macro_name}\" accepts is deprecated and will throw in Twig 4.0; declare a variadic argument (\"...name\") in the macro definition to accept extra arguments (in \"{}\" at line {line}).",
                source.name()
            ),
        ));
    }
    for key in arguments.keys() {
        if let ArrayKey::Str(name) = key
            && signature.index_of(name.as_bytes()).is_none()
        {
            deprecations.report(Deprecation::twig(
                "3.29",
                format!(
                    "Passing the unknown named argument \"{}\" to the macro \"{macro_name}\" is deprecated and will throw in Twig 4.0; declare a variadic argument (\"...name\") in the macro definition to accept it (in \"{}\" at line {line}).",
                    name.to_string_lossy(),
                    source.name()
                ),
            ));
        }
    }
}

/// The name of a macro parameter in the compiled closure: the reserved names get `TempNameExpression`'s prefix
/// (`TwigMacro::$renamedArguments`).
fn parameter_name(name: &[u8]) -> Vec<u8> {
    if RESERVED_NAMES.iter().any(|r| r.as_bytes() == name) {
        let mut prefixed = RESERVED_NAME_PREFIX.as_bytes().to_vec();
        prefixed.extend_from_slice(name);
        prefixed
    } else {
        name.to_vec()
    }
}

/// `($this->body)(...$arguments)`: PHP binds the spread arguments to the closure `function ($a = <default>, ...,
/// ...$bucket)` (`ZEND_SEND_UNPACK`, `zend_handle_named_arg()`): integer keys fill the parameters in order and then
/// the bucket, string keys bind by (renamed) parameter name, unknown names go to the bucket. Returns one value per
/// parameter (its default when no argument binds it) and the bucket.
fn bind_arguments(
    params: &[MacroParam],
    arguments: &Array,
) -> Result<(Vec<Value>, Array), Exception> {
    if arguments.keys().all(|key| matches!(key, ArrayKey::Int(_))) {
        // positional arguments only: they fill the parameters in order, the rest goes to the bucket
        let mut values = arguments.values();
        let bound = params
            .iter()
            .map(|param| {
                values
                    .next()
                    .cloned()
                    .unwrap_or_else(|| param.default.clone())
            })
            .collect();
        let mut bucket = Array::new();
        for value in values {
            bucket.push(value.clone())?;
        }
        return Ok((bound, bucket));
    }
    let mut values: Vec<Option<Value>> = vec![None; params.len()];
    let mut bucket = Array::new();
    let mut named = false;
    let mut position = 0;
    for (key, value) in arguments {
        match key {
            ArrayKey::Int(_) => {
                if named {
                    return Err(PhpError::error(
                        "Cannot use positional argument after named argument during unpacking",
                    )
                    .into());
                }
                match values.get_mut(position) {
                    Some(slot) => *slot = Some(value.clone()),
                    None => {
                        bucket.push(value.clone())?;
                    }
                }
                position += 1;
            }
            ArrayKey::Str(name) => {
                named = true;
                // `renameArguments()`: the Twig name of a reserved parameter becomes its compiled name
                let index = params.iter().position(|param| {
                    param.name.as_bytes() == name.as_bytes()
                        || parameter_name(param.name.as_bytes()) == name.as_bytes()
                });
                match index {
                    Some(i) => {
                        if values[i].is_some() {
                            return Err(overwrite_error(name.as_bytes()));
                        }
                        values[i] = Some(value.clone());
                    }
                    None => {
                        bucket.insert(ArrayKey::Str(name.clone()), value.clone());
                    }
                }
            }
        }
    }
    let bound = params
        .iter()
        .zip(values)
        .map(|(param, value)| value.unwrap_or_else(|| param.default.clone()))
        .collect();
    Ok((bound, bucket))
}

/// The body of a [`TwigMacro`] made of a Rust closure.
type MacroBody = dyn Fn(Vec<Value>, Array) -> Result<Value> + Send + Sync;

/// A macro: its name, its arguments and its body. Twig: `Twig\TwigMacro` (TWIG/src/TwigMacro.php), which compiled
/// templates create for each `{% macro %}` and [`MacroNamespace::call`] calls.
///
/// [`TwigMacro::new`] makes one of a Rust closure, with the signature a compiled macro has: the arguments in order,
/// each with the default value of its parameter (`None`: declared without a default, which binds `null`), and
/// whether the macro declares a variadic argument. [`call_legacy`](Self::call_legacy) handles the arguments as Twig
/// 3 does, leniently, with deprecations for what Twig 4.0 will reject, and calls the body with one value per
/// argument and the bucket of the extra ones (`...$varargs`, the implicit bucket of a macro that is not variadic).
///
/// ```
/// use twig::runtime::TwigMacro;
/// use twig::value::{Array, Value};
/// use twig::{Source, context};
///
/// // {% macro input(name, type = 'text') %}: the body returns its arguments as a list
/// let input = TwigMacro::new(
///     "input",
///     |arguments: Vec<Value>, _extra: Array| Ok(Value::Array(Array::from_values(arguments))),
///     [("name", None), ("type", Some(Value::from("text")))],
///     false,
/// );
/// let mut deprecations = Vec::new();
/// let source = Source::new("", "index.twig");
/// let output = input.call_legacy(context! { name => "login" }, &source, 7, &mut deprecations)?;
/// assert_eq!(output, Value::from(vec!["login", "text"]));
///
/// // `input()`: lenient, the missing argument binds `null`
/// input.call_legacy(Array::new(), &source, 7, &mut deprecations)?;
/// assert_eq!(
///     deprecations[0].to_string(),
///     "Since twig/twig 3.29: Not passing a value for the \"name\" argument of macro \"input\" is deprecated and the \
///      argument will be required in Twig 4.0; give it a default value in the macro definition or pass a value when \
///      calling it (in \"index.twig\" at line 7).",
/// );
/// # Ok::<(), twig::Error>(())
/// ```
#[derive(Clone)]
pub struct TwigMacro {
    name: PhpString,
    params: Vec<MacroParam>,
    variadic: bool,
    body: Arc<MacroBody>,
}

impl fmt::Debug for TwigMacro {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TwigMacro")
            .field("name", &self.name)
            .field("params", &self.params)
            .field("variadic", &self.variadic)
            .finish_non_exhaustive()
    }
}

impl TwigMacro {
    /// `new TwigMacro($name, $body, $arguments, $variadic)`: a macro whose body is `body`, with the `arguments`
    /// in order, each with the default of its parameter (`None` for an argument declared without a default,
    /// whose parameter defaults to `null`), and `variadic` for a macro declaring a variadic argument (`...name`).
    ///
    /// `body` gets one value per argument (the argument passed for it, else its default) and the bucket of the
    /// other arguments (positional ones under their position, unknown named ones under their name).
    pub fn new<N: Into<PhpString>>(
        name: impl Into<PhpString>,
        body: impl Fn(Vec<Value>, Array) -> Result<Value> + Send + Sync + 'static,
        arguments: impl IntoIterator<Item = (N, Option<Value>)>,
        variadic: bool,
    ) -> Self {
        let params = arguments
            .into_iter()
            .map(|(name, default)| MacroParam {
                name: name.into(),
                explicit_default: default.is_some(),
                default: default.unwrap_or_default(),
            })
            .collect();
        TwigMacro {
            name: name.into(),
            params,
            variadic,
            body: Arc::new(body),
        }
    }

    /// The macro name.
    pub fn name(&self) -> &PhpString {
        &self.name
    }

    /// Calls the macro with `arguments` (positional arguments under their position, named ones under their name)
    /// on behalf of the template `source` at line `line`, as Twig 3 does: leniently, with Twig's deprecations
    /// (reported to `deprecations`) for missing arguments without a default, extra arguments and unknown named
    /// arguments of a macro that is not variadic. Twig: `TwigMacro::callLegacy(array $arguments, Source $source, int
    /// $lineno)`.
    ///
    /// # Errors
    ///
    /// `Positional arguments cannot be used after named arguments for macro "%s".` and `Argument "%s" is defined twice
    /// for macro "%s".` (at `line` in `source`), the PHP `Error` of binding a named argument to a parameter already
    /// bound (`Named parameter $x overwrites previous argument`), and what the body returns.
    pub fn call_legacy(
        &self,
        arguments: Array,
        source: &Source,
        line: i64,
        deprecations: &mut dyn DeprecationSink,
    ) -> Result<Value> {
        let signature = Signature::new(self.name.as_bytes(), &self.params, self.variadic);
        let (values, bucket) = legacy_arguments(&signature, &arguments, source, line, deprecations)
            .map_err(Exception::into_error)?;
        (self.body)(values, bucket)
    }
}

/// Runs the body of a macro with the context of its parameters, to which the bucket and the globals are added.
fn run_macro(
    state: &mut VmState,
    template: &Arc<Template>,
    def: &MacroDef,
    mut context: Array,
    bucket: Array,
) -> Result<Value, Exception> {
    let bucket_name = match &def.variadic {
        Some(name) => PhpString::from(name.as_slice()),
        None => PhpString::clone(&VARARGS),
    };
    context.insert(ArrayKey::from_string(bucket_name), Value::Array(bucket));
    let context = twig_value::ops::array_union(&context, &state.env.globals());
    let program = Arc::clone(template.program());
    let mut frame = Frame::new(
        context,
        Arc::clone(template),
        template.macros(),
        Blocks::default(),
    );
    let mut out = Output::detached();
    run_unit(state, &mut out, &mut frame, &program, def.unit)
}
