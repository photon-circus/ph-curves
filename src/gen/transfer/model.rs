//! Host-side physical sensor models.

// Host-only: module-local std link (crate root stays `#![no_std]`).
extern crate std;

use std::format;
use std::prelude::v1::*;

use serde::{Deserialize, Deserializer};

/// Denominator for the rational model-input scale: `u = count * scale / 1e6`.
///
/// `scale` is stored as an integer so `u16 * u32` stays below `2^48` and is
/// therefore exact in `f64` before this division.
pub(crate) const MODEL_INPUT_SCALE_DENOMINATOR: u64 = 1_000_000;

#[derive(Clone, Debug)]
pub enum ModelDef {
    NtcBetaDivider {
        nominal_resistance_ohms: f64,
        beta_kelvin: f64,
        nominal_temperature_celsius: f64,
        fixed_resistance_ohms: f64,
        adc_max_code: u16,
        topology: DividerTopology,
    },
    /// `y = c0 + c1*u + c2*u^2 + ...` with `u = count * scale / 1e6`.
    ///
    /// Standalone TOML supplies `scale`. Family TOML omits it (defaults to
    /// `None`); expansion fills it from the member so shared coefficients stay
    /// distinct from per-member scale.
    ScaledPolynomial {
        coefficients: Vec<f64>,
        scale: Option<u32>,
    },
}

// Preserve the documented permissive parser for existing NTC models while
// keeping the new scaled-polynomial vocabulary fail-closed.
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ModelDefWire {
    NtcBetaDivider {
        nominal_resistance_ohms: f64,
        beta_kelvin: f64,
        nominal_temperature_celsius: f64,
        fixed_resistance_ohms: f64,
        adc_max_code: u16,
        topology: DividerTopology,
    },
    ScaledPolynomial(ScaledPolynomialWire),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ScaledPolynomialWire {
    coefficients: Vec<f64>,
    #[serde(default)]
    scale: Option<u32>,
}

impl<'de> Deserialize<'de> for ModelDef {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(match ModelDefWire::deserialize(deserializer)? {
            ModelDefWire::NtcBetaDivider {
                nominal_resistance_ohms,
                beta_kelvin,
                nominal_temperature_celsius,
                fixed_resistance_ohms,
                adc_max_code,
                topology,
            } => Self::NtcBetaDivider {
                nominal_resistance_ohms,
                beta_kelvin,
                nominal_temperature_celsius,
                fixed_resistance_ohms,
                adc_max_code,
                topology,
            },
            ModelDefWire::ScaledPolynomial(ScaledPolynomialWire {
                coefficients,
                scale,
            }) => Self::ScaledPolynomial {
                coefficients,
                scale,
            },
        })
    }
}

#[derive(Copy, Clone, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DividerTopology {
    NtcToGround,
    NtcToSupply,
}

/// Model input `u` from an observation code using exact integer-product scale.
///
/// Computes `(count as u64 * scale as u64) as f64 / 1e6`. The product is
/// less than `2^48`, so it is exact in `f64` before the divide. Pre-rounding
/// `scale / 1e6` then multiplying by `count` is the off-by-one this exists
/// to prevent.
pub(crate) fn scaled_input(count: u16, scale: u32) -> f64 {
    let product = u64::from(count) * u64::from(scale);
    (product as f64) / MODEL_INPUT_SCALE_DENOMINATOR as f64
}

/// Inclusive observation-domain window covering `model_input` at `scale`.
///
/// Uses the same `u(count)` as evaluation: smallest code with `u >= min`,
/// largest with `u <= max`. `u16::MAX` is included when it falls inside the
/// window. Fewer than two codes is an error.
pub(crate) fn observation_domain(scale: u32, model_input: [f64; 2]) -> Result<[u16; 2], String> {
    if scale == 0 {
        return Err("scale must be positive".into());
    }
    let [min, max] = model_input;
    if !min.is_finite() || !max.is_finite() || min >= max {
        return Err(
            "applicability.model_input must be two strictly increasing finite values".into(),
        );
    }

    let mut first = None;
    let mut last = None;
    for code in 0..=u16::MAX {
        let u = scaled_input(code, scale);
        if u >= min && u <= max {
            if first.is_none() {
                first = Some(code);
            }
            last = Some(code);
        } else if first.is_some() && u > max {
            break;
        }
    }
    match (first, last) {
        (Some(lo), Some(hi)) if lo < hi => Ok([lo, hi]),
        _ => Err("applicability window contains fewer than two observation codes".into()),
    }
}

pub fn evaluate(
    name: &str,
    model: &ModelDef,
    output_range: [f64; 2],
) -> Result<(u16, Vec<f64>, String), String> {
    if !output_range[0].is_finite()
        || !output_range[1].is_finite()
        || output_range[0] >= output_range[1]
    {
        return Err(format!(
            "transfer `{name}`: output_range must contain two increasing finite values"
        ));
    }

    match model {
        ModelDef::NtcBetaDivider {
            nominal_resistance_ohms,
            beta_kelvin,
            nominal_temperature_celsius,
            fixed_resistance_ohms,
            adc_max_code,
            topology,
        } => {
            for (label, value) in [
                ("nominal_resistance_ohms", nominal_resistance_ohms),
                ("beta_kelvin", beta_kelvin),
                ("nominal_temperature_celsius", nominal_temperature_celsius),
                ("fixed_resistance_ohms", fixed_resistance_ohms),
            ] {
                if !value.is_finite() {
                    return Err(format!("transfer `{name}`: {label} must be finite"));
                }
            }
            if *nominal_resistance_ohms <= 0.0
                || *beta_kelvin <= 0.0
                || *fixed_resistance_ohms <= 0.0
            {
                return Err(format!(
                    "transfer `{name}`: resistances and beta must be positive"
                ));
            }
            if *nominal_temperature_celsius <= -273.15 {
                return Err(format!(
                    "transfer `{name}`: nominal temperature must exceed absolute zero"
                ));
            }
            if *adc_max_code < 2 {
                return Err(format!(
                    "transfer `{name}`: adc_max_code must be at least 2"
                ));
            }

            let mut accepted = Vec::new();
            for code in 1..*adc_max_code {
                let temperature = ntc_temperature(
                    code,
                    *adc_max_code,
                    *nominal_resistance_ohms,
                    *beta_kelvin,
                    *nominal_temperature_celsius,
                    *fixed_resistance_ohms,
                    *topology,
                );
                if temperature >= output_range[0] && temperature <= output_range[1] {
                    accepted.push((code, temperature));
                }
            }
            if accepted.len() < 2 {
                return Err(format!(
                    "transfer `{name}`: output_range contains fewer than two ADC codes"
                ));
            }

            let minimum = accepted[0].0;
            let maximum = accepted.last().expect("accepted count checked").0;
            if accepted.len() != usize::from(maximum - minimum) + 1 {
                return Err(format!(
                    "transfer `{name}`: derived model domain is not contiguous"
                ));
            }
            let values = accepted.into_iter().map(|(_, value)| value).collect();
            let description = format!(
                "NTC Beta divider: R0={nominal_resistance_ohms} ohm, B={beta_kelvin} K, \
                 T0={nominal_temperature_celsius} C, fixed={fixed_resistance_ohms} ohm, \
                 ADC max={adc_max_code}, topology={topology:?}"
            );
            Ok((minimum, values, description))
        }
        ModelDef::ScaledPolynomial { .. } => Err(format!(
            "transfer `{name}`: scaled_polynomial is not an output_range model; use domain"
        )),
    }
}

/// Evaluate `y = poly(u)` on an inclusive observation domain.
///
/// Coefficients are `[c0, c1, c2, ...]` for `y = c0 + c1*u + c2*u^2 + ...`,
/// evaluated with Horner from the high-degree end. `u16::MAX` is a legal
/// domain endpoint; saturation policy is a later schema.
pub fn evaluate_scaled_polynomial(
    name: &str,
    coefficients: &[f64],
    scale: u32,
    domain: [u16; 2],
) -> Result<(u16, Vec<f64>, String), String> {
    validate_coefficients(coefficients).map_err(|error| format!("transfer `{name}`: {error}"))?;
    if scale == 0 {
        return Err(format!(
            "transfer `{name}`: scaled_polynomial scale must be positive"
        ));
    }
    let [minimum, maximum] = domain;
    if minimum >= maximum {
        return Err(format!(
            "transfer `{name}`: domain must be strictly increasing"
        ));
    }

    let mut values = Vec::with_capacity(usize::from(maximum - minimum) + 1);
    for code in minimum..=maximum {
        let u = scaled_input(code, scale);
        let y = horner(coefficients, u);
        if !y.is_finite() {
            return Err(format!(
                "transfer `{name}`: scaled_polynomial produced a non-finite value at code {code}"
            ));
        }
        values.push(y);
    }

    Ok((
        minimum,
        values,
        format!(
            "scaled polynomial ({} coefficients, scale={scale})",
            coefficients.len()
        ),
    ))
}

pub(crate) fn validate_coefficients(coefficients: &[f64]) -> Result<(), String> {
    if coefficients.is_empty() {
        return Err("scaled_polynomial coefficients must not be empty".into());
    }
    for (index, value) in coefficients.iter().enumerate() {
        if !value.is_finite() {
            return Err(format!(
                "scaled_polynomial coefficient {index} must be finite"
            ));
        }
    }
    Ok(())
}

fn horner(coefficients: &[f64], u: f64) -> f64 {
    let mut acc = 0.0;
    for &coefficient in coefficients.iter().rev() {
        acc = acc * u + coefficient;
    }
    acc
}

fn ntc_temperature(
    code: u16,
    adc_max_code: u16,
    nominal_resistance: f64,
    beta: f64,
    nominal_temperature_celsius: f64,
    fixed_resistance: f64,
    topology: DividerTopology,
) -> f64 {
    let ratio = f64::from(code) / f64::from(adc_max_code);
    let resistance = match topology {
        DividerTopology::NtcToGround => fixed_resistance * ratio / (1.0 - ratio),
        DividerTopology::NtcToSupply => fixed_resistance * (1.0 - ratio) / ratio,
    };
    let nominal_kelvin = nominal_temperature_celsius + 273.15;
    let kelvin = 1.0 / (1.0 / nominal_kelvin + (resistance / nominal_resistance).ln() / beta);
    kelvin - 273.15
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ntc_model_keeps_ignoring_unknown_fields() {
        let model: ModelDef = toml::from_str(
            r#"
kind = "ntc_beta_divider"
nominal_resistance_ohms = 10000.0
beta_kelvin = 3950.0
nominal_temperature_celsius = 25.0
fixed_resistance_ohms = 10000.0
adc_max_code = 4095
topology = "ntc_to_ground"
future_calibration_field = "ignored for compatibility"
"#,
        )
        .unwrap();
        assert!(matches!(model, ModelDef::NtcBetaDivider { .. }));
    }

    #[test]
    fn scaled_polynomial_rejects_unknown_fields() {
        let error = toml::from_str::<ModelDef>(
            r#"
kind = "scaled_polynomial"
coefficients = [0.0, 1.0]
scale = 33600
scale_micro_lux_per_count = 33600
"#,
        )
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("unknown field `scale_micro_lux_per_count`"),
            "{error}"
        );
    }

    #[test]
    fn scaled_input_keeps_the_integer_product_exact() {
        assert_eq!(scaled_input(1875, 33_600), 63.0);
        assert!(scaled_input(1874, 33_600) < 63.0);
        assert_eq!(scaled_input(5581, 268_800), 1500.1728);
    }

    #[test]
    fn inclusive_lower_bound_includes_the_exact_endpoint_code() {
        let [lo, _] = observation_domain(33_600, [63.0, 100.0]).unwrap();
        assert_eq!(lo, 1875);
    }

    #[test]
    fn observation_domain_may_include_u16_max() {
        let [_, hi] = observation_domain(1_000, [0.0, 100.0]).unwrap();
        assert_eq!(hi, u16::MAX);
    }

    #[test]
    fn empty_observation_window_fails_closed() {
        let error = observation_domain(33_600, [0.0, 0.01]).unwrap_err();
        assert!(error.contains("fewer than two"), "{error}");
    }

    #[test]
    fn horner_matches_explicit_powers() {
        let coefficients = [1.0, 2.0, 3.0];
        let u = 4.0;
        let expected = 1.0 + 2.0 * u + 3.0 * u * u;
        assert_eq!(horner(&coefficients, u), expected);
    }
}
