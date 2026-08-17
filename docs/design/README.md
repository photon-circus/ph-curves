# Design records

These documents explain why ph-curves APIs and generator contracts have their
current shape. They capture constraints, non-goals, compatibility decisions,
and alternatives that do not belong in the concise root README.

For callable behavior, use the current [public API documentation](https://docs.rs/ph-curves).
API sketches in a design record may describe the state at the time of the
decision rather than the latest spelling.

## Host generation and inspection

| Record | Scope |
| --- | --- |
| [Generator build API](gen-build-api.md) | Feature split, build-script integration, generation options, reports, and resource budgets |
| [Host transfer IR](host-transfer-ir.md) | Definition, validation, inspection, overlay, and generation layers |
| [Transfer families and declared gaps](transfer-families.md) | Selector universes, completeness, provenance, stable emitted identity, and family emission |

Suggested path: read the generator build API first for the entry point, the
host IR for lifecycle and extension boundaries, then transfer families for the
full multi-member schema.

## Runtime conversion and calibration

| Record | Scope |
| --- | --- |
| [Inverse transfer](inverse-transfer.md) | Inverse lookup semantics, rounding, boundaries, and calibration composition |
| [Affine calibration](affine-calibration.md) | Checked integer gain/offset/scale calibration and round-trip constraints |

## Application decisions

| Record | Scope |
| --- | --- |
| [Hysteresis and debounce](decision-primitives.md) | Fixed-memory decision state, cadence assumptions, bounds, and non-goals |

The root [README pipeline quick start](../../README.md#quick-start-stabilize-and-decide)
shows how the shipped temporal and decision primitives compose. The design
record explains the boundary between reusable state machines and caller-owned
device policy.

## How to read these records

- **Shipped behavior:** verify exact signatures and errors in rustdoc.
- **Compatibility:** use the [compatibility policy](../compatibility.md) for
  release promises and migrations.
- **Historical references:** linked issues and pull requests explain the
  decision trail; their status text is not the current API contract.
- **Keep-outs and non-goals:** treat these as intentional scope boundaries
  unless a later accepted design record supersedes them.

Return to the [documentation map](../README.md).
