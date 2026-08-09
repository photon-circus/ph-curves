//! Fixed-point math helpers and the [`UnitValue`] abstraction.
//!
//! All arithmetic uses the [`fixed`] crate or plain integers so that the
//! library remains `no_std` and avoids floating-point operations at runtime.

use fixed::types::{I16F16, I32F32, U16F16};

/// Rounding policy used by [`quantize`] and the tickless scheduler.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Rounding {
    /// Round down to the nearest multiple of `step`.
    Floor,
    /// Round up to the nearest multiple of `step`.
    Ceil,
    /// Round to the nearest multiple of `step` (half-up).
    Nearest,
}

// ---------------------------------------------------------------------------
// UnitValue trait
// ---------------------------------------------------------------------------

/// A normalized value type usable as a curve domain / range.
///
/// Implementors map a unit interval onto a discrete integer type so that the
/// tickless scheduler can convert between wall-clock time fractions and curve
/// positions.
pub trait UnitValue: Copy + 'static {
    /// The value representing 0 (start of the interval).
    fn zero() -> Self;
    /// The value representing 1 (end of the interval).
    fn one() -> Self;

    /// Convert this value to a LUT index (0-based).
    fn to_index(self) -> usize;

    /// Create a value from a time fraction `elapsed / duration`.
    ///
    /// Returns [`Self::zero()`] when `elapsed == 0` and [`Self::one()`] when
    /// `elapsed >= duration`.
    fn from_time_frac(elapsed_ms: u32, duration_ms: u32) -> Self;

    /// Convert this value back to a wall-clock offset in milliseconds within
    /// `duration_ms`, rounding up.
    fn to_time_offset(self, duration_ms: u32) -> u32;

    /// Linearly interpolate between two `u16` endpoints using `self` as the
    /// blend weight.
    fn lerp_u16(self, a: u16, b: u16) -> u16;

    /// Inverse linear interpolation: find the blend weight `T` such that
    /// `T::lerp_u16(a, b) ≈ target`, rounding toward `b`.
    fn inv_lerp_u16(a: u16, b: u16, target: u16) -> Self;
}

// ---------------------------------------------------------------------------
// UnitValue implementation for u8
// ---------------------------------------------------------------------------

impl UnitValue for u8 {
    fn zero() -> Self {
        0
    }

    fn one() -> Self {
        255
    }

    fn to_index(self) -> usize {
        self as usize
    }

    fn from_time_frac(elapsed_ms: u32, duration_ms: u32) -> Self {
        if duration_ms == 0 || elapsed_ms >= duration_ms {
            return 255;
        }
        if elapsed_ms == 0 {
            return 0;
        }
        (u64::from(elapsed_ms) * 255 / u64::from(duration_ms)) as u8
    }

    fn to_time_offset(self, duration_ms: u32) -> u32 {
        if duration_ms == 0 {
            return 0;
        }
        // Splitting `duration_ms` as `255 * k + r` keeps this in 32-bit
        // arithmetic: `ceil(v * d / 255) == v * k + ceil(v * r / 255)`.
        // `v * k` peaks at exactly `u32::MAX` and `v * r` at 255 * 254, so
        // neither term overflows, and the sum is still bounded by
        // `duration_ms`.  A 64-bit divide costs ~2.3x as much on Cortex-M0.
        let v = u32::from(self);
        let k = duration_ms / 255;
        let r = duration_ms % 255;
        v * k + (v * r).div_ceil(255)
    }

    fn lerp_u16(self, a: u16, b: u16) -> u16 {
        let a_fix = I32F32::from_num(a);
        let b_fix = I32F32::from_num(b);
        let t = I32F32::from_num(self) / I32F32::from_num(255);
        let result = a_fix + t * (b_fix - a_fix);
        result.to_num::<i64>().clamp(0, u16::MAX as i64) as u16
    }

    fn inv_lerp_u16(a: u16, b: u16, target: u16) -> Self {
        if a == b {
            return 255;
        }
        let delta = I32F32::from_num(b) - I32F32::from_num(a);
        let numer = I32F32::from_num(target) - I32F32::from_num(a);
        let frac = numer / delta;
        let w = (frac * I32F32::from_num(255)).ceil();
        w.to_num::<i32>().clamp(0, 255) as u8
    }
}

// ---------------------------------------------------------------------------
// UnitValue implementation for u16
// ---------------------------------------------------------------------------

impl UnitValue for u16 {
    fn zero() -> Self {
        0
    }

    fn one() -> Self {
        65535
    }

    fn to_index(self) -> usize {
        self as usize
    }

    fn from_time_frac(elapsed_ms: u32, duration_ms: u32) -> Self {
        if duration_ms == 0 || elapsed_ms >= duration_ms {
            return 65535;
        }
        if elapsed_ms == 0 {
            return 0;
        }
        // `u64` rather than `I32F32`: the fixed-point type holds only 32
        // integer bits, so durations above `i32::MAX` overflowed on
        // conversion.  The guard above bounds the quotient below 65535.
        (u64::from(elapsed_ms) * 65535 / u64::from(duration_ms)) as u16
    }

    fn to_time_offset(self, duration_ms: u32) -> u32 {
        if duration_ms == 0 {
            return 0;
        }
        // As for `u8`: `ceil(v * d / 65535) == v * k + ceil(v * r / 65535)`
        // where `d == 65535 * k + r`.  `v * k` peaks at exactly `u32::MAX`
        // and `v * r` at `65535 * 65534`, so both stay in 32 bits.
        let v = u32::from(self);
        let k = duration_ms / 65535;
        let r = duration_ms % 65535;
        v * k + (v * r).div_ceil(65535)
    }

    fn lerp_u16(self, a: u16, b: u16) -> u16 {
        let a_fix = I32F32::from_num(a);
        let b_fix = I32F32::from_num(b);
        let t = I32F32::from_num(self) / I32F32::from_num(65535);
        let result = a_fix + t * (b_fix - a_fix);
        result.to_num::<i64>().clamp(0, u16::MAX as i64) as u16
    }

    fn inv_lerp_u16(a: u16, b: u16, target: u16) -> Self {
        if a == b {
            return 65535;
        }
        let delta = I32F32::from_num(b) - I32F32::from_num(a);
        let numer = I32F32::from_num(target) - I32F32::from_num(a);
        let frac = numer / delta;
        let w = (frac * I32F32::from_num(65535)).ceil();
        w.to_num::<i64>().clamp(0, 65535) as u16
    }
}

// ---------------------------------------------------------------------------
// Interpolation helpers
// ---------------------------------------------------------------------------

/// Convert a `u8` weight (`0..=255`) to a fixed-point fraction in `[0, 1]`.
fn weight_frac(w: u8) -> I16F16 {
    I16F16::from_num(w) / I16F16::from_num(255)
}

/// Linearly interpolate between `a` and `b` with a `u8` blend weight.
///
/// `w = 0` returns `a`, `w = 255` returns `b`.  Intermediate values are
/// computed with fixed-point arithmetic and clamped to `0..=255`.
pub fn lerp_u8(a: u8, b: u8, w: u8) -> u8 {
    let a_fix = I16F16::from_num(a);
    let b_fix = I16F16::from_num(b);
    let t = weight_frac(w);
    let result = a_fix + t * (b_fix - a_fix);
    result.to_num::<i32>().clamp(0, 255) as u8
}

/// Linearly interpolate between two `u16` values with a `u8` blend weight.
///
/// `w = 0` returns `a`, `w = 255` returns `b`.  The result is clamped to
/// `0..=u16::MAX`.
pub fn lerp_u16(a: u16, b: u16, w: u8) -> u16 {
    let a_fix = I32F32::from_num(a);
    let b_fix = I32F32::from_num(b);
    let t = I32F32::from_num(w) / I32F32::from_num(255);
    let result = a_fix + t * (b_fix - a_fix);
    result.to_num::<i64>().clamp(0, u16::MAX as i64) as u16
}

/// Scale a normalized `u8` value (`0..=255`) into a `u16` range `0..=max`.
///
/// `w = 0` returns `0`, `w = 255` returns `max`.
pub fn map_u8_to_u16(w: u8, max: u16) -> u16 {
    let t = U16F16::from_num(w) / U16F16::from_num(255);
    let result = t * U16F16::from_num(max);
    result.to_num::<u32>().min(u16::MAX as u32) as u16
}

// ---------------------------------------------------------------------------
// Quantization helpers
// ---------------------------------------------------------------------------

/// Snap `value` to the nearest multiple of `step` according to `rounding`.
///
/// # Panics
///
/// Panics if `step` is `0` (division by zero).
pub fn quantize(value: u16, step: u16, rounding: Rounding) -> u16 {
    let v = u32::from(value);
    let s = u32::from(step);
    let result = match rounding {
        Rounding::Floor => (v / s) * s,
        Rounding::Ceil => v.div_ceil(s) * s,
        Rounding::Nearest => ((v + s / 2) / s) * s,
    };
    result.min(u32::from(u16::MAX)) as u16
}

/// Return the next quantized target value one `step` closer to `end`.
///
/// If `increasing` is `true` the value advances upward; otherwise downward.
/// The result is clamped so it never overshoots `end`.
pub fn next_target_value(current: u16, end: u16, step: u16, increasing: bool) -> u16 {
    if increasing {
        let next = current.saturating_add(step);
        next.min(end)
    } else {
        let next = current.saturating_sub(step);
        next.max(end)
    }
}
