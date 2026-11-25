//! Correctly rounded fused multiply-add.
//!
//! Port of musl's `src/math/fma.c` (MIT, Copyright © 2005-2020 Rich Felker et al.; the algorithm
//! by Szabolcs Nagy), used when the target has no hardware FMA. The C code splits the 106-bit
//! mantissa product into two 64-bit halves with 32-bit multiplies; `u128` computes the same exact
//! product.

/// Exponent threshold of [`normalize`] for zero, infinity and NaN (`ZEROINFNAN`).
const ZEROINFNAN: i32 = 0x7ff - 0x3ff - 52 - 1;

/// `0x1p63`.
const P63: f64 = f64::from_bits(0x43e0_0000_0000_0000);

/// A finite double as `m * 2^e` with the top ten bits and the last bit of `m` clear.
struct Num {
    m: u64,
    e: i32,
    negative: bool,
}

/// `normalize()`: subnormals are scaled by 2^63 first; zero gets `e = 0x800 - ...` (above
/// [`ZEROINFNAN`]), infinities and NaNs `e = 0x7ff - ...` (equal to it).
fn normalize(x: f64) -> Num {
    let mut ix = x.to_bits();
    let mut e = (ix >> 52) as i32;
    let negative = e & 0x800 != 0;
    e &= 0x7ff;
    if e == 0 {
        ix = (x * P63).to_bits();
        e = ((ix >> 52) & 0x7ff) as i32;
        e = if e != 0 { e - 63 } else { 0x800 };
    }
    ix &= (1u64 << 52) - 1;
    ix |= 1u64 << 52;
    ix <<= 1;
    e -= 0x3ff + 52 + 1;
    Num { m: ix, e, negative }
}

/// `mul()`: the exact 128-bit product as `(hi, lo)`.
fn mul(x: u64, y: u64) -> (u64, u64) {
    let p = u128::from(x) * u128::from(y);
    ((p >> 64) as u64, p as u64)
}

/// Port of musl's `scalbn()` (`src/math/scalbn.c`): `x * 2^n` with a single rounding.
pub(crate) fn scalbn(x: f64, mut n: i32) -> f64 {
    // 0x1p1023 and 0x1p-1022 * 0x1p53
    const P1023: f64 = f64::from_bits(0x7fe0_0000_0000_0000);
    const PM969: f64 = f64::from_bits(0x0360_0000_0000_0000);
    let mut y = x;
    if n > 1023 {
        y *= P1023;
        n -= 1023;
        if n > 1023 {
            y *= P1023;
            n -= 1023;
            if n > 1023 {
                n = 1023;
            }
        }
    } else if n < -1022 {
        // make sure final n < -53 to avoid double rounding in the subnormal range
        y *= PM969;
        n += 1022 - 53;
        if n < -1022 {
            y *= PM969;
            n += 1022 - 53;
            if n < -1022 {
                n = -1022;
            }
        }
    }
    y * f64::from_bits(((0x3ff + n) as u64) << 52)
}

/// Port of musl's `fma()`: `x * y + z` with a single rounding (round to nearest, ties to even),
/// computed in software on every target. [`fma`](super::fma) uses it when the target has no FMA
/// instruction.
pub fn fma_soft(x: f64, y: f64, z: f64) -> f64 {
    // normalize so top 10bits and last bit are 0
    let nx = normalize(x);
    let ny = normalize(y);
    let nz = normalize(z);

    if nx.e >= ZEROINFNAN || ny.e >= ZEROINFNAN {
        return x * y + z;
    }
    if nz.e >= ZEROINFNAN {
        if nz.e > ZEROINFNAN {
            // z == 0
            return x * y;
        }
        return z;
    }

    // mul: r = x*y
    let (mut rhi, mut rlo) = mul(nx.m, ny.m);
    // either top 20 or 21 bits of rhi and last 2 bits of rlo are 0

    // align exponents
    let mut e = nx.e + ny.e;
    let mut d = nz.e - e;
    let zhi: u64;
    let zlo: u64;
    // shift bits z<<=kz, r>>=kr, so kz+kr == d, set e = e+kr (== ez-kz)
    if d > 0 {
        if d < 64 {
            zlo = nz.m << d;
            zhi = nz.m >> (64 - d);
        } else {
            zlo = 0;
            zhi = nz.m;
            e = nz.e - 64;
            d -= 64;
            if d == 0 {
            } else if d < 64 {
                rlo = (rhi << (64 - d)) | (rlo >> d) | u64::from(rlo << (64 - d) != 0);
                rhi >>= d;
            } else {
                rlo = 1;
                rhi = 0;
            }
        }
    } else {
        zhi = 0;
        d = -d;
        if d == 0 {
            zlo = nz.m;
        } else if d < 64 {
            zlo = (nz.m >> d) | u64::from(nz.m << (64 - d) != 0);
        } else {
            zlo = 1;
        }
    }

    // add
    let mut negative = nx.negative != ny.negative;
    let samesign = negative == nz.negative;
    let mut nonzero = true;
    if samesign {
        // r += z
        rlo = rlo.wrapping_add(zlo);
        rhi = rhi.wrapping_add(zhi).wrapping_add(u64::from(rlo < zlo));
    } else {
        // r -= z
        let t = rlo;
        rlo = rlo.wrapping_sub(zlo);
        rhi = rhi.wrapping_sub(zhi).wrapping_sub(u64::from(t < rlo));
        if rhi >> 63 != 0 {
            rlo = rlo.wrapping_neg();
            rhi = rhi.wrapping_neg().wrapping_sub(u64::from(rlo != 0));
            negative = !negative;
        }
        nonzero = rhi != 0;
    }

    // set rhi to top 63bit of the result (last bit is sticky)
    if nonzero {
        e += 64;
        d = rhi.leading_zeros() as i32 - 1;
        // note: d > 0
        rhi = (rhi << d) | (rlo >> (64 - d)) | u64::from(rlo << d != 0);
    } else if rlo != 0 {
        d = rlo.leading_zeros() as i32 - 1;
        if d < 0 {
            rhi = (rlo >> 1) | (rlo & 1);
        } else {
            rhi = rlo << d;
        }
    } else {
        // exact +-0
        return x * y + z;
    }
    e -= d;

    // convert to double
    let mut i = rhi as i64; // i is in [1<<62,(1<<63)-1]
    if negative {
        i = -i;
    }
    let mut r = i as f64; // |r| is in [0x1p62,0x1p63]

    if e < -1022 - 62 {
        // result is subnormal before rounding
        if e == -1022 - 63 {
            let c = if negative { -P63 } else { P63 };
            if r == c {
                // min normal after rounding (musl computes it through a float conversion to raise
                // the underflow flag; the value is DBL_MIN with the sign of the result)
                return if negative {
                    -f64::MIN_POSITIVE
                } else {
                    f64::MIN_POSITIVE
                };
            }
            // one bit is lost when scaled, add another top bit to only round once at conversion
            // if it is inexact
            if rhi << 53 != 0 {
                i = ((rhi >> 1) | (rhi & 1) | (1u64 << 62)) as i64;
                if negative {
                    i = -i;
                }
                r = i as f64;
                r = 2.0 * r - c; // remove top bit
            }
        } else {
            // only round once when scaled
            d = 10;
            i = (((rhi >> d) | u64::from(rhi << (64 - d) != 0)) << d) as i64;
            if negative {
                i = -i;
            }
            r = i as f64;
        }
    }
    scalbn(r, e)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A small deterministic generator (SplitMix64).
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
            z ^ (z >> 31)
        }

        /// Doubles biased towards hard cases: near-cancellation, subnormals, large exponents.
        fn double(&mut self) -> f64 {
            let bits = self.next();
            match bits % 8 {
                0 => f64::from_bits(bits >> 12), // subnormal or tiny
                1 => f64::from_bits((bits & 0x800f_ffff_ffff_ffff) | (0x3ff << 52)), // [1, 2)
                2 => f64::from_bits((bits & 0x800f_ffff_ffff_ffff) | (0x3fe << 52)), // [0.5, 1)
                _ => {
                    let f = f64::from_bits(bits);
                    if f.is_nan() { 1.5 } else { f }
                }
            }
        }
    }

    fn same(a: f64, b: f64) -> bool {
        a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan())
    }

    #[test]
    fn matches_hardware_or_platform_fma() {
        // `f64::mul_add` is documented to round once; compare on structured random inputs,
        // including z = -(x*y) rounded, which exercises exact cancellation.
        let mut rng = Rng(0x5eed);
        for _ in 0..400_000 {
            let x = rng.double();
            let y = rng.double();
            let z = match rng.next() % 4 {
                0 => -(x * y),
                1 => rng.double() * f64::EPSILON,
                _ => rng.double(),
            };
            let want = x.mul_add(y, z);
            let got = fma_soft(x, y, z);
            assert!(
                same(got, want),
                "fma({x:e}, {y:e}, {z:e}) = {got:e}, want {want:e}"
            );
        }
    }

    #[test]
    fn edge_cases() {
        let cases: &[(f64, f64, f64)] = &[
            (0.0, 0.0, -0.0),
            (-0.0, 0.0, 0.0),
            (1.0, 1.0, -1.0),
            (f64::MIN_POSITIVE, 0.5, 0.0),
            (f64::MIN_POSITIVE, -0.5, 0.0),
            (f64::from_bits(1), 0.5, 0.0),
            (f64::from_bits(1), -0.5, -0.0),
            (f64::MAX, 2.0, -f64::MAX),
            (f64::MAX, 1.0, f64::MAX),
            (f64::INFINITY, 0.0, 1.0),
            (f64::NAN, 1.0, 1.0),
            (1.0 + f64::EPSILON, 1.0 - f64::EPSILON, -1.0),
            (0.1, 10.0, -1.0),
            (
                f64::from_bits(0x0010_0000_0000_0001),
                0.5,
                f64::from_bits(1),
            ),
            (
                f64::from_bits(0x001f_ffff_ffff_ffff),
                0.5,
                f64::from_bits(0x8000_0000_0000_0001),
            ),
        ];
        for &(x, y, z) in cases {
            assert!(
                same(fma_soft(x, y, z), x.mul_add(y, z)),
                "fma({x:e}, {y:e}, {z:e})"
            );
        }
        assert_eq!(scalbn(1.0, -1074), f64::from_bits(1));
        assert_eq!(scalbn(1.0, 2000), f64::INFINITY);
        assert_eq!(scalbn(3.0, -1075), f64::from_bits(2));
    }
}
