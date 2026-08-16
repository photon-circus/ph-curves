//! Discrete transfer families: one shared source, explicit members, no interpolation.

// Host-only: module-local std link (crate root stays `#![no_std]`).
extern crate std;

use std::collections::BTreeMap;
use std::format;
use std::prelude::v1::*;

use serde::Deserialize;
use serde::de::{self, Deserializer, Visitor};

use super::model::ModelDef;
use super::{
    BoundaryDef, ObservationGuardDef, PhysicalPoint, TransferDef, default_boundary,
    validate_observation_guard,
};

/// Hard cap on knots for a family member. Stricter than the standalone
/// transfer cap (4096): families must not become dense ADC tables.
pub const FAMILY_MAX_KNOTS_HARD: usize = 256;

fn default_family_max_knots() -> usize {
    64
}

/// How a parsed family or standalone transfer declares its host source.
///
/// Built-in model parameters stay crate-private. Host tools distinguish a
/// model source from formula or points without depending on the NTC catalog.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeclaredSource {
    /// A formula over the observation-domain variable `x`.
    Formula,
    /// Sparse physical control points.
    Points,
    /// A built-in host model (NTC Beta-divider or scaled polynomial).
    Model,
}

/// One named family: shared source, per-member scale and applicability.
///
/// Metadata fields are public. Source and boundary fields are inspectable
/// through accessors so the NTC model catalog is not part of the public IR.
/// Dependents obtain the map via `DefinitionsFile::transfer_families`.
///
/// Scale is per-member only. A family-level `scale` field is rejected as
/// unknown so a shared-model scale cannot be silently overwritten.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransferFamilyDef {
    /// Observation-domain unit label copied onto each emitted transfer.
    pub input_unit: String,
    /// Physical-domain unit label copied onto each emitted transfer.
    pub output_unit: String,
    /// Integer output quanta per physical unit, copied onto each emitted transfer.
    pub output_scale: u32,
    /// Requested interpolation error bound, copied onto each emitted transfer.
    pub max_interpolation_error: u32,
    /// Knot budget for each emitted member (default 64, hard cap 256).
    #[serde(default = "default_family_max_knots")]
    pub max_knots: usize,
    /// Must be `false`. Selector interpolation is never supported.
    #[serde(default)]
    pub interpolate_selectors: bool,
    #[serde(default = "default_boundary")]
    pub(crate) below: BoundaryDef,
    #[serde(default = "default_boundary")]
    pub(crate) above: BoundaryDef,
    /// Explicit observation-code guard copied onto emitted members (TOML `saturation`).
    #[serde(default, rename = "saturation")]
    pub(crate) observation_guard: Option<ObservationGuardDef>,
    pub(crate) points: Option<Vec<PhysicalPoint>>,
    pub(crate) formula: Option<String>,
    pub(crate) model: Option<ModelDef>,
    pub(crate) domain: Option<[u16; 2]>,
    pub(crate) output_range: Option<[f64; 2]>,
    /// Explicit selector combinations. Never synthesized.
    pub members: Vec<FamilyMemberDef>,
}

impl TransferFamilyDef {
    /// Observation-domain below policy copied onto emitted members.
    pub fn below(&self) -> super::BoundaryDef {
        self.below
    }

    /// Observation-domain above policy copied onto emitted members.
    pub fn above(&self) -> super::BoundaryDef {
        self.above
    }

    /// Explicit observation-code guard copied onto emitted members (TOML `saturation`).
    pub fn observation_guard(&self) -> Option<ObservationGuardDef> {
        self.observation_guard
    }

    /// Shared formula text, when the family source is a formula.
    pub fn formula(&self) -> Option<&str> {
        self.formula.as_deref()
    }

    /// Shared physical control points, when the family source is points.
    pub fn points(&self) -> Option<&[super::PhysicalPoint]> {
        self.points.as_deref()
    }

    /// Inclusive observation domain, required for formula sources. Derived from
    /// applicability for scaled-polynomial families.
    pub fn domain(&self) -> Option<[u16; 2]> {
        self.domain
    }

    /// Physical output window, required for output-range models (NTC).
    pub fn output_range(&self) -> Option<[f64; 2]> {
        self.output_range
    }

    /// Whether the shared source is a built-in host model.
    pub fn has_model(&self) -> bool {
        self.model.is_some()
    }

    /// Which of formula, points, or model is set. `None` if the source is missing or mixed.
    pub fn declared_source(&self) -> Option<DeclaredSource> {
        match (
            self.formula.is_some(),
            self.points.is_some(),
            self.model.is_some(),
        ) {
            (true, false, false) => Some(DeclaredSource::Formula),
            (false, true, false) => Some(DeclaredSource::Points),
            (false, false, true) => Some(DeclaredSource::Model),
            _ => None,
        }
    }
}

/// One explicit selector combination. Never synthesized by interpolation.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FamilyMemberDef {
    /// Discrete selector map. Keys and typed values form the member identity.
    pub selectors: BTreeMap<String, SelectorValue>,
    /// Model-input scale for this member. Required and must be nonzero.
    ///
    /// For `kind = "scaled_polynomial"`, expansion applies this scale to host
    /// truth (`u = count * scale / 1e6`) and converts `applicability.model_input`
    /// to an observation-domain window. Formula and points sources leave it
    /// inspectable only.
    pub scale: u32,
    /// Whether this member is generated or description-only.
    pub status: MemberStatus,
    /// Source-backed window in the model's input units.
    pub applicability: ApplicabilityDef,
}

/// Where this member's mapping is source-backed.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicabilityDef {
    /// Inclusive model-input window `[min, max]`. Must be finite and strictly increasing.
    pub model_input: [f64; 2],
}

/// Whether a family member is generated or retained as description only.
#[derive(Debug, Clone, Copy, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MemberStatus {
    /// Generate a sparse `PiecewiseLinearTransfer` for this member.
    Emit,
    /// Recorded on the description; not generated.
    None,
    /// Recorded as forbidden in this region; not generated.
    DoNotUse,
}

/// A string or integer selector token. Other TOML types are rejected.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum SelectorValue {
    /// Integer selector token.
    Integer(i64),
    /// UTF-8 selector token.
    String(String),
}

impl<'de> Deserialize<'de> for SelectorValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct SelectorVisitor;

        impl Visitor<'_> for SelectorVisitor {
            type Value = SelectorValue;

            fn expecting(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                formatter.write_str("a string or integer selector")
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
                Ok(SelectorValue::String(value.to_string()))
            }

            fn visit_string<E: de::Error>(self, value: String) -> Result<Self::Value, E> {
                Ok(SelectorValue::String(value))
            }

            fn visit_i64<E: de::Error>(self, value: i64) -> Result<Self::Value, E> {
                Ok(SelectorValue::Integer(value))
            }

            fn visit_u64<E: de::Error>(self, value: u64) -> Result<Self::Value, E> {
                i64::try_from(value)
                    .map(SelectorValue::Integer)
                    .map_err(|_| E::custom("selector integer does not fit i64"))
            }

            fn visit_bool<E: de::Error>(self, value: bool) -> Result<Self::Value, E> {
                Err(E::custom(format!(
                    "selector must be a string or integer, got boolean {value}"
                )))
            }

            fn visit_f64<E: de::Error>(self, value: f64) -> Result<Self::Value, E> {
                Err(E::custom(format!(
                    "selector must be a string or integer, got float {value}"
                )))
            }
        }

        deserializer.deserialize_any(SelectorVisitor)
    }
}

impl SelectorValue {
    fn token(&self) -> String {
        match self {
            Self::Integer(value) => value.to_string(),
            Self::String(value) => value.clone(),
        }
    }
}

/// A channel or procedure the sources do not define as a transfer.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GapDef {
    /// Must be `"undefined"`: a gap must not be filled with a plausible model.
    pub status: GapStatus,
    /// Non-blank explanation of why the mapping is undefined.
    pub reason: String,
}

/// Only `undefined` is valid: a gap must not be filled with a plausible model.
#[derive(Debug, Clone, Copy, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum GapStatus {
    /// The sources do not define this mapping; do not invent one.
    Undefined,
}

/// Expand families into standalone transfer defs.
///
/// Every member is validated before non-`emit` statuses are filtered.
pub(crate) fn expand_families(
    families: &BTreeMap<String, TransferFamilyDef>,
) -> Result<BTreeMap<String, TransferDef>, String> {
    let mut out = BTreeMap::new();
    for (family_name, family) in families {
        validate_family(family_name, family)?;

        let mut generated = 0usize;
        for member in &family.members {
            if member.status != MemberStatus::Emit {
                continue;
            }
            let member_name = expanded_name(family_name, &member.selectors)?;
            let def = member_transfer(family_name, family, member)?;
            if let Some(_previous) = out.insert(member_name.clone(), def) {
                return Err(format!(
                    "transfer family `{family_name}`: duplicate expanded name `{member_name}` \
                     from selectors {}",
                    format_selectors(&member.selectors)
                ));
            }
            generated += 1;
        }
        if generated == 0 {
            return Err(format!(
                "transfer family `{family_name}`: no members with status = \"emit\""
            ));
        }
    }
    Ok(out)
}

fn validate_family(family_name: &str, family: &TransferFamilyDef) -> Result<(), String> {
    if family.interpolate_selectors {
        return Err(format!(
            "transfer family `{family_name}`: interpolate_selectors = true is not supported; \
             list members explicitly"
        ));
    }
    if !(2..=FAMILY_MAX_KNOTS_HARD).contains(&family.max_knots) {
        return Err(format!(
            "transfer family `{family_name}`: max_knots must be in 2..={FAMILY_MAX_KNOTS_HARD}"
        ));
    }
    let source_count = family.points.is_some() as u8
        + family.formula.is_some() as u8
        + family.model.is_some() as u8;
    if source_count != 1 {
        return Err(format!(
            "transfer family `{family_name}`: exactly one of points, formula, or model must be specified"
        ));
    }
    if family.members.is_empty() {
        return Err(format!(
            "transfer family `{family_name}`: members must not be empty"
        ));
    }
    if let Some(ModelDef::ScaledPolynomial {
        scale,
        coefficients,
        ..
    }) = &family.model
    {
        if scale.is_some() {
            return Err(format!(
                "transfer family `{family_name}`: scale belongs on members, not the shared model"
            ));
        }
        if family.domain.is_some() {
            return Err(format!(
                "transfer family `{family_name}`: domain is derived from applicability for scaled_polynomial"
            ));
        }
        if family.output_range.is_some() {
            return Err(format!(
                "transfer family `{family_name}`: output_range is forbidden for scaled_polynomial"
            ));
        }
        super::model::validate_coefficients(coefficients)
            .map_err(|error| format!("transfer family `{family_name}`: {error}"))?;
    }

    let mut seen_identities: BTreeMap<&BTreeMap<String, SelectorValue>, usize> = BTreeMap::new();
    let mut seen_emitted_names: BTreeMap<String, String> = BTreeMap::new();
    for (index, member) in family.members.iter().enumerate() {
        validate_member(family_name, family, index, member)?;
        if let Some(&previous) = seen_identities.get(&member.selectors) {
            return Err(format!(
                "transfer family `{family_name}`: members {previous} and {index} share selector identity {}",
                format_selectors(&member.selectors)
            ));
        }
        seen_identities.insert(&member.selectors, index);

        if member.status == MemberStatus::Emit {
            let member_name = expanded_name(family_name, &member.selectors)?;
            if let Some(previous) =
                seen_emitted_names.insert(member_name.clone(), format_selectors(&member.selectors))
            {
                return Err(format!(
                    "transfer family `{family_name}`: selector maps {previous} and {} \
                     both expand to `{member_name}`",
                    format_selectors(&member.selectors)
                ));
            }
        }
    }
    Ok(())
}

fn validate_member(
    family_name: &str,
    family: &TransferFamilyDef,
    index: usize,
    member: &FamilyMemberDef,
) -> Result<(), String> {
    if member.selectors.is_empty() {
        return Err(format!(
            "transfer family `{family_name}` member {index}: selectors must not be empty"
        ));
    }
    for (key, value) in &member.selectors {
        if key.is_empty() {
            return Err(format!(
                "transfer family `{family_name}` member {index}: selector keys must not be empty"
            ));
        }
        if let SelectorValue::String(text) = value
            && text.is_empty()
        {
            return Err(format!(
                "transfer family `{family_name}` member {index}: selector `{key}` must not be blank"
            ));
        }
    }
    if member.scale == 0 {
        return Err(format!(
            "transfer family `{family_name}` member {index}: scale must be positive"
        ));
    }
    let [lo, hi] = member.applicability.model_input;
    if !lo.is_finite() || !hi.is_finite() || lo >= hi {
        return Err(format!(
            "transfer family `{family_name}` member {index}: applicability.model_input \
             must be two strictly increasing finite values"
        ));
    }
    if matches!(family.model, Some(ModelDef::ScaledPolynomial { .. })) {
        let domain =
            super::model::observation_domain(member.scale, member.applicability.model_input)
                .map_err(|error| {
                    format!("transfer family `{family_name}` member {index}: {error}")
                })?;
        validate_observation_guard(
            &format!("transfer family `{family_name}` member {index}"),
            family.observation_guard,
            domain[1],
        )?;
    } else if let Some(domain) = family.domain {
        validate_observation_guard(
            &format!("transfer family `{family_name}` member {index}"),
            family.observation_guard,
            domain[1],
        )?;
    } else if let Some(points) = &family.points
        && let Some(last) = points.last()
    {
        validate_observation_guard(
            &format!("transfer family `{family_name}` member {index}"),
            family.observation_guard,
            last.input,
        )?;
    }
    Ok(())
}

fn member_transfer(
    family_name: &str,
    family: &TransferFamilyDef,
    member: &FamilyMemberDef,
) -> Result<TransferDef, String> {
    let (model, domain, output_range) = match &family.model {
        Some(ModelDef::ScaledPolynomial { coefficients, .. }) => {
            let domain =
                super::model::observation_domain(member.scale, member.applicability.model_input)
                    .map_err(|error| format!("transfer family `{family_name}`: {error}"))?;
            (
                Some(ModelDef::ScaledPolynomial {
                    coefficients: coefficients.clone(),
                    scale: Some(member.scale),
                }),
                Some(domain),
                None,
            )
        }
        other => (other.clone(), family.domain, family.output_range),
    };
    Ok(TransferDef {
        input_unit: family.input_unit.clone(),
        output_unit: family.output_unit.clone(),
        output_scale: family.output_scale,
        max_interpolation_error: family.max_interpolation_error,
        max_knots: family.max_knots,
        below: family.below,
        above: family.above,
        observation_guard: family.observation_guard,
        points: family.points.clone(),
        formula: family.formula.clone(),
        model,
        domain,
        output_range,
    })
}

pub(crate) fn expanded_name(
    family_name: &str,
    selectors: &BTreeMap<String, SelectorValue>,
) -> Result<String, String> {
    let mut name = family_name.to_string();
    for (key, value) in selectors {
        name.push('_');
        name.push_str(key);
        name.push('_');
        name.push_str(&value.token());
    }
    Ok(name)
}

pub(crate) fn format_selectors(selectors: &BTreeMap<String, SelectorValue>) -> String {
    let parts: Vec<String> = selectors
        .iter()
        .map(|(key, value)| match value {
            SelectorValue::String(text) => format!("{key}={text:?}"),
            SelectorValue::Integer(int) => format!("{key}={int}"),
        })
        .collect();
    format!("{{{}}}", parts.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::r#gen::{DefinitionsFile, ObservationGuardBehaviorDef, ObservationGuardDef};
    use std::vec;

    fn emit_member(gain: &str, integration_time_ms: i64, scale: u32) -> FamilyMemberDef {
        member(gain, integration_time_ms, scale, MemberStatus::Emit)
    }

    fn member(
        gain: &str,
        integration_time_ms: i64,
        scale: u32,
        status: MemberStatus,
    ) -> FamilyMemberDef {
        FamilyMemberDef {
            selectors: BTreeMap::from([
                ("gain".into(), SelectorValue::String(gain.into())),
                (
                    "integration_time_ms".into(),
                    SelectorValue::Integer(integration_time_ms),
                ),
            ]),
            scale,
            status,
            applicability: ApplicabilityDef {
                model_input: [100.0, 22_000.0],
            },
        }
    }

    fn formula_family(members: Vec<FamilyMemberDef>) -> TransferFamilyDef {
        TransferFamilyDef {
            input_unit: "count".into(),
            output_unit: "unit".into(),
            output_scale: 1000,
            max_interpolation_error: 50,
            max_knots: 64,
            interpolate_selectors: false,
            below: BoundaryDef::Error,
            above: BoundaryDef::Error,
            observation_guard: None,
            points: None,
            formula: Some("x".into()),
            model: None,
            domain: Some([1, 10]),
            output_range: None,
            members,
        }
    }

    fn scaled_poly_family(
        coefficients: Vec<f64>,
        members: Vec<FamilyMemberDef>,
    ) -> TransferFamilyDef {
        TransferFamilyDef {
            input_unit: "count".into(),
            output_unit: "unit".into(),
            output_scale: 1,
            max_interpolation_error: 1,
            max_knots: 64,
            interpolate_selectors: false,
            below: BoundaryDef::Error,
            above: BoundaryDef::Error,
            observation_guard: None,
            points: None,
            formula: None,
            model: Some(ModelDef::ScaledPolynomial {
                coefficients,
                scale: None,
            }),
            domain: None,
            output_range: None,
            members,
        }
    }

    #[test]
    fn required_member_expands_with_selector_keys_in_the_name() {
        let mut families = BTreeMap::new();
        families.insert(
            "als".into(),
            formula_family(vec![emit_member("div4", 100, 268_800)]),
        );
        let expanded = expand_families(&families).unwrap();
        assert_eq!(expanded.len(), 1);
        assert!(expanded.contains_key("als_gain_div4_integration_time_ms_100"));
        let def = &expanded["als_gain_div4_integration_time_ms_100"];
        assert_eq!(def.domain, Some([1, 10]));
        assert_eq!(def.formula.as_deref(), Some("x"));
        assert_eq!(def.max_knots, 64);
    }

    #[test]
    fn interpolate_selectors_is_rejected() {
        let mut family = formula_family(vec![emit_member("div4", 100, 268_800)]);
        family.interpolate_selectors = true;
        let mut families = BTreeMap::new();
        families.insert("als".into(), family);
        let error = expand_families(&families).unwrap_err();
        assert!(error.contains("interpolate_selectors"));
    }

    #[test]
    fn description_only_members_are_validated_then_skipped() {
        let mut families = BTreeMap::new();
        families.insert(
            "als".into(),
            formula_family(vec![
                emit_member("div4", 100, 268_800),
                member("x1", 100, 4_200, MemberStatus::DoNotUse),
                member("x2", 100, 2_100, MemberStatus::None),
            ]),
        );
        let expanded = expand_families(&families).unwrap();
        assert_eq!(expanded.len(), 1);
        assert!(expanded.contains_key("als_gain_div4_integration_time_ms_100"));
        assert!(!expanded.contains_key("als_gain_x1_integration_time_ms_100"));
    }

    #[test]
    fn malformed_description_only_member_is_rejected() {
        let mut families = BTreeMap::new();
        families.insert(
            "als".into(),
            formula_family(vec![
                emit_member("div4", 100, 268_800),
                member("x1", 100, 0, MemberStatus::DoNotUse),
            ]),
        );
        let error = expand_families(&families).unwrap_err();
        assert!(error.contains("scale must be positive"));
        assert!(error.contains("member 1"));
    }

    #[test]
    fn zero_scale_is_rejected_for_none_members() {
        let mut families = BTreeMap::new();
        families.insert(
            "als".into(),
            formula_family(vec![
                emit_member("div4", 100, 268_800),
                member("x1", 25, 0, MemberStatus::None),
            ]),
        );
        let error = expand_families(&families).unwrap_err();
        assert!(error.contains("scale must be positive"));
    }

    #[test]
    fn reversed_model_input_is_rejected() {
        let mut bad = emit_member("div4", 100, 268_800);
        bad.applicability.model_input = [10.0, 1.0];
        let mut families = BTreeMap::new();
        families.insert("als".into(), formula_family(vec![bad]));
        let error = expand_families(&families).unwrap_err();
        assert!(error.contains("model_input"));
    }

    #[test]
    fn empty_selectors_are_rejected() {
        let mut bad = emit_member("div4", 100, 268_800);
        bad.selectors.clear();
        let mut families = BTreeMap::new();
        families.insert("als".into(), formula_family(vec![bad]));
        let error = expand_families(&families).unwrap_err();
        assert!(error.contains("selectors must not be empty"));
    }

    #[test]
    fn duplicate_selector_maps_are_rejected_across_statuses() {
        let mut families = BTreeMap::new();
        families.insert(
            "als".into(),
            formula_family(vec![
                emit_member("div4", 100, 268_800),
                member("div4", 100, 268_800, MemberStatus::None),
            ]),
        );
        let error = expand_families(&families).unwrap_err();
        assert!(error.contains("share selector identity"));
        assert!(error.contains("gain=\"div4\""));
    }

    #[test]
    fn integer_and_string_selector_values_remain_distinct_identities() {
        let mut string_one = emit_member("div4", 100, 268_800);
        string_one.selectors = BTreeMap::from([("a".into(), SelectorValue::String("1".into()))]);
        let mut int_one = emit_member("div8", 100, 537_600);
        int_one.selectors = BTreeMap::from([("a".into(), SelectorValue::Integer(1))]);
        let mut families = BTreeMap::new();
        families.insert("als".into(), formula_family(vec![string_one, int_one]));
        let error = expand_families(&families).unwrap_err();
        assert!(error.contains("both expand to `als_a_1`"));
        assert!(error.contains("a=\"1\""));
        assert!(error.contains("a=1"));
    }

    #[test]
    fn description_only_members_may_share_an_expanded_name() {
        let emitted = emit_member("div4", 100, 268_800);
        let mut string_one = member("x1", 100, 4_200, MemberStatus::None);
        string_one.selectors = BTreeMap::from([("a".into(), SelectorValue::String("1".into()))]);
        let mut int_one = member("x2", 100, 2_100, MemberStatus::DoNotUse);
        int_one.selectors = BTreeMap::from([("a".into(), SelectorValue::Integer(1))]);
        let mut families = BTreeMap::new();
        families.insert(
            "als".into(),
            formula_family(vec![emitted, string_one, int_one]),
        );

        let expanded = expand_families(&families).unwrap();
        assert_eq!(expanded.len(), 1);
        assert!(expanded.contains_key("als_gain_div4_integration_time_ms_100"));
    }

    #[test]
    fn description_only_member_may_share_an_emitted_name() {
        let mut string_one = emit_member("div4", 100, 268_800);
        string_one.selectors = BTreeMap::from([("a".into(), SelectorValue::String("1".into()))]);
        let mut int_one = member("x1", 100, 4_200, MemberStatus::None);
        int_one.selectors = BTreeMap::from([("a".into(), SelectorValue::Integer(1))]);
        let mut families = BTreeMap::new();
        families.insert("als".into(), formula_family(vec![string_one, int_one]));

        let expanded = expand_families(&families).unwrap();
        assert_eq!(expanded.len(), 1);
        assert!(expanded.contains_key("als_a_1"));
    }

    #[test]
    fn ambiguous_value_concatenation_fails_with_both_selector_maps() {
        let mut first = emit_member("div4", 100, 268_800);
        first.selectors = BTreeMap::from([
            ("a".into(), SelectorValue::String("x".into())),
            ("b".into(), SelectorValue::String("y".into())),
        ]);
        let mut second = emit_member("div8", 100, 537_600);
        second.selectors = BTreeMap::from([("a".into(), SelectorValue::String("x_b_y".into()))]);
        let mut families = BTreeMap::new();
        families.insert("als".into(), formula_family(vec![first, second]));
        let error = expand_families(&families).unwrap_err();
        assert!(error.contains("both expand to `als_a_x_b_y`"));
        assert!(error.contains("a=\"x\""));
        assert!(error.contains("a=\"x_b_y\""));
    }

    #[test]
    fn value_only_concatenation_is_not_the_identity() {
        let mut keyed = emit_member("div4", 100, 268_800);
        keyed.selectors = BTreeMap::from([
            ("gain".into(), SelectorValue::String("div4".into())),
            ("t".into(), SelectorValue::Integer(100)),
        ]);
        let mut collapsed = emit_member("div8", 200, 537_600);
        collapsed.selectors =
            BTreeMap::from([("gain".into(), SelectorValue::String("div4_t".into()))]);
        let mut families = BTreeMap::new();
        families.insert("als".into(), formula_family(vec![keyed, collapsed]));
        let expanded = expand_families(&families).unwrap();
        assert!(expanded.contains_key("als_gain_div4_t_100"));
        assert!(expanded.contains_key("als_gain_div4_t"));
    }

    #[test]
    fn no_emit_members_is_rejected() {
        let mut families = BTreeMap::new();
        families.insert(
            "als".into(),
            formula_family(vec![member("x1", 100, 4_200, MemberStatus::DoNotUse)]),
        );
        let error = expand_families(&families).unwrap_err();
        assert!(error.contains("no members with status = \"emit\""));
    }

    #[test]
    fn max_knots_above_family_cap_is_rejected() {
        let mut family = formula_family(vec![emit_member("div4", 100, 268_800)]);
        family.max_knots = 257;
        let mut families = BTreeMap::new();
        families.insert("als".into(), family);
        let error = expand_families(&families).unwrap_err();
        assert!(error.contains("max_knots must be in 2..=256"));
    }

    #[test]
    fn twenty_four_members_emit_only_the_correction_required_subset() {
        let mut members = Vec::new();
        for gain in ["x1", "x2", "div4", "div8"] {
            for &it in &[25_i64, 50, 100, 200, 400, 800] {
                let status = if gain == "div4" || gain == "div8" {
                    MemberStatus::Emit
                } else {
                    MemberStatus::DoNotUse
                };
                members.push(member(gain, it, 1_000, status));
            }
        }
        assert_eq!(members.len(), 24);
        let mut families = BTreeMap::new();
        families.insert("als".into(), formula_family(members));
        let expanded = expand_families(&families).unwrap();
        assert_eq!(expanded.len(), 12);
        for gain in ["div4", "div8"] {
            for it in [25, 50, 100, 200, 400, 800] {
                let name = format!("als_gain_{gain}_integration_time_ms_{it}");
                assert!(expanded.contains_key(&name), "missing {name}");
            }
        }
        for gain in ["x1", "x2"] {
            for it in [25, 50, 100, 200, 400, 800] {
                let name = format!("als_gain_{gain}_integration_time_ms_{it}");
                assert!(!expanded.contains_key(&name), "unexpected {name}");
            }
        }
    }

    fn parse_family(toml: &str) -> Result<DefinitionsFile, String> {
        DefinitionsFile::from_toml_str(toml).map_err(|error| error.to_string())
    }

    #[test]
    fn unknown_family_field_is_rejected() {
        let error = parse_family(
            r#"
[transfer_families.als]
input_unit = "count"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
formula = "x"
domain = [1, 10]
scale = 1
members = []
"#,
        )
        .unwrap_err();
        assert!(error.contains("unknown field `scale`"), "{error}");
    }

    #[test]
    fn unknown_member_field_is_rejected() {
        let error = parse_family(
            r#"
[transfer_families.als]
input_unit = "count"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
formula = "x"
domain = [1, 10]

[[transfer_families.als.members]]
selectors = { gain = "div4" }
scale = 1
status = "emit"
applicability = { model_input = [1.0, 2.0] }
correction = "required"
"#,
        )
        .unwrap_err();
        assert!(error.contains("unknown field `correction`"), "{error}");
    }

    #[test]
    fn unknown_applicability_field_is_rejected() {
        let error = parse_family(
            r#"
[transfer_families.als]
input_unit = "count"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
formula = "x"
domain = [1, 10]

[[transfer_families.als.members]]
selectors = { gain = "div4" }
scale = 1
status = "emit"
applicability = { model_input = [1.0, 2.0], uncorrected_lux = [1.0, 2.0] }
"#,
        )
        .unwrap_err();
        assert!(error.contains("unknown field `uncorrected_lux`"), "{error}");
    }

    #[test]
    fn unknown_gap_field_is_rejected() {
        let error = parse_family(
            r#"
[gaps.white]
status = "undefined"
reason = "counts only"
channel = "als"
"#,
        )
        .unwrap_err();
        assert!(error.contains("unknown field `channel`"), "{error}");
    }

    #[test]
    fn float_selector_is_rejected() {
        let error = parse_family(
            r#"
[transfer_families.als]
input_unit = "count"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
formula = "x"
domain = [1, 10]

[[transfer_families.als.members]]
selectors = { gain = 1.5 }
scale = 1
status = "emit"
applicability = { model_input = [1.0, 2.0] }
"#,
        )
        .unwrap_err();
        assert!(
            error.contains("string or integer") || error.contains("float"),
            "{error}"
        );
    }

    #[test]
    fn saturation_table_is_copied_onto_emitted_members() {
        let error_form = parse_family(
            r#"
[transfer_families.als]
input_unit = "count"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
formula = "x"
domain = [1, 10]
saturation = "error"
members = []
"#,
        )
        .unwrap_err();
        assert!(
            error_form.contains("invalid type") || error_form.contains("saturation"),
            "{error_form}"
        );

        let mut family = formula_family(vec![emit_member("div4", 100, 268_800)]);
        family.observation_guard = Some(ObservationGuardDef {
            code: 65_535,
            behavior: ObservationGuardBehaviorDef::Error,
        });
        let mut families = BTreeMap::new();
        families.insert("als".into(), family);
        let expanded = expand_families(&families).unwrap();
        let def = &expanded["als_gain_div4_integration_time_ms_100"];
        assert_eq!(def.domain, Some([1, 10]));
        assert_eq!(def.observation_guard.unwrap().code, 65_535);
        assert_eq!(
            def.observation_guard.unwrap().behavior,
            ObservationGuardBehaviorDef::Error
        );
    }

    #[test]
    fn saturation_code_inside_domain_is_rejected() {
        let defs = parse_family(
            r#"
[transfer_families.als]
input_unit = "count"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
formula = "x"
domain = [1, 10]
saturation = { code = 10, behavior = "error" }

[[transfer_families.als.members]]
selectors = { gain = "div4" }
scale = 1
status = "emit"
applicability = { model_input = [1.0, 2.0] }
"#,
        )
        .unwrap();
        let error = defs.validate().unwrap_err().to_string();
        assert!(error.contains("strictly above domain_max"), "{error}");
    }

    #[test]
    fn saturation_unknown_behavior_is_rejected() {
        let error = parse_family(
            r#"
[transfer_families.als]
input_unit = "count"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
formula = "x"
domain = [1, 10]
saturation = { code = 65535, behavior = "extrapolate" }
members = []
"#,
        )
        .unwrap_err();
        assert!(
            error.contains("unknown variant") || error.contains("extrapolate"),
            "{error}"
        );
    }

    #[test]
    fn saturation_unknown_field_is_rejected() {
        let error = parse_family(
            r#"
[transfer_families.als]
input_unit = "count"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
formula = "x"
domain = [1, 10]
saturation = { code = 65535, behavior = "error", extra = true }
members = []
"#,
        )
        .unwrap_err();
        assert!(error.contains("unknown field `extra`"), "{error}");
    }

    #[test]
    fn family_saturation_is_preserved_and_not_added_to_scaled_polynomial_domain() {
        let mut member = emit_member("div4", 100, 33_600);
        member.applicability.model_input = [63.0, 100.0];
        let mut family = scaled_poly_family(vec![0.0, 1.0], vec![member]);
        family.above = BoundaryDef::Clamp;
        family.observation_guard = Some(ObservationGuardDef {
            code: 65_535,
            behavior: ObservationGuardBehaviorDef::Error,
        });
        let mut families = BTreeMap::new();
        families.insert("als".into(), family);
        let expanded = expand_families(&families).unwrap();
        let (name, def) = expanded.iter().next().unwrap();
        assert_eq!(def.observation_guard.unwrap().code, 65_535);
        assert_eq!(def.above, BoundaryDef::Clamp);
        let domain = def.domain.unwrap();
        assert!(domain[1] < 65_535, "fitted domain {domain:?}");
        let data = super::super::build(name, def).unwrap();
        assert!(!data.inputs.contains(&65_535));
        assert_eq!(*data.inputs.last().unwrap(), domain[1]);
    }

    #[test]
    fn scaled_polynomial_family_injects_per_member_scale_and_domain() {
        let mut low = emit_member("div4", 800, 33_600);
        low.applicability.model_input = [63.0, 100.0];
        let mut high = emit_member("div4", 400, 67_200);
        high.applicability.model_input = [63.0, 100.0];
        let mut families = BTreeMap::new();
        families.insert(
            "als".into(),
            scaled_poly_family(vec![0.0, 1.0], vec![low, high]),
        );
        let expanded = expand_families(&families).unwrap();

        let slow = &expanded["als_gain_div4_integration_time_ms_800"];
        assert_eq!(slow.domain.unwrap()[0], 1875);
        match &slow.model {
            Some(ModelDef::ScaledPolynomial {
                scale: Some(33_600),
                coefficients,
            }) => assert_eq!(coefficients, &vec![0.0, 1.0]),
            other => panic!("expected injected scale 33600, got {other:?}"),
        }

        let fast = &expanded["als_gain_div4_integration_time_ms_400"];
        assert_eq!(fast.domain.unwrap()[0], 938);
        match &fast.model {
            Some(ModelDef::ScaledPolynomial {
                scale: Some(67_200),
                ..
            }) => {}
            other => panic!("expected injected scale 67200, got {other:?}"),
        }
    }

    #[test]
    fn scaled_polynomial_family_rejects_scale_on_the_shared_model() {
        let mut family = scaled_poly_family(vec![0.0, 1.0], vec![emit_member("div4", 100, 33_600)]);
        if let Some(ModelDef::ScaledPolynomial { scale, .. }) = &mut family.model {
            *scale = Some(33_600);
        }
        let mut families = BTreeMap::new();
        families.insert("als".into(), family);
        let error = expand_families(&families).unwrap_err();
        assert!(error.contains("scale belongs on members"), "{error}");
    }

    #[test]
    fn scaled_polynomial_family_rejects_family_level_domain() {
        let mut family = scaled_poly_family(vec![0.0, 1.0], vec![emit_member("div4", 100, 33_600)]);
        family.domain = Some([1, 10]);
        let mut families = BTreeMap::new();
        families.insert("als".into(), family);
        let error = expand_families(&families).unwrap_err();
        assert!(error.contains("derived from applicability"), "{error}");
    }

    #[test]
    fn scaled_polynomial_empty_applicability_window_fails_for_every_member() {
        let mut bad = emit_member("div4", 100, 33_600);
        bad.applicability.model_input = [0.0, 0.01];
        bad.status = MemberStatus::DoNotUse;
        let mut families = BTreeMap::new();
        families.insert(
            "als".into(),
            scaled_poly_family(vec![0.0, 1.0], vec![emit_member("div8", 100, 67_200), bad]),
        );
        let error = expand_families(&families).unwrap_err();
        assert!(error.contains("member 1"), "{error}");
        assert!(error.contains("fewer than two"), "{error}");
    }

    #[test]
    fn scaled_polynomial_optical_model_field_is_rejected() {
        let error = parse_family(
            r#"
[transfer_families.als]
input_unit = "count"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1

[transfer_families.als.model]
kind = "scaled_polynomial"
coefficients = [0.0, 1.0]
scale_micro_lux_per_count = 33600

[[transfer_families.als.members]]
selectors = { gain = "div4" }
scale = 33600
status = "emit"
applicability = { model_input = [63.0, 100.0] }
"#,
        )
        .unwrap_err();
        assert!(
            error.contains("unknown field `scale_micro_lux_per_count`"),
            "{error}"
        );
    }
}
