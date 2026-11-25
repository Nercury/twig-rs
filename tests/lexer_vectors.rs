//! Oracle vectors for the lexer and `TokenStream::expect()`: `tests/vectors/lexer.json` (hand-picked cases) and
//! `tests/vectors/lexer_random.json` (random templates).
//!
//! The vectors are Twig 3.31.1's own tokenization results (PHP 8.5.13 oracle). Regenerate them from the repository
//! root with (Git Bash on Windows needs `MSYS_NO_PATHCONV=1`):
//!
//! ```text
//! MSYS_NO_PATHCONV=1 docker run --rm -i twig-oracle:3.31.1 php < tests/vectors/lexer.gen.php > tests/vectors/lexer.json
//! MSYS_NO_PATHCONV=1 docker run --rm -i twig-oracle:3.31.1 php < tests/vectors/lexer_random.gen.php > tests/vectors/lexer_random.json
//! ```
//!
//! For a larger ad-hoc differential run, generate another random batch (`docker run -e SEED=7 -e COUNT=20000 ...`
//! with `lexer_random.gen.php`) and pass it to the ignored test `lexer_matches_twig_on_external_vectors`.

mod common;

use common::{Json, show};
use twig::Source;
use twig::deprecation::Deprecation;
use twig::lexer::{DEFAULT_OPERATORS, Lexer, LexerOptions, Token, TokenType, TokenValue};

/// Builds the lexer options of a case from Twig's option array. `whitespace_line_chars` is a PCRE character class
/// body in Twig and a literal set in twig-rs; the vectors only use class bodies without escapes, where both agree.
fn options(case: &Json) -> LexerOptions {
    let mut options = LexerOptions::default();
    let Some(overrides) = case.get("options") else {
        return options;
    };
    for (key, value) in overrides.as_object() {
        let pair = || -> [Vec<u8>; 2] {
            let items = value.as_array();
            [items[0].as_bytes(), items[1].as_bytes()]
        };
        match key.as_str() {
            "tag_comment" => options.tag_comment = pair(),
            "tag_block" => options.tag_block = pair(),
            "tag_variable" => options.tag_variable = pair(),
            "interpolation" => options.interpolation = pair(),
            "whitespace_trim" => options.whitespace_trim = value.as_bytes(),
            "whitespace_line_trim" => options.whitespace_line_trim = value.as_bytes(),
            "whitespace_line_chars" => options.whitespace_line_chars = value.as_bytes(),
            other => panic!("unknown lexer option {other}"),
        }
    }
    options
}

fn expected_value(json: &Json) -> TokenValue {
    if let Some(int) = json.get("int") {
        return TokenValue::Int(int.as_str().and_then(|s| s.parse().ok()).expect("int"));
    }
    if let Some(bits) = json.get("bits") {
        let bits = u64::from_str_radix(bits.as_str().expect("bits"), 16).expect("hex bits");
        return TokenValue::Float(f64::from_bits(bits));
    }
    TokenValue::Bytes(json.as_bytes())
}

/// Compares bytes with an expected byte string, which long strings encode as `{"len": n, "crc32": "hex"}`.
fn bytes_match(actual: &[u8], expected: &Json) -> bool {
    match (expected.get("len"), expected.get("crc32")) {
        (Some(len), Some(crc)) => {
            Some(actual.len() as i64) == len.as_i64()
                && u32::from_str_radix(crc.as_str().expect("crc32"), 16).ok()
                    == Some(common::crc32(actual))
        }
        _ => actual == expected.as_bytes(),
    }
}

fn check_token(actual: &Token, expected: &Json) -> Result<(), String> {
    let fields = expected.as_array();
    let ty = TokenType::from_int(fields[0].as_i64().expect("type")).expect("known type");
    let line = fields[2].as_i64().expect("line");
    let offset = fields[3].as_i64().map(|o| o as usize);

    let value_matches = match (actual.value(), fields[1].get("len")) {
        (TokenValue::Bytes(bytes), Some(_)) => bytes_match(bytes, &fields[1]),
        // Compare floats bit for bit (INF, -0.0, ...).
        (TokenValue::Float(a), _) if fields[1].get("bits").is_some() => {
            matches!(expected_value(&fields[1]), TokenValue::Float(b) if a.to_bits() == b.to_bits())
        }
        (value, _) => fields[1].get("len").is_none() && *value == expected_value(&fields[1]),
    };
    let documentation_matches = match (actual.documentation(), fields[4].is_null()) {
        (None, true) => true,
        (Some(documentation), false) => bytes_match(documentation, &fields[4]),
        _ => false,
    };
    if actual.ty() != ty
        || !value_matches
        || actual.line() != line
        || actual.offset() != offset
        || !documentation_matches
    {
        return Err(format!(
            "token mismatch: got {:?} {:?} line {} offset {:?} doc {:?}, expected {expected:?}",
            actual.ty(),
            actual.value(),
            actual.line(),
            actual.offset(),
            actual.documentation().map(show),
        ));
    }
    // PHP's string conversion of numbers (precision 14 for floats), used in error messages.
    if let Some(php_string) = fields[1].get("float") {
        let expected = php_string.as_str().expect("float string");
        if actual.value().to_string() != expected {
            return Err(format!(
                "float {:?} formats as {}, PHP gives {expected}",
                actual.value(),
                actual.value()
            ));
        }
    }
    Ok(())
}

fn check_error(actual: &twig::Error, expected: &Json) -> Result<(), String> {
    let class = expected.field("class").as_str().expect("class");
    let mut problems = Vec::new();
    if actual.kind().class_name() != class {
        problems.push(format!("class {} != {class}", actual.kind().class_name()));
    }
    if let Some(raw) = expected.get("raw")
        && actual.raw_message() != raw.as_lossy_string()
    {
        problems.push(format!(
            "raw message {:?} != {:?}",
            actual.raw_message(),
            raw.as_lossy_string()
        ));
    }
    if let Some(line) = expected.get("line")
        && Some(actual.template_line()) != line.as_i64()
    {
        problems.push(format!("line {} != {line:?}", actual.template_line()));
    }
    if let Some(column) = expected.get("column") {
        let expected_column = column.as_i64().map(|c| c as usize);
        if actual.template_column() != expected_column {
            problems.push(format!(
                "column {:?} != {expected_column:?}",
                actual.template_column()
            ));
        }
    }
    let message = expected.field("message").as_lossy_string();
    if actual.to_string() != message {
        problems.push(format!("message {:?} != {message:?}", actual.to_string()));
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.join("; "))
    }
}

type LexerCache = Vec<(LexerOptions, Vec<Vec<u8>>, Lexer)>;

fn check_case(case: &Json, cache: &mut LexerCache) -> Result<(), String> {
    let code = if let Some(source) = case.get("source") {
        source.as_bytes()
    } else if let Some(parts) = case.get("source_parts") {
        let mut code = Vec::new();
        for part in parts.as_array() {
            let part = part.as_array();
            code.extend(
                part[0]
                    .as_bytes()
                    .repeat(part[1].as_i64().expect("repeat count") as usize),
            );
        }
        code
    } else {
        common::base64_decode(case.field("source_b64").as_str().expect("base64 source"))
    };
    let name = case
        .get("name")
        .map_or_else(|| "index".to_owned(), Json::as_lossy_string);
    let options = options(case);
    let operators: Vec<Vec<u8>> = match case.get("operators") {
        Some(operators) => operators.as_array().iter().map(Json::as_bytes).collect(),
        None => DEFAULT_OPERATORS
            .iter()
            .map(|op| op.as_bytes().to_vec())
            .collect(),
    };
    // Building a lexer compiles its patterns: reuse lexers across cases with the same configuration.
    let index = match cache
        .iter()
        .position(|(o, ops, _)| *o == options && *ops == operators)
    {
        Some(index) => index,
        None => {
            let lexer = Lexer::new(options.clone(), &operators);
            cache.push((options, operators, lexer));
            cache.len() - 1
        }
    };
    let lexer = &cache[index].2;
    let source = Source::new(code, name);
    let mut deprecations: Vec<Deprecation> = Vec::new();
    let result = lexer.tokenize(&source, &mut deprecations);

    match (result, case.get("tokens"), case.get("error")) {
        (Ok(stream), Some(expected), None) => {
            let tokens = stream.tokens();
            let expected = expected.as_array();
            if tokens.len() != expected.len() {
                let got: Vec<String> = tokens
                    .iter()
                    .map(|t| format!("{:?}({:?})", t.ty(), t.value()))
                    .collect();
                return Err(format!(
                    "{} tokens instead of {}: {}",
                    tokens.len(),
                    expected.len(),
                    got.join(" ")
                ));
            }
            for (i, (token, expected)) in tokens.iter().zip(expected).enumerate() {
                check_token(token, expected).map_err(|e| format!("token #{i}: {e}"))?;
            }
            if let Some(expect) = case.get("expect") {
                check_expect(stream, expect)?;
            }
        }
        (Err(error), None, Some(expected)) => check_error(&error, expected)?,
        (Ok(stream), _, Some(_)) => return Err(format!("tokenized without error: {stream}")),
        (Err(error), _, _) => return Err(format!("unexpected error: {error}")),
        (Ok(_), None, None) => return Err("case has neither tokens nor error".to_owned()),
    }

    let expected: Vec<String> = case
        .field("deprecations")
        .as_array()
        .iter()
        .map(Json::as_lossy_string)
        .collect();
    let actual: Vec<String> = deprecations.iter().map(ToString::to_string).collect();
    if actual != expected {
        return Err(format!("deprecations {actual:?} != {expected:?}"));
    }
    Ok(())
}

fn check_expect(mut stream: twig::lexer::TokenStream, expect: &Json) -> Result<(), String> {
    for _ in 0..expect.field("skip").as_i64().expect("skip") {
        stream.next().map_err(|e| format!("next() failed: {e}"))?;
    }
    let ty = TokenType::from_int(expect.field("type").as_i64().expect("type")).expect("known type");
    let value = expect.field("value").as_str();
    let message = expect.field("message").as_str();
    match (
        stream.expect(ty, value, message),
        expect.get("token"),
        expect.get("error"),
    ) {
        (Ok(token), Some(expected), None) => {
            check_token(&token, expected).map_err(|e| format!("expect(): {e}"))
        }
        (Err(error), None, Some(expected)) => {
            check_error(&error, expected).map_err(|e| format!("expect(): {e}"))
        }
        (Ok(token), _, _) => Err(format!("expect() returned {token} instead of failing")),
        (Err(error), _, _) => Err(format!("expect() failed: {error}")),
    }
}

fn check_vector_file(file: &str, minimum_cases: usize) {
    check_vectors(file, &Json::load_vectors(file), minimum_cases);
}

fn check_vectors(file: &str, cases: &Json, minimum_cases: usize) {
    let cases = cases.as_array();
    assert!(cases.len() >= minimum_cases, "{file} looks truncated");
    let mut cache = LexerCache::new();
    let failures: Vec<String> = cases
        .iter()
        .filter_map(|case| {
            let id = case.field("id").as_str().expect("id");
            check_case(case, &mut cache)
                .err()
                .map(|error| format!("{id}: {error}"))
        })
        .collect();
    assert!(
        failures.is_empty(),
        "{file}: {} of {} cases differ:\n{}",
        failures.len(),
        cases.len(),
        failures.join("\n")
    );
}

/// Hand-picked cases: `tests/vectors/lexer.gen.php`.
#[test]
fn lexer_matches_twig() {
    check_vector_file("lexer.json", 450);
}

/// Random templates: `tests/vectors/lexer_random.gen.php`.
#[test]
fn lexer_matches_twig_on_random_templates() {
    check_vector_file("lexer_random.json", 3000);
}

/// Ad-hoc differential runs against any oracle batch in the format above, for example a larger random batch
/// (`docker run --rm -i -e SEED=7 -e COUNT=20000 twig-oracle:3.31.1 php < tests/vectors/lexer_random.gen.php`):
///
/// ```text
/// TWIG_LEXER_VECTORS=/path/to/batch.json cargo +stable test -p twig --test lexer_vectors -- --ignored
/// ```
#[test]
#[ignore = "needs an oracle batch in TWIG_LEXER_VECTORS"]
fn lexer_matches_twig_on_external_vectors() {
    let path = std::env::var("TWIG_LEXER_VECTORS").expect("TWIG_LEXER_VECTORS is not set");
    check_vectors(&path, &Json::load_path(&path), 1);
}
