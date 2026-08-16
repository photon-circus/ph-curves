//! Shared integer rounding used by transfer and stabilization paths.
//!
//! Transfer interpolation (forward and inverse), affine calibration, and the
//! temporal filters all round the same way — nearest, ties away from zero —
//! via [`div_nearest_ties_away`]. Curve LUT lookup does not use this helper;
//! it has its own quantization path.

/// Divide with nearest rounding, ties away from zero.
///
/// The sign of the result is the sign of `numerator / denominator`, so a
/// negative `denominator` flips the sense. Magnitudes are computed in `u64`,
/// which keeps every `i64` numerator representable without a separate checked
/// path.
///
/// # Panics
///
/// Debug builds assert that `denominator` is nonzero, and that the quotient
/// is representable. The single unrepresentable case is
/// `numerator == i64::MIN` with `denominator.unsigned_abs() == 1`, where the
/// exact quotient is `2^63` and does not fit `i64`. No call site in this crate
/// can reach it: transfer numerators are products of `u16` and `i32`
/// operands, and the widest stabilization product is `u32::MAX * u16::MAX`
/// from the exponential smoother, still far below `i64::MIN`. Release builds
/// return a wrapped value rather than aborting on the audio- and control-path
/// callers.
pub(crate) const fn div_nearest_ties_away(numerator: i64, denominator: i64) -> i64 {
    debug_assert!(denominator != 0);

    let numerator_abs = numerator.unsigned_abs();
    let denominator_abs = denominator.unsigned_abs();

    // `numerator_abs <= 2^63` and `denominator_abs / 2 < 2^63`, so the sum
    // cannot overflow `u64`.
    let quotient = (numerator_abs + denominator_abs / 2) / denominator_abs;
    debug_assert!(quotient <= i64::MAX as u64);

    if (numerator < 0) == (denominator < 0) {
        quotient as i64
    } else {
        (quotient as i64).wrapping_neg()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ties_round_away_from_zero_in_every_sign_combination() {
        assert_eq!(div_nearest_ties_away(1, 2), 1);
        assert_eq!(div_nearest_ties_away(-1, 2), -1);
        assert_eq!(div_nearest_ties_away(1, -2), -1);
        assert_eq!(div_nearest_ties_away(-1, -2), 1);
        assert_eq!(div_nearest_ties_away(3, 2), 2);
        assert_eq!(div_nearest_ties_away(-3, 2), -2);
        assert_eq!(div_nearest_ties_away(3, -2), -2);
        assert_eq!(div_nearest_ties_away(-3, -2), 2);
    }

    #[test]
    fn non_ties_round_to_nearest() {
        assert_eq!(div_nearest_ties_away(1, 3), 0);
        assert_eq!(div_nearest_ties_away(2, 3), 1);
        assert_eq!(div_nearest_ties_away(-1, 3), 0);
        assert_eq!(div_nearest_ties_away(-2, 3), -1);
        assert_eq!(div_nearest_ties_away(0, 7), 0);
    }

    #[test]
    fn exact_quotients_are_unchanged() {
        assert_eq!(div_nearest_ties_away(10, 5), 2);
        assert_eq!(div_nearest_ties_away(-10, 5), -2);
        assert_eq!(div_nearest_ties_away(10, -5), -2);
        assert_eq!(div_nearest_ties_away(-10, -5), 2);
    }

    #[test]
    fn wide_operands_stay_exact() {
        // Widest product the transfer layer can build: i32 range times u16 span.
        let numerator = i64::from(i32::MIN) * i64::from(u16::MAX);
        assert_eq!(
            div_nearest_ties_away(numerator, i64::from(u16::MAX)),
            i64::from(i32::MIN)
        );

        // Widest affine numerator: i32 * i32 + i32.
        let numerator = i64::from(i32::MAX) * i64::from(i32::MAX) + i64::from(i32::MAX);
        assert_eq!(div_nearest_ties_away(numerator, 1), numerator);
        assert_eq!(div_nearest_ties_away(numerator, -1), -numerator);
    }

    #[test]
    fn matches_the_previous_per_module_implementations() {
        // The stabilize filters only ever divided by a positive denominator.
        fn legacy_stabilize(numerator: i64, denominator: i64) -> i64 {
            if numerator >= 0 {
                (numerator + denominator / 2) / denominator
            } else {
                -((-numerator + denominator / 2) / denominator)
            }
        }

        for numerator in -1_000i64..=1_000 {
            for denominator in [1i64, 2, 3, 7, 255, 65_535] {
                assert_eq!(
                    div_nearest_ties_away(numerator, denominator),
                    legacy_stabilize(numerator, denominator),
                    "{numerator} / {denominator}"
                );
            }
        }
    }
}
