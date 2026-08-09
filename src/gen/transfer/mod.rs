//! Physical transfer-function schema and generation.

mod adaptive;
mod model;
mod points;

use crate::MonotonicDirection;
use serde::Deserialize;

use super::formula;

const ABSOLUTE_MAX_KNOTS: usize = 4096;

#[derive(Copy, Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum BoundaryDef {
    Error,
    Clamp,
}

impl BoundaryDef {
    pub fn rust_name(self) -> &'static str {
        match self {
            Self::Error => "BoundaryBehavior::Error",
            Self::Clamp => "BoundaryBehavior::Clamp",
        }
    }
}

fn default_boundary() -> BoundaryDef {
    BoundaryDef::Error
}

fn default_max_knots() -> usize {
    256
}

#[derive(Clone, Debug, Deserialize)]
pub struct PhysicalPoint {
    pub input: u16,
    pub output: f64,
}

#[derive(Debug, Deserialize)]
pub struct TransferDef {
    pub input_unit: String,
    pub output_unit: String,
    pub output_scale: u32,
    pub max_interpolation_error: u32,
    #[serde(default = "default_max_knots")]
    pub max_knots: usize,
    #[serde(default = "default_boundary")]
    pub below: BoundaryDef,
    #[serde(default = "default_boundary")]
    pub above: BoundaryDef,
    pub points: Option<Vec<PhysicalPoint>>,
    pub formula: Option<String>,
    pub model: Option<model::ModelDef>,
    pub domain: Option<[u16; 2]>,
    pub output_range: Option<[f64; 2]>,
}

pub struct TransferData {
    pub inputs: Vec<u16>,
    pub outputs: Vec<i32>,
    pub direction: MonotonicDirection,
    pub achieved_max_error: u32,
    pub achieved_max_error_exact: f64,
    pub worst_case_input: u16,
    pub provenance: String,
}

pub fn build(name: &str, def: &TransferDef) -> TransferData {
    assert!(
        def.output_scale > 0,
        "transfer `{name}`: output_scale must be positive"
    );
    assert!(
        (2..=ABSOLUTE_MAX_KNOTS).contains(&def.max_knots),
        "transfer `{name}`: max_knots must be in 2..={ABSOLUTE_MAX_KNOTS}"
    );

    let source_count =
        def.points.is_some() as u8 + def.formula.is_some() as u8 + def.model.is_some() as u8;
    assert!(
        source_count == 1,
        "transfer `{name}`: exactly one of points, formula, or model must be specified"
    );

    let (domain_min, truth, provenance) = if let Some(control_points) = &def.points {
        assert!(
            def.domain.is_none() && def.output_range.is_none(),
            "transfer `{name}`: points define their domain; domain and output_range are forbidden"
        );
        let (minimum, physical) = points::evaluate(name, control_points);
        (
            minimum,
            scale_truth(name, &physical, def.output_scale),
            format!("physical points ({} control points)", control_points.len()),
        )
    } else if let Some(expression) = &def.formula {
        assert!(
            def.output_range.is_none(),
            "transfer `{name}`: output_range is forbidden for formula sources"
        );
        let [minimum, maximum] = def
            .domain
            .unwrap_or_else(|| panic!("transfer `{name}`: formula requires domain = [min, max]"));
        assert!(
            minimum < maximum,
            "transfer `{name}`: domain must be strictly increasing"
        );
        let parsed = formula::Formula::parse(expression);
        let physical: Vec<f64> = (minimum..=maximum)
            .map(|input| parsed.eval("x", f64::from(input)))
            .collect();
        (
            minimum,
            scale_truth(name, &physical, def.output_scale),
            format!("formula y = {expression}"),
        )
    } else {
        assert!(
            def.domain.is_none(),
            "transfer `{name}`: domain is forbidden for model sources"
        );
        let output_range = def
            .output_range
            .unwrap_or_else(|| panic!("transfer `{name}`: model requires output_range"));
        let (minimum, physical, description) =
            model::evaluate(name, def.model.as_ref().unwrap(), output_range);
        (
            minimum,
            scale_truth(name, &physical, def.output_scale),
            description,
        )
    };

    let direction = validate_monotonic(name, &truth);
    let result = adaptive::fit(
        name,
        domain_min,
        &truth,
        def.max_interpolation_error,
        def.max_knots,
    );

    TransferData {
        inputs: result.inputs,
        outputs: result.outputs,
        direction,
        achieved_max_error: result.achieved_max_error,
        achieved_max_error_exact: result.achieved_max_error_exact,
        worst_case_input: result.worst_case_input,
        provenance,
    }
}

fn scale_truth(name: &str, physical: &[f64], output_scale: u32) -> Vec<f64> {
    physical
        .iter()
        .enumerate()
        .map(|(offset, &value)| {
            let scaled = value * f64::from(output_scale);
            assert!(
                scaled.is_finite(),
                "transfer `{name}`: non-finite output at domain offset {offset}"
            );
            assert!(
                scaled.round() >= f64::from(i32::MIN) && scaled.round() <= f64::from(i32::MAX),
                "transfer `{name}`: output at domain offset {offset} does not fit i32"
            );
            scaled
        })
        .collect()
}

fn validate_monotonic(name: &str, truth: &[f64]) -> MonotonicDirection {
    assert!(
        truth.len() >= 2,
        "transfer `{name}`: domain must contain at least two inputs"
    );
    let direction = if truth.last().unwrap() < &truth[0] {
        MonotonicDirection::Decreasing
    } else {
        MonotonicDirection::Increasing
    };

    for (offset, pair) in truth.windows(2).enumerate() {
        let valid = match direction {
            MonotonicDirection::Increasing => pair[1] >= pair[0],
            MonotonicDirection::Decreasing => pair[1] <= pair[0],
        };
        assert!(
            valid,
            "transfer `{name}`: source is not monotonic at domain offset {}",
            offset + 1
        );
    }
    direction
}

#[cfg(test)]
mod tests {
    use super::*;
    use model::{DividerTopology, ModelDef};

    fn base_def() -> TransferDef {
        TransferDef {
            input_unit: "adc_code".into(),
            output_unit: "degree_celsius".into(),
            output_scale: 1000,
            max_interpolation_error: 50,
            max_knots: 256,
            below: BoundaryDef::Error,
            above: BoundaryDef::Error,
            points: None,
            formula: None,
            model: None,
            domain: None,
            output_range: None,
        }
    }

    fn ntc_def(topology: DividerTopology) -> TransferDef {
        TransferDef {
            model: Some(ModelDef::NtcBetaDivider {
                nominal_resistance_ohms: 10_000.0,
                beta_kelvin: 3950.0,
                nominal_temperature_celsius: 25.0,
                fixed_resistance_ohms: 10_000.0,
                adc_max_code: 4095,
                topology,
            }),
            output_range: Some([-40.0, 125.0]),
            ..base_def()
        }
    }

    #[test]
    fn reference_ntc_meets_sparse_error_target() {
        let data = build("ntc", &ntc_def(DividerTopology::NtcToGround));
        assert_eq!(data.inputs.first(), Some(&142));
        assert_eq!(data.inputs.last(), Some(&3995));
        assert_eq!(data.inputs.len(), 61);
        assert_eq!(data.outputs.first(), Some(&124_957));
        assert_eq!(data.outputs.last(), Some(&-39_919));
        assert_eq!(data.direction, MonotonicDirection::Decreasing);
        assert!(data.achieved_max_error_exact <= 50.0);
    }

    #[test]
    fn opposite_ntc_topology_reverses_direction() {
        let data = build("ntc", &ntc_def(DividerTopology::NtcToSupply));
        assert_eq!(data.inputs.first(), Some(&100));
        assert_eq!(data.inputs.last(), Some(&3953));
        assert_eq!(data.direction, MonotonicDirection::Increasing);
        assert!(data.achieved_max_error_exact <= 50.0);
    }

    #[test]
    fn physical_points_are_not_normalized() {
        let data = build(
            "points",
            &TransferDef {
                points: Some(vec![
                    PhysicalPoint {
                        input: 100,
                        output: -10.0,
                    },
                    PhysicalPoint {
                        input: 200,
                        output: 40.0,
                    },
                ]),
                ..base_def()
            },
        );
        assert_eq!(data.inputs, vec![100, 200]);
        assert_eq!(data.outputs, vec![-10_000, 40_000]);
    }

    #[test]
    fn formula_uses_physical_x_variable() {
        let data = build(
            "formula",
            &TransferDef {
                formula: Some("x * 0.5 - 10".into()),
                domain: Some([20, 100]),
                ..base_def()
            },
        );
        assert_eq!(data.inputs, vec![20, 100]);
        assert_eq!(data.outputs, vec![0, 40_000]);
    }

    #[test]
    fn formula_validation_uses_declared_domain_values() {
        let data = build(
            "formula",
            &TransferDef {
                formula: Some("clamp(x, 1, x)".into()),
                domain: Some([2, 10]),
                ..base_def()
            },
        );
        assert_eq!(data.inputs, vec![2, 10]);
        assert_eq!(data.outputs, vec![2_000, 10_000]);
    }

    #[test]
    #[should_panic(expected = "greedy fitter did not meet maximum error")]
    fn knot_cap_prevents_full_domain_fallback() {
        let mut def = base_def();
        def.formula = Some("x * x".into());
        def.domain = Some([0, 100]);
        def.max_interpolation_error = 0;
        def.max_knots = 2;
        build("bounded", &def);
    }
}
