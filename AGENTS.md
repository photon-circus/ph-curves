# Agent instructions

Guidance for coding agents working in this repository. Read this before
changing code.

## What this crate is for

`ph-curves` gives firmware deterministic, allocation-free curve lookup,
ADC-to-measurement conversion, and tickless scheduling. **The `no_std` +
`no_alloc` runtime is the product**, not a nice-to-have. A change that weakens
it is wrong even if it compiles, passes tests, and is more ergonomic.

## Hard invariants

Do not break these. Each is enforced mechanically; the enforcement exists
because each one was broken at least once.

### 1. `#![no_std]` is unconditional

Never make it feature-conditional. Cargo unifies features across the whole
dependency graph, so `#![cfg_attr(not(feature = "..."), no_std)]` means any
unrelated crate enabling a host feature silently turns a firmware build into a
`std` build, at a distance, with no diagnostic. A guarantee a third-party
dependency can revoke is not a guarantee.

### 2. `std` is linked module-locally, never at the crate root

`src/gen/mod.rs` has `extern crate std;`. Each file under `src/gen` imports
`std::prelude::v1::*` by hand.

Do **not** move this to the crate root, and do **not** add `#[macro_use]`. That
form puts `format!` and friends in scope crate-wide, so an accidental
allocation on the runtime path compiles instead of failing. Keeping the prelude
out of the crate root is what makes a stray `String` in `src/transfer.rs` a
compile error.

If you add a host-only module, follow the same pattern.

### 3. A plain `--target` build does not prove `no_alloc`

Bare-metal `rust-std` ships `alloc` in the sysroot, so
`cargo build --target thumbv7em-none-eabi` **succeeds** with an `alloc`
dependency. Only a `core`-only sysroot proves it:

```bash
cargo +nightly build --target thumbv7em-none-eabi -Z build-std=core
```

`rust-toolchain.toml` pins 1.92.0 and a rustup file override outranks whatever
a CI action selected, so the `+nightly` is required, not stylistic — a bare
`cargo` here resolves to stable and rejects `-Z`.

### 4. The 0.1.x `gen` feature contract

Features are `gen-lib` (build.rs library API, no clap), `gen-cli` (adds the
binary), and `gen` — an alias for `gen-cli` kept so existing
`cargo install --features gen` and `cargo run --features gen` keep working.

Never redefine `gen` to mean something else. If you need a new host capability,
add a new feature name.

## Design rules that are easy to get wrong

**One rounding helper.** `crate::round::div_nearest_ties_away` is the single
nearest/ties-away implementation. Transfer interpolation (both directions),
affine calibration, and the temporal filters all route through it. Do not add
another; a second implementation is how quantized values start disagreeing
between modules.

**Boundary policies are declared against the observation domain.** `below` and
`above` describe inputs under `domain_min` / over `domain_max`. Inverse
conversion maps them onto the physical range through the table's
`MonotonicDirection` (see `range_behaviors`). On a decreasing table the codes
above `domain_max` are the ones producing physical values *below* `range_min`,
so selecting a policy by physical side alone makes the two directions disagree
about the same condition.

**Round trips are bounded, not exact.** Both directions round, so
`invert(convert(x))` through a calibration can differ from `x`. What *is*
guaranteed: anything `convert` produces is invertible. A calibrated value
within half an uncalibrated quantum of a range endpoint clamps to that endpoint
rather than range-erroring.

**`TransferMetadata` cannot be `#[non_exhaustive]`.** The generator emits a
struct literal into the *consumer's* crate, which the attribute forbids. Adding
a field is therefore a breaking change requiring a generator-and-crate lockstep
bump. Same reasoning applies to any type codegen constructs by literal.

**Metadata must describe the table, not runtime state.** `flat_resolution` was
removed from `TransferMetadata` because `with_flat_resolution` can change it
after the fact, so the baked-in copy contradicted the live policy. Do not add
fields that a builder method can invalidate.

## Working on the generator

Host code lives in `src/gen`. `src/bin/gen/main.rs` is a thin CLI over it.

Regenerate the checked-in fixture whenever table generation changes:

```bash
cargo run --features gen-cli --bin ph-curves-gen -- --input assets/transfers.toml --output tests/fixtures/ntc_generated.rs
```

`tests/ntc_transfer.rs` re-measures the emitted table at runtime and asserts it
matches the generator's recorded metadata. That cross-check is what catches
drift between host and runtime arithmetic — if you change one side's rounding,
it fails. Host audits should reuse `interpolate_segment` / `invert_segment`
rather than reimplementing the math.

## Validating

Run the same gate CI runs:

```powershell
./scripts/local-ci.ps1
```

It covers format, the `no_std` guards, tests across default / `gen-lib` /
`gen-cli` / `gen`, clippy at `-D warnings`, rustdoc, the no-std and core-only
target matrices, ESP32 Xtensa, and packaging. Dependency policy is
`cargo deny check`.

CI runs on **pull requests**; `push` is limited to `main`. A branch with no PR
open gets no CI, so open the PR to get coverage.

## Gotchas

- **Raw identifiers cannot appear in intra-doc links.** `` [`r#gen::foo`] ``
  fails the `-D warnings` rustdoc gate — rustdoc parses `r` out of it. Use a
  plain code span.
- **Feature-gated modules are invisible on docs.rs by default.**
  `[package.metadata.docs.rs]` sets `features = ["gen-lib"]` so `r#gen` is
  documented. Check this if you add another gated public module.
- **Verify a guard fires before trusting it.** Several checks here were written,
  looked right, and did nothing. Break the invariant deliberately, confirm the
  check fails, then restore.

## Conventions

- Match surrounding style; the codebase favours explicit integer math and
  documents *why* rather than *what*.
- `#![deny(missing_docs)]` is on. Public items need rustdoc.
- Update `CHANGELOG.md` under `## [Unreleased]` for user-visible changes.
- Releasing is documented in [RELEASING.md](RELEASING.md) and is owner-only.
- Compatibility policy — including the bar a breaking change must clear — is in
  [docs/compatibility.md](docs/compatibility.md).
