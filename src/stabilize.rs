//! Fixed-memory temporal stabilization for caller-supplied samples.
//!
//! These primitives are deterministic data processors. They do not acquire
//! samples, read clocks, choose a sampling cadence, or interact with hardware.

mod sealed {
    pub trait Sealed {}
}

/// Integer sample type supported by temporal stabilization primitives.
///
/// This trait is sealed and currently implemented for `u16` and `i32`.
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
    /// Panics for a zero-sized window or a window too large for the `i64`
    /// running-sum guarantee of `T`.
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
            FilterOutput::Ready(T::from_i64(round_div_nearest(self.sum, N as i64)))
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
        let adjustment = round_div_nearest(delta * i64::from(self.alpha), i64::from(u16::MAX));
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

fn round_div_nearest(numerator: i64, denominator: i64) -> i64 {
    if numerator >= 0 {
        (numerator + denominator / 2) / denominator
    } else {
        -((-numerator + denominator / 2) / denominator)
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
}
