//! Tickless scheduling helpers for monotonic curves.
//!
//! Instead of polling a curve at a fixed tick rate, the tickless scheduler
//! computes the exact wall-clock deadline at which the *quantized* output
//! value will next change.  This lets interrupt-driven firmware sleep between
//! transitions, saving power and CPU cycles.

use crate::MonotonicCurve;
use crate::math::{Rounding, UnitValue, next_target_value, quantize};

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
    /// Set a hardware timer or `sleep_until` to this value.
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
    pub fn end_ms(&self) -> u32 {
        self.t0_ms.saturating_add(self.duration_ms)
    }

    /// Compute the next deadline after `now_ms`.
    ///
    /// Returns the quantized output value that should be applied *now* and the
    /// wall-clock time at which the next quantized transition will occur.
    ///
    /// When `now_ms` is at or past the end of the segment, the returned
    /// deadline is clamped to the segment end and the final quantized value.
    pub fn next_deadline(&self, now_ms: u32) -> TicklessDeadline {
        let end_ms = self.end_ms();

        let current_t = self.time_to_t(now_ms);
        let w = self.curve.eval(current_t);
        let raw_val = w.lerp_u16(self.start_val, self.end_val);
        let current_val = quantize(raw_val, self.step, self.rounding);
        let end_val_q = quantize(self.end_val, self.step, self.rounding);

        if now_ms >= end_ms || current_val == end_val_q {
            return TicklessDeadline {
                deadline_ms: end_ms.max(now_ms),
                current_val,
            };
        }

        let increasing = self.end_val >= self.start_val;
        let target_val = next_target_value(current_val, end_val_q, self.step, increasing);
        let w_target = T::inv_lerp_u16(self.start_val, self.end_val, target_val);
        let u_target = self.curve.inv(w_target);
        let mut deadline_ms = self.t_to_time(u_target);

        let min_deadline = now_ms.saturating_add(self.min_dt_ms);
        if deadline_ms < min_deadline {
            deadline_ms = min_deadline;
        }
        if deadline_ms > end_ms {
            deadline_ms = end_ms;
        }
        if deadline_ms < now_ms {
            deadline_ms = now_ms;
        }

        TicklessDeadline {
            deadline_ms,
            current_val,
        }
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

    // -- private helpers --------------------------------------------------

    fn time_to_t(&self, now_ms: u32) -> T {
        if self.duration_ms == 0 || now_ms >= self.end_ms() {
            return T::one();
        }
        if now_ms <= self.t0_ms {
            return T::zero();
        }
        T::from_time_frac(now_ms - self.t0_ms, self.duration_ms)
    }

    fn t_to_time(&self, t: T) -> u32 {
        self.t0_ms
            .saturating_add(t.to_time_offset(self.duration_ms))
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
        match self.schedule.repeat {
            RepeatMode::Once => false,
            RepeatMode::Repeat => {
                self.t0_ms = self.t0_ms.saturating_add(self.schedule.duration_ms);
                true
            }
            RepeatMode::PingPong => {
                self.t0_ms = self.t0_ms.saturating_add(self.schedule.duration_ms);
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

        let cycle_finished = dl.deadline_ms >= end_ms || dl.current_val == end_val_q;

        if cycle_finished {
            if !self.advance_cycle() {
                self.done = true;
            } else {
                self.now_ms = end_ms;
            }
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
