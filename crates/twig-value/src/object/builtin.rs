//! Built-in class entries (core interfaces, `stdClass`, `Closure`, `ArrayObject`,
//! `ArrayIterator`, `RecursiveArrayIterator`) and a case-insensitive class table.

use std::collections::HashMap;
use std::sync::LazyLock;

use super::class::{ClassBuilder, ClassRef, Visibility};
use crate::error::{PhpError, PhpResult};
use crate::value::Value;

macro_rules! lazy_class {
    ($(#[$doc:meta])* $fn_name:ident, $static_name:ident, $init:expr) => {
        static $static_name: LazyLock<ClassRef> = LazyLock::new(|| $init);
        $(#[$doc])*
        pub fn $fn_name() -> ClassRef {
            $static_name.clone()
        }
    };
}

lazy_class!(
    /// `interface Traversable`.
    traversable,
    TRAVERSABLE,
    ClassBuilder::interface("Traversable").build()
);
lazy_class!(
    /// `interface Iterator extends Traversable`.
    iterator,
    ITERATOR,
    ClassBuilder::interface("Iterator")
        .implements(&traversable())
        .method("current")
        .method("next")
        .method("key")
        .method("valid")
        .method("rewind")
        .build()
);
lazy_class!(
    /// `interface IteratorAggregate extends Traversable`.
    iterator_aggregate,
    ITERATOR_AGGREGATE,
    ClassBuilder::interface("IteratorAggregate")
        .implements(&traversable())
        .method("getIterator")
        .build()
);
lazy_class!(
    /// `interface ArrayAccess`.
    array_access,
    ARRAY_ACCESS,
    ClassBuilder::interface("ArrayAccess")
        .method("offsetExists")
        .method("offsetGet")
        .method("offsetSet")
        .method("offsetUnset")
        .build()
);
lazy_class!(
    /// `interface Countable`.
    countable,
    COUNTABLE,
    ClassBuilder::interface("Countable").method("count").build()
);
lazy_class!(
    /// `interface Stringable` (added automatically to classes declaring `__toString`).
    stringable,
    STRINGABLE,
    ClassBuilder::interface("Stringable")
        .method("__toString")
        .build()
);
lazy_class!(
    /// `interface JsonSerializable`.
    json_serializable,
    JSON_SERIALIZABLE,
    ClassBuilder::interface("JsonSerializable")
        .method("jsonSerialize")
        .build()
);
lazy_class!(
    /// `interface Serializable`.
    serializable,
    SERIALIZABLE,
    ClassBuilder::interface("Serializable")
        .method("serialize")
        .method("unserialize")
        .build()
);
lazy_class!(
    /// `interface SeekableIterator extends Iterator`.
    seekable_iterator,
    SEEKABLE_ITERATOR,
    ClassBuilder::interface("SeekableIterator")
        .implements(&iterator())
        .method("seek")
        .build()
);
lazy_class!(
    /// `interface RecursiveIterator extends Iterator`.
    recursive_iterator,
    RECURSIVE_ITERATOR,
    ClassBuilder::interface("RecursiveIterator")
        .implements(&iterator())
        .method("hasChildren")
        .method("getChildren")
        .build()
);
lazy_class!(
    /// `interface UnitEnum`.
    unit_enum,
    UNIT_ENUM,
    ClassBuilder::interface("UnitEnum")
        .method_with("cases", Visibility::Public, true)
        .build()
);
lazy_class!(
    /// `interface BackedEnum extends UnitEnum`.
    backed_enum,
    BACKED_ENUM,
    ClassBuilder::interface("BackedEnum")
        .implements(&unit_enum())
        .method_with("from", Visibility::Public, true)
        .method_with("tryFrom", Visibility::Public, true)
        .build()
);
lazy_class!(
    /// `class stdClass` (`#[AllowDynamicProperties]`).
    std_class,
    STD_CLASS,
    ClassBuilder::class("stdClass")
        .allow_dynamic_properties()
        .build()
);
lazy_class!(
    /// `final class Closure`.
    closure,
    CLOSURE,
    ClassBuilder::class("Closure")
        .final_class()
        .method_with("bind", Visibility::Public, true)
        .method("bindTo")
        .method("call")
        .method_with("fromCallable", Visibility::Public, true)
        .method_with("getCurrent", Visibility::Public, true)
        .build()
);

/// Methods shared by `ArrayObject` and `ArrayIterator` before their specific ones.
const ARRAY_COMMON_METHODS: &[&str] = &[
    "__construct",
    "offsetExists",
    "offsetGet",
    "offsetSet",
    "offsetUnset",
    "append",
    "getArrayCopy",
    "count",
    "getFlags",
    "setFlags",
    "asort",
    "ksort",
    "uasort",
    "uksort",
    "natsort",
    "natcasesort",
    "unserialize",
    "serialize",
    "__serialize",
    "__unserialize",
];

lazy_class!(
    /// `class ArrayObject implements IteratorAggregate, ArrayAccess, Serializable, Countable`.
    array_object,
    ARRAY_OBJECT,
    {
        let mut b = ClassBuilder::class("ArrayObject")
            .implements(&iterator_aggregate())
            .implements(&array_access())
            .implements(&serializable())
            .implements(&countable())
            .constant("STD_PROP_LIST", Value::Int(1))
            .constant("ARRAY_AS_PROPS", Value::Int(2));
        for m in ARRAY_COMMON_METHODS {
            b = b.method(m);
        }
        for m in [
            "getIterator",
            "exchangeArray",
            "setIteratorClass",
            "getIteratorClass",
            "__debugInfo",
        ] {
            b = b.method(m);
        }
        b.build()
    }
);
lazy_class!(
    /// `class ArrayIterator implements SeekableIterator, ArrayAccess, Serializable, Countable`.
    array_iterator,
    ARRAY_ITERATOR,
    {
        let mut b = ClassBuilder::class("ArrayIterator")
            .implements(&seekable_iterator())
            .implements(&array_access())
            .implements(&serializable())
            .implements(&countable())
            .constant("STD_PROP_LIST", Value::Int(1))
            .constant("ARRAY_AS_PROPS", Value::Int(2));
        for m in ARRAY_COMMON_METHODS {
            b = b.method(m);
        }
        for m in [
            "rewind",
            "current",
            "key",
            "next",
            "valid",
            "seek",
            "__debugInfo",
        ] {
            b = b.method(m);
        }
        b.build()
    }
);
lazy_class!(
    /// `class RecursiveArrayIterator extends ArrayIterator implements RecursiveIterator`
    /// (PHPSRC/ext/spl/spl_array.stub.php).
    recursive_array_iterator,
    RECURSIVE_ARRAY_ITERATOR,
    ClassBuilder::class("RecursiveArrayIterator")
        .extends(&array_iterator())
        .implements(&recursive_iterator())
        .constant("CHILD_ARRAYS_ONLY", Value::Int(4))
        .method("hasChildren")
        .method("getChildren")
        .build()
);

/// A case-insensitive class table (PHP's `EG(class_table)`): looks classes up by name for
/// `constant('Foo::BAR')`, `enum('Foo')`, `class_exists()` and friends.
#[derive(Clone, Default)]
pub struct ClassTable {
    classes: HashMap<Box<str>, ClassRef>,
}

impl ClassTable {
    /// An empty table.
    pub fn new() -> ClassTable {
        ClassTable::default()
    }

    /// A table with every built-in class and interface of this module registered.
    pub fn with_builtins() -> ClassTable {
        let mut t = ClassTable::new();
        for c in [
            traversable(),
            iterator(),
            iterator_aggregate(),
            array_access(),
            countable(),
            stringable(),
            json_serializable(),
            serializable(),
            seekable_iterator(),
            recursive_iterator(),
            unit_enum(),
            backed_enum(),
            std_class(),
            closure(),
            array_object(),
            array_iterator(),
            recursive_array_iterator(),
        ] {
            t.classes.insert(c.lc_name().into(), c);
        }
        t
    }

    /// Registers a class; a name already in use throws
    /// `Cannot declare class X, because the name is already in use`.
    pub fn register(&mut self, class: ClassRef) -> PhpResult<()> {
        let key: Box<str> = class.lc_name().into();
        if self.classes.contains_key(&key) {
            let what = if class.is_interface() {
                "interface"
            } else if class.is_enum() {
                "enum"
            } else {
                "class"
            };
            return Err(PhpError::error(format!(
                "Cannot declare {} {}, because the name is already in use",
                what,
                class.name()
            )));
        }
        self.classes.insert(key, class);
        Ok(())
    }

    /// Looks a class up (case-insensitive, leading `\` ignored).
    pub fn get(&self, name: &str) -> Option<&ClassRef> {
        let name = name.strip_prefix('\\').unwrap_or(name);
        self.classes.get(name.to_ascii_lowercase().as_str())
    }

    /// `class_exists()` (classes and enums, not interfaces).
    pub fn class_exists(&self, name: &str) -> bool {
        self.get(name).is_some_and(|c| !c.is_interface())
    }

    /// `interface_exists()`.
    pub fn interface_exists(&self, name: &str) -> bool {
        self.get(name).is_some_and(|c| c.is_interface())
    }

    /// `enum_exists()`.
    pub fn enum_exists(&self, name: &str) -> bool {
        self.get(name).is_some_and(|c| c.is_enum())
    }

    /// A class constant by `Class::NAME` (the class-constant part of `defined()`/`constant()`).
    pub fn class_constant(&self, qualified: &str) -> Option<Value> {
        let (class, name) = qualified.split_once("::")?;
        self.get(class)?.constant(name)
    }

    /// Iterates over the registered classes (unordered).
    pub fn iter(&self) -> impl Iterator<Item = &ClassRef> {
        self.classes.values()
    }
}
