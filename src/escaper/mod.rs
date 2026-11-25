//! Escaping: the default strategy of a template, the file-extension strategy guesser, and the built-in escaping
//! strategies (`html`, `js`, `css`, `html_attr`, `html_attr_relaxed`, `url`).
//!
//! - [`DefaultStrategy`]: Twig's `autoescape` environment option, as given to
//!   [`EscaperExtension`](crate::extension::EscaperExtension) (`EscaperExtension::setDefaultStrategy()`).
//! - [`guess`]: Twig's `FileExtensionEscapingStrategy::guess()` (TWIG/src/FileExtensionEscapingStrategy.php), the
//!   strategy of the `'name'` default.
//! - [`Strategy`], [`implied_strategies`], [`invalid_strategy_message`]: the strategy names and rules of
//!   `EscaperRuntime` (TWIG/src/Runtime/EscaperRuntime.php).
//! - [`escape`] and the per-strategy functions ([`escape_html`], [`escape_js`], [`escape_css`],
//!   [`escape_html_attr`], [`escape_url`]): the string part of `EscaperRuntime::escape()` once the value is a string
//!   and the charset is known, with PHP's `htmlspecialchars()` and `iconv()` from `twig-value`. The `escape` filter's
//!   runtime (the `Markup` and safe-class shortcuts, non-string values, custom escapers) is built on top of them:
//!
//! ```text
//! escape($value, $strategy, $charset, $autoescape):
//!     Markup / Stringable / non-string handling          (the filter's runtime)
//!     match Strategy::from_name($strategy):
//!         Some(strategy) => escape($string, strategy, $charset ?: env charset, diagnostics)
//!         None => custom escaper, else RuntimeError(invalid_strategy_message($strategy, custom names))
//! ```

mod strategies;

pub use strategies::{
    EscapeError, EscapeResult, INVALID_UTF8_MESSAGE, escape, escape_css, escape_html,
    escape_html_attr, escape_js, escape_url,
};

use std::fmt;
use std::sync::Arc;

use crate::ast::ConstantValue;
use crate::error::Result;

/// The built-in strategies, in the order of Twig's `Invalid escaping strategy` message.
pub const BUILTIN_STRATEGIES: [&str; 6] =
    ["html", "js", "url", "css", "html_attr", "html_attr_relaxed"];

/// A built-in escaping strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Strategy {
    /// `html`: `htmlspecialchars()` with `ENT_QUOTES | ENT_SUBSTITUTE`.
    Html,
    /// `js`: JavaScript string escapes (`\uHHHH`, short escapes).
    Js,
    /// `css`: CSS hex escapes (`\HH `).
    Css,
    /// `html_attr`: HTML attribute value escapes (`&#xHH;`, named entities).
    HtmlAttr,
    /// `html_attr_relaxed`: as `html_attr`, also keeping `:`, `@`, `[` and `]`.
    HtmlAttrRelaxed,
    /// `url`: `rawurlencode()`.
    Url,
}

impl Strategy {
    /// The strategy with this name (exact, case-sensitive match, as Twig's `switch`).
    pub fn from_name(name: &[u8]) -> Option<Strategy> {
        Some(match name {
            b"html" => Strategy::Html,
            b"js" => Strategy::Js,
            b"css" => Strategy::Css,
            b"html_attr" => Strategy::HtmlAttr,
            b"html_attr_relaxed" => Strategy::HtmlAttrRelaxed,
            b"url" => Strategy::Url,
            _ => return None,
        })
    }

    /// The strategy name.
    pub fn name(self) -> &'static str {
        match self {
            Strategy::Html => "html",
            Strategy::Js => "js",
            Strategy::Css => "css",
            Strategy::HtmlAttr => "html_attr",
            Strategy::HtmlAttrRelaxed => "html_attr_relaxed",
            Strategy::Url => "url",
        }
    }
}

/// The strategies a value safe for `strategy` is also safe for: `html_attr` implies `html_attr_relaxed` and `html`,
/// `html_attr_relaxed` implies `html`.
///
/// Twig: `EscaperRuntime::IMPLIED_STRATEGIES` (the same rule as `SafeAnalysisNodeVisitor::getSafe()`).
pub fn implied_strategies(strategy: &[u8]) -> &'static [&'static str] {
    match strategy {
        b"html_attr" => &["html_attr_relaxed", "html"],
        b"html_attr_relaxed" => &["html"],
        _ => &[],
    }
}

/// The message of the `RuntimeError` for an unknown strategy: `Invalid escaping strategy "%s" (valid ones: "%s").`
/// listing the built-in strategies, then the custom escapers in registration order.
///
/// Twig: the `default` branch of `EscaperRuntime::escape()`.
pub fn invalid_strategy_message<'a>(
    strategy: &[u8],
    custom_escapers: impl IntoIterator<Item = &'a [u8]>,
) -> String {
    let mut valid: Vec<String> = BUILTIN_STRATEGIES.iter().map(|s| (*s).to_owned()).collect();
    valid.extend(
        custom_escapers
            .into_iter()
            .map(|name| String::from_utf8_lossy(name).into_owned()),
    );
    format!(
        "Invalid escaping strategy \"{}\" (valid ones: \"{}\").",
        String::from_utf8_lossy(strategy),
        valid.join("\", \"")
    )
}

/// Guesses the escaping strategy from a template name: `html` for names ending with `/` or `\`, otherwise from the
/// extension of the name without a `.twig` suffix: `js` for `js` and `json`, `css` for `css`, `None` (Twig's
/// `false`: no escaping) for `txt`, `html` for everything else.
///
/// Twig: `FileExtensionEscapingStrategy::guess()`; the extension is PHP's `pathinfo($name, PATHINFO_EXTENSION)`.
pub fn guess(name: &str) -> Option<&'static str> {
    let name = name.as_bytes();
    if matches!(name.last(), Some(b'/' | b'\\')) {
        return Some("html"); // return html for directories
    }
    let name = name.strip_suffix(b".twig").unwrap_or(name);
    match pathinfo_extension(name) {
        b"js" | b"json" => Some("js"),
        b"css" => Some("css"),
        b"txt" => None,
        _ => Some("html"),
    }
}

/// `pathinfo($path, PATHINFO_EXTENSION)`: the part of the base name after its last `.`, `""` without one.
///
/// Port of `PHP_FUNCTION(pathinfo)` and `php_basename()` (PHPSRC/ext/standard/string.c) for the non-Windows build
/// with an ASCII-compatible `LC_CTYPE` (the oracle's `C.UTF-8`): trailing `/` are dropped, the base name starts after
/// the last `/`.
fn pathinfo_extension(path: &[u8]) -> &[u8] {
    let trimmed = match path.iter().rposition(|&b| b != b'/') {
        Some(last) => &path[..=last],
        None => return b"",
    };
    let basename = match trimmed.iter().rposition(|&b| b == b'/') {
        Some(slash) => &trimmed[slash + 1..],
        None => trimmed,
    };
    match basename.iter().rposition(|&b| b == b'.') {
        Some(dot) => &basename[dot + 1..],
        None => b"",
    }
}

/// A callable deciding the default strategy of a template from its name (see [`DefaultStrategy::Callback`]).
pub type DefaultStrategyCallback = Arc<dyn Fn(&str) -> Result<ConstantValue> + Send + Sync>;

/// The default escaping strategy: Twig's `autoescape` environment option, given to
/// [`EscaperExtension`](crate::extension::EscaperExtension).
///
/// Twig: the `$defaultStrategy` of `EscaperExtension::setDefaultStrategy()` (`string|false|callable`).
#[derive(Clone)]
pub enum DefaultStrategy {
    /// A strategy name (`html`, `js`, ...). The name `name` selects the strategy guessed from the template name
    /// ([`guess`]), as Twig's `'name'`.
    Strategy(String),
    /// `false`: templates are not auto-escaped.
    Disabled,
    /// A callable receiving the template name. It returns the strategy: a string, `false`, or another value the
    /// escaper visitor coerces as PHP does (Twig calls every default that is neither a string nor `false`).
    Callback(DefaultStrategyCallback),
}

impl DefaultStrategy {
    /// Twig's default, `html`.
    pub fn html() -> Self {
        DefaultStrategy::Strategy("html".to_owned())
    }
}

impl Default for DefaultStrategy {
    fn default() -> Self {
        DefaultStrategy::html()
    }
}

impl From<&str> for DefaultStrategy {
    fn from(strategy: &str) -> Self {
        DefaultStrategy::Strategy(strategy.to_owned())
    }
}

impl fmt::Debug for DefaultStrategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DefaultStrategy::Strategy(strategy) => {
                f.debug_tuple("Strategy").field(strategy).finish()
            }
            DefaultStrategy::Disabled => f.write_str("Disabled"),
            DefaultStrategy::Callback(_) => f.write_str("Callback(..)"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_extensions_like_pathinfo() {
        assert_eq!(pathinfo_extension(b"foo.html"), b"html");
        assert_eq!(pathinfo_extension(b"a.b/c"), b"");
        assert_eq!(pathinfo_extension(b"a/b.c/"), b"c");
        assert_eq!(pathinfo_extension(b"///"), b"");
        assert_eq!(pathinfo_extension(b""), b"");
        assert_eq!(pathinfo_extension(b".js"), b"js");
        assert_eq!(pathinfo_extension(b"a."), b"");
        assert_eq!(pathinfo_extension(b"a\\b.js"), b"js");
    }

    #[test]
    fn lists_valid_strategies() {
        assert_eq!(
            invalid_strategy_message(b"bar", [b"foo".as_slice()]),
            "Invalid escaping strategy \"bar\" (valid ones: \"html\", \"js\", \"url\", \"css\", \"html_attr\", \"html_attr_relaxed\", \"foo\")."
        );
    }
}
