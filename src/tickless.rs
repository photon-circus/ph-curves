//! Tickless scheduling helpers for monotonic curves.
//!
//! Instead of polling a curve at a fixed tick rate, the tickless scheduler
//! computes the exact wall-clock deadline at which the *quantized* output
//! value will next change.  This lets interrupt-driven firmware sleep between
//! transitions, saving power and CPU cycles.
//!
//! # Wrapping clocks
//!
//! Timestamps are free-running `u32` milliseconds. All schedule math uses
//! wrapping arithmetic so a segment that starts near `u32::MAX` can cross the
//! ~49.7-day rollover.
//!
//! For durations up to `i32::MAX` milliseconds (~24.85 days), before-start /
//! past-end classification uses the usual half-range signed-delta convention
//! (`now.wrapping_sub(t0) as i32`). Longer durations remain supported when
//! `now_ms` is a segment-relative elapsed time with `t0_ms == 0` and
//! `now_ms <= duration_ms` — the mode used for multi-day relative ramps.
//! A wrapping wall clock cannot unambiguously represent a single segment
//! longer than half the clock period.
//!
//! Deadline clamping works on *offsets from `t0_ms`*, never on absolute
//! timestamps. Every offset is bounded by `duration_ms`, so the comparisons
//! stay ordinary `u32` ones and remain correct across the full `u32` duration
//! range. The half-range convention appears only where it is unavoidable —
//! deciding whether `now_ms` precedes the segment at all. Clamping absolute
//! timestamps against a half-range convention instead would misread any
//! segment longer than ~24.85 days as already finished.

use crate::MonotonicCurve;
use crate::math::{Rounding, UnitValue, quantize};

/// Half the `u32` range. Deltas larger than this are treated as negative under
/// the signed wrapping convention used by free-running embedded clocks.
const HALF_RANGE_MS: u32 = i32::MAX as u32;

/// Progress of `now_ms` relative to a segment `[t0, t0+duration)`.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum SegmentProgress {
    BeforeStart,
    InSegment(u32),
    PastEnd,
}

/// Classify `now` against a segment start/duration with wrap-safe elapsed math.
///
/// When `duration_ms <= HALF_RANGE_MS`, elapsed values above the half-range are
/// "before start". Longer durations skip that check so relative `t0 == 0`
/// schedules can still use the full `u32` duration range.
fn segment_progress(t0_ms: u32, duration_ms: u32, now_ms: u32) -> SegmentProgress {
    if duration_ms == 0 {
        return SegmentProgress::PastEnd;
    }
    let elapsed = now_ms.wrapping_sub(t0_ms);
    if duration_ms <= HALF_RANGE_MS && elapsed > HALF_RANGE_MS {
        SegmentProgress::BeforeStart
    } else if elapsed >= duration_ms {
        SegmentProgress::PastEnd
    } else {
        SegmentProgress::InSegment(elapsed)
    }
}

/// Repeat behaviour for a tickless schedule.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum RepeatMode {
    /// Play once and stop.
    Once,
    /// Loop back to the start value after each cycle.
    Repeat,
    /// Reverse direction after each cycle (start→end, end→start, …).
    PingPong,
}

/// A single output produced by the tickless scheduler.
///
/// Each deadline tells the caller what the current quantized output value is
/// and when the *next* transition will occur, so the caller can set a timer
/// and go to sleep.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct TicklessDeadline {
    /// Wall-clock time (in milliseconds) at which the output will next change.
    ///
    /// Set a hardware timer or `sleep_until` to this value. On a free-running
    /// `u32` clock the value may be numerically less than `now` when the
    /// deadline crosses the rollover; compare with wrapping remaining-time
    /// (`deadline.wrapping_sub(now)`), not signed absolute order.
    pub deadline_ms: u32,
    /// The quantized output value that should be applied *now* (at the time
    /// this deadline was computed).
    pub current_val: u16,
}

/// A tickless schedule bound to a monotonic curve and segment parameters.
///
/// `C` is the curve type and `T` is the curve's normalised value type
/// (e.g. `u8`). Use [`Tickless::tickless_schedule`] to construct one
/// fluently, or build it directly with [`TicklessSchedule::new`].
#[derive(Copy, Clone, Debug)]
pub struct TicklessSchedule<C, T: UnitValue = u8> {
    curve: C,
    t0_ms: u32,
    duration_ms: u32,
    start_val: u16,
    end_val: u16,
    step: u16,
    rounding: Rounding,
    min_dt_ms: u32,
    repeat: RepeatMode,
    _marker: core::marker::PhantomData<T>,
}

impl<C, T> TicklessSchedule<C, T>
where
    C: MonotonicCurve<T, T>,
    T: UnitValue,
{
    /// Create a new tickless schedule.
    ///
    /// # Parameters
    ///
    /// - `curve` — the monotonic curve that shapes the transition.
    /// - `t0_ms` — wall-clock start time of the segment in milliseconds.
    /// - `duration_ms` — total duration of the segment in milliseconds.
    ///   A zero duration is treated as already finished: [`Self::next_deadline`]
    ///   returns the quantized end value due immediately.
    ///   [`RepeatMode::Repeat`] and [`RepeatMode::PingPong`] do not cycle a
    ///   zero-length segment, so [`Self::iter`] terminates after that single
    ///   due-now deadline instead of spinning.
    /// - `start_val` — raw output value at `t = 0` (before quantization).
    /// - `end_val` — raw output value at `t = 1` (before quantization).
    /// - `step` — quantization step size (clamped to a minimum of 1).
    /// - `rounding` — how values are snapped to the quantization grid.
    /// - `min_dt_ms` — minimum time between successive deadlines.  Useful for
    ///   rate-limiting hardware updates.  Set to `0` for no limit.
    pub fn new(
        curve: C,
        t0_ms: u32,
        duration_ms: u32,
        start_val: u16,
        end_val: u16,
        step: u16,
        rounding: Rounding,
        min_dt_ms: u32,
    ) -> Self {
        Self {
            curve,
            t0_ms,
            duration_ms,
            start_val,
            end_val,
            step: step.max(1),
            rounding,
            min_dt_ms,
            repeat: RepeatMode::Once,
            _marker: core::marker::PhantomData,
        }
    }

    /// Set the repeat mode, consuming and returning `self` for chaining.
    pub fn with_repeat(mut self, mode: RepeatMode) -> Self {
        self.repeat = mode;
        self
    }

    /// The end time of the current segment in milliseconds.
    ///
    /// Computed with wrapping addition so a segment that starts near
    /// `u32::MAX` can end after the clock rolls over.
    pub fn end_ms(&self) -> u32 {
        self.t0_ms.wrapping_add(self.duration_ms)
    }

    /// Compute the next deadline after `now_ms`.
    ///
    /// Returns the quantized output value that should be applied *now* and the
    /// wall-clock time at which the next quantized transition will occur.
    ///
    /// When `now_ms` is at or past the end of the segment, the returned
    /// deadline is `now_ms` (already due) and the final quantized value.
    pub fn next_deadline(&self, now_ms: u32) -> TicklessDeadline {
        let end_ms = self.end_ms();
        let progress = segment_progress(self.t0_ms, self.duration_ms, now_ms);

        let current_t = match progress {
            SegmentProgress::BeforeStart => T::zero(),
            SegmentProgress::PastEnd => T::one(),
            SegmentProgress::InSegment(elapsed) => {
                if elapsed == 0 {
                    T::zero()
                } else {
                    T::from_time_frac(elapsed, self.duration_ms)
                }
            }
        };

        let w = self.curve.eval(current_t);
        let raw_val = w.lerp_u16(self.start_val, self.end_val);
        let current_val = quantize(raw_val, self.step, self.rounding);
        let end_val_q = quantize(self.end_val, self.step, self.rounding);

        if matches!(progress, SegmentProgress::PastEnd) || current_val == end_val_q {
            let deadline_ms = if matches!(progress, SegmentProgress::PastEnd) {
                // Already due. Prefer `now` over a numerically-smaller wrapped
                // `end_ms` so callers do not arm a nearly-full-period sleep.
                now_ms
            } else {
                end_ms
            };
            return TicklessDeadline {
                deadline_ms,
                current_val,
            };
        }

        // Clamp in offset-from-`t0` space. `PastEnd` already returned above, so
        // `now` is either inside the segment or ahead of it, and every offset
        // below is bounded by `duration_ms` — plain `u32` comparisons hold even
        // when the segment is longer than half the clock period.
        let (now_off, min_off) = match progress {
            SegmentProgress::InSegment(elapsed) => {
                (elapsed, elapsed.saturating_add(self.min_dt_ms))
            }
            // `now` precedes `t0`; its offset is negative, so it can never floor
            // a non-negative deadline. Only the `min_dt` window reaches into the
            // segment, and only by whatever is left after covering the gap.
            SegmentProgress::BeforeStart => (
                0,
                self.min_dt_ms
                    .saturating_sub(self.t0_ms.wrapping_sub(now_ms)),
            ),
            SegmentProgress::PastEnd => (self.duration_ms, self.duration_ms),
        };

        // Search elapsed time, not inverted grid points. `inv_lerp` of a raw
        // quantization boundary disagrees with forward `lerp_u16` truncation,
        // so a closed-form inverse wakes early and the iterator then sees a
        // due-now deadline at the same quantized value.
        let mut dl_off = self.first_quantized_change_offset(now_off, current_val);
        if dl_off < min_off {
            dl_off = min_off;
        }
        if dl_off > self.duration_ms {
            dl_off = self.duration_ms;
        }
        if dl_off < now_off {
            dl_off = now_off;
        }

        TicklessDeadline {
            deadline_ms: self.t0_ms.wrapping_add(dl_off),
            current_val,
        }
    }

    /// Quantized output at offset `elapsed_ms` from `t0`, using the same
    /// `from_time_frac` / eval / lerp / quantize path as [`Self::next_deadline`].
    fn quantized_at_offset(&self, elapsed_ms: u32) -> u16 {
        let t = if elapsed_ms == 0 {
            T::zero()
        } else {
            T::from_time_frac(elapsed_ms, self.duration_ms)
        };
        let raw = self.curve.eval(t).lerp_u16(self.start_val, self.end_val);
        quantize(raw, self.step, self.rounding)
    }

    /// First offset `> now_off` at which [`Self::quantized_at_offset`] differs
    /// from `current_val`, or `duration_ms` if it never does.
    fn first_quantized_change_offset(&self, now_off: u32, current_val: u16) -> u32 {
        let mut lo = now_off;
        let mut hi = self.duration_ms;
        if self.quantized_at_offset(hi) == current_val {
            return hi;
        }
        while lo + 1 < hi {
            let mid = lo + (hi - lo) / 2;
            if self.quantized_at_offset(mid) != current_val {
                hi = mid;
            } else {
                lo = mid;
            }
        }
        hi
    }

    /// Return an iterator that yields successive [`TicklessDeadline`] values
    /// starting from `now_ms`, automatically advancing to each deadline.
    ///
    /// For [`RepeatMode::Once`] the iterator finishes when the segment ends.
    /// For [`RepeatMode::Repeat`] and [`RepeatMode::PingPong`] it cycles
    /// indefinitely.
    pub fn iter(&self, now_ms: u32) -> TicklessIter<'_, C, T> {
        TicklessIter {
            schedule: self,
            t0_ms: self.t0_ms,
            start_val: self.start_val,
            end_val: self.end_val,
            now_ms,
            done: false,
        }
    }
}

/// Iterator over successive [`TicklessDeadline`] values produced by a
/// [`TicklessSchedule`].
#[derive(Debug)]
pub struct TicklessIter<'a, C, T: UnitValue = u8> {
    schedule: &'a TicklessSchedule<C, T>,
    t0_ms: u32,
    start_val: u16,
    end_val: u16,
    now_ms: u32,
    done: bool,
}

impl<C, T> TicklessIter<'_, C, T>
where
    C: MonotonicCurve<T, T> + Copy,
    T: UnitValue,
{
    /// Build a single-cycle schedule from the iterator's current state.
    fn cycle_schedule(&self) -> TicklessSchedule<C, T> {
        TicklessSchedule {
            curve: self.schedule.curve,
            t0_ms: self.t0_ms,
            duration_ms: self.schedule.duration_ms,
            start_val: self.start_val,
            end_val: self.end_val,
            step: self.schedule.step,
            rounding: self.schedule.rounding,
            min_dt_ms: self.schedule.min_dt_ms,
            repeat: RepeatMode::Once,
            _marker: core::marker::PhantomData,
        }
    }

    /// Advance to the next cycle, returning `true` if the iterator continues.
    fn advance_cycle(&mut self) -> bool {
        if self.schedule.duration_ms == 0 {
            return false;
        }
        match self.schedule.repeat {
            RepeatMode::Once => false,
            RepeatMode::Repeat => {
                self.t0_ms = self.t0_ms.wrapping_add(self.schedule.duration_ms);
                true
            }
            RepeatMode::PingPong => {
                self.t0_ms = self.t0_ms.wrapping_add(self.schedule.duration_ms);
                core::mem::swap(&mut self.start_val, &mut self.end_val);
                true
            }
        }
    }
}

impl<C, T> Iterator for TicklessIter<'_, C, T>
where
    C: MonotonicCurve<T, T> + Copy,
    T: UnitValue,
{
    type Item = TicklessDeadline;

    fn next(&mut self) -> Option<TicklessDeadline> {
        if self.done {
            return None;
        }

        let cycle = self.cycle_schedule();
        let dl = cycle.next_deadline(self.now_ms);
        let end_ms = cycle.end_ms();
        let end_val_q = quantize(self.end_val, self.schedule.step, self.schedule.rounding);

        // Finish a cycle only once the emitted value is the terminal quantized
        // end. A deadline that lands on `end_ms` while `current_val` is still
        // pre-end must yield a follow-up PastEnd item so callers who apply
        // `current_val` at each deadline actually reach the endpoint.
        //
        // If the next change is due *now* but we are not at the terminal value,
        // jump to `end_ms` rather than parking `now` on the same timestamp —
        // otherwise Once never terminates and Repeat/PingPong spin.
        if dl.current_val == end_val_q {
            if !self.advance_cycle() {
                self.done = true;
            } else {
                self.now_ms = end_ms;
            }
        } else if dl.deadline_ms == self.now_ms {
            self.now_ms = end_ms;
        } else {
            self.now_ms = dl.deadline_ms;
        }

        Some(dl)
    }
}

/// Extension trait that adds tickless scheduling to any
/// [`MonotonicCurve<T, T>`] where `T: UnitValue`.
///
/// This is the primary entry point for building a [`TicklessSchedule`].
/// It is automatically implemented for every type that satisfies the bounds.
pub trait Tickless<T: UnitValue>: MonotonicCurve<T, T> + Sized + Copy {
    /// Build a [`TicklessSchedule`] for this curve.
    ///
    /// See [`TicklessSchedule::new`] for parameter descriptions.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use ph_curves::{Tickless, Rounding};
    ///
    /// let schedule = curve.tickless_schedule(
    ///     0,     // t0_ms: start time
    ///     1000,  // duration_ms
    ///     0,     // start_val
    ///     255,   // end_val
    ///     10,    // step (quantization)
    ///     Rounding::Nearest,
    ///     0,     // min_dt_ms
    /// );
    ///
    /// for deadline in schedule.iter(0) {
    ///     set_timer(deadline.deadline_ms);
    ///     set_output(deadline.current_val);
    /// }
    /// ```
    fn tickless_schedule(
        self,
        t0_ms: u32,
        duration_ms: u32,
        start_val: u16,
        end_val: u16,
        step: u16,
        rounding: Rounding,
        min_dt_ms: u32,
    ) -> TicklessSchedule<Self, T> {
        TicklessSchedule::new(
            self,
            t0_ms,
            duration_ms,
            start_val,
            end_val,
            step,
            rounding,
            min_dt_ms,
        )
    }
}

impl<C, T> Tickless<T> for C
where
    C: MonotonicCurve<T, T> + Copy,
    T: UnitValue,
{
}
