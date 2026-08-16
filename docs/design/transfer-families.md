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
slice of [#23](https://github.com/photon-circus/ph-curves/issues/23). The
host inspection/extension IR is documented in
[host-transfer-ir.md](host-transfer-ir.md). This slice does not add a
device-specific model, a runtime family registry, or firmware API.

## Schema

A definitions document may contain `[transfer_families.<name>]` and
`[gaps.<name>]` alongside standalone `[transfers]`. Families forbid
`[curves]` in the same document so a 16-bit observation cannot silently take
the dense LUT path.

Each family has one shared source (`formula`, `points`, or `model`) and an
explicit `members` array. Selectors are string or integer maps and are never
interpolated (`interpolate_selectors = true` is an error). Per-member `scale`
is required and nonzero. For `kind = "scaled_polynomial"` it is applied to
host truth as `u = count * scale / 1e6` (integer product first, so the
`< 2^48` value is exact in `f64` before dividing) and inclusive
`applicability.model_input` is converted to an observation-domain window with
that same `u`. Formula and points sources leave scale inspectable only.
Coefficients are `[c0, c1, c2, ...]` for `y = c0 + c1*u + c2*u^2 + ...`,
evaluated with Horner from the high-degree end. Standalone polynomial
definitions carry their own `scale` and `domain`. `u16::MAX` is legal unless
the caller’s window excludes it. `status` is `emit`, `none`, or `do_not_use`.

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
   Scaled-polynomial members receive the member's `scale` and an observation
   `domain` converted from `applicability.model_input`.
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

- Independent `saturation` / `extrapolation` fields (#28)
- VEML knot budget and vendor oracle (#29)
- `kind = "veml7700"` or any device lifecycle API
