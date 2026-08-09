//! Host-side physical sensor models.

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ModelDef {
    NtcBetaDivider {
        nominal_resistance_ohms: f64,
        beta_kelvin: f64,
        nominal_temperature_celsius: f64,
        fixed_resistance_ohms: f64,
        adc_max_code: u16,
        topology: DividerTopology,
    },
}

#[derive(Copy, Clone, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DividerTopology {
    NtcToGround,
    NtcToSupply,
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
    }
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
