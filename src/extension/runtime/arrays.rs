//! The array and iteration group: the sequence and mapping callables of `CoreExtension`
//! (TWIG/src/Extension/CoreExtension.php, reference/twig/core-extension.md §4-§7).
//!
//! | callable | PHP callable |
//! |---|---|
//! | filters `batch`, `column`, `filter`, `find`, `first`, `invoke`, `join`, `keys`, `last`, `length`, `map`, `merge`, `reduce`, `reverse`, `shuffle`, `slice` (also `x[a:b]`), `sort`, `split` | `CoreExtension::<name>` |
//! | functions `cycle`, `random` | `CoreExtension::cycle`, `CoreExtension::random` |
//! | functions `max`, `min`, `range` | PHP's `max()`, `min()`, `range()` |
//! | tests `empty`, `iterable`, `mapping`, `sequence` | `CoreExtension::testEmpty`, `is_iterable()`, `CoreExtension::testMapping`, `CoreExtension::testSequence` |
//!
//! Each runtime ports its PHP method in upstream's evaluation order: arguments are bound like PHP binds them to
//! the userland method (or internal function), the PHP functions the method calls are the `twig-value` builtins
//! with their argument coercion and verbatim messages, and `Traversable` values are consumed through the object
//! protocol with PHP's call order (`IteratorAggregate::getIterator()` method calls, `foreach`, `iterator_to_array()`,
//! the SPL iterators of [`iterators`]).

mod callables;
mod iterators;
mod random;
mod text;

pub(crate) use random::with_mt_rand;

use twig_value::builtins::array::{self as php_array, SORT_REGULAR};
use twig_value::builtins::string::{explode, implode_frameless};
use twig_value::object::{CallArgs, FunctionKind, ObjectRef, Param};
use twig_value::param::{self, Arg, DeclaredType, TypeMask, UserArg, verify_user_arg};
use twig_value::types::get_debug_type;
use twig_value::{
    Array, ArrayKey, Diagnostics, FetchMode, KeyContext, Number, PhpError, PhpString, Value,
    compare, convert, dim, object, ops,
};

use super::{Call, CallResult, RuntimeTable};
use crate::deprecation::Deprecation;
use crate::error::Error;
use crate::lltl::Instr;
use crate::runtime::core::{check_arrow, for_each, test_empty, to_array};
use crate::vm::{Exception, invoke_value};

use callables::{call_value, check_callable};
use text::{convert_encoding, mb_strlen, mb_substr, split_into_characters};

/// The class of the core extension, the prefix of the reflection names of its callables.
const CORE: &str = "Twig\\Extension\\CoreExtension";

/// Registers the runtimes of the array group.
pub(crate) fn register(table: &mut RuntimeTable) {
    table.register("Twig\\Extension\\CoreExtension::join", join);
    table.register("Twig\\Extension\\CoreExtension::split", split);
    table.register("Twig\\Extension\\CoreExtension::sort", sort);
    table.register("Twig\\Extension\\CoreExtension::merge", merge);
    table.register("Twig\\Extension\\CoreExtension::batch", batch);
    table.register("Twig\\Extension\\CoreExtension::column", column);
    table.register("Twig\\Extension\\CoreExtension::filter", filter);
    table.register("Twig\\Extension\\CoreExtension::map", map);
    table.register("Twig\\Extension\\CoreExtension::reduce", reduce);
    table.register("Twig\\Extension\\CoreExtension::find", find);
    table.register("Twig\\Extension\\CoreExtension::reverse", reverse);
    table.register("Twig\\Extension\\CoreExtension::shuffle", shuffle);
    table.register("Twig\\Extension\\CoreExtension::length", length);
    table.register("Twig\\Extension\\CoreExtension::slice", slice);
    table.register("Twig\\Extension\\CoreExtension::first", first);
    table.register("Twig\\Extension\\CoreExtension::last", last);
    table.register("Twig\\Extension\\CoreExtension::keys", keys);
    table.register("Twig\\Extension\\CoreExtension::invoke", invoke);
    table.register("Twig\\Extension\\CoreExtension::cycle", cycle);
    table.register("Twig\\Extension\\CoreExtension::random", random_function);
    table.register(
        "Twig\\Extension\\CoreExtension::testEmpty",
        test_empty_runtime,
    );
    table.register(
        "Twig\\Extension\\CoreExtension::testSequence",
        test_sequence,
    );
    table.register("Twig\\Extension\\CoreExtension::testMapping", test_mapping);
    table.register("max", max);
    table.register("min", min);
    table.register("range", range);
    table.register("is_iterable", is_iterable);
}

/// Binds the arguments of a call to the parameters of a userland `CoreExtension` method with PHP's rules (extra
/// positional arguments are ignored, named arguments of unpacked string keys are matched by name, a trailing
/// variadic collects the rest) and returns the values of `params`.
///
/// `injected` names the parameters Twig fills before the template's arguments (`$charset`, `$env`,
/// `$isSandboxed`): they count in PHP's messages (`Too few arguments to function
/// Twig\Extension\CoreExtension::map(), 3 passed and exactly 4 expected`) and a named argument of the same name
/// overwrites them (`Named parameter $env overwrites previous argument`).
///
/// # Errors
///
/// The `Error`s and `ArgumentCountError`s of PHP's binding.
fn bind_method(
    call: &Call<'_>,
    method: &str,
    injected: &[&'static str],
    params: &[Param],
) -> CallResult<Vec<Value>> {
    let mut all: Vec<Param> = injected.iter().map(|name| Param::required(name)).collect();
    all.extend_from_slice(params);
    let mut positional = vec![Value::Null; injected.len()];
    positional.extend_from_slice(call.args());
    let args = CallArgs {
        positional,
        named: call.named_args().to_vec(),
    };
    let mut bound = args.bind(&format!("{CORE}::{method}"), &all, FunctionKind::User)?;
    Ok(bound.split_off(injected.len()))
}

/// Takes the bound values in order.
fn take<const N: usize>(bound: Vec<Value>) -> [Value; N] {
    let mut values = bound.into_iter();
    std::array::from_fn(|_| values.next().unwrap_or_default())
}

/// `RuntimeError('The "<name>" <kind> expects a sequence or a mapping, got "<type>".')`.
fn expects_iterable(name: &str, kind: &str, value: &Value) -> Exception {
    Error::runtime(format!(
        "The \"{name}\" {kind} expects a sequence or a mapping, got \"{}\".",
        get_debug_type(value)
    ))
    .into()
}

/// `iterator_to_array($object)` (keys preserved, through `array_set_zval_key()`).
fn object_to_array(object: &ObjectRef, diag: &mut dyn Diagnostics) -> CallResult<Array> {
    Ok(object::iterator_to_array(object, true, diag)?)
}

/// Twig: `CoreExtension::toArray($seq, $preserveKeys)` with the flag as a PHP value: a `Traversable` goes through
/// `iterator_to_array($seq, $preserveKeys)` (whose `bool` parameter is coerced, with its `TypeError` and `null`
/// deprecation), an array is kept or listed by the flag's truthiness, anything else is returned unchanged.
fn to_array_with(
    seq: Value,
    preserve_keys: &Value,
    diag: &mut dyn Diagnostics,
) -> CallResult<Value> {
    match seq {
        Value::Object(object) if object.instance_of("Traversable") => Ok(Value::Array(
            php_array::iterator_to_array(&Value::Object(object), Some(preserve_keys), diag)?,
        )),
        Value::Array(array) => {
            if convert::to_bool(preserve_keys, diag)? {
                Ok(Value::Array(array))
            } else {
                Ok(Value::Array(Array::from_values(array.values().cloned())))
            }
        }
        other => Ok(other),
    }
}

/// A userland method's return type `string` in coercive mode (`zend_verify_return_type()`): scalars and
/// `Stringable` objects are converted, anything else throws `<method>(): Return value must be of type string, X
/// returned`.
fn return_string(method: &str, value: Value, diag: &mut dyn Diagnostics) -> CallResult {
    match &value {
        Value::String(_) => Ok(value),
        Value::Int(_) | Value::Float(_) | Value::Bool(_) => {
            Ok(Value::String(convert::to_string(&value, diag)?))
        }
        Value::Object(o) if o.instance_of("Stringable") => {
            Ok(Value::String(o.to_php_string(diag)?))
        }
        other => Err(PhpError::type_error(format!(
            "{CORE}::{method}(): Return value must be of type string, {} returned",
            twig_value::types::zend_zval_value_name(other)
        ))
        .into()),
    }
}

/// Twig: `CoreExtension::join($value, $glue = '', $and = null): string`.
///
/// Non-iterables become `[]` (`null`) or `[$value]`; `implode()` is PHP 8.4's frameless two-argument variant (Twig
/// calls it directly); a single element with a distinct `$and` is returned through the `string` return type.
fn join(call: &mut Call<'_>) -> CallResult {
    let [value, glue, and] = take(bind_method(
        call,
        "join",
        &[],
        &[
            Param::required("value"),
            Param::optional("glue", ""),
            Param::optional("and", Value::Null),
        ],
    )?);
    let diag = call.diagnostics();
    let value = if value.is_iterable() {
        value
    } else if value.is_null() {
        Value::Array(Array::new())
    } else {
        Value::Array(Array::from_values([value]))
    };
    let value = to_array(value, false, diag)?;
    let Value::Array(items) = &value else {
        return Ok(Value::from(""));
    };
    if items.is_empty() {
        return Ok(Value::from(""));
    }
    if and.is_null() || and.identical(&glue) {
        return Ok(Value::String(implode_frameless(&glue, Some(&value), diag)?));
    }
    if items.len() == 1 {
        let only = items.get_int(0).cloned().unwrap_or_default();
        return return_string("join", only, diag);
    }
    // implode($glue, array_slice($value, 0, -1)).$and.$value[count($value) - 1]
    let head = php_array::array_slice(&value, &Value::Int(0), Some(&Value::Int(-1)), None, diag)?;
    let joined = implode_frameless(&glue, Some(&Value::Array(head)), diag)?;
    let with_and = ops::concat(&Value::String(joined), &and, diag)?;
    let last = dim::array_read(
        items,
        &Value::Int(items.len() as i64 - 1),
        FetchMode::Read,
        diag,
    )?;
    Ok(Value::String(ops::concat(
        &Value::String(with_and),
        &last,
        diag,
    )?))
}

/// Twig: `CoreExtension::split(string $charset, $value, $delimiter, $limit = null): array`.
///
/// A non-empty delimiter (strictly `'' !== $delimiter`) is `explode()`; otherwise the value is cut into characters
/// (`splitIntoCharacters()`) when `$limit <= 1`, or into chunks of `$limit` characters with `mb_strlen()` and
/// `mb_substr()` in the environment charset; comparisons and `$i += $limit` are PHP's operators.
fn split(call: &mut Call<'_>) -> CallResult {
    let [value, delimiter, limit] = take(bind_method(
        call,
        "split",
        &["charset"],
        &[
            Param::required("value"),
            Param::required("delimiter"),
            Param::optional("limit", Value::Null),
        ],
    )?);
    let charset = call.charset();
    let diag = call.diagnostics();
    let value = if value.is_null() {
        Value::from("")
    } else {
        value
    };
    if !matches!(&delimiter, Value::String(s) if s.is_empty()) {
        let limit = (!limit.is_null()).then_some(&limit);
        return Ok(Value::Array(explode(&delimiter, &value, limit, diag)?));
    }
    if compare::less_or_equal(&limit, &Value::Int(1), diag)? {
        // private static function splitIntoCharacters(string $string, string $name): array
        let string = match verify_user_arg(
            value,
            &DeclaredType::builtin(TypeMask::STRING),
            &UserArg::new(
                "Twig\\Extension\\CoreExtension::splitIntoCharacters",
                1,
                "string",
            ),
            diag,
        )? {
            Value::String(s) => s,
            other => convert::to_string(&other, diag)?,
        };
        let characters = split_into_characters(&string, "split")?;
        return Ok(Value::Array(Array::from_values(
            characters.into_iter().map(Value::String),
        )));
    }
    let length = Value::Int(mb_strlen(&value, &charset, diag)?);
    if compare::less(&length, &limit, diag)? {
        return Ok(Value::Array(Array::from_values([value])));
    }
    let mut result = Array::new();
    let mut i = Value::Int(0);
    while compare::less(&i, &length, diag)? {
        let chunk = mb_substr(&value, &i, &limit, &charset, diag)?;
        result.push(Value::String(chunk))?;
        i = ops::add(&i, &limit, diag)?;
    }
    Ok(Value::Array(result))
}

/// Twig: `CoreExtension::sort(Environment $env, bool $isSandboxed, $array, $arrow = null): array`.
///
/// A `Traversable` is converted with `iterator_to_array()` (keys preserved); with an arrow (`checkArrow()`, then
/// `uasort()`'s `callable` check) the values are sorted by the arrow (`zval_get_long()` of its result, PHP's stable
/// `zend_sort`), otherwise with `asort()` (`SORT_REGULAR`); keys are kept.
fn sort(call: &mut Call<'_>) -> CallResult {
    let [array, arrow] = take(bind_method(
        call,
        "sort",
        &["env", "isSandboxed"],
        &[
            Param::required("array"),
            Param::optional("arrow", Value::Null),
        ],
    )?);
    let sandboxed = call.is_sandboxed();
    let mut array = match array {
        Value::Object(object) if object.instance_of("Traversable") => {
            object_to_array(&object, call.diagnostics())?
        }
        Value::Array(array) => array,
        other => return Err(expects_iterable("sort", "filter", &other)),
    };
    if arrow.is_null() {
        php_array::asort(&mut array, SORT_REGULAR, call.diagnostics())?;
    } else {
        check_arrow(call.state(), sandboxed, &arrow, "sort", "filter")?;
        check_callable(&arrow, "uasort", 2, "callback", false)?;
        php_array::uasort(
            &mut array,
            &mut |args: &[Value], diag: &mut dyn Diagnostics| {
                call_value(&arrow, args.to_vec(), diag)
            },
            call.diagnostics(),
        )?;
    }
    Ok(Value::Array(array))
}

/// Twig: `CoreExtension::merge(...$arrays): array`: `array_merge()` of the `toArray()` of every argument, the
/// input first; a non-iterable throws `The "merge" filter expects a sequence or a mapping, got "%s" for argument
/// %d.` with `$argNumber + 1` (the variadic's key: a named argument of a spread makes it a string).
fn merge(call: &mut Call<'_>) -> CallResult {
    let [arrays] = take(bind_method(
        call,
        "merge",
        &[],
        &[Param::variadic("arrays")],
    )?);
    let diag = call.diagnostics();
    let arrays = match arrays {
        Value::Array(arrays) => arrays,
        _ => Array::new(),
    };
    let mut result = Array::new();
    for (arg_number, array) in &arrays {
        if !array.is_iterable() {
            let debug_type = get_debug_type(array);
            let number = ops::add(&arg_number.to_value(), &Value::Int(1), diag)?;
            let number = convert::to_long(&number, diag)?;
            return Err(Error::runtime(format!(
                "The \"merge\" filter expects a sequence or a mapping, got \"{debug_type}\" for argument {number}."
            ))
            .into());
        }
        let array = to_array(array.clone(), true, diag)?;
        result = php_array::array_merge(&[Value::Array(result), array])?;
    }
    Ok(Value::Array(result))
}

/// Twig: `CoreExtension::batch($items, $size, $fill = null, $preserveKeys = true): array`.
///
/// `$size = (int) ceil($size)` (`ceil()`'s `int|float` parameter: `ceil(): Argument #1 ($num) must be of type
/// int|float, string given`), `array_chunk(toArray($items, $preserveKeys), $size, $preserveKeys)`, then the last
/// row is filled with `$fill` (appended with the row's next integer key) when `$fill` is not `null`.
fn batch(call: &mut Call<'_>) -> CallResult {
    let [items, size, fill, preserve_keys] = take(bind_method(
        call,
        "batch",
        &[],
        &[
            Param::required("items"),
            Param::required("size"),
            Param::optional("fill", Value::Null),
            Param::optional("preserveKeys", true),
        ],
    )?);
    let diag = call.diagnostics();
    if !items.is_iterable() {
        return Err(expects_iterable("batch", "filter", &items));
    }
    let ceiled = match param::parse_number(&size, &Arg::new("ceil", 1, "num"), diag)? {
        Number::Int(i) => i as f64,
        Number::Float(f) => twig_value::math::ceil(f),
    };
    let size = convert::to_long(&Value::Float(ceiled), diag)?;
    let items = to_array_with(items, &preserve_keys, diag)?;
    let mut result = php_array::array_chunk(&items, &Value::Int(size), Some(&preserve_keys), diag)?;
    if !fill.is_null() && !result.is_empty() {
        let last = result.len() as i64 - 1;
        let row_len = match result.get_int(last) {
            Some(Value::Array(row)) => row.len() as i64,
            _ => 0,
        };
        let fill_count = size - row_len;
        if fill_count != 0
            && let Some(Value::Array(row)) = result.get_mut(&ArrayKey::Int(last))
        {
            for _ in 0..fill_count {
                row.push(fill.clone())?;
            }
        }
    }
    Ok(Value::Array(result))
}

/// Twig: `CoreExtension::column(Environment $env, bool $isSandboxed, $array, $name, $index = null): array`:
/// `array_column()` of the array (a `Traversable` through `iterator_to_array()`).
///
/// In the sandbox every object row is first checked against the security policy itself (not the checker, so the
/// error has no source or line until the template handles it): `checkPropertyAllowed($item, (string) $name)`, and
/// the same for `$index` when it is not `null`.
fn column(call: &mut Call<'_>) -> CallResult {
    let [array, name, index] = take(bind_method(
        call,
        "column",
        &["env", "isSandboxed"],
        &[
            Param::required("array"),
            Param::required("name"),
            Param::optional("index", Value::Null),
        ],
    )?);
    if !array.is_iterable() {
        return Err(expects_iterable("column", "filter", &array));
    }
    let policy = if call.is_sandboxed() {
        call.env()
            .sandbox_checker()
            .map(|checker| checker.security_policy())
    } else {
        None
    };
    let diag = call.diagnostics();
    let array = match array {
        Value::Object(object) => Value::Array(object_to_array(&object, diag)?),
        other => other,
    };
    if let (Some(policy), Value::Array(items)) = (&policy, &array) {
        for item in items.values() {
            let Value::Object(object) = item else {
                continue;
            };
            let property = Value::String(convert::to_string(&name, diag)?);
            policy
                .check_property_allowed(object, &property)
                .map_err(|violation| Exception::from(violation.into_error()))?;
            if !index.is_null() {
                let property = Value::String(convert::to_string(&index, diag)?);
                policy
                    .check_property_allowed(object, &property)
                    .map_err(|violation| Exception::from(violation.into_error()))?;
            }
        }
    }
    Ok(Value::Array(php_array::array_column(
        &array,
        &name,
        Some(&index),
        diag,
    )?))
}

/// Twig: `CoreExtension::filter(Environment $env, bool $isSandboxed, $array, $arrow)`.
///
/// An array goes through `array_filter($array, $arrow, ARRAY_FILTER_USE_BOTH)` (keys kept; `?callable` check, so a
/// `null` arrow keeps the truthy values); a `Traversable` gives a lazy `CallbackFilterIterator` over an
/// `IteratorIterator` whose callback receives `($value, $key, $iterator)`.
fn filter(call: &mut Call<'_>) -> CallResult {
    let [array, arrow] = take(bind_method(
        call,
        "filter",
        &["env", "isSandboxed"],
        &[Param::required("array"), Param::required("arrow")],
    )?);
    if !array.is_iterable() {
        return Err(Error::runtime(format!(
            "The \"filter\" filter expects a sequence/mapping or \"Traversable\", got \"{}\".",
            get_debug_type(&array)
        ))
        .into());
    }
    let sandboxed = call.is_sandboxed();
    check_arrow(call.state(), sandboxed, &arrow, "filter", "filter")?;
    let diag = call.diagnostics();
    match &array {
        Value::Object(object) => Ok(iterators::callback_filter_iterator(object, &arrow, diag)?),
        _ => {
            check_callable(&arrow, "array_filter", 2, "callback", true)?;
            let filtered = if arrow.is_null() {
                php_array::array_filter(
                    &array,
                    None,
                    Some(&Value::Int(php_array::ARRAY_FILTER_USE_BOTH)),
                    diag,
                )?
            } else {
                php_array::array_filter(
                    &array,
                    Some(&mut |args: &[Value], diag: &mut dyn Diagnostics| {
                        call_value(&arrow, args.to_vec(), diag)
                    }),
                    Some(&Value::Int(php_array::ARRAY_FILTER_USE_BOTH)),
                    diag,
                )?
            };
            Ok(Value::Array(filtered))
        }
    }
}

/// Twig: `CoreExtension::map(Environment $env, bool $isSandboxed, $array, $arrow)`: `$r[$k] = $arrow($v, $k)` for
/// every element (`foreach`), keys preserved (a repeated key of a `Traversable` keeps its first position).
fn map(call: &mut Call<'_>) -> CallResult {
    let [array, arrow] = take(bind_method(
        call,
        "map",
        &["env", "isSandboxed"],
        &[Param::required("array"), Param::required("arrow")],
    )?);
    if !array.is_iterable() {
        return Err(expects_iterable("map", "filter", &array));
    }
    let sandboxed = call.is_sandboxed();
    let state = call.state();
    check_arrow(state, sandboxed, &arrow, "map", "filter")?;
    let mut result = Array::new();
    for_each(state, &array, |state, key, value| {
        let mapped = invoke_value(state, &arrow, vec![value, key.clone()])?;
        let key = ArrayKey::from_value(&key, KeyContext::Write, state)?;
        result.insert(key, mapped);
        Ok(true)
    })?;
    Ok(Value::Array(result))
}

/// Twig: `CoreExtension::reduce(Environment $env, bool $isSandboxed, $array, $arrow, $initial = null)`:
/// `$accumulator = $arrow($accumulator, $value, $key)` for every element, from `$initial`.
fn reduce(call: &mut Call<'_>) -> CallResult {
    let [array, arrow, initial] = take(bind_method(
        call,
        "reduce",
        &["env", "isSandboxed"],
        &[
            Param::required("array"),
            Param::required("arrow"),
            Param::optional("initial", Value::Null),
        ],
    )?);
    if !array.is_iterable() {
        return Err(expects_iterable("reduce", "filter", &array));
    }
    let sandboxed = call.is_sandboxed();
    let state = call.state();
    check_arrow(state, sandboxed, &arrow, "reduce", "filter")?;
    let mut accumulator = initial;
    for_each(state, &array, |state, key, value| {
        let carry = std::mem::take(&mut accumulator);
        accumulator = invoke_value(state, &arrow, vec![carry, value, key])?;
        Ok(true)
    })?;
    Ok(accumulator)
}

/// Twig: `CoreExtension::find(Environment $env, bool $isSandboxed, $array, $arrow)`: the first value for which
/// `$arrow($v, $k)` is truthy, else `null`.
fn find(call: &mut Call<'_>) -> CallResult {
    let [array, arrow] = take(bind_method(
        call,
        "find",
        &["env", "isSandboxed"],
        &[Param::required("array"), Param::required("arrow")],
    )?);
    if !array.is_iterable() {
        return Err(expects_iterable("find", "filter", &array));
    }
    let sandboxed = call.is_sandboxed();
    let state = call.state();
    check_arrow(state, sandboxed, &arrow, "find", "filter")?;
    let mut found = Value::Null;
    for_each(state, &array, |state, key, value| {
        let returned = invoke_value(state, &arrow, vec![value.clone(), key])?;
        if convert::to_bool(&returned, state)? {
            found = value;
            return Ok(false);
        }
        Ok(true)
    })?;
    Ok(found)
}

/// Twig: `CoreExtension::reverse(string $charset, $item, $preserveKeys = false)`.
///
/// A `Traversable` (through `iterator_to_array()`, keys preserved first) or an array goes through
/// `array_reverse($item, $preserveKeys)`; anything else is cast to a string and reversed per code point
/// (`splitIntoCharacters()`), converted from and back to the charset with iconv when it is not UTF-8.
fn reverse(call: &mut Call<'_>) -> CallResult {
    let [item, preserve_keys] = take(bind_method(
        call,
        "reverse",
        &["charset"],
        &[
            Param::required("item"),
            Param::optional("preserveKeys", false),
        ],
    )?);
    let charset = call.charset();
    let diag = call.diagnostics();
    let array = match item {
        Value::Object(object) if object.instance_of("Traversable") => {
            Value::Array(object_to_array(&object, diag)?)
        }
        Value::Array(array) => Value::Array(array),
        item => {
            let mut string = convert::to_string(&item, diag)?;
            let utf8 = charset.as_bytes() == b"UTF-8";
            if !utf8 {
                string = convert_encoding(&string, b"UTF-8", &charset, diag)?;
            }
            let mut characters = split_into_characters(&string, "reverse")?;
            characters.reverse();
            let mut string = PhpString::from(
                characters
                    .iter()
                    .flat_map(|c| c.as_bytes().iter().copied())
                    .collect::<Vec<u8>>(),
            );
            if !utf8 {
                string = convert_encoding(&string, &charset, b"UTF-8", diag)?;
            }
            return Ok(Value::String(string));
        }
    };
    Ok(Value::Array(php_array::array_reverse(
        &array,
        Some(&preserve_keys),
        diag,
    )?))
}

/// Twig: `CoreExtension::shuffle(string $charset, $item)`: a string is shuffled per code point, an iterable is
/// listed (`toArray($item, false)`) and shuffled, anything else is returned unchanged. `shuffle()` draws from the
/// thread's Mersenne Twister ([`with_mt_rand`]).
fn shuffle(call: &mut Call<'_>) -> CallResult {
    let [item] = take(bind_method(
        call,
        "shuffle",
        &["charset"],
        &[Param::required("item")],
    )?);
    let charset = call.charset();
    let diag = call.diagnostics();
    if let Value::String(string) = &item {
        let utf8 = charset.as_bytes() == b"UTF-8";
        let string = if utf8 {
            string.clone()
        } else {
            convert_encoding(string, b"UTF-8", &charset, diag)?
        };
        let characters = split_into_characters(&string, "shuffle")?;
        let mut characters = Array::from_values(characters.into_iter().map(Value::String));
        with_mt_rand(|rng| php_array::shuffle(rng, &mut characters))?;
        let joined = implode_frameless(&Value::from(""), Some(&Value::Array(characters)), diag)?;
        return Ok(Value::String(if utf8 {
            joined
        } else {
            convert_encoding(&joined, &charset, b"UTF-8", diag)?
        }));
    }
    if item.is_iterable() {
        let Value::Array(mut array) = to_array(item, false, diag)? else {
            return Ok(Value::Array(Array::new()));
        };
        with_mt_rand(|rng| php_array::shuffle(rng, &mut array))?;
        return Ok(Value::Array(array));
    }
    Ok(item)
}

/// Twig: `CoreExtension::length(string $charset, $thing): int`: `0` for `null`, `mb_strlen()` of scalars,
/// `count()` of arrays and `Countable`s, `iterator_count()` of other `Traversable`s, `mb_strlen()` of `Stringable`s,
/// `1` for any other object.
fn length(call: &mut Call<'_>) -> CallResult {
    let [thing] = take(bind_method(
        call,
        "length",
        &["charset"],
        &[Param::required("thing")],
    )?);
    let charset = call.charset();
    let diag = call.diagnostics();
    Ok(Value::Int(match &thing {
        Value::Null => 0,
        Value::Array(array) => array.len() as i64,
        Value::Object(object) => {
            if object.instance_of("Countable") || object.instance_of("SimpleXMLElement") {
                dim::count(&thing, 0, diag)?
            } else if object.instance_of("Traversable") {
                object::iterator_count(object, diag)?
            } else if object.instance_of("Stringable") {
                let string = Value::String(object.to_php_string(diag)?);
                mb_strlen(&string, &charset, diag)?
            } else {
                1
            }
        }
        scalar => mb_strlen(scalar, &charset, diag)?,
    }))
}

/// Twig: `CoreExtension::slice(string $charset, $item, $start, $length = null, $preserveKeys = false)` (the `slice`
/// filter and the `x[a:b]` syntax).
fn slice(call: &mut Call<'_>) -> CallResult {
    let [item, start, length, preserve_keys] = take(bind_method(
        call,
        "slice",
        &["charset"],
        &[
            Param::required("item"),
            Param::required("start"),
            Param::optional("length", Value::Null),
            Param::optional("preserveKeys", false),
        ],
    )?);
    let charset = call.charset();
    slice_value(
        &charset,
        item,
        &start,
        &length,
        &preserve_keys,
        call.diagnostics(),
    )
}

/// The body of `CoreExtension::slice()`.
///
/// A `Traversable` is unwrapped through `getIterator()` method calls while it is an `IteratorAggregate`; an
/// `Iterator` sliced with `$start >= 0 && $length >= 0` (PHP comparisons: `null >= 0` holds) is read through a
/// `LimitIterator` (an `OutOfBoundsException` gives `[]`), anything else through `iterator_to_array($item,
/// $preserveKeys)`. Arrays go through `array_slice()`, other values through `mb_substr((string) $item, ...)` in the
/// environment charset.
fn slice_value(
    charset: &[u8],
    item: Value,
    start: &Value,
    length: &Value,
    preserve_keys: &Value,
    diag: &mut dyn Diagnostics,
) -> CallResult {
    let mut item = item;
    if matches!(&item, Value::Object(o) if o.instance_of("Traversable")) {
        while let Value::Object(object) = &item
            && object.instance_of("IteratorAggregate")
        {
            item = object.call_method("getIterator", CallArgs::new(), diag)?;
        }
        if compare::greater_or_equal(start, &Value::Int(0), diag)?
            && compare::greater_or_equal(length, &Value::Int(0), diag)?
            && let Value::Object(iterator) = &item
            && iterator.instance_of("Iterator")
        {
            let limit = if length.is_null() {
                Value::Int(-1)
            } else {
                length.clone()
            };
            return Ok(Value::Array(iterators::limit_iterator_to_array(
                iterator,
                start,
                &limit,
                preserve_keys,
                diag,
            )?));
        }
        item = Value::Array(php_array::iterator_to_array(
            &item,
            Some(preserve_keys),
            diag,
        )?);
    }
    if let Value::Array(_) = &item {
        return Ok(Value::Array(php_array::array_slice(
            &item,
            start,
            Some(length),
            Some(preserve_keys),
            diag,
        )?));
    }
    let string = Value::String(convert::to_string(&item, diag)?);
    Ok(Value::String(mb_substr(
        &string, start, length, charset, diag,
    )?))
}

/// The common body of `CoreExtension::first()` and `last()`: `$elements = slice($charset, $item, $start, 1, false)`,
/// then the string itself or `current($elements)` (`false` for `[]`).
fn first_or_last(call: &mut Call<'_>, method: &str, start: i64) -> CallResult {
    let [item] = take(bind_method(
        call,
        method,
        &["charset"],
        &[Param::required("item")],
    )?);
    let charset = call.charset();
    let elements = slice_value(
        &charset,
        item,
        &Value::Int(start),
        &Value::Int(1),
        &Value::Bool(false),
        call.diagnostics(),
    )?;
    Ok(match elements {
        Value::Array(array) => php_array::current(&array),
        other => other,
    })
}

/// Twig: `CoreExtension::first(string $charset, $item)`.
fn first(call: &mut Call<'_>) -> CallResult {
    first_or_last(call, "first", 0)
}

/// Twig: `CoreExtension::last(string $charset, $item)`.
fn last(call: &mut Call<'_>) -> CallResult {
    first_or_last(call, "last", -1)
}

/// Twig: `CoreExtension::keys($array): array`.
///
/// A `Traversable` is unwrapped through `getIterator()` method calls; an `Iterator` is walked with its `rewind()`,
/// `valid()`, `key()`, `next()` methods (keys kept raw, repeated keys included), anything else with `foreach`.
/// Arrays give `array_keys()`, other values `[]`.
fn keys(call: &mut Call<'_>) -> CallResult {
    let [mut array] = take(bind_method(call, "keys", &[], &[Param::required("array")])?);
    if matches!(&array, Value::Object(o) if o.instance_of("Traversable")) {
        let diag = call.diagnostics();
        while let Value::Object(object) = &array
            && object.instance_of("IteratorAggregate")
        {
            array = object.call_method("getIterator", CallArgs::new(), diag)?;
        }
        let mut keys = Array::new();
        if let Value::Object(iterator) = &array
            && iterator.instance_of("Iterator")
        {
            iterator.call_method("rewind", CallArgs::new(), diag)?;
            loop {
                let valid = iterator.call_method("valid", CallArgs::new(), diag)?;
                if !convert::to_bool(&valid, diag)? {
                    break;
                }
                keys.push(iterator.call_method("key", CallArgs::new(), diag)?)?;
                iterator.call_method("next", CallArgs::new(), diag)?;
            }
            return Ok(Value::Array(keys));
        }
        foreach_keys(call, &array, &mut keys)?;
        return Ok(Value::Array(keys));
    }
    Ok(match &array {
        Value::Array(array) => {
            Value::Array(Array::from_values(array.keys().map(ArrayKey::to_value)))
        }
        _ => Value::Array(Array::new()),
    })
}

/// `foreach ($value as $key => $item) { $keys[] = $key; }` for what `keys` left after unwrapping aggregates: an
/// array, a `Traversable` that is not an `Iterator`, or (when `getIterator()` returned one) a plain object, whose
/// visible properties `foreach` walks.
fn foreach_keys(call: &mut Call<'_>, value: &Value, keys: &mut Array) -> CallResult<()> {
    if let Value::Object(object) = value
        && !object.instance_of("Traversable")
    {
        for (name, _) in object.visible_properties(call.diagnostics())? {
            keys.push(Value::String(name))?;
        }
        return Ok(());
    }
    for_each(call.state(), value, |_, key, _| {
        keys.push(key)?;
        Ok(true)
    })
}

/// Twig: `CoreExtension::invoke(\Closure $arrow, ...$arguments): mixed`: `$arrow(...$arguments)`.
///
/// A non-`Closure` input throws PHP's `TypeError` without the `, called in ...` suffix (`reference/deviations.md`).
fn invoke(call: &mut Call<'_>) -> CallResult {
    let [arrow, arguments] = take(bind_method(
        call,
        "invoke",
        &[],
        &[Param::required("arrow"), Param::variadic("arguments")],
    )?);
    let diag = call.diagnostics();
    let arrow = verify_user_arg(
        arrow,
        &DeclaredType::new(&["Closure"], TypeMask::NONE),
        &UserArg::new("Twig\\Extension\\CoreExtension::invoke", 1, "arrow"),
        diag,
    )?;
    let arguments = match &arguments {
        Value::Array(arguments) => CallArgs::from_spread(arguments)?,
        _ => CallArgs::new(),
    };
    match &arrow {
        Value::Object(closure) => Ok(closure.invoke(arguments, diag)?),
        _ => Ok(Value::Null),
    }
}

/// Twig: `CoreExtension::cycle($values, $position): mixed`: `$values[$position % count($values)]` for an array or
/// an `ArrayAccess` (a non-countable one is deprecated and converted with `toArray($values, false)`).
fn cycle(call: &mut Call<'_>) -> CallResult {
    let [values, position] = take(bind_method(
        call,
        "cycle",
        &[],
        &[Param::required("values"), Param::required("position")],
    )?);
    let mut values = values;
    if !matches!(values, Value::Array(_)) {
        let Value::Object(object) = &values else {
            return Err(Error::runtime(
                "The \"cycle\" function expects an array or \"ArrayAccess\" as first argument.",
            )
            .into());
        };
        if !object.instance_of("ArrayAccess") {
            return Err(Error::runtime(
                "The \"cycle\" function expects an array or \"ArrayAccess\" as first argument.",
            )
            .into());
        }
        if !object.instance_of("Countable") {
            call.deprecate(Deprecation::twig(
                "3.12",
                "Passing a non-countable sequence of values to \"Twig\\Extension\\CoreExtension::cycle()\" is deprecated.",
            ));
            values = to_array(values, false, call.diagnostics())?;
        }
    }
    let diag = call.diagnostics();
    let count = dim::count(&values, 0, diag)?;
    if count == 0 {
        return Err(Error::runtime("The \"cycle\" function expects a non-empty sequence.").into());
    }
    let index = ops::modulo(&position, &Value::Int(count), diag)?;
    Ok(dim::read(&values, &index, FetchMode::Read, diag)?)
}

/// Twig: `CoreExtension::random(string $charset, $values = null, $max = null)`.
///
/// `null` gives `mt_rand()` (or `mt_rand(0, (int) $max)`); an int or float a random integer between it and `0` (or
/// `$max`); a string a random character (split per code point, through UTF-8 when the charset is not); an iterable
/// a random element (`array_rand($values, 1)` of `toArray($values)`); anything else is returned as it is. All draws
/// come from the thread's Mersenne Twister ([`with_mt_rand`]).
fn random_function(call: &mut Call<'_>) -> CallResult {
    let [values, max] = take(bind_method(
        call,
        "random",
        &["charset"],
        &[
            Param::optional("values", Value::Null),
            Param::optional("max", Value::Null),
        ],
    )?);
    let charset = call.charset();
    let diag = call.diagnostics();
    if values.is_null() {
        if max.is_null() {
            return Ok(Value::Int(with_mt_rand(php_array::mt_rand)));
        }
        let max = Value::Int(convert::to_long(&max, diag)?);
        return Ok(Value::Int(with_mt_rand(|rng| {
            php_array::mt_rand_range(rng, &Value::Int(0), &max, diag)
        })?));
    }
    if matches!(values, Value::Int(_) | Value::Float(_)) {
        let (min, max) = if max.is_null() {
            if compare::less(&values, &Value::Int(0), diag)? {
                (values, Value::Int(0))
            } else {
                (Value::Int(0), values)
            }
        } else {
            (values, max)
        };
        let min = Value::Int(convert::to_long(&min, diag)?);
        let max = Value::Int(convert::to_long(&max, diag)?);
        return Ok(Value::Int(with_mt_rand(|rng| {
            php_array::mt_rand_range(rng, &min, &max, diag)
        })?));
    }
    let mut values = values;
    if let Value::String(string) = &values {
        if string.is_empty() {
            return Ok(Value::from(""));
        }
        let utf8 = charset.as_bytes() == b"UTF-8";
        let string = if utf8 {
            string.clone()
        } else {
            convert_encoding(string, b"UTF-8", &charset, diag)?
        };
        let mut characters = split_into_characters(&string, "random")?;
        if !utf8 {
            for character in &mut characters {
                *character = convert_encoding(character, &charset, b"UTF-8", diag)?;
            }
        }
        values = Value::Array(Array::from_values(
            characters.into_iter().map(Value::String),
        ));
    }
    if !values.is_iterable() {
        return Ok(values);
    }
    let Value::Array(values) = to_array(values, true, diag)? else {
        return Ok(Value::Null);
    };
    if values.is_empty() {
        return Err(Error::runtime(
            "The \"random\" function cannot pick from an empty sequence or mapping.",
        )
        .into());
    }
    let key = with_mt_rand(|rng| php_array::array_pick_keys(rng, &values, 1))?;
    Ok(dim::array_read(&values, &key, FetchMode::Read, diag)?)
}

/// Whether the argument list of the running call unpacked an argument (`max(...values)`), read from the call's
/// instructions: the arguments between the call's `ArgsStart` and its `Call` (nested calls skipped) include an
/// `ArgSpread`. PHP compiles a direct call with unpacked (or named) arguments as a regular call and a direct call
/// with exactly two plain arguments to the frameless variant of `min()`/`max()`, whose ties differ.
fn has_unpacked_arguments(call: &mut Call<'_>) -> bool {
    let state = call.state();
    let Some(frame) = state.frames().last() else {
        return false;
    };
    let code = frame.program.unit(frame.unit).code();
    let ip = frame.ip as usize;
    if !matches!(code.get(ip), Some(Instr::Call(_))) {
        return false;
    }
    let mut depth = 0usize;
    for instr in code[..ip].iter().rev() {
        match instr {
            Instr::Call(_) => depth += 1,
            Instr::ArgsStart { .. } => {
                if depth == 0 {
                    return false;
                }
                depth -= 1;
            }
            Instr::ArgSpread if depth == 0 => return true,
            _ => {}
        }
    }
    false
}

/// PHP's `max(mixed $value, mixed ...$values)` / `min()` called directly by the compiled template.
///
/// Two plain arguments run the frameless variant (`ZEND_FRAMELESS_FUNCTION(max, 2)`: ties return the left operand
/// for `max`, the right one for `min`); one argument, three or more, or unpacked arguments run the regular
/// function (the first of equal values wins). Named arguments of a spread bind `$value`; any other name is
/// rejected by `Z_PARAM_VARIADIC` (`max() does not accept unknown named parameters`).
fn min_max(call: &mut Call<'_>, name: &str, pick_max: bool) -> CallResult {
    let mut args = call.args().to_vec();
    let named = call.named_args().to_vec();
    let regular = |args: &[Value], diag: &mut dyn Diagnostics| -> CallResult {
        Ok(if pick_max {
            php_array::max(args, diag)?
        } else {
            php_array::min(args, diag)?
        })
    };
    if !named.is_empty() {
        let mut unknown = false;
        for (parameter, value) in named {
            if parameter.as_bytes() == b"value" {
                if !args.is_empty() {
                    return Err(PhpError::error(
                        "Named parameter $value overwrites previous argument",
                    )
                    .into());
                }
                args.push(value);
            } else {
                unknown = true;
            }
        }
        if args.is_empty() {
            return Err(PhpError::argument_count_error(format!(
                "{name}() expects at least 1 argument, 0 given"
            ))
            .into());
        }
        if unknown {
            return Err(PhpError::argument_count_error(format!(
                "{name}() does not accept unknown named parameters"
            ))
            .into());
        }
        return regular(&args, call.diagnostics());
    }
    if args.len() == 2 && !has_unpacked_arguments(call) {
        let diag = call.diagnostics();
        return Ok(if pick_max {
            php_array::max_frameless(&args[0], &args[1], diag)?
        } else {
            php_array::min_frameless(&args[0], &args[1], diag)?
        });
    }
    regular(&args, call.diagnostics())
}

/// PHP's `max()` (the `max` function).
fn max(call: &mut Call<'_>) -> CallResult {
    min_max(call, "max", true)
}

/// PHP's `min()` (the `min` function).
fn min(call: &mut Call<'_>) -> CallResult {
    min_max(call, "min", false)
}

/// PHP's `range($start, $end, $step = 1)` (the `range` function; an internal function: extra arguments are an
/// error). The `..` operator compiles to its own instruction.
fn range(call: &mut Call<'_>) -> CallResult {
    if call.named_args().is_empty() {
        let args = call.args().to_vec();
        return Ok(Value::Array(php_array::range_call(
            &args,
            call.diagnostics(),
        )?));
    }
    let [start, end, step] = take(call.bind(
        "range",
        &[
            Param::required("start"),
            Param::required("end"),
            Param::optional("step", 1),
        ],
        FunctionKind::Internal,
    )?);
    Ok(Value::Array(php_array::range(
        &start,
        &end,
        Some(&step),
        call.diagnostics(),
    )?))
}

/// PHP's `is_iterable($value)` (the `iterable` test; an internal function).
fn is_iterable(call: &mut Call<'_>) -> CallResult {
    let [value] = take(call.bind(
        "is_iterable",
        &[Param::required("value")],
        FunctionKind::Internal,
    )?);
    Ok(Value::Bool(value.is_iterable()))
}

/// Twig: `CoreExtension::testEmpty($value): bool` (the `empty` test).
fn test_empty_runtime(call: &mut Call<'_>) -> CallResult {
    let [value] = take(bind_method(
        call,
        "testEmpty",
        &[],
        &[Param::required("value")],
    )?);
    Ok(Value::Bool(test_empty(&value, call.diagnostics())?))
}

/// The conversions of `testSequence()` and `testMapping()`: an `ArrayObject` becomes `getArrayCopy()`, then a
/// `Traversable` becomes `iterator_to_array()` (consuming generators).
fn sequence_or_mapping_value(call: &mut Call<'_>, method: &str) -> CallResult {
    let [mut value] = take(bind_method(call, method, &[], &[Param::required("value")])?);
    let diag = call.diagnostics();
    if let Value::Object(object) = &value
        && object.instance_of("ArrayObject")
    {
        value = object.call_method("getArrayCopy", CallArgs::new(), diag)?;
    }
    if let Value::Object(object) = &value
        && object.instance_of("Traversable")
    {
        value = Value::Array(object_to_array(object, diag)?);
    }
    Ok(value)
}

/// Twig: `CoreExtension::testSequence($value): bool`: an array with keys `0..n` (`array_is_list()`).
fn test_sequence(call: &mut Call<'_>) -> CallResult {
    let value = sequence_or_mapping_value(call, "testSequence")?;
    Ok(Value::Bool(
        matches!(&value, Value::Array(a) if a.is_list()),
    ))
}

/// Twig: `CoreExtension::testMapping($value): bool`: an array that is not a list, or any other object.
fn test_mapping(call: &mut Call<'_>) -> CallResult {
    let value = sequence_or_mapping_value(call, "testMapping")?;
    Ok(Value::Bool(match &value {
        Value::Array(array) => !array.is_list(),
        Value::Object(_) => true,
        _ => false,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::ConstantValue;
    use crate::escaper::DefaultStrategy;
    use crate::extension::runtime::CallableRuntime;
    use crate::extension::{
        CallableParameter, FilterOptions, FunctionOptions, PhpCallable, TwigFilter, TwigFunction,
    };
    use crate::loader::ArrayLoader;
    use crate::runtime::{DiagnosticsPolicy, Environment, EnvironmentOptions, Notice};
    use twig_value::Level;

    fn vectors(file: &str) -> serde_json::Value {
        let path = format!(
            "{}/tests/vectors/callables/arrays/{file}",
            env!("CARGO_MANIFEST_DIR")
        );
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path}: {e}"))
    }

    /// The top-level scalars of a case's data (all the `random` cases use).
    fn context(case: &serde_json::Value) -> Array {
        let mut context = Array::new();
        let data = case.get("data_json").or_else(|| case.get("data_rust"));
        if let Some(serde_json::Value::Object(fields)) = data {
            for (name, value) in fields {
                let value = match value {
                    serde_json::Value::Bool(b) => Value::Bool(*b),
                    serde_json::Value::Number(n) => match n.as_i64() {
                        Some(i) => Value::Int(i),
                        None => Value::Float(n.as_f64().unwrap_or_default()),
                    },
                    serde_json::Value::String(s) => Value::from(s.as_str()),
                    _ => Value::Null,
                };
                context.insert(name.as_str(), value);
            }
        }
        context
    }

    /// A plain environment (autoescape off, diagnostics collected) whose `random` function is this module's
    /// `CoreExtension::random` port (the misc group registers its own after this group) and whose `json_encode` is
    /// PHP's builtin, as in tests/callables_arrays.rs.
    fn environment(template: &str) -> Environment {
        let options = EnvironmentOptions {
            autoescape: DefaultStrategy::Disabled,
            diagnostics: DiagnosticsPolicy::Collect,
            ..EnvironmentOptions::default()
        };
        let env = Environment::with_options(ArrayLoader::new([("index.twig", template)]), options);
        let random = PhpCallable::method(
            CORE,
            "random",
            vec![
                CallableParameter::required("charset"),
                CallableParameter::optional("values", ConstantValue::Null),
                CallableParameter::optional("max", ConstantValue::Null),
            ],
        );
        CallableRuntime::new(random_function).attach(&random);
        env.add_function(TwigFunction::new(
            "random",
            Some(random),
            FunctionOptions {
                needs_charset: true,
                ..FunctionOptions::default()
            },
        ))
        .expect("random");
        let json_encode = PhpCallable::function(
            "json_encode",
            vec![
                CallableParameter::required("value"),
                CallableParameter::optional("flags", ConstantValue::Int(0)),
                CallableParameter::optional("depth", ConstantValue::Int(512)),
            ],
        );
        CallableRuntime::new(|call| {
            let [value, flags, depth] = take(call.bind(
                "json_encode",
                &[
                    Param::required("value"),
                    Param::optional("flags", 0),
                    Param::optional("depth", 512),
                ],
                FunctionKind::Internal,
            )?);
            Ok(twig_value::builtins::format::json_encode(
                &value,
                Some(&flags),
                Some(&depth),
                call.diagnostics(),
            )?)
        })
        .attach(&json_encode);
        env.add_filter(TwigFilter::new(
            "json_encode",
            Some(json_encode),
            FilterOptions::default(),
        ))
        .expect("json_encode");
        env
    }

    /// Renders a vector case and describes how it differs from the oracle's result, if it does.
    fn mismatch(case: &serde_json::Value, expected: &serde_json::Value) -> Option<String> {
        let template = case["templates"]["index.twig"].as_str().expect("template");
        let env = environment(template);
        let result = env.render("index.twig", context(case));
        let notices: Vec<String> = env
            .take_notices()
            .into_iter()
            .map(|notice| match notice {
                Notice::Php(d) => format!(
                    "{}: {}",
                    match d.level {
                        Level::Warning => "E_WARNING",
                        Level::Notice => "E_NOTICE",
                        Level::Deprecated => "E_DEPRECATED",
                        Level::UserDeprecated => "E_USER_DEPRECATED",
                    },
                    d.message.to_string_lossy()
                ),
                Notice::Deprecation(d) => format!("E_USER_DEPRECATED: {d}"),
            })
            .collect();
        let want_notices: Vec<String> = expected["diagnostics"]
            .as_array()
            .map(|list| {
                list.iter()
                    .map(|d| {
                        format!(
                            "{}: {}",
                            d["type"].as_str().unwrap_or_default(),
                            d["message"].as_str().unwrap_or_default()
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        let got = match result {
            Ok(output) => format!("ok {output}"),
            Err(error) => format!("{}: {error}", error.kind().class_name()),
        };
        let want = if expected["ok"].as_bool() == Some(true) {
            format!("ok {}", expected["output"].as_str().unwrap_or_default())
        } else {
            format!(
                "{}: {}",
                expected["error"]["class"].as_str().unwrap_or_default(),
                expected["error"]["message"].as_str().unwrap_or_default()
            )
        };
        if got == want && notices == want_notices {
            None
        } else {
            Some(format!(
                "{template:?}: {got:?} {notices:?}, expected {want:?} {want_notices:?}"
            ))
        }
    }

    /// Seeded `random` and `shuffle` against PHP: every case of tests/vectors/callables/arrays/random.gen.json runs
    /// `mt_srand($seed)` in its `data_php` before rendering; the thread's generator gets the same seed here, and
    /// `random` and `shuffle` share it as in PHP (regenerate with tests/vectors/callables/arrays/arrays.gen.py).
    #[test]
    fn seeded_random_matches_php() {
        let cases = vectors("random.gen.json");
        let results = vectors("random.json");
        let cases = cases["cases"].as_array().expect("cases");
        let results = results.as_array().expect("results");
        assert_eq!(cases.len(), results.len());
        let mut failures = Vec::new();
        for (case, expected) in cases.iter().zip(results) {
            random::mt_srand(case["seed"].as_u64().expect("seed") as u32);
            failures.extend(mismatch(case, expected));
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// The deterministic `random` cases of the `functions` vectors through this module's port.
    #[test]
    fn deterministic_random_matches_php() {
        let cases = vectors("functions.gen.json");
        let results = vectors("functions.json");
        let cases = cases["cases"].as_array().expect("cases");
        let results = results.as_array().expect("results");
        let mut failures = Vec::new();
        let mut count = 0;
        for (case, expected) in cases.iter().zip(results) {
            let template = case["templates"]["index.twig"].as_str().expect("template");
            if !template.contains("random(") {
                continue;
            }
            count += 1;
            failures.extend(mismatch(case, expected));
        }
        assert!(count > 20, "{count} random cases");
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }
}
