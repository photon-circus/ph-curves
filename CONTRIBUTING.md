# Contributing to ph-curves

Thanks for your interest in contributing! This document covers the basics.

## Getting started

1. Fork and clone the repo.
2. Make sure you have Rust **1.92.0+** installed (or use the bundled `rust-toolchain.toml`).
3. Run the tests:

```sh
cargo test
cargo test --features gen
```

## Making changes

- Create a feature branch from `main`.
- Keep commits focused — one logical change per commit.
- Follow the existing code style. Run `cargo fmt` and `cargo clippy` before submitting.
- This is a `no_std` library. Avoid introducing `std` dependencies in the core crate.
- If you add or change public API, update the doc comments accordingly.
- Add or update tests to cover your changes.

## Submitting a pull request

1. Push your branch and open a PR against `main`.
2. Describe **what** you changed and **why**.
3. Make sure CI passes (formatting, clippy, tests).

## Reporting bugs

Open an issue on [GitHub](https://github.com/photon-circus/ph-curves/issues) with:

- A clear description of the problem.
- Steps to reproduce (minimal example if possible).
- Expected vs. actual behaviour.
- Rust version and target platform.

## Feature requests

Feature requests are welcome — please open an issue describing your use-case before starting work on a large change so we can discuss the approach.

## Code of conduct

This project follows the [Contributor Covenant Code of Conduct](CODE_OF_CONDUCT.md). By participating you agree to uphold it.

## License

By contributing you agree that your contributions will be licensed under the [MIT License](LICENSE.md).
