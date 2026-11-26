//! Attribute access: `CoreExtension::getAttribute()`, what `a.b`, `a.b()`, `a[b]` (strict mode) and `attribute()`
//! compile to.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

use twig_value::object::{CallArgs, ClassRef, FetchMode, ObjectRef, PropertyCheck, Visibility};
use twig_value::param::{DeclaredType, TypeMask, UserArg};
use twig_value::types::get_debug_type;
use twig_value::{ArrayKey, Diagnostics, KeyRef, ThrowableClass, Value, convert};

use super::sandbox::Violation;
use crate::ast::CallType;
use crate::error::Error;
use crate::lltl::{GetAttrOp, Program};
use crate::vm::{Exception, VmState};

/// Twig: `CoreExtension::ARRAY_LIKE_CLASSES`: classes whose `ArrayAccess` implementation the sandbox trusts.
pub const ARRAY_LIKE_CLASSES: [&str; 11] = [
    "ArrayIterator",
    "ArrayObject",
    "CachingIterator",
    "RecursiveArrayIterator",
    "RecursiveCachingIterator",
    "SplDoublyLinkedList",
    "SplFixedArray",
    "SplObjectStorage",
    "SplQueue",
    "SplStack",
    "WeakMap",
];

/// `in_array($object::class, CoreExtension::ARRAY_LIKE_CLASSES, true)` (exact class name).
pub(crate) fn is_array_like_class(object: &ObjectRef) -> bool {
    ARRAY_LIKE_CLASSES.contains(&object.class_name())
}

/// The method map of a class (`$cache[$class]` in `getAttribute()`): exact names, lowercase names and the
/// `get`/`is`/`has` aliases.
type MethodMap = HashMap<Vec<u8>, Arc<str>>;

/// Method maps by class entry address (the class is kept so the address stays unique).
type MethodMaps = HashMap<usize, (ClassRef, Arc<MethodMap>)>;

/// The process-wide cache of method maps (Twig's `static $cache`), keyed by class entry.
static METHOD_MAPS: LazyLock<Mutex<MethodMaps>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// The method map part of `getAttribute()`: `get_class_methods()` (plus `__invoke` for closures), sorted; each method
/// under its name and its lowercase name; `getFooBar()`/`isFooBar()`/`hasFooBar()` also under `fooBar` and `foobar`
/// unless taken (first in sorted order wins, `has*` is skipped when the `is*` method exists).
fn method_map(object: &ObjectRef) -> Arc<MethodMap> {
    let class = object.class();
    let key = Arc::as_ptr(class) as usize;
    {
        let maps = METHOD_MAPS.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((_, map)) = maps.get(&key) {
            return Arc::clone(map);
        }
    }
    let mut methods: Vec<Arc<str>> = class.public_method_names();
    if object
        .downcast_ref::<twig_value::object::Closure>()
        .is_some()
    {
        methods.push(Arc::from("__invoke"));
    }
    // sort(): method names are not numeric, so SORT_REGULAR compares them as byte strings
    methods.sort_by(|a, b| twig_value::compare::smart_strcmp(a.as_bytes(), b.as_bytes()).cmp(&0));
    let lc_methods: Vec<Vec<u8>> = methods
        .iter()
        .map(|m| m.as_bytes().to_ascii_lowercase())
        .collect();
    let mut map: MethodMap = HashMap::new();
    for (i, method) in methods.iter().enumerate() {
        map.insert(method.as_bytes().to_vec(), Arc::clone(method));
        let lc_name = &lc_methods[i];
        map.insert(lc_name.clone(), Arc::clone(method));
        let prefix_length = if lc_name.starts_with(b"get") {
            3
        } else if lc_name.starts_with(b"is") {
            2
        } else if lc_name.starts_with(b"has") {
            let mut is_name = b"is".to_vec();
            is_name.extend_from_slice(&lc_name[3..]);
            if lc_methods.contains(&is_name) {
                continue;
            }
            3
        } else {
            continue;
        };
        let stripped = &lc_name[prefix_length..];
        // `if ($lcName)`: PHP truthiness, "0" is skipped too
        if stripped.is_empty() || stripped == b"0" {
            continue;
        }
        // $lcName[0].substr($method, $prefixLength + 1)
        let mut camel = vec![stripped[0]];
        camel.extend_from_slice(
            method
                .as_bytes()
                .get(prefix_length + 1..)
                .unwrap_or_default(),
        );
        map.entry(camel).or_insert_with(|| Arc::clone(method));
        map.entry(stripped.to_vec())
            .or_insert_with(|| Arc::clone(method));
    }
    let map = Arc::new(map);
    METHOD_MAPS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(key, (class.clone(), Arc::clone(&map)));
    map
}

/// What `getPropertyChecker()` returns for a class and a property.
enum PropertyChecker {
    /// `false`: the property cannot be read (declared but inaccessible, without `__isset`).
    Never,
    /// `true`: only `isset()` decides (inaccessible but the class has `__isset`).
    IssetOnly,
    /// `property_exists()` (the class declares no such property: dynamic properties).
    PropertyExists,
    /// `ReflectionProperty::isInitialized()` (a declared public property).
    IsInitialized,
}

/// Twig: `CoreExtension::getPropertyChecker()`.
fn property_checker(class: &ClassRef, property: &[u8]) -> PropertyChecker {
    match class.visible_property(property) {
        None => PropertyChecker::PropertyExists,
        Some(info) if info.visibility != Visibility::Public || info.is_static => {
            if class.has_method("__isset") {
                PropertyChecker::IssetOnly
            } else {
                PropertyChecker::Never
            }
        }
        Some(_) => PropertyChecker::IsInitialized,
    }
}

/// `sprintf('%s', $value)`: PHP string conversion (arrays print `Array` with a warning).
fn display(value: &Value, diag: &mut dyn Diagnostics) -> Result<String, Exception> {
    Ok(convert::to_string(value, diag)?
        .to_string_lossy()
        .into_owned())
}

/// Twig: `CoreExtension::getAttribute()` (TWIG/src/Extension/CoreExtension.php): array keys, `ArrayAccess`,
/// properties, `DateTime` pseudo-properties, class constants, then methods (exact, lowercase, `get`/`is`/`has`
/// aliases, `__call`), with the strict-mode messages.
///
/// With `$sandboxed` (the `SandboxExtension` was registered when the template was compiled), a `Stringable` key must
/// be allowed to call `__toString()`, an `ArrayAccess` offset (except on the array-like built-in classes) and a
/// property must be allowed by the policy (else the lookup goes on with the methods, and the property violation is
/// what fails when no method matches), and so must the method.
///
/// `arguments` is what the compiled call passes as `array $arguments` (`[]` without an arguments node): the `RECV` of
/// that typed parameter rejects anything but an array before the lookup starts (`attribute(x, 'a', null)`).
///
/// # Errors
///
/// The `TypeError` of `$arguments` (`Twig\Extension\CoreExtension::getAttribute(): Argument #5 ($arguments) must be
/// of type array, null given`; PHP's `, called in <file> on line N` suffix is left out, reference/deviations.md), the
/// `RuntimeError`s of strict mode (with the access line and the template source) and whatever a called method or
/// property handler throws.
pub(crate) fn get_attribute(
    state: &mut VmState,
    program: &Program,
    object: Value,
    item: Value,
    arguments: Value,
    op: &GetAttrOp,
) -> Result<Value, Exception> {
    // An existing element of an array under a string or int item: the key the array branch builds (`(string)` of
    // the item, then the array offset) is the item's own symtable key, and nothing before the lookup has an effect
    // (only objects meet the sandbox checks).
    if op.call_type != CallType::Method
        && matches!(arguments, Value::Array(_))
        && let Value::Array(array) = &object
    {
        let key = match &item {
            Value::String(name) => Some(KeyRef::sym(name)),
            Value::Int(i) => Some(KeyRef::Int(*i)),
            _ => None,
        };
        if let Some(value) = key.and_then(|key| array.get_ref(key)) {
            return Ok(if op.defined_test {
                Value::Bool(true)
            } else {
                value.clone()
            });
        }
    }
    // `array $arguments = []` (TWIG/src/Extension/CoreExtension.php): no weak coercion reaches an array, so
    // zend_verify_arg_error() names the value (`zend_zval_value_name()`: `true`/`false` for booleans)
    let arguments = match arguments {
        Value::Array(arguments) => arguments,
        other => {
            return Err(UserArg::new(
                "Twig\\Extension\\CoreExtension::getAttribute",
                5,
                "arguments",
            )
            .type_error(&DeclaredType::builtin(TypeMask::ARRAY), &other)
            .into());
        }
    };
    let strict = state.env.is_strict_variables();
    let lenient = op.ignore_strict_check || !strict;
    let source = program.source();
    let error = |message: String| -> Exception {
        Error::runtime(message)
            .with_line(op.line)
            .with_source(source.clone())
            .into()
    };
    let checker = if op.sandboxed {
        state.env.sandbox_checker()
    } else {
        None
    };
    let mut property_not_allowed: Option<Exception> = None;
    if let Some(checker) = &checker
        && let Value::Object(key) = &item
        && key.instance_of("Stringable")
    {
        checker.ensure_to_string_allowed(state, item.clone(), op.line, source)?;
    }
    // `goto methodCheck`, with the item as a string
    let mut method_check: Option<twig_value::PhpString> = None;

    // array
    if op.call_type != CallType::Method {
        let mut array_item = match &item {
            Value::Bool(b) => Value::Int(i64::from(*b)),
            Value::Float(_) => Value::Int(convert::to_long(&item, state)?),
            other => other.clone(),
        };
        if let (Value::Object(key), Value::Object(container)) = (&array_item, &object)
            && key.instance_of("Stringable")
            && (container.instance_of("ArrayObject") || container.instance_of("ArrayIterator"))
        {
            array_item = Value::String(convert::to_string(&array_item, state)?);
        }

        if let Some(checker) = &checker
            && let Value::Object(container) = &object
            && container.instance_of("ArrayAccess")
            && !is_array_like_class(container)
        {
            match checker.check_property_allowed(container, &array_item, op.line, source, state) {
                Ok(()) => {}
                Err(Violation::NotAllowed(violation)) => {
                    property_not_allowed = Some(violation);
                    method_check = Some(convert::to_string(&item, state)?);
                }
                Err(Violation::Other(other)) => return Err(other),
            }
        }

        if method_check.is_none() {
            let exists = match &object {
                Value::Array(array) => {
                    let key_string = convert::to_string(&array_item, state)?;
                    array_item = Value::String(key_string.clone());
                    array.contains_key(&ArrayKey::from_string(key_string))
                }
                Value::Object(o) if o.instance_of("ArrayAccess") => {
                    let result = o.call_method(
                        "offsetExists",
                        CallArgs::from(vec![array_item.clone()]),
                        state,
                    )?;
                    convert::to_bool(&result, state)?
                }
                _ => false,
            };
            if exists {
                if op.defined_test {
                    return Ok(Value::Bool(true));
                }
                return Ok(match &object {
                    Value::Array(array) => {
                        let key =
                            ArrayKey::from_value(&array_item, twig_value::KeyContext::Read, state)?;
                        array.get(&key).cloned().unwrap_or_default()
                    }
                    Value::Object(o) => o.read_dimension(&array_item, FetchMode::Read, state)?,
                    _ => Value::Null,
                });
            }

            if op.call_type == CallType::Array || !matches!(object, Value::Object(_)) {
                if op.defined_test {
                    return Ok(Value::Bool(false));
                }
                if lenient {
                    return Ok(Value::Null);
                }
                let message = match &object {
                    Value::Object(o) if o.instance_of("ArrayAccess") => {
                        if matches!(array_item, Value::Object(_) | Value::Array(_)) {
                            format!(
                                "Key of type \"{}\" does not exist in ArrayAccess-able object of class \"{}\".",
                                get_debug_type(&array_item),
                                get_debug_type(&object)
                            )
                        } else {
                            format!(
                                "Key \"{}\" does not exist in ArrayAccess-able object of class \"{}\".",
                                display(&array_item, state)?,
                                get_debug_type(&object)
                            )
                        }
                    }
                    Value::Object(_) => format!(
                        "Impossible to access a key \"{}\" on an object of class \"{}\" that does not implement ArrayAccess interface.",
                        display(&item, state)?,
                        get_debug_type(&object)
                    ),
                    Value::Array(array) => {
                        if array.is_empty() {
                            format!(
                                "Key \"{}\" does not exist as the sequence/mapping is empty.",
                                display(&array_item, state)?
                            )
                        } else {
                            let keys: Vec<String> = array
                                .keys()
                                .map(|k| k.to_php_string().to_string_lossy().into_owned())
                                .collect();
                            format!(
                                "Key \"{}\" for sequence/mapping with keys \"{}\" does not exist.",
                                display(&array_item, state)?,
                                keys.join(", ")
                            )
                        }
                    }
                    Value::Null if op.call_type == CallType::Array => format!(
                        "Impossible to access a key (\"{}\") on a null variable.",
                        display(&item, state)?
                    ),
                    _ if op.call_type == CallType::Array => format!(
                        "Impossible to access a key (\"{}\") on a {} variable (\"{}\").",
                        display(&item, state)?,
                        get_debug_type(&object),
                        display(&object, state)?
                    ),
                    Value::Null => format!(
                        "Impossible to access an attribute (\"{}\") on a null variable.",
                        display(&item, state)?
                    ),
                    _ => format!(
                        "Impossible to access an attribute (\"{}\") on a {} variable (\"{}\").",
                        display(&item, state)?,
                        get_debug_type(&object),
                        display(&object, state)?
                    ),
                };
                return Err(error(message));
            }
        }
    }

    let (item, object_ref) = match method_check.take() {
        Some(item) => match &object {
            Value::Object(object_ref) => (item, object_ref),
            _ => unreachable!("only an ArrayAccess object skips to the methods"),
        },
        None => {
            // $item = (string) $item;
            let item = convert::to_string(&item, state)?;

            let Value::Object(object_ref) = &object else {
                if op.defined_test {
                    return Ok(Value::Bool(false));
                }
                if lenient {
                    return Ok(Value::Null);
                }
                let item_text = item.to_string_lossy();
                let message = match &object {
                    Value::Null => {
                        format!(
                            "Impossible to invoke a method (\"{item_text}\") on a null variable."
                        )
                    }
                    Value::Array(_) => {
                        format!(
                            "Impossible to invoke a method (\"{item_text}\") on a sequence/mapping."
                        )
                    }
                    _ => format!(
                        "Impossible to invoke a method (\"{item_text}\") on a {} variable (\"{}\").",
                        get_debug_type(&object),
                        display(&object, state)?
                    ),
                };
                return Err(error(message));
            };
            (item, object_ref)
        }
    };

    // object property
    let mut properties = op.call_type != CallType::Method && property_not_allowed.is_none();
    if properties && let Some(checker) = &checker {
        match checker.check_property_allowed(
            object_ref,
            &Value::String(item.clone()),
            op.line,
            source,
            state,
        ) {
            Ok(()) => {}
            Err(Violation::NotAllowed(violation)) => {
                property_not_allowed = Some(violation);
                properties = false;
            }
            Err(Violation::Other(other)) => return Err(other),
        }
    }
    if properties {
        let class = object_ref.class().clone();
        let checker = property_checker(&class, &item);
        if !matches!(checker, PropertyChecker::Never) {
            let mut readable = object_ref.has_property(&item, PropertyCheck::Isset, state)?;
            if !readable {
                readable = match checker {
                    PropertyChecker::PropertyExists => object_ref.property_exists(&item, state)?,
                    PropertyChecker::IsInitialized => {
                        object_ref.has_property(&item, PropertyCheck::Exists, state)?
                    }
                    PropertyChecker::Never | PropertyChecker::IssetOnly => false,
                };
            }
            if readable {
                if op.defined_test {
                    return Ok(Value::Bool(true));
                }
                return Ok(object_ref.read_property(&item, FetchMode::Read, state)?);
            }
        }

        if object_ref.instance_of("DateTimeInterface")
            && matches!(item.as_bytes(), b"date" | b"timezone" | b"timezone_type")
        {
            if op.defined_test {
                return Ok(Value::Bool(true));
            }
            let properties = convert::to_array(&object, state)?;
            return Ok(match properties.get_str(&item) {
                Some(value) => value.clone(),
                None => {
                    twig_value::dim::undefined_key(&ArrayKey::Str(item.clone()), state)?;
                    Value::Null
                }
            });
        }

        if let Some(value) = class.constant(&item.to_string_lossy()) {
            if op.defined_test {
                return Ok(Value::Bool(true));
            }
            return Ok(value);
        }
    }

    // methodCheck:
    let map = method_map(object_ref);
    let mut call = false;
    let method: Arc<str> = if let Some(method) = map.get(item.as_bytes()) {
        Arc::clone(method)
    } else if let Some(method) = map.get(&item.as_bytes().to_ascii_lowercase()) {
        Arc::clone(method)
    } else if map.contains_key(b"__call".as_slice()) {
        call = true;
        Arc::from(item.to_string_lossy().as_ref())
    } else {
        if op.defined_test {
            return Ok(Value::Bool(false));
        }
        if let Some(violation) = property_not_allowed {
            return Err(violation);
        }
        if lenient {
            return Ok(Value::Null);
        }
        let item_text = item.to_string_lossy();
        return Err(error(format!(
            "Neither the property \"{item_text}\" nor one of the methods \"{item_text}()\", \"get{item_text}()\", \"is{item_text}()\", \"has{item_text}()\" or \"__call()\" exist and have public access in class \"{}\".",
            object_ref.class_name()
        )));
    };

    if let Some(checker) = &checker {
        match checker.check_method_allowed(object_ref, method.as_bytes(), op.line, source) {
            Ok(()) => {}
            Err(Violation::NotAllowed(violation)) => {
                if op.defined_test {
                    return Ok(Value::Bool(false));
                }
                return Err(property_not_allowed.unwrap_or(violation));
            }
            Err(Violation::Other(other)) => return Err(other),
        }
    }

    if op.defined_test {
        return Ok(Value::Bool(true));
    }

    // `$object->$method(...$arguments)`: ZEND_INIT_METHOD_CALL finds the function, then ZEND_SEND_UNPACK sends the
    // elements, a named one checked against the function's parameters as it is sent (zend_handle_named_arg(),
    // PHPSRC/Zend/zend_execute.c); positional elements alone need no parameters, nor do unknown ones
    let named = arguments.keys().any(|key| matches!(key, ArrayKey::Str(_)));
    let args = match named.then(|| object_ref.method_params(&method)).flatten() {
        Some(params) => params.unpack(&Value::Array(arguments), state)?,
        None => CallArgs::from_spread(&arguments)?,
    };
    match object_ref.call_method(&method, args, state) {
        Ok(value) => Ok(value),
        Err(e) => {
            if e.is_a(&ThrowableClass::BadMethodCallException) && call && lenient {
                return Ok(Value::Null);
            }
            Err(e.into())
        }
    }
}

/// The method a method map key resolves to, for tests.
#[cfg(test)]
fn method_for(object: &ObjectRef, item: &str) -> Option<String> {
    let map = method_map(object);
    map.get(item.as_bytes())
        .or_else(|| map.get(&item.as_bytes().to_ascii_lowercase()))
        .map(|m| m.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use twig_value::object::{ClassBuilder, Object, PropertyStore};

    struct Thing {
        class: ClassRef,
    }

    impl Object for Thing {
        fn class(&self) -> &ClassRef {
            &self.class
        }

        fn property_store(&self) -> Option<&PropertyStore> {
            None
        }
    }

    #[test]
    fn builds_the_method_map_like_twig() {
        let class = ClassBuilder::class("Thing")
            .method("getFooBar")
            .method("isFooBar")
            .method("hasBaz")
            .method("isBaz")
            .method("hasQux")
            .method("get")
            .build();
        let object = ObjectRef::new(Thing { class });
        assert_eq!(method_for(&object, "fooBar").as_deref(), Some("getFooBar"));
        assert_eq!(method_for(&object, "FOOBAR").as_deref(), Some("getFooBar"));
        assert_eq!(method_for(&object, "baz").as_deref(), Some("isBaz"));
        assert_eq!(method_for(&object, "qux").as_deref(), Some("hasQux"));
        assert_eq!(method_for(&object, "get").as_deref(), Some("get"));
        assert_eq!(method_for(&object, "foo_bar"), None);
    }
}
