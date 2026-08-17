use ph_curves::{
    AffineOverflow, AffineTransform, Hysteresis, MedianFilter, MovingAverage, Stability,
    StabilityDetector, TemporalFilter, TemporalSample,
};

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum PipelineState<T> {
    FilterWarming,
    StabilityWarming {
        filtered: T,
    },
    Unstable {
        filtered: T,
        minimum: T,
        maximum: T,
        span: u64,
    },
    Stable {
        filtered: T,
        minimum: T,
        maximum: T,
        span: u64,
        decision: bool,
    },
}

fn classify<T: TemporalSample, const N: usize>(
    detector: &mut StabilityDetector<T, N>,
    latch: &mut Hysteresis<T>,
    filtered: T,
) -> PipelineState<T> {
    match detector.update(filtered) {
        Stability::WarmingUp { .. } => PipelineState::StabilityWarming { filtered },
        Stability::Unstable {
            minimum,
            maximum,
            span,
        } => PipelineState::Unstable {
            filtered,
            minimum,
            maximum,
            span,
        },
        Stability::Stable {
            minimum,
            maximum,
            span,
        } => PipelineState::Stable {
            filtered,
            minimum,
            maximum,
            span,
            decision: latch.update(filtered),
        },
    }
}

fn update_unsigned_pipeline(
    average: &mut MovingAverage<u32, 2>,
    detector: &mut StabilityDetector<u32, 2>,
    latch: &mut Hysteresis<u32>,
    sample: u32,
) -> PipelineState<u32> {
    let Some(filtered) = average.update(sample).ready() else {
        return PipelineState::FilterWarming;
    };
    classify(detector, latch, filtered)
}

fn update_calibrated_pipeline(
    transform: &AffineTransform,
    median: &mut MedianFilter<i32, 3>,
    detector: &mut StabilityDetector<i32, 2>,
    latch: &mut Hysteresis<i32>,
    sample: i32,
) -> Result<PipelineState<i32>, AffineOverflow> {
    // Apply before touching temporal state: a failed calibration consumes no
    // sample and leaves both windows and the application latch unchanged.
    let corrected = transform.apply(sample)?;
    let Some(filtered) = median.update(corrected).ready() else {
        return Ok(PipelineState::FilterWarming);
    };
    Ok(classify(detector, latch, filtered))
}

#[test]
fn already_converted_u32_pipeline_keeps_each_stage_explicit() {
    let mut average = MovingAverage::<u32, 2>::new();
    let mut detector = StabilityDetector::<u32, 2>::new(0);
    let mut latch = Hysteresis::<u32>::new(90, 110);

    let samples = [80, 80, 80, 120, 120, 120, 100, 100, 100, 80, 80, 80];
    let expected = [
        PipelineState::FilterWarming,
        PipelineState::StabilityWarming { filtered: 80 },
        PipelineState::Stable {
            filtered: 80,
            minimum: 80,
            maximum: 80,
            span: 0,
            decision: false,
        },
        PipelineState::Unstable {
            filtered: 100,
            minimum: 80,
            maximum: 100,
            span: 20,
        },
        PipelineState::Unstable {
            filtered: 120,
            minimum: 100,
            maximum: 120,
            span: 20,
        },
        PipelineState::Stable {
            filtered: 120,
            minimum: 120,
            maximum: 120,
            span: 0,
            decision: true,
        },
        PipelineState::Unstable {
            filtered: 110,
            minimum: 110,
            maximum: 120,
            span: 10,
        },
        PipelineState::Unstable {
            filtered: 100,
            minimum: 100,
            maximum: 110,
            span: 10,
        },
        PipelineState::Stable {
            filtered: 100,
            minimum: 100,
            maximum: 100,
            span: 0,
            decision: true,
        },
        PipelineState::Unstable {
            filtered: 90,
            minimum: 90,
            maximum: 100,
            span: 10,
        },
        PipelineState::Unstable {
            filtered: 80,
            minimum: 80,
            maximum: 90,
            span: 10,
        },
        PipelineState::Stable {
            filtered: 80,
            minimum: 80,
            maximum: 80,
            span: 0,
            decision: false,
        },
    ];

    for (sample, expected_state) in samples.into_iter().zip(expected) {
        assert_eq!(
            update_unsigned_pipeline(&mut average, &mut detector, &mut latch, sample),
            expected_state
        );
    }

    average.reset();
    detector.reset();
    latch.reset();
    assert!(average.is_empty());
    assert!(detector.is_empty());
    assert!(!latch.state());
}

#[test]
fn calibrated_i32_pipeline_preserves_policy_and_error_boundaries() {
    let overflow = AffineTransform::new(i32::MAX, 0, 1).unwrap();
    let mut median = MedianFilter::<i32, 3>::new();
    let mut detector = StabilityDetector::<i32, 2>::new(0);
    let mut latch = Hysteresis::<i32>::new(55_000, 60_000);

    assert_eq!(
        update_calibrated_pipeline(&overflow, &mut median, &mut detector, &mut latch, i32::MAX,),
        Err(AffineOverflow::Overflow)
    );
    assert!(median.is_empty());
    assert!(detector.is_empty());
    assert!(!latch.state());

    let transform = AffineTransform::new(1, -1_000, 1).unwrap();
    let samples = [
        55_000, 55_000, 55_000, 55_000, 61_000, 61_000, 61_000, 58_500, 58_500, 58_500, 56_000,
        56_000, 56_000,
    ];
    let expected = [
        PipelineState::FilterWarming,
        PipelineState::FilterWarming,
        PipelineState::StabilityWarming { filtered: 54_000 },
        PipelineState::Stable {
            filtered: 54_000,
            minimum: 54_000,
            maximum: 54_000,
            span: 0,
            decision: false,
        },
        PipelineState::Stable {
            filtered: 54_000,
            minimum: 54_000,
            maximum: 54_000,
            span: 0,
            decision: false,
        },
        PipelineState::Unstable {
            filtered: 60_000,
            minimum: 54_000,
            maximum: 60_000,
            span: 6_000,
        },
        PipelineState::Stable {
            filtered: 60_000,
            minimum: 60_000,
            maximum: 60_000,
            span: 0,
            decision: true,
        },
        PipelineState::Stable {
            filtered: 60_000,
            minimum: 60_000,
            maximum: 60_000,
            span: 0,
            decision: true,
        },
        PipelineState::Unstable {
            filtered: 57_500,
            minimum: 57_500,
            maximum: 60_000,
            span: 2_500,
        },
        PipelineState::Stable {
            filtered: 57_500,
            minimum: 57_500,
            maximum: 57_500,
            span: 0,
            decision: true,
        },
        PipelineState::Stable {
            filtered: 57_500,
            minimum: 57_500,
            maximum: 57_500,
            span: 0,
            decision: true,
        },
        PipelineState::Unstable {
            filtered: 55_000,
            minimum: 55_000,
            maximum: 57_500,
            span: 2_500,
        },
        PipelineState::Stable {
            filtered: 55_000,
            minimum: 55_000,
            maximum: 55_000,
            span: 0,
            decision: false,
        },
    ];

    for (sample, expected_state) in samples.into_iter().zip(expected) {
        assert_eq!(
            update_calibrated_pipeline(&transform, &mut median, &mut detector, &mut latch, sample,),
            Ok(expected_state)
        );
    }
}
