//! `json_encode()`: ports of `PHP_FUNCTION(json_encode)` (PHPSRC/ext/json/json.c) and the encoder
//! (PHPSRC/ext/json/json_encoder.c: `php_json_encode_zval()`, `php_json_encode_array()`,
//! `php_json_escape_string()`, `php_json_encode_serializable_object()`), reference/php/stdlib.md
//! §11.
//!
//! Arrays and objects are walked with an explicit stack instead of C recursion, in the same order
//! and with the same per-container bookkeeping (depth check after the children, recursion guards,
//! `PARTIAL_OUTPUT_ON_ERROR` continuation), so nesting depth is not limited by the Rust stack.
//! PHP's own C-stack check (`php_json_check_stack_limit()`, a "Maximum stack depth exceeded" error
//! for nesting in the tens of thousands of levels) is therefore not reproduced.

use crate::diag::Diagnostics;
use crate::error::{PhpError, PhpResult, ThrowableClass};
use crate::float;
use crate::key::ArrayKey;
use crate::numeric;
use crate::object::{CallArgs, ClassKind, ObjectRef, PropertyPurpose};
use crate::param::{self, Arg};
use crate::string::PhpString;
use crate::value::{Number, Value};

use super::{check_alloc, check_arg_count};

/// `JSON_HEX_TAG`: `<` and `>` as `\u003C` / `\u003E`.
pub const JSON_HEX_TAG: i64 = 1;
/// `JSON_HEX_AMP`: `&` as `\u0026`.
pub const JSON_HEX_AMP: i64 = 2;
/// `JSON_HEX_APOS`: `'` as `\u0027`.
pub const JSON_HEX_APOS: i64 = 4;
/// `JSON_HEX_QUOT`: `"` as `\u0022`.
pub const JSON_HEX_QUOT: i64 = 8;
/// `JSON_FORCE_OBJECT`: lists as objects too.
pub const JSON_FORCE_OBJECT: i64 = 16;
/// `JSON_NUMERIC_CHECK`: numeric strings as numbers.
pub const JSON_NUMERIC_CHECK: i64 = 32;
/// `JSON_UNESCAPED_SLASHES`: `/` not escaped.
pub const JSON_UNESCAPED_SLASHES: i64 = 64;
/// `JSON_PRETTY_PRINT`: newlines and four-space indentation.
pub const JSON_PRETTY_PRINT: i64 = 128;
/// `JSON_UNESCAPED_UNICODE`: non-ASCII characters as UTF-8.
pub const JSON_UNESCAPED_UNICODE: i64 = 256;
/// `JSON_PARTIAL_OUTPUT_ON_ERROR`: substitute unencodable values instead of failing.
pub const JSON_PARTIAL_OUTPUT_ON_ERROR: i64 = 512;
/// `JSON_PRESERVE_ZERO_FRACTION`: `1.0` instead of `1`.
pub const JSON_PRESERVE_ZERO_FRACTION: i64 = 1024;
/// `JSON_UNESCAPED_LINE_TERMINATORS`: with `JSON_UNESCAPED_UNICODE`, U+2028/U+2029 unescaped.
pub const JSON_UNESCAPED_LINE_TERMINATORS: i64 = 2048;
/// `JSON_INVALID_UTF8_IGNORE`: drop invalid UTF-8.
pub const JSON_INVALID_UTF8_IGNORE: i64 = 1_048_576;
/// `JSON_INVALID_UTF8_SUBSTITUTE`: replace invalid UTF-8 with U+FFFD.
pub const JSON_INVALID_UTF8_SUBSTITUTE: i64 = 2_097_152;
/// `JSON_THROW_ON_ERROR`: throw `JsonException` instead of returning `false`.
pub const JSON_THROW_ON_ERROR: i64 = 4_194_304;

/// `PHP_JSON_PARSER_DEFAULT_DEPTH`
const DEFAULT_DEPTH: i64 = 512;

/// `php_json_error_code` (the `JSON_ERROR_*` constants).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum JsonError {
    /// `JSON_ERROR_NONE` (0)
    None,
    /// `JSON_ERROR_DEPTH` (1)
    Depth,
    /// `JSON_ERROR_STATE_MISMATCH` (2)
    StateMismatch,
    /// `JSON_ERROR_CTRL_CHAR` (3)
    CtrlChar,
    /// `JSON_ERROR_SYNTAX` (4)
    Syntax,
    /// `JSON_ERROR_UTF8` (5)
    Utf8,
    /// `JSON_ERROR_RECURSION` (6)
    Recursion,
    /// `JSON_ERROR_INF_OR_NAN` (7)
    InfOrNan,
    /// `JSON_ERROR_UNSUPPORTED_TYPE` (8)
    UnsupportedType,
    /// `JSON_ERROR_INVALID_PROPERTY_NAME` (9)
    InvalidPropertyName,
    /// `JSON_ERROR_UTF16` (10)
    Utf16,
    /// `JSON_ERROR_NON_BACKED_ENUM` (11)
    NonBackedEnum,
}

impl JsonError {
    /// The `JSON_ERROR_*` value (`json_last_error()`, `JsonException::getCode()`).
    pub fn code(self) -> i64 {
        match self {
            JsonError::None => 0,
            JsonError::Depth => 1,
            JsonError::StateMismatch => 2,
            JsonError::CtrlChar => 3,
            JsonError::Syntax => 4,
            JsonError::Utf8 => 5,
            JsonError::Recursion => 6,
            JsonError::InfOrNan => 7,
            JsonError::UnsupportedType => 8,
            JsonError::InvalidPropertyName => 9,
            JsonError::Utf16 => 10,
            JsonError::NonBackedEnum => 11,
        }
    }

    /// Port of `php_json_get_error_msg()` (`json_last_error_msg()`, the `JsonException` message).
    pub fn message(self) -> &'static str {
        match self {
            JsonError::None => "No error",
            JsonError::Depth => "Maximum stack depth exceeded",
            JsonError::StateMismatch => "State mismatch (invalid or malformed JSON)",
            JsonError::CtrlChar => "Control character error, possibly incorrectly encoded",
            JsonError::Syntax => "Syntax error",
            JsonError::Utf8 => "Malformed UTF-8 characters, possibly incorrectly encoded",
            JsonError::Recursion => "Recursion detected",
            JsonError::InfOrNan => "Inf and NaN cannot be JSON encoded",
            JsonError::UnsupportedType => "Type is not supported",
            JsonError::InvalidPropertyName => "The decoded property name is invalid",
            JsonError::Utf16 => "Single unpaired UTF-16 surrogate in unicode escape",
            JsonError::NonBackedEnum => "Non-backed enums have no default serialization",
        }
    }
}

/// Result of [`json_encode_value`]: the encoder's output buffer and its error code.
#[derive(Clone, Debug, PartialEq)]
pub struct JsonEncoded {
    /// The text produced (complete when `error` is [`JsonError::None`], partial otherwise; with
    /// `JSON_PARTIAL_OUTPUT_ON_ERROR` this is what `json_encode()` returns).
    pub output: PhpString,
    /// The last error recorded (`json_last_error()`); [`JsonError::None`] on success.
    pub error: JsonError,
}

/// Port of `PHP_FUNCTION(json_encode)` (`json_encode(mixed $value, int $flags = 0, int $depth =
/// 512): string|false`, the `json_encode` filter). `None` is an omitted argument.
///
/// Returns the JSON text, or `false` when an error was recorded (unless
/// `JSON_PARTIAL_OUTPUT_ON_ERROR`); with `JSON_THROW_ON_ERROR` (and without partial output) an
/// error throws `JsonException` with [`JsonError::message`]. `$depth` outside the C int range
/// throws `ValueError: json_encode(): Argument #3 ($depth) must be between -2147483648 and
/// 2147483647`; the flags are truncated to a C int for the encoder, like upstream.
pub fn json_encode(
    value: &Value,
    flags: Option<&Value>,
    depth: Option<&Value>,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Value> {
    let options = match flags {
        Some(v) => param::parse_long(v, &Arg::new("json_encode", 2, "flags"), diag)?,
        None => 0,
    };
    let depth = match depth {
        Some(v) => param::parse_long(v, &Arg::new("json_encode", 3, "depth"), diag)?,
        None => DEFAULT_DEPTH,
    };
    if depth < i64::from(i32::MIN) || depth > i64::from(i32::MAX) {
        return Err(Arg::new("json_encode", 3, "depth").value_error(&format!(
            "must be between {} and {}",
            i32::MIN,
            i32::MAX
        )));
    }

    let encoded = json_encode_value(value, options as i32, depth as i32, diag)?;

    if options & JSON_THROW_ON_ERROR == 0 || options & JSON_PARTIAL_OUTPUT_ON_ERROR != 0 {
        if encoded.error != JsonError::None && options & JSON_PARTIAL_OUTPUT_ON_ERROR == 0 {
            return Ok(Value::Bool(false));
        }
    } else if encoded.error != JsonError::None {
        return Err(PhpError::new(
            ThrowableClass::JsonException,
            encoded.error.message(),
        ));
    }
    Ok(Value::String(encoded.output))
}

/// [`json_encode`] called with an argument list (the `json_encode` filter is the bare PHP
/// function, so extra filter arguments reach it): one to three arguments, else
/// `ArgumentCountError: json_encode() expects at most 3 arguments, 4 given`.
pub fn json_encode_call(args: &[Value], diag: &mut dyn Diagnostics) -> PhpResult<Value> {
    check_arg_count("json_encode", args, 1, Some(3))?;
    json_encode(&args[0], args.get(1), args.get(2), diag)
}

/// Port of `php_json_encode_zval()` with a fresh encoder (`php_json_encode_init()`,
/// `encoder.max_depth = depth`): encodes `value` with the `JSON_*` bits of `options` (a C int).
///
/// PHP exceptions raised during encoding (by `jsonSerialize()` methods, property handlers or the
/// diagnostics sink) are returned as `Err`; encoder errors are reported in
/// [`JsonEncoded::error`].
pub fn json_encode_value(
    value: &Value,
    options: i32,
    depth: i32,
    diag: &mut dyn Diagnostics,
) -> PhpResult<JsonEncoded> {
    let mut encoder = Encoder {
        buf: Vec::new(),
        depth: 0,
        max_depth: depth,
        error: JsonError::None,
        options: i64::from(options),
        serializing: Vec::new(),
        protected: Vec::new(),
        diag,
    };
    encoder.run(value)?;
    Ok(JsonEncoded {
        output: PhpString::from(encoder.buf),
        error: encoder.error,
    })
}

/// The `JSON_*` flags tested by the encoder.
const PARTIAL: i64 = JSON_PARTIAL_OUTPUT_ON_ERROR;
const PRETTY: i64 = JSON_PRETTY_PRINT;

/// `static const char digits[] = "0123456789abcdef";`
const DIGITS: &[u8; 16] = b"0123456789abcdef";

/// One array or object being written (the locals of a `php_json_encode_array()` call).
struct Container {
    /// Elements in order; objects: the property table without uninitialised properties.
    entries: Vec<(ArrayKey, Value)>,
    next: usize,
    encode_as_object: bool,
    /// `Z_TYPE_P(val) == IS_OBJECT`: mangled (non-public) names are skipped.
    is_object: bool,
    need_comma: bool,
    /// The object whose property table is protected against recursion
    /// (`PHP_JSON_HASH_PROTECT_RECURSION`).
    protected: Option<u64>,
}

/// A suspended step of the C recursion.
enum Frame {
    Container(Container),
    /// The result of `jsonSerialize()` being encoded while the object's JSON recursion guard is
    /// set (`ZEND_GUARD_PROTECT_RECURSION(guard, JSON)`).
    Serialized(u64),
}

/// How encoding a value started.
enum Step {
    /// Finished with `SUCCESS` (`true`) or `FAILURE` (`false`).
    Done(bool),
    /// A container frame was pushed.
    Pushed,
    /// A frame was pushed and this value must be encoded next (a tail call of the C code).
    Then(Value),
}

/// `php_json_encoder` plus the recursion guards the engine keeps on objects.
struct Encoder<'d> {
    buf: Vec<u8>,
    depth: i32,
    max_depth: i32,
    error: JsonError,
    options: i64,
    /// Objects inside their `jsonSerialize()` encoding (JSON recursion guard).
    serializing: Vec<u64>,
    /// Objects whose properties are being encoded (`GC_PROTECT_RECURSION`).
    protected: Vec<u64>,
    diag: &'d mut dyn Diagnostics,
}

impl Encoder<'_> {
    fn has(&self, flag: i64) -> bool {
        self.options & flag != 0
    }

    fn pretty_print_char(&mut self, c: u8) {
        if self.has(PRETTY) {
            self.buf.push(c);
        }
    }

    fn pretty_print_indent(&mut self) {
        if self.has(PRETTY) {
            for _ in 0..self.depth.max(0) {
                self.buf.extend_from_slice(b"    ");
            }
        }
    }

    /// Drives the encoding of `root` (the C recursion, unrolled).
    fn run(&mut self, root: &Value) -> PhpResult<bool> {
        let mut stack: Vec<Frame> = Vec::new();
        let mut done: Option<bool> = match self.start(root, &mut stack)? {
            Step::Done(status) => Some(status),
            Step::Pushed => None,
            Step::Then(v) => self.start_chain(v, &mut stack)?,
        };
        loop {
            check_alloc(self.buf.len())?;
            match done.take() {
                Some(status) => match stack.last_mut() {
                    None => return Ok(status),
                    Some(Frame::Serialized(id)) => {
                        let id = *id;
                        stack.pop();
                        self.unguard_serializing(id);
                        done = Some(status);
                    }
                    Some(Frame::Container(c)) => {
                        if !status && !self.has(PARTIAL) {
                            let protected = c.protected;
                            stack.pop();
                            self.unprotect(protected);
                            done = Some(false);
                        }
                    }
                },
                None => {
                    let Some(Frame::Container(c)) = stack.last_mut() else {
                        unreachable!("only containers wait for their next element");
                    };
                    if c.next >= c.entries.len() {
                        let Some(Frame::Container(c)) = stack.pop() else {
                            unreachable!()
                        };
                        done = Some(self.finish_container(c));
                        continue;
                    }
                    let (key, data) = c.entries[c.next].clone();
                    c.next += 1;
                    let (encode_as_object, is_object) = (c.encode_as_object, c.is_object);
                    if !encode_as_object {
                        let need_comma = std::mem::replace(&mut c.need_comma, true);
                        if need_comma {
                            self.buf.push(b',');
                        }
                        self.pretty_print_char(b'\n');
                        self.pretty_print_indent();
                    } else {
                        match &key {
                            ArrayKey::Str(k) => {
                                if is_object && k.first() == Some(&0) {
                                    // Skip protected and private members.
                                    continue;
                                }
                                let need_comma = std::mem::replace(&mut c.need_comma, true);
                                if need_comma {
                                    self.buf.push(b',');
                                }
                                self.pretty_print_char(b'\n');
                                self.pretty_print_indent();
                                if !self.escape_string(k, self.options & !JSON_NUMERIC_CHECK) {
                                    if !self.has(PARTIAL) {
                                        let Some(Frame::Container(c)) = stack.pop() else {
                                            unreachable!()
                                        };
                                        self.unprotect(c.protected);
                                        done = Some(false);
                                        continue;
                                    }
                                    let len = self.buf.len();
                                    self.buf.truncate(len - 4);
                                    self.buf.extend_from_slice(b"\"\"");
                                }
                            }
                            ArrayKey::Int(index) => {
                                let need_comma = std::mem::replace(&mut c.need_comma, true);
                                if need_comma {
                                    self.buf.push(b',');
                                }
                                self.pretty_print_char(b'\n');
                                self.pretty_print_indent();
                                self.buf.push(b'"');
                                self.buf.extend_from_slice(index.to_string().as_bytes());
                                self.buf.push(b'"');
                            }
                        }
                        self.buf.push(b':');
                        self.pretty_print_char(b' ');
                    }
                    done = match self.start(&data, &mut stack)? {
                        Step::Done(status) => Some(status),
                        Step::Pushed => None,
                        Step::Then(v) => self.start_chain(v, &mut stack)?,
                    };
                }
            }
        }
    }

    /// Follows [`Step::Then`] links (a `jsonSerialize()` result that is itself
    /// `JsonSerializable`, ...) without recursion.
    fn start_chain(&mut self, mut v: Value, stack: &mut Vec<Frame>) -> PhpResult<Option<bool>> {
        loop {
            match self.start(&v, stack)? {
                Step::Done(status) => return Ok(Some(status)),
                Step::Pushed => return Ok(None),
                Step::Then(next) => v = next,
            }
        }
    }

    fn unprotect(&mut self, protected: Option<u64>) {
        if let Some(id) = protected
            && let Some(pos) = self.protected.iter().rposition(|&p| p == id)
        {
            self.protected.remove(pos);
        }
    }

    fn unguard_serializing(&mut self, id: u64) {
        if let Some(pos) = self.serializing.iter().rposition(|&p| p == id) {
            self.serializing.remove(pos);
        }
    }

    /// The end of `php_json_encode_array()`: unprotect, depth check, closing bracket.
    fn finish_container(&mut self, c: Container) -> bool {
        self.unprotect(c.protected);
        if self.depth > self.max_depth {
            self.error = JsonError::Depth;
            if !self.has(PARTIAL) {
                return false;
            }
        }
        self.depth -= 1;
        // Only keep closing bracket on same line for empty arrays/objects
        if c.need_comma {
            self.pretty_print_char(b'\n');
            self.pretty_print_indent();
        }
        self.buf.push(if c.encode_as_object { b'}' } else { b']' });
        true
    }

    /// Port of `php_json_encode_zval()` (scalars inline; containers and `jsonSerialize()` results
    /// continue in [`Encoder::run`]).
    fn start(&mut self, val: &Value, stack: &mut Vec<Frame>) -> PhpResult<Step> {
        Ok(match val {
            Value::Null => {
                self.buf.extend_from_slice(b"null");
                Step::Done(true)
            }
            Value::Bool(true) => {
                self.buf.extend_from_slice(b"true");
                Step::Done(true)
            }
            Value::Bool(false) => {
                self.buf.extend_from_slice(b"false");
                Step::Done(true)
            }
            Value::Int(l) => {
                self.buf.extend_from_slice(l.to_string().as_bytes());
                Step::Done(true)
            }
            Value::Float(d) => {
                if d.is_finite() {
                    self.encode_double(*d);
                } else {
                    self.error = JsonError::InfOrNan;
                    self.buf.push(b'0');
                }
                Step::Done(true)
            }
            Value::String(s) => Step::Done(self.escape_string(s, self.options)),
            Value::Object(o) => {
                if o.instance_of("JsonSerializable") {
                    return self.encode_serializable_object(o, stack);
                }
                if o.class().is_enum() {
                    return self.encode_serializable_enum(o, stack);
                }
                // Non-serializable object
                self.start_container(val, stack)?
            }
            Value::Array(_) => self.start_container(val, stack)?,
        })
    }

    /// Port of `php_json_encode_serializable_object()`.
    fn encode_serializable_object(
        &mut self,
        obj: &ObjectRef,
        stack: &mut Vec<Frame>,
    ) -> PhpResult<Step> {
        let id = obj.id();
        if self.serializing.contains(&id) {
            self.error = JsonError::Recursion;
            if self.has(PARTIAL) {
                self.buf.extend_from_slice(b"null");
            }
            return Ok(Step::Done(false));
        }

        self.serializing.push(id);
        let retval = match obj.invoke_method("jsonserialize", CallArgs::new(), self.diag) {
            Ok(v) => v,
            Err(e) => {
                // An exception has occurred
                self.unguard_serializing(id);
                return Err(e);
            }
        };

        if let Value::Object(r) = &retval
            && ObjectRef::ptr_eq(r, obj)
        {
            // Handle the case where jsonSerialize does: return $this; by going straight to encode
            // array
            self.unguard_serializing(id);
            return self.start_container(&retval, stack);
        }
        // All other types, encode as normal (the guard stays until the result is written)
        stack.push(Frame::Serialized(id));
        Ok(Step::Then(retval))
    }

    /// Port of `php_json_encode_serializable_enum()`.
    fn encode_serializable_enum(
        &mut self,
        obj: &ObjectRef,
        stack: &mut Vec<Frame>,
    ) -> PhpResult<Step> {
        if matches!(obj.class().kind(), ClassKind::Enum(None)) {
            self.error = JsonError::NonBackedEnum;
            self.buf.push(b'0');
            return Ok(Step::Done(false));
        }
        let value = obj.enum_case_value().cloned().unwrap_or(Value::Null);
        self.start(&value, stack)
    }

    /// The start of `php_json_encode_array()`: entries, recursion check, opening bracket.
    fn start_container(&mut self, val: &Value, stack: &mut Vec<Frame>) -> PhpResult<Step> {
        let mut encode_as_object = self.has(JSON_FORCE_OBJECT);
        let (entries, is_object, protected) = match val {
            Value::Array(a) => {
                encode_as_object = encode_as_object || !a.is_list();
                let entries = a.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                (entries, false, None)
            }
            Value::Object(o) => {
                let id = o.id();
                if self.protected.contains(&id) {
                    self.error = JsonError::Recursion;
                    self.buf.extend_from_slice(b"null");
                    return Ok(Step::Done(false));
                }
                let table = o.properties_for(PropertyPurpose::Json, self.diag)?;
                let entries = table
                    .into_iter()
                    .filter_map(|e| e.value.map(|v| (e.key, v)))
                    .collect();
                encode_as_object = true;
                self.protected.push(id);
                (entries, true, Some(id))
            }
            _ => unreachable!("only arrays and objects are containers"),
        };

        self.buf.push(if encode_as_object { b'{' } else { b'[' });
        self.depth += 1;
        stack.push(Frame::Container(Container {
            entries,
            next: 0,
            encode_as_object,
            is_object,
            need_comma: false,
            protected,
        }));
        Ok(Step::Pushed)
    }

    /// Port of `php_json_encode_double()`: `serialize_precision` (-1) digits with `e`, plus `.0`
    /// with `JSON_PRESERVE_ZERO_FRACTION`.
    fn encode_double(&mut self, d: f64) {
        let s = float::format_json(d, self.has(JSON_PRESERVE_ZERO_FRACTION));
        self.buf.extend_from_slice(s.as_bytes());
    }

    /// Port of `php_json_escape_string()`: `false` = `FAILURE` (invalid UTF-8 without
    /// `JSON_INVALID_UTF8_*`; the buffer is reset to where the string started, plus `null` with
    /// partial output).
    fn escape_string(&mut self, s: &[u8], options: i64) -> bool {
        if s.is_empty() {
            self.buf.extend_from_slice(b"\"\"");
            return true;
        }

        if options & JSON_NUMERIC_CHECK != 0 {
            match numeric::is_numeric_string(s, false) {
                Some(Number::Int(p)) => {
                    self.buf.extend_from_slice(p.to_string().as_bytes());
                    return true;
                }
                Some(Number::Float(d)) if d.is_finite() => {
                    self.encode_double(d);
                    return true;
                }
                _ => {}
            }
        }
        let checkpoint = self.buf.len();
        let has = |flag: i64| options & flag != 0;

        self.buf.reserve(s.len() + 2);
        self.buf.push(b'"');

        let mut s = s;
        let mut len = s.len();
        let mut pos = 0usize;
        loop {
            let us = s[pos];
            if !needs_escape(us) {
                pos += 1;
                len -= 1;
                if len == 0 {
                    self.buf.extend_from_slice(&s[..pos]);
                    break;
                }
            } else {
                if pos > 0 {
                    self.buf.extend_from_slice(&s[..pos]);
                    s = &s[pos..];
                    pos = 0;
                }
                let us = s[0];
                if us >= 0x80 {
                    let (cp, consumed) = next_utf8_char(&s[..len]);
                    pos = consumed;
                    match cp {
                        None => {
                            // check whether UTF8 character is correct
                            if has(JSON_INVALID_UTF8_IGNORE) {
                                // ignore invalid UTF8 character
                            } else if has(JSON_INVALID_UTF8_SUBSTITUTE) {
                                // Use Unicode character 'REPLACEMENT CHARACTER' (U+FFFD)
                                if has(JSON_UNESCAPED_UNICODE) {
                                    self.buf.extend_from_slice(b"\xef\xbf\xbd");
                                } else {
                                    self.buf.extend_from_slice(b"\\ufffd");
                                }
                            } else {
                                self.buf.truncate(checkpoint);
                                self.error = JsonError::Utf8;
                                if has(PARTIAL) {
                                    self.buf.extend_from_slice(b"null");
                                }
                                return false;
                            }
                        }
                        // Escape U+2028/U+2029 line terminators, UNLESS both
                        // JSON_UNESCAPED_UNICODE and JSON_UNESCAPED_LINE_TERMINATORS were provided
                        Some(cp)
                            if has(JSON_UNESCAPED_UNICODE)
                                && (has(JSON_UNESCAPED_LINE_TERMINATORS)
                                    || !(0x2028..=0x2029).contains(&cp)) =>
                        {
                            self.buf.extend_from_slice(&s[..pos]);
                        }
                        Some(mut cp) => {
                            // From http://en.wikipedia.org/wiki/UTF16
                            if cp >= 0x10000 {
                                cp -= 0x10000;
                                let next = (cp & 0x3ff) | 0xdc00;
                                self.push_unicode_escape((cp >> 10) | 0xd800);
                                cp = next;
                            }
                            self.push_unicode_escape(cp);
                        }
                    }
                    s = &s[pos..];
                    len -= pos;
                    pos = 0;
                } else {
                    s = &s[1..];
                    match us {
                        b'"' => {
                            if has(JSON_HEX_QUOT) {
                                self.buf.extend_from_slice(b"\\u0022");
                            } else {
                                self.buf.extend_from_slice(b"\\\"");
                            }
                        }
                        b'\\' => self.buf.extend_from_slice(b"\\\\"),
                        b'/' => {
                            if has(JSON_UNESCAPED_SLASHES) {
                                self.buf.push(b'/');
                            } else {
                                self.buf.extend_from_slice(b"\\/");
                            }
                        }
                        0x08 => self.buf.extend_from_slice(b"\\b"),
                        0x0c => self.buf.extend_from_slice(b"\\f"),
                        b'\n' => self.buf.extend_from_slice(b"\\n"),
                        b'\r' => self.buf.extend_from_slice(b"\\r"),
                        b'\t' => self.buf.extend_from_slice(b"\\t"),
                        b'<' => {
                            if has(JSON_HEX_TAG) {
                                self.buf.extend_from_slice(b"\\u003C");
                            } else {
                                self.buf.push(b'<');
                            }
                        }
                        b'>' => {
                            if has(JSON_HEX_TAG) {
                                self.buf.extend_from_slice(b"\\u003E");
                            } else {
                                self.buf.push(b'>');
                            }
                        }
                        b'&' => {
                            if has(JSON_HEX_AMP) {
                                self.buf.extend_from_slice(b"\\u0026");
                            } else {
                                self.buf.push(b'&');
                            }
                        }
                        b'\'' => {
                            if has(JSON_HEX_APOS) {
                                self.buf.extend_from_slice(b"\\u0027");
                            } else {
                                self.buf.push(b'\'');
                            }
                        }
                        _ => {
                            // control characters below ' '
                            self.buf.extend_from_slice(&[
                                b'\\',
                                b'u',
                                b'0',
                                b'0',
                                DIGITS[usize::from(us >> 4)],
                                DIGITS[usize::from(us & 0xf)],
                            ]);
                        }
                    }
                    len -= 1;
                }
            }
            if len == 0 {
                break;
            }
        }

        self.buf.push(b'"');
        true
    }

    /// `\uXXXX` with lowercase hex digits.
    fn push_unicode_escape(&mut self, us: u32) {
        self.buf.extend_from_slice(&[
            b'\\',
            b'u',
            DIGITS[((us >> 12) & 0xf) as usize],
            DIGITS[((us >> 8) & 0xf) as usize],
            DIGITS[((us >> 4) & 0xf) as usize],
            DIGITS[(us & 0xf) as usize],
        ]);
    }
}

/// The `charmap` of `php_json_escape_string()`: control characters, `"`, `&`, `'`, `/`, `<`, `>`,
/// `\` and every byte >= 0x80 take the slow path (0x7F does not).
fn needs_escape(c: u8) -> bool {
    const CHARMAP: [u32; 8] = [
        0xffffffff, 0x500080c4, 0x10000000, 0x00000000, 0xffffffff, 0xffffffff, 0xffffffff,
        0xffffffff,
    ];
    CHARMAP[usize::from(c >> 5)] & (1 << (c & 31)) != 0
}

/// Port of `php_next_utf8_char()` / `get_next_char(cs_utf_8, ...)` (PHPSRC/ext/standard/html.c):
/// decodes one character at the start of `str` (UTR #36 strategy 2: an invalid sequence never
/// swallows a following byte that could start a valid one). Returns the code point (`None` for
/// an invalid sequence) and the number of bytes consumed.
fn next_utf8_char(str: &[u8]) -> (Option<u32>, usize) {
    let avail = str.len();
    if avail == 0 {
        return (None, 1);
    }
    let lead = |c: u8| c < 0x80 || (0xc2..=0xf4).contains(&c);
    let trail = |c: u8| (0x80..=0xbf).contains(&c);
    let c = str[0];
    if c < 0x80 {
        (Some(u32::from(c)), 1)
    } else if c < 0xc2 {
        (None, 1)
    } else if c < 0xe0 {
        if avail < 2 {
            return (None, 1);
        }
        if !trail(str[1]) {
            return (None, if lead(str[1]) { 1 } else { 2 });
        }
        let cp = (u32::from(c & 0x1f) << 6) | u32::from(str[1] & 0x3f);
        if cp < 0x80 {
            // non-shortest form
            return (None, 2);
        }
        (Some(cp), 2)
    } else if c < 0xf0 {
        if avail < 3 || !trail(str[1]) || !trail(str[2]) {
            return if avail < 2 || lead(str[1]) {
                (None, 1)
            } else if avail < 3 || lead(str[2]) {
                (None, 2)
            } else {
                (None, 3)
            };
        }
        let cp = (u32::from(c & 0x0f) << 12)
            | (u32::from(str[1] & 0x3f) << 6)
            | u32::from(str[2] & 0x3f);
        if cp < 0x800 || (0xd800..=0xdfff).contains(&cp) {
            // non-shortest form or surrogate
            return (None, 3);
        }
        (Some(cp), 3)
    } else if c < 0xf5 {
        if avail < 4 || !trail(str[1]) || !trail(str[2]) || !trail(str[3]) {
            return if avail < 2 || lead(str[1]) {
                (None, 1)
            } else if avail < 3 || lead(str[2]) {
                (None, 2)
            } else if avail < 4 || lead(str[3]) {
                (None, 3)
            } else {
                (None, 4)
            };
        }
        let cp = (u32::from(c & 0x07) << 18)
            | (u32::from(str[1] & 0x3f) << 12)
            | (u32::from(str[2] & 0x3f) << 6)
            | u32::from(str[3] & 0x3f);
        if !(0x10000..=0x10ffff).contains(&cp) {
            // non-shortest form or outside range
            return (None, 4);
        }
        (Some(cp), 4)
    } else {
        (None, 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::array::Array;
    use crate::diag::Diagnostic;
    use crate::object::StdClass;

    fn enc(v: &Value, options: i64) -> (String, JsonError) {
        let mut d: Vec<Diagnostic> = Vec::new();
        let r = json_encode_value(v, options as i32, 512, &mut d).unwrap();
        (String::from_utf8(r.output.to_vec()).unwrap(), r.error)
    }

    #[test]
    fn scalars_and_strings() {
        assert_eq!(enc(&Value::Null, 0).0, "null");
        assert_eq!(enc(&Value::Float(0.1), 0).0, "0.1");
        assert_eq!(
            enc(&Value::Float(1.0), JSON_PRESERVE_ZERO_FRACTION).0,
            "1.0"
        );
        assert_eq!(
            enc(&Value::Float(f64::NAN), 0),
            ("0".into(), JsonError::InfOrNan)
        );
        assert_eq!(enc(&Value::from("a/b\"c"), 0).0, "\"a\\/b\\\"c\"");
        assert_eq!(
            enc(&Value::from("\u{0}\u{1f}\u{7f}\u{2028}é😀"), 0).0,
            "\"\\u0000\\u001f\u{7f}\\u2028\\u00e9\\ud83d\\ude00\""
        );
        assert_eq!(
            enc(&Value::from("\u{2028}é"), JSON_UNESCAPED_UNICODE).0,
            "\"\\u2028é\""
        );
        assert_eq!(
            enc(&Value::from(&b"a\xffb"[..]), 0),
            (String::new(), JsonError::Utf8)
        );
        assert_eq!(
            enc(&Value::from(&b"a\xffb"[..]), JSON_INVALID_UTF8_SUBSTITUTE).0,
            "\"a\\ufffdb\""
        );
        assert_eq!(enc(&Value::from("1e3"), JSON_NUMERIC_CHECK).0, "1000");
        assert_eq!(enc(&Value::from(" 12"), JSON_NUMERIC_CHECK).0, "12");
    }

    #[test]
    fn containers() {
        let list = Value::Array(Array::from_values([Value::Int(1), Value::Int(2)]));
        assert_eq!(enc(&list, 0).0, "[1,2]");
        assert_eq!(enc(&list, JSON_FORCE_OBJECT).0, "{\"0\":1,\"1\":2}");
        assert_eq!(enc(&list, JSON_PRETTY_PRINT).0, "[\n    1,\n    2\n]");
        let mut map = Array::new();
        map.insert("a", Value::Int(1));
        map.insert(5, Value::Array(Array::new()));
        assert_eq!(enc(&Value::Array(map), 0).0, "{\"a\":1,\"5\":[]}");
        let obj = StdClass::with_properties([("x", Value::Int(1))]).into_ref();
        assert_eq!(enc(&Value::Object(obj.clone()), 0).0, "{\"x\":1}");
        obj.object()
            .property_store()
            .unwrap()
            .set(b"self", Value::Object(obj.clone()));
        assert_eq!(
            enc(&Value::Object(obj.clone()), 0),
            ("{\"x\":1,\"self\":null".into(), JsonError::Recursion)
        );
        assert_eq!(
            enc(&Value::Object(obj), JSON_PARTIAL_OUTPUT_ON_ERROR),
            ("{\"x\":1,\"self\":null}".into(), JsonError::Recursion)
        );
    }

    #[test]
    fn depth() {
        let nested = Value::Array(Array::from_values([Value::Array(Array::from_values([
            Value::Float(f64::INFINITY),
        ]))]));
        let mut d: Vec<Diagnostic> = Vec::new();
        let r = json_encode_value(&nested, JSON_PARTIAL_OUTPUT_ON_ERROR as i32, 1, &mut d).unwrap();
        assert_eq!(r.output, "[[0]]");
        assert_eq!(r.error, JsonError::Depth);
        let throw = Value::Int(JSON_THROW_ON_ERROR);
        let e = json_encode(&nested, Some(&throw), Some(&Value::Int(1)), &mut d).unwrap_err();
        assert_eq!(e.class().name(), "JsonException");
        assert_eq!(e.message(), "Maximum stack depth exceeded");
        let args = [Value::Int(1), Value::Int(0), Value::Int(i64::MAX)];
        let e = json_encode_call(&args, &mut d).unwrap_err();
        assert_eq!(
            e.message(),
            "json_encode(): Argument #3 ($depth) must be between -2147483648 and 2147483647"
        );
        let e = json_encode_call(&[], &mut d).unwrap_err();
        assert_eq!(
            e.message(),
            "json_encode() expects at least 1 argument, 0 given"
        );
    }

    #[test]
    fn deep_nesting_is_iterative() {
        let mut v = Value::Int(1);
        for _ in 0..100_000 {
            v = Value::Array(Array::from_values([v]));
        }
        let mut d: Vec<Diagnostic> = Vec::new();
        let r = json_encode_value(&v, 0, i32::MAX, &mut d).unwrap();
        assert_eq!(r.error, JsonError::None);
        assert_eq!(r.output.len(), 200_001);
        // Dropping deeply nested values recursively is not this module's concern; leak the
        // value instead of risking a stack overflow in its destructor.
        std::mem::forget(v);
    }
}
