//! Rust versions of the PHP classes the engine vectors create in `data_php` (see the `TwigRs*` classes at the top of
//! every object case of `tests/vectors/engine/*.gen.json`).

use std::sync::{LazyLock, Mutex};

use twig::runtime::Markup;
use twig::value::object::{
    ArrayIterator, ArrayObject, CallArgs, ClassBuilder, ClassRef, FunctionKind, Object, ObjectRef,
    Param, PropertyInfo, PropertyStore, Visibility, builtin,
};
use twig::value::{
    Array, Diagnostics, PhpError, PhpResult, PhpString, ThrowableClass, Value, builtins, dim,
};

fn property(
    name: &str,
    visibility: Visibility,
    type_name: Option<&str>,
    default: Option<Value>,
    class: &str,
) -> PropertyInfo {
    PropertyInfo {
        name: PhpString::from(name),
        visibility,
        is_static: false,
        is_readonly: false,
        type_name: type_name.map(Into::into),
        default,
        declaring_class: class.into(),
    }
}

// ---- TwigRsBag --------------------------------------------------------------------------------------------------

static BAG: LazyLock<ClassRef> = LazyLock::new(|| {
    let c = "TwigRsBag";
    ClassBuilder::class(c)
        .property_info(property(
            "pub",
            Visibility::Public,
            None,
            Some(Value::from("pub")),
            c,
        ))
        .property_info(property(
            "nul",
            Visibility::Public,
            None,
            Some(Value::Null),
            c,
        ))
        .property_info(property("typed", Visibility::Public, Some("?int"), None, c))
        .property_info(property(
            "priv",
            Visibility::Private,
            None,
            Some(Value::from("priv")),
            c,
        ))
        .property_info(property(
            "prot",
            Visibility::Protected,
            None,
            Some(Value::from("prot")),
            c,
        ))
        .constant("KONST", "konst")
        .method("getPriv")
        .method("getFoo")
        .method("isBar")
        .method("hasBaz")
        .method("isBaz")
        .method("hasQux")
        .method("get")
        .method("pub")
        .method("args")
        .method("named")
        .method("getSelf")
        .method("getNull")
        .method("__toString")
        .build()
});

struct Bag {
    properties: PropertyStore,
}

impl Object for Bag {
    fn class(&self) -> &ClassRef {
        &BAG
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
        Ok(match method {
            "getpriv" => Value::from("getPriv()"),
            "getfoo" => Value::from("getFoo()"),
            "isbar" => Value::from("isBar()"),
            "hasbaz" => Value::from("hasBaz()"),
            "isbaz" => Value::from("isBaz()"),
            "hasqux" => Value::from("hasQux()"),
            "get" => Value::from("get()"),
            "pub" => Value::from("pub()"),
            "args" => {
                // public function args(...$a) { return json_encode($a); }
                let bound = args.bind(
                    "TwigRsBag::args",
                    &[Param::variadic("a")],
                    FunctionKind::User,
                )?;
                builtins::format::json_encode(&bound[0], None, None, diag)?
            }
            "named" => {
                // public function named($first, $second = 'S') { return $first . '-' . $second; }
                let bound = args.bind(
                    "TwigRsBag::named",
                    &[Param::required("first"), Param::optional("second", "S")],
                    FunctionKind::User,
                )?;
                Value::String(twig::value::ops::concat(
                    &Value::String(twig::value::ops::concat(
                        &bound[0],
                        &Value::from("-"),
                        diag,
                    )?),
                    &bound[1],
                    diag,
                )?)
            }
            "getself" => Value::Object(this.clone()),
            "getnull" => Value::Null,
            "__tostring" => Value::from("bag"),
            _ => return Err(twig::value::object::undefined_method(&BAG, method)),
        })
    }
}

// ---- TwigRsMagic ------------------------------------------------------------------------------------------------

static MAGIC: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::class("TwigRsMagic")
        .method("__isset")
        .method("__get")
        .method("__call")
        .build()
});

struct Magic {
    properties: PropertyStore,
}

impl Object for Magic {
    fn class(&self) -> &ClassRef {
        &MAGIC
    }

    fn property_store(&self) -> Option<&PropertyStore> {
        Some(&self.properties)
    }

    fn call(
        &self,
        _this: &ObjectRef,
        method: &str,
        args: CallArgs,
        _diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        let name = args
            .get(0)
            .and_then(Value::as_str)
            .cloned()
            .unwrap_or_default();
        Ok(match method {
            "__isset" => Value::Bool(name.as_bytes() == b"magic"),
            "__get" => Value::String(PhpString::concat(&[b"got ", name.as_bytes()])),
            "__call" => {
                if name.as_bytes() == b"bad" {
                    return Err(PhpError::new(
                        ThrowableClass::BadMethodCallException,
                        "bad call",
                    ));
                }
                let count = match args.get(1) {
                    Some(Value::Array(a)) => a.len(),
                    _ => 0,
                };
                Value::String(PhpString::concat(&[
                    b"called ",
                    name.as_bytes(),
                    format!("({count})").as_bytes(),
                ]))
            }
            _ => return Err(twig::value::object::undefined_method(&MAGIC, method)),
        })
    }
}

// ---- TwigRsAccess -----------------------------------------------------------------------------------------------

static ACCESS: LazyLock<ClassRef> = LazyLock::new(|| {
    let c = "TwigRsAccess";
    ClassBuilder::class(c)
        .implements(&builtin::array_access())
        .implements(&builtin::countable())
        .implements(&builtin::iterator_aggregate())
        .property_info(property(
            "data",
            Visibility::Private,
            None,
            Some(Value::Null),
            c,
        ))
        .method("__construct")
        .method("offsetExists")
        .method("offsetGet")
        .method("offsetSet")
        .method("offsetUnset")
        .method("count")
        .method("getIterator")
        .build()
});

struct Access {
    properties: PropertyStore,
}

impl Access {
    fn data(&self) -> Value {
        self.properties.get(b"data").unwrap_or_default()
    }
}

impl Object for Access {
    fn class(&self) -> &ClassRef {
        &ACCESS
    }

    fn property_store(&self) -> Option<&PropertyStore> {
        Some(&self.properties)
    }

    fn call(
        &self,
        _this: &ObjectRef,
        method: &str,
        args: CallArgs,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        let offset = args.get(0).cloned().unwrap_or_default();
        Ok(match method {
            // return isset($this->data[$o]);
            "offsetexists" => Value::Bool(dim::isset(&self.data(), &offset, diag)?),
            // return $this->data[$o] ?? null;
            "offsetget" => dim::read(
                &self.data(),
                &offset,
                twig::value::object::FetchMode::Isset,
                diag,
            )?,
            "offsetset" | "offsetunset" => Value::Null,
            "count" => Value::Int(dim::count(&self.data(), 0, diag)?),
            "getiterator" => match self.data() {
                Value::Array(a) => Value::Object(ArrayIterator::new(a).into_ref()),
                _ => Value::Null,
            },
            _ => return Err(twig::value::object::undefined_method(&ACCESS, method)),
        })
    }
}

// ---- TwigRsIter -------------------------------------------------------------------------------------------------

static ITER: LazyLock<ClassRef> = LazyLock::new(|| {
    let c = "TwigRsIter";
    ClassBuilder::class(c)
        .implements(&builtin::iterator())
        .property_info(property(
            "i",
            Visibility::Private,
            None,
            Some(Value::Int(0)),
            c,
        ))
        .property_info(property(
            "log",
            Visibility::Public,
            None,
            Some(Value::Array(Array::new())),
            c,
        ))
        .property_info(property(
            "items",
            Visibility::Private,
            Some("array"),
            None,
            c,
        ))
        .method("__construct")
        .method("current")
        .method("key")
        .method("next")
        .method("rewind")
        .method("valid")
        .build()
});

struct Iter {
    properties: PropertyStore,
    state: Mutex<i64>,
}

impl Iter {
    fn log(&self, what: &str) {
        let mut log = match self.properties.get(b"log") {
            Some(Value::Array(a)) => a,
            _ => Array::new(),
        };
        let _ = log.push(Value::from(what));
        self.properties.set(b"log", Value::Array(log));
    }

    fn items(&self) -> Array {
        match self.properties.get(b"items") {
            Some(Value::Array(a)) => a,
            _ => Array::new(),
        }
    }
}

impl Object for Iter {
    fn class(&self) -> &ClassRef {
        &ITER
    }

    fn property_store(&self) -> Option<&PropertyStore> {
        Some(&self.properties)
    }

    fn call(
        &self,
        _this: &ObjectRef,
        method: &str,
        _args: CallArgs,
        _diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        let mut i = self.state.lock().unwrap();
        Ok(match method {
            "current" => {
                self.log("current");
                self.items().get_int(*i).cloned().unwrap_or_default()
            }
            "key" => {
                self.log("key");
                Value::String(PhpString::from(format!("k{}", *i)))
            }
            "next" => {
                self.log("next");
                *i += 1;
                self.properties.set(b"i", Value::Int(*i));
                Value::Null
            }
            "rewind" => {
                self.log("rewind");
                *i = 0;
                self.properties.set(b"i", Value::Int(0));
                Value::Null
            }
            "valid" => {
                self.log("valid");
                Value::Bool((*i as usize) < self.items().len())
            }
            _ => return Err(twig::value::object::undefined_method(&ITER, method)),
        })
    }
}

// ---- TwigRsThrower ----------------------------------------------------------------------------------------------

static THROWER: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::class("TwigRsThrower")
        .method("boom")
        .method("getValue")
        .build()
});

struct Thrower;

impl Object for Thrower {
    fn class(&self) -> &ClassRef {
        &THROWER
    }

    fn call(
        &self,
        _this: &ObjectRef,
        method: &str,
        _args: CallArgs,
        _diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        match method {
            "boom" => Err(PhpError::new(ThrowableClass::RuntimeException, "boom!")),
            "getvalue" => Ok(Value::Int(42)),
            _ => Err(twig::value::object::undefined_method(&THROWER, method)),
        }
    }
}

/// The object a vector case names, built like the PHP expression of the generator.
pub fn build(name: &str) -> Value {
    let object = match name {
        "bag" => ObjectRef::new(Bag {
            properties: PropertyStore::new(&BAG),
        }),
        "magic" => ObjectRef::new(Magic {
            properties: PropertyStore::empty(),
        }),
        "access" => {
            let access = Access {
                properties: PropertyStore::new(&ACCESS),
            };
            let mut data = Array::new();
            data.insert("a", 1);
            data.insert("b", Value::Null);
            data.insert(
                "c",
                Value::Array(Array::from_values([Value::Int(1), Value::Int(2)])),
            );
            access.properties.set(b"data", Value::Array(data));
            ObjectRef::new(access)
        }
        "iter" => {
            let iter = Iter {
                properties: PropertyStore::new(&ITER),
                state: Mutex::new(0),
            };
            iter.properties.set(
                b"items",
                Value::Array(Array::from_values(["x", "y", "z"].map(Value::from))),
            );
            ObjectRef::new(iter)
        }
        "thrower" => ObjectRef::new(Thrower),
        "arrobj" => {
            let mut data = Array::new();
            data.insert("a", 1);
            data.insert("b", 2);
            ArrayObject::new(data).into_ref()
        }
        "markup" => Markup::create(PhpString::from("<b>bold</b>"), Some("UTF-8".into()), None),
        "empty_markup" => Markup::create(PhpString::empty(), Some("UTF-8".into()), None),
        "zero_markup" => Markup::create(PhpString::from("0"), Some("UTF-8".into()), None),
        "html_markup" => Markup::create(
            PhpString::from("<i>x</i>"),
            Some("UTF-8".into()),
            Some(vec![PhpString::from("html")]),
        ),
        other => panic!("unknown vector object {other}"),
    };
    Value::Object(object)
}
