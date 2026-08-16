//! Discrete transfer families: one shared source, explicit members, no interpolation.

// Host-only: module-local std link (crate root stays `#![no_std]`).
extern crate std;

use std::collections::BTreeMap;
use std::format;
use std::ops::Deref;
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

/// One named family: shared source, per-member input transform and applicability.
///
/// Metadata fields are public. Source and boundary fields are inspectable
/// through accessors so the NTC model catalog is not part of the public IR.
/// Dependents obtain the map via `DefinitionsFile::transfer_families`.
///
/// Selectors are never interpolated. Family-level `domain` and `output_range`
/// are rejected as unknown; members declare those coordinates through
/// `applicability`. `input_transform` is per-member only. A family-level
/// `scale` field is rejected as unknown so a shared-model scale cannot be
/// silently overwritten.
///
/// Family knot default is 64 with hard cap 256: this is family-only policy so
/// discrete members cannot become dense ADC tables. Standalone transfers keep
/// default 256 / cap 4096.
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
    /// Optional aggregate knot budget across every emitted member.
    ///
    /// Omitted means no family-total cap. Per-member [`Self::max_knots`] still
    /// applies. Zero is rejected during validation.
    #[serde(default)]
    pub max_total_knots: Option<usize>,
    /// Optional aggregate `_INPUTS` + `_OUTPUTS` array-payload budget.
    ///
    /// Counted at six bytes per knot. Structural runtime overhead is excluded.
    /// Omitted means no family-total cap. Zero is rejected during validation.
    #[serde(default)]
    pub max_table_bytes: Option<usize>,
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

/// Exact rational map from observation codes to shared-model input:
/// `u = count * numerator / denominator`.
///
/// Accepted only for `kind = "scaled_polynomial"` members. Integer product
/// first, then divide, so the `< 2^48` value is exact in `f64` before the
/// quotient. Both terms must be nonzero.
#[derive(Debug, Clone, Copy, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct InputTransform {
    /// Nonzero scale numerator.
    pub numerator: u32,
    /// Nonzero scale denominator.
    pub denominator: u32,
}

/// One explicit selector combination. Never synthesized by interpolation.
///
/// Every accepted field is source-aware: a field unsupported by the family's
/// shared source fails validation. See [`ApplicabilityDef`] for the coordinate
/// space of `applicability`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FamilyMemberDef {
    /// Discrete selector map. Keys and typed values form the member identity.
    pub selectors: BTreeMap<String, SelectorValue>,
    /// Exact input transform. Required for mapped `scaled_polynomial` members;
    /// forbidden for other sources and for `unsupported` members.
    #[serde(default)]
    pub input_transform: Option<InputTransform>,
    /// Whether this member is generated or retained as description only.
    pub status: MemberStatus,
    /// Non-blank rationale. Required for every non-`emit` status; forbidden on `emit`.
    #[serde(default)]
    pub reason: Option<String>,
    /// Source-backed window in the coordinate space the shared source uses.
    ///
    /// Required for `emit`, `unnecessary`, and `forbidden` members. An
    /// `unsupported` member has no source mapping, so it may omit this field
    /// and must leave every coordinate unset.
    #[serde(default)]
    pub applicability: ApplicabilityDef,
}

/// Where this member's mapping is source-backed.
///
/// For `emit`, `unnecessary`, and `forbidden` members, exactly one of
/// `observation`, `model_input`, or `physical` must be set and it must match
/// the family's shared source. An `unsupported` member must leave all three
/// unset because it has no source mapping:
///
/// - formula requires `observation` (member observation domain)
/// - points requires `observation` (inclusive clip of the shared point set)
/// - `scaled_polynomial` requires `model_input` (converted to codes through
///   `input_transform`)
/// - `ntc_beta_divider` requires `physical` (member output range)
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicabilityDef {
    /// Inclusive observation-code window `[min, max]`.
    #[serde(default)]
    pub observation: Option<[u16; 2]>,
    /// Inclusive shared-model-input window `[min, max]`.
    #[serde(default)]
    pub model_input: Option<[f64; 2]>,
    /// Inclusive physical-output window `[min, max]`.
    #[serde(default)]
    pub physical: Option<[f64; 2]>,
}

/// Whether a family member is generated or retained as description only.
#[derive(Debug, Clone, Copy, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MemberStatus {
    /// Generate a sparse `PiecewiseLinearTransfer` for this member.
    Emit,
    /// Listed combination has a source mapping but need not be generated.
    /// Requires [`FamilyMemberDef::reason`].
    Unnecessary,
    /// Listed combination has no source mapping. Requires
    /// [`FamilyMemberDef::reason`] and forbids source applicability and input
    /// transforms.
    Unsupported,
    /// Listed combination has a source mapping but must not be used. Requires
    /// [`FamilyMemberDef::reason`].
    Forbidden,
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

/// Family identity retained through expansion so generation reports do not
/// reconstruct members by matching expanded names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FamilyMemberOrigin {
    pub family: String,
    pub selectors: BTreeMap<String, SelectorValue>,
}

/// Standalone transfer or expanded family member, with optional family origin.
#[derive(Clone, Debug)]
pub(crate) struct ResolvedTransfer {
    pub def: TransferDef,
    pub origin: Option<FamilyMemberOrigin>,
}

impl Deref for ResolvedTransfer {
    type Target = TransferDef;

    fn deref(&self) -> &TransferDef {
        &self.def
    }
}

/// Expand families into standalone transfer defs.
///
/// Every member is validated before non-`emit` statuses are filtered.
pub(crate) fn expand_families(
    families: &BTreeMap<String, TransferFamilyDef>,
) -> Result<BTreeMap<String, ResolvedTransfer>, String> {
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
            let resolved = ResolvedTransfer {
                def,
                origin: Some(FamilyMemberOrigin {
                    family: family_name.clone(),
                    selectors: member.selectors.clone(),
                }),
            };
            if let Some(_previous) = out.insert(member_name.clone(), resolved) {
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
    if !(2..=FAMILY_MAX_KNOTS_HARD).contains(&family.max_knots) {
        return Err(format!(
            "transfer family `{family_name}`: max_knots must be in 2..={FAMILY_MAX_KNOTS_HARD}"
        ));
    }
    if family.max_total_knots == Some(0) {
        return Err(format!(
            "transfer family `{family_name}`: max_total_knots must be positive"
        ));
    }
    if family.max_table_bytes == Some(0) {
        return Err(format!(
            "transfer family `{family_name}`: max_table_bytes must be positive"
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
                "transfer family `{family_name}`: scale belongs on members as input_transform, \
                 not the shared model"
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

fn member_label(family_name: &str, index: usize) -> String {
    format!("transfer family `{family_name}` member {index}")
}

fn source_kind(family: &TransferFamilyDef) -> &'static str {
    match &family.model {
        Some(ModelDef::ScaledPolynomial { .. }) => "scaled_polynomial",
        Some(ModelDef::NtcBetaDivider { .. }) => "ntc_beta_divider",
        None if family.formula.is_some() => "formula",
        None if family.points.is_some() => "points",
        _ => "unknown",
    }
}

fn reject_unsupported_field(label: &str, field: &str, kind: &str) -> String {
    format!("{label}: `{field}` is not supported for {kind} sources")
}

fn reject_unsupported_member_field(label: &str, field: &str) -> String {
    format!(
        "{label}: `{field}` is forbidden for status = \"unsupported\" because unsupported members have no source mapping"
    )
}

fn validate_status_reason(label: &str, member: &FamilyMemberDef) -> Result<(), String> {
    match member.status {
        MemberStatus::Emit => {
            if member.reason.is_some() {
                return Err(format!(
                    "{label}: reason is forbidden for status = \"emit\""
                ));
            }
        }
        MemberStatus::Unnecessary | MemberStatus::Unsupported | MemberStatus::Forbidden => {
            match member.reason.as_deref().map(str::trim) {
                None | Some("") => {
                    return Err(format!(
                        "{label}: status = \"{}\" requires a non-blank reason",
                        match member.status {
                            MemberStatus::Unnecessary => "unnecessary",
                            MemberStatus::Unsupported => "unsupported",
                            MemberStatus::Forbidden => "forbidden",
                            MemberStatus::Emit => unreachable!(),
                        }
                    ));
                }
                Some(_) => {}
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
    let label = member_label(family_name, index);
    if member.selectors.is_empty() {
        return Err(format!("{label}: selectors must not be empty"));
    }
    for (key, value) in &member.selectors {
        if key.is_empty() {
            return Err(format!("{label}: selector keys must not be empty"));
        }
        if let SelectorValue::String(text) = value
            && text.is_empty()
        {
            return Err(format!("{label}: selector `{key}` must not be blank"));
        }
    }
    validate_status_reason(&label, member)?;

    if member.status == MemberStatus::Unsupported {
        if member.input_transform.is_some() {
            return Err(reject_unsupported_member_field(&label, "input_transform"));
        }
        if member.applicability.observation.is_some() {
            return Err(reject_unsupported_member_field(
                &label,
                "applicability.observation",
            ));
        }
        if member.applicability.model_input.is_some() {
            return Err(reject_unsupported_member_field(
                &label,
                "applicability.model_input",
            ));
        }
        if member.applicability.physical.is_some() {
            return Err(reject_unsupported_member_field(
                &label,
                "applicability.physical",
            ));
        }
        return Ok(());
    }

    let kind = source_kind(family);
    match kind {
        "formula" => {
            if member.input_transform.is_some() {
                return Err(reject_unsupported_field(&label, "input_transform", kind));
            }
            if member.applicability.model_input.is_some() {
                return Err(reject_unsupported_field(
                    &label,
                    "applicability.model_input",
                    kind,
                ));
            }
            if member.applicability.physical.is_some() {
                return Err(reject_unsupported_field(
                    &label,
                    "applicability.physical",
                    kind,
                ));
            }
            let domain = observation_window(&label, member.applicability.observation)?;
            validate_observation_guard(&label, family.observation_guard, domain[1])?;
        }
        "points" => {
            if member.input_transform.is_some() {
                return Err(reject_unsupported_field(&label, "input_transform", kind));
            }
            if member.applicability.model_input.is_some() {
                return Err(reject_unsupported_field(
                    &label,
                    "applicability.model_input",
                    kind,
                ));
            }
            if member.applicability.physical.is_some() {
                return Err(reject_unsupported_field(
                    &label,
                    "applicability.physical",
                    kind,
                ));
            }
            let window = observation_window(&label, member.applicability.observation)?;
            let clipped = clip_points(
                family.points.as_deref().expect("points source checked"),
                window,
            )
            .map_err(|error| format!("{label}: {error}"))?;
            validate_observation_guard(
                &label,
                family.observation_guard,
                clipped.last().expect("clip requires two points").input,
            )?;
        }
        "scaled_polynomial" => {
            if member.applicability.observation.is_some() {
                return Err(reject_unsupported_field(
                    &label,
                    "applicability.observation",
                    kind,
                ));
            }
            if member.applicability.physical.is_some() {
                return Err(reject_unsupported_field(
                    &label,
                    "applicability.physical",
                    kind,
                ));
            }
            let transform = member.input_transform.ok_or_else(|| {
                format!("{label}: input_transform is required for scaled_polynomial sources")
            })?;
            if transform.numerator == 0 {
                return Err(format!(
                    "{label}: input_transform.numerator must be positive"
                ));
            }
            if transform.denominator == 0 {
                return Err(format!(
                    "{label}: input_transform.denominator must be nonzero"
                ));
            }
            let model_input = member.applicability.model_input.ok_or_else(|| {
                format!("{label}: applicability.model_input is required for this source")
            })?;
            let domain = super::model::observation_domain(
                transform.numerator,
                transform.denominator,
                model_input,
            )
            .map_err(|error| format!("{label}: {error}"))?;
            validate_observation_guard(&label, family.observation_guard, domain[1])?;
        }
        "ntc_beta_divider" => {
            if member.input_transform.is_some() {
                return Err(reject_unsupported_field(&label, "input_transform", kind));
            }
            if member.applicability.observation.is_some() {
                return Err(reject_unsupported_field(
                    &label,
                    "applicability.observation",
                    kind,
                ));
            }
            if member.applicability.model_input.is_some() {
                return Err(reject_unsupported_field(
                    &label,
                    "applicability.model_input",
                    kind,
                ));
            }
            let physical = physical_window(&label, member.applicability.physical)?;
            let (minimum, values, _) = super::model::evaluate(
                &label,
                family.model.as_ref().expect("ntc source checked"),
                physical,
            )?;
            let last_offset = values.len() - 1;
            let domain_max = minimum
                .checked_add(
                    u16::try_from(last_offset)
                        .map_err(|_| format!("{label}: derived model domain exceeds u16"))?,
                )
                .ok_or_else(|| format!("{label}: derived model domain exceeds u16"))?;
            validate_observation_guard(&label, family.observation_guard, domain_max)?;
        }
        _ => {
            return Err(format!(
                "transfer family `{family_name}`: exactly one of points, formula, or model must be specified"
            ));
        }
    }
    Ok(())
}

fn observation_window(label: &str, window: Option<[u16; 2]>) -> Result<[u16; 2], String> {
    let [lo, hi] = window
        .ok_or_else(|| format!("{label}: applicability.observation is required for this source"))?;
    if lo >= hi {
        return Err(format!(
            "{label}: applicability.observation must be two strictly increasing values"
        ));
    }
    Ok([lo, hi])
}

fn physical_window(label: &str, window: Option<[f64; 2]>) -> Result<[f64; 2], String> {
    let [lo, hi] = window
        .ok_or_else(|| format!("{label}: applicability.physical is required for this source"))?;
    if !lo.is_finite() || !hi.is_finite() || lo >= hi {
        return Err(format!(
            "{label}: applicability.physical must be two strictly increasing finite values"
        ));
    }
    Ok([lo, hi])
}

fn clip_points(points: &[PhysicalPoint], window: [u16; 2]) -> Result<Vec<PhysicalPoint>, String> {
    let [lo, hi] = window;
    let clipped: Vec<PhysicalPoint> = points
        .iter()
        .filter(|point| point.input >= lo && point.input <= hi)
        .cloned()
        .collect();
    if clipped.len() < 2 {
        return Err(
            "applicability.observation contains fewer than two points after inclusive clip".into(),
        );
    }
    Ok(clipped)
}

fn member_transfer(
    family_name: &str,
    family: &TransferFamilyDef,
    member: &FamilyMemberDef,
) -> Result<TransferDef, String> {
    let (model, domain, output_range, points) = match source_kind(family) {
        "scaled_polynomial" => {
            let transform = member
                .input_transform
                .expect("scaled_polynomial transform validated");
            let domain = super::model::observation_domain(
                transform.numerator,
                transform.denominator,
                member.applicability.model_input.expect("validated"),
            )
            .map_err(|error| format!("transfer family `{family_name}`: {error}"))?;
            let coefficients = match &family.model {
                Some(ModelDef::ScaledPolynomial { coefficients, .. }) => coefficients.clone(),
                _ => unreachable!("source kind checked"),
            };
            (
                Some(ModelDef::ScaledPolynomial {
                    coefficients,
                    scale: Some(transform.numerator),
                    denominator: transform.denominator,
                }),
                Some(domain),
                None,
                None,
            )
        }
        "ntc_beta_divider" => (
            family.model.clone(),
            None,
            member.applicability.physical,
            None,
        ),
        "formula" => (None, member.applicability.observation, None, None),
        "points" => {
            let window = member.applicability.observation.expect("validated");
            let clipped = clip_points(family.points.as_deref().expect("points source"), window)
                .map_err(|error| format!("transfer family `{family_name}`: {error}"))?;
            (None, None, None, Some(clipped))
        }
        _ => unreachable!("validate_family requires one source"),
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
        points,
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

    fn observation(window: [u16; 2]) -> ApplicabilityDef {
        ApplicabilityDef {
            observation: Some(window),
            model_input: None,
            physical: None,
        }
    }

    fn model_input(window: [f64; 2]) -> ApplicabilityDef {
        ApplicabilityDef {
            observation: None,
            model_input: Some(window),
            physical: None,
        }
    }

    fn millionths(numerator: u32) -> InputTransform {
        InputTransform {
            numerator,
            denominator: 1_000_000,
        }
    }

    fn emit_member(gain: &str, integration_time_ms: i64) -> FamilyMemberDef {
        formula_member(gain, integration_time_ms, MemberStatus::Emit, None)
    }

    fn described(gain: &str, integration_time_ms: i64, status: MemberStatus) -> FamilyMemberDef {
        formula_member(
            gain,
            integration_time_ms,
            status,
            Some("fixture description-only member"),
        )
    }

    fn formula_member(
        gain: &str,
        integration_time_ms: i64,
        status: MemberStatus,
        reason: Option<&str>,
    ) -> FamilyMemberDef {
        FamilyMemberDef {
            selectors: BTreeMap::from([
                ("gain".into(), SelectorValue::String(gain.into())),
                (
                    "integration_time_ms".into(),
                    SelectorValue::Integer(integration_time_ms),
                ),
            ]),
            input_transform: None,
            status,
            reason: reason.map(str::to_string),
            applicability: observation([1, 10]),
        }
    }

    fn scaled_emit(gain: &str, integration_time_ms: i64, numerator: u32) -> FamilyMemberDef {
        FamilyMemberDef {
            selectors: BTreeMap::from([
                ("gain".into(), SelectorValue::String(gain.into())),
                (
                    "integration_time_ms".into(),
                    SelectorValue::Integer(integration_time_ms),
                ),
            ]),
            input_transform: Some(millionths(numerator)),
            status: MemberStatus::Emit,
            reason: None,
            applicability: model_input([100.0, 22_000.0]),
        }
    }

    fn formula_family(members: Vec<FamilyMemberDef>) -> TransferFamilyDef {
        TransferFamilyDef {
            input_unit: "count".into(),
            output_unit: "unit".into(),
            output_scale: 1000,
            max_interpolation_error: 50,
            max_knots: 64,
            max_total_knots: None,
            max_table_bytes: None,
            below: BoundaryDef::Error,
            above: BoundaryDef::Error,
            observation_guard: None,
            points: None,
            formula: Some("x".into()),
            model: None,
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
            max_total_knots: None,
            max_table_bytes: None,
            below: BoundaryDef::Error,
            above: BoundaryDef::Error,
            observation_guard: None,
            points: None,
            formula: None,
            model: Some(ModelDef::ScaledPolynomial {
                coefficients,
                scale: None,
                denominator: 1_000_000,
            }),
            members,
        }
    }

    #[test]
    fn required_member_expands_with_selector_keys_in_the_name() {
        let mut families = BTreeMap::new();
        families.insert("als".into(), formula_family(vec![emit_member("div4", 100)]));
        let expanded = expand_families(&families).unwrap();
        assert_eq!(expanded.len(), 1);
        assert!(expanded.contains_key("als_gain_div4_integration_time_ms_100"));
        let def = &expanded["als_gain_div4_integration_time_ms_100"];
        assert_eq!(def.domain, Some([1, 10]));
        assert_eq!(def.formula.as_deref(), Some("x"));
        assert_eq!(def.max_knots, 64);
    }

    #[test]
    fn interpolate_selectors_is_an_unknown_field() {
        let error = parse_family(
            r#"
[transfer_families.als]
input_unit = "count"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
formula = "x"
interpolate_selectors = false

[[transfer_families.als.members]]
selectors = { gain = "div4" }
status = "emit"
applicability = { observation = [1, 10] }
"#,
        )
        .unwrap_err();
        assert!(
            error.contains("unknown field `interpolate_selectors`"),
            "{error}"
        );
    }

    #[test]
    fn description_only_members_are_validated_then_skipped() {
        let mut families = BTreeMap::new();
        families.insert(
            "als".into(),
            formula_family(vec![
                emit_member("div4", 100),
                described("x1", 100, MemberStatus::Forbidden),
                described("x2", 100, MemberStatus::Unnecessary),
            ]),
        );
        let expanded = expand_families(&families).unwrap();
        assert_eq!(expanded.len(), 1);
        assert!(expanded.contains_key("als_gain_div4_integration_time_ms_100"));
        assert!(!expanded.contains_key("als_gain_x1_integration_time_ms_100"));
    }

    #[test]
    fn malformed_description_only_member_is_rejected() {
        let mut bad = described("x1", 100, MemberStatus::Forbidden);
        bad.applicability.observation = Some([10, 1]);
        let mut families = BTreeMap::new();
        families.insert(
            "als".into(),
            formula_family(vec![emit_member("div4", 100), bad]),
        );
        let error = expand_families(&families).unwrap_err();
        assert!(error.contains("applicability.observation"), "{error}");
        assert!(error.contains("member 1"));
    }

    #[test]
    fn zero_denominator_is_rejected_for_unnecessary_members() {
        let mut bad = scaled_emit("x1", 25, 33_600);
        bad.status = MemberStatus::Unnecessary;
        bad.reason = Some("fixture description-only member".into());
        bad.input_transform = Some(InputTransform {
            numerator: 33_600,
            denominator: 0,
        });
        let mut families = BTreeMap::new();
        families.insert(
            "als".into(),
            scaled_poly_family(vec![0.0, 1.0], vec![scaled_emit("div4", 100, 33_600), bad]),
        );
        let error = expand_families(&families).unwrap_err();
        assert!(error.contains("denominator must be nonzero"), "{error}");
    }

    #[test]
    fn reversed_observation_is_rejected() {
        let mut bad = emit_member("div4", 100);
        bad.applicability.observation = Some([10, 1]);
        let mut families = BTreeMap::new();
        families.insert("als".into(), formula_family(vec![bad]));
        let error = expand_families(&families).unwrap_err();
        assert!(error.contains("applicability.observation"), "{error}");
    }

    #[test]
    fn empty_selectors_are_rejected() {
        let mut bad = emit_member("div4", 100);
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
                emit_member("div4", 100),
                described("div4", 100, MemberStatus::Unnecessary),
            ]),
        );
        let error = expand_families(&families).unwrap_err();
        assert!(error.contains("share selector identity"));
        assert!(error.contains("gain=\"div4\""));
    }

    #[test]
    fn integer_and_string_selector_values_remain_distinct_identities() {
        let mut string_one = emit_member("div4", 100);
        string_one.selectors = BTreeMap::from([("a".into(), SelectorValue::String("1".into()))]);
        let mut int_one = emit_member("div8", 100);
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
        let emitted = emit_member("div4", 100);
        let mut string_one = described("x1", 100, MemberStatus::Unnecessary);
        string_one.selectors = BTreeMap::from([("a".into(), SelectorValue::String("1".into()))]);
        let mut int_one = described("x2", 100, MemberStatus::Forbidden);
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
        let mut string_one = emit_member("div4", 100);
        string_one.selectors = BTreeMap::from([("a".into(), SelectorValue::String("1".into()))]);
        let mut int_one = described("x1", 100, MemberStatus::Unnecessary);
        int_one.selectors = BTreeMap::from([("a".into(), SelectorValue::Integer(1))]);
        let mut families = BTreeMap::new();
        families.insert("als".into(), formula_family(vec![string_one, int_one]));

        let expanded = expand_families(&families).unwrap();
        assert_eq!(expanded.len(), 1);
        assert!(expanded.contains_key("als_a_1"));
    }

    #[test]
    fn ambiguous_value_concatenation_fails_with_both_selector_maps() {
        let mut first = emit_member("div4", 100);
        first.selectors = BTreeMap::from([
            ("a".into(), SelectorValue::String("x".into())),
            ("b".into(), SelectorValue::String("y".into())),
        ]);
        let mut second = emit_member("div8", 100);
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
        let mut keyed = emit_member("div4", 100);
        keyed.selectors = BTreeMap::from([
            ("gain".into(), SelectorValue::String("div4".into())),
            ("t".into(), SelectorValue::Integer(100)),
        ]);
        let mut collapsed = emit_member("div8", 200);
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
            formula_family(vec![described("x1", 100, MemberStatus::Forbidden)]),
        );
        let error = expand_families(&families).unwrap_err();
        assert!(error.contains("no members with status = \"emit\""));
    }

    #[test]
    fn max_knots_above_family_cap_is_rejected() {
        let mut family = formula_family(vec![emit_member("div4", 100)]);
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
                let member = if gain == "div4" || gain == "div8" {
                    emit_member(gain, it)
                } else {
                    described(gain, it, MemberStatus::Forbidden)
                };
                members.push(member);
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

[[transfer_families.als.members]]
selectors = { gain = "div4" }
status = "emit"
applicability = { observation = [1, 10] }
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

[[transfer_families.als.members]]
selectors = { gain = "div4" }
status = "emit"
applicability = { observation = [1, 10], uncorrected_lux = [1.0, 2.0] }
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

[[transfer_families.als.members]]
selectors = { gain = 1.5 }
status = "emit"
applicability = { observation = [1, 10] }
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
saturation = "error"
members = []
"#,
        )
        .unwrap_err();
        assert!(
            error_form.contains("invalid type") || error_form.contains("saturation"),
            "{error_form}"
        );

        let mut family = formula_family(vec![emit_member("div4", 100)]);
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
saturation = { code = 10, behavior = "error" }

[[transfer_families.als.members]]
selectors = { gain = "div4" }
status = "emit"
applicability = { observation = [1, 10] }
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
saturation = { code = 65535, behavior = "error", extra = true }
members = []
"#,
        )
        .unwrap_err();
        assert!(error.contains("unknown field `extra`"), "{error}");
    }

    #[test]
    fn family_saturation_is_preserved_and_not_added_to_scaled_polynomial_domain() {
        let mut member = scaled_emit("div4", 100, 33_600);
        member.applicability = model_input([63.0, 100.0]);
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
    fn scaled_polynomial_family_injects_per_member_transform_and_domain() {
        let mut low = scaled_emit("div4", 800, 33_600);
        low.applicability = model_input([63.0, 100.0]);
        let mut high = scaled_emit("div4", 400, 67_200);
        high.applicability = model_input([63.0, 100.0]);
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
                denominator: 1_000_000,
                coefficients,
            }) => assert_eq!(coefficients, &vec![0.0, 1.0]),
            other => panic!("expected injected transform 33600/1e6, got {other:?}"),
        }

        let fast = &expanded["als_gain_div4_integration_time_ms_400"];
        assert_eq!(fast.domain.unwrap()[0], 938);
        match &fast.model {
            Some(ModelDef::ScaledPolynomial {
                scale: Some(67_200),
                denominator: 1_000_000,
                ..
            }) => {}
            other => panic!("expected injected transform 67200/1e6, got {other:?}"),
        }
    }

    #[test]
    fn scaled_polynomial_family_rejects_scale_on_the_shared_model() {
        let mut family = scaled_poly_family(vec![0.0, 1.0], vec![scaled_emit("div4", 100, 33_600)]);
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
        let error = parse_family(
            r#"
[transfer_families.als]
input_unit = "count"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
domain = [1, 10]

[transfer_families.als.model]
kind = "scaled_polynomial"
coefficients = [0.0, 1.0]

[[transfer_families.als.members]]
selectors = { gain = "div4" }
status = "emit"
input_transform = { numerator = 33600, denominator = 1000000 }
applicability = { model_input = [63.0, 100.0] }
"#,
        )
        .unwrap_err();
        assert!(error.contains("unknown field `domain`"), "{error}");
    }

    #[test]
    fn scaled_polynomial_empty_applicability_window_fails_for_every_member() {
        let mut bad = scaled_emit("div4", 100, 33_600);
        bad.applicability = model_input([0.0, 0.01]);
        bad.status = MemberStatus::Forbidden;
        bad.reason = Some("empty window still validated".into());
        let mut families = BTreeMap::new();
        families.insert(
            "als".into(),
            scaled_poly_family(vec![0.0, 1.0], vec![scaled_emit("div8", 100, 67_200), bad]),
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
status = "emit"
input_transform = { numerator = 33600, denominator = 1000000 }
applicability = { model_input = [63.0, 100.0] }
"#,
        )
        .unwrap_err();
        assert!(
            error.contains("unknown field `scale_micro_lux_per_count`"),
            "{error}"
        );
    }

    #[test]
    fn formula_rejects_input_transform_and_model_input() {
        let mut with_transform = emit_member("div4", 100);
        with_transform.input_transform = Some(millionths(268_800));
        let error = expand_families(&BTreeMap::from([(
            "als".into(),
            formula_family(vec![with_transform]),
        )]))
        .unwrap_err();
        assert!(
            error.contains("`input_transform` is not supported for formula"),
            "{error}"
        );

        let mut with_model_input = emit_member("div8", 100);
        with_model_input.applicability = model_input([1.0, 10.0]);
        let error = expand_families(&BTreeMap::from([(
            "als".into(),
            formula_family(vec![with_model_input]),
        )]))
        .unwrap_err();
        assert!(
            error.contains("`applicability.model_input` is not supported for formula"),
            "{error}"
        );
    }

    #[test]
    fn formula_observation_window_is_the_emitted_domain() {
        let mut wide = emit_member("div4", 100);
        wide.applicability = observation([1, 10]);
        let mut narrow = emit_member("div8", 100);
        narrow.applicability = observation([3, 7]);
        let expanded = expand_families(&BTreeMap::from([(
            "als".into(),
            formula_family(vec![wide, narrow]),
        )]))
        .unwrap();
        assert_eq!(
            expanded["als_gain_div4_integration_time_ms_100"].domain,
            Some([1, 10])
        );
        assert_eq!(
            expanded["als_gain_div8_integration_time_ms_100"].domain,
            Some([3, 7])
        );
    }

    #[test]
    fn points_observation_clips_the_shared_table() {
        let toml = r#"
[transfer_families.front_end]
input_unit = "count"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
points = [
  { input = 1, output = 1.0 },
  { input = 5, output = 5.0 },
  { input = 10, output = 10.0 },
]

[[transfer_families.front_end.members]]
selectors = { range = "full" }
status = "emit"
applicability = { observation = [1, 10] }

[[transfer_families.front_end.members]]
selectors = { range = "low" }
status = "emit"
applicability = { observation = [1, 5] }
"#;
        let defs = parse_family(toml).unwrap();
        let expanded = defs.resolved_transfers().unwrap();
        assert_eq!(
            expanded["front_end_range_full"]
                .points
                .as_ref()
                .unwrap()
                .len(),
            3
        );
        let low = expanded["front_end_range_low"].points.as_ref().unwrap();
        assert_eq!(low.len(), 2);
        assert_eq!(low[0].input, 1);
        assert_eq!(low[1].input, 5);
    }

    #[test]
    fn points_rejects_input_transform() {
        let error = parse_family(
            r#"
[transfer_families.front_end]
input_unit = "count"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
points = [
  { input = 1, output = 1.0 },
  { input = 10, output = 10.0 },
]

[[transfer_families.front_end.members]]
selectors = { range = "full" }
status = "emit"
input_transform = { numerator = 1, denominator = 1 }
applicability = { observation = [1, 10] }
"#,
        )
        .unwrap()
        .validate()
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("`input_transform` is not supported for points"),
            "{error}"
        );
    }

    #[test]
    fn scaled_polynomial_requires_transform_and_rejects_observation() {
        let missing = FamilyMemberDef {
            selectors: BTreeMap::from([("gain".into(), SelectorValue::String("div4".into()))]),
            input_transform: None,
            status: MemberStatus::Emit,
            reason: None,
            applicability: model_input([63.0, 100.0]),
        };
        let error = expand_families(&BTreeMap::from([(
            "als".into(),
            scaled_poly_family(vec![0.0, 1.0], vec![missing]),
        )]))
        .unwrap_err();
        assert!(error.contains("input_transform is required"), "{error}");

        let mut observation_member = scaled_emit("div4", 100, 33_600);
        observation_member.applicability = observation([1, 10]);
        let error = expand_families(&BTreeMap::from([(
            "als".into(),
            scaled_poly_family(vec![0.0, 1.0], vec![observation_member]),
        )]))
        .unwrap_err();
        assert!(
            error.contains("`applicability.observation` is not supported for scaled_polynomial"),
            "{error}"
        );
    }

    #[test]
    fn ntc_physical_applicability_sets_output_range() {
        let toml = r#"
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

[[transfer_families.ntc.members]]
selectors = { probe = "narrow" }
status = "emit"
applicability = { physical = [0.0, 40.0] }
"#;
        let defs = parse_family(toml).unwrap();
        let expanded = defs.resolved_transfers().unwrap();
        assert_eq!(expanded["ntc_probe_wide"].output_range, Some([-20.0, 80.0]));
        assert_eq!(expanded["ntc_probe_narrow"].output_range, Some([0.0, 40.0]));
    }

    #[test]
    fn ntc_rejects_model_input_and_transform() {
        let error = parse_family(
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
input_transform = { numerator = 1, denominator = 1 }
applicability = { physical = [-20.0, 80.0] }
"#,
        )
        .unwrap()
        .validate()
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("`input_transform` is not supported for ntc_beta_divider"),
            "{error}"
        );
    }

    #[test]
    fn emit_rejects_reason_and_non_emit_requires_reason() {
        let mut with_reason = emit_member("div4", 100);
        with_reason.reason = Some("should not be here".into());
        let error = expand_families(&BTreeMap::from([(
            "als".into(),
            formula_family(vec![with_reason]),
        )]))
        .unwrap_err();
        assert!(
            error.contains("reason is forbidden for status = \"emit\""),
            "{error}"
        );

        let mut missing = emit_member("div8", 100);
        missing.status = MemberStatus::Unsupported;
        missing.reason = None;
        let error = expand_families(&BTreeMap::from([(
            "als".into(),
            formula_family(vec![emit_member("div4", 100), missing]),
        )]))
        .unwrap_err();
        assert!(
            error.contains("status = \"unsupported\" requires a non-blank reason"),
            "{error}"
        );
    }

    #[test]
    fn unsupported_member_omits_source_mapping_and_remains_inspectable() {
        let toml = r#"
[transfer_families.als]
input_unit = "count"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
formula = "x"

[[transfer_families.als.members]]
selectors = { gain = "div4" }
status = "emit"
applicability = { observation = [1, 10] }

[[transfer_families.als.members]]
selectors = { gain = "x1" }
status = "unsupported"
reason = "the source does not define this selector combination"
"#;
        let defs = parse_family(toml).unwrap();
        let validated = defs.validate().unwrap();
        let family = &validated.families()[0];
        let unsupported = family
            .members()
            .iter()
            .find(|member| member.status() == MemberStatus::Unsupported)
            .unwrap();

        assert_eq!(unsupported.input_transform(), None);
        assert_eq!(unsupported.applicability().observation, None);
        assert_eq!(unsupported.applicability().model_input, None);
        assert_eq!(unsupported.applicability().physical, None);
        assert_eq!(validated.emitted_transfer_names().count(), 1);
    }

    #[test]
    fn unsupported_scaled_polynomial_member_needs_no_transform_or_applicability() {
        let unsupported = FamilyMemberDef {
            selectors: BTreeMap::from([("gain".into(), SelectorValue::String("x1".into()))]),
            input_transform: None,
            status: MemberStatus::Unsupported,
            reason: Some("the shared model has no mapping for this gain".into()),
            applicability: ApplicabilityDef::default(),
        };
        let expanded = expand_families(&BTreeMap::from([(
            "als".into(),
            scaled_poly_family(
                vec![0.0, 1.0],
                vec![scaled_emit("div4", 100, 33_600), unsupported],
            ),
        )]))
        .unwrap();

        assert_eq!(expanded.len(), 1);
        assert!(expanded.contains_key("als_gain_div4_integration_time_ms_100"));
    }

    #[test]
    fn unsupported_member_rejects_invented_source_mapping() {
        let mapped = described("x1", 100, MemberStatus::Unsupported);
        let error = expand_families(&BTreeMap::from([(
            "als".into(),
            formula_family(vec![emit_member("div4", 100), mapped]),
        )]))
        .unwrap_err();
        assert!(
            error.contains("`applicability.observation` is forbidden for status = \"unsupported\""),
            "{error}"
        );

        let mut transformed = FamilyMemberDef {
            selectors: BTreeMap::from([("gain".into(), SelectorValue::String("x1".into()))]),
            input_transform: Some(millionths(33_600)),
            status: MemberStatus::Unsupported,
            reason: Some("the shared model has no mapping for this gain".into()),
            applicability: ApplicabilityDef::default(),
        };
        let error = expand_families(&BTreeMap::from([(
            "als".into(),
            scaled_poly_family(
                vec![0.0, 1.0],
                vec![scaled_emit("div4", 100, 33_600), transformed.clone()],
            ),
        )]))
        .unwrap_err();
        assert!(
            error.contains("`input_transform` is forbidden for status = \"unsupported\""),
            "{error}"
        );

        transformed.input_transform = None;
        transformed.applicability = model_input([100.0, 22_000.0]);
        let error = expand_families(&BTreeMap::from([(
            "als".into(),
            scaled_poly_family(
                vec![0.0, 1.0],
                vec![scaled_emit("div4", 100, 33_600), transformed],
            ),
        )]))
        .unwrap_err();
        assert!(
            error.contains("`applicability.model_input` is forbidden for status = \"unsupported\""),
            "{error}"
        );
    }

    #[test]
    fn source_specific_field_error_precedes_missing_or_cardinality_errors() {
        let mut bad = emit_member("div4", 100);
        bad.applicability.physical = Some([1.0, 10.0]);
        let error = expand_families(&BTreeMap::from([("als".into(), formula_family(vec![bad]))]))
            .unwrap_err();

        assert!(
            error.contains("`applicability.physical` is not supported for formula sources"),
            "{error}"
        );
        assert!(!error.contains("exactly one"), "{error}");
    }

    #[test]
    fn member_scale_field_is_unknown() {
        let error = parse_family(
            r#"
[transfer_families.als]
input_unit = "count"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
formula = "x"

[[transfer_families.als.members]]
selectors = { gain = "div4" }
scale = 1
status = "emit"
applicability = { observation = [1, 10] }
"#,
        )
        .unwrap_err();
        assert!(error.contains("unknown field `scale`"), "{error}");
    }
}
