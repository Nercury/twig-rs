#[derive(Copy, Clone)]
pub struct Delimiters {
    pub start: &'static str,
    pub end: &'static str,
}

impl Delimiters {
    pub fn new(start: &'static str, end: &'static str) -> Delimiters {
        Delimiters {
            start: start,
            end: end,
        }
    }
}
