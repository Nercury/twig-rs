mod core;
mod escaper;

use environment::Environment;

pub use self::core::CoreExtension;
pub use self::escaper::EscaperExtension;

/// Implement this trait to create a new Twig extension.
pub trait Extension {
    fn apply(env: &mut Environment);
}
