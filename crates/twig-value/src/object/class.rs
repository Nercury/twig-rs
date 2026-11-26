//! Class descriptors (`zend_class_entry`): names, inheritance, interfaces, constants, declared
//! properties and methods, enum cases.

use std::fmt;
use std::sync::{Arc, OnceLock};

use indexmap::IndexMap;

use crate::object::ObjectRef;
use crate::string::PhpString;
use crate::value::Value;

/// A shared class descriptor.
pub type ClassRef = Arc<ClassEntry>;

/// Member visibility.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Visibility {
    /// `public`
    Public,
    /// `protected`
    Protected,
    /// `private`
    Private,
}

impl Visibility {
    /// `public` / `protected` / `private` (the word used in error messages).
    pub fn as_str(self) -> &'static str {
        match self {
            Visibility::Public => "public",
            Visibility::Protected => "protected",
            Visibility::Private => "private",
        }
    }
}

/// What kind of class-like this is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClassKind {
    /// A class (possibly abstract or final).
    Class,
    /// An interface.
    Interface,
    /// An enum; `Some` for backed enums.
    Enum(Option<EnumBacking>),
}

/// Backing type of a backed enum.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnumBacking {
    /// `enum E: int`
    Int,
    /// `enum E: string`
    String,
}

/// A declared property (`zend_property_info`).
#[derive(Clone, Debug)]
pub struct PropertyInfo {
    /// Property name (without `$`).
    pub name: PhpString,
    /// Visibility.
    pub visibility: Visibility,
    /// `static` properties are not part of instances.
    pub is_static: bool,
    /// `readonly`.
    pub is_readonly: bool,
    /// Declared type as PHP prints it (`?int`, `string`, `Foo`), `None` when untyped. Typed
    /// properties without a default start uninitialised.
    pub type_name: Option<Arc<str>>,
    /// Default value; `None` for a typed property without default (uninitialised). Untyped
    /// properties without an explicit default have `Some(Value::Null)`.
    pub default: Option<Value>,
    /// Name of the declaring class (used for private-name mangling and messages).
    pub declaring_class: Arc<str>,
}

/// A declared method (`zend_function` in the class's function table).
#[derive(Clone, Debug)]
pub struct MethodInfo {
    /// Method name as declared (case preserved).
    pub name: Arc<str>,
    /// Visibility.
    pub visibility: Visibility,
    /// `static`.
    pub is_static: bool,
    /// Name of the declaring class.
    pub declaring_class: Arc<str>,
}

/// A class constant.
#[derive(Clone, Debug)]
pub struct ConstantInfo {
    /// Constant name (case-sensitive).
    pub name: Arc<str>,
    /// Value (`null` for an alias of an enum case, see `enum_case`).
    pub value: Value,
    /// Visibility.
    pub visibility: Visibility,
    /// Declaring class.
    pub declaring_class: Arc<str>,
    /// For enum constants defined as another case (`const Wild = self::Spades;`): that case's name.
    pub enum_case: Option<Arc<str>>,
}

/// A class descriptor. Build with [`ClassBuilder`]; share as [`ClassRef`].
///
/// Lookups follow PHP: class names and method names are case-insensitive, constant and property
/// names case-sensitive. Inherited members are merged at build time (own methods first, then
/// inherited ones, like `get_class_methods()`; parent properties first in slot order, like
/// `var_dump()`).
pub struct ClassEntry {
    name: Arc<str>,
    lc_name: Box<str>,
    kind: ClassKind,
    is_abstract: bool,
    is_final: bool,
    is_anonymous: bool,
    allow_dynamic_properties: bool,
    parent: Option<ClassRef>,
    interfaces: Vec<ClassRef>,
    constants: IndexMap<Arc<str>, ConstantInfo>,
    properties: IndexMap<PhpString, PropertyInfo>,
    methods: IndexMap<Box<str>, MethodInfo>,
    case_specs: Vec<(Arc<str>, Option<Value>)>,
    cases: OnceLock<Vec<ObjectRef>>,
}

impl ClassEntry {
    /// The class name as declared (`$obj::class`). Anonymous class names contain a NUL byte,
    /// followed by the defining location; see [`ClassEntry::display_name`].
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The name up to the first NUL byte: what PHP prints with `%s` (`class@anonymous`).
    pub fn display_name(&self) -> &str {
        match self.name.find('\0') {
            Some(pos) => &self.name[..pos],
            None => &self.name,
        }
    }

    /// The kind (class, interface, enum).
    pub fn kind(&self) -> ClassKind {
        self.kind
    }

    /// Whether this is an interface.
    pub fn is_interface(&self) -> bool {
        self.kind == ClassKind::Interface
    }

    /// Whether this is an enum.
    pub fn is_enum(&self) -> bool {
        matches!(self.kind, ClassKind::Enum(_))
    }

    /// `abstract class`.
    pub fn is_abstract(&self) -> bool {
        self.is_abstract
    }

    /// `final class` (enums and `Closure` are final).
    pub fn is_final(&self) -> bool {
        self.is_final
    }

    /// Anonymous class (`ZEND_ACC_ANON_CLASS`).
    pub fn is_anonymous(&self) -> bool {
        self.is_anonymous
    }

    /// Whether dynamic properties can be created without the PHP 8.2 deprecation
    /// (`#[AllowDynamicProperties]`, e.g. `stdClass`).
    pub fn allows_dynamic_properties(&self) -> bool {
        self.allow_dynamic_properties
    }

    /// The parent class.
    pub fn parent(&self) -> Option<&ClassRef> {
        self.parent.as_ref()
    }

    /// All implemented interfaces (inherited ones included), in PHP's interface-table order
    /// (`class_implements()` order).
    pub fn interfaces(&self) -> &[ClassRef] {
        &self.interfaces
    }

    /// The parent chain, nearest first (`class_parents()` order).
    pub fn parents(&self) -> Vec<ClassRef> {
        let mut out = Vec::new();
        let mut cur = self.parent.clone();
        while let Some(c) = cur {
            cur = c.parent.clone();
            out.push(c);
        }
        out
    }

    /// `instanceof_function()`: whether this class is `name`, extends it, or implements it
    /// (case-insensitive).
    pub fn is_subclass_of(&self, name: &str) -> bool {
        let name = name.strip_prefix('\\').unwrap_or(name);
        if self.name.eq_ignore_ascii_case(name) {
            return true;
        }
        if self
            .interfaces
            .iter()
            .any(|i| i.name.eq_ignore_ascii_case(name))
        {
            return true;
        }
        let mut cur = self.parent.as_ref();
        while let Some(c) = cur {
            if c.name.eq_ignore_ascii_case(name) {
                return true;
            }
            cur = c.parent.as_ref();
        }
        false
    }

    /// `instanceof` against another class entry.
    pub fn is_subclass_of_class(&self, other: &ClassEntry) -> bool {
        self.is_subclass_of(&other.name)
    }

    /// Looks up a method (case-insensitive), including inherited ones.
    pub fn method(&self, name: &str) -> Option<&MethodInfo> {
        self.method_entry(name).map(|(_, m)| m)
    }

    /// [`ClassEntry::method`] with the lowercase name of the method (the key of the method table).
    pub(crate) fn method_entry(&self, name: &str) -> Option<(&str, &MethodInfo)> {
        if let Some((key, m)) = self.methods.get_key_value(name) {
            return Some((key, m));
        }
        // the lowercase name, without allocating for the usual lengths
        let mut buf = [0u8; 64];
        let owned;
        let lc = match buf.get_mut(..name.len()) {
            Some(bytes) => {
                bytes.copy_from_slice(name.as_bytes());
                bytes.make_ascii_lowercase();
                std::str::from_utf8(bytes).expect("ASCII lowercasing keeps UTF-8")
            }
            None => {
                owned = name.to_ascii_lowercase();
                owned.as_str()
            }
        };
        self.methods.get_key_value(lc).map(|(key, m)| (&**key, m))
    }

    /// All methods in function-table order (own first, then inherited).
    pub fn methods(&self) -> impl Iterator<Item = &MethodInfo> {
        self.methods.values()
    }

    /// Port of `get_class_methods()` from a foreign scope: the public methods, in function-table
    /// order.
    pub fn public_method_names(&self) -> Vec<Arc<str>> {
        self.methods
            .values()
            .filter(|m| m.visibility == Visibility::Public)
            .map(|m| m.name.clone())
            .collect()
    }

    /// Looks up a declared property (static or instance), including inherited ones (a parent's
    /// private properties too; see [`ClassEntry::visible_property`]).
    pub fn property(&self, name: &[u8]) -> Option<&PropertyInfo> {
        self.properties.get(name)
    }

    /// The declared property as this class sees it: a private property declared by a parent class
    /// is not visible (`ReflectionClass::hasProperty()` returns false for it and property access
    /// treats it as dynamic).
    pub fn visible_property(&self, name: &[u8]) -> Option<&PropertyInfo> {
        self.properties
            .get(name)
            .filter(|p| p.visibility != Visibility::Private || *p.declaring_class == *self.name)
    }

    /// `ReflectionClass::hasMethod()`: any visibility, case-insensitive.
    pub fn has_method(&self, name: &str) -> bool {
        self.method(name).is_some()
    }

    /// All declared properties (instance properties in slot order: parent first).
    pub fn properties(&self) -> impl Iterator<Item = &PropertyInfo> {
        self.properties.values()
    }

    /// Declared instance (non-static) properties in slot order.
    pub fn instance_properties(&self) -> impl Iterator<Item = &PropertyInfo> {
        self.properties.values().filter(|p| !p.is_static)
    }

    /// Looks up a class constant (case-sensitive; enum cases included).
    pub fn constant(&self, name: &str) -> Option<Value> {
        if self.is_enum()
            && let Some(case) = self.enum_case(name)
        {
            return Some(Value::Object(case));
        }
        let c = self.constants.get(name)?;
        match &c.enum_case {
            Some(case) => self.enum_case(case).map(Value::Object),
            None => Some(c.value.clone()),
        }
    }

    /// Constant metadata (enum cases are not listed here; see [`ClassEntry::enum_cases`]).
    pub fn constant_info(&self, name: &str) -> Option<&ConstantInfo> {
        self.constants.get(name)
    }

    /// All declared constants (without enum cases), in declaration order.
    pub fn constants(&self) -> impl Iterator<Item = &ConstantInfo> {
        self.constants.values()
    }

    /// The enum's cases in declaration order (`Enum::cases()`); empty for non-enums.
    pub fn enum_cases(&self) -> &[ObjectRef] {
        self.cases.get().map(Vec::as_slice).unwrap_or(&[])
    }

    /// The enum case named `name`.
    pub fn enum_case(&self, name: &str) -> Option<ObjectRef> {
        self.enum_cases()
            .iter()
            .find(|c| c.enum_case_name() == Some(name))
            .cloned()
    }

    pub(crate) fn lc_name(&self) -> &str {
        &self.lc_name
    }
}

impl fmt::Debug for ClassEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "class {}", self.name)
    }
}

/// Builder for [`ClassEntry`].
///
/// ```
/// use twig_value::object::{ClassBuilder, builtin};
/// let class = ClassBuilder::class("App\\User")
///     .implements(&builtin::countable())
///     .constant("ROLE", "admin")
///     .property("name", twig_value::Value::from("anon"))
///     .method("getName")
///     .method("count")
///     .build();
/// assert!(class.is_subclass_of("countable"));
/// assert_eq!(class.method("GETNAME").unwrap().name.as_ref(), "getName");
/// ```
pub struct ClassBuilder {
    name: Arc<str>,
    kind: ClassKind,
    is_abstract: bool,
    is_final: bool,
    is_anonymous: bool,
    allow_dynamic_properties: bool,
    parent: Option<ClassRef>,
    declared_interfaces: Vec<ClassRef>,
    constants: Vec<ConstantInfo>,
    properties: Vec<PropertyInfo>,
    methods: Vec<MethodInfo>,
    cases: Vec<(Arc<str>, Option<Value>)>,
}

impl ClassBuilder {
    fn new(name: &str, kind: ClassKind) -> Self {
        let name: Arc<str> = Arc::from(name.strip_prefix('\\').unwrap_or(name));
        ClassBuilder {
            name,
            kind,
            is_abstract: false,
            is_final: matches!(kind, ClassKind::Enum(_)),
            is_anonymous: false,
            allow_dynamic_properties: false,
            parent: None,
            declared_interfaces: Vec::new(),
            constants: Vec::new(),
            properties: Vec::new(),
            methods: Vec::new(),
            cases: Vec::new(),
        }
    }

    /// A class.
    pub fn class(name: &str) -> Self {
        Self::new(name, ClassKind::Class)
    }

    /// An interface.
    pub fn interface(name: &str) -> Self {
        Self::new(name, ClassKind::Interface)
    }

    /// An enum (`None` = pure/unit enum). Enums implement `UnitEnum` (and `BackedEnum` when backed)
    /// and get the `cases` (and `from`/`tryFrom`) static methods automatically.
    pub fn enumeration(name: &str, backing: Option<EnumBacking>) -> Self {
        let mut b = Self::new(name, ClassKind::Enum(backing));
        b.declared_interfaces.push(super::builtin::unit_enum());
        if backing.is_some() {
            b.declared_interfaces.push(super::builtin::backed_enum());
        }
        b.properties.push(PropertyInfo {
            name: PhpString::from("name"),
            visibility: Visibility::Public,
            is_static: false,
            is_readonly: true,
            type_name: Some(Arc::from("string")),
            default: None,
            declaring_class: b.name.clone(),
        });
        if let Some(backing) = backing {
            b.properties.push(PropertyInfo {
                name: PhpString::from("value"),
                visibility: Visibility::Public,
                is_static: false,
                is_readonly: true,
                type_name: Some(Arc::from(match backing {
                    EnumBacking::Int => "int",
                    EnumBacking::String => "string",
                })),
                default: None,
                declaring_class: b.name.clone(),
            });
        }
        b.methods.push(MethodInfo {
            name: Arc::from("cases"),
            visibility: Visibility::Public,
            is_static: true,
            declaring_class: b.name.clone(),
        });
        if backing.is_some() {
            for m in ["from", "tryFrom"] {
                b.methods.push(MethodInfo {
                    name: Arc::from(m),
                    visibility: Visibility::Public,
                    is_static: true,
                    declaring_class: b.name.clone(),
                });
            }
        }
        b
    }

    /// Marks the class anonymous (`new class {}`); `name` should be PHP's generated name
    /// (`class@anonymous` + NUL + location).
    pub fn anonymous(mut self) -> Self {
        self.is_anonymous = true;
        self
    }

    /// `abstract class`.
    pub fn abstract_class(mut self) -> Self {
        self.is_abstract = true;
        self
    }

    /// `final class`.
    pub fn final_class(mut self) -> Self {
        self.is_final = true;
        self
    }

    /// `#[AllowDynamicProperties]`.
    pub fn allow_dynamic_properties(mut self) -> Self {
        self.allow_dynamic_properties = true;
        self
    }

    /// `extends parent`: inherits the parent's interfaces, constants, non-private properties and
    /// methods.
    pub fn extends(mut self, parent: &ClassRef) -> Self {
        self.parent = Some(parent.clone());
        if parent.allow_dynamic_properties {
            self.allow_dynamic_properties = true;
        }
        self
    }

    /// `implements interface` (or `extends` for interfaces). The interface's own parents are added
    /// after it.
    pub fn implements(mut self, interface: &ClassRef) -> Self {
        self.declared_interfaces.push(interface.clone());
        self
    }

    /// A public constant.
    pub fn constant(self, name: &str, value: impl Into<Value>) -> Self {
        self.constant_with_visibility(name, value, Visibility::Public)
    }

    /// A constant with explicit visibility.
    pub fn constant_with_visibility(
        mut self,
        name: &str,
        value: impl Into<Value>,
        visibility: Visibility,
    ) -> Self {
        self.constants.push(ConstantInfo {
            name: Arc::from(name),
            value: value.into(),
            visibility,
            declaring_class: self.name.clone(),
            enum_case: None,
        });
        self
    }

    /// An enum constant that names one of the enum's cases (`const Wild = self::Spades;`).
    pub fn enum_case_constant(mut self, name: &str, case: &str) -> Self {
        self.constants.push(ConstantInfo {
            name: Arc::from(name),
            value: Value::Null,
            visibility: Visibility::Public,
            declaring_class: self.name.clone(),
            enum_case: Some(Arc::from(case)),
        });
        self
    }

    /// A public untyped instance property with a default value.
    pub fn property(self, name: &str, default: impl Into<Value>) -> Self {
        let declaring_class = self.name.clone();
        self.property_info(PropertyInfo {
            name: PhpString::from(name),
            visibility: Visibility::Public,
            is_static: false,
            is_readonly: false,
            type_name: None,
            default: Some(default.into()),
            declaring_class,
        })
    }

    /// A fully specified declared property.
    pub fn property_info(mut self, mut info: PropertyInfo) -> Self {
        info.declaring_class = self.name.clone();
        self.properties.push(info);
        self
    }

    /// A public method.
    pub fn method(self, name: &str) -> Self {
        self.method_with(name, Visibility::Public, false)
    }

    /// A method with explicit visibility and staticness.
    pub fn method_with(mut self, name: &str, visibility: Visibility, is_static: bool) -> Self {
        self.methods.push(MethodInfo {
            name: Arc::from(name),
            visibility,
            is_static,
            declaring_class: self.name.clone(),
        });
        self
    }

    /// An enum case; `value` is the backing value for backed enums (`None` for unit enums).
    pub fn case(mut self, name: &str, value: Option<Value>) -> Self {
        self.cases.push((Arc::from(name), value));
        self
    }

    /// Finishes the class.
    pub fn build(mut self) -> ClassRef {
        let lc_name = self.name.to_ascii_lowercase().into_boxed_str();

        // A class declaring __toString() implicitly implements Stringable (PHP 8.0).
        if &*lc_name != "stringable"
            && self
                .methods
                .iter()
                .any(|m| m.name.eq_ignore_ascii_case("__toString"))
            && !self
                .declared_interfaces
                .iter()
                .any(|i| i.lc_name.as_ref() == "stringable")
        {
            self.declared_interfaces.push(super::builtin::stringable());
        }

        // Interfaces (zend interface-table order): the parent class's first; then for a class each
        // declared interface followed by the interfaces it extends, for an interface the extended
        // interface's own table followed by the extended interface (so `SeekableIterator` lists
        // `Traversable, Iterator` and `ArrayIterator` lists `SeekableIterator, Traversable,
        // Iterator, ...`, as `class_implements()` shows).
        let mut interfaces: Vec<ClassRef> = Vec::new();
        let add = |list: &mut Vec<ClassRef>, i: &ClassRef| {
            if !list.iter().any(|x| x.lc_name == i.lc_name) {
                list.push(i.clone());
            }
        };
        if let Some(parent) = &self.parent {
            for i in &parent.interfaces {
                add(&mut interfaces, i);
            }
        }
        for i in &self.declared_interfaces {
            if self.kind == ClassKind::Interface {
                for j in &i.interfaces {
                    add(&mut interfaces, j);
                }
                add(&mut interfaces, i);
            } else {
                add(&mut interfaces, i);
                for j in &i.interfaces {
                    add(&mut interfaces, j);
                }
            }
        }

        // Constants: own, then parent's, then interfaces'.
        let mut constants: IndexMap<Arc<str>, ConstantInfo> = IndexMap::new();
        for c in self.constants {
            constants.insert(c.name.clone(), c);
        }
        if let Some(parent) = &self.parent {
            for c in parent.constants.values() {
                if c.visibility != Visibility::Private {
                    constants.entry(c.name.clone()).or_insert_with(|| c.clone());
                }
            }
        }
        for i in &interfaces {
            for c in i.constants.values() {
                constants.entry(c.name.clone()).or_insert_with(|| c.clone());
            }
        }

        // Properties: parent's slots first (private ones included, like `properties_info` after
        // `do_inherit_property()`; lookups treat a parent's private property as undeclared), then
        // own.
        let mut properties: IndexMap<PhpString, PropertyInfo> = IndexMap::new();
        if let Some(parent) = &self.parent {
            for p in parent.properties.values() {
                properties.insert(p.name.clone(), p.clone());
            }
        }
        for p in self.properties {
            properties.insert(p.name.clone(), p);
        }

        // Methods: own first, then inherited (function-table order of zend_do_inheritance).
        let mut methods: IndexMap<Box<str>, MethodInfo> = IndexMap::new();
        for m in self.methods {
            methods.insert(m.name.to_ascii_lowercase().into_boxed_str(), m);
        }
        if let Some(parent) = &self.parent {
            for (k, m) in &parent.methods {
                if !methods.contains_key(k) {
                    methods.insert(k.clone(), m.clone());
                }
            }
        }
        for i in &interfaces {
            for (k, m) in &i.methods {
                if !methods.contains_key(k) {
                    methods.insert(k.clone(), m.clone());
                }
            }
        }

        let class = Arc::new(ClassEntry {
            name: self.name,
            lc_name,
            kind: self.kind,
            is_abstract: self.is_abstract,
            is_final: self.is_final,
            is_anonymous: self.is_anonymous,
            allow_dynamic_properties: self.allow_dynamic_properties,
            parent: self.parent,
            interfaces,
            constants,
            properties,
            methods,
            case_specs: self.cases,
            cases: OnceLock::new(),
        });
        if class.is_enum() {
            let cases = class
                .case_specs
                .iter()
                .map(|(name, value)| {
                    super::enum_case::EnumCase::new_object(&class, name.clone(), value.clone())
                })
                .collect();
            let _ = class.cases.set(cases);
        }
        class
    }
}
