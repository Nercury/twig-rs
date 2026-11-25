//! Operators (`+ - * / % ** . & | ^ << >> xor`, Twig's `//`, unary `- + ~ !`) against PHP 8.5.13,
//! over a value matrix with diagnostics and exceptions.
//!
//! Regenerate `tests/vectors/operators.json` with the Docker oracle (`research/php-oracle`):
//!
//! ```text
//! cd crates/twig-value/tests/vectors
//! MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//!     php /t/operators.gen.php > operators.json
//! ```

mod support;

use support::values::{messages, res_json, resolve_diags, resolve_res, typed_value};
use support::{Mismatches, diag_list, load};
use twig_value::ops::{self, BinaryOp};
use twig_value::{Diagnostics, PhpResult, Value};

fn binary(op: &str, a: &Value, b: &Value, diag: &mut dyn Diagnostics) -> PhpResult<Value> {
    let op = match op {
        "+" => BinaryOp::Add,
        "-" => BinaryOp::Sub,
        "*" => BinaryOp::Mul,
        "/" => BinaryOp::Div,
        "%" => BinaryOp::Mod,
        "**" => BinaryOp::Pow,
        "." => BinaryOp::Concat,
        "&" => BinaryOp::BitAnd,
        "|" => BinaryOp::BitOr,
        "^" => BinaryOp::BitXor,
        "<<" => BinaryOp::ShiftLeft,
        ">>" => BinaryOp::ShiftRight,
        "xor" => return ops::boolean_xor(a, b, diag).map(Value::Bool),
        "//" => return ops::floor_div(a, b, diag),
        other => panic!("unknown operator {other}"),
    };
    ops::binary_op(op, a, b, diag)
}

fn unary(op: &str, a: &Value, diag: &mut dyn Diagnostics) -> PhpResult<Value> {
    match op {
        "-" => ops::negate(a, diag),
        "+" => ops::unary_plus(a, diag),
        "~" => ops::bitwise_not(a, diag),
        "!" => ops::boolean_not(a, diag).map(Value::Bool),
        other => panic!("unknown operator {other}"),
    }
}

/// NaN results are compared by kind (sign and payload are not observable from PHP).
fn normalize_nan(mut j: serde_json::Value) -> serde_json::Value {
    if let Some(ok) = j.get_mut("ok")
        && ok["t"] == "float"
        && let Some(bits) = ok["bits"].as_str()
    {
        let f = f64::from_bits(u64::from_str_radix(bits, 16).expect("bits"));
        if f.is_nan() {
            ok["bits"] = serde_json::json!("nan");
        }
    }
    j
}

#[test]
fn operator_vectors() {
    let v = load("operators.json");
    let msgs = messages(&v);
    let values: Vec<Value> = v["values"]
        .as_array()
        .expect("values")
        .iter()
        .map(typed_value)
        .collect();
    let mut m = Mismatches::new("operators");
    for (op, rows) in v["binary"].as_object().expect("binary") {
        for row in rows.as_array().expect("rows") {
            let i = row[0].as_u64().expect("i") as usize;
            let j = row[1].as_u64().expect("j") as usize;
            let mut diag = Vec::new();
            let got = binary(op, &values[i], &values[j], &mut diag);
            m.check(
                || format!("{:?} {op} {:?}", values[i], values[j]),
                (normalize_nan(res_json(&got)), diag_list(&diag)),
                (
                    normalize_nan(resolve_res(&row[2], &msgs)),
                    resolve_diags(&row[3], &msgs),
                ),
            );
        }
    }
    for (op, rows) in v["unary"].as_object().expect("unary") {
        for row in rows.as_array().expect("rows") {
            let i = row[0].as_u64().expect("i") as usize;
            let mut diag = Vec::new();
            let got = unary(op, &values[i], &mut diag);
            m.check(
                || format!("{op}{:?}", values[i]),
                (normalize_nan(res_json(&got)), diag_list(&diag)),
                (
                    normalize_nan(resolve_res(&row[1], &msgs)),
                    resolve_diags(&row[2], &msgs),
                ),
            );
        }
    }
    m.finish();
}
