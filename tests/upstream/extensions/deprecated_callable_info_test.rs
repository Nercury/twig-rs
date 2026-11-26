//! Port of `TWIG/tests/DeprecatedCallableInfoTest.php`: `setType('function')` and `setName('foo')` are the type and
//! name of the `TwigFunction` carrying the info; `triggerDeprecation()` is `TwigFunction::trigger_deprecation()`.

use twig::deprecation::Deprecation;
use twig::extension::{DeprecatedCallableInfo, FunctionOptions, TwigFunction};

use super::support::check_cases;

/// `$info->setType('function'); $info->setName('foo'); $info->triggerDeprecation($file, $line);`
fn trigger(info: &DeprecatedCallableInfo, file: Option<&str>, line: Option<i64>) -> Vec<String> {
    let function = TwigFunction::new(
        "foo",
        None,
        FunctionOptions {
            deprecation_info: Some(info.clone()),
            ..Default::default()
        },
    );
    let mut deprecations: Vec<Deprecation> = Vec::new();
    function.trigger_deprecation(file, line, &mut deprecations);
    deprecations.iter().map(ToString::to_string).collect()
}

fn info(
    package: &str,
    version: &str,
    alt_name: Option<&str>,
    alt_package: Option<&str>,
    alt_version: Option<&str>,
) -> DeprecatedCallableInfo {
    DeprecatedCallableInfo {
        alt_name: alt_name.map(Into::into),
        alt_package: alt_package.map(Into::into),
        alt_version: alt_version.map(Into::into),
        ..DeprecatedCallableInfo::new(package, version)
    }
}

fn provide_tests_for_trigger_deprecation() -> Vec<(String, (&'static str, DeprecatedCallableInfo))>
{
    vec![
        (
            "Since foo/bar 1.1: Twig Function \"foo\" is deprecated in foo.twig at line 1.",
            info("foo/bar", "1.1", None, None, None),
        ),
        (
            "Since foo/bar 1.1: Twig Function \"foo\" is deprecated; use \"alt_foo\" from the \"all/bar\" package (available since version 12.10) instead in foo.twig at line 1.",
            info("foo/bar", "1.1", Some("alt_foo"), Some("all/bar"), Some("12.10")),
        ),
        (
            "Since foo/bar 1.1: Twig Function \"foo\" is deprecated; use \"alt_foo\" from the \"all/bar\" package instead in foo.twig at line 1.",
            info("foo/bar", "1.1", Some("alt_foo"), Some("all/bar"), None),
        ),
        (
            "Since foo/bar 1.1: Twig Function \"foo\" is deprecated; use \"alt_foo\" instead in foo.twig at line 1.",
            info("foo/bar", "1.1", Some("alt_foo"), None, None),
        ),
    ]
    .into_iter()
    .enumerate()
    .map(|(n, case)| (format!("#{n}"), case))
    .collect()
}

#[test]
fn test_trigger_deprecation() {
    check_cases(
        "testTriggerDeprecation",
        provide_tests_for_trigger_deprecation(),
        |(expected, info)| {
            let deprecations = trigger(info, Some("foo.twig"), Some(1));
            if deprecations == [*expected] {
                Ok(())
            } else {
                Err(format!("got {deprecations:?}"))
            }
        },
    );
}

#[test]
fn test_trigger_deprecation_without_file_or_line() {
    let info = DeprecatedCallableInfo::new("foo/bar", "1.1");
    let mut deprecations = trigger(&info, None, None);
    deprecations.extend(trigger(&info, Some("foo.twig"), None));
    assert_eq!(
        deprecations,
        [
            "Since foo/bar 1.1: Twig Function \"foo\" is deprecated.",
            "Since foo/bar 1.1: Twig Function \"foo\" is deprecated in foo.twig.",
        ]
    );
}
