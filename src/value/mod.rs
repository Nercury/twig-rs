#[derive(PartialEq, Debug, Copy, Clone)]
pub enum TwigValueRef<'a> {
    Num(TwigNumberRef<'a>),
    Str(&'a str),
}

impl<'a> TwigValueRef<'a> {
    pub fn new_big_num<'c>(num: &'c str) -> TwigValueRef<'c> {
        TwigValueRef::Num(TwigNumberRef::Big(num))
    }

    pub fn new_float<'c>(num: f64) -> TwigValueRef<'c> {
        TwigValueRef::Num(TwigNumberRef::Float(num))
    }

    pub fn new_int<'c>(num: i64) -> TwigValueRef<'c> {
        TwigValueRef::Num(TwigNumberRef::Int(num))
    }

    pub fn new_str<'c>(s: &'c str) -> TwigValueRef<'c> {
        TwigValueRef::Str(s)
    }
}

impl<'a> Into<TwigValue> for TwigValueRef<'a> {
    fn into(self) -> TwigValue {
        match self {
            TwigValueRef::Num(n) => TwigValue::Num(n.into()),
            TwigValueRef::Str(s) => TwigValue::Str(s.into()),
        }
    }
}

#[derive(PartialEq, Debug, Clone)]
pub enum TwigValue {
    Num(TwigNumber),
    Str(String),
}

/// Parsed twig number representation.
#[derive(PartialEq, Debug, Copy, Clone)]
pub enum TwigNumberRef<'a> {
    Big(&'a str),
    Float(f64),
    Int(i64),
}

/// Parsed twig number representation.
#[derive(PartialEq, Debug, Clone)]
pub enum TwigNumber {
    Big(String),
    Float(f64),
    Int(i64),
}

impl<'a> Into<TwigNumber> for TwigNumberRef<'a> {
    fn into(self) -> TwigNumber {
        match self {
            TwigNumberRef::Big(n) => TwigNumber::Big(n.to_string()),
            TwigNumberRef::Float(v) => TwigNumber::Float(v),
            TwigNumberRef::Int(v) => TwigNumber::Int(v),
        }
    }
}
