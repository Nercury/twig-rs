//! Everything the fixture harness needs for one case, in Rust values.

use twig_value::{PhpError, PhpString, Value};

use crate::config::EnvironmentOptions;
use crate::convert::{Conversion, ConversionError, convert};
use crate::corpus::{Case, Phase, PhpState, SupplementStatus};
use crate::factory::{NoFactory, ObjectFactory};
use crate::json::ErrorInfo;

/// Why the options of a case could not be built.
#[derive(Clone, Debug)]
pub enum ConfigError {
    /// The CONFIG tree contains something the conversion cannot reproduce.
    Conversion(ConversionError),
    /// The CONFIG value is not an array.
    NotAnArray,
    /// `Environment::__construct()` would throw.
    Php(PhpError),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::Conversion(e) => write!(f, "CONFIG conversion failed: {e}"),
            ConfigError::NotAnArray => f.write_str("CONFIG is not an array"),
            ConfigError::Php(e) => write!(f, "Environment::__construct() throws {e}"),
        }
    }
}

impl std::error::Error for ConfigError {}

/// What the fixture file declares for a case.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Declared {
    /// `--EXPECT--`, trimmed of `"\n "` like the harness does before comparing.
    Output(PhpString),
    /// `--EXCEPTION--`, trimmed like the harness (`trim()`): `"<class>: <message>"`; render-phase
    /// errors are compared with PHPUnit's `assertStringMatchesFormat()` (`%s` placeholders),
    /// load-phase errors exactly.
    Exception(String),
}

/// What PHP did (the oracle run).
#[derive(Clone, Debug)]
pub struct PhpOutcome {
    /// Where the case ended.
    pub phase: Phase,
    /// The compared output (see [`CaseResult::output`](crate::CaseResult::output)).
    pub output: Option<PhpString>,
    /// The exception, if one was thrown.
    pub error: Option<ErrorInfo>,
    /// The harness verdict (true for every case of Twig 3.31.1 on PHP 8.5.13).
    pub pass: bool,
}

/// The deprecations of a case.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Deprecations {
    /// The `--DEPRECATION--` section (`''` when absent).
    pub declared: String,
    /// What the harness compared with it: load-phase deprecations, plus the render-phase ones
    /// when the fixture declares deprecations (joined with `"\n"` for the comparison).
    pub compared: Vec<String>,
    /// Every deprecation raised while evaluating DATA and rendering.
    pub render: Vec<String>,
}

/// One case, converted.
#[derive(Clone, Debug)]
pub struct PreparedCase {
    /// The fixture path.
    pub file: String,
    /// The case index.
    pub index: usize,
    /// The template sources with the harness's cumulative padding, in section order.
    pub templates: Vec<(String, String)>,
    /// The DATA context; `None` when the case stopped in the load phase (DATA never ran).
    pub context: Option<Result<Conversion, ConversionError>>,
    /// The Environment options.
    pub config: Result<EnvironmentOptions, ConfigError>,
    /// PHP's state before rendering (render-phase cases).
    pub php_state: Option<PhpState>,
    /// What the fixture declares.
    pub declared: Declared,
    /// What PHP did.
    pub php: PhpOutcome,
    /// Deprecations.
    pub deprecations: Deprecations,
    /// Whether the DATA tree came from the supplement.
    pub supplement: SupplementStatus,
}

impl PreparedCase {
    /// The context value, when it was evaluated and fully converted.
    pub fn context_value(&self) -> Option<&Value> {
        match &self.context {
            Some(Ok(c)) => Some(&c.value),
            _ => None,
        }
    }
}

/// PHP's `trim()` with the default characters.
fn php_trim(s: &str) -> &str {
    s.trim_matches(|c| matches!(c, ' ' | '\t' | '\n' | '\r' | '\0' | '\x0B'))
}

impl Case<'_> {
    /// Converts the case's CONFIG tree into Environment options.
    pub fn options(&self) -> Result<EnvironmentOptions, ConfigError> {
        let conv = convert(&self.result.config, &NoFactory).map_err(ConfigError::Conversion)?;
        let Value::Array(options) = conv.value else {
            return Err(ConfigError::NotAnArray);
        };
        let mut diag: Vec<twig_value::Diagnostic> = Vec::new();
        EnvironmentOptions::from_options(&options, &mut diag).map_err(ConfigError::Php)
    }

    /// Converts the DATA context; `None` for load-phase cases.
    pub fn context(
        &self,
        factory: &dyn ObjectFactory,
    ) -> Option<Result<Conversion, ConversionError>> {
        self.data_tree().map(|tree| convert(tree, factory))
    }

    /// What the fixture declares for this case.
    pub fn declared(&self) -> Declared {
        match &self.fixture.exception_expected {
            Some(e) => Declared::Exception(php_trim(e).to_string()),
            None => {
                let raw = self
                    .section()
                    .map(|s| s.expect_raw.as_str())
                    .unwrap_or_default();
                Declared::Output(PhpString::from(raw.trim_matches(|c| c == '\n' || c == ' ')))
            }
        }
    }

    /// Everything the harness needs for this case.
    pub fn prepare(&self, factory: &dyn ObjectFactory) -> PreparedCase {
        let r = self.result;
        PreparedCase {
            file: self.file().to_string(),
            index: self.index(),
            templates: self.templates(),
            context: self.context(factory),
            config: self.options(),
            php_state: r.state.clone(),
            declared: self.declared(),
            php: PhpOutcome {
                phase: r.phase,
                output: r.output_bytes(),
                error: r.error.clone(),
                pass: r.pass,
            },
            deprecations: Deprecations {
                declared: self.fixture.deprecation_expected.clone(),
                compared: r.deprecations.clone(),
                render: r.render_deprecations.clone().unwrap_or_default(),
            },
            supplement: self.supplement_status().clone(),
        }
    }
}
