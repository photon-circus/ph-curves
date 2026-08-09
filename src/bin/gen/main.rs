//! CLI front-end for [`ph_curves::r#gen`].
//!
//! # Usage
//!
//! ```sh
//! ph-curves-gen --input curves.toml --output curves.rs
//! ph-curves-gen --input curves.toml --value-type u16 --lut-size 65536
//! ```
//!
//! Generation logic lives in the library behind `features = ["gen"]`. This
//! binary only parses CLI flags and writes the result. See `ph_curves::r#gen`
//! module docs for TOML schema details and `build.rs` usage.

use std::fs;
use std::path::PathBuf;
use std::process;

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

fn main() {
    let cli = Cli::parse();

    let value_type = match ValueType::parse(&cli.value_type) {
        Ok(value_type) => value_type,
        Err(_) => {
            eprintln!(
                "unsupported --value-type `{}` (expected `u8` or `u16`)",
                cli.value_type
            );
            process::exit(1);
        }
    };

    let opts = GenerateOptions {
        value_type,
        lut_size: cli.lut_size,
    };

    let output = match generate_from_toml(&cli.input, &opts) {
        Ok(output) => output,
        Err(Error::Io(error)) => {
            panic!("failed to read {}: {error}", cli.input.display());
        }
        Err(Error::Toml(error)) => {
            panic!("invalid TOML: {error}");
        }
        Err(Error::Validation(message)) => {
            eprintln!("{message}");
            process::exit(1);
        }
    };

    match cli.output {
        Some(path) => {
            fs::write(&path, &output)
                .unwrap_or_else(|e| panic!("failed to write {}: {e}", path.display()));
            eprintln!("wrote {}", path.display());
        }
        None => print!("{output}"),
    }
}
