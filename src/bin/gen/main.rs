//! CLI front-end for the `ph_curves::r#gen` module.
//!
//! Not an intra-doc link: raw identifiers such as `r#gen` do not resolve in
//! doc links, and rustdoc parses `ph_curves::r` out of the attempt.
//!
//! # Usage
//!
//! ```sh
//! ph-curves-gen --input curves.toml --output curves.rs
//! ph-curves-gen --input curves.toml --value-type u16 --lut-size 65536
//! ```
//!
//! Generation logic lives in the library behind `features = ["gen-cli"]`
//! (or the `gen` compatibility alias). This binary only parses CLI flags and
//! writes the result. Prefer `gen-lib` from `build.rs`. See `ph_curves::r#gen`
//! module docs for TOML schema details and `build.rs` usage.

use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use ph_curves::r#gen::{Error, GenerateOptions, ValueType, generate_from_toml};

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

fn main() -> ExitCode {
    let cli = Cli::parse();

    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<(), String> {
    let value_type = match ValueType::parse(&cli.value_type) {
        Ok(value_type) => value_type,
        Err(_) => {
            return Err(format!(
                "unsupported --value-type `{}` (expected `u8` or `u16`)",
                cli.value_type
            ));
        }
    };

    let opts = GenerateOptions {
        value_type,
        lut_size: cli.lut_size,
    };

    let output = match generate_from_toml(&cli.input, &opts) {
        Ok(output) => output,
        Err(Error::Io(error)) => {
            return Err(format!("failed to read {}: {error}", cli.input.display()));
        }
        Err(Error::Toml(error)) => {
            return Err(format!("invalid TOML: {error}"));
        }
        Err(Error::Validation(message)) => {
            return Err(message);
        }
    };

    match cli.output {
        Some(path) => {
            fs::write(&path, &output)
                .map_err(|error| format!("failed to write {}: {error}", path.display()))?;
            eprintln!("wrote {}", path.display());
        }
        None => print!("{output}"),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_input_is_a_clean_error_instead_of_a_panic() {
        let missing = std::env::temp_dir().join(format!(
            "ph-curves-definitely-missing-{}-{}.toml",
            std::process::id(),
            line!()
        ));
        let error = run(Cli {
            input: missing.clone(),
            output: None,
            value_type: "u8".into(),
            lut_size: 256,
        })
        .unwrap_err();
        assert!(error.contains("failed to read"), "{error}");
        assert!(error.contains(&missing.display().to_string()), "{error}");
    }

    #[test]
    fn invalid_value_type_is_a_clean_error_instead_of_exiting() {
        let error = run(Cli {
            input: PathBuf::from("unused.toml"),
            output: None,
            value_type: "u32".into(),
            lut_size: 256,
        })
        .unwrap_err();
        assert_eq!(
            error,
            "unsupported --value-type `u32` (expected `u8` or `u16`)"
        );
    }

    #[test]
    fn invalid_toml_is_a_clean_error_instead_of_a_panic() {
        let path = std::env::temp_dir().join(format!(
            "ph-curves-invalid-toml-{}-{}.toml",
            std::process::id(),
            line!()
        ));
        fs::write(&path, "[unterminated").unwrap();
        let result = run(Cli {
            input: path.clone(),
            output: None,
            value_type: "u8".into(),
            lut_size: 256,
        });
        fs::remove_file(path).unwrap();

        let error = result.unwrap_err();
        assert!(error.starts_with("invalid TOML:"), "{error}");
        assert!(!error.contains("panicked"), "{error}");
    }
}
