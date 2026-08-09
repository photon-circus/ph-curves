//! Piecewise-linear interpolation from control points.

/// Build a forward LUT by linearly interpolating between control points.
///
/// # Panics
///
/// - Fewer than 2 points.
/// - First point doesn't start at `u = 0`.
/// - Last point doesn't end at `u = lut_size - 1`.
/// - Input (`u`) values are not strictly increasing.
pub fn build(name: &str, points: &[[u16; 2]], lut_size: usize) -> Vec<u32> {
    assert!(
        points.len() >= 2,
        "points curve `{name}` must have at least 2 points"
    );

    let max = (lut_size - 1) as u16;
    assert!(points[0][0] == 0, "points curve `{name}` must start at u=0");
    assert!(
        points.last().unwrap()[0] == max,
        "points curve `{name}` must end at u={max}"
    );

    for window in points.windows(2) {
        assert!(
            window[1][0] > window[0][0],
            "points curve `{name}` must have strictly increasing u values"
        );
    }

    let mut fwd = vec![0u32; lut_size];
    for window in points.windows(2) {
        let (u0, w0) = (window[0][0], window[0][1]);
        let (u1, w1) = (window[1][0], window[1][1]);
        for u in u0..=u1 {
            fwd[u as usize] = interpolate(u, u0, w0, u1, w1);
        }
    }
    fwd
}

fn interpolate(u: u16, u0: u16, w0: u16, u1: u16, w1: u16) -> u32 {
    let t = u64::from(u.saturating_sub(u0));
    let span = u64::from(u1 - u0);
    if span == 0 {
        return w0 as u32;
    }
    let distance = u64::from(w0.abs_diff(w1));
    let offset = (distance * t + span / 2) / span;
    (if w1 >= w0 {
        u64::from(w0) + offset
    } else {
        u64::from(w0) - offset
    }) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_identity() {
        let pts = [[0, 0], [255, 255]];
        let fwd = build("test", &pts, 256);
        for (i, &v) in fwd.iter().enumerate() {
            assert_eq!(v, i as u32);
        }
    }

    #[test]
    fn three_point_curve() {
        let pts = [[0, 0], [128, 255], [255, 128]];
        let fwd = build("test", &pts, 256);
        assert_eq!(fwd[0], 0);
        // Midpoint at u=128: interpolate(128, 0, 0, 128, 255)
        // t=128, span=128, delta=255, numer=255*128+64=32704, w=0+32704/128=255
        assert_eq!(fwd[128], 255);
        assert_eq!(fwd[255], 128);
    }

    #[test]
    fn endpoints_are_exact() {
        let pts = [[0, 0], [9, 9]];
        let fwd = build("test", &pts, 10);
        assert_eq!(fwd[0], 0);
        assert_eq!(fwd[9], 9);
    }

    #[test]
    fn interpolation_midpoint() {
        // Two-point linear from 0→100 over 10 entries.
        let pts = [[0, 0], [9, 100]];
        let fwd = build("test", &pts, 10);
        // Midpoint at u=4: 4/9 * 100 ≈ 44 (with rounding to nearest)
        // interpolate uses (delta * t + span/2) / span, so:
        // (100 * 4 + 4) / 9 = 404/9 = 44
        assert_eq!(fwd[4], 44);
    }

    #[test]
    fn decreasing_segment() {
        // Curve goes from 200 down to 0.
        let pts = [[0, 200], [255, 0]];
        let fwd = build("test", &pts, 256);
        assert_eq!(fwd[0], 200);
        assert_eq!(fwd[255], 0);
        // Midpoint should be around 100 (±1 for rounding).
        assert!((fwd[128] as i32 - 100).abs() <= 1);
    }

    #[test]
    fn interpolates_increasing_across_full_u16_range() {
        let pts = [[0, 0], [65535, 65535]];
        let fwd = build("test", &pts, 65536);
        assert_eq!(fwd[0], 0);
        assert_eq!(fwd[32768], 32768);
        assert_eq!(fwd[65535], 65535);
    }

    #[test]
    fn interpolates_decreasing_across_full_u16_range() {
        let pts = [[0, 65535], [65535, 0]];
        let fwd = build("test", &pts, 65536);
        assert_eq!(fwd[0], 65535);
        assert_eq!(fwd[32768], 32767);
        assert_eq!(fwd[65535], 0);
    }

    #[test]
    #[should_panic(expected = "at least 2 points")]
    fn too_few_points() {
        build("test", &[[0, 0]], 256);
    }

    #[test]
    #[should_panic(expected = "start at u=0")]
    fn bad_start() {
        build("test", &[[1, 0], [255, 255]], 256);
    }

    #[test]
    #[should_panic(expected = "end at u=")]
    fn bad_end() {
        build("test", &[[0, 0], [200, 255]], 256);
    }

    #[test]
    #[should_panic(expected = "strictly increasing")]
    fn non_increasing_u() {
        build("test", &[[0, 0], [100, 50], [100, 100], [255, 255]], 256);
    }
}
