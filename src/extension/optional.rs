//! The optional core extensions: `SandboxExtension`, `DebugExtension`, `StringLoaderExtension` and
//! `SandboxBridgeExtension` (TWIG/src/Extension/).

use std::any::Any;
use std::sync::Arc;

use super::callable::{
    CallableParameter as P, FunctionNodeClass, FunctionOptions, PhpCallable, TwigFunction,
};
use super::{Extension, TokenParser};
use crate::ast::{ArgumentKey, Arguments, ConstantValue as V, ExprKind};
use crate::token_parser::SandboxTokenParser;

/// `Twig\Extension\SandboxExtension`: the deprecated `sandbox` tag (the sandbox visitor, the security policy and
/// the checker come with the sandbox stage). It registers no callables.
#[derive(Debug, Default)]
pub struct SandboxExtension {
    _private: (),
}

impl SandboxExtension {
    /// The sandbox extension.
    pub fn new() -> Self {
        SandboxExtension::default()
    }
}

impl Extension for SandboxExtension {
    fn class_name(&self) -> &str {
        "Twig\\Extension\\SandboxExtension"
    }

    /// Twig: `SandboxExtension::getTokenParsers()`.
    fn token_parsers(&self) -> Vec<Arc<dyn TokenParser>> {
        vec![Arc::new(SandboxTokenParser)]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// `Twig\Extension\DebugExtension`: the `dump` function.
#[derive(Debug, Default)]
pub struct DebugExtension {
    _private: (),
}

impl DebugExtension {
    /// The debug extension.
    pub fn new() -> Self {
        DebugExtension::default()
    }
}

impl Extension for DebugExtension {
    fn class_name(&self) -> &str {
        "Twig\\Extension\\DebugExtension"
    }

    /// Twig: `DebugExtension::getFunctions()`. The output of `dump` is HTML-safe under the CLI SAPI of the target
    /// (`'cli' === PHP_SAPI`), which twig-rs reproduces.
    fn functions(&self) -> Vec<TwigFunction> {
        vec![TwigFunction::new(
            "dump",
            Some(PhpCallable::method(
                "Twig\\Extension\\DebugExtension",
                "dump",
                vec![
                    P::required("env"),
                    P::required("context"),
                    P::variadic("vars"),
                ],
            )),
            FunctionOptions {
                is_safe: Some(vec!["html".into()]),
                needs_context: true,
                needs_environment: true,
                is_variadic: true,
                ..Default::default()
            },
        )]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// `Twig\Extension\StringLoaderExtension`: the `template_from_string` function.
#[derive(Debug, Default)]
pub struct StringLoaderExtension {
    _private: (),
}

impl StringLoaderExtension {
    /// The string loader extension.
    pub fn new() -> Self {
        StringLoaderExtension::default()
    }
}

impl Extension for StringLoaderExtension {
    fn class_name(&self) -> &str {
        "Twig\\Extension\\StringLoaderExtension"
    }

    /// Twig: `StringLoaderExtension::getFunctions()`.
    fn functions(&self) -> Vec<TwigFunction> {
        vec![TwigFunction::new(
            "template_from_string",
            Some(PhpCallable::method(
                "Twig\\Extension\\StringLoaderExtension",
                "templateFromString",
                vec![
                    P::required("env"),
                    P::required("template"),
                    P::optional("name", V::Null),
                ],
            )),
            FunctionOptions {
                needs_environment: true,
                ..Default::default()
            },
        )]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// `Twig\Extension\SandboxBridgeExtension`: the `render_sandboxed` function.
#[derive(Debug, Default)]
pub struct SandboxBridgeExtension {
    _private: (),
}

impl SandboxBridgeExtension {
    /// The sandbox bridge extension.
    pub fn new() -> Self {
        SandboxBridgeExtension::default()
    }
}

/// Twig: `RenderSandboxedFunction::getSafe()`: the literal `output_strategy` argument (position 2 or a name that
/// normalises to `outputstrategy`) when it is a non-empty string other than `all`.
pub fn render_sandboxed_is_safe(arguments: &Arguments) -> Vec<V> {
    let found = arguments.entries().iter().find(|(key, _)| match key {
        ArgumentKey::Index(i) => *i == 2,
        ArgumentKey::Name(name) => {
            let normalized: Vec<u8> = name
                .iter()
                .filter(|&&b| b != b'_')
                .map(u8::to_ascii_lowercase)
                .collect();
            normalized == b"outputstrategy"
        }
    });
    let Some((_, value)) = found else {
        return Vec::new();
    };
    match &value.kind {
        ExprKind::Constant(constant) => match &constant.value {
            V::String(s) if !s.is_empty() && s.as_slice() != b"all" => vec![constant.value.clone()],
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}

impl Extension for SandboxBridgeExtension {
    fn class_name(&self) -> &str {
        "Twig\\Extension\\SandboxBridgeExtension"
    }

    /// Twig: `SandboxBridgeExtension::getFunctions()`.
    fn functions(&self) -> Vec<TwigFunction> {
        vec![TwigFunction::new(
            "render_sandboxed",
            Some(PhpCallable::method(
                "Twig\\Runtime\\SandboxBridgeRuntime",
                "render",
                vec![P::required("name"), P::required("context").array()],
            )),
            FunctionOptions {
                is_safe_callback: Some(Arc::new(render_sandboxed_is_safe)),
                node_class: FunctionNodeClass::RenderSandboxed,
                ..Default::default()
            },
        )]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
