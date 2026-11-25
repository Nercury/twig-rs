//! C library math functions whose results PHP exposes, reproduced bit for bit.
//!
//! PHP's `**`/`pow()` call the C library `pow()`, `floor()`/`ceil()` call C `floor`/`ceil`, and
//! float-to-int wrapping uses `fmod()`. The oracle runs glibc 2.41 on x86-64; Rust's `f64::powf`
//! uses the platform C library (the Windows CRT differs in the last bit, so does the `libm` crate),
//! hence the ports here:
//!
//! * [`pow`]: ARM optimized-routines `pow` as built into glibc's FMA variant (see [`pow`] for the
//!   details and the validation);
//! * [`floor`], [`ceil`], [`trunc`], [`fmod`]: exactly specified operations, ported from musl so
//!   the result never depends on the platform;
//! * [`fma`]: correctly rounded fused multiply-add (hardware instruction when the target has one,
//!   otherwise a port of musl's software `fma`).
//!
//! The ports carry their upstream copyright notices (musl: MIT; ARM optimized-routines: MIT).

mod basic;
mod pow;
mod pow_data;
mod soft_fma;

pub use basic::{ceil, floor, fmod, trunc};
pub use pow::pow;
pub use soft_fma::fma_soft;

/// Correctly rounded `x * y + z` (one rounding, C99 `fma()`).
///
/// Uses the hardware instruction when the build target has one (x86-64 with
/// `-C target-feature=+fma`, aarch64) and a port of musl's software `fma()` otherwise, so the result
/// never depends on the platform C library (some C runtimes implement `fma` as an unfused
/// `x * y + z`).
#[inline]
pub fn fma(x: f64, y: f64, z: f64) -> f64 {
    #[cfg(any(target_feature = "fma", target_arch = "aarch64"))]
    {
        x.mul_add(y, z)
    }
    #[cfg(not(any(target_feature = "fma", target_arch = "aarch64")))]
    {
        soft_fma::fma_soft(x, y, z)
    }
}
