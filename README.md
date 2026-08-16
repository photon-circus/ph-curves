# ph-curves

[![Crates.io](https://img.shields.io/crates/v/ph-curves)](https://crates.io/crates/ph-curves)
[![docs.rs](https://img.shields.io/docsrs/ph-curves)](https://docs.rs/ph-curves)
[![CI](https://github.com/photon-circus/ph-curves/actions/workflows/ci.yml/badge.svg)](https://github.com/photon-circus/ph-curves/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/crates/l/ph-curves)](LICENSE.md)
[![MSRV](https://img.shields.io/badge/MSRV-1.92.0-blue)](https://github.com/photon-circus/ph-curves/blob/main/rust-toolchain.toml)
[![no_std](https://img.shields.io/badge/no__std-yes-green)](src/lib.rs)

`no_std`, zero-allocation curve lookup tables, ADC-to-measurement transfer
functions with inverse and calibration, temporal filters, and tickless
scheduling for embedded Rust.

## Features

- **Static LUTs** — curves are pre-computed at build time into `static` arrays,
  so evaluation is a single index into a `&'static [u8; 256]` (or `[u16; 65536]`).
- **`no_std` / `no_alloc`** — the library itself has zero runtime allocation.
  Only the `fixed` crate is required at runtime (fixed-point math).
- **Tickless scheduling** — computes the *next* wall-clock deadline where the
  quantized output value changes, so your firmware can sleep instead of polling.
- **Code-gen CLI** — a companion binary (`ph-curves-gen`) reads a simple TOML
  file and emits the Rust source for all your curves.
- **Physical transfer functions** — sparse adaptive knots convert integer ADC
  observations to signed, scaled measurements with explicit range behavior.
  Firmware uses only integer math; physical modeling and fitting are host-only.
- **Runtime calibration** — `AffineCalibration` applies a caller-supplied
  integer gain/offset/scale on top of any transfer, in both directions, without
  regenerating tables or touching NVM.
- **Temporal stabilization** — fixed-memory integer moving-average, median,
  exponential smoothing, and stability detection for caller-supplied samples.
- **Decision primitives** — `Hysteresis` and `Debounce` latch application
  decisions from sample-count cadence alone; no clock, no GPIO.

## Scope and non-goals

`ph-curves` is a pure math and scheduling-primitives crate, not a hardware
driver crate. It may map caller-provided observations, normalized positions,
and timestamps to values or future deadlines. It does not own or access ADCs,
GPIO, buses, clocks, timers, interrupts, async runtimes, sensors, actuators, or
device lifecycle. Hardware acquisition and application remain the caller's
responsibility.

New APIs must remain deterministic and side-effect-free: data in, data or
deadlines out. Sensor models in the host generator describe transfer
mathematics only; they must not grow into sensor configuration, sampling,
calibration storage, fault management, or device-specific driver behavior.

## Quick start

### 1. Define curves in TOML

Create a file (e.g. `assets/curves.toml`):

```toml
[curves.ease_in_quad]
builtin = "ease_in_quad"

[curves.gamma_22]
formula = "pow(t, 2.2)"

[curves.contrast_boost]
points = [[0, 0], [64, 32], [128, 128], [192, 224], [255, 255]]
```

Each curve uses exactly **one** of three definition styles:

| Style      | Description                                               |
|------------|-----------------------------------------------------------|
| `builtin`  | Name of a built-in easing function (14 available)         |
| `formula`  | Math expression in `t` (0→1), evaluated at build time     |
| `points`   | Piecewise-linear control points `[input, output]`         |

Set `monotonic = false` to skip inverse-LUT generation (default is `true`).
Curve and transfer names are normalized to uppercase Rust identifiers. Names
with no ASCII letters or digits, names that normalize to the same identifier,
and names that collide with generated companions (`_FWD`, `_INV`, `_INPUTS`,
`_OUTPUTS`, `_METADATA`, `_OBSERVATION_GUARD`) are rejected.

### 2. Generate Rust source

```sh
cargo install --path . --features gen-cli

ph-curves-gen --input assets/curves.toml --output src/curves.rs
```

This produces a `.rs` file with `static` arrays and `const` curve values
ready to `include!` or copy into your crate. LUTs must cover the complete value
domain: `u8` uses exactly 256 entries and `u16` uses exactly 65,536 entries.

For 16-bit resolution:

```sh
ph-curves-gen --input assets/curves.toml --output src/curves.rs \
    --value-type u16 --lut-size 65536
```

### 3. Use in firmware

```rust
use ph_curves::{Curve, MonotonicCurve, Tickless, Rounding};

include!("curves.rs");

// Simple evaluation — one table lookup.
let brightness: u8 = GAMMA_22.eval(input);

// Tickless scheduling — sleep until the next value change.
let schedule = EASE_IN_QUAD.tickless_schedule(
    0,           // t0_ms: start time
    1000,        // duration_ms
    0,           // start value
    255,         // end value
    10,          // step (quantization)
    Rounding::Nearest,
    0,           // min_dt_ms
);

for deadline in schedule.iter(0) {
    set_timer(deadline.deadline_ms);
    set_output(deadline.current_val);
}
```

## ADC-to-measurement transfer functions

Transfer functions are separate from normalized easing curves and
`UnitValue`. They accept a real `u16` input domain (raw ADC codes or explicitly
scaled voltage-like integers) and return signed `i32` measurement quanta.

The included `assets/transfers.toml` reference models a 10 kOhm, Beta 3950 NTC
thermistor in a 10 kOhm ratiometric divider on a 12-bit ADC:

```toml
[transfers.ntc_10k_beta_3950]
input_unit = "adc_code"
output_unit = "degree_celsius"
output_scale = 1000
max_interpolation_error = 50
max_knots = 256
below = "error"
above = "error"
output_range = [-40.0, 125.0]

[transfers.ntc_10k_beta_3950.model]
kind = "ntc_beta_divider"
nominal_resistance_ohms = 10000.0
beta_kelvin = 3950.0
nominal_temperature_celsius = 25.0
fixed_resistance_ohms = 10000.0
adc_max_code = 4095
topology = "ntc_to_ground"
```

Generate and use it:

```sh
ph-curves-gen --input assets/transfers.toml --output ntc_transfer.rs
```

```rust
use ph_curves::TransferFunction;

include!("ntc_transfer.rs");

let milli_celsius = NTC_10K_BETA_3950.convert(adc_code)?;
```

The reference generates 61 nonuniform knots over ADC codes `142..=3995`:
366 bytes of array payload rather than a 4,096- or 65,536-entry LUT. The
generator checks every integer ADC code and reports a measured worst-case
numerical error. Adaptive fitting defaults to at most 256 knots (configurable
up to an absolute 4,096-knot safety limit) and fails rather than silently
emitting a full domain table.

Knot selection is a bounded greedy heuristic: it repeatedly adds the input
with the current worst error. Exhaustive verification guarantees that every
emitted table meets the requested error, but reaching `max_knots` does not
prove that no alternative knot placement could meet it. Increase `max_knots`
or generate physical points with a domain-specific fitting tool when that
distinction matters.

`below` and `above` independently select `"error"` (the default) or `"clamp"`.
Transfer functions never extrapolate.

One exact observation code may have a separate guard. It is checked before
ordinary boundary behavior, must be strictly above the generated domain, and
is never inferred from the integer value:

```toml
[transfers]
requires = ["observation_guard_v1"]

[transfers.ambient_light]
input_unit = "count"
output_unit = "lux"
output_scale = 1
max_interpolation_error = 1
above = "clamp"
saturation = { code = 65535, behavior = "error" }
formula = "x"
domain = [1, 60000]
```

`behavior = "error"` returns `TransferError::RejectedObservation`;
`behavior = "clamp"` returns the output at `domain_max` regardless of
`above`. Every other out-of-domain code still follows `below` / `above`, and
inverse conversion is unchanged. Classification as saturation is explicit
consumer/device policy, not a generic rule for `u16::MAX`.

The `[transfers] requires` line is mandatory whenever a standalone transfer
uses `saturation`. Its array shape makes an older generator reject the whole
document instead of silently ignoring the guard. An unused capability or a
guard misplaced inside a model/point value is also rejected. A legacy transfer
named `requires` must be renamed before this capability can be declared.
Generated output exposes
`<NAME>_OBSERVATION_GUARD: Option<ObservationGuardMetadata>`; it agrees with
`<NAME>.observation_guard()` and is `None` for an unguarded transfer.

### What transfer functions enable

The transfer API is a good fit when all of the following are true:

- One `u16` integer observation determines one signed, scaled `i32` result.
- The relationship is static and monotonic, either increasing or decreasing.
- A formula, empirical calibration points, or a supported host model can
  describe the ideal relationship.
- Endpoint errors or clamps, plus at most one explicit above-domain guard, are
  sufficient outside the generated domain.
- Numerical interpolation error can be bounded independently from real-world
  sensor accuracy.

Examples include ADC code or integer millivolts to temperature, pressure,
resistance, illuminance, position, calibrated voltage, tank level, or a rough
user-facing battery charge estimate. The same primitives work for any unit;
the crate does not attach sensor-specific behavior to unit labels.

The strongest supported pipeline is:

`one integer observation -> one monotonic physical result -> optional temporal stabilization`

### Honest limitations

The transfer layer does **not** currently provide:

- Signed or wider-than-`u16` input domains, or outputs wider than `i32`.
- Nonmonotonic forward maps.
- Dense physical-domain inverse LUTs (inverse uses runtime search on the forward knots).
- Multidimensional compensation such as measurement by temperature or load.
- Runtime/factory gain-and-offset calibration wrappers.
- Automatic chaining or unit conversion between transfer functions.
- Sensor fusion, state estimation, hysteretic application decisions, or
  general missing/invalid-sample policy beyond the single explicit
  observation-code guard.
- A plugin interface for arbitrary host model code. Dedicated crates inspect
  the validated transfer graph and supply evaluated truth or prefitted knots
  through the host `gen-lib` IR instead.

Only the NTC Beta-divider has a built-in physical model. Other devices should
normally use a formula or empirical points. Dedicated crates may provide
domain-specific models and policies while emitting or consuming generic
`ph-curves` transfers.

### Writing a custom transfer

Use `assets/custom-transfers.toml` as a complete guide. A custom transfer has
six design steps:

1. Choose the integer input representation firmware already has, such as raw
   ADC code or millivolts. Include divider/reference calibration in the model
   if it is static.
2. Choose an output unit and integer scale. For example,
   `output_unit = "kilopascal"` with `output_scale = 1000` emits milli-kPa.
3. Define the valid input domain and explicit below/above behavior; optionally
   declare one exact above-domain observation guard.
4. Select either a formula over `x` or increasing-input physical points.
5. Set the numerical error target and a bounded knot budget.
6. Generate the table, inspect its reported domain/knot/error metadata, and
   validate it against independent reference measurements.

For an analytical sensor, use a formula:

```toml
[transfers.pressure_100kpa]
input_unit = "adc_code"
output_unit = "kilopascal"
output_scale = 1000
domain = [410, 3686]
formula = "(x - 410) * 100.0 / 3276.0"
max_interpolation_error = 1
max_knots = 32
```

The formula is evaluated only by the host generator. `x` is the integer input;
the existing formula operators/functions are available. The generated
firmware table contains no floating point.

For an empirical or piecewise model, use physical points:

```toml
[transfers.tank_level]
input_unit = "millivolt"
output_unit = "percent"
output_scale = 100
max_interpolation_error = 5
max_knots = 32
below = "clamp"
above = "clamp"
points = [
  { input = 500, output = 0.0 },
  { input = 1200, output = 28.0 },
  { input = 2050, output = 82.0 },
  { input = 2500, output = 100.0 },
]
```

Point inputs must be strictly increasing and outputs must be monotonic.
Endpoints define the valid domain; unlike normalized easing curves, physical
points do not need to start at zero or end at full scale.

Discrete selector combinations that share one source belong in a transfer
family. Selectors are never interpolated. Only `status = "emit"` members are
generated. `unnecessary` and `forbidden` members stay on the description,
require a non-blank `reason`, and retain a validated source mapping.
`unsupported` members also require a reason but set no applicability coordinate
or `input_transform` because no source mapping exists. `[gaps]` records
channels the sources leave undefined:

```toml
[transfer_families.als]
input_unit = "count"
output_unit = "unit"
output_scale = 1000
max_interpolation_error = 50
formula = "x"

[[transfer_families.als.members]]
selectors = { gain = "div4", integration_time_ms = 100 }
status = "emit"
applicability = { observation = [1, 10] }

[gaps.white_channel]
status = "undefined"
reason = "counts only; no conversion"
```

Families may share a document with unrelated `[curves]`. Dense LUT generation
stays curve-only; family members remain sparse integer transfers. Family knot
default is 64 with a hard cap of 256. Every accepted member field is
source-aware: formula and points members declare `applicability.observation`,
NTC members declare `applicability.physical`, and `kind = "scaled_polynomial"`
requires an exact `input_transform` plus `applicability.model_input`. The
matrix applies to mapped (`emit`, `unnecessary`, and `forbidden`) members;
`unsupported` members carry selector identity and a reason only. Unknown
fields in family point entries and NTC model tables fail closed, while the
legacy standalone source formats retain their compatibility behavior. The
generator applies that transform as `u = count * numerator / denominator` with
an exact integer product, and converts inclusive model-input bounds to
observation codes:

```toml
[transfer_families.als.model]
kind = "scaled_polynomial"
coefficients = [0.0, 1.0023, 8.1488e-5, -9.3924e-9, 6.0135e-13]

[[transfer_families.als.members]]
selectors = { gain = "div4", integration_time_ms = 100 }
status = "emit"
input_transform = { numerator = 268800, denominator = 1000000 }
applicability = { model_input = [100.0, 22000.0] }
```

Standalone polynomial definitions supply their own `scale` and `domain`. The
generic polynomial evaluator includes `u16::MAX` whenever that declared domain
includes it; there is no implicit saturation rule. A guard may target that code
only when the fitted `domain_max` is lower. A family-level `saturation` table is
copied to every emitted member and validated against each member's domain.
Generated output is still independent `PiecewiseLinearTransfer` constants.
Inspect parsed families and gaps through
`DefinitionsFile::transfer_families` and `gaps`. `DefinitionsFile::validate`
returns a `ValidatedDefinitions` graph that includes description-only members
and gap reasons. A host tool that owns device evaluation can overlay
`TransferSource::evaluated_truth` or prefitted knots on an emitted member and
still receive ordinary generated tables. A family-member overlay must span
the member's resolved observation domain; a standalone overlay replaces its
declared source and may define a different domain. Transfer-only generation
uses `GenerateOptions::transfers_only()`; curve LUT `value_type` / `lut_size`
are not required.

If a model needs conditionals, multiple independent inputs, dynamic
calibration, temperature/load compensation, or domain-specific state, compute
calibration points or dense truth in a dedicated host tool/crate and feed them
to the generic generator through `TransferSpec` / `TransferSource`. Do not turn
`ph-curves` into a device driver or an open-ended sensor-model catalog.

### Accuracy scope

The generated error bound covers integer output quantization and interpolation
against the configured ideal formula, point set, or model. It is **not** total
sensor accuracy. For an NTC system, separately account for Beta-model error,
thermistor and resistor tolerance, ADC/reference error, self-heating, wiring,
and calibration uncertainty.

All floating-point formulas, models, fitting, and error analysis are compiled
only behind the host `gen-lib` feature (library API) / `gen-cli` (binary). The
default library and generated firmware code contain integer arrays, binary
search, and `i64` interpolation only.

### Host features and the runtime guarantee

| Feature | Pulls in | Use |
| ------- | -------- | --- |
| *(none)* | — | Firmware. `no_std`, no allocator, integer-only. |
| `gen-lib` | serde, toml | `build.rs` and host tools calling `ph_curves::r#gen`. |
| `gen-cli` | `gen-lib` + clap | Building or installing the `ph-curves-gen` binary. |
| `gen` | `gen-cli` | 0.1.x compatibility alias. Prefer `gen-lib` in a build script. |

`#![no_std]` is unconditional and **no feature relaxes it**. The host features
link `std` only inside `src/gen` (module-local `extern crate std` and explicit
imports); the crate root does not `extern crate std`. A crate elsewhere in
your dependency graph enabling `ph-curves/gen-lib` therefore cannot turn your
firmware build into a `std` build via Cargo's feature unification. CI enforces
this by building the default feature set against a `core`-only sysroot
(`-Z build-std=core`), which fails if anything on the runtime path reaches for
`alloc` or `std`.

### `build.rs` integration

```toml
[build-dependencies]
ph-curves = { version = "0.2", features = ["gen-lib"] }
```

```rust
// build.rs
use std::env;
use std::path::PathBuf;
use ph_curves::r#gen::{generate_to_path, GenerateOptions};

fn main() {
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("curves.rs");
    generate_to_path("assets/curves.toml", &out, &GenerateOptions::default())
        .expect("ph-curves gen");
    println!("cargo:rerun-if-changed=assets/curves.toml");
}
```

```rust
// firmware lib.rs — no host features; still no_std + no_alloc
include!(concat!(env!("OUT_DIR"), "/curves.rs"));
```

### Runtime calibration

Per-unit trim lives outside the generated table. `AffineCalibration` applies a
caller-supplied integer triple — read from EEPROM, flash, or a test fixture —
as `y' = (y * gain + offset) / scale`, with the same nearest, ties-away
rounding as the table itself. It never reads NVM, regenerates knots, or edits
`TransferMetadata`.

```rust
use ph_curves::{AffineCalibration, InverseTransferFunction, TransferFunction};

// +0.5 % gain, -120 milli-Celsius offset, from this unit's factory trim.
let trimmed = AffineCalibration::new(NTC_10K_BETA_3950, 1_005, -120, 1_000)?;

let milli_celsius = trimmed.convert(adc_code)?;   // calibrated reading
let setpoint_code = trimmed.invert(25_000)?;      // calibrated setpoint
```

Inversion undoes the affine and then inverts the table, so a calibrated
setpoint is one call rather than hand-rolled arithmetic. Range errors are
reported in *calibrated* units so the bounds are comparable with the value you
passed in, and a calibration whose `gain` and `scale` have opposite signs
reverses orientation, flipping `BelowRange` and `AboveRange` accordingly.
`gain == 0` is rejected at construction — it collapses every observation onto
one value and has no inverse.

Both directions round, so a convert-then-invert round trip is bounded rather
than exact. Anything `convert` produces is guaranteed invertible; a value
within half an uncalibrated quantum of a range endpoint clamps to that endpoint
instead of failing. `TransferMetadata::achieved_max_inverse_code_error`
describes the *uncalibrated* table — wrapping it in a calibration can widen
that bound.

## Temporal stabilization

A transfer function converts one observation. Meaningful measurements often
need several observations to suppress noise, reject spikes, or determine that
a signal has settled. `ph-curves` provides caller-driven, fixed-memory
primitives without acquiring samples or owning a clock:

```rust
use ph_curves::{MedianFilter, Stability, StabilityDetector, TemporalFilter};

let mut median = MedianFilter::<u16, 5>::new();
let mut stable = StabilityDetector::<i32, 4>::new(100); // 0.1 C in milli-C

if let Some(filtered_adc) = median.update(adc_code).ready() {
    let milli_celsius = NTC_10K_BETA_3950.convert(filtered_adc)?;
    if matches!(stable.update(milli_celsius), Stability::Stable { .. }) {
        use_measurement(milli_celsius);
    }
}
```

Available primitives:

- `MovingAverage<T, N>`: `O(1)` exact fixed-window mean with explicit warm-up.
- `MedianFilter<T, N>`: robust isolated-spike rejection for small odd windows.
- `ExponentialSmoother<T>`: constant-memory smoothing with an explicit integer
  blend coefficient (`alpha` in `0..=65535`). Because the update is quantized
  as `round(delta * alpha / 65535)`, small steps can produce a zero adjustment
  when `|delta| * alpha < 32768`. Prefer a larger `alpha`, or a moving average /
  median, when tracking fine ADC or milli-unit noise.
- `StabilityDetector<T, N>`: reports warming, stable, or unstable from the
  recent range; it never substitutes a stale last-good value.

Filtering raw ADC codes and filtering converted measurements are intentionally
separate composition choices. For a nonlinear transfer,
`transfer(mean(raw))` generally differs from `mean(transfer(raw))`. Raw-domain
filtering suppresses acquisition noise before conversion; physical-domain
filtering expresses windows and thresholds in measurement units. A median is
order-based and therefore composes predictably with monotonic transfers,
apart from integer rounding.

Window sizes count caller-supplied valid samples. Sampling cadence, invalid
sample policy, transfer errors, and whether instability resets application
state remain caller responsibilities. The crate does not read timestamps or
silently assume a sample rate.

### Decision primitives

Filters smooth a value; deciding what to *do* is separate. `Hysteresis` and
`Debounce` latch boolean decisions from sample-count cadence only — they read
no clock and own no GPIO.

```rust
use ph_curves::{Debounce, DebounceOutput, Hysteresis};

// Fan on at 60 C, off at 55 C. The 5 C band stops chatter at the threshold.
let mut fan = Hysteresis::<i32>::new(55_000, 60_000);
let fan_on = fan.update(milli_celsius);

// Require 3 consecutive agreeing samples before acting on a fault line.
let mut fault = Debounce::<3>::new();
match fault.update(raw_fault) {
    DebounceOutput::Edge { level } => latch_fault(level),
    DebounceOutput::Steady(_) | DebounceOutput::WarmingUp { .. } => {}
}
```

`Hysteresis` needs `low <= high`; equal thresholds degrade to a plain
comparison. `Debounce` reports `WarmingUp` until it has seen `N` consecutive
matching samples, then `Edge` exactly once per confirmed transition and
`Steady` otherwise, so callers can act on changes rather than re-applying a
level every sample.

## Built-in curves

| Name                 | Formula              | Description                      |
|----------------------|----------------------|----------------------------------|
| `linear`             | `t`                  | Identity / straight line         |
| `ease_in_quad`       | `t²`                 | Quadratic ease-in                |
| `ease_out_quad`      | `1-(1-t)²`           | Quadratic ease-out               |
| `ease_in_out_quad`   | piecewise quadratic  | Quadratic ease-in-out            |
| `ease_in_cubic`      | `t³`                 | Cubic ease-in                    |
| `ease_out_cubic`     | `1-(1-t)³`           | Cubic ease-out                   |
| `ease_in_out_cubic`  | piecewise cubic      | Cubic ease-in-out                |
| `ease_in_quart`      | `t⁴`                 | Quartic ease-in                  |
| `ease_out_quart`     | `1-(1-t)⁴`           | Quartic ease-out                 |
| `ease_in_out_quart`  | piecewise quartic    | Quartic ease-in-out              |
| `ease_in_expo`       | `2^(10(t-1))`        | Exponential ease-in              |
| `ease_out_expo`      | `1-2^(-10t)`         | Exponential ease-out             |
| `smoothstep`         | `3t²-2t³`            | Hermite smoothstep               |
| `smoother_step`      | `6t⁵-15t⁴+10t³`     | Ken Perlin's improved smoothstep |

Legacy aliases: `ease_in`, `ease_out`, `ease_in_out` (mapped to the quad
variants).

## Formula syntax

Formulas are math expressions over the variable `t` (0.0 to 1.0).

**Operators:** `+` `-` `*` `/` `^` (or `**`), unary `-`, parentheses.

**Functions:** `pow(x,y)` `sqrt(x)` `abs(x)` `min(x,y)` `max(x,y)`
`clamp(x,lo,hi)` `sin(x)` `cos(x)` `tan(x)` `exp(x)` `ln(x)` `log2(x)`

**Constants:** `pi` `e`

```toml
[curves.cie_lightness]
formula = "pow((t + 0.16) / 1.16, 3.0)"
```

## Library API

### Core types

| Type                   | Description                                         |
|------------------------|-----------------------------------------------------|
| `CurveLut<I,V,N,M>`   | Forward LUT + optional inverse LUT                  |
| `MonotonicCurveLut<I,V,N,M>` | Forward + required inverse LUT                |
| `CurveLut256`          | Type alias: `CurveLut<u8, u8, 256>`                |
| `MonotonicCurveLut256` | Type alias: `MonotonicCurveLut<u8, u8, 256>`        |
| `CurveLut65536`        | Type alias: `CurveLut<u16, u16, 65536>`             |
| `MonotonicCurveLut65536` | Type alias: `MonotonicCurveLut<u16, u16, 65536>`  |
| `PiecewiseLinearTransfer<N>` | Sparse integer ADC↔measurement transfer (forward + inverse) |
| `ObservationGuard`         | Explicit observation-code policy, independent of `below`/`above` |
| `ObservationGuardMetadata` | Adjacent optional guard facts (not a `TransferMetadata` field) |
| `TransferMetadata`       | Units, scale, domain/range, flats, and error bounds |
| `FlatResolution`         | Policy for non-unique (flat) inverse outputs      |
| `AffineCalibration<T>`   | Gain/offset/scale wrapper over any transfer, invertible |
| `MovingAverage<T,N>`     | Exact fixed-window integer mean                  |
| `MedianFilter<T,N>`      | Small fixed-window outlier rejection             |
| `ExponentialSmoother<T>` | Constant-memory integer smoothing                |
| `StabilityDetector<T,N>` | Independent recent-range stability classification |
| `Hysteresis<T>`          | Dual-threshold latch with a hold band             |
| `Debounce<N>`            | N-consecutive-sample confirmation, with edge reporting |

### Traits

- **`Curve<I, V>`** — `eval(u: I) -> V` — single table lookup.
- **`MonotonicCurve<I, V>`** — adds `inv(w: V) -> I` — inverse lookup.
- **`Tickless<T>`** — adds `tickless_schedule(...)` to any `MonotonicCurve`.
- **`UnitValue`** — implemented for `u8` and `u16`; maps the unit interval
  onto a discrete integer range with fixed-point helpers.
- **`TransferFunction`** — checked physical conversion with explicit
  below/above-domain behavior and no extrapolation.
- **`InverseTransferFunction`** — physical → observation invert on the same
  sparse knots (`invert` / `invert_physical`), with `FlatResolution` for
  plateaus.
- **`TemporalFilter`** — caller-driven update/reset interface with explicit
  warm-up output.

### Tickless scheduling

`TicklessSchedule` computes the exact deadline (in milliseconds) at which the
quantized output value will next change. This lets interrupt-driven firmware
sleep between value changes instead of polling at a fixed tick rate.

Timestamps are free-running `u32` milliseconds: schedule math uses wrapping
addition/subtraction so a segment may start near `u32::MAX` and cross the
~49.7-day rollover. For durations up to `i32::MAX` ms (~24.85 days), before /
after classification uses the usual half-range signed-delta convention.
Longer durations remain valid when `now_ms` is segment-relative elapsed time
with `t0_ms == 0` (a wrapping wall clock cannot uniquely represent a single
segment longer than half the clock period). Deadlines themselves are clamped
on offsets from `t0_ms`, which stay bounded by `duration_ms`, so the full
`u32` duration range works in either mode.

`deadline_ms` may be numerically smaller than `now_ms` when a deadline falls
past the rollover. Compare with wrapping remaining-time
(`deadline_ms.wrapping_sub(now_ms)`) rather than absolute ordering; the same
applies to `end_ms()`, which now wraps rather than saturating.

Supports `RepeatMode::Once`, `RepeatMode::Repeat`, and `RepeatMode::PingPong`.

### Segment helpers

- `interpolate_segment(input, x0, y0, x1, y1)` — one signed segment forward.
- `invert_segment(physical, x0, y0, x1, y1)` — its mirror. Host tools use it so
  a generated round-trip audit rounds exactly the way the runtime does.

### Math helpers

- `lerp_u8(a, b, w)` / `lerp_u16(a, b, w)` — interpolate with `u8` weight.
- `map_u8_to_u16(w, max)` — scale a `u8` into a `u16` range.
- `quantize(value, step, rounding)` — snap to a step size.
- `next_target_value(current, end, step, increasing)` — next quantized target.

## Minimum supported Rust version

Rust **1.92.0** (edition 2024).

## Contributing

Contributions are welcome! Please read the [contributing guide](CONTRIBUTING.md)
before opening a pull request.

### CI

Every pull request runs [`.github/workflows/ci.yml`](https://github.com/photon-circus/ph-curves/blob/main/.github/workflows/ci.yml):
format, clippy at `-D warnings` across the feature matrix, tests, rustdoc,
no-std and ESP32 target builds, dependency policy, and packaging.

Two jobs guard the crate's core promises. **`runtime-purity`** rejects a
feature-conditional `#![no_std]` and builds the default feature set against a
`core`-only sysroot, which is what proves *no-alloc* — a plain `--target`
build only proves *no-std*, because bare-metal `rust-std` ships `alloc`.
**`feature-compat`** runs the 0.1.x `cargo run --features gen` invocation so
the compatibility alias cannot rot.

To run the same gate locally before pushing:

```powershell
./scripts/local-ci.ps1
```

This project follows the
[Contributor Covenant Code of Conduct](CODE_OF_CONDUCT.md). By participating you
agree to uphold it.

For security issues, see our [security policy](SECURITY.md).

## License

[MIT](LICENSE.md)
