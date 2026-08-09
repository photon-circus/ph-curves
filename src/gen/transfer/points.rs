//! Host-side physical point interpolation.

use super::PhysicalPoint;

pub fn evaluate(name: &str, points: &[PhysicalPoint]) -> (u16, Vec<f64>) {
    assert!(
        points.len() >= 2,
        "transfer `{name}`: points must contain at least two entries"
    );
    for (index, point) in points.iter().enumerate() {
        assert!(
            point.output.is_finite(),
            "transfer `{name}`: point {index} has a non-finite output"
        );
    }
    for pair in points.windows(2) {
        assert!(
            pair[1].input > pair[0].input,
            "transfer `{name}`: point inputs must be strictly increasing"
        );
    }

    let minimum = points[0].input;
    let maximum = points.last().unwrap().input;
    let mut segment = 0usize;
    let mut values = Vec::with_capacity(usize::from(maximum - minimum) + 1);

    for input in minimum..=maximum {
        while segment + 1 < points.len() - 1 && input > points[segment + 1].input {
            segment += 1;
        }
        let left = &points[segment];
        let right = &points[segment + 1];
        let fraction = f64::from(input - left.input) / f64::from(right.input - left.input);
        values.push(left.output + fraction * (right.output - left.output));
    }

    (minimum, values)
}
