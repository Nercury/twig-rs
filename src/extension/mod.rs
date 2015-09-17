pub mod core;

use Environment;

/// Implement this trait to create a new Twig extension.
pub trait Extension {
    fn apply(env: &mut Environment);
}
