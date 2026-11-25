//! `var_dump()`, `var_export()` and `print_r()` against PHP 8.5.13.
//!
//! Regenerate `tests/vectors/dumps.json` with the Docker oracle (`research/php-oracle`):
//!
//! ```text
//! cd crates/twig-value/tests/vectors
//! MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//!     php /t/dumps.gen.php > dumps.json
//! ```
//!
//! Object handles (`#N`) are allocation-order dependent and cannot match PHP (`ObjectRef::id()` is a
//! process-wide counter); both sides are normalised to `#N` before comparing.

mod support;

use support::values::typed_value;
use support::{Mismatches, diag_list, json_diags, load, s, unhex};
use twig_value::{PhpString, dump};

/// Replaces the handle in every `object(Class)#123 (` with `#N`.
fn normalize_handles(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        if text[i] == b'#' && i > 0 && text[i - 1] == b')' {
            let mut j = i + 1;
            while j < text.len() && text[j].is_ascii_digit() {
                j += 1;
            }
            if j > i + 1 && text[j..].starts_with(b" (") {
                out.extend_from_slice(b"#N");
                i = j;
                continue;
            }
        }
        out.push(text[i]);
        i += 1;
    }
    out
}

fn show(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn dump_vectors() {
    let v = load("dumps.json");
    let mut m = Mismatches::new("dumps");
    for case in v["cases"].as_array().expect("cases") {
        let value = typed_value(&case["v"]);
        type DumpFn = fn(
            &twig_value::Value,
            &mut dyn twig_value::Diagnostics,
        ) -> twig_value::PhpResult<PhpString>;
        let fns: [(&str, DumpFn); 3] = [
            ("var_dump", dump::var_dump),
            ("var_export", dump::var_export),
            ("print_r", dump::print_r),
        ];
        for (name, f) in fns {
            let mut diag = Vec::new();
            let got = f(&value, &mut diag).expect("dump");
            let want = unhex(s(&case[name]));
            m.check(
                || format!("{name}({value:?})"),
                (show(&normalize_handles(&got)), diag_list(&diag)),
                (
                    show(&normalize_handles(&want)),
                    json_diags(&case[format!("{name}_diag")]),
                ),
            );
        }
    }
    m.finish();
}
