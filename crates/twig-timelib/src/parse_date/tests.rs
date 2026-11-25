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
