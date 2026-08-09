//! Curve definition, building, and validation.
//!
//! This module owns the TOML schema types and orchestrates curve
//! construction by dispatching to [`super::builtin`], [`super::formula`],
//! and [`super::points`].

use std::collections::BTreeMap;

use serde::Deserialize;

use super::{builtin, formula, points, transfer};

// ---------------------------------------------------------------------------
// TOML schema
//
// A curve is defined by exactly ONE of: `builtin`, `formula`, or `points`.
// ---------------------------------------------------------------------------

/// Parsed TOML definitions for normalized curves and physical transfers.
///
/// Construct via [`Self::from_toml_str`] or [`super::generate_from_str`].
/// Field access is crate-visible; dependents should prefer the `generate_*`
/// helpers over hand-building schema graphs.
#[derive(Debug, Deserialize)]
pub struct DefinitionsFile {
    /// Normalized LUT curves keyed by TOML table name.
    #[serde(default)]
    pub(crate) curves: BTreeMap<String, CurveDef>,
    /// Sparse physical transfer functions keyed by TOML table name.
    #[serde(default)]
    pub(crate) transfers: BTreeMap<String, transfer::TransferDef>,
}

impl DefinitionsFile {
    /// Parse a TOML definitions document.
    pub fn from_toml_str(toml: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(toml)
    }
}

/// One normalized curve definition from the TOML `[curves]` map.
#[derive(Debug, Deserialize)]
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
}
