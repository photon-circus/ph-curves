# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Sparse, integer-only `PiecewiseLinearTransfer` support for physical
  ADC-to-measurement conversion with signed outputs, explicit below/above
  policies, and no extrapolation.
- Host-side adaptive transfer generation from physical points, formulas, and
  an NTC Beta-divider model, with exhaustive discrete-domain error reporting
  and bounded knot counts.
- Fixed-memory integer moving-average, median, and exponential filters plus a
  separate range-based stability detector for caller-supplied sample series.

### Fixed

- `ph-curves-gen` rejects curve/transfer names that normalize to the same Rust
  identifier or collide with generated companions (`_FWD`, `_INV`, `_INPUTS`,
  `_OUTPUTS`, `_METADATA`).
- Generated `///` docs Debug-escape curve/transfer names, provenance, and unit
  strings so TOML text cannot break out of line comments.

### Notes

- Remote GitHub Actions remain disabled on the ADC transfer-functions feature
  branch (`.github/ci.yml.disabled`). Use `scripts/local-ci.ps1` until Actions
  are restored before merge.

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

[Unreleased]: https://github.com/photon-circus/ph-curves/compare/v0.1.1...HEAD
[0.1.1]: https://github.com/photon-circus/ph-curves/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/photon-circus/ph-curves/releases/tag/v0.1.0
