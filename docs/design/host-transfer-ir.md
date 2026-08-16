# Host transfer inspection and extension IR

**Status:** Host generator API in `gen-lib` — design rationale only; the
code and its rustdoc are authoritative.

## Motivation

A device crate should own source interpretation. ph-curves should own generic
validation, fitting, metadata, and integer code generation. Issue #24 exposed
read-only family and gap maps, but dependents still could not inspect a
validated graph, construct transfers without TOML, or inject evaluated truth
without serializing into the closed formula/points/model vocabulary.
`GenerateOptions` also described dense curve LUT size and was validated even
for transfer-only documents.

Issue #25 is that host-only inspection and extension boundary. It is not a
plugin ABI: README and `docs/design/gen-build-api.md` keep that as a non-goal.

## Layers

1. **Parsed** — `DefinitionsFile` from TOML or `insert_transfer`.
2. **Validated** — `DefinitionsFile::validate` runs family/gap/identity checks
   without fitting or emitting Rust. Every member, including `unnecessary`,
   `unsupported`, and `forbidden`, is inspectable. Expanded names are reserved
   only for `emit`.
3. **Generation sources** — TOML formula/points/model, or a `TransferSource`
   overlay (`EvaluatedTruth`, `PrefittedKnots`, `Points`). Source facts stay on
   the graph; fit policy stays on the transfer spec. `kind = "scaled_polynomial"`
   applies per-member `input_transform` as exact
   `u = count * numerator / denominator` (standalone TOML still uses `scale` /
   `1e6`) and evaluates `[c0, c1, ...]` with Horner; firmware still sees integer
   knots. Overlays on family members must span the member's resolved observation
   domain. Standalone overlays replace their declared source and may define a
   different observation domain.

This slice stores resolved family-member observation domains internally so
overlay validation is source-independent. Exposing that derived fact on the
complete public family IR remains part of
[#41](https://github.com/photon-circus/ph-curves/issues/41).

## Public surface

Host tools inspect through nameable types: `ValidatedDefinitions`,
`ValidatedFamily`, `ValidatedMember`, `DeclaredSource`, `TransferFamilyDef`
accessors, `DefinitionsFile::curves` / `transfers` / `transfer_families` /
`gaps`. Built-in `ModelDef` remains crate-private.

Extension:

- `TransferSpec` + `TransferSource` construct a standalone transfer without TOML.
- `ValidatedDefinitions::set_source` overlays truth or knots on a standalone
  transfer or an **emitted** family member. Description-only members reject
  overlays so source facts and generation input stay distinct.
- Evaluated truth is dense unscaled physical samples; the existing greedy
  fitter runs.
- Prefitted knots skip the fitter. Inverse-code-error measurement and
  `emit_transfer` still run. A dense oracle is required so interpolation error
  is verified before the table and its accuracy metadata are emitted.

Output remains ordinary `PiecewiseLinearTransfer` constants. An optional
observation-code guard (TOML `saturation`) is copied through family expansion
and emitted as `with_observation_guard` plus an adjacent
`Option<ObservationGuardMetadata>` constant. Classification as saturation is
declared consumer/device policy, not inferred from the integer value.
Standalone TOML guards require the localized
`[transfers] requires = ["observation_guard_v1"]` capability; its wire shape
makes older generators reject rather than silently omit the guard.

## Options

`GenerateOptions::transfers_only` exists so transfer-only generation does not
need meaningful `value_type` / `lut_size`. Those fields are validated only
when the document contains `[curves]`. Fit policy (`max_interpolation_error`,
`max_knots`, boundaries) lives on the transfer/family spec.

## Keep-outs

- Plugin, callback, or WASM evaluator ABI
- Independent `extrapolation` fields
- VEML knot budget and vendor oracle (#29)
- `kind = "veml7700"` or any device lifecycle API
- Runtime family types, `std` / alloc / float on the default-feature API
