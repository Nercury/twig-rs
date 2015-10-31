use std::fmt;
use error::{ Location, Error };

/// Stack trace record.
#[derive(Clone, Debug)]
pub enum TraceEntry {
    /// Trace source file change that caused the error.
    SourceFile { target: String },
    /// Trace operator call that caused the error.
    Operator { target: String, extension: String },
    /// Trace position in last known source that cause the error.
    Position { from: Location },
}

#[derive(Debug, Clone)]
pub enum CastTarget {
    Int,
    Float,
    Number,
}

#[derive(Clone, Debug)]
pub enum CastError {
    /// Float is infinite, target can not be infinite.
    FloatIsInfinite(f64),
    /// Float is not a number, target has to be a number.
    FloatNotANumber(f64),
    /// Float can not be represented, target does not support the range.
    FloatRange(f64),
    /// Null can not be represented.
    Null,
    /// Target can not be created from Array.
    Array,
    /// Target is not be created from Hash.
    Hash,
    /// Target is not be created from Object.
    Object,
    /// Target is not be created from Function.
    Function,
    /// Empty string can not be represented.
    StringEmpty,
    /// String could not be parsed as number.
    StringNotNumerical(String),
}

impl fmt::Display for CastError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            CastError::FloatIsInfinite(v) => write!(f, "Infinite float {:?}", v),
            CastError::FloatNotANumber(v) => write!(f, "Nonnumerical float {:?}", v),
            CastError::FloatRange(v) => write!(f, "Out-of-range float {:?}", v),
            CastError::Null => "Null".fmt(f),
            CastError::Array => "Array".fmt(f),
            CastError::Hash => "Associative array".fmt(f),
            CastError::Object => "Object".fmt(f),
            CastError::Function => "Function".fmt(f),
            CastError::StringEmpty => "Empty string".fmt(f),
            CastError::StringNotNumerical(ref v) => write!(f, "Nonnumerical string {:?}", v),
        }
    }
}

#[derive(Clone, Debug)]
/// Runtime error message.
pub enum RuntimeError {
    /// Callable invoked with argument count that does not match defined count.
    InvalidArgumentCount { defined: usize, given: usize },
    /// Tried to access object property that does not exist.
    ObjectHasNoProperty(String),
    /// Tried to access object method that does not exist.
    ObjectHasNoMethod(String),
    /// Tried to access object property but it was a method.
    ObjectPropertyIsNotMethod(String),
    /// Tried to access object method but it was a property.
    ObjectMethodIsNotProperty(String),
    /// Tried to call object method with wrong argument count.
    ObjectMethodArgumentMismatch { name: String, defined: u16, given: u16 },
    /// Value casting error.
    ImpossibleCast { target: CastTarget, reason: CastError },
}

impl RuntimeError {
    pub fn at(self, stack_trace: Vec<TraceEntry>) -> TracedRuntimeError {
        TracedRuntimeError {
            message: self,
            stack_trace: stack_trace
        }
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            RuntimeError::InvalidArgumentCount { ref defined, ref given } => {
                write!(f, "Target requires {} arguments, called with {}", defined, given)
            },
            RuntimeError::ObjectHasNoProperty(ref name) => {
                write!(f, "Object has no property {:?}", name)
            },
            RuntimeError::ObjectHasNoMethod(ref name) => {
                write!(f, "Object has no method {:?}", name)
            },
            RuntimeError::ObjectPropertyIsNotMethod(ref name) => {
                write!(f, "Property is not a method {:?}", name)
            },
            RuntimeError::ObjectMethodIsNotProperty(ref name) => {
                write!(f, "Method is not a property {:?}", name)
            },
            RuntimeError::ObjectMethodArgumentMismatch { ref name, ref defined, ref given } => {
                write!(f, "Method {:?} requires {} arguments, called with {}", name, defined, given)
            },
            RuntimeError::ImpossibleCast { ref target, ref reason } => {
                write!(f, "{:?} is not {}", reason, match *target {
                    CastTarget::Float => "a float",
                    CastTarget::Int => "an integer",
                    CastTarget::Number => "a number",
                })
            }
        }
    }
}

/// Runtime error with stack trace.
#[derive(Clone, Debug)]
pub struct TracedRuntimeError {
    pub message: RuntimeError,
    pub stack_trace: Vec<TraceEntry>,
}

impl TracedRuntimeError {
    pub fn new(message: RuntimeError) -> TracedRuntimeError {
        TracedRuntimeError {
            message: message,
            stack_trace: Vec::new(),
        }
    }
}

impl fmt::Display for TracedRuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self.message)
    }
}

impl From<TracedRuntimeError> for Error {
    fn from(inner: TracedRuntimeError) -> Error {
        Error::Runtime(inner)
    }
}
