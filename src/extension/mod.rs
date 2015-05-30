pub mod core;

use environment::ExtendEnvironment;

pub trait Apply {
    fn apply<E: ExtendEnvironment>(env: &mut E);
}
