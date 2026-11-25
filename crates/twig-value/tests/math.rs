//! `**` against PHP 8.5.13: the `int ** int` squaring loop and the C library `pow()` (glibc 2.41,
//! FMA build) bit for bit.
//!
//! Regenerate `tests/vectors/pow.json` with the Docker oracle (`research/php-oracle`):
//!
//! ```text
//! cd crates/twig-value/tests/vectors
//! MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//!     php /t/pow.gen.php > pow.json
//! ```

mod support;

use support::{Mismatches, bits, diag_list, json_diags, load, s, to_bits};
use twig_value::{Value, math, ops};

/// NaN results are compared by kind only (their sign/payload is not observable in PHP output).
fn float_key(f: f64) -> String {
    if f.is_nan() {
        "nan".to_string()
    } else {
        to_bits(f)
    }
}

#[test]
fn math_vectors() {
    let v = load("math.json");
    let mut m = Mismatches::new("math");
    for row in v["ints"].as_array().expect("ints") {
        let b = row[0].as_i64().expect("base");
        let e = row[1].as_i64().expect("exponent");
        let got = ops::pow(&Value::Int(b), &Value::Int(e), &mut Vec::new());
        let got = match got {
            Ok(Value::Int(i)) => i.to_string(),
            Ok(Value::Float(f)) => format!("f:{}", float_key(f)),
            other => format!("{other:?}"),
        };
        let want = match &row[2] {
            j if j.is_i64() => j.as_i64().expect("int").to_string(),
            j => format!("f:{}", float_key(bits(s(&j["f"])))),
        };
        m.check(|| format!("{b} ** {e}"), got, want);
    }
    for row in v["floats"].as_array().expect("floats") {
        let (x, y) = (bits(s(&row[0])), bits(s(&row[1])));
        m.check(
            || format!("pow({x:e}, {y:e}) [{} {}]", s(&row[0]), s(&row[1])),
            float_key(math::pow(x, y)),
            float_key(bits(s(&row[2]))),
        );
        let mut diag = Vec::new();
        let op = ops::pow(&Value::Float(x), &Value::Float(y), &mut diag);
        m.check(
            || format!("{x:e} ** {y:e}"),
            match op {
                Ok(Value::Float(f)) => float_key(f),
                other => format!("{other:?}"),
            },
            float_key(bits(s(&row[2]))),
        );
    }
    for row in v["diags"].as_array().expect("diags") {
        let (x, y) = (bits(s(&row[0])), bits(s(&row[1])));
        let mut diag = Vec::new();
        let _ = ops::pow(&Value::Float(x), &Value::Float(y), &mut diag);
        m.check(
            || format!("diagnostics of {x:e} ** {y:e}"),
            diag_list(&diag),
            json_diags(&row[2]),
        );
    }
    for row in v["round"].as_array().expect("round") {
        let x = bits(s(&row[0]));
        m.check(
            || format!("floor/ceil({x:e}) [{}]", s(&row[0])),
            (float_key(math::floor(x)), float_key(math::ceil(x))),
            (float_key(bits(s(&row[1]))), float_key(bits(s(&row[2])))),
        );
    }
    for row in v["fmod"].as_array().expect("fmod") {
        let (x, y) = (bits(s(&row[0])), bits(s(&row[1])));
        m.check(
            || format!("fmod({x:e}, {y:e})"),
            float_key(math::fmod(x, y)),
            float_key(bits(s(&row[2]))),
        );
    }
    m.finish();
}
