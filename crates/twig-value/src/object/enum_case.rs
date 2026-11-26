//! Enum cases (`enum Suit: string { case Hearts = 'H'; }`).

use std::sync::Arc;

use super::class::{ClassKind, ClassRef, EnumBacking};
use super::props::PropertyStore;
use super::{CallArgs, FunctionKind, Object, ObjectRef, Param};
use crate::array::Array;
use crate::diag::Diagnostics;
use crate::error::{PhpError, PhpResult};
use crate::param::{self, Arg, StrOrLong};
use crate::string::PhpString;
use crate::value::Value;

/// One case of an enum: a singleton object of the enum class with the readonly properties `name`
/// (and `value` for backed enums).
///
/// Behaviour (PHP 8.5): uncomparable except to itself, uncloneable (`Trying to clone an uncloneable
/// object of class X`), no string conversion, truthy, no dynamic properties; methods `cases()`,
/// `from()`, `tryFrom()` (callable through an instance, like `$case->cases()`).
pub struct EnumCase {
    class: ClassRef,
    name: Arc<str>,
    value: Option<Value>,
    props: PropertyStore,
}

impl EnumCase {
    pub(crate) fn new_object(class: &ClassRef, name: Arc<str>, value: Option<Value>) -> ObjectRef {
        let props = PropertyStore::new(class);
        props.set(b"name", Value::from(&*name));
        if let Some(v) = &value {
            props.set(b"value", v.clone());
        }
        ObjectRef::new(EnumCase {
            class: class.clone(),
            name,
            value,
            props,
        })
    }

    /// The case name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The backing value (backed enums only).
    pub fn value(&self) -> Option<&Value> {
        self.value.as_ref()
    }
}

/// Port of `zend_enum_from_base()` (PHPSRC/Zend/zend_enum.c): `Enum::from()` / `Enum::tryFrom()` in
/// coercive mode. Int-backed enums take an `int` parameter, string-backed ones `string|int` (ints
/// are converted to strings). A missing value throws
/// `ValueError: 7 is not a valid backing value for enum I` /
/// `ValueError: "X" is not a valid backing value for enum Suit` (or returns `null` for `tryFrom`).
pub fn enum_from(
    class: &ClassRef,
    value: &Value,
    try_from: bool,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Value> {
    let backing = match class.kind() {
        ClassKind::Enum(Some(b)) => b,
        _ => {
            return Err(super::undefined_method(
                class,
                if try_from { "tryFrom" } else { "from" },
            ));
        }
    };
    let fname = format!(
        "{}::{}",
        class.name(),
        if try_from { "tryFrom" } else { "from" }
    );
    let arg = Arg::new(&fname, 1, "value");
    let found = match backing {
        EnumBacking::Int => {
            let key = param::parse_long(value, &arg, diag)?;
            let case = class
                .enum_cases()
                .iter()
                .find(|c| c.enum_case_value() == Some(&Value::Int(key)))
                .cloned();
            match case {
                Some(c) => Ok(c),
                None => Err(PhpError::value_error(format!(
                    "{} is not a valid backing value for enum {}",
                    key,
                    class.name()
                ))),
            }
        }
        EnumBacking::String => {
            let key = match param::parse_str_or_long(value, &arg.declared("string|int"), diag)? {
                StrOrLong::Str(s) => s,
                StrOrLong::Long(l) => PhpString::from(l.to_string()),
            };
            let case = class
                .enum_cases()
                .iter()
                .find(|c| matches!(c.enum_case_value(), Some(Value::String(s)) if *s == key))
                .cloned();
            match case {
                Some(c) => Ok(c),
                None => Err(PhpError::value_error(PhpString::concat(&[
                    b"\"",
                    key.c_str(),
                    b"\" is not a valid backing value for enum ",
                    class.name().as_bytes(),
                ]))),
            }
        }
    };
    match found {
        Ok(c) => Ok(Value::Object(c)),
        Err(e) if try_from && e.class().name() == "ValueError" => Ok(Value::Null),
        Err(e) => Err(e),
    }
}

impl Object for EnumCase {
    fn class(&self) -> &ClassRef {
        &self.class
    }

    /// The internal methods every enum gets (`zend_enum_register_funcs()`,
    /// PHPSRC/Zend/zend_enum.c): `cases()`, and `from(int|string $value)` /
    /// `tryFrom(int|string $value)` for a backed enum.
    fn method_params(&self, method: &str) -> Option<super::MethodParams> {
        let params = match method {
            "cases" => Vec::new(),
            "from" | "tryfrom" if self.value.is_some() => vec![Param::required("value")],
            _ => return None,
        };
        Some(super::MethodParams::internal(params))
    }

    fn call(
        &self,
        _this: &ObjectRef,
        method: &str,
        args: CallArgs,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        match method {
            "cases" => {
                args.bind(
                    &format!("{}::cases", self.class.name()),
                    &[],
                    FunctionKind::Internal,
                )?;
                Ok(Value::Array(Array::from_values(
                    self.class.enum_cases().iter().cloned().map(Value::Object),
                )))
            }
            "from" | "tryfrom" if self.value.is_some() => {
                let try_from = method == "tryfrom";
                let fname = format!(
                    "{}::{}",
                    self.class.name(),
                    if try_from { "tryFrom" } else { "from" }
                );
                let bound =
                    args.bind(&fname, &[Param::required("value")], FunctionKind::Internal)?;
                enum_from(&self.class, &bound[0], try_from, diag)
            }
            _ => Err(super::undefined_method(&self.class, method)),
        }
    }

    fn property_store(&self) -> Option<&PropertyStore> {
        Some(&self.props)
    }

    /// `zend_objects_not_comparable()`.
    fn compare(
        &self,
        _this: &ObjectRef,
        _other: &Value,
        _this_is_left: bool,
        _compare_values: &mut dyn FnMut(&Value, &Value) -> PhpResult<i32>,
    ) -> Option<PhpResult<i32>> {
        Some(Ok(1))
    }
}
