# ph-curves 0.2.0 release train

This branch (`release/0.2.0`) is the packing spine for the 0.2.0 release. The anchor and all four companion features are **merged here**. Landing on `main` is a separate, explicit owner decision — see *Outstanding gates* below.

## Contents

| Item | PR | Scope |
| ---- | -- | ----- |
| **Transfer + Stabilize** (anchor) | [#1](https://github.com/photon-circus/ph-curves/pull/1) | `TransferFunction`, `PiecewiseLinearTransfer`, `TransferMetadata`; temporal filters (`MovingAverage`, `MedianFilter`, `ExponentialSmoother`, `StabilityDetector`). Pure math primitives — not ADC/GPIO/timer ownership. |
| **Inverse transfer** | [#4](https://github.com/photon-circus/ph-curves/pull/4) | [inverse-transfer.md](./inverse-transfer.md) — physical → observation invert on the anchor's knots; no second adaptive fitter; no dense `_INV_*` LUT |
| **Affine calibration** | [#5](https://github.com/photon-circus/ph-curves/pull/5) | [affine-calibration.md](./affine-calibration.md) — `AffineCalibration<T: TransferFunction>` gain/offset wrapper; no NVM/discovery |
| **Decision primitives** | [#6](https://github.com/photon-circus/ph-curves/pull/6) | [decision-primitives.md](./decision-primitives.md) — `Hysteresis` + `Debounce` beside Stabilize; sample-count only; no clock/GPIO |
| **gen build API** | [#7](https://github.com/photon-circus/ph-curves/pull/7) | [gen-build-api.md](./gen-build-api.md) — `ph_curves::r#gen` for `build.rs`; thin CLI behind `gen-cli`; no proc-macro DSL |

Merge order was #7 → #6 → #4 → #5. Only #4 ↔ #5 conflicted, in `src/transfer.rs` and `src/lib.rs`; both sides were additive and were kept whole.

## Validation

`scripts/local-ci.ps1` passes end-to-end on this branch: `fmt --check`; tests under default, `gen`, and `gen-cli`; clippy `--all-targets` at `-D warnings` for all three; rustdoc at `-D warnings`; no-std builds for thumbv7em, thumbv6m, riscv32imac, riscv32imc, and wasm32; ESP32 / S2 / S3 via `+esp` with `-Zbuild-std=core`; and `cargo package`.

## Outstanding gates

- **GitHub Actions are still disabled** (`.github/ci.yml.disabled`). Merging this branch to `main` would land `main` without remote CI. Restoring `.github/workflows/ci.yml` — and updating it for the `gen` / `gen-cli` split — is an owner decision that blocks the merge.
- No crates.io publish is implied. `Cargo.toml` is at `0.2.0` and `CHANGELOG.md` has a dated `[0.2.0]` section so the release PR is self-describing; publishing remains a separate step.

## Known gaps carried into the release

Neither blocks the merge, but both are real and undocumented elsewhere:

- **No calibrated inverse.** `AffineCalibration` implements `TransferFunction` but not `InverseTransferFunction`, so a setpoint-to-code conversion with factory calibration applied does not compose. Inverting the affine step before delegating to the inner transfer would close this.
- **Boundary policy is mirrored, not corresponding, on decreasing tables.** `invert` applies `below` to low-physical values. On a decreasing table — the NTC reference case — codes above `domain_max` correspond to physical values below `range_min`, so a table configured `below = Error, above = Clamp` clamps in the forward direction and errors in the inverse for the same physical situation. Documented in the trait, but the two directions still disagree.
- **Three near-duplicate rounding helpers** now coexist: `stabilize::round_div_nearest`, `transfer::div_nearest_ties_away`, and `transfer::round_div_nearest_checked`. Same ties-away semantics, one of them unchecked.
