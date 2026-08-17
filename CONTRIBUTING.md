# Contributing to ph-curves

Thanks for your interest in contributing! This document covers the basics.

## Getting started

1. Fork and clone the repo.
2. Make sure you have Rust **1.92.0+** installed (or use the bundled `rust-toolchain.toml`).
3. Run the tests:

```sh
cargo test
cargo test --features gen-lib
cargo test --features gen-cli
cargo build --features gen-cli --bin ph-curves-gen
```

## Making changes

- Create a feature branch from `main`.
- Keep commits focused — one logical change per commit.
- Follow the existing code style. Run `cargo fmt` and `cargo clippy` before submitting.
- This is a `no_std` library. Avoid introducing `std` dependencies in the core crate.
- If you add or change public API, update the doc comments accordingly.
- Add or update tests to cover your changes.

## Submitting a pull request

1. Push your branch and normally open a PR against `main`. When a maintainer has
   named an active `release/x.y.z` branch, changes explicitly scoped to that
   release may target it instead.
2. Describe **what** you changed and **why**.
3. Make sure CI passes (formatting, clippy, tests).

An active release branch enters `main` only through its own dedicated,
non-draft release pull request. That aggregate pull request must be explicitly
approved by a human reviewer on its current head, pass the required `ci` check,
and have every review conversation resolved. Automated review does not replace
that approval; do not integrate a release branch into `main` directly.

## Reporting bugs

Open an issue on [GitHub](https://github.com/photon-circus/ph-curves/issues) with:

- A clear description of the problem.
- Steps to reproduce (minimal example if possible).
- Expected vs. actual behaviour.
- Rust version and target platform.

## Feature requests

Feature requests are welcome — please open an issue describing your use-case before starting work on a large change so we can discuss the approach.

## Releasing

Publishing to crates.io is an owner-only step and is not automatic on merge.
The checklist, the invariants a release must not break, and the version-choice
rules live in [RELEASING.md](RELEASING.md).

## Code of conduct

This project follows the [Contributor Covenant Code of Conduct](CODE_OF_CONDUCT.md). By participating you agree to uphold it.

## License

By contributing you agree that your contributions will be licensed under the [MIT License](LICENSE.md).
