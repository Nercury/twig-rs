//! Comparison operators (`== != < <= > >= <=> === !==`) against PHP 8.5.13: the value matrix of
//! engine-semantics.md §7.10 extended with edge cases, and random number/string pairs.
//!
//! Regenerate `tests/vectors/compare.json` with the Docker oracle (`research/php-oracle`):
//!
//! ```text
//! cd crates/twig-value/tests/vectors
//! MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//!     php /t/compare.gen.php > compare.json
//! ```

mod support;

use support::values::{messages, resolve_diags, typed_value};
use support::{Mismatches, diag_list, hex, load, s};
use twig_value::compare::{self, CompareOp};
use twig_value::{PhpError, Value};

const OPS: [CompareOp; 9] = [
    CompareOp::Equal,
    CompareOp::NotEqual,
    CompareOp::Less,
    CompareOp::LessOrEqual,
    CompareOp::Greater,
    CompareOp::GreaterOrEqual,
    CompareOp::Spaceship,
    CompareOp::Identical,
    CompareOp::NotIdentical,
];

/// Per-operator extras of one matrix cell: operator index, exception, diagnostics.
type Extra = (usize, Option<(String, String)>, Vec<(String, String)>);

fn code(r: &Result<Value, PhpError>) -> char {
    match r {
        Err(_) => 'E',
        Ok(Value::Bool(true)) => 't',
        Ok(Value::Bool(false)) => 'f',
        Ok(Value::Int(i)) if *i < 0 => '-',
        Ok(Value::Int(0)) => '0',
        Ok(Value::Int(_)) => '+',
        Ok(other) => panic!("unexpected comparison result {other:?}"),
    }
}

#[test]
fn compare_vectors() {
    let v = load("compare.json");
    let msgs = messages(&v);
    let ops: Vec<&str> = v["ops"].as_array().expect("ops").iter().map(s).collect();
    assert_eq!(
        ops,
        OPS.iter().map(|o| o.symbol()).collect::<Vec<_>>(),
        "operator order"
    );
    let values: Vec<Value> = v["values"]
        .as_array()
        .expect("values")
        .iter()
        .map(typed_value)
        .collect();
    let mut m = Mismatches::new("compare");
    for row in v["matrix"].as_array().expect("matrix") {
        let i = row[0].as_u64().expect("i") as usize;
        let j = row[1].as_u64().expect("j") as usize;
        let (a, b) = (&values[i], &values[j]);
        let mut codes = String::new();
        let mut extra = Vec::new();
        for (k, op) in OPS.iter().enumerate() {
            let mut diag = Vec::new();
            let r = compare::compare_op(*op, a, b, &mut diag);
            codes.push(code(&r));
            if !diag.is_empty() || r.is_err() {
                let err = r
                    .as_ref()
                    .err()
                    .map(|e| (e.class().name().to_string(), hex(e.message().as_bytes())));
                extra.push((k, err, diag_list(&diag)));
            }
        }
        let want_extra: Vec<Extra> = row
            .get(3)
            .and_then(|e| e.as_array())
            .map(|list| {
                list.iter()
                    .map(|e| {
                        let err = e[1].as_array().map(|err| {
                            (
                                s(&err[0]).to_string(),
                                msgs[err[1].as_u64().expect("idx") as usize].clone(),
                            )
                        });
                        (
                            e[0].as_u64().expect("op") as usize,
                            err,
                            resolve_diags(&e[2], &msgs),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        m.check(
            || format!("{a:?} vs {b:?} (ops {})", ops.join(" ")),
            (codes, extra),
            (s(&row[2]).to_string(), want_extra),
        );
    }
    for row in v["random"].as_array().expect("random") {
        let a = typed_value(&row[0]);
        let b = typed_value(&row[1]);
        let cmp = compare::spaceship(&a, &b, &mut Vec::new()).expect("<=>");
        let eq = compare::equals(&a, &b, &mut Vec::new()).expect("==");
        m.check(
            || format!("{a:?} <=> {b:?}, =="),
            (cmp, eq),
            (row[2].as_i64().expect("<=>"), row[3].as_bool().expect("==")),
        );
    }
    m.finish();
}
