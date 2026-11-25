//! Thread-safety and stack-usage checks for the vendored PCRE2 and the per-thread PHP state.

use std::sync::Arc;
use std::thread;

use twig_pcre::pcre2::{Code, MatchData, compile_options};
use twig_pcre::{Pcre, PregError, matches};

#[test]
fn per_thread_state_matches_concurrently() {
    let jit = Pcre::new().jit_supported();
    let handles: Vec<_> = (0..8)
        .map(|t| {
            thread::spawn(move || {
                for i in 0..200 {
                    let subject = format!("item-{t}-{i}");
                    assert_eq!(matches(b"/^item-\\d+-\\d+$/", subject.as_bytes()), Ok(1));
                    if jit {
                        // Each thread has its own JIT stack (192 KiB), as each PHP thread does.
                        assert_eq!(
                            matches(b"/^(a|b)*$/", &b"a".repeat(6144))
                                .unwrap_err()
                                .message(b"x"),
                            b"Regexp \"x\" passed to \"matches\" failed: JIT stack limit exhausted."
                        );
                        assert_eq!(twig_pcre::preg_last_error(), PregError::JitStackLimit);
                        assert_eq!(matches(b"/^(a|b)*$/", &b"a".repeat(6143)), Ok(1));
                    }
                    assert_eq!(matches(b"/\\d/u", "٣".as_bytes()), Ok(1));
                    assert_eq!(twig_pcre::preg_last_error(), PregError::NoError);
                }
            })
        })
        .collect();
    for h in handles {
        h.join().expect("thread panicked");
    }
}

#[test]
fn compiled_code_is_shared_between_threads() {
    let mut code = Code::compile(
        b"(\\w+)@(\\w+)",
        compile_options::UCP | compile_options::UTF,
        None,
    )
    .expect("compiles");
    code.jit_compile(1);
    let code = Arc::new(code);
    let handles: Vec<_> = (0..8)
        .map(|t| {
            let code = Arc::clone(&code);
            thread::spawn(move || {
                let mut md = MatchData::for_code(&code);
                for i in 0..500 {
                    let subject = format!("mail ü{t}@host{i}");
                    assert_eq!(code.match_at(subject.as_bytes(), 0, 0, &mut md, None), 3);
                    assert_eq!(md.group(1), Some((5, 7 + t.to_string().len())));
                }
            })
        })
        .collect();
    for h in handles {
        h.join().expect("thread panicked");
    }
}

/// Measured: 256 KiB overflows (Windows x64, PCRE2 at -O2), 384 KiB suffices; JIT compilation is the deepest part.
const STACK: usize = 512 * 1024;

#[test]
fn deepest_nesting_compiles_on_a_small_stack() {
    // PARENS_NEST_LIMIT 250 bounds the recursion of pcre2_compile() and pcre2_jit_compile(), so the
    // deepest valid pattern needs a bounded amount of stack.
    let result = thread::Builder::new()
        .stack_size(STACK)
        .spawn(|| {
            let mut pcre = Pcre::new();
            let pattern = format!("/{}a{}/", "(".repeat(250), ")".repeat(250));
            let ok = pcre.preg_match(pattern.as_bytes(), b"a", &mut |_| {});
            let pattern = format!("/{}a{}/", "(".repeat(251), ")".repeat(251));
            let mut warnings = Vec::new();
            let too_deep = pcre.preg_match(pattern.as_bytes(), b"a", &mut |d| {
                warnings.push(d.to_string())
            });
            (ok, too_deep, warnings)
        })
        .expect("spawn")
        .join()
        .expect("thread panicked");
    assert_eq!(result.0, Some(1));
    assert_eq!(result.1, None);
    assert_eq!(
        result.2,
        vec![
            "preg_match(): Compilation failed: parentheses are too deeply nested at offset 251"
                .to_owned()
        ]
    );
}
