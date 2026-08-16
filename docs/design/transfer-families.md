# Transfer families and declared gaps

**Status:** Host generator schema in `gen-lib` — design rationale only; the
code and its rustdoc are authoritative.

## Motivation

Some devices expose discrete selector combinations that share one physical
source while varying input mapping, applicability, or policy. Duplicating
standalone `[transfers]` tables loses selector identity and encourages
interpolating between configurations that must not be interpolated. Sources
may also explicitly leave channels or procedures undefined; those gaps must
remain distinguishable from omissions.

Issue #24 is the generic schema, integrity, inspection, and sparse emission
slice of [#23](https://github.com/photon-circus/ph-curves/issues/23). Issue
[#39](https://github.com/photon-circus/ph-curves/issues/39) makes every
accepted member field source-aware and effectful. The host
inspection/extension IR is documented in
[host-transfer-ir.md](host-transfer-ir.md). This slice does not add a
device-specific model, a runtime family registry, or firmware API.

## Schema

A definitions document may contain `[transfer_families.<name>]` and
`[gaps.<name>]` alongside standalone `[transfers]` and unrelated `[curves]`.
Dense LUT generation remains curve-only; family members still expand to sparse
`PiecewiseLinearTransfer` constants. Family knot default is 64 with a hard cap
of 256 so discrete members cannot become dense ADC tables. Standalone
transfers keep default 256 / cap 4096.

Each family has one shared source (`formula`, `points`, or `model`) and an
explicit `members` array. Selectors are string or integer maps and are never
interpolated. `interpolate_selectors` is not a field; leftover copies are
unknown-field errors.

A source-backed family requires a structured `provenance` table with a
non-blank `identity` (title or stable identifier). Optional `revision`,
`locator`, `url`, and `note` fields locate the cited document. URLs are stored
as opaque strings and are never fetched. Members inherit that citation unless
they declare a `provenance` override. Set fields replace; optional fields named
by `clear = ["revision", "locator", "url", "note"]` are removed. Unset fields
inherit while `identity` is unchanged. Replacing `identity` starts a new
citation, so unspecified optional fields are cleared instead of being combined
with fields from the old document. Global `[gaps]` may declare their own
citation; without a parent family they still require `identity` when
`provenance` is present.

Fit budget, knot cap, `below` / `above`, and `saturation` are
generation/consumer policy inspectable as `GenerationPolicy`, not as part of
the citation. Member `status` is a separate emission decision exposed through
`ValidatedMember::status`.
Observation-guard classification is policy unless `saturation` carries a nested
`provenance` override that cites a source supporting that classification. A
family-level guard citation resolves against family provenance before any
member override is applied. `GenerationPolicy` projects only the guard code and
behavior; inspect the resolved citation separately through
`ValidatedFamily::observation_guard_provenance`.

Mapped member fields are a capability matrix. A field unsupported by the
selected source fails validation with a diagnostic that names the family,
member, field, and source kind.

| Shared source (mapped statuses) | `input_transform` | `applicability` (exactly one key) | Effect |
| --- | --- | --- | --- |
| formula | reject | `observation = [u16, u16]` | Member observation domain |
| points | reject | `observation = [u16, u16]` | Inclusive clip of the shared point set; fewer than two remaining points fails |
| `scaled_polynomial` | required `{ numerator, denominator }` | `model_input = [f64, f64]` | `u = count * numerator / denominator` (integer product first); inclusive code window |
| `ntc_beta_divider` | reject | `physical = [f64, f64]` | Member output range |

Family-level `domain` and `output_range` are unknown fields; members own those
coordinates. Standalone `scaled_polynomial` keeps TOML `scale` with implicit
denominator `1e6`. Family members make both terms explicit. Zero denominators
and zero numerators fail validation. Illuminance-specific names such as
`scale_micro_lux_per_count` are not part of this schema.

Inclusive `model_input` bounds convert to observation codes with the same `u`
as evaluation: smallest code with `u >= min`, largest with `u <= max`.
`u16::MAX` is legal unless the caller’s window excludes it.

`status` is `emit`, `unnecessary`, `unsupported`, or `forbidden`. `emit`
forbids `reason`. Every other status requires a non-blank `reason`.
`unnecessary` and `forbidden` describe known mappings, so their transform and
applicability are validated just like an emitted member. `unsupported` means
the selector combination has no source mapping and therefore forbids both
`input_transform` and `applicability`.

Gaps require `status = "undefined"` and a non-blank `reason`. They are not
generated as transfers.

Unknown fields on family, shared point entry, shared NTC model, member,
applicability, input-transform, and gap tables are rejected. Unreserved fields
in standalone point and legacy NTC source values retain their compatibility
behavior; reserved guard and provenance spellings fail closed.

Evaluated-truth and prefitted overlays are observation-space generation
inputs. They do not re-apply `input_transform` to samples. The member's
resolved observation domain (from applicability) is a constraint: the overlay
span must equal that domain. Standalone TOML overlays replace the declared
source and may define a different domain. Programmatic `TransferSpec` overlays
also define their own domain.

## Validation order

1. Deserialize with top-level and nested `deny_unknown_fields` on the family
   types.
2. Validate **every** member (selectors, identity, status/reason, and the
   source/member capability matrix for mapped statuses) before filtering
   non-`emit` statuses. An `unsupported` member is instead checked to ensure
   no source mapping was invented.
3. Expand only `status = "emit"` members into ordinary `TransferDef` values.
   Scaled-polynomial members receive the member's `input_transform` and an
   observation `domain` converted from `applicability.model_input`. Formula
   members receive `applicability.observation` as `domain`. Points members
   receive the clipped point set. NTC members receive `applicability.physical`
   as `output_range`.
4. Reject a gap name colliding with a curve, standalone transfer, family, or
   emitted member.
5. Run the existing identifier / companion-symbol collision check on the
   merged transfer set.

Canonical identity is the selector map itself: keys, value types, and values.
Value-only concatenation is not an identity. Generated names include selector
keys (`als_gain_div4_integration_time_ms_100`). If two distinct maps would
emit the same name, generation fails and prints both maps.

## Emission

Generated firmware remains independent `PiecewiseLinearTransfer` constants.
There is no runtime family type. A family-level `saturation = { code, behavior }`
table is copied onto emitted members as an observation-code guard; it is not
folded into `above` and the guarded code is not added to the fitting domain.

## Keep-outs

- Independent `extrapolation` fields
- VEML knot budget and vendor oracle (#29)
- `kind = "veml7700"` or any device lifecycle API
- Selector interpolation
- A runtime family registry
- Fetching provenance URLs or parsing vendor-specific source documents
