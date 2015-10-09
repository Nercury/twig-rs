use std::fmt;

/// Parsed twig number representation.
#[derive(PartialEq, Debug, Copy, Clone)]
pub enum TwigNumber<'a> {
    Big(&'a str),
    Float(f64),
    Int(u64),
}

/// Parsed Twig string representation.
#[derive(Eq, PartialEq, Copy, Clone)]
pub struct TwigString<'a>(&'a str);

impl<'a> TwigString<'a> {
    pub fn new<'r>(source: &'r str) -> TwigString<'r> {
        TwigString(source)
    }
}

impl<'a> fmt::Debug for TwigString<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        let &TwigString(ref v) = self;
        write!(f, "{}", v)
    }
}

/// Parsed twig number representation.
#[derive(PartialEq, Debug, Clone)]
pub enum DebugTwigNumber {
    Big(String),
    Float(f64),
    Int(u64),
}

impl<'a> Into<DebugTwigNumber> for TwigNumber<'a> {
    fn into(self) -> DebugTwigNumber {
        match self {
            TwigNumber::Big(n) => DebugTwigNumber::Big(n.to_string()),
            TwigNumber::Float(v) => DebugTwigNumber::Float(v),
            TwigNumber::Int(v) => DebugTwigNumber::Int(v),
        }
    }
}

/// Parsed Twig string representation.
#[derive(Eq, PartialEq, Debug, Clone)]
pub struct DebugTwigString(String);

impl<'a> Into<DebugTwigString> for TwigString<'a> {
    fn into(self) -> DebugTwigString {
        DebugTwigString(self.0.to_string())
    }
}
