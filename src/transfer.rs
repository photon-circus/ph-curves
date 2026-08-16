//! Integer-only transfer functions for physical measurements.
//!
//! Transfer functions map an integer observation, such as an ADC code, to a
//! signed measurement value in a declared scale. Unlike normalized curves,
//! they are not coupled to [`crate::UnitValue`] or tickless scheduling.
//!
//! Inverse conversion maps a physical setpoint back to an observation using
//! the same sparse knot tables — no dense physical-domain LUT.

use crate::round::div_nearest_ties_away;

/// A conversion from an observation to a measurement.
pub trait TransferFunction {
    /// Input observation type.
    type Input: Copy;
    /// Output measurement type.
    type Output: Copy;

    /// Convert an observation to a measurement.
    ///
    /// When a table declares an [`ObservationGuard`], that exact code is
    /// classified first. Remaining inputs outside the table domain follow the
    /// table's explicit lower and upper [`BoundaryBehavior`] settings.
    /// Transfer functions never extrapolate.
    fn convert(&self, input: Self::Input) -> Result<Self::Output, TransferError<Self::Input>>;
}

/// A conversion from a physical measurement back to an observation.
///
/// Parallel to [`TransferFunction`]; not a supertrait, because the map
/// direction and error type differ.
pub trait InverseTransferFunction {
    /// Physical measurement type (input to inverse).
    type Physical: Copy;
    /// Observation type (output of inverse).
    type Observation: Copy;

    /// Invert a physical measurement to an observation.
    ///
    /// Values outside the table's physical range follow the same
    /// [`BoundaryBehavior`] settings as the forward direction, mapped through
    /// the table's [`MonotonicDirection`] so both directions agree about the
    /// same out-of-range condition. On a decreasing table the codes above
    /// `domain_max` are the ones producing physical values below `range_min`,
    /// so `above` governs the low-physical side there. Transfer inverses never
    /// extrapolate.
    fn invert(
        &self,
        physical: Self::Physical,
    ) -> Result<Self::Observation, InverseTransferError<Self::Physical>>;
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
///
/// Both settings are declared against the **observation domain**. For
/// [`InverseTransferFunction`] they are mapped onto the physical range through
/// the table's [`MonotonicDirection`], so `below` governs whichever end of the
/// physical range corresponds to inputs under `domain_min` — the low end on an
/// increasing table, the high end on a decreasing one. See
/// [`PiecewiseLinearTransfer::range_behaviors`].
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum BoundaryBehavior {
    /// Return a domain/range error.
    Error,
    /// Return the nearest endpoint value.
    Clamp,
}

/// Policy applied when one explicitly declared observation code is seen.
///
/// Distinct from [`BoundaryBehavior`]: ordinary codes outside the fitted
/// domain follow `below` / `above`, while this policy applies only to the
/// guarded code and is not mapped through [`MonotonicDirection`].
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ObservationGuardBehavior {
    /// Return [`TransferError::RejectedObservation`].
    Error,
    /// Return the output at `domain_max`, regardless of the `above` policy.
    Clamp,
}

/// One explicitly declared observation code and the policy applied to it.
///
/// The code is consumer or device policy, not inferred from its integer
/// value. It must be strictly above the table's fitted `domain_max`.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct ObservationGuard {
    /// Observation code classified by this guard.
    pub code: u16,
    /// Policy applied when `code` is observed.
    pub behavior: ObservationGuardBehavior,
}

/// Compact facts for a transfer's optional observation-code guard.
///
/// Adjacent to [`TransferMetadata`] rather than a required field on it, so
/// generated struct literals for table metadata stay additive. Classification
/// of a code as saturation is declared consumer/device policy, not inferred
/// from the integer value.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct ObservationGuardMetadata {
    /// Observation code classified by this guard.
    pub code: u16,
    /// Policy applied when `code` is observed.
    pub behavior: ObservationGuardBehavior,
}

/// How to resolve a physical value that lands on a flat (non-unique) output run.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum FlatResolution {
    /// Return the smallest input of the flat run.
    #[default]
    PreferLowInput,
    /// Return the largest input of the flat run.
    PreferHighInput,
    /// Return `(low + high) / 2`, truncating toward the low input.
    Midpoint,
    /// Return [`InverseTransferError::AmbiguousFlat`].
    Error,
}

/// Error returned when an observation is outside a transfer domain, a
/// declared observation guard rejects it, or affine calibration arithmetic
/// cannot be represented.
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
    /// The observation matches an explicit [`ObservationGuard`] whose policy
    /// is [`ObservationGuardBehavior::Error`].
    ///
    /// Distinct from [`Self::AboveDomain`]: the code was declared as a
    /// rejected observation, not as an ordinary domain violation.
    RejectedObservation {
        /// Observation supplied by the caller.
        input: I,
    },
    /// Affine calibration overflowed `i64` intermediates or the final `i32`
    /// result. Domain policy from the inner transfer is unchanged.
    Overflow,
}

/// Error returned when a physical value cannot be inverted uniquely.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum InverseTransferError<P> {
    /// The physical value is below the minimum supported output.
    BelowRange {
        /// Physical value supplied by the caller.
        physical: P,
        /// Smallest supported physical output.
        minimum: P,
    },
    /// The physical value is above the maximum supported output.
    AboveRange {
        /// Physical value supplied by the caller.
        physical: P,
        /// Largest supported physical output.
        maximum: P,
    },
    /// The physical value lies on a flat run and [`FlatResolution::Error`] is set.
    AmbiguousFlat {
        /// Physical value supplied by the caller.
        physical: P,
        /// Smallest observation on the flat run.
        low: u16,
        /// Largest observation on the flat run.
        high: u16,
    },
    /// Undoing affine calibration produced a value outside `i32`, or a range
    /// bound could not be re-expressed in calibrated units.
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
    /// Segment outputs are equal, so the segment cannot be inverted uniquely.
    FlatSegment,
    /// The physical value is outside the closed segment's output span.
    OutsidePhysicalSpan {
        /// Physical value supplied by the caller.
        physical: i32,
        /// Segment's lower output.
        minimum: i32,
        /// Segment's upper output.
        maximum: i32,
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
    /// Inclusive minimum physical output (endpoint of the knot range).
    pub range_min: i32,
    /// Inclusive maximum physical output.
    pub range_max: i32,
    /// Output monotonic direction.
    pub direction: MonotonicDirection,
    /// Number of piecewise-linear knots.
    pub knot_count: usize,
    /// True when every adjacent knot pair has unequal outputs.
    pub strictly_monotonic: bool,
    /// Count of adjacent knot pairs with equal outputs.
    pub flat_segment_count: usize,
    /// Requested maximum numerical error in output quanta.
    pub requested_max_error: u32,
    /// Exhaustively measured, conservatively rounded-up maximum error.
    pub achieved_max_error: u32,
    /// Input where the achieved maximum error first occurs.
    pub worst_case_input: u16,
    /// Exhaustively host-measured worst
    /// `|invert(convert(x)) as i32 − x as i32|` over the input domain, in
    /// codes, under the table's default [`FlatResolution`].
    ///
    /// This is a measured round-trip bound, not a promise of identity: a
    /// nonzero value means some observations do not survive a
    /// convert-then-invert cycle exactly. Flat runs are resolved with
    /// [`FlatResolution::PreferLowInput`]; overriding the policy with
    /// [`PiecewiseLinearTransfer::with_flat_resolution`] can exceed this
    /// bound on tables where `flat_segment_count` is nonzero.
    pub achieved_max_inverse_code_error: u16,
}

/// A sparse, nonuniform, piecewise-linear `u16` to `i32` transfer function.
///
/// Inputs are searched in `O(log N)` time. The two arrays use six bytes of
/// table payload per knot and require no allocation. Inverse conversion
/// binary-searches the same output knots — no dense physical→input LUT.
///
/// An optional [`ObservationGuard`] is stored on the table itself. A transfer
/// constructed without one keeps the previous convert/invert behavior; the
/// struct is larger by that optional field.
#[derive(Copy, Clone, Debug)]
pub struct PiecewiseLinearTransfer<const N: usize> {
    inputs: &'static [u16; N],
    outputs: &'static [i32; N],
    direction: MonotonicDirection,
    below: BoundaryBehavior,
    above: BoundaryBehavior,
    flat_resolution: FlatResolution,
    observation_guard: Option<ObservationGuard>,
}

impl<const N: usize> PiecewiseLinearTransfer<N> {
    /// Construct a transfer whose below/above behaviors both default to error.
    ///
    /// Flat runs default to [`FlatResolution::PreferLowInput`].
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
            flat_resolution: FlatResolution::PreferLowInput,
            observation_guard: None,
        }
    }

    /// Set independent below-domain and above-domain behavior.
    ///
    /// Both are declared against the observation domain. Inverse conversion
    /// maps them onto the physical range through the table's direction; see
    /// [`range_behaviors`](Self::range_behaviors).
    pub const fn with_boundaries(
        mut self,
        below: BoundaryBehavior,
        above: BoundaryBehavior,
    ) -> Self {
        self.below = below;
        self.above = above;
        self
    }

    /// Set how flat (equal-output) runs are resolved by [`invert`](InverseTransferFunction::invert).
    pub const fn with_flat_resolution(mut self, policy: FlatResolution) -> Self {
        self.flat_resolution = policy;
        self
    }

    /// Set an explicit observation-code guard independent of `below` / `above`.
    ///
    /// The guarded code is classified before ordinary domain policy and is
    /// not mapped through inverse range behavior. Classification of a code as
    /// saturation is declared consumer/device policy, not inferred from the
    /// integer value.
    ///
    /// # Panics
    ///
    /// Panics while defining the table if `code` is not strictly above the
    /// fitted `domain_max`. Generated tables call this in a constant context,
    /// making an invalid guard a compile error.
    pub const fn with_observation_guard(
        mut self,
        code: u16,
        behavior: ObservationGuardBehavior,
    ) -> Self {
        assert!(
            code > self.inputs[N - 1],
            "observation guard code must be strictly above domain_max"
        );
        self.observation_guard = Some(ObservationGuard { code, behavior });
        self
    }

    /// Return the explicit observation-code guard, if one is set.
    pub const fn observation_guard(&self) -> Option<ObservationGuard> {
        self.observation_guard
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

    /// Return the below-observation-domain behavior.
    ///
    /// Declared against the observation domain, not the physical range. For
    /// the physical-side policies used by inverse conversion, see
    /// [`range_behaviors`](Self::range_behaviors).
    pub const fn below_behavior(&self) -> BoundaryBehavior {
        self.below
    }

    /// Return the above-observation-domain behavior.
    ///
    /// Declared against the observation domain, not the physical range. For
    /// the physical-side policies used by inverse conversion, see
    /// [`range_behaviors`](Self::range_behaviors).
    pub const fn above_behavior(&self) -> BoundaryBehavior {
        self.above
    }

    /// Return the flat-run resolution policy.
    pub const fn flat_resolution(&self) -> FlatResolution {
        self.flat_resolution
    }

    /// Return the inclusive input domain.
    pub const fn domain(&self) -> (u16, u16) {
        (self.inputs[0], self.inputs[N - 1])
    }

    /// Return the inclusive physical output range as `(min, max)`.
    pub const fn physical_range(&self) -> (i32, i32) {
        let first = self.outputs[0];
        let last = self.outputs[N - 1];
        if first <= last {
            (first, last)
        } else {
            (last, first)
        }
    }

    /// Invert a physical measurement to an observation.
    ///
    /// Convenience alias for [`InverseTransferFunction::invert`].
    pub fn invert_physical(&self, physical: i32) -> Result<u16, InverseTransferError<i32>> {
        InverseTransferFunction::invert(self, physical)
    }

    /// Map the domain policies onto the physical range as
    /// `(low_physical, high_physical)`.
    ///
    /// `below` and `above` are declared against the *observation* domain, so
    /// on a decreasing table they swap: the codes above `domain_max` are the
    /// ones that produce physical values below `range_min`. Selecting by
    /// physical side alone would make a table configured
    /// `below = Error, above = Clamp` clamp in the forward direction and error
    /// in the inverse for the very same out-of-range condition.
    pub const fn range_behaviors(&self) -> (BoundaryBehavior, BoundaryBehavior) {
        match self.direction {
            MonotonicDirection::Increasing => (self.below, self.above),
            MonotonicDirection::Decreasing => (self.above, self.below),
        }
    }

    fn observation_at_physical_end(&self, low_physical: bool) -> u16 {
        match (self.direction, low_physical) {
            (MonotonicDirection::Increasing, true) | (MonotonicDirection::Decreasing, false) => {
                self.inputs[0]
            }
            (MonotonicDirection::Increasing, false) | (MonotonicDirection::Decreasing, true) => {
                self.inputs[N - 1]
            }
        }
    }

    fn resolve_flat_run(
        &self,
        physical: i32,
        left: usize,
        right: usize,
    ) -> Result<u16, InverseTransferError<i32>> {
        let low = self.inputs[left];
        let high = self.inputs[right];
        if left == right {
            return Ok(low);
        }
        match self.flat_resolution {
            FlatResolution::PreferLowInput => Ok(low),
            FlatResolution::PreferHighInput => Ok(high),
            FlatResolution::Midpoint => Ok(low + (high - low) / 2),
            FlatResolution::Error => Err(InverseTransferError::AmbiguousFlat {
                physical,
                low,
                high,
            }),
        }
    }

    fn expand_flat_run(&self, index: usize) -> (usize, usize) {
        let value = self.outputs[index];
        let mut left = index;
        while left > 0 && self.outputs[left - 1] == value {
            left -= 1;
        }
        let mut right = index;
        while right + 1 < N && self.outputs[right + 1] == value {
            right += 1;
        }
        (left, right)
    }

    /// Largest knot index whose output is on the inclusive low-physical side
    /// of `physical` for the table's monotonic direction.
    fn largest_index_at_or_past(&self, physical: i32) -> usize {
        let mut low = 0usize;
        let mut high = N - 1;
        while low < high {
            let middle = low + (high - low).div_ceil(2);
            let past = match self.direction {
                MonotonicDirection::Increasing => self.outputs[middle] <= physical,
                MonotonicDirection::Decreasing => self.outputs[middle] >= physical,
            };
            if past {
                low = middle;
            } else {
                high = middle - 1;
            }
        }
        low
    }
}

impl<const N: usize> TransferFunction for PiecewiseLinearTransfer<N> {
    type Input = u16;
    type Output = i32;

    fn convert(&self, input: u16) -> Result<i32, TransferError<u16>> {
        let minimum = self.inputs[0];
        let maximum = self.inputs[N - 1];

        if let Some(guard) = self.observation_guard {
            if input == guard.code {
                return match guard.behavior {
                    ObservationGuardBehavior::Error => {
                        Err(TransferError::RejectedObservation { input })
                    }
                    ObservationGuardBehavior::Clamp => Ok(self.outputs[N - 1]),
                };
            }
        }

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

impl<const N: usize> InverseTransferFunction for PiecewiseLinearTransfer<N> {
    type Physical = i32;
    type Observation = u16;

    fn invert(&self, physical: i32) -> Result<u16, InverseTransferError<i32>> {
        let (minimum, maximum) = self.physical_range();
        let (low_physical, high_physical) = self.range_behaviors();

        if physical < minimum {
            return match low_physical {
                BoundaryBehavior::Error => {
                    Err(InverseTransferError::BelowRange { physical, minimum })
                }
                BoundaryBehavior::Clamp => Ok(self.observation_at_physical_end(true)),
            };
        }
        if physical > maximum {
            return match high_physical {
                BoundaryBehavior::Error => {
                    Err(InverseTransferError::AboveRange { physical, maximum })
                }
                BoundaryBehavior::Clamp => Ok(self.observation_at_physical_end(false)),
            };
        }

        let index = self.largest_index_at_or_past(physical);
        if self.outputs[index] == physical {
            let (left, right) = self.expand_flat_run(index);
            return self.resolve_flat_run(physical, left, right);
        }

        // `index` is the last knot on the inclusive low-physical side, so the
        // bracketing sloped segment is `index .. index + 1`.
        debug_assert!(index + 1 < N);
        Ok(invert_valid_segment(
            physical,
            self.inputs[index],
            self.outputs[index],
            self.inputs[index + 1],
            self.outputs[index + 1],
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

/// Invert one signed integer segment with nearest, ties-away rounding.
///
/// The mirror of [`interpolate_segment`]. `physical` must lie within the
/// closed output span, `x1` must be greater than `x0`, and the segment must
/// not be flat. All arithmetic uses `i64`; the full `u16`/`i32` ranges are
/// safe. Host tools use this so a generated round-trip audit measures the
/// same arithmetic the runtime performs.
pub fn invert_segment(
    physical: i32,
    x0: u16,
    y0: i32,
    x1: u16,
    y1: i32,
) -> Result<u16, InterpolationError> {
    if x1 <= x0 {
        return Err(InterpolationError::InvalidSpan);
    }
    if y0 == y1 {
        return Err(InterpolationError::FlatSegment);
    }
    let (minimum, maximum) = if y0 < y1 { (y0, y1) } else { (y1, y0) };
    if physical < minimum || physical > maximum {
        return Err(InterpolationError::OutsidePhysicalSpan {
            physical,
            minimum,
            maximum,
        });
    }
    Ok(invert_valid_segment(physical, x0, y0, x1, y1))
}

fn interpolate_valid_segment(input: u16, x0: u16, y0: i32, x1: u16, y1: i32) -> i32 {
    let offset = i64::from(input - x0);
    let span = i64::from(x1 - x0);
    let delta = i64::from(y1) - i64::from(y0);
    let numerator = i64::from(y0) * span + delta * offset;
    let result = div_nearest_ties_away(numerator, span);
    debug_assert!((i64::from(i32::MIN)..=i64::from(i32::MAX)).contains(&result));
    result as i32
}

fn invert_valid_segment(physical: i32, x0: u16, y0: i32, x1: u16, y1: i32) -> u16 {
    let dy = i64::from(y1) - i64::from(y0);
    debug_assert!(dy != 0);
    let dx = i64::from(x1) - i64::from(x0);
    let numerator = i64::from(x0) * dy + (i64::from(physical) - i64::from(y0)) * dx;
    let result = div_nearest_ties_away(numerator, dy);
    result.clamp(i64::from(x0), i64::from(x1)) as u16
}

/// Runtime/factory gain-and-offset wrapper around an inner transfer.
///
/// Applies `y' = (y * gain + offset) / scale` with `i64` intermediates and
/// nearest, ties-away-from-zero rounding. The caller supplies the integer
/// triple (for example from EEPROM or flash); this type never reads NVM,
/// regenerates knot tables, or updates [`TransferMetadata`].
///
/// Identity (modulo rounding when `|scale| ≠ 1`) is `gain = scale` and
/// `offset = 0`. A negative `gain` or `scale` is allowed and flips sense.
/// Nesting multiple wrappers is permitted via [`TransferFunction`], but
/// precision loss and overflow risk stack with each layer.
///
/// # Inverse
///
/// When the inner type implements [`InverseTransferFunction`], so does the
/// wrapper: [`invert`](InverseTransferFunction::invert) undoes the affine with
/// `y = (y' * scale - offset) / gain` and then inverts the inner transfer.
/// That is what makes a calibrated setpoint — "which ADC code reads 25 °C
/// *after* this unit's factory calibration?" — a single call.
///
/// Because `gain` must be nonzero for the affine to be invertible,
/// [`AffineCalibration::new`] rejects `gain == 0` outright rather than
/// deferring the failure to `invert`.
///
/// # Numerical scope
///
/// For any `i32` `y`, `gain`, and `offset`, the product/sum
/// `y * gain + offset` always fits in `i64`; the same holds for
/// `y' * scale - offset` on the inverse path. Both directions still report
/// overflow — [`TransferError::Overflow`] forward,
/// [`InverseTransferError::Overflow`] inverse — when the result does not fit
/// `i32`.
///
/// Both directions round, so a convert-then-invert round trip through a
/// calibration is bounded, not exact. A calibration that compresses the
/// physical scale cannot restore what the forward quantization discarded.
/// When that compression would make `unapply` overshoot an inner endpoint
/// for a calibrated value that is still in the forward image,
/// [`invert`](InverseTransferFunction::invert) clamps to the endpoint rather
/// than returning a spurious range error — so `invert(convert(x))` stays
/// in-domain for every in-domain observation `x`.
#[derive(Copy, Clone, Debug)]
pub struct AffineCalibration<T> {
    inner: T,
    gain: i32,
    offset: i32,
    scale: i32,
}

/// Error returned when affine calibration constants are invalid.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum AffineCalibrationError {
    /// The scale divisor is zero.
    ZeroScale,
    /// The gain is zero, which collapses every observation onto one output.
    ZeroGain,
}

impl<T> AffineCalibration<T> {
    /// Wrap `inner` with affine calibration constants.
    ///
    /// # Errors
    ///
    /// Returns [`AffineCalibrationError::ZeroScale`] if `scale == 0`, or
    /// [`AffineCalibrationError::ZeroGain`] if `gain == 0`. A zero gain maps
    /// every observation onto the single value `offset / scale`, discarding
    /// the sensor and leaving the calibration non-invertible.
    pub fn new(
        inner: T,
        gain: i32,
        offset: i32,
        scale: i32,
    ) -> Result<Self, AffineCalibrationError> {
        if scale == 0 {
            return Err(AffineCalibrationError::ZeroScale);
        }
        if gain == 0 {
            return Err(AffineCalibrationError::ZeroGain);
        }
        Ok(Self {
            inner,
            gain,
            offset,
            scale,
        })
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

impl<T> InverseTransferFunction for AffineCalibration<T>
where
    T: InverseTransferFunction<Physical = i32>,
{
    type Physical = i32;
    type Observation = T::Observation;

    /// Undo the calibration, then invert the inner transfer.
    ///
    /// Range errors from the inner transfer are re-expressed in calibrated
    /// units, so `minimum` / `maximum` are directly comparable with the value
    /// the caller passed in. A calibration with `gain` and `scale` of opposite
    /// signs reverses orientation, which turns an inner `BelowRange` into an
    /// `AboveRange` and vice versa.
    ///
    /// When `|scale| > |gain|`, undoing the affine can land just outside the
    /// inner physical range even though `physical` is in the forward image of
    /// that range (the classic `invert(convert(endpoint))` compression case).
    /// Those values are clamped to the inner endpoint before the second
    /// invert attempt so in-domain calibrated setpoints never spuriously
    /// range-error. Values outside the calibrated forward image still report
    /// [`InverseTransferError::BelowRange`] /
    /// [`InverseTransferError::AboveRange`].
    fn invert(&self, physical: i32) -> Result<T::Observation, InverseTransferError<i32>> {
        let uncalibrated = unapply_affine_i64(physical, self.gain, self.offset, self.scale)?;
        match self.inner.invert(uncalibrated) {
            Ok(observation) => Ok(observation),
            Err(error) => self.recover_compressed_endpoint(physical, error),
        }
    }
}

impl<T> AffineCalibration<T> {
    /// True when the calibration preserves the inner transfer's orientation.
    const fn preserves_orientation(&self) -> bool {
        (self.gain > 0) == (self.scale > 0)
    }

    /// Re-express an inner range error in calibrated units.
    ///
    /// `physical` is echoed back unchanged — it is what the caller supplied.
    /// The bound is mapped forward through the affine, and the variant flips
    /// when the calibration reverses orientation.
    fn recalibrate_error(
        &self,
        physical: i32,
        error: InverseTransferError<i32>,
    ) -> InverseTransferError<i32> {
        let (bound, was_low) = match error {
            InverseTransferError::BelowRange { minimum, .. } => (minimum, true),
            InverseTransferError::AboveRange { maximum, .. } => (maximum, false),
            InverseTransferError::AmbiguousFlat { low, high, .. } => {
                return InverseTransferError::AmbiguousFlat {
                    physical,
                    low,
                    high,
                };
            }
            InverseTransferError::Overflow => return InverseTransferError::Overflow,
        };

        let Ok(calibrated) = apply_affine_i64::<i32>(bound, self.gain, self.offset, self.scale)
        else {
            return InverseTransferError::Overflow;
        };

        if was_low == self.preserves_orientation() {
            InverseTransferError::BelowRange {
                physical,
                minimum: calibrated,
            }
        } else {
            InverseTransferError::AboveRange {
                physical,
                maximum: calibrated,
            }
        }
    }

    /// If compression/rounding pushed `unapply` past an inner endpoint while
    /// `physical` is still inside the forward image, clamp to that endpoint
    /// and retry; otherwise surface the recalibrated range error.
    fn recover_compressed_endpoint(
        &self,
        physical: i32,
        error: InverseTransferError<i32>,
    ) -> Result<T::Observation, InverseTransferError<i32>>
    where
        T: InverseTransferFunction<Physical = i32>,
    {
        let bound = match error {
            InverseTransferError::BelowRange { minimum, .. } => minimum,
            InverseTransferError::AboveRange { maximum, .. } => maximum,
            other => return Err(self.recalibrate_error(physical, other)),
        };

        let calibrated_error = self.recalibrate_error(physical, error);
        let inside_forward_image = match calibrated_error {
            InverseTransferError::BelowRange { minimum, .. } => physical >= minimum,
            InverseTransferError::AboveRange { maximum, .. } => physical <= maximum,
            _ => false,
        };

        if !inside_forward_image {
            return Err(calibrated_error);
        }

        self.inner
            .invert(bound)
            .map_err(|retry_error| self.recalibrate_error(physical, retry_error))
    }
}

/// Apply `y' = (y * gain + offset) / scale` with checked `i64` math.
///
/// Rounding is nearest, ties away from zero. `scale` must be nonzero; callers
/// such as [`AffineCalibration::new`] validate that before invocation.
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
    let scaled = div_nearest_ties_away(numerator, i64::from(scale));
    i32::try_from(scaled).map_err(|_| TransferError::Overflow)
}

/// Undo `y' = (y * gain + offset) / scale`, recovering `y`.
///
/// Solves `y = (y' * scale - offset) / gain` with the same nearest,
/// ties-away rounding. `gain` and `scale` must both be nonzero;
/// [`AffineCalibration::new`] enforces that.
///
/// Both directions round, so `unapply(apply(y))` is bounded rather than exact:
/// a calibration that compresses the physical scale cannot restore what the
/// forward quantization discarded.
fn unapply_affine_i64(
    calibrated: i32,
    gain: i32,
    offset: i32,
    scale: i32,
) -> Result<i32, InverseTransferError<i32>> {
    debug_assert!(gain != 0 && scale != 0);

    // `|calibrated * scale|` is at most `2^62`, so neither step can overflow
    // `i64` for any `i32` operands.
    let product = i64::from(calibrated)
        .checked_mul(i64::from(scale))
        .ok_or(InverseTransferError::Overflow)?;
    let numerator = product
        .checked_sub(i64::from(offset))
        .ok_or(InverseTransferError::Overflow)?;
    let uncalibrated = div_nearest_ties_away(numerator, i64::from(gain));
    i32::try_from(uncalibrated).map_err(|_| InverseTransferError::Overflow)
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;

    static INPUTS: [u16; 3] = [100, 200, 400];
    static OUTPUTS: [i32; 3] = [-1_000, 0, 2_000];
    static DECREASING: [i32; 3] = [2_000, 0, -1_000];
    static FLAT_OUTPUTS: [i32; 4] = [0, 10, 10, 20];
    static FLAT_INPUTS: [u16; 4] = [0, 10, 20, 30];

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
    fn observation_guard_error_overrides_above_clamp() {
        let transfer =
            PiecewiseLinearTransfer::new(&INPUTS, &OUTPUTS, MonotonicDirection::Increasing)
                .with_boundaries(BoundaryBehavior::Error, BoundaryBehavior::Clamp)
                .with_observation_guard(65_535, ObservationGuardBehavior::Error);
        assert_eq!(
            transfer.observation_guard(),
            Some(ObservationGuard {
                code: 65_535,
                behavior: ObservationGuardBehavior::Error,
            })
        );
        assert_eq!(
            transfer.convert(65_535),
            Err(TransferError::RejectedObservation { input: 65_535 })
        );
        assert_eq!(transfer.convert(401), Ok(2_000));
        assert_eq!(transfer.convert(400), Ok(2_000));
        assert_eq!(transfer.convert(200), Ok(0));
    }

    #[test]
    fn observation_guard_clamp_overrides_above_error() {
        let transfer =
            PiecewiseLinearTransfer::new(&INPUTS, &OUTPUTS, MonotonicDirection::Increasing)
                .with_boundaries(BoundaryBehavior::Error, BoundaryBehavior::Error)
                .with_observation_guard(65_535, ObservationGuardBehavior::Clamp);
        assert_eq!(transfer.convert(65_535), Ok(2_000));
        assert_eq!(
            transfer.convert(401),
            Err(TransferError::AboveDomain {
                input: 401,
                maximum: 400
            })
        );
    }

    #[test]
    fn observation_guard_absent_leaves_above_policy() {
        let clamped =
            PiecewiseLinearTransfer::new(&INPUTS, &OUTPUTS, MonotonicDirection::Increasing)
                .with_boundaries(BoundaryBehavior::Clamp, BoundaryBehavior::Clamp);
        assert_eq!(clamped.observation_guard(), None);
        assert_eq!(clamped.convert(65_535), Ok(2_000));
        assert_eq!(clamped.convert(401), Ok(2_000));

        let errored =
            PiecewiseLinearTransfer::new(&INPUTS, &OUTPUTS, MonotonicDirection::Increasing);
        assert_eq!(
            errored.convert(65_535),
            Err(TransferError::AboveDomain {
                input: 65_535,
                maximum: 400
            })
        );
        assert_eq!(
            errored.convert(401),
            Err(TransferError::AboveDomain {
                input: 401,
                maximum: 400
            })
        );
    }

    #[test]
    fn observation_guard_does_not_change_inverse() {
        let error_above =
            PiecewiseLinearTransfer::new(&INPUTS, &OUTPUTS, MonotonicDirection::Increasing)
                .with_observation_guard(65_535, ObservationGuardBehavior::Error);
        assert_eq!(error_above.invert_physical(2_000), Ok(400));
        assert_eq!(
            error_above.invert_physical(2_001),
            Err(InverseTransferError::AboveRange {
                physical: 2_001,
                maximum: 2_000
            })
        );

        let clamp_above =
            PiecewiseLinearTransfer::new(&INPUTS, &OUTPUTS, MonotonicDirection::Increasing)
                .with_boundaries(BoundaryBehavior::Error, BoundaryBehavior::Clamp)
                .with_observation_guard(65_535, ObservationGuardBehavior::Error);
        assert_eq!(clamp_above.invert_physical(3_000), Ok(400));
    }

    #[test]
    fn observation_guard_rejects_code_inside_or_at_domain() {
        assert!(
            std::panic::catch_unwind(|| {
                PiecewiseLinearTransfer::new(&INPUTS, &OUTPUTS, MonotonicDirection::Increasing)
                    .with_observation_guard(400, ObservationGuardBehavior::Error);
            })
            .is_err()
        );
        assert!(
            std::panic::catch_unwind(|| {
                PiecewiseLinearTransfer::new(&INPUTS, &OUTPUTS, MonotonicDirection::Increasing)
                    .with_observation_guard(200, ObservationGuardBehavior::Clamp);
            })
            .is_err()
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
    fn standalone_inversion_validates_arguments() {
        assert_eq!(
            invert_segment(0, 10, 0, 10, 1),
            Err(InterpolationError::InvalidSpan)
        );
        assert_eq!(
            invert_segment(5, 10, 7, 20, 7),
            Err(InterpolationError::FlatSegment)
        );
        assert_eq!(
            invert_segment(21, 10, 0, 20, 20),
            Err(InterpolationError::OutsidePhysicalSpan {
                physical: 21,
                minimum: 0,
                maximum: 20
            })
        );
        // A decreasing segment reports its span low-to-high.
        assert_eq!(
            invert_segment(25, 10, 20, 20, 0),
            Err(InterpolationError::OutsidePhysicalSpan {
                physical: 25,
                minimum: 0,
                maximum: 20
            })
        );
    }

    #[test]
    fn affine_identity_and_factory_scale() {
        let base = PiecewiseLinearTransfer::new(&INPUTS, &OUTPUTS, MonotonicDirection::Increasing);
        let identity = AffineCalibration::new(base, 1, 0, 1).unwrap();
        assert_eq!(identity.convert(150), Ok(-500));
        assert_eq!(identity.gain(), 1);
        assert_eq!(identity.offset(), 0);
        assert_eq!(identity.scale(), 1);
        assert_eq!(identity.inner().convert(150), Ok(-500));

        let cal = AffineCalibration::new(base, 1005, -120, 1000).unwrap();
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
        let half_up = AffineCalibration::new(base, 1, 0, 2).unwrap();
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
        let cal = AffineCalibration::new(base, 1, 0, 1).unwrap();
        assert_eq!(
            cal.convert(99),
            Err(TransferError::BelowDomain {
                input: 99,
                minimum: 100
            })
        );

        let guarded =
            PiecewiseLinearTransfer::new(&INPUTS, &OUTPUTS, MonotonicDirection::Increasing)
                .with_observation_guard(65_535, ObservationGuardBehavior::Error);
        let guarded_cal = AffineCalibration::new(guarded, 1, 0, 1).unwrap();
        assert_eq!(
            guarded_cal.convert(65_535),
            Err(TransferError::RejectedObservation { input: 65_535 })
        );

        // Large gain maps an in-domain knot outside i32.
        let overflow = AffineCalibration::new(base, i32::MAX, 0, 1).unwrap();
        assert_eq!(overflow.convert(400), Err(TransferError::Overflow));
        assert_eq!(
            apply_affine_i64::<u16>(i32::MAX, 2, 0, 1),
            Err(TransferError::Overflow)
        );
    }

    #[test]
    fn standalone_inversion_matches_the_table_path() {
        // Same knots the increasing fixture table uses for its first segment.
        for physical in -1_000..=0 {
            assert_eq!(
                invert_segment(physical, 100, -1_000, 200, 0),
                Ok(invert_valid_segment(physical, 100, -1_000, 200, 0)),
                "physical {physical}"
            );
        }
        assert_eq!(invert_segment(-500, 100, -1_000, 200, 0), Ok(150));
        assert_eq!(invert_segment(1_000, 200, 0, 400, 2_000), Ok(300));
    }

    #[test]
    fn invert_exact_knots_and_midpoints() {
        let transfer =
            PiecewiseLinearTransfer::new(&INPUTS, &OUTPUTS, MonotonicDirection::Increasing);
        assert_eq!(transfer.invert_physical(-1_000), Ok(100));
        assert_eq!(transfer.invert_physical(0), Ok(200));
        assert_eq!(transfer.invert_physical(2_000), Ok(400));
        assert_eq!(transfer.invert_physical(-500), Ok(150));
        assert_eq!(transfer.invert_physical(1_000), Ok(300));
    }

    #[test]
    fn invert_decreasing_maps_by_physical_range() {
        let transfer =
            PiecewiseLinearTransfer::new(&INPUTS, &DECREASING, MonotonicDirection::Decreasing)
                .with_boundaries(BoundaryBehavior::Clamp, BoundaryBehavior::Clamp);
        assert_eq!(transfer.invert_physical(1_000), Ok(150));
        assert_eq!(transfer.invert_physical(-500), Ok(300));
        // Below physical min (-1000) clamps to the high-input endpoint.
        assert_eq!(transfer.invert_physical(-2_000), Ok(400));
        // Above physical max (2000) clamps to the low-input endpoint.
        assert_eq!(transfer.invert_physical(3_000), Ok(100));
    }

    #[test]
    fn boundary_policy_agrees_between_directions_on_a_decreasing_table() {
        // NTC-shaped: decreasing, error under domain_min, clamp over domain_max.
        let transfer =
            PiecewiseLinearTransfer::new(&INPUTS, &DECREASING, MonotonicDirection::Decreasing)
                .with_boundaries(BoundaryBehavior::Error, BoundaryBehavior::Clamp);

        // `above` governs codes over domain_max, which are exactly the codes
        // producing physical values under range_min. Both directions clamp.
        assert_eq!(transfer.convert(500), Ok(-1_000));
        assert_eq!(transfer.invert_physical(-2_000), Ok(400));

        // `below` governs codes under domain_min, which produce physical
        // values over range_max. Both directions error.
        assert_eq!(
            transfer.convert(99),
            Err(TransferError::BelowDomain {
                input: 99,
                minimum: 100
            })
        );
        assert_eq!(
            transfer.invert_physical(3_000),
            Err(InverseTransferError::AboveRange {
                physical: 3_000,
                maximum: 2_000
            })
        );

        assert_eq!(
            transfer.range_behaviors(),
            (BoundaryBehavior::Clamp, BoundaryBehavior::Error)
        );
    }

    #[test]
    fn boundary_policy_is_unswapped_on_an_increasing_table() {
        let transfer =
            PiecewiseLinearTransfer::new(&INPUTS, &OUTPUTS, MonotonicDirection::Increasing)
                .with_boundaries(BoundaryBehavior::Error, BoundaryBehavior::Clamp);

        assert_eq!(
            transfer.range_behaviors(),
            (BoundaryBehavior::Error, BoundaryBehavior::Clamp)
        );
        assert_eq!(transfer.convert(500), Ok(2_000));
        assert_eq!(transfer.invert_physical(3_000), Ok(400));
        assert_eq!(
            transfer.invert_physical(-2_000),
            Err(InverseTransferError::BelowRange {
                physical: -2_000,
                minimum: -1_000
            })
        );
    }

    #[test]
    fn invert_range_errors_use_physical_bounds() {
        let transfer =
            PiecewiseLinearTransfer::new(&INPUTS, &OUTPUTS, MonotonicDirection::Increasing);
        assert_eq!(
            transfer.invert(-1_001),
            Err(InverseTransferError::BelowRange {
                physical: -1_001,
                minimum: -1_000
            })
        );
        assert_eq!(
            transfer.invert(2_001),
            Err(InverseTransferError::AboveRange {
                physical: 2_001,
                maximum: 2_000
            })
        );
    }

    #[test]
    fn flat_resolution_policies() {
        let base = PiecewiseLinearTransfer::new(
            &FLAT_INPUTS,
            &FLAT_OUTPUTS,
            MonotonicDirection::Increasing,
        );

        assert_eq!(
            base.with_flat_resolution(FlatResolution::PreferLowInput)
                .invert(10),
            Ok(10)
        );
        assert_eq!(
            base.with_flat_resolution(FlatResolution::PreferHighInput)
                .invert(10),
            Ok(20)
        );
        assert_eq!(
            base.with_flat_resolution(FlatResolution::Midpoint)
                .invert(10),
            Ok(15)
        );
        assert_eq!(
            base.with_flat_resolution(FlatResolution::Error).invert(10),
            Err(InverseTransferError::AmbiguousFlat {
                physical: 10,
                low: 10,
                high: 20
            })
        );
        // Unique knot: FlatResolution::Error is unused.
        assert_eq!(
            base.with_flat_resolution(FlatResolution::Error).invert(0),
            Ok(0)
        );
        assert_eq!(base.invert(5), Ok(5));
    }

    #[test]
    fn invert_segment_rounding_ties_away() {
        // Forward: input 1 on [0,2] with y 0→1 rounds to 1.
        // Inverse of that physical should prefer the observation side of the tie.
        assert_eq!(invert_valid_segment(1, 0, 0, 2, 2), 1);
        assert_eq!(invert_valid_segment(-1, 0, 0, 2, -2), 1);
        // Half-quantum ties away from zero along the input axis from x0.
        assert_eq!(invert_valid_segment(1, 0, 0, 4, 2), 2);
        assert_eq!(invert_valid_segment(-1, 0, 0, 4, -2), 2);
    }

    #[test]
    fn round_trip_invert_convert_on_non_flat_table() {
        let transfer =
            PiecewiseLinearTransfer::new(&INPUTS, &OUTPUTS, MonotonicDirection::Increasing);
        for input in INPUTS[0]..=INPUTS[INPUTS.len() - 1] {
            let physical = transfer.convert(input).unwrap();
            let recovered = transfer.invert(physical).unwrap();
            let distance = i32::from(recovered).abs_diff(i32::from(input));
            assert!(
                distance <= 1,
                "input {input}: invert(convert) -> {recovered} (Δ={distance})"
            );
        }
    }

    #[test]
    fn div_nearest_ties_away_matches_forward_policy() {
        assert_eq!(div_nearest_ties_away(1, 2), 1);
        assert_eq!(div_nearest_ties_away(-1, 2), -1);
        assert_eq!(div_nearest_ties_away(1, -2), -1);
        assert_eq!(div_nearest_ties_away(-1, -2), 1);
        assert_eq!(div_nearest_ties_away(3, 2), 2);
        assert_eq!(div_nearest_ties_away(-3, 2), -2);
    }

    #[test]
    fn affine_constructor_returns_error_for_zero_scale() {
        let base = PiecewiseLinearTransfer::new(&INPUTS, &OUTPUTS, MonotonicDirection::Increasing);
        assert!(matches!(
            AffineCalibration::new(base, 1, 0, 0),
            Err(AffineCalibrationError::ZeroScale)
        ));
    }

    #[test]
    fn affine_nesting_composes() {
        let base = PiecewiseLinearTransfer::new(&INPUTS, &OUTPUTS, MonotonicDirection::Increasing);
        let inner = AffineCalibration::new(base, 2, 10, 1).unwrap();
        let outer = AffineCalibration::new(inner, 1, -10, 2).unwrap();
        // y=0 → (0*2+10)=10 → (10-10)/2 = 0
        assert_eq!(outer.convert(200), Ok(0));
        // y=2000 → 4010 → (4010-10)/2 = 2000
        assert_eq!(outer.convert(400), Ok(2_000));
    }

    #[test]
    fn affine_constructor_returns_error_for_zero_gain() {
        let base = PiecewiseLinearTransfer::new(&INPUTS, &OUTPUTS, MonotonicDirection::Increasing);
        assert!(matches!(
            AffineCalibration::new(base, 0, 5, 1),
            Err(AffineCalibrationError::ZeroGain)
        ));
    }

    #[test]
    fn calibrated_inverse_round_trips_through_the_table() {
        let base = PiecewiseLinearTransfer::new(&INPUTS, &OUTPUTS, MonotonicDirection::Increasing);
        let cal = AffineCalibration::new(base, 1_005, -120, 1_000).unwrap();

        // Every in-domain code survives convert-then-invert on this table.
        for code in INPUTS[0]..=INPUTS[INPUTS.len() - 1] {
            let calibrated = cal.convert(code).unwrap();
            let recovered = cal.invert(calibrated).unwrap();
            assert!(
                recovered.abs_diff(code) <= 1,
                "code {code}: convert -> {calibrated} -> invert -> {recovered}"
            );
        }
    }

    #[test]
    fn calibrated_inverse_survives_compressing_calibration_at_endpoints() {
        let base = PiecewiseLinearTransfer::new(&INPUTS, &OUTPUTS, MonotonicDirection::Increasing);
        // |scale| > |gain|: unapply expands and can overshoot an inner endpoint
        // even when the calibrated value is exactly convert(endpoint).
        let cal = AffineCalibration::new(base, 2, 0, 3).unwrap();

        let low = cal.convert(100).unwrap();
        assert_eq!(low, -667);
        assert_eq!(cal.invert(low), Ok(100));

        let high = cal.convert(400).unwrap();
        assert_eq!(high, 1_333);
        assert_eq!(cal.invert(high), Ok(400));

        for code in INPUTS[0]..=INPUTS[INPUTS.len() - 1] {
            let calibrated = cal.convert(code).unwrap();
            let recovered = cal.invert(calibrated).unwrap();
            assert!(
                recovered.abs_diff(code) <= 1,
                "code {code}: convert -> {calibrated} -> invert -> {recovered}"
            );
        }

        // Truly outside the calibrated forward image still range-errors.
        assert_eq!(
            cal.invert(-668),
            Err(InverseTransferError::BelowRange {
                physical: -668,
                minimum: -667
            })
        );
        assert_eq!(
            cal.invert(1_334),
            Err(InverseTransferError::AboveRange {
                physical: 1_334,
                maximum: 1_333
            })
        );
    }

    #[test]
    fn compressing_calibration_still_flips_orientation_on_range_errors() {
        let base = PiecewiseLinearTransfer::new(&INPUTS, &OUTPUTS, MonotonicDirection::Increasing);
        let flipped = AffineCalibration::new(base, -2, 0, 3).unwrap();
        assert!(!flipped.preserves_orientation());

        let low_obs = flipped.convert(100).unwrap();
        let high_obs = flipped.convert(400).unwrap();
        assert_eq!(flipped.invert(low_obs), Ok(100));
        assert_eq!(flipped.invert(high_obs), Ok(400));

        // Past the calibrated image of the former low endpoint → AboveRange.
        assert_eq!(
            flipped.invert(low_obs + 1),
            Err(InverseTransferError::AboveRange {
                physical: low_obs + 1,
                maximum: low_obs
            })
        );
        assert_eq!(
            flipped.invert(high_obs - 1),
            Err(InverseTransferError::BelowRange {
                physical: high_obs - 1,
                minimum: high_obs
            })
        );
    }

    #[test]
    fn calibrated_inverse_undoes_the_affine_before_the_table() {
        let base = PiecewiseLinearTransfer::new(&INPUTS, &OUTPUTS, MonotonicDirection::Increasing);
        let identity = AffineCalibration::new(base, 1, 0, 1).unwrap();
        assert_eq!(identity.invert(-500), Ok(150));
        assert_eq!(identity.invert(0), Ok(200));

        // Scale by ten: a calibrated -5000 is an uncalibrated -500.
        let scaled = AffineCalibration::new(base, 10, 0, 1).unwrap();
        assert_eq!(scaled.invert(-5_000), Ok(150));
        assert_eq!(scaled.convert(150), Ok(-5_000));
    }

    #[test]
    fn calibrated_inverse_reports_bounds_in_calibrated_units() {
        let base = PiecewiseLinearTransfer::new(&INPUTS, &OUTPUTS, MonotonicDirection::Increasing);
        // Uncalibrated range is -1000..=2000; at gain 10 that is -10000..=20000.
        let cal = AffineCalibration::new(base, 10, 0, 1).unwrap();

        // Within half an uncalibrated quantum of the bound, undoing the affine
        // rounds back into range rather than failing: -10_001 / 10 is -1000.1,
        // which is the endpoint knot.
        assert_eq!(cal.invert(-10_001), Ok(100));
        assert_eq!(cal.invert(20_001), Ok(400));

        // Past that, the bound is reported in calibrated units so the caller
        // can compare it against the value they passed in.
        assert_eq!(
            cal.invert(-10_010),
            Err(InverseTransferError::BelowRange {
                physical: -10_010,
                minimum: -10_000
            })
        );
        assert_eq!(
            cal.invert(20_010),
            Err(InverseTransferError::AboveRange {
                physical: 20_010,
                maximum: 20_000
            })
        );
    }

    #[test]
    fn calibrated_inverse_flips_variants_when_orientation_reverses() {
        let base = PiecewiseLinearTransfer::new(&INPUTS, &OUTPUTS, MonotonicDirection::Increasing);
        // Negative gain reverses sense: uncalibrated -1000..=2000 becomes
        // calibrated -2000..=1000, so the inner low bound is the high one here.
        let flipped = AffineCalibration::new(base, -1, 0, 1).unwrap();
        assert!(!flipped.preserves_orientation());
        assert_eq!(flipped.convert(100), Ok(1_000));
        assert_eq!(flipped.convert(400), Ok(-2_000));
        assert_eq!(flipped.invert(1_000), Ok(100));
        assert_eq!(flipped.invert(-2_000), Ok(400));

        // The inner transfer reports BelowRange; calibrated, it is AboveRange.
        assert_eq!(
            flipped.invert(1_001),
            Err(InverseTransferError::AboveRange {
                physical: 1_001,
                maximum: 1_000
            })
        );
        assert_eq!(
            flipped.invert(-2_001),
            Err(InverseTransferError::BelowRange {
                physical: -2_001,
                minimum: -2_000
            })
        );
    }

    #[test]
    fn calibrated_inverse_honors_clamp_and_flat_policy() {
        let clamped =
            PiecewiseLinearTransfer::new(&INPUTS, &OUTPUTS, MonotonicDirection::Increasing)
                .with_boundaries(BoundaryBehavior::Clamp, BoundaryBehavior::Clamp);
        let cal = AffineCalibration::new(clamped, 10, 0, 1).unwrap();
        assert_eq!(cal.invert(-99_999), Ok(100));
        assert_eq!(cal.invert(99_999), Ok(400));

        let flat = PiecewiseLinearTransfer::new(
            &FLAT_INPUTS,
            &FLAT_OUTPUTS,
            MonotonicDirection::Increasing,
        )
        .with_flat_resolution(FlatResolution::Error);
        let cal = AffineCalibration::new(flat, 2, 0, 1).unwrap();
        // Flat run sits at uncalibrated 10, i.e. calibrated 20.
        assert_eq!(
            cal.invert(20),
            Err(InverseTransferError::AmbiguousFlat {
                physical: 20,
                low: 10,
                high: 20
            })
        );
    }

    #[test]
    fn calibrated_inverse_rejects_unrepresentable_input() {
        let base = PiecewiseLinearTransfer::new(&INPUTS, &OUTPUTS, MonotonicDirection::Increasing);
        // Undoing a large scale pushes the uncalibrated value outside i32.
        let cal = AffineCalibration::new(base, 1, 0, i32::MAX).unwrap();
        assert_eq!(cal.invert(i32::MAX), Err(InverseTransferError::Overflow));
    }

    #[test]
    fn nested_calibration_inverts_through_every_layer() {
        let base = PiecewiseLinearTransfer::new(&INPUTS, &OUTPUTS, MonotonicDirection::Increasing);
        let inner = AffineCalibration::new(base, 2, 10, 1).unwrap();
        let outer = AffineCalibration::new(inner, 1, -10, 2).unwrap();
        assert_eq!(outer.convert(200), Ok(0));
        assert_eq!(outer.invert(0), Ok(200));
        assert_eq!(outer.convert(400), Ok(2_000));
        assert_eq!(outer.invert(2_000), Ok(400));
    }
}
