# Host transfer inspection and extension IR

**Status:** Host generator API in `gen-lib` — design rationale only; the
code and its rustdoc are authoritative.

`FamilySpec` construction, `TransferSource` overlays, and TOML generation are
proven equivalent by the device-neutral ADC fixture in
`assets/family-acceptance.toml` and `tests/family_acceptance_gen.rs`.

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

1. **Parsed** — `DefinitionsFile` from TOML, `insert_transfer`, or
   `insert_family`.
2. **Validated** — `DefinitionsFile::validate` runs family/gap/identity,
   source-specific mapping-shape, domain, collision, and selector-universe
   completeness checks. Source-window derivation/checking may perform limited
   numerical work, notably NTC model evaluation, but validation does not fit or
   emit Rust.
   Every member, including `unnecessary`, `unsupported`, and `forbidden`, is
   inspectable, as is every family-scoped gap. Explicit and derived emitted
   names are reserved only for `emit`. Successful validation yields
   `FamilyCompleteness::Complete`. Document-level `[gaps]` remain a separate
   named map and do not occupy family identities.
   Mapped description-only members (`unnecessary` and `forbidden`) prove that
   the required applicability coordinate and input transform are structurally
   valid for the selected family source and that its window can be derived and
   checked. Because they are not emitted, the generator does not sweep that
   window for full-window monotonicity, fit or error-measure it, lower it, or
   emit it.
3. **Generation sources** — TOML formula/points/model, or a `TransferSource`
   overlay (`EvaluatedTruth`, `PrefittedKnots`, `Points`). Source *citations*
   (`SourceProvenance`) are distinct from the selected representation (formula
   text, point count, NTC parameters) and from fit policy on the transfer spec.
   Every overlay explicitly inherits, replaces, or clears the target citation.
   Inheritance means the declared, resolved pre-overlay citation and restores
   it when replacing an earlier overlay. An emitted family member cannot clear
   its mandatory citation.
   `kind = "scaled_polynomial"`
   applies per-member `input_transform` as exact
   `u = count * numerator / denominator` (standalone TOML still uses `scale` /
   `1e6`) and evaluates `[c0, c1, ...]` with Horner; firmware still sees integer
   knots. Overlays on family members must span the member's resolved observation
   domain, which is also exposed on `ValidatedMember::observation_domain`.
   Standalone overlays replace their declared source and may define a
   different observation domain.

`ValidatedFamily::source` exposes the exact validated `FamilySource`, including
the scaled-polynomial coefficients or all NTC Beta-divider parameters, along
with units, output scale, aggregate budgets, citation-free policy, and selector
universe. The declared-source, formula, points, and model-presence accessors
remain compatibility projections. `FamilySpec` / `FamilySource` construct the same
`TransferFamilyDef` graph programmatically; TOML and the builder converge on
one validation and generation pipeline. Optional member `emitted_name` is an
explicit table-name stem; the default remains the deterministic
family-plus-selector expansion. `ValidatedDefinitions::emission_manifest`
lists every emit member exactly once as family + typed selectors → symbol +
companion names. Emitted-name collision diagnostics identify both origin
families and their exact typed selector maps. Generated rustdoc for family
members includes the family name and the exact selector map. Selector universe,
family-scoped gaps, typed identities, and completeness remain on
`ValidatedFamily`
([#40](https://github.com/photon-circus/ph-curves/issues/40)).

## Public surface

Host tools inspect through nameable types: `ValidatedDefinitions`,
`ValidatedFamily`, `ValidatedMember`, `ValidatedFamilyGap`,
`SelectorUniverse`, its lazy `SelectorIdentities` iterator,
`FamilyCompleteness`, `FamilySource`, `DeclaredSource`, `SourceProvenance`,
`GenerationPolicy`, `ObservationGuardPolicy`, `EmissionManifest`,
`TransferFamilyDef` accessors, `DefinitionsFile::curves` / `transfers` /
`transfer_families` / `gaps`. `ValidatedFamily::gaps` is the family-scoped
selector list; `ValidatedDefinitions::gaps` is the document-level named map.
Built-in `ModelDef` remains crate-private. Both programmatic construction and
validated inspection use the public `FamilySource::NtcBetaDivider` variant and
`DividerTopology`, without exposing that catalog type.

`SelectorUniverse::identity_count` is the explicit-list length or checked
Cartesian axis-length product. Family validation rejects a Cartesian product
that does not fit the host's `usize`. Completeness follows from indexed
in-universe membership, duplicate rejection, and equality between occupied
and expected counts; validation never materializes the product.
`SelectorUniverse::identities` yields maps lazily in deterministic axis/value
order for callers that need independent enumeration.

`ValidatedFamily::provenance` and
`ValidatedFamily::observation_guard_provenance` are separately inspectable
from the citation-free `ValidatedFamily::policy`; members expose the resolved
citation plus any declared override. Family-scoped gaps likewise expose their
resolved inherited citation and optional declared override. A successful
overlay replacement updates the member's effective `provenance()` while
`provenance_override()` remains the document declaration; replacing that
overlay with inheritance restores the declared, resolved citation.

Extension:

- `TransferSpec` + `TransferSource` construct a standalone transfer without TOML.
  `TransferSpec::with_provenance` attaches the same citation type as TOML.
- `FamilySpec` + `FamilySource` construct a family without TOML, including
  members, family-scoped gaps, selector universe, shared source, and
  fitting/boundary/guard policy. `FamilySpec` requires provenance because a
  source-backed family must name its document. `insert_family` on
  `DefinitionsFile` or `ValidatedDefinitions` (the latter re-validates) uses
  the same structural pipeline as parsed TOML. These insertion calls are
  transactional and enforce the same names, symbols, selector identities, and
  description-only collision rules before or after validation. They may run
  source-specific window derivation/checks (including numerical NTC work), but
  full-window monotonicity, fitting, error measurement, lowering, and emission
  run only for `emit` members in `generate` / `generate_report`.
- `ValidatedDefinitions::set_source` overlays truth or knots on a standalone
  transfer or an **emitted** family member. Description-only members reject
  overlays so source facts and generation input stay distinct. Construct the
  required `TransferSourceOverlay` with
  `TransferSource::{inherit_provenance, with_provenance, clear_provenance}`.
  Emitted family-member overlays accept intentional inheritance or
  replacement, not clearing.
- `ValidatedDefinitions::generate` / `generate_report` emit ordinary
  `PiecewiseLinearTransfer` constants. The report path is the source of
  family totals and aggregate budget diagnostics; the `String` helper
  discards the report after the same checks. Reports preserve effective
  overlay provenance, pre-overlay guard provenance, citation-free policy,
  every family member and scoped gap, and named document gaps. Totals remain
  emit-only.
- Evaluated truth is dense unscaled physical samples; the existing greedy
  fitter runs.
- Prefitted knots skip the fitter. Inverse-code-error measurement and
  `emit_transfer` still run. A dense oracle is required; after output scaling,
  it must be monotonic in the same direction as the knots before interpolation
  error is measured and the table and its accuracy metadata are emitted.

Output remains ordinary `PiecewiseLinearTransfer` constants. An optional
observation-code guard (TOML `saturation`) is copied through family expansion
and emitted as `with_observation_guard` plus an adjacent
`Option<ObservationGuardMetadata>` constant. Classification as saturation is
declared consumer/device policy, not inferred from the integer value.
A family guard citation resolves against family provenance before member
overrides. A standalone guard citation resolves against the declared transfer
citation before a generation-source overlay, so replacing or clearing source
provenance does not silently rewrite or remove the guard-classification
citation.
Standalone TOML guards require the localized
`[transfers] requires = ["observation_guard_v1"]` capability; its wire shape
makes older generators reject rather than silently omit the guard.
Standalone TOML source provenance likewise requires
`[transfers] requires = ["source_provenance_v1"]`. Documents using both list
both strings in the same array. TOML documents containing families add
`"transfer_families_v1"`; released 0.2.1 generators reject the array before
they can ignore the unknown family table. Family guards and family provenance
are covered by that family capability and do not consume either standalone
capability. Therefore a family-only document lists only
`"transfer_families_v1"`; standalone capability strings are combined with it
only when standalone transfers use those features. Missing and unused
capabilities fail parsing. Programmatic construction needs no marker.

## Options

`GenerateOptions::transfers_only` exists so transfer-only generation does not
need meaningful `value_type` / `lut_size`. Those fields are validated only
when the document contains `[curves]`. Fit policy (`max_interpolation_error`,
`max_knots`, boundaries) lives on the transfer/family spec. Optional family
`max_total_knots` / `max_table_bytes` are aggregate generation budgets, not
LUT options; `generate` and `generate_report` both enforce them.

## Keep-outs

- Plugin, callback, or WASM evaluator ABI
- Independent `extrapolation` fields
- VEML knot budget and vendor oracle (#29)
- `kind = "veml7700"` or any device lifecycle API
- Runtime family types, `std` / alloc / float on the default-feature API
- Fetching provenance URLs or embedding citation strings in runtime transfers
