//! PHP constants and enums: the `constant` function (`CoreExtension::constant()`, also behind `constant(...) is
//! defined`) and the `enum` and `enum_cases` functions, over PHP's `defined()`, `constant()` and `enum_exists()`, and
//! the class lookups of PHP callables.
//!
//! Constants and classes come from the environment's registry ([`Constants`](crate::runtime::Constants): the
//! `twig-value` built-in classes, the date classes and `DATE_*` constants, and what the host registers), then from
//! the PHP constants twig-rs ships ([`php_constants`](super::php_constants): Core, standard, pcre, json, mbstring,
//! iconv and random, as the target PHP defines them) and the built-in classes the registry does not hold
//! ([`builtin_classes`]: PHP 8.4's `RoundingMode` enum and every class, interface, trait and enum of Twig, generated
//! from the oracle's reflection in [`twig_classes`](super::twig_classes)).

use std::collections::HashSet;
use std::sync::LazyLock;

use twig_value::builtins::format::rounding_mode_class;
use twig_value::object::builtin::ClassTable;
use twig_value::object::{
    ClassBuilder, ClassRef, ConstantInfo, EnumBacking, FunctionKind, Param, Visibility,
};
use twig_value::param::{self, Arg, DeclaredType, TypeMask, UserArg, verify_user_arg};
use twig_value::types::zend_zval_value_name;
use twig_value::{Array, ArrayKey, Diagnostics, PhpError, PhpResult, PhpString, Value, convert};

use super::bind;
use super::php_constants::{self, CONSTANTS, PhpConstant};
use super::twig_classes::{self, K, Kind, TwigClass, V};
use crate::error::Error;
use crate::extension::runtime::{Call, CallResult};
use crate::runtime::Environment;

/// `Twig\Extension\CoreExtension`, the class of the `constant` function's PHP method.
const CORE_EXTENSION: &str = "Twig\\Extension\\CoreExtension";
/// `Twig\Template`, the parent of every compiled template class.
const TEMPLATE: &str = "Twig\\Template";

/// The value of a constant of the generated class table.
fn class_constant_value(value: &twig_classes::C) -> Value {
    use twig_classes::C;
    match *value {
        C::Null => Value::Null,
        C::Bool(b) => Value::Bool(b),
        C::Int(i) => Value::Int(i),
        C::Float(bits) => Value::Float(f64::from_bits(bits)),
        C::Str(s) => Value::String(PhpString::from(s)),
        C::Array(entries) => {
            let mut array = Array::new();
            for (key, item) in entries {
                let key = match *key {
                    K::Int(i) => ArrayKey::Int(i),
                    K::Str(s) => ArrayKey::from_string(PhpString::from(s)),
                };
                array.insert(key, class_constant_value(item));
            }
            Value::Array(array)
        }
    }
}

/// The modifiers of a method of the generated class table: `^` static, `!` abstract, `#` protected, `-` private.
fn method_modifiers(token: &str) -> (&str, bool, bool, Visibility) {
    let is_static = token.starts_with('^');
    let token = token.strip_prefix('^').unwrap_or(token);
    let is_abstract = token.starts_with('!');
    let token = token.strip_prefix('!').unwrap_or(token);
    match token.as_bytes().first() {
        Some(b'#') => (&token[1..], is_static, is_abstract, Visibility::Protected),
        Some(b'-') => (&token[1..], is_static, is_abstract, Visibility::Private),
        _ => (token, is_static, is_abstract, Visibility::Public),
    }
}

/// Builds one class of the generated table; its parent and interfaces are looked up in `table` (the classes built so
/// far) and in `twig-value`'s built-in classes. The parent and interfaces neither has (PHPUnit's) are left out.
fn build_class(spec: &TwigClass, table: &ClassTable, builtins: &ClassTable) -> ClassRef {
    let resolve = |name: &str| table.get(name).or_else(|| builtins.get(name)).cloned();
    let mut builder = match spec.kind {
        Kind::Interface => ClassBuilder::interface(spec.name),
        Kind::Enum => ClassBuilder::enumeration(spec.name, None),
        Kind::IntEnum => ClassBuilder::enumeration(spec.name, Some(EnumBacking::Int)),
        Kind::StringEnum => ClassBuilder::enumeration(spec.name, Some(EnumBacking::String)),
        Kind::AbstractClass => ClassBuilder::class(spec.name).abstract_class(),
        Kind::FinalClass => ClassBuilder::class(spec.name).final_class(),
        // a trait is found like a class; Twig's traits declare no constants and no static methods, so nothing PHP
        // does differently for traits (`Cannot access trait constant`, static trait calls) is reachable
        Kind::Class | Kind::Trait => ClassBuilder::class(spec.name),
    };
    if let Some(parent) = spec.parent.and_then(resolve) {
        builder = builder.extends(&parent);
    }
    for interface in spec.interfaces.iter().filter_map(|name| resolve(name)) {
        builder = builder.implements(&interface);
    }
    for (name, visibility, value) in spec.constants {
        let visibility = match visibility {
            V::Public => Visibility::Public,
            V::Protected => Visibility::Protected,
            V::Private => Visibility::Private,
        };
        builder = builder.constant_with_visibility(name, class_constant_value(value), visibility);
    }
    for (name, value) in spec.cases {
        builder = builder.case(name, value.as_ref().map(class_constant_value));
    }
    for token in spec.methods.split(' ').filter(|token| !token.is_empty()) {
        let (name, is_static, _, visibility) = method_modifiers(token);
        builder = builder.method_with(name, visibility, is_static);
    }
    builder.build()
}

/// The classes the environment's registry does not hold: PHP 8.4's `enum RoundingMode`, and every class, interface,
/// trait and enum of Twig 3.31.1 with the PHP classes they extend or implement that `twig-value` lacks (`Exception`,
/// `Throwable`, `IteratorIterator`, `OuterIterator`), as the oracle's reflection reports them
/// ([`twig_classes`](super::twig_classes)): constants with their values and visibility, enum cases, methods with
/// their visibility and staticness, inherited members included. In PHP every Twig class is autoloadable, so a
/// template can name any of them (`constant('Twig\\Token::NAME_TYPE')`), and `get_class()` gives the class of a
/// `Markup`, a `TemplateWrapper` or a macro namespace.
pub(crate) fn builtin_classes() -> &'static ClassTable {
    static TABLE: LazyLock<ClassTable> = LazyLock::new(|| {
        let builtins = ClassTable::with_builtins();
        let mut table = ClassTable::new();
        // the names are distinct
        let _ = table.register(rounding_mode_class());
        for spec in twig_classes::CLASSES {
            let class = build_class(spec, &table, &builtins);
            let _ = table.register(class);
        }
        table
    });
    &TABLE
}

/// The methods the generated classes declare `abstract` (interface methods aside), as lower-case `class::method`.
static ABSTRACT_METHODS: LazyLock<HashSet<String>> = LazyLock::new(|| {
    twig_classes::CLASSES
        .iter()
        .filter(|spec| spec.kind != Kind::Interface)
        .flat_map(|spec| {
            spec.methods
                .split(' ')
                .filter(|token| !token.is_empty())
                .filter_map(move |token| {
                    let (name, _, is_abstract, _) = method_modifiers(token);
                    is_abstract.then(|| format!("{}::{name}", spec.name).to_ascii_lowercase())
                })
        })
        .collect()
});

/// Whether the method `method` declared by the class `declaring` is abstract (`ZEND_ACC_ABSTRACT`): every method of an
/// interface, and the abstract methods of Twig's abstract classes. `twig-value`'s classes have no other abstract
/// methods.
pub(crate) fn is_abstract_method(env: &Environment, declaring: &str, method: &str) -> bool {
    find_class(env, declaring.as_bytes()).is_some_and(|class| class.is_interface())
        || ABSTRACT_METHODS.contains(&format!("{declaring}::{method}").to_ascii_lowercase())
}

/// A PHP constant twig-rs ships, by exact name.
fn php_constant(name: &[u8]) -> Option<&'static PhpConstant> {
    CONSTANTS
        .binary_search_by(|constant| constant.name.as_bytes().cmp(name))
        .ok()
        .map(|i| &CONSTANTS[i])
}

/// The value of a shipped constant.
fn php_constant_value(constant: &PhpConstant) -> Value {
    use php_constants::C;
    match constant.value {
        C::Null => Value::Null,
        C::Bool(b) => Value::Bool(b),
        C::Int(i) => Value::Int(i),
        C::Float(bits) => Value::Float(f64::from_bits(bits)),
        C::Str(s) => Value::String(PhpString::from(s)),
    }
}

/// A class by name, as `zend_lookup_class()` finds it (case-insensitive, a leading `\` ignored): the environment's
/// registry first, then [`builtin_classes`].
pub(crate) fn find_class(env: &Environment, name: &[u8]) -> Option<ClassRef> {
    let name = String::from_utf8_lossy(name);
    env.with_constants(|constants| constants.class(&name).cloned())
        .or_else(|| builtin_classes().get(&name).cloned())
}

/// The traits of the generated classes, by lower-case name.
static TRAITS: LazyLock<HashSet<String>> = LazyLock::new(|| {
    twig_classes::CLASSES
        .iter()
        .filter(|spec| spec.kind == Kind::Trait)
        .map(|spec| spec.name.to_ascii_lowercase())
        .collect()
});

/// Whether the class named `name` is a trait (`ZEND_ACC_TRAIT`): only Twig's own traits are (`twig-value` has none).
pub(crate) fn is_trait(name: &str) -> bool {
    TRAITS.contains(&name.to_ascii_lowercase())
}

/// A class of [`builtin_classes`] that twig-rs refers to by name (`Twig\Extension\CoreExtension`, ...).
pub(crate) fn twig_class(name: &str) -> ClassRef {
    builtin_classes()
        .get(name)
        .cloned()
        .unwrap_or_else(|| ClassBuilder::class(name).build())
}

/// The class scope a call of PHP's `constant()` or `defined()` runs in (`zend_get_executed_scope()`): it resolves
/// `self`, `parent` and `static` (`zend_get_class_constant_ex()`) and decides which private and protected constants
/// are accessible (`zend_verify_const_access()`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Scope {
    /// `CoreExtension::constant()` (the `constant` function): a static method of `Twig\Extension\CoreExtension`,
    /// called statically, so `self` and `static` are that class and `parent` is `Twig\Extension\AbstractExtension`;
    /// the private constants of `CoreExtension` are accessible.
    CoreExtension,
    /// The compiled code of a template (`ConstantTest` compiles to `constant()` in the template class, a subclass of
    /// `Twig\Template` that declares no constant): `self`, `static` and `parent` see the constants of `Twig\Template`,
    /// and only public constants (and protected ones of `Twig\Template`) are accessible.
    Template,
}

impl Scope {
    /// The class `self` or `static` designates (for [`Scope::Template`], the parent standing for the template class:
    /// `Twig\Template` declares only public constants, which the template class inherits).
    fn self_class(self) -> ClassRef {
        match self {
            Scope::CoreExtension => twig_class(CORE_EXTENSION),
            Scope::Template => twig_class(TEMPLATE),
        }
    }

    /// The class `parent` designates.
    fn parent_class(self) -> ClassRef {
        match self {
            Scope::CoreExtension => twig_class("Twig\\Extension\\AbstractExtension"),
            Scope::Template => twig_class(TEMPLATE),
        }
    }

    /// The scope class and its parents, by name (the template class itself declares nothing and is left out).
    fn chain(self) -> &'static [&'static str] {
        match self {
            Scope::CoreExtension => &[CORE_EXTENSION, "Twig\\Extension\\AbstractExtension"],
            Scope::Template => &[TEMPLATE],
        }
    }

    /// Port of `zend_verify_const_access()` (PHPSRC/Zend/zend_constants.c): a public constant is accessible, a
    /// private one only from its declaring class, a protected one when `zend_check_protected()` relates the declaring
    /// class and the scope through their parent chains.
    fn can_access(self, env: &Environment, constant: &ConstantInfo) -> bool {
        let declaring = &*constant.declaring_class;
        match constant.visibility {
            Visibility::Public => true,
            Visibility::Private => self == Scope::CoreExtension && declaring == CORE_EXTENSION,
            Visibility::Protected => {
                // the scope class is the declaring class or one of its parents (never the template class, which no
                // class extends) ...
                let mut class = find_class(env, declaring.as_bytes());
                while let Some(current) = class {
                    if self == Scope::CoreExtension && current.name() == CORE_EXTENSION {
                        return true;
                    }
                    class = current.parent().cloned();
                }
                // ... or the declaring class is the scope class or one of its parents
                self.chain().contains(&declaring)
            }
        }
    }
}

/// The bytes of `s` up to its first NUL byte: what C's `%s` prints of a PHP string.
fn c_str(bytes: &[u8]) -> &[u8] {
    match memchr::memchr(0, bytes) {
        Some(nul) => &bytes[..nul],
        None => bytes,
    }
}

/// Splits `Class::NAME` like `zend_get_constant_ex()`: at the last `:` that follows another `:` (`::B` is the
/// constant `B` of the class ``).
fn split_class_constant(name: &[u8]) -> Option<(&[u8], &[u8])> {
    let colon = name.iter().rposition(|&b| b == b':')?;
    (colon > 0 && name[colon - 1] == b':').then(|| (&name[..colon - 1], &name[colon + 1..]))
}

/// Port of `zend_get_class_constant_ex()` (PHPSRC/Zend/zend_constants.c): the class (`self`, `parent` and `static`
/// resolved in `scope`, any other name looked up), the constant or enum case, its accessibility from `scope`, the
/// deprecation of a deprecated constant. `silent` is `ZEND_FETCH_CLASS_SILENT` (`defined()`): every failure is
/// `Ok(None)`; otherwise (`ZEND_FETCH_CLASS_EXCEPTION`, `constant()`) the failures are the `Error`s `Class "%s" not
/// found`, `Undefined constant %s::%s` and `Cannot access %s constant %s::%s`, which name the class as written.
fn class_constant(
    env: &Environment,
    scope: Scope,
    class_name: &[u8],
    constant: &[u8],
    silent: bool,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Option<Value>> {
    let class =
        if class_name.eq_ignore_ascii_case(b"self") || class_name.eq_ignore_ascii_case(b"static") {
            scope.self_class()
        } else if class_name.eq_ignore_ascii_case(b"parent") {
            scope.parent_class()
        } else {
            match find_class(env, class_name) {
                Some(class) => class,
                None if silent => return Ok(None),
                None => {
                    return Err(PhpError::error(PhpString::concat(&[
                        b"Class \"",
                        c_str(class_name),
                        b"\" not found",
                    ])));
                }
            }
        };
    let name = String::from_utf8_lossy(constant).into_owned();
    // `%s::%s` of `ZSTR_VAL()`s: C strings
    let qualified = || PhpString::concat(&[c_str(class_name), b"::", c_str(constant)]);
    if let Some(info) = class.constant_info(&name) {
        if !scope.can_access(env, info) {
            if silent {
                return Ok(None);
            }
            return Err(PhpError::error(PhpString::concat(&[
                b"Cannot access ",
                info.visibility.as_str().as_bytes(),
                b" constant ",
                qualified().as_bytes(),
            ])));
        }
        // zend_deprecated_class_constant() names the declaring class
        if !silent
            && let Some(suffix) =
                twig_date::classes::class_constant_deprecation(&info.declaring_class, &name)
        {
            diag.deprecated(format!(
                "Constant {}::{name} is deprecated {suffix}",
                info.declaring_class
            ))?;
        }
    }
    match class.constant(&name) {
        Some(value) => Ok(Some(value)),
        None if silent => Ok(None),
        None => Err(PhpError::error(PhpString::concat(&[
            b"Undefined constant ",
            qualified().as_bytes(),
        ]))),
    }
}

/// Port of `defined($constant_name)` (`zend_get_constant_ex()` with `ZEND_FETCH_CLASS_SILENT`,
/// PHPSRC/Zend/zend_constants.c) called in `scope`: a class constant needs an existing class and a constant
/// accessible from the scope (enum cases are public), a global constant must exist (`true`, `false` and `null` in any
/// case always do).
pub(crate) fn defined(env: &Environment, scope: Scope, name: &[u8]) -> bool {
    let name = name.strip_prefix(b"\\").unwrap_or(name);
    if let Some((class, constant)) = split_class_constant(name) {
        let mut diag: Vec<twig_value::Diagnostic> = Vec::new();
        return matches!(
            class_constant(env, scope, class, constant, true, &mut diag),
            Ok(Some(_))
        );
    }
    php_constant(name).is_some() || env.with_constants(|constants| constants.defined(name))
}

/// Port of `constant($name)` (`zend_get_constant_ex()` with `ZEND_FETCH_CLASS_EXCEPTION`) called in `scope`: the value
/// of a defined constant, with the deprecation of a deprecated one (`Constant %s is deprecated since ...`); the
/// `Error`s of an unknown class or constant and of an inaccessible constant.
pub(crate) fn scoped_constant_value(
    env: &Environment,
    scope: Scope,
    name: &[u8],
    diag: &mut dyn Diagnostics,
) -> PhpResult<Value> {
    let name = name.strip_prefix(b"\\").unwrap_or(name);
    if let Some((class, constant)) = split_class_constant(name) {
        return class_constant(env, scope, class, constant, false, diag)
            .map(Option::unwrap_or_default);
    }
    if let Some(constant) = php_constant(name) {
        if let Some(suffix) = constant.deprecated {
            diag.deprecated(PhpString::concat(&[
                b"Constant ",
                constant.name.as_bytes(),
                b" is deprecated",
                suffix,
            ]))?;
        }
        return Ok(php_constant_value(constant));
    }
    env.with_constants(|constants| constants.constant(name, diag))
}

/// PHP's `constant($name)` called by the compiled code of a template: what Twig's `ConstantTest` compiles to
/// (`constant($name)` or `constant(get_class($object)."::".$name)`, in the scope of the template class,
/// [`Scope::Template`]).
///
/// # Errors
///
/// The `Error`s of [`scoped_constant_value`].
pub(crate) fn constant_value(
    env: &Environment,
    name: &[u8],
    diag: &mut dyn Diagnostics,
) -> PhpResult<Value> {
    scoped_constant_value(env, Scope::Template, name, diag)
}

/// Twig: `CoreExtension::constant($constant, $object = null, bool $checkDefined = false)`
/// (TWIG/src/Extension/CoreExtension.php): the global or class constant `$constant` (with an object, the constant of
/// its class, and `'class'` its class name); with `$checkDefined` (`constant(...) is defined`) whether it is defined.
///
/// Errors: `Constant "%s" is undefined.`, `You cannot use the Twig function "constant" to access "%s". You could
/// provide an object and call constant("class", $object) or use the class name directly as a string.` (runtime
/// errors), `Cannot use "::class" on %s` for a non-object, the `TypeError` of `defined()` for an array.
pub(super) fn constant(call: &mut Call<'_>) -> CallResult {
    const FUNCTION: &str = "Twig\\Extension\\CoreExtension::constant";
    let args = bind(
        call,
        FUNCTION,
        0,
        &[
            Param::required("constant"),
            Param::optional("object", Value::Null),
            Param::optional("checkDefined", false),
        ],
        FunctionKind::User,
    )?;
    let mut args = args.into_iter();
    let mut constant = args.next().unwrap_or_default();
    let object = args.next().unwrap_or_default();
    let check_defined = verify_user_arg(
        args.next().unwrap_or_default(),
        &DeclaredType::builtin(TypeMask::BOOL),
        &UserArg::new(FUNCTION, 3, "checkDefined"),
        call.diagnostics(),
    )?;
    let check_defined = matches!(check_defined, Value::Bool(true));
    let env = call.env().clone();
    let diag = call.diagnostics();

    if !object.is_null() {
        // `'class' === $constant`: `$checkDefined ? true : $object::class`
        let class_constant = matches!(&constant, Value::String(s) if s.as_bytes() == b"class");
        if class_constant && check_defined {
            return Ok(Value::Bool(true));
        }
        // `$object::class`
        let Value::Object(object) = &object else {
            return Err(PhpError::type_error(format!(
                "Cannot use \"::class\" on {}",
                zend_zval_value_name(&object)
            ))
            .into());
        };
        if class_constant {
            return Ok(Value::from(object.class_name()));
        }
        let name = convert::to_string(&constant, diag)?;
        constant = Value::String(PhpString::concat(&[
            object.class_name().as_bytes(),
            b"::",
            name.as_bytes(),
        ]));
    }

    let name = param::parse_str(&constant, &Arg::new("defined", 1, "constant_name"), diag)?;
    if !defined(&env, Scope::CoreExtension, &name) {
        if check_defined {
            return Ok(Value::Bool(false));
        }
        // `strtolower(substr($constant, -7))`
        let tail = param::parse_str(&constant, &Arg::new("substr", 1, "string"), diag)?;
        let tail = &tail.as_bytes()[tail.len().saturating_sub(7)..];
        // `sprintf('%s', $constant)`
        let shown = convert::to_string(&constant, diag)?;
        let shown = shown.to_string_lossy();
        if tail.eq_ignore_ascii_case(b"::class") {
            return Err(Error::runtime(format!(
                "You cannot use the Twig function \"constant\" to access \"{shown}\". You could provide an object and call constant(\"class\", $object) or use the class name directly as a string."
            ))
            .into());
        }
        return Err(Error::runtime(format!("Constant \"{shown}\" is undefined.")).into());
    }
    if check_defined {
        return Ok(Value::Bool(true));
    }
    let name = param::parse_str(&constant, &Arg::new("constant", 1, "name"), diag)?;
    Ok(scoped_constant_value(
        &env,
        Scope::CoreExtension,
        &name,
        diag,
    )?)
}

/// The `string $enum` parameter of `CoreExtension::enum()` / `enumCases()` (a userland parameter in coercive mode).
fn enum_name(call: &mut Call<'_>, function: &str) -> CallResult<PhpString> {
    let args = bind(
        call,
        function,
        0,
        &[Param::required("enum")],
        FunctionKind::User,
    )?;
    let value = verify_user_arg(
        args.into_iter().next().unwrap_or_default(),
        &DeclaredType::builtin(TypeMask::STRING),
        &UserArg::new(function, 1, "enum"),
        call.diagnostics(),
    )?;
    Ok(match value {
        Value::String(name) => name,
        other => convert::to_string(&other, call.diagnostics())?,
    })
}

/// `enum_exists($enum)`: the class, when it exists and is an enum (also what `EnumFunction` and `EnumCasesFunction`
/// check while compiling a literal name).
pub(crate) fn find_enum(env: &Environment, name: &[u8]) -> Option<ClassRef> {
    find_class(env, name).filter(|class| class.is_enum())
}

/// Twig: `CoreExtension::enum(string $enum): \UnitEnum`: the first case of the enum.
///
/// Errors: `"%s" is not an enum.`, `"%s" is an empty enum.` (runtime errors).
pub(super) fn enum_case(call: &mut Call<'_>) -> CallResult {
    let name = enum_name(call, "Twig\\Extension\\CoreExtension::enum")?;
    let Some(class) = find_enum(call.env(), &name) else {
        return Err(
            Error::runtime(format!("\"{}\" is not an enum.", name.to_string_lossy())).into(),
        );
    };
    match class.enum_cases().first() {
        Some(case) => Ok(Value::Object(case.clone())),
        None => {
            Err(Error::runtime(format!("\"{}\" is an empty enum.", name.to_string_lossy())).into())
        }
    }
}

/// Twig: `CoreExtension::enumCases(string $enum): array`: `$enum::cases()`.
///
/// Errors: `Enum "%s" does not exist.` (a runtime error).
pub(super) fn enum_cases(call: &mut Call<'_>) -> CallResult {
    let name = enum_name(call, "Twig\\Extension\\CoreExtension::enumCases")?;
    let Some(class) = find_enum(call.env(), &name) else {
        return Err(Error::runtime(format!(
            "Enum \"{}\" does not exist.",
            name.to_string_lossy()
        ))
        .into());
    };
    Ok(Value::Array(Array::from_values(
        class.enum_cases().iter().cloned().map(Value::Object),
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipped_constants_are_sorted_and_typed() {
        assert!(
            CONSTANTS
                .windows(2)
                .all(|w| w[0].name.as_bytes() < w[1].name.as_bytes())
        );
        let value = |name: &str| php_constant(name.as_bytes()).map(php_constant_value);
        assert_eq!(value("PHP_INT_MAX"), Some(Value::Int(i64::MAX)));
        assert_eq!(value("PHP_INT_MIN"), Some(Value::Int(i64::MIN)));
        assert_eq!(value("M_PI"), Some(Value::Float(std::f64::consts::PI)));
        assert_eq!(value("PHP_EOL"), Some(Value::from("\n")));
        assert_eq!(value("NULL"), Some(Value::Null));
        assert_eq!(value("php_eol"), None);
        assert!(
            php_constant(b"E_STRICT")
                .and_then(|c| c.deprecated)
                .is_some()
        );
    }

    #[test]
    fn splits_class_constants_like_php() {
        assert_eq!(split_class_constant(b"A::B"), Some((&b"A"[..], &b"B"[..])));
        assert_eq!(
            split_class_constant(b"A::B::C"),
            Some((&b"A::B"[..], &b"C"[..]))
        );
        assert_eq!(split_class_constant(b"::B"), Some((&b""[..], &b"B"[..])));
        assert_eq!(split_class_constant(b":B"), None);
        assert_eq!(split_class_constant(b"A:B"), None);
        assert_eq!(split_class_constant(b"PHP_EOL"), None);
    }
}
