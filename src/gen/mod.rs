//! Host-side TOML → Rust code generation for curves and transfers.
//!
//! Enable with `features = ["gen"]` from a `build.rs` or host tool. Firmware
//! crates should keep the default feature set and `include!` generated source
//! without enabling this module.
//!
//! # `build.rs` example
//!
//! ```ignore
//! use std::env;
//! use std::path::PathBuf;
//! use ph_curves::r#gen::{generate_to_path, GenerateOptions};
//!
//! fn main() {
//!     let out = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("curves.rs");
//!     generate_to_path("assets/curves.toml", &out, &GenerateOptions::default())
//!         .expect("ph-curves gen");
//!     println!("cargo:rerun-if-changed=assets/curves.toml");
//! }
//! ```

#![allow(clippy::std_instead_of_core)]
#![allow(clippy::std_instead_of_alloc)]

mod api;
pub(crate) mod builtin;
pub(crate) mod codegen;
pub(crate) mod curve;
pub(crate) mod formula;
pub(crate) mod points;
pub(crate) mod transfer;

pub use api::{
    Error, GenerateOptions, ValueType, generate, generate_from_str, generate_from_toml,
    generate_to_path,
};
pub use curve::DefinitionsFile;
