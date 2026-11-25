//! Parses a template the way `$env->parse($env->tokenize($source))` does (the parser, every core tag and the node
//! visitors of a default environment: correctness checks, auto-escaping, optimizations, plus the sandbox's checks) and
//! prints its node tree (the format of the oracle vectors), the error, and the deprecations.
//!
//!     cargo run --example parse_nodes -- '{% for user in users %}{{ user.name|upper }}{% endfor %}'
//!     cargo run --example parse_nodes -- --file template.twig
//!
//! `--quiet` (first argument) only reports errors and deprecations.

use std::process::ExitCode;
use std::sync::Arc;

use twig::Source;
use twig::ast::dump::dump_node;
use twig::deprecation::Deprecation;
use twig::extension::{ExtensionSet, OptimizerExtension, SandboxExtension};
use twig::parser::ParseContext;

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let quiet = args.first().is_some_and(|a| a == "--quiet");
    if quiet {
        args.remove(0);
    }
    let (code, name) = match args.as_slice() {
        [flag, path] if flag == "--file" => match std::fs::read(path) {
            Ok(code) => (code, path.clone()),
            Err(error) => {
                eprintln!("cannot read {path}: {error}");
                return ExitCode::FAILURE;
            }
        },
        [code] => (code.as_bytes().to_vec(), "index".to_owned()),
        _ => {
            eprintln!(
                "usage: parse_nodes [--quiet] <template> | parse_nodes [--quiet] --file <path>"
            );
            return ExitCode::FAILURE;
        }
    };

    // The extensions of `new Environment()` (core, escaper, optimizer), plus the sandbox extension for the
    // (deprecated) `sandbox` tag, whose visitor adds the security checks.
    let mut extensions = ExtensionSet::with_core_extensions();
    for extension in [
        Arc::new(OptimizerExtension::default()) as Arc<dyn twig::extension::Extension>,
        Arc::new(SandboxExtension::default()),
    ] {
        if let Err(error) = extensions.add_extension(extension) {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    }
    let context = ParseContext::new(extensions);

    let mut deprecations: Vec<Deprecation> = Vec::new();
    let result = context.parse_template(&Source::new(code, name), &mut deprecations);
    for deprecation in &deprecations {
        eprintln!("deprecated: {deprecation}");
    }
    match result {
        Ok(module) => {
            if !quiet {
                println!("{}", dump_node(&module));
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{}: {error}", error.class_name());
            ExitCode::FAILURE
        }
    }
}
