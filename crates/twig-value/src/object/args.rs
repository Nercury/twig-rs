//! Call arguments: positional and named (PHP 8 named arguments), spread unpacking and parameter
//! binding with PHP's error texts.

use crate::array::Array;
use crate::error::{PhpError, PhpResult};
use crate::key::ArrayKey;
use crate::string::PhpString;
use crate::value::Value;

/// Arguments of a call: positional values followed by named ones (`f(1, 2, name: 3)`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CallArgs {
    /// Positional arguments, in order.
    pub positional: Vec<Value>,
    /// Named arguments, in call order.
    pub named: Vec<(PhpString, Value)>,
}

impl CallArgs {
    /// No arguments.
    pub fn new() -> CallArgs {
        CallArgs::default()
    }

    /// Adds a positional argument.
    pub fn arg(mut self, value: impl Into<Value>) -> CallArgs {
        self.positional.push(value.into());
        self
    }

    /// Adds a named argument.
    pub fn named_arg(mut self, name: impl Into<PhpString>, value: impl Into<Value>) -> CallArgs {
        self.named.push((name.into(), value.into()));
        self
    }

    /// Total number of arguments.
    pub fn len(&self) -> usize {
        self.positional.len() + self.named.len()
    }

    /// Whether there are no arguments.
    pub fn is_empty(&self) -> bool {
        self.positional.is_empty() && self.named.is_empty()
    }

    /// The `i`-th positional argument.
    pub fn get(&self, i: usize) -> Option<&Value> {
        self.positional.get(i)
    }

    /// Argument unpacking `f(...$array)` (`ZEND_SEND_UNPACK`): int keys are positional, string keys
    /// named; a positional element after a named one throws
    /// `Error: Cannot use positional argument after named argument during unpacking`.
    pub fn from_spread(array: &Array) -> PhpResult<CallArgs> {
        let mut args = CallArgs::new();
        for (k, v) in array {
            match k {
                ArrayKey::Int(_) => {
                    if !args.named.is_empty() {
                        return Err(PhpError::error(
                            "Cannot use positional argument after named argument during unpacking",
                        ));
                    }
                    args.positional.push(v.clone());
                }
                ArrayKey::Str(s) => args.named.push((s.clone(), v.clone())),
            }
        }
        Ok(args)
    }

    /// The arguments as the array `__call()` receives: positional values as a list followed by named
    /// arguments under their names.
    pub fn to_array(&self) -> Array {
        let mut a = Array::from_values(self.positional.iter().cloned());
        for (name, v) in &self.named {
            a.insert(ArrayKey::Str(name.clone()), v.clone());
        }
        a
    }

    /// Binds the arguments to `params` with PHP's named-argument rules
    /// (`zend_handle_named_arg()`, `zend_handle_undef_args()`, `zend_wrong_parameters_count_error()`)
    /// and returns one value per parameter. A trailing variadic parameter receives an array of the
    /// remaining positional arguments plus unknown named ones (string keys).
    ///
    /// `function` is the name used in messages (`strlen`, `ArrayObject::offsetGet`).
    ///
    /// Errors (verbatim): `Unknown named parameter $x`, `Named parameter $x overwrites previous
    /// argument` (`Error`); `f(): Argument #2 ($x) not passed`, `f() expects exactly 1 argument, 2
    /// given` / `at least` / `at most` for internal functions, `Too few arguments to function f(), 0
    /// passed and exactly 1 expected` for user functions (PHP adds ` in <file> on line <n>` after
    /// `passed`, which has no equivalent here) (`ArgumentCountError`). User functions ignore extra
    /// positional arguments.
    pub fn bind(
        &self,
        function: &str,
        params: &[Param],
        kind: FunctionKind,
    ) -> PhpResult<Vec<Value>> {
        let variadic = params.last().is_some_and(|p| p.variadic);
        let fixed = if variadic {
            params.len() - 1
        } else {
            params.len()
        };
        let required = params
            .iter()
            .take(fixed)
            .filter(|p| p.default.is_none())
            .count();
        let max = if variadic { usize::MAX } else { fixed };

        if self.positional.len() > fixed && !variadic && kind == FunctionKind::Internal {
            return Err(count_error(function, required, max, self.positional.len()));
        }

        let mut slots: Vec<Option<Value>> = vec![None; fixed];
        let mut rest = Array::new();
        for (i, v) in self.positional.iter().enumerate() {
            if i < fixed {
                slots[i] = Some(v.clone());
            } else if variadic {
                let _ = rest.push(v.clone());
            }
        }
        for (name, v) in &self.named {
            match params
                .iter()
                .take(fixed)
                .position(|p| p.name.as_bytes() == name.as_bytes())
            {
                Some(i) => {
                    if slots[i].is_some() {
                        return Err(PhpError::error(PhpString::concat(&[
                            b"Named parameter $",
                            name.as_bytes(),
                            b" overwrites previous argument",
                        ])));
                    }
                    slots[i] = Some(v.clone());
                }
                None if variadic => {
                    rest.insert(ArrayKey::Str(name.clone()), v.clone());
                }
                None => {
                    return Err(PhpError::error(PhpString::concat(&[
                        b"Unknown named parameter $",
                        name.as_bytes(),
                    ])));
                }
            }
        }

        let mut out = Vec::with_capacity(params.len());
        for (i, slot) in slots.into_iter().enumerate() {
            match slot {
                Some(v) => out.push(v),
                None => match &params[i].default {
                    Some(d) => out.push(d.clone()),
                    None => {
                        if !self.named.is_empty() {
                            return Err(PhpError::argument_count_error(format!(
                                "{}(): Argument #{} (${}) not passed",
                                function,
                                i + 1,
                                params[i].name
                            )));
                        }
                        let given = self.positional.len();
                        return Err(match kind {
                            FunctionKind::Internal => count_error(function, required, max, given),
                            // zend_missing_arg_error(): "exactly" when `required_num_args ==
                            // num_args`; `num_args` does not count a variadic parameter.
                            FunctionKind::User => PhpError::argument_count_error(format!(
                                "Too few arguments to function {}(), {} passed and {} {} expected",
                                function,
                                given,
                                if required == fixed {
                                    "exactly"
                                } else {
                                    "at least"
                                },
                                required
                            )),
                        });
                    }
                },
            }
        }
        if variadic {
            out.push(Value::Array(rest));
        }
        Ok(out)
    }
}

/// `%s() expects %s %d argument%s, %d given` (`zend_wrong_parameters_count_error`).
fn count_error(function: &str, min: usize, max: usize, given: usize) -> PhpError {
    let (word, n) = if min == max {
        ("exactly", min)
    } else if given < min {
        ("at least", min)
    } else {
        ("at most", max)
    };
    PhpError::argument_count_error(format!(
        "{}() expects {} {} argument{}, {} given",
        function,
        word,
        n,
        if n == 1 { "" } else { "s" },
        given
    ))
}

impl From<Vec<Value>> for CallArgs {
    fn from(positional: Vec<Value>) -> CallArgs {
        CallArgs {
            positional,
            named: Vec::new(),
        }
    }
}

/// Whether a callee behaves like an internal (C) function or a userland function for argument-count
/// errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FunctionKind {
    /// Internal function: too many arguments is an error.
    Internal,
    /// Userland function: extra positional arguments are ignored.
    User,
}

/// A parameter for [`CallArgs::bind`].
#[derive(Clone, Debug)]
pub struct Param {
    /// Name without `$`.
    pub name: &'static str,
    /// Default value; `None` = required.
    pub default: Option<Value>,
    /// `...$name` (must be last).
    pub variadic: bool,
}

impl Param {
    /// A required parameter.
    pub fn required(name: &'static str) -> Param {
        Param {
            name,
            default: None,
            variadic: false,
        }
    }

    /// An optional parameter with a default.
    pub fn optional(name: &'static str, default: impl Into<Value>) -> Param {
        Param {
            name,
            default: Some(default.into()),
            variadic: false,
        }
    }

    /// A variadic parameter (`...$name`).
    pub fn variadic(name: &'static str) -> Param {
        Param {
            name,
            default: None,
            variadic: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binding() {
        let params = [
            Param::optional("param1", Value::Null),
            Param::optional("param2", Value::Null),
        ];
        let args = CallArgs::new().named_arg("param2", "x");
        assert_eq!(
            args.bind("bar", &params, FunctionKind::User).unwrap(),
            vec![Value::Null, Value::from("x")]
        );

        let err = CallArgs::new()
            .named_arg("nope", 1)
            .bind("bar", &params, FunctionKind::User)
            .unwrap_err();
        assert_eq!(err.message(), "Unknown named parameter $nope");
        let err = CallArgs::new()
            .arg(1)
            .named_arg("param1", 2)
            .bind("bar", &params, FunctionKind::User)
            .unwrap_err();
        assert_eq!(
            err.message(),
            "Named parameter $param1 overwrites previous argument"
        );

        let p = [Param::required("string")];
        let err = CallArgs::new()
            .bind("strlen", &p, FunctionKind::Internal)
            .unwrap_err();
        assert_eq!(
            err.message(),
            "strlen() expects exactly 1 argument, 0 given"
        );
        let err = CallArgs::new()
            .arg(1)
            .arg(2)
            .bind("strlen", &p, FunctionKind::Internal)
            .unwrap_err();
        assert_eq!(
            err.message(),
            "strlen() expects exactly 1 argument, 2 given"
        );
        let p2 = [Param::required("a"), Param::required("b")];
        let err = CallArgs::new()
            .named_arg("b", 1)
            .bind("f", &p2, FunctionKind::Internal)
            .unwrap_err();
        assert_eq!(err.message(), "f(): Argument #1 ($a) not passed");
        assert_eq!(err.class().name(), "ArgumentCountError");

        let pv = [Param::required("a"), Param::variadic("rest")];
        let out = CallArgs::new()
            .arg(1)
            .arg(2)
            .arg(3)
            .named_arg("k", 4)
            .bind("f", &pv, FunctionKind::Internal)
            .unwrap();
        assert_eq!(out.len(), 2);
        let rest = out[1].as_array().unwrap();
        assert_eq!(rest.len(), 3);
        assert_eq!(rest.get_str(b"k"), Some(&Value::Int(4)));

        let mut spread = Array::new();
        spread.insert("a", 1);
        let _ = spread.push(2);
        let err = CallArgs::from_spread(&spread).unwrap_err();
        assert_eq!(
            err.message(),
            "Cannot use positional argument after named argument during unpacking"
        );
    }

    /// Messages verified with PHP 8.5.13 (`function f($a, ...$r)`, `g($a, $b = 1)`, `h($a, $b)`,
    /// `str_pad()`, `sprintf()`).
    #[test]
    fn arity_messages() {
        let msg = |params: &[Param], args: CallArgs, name: &str, kind: FunctionKind| {
            args.bind(name, params, kind)
                .unwrap_err()
                .message()
                .to_string()
        };
        let f = [Param::required("a"), Param::variadic("r")];
        let g = [Param::required("a"), Param::optional("b", 1)];
        let h = [Param::required("a"), Param::required("b")];
        let user = FunctionKind::User;
        assert_eq!(
            msg(&f, CallArgs::new(), "f", user),
            "Too few arguments to function f(), 0 passed and exactly 1 expected"
        );
        assert_eq!(
            msg(&g, CallArgs::new(), "g", user),
            "Too few arguments to function g(), 0 passed and at least 1 expected"
        );
        assert_eq!(
            msg(&h, CallArgs::new(), "h", user),
            "Too few arguments to function h(), 0 passed and exactly 2 expected"
        );
        assert_eq!(
            msg(&h, CallArgs::new().named_arg("b", 1), "h", user),
            "h(): Argument #1 ($a) not passed"
        );
        let str_pad = [
            Param::required("string"),
            Param::required("length"),
            Param::optional("pad_string", " "),
            Param::optional("pad_type", 1),
        ];
        let internal = FunctionKind::Internal;
        assert_eq!(
            msg(&str_pad, CallArgs::new(), "str_pad", internal),
            "str_pad() expects at least 2 arguments, 0 given"
        );
        let five = CallArgs::from(vec![
            Value::from("a"),
            1.into(),
            " ".into(),
            0.into(),
            5.into(),
        ]);
        assert_eq!(
            msg(&str_pad, five, "str_pad", internal),
            "str_pad() expects at most 4 arguments, 5 given"
        );
        assert_eq!(
            msg(
                &str_pad,
                CallArgs::new().named_arg("length", 3),
                "str_pad",
                internal
            ),
            "str_pad(): Argument #1 ($string) not passed"
        );
        let sprintf = [Param::required("format"), Param::variadic("values")];
        assert_eq!(
            msg(&sprintf, CallArgs::new(), "sprintf", internal),
            "sprintf() expects at least 1 argument, 0 given"
        );
    }
}
