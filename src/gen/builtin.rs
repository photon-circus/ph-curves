//! Built-in easing curves.
//!
//! Each builtin is a pure `f64 → f64` function that maps normalised time
//! `t` (0.0..=1.0) to a normalised output value (0.0..=1.0).

/// Evaluate a builtin curve at normalised position `t` (0.0..=1.0).
///
/// # Panics
///
/// Panics if `name` is not a recognised builtin.
pub fn eval(name: &str, t: f64) -> f64 {
    match name {
        "linear" => t,

        // Quadratic
        "ease_in" | "ease_in_quad" => t * t,
        "ease_out" | "ease_out_quad" => 1.0 - (1.0 - t).powi(2),
        "ease_in_out" | "ease_in_out_quad" => {
            if t < 0.5 {
                2.0 * t * t
            } else {
                1.0 - (-2.0 * t + 2.0).powi(2) / 2.0
            }
        }

        // Cubic
        "ease_in_cubic" => t * t * t,
        "ease_out_cubic" => 1.0 - (1.0 - t).powi(3),
        "ease_in_out_cubic" => {
            if t < 0.5 {
                4.0 * t * t * t
            } else {
                1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
            }
        }

        // Quartic
        "ease_in_quart" => t * t * t * t,
        "ease_out_quart" => 1.0 - (1.0 - t).powi(4),
        "ease_in_out_quart" => {
            if t < 0.5 {
                8.0 * t.powi(4)
            } else {
                1.0 - (-2.0 * t + 2.0).powi(4) / 2.0
            }
        }

        // Exponential
        "ease_in_expo" => {
            if t <= 0.0 {
                0.0
            } else {
                (2.0_f64).powf(10.0 * t - 10.0)
            }
        }
        "ease_out_expo" => {
            if t >= 1.0 {
                1.0
            } else {
                1.0 - (2.0_f64).powf(-10.0 * t)
            }
        }

        // Smoothstep family
        "smoothstep" => 3.0 * t * t - 2.0 * t * t * t,
        "smoother_step" => {
            // 6t^5 - 15t^4 + 10t^3 (Ken Perlin)
            t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
        }

        other => panic!("unknown builtin curve `{other}`"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_close(a: f64, b: f64) {
        assert!(
            (a - b).abs() < 1e-12,
            "expected {b}, got {a} (delta {})",
            (a - b).abs()
        );
    }

    // Endpoints: all builtins must map 0→0 and 1→1.
    #[test]
    fn all_builtins_endpoints() {
        let names = [
            "linear",
            "ease_in_quad",
            "ease_out_quad",
            "ease_in_out_quad",
            "ease_in_cubic",
            "ease_out_cubic",
            "ease_in_out_cubic",
            "ease_in_quart",
            "ease_out_quart",
            "ease_in_out_quart",
            "ease_in_expo",
            "ease_out_expo",
            "smoothstep",
            "smoother_step",
        ];
        for name in names {
            assert_close(eval(name, 0.0), 0.0);
            assert_close(eval(name, 1.0), 1.0);
        }
    }

    // Legacy aliases resolve to the same function.
    #[test]
    fn legacy_aliases() {
        for t in [0.0, 0.25, 0.5, 0.75, 1.0] {
            assert_close(eval("ease_in", t), eval("ease_in_quad", t));
            assert_close(eval("ease_out", t), eval("ease_out_quad", t));
            assert_close(eval("ease_in_out", t), eval("ease_in_out_quad", t));
        }
    }

    // Midpoint spot checks.
    #[test]
    fn linear_midpoint() {
        assert_close(eval("linear", 0.5), 0.5);
    }

    #[test]
    fn ease_in_quad_midpoint() {
        assert_close(eval("ease_in_quad", 0.5), 0.25);
    }

    #[test]
    fn ease_out_quad_midpoint() {
        assert_close(eval("ease_out_quad", 0.5), 0.75);
    }

    #[test]
    fn ease_in_out_quad_midpoint() {
        assert_close(eval("ease_in_out_quad", 0.5), 0.5);
    }

    #[test]
    fn ease_in_cubic_midpoint() {
        assert_close(eval("ease_in_cubic", 0.5), 0.125);
    }

    #[test]
    fn ease_out_cubic_midpoint() {
        assert_close(eval("ease_out_cubic", 0.5), 0.875);
    }

    #[test]
    fn ease_in_out_cubic_midpoint() {
        assert_close(eval("ease_in_out_cubic", 0.5), 0.5);
    }

    #[test]
    fn ease_in_quart_quarter() {
        // 0.25^4 = 0.00390625
        assert_close(eval("ease_in_quart", 0.25), 0.00390625);
    }

    #[test]
    fn ease_out_quart_quarter() {
        // 1 - (1 - 0.25)^4 = 1 - 0.75^4 = 1 - 0.31640625
        assert_close(eval("ease_out_quart", 0.25), 1.0 - 0.75_f64.powi(4));
    }

    #[test]
    fn ease_in_out_quart_below_half() {
        // t=0.25 < 0.5 → 8 * 0.25^4 = 8 * 0.00390625 = 0.03125
        assert_close(eval("ease_in_out_quart", 0.25), 0.03125);
    }

    #[test]
    fn ease_in_out_quart_above_half() {
        // t=0.75 → 1 - (-2*0.75 + 2)^4 / 2 = 1 - 0.5^4 / 2 = 1 - 0.03125
        assert_close(eval("ease_in_out_quart", 0.75), 1.0 - 0.5_f64.powi(4) / 2.0);
    }

    #[test]
    fn ease_in_expo_near_zero() {
        assert_close(eval("ease_in_expo", 0.0), 0.0);
    }

    #[test]
    fn ease_out_expo_near_one() {
        assert_close(eval("ease_out_expo", 1.0), 1.0);
    }

    #[test]
    fn smoothstep_midpoint() {
        assert_close(eval("smoothstep", 0.5), 0.5);
    }

    #[test]
    fn smoother_step_midpoint() {
        assert_close(eval("smoother_step", 0.5), 0.5);
    }

    // Monotonicity: all builtins should be non-decreasing.
    #[test]
    fn all_builtins_monotonic() {
        let names = [
            "linear",
            "ease_in_quad",
            "ease_out_quad",
            "ease_in_out_quad",
            "ease_in_cubic",
            "ease_out_cubic",
            "ease_in_out_cubic",
            "ease_in_quart",
            "ease_out_quart",
            "ease_in_out_quart",
            "ease_in_expo",
            "ease_out_expo",
            "smoothstep",
            "smoother_step",
        ];
        for name in names {
            let mut prev = eval(name, 0.0);
            for i in 1..=1000 {
                let t = i as f64 / 1000.0;
                let v = eval(name, t);
                assert!(
                    v >= prev - 1e-12,
                    "{name} not monotonic at t={t}: {v} < {prev}"
                );
                prev = v;
            }
        }
    }

    #[test]
    #[should_panic(expected = "unknown builtin curve")]
    fn unknown_builtin_panics() {
        eval("nonexistent", 0.5);
    }
}
