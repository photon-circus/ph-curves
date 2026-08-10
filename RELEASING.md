# Releasing ph-curves

Publishing is a deliberate, owner-only step. Nothing here happens automatically
on merge.

## Invariants a release must not break

These are the promises the crate is built on. If a release would weaken any of
them, it is not ready.

1. **The runtime is `no_std` and `no_alloc`.** `#![no_std]` is unconditional and
   no feature relaxes it. The `runtime-purity` CI job proves it by building the
   default feature set against a `core`-only sysroot; a plain `--target` build
   only proves `no_std`, because bare-metal `rust-std` ships `alloc`.
2. **The 0.1.x feature contract holds.** `cargo run --features gen --bin
   ph-curves-gen` must keep producing the CLI. The `feature-compat` CI job runs
   that exact command.
3. **Breaking changes are justified, gated, and documented.** See
   [docs/0.2.0/baseline-compatibility.md](https://github.com/photon-circus/ph-curves/blob/main/docs/0.2.0/baseline-compatibility.md)
   for the standard: a break earns its cost only when it removes a footgun that
   cannot be fixed additively. Ergonomics does not qualify.

## Pre-release checklist

- [ ] `main` contains the release commit, and CI is green on it.
- [ ] `Cargo.toml` `version` is the version being released.
- [ ] `CHANGELOG.md` has a dated `## [x.y.z] - YYYY-MM-DD` section — no
      entries left under `## [Unreleased]`. **Date it in UTC**, using the day
      you actually publish. crates.io records the publish time in UTC and the
      GitHub release displays UTC, so a local-time date reads as off by one
      against both whenever you release in the evening west of Greenwich.
      Check with `date -u +%F`, not the clock on the wall.
- [ ] `CHANGELOG.md` has a `[x.y.z]:` compare link at the bottom, and
      `[Unreleased]:` compares from the new tag.
- [ ] `SECURITY.md` lists the new minor line as supported.
- [ ] Public API additions carry rustdoc; `#![deny(missing_docs)]` enforces this
      but does not judge quality.
- [ ] Any new host-only module links `std` **module-locally**, never at the
      crate root. `scripts/local-ci.ps1` and CI both check this.

## Verify

```bash
cargo publish --dry-run
```

That packages, then compiles the packaged crate. Run the full gate first:

```bash
pwsh -File scripts/local-ci.ps1
```

Confirm the packaged file list contains no development-only paths. `docs/` and
`scripts/` are excluded in `Cargo.toml`:

```bash
cargo package --list
```

## Publish

```bash
git tag -a v0.2.0 -m "ph-curves 0.2.0"
```

Push the tag, then publish. The tag must exist first, so the `CHANGELOG.md`
compare links resolve:

```bash
git push origin v0.2.0
```

```bash
cargo publish
```

Then create the GitHub release from the tag, using the `CHANGELOG.md` section
as the body.

## After publishing

- [ ] Confirm docs.rs built successfully. `[package.metadata.docs.rs]` sets
      `features = ["gen-lib"]`; without it the `r#gen` build-script API is
      absent from the rendered docs, since docs.rs otherwise builds with
      default features only.
- [ ] Confirm the README badges resolve on crates.io — version, docs.rs, CI,
      license, MSRV, `no_std`.
- [ ] Add a fresh empty `## [Unreleased]` section to `CHANGELOG.md`.

## Version choice

Semantic versioning, with the pre-1.0 convention that a minor bump signals a
breaking change.

- **Patch** — fixes with no API change.
- **Minor** — additive API, or a breaking change while pre-1.0.
- Adding a variant to a public non-`#[non_exhaustive]` enum, or a field to a
  public struct, is breaking. Both apply to `TransferError`,
  `InverseTransferError`, `InterpolationError`, and `TransferMetadata`.

`TransferMetadata` deliberately is **not** `#[non_exhaustive]`: the code
generator emits a struct literal into the consumer's crate, which the attribute
would forbid. New metadata fields therefore require a generator-and-crate
lockstep bump.
