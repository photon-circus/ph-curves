# AffineCalibration

**Branch:** `feature/0.2.0-affine-calibration`  
**Status:** Implemented on this branch only; do not merge to `main` without owner decision.

## Motivation

PR #1 lists “runtime/factory gain-and-offset calibration wrappers” as a limitation. Almost every real sensor needs a two-point or stored gain/offset without regenerating the knot table. Caller supplies constants (EEPROM/flash/elsewhere); the crate stays pure math.

## Relation to PR #1 types

| PR #1 surface | This design |
| ------------- | ----------- |
| `TransferFunction::convert` | `AffineCalibration<T>` implements `TransferFunction` and composes `convert` |
| `PiecewiseLinearTransfer` | Typical inner `T`; nests anywhere a transfer is used |
| `TransferError` | Domain errors propagate from inner; affine may add overflow handling |
| `BoundaryBehavior` | Unchanged; delegated to inner |
| `TransferMetadata` | Not owned/updated by affine (table-fit facts stay table-only) |
| Stabilize filters | Optional downstream: smooth the calibrated `i32` |

Pipeline placement: observation → Transfer → **AffineCal** → optional TemporalFilter → app.

## API sketch

```rust
/// y' = (y * gain + offset) / scale   (i64 intermediates)
pub struct AffineCalibration<T> {
    inner: T,
    gain: i32,
    offset: i32,
    scale: i32, // nonzero; typically output_scale-related
}

impl<T> AffineCalibration<T> {
    pub fn new(
        inner: T,
        gain: i32,
        offset: i32,
        scale: i32,
    ) -> Result<Self, AffineCalibrationError>;
    // returns AffineCalibrationError::ZeroScale if scale == 0

    pub const fn inner(&self) -> &T;
    pub const fn gain(&self) -> i32;
    pub const fn offset(&self) -> i32;
    pub const fn scale(&self) -> i32;
}

impl<T> TransferFunction for AffineCalibration<T>
where
    T: TransferFunction<Output = i32>,
{
    type Input = T::Input;
    type Output = i32;

    fn convert(&self, input: Self::Input)
        -> Result<i32, TransferError<Self::Input>>
    {
        let y = self.inner.convert(input)?;
        // i64: (y * gain + offset) / scale, nearest ties-away
        Ok(apply_affine_i64(y, self.gain, self.offset, self.scale)?)
    }
}
```

Identity: `gain = scale`, `offset = 0` → passthrough (modulo rounding when `|scale| ≠ 1`). Negative scale allowed (flips sense). Suggested module: `transfer` (+ crate re-export).

## Keep-outs / bounds

- No NVM / EEPROM / flash drivers — caller supplies the integer triple
- No per-device IDs, discovery, or provisioning UX
- No regenerating or mutating knot tables (wrapper only)
- No updating `TransferMetadata` error fields for factory cal
- No float calibration on the firmware path (host may solve offline)
- No multidimensional / temp-compensated calibration

## Non-goals

- Nesting many affines as a first-class story (allowed via trait; document precision/overflow stacking)
- Silent wrap on `i32` overflow — prefer dedicated error or saturating variant (pick one public API)
- Landing on `main`, version bumps, or crates.io release from this branch

## Merge gate

Implementation lives on this branch only; do not merge to `main` without owner decision.

## Inverse (added during 0.2.0 integration)

`AffineCalibration<T>` implements `InverseTransferFunction` whenever `T` does, closing the composition gap between this companion and `feature/0.2.0-inverse-transfer`. Without it, a calibrated setpoint required the caller to hand-roll the affine inverse and get the rounding right.

```rust
let trimmed = AffineCalibration::new(NTC_10K_BETA_3950, 1_005, -120, 1_000)?;
let code = trimmed.invert(25_000)?;   // calibrated setpoint -> ADC code
```

- Solves `y = (y' * scale - offset) / gain` with the same nearest, ties-away rounding as the forward path, then delegates to the inner `invert`.
- `gain == 0` is rejected by `new` with `AffineCalibrationError::ZeroGain`: it collapses every observation onto `offset / scale`, so the affine has no inverse. This is a deliberate tightening of the constructor rather than a deferred failure in `invert`.
- Inner range errors are re-expressed in **calibrated** units, so `minimum` / `maximum` are comparable with the value the caller passed. When `gain` and `scale` have opposite signs the calibration reverses orientation, and an inner `BelowRange` surfaces as `AboveRange`.
- `InverseTransferError::Overflow` covers an undone value that does not fit `i32`, and a range bound that cannot be re-expressed.

### Round-trip bound

Both directions round, so `invert(convert(x))` through a calibration is bounded, not exact. A calibration that compresses the physical scale cannot restore what the forward quantization discarded. A calibrated value within half an *uncalibrated* quantum of a range endpoint rounds back into range rather than erroring.

`TransferMetadata::achieved_max_inverse_code_error` describes the uncalibrated table only; wrapping in `AffineCalibration` can widen it.
