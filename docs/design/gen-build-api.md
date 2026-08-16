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
pub fn generate_report(defs: &DefinitionsFile, opts: &GenerateOptions)
    -> Result<GenerationResult, Error>;
pub fn generate_from_str_report(toml: &str, opts: &GenerateOptions)
    -> Result<GenerationResult, Error>;
pub fn generate_from_toml_report(path: impl AsRef<Path>, opts: &GenerateOptions)
    -> Result<GenerationResult, Error>;

pub struct GenerateOptions { /* value_type, lut_size; ignored without [curves] */ }
pub fn GenerateOptions::transfers_only() -> Self;

pub const TABLE_BYTES_PER_KNOT: usize = 6; // u16 input + i32 output arrays only
pub struct GenerationResult { pub source: String, pub report: GenerationReport }
pub struct GenerationReport {
    pub transfers: Vec<TransferReport>, // sorted by table name
    pub families: Vec<FamilyReport>,    // sorted by family name; all members/gaps
    pub gaps: Vec<GapReport>,           // document-level, sorted by name
    pub totals: ResourceTotals,         // all emitted transfers; no curve LUTs
}

// Inspection / extension IR (host-only; not a plugin ABI)
impl DefinitionsFile {
    pub fn validate(&self) -> Result<ValidatedDefinitions, Error>;
    pub fn insert_transfer(&mut self, spec: TransferSpec) -> Result<(), Error>;
    pub fn insert_family(&mut self, spec: FamilySpec) -> Result<(), Error>;
    pub fn curves(&self) -> &BTreeMap<String, CurveDef>;
    pub fn transfers(&self) -> &BTreeMap<String, TransferDef>;
    pub fn transfer_families(&self) -> &BTreeMap<String, TransferFamilyDef>;
    pub fn gaps(&self) -> &BTreeMap<String, GapDef>;
}
impl ValidatedDefinitions {
    pub fn set_source(
        &mut self,
        name: &str,
        overlay: TransferSourceOverlay,
    ) -> Result<(), Error>;
    pub fn insert_family(&mut self, spec: FamilySpec) -> Result<(), Error>;
    pub fn emission_manifest(&self) -> EmissionManifest;
    pub fn generate(&self, opts: &GenerateOptions) -> Result<String, Error>;
    pub fn generate_report(&self, opts: &GenerateOptions) -> Result<GenerationResult, Error>;
}
impl TransferSource {
    pub fn inherit_provenance(self) -> TransferSourceOverlay;
    pub fn with_provenance(self, provenance: SourceProvenance) -> TransferSourceOverlay;
    pub fn clear_provenance(self) -> TransferSourceOverlay;
}
pub enum Error { Io(...), Toml(...), Validation(...) }
```

`TransferReport` keeps runtime metadata metrics together with the fitting path,
effective source provenance, independently resolved pre-overlay guard
provenance, and citation-free `GenerationPolicy`. `FamilyReport` preserves the
family citation, guard citation, policy, compact `SelectorUniverse`,
completeness result, total-budget declarations, every member, and scoped gaps.
Member and gap report records expose their effective provenance separately
from the declared override; only emitted members carry a table/symbol mapping
plus `_METADATA` and `_OBSERVATION_GUARD` companion names. `FamilySpec`
constructs the same family graph as TOML. `emission_manifest` is the pre-fit
identity map (family + typed selectors → stem/symbol/companions).
Resource totals and aggregate budget enforcement continue to count emitted
members only. Member and scoped-gap vectors retain declaration order.

The overlay citation disposition is mandatory. Use inheritance only when the
new representation still comes from the already-cited source. It means the
target's declared, resolved pre-overlay citation and restores that citation
when replacing an earlier overlay. Standalone overlays may replace or clear a
citation; emitted family-member overlays may inherit or replace it but cannot
clear it.

**Module layout:** `src/gen/{mod,api,ir,codegen,curve,builtin,formula,points,rustdoc,transfer/*}.rs` exposed as `ph_curves::r#gen`; `src/bin/gen/main.rs` is a thin clap → lib API.

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
- No WASM/plugin host ABI, runtime TOML watching, or proc-macro auto-invoke.
  Device crates inspect and extend through the public IR in
  [host-transfer-ir.md](host-transfer-ir.md) instead of a callback/trait plugin.
- Keep CLI flags and generated source shape stable where practical
- Library generate path does not print transfer-fit progress to stderr (avoids spamming `build.rs` logs); the CLI may report write status separately
- String-returning `generate*` helpers stay; they call the report pipeline so family aggregate budgets fail closed without a CLI `--report` flag
- Array payload in reports is `_INPUTS` + `_OUTPUTS` only (`TABLE_BYTES_PER_KNOT = 6`); structural/runtime overhead is excluded

## Non-goals

- Changing Curve / Tickless / Transfer runtime types
- Expanding the built-in host model catalog as part of this refactor
