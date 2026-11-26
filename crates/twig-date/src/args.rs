//! Parameter parsing for the date methods (`ZEND_PARSE_PARAMETERS_*` / `zend_parse_method_parameters()` in
//! coercive mode), on top of `twig_value::param`.

use twig_value::object::{CallArgs, FunctionKind, MethodParams, ObjectRef, Param};
use twig_value::param::{self, Arg};
use twig_value::{Diagnostics, PhpResult, PhpString, Value};

/// The parameters of the methods of one date class, by lowercase method name, as its stub declares them
/// (PHPSRC/ext/date/php_date.stub.php, the arginfo of each method). The class reports them through
/// `Object::method_params()`, so that the engine checks named arguments against them as PHP sends them
/// (`zend_handle_named_arg()`), and its methods bind their arguments to them ([`Bound::method`]): one table for
/// both. `None` for a name the class does not declare.
pub(crate) type ParamTable = fn(&str) -> Option<Vec<Param>>;

/// What `Object::method_params()` reports for the lowercase `method` of a class whose methods are internal
/// functions: the parameters `table` declares.
pub(crate) fn method_params(table: ParamTable, method: &str) -> Option<MethodParams> {
    table(method).map(MethodParams::internal)
}

/// The arguments of one internal method call, bound to its parameters.
pub(crate) struct Bound<'a> {
    /// `Class::method` (the declaring class, as `get_active_function_or_method_name()` prints it).
    pub func: &'a str,
    params: Vec<Param>,
    pub values: Vec<Value>,
}

impl<'a> Bound<'a> {
    /// Binds `args` to `params` with the arity and named-argument errors of an internal function.
    pub fn new(func: &'a str, args: &CallArgs, params: Vec<Param>) -> PhpResult<Bound<'a>> {
        let values = args.bind(func, &params, FunctionKind::Internal)?;
        Ok(Bound {
            func,
            params,
            values,
        })
    }

    /// Binds `args` to the parameters `table` declares for the lowercase `method` ([`Bound::new`]); `func` is the
    /// method's name in messages (`DateTime::setTime`).
    pub fn method(
        func: &'a str,
        method: &str,
        table: ParamTable,
        args: &CallArgs,
    ) -> PhpResult<Bound<'a>> {
        let params = table(method);
        debug_assert!(params.is_some(), "no parameters declared for {func}");
        Bound::new(func, args, params.unwrap_or_default())
    }

    fn arg(&self, i: usize) -> Arg<'_> {
        Arg::new(self.func, i as u32 + 1, self.params[i].name)
    }

    /// `Z_PARAM_LONG`.
    pub fn long(&self, i: usize, diag: &mut dyn Diagnostics) -> PhpResult<i64> {
        param::parse_long(&self.values[i], &self.arg(i), diag)
    }

    /// `Z_PARAM_BOOL`.
    pub fn bool(&self, i: usize, diag: &mut dyn Diagnostics) -> PhpResult<bool> {
        param::parse_bool(&self.values[i], &self.arg(i), diag)
    }

    /// `Z_PARAM_STR` / `Z_PARAM_STRING`.
    pub fn string(&self, i: usize, diag: &mut dyn Diagnostics) -> PhpResult<PhpString> {
        param::parse_str(&self.values[i], &self.arg(i), diag)
    }

    /// `Z_PARAM_STRING_OR_NULL`.
    pub fn string_or_null(
        &self,
        i: usize,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Option<PhpString>> {
        param::parse_str_or_null(&self.values[i], &self.arg(i), diag)
    }

    /// `Z_PARAM_PATH` / `Z_PARAM_PATH_STR`: a string without NUL bytes
    /// (`must not contain any null bytes`, a `ValueError`).
    pub fn path(&self, i: usize, diag: &mut dyn Diagnostics) -> PhpResult<PhpString> {
        let s = self.string(i, diag)?;
        if s.as_bytes().contains(&0) {
            return Err(self.arg(i).value_error("must not contain any null bytes"));
        }
        Ok(s)
    }

    /// `Z_PARAM_NUMBER`.
    pub fn number(&self, i: usize, diag: &mut dyn Diagnostics) -> PhpResult<twig_value::Number> {
        param::parse_number(&self.values[i], &self.arg(i), diag)
    }

    /// `Z_PARAM_ARRAY` / `Z_PARAM_ARRAY_HT`.
    pub fn array(&self, i: usize) -> PhpResult<twig_value::Array> {
        param::parse_array(&self.values[i], &self.arg(i))
    }

    /// `Z_PARAM_OBJECT_OF_CLASS`: `must be of type Class, X given`.
    pub fn object_of(&self, i: usize, class: &str) -> PhpResult<ObjectRef> {
        match &self.values[i] {
            Value::Object(o) if o.instance_of(class) => Ok(o.clone()),
            other => Err(self.arg(i).type_error(&format!("of type {class}"), other)),
        }
    }

    /// `Z_PARAM_OBJECT_OF_CLASS_OR_NULL`: `must be of type ?Class, X given`.
    pub fn object_of_or_null(&self, i: usize, class: &str) -> PhpResult<Option<ObjectRef>> {
        match &self.values[i] {
            Value::Null => Ok(None),
            Value::Object(o) if o.instance_of(class) => Ok(Some(o.clone())),
            other => Err(self.arg(i).type_error(&format!("of type ?{class}"), other)),
        }
    }
}
