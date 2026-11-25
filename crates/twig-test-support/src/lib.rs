//! Support for the twig-rs fixture harness: Twig 3.31.1's integration fixtures, as the PHP oracle
//! ran them, turned into `twig-value` values.
//!
//! The upstream fixtures (`tests/twig/Fixtures`) build their template context with PHP code
//! (`--DATA--`) and their Environment options with `--CONFIG--`. This crate does not evaluate PHP:
//! it reads what the oracle (PHP 8.5.13 + Twig 3.31.1, `research/php-oracle`) recorded when it
//! evaluated that code — the typed trees of `tests/twig/oracle-corpus.json`, completed by this
//! crate's `tests/vectors/fixture_data.json` — and rebuilds the values exactly, including objects of
//! the PHP classes the DATA code creates.
//!
//! It deliberately does not depend on the `twig` crate: objects owned by the engine
//! (`Twig\Markup`, `Twig\TemplateWrapper`) and by the date layer (`DateTime`,
//! `DateTimeImmutable`, `DateTimeZone`, `DateInterval`) come from a caller-supplied
//! [`ObjectFactory`].
//!
//! # Usage
//!
//! ```no_run
//! use twig_test_support::{Corpus, NoFactory};
//!
//! let corpus = Corpus::load_default()?;
//! for case in corpus.cases() {
//!     let prepared = case.prepare(&NoFactory);
//!     match &prepared.context {
//!         None => { /* load-phase case: DATA never ran */ }
//!         Some(Ok(conversion)) => { let _context = &conversion.value; }
//!         Some(Err(problems)) => eprintln!("{}#{}: {problems}", prepared.file, prepared.index),
//!     }
//! }
//! # Ok::<(), twig_test_support::LoadError>(())
//! ```
//!
//! # Map of the crate
//!
//! | module | contents |
//! |---|---|
//! | [`corpus`] | serde model of the corpus file, the supplement merge, [`Corpus`] / [`Case`] |
//! | [`typed`] | typed trees ([`TypedNode`]): the JSON node format of both producers |
//! | [`convert`](mod@convert) | typed tree → [`Value`](twig_value::Value) ([`convert()`], [`Unsupported`], [`Session`](convert::Session)) |
//! | [`factory`] | [`ObjectFactory`] for classes owned by other crates |
//! | [`config`] | CONFIG → [`EnvironmentOptions`] (port of Twig's option handling) |
//! | [`case`] | [`PreparedCase`]: context, options, PHP state, expectations, deprecations |
//! | [`classes`] | Rust ports of the PHP classes in DATA, [`fixture_class_table`](classes::fixture_class_table) |
//! | [`encode`] | [`Value`](twig_value::Value) → typed tree (port of the encoder, for round-trip checks) |
//! | [`compare`] | structural tree comparison with object identities matched up to renaming |
//!
//! # Exactness
//!
//! Scalars are bit-exact (floats from their `var_export()` text or bit pattern, strings from their
//! raw bytes); arrays keep order and key types; objects keep identity and cycles. The class ports
//! declare members in PHP's order and port every method body; `tests/` checks them against oracle
//! vectors (`var_dump()`/`var_export()`/`print_r()` of every DATA value and method probes run in
//! PHP). Known differences are listed in the repository's `reference/deviations.md`.

pub mod case;
pub mod classes;
pub mod compare;
pub mod config;
pub mod convert;
pub mod corpus;
pub mod encode;
pub mod factory;
pub mod json;
mod php;
pub mod typed;

pub use case::{ConfigError, Declared, Deprecations, PhpOutcome, PreparedCase};
pub use config::{Autoescape, CacheOption, EnvironmentOptions};
pub use convert::{Conversion, ConversionError, Unsupported, convert};
pub use corpus::{
    Case, CaseResult, Corpus, CorpusFile, Fixture, FixtureRun, Flags, LoadError, OutputSection,
    Phase, PhpState, SupplementStatus,
};
pub use factory::{FACTORY_CLASSES, FactoryRequest, NoFactory, ObjectFactory, RequestKind};
pub use json::ErrorInfo;
pub use typed::{DateIntervalSpec, EnumNode, Item, Key, ObjectNode, TypedNode};
