//! Objects this crate does not own: the caller creates them.
//!
//! `Twig\Markup` and `Twig\TemplateWrapper` belong to the twig engine, `DateTime`,
//! `DateTimeImmutable`, `DateTimeZone` and `DateInterval` to the date layer; both are written
//! concurrently with this crate, so the converter asks an [`ObjectFactory`] for them. DATA code that
//! used `$twig` produced such objects too (`$twig->load('foo.twig')` is a
//! [`RequestKind::TemplateWrapper`]); its other effects (`setDateFormat()`, `setTimezone()`,
//! `setNumberFormat()`, `date_default_timezone_set()`) are in the case's
//! [`PhpState`](crate::PhpState).
//!
//! Resources (`opendir()` in `tests/in.test`) are not modelled by `twig-value`; they are offered to
//! the factory as well ([`RequestKind::Resource`]) so that a harness can pick a stand-in.

use twig_value::{PhpResult, PhpString, Value};

use crate::typed::{DateIntervalSpec, ObjectNode, TypedNode};

/// Creates the values of the classes listed in the [module documentation](self).
pub trait ObjectFactory {
    /// Creates the value for `request`; `None` declines (the conversion then reports
    /// [`Unsupported::NeedsFactory`](crate::Unsupported::NeedsFactory)). An `Err` is reported as
    /// [`Unsupported::FactoryFailed`](crate::Unsupported::FactoryFailed).
    fn create(&self, request: &FactoryRequest<'_>) -> Option<PhpResult<Value>>;
}

/// A factory that declines everything.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoFactory;

impl ObjectFactory for NoFactory {
    fn create(&self, _request: &FactoryRequest<'_>) -> Option<PhpResult<Value>> {
        None
    }
}

impl<F: Fn(&FactoryRequest<'_>) -> Option<PhpResult<Value>>> ObjectFactory for F {
    fn create(&self, request: &FactoryRequest<'_>) -> Option<PhpResult<Value>> {
        self(request)
    }
}

/// What the converter needs.
#[derive(Clone, Debug)]
pub struct FactoryRequest<'a> {
    /// The PHP class (`Twig\Markup`, ...); `resource(<type>)` for resources.
    pub class: &'a str,
    /// The node the value comes from.
    pub node: &'a TypedNode,
    /// The decoded payload.
    pub kind: RequestKind<'a>,
    /// Where the value sits in the context (`$["foo"][0]`).
    pub path: &'a str,
}

/// The decoded payload of a [`FactoryRequest`].
#[derive(Clone, Debug)]
pub enum RequestKind<'a> {
    /// `new \Twig\Markup($content, $charset, $safeStrategies)`.
    Markup {
        /// The content bytes.
        content: PhpString,
        /// The charset.
        charset: &'a str,
        /// The `$safeStrategies` constructor argument. Only rich (supplement) trees record it:
        /// `None` means unknown, not the default `['all']`.
        safe_strategies: Option<Vec<PhpString>>,
    },
    /// A `Twig\TemplateWrapper` (`$twig->load($template)`). Twig 3.31 caches wrappers per template
    /// class, so two loads of the same name are the same object (the converter keeps identities).
    TemplateWrapper {
        /// The template name.
        template: &'a str,
    },
    /// `DateTime` / `DateTimeImmutable`.
    DateTime {
        /// `DateTimeImmutable` rather than `DateTime`.
        immutable: bool,
        /// `format('Y-m-d\TH:i:s.uP')`.
        datetime: &'a str,
        /// `getTimezone()->getName()`.
        timezone: &'a str,
        /// `getTimestamp()`.
        timestamp: i64,
    },
    /// `DateTimeZone`.
    DateTimeZone {
        /// `getName()`.
        name: &'a str,
    },
    /// `DateInterval`.
    DateInterval {
        /// The public fields.
        spec: &'a DateIntervalSpec,
    },
    /// A resource.
    Resource {
        /// `get_resource_type()`.
        resource_type: &'a str,
    },
    /// Any other class this crate does not implement.
    Other(&'a ObjectNode),
}

/// The classes the converter always hands to the factory.
pub const FACTORY_CLASSES: &[&str] = &[
    "Twig\\Markup",
    "Twig\\TemplateWrapper",
    "DateTime",
    "DateTimeImmutable",
    "DateTimeZone",
    "DateInterval",
];
