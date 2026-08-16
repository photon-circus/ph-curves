//! Physical transfer-function schema and generation.

// Host-only: module-local std link (crate root stays `#![no_std]`).
extern crate std;

use std::format;
use std::prelude::v1::*;

mod adaptive;
pub(crate) mod family;
mod model;
mod points;
mod source;

use crate::MonotonicDirection;
use serde::Deserialize;

use super::formula;

pub use family::{
    ApplicabilityDef, DeclaredSource, FamilyMemberDef, GapDef, GapStatus, MemberStatus,
    SelectorValue, TransferFamilyDef,
};
pub use source::{EvaluatedTruth, TransferSource, TransferSpec};

const ABSOLUTE_MAX_KNOTS: usize = 4096;

/// How a generated transfer treats observations outside its domain.
#[derive(Copy, Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum BoundaryDef {
    /// Out-of-domain observations return an error.
    Error,
    /// Out-of-domain observations clamp to the nearest endpoint.
    Clamp,
}

impl BoundaryDef {
    pub(crate) fn rust_name(self) -> &'static str {
        match self {
            Self::Error => "BoundaryBehavior::Error",
            Self::Clamp => "BoundaryBehavior::Clamp",
        }
    }
}

pub(crate) fn default_boundary() -> BoundaryDef {
    BoundaryDef::Error
}

fn default_max_knots() -> usize {
    256
}

/// One physical control point: integer observation to unscaled physical output.
#[derive(Clone, Debug, Deserialize)]
pub struct PhysicalPoint {
    /// Observation-domain input code.
    pub input: u16,
    /// Unscaled physical output at this input.
    pub output: f64,
}

impl PhysicalPoint {
    /// Construct a control point.
    pub fn new(input: u16, output: f64) -> Self {
        Self { input, output }
    }
}

/// Parsed standalone transfer, or the policy copied onto an expanded family member.
///
/// Built-in model parameters stay crate-private; use [`Self::has_model`] and
/// [`Self::declared_source`] to inspect the source kind.
#[derive(Clone, Debug, Deserialize)]
pub struct TransferDef {
    /// Observation-domain unit label.
    pub input_unit: String,
    /// Physical-domain unit label.
    pub output_unit: String,
    /// Integer output quanta per physical unit.
    pub output_scale: u32,
    /// Requested interpolation error bound in output quanta.
    pub max_interpolation_error: u32,
    /// Knot budget (default 256, hard cap 4096 for standalone transfers).
    #[serde(default = "default_max_knots")]
    pub max_knots: usize,
    /// Below-domain policy.
    #[serde(default = "default_boundary")]
    pub below: BoundaryDef,
    /// Above-domain policy.
    #[serde(default = "default_boundary")]
    pub above: BoundaryDef,
    /// Sparse physical control points, when that is the declared source.
    pub points: Option<Vec<PhysicalPoint>>,
    /// Formula over `x`, when that is the declared source.
    pub formula: Option<String>,
    pub(crate) model: Option<model::ModelDef>,
    /// Inclusive observation domain, required for formula and scaled-polynomial sources.
    pub domain: Option<[u16; 2]>,
    /// Physical output window, required for output-range models (NTC Beta-divider).
    pub output_range: Option<[f64; 2]>,
}

impl TransferDef {
    /// Shared formula text, when the source is a formula.
    pub fn formula_text(&self) -> Option<&str> {
        self.formula.as_deref()
    }

    /// Physical control points, when the source is points.
    pub fn control_points(&self) -> Option<&[PhysicalPoint]> {
        self.points.as_deref()
    }

    /// Whether the source is a built-in host model.
    pub fn has_model(&self) -> bool {
        self.model.is_some()
    }

    /// Which of formula, points, or model is set. `None` if missing or mixed.
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
    build_with_source(name, def, None)
}

pub(crate) fn build_with_source(
    name: &str,
    def: &TransferDef,
    overlay: Option<&TransferSource>,
) -> Result<TransferData, String> {
    if def.output_scale == 0 {
        return Err(format!("transfer `{name}`: output_scale must be positive"));
    }
    if !(2..=ABSOLUTE_MAX_KNOTS).contains(&def.max_knots) {
        return Err(format!(
            "transfer `{name}`: max_knots must be in 2..={ABSOLUTE_MAX_KNOTS}"
        ));
    }

    if let Some(overlay) = overlay {
        return build_from_overlay(name, def, overlay);
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
    } else if let Some(model::ModelDef::ScaledPolynomial {
        coefficients,
        scale,
    }) = &def.model
    {
        if def.output_range.is_some() {
            return Err(format!(
                "transfer `{name}`: output_range is forbidden for scaled_polynomial; use domain"
            ));
        }
        let scale =
            scale.ok_or_else(|| format!("transfer `{name}`: scaled_polynomial requires scale"))?;
        let domain = def.domain.ok_or_else(|| {
            format!("transfer `{name}`: scaled_polynomial requires domain = [min, max]")
        })?;
        let (minimum, physical, description) =
            model::evaluate_scaled_polynomial(name, coefficients, scale, domain)?;
        (
            minimum,
            scale_truth(name, &physical, def.output_scale)?,
            description,
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

    finish_from_scaled_truth(name, def, domain_min, &truth, provenance)
}

fn build_from_overlay(
    name: &str,
    def: &TransferDef,
    overlay: &TransferSource,
) -> Result<TransferData, String> {
    match overlay {
        TransferSource::EvaluatedTruth(truth) => {
            let (domain_min, scaled, provenance) = evaluated_truth_to_scaled(name, def, truth)?;
            finish_from_scaled_truth(name, def, domain_min, &scaled, provenance)
        }
        TransferSource::Points(control_points) => {
            let (minimum, physical) = points::evaluate(name, control_points)?;
            let scaled = scale_truth(name, &physical, def.output_scale)?;
            finish_from_scaled_truth(
                name,
                def,
                minimum,
                &scaled,
                format!("physical points ({} control points)", control_points.len()),
            )
        }
        TransferSource::PrefittedKnots {
            inputs,
            outputs,
            truth,
        } => build_prefitted(name, def, inputs, outputs, truth),
    }
}

fn evaluated_truth_to_scaled(
    name: &str,
    def: &TransferDef,
    truth: &EvaluatedTruth,
) -> Result<(u16, Vec<f64>, String), String> {
    let physical = truth.physical();
    if physical.len() < 2 {
        return Err(format!(
            "transfer `{name}`: evaluated truth must contain at least two samples"
        ));
    }
    let last_offset = physical.len() - 1;
    if last_offset > usize::from(u16::MAX - truth.domain_min()) {
        return Err(format!(
            "transfer `{name}`: evaluated truth domain exceeds u16"
        ));
    }
    let scaled = scale_truth(name, physical, def.output_scale)?;
    Ok((
        truth.domain_min(),
        scaled,
        format!("evaluated physical truth ({} samples)", physical.len()),
    ))
}

fn finish_from_scaled_truth(
    name: &str,
    def: &TransferDef,
    domain_min: u16,
    truth: &[f64],
    provenance: String,
) -> Result<TransferData, String> {
    let direction = validate_monotonic(name, truth)?;
    let result = adaptive::fit(
        name,
        domain_min,
        truth,
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

fn build_prefitted(
    name: &str,
    def: &TransferDef,
    inputs: &[u16],
    outputs: &[i32],
    truth: &EvaluatedTruth,
) -> Result<TransferData, String> {
    if inputs.len() != outputs.len() {
        return Err(format!(
            "transfer `{name}`: prefitted inputs and outputs must have the same length"
        ));
    }
    if inputs.len() < 2 {
        return Err(format!(
            "transfer `{name}`: prefitted knots must contain at least two entries"
        ));
    }
    if inputs.len() > def.max_knots {
        return Err(format!(
            "transfer `{name}`: prefitted knot count {} exceeds max_knots={}",
            inputs.len(),
            def.max_knots
        ));
    }
    for pair in inputs.windows(2) {
        if pair[1] <= pair[0] {
            return Err(format!(
                "transfer `{name}`: prefitted inputs must be strictly increasing"
            ));
        }
    }

    let scaled_knots: Vec<f64> = outputs.iter().map(|&value| f64::from(value)).collect();
    let direction = validate_monotonic(name, &scaled_knots)?;

    let (domain_min, scaled, _) = evaluated_truth_to_scaled(name, def, truth)?;
    if domain_min != inputs[0] {
        return Err(format!(
            "transfer `{name}`: prefitted truth domain_min must match the first knot"
        ));
    }
    let last = *inputs.last().expect("knot count checked");
    let expected_len = usize::from(last - domain_min) + 1;
    if scaled.len() != expected_len {
        return Err(format!(
            "transfer `{name}`: prefitted truth must cover {domain_min}..={last} ({expected_len} samples)"
        ));
    }
    let knot_offsets: Vec<usize> = inputs
        .iter()
        .map(|&input| usize::from(input - domain_min))
        .collect();
    let (worst_offset, worst_error) =
        adaptive::measure_error(domain_min, &scaled, &knot_offsets, outputs)?;
    if worst_error > f64::from(def.max_interpolation_error) {
        return Err(format!(
            "transfer `{name}`: prefitted knots exceed maximum error {}; \
                 measured {worst_error:.6} at input {}",
            def.max_interpolation_error,
            domain_min + worst_offset as u16
        ));
    }

    Ok(TransferData {
        inputs: inputs.to_vec(),
        outputs: outputs.to_vec(),
        direction,
        achieved_max_error: worst_error.ceil() as u32,
        achieved_max_error_exact: worst_error,
        worst_case_input: domain_min + worst_offset as u16,
        provenance: format!(
            "prefitted knots ({} knots) verified against evaluated truth",
            inputs.len()
        ),
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

    fn scaled_poly(
        coefficients: Vec<f64>,
        scale: u32,
        domain: [u16; 2],
        output_scale: u32,
    ) -> TransferDef {
        TransferDef {
            output_scale,
            max_interpolation_error: 1,
            max_knots: 64,
            model: Some(ModelDef::ScaledPolynomial {
                coefficients,
                scale: Some(scale),
            }),
            domain: Some(domain),
            ..base_def()
        }
    }

    #[test]
    fn scaled_polynomial_exact_half_quantum_tie_quantizes_away_from_zero() {
        let (minimum, physical, _) =
            model::evaluate_scaled_polynomial("tie", &[0.0, 0.5], 33_600, [1875, 1876]).unwrap();
        assert_eq!(minimum, 1875);
        assert_eq!(physical[0], 31.5);
        assert_eq!(physical[0].round() as i32, 32);

        let data = build("tie", &scaled_poly(vec![0.0, 0.5], 33_600, [1875, 1876], 1)).unwrap();
        assert_eq!(data.inputs[0], 1875);
        assert_eq!(data.outputs[0], 32);
    }

    #[test]
    fn scaled_polynomial_reproduces_the_vendor_worked_example() {
        // Vishay AN84323 rev 06-Mar-2025 p.5: 5581 counts at ×1/4 100 ms.
        // Exact u is 1500.1728 (vendor prints 1500 lx); both round to 1658 lx
        // at output_scale = 1. Milli-lux / knot budget stays issue #29.
        let data = build(
            "als",
            &scaled_poly(
                vec![0.0, 1.0023, 8.1488e-5, -9.3924e-9, 6.0135e-13],
                268_800,
                [5581, 5582],
                1,
            ),
        )
        .unwrap();
        assert_eq!(data.inputs[0], 5581);
        assert_eq!(data.outputs[0], 1658);
    }

    #[test]
    fn scaled_polynomial_standalone_may_include_u16_max() {
        let data = build(
            "full",
            &scaled_poly(vec![0.0, 1.0], 1_000, [65534, 65535], 1),
        )
        .unwrap();
        assert_eq!(*data.inputs.last().unwrap(), u16::MAX);
        assert!(!data.provenance.contains("saturation"));
    }

    #[test]
    fn scaled_polynomial_rejects_empty_coefficients() {
        let error = build("empty", &scaled_poly(vec![], 33_600, [1, 2], 1)).unwrap_err();
        assert!(error.contains("coefficients must not be empty"), "{error}");
    }

    #[test]
    fn scaled_polynomial_rejects_non_finite_coefficients() {
        let error = build("nan", &scaled_poly(vec![0.0, f64::NAN], 33_600, [1, 2], 1)).unwrap_err();
        assert!(error.contains("coefficient 1 must be finite"), "{error}");
    }

    #[test]
    fn scaled_polynomial_rejects_zero_scale() {
        let error = build("zero", &scaled_poly(vec![0.0, 1.0], 0, [1, 2], 1)).unwrap_err();
        assert!(error.contains("scale must be positive"), "{error}");
    }

    #[test]
    fn scaled_polynomial_requires_scale_on_standalone_definitions() {
        let mut def = scaled_poly(vec![0.0, 1.0], 1, [1, 2], 1);
        if let Some(ModelDef::ScaledPolynomial { scale, .. }) = &mut def.model {
            *scale = None;
        }
        let error = build("missing", &def).unwrap_err();
        assert!(error.contains("requires scale"), "{error}");
    }

    #[test]
    fn scaled_polynomial_rejects_invalid_domain() {
        let error = build("flat", &scaled_poly(vec![0.0, 1.0], 33_600, [10, 10], 1)).unwrap_err();
        assert!(
            error.contains("domain must be strictly increasing"),
            "{error}"
        );
    }

    #[test]
    fn scaled_polynomial_rejects_non_monotonic_truth() {
        // y = (u - 2)^2 is not monotonic across u = 0..=4.
        let error = build(
            "quad",
            &scaled_poly(vec![4.0, -4.0, 1.0], 1_000_000, [0, 4], 1),
        )
        .unwrap_err();
        assert!(error.contains("not monotonic"), "{error}");
    }

    #[test]
    fn scaled_polynomial_rejects_non_finite_output() {
        let error = build("inf", &scaled_poly(vec![0.0, 1e308], 1_000_000, [1, 2], 1)).unwrap_err();
        assert!(error.contains("non-finite"), "{error}");
    }

    #[test]
    fn scaled_polynomial_rejects_scaled_i32_overflow() {
        let error = build(
            "overflow",
            &scaled_poly(vec![0.0, 1.0], 1_000_000, [65534, 65535], 100_000),
        )
        .unwrap_err();
        assert!(error.contains("does not fit i32"), "{error}");
    }

    #[test]
    fn scaled_polynomial_forbids_output_range() {
        let mut def = scaled_poly(vec![0.0, 1.0], 33_600, [1, 2], 1);
        def.output_range = Some([0.0, 1.0]);
        let error = build("range", &def).unwrap_err();
        assert!(error.contains("output_range is forbidden"), "{error}");
    }
}
