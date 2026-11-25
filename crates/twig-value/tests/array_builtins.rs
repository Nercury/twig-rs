//! Array builtins (`builtins::array`) against PHP 8.5.13: `zend_sort` and the sort family, `range()`,
//! `min()`/`max()`, MT19937 randomness and the other array functions.
//!
//! Regenerate the vectors under `tests/vectors/array/` with the Docker oracle
//! (`research/php-oracle`):
//!
//! ```text
//! cd crates/twig-value/tests/vectors
//! for t in sort range minmax random functions; do
//!   MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//!       php /t/array/$t.gen.php > array/$t.json
//! done
//! ```
//!
//! Arrays are encoded like `values.inc.php`'s `enc()` plus `"next"`, the key `$a[] = x` would use
//! (`array.inc.php`'s `encn()`), so results are compared including `nNextFreeElement`.

mod support;

use serde_json::{Value as Json, json};
use support::values::{enc_value, messages, res_json, resolve_diags, resolve_res, typed_value};
use support::{Mismatches, diag_list, hex, load, s, unhex};
use twig_value::builtins::array as arr;
use twig_value::object::{ArrayIterator, StdClass};
use twig_value::{Array, ArrayKey, Diagnostics, PhpError, PhpResult, Value, compare, convert, ops};

// ---------------------------------------------------------------------------------------------
// Encoding
// ---------------------------------------------------------------------------------------------

/// The key `$a[] = x` would use, or `None` when it is occupied.
fn append_key(a: &Array) -> Option<i64> {
    let mut t = a.clone();
    t.push(Value::Int(0)).ok()
}

/// Decodes `encn()`: `enc()` values whose arrays carry `"next"`; also `{"t":"iter","items":...}`
/// for an `ArrayIterator` over an array.
fn decode(j: &Json) -> Value {
    match s(&j["t"]) {
        "array" => Value::Array(decode_array(j)),
        "iter" => {
            let inner = decode_array(&j["array"]);
            Value::Object(ArrayIterator::new(inner).into_ref())
        }
        _ => typed_value(j),
    }
}

fn decode_array(j: &Json) -> Array {
    let mut a = Array::new();
    for item in j["items"].as_array().expect("items") {
        let key = match decode(&item[0]) {
            Value::Int(i) => ArrayKey::Int(i),
            Value::String(s) => ArrayKey::Str(s),
            other => panic!("bad key {other:?}"),
        };
        a.insert(key, decode(&item[1]));
    }
    if let Some(next) = j["next"].as_i64()
        && append_key(&a) != Some(next)
    {
        a.set_next_free(Some(next));
    }
    a
}

/// `encn()` of a Rust value.
fn encode(v: &Value) -> Json {
    match v {
        Value::Array(a) => {
            let items: Vec<Json> = a
                .iter()
                .map(|(k, v)| json!([enc_value(&k.to_value()), encode(v)]))
                .collect();
            json!({"t": "array", "items": items, "next": append_key(a)})
        }
        other => enc_value(other),
    }
}

/// `resn()` of a Rust result with the message resolved.
fn res(r: &PhpResult<Value>) -> Json {
    match r {
        Ok(v) => json!({ "ok": encode(v) }),
        Err(e) => json!({ "err": [e.class().name(), hex(e.message().as_bytes())] }),
    }
}

/// A PHP `resn()` entry with its message index resolved.
fn php_res(j: &Json, msgs: &[String]) -> Json {
    match j.get("err") {
        Some(e) => json!({ "err": [e[0].clone(), msgs[e[1].as_u64().expect("idx") as usize]] }),
        None => j.clone(),
    }
}

fn ints(j: &Json) -> Vec<i64> {
    j.as_array()
        .expect("list")
        .iter()
        .map(|v| v.as_i64().expect("int"))
        .collect()
}

fn list_of_ints(values: &[i64]) -> Array {
    values.iter().map(|&i| Value::Int(i)).collect()
}

fn keys_of(a: &Array) -> Vec<i64> {
    a.keys().map(|k| k.as_int().expect("int key")).collect()
}

// ---------------------------------------------------------------------------------------------
// Callbacks mirroring array.inc.php
// ---------------------------------------------------------------------------------------------

/// The comparators of `comparators()`, counting calls.
fn comparator<'a>(
    name: &'a str,
    calls: &'a mut u64,
) -> impl FnMut(&[Value], &mut dyn Diagnostics) -> PhpResult<Value> + 'a {
    move |args: &[Value], diag: &mut dyn Diagnostics| {
        *calls += 1;
        let (a, b) = (&args[0], &args[1]);
        let ship = |diag: &mut dyn Diagnostics| -> PhpResult<Value> {
            Ok(Value::Int(compare::spaceship(a, b, diag)?))
        };
        Ok(match name {
            "cmp" => ship(diag)?,
            "rev" => Value::Int(compare::spaceship(b, a, diag)?),
            "sub" => ops::sub(a, b, diag)?,
            "gt" => Value::Bool(compare::greater(a, b, diag)?),
            "lt" => Value::Bool(compare::less(a, b, diag)?),
            "half" => ops::mul(&ship(diag)?, &Value::Float(0.5), diag)?,
            "big" => ops::mul(&ship(diag)?, &Value::Float(1e30), diag)?,
            "str" => Value::String(convert::to_string(&ship(diag)?, diag)?),
            "strx" => Value::String(ops::concat(&ship(diag)?, &Value::from("x"), diag)?),
            "null" => Value::Null,
            "arr" => Value::Array(Array::from_values([a.clone()])),
            "obj" => Value::Object(StdClass::new().into_ref()),
            "nan" => Value::Float(f64::NAN),
            "throw" => {
                if *calls == 3 {
                    return Err(PhpError::exception("stop at 3"));
                }
                ship(diag)?
            }
            other => panic!("unknown comparator {other}"),
        })
    }
}

/// The callbacks of `callbacks()`, counting calls.
fn callback<'a>(
    name: &'a str,
    calls: &'a mut u64,
) -> impl FnMut(&[Value], &mut dyn Diagnostics) -> PhpResult<Value> + 'a {
    move |args: &[Value], diag: &mut dyn Diagnostics| {
        *calls += 1;
        Ok(match name {
            "truthy" | "first" => args[0].clone(),
            "odd" => Value::Bool(matches!(args[0], Value::Int(i) if i % 2 != 0)),
            "both" => Value::Bool(
                args.len() == 2
                    && !args[1].identical(&Value::from("b"))
                    && !args[1].identical(&Value::Int(1)),
            ),
            "nargs" => {
                if args.len() == 1 {
                    Value::Float(0.0)
                } else {
                    Value::from("yes")
                }
            }
            "pack" => Value::Array(Array::from_values(args.iter().cloned())),
            "concat" => {
                let left = ops::concat(&args[0], &Value::from("|"), diag)?;
                let right = if matches!(args[1], Value::Array(_)) {
                    Value::from("A")
                } else {
                    args[1].clone()
                };
                Value::String(ops::concat(&Value::String(left), &right, diag)?)
            }
            "count" => {
                let carry = if args[0].is_null() {
                    Value::Int(0)
                } else {
                    args[0].clone()
                };
                ops::add(&carry, &Value::Int(1), diag)?
            }
            "throw" => {
                if *calls == 2 {
                    return Err(PhpError::new(
                        twig_value::ThrowableClass::RuntimeException,
                        "callback stop",
                    ));
                }
                args[0].clone()
            }
            other => panic!("unknown callback {other}"),
        })
    }
}

// ---------------------------------------------------------------------------------------------
// sort.json
// ---------------------------------------------------------------------------------------------

/// The inconsistent comparator of `sort.gen.php`.
fn inconsistent(x: i64, y: i64) -> i64 {
    ((x * 31 + y * 17 + (x ^ y)) % 5) - 2
}

#[test]
fn sort_vectors() {
    let v = load("array/sort.json");
    let msgs = messages(&v);
    let mut m = Mismatches::new("array/sort");

    for row in v["zsort"].as_array().expect("zsort") {
        let input = ints(&row[1]);
        let mut a = list_of_ints(&input);
        let mut calls = 0u64;
        let mut d = Vec::new();
        arr::uasort(
            &mut a,
            &mut |args: &[Value], _diag: &mut dyn Diagnostics| {
                calls += 1;
                let x = args[0].as_int().expect("int");
                let y = args[1].as_int().expect("int");
                Ok(Value::Int(inconsistent(x, y)))
            },
            &mut d,
        )
        .expect("uasort");
        let n = input.len();
        m.check(|| format!("zsort n={n} keys"), keys_of(&a), ints(&row[2]));
        m.check(
            || format!("zsort n={n} calls"),
            calls,
            row[3].as_u64().expect("calls"),
        );
    }

    for row in v["stable"].as_array().expect("stable") {
        let input = ints(&row[0]);
        let mut a = list_of_ints(&input);
        let mut d = Vec::new();
        arr::asort(&mut a, arr::SORT_REGULAR, &mut d).expect("asort");
        m.check(
            || format!("stable n={}", input.len()),
            keys_of(&a),
            ints(&row[1]),
        );
    }

    let arrays: Vec<Array> = v["arrays"]
        .as_array()
        .expect("arrays")
        .iter()
        .map(decode_array)
        .collect();

    for row in v["flags"].as_array().expect("flags") {
        let ai = row[0].as_u64().expect("ai") as usize;
        let func = s(&row[1]);
        let flags = row[2].as_i64().expect("flags");
        let mut a = arrays[ai].clone();
        let mut d = Vec::new();
        let r = match func {
            "sort" => arr::sort(&mut a, flags, &mut d),
            "rsort" => arr::rsort(&mut a, flags, &mut d),
            "asort" => arr::asort(&mut a, flags, &mut d),
            "arsort" => arr::arsort(&mut a, flags, &mut d),
            "ksort" => arr::ksort(&mut a, flags, &mut d),
            "krsort" => arr::krsort(&mut a, flags, &mut d),
            "natsort" => arr::natsort(&mut a, &mut d),
            "natcasesort" => arr::natcasesort(&mut a, &mut d),
            other => panic!("unknown sort function {other}"),
        }
        .map(|()| Value::Array(a));
        let what = || format!("{func}(arrays[{ai}], {flags})");
        m.check(what, res(&r), php_res(&row[3], &msgs));
        m.check(
            || format!("{func}(arrays[{ai}], {flags}) diagnostics"),
            diag_list(&d),
            resolve_diags(&row[4], &msgs),
        );
    }

    for row in v["user"].as_array().expect("user") {
        let ai = row[0].as_u64().expect("ai") as usize;
        let func = s(&row[1]);
        let name = s(&row[2]);
        let mut a = arrays[ai].clone();
        let mut d = Vec::new();
        let mut calls = 0u64;
        let r = {
            let mut cmp = comparator(name, &mut calls);
            match func {
                "usort" => arr::usort(&mut a, &mut cmp, &mut d),
                "uasort" => arr::uasort(&mut a, &mut cmp, &mut d),
                "uksort" => arr::uksort(&mut a, &mut cmp, &mut d),
                other => panic!("unknown sort function {other}"),
            }
        };
        let r = r.map(|()| Value::Array(a));
        m.check(
            || format!("{func}(arrays[{ai}], {name})"),
            res(&r),
            php_res(&row[3], &msgs),
        );
        m.check(
            || format!("{func}(arrays[{ai}], {name}) diagnostics"),
            diag_list(&d),
            resolve_diags(&row[4], &msgs),
        );
        m.check(
            || format!("{func}(arrays[{ai}], {name}) calls"),
            calls,
            row[5].as_u64().expect("calls"),
        );
    }

    for row in v["natpairs"].as_array().expect("natpairs") {
        let a = unhex(s(&row[0]));
        let b = unhex(s(&row[1]));
        m.check(
            || format!("strnatcmp({a:?}, {b:?})"),
            i64::from(arr::strnatcmp_ex(&a, &b, false)),
            row[2].as_i64().expect("r"),
        );
        m.check(
            || format!("strnatcasecmp({a:?}, {b:?})"),
            i64::from(arr::strnatcmp_ex(&a, &b, true)),
            row[3].as_i64().expect("r"),
        );
    }
    m.finish();
}

// ---------------------------------------------------------------------------------------------
// range.json
// ---------------------------------------------------------------------------------------------

/// `tokens()` of `range.gen.php` for a Rust result.
fn range_tokens(r: &PhpResult<Array>) -> Json {
    match r {
        Err(e) => json!({ "err": [e.class().name(), hex(e.message().as_bytes())] }),
        Ok(a) => {
            assert!(a.is_list(), "range() result must be a list");
            let tokens: Vec<String> = a
                .values()
                .map(|v| match v {
                    Value::Int(i) => format!("i{i}"),
                    Value::Float(f) => format!("f{:016x}", f.to_bits()),
                    Value::String(s) => format!("s{}", hex(s.as_bytes())),
                    other => panic!("unexpected range() element {other:?}"),
                })
                .collect();
            json!({ "ok": tokens, "next": append_key(a) })
        }
    }
}

#[test]
fn range_vectors() {
    let v = load("array/range.json");
    let msgs = messages(&v);
    let values: Vec<Value> = v["values"]
        .as_array()
        .expect("values")
        .iter()
        .map(decode)
        .collect();
    let steps: Vec<Value> = v["steps"]
        .as_array()
        .expect("steps")
        .iter()
        .map(decode)
        .collect();
    let mut m = Mismatches::new("array/range");
    let check =
        |m: &mut Mismatches, what: String, args: &[Value], row_res: &Json, row_diags: &Json| {
            let mut d = Vec::new();
            let r = arr::range_call(args, &mut d);
            m.check(|| what.clone(), range_tokens(&r), php_res(row_res, &msgs));
            m.check(
                || format!("{what} diagnostics"),
                diag_list(&d),
                resolve_diags(row_diags, &msgs),
            );
        };
    for row in v["grid"].as_array().expect("grid") {
        let si = row[0].as_u64().expect("si") as usize;
        let ei = row[1].as_u64().expect("ei") as usize;
        let ti = row[2].as_i64().expect("ti");
        let mut args = vec![values[si].clone(), values[ei].clone()];
        if ti >= 0 {
            args.push(steps[ti as usize].clone());
        }
        check(&mut m, format!("range({args:?})"), &args, &row[3], &row[4]);
    }
    for section in ["extra", "arity"] {
        for row in v[section].as_array().expect(section) {
            let args: Vec<Value> = row[0]
                .as_array()
                .expect("args")
                .iter()
                .map(decode)
                .collect();
            check(&mut m, format!("range({args:?})"), &args, &row[1], &row[2]);
        }
    }
    m.finish();
}

#[test]
fn range_memory_limit() {
    // PHP 8.5.13 with the default memory_limit (php:8.5-cli): range(1, 4194304) succeeds,
    // range(1, 4194305) dies with "Allowed memory size of 134217728 bytes exhausted (tried to
    // allocate 134221824 bytes)". The oracle runs with memory_limit=-1, so this is checked here.
    let mut d = Vec::new();
    let e = arr::range(&Value::Int(1), &Value::Int(4_194_305), None, &mut d).unwrap_err();
    assert_eq!(e.class().name(), "Error");
    assert_eq!(
        e.message(),
        "Allowed memory size of 134217728 bytes exhausted (tried to allocate 134221824 bytes)"
    );
    let e = arr::range(&Value::Float(0.5), &Value::Int(5_000_000), None, &mut d).unwrap_err();
    assert!(e.message().to_string().starts_with("Allowed memory size"));
    assert_eq!(
        arr::range(&Value::Int(1), &Value::Int(4_194_304), None, &mut d)
            .unwrap()
            .len(),
        4_194_304
    );
}

// ---------------------------------------------------------------------------------------------
// minmax.json
// ---------------------------------------------------------------------------------------------

/// `code()` of `minmax.gen.php`: positions of the candidates whose encoding equals the result's.
fn minmax_code(r: &PhpResult<Value>, cands: &[Value]) -> String {
    match r {
        Err(_) => "E".to_string(),
        Ok(v) => {
            let e = enc_value(v);
            let s: String = cands
                .iter()
                .enumerate()
                .filter(|(_, c)| enc_value(c) == e)
                .map(|(i, _)| char::from_digit(i as u32, 10).expect("digit"))
                .collect();
            if s.is_empty() { "?".to_string() } else { s }
        }
    }
}

type MinMaxCall<'a> = Box<dyn Fn(&mut dyn Diagnostics) -> PhpResult<Value> + 'a>;

/// Runs the calls like `run()` of `minmax.gen.php`: codes and extras (with messages resolved).
fn minmax_run(calls: &[MinMaxCall<'_>], cands: &[Value]) -> (Vec<String>, Json) {
    let mut codes = Vec::new();
    let mut extra = Vec::new();
    for (k, f) in calls.iter().enumerate() {
        let mut d = Vec::new();
        let r = f(&mut d);
        codes.push(minmax_code(&r, cands));
        if !d.is_empty() || r.is_err() {
            let err = r
                .as_ref()
                .err()
                .map(|e| json!([e.class().name(), hex(e.message().as_bytes())]));
            extra.push(json!([k, err, diag_list(&d)]));
        }
    }
    (codes, Json::Array(extra))
}

/// The `extra` list of a vector row with messages resolved.
fn php_extra(j: &Json, msgs: &[String]) -> Json {
    Json::Array(
        j.as_array()
            .expect("extra")
            .iter()
            .map(|e| {
                let err = match e[1].as_array() {
                    Some(pair) => json!([
                        pair[0].clone(),
                        msgs[pair[1].as_u64().expect("idx") as usize]
                    ]),
                    None => Json::Null,
                };
                json!([e[0].clone(), err, resolve_diags(&e[2], msgs)])
            })
            .collect(),
    )
}

fn json_strings(j: &Json) -> Vec<String> {
    j.as_array()
        .expect("list")
        .iter()
        .map(|c| s(c).to_string())
        .collect()
}

#[test]
fn minmax_vectors() {
    let v = load("array/minmax.json");
    let msgs = messages(&v);
    let values: Vec<Value> = v["values"]
        .as_array()
        .expect("values")
        .iter()
        .map(decode)
        .collect();
    let mut m = Mismatches::new("array/minmax");

    for row in v["pairs"].as_array().expect("pairs") {
        let i = row[0].as_u64().expect("i") as usize;
        let j = row[1].as_u64().expect("j") as usize;
        let (a, b) = (&values[i], &values[j]);
        let pair = [a.clone(), b.clone()];
        let arr_ab = Value::Array(Array::from_values(pair.clone()));
        let calls: Vec<MinMaxCall<'_>> = vec![
            Box::new(|d| arr::min_frameless(a, b, d)),
            Box::new(|d| arr::max_frameless(a, b, d)),
            Box::new(|d| arr::min(&pair, d)),
            Box::new(|d| arr::max(&pair, d)),
            Box::new(|d| arr::min(std::slice::from_ref(&arr_ab), d)),
            Box::new(|d| arr::max(std::slice::from_ref(&arr_ab), d)),
        ];
        let (codes, extra) = minmax_run(&calls, &pair);
        m.check(
            || format!("pair {i},{j} ({a:?}, {b:?})"),
            codes,
            json_strings(&row[2]),
        );
        m.check(
            || format!("pair {i},{j} extra"),
            extra,
            php_extra(&row[3], &msgs),
        );
    }

    for row in v["triples"].as_array().expect("triples") {
        let idx = ints(&row[0]);
        let args: Vec<Value> = idx.iter().map(|&i| values[i as usize].clone()).collect();
        let calls: Vec<MinMaxCall<'_>> = vec![
            Box::new(|d| arr::min(&args, d)),
            Box::new(|d| arr::max(&args, d)),
        ];
        let (codes, extra) = minmax_run(&calls, &args);
        m.check(|| format!("triple {idx:?}"), codes, json_strings(&row[1]));
        m.check(
            || format!("triple {idx:?} extra"),
            extra,
            php_extra(&row[2], &msgs),
        );
    }

    for row in v["arrays"].as_array().expect("arrays") {
        let idx = ints(&row[0]);
        let elems: Vec<Value> = idx.iter().map(|&i| values[i as usize].clone()).collect();
        let whole = Value::Array(Array::from_values(elems.clone()));
        let calls: Vec<MinMaxCall<'_>> = vec![
            Box::new(|d| arr::min(std::slice::from_ref(&whole), d)),
            Box::new(|d| arr::max(std::slice::from_ref(&whole), d)),
            Box::new(|d| arr::min(&elems, d)),
            Box::new(|d| arr::max(&elems, d)),
        ];
        let (codes, extra) = minmax_run(&calls, &elems);
        m.check(|| format!("array {idx:?}"), codes, json_strings(&row[1]));
        m.check(
            || format!("array {idx:?} extra"),
            extra,
            php_extra(&row[2], &msgs),
        );
    }

    for row in v["errors"].as_array().expect("errors") {
        let func = s(&row[0]);
        let args: Vec<Value> = row[1]
            .as_array()
            .expect("args")
            .iter()
            .map(decode)
            .collect();
        let mut d = Vec::new();
        let r = if func == "min" {
            arr::min(&args, &mut d)
        } else {
            arr::max(&args, &mut d)
        };
        m.check(
            || format!("{func}({args:?})"),
            res_json(&r),
            resolve_res(&row[2], &msgs),
        );
        m.check(
            || format!("{func}({args:?}) diagnostics"),
            diag_list(&d),
            resolve_diags(&row[3], &msgs),
        );
    }
    m.finish();
}

// ---------------------------------------------------------------------------------------------
// random.json
// ---------------------------------------------------------------------------------------------

/// A generator after `mt_srand($seed, $mode)`.
fn seeded(seed: i64, mode: i64, d: &mut Vec<twig_value::Diagnostic>) -> arr::Mt19937 {
    let mut rng = arr::Mt19937::new(0);
    arr::mt_srand(&mut rng, Some(seed), mode, d).expect("mt_srand");
    rng
}

#[test]
fn random_vectors() {
    let v = load("array/random.json");
    let msgs = messages(&v);
    let mut m = Mismatches::new("array/random");

    for row in v["seq"].as_array().expect("seq") {
        let seed = row[0].as_i64().expect("seed");
        let mode = row[1].as_i64().expect("mode");
        let mut d = Vec::new();
        let mut rng = seeded(seed, mode, &mut d);
        m.check(
            || format!("mt_srand({seed}, {mode}) diagnostics"),
            diag_list(&d),
            resolve_diags(&row[2], &msgs),
        );
        let want = ints(&row[3]);
        let got: Vec<i64> = (0..want.len()).map(|_| arr::mt_rand(&mut rng)).collect();
        m.check(
            || format!("mt_rand() after mt_srand({seed}, {mode})"),
            got,
            want,
        );
    }

    for row in v["ranges"].as_array().expect("ranges") {
        let seed = row[0].as_i64().expect("seed");
        let mode = row[1].as_i64().expect("mode");
        let mut d = Vec::new();
        let mut rng = seeded(seed, mode, &mut d);
        for r in row[2].as_array().expect("rows") {
            let (min, max) = (r[0].as_i64().expect("min"), r[1].as_i64().expect("max"));
            let want = ints(&r[2]);
            let got: Vec<i64> = (0..want.len())
                .map(|_| {
                    arr::mt_rand_range(&mut rng, &Value::Int(min), &Value::Int(max), &mut d)
                        .expect("mt_rand")
                })
                .collect();
            m.check(
                || format!("mt_rand({min}, {max}) seed {seed} mode {mode}"),
                got,
                want,
            );
        }
    }

    for row in v["shuffle"].as_array().expect("shuffle") {
        let seed = row[0].as_i64().expect("seed");
        let input = decode_array(&row[1]);
        let mut rng = arr::Mt19937::new(seed as u32);
        let mut a = input.clone();
        arr::shuffle(&mut rng, &mut a).expect("shuffle");
        let first = encode(&Value::Array(a.clone()));
        arr::shuffle(&mut rng, &mut a).expect("shuffle");
        let second = encode(&Value::Array(a));
        m.check(
            || format!("shuffle({input:?}) seed {seed}"),
            json!([first, second]),
            row[2].clone(),
        );
    }

    for row in v["rand"].as_array().expect("rand") {
        let seed = row[0].as_i64().expect("seed");
        let input = Value::Array(decode_array(&row[1]));
        let num = row[2].as_i64().expect("num");
        let mut rng = arr::Mt19937::new(seed as u32);
        let mut d = Vec::new();
        let r = arr::array_rand(&mut rng, &input, Some(&Value::Int(num)), &mut d);
        m.check(
            || format!("array_rand({input:?}, {num}) seed {seed}"),
            res(&r),
            php_res(&row[3], &msgs),
        );
        m.check(
            || format!("array_rand({input:?}, {num}) diagnostics"),
            diag_list(&d),
            resolve_diags(&row[4], &msgs),
        );
        m.check(
            || format!("mt_rand() after array_rand({input:?}, {num})"),
            arr::mt_rand(&mut rng),
            row[5].as_i64().expect("state"),
        );
    }

    for row in v["errors"].as_array().expect("errors") {
        let func = s(&row[0]);
        let args: Vec<Value> = row[1]
            .as_array()
            .expect("args")
            .iter()
            .map(decode)
            .collect();
        let mut rng = arr::Mt19937::new(1);
        let mut d = Vec::new();
        let r = match func {
            "mt_rand" => arr::mt_rand_call(&mut rng, &args, &mut d).map(Value::Int),
            "random_int" => arr::random_int(
                &args[0],
                &args[1],
                &mut || panic!("random_int must not draw here"),
                &mut d,
            )
            .map(Value::Int),
            "array_rand" => arr::array_rand(&mut rng, &args[0], args.get(1), &mut d),
            other => panic!("unknown function {other}"),
        };
        m.check(
            || format!("{func}({args:?})"),
            res_json(&r),
            resolve_res(&row[2], &msgs),
        );
        m.check(
            || format!("{func}({args:?}) diagnostics"),
            diag_list(&d),
            resolve_diags(&row[3], &msgs),
        );
    }
    m.finish();
}

// ---------------------------------------------------------------------------------------------
// functions.json
// ---------------------------------------------------------------------------------------------

/// Calls one array function the way `functions.gen.php` does (`args` excludes the callback).
fn call_function(
    func: &str,
    args: &[Value],
    cb: Option<&mut arr::Callback<'_>>,
    d: &mut Vec<twig_value::Diagnostic>,
) -> PhpResult<Value> {
    let a = |i: usize| args.get(i);
    let arr_ = |r: PhpResult<Array>| r.map(Value::Array);
    match func {
        "array_merge" => arr_(arr::array_merge(args)),
        "array_chunk" => arr_(arr::array_chunk(&args[0], &args[1], a(2), d)),
        "array_column" => arr_(arr::array_column(&args[0], &args[1], a(2), d)),
        "array_slice" => arr_(arr::array_slice(&args[0], &args[1], a(2), a(3), d)),
        "array_keys" => arr_(arr::array_keys(&args[0], a(1), a(2), d)),
        "array_values" => arr_(arr::array_values(&args[0])),
        "array_reverse" => arr_(arr::array_reverse(&args[0], a(1), d)),
        "array_key_first" => arr::array_key_first(&args[0]),
        "array_key_last" => arr::array_key_last(&args[0]),
        "array_is_list" => arr::array_is_list(&args[0]).map(Value::Bool),
        "in_array" => arr::in_array(&args[0], &args[1], a(2), d).map(Value::Bool),
        "array_search" => arr::array_search(&args[0], &args[1], a(2), d),
        "array_key_exists" => arr::array_key_exists(&args[0], &args[1], d).map(Value::Bool),
        "array_pad" => arr_(arr::array_pad(&args[0], &args[1], &args[2], d)),
        "array_fill_keys" => arr_(arr::array_fill_keys(&args[0], &args[1], d)),
        "array_flip" => arr_(arr::array_flip(&args[0], d)),
        "array_filter" => {
            // Without a callback the vector passes null explicitly as the second argument.
            let mode = if cb.is_some() { a(1) } else { a(2) };
            arr_(arr::array_filter(&args[0], cb, mode, d))
        }
        "array_map" => arr_(arr::array_map(cb, args, d)),
        "array_reduce" => arr::array_reduce(&args[0], cb.expect("callback"), a(1), d),
        "iterator_to_array" => arr_(arr::iterator_to_array(&args[0], a(1), d)),
        "iterator_count" => arr::iterator_count(&args[0], d).map(Value::Int),
        "array_intersect_key" => arr_(arr::array_intersect_key(args)),
        "array_diff_key" => arr_(arr::array_diff_key(args)),
        other => panic!("unknown function {other}"),
    }
}

#[test]
fn functions_vectors() {
    let v = load("array/functions.json");
    let msgs = messages(&v);
    let values: Vec<Value> = v["values"]
        .as_array()
        .expect("values")
        .iter()
        .map(decode)
        .collect();
    let mut m = Mismatches::new("array/functions");
    for row in v["calls"].as_array().expect("calls") {
        let func = s(&row[0]);
        let ix = ints(&row[1]);
        let args: Vec<Value> = ix.iter().map(|&i| values[i as usize].clone()).collect();
        let cb_name = row[2].as_str();
        let mut d = Vec::new();
        let mut calls = 0u64;
        let r = match cb_name {
            Some(name) => {
                let mut f = callback(name, &mut calls);
                call_function(func, &args, Some(&mut f), &mut d)
            }
            None => call_function(func, &args, None, &mut d),
        };
        let what = || format!("{func}{ix:?} cb={cb_name:?} ({args:?})");
        m.check(what, res(&r), php_res(&row[3], &msgs));
        m.check(
            || format!("{func}{ix:?} cb={cb_name:?} diagnostics"),
            diag_list(&d),
            resolve_diags(&row[4], &msgs),
        );
        m.check(
            || format!("{func}{ix:?} cb={cb_name:?} callback calls"),
            calls,
            row[5].as_u64().expect("calls"),
        );
    }
    m.finish();
}
