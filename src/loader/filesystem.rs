//! Templates from the filesystem.
//!
//! Twig: `Twig\Loader\FilesystemLoader` (TWIG/src/Loader/FilesystemLoader.php), with the PHP functions it calls
//! (`realpath()`, `is_dir()`, `is_file()`, `file_get_contents()`, `filemtime()`, `parse_url()`) ported as far as
//! the loader can observe them.
//!
//! # Paths, namespaces and names
//!
//! Directories are registered per namespace ([`FilesystemLoader::MAIN_NAMESPACE`] by default). A template name is
//! either `path/in/a/directory.twig` (main namespace) or `@namespace/path.twig`. Before a lookup the name is
//! normalised: backslashes become slashes, and when the name starts with `/` or `.`, contains `//` or `/.`, or
//! starts with `@__main__/`, empty and `.` segments are dropped and `..` segments cancel the previous segment
//! (`sub/../index.twig` is `index.twig`, `@__main__/index.twig` is `index.twig`). A normalised main-namespace name
//! that starts with `@` keeps the `@__main__/` prefix so it is not mistaken for a namespaced name. The normalised
//! name is the [`Source::name`] of a loaded template; error messages quote the name as it was passed.
//!
//! A name that still climbs out of its directory (`../x.twig`, `@ns/../x.twig`) or contains a NUL byte is
//! rejected before any filesystem access. Directories are searched in order; the first regular file wins and its
//! [`Source::path`] is its `realpath()` (symbolic links resolved).
//!
//! Relative directories are resolved against the root path: the current directory at construction time, or the
//! `realpath()` of the root path given to [`FilesystemLoader::with_root_path`] (kept as given when it does not
//! exist). Cache keys are template paths relative to the root path when they lie below it.
//!
//! # Caching
//!
//! Like Twig, the loader remembers the path of every template it found and the error of every name it could not
//! load, until a directory is added ([`add_path`](FilesystemLoader::add_path),
//! [`prepend_path`](FilesystemLoader::prepend_path), [`set_paths`](FilesystemLoader::set_paths) with at least one
//! path). A file created after a failed lookup of the same name is therefore not seen, and a template whose file
//! was deleted still [`exists`](Loader::exists). Failed lookups are remembered under the name as passed, found
//! templates under the normalised name.
//!
//! # PHP warnings
//!
//! Where PHP emits a warning or notice and carries on, the loader returns an
//! [`ErrorKind::Runtime`](crate::ErrorKind::Runtime) error with PHP's message, as PHP behaves when warnings are
//! turned into exceptions (Twig's own test suite does that): reading a template whose file disappeared
//! (`file_get_contents(…): Failed to open stream: …`), reading a directory (`file_get_contents(): Read of N bytes
//! failed with errno=21 Is a directory`) and [`is_fresh`](Loader::is_fresh) on a vanished file
//! (`filemtime(): stat failed for …`).
//!
//! # Platforms
//!
//! The PHP oracle runs on Linux, and every behaviour above is verified there. Paths are PHP byte strings: on Unix
//! the bytes go to the operating system unchanged (non-UTF-8 directory names from the current directory or from
//! symbolic links work; [`Source::path`] shows them lossily). On Windows the loader does what PHP on Windows does:
//! `DIRECTORY_SEPARATOR` is `\` (so the root path ends with `\`), a template path is built as
//! `<directory>/<name>` with a forward slash, `realpath()` results use backslashes and no `\\?\` prefix, and the
//! operating system's path rules apply (case-insensitive names, `\` as a separator, `C:\`, drive-relative paths,
//! stripped trailing dots and spaces). Twig's own absolute-path test is platform independent: a path starting with
//! `/` or `\`, a drive (`C:/x`, `C:\x`, longer than three bytes) or anything `parse_url()` reads as having a scheme
//! is absolute everywhere. Paths that are not valid UTF-8 never exist on Windows.
//!
//! # Stream wrappers
//!
//! Only PHP's plain-files wrapper is implemented: `file:///path` and `file://localhost/path` URLs work as in PHP
//! (the URL itself becomes the template path, since `realpath()` does not resolve URLs). Every other wrapper
//! (`phar://`, `data:`, `glob://`, `php://`, `http://`, …) is unsupported and such paths never exist.

use std::collections::HashMap;
use std::ffi::OsString;
use std::fmt;
use std::path::PathBuf;
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

use super::Loader;
use crate::{Error, Result, Source};

/// PHP's `DIRECTORY_SEPARATOR`.
const DIRECTORY_SEPARATOR: u8 = std::path::MAIN_SEPARATOR as u8;

/// Loads templates from directories on the filesystem, grouped in namespaces.
///
/// Twig: `Twig\Loader\FilesystemLoader` (TWIG/src/Loader/FilesystemLoader.php). See the [module
/// documentation](self) for name normalisation, caching, PHP warnings and platform behaviour.
///
/// Directories can be added after the loader has been handed to an environment (all methods take `&self`), like
/// calling `addPath()` on Twig's loader at any time.
///
/// ```
/// use twig::loader::{FilesystemLoader, Loader};
///
/// let loader = FilesystemLoader::new(["tests/loader_fixtures/Loader/Fixtures/normal"])?;
/// loader.add_path("tests/loader_fixtures/Loader/Fixtures/named", "named")?;
/// assert_eq!(loader.get_source_context("index.html")?.code(), b"path\n");
/// assert_eq!(loader.get_source_context("@named/./index.html")?.name(), "@named/index.html");
/// assert_eq!(
///     loader.get_source_context("../index.html").unwrap_err().to_string(),
///     "Looks like you try to load a template outside configured directories (../index.html).",
/// );
/// # Ok::<(), twig::Error>(())
/// ```
pub struct FilesystemLoader {
    /// Twig's `$rootPath`: a PHP byte string ending with `DIRECTORY_SEPARATOR`.
    root_path: Vec<u8>,
    state: RwLock<State>,
}

#[derive(Debug, Default)]
struct State {
    /// `$paths`: namespace => registered paths (trailing slashes removed), in insertion order like a PHP array.
    paths: Vec<(String, Vec<String>)>,
    /// `$cache`: normalised name => resolved template path.
    cache: HashMap<String, Vec<u8>>,
    /// `$errorCache`: name as passed to `findTemplate()` => error message.
    error_cache: HashMap<String, String>,
}

impl State {
    /// `$this->paths[$namespace] ?? null`.
    fn paths(&self, namespace: &str) -> Option<&Vec<String>> {
        self.paths
            .iter()
            .find(|(name, _)| name == namespace)
            .map(|(_, paths)| paths)
    }

    /// `$this->paths[$namespace]` written to: a missing namespace is appended with no paths.
    fn paths_mut(&mut self, namespace: &str) -> &mut Vec<String> {
        let index = match self.paths.iter().position(|(name, _)| name == namespace) {
            Some(index) => index,
            None => {
                self.paths.push((namespace.to_owned(), Vec::new()));
                self.paths.len() - 1
            }
        };
        &mut self.paths[index].1
    }

    /// `$this->cache = $this->errorCache = [];`
    fn clear_caches(&mut self) {
        self.cache.clear();
        self.error_cache.clear();
    }
}

impl FilesystemLoader {
    /// Identifier of the main namespace. Twig: `FilesystemLoader::MAIN_NAMESPACE`.
    pub const MAIN_NAMESPACE: &'static str = "__main__";

    /// A loader for the given directories (main namespace), with the current directory as root path for relative
    /// directories.
    ///
    /// Twig: `new FilesystemLoader($paths)`. Fails like [`add_path`](Self::add_path) when a directory does not
    /// exist. Twig's string form `new FilesystemLoader('templates')` is `new(["templates"])`; note that Twig
    /// registers nothing for the falsy strings `''` and `'0'`, which this list form cannot express.
    pub fn new<I>(paths: I) -> Result<Self>
    where
        I: IntoIterator,
        I::Item: AsRef<str>,
    {
        Self::with_root(collect_paths(paths), Self::current_root())
    }

    /// A loader for the given directories (main namespace), resolving relative directories against `root_path`.
    ///
    /// Twig: `new FilesystemLoader($paths, $rootPath)`. The root path is `realpath()`-ed when it exists and used as
    /// given otherwise, and `""` means the current directory (`realpath('')`). A root path containing a NUL byte
    /// fails with PHP's `ValueError` message `realpath(): Argument #1 ($path) must not contain any null bytes`, as an
    /// [`ErrorKind::Logic`](crate::ErrorKind::Logic) error.
    pub fn with_root_path<I>(paths: I, root_path: &str) -> Result<Self>
    where
        I: IntoIterator,
        I::Item: AsRef<str>,
    {
        let paths = collect_paths(paths);
        // $this->rootPath = ($rootPath ?? getcwd()).DIRECTORY_SEPARATOR;
        let mut root = root_path.as_bytes().to_vec();
        root.push(DIRECTORY_SEPARATOR);
        // if (null !== $rootPath && false !== ($realPath = realpath($rootPath)))
        if let Some(mut real) = php::realpath(root_path.as_bytes())? {
            real.push(DIRECTORY_SEPARATOR);
            root = real;
        }
        Self::with_root(paths, root)
    }

    /// `getcwd().DIRECTORY_SEPARATOR`, where a failing `getcwd()` (`false`) contributes nothing.
    fn current_root() -> Vec<u8> {
        let mut root = php::getcwd().unwrap_or_default();
        root.push(DIRECTORY_SEPARATOR);
        root
    }

    fn with_root(paths: Vec<String>, root_path: Vec<u8>) -> Result<Self> {
        let loader = FilesystemLoader {
            root_path,
            state: RwLock::default(),
        };
        // if ($paths) { $this->setPaths($paths); }
        if !paths.is_empty() {
            loader.set_paths(paths, Self::MAIN_NAMESPACE)?;
        }
        Ok(loader)
    }

    /// The registered directories of a namespace (empty for an unknown namespace).
    ///
    /// Twig: `FilesystemLoader::getPaths(string $namespace = self::MAIN_NAMESPACE)`. Directories are listed as they
    /// were registered, without trailing slashes or backslashes.
    pub fn get_paths(&self, namespace: &str) -> Vec<String> {
        self.read().paths(namespace).cloned().unwrap_or_default()
    }

    /// The namespaces that have registered directories (or were set to an empty list), in registration order.
    ///
    /// Twig: `FilesystemLoader::getNamespaces()`. A loader created without directories has no namespace at all.
    /// PHP returns numeric namespace names (`"123"`) as integers; here they stay strings.
    pub fn get_namespaces(&self) -> Vec<String> {
        self.read()
            .paths
            .iter()
            .map(|(name, _)| name.clone())
            .collect()
    }

    /// Replaces the directories of a namespace, checking that each one exists.
    ///
    /// Twig: `FilesystemLoader::setPaths($paths, string $namespace = self::MAIN_NAMESPACE)`. The namespace is first
    /// emptied, then each directory is added with [`add_path`](Self::add_path); when one does not exist the error is
    /// returned and the directories before it stay registered. An empty list keeps the lookup caches.
    pub fn set_paths<I>(&self, paths: I, namespace: &str) -> Result<()>
    where
        I: IntoIterator,
        I::Item: AsRef<str>,
    {
        let paths = collect_paths(paths);
        let mut state = self.write();
        // $this->paths[$namespace] = [];
        state.paths_mut(namespace).clear();
        for path in &paths {
            self.add_path_to(&mut state, path, namespace, true)?;
        }
        Ok(())
    }

    /// Appends a directory to a namespace after checking that it exists.
    ///
    /// Twig: `FilesystemLoader::addPath(string $path, string $namespace = self::MAIN_NAMESPACE)`. The lookup caches
    /// are cleared first, even when the check fails. A relative directory is checked below the root path; the error
    /// is `The "<path>" directory does not exist ("<checked path>").` Trailing slashes and backslashes are removed
    /// before the directory is stored (so `/` is stored as `""`, which then means the root path).
    pub fn add_path(&self, path: &str, namespace: &str) -> Result<()> {
        self.add_path_to(&mut self.write(), path, namespace, true)
    }

    /// Appends a directory to a namespace without checking that it exists.
    ///
    /// Twig: `FilesystemLoader::addPath($path, $namespace, false)` (the undeclared third argument `$check`).
    pub fn add_path_unchecked(&self, path: &str, namespace: &str) {
        // Without the check nothing can fail.
        let _ = self.add_path_to(&mut self.write(), path, namespace, false);
    }

    /// Inserts a directory before the other directories of a namespace after checking that it exists.
    ///
    /// Twig: `FilesystemLoader::prependPath(string $path, string $namespace = self::MAIN_NAMESPACE)`; see
    /// [`add_path`](Self::add_path) for the check and the stored form.
    pub fn prepend_path(&self, path: &str, namespace: &str) -> Result<()> {
        self.prepend_path_to(&mut self.write(), path, namespace, true)
    }

    /// Inserts a directory before the other directories of a namespace without checking that it exists.
    ///
    /// Twig: `FilesystemLoader::prependPath($path, $namespace, false)`.
    pub fn prepend_path_unchecked(&self, path: &str, namespace: &str) {
        // Without the check nothing can fail.
        let _ = self.prepend_path_to(&mut self.write(), path, namespace, false);
    }

    /// Twig: `FilesystemLoader::addPath()`.
    fn add_path_to(
        &self,
        state: &mut State,
        path: &str,
        namespace: &str,
        check: bool,
    ) -> Result<()> {
        // invalidate the cache
        state.clear_caches();
        if check {
            self.check_directory(path)?;
        }
        state
            .paths_mut(namespace)
            .push(rtrim_slashes(path).to_owned());
        Ok(())
    }

    /// Twig: `FilesystemLoader::prependPath()`.
    fn prepend_path_to(
        &self,
        state: &mut State,
        path: &str,
        namespace: &str,
        check: bool,
    ) -> Result<()> {
        // invalidate the cache
        state.clear_caches();
        if check {
            self.check_directory(path)?;
        }
        let path = rtrim_slashes(path).to_owned();
        // if (!isset($this->paths[$namespace])) { $this->paths[$namespace][] = $path; } else { array_unshift(...); }
        state.paths_mut(namespace).insert(0, path);
        Ok(())
    }

    /// The directory check of `addPath()`/`prependPath()`.
    fn check_directory(&self, path: &str) -> Result<()> {
        let check_path = if is_absolute_path(path.as_bytes()) {
            path.as_bytes().to_vec()
        } else {
            [self.root_path.as_slice(), path.as_bytes()].concat()
        };
        if !php::is_dir(&check_path) {
            return Err(Error::loader(format!(
                "The \"{path}\" directory does not exist (\"{}\").",
                String::from_utf8_lossy(&check_path)
            )));
        }
        Ok(())
    }

    /// The resolved path of a template, or the loader error for it.
    ///
    /// Twig: `FilesystemLoader::findTemplate(string $name, bool $throw = true)`. Twig's `$throw = false` variant
    /// returns `null` exactly when this returns a loader error; both update the caches the same way.
    fn find_template(&self, name: &str) -> Result<Vec<u8>> {
        let original_name = name;
        let name = normalize_name(name);
        {
            let state = self.read();
            if let Some(found) = lookup_caches(&state, &name, original_name) {
                return found;
            }
        }
        let mut state = self.write();
        // Another thread may have filled the caches in between; PHP runs this method as one step.
        if let Some(found) = lookup_caches(&state, &name, original_name) {
            return found;
        }

        let (namespace, shortname) = parse_name(&name)?;
        validate_name(shortname, original_name)?;

        let Some(paths) = state.paths(namespace).cloned() else {
            let message = format!("There are no registered paths for namespace \"{namespace}\".");
            state
                .error_cache
                .insert(original_name.to_owned(), message.clone());
            return Err(Error::loader(message));
        };
        for path in &paths {
            let mut candidate = if is_absolute_path(path.as_bytes()) {
                path.as_bytes().to_vec()
            } else {
                [self.root_path.as_slice(), path.as_bytes()].concat()
            };
            candidate.push(b'/');
            candidate.extend_from_slice(shortname.as_bytes());
            if php::is_file(&candidate) {
                let resolved = php::realpath(&candidate)?.unwrap_or(candidate);
                state.cache.insert(name.clone(), resolved.clone());
                return Ok(resolved);
            }
        }
        let message = format!(
            "Unable to find template \"{original_name}\" (looked into: {}).",
            paths.join(", ")
        );
        state
            .error_cache
            .insert(original_name.to_owned(), message.clone());
        Err(Error::loader(message))
    }

    fn read(&self) -> RwLockReadGuard<'_, State> {
        // A poisoned lock only means another thread panicked; the state is updated in single steps and stays valid.
        self.state.read().unwrap_or_else(|e| e.into_inner())
    }

    fn write(&self) -> RwLockWriteGuard<'_, State> {
        self.state.write().unwrap_or_else(|e| e.into_inner())
    }
}

/// The two cache checks at the start of `findTemplate()`: a found template (by normalised name) wins over a
/// remembered error (by name as passed).
fn lookup_caches(state: &State, name: &str, original_name: &str) -> Option<Result<Vec<u8>>> {
    if let Some(path) = state.cache.get(name) {
        return Some(Ok(path.clone()));
    }
    state
        .error_cache
        .get(original_name)
        .map(|message| Err(Error::loader(message.clone())))
}

impl Default for FilesystemLoader {
    /// A loader without directories (and without namespaces) whose root path is the current directory.
    ///
    /// Twig: `new FilesystemLoader()`.
    fn default() -> Self {
        FilesystemLoader {
            root_path: Self::current_root(),
            state: RwLock::default(),
        }
    }
}

impl fmt::Debug for FilesystemLoader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FilesystemLoader")
            .field("root_path", &String::from_utf8_lossy(&self.root_path))
            .field("paths", &self.read().paths)
            .finish_non_exhaustive()
    }
}

impl Loader for FilesystemLoader {
    /// Twig: `FilesystemLoader::getSourceContext()`: the code of the file, the normalised name and the resolved path.
    fn get_source_context(&self, name: &str) -> Result<Source> {
        let path = self.find_template(name)?;
        let code = php::file_get_contents(&path)?;
        Ok(Source::with_path(
            code,
            normalize_name(name),
            String::from_utf8_lossy(&path),
        ))
    }

    /// Twig: `FilesystemLoader::getCacheKey()`: the resolved path, relative to the root path when it starts with it.
    fn get_cache_key(&self, name: &str) -> Result<Vec<u8>> {
        let path = self.find_template(name)?;
        Ok(match path.strip_prefix(self.root_path.as_slice()) {
            Some(relative) => relative.to_vec(),
            None => path,
        })
    }

    /// Twig: `FilesystemLoader::isFresh()`: whether the file's modification time is before `time`.
    fn is_fresh(&self, name: &str, time: i64) -> Result<bool> {
        let path = self.find_template(name)?;
        Ok(php::filemtime(&path)? < time)
    }

    /// Twig: `FilesystemLoader::exists()`. Calls `findTemplate()` with the normalised name, so a failed check is
    /// remembered under the normalised name.
    fn exists(&self, name: &str) -> bool {
        let name = normalize_name(name);
        if self.read().cache.contains_key(&name) {
            return true;
        }
        self.find_template(&name).is_ok()
    }

    fn class_name(&self) -> &str {
        "Twig\\Loader\\FilesystemLoader"
    }
}

fn collect_paths<I>(paths: I) -> Vec<String>
where
    I: IntoIterator,
    I::Item: AsRef<str>,
{
    paths
        .into_iter()
        .map(|path| path.as_ref().to_owned())
        .collect()
}

/// PHP `rtrim($path, '/\\')`.
fn rtrim_slashes(path: &str) -> &str {
    path.trim_end_matches(['/', '\\'])
}

/// Twig: `FilesystemLoader::normalizeName()`.
fn normalize_name(name: &str) -> String {
    let name = name.replace('\\', "/");
    // if (!preg_match('#(?:^|/)[/.]|^@__main__/#', $name)) return $name;
    let bytes = name.as_bytes();
    let needs_normalizing = matches!(bytes.first(), Some(b'/' | b'.'))
        || bytes
            .windows(2)
            .any(|pair| pair[0] == b'/' && matches!(pair[1], b'/' | b'.'))
        || name.starts_with("@__main__/");
    if !needs_normalizing {
        return name;
    }

    let (prefix, rest) = match name.find('/') {
        Some(slash) if bytes[0] == b'@' => name.split_at(slash + 1),
        _ => ("", name.as_str()),
    };
    let mut parts: Vec<&str> = Vec::new();
    for part in rest.split('/') {
        if part == ".." && parts.last().is_some_and(|last| *last != "..") {
            parts.pop();
        } else if !part.is_empty() && part != "." {
            parts.push(part);
        }
    }
    let joined = parts.join("/");

    if prefix.is_empty() || prefix == "@__main__/" {
        // without the main namespace, a name starting with "@" would be read as a namespaced name
        return if joined.starts_with('@') {
            format!("@__main__/{joined}")
        } else {
            joined
        };
    }
    format!("{prefix}{joined}")
}

/// Twig: `FilesystemLoader::parseName()`: `(namespace, short name)`.
fn parse_name(name: &str) -> Result<(&str, &str)> {
    if name.starts_with('@') {
        let Some(slash) = name.find('/') else {
            return Err(Error::loader(format!(
                "Malformed namespaced template name \"{name}\" (expecting \"@namespace/template_name\")."
            )));
        };
        return Ok((&name[1..slash], &name[slash + 1..]));
    }
    Ok((FilesystemLoader::MAIN_NAMESPACE, name))
}

/// Twig: `FilesystemLoader::validateName()`.
fn validate_name(name: &str, original_name: &str) -> Result<()> {
    if name.contains('\0') {
        return Err(Error::loader("A template name cannot contain NUL bytes."));
    }
    let mut level = 0i64;
    for part in name.trim_start_matches('/').split('/') {
        if part == ".." {
            level -= 1;
        } else if part != "." {
            level += 1;
        }
        if level < 0 {
            return Err(Error::loader(format!(
                "Looks like you try to load a template outside configured directories ({original_name})."
            )));
        }
    }
    Ok(())
}

/// Twig: `FilesystemLoader::isAbsolutePath()`. The same on every platform.
fn is_absolute_path(file: &[u8]) -> bool {
    matches!(file.first(), Some(b'/' | b'\\'))
        || (file.len() > 3
            && file[0].is_ascii_alphabetic()
            && file[1] == b':'
            && matches!(file[2], b'/' | b'\\'))
        || url_has_scheme_or_is_malformed(file)
}

/// Whether `parse_url($url, PHP_URL_SCHEME)` is not `null`: the URL has a scheme, or `parse_url()` rejects it and
/// returns `false`.
///
/// Port of the control flow of `php_url_parse_ex2()` (PHPSRC/ext/standard/url.c) that decides between those
/// outcomes. Character classes are the C locale's (`C.UTF-8` in PHP 8.5 classifies only ASCII bytes).
fn url_has_scheme_or_is_malformed(url: &[u8]) -> bool {
    let ue = url.len();
    match url.iter().position(|&c| c == b':') {
        Some(e) if e != 0 => {
            // validate scheme: 1*[ alpha | digit | "+" | "-" | "." ]
            if url[..e]
                .iter()
                .any(|&c| !(c.is_ascii_alphanumeric() || matches!(c, b'+' | b'.' | b'-')))
            {
                if e + 1 < ue && e < first_of(url, 0, b"?#") {
                    return url_parse_port(url, 0, e);
                }
                if starts_with_two_slashes(url, 0) {
                    // relative-scheme URL
                    return url_parse_host(url, 2, 0);
                }
                return false;
            }
            if e + 1 != ue && url[e + 1] != b'/' {
                // check if the data we get is a port this allows us to correctly parse things like a.com:80
                let digits_end = e
                    + 1
                    + url[e + 1..]
                        .iter()
                        .take_while(|c| c.is_ascii_digit())
                        .count();
                if (digits_end == ue || url[digits_end] == b'/') && digits_end - e < 7 {
                    return url_parse_port(url, 0, e);
                }
            }
            // The scheme is set; a later failure only turns the result into `false`, which is not `null` either.
            true
        }
        // no scheme; starts with colon: look for port
        Some(e) => url_parse_port(url, 0, e),
        // relative-scheme URL
        None => starts_with_two_slashes(url, 0) && url_parse_host(url, 2, 0),
    }
}

/// The `parse_port:` part of `php_url_parse_ex2()`: `colon` is the position of the colon, `start` the current
/// start of the URL. Returns whether `parse_url()` fails (no scheme is set on this path).
fn url_parse_port(url: &[u8], start: usize, colon: usize) -> bool {
    let ue = url.len();
    let p = colon + 1;
    let mut pp = p;
    while pp < ue && pp - p < 6 && url[pp].is_ascii_digit() {
        pp += 1;
    }
    if pp - p > 0 && pp - p < 6 && (pp == ue || url[pp] == b'/') {
        return match c_strtol(&url[p..pp]) {
            Some(port) if (0..=65535).contains(&port) => {
                let start = if starts_with_two_slashes(url, start) {
                    start + 2
                } else {
                    start
                };
                url_parse_host(url, start, port)
            }
            _ => true,
        };
    }
    if p == pp && pp == ue {
        return true;
    }
    if starts_with_two_slashes(url, start) {
        return url_parse_host(url, start + 2, 0);
    }
    false
}

/// The `parse_host:` part of `php_url_parse_ex2()`; `port` is the port found so far (`0` counts as none, like
/// `ret->port`). Returns whether `parse_url()` fails.
fn url_parse_host(url: &[u8], start: usize, port: i64) -> bool {
    let ue = url.len();
    let e = first_of(url, start, b"/?#");
    let mut s = start;
    // check for login and password
    if let Some(at) = url[s..e].iter().rposition(|&c| c == b'@') {
        s += at + 1;
    }
    // check for port; an IPv6 address in brackets has none
    let colon = if s < ue && url[s] == b'[' && url[e - 1] == b']' {
        None
    } else {
        url[s..e].iter().rposition(|&c| c == b':').map(|i| s + i)
    };
    let host_end = match colon {
        Some(colon) => {
            if port == 0 {
                let digits = &url[colon + 1..e];
                // port cannot be longer then 5 characters
                if digits.len() > 5 {
                    return true;
                }
                if !digits.is_empty()
                    && !matches!(c_strtol(digits), Some(p) if (0..=65535).contains(&p))
                {
                    return true;
                }
            }
            colon
        }
        None => e,
    };
    // check if we have a valid host, if we don't reject the string as url
    host_end == s
}

/// `binary_strcspn()`: the position of the first byte of `url[from..]` that is in `chars`, or the length.
fn first_of(url: &[u8], from: usize, chars: &[u8]) -> usize {
    url[from..]
        .iter()
        .position(|c| chars.contains(c))
        .map_or(url.len(), |i| from + i)
}

fn starts_with_two_slashes(url: &[u8], from: usize) -> bool {
    url.len() > from + 1 && url[from] == b'/' && url[from + 1] == b'/'
}

/// C `strtol($s, &end, 10)` on at most five bytes, as `php_url_parse_ex2()` uses it on ports: `None` when no digit
/// was converted (`end == port_buf`).
fn c_strtol(s: &[u8]) -> Option<i64> {
    let mut i = s
        .iter()
        .take_while(|&&c| matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r'))
        .count();
    let negative = match s.get(i) {
        Some(b'-') => {
            i += 1;
            true
        }
        Some(b'+') => {
            i += 1;
            false
        }
        _ => false,
    };
    let digits = s[i..].iter().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 {
        return None;
    }
    let value = s[i..i + digits]
        .iter()
        .fold(0i64, |value, &digit| value * 10 + i64::from(digit - b'0'));
    Some(if negative { -value } else { value })
}

/// The PHP filesystem functions `FilesystemLoader` calls, on PHP byte-string paths.
mod php {
    use std::fs;
    use std::io::{self, Read as _};

    use super::{os_path, os_to_bytes, plain_files_path, strip_verbatim_prefix};
    use crate::{Error, Result};

    /// PHP `getcwd()`; `None` is PHP's `false`.
    pub(super) fn getcwd() -> Option<Vec<u8>> {
        std::env::current_dir()
            .ok()
            .map(|cwd| os_to_bytes(cwd.into_os_string()))
    }

    /// PHP `realpath()`; `Ok(None)` is PHP's `false`.
    ///
    /// Port of `PHP_FUNCTION(realpath)` (PHPSRC/ext/standard/file.c) and `tsrm_realpath()`
    /// (PHPSRC/Zend/zend_virtual_cwd.c), whose symlink and `..` resolution equals the operating system's
    /// `realpath()`; PHP's realpath cache is not modelled.
    pub(super) fn realpath(path: &[u8]) -> Result<Option<Vec<u8>>> {
        if path.contains(&0) {
            return Err(Error::logic(
                "realpath(): Argument #1 ($path) must not contain any null bytes",
            ));
        }
        // realpath("") returns CWD
        let cwd;
        let path = if path.is_empty() {
            match getcwd() {
                Some(dir) => {
                    cwd = dir;
                    cwd.as_slice()
                }
                None => return Ok(None),
            }
        } else {
            path
        };
        let Some(path) = os_path(path) else {
            return Ok(None);
        };
        Ok(fs::canonicalize(path)
            .ok()
            .map(|real| strip_verbatim_prefix(os_to_bytes(real.into_os_string()), cfg!(windows))))
    }

    /// PHP `is_dir()`.
    pub(super) fn is_dir(path: &[u8]) -> bool {
        stat(path).is_some_and(|metadata| metadata.is_dir())
    }

    /// PHP `is_file()`.
    pub(super) fn is_file(path: &[u8]) -> bool {
        stat(path).is_some_and(|metadata| metadata.is_file())
    }

    /// `php_stat()` (PHPSRC/ext/standard/filestat.c) without PHP's stat cache: symbolic links are followed; an
    /// empty name or one containing a NUL byte has no status.
    fn stat(path: &[u8]) -> Option<fs::Metadata> {
        if path.is_empty() || path.contains(&0) {
            return None;
        }
        let local = plain_files_path(path, cfg!(windows))?;
        fs::metadata(os_path(local)?).ok()
    }

    /// PHP `file_get_contents()`. PHP's warning (unreadable file) or notice (failed read) becomes a runtime error.
    pub(super) fn file_get_contents(path: &[u8]) -> Result<Vec<u8>> {
        let open_error = |reason: &str| {
            Error::runtime(format!(
                "file_get_contents({}): Failed to open stream: {reason}",
                String::from_utf8_lossy(path)
            ))
        };
        let Some(local) = plain_files_path(path, cfg!(windows)) else {
            return Err(open_error("no suitable wrapper could be found"));
        };
        let Some(local) = os_path(local) else {
            return Err(open_error("No such file or directory"));
        };
        let mut file = fs::File::open(local).map_err(|error| open_error(&strerror(&error)))?;
        // _php_stream_copy_to_mem() asks for the file size plus 8 KiB in its first read.
        let first_read = match file.metadata() {
            Ok(metadata) if metadata.len() > 0 => metadata.len().saturating_add(8192),
            _ => 8192,
        };
        let mut code = Vec::new();
        file.read_to_end(&mut code).map_err(|error| {
            Error::runtime(format!(
                "file_get_contents(): Read of {first_read} bytes failed with errno={} {}",
                error.raw_os_error().unwrap_or(0),
                strerror(&error)
            ))
        })?;
        Ok(code)
    }

    /// PHP `filemtime()`. PHP's warning for a missing file becomes a runtime error.
    pub(super) fn filemtime(path: &[u8]) -> Result<i64> {
        match stat(path) {
            Some(metadata) => Ok(mtime(&metadata)),
            None => Err(Error::runtime(format!(
                "filemtime(): stat failed for {}",
                String::from_utf8_lossy(path)
            ))),
        }
    }

    #[cfg(unix)]
    fn mtime(metadata: &fs::Metadata) -> i64 {
        std::os::unix::fs::MetadataExt::mtime(metadata)
    }

    #[cfg(not(unix))]
    fn mtime(metadata: &fs::Metadata) -> i64 {
        use std::time::UNIX_EPOCH;
        let Ok(modified) = metadata.modified() else {
            return 0;
        };
        match modified.duration_since(UNIX_EPOCH) {
            Ok(after) => i64::try_from(after.as_secs()).unwrap_or(i64::MAX),
            // st_mtime rounds towards negative infinity
            Err(before) => {
                let before = before.duration();
                -i64::try_from(before.as_secs()).unwrap_or(i64::MAX)
                    - i64::from(before.subsec_nanos() > 0)
            }
        }
    }

    /// The C library's `strerror()` text of an I/O error, as PHP prints it.
    fn strerror(error: &io::Error) -> String {
        if !cfg!(unix) {
            // PHP on Windows maps system errors to C errno values first.
            match error.kind() {
                io::ErrorKind::NotFound => return "No such file or directory".to_owned(),
                io::ErrorKind::PermissionDenied => return "Permission denied".to_owned(),
                _ => {}
            }
        }
        // Rust prints operating system errors as "<strerror text> (os error N)".
        let text = error.to_string();
        match text.rfind(" (os error ") {
            Some(at) if error.raw_os_error().is_some() => text[..at].to_owned(),
            _ => text,
        }
    }
}

/// The local path PHP's plain-files stream wrapper opens for `path`: `path` itself, or the path inside a `file://`
/// URL; `None` when PHP would use another stream wrapper (unsupported here) or refuses the URL (a remote
/// `file://host/` URL).
///
/// Port of `php_stream_locate_url_wrapper()` (PHPSRC/main/streams/streams.c) for the plain-files wrapper;
/// `windows` selects PHP's Windows variant (`file://C:/x`, `file:///C:/x`).
fn plain_files_path(path: &[u8], windows: bool) -> Option<&[u8]> {
    let n = path
        .iter()
        .take_while(|&&c| c.is_ascii_alphanumeric() || matches!(c, b'+' | b'-' | b'.'))
        .count();
    let is_url = path.get(n) == Some(&b':')
        && n > 1
        && (path[n + 1..].starts_with(b"//") || (n == 4 && path.starts_with(b"data:")));
    if !is_url {
        return Some(path);
    }
    if !path[..n].eq_ignore_ascii_case(b"file") {
        return None;
    }
    let at = |i: usize| path.get(i).copied().unwrap_or(0);
    let localhost = path.len() >= 17 && path[..17].eq_ignore_ascii_case(b"file://localhost/");
    if !localhost && at(n + 3) != 0 && at(n + 3) != b'/' && !(windows && at(n + 4) == b':') {
        // Remote host file access not supported
        return None;
    }
    // skip past protocol and :/, but handle windows correctly
    let mut start = n + 1;
    if localhost {
        start += 11;
    }
    loop {
        start += 1;
        if at(start) != b'/' {
            break;
        }
    }
    if !(windows && at(start + 1) == b':') {
        start -= 1;
    }
    Some(&path[start..])
}

/// A PHP byte-string path as an operating-system path: the bytes themselves on Unix, UTF-8 elsewhere (like PHP
/// on Windows; other bytes name no file).
#[cfg(unix)]
fn os_path(path: &[u8]) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    Some(PathBuf::from(std::ffi::OsStr::from_bytes(path)))
}

#[cfg(not(unix))]
fn os_path(path: &[u8]) -> Option<PathBuf> {
    std::str::from_utf8(path).ok().map(PathBuf::from)
}

/// An operating-system path as a PHP byte string.
#[cfg(unix)]
fn os_to_bytes(path: OsString) -> Vec<u8> {
    use std::os::unix::ffi::OsStringExt;
    path.into_vec()
}

#[cfg(not(unix))]
fn os_to_bytes(path: OsString) -> Vec<u8> {
    path.into_string()
        .unwrap_or_else(|path| path.to_string_lossy().into_owned())
        .into_bytes()
}

/// `std::fs::canonicalize()` returns verbatim paths on Windows (`\\?\C:\x`, `\\?\UNC\server\share\x`) where PHP's
/// `realpath()` returns `C:\x` and `\\server\share\x`.
fn strip_verbatim_prefix(path: Vec<u8>, windows: bool) -> Vec<u8> {
    if windows {
        if let Some(share) = path.strip_prefix(br"\\?\UNC\") {
            return [br"\\".as_slice(), share].concat();
        }
        if let Some(drive) = path.strip_prefix(br"\\?\")
            && drive.len() >= 2
            && drive[0].is_ascii_alphabetic()
            && drive[1] == b':'
        {
            return drive.to_vec();
        }
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Oracle vectors: `tests/vectors/loader/names.json` (regenerate with `tests/vectors/loader/names.gen.php`).
    fn vectors() -> serde_json::Value {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/vectors/loader/names.json"
        );
        let text = std::fs::read_to_string(path).expect("names.json");
        serde_json::from_str(&text).expect("valid JSON")
    }

    #[test]
    fn is_absolute_path_matches_twig() {
        let vectors = vectors();
        let cases = vectors["is_absolute_path"].as_array().expect("array");
        assert!(cases.len() > 50_000, "the vector file looks truncated");
        let failures: Vec<String> = cases
            .iter()
            .filter_map(|case| {
                let path = case[0].as_str().expect("path");
                let expected = case[1].as_bool().expect("bool");
                (is_absolute_path(path.as_bytes()) != expected)
                    .then(|| format!("{path:?}: expected {expected}"))
            })
            .collect();
        assert!(
            failures.is_empty(),
            "{} mismatches:\n{}",
            failures.len(),
            failures.join("\n")
        );
    }

    #[test]
    fn names_are_normalized_parsed_and_validated_like_twig() {
        let vectors = vectors();
        let cases = vectors["names"].as_array().expect("array");
        assert!(cases.len() > 10_000, "the vector file looks truncated");
        let mut failures = Vec::new();
        for case in cases {
            let name = case[0].as_str().expect("name");
            let normalized = normalize_name(name);
            if normalized != case[1].as_str().expect("normalized") {
                failures.push(format!(
                    "normalize {name:?}: got {normalized:?}, expected {}",
                    case[1]
                ));
                continue;
            }
            let outcome = parse_name(&normalized).and_then(|(namespace, short)| {
                validate_name(short, name)?;
                Ok((namespace, short))
            });
            let parsed = parse_name(&normalized).ok();
            let expected_parse = case[2]
                .as_str()
                .map(|namespace| (namespace, case[3].as_str().expect("short")));
            if parsed != expected_parse {
                failures.push(format!(
                    "parse {name:?}: got {parsed:?}, expected {expected_parse:?}"
                ));
            }
            let error = outcome.err().map(|error| error.to_string());
            if error.as_deref() != case[4].as_str() {
                failures.push(format!(
                    "errors of {name:?}: got {error:?}, expected {}",
                    case[4]
                ));
            }
        }
        assert!(
            failures.is_empty(),
            "{} mismatches:\n{}",
            failures.len(),
            failures.join("\n")
        );
    }

    #[test]
    fn strtol_reads_ports_like_c() {
        assert_eq!(c_strtol(b"80"), Some(80));
        assert_eq!(c_strtol(b" \t+8x"), Some(8));
        assert_eq!(c_strtol(b"-0"), Some(0));
        assert_eq!(c_strtol(b"-12"), Some(-12));
        assert_eq!(c_strtol(b""), None);
        assert_eq!(c_strtol(b"+"), None);
        assert_eq!(c_strtol(b"\0"), None);
        assert_eq!(c_strtol(b"x1"), None);
    }

    #[test]
    fn locates_the_plain_files_wrapper_like_php() {
        let unix = |path: &str| {
            plain_files_path(path.as_bytes(), false)
                .map(|p| String::from_utf8_lossy(p).into_owned())
        };
        assert_eq!(unix("/tmp/x").as_deref(), Some("/tmp/x"));
        assert_eq!(unix("relative").as_deref(), Some("relative"));
        assert_eq!(unix("a:b").as_deref(), Some("a:b"));
        assert_eq!(unix("C://x").as_deref(), Some("C://x"));
        assert_eq!(unix("file:/x").as_deref(), Some("file:/x"));
        assert_eq!(unix("file:///tmp/x").as_deref(), Some("/tmp/x"));
        assert_eq!(unix("FILE:///tmp/x").as_deref(), Some("/tmp/x"));
        assert_eq!(unix("file://///tmp/x").as_deref(), Some("/tmp/x"));
        assert_eq!(unix("file://").as_deref(), Some("/"));
        assert_eq!(unix("file://localhost/tmp/x").as_deref(), Some("/tmp/x"));
        assert_eq!(unix("file://host/tmp/x"), None);
        assert_eq!(unix("file://C:/x"), None);
        assert_eq!(unix("phar://x.phar/y"), None);
        assert_eq!(unix("data:text/plain,x"), None);
        assert_eq!(unix("data://x"), None);
        assert_eq!(unix("foo://x"), None);

        let windows = |path: &str| {
            plain_files_path(path.as_bytes(), true).map(|p| String::from_utf8_lossy(p).into_owned())
        };
        assert_eq!(windows("file:///C:/x").as_deref(), Some("C:/x"));
        assert_eq!(windows("file://C:/x").as_deref(), Some("C:/x"));
        assert_eq!(windows("file://C:\\x/y").as_deref(), Some("C:\\x/y"));
        assert_eq!(windows("file:///tmp/x").as_deref(), Some("/tmp/x"));
        assert_eq!(windows("file://").as_deref(), Some("/"));
        assert_eq!(windows("file://host/x"), None);
    }

    #[test]
    fn strips_verbatim_prefixes_on_windows_only() {
        let strip = |path: &[u8], windows| strip_verbatim_prefix(path.to_vec(), windows);
        assert_eq!(strip(br"\\?\C:\x\y", true), br"C:\x\y");
        assert_eq!(
            strip(br"\\?\UNC\server\share\x", true),
            br"\\server\share\x"
        );
        assert_eq!(strip(br"\\?\Volume{1}\x", true), br"\\?\Volume{1}\x");
        assert_eq!(strip(br"C:\x", true), br"C:\x");
        assert_eq!(strip(br"\\?\C:\x", false), br"\\?\C:\x");
    }

    #[test]
    fn realpath_of_the_empty_path_is_the_current_directory() {
        let cwd = php::getcwd().expect("current directory");
        assert_eq!(php::realpath(b"").unwrap(), php::realpath(&cwd).unwrap());
        assert!(php::realpath(b"").unwrap().is_some());
        assert_eq!(php::realpath(b"/definitely/not/here/twig").unwrap(), None);
        let error = php::realpath(b"a\0b").unwrap_err();
        assert_eq!(error.kind(), crate::ErrorKind::Logic);
        assert_eq!(
            error.to_string(),
            "realpath(): Argument #1 ($path) must not contain any null bytes"
        );
    }

    #[test]
    fn rtrim_removes_both_slash_kinds() {
        assert_eq!(rtrim_slashes("a/b//\\/"), "a/b");
        assert_eq!(rtrim_slashes("/"), "");
        assert_eq!(rtrim_slashes("\\\\"), "");
        assert_eq!(rtrim_slashes("a"), "a");
    }

    #[test]
    fn class_name_is_twigs() {
        assert_eq!(
            FilesystemLoader::default().class_name(),
            "Twig\\Loader\\FilesystemLoader"
        );
    }
}
