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

use super::transfer::family::{
    resolved_family_gap_provenance, resolved_member_provenance, resolved_selector_universe,
};
use super::transfer::{
    FamilyCompleteness, GapDef, GapStatus, GenerationPolicy, MemberStatus, SelectorUniverse,
    SelectorValue, SourceProvenance, SourceProvenanceOverride, TransferFamilyDef,
    resolve_guard_provenance,
};

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
/// Transfers are ordered by table name, matching emit order. Families and
/// document gaps are ordered by name. Members and family-scoped gaps retain
/// declaration order. Document [`Self::totals`] count every emitted transfer,
/// including standalones; curve LUT bytes are excluded. Duplicate tables are
/// not coalesced: identical payload still counts once per member.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GenerationReport {
    /// One entry per emitted `PiecewiseLinearTransfer`, sorted by table name.
    pub transfers: Vec<TransferReport>,
    /// One entry per family, sorted by name.
    pub families: Vec<FamilyReport>,
    /// Declared document-level gaps, sorted by name.
    pub gaps: Vec<GapReport>,
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

/// Audit of one family, including description-only members and scoped gaps.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FamilyReport {
    /// Family table name from the definitions document.
    pub name: String,
    /// Shared family citation before member-level overrides.
    pub provenance: SourceProvenance,
    /// Citation supporting the family observation-guard classification.
    ///
    /// This is resolved before member and generation-source overlays, so it
    /// cannot be silently re-parented to a different source citation.
    pub observation_guard_provenance: Option<SourceProvenance>,
    /// Citation-free fit, boundary, and observation-guard policy.
    pub policy: GenerationPolicy,
    /// Declared selector universe. Cartesian products remain compact and are
    /// not materialized for the report.
    pub selector_universe: SelectorUniverse,
    /// Validated selector-occupancy result.
    pub completeness: FamilyCompleteness,
    /// Every declared member, including non-emitting members, in declaration
    /// order.
    pub members: Vec<FamilyMemberReport>,
    /// Family-scoped gaps in declaration order.
    pub gaps: Vec<FamilyGapReport>,
    /// Number of `status = "emit"` members included in [`Self::totals`].
    pub member_count: usize,
    /// Optional aggregate knot budget declared by the family.
    pub max_total_knots: Option<usize>,
    /// Optional aggregate table-payload budget declared by the family.
    pub max_table_bytes: Option<usize>,
    /// Knot and payload totals for those emitted members.
    pub totals: ResourceTotals,
}

/// One declared family member in a generation audit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FamilyMemberReport {
    /// Typed selector identity.
    pub selectors: BTreeMap<String, SelectorValue>,
    /// Whether the member emits or remains description-only.
    pub status: MemberStatus,
    /// Required rationale for a non-emitting status.
    pub reason: Option<String>,
    /// Effective citation after the declared member override and any
    /// generation-source overlay.
    pub provenance: SourceProvenance,
    /// Member-level citation override exactly as declared.
    pub provenance_override: Option<SourceProvenanceOverride>,
    /// Generated table name for an emitted member; absent otherwise.
    pub table_name: Option<String>,
    /// Generated Rust constant name for an emitted member; absent otherwise.
    pub symbol: Option<String>,
    /// Companion `{symbol}_METADATA` name for an emitted member; absent otherwise.
    pub metadata_symbol: Option<String>,
    /// Companion `{symbol}_OBSERVATION_GUARD` name for an emitted member; absent otherwise.
    pub observation_guard_symbol: Option<String>,
}

/// One family-scoped selector gap in a generation audit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FamilyGapReport {
    /// Typed selector identity occupied by this gap.
    pub selectors: BTreeMap<String, SelectorValue>,
    /// Always [`GapStatus::Undefined`].
    pub status: GapStatus,
    /// Explanation of why this selector identity has no member.
    pub reason: String,
    /// Effective citation after resolving the gap override against the family
    /// citation.
    pub provenance: SourceProvenance,
    /// Gap-level citation override exactly as declared.
    pub provenance_override: Option<SourceProvenanceOverride>,
}

/// One named document-level gap in a generation audit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GapReport {
    /// Gap table name.
    pub name: String,
    /// Always [`GapStatus::Undefined`].
    pub status: GapStatus,
    /// Explanation of why this mapping is undefined.
    pub reason: String,
    /// Effective standalone citation, when one was declared.
    pub provenance: Option<SourceProvenance>,
    /// Citation override exactly as declared.
    pub provenance_override: Option<SourceProvenanceOverride>,
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
    /// Companion metadata constant name (`{symbol}_METADATA`).
    pub metadata_symbol: String,
    /// Companion observation-guard constant name (`{symbol}_OBSERVATION_GUARD`).
    pub observation_guard_symbol: String,
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
    /// Effective source citation after any generation-source overlay.
    pub provenance: Option<SourceProvenance>,
    /// Citation supporting the observation-guard classification.
    ///
    /// This is resolved before source overlays, independently of
    /// [`Self::provenance`].
    pub observation_guard_provenance: Option<SourceProvenance>,
    /// Citation-free fit, boundary, and observation-guard policy.
    pub policy: GenerationPolicy,
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

pub(crate) fn assemble_report(
    mut transfers: Vec<TransferReport>,
    family_defs: &BTreeMap<String, TransferFamilyDef>,
    gap_defs: &BTreeMap<String, GapDef>,
) -> Result<GenerationReport, String> {
    transfers.sort_by(|left, right| left.table_name.cmp(&right.table_name));
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
    let mut emitted_by_origin = BTreeMap::new();
    for transfer in &transfers {
        let Some(family) = &transfer.family else {
            continue;
        };
        let key = (family.clone(), transfer.selectors.clone());
        if emitted_by_origin.insert(key, transfer).is_some() {
            return Err(format!(
                "transfer family `{family}`: duplicate emitted report origin for selectors {:?}",
                transfer.selectors
            ));
        }
    }
    let mut families = Vec::with_capacity(family_defs.len());
    for (name, family) in family_defs {
        let family_totals = family_members.get(name).copied().unwrap_or_default();
        let mut members = Vec::with_capacity(family.members.len());
        for member in &family.members {
            let emitted = emitted_by_origin.remove(&(name.clone(), member.selectors.clone()));
            match (member.status, emitted) {
                (MemberStatus::Emit, None) => {
                    return Err(format!(
                        "transfer family `{name}`: emitted member with selectors {:?} is missing from the generation report",
                        member.selectors
                    ));
                }
                (MemberStatus::Emit, Some(_)) | (_, None) => {}
                (_, Some(_)) => {
                    return Err(format!(
                        "transfer family `{name}`: description-only member with selectors {:?} unexpectedly emitted",
                        member.selectors
                    ));
                }
            }
            let declared_provenance = resolved_member_provenance(name, family, member)?;
            let provenance = match emitted {
                Some(transfer) => transfer.provenance.clone().ok_or_else(|| {
                    format!(
                        "transfer family `{name}`: emitted member with selectors {:?} has no effective source provenance",
                        member.selectors
                    )
                })?,
                None => declared_provenance,
            };
            members.push(FamilyMemberReport {
                selectors: member.selectors.clone(),
                status: member.status,
                reason: member.reason.clone(),
                provenance,
                provenance_override: member.provenance.clone(),
                table_name: emitted.map(|transfer| transfer.table_name.clone()),
                symbol: emitted.map(|transfer| transfer.symbol.clone()),
                metadata_symbol: emitted.map(|transfer| transfer.metadata_symbol.clone()),
                observation_guard_symbol: emitted
                    .map(|transfer| transfer.observation_guard_symbol.clone()),
            });
        }

        let mut gaps = Vec::with_capacity(family.gaps.len());
        for (index, gap) in family.gaps.iter().enumerate() {
            gaps.push(FamilyGapReport {
                selectors: gap.selectors.clone(),
                status: gap.status,
                reason: gap.reason.clone(),
                provenance: resolved_family_gap_provenance(name, family, index, gap)?,
                provenance_override: gap.provenance.clone(),
            });
        }

        let provenance = family.provenance.clone().ok_or_else(|| {
            format!("transfer family `{name}`: source-backed family requires provenance.identity")
        })?;
        families.push(FamilyReport {
            name: name.clone(),
            provenance,
            observation_guard_provenance: resolve_guard_provenance(
                &format!("transfer family `{name}`"),
                family.observation_guard(),
                family.provenance(),
            )?,
            policy: family.policy(),
            selector_universe: resolved_selector_universe(name, family)?,
            completeness: FamilyCompleteness::Complete,
            members,
            gaps,
            member_count: family_totals.member_count,
            max_total_knots: family.max_total_knots,
            max_table_bytes: family.max_table_bytes,
            totals: family_totals,
        });
    }
    if let Some(((family, selectors), _)) = emitted_by_origin.into_iter().next() {
        return Err(format!(
            "transfer family `{family}`: emitted report origin with selectors {selectors:?} does not match a declared member"
        ));
    }

    let gaps = gap_defs
        .iter()
        .map(|(name, gap)| {
            Ok(GapReport {
                name: name.clone(),
                status: gap.status,
                reason: gap.reason.clone(),
                provenance: gap.resolved_provenance()?,
                provenance_override: gap.provenance.clone(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;

    Ok(GenerationReport {
        transfers,
        families,
        gaps,
        totals,
    })
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
        if let Some(limit) = family.max_total_knots
            && totals.knot_count > limit
        {
            return Err(format!(
                "transfer family `{name}`: max_total_knots={limit} exceeded: {} knots across {} emitted members",
                totals.knot_count, totals.member_count
            ));
        }
        if let Some(limit) = family.max_table_bytes
            && totals.table_bytes > limit
        {
            return Err(format!(
                "transfer family `{name}`: max_table_bytes={limit} exceeded: {} bytes array payload across {} emitted members",
                totals.table_bytes, totals.member_count
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ObservationGuardBehavior;
    use crate::r#gen::{
        DefinitionsFile, EvaluatedTruth, GenerateOptions, ObservationGuardBehaviorDef,
        SourceProvenance, TransferSource, TransferSpec, generate, generate_from_str,
        generate_from_str_report, generate_report,
    };
    use std::format;
    use std::vec;

    fn transfers_only() -> GenerateOptions {
        GenerateOptions::transfers_only()
    }

    fn multi_member_toml() -> &'static str {
        r#"
[transfer_families.front_end]
provenance = { identity = "report fixture" }
input_unit = "count"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
max_knots = 8
formula = "x"
selector_axes = { range = ["low", "high", "idle"] }

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
        assert_eq!(report.families[0].members.len(), 3);
        assert_eq!(
            report.families[0].members[2].status,
            MemberStatus::Unnecessary
        );
        assert_eq!(report.families[0].members[2].table_name, None);
        assert_eq!(report.families[0].members[2].symbol, None);
        assert_eq!(report.families[0].provenance.identity, "report fixture");
        assert_eq!(
            report.families[0].selector_universe.identity_count(),
            Some(3)
        );
        assert_eq!(
            report.families[0].completeness,
            FamilyCompleteness::Complete
        );
        assert_eq!(report.families[0].max_total_knots, None);
        assert_eq!(report.families[0].max_table_bytes, None);
        assert_eq!(report.families[0].totals.member_count, 2);
        assert_eq!(report.families[0].totals.knot_count, 4);
        assert_eq!(report.families[0].totals.table_bytes, 24);

        assert_eq!(report.totals.member_count, 3);
        assert_eq!(report.totals.knot_count, 6);
        assert_eq!(report.totals.table_bytes, 36);
        assert!(report.gaps.is_empty());
    }

    #[test]
    fn non_emit_members_are_reported_but_absent_from_totals() {
        let report = report_from_toml(multi_member_toml());
        assert!(
            report
                .transfers
                .iter()
                .all(|transfer| !transfer.table_name.contains("idle"))
        );
        assert_eq!(report.families[0].member_count, 2);
        let idle = report.families[0]
            .members
            .iter()
            .find(|member| {
                member.selectors.get("range") == Some(&SelectorValue::String("idle".into()))
            })
            .unwrap();
        assert_eq!(idle.status, MemberStatus::Unnecessary);
        assert_eq!(
            idle.reason.as_deref(),
            Some("documented spare range; not generated")
        );
        assert_eq!(idle.provenance.identity, "report fixture");
        assert!(idle.table_name.is_none());
        assert!(idle.symbol.is_none());
    }

    fn budget_toml(extra: &str) -> String {
        format!(
            r#"
[transfer_families.front_end]
provenance = {{ identity = "budget fixture" }}
input_unit = "count"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
max_knots = 8
formula = "x"
selector_axes = {{ range = ["a", "b", "c"] }}
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
provenance = { identity = "duplicate fixture" }
input_unit = "count"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
max_knots = 8
formula = "x"
selector_axes = { range = ["a", "b"] }

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
            .with_max_knots(8)
            .with_provenance(SourceProvenance::new("programmatic samples")),
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
        assert_eq!(
            report.transfers[0]
                .provenance
                .as_ref()
                .map(|source| source.identity.as_str()),
            Some("programmatic samples")
        );
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
                TransferSource::evaluated_truth(0, vec![0.0, 1.0, 2.0, 3.0, 4.0])
                    .inherit_provenance(),
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

    fn cited_guard_toml() -> &'static str {
        r#"
[transfers]
requires = ["observation_guard_v1", "source_provenance_v1"]

[transfers.standalone]
input_unit = "code"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
max_knots = 8
below = "clamp"
above = "error"
provenance = { identity = "declared source", revision = "A", locator = "transfer table" }
saturation = { code = 65535, behavior = "error", provenance = { locator = "guard table" } }
formula = "x"
domain = [1, 3]
"#
    }

    fn replacement_truth() -> TransferSource {
        TransferSource::evaluated_truth(1, vec![10.0, 20.0, 30.0])
    }

    #[test]
    fn report_tracks_overlay_inherit_replace_and_clear_without_reparenting_guard() {
        let mut validated = DefinitionsFile::from_toml_str(cited_guard_toml())
            .unwrap()
            .validate()
            .unwrap();

        let declared = validated
            .generate_report(&transfers_only())
            .unwrap()
            .report
            .transfers
            .remove(0);
        assert_eq!(
            declared
                .provenance
                .as_ref()
                .map(|source| source.identity.as_str()),
            Some("declared source")
        );
        assert_eq!(
            declared
                .observation_guard_provenance
                .as_ref()
                .and_then(|source| source.locator.as_deref()),
            Some("guard table")
        );

        validated
            .set_source("standalone", replacement_truth().inherit_provenance())
            .unwrap();
        let inherited = validated
            .generate_report(&transfers_only())
            .unwrap()
            .report
            .transfers
            .remove(0);
        assert_eq!(inherited.provenance, declared.provenance);

        validated
            .set_source(
                "standalone",
                replacement_truth().with_provenance(
                    SourceProvenance::new("overlay source").with_locator("evaluated samples"),
                ),
            )
            .unwrap();
        let replaced = validated
            .generate_report(&transfers_only())
            .unwrap()
            .report
            .transfers
            .remove(0);
        assert_eq!(
            replaced
                .provenance
                .as_ref()
                .map(|source| source.identity.as_str()),
            Some("overlay source")
        );
        assert_eq!(
            replaced.observation_guard_provenance,
            declared.observation_guard_provenance
        );
        assert_eq!(replaced.policy, declared.policy);

        validated
            .set_source("standalone", replacement_truth().clear_provenance())
            .unwrap();
        let cleared = validated
            .generate_report(&transfers_only())
            .unwrap()
            .report
            .transfers
            .remove(0);
        assert_eq!(cleared.provenance, None);
        assert_eq!(
            cleared.observation_guard_provenance,
            declared.observation_guard_provenance
        );
        assert_eq!(cleared.policy, declared.policy);
    }

    #[test]
    fn equal_guard_policy_does_not_hide_distinct_citations() {
        let toml = r#"
[transfers]
requires = ["observation_guard_v1", "source_provenance_v1"]

[transfers.first]
input_unit = "code"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
max_knots = 8
provenance = { identity = "first datasheet" }
saturation = { code = 65535, behavior = "error", provenance = { locator = "first guard row" } }
formula = "x"
domain = [1, 3]

[transfers.second]
input_unit = "code"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
max_knots = 8
provenance = { identity = "second datasheet" }
saturation = { code = 65535, behavior = "error", provenance = { locator = "second guard row" } }
formula = "x"
domain = [1, 3]
"#;
        let report = report_from_toml(toml);
        assert_eq!(report.transfers[0].policy, report.transfers[1].policy);
        assert_eq!(
            report.transfers[0].policy.observation_guard,
            Some(super::super::transfer::ObservationGuardPolicy {
                code: 65_535,
                behavior: ObservationGuardBehaviorDef::Error,
            })
        );
        assert_ne!(
            report.transfers[0].observation_guard_provenance,
            report.transfers[1].observation_guard_provenance
        );
    }

    #[test]
    fn family_report_preserves_members_gaps_provenance_and_emit_mapping() {
        let toml = r#"
[transfer_families.front_end]
provenance = { identity = "family source", revision = "A", locator = "family table" }
input_unit = "code"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
max_knots = 8
max_total_knots = 6
max_table_bytes = 36
below = "clamp"
above = "error"
saturation = { code = 65535, behavior = "clamp", provenance = { locator = "guard row" } }
formula = "x"
selector_axes = { mode = ["emit", "documented", "hole"] }

[[transfer_families.front_end.members]]
selectors = { mode = "emit" }
status = "emit"
applicability = { observation = [1, 3] }
provenance = { locator = "member row" }

[[transfer_families.front_end.members]]
selectors = { mode = "documented" }
status = "unnecessary"
reason = "documented but deliberately not generated"
applicability = { observation = [1, 3] }
provenance = { identity = "member-specific source", note = "description only" }

[[transfer_families.front_end.gaps]]
selectors = { mode = "hole" }
status = "undefined"
reason = "the source has no mapping"
provenance = { locator = "missing row" }

[gaps.zeta]
status = "undefined"
reason = "last named gap"
provenance = { identity = "gap catalog", locator = "Z" }

[gaps.alpha]
status = "undefined"
reason = "first named gap"
"#;
        let mut validated = DefinitionsFile::from_toml_str(toml)
            .unwrap()
            .validate()
            .unwrap();
        validated
            .set_source(
                "front_end_mode_emit",
                replacement_truth().with_provenance(
                    SourceProvenance::new("overlay measurements").with_revision("run 2"),
                ),
            )
            .unwrap();

        let report = validated.generate_report(&transfers_only()).unwrap().report;
        assert_eq!(report.families.len(), 1);
        let family = &report.families[0];
        assert_eq!(family.name, "front_end");
        assert_eq!(family.provenance.identity, "family source");
        assert_eq!(
            family
                .observation_guard_provenance
                .as_ref()
                .and_then(|source| source.locator.as_deref()),
            Some("guard row")
        );
        assert_eq!(
            family.policy.observation_guard,
            Some(super::super::transfer::ObservationGuardPolicy {
                code: 65_535,
                behavior: ObservationGuardBehaviorDef::Clamp,
            })
        );
        assert_eq!(family.selector_universe.identity_count(), Some(3));
        assert_eq!(family.completeness, FamilyCompleteness::Complete);
        assert_eq!(family.max_total_knots, Some(6));
        assert_eq!(family.max_table_bytes, Some(36));
        assert_eq!(family.member_count, 1);
        assert_eq!(family.totals.member_count, 1);

        assert_eq!(family.members.len(), 2);
        let emitted = &family.members[0];
        assert_eq!(emitted.status, MemberStatus::Emit);
        assert_eq!(emitted.provenance.identity, "overlay measurements");
        assert_eq!(
            emitted
                .provenance_override
                .as_ref()
                .and_then(|source| source.locator.as_deref()),
            Some("member row")
        );
        assert_eq!(emitted.table_name.as_deref(), Some("front_end_mode_emit"));
        assert_eq!(emitted.symbol.as_deref(), Some("FRONT_END_MODE_EMIT"));

        let documented = &family.members[1];
        assert_eq!(documented.status, MemberStatus::Unnecessary);
        assert_eq!(documented.provenance.identity, "member-specific source");
        assert_eq!(documented.provenance.revision, None);
        assert_eq!(documented.provenance.locator, None);
        assert_eq!(
            documented.provenance.note.as_deref(),
            Some("description only")
        );
        assert!(documented.table_name.is_none());
        assert!(documented.symbol.is_none());

        assert_eq!(family.gaps.len(), 1);
        assert_eq!(family.gaps[0].status, GapStatus::Undefined);
        assert_eq!(family.gaps[0].provenance.identity, "family source");
        assert_eq!(family.gaps[0].provenance.revision.as_deref(), Some("A"));
        assert_eq!(
            family.gaps[0].provenance.locator.as_deref(),
            Some("missing row")
        );
        assert!(family.gaps[0].provenance_override.is_some());

        assert_eq!(
            report
                .gaps
                .iter()
                .map(|gap| gap.name.as_str())
                .collect::<Vec<_>>(),
            ["alpha", "zeta"]
        );
        assert_eq!(report.gaps[0].provenance, None);
        assert_eq!(
            report.gaps[1]
                .provenance
                .as_ref()
                .map(|source| source.identity.as_str()),
            Some("gap catalog")
        );
        assert!(report.gaps[1].provenance_override.is_some());
    }

    #[test]
    fn gap_only_document_still_returns_a_complete_report() {
        let report = report_from_toml(
            r#"
[gaps.channel]
status = "undefined"
reason = "no physical mapping exists"
provenance = { identity = "interface specification", locator = "Table 9" }
"#,
        );
        assert!(report.transfers.is_empty());
        assert!(report.families.is_empty());
        assert_eq!(report.totals, ResourceTotals::default());
        assert_eq!(report.gaps.len(), 1);
        assert_eq!(report.gaps[0].name, "channel");
        assert_eq!(report.gaps[0].status, GapStatus::Undefined);
        assert_eq!(
            report.gaps[0]
                .provenance
                .as_ref()
                .and_then(|source| source.locator.as_deref()),
            Some("Table 9")
        );
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
