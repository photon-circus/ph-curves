# Gen build-script library API (design)

**Branch:** `feature/0.2.0-gen-build-api`  
**Status:** Design documentation only. Implementation lives on this branch only; do not merge to `main` without owner decision.

## Motivation

Today `ph-curves-gen` is a CLI binary; embedded crates want `build.rs` integration without awkward shell-outs. Exposing host codegen as `ph_curves::gen` (feature `gen`) improves adoption for both Curve and PR #1 Transfer generation, with no firmware API growth.

## Relation to PR #1 types

| PR #1 / main surface | This design |
| -------------------- | ----------- |
| `src/bin/gen/*` (+ PR #1 `transfer/` codegen) | Relocate into `src/gen/*` library modules |
| Generated `PiecewiseLinearTransfer` / metadata consts | Same TOML → Rust pipeline; CLI and `build.rs` share bytes |
| Runtime Transfer / Stabilize / Curve / Tickless | Unchanged when `gen` is off; firmware stays `no_std` |
| Host adaptive fit / NTC model floats | Remain behind `gen` only — never on the firmware path |

## API sketch

**Feature split**

| Feature | Enables | Audience |
| ------- | ------- | -------- |
| `gen` | `std` + `serde` + `toml` (no clap) | `build.rs` / host tools |
| `gen-cli` | `gen` + `clap` | `ph-curves-gen` binary only |
| default | no `gen` | Firmware dependents |

**Public entry points**

```rust
// ph_curves::gen
pub fn generate_from_toml(path: impl AsRef<Path>, opts: &GenerateOptions)
    -> Result<String, Error>;
pub fn generate_from_str(toml: &str, opts: &GenerateOptions)
    -> Result<String, Error>;
pub fn generate_to_path(input: impl AsRef<Path>, output: impl AsRef<Path>, opts: &GenerateOptions)
    -> Result<(), Error>;
pub fn generate(defs: &DefinitionsFile, opts: &GenerateOptions)
    -> Result<String, Error>;

pub struct GenerateOptions { /* value_type, lut_size; CLI-compatible defaults */ }
pub enum Error { Io(...), Toml(...), Validation(...) }
```

**Module layout (target):** `src/gen/{mod,api,codegen,curve,builtin,formula,points,transfer/*}.rs`; `src/bin/gen/main.rs` becomes thin clap → lib API.

**Consumer sketch**

```toml
[build-dependencies]
ph-curves = { version = "0.2", features = ["gen"] }
```

```rust
// build.rs
generate_to_path("assets/curves.toml", &out, &GenerateOptions::default())?;
```

## Keep-outs / bounds

- No proc-macro DSL (`#[curve(...)]` / derive embedding TOML) in 0.2.0
- No firmware API growth from enabling `gen`
- No separate crates.io `ph-curves-gen` package required for this design
- No WASM/plugin host ABI, runtime TOML watching, or proc-macro auto-invoke
- Keep CLI flags and generated source shape stable where practical

## Non-goals

- Changing Curve / Tickless / Transfer runtime types
- Expanding the built-in host model catalog as part of this refactor
- Landing on `main`, version bumps, or crates.io release from this branch

## Merge gate

Implementation lives on this branch only; do not merge to `main` without owner decision.
