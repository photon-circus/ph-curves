use ph_curves::{TransferError, TransferFunction};

include!("fixtures/ntc_generated.rs");

#[test]
fn generated_ntc_reference_vectors() {
    assert_eq!(NTC_10K_BETA_3950.convert(142), Ok(124_957));
    assert_eq!(NTC_10K_BETA_3950.convert(3995), Ok(-39_919));

    let room_temperature = NTC_10K_BETA_3950.convert(2048).unwrap();
    assert!((24_950..=25_050).contains(&room_temperature));
}

#[test]
fn generated_ntc_metadata_and_boundaries() {
    assert_eq!(NTC_10K_BETA_3950_METADATA.domain_min, 142);
    assert_eq!(NTC_10K_BETA_3950_METADATA.domain_max, 3995);
    assert_eq!(NTC_10K_BETA_3950_METADATA.knot_count, 61);
    assert_eq!(NTC_10K_BETA_3950_METADATA.achieved_max_error, 47);
    assert_eq!(
        NTC_10K_BETA_3950_METADATA.direction,
        ph_curves::MonotonicDirection::Decreasing
    );
    assert_eq!(
        NTC_10K_BETA_3950.convert(141),
        Err(TransferError::BelowDomain {
            input: 141,
            minimum: 142
        })
    );
    assert_eq!(
        NTC_10K_BETA_3950.convert(3996),
        Err(TransferError::AboveDomain {
            input: 3996,
            maximum: 3995
        })
    );
}
