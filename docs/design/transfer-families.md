# Transfer families and declared gaps

**Status:** Host generator schema in `gen-lib` — design rationale only; the
code and its rustdoc are authoritative.

## Motivation

Some devices expose discrete selector combinations that share one physical
source while varying scale and applicability. Duplicating standalone
`[transfers]` tables loses selector identity and encourages interpolating
between configurations that must not be interpolated. Sources may also
explicitly leave channels or procedures undefined; those gaps must remain
distinguishable from omissions.

Issue #24 is the generic schema, integrity, inspection, and sparse emission
slice of [#23](https://github.com/photon-circus/ph-curves/issues/23). It does
not add a device-specific model, a runtime family registry, or firmware API.

## Schema

A definitions document may contain `[transfer_families.<name>]` and
`[gaps.<name>]` alongside standalone `[transfers]`. Families forbid
`[curves]` in the same document so a 16-bit observation cannot silently take
the dense LUT path.

Each family has one shared source (`formula`, `points`, or `model`) and an
explicit `members` array. Selectors are string or integer maps and are never
interpolated (`interpolate_selectors = true` is an error). Per-member `scale`
is required and nonzero; it is inspectable here and is not applied to truth
until a later scaled-model issue. `status` is `emit`, `none`, or
`do_not_use`. Applicability is a generic `model_input = [min, max]` window.

Gaps require `status = "undefined"` and a non-blank `reason`. They are not
generated as transfers.

Unknown fields on family, member, applicability, and gap tables are rejected.
Illuminance-specific names such as `scale_micro_lux_per_count` and
`uncorrected_lux` are not part of this schema.

## Validation order

1. Deserialize with top-level and nested `deny_unknown_fields` on the family
   types.
2. Validate **every** member (selectors, scale, applicability, identity)
   before filtering non-`emit` statuses.
3. Expand only `status = "emit"` members into ordinary `TransferDef` values.
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
Family knot default is 64 with a hard cap of 256. There is no runtime family
type.

## Keep-outs

- `kind = "scaled_polynomial"` and applying member scale to host truth (#30)
- Independent `saturation` / `extrapolation` fields (#28)
- VEML knot budget and vendor oracle (#29)
- The full host inspection/extension IR (#25); this slice only exposes
  read-only `transfer_families()` / `gaps()` views
- `kind = "veml7700"` or any device lifecycle API
