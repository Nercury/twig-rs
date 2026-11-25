//! Replays the method probes of `tests/vectors/fixture_data.json` on the converted objects and
//! compares every result, exception and diagnostic with PHP's.
//!
//! Regenerate the vectors with the Docker oracle (`research/php-oracle`, image `twig-oracle:3.31.1`):
//!
//! ```text
//! cd crates/twig-test-support/tests/vectors
//! MSYS_NO_PATHCONV=1 docker run --rm -v "$(pwd -W 2>/dev/null || pwd):/t" twig-oracle:3.31.1 \
//!     php /t/fixture_data.gen.php > fixture_data.json
//! ```
//!
//! The PHP generator ran the probes on the DATA objects right after encoding them, target after
//! target, op after op; state changes (iterator positions, generator counters, constructor
//! re-calls) carry over exactly like here. Results are rich typed trees that refer to the context
//! objects by their tree ids; new objects (iterators, SimpleXML children, generator objects) are
//! matched by identity renaming.

mod support;

use std::collections::BTreeMap;

use support::{Op, StubFactory, data_file, php_diag, rust_diag, stub_hook};
use twig_test_support::TypedNode;
use twig_test_support::compare::{CompareOptions, Comparer};
use twig_test_support::convert::Session;
use twig_test_support::encode::Encoder;
use twig_value::object::{CallArgs, ObjectRef, PropertyCheck};
use twig_value::{Array, Collect, FetchMode, PhpResult, PhpString, Value, convert, dim};

/// Userland classes whose `class_implements()` order differs (reference/deviations.md).
const UNORDERED: &[&str] = &[
    "Twig\\Tests\\SimpleIteratorForTesting",
    "ItemsIteratorCountable",
];

/// `foreach ($o as $k => $x)` collecting `[$k, $x]` pairs, stopping after `limit` items (PHP's
/// loop breaks before calling `next()` again).
fn foreach(o: &ObjectRef, limit: usize, diag: &mut Collect) -> PhpResult<Value> {
    let mut out = Array::new();
    let pair = |k: Value, v: Value| Value::Array(Array::from_values([k, v]));
    match o.get_iterator(diag)? {
        Some(mut it) => {
            it.rewind(diag)?;
            while it.valid(diag)? {
                let v = it.current(diag)?;
                let k = it.key(diag)?;
                let _ = out.push(pair(k, v));
                if out.len() >= limit {
                    break;
                }
                it.next(diag)?;
            }
        }
        None => {
            for (k, v) in o.visible_properties(diag)? {
                let _ = out.push(pair(Value::String(k), v));
                if out.len() >= limit {
                    break;
                }
            }
        }
    }
    Ok(Value::Array(out))
}

fn run_op(
    o: &ObjectRef,
    op: &Op,
    args: Vec<Value>,
    named: Vec<(PhpString, Value)>,
    offset: Option<Value>,
    diag: &mut Collect,
) -> PhpResult<Value> {
    let this = Value::Object(o.clone());
    let name = || op.name.clone().unwrap_or_default();
    match op.op.as_str() {
        "methods" => Ok(Value::Array(Array::from_values(
            o.class()
                .public_method_names()
                .iter()
                .map(|m| Value::from(&**m)),
        ))),
        "bool" => convert::to_bool(&this, diag).map(Value::Bool),
        "array_cast" => convert::to_array(&this, diag).map(Value::Array),
        "string" => convert::to_string(&this, diag).map(Value::String),
        "count" => dim::count(&this, 0, diag).map(Value::Int),
        "call" => o.call_method(
            op.method.as_deref().unwrap_or_default(),
            CallArgs {
                positional: args,
                named,
            },
            diag,
        ),
        "prop" => o.read_property(name().as_bytes(), FetchMode::Read, diag),
        "isset_prop" => o
            .has_property(name().as_bytes(), PropertyCheck::Isset, diag)
            .map(Value::Bool),
        "dim" => dim::read(&this, &offset.unwrap_or_default(), FetchMode::Read, diag),
        "isset_dim" => dim::isset(&this, &offset.unwrap_or_default(), diag).map(Value::Bool),
        "iterator_count" => twig_value::object::iterator_count(o, diag).map(Value::Int),
        "to_array" => twig_value::object::iterator_to_array(o, op.keys.unwrap_or(true), diag)
            .map(Value::Array),
        "iterate" => foreach(o, op.limit.unwrap_or(50), diag),
        other => panic!("unknown probe op {other}"),
    }
}

#[test]
fn probes_match_php() {
    let file = data_file();
    let mut ops_checked = 0usize;
    let mut per_class: BTreeMap<String, usize> = BTreeMap::new();
    let mut failures = Vec::new();
    for case in &file.cases {
        if case.probes.is_empty() {
            continue;
        }
        let Some(tree) = &case.data else { continue };
        let what = format!("{}#{}", case.file, case.index);
        let mut session = Session::new(&StubFactory);
        session
            .convert(tree)
            .unwrap_or_else(|e| panic!("{what}: {e:?}"));
        let mut encoder = Encoder::new(true, Some(&stub_hook));
        let mut comparer = Comparer::new(CompareOptions {
            extras: true,
            ignore_volatile_dates: false,
            unordered_interfaces_for: UNORDERED,
        });
        comparer.learn(tree, tree);
        let mut known = std::collections::BTreeSet::new();
        let mut sync =
            |session: &Session<'_>, encoder: &mut Encoder<'_>, comparer: &mut Comparer| {
                for (id, v) in session.objects() {
                    if known.insert(*id) {
                        if let Value::Object(o) = v {
                            encoder.mark_seen(o, *id);
                        }
                        comparer.pair(*id, *id).unwrap();
                    }
                }
            };
        sync(&session, &mut encoder, &mut comparer);
        for target in &case.probes {
            let class = target.class.split('\0').next().unwrap().to_string();
            let Some(Value::Object(obj)) = session.objects().get(&target.id).cloned() else {
                panic!("{what}: probe target #{} not converted", target.id);
            };
            for (n, op) in target.ops.iter().enumerate() {
                let label = format!(
                    "{what} {class}#{} op {n} {}{}",
                    target.id,
                    op.op,
                    op.method
                        .as_deref()
                        .or(op.name.as_deref())
                        .map(|m| format!(" {m}"))
                        .unwrap_or_default()
                );
                // Arguments: tree objects resolve through the session, new ones are created.
                let mut convert_arg = |node: &TypedNode, comparer: &mut Comparer| {
                    comparer.learn(node, node);
                    session
                        .convert(node)
                        .unwrap_or_else(|e| panic!("{label}: argument: {e:?}"))
                };
                let args: Vec<Value> = op
                    .args
                    .iter()
                    .map(|a| convert_arg(a, &mut comparer))
                    .collect();
                let named: Vec<(PhpString, Value)> = op
                    .named
                    .iter()
                    .map(|(k, a)| (PhpString::from(k.as_str()), convert_arg(a, &mut comparer)))
                    .collect();
                let offset = op.offset.as_ref().map(|a| convert_arg(a, &mut comparer));
                sync(&session, &mut encoder, &mut comparer);

                let mut diag = Collect::new();
                let result = run_op(&obj, op, args, named, offset, &mut diag);
                ops_checked += 1;
                *per_class.entry(class.clone()).or_default() += 1;
                match (&op.ok, &op.err, result) {
                    (Some(expected), None, Ok(v)) => {
                        let got = encoder.encode(&v);
                        if let Err(e) = comparer.compare(expected, &got) {
                            failures.push(format!("{label}: {e}"));
                        }
                    }
                    (None, Some(expected), Err(e)) => {
                        if expected.class != e.class().name()
                            || expected.message_bytes() != *e.message()
                        {
                            failures.push(format!(
                                "{label}: PHP {}: {}, Rust {}: {}",
                                expected.class,
                                expected.message_bytes().to_string_lossy(),
                                e.class().name(),
                                e.message_lossy()
                            ));
                        }
                    }
                    (_, Some(expected), Ok(v)) => failures.push(format!(
                        "{label}: PHP threw {}: {}, Rust returned {v:?}",
                        expected.class,
                        expected.message_bytes().to_string_lossy()
                    )),
                    (_, _, Err(e)) => {
                        failures.push(format!("{label}: PHP returned, Rust threw {e}"))
                    }
                    _ => failures.push(format!("{label}: malformed probe")),
                }
                let want: Vec<_> = op.diag.iter().map(|d| php_diag(d)).collect();
                let have: Vec<_> = diag.take().iter().map(rust_diag).collect();
                if want != have {
                    failures.push(format!("{label} diagnostics: PHP {want:?}, Rust {have:?}"));
                }
            }
        }
    }
    eprintln!("{ops_checked} probe ops: {per_class:#?}");
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert!(ops_checked > 1000, "only {ops_checked} probe ops");
}
