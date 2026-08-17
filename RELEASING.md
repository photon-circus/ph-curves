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
   [docs/compatibility.md](https://github.com/photon-circus/ph-curves/blob/main/docs/compatibility.md)
   for the standard: a break earns its cost only when it removes a footgun that
   cannot be fixed additively. Ergonomics does not qualify.
4. **The published dependency graph respects the documented Rust 1.92 MSRV.**
   The `downstream-msrv` CI job builds a fresh edition-2021, resolver-2 crate
   for `thumbv7em-none-eabi` and verifies that it selects `fixed` 1.30.x. The
   repository lockfile is not evidence for a downstream library consumer.

## Release integration

An active `release/x.y.z` branch may enter `main` only through a dedicated,
non-draft pull request whose head is that release branch and whose base is
`main`. Do not push or merge the branch into `main` out of band, and do not tag
or publish from the release branch.

Before opening that pull request, complete version/changelog/security metadata
and the full validation gate on the release branch. The pull request must expose
the complete aggregate release diff, identify the issue it closes, and state
which tag/publish steps remain owner-only after merge. Review the aggregate
diff, require the pull request's `ci` check to pass on the merge result, and
resolve every review conversation before merging it. The current head commit
must also have an explicit approving review from a human reviewer. Automated
review, an AI/agent audit, green CI, and resolved bot conversations are useful
evidence but never count as that human approval; until it is recorded, the
release gate is unsatisfied.

Only after the release pull request merges and `main` CI is green may the owner
create the annotated tag, publish to crates.io, and create the GitHub release.

## Pre-release checklist

- [ ] The dedicated non-draft `release/x.y.z` -> `main` pull request has been
      explicitly approved by a human reviewer on its current head commit, its
      required `ci` check is green, and every review conversation is resolved.
      Bot/AI review does not satisfy this item.
- [ ] That pull request is merged; `main` contains the release commit, and CI is
      green on it.
- [ ] `Cargo.toml` `version` is the version being released.
- [ ] **`Cargo.toml` `description` still describes the crate.** This is the
      text crates.io shows, and it is frozen into the published version — it
      cannot be corrected without releasing again. It drifts silently because
      the README tagline gets updated when features land and the manifest does
      not; 0.2.0 shipped describing only curves and scheduling, with no mention
      of transfer functions, calibration, or filtering. Read it against the
      README's opening line and against the `## Features` list.
- [ ] `keywords` and `categories` are still accurate, and `categories` are
      valid crates.io slugs — an invalid slug fails the upload, not the
      dry run.
- [ ] `CHANGELOG.md` starts with a real, empty `## [Unreleased]` heading,
      followed by a dated `## [x.y.z] - YYYY-MM-DD` section. No change entry or
      subsection remains under `Unreleased`. **Date the release in UTC**, using
      the day you actually publish. crates.io records the publish time in UTC
      and the GitHub release displays UTC, so a local-time date reads as off by
      one against both whenever you release in the evening west of Greenwich.
      Check with `date -u +%F`, not the clock on the wall.
- [ ] `CHANGELOG.md` has a `[x.y.z]:` compare link at the bottom, and
      `[Unreleased]:` compares from the new tag.
- [ ] `SECURITY.md` keeps the currently published minor supported until the
      new release is actually published and states the publication-triggered
      transition to the new minor precisely.
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
`scripts/` are excluded in `Cargo.toml`. It must include the documented `u16`
input and the three generated fixtures consumed by packaged examples:

```bash
cargo package --list
```

Required paths are `assets/curves-u16.toml`,
`tests/fixtures/family_acceptance_generated.rs`,
`tests/fixtures/ntc_generated.rs`, and
`tests/fixtures/observation_guards_generated.rs`; CI checks each exact entry.

## Publish

Start from a clean, current `main`, not whichever branch happens to be checked
out. The fast-forward-only update refuses a divergent local branch, and the
explicit equality check proves the tag target is exactly the reviewed commit on
`origin/main`:

```bash
set -eu
release_version=X.Y.Z
git fetch --prune --tags origin
git switch main
test -z "$(git status --porcelain)" || {
  echo "refusing to release from a dirty worktree" >&2
  exit 1
}
git pull --ff-only origin main
test "$(git rev-parse HEAD)" = "$(git rev-parse origin/main)" || {
  echo "local main is not exactly origin/main" >&2
  exit 1
}
test -z "$(git status --porcelain)" || {
  echo "refusing to tag a dirty worktree" >&2
  exit 1
}
case "$(cargo pkgid)" in
  *@"${release_version}") ;;
  *)
    echo "Cargo.toml package version does not match ${release_version}" >&2
    exit 1
    ;;
esac
git rev-parse -q --verify "refs/tags/v${release_version}" >/dev/null && {
  echo "tag v${release_version} already exists" >&2
  exit 1
}
git tag -a "v${release_version}" -m "ph-curves ${release_version}"
test "$(git rev-list -n 1 "v${release_version}")" = "$(git rev-parse origin/main)"
```

Push the verified tag, then publish from the same clean `main`. The tag must
exist first, so the `CHANGELOG.md` compare links resolve:

```bash
git push origin "v${release_version}"
```

```bash
cargo publish
```

Then create the GitHub release from the tag. `--verify-tag` refuses to invent
a tag if you mistyped it:

```bash
gh release create "v${release_version}" \
  --title "v${release_version} — short summary" \
  --notes-file notes.md \
  --verify-tag
```

Build `notes.md` from that version's `CHANGELOG.md` section. Lead with a few
highlights and the compatibility statement, since the changelog body is
organised by change type rather than by importance. If the tag also contains
repository-only work that is excluded from the package — docs, agent
instructions, CI config — say so, so the notes are not read as describing the
published artifact.

## After publishing

- [ ] Confirm docs.rs built successfully. `[package.metadata.docs.rs]` sets
      `features = ["gen-lib"]`; without it the `r#gen` build-script API is
      absent from the rendered docs, since docs.rs otherwise builds with
      default features only.
- [ ] Confirm the README badges resolve on crates.io — version, docs.rs, CI,
      license, MSRV, `no_std`.
- [ ] Confirm the empty `## [Unreleased]` heading remains first in
      `CHANGELOG.md`; future changes go there rather than into the frozen
      release section.
- [ ] Confirm the crates.io publication activated `SECURITY.md`'s support
      transition: the new minor is supported and the preceding minor is not.
- [ ] Refresh the GitHub repository description and topics if the release
      changed what the crate does. Unlike the manifest description, these are
      mutable at any time — but they drift for the same reason, so check them
      while the release is fresh.

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
