use super::*;

fn parse(s: &str) -> (Time, ErrorContainer) {
    strtotime_builtin(s.as_bytes())
}

#[test]
fn simple_dates() {
    let (t, e) = parse("2021-01-31");
    assert_eq!(e.error_count(), 0);
    assert_eq!((t.y, t.m, t.d), (2021, 1, 31));
    assert_eq!(t.h, UNSET);

    let (t, e) = parse("+1 month");
    assert_eq!(e.error_count(), 0);
    assert_eq!(t.relative.m, 1);
    assert_eq!(t.have_relative, 1);
}

#[test]
fn errors() {
    let (_, e) = parse("");
    assert_eq!(e.error_messages[0].message, "Empty string");
    let (_, e) = parse("not a date");
    assert_eq!(
        e.error_messages[0].message,
        "The timezone could not be found in the database"
    );
    assert_eq!(e.error_messages[0].position, 0);
    assert_eq!(e.error_messages[0].character, b'n');
}

/// An input that is one `timestamp` token is parsed without the scanner: the time and the errors are what scanning
/// it gives.
#[test]
fn timestamps_parse_like_the_scanner() {
    let cache = TzCache::builtin();
    let mut wrapper = |name: &str, _db: &TzDb| cache.get(name);
    let mut inputs: Vec<String> = [
        "@0",
        "@-0",
        "@1",
        "@-1",
        "@00",
        "@007",
        "@-007",
        "@1700000000",
        "@-1700000000",
        " @5",
        "@5 ",
        "\t@12\n",
        "@999999999999999999",
        "@-999999999999999999",
        "@1234567890123456789",
        "@1.5",
        "@",
        "@-",
        "@--1",
        "@+1",
        "@1a",
        "@ 1",
        "@1 2",
        "@-1-",
        "x@1",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let mut n: i64 = 1;
    for _ in 0..60 {
        inputs.push(format!("@{n}"));
        inputs.push(format!("@-{n}"));
        n = n.wrapping_mul(7).wrapping_add(3) % 1_000_000_000_000_000_000;
    }
    for input in inputs {
        let fast = strtotime_with(input.as_bytes(), cache.tzdb(), &mut wrapper, true);
        let scanned = strtotime_with(input.as_bytes(), cache.tzdb(), &mut wrapper, false);
        assert_eq!(fast, scanned, "{input:?}");
    }
}
