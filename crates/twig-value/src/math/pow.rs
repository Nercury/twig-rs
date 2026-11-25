//! Double-precision `pow(x, y)`, bit-identical to the oracle's C library.
//!
//! Algorithm: ARM optimized-routines `pow` (Copyright (c) 2018, Arm Limited, SPDX-License-Identifier:
//! MIT), the code in musl's `src/math/pow.c` and glibc's `sysdeps/ieee754/dbl-64/e_pow.c`. Worst-case
//! error 0.54 ULP, so it is not correctly rounded and the exact evaluation matters.
//!
//! PHP calls the C library `pow()`. On x86-64 CPUs with FMA and AVX2 (every machine the oracle and CI
//! run on) glibc 2.41 selects `__ieee754_pow_fma`: `e_pow.c` compiled with `-mfma -mavx2` and GCC's
//! default `-ffp-contract=fast`. That build uses the `__FP_FAST_FMA` branches of the source *and*
//! contracts further `a * b + c` expressions into fused multiply-adds. This port reproduces exactly
//! the fused operations of that binary (read from the disassembly of `libm.so.6` in
//! `twig-oracle:3.31.1`); each contracted expression is marked "contracted" below. Validated against
//! PHP `**` by `tests/vectors/pow.json`.

use super::fma;
use super::pow_data::{
    EXP_POLY, EXP_TAB, INV_LN2_N, LN2HI, LN2LO, LOG_TAB, NEG_LN2_HI_N, NEG_LN2_LO_N, POLY, SHIFT,
};

/// `POW_LOG_TABLE_BITS`.
const POW_LOG_TABLE_BITS: u32 = 7;
/// `EXP_TABLE_BITS`.
const EXP_TABLE_BITS: u32 = 7;
/// `OFF`: the log argument reduction maps x to z in `[OFF, 2 * OFF)`.
const OFF: u64 = 0x3fe6_9555_0000_0000;
/// `SIGN_BIAS`: added to the exponent bits of the result to make it negative.
const SIGN_BIAS: u64 = 0x800 << EXP_TABLE_BITS;
/// Bits of `1.0`.
const ONE_BITS: u64 = 0x3ff0_0000_0000_0000;
/// Bits of `INFINITY`.
const INF_BITS: u64 = 0x7ff0_0000_0000_0000;

/// Top 12 bits of a double (sign and exponent bits).
#[inline]
fn top12(x: f64) -> u32 {
    (x.to_bits() >> 52) as u32
}

/// `log_inline()`: returns `(y, tail)` with `y + tail = log(x)`, where `y` is the rounded result and
/// `tail` has about 15 bits of additional precision. `ix` is the bit representation of x,
/// normalized in the subnormal range using the sign bit for the exponent.
#[inline]
fn log_inline(ix: u64) -> (f64, f64) {
    // x = 2^k z; where z is in range [OFF,2*OFF) and exact.
    // The range is split into N subintervals.
    // The ith subinterval contains z and c is near its center.
    let tmp = ix.wrapping_sub(OFF);
    let i = ((tmp >> (52 - POW_LOG_TABLE_BITS)) % (1 << POW_LOG_TABLE_BITS)) as usize;
    let k = ((tmp as i64) >> 52) as i32; // arithmetic shift
    let iz = ix.wrapping_sub(tmp & (0xfff << 52));
    let z = f64::from_bits(iz);
    let kd = f64::from(k);

    // log(x) = k*Ln2 + log(c) + log1p(z/c-1).
    let [invc, logc, logctail] = LOG_TAB[i];

    // Note: 1/c is j/N or j/N/2 where j is an integer in [N,2N) and |z/c - 1| < 1/N, so
    // r = z/c - 1 is exactly representible.
    let r = fma(z, invc, -1.0);

    // k*Ln2 + log(c) + r.
    let t1 = fma(kd, LN2HI, logc); // contracted: kd * Ln2hi + logc
    let t2 = t1 + r;
    let lo1 = fma(kd, LN2LO, logctail); // contracted: kd * Ln2lo + logctail
    let lo2 = t1 - t2 + r;

    // Evaluation is optimized assuming superscalar pipelined execution.
    let ar = POLY[0] * r; // A[0] = -0.5.
    let ar2 = r * ar;
    let ar3 = r * ar2;
    // k*Ln2 + log(c) + r + A[0]*r*r.
    let hi = t2 + ar2;
    let lo3 = fma(ar, r, -ar2);
    let lo4 = t2 - hi + ar2;
    // p = log1p(r) - r - A[0]*r*r.
    // p = ar3 * (A[1] + r * A[2] + ar2 * (A[3] + r * A[4] + ar2 * (A[5] + r * A[6])));
    // contracted: every `+ x * y` above is fused, and `lo = ... + p` fuses the outer product.
    let q = fma(
        ar2,
        fma(ar2, fma(r, POLY[6], POLY[5]), fma(r, POLY[4], POLY[3])),
        fma(r, POLY[2], POLY[1]),
    );
    let lo = fma(ar3, q, lo1 + lo2 + lo3 + lo4);
    let y = hi + lo;
    let tail = hi - y + lo;
    (y, tail)
}

/// `__math_oflow(sign)`: `(sign ? -0x1p769 : 0x1p769) * 0x1p769`.
fn oflow(sign: u64) -> f64 {
    let x = f64::from_bits(0x7000_0000_0000_0000); // 0x1p769
    (if sign != 0 { -x } else { x }) * x
}

/// `__math_uflow(sign)`: `(sign ? -0x1p-767 : 0x1p-767) * 0x1p-767`.
fn uflow(sign: u64) -> f64 {
    let x = f64::from_bits(0x1000_0000_0000_0000); // 0x1p-767
    (if sign != 0 { -x } else { x }) * x
}

/// `__math_invalid(x)`: `(x - x) / (x - x)`, i.e. the default NaN of the FPU (x86-64: negative
/// quiet NaN) for finite `x`.
#[allow(clippy::eq_op)]
fn invalid(x: f64) -> f64 {
    let d = x - x;
    if d == 0.0 {
        // 0/0 on x86-64 yields the "default NaN" with the sign bit set.
        f64::from_bits(0xfff8_0000_0000_0000)
    } else {
        d / d
    }
}

/// `specialcase()`: handles results that may overflow or underflow, computing `scale * (1 + tmp)`
/// without intermediate rounding. `sbits` is the bit pattern of scale with a computed exponent
/// that may have overflown into the sign bit; `(int32_t) ki` is the k of the argument reduction
/// (positive: may overflow, negative: may underflow).
// The C operand order is kept (`lo = one - hi + y + lo`), hence no `+=`.
#[allow(clippy::assign_op_pattern)]
fn specialcase(tmp: f64, mut sbits: u64, ki: u64) -> f64 {
    if ki & 0x8000_0000 == 0 {
        // k > 0, the exponent of scale might have overflowed by <= 460.
        sbits = sbits.wrapping_sub(1009u64 << 52);
        let scale = f64::from_bits(sbits);
        let p1009 = f64::from_bits(0x7f00_0000_0000_0000); // 0x1p1009
        return p1009 * fma(scale, tmp, scale); // contracted: scale + scale * tmp
    }
    // k < 0, need special care in the subnormal range.
    sbits = sbits.wrapping_add(1022u64 << 52);
    // Note: sbits is signed scale.
    let scale = f64::from_bits(sbits);
    // `scale * tmp` is computed once (it is reused below, in another basic block), so this sum is
    // NOT contracted in the oracle's binary.
    let scale_tmp = scale * tmp;
    let mut y = scale + scale_tmp;
    if y.abs() < 1.0 {
        // Round y to the right precision before scaling it into the subnormal range to avoid
        // double rounding that can cause 0.5+E/2 ulp error where E is the worst-case ulp error
        // outside the subnormal range.
        let one = if y < 0.0 { -1.0 } else { 1.0 };
        let mut lo = scale - y + scale_tmp;
        let hi = one + y;
        lo = one - hi + y + lo;
        y = (hi + lo) - one;
        // Fix the sign of 0.
        if y == 0.0 {
            y = f64::from_bits(sbits & 0x8000_0000_0000_0000);
        }
    }
    f64::from_bits(0x0010_0000_0000_0000) * y // 0x1p-1022 * y
}

/// `exp_inline()`: `sign * exp(x + xtail)` where `|xtail| < 2^-8/N` and `|xtail| <= |x|`.
/// `sign_bias` is [`SIGN_BIAS`] or 0 and sets the sign to -1 or 1.
#[inline]
fn exp_inline(x: f64, xtail: f64, sign_bias: u64) -> f64 {
    const N: u64 = 1 << EXP_TABLE_BITS;
    let mut abstop = top12(x) & 0x7ff;
    let tiny = top12(f64::from_bits(0x3c90_0000_0000_0000)); // top12(0x1p-54)
    if abstop.wrapping_sub(tiny) >= top12(512.0).wrapping_sub(tiny) {
        if abstop.wrapping_sub(tiny) >= 0x8000_0000 {
            // Avoid spurious underflow for tiny x.
            // Note: 0 is common input.
            let one = 1.0 + x;
            return if sign_bias != 0 { -one } else { one };
        }
        if abstop >= top12(1024.0) {
            // Note: inf and nan are already handled.
            return if x.to_bits() >> 63 != 0 {
                uflow(sign_bias)
            } else {
                oflow(sign_bias)
            };
        }
        // Large x is special cased below.
        abstop = 0;
    }

    // exp(x) = 2^(k/N) * exp(r), with exp(r) in [2^(-1/2N),2^(1/2N)].
    // x = ln2/N*k + r, with int k and r in [-ln2/2N, ln2/2N].
    // z = InvLn2N * x; kd = z + Shift;  (contracted into one fused operation)
    let kd = fma(INV_LN2_N, x, SHIFT);
    let ki = kd.to_bits();
    let kd = kd - SHIFT;
    // r = x + kd * NegLn2hiN + kd * NegLn2loN;  (contracted)
    let mut r = fma(kd, NEG_LN2_LO_N, fma(kd, NEG_LN2_HI_N, x));
    // The code assumes 2^-200 < |xtail| < 2^-8/N.
    r += xtail;
    // 2^(k/N) ~= scale * (1 + tail).
    let idx = (2 * (ki % N)) as usize;
    let top = ki.wrapping_add(sign_bias) << (52 - EXP_TABLE_BITS);
    let tail = f64::from_bits(EXP_TAB[idx]);
    // This is only a valid scale when -1023*N < k < 1024*N.
    let sbits = EXP_TAB[idx + 1].wrapping_add(top);
    // exp(x) = 2^(k/N) * exp(r) ~= scale + scale * (tail + exp(r) - 1).
    // Evaluation is optimized assuming superscalar pipelined execution.
    let r2 = r * r;
    // tmp = tail + r + r2 * (C2 + r * C3) + r2 * r2 * (C4 + r * C5);  (contracted)
    let [c2, c3, c4, c5] = EXP_POLY;
    let tmp = fma(r2 * r2, fma(r, c5, c4), fma(r2, fma(r, c3, c2), tail + r));
    if abstop == 0 {
        return specialcase(tmp, sbits, ki);
    }
    let scale = f64::from_bits(sbits);
    // Note: tmp == 0 or |tmp| > 2^-200 and scale > 2^-739, so there is no spurious underflow
    // here even without fma.
    fma(scale, tmp, scale) // contracted: scale + scale * tmp
}

/// `checkint()`: 0 if not an integer, 1 if an odd integer, 2 if an even integer. `iy` is the bit
/// representation of a non-zero finite value.
fn checkint(iy: u64) -> i32 {
    let e = ((iy >> 52) & 0x7ff) as i32;
    if e < 0x3ff {
        return 0;
    }
    if e > 0x3ff + 52 {
        return 2;
    }
    if iy & ((1u64 << (0x3ff + 52 - e)) - 1) != 0 {
        return 0;
    }
    if iy & (1u64 << (0x3ff + 52 - e)) != 0 {
        return 1;
    }
    2
}

/// `zeroinfnan()`: whether `i` is the bit representation of 0, infinity or NaN.
fn zeroinfnan(i: u64) -> bool {
    (2u64.wrapping_mul(i)).wrapping_sub(1) >= (2 * INF_BITS) - 1
}

/// `issignaling_inline()`.
fn issignaling(x: f64) -> bool {
    let ix = x.to_bits();
    2u64.wrapping_mul(ix ^ 0x0008_0000_0000_0000) > 2 * 0x7ff8_0000_0000_0000
}

/// `x` raised to the power `y`: the C library `pow()` PHP's `**` and `pow()` call (glibc 2.41
/// `__ieee754_pow_fma`, see the module documentation). Special cases follow C99 Annex F
/// (`pow(x, ±0) = 1`, `pow(1, y) = 1`, `pow(-x, non-integer) = NaN`, ...).
pub fn pow(x: f64, y: f64) -> f64 {
    let mut sign_bias: u64 = 0;
    let mut ix = x.to_bits();
    let iy = y.to_bits();
    let mut topx = top12(x);
    let topy = top12(y);
    if topx.wrapping_sub(0x001) >= 0x7ff - 0x001
        || (topy & 0x7ff).wrapping_sub(0x3be) >= 0x43e - 0x3be
    {
        // Note: if |y| > 1075 * ln2 * 2^53 ~= 0x1.749p62 then pow(x,y) = inf/0
        // and if |y| < 2^-54 / 1075 ~= 0x1.e7b6p-65 then pow(x,y) = +-1.
        // Special cases: (x < 0x1p-126 or inf or nan) or
        // (|y| < 0x1p-65 or |y| >= 0x1p63 or nan).
        if zeroinfnan(iy) {
            if iy.wrapping_mul(2) == 0 {
                return if issignaling(x) { x + y } else { 1.0 };
            }
            if ix == ONE_BITS {
                return if issignaling(y) { x + y } else { 1.0 };
            }
            if ix.wrapping_mul(2) > 2 * INF_BITS || iy.wrapping_mul(2) > 2 * INF_BITS {
                return x + y;
            }
            if ix.wrapping_mul(2) == 2 * ONE_BITS {
                return 1.0;
            }
            if (ix.wrapping_mul(2) < 2 * ONE_BITS) == (iy >> 63 == 0) {
                return 0.0; // |x|<1 && y==inf or |x|>1 && y==-inf.
            }
            return y * y;
        }
        if zeroinfnan(ix) {
            let mut x2 = x * x;
            if ix >> 63 != 0 && checkint(iy) == 1 {
                x2 = -x2;
            }
            // glibc returns __math_divzero(sign) = (sign ? -1.0 : 1.0) / 0.0 for x == 0 and
            // y < 0; that equals 1 / x2.
            return if iy >> 63 != 0 { 1.0 / x2 } else { x2 };
        }
        // Here x and y are non-zero finite.
        if ix >> 63 != 0 {
            // Finite x < 0.
            let yint = checkint(iy);
            if yint == 0 {
                return invalid(x);
            }
            if yint == 1 {
                sign_bias = SIGN_BIAS;
            }
            ix &= 0x7fff_ffff_ffff_ffff;
            topx &= 0x7ff;
        }
        if (topy & 0x7ff).wrapping_sub(0x3be) >= 0x43e - 0x3be {
            // Note: sign_bias == 0 here because y is not odd.
            if ix == ONE_BITS {
                return 1.0;
            }
            if (topy & 0x7ff) < 0x3be {
                // |y| < 2^-65, x^y ~= 1 + y*log(x).
                return if ix > ONE_BITS { 1.0 + y } else { 1.0 - y };
            }
            return if (ix > ONE_BITS) == (topy < 0x800) {
                oflow(0)
            } else {
                uflow(0)
            };
        }
        if topx == 0 {
            // Normalize subnormal x so exponent becomes negative.
            ix = (x * f64::from_bits(0x4330_0000_0000_0000)).to_bits(); // x * 0x1p52
            ix &= 0x7fff_ffff_ffff_ffff;
            ix = ix.wrapping_sub(52u64 << 52);
        }
    }

    let (hi, lo) = log_inline(ix);
    // __FP_FAST_FMA branch: ehi = y * hi; elo = y * lo + fma(y, hi, -ehi)  (the sum is contracted)
    let ehi = y * hi;
    let elo = fma(y, lo, fma(y, hi, -ehi));
    exp_inline(ehi, elo, sign_bias)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn special_values() {
        let nan = f64::NAN;
        let inf = f64::INFINITY;
        assert_eq!(pow(2.0, 3.0), 8.0);
        assert_eq!(pow(2.0, -1.0), 0.5);
        assert_eq!(pow(2.0, 0.5), std::f64::consts::SQRT_2);
        assert_eq!(pow(nan, 0.0), 1.0);
        assert_eq!(pow(1.0, nan), 1.0);
        assert!(pow(nan, 1.0).is_nan());
        assert!(pow(-8.0, 1.0 / 3.0).is_nan());
        assert_eq!(pow(0.0, -1.0), inf);
        assert_eq!(pow(-0.0, -1.0), -inf);
        assert_eq!(pow(-0.0, -2.0), inf);
        assert_eq!(pow(-0.0, 3.0).to_bits(), (-0.0f64).to_bits());
        assert_eq!(pow(-1.0, inf), 1.0);
        assert_eq!(pow(0.5, inf), 0.0);
        assert_eq!(pow(2.0, inf), inf);
        assert_eq!(pow(2.0, -inf), 0.0);
        assert_eq!(pow(-2.0, 3.0), -8.0);
        assert_eq!(pow(10.0, 20.0), 1e20);
        assert_eq!(pow(2.0, 1024.0), inf);
        assert_eq!(pow(-2.0, 1025.0), -inf);
        assert_eq!(pow(2.0, -1074.0), f64::from_bits(1));
        assert_eq!(pow(2.0, -1075.0), 0.0);
        assert_eq!(
            pow(f64::from_bits(1), 0.5),
            f64::from_bits(0x1e60_0000_0000_0000)
        );
        assert_eq!(pow(9.0, 0.5), 3.0);
        assert_eq!(pow(1.0000000000000002, 1e300), inf);
        assert_eq!(pow(3.0, 1e-70), 1.0);
    }
}
