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
//! - **Physical transfer functions** — [`TransferFunction`], the sparse,
//!   integer-only [`PiecewiseLinearTransfer`] for ADC-to-measurement
//!   conversion, and [`AffineCalibration`] for caller-supplied gain/offset.
//! - **Temporal stabilization** — [`MovingAverage`], [`MedianFilter`],
//!   [`ExponentialSmoother`], and [`StabilityDetector`] over caller-supplied
//!   integer samples.
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
//! const-generic state. Windowed filters return [`FilterOutput::WarmingUp`]
//! until ready. Stability classification is separate from smoothing so a
//! filtered value is not implicitly treated as settled.
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
//! one monotonic `i32` output. [`AffineCalibration`] applies a caller-supplied
//! integer gain/offset/scale after the table without regenerating knots or
//! touching NVM. The crate still does not provide inverse conversion,
//! nonmonotonic maps, multidimensional compensation, sensor fusion, or device
//! policy. Those concerns belong in application or domain-specific crates that
//! compose with this crate's generic primitives.
//!
//! ```ignore
//! use ph_curves::TransferFunction;
//!
//! include!("ntc_transfer.rs");
//!
//! let milli_celsius = NTC_10K_BETA_3950.convert(adc_code)?;
//! ```

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

mod curve;
mod math;
mod stabilize;
mod tickless;
mod transfer;

pub use curve::{
    Curve, CurveLut, CurveLut256, CurveLut65536, MonotonicCurve, MonotonicCurveLut,
    MonotonicCurveLut256, MonotonicCurveLut65536,
};
pub use math::{
    Rounding, UnitValue, lerp_u8, lerp_u16, map_u8_to_u16, next_target_value, quantize,
};
pub use stabilize::{
    ExponentialSmoother, FilterOutput, MedianFilter, MovingAverage, Stability, StabilityDetector,
    TemporalFilter, TemporalSample,
};
pub use tickless::{RepeatMode, Tickless, TicklessDeadline, TicklessIter, TicklessSchedule};
pub use transfer::{
    AffineCalibration, AffineCalibrationError, BoundaryBehavior, InterpolationError,
    MonotonicDirection, PiecewiseLinearTransfer, TransferError, TransferFunction, TransferMetadata,
    interpolate_segment,
};

#[cfg(test)]
mod tests;
