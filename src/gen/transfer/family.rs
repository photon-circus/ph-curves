//! Discrete transfer families: one shared source, explicit members, no interpolation.

// Host-only: module-local std link (crate root stays `#![no_std]`).
extern crate std;

use std::collections::{BTreeMap, BTreeSet};
use std::format;
use std::ops::Deref;
use std::prelude::v1::*;
use std::vec;

use serde::Deserialize;
use serde::de::{self, Deserializer, Visitor};

use super::model::ModelDef;
use super::{
    BoundaryDef, GenerationPolicy, ObservationGuardDef, PhysicalPoint, SourceProvenance,
    SourceProvenanceOverride, TransferDef, default_boundary, resolve_guard_provenance,
    validate_observation_guard,
};

/// Hard cap on knots for a family member. Stricter than the standalone
/// transfer cap (4096): families must not become dense ADC tables.
pub const FAMILY_MAX_KNOTS_HARD: usize = 256;

pub(crate) fn default_family_max_knots() -> usize {
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
/// Selectors are never interpolated. Every family declares its expected
/// selector universe with exactly one of `selector_axes` (Cartesian product)
/// or `expected_selectors` (explicit maps). Each expected identity is occupied
/// by exactly one member or family-scoped gap. Family-level `domain` and
/// `output_range` are rejected as unknown; members declare those coordinates
/// through `applicability`. `input_transform` is per-member only. A
/// family-level `scale` field is rejected as unknown so a shared-model scale
/// cannot be silently overwritten.
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
    /// Shared source citation. Required for a source-backed family.
    #[serde(default)]
    pub provenance: Option<SourceProvenance>,
    pub(crate) points: Option<Vec<PhysicalPoint>>,
    pub(crate) formula: Option<String>,
    pub(crate) model: Option<ModelDef>,
    /// Named selector axes whose Cartesian product is the expected universe.
    ///
    /// Mutually exclusive with [`Self::expected_selectors`]. Exactly one of
    /// the two must be set. The checked product of axis lengths must fit the
    /// generator host's `usize`.
    #[serde(default)]
    pub selector_axes: Option<BTreeMap<String, Vec<SelectorValue>>>,
    /// Explicit expected selector maps for a non-Cartesian family.
    ///
    /// Mutually exclusive with [`Self::selector_axes`]. Missing product cells
    /// are not invented.
    #[serde(default)]
    pub expected_selectors: Option<Vec<BTreeMap<String, SelectorValue>>>,
    /// Explicit selector combinations. Never synthesized.
    pub members: Vec<FamilyMemberDef>,
    /// Family-scoped gaps occupying expected selector identities.
    ///
    /// Distinct from document-level `[gaps]`, which are globally named and do
    /// not satisfy family completeness.
    #[serde(default)]
    pub gaps: Vec<FamilyGapDef>,
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
    pub fn observation_guard(&self) -> Option<&ObservationGuardDef> {
        self.observation_guard.as_ref()
    }

    /// Shared source citation, when declared. Validation requires this on a family.
    pub fn provenance(&self) -> Option<&SourceProvenance> {
        self.provenance.as_ref()
    }

    /// Fit budget, boundaries, and observation-guard classification.
    pub fn policy(&self) -> GenerationPolicy {
        GenerationPolicy::new(
            self.max_interpolation_error,
            self.max_knots,
            self.below,
            self.above,
            self.observation_guard.as_ref(),
        )
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
    /// Optional citation override. Unset fields inherit the family citation.
    #[serde(default)]
    pub provenance: Option<SourceProvenanceOverride>,
    /// Optional explicit table-name stem, independent of selector spelling.
    ///
    /// When omitted, the stem is derived from the family name and selector map.
    /// Description-only members may declare a stem, but it is not generated.
    #[serde(default)]
    pub emitted_name: Option<String>,
}

impl FamilyMemberDef {
    /// Override the derived table-name stem used when this member is emitted.
    pub fn with_emitted_name(mut self, name: impl Into<String>) -> Self {
        self.emitted_name = Some(name.into());
        self
    }
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

    fn type_name(&self) -> &'static str {
        match self {
            Self::Integer(_) => "integer",
            Self::String(_) => "string",
        }
    }
}

/// Declared expected selector identities for one family.
///
/// Cartesian axes expand to their product. Explicit maps are the universe as
/// written; missing product cells are not invented.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SelectorUniverse {
    /// Named axes whose Cartesian product is expected. Validated products have
    /// a cardinality representable by `usize`.
    Cartesian {
        /// Axis name → allowed typed values in declaration order.
        axes: BTreeMap<String, Vec<SelectorValue>>,
    },
    /// Explicit expected selector maps for a non-Cartesian family.
    Explicit {
        /// Expected identities in declaration order.
        identities: Vec<BTreeMap<String, SelectorValue>>,
    },
}

impl SelectorUniverse {
    /// Lazily enumerate every expected selector identity.
    ///
    /// Cartesian identities follow `BTreeMap` axis-key order, then each axis's
    /// declared value order, with the last axis changing fastest. Explicit
    /// identities keep declaration order. Constructing the iterator uses
    /// memory proportional to the number of axes; it never materializes the
    /// Cartesian product.
    pub fn identities(&self) -> SelectorIdentities<'_> {
        SelectorIdentities::new(self)
    }

    /// Checked number of expected identities.
    ///
    /// Explicit universes always return their list length. Cartesian
    /// universes return the checked product of their axis lengths, or `None`
    /// when that product cannot be represented by `usize`. Family validation
    /// rejects the overflow case, so a universe obtained from a validated
    /// family always returns `Some`.
    pub fn identity_count(&self) -> Option<usize> {
        match self {
            Self::Cartesian { axes } => {
                if axes.values().any(Vec::is_empty) {
                    return Some(0);
                }
                axes.values()
                    .try_fold(1usize, |count, values| count.checked_mul(values.len()))
            }
            Self::Explicit { identities } => Some(identities.len()),
        }
    }
}

/// Lazy iterator over a selector universe's expected identities.
///
/// The iterator owns only axis positions and produces one selector map at a
/// time. In particular, obtaining or partially consuming it cannot allocate
/// the full Cartesian product.
#[must_use = "iterators are lazy and do nothing unless consumed"]
pub struct SelectorIdentities<'a> {
    inner: SelectorIdentitiesInner<'a>,
}

enum SelectorIdentitiesInner<'a> {
    Cartesian(CartesianIdentities<'a>),
    Explicit(std::slice::Iter<'a, BTreeMap<String, SelectorValue>>),
}

impl<'a> SelectorIdentities<'a> {
    fn new(universe: &'a SelectorUniverse) -> Self {
        let inner = match universe {
            SelectorUniverse::Cartesian { axes } => {
                SelectorIdentitiesInner::Cartesian(CartesianIdentities::new(axes))
            }
            SelectorUniverse::Explicit { identities } => {
                SelectorIdentitiesInner::Explicit(identities.iter())
            }
        };
        Self { inner }
    }
}

impl Iterator for SelectorIdentities<'_> {
    type Item = BTreeMap<String, SelectorValue>;

    fn next(&mut self) -> Option<Self::Item> {
        match &mut self.inner {
            SelectorIdentitiesInner::Cartesian(iter) => iter.next(),
            SelectorIdentitiesInner::Explicit(iter) => iter.next().cloned(),
        }
    }
}

impl std::iter::FusedIterator for SelectorIdentities<'_> {}

struct CartesianIdentities<'a> {
    axes: Vec<(&'a str, &'a [SelectorValue])>,
    positions: Vec<usize>,
    finished: bool,
}

impl<'a> CartesianIdentities<'a> {
    fn new(axes: &'a BTreeMap<String, Vec<SelectorValue>>) -> Self {
        let axes: Vec<_> = axes
            .iter()
            .map(|(name, values)| (name.as_str(), values.as_slice()))
            .collect();
        let finished = axes.iter().any(|(_, values)| values.is_empty());
        let positions = vec![0; axes.len()];
        Self {
            axes,
            positions,
            finished,
        }
    }
}

impl Iterator for CartesianIdentities<'_> {
    type Item = BTreeMap<String, SelectorValue>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }

        let identity = self
            .axes
            .iter()
            .zip(&self.positions)
            .map(|((name, values), &position)| ((*name).to_string(), values[position].clone()))
            .collect();

        if self.axes.is_empty() {
            self.finished = true;
            return Some(identity);
        }

        for axis in (0..self.axes.len()).rev() {
            let next = self.positions[axis] + 1;
            if next < self.axes[axis].1.len() {
                self.positions[axis] = next;
                return Some(identity);
            }
            self.positions[axis] = 0;
        }
        self.finished = true;
        Some(identity)
    }
}

impl std::iter::FusedIterator for CartesianIdentities<'_> {}

#[derive(Clone, Copy, Default)]
struct SelectorTypeSet {
    integer: bool,
    string: bool,
}

impl SelectorTypeSet {
    fn add(&mut self, value: &SelectorValue) {
        match value {
            SelectorValue::Integer(_) => self.integer = true,
            SelectorValue::String(_) => self.string = true,
        }
    }

    fn matches(self, value: &SelectorValue) -> bool {
        match value {
            SelectorValue::Integer(_) => self.integer,
            SelectorValue::String(_) => self.string,
        }
    }

    fn is_homogeneous(self) -> bool {
        self.integer != self.string
    }

    fn name(self) -> &'static str {
        match (self.integer, self.string) {
            (true, false) => "integer",
            (false, true) => "string",
            (true, true) => "integer or string",
            (false, false) => "no declared type",
        }
    }
}

struct SelectorUniverseIndex {
    keys: BTreeSet<String>,
    types: BTreeMap<String, SelectorTypeSet>,
    membership: SelectorMembershipIndex,
    identity_count: usize,
}

enum SelectorMembershipIndex {
    Cartesian(BTreeMap<String, BTreeSet<SelectorValue>>),
    Explicit(BTreeSet<BTreeMap<String, SelectorValue>>),
}

impl SelectorUniverseIndex {
    fn new(universe: &SelectorUniverse) -> Self {
        match universe {
            SelectorUniverse::Cartesian { axes } => {
                let mut types = BTreeMap::new();
                let membership = axes
                    .iter()
                    .map(|(key, values)| {
                        let mut value_types = SelectorTypeSet::default();
                        for value in values {
                            value_types.add(value);
                        }
                        types.insert(key.clone(), value_types);
                        (key.clone(), values.iter().cloned().collect())
                    })
                    .collect();
                Self {
                    keys: axes.keys().cloned().collect(),
                    types,
                    membership: SelectorMembershipIndex::Cartesian(membership),
                    identity_count: universe
                        .identity_count()
                        .expect("validated Cartesian cardinality"),
                }
            }
            SelectorUniverse::Explicit { identities } => {
                let keys = identities
                    .first()
                    .map(|identity| identity.keys().cloned().collect())
                    .unwrap_or_default();
                let mut types: BTreeMap<String, SelectorTypeSet> = BTreeMap::new();
                for identity in identities {
                    for (key, value) in identity {
                        types.entry(key.clone()).or_default().add(value);
                    }
                }
                Self {
                    keys,
                    types,
                    membership: SelectorMembershipIndex::Explicit(
                        identities.iter().cloned().collect(),
                    ),
                    identity_count: identities.len(),
                }
            }
        }
    }

    fn types_for(&self, key: &str) -> SelectorTypeSet {
        self.types.get(key).copied().unwrap_or_default()
    }

    fn contains(&self, selectors: &BTreeMap<String, SelectorValue>) -> bool {
        match &self.membership {
            SelectorMembershipIndex::Cartesian(axes) => {
                selectors.len() == axes.len()
                    && selectors.iter().all(|(key, value)| {
                        axes.get(key).is_some_and(|values| values.contains(value))
                    })
            }
            SelectorMembershipIndex::Explicit(identities) => identities.contains(selectors),
        }
    }
}

/// Completeness of a validated family's selector occupancy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FamilyCompleteness {
    /// Every declared expected selector identity is occupied by exactly one
    /// member or family-scoped gap. Validation proves this from exact indexed
    /// membership, duplicate rejection, and checked cardinality equality.
    Complete,
}

/// A selector-addressed hole in a family's declared universe.
///
/// Distinct from document-level [`GapDef`]: this record carries the same typed
/// selector map as a member and occupies that expected identity. A non-blank
/// `reason` is required. Its optional citation override resolves against the
/// mandatory family citation. Global `[gaps]` do not satisfy family
/// completeness.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FamilyGapDef {
    /// Discrete selector map. Keys and typed values form the gap identity.
    pub selectors: BTreeMap<String, SelectorValue>,
    /// Must be `"undefined"`: a gap must not be filled with a plausible model.
    pub status: GapStatus,
    /// Non-blank explanation of why this expected identity is not a member.
    pub reason: String,
    /// Optional citation override. Unset fields inherit the family citation.
    #[serde(default)]
    pub provenance: Option<SourceProvenanceOverride>,
}

/// A channel or procedure the sources do not define as a transfer.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GapDef {
    /// Must be `"undefined"`: a gap must not be filled with a plausible model.
    pub status: GapStatus,
    /// Non-blank explanation of why the mapping is undefined.
    pub reason: String,
    /// Optional source citation for this gap.
    #[serde(default)]
    pub provenance: Option<SourceProvenanceOverride>,
}

/// Only `undefined` is valid: a gap must not be filled with a plausible model.
#[derive(Debug, Clone, Copy, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum GapStatus {
    /// The sources do not define this mapping; do not invent one.
    Undefined,
}

/// Family identity retained through expansion so generation reports do not
/// reconstruct members by matching resolved emitted names.
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

impl GapDef {
    /// Declared citation override, when present.
    pub fn provenance(&self) -> Option<&SourceProvenanceOverride> {
        self.provenance.as_ref()
    }

    /// Resolved citation. `None` when no override was declared.
    pub fn resolved_provenance(&self) -> Result<Option<SourceProvenance>, String> {
        match &self.provenance {
            Some(overlay) => overlay.resolve(None).map(Some),
            None => Ok(None),
        }
    }
}

/// Expand families into standalone transfer defs.
///
/// Every member's identity, status, provenance, source-specific fields, and
/// applicability window are validated before non-`emit` statuses are
/// filtered. Window derivation may include limited numerical model evaluation.
/// Only `emit` members are lowered into transfer definitions, swept for
/// full-window monotonicity, fitted, error-measured, and generated;
/// description-only mappings are not generation inputs.
pub(crate) fn expand_families(
    families: &BTreeMap<String, TransferFamilyDef>,
) -> Result<BTreeMap<String, ResolvedTransfer>, String> {
    let mut out: BTreeMap<String, ResolvedTransfer> = BTreeMap::new();
    for (family_name, family) in families {
        validate_family(family_name, family)?;

        let mut generated = 0usize;
        for member in &family.members {
            if member.status != MemberStatus::Emit {
                continue;
            }
            let member_name = member_emitted_name(family_name, member)?;
            let def = member_transfer(family_name, family, member)?;
            let resolved = ResolvedTransfer {
                def,
                origin: Some(FamilyMemberOrigin {
                    family: family_name.clone(),
                    selectors: member.selectors.clone(),
                }),
            };
            if let Some(previous) = out.get(&member_name) {
                let previous_origin = previous
                    .origin
                    .as_ref()
                    .expect("emitted family members retain their origin");
                let origin = resolved
                    .origin
                    .as_ref()
                    .expect("emitted family members retain their origin");
                return Err(format!(
                    "resolved emitted name `{member_name}` collides between transfer family `{}` \
                     selectors {} and transfer family `{}` selectors {}",
                    previous_origin.family,
                    format_selectors(&previous_origin.selectors),
                    origin.family,
                    format_selectors(&origin.selectors)
                ));
            }
            out.insert(member_name, resolved);
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
    let provenance = family.provenance.as_ref().ok_or_else(|| {
        format!(
            "transfer family `{family_name}`: source-backed family requires provenance.identity"
        )
    })?;
    provenance
        .validate()
        .map_err(|error| format!("transfer family `{family_name}`: {error}"))?;

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
    for (index, member) in family.members.iter().enumerate() {
        validate_member(family_name, family, index, member)?;
        if let Some(&previous) = seen_identities.get(&member.selectors) {
            return Err(format!(
                "transfer family `{family_name}`: members {previous} and {index} share selector identity {}",
                format_selectors(&member.selectors)
            ));
        }
        seen_identities.insert(&member.selectors, index);
    }

    let universe = resolved_selector_universe(family_name, family)?;
    let universe_index = SelectorUniverseIndex::new(&universe);

    let mut seen_emitted_names: BTreeMap<String, String> = BTreeMap::new();
    for (index, member) in family.members.iter().enumerate() {
        validate_identity_in_universe(
            &member_label(family_name, index),
            family_name,
            &member.selectors,
            &universe_index,
        )?;
        if member.status == MemberStatus::Emit {
            let member_name = member_emitted_name(family_name, member)?;
            if let Some(previous) =
                seen_emitted_names.insert(member_name.clone(), format_selectors(&member.selectors))
            {
                return Err(format!(
                    "transfer family `{family_name}`: selector maps {previous} and {} \
                     both resolve to `{member_name}`",
                    format_selectors(&member.selectors)
                ));
            }
        }
    }

    let mut seen_gap_identities: BTreeMap<&BTreeMap<String, SelectorValue>, usize> =
        BTreeMap::new();
    for (index, gap) in family.gaps.iter().enumerate() {
        validate_family_gap(family_name, family, index, gap)?;
        if let Some(&previous) = seen_gap_identities.get(&gap.selectors) {
            return Err(format!(
                "transfer family `{family_name}`: family-scoped gaps {previous} and {index} \
                 share selector identity {}",
                format_selectors(&gap.selectors)
            ));
        }
        seen_gap_identities.insert(&gap.selectors, index);
        if let Some(&member_index) = seen_identities.get(&gap.selectors) {
            return Err(format!(
                "transfer family `{family_name}`: member {member_index} and family-scoped gap {index} \
                 share selector identity {}",
                format_selectors(&gap.selectors)
            ));
        }
        validate_identity_in_universe(
            &gap_label(family_name, index),
            family_name,
            &gap.selectors,
            &universe_index,
        )?;
    }

    validate_completeness(family_name, family, &universe_index)?;
    Ok(())
}

/// Resolve the declared selector universe. Callers may use this after
/// `validate_family` has succeeded; it re-checks declaration shape.
pub(crate) fn resolved_selector_universe(
    family_name: &str,
    family: &TransferFamilyDef,
) -> Result<SelectorUniverse, String> {
    let universe = match (
        family.selector_axes.as_ref(),
        family.expected_selectors.as_ref(),
    ) {
        (None, None) => {
            return Err(format!(
                "transfer family `{family_name}`: exactly one of selector_axes or expected_selectors \
             must be specified"
            ));
        }
        (Some(_), Some(_)) => {
            return Err(format!(
                "transfer family `{family_name}`: selector_axes and expected_selectors are mutually exclusive"
            ));
        }
        (Some(axes), None) => SelectorUniverse::Cartesian {
            axes: validate_selector_axes(family_name, axes)?,
        },
        (None, Some(identities)) => SelectorUniverse::Explicit {
            identities: validate_expected_selectors(family_name, identities)?,
        },
    };
    if universe.identity_count().is_none() {
        return Err(format!(
            "transfer family `{family_name}`: selector_axes Cartesian product cardinality \
             exceeds this platform's usize capacity"
        ));
    }
    Ok(universe)
}

fn validate_selector_axes(
    family_name: &str,
    axes: &BTreeMap<String, Vec<SelectorValue>>,
) -> Result<BTreeMap<String, Vec<SelectorValue>>, String> {
    if axes.is_empty() {
        return Err(format!(
            "transfer family `{family_name}`: selector_axes must not be empty"
        ));
    }
    for (name, values) in axes {
        if name.trim().is_empty() {
            return Err(format!(
                "transfer family `{family_name}`: selector axis names must not be blank"
            ));
        }
        if values.is_empty() {
            return Err(format!(
                "transfer family `{family_name}`: selector axis `{name}` must not be empty"
            ));
        }
        let mut seen = BTreeSet::new();
        for value in values {
            if let SelectorValue::String(text) = value
                && text.trim().is_empty()
            {
                return Err(format!(
                    "transfer family `{family_name}`: selector axis `{name}` must not contain a blank string"
                ));
            }
            if !seen.insert(value) {
                return Err(format!(
                    "transfer family `{family_name}`: selector axis `{name}` repeats value {}",
                    format_selector_value(value)
                ));
            }
        }
    }
    Ok(axes.clone())
}

fn validate_expected_selectors(
    family_name: &str,
    identities: &[BTreeMap<String, SelectorValue>],
) -> Result<Vec<BTreeMap<String, SelectorValue>>, String> {
    if identities.is_empty() {
        return Err(format!(
            "transfer family `{family_name}`: expected_selectors must not be empty"
        ));
    }
    let expected_keys: BTreeSet<String> = identities[0].keys().cloned().collect();
    if expected_keys.is_empty() {
        return Err(format!(
            "transfer family `{family_name}`: expected selector 0 must not be empty"
        ));
    }
    if expected_keys.iter().any(|key| key.trim().is_empty()) {
        return Err(format!(
            "transfer family `{family_name}`: expected selector 0 keys must not be blank"
        ));
    }
    let mut seen: BTreeMap<&BTreeMap<String, SelectorValue>, usize> = BTreeMap::new();
    for (index, identity) in identities.iter().enumerate() {
        if identity.is_empty() {
            return Err(format!(
                "transfer family `{family_name}`: expected selector {index} must not be empty"
            ));
        }
        for (key, value) in identity {
            if key.trim().is_empty() {
                return Err(format!(
                    "transfer family `{family_name}`: expected selector {index} keys must not be blank"
                ));
            }
            if let SelectorValue::String(text) = value
                && text.trim().is_empty()
            {
                return Err(format!(
                    "transfer family `{family_name}`: expected selector {index} `{key}` must not be blank"
                ));
            }
        }
        let keys: BTreeSet<String> = identity.keys().cloned().collect();
        if keys != expected_keys {
            return Err(format!(
                "transfer family `{family_name}`: expected selector {index} keys {} do not match \
                 selector 0 keys {}",
                format_keys(&keys),
                format_keys(&expected_keys)
            ));
        }
        if let Some(&previous) = seen.get(identity) {
            return Err(format!(
                "transfer family `{family_name}`: expected selectors {previous} and {index} share identity {}",
                format_selectors(identity)
            ));
        }
        seen.insert(identity, index);
    }
    Ok(identities.to_vec())
}

fn validate_family_gap(
    family_name: &str,
    family: &TransferFamilyDef,
    index: usize,
    gap: &FamilyGapDef,
) -> Result<(), String> {
    let label = gap_label(family_name, index);
    if gap.selectors.is_empty() {
        return Err(format!("{label}: selectors must not be empty"));
    }
    for (key, value) in &gap.selectors {
        if key.trim().is_empty() {
            return Err(format!("{label}: selector keys must not be blank"));
        }
        if let SelectorValue::String(text) = value
            && text.trim().is_empty()
        {
            return Err(format!("{label}: selector `{key}` must not be blank"));
        }
    }
    if gap.reason.trim().is_empty() {
        return Err(format!("{label}: reason must not be blank"));
    }
    resolved_family_gap_provenance(family_name, family, index, gap)?;
    Ok(())
}

fn validate_completeness(
    family_name: &str,
    family: &TransferFamilyDef,
    universe: &SelectorUniverseIndex,
) -> Result<(), String> {
    let occupied_count = family
        .members
        .len()
        .checked_add(family.gaps.len())
        .ok_or_else(|| {
            format!("transfer family `{family_name}`: selector occupancy count exceeds usize")
        })?;
    if occupied_count != universe.identity_count {
        return Err(format!(
            "transfer family `{family_name}`: declared selector universe contains {} identities, \
             but members and family-scoped gaps occupy {occupied_count}",
            universe.identity_count
        ));
    }
    Ok(())
}

fn validate_identity_in_universe(
    label: &str,
    family_name: &str,
    selectors: &BTreeMap<String, SelectorValue>,
    universe: &SelectorUniverseIndex,
) -> Result<(), String> {
    let actual_keys: BTreeSet<String> = selectors.keys().cloned().collect();
    if actual_keys != universe.keys {
        return Err(format!(
            "{label}: selector keys {} do not match the declared universe keys {}",
            format_keys(&actual_keys),
            format_keys(&universe.keys)
        ));
    }
    for (key, value) in selectors {
        let allowed = universe.types_for(key);
        if allowed.is_homogeneous() && !allowed.matches(value) {
            return Err(format!(
                "{label}: selector `{key}` has a {} value; the declared universe uses {}",
                value.type_name(),
                allowed.name()
            ));
        }
    }
    if !universe.contains(selectors) {
        return Err(format!(
            "transfer family `{family_name}`: selector identity {} is outside the declared universe",
            format_selectors(selectors)
        ));
    }
    Ok(())
}

fn format_keys(keys: &BTreeSet<String>) -> String {
    if keys.is_empty() {
        return "(none)".into();
    }
    let parts: Vec<String> = keys.iter().map(|key| format!("`{key}`")).collect();
    parts.join(", ")
}

fn format_selector_value(value: &SelectorValue) -> String {
    match value {
        SelectorValue::String(text) => format!("{text:?}"),
        SelectorValue::Integer(int) => int.to_string(),
    }
}

fn member_label(family_name: &str, index: usize) -> String {
    format!("transfer family `{family_name}` member {index}")
}

fn gap_label(family_name: &str, index: usize) -> String {
    format!("transfer family `{family_name}` family-scoped gap {index}")
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
    if let Some(name) = &member.emitted_name
        && name.trim().is_empty()
    {
        return Err(format!("{label}: emitted_name must not be blank"));
    }
    for (key, value) in &member.selectors {
        if key.trim().is_empty() {
            return Err(format!("{label}: selector keys must not be blank"));
        }
        if let SelectorValue::String(text) = value
            && text.trim().is_empty()
        {
            return Err(format!("{label}: selector `{key}` must not be blank"));
        }
    }
    validate_status_reason(&label, member)?;
    if let Some(overlay) = &member.provenance {
        family
            .provenance
            .as_ref()
            .ok_or_else(|| {
                format!(
                    "transfer family `{family_name}`: source-backed family requires provenance.identity"
                )
            })?
            .merge(overlay)
            .map_err(|error| format!("{label}: {error}"))?;
    }

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
            validate_observation_guard(&label, family.observation_guard.as_ref(), domain[1])?;
            resolve_guard_provenance(
                &label,
                family.observation_guard.as_ref(),
                family.provenance.as_ref(),
            )?;
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
                family.observation_guard.as_ref(),
                clipped.last().expect("clip requires two points").input,
            )?;
            resolve_guard_provenance(
                &label,
                family.observation_guard.as_ref(),
                family.provenance.as_ref(),
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
            validate_observation_guard(&label, family.observation_guard.as_ref(), domain[1])?;
            resolve_guard_provenance(
                &label,
                family.observation_guard.as_ref(),
                family.provenance.as_ref(),
            )?;
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
            validate_observation_guard(&label, family.observation_guard.as_ref(), domain_max)?;
            resolve_guard_provenance(
                &label,
                family.observation_guard.as_ref(),
                family.provenance.as_ref(),
            )?;
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

pub(crate) fn member_transfer(
    family_name: &str,
    family: &TransferFamilyDef,
    member: &FamilyMemberDef,
) -> Result<TransferDef, String> {
    let resolved_guard_provenance = resolve_guard_provenance(
        &format!("transfer family `{family_name}`"),
        family.observation_guard.as_ref(),
        family.provenance.as_ref(),
    )?;
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
        observation_guard: family.observation_guard.clone(),
        provenance: Some(resolved_member_provenance(family_name, family, member)?),
        resolved_guard_provenance,
        points,
        formula: family.formula.clone(),
        model,
        domain,
        output_range,
    })
}

pub(crate) fn resolved_member_provenance(
    family_name: &str,
    family: &TransferFamilyDef,
    member: &FamilyMemberDef,
) -> Result<SourceProvenance, String> {
    let base = family.provenance.as_ref().ok_or_else(|| {
        format!(
            "transfer family `{family_name}`: source-backed family requires provenance.identity"
        )
    })?;
    match &member.provenance {
        Some(overlay) => base
            .merge(overlay)
            .map_err(|error| format!("transfer family `{family_name}`: {error}")),
        None => {
            base.validate()
                .map_err(|error| format!("transfer family `{family_name}`: {error}"))?;
            Ok(base.clone())
        }
    }
}

pub(crate) fn resolved_family_gap_provenance(
    family_name: &str,
    family: &TransferFamilyDef,
    index: usize,
    gap: &FamilyGapDef,
) -> Result<SourceProvenance, String> {
    let label = gap_label(family_name, index);
    let base = family.provenance.as_ref().ok_or_else(|| {
        format!(
            "transfer family `{family_name}`: source-backed family requires provenance.identity"
        )
    })?;
    match &gap.provenance {
        Some(overlay) => base
            .merge(overlay)
            .map_err(|error| format!("{label}: {error}")),
        None => {
            base.validate()
                .map_err(|error| format!("{label}: {error}"))?;
            Ok(base.clone())
        }
    }
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

/// Resolved table-name stem: explicit `emitted_name` or the derived expansion.
pub(crate) fn member_emitted_name(
    family_name: &str,
    member: &FamilyMemberDef,
) -> Result<String, String> {
    match &member.emitted_name {
        Some(name) if name.trim().is_empty() => Err(format!(
            "transfer family `{family_name}`: emitted_name must not be blank"
        )),
        Some(name) => Ok(name.clone()),
        None => expanded_name(family_name, &member.selectors),
    }
}

/// Resolved observation-code span for a source-mapped member.
pub(crate) fn member_observation_domain(
    family_name: &str,
    family: &TransferFamilyDef,
    member: &FamilyMemberDef,
) -> Result<Option<[u16; 2]>, String> {
    if member.status == MemberStatus::Unsupported {
        return Ok(None);
    }
    let stem = member_emitted_name(family_name, member)?;
    let def = member_transfer(family_name, family, member)?;
    super::family_source_observation_domain(&stem, &def).map(Some)
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
            provenance: None,
            emitted_name: None,
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
            provenance: None,
            emitted_name: None,
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
            provenance: Some(SourceProvenance::new("test fixture")),
            points: None,
            formula: Some("x".into()),
            model: None,
            selector_axes: None,
            expected_selectors: expected_from_members(&members, &[]),
            members,
            gaps: Vec::new(),
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
            provenance: Some(SourceProvenance::new("test fixture")),
            points: None,
            formula: None,
            model: Some(ModelDef::ScaledPolynomial {
                coefficients,
                scale: None,
                denominator: 1_000_000,
            }),
            selector_axes: None,
            expected_selectors: expected_from_members(&members, &[]),
            members,
            gaps: Vec::new(),
        }
    }

    fn expected_from_members(
        members: &[FamilyMemberDef],
        gaps: &[FamilyGapDef],
    ) -> Option<Vec<BTreeMap<String, SelectorValue>>> {
        let mut identities = Vec::new();
        for selectors in members
            .iter()
            .map(|member| &member.selectors)
            .chain(gaps.iter().map(|gap| &gap.selectors))
        {
            if selectors.is_empty() {
                continue;
            }
            if !identities.iter().any(|identity| identity == selectors) {
                identities.push(selectors.clone());
            }
        }
        if identities.is_empty() {
            identities.push(BTreeMap::from([(
                "gain".into(),
                SelectorValue::String("div4".into()),
            )]));
        }
        Some(identities)
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
provenance = { identity = "test fixture" }
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
    fn whitespace_only_selector_keys_and_values_are_rejected() {
        let mut blank_key = emit_member("div4", 100);
        blank_key.selectors =
            BTreeMap::from([("   ".into(), SelectorValue::String("value".into()))]);
        let error = expand_families(&BTreeMap::from([(
            "als".into(),
            formula_family(vec![blank_key]),
        )]))
        .unwrap_err();
        assert!(error.contains("selector keys must not be blank"), "{error}");

        let mut blank_value = emit_member("div4", 100);
        blank_value.selectors =
            BTreeMap::from([("gain".into(), SelectorValue::String(" \t ".into()))]);
        let error = expand_families(&BTreeMap::from([(
            "als".into(),
            formula_family(vec![blank_value]),
        )]))
        .unwrap_err();
        assert!(
            error.contains("selector `gain` must not be blank"),
            "{error}"
        );

        let axes = BTreeMap::from([(" \t ".into(), vec![SelectorValue::String("value".into())])]);
        let error = validate_selector_axes("als", &axes).unwrap_err();
        assert!(error.contains("axis names must not be blank"), "{error}");

        let identities = vec![BTreeMap::from([(
            "gain".into(),
            SelectorValue::String("   ".into()),
        )])];
        let error = validate_expected_selectors("als", &identities).unwrap_err();
        assert!(error.contains("`gain` must not be blank"), "{error}");

        let family = formula_family(vec![emit_member("div4", 100)]);
        let gap = FamilyGapDef {
            selectors: BTreeMap::from([("gain".into(), SelectorValue::String("   ".into()))]),
            status: GapStatus::Undefined,
            reason: "not characterized".into(),
            provenance: None,
        };
        let error = validate_family_gap("als", &family, 0, &gap).unwrap_err();
        assert!(
            error.contains("selector `gain` must not be blank"),
            "{error}"
        );
    }

    #[test]
    fn nonblank_selector_whitespace_remains_part_of_exact_identity() {
        let axes = BTreeMap::from([(" gain ".into(), vec![SelectorValue::String(" x1 ".into())])]);
        let validated = validate_selector_axes("als", &axes).unwrap();
        assert_eq!(validated, axes);
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
        assert!(error.contains("both resolve to `als_a_1`"));
        assert!(error.contains("a=\"1\""));
        assert!(error.contains("a=1"));
    }

    #[test]
    fn description_only_members_may_share_an_expanded_name() {
        let mut emitted = emit_member("div4", 100);
        emitted.selectors = BTreeMap::from([("a".into(), SelectorValue::String("div4".into()))]);
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
        assert!(expanded.contains_key("als_a_div4"));
        assert!(!expanded.contains_key("als_a_1"));
    }

    #[test]
    fn description_only_formula_mapping_is_not_evaluated_or_fitted() {
        let mut emitted = emit_member("safe", 100);
        emitted.applicability = observation([1, 4]);
        let mut described = described("hazard", 100, MemberStatus::Unnecessary);
        described.applicability = observation([4, 6]);
        let mut family = formula_family(vec![emitted, described]);
        family.formula = Some("1 / (x - 5)".into());

        // The description-only window includes x=5, so treating it as a
        // generation input would fail numerical evaluation.
        let described_def = member_transfer("als", &family, &family.members[1]).unwrap();
        let error = super::super::build("description_only_probe", &described_def).unwrap_err();
        assert!(error.contains("non-finite output"), "{error}");

        let expanded = expand_families(&BTreeMap::from([("als".into(), family)])).unwrap();
        assert_eq!(expanded.len(), 1);
        let emitted = &expanded["als_gain_safe_integration_time_ms_100"];
        super::super::build("emitted", emitted).unwrap();
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
    fn member_missing_a_declared_selector_key_is_rejected() {
        let mut first = emit_member("div4", 100);
        first.selectors = BTreeMap::from([
            ("a".into(), SelectorValue::String("x".into())),
            ("b".into(), SelectorValue::String("y".into())),
        ]);
        let mut second = emit_member("div8", 100);
        second.selectors = BTreeMap::from([("a".into(), SelectorValue::String("x_b_y".into()))]);
        let mut family = formula_family(vec![first, second]);
        family.expected_selectors = None;
        family.selector_axes = Some(BTreeMap::from([
            ("a".into(), vec![SelectorValue::String("x".into())]),
            ("b".into(), vec![SelectorValue::String("y".into())]),
        ]));
        let error = expand_families(&BTreeMap::from([("als".into(), family)])).unwrap_err();
        assert!(error.contains("selector keys"), "{error}");
        assert!(error.contains("`b`"), "{error}");
    }

    #[test]
    fn value_only_concatenation_is_not_the_identity() {
        let mut keyed = emit_member("div4", 100);
        keyed.selectors = BTreeMap::from([
            ("gain".into(), SelectorValue::String("div4".into())),
            ("t".into(), SelectorValue::Integer(100)),
        ]);
        let mut distinct = emit_member("div8", 200);
        distinct.selectors = BTreeMap::from([
            ("gain".into(), SelectorValue::String("div4_t".into())),
            ("t".into(), SelectorValue::Integer(100)),
        ]);
        let mut families = BTreeMap::new();
        families.insert("als".into(), formula_family(vec![keyed, distinct]));
        let expanded = expand_families(&families).unwrap();
        assert!(expanded.contains_key("als_gain_div4_t_100"));
        assert!(expanded.contains_key("als_gain_div4_t_t_100"));
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
        let document = format!("[transfers]\nrequires = [\"transfer_families_v1\"]\n\n{toml}");
        DefinitionsFile::from_toml_str(&document).map_err(|error| error.to_string())
    }

    #[test]
    fn unknown_family_field_is_rejected() {
        let error = parse_family(
            r#"
[transfer_families.als]
provenance = { identity = "test fixture" }
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
provenance = { identity = "test fixture" }
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
provenance = { identity = "test fixture" }
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
provenance = { identity = "test fixture" }
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
provenance = { identity = "test fixture" }
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
            provenance: None,
        });
        let mut families = BTreeMap::new();
        families.insert("als".into(), family);
        let expanded = expand_families(&families).unwrap();
        let def = &expanded["als_gain_div4_integration_time_ms_100"];
        assert_eq!(def.domain, Some([1, 10]));
        assert_eq!(def.observation_guard.as_ref().unwrap().code, 65_535);
        assert_eq!(
            def.observation_guard.as_ref().unwrap().behavior,
            ObservationGuardBehaviorDef::Error
        );
    }

    #[test]
    fn saturation_code_inside_domain_is_rejected() {
        let defs = parse_family(
            r#"
[transfer_families.als]
provenance = { identity = "test fixture" }
input_unit = "count"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
formula = "x"
saturation = { code = 10, behavior = "error" }
selector_axes = { gain = ["div4"] }

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
provenance = { identity = "test fixture" }
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
provenance = { identity = "test fixture" }
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
            provenance: None,
        });
        let mut families = BTreeMap::new();
        families.insert("als".into(), family);
        let expanded = expand_families(&families).unwrap();
        let (name, def) = expanded.iter().next().unwrap();
        assert_eq!(def.observation_guard.as_ref().unwrap().code, 65_535);
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
provenance = { identity = "test fixture" }
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
provenance = { identity = "test fixture" }
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
provenance = { identity = "test fixture" }
input_unit = "count"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
selector_axes = { range = ["full", "low"] }
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
provenance = { identity = "test fixture" }
input_unit = "count"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
selector_axes = { range = ["full"] }
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
            provenance: None,
            emitted_name: None,
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
provenance = { identity = "test fixture" }
input_unit = "adc_code"
output_unit = "degree_celsius"
output_scale = 1000
max_interpolation_error = 50
selector_axes = { probe = ["wide", "narrow"] }

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
provenance = { identity = "test fixture" }
input_unit = "count"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
formula = "x"
selector_axes = { gain = ["div4", "x1"] }

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
            selectors: BTreeMap::from([
                ("gain".into(), SelectorValue::String("x1".into())),
                ("integration_time_ms".into(), SelectorValue::Integer(100)),
            ]),
            input_transform: None,
            status: MemberStatus::Unsupported,
            reason: Some("the shared model has no mapping for this gain".into()),
            applicability: ApplicabilityDef::default(),
            provenance: None,
            emitted_name: None,
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
            selectors: BTreeMap::from([
                ("gain".into(), SelectorValue::String("x1".into())),
                ("integration_time_ms".into(), SelectorValue::Integer(100)),
            ]),
            input_transform: Some(millionths(33_600)),
            status: MemberStatus::Unsupported,
            reason: Some("the shared model has no mapping for this gain".into()),
            applicability: ApplicabilityDef::default(),
            provenance: None,
            emitted_name: None,
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
provenance = { identity = "test fixture" }
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

    fn two_axis(range: &str, gain: i64) -> BTreeMap<String, SelectorValue> {
        BTreeMap::from([
            ("range".into(), SelectorValue::String(range.into())),
            ("gain".into(), SelectorValue::Integer(gain)),
        ])
    }

    fn two_axis_emit(range: &str, gain: i64) -> FamilyMemberDef {
        let mut member = emit_member("div4", 100);
        member.selectors = two_axis(range, gain);
        member
    }

    fn two_axis_axes() -> BTreeMap<String, Vec<SelectorValue>> {
        BTreeMap::from([
            (
                "range".into(),
                vec![
                    SelectorValue::String("low".into()),
                    SelectorValue::String("high".into()),
                ],
            ),
            (
                "gain".into(),
                vec![SelectorValue::Integer(1), SelectorValue::Integer(8)],
            ),
        ])
    }

    fn cartesian_family(
        members: Vec<FamilyMemberDef>,
        gaps: Vec<FamilyGapDef>,
    ) -> TransferFamilyDef {
        let mut family = formula_family(members);
        family.selector_axes = Some(two_axis_axes());
        family.expected_selectors = None;
        family.gaps = gaps;
        family
    }

    fn binary_axes(axis_count: usize) -> BTreeMap<String, Vec<SelectorValue>> {
        (0..axis_count)
            .map(|index| {
                (
                    format!("axis_{index:03}"),
                    vec![SelectorValue::Integer(0), SelectorValue::Integer(1)],
                )
            })
            .collect()
    }

    #[test]
    fn omitted_cartesian_cell_is_rejected() {
        let family = cartesian_family(
            vec![
                two_axis_emit("low", 1),
                two_axis_emit("high", 1),
                two_axis_emit("high", 8),
            ],
            Vec::new(),
        );
        let error = expand_families(&BTreeMap::from([("front_end".into(), family)])).unwrap_err();
        assert!(error.contains("contains 4 identities"), "{error}");
        assert!(error.contains("occupy 3"), "{error}");
    }

    #[test]
    fn cartesian_cardinality_overflow_is_rejected_without_expansion() {
        let axes = binary_axes(usize::BITS as usize);
        let mut member = emit_member("div4", 100);
        member.selectors = axes
            .keys()
            .map(|key| (key.clone(), SelectorValue::Integer(0)))
            .collect();
        let mut family = formula_family(vec![member]);
        family.selector_axes = Some(axes);
        family.expected_selectors = None;

        let error = expand_families(&BTreeMap::from([("front_end".into(), family)])).unwrap_err();
        assert!(error.contains("Cartesian product cardinality"), "{error}");
        assert!(error.contains("usize capacity"), "{error}");
    }

    #[test]
    fn cartesian_identity_enumeration_is_lazy_even_when_cardinality_overflows() {
        let axis_count = usize::BITS as usize;
        let universe = SelectorUniverse::Cartesian {
            axes: binary_axes(axis_count),
        };
        assert_eq!(universe.identity_count(), None);

        let identities: Vec<_> = universe.identities().take(3).collect();
        assert_eq!(identities.len(), 3);
        assert!(
            identities[0]
                .values()
                .all(|value| value == &SelectorValue::Integer(0))
        );
        assert_eq!(
            identities[1][&format!("axis_{:03}", axis_count - 1)],
            SelectorValue::Integer(1)
        );
        assert_eq!(
            identities[2][&format!("axis_{:03}", axis_count - 2)],
            SelectorValue::Integer(1)
        );
        assert_eq!(
            identities[2][&format!("axis_{:03}", axis_count - 1)],
            SelectorValue::Integer(0)
        );
    }

    #[test]
    fn empty_axis_makes_cardinality_zero_after_an_overflowing_prefix() {
        let mut axes = binary_axes(usize::BITS as usize);
        axes.insert("zzz_empty".into(), Vec::new());
        let universe = SelectorUniverse::Cartesian { axes };

        assert_eq!(universe.identity_count(), Some(0));
        assert_eq!(universe.identities().count(), 0);
    }

    #[test]
    fn selector_addressed_gap_makes_the_cartesian_family_complete() {
        let gap = FamilyGapDef {
            selectors: two_axis("low", 8),
            status: GapStatus::Undefined,
            reason: "not characterized at this combination".into(),
            provenance: None,
        };
        let family = cartesian_family(
            vec![
                two_axis_emit("low", 1),
                two_axis_emit("high", 1),
                two_axis_emit("high", 8),
            ],
            vec![gap],
        );
        let expanded = expand_families(&BTreeMap::from([("front_end".into(), family)])).unwrap();
        assert_eq!(expanded.len(), 3);
        assert!(!expanded.contains_key("front_end_range_low_gain_8"));
    }

    #[test]
    fn global_named_gap_does_not_satisfy_family_completeness() {
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
status = "emit"
applicability = { observation = [1, 10] }

[[transfer_families.front_end.members]]
selectors = { range = "high", gain = 8 }
status = "emit"
applicability = { observation = [1, 10] }

[gaps.low_gain_8]
status = "undefined"
reason = "named globally; not a family-scoped identity"
"#;
        let error = parse_family(toml)
            .unwrap()
            .validate()
            .unwrap_err()
            .to_string();
        assert!(error.contains("contains 4 identities"), "{error}");
        assert!(error.contains("occupy 3"), "{error}");
    }

    #[test]
    fn expected_selectors_do_not_invent_cartesian_cells() {
        let toml = r#"
[transfer_families.front_end]
provenance = { identity = "test fixture" }
input_unit = "count"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
formula = "x"
expected_selectors = [
  { range = "low", gain = 1 },
  { range = "high", gain = 8 },
]

[[transfer_families.front_end.members]]
selectors = { range = "low", gain = 1 }
status = "emit"
applicability = { observation = [1, 10] }

[[transfer_families.front_end.members]]
selectors = { range = "high", gain = 8 }
status = "emit"
applicability = { observation = [1, 10] }
"#;
        let defs = parse_family(toml).unwrap();
        let validated = defs.validate().unwrap();
        let family = &validated.families()[0];
        match family.selector_universe() {
            SelectorUniverse::Explicit { identities } => assert_eq!(identities.len(), 2),
            other => panic!("expected explicit universe, got {other:?}"),
        }
        assert_eq!(family.selector_universe().identity_count(), Some(2));
        assert_eq!(family.selector_universe().identities().count(), 2);
        assert_eq!(family.completeness(), FamilyCompleteness::Complete);
        assert_eq!(validated.emitted_transfer_names().count(), 2);
    }

    #[test]
    fn large_explicit_universe_uses_indexed_membership() {
        const IDENTITY_COUNT: i64 = 4_096;
        let mut identities = Vec::with_capacity(IDENTITY_COUNT as usize);
        let mut members = Vec::with_capacity(IDENTITY_COUNT as usize);
        for value in 0..IDENTITY_COUNT {
            let selectors = BTreeMap::from([("n".into(), SelectorValue::Integer(value))]);
            identities.push(selectors.clone());
            let member = if value == 0 {
                let mut member = emit_member("div4", 100);
                member.selectors = selectors;
                member
            } else {
                FamilyMemberDef {
                    selectors,
                    input_transform: None,
                    status: MemberStatus::Unsupported,
                    reason: Some("not supported by the shared source".into()),
                    applicability: ApplicabilityDef::default(),
                    provenance: None,
                    emitted_name: None,
                }
            };
            members.push(member);
        }

        let mut family = formula_family(vec![members[0].clone()]);
        family.members = members;
        family.selector_axes = None;
        family.expected_selectors = Some(identities);
        let expanded = expand_families(&BTreeMap::from([("large".into(), family)])).unwrap();
        assert_eq!(expanded.len(), 1);
        assert!(expanded.contains_key("large_n_0"));
    }

    #[test]
    fn integer_and_string_axis_values_remain_distinct_expected_identities() {
        let mut int_member = emit_member("div4", 100);
        int_member.selectors = BTreeMap::from([("n".into(), SelectorValue::Integer(1))]);
        let mut family = formula_family(vec![int_member]);
        family.expected_selectors = None;
        family.selector_axes = Some(BTreeMap::from([(
            "n".into(),
            vec![SelectorValue::Integer(1), SelectorValue::String("1".into())],
        )]));
        let error = expand_families(&BTreeMap::from([("als".into(), family)])).unwrap_err();
        assert!(error.contains("contains 2 identities"), "{error}");
        assert!(error.contains("occupy 1"), "{error}");
    }

    #[test]
    fn selector_value_type_mismatch_is_rejected() {
        let mut member = emit_member("div4", 100);
        member.selectors = BTreeMap::from([("n".into(), SelectorValue::String("1".into()))]);
        let mut family = formula_family(vec![member]);
        family.expected_selectors = None;
        family.selector_axes = Some(BTreeMap::from([(
            "n".into(),
            vec![SelectorValue::Integer(1), SelectorValue::Integer(2)],
        )]));
        let error = expand_families(&BTreeMap::from([("als".into(), family)])).unwrap_err();
        assert!(error.contains("has a string value"), "{error}");
        assert!(error.contains("uses integer"), "{error}");
    }

    #[test]
    fn identity_outside_the_declared_universe_is_rejected() {
        let mut extra = emit_member("div4", 100);
        extra.selectors = BTreeMap::from([("n".into(), SelectorValue::Integer(3))]);
        let mut covered = emit_member("div8", 100);
        covered.selectors = BTreeMap::from([("n".into(), SelectorValue::Integer(1))]);
        let mut family = formula_family(vec![covered, extra]);
        family.expected_selectors = None;
        family.selector_axes = Some(BTreeMap::from([(
            "n".into(),
            vec![SelectorValue::Integer(1), SelectorValue::Integer(2)],
        )]));
        let error = expand_families(&BTreeMap::from([("als".into(), family)])).unwrap_err();
        assert!(error.contains("outside the declared universe"), "{error}");
        assert!(error.contains("n=3"), "{error}");
    }

    #[test]
    fn member_and_family_gap_for_the_same_identity_are_rejected() {
        let gap = FamilyGapDef {
            selectors: two_axis("low", 1),
            status: GapStatus::Undefined,
            reason: "conflicts with the emitted member".into(),
            provenance: None,
        };
        let family = cartesian_family(
            vec![
                two_axis_emit("low", 1),
                two_axis_emit("low", 8),
                two_axis_emit("high", 1),
                two_axis_emit("high", 8),
            ],
            vec![gap],
        );
        let error = expand_families(&BTreeMap::from([("front_end".into(), family)])).unwrap_err();
        assert!(
            error.contains("member 0 and family-scoped gap 0 share selector identity"),
            "{error}"
        );
    }

    #[test]
    fn family_scoped_gap_requires_a_non_blank_reason() {
        let gap = FamilyGapDef {
            selectors: two_axis("low", 8),
            status: GapStatus::Undefined,
            reason: "   ".into(),
            provenance: None,
        };
        let family = cartesian_family(
            vec![
                two_axis_emit("low", 1),
                two_axis_emit("high", 1),
                two_axis_emit("high", 8),
            ],
            vec![gap],
        );
        let error = expand_families(&BTreeMap::from([("front_end".into(), family)])).unwrap_err();
        assert!(error.contains("reason must not be blank"), "{error}");
    }

    #[test]
    fn selector_axes_and_expected_selectors_are_mutually_exclusive() {
        let mut family = formula_family(vec![emit_member("div4", 100)]);
        family.selector_axes = Some(BTreeMap::from([(
            "gain".into(),
            vec![SelectorValue::String("div4".into())],
        )]));
        let error = expand_families(&BTreeMap::from([("als".into(), family)])).unwrap_err();
        assert!(error.contains("mutually exclusive"), "{error}");
    }

    #[test]
    fn missing_selector_universe_is_rejected() {
        let mut family = formula_family(vec![emit_member("div4", 100)]);
        family.selector_axes = None;
        family.expected_selectors = None;
        let error = expand_families(&BTreeMap::from([("als".into(), family)])).unwrap_err();
        assert!(
            error.contains("exactly one of selector_axes or expected_selectors"),
            "{error}"
        );
    }

    #[test]
    fn unknown_family_gap_field_is_rejected() {
        let error = parse_family(
            r#"
[transfer_families.als]
provenance = { identity = "test fixture" }
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

[[transfer_families.als.gaps]]
selectors = { gain = "div8" }
status = "undefined"
reason = "not characterized"
channel = "als"
"#,
        )
        .unwrap_err();
        assert!(error.contains("unknown field `channel`"), "{error}");
    }

    #[test]
    fn description_only_member_rationale_is_required_and_exposed() {
        let toml = r#"
[transfer_families.als]
provenance = { identity = "test fixture" }
input_unit = "count"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
formula = "x"
selector_axes = { gain = ["div4", "x1"] }

[[transfer_families.als.members]]
selectors = { gain = "div4" }
status = "emit"
applicability = { observation = [1, 10] }

[[transfer_families.als.members]]
selectors = { gain = "x1" }
status = "forbidden"
reason = "exceeds the absolute maximum rating"
applicability = { observation = [1, 10] }
"#;
        let family = parse_family(toml).unwrap().validate().unwrap().families()[0].clone();
        let forbidden = &family.members()[1];
        assert_eq!(forbidden.status(), MemberStatus::Forbidden);
        assert_eq!(
            forbidden.reason(),
            Some("exceeds the absolute maximum rating")
        );
        assert_eq!(family.completeness(), FamilyCompleteness::Complete);
    }

    #[test]
    fn source_backed_family_requires_provenance_identity() {
        let defs = parse_family(
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
"#,
        )
        .unwrap();
        let error = defs.validate().unwrap_err().to_string();
        assert!(
            error.contains("source-backed family requires provenance.identity"),
            "{error}"
        );
    }

    #[test]
    fn blank_family_provenance_identity_is_rejected() {
        let defs = parse_family(
            r#"
[transfer_families.als]
provenance = { identity = "  " }
input_unit = "count"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
formula = "x"

[[transfer_families.als.members]]
selectors = { gain = "div4" }
status = "emit"
applicability = { observation = [1, 10] }
"#,
        )
        .unwrap();
        let error = defs.validate().unwrap_err().to_string();
        assert!(
            error.contains("provenance.identity must not be blank"),
            "{error}"
        );
    }

    #[test]
    fn unknown_provenance_field_is_rejected() {
        let error = parse_family(
            r#"
[transfer_families.als]
provenance = { identity = "datasheet", fetched = true }
input_unit = "count"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
formula = "x"

[[transfer_families.als.members]]
selectors = { gain = "div4" }
status = "emit"
applicability = { observation = [1, 10] }
"#,
        )
        .unwrap_err();
        assert!(error.contains("unknown field `fetched`"), "{error}");
    }

    #[test]
    fn member_inherits_family_provenance_unless_overridden() {
        let defs = parse_family(
            r#"
[transfer_families.als]
provenance = { identity = "synthetic ALS application note", revision = "1.0", locator = "Table 1" }
input_unit = "count"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
formula = "x"
selector_axes = { gain = ["div4", "x1"] }

[[transfer_families.als.members]]
selectors = { gain = "div4" }
status = "emit"
applicability = { observation = [1, 10] }

[[transfer_families.als.members]]
selectors = { gain = "x1" }
status = "forbidden"
reason = "datasheet marks this combination invalid"
applicability = { observation = [1, 10] }
provenance = { locator = "§4.2 forbidden matrix" }
"#,
        )
        .unwrap();
        let validated = defs.validate().unwrap();
        let family = &validated.families()[0];
        let expected = SourceProvenance::new("synthetic ALS application note")
            .with_revision("1.0")
            .with_locator("Table 1");
        assert_eq!(family.provenance(), &expected);
        assert_eq!(family.members()[0].provenance(), &expected);
        assert!(family.members()[0].provenance_override().is_none());
        assert_eq!(
            family.members()[1].provenance(),
            &SourceProvenance::new("synthetic ALS application note")
                .with_revision("1.0")
                .with_locator("§4.2 forbidden matrix")
        );
        assert_eq!(
            family.members()[1]
                .provenance_override()
                .and_then(|overlay| overlay.locator.as_deref()),
            Some("§4.2 forbidden matrix")
        );
        let expanded = defs.resolved_transfers().unwrap();
        assert_eq!(
            expanded["als_gain_div4"].provenance.as_ref(),
            Some(&expected)
        );
    }

    #[test]
    fn expanded_member_carries_resolved_citation_not_representation() {
        let mut families = BTreeMap::new();
        families.insert("als".into(), formula_family(vec![emit_member("div4", 100)]));
        let expanded = expand_families(&families).unwrap();
        let def = &expanded["als_gain_div4_integration_time_ms_100"];
        assert_eq!(
            def.provenance
                .as_ref()
                .map(|citation| citation.identity.as_str()),
            Some("test fixture")
        );
    }

    #[test]
    fn explicit_emitted_name_is_the_expansion_key() {
        let mut member = emit_member("div4", 100);
        member.emitted_name = Some("als_x4".into());
        let expanded = expand_families(&BTreeMap::from([(
            "als".into(),
            formula_family(vec![member]),
        )]))
        .unwrap();
        assert!(expanded.contains_key("als_x4"));
        assert!(!expanded.contains_key("als_gain_div4_integration_time_ms_100"));
    }

    #[test]
    fn explicit_and_derived_stems_collide() {
        let mut first = emit_member("div4", 100);
        first.emitted_name = Some("shared_stem".into());
        let mut second = emit_member("div8", 100);
        second.emitted_name = Some("shared_stem".into());
        let error = expand_families(&BTreeMap::from([(
            "als".into(),
            formula_family(vec![first, second]),
        )]))
        .unwrap_err();
        assert!(error.contains("both resolve to `shared_stem`"), "{error}");
    }

    #[test]
    fn cross_family_explicit_and_derived_stems_report_both_origins() {
        let derived = emit_member("div4", 100);
        let mut explicit = emit_member("x1", 50);
        explicit.emitted_name = Some("als_gain_div4_integration_time_ms_100".into());

        let error = expand_families(&BTreeMap::from([
            ("als".into(), formula_family(vec![derived])),
            ("legacy".into(), formula_family(vec![explicit])),
        ]))
        .unwrap_err();

        assert_eq!(
            error,
            "resolved emitted name `als_gain_div4_integration_time_ms_100` collides between \
             transfer family `als` selectors {gain=\"div4\", integration_time_ms=100} and \
             transfer family `legacy` selectors {gain=\"x1\", integration_time_ms=50}"
        );
    }

    #[test]
    fn cross_family_derived_stems_report_both_origins() {
        let mut first = emit_member("unused", 100);
        first.selectors = BTreeMap::from([("b".into(), SelectorValue::String("c_d".into()))]);
        let mut second = emit_member("unused", 100);
        second.selectors = BTreeMap::from([("c".into(), SelectorValue::String("d".into()))]);

        let error = expand_families(&BTreeMap::from([
            ("a".into(), formula_family(vec![first])),
            ("a_b".into(), formula_family(vec![second])),
        ]))
        .unwrap_err();

        assert_eq!(
            error,
            "resolved emitted name `a_b_c_d` collides between transfer family `a` selectors \
             {b=\"c_d\"} and transfer family `a_b` selectors {c=\"d\"}"
        );
    }

    #[test]
    fn toml_emitted_name_is_independent_of_selector_spelling() {
        let toml = r#"
[transfer_families.als]
provenance = { identity = "test fixture" }
input_unit = "count"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
formula = "x"
selector_axes = { gain = ["x4"] }

[[transfer_families.als.members]]
selectors = { gain = "x4" }
status = "emit"
emitted_name = "als_gain_div4"
applicability = { observation = [1, 10] }
"#;
        let defs = parse_family(toml).unwrap();
        let validated = defs.validate().unwrap();
        let member = &validated.families()[0].members()[0];
        assert_eq!(member.expanded_name(), "als_gain_x4");
        assert_eq!(member.emitted_name(), "als_gain_div4");
    }
}
