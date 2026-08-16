//! Structured generation reports and family aggregate resource budgets.
//!
//! Host callers that only need Rust source keep using the `String`-returning
//! helpers. Those helpers run the same pipeline as the report API, so a family
//! aggregate budget still fails closed when the caller discards the report.

// Host-only: module-local std link (crate root stays `#![no_std]`).
extern crate std;

use std::collections::BTreeMap;
use std::format;
use std::prelude::v1::*;

use crate::ObservationGuardMetadata;

use super::transfer::{SelectorValue, TransferFamilyDef};

/// Bytes of `_INPUTS` plus `_OUTPUTS` array payload per knot (`u16` + `i32`).
///
/// Structural runtime overhead — the `PiecewiseLinearTransfer` fields,
/// `_METADATA`, `_OBSERVATION_GUARD`, and symbol/section size — is excluded.
pub const TABLE_BYTES_PER_KNOT: usize = 6;

/// Generated Rust source together with the host audit report that produced it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GenerationResult {
    /// Complete generated Rust source. Identical to the `String` helpers.
    pub source: String,
    /// Per-transfer metrics plus family and document totals.
    pub report: GenerationReport,
}

/// Audit of every emitted transfer in one generation.
///
/// Transfers are ordered by table name, matching emit order. Families are
/// ordered by family name. Document [`Self::totals`] count every emitted
/// transfer, including standalones; curve LUT bytes are excluded. Duplicate
/// tables are not coalesced: identical payload still counts once per member.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GenerationReport {
    /// One entry per emitted `PiecewiseLinearTransfer`, sorted by table name.
    pub transfers: Vec<TransferReport>,
    /// One entry per family that emitted at least one member, sorted by name.
    pub families: Vec<FamilyReport>,
    /// Totals across every emitted transfer in the document.
    pub totals: ResourceTotals,
}

/// Knot and array-payload totals for a family or a whole document.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ResourceTotals {
    /// Number of emitted transfers included in these totals.
    pub member_count: usize,
    /// Sum of knot counts.
    pub knot_count: usize,
    /// Sum of `_INPUTS` + `_OUTPUTS` array payload bytes.
    pub table_bytes: usize,
}

/// Aggregate metrics for one family's emitted members.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FamilyReport {
    /// Family table name from the definitions document.
    pub name: String,
    /// Number of `status = "emit"` members included in [`Self::totals`].
    pub member_count: usize,
    /// Knot and payload totals for those emitted members.
    pub totals: ResourceTotals,
}

/// Host audit of one emitted transfer.
///
/// Domain, range, requested/achieved error, worst-case input, and knot count
/// agree with the generated `TransferMetadata` constant. `table_bytes` is
/// [`TABLE_BYTES_PER_KNOT`] times `knot_count` and is the figure compared to
/// `max_table_bytes`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransferReport {
    /// Family table name when this transfer was expanded from a family member.
    pub family: Option<String>,
    /// Selector identity of a family member. Empty for standalone transfers.
    pub selectors: BTreeMap<String, SelectorValue>,
    /// Definitions-table name used for codegen (expanded name for members).
    pub table_name: String,
    /// Emitted Rust constant name (`to_const_name` of [`Self::table_name`]).
    pub symbol: String,
    /// Inclusive observation-domain minimum (first knot input).
    pub domain_min: u16,
    /// Inclusive observation-domain maximum (last knot input).
    pub domain_max: u16,
    /// Inclusive physical-range minimum (min of the knot outputs).
    pub range_min: i32,
    /// Inclusive physical-range maximum (max of the knot outputs).
    pub range_max: i32,
    /// Requested interpolation error bound in output quanta.
    pub requested_max_error: u32,
    /// Conservative achieved interpolation error bound written to metadata.
    pub achieved_max_error: u32,
    /// Observation input at which the achieved error was measured.
    pub worst_case_input: u16,
    /// Number of knots in the emitted table.
    pub knot_count: usize,
    /// `_INPUTS` + `_OUTPUTS` array payload bytes for this table.
    pub table_bytes: usize,
    /// Source actually used to fit or verify this table.
    pub generation_path: GenerationPath,
    /// Observation-code guard copied onto the emitted transfer, when present.
    pub observation_guard: Option<ObservationGuardMetadata>,
}

/// How a transfer was produced. Overlays replace the declared TOML source.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GenerationPath {
    /// Declared formula over the observation-domain variable `x`.
    Formula,
    /// Sparse physical control points (TOML `points` or a points overlay).
    PhysicalPoints,
    /// Built-in NTC Beta-divider model.
    NtcBetaDivider,
    /// Built-in scaled-polynomial model.
    ScaledPolynomial,
    /// Host-evaluated dense physical truth overlay.
    EvaluatedTruth,
    /// Caller-chosen knots verified against dense evaluated truth.
    PrefittedKnots,
}

impl ResourceTotals {
    fn include(&mut self, transfer: &TransferReport) {
        self.member_count += 1;
        self.knot_count += transfer.knot_count;
        self.table_bytes += transfer.table_bytes;
    }
}

pub(crate) fn assemble_report(transfers: Vec<TransferReport>) -> GenerationReport {
    let mut family_members: BTreeMap<String, ResourceTotals> = BTreeMap::new();
    let mut totals = ResourceTotals::default();
    for transfer in &transfers {
        totals.include(transfer);
        if let Some(family) = &transfer.family {
            family_members
                .entry(family.clone())
                .or_default()
                .include(transfer);
        }
    }
    let families = family_members
        .into_iter()
        .map(|(name, family_totals)| FamilyReport {
            name,
            member_count: family_totals.member_count,
            totals: family_totals,
        })
        .collect();
    GenerationReport {
        transfers,
        families,
        totals,
    }
}

pub(crate) fn enforce_family_budgets(
    families: &BTreeMap<String, TransferFamilyDef>,
    report: &GenerationReport,
) -> Result<(), String> {
    for (name, family) in families {
        let totals = report
            .families
            .iter()
            .find(|entry| entry.name == *name)
            .map(|entry| entry.totals)
            .unwrap_or_default();
        if let Some(limit) = family.max_total_knots {
            if totals.knot_count > limit {
                return Err(format!(
                    "transfer family `{name}`: max_total_knots={limit} exceeded: {} knots across {} emitted members",
                    totals.knot_count, totals.member_count
                ));
            }
        }
        if let Some(limit) = family.max_table_bytes {
            if totals.table_bytes > limit {
                return Err(format!(
                    "transfer family `{name}`: max_table_bytes={limit} exceeded: {} bytes array payload across {} emitted members",
                    totals.table_bytes, totals.member_count
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ObservationGuardBehavior;
    use crate::r#gen::{
        DefinitionsFile, EvaluatedTruth, GenerateOptions, TransferSource, TransferSpec, generate,
        generate_from_str, generate_from_str_report, generate_report,
    };
    use std::format;
    use std::vec;

    fn transfers_only() -> GenerateOptions {
        GenerateOptions::transfers_only()
    }

    fn multi_member_toml() -> &'static str {
        r#"
[transfer_families.front_end]
input_unit = "count"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
max_knots = 8
formula = "x"

[[transfer_families.front_end.members]]
selectors = { range = "low" }
status = "emit"
applicability = { observation = [0, 4] }

[[transfer_families.front_end.members]]
selectors = { range = "high" }
status = "emit"
applicability = { observation = [10, 20] }

[[transfer_families.front_end.members]]
selectors = { range = "idle" }
status = "unnecessary"
reason = "documented spare range; not generated"
applicability = { observation = [0, 4] }

[transfers.standalone]
input_unit = "count"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
max_knots = 8
formula = "x"
domain = [0, 2]
"#
    }

    fn report_from_toml(toml: &str) -> GenerationReport {
        generate_from_str_report(toml, &transfers_only())
            .unwrap()
            .report
    }

    #[test]
    fn multi_member_fixture_reports_exact_family_and_document_totals() {
        let report = report_from_toml(multi_member_toml());

        assert_eq!(report.transfers.len(), 3);
        assert_eq!(report.transfers[0].table_name, "front_end_range_high");
        assert_eq!(report.transfers[0].symbol, "FRONT_END_RANGE_HIGH");
        assert_eq!(report.transfers[0].family.as_deref(), Some("front_end"));
        assert_eq!(
            report.transfers[0].selectors[&"range".to_string()],
            crate::r#gen::SelectorValue::String("high".into())
        );
        assert_eq!(report.transfers[0].domain_min, 10);
        assert_eq!(report.transfers[0].domain_max, 20);
        assert_eq!(report.transfers[0].range_min, 10);
        assert_eq!(report.transfers[0].range_max, 20);
        assert_eq!(report.transfers[0].requested_max_error, 1);
        assert_eq!(report.transfers[0].achieved_max_error, 0);
        assert_eq!(report.transfers[0].knot_count, 2);
        assert_eq!(report.transfers[0].table_bytes, 12);
        assert_eq!(report.transfers[0].generation_path, GenerationPath::Formula);
        assert_eq!(report.transfers[0].observation_guard, None);

        assert_eq!(report.transfers[1].table_name, "front_end_range_low");
        assert_eq!(report.transfers[1].domain_min, 0);
        assert_eq!(report.transfers[1].domain_max, 4);
        assert_eq!(report.transfers[1].knot_count, 2);
        assert_eq!(report.transfers[1].table_bytes, 12);

        assert_eq!(report.transfers[2].table_name, "standalone");
        assert_eq!(report.transfers[2].family, None);
        assert!(report.transfers[2].selectors.is_empty());
        assert_eq!(report.transfers[2].domain_min, 0);
        assert_eq!(report.transfers[2].domain_max, 2);
        assert_eq!(report.transfers[2].knot_count, 2);
        assert_eq!(report.transfers[2].table_bytes, 12);

        assert_eq!(report.families.len(), 1);
        assert_eq!(report.families[0].name, "front_end");
        assert_eq!(report.families[0].member_count, 2);
        assert_eq!(report.families[0].totals.member_count, 2);
        assert_eq!(report.families[0].totals.knot_count, 4);
        assert_eq!(report.families[0].totals.table_bytes, 24);

        assert_eq!(report.totals.member_count, 3);
        assert_eq!(report.totals.knot_count, 6);
        assert_eq!(report.totals.table_bytes, 36);
    }

    #[test]
    fn non_emit_members_are_absent_from_totals() {
        let report = report_from_toml(multi_member_toml());
        assert!(
            report
                .transfers
                .iter()
                .all(|transfer| !transfer.table_name.contains("idle"))
        );
        assert_eq!(report.families[0].member_count, 2);
    }

    fn budget_toml(extra: &str) -> String {
        format!(
            r#"
[transfer_families.front_end]
input_unit = "count"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
max_knots = 8
formula = "x"
{extra}

[[transfer_families.front_end.members]]
selectors = {{ range = "a" }}
status = "emit"
applicability = {{ observation = [0, 4] }}

[[transfer_families.front_end.members]]
selectors = {{ range = "b" }}
status = "emit"
applicability = {{ observation = [0, 4] }}

[[transfer_families.front_end.members]]
selectors = {{ range = "c" }}
status = "emit"
applicability = {{ observation = [0, 4] }}
"#
        )
    }

    #[test]
    fn family_total_knots_budget_fails_after_per_member_success() {
        let toml = budget_toml("max_total_knots = 5");
        let error = generate_from_str(&toml, &transfers_only())
            .unwrap_err()
            .to_string();
        assert!(
            error.contains(
                "transfer family `front_end`: max_total_knots=5 exceeded: 6 knots across 3 emitted members"
            ),
            "{error}"
        );
        let report_error = generate_from_str_report(&toml, &transfers_only())
            .unwrap_err()
            .to_string();
        assert_eq!(error, report_error);
    }

    #[test]
    fn family_total_bytes_budget_fails_closed() {
        let toml = budget_toml("max_table_bytes = 24");
        let error = generate_from_str(&toml, &transfers_only())
            .unwrap_err()
            .to_string();
        assert!(
            error.contains(
                "transfer family `front_end`: max_table_bytes=24 exceeded: 36 bytes array payload across 3 emitted members"
            ),
            "{error}"
        );
    }

    #[test]
    fn knot_budget_is_diagnosed_before_byte_budget() {
        let toml = budget_toml("max_total_knots = 5\nmax_table_bytes = 24");
        let error = generate_from_str(&toml, &transfers_only())
            .unwrap_err()
            .to_string();
        assert!(error.contains("max_total_knots=5"), "{error}");
        assert!(!error.contains("max_table_bytes"), "{error}");
    }

    #[test]
    fn duplicate_tables_count_payload_twice() {
        let toml = r#"
[transfer_families.front_end]
input_unit = "count"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
max_knots = 8
formula = "x"

[[transfer_families.front_end.members]]
selectors = { range = "a" }
status = "emit"
applicability = { observation = [0, 4] }

[[transfer_families.front_end.members]]
selectors = { range = "b" }
status = "emit"
applicability = { observation = [0, 4] }
"#;
        let report = report_from_toml(toml);
        assert_eq!(
            report.transfers[0].knot_count,
            report.transfers[1].knot_count
        );
        assert_eq!(
            report.transfers[0].table_bytes,
            report.transfers[1].table_bytes
        );
        assert_eq!(
            report.families[0].totals.table_bytes,
            report.transfers[0].table_bytes * 2
        );
        assert_eq!(
            report.families[0].totals.knot_count,
            report.transfers[0].knot_count * 2
        );
    }

    #[test]
    fn toml_points_path_is_physical_points() {
        let toml = r#"
[transfers.sensor]
input_unit = "count"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
max_knots = 8
points = [
  { input = 0, output = 0.0 },
  { input = 10, output = 10.0 },
]
"#;
        let report = report_from_toml(toml);
        assert_eq!(
            report.transfers[0].generation_path,
            GenerationPath::PhysicalPoints
        );
        assert_eq!(report.transfers[0].knot_count, 2);
        assert_eq!(report.transfers[0].table_bytes, TABLE_BYTES_PER_KNOT * 2);
    }

    #[test]
    fn programmatic_evaluated_truth_sets_generation_path() {
        let mut defs = DefinitionsFile::default();
        defs.insert_transfer(
            TransferSpec::new(
                "linear",
                "code",
                "unit",
                1,
                1,
                TransferSource::evaluated_truth(0, vec![0.0, 1.0, 2.0, 3.0]),
            )
            .with_max_knots(8),
        )
        .unwrap();
        let report = generate_report(&defs, &transfers_only()).unwrap().report;
        assert_eq!(report.transfers[0].table_name, "linear");
        assert_eq!(report.transfers[0].symbol, "LINEAR");
        assert_eq!(
            report.transfers[0].generation_path,
            GenerationPath::EvaluatedTruth
        );
        assert_eq!(report.transfers[0].family, None);
    }

    #[test]
    fn prefitted_overlay_sets_generation_path() {
        let mut defs = DefinitionsFile::default();
        let truth = EvaluatedTruth::new(0, vec![0.0, 1.0, 2.0]);
        defs.insert_transfer(
            TransferSpec::new(
                "linear",
                "code",
                "unit",
                1,
                1,
                TransferSource::prefitted_knots_verified(vec![0, 2], vec![0, 2], truth),
            )
            .with_max_knots(8),
        )
        .unwrap();
        let report = generate_report(&defs, &transfers_only()).unwrap().report;
        assert_eq!(
            report.transfers[0].generation_path,
            GenerationPath::PrefittedKnots
        );
        assert_eq!(report.transfers[0].knot_count, 2);
    }

    #[test]
    fn evaluated_truth_overlay_replaces_declared_formula_path() {
        let defs = DefinitionsFile::from_toml_str(multi_member_toml()).unwrap();
        let mut validated = defs.validate().unwrap();
        validated
            .set_source(
                "front_end_range_low",
                TransferSource::evaluated_truth(0, vec![0.0, 1.0, 2.0, 3.0, 4.0]),
            )
            .unwrap();
        let report = validated.generate_report(&transfers_only()).unwrap().report;
        let low = report
            .transfers
            .iter()
            .find(|transfer| transfer.table_name == "front_end_range_low")
            .unwrap();
        assert_eq!(low.generation_path, GenerationPath::EvaluatedTruth);
        let high = report
            .transfers
            .iter()
            .find(|transfer| transfer.table_name == "front_end_range_high")
            .unwrap();
        assert_eq!(high.generation_path, GenerationPath::Formula);
    }

    #[test]
    fn scaled_polynomial_and_ntc_paths_are_classified() {
        let scaled = r#"
[transfers.poly]
input_unit = "count"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
max_knots = 8
domain = [0, 4]

[transfers.poly.model]
kind = "scaled_polynomial"
coefficients = [0.0, 1.0]
scale = 1_000_000
"#;
        let report = report_from_toml(scaled);
        assert_eq!(
            report.transfers[0].generation_path,
            GenerationPath::ScaledPolynomial
        );

        let ntc = r#"
[transfers.ntc]
input_unit = "adc_code"
output_unit = "degree_celsius"
output_scale = 1000
max_interpolation_error = 50
max_knots = 64
output_range = [0.0, 40.0]

[transfers.ntc.model]
kind = "ntc_beta_divider"
nominal_resistance_ohms = 10000.0
beta_kelvin = 3950.0
nominal_temperature_celsius = 25.0
fixed_resistance_ohms = 10000.0
adc_max_code = 4095
topology = "ntc_to_ground"
"#;
        let report = report_from_toml(ntc);
        assert_eq!(
            report.transfers[0].generation_path,
            GenerationPath::NtcBetaDivider
        );
    }

    #[test]
    fn observation_guard_is_copied_into_the_report() {
        let toml = r#"
[transfers]
requires = ["observation_guard_v1"]

[transfers.guarded]
input_unit = "count"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
max_knots = 8
formula = "x"
domain = [1, 10]
saturation = { code = 65535, behavior = "error" }
"#;
        let report = report_from_toml(toml);
        assert_eq!(
            report.transfers[0].observation_guard,
            Some(ObservationGuardMetadata {
                code: 65_535,
                behavior: ObservationGuardBehavior::Error,
            })
        );
    }

    #[test]
    fn zero_aggregate_budgets_fail_during_validation() {
        for field in ["max_total_knots = 0", "max_table_bytes = 0"] {
            let toml = budget_toml(field);
            let error = DefinitionsFile::from_toml_str(&toml)
                .unwrap()
                .validate()
                .unwrap_err()
                .to_string();
            assert!(error.contains("must be positive"), "{field}: {error}");
        }
    }

    #[test]
    fn string_generate_helpers_still_return_source() {
        let source = generate_from_str(multi_member_toml(), &transfers_only()).unwrap();
        assert!(source.contains("pub const FRONT_END_RANGE_LOW"));
        let defs = DefinitionsFile::from_toml_str(multi_member_toml()).unwrap();
        assert_eq!(generate(&defs, &transfers_only()).unwrap(), source);
    }
}
