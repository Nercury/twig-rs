//! Tokenizes `templates/fos_login.html.twig` with the Twig lexer and prints the tokens.
//!
//! ```bash
//! cargo run --example lex_tokens [path/to/template.twig]
//! ```

use std::path::PathBuf;
use std::process::ExitCode;

use twig::Source;
use twig::deprecation::Deprecation;
use twig::lexer::Lexer;

fn main() -> ExitCode {
    let path = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("templates/fos_login.html.twig")
        });
    let code = match std::fs::read(&path) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("cannot read {}: {error}", path.display());
            return ExitCode::FAILURE;
        }
    };
    let name = path
        .file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
    let source = Source::with_path(code, name, path.display().to_string());

    let mut deprecations: Vec<Deprecation> = Vec::new();
    let stream = match Lexer::default().tokenize(&source, &mut deprecations) {
        Ok(stream) => stream,
        Err(error) => {
            eprintln!("{}: {error}", error.kind().class_name());
            return ExitCode::FAILURE;
        }
    };
    for token in stream.tokens() {
        let offset = token
            .offset()
            .map_or_else(|| "-".to_owned(), |offset| offset.to_string());
        println!(
            "{:>4}:{:<6} {:<24} {:?}",
            token.line(),
            offset,
            token.ty().name(),
            token.value()
        );
    }
    for deprecation in &deprecations {
        eprintln!("deprecation: {deprecation}");
    }
    ExitCode::SUCCESS
}
