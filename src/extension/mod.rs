pub mod core;

use Environment;

pub trait Extension {
    fn apply(env: &mut Environment);
}
