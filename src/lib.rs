//! `no_std`, zero-allocation curve lookup tables, physical transfer functions,
//! and tickless scheduling for embedded Rust.
//!
//! `ph-curves` stores pre-computed forward (and optionally inverse) lookup
//! tables as `static` arrays so that curve evaluation reduces to a single
//! array index.  Combined with the tickless scheduler, interrupt-driven
//! firmware can sleep between value transitions instead of polling at a
//! fixed tick rate.
//!
//! # Quick start
//!
//! Use the companion CLI (`ph-curves-gen`) to generate `static` LUTs from a
//! TOML definition file, then `include!` the output in your crate:
//!
//! ```ignore
//! use ph_curves::{Curve, MonotonicCurve, Tickless, Rounding};
//!
//! include!("curves.rs");
//!
//! // Forward evaluation — a single table lookup.
//! let brightness: u8 = GAMMA_22.eval(input);
//!
//! // Inverse lookup (monotonic curves only).
//! let input: u8 = GAMMA_22.inv(brightness);
//!
//! // Tickless scheduling — sleep until the next quantized value change.
//! let schedule = EASE_IN_QUAD.tickless_schedule(
//!     0,     // t0_ms
//!     1000,  // duration_ms
//!     0,     // start_val
//!     255,   // end_val
//!     10,    // step (quantization)
//!     Rounding::Nearest,
//!     0,     // min_dt_ms
//! );
//!
//! for deadline in schedule.iter(0) {
//!     set_timer(deadline.deadline_ms);
//!     set_output(deadline.current_val);
//! }
//! ```
//!
//! # Key types
//!
//! Everything is re-exported at the crate root.
//!
//! - **Curves** — [`Curve`] / [`MonotonicCurve`] traits and the LUT-backed
//!   [`CurveLut`] / [`MonotonicCurveLut`] types.
//! - **Tickless scheduling** — [`Tickless`] extension trait,
//!   [`TicklessSchedule`], and the [`TicklessIter`] iterator.
//! - **Physical transfer functions** — [`TransferFunction`] /
//!   [`InverseTransferFunction`] and the sparse, integer-only
//!   [`PiecewiseLinearTransfer`] for ADC ↔ measurement conversion, plus
//!   [`AffineCalibration`] for caller-supplied gain/offset after a transfer
//!   and [`AffineTransform`] for the same arithmetic on an existing `i32`.
//! - **Temporal stabilization** — [`MovingAverage`], [`MedianFilter`],
//!   [`ExponentialSmoother`], [`StabilityDetector`], [`Hysteresis`], and
//!   [`Debounce`] over caller-supplied integer samples.
//! - **Math helpers** — [`UnitValue`] trait, [`lerp_u8`], [`lerp_u16`],
//!   [`map_u8_to_u16`], [`quantize`], and [`next_target_value`].
//!
//! # Scope
//!
//! This crate provides pure mappings and scheduling calculations, not hardware
//! drivers. It never owns or accesses ADCs, GPIO, buses, clocks, timers,
//! interrupts, async runtimes, sensors, or actuators. Callers provide
//! observations and timestamps, then decide how to acquire inputs, schedule
//! wakeups, and apply outputs.
//!
//! # Temporal stabilization
//!
//! Filters consume samples supplied by the caller and retain bounded,
//! const-generic state. Sample types are [`u16`], [`i32`], and [`u32`].
//! Windowed filters return [`FilterOutput::WarmingUp`] until ready. Stability
//! classification is separate from smoothing so a filtered value is not
//! implicitly treated as settled. [`Hysteresis`] and [`Debounce`] latch
//! application decisions from sample-count cadence only; they do not live
//! inside [`TransferFunction`] and never own GPIO or clocks.
//!
//! # Physical measurements
//!
//! Transfer functions are deliberately separate from normalized curves. The
//! host-only generator may use floating point to fit a physical model, but it
//! emits only `u16` input knots and signed `i32` output knots. Firmware
//! conversion uses binary search and checked-range `i64` interpolation.
//! Formula and empirical-point sources let users describe custom monotonic
//! models without adding sensor-specific runtime code.
//!
//! The transfer layer is intentionally limited to one static `u16` input and
//! one monotonic `i32` output, with runtime inverse on the same knots.
//! [`AffineCalibration`] applies a caller-supplied integer gain/offset/scale
//! after the table without regenerating knots or touching NVM. The same
//! arithmetic is available as [`AffineTransform`] on an already-converted
//! `i32` measurement; those coefficients are caller runtime state, not
//! generated-table metadata. The crate does not provide nonmonotonic maps,
//! multidimensional compensation, calibration discovery, sensor fusion, or
//! device policy. Those concerns belong in application or domain-specific
//! crates that compose with this crate's generic primitives.
//!
//! ```ignore
//! use ph_curves::{InverseTransferFunction, TransferFunction};
//!
//! include!("ntc_transfer.rs");
//!
//! let milli_celsius = NTC_10K_BETA_3950.convert(adc_code)?;
//! let setpoint_code = NTC_10K_BETA_3950.invert(25_000)?;
//! ```
//!
//! # Code generation (`gen-lib` feature)
//!
//! With `features = ["gen-lib"]`, host tools and `build.rs` can call
//! `r#gen::generate_from_toml` / `r#gen::generate_to_path` without shelling
//! out to the CLI. (The module is spelled `r#gen` because `gen` is a reserved
//! keyword in Rust 2024; raw identifiers cannot appear in intra-doc links,
//! so these are plain code spans rather than links.)
//!
//! # The runtime is always no-std and no-alloc
//!
//! The crate-level `no_std` attribute is unconditional. It is **not** relaxed
//! by any feature. The `gen-lib` / `gen-cli` features link `std` only inside
//! `src/gen` via a module-local `extern crate std` and explicit imports; they
//! never put `std` or an allocator on the runtime path, and the crate root
//! does not `extern crate std`.
//!
//! This matters because Cargo unifies features across a dependency graph. If
//! the attribute were conditional, one unrelated crate enabling
//! `ph-curves/gen-lib` would silently turn a firmware build into a `std`
//! build. Keeping it unconditional makes that impossible rather than merely
//! unlikely. The generator's `String` / `Vec` / `format!` usage is imported
//! explicitly inside `src/gen` instead of arriving through the `std` prelude,
//! so a stray allocation on the runtime path is a compile error.

#![no_std]
#![deny(missing_docs)]
#![allow(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]
// Clippy lint levels live here; thresholds and config are in clippy.toml.
#![deny(clippy::correctness)]
#![warn(
    clippy::suspicious,
    clippy::style,
    clippy::complexity,
    clippy::perf,
    clippy::cloned_instead_of_copied,
    clippy::explicit_iter_loop,
    clippy::implicit_clone,
    clippy::inconsistent_struct_constructor,
    clippy::manual_assert,
    clippy::manual_let_else,
    clippy::match_same_arms,
    clippy::needless_pass_by_value,
    clippy::semicolon_if_nothing_returned,
    clippy::uninlined_format_args,
    clippy::unnested_or_patterns,
    clippy::std_instead_of_core,
    clippy::std_instead_of_alloc,
    clippy::alloc_instead_of_core
)]
#![allow(
    clippy::mod_module_files,
    clippy::self_named_module_files,
    clippy::similar_names,
    clippy::too_many_arguments,
    clippy::struct_excessive_bools,
    clippy::fn_params_excessive_bools,
    clippy::type_complexity,
    clippy::must_use_candidate,
    clippy::assertions_on_constants,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_lossless,
    clippy::panic_in_result_fn,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::module_name_repetitions,
    clippy::wildcard_imports,
    clippy::items_after_statements,
    clippy::let_underscore_future
)]

mod affine;
mod curve;
mod math;
mod round;
mod stabilize;
mod tickless;
mod transfer;

pub use affine::{AffineOverflow, AffineTransform, AffineTransformError};
pub use curve::{
    Curve, CurveLut, CurveLut256, MonotonicCurve, MonotonicCurveLut, MonotonicCurveLut256,
};
#[cfg(not(target_pointer_width = "16"))]
pub use curve::{CurveLut65536, MonotonicCurveLut65536};
pub use math::{
    Rounding, UnitValue, lerp_u8, lerp_u16, map_u8_to_u16, next_target_value, quantize,
};
pub use stabilize::{
    Debounce, DebounceOutput, ExponentialSmoother, FilterOutput, Hysteresis, MedianFilter,
    MovingAverage, Stability, StabilityDetector, TemporalFilter, TemporalSample,
};
pub use tickless::{RepeatMode, Tickless, TicklessDeadline, TicklessIter, TicklessSchedule};
pub use transfer::{
    AffineCalibration, AffineCalibrationError, BoundaryBehavior, FlatResolution,
    InterpolationError, InverseTransferError, InverseTransferFunction, MonotonicDirection,
    ObservationGuard, ObservationGuardBehavior, ObservationGuardMetadata, PiecewiseLinearTransfer,
    TransferError, TransferFunction, TransferMetadata, interpolate_segment, invert_segment,
};

#[cfg(feature = "gen-lib")]
pub mod r#gen;

#[cfg(test)]
mod tests;
