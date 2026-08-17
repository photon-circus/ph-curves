# Measurement pipelines

ph-curves supplies composable integer primitives rather than a hidden runtime
pipeline. A common order is:

`observation -> transfer -> optional affine correction -> filter -> stability -> decision`

The caller owns acquisition, units, cadence, invalid-sample handling, resets,
coefficient storage, and hardware action.

## Runtime calibration

`AffineTransform` applies

`corrected = (measurement * gain + offset) / scale`

with checked `i64` arithmetic and nearest, ties-away rounding. `offset` is in
the numerator. To apply an output-space offset `d`, pass `d * scale`.

```rust
use ph_curves::AffineTransform;

// +0.5% gain and -120 output quanta.
let trim = AffineTransform::new(1_005, -120_000, 1_000).unwrap();
let corrected = trim.apply(25_000).unwrap();
let original = trim.unapply(corrected).unwrap();
assert!((original - 25_000).abs() <= 1);
```

Apply a fallible correction before mutating filter state so an overflow does
not insert a sample into downstream windows. `AffineCalibration` wraps a
transfer with the same primitive in both directions. It applies coefficients;
it does not discover or persist them.

## Choose a temporal primitive

| Primitive | State and update cost | Best fit |
| --- | --- | --- |
| `MovingAverage<T, N>` | `[T; N]`, running `i64` sum, indices; `O(1)` | Exact smoothing over a fixed window |
| `MedianFilter<T, N>` | `[T; N]`, indices; bounded small-window sort | Isolated spike rejection |
| `ExponentialSmoother<T>` | One estimate and blend coefficient; `O(1)` | Constant-memory smoothing |
| `StabilityDetector<T, N>` | Separate `[T; N]`, threshold, indices; `O(N)` | Classify a recent range without replacing the value |

Supported sample types are `u16`, `i32`, and `u32`. Window storage is real
fixed memory: a mathematically safe maximum is not a practical recommendation.
The exponential smoother quantizes
`round(delta * alpha / 65535)`; a sufficiently small delta and alpha can
produce no adjustment.

Filtering raw observations and filtering physical measurements answer
different questions. For a nonlinear transfer,
`transfer(mean(raw)) != mean(transfer(raw))` in general. Even affine correction
can differ across the two orders because integer stages round. Choose the
domain whose units should define noise, window, and stability thresholds.

## Warm-up and decisions

A filter window `F` followed by a stability window `S` needs `F + S - 1`
accepted samples before the first stability classification. Warm-up and
instability do not implicitly reset or update an application latch.

`Hysteresis` latches a Boolean state using low and high thresholds; the band
prevents chatter. `Debounce<N>` confirms a Boolean input only after `N`
consecutive matching samples and reports a confirmed edge once.

```rust
use ph_curves::{
    Hysteresis, MovingAverage, Stability, StabilityDetector, TemporalFilter,
};

let mut average = MovingAverage::<u32, 4>::new();
let mut settled = StabilityDetector::<u32, 3>::new(5_000);
let mut high = Hysteresis::<u32>::new(900_000, 1_000_000);
let mut high_light = false;

for micro_lux in [
    1_010_000, 1_006_000, 1_004_000, 1_002_000, 1_001_000, 999_000,
] {
    let Some(smoothed) = average.update(micro_lux).ready() else {
        continue;
    };
    if matches!(settled.update(smoothed), Stability::Stable { .. }) {
        high_light = high.update(smoothed);
    }
}

assert!(high_light);
```

This example deliberately holds the existing latch during warm-up or
instability. Updating it on every filtered value, resetting it, or holding it
are application policies, not hidden crate behavior.

## Caller-owned failure and reset policy

Each update represents one valid sample accepted by the caller. Decide
explicitly whether a missing sample, transfer error, cadence gap, or device
fault should skip an update or reset the filter, detector, and decision state.
No primitive reads a clock, acquires data, stores calibration, or drives
hardware.

The root [pipeline quick start](../../README.md#quick-start-stabilize-and-decide)
provides the compact path. See rustdoc for exact overflow, range, warm-up, and
reset contracts.
