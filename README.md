# ph-curves

[![Crates.io](https://img.shields.io/crates/v/ph-curves)](https://crates.io/crates/ph-curves)
[![docs.rs](https://img.shields.io/docsrs/ph-curves)](https://docs.rs/ph-curves)
[![CI](https://github.com/photon-circus/ph-curves/actions/workflows/ci.yml/badge.svg)](https://github.com/photon-circus/ph-curves/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/crates/l/mit)](LICENSE.md)
[![MSRV](https://img.shields.io/badge/MSRV-1.92.0-blue)](rust-toolchain.toml)
[![no_std](https://img.shields.io/badge/no__std-yes-green)](src/lib.rs)

`no_std`, zero-allocation curve lookup tables and tickless scheduling for
embedded Rust.

## Features

- **Static LUTs** — curves are pre-computed at build time into `static` arrays,
  so evaluation is a single index into a `&'static [u8; 256]` (or `[u16; 65536]`).
- **`no_std` / `no_alloc`** — the library itself has zero runtime allocation.
  Only the `fixed` crate is required at runtime (fixed-point math).
- **Tickless scheduling** — computes the *next* wall-clock deadline where the
  quantized output value changes, so your firmware can sleep instead of polling.
- **Code-gen CLI** — a companion binary (`ph-curves-gen`) reads a simple TOML
  file and emits the Rust source for all your curves.

## Quick start

### 1. Define curves in TOML

Create a file (e.g. `assets/curves.toml`):

```toml
[curves.ease_in_quad]
builtin = "ease_in_quad"

[curves.gamma_22]
formula = "pow(t, 2.2)"

[curves.contrast_boost]
points = [[0, 0], [64, 32], [128, 128], [192, 224], [255, 255]]
```

Each curve uses exactly **one** of three definition styles:

| Style      | Description                                               |
|------------|-----------------------------------------------------------|
| `builtin`  | Name of a built-in easing function (14 available)         |
| `formula`  | Math expression in `t` (0→1), evaluated at build time     |
| `points`   | Piecewise-linear control points `[input, output]`         |

Set `monotonic = false` to skip inverse-LUT generation (default is `true`).

### 2. Generate Rust source

```sh
cargo install --path . --features gen

ph-curves-gen --input assets/curves.toml --output src/curves.rs
```

This produces a `.rs` file with `static` arrays and `const` curve values
ready to `include!` or copy into your crate.

For 16-bit resolution:

```sh
ph-curves-gen --input assets/curves.toml --output src/curves.rs \
    --value-type u16 --lut-size 65536
```

### 3. Use in firmware

```rust
use ph_curves::{Curve, MonotonicCurve, Tickless, Rounding};

include!("curves.rs");

// Simple evaluation — one table lookup.
let brightness: u8 = GAMMA_22.eval(input);

// Tickless scheduling — sleep until the next value change.
let schedule = EASE_IN_QUAD.tickless_schedule(
    0,           // t0_ms: start time
    1000,        // duration_ms
    0,           // start value
    255,         // end value
    10,          // step (quantization)
    Rounding::Nearest,
    0,           // min_dt_ms
);

for deadline in schedule.iter(0) {
    set_timer(deadline.deadline_ms);
    set_output(deadline.current_val);
}
```

## Built-in curves

| Name                 | Formula              | Description                      |
|----------------------|----------------------|----------------------------------|
| `linear`             | `t`                  | Identity / straight line         |
| `ease_in_quad`       | `t²`                 | Quadratic ease-in                |
| `ease_out_quad`      | `1-(1-t)²`           | Quadratic ease-out               |
| `ease_in_out_quad`   | piecewise quadratic  | Quadratic ease-in-out            |
| `ease_in_cubic`      | `t³`                 | Cubic ease-in                    |
| `ease_out_cubic`     | `1-(1-t)³`           | Cubic ease-out                   |
| `ease_in_out_cubic`  | piecewise cubic      | Cubic ease-in-out                |
| `ease_in_quart`      | `t⁴`                 | Quartic ease-in                  |
| `ease_out_quart`     | `1-(1-t)⁴`           | Quartic ease-out                 |
| `ease_in_out_quart`  | piecewise quartic    | Quartic ease-in-out              |
| `ease_in_expo`       | `2^(10(t-1))`        | Exponential ease-in              |
| `ease_out_expo`      | `1-2^(-10t)`         | Exponential ease-out             |
| `smoothstep`         | `3t²-2t³`            | Hermite smoothstep               |
| `smoother_step`      | `6t⁵-15t⁴+10t³`     | Ken Perlin's improved smoothstep |

Legacy aliases: `ease_in`, `ease_out`, `ease_in_out` (mapped to the quad
variants).

## Formula syntax

Formulas are math expressions over the variable `t` (0.0 to 1.0).

**Operators:** `+` `-` `*` `/` `^` (or `**`), unary `-`, parentheses.

**Functions:** `pow(x,y)` `sqrt(x)` `abs(x)` `min(x,y)` `max(x,y)`
`clamp(x,lo,hi)` `sin(x)` `cos(x)` `tan(x)` `exp(x)` `ln(x)` `log2(x)`

**Constants:** `pi` `e`

```toml
[curves.cie_lightness]
formula = "pow((t + 0.16) / 1.16, 3.0)"
```

## Library API

### Core types

| Type                   | Description                                         |
|------------------------|-----------------------------------------------------|
| `CurveLut<I,V,N,M>`   | Forward LUT + optional inverse LUT                  |
| `MonotonicCurveLut<I,V,N,M>` | Forward + required inverse LUT                |
| `CurveLut256`          | Type alias: `CurveLut<u8, u8, 256>`                |
| `MonotonicCurveLut256` | Type alias: `MonotonicCurveLut<u8, u8, 256>`        |
| `CurveLut65536`        | Type alias: `CurveLut<u16, u16, 65536>`             |
| `MonotonicCurveLut65536` | Type alias: `MonotonicCurveLut<u16, u16, 65536>`  |

### Traits

- **`Curve<I, V>`** — `eval(u: I) -> V` — single table lookup.
- **`MonotonicCurve<I, V>`** — adds `inv(w: V) -> I` — inverse lookup.
- **`Tickless<T>`** — adds `tickless_schedule(...)` to any `MonotonicCurve`.
- **`UnitValue`** — implemented for `u8` and `u16`; maps the unit interval
  onto a discrete integer range with fixed-point helpers.

### Tickless scheduling

`TicklessSchedule` computes the exact deadline (in milliseconds) at which the
quantized output value will next change. This lets interrupt-driven firmware
sleep between value changes instead of polling at a fixed tick rate.

Supports `RepeatMode::Once`, `RepeatMode::Repeat`, and `RepeatMode::PingPong`.

### Math helpers

- `lerp_u8(a, b, w)` / `lerp_u16(a, b, w)` — interpolate with `u8` weight.
- `map_u8_to_u16(w, max)` — scale a `u8` into a `u16` range.
- `quantize(value, step, rounding)` — snap to a step size.
- `next_target_value(current, end, step, increasing)` — next quantized target.

## Minimum supported Rust version

Rust **1.92.0** (edition 2024).

## Contributing

Contributions are welcome! Please read the [contributing guide](CONTRIBUTING.md)
before opening a pull request.

This project follows the
[Contributor Covenant Code of Conduct](CODE_OF_CONDUCT.md). By participating you
agree to uphold it.

For security issues, see our [security policy](SECURITY.md).

## License

[MIT](LICENSE.md)
