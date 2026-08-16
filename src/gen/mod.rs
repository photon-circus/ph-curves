//! Host-side TOML → Rust code generation for curves and transfers.
//!
//! Enable with `features = ["gen-lib"]` from a `build.rs` or host tool.
//! Firmware crates should keep the default feature set and `include!`
//! generated source without enabling this module.
//!
//! Host tools that own device evaluation inspect the validated transfer graph
//! (`DefinitionsFile::validate`) and may overlay [`TransferSource`] values.
//! That is a public IR, not a plugin ABI: ph-curves does not load or call
//! device-specific model code.
//!
//! This module is the crate's only `std` consumer. The crate root is
//! unconditionally `#![no_std]` and does not `extern crate std`. Each file
//! under `src/gen` links `std` with a module-local `extern crate std`, imports
//! the prelude by hand, and pulls in `format!` / `vec!` explicitly (those
//! macros are not available without a crate-root `#[macro_use]`). Enabling
//! `gen-lib` therefore cannot add `std` or an allocator to the runtime API.
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

// Host-only: crate root stays `#![no_std]` with no crate-wide `std` link.
extern crate std;

mod api;
pub(crate) mod builtin;
pub(crate) mod codegen;
pub(crate) mod curve;
pub(crate) mod formula;
mod ir;
pub(crate) mod points;
pub(crate) mod transfer;

pub use api::{
    Error, GenerateOptions, ValueType, generate, generate_from_str, generate_from_toml,
    generate_to_path,
};
pub use curve::{CurveDef, DefinitionsFile};
pub use ir::{ValidatedDefinitions, ValidatedFamily, ValidatedMember};
pub use transfer::{
    ApplicabilityDef, BoundaryDef, DeclaredSource, EvaluatedTruth, FamilyMemberDef, GapDef,
    GapStatus, InputTransform, MemberStatus, ObservationGuardBehaviorDef, ObservationGuardDef,
    PhysicalPoint, SelectorValue, TransferDef, TransferFamilyDef, TransferSource, TransferSpec,
};
