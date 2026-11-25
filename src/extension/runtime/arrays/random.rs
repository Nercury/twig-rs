//! PHP's per-request Mersenne Twister state (`RANDOM_G(mt19937)`, PHPSRC/ext/random/random.c), which `mt_rand()`,
//! `shuffle()` and `array_rand()` share, so Twig's `random` function and `shuffle` filter draw from one generator.
//!
//! PHP keeps one state per request and seeds it from the CSPRNG on first use unless `mt_srand()` ran. A render is
//! synchronous on its thread, so twig-rs keeps one state per thread, seeded on first use like
//! [`Mt19937::from_entropy`] (see `reference/deviations.md` for unseeded randomness).

use std::cell::RefCell;

use twig_value::builtins::array::Mt19937;

thread_local! {
    static MT_RAND: RefCell<Option<Mt19937>> = const { RefCell::new(None) };
}

/// Runs `f` with this thread's generator, seeding it on first use (`php_random_mt19937_seed_default()`). `f` must
/// not call back into user code.
pub(crate) fn with_mt_rand<R>(f: impl FnOnce(&mut Mt19937) -> R) -> R {
    MT_RAND.with_borrow_mut(|rng| f(rng.get_or_insert_with(Mt19937::from_entropy)))
}

/// `mt_srand($seed)`: reseeds this thread's generator (`MT_RAND_MT19937` mode), for tests that compare seeded
/// sequences with PHP.
#[cfg(test)]
pub(crate) fn mt_srand(seed: u32) {
    MT_RAND.with_borrow_mut(|rng| *rng = Some(Mt19937::new(seed)));
}
