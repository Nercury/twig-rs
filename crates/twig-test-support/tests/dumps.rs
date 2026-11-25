//! `var_dump()`, `var_export()` and `print_r()` of every top-level DATA value, compared with PHP's
//! output recorded in `tests/vectors/fixture_data.json`.
//!
//! Regenerate the vectors with the Docker oracle (`research/php-oracle`, image `twig-oracle:3.31.1`):
//!
//! ```text
//! cd crates/twig-test-support/tests/vectors
//! MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//!     php /t/fixture_data.gen.php > fixture_data.json
//! ```
//!
//! The converted values are dumped with `twig-value`'s ports. Object handles (`#N`) are mapped
//! through the conversion's identities: a Rust object prints the PHP id of the object it was
//! converted from, so shared objects and cycles are checked too; temporary objects created while
//! dumping (SimpleXML children) print as `#?` on both sides. Values containing stand-ins for
//! factory classes other than `Twig\Markup` (dates, template wrappers, resources) are skipped.

mod support;

use std::collections::{BTreeSet, HashMap};

use support::{DumpKey, StubFactory, contains_opaque_stub, data_file, php_diag, rust_diag};
use twig_test_support::convert::Session;
use twig_test_support::json::field_bytes;
use twig_value::{Collect, PhpString, Value, dump};

/// Rewrites the `)#N (` handles of a `var_dump()`.
fn map_handles(out: &[u8], map: &dyn Fn(u64) -> Option<i64>) -> Vec<u8> {
    let mut res = Vec::with_capacity(out.len());
    let mut i = 0;
    while i < out.len() {
        if out[i..].starts_with(b")#") {
            let start = i + 2;
            let mut end = start;
            while end < out.len() && out[end].is_ascii_digit() {
                end += 1;
            }
            if end > start && out[end..].starts_with(b" (") {
                let n: u64 = std::str::from_utf8(&out[start..end])
                    .unwrap()
                    .parse()
                    .unwrap();
                res.extend_from_slice(b")#");
                match map(n) {
                    Some(id) => res.extend_from_slice(id.to_string().as_bytes()),
                    None => res.push(b'?'),
                }
                i = end;
                continue;
            }
        }
        res.push(out[i]);
        i += 1;
    }
    res
}

#[test]
fn dumps_match_php() {
    let file = data_file();
    let mut checked = 0usize;
    let mut skipped = BTreeSet::new();
    let mut failures = Vec::new();
    for case in &file.cases {
        let Some(tree) = &case.data else { continue };
        let what = format!("{}#{}", case.file, case.index);
        let mut session = Session::new(&StubFactory);
        let context = session
            .convert(tree)
            .unwrap_or_else(|e| panic!("{what}: {e:?}"));
        let php_ids: BTreeSet<i64> = session.objects().keys().copied().collect();
        let rust_to_php: HashMap<u64, i64> = session
            .objects()
            .iter()
            .filter_map(|(id, v)| v.as_object().map(|o| (o.id(), *id)))
            .collect();
        let context = context.as_array().unwrap().clone();
        for (key, dumps) in &case.dumps {
            if let Some(reason) = &dumps.skipped {
                skipped.insert(format!("{what}[{key:?}]: {reason}"));
                continue;
            }
            let value = match key {
                DumpKey::Int(i) => context.get_int(*i),
                DumpKey::Str(s) => context.get_str(s.as_bytes()),
            }
            .cloned()
            .unwrap_or_else(|| panic!("{what}: no key {key:?}"));
            if contains_opaque_stub(&value) {
                skipped.insert(format!("{what}[{key:?}]: factory stand-in"));
                continue;
            }
            let mut check =
                |name: &str,
                 expected: &Option<String>,
                 expected_b64: &Option<String>,
                 expected_diag: &Option<Vec<Vec<String>>>,
                 run: &dyn Fn(&Value, &mut Collect) -> twig_value::PhpResult<PhpString>,
                 handles: bool| {
                    let Some(expected) = expected else {
                        failures.push(format!("{what}[{key:?}] {name}: PHP failed"));
                        return;
                    };
                    let expected = field_bytes(expected, expected_b64.as_deref());
                    let mut diag = Collect::new();
                    let got = match run(&value, &mut diag) {
                        Ok(s) => s,
                        Err(e) => {
                            failures.push(format!("{what}[{key:?}] {name}: {e}"));
                            return;
                        }
                    };
                    let (expected, got) = if handles {
                        (
                            map_handles(&expected, &|n| {
                                let id = n as i64;
                                php_ids.contains(&id).then_some(id)
                            }),
                            map_handles(&got, &|n| rust_to_php.get(&n).copied()),
                        )
                    } else {
                        (expected.to_vec(), got.to_vec())
                    };
                    checked += 1;
                    if expected != got {
                        failures.push(format!(
                            "{what}[{key:?}] {name}:\n--- PHP\n{}\n--- Rust\n{}",
                            String::from_utf8_lossy(&expected),
                            String::from_utf8_lossy(&got)
                        ));
                    }
                    let want: Vec<_> = expected_diag
                        .iter()
                        .flatten()
                        .map(|d| php_diag(d))
                        .collect();
                    let have: Vec<_> = diag.take().iter().map(rust_diag).collect();
                    if want != have {
                        failures.push(format!(
                            "{what}[{key:?}] {name} diagnostics: PHP {want:?}, Rust {have:?}"
                        ));
                    }
                };
            check(
                "var_dump",
                &dumps.var_dump,
                &dumps.var_dump_b64,
                &dumps.var_dump_diag,
                &|v, d| dump::var_dump(v, d),
                true,
            );
            check(
                "var_export",
                &dumps.var_export,
                &dumps.var_export_b64,
                &dumps.var_export_diag,
                &|v, d| dump::var_export(v, d),
                false,
            );
            check(
                "print_r",
                &dumps.print_r,
                &dumps.print_r_b64,
                &dumps.print_r_diag,
                &|v, d| dump::print_r(v, d),
                false,
            );
        }
    }
    eprintln!("{checked} dumps compared; skipped: {skipped:#?}");
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert_eq!(checked, 3 * 352, "dump count changed");
}
