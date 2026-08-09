# Inverse transfer (design)

**Branch:** `feature/0.2.0-inverse-transfer`  
**Status:** Design documentation only. Implementation lives on this branch only; do not merge to `main` without owner decision.

## Motivation

PR #1 adds forward `TransferFunction::convert` (`u16` observation → `i32` physical) but explicitly defers inverse transfer. Firmware still needs setpoints and diagnostics (“what ADC code is 25.000 °C?”) without inventing a second table format. This mirrors `MonotonicCurve::inv` for the transfer pillar.

## Relation to PR #1 types

| PR #1 type | Inverse role |
| ---------- | ------------ |
| `TransferFunction` | Parallel trait `InverseTransferFunction` (not a supertrait) |
| `PiecewiseLinearTransfer<N>` | Implements inverse on the same `_INPUTS` / `_OUTPUTS` knots |
| `TransferMetadata` | Additive inverse-related fields (range, flat census, audit) |
| `BoundaryBehavior` | Reused for below/above **physical** range |
| `MonotonicDirection` | Selects search order on outputs (decreasing NTC is the motivating case) |
| `TransferError<I>` | Parallel `InverseTransferError<P>` |

## API sketch

```rust
pub trait InverseTransferFunction {
    type Physical: Copy;
    type Observation: Copy;

    fn invert(
        &self,
        physical: Self::Physical,
    ) -> Result<Self::Observation, InverseTransferError<Self::Physical>>;
}

impl<const N: usize> InverseTransferFunction for PiecewiseLinearTransfer<N> {
    type Physical = i32;
    type Observation = u16;
    fn invert(&self, physical: i32) -> Result<u16, InverseTransferError<i32>>;
}

impl<const N: usize> PiecewiseLinearTransfer<N> {
    pub fn invert_physical(&self, physical: i32)
        -> Result<u16, InverseTransferError<i32>>;
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum FlatResolution {
    PreferLowInput,  // default
    PreferHighInput,
    Midpoint,
    Error,
}

pub enum InverseTransferError<P> {
    BelowRange { physical: P, minimum: P },
    AboveRange { physical: P, maximum: P },
    AmbiguousFlat { physical: P, low: u16, high: u16 },
}
```

**Algorithm (runtime only):** binary search knot index by output (respecting direction) → exact flat-run + `FlatResolution`, or `invert_valid_segment` with shared nearest/ties-away `i64` divider matching the forward path. No dense physical→input LUT.

**Metadata (additive):** `range_min` / `range_max`, `strictly_monotonic`, `flat_segment_count`, `flat_resolution`, `achieved_max_inverse_code_error`.

## Keep-outs / bounds

- Do **not** invent a second adaptive fitter or re-sample the host model for inverse
- Do **not** emit dense `_INV_*` arrays over the `i32` physical domain
- Optional reordered view arrays are deferred (micro-opt only)
- Round-trip `convert ∘ invert` is bounded, not identity for every physical value

## Non-goals

- Multidimensional / temp-compensated inverse
- Wider domains (`u32` / `i64`) in this design
- Landing on `main`, version bumps, or crates.io release from this branch

## Merge gate

Implementation lives on this branch only; do not merge to `main` without owner decision.
