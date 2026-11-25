//! Deprecation notices.
//!
//! Twig reports deprecations with `trigger_deprecation()` from symfony/deprecation-contracts, which raises an
//! `E_USER_DEPRECATED` PHP notice and lets execution continue. twig-rs reports them to a [`DeprecationSink`]
//! passed by the caller (ultimately the environment, which decides the policy); the library never prints or
//! silently drops them.
//!
//! Messages are Rust strings: template bytes that are not valid UTF-8 (for example an escaped byte of a multi-byte
//! character in Twig 3.12's escape deprecation) are converted lossily, as in error messages.

use std::fmt;

/// A deprecation notice.
///
/// Its [`Display`](fmt::Display) form is the exact message PHP's `trigger_deprecation($package, $version,
/// $message)` raises: `Since <package> <version>: <message>`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Deprecation {
    package: String,
    version: String,
    message: String,
}

impl Deprecation {
    /// A notice for `package` (e.g. `twig/twig`) deprecated since `version`, with an already formatted message.
    pub fn new(
        package: impl Into<String>,
        version: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Deprecation {
            package: package.into(),
            version: version.into(),
            message: message.into(),
        }
    }

    /// A `twig/twig` notice. Twig: `trigger_deprecation('twig/twig', $version, $message)`.
    pub fn twig(version: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new("twig/twig", version, message)
    }

    /// The package name, e.g. `twig/twig`.
    pub fn package(&self) -> &str {
        &self.package
    }

    /// The version the behaviour is deprecated since, e.g. `3.12`.
    pub fn version(&self) -> &str {
        &self.version
    }

    /// The message without the `Since <package> <version>: ` prefix.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for Deprecation {
    /// `($package || $version ? "Since $package $version: " : '').$message`
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let truthy = |s: &str| !s.is_empty() && s != "0";
        if truthy(&self.package) || truthy(&self.version) {
            write!(f, "Since {} {}: ", self.package, self.version)?;
        }
        f.write_str(&self.message)
    }
}

/// Receives the deprecation notices raised while lexing, parsing, compiling or rendering.
pub trait DeprecationSink {
    /// Reports one deprecation notice. Execution continues afterwards, as in PHP.
    fn report(&mut self, deprecation: Deprecation);
}

/// Collects notices in order.
impl DeprecationSink for Vec<Deprecation> {
    fn report(&mut self, deprecation: Deprecation) {
        self.push(deprecation);
    }
}

impl<S: DeprecationSink + ?Sized> DeprecationSink for &mut S {
    fn report(&mut self, deprecation: Deprecation) {
        (**self).report(deprecation);
    }
}

/// A sink that discards every notice, for callers that explicitly do not care about deprecations.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IgnoreDeprecations;

impl DeprecationSink for IgnoreDeprecations {
    fn report(&mut self, _deprecation: Deprecation) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_like_trigger_deprecation() {
        let deprecation = Deprecation::twig("3.12", "Character \"T\" should not be escaped.");
        assert_eq!(
            deprecation.to_string(),
            "Since twig/twig 3.12: Character \"T\" should not be escaped."
        );
        assert_eq!(Deprecation::new("", "", "bare").to_string(), "bare");
        let mut sink: Vec<Deprecation> = Vec::new();
        sink.report(deprecation.clone());
        assert_eq!(sink, [deprecation]);
    }
}
