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

Merge order was #7 → #6 → #4 → #5, then #9 (integration gaps) and #10 (restore CI, preserve the 0.1.x baseline). Only #4 ↔ #5 conflicted, in `src/transfer.rs` and `src/lib.rs`; both sides were additive and were kept whole.

## Validation

`scripts/local-ci.ps1` passes end-to-end on this branch: `fmt --check`; tests under default, `gen-lib`, `gen-cli`, and `gen`; the 0.1.x feature-compat CLI smoke (`cargo run --features gen --bin ph-curves-gen -- --help`); clippy `--all-targets` at `-D warnings` for default / `gen-lib` / `gen-cli`; rustdoc at `-D warnings`; no-std builds for thumbv7em, thumbv6m, riscv32imac, riscv32imc, and wasm32; ESP32 / S2 / S3 via `+esp` with `-Zbuild-std=core`; and `cargo package`.

## Runtime guarantee

The crate exists so firmware gets deterministic, allocation-free lookup and scheduling. `#![no_std]` is **unconditional** — no feature relaxes it. Host code generation links `std` only inside `src/gen` (module-local `extern crate std` plus explicit prelude imports); the crate root does not link `std`, so a stray `String` on the runtime path is a compile error rather than a silent allocator dependency.

CI enforces this in the `runtime-purity` job: it rejects a feature-conditional `#![no_std]`, then builds the default feature set against a `core`-only sysroot (`-Z build-std=core`) on thumbv7em, thumbv6m, and riscv32imc. A plain `--target` build only proves no-std — bare-metal `rust-std` ships `alloc` — so the core-only build is what proves no-alloc.

## Baseline compatibility

**0.2.0 breaks nothing against 0.1.2.** See [baseline-compatibility.md](./baseline-compatibility.md) for the full assessment of both candidate breaks, why each was reworked instead of shipped, and where the bar for a justified break actually sits.

## Outstanding gates

- Remote CI is restored at `.github/workflows/ci.yml`, covering format, runtime purity, clippy and tests across `gen-lib` / `gen-cli` / `gen`, 0.1.x feature compatibility, the no-std and Xtensa target matrices, docs, and packaging. It runs on `main` and `release/**`.
- No crates.io publish is implied. `Cargo.toml` is at `0.2.0` and `CHANGELOG.md` has a dated `[0.2.0]` section with compare links, so the release PR is self-describing. `cargo publish --dry-run` passes. Publishing remains an owner-only step — see [RELEASING.md](../../RELEASING.md).

## Integration gaps found and closed

Three gaps surfaced while packing the train. None came from a single companion — each only became visible once the features sat together — and all three are now fixed on this branch:

- **Calibrated inverse.** `AffineCalibration` implemented `TransferFunction` but not `InverseTransferFunction`, so setpoint-to-code with factory calibration applied did not compose — arguably the main reason to ship #4 and #5 together. It now undoes the affine with `y = (y' * scale - offset) / gain` and delegates, re-expressing inner range errors in calibrated units and flipping `BelowRange` / `AboveRange` when the calibration reverses orientation. `gain == 0` is rejected at construction, since it makes the affine non-invertible.
- **Boundary policy on decreasing tables.** `invert` selected its policy by physical side alone. On a decreasing table — the NTC reference case — codes above `domain_max` produce physical values below `range_min`, so a table configured `below = Error, above = Clamp` clamped forward and errored inverse for the same condition. `below` and `above` are declared against the observation domain and are now mapped onto the physical range through the table's direction; see `range_behaviors`.
- **Duplicate rounding.** `stabilize::round_div_nearest`, `transfer::div_nearest_ties_away`, and `transfer::round_div_nearest_checked` were three implementations of the same nearest/ties-away rule, one of them unchecked. They are now one shared `crate::round` helper, so a quantized value cannot drift depending on which module produced it.

## Notes on behavior at the margins

- A convert-then-invert round trip through a calibration is **bounded, not exact**: both directions round. A value within half an uncalibrated quantum of a range endpoint rounds back into range rather than erroring, which is correct — the calibrated scale can be finer than the table can represent.
- `achieved_max_inverse_code_error` describes the **uncalibrated** table. Wrapping a table in `AffineCalibration` can widen the round-trip bound.
