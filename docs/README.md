# Documentation

This directory holds the durable compatibility record and design rationale for
ph-curves. Start with the repository [README](../README.md) for a concise
product overview and use-case quick starts, then follow the routes below when
you need the detailed contract or the reasoning behind it.

## Choose a route

| Goal | Read |
| --- | --- |
| Install the crate or run a first example | [README quick starts](../README.md#quick-starts) |
| Understand a runtime type, error, panic, or invariant | [Public API documentation](https://docs.rs/ph-curves) |
| Go beyond a quick start with practical workflow guidance | [User-guide index](guides/README.md) |
| Migrate between releases or assess a deliberate break | [Compatibility policy and release history](compatibility.md) |
| Understand the host generator and build-script API | [Generator build API](design/gen-build-api.md) |
| Inspect or extend validated transfer definitions | [Host transfer IR](design/host-transfer-ir.md) |
| Model selector-complete device families and declared gaps | [Transfer families](design/transfer-families.md) |
| Understand runtime conversion, calibration, or decisions | [Design-record index](design/README.md) |
| See every change in a release | [CHANGELOG](../CHANGELOG.md) |

## Documentation authority

The same subject can appear at several levels. When details differ, use this
order:

1. Public Rust API documentation and the implementation define the callable
   contract.
2. [Compatibility policy](compatibility.md) defines release and migration
   commitments.
3. The root [README](../README.md) provides orientation and representative
   workflows, not an exhaustive schema or API reference.
4. [Design records](design/README.md) preserve decisions, constraints, and
   rejected alternatives. Their historical API sketches are not substitutes
   for current rustdoc.
5. The [CHANGELOG](../CHANGELOG.md) inventories release-specific changes.

## Examples and verification inputs

- [`assets/`](../assets) contains representative curve, transfer, guard, and
  transfer-family definitions.
- [`examples/`](../examples) contains buildable integration examples.
- [`tests/`](../tests) contains independent runtime and generator acceptance
  checks.

The `docs/` directory is repository documentation and is intentionally not
included in the published crate package. Links from packaged documentation use
GitHub or docs.rs where necessary.

## Maintainer paths

- [Contributing](../CONTRIBUTING.md)
- [Release process](../RELEASING.md)
- [Security policy](../SECURITY.md)
- [Code of conduct](../CODE_OF_CONDUCT.md)
