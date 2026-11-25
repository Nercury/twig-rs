//! PHP's seeded randomness: the MT19937 engine (PHPSRC/ext/random/engine_mt19937.c), its range
//! mapping (`php_random_range()`, PHPSRC/ext/random/random.c), `mt_rand()`/`mt_srand()`, and the
//! array functions built on it, `shuffle()` and `array_rand()` (PHPSRC/ext/standard/array.c).
//!
//! PHP keeps one MT19937 state per request, seeded from the CSPRNG on first use unless
//! `mt_srand()` ran; here the state is an explicit [`Mt19937`] value. With the same seed, every
//! function produces PHP's exact sequence (`mt_srand(42); mt_rand(1, 100)` gives the same number in
//! both).

use std::hash::{BuildHasher, Hasher};

use crate::array::Array;
use crate::diag::Diagnostics;
use crate::error::{PhpError, PhpResult, ThrowableClass};
use crate::param::{self, Arg};
use crate::value::Value;

/// `N`: length of the state vector.
const N: usize = 624;
/// `M`: the period parameter.
const M: usize = 397;
/// `PHP_RANDOM_RANGE_ATTEMPTS`.
const RANGE_ATTEMPTS: u32 = 50;

/// `PHP_MT_RAND_MAX` (`mt_getrandmax()`): the largest value `mt_rand()` without arguments returns.
pub const MT_RAND_MAX: i64 = 0x7FFF_FFFF;
/// `MT_RAND_MT19937`: the correct Mersenne Twister (default).
pub const MT_RAND_MT19937: i64 = 0;
/// `MT_RAND_PHP`: PHP's historical, incorrect variant (deprecated since PHP 8.3).
pub const MT_RAND_PHP: i64 = 1;

/// The variant of the twist and of `mt_rand()`'s range scaling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MtMode {
    /// `MT_RAND_MT19937`.
    Mt19937,
    /// `MT_RAND_PHP`: `twist_php()` (uses the low bit of the wrong word) and the "bad scaling"
    /// of `mt_rand($min, $max)`.
    Php,
}

/// The MT19937 engine state (`php_random_status_state_mt19937`).
#[derive(Clone)]
pub struct Mt19937 {
    state: [u32; N],
    count: usize,
    mode: MtMode,
}

impl std::fmt::Debug for Mt19937 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Mt19937")
            .field("count", &self.count)
            .field("mode", &self.mode)
            .finish_non_exhaustive()
    }
}

/// `hiBit(u) | loBits(v)`.
fn mix_bits(u: u32, v: u32) -> u32 {
    (u & 0x8000_0000) | (v & 0x7FFF_FFFF)
}

/// `twist(m, u, v)` (`MT_RAND_MT19937`).
fn twist(m: u32, u: u32, v: u32) -> u32 {
    m ^ (mix_bits(u, v) >> 1) ^ ((v & 1).wrapping_neg() & 0x9908_b0df)
}

/// `twist_php(m, u, v)` (`MT_RAND_PHP`): the low bit of `u` instead of `v`.
fn twist_php(m: u32, u: u32, v: u32) -> u32 {
    m ^ (mix_bits(u, v) >> 1) ^ ((u & 1).wrapping_neg() & 0x9908_b0df)
}

/// An unpredictable 32-bit seed (stand-in for `php_random_bytes_silent()`): the standard library's
/// randomly keyed hasher over the clock.
fn entropy_seed() -> u32 {
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    h.write_u128(nanos);
    h.finish() as u32
}

/// `Random\BrokenRandomEngineError` (`extends Random\RandomError extends Error`).
fn broken_engine() -> PhpError {
    PhpError::new(
        ThrowableClass::other(
            "Random\\BrokenRandomEngineError",
            ThrowableClass::other("Random\\RandomError", ThrowableClass::Error),
        ),
        format!("Failed to generate an acceptable random number in {RANGE_ATTEMPTS} attempts"),
    )
}

impl Mt19937 {
    /// A generator seeded with `seed` in `MT_RAND_MT19937` mode (`mt_srand($seed)`).
    pub fn new(seed: u32) -> Mt19937 {
        Mt19937::with_mode(seed, MtMode::Mt19937)
    }

    /// A generator seeded with `seed` in the given mode.
    pub fn with_mode(seed: u32, mode: MtMode) -> Mt19937 {
        let mut mt = Mt19937 {
            state: [0; N],
            count: 0,
            mode,
        };
        mt.seed(seed);
        mt
    }

    /// A generator with an unpredictable seed, PHP's state before any `mt_srand()` call
    /// (`php_random_mt19937_seed_default()`). PHP takes the seed from the CSPRNG; here it comes from
    /// the standard library's randomly keyed hasher mixed with the clock, which is not
    /// cryptographically strong (neither is MT19937).
    pub fn from_entropy() -> Mt19937 {
        Mt19937::new(entropy_seed())
    }

    /// The mode of the twist and of `mt_rand()` scaling.
    pub fn mode(&self) -> MtMode {
        self.mode
    }

    /// Sets the mode without reseeding.
    pub fn set_mode(&mut self, mode: MtMode) {
        self.mode = mode;
    }

    /// Port of `php_random_mt19937_seed32()`: Knuth's initialisation followed by a reload.
    pub fn seed(&mut self, seed: u32) {
        self.state[0] = seed;
        for i in 1..N {
            let prev = self.state[i - 1];
            self.state[i] = 1_812_433_253u32
                .wrapping_mul(prev ^ (prev >> 30))
                .wrapping_add(i as u32);
        }
        self.reload();
    }

    /// Port of `mt19937_reload()`.
    fn reload(&mut self) {
        let t = if self.mode == MtMode::Mt19937 {
            twist
        } else {
            twist_php
        };
        let s = &mut self.state;
        let mut p = 0;
        for _ in 0..N - M {
            s[p] = t(s[p + M], s[p], s[p + 1]);
            p += 1;
        }
        for _ in 0..M - 1 {
            s[p] = t(s[p + M - N], s[p], s[p + 1]);
            p += 1;
        }
        s[p] = t(s[p + M - N], s[p], s[0]);
        self.count = 0;
    }

    /// Port of the engine's `generate()`: the next tempered 32-bit output.
    pub fn next_u32(&mut self) -> u32 {
        if self.count >= N {
            self.reload();
        }
        let mut s1 = self.state[self.count];
        self.count += 1;
        s1 ^= s1 >> 11;
        s1 ^= (s1 << 7) & 0x9d2c_5680;
        s1 ^= (s1 << 15) & 0xefc6_0000;
        s1 ^ (s1 >> 18)
    }

    /// Port of `php_random_range32()`: uniform in `0..=umax` by rejection sampling.
    fn range32(&mut self, umax: u32) -> PhpResult<u32> {
        let mut result = self.next_u32();
        // Special case where no modulus is required
        if umax == u32::MAX {
            return Ok(result);
        }
        // Increment the max so range is inclusive of max
        let umax = umax + 1;
        // Powers of two are not biased
        if umax & (umax - 1) == 0 {
            return Ok(result & (umax - 1));
        }
        // Ceiling under which UINT32_MAX % max == 0
        let limit = u32::MAX - (u32::MAX % umax) - 1;
        // Discard numbers over the limit to avoid modulo bias
        let mut count = 0;
        while result > limit {
            count += 1;
            if count > RANGE_ATTEMPTS {
                return Err(broken_engine());
            }
            result = self.next_u32();
        }
        Ok(result % umax)
    }

    /// Port of `php_random_range64()`: like [`range32`](Self::range32) on two outputs (the first in
    /// the low half).
    fn range64(&mut self, umax: u64) -> PhpResult<u64> {
        let next64 = |mt: &mut Mt19937| -> u64 {
            let lo = u64::from(mt.next_u32());
            lo | (u64::from(mt.next_u32()) << 32)
        };
        let mut result = next64(self);
        if umax == u64::MAX {
            return Ok(result);
        }
        let umax = umax + 1;
        if umax & (umax - 1) == 0 {
            return Ok(result & (umax - 1));
        }
        let limit = u64::MAX - (u64::MAX % umax) - 1;
        let mut count = 0;
        while result > limit {
            count += 1;
            if count > RANGE_ATTEMPTS {
                return Err(broken_engine());
            }
            result = next64(self);
        }
        Ok(result % umax)
    }

    /// Port of `php_random_range()`: a uniform integer in `min..=max` (`min <= max`), using one
    /// 32-bit output when the span fits in 32 bits and two otherwise. The
    /// `Random\BrokenRandomEngineError` of upstream (50 rejected draws in a row) is practically
    /// unreachable for MT19937 but reproduced.
    pub fn range(&mut self, min: i64, max: i64) -> PhpResult<i64> {
        let umax = (max as u64).wrapping_sub(min as u64);
        if umax > u64::from(u32::MAX) {
            return Ok(self.range64(umax)?.wrapping_add(min as u64) as i64);
        }
        Ok(u64::from(self.range32(umax as u32)?).wrapping_add(min as u64) as i64)
    }

    /// Port of `php_mt_rand_common()`: [`range`](Self::range) in `MT_RAND_MT19937` mode; in
    /// `MT_RAND_PHP` mode the historical scaling `min + (max - min + 1) * (r / 2^31)` of a 31-bit
    /// output.
    fn rand_common(&mut self, min: i64, max: i64) -> PhpResult<i64> {
        if self.mode == MtMode::Mt19937 {
            return self.range(min, max);
        }
        let r = u64::from(self.next_u32() >> 1);
        // RAND_RANGE_BADSCALING without UB for (max - min) > ZEND_LONG_MAX
        let offset =
            ((max as f64 - min as f64 + 1.0) * (r as f64 / (MT_RAND_MAX as f64 + 1.0))) as u64;
        Ok(offset.wrapping_add(min as u64) as i64)
    }
}

/// Port of `mt_rand()` without arguments: the next output shifted right by one
/// (`0..=`[`MT_RAND_MAX`]).
pub fn mt_rand(rng: &mut Mt19937) -> i64 {
    i64::from(rng.next_u32() >> 1)
}

/// Port of `mt_rand(int $min, int $max)`: weak-mode int parameters (`mt_rand(): Argument #1 ($min)
/// must be of type int, X given`), `ValueError: mt_rand(): Argument #2 ($max) must be greater than
/// or equal to argument #1 ($min)`, then a uniform integer in `min..=max`.
pub fn mt_rand_range(
    rng: &mut Mt19937,
    min: &Value,
    max: &Value,
    diag: &mut dyn Diagnostics,
) -> PhpResult<i64> {
    let min = param::parse_long(min, &Arg::new("mt_rand", 1, "min"), diag)?;
    let max_arg = Arg::new("mt_rand", 2, "max");
    let max = param::parse_long(max, &max_arg, diag)?;
    if max < min {
        return Err(max_arg.value_error("must be greater than or equal to argument #1 ($min)"));
    }
    rng.rand_common(min, max)
}

/// `mt_rand(...$args)` with an argument list: none ([`mt_rand`]) or two ([`mt_rand_range`]);
/// otherwise `ArgumentCountError: mt_rand() expects exactly 2 arguments, 1 given`.
pub fn mt_rand_call(
    rng: &mut Mt19937,
    args: &[Value],
    diag: &mut dyn Diagnostics,
) -> PhpResult<i64> {
    match args {
        [] => Ok(mt_rand(rng)),
        [min, max] => mt_rand_range(rng, min, max, diag),
        _ => Err(PhpError::argument_count_error(format!(
            "mt_rand() expects exactly 2 arguments, {} given",
            args.len()
        ))),
    }
}

/// Port of `mt_srand(?int $seed = null, int $mode = MT_RAND_MT19937)`: [`MT_RAND_PHP`] selects the
/// legacy mode with `Deprecated: The MT_RAND_PHP variant of Mt19937 is deprecated`, any other mode
/// value the standard one; then the generator is reseeded with the low 32 bits of `seed`, or from
/// entropy when `seed` is `None`.
pub fn mt_srand(
    rng: &mut Mt19937,
    seed: Option<i64>,
    mode: i64,
    diag: &mut dyn Diagnostics,
) -> PhpResult<()> {
    if mode == MT_RAND_PHP {
        rng.mode = MtMode::Php;
        diag.deprecated("The MT_RAND_PHP variant of Mt19937 is deprecated")?;
    } else {
        rng.mode = MtMode::Mt19937;
    }
    let seed = match seed {
        Some(s) => s as u32,
        None => entropy_seed(),
    };
    rng.seed(seed);
    Ok(())
}

/// Port of `php_array_data_shuffle()` (PHPSRC/ext/standard/array.c), i.e. `shuffle($array)`: the
/// array becomes a list of its values (keys dropped) shuffled with Fisher-Yates from the end,
/// drawing `range(0, n_left)` for each position.
pub fn shuffle(rng: &mut Mt19937, array: &mut Array) -> PhpResult<()> {
    let n_elems = array.len();
    if n_elems < 1 {
        return Ok(());
    }
    let mut values: Vec<Value> = array.values().cloned().collect();
    let mut n_left = n_elems;
    loop {
        n_left -= 1;
        if n_left == 0 {
            break;
        }
        let rnd_idx = rng.range(0, n_left as i64)? as usize;
        if rnd_idx != n_left {
            values.swap(n_left, rnd_idx);
        }
    }
    *array = Array::from_values(values);
    Ok(())
}

/// Port of `array_rand(array $array, int $num = 1)`: `Z_PARAM_ARRAY` / `Z_PARAM_LONG` coercion
/// (`array_rand(): Argument #2 ($num) must be of type int, string given`, ...), then
/// [`array_pick_keys`]. Twig's `random()` calls `array_rand($values, 1)`.
pub fn array_rand(
    rng: &mut Mt19937,
    array: &Value,
    num: Option<&Value>,
    diag: &mut dyn Diagnostics,
) -> PhpResult<Value> {
    let array = param::parse_array(array, &Arg::new("array_rand", 1, "array"))?;
    let num = match num {
        Some(n) => param::parse_long(n, &Arg::new("array_rand", 2, "num"), diag)?,
        None => 1,
    };
    array_pick_keys(rng, &array, num)
}

/// Port of `php_array_pick_keys()` (PHPSRC/ext/standard/array.c): one random key for `num == 1`,
/// else a list of `num` distinct keys in array order (a bitset of drawn positions, inverted when
/// more than half the keys are requested).
///
/// Errors: an empty array → `ValueError: array_rand(): Argument #1 ($array) must not be empty`;
/// `num` outside `1..=count` → `ValueError: array_rand(): Argument #2 ($num) must be between 1 and
/// the number of elements in argument #1 ($array)`.
///
/// Known deviation: for `num == 1` PHP draws a slot of the hash table including holes left by
/// `unset()` and retries on a hole; this model has no holes, so for arrays with removed elements
/// the drawn key can differ from PHP's (arrays built without removals, which is everything a
/// template creates, match exactly).
pub fn array_pick_keys(rng: &mut Mt19937, array: &Array, num: i64) -> PhpResult<Value> {
    let num_avail = array.len();
    if num_avail == 0 {
        return Err(Arg::new("array_rand", 1, "array").value_error("must not be empty"));
    }
    if num == 1 {
        // Without holes nNumUsed == num_avail, so the first sampled slot is always used.
        let randval = rng.range(0, num_avail as i64 - 1)? as usize;
        let (k, _) = array
            .get_index(randval)
            .expect("random position within the array");
        return Ok(k.to_value());
    }
    if num <= 0 || num as u64 > num_avail as u64 {
        return Err(Arg::new("array_rand", 2, "num")
            .value_error("must be between 1 and the number of elements in argument #1 ($array)"));
    }
    let mut num_req = num as usize;
    let mut negative = false;
    if num_req > num_avail >> 1 {
        negative = true;
        num_req = num_avail - num_req;
    }
    let mut bitset = vec![false; num_avail];
    let mut i = num_req;
    let mut failures = 0;
    while i > 0 {
        let randval = rng.range(0, num_avail as i64 - 1)? as usize;
        if bitset[randval] {
            failures += 1;
            if failures > RANGE_ATTEMPTS {
                return Err(broken_engine());
            }
        } else {
            bitset[randval] = true;
            i -= 1;
            failures = 0;
        }
    }
    Ok(Value::Array(Array::from_values(
        array
            .keys()
            .enumerate()
            .filter(|(pos, _)| bitset[*pos] ^ negative)
            .map(|(_, k)| k.to_value()),
    )))
}

/// Port of `random_int(int $min, int $max)` / `php_random_int()` (PHPSRC/ext/random/csprng.c):
/// weak-mode int parameters, `ValueError: random_int(): Argument #1 ($min) must be less than or
/// equal to argument #2 ($max)`, then rejection sampling on 64-bit draws from `next_u64` (PHP reads
/// them from the operating system's CSPRNG, which cannot be seeded; the caller supplies the
/// source). `min == max` consumes no randomness.
pub fn random_int(
    min: &Value,
    max: &Value,
    next_u64: &mut dyn FnMut() -> u64,
    diag: &mut dyn Diagnostics,
) -> PhpResult<i64> {
    let min_arg = Arg::new("random_int", 1, "min");
    let min = param::parse_long(min, &min_arg, diag)?;
    let max = param::parse_long(max, &Arg::new("random_int", 2, "max"), diag)?;
    if min > max {
        return Err(min_arg.value_error("must be less than or equal to argument #2 ($max)"));
    }
    if min == max {
        return Ok(min);
    }
    let umax = (max as u64).wrapping_sub(min as u64);
    let mut trial = next_u64();
    // Special case where no modulus is required
    if umax == u64::MAX {
        return Ok(trial as i64);
    }
    // Increment the max so the range is inclusive of max
    let umax = umax + 1;
    // Powers of two are not biased
    if umax & (umax - 1) != 0 {
        // Ceiling under which ZEND_LONG_MAX % max == 0
        let limit = u64::MAX - (u64::MAX % umax) - 1;
        // Discard numbers over the limit to avoid modulo bias
        while trial > limit {
            trial = next_u64();
        }
    }
    Ok((trial % umax).wrapping_add(min as u64) as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_sequence() {
        // mt_srand(1); mt_rand() x3 in PHP 8.5.13 (also the published MT19937 outputs >> 1 for
        // seed 1: 1791095845, 4282876139, 3093770124).
        let mut mt = Mt19937::new(1);
        assert_eq!(mt.next_u32(), 1_791_095_845);
        assert_eq!(mt.next_u32(), 4_282_876_139);
        assert_eq!(mt.next_u32(), 3_093_770_124);
        let mut mt = Mt19937::new(1);
        assert_eq!(mt_rand(&mut mt), 1_791_095_845 >> 1);
    }

    #[test]
    fn ranges() {
        let mut mt = Mt19937::new(7);
        for _ in 0..1000 {
            let v = mt.range(-3, 3).unwrap();
            assert!((-3..=3).contains(&v));
        }
        assert!(mt.range(i64::MIN, i64::MAX).is_ok());
        let mut d = Vec::new();
        let e = mt_rand_range(&mut mt, &Value::Int(5), &Value::Int(1), &mut d).unwrap_err();
        assert_eq!(
            e.message(),
            "mt_rand(): Argument #2 ($max) must be greater than or equal to argument #1 ($min)"
        );
        let mut a: Array = (0..10).map(Value::from).collect();
        shuffle(&mut mt, &mut a).unwrap();
        assert!(a.is_list());
        assert_eq!(a.len(), 10);
    }

    #[test]
    fn random_int_sampling() {
        // php_random_int(): trial % (max - min + 1) + min, rejecting trials above the limit.
        let mut d = Vec::new();
        let mut draws = [u64::MAX, 5, 0].into_iter();
        let mut next = || draws.next().unwrap();
        // umax + 1 = 3: limit = u64::MAX - (u64::MAX % 3) - 1 rejects u64::MAX, then 5 % 3 = 2.
        let r = random_int(&Value::Int(10), &Value::Int(12), &mut next, &mut d).unwrap();
        assert_eq!(r, 12);
        // powers of two are not rejected
        let mut next = || u64::MAX;
        let r = random_int(&Value::Int(0), &Value::Int(7), &mut next, &mut d).unwrap();
        assert_eq!(r, 7);
        // the full range takes the draw as it is
        let mut next = || 0x8000_0000_0000_0001;
        let r = random_int(
            &Value::Int(i64::MIN),
            &Value::Int(i64::MAX),
            &mut next,
            &mut d,
        );
        assert_eq!(r.unwrap(), i64::MIN + 1);
        assert!(d.is_empty());
    }
}
