//! Rust versions of the PHP helper classes of the ported tests (TWIG/tests/TemplateTest.php, ErrorTest.php). Each
//! class declares its members in PHP's order with PHP's visibility and ports every method body; names are the PHP
//! class names, so error messages match.

use std::sync::LazyLock;

use twig::value::object::{
    ArrayIterator, ArrayObject, CallArgs, ClassBuilder, ClassRef, Closure, FetchMode, FunctionKind,
    Object, ObjectIterator, ObjectRef, Param, PropertyInfo, PropertyStore, StdClass, Visibility,
    builtin,
};
use twig::value::{Array, Diagnostics, PhpError, PhpResult, PhpString, ThrowableClass, Value, dim};

fn property(name: &str, visibility: Visibility, default: Option<Value>) -> PropertyInfo {
    PropertyInfo {
        name: PhpString::from(name),
        visibility,
        is_static: false,
        is_readonly: false,
        type_name: None,
        default,
        declaring_class: "".into(),
    }
}

fn typed_property(name: &str, type_name: &str, default: Option<Value>) -> PropertyInfo {
    PropertyInfo {
        type_name: Some(type_name.into()),
        ..property(name, Visibility::Public, default)
    }
}

/// `\array_key_exists($key, $array)` in a userland method: the fully qualified call compiles to PHP's
/// `ZEND_ARRAY_KEY_EXISTS` opcode, whose invalid-key error is `zend_illegal_array_offset_access()` (`Cannot access
/// offset of type X on array`) instead of the `array_key_exists()` function's `Argument #1 ($key) must be a valid
/// array offset type` (PHPSRC/Zend/zend_execute.c, `zend_array_key_exists_fast()`).
fn array_key_exists(key: &Value, array: &Value, diag: &mut dyn Diagnostics) -> PhpResult<bool> {
    let type_name = match key {
        Value::Object(object) => object.class_name().to_owned(),
        Value::Array(_) => "array".to_owned(),
        _ => return twig::value::builtins::array::array_key_exists(key, array, diag),
    };
    Err(PhpError::type_error(format!(
        "Cannot access offset of type {type_name} on array"
    )))
}

fn arg(args: &CallArgs, i: usize) -> Value {
    args.get(i).cloned().unwrap_or_default()
}

/// The `$array` of `TemplateTest::getGetAttributeTests()` (also the `$attributes` of the ArrayAccess and magic
/// objects, without `defined` for the latter).
pub fn attribute_array(with_defined: bool) -> Array {
    let mut array = Array::new();
    if with_defined {
        array.insert("defined", "defined");
    }
    array.insert("zero", 0);
    array.insert("null", Value::Null);
    array.insert("1", 1);
    array.insert("bar", true);
    array.insert("foo", true);
    array.insert("baz", "baz");
    array.insert("baf", "baf");
    array.insert("09", "09");
    array.insert("+4", "+4");
    array
}

/// A userland object: its class, its property table and the bodies of its methods.
struct UserObject {
    class: ClassRef,
    properties: PropertyStore,
    methods: fn(&UserObject, &ObjectRef, &str, CallArgs, &mut dyn Diagnostics) -> PhpResult<Value>,
}

impl UserObject {
    fn property(&self, name: &str) -> Value {
        self.properties.get(name.as_bytes()).unwrap_or_default()
    }
}

impl Object for UserObject {
    fn class(&self) -> &ClassRef {
        &self.class
    }

    fn property_store(&self) -> Option<&PropertyStore> {
        Some(&self.properties)
    }

    /// Binds the arguments like PHP binds them to a userland method (`Too few arguments to function ...` for a missing
    /// required parameter, extra arguments ignored), then runs the method body.
    fn call(
        &self,
        this: &ObjectRef,
        method: &str,
        args: CallArgs,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        // the parameters every helper class declares for these methods (`offsetGet($name)`, `__call($method,
        // $arguments)`, ...); the other methods have none
        let parameters: &[&'static str] = match method {
            "offsetexists" | "offsetget" | "offsetunset" | "__isset" | "__get" => &["offset"],
            "offsetset" => &["offset", "value"],
            "__call" => &["method", "arguments"],
            _ => &[],
        };
        let args = if parameters.is_empty() {
            args
        } else {
            let declared = self
                .class
                .method(method)
                .map_or_else(|| method.to_owned(), |m| m.name.to_string());
            let params: Vec<Param> = parameters.iter().copied().map(Param::required).collect();
            CallArgs::from(args.bind(
                &format!("{}::{declared}", self.class.name()),
                &params,
                FunctionKind::User,
            )?)
        };
        (self.methods)(self, this, method, args, diag)
    }
}

fn new_user(
    class: &ClassRef,
    methods: fn(&UserObject, &ObjectRef, &str, CallArgs, &mut dyn Diagnostics) -> PhpResult<Value>,
) -> ObjectRef {
    ObjectRef::new(UserObject {
        class: class.clone(),
        properties: PropertyStore::new(class),
        methods,
    })
}

fn undefined(this: &ObjectRef, method: &str) -> PhpResult<Value> {
    Err(twig::value::object::undefined_method(this.class(), method))
}

// ---- TemplateArrayAccessObject ----------------------------------------------------------------------------------

static TEMPLATE_ARRAY_ACCESS_OBJECT: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::class("Twig\\Tests\\TemplateArrayAccessObject")
        .implements(&builtin::array_access())
        .property_info(property(
            "protected",
            Visibility::Protected,
            Some(Value::from("protected")),
        ))
        .property_info(property(
            "attributes",
            Visibility::Public,
            Some(Value::Array(attribute_array(true))),
        ))
        .method("offsetExists")
        .method("offsetGet")
        .method("offsetSet")
        .method("offsetUnset")
        .build()
});

/// `class TemplateArrayAccessObject implements \ArrayAccess`: `offsetExists()` is `array_key_exists($name,
/// $this->attributes)`, `offsetGet()` the attribute or `null`.
pub fn template_array_access_object() -> Value {
    Value::Object(new_user(
        &TEMPLATE_ARRAY_ACCESS_OBJECT,
        |o, this, method, args, diag| {
            let attributes = o.property("attributes");
            match method {
                "offsetexists" => Ok(Value::Bool(array_key_exists(
                    &arg(&args, 0),
                    &attributes,
                    diag,
                )?)),
                "offsetget" => {
                    let name = arg(&args, 0);
                    if array_key_exists(&name, &attributes, diag)? {
                        dim::read(&attributes, &name, FetchMode::Read, diag)
                    } else {
                        Ok(Value::Null)
                    }
                }
                "offsetset" | "offsetunset" => Ok(Value::Null),
                _ => undefined(this, method),
            }
        },
    ))
}

// ---- TemplateMagicPropertyObject ---------------------------------------------------------------------------------

static TEMPLATE_MAGIC_PROPERTY_OBJECT: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::class("Twig\\Tests\\TemplateMagicPropertyObject")
        .property_info(property(
            "defined",
            Visibility::Public,
            Some(Value::from("defined")),
        ))
        .property_info(property(
            "attributes",
            Visibility::Public,
            Some(Value::Array(attribute_array(false))),
        ))
        .property_info(property(
            "protected",
            Visibility::Protected,
            Some(Value::from("protected")),
        ))
        .method("__isset")
        .method("__get")
        .build()
});

/// `class TemplateMagicPropertyObject`: `__isset()`/`__get()` over `$attributes`.
pub fn template_magic_property_object() -> Value {
    Value::Object(new_user(
        &TEMPLATE_MAGIC_PROPERTY_OBJECT,
        |o, this, method, args, diag| {
            let attributes = o.property("attributes");
            let name = arg(&args, 0);
            match method {
                "__isset" => Ok(Value::Bool(array_key_exists(&name, &attributes, diag)?)),
                "__get" => {
                    if array_key_exists(&name, &attributes, diag)? {
                        dim::read(&attributes, &name, FetchMode::Read, diag)
                    } else {
                        Ok(Value::Null)
                    }
                }
                _ => undefined(this, method),
            }
        },
    ))
}

// ---- TemplateMagicPropertyObjectWithException --------------------------------------------------------------------

static TEMPLATE_MAGIC_PROPERTY_OBJECT_WITH_EXCEPTION: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::class("Twig\\Tests\\TemplateMagicPropertyObjectWithException")
        .method("__isset")
        .build()
});

/// `class TemplateMagicPropertyObjectWithException`: `__isset()` throws.
pub fn template_magic_property_object_with_exception() -> Value {
    Value::Object(new_user(
        &TEMPLATE_MAGIC_PROPERTY_OBJECT_WITH_EXCEPTION,
        |_, this, method, _, _| match method {
            "__isset" => Err(PhpError::exception("Hey! Don't try to isset me!")),
            _ => undefined(this, method),
        },
    ))
}

// ---- TemplatePropertyObject and its subclasses -------------------------------------------------------------------

fn template_property_object_builder(name: &str) -> ClassBuilder {
    ClassBuilder::class(name)
        .property_info(property(
            "defined",
            Visibility::Public,
            Some(Value::from("defined")),
        ))
        .property_info(property("zero", Visibility::Public, Some(Value::Int(0))))
        .property_info(property("null", Visibility::Public, Some(Value::Null)))
        .property_info(property("bar", Visibility::Public, Some(Value::Bool(true))))
        .property_info(property("foo", Visibility::Public, Some(Value::Bool(true))))
        .property_info(property(
            "baz",
            Visibility::Public,
            Some(Value::from("baz")),
        ))
        .property_info(property(
            "baf",
            Visibility::Public,
            Some(Value::from("baf")),
        ))
        .property_info(property(
            "protected",
            Visibility::Protected,
            Some(Value::from("protected")),
        ))
}

static TEMPLATE_PROPERTY_OBJECT: LazyLock<ClassRef> = LazyLock::new(|| {
    template_property_object_builder("Twig\\Tests\\TemplatePropertyObject").build()
});

/// `class TemplatePropertyObject`: public properties only (and a protected one).
pub fn template_property_object() -> Value {
    Value::Object(new_user(
        &TEMPLATE_PROPERTY_OBJECT,
        |_, this, method, _, _| undefined(this, method),
    ))
}

static TEMPLATE_PROPERTY_OBJECT_AND_ITERATOR: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::class("Twig\\Tests\\TemplatePropertyObjectAndIterator")
        .extends(&TEMPLATE_PROPERTY_OBJECT)
        .implements(&builtin::iterator_aggregate())
        .method("getIterator")
        .build()
});

/// `class TemplatePropertyObjectAndIterator extends TemplatePropertyObject implements \IteratorAggregate`.
pub fn template_property_object_and_iterator() -> Value {
    Value::Object(new_user(
        &TEMPLATE_PROPERTY_OBJECT_AND_ITERATOR,
        |_, this, method, _, _| match method {
            "getiterator" => Ok(Value::Object(
                ArrayIterator::new(Array::from_values([Value::from("foo"), Value::from("bar")]))
                    .into_ref(),
            )),
            _ => undefined(this, method),
        },
    ))
}

static TEMPLATE_PROPERTY_OBJECT_AND_ARRAY_ACCESS: LazyLock<ClassRef> = LazyLock::new(|| {
    let mut data = Array::new();
    data.insert("defined", "defined");
    data.insert("zero", 0);
    data.insert("null", Value::Null);
    data.insert("bar", true);
    data.insert("foo", true);
    data.insert("baz", "baz");
    data.insert("baf", "baf");
    ClassBuilder::class("Twig\\Tests\\TemplatePropertyObjectAndArrayAccess")
        .extends(&TEMPLATE_PROPERTY_OBJECT)
        .implements(&builtin::array_access())
        .property_info(property(
            "data",
            Visibility::Private,
            Some(Value::Array(data)),
        ))
        .method("offsetExists")
        .method("offsetGet")
        .method("offsetSet")
        .method("offsetUnset")
        .build()
});

/// `class TemplatePropertyObjectAndArrayAccess extends TemplatePropertyObject implements \ArrayAccess`:
/// `offsetGet()` returns `'n/a'` for unknown offsets.
pub fn template_property_object_and_array_access() -> Value {
    Value::Object(new_user(
        &TEMPLATE_PROPERTY_OBJECT_AND_ARRAY_ACCESS,
        |o, this, method, args, diag| {
            let data = o.property("data");
            let offset = arg(&args, 0);
            match method {
                "offsetexists" => Ok(Value::Bool(array_key_exists(&offset, &data, diag)?)),
                "offsetget" => {
                    // return $this->offsetExists($offset) ? $this->data[$offset] : 'n/a';
                    let exists = this.invoke_method(
                        "offsetexists",
                        CallArgs::from(vec![offset.clone()]),
                        diag,
                    )?;
                    if exists.as_bool() == Some(true) {
                        dim::read(&data, &offset, FetchMode::Read, diag)
                    } else {
                        Ok(Value::from("n/a"))
                    }
                }
                "offsetset" | "offsetunset" => Ok(Value::Null),
                _ => undefined(this, method),
            }
        },
    ))
}

static TEMPLATE_PROPERTY_OBJECT_DEFINED_WITH_UNDEFINED_VALUE: LazyLock<ClassRef> =
    LazyLock::new(|| {
        ClassBuilder::class("Twig\\Tests\\TemplatePropertyObjectDefinedWithUndefinedValue")
            .property_info(property("foo", Visibility::Public, Some(Value::Null)))
            .method("__construct")
            .build()
    });

/// `class TemplatePropertyObjectDefinedWithUndefinedValue`: `$this->foo = @$notExist;` leaves `null`.
pub fn template_property_object_defined_with_undefined_value() -> Value {
    Value::Object(new_user(
        &TEMPLATE_PROPERTY_OBJECT_DEFINED_WITH_UNDEFINED_VALUE,
        |_, this, method, _, _| match method {
            "__construct" => Ok(Value::Null),
            _ => undefined(this, method),
        },
    ))
}

// ---- TemplateMethodObject ----------------------------------------------------------------------------------------

static TEMPLATE_METHOD_OBJECT: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::class("Twig\\Tests\\TemplateMethodObject")
        .method("getDefined")
        .method("get1")
        .method("get09")
        .method("getZero")
        .method("getNull")
        .method("isBar")
        .method("hasFoo")
        .method("hasBaz")
        .method("isBaz")
        .method("getBaz")
        .method("baz")
        .method("hasBaf")
        .method("isBaf")
        .method_with("getProtected", Visibility::Protected, false)
        .method_with("getStatic", Visibility::Public, true)
        .build()
});

/// `class TemplateMethodObject`: getters, issers and hassers.
pub fn template_method_object() -> Value {
    Value::Object(new_user(
        &TEMPLATE_METHOD_OBJECT,
        |_, this, method, _, _| {
            Ok(match method {
                "getdefined" => Value::from("defined"),
                "get1" => Value::Int(1),
                "get09" => Value::from("09"),
                "getzero" => Value::Int(0),
                "getnull" => Value::Null,
                "isbar" | "hasfoo" => Value::Bool(true),
                "hasbaz" => Value::from("should never be returned (has)"),
                "isbaz" => Value::from("should never be returned (is)"),
                "getbaz" => Value::from("Baz"),
                "baz" => Value::from("baz"),
                "hasbaf" => Value::from("should never be returned (has)"),
                "isbaf" => Value::from("baf"),
                "getprotected" => Value::from("protected"),
                "getstatic" => Value::from("static"),
                _ => return undefined(this, method),
            })
        },
    ))
}

// ---- TemplateGetIsMethods ----------------------------------------------------------------------------------------

static TEMPLATE_GET_IS_METHODS: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::class("Twig\\Tests\\TemplateGetIsMethods")
        .method("get")
        .method("is")
        .build()
});

/// `class TemplateGetIsMethods`: `get(): void` and `is(): void`.
pub fn template_get_is_methods() -> Value {
    Value::Object(new_user(
        &TEMPLATE_GET_IS_METHODS,
        |_, this, method, _, _| match method {
            "get" | "is" => Ok(Value::Null),
            _ => undefined(this, method),
        },
    ))
}

// ---- TemplateMethodAndPropObject ---------------------------------------------------------------------------------

static TEMPLATE_METHOD_AND_PROP_OBJECT: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::class("Twig\\Tests\\TemplateMethodAndPropObject")
        .property_info(property(
            "a",
            Visibility::Private,
            Some(Value::from("a_prop")),
        ))
        .property_info(property(
            "b",
            Visibility::Public,
            Some(Value::from("b_prop")),
        ))
        .property_info(property(
            "c",
            Visibility::Private,
            Some(Value::from("c_prop")),
        ))
        .method("getA")
        .method("getB")
        .method_with("getC", Visibility::Private, false)
        .build()
});

/// `class TemplateMethodAndPropObject`: a private property with a public getter, a public property with a public
/// getter, a private property with a private getter.
pub fn template_method_and_prop_object() -> Value {
    Value::Object(new_user(
        &TEMPLATE_METHOD_AND_PROP_OBJECT,
        |_, this, method, _, _| {
            Ok(match method {
                "geta" => Value::from("a"),
                "getb" => Value::from("b"),
                "getc" => Value::from("c"),
                _ => return undefined(this, method),
            })
        },
    ))
}

// ---- TemplateArrayAccess -----------------------------------------------------------------------------------------

static TEMPLATE_ARRAY_ACCESS: LazyLock<ClassRef> = LazyLock::new(|| {
    let mut vars = Array::new();
    vars.insert("foo", "bar");
    ClassBuilder::class("Twig\\Tests\\TemplateArrayAccess")
        .implements(&builtin::array_access())
        .property_info(property(
            "vars",
            Visibility::Public,
            Some(Value::Array(vars)),
        ))
        .property_info(property(
            "children",
            Visibility::Private,
            Some(Value::Array(Array::new())),
        ))
        .method("offsetExists")
        .method("offsetGet")
        .method("offsetSet")
        .method("offsetUnset")
        .build()
});

/// `class TemplateArrayAccess implements \ArrayAccess`: a public `$vars` property and private `$children`.
pub fn template_array_access() -> Value {
    Value::Object(new_user(
        &TEMPLATE_ARRAY_ACCESS,
        |o, this, method, args, diag| {
            let children = o.property("children");
            let offset = arg(&args, 0);
            match method {
                "offsetexists" => Ok(Value::Bool(array_key_exists(&offset, &children, diag)?)),
                "offsetget" => dim::read(&children, &offset, FetchMode::Read, diag),
                "offsetset" => {
                    let mut children = children.as_array().cloned().unwrap_or_default();
                    let key = twig::value::ArrayKey::from_value(
                        &offset,
                        twig::value::KeyContext::Write,
                        diag,
                    )?;
                    children.insert(key, arg(&args, 1));
                    o.properties.set(b"children", Value::Array(children));
                    Ok(Value::Null)
                }
                "offsetunset" => Ok(Value::Null),
                _ => undefined(this, method),
            }
        },
    ))
}

// ---- TemplateMagicMethodObject, TemplateMagicMethodExceptionObject ------------------------------------------------

static TEMPLATE_MAGIC_METHOD_OBJECT: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::class("Twig\\Tests\\TemplateMagicMethodObject")
        .method("__call")
        .build()
});

/// `class TemplateMagicMethodObject`: `__call($method, $arguments)` returns `'__call_'.$method`.
pub fn template_magic_method_object() -> Value {
    Value::Object(new_user(
        &TEMPLATE_MAGIC_METHOD_OBJECT,
        |_, this, method, args, diag| match method {
            "__call" => Ok(Value::String(twig::value::ops::concat(
                &Value::from("__call_"),
                &arg(&args, 0),
                diag,
            )?)),
            _ => undefined(this, method),
        },
    ))
}

static TEMPLATE_MAGIC_METHOD_EXCEPTION_OBJECT: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::class("Twig\\Tests\\TemplateMagicMethodExceptionObject")
        .method("__call")
        .build()
});

/// `class TemplateMagicMethodExceptionObject`: `__call()` throws a `BadMethodCallException`.
pub fn template_magic_method_exception_object() -> Value {
    Value::Object(new_user(
        &TEMPLATE_MAGIC_METHOD_EXCEPTION_OBJECT,
        |_, this, method, args, _| match method {
            "__call" => Err(PhpError::new(
                ThrowableClass::BadMethodCallException,
                format!(
                    "Unknown method \"{}\".",
                    arg(&args, 0)
                        .as_str()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default()
                ),
            )),
            _ => undefined(this, method),
        },
    ))
}

// ---- TemplateAttributeResolution, TemplateMagicPrivatePropertyObject ----------------------------------------------

static TEMPLATE_ATTRIBUTE_RESOLUTION: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::class("Twig\\Tests\\TemplateAttributeResolution")
        .constant("NAME", "constant")
        .property_info(property(
            "hidden",
            Visibility::Private,
            Some(Value::from("property")),
        ))
        .property_info(property(
            "secret",
            Visibility::Private,
            Some(Value::from("property")),
        ))
        .method("getHidden")
        .build()
});

/// `class TemplateAttributeResolution`: a constant, a private property with a getter, a private property without.
pub fn template_attribute_resolution() -> Value {
    Value::Object(new_user(
        &TEMPLATE_ATTRIBUTE_RESOLUTION,
        |_, this, method, _, _| match method {
            "gethidden" => Ok(Value::from("getter")),
            _ => undefined(this, method),
        },
    ))
}

static TEMPLATE_MAGIC_PRIVATE_PROPERTY_OBJECT: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::class("Twig\\Tests\\TemplateMagicPrivatePropertyObject")
        .property_info(property(
            "title",
            Visibility::Private,
            Some(Value::from("private")),
        ))
        .method("__isset")
        .method("__get")
        .build()
});

/// `class TemplateMagicPrivatePropertyObject`: a private `$title` and `__isset()`/`__get()` answering for it.
pub fn template_magic_private_property_object() -> Value {
    Value::Object(new_user(
        &TEMPLATE_MAGIC_PRIVATE_PROPERTY_OBJECT,
        |_, this, method, args, _| {
            let is_title = arg(&args, 0).as_str().map(|s| s.as_bytes() == b"title") == Some(true);
            match method {
                "__isset" => Ok(Value::Bool(is_title)),
                "__get" => Ok(if is_title {
                    Value::from("magic")
                } else {
                    Value::Null
                }),
                _ => undefined(this, method),
            }
        },
    ))
}

// ---- TemplateStringableKey ---------------------------------------------------------------------------------------

static TEMPLATE_STRINGABLE_KEY: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::class("Twig\\Tests\\TemplateStringableKey")
        .final_class()
        .implements(&builtin::stringable())
        .property_info(typed_property("toStringCalls", "int", Some(Value::Int(0))))
        .method("__toString")
        .build()
});

/// `final class TemplateStringableKey implements \Stringable`: `__toString()` counts its calls and returns
/// `'string'`.
pub fn template_stringable_key() -> ObjectRef {
    new_user(
        &TEMPLATE_STRINGABLE_KEY,
        |o, this, method, _, _| match method {
            "__tostring" => {
                let calls = o.property("toStringCalls").as_int().unwrap_or(0);
                o.properties.set(b"toStringCalls", Value::Int(calls + 1));
                Ok(Value::from("string"))
            }
            _ => undefined(this, method),
        },
    )
}

/// `$key->toStringCalls`.
pub fn to_string_calls(key: &ObjectRef) -> i64 {
    key.read_property(b"toStringCalls", FetchMode::Read, &mut twig::value::Ignore)
        .ok()
        .and_then(|v| v.as_int())
        .unwrap_or(-1)
}

// ---- TemplateTrackingArrayAccess ---------------------------------------------------------------------------------

static TEMPLATE_TRACKING_ARRAY_ACCESS: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::class("Twig\\Tests\\TemplateTrackingArrayAccess")
        .final_class()
        .implements(&builtin::array_access())
        .property_info(typed_property(
            "offsetExistsCalls",
            "array",
            Some(Value::Array(Array::new())),
        ))
        .property_info(typed_property(
            "offsetExistsError",
            "?TypeError",
            Some(Value::Null),
        ))
        .property_info(typed_property(
            "offsetGetCalls",
            "array",
            Some(Value::Array(Array::new())),
        ))
        .property_info(PropertyInfo {
            type_name: Some("object".into()),
            ..property("key", Visibility::Private, None)
        })
        .property_info(PropertyInfo {
            type_name: Some("bool".into()),
            ..property("rejectObjectOffset", Visibility::Private, None)
        })
        .method("__construct")
        .method("offsetExists")
        .method("offsetGet")
        .method("offsetSet")
        .method("offsetUnset")
        .build()
});

fn push_property(o: &UserObject, name: &str, value: Value) {
    let mut list = o.property(name).as_array().cloned().unwrap_or_default();
    list.push(value).expect("push");
    o.properties.set(name.as_bytes(), Value::Array(list));
}

/// `final class TemplateTrackingArrayAccess implements \ArrayAccess`: records the offsets given to `offsetExists()`
/// and `offsetGet()`; `offsetExists()` throws a `TypeError` for object offsets when `$rejectObjectOffset`, else
/// answers `$this->key === $offset`.
pub fn template_tracking_array_access(key: &ObjectRef, reject_object_offset: bool) -> ObjectRef {
    let object = new_user(
        &TEMPLATE_TRACKING_ARRAY_ACCESS,
        |o, this, method, args, _| {
            let offset = arg(&args, 0);
            match method {
                "offsetexists" => {
                    push_property(o, "offsetExistsCalls", offset.clone());
                    if o.property("rejectObjectOffset").as_bool() == Some(true)
                        && matches!(offset, Value::Object(_))
                    {
                        // throw $this->offsetExistsError = new \TypeError('Object offsets are not supported.');
                        return Err(PhpError::type_error("Object offsets are not supported."));
                    }
                    Ok(Value::Bool(o.property("key").identical(&offset)))
                }
                "offsetget" => {
                    push_property(o, "offsetGetCalls", offset);
                    Ok(Value::from("value"))
                }
                "offsetset" | "offsetunset" | "__construct" => Ok(Value::Null),
                _ => undefined(this, method),
            }
        },
    );
    let store = &object
        .downcast_ref::<UserObject>()
        .expect("a user object")
        .properties;
    store.set(b"key", Value::Object(key.clone()));
    store.set(b"rejectObjectOffset", Value::Bool(reject_object_offset));
    object
}

/// `$data->offsetExistsCalls` / `$data->offsetGetCalls`.
pub fn tracked_calls(data: &ObjectRef, property: &str) -> Vec<Value> {
    data.read_property(
        property.as_bytes(),
        FetchMode::Read,
        &mut twig::value::Ignore,
    )
    .ok()
    .and_then(|v| v.as_array().cloned())
    .map(|a| a.values().cloned().collect())
    .unwrap_or_default()
}

// ---- subclasses of ArrayObject and ArrayIterator ------------------------------------------------------------------

/// A class extending an internal SPL array class without overriding anything (`final class
/// TemplateArrayObjectSubclass extends \ArrayObject {}`): the object behaves exactly like its parent's instance, only
/// its class differs. twig-rs's `ArrayObject`/`ArrayIterator` cannot be given another class, so the subclass forwards
/// every handler to a parent instance.
struct SplArraySubclass {
    class: ClassRef,
    inner: ObjectRef,
}

impl Object for SplArraySubclass {
    fn class(&self) -> &ClassRef {
        &self.class
    }

    fn call(
        &self,
        _this: &ObjectRef,
        method: &str,
        args: CallArgs,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        self.inner.invoke_method(method, args, diag)
    }

    fn read_dimension(
        &self,
        _this: &ObjectRef,
        offset: &Value,
        mode: FetchMode,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        self.inner.read_dimension(offset, mode, diag)
    }

    fn has_dimension(
        &self,
        _this: &ObjectRef,
        offset: &Value,
        check_empty: bool,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<bool> {
        self.inner.has_dimension(offset, check_empty, diag)
    }

    fn write_dimension(
        &self,
        _this: &ObjectRef,
        offset: Option<&Value>,
        value: Value,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<()> {
        self.inner.write_dimension(offset, value, diag)
    }

    fn unset_dimension(
        &self,
        _this: &ObjectRef,
        offset: &Value,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<()> {
        self.inner.unset_dimension(offset, diag)
    }

    fn count_elements(
        &self,
        _this: &ObjectRef,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Option<i64>> {
        self.inner.count(diag)
    }

    fn get_iterator(
        &self,
        _this: &ObjectRef,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Option<Box<dyn ObjectIterator>>> {
        self.inner.get_iterator(diag)
    }
}

static TEMPLATE_ARRAY_OBJECT_SUBCLASS: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::class("Twig\\Tests\\TemplateArrayObjectSubclass")
        .final_class()
        .extends(&builtin::array_object())
        .build()
});

static TEMPLATE_ARRAY_ITERATOR_SUBCLASS: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::class("Twig\\Tests\\TemplateArrayIteratorSubclass")
        .final_class()
        .extends(&builtin::array_iterator())
        .build()
});

static RECURSIVE_ARRAY_ITERATOR: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::class("RecursiveArrayIterator")
        .extends(&builtin::array_iterator())
        .method("hasChildren")
        .method("getChildren")
        .build()
});

/// A subclass of `ArrayIterator` named `name` (only `RecursiveArrayIterator`, which twig-value does not provide).
pub fn named_array_iterator_subclass(name: &str, array: Array) -> Value {
    assert_eq!(name, "RecursiveArrayIterator");
    Value::Object(ObjectRef::new(SplArraySubclass {
        class: RECURSIVE_ARRAY_ITERATOR.clone(),
        inner: ArrayIterator::new(array).into_ref(),
    }))
}

/// `new TemplateArrayObjectSubclass($array)`.
pub fn template_array_object_subclass(array: Array) -> Value {
    Value::Object(ObjectRef::new(SplArraySubclass {
        class: TEMPLATE_ARRAY_OBJECT_SUBCLASS.clone(),
        inner: ArrayObject::new(array).into_ref(),
    }))
}

/// `new TemplateArrayIteratorSubclass($array)`.
pub fn template_array_iterator_subclass(array: Array) -> Value {
    Value::Object(ObjectRef::new(SplArraySubclass {
        class: TEMPLATE_ARRAY_ITERATOR_SUBCLASS.clone(),
        inner: ArrayIterator::new(array).into_ref(),
    }))
}

// ---- ErrorTest_Foo -----------------------------------------------------------------------------------------------

static ERROR_TEST_FOO: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::class("Twig\\Tests\\ErrorTest_Foo")
        .method("bar")
        .build()
});

/// `class ErrorTest_Foo`: `bar()` throws `new \Exception('Runtime error...')`.
pub fn error_test_foo() -> Value {
    Value::Object(new_user(
        &ERROR_TEST_FOO,
        |_, this, method, _, _| match method {
            "bar" => Err(PhpError::exception("Runtime error...")),
            _ => undefined(this, method),
        },
    ))
}

// ---- other values ------------------------------------------------------------------------------------------------

/// `(object) $array`.
pub fn std_object(array: &Array) -> Value {
    Value::Object(
        StdClass::with_properties(
            array
                .iter()
                .map(|(key, value)| (key.to_php_string(), value.clone())),
        )
        .into_ref(),
    )
}

/// `new \stdClass()`.
pub fn std_class() -> Value {
    Value::Object(StdClass::new().into_ref())
}

/// `static fn (): string => $result`.
pub fn closure(result: &'static str) -> Value {
    Value::Object(Closure::new(move |_, _| Ok(Value::from(result))).into_ref())
}
