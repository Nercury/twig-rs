pub mod core;

use environment::StagedEnvironment;

pub trait Extension {
    fn apply(env: &mut StagedEnvironment);
}
