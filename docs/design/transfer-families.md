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
accepted member field source-aware and effectful. Issue
[#40](https://github.com/photon-circus/ph-curves/issues/40) requires a declared
selector universe and family-scoped gaps so an omitted combination is not
indistinguishable from an intentional hole. The host inspection/extension IR
is documented in [host-transfer-ir.md](host-transfer-ir.md). This slice does
not add a device-specific model, a runtime family registry, or firmware API.

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

A source-backed family requires a structured `provenance` table with a
non-blank `identity` (title or stable identifier). Optional `revision`,
`locator`, `url`, and `note` fields locate the cited document. URLs are stored
as opaque strings and are never fetched. Members inherit that citation unless
they declare a `provenance` override. Set fields replace; optional fields named
by `clear = ["revision", "locator", "url", "note"]` are removed. Unset fields
inherit while `identity` is unchanged. Replacing `identity` starts a new
citation, so unspecified optional fields are cleared instead of being combined
with fields from the old document. Family-scoped gaps inherit and may override
the family citation under the same rules; `ValidatedFamilyGap` exposes the
effective citation and declared override. Global `[gaps]` may declare their
own citation; without a parent family they still require `identity` when
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

Every family declares its expected selector universe with exactly one of:

- `selector_axes` — named axes whose Cartesian product is expected (`BTreeMap`
  axis-key order, then each axis's declared value order);
- `expected_selectors` — an explicit list of selector maps for a non-Cartesian
  family. Missing product cells are not invented.

Cartesian cardinality is the checked product of the declared axis lengths.
Validation rejects a product that cannot be represented by the host's
`usize`; it never allocates the Cartesian product. Explicit universes are
indexed once for exact identity and per-key type checks instead of being
rescanned for every member.

Every expected identity is occupied by exactly one member (any status) or one
family-scoped gap. A missing occupancy is an error; so is a member or
family-scoped gap outside the universe, a wrong or missing selector key, a
selector value type that disagrees with a homogeneous axis, or the same typed
map used as both a member and a gap. Integer `1` and string `"1"` remain
distinct identities.

Family-scoped gaps are `[[transfer_families.<name>.gaps]]` records with the
same typed selector map, `status = "undefined"`, a non-blank `reason`, and an
optional family-relative `provenance` override. A selector key named
`provenance` inside `selectors` remains an ordinary identity key.
Document-level `[gaps.<name>]` remain globally named `{ status, reason }`
records. They do not occupy family selector identities and do not satisfy
completeness.

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
generated as transfers. Family-scoped gaps carry a selector identity;
document-level gaps do not.

Unknown fields on family, shared point entry, shared NTC model, member,
applicability, input-transform, family-scoped gap, and document-level gap
tables are rejected. Unreserved fields in standalone point and legacy NTC
source values retain their compatibility behavior; reserved guard and
provenance spellings fail closed.

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
3. Resolve the declared selector universe (`selector_axes` xor
   `expected_selectors`). Reject empty axes, duplicate typed axis values,
   empty or duplicate expected maps, and heterogeneous key sets in
   `expected_selectors`. Reject Cartesian cardinality overflow using checked
   multiplication.
4. Check every member and family-scoped gap against that universe (keys,
   value types, and membership). Duplicate member identities, duplicate
   family-scoped gap identities, and member/gap occupancy of the same map
   fail here. Because those checks prove that occupied identities are a
   duplicate-free subset of the universe, completeness is exactly
   `occupied_count == checked_universe_cardinality`; validation does not
   materialize or enumerate a Cartesian product.
5. Expand only `status = "emit"` members into ordinary `TransferDef` values.
   Scaled-polynomial members receive the member's `input_transform` and an
   observation `domain` converted from `applicability.model_input`. Formula
   members receive `applicability.observation` as `domain`. Points members
   receive the clipped point set. NTC members receive `applicability.physical`
   as `output_range`.
6. Reject a document-level gap name colliding with a curve, standalone
   transfer, family, or emitted member.
7. Run the existing identifier / companion-symbol collision check on the
   merged transfer set.

Canonical identity is the selector map itself: keys, value types, and values.
Value-only concatenation is not an identity. Generated names default to
including selector keys (`als_gain_div4_integration_time_ms_100`). An optional
member `emitted_name` overrides that stem without changing selector identity,
so a display-token rename does not force a symbol rename. Explicit and derived
stems share one emit-member collision set; if two distinct maps would emit the
same name, generation fails and identifies both origin families and their exact
typed selector maps. Description-only members may declare a stem but do not
occupy that set and are not generated.

Validated host IR exposes the checked count through
`SelectorUniverse::identity_count`. `SelectorUniverse::identities` is lazy:
it yields one owned selector map at a time in the documented order and stores
only axis references and positions, so inspecting a prefix never allocates the
full product.

## Emission

Generated firmware remains independent `PiecewiseLinearTransfer` constants.
There is no runtime family type. Generated rustdoc for a family member names
the family and the exact typed selector map. A family-level `saturation = { code, behavior }`
table is copied onto emitted members as an observation-code guard; it is not
folded into `above` and the guarded code is not added to the fitting domain.

Host generation returns a structured report alongside the source. Each
emitted transfer records resolved family/member identity (empty for
standalones), the Rust symbol, observation domain and physical range,
requested and achieved interpolation error, worst-case input, knot count,
array-payload bytes, the fitting path actually used (overlays replace the
declared TOML source), observation-guard metadata, citation-free generation
policy, effective source provenance, and the independently resolved
pre-overlay guard citation. Family records retain the family citation, guard
citation, policy, compact selector universe/completeness, aggregate-budget
declarations, every member (including description-only statuses), and scoped
gaps. Member and gap records keep effective provenance separate from the
declared override; emitted members map to their table and symbol. Named
document-level gaps are reported independently. Transfers are ordered by table
name, families and document gaps by name, and family members/scoped gaps by
declaration order. Totals include every emitted transfer, exclude curve LUT
bytes, and never count description-only members or gaps.

## Keep-outs

- Independent `extrapolation` fields
- VEML knot budget and vendor oracle (#29)
- `kind = "veml7700"` or any device lifecycle API
- Selector interpolation
- A runtime family registry
- Fetching provenance URLs or parsing vendor-specific source documents
