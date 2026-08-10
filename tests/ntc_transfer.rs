use ph_curves::{
    AffineCalibration, InverseTransferError, InverseTransferFunction, MedianFilter, Stability,
    StabilityDetector, TemporalFilter, TransferError, TransferFunction,
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
fn generated_ntc_inverse_setpoint_and_round_trip() {
    assert_eq!(NTC_10K_BETA_3950_METADATA.range_min, -39_919);
    assert_eq!(NTC_10K_BETA_3950_METADATA.range_max, 124_957);
    assert_eq!(NTC_10K_BETA_3950_METADATA.flat_segment_count, 0);
    assert_eq!(
        NTC_10K_BETA_3950_METADATA.strictly_monotonic,
        NTC_10K_BETA_3950_METADATA.flat_segment_count == 0
    );

    let room = NTC_10K_BETA_3950.invert(25_000).unwrap();
    assert!((2040..=2056).contains(&room));
    assert_eq!(NTC_10K_BETA_3950.invert_physical(124_957), Ok(142));
    assert_eq!(NTC_10K_BETA_3950.invert_physical(-39_919), Ok(3995));

    let mut worst = 0u16;
    for code in NTC_10K_BETA_3950_METADATA.domain_min..=NTC_10K_BETA_3950_METADATA.domain_max {
        let physical = NTC_10K_BETA_3950.convert(code).unwrap();
        let recovered = NTC_10K_BETA_3950.invert(physical).unwrap();
        let distance = u16::try_from(i32::from(recovered).abs_diff(i32::from(code))).unwrap();
        worst = worst.max(distance);
    }
    assert_eq!(
        worst, NTC_10K_BETA_3950_METADATA.achieved_max_inverse_code_error,
        "update fixture achieved_max_inverse_code_error if the table changes"
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

/// A calibrated setpoint on the real NTC table: "which ADC code reads 25 °C
/// after this unit's factory trim?" — one call, no manual affine arithmetic.
#[test]
fn calibrated_setpoint_on_the_reference_ntc() {
    // +0.5 % gain, -120 milli-Celsius offset, as a factory trim would supply.
    let trimmed = AffineCalibration::new(NTC_10K_BETA_3950, 1_005, -120, 1_000).unwrap();

    let code = trimmed.invert(25_000).expect("25 C is inside the range");
    let reading = trimmed.convert(code).expect("code is inside the domain");
    assert!(
        (reading - 25_000).abs() <= 60,
        "calibrated round trip drifted: code {code} reads {reading} mC"
    );

    // The uncalibrated table disagrees, which is the whole point of the wrapper.
    let raw_code = NTC_10K_BETA_3950.invert(25_000).unwrap();
    assert_ne!(code, raw_code);
}

#[test]
fn calibrated_inverse_reports_the_decreasing_range_in_calibrated_units() {
    let trimmed = AffineCalibration::new(NTC_10K_BETA_3950, 2, 0, 1).unwrap();

    // Uncalibrated range is -39_919..=124_957; doubled it is -79_838..=249_914.
    assert_eq!(trimmed.invert(249_914), Ok(142));
    assert_eq!(trimmed.invert(-79_838), Ok(3995));

    assert_eq!(
        trimmed.invert(249_915),
        Err(InverseTransferError::AboveRange {
            physical: 249_915,
            maximum: 249_914
        })
    );
}
