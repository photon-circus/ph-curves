//! Tests for curve types and tickless scheduling.

extern crate alloc;
extern crate std;

use alloc::vec::Vec;

use crate::{
    Curve, CurveLut, CurveLut256, MonotonicCurveLut256, RepeatMode, Rounding, Tickless,
    TicklessDeadline, UnitValue,
};

// ---------------------------------------------------------------------------
// Inline test curves (u8)
// ---------------------------------------------------------------------------

const fn identity_lut() -> [u8; 256] {
    let mut arr = [0u8; 256];
    let mut i = 0;
    while i < 256 {
        arr[i] = i as u8;
        i += 1;
    }
    arr
}

static LINEAR_LUT: [u8; 256] = identity_lut();

const fn linear_curve() -> MonotonicCurveLut256 {
    MonotonicCurveLut256::new(&LINEAR_LUT, &LINEAR_LUT)
}

const fn ease_in_lut() -> [u8; 256] {
    let mut arr = [0u8; 256];
    let mut i = 0;
    while i < 256 {
        let u = i as u16;
        let w = (u * u + 127) / 255;
        arr[i] = w as u8;
        i += 1;
    }
    arr
}

static EASE_IN_FWD: [u8; 256] = ease_in_lut();

const fn build_inverse_lut(fwd: &[u8; 256]) -> [u8; 256] {
    let mut inv = [0u8; 256];
    let mut u: usize = 0;
    let mut w: usize = 0;
    while w < 256 {
        while u < 256 && (fwd[u] as usize) < w {
            u += 1;
        }
        inv[w] = if u >= 256 { 255 } else { u as u8 };
        w += 1;
    }
    inv
}

static EASE_IN_INV: [u8; 256] = build_inverse_lut(&EASE_IN_FWD);

const fn ease_in_curve() -> MonotonicCurveLut256 {
    MonotonicCurveLut256::new(&EASE_IN_FWD, &EASE_IN_INV)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn lut_endpoints() {
    let linear = linear_curve();
    assert_eq!(linear.fwd_lut()[0], 0);
    assert_eq!(linear.fwd_lut()[255], 255);

    let ease_in = ease_in_curve();
    assert_eq!(ease_in.fwd_lut()[0], 0);
    assert_eq!(ease_in.fwd_lut()[255], 255);
}

#[test]
fn lut_monotonicity() {
    for fwd in [linear_curve().fwd_lut(), ease_in_curve().fwd_lut()] {
        for i in 1..256 {
            assert!(fwd[i] >= fwd[i - 1], "fwd[{i}] < fwd[{}]", i - 1);
        }
    }
}

#[test]
fn inverse_round_trip() {
    let curve = ease_in_curve();
    let fwd = curve.fwd_lut();
    let inv = curve.inv_lut();
    for w in 0u8..=255 {
        let u = inv[w as usize] as usize;
        assert!(fwd[u] >= w, "inv round-trip at w={w}");
        if u > 0 {
            assert!(fwd[u - 1] < w, "inv minimality at w={w}");
        }
    }
}

#[test]
fn curve_eval_trait() {
    let curve = linear_curve();
    assert_eq!(curve.eval(0u8), 0u8);
    assert_eq!(curve.eval(128u8), 128u8);
    assert_eq!(curve.eval(255u8), 255u8);
}

#[test]
fn curve_lut_with_optional_inv() {
    let curve = CurveLut256::new(&LINEAR_LUT, Some(&LINEAR_LUT));
    assert_eq!(curve.eval(42u8), 42u8);
    assert!(curve.inv_lut().is_some());

    let mono = curve.monotonic().expect("should be monotonic");
    assert_eq!(mono.eval(42u8), 42u8);
}

#[test]
fn tickless_deadlines_are_monotonic() {
    let curve = linear_curve();
    let schedule = curve.tickless_schedule(0, 1000, 0, 255, 10, Rounding::Nearest, 0);
    let mut deadlines: Vec<TicklessDeadline> = Vec::new();
    for now in (0u32..=1000).step_by(100) {
        let dl = schedule.next_deadline(now);
        assert!(dl.deadline_ms >= now);
        deadlines.push(dl);
    }
    for window in deadlines.windows(2) {
        assert!(window[1].deadline_ms >= window[0].deadline_ms);
    }
}

#[test]
fn tickless_deadline_hits_segment_end() {
    let curve = linear_curve();
    let schedule = curve.tickless_schedule(0, 500, 0, 255, 5, Rounding::Floor, 0);
    let dl = schedule.next_deadline(600);
    assert!(dl.deadline_ms >= 600);
}

#[test]
fn tickless_deadline_respects_min_dt() {
    let curve = linear_curve();
    let schedule = curve.tickless_schedule(0, 1000, 0, 255, 1, Rounding::Nearest, 25);
    let dl = schedule.next_deadline(10);
    assert!(dl.deadline_ms >= 35);
}

#[test]
fn tickless_handles_decreasing_ramp() {
    let curve = linear_curve();
    let schedule = curve.tickless_schedule(0, 1000, 255, 0, 10, Rounding::Nearest, 0);
    let dl = schedule.next_deadline(0);
    assert!(dl.deadline_ms > 0);
}

#[test]
fn tickless_iter_covers_segment() {
    let curve = linear_curve();
    let schedule = curve.tickless_schedule(0, 1000, 0, 255, 10, Rounding::Nearest, 0);
    let deadlines: Vec<TicklessDeadline> = schedule.iter(0).collect();

    assert!(!deadlines.is_empty());
    for window in deadlines.windows(2) {
        assert!(window[1].deadline_ms >= window[0].deadline_ms);
    }
    assert!(deadlines.last().unwrap().deadline_ms >= 1000);
}

#[test]
fn tickless_repeat_loops() {
    let curve = linear_curve();
    let schedule = curve
        .tickless_schedule(0, 100, 0, 255, 50, Rounding::Nearest, 0)
        .with_repeat(RepeatMode::Repeat);

    let deadlines: Vec<TicklessDeadline> = schedule.iter(0).take(100).collect();
    assert_eq!(deadlines.len(), 100);
    for window in deadlines.windows(2) {
        assert!(window[1].deadline_ms >= window[0].deadline_ms);
    }
    assert!(deadlines.last().unwrap().deadline_ms > 100);
}

#[test]
fn tickless_pingpong() {
    let curve = linear_curve();
    let schedule = curve
        .tickless_schedule(0, 100, 0, 200, 50, Rounding::Nearest, 0)
        .with_repeat(RepeatMode::PingPong);

    let deadlines: Vec<TicklessDeadline> = schedule.iter(0).take(100).collect();
    assert_eq!(deadlines.len(), 100);
    for window in deadlines.windows(2) {
        assert!(window[1].deadline_ms >= window[0].deadline_ms);
    }
    let has_low = deadlines.iter().any(|d| d.current_val <= 50);
    let has_high = deadlines.iter().any(|d| d.current_val >= 150);
    assert!(has_low, "ping-pong should visit low values");
    assert!(has_high, "ping-pong should visit high values");
}

// ---------------------------------------------------------------------------
// Inline test curves (u16 values with u8 index)
// ---------------------------------------------------------------------------

const fn identity_lut_u16() -> [u16; 256] {
    let mut arr = [0u16; 256];
    let mut i = 0;
    while i < 256 {
        // Map u8 index 0..255 → u16 value 0..65535
        arr[i] = (i as u32 * 65535 / 255) as u16;
        i += 1;
    }
    arr
}

static LINEAR_U16_FWD: [u16; 256] = identity_lut_u16();

const fn ease_in_lut_u16() -> [u16; 256] {
    let mut arr = [0u16; 256];
    let mut i = 0;
    while i < 256 {
        let u = i as u32;
        // quadratic ease-in scaled to u16 range
        let w = u * u * 65535 / (255 * 255);
        arr[i] = w as u16;
        i += 1;
    }
    arr
}

static EASE_IN_U16_FWD: [u16; 256] = ease_in_lut_u16();

// ---------------------------------------------------------------------------
// u16 tests
// ---------------------------------------------------------------------------

#[test]
fn u16_lut_endpoints() {
    let linear = CurveLut::<u8, u16, 256>::new(&LINEAR_U16_FWD, None);
    assert_eq!(linear.eval(0u8), 0u16);
    assert_eq!(linear.eval(255u8), 65535u16);

    let ease_in = CurveLut::<u8, u16, 256>::new(&EASE_IN_U16_FWD, None);
    assert_eq!(ease_in.eval(0u8), 0u16);
    assert_eq!(ease_in.eval(255u8), 65535u16);
}

#[test]
fn u16_lut_monotonicity() {
    for fwd in [&LINEAR_U16_FWD, &EASE_IN_U16_FWD] {
        for i in 1..256 {
            assert!(fwd[i] >= fwd[i - 1], "fwd[{i}] < fwd[{}]", i - 1);
        }
    }
}

#[test]
fn u16_curve_eval_trait() {
    let curve = CurveLut::<u8, u16, 256>::new(&LINEAR_U16_FWD, None);
    assert_eq!(curve.eval(0u8), 0u16);
    let mid = curve.eval(128u8);
    let expected = (128u32 * 65535 / 255) as u16;
    assert_eq!(mid, expected);
    assert_eq!(curve.eval(255u8), 65535u16);
}

#[test]
fn u16_value_range() {
    // Verify u16 values span the full range.
    let fwd = &LINEAR_U16_FWD;
    assert!(fwd[1] > 0, "first non-zero value");
    assert!(fwd[254] < 65535, "last non-max value");

    // Check approximate linearity.
    let mid = fwd[128] as i32;
    let expected = 128 * 65535 / 255;
    assert!(
        (mid - expected).abs() <= 1,
        "midpoint should be ~{expected}, got {mid}"
    );
}

// ===========================================================================
// math.rs coverage — UnitValue, lerp, map, quantize, next_target_value
// ===========================================================================

// ---------------------------------------------------------------------------
// UnitValue for u8 — zero / one / to_index
// ---------------------------------------------------------------------------

#[test]
fn u8_unit_value_zero_one() {
    assert_eq!(u8::zero(), 0);
    assert_eq!(u8::one(), 255);
}

#[test]
fn u8_to_index() {
    assert_eq!(0u8.to_index(), 0);
    assert_eq!(128u8.to_index(), 128);
    assert_eq!(255u8.to_index(), 255);
}

// ---------------------------------------------------------------------------
// UnitValue for u8 — from_time_frac
// ---------------------------------------------------------------------------

#[test]
fn u8_from_time_frac_zero_elapsed() {
    assert_eq!(u8::from_time_frac(0, 1000), 0);
}

#[test]
fn u8_from_time_frac_full_elapsed() {
    assert_eq!(u8::from_time_frac(1000, 1000), 255);
}

#[test]
fn u8_from_time_frac_over_elapsed() {
    assert_eq!(u8::from_time_frac(2000, 1000), 255);
}

#[test]
fn u8_from_time_frac_zero_duration() {
    assert_eq!(u8::from_time_frac(500, 0), 255);
}

#[test]
fn u8_from_time_frac_half() {
    let val = u8::from_time_frac(500, 1000);
    assert!((126..=128).contains(&val), "half should be ~127, got {val}");
}

#[test]
fn u8_from_time_frac_quarter() {
    let val = u8::from_time_frac(250, 1000);
    assert!((62..=64).contains(&val), "quarter should be ~63, got {val}");
}

#[test]
fn u8_from_time_frac_one_ms() {
    // Very small fraction should be > 0 for non-zero elapsed.
    let val = u8::from_time_frac(1, 1000);
    // 1/1000 * 255 ≈ 0.255, truncates to 0 in fixed-point.
    assert!(val <= 1, "tiny fraction should be 0 or 1, got {val}");
}

// ---------------------------------------------------------------------------
// UnitValue for u8 — to_time_offset
// ---------------------------------------------------------------------------

#[test]
fn u8_to_time_offset_zero() {
    assert_eq!(0u8.to_time_offset(1000), 0);
}

#[test]
fn u8_to_time_offset_full() {
    assert_eq!(255u8.to_time_offset(1000), 1000);
}

#[test]
fn u8_to_time_offset_zero_duration() {
    assert_eq!(128u8.to_time_offset(0), 0);
}

#[test]
fn u8_to_time_offset_mid() {
    let ms = 128u8.to_time_offset(1000);
    // 128/255 * 1000 ≈ 502, ceil rounds up.
    assert!(
        (501..=503).contains(&ms),
        "mid offset should be ~502, got {ms}"
    );
}

#[test]
fn u8_from_time_frac_to_time_offset_roundtrip() {
    // from_time_frac then to_time_offset should approximate the original.
    for elapsed in [0, 100, 250, 500, 750, 999, 1000] {
        let val = u8::from_time_frac(elapsed, 1000);
        let back = val.to_time_offset(1000);
        // Allow ±5ms tolerance due to quantization.
        let diff = (back as i32 - elapsed as i32).unsigned_abs();
        assert!(
            diff <= 5,
            "roundtrip for {elapsed}ms: got {back}ms (diff={diff})"
        );
    }
}

// ---------------------------------------------------------------------------
// UnitValue for u8 — lerp_u16 / inv_lerp_u16
// ---------------------------------------------------------------------------

#[test]
fn u8_lerp_u16_endpoints() {
    assert_eq!(0u8.lerp_u16(100, 200), 100);
    assert_eq!(255u8.lerp_u16(100, 200), 200);
}

#[test]
fn u8_lerp_u16_midpoint() {
    let mid = 128u8.lerp_u16(0, 1000);
    // 128/255 * 1000 ≈ 502
    assert!(
        (501..=503).contains(&mid),
        "midpoint lerp should be ~502, got {mid}"
    );
}

#[test]
fn u8_lerp_u16_same_values() {
    assert_eq!(128u8.lerp_u16(500, 500), 500);
}

#[test]
fn u8_lerp_u16_decreasing() {
    let val = 128u8.lerp_u16(1000, 0);
    // 128/255 * (0-1000) + 1000 ≈ 498
    assert!(
        (497..=499).contains(&val),
        "decreasing lerp should be ~498, got {val}"
    );
}

#[test]
fn u8_lerp_u16_full_u16_range() {
    assert_eq!(0u8.lerp_u16(0, 65535), 0);
    assert_eq!(255u8.lerp_u16(0, 65535), 65535);
}

#[test]
fn u8_inv_lerp_u16_endpoints() {
    // Target == a → should be 0 (or very small).
    let w = u8::inv_lerp_u16(100, 200, 100);
    assert!(w <= 1, "inv_lerp at a should be ~0, got {w}");

    // Target == b → should be 255.
    assert_eq!(u8::inv_lerp_u16(100, 200, 200), 255);
}

#[test]
fn u8_inv_lerp_u16_equal_endpoints() {
    assert_eq!(u8::inv_lerp_u16(500, 500, 500), 255);
}

#[test]
fn u8_inv_lerp_u16_midpoint() {
    let w = u8::inv_lerp_u16(0, 1000, 500);
    // 500/1000 * 255 = 127.5, ceil → 128
    assert!(
        (127..=129).contains(&w),
        "inv_lerp mid should be ~128, got {w}"
    );
}

// ---------------------------------------------------------------------------
// UnitValue for u16 — zero / one / to_index
// ---------------------------------------------------------------------------

#[test]
fn u16_unit_value_zero_one() {
    assert_eq!(u16::zero(), 0);
    assert_eq!(u16::one(), 65535);
}

#[test]
fn u16_to_index() {
    assert_eq!(0u16.to_index(), 0);
    assert_eq!(32768u16.to_index(), 32768);
    assert_eq!(65535u16.to_index(), 65535);
}

// ---------------------------------------------------------------------------
// UnitValue for u16 — from_time_frac
// ---------------------------------------------------------------------------

#[test]
fn u16_from_time_frac_zero_elapsed() {
    assert_eq!(u16::from_time_frac(0, 1000), 0);
}

#[test]
fn u16_from_time_frac_full_elapsed() {
    assert_eq!(u16::from_time_frac(1000, 1000), 65535);
}

#[test]
fn u16_from_time_frac_over_elapsed() {
    assert_eq!(u16::from_time_frac(5000, 1000), 65535);
}

#[test]
fn u16_from_time_frac_zero_duration() {
    assert_eq!(u16::from_time_frac(500, 0), 65535);
}

#[test]
fn u16_from_time_frac_half() {
    let val = u16::from_time_frac(500, 1000);
    // 0.5 * 65535 = 32767.5, truncates to 32767
    assert!(
        (32766..=32768).contains(&val),
        "half should be ~32767, got {val}"
    );
}

// ---------------------------------------------------------------------------
// UnitValue for u16 — to_time_offset
// ---------------------------------------------------------------------------

#[test]
fn u16_to_time_offset_zero() {
    assert_eq!(0u16.to_time_offset(1000), 0);
}

#[test]
fn u16_to_time_offset_full() {
    assert_eq!(65535u16.to_time_offset(1000), 1000);
}

#[test]
fn u16_to_time_offset_zero_duration() {
    assert_eq!(32768u16.to_time_offset(0), 0);
}

#[test]
fn u16_from_time_frac_to_time_offset_roundtrip() {
    for elapsed in [0, 100, 250, 500, 750, 999, 1000] {
        let val = u16::from_time_frac(elapsed, 1000);
        let back = val.to_time_offset(1000);
        let diff = (back as i32 - elapsed as i32).unsigned_abs();
        assert!(
            diff <= 1,
            "u16 roundtrip for {elapsed}ms: got {back}ms (diff={diff})"
        );
    }
}

// ---------------------------------------------------------------------------
// UnitValue for u16 — lerp_u16 / inv_lerp_u16
// ---------------------------------------------------------------------------

#[test]
fn u16_lerp_u16_endpoints() {
    assert_eq!(0u16.lerp_u16(100, 200), 100);
    assert_eq!(65535u16.lerp_u16(100, 200), 200);
}

#[test]
fn u16_lerp_u16_midpoint() {
    let mid = 32768u16.lerp_u16(0, 1000);
    assert!(
        (499..=501).contains(&mid),
        "u16 midpoint lerp should be ~500, got {mid}"
    );
}

#[test]
fn u16_inv_lerp_u16_endpoints() {
    let w = u16::inv_lerp_u16(100, 200, 100);
    assert!(w <= 1, "inv_lerp at a should be ~0, got {w}");
    assert_eq!(u16::inv_lerp_u16(100, 200, 200), 65535);
}

#[test]
fn u16_inv_lerp_u16_equal_endpoints() {
    assert_eq!(u16::inv_lerp_u16(500, 500, 500), 65535);
}

// ---------------------------------------------------------------------------
// Free-function lerp_u8
// ---------------------------------------------------------------------------

#[test]
fn lerp_u8_endpoints() {
    assert_eq!(crate::lerp_u8(10, 200, 0), 10);
    assert_eq!(crate::lerp_u8(10, 200, 255), 200);
}

#[test]
fn lerp_u8_midpoint() {
    let val = crate::lerp_u8(0, 100, 128);
    // 128/255 * 100 ≈ 50.2
    assert!(
        (49..=51).contains(&val),
        "lerp_u8 mid should be ~50, got {val}"
    );
}

#[test]
fn lerp_u8_same_endpoints() {
    assert_eq!(crate::lerp_u8(42, 42, 128), 42);
}

#[test]
fn lerp_u8_decreasing() {
    let val = crate::lerp_u8(200, 100, 128);
    assert!(
        (149..=151).contains(&val),
        "lerp_u8 decreasing mid should be ~150, got {val}"
    );
}

#[test]
fn lerp_u8_zero_range() {
    assert_eq!(crate::lerp_u8(0, 0, 128), 0);
}

#[test]
fn lerp_u8_full_range() {
    assert_eq!(crate::lerp_u8(0, 255, 0), 0);
    assert_eq!(crate::lerp_u8(0, 255, 255), 255);
    let mid = crate::lerp_u8(0, 255, 128);
    assert!(
        (127..=129).contains(&mid),
        "full-range mid should be ~128, got {mid}"
    );
}

// ---------------------------------------------------------------------------
// Free-function lerp_u16 (u8 weight)
// ---------------------------------------------------------------------------

#[test]
fn lerp_u16_endpoints() {
    assert_eq!(crate::lerp_u16(1000, 5000, 0), 1000);
    assert_eq!(crate::lerp_u16(1000, 5000, 255), 5000);
}

#[test]
fn lerp_u16_midpoint() {
    let val = crate::lerp_u16(0, 10000, 128);
    // 128/255 * 10000 ≈ 5020
    assert!(
        (5010..=5030).contains(&val),
        "lerp_u16 mid should be ~5020, got {val}"
    );
}

#[test]
fn lerp_u16_full_u16_range() {
    assert_eq!(crate::lerp_u16(0, 65535, 0), 0);
    assert_eq!(crate::lerp_u16(0, 65535, 255), 65535);
}

#[test]
fn lerp_u16_same_endpoints() {
    assert_eq!(crate::lerp_u16(12345, 12345, 128), 12345);
}

// ---------------------------------------------------------------------------
// map_u8_to_u16
// ---------------------------------------------------------------------------

#[test]
fn map_u8_to_u16_endpoints() {
    assert_eq!(crate::map_u8_to_u16(0, 65535), 0);
    assert_eq!(crate::map_u8_to_u16(255, 65535), 65535);
}

#[test]
fn map_u8_to_u16_half() {
    let val = crate::map_u8_to_u16(128, 65535);
    // 128/255 * 65535 ≈ 32896
    assert!(
        (32890..=32900).contains(&val),
        "map half should be ~32896, got {val}"
    );
}

#[test]
fn map_u8_to_u16_small_max() {
    assert_eq!(crate::map_u8_to_u16(0, 100), 0);
    assert_eq!(crate::map_u8_to_u16(255, 100), 100);
    let mid = crate::map_u8_to_u16(128, 100);
    assert!(
        (49..=51).contains(&mid),
        "map to 100 at mid should be ~50, got {mid}"
    );
}

#[test]
fn map_u8_to_u16_zero_max() {
    assert_eq!(crate::map_u8_to_u16(128, 0), 0);
}

// ---------------------------------------------------------------------------
// quantize
// ---------------------------------------------------------------------------

#[test]
fn quantize_floor() {
    assert_eq!(crate::quantize(0, 10, Rounding::Floor), 0);
    assert_eq!(crate::quantize(5, 10, Rounding::Floor), 0);
    assert_eq!(crate::quantize(9, 10, Rounding::Floor), 0);
    assert_eq!(crate::quantize(10, 10, Rounding::Floor), 10);
    assert_eq!(crate::quantize(15, 10, Rounding::Floor), 10);
    assert_eq!(crate::quantize(255, 10, Rounding::Floor), 250);
}

#[test]
fn quantize_ceil() {
    assert_eq!(crate::quantize(0, 10, Rounding::Ceil), 0);
    assert_eq!(crate::quantize(1, 10, Rounding::Ceil), 10);
    assert_eq!(crate::quantize(5, 10, Rounding::Ceil), 10);
    assert_eq!(crate::quantize(10, 10, Rounding::Ceil), 10);
    assert_eq!(crate::quantize(11, 10, Rounding::Ceil), 20);
}

#[test]
fn quantize_nearest() {
    assert_eq!(crate::quantize(0, 10, Rounding::Nearest), 0);
    assert_eq!(crate::quantize(4, 10, Rounding::Nearest), 0);
    assert_eq!(crate::quantize(5, 10, Rounding::Nearest), 10);
    assert_eq!(crate::quantize(14, 10, Rounding::Nearest), 10);
    assert_eq!(crate::quantize(15, 10, Rounding::Nearest), 20);
}

#[test]
fn quantize_step_one() {
    // Step of 1 should be a no-op for all rounding modes.
    for v in [0, 1, 100, 255, 1000] {
        assert_eq!(crate::quantize(v, 1, Rounding::Floor), v);
        assert_eq!(crate::quantize(v, 1, Rounding::Ceil), v);
        assert_eq!(crate::quantize(v, 1, Rounding::Nearest), v);
    }
}

#[test]
fn quantize_exact_multiple() {
    // Already on a step boundary — all modes agree.
    assert_eq!(crate::quantize(100, 25, Rounding::Floor), 100);
    assert_eq!(crate::quantize(100, 25, Rounding::Ceil), 100);
    assert_eq!(crate::quantize(100, 25, Rounding::Nearest), 100);
}

// ---------------------------------------------------------------------------
// next_target_value
// ---------------------------------------------------------------------------

#[test]
fn next_target_increasing_basic() {
    assert_eq!(crate::next_target_value(100, 200, 10, true), 110);
}

#[test]
fn next_target_increasing_clamps_at_end() {
    assert_eq!(crate::next_target_value(195, 200, 10, true), 200);
}

#[test]
fn next_target_increasing_already_at_end() {
    assert_eq!(crate::next_target_value(200, 200, 10, true), 200);
}

#[test]
fn next_target_decreasing_basic() {
    assert_eq!(crate::next_target_value(100, 50, 10, false), 90);
}

#[test]
fn next_target_decreasing_clamps_at_end() {
    assert_eq!(crate::next_target_value(55, 50, 10, false), 50);
}

#[test]
fn next_target_decreasing_already_at_end() {
    assert_eq!(crate::next_target_value(50, 50, 10, false), 50);
}

#[test]
fn next_target_increasing_saturates() {
    // Should not overflow u16::MAX.
    assert_eq!(crate::next_target_value(65530, 65535, 10, true), 65535);
}

#[test]
fn next_target_decreasing_saturates() {
    // Should not underflow below 0.
    assert_eq!(crate::next_target_value(5, 0, 10, false), 0);
}

// ---------------------------------------------------------------------------
// CurveLut accessors & monotonic() branch coverage
// ---------------------------------------------------------------------------

#[test]
fn curve_lut_fwd_lut_accessor() {
    let curve = CurveLut256::new(&LINEAR_LUT, Some(&LINEAR_LUT));
    let fwd = curve.fwd_lut();
    assert_eq!(fwd[0], 0);
    assert_eq!(fwd[128], 128);
    assert_eq!(fwd[255], 255);
}

#[test]
fn curve_lut_monotonic_returns_none() {
    let curve = CurveLut256::new(&LINEAR_LUT, None);
    assert!(curve.inv_lut().is_none());
    assert!(curve.monotonic().is_none());
}

// ---------------------------------------------------------------------------
// Tickless edge-case guards
// ---------------------------------------------------------------------------

/// Construct a curve whose inverse maps to t=1 for high values,
/// producing a deadline at or beyond end_ms so the `deadline_ms > end_ms`
/// guard fires.
#[test]
fn tickless_deadline_clamped_to_end_ms() {
    // A schedule where the step is very small (step=1), so next_target is
    // always just +1 above current. For a short duration, the t→time
    // conversion can overshoot the end_ms.
    let curve = linear_curve();
    let schedule = curve.tickless_schedule(0, 10, 0, 255, 1, Rounding::Floor, 0);
    // At t=0 the next target is 1, which maps to inv_lerp_u16(0,255,1) ≈
    // first non-zero index, then t_to_time rounds up — could equal end_ms.
    // Verify the deadline never exceeds end_ms.
    let dl = schedule.next_deadline(0);
    assert!(dl.deadline_ms <= 10);
}

/// Use a very large min_dt that would push deadline well past now_ms,
/// then verify deadline is still clamped to end_ms when it overshoots.
#[test]
fn tickless_deadline_min_dt_pushes_past_end() {
    let curve = linear_curve();
    // duration=100ms, min_dt=200 → the adjusted deadline would be
    // now_ms + 200 = 250 > end_ms (100), so the guard clamps it.
    let schedule = curve.tickless_schedule(0, 100, 0, 255, 10, Rounding::Nearest, 200);
    let dl = schedule.next_deadline(50);
    assert!(
        dl.deadline_ms <= 100,
        "deadline {} should be <= end_ms 100",
        dl.deadline_ms
    );
}

#[test]
fn quantize_nearest_no_u16_overflow() {
    // step=2000, value=65535: (65535+1000)/2000*2000 = 66000 would overflow u16.
    // Must produce 65535 (capped), not 999 (wrapped).
    assert_eq!(crate::quantize(65535, 2000, Rounding::Nearest), 65535);
    assert_eq!(crate::quantize(65000, 2000, Rounding::Nearest), 65535);
    // Just below the overflow threshold — should round down normally.
    assert_eq!(crate::quantize(64001, 2000, Rounding::Nearest), 64000);
    assert_eq!(crate::quantize(64000, 2000, Rounding::Nearest), 64000);
}

#[test]
fn quantize_ceil_no_u16_overflow() {
    // div_ceil(65535, 2000) * 2000 = 33 * 2000 = 66000 would overflow u16.
    assert_eq!(crate::quantize(65535, 2000, Rounding::Ceil), 65535);
    assert_eq!(crate::quantize(64001, 2000, Rounding::Ceil), 65535);
    // Exact multiple — no rounding needed.
    assert_eq!(crate::quantize(64000, 2000, Rounding::Ceil), 64000);
}

#[test]
fn quantize_floor_large_step_at_max() {
    // Floor should never overflow (result <= value), but verify.
    assert_eq!(crate::quantize(65535, 2000, Rounding::Floor), 64000);
    assert_eq!(crate::quantize(65535, 30000, Rounding::Floor), 60000);
}

#[test]
fn quantize_large_step_near_max() {
    // step > u16::MAX/2 — previous code would always overflow for Nearest.
    assert_eq!(crate::quantize(40000, 40000, Rounding::Nearest), 40000);
    assert_eq!(crate::quantize(60000, 40000, Rounding::Nearest), 65535);
    assert_eq!(crate::quantize(60000, 40000, Rounding::Ceil), 65535);
    assert_eq!(crate::quantize(60000, 40000, Rounding::Floor), 40000);
}

#[test]
fn quantize_step_equals_max_u16() {
    // Edge case: step = u16::MAX.
    assert_eq!(crate::quantize(0, 65535, Rounding::Floor), 0);
    assert_eq!(crate::quantize(0, 65535, Rounding::Ceil), 0);
    assert_eq!(crate::quantize(0, 65535, Rounding::Nearest), 0);
    assert_eq!(crate::quantize(65535, 65535, Rounding::Floor), 65535);
    assert_eq!(crate::quantize(65535, 65535, Rounding::Ceil), 65535);
    assert_eq!(crate::quantize(65535, 65535, Rounding::Nearest), 65535);
    assert_eq!(crate::quantize(32767, 65535, Rounding::Nearest), 0);
    assert_eq!(crate::quantize(32768, 65535, Rounding::Nearest), 65535);
}

/// Proves that the bug caused ramps to 65535 to be skipped entirely.
/// With the overflow, `end_val_q` wraps to 0, matching `current_val` at t=0,
/// making the scheduler think the ramp is already complete.
#[test]
fn tickless_ramp_to_max_with_large_step_not_skipped() {
    let curve = linear_curve();
    let schedule = curve.tickless_schedule(
        0,     // t0_ms
        100,   // duration_ms
        0,     // start_val
        65535, // end_val
        2000,  // step — triggers the overflow in old code
        Rounding::Nearest,
        0, // min_dt_ms
    );

    // At t=0, the ramp should NOT be finished — it should have intermediate steps.
    let dl_start = schedule.next_deadline(0);
    assert_eq!(dl_start.current_val, 0, "start value should be 0");
    // The deadline should be BEFORE end_ms, meaning there's a transition to wake for.
    assert!(
        dl_start.deadline_ms < 100,
        "first deadline {} should be before end_ms 100 (ramp has intermediate steps)",
        dl_start.deadline_ms
    );

    // At t=50 (midpoint), value should be roughly half of 65535.
    let dl_mid = schedule.next_deadline(50);
    assert!(
        dl_mid.current_val > 0,
        "midpoint value should be non-zero, got {}",
        dl_mid.current_val
    );
    assert!(
        dl_mid.current_val < 65535,
        "midpoint value should be below max, got {}",
        dl_mid.current_val
    );

    // Collect all deadlines — should have multiple steps, not just one.
    let deadlines: Vec<TicklessDeadline> = schedule.iter(0).collect();
    assert!(
        deadlines.len() > 2,
        "ramp 0→65535 with step=2000 should produce >2 deadlines, got {}",
        deadlines.len()
    );
}

/// Same test but for decreasing ramp with Ceil rounding.
#[test]
fn tickless_ramp_from_max_with_large_step_ceil() {
    let curve = linear_curve();
    let schedule = curve.tickless_schedule(
        0,     // t0_ms
        100,   // duration_ms
        65535, // start_val
        0,     // end_val
        2000,  // step
        Rounding::Ceil,
        0, // min_dt_ms
    );

    let dl_start = schedule.next_deadline(0);
    // Start should be quantized from 65535 — with fix, should be 65535 (capped).
    assert!(
        dl_start.current_val > 60000,
        "start value should be near max, got {}",
        dl_start.current_val
    );

    let deadlines: Vec<TicklessDeadline> = schedule.iter(0).collect();
    assert!(
        deadlines.len() > 2,
        "ramp 65535→0 with step=2000 should produce >2 deadlines, got {}",
        deadlines.len()
    );
}
