//! Validated host IR: inspect families and overlay generation sources.

// Host-only: module-local std link (crate root stays `#![no_std]`).
extern crate std;

use std::collections::{BTreeMap, BTreeSet};
use std::format;
use std::prelude::v1::*;

use super::api::{Error, GenerateOptions};
use super::curve::DefinitionsFile;
use super::report::GenerationResult;
use super::transfer::family::{
    expanded_name, member_emitted_name, member_observation_domain, resolved_family_gap_provenance,
    resolved_member_provenance, resolved_selector_universe,
};
use super::transfer::{
    ApplicabilityDef, DeclaredSource, FamilyCompleteness, FamilySpec, GapDef, GapStatus,
    GenerationPolicy, InputTransform, MemberStatus, PhysicalPoint, SelectorUniverse, SelectorValue,
    SourceProvenance, SourceProvenanceDisposition, SourceProvenanceOverride, TransferFamilyDef,
    TransferSourceOverlay, TransferSpec, family_source_observation_domain,
    overlay_observation_span, resolve_guard_provenance,
};

/// Family, member, and gap graph after identity and collision checks.
///
/// Validation does not fit knots or emit Rust. Overlay generation sources
/// with [`Self::set_source`], [`Self::insert_transfer`], or [`Self::insert_family`],
/// then [`Self::generate`].
#[derive(Clone, Debug)]
pub struct ValidatedDefinitions {
    defs: DefinitionsFile,
    families: Vec<ValidatedFamily>,
    resolved_names: BTreeSet<String>,
    family_overlay_domains: BTreeMap<String, [u16; 2]>,
}

/// One validated family, including description-only members and family-scoped gaps.
#[derive(Clone, Debug)]
pub struct ValidatedFamily {
    name: String,
    input_unit: String,
    output_unit: String,
    output_scale: u32,
    declared_source: DeclaredSource,
    formula: Option<String>,
    points: Option<Vec<PhysicalPoint>>,
    has_model: bool,
    max_total_knots: Option<usize>,
    max_table_bytes: Option<usize>,
    provenance: SourceProvenance,
    guard_provenance: Option<SourceProvenance>,
    policy: GenerationPolicy,
    selector_universe: SelectorUniverse,
    members: Vec<ValidatedMember>,
    gaps: Vec<ValidatedFamilyGap>,
    completeness: FamilyCompleteness,
}

impl ValidatedFamily {
    /// Family table name from the definitions document.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Observation-domain unit label copied onto emitted members.
    pub fn input_unit(&self) -> &str {
        &self.input_unit
    }

    /// Physical-domain unit label copied onto emitted members.
    pub fn output_unit(&self) -> &str {
        &self.output_unit
    }

    /// Integer output quanta per physical unit.
    pub fn output_scale(&self) -> u32 {
        self.output_scale
    }

    /// Shared source kind and therefore the member applicability coordinate space.
    ///
    /// Formula and points members use `applicability.observation`. Scaled
    /// polynomial members use `applicability.model_input` plus
    /// `input_transform`. NTC members use `applicability.physical`.
    pub fn declared_source(&self) -> DeclaredSource {
        self.declared_source
    }

    /// Shared formula text, when the family source is a formula.
    pub fn formula(&self) -> Option<&str> {
        self.formula.as_deref()
    }

    /// Shared physical control points, when the family source is points.
    pub fn points(&self) -> Option<&[PhysicalPoint]> {
        self.points.as_deref()
    }

    /// Whether the shared source is a built-in host model.
    ///
    /// Model parameters stay crate-private. Distinguish kinds through
    /// [`Self::declared_source`].
    pub fn has_model(&self) -> bool {
        self.has_model
    }

    /// Optional aggregate knot budget across every emitted member.
    pub fn max_total_knots(&self) -> Option<usize> {
        self.max_total_knots
    }

    /// Optional aggregate `_INPUTS` + `_OUTPUTS` array-payload budget.
    pub fn max_table_bytes(&self) -> Option<usize> {
        self.max_table_bytes
    }

    /// Shared source citation. Distinct from [`Self::policy`].
    pub fn provenance(&self) -> &SourceProvenance {
        &self.provenance
    }

    /// Resolved citation supporting the observation-guard classification.
    ///
    /// This is separate from [`Self::policy`], which contains only the guard's
    /// code and behavior.
    pub fn observation_guard_provenance(&self) -> Option<&SourceProvenance> {
        self.guard_provenance.as_ref()
    }

    /// Fit budget, boundaries, and observation-guard classification.
    pub fn policy(&self) -> &GenerationPolicy {
        &self.policy
    }

    /// Declared expected selector identities (Cartesian axes or an explicit set).
    ///
    /// [`SelectorUniverse::identity_count`] reports the checked cardinality;
    /// [`SelectorUniverse::identities`] enumerates lazily without materializing
    /// a Cartesian product.
    pub fn selector_universe(&self) -> &SelectorUniverse {
        &self.selector_universe
    }

    /// Every member, in declaration order. Non-`emit` members are present.
    pub fn members(&self) -> &[ValidatedMember] {
        &self.members
    }

    /// Family-scoped gaps, in declaration order.
    ///
    /// Document-level `[gaps]` are on [`ValidatedDefinitions::gaps`] and do not
    /// occupy family selector identities.
    pub fn gaps(&self) -> &[ValidatedFamilyGap] {
        &self.gaps
    }

    /// Occupancy result. Successful validation always yields
    /// [`FamilyCompleteness::Complete`].
    pub fn completeness(&self) -> FamilyCompleteness {
        self.completeness
    }
}

/// One validated family-scoped gap occupying an expected selector identity.
#[derive(Clone, Debug)]
pub struct ValidatedFamilyGap {
    selectors: BTreeMap<String, SelectorValue>,
    status: GapStatus,
    reason: String,
    provenance: SourceProvenance,
    provenance_override: Option<SourceProvenanceOverride>,
}

impl ValidatedFamilyGap {
    /// Selector map; keys, value types, and values are the identity.
    pub fn selectors(&self) -> &BTreeMap<String, SelectorValue> {
        &self.selectors
    }

    /// Always [`GapStatus::Undefined`].
    pub fn status(&self) -> GapStatus {
        self.status
    }

    /// Non-blank rationale for this expected identity not being a member.
    pub fn reason(&self) -> &str {
        &self.reason
    }

    /// Effective citation after applying the declared gap override to the
    /// mandatory family citation.
    pub fn provenance(&self) -> &SourceProvenance {
        &self.provenance
    }

    /// Gap-level citation override declared in the family document.
    pub fn provenance_override(&self) -> Option<&SourceProvenanceOverride> {
        self.provenance_override.as_ref()
    }
}

/// One validated selector combination.
#[derive(Clone, Debug)]
pub struct ValidatedMember {
    selectors: BTreeMap<String, SelectorValue>,
    input_transform: Option<InputTransform>,
    status: MemberStatus,
    reason: Option<String>,
    applicability: ApplicabilityDef,
    expanded_name: String,
    explicit_emitted_name: Option<String>,
    emitted_name: String,
    observation_domain: Option<[u16; 2]>,
    declared_provenance: SourceProvenance,
    provenance: SourceProvenance,
    provenance_override: Option<SourceProvenanceOverride>,
}

impl ValidatedMember {
    /// Selector map; keys, value types, and values are the identity.
    pub fn selectors(&self) -> &BTreeMap<String, SelectorValue> {
        &self.selectors
    }

    /// Exact input transform, when the shared source applies one.
    ///
    /// Present for mapped `kind = "scaled_polynomial"` members
    /// (`u = count * numerator / denominator`). Absent for formula, points,
    /// NTC, and `unsupported` members.
    pub fn input_transform(&self) -> Option<InputTransform> {
        self.input_transform
    }

    /// Whether this member is generated.
    pub fn status(&self) -> MemberStatus {
        self.status
    }

    /// Non-blank rationale for a non-`emit` status.
    pub fn reason(&self) -> Option<&str> {
        self.reason.as_deref()
    }

    /// Source-backed window in the coordinate space the shared source uses.
    /// All fields are absent for an `unsupported` member.
    pub fn applicability(&self) -> &ApplicabilityDef {
        &self.applicability
    }

    /// Candidate selector-derived transfer name.
    ///
    /// Always derived from the family name and selector map. An explicit
    /// [`Self::emitted_name`] may differ. Reserved for codegen only when
    /// [`Self::status`] is [`MemberStatus::Emit`] and no explicit stem was set.
    pub fn expanded_name(&self) -> &str {
        &self.expanded_name
    }

    /// Explicit table-name stem declared on the member, when present.
    pub fn explicit_emitted_name(&self) -> Option<&str> {
        self.explicit_emitted_name.as_deref()
    }

    /// Resolved table-name stem: explicit if set, otherwise [`Self::expanded_name`].
    ///
    /// Codegen and overlays key off this stem only when [`Self::status`] is
    /// [`MemberStatus::Emit`].
    pub fn emitted_name(&self) -> &str {
        &self.emitted_name
    }

    /// Resolved observation-code span for a source-mapped member.
    ///
    /// Present for `emit`, `unnecessary`, and `forbidden`. Absent for
    /// `unsupported`. For points sources this is the clipped control-point
    /// span, which may be narrower than `applicability.observation`.
    pub fn observation_domain(&self) -> Option<[u16; 2]> {
        self.observation_domain
    }

    /// Effective source citation after applying the declared member override
    /// and any successful generation-source overlay replacement.
    pub fn provenance(&self) -> &SourceProvenance {
        &self.provenance
    }

    /// Member-level citation override declared in the family document.
    ///
    /// Generation-source overlays do not rewrite this declaration; inspect
    /// [`Self::provenance`] for the effective citation used by generation.
    pub fn provenance_override(&self) -> Option<&SourceProvenanceOverride> {
        self.provenance_override.as_ref()
    }
}

impl DefinitionsFile {
    /// Validate families, gaps, and name collisions without generating Rust.
    ///
    /// Every family member is checked before non-`emit` statuses are filtered.
    /// Description-only members remain inspectable on the returned graph.
    pub fn validate(&self) -> Result<ValidatedDefinitions, Error> {
        let resolved = self.resolved_transfers().map_err(Error::Validation)?;
        let curves: Vec<_> = self.curves.iter().collect();
        let transfers: Vec<_> = resolved
            .iter()
            .map(|(name, resolved)| (name, &resolved.def))
            .collect();
        super::codegen::emitted_const_names(&curves, &transfers).map_err(Error::Validation)?;
        let families = inspect_families(&self.transfer_families).map_err(Error::Validation)?;
        let family_overlay_domains =
            family_overlay_domains(&families, &resolved).map_err(Error::Validation)?;
        let resolved_names = resolved.keys().cloned().collect();

        Ok(ValidatedDefinitions {
            defs: self.clone(),
            families,
            resolved_names,
            family_overlay_domains,
        })
    }

    /// Add a standalone transfer constructed without TOML.
    ///
    /// The spec's source is stored as an overlay so it is not forced through
    /// the TOML formula/points/model vocabulary.
    pub fn insert_transfer(&mut self, spec: TransferSpec) -> Result<(), Error> {
        let name = spec.name().to_string();
        check_insert_name(self, &name)?;
        let (_, def, source) = spec.into_parts();
        self.transfers.insert(name.clone(), def);
        self.overlays.insert(name, source.inherit_provenance());
        Ok(())
    }

    /// Add a family constructed without TOML.
    ///
    /// The spec becomes an ordinary [`TransferFamilyDef`] and then uses the
    /// same validation and generation pipeline as a parsed document.
    pub fn insert_family(&mut self, spec: FamilySpec) -> Result<(), Error> {
        let name = spec.name().to_string();
        check_insert_family_name(self, &name)?;
        let (_, family) = spec.into_family();
        self.transfer_families.insert(name, family);
        Ok(())
    }
}

fn check_insert_name(defs: &DefinitionsFile, name: &str) -> Result<(), Error> {
    if name.trim().is_empty() {
        return Err(Error::Validation("transfer name must not be blank".into()));
    }
    if defs.curves.contains_key(name) {
        return Err(Error::Validation(format!(
            "transfer `{name}` collides with a [curves] entry"
        )));
    }
    if defs.transfers.contains_key(name) {
        return Err(Error::Validation(format!(
            "transfer `{name}` collides with a [transfers] entry"
        )));
    }
    if defs.transfer_families.contains_key(name) {
        return Err(Error::Validation(format!(
            "transfer `{name}` collides with a [transfer_families] entry"
        )));
    }
    if defs.gaps.contains_key(name) {
        return Err(Error::Validation(format!(
            "transfer `{name}` collides with a [gaps] entry"
        )));
    }
    Ok(())
}

fn check_insert_family_name(defs: &DefinitionsFile, name: &str) -> Result<(), Error> {
    if name.trim().is_empty() {
        return Err(Error::Validation("family name must not be blank".into()));
    }
    if defs.curves.contains_key(name) {
        return Err(Error::Validation(format!(
            "family `{name}` collides with a [curves] entry"
        )));
    }
    if defs.transfers.contains_key(name) {
        return Err(Error::Validation(format!(
            "family `{name}` collides with a [transfers] entry"
        )));
    }
    if defs.transfer_families.contains_key(name) {
        return Err(Error::Validation(format!(
            "family `{name}` collides with a [transfer_families] entry"
        )));
    }
    if defs.gaps.contains_key(name) {
        return Err(Error::Validation(format!(
            "family `{name}` collides with a [gaps] entry"
        )));
    }
    Ok(())
}

fn inspect_families(
    families: &BTreeMap<String, TransferFamilyDef>,
) -> Result<Vec<ValidatedFamily>, String> {
    let mut out = Vec::new();
    for (family_name, family) in families {
        let selector_universe = resolved_selector_universe(family_name, family)?;
        let mut members = Vec::new();
        for member in &family.members {
            let provenance = resolved_member_provenance(family_name, family, member)?;
            members.push(ValidatedMember {
                selectors: member.selectors.clone(),
                input_transform: member.input_transform,
                status: member.status,
                reason: member.reason.clone(),
                applicability: member.applicability.clone(),
                expanded_name: expanded_name(family_name, &member.selectors)?,
                explicit_emitted_name: member.emitted_name.clone(),
                emitted_name: member_emitted_name(family_name, member)?,
                observation_domain: member_observation_domain(family_name, family, member)?,
                declared_provenance: provenance.clone(),
                provenance,
                provenance_override: member.provenance.clone(),
            });
        }
        let provenance = family.provenance.clone().ok_or_else(|| {
            format!(
                "transfer family `{family_name}`: source-backed family requires provenance.identity"
            )
        })?;
        let declared_source = family.declared_source().ok_or_else(|| {
            format!(
                "transfer family `{family_name}`: exactly one of points, formula, or model must be specified"
            )
        })?;
        let gaps = family
            .gaps
            .iter()
            .enumerate()
            .map(|(index, gap)| {
                Ok(ValidatedFamilyGap {
                    selectors: gap.selectors.clone(),
                    status: gap.status,
                    reason: gap.reason.clone(),
                    provenance: resolved_family_gap_provenance(family_name, family, index, gap)?,
                    provenance_override: gap.provenance.clone(),
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        out.push(ValidatedFamily {
            name: family_name.clone(),
            input_unit: family.input_unit.clone(),
            output_unit: family.output_unit.clone(),
            output_scale: family.output_scale,
            declared_source,
            formula: family.formula.clone(),
            points: family.points.clone(),
            has_model: family.has_model(),
            max_total_knots: family.max_total_knots,
            max_table_bytes: family.max_table_bytes,
            provenance,
            guard_provenance: resolve_guard_provenance(
                &format!("transfer family `{family_name}`"),
                family.observation_guard(),
                family.provenance(),
            )?,
            policy: family.policy(),
            selector_universe,
            members,
            gaps,
            completeness: FamilyCompleteness::Complete,
        });
    }
    Ok(out)
}

fn family_overlay_domains(
    families: &[ValidatedFamily],
    resolved: &BTreeMap<String, super::transfer::ResolvedTransfer>,
) -> Result<BTreeMap<String, [u16; 2]>, String> {
    let mut domains = BTreeMap::new();
    for family in families {
        for member in &family.members {
            if member.status != MemberStatus::Emit {
                continue;
            }
            let def = resolved.get(&member.emitted_name).ok_or_else(|| {
                format!(
                    "transfer family `{}`: emitted member `{}` was not expanded",
                    family.name, member.emitted_name
                )
            })?;
            let domain = family_source_observation_domain(&member.emitted_name, &def.def)?;
            domains.insert(member.emitted_name.clone(), domain);
        }
    }
    Ok(domains)
}

/// One emitted family member in the pre-fit identity manifest.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmissionEntry {
    /// Family table name.
    pub family: String,
    /// Typed selector identity.
    pub selectors: BTreeMap<String, SelectorValue>,
    /// Resolved table-name stem used by codegen.
    pub table_name: String,
    /// Emitted Rust constant name (`to_const_name` of [`Self::table_name`]).
    pub symbol: String,
    /// Companion metadata constant name (`{symbol}_METADATA`).
    pub metadata_symbol: String,
    /// Companion observation-guard constant name (`{symbol}_OBSERVATION_GUARD`).
    pub observation_guard_symbol: String,
}

/// Deterministic family-and-selector to symbol mapping after validation.
///
/// Only `emit` members appear. Description-only members and gaps remain on
/// [`ValidatedFamily`]. Available without fitting.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmissionManifest {
    entries: Vec<EmissionEntry>,
}

impl EmissionManifest {
    /// Emitted members sorted by table name. Every emit member appears once.
    pub fn entries(&self) -> &[EmissionEntry] {
        &self.entries
    }
}

impl ValidatedDefinitions {
    /// Validated families in name order (`BTreeMap` iteration).
    pub fn families(&self) -> &[ValidatedFamily] {
        &self.families
    }

    /// Declared document-level gaps. A missing gap is not the same as an
    /// undefined one. Family-scoped selector gaps live on
    /// [`ValidatedFamily::gaps`].
    pub fn gaps(&self) -> &BTreeMap<String, GapDef> {
        self.defs.gaps()
    }

    /// Names that will emit `PiecewiseLinearTransfer` constants.
    pub fn emitted_transfer_names(&self) -> impl Iterator<Item = &str> {
        self.resolved_names.iter().map(String::as_str)
    }

    /// Family plus typed-selector mapping to emitted symbols and companions.
    ///
    /// Order is deterministic by table name. Description-only members and gaps
    /// are omitted. Standalone transfers are omitted; they are not family
    /// members.
    pub fn emission_manifest(&self) -> EmissionManifest {
        let mut entries = Vec::new();
        for family in &self.families {
            for member in &family.members {
                if member.status != MemberStatus::Emit {
                    continue;
                }
                let symbol = super::codegen::to_const_name(&member.emitted_name)
                    .expect("validated emit stems normalize to Rust identifiers");
                entries.push(EmissionEntry {
                    family: family.name.clone(),
                    selectors: member.selectors.clone(),
                    table_name: member.emitted_name.clone(),
                    metadata_symbol: format!("{symbol}_METADATA"),
                    observation_guard_symbol: format!("{symbol}_OBSERVATION_GUARD"),
                    symbol,
                });
            }
        }
        entries.sort_by(|left, right| left.table_name.cmp(&right.table_name));
        EmissionManifest { entries }
    }

    /// Overlay a generation source on a standalone transfer or emitted member.
    ///
    /// Rejects unknown names and description-only members so source facts on
    /// the graph stay distinct from generation input. Family-member overlays
    /// must cover the resolved shared-source observation domain exactly;
    /// standalone overlays replace their declared source and may define a
    /// different observation domain. An `Inherit` disposition always means
    /// the target's declared, resolved pre-overlay citation; replacing an
    /// earlier overlay with `Inherit` restores it.
    pub fn set_source(&mut self, name: &str, overlay: TransferSourceOverlay) -> Result<(), Error> {
        if self.resolved_names.contains(name) {
            let is_family_member = self.family_overlay_domains.contains_key(name);
            if let Some(expected) = self.family_overlay_domains.get(name) {
                if matches!(
                    overlay.provenance_disposition(),
                    SourceProvenanceDisposition::Clear
                ) {
                    return Err(Error::Validation(format!(
                        "transfer `{name}`: a source-backed family overlay cannot clear its mandatory provenance; inherit it intentionally or supply a replacement"
                    )));
                }
                let span =
                    overlay_observation_span(name, overlay.source()).map_err(Error::Validation)?;
                if span != *expected {
                    return Err(Error::Validation(format!(
                        "transfer `{name}`: overlay observation domain [{}, {}] must equal \
                         resolved member observation domain [{}, {}]",
                        span[0], span[1], expected[0], expected[1]
                    )));
                }
            }
            let replacement = match overlay.provenance_disposition() {
                SourceProvenanceDisposition::Replace(provenance) => {
                    provenance.validate().map_err(|error| {
                        Error::Validation(format!("transfer `{name}`: {error}"))
                    })?;
                    Some(provenance.clone())
                }
                SourceProvenanceDisposition::Inherit | SourceProvenanceDisposition::Clear => None,
            };
            if is_family_member {
                let member = self
                    .families
                    .iter_mut()
                    .flat_map(|family| family.members.iter_mut())
                    .find(|member| {
                        member.emitted_name == name && member.status == MemberStatus::Emit
                    })
                    .ok_or_else(|| {
                        Error::Validation(format!(
                            "transfer `{name}`: emitted family member is missing from the validated graph"
                        ))
                    })?;
                member.provenance =
                    replacement.unwrap_or_else(|| member.declared_provenance.clone());
            }
            self.defs.overlays.insert(name.to_string(), overlay);
            return Ok(());
        }
        if self.is_description_only(name) {
            return Err(Error::Validation(format!(
                "transfer `{name}` is a description-only family member; overlays apply only to emitted members"
            )));
        }
        Err(Error::Validation(format!(
            "transfer `{name}` is not a standalone transfer or emitted family member"
        )))
    }

    /// Add a standalone transfer after validation.
    pub fn insert_transfer(&mut self, spec: TransferSpec) -> Result<(), Error> {
        let name = spec.name().to_string();
        if self.resolved_names.contains(&name) {
            return Err(Error::Validation(format!(
                "transfer `{name}` collides with an emitted transfer"
            )));
        }
        if self.is_description_only(&name) {
            return Err(Error::Validation(format!(
                "transfer `{name}` collides with a description-only family member name"
            )));
        }
        self.defs.insert_transfer(spec)?;
        self.resolved_names.insert(name);
        Ok(())
    }

    /// Add a family after validation and rebuild the inspectable graph.
    pub fn insert_family(&mut self, spec: FamilySpec) -> Result<(), Error> {
        let mut defs = self.defs.clone();
        defs.insert_family(spec)?;
        *self = defs.validate()?;
        Ok(())
    }

    /// Emit Rust source. LUT options are required only when the document has curves.
    pub fn generate(&self, opts: &GenerateOptions) -> Result<String, Error> {
        super::api::generate(&self.defs, opts)
    }

    /// Emit Rust source plus the host audit report.
    ///
    /// Family aggregate budgets fail closed here, matching [`Self::generate`].
    pub fn generate_report(&self, opts: &GenerateOptions) -> Result<GenerationResult, Error> {
        super::api::generate_report(&self.defs, opts)
    }

    fn is_description_only(&self, name: &str) -> bool {
        self.families.iter().any(|family| {
            family.members.iter().any(|member| {
                member.status != MemberStatus::Emit
                    && (member.expanded_name == name || member.emitted_name == name)
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::r#gen::{
        ApplicabilityDef, DeclaredSource, DividerTopology, FamilyCompleteness, FamilyGapDef,
        FamilyMemberDef, FamilySource, FamilySpec, GapStatus, GenerateOptions, InputTransform,
        MemberStatus, ObservationGuardBehaviorDef, ObservationGuardDef, PhysicalPoint,
        SelectorUniverse, SelectorValue, SourceProvenance, SourceProvenanceOverride,
        TransferSource, TransferSpec,
    };
    use std::vec;

    fn family_toml() -> &'static str {
        r#"
[transfer_families.als]
provenance = { identity = "test fixture" }
input_unit = "count"
output_unit = "unit"
output_scale = 1000
max_interpolation_error = 50
formula = "x"
selector_axes = { gain = ["div4", "x1", "x2"], integration_time_ms = [100] }

[[transfer_families.als.members]]
selectors = { gain = "div4", integration_time_ms = 100 }
status = "emit"
applicability = { observation = [1, 10] }

[[transfer_families.als.members]]
selectors = { gain = "x1", integration_time_ms = 100 }
status = "unnecessary"
reason = "high-gain row is documented, not generated"
applicability = { observation = [1, 10] }

[[transfer_families.als.members]]
selectors = { gain = "x2", integration_time_ms = 100 }
status = "forbidden"
reason = "exceeds the absolute maximum rating"
applicability = { observation = [1, 10] }

[gaps.white_channel]
status = "undefined"
reason = "counts only; no conversion"
"#
    }

    #[test]
    fn validate_inspects_every_member_and_gap_reason() {
        let defs = DefinitionsFile::from_toml_str(family_toml()).unwrap();
        assert_eq!(
            defs.transfer_families()["als"].declared_source(),
            Some(crate::r#gen::DeclaredSource::Formula)
        );
        assert_eq!(defs.transfer_families()["als"].formula(), Some("x"));

        let validated = defs.validate().unwrap();
        assert_eq!(validated.families().len(), 1);
        let family = &validated.families()[0];
        assert_eq!(family.name(), "als");
        assert_eq!(family.input_unit(), "count");
        assert_eq!(family.output_unit(), "unit");
        assert_eq!(family.output_scale(), 1000);
        assert_eq!(
            family.declared_source(),
            crate::r#gen::DeclaredSource::Formula
        );
        assert_eq!(family.formula(), Some("x"));
        assert!(family.points().is_none());
        assert!(!family.has_model());
        assert_eq!(family.max_total_knots(), None);
        assert_eq!(family.max_table_bytes(), None);
        assert_eq!(family.members().len(), 3);

        let emit = &family.members()[0];
        assert_eq!(emit.status(), MemberStatus::Emit);
        assert_eq!(emit.input_transform(), None);
        assert_eq!(emit.applicability().observation, Some([1, 10]));
        assert_eq!(
            emit.selectors()["gain"],
            SelectorValue::String("div4".into())
        );
        assert_eq!(
            emit.expanded_name(),
            "als_gain_div4_integration_time_ms_100"
        );
        assert_eq!(emit.explicit_emitted_name(), None);
        assert_eq!(emit.emitted_name(), "als_gain_div4_integration_time_ms_100");
        assert_eq!(emit.observation_domain(), Some([1, 10]));

        assert_eq!(family.members()[1].status(), MemberStatus::Unnecessary);
        assert_eq!(
            family.members()[1].reason(),
            Some("high-gain row is documented, not generated")
        );
        assert_eq!(
            family.members()[1].expanded_name(),
            "als_gain_x1_integration_time_ms_100"
        );
        assert_eq!(family.members()[1].observation_domain(), Some([1, 10]));
        assert_eq!(family.members()[2].status(), MemberStatus::Forbidden);
        assert_eq!(family.members()[2].observation_domain(), Some([1, 10]));

        assert_eq!(
            validated.gaps()["white_channel"].reason,
            "counts only; no conversion"
        );
        let emitted: Vec<_> = validated.emitted_transfer_names().collect();
        assert_eq!(emitted, ["als_gain_div4_integration_time_ms_100"]);
        let manifest = validated.emission_manifest();
        assert_eq!(manifest.entries().len(), 1);
        let entry = &manifest.entries()[0];
        assert_eq!(entry.family, "als");
        assert_eq!(entry.table_name, "als_gain_div4_integration_time_ms_100");
        assert_eq!(entry.symbol, "ALS_GAIN_DIV4_INTEGRATION_TIME_MS_100");
        assert_eq!(
            entry.metadata_symbol,
            "ALS_GAIN_DIV4_INTEGRATION_TIME_MS_100_METADATA"
        );
        assert_eq!(
            entry.observation_guard_symbol,
            "ALS_GAIN_DIV4_INTEGRATION_TIME_MS_100_OBSERVATION_GUARD"
        );
        assert_eq!(
            entry.selectors["gain"],
            SelectorValue::String("div4".into())
        );
        assert_eq!(family.completeness(), FamilyCompleteness::Complete);
        assert_eq!(family.gaps().len(), 0);
        match family.selector_universe() {
            SelectorUniverse::Cartesian { axes } => {
                assert_eq!(axes.get("gain").map(Vec::len), Some(3));
                assert_eq!(axes.get("integration_time_ms").map(Vec::len), Some(1));
            }
            other => panic!("expected cartesian universe, got {other:?}"),
        }
        assert_eq!(family.selector_universe().identity_count(), Some(3));
        assert_eq!(family.selector_universe().identities().count(), 3);
    }

    #[test]
    fn validate_enumerates_family_scoped_gaps_independently_of_global_gaps() {
        let toml = r#"
[transfer_families.front_end]
provenance = { identity = "test fixture" }
input_unit = "count"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
formula = "x"
selector_axes = { range = ["low", "high"], gain = [1, 8] }

[[transfer_families.front_end.members]]
selectors = { range = "low", gain = 1 }
status = "emit"
applicability = { observation = [1, 10] }

[[transfer_families.front_end.members]]
selectors = { range = "high", gain = 1 }
status = "forbidden"
reason = "exceeds the absolute maximum rating"
applicability = { observation = [1, 10] }

[[transfer_families.front_end.members]]
selectors = { range = "high", gain = 8 }
status = "emit"
applicability = { observation = [1, 10] }

[[transfer_families.front_end.gaps]]
selectors = { range = "low", gain = 8 }
status = "undefined"
reason = "not characterized at this combination"

[gaps.white_channel]
status = "undefined"
reason = "counts only; no conversion"
"#;
        let validated = DefinitionsFile::from_toml_str(toml)
            .unwrap()
            .validate()
            .unwrap();
        let family = &validated.families()[0];
        assert_eq!(family.completeness(), FamilyCompleteness::Complete);
        assert_eq!(family.members().len(), 3);
        assert_eq!(family.gaps().len(), 1);
        assert_eq!(
            family.gaps()[0].reason(),
            "not characterized at this combination"
        );
        assert_eq!(family.gaps()[0].status(), GapStatus::Undefined);
        assert_eq!(
            family.gaps()[0].selectors()["gain"],
            SelectorValue::Integer(8)
        );
        let identities: Vec<_> = family.selector_universe().identities().collect();
        assert_eq!(identities.len(), 4);
        assert!(identities.iter().any(|identity| {
            identity.get("range") == Some(&SelectorValue::String("low".into()))
                && identity.get("gain") == Some(&SelectorValue::Integer(8))
        }));
        assert_eq!(
            validated.gaps()["white_channel"].reason,
            "counts only; no conversion"
        );
        assert_eq!(
            family.members()[1].reason(),
            Some("exceeds the absolute maximum rating")
        );
    }

    #[test]
    fn family_gap_inherits_family_provenance_in_validated_ir() {
        let toml = r#"
[transfer_families.front_end]
provenance = { identity = "front-end datasheet", revision = "2.0", locator = "Table 7", url = "https://example.invalid/front-end", note = "characterization matrix" }
input_unit = "count"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
formula = "x"
selector_axes = { mode = ["mapped", "undefined"] }

[[transfer_families.front_end.members]]
selectors = { mode = "mapped" }
status = "emit"
applicability = { observation = [1, 10] }

[[transfer_families.front_end.gaps]]
selectors = { mode = "undefined" }
status = "undefined"
reason = "the source does not characterize this mode"
"#;
        let validated = DefinitionsFile::from_toml_str(toml)
            .unwrap()
            .validate()
            .unwrap();
        let gap = &validated.families()[0].gaps()[0];
        let expected = SourceProvenance::new("front-end datasheet")
            .with_revision("2.0")
            .with_locator("Table 7")
            .with_url("https://example.invalid/front-end")
            .with_note("characterization matrix");

        assert_eq!(gap.provenance(), &expected);
        assert_eq!(gap.provenance_override(), None);
    }

    #[test]
    fn family_gap_override_replaces_identity_or_clears_inherited_fields() {
        let toml = r#"
[transfer_families.front_end]
provenance = { identity = "front-end datasheet", revision = "2.0", locator = "Table 7", url = "https://example.invalid/front-end", note = "characterization matrix" }
input_unit = "count"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
formula = "x"
selector_axes = { mode = ["mapped", "replaced", "cleared"] }

[[transfer_families.front_end.members]]
selectors = { mode = "mapped" }
status = "emit"
applicability = { observation = [1, 10] }

[[transfer_families.front_end.gaps]]
selectors = { mode = "replaced" }
status = "undefined"
reason = "defined by a different source"
provenance = { identity = "errata sheet" }

[[transfer_families.front_end.gaps]]
selectors = { mode = "cleared" }
status = "undefined"
reason = "the table locator does not apply"
provenance = { clear = ["locator"] }
"#;
        let validated = DefinitionsFile::from_toml_str(toml)
            .unwrap()
            .validate()
            .unwrap();
        let gaps = validated.families()[0].gaps();

        assert_eq!(gaps[0].provenance(), &SourceProvenance::new("errata sheet"));
        let replaced = gaps[0].provenance_override().unwrap();
        assert_eq!(replaced.identity.as_deref(), Some("errata sheet"));
        assert!(replaced.revision.is_none());
        assert!(replaced.locator.is_none());
        assert!(replaced.url.is_none());
        assert!(replaced.note.is_none());

        assert_eq!(gaps[1].provenance().identity, "front-end datasheet");
        assert_eq!(gaps[1].provenance().revision.as_deref(), Some("2.0"));
        assert_eq!(gaps[1].provenance().locator, None);
        assert_eq!(
            gaps[1].provenance().url.as_deref(),
            Some("https://example.invalid/front-end")
        );
        assert_eq!(
            gaps[1].provenance().note.as_deref(),
            Some("characterization matrix")
        );
        assert!(
            gaps[1]
                .provenance_override()
                .unwrap()
                .clear
                .contains(&crate::r#gen::SourceProvenanceField::Locator)
        );
    }

    #[test]
    fn invalid_family_gap_provenance_overrides_fail_validation() {
        let template = r#"
[transfer_families.front_end]
provenance = { identity = "front-end datasheet", locator = "Table 7" }
input_unit = "count"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
formula = "x"
selector_axes = { mode = ["mapped", "undefined"] }

[[transfer_families.front_end.members]]
selectors = { mode = "mapped" }
status = "emit"
applicability = { observation = [1, 10] }

[[transfer_families.front_end.gaps]]
selectors = { mode = "undefined" }
status = "undefined"
reason = "not characterized"
provenance = __OVERRIDE__
"#;
        for (declaration, expected) in [
            (
                r#"{ locator = "   " }"#,
                "provenance.locator must not be blank",
            ),
            (
                r#"{ locator = "Table 9", clear = ["locator"] }"#,
                "provenance.locator cannot be both set and cleared",
            ),
        ] {
            let error =
                DefinitionsFile::from_toml_str(&template.replace("__OVERRIDE__", declaration))
                    .unwrap()
                    .validate()
                    .unwrap_err()
                    .to_string();
            assert!(error.contains("family-scoped gap 0"), "{error}");
            assert!(error.contains(expected), "{error}");
        }
    }

    #[test]
    fn programmatic_family_gap_override_is_preserved_in_validated_ir() {
        let toml = r#"
[transfer_families.front_end]
provenance = { identity = "front-end datasheet", locator = "Table 7", url = "https://example.invalid/front-end" }
input_unit = "count"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
formula = "x"
selector_axes = { mode = ["mapped", "undefined"] }

[[transfer_families.front_end.members]]
selectors = { mode = "mapped" }
status = "emit"
applicability = { observation = [1, 10] }
"#;
        let mut defs = DefinitionsFile::from_toml_str(toml).unwrap();
        let declared =
            SourceProvenanceOverride::default().clearing(crate::r#gen::SourceProvenanceField::Url);
        defs.transfer_families
            .get_mut("front_end")
            .unwrap()
            .gaps
            .push(crate::r#gen::FamilyGapDef {
                selectors: BTreeMap::from([(
                    "mode".into(),
                    SelectorValue::String("undefined".into()),
                )]),
                status: GapStatus::Undefined,
                reason: "not characterized".into(),
                provenance: Some(declared.clone()),
            });

        let validated = defs.validate().unwrap();
        let gap = &validated.families()[0].gaps()[0];
        assert_eq!(gap.provenance().identity, "front-end datasheet");
        assert_eq!(gap.provenance().locator.as_deref(), Some("Table 7"));
        assert_eq!(gap.provenance().url, None);
        assert_eq!(gap.provenance_override(), Some(&declared));
    }

    #[test]
    fn provenance_remains_a_valid_family_gap_selector_key() {
        let toml = r#"
[transfer_families.front_end]
provenance = { identity = "front-end datasheet", locator = "Table 7" }
input_unit = "count"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
formula = "x"
selector_axes = { provenance = ["mapped", "undefined"] }

[[transfer_families.front_end.members]]
selectors = { provenance = "mapped" }
status = "emit"
applicability = { observation = [1, 10] }

[[transfer_families.front_end.gaps]]
selectors = { provenance = "undefined" }
status = "undefined"
reason = "not characterized"
provenance = { locator = "Table 8" }
"#;
        let validated = DefinitionsFile::from_toml_str(toml)
            .unwrap()
            .validate()
            .unwrap();
        let gap = &validated.families()[0].gaps()[0];
        assert_eq!(
            gap.selectors()["provenance"],
            SelectorValue::String("undefined".into())
        );
        assert_eq!(gap.provenance().identity, "front-end datasheet");
        assert_eq!(gap.provenance().locator.as_deref(), Some("Table 8"));
    }

    #[test]
    fn overlay_evaluated_truth_emits_ordinary_transfer() {
        let defs = DefinitionsFile::from_toml_str(family_toml()).unwrap();
        let mut validated = defs.validate().unwrap();
        let physical: Vec<f64> = (1..=10).map(f64::from).collect();
        validated
            .set_source(
                "als_gain_div4_integration_time_ms_100",
                TransferSource::evaluated_truth(1, physical).inherit_provenance(),
            )
            .unwrap();

        let description_only = validated.set_source(
            "als_gain_x1_integration_time_ms_100",
            TransferSource::evaluated_truth(1, vec![1.0, 2.0]).inherit_provenance(),
        );
        assert!(
            description_only
                .unwrap_err()
                .to_string()
                .contains("description-only")
        );

        let out = validated
            .generate(&GenerateOptions::transfers_only())
            .unwrap();
        assert!(out.contains("PiecewiseLinearTransfer"));
        assert!(out.contains("ALS_GAIN_DIV4_INTEGRATION_TIME_MS_100"));
        assert!(out.contains("evaluated physical truth"));
        assert!(out.contains("TransferMetadata"));
        assert!(!out.contains("CurveLut"));
        assert!(!out.contains("pub const ALS_GAIN_X1"));
    }

    #[test]
    fn overlay_evaluated_truth_must_match_member_observation_domain() {
        let defs = DefinitionsFile::from_toml_str(family_toml()).unwrap();
        let mut validated = defs.validate().unwrap();
        let error = validated
            .set_source(
                "als_gain_div4_integration_time_ms_100",
                TransferSource::evaluated_truth(2, vec![2.0, 3.0, 4.0]).inherit_provenance(),
            )
            .unwrap_err()
            .to_string();
        assert!(error.contains("overlay observation domain"), "{error}");
        assert!(error.contains("[2, 4]"), "{error}");
        assert!(error.contains("[1, 10]"), "{error}");
    }

    #[test]
    fn points_family_overlay_uses_resolved_clipped_control_point_domain() {
        let defs = DefinitionsFile::from_toml_str(
            r#"
                [transfer_families.front_end]
provenance = { identity = "test fixture" }
                input_unit = "count"
                output_unit = "unit"
                output_scale = 1
                max_interpolation_error = 1
                selector_axes = { range = ["middle"] }
                points = [
                    { input = 1, output = 1.0 },
                    { input = 5, output = 5.0 },
                    { input = 8, output = 8.0 },
                    { input = 10, output = 10.0 },
                ]

                [[transfer_families.front_end.members]]
                selectors = { range = "middle" }
                status = "emit"
                applicability = { observation = [2, 9] }
            "#,
        )
        .unwrap();
        let mut validated = defs.validate().unwrap();
        let name = "front_end_range_middle";
        assert_eq!(validated.family_overlay_domains[name], [5, 8]);

        let error = validated
            .set_source(
                name,
                TransferSource::evaluated_truth(2, (2..=9).map(f64::from).collect())
                    .inherit_provenance(),
            )
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("overlay observation domain [2, 9]"),
            "{error}"
        );
        assert!(error.contains("[5, 8]"), "{error}");

        validated
            .set_source(
                name,
                TransferSource::evaluated_truth(5, (5..=8).map(f64::from).collect())
                    .inherit_provenance(),
            )
            .unwrap();
    }

    #[test]
    fn ntc_family_overlay_uses_derived_observation_domain() {
        let defs = DefinitionsFile::from_toml_str(
            r#"
                [transfer_families.ntc]
provenance = { identity = "test fixture" }
                input_unit = "adc_code"
                output_unit = "degree_celsius"
                output_scale = 1000
                max_interpolation_error = 50
                selector_axes = { probe = ["wide"] }

                [transfer_families.ntc.model]
                kind = "ntc_beta_divider"
                nominal_resistance_ohms = 10000.0
                beta_kelvin = 3950.0
                nominal_temperature_celsius = 25.0
                fixed_resistance_ohms = 10000.0
                adc_max_code = 4095
                topology = "ntc_to_ground"

                [[transfer_families.ntc.members]]
                selectors = { probe = "wide" }
                status = "emit"
                applicability = { physical = [-20.0, 80.0] }
            "#,
        )
        .unwrap();
        let mut validated = defs.validate().unwrap();
        let name = "ntc_probe_wide";
        let expected = validated.family_overlay_domains[name];
        assert_eq!(expected, [462, 3740]);

        let wrong_len = usize::from(expected[1] - expected[0]);
        let error = validated
            .set_source(
                name,
                TransferSource::evaluated_truth(expected[0] + 1, vec![0.0; wrong_len])
                    .inherit_provenance(),
            )
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("overlay observation domain [463, 3740]"),
            "{error}"
        );
        assert!(error.contains("[462, 3740]"), "{error}");

        let matching_len = wrong_len + 1;
        validated
            .set_source(
                name,
                TransferSource::evaluated_truth(expected[0], vec![0.0; matching_len])
                    .inherit_provenance(),
            )
            .unwrap();
    }

    #[test]
    fn standalone_toml_overlay_may_replace_the_declared_domain() {
        let defs = DefinitionsFile::from_toml_str(
            r#"
                [transfers.standalone]
                input_unit = "code"
                output_unit = "unit"
                output_scale = 1
                max_interpolation_error = 1
                formula = "x"
                domain = [1, 10]
            "#,
        )
        .unwrap();
        let mut validated = defs.validate().unwrap();
        validated
            .set_source(
                "standalone",
                TransferSource::evaluated_truth(20, vec![20.0, 21.0, 22.0]).clear_provenance(),
            )
            .unwrap();

        let out = validated
            .generate(&GenerateOptions::transfers_only())
            .unwrap();
        assert!(out.contains("domain_min: 20"), "{out}");
        assert!(out.contains("domain_max: 22"), "{out}");
        assert!(out.contains("evaluated physical truth"), "{out}");
    }

    fn standalone_with_provenance_toml() -> &'static str {
        r#"
[transfers]
requires = ["source_provenance_v1"]

[transfers.standalone]
input_unit = "code"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
provenance = { identity = "old source", locator = "old table" }
formula = "x"
domain = [1, 3]
"#
    }

    fn replacement_truth_source() -> TransferSource {
        TransferSource::evaluated_truth(1, vec![10.0, 20.0, 30.0])
    }

    #[test]
    fn standalone_source_overlay_replaces_provenance() {
        let mut validated = DefinitionsFile::from_toml_str(standalone_with_provenance_toml())
            .unwrap()
            .validate()
            .unwrap();
        validated
            .set_source(
                "standalone",
                replacement_truth_source().with_provenance(
                    SourceProvenance::new("replacement source").with_locator("new table"),
                ),
            )
            .unwrap();

        let out = validated
            .generate(&GenerateOptions::transfers_only())
            .unwrap();
        assert!(out.contains(r#"identity "replacement source"; locator "new table""#));
        assert!(!out.contains("old source"), "{out}");
        assert!(!out.contains("old table"), "{out}");
    }

    #[test]
    fn standalone_source_overlay_can_clear_provenance() {
        let mut validated = DefinitionsFile::from_toml_str(standalone_with_provenance_toml())
            .unwrap()
            .validate()
            .unwrap();
        validated
            .set_source("standalone", replacement_truth_source().clear_provenance())
            .unwrap();

        let out = validated
            .generate(&GenerateOptions::transfers_only())
            .unwrap();
        assert!(out.contains("Source provenance: none declared."), "{out}");
        assert!(!out.contains("old source"), "{out}");
    }

    #[test]
    fn standalone_source_overlay_can_intentionally_inherit_provenance() {
        let mut validated = DefinitionsFile::from_toml_str(standalone_with_provenance_toml())
            .unwrap()
            .validate()
            .unwrap();
        validated
            .set_source(
                "standalone",
                replacement_truth_source().inherit_provenance(),
            )
            .unwrap();

        let out = validated
            .generate(&GenerateOptions::transfers_only())
            .unwrap();
        assert!(out.contains(r#"identity "old source"; locator "old table""#));
    }

    #[test]
    fn standalone_guard_citation_survives_source_provenance_replace_and_clear() {
        let toml = r#"
[transfers]
requires = ["observation_guard_v1", "source_provenance_v1"]

[transfers.standalone]
input_unit = "code"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
provenance = { identity = "declared source", revision = "A", locator = "transfer table" }
saturation = { code = 65535, behavior = "error", provenance = { locator = "guard table" } }
formula = "x"
domain = [1, 3]
"#;
        let mut validated = DefinitionsFile::from_toml_str(toml)
            .unwrap()
            .validate()
            .unwrap();
        validated
            .set_source(
                "standalone",
                replacement_truth_source().with_provenance(SourceProvenance::new("overlay source")),
            )
            .unwrap();
        let replaced = validated
            .generate(&GenerateOptions::transfers_only())
            .unwrap();
        assert!(replaced.contains(
            r#"Classification of this code as saturation is cited from identity "declared source"; revision "A"; locator "guard table""#
        ));
        assert!(replaced.contains(r#"Source provenance: identity "overlay source"."#));
        assert!(!replaced.contains(
            r#"Classification of this code as saturation is cited from identity "overlay source""#
        ));

        validated
            .set_source("standalone", replacement_truth_source().clear_provenance())
            .unwrap();
        let cleared = validated
            .generate(&GenerateOptions::transfers_only())
            .unwrap();
        assert!(cleared.contains(
            r#"Classification of this code as saturation is cited from identity "declared source"; revision "A"; locator "guard table""#
        ));
        assert!(cleared.contains("Source provenance: none declared."));
    }

    #[test]
    fn family_source_overlay_requires_provenance() {
        let mut validated = DefinitionsFile::from_toml_str(family_toml())
            .unwrap()
            .validate()
            .unwrap();
        let error = validated
            .set_source(
                "als_gain_div4_integration_time_ms_100",
                TransferSource::evaluated_truth(1, (1..=10).map(f64::from).collect())
                    .clear_provenance(),
            )
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("cannot clear its mandatory provenance"),
            "{error}"
        );

        validated
            .set_source(
                "als_gain_div4_integration_time_ms_100",
                TransferSource::evaluated_truth(1, (1..=10).map(f64::from).collect())
                    .with_provenance(SourceProvenance::new("replacement family source")),
            )
            .unwrap();
        let out = validated
            .generate(&GenerateOptions::transfers_only())
            .unwrap();
        assert!(out.contains(r#"Source provenance: identity "replacement family source"."#));
        assert!(!out.contains(r#"Source provenance: identity "test fixture"."#));
    }

    #[test]
    fn programmatic_transfer_overlay_may_replace_its_source_domain() {
        let mut defs = DefinitionsFile::default();
        defs.insert_transfer(TransferSpec::new(
            "programmatic",
            "code",
            "unit",
            1,
            1,
            TransferSource::evaluated_truth(1, vec![1.0, 2.0]),
        ))
        .unwrap();
        let mut validated = defs.validate().unwrap();
        validated
            .set_source(
                "programmatic",
                TransferSource::evaluated_truth(30, vec![30.0, 31.0, 32.0]).clear_provenance(),
            )
            .unwrap();

        let out = validated
            .generate(&GenerateOptions::transfers_only())
            .unwrap();
        assert!(out.contains("domain_min: 30"), "{out}");
        assert!(out.contains("domain_max: 32"), "{out}");
    }

    #[test]
    fn prefitted_knots_on_a_family_member_must_match_observation_domain() {
        let defs = DefinitionsFile::from_toml_str(family_toml()).unwrap();
        let mut validated = defs.validate().unwrap();
        let truth = crate::r#gen::EvaluatedTruth::new(1, (1..=10).map(f64::from).collect());
        validated
            .set_source(
                "als_gain_div4_integration_time_ms_100",
                TransferSource::prefitted_knots_verified(vec![1, 10], vec![1000, 10_000], truth)
                    .inherit_provenance(),
            )
            .unwrap();
        let out = validated
            .generate(&GenerateOptions::transfers_only())
            .unwrap();
        assert!(out.contains("verified against evaluated truth"));
        assert!(out.contains("ALS_GAIN_DIV4_INTEGRATION_TIME_MS_100"));
    }

    #[test]
    fn prefitted_knots_reuse_codegen_without_copying_emit_path() {
        let mut defs = DefinitionsFile::default();
        let truth = crate::r#gen::EvaluatedTruth::new(
            20,
            (20..=100)
                .map(|input| f64::from(input - 20) / 2.0)
                .collect(),
        );
        defs.insert_transfer(
            TransferSpec::new(
                "linear",
                "code",
                "unit",
                1000,
                1,
                TransferSource::prefitted_knots_verified(vec![20, 100], vec![0, 40_000], truth),
            )
            .with_max_knots(8),
        )
        .unwrap();

        let out = crate::r#gen::generate(&defs, &GenerateOptions::transfers_only()).unwrap();
        assert!(out.contains("pub const LINEAR: PiecewiseLinearTransfer<2>"));
        assert!(out.contains("LINEAR_METADATA"));
        assert!(out.contains("verified against evaluated truth"));
        assert!(out.contains("achieved_max_error: 0"));
        assert_eq!(out.matches("pub const LINEAR:").count(), 1);
    }

    #[test]
    fn validate_rejects_codegen_identifier_collisions() {
        let toml = r#"
            [transfers."foo-bar"]
            input_unit = "code"
            output_unit = "unit"
            output_scale = 1
            max_interpolation_error = 1
            points = [{ input = 0, output = 0.0 }, { input = 1, output = 1.0 }]

            [transfers.foo_bar]
            input_unit = "code"
            output_unit = "unit"
            output_scale = 1
            max_interpolation_error = 1
            points = [{ input = 0, output = 0.0 }, { input = 1, output = 1.0 }]
        "#;
        let defs = DefinitionsFile::from_toml_str(toml).unwrap();
        let error = defs.validate().unwrap_err();
        assert!(
            error
                .to_string()
                .contains("both normalize to Rust identifier")
        );
    }

    #[test]
    fn points_overlay_replaces_formula_source_fields() {
        let defs = DefinitionsFile::from_toml_str(family_toml()).unwrap();
        let mut validated = defs.validate().unwrap();
        validated
            .set_source(
                "als_gain_div4_integration_time_ms_100",
                TransferSource::points(vec![
                    PhysicalPoint::new(1, 1.0),
                    PhysicalPoint::new(10, 10.0),
                ])
                .inherit_provenance(),
            )
            .unwrap();

        let out = validated
            .generate(&GenerateOptions::transfers_only())
            .unwrap();
        assert!(out.contains("physical points (2 control points)"));
    }

    #[test]
    fn programmatic_points_spec_matches_toml_points_shape() {
        let mut defs = DefinitionsFile::default();
        defs.insert_transfer(
            TransferSpec::new(
                "points",
                "adc_code",
                "degree_celsius",
                1000,
                50,
                TransferSource::points(vec![
                    PhysicalPoint::new(100, -10.0),
                    PhysicalPoint::new(200, 40.0),
                ]),
            )
            .with_max_knots(8)
            .with_observation_guard(ObservationGuardDef {
                code: 65_535,
                behavior: ObservationGuardBehaviorDef::Error,
                provenance: None,
            }),
        )
        .unwrap();

        let out = crate::r#gen::generate(
            &defs,
            &GenerateOptions {
                value_type: crate::r#gen::ValueType::U8,
                lut_size: 1,
            },
        )
        .unwrap();
        assert!(out.contains("pub const POINTS: PiecewiseLinearTransfer<2>"));
        assert!(out.contains("physical points (2 control points)"));
        assert!(out.contains("-10000") || out.contains("-10_000"));
        assert!(out.contains(".with_observation_guard(65535, ObservationGuardBehavior::Error)"));
        assert!(out.contains("POINTS_OBSERVATION_GUARD"));
        assert!(out.contains("code: 65535"));
    }

    #[test]
    fn provenance_round_trips_through_toml_and_inspection() {
        let toml = r#"
[transfer_families.als]
provenance = { identity = "synthetic ALS application note", revision = "1.0", locator = "Table 1", url = "https://example.invalid/als", note = "gain/IT matrix" }
input_unit = "count"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
max_knots = 8
below = "error"
above = "clamp"
formula = "x"
selector_axes = { gain = ["div4"] }

[[transfer_families.als.members]]
selectors = { gain = "div4" }
status = "emit"
applicability = { observation = [1, 10] }

[gaps.white_channel]
status = "undefined"
reason = "counts only; no conversion"
provenance = { identity = "synthetic ALS application note", locator = "§9 white channel" }
"#;
        let defs = DefinitionsFile::from_toml_str(toml).unwrap();
        let validated = defs.validate().unwrap();
        let expected = SourceProvenance::new("synthetic ALS application note")
            .with_revision("1.0")
            .with_locator("Table 1")
            .with_url("https://example.invalid/als")
            .with_note("gain/IT matrix");
        assert_eq!(validated.families()[0].provenance(), &expected);
        assert_eq!(validated.families()[0].members()[0].provenance(), &expected);
        let gap = &validated.gaps()["white_channel"];
        assert_eq!(gap.reason, "counts only; no conversion");
        assert_eq!(
            gap.resolved_provenance().unwrap(),
            Some(
                SourceProvenance::new("synthetic ALS application note")
                    .with_locator("§9 white channel")
            )
        );
        assert_eq!(
            gap.provenance().and_then(|overlay| overlay.locator.clone()),
            Some("§9 white channel".into())
        );
    }

    #[test]
    fn policy_is_independently_inspectable_from_provenance() {
        let toml = r#"
[transfer_families.tight]
provenance = { identity = "shared datasheet" }
input_unit = "count"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
max_knots = 8
below = "error"
above = "error"
formula = "x"
selector_axes = { gain = ["div4"] }

[[transfer_families.tight.members]]
selectors = { gain = "div4" }
status = "emit"
applicability = { observation = [1, 10] }

[transfer_families.loose]
provenance = { identity = "shared datasheet" }
input_unit = "count"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 50
max_knots = 64
below = "clamp"
above = "clamp"
saturation = { code = 65535, behavior = "error" }
formula = "x"
selector_axes = { gain = ["div4"] }

[[transfer_families.loose.members]]
selectors = { gain = "div4" }
status = "emit"
applicability = { observation = [1, 10] }
"#;
        let validated = DefinitionsFile::from_toml_str(toml)
            .unwrap()
            .validate()
            .unwrap();
        let tight = &validated.families()[1];
        let loose = &validated.families()[0];
        // BTreeMap iteration is name order: loose, tight
        let (loose, tight) = if loose.name() == "loose" {
            (loose, tight)
        } else {
            (tight, loose)
        };
        assert_eq!(tight.provenance(), loose.provenance());
        assert_ne!(tight.policy(), loose.policy());
        assert_eq!(tight.policy().max_knots, 8);
        assert_eq!(loose.policy().max_knots, 64);
        assert!(loose.policy().observation_guard.is_some());
        assert!(tight.policy().observation_guard.is_none());
    }

    fn guarded_family_toml() -> &'static str {
        r#"
[transfer_families.sensor]
provenance = { identity = "family source", revision = "A", locator = "transfer table" }
input_unit = "count"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
max_knots = 8
saturation = { code = 65535, behavior = "error", provenance = { locator = "guard table" } }
formula = "x"
selector_axes = { range = ["low"] }

[[transfer_families.sensor.members]]
selectors = { range = "low" }
status = "emit"
applicability = { observation = [1, 10] }
provenance = { identity = "member source", revision = "B", locator = "member table" }
"#
    }

    #[test]
    fn family_guard_provenance_stays_resolved_against_the_family() {
        let mut validated = DefinitionsFile::from_toml_str(guarded_family_toml())
            .unwrap()
            .validate()
            .unwrap();
        let family = &validated.families()[0];
        assert_eq!(
            family
                .observation_guard_provenance()
                .map(|provenance| provenance.identity.as_str()),
            Some("family source")
        );
        assert_eq!(
            family
                .observation_guard_provenance()
                .and_then(|provenance| provenance.locator.as_deref()),
            Some("guard table")
        );
        assert_eq!(family.members()[0].provenance().identity, "member source");

        let out = validated
            .generate(&GenerateOptions::transfers_only())
            .unwrap();
        assert!(out.contains(
            r#"Classification of this code as saturation is cited from identity "family source"; revision "A"; locator "guard table""#
        ));
        assert!(out.contains(
            r#"Source provenance: identity "member source"; revision "B"; locator "member table"."#
        ));
        assert!(!out.contains(
            r#"Classification of this code as saturation is cited from identity "member source""#
        ));

        validated
            .set_source(
                "sensor_range_low",
                TransferSource::evaluated_truth(1, (1..=10).map(f64::from).collect())
                    .with_provenance(SourceProvenance::new("overlay source")),
            )
            .unwrap();
        let overlaid_member = &validated.families()[0].members()[0];
        assert_eq!(overlaid_member.provenance().identity, "overlay source");
        assert_eq!(
            overlaid_member
                .provenance_override()
                .and_then(|provenance| provenance.identity.as_deref()),
            Some("member source")
        );
        let overlay_out = validated
            .generate(&GenerateOptions::transfers_only())
            .unwrap();
        assert!(overlay_out.contains(
            r#"Classification of this code as saturation is cited from identity "family source"; revision "A"; locator "guard table""#
        ));
        assert!(
            overlay_out.contains(r#"Source provenance: identity "overlay source"."#),
            "{overlay_out}"
        );
        assert!(!overlay_out.contains(
            r#"Classification of this code as saturation is cited from identity "overlay source""#
        ));

        validated
            .set_source(
                "sensor_range_low",
                TransferSource::evaluated_truth(1, (1..=10).map(f64::from).collect())
                    .inherit_provenance(),
            )
            .unwrap();
        let inherited_member = &validated.families()[0].members()[0];
        assert_eq!(inherited_member.provenance().identity, "member source");
        assert_eq!(
            inherited_member
                .provenance_override()
                .and_then(|provenance| provenance.identity.as_deref()),
            Some("member source")
        );
        let inherited_out = validated
            .generate(&GenerateOptions::transfers_only())
            .unwrap();
        assert!(inherited_out.contains(
            r#"Source provenance: identity "member source"; revision "B"; locator "member table"."#
        ));
        assert!(!inherited_out.contains(r#"Source provenance: identity "overlay source"."#));
    }

    #[test]
    fn observation_guard_policy_excludes_citation_identity() {
        let first = DefinitionsFile::from_toml_str(guarded_family_toml())
            .unwrap()
            .validate()
            .unwrap();
        let second_toml = guarded_family_toml()
            .replace("family source", "another family source")
            .replace("guard table", "another guard table");
        let second = DefinitionsFile::from_toml_str(&second_toml)
            .unwrap()
            .validate()
            .unwrap();

        assert_eq!(first.families()[0].policy(), second.families()[0].policy());
        assert_ne!(
            first.families()[0].observation_guard_provenance(),
            second.families()[0].observation_guard_provenance()
        );
        assert_eq!(
            first.families()[0]
                .policy()
                .observation_guard
                .as_ref()
                .map(|guard| (guard.code, guard.behavior)),
            Some((65_535, ObservationGuardBehaviorDef::Error))
        );
    }

    #[test]
    fn member_provenance_can_explicitly_clear_inherited_fields() {
        let toml = r#"
[transfer_families.als]
provenance = { identity = "datasheet", revision = "1.0", locator = "Table 1", url = "https://example.invalid", note = "family note" }
input_unit = "count"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
formula = "x"
selector_axes = { gain = ["div4"] }

[[transfer_families.als.members]]
selectors = { gain = "div4" }
status = "emit"
applicability = { observation = [1, 10] }
provenance = { locator = "Table 2", clear = ["url", "note"] }
"#;
        let validated = DefinitionsFile::from_toml_str(toml)
            .unwrap()
            .validate()
            .unwrap();
        let citation = validated.families()[0].members()[0].provenance();
        assert_eq!(citation.identity, "datasheet");
        assert_eq!(citation.revision.as_deref(), Some("1.0"));
        assert_eq!(citation.locator.as_deref(), Some("Table 2"));
        assert_eq!(citation.url, None);
        assert_eq!(citation.note, None);
    }

    #[test]
    fn member_provenance_rejects_setting_and_clearing_one_field() {
        let toml = family_toml().replace(
            "status = \"emit\"\napplicability = { observation = [1, 10] }",
            "status = \"emit\"\napplicability = { observation = [1, 10] }\nprovenance = { locator = \"Table 2\", clear = [\"locator\"] }",
        );
        let error = DefinitionsFile::from_toml_str(&toml)
            .unwrap()
            .validate()
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("provenance.locator cannot be both set and cleared"),
            "{error}"
        );
    }

    #[test]
    fn transfer_spec_provenance_matches_standalone_toml() {
        let citation = SourceProvenance::new("bench notes").with_locator("row 3");
        let toml = r#"
[transfers]
requires = ["source_provenance_v1"]

[transfers.linear]
input_unit = "count"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
max_knots = 8
provenance = { identity = "bench notes", locator = "row 3" }
points = [
  { input = 0, output = 0.0 },
  { input = 10, output = 10.0 },
]
"#;
        let from_toml = DefinitionsFile::from_toml_str(toml).unwrap();
        assert_eq!(
            from_toml.transfers()["linear"].provenance(),
            Some(&citation)
        );

        let mut programmatic = DefinitionsFile::default();
        programmatic
            .insert_transfer(
                TransferSpec::new(
                    "linear",
                    "count",
                    "unit",
                    1,
                    1,
                    TransferSource::points(vec![
                        PhysicalPoint::new(0, 0.0),
                        PhysicalPoint::new(10, 10.0),
                    ]),
                )
                .with_max_knots(8)
                .with_provenance(citation.clone()),
            )
            .unwrap();
        assert_eq!(
            programmatic.transfers()["linear"].provenance(),
            Some(&citation)
        );

        let toml_out =
            crate::r#gen::generate(&from_toml, &GenerateOptions::transfers_only()).unwrap();
        let spec_out =
            crate::r#gen::generate(&programmatic, &GenerateOptions::transfers_only()).unwrap();
        assert!(
            toml_out.contains(r#"Source provenance: identity "bench notes"; locator "row 3"."#)
        );
        assert_eq!(toml_out, spec_out);
    }

    #[test]
    fn gap_provenance_without_identity_fails_validation() {
        let toml = r#"
[transfer_families.als]
provenance = { identity = "datasheet" }
input_unit = "count"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
formula = "x"

[[transfer_families.als.members]]
selectors = { gain = "div4" }
status = "emit"
applicability = { observation = [1, 10] }

[gaps.white_channel]
status = "undefined"
reason = "counts only"
provenance = { locator = "§9" }
"#;
        let error = DefinitionsFile::from_toml_str(toml)
            .unwrap()
            .validate()
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("gap `white_channel`")
                && error.contains("source-backed citation requires provenance.identity"),
            "{error}"
        );
    }

    fn formula_member(gain: &str, status: MemberStatus, reason: Option<&str>) -> FamilyMemberDef {
        FamilyMemberDef {
            selectors: BTreeMap::from([("gain".into(), SelectorValue::String(gain.into()))]),
            input_transform: None,
            status,
            reason: reason.map(str::to_string),
            applicability: ApplicabilityDef {
                observation: Some([1, 10]),
                model_input: None,
                physical: None,
            },
            provenance: None,
            emitted_name: None,
        }
    }

    fn formula_family_toml() -> &'static str {
        r#"
[transfer_families.als]
provenance = { identity = "synthetic ALS application note", revision = "1.0", locator = "Table 1" }
input_unit = "count"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
max_knots = 8
below = "error"
above = "clamp"
saturation = { code = 65535, behavior = "error", provenance = { locator = "guard table" } }
formula = "x"
selector_axes = { gain = ["div4", "x1", "idle"] }

[[transfer_families.als.members]]
selectors = { gain = "div4" }
status = "emit"
applicability = { observation = [1, 10] }

[[transfer_families.als.members]]
selectors = { gain = "x1" }
status = "unnecessary"
reason = "documented spare gain"
applicability = { observation = [1, 10] }
provenance = { locator = "member table" }

[[transfer_families.als.gaps]]
selectors = { gain = "idle" }
status = "undefined"
reason = "not characterized"
provenance = { identity = "errata sheet" }
"#
    }

    fn formula_family_spec() -> FamilySpec {
        FamilySpec::new(
            "als",
            "count",
            "unit",
            1,
            1,
            FamilySource::formula("x"),
            SourceProvenance::new("synthetic ALS application note")
                .with_revision("1.0")
                .with_locator("Table 1"),
        )
        .with_max_knots(8)
        .with_boundaries(
            crate::r#gen::BoundaryDef::Error,
            crate::r#gen::BoundaryDef::Clamp,
        )
        .with_observation_guard(ObservationGuardDef {
            code: 65_535,
            behavior: ObservationGuardBehaviorDef::Error,
            provenance: Some(SourceProvenanceOverride {
                locator: Some("guard table".into()),
                ..SourceProvenanceOverride::default()
            }),
        })
        .with_selector_axes(BTreeMap::from([(
            "gain".into(),
            vec![
                SelectorValue::String("div4".into()),
                SelectorValue::String("x1".into()),
                SelectorValue::String("idle".into()),
            ],
        )]))
        .with_members(vec![
            formula_member("div4", MemberStatus::Emit, None),
            FamilyMemberDef {
                provenance: Some(SourceProvenanceOverride {
                    locator: Some("member table".into()),
                    ..SourceProvenanceOverride::default()
                }),
                ..formula_member(
                    "x1",
                    MemberStatus::Unnecessary,
                    Some("documented spare gain"),
                )
            },
        ])
        .with_gaps(vec![FamilyGapDef {
            selectors: BTreeMap::from([("gain".into(), SelectorValue::String("idle".into()))]),
            status: GapStatus::Undefined,
            reason: "not characterized".into(),
            provenance: Some(SourceProvenanceOverride::new("errata sheet")),
        }])
    }

    #[test]
    fn programmatic_family_matches_toml_validated_ir_and_generated_bytes() {
        let from_toml = DefinitionsFile::from_toml_str(formula_family_toml())
            .unwrap()
            .validate()
            .unwrap();
        let mut programmatic = DefinitionsFile::default();
        programmatic.insert_family(formula_family_spec()).unwrap();
        let from_spec = programmatic.validate().unwrap();

        let toml_family = &from_toml.families()[0];
        let spec_family = &from_spec.families()[0];
        assert_eq!(toml_family.name(), spec_family.name());
        assert_eq!(toml_family.input_unit(), spec_family.input_unit());
        assert_eq!(toml_family.output_unit(), spec_family.output_unit());
        assert_eq!(toml_family.output_scale(), spec_family.output_scale());
        assert_eq!(toml_family.declared_source(), spec_family.declared_source());
        assert_eq!(toml_family.formula(), spec_family.formula());
        assert_eq!(toml_family.policy(), spec_family.policy());
        assert_eq!(toml_family.provenance(), spec_family.provenance());
        assert_eq!(
            toml_family.observation_guard_provenance(),
            spec_family.observation_guard_provenance()
        );
        assert_eq!(toml_family.members().len(), spec_family.members().len());
        for (left, right) in toml_family.members().iter().zip(spec_family.members()) {
            assert_eq!(left.selectors(), right.selectors());
            assert_eq!(left.status(), right.status());
            assert_eq!(left.reason(), right.reason());
            assert_eq!(left.observation_domain(), right.observation_domain());
            assert_eq!(left.emitted_name(), right.emitted_name());
            assert_eq!(left.provenance(), right.provenance());
            assert_eq!(left.provenance_override(), right.provenance_override());
        }
        assert_eq!(toml_family.gaps().len(), spec_family.gaps().len());
        for (left, right) in toml_family.gaps().iter().zip(spec_family.gaps()) {
            assert_eq!(left.selectors(), right.selectors());
            assert_eq!(left.reason(), right.reason());
            assert_eq!(left.provenance(), right.provenance());
        }

        let opts = GenerateOptions::transfers_only();
        assert_eq!(
            from_toml.generate(&opts).unwrap(),
            from_spec.generate(&opts).unwrap()
        );
    }

    #[test]
    fn programmatic_points_and_scaled_polynomial_families_match_toml() {
        let points_toml = r#"
[transfer_families.front_end]
provenance = { identity = "test fixture" }
input_unit = "count"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
max_knots = 8
points = [
  { input = 0, output = 0.0 },
  { input = 5, output = 5.0 },
  { input = 10, output = 10.0 },
]
selector_axes = { range = ["low"] }

[[transfer_families.front_end.members]]
selectors = { range = "low" }
status = "emit"
applicability = { observation = [0, 10] }
"#;
        let mut programmatic = DefinitionsFile::default();
        programmatic
            .insert_family(
                FamilySpec::new(
                    "front_end",
                    "count",
                    "unit",
                    1,
                    1,
                    FamilySource::points(vec![
                        PhysicalPoint::new(0, 0.0),
                        PhysicalPoint::new(5, 5.0),
                        PhysicalPoint::new(10, 10.0),
                    ]),
                    SourceProvenance::new("test fixture"),
                )
                .with_max_knots(8)
                .with_selector_axes(BTreeMap::from([(
                    "range".into(),
                    vec![SelectorValue::String("low".into())],
                )]))
                .with_members(vec![FamilyMemberDef {
                    selectors: BTreeMap::from([(
                        "range".into(),
                        SelectorValue::String("low".into()),
                    )]),
                    input_transform: None,
                    status: MemberStatus::Emit,
                    reason: None,
                    applicability: ApplicabilityDef {
                        observation: Some([0, 10]),
                        model_input: None,
                        physical: None,
                    },
                    provenance: None,
                    emitted_name: None,
                }]),
            )
            .unwrap();
        let from_toml = DefinitionsFile::from_toml_str(points_toml)
            .unwrap()
            .validate()
            .unwrap();
        let from_spec = programmatic.validate().unwrap();
        assert_eq!(
            from_toml.families()[0].declared_source(),
            DeclaredSource::Points
        );
        assert_eq!(
            from_spec.families()[0].members()[0].observation_domain(),
            from_toml.families()[0].members()[0].observation_domain()
        );
        let opts = GenerateOptions::transfers_only();
        assert_eq!(
            from_toml.generate(&opts).unwrap(),
            from_spec.generate(&opts).unwrap()
        );

        let poly_toml = r#"
[transfer_families.als]
provenance = { identity = "test fixture" }
input_unit = "count"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
max_knots = 8
selector_axes = { gain = ["div4"] }

[transfer_families.als.model]
kind = "scaled_polynomial"
coefficients = [0.0, 1.0]

[[transfer_families.als.members]]
selectors = { gain = "div4" }
status = "emit"
input_transform = { numerator = 33600, denominator = 1000000 }
applicability = { model_input = [100.0, 22000.0] }
"#;
        let mut poly_spec = DefinitionsFile::default();
        poly_spec
            .insert_family(
                FamilySpec::new(
                    "als",
                    "count",
                    "unit",
                    1,
                    1,
                    FamilySource::scaled_polynomial(vec![0.0, 1.0]),
                    SourceProvenance::new("test fixture"),
                )
                .with_max_knots(8)
                .with_selector_axes(BTreeMap::from([(
                    "gain".into(),
                    vec![SelectorValue::String("div4".into())],
                )]))
                .with_members(vec![FamilyMemberDef {
                    selectors: BTreeMap::from([(
                        "gain".into(),
                        SelectorValue::String("div4".into()),
                    )]),
                    input_transform: Some(InputTransform {
                        numerator: 33_600,
                        denominator: 1_000_000,
                    }),
                    status: MemberStatus::Emit,
                    reason: None,
                    applicability: ApplicabilityDef {
                        observation: None,
                        model_input: Some([100.0, 22_000.0]),
                        physical: None,
                    },
                    provenance: None,
                    emitted_name: None,
                }]),
            )
            .unwrap();
        let poly_toml_ir = DefinitionsFile::from_toml_str(poly_toml)
            .unwrap()
            .validate()
            .unwrap();
        let poly_spec_ir = poly_spec.validate().unwrap();
        assert_eq!(
            poly_toml_ir.families()[0].declared_source(),
            DeclaredSource::Model
        );
        assert!(poly_spec_ir.families()[0].has_model());
        assert_eq!(
            poly_toml_ir.generate(&opts).unwrap(),
            poly_spec_ir.generate(&opts).unwrap()
        );
    }

    #[test]
    fn programmatic_ntc_family_validates_without_toml() {
        let mut defs = DefinitionsFile::default();
        defs.insert_family(
            FamilySpec::new(
                "ntc",
                "adc_code",
                "degree_celsius",
                1000,
                50,
                FamilySource::ntc_beta_divider(
                    10_000.0,
                    3950.0,
                    25.0,
                    10_000.0,
                    4095,
                    DividerTopology::NtcToGround,
                ),
                SourceProvenance::new("test fixture"),
            )
            .with_selector_axes(BTreeMap::from([(
                "probe".into(),
                vec![SelectorValue::String("wide".into())],
            )]))
            .with_members(vec![FamilyMemberDef {
                selectors: BTreeMap::from([("probe".into(), SelectorValue::String("wide".into()))]),
                input_transform: None,
                status: MemberStatus::Emit,
                reason: None,
                applicability: ApplicabilityDef {
                    observation: None,
                    model_input: None,
                    physical: Some([-20.0, 80.0]),
                },
                provenance: None,
                emitted_name: None,
            }]),
        )
        .unwrap();
        let validated = defs.validate().unwrap();
        let family = &validated.families()[0];
        assert_eq!(family.declared_source(), DeclaredSource::Model);
        assert!(family.has_model());
        assert!(family.members()[0].observation_domain().is_some());
        assert!(
            validated
                .generate(&GenerateOptions::transfers_only())
                .unwrap()
                .contains("pub const NTC_PROBE_WIDE:")
        );
    }

    #[test]
    fn explicit_emitted_name_survives_selector_display_rename() {
        let mut renamed = DefinitionsFile::default();
        renamed
            .insert_family(
                FamilySpec::new(
                    "als",
                    "count",
                    "unit",
                    1,
                    1,
                    FamilySource::formula("x"),
                    SourceProvenance::new("test fixture"),
                )
                .with_max_knots(8)
                .with_selector_axes(BTreeMap::from([(
                    "gain".into(),
                    vec![SelectorValue::String("x4".into())],
                )]))
                .with_members(vec![
                    formula_member("x4", MemberStatus::Emit, None)
                        .with_emitted_name("als_gain_div4"),
                ]),
            )
            .unwrap();
        let validated = renamed.validate().unwrap();
        let member = &validated.families()[0].members()[0];
        assert_eq!(member.expanded_name(), "als_gain_x4");
        assert_eq!(member.explicit_emitted_name(), Some("als_gain_div4"));
        assert_eq!(member.emitted_name(), "als_gain_div4");
        let manifest = validated.emission_manifest();
        let entry = &manifest.entries()[0];
        assert_eq!(entry.table_name, "als_gain_div4");
        assert_eq!(entry.symbol, "ALS_GAIN_DIV4");
        let out = validated
            .generate(&GenerateOptions::transfers_only())
            .unwrap();
        assert!(out.contains("pub const ALS_GAIN_DIV4:"));
        assert!(!out.contains("pub const ALS_GAIN_X4:"));
        assert!(out.contains(r#"Family: "als"."#));
        assert!(out.contains(r#"Selectors: "gain" = "x4"."#));
    }

    #[test]
    fn derived_naming_is_independent_of_selector_declaration_order() {
        let mut first = DefinitionsFile::default();
        first
            .insert_family(
                FamilySpec::new(
                    "als",
                    "count",
                    "unit",
                    1,
                    1,
                    FamilySource::formula("x"),
                    SourceProvenance::new("test fixture"),
                )
                .with_max_knots(8)
                .with_expected_selectors(vec![BTreeMap::from([
                    ("gain".into(), SelectorValue::String("div4".into())),
                    ("it".into(), SelectorValue::Integer(100)),
                ])])
                .with_members(vec![FamilyMemberDef {
                    selectors: BTreeMap::from([
                        ("it".into(), SelectorValue::Integer(100)),
                        ("gain".into(), SelectorValue::String("div4".into())),
                    ]),
                    input_transform: None,
                    status: MemberStatus::Emit,
                    reason: None,
                    applicability: ApplicabilityDef {
                        observation: Some([1, 10]),
                        model_input: None,
                        physical: None,
                    },
                    provenance: None,
                    emitted_name: None,
                }]),
            )
            .unwrap();
        let validated = first.validate().unwrap();
        assert_eq!(
            validated.families()[0].members()[0].emitted_name(),
            "als_gain_div4_it_100"
        );
    }

    #[test]
    fn explicit_emitted_names_share_collision_rules_with_derived_stems() {
        let mut colliding = DefinitionsFile::default();
        let error = colliding
            .insert_family(
                FamilySpec::new(
                    "als",
                    "count",
                    "unit",
                    1,
                    1,
                    FamilySource::formula("x"),
                    SourceProvenance::new("test fixture"),
                )
                .with_max_knots(8)
                .with_selector_axes(BTreeMap::from([(
                    "gain".into(),
                    vec![
                        SelectorValue::String("div4".into()),
                        SelectorValue::String("x1".into()),
                    ],
                )]))
                .with_members(vec![
                    formula_member("div4", MemberStatus::Emit, None),
                    formula_member("x1", MemberStatus::Emit, None)
                        .with_emitted_name("als_gain_div4"),
                ]),
            )
            .ok()
            .and_then(|()| colliding.validate().err())
            .unwrap()
            .to_string();
        assert!(error.contains("both expand to `als_gain_div4`"), "{error}");

        let mut blank = DefinitionsFile::default();
        let error = blank
            .insert_family(
                FamilySpec::new(
                    "als",
                    "count",
                    "unit",
                    1,
                    1,
                    FamilySource::formula("x"),
                    SourceProvenance::new("test fixture"),
                )
                .with_max_knots(8)
                .with_selector_axes(BTreeMap::from([(
                    "gain".into(),
                    vec![SelectorValue::String("div4".into())],
                )]))
                .with_members(vec![
                    formula_member("div4", MemberStatus::Emit, None).with_emitted_name("   "),
                ]),
            )
            .ok()
            .and_then(|()| blank.validate().err())
            .unwrap()
            .to_string();
        assert!(error.contains("emitted_name must not be blank"), "{error}");
    }

    #[test]
    fn description_only_members_and_gaps_are_absent_from_the_manifest() {
        let validated = DefinitionsFile::from_toml_str(formula_family_toml())
            .unwrap()
            .validate()
            .unwrap();
        let family = &validated.families()[0];
        assert_eq!(family.members()[1].status(), MemberStatus::Unnecessary);
        assert!(family.gaps()[0].reason().contains("not characterized"));
        let names: Vec<_> = validated
            .emission_manifest()
            .entries()
            .iter()
            .map(|entry| entry.table_name.clone())
            .collect();
        assert_eq!(names, ["als_gain_div4"]);
        let report = validated
            .generate_report(&GenerateOptions::transfers_only())
            .unwrap()
            .report;
        let member = &report.families[0].members[1];
        assert!(member.symbol.is_none());
        assert!(member.metadata_symbol.is_none());
        assert!(member.observation_guard_symbol.is_none());
        let emitted = &report.families[0].members[0];
        assert_eq!(emitted.symbol.as_deref(), Some("ALS_GAIN_DIV4"));
        assert_eq!(
            emitted.metadata_symbol.as_deref(),
            Some("ALS_GAIN_DIV4_METADATA")
        );
        assert_eq!(
            emitted.observation_guard_symbol.as_deref(),
            Some("ALS_GAIN_DIV4_OBSERVATION_GUARD")
        );
    }

    #[test]
    fn validated_insert_family_rebuilds_the_graph() {
        let mut validated = DefinitionsFile::default().validate().unwrap();
        validated.insert_family(formula_family_spec()).unwrap();
        assert_eq!(validated.families().len(), 1);
        assert_eq!(validated.emission_manifest().entries().len(), 1);
    }

    #[test]
    fn standalone_transfer_spec_remains_supported_alongside_families() {
        let mut defs = DefinitionsFile::default();
        defs.insert_family(formula_family_spec()).unwrap();
        defs.insert_transfer(
            TransferSpec::new(
                "linear",
                "count",
                "unit",
                1,
                1,
                TransferSource::points(vec![
                    PhysicalPoint::new(0, 0.0),
                    PhysicalPoint::new(10, 10.0),
                ]),
            )
            .with_max_knots(8),
        )
        .unwrap();
        let out = crate::r#gen::generate(&defs, &GenerateOptions::transfers_only()).unwrap();
        assert!(out.contains("pub const ALS_GAIN_DIV4:"));
        assert!(out.contains("pub const LINEAR:"));
    }
}
