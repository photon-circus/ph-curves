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
    ApplicabilityDef, GapDef, MemberStatus, SelectorValue, TransferFamilyDef, TransferSource,
    TransferSpec,
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
    scale: u32,
    status: MemberStatus,
    applicability: ApplicabilityDef,
    expanded_name: String,
}

impl ValidatedMember {
    /// Selector map; keys, value types, and values are the identity.
    pub fn selectors(&self) -> &BTreeMap<String, SelectorValue> {
        &self.selectors
    }

    /// Per-member model-input scale. Not applied to truth by this crate.
    pub fn scale(&self) -> u32 {
        self.scale
    }

    /// Whether this member is generated.
    pub fn status(&self) -> MemberStatus {
        self.status
    }

    /// Source-backed window in the model's input units.
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
        if !self.curves.is_empty() && !self.transfer_families.is_empty() {
            return Err(Error::Validation(
                "transfer families forbid [curves] in the same document (dense LUT path is not allowed)"
                    .into(),
            ));
        }

        let resolved = self.resolved_transfers().map_err(Error::Validation)?;
        let families = inspect_families(&self.transfer_families).map_err(Error::Validation)?;
        let resolved_names = resolved.keys().cloned().collect();

        Ok(ValidatedDefinitions {
            defs: self.clone(),
            families,
            resolved_names,
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
                scale: member.scale,
                status: member.status,
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
    /// the graph stay distinct from generation input.
    pub fn set_source(&mut self, name: &str, source: TransferSource) -> Result<(), Error> {
        if self.resolved_names.contains(name) {
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
        GenerateOptions, MemberStatus, PhysicalPoint, SelectorValue, TransferSource, TransferSpec,
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
domain = [1, 10]
interpolate_selectors = false

[[transfer_families.als.members]]
selectors = { gain = "div4", integration_time_ms = 100 }
scale = 268800
status = "emit"
applicability = { model_input = [100.0, 22000.0] }

[[transfer_families.als.members]]
selectors = { gain = "x1", integration_time_ms = 100 }
scale = 4200
status = "none"
applicability = { model_input = [100.0, 22000.0] }

[[transfer_families.als.members]]
selectors = { gain = "x2", integration_time_ms = 100 }
scale = 2100
status = "do_not_use"
applicability = { model_input = [100.0, 22000.0] }

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
        assert_eq!(emit.scale(), 268_800);
        assert_eq!(
            emit.selectors()["gain"],
            SelectorValue::String("div4".into())
        );
        assert_eq!(
            emit.expanded_name(),
            "als_gain_div4_integration_time_ms_100"
        );

        assert_eq!(family.members()[1].status(), MemberStatus::None);
        assert_eq!(
            family.members()[1].expanded_name(),
            "als_gain_x1_integration_time_ms_100"
        );
        assert_eq!(family.members()[2].status(), MemberStatus::DoNotUse);

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
    fn prefitted_knots_reuse_codegen_without_copying_emit_path() {
        let mut defs = DefinitionsFile::default();
        defs.insert_transfer(
            TransferSpec::new(
                "linear",
                "code",
                "unit",
                1000,
                1,
                TransferSource::prefitted_knots(vec![20, 100], vec![0, 40_000]),
            )
            .with_max_knots(8),
        )
        .unwrap();

        let out = crate::r#gen::generate(&defs, &GenerateOptions::transfers_only()).unwrap();
        assert!(out.contains("pub const LINEAR: PiecewiseLinearTransfer<2>"));
        assert!(out.contains("LINEAR_METADATA"));
        assert!(out.contains("not verified against a source oracle"));
        assert_eq!(out.matches("pub const LINEAR:").count(), 1);
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
            .with_max_knots(8),
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
    }
}
