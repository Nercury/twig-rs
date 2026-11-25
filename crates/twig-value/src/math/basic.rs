//! `floor()`, `ceil()`, `trunc()` and `fmod()`: exact operations, ported from musl (MIT,
//! `src/math/floor.c`, `ceil.c`, `trunc.c`, `fmod.c`) so that results never depend on the platform
//! C library. They agree with glibc (the oracle's libm) because every one of them is exactly
//! specified by IEEE 754 / C99 (no rounding error).

/// `1 / DBL_EPSILON` (`toint`).
const TOINT: f64 = 4_503_599_627_370_496.0;

/// Port of musl's `floor()`.
pub fn floor(x: f64) -> f64 {
    let i = x.to_bits();
    let e = ((i >> 52) & 0x7ff) as i32;
    if e >= 0x3ff + 52 || x == 0.0 {
        return x;
    }
    // y = int(x) - x, where int(x) is an integer neighbor of x
    let y = if i >> 63 != 0 {
        x - TOINT + TOINT - x
    } else {
        x + TOINT - TOINT - x
    };
    // special case because of non-nearest rounding modes (C: `e <= 0x3ff-1`)
    if e < 0x3ff {
        return if i >> 63 != 0 { -1.0 } else { 0.0 };
    }
    if y > 0.0 { x + y - 1.0 } else { x + y }
}

/// Port of musl's `ceil()`.
pub fn ceil(x: f64) -> f64 {
    let i = x.to_bits();
    let e = ((i >> 52) & 0x7ff) as i32;
    if e >= 0x3ff + 52 || x == 0.0 {
        return x;
    }
    // y = int(x) - x, where int(x) is an integer neighbor of x
    let y = if i >> 63 != 0 {
        x - TOINT + TOINT - x
    } else {
        x + TOINT - TOINT - x
    };
    // special case because of non-nearest rounding modes (C: `e <= 0x3ff-1`)
    if e < 0x3ff {
        return if i >> 63 != 0 { -0.0 } else { 1.0 };
    }
    if y < 0.0 { x + y + 1.0 } else { x + y }
}

/// Port of musl's `trunc()`.
pub fn trunc(x: f64) -> f64 {
    let mut i = x.to_bits();
    let mut e = ((i >> 52) & 0x7ff) as i32 - 0x3ff + 12;
    if e >= 52 + 12 {
        return x;
    }
    if e < 12 {
        e = 1;
    }
    let m = u64::MAX >> e;
    if i & m == 0 {
        return x;
    }
    i &= !m;
    f64::from_bits(i)
}

/// Port of musl's `fmod()`: the exact remainder of `x / y` with the sign of `x` (NaN for
/// `y == 0`, infinite `x` or NaN operands).
// `(x*y)/(x*y)` deliberately produces the NaN of the invalid cases, like the C code.
#[allow(clippy::eq_op)]
pub fn fmod(x: f64, y: f64) -> f64 {
    let mut uyi = y.to_bits();
    let mut ex = ((x.to_bits() >> 52) & 0x7ff) as i32;
    let mut ey = ((uyi >> 52) & 0x7ff) as i32;
    let sx = x.to_bits() >> 63;
    let mut uxi = x.to_bits();

    if uyi << 1 == 0 || y.is_nan() || ex == 0x7ff {
        return (x * y) / (x * y);
    }
    if uxi << 1 <= uyi << 1 {
        if uxi << 1 == uyi << 1 {
            return 0.0 * x;
        }
        return x;
    }

    // normalize x and y
    let mut i: u64;
    if ex == 0 {
        i = uxi << 12;
        while i >> 63 == 0 {
            ex -= 1;
            i <<= 1;
        }
        uxi <<= -ex + 1;
    } else {
        uxi &= u64::MAX >> 12;
        uxi |= 1u64 << 52;
    }
    if ey == 0 {
        i = uyi << 12;
        while i >> 63 == 0 {
            ey -= 1;
            i <<= 1;
        }
        uyi <<= -ey + 1;
    } else {
        uyi &= u64::MAX >> 12;
        uyi |= 1u64 << 52;
    }

    // x mod y
    while ex > ey {
        i = uxi.wrapping_sub(uyi);
        if i >> 63 == 0 {
            if i == 0 {
                return 0.0 * x;
            }
            uxi = i;
        }
        uxi <<= 1;
        ex -= 1;
    }
    i = uxi.wrapping_sub(uyi);
    if i >> 63 == 0 {
        if i == 0 {
            return 0.0 * x;
        }
        uxi = i;
    }
    while uxi >> 52 == 0 {
        uxi <<= 1;
        ex -= 1;
    }

    // scale result
    if ex > 0 {
        uxi -= 1u64 << 52;
        uxi |= (ex as u64) << 52;
    } else {
        uxi >>= -ex + 1;
    }
    uxi |= sx << 63;
    f64::from_bits(uxi)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn same(a: f64, b: f64) -> bool {
        a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan())
    }

    #[test]
    fn agree_with_std() {
        let mut s: u64 = 0x1234_5678_9abc_def0;
        let mut next = || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            s
        };
        let specials = [
            0.0,
            -0.0,
            0.5,
            -0.5,
            1.0,
            -1.0,
            1.5,
            -1.5,
            2.5,
            4503599627370495.5,
            -4503599627370495.5,
            4503599627370496.0,
            9007199254740993.0,
            f64::MIN_POSITIVE,
            f64::from_bits(1),
            f64::MAX,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NAN,
            1e300,
            -1e-300,
            18446744073709551616.0,
        ];
        let mut values: Vec<f64> = specials.to_vec();
        for _ in 0..200_000 {
            let b = next();
            values.push(match b % 3 {
                0 => f64::from_bits(b),
                1 => (b >> 11) as f64 / 1024.0 * if b & 1 == 0 { 1.0 } else { -1.0 },
                _ => f64::from_bits((b & 0x803f_ffff_ffff_ffff) | (0x3ff0 << 48)),
            });
        }
        for &x in &values {
            assert!(same(floor(x), x.floor()), "floor({x:e})");
            assert!(same(ceil(x), x.ceil()), "ceil({x:e})");
            assert!(same(trunc(x), x.trunc()), "trunc({x:e})");
        }
        for w in values.windows(2) {
            let (x, y) = (w[0], w[1]);
            assert!(same(fmod(x, y), x % y), "fmod({x:e}, {y:e})");
            assert!(same(fmod(y, 3.0), y % 3.0), "fmod({y:e}, 3)");
            assert!(
                same(fmod(x, 18446744073709551616.0), x % 18446744073709551616.0),
                "fmod({x:e}, 2^64)"
            );
        }
    }
}
