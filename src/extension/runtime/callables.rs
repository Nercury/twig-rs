//! PHP callables: the values Twig's methods call. `CoreExtension::map()`, `reduce()` and `find()` call them directly
//! (`$arrow($v, $k)`: `ZEND_INIT_DYNAMIC_CALL`); `sort()` and `filter()` hand them to internal functions whose
//! parameter is `callable` (`uasort()`, `array_filter()`, `CallbackFilterIterator::__construct()`: `Z_PARAM_FUNC`,
//! `zend_is_callable_ex()`), which check them first.
//!
//! Both resolve a callable like PHP: closures and invokable objects, `[$object, 'method']`, and `'Class::method'` and
//! `['Class', 'method']` with the class looked up in the class table (the environment's registry, then the built-in
//! classes: [`find_class`]) and `self`, `parent` and `static` resolved. They run in the class scope of the Twig
//! method, a static method of `Twig\Extension\CoreExtension` called statically: its private methods are accessible,
//! `self` and `static` are that class, `parent` is `Twig\Extension\AbstractExtension`, and there is no `$this`.
//!
//! twig-rs has no PHP code to run (reference/deviations.md, "String callables"): a function name is never found (no
//! function table), and a static method of a class (or its `__callStatic()`) that PHP would call fails with a
//! `LogicException` when it is called.

use std::sync::Arc;

use twig_value::object::{CallArgs, ClassRef, Closure, MethodInfo, ObjectRef, Visibility};
use twig_value::types::zend_zval_type_name;
use twig_value::{Array, Diagnostics, PhpError, PhpResult, PhpString, ThrowableClass, Value};

use super::misc::constants::{find_class, is_abstract_method, is_trait, twig_class};
use crate::runtime::Environment;
use crate::vm::{Exception, VmState};

/// `Twig\Extension\CoreExtension`: the class scope of the Twig methods that check and call callables.
const SCOPE: &str = "Twig\\Extension\\CoreExtension";
/// The parent of [`SCOPE`].
const SCOPE_PARENT: &str = "Twig\\Extension\\AbstractExtension";

/// What a callable resolves to (the part of PHP's `zend_fcall_info_cache` twig-rs needs to call it).
#[derive(Clone, Debug)]
pub(crate) enum Callee {
    /// A closure or an object with `__invoke()` (`get_closure`).
    Object(ObjectRef),
    /// A method of an object (`[$object, 'method']`), its `__call()` included; the name as given.
    Method(ObjectRef, String),
    /// A static method of a class, or its `__callStatic()` (class and method as PHP would name them): PHP runs its
    /// code, twig-rs cannot.
    Static(Arc<str>, Arc<str>),
}

/// Calls a resolved callable with positional arguments (`zend_call_function()`).
///
/// # Errors
///
/// What the callable throws; for [`Callee::Static`] the `LogicException` `Calling %s::%s() from a template is not
/// supported by twig-rs.`.
pub(crate) fn call_callee(
    callee: &Callee,
    args: Vec<Value>,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Value> {
    match callee {
        Callee::Object(object) => object.invoke(CallArgs::from(args), diag),
        Callee::Method(object, method) => object.call_method(method, CallArgs::from(args), diag),
        Callee::Static(class, method) => Err(PhpError::new(
            ThrowableClass::LogicException,
            format!("Calling {class}::{method}() from a template is not supported by twig-rs."),
        )),
    }
}

/// `$callable(...$args)` in the code of a Twig method (`CoreExtension::map()`, `reduce()`, `find()`): the callable
/// resolved by [`init_dynamic_call`], then called with the running VM as the diagnostics sink (template arrow
/// functions run on it).
///
/// # Errors
///
/// The `Error`s of [`init_dynamic_call`] and what the callable throws.
pub(crate) fn invoke(
    state: &mut VmState,
    callable: &Value,
    args: Vec<Value>,
) -> Result<Value, Exception> {
    // a closure or invokable object resolves to itself, without the environment
    if let Value::Object(object) = callable
        && object.is_callable()
    {
        return Ok(call_callee(&Callee::Object(object.clone()), args, state)?);
    }
    let env = state.env.clone();
    let callee = init_dynamic_call(&env, callable, state)?;
    Ok(call_callee(&callee, args, state)?)
}

/// The bytes of `s` up to its first NUL byte: what C's `%s` prints of a PHP string.
fn c_str(bytes: &[u8]) -> &[u8] {
    match memchr::memchr(0, bytes) {
        Some(nul) => &bytes[..nul],
        None => bytes,
    }
}

/// Splits `Class::method` at the last `:` that follows another `:` (`zend_memrchr(name, ':')`): the position of the
/// first `:` of the separator, when it is not at the start.
fn compound_name(name: &[u8]) -> Option<usize> {
    let colon = memchr::memrchr(b':', name)?;
    (colon > 0 && name[colon - 1] == b':').then(|| colon - 1)
}

/// `zend_check_method_accessible()` (PHPSRC/Zend/zend_object_handlers.h) from [`SCOPE`]: a private method only from
/// its declaring class, a protected one when `zend_check_protected()` relates its class and the scope through their
/// parent chains.
fn method_accessible(env: &Environment, method: &MethodInfo) -> bool {
    let declaring = &*method.declaring_class;
    match method.visibility {
        Visibility::Public => true,
        Visibility::Private => declaring == SCOPE,
        Visibility::Protected => {
            let mut class = find_class(env, declaring.as_bytes());
            while let Some(current) = class {
                if current.name() == SCOPE {
                    return true;
                }
                class = current.parent().cloned();
            }
            declaring == SCOPE || declaring == SCOPE_PARENT
        }
    }
}

/// The method `name` of `class` (case-insensitive, inherited ones included).
fn find_method(class: &ClassRef, name: &[u8]) -> Option<MethodInfo> {
    class.method(&String::from_utf8_lossy(name)).cloned()
}

/// Port of `zend_is_callable_check_class()` (PHPSRC/Zend/zend_API.c): the class a callable names, `self`, `parent` and
/// `static` resolved against `scope` (the calling frame's class, or the class of an array callable for its
/// `Class::method` method part) and the frame (`static` is the called class, [`SCOPE`]). The `E_DEPRECATED` `Use of
/// "self" in callables is deprecated` (and `parent`, `static`) unless `suppress`. Returns the class and whether it is
/// "strict" (anything but `self`), or the reason the callable is not valid.
fn check_class(
    env: &Environment,
    name: &[u8],
    scope: &ClassRef,
    suppress: bool,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Result<(ClassRef, bool), PhpString>> {
    let deprecated = |diag: &mut dyn Diagnostics, keyword: &str| -> PhpResult<()> {
        if suppress {
            return Ok(());
        }
        diag.deprecated(format!("Use of \"{keyword}\" in callables is deprecated"))
    };
    if name.eq_ignore_ascii_case(b"self") {
        deprecated(diag, "self")?;
        return Ok(Ok((scope.clone(), false)));
    }
    if name.eq_ignore_ascii_case(b"parent") {
        let Some(parent) = scope.parent().cloned() else {
            return Ok(Err(PhpString::from(
                "cannot access \"parent\" when current class scope has no parent",
            )));
        };
        deprecated(diag, "parent")?;
        return Ok(Ok((parent, true)));
    }
    if name.eq_ignore_ascii_case(b"static") {
        deprecated(diag, "static")?;
        return Ok(Ok((twig_class(SCOPE), true)));
    }
    match find_class(env, name) {
        Some(class) => Ok(Ok((class, true))),
        None => Ok(Err(PhpString::concat(&[
            b"class \"",
            c_str(name),
            b"\" not found",
        ]))),
    }
}

/// What [`check_func`] found.
enum Found {
    /// A method (`function_handler`), called through `call_via_handler` (a trampoline: `__call()`, `__callStatic()`,
    /// a closure's `__invoke()`) or not.
    Method { name: Arc<str>, via_handler: bool },
}

/// Port of `zend_is_callable_check_func()` (PHPSRC/Zend/zend_API.c) for the callable string `callable` after the
/// class part of an array callable resolved to `calling` (`ce_org`, with the object of an object callable), or on
/// its own (a function name or `Class::method`). Returns the callee, or the reason PHP gives.
fn check_func(
    env: &Environment,
    calling: Option<ClassRef>,
    object: Option<&ObjectRef>,
    callable: &PhpString,
    mut strict_class: bool,
    suppress: bool,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Result<Callee, PhpString>> {
    let ce_org = calling;
    let bytes = callable.as_bytes();
    // twig-rs has no function table: a function name is never found
    let (class, method) = if let Some(separator) = compound_name(bytes) {
        if separator == 0 {
            return Ok(Err(PhpString::from("invalid function name")));
        }
        let scope = ce_org.clone().unwrap_or_else(|| twig_class(SCOPE));
        let (class, strict) = match check_class(
            env,
            &bytes[..separator],
            &scope,
            suppress || ce_org.is_some(),
            diag,
        )? {
            Ok(found) => found,
            Err(reason) => return Ok(Err(reason)),
        };
        strict_class = strict;
        if let Some(org) = &ce_org {
            if !org.is_subclass_of_class(&class) {
                return Ok(Err(PhpString::from(format!(
                    "class {} is not a subclass of {}",
                    org.display_name(),
                    class.display_name()
                ))));
            }
            if !suppress {
                diag.deprecated(PhpString::concat(&[
                    b"Callables of the form [\"",
                    org.display_name().as_bytes(),
                    b"\", \"",
                    c_str(bytes),
                    b"\"] are deprecated",
                ]))?;
            }
        }
        (class, &bytes[separator + 2..])
    } else if let Some(org) = &ce_org {
        (org.clone(), bytes)
    } else {
        return Ok(Err(PhpString::concat(&[
            b"function \"",
            c_str(bytes),
            b"\" not found or invalid function name",
        ])));
    };

    let has = |name: &str| class.method(name).is_some();
    // the object's own class (`fcc->calling_scope == ce_org`), not a `Parent::method` form
    let own_class = object.is_some()
        && ce_org
            .as_ref()
            .is_some_and(|org| org.name().eq_ignore_ascii_case(class.name()));
    let trampoline = || Found::Method {
        name: Arc::from(String::from_utf8_lossy(method).as_ref()),
        via_handler: true,
    };
    let mut handler: Option<MethodInfo> = None;
    let mut found: Option<Found> = None;
    if strict_class && method.eq_ignore_ascii_case(b"__construct") {
        // the constructor, without a fallback
        handler = find_method(&class, b"__construct");
    } else {
        if let Some(method_info) = find_method(&class, method) {
            // a method that is not accessible goes to `__call()` / `__callStatic()` when the class has it
            let fallback = method_info.visibility != Visibility::Public
                && ((object.is_some() && has("__call"))
                    || (object.is_none() && has("__callStatic")))
                && !method_accessible(env, &method_info);
            if !fallback {
                handler = Some(method_info);
            }
        }
        if handler.is_none() {
            // get_function_via_handler
            if own_class {
                // `__call()` (`zend_std_get_method()`), or a closure's `__invoke()` (`zend_closure_get_method()`)
                let closure_invoke = object.is_some_and(|object| {
                    object.downcast_ref::<Closure>().is_some()
                        && method.eq_ignore_ascii_case(b"__invoke")
                });
                if has("__call") || closure_invoke {
                    found = Some(trampoline());
                }
            } else if has("__callStatic") {
                // `zend_std_get_static_method()`: the `__call()` fallback needs a `$this`, which a static Twig method
                // does not have
                found = Some(trampoline());
            }
        }
    }
    if let Some(method_info) = &handler {
        let name = &method_info.name;
        if is_abstract_method(env, &method_info.declaring_class, name) {
            return Ok(Err(PhpString::from(format!(
                "cannot call abstract method {}::{name}()",
                class.display_name()
            ))));
        }
        if object.is_none() && !method_info.is_static {
            return Ok(Err(PhpString::from(format!(
                "non-static method {}::{name}() cannot be called statically",
                class.display_name()
            ))));
        }
        if !method_accessible(env, method_info) {
            return Ok(Err(PhpString::from(format!(
                "cannot access {} method {}::{name}()",
                method_info.visibility.as_str(),
                class.display_name()
            ))));
        }
        found = Some(Found::Method {
            name: name.clone(),
            via_handler: false,
        });
    }
    let Some(Found::Method { name, via_handler }) = found else {
        return Ok(Err(PhpString::concat(&[
            b"class ",
            class.display_name().as_bytes(),
            b" does not have a method \"",
            c_str(method),
            b"\"",
        ])));
    };
    let method_name = String::from_utf8_lossy(method).into_owned();
    Ok(Ok(match object {
        // a method of the object's class, or the same method reached through `Parent::method`
        Some(object)
            if own_class
                || (!via_handler
                    && ce_org.as_ref().is_some_and(|org| {
                        find_method(org, method).map(|m| m.declaring_class)
                            == handler.as_ref().map(|m| m.declaring_class.clone())
                    })) =>
        {
            Callee::Method(object.clone(), method_name)
        }
        // a static method, `__callStatic()`, or another class's implementation of the object's method
        _ => Callee::Static(Arc::from(class.display_name()), name),
    }))
}

/// Port of `zend_is_callable_at_frame()` (PHPSRC/Zend/zend_API.c) at the frame of a Twig method ([`SCOPE`]): the
/// callee, or why the value is not callable, in PHP's words. `suppress` is `IS_CALLABLE_SUPPRESS_DEPRECATIONS`.
fn is_callable(
    env: &Environment,
    value: &Value,
    suppress: bool,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Result<Callee, PhpString>> {
    match value {
        Value::Object(object) if object.is_callable() => Ok(Ok(Callee::Object(object.clone()))),
        Value::String(name) => check_func(env, None, None, name, false, suppress, diag),
        Value::Array(array) => {
            if array.len() != 2 {
                return Ok(Err(PhpString::from(
                    "array callback must have exactly two members",
                )));
            }
            let (Some(target), Some(method)) = (array.get_int(0), array.get_int(1)) else {
                return Ok(Err(PhpString::from(
                    "array callback has to contain indices 0 and 1",
                )));
            };
            if !matches!(target, Value::String(_) | Value::Object(_)) {
                return Ok(Err(PhpString::from(
                    "first array member is not a valid class name or object",
                )));
            }
            let Value::String(method) = method else {
                return Ok(Err(PhpString::from(
                    "second array member is not a valid method",
                )));
            };
            match target {
                Value::String(class) => {
                    let (class, strict) = match check_class(
                        env,
                        class.as_bytes(),
                        &twig_class(SCOPE),
                        suppress,
                        diag,
                    )? {
                        Ok(found) => found,
                        Err(reason) => return Ok(Err(reason)),
                    };
                    check_func(env, Some(class), None, method, strict, suppress, diag)
                }
                Value::Object(object) => {
                    let class = object.class().clone();
                    check_func(
                        env,
                        Some(class),
                        Some(object),
                        method,
                        false,
                        suppress,
                        diag,
                    )
                }
                _ => unreachable!("checked above"),
            }
        }
        _ => Ok(Err(PhpString::from("no array or string given"))),
    }
}

/// The `Z_PARAM_FUNC` (`callable`) or `Z_PARAM_FUNC_OR_NULL` (`?callable`) check of an internal function's parameter
/// (`zend_is_callable_ex()`): the callee (`None` for an accepted `null`), with the deprecations of the check.
///
/// # Errors
///
/// `TypeError: <function>(): Argument #<n> ($<name>) must be a valid callback[ or null], <reason>`; a deprecation the
/// diagnostics sink turned into an exception (then PHP reports no `TypeError`).
pub(crate) fn check_callable(
    env: &Environment,
    value: &Value,
    function: &str,
    position: u32,
    name: &str,
    nullable: bool,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Option<Callee>> {
    if nullable && value.is_null() {
        return Ok(None);
    }
    match is_callable(env, value, false, diag)? {
        Ok(callee) => Ok(Some(callee)),
        Err(reason) => Err(PhpError::type_error(PhpString::concat(&[
            format!(
                "{function}(): Argument #{position} (${name}) must be a valid callback{}, ",
                if nullable { " or null" } else { "" }
            )
            .as_bytes(),
            &reason,
        ]))),
    }
}

/// Port of `zend_std_get_static_method()` (PHPSRC/Zend/zend_object_handlers.c) from [`SCOPE`] (`$this` is not set):
/// the method, or the `__callStatic()` trampoline; `Ok(None)` when there is neither.
///
/// # Errors
///
/// `Call to %s method %s::%s() from scope Twig\Extension\CoreExtension` for an inaccessible method without
/// `__callStatic()`, `Cannot call abstract method %s::%s()`, a deprecation turned into an exception (`Calling static
/// trait method %s::%s is deprecated, it should only be called on a class using the trait`).
fn get_static_method(
    env: &Environment,
    class: &ClassRef,
    method: &[u8],
    diag: &mut dyn Diagnostics,
) -> PhpResult<Option<(MethodInfo, bool)>> {
    let call_static = || {
        class.method("__callStatic").map(|m| {
            let mut trampoline = m.clone();
            trampoline.name = Arc::from(String::from_utf8_lossy(method).as_ref());
            trampoline.is_static = true;
            trampoline.declaring_class = Arc::from(class.name());
            (trampoline, true)
        })
    };
    let found = match find_method(class, method) {
        Some(method_info) if !method_accessible(env, &method_info) => match call_static() {
            Some(trampoline) => Some(trampoline),
            None => {
                return Err(PhpError::error(PhpString::concat(&[
                    b"Call to ",
                    method_info.visibility.as_str().as_bytes(),
                    b" method ",
                    method_info.declaring_class.as_bytes(),
                    b"::",
                    c_str(method),
                    format!("() from scope {SCOPE}").as_bytes(),
                ])));
            }
        },
        Some(method_info) => Some((method_info, false)),
        None => call_static(),
    };
    if let Some((method_info, via_handler)) = &found {
        if !via_handler && is_abstract_method(env, &method_info.declaring_class, &method_info.name)
        {
            return Err(PhpError::error(format!(
                "Cannot call abstract method {}::{}()",
                method_info.declaring_class, method_info.name
            )));
        }
        if is_trait(&method_info.declaring_class) {
            diag.deprecated(format!(
                "Calling static trait method {}::{} is deprecated, it should only be called on a class using the trait",
                method_info.declaring_class, method_info.name
            ))?;
        }
    }
    Ok(found)
}

/// The static method `method` of the class named `class_name`: `zend_fetch_class_by_name()`, then
/// [`get_static_method`], then the checks `zend_init_dynamic_call_string()` and `zend_init_dynamic_call_array()`
/// make (PHPSRC/Zend/zend_execute.c).
fn dynamic_static_call(
    env: &Environment,
    class_name: &[u8],
    method: &[u8],
    diag: &mut dyn Diagnostics,
) -> PhpResult<Callee> {
    let Some(class) = find_class(env, class_name) else {
        return Err(PhpError::error(PhpString::concat(&[
            b"Class \"",
            c_str(class_name),
            b"\" not found",
        ])));
    };
    let Some((method_info, _)) = get_static_method(env, &class, method, diag)? else {
        return Err(PhpError::error(PhpString::concat(&[
            b"Call to undefined method ",
            class.display_name().as_bytes(),
            b"::",
            c_str(method),
            b"()",
        ])));
    };
    if !method_info.is_static {
        return Err(PhpError::error(format!(
            "Non-static method {}::{}() cannot be called statically",
            method_info.declaring_class, method_info.name
        )));
    }
    Ok(Callee::Static(
        Arc::from(class.display_name()),
        method_info.name,
    ))
}

/// Port of `ZEND_INIT_DYNAMIC_CALL` (PHPSRC/Zend/zend_vm_def.h) in the code of a Twig method ([`SCOPE`]): what
/// `$callable(...)` calls. A string is a function name (twig-rs has none) or `Class::method`
/// (`zend_init_dynamic_call_string()`), an object must be callable (`zend_init_dynamic_call_object()`), an array is
/// `[$object, 'method']` or `['Class', 'method']` (`zend_init_dynamic_call_array()`); for an object's method,
/// existence and visibility are checked when it is called.
///
/// # Errors
///
/// PHP's `Error`s: `Call to undefined function %s()`, `Class "%s" not found`, `Call to undefined method %s::%s()`,
/// `Non-static method %s::%s() cannot be called statically`, `Object of type %s is not callable`, `Array callback
/// must have exactly two elements`, `Array callback has to contain indices 0 and 1`, `First array member is not a
/// valid class name or object`, `Second array member is not a valid method`, `Value of type %s is not callable`
/// (`zend_zval_type_name()`: `bool`, not `true`); those of [`get_static_method`].
pub(crate) fn init_dynamic_call(
    env: &Environment,
    callable: &Value,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Callee> {
    match callable {
        Value::String(name) => {
            let bytes = name.as_bytes();
            match memchr::memrchr(b':', bytes) {
                Some(colon) if colon > 0 && bytes[colon - 1] == b':' => {
                    dynamic_static_call(env, &bytes[..colon - 1], &bytes[colon + 1..], diag)
                }
                _ => Err(PhpError::error(PhpString::concat(&[
                    b"Call to undefined function ",
                    c_str(bytes),
                    b"()",
                ]))),
            }
        }
        Value::Object(object) => {
            if object.is_callable() {
                Ok(Callee::Object(object.clone()))
            } else {
                Err(PhpError::error(format!(
                    "Object of type {} is not callable",
                    object.class().display_name()
                )))
            }
        }
        Value::Array(array) => dynamic_array_call(env, array, diag),
        other => Err(PhpError::error(format!(
            "Value of type {} is not callable",
            zend_zval_type_name(other)
        ))),
    }
}

/// `zend_init_dynamic_call_array()` (PHPSRC/Zend/zend_execute.c).
fn dynamic_array_call(
    env: &Environment,
    array: &Array,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Callee> {
    if array.len() != 2 {
        return Err(PhpError::error(
            "Array callback must have exactly two elements",
        ));
    }
    let (Some(target), Some(method)) = (array.get_int(0), array.get_int(1)) else {
        return Err(PhpError::error(
            "Array callback has to contain indices 0 and 1",
        ));
    };
    if !matches!(target, Value::String(_) | Value::Object(_)) {
        return Err(PhpError::error(
            "First array member is not a valid class name or object",
        ));
    }
    let Value::String(method) = method else {
        return Err(PhpError::error("Second array member is not a valid method"));
    };
    match target {
        Value::String(class) => dynamic_static_call(env, class.as_bytes(), method.as_bytes(), diag),
        Value::Object(object) => {
            // `get_method` (`zend_std_get_method()`) from the scope of the Twig method; the call checks the rest
            let class = object.class();
            if let Some(method_info) = find_method(class, method.as_bytes())
                && !method_accessible(env, &method_info)
                && class.method("__call").is_none()
            {
                return Err(PhpError::error(PhpString::concat(&[
                    b"Call to ",
                    method_info.visibility.as_str().as_bytes(),
                    b" method ",
                    method_info.declaring_class.as_bytes(),
                    b"::",
                    c_str(method.as_bytes()),
                    format!("() from scope {SCOPE}").as_bytes(),
                ])));
            }
            Ok(Callee::Method(
                object.clone(),
                method.to_string_lossy().into_owned(),
            ))
        }
        _ => unreachable!("checked above"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loader::ArrayLoader;

    fn reason(env: &Environment, value: Value) -> Option<String> {
        let mut diag: Vec<twig_value::Diagnostic> = Vec::new();
        match is_callable(env, &value, true, &mut diag).unwrap() {
            Ok(_) => None,
            Err(reason) => Some(reason.to_string_lossy().into_owned()),
        }
    }

    #[test]
    fn reasons_follow_zend_is_callable() {
        let env = Environment::new(ArrayLoader::new(Vec::<(String, String)>::new()));
        let env = &env;
        assert_eq!(
            reason(env, Value::from("nope")).as_deref(),
            Some("function \"nope\" not found or invalid function name")
        );
        assert_eq!(
            reason(env, Value::from("A::b")).as_deref(),
            Some("class \"A\" not found")
        );
        assert_eq!(
            reason(env, Value::from("::b")).as_deref(),
            Some("invalid function name")
        );
        assert_eq!(
            reason(env, Value::from("stdClass::x")).as_deref(),
            Some("class stdClass does not have a method \"x\"")
        );
        assert_eq!(
            reason(env, Value::Null).as_deref(),
            Some("no array or string given")
        );
        assert_eq!(
            reason(env, Value::Array(Array::from_values([Value::Int(1)]))).as_deref(),
            Some("array callback must have exactly two members")
        );
        assert_eq!(
            reason(
                env,
                Value::Array(Array::from_values([Value::Int(1), Value::from("m")]))
            )
            .as_deref(),
            Some("first array member is not a valid class name or object")
        );
        assert_eq!(
            reason(
                env,
                Value::Array(Array::from_values([
                    Value::from("arrayobject"),
                    Value::from("COUNT")
                ]))
            )
            .as_deref(),
            Some("non-static method ArrayObject::count() cannot be called statically")
        );
        assert_eq!(
            reason(
                env,
                Value::Array(Array::from_values([
                    Value::from("Countable"),
                    Value::from("count")
                ]))
            )
            .as_deref(),
            Some("cannot call abstract method Countable::count()")
        );
        let closure = Value::Object(Closure::new(|_, _| Ok(Value::Null)).into_ref());
        assert_eq!(reason(env, closure.clone()), None);
        assert_eq!(
            reason(
                env,
                Value::Array(Array::from_values([closure, Value::from("__invoke")]))
            ),
            None
        );
        let mut diag: Vec<twig_value::Diagnostic> = Vec::new();
        let error = check_callable(
            env,
            &Value::Int(1),
            "uasort",
            2,
            "callback",
            false,
            &mut diag,
        )
        .unwrap_err();
        assert_eq!(
            error.message().to_string_lossy(),
            "uasort(): Argument #2 ($callback) must be a valid callback, no array or string given"
        );
        assert!(
            check_callable(
                env,
                &Value::Null,
                "array_filter",
                2,
                "callback",
                true,
                &mut diag
            )
            .unwrap()
            .is_none()
        );
    }

    #[test]
    fn dynamic_calls_follow_zend_init_dynamic_call() {
        let env = Environment::new(ArrayLoader::new(Vec::<(String, String)>::new()));
        let message = |value: Value| {
            let mut diag: Vec<twig_value::Diagnostic> = Vec::new();
            init_dynamic_call(&env, &value, &mut diag)
                .map(|_| String::new())
                .unwrap_or_else(|error| error.message().to_string_lossy().into_owned())
        };
        assert_eq!(
            message(Value::Bool(true)),
            "Value of type bool is not callable"
        );
        assert_eq!(
            message(Value::from("nope")),
            "Call to undefined function nope()"
        );
        assert_eq!(
            message(Value::Array(Array::from_values([
                Value::from("a"),
                Value::from("b")
            ]))),
            "Class \"a\" not found"
        );
        assert_eq!(
            message(Value::from("stdClass::x")),
            "Call to undefined method stdClass::x()"
        );
        assert_eq!(
            message(Value::from("Twig\\Markup::count")),
            "Non-static method Twig\\Markup::count() cannot be called statically"
        );
    }
}
