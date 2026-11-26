//! The SPL classes fixture DATA uses that `twig-value` does not provide: `SplObjectStorage`
//! (PHPSRC/ext/spl/spl_observer.c), `SplFileInfo` (PHPSRC/ext/spl/spl_directory.c) and
//! `CallbackFilterIterator` with its parents `FilterIterator` and `IteratorIterator`
//! (PHPSRC/ext/spl/spl_iterators.c).
//!
//! Methods the fixtures never reach throw `Error: X::m() is not supported by twig-test-support`.

use std::sync::{Arc, LazyLock, Mutex, MutexGuard};

use twig_value::object::{
    CallArgs, ClassBuilder, ClassRef, Closure, FetchMode, FunctionKind, MethodParams, Object,
    ObjectIterator, ObjectRef, Param, PropertyEntry, PropertyPurpose, PropertyStore, Visibility,
    builtin, std_has_dimension, std_read_dimension,
};
use twig_value::param::Arg;
use twig_value::{
    Array, ArrayKey, Diagnostics, PhpError, PhpResult, PhpString, ThrowableClass, Value, convert,
};

use crate::php::php_basename;

fn not_supported(class: &ClassRef, method: &str) -> PhpError {
    let name = class
        .method(method)
        .map(|m| m.name.to_string())
        .unwrap_or_else(|| method.to_string());
    PhpError::error(format!(
        "{}::{}() is not supported by twig-test-support",
        class.display_name(),
        name
    ))
}

fn internal_none(class: &str, method: &str, args: &CallArgs) -> PhpResult<()> {
    args.bind(&format!("{class}::{method}"), &[], FunctionKind::Internal)
        .map(|_| ())
}

/// Binds the arguments of the method `method` (named as declared) of `class` to the parameters
/// `params` gives for it (the arginfo of the class's stub).
fn bind_internal(
    class: &str,
    method: &str,
    params: fn(&str) -> Option<Vec<Param>>,
    args: &CallArgs,
) -> PhpResult<Vec<Value>> {
    let params = params(&method.to_ascii_lowercase()).unwrap_or_default();
    args.bind(
        &format!("{class}::{method}"),
        &params,
        FunctionKind::Internal,
    )
}

/// `Z_PARAM_OBJ`: `%s(): Argument #%d ($%s) must be of type object, %s given`.
fn object_param(v: &Value, function: &str, position: u32, name: &str) -> PhpResult<ObjectRef> {
    match v {
        Value::Object(o) => Ok(o.clone()),
        other => Err(Arg::new(function, position, name).type_error("of type object", other)),
    }
}

// ---------------------------------------------------------------------------------------------
// SplObjectStorage
// ---------------------------------------------------------------------------------------------

static SPL_OBJECT_STORAGE: LazyLock<ClassRef> = LazyLock::new(|| {
    let mut b = ClassBuilder::class("SplObjectStorage")
        .implements(&builtin::countable())
        .implements(&builtin::seekable_iterator())
        .implements(&builtin::serializable())
        .implements(&builtin::array_access());
    for m in [
        "attach",
        "detach",
        "contains",
        "addAll",
        "removeAll",
        "removeAllExcept",
        "getInfo",
        "setInfo",
        "count",
        "rewind",
        "valid",
        "key",
        "current",
        "next",
        "seek",
        "unserialize",
        "serialize",
        "offsetExists",
        "offsetGet",
        "offsetSet",
        "offsetUnset",
        "getHash",
        "__serialize",
        "__unserialize",
        "__debugInfo",
    ] {
        b = b.method(m);
    }
    b.build()
});

/// `class SplObjectStorage implements Countable, SeekableIterator, Serializable, ArrayAccess`.
pub fn spl_object_storage_class() -> ClassRef {
    SPL_OBJECT_STORAGE.clone()
}

#[derive(Default)]
struct StorageState {
    /// `[object, info]` in hash order (PHP keys the hash by object handle).
    entries: Vec<(ObjectRef, Value)>,
    /// `intern->pos`: a position in `entries` (past the end = invalid).
    pos: usize,
    /// `intern->index`.
    index: i64,
}

impl StorageState {
    /// `zend_hash_move_forward_ex()`: to the next entry, or past the end; past the end stays.
    fn move_forward(&mut self) {
        if self.pos < self.entries.len() {
            self.pos += 1;
        }
    }

    /// `zend_hash_move_backwards_ex()`: to the previous entry, or past the end from the first one;
    /// past the end stays (the function fails).
    fn move_backwards(&mut self) {
        if self.pos < self.entries.len() {
            self.pos = match self.pos {
                0 => self.entries.len(),
                pos => pos - 1,
            };
        }
    }
}

/// `SplObjectStorage`: objects mapped to data, iterated in insertion order.
pub struct SplObjectStorage {
    class: ClassRef,
    props: PropertyStore,
    state: Mutex<StorageState>,
}

impl Default for SplObjectStorage {
    fn default() -> Self {
        SplObjectStorage::new()
    }
}

impl SplObjectStorage {
    /// An empty storage.
    pub fn new() -> SplObjectStorage {
        let class = spl_object_storage_class();
        let props = PropertyStore::new(&class);
        SplObjectStorage {
            class,
            props,
            state: Mutex::new(StorageState::default()),
        }
    }

    /// Wraps into a handle.
    pub fn into_ref(self) -> ObjectRef {
        ObjectRef::new(self)
    }

    fn state(&self) -> MutexGuard<'_, StorageState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// `spl_object_storage_attach_handle()`: a known object gets the new info in place.
    pub fn attach(&self, object: ObjectRef, info: Value) {
        let mut st = self.state();
        if let Some(e) = st
            .entries
            .iter_mut()
            .find(|(o, _)| ObjectRef::ptr_eq(o, &object))
        {
            e.1 = info;
            return;
        }
        st.entries.push((object, info));
    }

    /// `zend_hash_index_del()` of the object's entry.
    fn detach(&self, object: &ObjectRef) {
        let mut st = self.state();
        if let Some(i) = st
            .entries
            .iter()
            .position(|(o, _)| ObjectRef::ptr_eq(o, object))
        {
            st.entries.remove(i);
            // A HashPosition keeps pointing at the bucket; the next valid bucket follows.
            if st.pos > i {
                st.pos -= 1;
            }
        }
    }

    fn find(&self, object: &ObjectRef) -> Option<Value> {
        self.state()
            .entries
            .iter()
            .find(|(o, _)| ObjectRef::ptr_eq(o, object))
            .map(|(_, i)| i.clone())
    }

    /// The `[object, info]` entries.
    pub fn entries(&self) -> Vec<(ObjectRef, Value)> {
        self.state().entries.clone()
    }

    fn deprecated(&self, method: &str, instead: &str, diag: &mut dyn Diagnostics) -> PhpResult<()> {
        // zend_deprecated_function() with the #[\Deprecated] attribute of the stub.
        diag.deprecated(format!(
            "Method SplObjectStorage::{method}() is deprecated since 8.5, use method SplObjectStorage::{instead}() instead"
        ))
    }
}

/// The parameters of the implemented methods of `SplObjectStorage` (by lowercase name), from
/// PHPSRC/ext/spl/spl_observer.stub.php.
fn spl_object_storage_params(method: &str) -> Option<Vec<Param>> {
    Some(match method {
        // attach(object $object, mixed $info = null), offsetSet($object, mixed $info = null)
        "attach" | "offsetset" => vec![
            Param::required("object"),
            Param::optional("info", Value::Null),
        ],
        "detach" | "offsetunset" | "contains" | "offsetexists" | "offsetget" => {
            vec![Param::required("object")]
        }
        // count(int $mode = COUNT_NORMAL)
        "count" => vec![Param::optional("mode", 0)],
        "setinfo" => vec![Param::required("info")],
        "seek" => vec![Param::required("offset")],
        "rewind" | "valid" | "key" | "current" | "getinfo" | "next" | "__debuginfo" => Vec::new(),
        _ => return None,
    })
}

impl Object for SplObjectStorage {
    fn class(&self) -> &ClassRef {
        &self.class
    }

    fn property_store(&self) -> Option<&PropertyStore> {
        Some(&self.props)
    }

    fn method_params(&self, method: &str) -> Option<MethodParams> {
        spl_object_storage_params(method).map(MethodParams::internal)
    }

    fn call(
        &self,
        _this: &ObjectRef,
        method: &str,
        args: CallArgs,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        const C: &str = "SplObjectStorage";
        let bind = |m: &str| bind_internal(C, m, spl_object_storage_params, &args);
        let one_object = |m: &str| -> PhpResult<ObjectRef> {
            let a = bind(m)?;
            object_param(&a[0], &format!("{C}::{m}"), 1, "object")
        };
        match method {
            "attach" | "offsetset" => {
                let m = if method == "attach" {
                    "attach"
                } else {
                    "offsetSet"
                };
                if method == "attach" {
                    self.deprecated("attach", "offsetSet", diag)?;
                }
                let a = bind(m)?;
                let o = object_param(&a[0], &format!("{C}::{m}"), 1, "object")?;
                self.attach(o, a[1].clone());
                Ok(Value::Null)
            }
            "detach" | "offsetunset" => {
                if method == "detach" {
                    self.deprecated("detach", "offsetUnset", diag)?;
                }
                let o = one_object(if method == "detach" {
                    "detach"
                } else {
                    "offsetUnset"
                })?;
                self.detach(&o);
                // PHP_METHOD(SplObjectStorage, detach): the iterator restarts.
                let mut st = self.state();
                st.pos = 0;
                st.index = 0;
                Ok(Value::Null)
            }
            "contains" | "offsetexists" => {
                if method == "contains" {
                    self.deprecated("contains", "offsetExists", diag)?;
                }
                let o = one_object(if method == "contains" {
                    "contains"
                } else {
                    "offsetExists"
                })?;
                Ok(Value::Bool(self.find(&o).is_some()))
            }
            "offsetget" => {
                let o = one_object("offsetGet")?;
                self.find(&o).ok_or_else(|| {
                    PhpError::new(ThrowableClass::UnexpectedValueException, "Object not found")
                })
            }
            "count" => {
                let f = format!("{C}::count");
                let a = bind("count")?;
                let mode = twig_value::param::parse_long(&a[0], &Arg::new(&f, 1, "mode"), diag)?;
                if mode == 1 {
                    return Err(not_supported(&self.class, "count"));
                }
                Ok(Value::Int(self.state().entries.len() as i64))
            }
            "rewind" => {
                internal_none(C, "rewind", &args)?;
                let mut st = self.state();
                st.pos = 0;
                st.index = 0;
                Ok(Value::Null)
            }
            "valid" => {
                internal_none(C, "valid", &args)?;
                let st = self.state();
                Ok(Value::Bool(st.pos < st.entries.len()))
            }
            "key" => {
                internal_none(C, "key", &args)?;
                Ok(Value::Int(self.state().index))
            }
            "current" => {
                internal_none(C, "current", &args)?;
                let st = self.state();
                match st.entries.get(st.pos) {
                    Some((o, _)) => Ok(Value::Object(o.clone())),
                    None => Err(PhpError::new(
                        ThrowableClass::RuntimeException,
                        "Called current() on invalid iterator",
                    )),
                }
            }
            "getinfo" => {
                internal_none(C, "getInfo", &args)?;
                let st = self.state();
                Ok(st
                    .entries
                    .get(st.pos)
                    .map(|(_, i)| i.clone())
                    .unwrap_or(Value::Null))
            }
            "setinfo" => {
                let a = bind("setInfo")?;
                let mut st = self.state();
                let pos = st.pos;
                if let Some(e) = st.entries.get_mut(pos) {
                    e.1 = a[0].clone();
                }
                Ok(Value::Null)
            }
            "next" => {
                internal_none(C, "next", &args)?;
                let mut st = self.state();
                st.move_forward();
                st.index += 1;
                Ok(Value::Null)
            }
            // PHP_METHOD(SplObjectStorage, seek) (PHPSRC/ext/spl/spl_observer.c): the position moves
            // forwards, or backwards when that is shorter than restarting. Moving backwards from past
            // the end fails (zend_hash_move_backwards_ex()) while `index` still counts down, so the
            // iterator stays invalid.
            "seek" => {
                let f = format!("{C}::seek");
                let a = bind("seek")?;
                let position =
                    twig_value::param::parse_long(&a[0], &Arg::new(&f, 1, "offset"), diag)?;
                let mut st = self.state();
                if position < 0 || position >= st.entries.len() as i64 {
                    return Err(PhpError::new(
                        ThrowableClass::OutOfBoundsException,
                        format!("Seek position {position} is out of range"),
                    ));
                }
                if position == 0 {
                    st.pos = 0;
                    st.index = 0;
                } else if position > st.index {
                    while position > st.index {
                        st.move_forward();
                        st.index += 1;
                    }
                } else if position < st.index {
                    if st.index - position > position {
                        st.pos = 0;
                        st.index = 0;
                        while position > st.index {
                            st.move_forward();
                            st.index += 1;
                        }
                    } else {
                        while position < st.index {
                            st.move_backwards();
                            st.index -= 1;
                        }
                    }
                }
                Ok(Value::Null)
            }
            "__debuginfo" => {
                internal_none(C, "__debugInfo", &args)?;
                Ok(Value::Array(twig_value::object::property_entries_to_array(
                    self.debug_entries(),
                )))
            }
            _ if self.class.method(method).is_some() => Err(not_supported(&self.class, method)),
            _ => Err(twig_value::object::undefined_method(&self.class, method)),
        }
    }

    /// `spl_object_storage_read_dimension()`: object offsets are looked up directly
    /// (`UnexpectedValueException: Object not found`, `null` for `isset`/`??`); other offsets go
    /// through `offsetGet()`.
    fn read_dimension(
        &self,
        this: &ObjectRef,
        offset: &Value,
        mode: FetchMode,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        let Value::Object(o) = offset else {
            return std_read_dimension(this, offset, mode, diag);
        };
        match self.find(o) {
            Some(v) => Ok(v),
            None if mode == FetchMode::Isset => Ok(Value::Null),
            None => Err(PhpError::new(
                ThrowableClass::UnexpectedValueException,
                "Object not found",
            )),
        }
    }

    /// `spl_object_storage_has_dimension()`.
    fn has_dimension(
        &self,
        this: &ObjectRef,
        offset: &Value,
        check_empty: bool,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<bool> {
        let Value::Object(o) = offset else {
            return std_has_dimension(this, offset, check_empty, diag);
        };
        match self.find(o) {
            None => Ok(false),
            Some(v) if check_empty => convert::to_bool(&v, diag),
            Some(_) => Ok(true),
        }
    }

    /// `spl_object_storage_write_dimension()`.
    fn write_dimension(
        &self,
        this: &ObjectRef,
        offset: Option<&Value>,
        value: Value,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<()> {
        match offset {
            Some(Value::Object(o)) => {
                self.attach(o.clone(), value);
                Ok(())
            }
            _ => {
                let offset = offset.cloned().unwrap_or(Value::Null);
                this.invoke_method("offsetset", CallArgs::from(vec![offset, value]), diag)
                    .map(|_| ())
            }
        }
    }

    /// `spl_object_storage_unset_dimension()`.
    fn unset_dimension(
        &self,
        this: &ObjectRef,
        offset: &Value,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<()> {
        match offset {
            Value::Object(o) => {
                self.detach(o);
                Ok(())
            }
            _ => this
                .invoke_method("offsetunset", CallArgs::from(vec![offset.clone()]), diag)
                .map(|_| ()),
        }
    }

    /// `spl_object_storage_debug_info()` for `var_dump()`; the standard table otherwise (so
    /// `var_export()` and `(array)` show no storage, like PHP).
    fn properties_for(
        &self,
        this: &ObjectRef,
        purpose: PropertyPurpose,
        _diag: &mut dyn Diagnostics,
    ) -> PhpResult<Vec<PropertyEntry>> {
        let _ = this;
        Ok(match purpose {
            PropertyPurpose::Debug => self.debug_entries(),
            _ => self.props.entries(&self.class),
        })
    }

    /// `spl_object_storage_clone()`: the storage is copied.
    fn clone_object(&self, _this: &ObjectRef, _diag: &mut dyn Diagnostics) -> PhpResult<ObjectRef> {
        let copy = SplObjectStorage::new();
        copy.state().entries = self.entries();
        Ok(copy.into_ref())
    }
}

impl SplObjectStorage {
    fn debug_entries(&self) -> Vec<PropertyEntry> {
        let mut out = self.props.entries(&self.class);
        let mut storage = Array::new();
        for (o, inf) in self.entries() {
            let mut pair = Array::new();
            pair.insert("obj", Value::Object(o));
            pair.insert("inf", inf);
            let _ = storage.push(Value::Array(pair));
        }
        out.push(PropertyEntry {
            key: ArrayKey::Str(twig_value::object::mangle_property_name(
                Visibility::Private,
                "SplObjectStorage",
                b"storage",
            )),
            value: Some(Value::Array(storage)),
            type_name: None,
        });
        out
    }
}

// ---------------------------------------------------------------------------------------------
// SplFileInfo
// ---------------------------------------------------------------------------------------------

static SPL_FILE_INFO: LazyLock<ClassRef> = LazyLock::new(|| {
    let mut b = ClassBuilder::class("SplFileInfo").implements(&builtin::stringable());
    for m in [
        "__construct",
        "getPath",
        "getFilename",
        "getExtension",
        "getBasename",
        "getPathname",
        "getPerms",
        "getInode",
        "getSize",
        "getOwner",
        "getGroup",
        "getATime",
        "getMTime",
        "getCTime",
        "getType",
        "isWritable",
        "isReadable",
        "isExecutable",
        "isFile",
        "isDir",
        "isLink",
        "getLinkTarget",
        "getRealPath",
        "getFileInfo",
        "getPathInfo",
        "openFile",
        "setFileClass",
        "setInfoClass",
        "__toString",
        "__debugInfo",
    ] {
        b = b.method(m);
    }
    b.method_with("_bad_state_ex", Visibility::Public, false)
        .build()
});

/// `class SplFileInfo implements Stringable`.
pub fn spl_file_info_class() -> ClassRef {
    SPL_FILE_INFO.clone()
}

#[derive(Default)]
struct FileInfoState {
    file_name: Option<PhpString>,
    path: Option<PhpString>,
}

/// `SplFileInfo`: path-name arithmetic only (no file system access).
pub struct SplFileInfo {
    class: ClassRef,
    props: PropertyStore,
    state: Mutex<FileInfoState>,
}

impl SplFileInfo {
    /// `new SplFileInfo($path)`.
    pub fn new(path: &[u8]) -> SplFileInfo {
        let class = spl_file_info_class();
        let props = PropertyStore::new(&class);
        let info = SplFileInfo {
            class,
            props,
            state: Mutex::new(FileInfoState::default()),
        };
        info.set_filename(path);
        info
    }

    /// Wraps into a handle.
    pub fn into_ref(self) -> ObjectRef {
        ObjectRef::new(self)
    }

    fn state(&self) -> MutexGuard<'_, FileInfoState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Port of `spl_filesystem_info_set_filename()`.
    fn set_filename(&self, path: &[u8]) {
        let mut path_len = path.len();
        let file_name = if path_len > 1 && path[path_len - 1] == b'/' {
            loop {
                path_len -= 1;
                if !(path_len > 1 && path[path_len - 1] == b'/') {
                    break;
                }
            }
            PhpString::from(&path[..path_len])
        } else {
            PhpString::from(path)
        };
        while path_len > 1 && path[path_len - 1] != b'/' {
            path_len -= 1;
        }
        path_len = path_len.saturating_sub(1);
        let mut st = self.state();
        st.file_name = Some(file_name);
        st.path = Some(PhpString::from(&path[..path_len]));
    }

    /// `getPathname()` (`spl_filesystem_object_get_pathname()` for `SPL_FS_INFO`).
    pub fn pathname(&self) -> PhpString {
        self.state().file_name.clone().unwrap_or_default()
    }

    /// The file-name part `getFilename()` and the debug info compute: the file name without the
    /// path and its slash when the path is a non-empty proper prefix.
    fn file_name_part(&self) -> Option<PhpString> {
        let st = self.state();
        let file_name = st.file_name.as_ref()?;
        let path = st.path.as_ref();
        Some(match path {
            Some(p) if !p.is_empty() && p.len() < file_name.len() => {
                PhpString::from(&file_name.as_bytes()[p.len() + 1..])
            }
            _ => file_name.clone(),
        })
    }

    fn debug_entries(&self) -> Vec<PropertyEntry> {
        let mut out = self.props.entries(&self.class);
        let private = |name: &[u8]| {
            ArrayKey::Str(twig_value::object::mangle_property_name(
                Visibility::Private,
                "SplFileInfo",
                name,
            ))
        };
        out.push(PropertyEntry {
            key: private(b"pathName"),
            value: Some(Value::String(self.pathname())),
            type_name: None,
        });
        if let Some(name) = self.file_name_part() {
            out.push(PropertyEntry {
                key: private(b"fileName"),
                value: Some(Value::String(name)),
                type_name: None,
            });
        }
        out
    }
}

/// The parameters of the implemented methods of `SplFileInfo` (by lowercase name), from
/// PHPSRC/ext/spl/spl_directory.stub.php.
fn spl_file_info_params(method: &str) -> Option<Vec<Param>> {
    Some(match method {
        // __construct(string $filename)
        "__construct" => vec![Param::required("filename")],
        // getBasename(string $suffix = "")
        "getbasename" => vec![Param::optional("suffix", "")],
        "getpathname" | "__tostring" | "getpath" | "getfilename" | "getextension"
        | "__debuginfo" => Vec::new(),
        _ => return None,
    })
}

impl Object for SplFileInfo {
    fn class(&self) -> &ClassRef {
        &self.class
    }

    fn property_store(&self) -> Option<&PropertyStore> {
        Some(&self.props)
    }

    fn method_params(&self, method: &str) -> Option<MethodParams> {
        spl_file_info_params(method).map(MethodParams::internal)
    }

    fn call(
        &self,
        _this: &ObjectRef,
        method: &str,
        args: CallArgs,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        const C: &str = "SplFileInfo";
        let not_initialized = || PhpError::error("Object not initialized");
        match method {
            // __toString is an alias of getPathname.
            "getpathname" | "__tostring" => {
                internal_none(
                    C,
                    if method == "__tostring" {
                        "__toString"
                    } else {
                        "getPathname"
                    },
                    &args,
                )?;
                Ok(Value::String(self.pathname()))
            }
            "getpath" => {
                internal_none(C, "getPath", &args)?;
                Ok(Value::String(self.state().path.clone().unwrap_or_default()))
            }
            "getfilename" => {
                internal_none(C, "getFilename", &args)?;
                self.file_name_part()
                    .map(Value::String)
                    .ok_or_else(not_initialized)
            }
            "getbasename" => {
                let f = format!("{C}::getBasename");
                let a = bind_internal(C, "getBasename", spl_file_info_params, &args)?;
                let suffix = twig_value::param::parse_str(&a[0], &Arg::new(&f, 1, "suffix"), diag)?;
                let name = self.file_name_part().ok_or_else(not_initialized)?;
                Ok(Value::String(php_basename(
                    name.as_bytes(),
                    Some(suffix.as_bytes()),
                )))
            }
            "getextension" => {
                internal_none(C, "getExtension", &args)?;
                let name = self.file_name_part().ok_or_else(not_initialized)?;
                let base = php_basename(name.as_bytes(), None);
                Ok(Value::String(match base.iter().rposition(|&c| c == b'.') {
                    Some(i) => PhpString::from(&base.as_bytes()[i + 1..]),
                    None => PhpString::empty(),
                }))
            }
            "__construct" => {
                let f = format!("{C}::__construct");
                let a = bind_internal(C, "__construct", spl_file_info_params, &args)?;
                let path = twig_value::param::parse_str(&a[0], &Arg::new(&f, 1, "filename"), diag)?;
                if path.contains(&0) {
                    return Err(
                        Arg::new(&f, 1, "filename").value_error("must not contain any null bytes")
                    );
                }
                self.set_filename(path.as_bytes());
                Ok(Value::Null)
            }
            "__debuginfo" => {
                internal_none(C, "__debugInfo", &args)?;
                Ok(Value::Array(twig_value::object::property_entries_to_array(
                    self.debug_entries(),
                )))
            }
            _ if self.class.method(method).is_some() => Err(not_supported(&self.class, method)),
            _ => Err(twig_value::object::undefined_method(&self.class, method)),
        }
    }

    /// `spl_filesystem_object_get_debug_info()` for `var_dump()`.
    fn properties_for(
        &self,
        _this: &ObjectRef,
        purpose: PropertyPurpose,
        _diag: &mut dyn Diagnostics,
    ) -> PhpResult<Vec<PropertyEntry>> {
        Ok(match purpose {
            PropertyPurpose::Debug => self.debug_entries(),
            _ => self.props.entries(&self.class),
        })
    }
}

// ---------------------------------------------------------------------------------------------
// IteratorIterator / FilterIterator / CallbackFilterIterator
// ---------------------------------------------------------------------------------------------

static OUTER_ITERATOR: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::interface("OuterIterator")
        .implements(&builtin::iterator())
        .method("getInnerIterator")
        .build()
});

static ITERATOR_ITERATOR: LazyLock<ClassRef> = LazyLock::new(|| {
    let mut b = ClassBuilder::class("IteratorIterator").implements(&OUTER_ITERATOR);
    for m in [
        "__construct",
        "getInnerIterator",
        "rewind",
        "valid",
        "key",
        "current",
        "next",
    ] {
        b = b.method(m);
    }
    b.build()
});

static FILTER_ITERATOR: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::class("FilterIterator")
        .abstract_class()
        .extends(&ITERATOR_ITERATOR)
        .method("accept")
        .method("__construct")
        .method("rewind")
        .method("next")
        .build()
});

static CALLBACK_FILTER_ITERATOR: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::class("CallbackFilterIterator")
        .extends(&FILTER_ITERATOR)
        .method("__construct")
        .method("accept")
        .build()
});

/// `interface OuterIterator extends Iterator`.
pub fn outer_iterator_interface() -> ClassRef {
    OUTER_ITERATOR.clone()
}

/// `class IteratorIterator implements OuterIterator`.
pub fn iterator_iterator_class() -> ClassRef {
    ITERATOR_ITERATOR.clone()
}

/// `abstract class FilterIterator extends IteratorIterator`.
pub fn filter_iterator_class() -> ClassRef {
    FILTER_ITERATOR.clone()
}

/// `class CallbackFilterIterator extends FilterIterator`.
pub fn callback_filter_iterator_class() -> ClassRef {
    CALLBACK_FILTER_ITERATOR.clone()
}

#[derive(Default)]
struct DualState {
    /// `intern->current.data` / `.key` (`None` = `IS_UNDEF`).
    data: Option<Value>,
    key: Option<Value>,
    /// `intern->current.pos`.
    pos: i64,
}

/// `CallbackFilterIterator` (`spl_dual_it_object` with `DIT_CallbackFilterIterator`).
pub struct CallbackFilterIterator {
    class: ClassRef,
    props: PropertyStore,
    inner: Mutex<Option<ObjectRef>>,
    /// `intern->inner.iterator` (created from the inner object at construction).
    inner_iter: Mutex<Option<Box<dyn ObjectIterator>>>,
    callback: Mutex<Option<ObjectRef>>,
    state: Mutex<DualState>,
}

impl CallbackFilterIterator {
    /// `new CallbackFilterIterator($iterator, $callback)` without the argument checks; `inner`
    /// must be an `Iterator` and `callback` callable.
    pub fn new(
        inner: ObjectRef,
        callback: ObjectRef,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<CallbackFilterIterator> {
        let it = CallbackFilterIterator::uninitialised();
        it.init(inner, callback, diag)?;
        Ok(it)
    }

    /// An object whose constructor has not run (the converter fills it after registering it).
    pub(crate) fn uninitialised() -> CallbackFilterIterator {
        let class = callback_filter_iterator_class();
        let props = PropertyStore::new(&class);
        CallbackFilterIterator {
            class,
            props,
            inner: Mutex::new(None),
            inner_iter: Mutex::new(None),
            callback: Mutex::new(None),
            state: Mutex::new(DualState::default()),
        }
    }

    /// `spl_dual_it_construct()`: keeps the inner object and its `get_iterator` result.
    pub(crate) fn init(
        &self,
        inner: ObjectRef,
        callback: ObjectRef,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<()> {
        let iter = inner.get_iterator(diag)?.ok_or_else(|| {
            PhpError::type_error(format!(
                "CallbackFilterIterator::__construct(): Argument #1 ($iterator) must be of type Iterator, {} given",
                inner.class().display_name()
            ))
        })?;
        *lock(&self.inner) = Some(inner);
        *lock(&self.inner_iter) = Some(iter);
        *lock(&self.callback) = Some(callback);
        Ok(())
    }

    /// The inner iterator.
    pub fn inner(&self) -> Option<ObjectRef> {
        lock(&self.inner).clone()
    }

    fn with_inner<T>(
        &self,
        f: impl FnOnce(&mut dyn ObjectIterator) -> PhpResult<T>,
    ) -> PhpResult<Option<T>> {
        let mut guard = lock(&self.inner_iter);
        match guard.as_mut() {
            Some(it) => f(it.as_mut()).map(Some),
            None => Ok(None),
        }
    }

    /// `spl_dual_it_free()`.
    fn free(&self) {
        let mut st = lock(&self.state);
        st.data = None;
        st.key = None;
    }

    /// `spl_dual_it_rewind()`.
    fn dual_rewind(&self, diag: &mut dyn Diagnostics) -> PhpResult<()> {
        self.free();
        lock(&self.state).pos = 0;
        self.with_inner(|it| it.rewind(diag))?;
        Ok(())
    }

    /// `spl_dual_it_fetch(intern, 1)`: `true` is `SUCCESS`.
    fn dual_fetch(&self, diag: &mut dyn Diagnostics) -> PhpResult<bool> {
        self.free();
        let fetched = self.with_inner(|it| {
            if !it.valid(diag)? {
                return Ok(None);
            }
            let data = it.current(diag)?;
            let key = it.key(diag)?;
            Ok(Some((data, key)))
        })?;
        match fetched.flatten() {
            Some((data, key)) => {
                let mut st = lock(&self.state);
                st.data = Some(data);
                st.key = Some(key);
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// `spl_filter_it_fetch()`: skips elements until `accept()` is true.
    fn filter_fetch(&self, this: &ObjectRef, diag: &mut dyn Diagnostics) -> PhpResult<()> {
        while self.dual_fetch(diag)? {
            let accepted = this.invoke_method("accept", CallArgs::new(), diag)?;
            if convert::to_bool(&accepted, diag)? {
                return Ok(());
            }
            self.with_inner(|it| it.next(diag))?;
        }
        self.free();
        Ok(())
    }

    /// `PHP_METHOD(CallbackFilterIterator, accept)`.
    fn accept(&self, diag: &mut dyn Diagnostics) -> PhpResult<Value> {
        let (data, key) = {
            let st = lock(&self.state);
            match (&st.data, &st.key) {
                (Some(d), Some(k)) => (d.clone(), k.clone()),
                _ => return Ok(Value::Bool(false)),
            }
        };
        let inner = self.inner().map(Value::Object).unwrap_or(Value::Null);
        let callback = lock(&self.callback).clone();
        match callback {
            Some(cb) => cb.invoke(CallArgs::from(vec![data, key, inner]), diag),
            None => Ok(Value::Bool(false)),
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The parameters of the methods of `CallbackFilterIterator` (by lowercase name), from
/// PHPSRC/ext/spl/spl_iterators.stub.php (`__construct()` is not supported by the port).
fn callback_filter_iterator_params(method: &str) -> Option<Vec<Param>> {
    Some(match method {
        // __construct(Iterator $iterator, callable $callback)
        "__construct" => vec![Param::required("iterator"), Param::required("callback")],
        "rewind" | "next" | "valid" | "key" | "current" | "getinneriterator" | "accept" => {
            Vec::new()
        }
        _ => return None,
    })
}

impl Object for CallbackFilterIterator {
    fn class(&self) -> &ClassRef {
        &self.class
    }

    fn property_store(&self) -> Option<&PropertyStore> {
        Some(&self.props)
    }

    fn method_params(&self, method: &str) -> Option<MethodParams> {
        callback_filter_iterator_params(method).map(MethodParams::internal)
    }

    fn call(
        &self,
        this: &ObjectRef,
        method: &str,
        args: CallArgs,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        let none = |class: &str, m: &str| internal_none(class, m, &args);
        match method {
            // FilterIterator::rewind(): spl_filter_it_rewind()
            "rewind" => {
                none("FilterIterator", "rewind")?;
                self.dual_rewind(diag)?;
                self.filter_fetch(this, diag)?;
                Ok(Value::Null)
            }
            // FilterIterator::next(): spl_filter_it_next()
            "next" => {
                none("FilterIterator", "next")?;
                self.free();
                self.with_inner(|it| it.next(diag))?;
                lock(&self.state).pos += 1;
                self.filter_fetch(this, diag)?;
                Ok(Value::Null)
            }
            "valid" => {
                none("IteratorIterator", "valid")?;
                Ok(Value::Bool(lock(&self.state).data.is_some()))
            }
            "key" => {
                none("IteratorIterator", "key")?;
                Ok(lock(&self.state).key.clone().unwrap_or(Value::Null))
            }
            "current" => {
                none("IteratorIterator", "current")?;
                Ok(lock(&self.state).data.clone().unwrap_or(Value::Null))
            }
            "getinneriterator" => {
                none("IteratorIterator", "getInnerIterator")?;
                Ok(self.inner().map(Value::Object).unwrap_or(Value::Null))
            }
            "accept" => {
                none("CallbackFilterIterator", "accept")?;
                self.accept(diag)
            }
            "__construct" => Err(not_supported(&self.class, method)),
            _ => Err(twig_value::object::undefined_method(&self.class, method)),
        }
    }
}

/// A closure that returns the same value whatever its arguments (`function ($el) { return true; }`).
pub fn constant_closure(value: bool) -> ObjectRef {
    let f = Arc::new(value);
    Closure::new(move |_args, _diag| Ok(Value::Bool(*f))).into_ref()
}
