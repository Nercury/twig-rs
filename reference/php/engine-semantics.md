# PHP 8.5.13 engine-level semantics — reference for the twig-rs port

Scope: the Zend-engine behaviours a faithful Rust port of Twig 3.31.1 depends on: value casts, numeric strings,
float formatting, arithmetic, comparison, arrays (zend_hash), string conversion, var_dump/var_export.
Everything below was read from php-src 8.5.13 and **verified in Docker** (`php:8.5-cli`, PHP 8.5.13) unless it is
explicitly marked **UNVERIFIED**. Source pointers are `file:line` relative to `ext/php-src-8.5.13/`.

Resolved since this survey: `pow`/`**` bit-exactness (§6.6, §14) is verified. glibc 2.41 on the oracle's FMA+AVX2 CPU
uses `__ieee754_pow_fma`; `twig-value::math::pow` ports it, including GCC's fused multiply-adds, with 0 mismatches on
2.05M oracle cases. The float-formatting recipes of §5 are implemented and vector-tested in `twig-value::float`.

---

## Table of contents

0. [Environment, conventions, and how Twig reaches these semantics](#0-environment-conventions-and-how-twig-reaches-these-semantics)
1. [Top porting gotchas (summary)](#1-top-porting-gotchas-summary)
2. [Value model](#2-value-model)
3. [Casts: to bool / int / float / array / object](#3-casts-to-bool--int--float--array--object)
4. [Numeric strings (`is_numeric_string`) and `zend_strtod`](#4-numeric-strings-is_numeric_string-and-zend_strtod)
5. [Float to string: `precision` vs `serialize_precision`](#5-float-to-string-precision-vs-serialize_precision)
6. [Arithmetic and other operators](#6-arithmetic-and-other-operators)
7. [Comparison operators](#7-comparison-operators)
8. [Arrays (zend_hash), foreach, string offsets, count](#8-arrays-zend_hash-foreach-string-offsets-count)
9. [String conversion of every type](#9-string-conversion-of-every-type)
10. [var_dump / var_export / print_r output formats](#10-var_dump--var_export--print_r-output-formats)
11. [Coercive-mode parameter passing (internal and user functions)](#11-coercive-mode-parameter-passing-internal-and-user-functions)
12. [Diagnostic message catalogue (verbatim)](#12-diagnostic-message-catalogue-verbatim)
13. [Verification artefacts and Rust validation harnesses](#13-verification-artefacts-and-rust-validation-harnesses)
14. [UNVERIFIED items, risks, and things outside this scope](#14-unverified-items-risks-and-things-outside-this-scope)

---

## 0. Environment, conventions, and how Twig reaches these semantics

### 0.1 Oracle environment (verified)

| Property | Value | Evidence |
|---|---|---|
| PHP | 8.5.13 (cli) NTS, Zend Engine v4.5.13 | `php -v` |
| Platform | Linux x86_64, Debian 13.8, glibc 2.41 | `ldd --version`, `/etc/debian_version` |
| `PHP_INT_SIZE` | 8 (zend_long = int64) | `var_dump(PHP_INT_SIZE)` |
| php.ini | none loaded (`php_ini_loaded_file() === false`) → built-in defaults | |
| `precision` | `"14"` | `main/main.c:893` |
| `serialize_precision` | `"-1"` | `main/main.c:847` |
| `error_reporting` | `E_ALL` (= 30719 in 8.5) | `error_reporting()` |
| `display_errors` | `"1"` (CLI prints to stdout) | `main/main.c:823` |

`precision` and `serialize_precision` accept any integer `>= -1` (`main/main.c:305-328`); `-1` selects the
"shortest round-trip" algorithm (dtoa mode 0), `0` is treated as `1` (see §5).

### 0.2 How Twig 3.31.1 reaches the engine

* On PHP >= 8.0 Twig compiles binary operators **directly to PHP operators** (e.g.
  `src/Node/Expression/Binary/EqualBinary.php` falls through to `AbstractBinary::compile()` when
  `PHP_VERSION_ID >= 80000`). Mapping: `+ - * / % **` → same; `~` → `.`; `== != < > <= >= <=>` → same;
  `same as` → `===`; `b-and`/`b-or`/`b-xor` → `& | ^`; `//` → `(int) floor(a / b)` (`FloorDivBinary`);
  `..` → `range(a, b)` (`RangeBinary`); unary `-`/`+` → PHP unary `-`/`+`; `not` → `!`.
* `{{ expr }}` compiles to `yield (string) expr;` (`src/Node/PrintNode.php`) — i.e. the `(string)` cast rules of §9
  (floats use `precision`=14, arrays → `"Array"` + warning, objects need `__toString`).
* No file in Twig's `src/` declares `strict_types` → **every call from Twig code into PHP functions is coercive
  ("weak") mode** (§11).
* Float literals in templates are compiled with `var_export()` at `serialize_precision=-1`
  (`src/Compiler.php:165-171`), so a literal round-trips exactly.
* `dump()` uses `var_dump()` (`src/Extension/DebugExtension.php:55-57`); Twig's test fixture
  `tests/Fixtures/functions/dump_array.test` asserts exact `var_dump` text (§10).
* Many Twig error messages embed `get_debug_type()` output (§2.3).

### 0.3 Conventions used in this document

* `W:` = `E_WARNING`, `N:` = `E_NOTICE`, `D:` = `E_DEPRECATED`; `throws Cls("msg")` = exception thrown.
* "verified" outputs were produced by the scripts listed in §13 (an error handler prints `{W: msg}` etc. before the
  value; exceptions are printed as `!Class: msg`).
* Uncaught diagnostics are displayed by `php_error_cb` (`main/main.c:1517`) with format
  `"%s\n%s: %s in %s on line %d%s%s\n%s"` → in CLI (`html_errors` off, no prepend/append strings):
  `"\nWarning: <message> in <file> on line <n>\n"`. The type words are `"Fatal error"`, `"Recoverable fatal error"`,
  `"Warning"`, `"Parse error"`, `"Notice"`, `"Deprecated"` (`main/main.c:1444-1473`). Verified example:
  ```
  (int): 
  Warning: The float 1.0E+20 is not representable as an int, cast occurred in /t/t1.php on line 5
  int(7766279631452241920)
  ```

---

## 1. Top porting gotchas (summary)

Each item is verified; details in the referenced section.

1. **Echo/concat of floats uses 14 significant digits** with round-half-even on the exact binary value, trailing
   zeros stripped, and switches to `d.dddE+X` when the (rounded) decimal exponent is `>= 14` or `<= -5`:
   `0.1+0.2` → `0.3`, `99999999999999.0` → `99999999999999`, `1e14` → `1.0E+14`, `0.0001` → `0.0001`,
   `0.00001` → `1.0E-5`, `100.0` → `100`, `-0.0` → `-0`, `123456789012345.0` → `1.2345678901234E+14`
   (exact tie → even). var_dump/var_export use exponent thresholds `>= 17` / `<= -5`. (§5)
2. **dtoa "small integer" quirk**: an integral float `< 1e15` whose dropped digits are exactly `5` and whose last
   kept digit is even keeps its trailing zeros: `123456789012305.0` → `1.2345678901230E+14`,
   `900000000000005.0` → `9.0000000000000E+14`, but `100000000000001.0` → `1.0E+14`. (§5.4)
3. **var_dump / var_export / json_encode use the shortest round-trip repr** (dtoa mode 0) with
   **ties-to-even** between equally short candidates — Rust's `{}`/`{:e}` breaks those ties upward
   (`662936471232937.25` → PHP `662936471232937.2`, Rust `…937.3`; 2,023 of the 152,127 generated test doubles,
   ≈1.3 %, were affected). (§5.5)
4. **Float→int (PHP 8.5)**: NaN/±Inf → `0`, out-of-range → wrap modulo 2^64, both with
   `W: The float %s is not representable as an int, cast occurred`. String→int **saturates silently**
   (`(int)"1e100"` → `PHP_INT_MAX`). (§3.2)
5. **Implicit int contexts** (`%`, `|`, `&`, `^`, `<<`, `>>`, array keys, int parameters) emit
   `D: Implicit conversion from float 1.5 to int loses precision` / `…float-string "1.5"…`. Explicit `(int)` never
   does. (§3.2.4)
6. **Non-numeric strings in arithmetic throw** `TypeError("Unsupported operand types: string + int")`;
   leading-numeric strings (`"12abc"`) work with `W: A non-numeric value encountered`; `""` is non-numeric. (§6.1)
7. **Unary minus compiles to `x * -1`**, so `-"abc"` throws `Unsupported operand types: string * int`. (§6.7)
8. **PHP 8 string↔number comparison**: if the string is not fully numeric (leading/trailing whitespace allowed),
   the number is converted to a string and compared bytewise: `0 == "a"` false, `1 == "1abc"` false,
   `1 == " 1"` and `1 == "1 "` true, `"abc" == "ABC"` false, `"1e3" == "1000"` true. (§7)
9. **`a > b` is compiled as `b < a`**; "uncomparable" returns `1` for both orders, so `<`, `>`, `<=`, `>=`, `==`
   are all false (arrays with different keys, objects of different classes, enums, NaN). (§7.1)
10. **Array `==` is unordered** (same key/value pairs), `===` is ordered + identical; both short-circuit to
    *equal* when the two operands share the same underlying HashTable (copy-on-write), so `$m = $n = [NAN];`
    gives `$m == $n` **true** while `[NAN] == [NAN]` is false. (§2.2, §7.6)
11. **PHP 8.5 deprecates `null` array offsets**: `$a[null]`, `[null => 1]` → `D: Using null as an array offset is
    deprecated, use an empty string instead` (key becomes `""`). (§8.1)
12. **Negative-key append (PHP >= 8.3)**: `[-5 => 'a', 'b']` → keys `-5, -4`. (§8.3)
13. **PHP 8.5 NaN coercion warnings**: NaN → bool/string/array/object emits
    `W: unexpected NAN value was coerced to bool|string|array|object`; comparisons with bool/null do not. (§3.1)
14. **`int ** int` overflow uses a squaring loop**; 2904 of 7680 overflowing results differ from naive
    `pow((float)b, (float)e)`. Port the loop exactly; `pow()` itself is libm-dependent (glibc 2.41). (§6.6)
15. **`echo` of `true` is `"1"`, `false`/`null` are `""`**, arrays are `"Array"` with `W: Array to string conversion`,
    objects without `__toString` throw `Error("Object of class X could not be converted to string")`. (§9)
16. **A php-src pointer quirk** makes `"9223372036854775808e+" + 0` → `int(-9223372036854775808)`. (§4.5)

---

## 2. Value model

### 2.1 Type tags and their order

`Zend/zend_types.h:633-643`:

| tag | value |
|---|---|
| `IS_UNDEF` | 0 |
| `IS_NULL` | 1 |
| `IS_FALSE` | 2 |
| `IS_TRUE` | 3 |
| `IS_LONG` (int) | 4 |
| `IS_DOUBLE` (float) | 5 |
| `IS_STRING` | 6 |
| `IS_ARRAY` | 7 |
| `IS_OBJECT` | 8 |
| `IS_RESOURCE` | 9 |
| `IS_REFERENCE` | 10 |

The **numeric order matters**: engine code tests e.g. `Z_TYPE_P(op) < IS_TRUE` ("null or false"),
`Z_TYPE_P(op) <= IS_STRING` ("scalar"). Booleans are two distinct tags. Port with an enum in the same order.
Strings are **byte strings** (no encoding); all comparisons/conversions below are bytewise.

### 2.2 Value vs handle semantics

* `null`, bool, int, float, string, **array** are values. Arrays are copy-on-write: `$b = $a` shares the HashTable
  until one side is modified; logically `$b` is an independent copy, including the "next free element" counter
  (verified: `$a=[5=>1]; unset($a[5]); $b=$a; $b[]=2;` → `[6 => 2]`).
* Objects are **handles**: assignment, passing, and copying an array that contains an object all share the object
  (verified: `$copy = $arr; $copy['o']->v = 3;` changes `$o->v`). Enum cases are singletons. Closures are objects.
* **Pointer-equality shortcuts are observable**:
  * `zend_is_identical` for arrays: `Z_ARRVAL_P(op1) == Z_ARRVAL_P(op2) || zend_hash_compare(...)`
    (`zend_operators.c:2497-2499`).
  * `zend_compare_symbol_tables`: `if (ht1 == ht2) return 0;` (`zend_operators.c:3494-3499`).
  * Consequence (verified): `$n = [NAN]; $m = $n;` → `$m == $n` **true**, `$m === $n` **true**;
    but `[NAN] == [NAN]` false, `[NAN] === [NAN]` false, `NAN == NAN` false, `NAN === NAN` false, and
    `$x = NAN; $x == $x` false (scalars have no shortcut). The shortcut also applies to nested arrays during
    recursive comparison. A Rust port that stores arrays as `Rc<…>` with COW should use `Rc::ptr_eq` at exactly
    these two points to be faithful.

### 2.3 Type names used in messages

Four different naming functions are used by different messages (`Zend/zend_API.c:119-218`,
`ext/standard/type.c:39-80`):

| value | `zend_get_type_by_const` | `zend_zval_type_name` | `zend_zval_value_name` | `gettype()` | `get_debug_type()` |
|---|---|---|---|---|---|
| null | `null` | `null` | `null` | `NULL` | `null` |
| false | `bool` | `bool` | `false` | `boolean` | `bool` |
| true | `bool` | `bool` | `true` | `boolean` | `bool` |
| int | `int` | `int` | `int` | `integer` | `int` |
| float | `float` | `float` | `float` | `double` | `float` |
| string | `string` | `string` | `string` | `string` | `string` |
| array | `array` | `array` | `array` | `array` | `array` |
| object | `object` | class name | class name | `object` | class name (anonymous class: the C string of `ce->name`, i.e. up to the embedded NUL) |
| resource | `resource` | `resource` | `resource` | `resource` / `resource (closed)` | `resource (stream)` etc. / `resource (closed)` |

Who uses which:
* `zend_zval_type_name`: `Unsupported operand types: %s %s %s` (`zend_operators.c:1124`),
  `Cannot access offset of type %s on %s` (`Zend/zend.c:1881`), `Cannot use %s as array` (`zend_execute.c:3200`).
  → booleans print as `bool` (verified: `true + []` → `Unsupported operand types: bool + array`).
* `zend_zval_value_name`: parameter type errors `must be of type %s, %s given`, `Cannot increment %s`,
  `Cannot decrement %s`, `Cannot perform bitwise not on %s`, `Trying to access array offset on %s`,
  `Attempt to read property "%s" on %s` → booleans print as `true`/`false`
  (verified: `~true` → `Cannot perform bitwise not on true`; `true["a"]` → `Trying to access array offset on true`).
* `zend_get_type_by_const`: `Object of class %s could not be converted to %s`, `unexpected NAN value was coerced to %s`.

---

## 3. Casts: to bool / int / float / array / object

(To-string is §9 because it is a scope item of its own.)

### 3.1 To bool — `i_zend_is_true` (`Zend/zend_operators.h:409-461`), `convert_to_boolean` (`zend_operators.c:687-756`)

| input | result |
|---|---|
| null | false |
| false / true | itself |
| int | `!= 0` |
| float | `!= 0.0` (so `-0.0` → false); **NaN → true and `W: unexpected NAN value was coerced to bool`** |
| string | false **only** for `""` and `"0"` (exact bytes). `"0.0"`, `"00"`, `" "`, `"false"` → true |
| array | `count > 0` |
| object | true for all userland/std objects (`cast_object == zend_std_cast_object_tostring` short-circuit); enum cases → true |
| resource | handle != 0 (always true in practice) |

Contexts (all go through `i_zend_is_true`): `if`, `while`, `?:`, `? :`, `!`, `&&`, `||`, `xor`, `(bool)`,
`boolval()`, weak `bool` parameters. Verified NaN warnings for `(bool)NAN`, `if (NAN)`, `!NAN`, `NAN ? 1 : 0`.
**Not** in comparisons: `NAN == true` → true, `NAN == false` → false, `null < NAN` → true, all without warnings
(special-cased in `zend_compare`, §7.2).

### 3.2 To int

#### 3.2.1 Explicit: `(int)`, `intval($x)` (base 10), `settype($x, "int")` — `zval_get_long_func(op, is_strict=false)` (`zend_operators.c:957-1024`)

| input | result | diagnostics |
|---|---|---|
| null, false | 0 | |
| true | 1 | |
| int | itself | |
| float | `zend_dval_to_lval(d)` (§3.2.2) | `W` for NaN/Inf/out-of-range only (never the "loses precision" deprecation) |
| string | `is_numeric_string(s, allow_errors=true)` (§4): not numeric → 0; IS_LONG → value; IS_DOUBLE → `zend_dval_to_lval_cap` (§3.2.3, saturating) | none (silent prefix parse) |
| array | `count > 0 ? 1 : 0` | |
| object | `cast_object(IS_LONG)`; std objects fail → **1** | `W: Object of class %s could not be converted to int` (`zend_operators.c:232-238`) |
| resource | resource handle | |

Verified `(int)` of strings: `'123'`→123, `' 123'`→123, `'123 '`→123, `"\t\n123\r\v\f"`→123, `'12abc'`→12,
`'abc'`→0, `''`→0, `'0x1A'`→0, `'0b11'`→0, `'012'`→12, `'1e3'`→1000, `'1.9'`→1, `'-1.9'`→-1, `'.5'`→0, `'5.'`→5,
`'-'`→0, `'+5'`→5, `'+-5'`→0, `' - 5'`→0, `'1e'`→1, `'1e+'`→1, `'9223372036854775807'`→PHP_INT_MAX,
`'9223372036854775808'`→PHP_INT_MAX, `'-9223372036854775809'`→PHP_INT_MIN, `'1e100'`→PHP_INT_MAX,
`'-1e100'`→PHP_INT_MIN, `'99999999999999999999'`→PHP_INT_MAX, `'1_000'`→1, `"1\0"`→1, `'INF'`/`'NAN'`/`'inf'`→0,
`'  '`→0, `'-0'`→0, `'1.5e3abc'`→1500, `'1.9999999999999999'`→2 (strtod rounds to 2.0 first).

`intval($s, 0)` / base 16/8/2 uses `strtol`-style parsing with prefixes (`intval("0x1A", 0)`→26,
`intval("012", 0)`→10, `intval("0b11", 0)`→3) — a library function, outside this document except as noted.

#### 3.2.2 Float → int: `zend_dval_to_lval` (PHP 8.5) (`Zend/zend_operators.h:126-136`)

```text
fn zend_dval_to_lval(d: f64) -> i64:
    if !d.is_finite():                                   // NaN, +INF, -INF
        warn("The float " + fmt_H(d) + " is not representable as an int, cast occurred")
        return 0
    if !(d < 9223372036854775808.0 && d >= -9223372036854775808.0):   // ZEND_DOUBLE_FITS_LONG, zend_operators.h:115
        warn(same message)
        return zend_dval_to_lval_slow(d)                 // modular
    return d as i64                                      // truncation toward zero

fn zend_dval_to_lval_slow(d: f64) -> i64:                // zend_operators.c:3886-3898 (64-bit)
    let two64 = 18446744073709551616.0
    let mut dmod = fmod(d, two64)                        // exact
    if dmod < 0.0 { dmod += two64 }                      // d had no fraction, so no ceil needed
    return (dmod as u64) as i64
```

`fmt_H(d)` = `%.*H` with precision `-1` = the serialize_precision algorithm of §5 (uppercase `E`, no `.0`
suffix): `1.0E+20`, `-1.0E+20`, `NAN`, `INF`, `-INF`, `9.223372036854776E+18`.

Verified (`(int)$f`):

| float | result | warning |
|---|---|---|
| `1.0E+20` | 7766279631452241920 | yes |
| `-1.0E+20` | -7766279631452241920 | yes |
| `NAN`, `INF`, `-INF` | 0 | yes |
| `1.5` / `-1.5` | 1 / -1 | no |
| `9.223372036854776E+18` (= 2^63) | -9223372036854775808 | yes |
| `-9.223372036854776E+18` (= -2^63) | -9223372036854775808 | no (fits) |
| `1.0E+19` | -8446744073709551616 | yes |
| `1.8446744073709552E+19` (= 2^64) | 0 | yes |
| 2^64 + 8192 | 8192 | yes |

`zend_dval_to_lval_silent` (`zend_operators.h:138-146`) is the same without the warning (used e.g. for string
offsets, §8.9).

#### 3.2.3 Float-string → int: `zend_dval_to_lval_cap` (`Zend/zend_operators.h:149-158`)

```text
fn zend_dval_to_lval_cap(d: f64) -> i64:
    if !d.is_finite() { return 0 }
    if !FITS_LONG(d) { return if d > 0.0 { i64::MAX } else { i64::MIN } }
    return d as i64
```
No diagnostic (the `zend_oob_string_to_long_error` function exists at `zend_operators.c:947-950` but is never
called in 8.5.13). Used whenever a numeric string of float type is converted to int.

#### 3.2.4 Implicit int contexts — `zendi_try_get_long` (`zend_operators.c:378-470`), `zend_dval_to_lval_safe` (`zend_operators.h:165-172`)

Used by `%`, `<<`, `>>`, and `|`, `&`, `^` when not both operands are strings (also `~float`):

| operand | result | diagnostics |
|---|---|---|
| null/false, true | 0, 1 | |
| int | itself | |
| float | `l = zend_dval_to_lval(d)` | `W` per §3.2.2; then if `(double)l != d && FITS_LONG(d)`: `D: Implicit conversion from float %s to int loses precision` (`%s` = fmt_H(d)). **NaN gets both** the W and the D (FITS_LONG(NaN) is true). |
| string | `is_numeric_string_ex(allow_errors=true)` | not numeric → **failure** (operator throws `TypeError` "Unsupported operand types"); trailing data → `W: A non-numeric value encountered` (emitted first); IS_DOUBLE → `zend_dval_to_lval_cap`, then if `(double)l != d`: `D: Implicit conversion from float-string "%s" to int loses precision` with the **raw original string** (verified `"7.5x" % 2` → `{W: A non-numeric value encountered} {D: Implicit conversion from float-string "7.5x" to int loses precision} int(1)`) |
| array, resource | failure → TypeError | |
| object | `cast_object(IS_LONG)`; std objects fail → TypeError | |

Array keys use `zend_dval_to_lval_safe` too (§8.1). Parameters typed `int` use a *different* routine (§11:
out-of-range or NaN floats are a TypeError there).

### 3.3 To float — `zval_get_double_func` (`zend_operators.c:1027-1063`)

| input | result |
|---|---|
| null / false / true | 0.0 / 0.0 / 1.0 |
| int | `(double)i` (round-to-nearest-even for magnitudes above 2^53) |
| string | `zend_strtod(s, NULL)`: longest valid decimal prefix after leading whitespace, **never fails, no warnings** (§4.4) |
| array | 0.0 if empty, else 1.0 |
| object | `W: Object of class %s could not be converted to float`, 1.0 |
| resource | handle |

Verified `(float)`: `'123'`→123.0, `' 1.5'`/`'1.5 '`/`'1.5abc'`→1.5, `'abc'`/`''`→0.0, `'0x1A'`→0.0, `'1e3'`→1000.0,
`'1E-3'`→0.001, `'.5'`→0.5, `'5.'`→5.0, `'-.5'`→-0.5, `'+.5'`→0.5, `'- 5'`→0.0, `'1e'`/`'1e+'`→1.0,
`'1e400'`→INF, `'-1e400'`→-INF, `'INF'`/`'inf'`/`'NAN'`→0.0, `"\n1"`→1.0, `'1e-400'`→0.0, `'1.e5'`→100000.0,
`'.e5'`→0.0, `'00012.5'`→12.5, `'-0'`→**-0.0**, `'9223372036854775808'`→9.223372036854776E+18.

### 3.4 To array — `convert_to_array` (`zend_operators.c:843-886`), `convert_scalar_to_array` (`:832-840`)

| input | result |
|---|---|
| null | `[]` |
| bool/int/float/string/resource | `[0 => value]`; float NaN additionally `W: unexpected NAN value was coerced to array` |
| array | itself |
| object (non-Closure) | property table; private props as key `"\0Class\0name"`, protected as `"\0*\0name"`; integer-like property names become int keys |
| Closure | `[0 => closure]` |

### 3.5 To object — `convert_to_object` (`zend_operators.c:889-932`)

null → empty `stdClass`; array → `stdClass` with the elements as properties (int keys become string property
names); scalar → `stdClass` with property `"scalar"` (verified `(object)1` → `['scalar' => 1]`); NaN →
`W: unexpected NAN value was coerced to object`.

### 3.6 Float-to-int summary of contexts

| context | NaN / Inf | out of range | fractional |
|---|---|---|---|
| `(int)`, `intval`, `settype` | 0 + W | wrap mod 2^64 + W | truncate, silent |
| `%`, `<<`, `>>`, `&`, `\|`, `^`, `~` | 0 + W (+D for NaN) | wrap + W | truncate + D |
| array key (`$a[1.5]`, `[1.5 => x]`, `array_key_exists(1.5, …)`) | 0 + W (+D for NaN) | wrap + W | truncate + D |
| string offset read (`$s[1.7]`) | `W: String offset cast occurred`, offset 0 | `W: String offset cast occurred`, silent wrap (`$s[1e20]` then `W: Uninitialized string offset 7766279631452241920`) | truncate + `W: String offset cast occurred` |
| string offset in `isset($s[1.5])` | (not tested) | (not tested) | truncate + `D: Implicit conversion from float 1.5 to int loses precision` |
| string offset in `$s[1.5] ?? x` | silent | silent | silent |
| `int` parameter (weak mode) | **TypeError** | **TypeError** | truncate + D |
| float-**string** anywhere (`(int)"1e100"`) | 0 | saturate (no W) | truncate (D only in implicit contexts) |

---

## 4. Numeric strings (`is_numeric_string`) and `zend_strtod`

### 4.1 What counts as numeric (PHP 8 "saner numeric strings")

A string is **numeric** iff it is: optional leading whitespace, optional single sign, then either digits with
optional `.digits*` or `.digits+`, optional exponent `[eE][+-]?digits+`, then optional trailing whitespace —
and nothing else. Whitespace = space, `\t`, `\n`, `\r`, `\v` (0x0B), `\f` (0x0C) (`zend_operators.c:3659`,
`:3727`). No hex/octal/binary, no `_`, no `inf`/`nan`, no locale decimal separator, no Unicode digits.
A string with a numeric **prefix** followed by other bytes is **leading-numeric** (accepted only where
`allow_errors=true`, with a warning in arithmetic contexts).

Integer vs float result: no `.`/exponent and fits in int64 → int; otherwise float. Integers that overflow are
returned as float with an overflow flag (`oflow = ±1`) used by string-string comparison (§7.4).

Verified `is_numeric()` (`allow_errors=false`): true for `'123'`, `' 123'`, `'123 '`, `' 123 '`, `'1e3'`, `'.5'`,
`'5.'`, `'-.5'`, `'+.5'`, `"123\n"`, `"\n123"`, `'1e+5'`, `'1e-5'`, `'1E5'`, `'+0'`, `'-0'`, `'00'`, `"\v1"`, `"\f1"`,
`'1.e5'`; false for `'1e'`, `'.'`, `'0x1A'`, `'1_000'`, `"1\0"`, `''`, `' '`, `'-'`, `'+'`, `'1.5.5'`, `'1e5e5'`, `'.e5'`.

### 4.2 Port-ready algorithm — `_is_numeric_string_ex` (`zend_operators.c:3636-3772`, pre-check `zend_operators.h:176-183`)

The port below assumes the caller always wants `dval` (true for every engine call site relevant to Twig; the
`dval == NULL` variant only changes performance, not results). `byte(i)` returns `0` for `i >= len`
(C strings are NUL-terminated; embedded NUL bytes simply stop scanning).

```text
enum Num { NotNumeric, Long(i64), Double(f64) }
fn is_ws(c) = c in {b' ', b'\t', b'\n', b'\r', 0x0B, 0x0C}

fn is_numeric_string_ex(s: &[u8], allow_errors: bool) -> (Num, oflow: i8, trailing: bool) {
    if s.is_empty() { return (NotNumeric, 0, false) }                 // :3646
    if byte(0) > b'9' { return (NotNumeric, 0, false) }               // inline pre-check (letters etc.)
    let mut str = 0; while is_ws(byte(str)) { str += 1 }              // leading whitespace
    let mut ptr = str;
    let neg = byte(ptr) == b'-';
    if byte(ptr) == b'-' || byte(ptr) == b'+' { ptr += 1 }
    let (mut digits, mut dp_or_e, mut tmp, mut oflow) = (0, 0, 0u64, 0i8);
    let mut is_double = false;
    if is_digit(byte(ptr)) {
        while byte(ptr) == b'0' { ptr += 1 }                          // leading zeros don't count
        loop {
            if digits >= 20 { break }                                 // MAX_LENGTH_OF_LONG = 20 (zend_long.h:112)
            let c = byte(ptr);
            if is_digit(c) { tmp = tmp.wrapping_mul(10).wrapping_add((c - b'0') as u64); digits += 1; ptr += 1; continue }
            if c == b'.' && dp_or_e < 1 { is_double = true; break }
            if (c == b'e' || c == b'E') && dp_or_e < 2 {
                let mut e = ptr + 1;
                if byte(e) == b'-' || byte(e) == b'+' { ptr = e; e += 1 }   // QUIRK (§4.5): ptr is moved onto the sign
                if is_digit(byte(e)) { is_double = true; break }
            }
            break
        }
        if !is_double && digits >= 20 { oflow = if byte(str) == b'-' { -1 } else { 1 }; is_double = true }
    } else if byte(ptr) == b'.' && is_digit(byte(ptr + 1)) {
        is_double = true
    } else {
        return (NotNumeric, 0, false)
    }
    let mut dval = 0.0;
    if is_double {
        let (v, consumed) = zend_strtod_prefix(&s[str..]);            // §4.4; starts at the sign, not at ptr
        dval = v; ptr = str + consumed;
    }
    let mut trailing = false;
    if ptr != s.len() {
        if !s[ptr..].iter().all(|&c| is_ws(c)) {                      // trailing whitespace is allowed
            if !allow_errors { return (NotNumeric, 0, false) }
            trailing = true;
        }
    }
    if !is_double {
        if digits == 19 {                                             // MAX_LENGTH_OF_LONG - 1
            // C: strcmp(&ptr[-digits], "9223372036854775808") — compares up to the first NUL/end
            let cmp = c_strcmp(&s[ptr - 19..], b"9223372036854775808");
            if !(cmp < 0 || (cmp == 0 && byte(str) == b'-')) {
                let d = zend_strtod_prefix(&s[str..]).0;
                return (Double(d), if byte(str) == b'-' { -1 } else { 1 }, trailing)
            }
        }
        let v = if neg { tmp.wrapping_neg() } else { tmp } as i64;
        return (Long(v), 0, trailing)
    }
    (Double(dval), oflow, trailing)
}
```

Notes:
* Leading zeros: `"00"` → `Long(0)`, `"007"` → `Long(7)`, `"-0"` → `Long(0)` (int zero, but `(float)"-0"` is `-0.0`).
* The digit counter stops at 20 digits and hands the whole thing to `zend_strtod` (overflow → float, `oflow=±1`).
* The 19-digit case compares against the magnitude of `PHP_INT_MIN`; exactly `"-9223372036854775808"` stays int.
* The C trailing-whitespace test (`zend_operators.c:3725-3739`) is equivalent to "all remaining bytes are
  whitespace" (it counts the whitespace run and compares with the end).

### 4.3 Which contexts use which mode

| context | routine | `allow_errors` | trailing garbage | not numeric |
|---|---|---|---|---|
| `+ - * / **`, unary `-`/`+` | `_zendi_try_convert_scalar_to_number` (`zend_operators.c:325-364`) | true | `W: A non-numeric value encountered`, prefix value used | `TypeError: Unsupported operand types: …` |
| `% << >>`, and `& \| ^` unless both strings | `zendi_try_get_long` (`:378-470`) | true | same W; float-strings also D (§3.2.4) | TypeError |
| `(int)`, `intval`, `settype` | `zval_get_long_func` (`:957-1024`) | true | silent prefix | 0 |
| `(float)`, `floatval` | `zend_strtod` directly | n/a | silent prefix | 0.0 |
| `==`, `<`, `<=>`, … (string vs number, string vs string) | `is_numeric_string(…, false)` | false | string is "non-numeric" → bytewise comparison | bytewise comparison |
| `is_numeric()` | `allow_errors=false` | false | false | false |
| `++` / `--` on strings | `is_numeric_str_function` (`:3630-3634`) | false | treated as non-numeric (string increment) | string increment |
| int/float/`int\|float` **parameters** (weak mode) | `is_numeric_str_function` | false | **TypeError** `must be of type int, string given` | TypeError |
| string offsets `$s["1"]` | `is_numeric_string_ex(…, true)`, must be **Long** | true | `W: Illegal string offset "1x"`, prefix used | `TypeError: Cannot access offset of type string on string` |
| array keys `$a["1"]` | **not** this routine: `_zend_handle_numeric_str` (§8.2, canonical decimal integers only) | — | — | — |

Verified arithmetic examples (`$s + 0`): `'1e'` → `{W} int(1)`; `'1e5x'` → `{W} float(100000.0)`; `'.5x'` →
`{W} float(0.5)`; `' 1 '` → `int(1)`; `'1 x'` → `{W} int(1)`; `"\n"` → TypeError; `'1e1000'` → `float(INF)`;
`'99999999999999999999abc'` → `{W} float(1.0E+20)`; `'0x1A'` → `{W} int(0)`; `'1.5.5'` → `{W} float(1.5)`;
`'-'` → TypeError; `'+1'` → `int(1)`; `'- 1'` → TypeError; `'00012'` → `int(12)`; `'1_000'` → `{W} int(1)`;
`' .5 '` → `float(0.5)`; `"1\0"` → `{W} int(1)`; U+0661 ARABIC-INDIC DIGIT ONE → TypeError; `''` → TypeError;
`'9223372036854775807' + 1` → `float(9.223372036854776E+18)`; `'9223372036854775808' + 0` →
`float(9.223372036854776E+18)`.

### 4.4 `zend_strtod` (`Zend/zend_strtod.c:2516-…`) and the float parse

Configuration (`Zend/zend_strtod_int.h`): `NO_HEX_FP` (line 45) → no `0x` floats; `USE_LOCALE` undefined
(lines 49-50) → `.` is always the decimal point; `NO_INFNAN_CHECK` (lines 53-54) → no `inf`/`nan` words;
IEEE little-endian (`IEEE_8087`, line 93). It is David Gay's correctly-rounded strtod.

Accepted grammar (longest prefix; the end pointer reports how much was consumed):

```text
ws*  sign?  ( digits ('.' digits*)? | '.' digits )  ( [eE] sign? digits )?
ws   = ' ' \t \n \v \f \r          (skipped only BEFORE the sign; "- 5" is not a number)
```
* If no mantissa digit is present (`""`, `"-"`, `"."`, `".e5"`, `"- 5"`), the result is `0.0` and nothing is
  consumed (`zend_strtod.c:2741-2744`).
* An exponent marker not followed by digits is not consumed (`"1e"`, `"1e+"` → 1.0, end before `e`).
* Overflow → `±INF`; underflow → correctly rounded subnormal or `±0.0`; exponents with more than 8 digits or
  `> 19999` are clamped to 19999 (`zend_strtod.c:2695-2699`; affects only absurd inputs).
* Sign is applied last, so `"-0"` → `-0.0`.

**Rust port:** extract the prefix with the grammar above (keeping the sign), then use `str::parse::<f64>()` on that
slice. Verified bit-identical against PHP's `(float)$s` for **59,294** random decimal strings (1–800 digit mantissas,
exponents ±330, subnormals, 16–18 significant digit near-ties): **0 mismatches** (§13, `rustcheck2`).
UNVERIFIED: inputs relying on the 19999-exponent clamp (e.g. 30 000 leading fractional zeros plus `e+20005`)
would differ, because Rust does not clamp.

### 4.5 Quirk: exponent sign moves the digit pointer

In the integer scan, `if (*e == '-' || *e == '+') { ptr = e++; }` (`zend_operators.c:3691-3693`) moves `ptr` onto
the sign even when no exponent digit follows. With exactly 19 significant integer digits the later
`strcmp(&ptr[-digits], long_min_digits)` then starts one byte late, so a value `>= 2^63` can be accepted as an int
and wraps. Verified:

| input | `$s + 0` | `(int)$s` |
|---|---|---|
| `"9223372036854775808e+"` | `{W} int(-9223372036854775808)` | `int(-9223372036854775808)` |
| `"9300000000000000000e-"` | `{W} int(-9146744073709551616)` | `int(-9146744073709551616)` |
| `"-9223372036854775809e+"` | `{W} int(9223372036854775807)` | `int(9223372036854775807)` |
| `"9223372036854775808e"` (no sign) | `{W} float(9.223372036854776E+18)` | `int(9223372036854775807)` |
| `"9300000000000000000x"` | `{W} float(9.3E+18)` | `int(9223372036854775807)` |

Only inputs with `allow_errors=true` reach the int result (with `allow_errors=false` the trailing sign makes the
string non-numeric). Port the pointer move literally if bit-exactness matters.

---

## 5. Float to string: `precision` vs `serialize_precision`

### 5.1 Which consumers use which setting

| setting (default) | routine | consumers (verified unless noted) |
|---|---|---|
| `precision` = **14** | `zend_double_to_str` (`zend_operators.c:3616-3628`): `zend_gcvt(num, precision ? precision : 1, '.', 'E', buf)`; NaN also emits `W: unexpected NAN value was coerced to string` | `echo`, `print`, `.` concatenation, string interpolation, `(string)`, `strval`, `settype(…,"string")`, `implode`, `sprintf('%s')`, Twig `{{ }}` (`yield (string)`), weak-mode `string` parameters (`strlen(1.5)` → 3), `print_r` (`Zend/zend.c:601`), number-vs-non-numeric-string comparison fallback (`zend_operators.c:2298`) |
| `serialize_precision` = **-1** | `%.*H` in `php_printf` / `smart_str_append_double(…, serialize_precision, zero_frac)` | `var_dump` (`ext/standard/var.c:147`), `var_export` (`var.c:574`, appends `.0`), `json_encode` (lowercase `e`, no `.0` unless `JSON_PRESERVE_ZERO_FRACTION`), `serialize` (`d:…;`, `var.c:1183`), `debug_zval_dump` (`var.c:350`), engine messages formatted with `%.*H` and precision −1 (float→int warnings/deprecations, `zend_operators.c:937,945`) |

### 5.2 `zend_gcvt` — port-ready (`Zend/zend_strtod.c:4509-4606`)

```text
// p: the precision value (>= 1 after the caller's 0→1 mapping), or -1 for "shortest"
// exp_char: 'E' for every PHP output above (json_encode passes 'e')
fn zend_gcvt(value: f64, p: i32, exp_char: u8) -> String {
    let (mode, ndigit) = if p >= 0 { (2, p) } else { (0, 17) };
    let (digits, decpt, sign) = zend_dtoa(value, mode, ndigit);   // §5.3
    if decpt == 9999 {                                            // Inf / NaN
        let s = (if sign && digits[0] == b'I' { "-" } else { "" }).to_string()
              + if digits[0] == b'I' { "INF" } else { "NAN" };
        return s[..min(s.len(), ndigit as usize)];                // snprintf(buf, ndigit + 1, …) truncation
    }
    let mut out = if sign { "-" } else { "" };                    // sign bit: -0.0 gives "-0"
    if (decpt >= 0 && decpt > ndigit) || decpt < -3 {             // exponential form
        let e = decpt - 1;
        out += digits[0]; out += '.';
        out += if digits.len() == 1 { "0" } else { &digits[1..] };
        out += exp_char; out += if e < 0 { '-' } else { '+' }; out += abs(e).to_string();   // no zero padding
    } else if decpt < 0 {                                         // decpt in -3..=-1
        out += "0."; out += "0".repeat(-decpt); out += digits;
    } else {                                                      // 0 <= decpt <= ndigit
        for i in 0..decpt { out += if i < digits.len() { digits[i] } else { '0' } }
        if digits.len() > decpt as usize {
            if decpt == 0 { out += '0' }
            out += '.'; out += &digits[decpt..];
        }
    }
    out
}
```
`zend_dtoa` returns `"0"` with `decpt = 1` for ±0.0 (so `0.0` → `"0"`, `-0.0` → `"-0"`).
`decpt` is the decimal exponent + 1 (value = 0.d1d2… × 10^decpt).

Consequences with the defaults:
* echo (`p = 14`): exponential when `decpt > 14` (value ≥ 1e14 after rounding) or `decpt < -3` (value < 1e-4).
* var_dump/var_export (`p = -1` → `ndigit = 17`): exponential when `decpt > 17` (≥ 1e17) or `decpt < -3`.
* The mantissa always has a `.` in exponential form (`1.0E+25`, never `1E+25`); the exponent has a sign and no
  padding (`E+25`, `E-5`, `E+100`, `E-324`).
* `NAN` is never signed. For `precision` 1–3 the snprintf truncation is visible (verified: precision=1 →
  `INF`→`"I"`, `-INF`→`"-"`, `NAN`→`"N"`; precision=2 → `"IN"`, `"-I"`; precision=3 → `"-IN"`).
  The `%H` path in `main/snprintf.c:886-899` / `main/spprintf.c:595-608` handles NaN/Inf *before* calling
  `zend_gcvt`, so var_dump always prints full `NAN`/`INF`/`-INF`.
* `smart_str_append_double(…, zero_fraction=true)` (`Zend/zend_smart_str.c:156-165`, used by var_export) appends
  `".0"` when the value is finite and the text contains no `'.'`.

### 5.3 `zend_dtoa` digit generation (`Zend/zend_strtod.c:3703-4400`)

Mode semantics (header comment `zend_strtod.c:3711-3737`):

* **Mode 2** (`precision >= 1`): the first `N = ndigit` significant digits of |value|, **correctly rounded with
  round-half-even on the exact binary value** (`zend_strtod.c:4111-4115`, `:4355-4361`), trailing zeros removed
  (`:4376`) — except the quirk of §5.4.
* **Mode 0** (`precision = -1`): the **shortest** digit string that reads back (round-to-nearest-even) as the same
  double; boundary values are acceptable iff the significand is even (`zend_strtod.c:4255`, `:4272-4275`). When two
  candidates of that minimal length are acceptable, the **nearest** to the exact value is chosen and an **exact tie
  goes to the even digit** (`zend_strtod.c:4290-4299`: round up only if `j1 > 0 || (j1 == 0 && dig & 1)`).

Verified tie behaviour (mode 2 with `ini_set('precision', p)`):

| p | 0.5 | 1.5 | 2.5 | 3.5 | 0.25 | 0.125 | 0.375 | 1234.5 | 1235.5 |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 0.5 | 2 | 2 | 4 | 0.2 | 0.1 | 0.4 | 1.0E+3 | 1.0E+3 |
| 2 | 0.5 | 1.5 | 2.5 | 3.5 | 0.25 | 0.12 | 0.38 | 1.2E+3 | 1.2E+3 |
| 3 | 0.5 | 1.5 | 2.5 | 3.5 | 0.25 | 0.125 | 0.375 | 1.23E+3 | 1.24E+3 |

`precision=0` behaves as 1 (`1.5`→`2`, `2.5`→`2`, `0.25`→`0.2`); `precision=-1` gives the shortest form
(`0.1+0.2` → `0.30000000000000004`, `1e15` → `1000000000000000`, `1e17` → `1.0E+17`); `precision=17` →
`0.10000000000000001`, `0.33333333333333331`, `1.0000000000000001E-5`.

### 5.4 Quirk: integral floats below 1e15 keep trailing zeros on an exact tie

Path (mode 2, `N <= 14`): the "quick" floating-point attempt (`zend_strtod.c:3961-4073`, used when
`ilim <= Quick_max = 14`) gives up when the remainder is within its error band of ½; for an integral value with
`k <= Int_max = 14` (|value| < 1e15) dtoa then uses the "small integer" loop (`zend_strtod.c:4075-4131`), which
rounds half-even but, when it does **not** round up, breaks out **without stripping trailing zeros**
(`goto ret1`, no `while(*--s == '0')`). For integers the quick path only fails on an exact tie, so:

> If |v| is an integer, |v| < 1e15, it has more than N significant digits, the dropped digits are exactly `5`
> followed by zeros, and the N-th digit is even → the digit string is the first N digits **verbatim** (zeros kept).

Since this needs more than N digits, the output is always in exponential form. Verified (precision 14 unless noted):

| value | echo |
|---|---|
| `100000000000001.0` | `1.0E+14` (not a tie → stripped) |
| `100000000000004.0` | `1.0E+14` |
| `100000000000005.0` | `1.0000000000000E+14` |
| `100000000000006.0` | `1.0000000000001E+14` |
| `100000000000015.0` | `1.0000000000002E+14` (odd → round up, nothing to keep) |
| `120000000000001.0` | `1.2E+14` |
| `123456789012305.0` | `1.2345678901230E+14` |
| `123456789012315.0` | `1.2345678901232E+14` |
| `900000000000005.0` | `9.0000000000000E+14` |
| `999999999999995.0` | `1.0E+15` (round up carries) |
| `566874425495005.0` | `5.6687442549500E+14` |
| precision 5: `120005.0` / `120015.0` / `120004.0` | `1.2000E+5` / `1.2002E+5` / `1.2E+5` |

### 5.5 Rust implementation recipe (validated)

From `rustcheck/src/main.rs` (§13); `a = |value|`, finite, non-zero:

```rust
// dtoa mode 0: Rust gives the shortest length n; re-round to n digits with ties-to-even if that round-trips.
fn shortest_even(a: f64) -> String {
    let s = format!("{:e}", a);
    let n = s.split('e').next().unwrap().bytes().filter(|c| c.is_ascii_digit()).count();
    let c = format!("{:.*e}", n - 1, a);          // Rust's exact formatter: correctly rounded, ties-to-even
    if c.parse::<f64>().unwrap() == a { c } else { s }
}
// dtoa mode 2: format!("{:.*e}", N - 1, a), then strip trailing zeros of the mantissa,
// unless small_int_tie_keep_zeros(a, N):
fn small_int_tie_keep_zeros(a: f64, n: i32) -> bool {
    if n > 14 || a.fract() != 0.0 || a >= 1e15 { return false; }
    let s = format!("{}", a as u64);
    if s.len() as i32 <= n { return false; }
    let b = s.as_bytes();
    let rest = &b[n as usize..];
    rest[0] == b'5' && rest[1..].iter().all(|&c| c == b'0') && (b[n as usize - 1] - b'0') % 2 == 0
}
```
Then split the mantissa/exponent (`decpt = exp10 + 1`) and apply `zend_gcvt` (§5.2). Validation against PHP for
**314,254** doubles (two seeds: random bit patterns, random short decimals, 15-digit integers ending in 5 and their
/10, /1000, other integer ties, all powers of 2 and 10, subnormals, negatives): **0 mismatches** for echo
(precision 14), var_export (serialize_precision −1 + `.0`), and precision 17. Without the ties-to-even fix,
2,023 of 152,127 values differed in var_export; without the small-integer rule, 1,989 differed in echo.
Toolchain used: rustc 1.102.0-nightly (2026-11-19). UNVERIFIED: older Rust releases' exact-mode tie handling.

### 5.6 Verified examples

| value | `echo` (p=14) | `var_export` | `json_encode` | `var_dump` |
|---|---|---|---|---|
| `0.0` | `0` | `0.0` | `0` | `float(0)` |
| `-0.0` | `-0` | `-0.0` | `-0` | `float(-0)` |
| `1.0` | `1` | `1.0` | `1` | `float(1)` |
| `0.1` | `0.1` | `0.1` | `0.1` | `float(0.1)` |
| `100.0` | `100` | `100.0` | `100` | `float(100)` |
| `1e14` | `1.0E+14` | `100000000000000.0` | `100000000000000` | `float(100000000000000)` |
| `1e15` | `1.0E+15` | `1000000000000000.0` | `1000000000000000` | `float(1000000000000000)` |
| `1e16` | `1.0E+16` | `10000000000000000.0` | `10000000000000000` | `float(10000000000000000)` |
| `1e17` | `1.0E+17` | `1.0E+17` | `1.0e+17` | `float(1.0E+17)` |
| `1e25` | `1.0E+25` | `1.0E+25` | `1.0e+25` | `float(1.0E+25)` |
| `123456789012345.0` | `1.2345678901234E+14` | `123456789012345.0` | `123456789012345` | `float(123456789012345)` |
| `1234567890123456.0` | `1.2345678901235E+15` | `1234567890123456.0` | `1234567890123456` | `float(1234567890123456)` |
| `12345678901234567.0` | `1.2345678901235E+16` | `12345678901234568.0` | `12345678901234568` | `float(12345678901234568)` |
| `0.0001` | `0.0001` | `0.0001` | `0.0001` | `float(0.0001)` |
| `0.00001` | `1.0E-5` | `1.0E-5` | `1.0e-5` | `float(1.0E-5)` |
| `0.000123` | `0.000123` | `0.000123` | `0.000123` | `float(0.000123)` |
| `1.5e-7` | `1.5E-7` | `1.5E-7` | `1.5e-7` | `float(1.5E-7)` |
| `0.1+0.2` | `0.3` | `0.30000000000000004` | `0.30000000000000004` | `float(0.30000000000000004)` |
| `1/3` | `0.33333333333333` | `0.3333333333333333` | `0.3333333333333333` | `float(0.3333333333333333)` |
| `2/3` | `0.66666666666667` | `0.6666666666666666` | | |
| `10/3` | `3.3333333333333` | `3.3333333333333335` | | |
| `M_PI` | `3.1415926535898` | `3.141592653589793` | | |
| `PHP_INT_MAX + 1` | `9.2233720368548E+18` | `9.223372036854776E+18` | | `float(9.223372036854776E+18)` |
| `1.7976931348623157E+308` | `1.7976931348623E+308` | `1.7976931348623157E+308` | | |
| `4.9E-324` | `4.9406564584125E-324` | `5.0E-324` | `5.0e-324` | `float(5.0E-324)` |
| `99999999999999.99` | `1.0E+14` | `99999999999999.98` | | |
| `999999999999999.9` | `1.0E+15` | `999999999999999.9` | | |
| `9007199254740993.0` (→ 2^53) | `9.007199254741E+15` | `9007199254740992.0` | | |
| `NAN` | `{W: unexpected NAN value was coerced to string} NAN` | `NAN` | `false` (`json_last_error_msg()` = `Inf and NaN cannot be JSON encoded`) | `float(NAN)` |
| `INF` / `-INF` | `INF` / `-INF` | `INF` / `-INF` | `false` | `float(INF)` / `float(-INF)` |
| `round(-0.4)` | `-0` | `-0.0` | | |
| `7.0/3` | `2.3333333333333` | | | |
| `sqrt(2)` | `1.4142135623731` | | | |
| `1/7` | `0.14285714285714` | | | |

Other verified serialize-precision outputs: `serialize(0.1+0.2)` → `d:0.30000000000000004;`, `serialize(-0.0)` →
`d:-0;`, `serialize(1e100)` → `d:1.0E+100;`, `json_encode(1e100)` → `1.0e+100`,
`json_encode(1.0, JSON_PRESERVE_ZERO_FRACTION)` → `1.0`, `print_r(1.0)` → `1`, `print_r(-0.0)` → `-0`,
`print_r(1e25)` → `1.0E+25`. With `serialize_precision=17`: `var_dump(0.1)` → `float(0.10000000000000001)`;
with `serialize_precision=5`: `var_export(100000.0)` → `1.0E+5` (contains `.`, so no `.0` appended).

---

## 6. Arithmetic and other operators

### 6.1 Operand conversion and dispatch (`+ - * / **`) — `zend_operators.c:1144-1446`

```text
fn arith(op, a, b) -> Result<Value>:
    if let Some(r) = fast(op, a, b) { return r }        // (int,int) (float,float) (int,float) (float,int); '+' also (array,array)
    a = deref(a); b = deref(b); retry fast(op, a, b)
    // objects with a do_operation handler (GMP, BCMath\Number) would be tried here; userland objects have none
    na = to_number(a)?;                                  // op1 first …
    nb = to_number(b)?;                                  // … then op2
    fast(op, na, nb)

fn to_number(v) -> Result<Number>:                       // _zendi_try_convert_scalar_to_number, :325-364
    null | false → Ok(Long(0));  true → Ok(Long(1));  Long/Double → Ok(v)
    String s → match is_numeric_string_ex(s, allow_errors = true):
        NotNumeric            → Err
        (n, trailing = true)  → warn("A non-numeric value encountered"); Ok(n)
        (n, _)                → Ok(n)
    Array | Resource | Object(std) → Err
on Err: throw TypeError(format!("Unsupported operand types: {} {} {}", type_name(a), SYM, type_name(b)))
        // type_name = zend_zval_type_name (§2.3); SYM ∈ + - * / % ** | & ^ << >>  (zend_operators.c:1119-1126)
```
Ordering is observable (verified): `"1x" + "abc"` → `{W: A non-numeric value encountered}` then
`TypeError: Unsupported operand types: string + string`; `"abc" + "1x"` → TypeError only; `"1x" + "2y"` → two
warnings, `int(3)`. The error names the **original** operand types (`null`, `bool`, `int`, `float`, `string`,
`array`, class name).

Verified TypeErrors: `"abc" + 1` → `string + int`; `1 + "abc"` → `int + string`; `"" + 1` → `string + int`;
`[] + 1` → `array + int`; `1 + []` → `int + array`; `[] + null` → `array + null`; `new stdClass + 1` →
`stdClass + int`; `true + []` → `bool + array`; `1.5 + []` → `float + array`; `Suit::Hearts + 1` → `Suit + int`;
`"abc" - 1` → `string - int`; `[] - []` → `array - array`; `"3" * []` → `string * array`; `null * "x"` →
`null * string`; `"2" ** "abc"` → `string ** string`; `"abc" / 0` → `string / int` (type check precedes the
zero check); `"6" / ""` → `string / string`.

### 6.2 `+`, `-`, `*`

* int ⊕ int: exact; on overflow the result is the float `(double)a ⊕ (double)b` computed from the original
  operands (`zend_operators.h:711-917`; multiply: `Zend/zend_multiply.h:27-80`, `dval = (double)a * (double)b`).
* any float operand: IEEE double arithmetic after converting the int side with `(double)`.
* Result is int iff both (converted) operands are int and no overflow.

Verified: `PHP_INT_MAX + 1` → `float(9.223372036854776E+18)`; `PHP_INT_MIN - 1` → `float(-9.223372036854776E+18)`;
`PHP_INT_MAX * 2` → `float(1.8446744073709552E+19)`; `PHP_INT_MAX * PHP_INT_MAX` → `float(8.507059173023462E+37)`;
`-1 * PHP_INT_MIN` → `float(9.223372036854776E+18)`; `"1" + "1"` → `int(2)`; `"1.0" + 1` → `float(2.0)`;
`"1e2" + 1` → `float(101.0)`; `" 1" + 1` / `"1 " + 1` → `int(2)`; `"12abc" + 1` → `{W} int(13)`;
`null + 1` → 1; `true + 1` → 2; `false + 1.5` → 1.5; `null + null` → 0; `0.1 * 3` → `0.30000000000000004`;
`"1" - "1.0"` → `float(0.0)`.

### 6.3 `array + array` (union) — `add_function_array`, `zend_operators.c:1129-1141`

Result = copy of the left array; then each `(key, value)` of the right array, in the right array's order, is added
**only if the key is absent** (`zend_hash_merge(…, overwrite=0)`). `$a += $a` is a no-op. Only `+` accepts arrays;
`array + scalar` throws `Unsupported operand types: array + int`.
Verified: `[1] + [2,3]` → `[0=>1, 1=>3]`; `['a'=>1] + ['a'=>2,'b'=>3]` → `['a'=>1,'b'=>3]`;
`[1, 2] + [5 => 3, 0 => 9]` → `[0=>1, 1=>2, 5=>3]`.

### 6.4 `/` — `div_function_base` / `div_function`, `zend_operators.c:1454-1538`

```text
(int a, int b):    b == 0                  → throw DivisionByZeroError("Division by zero")
                   b == -1 && a == i64::MIN → Double(9223372036854775808.0)   // 2^63
                   a % b == 0              → Long(a / b)
                   else                    → Double((a as f64) / (b as f64))
(float/int mixes): divisor == 0.0 (incl. -0.0) → throw DivisionByZeroError("Division by zero")
                   else Double(x / y)
other types: to_number both (TypeError on failure), then the same rules
```
Verified: `6/3` → `int(2)`; `7/2` → 3.5; `-7/2` → -3.5; `1/0`, `1/0.0`, `1.5/0`, `0/0`, `1/-0.0`, `6/"0"`,
`6/null`, `6/false` → `DivisionByZeroError: Division by zero`; `"6"/"3"` → `int(2)`; `"10"/"4"` → 2.5;
`PHP_INT_MIN / -1` → `float(9.223372036854776E+18)`; `6.0/3` → `float(2.0)`; `null / 1` → `int(0)`.
(`intdiv(1,0)` → `DivisionByZeroError: Division by zero`; `intdiv(PHP_INT_MIN,-1)` →
`ArithmeticError: Division of PHP_INT_MIN by -1 is not an integer`; `fdiv(1,0)` → INF.)

### 6.5 `%` — `mod_function`, `zend_operators.c:1541-1572`

```text
a = to_long_implicit(op1)?; b = to_long_implicit(op2)?     // zendi_try_get_long (§3.2.4): W/D diagnostics, TypeError on failure
if b == 0 { throw DivisionByZeroError("Modulo by zero") }   // after the operand diagnostics
if b == -1 { return Long(0) }                               // avoids INT_MIN % -1
return Long(a % b)                                          // C remainder: sign of the dividend
```
Verified: `7%3`=1, `-7%3`=-1, `7%-3`=1, `-7%-3`=-1, `7.0%3`=1, `7.9%3` → `{D: Implicit conversion from float 7.9 to
int loses precision} 1`, `7%3.9` → `{D: … float 3.9 …} 1`, `"7"%"3"`=1, `"7.5"%2` → `{D: Implicit conversion from
float-string "7.5" to int loses precision} 1`, `"7abc"%2` → `{W} 1`, `"abc"%2` → `TypeError: Unsupported operand
types: string % int`, `7%0`, `7%"0"`, `7%null` → `DivisionByZeroError: Modulo by zero`, `7%0.5` → `{D: … float 0.5 …}`
then `Modulo by zero`, `PHP_INT_MIN % -1` = 0, `1e20 % 3` → `{W: The float 1.0E+20 is not representable as an int,
cast occurred} 2`, `NAN % 3` → `{W: The float NAN …} {D: Implicit conversion from float NAN to int loses precision} 0`,
`INF % 3` → `{W} 0`, `true % 2` = 1, `"1e3" % 7` = 6, `"9.5e18" % 7` → `{D: … float-string "9.5e18" …} 0`
(saturated to PHP_INT_MAX), `[] % 2` → `array % int`, `7 % []` → `int % array`.

### 6.6 `**` — `pow_function_base`, `zend_operators.c:1344-1413`

```text
fn php_pow(a: Number, b: Number) -> Number:
  match (a, b):
    (Long(base), Long(exp)) if exp >= 0:
        if exp == 0 { return Long(1) }            // includes 0 ** 0
        if base == 0 { return Long(0) }
        let (mut l1, mut l2, mut i) = (1i64, base, exp);
        while i >= 1 {
            if i % 2 != 0 {
                i -= 1;
                match l1.checked_mul(l2) {
                    Some(v) => l1 = v,
                    None => return Double(((l1 as f64) * (l2 as f64)) * pow_c(l2 as f64, i as f64)),
                }
            } else {
                i /= 2;
                match l2.checked_mul(l2) {
                    Some(v) => l2 = v,
                    None => return Double((l1 as f64) * pow_c((l2 as f64) * (l2 as f64), i as f64)),
                }
            }
        }
        Long(l1)
    (Long(base), Long(exp)) /* exp < 0 */ => Double(safe_pow(base as f64, exp as f64))
    _ => Double(safe_pow(a as f64, b as f64))

fn safe_pow(x, y):                                 // :1349-1356
    if x == 0.0 && y < 0.0 { deprecated("Power of base 0 and negative exponent is deprecated") }
    pow_c(x, y)                                    // C library pow()
```
Notes:
* The overflow branches use the pre-update values (`i` already decremented/halved), exactly as above.
* Verified: a userland re-implementation of this loop using PHP's own `pow()` reproduces `$b ** $e` for all
  `b ∈ [-60, 60]`, `e ∈ [0, 80]` (9,801 cases, 0 mismatches), while **2,904 of the 7,680** float results differ
  from naive `pow((float)$b, (float)$e)` → the loop must be ported literally.
* `pow_c` is glibc 2.41's `pow` in the oracle. A Rust port using the Windows CRT `powf` mismatched 15/29,944
  cases (int-overflow and random float pairs); the `libm` crate 0.2.16 mismatched 1,210/29,944. For bit-exact
  results port glibc's `pow` (sysdeps/ieee754/dbl-64/e_pow.c, the ARM optimized-routines implementation) or
  accept rare last-bit differences (visible only with 17-digit output such as var_export/json). UNVERIFIED: that a
  port of glibc's e_pow.c is bit-identical (not tested).

Verified: `2**3`=8; `2**-1`=0.5; `2**63` → `float(9.223372036854776E+18)`; `2**62` → `int(4611686018427387904)`;
`(-2)**63` → `int(-9223372036854775808)`; `-2 ** 2` → -4 (`**` binds tighter than unary minus);
`2**0.5` → 1.4142135623730951; `0**0` → 1; `0**-1` and `0.0**-1` → `{D: Power of base 0 and negative exponent is
deprecated} float(INF)`; `(-8)**(1/3)` → NAN; `"2"**"3"` → 8; `"2"**"0.5"` → 1.4142135623730951; `10**20` →
`float(1.0E+20)`; `3**40` → `float(1.2157665459056929E+19)`; `7**22` → `int(3909821048582988049)`;
`(-1)**PHP_INT_MAX` → -1; `1**-1` → `float(1.0)`; `2.0**3` → `float(8.0)`; `null**2` → 0; `[]**2` → TypeError
`array ** int`; `pow(2,3)` → 8; `pow("2",3)` → 8.

### 6.7 Unary minus and plus — `zend_compile_unary_pm`, `Zend/zend_compile.c:10338-10357`

A constant operand is folded at compile time; otherwise the compiler emits **`ZEND_MUL expr, -1`** (minus) or
**`ZEND_MUL expr, 1`** (plus). All semantics and messages are therefore those of `*`.
Verified (runtime operands): `-"5"` → -5; `-"5.5"` → -5.5;
`-"abc"` → `TypeError: Unsupported operand types: string * int`;
`-"5abc"` → `{W} -5`; `-null` → 0; `-true` → -1; `-[]` → `array * int`; `+"abc"` → `string * int`;
`-$i` with `$i = 0` → `int(0)`; `-$f` with `$f = 0.0` → `float(-0.0)`; `-"0.0"` → `float(-0.0)`; `-"0"` → `int(0)`;
`-PHP_INT_MIN` → `float(9.223372036854776E+18)`; `-$obj` → `stdClass * int`.

### 6.8 Bitwise `& | ^ ~ << >>` (Twig exposes `b-and`, `b-or`, `b-xor`) — `zend_operators.c:1650-2015`

* `&`, `|`, `^` with **both operands strings** → bytewise operation on the strings (`|`: result length = longer,
  the tail of the longer string is copied; `&`, `^`: length = shorter). Verified `"5" | "3"` → `"7"`,
  `"abc" & "  "` → `"  "`, `"12" ^ "3"` → `"\x02"`.
* Otherwise both operands go through `zendi_try_get_long` (§3.2.4). Verified: `5 | "3"` → 7; `1.5 | 0` →
  `{D: … float 1.5 …} 1`; `"1.5" | 0` → `{D: … float-string "1.5" …} 1`; `"abc" | 1` → `TypeError: Unsupported
  operand types: string | int`; `"1abc" | 1` → `{W} 1`; `null | 1` → 1; `true ^ 1` → 0; `[] & 1` → `array & int`;
  `1e20 & 1` → `{W: The float 1.0E+20 …} 0`.
* `~`: int → bitwise not; float → `~zend_dval_to_lval_safe(d)` (`~1.5` → `{D} -2`); string → bytewise not;
  anything else → `TypeError("Cannot perform bitwise not on %s")` with the value name (`~null` → `… on null`,
  `~[]` → `… on array`, `~true` → `… on true`).
* `<<`/`>>`: count ≥ 64 → `<<` gives 0, `>>` gives 0 or -1 (sign); negative count →
  `ArithmeticError("Bit shift by negative number")`; `<<` is performed on unsigned bits (`1 << 63` →
  `PHP_INT_MIN`).

### 6.9 Logical operators

`!`, `&&`, `||`, `xor`, `and`, `or` convert operands with §3.1 and return bool; `?:` returns the left operand if it
is truthy, else the right; `??` tests `isset` (null/undefined) and never truthiness. Verified: `"0" && true` → false;
`"0.0" || false` → true; `[] xor true` → true; `null ?: "d"` → `"d"`; `"0" ?: "d"` → `"d"`; `0 ?? "d"` → 0.

### 6.10 Increment / decrement (not reachable from Twig syntax; for completeness) — `zend_operators.c:2613-2902`

| operand | `++` | `--` |
|---|---|---|
| int | +1, `PHP_INT_MAX` → float | −1, `PHP_INT_MIN` → float |
| float | +1.0 | −1.0 |
| null | `int(1)` | unchanged + `W: Decrement on type null has no effect, this will change in the next major version of PHP` |
| bool | unchanged + `W: Increment on type bool has no effect, this will change in the next major version of PHP` | same with "Decrement" |
| numeric string | numeric +1 (`"9"`→10, `"9.5"`→10.5, `"1e2"`→101.0) | numeric −1 |
| `""` | `"1"` + `D: Increment on non-numeric string is deprecated, use str_increment() instead` | `int(-1)` + `D: Decrement on empty string is deprecated as non-numeric` |
| other string | Perl-style alphanumeric increment + the same D (`"a"`→`"b"`, `"z"`→`"aa"`, `"Az"`→`"Ba"`, `"a9"`→`"b0"`, `"Zz"`→`"AAa"`, `"a-"`→`"a-"`, `"-a"`→`"-b"`) | unchanged + `D: Decrement on non-numeric string has no effect and is deprecated` |
| array / resource / std object | `TypeError("Cannot increment array")` (value name) | `TypeError("Cannot decrement …")` |

### 6.11 Concatenation `.` (Twig `~`) — `concat_function`, `zend_operators.c:2017-2163`

Both operands are converted with `zval_try_get_string_func` (§9): arrays → `"Array"` + warning, objects need
`__toString`, floats use `precision`. Result longer than the max string size → `Error("String size overflow")`.
Verified: `1 . 2` → `"12"`; `1.5 . ""` → `"1.5"`; `-0.0 . ""` → `"-0"`; `1e15 . ""` → `"1.0E+15"`;
`0.00001 . ""` → `"1.0E-5"`; `"x" . []` → `{W: Array to string conversion} "xArray"`;
`"x" . new stdClass` → `Error: Object of class stdClass could not be converted to string`.

---

## 7. Comparison operators

### 7.1 How the operators are compiled and what "uncomparable" means

| PHP | opcode / function | result from `c = zend_compare(a, b)` |
|---|---|---|
| `a == b` | `ZEND_IS_EQUAL` (`fast_equal_check_function`, `zend_operators.h:930-950`) | `c == 0` |
| `a != b` | `ZEND_IS_NOT_EQUAL` | `c != 0` |
| `a < b` | `ZEND_IS_SMALLER` | `c < 0` |
| `a <= b` | `ZEND_IS_SMALLER_OR_EQUAL` | `c <= 0` |
| `a > b` | **`ZEND_IS_SMALLER` with swapped operands** (`zend_compile.c:10293-10318`, constant folding `:10087-10092`) | `zend_compare(b, a) < 0` |
| `a >= b` | `ZEND_IS_SMALLER_OR_EQUAL` swapped | `zend_compare(b, a) <= 0` |
| `a <=> b` | `ZEND_SPACESHIP` → `compare_function` | `c` (always -1, 0 or 1) |

`ZEND_UNCOMPARABLE` is `1` (`zend_operators.h:463-467`) and is returned for **both** operand orders, so for an
uncomparable pair `<`, `<=`, `>`, `>=`, `==` are all false, `!=` is true, and `<=>` is 1 in both directions.
The VM fast paths (int/float pairs via C `==`/`<`, string pairs via `zend_fast_equal_strings`) give identical
results. Verified on the 30×30 matrix of §7.10: `a == b ⇔ (a <=> b) == 0`, `a < b ⇔ (a <=> b) == -1`,
`a > b ⇔ (b <=> a) == -1` — no exception.

### 7.2 `zend_compare` — port-ready (`zend_operators.c:2306-2462`)

```text
fn three_way(x: f64, y: f64) -> i32 { if x == y { 0 } else if x < y { -1 } else { 1 } }   // NaN → 1 (zend_portability.h:516)

fn zend_compare(a, b) -> i32 {
  let mut converted = false;
  loop { match (a, b) {
    (Long(x), Long(y))       => return x.cmp(y) as -1/0/1,
    (Double(x), Long(y))     => return three_way(x, y as f64),
    (Long(x), Double(y))     => return three_way(x as f64, y),
    (Double(x), Double(y))   => return three_way(x, y),
    (Array(x), Array(y))     => return compare_arrays(x, y),               // §7.6
    (Null|False, Null|False) | (True, True) => return 0,
    (Null, True)             => return -1,
    (True, Null)             => return 1,
    (String(x), String(y))   => return if same_ptr(x, y) { 0 } else { smart_strcmp(x, y) },   // §7.4 (shortcut not observable for strings)
    (Null, String(s))        => return if s.is_empty() { 0 } else { -1 },
    (String(s), Null)        => return if s.is_empty() { 0 } else { 1 },
    (Long(l), String(s))     => return compare_long_to_string(l, s),        // §7.3
    (String(s), Long(l))     => return -compare_long_to_string(l, s),
    (Double(d), String(s))   => return if d.is_nan() { 1 } else { compare_double_to_string(d, s) },
    (String(s), Double(d))   => return if d.is_nan() { 1 } else { -compare_double_to_string(d, s) },
    (Object, Null)           => return 1,
    (Null, Object)           => return -1,
    _ => {
      if a or b is a reference { deref; continue }
      if a is Object || b is Object {
          let (obj, other, obj_left) = if a is Object { (a, b, true) } else { (b, a, false) };
          if other is Object { if same_object(obj, other) { return 0 } }
          else if other is True/False {
              match obj.cast(_IS_BOOL) {                    // std objects and enums: true
                  Err => return if obj_left { 1 } else { -1 },
                  Ok(c) => return if obj_left { zend_compare(c, other) } else { zend_compare(other, c) },
              }
          }
          return obj.compare_handler(a, b)                  // §7.7 (std objects, enums, closures)
      }
      if !converted {
          if (a is Double && a.is_nan()) || (b is Double && b.is_nan()) {
              if tag(a) < IS_TRUE                { return -1 }    // null/false vs NaN
              else if a is True || b is True     { return 0 }     // NaN counts as truthy
              else if tag(b) < IS_TRUE           { return 1 }
              else if a is not Double            { a = to_number_silent(a); converted = true }
              else if b is not Double            { b = to_number_silent(b); converted = true }
          }
          else if tag(a) < IS_TRUE { return if is_true(b) { -1 } else { 0 } }   // null/false vs anything: bool compare
          else if a is True        { return if is_true(b) { 0 } else { 1 } }
          else if tag(b) < IS_TRUE { return if is_true(a) { 1 } else { 0 } }
          else if b is True        { return if is_true(a) { 0 } else { -1 } }
          else { a = to_number_silent(a); b = to_number_silent(b); converted = true;
                 if exception_pending { return 1 } }
      }
      else if a is Array { return 1 }       // array is greater than any non-array (non-null/bool) value
      else if b is Array { return -1 }
      else { unreachable }
    }
  }}
}

fn to_number_silent(v):           // _zendi_convert_scalar_to_number_silent, zend_operators.c:292-322
    null/false → Long(0); true → Long(1)
    String → is_numeric_string(s, allow_errors=true) value, or Long(0); NO warning
    Resource → Long(handle); Object → cast(_IS_NUMBER) or Long(1)
    Long/Double/Array → unchanged
```

Practical consequences:
* Any comparison with `null`/`false`/`true` on one side (other than null-vs-string and null-vs-object, which have
  dedicated arms) is a **boolean** comparison of the other operand's truthiness. `null == 0` true,
  `null < -1` **true** (false < true), `null == []` true, `"0" == false` true, `"a" == true` true,
  `-1 == true` true, `[0] == true` true.
* `null` vs string: equal only to `""`; `null == "0"` is **false** (`"0"` is non-empty), `null < "0"` true.
* array vs scalar (non-null/bool): the array is always greater (`[] > 0`, `[1] > 1000`, `"a" < [1]`,
  `[] == 0` false).
* resource vs number: compares the handle (`STDIN == 1` true, `STDIN == "1"` true).

### 7.3 Number vs string — `compare_long_to_string` / `compare_double_to_string` (`zend_operators.c:2260-2304`)

```text
fn compare_long_to_string(l: i64, s) -> i32:
    match is_numeric_string(s, allow_errors=false) {
        Long(sl)   => l.cmp(sl),
        Double(sd) => three_way(l as f64, sd),
        NotNumeric => normalize(zend_binary_strcmp(decimal(l), s)),
    }
fn compare_double_to_string(d: f64 /* not NaN */, s) -> i32:
    match is_numeric_string(s, false) {
        Long(sl)   => three_way(d, sl as f64),
        Double(sd) => three_way(d, sd),
        NotNumeric => normalize(zend_binary_strcmp(zend_double_to_str(d) /* precision 14 */, s)),
    }
fn zend_binary_strcmp(s1, s2) -> i32:    // zend_operators.c:3259-3272
    let r = memcmp(s1, s2, min(len1, len2)); if r != 0 { r } else { three_way(len1, len2) }
normalize(n) = n.signum()                // ZEND_NORMALIZE_BOOL, zend_portability.h:507-508
```
Verified (`a` vs `b`: `==`, `<=>`): `0` vs `""` → false, 1; `0` vs `"0"` → true, 0; `0` vs `"a"` → false, -1;
`1` vs `"1abc"` → false, -1; `1` vs `" 1"` / `"1 "` / `"1.0"` / `"01"` → true, 0; `10` vs `"1e1"` → true;
`PHP_INT_MAX` vs `"9223372036854775808"` → **true** (both 2^63 as double); `9007199254740993` vs
`"9007199254740992.0"` → true; `1.5` vs `"1.50"` → true; `1.5` vs `"abc"` → false, -1 (`"1.5"` < `"abc"`);
`0.1+0.2` vs `"0.3"` → false, 1; `0.3` vs `"0.3"` → true; `1e100` vs `"1e100"` → true; **`INF` vs `"INF"` → true**
(`"INF"` is non-numeric → string compare `"INF"` == `"INF"`); `-INF` vs `"-INF"` → true; `100` vs `"1e2abc"` →
false, -1; `42` vs `"42abc"` → false, -1; `0` vs `"00"` → true; `0` vs `" "` → false, 1; `1.0` vs `"1abc"` → false.

### 7.4 String vs string — `zendi_smart_strcmp` / `zendi_smart_streq` (`zend_operators.c:3389-3485`)

```text
fn smart_strcmp(s1, s2) -> i32:
    let (r1, o1) = is_numeric_string_ex(s1, false)           // o = overflow flag ±1 (only for integer-looking overflow)
    if r1 != NotNumeric {
      let (r2, o2) = is_numeric_string_ex(s2, false)
      if r2 != NotNumeric {
        if o1 != 0 && o1 == o2 && d1 - d2 == 0.0 { goto string_cmp }     // both integers overflowed to the same side
        if r1 is Double || r2 is Double {
            if r1 is not Double { if o2 != 0 { return -o2 }; d1 = l1 as f64 }
            else if r2 is not Double { if o1 != 0 { return o1 }; d2 = l2 as f64 }
            else if d1 == d2 && !d1.is_finite() { goto string_cmp }      // both ±INF of the same sign
            return normalize(d1 - d2)
        }
        return l1.cmp(l2)
      }
    }
  string_cmp:
    normalize(zend_binary_strcmp(s1, s2))
```
`smart_streq` (used by `==`) has the same structure, returning `false` in the two `return ±o` branches and
`d1 == d2` / `l1 == l2` otherwise. The VM's `zend_fast_equal_strings` (`zend_operators.h:919-928`) first returns
true for the same string, and uses plain byte equality when either first byte is `> '9'` (such strings cannot be
numeric) — results identical.

Verified string pairs (`==`, `<=>`, `<`, `>`):

| a | b | == | <=> |
|---|---|---|---|
| `"9223372036854775807"` | `"9223372036854775808"` | false | -1 |
| `"9223372036854775808"` | `"9223372036854775809"` | false | -1 (both overflow → bytewise) |
| `"9223372036854775808"` | `"9.2233720368547758E+18"` | true | 0 |
| `"1e1000"` | `"2e1000"` | false | -1 (both INF → bytewise) |
| `"1e1000"` | `"1e1000"` | true | 0 |
| `"-1e1000"` | `"-2e1000"` | false | -1 |
| `"abc"` | `"abd"` | false | -1 |
| `"a"` | `"B"` | false | 1 (bytes: `a`=0x61 > `B`=0x42) |
| `"Z"` | `"a"` | false | -1 |
| `"10"` | `"9"` | false | 1 (numeric) |
| `"10"` | `"9a"` | false | -1 (bytewise) |
| `"1e3"` | `"1000"` | true | 0 |
| `"0x10"` | `"16"` | false | -1 |
| `" 1"` / `"1 "` / `"01"` / `"1.0"` / `"1e0"` / `"+1"` | `"1"` | true | 0 |
| `"  1"` | `"1  "` | true | 0 |
| `"10"` | `"1e1"`; `"100"` vs `"1e2"` | true | 0 |
| `"abc"` | `"ABC"` | false | 1 |
| `""` | `"0"` | false | -1 |
| `"0.0"` / `"-0"` / `"-0.0"` | `"0"` | true | 0 |
| `".5"` | `"0.5"`; `"1."` vs `"1"` | true | 0 |
| `"abc"` | `"abcd"` | false | -1 |
| `"\0"` | `""` | false | 1 |
| `"1\0"` | `"1"` | false | 1 (NUL makes it non-numeric) |
| `"9007199254740993"` | `"9007199254740992"` | false | 1 (both fit in int64) |
| `"9007199254740993"` | `"9007199254740993.0"` | true | 0 (int → double loses the 1) |
| `"INF"` / `"NAN"` | itself | true | 0 (same bytes) |
| `"1e-1000"` | `"0"` | true | 0 (underflows to 0.0) |

### 7.5 `null`/bool rules

See the table rows of §7.10. In short: null ≡ false ≡ "" in bool context; true equals every truthy value;
null/false are less than every truthy value; null vs string compares to `""` bytewise (`null < "a"` true,
`null == "0"` false), but `false == "0"` true (bool comparison).

### 7.6 Arrays — `zend_compare_arrays` → `zend_hash_compare(ordered=0)` (`zend_hash.c:3320-3453`, `zend_operators.c:3494-3516`)

```text
fn compare_arrays(h1, h2) -> i32:                  // used by == != < <= > >= <=>
    if same_ptr(h1, h2) { return 0 }               // COW sharing shortcut (see §2.2)
    if h1.len() != h2.len() { return if h1.len() > h2.len() { 1 } else { -1 } }
    for (k, v1) in h1 (insertion order) {
        match h2.get(k) {                          // lookup by key, order of h2 irrelevant
            None     => return 1,                  // UNCOMPARABLE (both directions → 1)
            Some(v2) => { let r = zend_compare(v1, v2); if r != 0 { return r } }
        }
    }
    0
```
Recursion guard: if `h1` is already being compared → `Error("Nesting level too deep - recursive dependency?")`
(`zend_hash.c:3443-3446`).

Verified: `['a'=>1,'b'=>2] == ['b'=>2,'a'=>1]` true; `[1,2,3] <=> [1,2,4]` -1; `[1,2] <=> [1,2,3]` -1 (count first);
`[5] <=> [1,2]` -1; `['a'=>1] <=> ['b'=>1]` 1 and reversed 1, `<` false, `>` false, `==` false; `["1"] == [1]` true;
`[0=>"a"] == ["0"=>"a"]` true; `[1=>1,0=>0] == [0,1]` true; `[1=>2, 0=>1] <=> [1, 2]` 0; `[1, 3] <=> [1=>2, 0=>9]` -1;
`[[1]] == [[1]]` true; `[null] == [false]` true; `[null] == []` false; `[NAN] == [NAN]` false; `[] < [1]` true.

### 7.7 Objects, enums, closures

`zend_std_compare_objects` (`Zend/zend_object_handlers.c:2227-2361`):

```text
fn std_compare(o1, o2) -> i32:
  if exactly one side is an object:
      let target = tag(value);                           // never bool/null here (handled in zend_compare)
      match obj.cast(target) {                           // only __toString can succeed (target string)
          Ok(c) => return if obj_left { zend_compare(c, value) } else { zend_compare(value, c) },
          Err if target is Long or Double => {
              notice(format!("Object of class {} could not be converted to {}", class, "int"|"float"));
              c = Long(1) / Double(1.0); compare as above
          }
          Err => return if obj_left { 1 } else { -1 }     // string w/o __toString, array: object is greater
      }
  if same object → 0
  if different classes → 1 (UNCOMPARABLE)
  if neither object has a dynamic-property table (and not lazy):
      if class declares no properties → 0
      for each declared property slot in declaration order:
          both initialized → r = zend_compare(p1, p2); if r != 0 return r
          only one initialized → return 1
      return 0
  else return compare_arrays(properties(o1), properties(o2))    // count first, then by name
```
* **Enums** use `zend_objects_not_comparable` → always 1 except the identical case (same-object check in
  `zend_compare`). Verified: `Suit::Hearts == Suit::Hearts` true; `Hearts == Spades` false; `Hearts <=> Spades` and
  `Spades <=> Hearts` both 1; `Hearts == "H"` false, `Hearts <=> "H"` 1, `"H" <=> Hearts` 1; `Hearts == 1` false (no
  notice); `Hearts == true` true; `Hearts == null` false.
* **Closures** (`Zend/zend_closures.c:446-482`): uncomparable unless both are "fake closures" (first-class callable
  syntax / `Closure::fromCallable`) of the same function, scope and `$this`. Verified: same closure object `==` true;
  two different closures `==` false, `<=>` 1; `strlen(...) == strlen(...)` true but `===` false.
* Verified std objects: `new stdClass == new stdClass` true; after adding a property to one: false, `<=>` -1 / 1
  (count first); `new A == new A` true; `new A == new B` (same props, different class) false, `<=>` 1 both ways;
  `A(b=2) <=> A(b=3)` -1; `obj == 1` → `{N: Object of class stdClass could not be converted to int} true`;
  `obj == 2` → `{N} false`; `obj == 1.0` → `{N: … to float} true`; `obj == "x"` false, `obj <=> "x"` 1,
  `"x" <=> obj` -1; `obj == []` false, `obj <=> []` 1; `obj == true` true; `obj == null` false;
  class with `__toString` returning `"abc"`: `== "abc"` true, `<=> "abd"` -1, `== 0` → `{N: Object of class S could
  not be converted to int} false`; two instances of a property-less class: `==` true, `<` false.

### 7.8 NaN summary (verified)

| expression | result |
|---|---|
| `NAN == NAN`, `NAN === NAN` | false, false |
| `NAN != NAN`, `NAN !== NAN` | true, true |
| `NAN < 0`, `NAN > 0`, `NAN == 0`, `NAN <= 0`, `NAN >= 0`, `NAN != 0` | false, false, false, false, false, true |
| `NAN <=> 0`, `0 <=> NAN` | 1, 1 |
| `NAN == true` / `NAN == false` | true / false (no warning) |
| `null == NAN` / `null < NAN` / `NAN < null` | false / true / false |
| `NAN == "a"`, `NAN <=> "a"`, `"a" <=> NAN` | false, 1, 1 |
| `NAN <=> []` / `[] <=> NAN` | -1 / 1 |
| `$x = NAN; $x == $x` | false |
| `$n = [NAN]; $m = $n; $m == $n` / `===` | true / true (shared HashTable) |
| `[NAN] == [NAN]` | false |

### 7.9 Identity `===` / `!==` — `zend_is_identical` (`zend_operators.c:2479-2505`)

Different tags → false (`true` and `false` are different tags; `1 === 1.0` false). null/false/true → true.
int: `==`; float: C `==` (so `0.0 === -0.0` **true**, `NAN === NAN` false); string: same bytes (`"1" === "01"`
false); array: same HashTable, or same count and, **in order**, equal keys (int vs int, string bytes) and identical
values (`zend_hash_compare(…, ordered=1)`); object: same handle; resource: same resource.
Verified: `['a'=>1,'b'=>2] === ['b'=>2,'a'=>1]` false; `["1"] === [1]` false; `[1=>1,0=>0] === [0,1]` false;
`"1e3" === "1000"` false; `$o === $o` true; `new stdClass === new stdClass` false.

### 7.10 Verified comparison matrix

Cell = `a <=> b` (row `a`, column `b`) as `-`, `0`, `+`; a trailing `=` means `a == b` is true. Derive the rest
from §7.1: `a < b` ⇔ cell(a,b) is `-`; `a > b` ⇔ cell(b,a) is `-`. `{}` = `new stdClass`, `{a:1}` = stdClass with
one property. Building the matrix emitted only `N: Object of class stdClass could not be converted to int` and
`N: Object of class stdClass could not be converted to float` (for the int/float vs object cells).

```text
a\b    null   true   false  0      1      -1     0.0    1.5    NAN    INF    ""     "0"    "1"    "-1"   "a"    "abc"  "1abc" " 1"   "1 "   "1e0"  "01"   "0.0"  "1.5"  "INF"  []     [0]    [1]    [1,2]  {}     {a:1}
null   0=     -      0=     0=     -      -      0=     -      -      -      0=     -      -      -      -      -      -      -      -      -      -      -      -      -      0=     -      -      -      -      -
true   +      0=     +      +      0=     0=     +      0=     0=     0=     +      +      0=     0=     0=     0=     0=     0=     0=     0=     0=     0=     0=     0=     +      0=     0=     0=     0=     0=
false  0=     -      0=     0=     -      -      0=     -      -      -      0=     0=     -      -      -      -      -      -      -      -      -      -      -      -      0=     -      -      -      -      -
0      0=     -      0=     0=     -      +      0=     -      +      -      +      0=     -      +      -      -      -      -      -      -      -      0=     -      -      -      -      -      -      -      -
1      +      0=     +      +      0=     +      +      -      +      -      +      +      0=     +      -      -      -      0=     0=     0=     0=     +      -      -      -      -      -      -      0=     0=
-1     +      0=     +      -      -      0=     -      -      +      -      +      -      -      0=     -      -      -      -      -      -      -      -      -      -      -      -      -      -      -      -
0.0    0=     -      0=     0=     -      +      0=     -      +      -      +      0=     -      +      -      -      -      -      -      -      -      0=     -      -      -      -      -      -      -      -
1.5    +      0=     +      +      +      +      +      0=     +      -      +      +      +      +      -      -      -      +      +      +      +      +      0=     -      -      -      -      -      +      +
NAN    +      0=     +      +      +      +      +      +      +      +      +      +      +      +      +      +      +      +      +      +      +      +      +      +      -      -      -      -      +      +
INF    +      0=     +      +      +      +      +      +      +      0=     +      +      +      +      -      -      +      +      +      +      +      +      +      0=     -      -      -      -      +      +
""     0=     -      0=     -      -      -      -      -      +      -      0=     -      -      -      -      -      -      -      -      -      -      -      -      -      -      -      -      -      -      -
"0"    +      -      0=     0=     -      +      0=     -      +      -      +      0=     -      +      -      -      -      -      -      -      -      0=     -      -      -      -      -      -      -      -
"1"    +      0=     +      +      0=     +      +      -      +      -      +      +      0=     +      -      -      -      0=     0=     0=     0=     +      -      -      -      -      -      -      -      -
"-1"   +      0=     +      -      -      0=     -      -      +      -      +      -      -      0=     -      -      -      -      -      -      -      -      -      -      -      -      -      -      -      -
"a"    +      0=     +      +      +      +      +      +      +      +      +      +      +      +      0=     -      +      +      +      +      +      +      +      +      -      -      -      -      -      -
"abc"  +      0=     +      +      +      +      +      +      +      +      +      +      +      +      +      0=     +      +      +      +      +      +      +      +      -      -      -      -      -      -
"1abc" +      0=     +      +      +      +      +      +      +      -      +      +      +      +      -      -      0=     +      +      -      +      +      +      -      -      -      -      -      -      -
" 1"   +      0=     +      +      0=     +      +      -      +      -      +      +      0=     +      -      -      -      0=     0=     0=     0=     +      -      -      -      -      -      -      -      -
"1 "   +      0=     +      +      0=     +      +      -      +      -      +      +      0=     +      -      -      -      0=     0=     0=     0=     +      -      -      -      -      -      -      -      -
"1e0"  +      0=     +      +      0=     +      +      -      +      -      +      +      0=     +      -      -      +      0=     0=     0=     0=     +      -      -      -      -      -      -      -      -
"01"   +      0=     +      +      0=     +      +      -      +      -      +      +      0=     +      -      -      -      0=     0=     0=     0=     +      -      -      -      -      -      -      -      -
"0.0"  +      0=     +      0=     -      +      0=     -      +      -      +      0=     -      +      -      -      -      -      -      -      -      0=     -      -      -      -      -      -      -      -
"1.5"  +      0=     +      +      +      +      +      0=     +      -      +      +      +      +      -      -      -      +      +      +      +      +      0=     -      -      -      -      -      -      -
"INF"  +      0=     +      +      +      +      +      +      +      0=     +      +      +      +      -      -      +      +      +      +      +      +      +      0=     -      -      -      -      -      -
[]     0=     -      0=     +      +      +      +      +      +      +      +      +      +      +      +      +      +      +      +      +      +      +      +      +      0=     -      -      -      -      -
[0]    +      0=     +      +      +      +      +      +      +      +      +      +      +      +      +      +      +      +      +      +      +      +      +      +      +      0=     -      -      -      -
[1]    +      0=     +      +      +      +      +      +      +      +      +      +      +      +      +      +      +      +      +      +      +      +      +      +      +      +      0=     -      -      -
[1,2]  +      0=     +      +      +      +      +      +      +      +      +      +      +      +      +      +      +      +      +      +      +      +      +      +      +      +      +      0=     -      -
{}     +      0=     +      +      0=     +      +      -      +      -      +      +      +      +      +      +      +      +      +      +      +      +      +      +      +      +      +      +      0=     -
{a:1}  +      0=     +      +      0=     +      +      -      +      -      +      +      +      +      +      +      +      +      +      +      +      +      +      +      +      +      +      +      +      0=
```

Further verified loose-equality facts: `"1e3" == "1000"` true, `"0x1A" == 26` false, `"abc" == 0` false,
`null == false` true, `[] == false` true, `"0" == false` true, `"" == null` true, `"0" == null` false,
`0 == null` true, `"1" == "1.0"` true, `9007199254740993 == 9007199254740992.0` true,
`PHP_INT_MAX == (float)PHP_INT_MAX` true (`<=>` 0), `0 == -0.0` true, `INF <=> PHP_INT_MAX` 1.

---

## 8. Arrays (zend_hash), foreach, string offsets, count

### 8.1 Key normalisation by key type

PHP arrays have two key kinds: int (`zend_ulong` storage, interpreted as `i64`) and string (bytes).
Conversion happens in `zend_fetch_dimension_address_inner` / `slow_index_convert(_w)`
(`Zend/zend_execute.c:2636-2884`), in the array-literal opcode `ZEND_ADD_ARRAY_ELEMENT`
(`Zend/zend_vm_def.h:6265-6320`) and in `array_key_exists` (`ext/standard/array.c:7091-7130`).

| key value | resulting key | diagnostics (verified) |
|---|---|---|
| int | itself | |
| string | int if canonical decimal integer (§8.2), else the string | |
| true / false | 1 / 0 | |
| float | `zend_dval_to_lval_safe(d)` (§3.2.4) | fractional: `D: Implicit conversion from float 1.5 to int loses precision`; out of range: `W: The float 1.0E+20 is not representable as an int, cast occurred` (key = wrapped value 7766279631452241920); NaN: W **and** D, key 0; INF: W, key 0; `-0.0` → 0 silently |
| null | `""` | **PHP 8.5:** `D: Using null as an array offset is deprecated, use an empty string instead` (read, write, `isset`, `empty`, `??`, array literal `[null => 1]`); **no** deprecation for `unset($a[null])` |
| resource | its handle (int) | `W: Resource ID#%d used as offset, casting to integer (%d)` (`zend_execute.c:2597-2602`) |
| array / object / other | — | read/write/`??`: `TypeError: Cannot access offset of type %s on array`; `isset`/`empty`: `TypeError: Cannot access offset of type %s in isset or empty`; `unset`: `TypeError: Cannot unset offset of type %s on array` (`Zend/zend.c:1865-1885`, `%s` = `zend_zval_type_name`, e.g. `array`, `stdClass`) |

`array_key_exists($k, $a)`: string → symtable lookup (numeric strings normalised), int, float (D as above),
bool, resource (W), **null → `D: Using null as the key parameter for array_key_exists() is deprecated, use an empty
string instead`** and looks up `""`; an array key gives `TypeError: Cannot access offset of type array on array`
(verified; the `must be a valid array offset type` branch in `array.c:7126` is not what the compiled call reports).
`isset($a[$k])` is false for a `null` value while `array_key_exists` is true (verified).

### 8.2 Canonical integer strings — `_zend_handle_numeric_str(_ex)` (`Zend/zend_hash.h:435-451`, `Zend/zend_hash.c:3523-3560`)

```text
fn handle_numeric_str(key: &[u8]) -> Option<i64> {          // byte(i) = 0 past the end
    let first = byte(0);
    if first > b'9' { return None }
    let mut i = 0;
    if first < b'0' {                                        // only '-' may precede the digits
        if first != b'-' || !is_digit(byte(1)) { return None }
        i = 1;
    }
    if (key[i] == b'0' && key.len() > 1)                     // leading zero; NB compares the FULL length → "-0" rejected
        || key.len() - i > 19 { return None }                // > MAX_LENGTH_OF_LONG - 1 digits
    let mut idx: u64 = (key[i] - b'0') as u64;
    loop {
        i += 1;
        if i == key.len() {
            if key[0] == b'-' {
                if idx - 1 > i64::MAX as u64 { return None } // allows exactly 9223372036854775808 → i64::MIN
                return Some(0u64.wrapping_sub(idx) as i64);
            }
            if idx > i64::MAX as u64 { return None }
            return Some(idx as i64);
        }
        if !is_digit(key[i]) { return None }
        idx = idx * 10 + (key[i] - b'0') as u64;
    }
}
```
Verified (runtime string keys): `'0'`→0, `'1'`→1, `'-1'`→-1, `'9223372036854775807'`→int,
`'-9223372036854775808'`→int (PHP_INT_MIN); stay **strings**: `'-0'`, `'00'`, `'01'`, `'08'`, `'-01'`, `'1.5'`,
`'1.0'`, `' 1'`, `'1 '`, `'+1'`, `'9223372036854775808'`, `'-9223372036854775809'`, `'0x1'`, `'1e3'`, `''`, `'a'`,
`'-'`, `'0.0'`, `"1\0"`. This is **not** `is_numeric_string`: whitespace, `+`, leading zeros, decimals, exponents
all keep the string. Compile-time constant keys are normalised by the compiler with the same rule.

### 8.3 Next free element (`$a[] = …`, literal elements without key)

`HashTable.nNextFreeElement` starts at `ZEND_LONG_MIN` meaning "no int key yet" (`zend_hash.c:291`, `:305`).
`_zend_hash_index_add_or_update_i` (`zend_hash.c:1265-1405`):

```text
on append:  h = if next == i64::MIN { 0 } else { next }                  // :1274-1276
            insert with "add" semantics; if key h already exists (only possible once next has been clamped at
            i64::MAX) the caller throws Error("Cannot add element to the array as the next element is already occupied")
on every int-key insert (append or explicit) of key h:
            if h >= next { next = if h < i64::MAX { h + 1 } else { i64::MAX } }   // :1374-1376 (packed: :1315)
```
So negative keys participate (PHP >= 8.3): verified `[-5 => 'a', 'b']` → `[-5=>'a', -4=>'b']`;
`$a = [-5 => 'a']; $a[] = 'b'` → same; `[-10 => 'a', -20 => 'b', 'c']` → `-9 => 'c'`;
`[PHP_INT_MIN => 'a', 'b']` → `PHP_INT_MIN + 1 => 'b'`; `["-5" => 'a']` then append → `-4`; `[3=>'a', 1=>'b', 'c']`
→ `4 => 'c'`; `['x' => 'a', 'b']` → `0 => 'b'`; `[PHP_INT_MAX => 1]` then append →
`Error: Cannot add element to the array as the next element is already occupied`.
Not reset by `unset` (`[1,2,3]`, unset key 2, append → key 3); copied with the array; recomputed by
`array_pop`/`array_shift`/`array_splice`/`array_unshift` (verified: after `array_pop($a)` on `[1,2,3]`, append →
key 2; `array_shift` on `[5=>'a',6=>'b']` → `[0=>'b']`, append → 1). `array_fill(-5, 3, 'x')` → keys -5, -4, -3.
Spread `[...[1,2], ...['a'=>3, 5=>4]]` → `[0=>1, 1=>2, 'a'=>3, 2=>4]` (int keys renumbered).

### 8.4 Ordering

Arrays are insertion-ordered dictionaries. Updating an existing key keeps its position; `unset` + re-insert moves
the key to the end; integer keys are **not** sorted (verified: `$a[5]='x'; $a[1]='y'; $a[]='z'` → order 5, 1, 6).
Duplicate keys in a literal: last value wins, position of the first occurrence (`['a'=>1,'b'=>2,'a'=>3]` →
`['a'=>3,'b'=>2]`); `[true=>'a', "1"=>'b', 1.2=>'c']` → `[1 => 'c']` (+ D for 1.2).

### 8.5 Packed vs hash representation

`HT_IS_PACKED` arrays (keys 0..n-1 inserted in order) are an internal optimisation; converting to hash
(`zend_hash_packed_to_hash`) preserves order and `nNextFreeElement`. No observable difference was found or is
implied by the source for iteration order, comparison, var_dump or key handling. A port can use a single ordered
map (e.g. `IndexMap<Key, Value>` with tombstone-free deletion that preserves order) plus `next_free: i64`.

### 8.6 Value semantics and foreach

* Arrays are copied on assignment / parameter passing / return (COW). Modifying a copy never affects the
  original (verified nested: `$b = $a; $b[1][0] = 'x'` leaves `$a` intact). Objects inside are shared handles.
* `foreach ($arr as $k => $v)` (by value) iterates over the array **as it was when the loop started**; writes,
  appends and unsets of `$arr` inside the body do not affect the iteration (verified: loop over `[1,2,3]` that
  appends 4, sets `[1]=20`, unsets `[2]` in the first iteration still yields `0=1,1=2,2=3`).
* `foreach (… as &$v)` (by reference) iterates the live array: verified the same modifications yield
  `0=1,1=20,2=3,3=4`.
* Keys produced by foreach are the stored keys (int keys stay int: `['10'=>…]` yields `int(10)`).
* Plain objects iterate their properties visible from the calling scope (outside the class: public only,
  declaration order, then dynamic ones). `Traversable` objects use the iterator protocol; a finished generator
  cannot be iterated again: `Exception: Cannot traverse an already closed generator`. `iterator_to_array($gen)`
  keeps keys (later duplicates overwrite: `['a' => 2, 0 => 3]`), `iterator_to_array($gen, false)` renumbers.
  (Twig's `for` wraps non-iterables with `CoreExtension::ensureTraversable()` → `[]`.)

### 8.7 Array literal evaluation order

Elements are evaluated left to right; for each element the **key expression before the value expression**
(verified `[f('k1') => f('v1'), f('k2') => f('v2')]` prints `<k1><v1><k2><v2>`). Key conversion diagnostics are
emitted when the element is added (i.e. after the value is evaluated).

### 8.8 Reading missing keys / offsets on non-arrays

| expression | result |
|---|---|
| `$a['k']` missing | `W: Undefined array key "k"` → null (`zend_execute.c:2502-2505`) |
| `$a[5]` or `$a['5']` missing | `W: Undefined array key 5` → null (`:2497-2500`) |
| `$a['k'] ?? d`, `isset`, `empty` | no warning |
| `null[0]` | `W: Trying to access array offset on null` → null (`zend_execute.c:3203`, value name) |
| `5[0]` / `true['a']` | `W: Trying to access array offset on int` / `… on true` → null |
| `[$x, $y] = [1]` | `W: Undefined array key 1`, `$y = null` |
| `[$x] = "str"` | `W: Cannot use string as array`, `$x = null` (`zend_execute.c:3200`) |
| `[$x] = null` | silent, `$x = null` |
| `$obj['k']` on non-ArrayAccess | `Error: Cannot use object of type %s as array` (`zend_execute.c:1536`) |

### 8.9 String offsets (`$s[$i]`) — read path `zend_execute.c:3070-3175`, `zend_check_string_offset` `:1713-1756`

Offset conversion (read):
* int → itself.
* string → `is_numeric_string_ex(…, allow_errors=true)`; must yield **Long**: trailing data →
  `W: Illegal string offset "<raw>"` and the prefix is used; otherwise (non-numeric, or float-like such as `"1.0"`,
  `"1e3"`) → `TypeError: Cannot access offset of type string on string` (in `??`/`isset` context: null/false
  silently). Leading whitespace is accepted (`$s[" 1"]` → `'b'`).
* float, null, bool → `W: String offset cast occurred`, then float → `zend_dval_to_lval_silent`, null/bool → 0/1.
* array/object → `TypeError: Cannot access offset of type array on string`.

Range: negative offsets count from the end; out of range → `W: Uninitialized string offset %d` and `""`
(`?? / isset` → null/false). Result is a 1-byte string.

Verified with `$s = "abc"`: `$s[0]`→`a`, `$s[-1]`→`c`, `$s[3]` → `{W: Uninitialized string offset 3} ''`, `$s[-4]` →
`{W: Uninitialized string offset -4} ''`, `$s["1"]`→`b`, `$s["x"]` → TypeError, `$s["1x"]` →
`{W: Illegal string offset "1x"} 'b'`, `$s[" 1"]`→`b`, `$s["1.0"]` → TypeError, `$s[1.7]` →
`{W: String offset cast occurred} 'b'`, `$s[true]` → `{W} 'b'`, `$s[null]` → `{W} 'a'`, `$s[[]]` →
`TypeError: Cannot access offset of type array on string`, `$s[0][0]`→`a`, `isset($s[3])` false, `isset($s["x"])`
false, `isset($s["1"])` true, `isset($s["1.0"])` false, `$s["x"] ?? 'd'` → `'d'`, `$s[NAN]`/`$s[INF]` →
`{W} 'a'`, `$s[1e20]` → `{W: String offset cast occurred} {W: Uninitialized string offset 7766279631452241920} ''`,
`$s[-1.5]` → `{W} 'c'`, `isset($s[1.5])` → `{D: Implicit conversion from float 1.5 to int loses precision} true`.
Writing past the end pads with spaces (`$s[5] = 'x'` on `"abc"` → `"abc  x"`).

### 8.10 `count()`

`count($array)` = number of elements; `count($a, COUNT_RECURSIVE)` also counts nested elements
(`count([1,[2,3]], 1)` → 4). `Countable` objects → `->count()`. Anything else →
`TypeError: count(): Argument #1 ($value) must be of type Countable|array, string given` (`null given`, etc.).

---

## 9. String conversion of every type

`zval_get_string_func` / `zval_try_get_string_func` (`zend_operators.c:1066-1117`), `_convert_to_string`
(`:759-814`); used by `echo`, `print`, `.`, interpolation, `(string)`, `strval`, `settype`, `implode`, `sprintf('%s')`,
Twig `{{ }}`, weak `string` parameters.

| value | string | diagnostics |
|---|---|---|
| null | `""` | |
| false | `""` | |
| true | `"1"` | |
| int | decimal, `-` sign (`zend_long_to_str`, `:3532-3543`); `PHP_INT_MIN` → `"-9223372036854775808"`; `-0` literal is int 0 → `"0"` | |
| float | `zend_double_to_str` with `precision` = 14 (§5); `-0.0` → `"-0"`, `INF` → `"INF"`, `-INF` → `"-INF"` | NaN → `"NAN"` + `W: unexpected NAN value was coerced to string` |
| string | itself | |
| array | `"Array"` | `W: Array to string conversion` (`:791`, `:1083`) — once per conversion |
| object with `__toString` | its return value | (declared `string` return type enforced by the engine) |
| object without `__toString` (incl. enums, closures, stdClass) | — | `Error: Object of class %s could not be converted to string` (`:803`, `:1092`) |
| resource | `"Resource id #N"` | |

Verified: `(string)true`→`'1'`, `(string)false`→`''`, `(string)null`→`''`, `(string)-0`→`'0'`, `(string)1.0`→`'1'`,
`(string)-0.0`→`'-0'`, `(string)0.1`→`'0.1'`, `(string)1e25`→`'1.0E+25'`, `(string)NAN` → `{W} 'NAN'`,
`(string)[]` → `{W: Array to string conversion} 'Array'`, `"x" . []` → `{W} 'xArray'`, `"x{$a}"` with an array →
`{W} 'xArray'`, `implode(",", [1, [2], null, true, 1.5])` → `{W} '1,Array,,1,1.5'`, `sprintf("%s", [])` →
`{W} 'Array'`, `sprintf("%s", 1.0)` → `'1'`, `strval(E::A)` / `(string)E::A` → `Error: Object of class E could not be
converted to string`, `"x" . BE::One` (backed enum) → `Error: Object of class BE could not be converted to string`,
`(string)(function() {})` → `Error: Object of class Closure could not be converted to string`, `(string)STDIN` →
`'Resource id #1'`, `"$f|$g|$b|$n|$i"` with `1.0, 0.1+0.2, true, null, -0` → `'1|0.3|1||0'`.

`__toString` always has an (implicit) `string` return type, checked in coercive mode: a scalar return value is
coerced (verified: `return 5;` with or without `: string` → `"5"`), a non-coercible one throws
`TypeError: T3::__toString(): Return value must be of type string, array returned` (verified). The fallback message
`Method %s::__toString() must return a string value` (`Zend/zend_object_handlers.c:2578`) was not observed.

---

## 10. var_dump / var_export / print_r output formats

### 10.1 `var_dump` — `php_var_dump` (`ext/standard/var.c:118-246`)

`var_dump($a, $b, …)` dumps each argument at `level = 1`. Pseudocode with exact whitespace:

```text
fn var_dump(v, level):
    if level > 1 { out(" " * (level - 1)) }
    let amp = if v is a reference with refcount > 1 { "&" } else { "" }     // COMMON macro, :43, :233-239
    match v:
      false  → out(amp + "bool(false)\n");  true → out(amp + "bool(true)\n")
      null   → out(amp + "NULL\n")
      int    → out(amp + "int(" + decimal + ")\n")
      float  → out(amp + "float(" + gcvt(v, -1 /*serialize_precision*/) + ")\n")     // no ".0": float(1), float(-0), float(1.0E+25), float(NAN), float(INF), float(-INF)
      string → out(amp + "string(" + byte_len + ") \"" + raw bytes + "\"\n")          // no escaping at all
      array  → if being dumped already (recursion) { out("*RECURSION*\n"); return }
               out(amp + "array(" + count + ") {\n")
               for (k, x) in v:
                   out(" " * (level + 1))
                   out(match k { Int(i) → "[" + i + "]=>\n", Str(s) → "[\"" + raw s + "\"]=>\n" })
                   var_dump(x, level + 2)
               if level > 1 { out(" " * (level - 1)) }
               out("}\n")
      enum case → out(amp + "enum(" + Class + "::" + Case + ")\n")
      object → if recursive { out("*RECURSION*\n"); return }
               out(amp + prefix /* "" | "lazy ghost " | "lazy proxy " */ + "object(" + class + ")#" + handle + " (" + nprops + ") {\n")
               for each property (declared order, then dynamic):
                   out(" " * (level + 1))
                   key: int → "[" + i + "]=>\n";
                        public → "[\"name\"]=>\n"; protected → "[\"name\":protected]=>\n";
                        private → "[\"name\":\"DeclaringClass\":private]=>\n"
                   uninitialized typed property → out(" " * (level + 1) + "uninitialized(" + type + ")\n")
                   else var_dump(x, level + 2)
               if level > 1 { out(" " * (level - 1)) }
               out("}\n")
      resource → out(amp + "resource(" + id + ") of type (" + type_name + ")\n")
```
* `handle` is the object store handle (`#1`, `#2`, …), allocation-order dependent; a port must keep a per-request
  object id counter to reproduce it (UNVERIFIED how Twig fixtures depend on it; none of the dump fixtures read
  contains objects).
* Closures dump as `object(Closure)#N (k) { ["name"]=> string(…) "{closure:/path/file.php:LINE}" ["file"]=> …
  ["line"]=> int(…) ["parameter"]=> array(…) { ["$x"]=> string(10) "<required>" } }` (PHP 8.4+ format, verified).
* Twig fixture `tests/Fixtures/functions/dump_array.test` expects exactly:
  ```
  array(3) {
    ["foo"]=>
    string(3) "foo"
    ["bar"]=>
    string(3) "bar"
    ["global"]=>
    string(6) "global"
  }
  ```

Verified output (from `t8.php`, abbreviated where marked):

```text
array(12) {
  [0]=>
  int(1)
  ["a"]=>
  array(3) {
    [0]=>
    bool(true)
    [1]=>
    bool(false)
    [2]=>
    NULL
  }
  ["f"]=>
  float(1.5)
  ["e"]=>
  array(0) {
  }
  ["s"]=>
  string(14) "q'uo"te\ <NUL>nul
"
  [-5]=>
  float(-1.0E+25)
  ["big"]=>
  int(-9223372036854775808)
  ["o"]=>
  object(stdClass)#1 (3) {
    ["a"]=>
    int(1)
    ["b c"]=>
    array(2) {
      [0]=>
      float(1)
      [1]=>
      float(-0)
    }
    ["0"]=>
    string(4) "zero"
  }
  ["p"]=>
  object(P)#2 (4) {
    ["pub"]=>
    int(1)
    ["pro":protected]=>
    float(2.5)
    ["pri":"P":private]=>
    string(1) "x"
    ["typed"]=>
    uninitialized(?int)
    ["n"]=>
    NULL
  }
  ["en"]=>
  enum(Suit::Hearts)
  …
}
```
(`<NUL>` stands for the raw 0x00 byte.) Note the object header count `(4)` for `P` excludes the uninitialized
typed property but the property is still listed. References: `$r = [1]; $ref = &$r[0]; var_dump($r)` →
`[0]=>\n  &int(1)`. Recursive array: `[1]=>\n  *RECURSION*`. Scalars at top level: `int(1)`, `string(3) "two"`,
`float(3)`; `var_dump([])` → `array(0) {\n}`; `var_dump(new stdClass)` → `object(stdClass)#7 (0) {\n}`.

### 10.2 `var_export` — `php_var_export_ex` (`ext/standard/var.c:480-757`)

```text
fn export(v, level) -> String:                 // var_export($v) calls export(v, 1)
  match v:
    false → "false"; true → "true"; null → "NULL"
    int   → if v == i64::MIN { "-9223372036854775807-1" } else { decimal }          // :563-572
    float → gcvt(v, -1) + (if finite && no '.' in text { ".0" } else { "" })       // 1.0, -0.0, 0.1, 1.0E+25, NAN, INF, -INF
    string → "'" + escape(v) + "'"
    array → if recursive { warn("var_export does not handle circular references"); return "NULL" }
            s = ""
            if level > 1 { s += "\n" + " " * (level - 1) }
            s += "array (\n"
            for (k, x) in v:
                s += " " * (level + 1)
                s += match k { Int(i) → decimal(i), Str(t) → "'" + escape(t) + "'" }
                s += " => " + export(x, level + 2) + ",\n"
            if level > 1 { s += " " * (level - 1) }
            s += ")"
    object → if recursive { warn(same); return "NULL" }
            s = if level > 1 { "\n" + " " * (level - 1) } else { "" }
            stdClass → s += "(object) array(\n"
            enum     → s += "\\" + Class + "::" + Case              (no body, no closing text)
            other    → s += "\\" + Class + "::__set_state(array(\n"
            for each property (unmangled name; visibility dropped):
                s += " " * (level + 2) + (int key ? decimal : "'" + escape(name) + "'") + " => " + export(x, level + 2) + ",\n"
            if level > 1 && !enum { s += " " * (level - 1) }
            stdClass → s += ")";  enum → "";  other → s += "))"

fn escape(t) = addcslashes(t, "'\\")  then replace every "\0" with  "' . \"\\0\" . '"
```
Consequences: a nested array/object value starts on a new line, so the parent line ends with `"=> "` (**trailing
space**); object property lines are indented one space more than array element lines (`level + 2` vs `level + 1`);
the top level has no leading newline. Uninitialized typed properties are omitted.

Verified (with `cat -A`, `$` = end of line):

```text
array ($
  'a' => $
  array ($
    0 => 1,$
    1 => $
    array ($
      0 => 2,$
    ),$
  ),$
  'o' => $
  (object) array($
     'x' => $
    array ($
      0 => 1,$
    ),$
  ),$
  'e' => $
  array ($
  ),$
)$
```
More verified outputs: `var_export([])` → `array (\n)`; `var_export([[]])` → `array (\n  0 => \n  array (\n  ),\n)`;
`var_export(new stdClass)` → `(object) array(\n)`; `var_export("a\0b")` → `'a' . "\0" . 'b'`;
`var_export("\0")` → `'' . "\0" . ''`; `var_export(['k\'ey' => 'v', "nu\0l" => 1])` →
`array (\n  'k\'ey' => 'v',\n  'nu' . "\0" . 'l' => 1,\n)`; strings keep raw newlines (`'nul\n'` spans lines);
class instance → `\P::__set_state(array(\n     'pub' => 1,\n     'pro' => 2.5,\n     'pri' => 'x',\n     'n' => NULL,\n  ))`
(at level 3); enum → `\Suit::Hearts`; closure → `\Closure::__set_state(array(\n  ))`; `var_export(PHP_INT_MIN)` →
`-9223372036854775807-1`; `var_export(0.1 + 0.2)` → `0.30000000000000004`; `var_export(-INF)` → `-INF`.

### 10.3 `print_r` — `zend_print_zval_r_to_buf` / `print_hash` (`Zend/zend.c:378-640`)

Floats use `precision` (14) without `.0` (`zend.c:601`): `print_r(1.0)` → `1`, `print_r(0.1+0.2)` → `0.3`,
`print_r(-0.0)` → `-0`; `true` → `1`; `false`/`null` → empty. Arrays:

```text
Array
(
    [0] => 1
    [a] => Array
        (
            [0] => 1
            [1] => 
            [2] => 
        )

    [f] => 1.5
)
```
(verified; nested arrays are indented by 8 spaces per level, followed by a blank line; objects print as
`ClassName Object` with `[prop:protected]`, `[prop:Class:private]`; enums as `Suit Enum:string ( [name] => Hearts
[value] => H )` / `U Enum ( [name] => A )`.)

---

## 11. Coercive-mode parameter passing (internal and user functions)

Twig's PHP code never enables `strict_types`, so every call from Twig (including the runtime functions/filters in
`CoreExtension`) into PHP functions uses these rules. They are **stricter than casts**: strings must be fully
numeric, out-of-range floats are rejected.

### 11.1 Internal functions — `zend_parse_arg_*_weak` (`Zend/zend_API.c:504-820`)

| declared type | accepted values (weak mode) | rejected → TypeError |
|---|---|---|
| `bool` | null (+ null D), int, float, string → `zend_is_true` (`:533-547`) | array, object, resource |
| `int` | int; true/false → 1/0; null (+ null D) → 0; float if not NaN and fits int64 (fractional → `D: Implicit conversion from float %s to int loses precision`); **fully numeric** string (`is_numeric_str_function`, allow_errors=false, surrounding whitespace OK): int value, or float value under the same float rules with `D: Implicit conversion from float-string "%s" to int loses precision` (`:567-634`) | NaN, ±INF, out-of-range floats, leading-numeric or non-numeric strings, array, object, resource |
| `float` | int → `(double)`; fully numeric string; null (+ null D) → 0.0; bool → 0.0/1.0 (`:654-683`) | non-numeric strings, array, object |
| `string` | null (+ null D) → `""`; int/float/bool → string conversion (§9, floats with precision 14); object with `__toString` (`:761-786`) | array, objects without `__toString`, resource |
| `int\|float` | int, float; numeric string → int or float by `is_numeric_string`; null (+ null D) → 0; bool → 0/1 (`:698-728`) | non-numeric string, array, object |

Null deprecation (`zend_null_arg_deprecated`, `:504-531`), verbatim format:
`"%s(): Passing null to parameter #%u%s%s%s of type %s is deprecated"` → e.g.
`strlen(): Passing null to parameter #1 ($string) of type string is deprecated`. The type string comes from the
function's arginfo (e.g. `int|float` for `abs`, `floor`).

Type error (`zend_argument_type_error`): `"%s(): Argument #%d ($%s) must be of type %s, %s given"` with the
arginfo type and `zend_zval_value_name` of the value.

Verified: `str_repeat("a", 2.5)` → `{D: Implicit conversion from float 2.5 to int loses precision} 'aa'`;
`str_repeat("a", "2.5")` → `{D: Implicit conversion from float-string "2.5" to int loses precision} 'aa'`;
`str_repeat("a", " 2")` / `"2 "` / `"2"` → `'aa'`; `str_repeat("a", "1e1")` → 10 × `a`; `str_repeat("a", "2abc")`
→ `TypeError: str_repeat(): Argument #2 ($times) must be of type int, string given`; `str_repeat("a", null)` →
`{D: str_repeat(): Passing null to parameter #2 ($times) of type int is deprecated} ''`; `str_repeat("a", true)` →
`'a'`; `str_repeat("a", 1e20)` and `str_repeat("a", NAN)` → `… must be of type int, float given`;
`str_repeat("a", [])` → `… array given`; `str_repeat("a", new stdClass)` → `… stdClass given`;
`strlen(null)` → `{D: strlen(): Passing null to parameter #1 ($string) of type string is deprecated} 0`;
`strlen(1.5)` → 3; `strlen(1e25)` → 7 (`"1.0E+25"`); `strlen(true)` → 1; `strlen(false)` → 0; `strlen([])` →
`TypeError: strlen(): Argument #1 ($string) must be of type string, array given`; `strtoupper(new stdClass)` →
`… must be of type string, stdClass given`; `abs("5")` → 5; `abs("5.5")` → 5.5; `abs("x")` →
`TypeError: abs(): Argument #1 ($num) must be of type int|float, string given`; `abs(null)` →
`{D: abs(): Passing null to parameter #1 ($num) of type int|float is deprecated} 0`; `floor("abc")` →
`TypeError: floor(): Argument #1 ($num) must be of type int|float, string given`; `floor(null)` →
`{D: floor(): Passing null to parameter #1 ($num) of type int|float is deprecated} 0.0`; `substr("abc", "1")` /
`substr("abc", 1.0)` → `'bc'`; `implode(",", "x")` → `TypeError: implode(): Argument #2 ($array) must be of type
?array, string given`; `count("abc")` → `TypeError: count(): Argument #1 ($value) must be of type Countable|array,
string given`. Library functions using loose comparison follow §7: `in_array("1e1", ["10"])` true,
`in_array("abc", [0])` false, `in_array(null, [0])` true, `array_search("1", [0, 1])` → 1, `max("10", "9")` →
`"10"`, `max("abc", 0)` → `"abc"`, `max([1,2], 5)` → `[1,2]`.

### 11.2 User functions/methods with scalar type declarations — `zend_verify_scalar_type_hint` (`Zend/zend_execute.c:818-837`), `zend_verify_weak_scalar_type_hint` (`:733-779`)

* `null` is accepted only by nullable types (`?int`, `int|null`, untyped); **no null coercion** for user functions
  (that exception exists only for internal functions).
* Otherwise try, in order, **int → float → string → bool** (only the members present in the declared type):
  * int: if the type also contains float and the value is a string → `is_numeric_string` decides int vs float;
    else the internal `int` rules of §11.1 (with the same deprecations);
  * float: §11.1 float rules; string: §11.1 string rules; bool: §11.1 bool rules (only if both true and false are
    allowed, i.e. `bool`).
* Failure: `TypeError("%s(): Argument #%d ($%s) must be of type %s, %s given, called in %s on line %d")`.

Verified (`function fi(int $x)` etc.): `fi("1.5")` → `{D: … float-string "1.5" …} 1`; `fi(1.5)` → `{D: … float 1.5 …} 1`;
`fi("1e2")` → 100; `fi(" 1")` / `fi("1 ")` → 1; `fi("abc")` / `fi("1abc")` → `TypeError: fi(): Argument #1 ($x) must
be of type int, string given, called in /t/t16.php on line 52`; `fi(null)` → `… null given …`; `fi(true)` → 1;
`fi(1e20)` → `… float given …`; `fi([])` → `… array given …`; `ff(1)` → 1.0; `ff("1")` → 1.0; `ff("x")` → TypeError;
`fs(1.5)` → `'1.5'`; `fs(1.0)` → `'1'`; `fs(true)` → `'1'`; `fs(new stdClass)` → TypeError; `fb("0")` → false;
`fb(0.0)` → false; `fb([])` → TypeError; `fif("1.5")` (int|float) → 1.5; `fif("15")` → int 15; `fif("1e2")` →
float 100.0; `fif(true)` → int 1; `fif("x")` → TypeError `must be of type int|float`; `fsi(1.5)` (string|int) →
`{D: … float 1.5 …} int(1)`; `fsi(1.0)` → int 1; `fsi(true)` → int 1; `fni(null)` (`?int`) → null; `fni("")` →
`TypeError: … must be of type ?int, string given …`.

### 11.3 Related engine errors (verified, for reference)

| situation | message |
|---|---|
| internal function, too few/many args | `ArgumentCountError: strlen() expects exactly 1 argument, 0 given`; `str_repeat() expects exactly 2 arguments, 1 given`; `strlen() expects exactly 1 argument, 2 given` |
| user function, too few args | `ArgumentCountError: Too few arguments to function two(), 1 passed in /t/t19.php on line 7 and exactly 2 expected` |
| undefined function / method / class | `Error: Call to undefined function undefined_fn()`; `Error: Call to undefined method K::nope()`; `Error: Class "Nope" not found` |
| method call on non-object | `Error: Call to a member function m() on null` / `… on string` |
| property read on null | `W: Attempt to read property "p" on null` → null |
| undefined property | `W: Undefined property: stdClass::$x` → null |
| private property | `Error: Cannot access private property K::$priv` |
| undefined variable | `W: Undefined variable $undefinedVar` → null |
| dynamic property creation | `D: Creation of dynamic property C::$dyn is deprecated` |
| invalid callback | `TypeError: array_map(): Argument #1 ($callback) must be a valid callback or null, function "nope" not found or invalid function name` |
| value errors | `ValueError: str_repeat(): Argument #2 ($times) must be greater than or equal to 0`; `ValueError: range(): Argument #3 ($step) cannot be 0`; `ValueError: array_combine(): Argument #1 ($keys) and argument #2 ($values) must have the same number of elements` |

---

## 12. Diagnostic message catalogue (verbatim)

`%s`/`%d` placeholders as in the C source; "fmt_H" = `%.*H` with precision −1 (§5, e.g. `1.0E+20`, `NAN`).

| level / class | message | source | trigger |
|---|---|---|---|
| W | `A non-numeric value encountered` | `Zend/zend_operators.c:345`, `:415` | leading-numeric string in arithmetic / implicit-int context |
| TypeError | `Unsupported operand types: %s %s %s` | `zend_operators.c:1124` | non-numeric string, array (except `+` with two arrays), object, resource in arithmetic/bitwise; type names per `zend_zval_type_name` |
| DivisionByZeroError | `Division by zero` | `zend_operators.c:1536` | `/` by 0, 0.0, -0.0, "0", null, false |
| DivisionByZeroError | `Modulo by zero` | `zend_operators.c:1550` | `%` by anything converting to 0 |
| ArithmeticError | `Bit shift by negative number` | `zend_operators.c:1960`, `:1997` | `<<`, `>>` with negative count |
| TypeError | `Cannot perform bitwise not on %s` | `zend_operators.c:1692` | `~` on null/bool/array/object (value name) |
| D | `Power of base 0 and negative exponent is deprecated` | `zend_operators.c:1346` | `0 ** -1`, `0.0 ** -1.5`, `pow(0, -1)` |
| W | `The float %s is not representable as an int, cast occurred` | `zend_operators.c:945` | float NaN/±INF/out of range → int (casts, implicit contexts, array keys); `%s` = fmt_H |
| D | `Implicit conversion from float %s to int loses precision` | `zend_operators.c:937` | fractional (or NaN) float in implicit int context, array key, int parameter; `%s` = fmt_H |
| D | `Implicit conversion from float-string "%s" to int loses precision` | `zend_operators.c:941` | float-like numeric string in implicit int context / int parameter; `%s` = raw string |
| W | `unexpected NAN value was coerced to %s` | `zend_operators.c:954` | NaN → bool (`if`, `!`, `?:`, `(bool)`), string, array, object |
| W | `Object of class %s could not be converted to %s` | `zend_operators.c:236` | `(int)`/`(float)`/`intval` of an object (result 1) |
| N | `Object of class %s could not be converted to %s` | `Zend/zend_object_handlers.c:2258` | comparing an object with an int/float (object treated as 1) |
| Error | `Object of class %s could not be converted to string` | `zend_operators.c:803`, `:1092` | string conversion of an object without `__toString` (incl. enums, closures) |
| W | `Array to string conversion` | `zend_operators.c:791`, `:1083` | string conversion of an array (result `"Array"`) |
| Error | `String size overflow` | `zend_operators.c:2118` | concatenation result too long |
| D | `Increment on non-numeric string is deprecated, use str_increment() instead` | `zend_operators.c:2624` | `$s++` on non-numeric string |
| D | `Decrement on empty string is deprecated as non-numeric` | `zend_operators.c:2812` | `""--` |
| D | `Decrement on non-numeric string has no effect and is deprecated` | `zend_operators.c:2839` | `"a"--` |
| W | `Increment on type bool has no effect, this will change in the next major version of PHP` | `zend_operators.c:2759` | `$b++` on bool |
| W | `Decrement on type null has no effect, this will change in the next major version of PHP` | `zend_operators.c:2853` | `$n--` on null |
| W | `Decrement on type bool has no effect, this will change in the next major version of PHP` | `zend_operators.c:2866` | `$b--` on bool |
| TypeError | `Cannot increment %s` / `Cannot decrement %s` | `zend_operators.c:2789`, `:2896` | `++`/`--` on array/resource/std object |
| Error | `Nesting level too deep - recursive dependency?` | `Zend/zend_hash.c:3444`, `zend_object_handlers.c:2298` | comparing recursive arrays/objects |
| W | `Undefined array key %d` / `Undefined array key "%s"` | `Zend/zend_execute.c:2499`, `:2504` | reading a missing key |
| D | `Using null as an array offset is deprecated, use an empty string instead` | `zend_execute.c` `slow_index_convert(_w)` (`:2636-2797`), `Zend/zend_vm_def.h:6292` | null key (read, write, isset, empty, `??`, literal) — PHP 8.5 |
| D | `Using null as the key parameter for array_key_exists() is deprecated, use an empty string instead` | `ext/standard/array.c:7109` | `array_key_exists(null, …)` — PHP 8.5 |
| W | `Resource ID#%d used as offset, casting to integer (%d)` | `zend_execute.c:2600` | resource key |
| TypeError | `Cannot access offset of type %s on %s` | `Zend/zend.c:1881` | array/object key on array; non-numeric/float-like string offset or array offset on string |
| TypeError | `Cannot access offset of type %s in isset or empty` | `zend.c:1869` | illegal key in `isset`/`empty` |
| TypeError | `Cannot unset offset of type %s on %s` / Error `Cannot unset string offsets` | `zend.c:1874-1877` | `unset` with illegal key / on a string |
| Error | `Cannot add element to the array as the next element is already occupied` | `zend_execute.c:2594` | append after key `PHP_INT_MAX` |
| W | `Trying to access array offset on %s` | `zend_execute.c:3203` | `$x[…]` read on null/bool/int/float (value name) |
| W | `Cannot use %s as array` | `zend_execute.c:3200` | `[$a] = "str"` destructuring a non-null scalar |
| Error | `Cannot use object of type %s as array` | `zend_execute.c:1536` | `$obj[…]` on non-ArrayAccess object |
| W | `Uninitialized string offset %d` | `zend_execute.c:3152` | string offset out of range |
| W | `Illegal string offset "%s"` | `zend_execute.c:1728`, `:3087` | leading-numeric string used as string offset |
| W | `String offset cast occurred` | `zend_execute.c:1737`, `:1745`, `:3122` | float/bool/null used as string offset |
| W | `var_export does not handle circular references` | `ext/standard/var.c:593`, `:633` | recursive structure in var_export (prints `NULL`) |
| D | `%s(): Passing null to parameter #%u ($%s) of type %s is deprecated` | `Zend/zend_API.c:521-525` | null to non-nullable scalar parameter of an internal function |
| TypeError | `%s(): Argument #%d ($%s) must be of type %s, %s given` | `zend_API.c:308`, `:318`, `:1123` | internal function parameter type mismatch |
| TypeError | `%s(): Argument #%d ($%s) must be of type %s, %s given, called in %s on line %d` | `zend_execute.c:721-722` | user function parameter type mismatch |

---

## 13. Verification artefacts and Rust validation harnesses

All under `<scratch>\`
(scratch, may be cleaned):

| path | purpose |
|---|---|
| `phpt\run.sh` | runs `phpt\<file>.php` in `php:8.5-cli` (mounts the folder at `/t`) |
| `phpt\h.php` | harness: error handler printing `{W: …}`/`{D: …}`/`{N: …}`, `t(label, fn)` printing value or `!Exception: msg` |
| `phpt\t0.php`–`t27.php`, `t5b.php` | the probes quoted throughout (t1 float→int, t2 float formatting, t3 string casts/is_numeric, t4 arithmetic, t5/t5b comparison matrix, t6 arrays/objects comparison, t7 keys/next index/offsets, t8–t9 var_dump/var_export, t10 conversions, t11 precision ties/bitwise/increment, t12 foreach, t13–t14 dtoa quirk, t15 string comparisons, t16 parameter coercion, t17 ordering/concat, t18 -0 and loose equality, t19 misc errors, t20 leading-numeric arithmetic, t21 enums/resources, t22 exponent-sign quirk, t23 error ordering/INF truncation, t24 float string offsets, t25 null/float keys, t26 pow(0,-1)/~obj/unset string offset, t27 __toString return coercion) |
| `phpt\dbl.php` + `doubles.txt` → `doubles_php.txt` | PHP echo / var_export / precision-17 strings for generated doubles |
| `rustcheck\src\main.rs` | Rust emulation of `zend_gcvt` + dtoa modes 0/2 (`gen` and `check` subcommands); result: 0 mismatches on 152,127 + 162,127 values |
| `phpt\strtod.php` + `numstr.txt` → `numstr_php.txt`, `rustcheck2\src\main.rs` | `(float)$s` vs Rust `parse::<f64>()`: 0 mismatches on 59,294 strings |
| `phpt\pow.php` → `pow_php.txt`, `rustcheck3\src\main.rs` | `int ** int` loop port + float pow; mismatches only from libm (15 with Windows CRT, 1,210 with `libm` 0.2.16) |
| `phpt\pow2.php` | PHP-userland emulation of the `**` loop: 0 mismatches on 9,801 cases; 2,904/7,680 differ from naive `pow()` |

---

## 14. UNVERIFIED items, risks, and things outside this scope

UNVERIFIED / risks:
* **pow() bit-exactness**: matching glibc 2.41 `pow` exactly requires porting its implementation; a port of
  `sysdeps/ieee754/dbl-64/e_pow.c` was not tested. Differences are last-bit only.
* `zend_strtod` exponent clamp (`e > 19999` with very long mantissas) vs Rust `parse` — theoretical difference, not
  tested.
* Object handle numbers in `var_dump` (`#N`) depend on allocation order in the PHP process; reproducing them needs
  an object-id counter with PHP's reuse semantics (freed handles are reused) — not analysed.
* Rust toolchain dependence: exact-mode float formatting ties-to-even verified on rustc 1.102.0-nightly only.
* 32-bit `zend_long` builds are ignored (different `MAX_LENGTH_OF_LONG`, `ZEND_DOUBLE_FITS_LONG`, smart_str
  comparison branch `zend_operators.c:3446-3449`).
* Internal classes with custom handlers (GMP, `BCMath\Number`, SimpleXML, ArrayObject `cast`/`compare`/
  `do_operation`) are not covered.
* Locale: the paths documented here are locale-independent (`.` decimal point, ASCII case tables); `printf("%f")`
  uses the C locale decimal point (`main/snprintf.c:859`), which is `.` in the default CLI locale.

Outside this document's scope (other surveys): `sprintf`/`number_format`/`round` algorithms, sorting (`zend_sort`
stability and comparison flags), mbstring/iconv, dates (timelib), PCRE, JSON beyond the float format,
`range()`, `str_*` library functions, Twig-level `getAttribute` rules.
