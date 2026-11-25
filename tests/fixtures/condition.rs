//! `--CONDITION--` sections. `doIntegrationTest()` runs `eval('$ret = '.$condition.';')` and skips the fixture
//! (`markTestSkipped($condition)`) when the result is falsy (TWIG/src/Test/IntegrationTestCase.php). The harness
//! cannot evaluate PHP, so it knows the verdict of every condition the fixtures use on the target, PHP 8.5.13 CLI
//! without Xdebug (`reference/target.md`, `reference/twig/test-suite.md` §2.6), and fails on any other condition.

/// `PHP_VERSION_ID` of the target.
const PHP_VERSION_ID: i64 = 80513;

/// Whether the Xdebug extension is loaded on the target.
const XDEBUG_LOADED: bool = false;

/// The verdict of a fixture's condition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// No condition (`if ($condition)` is false for `""` and `"0"`), or the condition is true: the fixture runs.
    Run,
    /// The condition is false: PHPUnit marks the fixture skipped with the condition as the message.
    Skip(String),
}

/// The verdict of `$condition` (the raw `--CONDITION--` text) on PHP 8.5.13.
///
/// # Errors
///
/// A condition the harness does not know.
pub fn evaluate(condition: &[u8]) -> Result<Verdict, String> {
    if condition.is_empty() || condition == b"0" {
        return Ok(Verdict::Run);
    }
    let expression = String::from_utf8_lossy(condition);
    // whitespace around the expression does not matter to eval()
    let expression = expression.trim_matches([' ', '\t', '\n', '\r', '\x0B', '\x0C']);
    let result = match expression {
        // functions/dump.test
        "!extension_loaded('xdebug')" => !XDEBUG_LOADED,
        // functions/dump_array.test; the right operand is not evaluated when Xdebug is absent
        "!extension_loaded('xdebug') || (($r = new \\ReflectionExtension('xdebug')) && version_compare($r->getVersion(), '2.2-dev', '<'))" => {
            !XDEBUG_LOADED
        }
        // functions/enum/*.test, functions/enum_cases/*.test, functions/template_from_string_error.test
        "\\PHP_VERSION_ID >= 80100" | "PHP_VERSION_ID >= 80100" => PHP_VERSION_ID >= 80100,
        // functions/template_from_string_error_php80.test
        "PHP_VERSION_ID < 80100" => PHP_VERSION_ID < 80100,
        // regression/simple_xml_element.test: version_compare('8.5.13', '8.0', '<') compares 8 = 8, then 5 > 0
        "version_compare(phpversion(), '8.0', '<')" => false,
        _ => {
            return Err(format!(
                "unknown --CONDITION-- {expression:?}: add its verdict on PHP 8.5.13 to tests/fixtures/condition.rs"
            ));
        }
    };
    Ok(if result {
        Verdict::Run
    } else {
        Verdict::Skip(String::from_utf8_lossy(condition).into_owned())
    })
}
