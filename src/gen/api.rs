//! Public code-generation entry points for `build.rs` and host tools.

// Host-only: module-local std link (crate root stays `#![no_std]`).
extern crate std;

use std::format;
use std::prelude::v1::*;

use std::fs;
use std::io;
use std::path::Path;

use super::codegen;
use super::curve::DefinitionsFile;

/// LUT index / value width used by generated curve tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueType {
    /// 8-bit domain (`lut_size` must be 256).
    U8,
    /// 16-bit domain (`lut_size` must be 65_536).
    U16,
}

impl ValueType {
    /// Parse a CLI / TOML-style type name (`"u8"` or `"u16"`).
    pub fn parse(name: &str) -> Result<Self, Error> {
        match name {
            "u8" => Ok(Self::U8),
            "u16" => Ok(Self::U16),
            other => Err(Error::Validation(format!(
                "unsupported value type `{other}` (expected `u8` or `u16`)"
            ))),
        }
    }

    /// Rust type name emitted in generated source.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::U8 => "u8",
            Self::U16 => "u16",
        }
    }

    /// Required full-domain LUT length for this value type.
    pub const fn required_lut_size(self) -> usize {
        match self {
            Self::U8 => 256,
            Self::U16 => 65_536,
        }
    }
}

/// Options controlling curve LUT generation.
///
/// Defaults match the CLI today: `u8` values with a 256-entry LUT.
/// Transfer-only documents should use [`Self::transfers_only`]; `value_type`
/// and `lut_size` are ignored unless the document contains `[curves]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerateOptions {
    /// Rust type used for LUT index and value (`u8` or `u16`).
    /// Ignored when the document has no `[curves]`.
    pub value_type: ValueType,
    /// Number of entries in each LUT (must cover the full value-type domain).
    /// Ignored when the document has no `[curves]`.
    pub lut_size: usize,
}

impl Default for GenerateOptions {
    fn default() -> Self {
        Self {
            value_type: ValueType::U8,
            lut_size: ValueType::U8.required_lut_size(),
        }
    }
}

impl GenerateOptions {
    /// Options for a document with no `[curves]`.
    ///
    /// LUT fields keep CLI-compatible defaults but are not validated or used
    /// when the definitions contain only transfers, families, or gaps.
    pub fn transfers_only() -> Self {
        Self::default()
    }
}

/// Errors from reading TOML, validating options, or generating source.
#[derive(Debug)]
pub enum Error {
    /// Filesystem read/write failure.
    Io(io::Error),
    /// Input was not valid TOML for the definitions schema.
    Toml(toml::de::Error),
    /// Options or definition content failed validation.
    Validation(String),
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "{error}"),
            Self::Toml(error) => write!(f, "invalid TOML: {error}"),
            Self::Validation(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Toml(error) => Some(error),
            Self::Validation(_) => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<toml::de::Error> for Error {
    fn from(error: toml::de::Error) -> Self {
        Self::Toml(error)
    }
}

/// Read a TOML definitions file and return generated Rust source.
pub fn generate_from_toml(path: impl AsRef<Path>, opts: &GenerateOptions) -> Result<String, Error> {
    let toml_str = fs::read_to_string(path)?;
    generate_from_str(&toml_str, opts)
}

/// Parse an in-memory TOML string and return generated Rust source.
pub fn generate_from_str(toml: &str, opts: &GenerateOptions) -> Result<String, Error> {
    let defs = DefinitionsFile::from_toml_str(toml)?;
    generate(&defs, opts)
}

/// Generate Rust source from `input` TOML and write it to `output`.
pub fn generate_to_path(
    input: impl AsRef<Path>,
    output: impl AsRef<Path>,
    opts: &GenerateOptions,
) -> Result<(), Error> {
    let source = generate_from_toml(input, opts)?;
    fs::write(output, source)?;
    Ok(())
}

/// Lower-level entry: already-parsed definitions → Rust source.
pub fn generate(defs: &DefinitionsFile, opts: &GenerateOptions) -> Result<String, Error> {
    if !defs.curves().is_empty() {
        validate_options(opts)?;
    }
    codegen::generate(defs, opts.value_type.as_str(), opts.lut_size).map_err(Error::Validation)
}

/// Read a TOML definitions file and return source plus the host audit report.
pub fn generate_from_toml_report(
    path: impl AsRef<Path>,
    opts: &GenerateOptions,
) -> Result<super::GenerationResult, Error> {
    let toml_str = fs::read_to_string(path)?;
    generate_from_str_report(&toml_str, opts)
}

/// Parse an in-memory TOML string and return source plus the host audit report.
pub fn generate_from_str_report(
    toml: &str,
    opts: &GenerateOptions,
) -> Result<super::GenerationResult, Error> {
    let defs = DefinitionsFile::from_toml_str(toml)?;
    generate_report(&defs, opts)
}

/// Lower-level entry: already-parsed definitions → source plus the host audit report.
///
/// Family aggregate budgets fail closed here. The `String`-returning helpers
/// call this function and discard the report, so they cannot bypass a budget.
pub fn generate_report(
    defs: &DefinitionsFile,
    opts: &GenerateOptions,
) -> Result<super::GenerationResult, Error> {
    if !defs.curves().is_empty() {
        validate_options(opts)?;
    }
    codegen::generate_with_report(defs, opts.value_type.as_str(), opts.lut_size)
        .map_err(Error::Validation)
}

fn validate_options(opts: &GenerateOptions) -> Result<(), Error> {
    let required = opts.value_type.required_lut_size();
    if opts.lut_size != required {
        return Err(Error::Validation(format!(
            "--lut-size must be {required} for --value-type {} \
             (the LUT must cover the full {} domain)",
            opts.value_type.as_str(),
            opts.value_type.as_str(),
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lut_size_must_cover_full_u8_domain() {
        let ok = GenerateOptions {
            value_type: ValueType::U8,
            lut_size: 256,
        };
        assert!(validate_options(&ok).is_ok());

        let bad = GenerateOptions {
            value_type: ValueType::U8,
            lut_size: 255,
        };
        assert_eq!(
            validate_options(&bad).unwrap_err().to_string(),
            "--lut-size must be 256 for --value-type u8 \
             (the LUT must cover the full u8 domain)"
        );
    }

    #[test]
    fn lut_size_must_cover_full_u16_domain() {
        let ok = GenerateOptions {
            value_type: ValueType::U16,
            lut_size: 65_536,
        };
        assert!(validate_options(&ok).is_ok());

        let bad = GenerateOptions {
            value_type: ValueType::U16,
            lut_size: 256,
        };
        assert_eq!(
            validate_options(&bad).unwrap_err().to_string(),
            "--lut-size must be 65536 for --value-type u16 \
             (the LUT must cover the full u16 domain)"
        );
    }

    #[test]
    fn value_type_parse_rejects_unknown() {
        let error = ValueType::parse("u32").unwrap_err().to_string();
        assert!(error.contains("unsupported value type"));
    }

    #[test]
    fn generate_from_str_linear_curve() {
        let toml = r#"
            [curves.linear]
            builtin = "linear"
        "#;
        let out = generate_from_str(toml, &GenerateOptions::default()).unwrap();
        assert!(out.contains("LINEAR_FWD"));
        assert!(out.contains("LINEAR_INV"));
        assert!(out.contains("// Auto-generated by ph-curves-gen."));
    }

    #[test]
    fn semantic_validation_returns_error_instead_of_panicking() {
        let error = generate_from_str("[curves.bad]\n", &GenerateOptions::default()).unwrap_err();

        assert!(matches!(error, Error::Validation(_)));
        assert!(error.to_string().contains("exactly one"));
    }

    #[test]
    fn invalid_formula_returns_validation_error() {
        let error = generate_from_str(
            "[curves.bad]\nformula = \"t @ 2\"\n",
            &GenerateOptions::default(),
        )
        .unwrap_err();

        assert!(matches!(error, Error::Validation(_)));
        assert!(error.to_string().contains("unexpected character"));
    }

    #[test]
    fn misspelled_transfers_table_returns_toml_error() {
        let error =
            generate_from_str("[tranfsers.sensor]\n", &GenerateOptions::default()).unwrap_err();

        assert!(matches!(error, Error::Toml(_)));
        let message = error.to_string();
        assert!(
            message.contains("unknown field `tranfsers`"),
            "expected the misspelled table to be named, got: {message}"
        );
    }

    #[test]
    fn unknown_table_returns_toml_error_not_header_only() {
        let toml = "[invented]\nfoo = 1\n";
        let error = generate_from_str(toml, &GenerateOptions::default()).unwrap_err();

        assert!(matches!(error, Error::Toml(_)));
        let message = error.to_string();
        assert!(
            message.contains("unknown field `invented`"),
            "expected a named unknown top-level table, got: {message}"
        );
    }

    #[test]
    fn misspelled_standalone_saturation_returns_toml_error() {
        let toml = r#"
[transfers]
requires = ["observation_guard_v1"]

[transfers.sensor]
input_unit = "count"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
saturaton = { code = 65535, behavior = "error" }
formula = "x"
domain = [1, 10]
"#;
        let error = generate_from_str(toml, &GenerateOptions::transfers_only()).unwrap_err();
        assert!(matches!(error, Error::Toml(_)));
        assert!(error.to_string().contains("unknown field `saturaton`"));
    }

    #[test]
    fn standalone_guard_without_capability_returns_toml_error() {
        let toml = r#"
[transfers.sensor]
input_unit = "count"
output_unit = "unit"
output_scale = 1
max_interpolation_error = 1
saturation = { code = 65535, behavior = "error" }
formula = "x"
domain = [1, 10]
"#;
        let error = generate_from_str(toml, &GenerateOptions::transfers_only()).unwrap_err();
        assert!(matches!(error, Error::Toml(_)));
        assert!(
            error
                .to_string()
                .contains("requires = [\"observation_guard_v1\"]")
        );
    }

    #[test]
    fn malformed_description_only_member_fails_generate() {
        let toml = r#"
[transfer_families.als]
input_unit = "count"
output_unit = "unit"
output_scale = 1000
max_interpolation_error = 50
formula = "x"

[[transfer_families.als.members]]
selectors = { gain = "div4", integration_time_ms = 100 }
status = "emit"
applicability = { observation = [1, 10] }

[[transfer_families.als.members]]
selectors = { gain = "x1", integration_time_ms = 100 }
status = "unnecessary"
reason = "still validated"
applicability = { observation = [10, 1] }
"#;
        let error = generate_from_str(toml, &GenerateOptions::default()).unwrap_err();
        assert!(matches!(error, Error::Validation(_)));
        assert!(error.to_string().contains("applicability.observation"));
    }

    #[test]
    fn transfer_only_generation_skips_lut_size_validation() {
        let toml = r#"
            [transfers.linear]
            input_unit = "code"
            output_unit = "unit"
            output_scale = 1
            max_interpolation_error = 1
            points = [
              { input = 0, output = 0.0 },
              { input = 10, output = 10.0 },
            ]
        "#;
        let opts = GenerateOptions {
            value_type: ValueType::U8,
            lut_size: 1,
        };
        let out = generate_from_str(toml, &opts).unwrap();
        assert!(out.contains("PiecewiseLinearTransfer"));
        assert!(!out.contains("CurveLut"));
    }

    #[test]
    fn curves_still_require_full_lut_domain() {
        let toml = r#"
            [curves.linear]
            builtin = "linear"
        "#;
        let opts = GenerateOptions {
            value_type: ValueType::U8,
            lut_size: 1,
        };
        let error = generate_from_str(toml, &opts).unwrap_err();
        assert!(error.to_string().contains("--lut-size must be 256"));
    }
}
