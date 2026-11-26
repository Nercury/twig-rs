//! `Twig\Markup`: a string marked as safe.

use std::sync::LazyLock;

use twig_value::object::{
    CallArgs, CastTarget, ClassBuilder, ClassRef, FunctionKind, MethodParams, Object, ObjectRef,
    Param, PropertyInfo, PropertyStore, Visibility, builtin, std_cast,
};
use twig_value::param::{DeclaredType, TypeMask, UserArg, verify_user_arg};
use twig_value::{
    Array, ArrayKey, Diagnostics, KeyContext, PhpError, PhpResult, PhpString, Value, convert,
};

use super::typed_property::verify_property_type;
use crate::escaper::implied_strategies;

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

    /// `$this->safeStrategies`: the declared strategies, as given (only `__construct()` called from a template can
    /// declare values that are not strings).
    fn declared_strategies(&self) -> Array {
        match self.properties.get(b"safeStrategies") {
            Some(Value::Array(strategies)) => strategies,
            _ => Array::new(),
        }
    }

    /// The declared strategies that are strings (other values never match a strategy, see
    /// [`is_safe_for`](Self::is_safe_for)).
    pub fn safe_strategies(&self) -> Vec<PhpString> {
        self.declared_strategies()
            .values()
            .filter_map(|v| v.as_str().cloned())
            .collect()
    }

    /// Twig: `Markup::isSafeFor()` (TWIG/src/Markup.php): whether a declared strategy, in declaration order, is
    /// `all`, the strategy itself, or implies it (`\in_array($strategy, EscaperRuntime::IMPLIED_STRATEGIES[$declared]
    /// ?? [], true)`: `html_attr` implies `html_attr_relaxed` and `html`, `html_attr_relaxed` implies `html`).
    ///
    /// # Errors
    ///
    /// A declared strategy that is not a string (only `__construct()` called from a template can declare one) is used
    /// as a key of `IMPLIED_STRATEGIES`: an array or an object throws the `TypeError` `Cannot access offset of type %s
    /// on array`, and `null` or a fractional float report their deprecations to `diag` (`Using null as an array
    /// offset is deprecated, use an empty string instead`, `Implicit conversion from float %s to int loses
    /// precision`), which may turn them into errors.
    pub fn is_safe_for(&self, strategy: &[u8], diag: &mut dyn Diagnostics) -> PhpResult<bool> {
        for declared in self.declared_strategies().values() {
            if let Value::String(name) = declared
                && (name == "all" || name.as_bytes() == strategy)
            {
                return Ok(true);
            }
            // the `?? []` fetch converts the offset like any array read (`slow_index_convert()`)
            if let ArrayKey::Str(name) = ArrayKey::from_value(declared, KeyContext::Read, diag)?
                && implied_strategies(name.as_bytes())
                    .iter()
                    .any(|implied| implied.as_bytes() == strategy)
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Twig: `Markup::count()` (TWIG/src/Markup.php): `mb_strlen($this->content, $this->charset)`, with mbstring's
    /// resolution of the charset name (an unknown name is PHP's `ValueError`, a `null` charset the internal encoding,
    /// UTF-8).
    ///
    /// # Errors
    ///
    /// The errors of `mb_strlen()`'s encoding argument; an encoding mbstring knows but twig-rs cannot count yet (the
    /// multi-byte encodings other than UTF-8) is a twig-rs `Error` (reference/deviations.md).
    pub fn count(&self, diag: &mut dyn Diagnostics) -> PhpResult<i64> {
        let charset = self.charset();
        crate::extension::runtime::strings::mb::mb_strlen(
            &self.content(),
            charset.as_ref().map(PhpString::as_bytes),
            diag,
        )
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

/// The parameters of `Twig\Markup`'s methods (by lowercase name), TWIG/src/Markup.php.
fn markup_params(method: &str) -> Option<Vec<Param>> {
    Some(match method {
        // __construct($content, $charset, private array $safeStrategies = ['all'])
        "__construct" => vec![
            Param::required("content"),
            Param::required("charset"),
            Param::optional(
                "safeStrategies",
                Value::Array(Array::from_values([Value::from("all")])),
            ),
        ],
        // withContent(string $content)
        "withcontent" => vec![Param::required("content")],
        // isSafeFor(string $strategy)
        "issafefor" => vec![Param::required("strategy")],
        "__tostring" | "getcharset" | "count" | "jsonserialize" => Vec::new(),
        _ => return None,
    })
}

/// A `string` parameter received in coercive typing mode (`ZEND_RECV`).
fn string_arg(
    function: &str,
    position: u32,
    name: &str,
    value: &Value,
    diag: &mut dyn Diagnostics,
) -> PhpResult<PhpString> {
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

    fn method_params(&self, method: &str) -> Option<MethodParams> {
        markup_params(method).map(MethodParams::user)
    }

    /// `(string) $markup` calls `__toString()`, which returns the content: [`std_cast`] without the dynamic call.
    fn cast(
        &self,
        this: &ObjectRef,
        target: CastTarget,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Option<Value>> {
        match target {
            CastTarget::String => Ok(Some(Value::String(self.content()))),
            _ => std_cast(this, target, diag),
        }
    }

    /// The methods of `Twig\Markup`, each called like a userland method: the arguments are bound to the declared
    /// parameters (`markup_params()`: named arguments, the count), then the typed parameters are received.
    fn call(
        &self,
        _this: &ObjectRef,
        method: &str,
        args: CallArgs,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        let Some(params) = markup_params(method) else {
            return Err(twig_value::object::undefined_method(&CLASS, method));
        };
        let declared = CLASS.method(method).map_or(method, |m| m.name.as_ref());
        let function = format!("{MARKUP_CLASS}::{declared}");
        let bound = args.bind(&function, &params, FunctionKind::User)?;
        match method {
            "issafefor" => {
                let strategy = string_arg(&function, 1, "strategy", &bound[0], diag)?;
                Ok(Value::Bool(self.is_safe_for(strategy.as_bytes(), diag)?))
            }
            "withcontent" => {
                let content = string_arg(&function, 1, "content", &bound[0], diag)?;
                let copy = Markup {
                    properties: self.properties.clone(),
                };
                copy.properties.set(b"content", Value::String(content));
                Ok(Value::Object(ObjectRef::new(copy)))
            }
            "__construct" => {
                let [content, charset, strategies] = <[Value; 3]>::try_from(bound)
                    .unwrap_or_else(|_| unreachable!("three parameters are bound"));
                // `array $safeStrategies` is received before the body runs
                if !matches!(strategies, Value::Array(_)) {
                    return Err(UserArg::new(&function, 3, "safeStrategies")
                        .type_error(&DeclaredType::builtin(TypeMask::ARRAY), &strategies));
                }
                // The promoted property is assigned first, as it is received (an array passes `array`), then the body
                // runs: `$this->content = (string) $content; $this->charset = $charset;`, the latter checked against
                // `private ?string $charset`. What a failing statement leaves is kept.
                self.properties.set(b"safeStrategies", strategies);
                let content = convert::to_string(&content, diag)?;
                self.properties.set(b"content", Value::String(content));
                let charset = verify_property_type(
                    MARKUP_CLASS,
                    "charset",
                    &DeclaredType::builtin(TypeMask::STRING | TypeMask::NULL),
                    charset,
                    diag,
                )?;
                self.properties.set(b"charset", charset);
                Ok(Value::Null)
            }
            "getcharset" => match self.charset() {
                Some(charset) => Ok(Value::String(charset)),
                None => Err(PhpError::type_error(
                    "Twig\\Markup::getCharset(): Return value must be of type string, null returned",
                )),
            },
            "count" => Ok(Value::Int(self.count(diag)?)),
            // `__toString()`, `jsonSerialize()`
            _ => Ok(Value::String(self.content())),
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
        assert!(inner.is_safe_for(b"js", &mut diag).unwrap());
        let only_attr = Markup::create(
            PhpString::from("x"),
            None,
            Some(vec![PhpString::from("html_attr")]),
        );
        let inner = only_attr.downcast_ref::<Markup>().unwrap();
        assert!(inner.is_safe_for(b"html", &mut diag).unwrap());
        assert!(inner.is_safe_for(b"html_attr_relaxed", &mut diag).unwrap());
        assert!(!inner.is_safe_for(b"js", &mut diag).unwrap());
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
        assert!(diag.is_empty(), "{diag:?}");
    }

    /// `__construct()` keeps the strategies as given and checks `$charset` against `private ?string $charset`
    /// (oracle: tests/vectors/fuzz/dynamic/p39-markup-construct.json).
    #[test]
    fn construct_assigns_like_php() {
        let markup = Markup::create(PhpString::from("x"), Some("UTF-8".into()), None);
        let mut diag = Vec::new();
        let construct = |args: CallArgs, diag: &mut Vec<twig_value::Diagnostic>| {
            markup.call_method("__construct", args, diag)
        };
        let args = CallArgs::new()
            .arg("y")
            .arg(5)
            .arg(Value::Array(Array::from_values([Value::Int(1)])));
        construct(args, &mut diag).unwrap();
        let inner = markup.downcast_ref::<Markup>().unwrap();
        assert_eq!(inner.charset().unwrap(), "5");
        assert!(inner.safe_strategies().is_empty());
        assert!(!inner.is_safe_for(b"html", &mut diag).unwrap());
        assert!(diag.is_empty(), "{diag:?}");

        // the charset is assigned last: the content is already replaced when it fails
        let args = CallArgs::new()
            .arg("z")
            .arg(Value::Array(Array::new()))
            .arg(Value::Array(Array::from_values([Value::from("html")])));
        let error = construct(args, &mut diag).unwrap_err();
        assert_eq!(
            error.message(),
            "Cannot assign array to property Twig\\Markup::$charset of type ?string"
        );
        assert_eq!(inner.content(), "z");
        assert_eq!(inner.charset().unwrap(), "5");
        assert!(inner.is_safe_for(b"html", &mut diag).unwrap());

        // a strategy that is not a string is an offset of IMPLIED_STRATEGIES
        let args = CallArgs::new()
            .arg("y")
            .arg(Value::Null)
            .arg(Value::Array(Array::from_values([Value::Float(1.5)])));
        construct(args, &mut diag).unwrap();
        assert!(!inner.is_safe_for(b"html", &mut diag).unwrap());
        assert_eq!(diag.len(), 1, "{diag:?}");
        assert_eq!(
            diag[0].message,
            "Implicit conversion from float 1.5 to int loses precision"
        );
        let args = CallArgs::new()
            .arg("y")
            .arg(Value::Null)
            .arg(Value::Array(Array::from_values([Value::Array(
                Array::new(),
            )])));
        construct(args, &mut diag).unwrap();
        let error = inner.is_safe_for(b"js", &mut diag).unwrap_err();
        assert_eq!(
            error.message(),
            "Cannot access offset of type array on array"
        );
    }
}
