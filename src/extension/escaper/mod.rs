use crate::environment::Environment;
use crate::extension::Extension;

pub struct EscaperExtension;

impl Extension for EscaperExtension {
    fn apply(_env: &mut Environment) {}
}
