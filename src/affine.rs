//! Standalone invertible `i32` affine transform.
//!
//! [`AffineTransform`] applies `y' = (y * gain + offset) / scale` to an
//! already-converted signed measurement. It is not a
//! [`crate::TransferFunction`] and does not own or update
//! [`crate::TransferMetadata`]: the coefficients are caller runtime state,
//! not table-fit facts.
//!
//! [`crate::AffineCalibration`] contains this type and applies it after an
//! inner transfer. Use the scalar primitive directly when the measurement
//! already exists as `i32`.

use crate::round::div_nearest_ties_away;

/// Invertible `i32` affine map `y' = (y * gain + offset) / scale`.
///
/// Arithmetic uses `i64` intermediates and nearest, ties-away-from-zero
/// rounding. Identity (modulo rounding when `|scale| ≠ 1`) is
/// `gain = scale` and `offset = 0`. A negative `gain` or `scale` is allowed
/// and flips sense.
///
/// This type never reads NVM, wraps a transfer, or writes
/// [`TransferMetadata`]. [`AffineCalibration`] contains one of these and
/// delegates its gain/offset/scale arithmetic here.
///
/// # Inverse
///
/// [`unapply`](Self::unapply) solves `y = (y' * scale - offset) / gain` with
/// the same rounding. Because a zero gain collapses every input onto
/// `offset / scale`, [`new`](Self::new) rejects `gain == 0` rather than
/// deferring the failure to `unapply`.
///
/// # Numerical scope
///
/// For any `i32` `y`, `gain`, and `offset`, the product/sum
/// `y * gain + offset` always fits in `i64`; the same holds for
/// `y' * scale - offset` on the inverse path. Both directions still report
/// [`AffineOverflow::Overflow`] when the rounded result does not fit `i32`.
///
/// Both directions round, so `unapply(apply(y))` is bounded rather than
/// exact. A transform that compresses the scale cannot restore what the
/// forward quantization discarded. At `i32` extremes, inverse rounding of a
/// forward result can land just outside `i32`, in which case `unapply`
/// reports overflow.
///
/// [`TransferFunction`]: crate::TransferFunction
/// [`TransferMetadata`]: crate::TransferMetadata
/// [`AffineCalibration`]: crate::AffineCalibration
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct AffineTransform {
    gain: i32,
    offset: i32,
    scale: i32,
}

/// Error returned when affine transform coefficients are invalid.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum AffineTransformError {
    /// The scale divisor is zero.
    ZeroScale,
    /// The gain is zero, which collapses every input onto one output.
    ZeroGain,
}

/// Error returned when affine arithmetic cannot be represented in `i32`.
///
/// Distinct from [`crate::TransferError`] and [`crate::InverseTransferError`]:
/// those carry domain and range variants that a scalar caller has no use for.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum AffineOverflow {
    /// The rounded result does not fit in `i32`.
    Overflow,
}

impl AffineTransform {
    /// Construct an invertible affine transform.
    ///
    /// # Errors
    ///
    /// Returns [`AffineTransformError::ZeroScale`] if `scale == 0`, or
    /// [`AffineTransformError::ZeroGain`] if `gain == 0`. A zero gain maps
    /// every input onto the single value `offset / scale` and has no inverse.
    pub const fn new(gain: i32, offset: i32, scale: i32) -> Result<Self, AffineTransformError> {
        if scale == 0 {
            return Err(AffineTransformError::ZeroScale);
        }
        if gain == 0 {
            return Err(AffineTransformError::ZeroGain);
        }
        Ok(Self {
            gain,
            offset,
            scale,
        })
    }

    /// Return the gain coefficient.
    pub const fn gain(&self) -> i32 {
        self.gain
    }

    /// Return the offset term.
    pub const fn offset(&self) -> i32 {
        self.offset
    }

    /// Return the nonzero scale divisor.
    pub const fn scale(&self) -> i32 {
        self.scale
    }

    /// Apply `y' = (y * gain + offset) / scale`.
    ///
    /// Rounding is nearest, ties away from zero.
    ///
    /// # Errors
    ///
    /// Returns [`AffineOverflow::Overflow`] when the rounded result does not
    /// fit in `i32`.
    pub fn apply(&self, value: i32) -> Result<i32, AffineOverflow> {
        // `scale != 0` is a constructor invariant.
        debug_assert!(self.scale != 0);

        let product = i64::from(value)
            .checked_mul(i64::from(self.gain))
            .ok_or(AffineOverflow::Overflow)?;
        let numerator = product
            .checked_add(i64::from(self.offset))
            .ok_or(AffineOverflow::Overflow)?;
        let scaled = div_nearest_ties_away(numerator, i64::from(self.scale));
        i32::try_from(scaled).map_err(|_| AffineOverflow::Overflow)
    }

    /// Undo `y' = (y * gain + offset) / scale`, recovering `y`.
    ///
    /// Solves `y = (y' * scale - offset) / gain` with the same nearest,
    /// ties-away rounding. Offset is subtracted in `i64` rather than negated
    /// as `i32`, so `offset == i32::MIN` is representable.
    ///
    /// Both directions round, so `unapply(apply(y))` is bounded rather than
    /// exact: a transform that compresses the scale cannot restore what the
    /// forward quantization discarded.
    ///
    /// # Errors
    ///
    /// Returns [`AffineOverflow::Overflow`] when the rounded result does not
    /// fit in `i32`.
    pub fn unapply(&self, value: i32) -> Result<i32, AffineOverflow> {
        debug_assert!(self.gain != 0 && self.scale != 0);

        // `|value * scale|` is at most `2^62`, so neither step can overflow
        // `i64` for any `i32` operands.
        let product = i64::from(value)
            .checked_mul(i64::from(self.scale))
            .ok_or(AffineOverflow::Overflow)?;
        let numerator = product
            .checked_sub(i64::from(self.offset))
            .ok_or(AffineOverflow::Overflow)?;
        let uncalibrated = div_nearest_ties_away(numerator, i64::from(self.gain));
        i32::try_from(uncalibrated).map_err(|_| AffineOverflow::Overflow)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transform(gain: i32, offset: i32, scale: i32) -> AffineTransform {
        AffineTransform::new(gain, offset, scale).unwrap()
    }

    /// Reconstruction error bound in input quanta for `unapply(apply(y))`.
    ///
    /// Forward rounding is at most half a scale quantum and inverse rounding
    /// is at most half a gain quantum. Expressed in input units that is
    /// bounded by `(|scale| + |gain| - 1) / |gain|`.
    fn reconstruction_bound(gain: i32, scale: i32) -> i64 {
        let gain_abs = i64::from(gain.unsigned_abs());
        let scale_abs = i64::from(scale.unsigned_abs());
        (scale_abs + gain_abs - 1) / gain_abs
    }

    #[test]
    fn constructor_rejects_zero_scale_and_zero_gain() {
        assert_eq!(
            AffineTransform::new(1, 0, 0),
            Err(AffineTransformError::ZeroScale)
        );
        assert_eq!(
            AffineTransform::new(0, 5, 1),
            Err(AffineTransformError::ZeroGain)
        );
        assert_eq!(
            AffineTransform::new(0, 0, 0),
            Err(AffineTransformError::ZeroScale)
        );
    }

    #[test]
    fn accessors_echo_the_coefficients() {
        let t = transform(1_005, -120, 1_000);
        assert_eq!(t.gain(), 1_005);
        assert_eq!(t.offset(), -120);
        assert_eq!(t.scale(), 1_000);
    }

    #[test]
    fn identity_is_passthrough() {
        let t = transform(1, 0, 1);
        assert_eq!(t.apply(0), Ok(0));
        assert_eq!(t.apply(42), Ok(42));
        assert_eq!(t.apply(-7), Ok(-7));
        assert_eq!(t.apply(i32::MAX), Ok(i32::MAX));
        assert_eq!(t.apply(i32::MIN), Ok(i32::MIN));
        assert_eq!(t.unapply(i32::MAX), Ok(i32::MAX));
        assert_eq!(t.unapply(i32::MIN), Ok(i32::MIN));
    }

    #[test]
    fn positive_and_negative_gain_and_scale() {
        assert_eq!(transform(2, 0, 1).apply(10), Ok(20));
        assert_eq!(transform(-2, 0, 1).apply(10), Ok(-20));
        assert_eq!(transform(2, 0, -1).apply(10), Ok(-20));
        assert_eq!(transform(-2, 0, -1).apply(10), Ok(20));

        // Factory-style +0.5 % / -120 offset.
        let trim = transform(1_005, -120, 1_000);
        // (-500 * 1005 + -120) / 1000 = -502.62 → -503
        assert_eq!(trim.apply(-500), Ok(-503));
        // (0 * 1005 + -120) / 1000 = -0.12 → 0
        assert_eq!(trim.apply(0), Ok(0));
        // (2000 * 1005 + -120) / 1000 = 2009.88 → 2010
        assert_eq!(trim.apply(2_000), Ok(2_010));
    }

    #[test]
    fn ties_round_away_from_zero_in_every_sign_combination() {
        // 1/2 → 1 and -1/2 → -1 (ties away from zero)
        assert_eq!(transform(1, 0, 2).apply(1), Ok(1));
        assert_eq!(transform(1, 0, 2).apply(-1), Ok(-1));
        assert_eq!(transform(1, 0, -2).apply(1), Ok(-1));
        assert_eq!(transform(1, 0, -2).apply(-1), Ok(1));
        assert_eq!(transform(1, 0, 2).apply(3), Ok(2));
        assert_eq!(transform(1, 0, 2).apply(-3), Ok(-2));

        // Inverse ties: (y' * scale) / gain with scale = 1, gain = 2.
        assert_eq!(transform(2, 0, 1).unapply(1), Ok(1));
        assert_eq!(transform(2, 0, 1).unapply(-1), Ok(-1));
        assert_eq!(transform(-2, 0, 1).unapply(1), Ok(-1));
        assert_eq!(transform(-2, 0, 1).unapply(-1), Ok(1));
    }

    #[test]
    fn i32_extremes_are_representable_when_the_result_fits() {
        let identity = transform(1, 0, 1);
        assert_eq!(identity.apply(i32::MAX), Ok(i32::MAX));
        assert_eq!(identity.apply(i32::MIN), Ok(i32::MIN));
        assert_eq!(identity.unapply(i32::MAX), Ok(i32::MAX));
        assert_eq!(identity.unapply(i32::MIN), Ok(i32::MIN));

        // Offset at i32::MIN is subtracted in i64, not negated as i32.
        let shifted = transform(1, i32::MIN, 1);
        assert_eq!(shifted.apply(0), Ok(i32::MIN));
        assert_eq!(shifted.unapply(i32::MIN), Ok(0));
        assert_eq!(shifted.apply(1), Ok(i32::MIN + 1));
        assert_eq!(shifted.unapply(i32::MIN + 1), Ok(1));

        let high_offset = transform(1, i32::MAX, 1);
        assert_eq!(high_offset.apply(0), Ok(i32::MAX));
        assert_eq!(high_offset.unapply(i32::MAX), Ok(0));
    }

    #[test]
    fn representable_intermediates_still_overflow_i32() {
        // i32::MAX * 2 fits in i64 and overflows i32.
        assert_eq!(
            transform(2, 0, 1).apply(i32::MAX),
            Err(AffineOverflow::Overflow)
        );
        assert_eq!(
            transform(i32::MAX, 0, 1).apply(2),
            Err(AffineOverflow::Overflow)
        );
        // Negating i32::MIN overflows i32 even though the i64 product fits.
        assert_eq!(
            transform(-1, 0, 1).apply(i32::MIN),
            Err(AffineOverflow::Overflow)
        );
        // Inverse: MAX * MAX / 1 does not fit i32.
        assert_eq!(
            transform(1, 0, i32::MAX).unapply(i32::MAX),
            Err(AffineOverflow::Overflow)
        );
    }

    #[test]
    fn identity_round_trips_are_exact() {
        let t = transform(1, 0, 1);
        for y in [i32::MIN, i32::MIN + 1, -1, 0, 1, i32::MAX - 1, i32::MAX] {
            let applied = t.apply(y).unwrap();
            assert_eq!(t.unapply(applied), Ok(y));
            let undone = t.unapply(y).unwrap();
            assert_eq!(t.apply(undone), Ok(y));
        }
    }

    #[test]
    fn compressing_round_trips_are_bounded_not_exact() {
        let t = transform(2, 0, 3);
        assert_eq!(t.apply(1), Ok(1));
        assert_eq!(t.unapply(1), Ok(2));
        assert_ne!(t.unapply(t.apply(1).unwrap()), Ok(1));

        let bound = reconstruction_bound(2, 3);
        for y in -2_000..=2_000 {
            let applied = t.apply(y).unwrap();
            let recovered = t.unapply(applied).unwrap();
            let error = i64::from(recovered) - i64::from(y);
            assert!(
                error.abs() <= bound,
                "y={y}: apply -> {applied} -> unapply -> {recovered} (Δ={error}, bound={bound})"
            );
        }
    }

    #[test]
    fn interior_apply_results_are_unapplyable() {
        let cases = [
            transform(1, 0, 1),
            transform(1_005, -120, 1_000),
            transform(2, 0, 3),
            transform(-2, 0, 3),
            transform(3, 0, 2),
            transform(1, i32::MIN, 1),
            transform(-1, 0, 2),
        ];
        let samples = [-10_000, -1, 0, 1, 10_000];

        for t in cases {
            for y in samples {
                let Ok(applied) = t.apply(y) else {
                    continue;
                };
                t.unapply(applied).unwrap_or_else(|_| {
                    panic!(
                        "apply({y}) -> {applied} was not unapplyable for gain={} offset={} scale={}",
                        t.gain(),
                        t.offset(),
                        t.scale()
                    )
                });
            }
        }
    }

    #[test]
    fn inverse_rounding_can_overflow_at_i32_max() {
        // apply(MAX) with 2/3 lands on a value whose ties-away inverse is
        // MAX + 0.5, which does not fit i32. MIN is representable because
        // the matching negative half-quantum is i32::MIN itself.
        let t = transform(2, 0, 3);
        let applied_max = t.apply(i32::MAX).unwrap();
        assert_eq!(t.unapply(applied_max), Err(AffineOverflow::Overflow));
        let applied_min = t.apply(i32::MIN).unwrap();
        assert_eq!(t.unapply(applied_min), Ok(i32::MIN));
    }

    #[test]
    fn expanding_round_trips_stay_within_the_bound() {
        let t = transform(3, 0, 2);
        let bound = reconstruction_bound(3, 2);
        for y in -2_000..=2_000 {
            let Ok(applied) = t.apply(y) else {
                continue;
            };
            let recovered = t.unapply(applied).unwrap();
            let error = i64::from(recovered) - i64::from(y);
            assert!(
                error.abs() <= bound,
                "y={y}: apply -> {applied} -> unapply -> {recovered} (Δ={error}, bound={bound})"
            );
        }
    }
}
