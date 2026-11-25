//! Oracle vectors for `Token::test()`: `tests/vectors/token_test.json` (PHP's loose `==` between token values and
//! test values, and the type matching with its OPERATOR/PUNCTUATION backward compatibility).
//!
//! Regenerate from the repository root with (Git Bash on Windows needs `MSYS_NO_PATHCONV=1`):
//!
//! ```text
//! MSYS_NO_PATHCONV=1 docker run --rm -i twig-oracle:3.31.1 php < tests/vectors/token_test.gen.php > tests/vectors/token_test.json
//! ```

mod common;

use common::{Json, show};
use twig::lexer::{Token, TokenType, TokenValue, TokenValues};

fn parse_token(json: &Json) -> Token {
    let fields = json.as_array();
    let ty = TokenType::from_int(fields[0].as_i64().expect("type")).expect("known type");
    let value = &fields[1];
    let value = if let Some(int) = value.get("int") {
        TokenValue::Int(int.as_str().and_then(|s| s.parse().ok()).expect("int"))
    } else if let Some(bits) = value.get("bits") {
        let bits = u64::from_str_radix(bits.as_str().expect("bits"), 16).expect("hex bits");
        TokenValue::Float(f64::from_bits(bits))
    } else {
        TokenValue::Bytes(value.as_bytes())
    };
    Token::new(ty, value, 1)
}

#[test]
fn loose_comparison_matches_php() {
    let vectors = Json::load_vectors("token_test.json");
    let loose = vectors.field("loose");
    let strings: Vec<Vec<u8>> = loose
        .field("strings")
        .as_array()
        .iter()
        .map(Json::as_bytes)
        .collect();
    let results = loose.field("results").as_array();
    let tokens = loose.field("tokens").as_array();
    assert!(
        tokens.len() >= 130 && strings.len() >= 100,
        "the vector file looks truncated"
    );
    let mut failures = Vec::new();
    for (token, expected) in tokens.iter().zip(results) {
        let token = parse_token(token);
        let expected = expected.as_str().expect("result bits").as_bytes();
        for (string, &bit) in strings.iter().zip(expected) {
            let actual = token.test_value(token.ty(), string.as_slice());
            if actual != (bit == b'1') {
                failures.push(format!(
                    "{:?} == {:?}: got {actual}",
                    token.value(),
                    show(string)
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} mismatches:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn type_matching_matches_php() {
    let vectors = Json::load_vectors("token_test.json");
    let bc = vectors.field("bc");
    let tests = bc.field("tests").as_array();
    let results = bc.field("results").as_array();
    let mut failures = Vec::new();
    for (token, expected) in bc.field("tokens").as_array().iter().zip(results) {
        let token = parse_token(token);
        let expected = expected.as_str().expect("result bits").as_bytes();
        for (test, &bit) in tests.iter().zip(expected) {
            let fields = test.as_array();
            let ty = TokenType::from_int(fields[0].as_i64().expect("type")).expect("known type");
            let actual = match &fields[1] {
                Json::Null => token.test(ty),
                Json::Array(values) => {
                    let values: Vec<&str> = values
                        .iter()
                        .map(|v| v.as_str().expect("UTF-8 test value"))
                        .collect();
                    token.test_value(ty, TokenValues::AnyOf(&values))
                }
                value => token.test_value(ty, value.as_bytes().as_slice()),
            };
            if actual != (bit == b'1') {
                failures.push(format!(
                    "{:?}({:?}).test({ty:?}, {:?}): got {actual}",
                    token.ty(),
                    token.value(),
                    fields[1]
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} mismatches:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
