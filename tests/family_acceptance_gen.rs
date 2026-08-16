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

fn expected_emits() -> [(&'static str, &'static str, &'static str, &'static str); 3] {
    [
        ("FRONT_END_HIGH_DC", "front_end_high_dc", "high", "dc"),
        ("FRONT_END_LOW_DC", "front_end_low_dc", "low", "dc"),
        ("FRONT_END_MID_DC", "front_end_mid_dc", "mid", "dc"),
    ]
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
        support::MAX_INTERPOLATION_ERROR,
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

fn generated_transfer_artifact<'a>(source: &'a str, symbol: &str) -> &'a str {
    let needle = format!("static {symbol}_INPUTS:");
    let anchor = source
        .find(&needle)
        .unwrap_or_else(|| panic!("missing generated inputs for {symbol}"));
    let start = source[..anchor]
        .rfind("#[rustfmt::skip]\n")
        .expect("generated inputs have a rustfmt attribute");
    let after_anchor = anchor + needle.len();
    let end = source[after_anchor..]
        .find("_INPUTS:")
        .map_or(source.len(), |offset| {
            let next_anchor = after_anchor + offset;
            source[..next_anchor]
                .rfind("#[rustfmt::skip]\n")
                .expect("next generated inputs have a rustfmt attribute")
        });
    source[start..end].trim_end()
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

    let expected = BTreeSet::from([
        identity("low", "dc"),
        identity("mid", "dc"),
        identity("high", "dc"),
        identity("low", "ac"),
        identity("mid", "ac"),
        identity("high", "ac"),
    ]);
    let enumerated: BTreeSet<_> = family.selector_universe().identities().collect();
    assert_eq!(enumerated, expected);
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

    assert_eq!(from_toml.emission_manifest(), from_spec.emission_manifest());

    let options = GenerateOptions::transfers_only();
    let toml_result = from_toml
        .generate_report(&options)
        .expect("TOML family generates");
    let spec_result = from_spec
        .generate_report(&options)
        .expect("programmatic family generates");
    let toml_family_report = toml_result
        .report
        .families
        .iter()
        .find(|family| family.name == "front_end")
        .expect("TOML family report exists");
    assert_eq!(toml_family_report, &spec_result.report.families[0]);
    let toml_transfer_reports: Vec<_> = toml_result
        .report
        .transfers
        .iter()
        .filter(|transfer| transfer.family.as_deref() == Some("front_end"))
        .collect();
    let spec_transfer_reports: Vec<_> = spec_result.report.transfers.iter().collect();
    assert_eq!(toml_transfer_reports, spec_transfer_reports);
    for (symbol, _, _, _) in expected_emits() {
        assert_eq!(
            generated_transfer_artifact(&toml_result.source, symbol),
            generated_transfer_artifact(&spec_result.source, symbol),
            "TOML and FamilySpec must emit byte-identical artifacts for {symbol}"
        );
    }

    let spec_source = &spec_result.source;
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
        overlay.requested_max_error,
        support::MAX_INTERPOLATION_ERROR
    );
    assert_eq!(
        overlay.policy.max_interpolation_error,
        support::MAX_INTERPOLATION_ERROR
    );
    assert!(overlay.achieved_max_error <= overlay.requested_max_error);
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
    let mut measured_max_error = f64::NEG_INFINITY;
    let mut measured_worst_input = overlay.domain_min;
    for code in overlay.domain_min..=overlay.domain_max {
        let converted = convert_from_knots(&inputs, &outputs, code);
        let expected =
            support::oracle_scaled(code, support::LOW_TRANSFORM.0, support::LOW_TRANSFORM.1);
        let error = (f64::from(converted) - expected).abs();
        assert!(
            error <= f64::from(support::MAX_INTERPOLATION_ERROR),
            "overlay code {code}: convert={converted} oracle={expected} error={error} requested_bound={}",
            support::MAX_INTERPOLATION_ERROR
        );
        if error > measured_max_error {
            measured_max_error = error;
            measured_worst_input = code;
        }
    }
    assert_eq!(
        measured_max_error.ceil() as u32,
        overlay.achieved_max_error,
        "overlay achieved-error report must match the independent exhaustive measurement"
    );
    assert_eq!(measured_worst_input, overlay.worst_case_input);
}

#[test]
fn removing_an_expected_identity_fails_completeness() {
    const MID_AC_GAP: &str = r#"[[transfer_families.front_end.gaps]]
selectors = { range = "mid", coupling = "ac" }
status = "undefined"
reason = "AC coupling on the mid range is not characterized"
provenance = { locator = "Table 1, omitted row" }
"#;
    let normalized = ASSET.replace("\r\n", "\n");
    assert_eq!(
        normalized.matches(MID_AC_GAP).count(),
        1,
        "fixture must contain exactly one mid/ac gap block"
    );
    let toml = normalized.replacen(MID_AC_GAP, "", 1);
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
    assert_eq!(
        family.policy.max_interpolation_error,
        support::MAX_INTERPOLATION_ERROR
    );
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

    for (symbol, table, range, coupling) in expected_emits() {
        let metadata = match symbol {
            "FRONT_END_HIGH_DC" => FRONT_END_HIGH_DC_METADATA,
            "FRONT_END_LOW_DC" => FRONT_END_LOW_DC_METADATA,
            "FRONT_END_MID_DC" => FRONT_END_MID_DC_METADATA,
            _ => unreachable!("expected_emits contains only fixture symbols"),
        };
        let expected_selectors = identity(range, coupling);
        let member = family
            .members
            .iter()
            .find(|member| member.symbol.as_deref() == Some(symbol))
            .unwrap_or_else(|| panic!("{symbol} family member reported"));
        assert_eq!(member.selectors, expected_selectors);
        assert_eq!(member.table_name.as_deref(), Some(table));
        let metadata_symbol = format!("{symbol}_METADATA");
        let observation_guard_symbol = format!("{symbol}_OBSERVATION_GUARD");
        assert_eq!(
            member.metadata_symbol.as_deref(),
            Some(metadata_symbol.as_str())
        );
        assert_eq!(
            member.observation_guard_symbol.as_deref(),
            Some(observation_guard_symbol.as_str())
        );
        assert_eq!(&member.provenance, &family.provenance);
        assert!(member.provenance_override.is_none());

        let transfer = result
            .report
            .transfers
            .iter()
            .find(|entry| entry.symbol == symbol)
            .unwrap_or_else(|| panic!("{symbol} reported"));
        assert_eq!(transfer.family.as_deref(), Some("front_end"));
        assert_eq!(transfer.selectors, expected_selectors);
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
            transfer.provenance.as_ref(),
            Some(&member.provenance),
            "{symbol} must retain its complete effective source citation"
        );
        assert_eq!(&transfer.policy, &family.policy);
        assert_eq!(
            transfer.observation_guard,
            Some(ObservationGuardMetadata {
                code: 65_535,
                behavior: ObservationGuardBehavior::Error,
            })
        );
        assert_eq!(
            transfer.observation_guard_provenance.as_ref(),
            family.observation_guard_provenance.as_ref()
        );
        let artifact = generated_transfer_artifact(&result.source, symbol);
        assert!(artifact.contains(&format!(
            r#"Selectors: "coupling" = "{coupling}", "range" = "{range}""#
        )));
        assert!(artifact.contains(r#"Family: "front\_end""#));
        assert!(artifact.contains(
            r#"Source provenance: identity "synthetic multi-range ADC note"; locator "Table 1"."#
        ));
        assert!(artifact.contains(
            "Generation policy: requested interpolation error <= 1; max_knots = 32; below = error; above = clamp."
        ));
        assert!(
            artifact.contains(&format!("pub const {symbol}: PiecewiseLinearTransfer")),
            "{symbol} rustdoc and emitted constant must share one artifact"
        );
    }

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
    assert_eq!(manifest.entries().len(), expected_emits().len());
    for (entry, (symbol, table, range, coupling)) in manifest.entries().iter().zip(expected_emits())
    {
        assert_eq!(entry.family, "front_end");
        assert_eq!(entry.selectors, identity(range, coupling));
        assert_eq!(entry.table_name, table);
        assert_eq!(entry.symbol, symbol);
        assert_eq!(entry.metadata_symbol, format!("{symbol}_METADATA"));
        assert_eq!(
            entry.observation_guard_symbol,
            format!("{symbol}_OBSERVATION_GUARD")
        );
    }
}
