//! Port of TWIG/tests/SourceTest.php.
//!
//! Not ported:
//! - The `getColumn(-1)` assertion (`null` for an unknown offset): twig-rs offsets are `usize`; callers pass no offset
//!   instead.
//! - `testGetPathCallsAClosureOnce`: `Source::createWithLazyPath()` (missing API: twig-rs sources have a fixed path).
//! - `testSerializingCallsTheClosure`: PHP `serialize()` of a lazy source.

use twig::Source;

#[test]
fn test_get_column() {
    let source = Source::new("foo\nbarbaz\nqux", "index");

    // first line: column is the 1-based offset
    assert_eq!(source.column(0), 1);
    assert_eq!(source.column(2), 3);

    // a "\n" closes the line; the next character starts a new line at column 1
    assert_eq!(source.column(4), 1);
    assert_eq!(source.column(7), 4);

    // "\r\n" and "\r" also close the line
    assert_eq!(Source::new("foo\r\nbar", "index").column(5), 1);
    assert_eq!(Source::new("foo\rbar", "index").column(4), 1);
}
