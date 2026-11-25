//! The userland PHP classes of the fixtures: the helper classes of Twig's
//! `tests/IntegrationTest.php` (namespace `Twig\Tests`) and the classes declared inline in
//! `--DATA--` sections.
//!
//! Every class is one [`UserObject`] kind: the class entry declares the properties and methods in
//! PHP's order (so `get_class_methods()`, `var_dump()` and property lookups match), the method
//! bodies are ported statement by statement below, each citing its upstream source.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

use twig_value::object::{
    ArrayIterator, CallArgs, ClassBuilder, ClassRef, FetchMode, Object, ObjectRef, Param,
    PropertyInfo, PropertyStore, Visibility, builtin,
};
use twig_value::param::{DeclaredType, UserArg, verify_user_arg};
use twig_value::{Array, Diagnostics, PhpError, PhpResult, PhpString, Value, convert, dim, ops};

use crate::php::{self, ArrayPointer, bind_user, verify_return};

/// Which class a [`UserObject`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum UserClass {
    /// `Twig\Tests\TwigTestFoo` (TWIG/tests/IntegrationTest.php:136-216).
    TwigTestFoo,
    /// `Twig\Tests\MagicCallStub` (TWIG/tests/IntegrationTest.php:382-388).
    MagicCallStub,
    /// `Twig\Tests\ToStringStub` (TWIG/tests/IntegrationTest.php:390-406).
    ToStringStub,
    /// `Twig\Tests\CountableStub` (TWIG/tests/IntegrationTest.php:413-431).
    CountableStub,
    /// `Twig\Tests\IteratorAggregateStub` (TWIG/tests/IntegrationTest.php:436-449).
    IteratorAggregateStub,
    /// `Twig\Tests\SimpleIteratorForTesting` (TWIG/tests/IntegrationTest.php:451-488).
    SimpleIteratorForTesting,
    /// `TwigTestStringy` (TWIG/tests/Fixtures/expressions/array_dynamic_keys.test).
    TwigTestStringy,
    /// `TwigTestHolder` (TWIG/tests/Fixtures/expressions/array_dynamic_keys.test).
    TwigTestHolder,
    /// `TestClassForMagicCallAttributes` (TWIG/tests/Fixtures/expressions/magic_call.test).
    TestClassForMagicCallAttributes,
    /// `MyObj` (TWIG/tests/Fixtures/regression/block-inheritance-issue.test).
    MyObj,
    /// `UserForAutoEscapeTest` (TWIG/tests/Fixtures/tags/autoescape/objects.test).
    UserForAutoEscapeTest,
    /// `ItemsIterator` (TWIG/tests/Fixtures/tags/for/objects.test).
    ItemsIterator,
    /// `ItemsIteratorCountable` (TWIG/tests/Fixtures/tags/for/objects_countable.test).
    ItemsIteratorCountable,
    /// `new class { public function getName() { return 'Fabien'; } }`
    /// (TWIG/tests/Fixtures/expressions/set.test).
    AnonymousGetName,
    /// `new class implements IteratorAggregate { public int $yielded = 0; ... }` with a generator
    /// `getIterator()` (TWIG/tests/Fixtures/expressions/set.test).
    AnonymousPairTraversable,
}

/// How a corpus tree without hidden properties (no supplement) can still produce the object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HiddenState {
    /// The class has no private/protected state, or only defaults no code can change.
    NotNeeded,
    /// The private string is what `__toString()` returns.
    FromToString(&'static str),
    /// The private state is only known from the supplement's `props_hidden`.
    Required,
}

impl UserClass {
    /// The class for a class name (anonymous classes by their display name).
    pub fn from_name(class: &str) -> Option<UserClass> {
        let display = class.split('\0').next().unwrap_or_default();
        Some(match display {
            "Twig\\Tests\\TwigTestFoo" => UserClass::TwigTestFoo,
            "Twig\\Tests\\MagicCallStub" => UserClass::MagicCallStub,
            "Twig\\Tests\\ToStringStub" => UserClass::ToStringStub,
            "Twig\\Tests\\CountableStub" => UserClass::CountableStub,
            "Twig\\Tests\\IteratorAggregateStub" => UserClass::IteratorAggregateStub,
            "Twig\\Tests\\SimpleIteratorForTesting" => UserClass::SimpleIteratorForTesting,
            "TwigTestStringy" => UserClass::TwigTestStringy,
            "TwigTestHolder" => UserClass::TwigTestHolder,
            "TestClassForMagicCallAttributes" => UserClass::TestClassForMagicCallAttributes,
            "MyObj" => UserClass::MyObj,
            "UserForAutoEscapeTest" => UserClass::UserForAutoEscapeTest,
            "ItemsIterator" => UserClass::ItemsIterator,
            "ItemsIteratorCountable" => UserClass::ItemsIteratorCountable,
            "class@anonymous" if class.contains('\0') => UserClass::AnonymousGetName,
            "IteratorAggregate@anonymous" if class.contains('\0') => {
                UserClass::AnonymousPairTraversable
            }
            _ => return None,
        })
    }

    /// Whether this is an anonymous class (its full name comes from the tree).
    pub fn is_anonymous(self) -> bool {
        matches!(
            self,
            UserClass::AnonymousGetName | UserClass::AnonymousPairTraversable
        )
    }

    /// Whether this class is declared by `tests/IntegrationTest.php` (always loaded in the harness),
    /// as opposed to an inline DATA declaration.
    pub fn is_harness_class(self) -> bool {
        matches!(
            self,
            UserClass::TwigTestFoo
                | UserClass::MagicCallStub
                | UserClass::ToStringStub
                | UserClass::CountableStub
                | UserClass::IteratorAggregateStub
                | UserClass::SimpleIteratorForTesting
        )
    }

    pub(crate) fn hidden_state(self) -> HiddenState {
        match self {
            UserClass::ToStringStub => HiddenState::FromToString("string"),
            UserClass::TwigTestStringy => HiddenState::FromToString("v"),
            UserClass::CountableStub
            | UserClass::IteratorAggregateStub
            | UserClass::SimpleIteratorForTesting => HiddenState::Required,
            _ => HiddenState::NotNeeded,
        }
    }

    /// The class entry. Anonymous classes are named after `name` (PHP's generated name, with the
    /// NUL-separated location suffix); other classes ignore it.
    pub fn class_entry(self, name: &str) -> ClassRef {
        static ANONYMOUS: LazyLock<Mutex<HashMap<(UserClass, String), ClassRef>>> =
            LazyLock::new(|| Mutex::new(HashMap::new()));
        if self.is_anonymous() {
            let mut cache = ANONYMOUS.lock().unwrap_or_else(|e| e.into_inner());
            return cache
                .entry((self, name.to_string()))
                .or_insert_with(|| build_class(self, name))
                .clone();
        }
        static NAMED: LazyLock<Mutex<HashMap<UserClass, ClassRef>>> =
            LazyLock::new(|| Mutex::new(HashMap::new()));
        let mut cache = NAMED.lock().unwrap_or_else(|e| e.into_inner());
        cache
            .entry(self)
            .or_insert_with(|| build_class(self, ""))
            .clone()
    }
}

fn typed_property(
    name: &str,
    visibility: Visibility,
    type_name: Option<&str>,
    default: Option<Value>,
) -> PropertyInfo {
    PropertyInfo {
        name: PhpString::from(name),
        visibility,
        is_static: false,
        is_readonly: false,
        type_name: type_name.map(Arc::from),
        default,
        declaring_class: Arc::from(""),
    }
}

fn methods(mut b: ClassBuilder, names: &[&str]) -> ClassBuilder {
    for m in names {
        b = b.method(m);
    }
    b
}

/// The class declarations, members in source order. PHP lists a class's interfaces in the order
/// "declared ones, then the interfaces they extend"; `ClassBuilder` puts each interface's parents
/// right after it, so `class_implements()` differs in order (not in content) for
/// `SimpleIteratorForTesting` and `ItemsIteratorCountable` (reference/deviations.md).
fn build_class(kind: UserClass, anonymous_name: &str) -> ClassRef {
    match kind {
        UserClass::TwigTestFoo => {
            let b = ClassBuilder::class("Twig\\Tests\\TwigTestFoo")
                .implements(&builtin::iterator())
                .constant("BAR_NAME", "bar")
                .property("position", 0)
                .property("array", Array::from_values([Value::Int(1), Value::Int(2)]))
                .property_info(PropertyInfo {
                    is_static: true,
                    ..typed_property("foo", Visibility::Public, None, Some(Value::from("Foo")))
                });
            methods(
                b,
                &[
                    "bar",
                    "getFoo",
                    "getEmpty",
                    "getNull",
                    "getSelf",
                    "is",
                    "in",
                    "not",
                    "strToLower",
                    "rewind",
                    "current",
                    "key",
                    "next",
                    "valid",
                ],
            )
            .build()
        }
        UserClass::MagicCallStub => ClassBuilder::class("Twig\\Tests\\MagicCallStub")
            .method("__call")
            .build(),
        UserClass::ToStringStub => ClassBuilder::class("Twig\\Tests\\ToStringStub")
            .property_info(typed_property(
                "string",
                Visibility::Private,
                None,
                Some(Value::Null),
            ))
            .method("__construct")
            .method("__toString")
            .build(),
        UserClass::CountableStub => ClassBuilder::class("Twig\\Tests\\CountableStub")
            .implements(&builtin::countable())
            .property_info(typed_property(
                "count",
                Visibility::Private,
                None,
                Some(Value::Null),
            ))
            .method("__construct")
            .method("count")
            .method("__toString")
            .build(),
        UserClass::IteratorAggregateStub => {
            ClassBuilder::class("Twig\\Tests\\IteratorAggregateStub")
                .implements(&builtin::iterator_aggregate())
                .property_info(typed_property(
                    "data",
                    Visibility::Private,
                    None,
                    Some(Value::Null),
                ))
                .method("__construct")
                .method("getIterator")
                .build()
        }
        UserClass::SimpleIteratorForTesting => {
            let data = Array::from_values((1..=7).map(Value::Int));
            let b = ClassBuilder::class("Twig\\Tests\\SimpleIteratorForTesting")
                .implements(&builtin::iterator())
                .property_info(typed_property(
                    "data",
                    Visibility::Private,
                    None,
                    Some(Value::Array(data)),
                ))
                .property_info(typed_property(
                    "key",
                    Visibility::Private,
                    None,
                    Some(Value::Int(0)),
                ));
            methods(
                b,
                &["current", "next", "key", "valid", "rewind", "__toString"],
            )
            .build()
        }
        UserClass::TwigTestStringy => ClassBuilder::class("TwigTestStringy")
            .implements(&builtin::stringable())
            .property_info(typed_property(
                "v",
                Visibility::Private,
                Some("string"),
                None,
            ))
            .method("__construct")
            .method("__toString")
            .build(),
        UserClass::TwigTestHolder => ClassBuilder::class("TwigTestHolder")
            .property_info(typed_property(
                "name",
                Visibility::Public,
                Some("string"),
                Some(Value::from("attr")),
            ))
            .property_info(typed_property(
                "stringable",
                Visibility::Public,
                Some("TwigTestStringy"),
                None,
            ))
            .method("__construct")
            .method("getStringable")
            .build(),
        UserClass::TestClassForMagicCallAttributes => {
            ClassBuilder::class("TestClassForMagicCallAttributes")
                .method("getBar")
                .method("__call")
                .build()
        }
        UserClass::MyObj => ClassBuilder::class("MyObj").method("__toString").build(),
        UserClass::UserForAutoEscapeTest => ClassBuilder::class("UserForAutoEscapeTest")
            .method("getName")
            .method("__toString")
            .build(),
        UserClass::ItemsIterator | UserClass::ItemsIteratorCountable => {
            let countable = kind == UserClass::ItemsIteratorCountable;
            let mut values = Array::new();
            values.insert("foo", "bar");
            values.insert("bar", "foo");
            let mut b = ClassBuilder::class(if countable {
                "ItemsIteratorCountable"
            } else {
                "ItemsIterator"
            })
            .implements(&builtin::iterator());
            if countable {
                b = b.implements(&builtin::countable());
            }
            b = b.property_info(typed_property(
                "values",
                Visibility::Protected,
                None,
                Some(Value::Array(values)),
            ));
            b = methods(b, &["current", "key", "next", "rewind", "valid"]);
            if countable {
                b = b.method("count");
            }
            b.build()
        }
        UserClass::AnonymousGetName => ClassBuilder::class(anonymous_name)
            .anonymous()
            .method("getName")
            .build(),
        UserClass::AnonymousPairTraversable => ClassBuilder::class(anonymous_name)
            .anonymous()
            .implements(&builtin::iterator_aggregate())
            .property_info(typed_property(
                "yielded",
                Visibility::Public,
                Some("int"),
                Some(Value::Int(0)),
            ))
            .method("getIterator")
            .build(),
    }
}

/// An object of one of the [`UserClass`]es: the standard property handlers over a
/// [`PropertyStore`] plus the ported method bodies.
pub struct UserObject {
    class: ClassRef,
    kind: UserClass,
    props: PropertyStore,
    /// `ItemsIterator`: the internal pointer of the `$values` array (`current()`/`next()`/...).
    pointer: Mutex<ArrayPointer>,
}

impl UserObject {
    /// A new object with the class's default property values (no constructor call), like
    /// `ReflectionClass::newInstanceWithoutConstructor()`.
    pub fn new_without_constructor(kind: UserClass, class_name: &str) -> UserObject {
        let class = kind.class_entry(class_name);
        let props = PropertyStore::new(&class);
        UserObject {
            class,
            kind,
            props,
            pointer: Mutex::new(ArrayPointer::default()),
        }
    }

    /// Which class this is.
    pub fn kind(&self) -> UserClass {
        self.kind
    }

    /// The property storage (for the object's own code: no visibility checks).
    pub fn properties(&self) -> &PropertyStore {
        &self.props
    }

    fn pointer(&self) -> std::sync::MutexGuard<'_, ArrayPointer> {
        self.pointer.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// `$this->name` from the class's own scope: the value, `Error` for an uninitialised typed
    /// property, a warning and `null` for a missing untyped one.
    fn own(&self, name: &str, diag: &mut dyn Diagnostics) -> PhpResult<Value> {
        if let Some(v) = self.props.get(name.as_bytes()) {
            return Ok(v);
        }
        match self.class.property(name.as_bytes()) {
            Some(p) if p.type_name.is_some() => Err(PhpError::error(format!(
                "Typed property {}::${} must not be accessed before initialization",
                self.class.display_name(),
                name
            ))),
            _ => {
                diag.warn(format!(
                    "Undefined property: {}::${}",
                    self.class.display_name(),
                    name
                ))?;
                Ok(Value::Null)
            }
        }
    }

    fn set(&self, name: &str, value: Value) {
        self.props.set(name.as_bytes(), value);
    }

    /// `Class::method` as PHP prints it in messages (anonymous classes up to the NUL).
    fn function(&self, method: &str) -> String {
        format!("{}::{}", self.class.display_name(), method)
    }
}

impl Object for UserObject {
    fn class(&self) -> &ClassRef {
        &self.class
    }

    fn property_store(&self) -> Option<&PropertyStore> {
        Some(&self.props)
    }

    fn call(
        &self,
        this: &ObjectRef,
        method: &str,
        args: CallArgs,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        match self.kind {
            UserClass::TwigTestFoo => twig_test_foo(self, this, method, &args, diag),
            UserClass::MagicCallStub => magic_call_stub(self, method, &args),
            UserClass::ToStringStub => to_string_stub(self, method, &args, diag),
            UserClass::CountableStub => countable_stub(self, method, &args, diag),
            UserClass::IteratorAggregateStub => iterator_aggregate_stub(self, method, &args, diag),
            UserClass::SimpleIteratorForTesting => {
                simple_iterator_for_testing(self, this, method, &args, diag)
            }
            UserClass::TwigTestStringy => twig_test_stringy(self, method, &args, diag),
            UserClass::TwigTestHolder => twig_test_holder(self, method, &args, diag),
            UserClass::TestClassForMagicCallAttributes => {
                magic_call_attributes(self, method, &args)
            }
            UserClass::MyObj => my_obj(self, method, &args),
            UserClass::UserForAutoEscapeTest => user_for_auto_escape(self, method, &args),
            UserClass::ItemsIterator | UserClass::ItemsIteratorCountable => {
                items_iterator(self, method, &args, diag)
            }
            UserClass::AnonymousGetName => anonymous_get_name(self, method, &args),
            UserClass::AnonymousPairTraversable => {
                anonymous_pair_traversable(self, this, method, &args)
            }
        }
    }

    /// `zend_objects_clone_obj()`: a shallow copy of the properties (and of the array pointer, which
    /// lives in the copied property array).
    fn clone_object(&self, _this: &ObjectRef, _diag: &mut dyn Diagnostics) -> PhpResult<ObjectRef> {
        Ok(ObjectRef::new(UserObject {
            class: self.class.clone(),
            kind: self.kind,
            props: self.props.clone(),
            pointer: Mutex::new(self.pointer().clone()),
        }))
    }
}

fn undefined(o: &UserObject, method: &str) -> PhpError {
    twig_value::object::undefined_method(&o.class, method)
}

/// Binds a method without parameters (unknown named arguments still throw).
fn no_params(o: &UserObject, name: &str, args: &CallArgs) -> PhpResult<()> {
    bind_user(args, &o.function(name), &[]).map(|_| ())
}

fn concat(a: &Value, b: &Value, diag: &mut dyn Diagnostics) -> PhpResult<Value> {
    ops::concat(a, b, diag).map(Value::String)
}

/// `Twig\Tests\TwigTestFoo` (TWIG/tests/IntegrationTest.php:136-216).
fn twig_test_foo(
    o: &UserObject,
    this: &ObjectRef,
    method: &str,
    args: &CallArgs,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Value> {
    match method {
        // return 'bar'.($param1 ? '_'.$param1 : '').($param2 ? '-'.$param2 : '');
        "bar" => {
            let a = bind_user(
                args,
                &o.function("bar"),
                &[
                    Param::optional("param1", Value::Null),
                    Param::optional("param2", Value::Null),
                ],
            )?;
            let first = if convert::to_bool(&a[0], diag)? {
                concat(&Value::from("_"), &a[0], diag)?
            } else {
                Value::from("")
            };
            let head = concat(&Value::from("bar"), &first, diag)?;
            let second = if convert::to_bool(&a[1], diag)? {
                concat(&Value::from("-"), &a[1], diag)?
            } else {
                Value::from("")
            };
            concat(&head, &second, diag)
        }
        "getfoo" => no_params(o, "getFoo", args).map(|_| Value::from("foo")),
        "getempty" => no_params(o, "getEmpty", args).map(|_| Value::from("")),
        "getnull" => no_params(o, "getNull", args).map(|_| Value::Null),
        "getself" => no_params(o, "getSelf", args).map(|_| Value::Object(this.clone())),
        "is" => no_params(o, "is", args).map(|_| Value::from("is")),
        "in" => no_params(o, "in", args).map(|_| Value::from("in")),
        "not" => no_params(o, "not", args).map(|_| Value::from("not")),
        // return strtolower($value);
        "strtolower" => {
            let a = bind_user(args, &o.function("strToLower"), &[Param::required("value")])?;
            twig_value::builtins::string::strtolower(&a[0], diag).map(Value::String)
        }
        // $this->position = 0;
        "rewind" => {
            no_params(o, "rewind", args)?;
            o.set("position", Value::Int(0));
            Ok(Value::Null)
        }
        // return $this->array[$this->position];
        "current" => {
            no_params(o, "current", args)?;
            let array = o.own("array", diag)?;
            let position = o.own("position", diag)?;
            dim::read(&array, &position, FetchMode::Read, diag)
        }
        "key" => no_params(o, "key", args).map(|_| Value::from("a")),
        // ++$this->position;
        "next" => {
            no_params(o, "next", args)?;
            let position = o.own("position", diag)?;
            o.set("position", php::increment(&position, diag)?);
            Ok(Value::Null)
        }
        // return isset($this->array[$this->position]);
        "valid" => {
            no_params(o, "valid", args)?;
            let array = o.own("array", diag)?;
            let position = o.own("position", diag)?;
            dim::isset(&array, &position, diag).map(Value::Bool)
        }
        _ => Err(undefined(o, method)),
    }
}

/// `Twig\Tests\MagicCallStub` (TWIG/tests/IntegrationTest.php:382-388).
fn magic_call_stub(o: &UserObject, method: &str, args: &CallArgs) -> PhpResult<Value> {
    match method {
        // throw new \Exception('__call shall not be called.');
        "__call" => {
            bind_user(
                args,
                &o.function("__call"),
                &[Param::required("name"), Param::required("args")],
            )?;
            Err(PhpError::exception("__call shall not be called."))
        }
        _ => Err(undefined(o, method)),
    }
}

/// `Twig\Tests\ToStringStub` (TWIG/tests/IntegrationTest.php:390-406).
fn to_string_stub(
    o: &UserObject,
    method: &str,
    args: &CallArgs,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Value> {
    match method {
        // $this->string = $string;
        "__construct" => {
            let a = bind_user(
                args,
                &o.function("__construct"),
                &[Param::required("string")],
            )?;
            o.set("string", a[0].clone());
            Ok(Value::Null)
        }
        // return $this->string;  (implicit `: string` of __toString())
        "__tostring" => {
            no_params(o, "__toString", args)?;
            let v = o.own("string", diag)?;
            verify_return(v, &php::STRING, &o.function("__toString"), diag)
        }
        _ => Err(undefined(o, method)),
    }
}

/// `Twig\Tests\CountableStub` (TWIG/tests/IntegrationTest.php:413-431).
fn countable_stub(
    o: &UserObject,
    method: &str,
    args: &CallArgs,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Value> {
    match method {
        "__construct" => {
            let a = bind_user(
                args,
                &o.function("__construct"),
                &[Param::required("count")],
            )?;
            o.set("count", a[0].clone());
            Ok(Value::Null)
        }
        // public function count(): int { return $this->count; }
        "count" => {
            no_params(o, "count", args)?;
            let v = o.own("count", diag)?;
            verify_return(v, &php::INT, &o.function("count"), diag)
        }
        // throw new \Exception('__toString shall not be called on \Countables.');
        "__tostring" => {
            no_params(o, "__toString", args)?;
            Err(PhpError::exception(
                "__toString shall not be called on \\Countables.",
            ))
        }
        _ => Err(undefined(o, method)),
    }
}

/// `Twig\Tests\IteratorAggregateStub` (TWIG/tests/IntegrationTest.php:436-449).
fn iterator_aggregate_stub(
    o: &UserObject,
    method: &str,
    args: &CallArgs,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Value> {
    match method {
        // public function __construct(array $data) { $this->data = $data; }
        "__construct" => {
            let f = o.function("__construct");
            let a = bind_user(args, &f, &[Param::required("data")])?;
            let data = verify_user_arg(
                a[0].clone(),
                &php::ARRAY,
                &UserArg::new(&f, 1, "data"),
                diag,
            )?;
            o.set("data", data);
            Ok(Value::Null)
        }
        // public function getIterator(): \Traversable { return new \ArrayIterator($this->data); }
        "getiterator" => {
            no_params(o, "getIterator", args)?;
            let data = o.own("data", diag)?;
            let it = ArrayIterator::new(Array::new()).into_ref();
            it.invoke_method("__construct", CallArgs::from(vec![data]), diag)?;
            verify_return(
                Value::Object(it),
                &DeclaredType::new(&["Traversable"], twig_value::param::TypeMask::NONE),
                &o.function("getIterator"),
                diag,
            )
        }
        _ => Err(undefined(o, method)),
    }
}

/// `Twig\Tests\SimpleIteratorForTesting` (TWIG/tests/IntegrationTest.php:451-488).
fn simple_iterator_for_testing(
    o: &UserObject,
    this: &ObjectRef,
    method: &str,
    args: &CallArgs,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Value> {
    match method {
        // return $this->key;
        "current" => {
            no_params(o, "current", args)?;
            o.own("key", diag)
        }
        // ++$this->key;
        "next" => {
            no_params(o, "next", args)?;
            let key = o.own("key", diag)?;
            o.set("key", php::increment(&key, diag)?);
            Ok(Value::Null)
        }
        "key" => {
            no_params(o, "key", args)?;
            o.own("key", diag)
        }
        // return isset($this->data[$this->key]);
        "valid" => {
            no_params(o, "valid", args)?;
            let data = o.own("data", diag)?;
            let key = o.own("key", diag)?;
            dim::isset(&data, &key, diag).map(Value::Bool)
        }
        "rewind" => {
            no_params(o, "rewind", args)?;
            o.set("key", Value::Int(0));
            Ok(Value::Null)
        }
        // return str_repeat('X', iterator_count($this) + 10);
        "__tostring" => {
            no_params(o, "__toString", args)?;
            let count = twig_value::object::iterator_count(this, diag)?;
            let times = ops::add(&Value::Int(count), &Value::Int(10), diag)?;
            let s = twig_value::builtins::string::str_repeat(&Value::from("X"), &times, diag)?;
            verify_return(
                Value::String(s),
                &php::STRING,
                &o.function("__toString"),
                diag,
            )
        }
        _ => Err(undefined(o, method)),
    }
}

/// `class TwigTestStringy implements \Stringable` (expressions/array_dynamic_keys.test).
fn twig_test_stringy(
    o: &UserObject,
    method: &str,
    args: &CallArgs,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Value> {
    match method {
        // public function __construct(private string $v) {}
        "__construct" => {
            let f = o.function("__construct");
            let a = bind_user(args, &f, &[Param::required("v")])?;
            let v = verify_user_arg(a[0].clone(), &php::STRING, &UserArg::new(&f, 1, "v"), diag)?;
            o.set("v", v);
            Ok(Value::Null)
        }
        // public function __toString(): string { return $this->v; }
        "__tostring" => {
            no_params(o, "__toString", args)?;
            let v = o.own("v", diag)?;
            verify_return(v, &php::STRING, &o.function("__toString"), diag)
        }
        _ => Err(undefined(o, method)),
    }
}

/// A `TwigTestStringy` built by its constructor.
pub fn twig_test_stringy_new(v: &str) -> ObjectRef {
    let obj = UserObject::new_without_constructor(UserClass::TwigTestStringy, "");
    obj.set("v", Value::from(v));
    ObjectRef::new(obj)
}

/// `class TwigTestHolder` (expressions/array_dynamic_keys.test).
fn twig_test_holder(
    o: &UserObject,
    method: &str,
    args: &CallArgs,
    _diag: &mut dyn Diagnostics,
) -> PhpResult<Value> {
    match method {
        // public function __construct() { $this->stringable = new TwigTestStringy('attr_obj'); }
        "__construct" => {
            no_params(o, "__construct", args)?;
            o.set(
                "stringable",
                Value::Object(twig_test_stringy_new("attr_obj")),
            );
            Ok(Value::Null)
        }
        // public function getStringable(): TwigTestStringy { return new TwigTestStringy('method_obj'); }
        "getstringable" => {
            no_params(o, "getStringable", args)?;
            Ok(Value::Object(twig_test_stringy_new("method_obj")))
        }
        _ => Err(undefined(o, method)),
    }
}

/// `class TestClassForMagicCallAttributes` (expressions/magic_call.test).
fn magic_call_attributes(o: &UserObject, method: &str, args: &CallArgs) -> PhpResult<Value> {
    match method {
        "getbar" => no_params(o, "getBar", args).map(|_| Value::from("bar_from_getbar")),
        // if ('foo' === $method) { return 'foo_from_call'; } return false;
        "__call" => {
            let a = bind_user(
                args,
                &o.function("__call"),
                &[Param::required("method"), Param::required("arguments")],
            )?;
            Ok(if a[0] == Value::from("foo") {
                Value::from("foo_from_call")
            } else {
                Value::Bool(false)
            })
        }
        _ => Err(undefined(o, method)),
    }
}

/// `class MyObj` (regression/block-inheritance-issue.test).
fn my_obj(o: &UserObject, method: &str, args: &CallArgs) -> PhpResult<Value> {
    match method {
        "__tostring" => no_params(o, "__toString", args).map(|_| Value::from("foo")),
        _ => Err(undefined(o, method)),
    }
}

/// `class UserForAutoEscapeTest` (tags/autoescape/objects.test).
fn user_for_auto_escape(o: &UserObject, method: &str, args: &CallArgs) -> PhpResult<Value> {
    match method {
        "getname" => no_params(o, "getName", args).map(|_| Value::from("Fabien<br />")),
        "__tostring" => no_params(o, "__toString", args).map(|_| Value::from("Fabien<br />")),
        _ => Err(undefined(o, method)),
    }
}

/// `class ItemsIterator implements \Iterator` (tags/for/objects.test) and
/// `ItemsIteratorCountable implements \Iterator, \Countable` (tags/for/objects_countable.test):
/// iteration through the internal pointer of the protected `$values` array.
fn items_iterator(
    o: &UserObject,
    method: &str,
    args: &CallArgs,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Value> {
    // `$values` is protected and only ever the default array; anything else is outside the port.
    let values = |diag: &mut dyn Diagnostics, f: &str| -> PhpResult<Array> {
        match o.own("values", diag)? {
            Value::Array(a) => Ok(a),
            _ => Err(PhpError::error(format!(
                "{}(): {}::$values is not an array (not supported by twig-test-support)",
                f,
                o.class.display_name()
            ))),
        }
    };
    match method {
        // return current($this->values);
        "current" => {
            no_params(o, "current", args)?;
            let a = values(diag, "current")?;
            Ok(o.pointer().current(&a))
        }
        // return key($this->values);
        "key" => {
            no_params(o, "key", args)?;
            let a = values(diag, "key")?;
            Ok(o.pointer().key(&a))
        }
        // next($this->values);
        "next" => {
            no_params(o, "next", args)?;
            let a = values(diag, "next")?;
            o.pointer().next(&a);
            Ok(Value::Null)
        }
        // reset($this->values);
        "rewind" => {
            no_params(o, "rewind", args)?;
            values(diag, "reset")?;
            o.pointer().reset();
            Ok(Value::Null)
        }
        // return false !== current($this->values);
        "valid" => {
            no_params(o, "valid", args)?;
            let a = values(diag, "current")?;
            Ok(Value::Bool(o.pointer().current(&a) != Value::Bool(false)))
        }
        // public function count(): int { return count($this->values); }
        "count" if o.kind == UserClass::ItemsIteratorCountable => {
            no_params(o, "count", args)?;
            let v = o.own("values", diag)?;
            let n = dim::count(&v, 0, diag)?;
            Ok(Value::Int(n))
        }
        _ => Err(undefined(o, method)),
    }
}

/// `new class { public function getName() { return 'Fabien'; } }` (expressions/set.test).
fn anonymous_get_name(o: &UserObject, method: &str, args: &CallArgs) -> PhpResult<Value> {
    match method {
        "getname" => no_params(o, "getName", args).map(|_| Value::from("Fabien")),
        _ => Err(undefined(o, method)),
    }
}

/// `new class implements IteratorAggregate` (expressions/set.test):
///
/// ```php
/// public int $yielded = 0;
/// public function getIterator(): Traversable
/// {
///     foreach ([5 => 'Fabien', 8 => 'Potencier', 13 => 'Ignored'] as $key => $value) {
///         ++$this->yielded;
///         yield $key => $value;
///     }
/// }
/// ```
fn anonymous_pair_traversable(
    o: &UserObject,
    this: &ObjectRef,
    method: &str,
    args: &CallArgs,
) -> PhpResult<Value> {
    match method {
        "getiterator" => {
            no_params(o, "getIterator", args)?;
            let target = this.clone();
            let class_name = o.class.name().to_string();
            let display = o.class.display_name().to_string();
            let items: [(i64, &'static str); 3] =
                [(5, "Fabien"), (8, "Potencier"), (13, "Ignored")];
            let mut next = 0usize;
            let step = move |diag: &mut dyn Diagnostics| -> PhpResult<Option<(Value, Value)>> {
                let Some(&(key, value)) = items.get(next) else {
                    return Ok(None);
                };
                next += 1;
                let Some(obj) = target.downcast_ref::<UserObject>() else {
                    return Err(PhpError::error("generator target is not a UserObject"));
                };
                let yielded = obj.own("yielded", diag)?;
                obj.set(
                    "yielded",
                    php::increment_typed_int(&yielded, &display, "yielded", diag)?,
                );
                Ok(Some((Value::Int(key), Value::from(value))))
            };
            let generator = super::generator::Generator::new(
                format!("{class_name}::getIterator"),
                Box::new(step),
            );
            Ok(Value::Object(generator.into_ref()))
        }
        _ => Err(undefined(o, method)),
    }
}
