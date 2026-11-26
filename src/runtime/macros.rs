//! Macros at runtime: `Twig\MacroNamespace` (the namespace an import binds, TWIG/src/MacroNamespace.php) and
//! `Twig\TwigMacro` (a macro of a template, TWIG/src/TwigMacro.php), with PHP's binding of the arguments to the
//! parameters of the compiled macro closure.

use std::sync::{Arc, LazyLock, Weak};

use twig_value::object::{ClassBuilder, ClassRef, Object, ObjectRef};
use twig_value::{Array, ArrayKey, PhpError, PhpString, Value};

use super::composition::get_parent;
use super::sandbox::ensure_security_checked;
use super::template::{Blocks, Template};
use crate::ast::{RESERVED_NAME_PREFIX, RESERVED_NAMES, VARARGS_NAME};
use crate::deprecation::Deprecation;
use crate::error::Error;
use crate::lltl::MacroDef;
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

/// `Twig\MacroNamespace`: the macros of a template, as `{% import %}`, `{% from %}` and `_self` bind them in
/// `$macros`. A namespace refers to its template weakly (templates import each other and themselves; the
/// environment keeps the templates alive), so `$this->macros` never forms a reference cycle.
pub(crate) struct MacroNamespace {
    template: Weak<Template>,
}

impl MacroNamespace {
    /// Twig: `Template::getMacroNamespace()`.
    pub(crate) fn value(template: &Arc<Template>) -> Value {
        Value::Object(ObjectRef::new(MacroNamespace {
            template: Arc::downgrade(template),
        }))
    }

    /// The template of a namespace value.
    fn template_of(value: &Value) -> Result<Arc<Template>, Exception> {
        let template = match value {
            Value::Object(object) => object
                .downcast_ref::<MacroNamespace>()
                .and_then(|namespace| namespace.template.upgrade()),
            _ => None,
        };
        template.ok_or_else(|| {
            Error::logic("The template of a macro namespace is no longer loaded.").into()
        })
    }
}

impl Object for MacroNamespace {
    fn class(&self) -> &ClassRef {
        &CLASS
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

/// The facts `TwigMacro`'s constructor derives from the signature: `$argumentIndexes`, `$requiredNames`,
/// `$requiredCount`.
struct Signature<'a> {
    def: &'a MacroDef,
    required_count: usize,
}

impl<'a> Signature<'a> {
    fn new(def: &'a MacroDef) -> Self {
        let required_count = def
            .params
            .iter()
            .rposition(|param| !param.explicit_default)
            .map_or(0, |i| i + 1);
        Signature {
            def,
            required_count,
        }
    }

    fn variadic(&self) -> bool {
        self.def.variadic.is_some()
    }

    fn index_of(&self, name: &[u8]) -> Option<usize> {
        self.def
            .params
            .iter()
            .position(|param| param.name.as_bytes() == name)
    }

    fn is_required(&self, name: &[u8]) -> bool {
        self.def
            .params
            .iter()
            .any(|param| param.name.as_bytes() == name && !param.explicit_default)
    }
}

/// Twig: `TwigMacro::callLegacy()`: the lenient Twig 3 argument handling, with deprecations for what Twig 4 rejects
/// and errors for what PHP would reject with its own wording; then the macro body is called with the arguments
/// spread (`($this->body)(...$arguments)`).
fn call_legacy(
    state: &mut VmState,
    (template, index): Found,
    arguments: Array,
    source: &Source,
    line: i64,
) -> Result<Value, Exception> {
    let program = Arc::clone(template.program());
    let def = &program.macros()[index];
    let signature = Signature::new(def);
    let declared = def.params.len();
    if is_list(&arguments) {
        let count = arguments.len();
        if count < signature.required_count || (!signature.variadic() && count > declared) {
            trigger_legacy_deprecations(state, &signature, &arguments, count, source, line);
        }
        return invoke(state, &template, def, &arguments);
    }

    // every declared argument is named: none can be missing, unknown, duplicated, or misordered
    let all_named = arguments.len() == declared
        && arguments.keys().all(|key| match key {
            ArrayKey::Str(name) => signature.index_of(name.as_bytes()).is_some(),
            ArrayKey::Int(_) => false,
        });
    if all_named {
        return invoke(state, &template, def, &arguments);
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
        return Err(misordered_error(&def.name, line, source));
    }
    if let Some(duplicate) = duplicate {
        return Err(duplicate_error(&duplicate, &def.name, line, source));
    }

    // for a fully named call, the coverage of the required arguments is exact; a mixed call falls back to the
    // precise (and slower) per-argument check
    let required_names = def.params.iter().filter(|p| !p.explicit_default).count();
    let may_miss_required = if positional_count == 0 {
        named_required < required_names
    } else {
        positional_count < signature.required_count
    };
    if may_miss_required
        || (!signature.variadic() && (has_unknown_named || positional_count > declared))
    {
        trigger_legacy_deprecations(
            state,
            &signature,
            &arguments,
            positional_count,
            source,
            line,
        );
    }
    invoke(state, &template, def, &arguments)
}

/// Twig: `TwigMacro::triggerLegacyDeprecations()`.
fn trigger_legacy_deprecations(
    state: &mut VmState,
    signature: &Signature<'_>,
    arguments: &Array,
    positional_count: usize,
    source: &Source,
    line: i64,
) {
    let def = signature.def;
    let macro_name = String::from_utf8_lossy(&def.name).into_owned();
    if positional_count < signature.required_count {
        for (i, param) in def.params.iter().enumerate() {
            if param.explicit_default {
                continue;
            }
            if i < positional_count
                || arguments.contains_key(&ArrayKey::from_string(param.name.clone()))
            {
                continue;
            }
            state.env.report_deprecation(Deprecation::twig(
                "3.29",
                format!(
                    "Not passing a value for the \"{}\" argument of macro \"{macro_name}\" is deprecated and the argument will be required in Twig 4.0; give it a default value in the macro definition or pass a value when calling it (in \"{}\" at line {line}).",
                    param.name.to_string_lossy(),
                    source.name()
                ),
            ));
        }
    }
    if signature.variadic() {
        return;
    }
    if positional_count > def.params.len() {
        state.env.report_deprecation(Deprecation::twig(
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
            state.env.report_deprecation(Deprecation::twig(
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
/// the bucket, string keys bind by (renamed) parameter name, unknown names go to the bucket; then the macro body runs
/// with the context `[<arguments>, <bucket> => $bucket] + $this->env->getGlobals()` and `$blocks = []`.
fn invoke(
    state: &mut VmState,
    template: &Arc<Template>,
    def: &MacroDef,
    arguments: &Array,
) -> Result<Value, Exception> {
    let mut values: Vec<Option<Value>> = vec![None; def.params.len()];
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
                let index = def.params.iter().position(|param| {
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
    let mut context = Array::new();
    for (param, value) in def.params.iter().zip(values) {
        context.insert(
            ArrayKey::from_string(param.name.clone()),
            value.unwrap_or_else(|| param.default.clone()),
        );
    }
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
