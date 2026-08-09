//! CLI tool for generating static curve LUTs from a TOML definition file.
//!
//! # Usage
//!
//! ```sh
//! ph-curves-gen --input curves.toml --output curves.rs
//! ph-curves-gen --input curves.toml --value-type u16 --lut-size 65536
//! ```
//!
//! The generated `.rs` file contains `static` arrays and `const` curve values
//! ready to be included in a `no_std` crate via `include!` or copied directly.
//!
//! # Curve definition formats
//!
//! Each curve in the TOML file can be defined in one of three ways.
//! Set exactly **one** of `builtin`, `formula`, or `points`.
//!
//! ## 1. Builtin name
//!
//! ```toml
//! [curves.my_curve]
//! builtin = "ease_in_quad"
//! ```
//!
//! Available builtins (all are monotonic):
//!
//! | Name                 | Formula                 | Description                      |
//! |----------------------|-------------------------|----------------------------------|
//! | `linear`             | `t`                     | Identity / straight line         |
//! | `ease_in_quad`       | `t²`                    | Quadratic ease-in                |
//! | `ease_out_quad`      | `1-(1-t)²`              | Quadratic ease-out               |
//! | `ease_in_out_quad`   | piecewise quadratic     | Quadratic ease-in-out            |
//! | `ease_in_cubic`      | `t³`                    | Cubic ease-in                    |
//! | `ease_out_cubic`     | `1-(1-t)³`              | Cubic ease-out                   |
//! | `ease_in_out_cubic`  | piecewise cubic         | Cubic ease-in-out                |
//! | `ease_in_quart`      | `t⁴`                    | Quartic ease-in                  |
//! | `ease_out_quart`     | `1-(1-t)⁴`              | Quartic ease-out                 |
//! | `ease_in_out_quart`  | piecewise quartic       | Quartic ease-in-out              |
//! | `ease_in_expo`       | `2^(10(t-1))`           | Exponential ease-in              |
//! | `ease_out_expo`      | `1-2^(-10t)`            | Exponential ease-out             |
//! | `smoothstep`         | `3t²-2t³`               | Hermite smoothstep               |
//! | `smoother_step`      | `6t⁵-15t⁴+10t³`        | Ken Perlin's improved smoothstep |
//!
//! Legacy aliases: `ease_in` = `ease_in_quad`, `ease_out` = `ease_out_quad`,
//! `ease_in_out` = `ease_in_out_quad`.
//!
//! ## 2. Formula (math expression over `t` in 0..1)
//!
//! ```toml
//! [curves.gamma_22]
//! formula = "pow(t, 2.2)"
//! ```
//!
//! The variable `t` ranges from 0.0 to 1.0. The expression must evaluate to
//! a value in 0.0..=1.0. Supported operators: `+`, `-`, `*`, `/`, `^` (or
//! `**`), unary `-`, and parentheses.
//!
//! Functions: `pow(x,y)`, `sqrt(x)`, `abs(x)`, `min(x,y)`, `max(x,y)`,
//! `clamp(x,lo,hi)`, `sin(x)`, `cos(x)`, `tan(x)`, `exp(x)`, `ln(x)`,
//! `log2(x)`.
//!
//! Constants: `pi`, `e`.
//!
//! ## 3. Points (piecewise-linear control points)
//!
//! ```toml
//! [curves.custom]
//! monotonic = false
//! points = [[0, 0], [64, 200], [192, 50], [255, 255]]
//! ```
//!
//! Point coordinates are in the LUT's index range (0..lut_size-1).
//! The first point must start at u=0 and the last must end at u=lut_size-1.
//! Curve names are normalized to uppercase Rust identifiers. Names that contain
//! no ASCII letters or digits, and names that normalize to the same identifier,
//! are rejected.
//!
//! ## Common options
//!
//! - `monotonic` (bool, default `true`): when `true` an inverse LUT is
//!   generated and the curve is emitted as a `MonotonicCurveLut`.

mod builtin;
mod codegen;
mod curve;
mod formula;
mod points;

use std::fs;
use std::path::PathBuf;

use clap::Parser;

/// Generate static Rust curve LUTs from a TOML definition file.
#[derive(Parser, Debug)]
#[command(name = "ph-curves-gen", version, about, long_about = None)]
struct Cli {
    /// Path to the input TOML file.
    #[arg(short, long)]
    input: PathBuf,

    /// Path to the output `.rs` file. If omitted, prints to stdout.
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Rust type used for the LUT index and value (`u8` or `u16`).
    #[arg(long, default_value = "u8")]
    value_type: String,

    /// Number of entries in each LUT (256 for u8; 65536 for u16).
    #[arg(long, default_value_t = 256)]
    lut_size: usize,
}

fn main() {
    let cli = Cli::parse();

    if let Err(error) = validate_lut_size(&cli.value_type, cli.lut_size) {
        eprintln!("{error}");
        std::process::exit(1);
    }

    let toml_str = fs::read_to_string(&cli.input)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", cli.input.display()));

    let curves_file: curve::CurvesFile =
        toml::from_str(&toml_str).unwrap_or_else(|e| panic!("invalid TOML: {e}"));

    let output =
        codegen::generate(&curves_file, &cli.value_type, cli.lut_size).unwrap_or_else(|error| {
            eprintln!("{error}");
            std::process::exit(1);
        });

    match cli.output {
        Some(path) => {
            fs::write(&path, &output)
                .unwrap_or_else(|e| panic!("failed to write {}: {e}", path.display()));
            eprintln!("wrote {}", path.display());
        }
        None => print!("{output}"),
    }
}

fn validate_lut_size(value_type: &str, lut_size: usize) -> Result<(), String> {
    let required_lut_size = match value_type {
        "u8" => 256,
        "u16" => 65_536,
        other => {
            return Err(format!(
                "unsupported --value-type `{other}` (expected `u8` or `u16`)"
            ));
        }
    };
    if lut_size != required_lut_size {
        return Err(format!(
            "--lut-size must be {required_lut_size} for --value-type {value_type} \
             (the LUT must cover the full {value_type} domain)"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate_lut_size;

    #[test]
    fn lut_size_must_cover_full_u8_domain() {
        assert!(validate_lut_size("u8", 256).is_ok());
        assert_eq!(
            validate_lut_size("u8", 255).unwrap_err(),
            "--lut-size must be 256 for --value-type u8 \
             (the LUT must cover the full u8 domain)"
        );
    }

    #[test]
    fn lut_size_must_cover_full_u16_domain() {
        assert!(validate_lut_size("u16", 65_536).is_ok());
        assert_eq!(
            validate_lut_size("u16", 256).unwrap_err(),
            "--lut-size must be 65536 for --value-type u16 \
             (the LUT must cover the full u16 domain)"
        );
    }
}
