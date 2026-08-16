use ph_curves::{InverseTransferError, TransferError, TransferFunction};

include!("fixtures/observation_guards_generated.rs");

#[test]
fn generated_standalone_guard_precedes_ordinary_above_clamp() {
    assert_eq!(
        GUARDED_ERROR.convert(65_535),
        Err(TransferError::RejectedObservation { input: 65_535 })
    );
    assert_eq!(GUARDED_ERROR.convert(65_534), Ok(10));
    assert_eq!(GUARDED_ERROR.convert(10), Ok(10));
}

#[test]
fn generated_family_guard_clamps_to_domain_max_before_above_error() {
    // This transfer decreases, so the output at domain_max is the physical
    // minimum. Guard clamp is defined by the observation endpoint, not by the
    // physical maximum.
    assert_eq!(GUARDED_FAMILY_VARIANT_CLAMP.convert(65_535), Ok(1));
    assert_eq!(
        GUARDED_FAMILY_VARIANT_CLAMP.convert(65_534),
        Err(TransferError::AboveDomain {
            input: 65_534,
            maximum: 10,
        })
    );
    assert_eq!(GUARDED_FAMILY_VARIANT_CLAMP.convert(10), Ok(1));
}

#[test]
fn generated_guard_metadata_matches_runtime_getters() {
    let standalone_runtime = GUARDED_ERROR.observation_guard().unwrap();
    let standalone_emitted = GUARDED_ERROR_OBSERVATION_GUARD.unwrap();
    assert_eq!(standalone_emitted.code, standalone_runtime.code);
    assert_eq!(standalone_emitted.behavior, standalone_runtime.behavior);

    let family_runtime = GUARDED_FAMILY_VARIANT_CLAMP.observation_guard().unwrap();
    let family_emitted = GUARDED_FAMILY_VARIANT_CLAMP_OBSERVATION_GUARD.unwrap();
    assert_eq!(family_emitted.code, family_runtime.code);
    assert_eq!(family_emitted.behavior, family_runtime.behavior);
}

#[test]
fn generated_guards_do_not_change_inverse_policy() {
    assert_eq!(GUARDED_ERROR.invert_physical(7), Ok(7));
    assert_eq!(GUARDED_ERROR.invert_physical(11), Ok(10));

    assert_eq!(GUARDED_FAMILY_VARIANT_CLAMP.invert_physical(5), Ok(6));
    assert_eq!(
        GUARDED_FAMILY_VARIANT_CLAMP.invert_physical(0),
        Err(InverseTransferError::BelowRange {
            physical: 0,
            minimum: 1,
        })
    );
    assert_eq!(GUARDED_FAMILY_VARIANT_CLAMP.invert_physical(11), Ok(1));
}
