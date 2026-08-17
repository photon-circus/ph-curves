# ph-curves

[![Crates.io](https://img.shields.io/crates/v/ph-curves)](https://crates.io/crates/ph-curves)
[![docs.rs](https://img.shields.io/docsrs/ph-curves)](https://docs.rs/ph-curves)
[![CI](https://github.com/photon-circus/ph-curves/actions/workflows/ci.yml/badge.svg)](https://github.com/photon-circus/ph-curves/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/crates/l/ph-curves)](LICENSE.md)
[![MSRV](https://img.shields.io/badge/MSRV-1.92.0-blue)](https://github.com/photon-circus/ph-curves/blob/main/rust-toolchain.toml)
[![no_std](https://img.shields.io/badge/no__std-yes-green)](src/lib.rs)

`ph-curves` turns integer observations and normalized positions into
deterministic firmware values: static curves, sparse ADC-to-measurement
transfers, runtime calibration, fixed-memory stabilization, hysteretic
decisions, and tickless deadlines.

The host generator may model and fit with floating point, but firmware receives
only static integer tables and bounded state. The runtime is always `no_std`
and allocation-free.

## Start here

Choose the path that matches the job:

| I need to… | Quick start | Main API |
| --- | --- | --- |
| Generate and evaluate a normalized curve | [Generate a curve](#quick-start-generate-a-curve) | `Curve`, `MonotonicCurve` |
| Convert an ADC code into a physical measurement | [Convert an ADC observation](#quick-start-convert-an-adc-observation) | `TransferFunction`, `InverseTransferFunction` |
| Apply per-unit gain/offset after conversion | [Calibrate a measurement](#quick-start-calibrate-a-measurement) | `AffineTransform`, `AffineCalibration` |
| Smooth a measurement and make a stable decision | [Stabilize and decide](#quick-start-stabilize-and-decide) | `MovingAverage`, `StabilityDetector`, `Hysteresis` |
| Sleep until a quantized curve output changes | [Schedule without polling](#quick-start-schedule-without-polling) | `Tickless`, `TicklessSchedule` |
| Generate tables from Cargo rather than a shell command | [Generate from build.rs](#quick-start-generate-from-buildrs) | `ph_curves::r#gen` |
| Describe hardware variants and intentional gaps together | [Model a transfer family](#quick-start-model-a-transfer-family) | `FamilySpec`, validated family IR and reports |

## Features

The features are ordered from build-time definition through the runtime
measurement pipeline:

| Stage | Capability | Value |
| --- | --- | --- |
| Foundation | **`no_std` / `no_alloc` runtime** | No allocator, hidden I/O, clock, GPIO, or async-runtime dependency; `fixed` is the only runtime dependency. |
| Generate | **CLI and `build.rs` code generation** | TOML, programmatic specifications, fitting, provenance, manifests, and resource reports stay on the host. |
| Lookup | **Static curves** | A normalized curve evaluation is one index into a complete-domain static LUT. |
| Convert | **Sparse physical transfers** | Adaptive integer knots map `u16` observations to signed, scaled `i32` measurements with explicit boundaries and inverse lookup. |
| Calibrate | **Runtime affine correction** | Caller-supplied integer gain, offset, and scale correct an existing measurement or wrap a transfer without regenerating tables. |
| Stabilize | **Fixed-memory temporal filters** | Moving average, median, exponential smoothing, and independent stability classification consume caller-supplied integer samples. |
| Decide | **Hysteresis and debounce** | Latch application decisions from sample cadence without owning hardware or time. |
| Schedule | **Tickless deadlines** | Compute the next wall-clock instant at which a quantized curve output changes so firmware can sleep instead of polling. |

## Install

Firmware normally uses the default, runtime-only crate:

```toml
[dependencies]
ph-curves = "0.3"
```

Host generation is opt-in:

| Feature | Use |
| --- | --- |
| *(none)* | Firmware runtime: `no_std`, no allocation, integer-only. |
| `gen-lib` | Host tools and `build.rs`; adds serde and TOML parsing. |
| `gen-cli` | `gen-lib` plus the `ph-curves-gen` binary. |
| `gen` | Compatibility alias for the 0.1.x CLI feature. |

`#![no_std]` is unconditional. Cargo feature unification cannot turn the
firmware runtime into a `std` build when another crate enables a host feature.

## Quick starts

### Quick start: generate a curve

Create `assets/curves.toml`:

```toml
[curves.gamma_22]
formula = "pow(t, 2.2)"

[curves.ease_in_quad]
builtin = "ease_in_quad"
```

Install or run the generator:

```sh
cargo install ph-curves --version 0.3.0 --features gen-cli --bin ph-curves-gen
ph-curves-gen --input assets/curves.toml --output src/curves.rs
```

Use the generated constant:

```rust
use ph_curves::{Curve, MonotonicCurve};

include!("curves.rs");

let brightness: u8 = GAMMA_22.eval(input);
let input_again: u8 = GAMMA_22.inv(brightness);
```

Curve definitions use exactly one of `builtin`, `formula`, or `points`.
Monotonic curves also emit inverse data. See the checked-in
[curve definitions](https://github.com/photon-circus/ph-curves/blob/main/assets/curves.toml)
for complete examples.

The [curve-generation guide](https://github.com/photon-circus/ph-curves/blob/main/docs/guides/curve-generation.md)
lists every built-in curve, the formula language, naming rules, and LUT target
constraints.

### Quick start: convert an ADC observation

Generate the reference NTC transfer:

```sh
ph-curves-gen --input assets/transfers.toml --output src/transfers.rs
```

```rust
use ph_curves::{InverseTransferFunction, TransferFunction};

include!("transfers.rs");

let milli_celsius = NTC_10K_BETA_3950.convert(adc_code)?;
let code_for_25_c = NTC_10K_BETA_3950.invert(25_000)?;
```

The reference uses 61 adaptive knots over a 12-bit ADC domain rather than a
4,096-entry dense table. The generator checks every integer code and fails if
the requested interpolation error cannot be met within the knot budget.

Use a transfer when one monotonic `u16` observation determines one signed,
scaled `i32` result. Formula, physical-point, and supported model fitting are
host-only; generated firmware uses binary search and checked `i64`
interpolation. Start from
[the transfer examples](https://github.com/photon-circus/ph-curves/blob/main/assets/custom-transfers.toml).

The [physical-transfer guide](https://github.com/photon-circus/ph-curves/blob/main/docs/guides/physical-transfer-generation.md)
covers source selection, fitting, guards, accuracy scope, and non-goals.

### Quick start: calibrate a measurement

Use `AffineTransform` when a measurement is already converted:

```rust
use ph_curves::AffineTransform;

// y' = (y * 1_005 - 120_000) / 1_000
let trim = AffineTransform::new(1_005, -120_000, 1_000).unwrap();
let corrected_milli_celsius = trim.apply(25_000).unwrap();
let original = trim.unapply(corrected_milli_celsius).unwrap();
assert!((original - 25_000).abs() <= 1);
```

Apply calibration before mutating temporal state so an affine overflow cannot
insert a sample into a filter window. `AffineCalibration<T>` provides the same
arithmetic around a `TransferFunction` and supports inverse conversion when
the inner transfer does.

### Quick start: stabilize and decide

This policy smooths already-converted unsigned measurements, independently
classifies stability, and changes the latch only while stable:

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

Warm-up, missing samples, invalid samples, reset behavior, cadence, and whether
to hold or clear a decision during instability are caller policy. The filters
own fixed, const-generic state only.

The [measurement-pipeline guide](https://github.com/photon-circus/ph-curves/blob/main/docs/guides/measurement-pipelines.md)
compares the primitives, their fixed state and update cost, warm-up behavior,
and caller-owned reset policy.

### Quick start: schedule without polling

Any monotonic curve can produce deadlines for its quantized output changes:

```rust
use ph_curves::{Rounding, Tickless};

include!("curves.rs");

let schedule = EASE_IN_QUAD.tickless_schedule(
    0,                 // segment start, milliseconds
    1_000,             // duration
    0,                 // start value
    255,               // end value
    10,                // output quantum
    Rounding::Nearest,
    0,                 // minimum deadline spacing
);

for deadline in schedule.iter(0) {
    set_timer(deadline.deadline_ms);
    set_output(deadline.current_val);
}
```

Timestamps are wrapping `u32` milliseconds. Compare deadlines with wrapping
remaining time rather than absolute numeric ordering across clock rollover.

The [tickless-scheduling guide](https://github.com/photon-circus/ph-curves/blob/main/docs/guides/tickless-scheduling.md)
explains rollover-safe comparisons, duration bounds, quantization, and repeat
modes.

### Quick start: generate from build.rs

```toml
[build-dependencies]
ph-curves = { version = "0.3", features = ["gen-lib"] }
```

```rust
// build.rs
use std::{env, path::PathBuf};
use ph_curves::r#gen::{generate_to_path, GenerateOptions};

fn main() {
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("curves.rs");
    generate_to_path("assets/curves.toml", &out, &GenerateOptions::default())
        .expect("generate curves");
    println!("cargo:rerun-if-changed=assets/curves.toml");
}
```

```rust
// Firmware target; no host feature is enabled here.
include!(concat!(env!("OUT_DIR"), "/curves.rs"));
```

The library and CLI share the same parse, validation, fitting, and emission
pipeline, so the same input produces the same Rust source.

### Quick start: model a transfer family

Transfer families describe discrete hardware variants that share one source
model without pretending selector values are interpolated dimensions. Start
with the complete
[family acceptance document](https://github.com/photon-circus/ph-curves/blob/main/assets/family-acceptance.toml):

```sh
ph-curves-gen \
  --input assets/family-acceptance.toml \
  --output src/front_end_transfers.rs
```

Each expected typed-selector identity must appear exactly once as an emitted or
description-only member, or as an explicit family gap with a reason. Stable
emitted names and the emission manifest identify firmware symbols before
fitting; structured provenance, resource budgets, and generation reports audit
the fitted result before flashing.

Every family TOML document declares:

```toml
[transfers]
requires = ["transfer_families_v1"]
```

That marker makes released 0.2.1 readers reject the document instead of
silently ignoring the family table. See the
[transfer-family guide](https://github.com/photon-circus/ph-curves/blob/main/docs/design/transfer-families.md)
and
[host IR guide](https://github.com/photon-circus/ph-curves/blob/main/docs/design/host-transfer-ir.md)
for the schema, programmatic `FamilySpec` construction, overlays, reports, and
provenance rules.

## How the pieces compose

The common measurement path is:

`u16 observation → transfer → optional affine calibration → optional filter → stability classification → optional hysteretic/debounced decision`

Each stage remains independent:

- A transfer converts one observation and owns no temporal state.
- Affine calibration applies caller-provided coefficients and owns no NVM.
- A filter changes a value but does not declare it stable.
- A stability detector classifies its own recent window.
- Hysteresis and debounce change state only when the caller updates them.
- Tickless scheduling maps a time-varying curve to deadlines; it does not own a
  clock or timer.

Ordering is intentional. Nonlinear transfer and filtering do not commute, and
even affine correction can differ across integer stages because each stage
rounds. Choose the domain in which thresholds and spans should be expressed.

## Guarantees and limits

- **Pure runtime:** no hardware access, allocation, hidden I/O, interrupts,
  clocks, async runtime, or device lifecycle.
- **Integer firmware:** floating-point formulas, models, fitting, and error
  analysis are host-only.
- **Bounded numerical error, not sensor accuracy:** separately account for
  sensor tolerance, ADC/reference error, self-heating, wiring, and calibration
  uncertainty.
- **Transfer shape:** one static monotonic `u16` input to one `i32` output.
  Multidimensional compensation, nonmonotonic maps, fusion, and state
  estimation belong in application or domain-specific crates.
- **Caller-owned policy:** units, acquisition, cadence, missing/invalid sample
  handling, reset, calibration storage, and hardware action remain outside the
  crate.
- **Complete-domain dense LUTs:** `u8` uses 256 entries and `u16` uses
  65,536. A full `u16` LUT requires a pointer width of at least 32 bits; use a
  sparse transfer or smaller domain on 16-bit-pointer targets.
- **Fail-closed host schema:** family, standalone guard, and provenance
  capabilities prevent older generators from silently discarding safety or
  identity information.

The complete migration contract is in
[docs/compatibility.md](https://github.com/photon-circus/ph-curves/blob/main/docs/compatibility.md).
The [documentation map](https://github.com/photon-circus/ph-curves/blob/main/docs/README.md)
routes from each use case to the relevant guide, design record, or API
reference.

## API map

Everything in the runtime is re-exported at the crate root. Detailed contracts,
errors, panic conditions, and compiled examples live on
[docs.rs](https://docs.rs/ph-curves).

| Area | Main types and traits |
| --- | --- |
| Curves | `Curve`, `MonotonicCurve`, `CurveLut`, `MonotonicCurveLut` |
| Transfers | `TransferFunction`, `InverseTransferFunction`, `PiecewiseLinearTransfer`, `TransferMetadata`, `ObservationGuard` |
| Calibration | `AffineTransform`, `AffineCalibration` |
| Temporal | `TemporalFilter`, `MovingAverage`, `MedianFilter`, `ExponentialSmoother`, `StabilityDetector` |
| Decisions | `Hysteresis`, `Debounce` |
| Scheduling | `Tickless`, `TicklessSchedule`, `TicklessIter`, `RepeatMode` |
| Host generation | `DefinitionsFile`, `TransferSpec`, `FamilySpec`, `GenerateOptions`, generation reports and manifests under `ph_curves::r#gen` |

## Reference inputs

- [Normalized curves](https://github.com/photon-circus/ph-curves/blob/main/assets/curves.toml)
- [Full-domain u16 curves](https://github.com/photon-circus/ph-curves/blob/main/assets/curves-u16.toml)
- [NTC transfer](https://github.com/photon-circus/ph-curves/blob/main/assets/transfers.toml)
- [Custom formula and point transfers](https://github.com/photon-circus/ph-curves/blob/main/assets/custom-transfers.toml)
- [Observation guards](https://github.com/photon-circus/ph-curves/blob/main/assets/observation-guards.toml)
- [Transfer-family acceptance document](https://github.com/photon-circus/ph-curves/blob/main/assets/family-acceptance.toml)

## Minimum supported Rust version

Rust **1.92.0** (edition 2024).

## Contributing

Contributions are welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md), the
[security policy](SECURITY.md), and the
[code of conduct](CODE_OF_CONDUCT.md) before participating. Run the same gate
as CI with:

```powershell
./scripts/local-ci.ps1
```

## License

[MIT](LICENSE.md)
