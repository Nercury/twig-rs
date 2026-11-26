//! Twig 3.31.1's `tests/Node/TextTest.php` (`provideTests()` compares generated PHP and is not ported).

use twig::ast::{AttrValue, Node, NodeKind, TextNode};

/// `testConstructor()`.
#[test]
fn constructor() {
    let node = Node::text("foo", 1);
    let NodeKind::Text(text) = &node.kind else {
        panic!("expected a TextNode, got {node:?}");
    };
    assert_eq!(text.data, b"foo");
    assert_eq!(node.line, 1);
    let attributes: Vec<(String, AttrValue)> = node
        .attributes()
        .into_iter()
        .map(|(name, value)| (name.into_owned(), value))
        .collect();
    assert_eq!(attributes, [("data".to_owned(), AttrValue::string("foo"))]);
}

/// `testIsBlank()` with `getIsBlankData()`: blank, also behind a UTF-8 byte order mark.
#[test]
fn is_blank() {
    for blank in [" ", "\t", "\n", "\n\t\n   "] {
        let text = TextNode {
            data: blank.as_bytes().to_vec(),
        };
        assert!(text.is_blank(), "{blank:?}");
        let mut data = b"\xEF\xBB\xBF".to_vec();
        data.extend_from_slice(blank.as_bytes());
        assert!(TextNode { data }.is_blank(), "BOM + {blank:?}");
    }
}
