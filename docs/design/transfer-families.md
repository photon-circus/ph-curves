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
transfers keep default 256 / cap 4096. Optional family-level `max_total_knots`
and `max_table_bytes` bound the **sum** of emitted members. They are
independent of per-member `max_knots`: every member may fit its own cap and
generation still fails if the family total exceeds the aggregate. Omitted
means no aggregate cap. Zero is rejected during validation. Diagnostics name
the family, the field, the requested limit, and the achieved amount. Knots are
checked before bytes, families in name order.

Array payload is six bytes per knot (`u16` input + `i32` output). That is the
`_INPUTS` plus `_OUTPUTS` static arrays only. Structural runtime overhead is
excluded. Identical tables are not deduplicated; duplicated payload remains
visible in the totals.

Each family has one shared source (`formula`, `points`, or `model`) and an
explicit `members` array. Selectors are string or integer maps and are never
interpolated. `interpolate_selectors` is not a field; leftover copies are
unknown-field errors.

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
applicability, input-transform, and gap tables are rejected. Standalone point
and legacy NTC source values retain their compatibility behavior.

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

Host generation returns a structured report alongside the source. Each
emitted transfer records resolved family/member identity (empty for
standalones), the Rust symbol, observation domain and physical range,
requested and achieved interpolation error, worst-case input, knot count,
array-payload bytes, the fitting path actually used (overlays replace the
declared TOML source), and observation-guard metadata when present.
Transfers are ordered by table name; families by family name. Document
totals include every emitted transfer and exclude curve LUT bytes.
Description-only members are absent from the report.

## Keep-outs

- Independent `extrapolation` fields
- VEML knot budget and vendor oracle (#29)
- `kind = "veml7700"` or any device lifecycle API
- Selector interpolation
- A runtime family registry
