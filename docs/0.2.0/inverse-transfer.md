# Inverse transfer (design)

**Branch:** `feature/0.2.0-inverse-transfer`  
**Status:** Implemented on this branch (runtime API). Do not merge to `main` without owner decision.

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
    pub const fn with_flat_resolution(self, policy: FlatResolution) -> Self;
    pub const fn physical_range(&self) -> (i32, i32);
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum FlatResolution {
    #[default]
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

**Metadata (additive):** `range_min` / `range_max`, `strictly_monotonic`, `flat_segment_count`, `achieved_max_inverse_code_error`.

`achieved_max_inverse_code_error` is measured exhaustively by the generator across the whole input domain, reusing the library's own `interpolate_segment` / `invert_segment` so the host audit rounds exactly the way the runtime does. `tests/ntc_transfer.rs` re-measures the emitted table at runtime and asserts it matches the recorded value, which is what catches drift between the host search and `PiecewiseLinearTransfer::invert`.

`flat_resolution` is deliberately **not** in `TransferMetadata`. It describes a runtime knob that `with_flat_resolution` can change after the table is defined, so a baked-in copy would contradict the live policy on any caller that overrides it. Read `PiecewiseLinearTransfer::flat_resolution()` instead; `TransferMetadata` records only facts about the table itself.

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
