# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0] - 2026-08-09

### Added

- `InverseTransferFunction` and `PiecewiseLinearTransfer::invert`, mapping a
  physical setpoint back to an observation on the same sparse knots with no
  dense physical-domain LUT. `FlatResolution` selects how a value landing on a
  flat (non-unique) output run resolves; `InverseTransferError` reports
  out-of-range and ambiguous-flat cases against the physical range.
- `invert_segment`, the public mirror of `interpolate_segment`, so host tools
  and the runtime share one rounding implementation.
- `AffineCalibration` implements `InverseTransferFunction` when its inner
  transfer does, so a calibrated setpoint — "which ADC code reads 25 °C after
  this unit's factory trim?" — is a single `invert` call. Range errors are
  re-expressed in calibrated units, and a calibration whose `gain` and `scale`
  have opposite signs flips `BelowRange` and `AboveRange` accordingly.
- `PiecewiseLinearTransfer::range_behaviors` reports how the domain boundary
  policies map onto the physical range.
- Transfer metadata now records `range_min` / `range_max`,
  `strictly_monotonic`, `flat_segment_count`, and an exhaustively measured
  `achieved_max_inverse_code_error` round-trip bound.
- `AffineCalibration` wrapper that applies caller-supplied `i32`
  gain/offset/scale after any `TransferFunction<Output = i32>` using checked
  `i64` math (nearest, ties-away). Overflow surfaces as
  `TransferError::Overflow`; `scale == 0` returns
  `AffineCalibrationError::ZeroScale` at construction.
- `Hysteresis` and `Debounce` decision primitives beside the temporal filters.
  Both are sample-count only: they read no clock and touch no GPIO.
- `ph_curves::r#gen`, a `build.rs` / host-tool library API behind the `gen`
  feature, exposing `generate_from_toml`, `generate_from_str`,
  `generate_to_path`, `generate`, `GenerateOptions`, and `ValueType`.
  Generator validation now returns `Error` values instead of panicking.
- Sparse, integer-only `PiecewiseLinearTransfer` support for physical
  ADC-to-measurement conversion with signed outputs, explicit below/above
  policies, and no extrapolation.
- Host-side adaptive transfer generation from physical points, formulas, and
  an NTC Beta-divider model, with exhaustive discrete-domain error reporting
  and bounded knot counts.
- Fixed-memory integer moving-average, median, and exponential filters plus a
  separate range-based stability detector for caller-supplied sample series.

### Fixed

- The 0.1.2 curve-name identifier rejection now also covers transfer names and
  the generated companions (`_FWD`, `_INV`, `_INPUTS`, `_OUTPUTS`,
  `_METADATA`), so a curve and a transfer cannot claim the same symbol.
- The 0.1.2 Debug-escaping of generated `///` docs now also covers transfer
  names, provenance, and unit strings, so TOML text cannot break out of line
  comments.
- Formula parsing no longer evaluates at an arbitrary out-of-domain value
  before the generator evaluates the declared input domain.
- Signed transfer interpolation rounds the complete result at half-way ties,
  matching the documented ties-away-from-zero behavior.
- The generator emitted `achieved_max_inverse_code_error: 0` unconditionally
  rather than measuring it, so any table whose codes do not survive a
  convert-then-invert cycle shipped a false round-trip bound. It is now swept
  exhaustively over the input domain.
- `TransferMetadata` no longer carries a `flat_resolution` copy. Codegen always
  baked in `PreferLowInput`, so metadata contradicted the live policy for any
  caller using `with_flat_resolution`. Read `flat_resolution()` instead.
- Inverse conversion selected its boundary policy by physical side alone, so on
  a decreasing table — the NTC reference case — a table configured
  `below = Error, above = Clamp` clamped in the forward direction and errored
  in the inverse for the same out-of-range condition. `below` and `above` are
  declared against the observation domain and are now mapped onto the physical
  range through the table's direction, so both directions agree.

### Changed

- `AffineCalibration::new` rejects `gain == 0` with
  `AffineCalibrationError::ZeroGain`. A zero gain collapses every observation
  onto `offset / scale`, discarding the sensor and leaving the calibration
  non-invertible. Not a baseline change — `AffineCalibration` ships new in
  0.2.0.
- The crate's three near-duplicate nearest/ties-away division helpers are now
  one shared implementation, so a quantized value cannot drift depending on
  which module produced it.
- Host code generation is split into two features. `gen-lib` is the `build.rs`
  library API (serde + toml, no clap); `gen-cli` adds the `ph-curves-gen`
  binary. **`gen` is unchanged from 0.1.x** — it is now an alias for `gen-cli`
  and still builds the binary, so existing `--features gen` invocations keep
  working. *No migration is required.* Build scripts should prefer `gen-lib`,
  which skips the clap dependency.
- Exhausting the greedy transfer fitter's knot budget now reports the
  heuristic limitation without claiming that no alternative knot placement
  could satisfy the requested error.

### Notes

- **No breaking changes against 0.1.2.** Every addition above is additive, and
  the two changes that would have broken the baseline — retiring `gen` as the
  CLI feature, and relaxing `#![no_std]` for host builds — were both reworked
  so the 0.1.x contract holds. A 0.1.2 dependency declaration and a 0.1.2
  `cargo run --features gen` invocation both keep working unchanged.
- **The runtime is `no_std` and `no_alloc`, unconditionally.** `#![no_std]` is
  not feature-gated, so Cargo's feature unification cannot turn a firmware
  build into a `std` build when an unrelated crate enables a host feature. CI
  proves it by building the default feature set against a `core`-only sysroot
  (`-Z build-std=core`) on thumbv7em, thumbv6m, and riscv32imc: reaching for
  `alloc` or `std` on the runtime path fails the build.
- Remote GitHub Actions are restored at `.github/workflows/ci.yml`, covering
  format, the runtime-purity gate, clippy and tests across `gen-lib` /
  `gen-cli` / `gen`, a 0.1.x feature-compatibility check, the no-std and Xtensa
  target matrices, docs, and packaging.

## [0.1.2] - 2026-08-09

### Fixed

- Panic in `<u8 as UnitValue>::from_time_frac` and `to_time_offset` for any
  `duration_ms` above 65,535 (~65 seconds). The `U16F16` intermediate holds
  only 16 integer bits, so converting a larger millisecond count aborted with
  `"… overflows"`. Both now use `u64` integer arithmetic and accept the full
  `u32` range.
- The same panic in `<u16 as UnitValue>::from_time_frac` and `to_time_offset`
  for any `duration_ms` above 2,147,483,647 (~24.9 days), where the `I32F32`
  intermediate overflowed its 32 integer bits.
- Sign error in the code generator's piecewise-linear interpolation: control
  point outputs above 32,767 were reinterpreted as negative `i16`, producing
  wrong `u16` curves. Segment endpoints are now exact in both directions.
- The generator accepted `--lut-size` values smaller than the value type's
  domain, emitting LUTs that panicked at runtime on out-of-range indices
  (`CurveLut::eval` indexes by value). See *Changed* below.

### Changed

- **Breaking (CLI):** `--lut-size` must now equal the full domain of
  `--value-type`: 256 for `u8`, 65536 for `u16`. Smaller values were already
  broken at runtime, so any invocation that still succeeds produces identical
  output. *Migration:* if you passed a smaller `--lut-size`, switch to the full
  size and regenerate; if you relied on a partial LUT, those generated curves
  were panicking for any input at or above `lut_size`.
- **Breaking (CLI):** curve names containing no ASCII letter or digit, and
  names that normalize to an identifier already claimed by another curve
  (`ease-in` and `ease_in` both yield `EASE_IN`), are now rejected with an
  explanatory error instead of emitting a file that fails to compile — or one
  curve's table silently shadowing another's. *Migration:* rename the reported
  curve. Names beginning with a digit are now prefixed with `CURVE_` rather
  than producing an invalid identifier.
- Time conversions now use exact integer arithmetic instead of fixed-point.
  Where the old code did not panic, results are unchanged or differ by exactly
  1 LSB, always toward the mathematically correct value — measured at ~1% of
  `from_time_frac` inputs (1/65535 of range) and ~0.01% of `to_time_offset`
  inputs (1 ms). `to_time_offset` still rounds up as documented and still never
  exceeds `duration_ms`, so schedules cannot overshoot a segment.
- `to_time_offset` splits the duration to stay in 32-bit arithmetic instead of
  dividing in 64 bits, avoiding a software 64-bit divide on cores without a
  hardware divider. Measured on Cortex-M0 under emulation (instructions
  retired, mean over a realistic ramp workload) against 0.1.1: `u8` 193 → 129,
  `u16` 495 → 160. `from_time_frac` is also cheaper than 0.1.1 (`u8` 320 → 206,
  `u16` 2782 → 258), because the fixed-point path it replaced already went
  through a 64-bit divide.
- Generated doc comments quote the curve name with `Debug` escaping, so a name
  containing newlines or quotes can no longer break the emitted source.

## [0.1.1] - 2026-02-12

### Fixed

- `quantize` overflow when rounding near `u16::MAX` with large step sizes
  (Ceil and Nearest could wrap around, causing tickless ramps to be skipped).

### Changed

- Stopped excluding `assets/` from the published crate.

## [0.1.0] - 2026-02-12

### Added

- `CurveLut` and `MonotonicCurveLut` types with const constructors.
- `Curve` and `MonotonicCurve` traits for forward and inverse evaluation.
- `Tickless` extension trait with `TicklessSchedule` and `TicklessIter`.
- `RepeatMode::Once`, `Repeat`, and `PingPong` for tickless scheduling.
- `UnitValue` trait implemented for `u8` and `u16`.
- Math helpers: `lerp_u8`, `lerp_u16`, `map_u8_to_u16`, `quantize`,
  `next_target_value`.
- Code-gen CLI (`ph-curves-gen`) with `builtin`, `formula`, and `points`
  curve definitions.
- 14 built-in easing curves plus legacy aliases.
- 16-bit LUT support (`--value-type u16 --lut-size 65536`).

[Unreleased]: https://github.com/photon-circus/ph-curves/compare/v0.1.2...HEAD
[0.1.2]: https://github.com/photon-circus/ph-curves/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/photon-circus/ph-curves/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/photon-circus/ph-curves/releases/tag/v0.1.0
