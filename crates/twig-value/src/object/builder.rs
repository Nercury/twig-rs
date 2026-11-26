//! [`ObjectBuilder`]: objects of a class made of public properties and methods implemented by closures, for host
//! data that needs methods but no hand-written [`Object`] implementation.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

use super::array_object::ArrayIterator;
use super::builtin;
use super::class::{ClassBuilder, ClassRef};
use super::props::PropertyStore;
use super::{CallArgs, FunctionKind, MethodParams, Object, ObjectRef, Param, undefined_method};
use crate::array::Array;
use crate::diag::Diagnostics;
use crate::error::{PhpError, PhpResult, ThrowableClass};
use crate::value::Value;

/// The body of a method of an [`ObjectBuilder`] object: it gets `$this`, the arguments bound to the method's
/// parameters (one value per parameter) and the diagnostics sink.
type MethodFn =
    dyn Fn(&ObjectRef, &[Value], &mut dyn Diagnostics) -> PhpResult<Value> + Send + Sync;

/// A method declared by an [`ObjectBuilder`].
#[derive(Clone)]
struct Method {
    /// The name as declared (methods are looked up case-insensitively).
    name: Arc<str>,
    /// `Class::name`, the function PHP names in argument errors.
    function: Arc<str>,
    params: Arc<[Param]>,
    body: Arc<MethodFn>,
}

/// The interfaces the hooks of an [`ObjectBuilder`] implement (`Stringable` comes with a `__toString()` method).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
struct Interfaces {
    iterator_aggregate: bool,
    array_access: bool,
    countable: bool,
}

/// What makes two builders declare the same class: its name and the names of its members.
#[derive(PartialEq, Eq, Hash)]
struct Shape {
    name: Arc<str>,
    properties: Vec<Arc<str>>,
    methods: Vec<Arc<str>>,
    interfaces: Interfaces,
}

/// The classes of built objects, one per [`Shape`]: the objects of one shape share their class entry, as the objects
/// of a PHP class do.
static CLASSES: LazyLock<Mutex<HashMap<Shape, ClassRef>>> = LazyLock::new(Default::default);

/// Builds an object of a PHP class made of public properties and of methods implemented by closures: what a PHP
/// application writes as a small class for its templates, without implementing [`Object`] by hand.
///
/// ```
/// use twig_value::object::{ObjectBuilder, Param};
/// use twig_value::{FetchMode, Ignore, Value, convert};
///
/// let user = ObjectBuilder::new("App\\User")
///     .property("name", "Fabien")
///     .method("isAdmin", [], |_, _, _| Ok(true))
///     .method("greet", [Param::optional("greeting", "Hello")], |this, args, diag| {
///         let name = this.read_property(b"name", FetchMode::Read, diag)?;
///         let greeting = convert::to_string(&args[0], diag)?;
///         Ok(format!("{} {}!", greeting.to_string_lossy(), convert::to_string(&name, diag)?.to_string_lossy()))
///     })
///     .stringable(|this, diag| this.read_property(b"name", FetchMode::Read, diag))
///     .build();
///
/// let mut diag = Ignore;
/// assert_eq!(user.class_name(), "App\\User");
/// assert_eq!(user.call_method("GREET", Default::default(), &mut diag)?, Value::from("Hello Fabien!"));
/// assert_eq!(user.to_php_string(&mut diag)?, "Fabien");
/// # Ok::<(), twig_value::PhpError>(())
/// ```
///
/// # PHP semantics
///
/// The object is an instance of this PHP class, with the methods in the order they were added and the closures as
/// their bodies:
///
/// ```php
/// namespace App;
///
/// class User implements \Stringable
/// {
///     public $name; // one public property per `property()`, set to its value
///
///     public function isAdmin() { /* ... */ }
///     public function greet($greeting = 'Hello') { /* ... */ }
///     public function __toString(): string { /* ... */ }
/// }
/// ```
///
/// Everything else follows from the class: the standard handlers of [`Object`] resolve properties and methods with
/// PHP's rules, so templates see the object exactly as Twig sees such a PHP object (`user.name` reads the property,
/// `user.admin` calls `isAdmin()`, method names are case-insensitive, `{{ user }}` calls `__toString()`). The
/// arguments of a call are bound to the declared parameters as PHP binds them for a userland method (positional and
/// named arguments, defaults, extra positional arguments ignored), with PHP's errors (`Unknown named parameter $x`;
/// `Too few arguments to function App\User::method(), 0 passed and exactly 1 expected`, without the calling file PHP
/// names). A method's closure gets the object itself (`$this`), one value per parameter (a variadic parameter gets an
/// array) and the diagnostics sink; it returns anything that converts into a [`Value`]. A clone (`clone $user`)
/// copies the properties and shares the closures.
///
/// The hooks implement PHP's interfaces: [`stringable`](Self::stringable) (`__toString()`),
/// [`countable`](Self::countable) (`Countable`), [`array_access`](Self::array_access) (`ArrayAccess`, read-only) and
/// [`iterable`](Self::iterable) (`IteratorAggregate`). Magic methods are ordinary methods: `__call($name,
/// $arguments)`, `__get($name)` and `__isset($name)` work like in PHP when added with [`method`](Self::method).
/// Adding a property or a method again replaces it (method names are compared case-insensitively). A builder is
/// cheap to clone: clones make objects that share the closures, each with its own properties.
///
/// Objects built with the same class name and the same members (property names, method names, hooks) are instances
/// of one class, as the objects of a PHP class are: they compare by their properties (`==`), and Twig's per-class
/// caches see one class. Class entries are kept for the life of the process, so property and method names belong in
/// code: data with arbitrary keys belongs in an [`Array`].
#[derive(Clone)]
pub struct ObjectBuilder {
    class: Arc<str>,
    properties: Vec<(Arc<str>, Value)>,
    methods: Vec<Method>,
    interfaces: Interfaces,
}

impl ObjectBuilder {
    /// An object of the class `class` (a leading `\` is ignored), without members yet.
    pub fn new(class: &str) -> ObjectBuilder {
        ObjectBuilder {
            class: Arc::from(class.strip_prefix('\\').unwrap_or(class)),
            properties: Vec::new(),
            methods: Vec::new(),
            interfaces: Interfaces::default(),
        }
    }

    /// A public property with its value (`public $name;`, set to `value`). Properties keep the order they were added
    /// in (`dump()`, `json_encode`).
    pub fn property(mut self, name: &str, value: impl Into<Value>) -> ObjectBuilder {
        let value = value.into();
        match self.properties.iter_mut().find(|(n, _)| **n == *name) {
            Some(property) => property.1 = value,
            None => self.properties.push((Arc::from(name), value)),
        }
        self
    }

    /// A public method (`public function name(params)`): `body` gets `$this`, one value per parameter (the argument
    /// given, else the parameter's default) and the diagnostics sink.
    pub fn method<R, F>(
        mut self,
        name: &str,
        params: impl IntoIterator<Item = Param>,
        body: F,
    ) -> ObjectBuilder
    where
        R: Into<Value>,
        F: Fn(&ObjectRef, &[Value], &mut dyn Diagnostics) -> PhpResult<R> + Send + Sync + 'static,
    {
        let method = Method {
            name: Arc::from(name),
            function: Arc::from(format!("{}::{name}", self.class)),
            params: params.into_iter().collect(),
            body: Arc::new(
                move |this: &ObjectRef, args: &[Value], diag: &mut dyn Diagnostics| {
                    body(this, args, diag).map(Into::into)
                },
            ),
        };
        match self
            .methods
            .iter_mut()
            .find(|m| m.name.eq_ignore_ascii_case(name))
        {
            Some(existing) => *existing = method,
            None => self.methods.push(method),
        }
        self
    }

    /// `__toString()`: what printing the object (`{{ object }}`), concatenation and the string filters use. PHP, with
    /// the closure as `$toString`:
    ///
    /// ```php
    /// public function __toString(): string { return $toString($this); }
    /// ```
    ///
    /// A result that is not a string is converted like PHP converts the return value of `__toString()` (ints, floats
    /// and booleans become strings, anything else is a `TypeError`).
    pub fn stringable<R, F>(self, to_string: F) -> ObjectBuilder
    where
        R: Into<Value>,
        F: Fn(&ObjectRef, &mut dyn Diagnostics) -> PhpResult<R> + Send + Sync + 'static,
    {
        self.method("__toString", [], move |this, _, diag| to_string(this, diag))
    }

    /// Makes the object `Countable`: `count()` is what the `length` filter returns. PHP, with the closure as `$count`:
    ///
    /// ```php
    /// class ... implements \Countable
    /// {
    ///     public function count(): int { return $count($this); }
    /// }
    /// ```
    pub fn countable<F>(mut self, count: F) -> ObjectBuilder
    where
        F: Fn(&ObjectRef, &mut dyn Diagnostics) -> PhpResult<i64> + Send + Sync + 'static,
    {
        self.interfaces.countable = true;
        self.method("count", [], move |this, _, diag| count(this, diag))
    }

    /// Makes the object `ArrayAccess`, read-only: `lookup` returns the value at an offset, `None` when there is
    /// none. Twig looks offsets up first for `object.name` and `object[key]`. The class implements `\ArrayAccess`
    /// with these methods:
    ///
    /// - `offsetExists($offset): bool`: whether `lookup` finds a value (a `null` value too);
    /// - `offsetGet($offset): mixed`: the value `lookup` finds, else `null`;
    /// - `offsetSet($offset, $value): void` and `offsetUnset($offset): void`: throw a `LogicException` with the
    ///   message `<class> is read-only.` (`App\Config is read-only.`).
    ///
    /// An offset is the PHP value the template gives, not normalised like an array key: `object.name` looks up
    /// `'name'`, `object[0]` the int `0`, `object['0']` the string `'0'` (Twig turns booleans and floats into ints).
    /// Adding `offsetSet` or `offsetUnset` with [`method`](Self::method) afterwards replaces the read-only one.
    pub fn array_access<F>(mut self, lookup: F) -> ObjectBuilder
    where
        F: Fn(&ObjectRef, &Value, &mut dyn Diagnostics) -> PhpResult<Option<Value>>
            + Send
            + Sync
            + 'static,
    {
        self.interfaces.array_access = true;
        let lookup = Arc::new(lookup);
        let exists = Arc::clone(&lookup);
        let read_only = || {
            let class = Arc::clone(&self.class);
            move |_: &ObjectRef, _: &[Value], _: &mut dyn Diagnostics| -> PhpResult<Value> {
                Err(PhpError::new(
                    ThrowableClass::LogicException,
                    format!("{class} is read-only."),
                ))
            }
        };
        let (set, unset) = (read_only(), read_only());
        self.method(
            "offsetExists",
            [Param::required("offset")],
            move |this, args, diag| Ok(exists(this, &args[0], diag)?.is_some()),
        )
        .method(
            "offsetGet",
            [Param::required("offset")],
            move |this, args, diag| Ok(lookup(this, &args[0], diag)?.unwrap_or_default()),
        )
        .method(
            "offsetSet",
            [Param::required("offset"), Param::required("value")],
            set,
        )
        .method("offsetUnset", [Param::required("offset")], unset)
    }

    /// Makes the object `IteratorAggregate` over the array `items` returns: `for` loops, `join`, `keys`, `first`,
    /// `last` and (unless the object is [`countable`](Self::countable)) `length` iterate it. PHP, with the closure as
    /// `$items`:
    ///
    /// ```php
    /// class ... implements \IteratorAggregate
    /// {
    ///     public function getIterator(): \Iterator { return new \ArrayIterator($items($this)); }
    /// }
    /// ```
    pub fn iterable<R, F>(mut self, items: F) -> ObjectBuilder
    where
        R: Into<Array>,
        F: Fn(&ObjectRef, &mut dyn Diagnostics) -> PhpResult<R> + Send + Sync + 'static,
    {
        self.interfaces.iterator_aggregate = true;
        self.method("getIterator", [], move |this, _, diag| {
            Ok(ArrayIterator::new(items(this, diag)?.into()).into_ref())
        })
    }

    /// Makes the object.
    pub fn build(self) -> ObjectRef {
        let class = self.class_entry();
        let properties = PropertyStore::new(&class);
        for (name, value) in self.properties {
            properties.set(name.as_bytes(), value);
        }
        ObjectRef::new(BuiltObject {
            class,
            methods: self.methods.into(),
            properties,
        })
    }

    /// The class of the object: the one of its [`Shape`], declared on first use.
    fn class_entry(&self) -> ClassRef {
        let shape = Shape {
            name: Arc::clone(&self.class),
            properties: self.properties.iter().map(|(n, _)| Arc::clone(n)).collect(),
            methods: self.methods.iter().map(|m| Arc::clone(&m.name)).collect(),
            interfaces: self.interfaces,
        };
        let mut classes = CLASSES.lock().unwrap_or_else(|e| e.into_inner());
        classes
            .entry(shape)
            .or_insert_with_key(|shape| {
                let mut class = ClassBuilder::class(&shape.name);
                if shape.interfaces.iterator_aggregate {
                    class = class.implements(&builtin::iterator_aggregate());
                }
                if shape.interfaces.array_access {
                    class = class.implements(&builtin::array_access());
                }
                if shape.interfaces.countable {
                    class = class.implements(&builtin::countable());
                }
                for name in &shape.properties {
                    class = class.property(name, Value::Null);
                }
                for name in &shape.methods {
                    class = class.method(name);
                }
                class.build()
            })
            .clone()
    }
}

/// An object made by an [`ObjectBuilder`].
struct BuiltObject {
    class: ClassRef,
    /// Shared with the clones of the object.
    methods: Arc<[Method]>,
    properties: PropertyStore,
}

impl BuiltObject {
    fn method(&self, name: &str) -> Option<&Method> {
        self.methods
            .iter()
            .find(|m| m.name.eq_ignore_ascii_case(name))
    }
}

impl Object for BuiltObject {
    fn class(&self) -> &ClassRef {
        &self.class
    }

    fn call(
        &self,
        this: &ObjectRef,
        method: &str,
        args: CallArgs,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        let Some(method) = self.method(method) else {
            return Err(undefined_method(&self.class, method));
        };
        let values = args.bind(&method.function, &method.params, FunctionKind::User)?;
        (method.body)(this, &values, diag)
    }

    fn method_params(&self, method: &str) -> Option<MethodParams> {
        self.method(method)
            .map(|method| MethodParams::user(method.params.to_vec()))
    }

    fn property_store(&self) -> Option<&PropertyStore> {
        Some(&self.properties)
    }

    /// `zend_objects_clone_obj()`: a copy of the properties; the methods are shared.
    fn clone_object(&self, _this: &ObjectRef, _diag: &mut dyn Diagnostics) -> PhpResult<ObjectRef> {
        Ok(ObjectRef::new(BuiltObject {
            class: Arc::clone(&self.class),
            methods: Arc::clone(&self.methods),
            properties: self.properties.clone(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::{FetchMode, iterator_to_array};
    use crate::{Ignore, PhpString, compare};

    fn user(name: &str) -> ObjectBuilder {
        ObjectBuilder::new("\\Test\\User")
            .property("name", name)
            .property("roles", vec!["admin"])
            .method("isAdmin", [], |_, _, _| Ok(true))
            .method(
                "greet",
                [Param::required("greeting"), Param::optional("end", "!")],
                |this, args, diag| {
                    let name = this.read_property(b"name", FetchMode::Read, diag)?;
                    Ok(PhpString::concat(&[
                        crate::convert::to_string(&args[0], diag)?.as_bytes(),
                        b" ",
                        crate::convert::to_string(&name, diag)?.as_bytes(),
                        crate::convert::to_string(&args[1], diag)?.as_bytes(),
                    ]))
                },
            )
    }

    fn names(methods: Vec<Arc<str>>) -> Vec<String> {
        methods.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn declares_a_class_of_public_members() {
        let object = user("Fabien").build();
        let class = object.class();
        assert_eq!(class.name(), "Test\\User");
        assert_eq!(names(class.public_method_names()), ["isAdmin", "greet"]);
        let properties: Vec<_> = class
            .instance_properties()
            .map(|p| p.name.to_string_lossy().into_owned())
            .collect();
        assert_eq!(properties, ["name", "roles"]);
        let mut diag = Ignore;
        let name = object.read_property(b"name", FetchMode::Read, &mut diag);
        assert_eq!(name.unwrap(), Value::from("Fabien"));
        assert!(!class.is_subclass_of("Stringable"));
    }

    #[test]
    fn calls_bind_arguments_like_a_userland_method() {
        let object = user("Fabien").build();
        let mut diag = Ignore;
        let mut call = |name: &str, args: CallArgs| object.call_method(name, args, &mut diag);
        assert_eq!(
            call("GREET", CallArgs::new().arg("Hi")).unwrap(),
            Value::from("Hi Fabien!")
        );
        let named = CallArgs::new()
            .named_arg("end", "?")
            .named_arg("greeting", "Yo");
        assert_eq!(call("greet", named).unwrap(), Value::from("Yo Fabien?"));
        // extra positional arguments are ignored, as for a userland method
        let extra = CallArgs::new().arg(1).arg(2);
        assert_eq!(call("isadmin", extra).unwrap(), Value::Bool(true));
        let error = call("greet", CallArgs::new()).unwrap_err();
        assert_eq!(error.class().name(), "ArgumentCountError");
        assert_eq!(
            error.message(),
            "Too few arguments to function Test\\User::greet(), 0 passed and at least 1 expected"
        );
        let error = call("greet", CallArgs::new().named_arg("nope", 1)).unwrap_err();
        assert_eq!(error.message(), "Unknown named parameter $nope");
        let error = call("missing", CallArgs::new()).unwrap_err();
        assert_eq!(
            error.message(),
            "Call to undefined method Test\\User::missing()"
        );
        let params = |params: Option<MethodParams>| {
            params.map(|p| p.params.iter().map(|p| p.name).collect::<Vec<_>>())
        };
        assert_eq!(
            params(object.method_params("Greet")),
            Some(vec!["greeting", "end"])
        );
        assert_eq!(params(object.method_params("isAdmin")), Some(vec![]));
    }

    #[test]
    fn objects_of_one_shape_share_their_class() {
        let a = Value::Object(user("a").build());
        let b = Value::Object(user("a").build());
        let c = Value::Object(user("c").build());
        let class = |value: &Value| match value {
            Value::Object(object) => object.class().clone(),
            _ => unreachable!(),
        };
        assert!(Arc::ptr_eq(&class(&a), &class(&b)));
        assert!(Arc::ptr_eq(&class(&a), &class(&c)));
        let mut diag = Ignore;
        // zend_std_compare_objects(): same class, compared by properties
        assert!(compare::equals(&a, &b, &mut diag).unwrap());
        assert!(!compare::equals(&a, &c, &mut diag).unwrap());
        // another member: another class, uncomparable
        let d = Value::Object(user("a").method("extra", [], |_, _, _| Ok(1)).build());
        assert!(!Arc::ptr_eq(&class(&a), &class(&d)));
        assert!(!compare::equals(&a, &d, &mut diag).unwrap());
    }

    #[test]
    fn later_members_replace_earlier_ones() {
        let object = user("a")
            .property("name", "b")
            .method("ISADMIN", [], |_, _, _| Ok(false))
            .build();
        let mut diag = Ignore;
        let name = object.read_property(b"name", FetchMode::Read, &mut diag);
        assert_eq!(name.unwrap(), Value::from("b"));
        let admin = object.call_method("isAdmin", CallArgs::new(), &mut diag);
        assert_eq!(admin.unwrap(), Value::Bool(false));
        assert_eq!(
            names(object.class().public_method_names()),
            ["ISADMIN", "greet"]
        );
    }

    #[test]
    fn hooks_implement_the_interfaces() {
        let items = || vec![Value::from("a"), Value::from("b")];
        let object = ObjectBuilder::new("Test\\Bag")
            .stringable(|_, _| Ok(5))
            .countable(|_, _| Ok(7))
            .array_access(|_, offset, _| {
                Ok(match offset {
                    Value::String(s) if s.as_bytes() == b"k" => Some(Value::from("v")),
                    Value::String(s) if s.as_bytes() == b"null" => Some(Value::Null),
                    _ => None,
                })
            })
            .iterable(move |_, _| Ok(items()))
            .build();
        let class = object.class();
        for interface in [
            "Stringable",
            "Countable",
            "ArrayAccess",
            "IteratorAggregate",
            "Traversable",
        ] {
            assert!(class.is_subclass_of(interface), "{interface}");
        }
        assert_eq!(
            names(class.public_method_names()),
            [
                "__toString",
                "count",
                "offsetExists",
                "offsetGet",
                "offsetSet",
                "offsetUnset",
                "getIterator"
            ]
        );
        let mut diag = Ignore;
        // `__toString(): string` coerces the int
        assert_eq!(object.to_php_string(&mut diag).unwrap(), "5");
        assert_eq!(object.count(&mut diag).unwrap(), Some(7));
        let key = Value::from("k");
        let value = object.read_dimension(&key, FetchMode::Read, &mut diag);
        assert_eq!(value.unwrap(), Value::from("v"));
        assert!(object.has_dimension(&key, false, &mut diag).unwrap());
        // found, but null: `isset()` asks offsetExists() only, `!empty()` reads the value too
        let null = Value::from("null");
        assert!(object.has_dimension(&null, false, &mut diag).unwrap());
        assert!(!object.has_dimension(&null, true, &mut diag).unwrap());
        assert!(
            !object
                .has_dimension(&Value::Int(0), false, &mut diag)
                .unwrap()
        );
        let error = object
            .write_dimension(Some(&key), Value::Null, &mut diag)
            .unwrap_err();
        assert_eq!(error.class().name(), "LogicException");
        assert_eq!(error.message(), "Test\\Bag is read-only.");
        let error = object.unset_dimension(&key, &mut diag).unwrap_err();
        assert_eq!(error.message(), "Test\\Bag is read-only.");
        let array = iterator_to_array(&object, true, &mut diag).unwrap();
        assert_eq!(array, Array::from(["a", "b"]));
    }

    #[test]
    fn to_string_results_follow_the_return_type() {
        let object = ObjectBuilder::new("Test\\Bad")
            .stringable(|_, _| Ok(Array::new()))
            .build();
        let error = object.to_php_string(&mut Ignore).unwrap_err();
        assert_eq!(error.class().name(), "TypeError");
        assert_eq!(
            error.message(),
            "Test\\Bad::__toString(): Return value must be of type string, array returned"
        );
    }

    #[test]
    fn clones_copy_the_properties() {
        let object = user("a").build();
        let mut diag = Ignore;
        let clone = object.clone_object(&mut diag).unwrap();
        assert!(!ObjectRef::ptr_eq(&object, &clone));
        assert!(Arc::ptr_eq(object.class(), clone.class()));
        clone
            .write_property(b"name", Value::from("b"), &mut diag)
            .unwrap();
        let name = object.read_property(b"name", FetchMode::Read, &mut diag);
        assert_eq!(name.unwrap(), Value::from("a"));
        let greeting = clone.call_method("greet", CallArgs::new().arg("Hi"), &mut diag);
        assert_eq!(greeting.unwrap(), Value::from("Hi b!"));
    }
}
