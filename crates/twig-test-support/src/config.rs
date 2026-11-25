//! `--CONFIG--` → Environment options.
//!
//! The harness passes `array_merge(['cache' => false, 'strict_variables' => true], CONFIG)` to
//! `new Environment($loader, $options)` (`IntegrationTestCase.php:251-254`); the corpus records that
//! merged array as a typed tree. [`EnvironmentOptions::from_options`] ports what
//! `Twig\Environment::__construct()` (TWIG/src/Environment.php:123-152) and
//! `EscaperExtension::setDefaultStrategy()` (TWIG/src/Extension/EscaperExtension.php:105-113) do
//! with such an array, so the engine gets plain Rust values.

use twig_value::param::{UserArg, verify_user_arg};
use twig_value::{
    Array, ArrayKey, Diagnostics, PhpError, PhpResult, PhpString, ThrowableClass, Value, convert,
};

use crate::php;

/// The default escaping strategy (`autoescape` option).
#[derive(Clone, Debug, PartialEq)]
pub enum Autoescape {
    /// `false`: no auto-escaping.
    Disabled,
    /// A strategy name (`html`, `js`, `css`, `url`, `html_attr`, or a custom one).
    Strategy(PhpString),
    /// `'name'`: Twig replaces it with `[FileExtensionEscapingStrategy::class, 'guess']`, which
    /// picks the strategy from the template name's extension.
    FileExtension,
    /// Any other value: a PHP callback Twig calls with the template name (`call_user_func()`);
    /// kept as the PHP value.
    Callable(Value),
}

/// The `cache` option.
#[derive(Clone, Debug, PartialEq)]
pub enum CacheOption {
    /// `false`: no compilation cache.
    Disabled,
    /// A directory for `FilesystemCache`.
    Directory(PhpString),
    /// A `Twig\Cache\CacheInterface` object.
    Object(Value),
}

/// The options an Environment is created with, as Twig resolves them.
#[derive(Clone, Debug, PartialEq)]
pub struct EnvironmentOptions {
    /// `debug` (`(bool)`).
    pub debug: bool,
    /// `charset` after `setCharset()` (upper case, `UTF8` → `UTF-8`).
    pub charset: PhpString,
    /// `strict_variables` (`(bool)`).
    pub strict_variables: bool,
    /// `autoescape` after `EscaperExtension::setDefaultStrategy()`.
    pub autoescape: Autoescape,
    /// `cache`.
    pub cache: CacheOption,
    /// `auto_reload`: `null` follows `debug`, otherwise `(bool)`.
    pub auto_reload: bool,
    /// `optimizations` (`(int)`; `-1` = all).
    pub optimizations: i64,
    /// `use_yield` (`(bool)`).
    pub use_yield: bool,
    /// Keys Twig ignores, in order.
    pub unknown: Vec<(ArrayKey, Value)>,
}

/// The option names `Environment::__construct()` reads.
const KNOWN: &[&str] = &[
    "debug",
    "charset",
    "strict_variables",
    "autoescape",
    "cache",
    "auto_reload",
    "optimizations",
    "use_yield",
];

/// Twig's defaults (TWIG/src/Environment.php:123-130), in `array_merge()` order.
fn twig_defaults() -> Array {
    let mut a = Array::new();
    a.insert("debug", false);
    a.insert("charset", "UTF-8");
    a.insert("strict_variables", false);
    a.insert("autoescape", "html");
    a.insert("cache", false);
    a.insert("auto_reload", Value::Null);
    a.insert("optimizations", -1);
    a.insert("use_yield", false);
    a
}

/// `array_merge($a, $b)`: string keys of `$b` overwrite, int keys are appended renumbered.
fn array_merge(a: &Array, b: &Array) -> Array {
    let mut out = Array::new();
    for (k, v) in a.iter().chain(b.iter()) {
        match k {
            ArrayKey::Int(_) => {
                let _ = out.push(v.clone());
            }
            ArrayKey::Str(_) => {
                out.insert(k.clone(), v.clone());
            }
        }
    }
    out
}

impl EnvironmentOptions {
    /// The options of a fixture without `--CONFIG--`: Twig's defaults with the harness's
    /// `cache => false, strict_variables => true`.
    /// (`from_options(['cache' => false, 'strict_variables' => true])`, which a unit test checks.)
    pub fn harness_default() -> EnvironmentOptions {
        EnvironmentOptions {
            debug: false,
            charset: PhpString::from("UTF-8"),
            strict_variables: true,
            autoescape: Autoescape::Strategy(PhpString::from("html")),
            cache: CacheOption::Disabled,
            auto_reload: false,
            optimizations: -1,
            use_yield: false,
            unknown: Vec::new(),
        }
    }

    /// Ports the option handling of `Environment::__construct()` for an options array.
    ///
    /// Errors are the PHP exceptions the constructor would throw: `TypeError` from the
    /// `setCharset(string $charset)` parameter, `LogicException: Cache can only be a string, false,
    /// or a \Twig\Cache\CacheInterface implementation.`.
    pub fn from_options(
        options: &Array,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<EnvironmentOptions> {
        let merged = array_merge(&twig_defaults(), options);
        let get = |name: &str| {
            merged
                .get_str(name.as_bytes())
                .cloned()
                .unwrap_or(Value::Null)
        };

        let use_yield = convert::to_bool(&get("use_yield"), diag)?;
        let optimizations = convert::to_long(&get("optimizations"), diag)?;
        let debug = convert::to_bool(&get("debug"), diag)?;
        // $this->setCharset($options['charset'] ?? 'UTF-8');
        let charset = match get("charset") {
            Value::Null => Value::from("UTF-8"),
            v => v,
        };
        let charset = set_charset(charset, diag)?;
        let auto_reload = match get("auto_reload") {
            Value::Null => debug,
            v => convert::to_bool(&v, diag)?,
        };
        let strict_variables = convert::to_bool(&get("strict_variables"), diag)?;
        let cache = set_cache(get("cache"))?;
        let autoescape = match get("autoescape") {
            Value::Bool(false) => Autoescape::Disabled,
            Value::String(s) if s.as_bytes() == b"name" => Autoescape::FileExtension,
            Value::String(s) => Autoescape::Strategy(s),
            other => Autoescape::Callable(other),
        };
        let unknown = options
            .iter()
            .filter(|(k, _)| !matches!(k, ArrayKey::Str(s) if KNOWN.iter().any(|n| s.as_bytes() == n.as_bytes())))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        Ok(EnvironmentOptions {
            debug,
            charset,
            strict_variables,
            autoescape,
            cache,
            auto_reload,
            optimizations,
            use_yield,
            unknown,
        })
    }
}

/// `Environment::setCharset(string $charset)` (TWIG/src/Environment.php:642-650):
/// `strtoupper($charset ?: '')`, `UTF8` → `UTF-8`.
fn set_charset(charset: Value, diag: &mut dyn Diagnostics) -> PhpResult<PhpString> {
    let charset = verify_user_arg(
        charset,
        &php::STRING,
        &UserArg::new("Twig\\Environment::setCharset", 1, "charset"),
        diag,
    )?;
    let s = match &charset {
        Value::String(s) if convert::to_bool(&charset, diag)? => s.clone(),
        _ => PhpString::empty(),
    };
    let upper = PhpString::from(s.to_ascii_uppercase());
    Ok(if upper.as_bytes() == b"UTF8" {
        PhpString::from("UTF-8")
    } else {
        upper
    })
}

/// `Environment::setCache()` (TWIG/src/Environment.php:298-311).
fn set_cache(cache: Value) -> PhpResult<CacheOption> {
    match cache {
        Value::String(s) => Ok(CacheOption::Directory(s)),
        Value::Bool(false) => Ok(CacheOption::Disabled),
        Value::Object(ref o) if o.instance_of("Twig\\Cache\\CacheInterface") => {
            Ok(CacheOption::Object(cache))
        }
        _ => Err(PhpError::new(
            ThrowableClass::LogicException,
            "Cache can only be a string, false, or a \\Twig\\Cache\\CacheInterface implementation.",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn harness_defaults() {
        let mut options = Array::new();
        options.insert("cache", false);
        options.insert("strict_variables", true);
        let mut d = twig_value::Collect::new();
        let o = EnvironmentOptions::from_options(&options, &mut d).unwrap();
        assert!(d.take().is_empty());
        assert_eq!(o, EnvironmentOptions::harness_default());
        assert!(!o.debug);
        assert_eq!(o.charset, "UTF-8");
        assert!(o.strict_variables);
        assert_eq!(o.autoescape, Autoescape::Strategy(PhpString::from("html")));
        assert_eq!(o.cache, CacheOption::Disabled);
        assert!(!o.auto_reload);
        assert_eq!(o.optimizations, -1);
        assert!(!o.use_yield);
        assert!(o.unknown.is_empty());
    }

    #[test]
    fn overrides() {
        let mut a = Array::new();
        a.insert("debug", true);
        a.insert("autoescape", false);
        a.insert("charset", "utf8");
        a.insert("bogus", 1);
        let mut d = twig_value::Collect::new();
        let o = EnvironmentOptions::from_options(&a, &mut d).unwrap();
        assert!(o.debug && o.auto_reload);
        assert_eq!(o.autoescape, Autoescape::Disabled);
        assert_eq!(o.charset, "UTF-8");
        assert_eq!(o.unknown.len(), 1);
        let mut a = Array::new();
        a.insert("autoescape", "name");
        a.insert("cache", true);
        let err = EnvironmentOptions::from_options(&a, &mut d).unwrap_err();
        assert_eq!(err.class().name(), "LogicException");
    }
}
