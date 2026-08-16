//! Host acceptance for the device-neutral transfer-family fixture.

#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};

use ph_curves::r#gen::{
    ApplicabilityDef, BoundaryDef, DefinitionsFile, FamilyCompleteness, FamilyGapDef,
    FamilyMemberDef, FamilySource, FamilySpec, GapStatus, GenerateOptions, GenerationPath,
    InputTransform, MemberStatus, ObservationGuardBehaviorDef, ObservationGuardDef, SelectorValue,
    SourceProvenance, SourceProvenanceOverride, TABLE_BYTES_PER_KNOT, TransferSource,
    generate_from_toml_report,
};
use ph_curves::interpolate_segment;

#[path = "support/family_oracle.rs"]
mod support;

include!("fixtures/family_acceptance_generated.rs");

const ASSET: &str = include_str!("../assets/family-acceptance.toml");

fn sel(value: &str) -> SelectorValue {
    SelectorValue::String(value.into())
}

fn identity(range: &str, coupling: &str) -> BTreeMap<String, SelectorValue> {
    BTreeMap::from([
        ("coupling".into(), sel(coupling)),
        ("range".into(), sel(range)),
    ])
}

fn mapped_member(
    range: &str,
    coupling: &str,
    status: MemberStatus,
    reason: Option<&str>,
    transform: (u32, u32),
    model_input: [f64; 2],
    emitted_name: Option<&str>,
) -> FamilyMemberDef {
    FamilyMemberDef {
        selectors: identity(range, coupling),
        input_transform: Some(InputTransform {
            numerator: transform.0,
            denominator: transform.1,
        }),
        status,
        reason: reason.map(str::to_string),
        applicability: ApplicabilityDef {
            observation: None,
            model_input: Some(model_input),
            physical: None,
        },
        provenance: None,
        emitted_name: emitted_name.map(str::to_string),
    }
}

fn front_end_spec() -> FamilySpec {
    FamilySpec::new(
        "front_end",
        "adc_code",
        "millivolt",
        support::OUTPUT_SCALE,
        1,
        FamilySource::scaled_polynomial(support::COEFFICIENTS.to_vec()),
        SourceProvenance::new("synthetic multi-range ADC note").with_locator("Table 1"),
    )
    .with_max_knots(32)
    .with_max_total_knots(8)
    .with_max_table_bytes(48)
    .with_boundaries(BoundaryDef::Error, BoundaryDef::Clamp)
    .with_observation_guard(ObservationGuardDef {
        code: 65_535,
        behavior: ObservationGuardBehaviorDef::Error,
        provenance: Some(SourceProvenanceOverride {
            locator: Some("§4 saturation".into()),
            ..SourceProvenanceOverride::default()
        }),
    })
    .with_selector_axes(BTreeMap::from([
        ("range".into(), vec![sel("low"), sel("mid"), sel("high")]),
        ("coupling".into(), vec![sel("dc"), sel("ac")]),
    ]))
    .with_members(vec![
        mapped_member(
            "low",
            "dc",
            MemberStatus::Emit,
            None,
            support::LOW_TRANSFORM,
            [0.2, 1.0],
            Some("front_end_low_dc"),
        ),
        mapped_member(
            "mid",
            "dc",
            MemberStatus::Emit,
            None,
            support::MID_TRANSFORM,
            [0.8, 3.2],
            Some("front_end_mid_dc"),
        ),
        mapped_member(
            "high",
            "dc",
            MemberStatus::Emit,
            None,
            support::HIGH_TRANSFORM,
            [3.2, 8.0],
            Some("front_end_high_dc"),
        ),
        mapped_member(
            "low",
            "ac",
            MemberStatus::Forbidden,
            Some("AC coupling saturates the low-range front end"),
            support::LOW_TRANSFORM,
            [0.2, 1.0],
            None,
        ),
        FamilyMemberDef {
            selectors: identity("high", "ac"),
            input_transform: None,
            status: MemberStatus::Unsupported,
            reason: Some("high-range AC path has no source mapping".into()),
            applicability: ApplicabilityDef::default(),
            provenance: None,
            emitted_name: None,
        },
    ])
    .with_gaps(vec![FamilyGapDef {
        selectors: identity("mid", "ac"),
        status: GapStatus::Undefined,
        reason: "AC coupling on the mid range is not characterized".into(),
        provenance: Some(SourceProvenanceOverride {
            locator: Some("Table 1, omitted row".into()),
            ..SourceProvenanceOverride::default()
        }),
    }])
}

fn load_asset() -> DefinitionsFile {
    DefinitionsFile::from_toml_str(ASSET).expect("family-acceptance.toml parses")
}

fn parse_int_array<T: core::str::FromStr>(source: &str, needle: &str) -> Vec<T>
where
    T::Err: core::fmt::Debug,
{
    let start = source
        .find(needle)
        .unwrap_or_else(|| panic!("missing {needle}"));
    let after = &source[start..];
    let assignment = after.find("= [").expect("array assignment");
    let values = &after[assignment + 3..];
    let close = values.find(']').expect("array close");
    values[..close]
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(|part| part.parse().unwrap_or_else(|_| panic!("parse {part}")))
        .collect()
}

fn convert_from_knots(inputs: &[u16], outputs: &[i32], code: u16) -> i32 {
    match inputs.binary_search(&code) {
        Ok(index) => outputs[index],
        Err(index) => interpolate_segment(
            code,
            inputs[index - 1],
            outputs[index - 1],
            inputs[index],
            outputs[index],
        )
        .expect("in-segment interpolation"),
    }
}

#[test]
fn every_expected_selector_identity_is_occupied_exactly_once() {
    let validated = load_asset().validate().expect("asset validates");
    let family = &validated.families()[0];
    assert_eq!(family.name(), "front_end");
    assert_eq!(family.completeness(), FamilyCompleteness::Complete);
    assert_eq!(family.selector_universe().identity_count(), Some(6));

    let mut occupied = BTreeSet::new();
    for member in family.members() {
        assert!(
            occupied.insert(member.selectors().clone()),
            "duplicate member identity {:?}",
            member.selectors()
        );
    }
    for gap in family.gaps() {
        assert!(
            occupied.insert(gap.selectors().clone()),
            "duplicate gap identity {:?}",
            gap.selectors()
        );
    }

    let expected: BTreeSet<_> = family.selector_universe().identities().collect();
    assert_eq!(occupied, expected);
    assert_eq!(expected.len(), 6);

    assert_eq!(validated.gaps().len(), 1);
    assert!(validated.gaps().contains_key("digital_flag"));
}

#[test]
fn toml_and_programmatic_construction_converge() {
    let from_toml = load_asset().validate().expect("asset validates");
    let mut programmatic = DefinitionsFile::default();
    programmatic
        .insert_family(front_end_spec())
        .expect("FamilySpec inserts");
    let from_spec = programmatic.validate().expect("FamilySpec validates");

    let toml_family = &from_toml.families()[0];
    let spec_family = &from_spec.families()[0];
    assert_eq!(toml_family.name(), spec_family.name());
    assert_eq!(toml_family.input_unit(), spec_family.input_unit());
    assert_eq!(toml_family.output_unit(), spec_family.output_unit());
    assert_eq!(toml_family.output_scale(), spec_family.output_scale());
    assert_eq!(toml_family.source(), spec_family.source());
    assert_eq!(toml_family.declared_source(), spec_family.declared_source());
    assert_eq!(toml_family.policy(), spec_family.policy());
    assert_eq!(toml_family.provenance(), spec_family.provenance());
    assert_eq!(
        toml_family.observation_guard_provenance(),
        spec_family.observation_guard_provenance()
    );
    assert_eq!(toml_family.max_total_knots(), spec_family.max_total_knots());
    assert_eq!(toml_family.max_table_bytes(), spec_family.max_table_bytes());
    assert_eq!(toml_family.completeness(), spec_family.completeness());
    assert_eq!(toml_family.members().len(), spec_family.members().len());
    for (left, right) in toml_family.members().iter().zip(spec_family.members()) {
        assert_eq!(left.selectors(), right.selectors());
        assert_eq!(left.status(), right.status());
        assert_eq!(left.reason(), right.reason());
        assert_eq!(left.input_transform(), right.input_transform());
        assert_eq!(left.observation_domain(), right.observation_domain());
        assert_eq!(left.emitted_name(), right.emitted_name());
        assert_eq!(left.provenance(), right.provenance());
        assert_eq!(left.provenance_override(), right.provenance_override());
    }
    assert_eq!(toml_family.gaps().len(), spec_family.gaps().len());
    for (left, right) in toml_family.gaps().iter().zip(spec_family.gaps()) {
        assert_eq!(left.selectors(), right.selectors());
        assert_eq!(left.reason(), right.reason());
        assert_eq!(left.provenance(), right.provenance());
    }

    let spec_source = from_spec
        .generate(&GenerateOptions::transfers_only())
        .expect("programmatic family generates");
    assert!(spec_source.contains("PiecewiseLinearTransfer"));
    assert!(!spec_source.contains("CurveLut"));
    assert!(!spec_source.contains("f32"));
    assert!(!spec_source.contains("f64"));
    assert!(spec_source.contains("pub const FRONT_END_LOW_DC:"));
    assert!(spec_source.contains("pub const FRONT_END_MID_DC:"));
    assert!(spec_source.contains("pub const FRONT_END_HIGH_DC:"));
    assert!(spec_source.contains("static FRONT_END_LOW_DC_INPUTS: [u16; 2] = [100, 500];"));
    assert!(spec_source.contains("static FRONT_END_LOW_DC_OUTPUTS: [i32; 2] = [200, 1000];"));
    assert!(spec_source.contains("static FRONT_END_MID_DC_INPUTS: [u16; 2] = [100, 400];"));
    assert!(spec_source.contains("static FRONT_END_HIGH_DC_INPUTS: [u16; 2] = [100, 250];"));
}

#[test]
fn evaluated_truth_overlay_matches_independent_oracle() {
    let mut validated = load_asset().validate().expect("asset validates");
    let domain = validated.families()[0].members()[0]
        .observation_domain()
        .expect("emit member has a domain");
    assert_eq!(
        validated.families()[0].members()[0].emitted_name(),
        "front_end_low_dc"
    );

    let physical: Vec<f64> = (domain[0]..=domain[1])
        .map(|code| {
            support::oracle_physical(code, support::LOW_TRANSFORM.0, support::LOW_TRANSFORM.1)
        })
        .collect();
    validated
        .set_source(
            "front_end_low_dc",
            TransferSource::evaluated_truth(domain[0], physical).inherit_provenance(),
        )
        .expect("overlay spans the member domain");

    let result = validated
        .generate_report(&GenerateOptions::default())
        .expect("overlay generation succeeds");
    let overlay = result
        .report
        .transfers
        .iter()
        .find(|transfer| transfer.symbol == "FRONT_END_LOW_DC")
        .expect("overlay member is reported");
    assert_eq!(overlay.generation_path, GenerationPath::EvaluatedTruth);
    assert_eq!(
        overlay
            .provenance
            .as_ref()
            .map(|citation| citation.identity.as_str()),
        Some("synthetic multi-range ADC note")
    );
    assert!(result.source.contains("evaluated physical truth"));
    assert!(result.source.contains("PiecewiseLinearTransfer"));
    assert!(!result.source.contains("f32"));
    assert!(!result.source.contains("f64"));

    let inputs = parse_int_array::<u16>(&result.source, "FRONT_END_LOW_DC_INPUTS");
    let outputs = parse_int_array::<i32>(&result.source, "FRONT_END_LOW_DC_OUTPUTS");
    for code in overlay.domain_min..=overlay.domain_max {
        let converted = convert_from_knots(&inputs, &outputs, code);
        let expected =
            support::oracle_scaled(code, support::LOW_TRANSFORM.0, support::LOW_TRANSFORM.1);
        let error = (f64::from(converted) - expected).abs();
        assert!(
            error <= f64::from(overlay.achieved_max_error),
            "overlay code {code}: convert={converted} oracle={expected} error={error} bound={}",
            overlay.achieved_max_error
        );
    }
}

#[test]
fn removing_an_expected_identity_fails_completeness() {
    let toml = ASSET.replace(
        r#"[[transfer_families.front_end.gaps]]
selectors = { range = "mid", coupling = "ac" }
status = "undefined"
reason = "AC coupling on the mid range is not characterized"
provenance = { locator = "Table 1, omitted row" }
"#,
        "",
    );
    let error = DefinitionsFile::from_toml_str(&toml)
        .expect("mutated document still parses")
        .validate()
        .expect_err("omitting the mid/ac gap must fail")
        .to_string();
    assert!(
        error.contains("declared selector universe contains 6 identities"),
        "{error}"
    );
    assert!(error.contains("occupy 5"), "{error}");
}

#[test]
fn mutating_provenance_identity_is_rejected_or_observed() {
    let blank = ASSET.replace(
        r#"identity = "synthetic multi-range ADC note""#,
        r#"identity = """#,
    );
    let blank_error = DefinitionsFile::from_toml_str(&blank)
        .expect("blank identity still parses")
        .validate()
        .expect_err("blank provenance.identity must fail")
        .to_string();
    assert!(
        blank_error.contains("provenance.identity must not be blank"),
        "{blank_error}"
    );

    let mutated = ASSET.replace("synthetic multi-range ADC note", "mutated citation");
    let generated = DefinitionsFile::from_toml_str(&mutated)
        .expect("mutated identity parses")
        .validate()
        .expect("non-blank replacement identity validates")
        .generate(&GenerateOptions::default())
        .expect("mutated identity generates");
    assert!(generated.contains(r#"identity "mutated citation""#));
    assert!(!generated.contains(r#"identity "synthetic multi-range ADC note""#));
}

#[test]
fn exceeding_family_budget_fails_closed() {
    let knots = ASSET.replace("max_total_knots = 8", "max_total_knots = 5");
    let knots_error = DefinitionsFile::from_toml_str(&knots)
        .expect("tighter knot budget parses")
        .validate()
        .expect("validation precedes fitting")
        .generate(&GenerateOptions::default())
        .expect_err("6 knots exceed max_total_knots=5")
        .to_string();
    assert!(
        knots_error.contains("max_total_knots=5 exceeded: 6 knots across 3 emitted members"),
        "{knots_error}"
    );

    let bytes = ASSET.replace("max_table_bytes = 48", "max_table_bytes = 24");
    let bytes_error = DefinitionsFile::from_toml_str(&bytes)
        .expect("tighter byte budget parses")
        .validate()
        .expect("validation precedes fitting")
        .generate(&GenerateOptions::default())
        .expect_err("36 bytes exceed max_table_bytes=24")
        .to_string();
    assert!(
        bytes_error.contains(
            "max_table_bytes=24 exceeded: 36 bytes array payload across 3 emitted members"
        ),
        "{bytes_error}"
    );
}

#[test]
fn host_report_maps_symbols_to_family_selectors_provenance_and_policy() {
    let result =
        generate_from_toml_report("assets/family-acceptance.toml", &GenerateOptions::default())
            .expect("report generation succeeds");

    let family = result
        .report
        .families
        .iter()
        .find(|entry| entry.name == "front_end")
        .expect("family report exists");
    assert_eq!(family.completeness, FamilyCompleteness::Complete);
    assert_eq!(family.provenance.identity, "synthetic multi-range ADC note");
    assert_eq!(family.provenance.locator.as_deref(), Some("Table 1"));
    assert_eq!(family.max_total_knots, Some(8));
    assert_eq!(family.max_table_bytes, Some(48));
    assert_eq!(family.totals.member_count, 3);
    assert_eq!(family.totals.knot_count, 6);
    assert_eq!(family.totals.table_bytes, 6 * TABLE_BYTES_PER_KNOT);
    assert_eq!(family.policy.max_interpolation_error, 1);
    assert_eq!(family.policy.max_knots, 32);
    assert_eq!(family.policy.below, BoundaryDef::Error);
    assert_eq!(family.policy.above, BoundaryDef::Clamp);
    assert_eq!(
        family
            .observation_guard_provenance
            .as_ref()
            .map(|citation| citation.locator.as_deref()),
        Some(Some("§4 saturation"))
    );

    assert_eq!(family.members.len(), 5);
    let statuses: Vec<_> = family.members.iter().map(|member| member.status).collect();
    assert_eq!(
        statuses,
        [
            MemberStatus::Emit,
            MemberStatus::Emit,
            MemberStatus::Emit,
            MemberStatus::Forbidden,
            MemberStatus::Unsupported
        ]
    );
    assert_eq!(family.gaps.len(), 1);
    assert_eq!(family.gaps[0].selectors, identity("mid", "ac"));

    for (symbol, metadata, table) in [
        (
            "FRONT_END_LOW_DC",
            FRONT_END_LOW_DC_METADATA,
            "front_end_low_dc",
        ),
        (
            "FRONT_END_MID_DC",
            FRONT_END_MID_DC_METADATA,
            "front_end_mid_dc",
        ),
        (
            "FRONT_END_HIGH_DC",
            FRONT_END_HIGH_DC_METADATA,
            "front_end_high_dc",
        ),
    ] {
        let transfer = result
            .report
            .transfers
            .iter()
            .find(|entry| entry.symbol == symbol)
            .unwrap_or_else(|| panic!("{symbol} reported"));
        assert_eq!(transfer.family.as_deref(), Some("front_end"));
        assert_eq!(transfer.table_name, table);
        assert_eq!(transfer.metadata_symbol, format!("{symbol}_METADATA"));
        assert_eq!(
            transfer.observation_guard_symbol,
            format!("{symbol}_OBSERVATION_GUARD")
        );
        assert_eq!(transfer.domain_min, metadata.domain_min);
        assert_eq!(transfer.domain_max, metadata.domain_max);
        assert_eq!(transfer.knot_count, metadata.knot_count);
        assert_eq!(
            transfer.table_bytes,
            metadata.knot_count * TABLE_BYTES_PER_KNOT
        );
        assert_eq!(transfer.achieved_max_error, metadata.achieved_max_error);
        assert_eq!(transfer.generation_path, GenerationPath::ScaledPolynomial);
        assert_eq!(
            transfer
                .provenance
                .as_ref()
                .map(|citation| citation.identity.as_str()),
            Some("synthetic multi-range ADC note")
        );
        assert_eq!(transfer.policy.max_interpolation_error, 1);
        assert!(
            result
                .source
                .contains(&format!("pub const {symbol}: PiecewiseLinearTransfer"))
        );
    }

    assert!(result.source.contains(r#"Family: "front\_end""#));
    assert!(
        result
            .source
            .contains(r#"Selectors: "coupling" = "dc", "range" = "low""#)
    );
    assert!(result.source.contains("Generation policy: requested interpolation error <= 1; max_knots = 32; below = error; above = clamp."));
    assert!(result.source.contains(
        r#"Source provenance: identity "synthetic multi-range ADC note"; locator "Table 1"."#
    ));
    assert!(result.source.contains("pub const LINEAR"));
    assert!(result.source.contains("CurveLut"));
    assert!(result.source.contains("PiecewiseLinearTransfer"));
    assert!(!result.source.contains("f32"));
    assert!(!result.source.contains("f64"));

    let document_gap = result
        .report
        .gaps
        .iter()
        .find(|gap| gap.name == "digital_flag")
        .expect("document-level gap is reported");
    assert_eq!(document_gap.status, GapStatus::Undefined);
}

#[test]
fn emission_manifest_lists_each_emit_member_once() {
    let validated = load_asset().validate().expect("asset validates");
    let manifest = validated.emission_manifest();
    let symbols: Vec<_> = manifest
        .entries()
        .iter()
        .map(|entry| entry.symbol.as_str())
        .collect();
    assert_eq!(
        symbols,
        ["FRONT_END_HIGH_DC", "FRONT_END_LOW_DC", "FRONT_END_MID_DC"]
    );
}
