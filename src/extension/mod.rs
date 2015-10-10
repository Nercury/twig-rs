mod core;

use Environment;

pub use self::core::CoreExtension;

/// Implement this trait to create a new Twig extension.
pub trait Extension {
    fn apply(env: &mut Environment);
}
