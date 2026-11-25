//! Oracle vectors for `FilesystemLoader` and `ChainLoader`: `tests/vectors/loader/scenarios.json`, recorded from
//! Twig 3.31.1 on PHP 8.5.13 working on a real directory tree.
//!
//! Regenerate from the repository root with (Git Bash on Windows needs `MSYS_NO_PATHCONV=1`):
//!
//! ```text
//! MSYS_NO_PATHCONV=1 docker run --rm -i twig-oracle:3.31.1 php < tests/vectors/loader/scenarios.gen.php > tests/vectors/loader/scenarios.json
//! ```
//!
//! Every scenario is replayed on a fresh copy of the vector's tree under `CARGO_TARGET_TMPDIR`, with `<ROOT>`
//! replaced by that directory in inputs and outputs. Where PHP emitted a warning or notice (and carried on), twig-rs
//! must return a runtime error with that message. On Windows, scenarios and steps marked `unix_only` are skipped
//! and `\` counts as `/` in compared strings, since `DIRECTORY_SEPARATOR` and `realpath()` use backslashes there.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

use serde_json::{Value, json};
use twig::loader::{ArrayLoader, ChainLoader, FilesystemLoader, Loader};
use twig::{Error, ErrorKind, Result, Source};

/// The loaders a scenario creates, by id.
enum AnyLoader {
    Filesystem(Arc<FilesystemLoader>),
    Array(Arc<ArrayLoader>),
    Chain(Arc<ChainLoader>),
    Throwing(Arc<ThrowingLoader>),
}

impl AnyLoader {
    fn as_loader(&self) -> Arc<dyn Loader> {
        match self {
            AnyLoader::Filesystem(loader) => loader.clone(),
            AnyLoader::Array(loader) => loader.clone(),
            AnyLoader::Chain(loader) => loader.clone(),
            AnyLoader::Throwing(loader) => loader.clone(),
        }
    }
}

/// The generator's `ThrowingLoader` class: a fixed `exists()` answer, and every other method throws.
struct ThrowingLoader {
    label: String,
    exists: bool,
    throws: String,
}

impl ThrowingLoader {
    fn fail(&self, name: &str) -> Error {
        let message = format!("{} cannot load \"{name}\".", self.label);
        match self.throws.as_str() {
            "loader" => Error::loader(message),
            "loader_with_line" => Error::loader(message)
                .with_line(3)
                .with_source(Source::new("", "origin.twig")),
            "runtime" => Error::runtime(message),
            other => panic!("unknown exception kind {other}"),
        }
    }
}

impl Loader for ThrowingLoader {
    fn get_source_context(&self, name: &str) -> Result<Source> {
        Err(self.fail(name))
    }

    fn get_cache_key(&self, name: &str) -> Result<Vec<u8>> {
        Err(self.fail(name))
    }

    fn is_fresh(&self, name: &str, _time: i64) -> Result<bool> {
        Err(self.fail(name))
    }

    fn exists(&self, _name: &str) -> bool {
        self.exists
    }

    fn class_name(&self) -> &str {
        "ThrowingLoader"
    }
}

/// A scenario's directory tree.
struct Tree {
    dir: PathBuf,
    /// `dir` as the loader reports it (canonical, without Windows' `\\?\` prefix).
    root: String,
}

impl Tree {
    fn create(index: usize, spec: &Value) -> Tree {
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("loader-vectors-{}-{index}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let canonical = std::fs::canonicalize(&dir)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let root = match canonical.strip_prefix(r"\\?\") {
            Some(stripped) => stripped.to_owned(),
            None => canonical,
        };
        let tree = Tree { dir, root };
        for dir in spec["dirs"].as_array().unwrap() {
            std::fs::create_dir_all(tree.path(dir.as_str().unwrap())).unwrap();
        }
        for (file, content) in spec["files"].as_object().unwrap() {
            std::fs::write(tree.path(file), content.as_str().unwrap()).unwrap();
        }
        tree
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.dir.join(relative)
    }

    /// `<ROOT>` in a vector input.
    fn subst(&self, text: &str) -> String {
        text.replace("<ROOT>", &self.root)
    }

    fn fs_step(&self, step: &Value) {
        let path = self.path(step["path"].as_str().unwrap());
        match step["fs"].as_str().unwrap() {
            "write" => std::fs::write(&path, step["content"].as_str().unwrap()).unwrap(),
            "unlink" => std::fs::remove_file(&path).unwrap(),
            "mkdir" => std::fs::create_dir_all(&path).unwrap(),
            "touch" => {
                let mtime = UNIX_EPOCH + Duration::from_secs(step["mtime"].as_u64().unwrap());
                std::fs::File::options()
                    .write(true)
                    .open(&path)
                    .unwrap()
                    .set_modified(mtime)
                    .unwrap();
            }
            "symlink" => symlink(&self.subst(step["target"].as_str().unwrap()), &path),
            other => panic!("unknown filesystem step {other}"),
        }
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[cfg(unix)]
fn symlink(target: &str, link: &Path) {
    std::os::unix::fs::symlink(target, link).unwrap();
}

#[cfg(not(unix))]
fn symlink(_target: &str, _link: &Path) {
    unreachable!("symlink scenarios are unix_only");
}

/// What a step produced, in the vector's terms.
#[derive(Debug, PartialEq)]
enum Outcome {
    Ok(Value),
    Error { kind: ErrorKind, message: String },
}

fn expected_outcome(expect: &Value, tree: &Tree) -> Outcome {
    if let Some(warnings) = expect.get("warnings") {
        // PHP warned and carried on; twig-rs reports the (first) warning as a runtime error.
        let mut message = warnings[0].as_str().unwrap().to_owned();
        adjust_read_size(&mut message, tree);
        return Outcome::Error {
            kind: ErrorKind::Runtime,
            message,
        };
    }
    if let Some(error) = expect.get("error") {
        let kind = match error["class"].as_str().unwrap() {
            "Twig\\Error\\LoaderError" => ErrorKind::Loader,
            "Twig\\Error\\RuntimeError" => ErrorKind::Runtime,
            // PHP's ValueError (realpath() of a root path with a NUL byte)
            "ValueError" => ErrorKind::Logic,
            other => panic!("unknown class {other}"),
        };
        return Outcome::Error {
            kind,
            message: error["message"].as_str().unwrap().to_owned(),
        };
    }
    Outcome::Ok(expect["ok"].clone())
}

/// PHP's notice for reading a directory names its first read size, `st_size + 8192`; directory sizes depend on the
/// filesystem, so the expectation is recomputed for the directory of the replay (`tpl/a.twig` in the vectors).
fn adjust_read_size(message: &mut String, tree: &Tree) {
    let (Some(start), Some(end)) = (message.find("Read of "), message.find(" bytes failed")) else {
        return;
    };
    let size = std::fs::metadata(tree.path("tpl/a.twig"))
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    let first_read = if size > 0 { size + 8192 } else { 8192 };
    message.replace_range(start + "Read of ".len()..end, &first_read.to_string());
}

fn actual_outcome(result: Result<Value>, tree: &Tree) -> Outcome {
    match result {
        Ok(value) => Outcome::Ok(unsubst_value(value, tree)),
        Err(error) => Outcome::Error {
            kind: error.kind(),
            message: error.to_string().replace(&tree.root, "<ROOT>"),
        },
    }
}

fn unsubst_value(value: Value, tree: &Tree) -> Value {
    match value {
        Value::String(text) => Value::String(text.replace(&tree.root, "<ROOT>")),
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .map(|item| unsubst_value(item, tree))
                .collect(),
        ),
        Value::Object(fields) => Value::Object(
            fields
                .into_iter()
                .map(|(key, value)| {
                    let value = if key == "code" {
                        value
                    } else {
                        unsubst_value(value, tree)
                    };
                    (key, value)
                })
                .collect(),
        ),
        other => other,
    }
}

/// On Windows `\` and `/` are compared as equal (outside template code).
fn platform_normalize(outcome: Outcome) -> Outcome {
    if !cfg!(windows) {
        return outcome;
    }
    fn slashes(value: Value) -> Value {
        match value {
            Value::String(text) => Value::String(text.replace('\\', "/")),
            Value::Array(items) => Value::Array(items.into_iter().map(slashes).collect()),
            Value::Object(fields) => Value::Object(
                fields
                    .into_iter()
                    .map(|(key, value)| {
                        let value = if key == "code" { value } else { slashes(value) };
                        (key, value)
                    })
                    .collect(),
            ),
            other => other,
        }
    }
    match outcome {
        Outcome::Ok(value) => Outcome::Ok(slashes(value)),
        Outcome::Error { kind, message } => Outcome::Error {
            kind,
            message: message.replace('\\', "/"),
        },
    }
}

fn source_json(source: Source) -> Value {
    json!({
        "code": String::from_utf8_lossy(source.code()),
        "name": source.name(),
        "path": source.path(),
    })
}

fn strings(value: &Value, tree: &Tree) -> Vec<String> {
    match value {
        Value::String(text) => vec![tree.subst(text)],
        Value::Array(items) => items
            .iter()
            .map(|item| tree.subst(item.as_str().unwrap()))
            .collect(),
        other => panic!("expected paths, got {other}"),
    }
}

fn run_step(tree: &Tree, loaders: &mut HashMap<String, AnyLoader>, step: &Value) -> Result<Value> {
    if let Some(id) = step["new"].as_str() {
        let loader = match step["class"].as_str().unwrap() {
            "filesystem" => {
                let paths = strings(&step["paths"], tree);
                let loader = match step["root"].as_str() {
                    Some(root) => FilesystemLoader::with_root_path(paths, &tree.subst(root))?,
                    None => FilesystemLoader::new(paths)?,
                };
                AnyLoader::Filesystem(Arc::new(loader))
            }
            "array" => {
                let templates = match &step["templates"] {
                    Value::Object(templates) => templates
                        .iter()
                        .map(|(name, code)| (name.clone(), code.as_str().unwrap().to_owned()))
                        .collect(),
                    _ => Vec::new(),
                };
                AnyLoader::Array(Arc::new(ArrayLoader::new(templates)))
            }
            "chain" => AnyLoader::Chain(Arc::new(ChainLoader::new(
                step["loaders"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|id| loaders[id.as_str().unwrap()].as_loader()),
            ))),
            "throwing" => AnyLoader::Throwing(Arc::new(ThrowingLoader {
                label: step["label"].as_str().unwrap().to_owned(),
                exists: step["exists"].as_bool().unwrap(),
                throws: step["throws"].as_str().unwrap().to_owned(),
            })),
            other => panic!("unknown loader class {other}"),
        };
        loaders.insert(id.to_owned(), loader);
        return Ok(Value::Null);
    }

    let target = &loaders[step["on"].as_str().unwrap()];
    let args: Vec<Value> = step["args"]
        .as_array()
        .map(|args| {
            args.iter()
                .map(|arg| match arg {
                    Value::String(text) => Value::String(tree.subst(text)),
                    other => other.clone(),
                })
                .collect()
        })
        .unwrap_or_default();
    let text = |index: usize| args[index].as_str().unwrap();
    let namespace = || {
        args.get(1)
            .map_or(FilesystemLoader::MAIN_NAMESPACE, |ns| ns.as_str().unwrap())
    };
    let checked = || args.get(2).is_none_or(|check| check.as_bool().unwrap());
    match (step["call"].as_str().unwrap(), target) {
        ("getSourceContext", _) => target
            .as_loader()
            .get_source_context(text(0))
            .map(source_json),
        ("getCacheKey", _) => target
            .as_loader()
            .get_cache_key(text(0))
            .map(|key| Value::String(String::from_utf8_lossy(&key).into_owned())),
        ("isFresh", _) => target
            .as_loader()
            .is_fresh(text(0), args[1].as_i64().unwrap())
            .map(Value::Bool),
        ("exists", _) => Ok(Value::Bool(target.as_loader().exists(text(0)))),
        ("getPaths", AnyLoader::Filesystem(loader)) => Ok(json!(loader.get_paths(text(0)))),
        ("getNamespaces", AnyLoader::Filesystem(loader)) => Ok(json!(loader.get_namespaces())),
        ("addPath", AnyLoader::Filesystem(loader)) => {
            if checked() {
                loader.add_path(text(0), namespace())?;
            } else {
                loader.add_path_unchecked(text(0), namespace());
            }
            Ok(Value::Null)
        }
        ("prependPath", AnyLoader::Filesystem(loader)) => {
            if checked() {
                loader.prepend_path(text(0), namespace())?;
            } else {
                loader.prepend_path_unchecked(text(0), namespace());
            }
            Ok(Value::Null)
        }
        ("setPaths", AnyLoader::Filesystem(loader)) => {
            loader.set_paths(strings(&args[0], tree), namespace())?;
            Ok(Value::Null)
        }
        ("setTemplate", AnyLoader::Array(loader)) => {
            loader.set_template(text(0), text(1));
            Ok(Value::Null)
        }
        ("addLoader", AnyLoader::Chain(loader)) => {
            loader.add_loader(loaders[step["loader"].as_str().unwrap()].as_loader());
            Ok(Value::Null)
        }
        (method, _) => panic!("unsupported call {method}"),
    }
}

#[test]
fn filesystem_and_chain_loaders_match_twig() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/vectors/loader/scenarios.json"
    );
    let vectors: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let unix_only = |value: &Value| value["unix_only"].as_bool() == Some(true);
    let mut failures = Vec::new();
    let mut checked = 0;
    for (index, scenario) in vectors["scenarios"].as_array().unwrap().iter().enumerate() {
        if unix_only(scenario) && !cfg!(unix) {
            continue;
        }
        let tree = Tree::create(index, &vectors["tree"]);
        let mut loaders = HashMap::new();
        for step in scenario["steps"].as_array().unwrap() {
            if unix_only(step) && !cfg!(unix) {
                continue;
            }
            if step.get("fs").is_some() {
                tree.fs_step(step);
                continue;
            }
            let expected = platform_normalize(expected_outcome(&step["expect"], &tree));
            let actual =
                platform_normalize(actual_outcome(run_step(&tree, &mut loaders, step), &tree));
            checked += 1;
            if actual != expected {
                let mut step = step.clone();
                step.as_object_mut().unwrap().remove("expect");
                failures.push(format!(
                    "[{}] {step}\n    expected {expected:?}\n    actual   {actual:?}",
                    scenario["name"].as_str().unwrap()
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} mismatches:\n{}",
        failures.len(),
        failures.join("\n")
    );
    let minimum = if cfg!(unix) { 450 } else { 400 };
    assert!(checked >= minimum, "only {checked} steps were checked");
}
