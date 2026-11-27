//! An environment converts charsets with its iconv backend (`EnvironmentOptions::iconv_backend`, PHP's `iconv()`):
//! the `convert_encoding` filter, the escaping strategies under a charset other than UTF-8 (through the environment's
//! escaper runtime), and `reverse`, `shuffle` and `random` under such a charset. A recording backend shows every call;
//! the default backend follows the `glibc-iconv` feature.

use std::sync::{Arc, Mutex, PoisonError};

use twig::context;
use twig::loader::ArrayLoader;
use twig::runtime::{
    BasicIconv, Environment, EnvironmentOptions, IconvBackend, default_iconv_backend,
};
use twig::value::{Array, Diagnostics, PhpResult, PhpString};

/// Converts with the clean-room backend and records each conversion as `from>to`.
#[derive(Debug, Default)]
struct Recording {
    calls: Mutex<Vec<String>>,
}

impl Recording {
    fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.calls.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

impl IconvBackend for Recording {
    fn iconv_bytes(
        &self,
        from: &[u8],
        to: &[u8],
        string: &[u8],
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Option<PhpString>> {
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(format!(
                "{}>{}",
                String::from_utf8_lossy(from),
                String::from_utf8_lossy(to)
            ));
        BasicIconv.iconv_bytes(from, to, string, diag)
    }
}

/// An environment with the `ISO-8859-1` charset over one template, converting through `backend`.
fn environment(template: &str, backend: Arc<Recording>) -> Environment {
    Environment::with_options(
        ArrayLoader::new([("index", template)]),
        EnvironmentOptions {
            charset: "ISO-8859-1".to_owned(),
            iconv_backend: backend,
            ..Default::default()
        },
    )
}

#[test]
fn every_charset_conversion_goes_through_the_environment_backend() {
    // (template, the variable `s`, output, conversions); the templates print variables because Twig does not
    // auto-escape literals
    let cases: &[(&str, &str, &str, &[&str])] = &[
        // CoreExtension::convertEncoding()
        (
            "{{ s|convert_encoding('UTF-16LE', 'ISO-8859-1')|url_encode }}",
            "ab",
            "a%00b%00",
            &["ISO-8859-1>UTF-16LE"],
        ),
        // EscaperRuntime::escape(): to UTF-8 and back, explicitly and when auto-escaping
        (
            "{{ s|e('html_attr') }}",
            "a b",
            "a&#x20;b",
            &["ISO-8859-1>UTF-8", "UTF-8>ISO-8859-1"],
        ),
        (
            "{% autoescape 'js' %}{{ s }}{% endautoescape %}",
            "a/b",
            r"a\/b",
            &["ISO-8859-1>UTF-8", "UTF-8>ISO-8859-1"],
        ),
        // `html` passes ISO-8859-1 to htmlspecialchars() itself; a charset outside Twig's list goes through UTF-8
        ("{{ s }}", "<", "&lt;", &[]),
        (
            "{{ s|e('html', 'ASCII') }}",
            "<",
            "&lt;",
            &["ASCII>UTF-8", "UTF-8>ASCII"],
        ),
        // reverse, shuffle and random split the string in UTF-8
        (
            "{{ s|reverse }}",
            "abc",
            "cba",
            &["ISO-8859-1>UTF-8", "UTF-8>ISO-8859-1"],
        ),
        (
            "{{ s|shuffle }}",
            "aaa",
            "aaa",
            &["ISO-8859-1>UTF-8", "UTF-8>ISO-8859-1"],
        ),
        (
            "{{ random(s) }}",
            "aa",
            "a",
            &["ISO-8859-1>UTF-8", "UTF-8>ISO-8859-1", "UTF-8>ISO-8859-1"],
        ),
    ];
    for (template, s, output, calls) in cases {
        let backend = Arc::new(Recording::default());
        let env = environment(template, Arc::clone(&backend));
        assert_eq!(
            env.render("index", context! { s => *s }).unwrap(),
            *output,
            "{template}"
        );
        assert_eq!(backend.take(), *calls, "{template}");
    }
}

#[test]
fn the_escaper_runtime_of_an_environment_has_its_backend() {
    let backend = Arc::new(Recording::default());
    let env = environment("", Arc::clone(&backend));
    let backend: Arc<dyn IconvBackend> = backend;
    assert!(Arc::ptr_eq(&env.iconv_backend(), &backend));
    assert!(Arc::ptr_eq(
        &env.escaper_runtime().iconv_backend(),
        &backend
    ));
}

#[test]
fn the_default_backend_follows_the_glibc_iconv_feature() {
    let expected = if cfg!(feature = "glibc-iconv") {
        "GlibcIconv"
    } else {
        "BasicIconv"
    };
    assert_eq!(format!("{:?}", default_iconv_backend()), expected);
    let env = Environment::new(ArrayLoader::new([
        (
            "index",
            "{{ 'é'|convert_encoding('ISO-2022-JP', 'UTF-8')|url_encode }}",
        ),
        ("ebcdic", "{{ 'é'|convert_encoding('IBM930', 'UTF-8') }}"),
    ]));
    assert_eq!(format!("{:?}", env.iconv_backend()), expected);
    // é is not in JIS X 0208: glibc's notice, and `false` turned into '' (both backends convert ISO-2022-JP)
    assert_eq!(env.render("index", Array::new()).unwrap(), "");
    if !cfg!(feature = "glibc-iconv") {
        // a charset beyond the default backend
        let error = env.render("ebcdic", Array::new()).unwrap_err().to_string();
        assert!(
            error
                .contains(r#"iconv(): conversion of charset "IBM930" is not supported by twig-rs"#),
            "{error}"
        );
    }
}
