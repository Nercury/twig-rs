//! Call arguments: positional and named (PHP 8 named arguments), spread unpacking and parameter
//! binding with PHP's error texts.
//!
//! [`CallArgs::bind`] replays PHP's sending of the arguments (positional ones, then named ones
//! through `zend_handle_named_arg()`) and the checks of the call itself (`zend_handle_undef_args()`,
//! the argument count), so its errors come in PHP's order. A caller that knows the callee while
//! the arguments are sent (an engine unpacking `...` into a call) checks named arguments as PHP
//! sends them with [`CallArgs::send_named`] / [`CallArgs::unpack`] and a [`CalleeParams`].

use crate::array::Array;
use crate::diag::Diagnostics;
use crate::error::{PhpError, PhpResult};
use crate::key::ArrayKey;
use crate::string::PhpString;
use crate::types;
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

    /// Argument unpacking `f(...$array)` of an array (`ZEND_SEND_UNPACK`): int keys are positional,
    /// string keys named; a positional element after a named one throws
    /// `Error: Cannot use positional argument after named argument during unpacking`.
    ///
    /// The callee is not known here, so named elements are checked when the arguments are bound
    /// ([`CallArgs::bind`]). PHP checks them as it unpacks (`zend_handle_named_arg()`), so an unknown
    /// or repeated name before the positional element would be reported instead; a caller that
    /// knows the callee uses [`CallArgs::unpack`] with a [`CalleeParams`].
    pub fn from_spread(array: &Array) -> PhpResult<CallArgs> {
        let mut args = CallArgs::new();
        args.unpack_array(array, None)?;
        Ok(args)
    }

    /// Port of `ZEND_SEND_UNPACK` (PHPSRC/Zend/zend_vm_def.h): appends the elements of `...value`.
    ///
    /// Arrays and `Traversable` objects are unpacked in order (an object iterator through
    /// `rewind`, then `valid`, `current`, `key`, `next` per element); int keys and canonical integer
    /// string keys of an iterator (`ZEND_HANDLE_NUMERIC()`) are positional arguments, other string
    /// keys named arguments. Errors (verbatim): `TypeError: Only arrays and Traversables can be
    /// unpacked, X given`, `Error: Keys must be of type int|string during argument unpacking`,
    /// `Error: Cannot use positional argument after named argument during unpacking` (a positional
    /// element after a named one of the same `...`).
    ///
    /// With the `callee`, every named element is sent with [`CallArgs::send_named`], so its error
    /// (`Unknown named parameter $x`, `Named parameter $x overwrites previous argument`) comes at
    /// that element and stops the unpacking, as in PHP; a positional element after named arguments
    /// of an earlier `...` goes to the next argument slot (`ZEND_CALL_NUM_ARGS() + 1`). Without
    /// it, named elements are checked by [`CallArgs::bind`] and a positional element after any
    /// named argument throws the error above.
    pub fn unpack(
        &mut self,
        value: &Value,
        callee: Option<&CalleeParams<'_>>,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<()> {
        match value {
            Value::Array(array) => self.unpack_array(array, callee),
            Value::Object(object) => {
                let Some(mut iter) = object.get_iterator(diag)? else {
                    return Err(unpack_type_error(value));
                };
                iter.rewind(diag)?;
                let mut have_named = false;
                while iter.valid(diag)? {
                    let element = iter.current(diag)?;
                    let name = match iter.key(diag)? {
                        Value::Int(_) => None,
                        Value::String(name) => match ArrayKey::from_string(name) {
                            ArrayKey::Int(_) => None,
                            ArrayKey::Str(name) => Some(name),
                        },
                        _ => {
                            return Err(PhpError::error(
                                "Keys must be of type int|string during argument unpacking",
                            ));
                        }
                    };
                    match name {
                        Some(name) => {
                            have_named = true;
                            self.send_unpacked_named(name, element, callee)?;
                        }
                        None => self.send_unpacked_positional(element, have_named, callee)?,
                    }
                    iter.next(diag)?;
                }
                Ok(())
            }
            _ => Err(unpack_type_error(value)),
        }
    }

    /// The array branch of `ZEND_SEND_UNPACK`.
    fn unpack_array(&mut self, array: &Array, callee: Option<&CalleeParams<'_>>) -> PhpResult<()> {
        let mut have_named = false;
        for (key, element) in array {
            match key {
                ArrayKey::Str(name) => {
                    have_named = true;
                    self.send_unpacked_named(name.clone(), element.clone(), callee)?;
                }
                ArrayKey::Int(_) => {
                    self.send_unpacked_positional(element.clone(), have_named, callee)?;
                }
            }
        }
        Ok(())
    }

    fn send_unpacked_named(
        &mut self,
        name: PhpString,
        value: Value,
        callee: Option<&CalleeParams<'_>>,
    ) -> PhpResult<()> {
        match callee {
            Some(callee) => self.send_named(name, value, callee),
            None => {
                self.named.push((name, value));
                Ok(())
            }
        }
    }

    /// A positional element of `...`: after a named element of the same `...` it is an error;
    /// after named arguments of an earlier one it takes the next slot.
    fn send_unpacked_positional(
        &mut self,
        value: Value,
        have_named: bool,
        callee: Option<&CalleeParams<'_>>,
    ) -> PhpResult<()> {
        if self.named.is_empty() {
            self.positional.push(value);
            return Ok(());
        }
        match callee {
            Some(callee) if !have_named => self.send_positional_after_named(value, callee),
            _ => Err(PhpError::error(
                "Cannot use positional argument after named argument during unpacking",
            )),
        }
    }

    /// A positional argument sent after named ones (only through two `...`:
    /// `f(...['b' => 1], ...[2])`): PHP puts it in the slot after the last one sent
    /// (`ZEND_CALL_NUM_ARGS() + 1`), which named arguments may have moved past skipped parameters.
    /// A declared parameter's slot is recorded as that parameter's named argument; past the
    /// declared parameters the named arguments are turned into positional ones first.
    fn send_positional_after_named(
        &mut self,
        value: Value,
        callee: &CalleeParams<'_>,
    ) -> PhpResult<()> {
        let sent = self.sent_slots(callee)?;
        let slot = sent.slots.len();
        if let Some(name) = callee.params.get(slot) {
            self.named.push((PhpString::from(*name), value));
            return Ok(());
        }
        let first = callee.leading + self.positional.len();
        if slot == first {
            self.positional.push(value);
            return Ok(());
        }
        // Named arguments filled the declared slots from `first` on, unless they skipped one.
        let filled = callee.params[first..slot]
            .iter()
            .all(|name| self.named.iter().any(|(n, _)| n.as_bytes() == *name));
        if filled {
            for name in &callee.params[first..slot] {
                if let Some(index) = self.named.iter().position(|(n, _)| n.as_bytes() == *name) {
                    let (_, moved) = self.named.remove(index);
                    self.positional.push(moved);
                }
            }
            self.positional.push(value);
            return Ok(());
        }
        // A skipped slot keeps waiting for a later named argument, its default or the call's
        // error. A user function ignores the extra argument; a variadic or internal callee would
        // need it after the skipped slot, which these arguments cannot express.
        if callee.kind == FunctionKind::User && !callee.variadic {
            return Ok(());
        }
        Err(PhpError::error(
            "A positional argument after named arguments that skip a parameter is not supported by twig-rs",
        ))
    }

    /// Port of `zend_handle_named_arg()` (PHPSRC/Zend/zend_execute.c): sends the named argument
    /// `name` to `callee` after the arguments in `self` and records it.
    ///
    /// Errors (`Error`, verbatim) come exactly where PHP sends the argument: `Unknown named
    /// parameter $x` when no declared parameter has the name and the callee is not variadic;
    /// `Named parameter $x overwrites previous argument` when the parameter already has an argument,
    /// or when a variadic callee already collected an unknown argument of that name.
    pub fn send_named(
        &mut self,
        name: PhpString,
        value: Value,
        callee: &CalleeParams<'_>,
    ) -> PhpResult<()> {
        let mut sent = self.sent_slots(callee)?;
        sent.named(&name, Value::Null, callee.offset(&name), callee.variadic)?;
        self.named.push((name, value));
        Ok(())
    }

    /// The argument slots of `callee` filled by `self` (values are not kept).
    fn sent_slots(&self, callee: &CalleeParams<'_>) -> PhpResult<Sent> {
        let mut sent = Sent {
            slots: vec![Some(Value::Null); callee.leading + self.positional.len()],
            extra: Vec::new(),
        };
        for (name, _) in &self.named {
            sent.named(name, Value::Null, callee.offset(name), callee.variadic)?;
        }
        Ok(sent)
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

    /// Binds the arguments to `params` and returns one value per parameter; a trailing variadic
    /// parameter receives an array of the remaining positional arguments followed by the unknown
    /// named ones (string keys).
    ///
    /// The positional arguments are sent first, then the named ones in order, as PHP's
    /// `ZEND_SEND_*` opcodes do: each named argument goes through `zend_handle_named_arg()`
    /// (PHPSRC/Zend/zend_execute.c), which throws `Error: Unknown named parameter $x` (not a
    /// declared parameter of a callee that is not variadic) or `Error: Named parameter $x
    /// overwrites previous argument` (the parameter already has an argument, or a variadic callee
    /// already collected that name). Then, as `ZEND_DO_FCALL` makes the call:
    ///
    /// - `zend_handle_undef_args()`: a parameter a named argument skipped takes its default, or
    ///   throws `ArgumentCountError: f(): Argument #1 ($x) not passed` when it is required;
    /// - internal functions check the count of the arguments sent (`ZEND_CALL_NUM_ARGS()`, the
    ///   skipped parameters included) in `zend_parse_parameters()`:
    ///   `ArgumentCountError: f() expects exactly 1 argument, 2 given` / `at least` / `at most`
    ///   (`zend_wrong_parameters_count_error()`);
    /// - user functions ignore extra positional arguments, and a required parameter after the
    ///   arguments sent throws `ArgumentCountError: Too few arguments to function f(), 1 passed and
    ///   exactly 2 expected` (`zend_missing_arg_error()`; PHP adds ` in <file> on line <n>` after
    ///   `passed` when the caller is userland code, which has no equivalent here).
    ///
    /// A parameter before the last required one is required, whatever its default (PHP's
    /// `required_num_args`; an optional parameter declared before a required one has no default).
    /// `function` is the name used in messages (`strlen`, `ArrayObject::offsetGet`).
    pub fn bind(
        &self,
        function: &str,
        params: &[Param],
        kind: FunctionKind,
    ) -> PhpResult<Vec<Value>> {
        let variadic = params.last().is_some_and(|p| p.variadic);
        let declared = &params[..params.len() - usize::from(variadic)];
        let mut sent = Sent {
            slots: self.positional.iter().cloned().map(Some).collect(),
            extra: Vec::new(),
        };
        for (name, value) in &self.named {
            let offset = declared
                .iter()
                .position(|p| p.name.as_bytes() == name.as_bytes());
            sent.named(name, value.clone(), offset, variadic)?;
        }
        sent.call(function, declared, variadic, kind)
    }
}

/// What PHP's named-argument handling knows about the callee while the arguments of a call are sent
/// (`zend_get_arg_offset_by_name()`, PHPSRC/Zend/zend_execute.c), for [`CallArgs::send_named`] and
/// [`CallArgs::unpack`].
#[derive(Clone, Copy, Debug)]
pub struct CalleeParams<'a> {
    /// The names of the declared parameters (without `$`) before a trailing variadic one, in
    /// order, the parameters of the arguments the caller adds first included.
    pub params: &'a [&'a [u8]],
    /// Whether a trailing variadic parameter collects named arguments of unknown names.
    pub variadic: bool,
    /// The positional arguments the caller sends before [`CallArgs::positional`] (an engine's
    /// injected arguments): they fill the first slots.
    pub leading: usize,
    /// Whether the callee is an internal function (extra positional arguments are an error) or a
    /// userland one (they are ignored); [`CallArgs::bind`] takes the same.
    pub kind: FunctionKind,
}

impl CalleeParams<'_> {
    /// The slot of the parameter `name` (case-sensitive, like `zend_string_equals()`).
    fn offset(&self, name: &PhpString) -> Option<usize> {
        self.params.iter().position(|p| *p == name.as_bytes())
    }
}

/// PHP's call frame while the arguments of a call are sent (`ZEND_SEND_*`, between `ZEND_INIT_*`
/// and `ZEND_DO_*CALL`): the argument slots (`ZEND_CALL_NUM_ARGS()` of them, a slot that a named
/// argument skipped is `UNDEF`, here `None`) and the `extra_named_params` that a variadic callee
/// collects.
struct Sent {
    slots: Vec<Option<Value>>,
    extra: Vec<(PhpString, Value)>,
}

impl Sent {
    /// Port of `zend_handle_named_arg()` (PHPSRC/Zend/zend_execute.c) for the argument `name`,
    /// whose declared parameter is at `offset`.
    fn named(
        &mut self,
        name: &PhpString,
        value: Value,
        offset: Option<usize>,
        variadic: bool,
    ) -> PhpResult<()> {
        match offset {
            Some(i) => {
                if i >= self.slots.len() {
                    self.slots.resize(i + 1, None);
                } else if self.slots[i].is_some() {
                    return Err(overwrites_error(name));
                }
                self.slots[i] = Some(value);
            }
            // An unknown name collected into the variadic parameter: zend_hash_add_empty_element()
            // fails for a name it already holds.
            None if variadic => {
                if self
                    .extra
                    .iter()
                    .any(|(n, _)| n.as_bytes() == name.as_bytes())
                {
                    return Err(overwrites_error(name));
                }
                self.extra.push((name.clone(), value));
            }
            None => {
                return Err(PhpError::error(PhpString::concat(&[
                    b"Unknown named parameter $",
                    name.as_bytes(),
                ])));
            }
        }
        Ok(())
    }

    /// The checks of the call itself (`ZEND_DO_FCALL`): `zend_handle_undef_args()`, then the
    /// argument count of an internal function (`zend_parse_parameters()`) or the missing arguments
    /// of a user function (`ZEND_RECV`), then the values of the parameters.
    fn call(
        self,
        function: &str,
        declared: &[Param],
        variadic: bool,
        kind: FunctionKind,
    ) -> PhpResult<Vec<Value>> {
        let fixed = declared.len();
        let required = declared
            .iter()
            .rposition(|p| p.default.is_none())
            .map_or(0, |i| i + 1);
        let given = self.slots.len();
        let mut slots = self.slots;
        // zend_handle_undef_args(): only slots of declared parameters can be skipped.
        for (i, slot) in slots.iter_mut().enumerate() {
            if slot.is_none() {
                let param = &declared[i];
                match &param.default {
                    Some(default) if i >= required => *slot = Some(default.clone()),
                    _ => {
                        return Err(PhpError::argument_count_error(format!(
                            "{}(): Argument #{} (${}) not passed",
                            function,
                            i + 1,
                            param.name
                        )));
                    }
                }
            }
        }
        match kind {
            FunctionKind::Internal => {
                let max = if variadic { usize::MAX } else { fixed };
                if given < required || given > max {
                    return Err(count_error(function, required, max, given));
                }
            }
            // zend_missing_arg_error(): "exactly" when `required_num_args == num_args`, which does
            // not count a variadic parameter.
            FunctionKind::User => {
                if given < required {
                    return Err(PhpError::argument_count_error(format!(
                        "Too few arguments to function {}(), {} passed and {} {} expected",
                        function,
                        given,
                        if required == fixed {
                            "exactly"
                        } else {
                            "at least"
                        },
                        required
                    )));
                }
            }
        }
        let mut values = slots.into_iter().map(Option::unwrap_or_default);
        let mut out = Vec::with_capacity(fixed + usize::from(variadic));
        for param in declared {
            out.push(match values.next() {
                Some(value) => value,
                None => param.default.clone().unwrap_or_default(),
            });
        }
        if variadic {
            let mut rest = Array::new();
            for value in values {
                let _ = rest.push(value);
            }
            for (name, value) in self.extra {
                rest.insert(ArrayKey::Str(name), value);
            }
            out.push(Value::Array(rest));
        }
        Ok(out)
    }
}

fn overwrites_error(name: &PhpString) -> PhpError {
    PhpError::error(PhpString::concat(&[
        b"Named parameter $",
        name.as_bytes(),
        b" overwrites previous argument",
    ]))
}

/// `Only arrays and Traversables can be unpacked, %s given` (`ZEND_SEND_UNPACK`).
fn unpack_type_error(value: &Value) -> PhpError {
    PhpError::type_error(format!(
        "Only arrays and Traversables can be unpacked, {} given",
        types::zend_zval_value_name(value)
    ))
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
