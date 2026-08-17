//! Fixed-memory temporal stabilization for caller-supplied samples.
//!
//! These primitives are deterministic data processors. They do not acquire
//! samples, read clocks, choose a sampling cadence, or interact with hardware.
//!
//! Decision helpers [`Hysteresis`] and [`Debounce`] sit beside the filter /
//! detector family: they latch application-level boolean decisions from
//! sample-count cadence only, without GPIO or wall-clock ownership.

use crate::round::div_nearest_ties_away;

mod sealed {
    pub trait Sealed {}
}

/// Integer sample type supported by temporal stabilization primitives.
///
/// This trait is sealed and implemented for `u16`, `i32`, and `u32`. Callers
/// cannot add implementations.
///
/// [`MovingAverage`] keeps an `i64` running sum, so each type declares the
/// largest `N` for which `N` copies of its widest sample still fit. For `u16`
/// and `i32` that bound is at least `usize::MAX` on 32-bit targets, so those
/// implementations cap at `usize::MAX` there. Every addressable window also
/// fits for `u32` on 16-bit-pointer targets. On wider targets its bound is
/// `floor(i64::MAX / u32::MAX) = 2_147_483_648`. On a 32-bit target, arrays
/// near that formal ceiling are already too large for a usable Rust value;
/// the explicit arithmetic bound nevertheless keeps the accumulator contract
/// target-independent instead of relying on a separate layout rejection.
///
/// The bound is an accumulator-safety ceiling, not a recommended window.
/// Storage is `[T; N]` plus the `i64` sum — `2_147_483_648` `u32` samples
/// occupy 8 GiB. Firmware chooses `N` from available RAM.
pub trait TemporalSample: sealed::Sealed + Copy + Ord {
    /// Zero value used to initialize fixed storage.
    #[doc(hidden)]
    const ZERO: Self;
    /// Largest safe moving-average window for this sample type.
    #[doc(hidden)]
    const MAX_WINDOW: usize;

    /// Convert to the shared signed accumulator representation.
    #[doc(hidden)]
    fn to_i64(self) -> i64;
    /// Convert a proven-in-range result from the accumulator representation.
    #[doc(hidden)]
    fn from_i64(value: i64) -> Self;
}

impl sealed::Sealed for u16 {}

impl TemporalSample for u16 {
    const ZERO: Self = 0;
    const MAX_WINDOW: usize = if usize::BITS > 32 {
        (i64::MAX / u16::MAX as i64) as usize
    } else {
        usize::MAX
    };

    fn to_i64(self) -> i64 {
        i64::from(self)
    }

    fn from_i64(value: i64) -> Self {
        debug_assert!((0..=i64::from(u16::MAX)).contains(&value));
        value as u16
    }
}

impl sealed::Sealed for i32 {}

impl TemporalSample for i32 {
    const ZERO: Self = 0;
    const MAX_WINDOW: usize = if usize::BITS > 32 {
        (i64::MAX / 2_147_483_648) as usize
    } else {
        usize::MAX
    };

    fn to_i64(self) -> i64 {
        i64::from(self)
    }

    fn from_i64(value: i64) -> Self {
        debug_assert!((i64::from(i32::MIN)..=i64::from(i32::MAX)).contains(&value));
        value as i32
    }
}

impl sealed::Sealed for u32 {}

impl TemporalSample for u32 {
    const ZERO: Self = 0;
    // Every window representable by a 16-bit `usize` fits the accumulator. On
    // 32/64-bit targets, preserve the exact mathematical accumulator cap even
    // though a 32-bit target cannot materialize arrays near that size.
    const MAX_WINDOW: usize = if usize::BITS < 32 {
        usize::MAX
    } else {
        (i64::MAX / u32::MAX as i64) as usize
    };

    fn to_i64(self) -> i64 {
        i64::from(self)
    }

    fn from_i64(value: i64) -> Self {
        debug_assert!((0..=i64::from(u32::MAX)).contains(&value));
        value as u32
    }
}

// This is deliberately a compile-time target guard: host unit tests cannot
// execute the 16-bit branch, while the MSP430 core-only CI build can.
#[cfg(target_pointer_width = "16")]
const _: () = assert!(<u32 as TemporalSample>::MAX_WINDOW == usize::MAX);

/// Output from a temporal filter.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum FilterOutput<T> {
    /// The fixed window has not yet received enough samples.
    WarmingUp {
        /// Number of samples currently retained.
        samples: usize,
        /// Number of samples required for a ready output.
        required: usize,
    },
    /// The filter has enough state to produce a value.
    Ready(T),
}

impl<T> FilterOutput<T> {
    /// Return the ready value, or `None` while warming up.
    pub fn ready(self) -> Option<T> {
        match self {
            Self::WarmingUp { .. } => None,
            Self::Ready(value) => Some(value),
        }
    }
}

/// Common interface for caller-driven temporal filters.
pub trait TemporalFilter<T> {
    /// Push one sample and return the current filter state.
    fn update(&mut self, value: T) -> FilterOutput<T>;
    /// Discard all retained history.
    fn reset(&mut self);
}

/// Exact fixed-window moving average.
///
/// Updates are `O(1)` using a checked-range `i64` running sum. Output begins
/// only after all `N` samples have been supplied.
///
/// The per-type window cap on [`TemporalSample`] is an accumulator-safety
/// ceiling so `N` copies of the widest sample still fit in `i64`. It is not a
/// practical size: storage is `[T; N]` plus that sum, and firmware chooses `N`
/// from available RAM.
#[derive(Clone, Debug)]
pub struct MovingAverage<T: TemporalSample, const N: usize> {
    samples: [T; N],
    sum: i64,
    next: usize,
    len: usize,
}

impl<T: TemporalSample, const N: usize> MovingAverage<T, N> {
    /// Construct an empty moving average.
    ///
    /// # Panics
    ///
    /// Panics for a zero-sized window or a window larger than the
    /// accumulator-safety ceiling of `T` (see [`TemporalSample`]). That ceiling
    /// is not a recommended window size; `N` is fixed storage.
    pub const fn new() -> Self {
        assert!(N > 0);
        assert!(N <= T::MAX_WINDOW);
        Self {
            samples: [T::ZERO; N],
            sum: 0,
            next: 0,
            len: 0,
        }
    }

    /// Number of retained samples.
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Whether no samples are retained.
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl<T: TemporalSample, const N: usize> Default for MovingAverage<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: TemporalSample, const N: usize> TemporalFilter<T> for MovingAverage<T, N> {
    fn update(&mut self, value: T) -> FilterOutput<T> {
        if self.len == N {
            self.sum -= self.samples[self.next].to_i64();
        } else {
            self.len += 1;
        }

        self.samples[self.next] = value;
        self.sum += value.to_i64();
        self.next += 1;
        if self.next == N {
            self.next = 0;
        }

        if self.len < N {
            FilterOutput::WarmingUp {
                samples: self.len,
                required: N,
            }
        } else {
            FilterOutput::Ready(T::from_i64(div_nearest_ties_away(self.sum, N as i64)))
        }
    }

    fn reset(&mut self) {
        self.samples = [T::ZERO; N];
        self.sum = 0;
        self.next = 0;
        self.len = 0;
    }
}

/// Fixed-window median filter for small odd window sizes.
///
/// The retained window is copied and insertion-sorted on each update, making
/// this most appropriate for small windows used to reject isolated spikes.
#[derive(Clone, Debug)]
pub struct MedianFilter<T: TemporalSample, const N: usize> {
    samples: [T; N],
    next: usize,
    len: usize,
}

impl<T: TemporalSample, const N: usize> MedianFilter<T, N> {
    /// Construct an empty median filter.
    ///
    /// # Panics
    ///
    /// Panics unless `N` is nonzero and odd.
    pub const fn new() -> Self {
        assert!(N > 0 && N % 2 == 1);
        Self {
            samples: [T::ZERO; N],
            next: 0,
            len: 0,
        }
    }

    /// Number of retained samples.
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Whether no samples are retained.
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl<T: TemporalSample, const N: usize> Default for MedianFilter<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: TemporalSample, const N: usize> TemporalFilter<T> for MedianFilter<T, N> {
    fn update(&mut self, value: T) -> FilterOutput<T> {
        self.samples[self.next] = value;
        self.next += 1;
        if self.next == N {
            self.next = 0;
        }
        if self.len < N {
            self.len += 1;
        }
        if self.len < N {
            return FilterOutput::WarmingUp {
                samples: self.len,
                required: N,
            };
        }

        let mut sorted = self.samples;
        let mut index = 1;
        while index < N {
            let value = sorted[index];
            let mut insert = index;
            while insert > 0 && sorted[insert - 1] > value {
                sorted[insert] = sorted[insert - 1];
                insert -= 1;
            }
            sorted[insert] = value;
            index += 1;
        }
        FilterOutput::Ready(sorted[N / 2])
    }

    fn reset(&mut self) {
        self.samples = [T::ZERO; N];
        self.next = 0;
        self.len = 0;
    }
}

/// Constant-memory exponential smoother.
///
/// `alpha` is an unsigned Q0.16-like blend weight: `0` retains the initialized
/// value and `65535` follows each new sample exactly. The first sample
/// initializes the smoother and is immediately ready.
///
/// Updates use nearest integer division:
/// `adjustment = round(delta * alpha / 65535)`.
/// When `|delta| * alpha < 32768`, the adjustment is zero, so light smoothing
/// can ignore small steps until the gap is large enough. Choose `alpha` with
/// that quantization floor in mind.
#[derive(Copy, Clone, Debug)]
pub struct ExponentialSmoother<T: TemporalSample> {
    alpha: u16,
    value: T,
    initialized: bool,
}

impl<T: TemporalSample> ExponentialSmoother<T> {
    /// Construct an uninitialized smoother with the supplied blend weight.
    pub const fn new(alpha: u16) -> Self {
        Self {
            alpha,
            value: T::ZERO,
            initialized: false,
        }
    }

    /// Return the configured Q0.16-like blend weight.
    pub const fn alpha(&self) -> u16 {
        self.alpha
    }

    /// Return the current value, or `None` before the first sample.
    pub const fn value(&self) -> Option<T> {
        if self.initialized {
            Some(self.value)
        } else {
            None
        }
    }
}

impl<T: TemporalSample> TemporalFilter<T> for ExponentialSmoother<T> {
    fn update(&mut self, value: T) -> FilterOutput<T> {
        if !self.initialized {
            self.value = value;
            self.initialized = true;
            return FilterOutput::Ready(value);
        }

        let current = self.value.to_i64();
        let delta = value.to_i64() - current;
        let adjustment = div_nearest_ties_away(delta * i64::from(self.alpha), i64::from(u16::MAX));
        self.value = T::from_i64(current + adjustment);
        FilterOutput::Ready(self.value)
    }

    fn reset(&mut self) {
        self.value = T::ZERO;
        self.initialized = false;
    }
}

/// Classification returned by a [`StabilityDetector`].
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Stability<T> {
    /// The detector has not yet received a full window.
    WarmingUp {
        /// Number of samples currently retained.
        samples: usize,
        /// Number of samples required for classification.
        required: usize,
    },
    /// The full window exceeds the configured range threshold.
    Unstable {
        /// Minimum retained sample.
        minimum: T,
        /// Maximum retained sample.
        maximum: T,
        /// Difference between maximum and minimum in sample quanta.
        span: u64,
    },
    /// The full window is within the configured range threshold.
    Stable {
        /// Minimum retained sample.
        minimum: T,
        /// Maximum retained sample.
        maximum: T,
        /// Difference between maximum and minimum in sample quanta.
        span: u64,
    },
}

/// Fixed-window range-based stability detector.
///
/// Classification begins only after all `N` samples are present. The detector
/// reports retained extrema and never substitutes a stale last-good value.
#[derive(Clone, Debug)]
pub struct StabilityDetector<T: TemporalSample, const N: usize> {
    samples: [T; N],
    threshold: u64,
    next: usize,
    len: usize,
}

impl<T: TemporalSample, const N: usize> StabilityDetector<T, N> {
    /// Construct an empty detector with a maximum stable range in sample
    /// quanta.
    ///
    /// # Panics
    ///
    /// Panics for a zero-sized window.
    pub const fn new(threshold: u64) -> Self {
        assert!(N > 0);
        Self {
            samples: [T::ZERO; N],
            threshold,
            next: 0,
            len: 0,
        }
    }

    /// Return the maximum range classified as stable.
    pub const fn threshold(&self) -> u64 {
        self.threshold
    }

    /// Number of retained samples.
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Whether no samples are retained.
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Push one sample and classify the retained window.
    pub fn update(&mut self, value: T) -> Stability<T> {
        self.samples[self.next] = value;
        self.next += 1;
        if self.next == N {
            self.next = 0;
        }
        if self.len < N {
            self.len += 1;
        }
        if self.len < N {
            return Stability::WarmingUp {
                samples: self.len,
                required: N,
            };
        }

        let mut minimum = self.samples[0];
        let mut maximum = self.samples[0];
        let mut index = 1;
        while index < N {
            minimum = minimum.min(self.samples[index]);
            maximum = maximum.max(self.samples[index]);
            index += 1;
        }
        let span = (maximum.to_i64() - minimum.to_i64()) as u64;
        if span <= self.threshold {
            Stability::Stable {
                minimum,
                maximum,
                span,
            }
        } else {
            Stability::Unstable {
                minimum,
                maximum,
                span,
            }
        }
    }

    /// Discard all retained history.
    pub fn reset(&mut self) {
        self.samples = [T::ZERO; N];
        self.next = 0;
        self.len = 0;
    }
}

/// Schmitt-trigger latch over integer samples.
///
/// Values at or above `high` latch on; values at or below `low` latch off.
/// Samples strictly between the thresholds hold the previous latch. When
/// `low == high`, the band collapses to a simple threshold with no hold
/// region. Cadence is caller-driven sample count — this type never reads a
/// clock or GPIO.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Hysteresis<T: TemporalSample> {
    low: T,
    high: T,
    latched: bool,
    initial: bool,
}

impl Hysteresis<i32> {
    /// Construct a hysteresis latch that starts off.
    ///
    /// # Panics
    ///
    /// Panics if `low > high`.
    pub const fn new(low: i32, high: i32) -> Self {
        assert!(low <= high);
        Self {
            low,
            high,
            latched: false,
            initial: false,
        }
    }
}

impl Hysteresis<u16> {
    /// Construct a hysteresis latch that starts off.
    ///
    /// # Panics
    ///
    /// Panics if `low > high`.
    pub const fn new(low: u16, high: u16) -> Self {
        assert!(low <= high);
        Self {
            low,
            high,
            latched: false,
            initial: false,
        }
    }
}

impl Hysteresis<u32> {
    /// Construct a hysteresis latch that starts off.
    ///
    /// # Panics
    ///
    /// Panics if `low > high`.
    pub const fn new(low: u32, high: u32) -> Self {
        assert!(low <= high);
        Self {
            low,
            high,
            latched: false,
            initial: false,
        }
    }
}

impl<T: TemporalSample> Hysteresis<T> {
    /// Set the initial latch used before the first crossing and after
    /// [`reset`](Self::reset).
    pub const fn with_initial(mut self, on: bool) -> Self {
        self.latched = on;
        self.initial = on;
        self
    }

    /// Return the configured low threshold.
    pub const fn low(&self) -> T {
        self.low
    }

    /// Return the configured high threshold.
    pub const fn high(&self) -> T {
        self.high
    }

    /// Push one sample and return the current latched level.
    ///
    /// The first sample inside the open band `(low, high)` holds the initial
    /// latch (default `false`). Applications that need an explicit unknown
    /// state should track `Option` separately.
    pub fn update(&mut self, value: T) -> bool {
        if value >= self.high {
            self.latched = true;
        } else if value <= self.low {
            self.latched = false;
        }
        self.latched
    }

    /// Return the current latched level without consuming a sample.
    pub const fn state(&self) -> bool {
        self.latched
    }

    /// Restore the latch to the value supplied by [`with_initial`](Self::with_initial)
    /// (or `false` when that builder was not used).
    pub fn reset(&mut self) {
        self.latched = self.initial;
    }
}

/// Output from a sample-count [`Debounce`].
///
/// No latched level is reported until `N` consecutive agreeing samples have
/// been observed. After arming, [`Steady`](DebounceOutput::Steady) holds the
/// previous latch while a new candidate accumulates, and
/// [`Edge`](DebounceOutput::Edge) reports only confirmed level changes.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum DebounceOutput {
    /// Fewer than `N` consecutive agreeing samples have been seen since start
    /// or the last candidate change, and no level has latched yet.
    WarmingUp {
        /// Consecutive samples agreeing on the current candidate.
        streak: usize,
        /// Samples required to latch (`N`).
        required: usize,
    },
    /// Latched level is unchanged on this sample.
    Steady(bool),
    /// Latched level changed on this sample (including the first latch).
    Edge {
        /// Newly latched level.
        level: bool,
    },
}

/// Sample-count contact debounce for boolean inputs.
///
/// Latches after `N` consecutive agreeing samples. A candidate flip mid-streak
/// resets the streak to one on the new candidate. Timing is entirely in sample
/// counts — callers that think in milliseconds must convert duration to `N`
/// themselves. This type never owns GPIO, EXTI, or clocks.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Debounce<const N: usize> {
    candidate: bool,
    streak: usize,
    latched: bool,
    armed: bool,
}

impl<const N: usize> Debounce<N> {
    /// Construct an unarmed debounce.
    ///
    /// # Panics
    ///
    /// Panics when `N == 0`.
    pub const fn new() -> Self {
        assert!(N > 0);
        Self {
            candidate: false,
            streak: 0,
            latched: false,
            armed: false,
        }
    }

    /// Push one boolean sample and return the debounce state.
    pub fn update(&mut self, sample: bool) -> DebounceOutput {
        if self.streak == 0 || sample != self.candidate {
            self.candidate = sample;
            self.streak = 1;
        } else if self.streak < N {
            self.streak += 1;
        }

        if self.streak < N {
            if self.armed {
                DebounceOutput::Steady(self.latched)
            } else {
                DebounceOutput::WarmingUp {
                    streak: self.streak,
                    required: N,
                }
            }
        } else if !self.armed {
            self.armed = true;
            self.latched = self.candidate;
            DebounceOutput::Edge {
                level: self.latched,
            }
        } else if self.candidate != self.latched {
            self.latched = self.candidate;
            DebounceOutput::Edge {
                level: self.latched,
            }
        } else {
            DebounceOutput::Steady(self.latched)
        }
    }

    /// Return the latched level after the first confirmed latch, or `None`
    /// while still warming up.
    pub const fn state(&self) -> Option<bool> {
        if self.armed { Some(self.latched) } else { None }
    }

    /// Discard streak and latch state.
    pub fn reset(&mut self) {
        self.candidate = false;
        self.streak = 0;
        self.latched = false;
        self.armed = false;
    }
}

impl<const N: usize> Default for Debounce<N> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;

    #[test]
    fn moving_average_warms_up_and_rolls() {
        let mut filter = MovingAverage::<i32, 3>::new();
        assert_eq!(
            filter.update(3),
            FilterOutput::WarmingUp {
                samples: 1,
                required: 3
            }
        );
        assert_eq!(filter.update(6).ready(), None);
        assert_eq!(filter.update(9), FilterOutput::Ready(6));
        assert_eq!(filter.update(12), FilterOutput::Ready(9));
    }

    #[test]
    fn moving_average_rounds_signed_ties_away() {
        let mut positive = MovingAverage::<i32, 2>::new();
        positive.update(0);
        assert_eq!(positive.update(1), FilterOutput::Ready(1));

        let mut negative = MovingAverage::<i32, 2>::new();
        negative.update(0);
        assert_eq!(negative.update(-1), FilterOutput::Ready(-1));
    }

    #[test]
    fn moving_average_handles_integer_extremes() {
        let mut signed = MovingAverage::<i32, 2>::new();
        signed.update(i32::MIN);
        assert_eq!(signed.update(i32::MAX), FilterOutput::Ready(-1));

        let mut unsigned = MovingAverage::<u16, 2>::new();
        unsigned.update(0);
        assert_eq!(unsigned.update(u16::MAX), FilterOutput::Ready(32_768));
    }

    #[test]
    fn median_rejects_isolated_spike() {
        let mut filter = MedianFilter::<u16, 5>::new();
        for value in [1000, 1001, 4095, 999] {
            assert!(filter.update(value).ready().is_none());
        }
        assert_eq!(filter.update(1002), FilterOutput::Ready(1001));
    }

    #[test]
    fn exponential_smoother_has_explicit_step_response() {
        let mut filter = ExponentialSmoother::<i32>::new(32_768);
        assert_eq!(filter.update(0), FilterOutput::Ready(0));
        assert_eq!(filter.update(1000), FilterOutput::Ready(500));
        assert_eq!(filter.update(1000), FilterOutput::Ready(750));
        assert_eq!(filter.value(), Some(750));
    }

    #[test]
    fn exponential_smoother_quantization_floor_ignores_small_steps() {
        let mut filter = ExponentialSmoother::<i32>::new(100);
        filter.update(0);
        // |delta| * alpha = 327 * 100 = 32700 < 32768, so adjustment rounds to 0.
        assert_eq!(filter.update(327), FilterOutput::Ready(0));
        // |delta| * alpha = 328 * 100 = 32800 >= 32768, so adjustment becomes 1.
        assert_eq!(filter.update(328), FilterOutput::Ready(1));
    }

    #[test]
    fn reset_restores_warmup_or_uninitialized_state() {
        let mut average = MovingAverage::<u16, 2>::new();
        average.update(10);
        average.update(20);
        average.reset();
        assert!(average.is_empty());
        assert!(average.update(30).ready().is_none());

        let mut exponential = ExponentialSmoother::<i32>::new(1000);
        exponential.update(42);
        exponential.reset();
        assert_eq!(exponential.value(), None);
        assert_eq!(exponential.update(-7), FilterOutput::Ready(-7));
    }

    #[test]
    fn detector_distinguishes_warm_stable_and_unstable() {
        let mut detector = StabilityDetector::<i32, 3>::new(4);
        assert!(matches!(detector.update(100), Stability::WarmingUp { .. }));
        assert!(matches!(detector.update(102), Stability::WarmingUp { .. }));
        assert_eq!(
            detector.update(104),
            Stability::Stable {
                minimum: 100,
                maximum: 104,
                span: 4
            }
        );
        assert_eq!(
            detector.update(110),
            Stability::Unstable {
                minimum: 102,
                maximum: 110,
                span: 8
            }
        );
    }

    #[test]
    fn detector_span_handles_full_i32_range() {
        let mut detector = StabilityDetector::<i32, 2>::new(u64::MAX);
        detector.update(i32::MIN);
        assert_eq!(
            detector.update(i32::MAX),
            Stability::Stable {
                minimum: i32::MIN,
                maximum: i32::MAX,
                span: u64::from(u32::MAX)
            }
        );
    }

    #[test]
    fn invalid_window_sizes_are_rejected() {
        assert!(std::panic::catch_unwind(MovingAverage::<i32, 0>::new).is_err());
        assert!(std::panic::catch_unwind(MedianFilter::<i32, 2>::new).is_err());
        assert!(std::panic::catch_unwind(|| StabilityDetector::<i32, 0>::new(0)).is_err());
    }

    #[test]
    fn hysteresis_latches_with_hold_band() {
        let mut hyst = Hysteresis::<i32>::new(10, 20);
        assert!(!hyst.update(15));
        assert!(hyst.update(20));
        assert!(hyst.update(15));
        assert!(!hyst.update(10));
        assert!(!hyst.update(15));
    }

    #[test]
    fn hysteresis_equal_thresholds_are_simple_threshold() {
        let mut hyst = Hysteresis::<u16>::new(100, 100);
        assert!(!hyst.update(99));
        // `value >= high` wins when low == high, so the threshold itself latches on.
        assert!(hyst.update(100));
        assert!(hyst.update(100));
        assert!(!hyst.update(99));
    }

    #[test]
    fn hysteresis_with_initial_and_reset() {
        let mut hyst = Hysteresis::<i32>::new(-5, 5).with_initial(true);
        assert!(hyst.state());
        assert!(hyst.update(0));
        assert!(!hyst.update(-5));
        hyst.reset();
        assert!(hyst.state());
    }

    #[test]
    fn hysteresis_rejects_inverted_band() {
        assert!(std::panic::catch_unwind(|| Hysteresis::<i32>::new(2, 1)).is_err());
        assert!(std::panic::catch_unwind(|| Hysteresis::<u16>::new(2, 1)).is_err());
    }

    #[test]
    fn debounce_warms_up_then_edges_on_change() {
        let mut deb = Debounce::<3>::new();
        assert_eq!(
            deb.update(true),
            DebounceOutput::WarmingUp {
                streak: 1,
                required: 3
            }
        );
        assert_eq!(
            deb.update(true),
            DebounceOutput::WarmingUp {
                streak: 2,
                required: 3
            }
        );
        assert_eq!(deb.update(true), DebounceOutput::Edge { level: true });
        assert_eq!(deb.state(), Some(true));
        assert_eq!(deb.update(true), DebounceOutput::Steady(true));
        assert_eq!(deb.update(false), DebounceOutput::Steady(true));
        assert_eq!(deb.update(false), DebounceOutput::Steady(true));
        assert_eq!(deb.update(false), DebounceOutput::Edge { level: false });
        assert_eq!(deb.update(false), DebounceOutput::Steady(false));
    }

    #[test]
    fn debounce_candidate_flip_resets_streak() {
        let mut deb = Debounce::<3>::new();
        assert!(matches!(
            deb.update(true),
            DebounceOutput::WarmingUp { streak: 1, .. }
        ));
        assert!(matches!(
            deb.update(false),
            DebounceOutput::WarmingUp { streak: 1, .. }
        ));
        assert!(matches!(
            deb.update(false),
            DebounceOutput::WarmingUp { streak: 2, .. }
        ));
        assert_eq!(deb.update(false), DebounceOutput::Edge { level: false });
    }

    #[test]
    fn debounce_n_one_is_passthrough_with_edges() {
        let mut deb = Debounce::<1>::new();
        assert_eq!(deb.update(false), DebounceOutput::Edge { level: false });
        assert_eq!(deb.update(false), DebounceOutput::Steady(false));
        assert_eq!(deb.update(true), DebounceOutput::Edge { level: true });
        assert_eq!(deb.update(true), DebounceOutput::Steady(true));
    }

    #[test]
    fn debounce_reset_returns_to_warmup() {
        let mut deb = Debounce::<2>::new();
        deb.update(true);
        deb.update(true);
        assert_eq!(deb.state(), Some(true));
        deb.reset();
        assert_eq!(deb.state(), None);
        assert!(matches!(
            deb.update(false),
            DebounceOutput::WarmingUp { .. }
        ));
    }

    #[test]
    fn debounce_rejects_zero_window() {
        assert!(std::panic::catch_unwind(Debounce::<0>::new).is_err());
    }

    #[test]
    fn hysteresis_into_debounce_composition() {
        let mut hyst = Hysteresis::<i32>::new(10, 20);
        let mut deb = Debounce::<2>::new();
        let mut edges = 0u8;
        for sample in [0, 25, 25, 15, 5, 5, 5] {
            let level = hyst.update(sample);
            if matches!(deb.update(level), DebounceOutput::Edge { .. }) {
                edges += 1;
            }
        }
        assert_eq!(edges, 2);
        assert_eq!(deb.state(), Some(false));
    }

    #[test]
    fn u32_moving_average_warms_up_rolls_and_resets() {
        let mut filter = MovingAverage::<u32, 3>::new();
        assert!(filter.is_empty());
        assert_eq!(
            filter.update(3),
            FilterOutput::WarmingUp {
                samples: 1,
                required: 3
            }
        );
        assert_eq!(filter.len(), 1);
        assert_eq!(filter.update(6).ready(), None);
        assert_eq!(filter.update(9), FilterOutput::Ready(6));
        assert_eq!(filter.len(), 3);
        assert_eq!(filter.update(12), FilterOutput::Ready(9));
        filter.reset();
        assert!(filter.is_empty());
        assert_eq!(filter.len(), 0);
        assert!(filter.update(30).ready().is_none());
    }

    #[test]
    fn u32_moving_average_handles_zero_max_and_ties() {
        let mut filter = MovingAverage::<u32, 2>::new();
        filter.update(0);
        assert_eq!(filter.update(u32::MAX), FilterOutput::Ready(2_147_483_648));

        let mut both_max = MovingAverage::<u32, 2>::new();
        both_max.update(u32::MAX);
        assert_eq!(both_max.update(u32::MAX), FilterOutput::Ready(u32::MAX));

        let mut zeros = MovingAverage::<u32, 2>::new();
        zeros.update(0);
        assert_eq!(zeros.update(0), FilterOutput::Ready(0));
    }

    #[test]
    fn u32_moving_average_window_bound_is_accumulator_limited() {
        const MAX: usize = <u32 as TemporalSample>::MAX_WINDOW;
        assert_eq!(MAX, 2_147_483_648);
        assert_eq!(MAX as i64, i64::MAX / i64::from(u32::MAX));
        assert!((MAX as i64).checked_mul(i64::from(u32::MAX)).is_some());
        assert!(
            ((MAX as i64) + 1)
                .checked_mul(i64::from(u32::MAX))
                .is_none()
        );
    }

    #[test]
    fn u32_median_covers_full_unsigned_width() {
        let mut filter = MedianFilter::<u32, 3>::new();
        assert!(filter.update(0).ready().is_none());
        assert!(filter.update(u32::MAX).ready().is_none());
        assert_eq!(filter.update(1), FilterOutput::Ready(1));
        assert_eq!(filter.update(u32::MAX), FilterOutput::Ready(u32::MAX));
    }

    #[test]
    fn u32_exponential_smoother_extreme_transitions() {
        let mut up = ExponentialSmoother::<u32>::new(u16::MAX);
        assert_eq!(up.update(0), FilterOutput::Ready(0));
        assert_eq!(up.update(u32::MAX), FilterOutput::Ready(u32::MAX));

        let mut down = ExponentialSmoother::<u32>::new(u16::MAX);
        assert_eq!(down.update(u32::MAX), FilterOutput::Ready(u32::MAX));
        assert_eq!(down.update(0), FilterOutput::Ready(0));
    }

    #[test]
    fn u32_detector_span_covers_full_unsigned_range() {
        let mut detector = StabilityDetector::<u32, 2>::new(u64::MAX);
        detector.update(0);
        assert_eq!(
            detector.update(u32::MAX),
            Stability::Stable {
                minimum: 0,
                maximum: u32::MAX,
                span: u64::from(u32::MAX)
            }
        );
    }

    #[test]
    fn u32_hysteresis_latches_at_unsigned_boundaries() {
        let mut hyst = Hysteresis::<u32>::new(0, u32::MAX);
        assert!(!hyst.update(1));
        assert!(hyst.update(u32::MAX));
        assert!(hyst.update(1));
        assert!(!hyst.update(0));
        assert!(!hyst.update(1));
    }

    #[test]
    fn u32_hysteresis_equal_thresholds_at_boundaries() {
        let mut at_zero = Hysteresis::<u32>::new(0, 0);
        assert!(at_zero.update(0));

        let mut at_max = Hysteresis::<u32>::new(u32::MAX, u32::MAX);
        assert!(!at_max.update(u32::MAX - 1));
        assert!(at_max.update(u32::MAX));
        assert!(at_max.update(u32::MAX));
        assert!(!at_max.update(u32::MAX - 1));
    }

    #[test]
    fn u32_hysteresis_rejects_inverted_band() {
        assert!(std::panic::catch_unwind(|| Hysteresis::<u32>::new(2, 1)).is_err());
        assert!(std::panic::catch_unwind(|| Hysteresis::<u32>::new(u32::MAX, 0)).is_err());
    }
}
