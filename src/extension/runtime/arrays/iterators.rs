//! The SPL iterators Twig's array filters create (PHPSRC/ext/spl/spl_iterators.c): the `IteratorIterator` and
//! `CallbackFilterIterator` objects `filter` returns for a `Traversable` (lazy: the arrow runs while the result is
//! iterated), and the `LimitIterator` that `slice` consumes with `iterator_to_array()`.
//!
//! The objects keep the state of PHP's `spl_dual_it_object` (the inner iterator, the fetched element and the
//! position) and call the inner iterator in upstream's order, so the calls a userland `Iterator` or a generator sees
//! (and when an arrow runs between them) are PHP's.

use std::sync::{LazyLock, Mutex, MutexGuard};

use twig_value::object::{
    CallArgs, ClassBuilder, ClassRef, FunctionKind, Object, ObjectIterator, ObjectRef, builtin,
};
use twig_value::param::{self, Arg};
use twig_value::{
    Array, ArrayKey, Diagnostics, KeyContext, PhpError, PhpResult, ThrowableClass, Value, convert,
};

use super::callables::{call_value, check_callable};
use crate::vm::Exception;

static OUTER_ITERATOR: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::interface("OuterIterator")
        .implements(&builtin::iterator())
        .method("getInnerIterator")
        .build()
});

static ITERATOR_ITERATOR: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::class("IteratorIterator")
        .implements(&OUTER_ITERATOR)
        .method("__construct")
        .method("getInnerIterator")
        .method("rewind")
        .method("valid")
        .method("key")
        .method("current")
        .method("next")
        .build()
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

/// Which SPL class a [`DualIterator`] is.
enum DualKind {
    /// `IteratorIterator`.
    IteratorIterator,
    /// `CallbackFilterIterator` with its callback.
    CallbackFilter(Value),
}

/// The mutable part of `spl_dual_it_object`.
struct DualState {
    /// `inner.iterator`; taken out while one of its calls runs (it may run user code).
    iterator: Option<Box<dyn ObjectIterator>>,
    /// `current.data` (`None` is `IS_UNDEF`).
    data: Option<Value>,
    /// `current.key` (`None` is `IS_UNDEF`).
    key: Option<Value>,
    /// `current.pos`.
    pos: i64,
}

/// An `IteratorIterator` or `CallbackFilterIterator` object (`spl_dual_it_object`).
struct DualIterator {
    class: ClassRef,
    kind: DualKind,
    /// `inner.zobject`: the inner object (after `IteratorAggregate::getIterator()` for an `IteratorIterator`).
    inner_object: ObjectRef,
    state: Mutex<DualState>,
}

impl DualIterator {
    fn new(kind: DualKind, inner_object: ObjectRef, iterator: Box<dyn ObjectIterator>) -> Self {
        let class = match kind {
            DualKind::IteratorIterator => ITERATOR_ITERATOR.clone(),
            DualKind::CallbackFilter(_) => CALLBACK_FILTER_ITERATOR.clone(),
        };
        DualIterator {
            class,
            kind,
            inner_object,
            state: Mutex::new(DualState {
                iterator: Some(iterator),
                data: None,
                key: None,
                pos: 0,
            }),
        }
    }

    fn lock(&self) -> MutexGuard<'_, DualState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Runs `f` on the inner iterator without holding the lock.
    fn with_inner<R>(
        &self,
        f: impl FnOnce(&mut dyn ObjectIterator) -> PhpResult<R>,
    ) -> PhpResult<R> {
        let taken = self.lock().iterator.take();
        let Some(mut iterator) = taken else {
            return Err(PhpError::error(format!(
                "Cannot use {} from inside one of its own calls (twig-rs limitation)",
                self.class.display_name()
            )));
        };
        let result = f(iterator.as_mut());
        self.lock().iterator = Some(iterator);
        result
    }

    /// `spl_dual_it_free()`.
    fn free(&self) {
        let mut state = self.lock();
        state.data = None;
        state.key = None;
    }

    /// `spl_dual_it_rewind()`.
    fn dual_rewind(&self, diag: &mut dyn Diagnostics) -> PhpResult<()> {
        {
            let mut state = self.lock();
            state.data = None;
            state.key = None;
            state.pos = 0;
        }
        self.with_inner(|it| it.rewind(diag))
    }

    /// `spl_dual_it_fetch()`: `Ok(false)` is `FAILURE` without an exception.
    fn dual_fetch(&self, check_more: bool, diag: &mut dyn Diagnostics) -> PhpResult<bool> {
        self.free();
        self.with_inner(|it| {
            if check_more && !it.valid(diag)? {
                return Ok(false);
            }
            let data = it.current(diag)?;
            self.lock().data = Some(data);
            let key = it.key(diag)?;
            self.lock().key = Some(key);
            Ok(true)
        })
    }

    /// `spl_dual_it_next($intern, 1)`.
    fn dual_next(&self, diag: &mut dyn Diagnostics) -> PhpResult<()> {
        self.free();
        let result = self.with_inner(|it| it.next(diag));
        self.lock().pos += 1;
        result
    }

    /// `spl_filter_it_fetch()`: fetches until `accept()` returns a truthy value.
    fn filter_fetch(&self, this: &ObjectRef, diag: &mut dyn Diagnostics) -> PhpResult<()> {
        loop {
            match self.dual_fetch(true, diag) {
                Ok(true) => {}
                Ok(false) => break,
                Err(e) => {
                    self.free();
                    return Err(e);
                }
            }
            let accepted = this.invoke_method("accept", CallArgs::new(), diag)?;
            if convert::to_bool(&accepted, diag)? {
                return Ok(());
            }
            self.with_inner(|it| it.next(diag))?;
        }
        self.free();
        Ok(())
    }
}

/// `ZEND_PARSE_PARAMETERS_NONE()` of an internal method.
fn no_args(args: &CallArgs, function: &str) -> PhpResult<()> {
    args.bind(function, &[], FunctionKind::Internal).map(|_| ())
}

impl Object for DualIterator {
    fn class(&self) -> &ClassRef {
        &self.class
    }

    fn call(
        &self,
        this: &ObjectRef,
        method: &str,
        args: CallArgs,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        let filter = matches!(self.kind, DualKind::CallbackFilter(_));
        match method {
            // spl_dual_it_construct(): the object is constructed already
            "__construct" => Err(PhpError::new(
                ThrowableClass::BadMethodCallException,
                format!(
                    "{}::getIterator() must be called exactly once per instance",
                    self.class.display_name()
                ),
            )),
            "getinneriterator" => {
                no_args(&args, "IteratorIterator::getInnerIterator")?;
                Ok(Value::Object(self.inner_object.clone()))
            }
            "valid" => {
                no_args(&args, "IteratorIterator::valid")?;
                Ok(Value::Bool(self.lock().data.is_some()))
            }
            "key" => {
                no_args(&args, "IteratorIterator::key")?;
                Ok(self.lock().key.clone().unwrap_or_default())
            }
            "current" => {
                no_args(&args, "IteratorIterator::current")?;
                Ok(self.lock().data.clone().unwrap_or_default())
            }
            "rewind" if filter => {
                no_args(&args, "FilterIterator::rewind")?;
                self.dual_rewind(diag)?;
                self.filter_fetch(this, diag)?;
                Ok(Value::Null)
            }
            "next" if filter => {
                no_args(&args, "FilterIterator::next")?;
                self.dual_next(diag)?;
                self.filter_fetch(this, diag)?;
                Ok(Value::Null)
            }
            "rewind" => {
                no_args(&args, "IteratorIterator::rewind")?;
                self.dual_rewind(diag)?;
                self.dual_fetch(true, diag)?;
                Ok(Value::Null)
            }
            "next" => {
                no_args(&args, "IteratorIterator::next")?;
                self.dual_next(diag)?;
                self.dual_fetch(true, diag)?;
                Ok(Value::Null)
            }
            "accept" => match &self.kind {
                // CallbackFilterIterator::accept(): callback($current, $key, $inner), its value unconverted
                DualKind::CallbackFilter(callback) => {
                    no_args(&args, "CallbackFilterIterator::accept")?;
                    let (data, key) = {
                        let state = self.lock();
                        (state.data.clone(), state.key.clone())
                    };
                    match (data, key) {
                        (Some(data), Some(key)) => call_value(
                            callback,
                            vec![data, key, Value::Object(self.inner_object.clone())],
                            diag,
                        ),
                        _ => Ok(Value::Bool(false)),
                    }
                }
                DualKind::IteratorIterator => {
                    Err(twig_value::object::undefined_method(&self.class, method))
                }
            },
            _ => Err(twig_value::object::undefined_method(&self.class, method)),
        }
    }
}

/// The class's iterator (`$ce->get_iterator()`), which every `Traversable` has.
fn iterator_of(
    object: &ObjectRef,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Box<dyn ObjectIterator>> {
    object.get_iterator(diag)?.ok_or_else(|| {
        PhpError::error(format!(
            "Object of type {} is not traversable",
            object.class().display_name()
        ))
    })
}

/// `new \CallbackFilterIterator(new \IteratorIterator($traversable), $callback)` (Twig: `CoreExtension::filter()`
/// on a `Traversable`).
///
/// The `IteratorIterator` constructor (`spl_dual_it_construct()`, `DIT_IteratorIterator`) calls `getIterator()` of
/// an `IteratorAggregate` that is not an `Iterator` once (`spl_get_iterator_from_aggregate()`) and takes the
/// iterator of the result; the `CallbackFilterIterator` constructor checks the callback (`Z_PARAM_FUNC`) and takes
/// the iterator of the `IteratorIterator`. Nothing is iterated until the result is.
pub(super) fn callback_filter_iterator(
    traversable: &ObjectRef,
    callback: &Value,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Value> {
    let mut inner = traversable.clone();
    if !inner.instance_of("Iterator") && inner.instance_of("IteratorAggregate") {
        let class = inner.class().display_name().to_owned();
        inner = match inner.invoke_method("getiterator", CallArgs::new(), diag)? {
            Value::Object(object) if object.instance_of("Traversable") => object,
            _ => {
                return Err(PhpError::new(
                    ThrowableClass::LogicException,
                    format!(
                        "{class}::getIterator() must return an object that implements Traversable"
                    ),
                ));
            }
        };
    }
    let iterator = iterator_of(&inner, diag)?;
    let inner_iterator = ObjectRef::new(DualIterator::new(
        DualKind::IteratorIterator,
        inner,
        iterator,
    ));
    check_callable(
        callback,
        "CallbackFilterIterator::__construct",
        2,
        "callback",
        false,
    )?;
    let iterator = iterator_of(&inner_iterator, diag)?;
    Ok(Value::Object(ObjectRef::new(DualIterator::new(
        DualKind::CallbackFilter(callback.clone()),
        inner_iterator,
        iterator,
    ))))
}

/// The state of a `LimitIterator` (`spl_dual_it_object` with `u.limit`).
struct Limit<'a> {
    object: &'a ObjectRef,
    iterator: Box<dyn ObjectIterator>,
    offset: i64,
    count: i64,
    data: Option<Value>,
    key: Option<Value>,
    pos: i64,
}

impl Limit<'_> {
    fn free(&mut self) {
        self.data = None;
        self.key = None;
    }

    /// `spl_dual_it_rewind()`.
    fn dual_rewind(&mut self, diag: &mut dyn Diagnostics) -> PhpResult<()> {
        self.free();
        self.pos = 0;
        self.iterator.rewind(diag)
    }

    /// `spl_dual_it_fetch()`.
    fn fetch(&mut self, check_more: bool, diag: &mut dyn Diagnostics) -> PhpResult<()> {
        self.free();
        if !check_more || self.iterator.valid(diag)? {
            self.data = Some(self.iterator.current(diag)?);
            self.key = Some(self.iterator.key(diag)?);
        }
        Ok(())
    }

    /// `spl_dual_it_next($intern, 1)`.
    fn dual_next(&mut self, diag: &mut dyn Diagnostics) -> PhpResult<()> {
        self.free();
        let result = self.iterator.next(diag);
        self.pos = self.pos.wrapping_add(1);
        result
    }

    /// `spl_limit_it_relative_pos()` against the count: whether `pos` is within the limit.
    fn within_count(&self, pos: i64) -> bool {
        self.count == -1 || pos.wrapping_sub(self.offset) < self.count
    }

    fn out_of_bounds(message: String) -> PhpError {
        PhpError::new(ThrowableClass::OutOfBoundsException, message)
    }

    /// `spl_limit_it_seek()`.
    fn seek(&mut self, pos: i64, diag: &mut dyn Diagnostics) -> PhpResult<()> {
        self.free();
        if pos < self.offset {
            return Err(Self::out_of_bounds(format!(
                "Cannot seek to {pos} which is below the offset {}",
                self.offset
            )));
        }
        if !self.within_count(pos) {
            return Err(Self::out_of_bounds(format!(
                "Cannot seek to {pos} which is behind offset {} plus count {}",
                self.offset, self.count
            )));
        }
        if pos != self.pos && self.object.instance_of("SeekableIterator") {
            self.free();
            self.object
                .invoke_method("seek", CallArgs::from(vec![Value::Int(pos)]), diag)?;
            self.pos = pos;
            // spl_limit_it_valid()
            if self.within_count(self.pos) && self.iterator.valid(diag)? {
                self.fetch(false, diag)?;
            }
        } else {
            // emulate the forward seek by next() calls; a backward seek is done by a previous rewind()
            if pos < self.pos {
                self.dual_rewind(diag)?;
            }
            while pos > self.pos && self.iterator.valid(diag)? {
                self.dual_next(diag)?;
            }
            if self.iterator.valid(diag)? {
                self.fetch(true, diag)?;
            }
        }
        Ok(())
    }

    /// `LimitIterator::rewind()`.
    fn rewind(&mut self, diag: &mut dyn Diagnostics) -> PhpResult<()> {
        self.dual_rewind(diag)?;
        self.seek(self.offset, diag)
    }

    /// `LimitIterator::valid()`.
    fn valid(&self) -> bool {
        self.within_count(self.pos) && self.data.is_some()
    }

    /// `LimitIterator::next()`.
    fn next(&mut self, diag: &mut dyn Diagnostics) -> PhpResult<()> {
        self.dual_next(diag)?;
        if self.within_count(self.pos) {
            self.fetch(true, diag)?;
        }
        Ok(())
    }
}

/// `iterator_to_array(new \LimitIterator($iterator, $offset, $limit), $preserveKeys)` inside Twig's
/// `try { ... } catch (\OutOfBoundsException) { return []; }` (`CoreExtension::slice()` on an `Iterator`).
///
/// The constructor parses `int $offset`, `int $limit` (`LimitIterator::__construct(): Argument #2 ($offset) must be
/// of type int, ...`) and checks their ranges, then takes the iterator of `$iterator`; `iterator_to_array()` parses
/// `bool $preserve_keys` and traverses the `LimitIterator` (`rewind()`: rewind and seek to the offset, through
/// `seek()` for a `SeekableIterator`; then `valid()`, `current()`, `key()`, `next()`), keys going through
/// `array_set_zval_key()`.
pub(super) fn limit_iterator_to_array(
    iterator: &ObjectRef,
    offset: &Value,
    limit: &Value,
    preserve_keys: &Value,
    diag: &mut dyn Diagnostics,
) -> Result<Array, Exception> {
    let result = (|| -> PhpResult<Array> {
        let offset_arg = Arg::new("LimitIterator::__construct", 2, "offset");
        let limit_arg = Arg::new("LimitIterator::__construct", 3, "limit");
        let offset = param::parse_long(offset, &offset_arg, diag)?;
        let count = param::parse_long(limit, &limit_arg, diag)?;
        if offset < 0 {
            return Err(offset_arg.value_error("must be greater than or equal to 0"));
        }
        if count < -1 {
            return Err(limit_arg.value_error("must be greater than or equal to -1"));
        }
        let mut limit = Limit {
            object: iterator,
            iterator: iterator_of(iterator, diag)?,
            offset,
            count,
            data: None,
            key: None,
            pos: 0,
        };
        let preserve_keys = param::parse_bool(
            preserve_keys,
            &Arg::new("iterator_to_array", 2, "preserve_keys"),
            diag,
        )?;
        let mut out = Array::new();
        limit.rewind(diag)?;
        while limit.valid() {
            let data = limit.data.clone().unwrap_or_default();
            if preserve_keys {
                let key = limit.key.clone().unwrap_or_default();
                out.insert(ArrayKey::from_value(&key, KeyContext::Write, diag)?, data);
            } else {
                out.push(data)?;
            }
            limit.next(diag)?;
        }
        Ok(out)
    })();
    match result {
        Ok(array) => Ok(array),
        Err(error) => {
            let exception = Exception::from(error);
            if exception
                .php_error()
                .is_some_and(|e| e.is_a(&ThrowableClass::OutOfBoundsException))
            {
                Ok(Array::new())
            } else {
                Err(exception)
            }
        }
    }
}
