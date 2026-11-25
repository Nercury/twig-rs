//! `tests/twig/oracle-corpus.json`, written by `research/php-oracle/tools/export-corpus.php`: for every fixture,
//! PHP's parse of the `.test` file, heuristic classification flags and the **actual** PHP results of every
//! DATA/EXPECT case from a faithful copy of `IntegrationTestCase::doIntegrationTest()` (`TwigOracle\run_fixture()` in
//! `research/php-oracle/lib.php`, one PHP process per fixture). Cases that reach the render phase carry the typed
//! tree of the evaluated `--DATA--` context and the PHP state right before rendering
//! (`reference/twig/test-suite.md` §4.5, §7.6).
//!
//! The typed trees ([`TypedValue`]) are the seam to the runtime: a later stage converts them into twig-rs values
//! (and the PHP test classes into host objects); this module only models them.

use std::fmt;
use std::path::Path;

use serde::Deserialize;
use serde::de::{self, Deserializer, MapAccess, SeqAccess, Visitor};

/// The corpus file.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Corpus {
    /// `php-oracle/tools/export-corpus.php`.
    pub generated_by: String,
    /// `3.31.1`.
    pub twig_version: String,
    /// `8.5.13`.
    pub php_version: String,
    /// One entry per `.test` file, sorted by path.
    pub fixtures: Vec<CorpusFixture>,
}

/// One `.test` file: PHP's parse and the oracle run.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CorpusFixture {
    /// The path relative to `tests/Fixtures` (`filters/abs.test`).
    pub file: String,
    /// The path contains `.legacy.test`.
    pub legacy: bool,
    /// `$message`.
    pub message: String,
    /// `$condition` (`""` when absent).
    pub condition: String,
    /// `$deprecation` (`""` when absent).
    pub deprecation_expected: String,
    /// `$exception`: the raw `--EXCEPTION--` text, `None` for output fixtures (PHP `false`).
    #[serde(deserialize_with = "false_or_string")]
    pub exception_expected: Option<String>,
    /// `$templates`: name => source in order.
    #[serde(deserialize_with = "ordered_string_map")]
    pub templates: Vec<(String, String)>,
    /// `$outputs`.
    pub outputs: Vec<CorpusOutput>,
    /// Heuristic classification (regexes over the sources, see `export-corpus.php`; not PHP facts).
    pub flags: Flags,
    /// The oracle run.
    pub actual: FixtureRun,
}

/// One entry of `$outputs`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CorpusOutput {
    /// `$data`: the `--DATA--` PHP statements.
    pub data_php: String,
    /// `$config`: the `--CONFIG--` PHP statements (`""` when absent), `None` (PHP `null`) in exception fixtures.
    pub config_php: Option<String>,
    /// `$expect`: the raw `--EXPECT--` text (`""` in exception fixtures).
    pub expect_raw: String,
}

/// The classification flags of `export-corpus.php`, for selecting fixtures by feature (the harness itself does not
/// use them).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
pub struct Flags {
    /// Test-harness callables the templates use (`filter:nl2br`, `test:multi word`, `tag:§`, ...).
    pub custom_extension: Vec<String>,
    /// The templates call `dump()`.
    pub debug_extension: bool,
    /// The templates call `template_from_string()`.
    pub string_loader_extension: bool,
    /// The templates use the sandbox.
    pub sandbox: bool,
    /// The templates use the `global` global, `_context` or `dump()` without arguments.
    pub harness_global: bool,
    /// The templates use `constant()`.
    pub php_constants: bool,
    /// The templates call `enum()` / `enum_cases()`.
    #[serde(rename = "enum")]
    pub enum_functions: bool,
    /// Dates in the DATA code or the templates.
    pub dates: bool,
    /// The DATA code uses `$twig`.
    pub data_uses_twig: bool,
    /// The DATA code declares classes.
    pub data_defines_classes: bool,
    /// Classes instantiated with `new` in the DATA code.
    pub data_new_classes: Vec<String>,
    /// Option names set by CONFIG.
    pub config_keys: Vec<String>,
}

/// The oracle run of one fixture (`TwigOracle\run_fixture()` with `typed: true`).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FixtureRun {
    /// The batch id (`null`).
    pub id: Option<serde_json::Value>,
    /// `fixture`.
    pub kind: String,
    /// Same as [`CorpusFixture::file`].
    pub file: String,
    /// Same as [`CorpusFixture::message`].
    pub message: String,
    /// Same as [`CorpusFixture::legacy`].
    pub legacy: bool,
    /// Same as [`CorpusFixture::exception_expected`].
    #[serde(deserialize_with = "false_or_string")]
    pub exception_expected: Option<String>,
    /// Same as [`CorpusFixture::deprecation_expected`].
    pub deprecation_expected: String,
    /// `condition: <expression>` when the `--CONDITION--` is false; the fixture then has no case.
    pub skipped: Option<String>,
    /// The cases that ran. A load-phase exception ends the fixture, as in the harness.
    pub cases: Vec<CaseResult>,
}

/// Where a case ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    /// `$twig->load()` of every template (lexing, parsing, compiling, constructing).
    Load,
    /// DATA evaluation and `render()` of `index.twig`.
    Render,
}

/// The actual PHP result of one DATA/EXPECT case.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaseResult {
    /// The 0-based case index.
    pub index: usize,
    /// Typed tree of `array_merge(['cache' => false, 'strict_variables' => true], CONFIG)`.
    pub config: TypedValue,
    /// Where the case ended.
    pub phase: Phase,
    /// Typed tree of the evaluated `--DATA--` context (render-phase cases).
    #[serde(default)]
    pub data: Option<TypedValue>,
    /// The PHP state right before rendering, after the DATA code ran (render-phase cases).
    #[serde(default)]
    pub state: Option<PhpState>,
    /// The exception PHP threw, if any.
    pub error: Option<PhpError>,
    /// What the harness compares: the rendered output trimmed with `"\n "`, or `Twig\Error\Error: <class>:
    /// <message>` for an unexpected render exception; `None` when an exception ended the case.
    pub output: Option<String>,
    /// Base64 of the output bytes when they are not valid UTF-8 (`output` then has `?` substitutions).
    #[serde(default)]
    pub output_b64: Option<String>,
    /// The `--EXPECT--` text trimmed with `"\n "` (output fixtures that reached the comparison).
    #[serde(default)]
    pub expected: Option<String>,
    /// Base64 of the expected bytes when they are not valid UTF-8.
    #[serde(default)]
    pub expected_b64: Option<String>,
    /// The harness verdict.
    pub pass: bool,
    /// The deprecations the harness compares with `--DEPRECATION--`: those raised while loading, plus those raised
    /// while rendering when the fixture has a `--DEPRECATION--` section.
    pub deprecations: Vec<String>,
    /// Every deprecation raised while evaluating DATA and rendering (render-phase cases).
    #[serde(default)]
    pub render_deprecations: Option<Vec<String>>,
}

/// The PHP state a case renders with (after its DATA code ran).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhpState {
    /// `date_default_timezone_get()` (DATA may call `date_default_timezone_set()`; `UTC` otherwise).
    pub php_default_timezone: String,
    /// `CoreExtension::getTimezone()->getName()`.
    pub core_timezone: String,
    /// `CoreExtension::getDateFormat()`: `[date format, interval format]`.
    pub core_date_format: (String, String),
    /// `CoreExtension::getNumberFormat()`: `[decimals, decimal point, thousands separator]`.
    pub core_number_format: (i64, String, String),
}

/// A PHP `Throwable` as `describe_throwable()` records it.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhpError {
    /// The class (`Twig\Error\SyntaxError`, `TypeError`, ...).
    pub class: String,
    /// `getMessage()`.
    pub message: String,
    /// Base64 of the message bytes when they are not valid UTF-8.
    #[serde(default)]
    pub message_b64: Option<String>,
    /// `<class>: <message>`, the string the harness compares with `--EXCEPTION--`.
    pub formatted: String,
    /// Base64 of `formatted` when it is not valid UTF-8.
    #[serde(default)]
    pub formatted_b64: Option<String>,
    /// Twig errors: `getRawMessage()`.
    #[serde(default, rename = "rawMessage")]
    pub raw_message: Option<String>,
    /// Base64 of the raw message when it is not valid UTF-8.
    #[serde(default, rename = "rawMessage_b64")]
    pub raw_message_b64: Option<String>,
    /// Twig errors: the name of the source context (absent or `null` when there is none).
    #[serde(default, rename = "templateName")]
    pub template_name: Option<String>,
    /// Twig errors: `getTemplateLine()`.
    #[serde(default, rename = "templateLine")]
    pub template_line: Option<i64>,
    /// Twig errors: `getTemplateColumn()`.
    #[serde(default, rename = "templateColumn")]
    pub template_column: Option<i64>,
    /// Other throwables: `getCode()`.
    #[serde(default)]
    pub code: Option<i64>,
    /// `getPrevious()`, up to 5 levels.
    #[serde(default)]
    pub previous: Option<Box<PhpError>>,
}

/// A PHP value as `typed()` in `research/php-oracle/lib.php` exports it.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "t", rename_all = "lowercase", deny_unknown_fields)]
pub enum TypedValue {
    /// `null`.
    Null,
    /// A bool.
    Bool {
        /// The value.
        v: bool,
    },
    /// An int (`zend_long`).
    Int {
        /// The value.
        v: i64,
    },
    /// A float, as `var_export()` prints it (shortest round trip, e.g. `0.1`, `1.0E+25`, `-0.0`), or `INF`, `-INF`,
    /// `NAN`.
    Float {
        /// The text.
        v: String,
    },
    /// A byte string.
    String {
        /// The text; invalid UTF-8 bytes are replaced by `?`.
        v: String,
        /// Base64 of the bytes when they are not valid UTF-8.
        #[serde(default)]
        v_b64: Option<String>,
    },
    /// An array: key/value pairs in order.
    Array {
        /// The entries.
        v: Vec<(ArrayKey, TypedValue)>,
    },
    /// An object seen for the first time in this tree.
    Object(Box<TypedObject>),
    /// An enum case.
    Enum {
        /// The enum class.
        class: String,
        /// `spl_object_id()`.
        id: i64,
        /// The case name.
        case: String,
        /// The backing value of a backed enum.
        #[serde(default)]
        value: Option<Box<TypedValue>>,
    },
    /// A closure.
    Closure {
        /// `Closure`.
        class: String,
        /// `spl_object_id()`.
        id: i64,
    },
    /// A resource.
    Resource {
        /// `get_resource_type()` (`stream`).
        #[serde(rename = "type")]
        kind: String,
    },
    /// An object already exported earlier in the same tree (`spl_object_id()` repeats).
    Ref {
        /// The class.
        class: String,
        /// `spl_object_id()` of the earlier [`TypedValue::Object`].
        id: i64,
    },
    /// Nesting deeper than 64 levels.
    Truncated,
}

/// An array key: PHP normalises integer-like string keys to integers.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum ArrayKey {
    /// An integer key.
    Int(i64),
    /// A string key.
    String(String),
}

/// An object: class, `spl_object_id()` and one class-specific payload.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TypedObject {
    /// The class name (anonymous classes include a NUL byte and the declaring file).
    pub class: String,
    /// `spl_object_id()`.
    pub id: i64,
    /// `Twig\Markup`: the string content.
    #[serde(default)]
    pub content: Option<TypedValue>,
    /// `Twig\Markup`: the charset.
    #[serde(default)]
    pub charset: Option<String>,
    /// `DateTimeInterface`: `format('Y-m-d\TH:i:s.uP')`.
    #[serde(default)]
    pub datetime: Option<String>,
    /// `DateTimeInterface`: the timezone name.
    #[serde(default)]
    pub timezone: Option<String>,
    /// `DateTimeInterface`: the Unix timestamp.
    #[serde(default)]
    pub timestamp: Option<i64>,
    /// `DateTimeZone`: the name.
    #[serde(default)]
    pub name: Option<String>,
    /// `DateInterval`: the fields.
    #[serde(default)]
    pub spec: Option<DateIntervalSpec>,
    /// `Twig\TemplateWrapper`: the template name.
    #[serde(default)]
    pub template: Option<String>,
    /// `ArrayObject` / `ArrayIterator`: `getArrayCopy()`.
    #[serde(default)]
    pub entries: Option<TypedValue>,
    /// `SimpleXMLElement`: `asXML()`.
    #[serde(default)]
    pub xml: Option<String>,
    /// Other objects: the implemented interfaces, sorted.
    #[serde(default)]
    pub interfaces: Option<Vec<String>>,
    /// Other objects with `__toString()`: the string conversion.
    #[serde(default, rename = "toString")]
    pub to_string: Option<TypedValue>,
    /// Other objects with `__toString()`: the exception the conversion threw.
    #[serde(default, rename = "toString_error")]
    pub to_string_error: Option<PhpError>,
    /// Other objects: `get_object_vars()` in order.
    #[serde(default)]
    pub props: Option<Vec<(ArrayKey, TypedValue)>>,
}

/// The fields of a `DateInterval`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DateIntervalSpec {
    /// Years.
    pub y: i64,
    /// Months.
    pub m: i64,
    /// Days.
    pub d: i64,
    /// Hours.
    pub h: i64,
    /// Minutes.
    pub i: i64,
    /// Seconds.
    pub s: i64,
    /// Microseconds as a fraction of a second.
    pub f: f64,
    /// 1 for a negative interval.
    pub invert: i64,
    /// The total number of days (intervals from `diff()`), `None` for PHP `false`.
    #[serde(deserialize_with = "false_or_int")]
    pub days: Option<i64>,
}

/// The `Environment` options of a case, from [`CaseResult::config`] with the defaults and casts of
/// `Environment::__construct()` and `setCharset()` (TWIG/src/Environment.php).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvironmentOptions {
    /// `(bool) $options['debug']`.
    pub debug: bool,
    /// The charset, upper-cased, `UTF8` spelled `UTF-8`.
    pub charset: String,
    /// `(bool) $options['strict_variables']`.
    pub strict_variables: bool,
    /// The default escaping strategy of the `EscaperExtension`.
    pub autoescape: Autoescape,
    /// `cache`: `None` for `false` (the harness never caches), else the cache directory.
    pub cache: Option<String>,
    /// `auto_reload`, following `debug` when it is `null`.
    pub auto_reload: bool,
    /// `(int) $options['optimizations']`.
    pub optimizations: i64,
    /// `(bool) $options['use_yield']`.
    pub use_yield: bool,
}

/// The `autoescape` option.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Autoescape {
    /// `false`.
    Disabled,
    /// A strategy name: `html`, `js`, ..., or `name` (guess from the template name's extension).
    Strategy(String),
}

impl Corpus {
    /// Loads and parses the corpus file.
    pub fn load(path: &Path) -> Result<Corpus, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        serde_json::from_str(&text).map_err(|e| format!("invalid corpus {}: {e}", path.display()))
    }

    /// The fixture with this path.
    pub fn fixture(&self, file: &str) -> Option<&CorpusFixture> {
        self.fixtures.iter().find(|f| f.file == file)
    }
}

impl FixtureRun {
    /// The case with this index, if PHP ran it.
    pub fn case(&self, index: usize) -> Option<&CaseResult> {
        self.cases.iter().find(|c| c.index == index)
    }
}

impl EnvironmentOptions {
    /// The options from a typed `config` tree (an array of option name => value).
    pub fn from_config(config: &TypedValue) -> Result<EnvironmentOptions, String> {
        let TypedValue::Array { v: entries } = config else {
            return Err(format!("the config is not an array: {config:?}"));
        };
        let get = |name: &str| {
            entries
                .iter()
                .rev()
                .find(|(k, _)| matches!(k, ArrayKey::String(s) if s == name))
                .map(|(_, v)| v)
        };
        let known = [
            "debug",
            "charset",
            "strict_variables",
            "autoescape",
            "cache",
            "auto_reload",
            "optimizations",
            "use_yield",
        ];
        if let Some((key, _)) = entries
            .iter()
            .find(|(k, _)| !matches!(k, ArrayKey::String(s) if known.contains(&s.as_str())))
        {
            return Err(format!("unknown Environment option {key:?}"));
        }
        let flag = |name: &str, default: bool| get(name).map_or(Ok(default), php_bool);
        let debug = flag("debug", false)?;
        let charset = match get("charset") {
            None | Some(TypedValue::Null) => "UTF-8".to_owned(),
            Some(TypedValue::String { v, v_b64: None }) => match v.to_ascii_uppercase() {
                upper if upper == "UTF8" => "UTF-8".to_owned(),
                upper => upper,
            },
            Some(other) => return Err(format!("unsupported charset {other:?}")),
        };
        let autoescape = match get("autoescape") {
            None => Autoescape::Strategy("html".to_owned()),
            Some(TypedValue::Bool { v: false }) => Autoescape::Disabled,
            Some(TypedValue::String { v, v_b64: None }) => Autoescape::Strategy(v.clone()),
            Some(other) => return Err(format!("unsupported autoescape {other:?}")),
        };
        let cache = match get("cache") {
            None | Some(TypedValue::Bool { v: false }) => None,
            Some(TypedValue::String { v, v_b64: None }) => Some(v.clone()),
            Some(other) => return Err(format!("unsupported cache {other:?}")),
        };
        let auto_reload = match get("auto_reload") {
            None | Some(TypedValue::Null) => debug,
            Some(value) => php_bool(value)?,
        };
        let optimizations = match get("optimizations") {
            None => -1,
            Some(TypedValue::Int { v }) => *v,
            Some(TypedValue::Bool { v }) => i64::from(*v),
            Some(TypedValue::Null) => 0,
            Some(other) => return Err(format!("unsupported optimizations {other:?}")),
        };
        Ok(EnvironmentOptions {
            debug,
            charset,
            strict_variables: flag("strict_variables", false)?,
            autoescape,
            cache,
            auto_reload,
            optimizations,
            use_yield: flag("use_yield", false)?,
        })
    }
}

/// PHP's `(bool)` cast of a scalar.
fn php_bool(value: &TypedValue) -> Result<bool, String> {
    Ok(match value {
        TypedValue::Null => false,
        TypedValue::Bool { v } => *v,
        TypedValue::Int { v } => *v != 0,
        TypedValue::String { v, v_b64: None } => !(v.is_empty() || v == "0"),
        TypedValue::Array { v } => !v.is_empty(),
        other => return Err(format!("unsupported option value {other:?}")),
    })
}

/// Decodes standard base64 with `=` padding, as PHP's `base64_encode()` writes it.
pub fn decode_base64(text: &str) -> Result<Vec<u8>, String> {
    fn sextet(c: u8) -> Option<u32> {
        Some(u32::from(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        }))
    }
    let bytes = text.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return Err(format!(
            "base64 length {} is not a multiple of 4",
            bytes.len()
        ));
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    for (index, chunk) in bytes.chunks(4).enumerate() {
        let padding = chunk.iter().rev().take_while(|&&c| c == b'=').count();
        if padding > 2 || (padding > 0 && (index + 1) * 4 != bytes.len()) {
            return Err("misplaced base64 padding".to_owned());
        }
        let mut acc = 0u32;
        for &c in &chunk[..4 - padding] {
            acc = (acc << 6) | sextet(c).ok_or_else(|| format!("invalid base64 byte {c:#04x}"))?;
        }
        acc <<= 6 * padding;
        out.extend_from_slice(&acc.to_be_bytes()[1..4 - padding]);
    }
    Ok(out)
}

/// `false` or a string: `None` for `false`.
fn false_or_string<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum FalseOrString {
        Text(String),
        Flag(bool),
    }
    match FalseOrString::deserialize(d)? {
        FalseOrString::Text(text) => Ok(Some(text)),
        FalseOrString::Flag(false) => Ok(None),
        FalseOrString::Flag(true) => Err(de::Error::custom("expected false or a string")),
    }
}

/// `false` or an integer: `None` for `false`.
fn false_or_int<'de, D: Deserializer<'de>>(d: D) -> Result<Option<i64>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum FalseOrInt {
        Int(i64),
        Flag(bool),
    }
    match FalseOrInt::deserialize(d)? {
        FalseOrInt::Int(value) => Ok(Some(value)),
        FalseOrInt::Flag(false) => Ok(None),
        FalseOrInt::Flag(true) => Err(de::Error::custom("expected false or an integer")),
    }
}

/// A JSON object of strings as its entries in document order (PHP arrays keep insertion order; `serde_json`'s map
/// does not). PHP's `json_encode()` writes an empty array as `[]`.
fn ordered_string_map<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<(String, String)>, D::Error> {
    struct Entries;

    impl<'de> Visitor<'de> for Entries {
        type Value = Vec<(String, String)>;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("an object of strings")
        }

        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            let mut entries = Vec::new();
            while let Some(entry) = map.next_entry::<String, String>()? {
                entries.push(entry);
            }
            Ok(entries)
        }

        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            match seq.next_element::<de::IgnoredAny>()? {
                None => Ok(Vec::new()),
                Some(_) => Err(de::Error::custom("expected an object")),
            }
        }
    }

    d.deserialize_any(Entries)
}
