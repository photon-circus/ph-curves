## Summary

<!--
What changed and why. Prefer why — the diff already says what. If this fixes a
defect, say what the wrong behaviour was, not only that it is now right.
-->

## Test plan

<!--
Tick what you ran. A test that passes against the fix but was never seen to
fail against the defect has not been shown to test anything.
-->

- [ ] `cargo test`
- [ ] `./scripts/local-ci.ps1` — fmt, no_std guards, feature-matrix tests, clippy `-D warnings`, rustdoc, no-std and core-only targets, ESP Xtensa, cargo-deny, package
- [ ] New tests were verified to fail against the unfixed code

## Checklist

- [ ] `CHANGELOG.md` updated under `## [Unreleased]` for user-visible changes
- [ ] Public items have rustdoc (`#![deny(missing_docs)]` enforces presence, not quality)
- [ ] No allocation added on the runtime path; any `std` use is module-local under `src/gen`
- [ ] Generated fixtures regenerated if table generation changed

<!--
Breaking changes need to clear the bar in docs/compatibility.md and be called
out here explicitly. Ergonomics does not qualify.
-->
