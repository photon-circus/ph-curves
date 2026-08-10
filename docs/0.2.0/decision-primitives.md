# Hysteresis + Debounce (design)

**Branch:** `feature/0.2.0-decision-primitives`  
**Status:** Shipped in 0.2.0 — `Hysteresis` and `Debounce` in `stabilize`. Retained as the design record; the code and its rustdoc are authoritative.

## Motivation

Stabilize answers “is the signal quiet?” Applications still need Schmitt/hysteresis thresholds and contact debounce without owning GPIO. PR #1 already excludes “hysteretic application decisions” from Transfer — put these beside Stabilize (like `StabilityDetector`), not inside `TransferFunction`.

## Relation to PR #1 types

| PR #1 surface | This design |
| ------------- | ----------- |
| `TemporalFilter` / `FilterOutput<T>` | Not forced — wrong shape for latched bools / edges |
| `StabilityDetector` | Precedent: caller-driven `update` / `reset`, small enum output |
| `TemporalSample` | Reused by `Hysteresis<T>` for `u16` / `i32` samples |
| Transfer / Affine | Optional upstream; raw digital inputs can feed `Debounce` directly |
| Explicit deferral of hysteretic decisions | Satisfied by this stabilize-family companion |

Pipeline placement: observation → Transfer → optional Affine → optional filter → **Hyst / Debounce** → app policy.

## API sketch

```rust
pub struct Hysteresis<T: TemporalSample> {
    low: T,
    high: T,
    latched: bool,
}

impl<T: TemporalSample> Hysteresis<T> {
    /// Panics if low > high.
    pub const fn new(low: T, high: T) -> Self;
    pub const fn with_initial(self, on: bool) -> Self;

    pub fn update(&mut self, value: T) -> bool {
        if value >= self.high { self.latched = true; }
        else if value <= self.low { self.latched = false; }
        self.latched
    }
    pub const fn state(&self) -> bool;
    pub fn reset(&mut self);
}

/// N consecutive agreeing bool samples → latch.
pub struct Debounce<const N: usize> {
    candidate: bool,
    streak: usize,
    latched: bool,
    armed: bool, // false until first latch
}

pub enum DebounceOutput {
    WarmingUp { streak: usize, required: usize },
    Steady(bool),
    Edge { level: bool },
}

impl<const N: usize> Debounce<N> {
    pub const fn new() -> Self; // assert N > 0
    pub fn update(&mut self, sample: bool) -> DebounceOutput;
    pub fn state(&self) -> Option<bool>;
    pub fn reset(&mut self);
}
```

Suggested module: `stabilize` (+ crate re-export). Cadence is **sample-count**, not wall time — caller converts ms → N if needed.

## Keep-outs / bounds

- No time-based debounce (`Instant`, embassy clocks, ms internals)
- No GPIO / pin / EXTI wrappers
- No hidden sample-rate assumptions (`N` is samples, not seconds)
- No auto last-good value injection (same honesty as `StabilityDetector`)
- Do not put hysteresis inside `TransferFunction`
- No async / interrupt frameworks

## Non-goals

- Application policy beyond latched bool / edge reporting
- Sensor-fusion or multi-input voting
