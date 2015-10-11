#[derive(PartialEq, Debug, Copy, Clone)]
pub enum TwigValueRef<'a> {
    Num(TwigNumber<'a>),
    Str(&'a str),
}

impl<'a> TwigValueRef<'a> {
    pub fn new_big_num<'c>(num: &'c str) -> TwigValueRef<'c> {
        TwigValueRef::Num(TwigNumber::Big(num))
    }

    pub fn new_float<'c>(num: f64) -> TwigValueRef<'c> {
        TwigValueRef::Num(TwigNumber::Float(num))
    }

    pub fn new_int<'c>(num: i64) -> TwigValueRef<'c> {
        TwigValueRef::Num(TwigNumber::Int(num))
    }

    pub fn new_str<'c>(s: &'c str) -> TwigValueRef<'c> {
        TwigValueRef::Str(s)
    }
}

impl<'a> Into<OwnedTwigValue> for TwigValueRef<'a> {
    fn into(self) -> OwnedTwigValue {
        match self {
            TwigValueRef::Num(n) => OwnedTwigValue::Num(n.into()),
            TwigValueRef::Str(s) => OwnedTwigValue::Str(s.into()),
        }
    }
}

#[derive(PartialEq, Debug, Clone)]
pub enum OwnedTwigValue {
    Num(OwnedTwigNumber),
    Str(String),
}

/// Parsed twig number representation.
#[derive(PartialEq, Debug, Copy, Clone)]
pub enum TwigNumber<'a> {
    Big(&'a str),
    Float(f64),
    Int(i64),
}

/// Parsed twig number representation.
#[derive(PartialEq, Debug, Clone)]
pub enum OwnedTwigNumber {
    Big(String),
    Float(f64),
    Int(i64),
}

impl<'a> Into<OwnedTwigNumber> for TwigNumber<'a> {
    fn into(self) -> OwnedTwigNumber {
        match self {
            TwigNumber::Big(n) => OwnedTwigNumber::Big(n.to_string()),
            TwigNumber::Float(v) => OwnedTwigNumber::Float(v),
            TwigNumber::Int(v) => OwnedTwigNumber::Int(v),
        }
    }
}
