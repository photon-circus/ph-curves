# Physical transfer generation

A physical transfer maps one monotonic `u16` observation domain to signed,
scaled `i32` measurement quanta. It is suitable for ADC codes, integer
millivolts, or another bounded integer observation—not multidimensional sensor
state or driver behavior.

## Design checklist

1. Choose the integer observation firmware already has.
2. Choose an output unit and integer scale. For example,
   `output_unit = "kilopascal"` with `output_scale = 1000` emits milli-kPa.
3. Declare the valid input domain and explicit below/above behavior.
4. Select a formula, empirical points, or a supported host model.
5. Set the interpolation-error target and bounded knot budget.
6. Generate, inspect the report and manifest, then compare against independent
   reference measurements.

## Source choices

An analytical relationship can use `x`, the integer observation:

```toml
[transfers.pressure_100kpa]
input_unit = "adc_code"
output_unit = "kilopascal"
output_scale = 1000
domain = [410, 3686]
formula = "(x - 410) * 100.0 / 3276.0"
max_interpolation_error = 1
max_knots = 32
```

Empirical or piecewise relationships use strictly increasing inputs and
monotonic outputs. The endpoints define the valid observation domain.

```toml
[transfers.tank_level]
input_unit = "millivolt"
output_unit = "percent"
output_scale = 100
max_interpolation_error = 5
max_knots = 32
below = "clamp"
above = "clamp"
points = [
  { input = 500, output = 0.0 },
  { input = 1200, output = 28.0 },
  { input = 2050, output = 82.0 },
  { input = 2500, output = 100.0 },
]
```

The built-in NTC Beta-divider model covers a ratiometric divider. Other device
models normally belong in a dedicated host crate that supplies physical
points, evaluated dense truth, or prefitted knots through the public host IR.
See [`assets/transfers.toml`](../../assets/transfers.toml) for the complete NTC
reference.

## Fitting and verification

The generator selects nonuniform knots with a bounded greedy heuristic and
then checks every integer observation in the emitted domain. The reported
achieved error includes integer output quantization and interpolation against
the configured ideal source.

Meeting the requested error is exhaustively verified for the emitted table.
Reaching `max_knots`, however, does not prove that no alternative placement
could meet the target. Increase the bounded knot budget or use a domain-specific
fitter when that distinction matters.

The numerical bound is not total sensor accuracy. Separately account for
sensor/model tolerance, ADC and reference error, self-heating, wiring,
calibration uncertainty, and environmental effects.

## Boundaries and observation guards

`below` and `above` independently select `"error"` or `"clamp"`. Transfers
never extrapolate. One exact code above the generated domain may use a separate
guard:

```toml
[transfers]
requires = ["observation_guard_v1"]

[transfers.guarded_identity]
input_unit = "adc_code"
output_unit = "millivolt"
output_scale = 1
domain = [1, 60000]
formula = "x"
max_interpolation_error = 1
above = "clamp"
saturation = { code = 65535, behavior = "error" }
```

The guard is checked before ordinary boundary behavior. `error` produces
`RejectedObservation`; `clamp` returns the value at `domain_max`. It does not
change inverse conversion and is explicit device policy—`u16::MAX` is not
implicitly saturation.

The capability marker makes older generators reject the document instead of
silently dropping the guard. Standalone provenance similarly uses
`source_provenance_v1`. Transfer families use `transfer_families_v1`, which
also covers family-local guard and provenance fields. See the
[compatibility policy](../compatibility.md#host-toml-fail-closed-guards-provenance-and-families)
before sharing definition files across generator versions.

## Fit and non-goals

Use this transfer layer when one static monotonic observation determines one
measurement and explicit endpoint policy is sufficient. It does not provide:

- signed or wider-than-`u16` inputs, or outputs wider than `i32`;
- nonmonotonic forward maps or multidimensional compensation;
- dense physical-domain inverse LUTs;
- calibration discovery, coefficient persistence, or automatic unit
  conversion and transfer chaining;
- sensor fusion, state estimation, acquisition, cadence, fault management, or
  hardware actuation;
- an arbitrary host-model plugin interface.

Variants sharing one source belong in a
[transfer family](../design/transfer-families.md). The root
[transfer quick start](../../README.md#quick-start-convert-an-adc-observation)
shows runtime conversion, and the [host IR](../design/host-transfer-ir.md)
documents programmatic extension.
