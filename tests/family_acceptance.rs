//! Runtime acceptance for the device-neutral transfer-family fixture.
//!
//! Exhaustive conversion is checked against an independent quadratic oracle,
//! not against the generator's fitted table.

#![allow(dead_code)]

use ph_curves::{Curve, TransferError, TransferFunction};

#[path = "support/family_oracle.rs"]
mod support;

include!("fixtures/family_acceptance_generated.rs");

fn assert_matches_oracle<const N: usize>(
    transfer: &ph_curves::PiecewiseLinearTransfer<N>,
    metadata: &ph_curves::TransferMetadata,
    numerator: u32,
    denominator: u32,
) {
    for code in metadata.domain_min..=metadata.domain_max {
        let converted = transfer.convert(code).expect("in-domain code converts");
        let expected = support::oracle_scaled(code, numerator, denominator);
        let error = (f64::from(converted) - expected).abs();
        assert!(
            error <= f64::from(metadata.achieved_max_error),
            "code {code}: convert={converted} oracle={expected} error={error} bound={}",
            metadata.achieved_max_error
        );
    }
}

#[test]
fn mixed_document_emits_an_unrelated_normalized_curve() {
    assert_eq!(LINEAR.eval(0), 0);
    assert_eq!(LINEAR.eval(128), 128);
    assert_eq!(LINEAR.eval(255), 255);
}

#[test]
fn exhaustive_independent_oracle_bounds_every_emitted_member() {
    assert_matches_oracle(
        &FRONT_END_LOW_DC,
        &FRONT_END_LOW_DC_METADATA,
        support::LOW_TRANSFORM.0,
        support::LOW_TRANSFORM.1,
    );
    assert_matches_oracle(
        &FRONT_END_MID_DC,
        &FRONT_END_MID_DC_METADATA,
        support::MID_TRANSFORM.0,
        support::MID_TRANSFORM.1,
    );
    assert_matches_oracle(
        &FRONT_END_HIGH_DC,
        &FRONT_END_HIGH_DC_METADATA,
        support::HIGH_TRANSFORM.0,
        support::HIGH_TRANSFORM.1,
    );
}

#[test]
fn standalone_and_family_observation_guards_are_identical() {
    assert_eq!(
        GUARDED_IDENTITY.convert(65_535),
        Err(TransferError::RejectedObservation { input: 65_535 })
    );
    assert_eq!(
        FRONT_END_LOW_DC.convert(65_535),
        Err(TransferError::RejectedObservation { input: 65_535 })
    );
    assert_eq!(
        FRONT_END_MID_DC.convert(65_535),
        Err(TransferError::RejectedObservation { input: 65_535 })
    );
    assert_eq!(
        FRONT_END_HIGH_DC.convert(65_535),
        Err(TransferError::RejectedObservation { input: 65_535 })
    );

    assert_eq!(
        GUARDED_IDENTITY.observation_guard(),
        FRONT_END_LOW_DC.observation_guard()
    );
    assert_eq!(
        GUARDED_IDENTITY_OBSERVATION_GUARD,
        FRONT_END_LOW_DC_OBSERVATION_GUARD
    );
}

#[test]
fn ordinary_boundaries_remain_independent_of_the_guard() {
    assert_eq!(
        GUARDED_IDENTITY.convert(0),
        Err(TransferError::BelowDomain {
            input: 0,
            minimum: 1
        })
    );
    assert_eq!(
        GUARDED_IDENTITY.convert(11),
        Ok(GUARDED_IDENTITY.convert(10).unwrap())
    );

    assert_eq!(
        FRONT_END_LOW_DC.convert(99),
        Err(TransferError::BelowDomain {
            input: 99,
            minimum: 100
        })
    );
    assert_eq!(
        FRONT_END_LOW_DC.convert(501),
        Ok(FRONT_END_LOW_DC.convert(500).unwrap())
    );
}

#[test]
fn companion_guard_metadata_matches_runtime_getters() {
    for (runtime, emitted) in [
        (
            GUARDED_IDENTITY.observation_guard(),
            GUARDED_IDENTITY_OBSERVATION_GUARD,
        ),
        (
            FRONT_END_LOW_DC.observation_guard(),
            FRONT_END_LOW_DC_OBSERVATION_GUARD,
        ),
        (
            FRONT_END_MID_DC.observation_guard(),
            FRONT_END_MID_DC_OBSERVATION_GUARD,
        ),
        (
            FRONT_END_HIGH_DC.observation_guard(),
            FRONT_END_HIGH_DC_OBSERVATION_GUARD,
        ),
    ] {
        let runtime = runtime.expect("fixture members are guarded");
        let emitted = emitted.expect("companion is Some");
        assert_eq!(emitted.code, runtime.code);
        assert_eq!(emitted.behavior, runtime.behavior);
    }
}

#[test]
fn family_members_are_piecewise_linear_transfers() {
    fn is_transfer<const N: usize>(_: &ph_curves::PiecewiseLinearTransfer<N>) {}
    is_transfer(&FRONT_END_LOW_DC);
    is_transfer(&FRONT_END_MID_DC);
    is_transfer(&FRONT_END_HIGH_DC);
    is_transfer(&GUARDED_IDENTITY);
}
