//! Host-side physical point interpolation.

// Host-only: module-local std link (crate root stays `#![no_std]`).
extern crate std;

use std::prelude::v1::*;
use std::format;

use super::PhysicalPoint;

pub fn evaluate(name: &str, points: &[PhysicalPoint]) -> Result<(u16, Vec<f64>), String> {
    if points.len() < 2 {
        return Err(format!(
            "transfer `{name}`: points must contain at least two entries"
        ));
    }
    for (index, point) in points.iter().enumerate() {
        if !point.output.is_finite() {
            return Err(format!(
                "transfer `{name}`: point {index} has a non-finite output"
            ));
        }
    }
    for pair in points.windows(2) {
        if pair[1].input <= pair[0].input {
            return Err(format!(
                "transfer `{name}`: point inputs must be strictly increasing"
            ));
        }
    }

    let minimum = points[0].input;
    let maximum = points.last().expect("point count checked").input;
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

    Ok((minimum, values))
}
