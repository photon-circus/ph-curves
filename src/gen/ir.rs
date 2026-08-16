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
    expanded_name, resolved_family_gap_provenance, resolved_member_provenance,
    resolved_selector_universe,
};
use super::transfer::{
    ApplicabilityDef, FamilyCompleteness, GapDef, GapStatus, GenerationPolicy, InputTransform,
    MemberStatus, SelectorUniverse, SelectorValue, SourceProvenance, SourceProvenanceDisposition,
    SourceProvenanceOverride, TransferFamilyDef, TransferSourceOverlay, TransferSpec,
    family_source_observation_domain, overlay_observation_span, resolve_guard_provenance,
};

/// Family, member, and gap graph after identity and collision checks.
///
/// Validation does not fit knots or emit Rust. Overlay generation sources
/// with [`Self::set_source`] or [`Self::insert_transfer`], then [`Self::generate`].
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

    /// Candidate expanded transfer name.
    ///
    /// Reserved for codegen only when [`Self::status`] is [`MemberStatus::Emit`].
    pub fn expanded_name(&self) -> &str {
        &self.expanded_name
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
            let def = resolved.get(&member.expanded_name).ok_or_else(|| {
                format!(
                    "transfer family `{}`: emitted member `{}` was not expanded",
                    family.name, member.expanded_name
                )
            })?;
            let domain = family_source_observation_domain(&member.expanded_name, &def.def)?;
            domains.insert(member.expanded_name.clone(), domain);
        }
    }
    Ok(domains)
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
                    .find(|member| member.expanded_name == name)
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
            family
                .members
                .iter()
                .any(|member| member.expanded_name == name && member.status != MemberStatus::Emit)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::r#gen::{
        FamilyCompleteness, GenerateOptions, MemberStatus, ObservationGuardBehaviorDef,
        ObservationGuardDef, PhysicalPoint, SelectorUniverse, SelectorValue, SourceProvenance,
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

        assert_eq!(family.members()[1].status(), MemberStatus::Unnecessary);
        assert_eq!(
            family.members()[1].reason(),
            Some("high-gain row is documented, not generated")
        );
        assert_eq!(
            family.members()[1].expanded_name(),
            "als_gain_x1_integration_time_ms_100"
        );
        assert_eq!(family.members()[2].status(), MemberStatus::Forbidden);

        assert_eq!(
            validated.gaps()["white_channel"].reason,
            "counts only; no conversion"
        );
        let emitted: Vec<_> = validated.emitted_transfer_names().collect();
        assert_eq!(emitted, ["als_gain_div4_integration_time_ms_100"]);
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
}
