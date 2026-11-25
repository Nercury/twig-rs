//! PHP constants and classes visible to templates: what `constant()`, the `constant` test and `enum()` look up.

use std::collections::HashMap;

use twig_value::object::ClassRef;
use twig_value::object::builtin::ClassTable;
use twig_value::{Diagnostics, PhpError, PhpResult, Value};

/// A global constant: its value and the `#[\Deprecated]` suffix of a deprecated one.
#[derive(Debug, Clone)]
struct GlobalConstant {
    value: Value,
    deprecated: Option<String>,
}

/// The constants and classes of the environment, standing for PHP's constant and class tables.
///
/// Preloaded: the built-in classes of `twig-value` (`ArrayObject`, `ArrayIterator`, `stdClass`, `Closure`, the core
/// interfaces), the date classes (`DateTimeInterface::ATOM`, ...) and the `DATE_*` constants. Hosts register more
/// with [`Environment::add_constant`](super::Environment::add_constant) and
/// [`Environment::add_class`](super::Environment::add_class).
#[derive(Clone)]
pub struct Constants {
    globals: HashMap<Vec<u8>, GlobalConstant>,
    classes: ClassTable,
}

impl std::fmt::Debug for Constants {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Constants")
            .field("globals", &self.globals.len())
            .finish_non_exhaustive()
    }
}

impl Default for Constants {
    fn default() -> Self {
        Constants::new()
    }
}

impl Constants {
    /// The preloaded registry.
    pub fn new() -> Self {
        let mut classes = ClassTable::with_builtins();
        // the date classes are not in the built-in table; registering them cannot clash
        let _ = twig_date::classes::register(&mut classes);
        let mut globals = HashMap::new();
        for constant in twig_date::classes::GLOBAL_CONSTANTS {
            globals.insert(
                constant.name.as_bytes().to_vec(),
                GlobalConstant {
                    value: constant.value.to_value(),
                    deprecated: constant.deprecated.map(str::to_owned),
                },
            );
        }
        Constants { globals, classes }
    }

    /// Defines (or redefines) a global constant.
    pub fn add_constant(&mut self, name: &str, value: Value) {
        self.globals.insert(
            name.as_bytes().to_vec(),
            GlobalConstant {
                value,
                deprecated: None,
            },
        );
    }

    /// Registers a class.
    ///
    /// # Errors
    ///
    /// `Cannot declare class X, because the name is already in use`.
    pub fn add_class(&mut self, class: ClassRef) -> PhpResult<()> {
        self.classes.register(class)
    }

    /// The class table.
    pub fn classes(&self) -> &ClassTable {
        &self.classes
    }

    /// A class by name (case-insensitive).
    pub fn class(&self, name: &str) -> Option<&ClassRef> {
        self.classes.get(name)
    }

    /// Port of `zend_get_constant_ex()` with `ZEND_FETCH_CLASS_EXCEPTION` (PHPSRC/Zend/zend_constants.c), what PHP's
    /// `constant()` does: `Class::NAME` looks up a class constant or enum case (`Class "%s" not found`, `Undefined
    /// constant %s::%s`), any other name a global constant (`Undefined constant "%s"`; `true`, `false` and `null` in
    /// any case). Deprecated constants raise `Constant %s is deprecated since ...`.
    ///
    /// # Errors
    ///
    /// The `Error`s above.
    pub fn constant(&self, name: &[u8], diag: &mut dyn Diagnostics) -> PhpResult<Value> {
        let name = name.strip_prefix(b"\\").unwrap_or(name);
        if let Some(colon) = name.iter().rposition(|&b| b == b':')
            && colon > 0
            && name[colon - 1] == b':'
        {
            let class_name = String::from_utf8_lossy(&name[..colon - 1]).into_owned();
            let constant_name = String::from_utf8_lossy(&name[colon + 1..]).into_owned();
            let Some(class) = self.classes.get(&class_name) else {
                return Err(PhpError::error(format!("Class \"{class_name}\" not found")));
            };
            return match class.constant(&constant_name) {
                Some(value) => {
                    if let Some(suffix) =
                        twig_date::classes::class_constant_deprecation(class.name(), &constant_name)
                    {
                        diag.deprecated(format!(
                            "Constant {}::{constant_name} is deprecated {suffix}",
                            class.name()
                        ))?;
                    }
                    Ok(value)
                }
                None => Err(PhpError::error(format!(
                    "Undefined constant {class_name}::{constant_name}"
                ))),
            };
        }
        if let Some(constant) = self.globals.get(name) {
            if let Some(suffix) = &constant.deprecated {
                diag.deprecated(format!(
                    "Constant {} is deprecated {suffix}",
                    String::from_utf8_lossy(name)
                ))?;
            }
            return Ok(constant.value.clone());
        }
        // zend_get_special_const()
        match name.to_ascii_lowercase().as_slice() {
            b"true" => return Ok(Value::Bool(true)),
            b"false" => return Ok(Value::Bool(false)),
            b"null" => return Ok(Value::Null),
            _ => {}
        }
        Err(PhpError::error(format!(
            "Undefined constant \"{}\"",
            String::from_utf8_lossy(name)
        )))
    }

    /// PHP's `defined()`: whether [`constant`](Self::constant) would find the name.
    pub fn defined(&self, name: &[u8]) -> bool {
        let name = name.strip_prefix(b"\\").unwrap_or(name);
        if let Some(colon) = name.iter().rposition(|&b| b == b':')
            && colon > 0
            && name[colon - 1] == b':'
        {
            let class_name = String::from_utf8_lossy(&name[..colon - 1]).into_owned();
            let constant_name = String::from_utf8_lossy(&name[colon + 1..]).into_owned();
            return self
                .classes
                .get(&class_name)
                .is_some_and(|class| class.constant(&constant_name).is_some());
        }
        self.globals.contains_key(name)
            || matches!(
                name.to_ascii_lowercase().as_slice(),
                b"true" | b"false" | b"null"
            )
    }
}
