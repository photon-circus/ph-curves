# Compatibility policy and release history

This document records compatibility decisions for published releases and the
standard a future breaking change must meet.

## 0.3.0 result

**0.3.0 is a deliberate pre-1.0 minor release with documented breaks against
0.2.1.** It adds `TransferError::RejectedObservation` to a public exhaustive
enum, reserves and emits the generated `<NAME>_OBSERVATION_GUARD` companion,
and tightens standalone transfer TOML by rejecting formerly ignored unknown
direct fields and requiring localized capability markers so guard and
provenance features fail closed across generator-version skew. These changes
remove safety and schema footguns that could not be fixed additively. The
transfer-family schema is new in 0.3.0 and therefore does not break a previously
published family document.

The release also adds device-neutral transfer families, structured provenance
and generation reports, standalone affine transforms, `u32` temporal
primitives, and 16-bit-pointer target support. The sections below record the
exact compatibility boundaries and migration requirements.

## 0.2.0 result

Assessment of every 0.2.0 change that could break the 0.1.2 baseline, what was
done about it, and whether breaking would have been worth it.

**0.2.0 has no breaking changes against 0.1.2.** A 0.1.2 dependency
declaration, a 0.1.2 `cargo run --features gen` invocation, and 0.1.2 firmware
source all keep working unchanged.

Two candidate breaks existed. Both were reworked rather than shipped.

## The 0.1.2 baseline

Public surface at `v0.1.2`:

- Types: `Curve`, `CurveLut`, `CurveLut256`, `CurveLut65536`, `MonotonicCurve`,
  `MonotonicCurveLut`, `MonotonicCurveLut256`, `MonotonicCurveLut65536`
- Math: `Rounding`, `UnitValue`, `lerp_u8`, `lerp_u16`, `map_u8_to_u16`,
  `next_target_value`, `quantize`
- Tickless: `RepeatMode`, `Tickless`, `TicklessDeadline`, `TicklessIter`,
  `TicklessSchedule`
- Crate attribute: unconditional `#![no_std]`
- Features: `gen = ["dep:serde", "dep:toml", "dep:clap"]`, which builds the
  `ph-curves-gen` binary

Everything 0.2.0 adds — the transfer layer, stabilization, decision primitives,
the `r#gen` library module — is new namespace. None of it touches the list
above.

## Candidate break 1: `gen` stops building the CLI

**As originally proposed.** `gen` became the library-only feature and the
binary moved behind a new `gen-cli`. Every existing
`cargo install --features gen` and `cargo run --features gen` would have
failed, silently producing no binary rather than erroring usefully.

**Why it was proposed.** A `build.rs` consumer wanting the codegen API should
not compile `clap`.

**How it was avoided.** Three features instead of two, with the 0.1.x name
keeping its 0.1.x meaning:

```toml
std     = []
gen-lib = ["std", "dep:serde", "dep:toml"]   # build.rs / host tools
gen-cli = ["gen-lib", "dep:clap"]            # the binary
gen     = ["gen-cli"]                        # 0.1.x compatibility alias
```

The build-script consumer gets a clap-free `gen-lib`, and the existing
invocation is untouched. The only cost is one extra feature name.

**Enforced by.** The `feature-compat` CI job runs
`cargo run --features gen --bin ph-curves-gen -- --help`. If `gen` ever stops
producing the binary, CI fails on the exact command existing users run.

## Candidate break 2: `#![no_std]` becomes conditional

**As originally proposed.** `#![cfg_attr(not(feature = "gen"), no_std)]`,
because the generator needs `std::fs`.

**Why this one was worse than it looked.** It is not only a baseline break. It
is *unsound as a guarantee*, because Cargo unifies features across the
dependency graph: any crate anywhere enabling `ph-curves/gen` would flip the
firmware build to `std`, at a distance, with no diagnostic. A no-std promise
that a third-party dependency can revoke is not a promise.

**How it was avoided.** `#![no_std]` is unconditional. The crate root does not
`extern crate std`. Each module under `src/gen` links `std` with a
module-local `extern crate std` and imports `std::prelude::v1::*` by hand.
Because the prelude is not crate-wide, a stray `String` or `format!` on the
runtime path is a compile error rather than a silent allocator dependency —
the boundary is enforced by the type system, not by review.

**Enforced by.** The `runtime-purity` CI job:

1. Fails if `#![no_std]` is feature-conditional or missing.
2. Builds the default feature set against a `core`-only sysroot
   (`-Z build-std=core`) on thumbv7em, thumbv6m, and riscv32imc.

Step 2 matters more than it appears. A plain `cargo build --target
thumbv7em-none-eabi` **passes** with an `alloc` dependency, because bare-metal
`rust-std` ships `alloc` in the sysroot. This was verified by adding
`extern crate alloc` to `src/math.rs`: the plain target build succeeded, and
only the `core`-only build failed with `can't find crate for alloc`. The
pre-0.2.0 CI matrix would not have caught an alloc regression. This one does.

## Would breaking the baseline have been a future positive?

No, on both counts — and the reasoning differs.

**For the `gen` feature**, the break bought nothing that the alias does not
also buy. The goal was a clap-free build-script path; that is achieved by
adding `gen-lib`, not by redefining `gen`. Breaking would have traded real
downstream churn for a marginally tidier feature table. The one long-term cost
of the alias is a permanent third feature name and the note explaining it —
cheap, and it can be deprecated on a future major without urgency.

**For `no_std`**, the break was actively negative. Relaxing the attribute
would have weakened the crate's central guarantee — deterministic,
allocation-free firmware behavior — in exchange for avoiding roughly ten
prelude imports. That is a bad trade at any price, because the guarantee is
the product. The unconditional form is also strictly more robust than what
0.1.2 shipped: 0.1.2 was `#![no_std]` by luck of never having a host module,
whereas 0.2.0 is `#![no_std]` with a host module and a CI gate proving the
separation holds.

**Where breaking *would* be justified.** Worth recording so the bar is
explicit. A break earns its cost when it removes a footgun that cannot be
fixed additively — for example, if `TransferMetadata` needed a field whose
absence made generated metadata wrong, or if a public signature made a
correctness bug unrepresentable only by changing it. Neither applied here.
Both candidates were ergonomics, and ergonomics is what additive design is for.

## Note on `#[non_exhaustive]`

`TransferMetadata` is a plausible candidate for `#[non_exhaustive]`, since it
gained five fields during 0.2.0 and may gain more. It must **not** get the
attribute: the code generator emits a `TransferMetadata { .. }` struct literal
into the *consumer's* crate, and `#[non_exhaustive]` forbids literal
construction outside the defining crate. Adding it would break every generated
file. Future fields therefore need a generator-and-crate lockstep bump, which
is the tradeoff the codegen design already accepts.

## Host TOML: unknown top-level tables

Host definitions now reject unknown top-level keys. Serde's default is to
ignore them, so a misspelled `[tranfsers…]` table used to parse as empty
`curves` and `transfers` maps. `generate_from_str` then succeeded with
header-only output, which a `build.rs` consumer reads as a compatible
generator while every expected symbol is missing.

That silent omission cannot be fixed additively while remaining fail-closed:
keeping the ignore-unknown default would keep dropping tables an older
generator does not understand. Narrowing formerly accepted documents is the
cost of making schema evolution explicit.

`[transfer_families]` and `[gaps]` are now known top-level tables. Nested
unknown fields on family, shared point entry, shared NTC model, member,
applicability, input-transform, and gap types are rejected. Standalone curves
still ignore unknown direct fields. Standalone transfers reject unknown direct
fields, while other, unreserved fields nested in standalone point values and
legacy standalone NTC model parameters remain permissive for compatibility.

The family member schema published in 0.3.0 is source-aware: every
accepted field must change validation, fitting, emission, metadata, or
documentation, and a field unsupported by the selected source fails closed.
Mapped `emit`, `unnecessary`, and `forbidden` members require exactly the
source-specific mapping. `unsupported` is the explicit no-mapping state and
forbids applicability and input transforms instead of requiring fabricated
source coordinates. Every family also declares its expected selector universe
with exactly one of `selector_axes` (Cartesian product) or
`expected_selectors` (explicit maps). Each expected identity is occupied by
exactly one member or family-scoped gap; document-level `[gaps]` do not
satisfy that occupancy. Cartesian cardinality uses checked multiplication and
must fit the generator host's `usize`; completeness validation compares that
count with indexed, duplicate-free occupancy without materializing the
product. A source-backed family also requires structured
`provenance.identity`. Members and family-scoped gaps inherit that citation;
their optional overrides use the same replace/clear rules and are validated
against the family citation. A selector key literally named `provenance`
remains part of the typed selector identity. Together, these constraints are
the intended first-publish shape. `[transfer_families]` has not shipped in
0.2.1, so this is not a 0.2.x document break. Legacy standalone transfer TOML
without the new provenance or guard fields is unchanged. The first-publish
shape is now evidenced by the device-neutral acceptance fixture
(`assets/family-acceptance.toml`, `tests/family_acceptance.rs`,
`tests/family_acceptance_gen.rs`): two selector axes, distinct member
transforms, an explicit gap and description-only statuses, observation-guard
parity, TOML/`FamilySpec`/overlay convergence, and mixed curve/family
emission. 0.3.0 is the first release that publishes families and keeps that
capability matrix, required provenance, and declared universe; it does not ship
accepted-but-inert member fields or undeclared selector spaces. The
observation-guard and provenance breaks recorded below ship in the same minor.

No runtime API is involved. 0.3.0 ships the TOML tightening as a release
decision, not as part of runtime behaviour. A broader whole-document
`schema_version` field remains a separate schema decision.

## Host TOML: fail-closed standalone guards and provenance

A standalone transfer using `saturation` must opt into the localized
capability in the already-known transfer section:

```toml
[transfers]
requires = ["observation_guard_v1"]

[transfers.sensor]
saturation = { code = 65535, behavior = "error" }
# remaining required transfer fields...
```

A standalone transfer using `provenance` must likewise declare
`"source_provenance_v1"`:

```toml
[transfers]
requires = ["source_provenance_v1"]

[transfers.sensor]
provenance = { identity = "device data sheet", locator = "Table 1" }
# remaining required transfer fields...
```

List both capability strings in the same array when both features are present.
This is deliberately localized: family provenance is part of the
first-published 0.3.0 `[transfer_families]` shape and needs no compatibility
marker; programmatic `TransferSpec` construction has no wire format.

The location and shape are intentional. Released 0.2.x generators model
`[transfers]` as `BTreeMap<String, TransferDef>` and ignore unknown fields
inside each transfer. They therefore reject the `requires` array as an invalid
transfer value before they can silently discard `saturation`. A new unknown
top-level key would not provide that guarantee because older releases ignored
unknown top-level keys too. Released 0.2.1 readers also ignored standalone
`provenance`, so the source-provenance marker provides the same rejection
guarantee. Unknown capability names and malformed capability values fail
closed. Each capability must correspond to at least one direct use of its
feature. Reserved guard and provenance keys found inside a model or point value
are rejected as misplaced, even when another direct citation makes the
capability otherwise appear used. This catches TOML table-scope mistakes
without tightening unrelated legacy NTC extensions.

A table named `[transfers.requires]` remains a legal legacy transfer name when
the document has neither a standalone observation guard nor source provenance;
only the array form is the marker. Because TOML cannot represent both forms at
once, that transfer must be renamed before either capability is added. The
parser diagnoses this combination explicitly.

The current parser also rejects unknown fields directly on standalone transfer
definitions, so a misspelled guard cannot disappear. Nested standalone curve
fields and unreserved standalone point/legacy NTC model parameters retain their
previous permissive parsing for compatibility; reserved guard and provenance
spellings fail closed.

This is a deliberate TOML compatibility tightening shipped in 0.3.0 with the
observation-guard and provenance host APIs. A broader whole-document version
policy remains a separate schema decision.

## Generated namespace: observation-guard companions

Every generated transfer emits and reserves
`<NAME>_OBSERVATION_GUARD: Option<ObservationGuardMetadata>`, including `None`
for an unguarded transfer. Uniform presence means adding or removing a guard
does not also add or remove a Rust symbol, and it satisfies the metadata
contract without changing `TransferMetadata` struct literals.

The cost is a new generated-name collision: a previously valid pair such as
`foo` and `foo_observation_guard` is now rejected. Rename one transfer before
regenerating. This is an intentional pre-1.0 generated-namespace break and is
part of the 0.3.0 minor release, not a patch-release change.

## Host `GenerateOptions` on transfer-only documents

`value_type` and `lut_size` describe dense curve LUTs. They are now validated
only when the definitions contain `[curves]`. Transfer-only and family-only
generation may use `GenerateOptions::transfers_only()`; mismatched LUT fields
are ignored rather than rejected. Documents that still contain curves keep the
existing full-domain LUT check. No runtime API is involved; the change is
additive for `gen-lib` callers.

## Host generation reports and family aggregate budgets

`generate_report` / `generate_from_str_report` / `generate_from_toml_report`
and `ValidatedDefinitions::generate_report` are additive `gen-lib` APIs. The
existing `String`-returning helpers remain and internally run the report
pipeline, so an aggregate budget cannot be bypassed by calling `generate`.
No report type is on the default-feature runtime path.

The report mirrors the validated description graph without moving citation
strings into firmware. Emitted transfers expose effective overlay provenance,
pre-overlay observation-guard provenance, and citation-free policy. Family
entries retain the compact selector universe, completeness result, family
provenance and budgets, all members (including non-emitting statuses), and
scoped gaps; document-level gaps are separate name-ordered entries. Declared
member/gap provenance overrides remain distinct from their effective resolved
citations. Totals and aggregate budgets continue to count emitted tables only.

Optional family keys `max_total_knots` and `max_table_bytes` are part of the
first-published 0.3.0 `[transfer_families]` shape, not a 0.2.1 document break.
Omitted keys mean no aggregate cap. Payload accounting includes only the
emitted `_INPUTS` and `_OUTPUTS` arrays (six bytes per knot). It excludes
`PiecewiseLinearTransfer` fields, `_METADATA`, `_OBSERVATION_GUARD`, and
symbol/section overhead. Curve LUT bytes are excluded from transfer document
totals. Duplicate tables are counted once per member.

`TransferMetadata` is unchanged. Observation-guard facts stay on the adjacent
companion constant and are copied into the host report when present.

## Host family IR, programmatic construction, and emitted identity

`ValidatedFamily` / `ValidatedMember` now expose units, the exact validated
`FamilySource` through `ValidatedFamily::source`, resolved observation domain,
and resolved emitted identity so a caller does not need to retain the
pre-validation `DefinitionsFile`. Exact
source inspection includes formula text, points, scaled-polynomial
coefficients, or all NTC Beta-divider parameters; `ModelDef` remains private.
`FamilySpec` / `FamilySource` and `insert_family` are additive `gen-lib` APIs
equivalent to TOML family construction. Optional member `emitted_name` is an
opt-in key on the first-published 0.3.0 `[transfer_families]` shape; omitted, the
stem stays the derived family-plus-selector expansion. `EmissionManifest` and
report companion-symbol fields are host-only. None of these types enter the
default-feature runtime path. Generated family-member rustdoc grows two comment
lines (family name and selector map); standalone transfers are unchanged.

## 16-bit-pointer targets

The 0.3.0 runtime is checked against `msp430-none-elf` with a core-only
sysroot. The generic curve, transfer, affine, and temporal APIs compile there.
The `CurveLut65536` and `MonotonicCurveLut65536` convenience aliases are
conditionally absent when `target_pointer_width = "16"`, because the required
array length 65,536 cannot be represented by that target's `usize`. Smaller
generic LUTs remain available.

This conditional surface is not a regression: earlier releases failed to
compile on 16-bit-pointer targets at those two aliases. On such targets every
window representable by `usize` fits the `i64` moving-average accumulator for
`u32`, so `TemporalSample` caps the window at `usize::MAX`. Targets with
32-bit or wider pointers retain the accumulator-derived cap
`floor(i64::MAX / u32::MAX) = 2_147_483_648`.
