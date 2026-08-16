//! Independent quadratic oracle for the family-acceptance fixture.
//!
//! Mirrors the generator's scaled-input and Horner evaluation without calling
//! host internals: `u = (count as u64 * numerator) as f64 / denominator`,
//! `y = c0 + c1 u + c2 u²`.

/// Shared scaled-polynomial coefficients from `assets/family-acceptance.toml`.
pub const COEFFICIENTS: [f64; 3] = [0.0, 1.0, 1.0e-4];

/// Family `output_scale`.
pub const OUTPUT_SCALE: u32 = 1000;

/// Low-range `input_transform` (`numerator`, `denominator`).
pub const LOW_TRANSFORM: (u32, u32) = (2_000, 1_000_000);

/// Mid-range `input_transform`.
pub const MID_TRANSFORM: (u32, u32) = (8_000, 1_000_000);

/// High-range `input_transform`.
pub const HIGH_TRANSFORM: (u32, u32) = (32_000, 1_000_000);

/// Exact integer-product model input used by `kind = "scaled_polynomial"`.
pub fn scaled_input(count: u16, numerator: u32, denominator: u32) -> f64 {
    let product = u64::from(count) * u64::from(numerator);
    (product as f64) / f64::from(denominator)
}

/// Horner evaluation of `[c0, c1, c2, ...]`.
pub fn horner(coefficients: &[f64], u: f64) -> f64 {
    let mut acc = 0.0;
    for &coefficient in coefficients.iter().rev() {
        acc = acc * u + coefficient;
    }
    acc
}

/// Unscaled physical oracle at one observation code.
pub fn oracle_physical(code: u16, numerator: u32, denominator: u32) -> f64 {
    horner(&COEFFICIENTS, scaled_input(code, numerator, denominator))
}

/// Scaled physical oracle (`y * output_scale`) at one observation code.
pub fn oracle_scaled(code: u16, numerator: u32, denominator: u32) -> f64 {
    oracle_physical(code, numerator, denominator) * f64::from(OUTPUT_SCALE)
}
