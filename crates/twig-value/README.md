# twig-value

PHP 8.5's value model in Rust: `null`, `bool`, `int`, `float`, byte strings, ordered copy-on-write arrays with PHP's
key rules, and objects, with PHP's casts, operators, comparisons, float formatting, `var_dump()` and `var_export()`,
and the PHP builtin functions Twig calls (strings, `sprintf()`, `number_format()`, `round()`, arrays and sorting,
`json_encode()`, `htmlspecialchars()`, `iconv()`, ...). Results, warnings and error messages are those of PHP 8.5.13
(64-bit, default ini: `precision=14`, `serialize_precision=-1`), which oracle-generated test vectors check.

It is the value layer of [twig-rs](https://github.com/Nercury/twig-rs), a Rust implementation of Twig that renders
templates exactly like Twig 3.31.1 on PHP 8.5.13: template contexts are arrays of this crate's values, and Rust types
become PHP objects by implementing its `Object` trait. The [`twig`](https://crates.io/crates/twig) crate re-exports it
as `twig::value`.

```rust
use twig_value::{Array, Diagnostic, PhpError, Value, compare, convert, dump, ops};

fn main() -> Result<(), PhpError> {
    // PHP's warnings, notices and deprecations go to a sink instead of being printed
    let mut diagnostics: Vec<Diagnostic> = Vec::new();

    // `"1e1" == "10"` compares two numeric strings as numbers
    assert!(compare::equals(&Value::from("1e1"), &Value::from("10"), &mut diagnostics)?);

    // `echo 0.1 + 0.2` prints 14 significant digits, `var_export()` the shortest string that reads back exactly
    let sum = ops::add(&Value::Float(0.1), &Value::Float(0.2), &mut diagnostics)?;
    assert_eq!(convert::to_string(&sum, &mut diagnostics)?, "0.3");
    assert_eq!(dump::var_export(&sum, &mut diagnostics)?, "0.30000000000000004");

    // the key "1" is the integer key 1, as in a PHP array
    let mut array = Array::new();
    array.insert("1", "a");
    array.insert(1, "b");
    let array = Value::from(array);
    assert_eq!(dump::var_export(&array, &mut diagnostics)?, "array (\n  1 => 'b',\n)");

    assert_eq!(convert::to_string(&array, &mut diagnostics)?, "Array");
    assert_eq!(diagnostics[0].to_string(), "Warning: Array to string conversion");
    Ok(())
}
```

The [API documentation](https://docs.rs/twig-value) maps the crate's modules to the PHP behaviour they reproduce.

## Licence

twig-rs is distributed under the BSD 3-Clause licence. This crate also contains code ported from php-src (PHP License
3.01, Zend Engine License 2.00) and smaller third-party parts under the MIT, ISC, Zlib, Apache-1.0, dtoa and Unicode-3.0
licences, so its licence is `BSD-3-Clause AND PHP-3.01 AND Zend-2.0 AND MIT AND ISC AND Zlib AND Apache-1.0 AND dtoa
AND Unicode-3.0`. The `LICENSE` and `NOTICE` files that come with the crate name each part and hold the licence texts.
