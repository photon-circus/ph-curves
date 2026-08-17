# Tickless scheduling

`TicklessSchedule` computes the first wall-clock deadline at which a quantized
monotonic curve output changes. Firmware can arm a timer for that deadline
instead of polling the curve at a fixed rate.

The root [scheduling quick start](../../README.md#quick-start-schedule-without-polling)
shows construction and iteration. This guide records the clock model that is
easy to miss in a short example.

## Clock and rollover model

Timestamps are free-running `u32` milliseconds. Schedule math uses wrapping
addition and subtraction, so a segment may begin near `u32::MAX` and cross the
roughly 49.7-day rollover.

A deadline after rollover can be numerically smaller than `now_ms`. Compare in
wrapping remaining-time order:

```rust
let remaining_ms = deadline_ms.wrapping_sub(now_ms);
```

Do not use ordinary absolute ordering across rollover. The same rule applies
to the schedule end timestamp.

For durations up to `i32::MAX` milliseconds—about 24.85 days—the usual
half-range signed-delta convention can classify an arbitrary wrapping wall
clock as before or after the segment. Longer durations remain valid when the
caller supplies segment-relative elapsed time with `t0_ms == 0`; a wrapping
wall clock cannot uniquely identify one interval longer than half its period.

## Quantization and repeat modes

The scheduler finds the first actual output change for the configured rounding
and step, including increasing and decreasing curves. `min_dt_ms` can enforce
a minimum wake-up interval without changing curve evaluation.

- `RepeatMode::Once` emits the terminal quantized value and terminates.
- `RepeatMode::Repeat` restarts the segment.
- `RepeatMode::PingPong` alternates direction at the endpoints.
- A zero-duration schedule emits its terminal value once and terminates rather
  than producing a zero-delay loop.

Use the current [`Tickless`](https://docs.rs/ph-curves/latest/ph_curves/trait.Tickless.html)
and `TicklessSchedule` rustdoc for exact constructor, rounding, iterator, and
error contracts.
