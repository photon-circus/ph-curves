# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `AffineTransform`, a standalone invertible `i32` affine map
  `y' = (y * gain + offset) / scale`. It reuses the crate's nearest/ties-away
  rounding and checked `i64` arithmetic, rejects `scale == 0` and `gain == 0`
  at construction, and reports overflow through `AffineOverflow` rather than
  transfer domain/range errors. Use it on an already-converted measurement;
  `AffineCalibration` still wraps a transfer and now contains this primitive.
- Device-neutral transfer-family acceptance fixtures and documentation. A
  mixed definitions document (`assets/family-acceptance.toml`) proves a
  synthetic multi-range ADC family: two selector axes, three emitted members
  with distinct input transforms and applicability windows, a forbidden
  mapped member, a selector-addressed family gap, an unsupported combination,
  ordinary boundaries plus an observation guard, an unrelated normalized
  curve, and a document-level gap. Runtime tests exhaustively check conversion
  against an independent quadratic oracle. Host tests prove TOML/`FamilySpec`
  parity, evaluated-truth overlays, fail-closed completeness/provenance/budget
  checks, structured reports, and rustdoc family/selector/provenance mapping.
  Representative generated fixtures compile on the no-std and core-only
  target matrix via `examples/no_std_generated_fixtures.rs`. The README
  worked example is this ADC front end; VEML remains a downstream integration
  concern.
- Complete host family IR, programmatic family construction, and stable
  emitted member identity. `ValidatedFamily` exposes units, output scale,
  its exact `FamilySource` through `ValidatedFamily::source` (including the
  scaled-polynomial coefficients or complete NTC Beta-divider parameters),
  aggregate budgets, and the existing
  policy/provenance/universe graph. `ValidatedMember` exposes the resolved
  observation-code domain, the selector-derived stem, and an optional explicit
  `emitted_name`. `FamilySpec` / `FamilySource` construct a family without TOML;
  `insert_family` feeds the same validate/generate pipeline. An optional member
  `emitted_name` keeps the generated stem stable when selector display spelling
  changes; explicit and derived stems share one collision set whose diagnostics
  identify both origin families and exact typed selector maps.
  `ValidatedDefinitions::emission_manifest` maps family plus typed selectors to
  the table stem, symbol, and `_METADATA` / `_OBSERVATION_GUARD` companions
  before fitting. Generated rustdoc for family members names the family and the
  exact selector map. Description-only members and gaps stay inspectable and
  have no runtime symbol. No builder or manifest type enters the default-feature
  runtime path.
- Host-only generation reports and optional family aggregate resource
  budgets. `generate_report`, `generate_from_str_report`, and
  `generate_from_toml_report` return the generated source together with
  per-transfer metrics (identity, symbol, domain/range, requested and
  achieved error, worst-case input, knot count, array-payload bytes,
  fitting path, observation-guard metadata, citation-free generation policy,
  effective source provenance, and separately resolved guard provenance).
  Family reports retain their citation, guard citation, policy, compact
  selector universe/completeness, aggregate-budget declarations, every member
  including description-only statuses, and family-scoped gaps. Member and gap
  records expose effective provenance separately from declared overrides;
  emitted members map directly to generated table and symbol names. Named
  document-level gaps are also reported. Family and document totals count
  every emitted table only; duplicate payload is not coalesced.
  Array payload is six bytes per knot (`u16` input + `i32` output);
  `PiecewiseLinearTransfer` fields, `_METADATA`, `_OBSERVATION_GUARD`,
  and symbol overhead are excluded. Optional family
  `max_total_knots` / `max_table_bytes` fail closed after every member
  has fitted, naming the family, requested limit, and achieved amount.
  Per-member `max_knots` is unchanged. Existing `String`-returning
  helpers call the report path and cannot bypass a budget. No report
  type enters the default-feature runtime path.
- Issue forms for bug reports and feature requests, and a pull request
  template. Blank issues are disabled so the chooser always renders, which is
  what puts the private disclosure route in front of someone about to paste a
  vulnerability into a public issue. The bug form asks which surface is
  involved — runtime, `gen-lib`, or `gen-cli` — because the `no_std` runtime
  and the host generator fail in unrelated ways.
- Host-only structured source provenance, separate from generation policy.
  A `provenance` table records identity plus optional revision, locator, URL,
  and note. Source-backed families require `provenance.identity`; members
  inherit the family citation unless they declare an override. Overrides can
  explicitly clear optional fields, and replacing `identity` resets every
  unspecified optional field so citations from two documents cannot be
  combined accidentally. Family-scoped gaps inherit the family citation and
  may override it with the same set/clear rules; validated gap IR exposes the
  effective citation and declared override. Document-level gaps may carry
  their own independent citation.
  `TransferSpec::with_provenance` and source overlays supply the same type as
  TOML. Each overlay explicitly inherits, replaces, or clears the target
  citation; emitted family-member overlays cannot clear it. Inheritance means
  the target's declared, resolved pre-overlay citation, so it restores that
  citation when replacing an earlier overlay. Standalone TOML
  provenance requires `[transfers] requires = ["source_provenance_v1"]`, whose
  shape makes 0.2.1 readers reject instead of silently discarding the citation.
  Provenance nested inside a model or point value is rejected as misplaced.
  Host inspection exposes `SourceProvenance` and a citation-free
  `GenerationPolicy` as distinct values; generated rustdoc labels source provenance,
  representation (the selected formula/points/model), and policy separately.
  Observation-guard classification remains consumer/device policy unless the
  `saturation` table cites a source, in which case rustdoc names that citation
  and still applies the classification as declared policy. A family guard's
  citation always resolves against family provenance, even when a member has
  its own citation. A standalone guard citation resolves against the declared
  transfer citation before an overlay, so replacing or clearing source
  provenance does not rewrite or remove the guard citation. URLs are stored
  and never fetched. Runtime
  `TransferMetadata` and observation-guard companions do not retain citation strings.
- Host-only `[transfer_families]` and `[gaps]` tables. A family shares one
  formula, points, or model source across explicit selector members; only
  `status = "emit"` members become independent `PiecewiseLinearTransfer`
  constants. Non-`emit` statuses are `unnecessary`, `unsupported`, and
  `forbidden`, each requiring a non-blank `reason`. `unnecessary` and
  `forbidden` members retain a validated source mapping; `unsupported`
  members have no mapping and therefore set no applicability coordinate or
  input transform. Gaps record
  `status = "undefined"` with a non-blank reason and are not generated.
  `DefinitionsFile::transfer_families` and `gaps` are read-only inspection
  views. Every family declares `selector_axes` (Cartesian product) or
  `expected_selectors` (explicit maps); each expected identity is occupied
  by exactly one member or family-scoped gap with a typed selector map and
  a non-blank reason. Document-level `[gaps]` do not satisfy that occupancy.
  Cartesian cardinality is checked for overflow, completeness is proven from
  indexed duplicate-free occupancy and count equality without materializing
  the product, and host IR enumeration is lazy. Explicit-universe membership
  and selector-type checks use a single precomputed index rather than repeated
  linear scans.
  Selectors are never interpolated, unknown nested family source,
  member, applicability, and gap fields are rejected, and every member is
  validated before non-emitted statuses are filtered. Mapped member fields
  follow a source capability matrix: formula and points use
  `applicability.observation`, NTC uses `applicability.physical`, and
  `scaled_polynomial` requires
  `input_transform = { numerator, denominator }` plus
  `applicability.model_input`. Family-scoped gap tables reject unknown
  fields. Families may share a document with unrelated
  `[curves]`; dense LUT generation stays curve-only.
- Host-only transfer inspection and extension IR. `DefinitionsFile::validate`
  returns a `ValidatedDefinitions` graph of families, every member (including
  `unnecessary` / `unsupported` / `forbidden`), and gap reasons without
  generating Rust.
  `ValidatedFamily` exposes the declared selector universe, every member,
  family-scoped gaps with resolved/declared provenance, typed selector
  identities, and completeness status.
  `TransferSpec` / `TransferSource` construct or overlay evaluated physical
  truth and prefitted knots so a device crate can own source interpretation
  while reusing ph-curves fitting, metadata, and `PiecewiseLinearTransfer`
  codegen. Prefitted knots require dense truth so emitted accuracy metadata is
  always verified. `GenerateOptions::transfers_only` skips dense LUT validation
  when the document has no `[curves]`. There is still no plugin/evaluator ABI.
- Host-only `kind = "scaled_polynomial"` models. Coefficients are
  `[c0, c1, ...]` for `y = c0 + c1*u + c2*u^2 + ...` with Horner evaluation.
  Model input is the exact rational `u = count * scale / 1e6` (integer product
  first) for standalone definitions. Family members keep shared coefficients
  and required per-member `input_transform = { numerator, denominator }`;
  standalone definitions carry their own scale and observation domain.
  Inclusive applicability bounds convert to codes without the floating-point
  off-by-one from pre-rounding `scale / 1e6`. `u16::MAX` is legal unless the
  caller excludes it. Empty or non-finite coefficients, zero scale, invalid
  domain, non-monotonic truth, non-finite output, and scaled `i32` overflow
  fail closed. Unknown scaled-polynomial model fields are rejected; the legacy
  NTC model continues to ignore unknown model parameters for compatibility.
  Generated firmware remains integer knots.
- Explicit observation-code guard on piecewise-linear transfers, independent
  of the fitted domain and of ordinary `below` / `above` policy. Forward
  conversion classifies a declared code first: `Error` returns
  `TransferError::RejectedObservation`, `Clamp` returns the output at
  `domain_max`. Inverse conversion is unchanged. The host schema names the
  field `saturation = { code, behavior }` (consumer/device policy, not
  inferred from the integer); host IR and runtime use observation-guard
  terminology. The guarded code must be strictly above `domain_max` and is
  not added to the fitting domain. Metadata is an adjacent
  `Option<ObservationGuardMetadata>` constant so `TransferMetadata` struct
  literals stay additive. Standalone TOML guards require
  `[transfers] requires = ["observation_guard_v1"]`; its array shape makes
  older transfer-map decoders fail instead of silently dropping `saturation`.
  Unused capabilities and guard keys misplaced inside model/point values are
  rejected. A legacy transfer named `requires` must be renamed before opting
  into the capability.

### Changed

- `AffineCalibration` contains an `AffineTransform` and delegates
  gain/offset/scale arithmetic to it. Constructor, accessor, forward, inverse,
  boundary, and error behavior are unchanged: `AffineCalibrationError` is
  still the construction error, overflow still surfaces as
  `TransferError::Overflow` / `InverseTransferError::Overflow`, and
  compressed-endpoint invertibility still lives on the wrapper.
- Generated rustdoc for family members now includes the family name and typed
  selector map. Host `TransferReport` / `FamilyMemberReport` also list
  `_METADATA` and `_OBSERVATION_GUARD` companion symbol names. Standalone
  transfer rustdoc is unchanged.
- **Unreleased host schema:** transfer-family member fields are source-aware.
  Accepted-but-inert `scale` / `applicability.model_input` on formula, points,
  and NTC members are rejected. `interpolate_selectors` is removed (discreteness
  is an invariant). Member statuses are `emit`, `unnecessary`, `unsupported`,
  and `forbidden`; non-`emit` statuses require a non-blank `reason`.
  `unsupported` represents a selector combination without a source mapping,
  while the other three statuses require the source-specific mapping. Families
  declare a selector universe (`selector_axes` or
  `expected_selectors`) and proves completeness with members and
  family-scoped gaps. Families may share a document with unrelated
  `[curves]`; dense LUT fallback remains curve-only. This is the publish
  shape of `[transfer_families]`, which has not shipped in 0.2.1. Legacy
  standalone transfers that do not use the new provenance or guard fields
  remain unchanged. A whole-document `schema_version` field remains a
  separate decision.

- **Breaking (pre-1.0):** `TransferError` gained `RejectedObservation { input }`
  so a deliberately rejected observation code is not an `AboveDomain`.
  Exhaustive downstream matches need a new arm. `PiecewiseLinearTransfer`
  stores an optional guard and is larger by that field; unguarded construction
  keeps the previous convert/invert behavior.
- **Breaking (pre-1.0 generated namespace):** every generated transfer now
  emits and reserves `<NAME>_OBSERVATION_GUARD`, including a `None` constant
  for unguarded transfers. A document containing both `foo` and
  `foo_observation_guard` must rename one transfer. The uniform `Option`
  companion keeps symbol presence stable when guard policy changes; this and
  the runtime API break require the next pre-1.0 minor release.
- `scripts/local-ci.ps1` sets `CARGO_INCREMENTAL=0`. Incremental compilation
  made the gate flaky on Windows: rustc could fail to finalize
  `target/debug/incremental` ("Access is denied", os error 5) and `cargo test`
  exited 101 while reporting every test as passing, landing on a different
  feature each run so the failure read as a real, moving defect. CI builds
  fresh and gains nothing from incremental.
- The README tagline now matches the manifest `description`, covering inverse
  transfers, calibration, and temporal filters. It had drifted the other way
  from the case `RELEASING.md` warns about — the manifest was the stale copy
  before 0.2.1 — but the tagline is the first thing a reader sees on both
  GitHub and crates.io, so the two should not disagree about what the crate
  does.
- `[transfer_families]` and `[gaps]` are now known top-level definition
  tables. Nested unknown fields on those types, on family point and NTC model
  sources, and on family members and applicability are rejected. Standalone
  curve definitions and unreserved fields in legacy standalone point/NTC
  source values retain their permissive compatibility behavior.

### Fixed

- `ValidatedDefinitions::insert_transfer` now rebuilds the validated graph
  transactionally, matching `insert_family`. Normalized Rust-identifier and
  companion-symbol collisions are rejected by the insertion call without
  leaving the previously validated definitions partially mutated.
- Generated rustdoc now escapes Markdown/HTML syntax in every user-derived
  documentation string (names, representation, units, and citations), so text
  such as `[missing]` cannot become a broken intra-doc link under `-D warnings`.
- Family source fields now fail closed: unknown keys in family point entries
  or NTC model tables are rejected with the family and source path instead of
  being accepted and discarded.
- Overlay domain constraints now apply only to emitted family members.
  Standalone TOML and programmatic transfer overlays may replace the original
  source with a different observation domain.
- Generated Rust files end with exactly one newline, so regenerated fixtures
  no longer introduce a blank line at end of file.

- Documentation CI now runs rustdoc with `--features gen-lib`, matching the
  docs.rs feature set. The previous default-features-only invocation never
  compiled `src/gen`, so a broken intra-doc link in the host generator could
  not fail the gate.
- The host generator now rejects unknown top-level definition tables instead
  of succeeding with header-only output. A misspelled `[tranfsers…]` table
  previously parsed as empty `curves`/`transfers` maps and looked like a
  compatible `build.rs` run while omitting every expected symbol. Parse now
  returns `Error::Toml` and names the unrecognized field.
- Standalone transfer definitions now reject unknown direct fields. A typo such
  as `saturaton`, or the unsupported runtime-oriented name
  `observation_guard`, therefore fails TOML parsing instead of producing an
  unguarded table. Other, unreserved nested legacy NTC model parameters remain
  permissive.

## [0.2.1] - 2026-08-10

### Added

- `AGENTS.md` with the repository's hard invariants for coding agents, and a
  `CLAUDE.md` shim pointing at it.
- `deny.toml` and a `cargo deny check` CI job covering advisories, licences,
  bans, and sources. The licence allow-list is exactly what the graph uses, so
  a new licence shows up as a diff rather than passing silently.

### Changed

- `Cargo.toml` `description` now covers transfer functions, calibration, and
  filtering. 0.2.0 published describing only curves and tickless scheduling,
  because the README tagline was updated when those features landed and the
  manifest was not. The manifest text is what crates.io displays and is frozen
  per version, so 0.2.0's listing cannot be corrected — this takes effect on
  the next release. `RELEASING.md` now checks it.
- Documentation reorganised by kind rather than by version: `docs/design/` for
  per-feature design records and `docs/compatibility.md` for the durable
  compatibility policy. The former `docs/0.2.0/` pinned durable policy to a
  release and would have orphaned every design doc at the next one.
- The README's contributing section described GitHub Actions as disabled and
  `scripts/local-ci.ps1` as the gate. Actions run on every pull request.
- README now documents `AffineCalibration`, `Hysteresis`, and `Debounce`, which
  shipped in 0.2.0 without any README coverage, plus `invert_segment`.

### Fixed

- `TicklessSchedule` used saturating absolute wall-clock math
  (`t0.saturating_add(duration)`, `now >= end`, `now.saturating_add(min_dt)`).
  On a free-running `u32` ms clock that wraps every ~49.7 days, a segment
  starting near `u32::MAX` clamped `end_ms` short and treated post-rollover
  timestamps as before the start, stalling the ramp partway — forcing callers
  to pass segment-relative elapsed with `t0_ms = 0` as a workaround.
  Scheduling now classifies `now_ms` against the segment with a half-range
  signed delta, and clamps the deadline on offsets from `t0_ms` rather than on
  absolute timestamps. Offsets are bounded by `duration_ms`, so the
  comparisons stay ordinary `u32` ones and long relative durations
  (`t0_ms == 0`, `duration_ms` up to `u32::MAX`) from the 0.1.2 `UnitValue`
  fix keep working unchanged.
- `TicklessSchedule::end_ms` now wraps rather than saturating, and
  `next_deadline` past the segment end reports `now_ms` rather than the
  clamped end. For any segment that does not cross the rollover both are
  identical to 0.2.0. Callers comparing `end_ms()` with `<` or `>` against a
  raw timestamp should switch to wrapping remaining-time
  (`end_ms().wrapping_sub(now)`); ordering comparisons on absolute values are
  not meaningful across the rollover.
- Two README links resolved only inside the repository and 404'd from the
  published crate, where `rust-toolchain.toml` and `.github/` are excluded.

### Removed

- The 0.2.0 release-train narrative. Its durable content — the no-alloc proof
  mechanism, the `std` boundary rule, the codegen/metadata lockstep — moved to
  `AGENTS.md`, where it changes behaviour instead of recording history.

## [0.2.0] - 2026-08-10

### Added

- `InverseTransferFunction` and `PiecewiseLinearTransfer::invert`, mapping a
  physical setpoint back to an observation on the same sparse knots with no
  dense physical-domain LUT. `FlatResolution` selects how a value landing on a
  flat (non-unique) output run resolves; `InverseTransferError` reports
  out-of-range and ambiguous-flat cases against the physical range.
- `invert_segment`, the public mirror of `interpolate_segment`, so host tools
  and the runtime share one rounding implementation.
- `AffineCalibration` implements `InverseTransferFunction` when its inner
  transfer does, so a calibrated setpoint — "which ADC code reads 25 °C after
  this unit's factory trim?" — is a single `invert` call. Range errors are
  re-expressed in calibrated units, and a calibration whose `gain` and `scale`
  have opposite signs flips `BelowRange` and `AboveRange` accordingly.
- `PiecewiseLinearTransfer::range_behaviors` reports how the domain boundary
  policies map onto the physical range.
- Transfer metadata now records `range_min` / `range_max`,
  `strictly_monotonic`, `flat_segment_count`, and an exhaustively measured
  `achieved_max_inverse_code_error` round-trip bound.
- `AffineCalibration` wrapper that applies caller-supplied `i32`
  gain/offset/scale after any `TransferFunction<Output = i32>` using checked
  `i64` math (nearest, ties-away). Overflow surfaces as
  `TransferError::Overflow`; `scale == 0` returns
  `AffineCalibrationError::ZeroScale` at construction.
- `Hysteresis` and `Debounce` decision primitives beside the temporal filters.
  Both are sample-count only: they read no clock and touch no GPIO.
- `ph_curves::r#gen`, a `build.rs` / host-tool library API behind the `gen`
  feature, exposing `generate_from_toml`, `generate_from_str`,
  `generate_to_path`, `generate`, `GenerateOptions`, and `ValueType`.
  Generator validation now returns `Error` values instead of panicking.
- Sparse, integer-only `PiecewiseLinearTransfer` support for physical
  ADC-to-measurement conversion with signed outputs, explicit below/above
  policies, and no extrapolation.
- Host-side adaptive transfer generation from physical points, formulas, and
  an NTC Beta-divider model, with exhaustive discrete-domain error reporting
  and bounded knot counts.
- Fixed-memory integer moving-average, median, and exponential filters plus a
  separate range-based stability detector for caller-supplied sample series.

### Fixed

- The 0.1.2 curve-name identifier rejection now also covers transfer names and
  the generated companions (`_FWD`, `_INV`, `_INPUTS`, `_OUTPUTS`,
  `_METADATA`), so a curve and a transfer cannot claim the same symbol.
- The 0.1.2 Debug-escaping of generated `///` docs now also covers transfer
  names, provenance, and unit strings, so TOML text cannot break out of line
  comments.
- Formula parsing no longer evaluates at an arbitrary out-of-domain value
  before the generator evaluates the declared input domain.
- Signed transfer interpolation rounds the complete result at half-way ties,
  matching the documented ties-away-from-zero behavior.
- The generator emitted `achieved_max_inverse_code_error: 0` unconditionally
  rather than measuring it, so any table whose codes do not survive a
  convert-then-invert cycle shipped a false round-trip bound. It is now swept
  exhaustively over the input domain.
- `TransferMetadata` no longer carries a `flat_resolution` copy. Codegen always
  baked in `PreferLowInput`, so metadata contradicted the live policy for any
  caller using `with_flat_resolution`. Read `flat_resolution()` instead.
- Inverse conversion selected its boundary policy by physical side alone, so on
  a decreasing table — the NTC reference case — a table configured
  `below = Error, above = Clamp` clamped in the forward direction and errored
  in the inverse for the same out-of-range condition. `below` and `above` are
  declared against the observation domain and are now mapped onto the physical
  range through the table's direction, so both directions agree.
- A compressing calibration (`|scale| > |gain|`) could make
  `AffineCalibration::invert` report a spurious `BelowRange` / `AboveRange` for
  a value the same calibration had just produced: undoing the affine expands
  the value and could overshoot an inner endpoint by one quantum. Values that
  are still inside the calibrated forward image now clamp to that endpoint.
  **Anything `convert` produces is invertible** — verified across 316,500
  round trips spanning increasing, decreasing, and flat-run tables. Values
  genuinely outside the image still range-error.
- The host `std` link moved from the crate root to a module-local
  `extern crate std` in `src/gen`. At the crate root, `#[macro_use]` put
  `format!` and friends in scope crate-wide whenever a host feature was on, so
  an accidental allocation on the runtime path would have compiled. It is now
  a compile error, which is what the no-alloc guarantee always claimed.

### Changed

- `AffineCalibration::new` rejects `gain == 0` with
  `AffineCalibrationError::ZeroGain`. A zero gain collapses every observation
  onto `offset / scale`, discarding the sensor and leaving the calibration
  non-invertible. Not a baseline change — `AffineCalibration` ships new in
  0.2.0.
- The crate's three near-duplicate nearest/ties-away division helpers are now
  one shared implementation, so a quantized value cannot drift depending on
  which module produced it.
- Host code generation is split into two features. `gen-lib` is the `build.rs`
  library API (serde + toml, no clap); `gen-cli` adds the `ph-curves-gen`
  binary. **`gen` is unchanged from 0.1.x** — it is now an alias for `gen-cli`
  and still builds the binary, so existing `--features gen` invocations keep
  working. *No migration is required.* Build scripts should prefer `gen-lib`,
  which skips the clap dependency.
- Exhausting the greedy transfer fitter's knot budget now reports the
  heuristic limitation without claiming that no alternative knot placement
  could satisfy the requested error.

### Notes

- **No breaking changes against 0.1.2.** Every addition above is additive, and
  the two changes that would have broken the baseline — retiring `gen` as the
  CLI feature, and relaxing `#![no_std]` for host builds — were both reworked
  so the 0.1.x contract holds. A 0.1.2 dependency declaration and a 0.1.2
  `cargo run --features gen` invocation both keep working unchanged.
- **The runtime is `no_std` and `no_alloc`, unconditionally.** `#![no_std]` is
  not feature-gated, so Cargo's feature unification cannot turn a firmware
  build into a `std` build when an unrelated crate enables a host feature. CI
  proves it by building the default feature set against a `core`-only sysroot
  (`-Z build-std=core`) on thumbv7em, thumbv6m, and riscv32imc: reaching for
  `alloc` or `std` on the runtime path fails the build.
- Remote GitHub Actions are restored at `.github/workflows/ci.yml`, covering
  format, the runtime-purity gate, clippy and tests across `gen-lib` /
  `gen-cli` / `gen`, a 0.1.x feature-compatibility check, the no-std and Xtensa
  target matrices, docs, and packaging.

## [0.1.2] - 2026-08-09

### Fixed

- Panic in `<u8 as UnitValue>::from_time_frac` and `to_time_offset` for any
  `duration_ms` above 65,535 (~65 seconds). The `U16F16` intermediate holds
  only 16 integer bits, so converting a larger millisecond count aborted with
  `"… overflows"`. Both now use `u64` integer arithmetic and accept the full
  `u32` range.
- The same panic in `<u16 as UnitValue>::from_time_frac` and `to_time_offset`
  for any `duration_ms` above 2,147,483,647 (~24.9 days), where the `I32F32`
  intermediate overflowed its 32 integer bits.
- Sign error in the code generator's piecewise-linear interpolation: control
  point outputs above 32,767 were reinterpreted as negative `i16`, producing
  wrong `u16` curves. Segment endpoints are now exact in both directions.
- The generator accepted `--lut-size` values smaller than the value type's
  domain, emitting LUTs that panicked at runtime on out-of-range indices
  (`CurveLut::eval` indexes by value). See *Changed* below.

### Changed

- **Breaking (CLI):** `--lut-size` must now equal the full domain of
  `--value-type`: 256 for `u8`, 65536 for `u16`. Smaller values were already
  broken at runtime, so any invocation that still succeeds produces identical
  output. *Migration:* if you passed a smaller `--lut-size`, switch to the full
  size and regenerate; if you relied on a partial LUT, those generated curves
  were panicking for any input at or above `lut_size`.
- **Breaking (CLI):** curve names containing no ASCII letter or digit, and
  names that normalize to an identifier already claimed by another curve
  (`ease-in` and `ease_in` both yield `EASE_IN`), are now rejected with an
  explanatory error instead of emitting a file that fails to compile — or one
  curve's table silently shadowing another's. *Migration:* rename the reported
  curve. Names beginning with a digit are now prefixed with `CURVE_` rather
  than producing an invalid identifier.
- Time conversions now use exact integer arithmetic instead of fixed-point.
  Where the old code did not panic, results are unchanged or differ by exactly
  1 LSB, always toward the mathematically correct value — measured at ~1% of
  `from_time_frac` inputs (1/65535 of range) and ~0.01% of `to_time_offset`
  inputs (1 ms). `to_time_offset` still rounds up as documented and still never
  exceeds `duration_ms`, so schedules cannot overshoot a segment.
- `to_time_offset` splits the duration to stay in 32-bit arithmetic instead of
  dividing in 64 bits, avoiding a software 64-bit divide on cores without a
  hardware divider. Measured on Cortex-M0 under emulation (instructions
  retired, mean over a realistic ramp workload) against 0.1.1: `u8` 193 → 129,
  `u16` 495 → 160. `from_time_frac` is also cheaper than 0.1.1 (`u8` 320 → 206,
  `u16` 2782 → 258), because the fixed-point path it replaced already went
  through a 64-bit divide.
- Generated doc comments quote the curve name with `Debug` escaping, so a name
  containing newlines or quotes can no longer break the emitted source.

## [0.1.1] - 2026-02-12

### Fixed

- `quantize` overflow when rounding near `u16::MAX` with large step sizes
  (Ceil and Nearest could wrap around, causing tickless ramps to be skipped).

### Changed

- Stopped excluding `assets/` from the published crate.

## [0.1.0] - 2026-02-12

### Added

- `CurveLut` and `MonotonicCurveLut` types with const constructors.
- `Curve` and `MonotonicCurve` traits for forward and inverse evaluation.
- `Tickless` extension trait with `TicklessSchedule` and `TicklessIter`.
- `RepeatMode::Once`, `Repeat`, and `PingPong` for tickless scheduling.
- `UnitValue` trait implemented for `u8` and `u16`.
- Math helpers: `lerp_u8`, `lerp_u16`, `map_u8_to_u16`, `quantize`,
  `next_target_value`.
- Code-gen CLI (`ph-curves-gen`) with `builtin`, `formula`, and `points`
  curve definitions.
- 14 built-in easing curves plus legacy aliases.
- 16-bit LUT support (`--value-type u16 --lut-size 65536`).

[Unreleased]: https://github.com/photon-circus/ph-curves/compare/v0.2.1...HEAD
[0.2.1]: https://github.com/photon-circus/ph-curves/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/photon-circus/ph-curves/compare/v0.1.2...v0.2.0
[0.1.2]: https://github.com/photon-circus/ph-curves/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/photon-circus/ph-curves/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/photon-circus/ph-curves/releases/tag/v0.1.0
