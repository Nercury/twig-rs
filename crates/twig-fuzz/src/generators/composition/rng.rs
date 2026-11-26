//! A small seeded random number generator (SplitMix64): the same seed gives the same case on every
//! platform, without a dependency.

/// SplitMix64 (Steele, Lea and Flood): one 64-bit state, good enough for test generation.
#[derive(Clone, Debug)]
pub struct Rng {
    state: u64,
}

impl Rng {
    /// A generator seeded with `seed`.
    pub fn new(seed: u64) -> Rng {
        let mut rng = Rng { state: seed };
        // decorrelate small consecutive seeds
        rng.next_u64();
        rng
    }

    /// The next 64 random bits.
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number in `0..n` (0 when `n` is 0).
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next_u64() % n as u64) as usize
        }
    }

    /// A number in `low..=high`.
    pub fn range(&mut self, low: usize, high: usize) -> usize {
        low + self.below(high.saturating_sub(low) + 1)
    }

    /// True with a probability of `percent` / 100.
    pub fn chance(&mut self, percent: u32) -> bool {
        self.below(100) < percent as usize
    }

    /// One of `items` (which must not be empty).
    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }

    /// One of `items` (which must not be empty), for string literals: unlike [`Rng::pick`] the
    /// result needs no dereference wherever a `&str` is expected.
    pub fn pick_str(&mut self, items: &[&'static str]) -> &'static str {
        items[self.below(items.len())]
    }

    /// One of `items` (which must not be empty), for byte-string literals; like [`Rng::pick_str`] it returns the
    /// element itself, which keeps inference identical on every supported compiler.
    pub fn pick_bytes(&mut self, items: &[&'static [u8]]) -> &'static [u8] {
        items[self.below(items.len())]
    }

    /// One of the values, chosen with the given weights (the last one when all weights are 0).
    pub fn weighted<T: Copy>(&mut self, items: &[(u32, T)]) -> T {
        let total: u64 = items.iter().map(|(w, _)| u64::from(*w)).sum();
        let mut roll = if total == 0 {
            0
        } else {
            self.next_u64() % total
        };
        for (weight, value) in items {
            let weight = u64::from(*weight);
            if roll < weight {
                return *value;
            }
            roll -= weight;
        }
        items.last().expect("weighted() needs at least one item").1
    }

    /// Shuffles `items` in place (Fisher-Yates).
    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = self.below(i + 1);
            items.swap(i, j);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Rng;

    #[test]
    fn deterministic_and_bounded() {
        let mut a = Rng::new(7);
        let mut b = Rng::new(7);
        for _ in 0..100 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
        let mut rng = Rng::new(1);
        for _ in 0..1000 {
            assert!(rng.below(3) < 3);
            let r = rng.range(2, 4);
            assert!((2..=4).contains(&r));
        }
        assert_eq!(rng.weighted(&[(0, 'a'), (5, 'b')]), 'b');
    }
}
