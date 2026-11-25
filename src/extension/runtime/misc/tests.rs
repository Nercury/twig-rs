//! The configuration of the `CoreExtension` against Twig 3.31.1 on PHP 8.5.13: `formatDate()`, `convertDate()`,
//! `modifyDate()` and `formatNumber()` of an extension configured with `setDateFormat()`, `setTimezone()` and
//! `setNumberFormat()`, compared with the oracle batch `tests/vectors/callables/misc/settings.gen.json` (results in
//! `settings.json`, written by `tests/vectors/callables/misc/misc.gen.py`), on a [`CoreSettings`] of its own (the
//! fixture harness configures the environment's settings through `Environment::core_settings()`).

use serde_json::Value as Json;
use twig_date::{DateContext, DateObject, IntervalObject, TimeZoneObject, classes};
use twig_value::object::CallArgs;
use twig_value::{Diagnostic, PhpString, Value, convert};

use super::call_method_on;
use super::dates::{convert_date, format_date, modify_date};
use super::numbers::number_format;
use super::settings::CoreSettings;
use crate::vm::Exception;

/// The value of a JSON spec: scalars, lists and the `$date`, `$tz` and `$interval` tags of the generator.
fn value(ctx: &DateContext, json: &Json) -> Value {
    match json {
        Json::Null => Value::Null,
        Json::Bool(b) => Value::Bool(*b),
        Json::Number(n) => match n.as_i64() {
            Some(i) => Value::Int(i),
            None => Value::Float(n.as_f64().expect("a number")),
        },
        Json::String(s) => Value::from(s.as_str()),
        Json::Array(items) => Value::Array(items.iter().map(|item| value(ctx, item)).collect()),
        Json::Object(fields) => {
            let (tag, arg) = fields.iter().next().expect("a tagged value");
            match tag.as_str() {
                "$date" => {
                    let parts = arg.as_array().expect("date parts");
                    let class = match parts[0].as_str() {
                        Some("DateTimeImmutable") => classes::datetime_immutable(),
                        _ => classes::datetime(),
                    };
                    let zone = parts[2]
                        .as_str()
                        .map(|zone| TimeZoneObject::construct(ctx, zone.as_bytes()).expect("zone"));
                    let time = parts[1].as_str().expect("time");
                    Value::Object(
                        DateObject::construct(ctx, class, time.as_bytes(), zone.as_ref())
                            .expect("date"),
                    )
                }
                "$tz" => Value::Object(
                    TimeZoneObject::construct(ctx, arg.as_str().expect("zone").as_bytes())
                        .expect("zone"),
                ),
                "$interval" => Value::Object(
                    IntervalObject::construct(ctx, arg.as_str().expect("interval").as_bytes())
                        .expect("interval"),
                ),
                other => panic!("unsupported tag {other}"),
            }
        }
    }
}

/// The class and message of an exception, as the oracle reports them.
fn describe(exception: &Exception) -> (String, String) {
    match (exception.twig_error(), exception.php_error()) {
        (Some(error), _) => (error.kind().class_name().to_owned(), error.message()),
        (_, Some(error)) => (
            error.class().name().to_owned(),
            error.message_lossy().into_owned(),
        ),
        _ => unreachable!(),
    }
}

/// Runs one case: `Ok(string)` or `Err((class, message))`, plus the diagnostics.
fn run(case: &Json) -> (Result<String, (String, String)>, Vec<Diagnostic>) {
    let ctx = DateContext::new();
    let mut diag: Vec<Diagnostic> = Vec::new();
    if let Some(zone) = case.get("timezone").and_then(Json::as_str) {
        assert!(
            ctx.default_timezone_set(zone.as_bytes(), &mut diag)
                .unwrap()
        );
    }
    let settings = CoreSettings::new();
    let result = (|| -> Result<Value, Exception> {
        if let Some(formats) = case.get("date_format") {
            let formats = formats.as_array().expect("formats");
            let format = |json: &Json| (!json.is_null()).then(|| value(&ctx, json));
            settings.set_date_format(format(&formats[0]), format(&formats[1]));
        }
        if let Some(defaults) = case.get("number_format") {
            let defaults = defaults.as_array().expect("defaults");
            settings.set_number_format(
                value(&ctx, &defaults[0]),
                value(&ctx, &defaults[1]),
                value(&ctx, &defaults[2]),
            );
        }
        if let Some(timezone) = case.get("core_timezone") {
            settings.set_timezone(&ctx, &value(&ctx, timezone), &mut diag)?;
        }
        let args: Vec<Value> = case["args"]
            .as_array()
            .expect("args")
            .iter()
            .map(|arg| value(&ctx, arg))
            .collect();
        let arg = |i: usize| args.get(i).cloned().unwrap_or_default();
        let format = Value::from("Y-m-d H:i:s T e");
        Ok(match case["call"].as_str().expect("call") {
            "formatDate" => format_date(&settings, &ctx, &arg(0), &arg(1), &arg(2), &mut diag)?,
            "convertDate" => {
                let date = convert_date(&settings, &ctx, &arg(0), &arg(1), &mut diag)?;
                call_method_on(&date, "format", vec![format], &mut diag)?
            }
            "modifyDate" => {
                let date = modify_date(&settings, &ctx, &arg(0), &arg(1), &mut diag)?;
                call_method_on(&date, "format", vec![format], &mut diag)?
            }
            "formatNumber" => {
                number_format(&settings, &arg(0), &arg(1), &arg(2), &arg(3), &mut diag)?
            }
            other => panic!("unknown call {other}"),
        })
    })();
    let result = match result {
        Ok(value) => Ok(convert::to_string(&value, &mut Vec::<Diagnostic>::new())
            .unwrap_or_else(|_| PhpString::from("?"))
            .to_string_lossy()
            .into_owned()),
        Err(exception) => Err(describe(&exception)),
    };
    (result, diag)
}

#[test]
fn settings_match_the_oracle() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/vectors/callables/misc/");
    let read = |file: &str| -> Json {
        let text = std::fs::read_to_string(format!("{root}{file}")).expect("vector file");
        serde_json::from_str(&text).expect("JSON")
    };
    let batch = read("settings.gen.json");
    let results = read("settings.json");
    let cases = batch["cases"].as_array().expect("cases");
    let results = results.as_array().expect("results");
    assert_eq!(cases.len(), results.len());
    let mut failures = Vec::new();
    for (case, expected) in cases.iter().zip(results) {
        let id = case["id"].as_str().unwrap_or_default();
        assert_eq!(expected["id"].as_str(), Some(id));
        let (result, diag) = run(case);
        let want = if expected["ok"] == Json::Bool(true) {
            Ok(expected["string"].as_str().unwrap_or_default().to_owned())
        } else {
            Err((
                expected["error"]["class"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
                expected["error"]["message"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
            ))
        };
        let diag: Vec<String> = diag
            .iter()
            .map(|d| d.message.to_string_lossy().into_owned())
            .collect();
        let want_diag: Vec<String> = expected["diagnostics"]
            .as_array()
            .map(|list| {
                list.iter()
                    .map(|d| d["message"].as_str().unwrap_or_default().to_owned())
                    .collect()
            })
            .unwrap_or_default();
        if result != want || diag != want_diag {
            failures.push(format!(
                "{id}: {} -> {result:?} {diag:?}, expected {want:?} {want_diag:?}",
                case["php_code"].as_str().unwrap_or_default()
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} settings cases differ from the oracle:\n{}",
        failures.len(),
        cases.len(),
        failures.join("\n")
    );
}

#[test]
fn settings_are_shared_by_clones() {
    let settings = CoreSettings::new();
    let clone = settings.clone();
    clone.set_date_format(Some(Value::from("Y")), None);
    clone.set_number_format(Value::Int(2), Value::from(","), Value::from("."));
    assert_eq!(settings.date_format()[0], Value::from("Y"));
    assert_eq!(settings.date_format()[1], Value::from("%d days"));
    assert_eq!(settings.number_format()[0], Value::Int(2));
    let ctx = DateContext::new();
    let mut diag: Vec<Diagnostic> = Vec::new();
    // the default timezone is remembered at first use
    let first = settings.timezone(&ctx).unwrap();
    assert!(ctx.default_timezone_set(b"Asia/Tokyo", &mut diag).unwrap());
    let again = settings.timezone(&ctx).unwrap();
    assert!(twig_value::ObjectRef::ptr_eq(&first, &again));
    let name = again
        .call_method("getName", CallArgs::new(), &mut diag)
        .unwrap();
    assert_eq!(name, Value::from("UTC"));
}
