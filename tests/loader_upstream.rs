//! Ports of Twig 3.31.1's `tests/Loader/FilesystemTest.php` and `tests/Loader/ChainTest.php`.
//!
//! `tests/loader_fixtures/` mirrors `TWIG/tests/`: `Fixtures/errors/` holds the two files of
//! `TWIG/tests/Fixtures/errors/` these tests load, and `Loader/Fixtures/` is `TWIG/tests/Loader/Fixtures/` without
//! the phar archive. Cargo runs integration tests in the package root, which plays the role of PHPUnit's current
//! directory (the Twig checkout) in `testPaths`.
//!
//! `testLoadTemplateAndRenderBlockWithCache` and `testArrayInheritance` render through an `Environment`; their
//! loader halves are checked separately. `testLoadTemplateFromPhar` documents that `phar://` paths are not
//! supported.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use twig::loader::{ArrayLoader, ChainLoader, FilesystemLoader, Loader};
use twig::runtime::Environment;
use twig::value::Array;
use twig::{ErrorKind, Result, Source};

const MAIN: &str = FilesystemLoader::MAIN_NAMESPACE;

/// `__DIR__` of the PHP tests: `TWIG/tests/Loader`.
fn dir() -> String {
    format!(
        "{}/tests/loader_fixtures/Loader",
        env!("CARGO_MANIFEST_DIR")
    )
}

/// `__DIR__.'/../Fixtures'`: `TWIG/tests/Fixtures`.
fn twig_fixtures() -> String {
    format!("{}/../Fixtures", dir())
}

/// PHP `realpath()` (`std::fs::canonicalize()` without Windows' `\\?\` prefix).
fn realpath(path: &str) -> String {
    let real = std::fs::canonicalize(path)
        .unwrap_or_else(|error| panic!("realpath({path}): {error}"))
        .to_string_lossy()
        .into_owned();
    match real.strip_prefix(r"\\?\") {
        Some(stripped) => stripped.to_owned(),
        None => real,
    }
}

fn code(loader: &dyn Loader, name: &str) -> String {
    let source = loader
        .get_source_context(name)
        .unwrap_or_else(|error| panic!("{name}: {error}"));
    String::from_utf8(source.code().to_vec()).expect("UTF-8 fixture")
}

fn error_message(result: Result<impl std::fmt::Debug>) -> String {
    let error = result.expect_err("a LoaderError should have been thrown");
    assert_eq!(error.kind(), ErrorKind::Loader, "{error}");
    error.to_string()
}

#[test]
fn get_source_context() {
    let path = twig_fixtures();
    let loader = FilesystemLoader::new([&path]).unwrap();
    let source = loader.get_source_context("errors/index.html").unwrap();
    assert_eq!(source.name(), "errors/index.html");
    assert_eq!(
        realpath(source.path()),
        realpath(&format!("{path}/errors/index.html"))
    );
}

#[test]
fn get_source_context_normalizes_name() {
    let loader = FilesystemLoader::new([twig_fixtures()]).unwrap();
    loader.add_path(&twig_fixtures(), "foo").unwrap();
    for (expected, name) in [
        ("errors/index.html", "errors\\index.html"),
        ("errors/index.html", "errors//index.html"),
        ("errors/index.html", "/errors/index.html"),
        ("errors/index.html", "./errors/index.html"),
        ("errors/index.html", "errors/./index.html"),
        ("errors/index.html", "filters/../errors/index.html"),
        ("errors/index.html", "filters/./..//errors/index.html"),
        ("errors/index.html", "@__main__/errors/index.html"),
        ("errors/index.html", "@__main__/./errors/index.html"),
        ("@foo/errors/index.html", "@foo/errors/index.html"),
        ("@foo/errors/index.html", "@foo//errors/index.html"),
        ("@foo/errors/index.html", "@foo/./errors/index.html"),
        (
            "@foo/errors/index.html",
            "@foo/filters/../errors/index.html",
        ),
    ] {
        assert_eq!(
            loader.get_source_context(name).unwrap().name(),
            expected,
            "{name}"
        );
    }
}

#[test]
fn normalizing_a_name_keeps_its_namespace() {
    let loader = FilesystemLoader::new([format!("{}/Fixtures/normal", dir())]).unwrap();
    loader
        .add_path(&format!("{}/Fixtures/named", dir()), "named")
        .unwrap();

    assert!(loader.exists("@named/index.html"));
    assert!(!loader.exists("/@named/index.html"));
    assert!(!loader.exists("./@named/index.html"));
    assert!(!loader.exists("@__main__/@named/index.html"));
}

#[test]
fn error_messages_use_the_name_as_passed() {
    let loader = FilesystemLoader::new([twig_fixtures()]).unwrap();
    for (expected, name) in [
        (
            "Unable to find template \"errors\\missing.html\"",
            "errors\\missing.html",
        ),
        (
            "Unable to find template \"/errors/missing.html\"",
            "/errors/missing.html",
        ),
        (
            "Unable to find template \"./errors/missing.html\"",
            "./errors/missing.html",
        ),
        (
            "Unable to find template \"@__main__/errors/missing.html\"",
            "@__main__/errors/missing.html",
        ),
        (
            "outside configured directories (errors/../../../etc/passwd)",
            "errors/../../../etc/passwd",
        ),
    ] {
        let message = error_message(loader.get_source_context(name));
        assert!(message.contains(expected), "{message:?} for {name:?}");
    }
}

#[test]
fn error_messages_do_not_depend_on_the_spelling_that_failed_first() {
    let loader = FilesystemLoader::new([twig_fixtures()]).unwrap();
    error_message(loader.get_source_context("errors\\missing.html"));

    let message = error_message(loader.get_source_context("errors/missing.html"));
    assert!(
        message.contains("Unable to find template \"errors/missing.html\""),
        "{message}"
    );
}

#[test]
fn security() {
    let loader = FilesystemLoader::new([twig_fixtures()]).unwrap();
    loader.add_path(&twig_fixtures(), "foo").unwrap();
    for template in [
        "AutoloaderTest\0.php",
        "..\\AutoloaderTest.php",
        "..\\\\\\AutoloaderTest.php",
        "../AutoloaderTest.php",
        "..////AutoloaderTest.php",
        "./../AutoloaderTest.php",
        ".\\..\\AutoloaderTest.php",
        "././././././../AutoloaderTest.php",
        ".\\./.\\./.\\./../AutoloaderTest.php",
        "foo/../../AutoloaderTest.php",
        "foo\\..\\..\\AutoloaderTest.php",
        "foo/../bar/../../AutoloaderTest.php",
        "foo/bar/../../../AutoloaderTest.php",
        "filters/../../AutoloaderTest.php",
        "filters//..//..//AutoloaderTest.php",
        "filters\\..\\..\\AutoloaderTest.php",
        "filters\\\\..\\\\..\\\\AutoloaderTest.php",
        "filters\\//../\\/\\..\\AutoloaderTest.php",
        "/../AutoloaderTest.php",
        "@__main__/../AutoloaderTest.php",
        "@foo/../AutoloaderTest.php",
        "@__main__/../../AutoloaderTest.php",
        "@foo/../../AutoloaderTest.php",
    ] {
        let message = error_message(loader.get_cache_key(template));
        assert!(
            !message.contains("Unable to find template"),
            "{template:?}: {message}"
        );
    }
}

#[test]
fn paths() {
    let cwd = std::env::current_dir()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let cases = [
        (
            format!("{}/Fixtures", dir()),
            "tests/loader_fixtures/Loader/Fixtures/named_quater/named_absolute.html",
            None,
        ),
        (
            format!("{}/Fixtures/../Fixtures", dir()),
            "tests/loader_fixtures/Loader/Fixtures/named_quater/named_absolute.html",
            None,
        ),
        (
            "tests/loader_fixtures/Loader/Fixtures".to_owned(),
            "tests/loader_fixtures/Loader/Fixtures/named_quater/named_absolute.html",
            Some(cwd.clone()),
        ),
        (
            "Fixtures".to_owned(),
            "Fixtures/named_quater/named_absolute.html",
            Some(format!("{cwd}/tests/loader_fixtures/Loader")),
        ),
        (
            "Fixtures".to_owned(),
            "Fixtures/named_quater/named_absolute.html",
            Some(format!("{cwd}/tests/../tests/loader_fixtures/Loader")),
        ),
    ];
    for (base_path, cache_key, root_path) in cases {
        let main_paths = [
            format!("{base_path}/normal"),
            format!("{base_path}/normal_bis"),
        ];
        let loader = match &root_path {
            Some(root_path) => FilesystemLoader::with_root_path(main_paths, root_path),
            None => FilesystemLoader::new(main_paths),
        }
        .unwrap();
        loader
            .set_paths(
                [
                    format!("{base_path}/named"),
                    format!("{base_path}/named_bis"),
                ],
                "named",
            )
            .unwrap();
        loader
            .add_path(&format!("{base_path}/named_ter"), "named")
            .unwrap();
        loader
            .add_path(&format!("{base_path}/normal_ter"), MAIN)
            .unwrap();
        loader
            .prepend_path(&format!("{base_path}/normal_final"), MAIN)
            .unwrap();
        loader
            .prepend_path(&format!("{base_path}/named/../named_quater"), "named")
            .unwrap();
        loader
            .prepend_path(&format!("{base_path}/named_final"), "named")
            .unwrap();

        assert_eq!(
            loader.get_paths(MAIN),
            [
                format!("{base_path}/normal_final"),
                format!("{base_path}/normal"),
                format!("{base_path}/normal_bis"),
                format!("{base_path}/normal_ter"),
            ]
        );
        assert_eq!(
            loader.get_paths("named"),
            [
                format!("{base_path}/named_final"),
                format!("{base_path}/named/../named_quater"),
                format!("{base_path}/named"),
                format!("{base_path}/named_bis"),
                format!("{base_path}/named_ter"),
            ]
        );

        // do not use realpath here as it would make the test unuseful
        let key = loader.get_cache_key("@named/named_absolute.html").unwrap();
        assert_eq!(
            String::from_utf8(key).unwrap().replace('\\', "/"),
            cache_key,
            "{base_path} with root {root_path:?}"
        );
        assert_eq!(code(&loader, "index.html"), "path (final)\n");
        assert_eq!(code(&loader, "@__main__/index.html"), "path (final)\n");
        assert_eq!(code(&loader, "@named/index.html"), "named path (final)\n");
    }
}

#[test]
fn empty_constructor() {
    let loader = FilesystemLoader::default();
    assert!(loader.get_paths(MAIN).is_empty());
    let loader = FilesystemLoader::new(Vec::<String>::new()).unwrap();
    assert!(loader.get_paths(MAIN).is_empty());
}

#[test]
fn get_namespaces() {
    let temp = std::env::temp_dir().to_string_lossy().into_owned();
    let loader = FilesystemLoader::new([&temp]).unwrap();
    assert_eq!(loader.get_namespaces(), [MAIN]);

    loader.add_path(&temp, "named").unwrap();
    assert_eq!(loader.get_namespaces(), [MAIN, "named"]);
}

#[test]
fn adding_a_path_that_is_not_a_directory_throws() {
    for prepend in [false, true] {
        for path in ["Fixtures/missing", "Fixtures/normal/index.html"] {
            let loader = FilesystemLoader::with_root_path(Vec::<String>::new(), &dir()).unwrap();
            let result = if prepend {
                loader.prepend_path(path, "named")
            } else {
                loader.add_path(path, "named")
            };
            assert_eq!(
                error_message(result),
                format!(
                    "The \"{path}\" directory does not exist (\"{}{}{path}\").",
                    realpath(&dir()),
                    std::path::MAIN_SEPARATOR
                )
            );
            assert!(loader.get_paths("named").is_empty());
        }
    }
}

#[test]
fn adding_a_path_without_checking_it_accepts_a_missing_directory() {
    for prepend in [false, true] {
        let loader = FilesystemLoader::with_root_path(Vec::<String>::new(), &dir()).unwrap();
        loader.add_path_unchecked("Fixtures/named", "named");
        if prepend {
            loader.prepend_path_unchecked("Fixtures/missing/", "named");
        } else {
            loader.add_path_unchecked("Fixtures/missing/", "named");
        }

        let expected = if prepend {
            ["Fixtures/missing", "Fixtures/named"]
        } else {
            ["Fixtures/named", "Fixtures/missing"]
        };
        assert_eq!(loader.get_paths("named"), expected);
        assert_eq!(code(&loader, "@named/index.html"), "named path\n");

        assert_eq!(
            error_message(loader.get_source_context("@named/nowhere.html")),
            format!(
                "Unable to find template \"@named/nowhere.html\" (looked into: {}).",
                expected.join(", ")
            )
        );
    }
}

#[test]
fn adding_a_path_without_checking_it_invalidates_the_cache() {
    for prepend in [false, true] {
        let loader = FilesystemLoader::with_root_path(Vec::<String>::new(), &dir()).unwrap();
        loader.add_path("Fixtures/normal", "named").unwrap();
        assert!(!loader.exists("@named/named_absolute.html"));

        if prepend {
            loader.prepend_path_unchecked("Fixtures/named_quater", "named");
        } else {
            loader.add_path_unchecked("Fixtures/named_quater", "named");
        }

        assert_eq!(
            code(&loader, "@named/named_absolute.html"),
            "named path (quater)\n"
        );
    }
}

#[test]
fn find_template_exception_namespace() {
    let base_path = format!("{}/Fixtures", dir());
    let loader = FilesystemLoader::new([format!("{base_path}/normal")]).unwrap();
    loader
        .add_path(&format!("{base_path}/named"), "named")
        .unwrap();

    let message = error_message(loader.get_source_context("@named/nowhere.html"));
    assert!(
        message.contains("Unable to find template \"@named/nowhere.html\""),
        "{message}"
    );
}

#[test]
fn find_template_with_cache() {
    let base_path = format!("{}/Fixtures", dir());
    let loader = FilesystemLoader::new([format!("{base_path}/normal")]).unwrap();
    loader
        .add_path(&format!("{base_path}/named"), "named")
        .unwrap();

    // prime the cache for index.html in the named namespace
    assert_eq!(code(&loader, "@named/index.html"), "named path\n");

    // get index.html from the main namespace
    assert_eq!(code(&loader, "index.html"), "path\n");
}

/// The loader half of `testLoadTemplateAndRenderBlockWithCache`: the theme directories are searched in order and
/// the namespaced copy stays reachable.
#[test]
fn load_template_with_cache_loader_part() {
    let loader = FilesystemLoader::default();
    loader
        .add_path(&format!("{}/Fixtures/themes/theme2", dir()), MAIN)
        .unwrap();
    loader
        .add_path(&format!("{}/Fixtures/themes/theme1", dir()), MAIN)
        .unwrap();
    loader
        .add_path(
            &format!("{}/Fixtures/themes/theme1", dir()),
            "default_theme",
        )
        .unwrap();

    assert!(
        code(&loader, "blocks.html.twig")
            .starts_with("{% use '@default_theme/blocks.html.twig' %}")
    );
    assert!(
        code(&loader, "@default_theme/blocks.html.twig")
            .starts_with("{% block b1 %}block from theme 1")
    );
}

/// `testLoadTemplateAndRenderBlockWithCache`: `blocks.html.twig` of theme 2 uses the blocks of theme 1 through
/// the `default_theme` namespace and overrides `b2`; the second load comes from the cache.
#[test]
fn load_template_and_render_block_with_cache() {
    let loader = FilesystemLoader::new(Vec::<String>::new()).unwrap();
    loader
        .add_path(&format!("{}/Fixtures/themes/theme2", dir()), MAIN)
        .unwrap();
    loader
        .add_path(&format!("{}/Fixtures/themes/theme1", dir()), MAIN)
        .unwrap();
    loader
        .add_path(
            &format!("{}/Fixtures/themes/theme1", dir()),
            "default_theme",
        )
        .unwrap();
    let twig = Environment::new(loader);

    let template = twig.load("blocks.html.twig").unwrap();
    assert_eq!(
        template.render_block("b1", Array::new()).unwrap(),
        "block from theme 1"
    );

    let template = twig.load("blocks.html.twig").unwrap();
    assert_eq!(
        template.render_block("b2", Array::new()).unwrap(),
        "block from theme 2"
    );
}

/// `testArrayInheritance`: `{% extends ['...', 'parent.html.twig'] %}` takes the first parent that exists (an
/// empty name and a missing template are skipped).
#[test]
fn array_inheritance() {
    for name in [
        "array_inheritance_valid_parent.html.twig",
        "array_inheritance_empty_parent.html.twig",
        "array_inheritance_nonexistent_parent.html.twig",
    ] {
        let loader = FilesystemLoader::new(Vec::<String>::new()).unwrap();
        loader
            .add_path(&format!("{}/Fixtures/inheritance", dir()), MAIN)
            .unwrap();
        let twig = Environment::new(loader);
        let template = twig.load(name).unwrap();
        assert_eq!(
            template.render_block("body", Array::new()).unwrap(),
            "VALID Child",
            "{name}"
        );
    }
}

/// The loader half of `testArrayInheritance`: the parents named by the fixtures resolve or fail as expected.
#[test]
fn array_inheritance_loader_part() {
    let loader = FilesystemLoader::default();
    loader
        .add_path(&format!("{}/Fixtures/inheritance", dir()), MAIN)
        .unwrap();
    for name in [
        "array_inheritance_valid_parent.html.twig",
        "array_inheritance_empty_parent.html.twig",
        "array_inheritance_nonexistent_parent.html.twig",
        "parent.html.twig",
        "spare_parent.html.twig",
    ] {
        assert!(loader.exists(name), "{name}");
    }
    assert!(!loader.exists("nonexistent.html.twig"));
    assert!(!loader.exists(""));
}

/// `testLoadTemplateFromPhar`: stream wrappers other than `file://` are not supported, so a `phar://` directory
/// does not exist (documented deviation).
#[test]
fn load_template_from_phar_is_not_supported() {
    let loader = FilesystemLoader::default();
    let path = format!("phar://{}/Fixtures/phar/phar-sample.phar", dir());
    assert_eq!(
        error_message(loader.add_path(&path, MAIN)),
        format!("The \"{path}\" directory does not exist (\"{path}\").")
    );
}

#[test]
fn template_exists_always_returns_bool() {
    let loader = FilesystemLoader::default();
    assert!(!loader.exists("foo\0.twig"));
    assert!(!loader.exists("../foo.twig"));
    assert!(!loader.exists("@foo"));
    assert!(!loader.exists("foo"));
    assert!(!loader.exists("@foo/bar.twig"));

    loader
        .add_path(&format!("{}/Fixtures/normal", dir()), MAIN)
        .unwrap();
    assert!(loader.exists("index.html"));
    loader
        .add_path(&format!("{}/Fixtures/normal", dir()), "foo")
        .unwrap();
    assert!(loader.exists("@foo/index.html"));
}

// ---- ChainTest.php ----

fn array_loader<const N: usize>(templates: [(&str, &str); N]) -> Arc<dyn Loader> {
    Arc::new(ArrayLoader::new(templates))
}

#[test]
fn chain_get_source_context() {
    let path = twig_fixtures();
    let loader = ChainLoader::new([
        array_loader([("foo", "bar")]),
        array_loader([("errors/index.html", "baz")]),
        Arc::new(FilesystemLoader::new([&path]).unwrap()),
    ]);

    assert_eq!(loader.get_source_context("foo").unwrap().name(), "foo");
    assert_eq!(loader.get_source_context("foo").unwrap().path(), "");

    let source = loader.get_source_context("errors/index.html").unwrap();
    assert_eq!(source.name(), "errors/index.html");
    assert_eq!(source.path(), "");
    assert_eq!(source.code(), b"baz");

    let source = loader.get_source_context("errors/base.html").unwrap();
    assert_eq!(source.name(), "errors/base.html");
    assert_eq!(
        realpath(source.path()),
        realpath(&format!("{path}/errors/base.html"))
    );
    assert_ne!(source.code(), b"baz");
}

#[test]
fn chain_get_source_context_when_template_does_not_exist() {
    let loader = ChainLoader::new([]);
    error_message(loader.get_source_context("foo"));
}

#[test]
fn chain_get_cache_key() {
    let loader = ChainLoader::new([
        array_loader([("foo", "bar")]),
        array_loader([("foo", "foobar"), ("bar", "foo")]),
    ]);

    assert_eq!(loader.get_cache_key("foo").unwrap(), b"foo:bar");
    assert_eq!(loader.get_cache_key("bar").unwrap(), b"bar:foo");
}

#[test]
fn chain_get_cache_key_when_template_does_not_exist() {
    let loader = ChainLoader::new([]);
    error_message(loader.get_cache_key("foo"));
}

#[test]
fn chain_add_loader() {
    let foo_loader = array_loader([("foo", "foo:code")]);
    let bar_loader = array_loader([("bar", "bar:code")]);
    let baz_loader = array_loader([("baz", "baz:code")]);
    let qux_loader = array_loader([("qux", "qux:code")]);

    let loader = ChainLoader::new([foo_loader.clone(), bar_loader.clone()]);
    loader.add_loader(baz_loader.clone());
    loader.add_loader(qux_loader.clone());

    assert_eq!(code(&loader, "foo"), "foo:code");
    assert_eq!(code(&loader, "bar"), "bar:code");
    assert_eq!(code(&loader, "baz"), "baz:code");
    assert_eq!(code(&loader, "qux"), "qux:code");

    let loaders = loader.get_loaders();
    let expected = [foo_loader, bar_loader, baz_loader, qux_loader];
    assert_eq!(loaders.len(), expected.len());
    for (actual, expected) in loaders.iter().zip(&expected) {
        assert!(Arc::ptr_eq(actual, expected));
    }
}

/// A `createMock(LoaderInterface::class)` stand-in that counts calls.
struct MockLoader {
    exists: bool,
    exists_calls: AtomicUsize,
    source_calls: AtomicUsize,
}

impl MockLoader {
    fn new(exists: bool) -> Arc<Self> {
        Arc::new(MockLoader {
            exists,
            exists_calls: AtomicUsize::new(0),
            source_calls: AtomicUsize::new(0),
        })
    }
}

impl Loader for MockLoader {
    fn get_source_context(&self, name: &str) -> Result<Source> {
        self.source_calls.fetch_add(1, Ordering::SeqCst);
        Ok(Source::new("", name))
    }

    fn get_cache_key(&self, name: &str) -> Result<Vec<u8>> {
        Ok(name.as_bytes().to_vec())
    }

    fn is_fresh(&self, _name: &str, _time: i64) -> Result<bool> {
        Ok(true)
    }

    fn exists(&self, _name: &str) -> bool {
        self.exists_calls.fetch_add(1, Ordering::SeqCst);
        self.exists
    }
}

#[test]
fn chain_exists() {
    let loader1 = MockLoader::new(false);
    let loader2 = MockLoader::new(true);

    let loader = ChainLoader::default();
    loader.add_loader(loader1.clone());
    loader.add_loader(loader2.clone());

    assert!(loader.exists("foo"));
    // memoized: asking again does not reach the loaders
    assert!(loader.exists("foo"));

    assert_eq!(loader1.exists_calls.load(Ordering::SeqCst), 1);
    assert_eq!(loader1.source_calls.load(Ordering::SeqCst), 0);
    assert_eq!(loader2.exists_calls.load(Ordering::SeqCst), 1);
    assert_eq!(loader2.source_calls.load(Ordering::SeqCst), 0);
}

#[test]
fn loaders_are_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<FilesystemLoader>();
    assert_send_sync::<ChainLoader>();
}
