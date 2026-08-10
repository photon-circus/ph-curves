//! Physical transfer-function schema and generation.

// Host-only: module-local std link (crate root stays `#![no_std]`).
extern crate std;

use std::prelude::v1::*;
use std::format;

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

#[derive(Debug)]
pub struct TransferData {
    pub inputs: Vec<u16>,
    pub outputs: Vec<i32>,
    pub direction: MonotonicDirection,
    pub achieved_max_error: u32,
    pub achieved_max_error_exact: f64,
    pub worst_case_input: u16,
    pub provenance: String,
}

pub fn build(name: &str, def: &TransferDef) -> Result<TransferData, String> {
    if def.output_scale == 0 {
        return Err(format!("transfer `{name}`: output_scale must be positive"));
    }
    if !(2..=ABSOLUTE_MAX_KNOTS).contains(&def.max_knots) {
        return Err(format!(
            "transfer `{name}`: max_knots must be in 2..={ABSOLUTE_MAX_KNOTS}"
        ));
    }

    let source_count =
        def.points.is_some() as u8 + def.formula.is_some() as u8 + def.model.is_some() as u8;
    if source_count != 1 {
        return Err(format!(
            "transfer `{name}`: exactly one of points, formula, or model must be specified"
        ));
    }

    let (domain_min, truth, provenance) = if let Some(control_points) = &def.points {
        if def.domain.is_some() || def.output_range.is_some() {
            return Err(format!(
                "transfer `{name}`: points define their domain; domain and output_range are forbidden"
            ));
        }
        let (minimum, physical) = points::evaluate(name, control_points)?;
        (
            minimum,
            scale_truth(name, &physical, def.output_scale)?,
            format!("physical points ({} control points)", control_points.len()),
        )
    } else if let Some(expression) = &def.formula {
        if def.output_range.is_some() {
            return Err(format!(
                "transfer `{name}`: output_range is forbidden for formula sources"
            ));
        }
        let [minimum, maximum] = def
            .domain
            .ok_or_else(|| format!("transfer `{name}`: formula requires domain = [min, max]"))?;
        if minimum >= maximum {
            return Err(format!(
                "transfer `{name}`: domain must be strictly increasing"
            ));
        }
        let parsed = formula::Formula::parse(expression)
            .map_err(|error| format!("transfer `{name}`: {error}"))?;
        let physical: Vec<f64> = (minimum..=maximum)
            .map(|input| {
                parsed
                    .eval("x", f64::from(input))
                    .map_err(|error| format!("transfer `{name}`: {error}"))
            })
            .collect::<Result<_, _>>()?;
        (
            minimum,
            scale_truth(name, &physical, def.output_scale)?,
            format!("formula y = {expression}"),
        )
    } else {
        if def.domain.is_some() {
            return Err(format!(
                "transfer `{name}`: domain is forbidden for model sources"
            ));
        }
        let output_range = def
            .output_range
            .ok_or_else(|| format!("transfer `{name}`: model requires output_range"))?;
        let (minimum, physical, description) = model::evaluate(
            name,
            def.model.as_ref().expect("source count checked"),
            output_range,
        )?;
        (
            minimum,
            scale_truth(name, &physical, def.output_scale)?,
            description,
        )
    };

    let direction = validate_monotonic(name, &truth)?;
    let result = adaptive::fit(
        name,
        domain_min,
        &truth,
        def.max_interpolation_error,
        def.max_knots,
    )?;

    Ok(TransferData {
        inputs: result.inputs,
        outputs: result.outputs,
        direction,
        achieved_max_error: result.achieved_max_error,
        achieved_max_error_exact: result.achieved_max_error_exact,
        worst_case_input: result.worst_case_input,
        provenance,
    })
}

fn scale_truth(name: &str, physical: &[f64], output_scale: u32) -> Result<Vec<f64>, String> {
    physical
        .iter()
        .enumerate()
        .map(|(offset, &value)| {
            let scaled = value * f64::from(output_scale);
            if !scaled.is_finite() {
                return Err(format!(
                    "transfer `{name}`: non-finite output at domain offset {offset}"
                ));
            }
            if scaled.round() < f64::from(i32::MIN) || scaled.round() > f64::from(i32::MAX) {
                return Err(format!(
                    "transfer `{name}`: output at domain offset {offset} does not fit i32"
                ));
            }
            Ok(scaled)
        })
        .collect()
}

fn validate_monotonic(name: &str, truth: &[f64]) -> Result<MonotonicDirection, String> {
    if truth.len() < 2 {
        return Err(format!(
            "transfer `{name}`: domain must contain at least two inputs"
        ));
    }
    let direction = if truth.last().expect("truth length checked") < &truth[0] {
        MonotonicDirection::Decreasing
    } else {
        MonotonicDirection::Increasing
    };

    for (offset, pair) in truth.windows(2).enumerate() {
        let valid = match direction {
            MonotonicDirection::Increasing => pair[1] >= pair[0],
            MonotonicDirection::Decreasing => pair[1] <= pair[0],
        };
        if !valid {
            return Err(format!(
                "transfer `{name}`: source is not monotonic at domain offset {}",
                offset + 1
            ));
        }
    }
    Ok(direction)
}

/// Exhaustively measure the worst round-trip code error over the input domain.
///
/// Returns `max |invert(convert(code)) - code|` for every code in
/// `inputs[0]..=inputs[last]`, under the default
/// [`FlatResolution::PreferLowInput`] policy that generated tables carry.
///
/// The segment arithmetic comes from the crate's own `interpolate_segment`
/// and `invert_segment`, so the host audit rounds exactly the way the
/// runtime does. `tests/ntc_transfer.rs` re-measures the emitted table at
/// runtime and asserts it matches the value recorded here, which is what
/// catches any drift between this search and
/// `PiecewiseLinearTransfer::invert`.
pub fn measure_inverse_code_error(
    inputs: &[u16],
    outputs: &[i32],
    direction: MonotonicDirection,
) -> u16 {
    let mut worst = 0u16;
    for code in inputs[0]..=inputs[inputs.len() - 1] {
        let physical = convert_code(inputs, outputs, code);
        let recovered = invert_physical(inputs, outputs, direction, physical);
        worst = worst.max(recovered.abs_diff(code));
    }
    worst
}

/// Forward-convert one in-domain code against the sparse knot table.
fn convert_code(inputs: &[u16], outputs: &[i32], code: u16) -> i32 {
    let left = match inputs.binary_search(&code) {
        Ok(index) => return outputs[index],
        // `code >= inputs[0]`, so the insertion point is never 0.
        Err(index) => index - 1,
    };
    crate::interpolate_segment(
        code,
        inputs[left],
        outputs[left],
        inputs[left + 1],
        outputs[left + 1],
    )
    .unwrap_or_else(|error| panic!("internal interpolation error: {error:?}"))
}

/// Invert one in-range physical value, mirroring the runtime search.
fn invert_physical(
    inputs: &[u16],
    outputs: &[i32],
    direction: MonotonicDirection,
    physical: i32,
) -> u16 {
    // Largest knot index on the inclusive low-physical side of `physical`.
    let (mut low, mut high) = (0usize, inputs.len() - 1);
    while low < high {
        let middle = low + (high - low).div_ceil(2);
        let past = match direction {
            MonotonicDirection::Increasing => outputs[middle] <= physical,
            MonotonicDirection::Decreasing => outputs[middle] >= physical,
        };
        if past {
            low = middle;
        } else {
            high = middle - 1;
        }
    }

    if outputs[low] == physical {
        // FlatResolution::PreferLowInput: walk to the start of the flat run.
        let mut left = low;
        while left > 0 && outputs[left - 1] == physical {
            left -= 1;
        }
        return inputs[left];
    }

    crate::invert_segment(
        physical,
        inputs[low],
        outputs[low],
        inputs[low + 1],
        outputs[low + 1],
    )
    .unwrap_or_else(|error| panic!("internal inversion error: {error:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use model::{DividerTopology, ModelDef};
    use std::vec;

    #[test]
    fn inverse_code_error_is_zero_for_a_faithful_table() {
        // One physical quantum per code: every code survives the round trip.
        let inputs = [0u16, 100];
        let outputs = [0i32, 100];
        assert_eq!(
            measure_inverse_code_error(&inputs, &outputs, MonotonicDirection::Increasing),
            0
        );
    }

    #[test]
    fn inverse_code_error_is_measured_on_a_coarse_table() {
        // 1001 codes share 11 physical values, so most codes land on a
        // plateau and invert back to that plateau's representative code.
        let inputs = [0u16, 1000];
        let outputs = [0i32, 10];
        let worst = measure_inverse_code_error(&inputs, &outputs, MonotonicDirection::Increasing);
        assert!(
            worst > 0,
            "coarse output scale must report a nonzero round-trip bound"
        );
        assert_eq!(worst, 50);
    }

    #[test]
    fn inverse_code_error_covers_decreasing_tables() {
        let inputs = [0u16, 1000];
        let outputs = [10i32, 0];
        let worst = measure_inverse_code_error(&inputs, &outputs, MonotonicDirection::Decreasing);
        assert_eq!(worst, 50);
    }

    #[test]
    fn inverse_code_error_reports_flat_run_width() {
        // A flat run over codes 10..=20 resolves to its low input, so code 20
        // comes back as 10.
        let inputs = [0u16, 10, 20, 30];
        let outputs = [0i32, 10, 10, 20];
        assert_eq!(
            measure_inverse_code_error(&inputs, &outputs, MonotonicDirection::Increasing),
            10
        );
    }

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
        let data = build("ntc", &ntc_def(DividerTopology::NtcToGround)).unwrap();
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
        let data = build("ntc", &ntc_def(DividerTopology::NtcToSupply)).unwrap();
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
        )
        .unwrap();
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
        )
        .unwrap();
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
        )
        .unwrap();
        assert_eq!(data.inputs, vec![2, 10]);
        assert_eq!(data.outputs, vec![2_000, 10_000]);
    }

    #[test]
    fn knot_cap_prevents_full_domain_fallback() {
        let mut def = base_def();
        def.formula = Some("x * x".into());
        def.domain = Some([0, 100]);
        def.max_interpolation_error = 0;
        def.max_knots = 2;
        assert!(
            build("bounded", &def)
                .unwrap_err()
                .contains("greedy fitter did not meet maximum error")
        );
    }
}
