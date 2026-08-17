# User guides

These guides preserve practical detail that would make the root README too
long. Begin with the [quick starts](../../README.md#quick-starts), then use the
guide that matches the part of the pipeline you are designing.

| Goal | Guide |
| --- | --- |
| Choose a built-in curve, formula, LUT size, or generated name | [Curve generation](curve-generation.md) |
| Model an ADC-to-measurement relationship and set fitting policy | [Physical transfer generation](physical-transfer-generation.md) |
| Compose calibration, filtering, stability, and decisions | [Measurement pipelines](measurement-pipelines.md) |
| Schedule quantized output changes across wrapping clocks | [Tickless scheduling](tickless-scheduling.md) |

For exact signatures, errors, and panic conditions, use the current
[public API documentation](https://docs.rs/ph-curves). For migration and wire
compatibility, use the [compatibility policy](../compatibility.md).

Return to the [documentation map](../README.md).
