//! Curve definition, building, and validation.
//!
//! This module owns the TOML schema types and orchestrates curve
//! construction by dispatching to [`super::builtin`], [`super::formula`],
//! and [`super::points`].

// Host-only: module-local std link (crate root stays `#![no_std]`).
extern crate std;

use std::prelude::v1::*;
use std::{format, vec};

use std::collections::BTreeMap;

use serde::Deserialize;
use serde::de::{self, Deserializer};

use super::{builtin, formula, points, transfer};

// ---------------------------------------------------------------------------
// TOML schema
//
// A curve is defined by exactly ONE of: `builtin`, `formula`, or `points`.
// ---------------------------------------------------------------------------

/// Parsed TOML definitions for normalized curves and physical transfers.
///
/// Construct via [`Self::from_toml_str`], [`Self::insert_transfer`], or
/// [`super::generate_from_str`]. Inspect maps through [`Self::curves`],
/// [`Self::transfers`], [`Self::transfer_families`], and [`Self::gaps`].
/// Validate the description graph with [`Self::validate`] before overlaying
/// host-evaluated truth.
///
/// Unknown top-level keys are rejected so a misspelled table cannot succeed
/// as empty output. Unknown fields directly on standalone transfer definitions
/// and nested fields on families, their point and NTC sources, members,
/// applicability, and gaps are also rejected. Nested unknown fields on
/// standalone curves, point values, and legacy NTC model parameters are still
/// ignored for compatibility; reserved
/// observation-guard and provenance spellings cannot be nested inside source
/// values.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DefinitionsFile {
    /// Normalized LUT curves keyed by TOML table name.
    #[serde(default)]
    pub(crate) curves: BTreeMap<String, CurveDef>,
    /// Sparse physical transfer functions keyed by TOML table name.
    #[serde(default, deserialize_with = "deserialize_transfers")]
    pub(crate) transfers: BTreeMap<String, transfer::TransferDef>,
    /// Shared-source families expanded into sparse transfers at generation.
    #[serde(default, deserialize_with = "deserialize_transfer_families")]
    pub(crate) transfer_families: BTreeMap<String, transfer::TransferFamilyDef>,
    /// Channels or procedures that must not be generated as transfers.
    #[serde(default)]
    pub(crate) gaps: BTreeMap<String, transfer::GapDef>,
    /// Programmatic generation sources keyed by transfer name.
    #[serde(skip)]
    pub(crate) overlays: BTreeMap<String, transfer::TransferSourceOverlay>,
}

const OBSERVATION_GUARD_CAPABILITY: &str = "observation_guard_v1";
const SOURCE_PROVENANCE_CAPABILITY: &str = "source_provenance_v1";

fn reject_misplaced_reserved_transfer_fields(
    definition_label: &str,
    direct_table: &str,
    value: &toml::Value,
) -> Result<(), String> {
    let toml::Value::Table(fields) = value else {
        return Ok(());
    };
    // `model` and point entries are the intentionally permissive legacy source
    // values. Restrict the scan to them: family selector maps are open user
    // keyspaces where reserved words can be legitimate selector names.
    for source_field in ["model", "points"] {
        if let Some(nested) = fields.get(source_field) {
            reject_nested_reserved_transfer_fields(
                definition_label,
                direct_table,
                nested,
                source_field,
            )?;
        }
    }
    Ok(())
}

fn reject_nested_reserved_transfer_fields(
    definition_label: &str,
    direct_table: &str,
    value: &toml::Value,
    path: &str,
) -> Result<(), String> {
    match value {
        toml::Value::Table(fields) => {
            for (field, nested) in fields {
                let nested_path = format!("{path}.{field}");
                let direct_field = match field.as_str() {
                    "saturation" | "observation_guard" => Some("saturation"),
                    "provenance" => Some("provenance"),
                    _ => None,
                };
                if let Some(direct_field) = direct_field {
                    return Err(format!(
                        "{definition_label}: misplaced `{field}` at `{nested_path}`; \
                         declare `{direct_field}` directly under `{direct_table}`"
                    ));
                }
                reject_nested_reserved_transfer_fields(
                    definition_label,
                    direct_table,
                    nested,
                    &nested_path,
                )?;
            }
        }
        toml::Value::Array(values) => {
            for (index, nested) in values.iter().enumerate() {
                reject_nested_reserved_transfer_fields(
                    definition_label,
                    direct_table,
                    nested,
                    &format!("{path}[{index}]"),
                )?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn reject_unknown_family_source_fields(
    family_name: &str,
    value: &toml::Value,
) -> Result<(), String> {
    let toml::Value::Table(fields) = value else {
        return Ok(());
    };

    if let Some(toml::Value::Array(points)) = fields.get("points") {
        for (index, point) in points.iter().enumerate() {
            let toml::Value::Table(point_fields) = point else {
                // Leave type diagnostics to `PhysicalPoint`'s deserializer.
                continue;
            };
            for field in point_fields.keys() {
                if field != "input" && field != "output" {
                    return Err(format!(
                        "transfer family `{family_name}`: unknown field `{field}` at \
                         `points[{index}].{field}`; family point fields are `input` and `output`"
                    ));
                }
            }
        }
    }

    if let Some(toml::Value::Table(model)) = fields.get("model") {
        let is_ntc_beta_divider = matches!(
            model.get("kind"),
            Some(toml::Value::String(kind)) if kind == "ntc_beta_divider"
        );
        if is_ntc_beta_divider {
            const NTC_BETA_DIVIDER_FIELDS: [&str; 7] = [
                "kind",
                "nominal_resistance_ohms",
                "beta_kelvin",
                "nominal_temperature_celsius",
                "fixed_resistance_ohms",
                "adc_max_code",
                "topology",
            ];
            for field in model.keys() {
                if !NTC_BETA_DIVIDER_FIELDS.contains(&field.as_str()) {
                    return Err(format!(
                        "transfer family `{family_name}`: unknown field `{field}` at \
                         `model.{field}` for `ntc_beta_divider`"
                    ));
                }
            }
        }
    }

    Ok(())
}

fn deserialize_transfer_families<'de, D>(
    deserializer: D,
) -> Result<BTreeMap<String, transfer::TransferFamilyDef>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = BTreeMap::<String, toml::Value>::deserialize(deserializer)?;
    let mut families = BTreeMap::new();
    for (name, value) in raw {
        reject_misplaced_reserved_transfer_fields(
            &format!("transfer family `{name}`"),
            &format!("[transfer_families.{name}]"),
            &value,
        )
        .map_err(de::Error::custom)?;
        reject_unknown_family_source_fields(&name, &value).map_err(de::Error::custom)?;
        let definition: transfer::TransferFamilyDef = value
            .try_into()
            .map_err(|error| de::Error::custom(format!("transfer family `{name}`: {error}")))?;
        families.insert(name, definition);
    }
    Ok(families)
}

/// Deserialize the `[transfers]` section while retaining a fail-closed
/// capability markers for observation guards and source provenance.
///
/// The marker is deliberately an array inside `[transfers]`. Generators that
/// predate this schema deserialize every value in that table as a transfer and
/// therefore reject the array instead of silently ignoring `saturation` or
/// `provenance` on a nested transfer definition.
fn deserialize_transfers<'de, D>(
    deserializer: D,
) -> Result<BTreeMap<String, transfer::TransferDef>, D::Error>
where
    D: Deserializer<'de>,
{
    let mut raw = BTreeMap::<String, toml::Value>::deserialize(deserializer)?;
    let capabilities = match raw.remove("requires") {
        Some(toml::Value::Array(values)) => {
            let mut capabilities = Vec::with_capacity(values.len());
            for value in values {
                match value {
                    toml::Value::String(capability) => capabilities.push(capability),
                    other => {
                        return Err(de::Error::custom(format!(
                            "[transfers] requires entries must be strings, got {other}"
                        )));
                    }
                }
            }
            capabilities
        }
        Some(table @ toml::Value::Table(_)) => {
            // Preserve the previously valid `[transfers.requires]`
            // transfer name. Only the array form is the section marker.
            raw.insert("requires".into(), table);
            Vec::new()
        }
        Some(other) => {
            return Err(de::Error::custom(format!(
                "[transfers] requires must be an array of capability strings, got {other}"
            )));
        }
        None => Vec::new(),
    };

    for capability in &capabilities {
        if capability != OBSERVATION_GUARD_CAPABILITY && capability != SOURCE_PROVENANCE_CAPABILITY
        {
            return Err(de::Error::custom(format!(
                "unsupported [transfers] capability `{capability}`"
            )));
        }
    }

    let mut transfers = BTreeMap::new();
    for (name, value) in raw {
        reject_misplaced_reserved_transfer_fields(
            &format!("standalone transfer `{name}`"),
            &format!("[transfers.{name}]"),
            &value,
        )
        .map_err(de::Error::custom)?;
        let definition: transfer::TransferDef = value
            .try_into()
            .map_err(|error| de::Error::custom(format!("standalone transfer `{name}`: {error}")))?;
        transfers.insert(name, definition);
    }

    let has_guard = transfers
        .values()
        .any(|definition| definition.observation_guard().is_some());
    let has_guard_capability = capabilities
        .iter()
        .any(|capability| capability == OBSERVATION_GUARD_CAPABILITY);
    let has_provenance = transfers.values().any(|definition| {
        definition.provenance().is_some()
            || definition
                .observation_guard()
                .is_some_and(|guard| guard.provenance.is_some())
    });
    let has_provenance_capability = capabilities
        .iter()
        .any(|capability| capability == SOURCE_PROVENANCE_CAPABILITY);
    if has_guard && !has_guard_capability {
        if transfers.contains_key("requires") {
            return Err(de::Error::custom(
                "a standalone transfer named `requires` cannot coexist with observation guards; \
                 rename that transfer before declaring the `[transfers] requires` capability marker",
            ));
        }
        return Err(de::Error::custom(
            "standalone transfers using `saturation` require \
             `[transfers] requires = [\"observation_guard_v1\"]`; \
             the marker makes older generators reject the document instead of silently dropping the guard",
        ));
    }
    if has_guard_capability && !has_guard {
        return Err(de::Error::custom(
            "[transfers] requires `observation_guard_v1`, but no standalone transfer declared a \
             direct `saturation` guard; check the guard spelling and TOML table placement",
        ));
    }
    if has_provenance && !has_provenance_capability {
        if transfers.contains_key("requires") {
            return Err(de::Error::custom(
                "a standalone transfer named `requires` cannot coexist with source provenance; \
                 rename that transfer before declaring the `[transfers] requires` capability marker",
            ));
        }
        return Err(de::Error::custom(
            "standalone transfers using `provenance` require \
             `[transfers] requires = [\"source_provenance_v1\"]`; \
             the marker makes older generators reject the document instead of silently dropping the citation",
        ));
    }
    if has_provenance_capability && !has_provenance {
        return Err(de::Error::custom(
            "[transfers] requires `source_provenance_v1`, but no standalone transfer declared \
             source provenance; check the provenance spelling and TOML table placement",
        ));
    }

    Ok(transfers)
}

impl DefinitionsFile {
    /// Parse a TOML definitions document.
    pub fn from_toml_str(toml: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(toml)
    }

    /// Normalized LUT curves keyed by TOML table name.
    pub fn curves(&self) -> &BTreeMap<String, CurveDef> {
        &self.curves
    }

    /// Standalone `[transfers]` entries. Family members are not included;
    /// inspect those through [`Self::validate`].
    pub fn transfers(&self) -> &BTreeMap<String, transfer::TransferDef> {
        &self.transfers
    }

    /// Declared transfer families. Empty on documents that only use `[transfers]`.
    pub fn transfer_families(&self) -> &BTreeMap<String, transfer::TransferFamilyDef> {
        &self.transfer_families
    }

    /// Declared gaps. A missing gap is not the same as an undefined one.
    pub fn gaps(&self) -> &BTreeMap<String, transfer::GapDef> {
        &self.gaps
    }

    /// Standalone transfers plus expanded `status = "emit"` family members.
    pub(crate) fn resolved_transfers(
        &self,
    ) -> Result<BTreeMap<String, transfer::TransferDef>, String> {
        for (name, gap) in &self.gaps {
            if gap.reason.trim().is_empty() {
                return Err(format!("gap `{name}`: reason must not be blank"));
            }
            if let Err(error) = gap.resolved_provenance() {
                return Err(format!("gap `{name}`: {error}"));
            }
            if self.curves.contains_key(name) {
                return Err(format!("gap `{name}` collides with a [curves] entry"));
            }
            if self.transfers.contains_key(name) {
                return Err(format!("gap `{name}` collides with a [transfers] entry"));
            }
            if self.transfer_families.contains_key(name) {
                return Err(format!(
                    "gap `{name}` collides with a [transfer_families] entry"
                ));
            }
        }

        let expanded = transfer::family::expand_families(&self.transfer_families)?;
        for name in expanded.keys() {
            if self.gaps.contains_key(name) {
                return Err(format!(
                    "gap `{name}` collides with an emitted family member"
                ));
            }
            if self.transfers.contains_key(name) {
                return Err(format!(
                    "expanded family member `{name}` collides with a standalone [transfers] entry"
                ));
            }
        }

        let mut resolved = BTreeMap::new();
        for (name, def) in &self.transfers {
            let mut def = def.clone();
            def.resolved_guard_provenance = transfer::resolve_guard_provenance(
                &format!("standalone transfer `{name}`"),
                def.observation_guard(),
                def.provenance(),
            )?;
            resolved.insert(name.clone(), def);
        }
        for (name, def) in expanded {
            resolved.insert(name, def);
        }
        Ok(resolved)
    }
}

/// One normalized curve definition from the TOML `[curves]` map.
#[derive(Debug, Clone, Deserialize)]
pub struct CurveDef {
    /// Name of a built-in curve (e.g. "linear", "ease_in_quad").
    pub builtin: Option<String>,
    /// A math expression in terms of `t` (0..1) that evaluates to 0..1.
    pub formula: Option<String>,
    /// Piecewise-linear control points as `[u, w]` pairs.
    pub points: Option<Vec<[u16; 2]>>,
    /// Whether the curve is monotonic non-decreasing (default: true).
    #[serde(default = "default_true")]
    pub monotonic: bool,
}

fn default_true() -> bool {
    true
}

// ---------------------------------------------------------------------------
// Curve data
// ---------------------------------------------------------------------------

pub struct CurveData {
    pub fwd: Vec<u32>,
    pub inv: Option<Vec<u32>>,
}

/// Build a complete [`CurveData`] (forward LUT + optional inverse) from a
/// [`CurveDef`].
///
pub fn build(name: &str, def: &CurveDef, lut_size: usize) -> Result<CurveData, String> {
    let set_count =
        def.builtin.is_some() as u8 + def.formula.is_some() as u8 + def.points.is_some() as u8;
    if set_count != 1 {
        return Err(format!(
            "curve `{name}`: exactly one of `builtin`, `formula`, or `points` \
             must be specified (found {set_count})"
        ));
    }

    let fwd = if let Some(b) = &def.builtin {
        build_from_easing(name, lut_size, |t| builtin::eval(b, t))?
    } else if let Some(f) = &def.formula {
        let parsed =
            formula::Formula::parse(f).map_err(|error| format!("curve `{name}`: {error}"))?;
        build_from_easing(name, lut_size, |t| parsed.eval("t", t))?
    } else {
        points::build(
            name,
            def.points.as_deref().expect("source count checked"),
            lut_size,
        )?
    };

    validate(name, &fwd, def.monotonic)?;

    let inv = if def.monotonic {
        Some(invert(&fwd))
    } else {
        None
    };

    Ok(CurveData { fwd, inv })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Sample an easing function (builtin or formula) into a forward LUT.
fn build_from_easing(
    label: &str,
    n: usize,
    f: impl Fn(f64) -> Result<f64, String>,
) -> Result<Vec<u32>, String> {
    // Validate early so we get a clear error on bad names / expressions.
    let _ = f(0.5).map_err(|error| format!("curve `{label}`: {error}"))?;

    let max = (n - 1) as f64;
    let mut fwd: Vec<u32> = (0..n)
        .map(|u| {
            let t = u as f64 / max;
            let w = f(t).map_err(|error| format!("curve `{label}`: {error}"))?;
            if !w.is_finite() {
                return Err(format!(
                    "curve `{label}`: produced non-finite value at t={t}"
                ));
            }
            Ok((w * max).round().clamp(0.0, max) as u32)
        })
        .collect::<Result<_, String>>()?;
    // Pin endpoints.
    fwd[0] = 0;
    *fwd.last_mut().expect("validated LUT size is nonzero") = max as u32;
    Ok(fwd)
}

fn validate(name: &str, fwd: &[u32], monotonic: bool) -> Result<(), String> {
    let max = (fwd.len() - 1) as u32;
    if fwd[0] != 0 || *fwd.last().expect("validated LUT size is nonzero") != max {
        return Err(format!(
            "curve `{name}` must map 0\u{2192}0 and {max}\u{2192}{max}"
        ));
    }
    if monotonic {
        for i in 1..fwd.len() {
            if fwd[i] < fwd[i - 1] {
                return Err(format!(
                    "curve `{name}` must be monotonic non-decreasing \
                     (fwd[{i}]={} < fwd[{}]={})",
                    fwd[i],
                    i - 1,
                    fwd[i - 1],
                ));
            }
        }
    }
    Ok(())
}

fn invert(fwd: &[u32]) -> Vec<u32> {
    let n = fwd.len();
    let max = (n - 1) as u32;
    let mut inv = vec![0u32; n];
    let mut u = 0usize;
    for (w, slot) in inv.iter_mut().enumerate() {
        while u < n && fwd[u] < w as u32 {
            u += 1;
        }
        *slot = if u >= n { max } else { u as u32 };
    }
    inv
}

#[cfg(test)]
mod tests {
    use super::*;

    fn linear_def() -> CurveDef {
        CurveDef {
            builtin: Some("linear".into()),
            formula: None,
            points: None,
            monotonic: true,
        }
    }

    fn formula_def(expr: &str) -> CurveDef {
        CurveDef {
            builtin: None,
            formula: Some(expr.into()),
            points: None,
            monotonic: true,
        }
    }

    fn points_def(pts: Vec<[u16; 2]>, monotonic: bool) -> CurveDef {
        CurveDef {
            builtin: None,
            formula: None,
            points: Some(pts),
            monotonic,
        }
    }

    // ── build dispatching ──────────────────────────────────────────

    #[test]
    fn build_builtin_linear() {
        let def = linear_def();
        let data = build("linear", &def, 256).unwrap();
        assert_eq!(data.fwd.len(), 256);
        assert_eq!(data.fwd[0], 0);
        assert_eq!(data.fwd[255], 255);
        assert!(data.inv.is_some());
    }

    #[test]
    fn build_formula_identity() {
        let def = formula_def("t");
        let data = build("ident", &def, 256).unwrap();
        for i in 0..256 {
            assert_eq!(data.fwd[i], i as u32);
        }
    }

    #[test]
    fn build_points_linear() {
        let def = points_def(vec![[0, 0], [255, 255]], true);
        let data = build("pts", &def, 256).unwrap();
        for i in 0..256 {
            assert_eq!(data.fwd[i], i as u32);
        }
    }

    #[test]
    fn build_non_monotonic_has_no_inv() {
        let def = points_def(vec![[0, 0], [64, 200], [192, 50], [255, 255]], false);
        let data = build("wave", &def, 256).unwrap();
        assert!(data.inv.is_none());
    }

    #[test]
    fn build_no_definition_returns_error() {
        let def = CurveDef {
            builtin: None,
            formula: None,
            points: None,
            monotonic: true,
        };
        assert!(build("empty", &def, 256).is_err());
    }

    #[test]
    fn build_multiple_definitions_returns_error() {
        let def = CurveDef {
            builtin: Some("linear".into()),
            formula: Some("t".into()),
            points: None,
            monotonic: true,
        };
        assert!(build("double", &def, 256).is_err());
    }

    // ── validate ───────────────────────────────────────────────────

    #[test]
    fn validate_rejects_non_monotonic_when_required() {
        // Hand-craft a non-monotonic fwd array.
        let mut fwd: Vec<u32> = (0..10).collect();
        fwd[5] = 3; // break monotonicity
        assert!(validate("bad", &fwd, true).is_err());
    }

    // ── invert ─────────────────────────────────────────────────────

    #[test]
    fn invert_identity() {
        let fwd: Vec<u32> = (0..256).collect();
        let inv = invert(&fwd);
        for (i, &v) in inv.iter().enumerate() {
            assert_eq!(v, i as u32);
        }
    }

    #[test]
    fn invert_round_trip() {
        // Build an ease_in_quad and verify inv[fwd[u]] ≥ u for all u.
        let def = CurveDef {
            builtin: Some("ease_in_quad".into()),
            formula: None,
            points: None,
            monotonic: true,
        };
        let data = build("eiq", &def, 256).unwrap();
        let inv = data.inv.unwrap();
        for u in 0..256 {
            let w = data.fwd[u] as usize;
            assert!(
                inv[w] as usize >= u || data.fwd[inv[w] as usize] >= w as u32,
                "round-trip failed at u={u}, w={w}"
            );
        }
    }

    // ── build_from_easing ──────────────────────────────────────────

    #[test]
    fn build_from_easing_pins_endpoints() {
        let fwd = build_from_easing("test", 10, |t| Ok(t * t)).unwrap();
        assert_eq!(fwd[0], 0);
        assert_eq!(fwd[9], 9);
    }

    #[test]
    fn build_from_easing_values_in_range() {
        let fwd = build_from_easing("test", 256, |t| Ok(t * t * t)).unwrap();
        for (i, &v) in fwd.iter().enumerate() {
            assert!(v <= 255, "fwd[{i}] = {v} out of range");
        }
    }

    // ── TOML deserialization ───────────────────────────────────────

    #[test]
    fn deserialize_curves_file() {
        let toml = r#"
[curves.test]
builtin = "linear"
"#;
        let cf: DefinitionsFile = toml::from_str(toml).unwrap();
        assert!(cf.curves.contains_key("test"));
        assert!(cf.curves["test"].monotonic); // default true
    }

    #[test]
    fn deserialize_non_monotonic() {
        let toml = r#"
[curves.wave]
points = [[0, 0], [128, 255], [255, 0]]
monotonic = false
"#;
        let cf: DefinitionsFile = toml::from_str(toml).unwrap();
        assert!(!cf.curves["wave"].monotonic);
    }

    #[test]
    fn from_toml_str_rejects_unknown_top_level_table() {
        let error = DefinitionsFile::from_toml_str("[invented]\n")
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("unknown field `invented`"),
            "expected the unrecognized field to be named, got: {error}"
        );
    }

    #[test]
    fn from_toml_str_accepts_empty_families_and_gaps() {
        let defs = DefinitionsFile::from_toml_str("[transfer_families]\n[gaps]\n").unwrap();
        assert!(defs.transfer_families().is_empty());
        assert!(defs.gaps().is_empty());
    }

    fn guarded_standalone_toml(include_capability: bool) -> String {
        let capability = if include_capability {
            "[transfers]\nrequires = [\"observation_guard_v1\"]\n\n"
        } else {
            ""
        };
        format!(
            "{capability}[transfers.guarded]\n\
             input_unit = \"count\"\n\
             output_unit = \"unit\"\n\
             output_scale = 1\n\
             max_interpolation_error = 1\n\
             above = \"clamp\"\n\
             saturation = {{ code = 65535, behavior = \"error\" }}\n\
             formula = \"x\"\n\
             domain = [1, 10]\n"
        )
    }

    fn provenance_standalone_toml(include_capability: bool) -> String {
        let capability = if include_capability {
            "[transfers]\nrequires = [\"source_provenance_v1\"]\n\n"
        } else {
            ""
        };
        format!(
            "{capability}[transfers.cited]\n\
             input_unit = \"count\"\n\
             output_unit = \"unit\"\n\
             output_scale = 1\n\
             max_interpolation_error = 1\n\
             provenance = {{ identity = \"fixture source\", locator = \"Table 1\" }}\n\
             formula = \"x\"\n\
             domain = [1, 10]\n"
        )
    }

    fn cited_guard_standalone_toml(capabilities: &str) -> String {
        guarded_standalone_toml(true)
            .replace(
                "requires = [\"observation_guard_v1\"]",
                &format!("requires = [{capabilities}]"),
            )
            .replace(
                "saturation = { code = 65535, behavior = \"error\" }",
                "saturation = { code = 65535, behavior = \"error\", provenance = { identity = \"device note\" } }",
            )
    }

    #[test]
    fn standalone_transfer_unknown_fields_fail_closed() {
        for unknown in ["saturaton", "observation_guard"] {
            let toml = guarded_standalone_toml(true).replace("saturation", unknown);
            let error = DefinitionsFile::from_toml_str(&toml)
                .unwrap_err()
                .to_string();
            assert!(
                error.contains(&format!("unknown field `{unknown}`")),
                "expected the unknown field to be named, got: {error}"
            );
        }
    }

    #[test]
    fn standalone_guard_requires_fail_closed_capability_marker() {
        let error = DefinitionsFile::from_toml_str(&guarded_standalone_toml(false))
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("requires = [\"observation_guard_v1\"]"),
            "expected the required marker to be named, got: {error}"
        );
    }

    #[test]
    fn standalone_guard_accepts_supported_capability_marker() {
        let defs = DefinitionsFile::from_toml_str(&guarded_standalone_toml(true)).unwrap();
        assert!(defs.transfers()["guarded"].observation_guard().is_some());
    }

    #[test]
    fn unused_observation_guard_capability_fails_closed() {
        let toml = guarded_standalone_toml(true)
            .replace("saturation = { code = 65535, behavior = \"error\" }\n", "");
        let error = DefinitionsFile::from_toml_str(&toml)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("no standalone transfer declared a direct `saturation` guard"),
            "expected the unused capability to fail, got: {error}"
        );
    }

    #[test]
    fn standalone_provenance_requires_fail_closed_capability_marker() {
        let error = DefinitionsFile::from_toml_str(&provenance_standalone_toml(false))
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("requires = [\"source_provenance_v1\"]"),
            "expected the required marker to be named, got: {error}"
        );
    }

    #[test]
    fn standalone_provenance_accepts_supported_capability_marker() {
        let defs = DefinitionsFile::from_toml_str(&provenance_standalone_toml(true)).unwrap();
        assert_eq!(
            defs.transfers()["cited"]
                .provenance()
                .map(|provenance| provenance.identity.as_str()),
            Some("fixture source")
        );
    }

    #[test]
    fn unused_source_provenance_capability_fails_closed() {
        let toml = provenance_standalone_toml(true).replace(
            "provenance = { identity = \"fixture source\", locator = \"Table 1\" }\n",
            "",
        );
        let error = DefinitionsFile::from_toml_str(&toml)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("no standalone transfer declared source provenance"),
            "expected the unused capability to fail, got: {error}"
        );
    }

    #[test]
    fn standalone_guard_with_citation_requires_both_capabilities() {
        let only_guard = cited_guard_standalone_toml("\"observation_guard_v1\"");
        let error = DefinitionsFile::from_toml_str(&only_guard)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("requires = [\"source_provenance_v1\"]"),
            "expected the source capability to be required, got: {error}"
        );

        let only_provenance = cited_guard_standalone_toml("\"source_provenance_v1\"");
        let error = DefinitionsFile::from_toml_str(&only_provenance)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("requires = [\"observation_guard_v1\"]"),
            "expected the guard capability to be required, got: {error}"
        );

        let both =
            cited_guard_standalone_toml("\"observation_guard_v1\", \"source_provenance_v1\"");
        let defs = DefinitionsFile::from_toml_str(&both).unwrap();
        assert!(
            defs.transfers()["guarded"]
                .observation_guard()
                .and_then(|guard| guard.provenance.as_ref())
                .is_some()
        );
    }

    #[test]
    fn observation_guard_nested_in_a_point_fails_closed() {
        let toml = r#"
[transfers]
requires = ["observation_guard_v1"]

[transfers.guarded]
input_unit = "count"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
points = [
  { input = 1, output = 1.0, saturation = { code = 65535, behavior = "error" } },
  { input = 10, output = 10.0 },
]
"#;
        let error = DefinitionsFile::from_toml_str(toml)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("misplaced `saturation` at `points[0].saturation`")
                && error.contains("directly under `[transfers.guarded]`"),
            "expected the misplaced guard to fail, got: {error}"
        );
    }

    #[test]
    fn observation_guard_nested_in_legacy_ntc_model_fails_closed() {
        let toml = r#"
[transfers]
requires = ["observation_guard_v1"]

[transfers.ntc]
input_unit = "adc_code"
output_unit = "degree_celsius"
output_scale = 1000
max_interpolation_error = 50
output_range = [-40.0, 125.0]

[transfers.ntc.model]
kind = "ntc_beta_divider"
nominal_resistance_ohms = 10000.0
beta_kelvin = 3950.0
nominal_temperature_celsius = 25.0
fixed_resistance_ohms = 10000.0
adc_max_code = 4095
topology = "ntc_to_ground"
saturation = { code = 65535, behavior = "error" }
"#;
        let error = DefinitionsFile::from_toml_str(toml)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("misplaced `saturation` at `model.saturation`")
                && error.contains("directly under `[transfers.ntc]`"),
            "expected the misplaced guard to fail, got: {error}"
        );
    }

    #[test]
    fn provenance_nested_in_a_standalone_point_fails_closed_when_capability_is_used() {
        let toml = r#"
[transfers]
requires = ["source_provenance_v1"]

[transfers.cited]
input_unit = "count"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
provenance = { identity = "valid citation" }
formula = "x"
domain = [1, 10]

[transfers.misplaced]
input_unit = "count"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
points = [
  { input = 1, output = 1.0, provenance = { identity = "silently ignored" } },
  { input = 10, output = 10.0 },
]
"#;
        let error = DefinitionsFile::from_toml_str(toml)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("standalone transfer `misplaced`")
                && error.contains("misplaced `provenance` at `points[0].provenance`")
                && error.contains("directly under `[transfers.misplaced]`"),
            "expected the misplaced citation to fail, got: {error}"
        );
    }

    #[test]
    fn provenance_nested_in_a_standalone_model_fails_closed_when_capability_is_used() {
        let toml = r#"
[transfers]
requires = ["source_provenance_v1"]

[transfers.cited]
input_unit = "count"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
provenance = { identity = "valid citation" }
formula = "x"
domain = [1, 10]

[transfers.misplaced]
input_unit = "adc_code"
output_unit = "degree_celsius"
output_scale = 1000
max_interpolation_error = 50
output_range = [-40.0, 125.0]

[transfers.misplaced.model]
kind = "ntc_beta_divider"
nominal_resistance_ohms = 10000.0
beta_kelvin = 3950.0
nominal_temperature_celsius = 25.0
fixed_resistance_ohms = 10000.0
adc_max_code = 4095
topology = "ntc_to_ground"
provenance = { identity = "silently ignored" }
"#;
        let error = DefinitionsFile::from_toml_str(toml)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("standalone transfer `misplaced`")
                && error.contains("misplaced `provenance` at `model.provenance`")
                && error.contains("directly under `[transfers.misplaced]`"),
            "expected the misplaced citation to fail, got: {error}"
        );
    }

    #[test]
    fn misplaced_standalone_provenance_fails_before_capability_accounting() {
        let point_toml = r#"
[transfers.misplaced]
input_unit = "count"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
points = [
  { input = 1, output = 1.0, provenance = { identity = "silently ignored" } },
  { input = 10, output = 10.0 },
]
"#;
        let model_toml = r#"
[transfers.misplaced]
input_unit = "adc_code"
output_unit = "degree_celsius"
output_scale = 1000
max_interpolation_error = 50
output_range = [-40.0, 125.0]
model = { kind = "ntc_beta_divider", nominal_resistance_ohms = 10000.0, beta_kelvin = 3950.0, nominal_temperature_celsius = 25.0, fixed_resistance_ohms = 10000.0, adc_max_code = 4095, topology = "ntc_to_ground", provenance = { identity = "silently ignored" } }
"#;

        for (toml, path) in [
            (point_toml, "points[0].provenance"),
            (model_toml, "model.provenance"),
        ] {
            let error = DefinitionsFile::from_toml_str(toml)
                .unwrap_err()
                .to_string();
            assert!(
                error.contains(&format!("misplaced `provenance` at `{path}`"))
                    && error.contains("directly under `[transfers.misplaced]`"),
                "expected placement to fail before capability accounting, got: {error}"
            );
        }
    }

    #[test]
    fn family_guard_nested_in_a_point_fails_closed() {
        let toml = r#"
[transfer_families.guarded]
provenance = { identity = "test fixture" }
input_unit = "count"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
points = [
  { input = 1, output = 1.0, saturation = { code = 65535, behavior = "error" } },
  { input = 10, output = 10.0 },
]

[[transfer_families.guarded.members]]
selectors = { variant = "one" }
status = "emit"
applicability = { observation = [1, 10] }
"#;
        let error = DefinitionsFile::from_toml_str(toml)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("transfer family `guarded`")
                && error.contains("misplaced `saturation` at `points[0].saturation`")
                && error.contains("directly under `[transfer_families.guarded]`"),
            "expected the misplaced family guard to fail, got: {error}"
        );
    }

    #[test]
    fn family_guard_nested_in_legacy_ntc_model_fails_closed() {
        let toml = r#"
[transfer_families.ntc]
provenance = { identity = "test fixture" }
input_unit = "adc_code"
output_unit = "degree_celsius"
output_scale = 1000
max_interpolation_error = 50
output_range = [-40.0, 125.0]

[transfer_families.ntc.model]
kind = "ntc_beta_divider"
nominal_resistance_ohms = 10000.0
beta_kelvin = 3950.0
nominal_temperature_celsius = 25.0
fixed_resistance_ohms = 10000.0
adc_max_code = 4095
topology = "ntc_to_ground"
saturation = { code = 65535, behavior = "error" }

[[transfer_families.ntc.members]]
selectors = { variant = "one" }
status = "emit"
applicability = { physical = [-40.0, 125.0] }
"#;
        let error = DefinitionsFile::from_toml_str(toml)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("transfer family `ntc`")
                && error.contains("misplaced `saturation` at `model.saturation`")
                && error.contains("directly under `[transfer_families.ntc]`"),
            "expected the misplaced family guard to fail, got: {error}"
        );
    }

    #[test]
    fn provenance_nested_in_a_family_point_fails_closed() {
        let toml = r#"
[transfer_families.misplaced]
provenance = { identity = "valid family citation" }
input_unit = "count"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
points = [
  { input = 1, output = 1.0, provenance = { identity = "silently ignored" } },
  { input = 10, output = 10.0 },
]

[[transfer_families.misplaced.members]]
selectors = { variant = "one" }
status = "emit"
applicability = { observation = [1, 10] }
"#;
        let error = DefinitionsFile::from_toml_str(toml)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("transfer family `misplaced`")
                && error.contains("misplaced `provenance` at `points[0].provenance`")
                && error.contains("directly under `[transfer_families.misplaced]`"),
            "expected the misplaced family citation to fail, got: {error}"
        );
    }

    #[test]
    fn provenance_nested_in_a_family_model_fails_closed() {
        let toml = r#"
[transfer_families.misplaced]
provenance = { identity = "valid family citation" }
input_unit = "adc_code"
output_unit = "degree_celsius"
output_scale = 1000
max_interpolation_error = 50
output_range = [-40.0, 125.0]

[transfer_families.misplaced.model]
kind = "ntc_beta_divider"
nominal_resistance_ohms = 10000.0
beta_kelvin = 3950.0
nominal_temperature_celsius = 25.0
fixed_resistance_ohms = 10000.0
adc_max_code = 4095
topology = "ntc_to_ground"
provenance = { identity = "silently ignored" }

[[transfer_families.misplaced.members]]
selectors = { variant = "one" }
status = "emit"
applicability = { physical = [-40.0, 125.0] }
"#;
        let error = DefinitionsFile::from_toml_str(toml)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("transfer family `misplaced`")
                && error.contains("misplaced `provenance` at `model.provenance`")
                && error.contains("directly under `[transfer_families.misplaced]`"),
            "expected the misplaced family citation to fail, got: {error}"
        );
    }

    #[test]
    fn family_point_unknown_fields_fail_closed_with_source_path() {
        let toml = r#"
[transfer_families.als]
provenance = { identity = "test fixture" }
input_unit = "count"
output_unit = "lux"
output_scale = 1000
max_interpolation_error = 1
points = [
  { input = 1, output = 0.1, scale = 42 },
  { input = 10, output = 1.0 },
]

[[transfer_families.als.members]]
selectors = { gain = "div4" }
status = "emit"
applicability = { observation = [1, 10] }
"#;
        let error = DefinitionsFile::from_toml_str(toml)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("transfer family `als`")
                && error.contains("unknown field `scale`")
                && error.contains("`points[0].scale`"),
            "expected the family point path to be named, got: {error}"
        );
    }

    #[test]
    fn family_ntc_unknown_fields_fail_closed_with_source_path() {
        let toml = r#"
[transfer_families.ntc]
provenance = { identity = "test fixture" }
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
scale = 42

[[transfer_families.ntc.members]]
selectors = { variant = "one" }
status = "emit"
applicability = { physical = [-40.0, 125.0] }
"#;
        let error = DefinitionsFile::from_toml_str(toml)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("transfer family `ntc`")
                && error.contains("unknown field `scale`")
                && error.contains("`model.scale`")
                && error.contains("`ntc_beta_divider`"),
            "expected the family NTC model path to be named, got: {error}"
        );
    }

    #[test]
    fn standalone_legacy_source_unreserved_fields_remain_permissive() {
        let toml = r#"
[transfers.points]
input_unit = "count"
output_unit = "lux"
output_scale = 1000
max_interpolation_error = 1
points = [
  { input = 1, output = 0.1, scale = 42 },
  { input = 10, output = 1.0 },
]

[transfers.ntc]
input_unit = "adc_code"
output_unit = "degree_celsius"
output_scale = 1000
max_interpolation_error = 50
output_range = [-40.0, 125.0]

[transfers.ntc.model]
kind = "ntc_beta_divider"
nominal_resistance_ohms = 10000.0
beta_kelvin = 3950.0
nominal_temperature_celsius = 25.0
fixed_resistance_ohms = 10000.0
adc_max_code = 4095
topology = "ntc_to_ground"
scale = 42
"#;
        let defs = DefinitionsFile::from_toml_str(toml).unwrap();
        assert!(defs.transfers().contains_key("points"));
        assert!(defs.transfers().contains_key("ntc"));
    }

    #[test]
    fn family_selector_may_be_named_saturation() {
        let toml = r#"
[transfer_families.valid]
provenance = { identity = "test fixture" }
input_unit = "count"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
points = [
  { input = 1, output = 1.0 },
  { input = 10, output = 10.0 },
]
selector_axes = { saturation = ["enabled"], observation_guard = [1] }

[[transfer_families.valid.members]]
selectors = { saturation = "enabled", observation_guard = 1 }
status = "emit"
applicability = { observation = [1, 10] }
"#;
        let defs = DefinitionsFile::from_toml_str(toml).unwrap();
        let selectors = &defs.transfer_families()["valid"].members[0].selectors;
        assert_eq!(
            selectors["saturation"],
            transfer::SelectorValue::String("enabled".into())
        );
        assert_eq!(
            selectors["observation_guard"],
            transfer::SelectorValue::Integer(1)
        );
    }

    #[test]
    fn unsupported_standalone_transfer_capability_fails_closed() {
        let toml =
            guarded_standalone_toml(true).replace("observation_guard_v1", "observation_guard_v2");
        let error = DefinitionsFile::from_toml_str(&toml)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("unsupported [transfers] capability `observation_guard_v2`"),
            "expected the unsupported capability to be named, got: {error}"
        );
    }

    #[test]
    fn standalone_capabilities_are_incompatible_with_legacy_transfer_map_shape() {
        #[allow(dead_code)]
        #[derive(Debug, Deserialize)]
        struct LegacyTransferDef {
            input_unit: String,
            output_unit: String,
            output_scale: u32,
            max_interpolation_error: u32,
            max_knots: Option<usize>,
            below: Option<String>,
            above: Option<String>,
            points: Option<Vec<toml::Value>>,
            formula: Option<String>,
            model: Option<toml::Value>,
            domain: Option<[u16; 2]>,
            output_range: Option<[f64; 2]>,
        }

        let unversioned: toml::Value = guarded_standalone_toml(false).parse().unwrap();
        let silently_unguarded = unversioned
            .get("transfers")
            .unwrap()
            .clone()
            .try_into::<BTreeMap<String, LegacyTransferDef>>()
            .unwrap();
        assert!(silently_unguarded.contains_key("guarded"));

        let document: toml::Value = guarded_standalone_toml(true).parse().unwrap();
        let legacy_section = document.get("transfers").unwrap().clone();
        let error = legacy_section
            .try_into::<BTreeMap<String, LegacyTransferDef>>()
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("requires")
                && (error.contains("invalid type") || error.contains("invalid length")),
            "legacy transfer-map decoder unexpectedly accepted the marker: {error}"
        );

        let unversioned: toml::Value = provenance_standalone_toml(false).parse().unwrap();
        let silently_uncited = unversioned
            .get("transfers")
            .unwrap()
            .clone()
            .try_into::<BTreeMap<String, LegacyTransferDef>>()
            .unwrap();
        assert!(silently_uncited.contains_key("cited"));

        let document: toml::Value = provenance_standalone_toml(true).parse().unwrap();
        let legacy_section = document.get("transfers").unwrap().clone();
        let error = legacy_section
            .try_into::<BTreeMap<String, LegacyTransferDef>>()
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("requires")
                && (error.contains("invalid type") || error.contains("invalid length")),
            "legacy transfer-map decoder unexpectedly accepted the source marker: {error}"
        );
    }

    #[test]
    fn requires_table_remains_a_legacy_transfer_name() {
        let toml = r#"
[transfers.requires]
input_unit = "count"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
formula = "x"
domain = [1, 10]
"#;
        let defs = DefinitionsFile::from_toml_str(toml).unwrap();
        assert!(defs.transfers().contains_key("requires"));
    }

    #[test]
    fn requires_transfer_must_be_renamed_before_adding_a_guard() {
        let toml = r#"
[transfers.requires]
input_unit = "count"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
formula = "x"
domain = [1, 10]

[transfers.guarded]
input_unit = "count"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
saturation = { code = 65535, behavior = "error" }
formula = "x"
domain = [1, 10]
"#;
        let error = DefinitionsFile::from_toml_str(toml)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("transfer named `requires` cannot coexist")
                && error.contains("rename that transfer"),
            "expected an actionable reserved-name diagnostic, got: {error}"
        );
    }
}
