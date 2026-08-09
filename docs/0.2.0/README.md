# ph-curves 0.2.0 release train (advisory)

This branch (`release/0.2.0`) is the **local packing spine** for a candidate 0.2.0 window. It is **not** merged to `main` by this work. Owner merge gates on GitHub remain the only path to `main`. No crates.io release or version bump is implied here.

## Anchor (already on this branch)

| Item | Branch / PR | Scope |
| ---- | ----------- | ----- |
| **Transfer + Stabilize** | PR [#1](https://github.com/photon-circus/ph-curves/pull/1) · `swgiacomelli-adc-transfer-functions` | `TransferFunction`, `PiecewiseLinearTransfer`, `TransferMetadata`; temporal filters (`MovingAverage`, `MedianFilter`, `ExponentialSmoother`, `StabilityDetector`). Pure math primitives — not ADC/GPIO/timer ownership. |

Base tip at branch creation: tip of `swgiacomelli-adc-transfer-functions`.

## Companion feature branches (design docs only)

Each companion is a **bounded** branch off `release/0.2.0`. Implementation (if any) stays on that branch until the owner decides; do not merge to `main` without an explicit owner decision.

| Branch | Design doc | Scope bound |
| ------ | ---------- | ----------- |
| `feature/0.2.0-inverse-transfer` | [inverse-transfer.md](./inverse-transfer.md) | Physical → observation invert on PR #1 knots; no second adaptive fitter; no dense `_INV_*` LUT |
| `feature/0.2.0-affine-calibration` | [affine-calibration.md](./affine-calibration.md) | `AffineCalibration<T: TransferFunction>` gain/offset wrapper; no NVM/discovery |
| `feature/0.2.0-decision-primitives` | [decision-primitives.md](./decision-primitives.md) | `Hysteresis` + `Debounce` beside Stabilize; sample-count only; no clock/GPIO |
| `feature/0.2.0-gen-build-api` | [gen-build-api.md](./gen-build-api.md) | Expose `ph_curves::gen` for `build.rs`; thin CLI; no proc-macro DSL |

## Explicit non-actions for this train setup

- Do **not** merge this branch or companions into `main`
- Do **not** cut a 0.2.0 crates.io release or bump versions on `main`
- Do **not** treat this packing list as a merge checklist

Landing remains an explicit owner decision outside this documentation.
