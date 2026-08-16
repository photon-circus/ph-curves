//! Compile representative generated fixtures as `no_std` firmware artifacts.
//!
//! CI and `scripts/local-ci.ps1` build this example on the repository's
//! no-std and core-only target matrix so generated tables cannot silently
//! pick up `std`, `alloc`, or floating-point types.

#![no_std]
#![allow(dead_code)]

/// Mixed family-acceptance fixture: curve LUT plus sparse transfers.
pub mod family_acceptance {
    include!("../tests/fixtures/family_acceptance_generated.rs");
}

/// Reference NTC transfer fixture.
pub mod ntc {
    include!("../tests/fixtures/ntc_generated.rs");
}

/// Standalone and family observation-guard fixture.
pub mod observation_guards {
    include!("../tests/fixtures/observation_guards_generated.rs");
}
