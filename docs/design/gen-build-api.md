# Gen build-script library API (design)

**Status:** Shipped in 0.2.0 — `ph_curves::r#gen` behind the `gen-lib` feature. Design rationale only; the code and its rustdoc are authoritative.

## Motivation

Today `ph-curves-gen` is a CLI binary; embedded crates want `build.rs` integration without awkward shell-outs. Exposing host codegen as `ph_curves::r#gen` improves adoption for both Curve and PR #1 Transfer generation, with no firmware API growth.

## Relation to PR #1 types

| PR #1 / main surface | This design |
| -------------------- | ----------- |
| `src/bin/gen/*` (+ PR #1 `transfer/` codegen) | Relocate into `src/gen/*` library modules |
| Generated `PiecewiseLinearTransfer` / metadata consts | Same TOML → Rust pipeline; CLI and `build.rs` share bytes |
| Runtime Transfer / Stabilize / Curve / Tickless | Unchanged when host features are off; firmware stays `no_std` |
| Host adaptive fit / NTC model floats | Remain behind `gen-lib` / `gen-cli` only — never on the firmware path |

## Feature split (shipped)

| Feature | Enables | Audience |
| ------- | ------- | -------- |
| `gen-lib` | `std` + `serde` + `toml` (no clap) | `build.rs` / host tools — **prefer this** |
| `gen-cli` | `gen-lib` + `clap` | `ph-curves-gen` binary only |
| `gen` | alias for `gen-cli` | 0.1.x compatibility; still builds the CLI |
| default | no host gen | Firmware dependents |

## API sketch

**Public entry points**

```rust
// ph_curves::r#gen  (`gen` is a Rust 2024 keyword; use the raw identifier)
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

**Module layout:** `src/gen/{mod,api,codegen,curve,builtin,formula,points,transfer/*}.rs` exposed as `ph_curves::r#gen`; `src/bin/gen/main.rs` is a thin clap → lib API.

**Consumer sketch**

```toml
[build-dependencies]
ph-curves = { version = "0.2", features = ["gen-lib"] }
```

```rust
// build.rs
generate_to_path("assets/curves.toml", &out, &GenerateOptions::default())?;
```

## Keep-outs / bounds

- No proc-macro DSL (`#[curve(...)]` / derive embedding TOML) in 0.2.0
- No firmware API growth from enabling `gen-lib`
- No separate crates.io `ph-curves-gen` package required for this design
- No WASM/plugin host ABI, runtime TOML watching, or proc-macro auto-invoke
- Keep CLI flags and generated source shape stable where practical
- Library generate path does not print transfer-fit progress to stderr (avoids spamming `build.rs` logs); the CLI may report write status separately

## Non-goals

- Changing Curve / Tickless / Transfer runtime types
- Expanding the built-in host model catalog as part of this refactor
