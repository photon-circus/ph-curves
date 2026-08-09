use ph_curves::{
    MedianFilter, Stability, StabilityDetector, TemporalFilter, TransferError, TransferFunction,
};

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
    assert_eq!(NTC_10K_BETA_3950_METADATA.achieved_max_error, 49);
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

#[test]
fn raw_median_and_physical_stability_compose_without_driver_state() {
    let mut median = MedianFilter::<u16, 5>::new();
    let mut detector = StabilityDetector::<i32, 3>::new(100);
    let mut last = None;

    for code in [2048, 2049, 4095, 2047, 2048, 2048, 2049] {
        let Some(filtered_code) = median.update(code).ready() else {
            continue;
        };
        let measurement = NTC_10K_BETA_3950.convert(filtered_code).unwrap();
        last = Some(detector.update(measurement));
    }

    assert!(matches!(last, Some(Stability::Stable { .. })));
}
