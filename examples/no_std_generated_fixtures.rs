//! Compile representative generated fixtures as `no_std` firmware artifacts.
//!
//! CI and `scripts/local-ci.ps1` build this example on the repository's
//! no-std and core-only target matrix so generated tables cannot silently
//! pick up `std`, `alloc`, or floating-point types.

#![no_std]
#![allow(dead_code)]

use ph_curves::{
    AffineCalibration, AffineTransform, FilterOutput, Hysteresis, InverseTransferFunction,
    MonotonicDirection, MovingAverage, PiecewiseLinearTransfer, TemporalFilter, TransferFunction,
};

static SMOKE_INPUTS: [u16; 2] = [0, u16::MAX];
static SMOKE_OUTPUTS: [i32; 2] = [-1_000, 1_000];

/// Compile the headline runtime APIs into every no-std/core-only matrix target.
///
/// The example is a library artifact, so CI does not execute this function;
/// compiling its body proves transfer conversion/inversion, both affine forms,
/// and `u32` temporal state stay on the core-only path.
pub fn runtime_api_smoke(code: u16, sample: u32) -> (i32, u16, u32, bool) {
    let transfer = PiecewiseLinearTransfer::new(
        &SMOKE_INPUTS,
        &SMOKE_OUTPUTS,
        MonotonicDirection::Increasing,
    );
    let calibrated = match AffineCalibration::new(transfer, 1_001, 0, 1_000) {
        Ok(value) => value,
        Err(_) => unreachable!(),
    };
    let physical = calibrated.convert(code).unwrap_or_default();
    let inverse = calibrated.invert(physical).unwrap_or_default();

    let affine = match AffineTransform::new(1_001, 0, 1_000) {
        Ok(value) => value,
        Err(_) => unreachable!(),
    };
    let _ = affine
        .apply(physical)
        .and_then(|value| affine.unapply(value));

    let mut average = MovingAverage::<u32, 1>::new();
    let filtered = match average.update(sample) {
        FilterOutput::Ready(value) => value,
        FilterOutput::WarmingUp { .. } => unreachable!(),
    };
    let mut latch = Hysteresis::<u32>::new(100, 200);
    (physical, inverse, filtered, latch.update(filtered))
}

/// Mixed family-acceptance fixture: curve LUT plus sparse transfers.
pub mod family_acceptance {
    include!("../tests/fixtures/family_acceptance_generated.rs");
}

/// Reference NTC transfer fixture.
pub mod ntc {
    include!("../tests/fixtures/ntc_generated.rs");
}

/// Standalone and family observation-guard fixture.
pub mod observation_guards {
    include!("../tests/fixtures/observation_guards_generated.rs");
}
