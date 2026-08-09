//! Integer-only transfer functions for physical measurements.
//!
//! Transfer functions map an integer observation, such as an ADC code, to a
//! signed measurement value in a declared scale. Unlike normalized curves,
//! they are not coupled to [`crate::UnitValue`] or tickless scheduling.

/// A conversion from an observation to a measurement.
pub trait TransferFunction {
    /// Input observation type.
    type Input: Copy;
    /// Output measurement type.
    type Output: Copy;

    /// Convert an observation to a measurement.
    ///
    /// Inputs outside the table domain follow the table's explicit lower and
    /// upper [`BoundaryBehavior`] settings. Transfer functions never
    /// extrapolate.
    fn convert(&self, input: Self::Input) -> Result<Self::Output, TransferError<Self::Input>>;
}

/// Monotonic direction of a transfer function's output.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum MonotonicDirection {
    /// Output values do not decrease as input increases.
    Increasing,
    /// Output values do not increase as input increases.
    Decreasing,
}

/// Behavior for observations outside one side of a transfer domain.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum BoundaryBehavior {
    /// Return a [`TransferError`].
    Error,
    /// Return the nearest endpoint value.
    Clamp,
}

/// Error returned when an observation is outside a transfer domain, or when
/// affine calibration arithmetic cannot be represented.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum TransferError<I> {
    /// The observation is below the minimum supported input.
    BelowDomain {
        /// Observation supplied by the caller.
        input: I,
        /// Smallest supported input.
        minimum: I,
    },
    /// The observation is above the maximum supported input.
    AboveDomain {
        /// Observation supplied by the caller.
        input: I,
        /// Largest supported input.
        maximum: I,
    },
    /// Affine calibration overflowed `i64` intermediates or the final `i32`
    /// result. Domain policy from the inner transfer is unchanged.
    Overflow,
}

/// Error returned for invalid standalone segment interpolation arguments.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum InterpolationError {
    /// Segment inputs are not strictly increasing.
    InvalidSpan,
    /// The interpolation input is outside the closed segment.
    OutsideSegment {
        /// Observation supplied by the caller.
        input: u16,
        /// Segment's lower input.
        minimum: u16,
        /// Segment's upper input.
        maximum: u16,
    },
}

/// Compact facts recorded by the host generator for a transfer table.
///
/// The error fields describe numerical table and output-quantization error
/// against the configured ideal source. They do not include sensor, component,
/// ADC, model, self-heating, or calibration uncertainty.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct TransferMetadata {
    /// Human-readable input unit, such as `"adc_code"` or `"millivolt"`.
    pub input_unit: &'static str,
    /// Human-readable physical output unit, such as `"degree_celsius"`.
    pub output_unit: &'static str,
    /// Integer output quanta per physical output unit.
    pub output_scale: u32,
    /// Inclusive minimum input.
    pub domain_min: u16,
    /// Inclusive maximum input.
    pub domain_max: u16,
    /// Output monotonic direction.
    pub direction: MonotonicDirection,
    /// Number of piecewise-linear knots.
    pub knot_count: usize,
    /// Requested maximum numerical error in output quanta.
    pub requested_max_error: u32,
    /// Exhaustively measured, conservatively rounded-up maximum error.
    pub achieved_max_error: u32,
    /// Input where the achieved maximum error first occurs.
    pub worst_case_input: u16,
}

/// A sparse, nonuniform, piecewise-linear `u16` to `i32` transfer function.
///
/// Inputs are searched in `O(log N)` time. The two arrays use six bytes of
/// table payload per knot and require no allocation.
#[derive(Copy, Clone, Debug)]
pub struct PiecewiseLinearTransfer<const N: usize> {
    inputs: &'static [u16; N],
    outputs: &'static [i32; N],
    direction: MonotonicDirection,
    below: BoundaryBehavior,
    above: BoundaryBehavior,
}

impl<const N: usize> PiecewiseLinearTransfer<N> {
    /// Construct a transfer whose below/above behaviors both default to error.
    ///
    /// # Panics
    ///
    /// Panics while defining the table if it has fewer than two knots, inputs
    /// are not strictly increasing, or outputs violate `direction`. Generated
    /// tables call this in a constant context, making invalid tables a compile
    /// error. Conversion of caller-supplied observations does not panic.
    pub const fn new(
        inputs: &'static [u16; N],
        outputs: &'static [i32; N],
        direction: MonotonicDirection,
    ) -> Self {
        assert!(N >= 2);

        let mut index = 1;
        while index < N {
            assert!(inputs[index] > inputs[index - 1]);
            match direction {
                MonotonicDirection::Increasing => {
                    assert!(outputs[index] >= outputs[index - 1]);
                }
                MonotonicDirection::Decreasing => {
                    assert!(outputs[index] <= outputs[index - 1]);
                }
            }
            index += 1;
        }

        Self {
            inputs,
            outputs,
            direction,
            below: BoundaryBehavior::Error,
            above: BoundaryBehavior::Error,
        }
    }

    /// Set independent below-domain and above-domain behavior.
    pub const fn with_boundaries(
        mut self,
        below: BoundaryBehavior,
        above: BoundaryBehavior,
    ) -> Self {
        self.below = below;
        self.above = above;
        self
    }

    /// Return the input knot array.
    pub const fn inputs(&self) -> &'static [u16; N] {
        self.inputs
    }

    /// Return the output knot array.
    pub const fn outputs(&self) -> &'static [i32; N] {
        self.outputs
    }

    /// Return the output monotonic direction.
    pub const fn direction(&self) -> MonotonicDirection {
        self.direction
    }

    /// Return the below-domain behavior.
    pub const fn below_behavior(&self) -> BoundaryBehavior {
        self.below
    }

    /// Return the above-domain behavior.
    pub const fn above_behavior(&self) -> BoundaryBehavior {
        self.above
    }

    /// Return the inclusive input domain.
    pub const fn domain(&self) -> (u16, u16) {
        (self.inputs[0], self.inputs[N - 1])
    }
}

impl<const N: usize> TransferFunction for PiecewiseLinearTransfer<N> {
    type Input = u16;
    type Output = i32;

    fn convert(&self, input: u16) -> Result<i32, TransferError<u16>> {
        let minimum = self.inputs[0];
        let maximum = self.inputs[N - 1];

        if input < minimum {
            return match self.below {
                BoundaryBehavior::Error => Err(TransferError::BelowDomain { input, minimum }),
                BoundaryBehavior::Clamp => Ok(self.outputs[0]),
            };
        }
        if input > maximum {
            return match self.above {
                BoundaryBehavior::Error => Err(TransferError::AboveDomain { input, maximum }),
                BoundaryBehavior::Clamp => Ok(self.outputs[N - 1]),
            };
        }
        if input == minimum {
            return Ok(self.outputs[0]);
        }
        if input == maximum {
            return Ok(self.outputs[N - 1]);
        }

        let mut low = 0usize;
        let mut high = N - 1;
        while low + 1 < high {
            let middle = low + (high - low) / 2;
            match input.cmp(&self.inputs[middle]) {
                core::cmp::Ordering::Less => high = middle,
                core::cmp::Ordering::Equal => return Ok(self.outputs[middle]),
                core::cmp::Ordering::Greater => low = middle,
            }
        }

        Ok(interpolate_valid_segment(
            input,
            self.inputs[low],
            self.outputs[low],
            self.inputs[high],
            self.outputs[high],
        ))
    }
}

/// Interpolate one signed integer segment with nearest, ties-away rounding.
///
/// The input must lie within the closed segment and `x1` must be greater than
/// `x0`. All arithmetic uses `i64`; the full `u16`/`i32` ranges are safe.
pub fn interpolate_segment(
    input: u16,
    x0: u16,
    y0: i32,
    x1: u16,
    y1: i32,
) -> Result<i32, InterpolationError> {
    if x1 <= x0 {
        return Err(InterpolationError::InvalidSpan);
    }
    if input < x0 || input > x1 {
        return Err(InterpolationError::OutsideSegment {
            input,
            minimum: x0,
            maximum: x1,
        });
    }
    Ok(interpolate_valid_segment(input, x0, y0, x1, y1))
}

fn interpolate_valid_segment(input: u16, x0: u16, y0: i32, x1: u16, y1: i32) -> i32 {
    let offset = i64::from(input - x0);
    let span = i64::from(x1 - x0);
    let delta = i64::from(y1) - i64::from(y0);
    let numerator = i64::from(y0) * span + delta * offset;
    let result = if numerator >= 0 {
        (numerator + span / 2) / span
    } else {
        -((-numerator + span / 2) / span)
    };
    debug_assert!((i64::from(i32::MIN)..=i64::from(i32::MAX)).contains(&result));
    result as i32
}

/// Runtime/factory gain-and-offset wrapper around an inner transfer.
///
/// Applies `y' = (y * gain + offset) / scale` with `i64` intermediates and
/// nearest, ties-away-from-zero rounding. The caller supplies the integer
/// triple (for example from EEPROM or flash); this type never reads NVM,
/// regenerates knot tables, or updates [`TransferMetadata`].
///
/// Identity (modulo rounding when `|scale| ≠ 1`) is `gain = scale` and
/// `offset = 0`. Negative `scale` is allowed and flips sense. Nesting multiple
/// wrappers is permitted via [`TransferFunction`], but precision loss and
/// overflow risk stack with each layer.
///
/// # Numerical scope
///
/// For any `i32` `y`, `gain`, and `offset`, the product/sum
/// `y * gain + offset` always fits in `i64`. The checked path still returns
/// [`TransferError::Overflow`] if rounding or the final cast cannot be
/// represented in `i32`.
#[derive(Copy, Clone, Debug)]
pub struct AffineCalibration<T> {
    inner: T,
    gain: i32,
    offset: i32,
    scale: i32,
}

impl<T> AffineCalibration<T> {
    /// Wrap `inner` with affine calibration constants.
    ///
    /// # Panics
    ///
    /// Panics if `scale == 0`.
    pub const fn new(inner: T, gain: i32, offset: i32, scale: i32) -> Self {
        assert!(scale != 0);
        Self {
            inner,
            gain,
            offset,
            scale,
        }
    }

    /// Return a reference to the inner transfer.
    pub const fn inner(&self) -> &T {
        &self.inner
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
}

impl<T> TransferFunction for AffineCalibration<T>
where
    T: TransferFunction<Output = i32>,
{
    type Input = T::Input;
    type Output = i32;

    fn convert(&self, input: Self::Input) -> Result<i32, TransferError<Self::Input>> {
        let y = self.inner.convert(input)?;
        apply_affine_i64(y, self.gain, self.offset, self.scale)
    }
}

/// Apply `y' = (y * gain + offset) / scale` with checked `i64` math.
///
/// Rounding is nearest, ties away from zero. `scale` must be nonzero; callers
/// such as [`AffineCalibration::new`] enforce that before invocation.
fn apply_affine_i64<I>(
    y: i32,
    gain: i32,
    offset: i32,
    scale: i32,
) -> Result<i32, TransferError<I>> {
    debug_assert!(scale != 0);

    let product = i64::from(y)
        .checked_mul(i64::from(gain))
        .ok_or(TransferError::Overflow)?;
    let numerator = product
        .checked_add(i64::from(offset))
        .ok_or(TransferError::Overflow)?;
    let scaled = round_div_nearest_checked(numerator, i64::from(scale))?;
    i32::try_from(scaled).map_err(|_| TransferError::Overflow)
}

/// Nearest division with ties away from zero; rejects unrepresentable cases.
fn round_div_nearest_checked<I>(numerator: i64, denominator: i64) -> Result<i64, TransferError<I>> {
    debug_assert!(denominator != 0);

    // Normalize to a positive divisor so ties-away matches interpolate_segment.
    let (numerator, denominator) = if denominator < 0 {
        (
            numerator.checked_neg().ok_or(TransferError::Overflow)?,
            denominator.checked_neg().ok_or(TransferError::Overflow)?,
        )
    } else {
        (numerator, denominator)
    };

    if numerator >= 0 {
        let adjusted = numerator
            .checked_add(denominator / 2)
            .ok_or(TransferError::Overflow)?;
        Ok(adjusted / denominator)
    } else {
        let abs_numerator = numerator.checked_neg().ok_or(TransferError::Overflow)?;
        let adjusted = abs_numerator
            .checked_add(denominator / 2)
            .ok_or(TransferError::Overflow)?;
        (adjusted / denominator)
            .checked_neg()
            .ok_or(TransferError::Overflow)
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;

    static INPUTS: [u16; 3] = [100, 200, 400];
    static OUTPUTS: [i32; 3] = [-1_000, 0, 2_000];
    static DECREASING: [i32; 3] = [2_000, 0, -1_000];

    #[test]
    fn exact_knots_and_binary_search() {
        let transfer =
            PiecewiseLinearTransfer::new(&INPUTS, &OUTPUTS, MonotonicDirection::Increasing);
        assert_eq!(transfer.convert(100), Ok(-1_000));
        assert_eq!(transfer.convert(200), Ok(0));
        assert_eq!(transfer.convert(400), Ok(2_000));
        assert_eq!(transfer.convert(150), Ok(-500));
        assert_eq!(transfer.convert(300), Ok(1_000));
    }

    #[test]
    fn independent_boundary_behavior() {
        let transfer =
            PiecewiseLinearTransfer::new(&INPUTS, &OUTPUTS, MonotonicDirection::Increasing)
                .with_boundaries(BoundaryBehavior::Clamp, BoundaryBehavior::Error);
        assert_eq!(transfer.convert(99), Ok(-1_000));
        assert_eq!(
            transfer.convert(401),
            Err(TransferError::AboveDomain {
                input: 401,
                maximum: 400
            })
        );
    }

    #[test]
    fn decreasing_signed_transfer() {
        let transfer =
            PiecewiseLinearTransfer::new(&INPUTS, &DECREASING, MonotonicDirection::Decreasing);
        assert_eq!(transfer.convert(150), Ok(1_000));
        assert_eq!(transfer.convert(300), Ok(-500));
    }

    #[test]
    fn signed_rounding_ties_away_from_zero() {
        assert_eq!(interpolate_segment(1, 0, 0, 2, 1), Ok(1));
        assert_eq!(interpolate_segment(1, 0, 0, 2, -1), Ok(-1));
        assert_eq!(interpolate_segment(1, 0, -10, 2, -9), Ok(-10));
        assert_eq!(interpolate_segment(1, 0, 10, 2, 9), Ok(10));
        assert_eq!(interpolate_segment(1, 0, 10, 3, 11), Ok(10));
        assert_eq!(interpolate_segment(2, 0, 10, 3, 11), Ok(11));
        assert_eq!(interpolate_segment(1, 0, -10, 3, -11), Ok(-10));
        assert_eq!(interpolate_segment(2, 0, -10, 3, -11), Ok(-11));
    }

    #[test]
    fn full_integer_ranges_are_safe() {
        assert_eq!(
            interpolate_segment(0, 0, i32::MIN, u16::MAX, i32::MAX),
            Ok(i32::MIN)
        );
        assert_eq!(
            interpolate_segment(u16::MAX, 0, i32::MIN, u16::MAX, i32::MAX),
            Ok(i32::MAX)
        );
        assert_eq!(
            interpolate_segment(0, 0, i32::MAX, u16::MAX, i32::MIN),
            Ok(i32::MAX)
        );
        assert_eq!(
            interpolate_segment(u16::MAX, 0, i32::MAX, u16::MAX, i32::MIN),
            Ok(i32::MIN)
        );
    }

    #[test]
    fn exhaustive_full_span_is_bounded_and_monotonic() {
        let mut previous_increasing = i32::MIN;
        let mut previous_decreasing = i32::MAX;
        for input in 0..=u16::MAX {
            let increasing = interpolate_segment(input, 0, i32::MIN, u16::MAX, i32::MAX).unwrap();
            let decreasing = interpolate_segment(input, 0, i32::MAX, u16::MAX, i32::MIN).unwrap();
            assert!(increasing >= previous_increasing);
            assert!(decreasing <= previous_decreasing);
            previous_increasing = increasing;
            previous_decreasing = decreasing;
        }
        assert_eq!(previous_increasing, i32::MAX);
        assert_eq!(previous_decreasing, i32::MIN);
    }

    #[test]
    fn constructor_rejects_invalid_tables() {
        static DUPLICATE_INPUTS: [u16; 2] = [10, 10];
        static DESCENDING_OUTPUTS: [i32; 2] = [10, 0];

        assert!(
            std::panic::catch_unwind(|| {
                PiecewiseLinearTransfer::new(
                    &DUPLICATE_INPUTS,
                    &DESCENDING_OUTPUTS,
                    MonotonicDirection::Decreasing,
                )
            })
            .is_err()
        );
        assert!(
            std::panic::catch_unwind(|| {
                PiecewiseLinearTransfer::new(
                    &[10, 20],
                    &DESCENDING_OUTPUTS,
                    MonotonicDirection::Increasing,
                )
            })
            .is_err()
        );
    }

    #[test]
    fn standalone_interpolation_validates_arguments() {
        assert_eq!(
            interpolate_segment(10, 10, 0, 10, 1),
            Err(InterpolationError::InvalidSpan)
        );
        assert_eq!(
            interpolate_segment(9, 10, 0, 20, 1),
            Err(InterpolationError::OutsideSegment {
                input: 9,
                minimum: 10,
                maximum: 20
            })
        );
    }

    #[test]
    fn affine_identity_and_factory_scale() {
        let base = PiecewiseLinearTransfer::new(&INPUTS, &OUTPUTS, MonotonicDirection::Increasing);
        let identity = AffineCalibration::new(base, 1, 0, 1);
        assert_eq!(identity.convert(150), Ok(-500));
        assert_eq!(identity.gain(), 1);
        assert_eq!(identity.offset(), 0);
        assert_eq!(identity.scale(), 1);
        assert_eq!(identity.inner().convert(150), Ok(-500));

        let cal = AffineCalibration::new(base, 1005, -120, 1000);
        // (-500 * 1005 + -120) / 1000 = -502.62 → -503 (ties-away / nearest)
        assert_eq!(cal.convert(150), Ok(-503));
        // (0 * 1005 + -120) / 1000 = -0.12 → 0
        assert_eq!(cal.convert(200), Ok(0));
        // (2000 * 1005 + -120) / 1000 = 2009.88 → 2010
        assert_eq!(cal.convert(400), Ok(2_010));
    }

    #[test]
    fn affine_rounding_ties_away_and_negative_scale() {
        let base = PiecewiseLinearTransfer::new(&INPUTS, &OUTPUTS, MonotonicDirection::Increasing);
        let half_up = AffineCalibration::new(base, 1, 0, 2);
        // 2000 / 2 = 1000 exact; 0 / 2 = 0; -1000 / 2 = -500
        assert_eq!(half_up.convert(400), Ok(1_000));
        assert_eq!(half_up.convert(200), Ok(0));

        // 1/2 → 1 and -1/2 → -1 (ties away from zero)
        assert_eq!(apply_affine_i64::<u16>(1, 1, 0, 2), Ok(1));
        assert_eq!(apply_affine_i64::<u16>(-1, 1, 0, 2), Ok(-1));
        assert_eq!(apply_affine_i64::<u16>(1, 1, 0, -2), Ok(-1));
        assert_eq!(apply_affine_i64::<u16>(-1, 1, 0, -2), Ok(1));
        assert_eq!(apply_affine_i64::<u16>(3, 1, 0, 2), Ok(2));
        assert_eq!(apply_affine_i64::<u16>(-3, 1, 0, 2), Ok(-2));
    }

    #[test]
    fn affine_propagates_domain_errors_and_rejects_overflow() {
        let base = PiecewiseLinearTransfer::new(&INPUTS, &OUTPUTS, MonotonicDirection::Increasing);
        let cal = AffineCalibration::new(base, 1, 0, 1);
        assert_eq!(
            cal.convert(99),
            Err(TransferError::BelowDomain {
                input: 99,
                minimum: 100
            })
        );

        // Large gain maps an in-domain knot outside i32.
        let overflow = AffineCalibration::new(base, i32::MAX, 0, 1);
        assert_eq!(overflow.convert(400), Err(TransferError::Overflow));
        assert_eq!(
            apply_affine_i64::<u16>(i32::MAX, 2, 0, 1),
            Err(TransferError::Overflow)
        );
    }

    #[test]
    fn affine_constructor_rejects_zero_scale() {
        let base = PiecewiseLinearTransfer::new(&INPUTS, &OUTPUTS, MonotonicDirection::Increasing);
        assert!(std::panic::catch_unwind(|| AffineCalibration::new(base, 1, 0, 0)).is_err());
    }

    #[test]
    fn affine_nesting_composes() {
        let base = PiecewiseLinearTransfer::new(&INPUTS, &OUTPUTS, MonotonicDirection::Increasing);
        let inner = AffineCalibration::new(base, 2, 10, 1);
        let outer = AffineCalibration::new(inner, 1, -10, 2);
        // y=0 → (0*2+10)=10 → (10-10)/2 = 0
        assert_eq!(outer.convert(200), Ok(0));
        // y=2000 → 4010 → (4010-10)/2 = 2000
        assert_eq!(outer.convert(400), Ok(2_000));
    }
}
