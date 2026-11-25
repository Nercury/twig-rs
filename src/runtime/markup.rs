//! `Twig\Markup`: a string marked as safe.

use std::sync::LazyLock;

use twig_value::object::{
    CallArgs, ClassBuilder, ClassRef, Object, ObjectRef, PropertyInfo, PropertyStore, Visibility,
    builtin,
};
use twig_value::param::{DeclaredType, TypeMask, UserArg, verify_user_arg};
use twig_value::{Array, Diagnostics, PhpError, PhpResult, PhpString, Value, convert};

/// The class name.
pub const MARKUP_CLASS: &str = "Twig\\Markup";

fn private_property(name: &str, type_name: Option<&str>, default: Option<Value>) -> PropertyInfo {
    PropertyInfo {
        name: PhpString::from(name),
        visibility: Visibility::Private,
        is_static: false,
        is_readonly: false,
        type_name: type_name.map(Into::into),
        default,
        declaring_class: MARKUP_CLASS.into(),
    }
}

static CLASS: LazyLock<ClassRef> = LazyLock::new(|| {
    // TWIG/src/Markup.php: `class Markup implements \Countable, \JsonSerializable, \Stringable` with `private
    // $content; private ?string $charset;` and the promoted `private array $safeStrategies`.
    ClassBuilder::class(MARKUP_CLASS)
        .implements(&builtin::countable())
        .implements(&builtin::json_serializable())
        .implements(&builtin::stringable())
        .property_info(private_property("content", None, Some(Value::Null)))
        .property_info(private_property("charset", Some("?string"), None))
        .property_info(private_property("safeStrategies", Some("array"), None))
        .method("__construct")
        .method("__toString")
        .method("getCharset")
        .method("withContent")
        .method("isSafeFor")
        .method("count")
        .method("jsonSerialize")
        .build()
});

/// `Twig\Markup` (TWIG/src/Markup.php): content that is safe for the listed escaping strategies (`['all']` by
/// default). Captures (`{% set %}...{% endset %}`, `{% apply %}`), macros and `include()` produce markup; the escaper
/// leaves markup that is safe for the active strategy alone, and the internal `true` test converts it to its string.
pub struct Markup {
    properties: PropertyStore,
}

impl Markup {
    /// The class entry of `Twig\Markup`.
    pub fn class() -> ClassRef {
        CLASS.clone()
    }

    /// `new Markup($content, $charset, $safeStrategies)`; `None` strategies stand for the default `['all']`.
    pub fn create(
        content: PhpString,
        charset: Option<PhpString>,
        safe_strategies: Option<Vec<PhpString>>,
    ) -> ObjectRef {
        let markup = Markup {
            properties: PropertyStore::new(&CLASS),
        };
        markup.init(
            content,
            charset,
            safe_strategies.unwrap_or_else(|| vec![PhpString::from("all")]),
        );
        ObjectRef::new(markup)
    }

    /// `new Markup($content, $charset)` as a value.
    pub fn new_value(content: PhpString, charset: impl Into<PhpString>) -> Value {
        Value::Object(Markup::create(content, Some(charset.into()), None))
    }

    fn init(&self, content: PhpString, charset: Option<PhpString>, safe: Vec<PhpString>) {
        self.properties.set(b"content", Value::String(content));
        self.properties
            .set(b"charset", charset.map_or(Value::Null, Value::String));
        self.properties.set(
            b"safeStrategies",
            Value::Array(safe.into_iter().map(Value::String).collect::<Array>()),
        );
    }

    /// Whether the object is a `Twig\Markup` (`$x instanceof Markup`).
    pub fn is_markup(object: &ObjectRef) -> bool {
        object.instance_of(MARKUP_CLASS)
    }

    /// The content (`__toString()`).
    pub fn content(&self) -> PhpString {
        match self.properties.get(b"content") {
            Some(Value::String(content)) => content,
            _ => PhpString::empty(),
        }
    }

    /// The charset given to the constructor.
    pub fn charset(&self) -> Option<PhpString> {
        match self.properties.get(b"charset") {
            Some(Value::String(charset)) => Some(charset),
            _ => None,
        }
    }

    /// The declared strategies.
    pub fn safe_strategies(&self) -> Vec<PhpString> {
        match self.properties.get(b"safeStrategies") {
            Some(Value::Array(strategies)) => strategies
                .values()
                .filter_map(|v| v.as_str().cloned())
                .collect(),
            _ => Vec::new(),
        }
    }

    /// Twig: `Markup::isSafeFor()`: a declared strategy is `all`, the strategy itself, or implies it
    /// (`EscaperRuntime::IMPLIED_STRATEGIES`: `html_attr` implies `html_attr_relaxed` and `html`, `html_attr_relaxed`
    /// implies `html`).
    pub fn is_safe_for(&self, strategy: &[u8]) -> bool {
        self.safe_strategies().iter().any(|declared| {
            let declared = declared.as_bytes();
            declared == b"all"
                || declared == strategy
                || match declared {
                    b"html_attr" => strategy == b"html_attr_relaxed" || strategy == b"html",
                    b"html_attr_relaxed" => strategy == b"html",
                    _ => false,
                }
        })
    }

    /// Twig: `Markup::count()`: `mb_strlen($this->content, $this->charset)`.
    ///
    /// Only UTF-8 (and single-byte charsets, counted in bytes) are supported (reference/deviations.md).
    pub fn count(&self) -> PhpResult<i64> {
        let content = self.content();
        let charset = self.charset();
        let name = charset
            .as_ref()
            .map(|c| c.to_string_lossy().to_ascii_uppercase())
            .unwrap_or_else(|| "UTF-8".to_owned());
        match name.as_str() {
            "UTF-8" | "UTF8" => Ok(twig_unicode::mb_strlen(&content) as i64),
            "ASCII" | "US-ASCII" | "8BIT" | "BINARY" => Ok(content.len() as i64),
            n if n.starts_with("ISO-8859-")
                || n.starts_with("WINDOWS-125")
                || n.starts_with("CP125")
                || n.starts_with("ISO8859-") =>
            {
                Ok(content.len() as i64)
            }
            _ => Err(PhpError::value_error(format!(
                "mb_strlen(): Argument #2 ($encoding) must be a valid encoding, \"{}\" given",
                charset
                    .map(|c| c.to_string_lossy().into_owned())
                    .unwrap_or_default()
            ))),
        }
    }
}

impl std::fmt::Debug for Markup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Markup")
            .field("content", &self.content())
            .field("charset", &self.charset())
            .finish()
    }
}

fn string_arg(
    function: &str,
    position: u32,
    name: &str,
    value: Option<&Value>,
    diag: &mut dyn Diagnostics,
) -> PhpResult<PhpString> {
    let Some(value) = value else {
        return Err(PhpError::argument_count_error(format!(
            "Too few arguments to function {function}(), {} passed and exactly {position} expected",
            position - 1
        )));
    };
    match verify_user_arg(
        value.clone(),
        &DeclaredType::builtin(TypeMask::STRING),
        &UserArg::new(function, position, name),
        diag,
    )? {
        Value::String(s) => Ok(s),
        other => convert::to_string(&other, diag),
    }
}

impl Object for Markup {
    fn class(&self) -> &ClassRef {
        &CLASS
    }

    fn property_store(&self) -> Option<&PropertyStore> {
        Some(&self.properties)
    }

    fn call(
        &self,
        this: &ObjectRef,
        method: &str,
        args: CallArgs,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        match method {
            "__tostring" | "jsonserialize" => Ok(Value::String(self.content())),
            "getcharset" => match self.charset() {
                Some(charset) => Ok(Value::String(charset)),
                None => Err(PhpError::type_error(
                    "Twig\\Markup::getCharset(): Return value must be of type string, null returned",
                )),
            },
            "count" => Ok(Value::Int(self.count()?)),
            "issafefor" => {
                let strategy =
                    string_arg("Twig\\Markup::isSafeFor", 1, "strategy", args.get(0), diag)?;
                Ok(Value::Bool(self.is_safe_for(&strategy)))
            }
            "withcontent" => {
                let content =
                    string_arg("Twig\\Markup::withContent", 1, "content", args.get(0), diag)?;
                let copy = Markup {
                    properties: self.properties.clone(),
                };
                copy.properties.set(b"content", Value::String(content));
                Ok(Value::Object(ObjectRef::new(copy)))
            }
            "__construct" => {
                let content = match args.get(0) {
                    Some(content) => convert::to_string(content, diag)?,
                    None => {
                        return Err(PhpError::argument_count_error(
                            "Too few arguments to function Twig\\Markup::__construct(), 0 passed and at least 2 expected",
                        ));
                    }
                };
                let charset = match args.get(1) {
                    Some(Value::Null) => None,
                    Some(charset) => Some(convert::to_string(charset, diag)?),
                    None => {
                        return Err(PhpError::argument_count_error(
                            "Too few arguments to function Twig\\Markup::__construct(), 1 passed and at least 2 expected",
                        ));
                    }
                };
                let safe = match args.get(2) {
                    Some(Value::Array(strategies)) => strategies
                        .values()
                        .map(|v| convert::to_string(v, diag))
                        .collect::<PhpResult<Vec<_>>>()?,
                    Some(other) => {
                        return Err(
                            UserArg::new("Twig\\Markup::__construct", 3, "safeStrategies")
                                .type_error(&DeclaredType::builtin(TypeMask::ARRAY), other),
                        );
                    }
                    None => vec![PhpString::from("all")],
                };
                self.init(content, charset, safe);
                let _ = this;
                Ok(Value::Null)
            }
            _ => Err(twig_value::object::undefined_method(&CLASS, method)),
        }
    }

    fn clone_object(&self, _this: &ObjectRef, _diag: &mut dyn Diagnostics) -> PhpResult<ObjectRef> {
        Ok(ObjectRef::new(Markup {
            properties: self.properties.clone(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn behaves_like_twig_markup() {
        let markup = Markup::create(PhpString::from("<b>é</b>"), Some("UTF-8".into()), None);
        let mut diag = Vec::new();
        assert!(markup.instance_of("Countable"));
        assert!(markup.instance_of("Stringable"));
        assert_eq!(markup.to_php_string(&mut diag).unwrap(), "<b>é</b>");
        assert_eq!(markup.count(&mut diag).unwrap(), Some(8));
        let inner = markup.downcast_ref::<Markup>().unwrap();
        assert!(inner.is_safe_for(b"js"));
        let only_attr = Markup::create(
            PhpString::from("x"),
            None,
            Some(vec![PhpString::from("html_attr")]),
        );
        let inner = only_attr.downcast_ref::<Markup>().unwrap();
        assert!(inner.is_safe_for(b"html"));
        assert!(inner.is_safe_for(b"html_attr_relaxed"));
        assert!(!inner.is_safe_for(b"js"));
        let dump = twig_value::dump::var_dump(&Value::Object(markup.clone()), &mut diag).unwrap();
        let dump = String::from_utf8_lossy(&dump).into_owned();
        assert!(
            dump.contains("[\"content\":\"Twig\\Markup\":private]=>"),
            "{dump}"
        );
        assert!(
            dump.contains("[\"safeStrategies\":\"Twig\\Markup\":private]=>"),
            "{dump}"
        );
    }
}
