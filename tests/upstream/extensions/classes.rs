//! Rust versions of the PHP helper classes of the ported test files: `FooObject` & co. of
//! `TWIG/tests/Extension/SandboxTest.php`, `CoreTestIterator` & co. of `TWIG/tests/Extension/CoreTest.php` and the
//! classes of `TWIG/tests/Runtime/EscaperRuntimeTest.php`. Members are declared in PHP's order; method bodies are
//! ported one to one.
//!
//! `FooObject::$called` is a static counter in PHP (PHPUnit runs the tests one after the other); the Rust tests run in
//! parallel threads, so every object carries the [`Counters`] of its test instead (`getAnotherFooObject()` passes them
//! on).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex};

use twig::value::object::{
    ArrayIterator, CallArgs, ClassBuilder, ClassRef, Object, ObjectRef, PropertyInfo,
    PropertyStore, Visibility, builtin,
};
use twig::value::{Array, ArrayKey, Diagnostics, PhpError, PhpResult, ThrowableClass, Value};
use twig_test_support::classes::generator::{Generator, GeneratorStep};

fn private_property(class: &str, name: &str, type_name: Option<&str>) -> PropertyInfo {
    PropertyInfo {
        name: name.into(),
        visibility: Visibility::Private,
        is_static: false,
        is_readonly: false,
        type_name: type_name.map(Into::into),
        default: if type_name.is_some() {
            None
        } else {
            Some(Value::Null)
        },
        declaring_class: class.into(),
    }
}

fn undefined(class: &ClassRef, method: &str) -> PhpError {
    twig::value::object::undefined_method(class, method)
}

/// A `Generator` yielding the given pairs (`yield from $array`, `yield $value`).
fn generator(function: &str, pairs: Vec<(Value, Value)>) -> Value {
    let mut pairs = pairs.into_iter();
    let body: GeneratorStep = Box::new(move |_diag| Ok(pairs.next()));
    Value::Object(Generator::new(function.to_owned(), body).into_ref())
}

/// The `(key, value)` pairs of an array, for `yield from`.
fn array_pairs(array: &Array) -> Vec<(Value, Value)> {
    array
        .iter()
        .map(|(key, value)| {
            let key = match key {
                ArrayKey::Int(i) => Value::Int(*i),
                ArrayKey::Str(s) => Value::String(s.clone()),
            };
            (key, value.clone())
        })
        .collect()
}

// ---- Twig\Tests\Extension\FooObject (SandboxTest.php) ------------------------------------------------------------

/// `FooObject::$called`: how often `__toString()`, `foo()` and `getFooBar()` ran.
#[derive(Debug, Default)]
pub struct Counters {
    pub to_string: AtomicUsize,
    pub foo: AtomicUsize,
    pub get_foo_bar: AtomicUsize,
}

impl Counters {
    pub fn new() -> Arc<Counters> {
        Arc::new(Counters::default())
    }

    pub fn to_string(&self) -> usize {
        self.to_string.load(Ordering::SeqCst)
    }

    pub fn foo(&self) -> usize {
        self.foo.load(Ordering::SeqCst)
    }

    pub fn get_foo_bar(&self) -> usize {
        self.get_foo_bar.load(Ordering::SeqCst)
    }
}

pub const FOO_OBJECT: &str = "Twig\\Tests\\Extension\\FooObject";

fn foo_object_methods(builder: ClassBuilder) -> ClassBuilder {
    builder
        .method_with("reset", Visibility::Public, true)
        .method("__toString")
        .method("foo")
        .method("getFooBar")
        .method("getAnotherFooObject")
}

static FOO_OBJECT_CLASS: LazyLock<ClassRef> = LazyLock::new(|| {
    foo_object_methods(ClassBuilder::class(FOO_OBJECT).property("bar", "bar")).build()
});

static COUNTABLE_FOO_OBJECT_CLASS: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::class("Twig\\Tests\\Extension\\CountableFooObject")
        .extends(&FOO_OBJECT_CLASS)
        .implements(&builtin::countable())
        .method("count")
        .build()
});

/// `class FooObject` (and `class CountableFooObject extends FooObject implements \Countable`).
pub struct FooObject {
    class: ClassRef,
    properties: PropertyStore,
    counters: Arc<Counters>,
}

impl FooObject {
    /// `new FooObject()` counting into `counters`.
    pub fn with_counters(counters: &Arc<Counters>) -> Value {
        Self::of_class(&FOO_OBJECT_CLASS, counters)
    }

    /// `new FooObject()` with counters nobody reads.
    pub fn value() -> Value {
        Self::with_counters(&Counters::new())
    }

    /// `new CountableFooObject()`.
    pub fn countable() -> Value {
        Self::of_class(&COUNTABLE_FOO_OBJECT_CLASS, &Counters::new())
    }

    fn of_class(class: &ClassRef, counters: &Arc<Counters>) -> Value {
        Value::Object(ObjectRef::new(FooObject {
            class: class.clone(),
            properties: PropertyStore::new(class),
            counters: Arc::clone(counters),
        }))
    }
}

impl Object for FooObject {
    fn class(&self) -> &ClassRef {
        &self.class
    }

    fn property_store(&self) -> Option<&PropertyStore> {
        Some(&self.properties)
    }

    fn call(
        &self,
        _this: &ObjectRef,
        method: &str,
        _args: CallArgs,
        _diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        Ok(match method {
            "reset" => Value::Null,
            "__tostring" => {
                self.counters.to_string.fetch_add(1, Ordering::SeqCst);
                Value::from("foo")
            }
            "foo" => {
                self.counters.foo.fetch_add(1, Ordering::SeqCst);
                Value::from("foo")
            }
            "getfoobar" => {
                self.counters.get_foo_bar.fetch_add(1, Ordering::SeqCst);
                Value::from("foobar")
            }
            // return new self();
            "getanotherfooobject" => FooObject::of_class(&FOO_OBJECT_CLASS, &self.counters),
            "count" if self.class.name() != FOO_OBJECT => Value::Int(0),
            _ => return Err(undefined(&self.class, method)),
        })
    }
}

// ---- ParentClass / ChildClass ---------------------------------------------------------------------------------

static PARENT_CLASS: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::class("Twig\\Tests\\Extension\\ParentClass")
        .method("ParentMethod")
        .build()
});

static CHILD_CLASS: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::class("Twig\\Tests\\Extension\\ChildClass")
        .extends(&PARENT_CLASS)
        .method("ChildMethod")
        .build()
});

/// `class ChildClass extends ParentClass`.
pub struct ChildClass;

impl ChildClass {
    pub fn value() -> Value {
        Value::Object(ObjectRef::new(ChildClass))
    }
}

impl Object for ChildClass {
    fn class(&self) -> &ClassRef {
        &CHILD_CLASS
    }

    fn call(
        &self,
        _this: &ObjectRef,
        method: &str,
        _args: CallArgs,
        _diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        match method {
            "parentmethod" | "childmethod" => Ok(Value::Null),
            _ => Err(undefined(&CHILD_CLASS, method)),
        }
    }
}

// ---- ArrayLikeObject extends \ArrayObject ----------------------------------------------------------------------

static ARRAY_LIKE_OBJECT: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::class("Twig\\Tests\\Extension\\ArrayLikeObject")
        .extends(&builtin::array_object())
        .method("offsetExists")
        .method("offsetGet")
        .method("offsetSet")
        .method("offsetUnset")
        .build()
});

/// `class ArrayLikeObject extends \ArrayObject` whose `offsetExists()`/`offsetGet()` throw.
pub struct ArrayLikeObject;

impl ArrayLikeObject {
    pub fn value() -> Value {
        Value::Object(ObjectRef::new(ArrayLikeObject))
    }
}

impl Object for ArrayLikeObject {
    fn class(&self) -> &ClassRef {
        &ARRAY_LIKE_OBJECT
    }

    fn call(
        &self,
        _this: &ObjectRef,
        method: &str,
        _args: CallArgs,
        _diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        match method {
            "offsetexists" | "offsetget" => Err(PhpError::new(
                ThrowableClass::BadMethodCallException,
                "Should not be called.",
            )),
            "offsetset" | "offsetunset" => Ok(Value::Null),
            "count" => Ok(Value::Int(0)),
            _ => Err(undefined(&ARRAY_LIKE_OBJECT, method)),
        }
    }
}

// ---- MagicObject ------------------------------------------------------------------------------------------------

static MAGIC_OBJECT: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::class("Twig\\Tests\\Extension\\MagicObject")
        .method("__get")
        .method("__isset")
        .build()
});

/// `class MagicObject` whose `__get()`/`__isset()` throw.
pub struct MagicObject {
    properties: PropertyStore,
}

impl MagicObject {
    pub fn value() -> Value {
        Value::Object(ObjectRef::new(MagicObject {
            properties: PropertyStore::new(&MAGIC_OBJECT),
        }))
    }
}

impl Object for MagicObject {
    fn class(&self) -> &ClassRef {
        &MAGIC_OBJECT
    }

    fn property_store(&self) -> Option<&PropertyStore> {
        Some(&self.properties)
    }

    fn call(
        &self,
        _this: &ObjectRef,
        method: &str,
        args: CallArgs,
        _diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        let name = args
            .get(0)
            .and_then(Value::as_str)
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        match method {
            "__get" | "__isset" => Err(PhpError::new(
                ThrowableClass::BadMethodCallException,
                format!("{method}({name}) should not be called inside the sandbox."),
            )),
            _ => Err(undefined(&MAGIC_OBJECT, method)),
        }
    }
}

// ---- MagicCallObject -------------------------------------------------------------------------------------------

pub const MAGIC_CALL_OBJECT: &str = "Twig\\Tests\\Extension\\MagicCallObject";

static MAGIC_CALL_OBJECT_CLASS: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::class(MAGIC_CALL_OBJECT)
        .property("secret", "secret")
        .method("__call")
        .build()
});

/// `class MagicCallObject` with `public $secret` and a `__call()` returning `call:<name>`.
pub struct MagicCallObject {
    properties: PropertyStore,
}

impl MagicCallObject {
    pub fn value() -> Value {
        Value::Object(ObjectRef::new(MagicCallObject {
            properties: PropertyStore::new(&MAGIC_CALL_OBJECT_CLASS),
        }))
    }
}

impl Object for MagicCallObject {
    fn class(&self) -> &ClassRef {
        &MAGIC_CALL_OBJECT_CLASS
    }

    fn property_store(&self) -> Option<&PropertyStore> {
        Some(&self.properties)
    }

    fn call(
        &self,
        _this: &ObjectRef,
        method: &str,
        args: CallArgs,
        _diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        match method {
            "__call" => {
                let name = args
                    .get(0)
                    .and_then(Value::as_str)
                    .cloned()
                    .unwrap_or_default();
                Ok(Value::String(twig::value::PhpString::concat(&[
                    b"call:",
                    name.as_bytes(),
                ])))
            }
            _ => Err(undefined(&MAGIC_CALL_OBJECT_CLASS, method)),
        }
    }
}

// ---- ColumnObject, PlainBaseObject, StringablePlainObject -------------------------------------------------------

pub const COLUMN_OBJECT: &str = "Twig\\Tests\\Extension\\ColumnObject";

static COLUMN_OBJECT_CLASS: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::class(COLUMN_OBJECT)
        .property("bar", "bar")
        .build()
});

/// `class ColumnObject { public $bar = 'bar'; }`.
pub struct ColumnObject {
    properties: PropertyStore,
}

impl ColumnObject {
    pub fn value() -> Value {
        Value::Object(ObjectRef::new(ColumnObject {
            properties: PropertyStore::new(&COLUMN_OBJECT_CLASS),
        }))
    }
}

impl Object for ColumnObject {
    fn class(&self) -> &ClassRef {
        &COLUMN_OBJECT_CLASS
    }

    fn property_store(&self) -> Option<&PropertyStore> {
        Some(&self.properties)
    }
}

static PLAIN_BASE_OBJECT: LazyLock<ClassRef> =
    LazyLock::new(|| ClassBuilder::class("Twig\\Tests\\Extension\\PlainBaseObject").build());

pub const STRINGABLE_PLAIN_OBJECT: &str = "Twig\\Tests\\Extension\\StringablePlainObject";

static STRINGABLE_PLAIN_OBJECT_CLASS: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::class(STRINGABLE_PLAIN_OBJECT)
        .extends(&PLAIN_BASE_OBJECT)
        .implements(&builtin::stringable())
        .method("__toString")
        .build()
});

/// `class StringablePlainObject extends PlainBaseObject implements \Stringable`.
pub struct StringablePlainObject;

impl StringablePlainObject {
    pub fn value() -> Value {
        Value::Object(ObjectRef::new(StringablePlainObject))
    }
}

impl Object for StringablePlainObject {
    fn class(&self) -> &ClassRef {
        &STRINGABLE_PLAIN_OBJECT_CLASS
    }

    fn call(
        &self,
        _this: &ObjectRef,
        method: &str,
        _args: CallArgs,
        _diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        match method {
            "__tostring" => Ok(Value::from("plain")),
            _ => Err(undefined(&STRINGABLE_PLAIN_OBJECT_CLASS, method)),
        }
    }
}

// ---- StringableTraversableObject, CyclicTraversableObject ---------------------------------------------------------

pub const STRINGABLE_TRAVERSABLE_OBJECT: &str =
    "Twig\\Tests\\Extension\\StringableTraversableObject";

static STRINGABLE_TRAVERSABLE_OBJECT_CLASS: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::class(STRINGABLE_TRAVERSABLE_OBJECT)
        .implements(&builtin::iterator_aggregate())
        .implements(&builtin::stringable())
        .property_info(private_property(
            STRINGABLE_TRAVERSABLE_OBJECT,
            "items",
            Some("array"),
        ))
        .method("__construct")
        .method("__toString")
        .method("getIterator")
        .build()
});

/// `class StringableTraversableObject implements \IteratorAggregate, \Stringable` (`getIterator()` is a generator).
pub struct StringableTraversableObject {
    properties: PropertyStore,
}

impl StringableTraversableObject {
    pub fn value(items: Array) -> Value {
        let object = StringableTraversableObject {
            properties: PropertyStore::new(&STRINGABLE_TRAVERSABLE_OBJECT_CLASS),
        };
        object.properties.set(b"items", Value::Array(items));
        Value::Object(ObjectRef::new(object))
    }
}

impl Object for StringableTraversableObject {
    fn class(&self) -> &ClassRef {
        &STRINGABLE_TRAVERSABLE_OBJECT_CLASS
    }

    fn property_store(&self) -> Option<&PropertyStore> {
        Some(&self.properties)
    }

    fn call(
        &self,
        _this: &ObjectRef,
        method: &str,
        _args: CallArgs,
        _diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        match method {
            "__tostring" => Ok(Value::from("stringable-traversable")),
            // yield from $this->items;
            "getiterator" => {
                let items = match self.properties.get(b"items") {
                    Some(Value::Array(items)) => items,
                    _ => Array::new(),
                };
                Ok(generator(
                    "Twig\\Tests\\Extension\\StringableTraversableObject::getIterator",
                    array_pairs(&items),
                ))
            }
            _ => Err(undefined(&STRINGABLE_TRAVERSABLE_OBJECT_CLASS, method)),
        }
    }
}

static CYCLIC_TRAVERSABLE_OBJECT: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::class("Twig\\Tests\\Extension\\CyclicTraversableObject")
        .implements(&builtin::iterator_aggregate())
        .method("getIterator")
        .build()
});

/// `class CyclicTraversableObject implements \IteratorAggregate` whose generator yields `$this`.
pub struct CyclicTraversableObject;

impl CyclicTraversableObject {
    pub fn value() -> Value {
        Value::Object(ObjectRef::new(CyclicTraversableObject))
    }
}

impl Object for CyclicTraversableObject {
    fn class(&self) -> &ClassRef {
        &CYCLIC_TRAVERSABLE_OBJECT
    }

    fn call(
        &self,
        this: &ObjectRef,
        method: &str,
        _args: CallArgs,
        _diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        match method {
            // yield $this;
            "getiterator" => Ok(generator(
                "Twig\\Tests\\Extension\\CyclicTraversableObject::getIterator",
                vec![(Value::Int(0), Value::Object(this.clone()))],
            )),
            _ => Err(undefined(&CYCLIC_TRAVERSABLE_OBJECT, method)),
        }
    }
}

// ---- anonymous ArrayAccess classes ----------------------------------------------------------------------------

static NULL_ARRAY_ACCESS: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::class("ArrayAccess@anonymous")
        .anonymous()
        .implements(&builtin::array_access())
        .method("offsetGet")
        .method("offsetExists")
        .method("offsetSet")
        .method("offsetUnset")
        .build()
});

/// `new class implements \ArrayAccess` of `testSandboxNonStringKeyAccessDoesNotTriggerImplicitConversionDeprecation`:
/// `offsetGet()` returns `null`, `offsetExists()` `false`.
pub struct NullArrayAccess;

impl NullArrayAccess {
    pub fn value() -> Value {
        Value::Object(ObjectRef::new(NullArrayAccess))
    }
}

impl Object for NullArrayAccess {
    fn class(&self) -> &ClassRef {
        &NULL_ARRAY_ACCESS
    }

    fn call(
        &self,
        _this: &ObjectRef,
        method: &str,
        _args: CallArgs,
        _diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        match method {
            "offsetget" | "offsetset" | "offsetunset" => Ok(Value::Null),
            "offsetexists" => Ok(Value::Bool(false)),
            _ => Err(undefined(&NULL_ARRAY_ACCESS, method)),
        }
    }
}

static NON_COUNTABLE_SEQUENCE: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::class("ArrayAccess@anonymous")
        .anonymous()
        .implements(&builtin::array_access())
        .implements(&builtin::iterator_aggregate())
        .method("offsetExists")
        .method("offsetGet")
        .method("offsetSet")
        .method("offsetUnset")
        .method("getIterator")
        .build()
});

/// `new class implements \ArrayAccess, \IteratorAggregate` of `testCycleWithArrayAccessAndTraversableButNotCountable`:
/// every offset exists and is `val`; the generator yields `odd` and `even`.
pub struct NonCountableSequence;

impl NonCountableSequence {
    pub fn value() -> Value {
        Value::Object(ObjectRef::new(NonCountableSequence))
    }
}

impl Object for NonCountableSequence {
    fn class(&self) -> &ClassRef {
        &NON_COUNTABLE_SEQUENCE
    }

    fn call(
        &self,
        _this: &ObjectRef,
        method: &str,
        _args: CallArgs,
        _diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        match method {
            "offsetexists" => Ok(Value::Bool(true)),
            "offsetget" => Ok(Value::from("val")),
            "offsetset" | "offsetunset" => Ok(Value::Null),
            "getiterator" => Ok(generator(
                "ArrayAccess@anonymous::getIterator",
                vec![
                    (Value::Int(0), Value::from("odd")),
                    (Value::Int(1), Value::from("even")),
                ],
            )),
            _ => Err(undefined(&NON_COUNTABLE_SEQUENCE, method)),
        }
    }
}

// ---- CoreTestIterator & co. (CoreTest.php) ---------------------------------------------------------------------

const CORE_TEST_ITERATOR: &str = "Twig\\Tests\\Extension\\CoreTestIterator";

static CORE_TEST_ITERATOR_CLASS: LazyLock<ClassRef> = LazyLock::new(|| {
    let c = CORE_TEST_ITERATOR;
    ClassBuilder::class(c)
        .final_class()
        .implements(&builtin::iterator())
        .property_info(private_property(c, "position", None))
        .property_info(private_property(c, "array", None))
        .property_info(private_property(c, "arrayKeys", None))
        .property_info(private_property(c, "allowValueAccess", None))
        .property_info(private_property(c, "maxPosition", None))
        .method("__construct")
        .method("rewind")
        .method("current")
        .method("key")
        .method("next")
        .method("valid")
        .build()
});

struct IteratorState {
    position: i64,
    array: Array,
    keys: Vec<Value>,
    allow_value_access: bool,
    max_position: i64,
}

/// `final class CoreTestIterator implements \Iterator`: throws when its values are read without `$allowValueAccess`
/// and when it is moved to `$maxPosition`.
pub struct CoreTestIterator {
    state: Mutex<IteratorState>,
}

impl CoreTestIterator {
    /// `new CoreTestIterator($values, $keys, $allowValueAccess, $maxPosition)`; `None` is `false` (`count($values) +
    /// 1`).
    pub fn object(
        values: &Array,
        keys: &[Value],
        allow_value_access: bool,
        max_position: Option<i64>,
    ) -> ObjectRef {
        ObjectRef::new(CoreTestIterator {
            state: Mutex::new(IteratorState {
                position: 0,
                array: values.clone(),
                keys: keys.to_vec(),
                allow_value_access,
                max_position: max_position.unwrap_or(values.len() as i64 + 1),
            }),
        })
    }

    pub fn value(
        values: &Array,
        keys: &[Value],
        allow_value_access: bool,
        max_position: Option<i64>,
    ) -> Value {
        Value::Object(Self::object(values, keys, allow_value_access, max_position))
    }
}

fn logic_exception(message: impl Into<String>) -> PhpError {
    PhpError::new(ThrowableClass::LogicException, message.into())
}

impl Object for CoreTestIterator {
    fn class(&self) -> &ClassRef {
        &CORE_TEST_ITERATOR_CLASS
    }

    fn call(
        &self,
        _this: &ObjectRef,
        method: &str,
        _args: CallArgs,
        _diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        let mut state = self.state.lock().unwrap();
        let key = |state: &IteratorState| {
            usize::try_from(state.position)
                .ok()
                .and_then(|i| state.keys.get(i).cloned())
                .unwrap_or_default()
        };
        Ok(match method {
            "rewind" => {
                state.position = 0;
                Value::Null
            }
            "current" => {
                if !state.allow_value_access {
                    return Err(logic_exception(
                        "Code should only use the keys, not the values provided by iterator.",
                    ));
                }
                let key = match key(&state) {
                    Value::Int(i) => ArrayKey::Int(i),
                    Value::String(s) => ArrayKey::from(s),
                    _ => return Ok(Value::Null),
                };
                state.array.get(&key).cloned().unwrap_or_default()
            }
            "key" => key(&state),
            "next" => {
                state.position += 1;
                if state.position == state.max_position {
                    return Err(logic_exception(format!(
                        "Code should not iterate beyond {}.",
                        state.max_position
                    )));
                }
                Value::Null
            }
            "valid" => Value::Bool(
                usize::try_from(state.position)
                    .ok()
                    .is_some_and(|i| i < state.keys.len()),
            ),
            _ => return Err(undefined(&CORE_TEST_ITERATOR_CLASS, method)),
        })
    }
}

static CORE_TEST_ITERATOR_AGGREGATE: LazyLock<ClassRef> = LazyLock::new(|| {
    let c = "Twig\\Tests\\Extension\\CoreTestIteratorAggregate";
    ClassBuilder::class(c)
        .final_class()
        .implements(&builtin::iterator_aggregate())
        .property_info(private_property(c, "iterator", None))
        .method("__construct")
        .method("getIterator")
        .build()
});

static CORE_TEST_ITERATOR_AGGREGATE_AGGREGATE: LazyLock<ClassRef> = LazyLock::new(|| {
    let c = "Twig\\Tests\\Extension\\CoreTestIteratorAggregateAggregate";
    ClassBuilder::class(c)
        .final_class()
        .implements(&builtin::iterator_aggregate())
        .property_info(private_property(c, "iterator", None))
        .method("__construct")
        .method("getIterator")
        .build()
});

/// `final class CoreTestIteratorAggregate` / `CoreTestIteratorAggregateAggregate`: `getIterator()` returns the inner
/// object created by the constructor (the same object every time).
pub struct CoreTestAggregate {
    class: ClassRef,
    iterator: ObjectRef,
}

impl CoreTestAggregate {
    /// `new CoreTestIteratorAggregate($array, $keys, $allowAccess, $maxPosition)`.
    pub fn aggregate(
        values: &Array,
        keys: &[Value],
        allow_value_access: bool,
        max_position: Option<i64>,
    ) -> ObjectRef {
        ObjectRef::new(CoreTestAggregate {
            class: CORE_TEST_ITERATOR_AGGREGATE.clone(),
            iterator: CoreTestIterator::object(values, keys, allow_value_access, max_position),
        })
    }

    /// `new CoreTestIteratorAggregateAggregate($array, $keys, $allowValueAccess, $maxPosition)`.
    pub fn aggregate_aggregate(
        values: &Array,
        keys: &[Value],
        allow_value_access: bool,
        max_position: Option<i64>,
    ) -> ObjectRef {
        ObjectRef::new(CoreTestAggregate {
            class: CORE_TEST_ITERATOR_AGGREGATE_AGGREGATE.clone(),
            iterator: Self::aggregate(values, keys, allow_value_access, max_position),
        })
    }
}

impl Object for CoreTestAggregate {
    fn class(&self) -> &ClassRef {
        &self.class
    }

    fn call(
        &self,
        _this: &ObjectRef,
        method: &str,
        _args: CallArgs,
        _diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        match method {
            "getiterator" => Ok(Value::Object(self.iterator.clone())),
            _ => Err(undefined(&self.class, method)),
        }
    }
}

/// `new \ArrayIterator($array)`.
pub fn array_iterator(array: Array) -> Value {
    Value::Object(ArrayIterator::new(array).into_ref())
}

// ---- Extension_SafeHtmlInterface, Extension_TestClass (EscaperRuntimeTest.php) ---------------------------------

pub const EXTENSION_SAFE_HTML_INTERFACE: &str = "Twig\\Tests\\Runtime\\Extension_SafeHtmlInterface";
pub const EXTENSION_TEST_CLASS: &str = "Twig\\Tests\\Runtime\\Extension_TestClass";

static SAFE_HTML_INTERFACE: LazyLock<ClassRef> =
    LazyLock::new(|| ClassBuilder::interface(EXTENSION_SAFE_HTML_INTERFACE).build());

static EXTENSION_TEST_CLASS_ENTRY: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::class(EXTENSION_TEST_CLASS)
        .implements(&SAFE_HTML_INTERFACE)
        .method("__toString")
        .build()
});

/// `class Extension_TestClass implements Extension_SafeHtmlInterface` whose `__toString()` returns `<br />`.
pub struct ExtensionTestClass;

impl ExtensionTestClass {
    pub fn value() -> Value {
        Value::Object(ObjectRef::new(ExtensionTestClass))
    }
}

impl Object for ExtensionTestClass {
    fn class(&self) -> &ClassRef {
        &EXTENSION_TEST_CLASS_ENTRY
    }

    fn call(
        &self,
        _this: &ObjectRef,
        method: &str,
        _args: CallArgs,
        _diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        match method {
            "__tostring" => Ok(Value::from("<br />")),
            _ => Err(undefined(&EXTENSION_TEST_CLASS_ENTRY, method)),
        }
    }
}
