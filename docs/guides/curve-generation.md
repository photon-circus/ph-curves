# Curve generation

Use normalized curves when a complete discrete input domain maps to a complete
discrete output domain: brightness ramps, easing, fades, and other monotonic or
nonmonotonic shaping functions. Use a [physical transfer](physical-transfer-generation.md)
instead when the input is an ADC code or another bounded observation with
physical units.

## Definition styles

Each `[curves.<name>]` table uses exactly one source:

| Source | Use |
| --- | --- |
| `builtin` | A named, maintained easing function |
| `formula` | A host-evaluated expression over normalized `t` in `0..=1` |
| `points` | Piecewise-linear control points expressed in absolute LUT indices |

Set `monotonic = false` when the curve is intentionally nonmonotonic and must
not receive an inverse LUT.

```toml
[curves.gamma_22]
formula = "pow(t, 2.2)"

[curves.contrast]
points = [[0, 0], [64, 32], [128, 128], [192, 224], [255, 255]]
```

## Built-in curves

| Name | Formula | Shape |
| --- | --- | --- |
| `linear` | `t` | Identity |
| `ease_in_quad` | `t²` | Quadratic ease-in |
| `ease_out_quad` | `1-(1-t)²` | Quadratic ease-out |
| `ease_in_out_quad` | piecewise quadratic | Quadratic ease-in-out |
| `ease_in_cubic` | `t³` | Cubic ease-in |
| `ease_out_cubic` | `1-(1-t)³` | Cubic ease-out |
| `ease_in_out_cubic` | piecewise cubic | Cubic ease-in-out |
| `ease_in_quart` | `t⁴` | Quartic ease-in |
| `ease_out_quart` | `1-(1-t)⁴` | Quartic ease-out |
| `ease_in_out_quart` | piecewise quartic | Quartic ease-in-out |
| `ease_in_expo` | `2^(10(t-1))` | Exponential ease-in |
| `ease_out_expo` | `1-2^(-10t)` | Exponential ease-out |
| `smoothstep` | `3t²-2t³` | Hermite smoothstep |
| `smoother_step` | `6t⁵-15t⁴+10t³` | Improved smoothstep |

Legacy aliases `ease_in`, `ease_out`, and `ease_in_out` select the quadratic
variants.

## Formula language

Formulas use `t` over `0.0..=1.0` and are evaluated only by the host generator.

- Operators: `+`, `-`, `*`, `/`, `^` or `**`, unary `-`, and parentheses.
- Functions: `pow`, `sqrt`, `abs`, `min`, `max`, `clamp`, `sin`, `cos`,
  `tan`, `exp`, `ln`, and `log2`.
- Constants: `pi` and `e`.

```toml
[curves.cie_lightness]
formula = "pow((t + 0.16) / 1.16, 3.0)"
```

## Domain and target constraints

Dense curves cover the complete domain of their input type. A `u8` curve has
256 entries; a `u16` curve has 65,536. Point coordinates are absolute indices,
so a point document must use endpoints for the selected LUT size.

A full-domain `u16` array requires a target whose `usize` can represent
65,536. On a 16-bit-pointer target, use a `u8` curve, a smaller custom
`UnitValue`, or a sparse physical transfer. Generic `CurveLut` construction is
`const` and therefore relies on the caller to provide complete-domain arrays;
evaluation panics if an input index is outside the supplied table.

## Generated identity

Curve and transfer names normalize to uppercase Rust identifiers. Generation
fails when a name has no ASCII letters or digits, two names normalize to the
same identifier, or a name collides with a generated companion such as
`_FWD`, `_INV`, `_INPUTS`, `_OUTPUTS`, `_METADATA`, or
`_OBSERVATION_GUARD`.

See [`assets/curves.toml`](../../assets/curves.toml) and
[`assets/curves-u16.toml`](../../assets/curves-u16.toml) for complete inputs.
The root [curve quick start](../../README.md#quick-start-generate-a-curve)
shows generation and firmware inclusion.
