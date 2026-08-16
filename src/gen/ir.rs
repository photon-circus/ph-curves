//! Validated host IR: inspect families and overlay generation sources.

// Host-only: module-local std link (crate root stays `#![no_std]`).
extern crate std;

use std::collections::{BTreeMap, BTreeSet};
use std::format;
use std::prelude::v1::*;

use super::api::{Error, GenerateOptions};
use super::curve::DefinitionsFile;
use super::transfer::family::expanded_name;
use super::transfer::{
    ApplicabilityDef, GapDef, InputTransform, MemberStatus, SelectorValue, TransferFamilyDef,
    TransferSource, TransferSpec, family_source_observation_domain, overlay_observation_span,
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

/// One validated family, including description-only members.
#[derive(Clone, Debug)]
pub struct ValidatedFamily {
    name: String,
    members: Vec<ValidatedMember>,
}

impl ValidatedFamily {
    /// Family table name from the definitions document.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Every member, in declaration order. Non-`emit` members are present.
    pub fn members(&self) -> &[ValidatedMember] {
        &self.members
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
}

impl DefinitionsFile {
    /// Validate families, gaps, and name collisions without generating Rust.
    ///
    /// Every family member is checked before non-`emit` statuses are filtered.
    /// Description-only members remain inspectable on the returned graph.
    pub fn validate(&self) -> Result<ValidatedDefinitions, Error> {
        let resolved = self.resolved_transfers().map_err(Error::Validation)?;
        let curves: Vec<_> = self.curves.iter().collect();
        let transfers: Vec<_> = resolved.iter().collect();
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
        self.overlays.insert(name, source);
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
        let mut members = Vec::new();
        for member in &family.members {
            members.push(ValidatedMember {
                selectors: member.selectors.clone(),
                input_transform: member.input_transform,
                status: member.status,
                reason: member.reason.clone(),
                applicability: member.applicability.clone(),
                expanded_name: expanded_name(family_name, &member.selectors)?,
            });
        }
        out.push(ValidatedFamily {
            name: family_name.clone(),
            members,
        });
    }
    Ok(out)
}

fn family_overlay_domains(
    families: &[ValidatedFamily],
    resolved: &BTreeMap<String, super::transfer::TransferDef>,
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
            let domain = family_source_observation_domain(&member.expanded_name, def)?;
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

    /// Declared gaps. A missing gap is not the same as an undefined one.
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
    /// different observation domain.
    pub fn set_source(&mut self, name: &str, source: TransferSource) -> Result<(), Error> {
        if self.resolved_names.contains(name) {
            if let Some(expected) = self.family_overlay_domains.get(name) {
                let span = overlay_observation_span(name, &source).map_err(Error::Validation)?;
                if span != *expected {
                    return Err(Error::Validation(format!(
                        "transfer `{name}`: overlay observation domain [{}, {}] must equal \
                         resolved member observation domain [{}, {}]",
                        span[0], span[1], expected[0], expected[1]
                    )));
                }
            }
            self.defs.overlays.insert(name.to_string(), source);
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
        GenerateOptions, MemberStatus, ObservationGuardBehaviorDef, ObservationGuardDef,
        PhysicalPoint, SelectorValue, TransferSource, TransferSpec,
    };
    use std::vec;

    fn family_toml() -> &'static str {
        r#"
[transfer_families.als]
input_unit = "count"
output_unit = "unit"
output_scale = 1000
max_interpolation_error = 50
formula = "x"

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
    }

    #[test]
    fn overlay_evaluated_truth_emits_ordinary_transfer() {
        let defs = DefinitionsFile::from_toml_str(family_toml()).unwrap();
        let mut validated = defs.validate().unwrap();
        let physical: Vec<f64> = (1..=10).map(f64::from).collect();
        validated
            .set_source(
                "als_gain_div4_integration_time_ms_100",
                TransferSource::evaluated_truth(1, physical),
            )
            .unwrap();

        let description_only = validated.set_source(
            "als_gain_x1_integration_time_ms_100",
            TransferSource::evaluated_truth(1, vec![1.0, 2.0]),
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
                TransferSource::evaluated_truth(2, vec![2.0, 3.0, 4.0]),
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
                input_unit = "count"
                output_unit = "unit"
                output_scale = 1
                max_interpolation_error = 1
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
                TransferSource::evaluated_truth(2, (2..=9).map(f64::from).collect()),
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
                TransferSource::evaluated_truth(5, (5..=8).map(f64::from).collect()),
            )
            .unwrap();
    }

    #[test]
    fn ntc_family_overlay_uses_derived_observation_domain() {
        let defs = DefinitionsFile::from_toml_str(
            r#"
                [transfer_families.ntc]
                input_unit = "adc_code"
                output_unit = "degree_celsius"
                output_scale = 1000
                max_interpolation_error = 50

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
                TransferSource::evaluated_truth(expected[0] + 1, vec![0.0; wrong_len]),
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
                TransferSource::evaluated_truth(expected[0], vec![0.0; matching_len]),
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
                TransferSource::evaluated_truth(20, vec![20.0, 21.0, 22.0]),
            )
            .unwrap();

        let out = validated
            .generate(&GenerateOptions::transfers_only())
            .unwrap();
        assert!(out.contains("domain_min: 20"), "{out}");
        assert!(out.contains("domain_max: 22"), "{out}");
        assert!(out.contains("evaluated physical truth"), "{out}");
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
                TransferSource::evaluated_truth(30, vec![30.0, 31.0, 32.0]),
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
                TransferSource::prefitted_knots_verified(vec![1, 10], vec![1000, 10_000], truth),
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
                ]),
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
}
