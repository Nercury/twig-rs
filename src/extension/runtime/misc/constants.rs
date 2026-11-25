//! PHP constants and enums: the `constant` function (`CoreExtension::constant()`, also behind `constant(...) is
//! defined`) and the `enum` and `enum_cases` functions, over PHP's `defined()`, `constant()` and `enum_exists()`.
//!
//! Constants and classes come from the environment's registry ([`Constants`](crate::runtime::Constants): the
//! `twig-value` built-in classes, the date classes and `DATE_*` constants, and what the host registers), then from
//! the PHP constants twig-rs ships ([`php_constants`](super::php_constants): Core, standard, pcre, json, mbstring,
//! iconv and random, as the target PHP defines them) and the built-in classes the registry does not hold yet (the
//! `RoundingMode` enum and the constants of Twig's own classes, [`builtin_classes`]).

use std::sync::LazyLock;

use twig_value::builtins::format::rounding_mode_class;
use twig_value::object::builtin::ClassTable;
use twig_value::object::{ClassBuilder, ClassRef, FunctionKind, Param, Visibility};
use twig_value::param::{self, Arg, DeclaredType, TypeMask, UserArg, verify_user_arg};
use twig_value::types::zend_zval_value_name;
use twig_value::{Array, Diagnostics, PhpError, PhpResult, PhpString, Value, convert};

use super::bind;
use super::php_constants::{C, CONSTANTS, PhpConstant};
use crate::error::Error;
use crate::extension::runtime::{Call, CallResult};
use crate::runtime::{ARRAY_LIKE_CLASSES, Environment};

/// The classes the environment's registry does not hold yet: PHP 8.4's `enum RoundingMode` and the public constants
/// of Twig's classes (`Twig\Environment::VERSION`, `Twig\Template::ANY_CALL`, ...).
pub(crate) fn builtin_classes() -> &'static ClassTable {
    static TABLE: LazyLock<ClassTable> = LazyLock::new(|| {
        let mut table = ClassTable::new();
        let classes = [
            rounding_mode_class(),
            // TWIG/src/Environment.php
            ClassBuilder::class("Twig\\Environment")
                .constant("VERSION", "3.31.1")
                .constant("VERSION_ID", 33101)
                .constant("MAJOR_VERSION", 3)
                .constant("MINOR_VERSION", 31)
                .constant("RELEASE_VERSION", 1)
                .constant("EXTRA_VERSION", "")
                .build(),
            // TWIG/src/Template.php
            ClassBuilder::class("Twig\\Template")
                .abstract_class()
                .constant("ANY_CALL", "any")
                .constant("ARRAY_CALL", "array")
                .constant("METHOD_CALL", "method")
                .build(),
            // TWIG/src/Extension/CoreExtension.php
            ClassBuilder::class("Twig\\Extension\\CoreExtension")
                .final_class()
                .constant(
                    "ARRAY_LIKE_CLASSES",
                    Array::from_values(ARRAY_LIKE_CLASSES.iter().map(|c| Value::from(*c))),
                )
                .build(),
            // TWIG/src/NodeVisitor/OptimizerNodeVisitor.php
            ClassBuilder::class("Twig\\NodeVisitor\\OptimizerNodeVisitor")
                .final_class()
                .constant("OPTIMIZE_ALL", -1)
                .constant("OPTIMIZE_NONE", 0)
                .constant("OPTIMIZE_FOR", 2)
                .constant("OPTIMIZE_RAW_FILTER", 4)
                .constant("OPTIMIZE_TEXT_NODES", 8)
                .build(),
            // TWIG/src/Loader/FilesystemLoader.php
            ClassBuilder::class("Twig\\Loader\\FilesystemLoader")
                .constant("MAIN_NAMESPACE", "__main__")
                .build(),
        ];
        for class in classes {
            // the names are distinct
            let _ = table.register(class);
        }
        table
    });
    &TABLE
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
    match constant.value {
        C::Null => Value::Null,
        C::Bool(b) => Value::Bool(b),
        C::Int(i) => Value::Int(i),
        C::Float(bits) => Value::Float(f64::from_bits(bits)),
        C::Str(s) => Value::String(PhpString::from(s)),
    }
}

/// A class by name: the environment's registry first, then [`builtin_classes`].
fn find_class(env: &Environment, name: &[u8]) -> Option<ClassRef> {
    let name = String::from_utf8_lossy(name);
    env.with_constants(|constants| constants.class(&name).cloned())
        .or_else(|| builtin_classes().get(&name).cloned())
}

/// Splits `Class::NAME` like `zend_get_constant_ex()`: at the last `:` that follows another `:` (`::B` is the
/// constant `B` of the class ``).
fn split_class_constant(name: &[u8]) -> Option<(&[u8], &[u8])> {
    let colon = name.iter().rposition(|&b| b == b':')?;
    (colon > 0 && name[colon - 1] == b':').then(|| (&name[..colon - 1], &name[colon + 1..]))
}

/// Port of `defined($constant_name)` (`zend_get_constant_ex()` with `ZEND_FETCH_CLASS_SILENT`,
/// PHPSRC/Zend/zend_constants.c) as called from `CoreExtension::constant()`: a class constant needs an existing class
/// and a constant accessible from the scope of `Twig\Extension\CoreExtension` (public; enum cases are public), a
/// global constant must exist (`true`, `false` and `null` in any case always do).
pub(crate) fn defined(env: &Environment, name: &[u8]) -> bool {
    let name = name.strip_prefix(b"\\").unwrap_or(name);
    if let Some((class, constant)) = split_class_constant(name) {
        let Some(class) = find_class(env, class) else {
            return false;
        };
        let constant = String::from_utf8_lossy(constant);
        return match class.constant_info(&constant) {
            Some(info) => info.visibility == Visibility::Public,
            None => class.enum_case(&constant).is_some(),
        };
    }
    php_constant(name).is_some() || env.with_constants(|constants| constants.defined(name))
}

/// Port of `constant($name)` (`zend_get_constant_ex()` with `ZEND_FETCH_CLASS_EXCEPTION`): the value of a defined
/// constant, with the deprecation of a deprecated one (`Constant %s is deprecated since ...`); the `Error`s of an
/// unknown class or constant.
pub(crate) fn constant_value(
    env: &Environment,
    name: &[u8],
    diag: &mut dyn Diagnostics,
) -> PhpResult<Value> {
    let name = name.strip_prefix(b"\\").unwrap_or(name);
    if let Some((class_name, constant)) = split_class_constant(name) {
        let Some(class) = find_class(env, class_name) else {
            return Err(PhpError::error(PhpString::concat(&[
                b"Class \"",
                class_name,
                b"\" not found",
            ])));
        };
        let constant = String::from_utf8_lossy(constant).into_owned();
        let undefined = || {
            PhpError::error(PhpString::concat(&[
                b"Undefined constant ",
                class_name,
                b"::",
                constant.as_bytes(),
            ]))
        };
        if let Some(info) = class.constant_info(&constant) {
            if info.visibility != Visibility::Public {
                return Err(PhpError::error(PhpString::concat(&[
                    b"Cannot access ",
                    info.visibility.as_str().as_bytes(),
                    b" constant ",
                    class_name,
                    b"::",
                    constant.as_bytes(),
                ])));
            }
            // zend_deprecated_class_constant() names the declaring class
            if let Some(suffix) =
                twig_date::classes::class_constant_deprecation(&info.declaring_class, &constant)
            {
                diag.deprecated(format!(
                    "Constant {}::{constant} is deprecated {suffix}",
                    info.declaring_class
                ))?;
            }
        }
        return class.constant(&constant).ok_or_else(undefined);
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
    if !defined(&env, &name) {
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
    Ok(constant_value(&env, &name, diag)?)
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

/// `enum_exists($enum)`: the class, when it exists and is an enum.
fn find_enum(env: &Environment, name: &[u8]) -> Option<ClassRef> {
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
