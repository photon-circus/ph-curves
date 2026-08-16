# 0.2.0 baseline compatibility

Assessment of every 0.2.0 change that could break the 0.1.2 baseline, what was
done about it, and whether breaking would have been worth it.

## Result

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
unknown fields on family, member, applicability, and gap types are rejected.
Standalone curves still ignore unknown direct fields. Standalone transfers
reject unknown direct fields, while legacy NTC model parameters remain
permissive for compatibility.

No runtime API is involved. The version that ships a TOML tightening is a
release decision, not part of the behaviour change.

## Host TOML: fail-closed standalone observation guards

A standalone transfer using `saturation` must opt into the localized
capability in the already-known transfer section:

```toml
[transfers]
requires = ["observation_guard_v1"]

[transfers.sensor]
saturation = { code = 65535, behavior = "error" }
# remaining required transfer fields...
```

The location and shape are intentional. Released 0.2.x generators model
`[transfers]` as `BTreeMap<String, TransferDef>` and ignore unknown fields
inside each transfer. They therefore reject the `requires` array as an invalid
transfer value before they can silently discard `saturation`. A new unknown
top-level key would not provide that guarantee because older releases ignored
unknown top-level keys too. Unknown capability names and malformed capability
values fail closed. The capability must correspond to at least one direct
`saturation` guard, and reserved guard keys found inside a model or point value
are rejected as misplaced. This catches TOML table-scope mistakes without
tightening unrelated legacy NTC extensions.

A table named `[transfers.requires]` remains a legal legacy transfer name when
the document has no observation guard; only the array form is the marker.
Because TOML cannot represent both forms at once, that transfer must be renamed
before any standalone guard is added. The parser diagnoses this combination
explicitly.

The current parser also rejects unknown fields directly on standalone transfer
definitions, so a misspelled guard cannot disappear. Nested standalone curve
fields and legacy NTC model parameters retain their previous permissive parsing
for compatibility. Programmatic `TransferSpec` construction does not need a
wire-format capability marker.

This is a deliberate TOML compatibility tightening and must ship with the same
next pre-1.0 minor release as the observation-guard runtime API. A broader
whole-document version policy remains a separate schema decision.

## Generated namespace: observation-guard companions

Every generated transfer emits and reserves
`<NAME>_OBSERVATION_GUARD: Option<ObservationGuardMetadata>`, including `None`
for an unguarded transfer. Uniform presence means adding or removing a guard
does not also add or remove a Rust symbol, and it satisfies the metadata
contract without changing `TransferMetadata` struct literals.

The cost is a new generated-name collision: a previously valid pair such as
`foo` and `foo_observation_guard` is now rejected. Rename one transfer before
regenerating. This is an intentional pre-1.0 generated-namespace break and is
part of the next minor-release decision, not a patch-release change.

## Host `GenerateOptions` on transfer-only documents

`value_type` and `lut_size` describe dense curve LUTs. They are now validated
only when the definitions contain `[curves]`. Transfer-only and family-only
generation may use `GenerateOptions::transfers_only()`; mismatched LUT fields
are ignored rather than rejected. Documents that still contain curves keep the
existing full-domain LUT check. No runtime API is involved; the change is
additive for `gen-lib` callers.
