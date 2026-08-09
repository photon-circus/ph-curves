//! Bounded adaptive knot selection and exhaustive verification.

use ph_curves::interpolate_segment;

pub struct AdaptiveResult {
    pub inputs: Vec<u16>,
    pub outputs: Vec<i32>,
    pub achieved_max_error: u32,
    pub achieved_max_error_exact: f64,
    pub worst_case_input: u16,
}

pub fn fit(
    name: &str,
    domain_min: u16,
    truth: &[f64],
    requested_max_error: u32,
    max_knots: usize,
) -> AdaptiveResult {
    let mut knot_offsets = vec![0usize, truth.len() - 1];

    loop {
        let outputs: Vec<i32> = knot_offsets
            .iter()
            .map(|&offset| truth[offset].round() as i32)
            .collect();
        let (worst_offset, worst_error) = measure_error(domain_min, truth, &knot_offsets, &outputs);

        if worst_error <= f64::from(requested_max_error) {
            return AdaptiveResult {
                inputs: knot_offsets
                    .iter()
                    .map(|&offset| domain_min + offset as u16)
                    .collect(),
                outputs,
                achieved_max_error: worst_error.ceil() as u32,
                achieved_max_error_exact: worst_error,
                worst_case_input: domain_min + worst_offset as u16,
            };
        }

        assert!(
            knot_offsets.binary_search(&worst_offset).is_err(),
            "transfer `{name}`: requested error cannot be met at the configured output_scale; worst error is {worst_error:.6} at input {}",
            domain_min + worst_offset as u16
        );
        assert!(
            knot_offsets.len() < max_knots,
            "transfer `{name}`: cannot meet maximum error {requested_max_error} within max_knots={max_knots}; best error is {worst_error:.6} at input {}",
            domain_min + worst_offset as u16
        );

        let insertion = knot_offsets
            .binary_search(&worst_offset)
            .unwrap_or_else(|index| index);
        knot_offsets.insert(insertion, worst_offset);
    }
}

fn measure_error(
    domain_min: u16,
    truth: &[f64],
    knot_offsets: &[usize],
    outputs: &[i32],
) -> (usize, f64) {
    let mut segment = 0usize;
    let mut worst_offset = 0usize;
    let mut worst_error = -1.0f64;

    for (offset, &expected) in truth.iter().enumerate() {
        while segment + 1 < knot_offsets.len() - 1 && offset > knot_offsets[segment + 1] {
            segment += 1;
        }
        let left_offset = knot_offsets[segment];
        let right_offset = knot_offsets[segment + 1];
        let input = domain_min + offset as u16;
        let actual = interpolate_segment(
            input,
            domain_min + left_offset as u16,
            outputs[segment],
            domain_min + right_offset as u16,
            outputs[segment + 1],
        )
        .unwrap_or_else(|error| panic!("internal interpolation error: {error:?}"));
        let error = (f64::from(actual) - expected).abs();
        if error > worst_error {
            worst_offset = offset;
            worst_error = error;
        }
    }

    (worst_offset, worst_error)
}
