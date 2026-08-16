//! Host generation reports must agree with emitted runtime metadata constants.
//!
//! Numbers come from the compiled fixture, not from parsing generated Rust.

use ph_curves::r#gen::{
    FamilyCompleteness, GenerateOptions, GenerationPath, MemberStatus, ObservationGuardBehaviorDef,
    TABLE_BYTES_PER_KNOT, generate_from_toml_report,
};

include!("fixtures/observation_guards_generated.rs");

#[test]
fn host_report_agrees_with_emitted_observation_guard_metadata() {
    let result = generate_from_toml_report(
        "assets/observation-guards.toml",
        &GenerateOptions::transfers_only(),
    )
    .unwrap();

    let standalone = result
        .report
        .transfers
        .iter()
        .find(|transfer| transfer.symbol == "GUARDED_ERROR")
        .unwrap();
    assert_eq!(standalone.table_name, "guarded_error");
    assert_eq!(standalone.family, None);
    assert_eq!(standalone.generation_path, GenerationPath::Formula);
    assert_eq!(standalone.provenance, None);
    assert_eq!(standalone.observation_guard_provenance, None);
    assert_eq!(standalone.domain_min, GUARDED_ERROR_METADATA.domain_min);
    assert_eq!(standalone.domain_max, GUARDED_ERROR_METADATA.domain_max);
    assert_eq!(standalone.range_min, GUARDED_ERROR_METADATA.range_min);
    assert_eq!(standalone.range_max, GUARDED_ERROR_METADATA.range_max);
    assert_eq!(
        standalone.requested_max_error,
        GUARDED_ERROR_METADATA.requested_max_error
    );
    assert_eq!(
        standalone.achieved_max_error,
        GUARDED_ERROR_METADATA.achieved_max_error
    );
    assert_eq!(
        standalone.worst_case_input,
        GUARDED_ERROR_METADATA.worst_case_input
    );
    assert_eq!(standalone.knot_count, GUARDED_ERROR_METADATA.knot_count);
    assert_eq!(
        standalone.table_bytes,
        GUARDED_ERROR_METADATA.knot_count * TABLE_BYTES_PER_KNOT
    );
    assert_eq!(
        standalone.observation_guard,
        GUARDED_ERROR_OBSERVATION_GUARD
    );
    assert_eq!(
        standalone.policy.max_interpolation_error,
        GUARDED_ERROR_METADATA.requested_max_error
    );
    assert_eq!(standalone.policy.max_knots, 8);
    let standalone_guard_policy = standalone.policy.observation_guard.unwrap();
    let standalone_guard_metadata = GUARDED_ERROR_OBSERVATION_GUARD.unwrap();
    assert_eq!(standalone_guard_policy.code, standalone_guard_metadata.code);
    assert_eq!(
        standalone_guard_policy.behavior,
        ObservationGuardBehaviorDef::Error
    );

    let family_member = result
        .report
        .transfers
        .iter()
        .find(|transfer| transfer.symbol == "GUARDED_FAMILY_VARIANT_CLAMP")
        .unwrap();
    assert_eq!(family_member.family.as_deref(), Some("guarded_family"));
    assert_eq!(
        family_member.metadata_symbol,
        "GUARDED_FAMILY_VARIANT_CLAMP_METADATA"
    );
    assert_eq!(
        family_member.observation_guard_symbol,
        "GUARDED_FAMILY_VARIANT_CLAMP_OBSERVATION_GUARD"
    );
    assert_eq!(
        family_member.domain_min,
        GUARDED_FAMILY_VARIANT_CLAMP_METADATA.domain_min
    );
    assert_eq!(
        family_member.domain_max,
        GUARDED_FAMILY_VARIANT_CLAMP_METADATA.domain_max
    );
    assert_eq!(
        family_member.range_min,
        GUARDED_FAMILY_VARIANT_CLAMP_METADATA.range_min
    );
    assert_eq!(
        family_member.range_max,
        GUARDED_FAMILY_VARIANT_CLAMP_METADATA.range_max
    );
    assert_eq!(
        family_member.requested_max_error,
        GUARDED_FAMILY_VARIANT_CLAMP_METADATA.requested_max_error
    );
    assert_eq!(
        family_member.achieved_max_error,
        GUARDED_FAMILY_VARIANT_CLAMP_METADATA.achieved_max_error
    );
    assert_eq!(
        family_member.worst_case_input,
        GUARDED_FAMILY_VARIANT_CLAMP_METADATA.worst_case_input
    );
    assert_eq!(
        family_member.knot_count,
        GUARDED_FAMILY_VARIANT_CLAMP_METADATA.knot_count
    );
    assert_eq!(
        family_member.observation_guard,
        GUARDED_FAMILY_VARIANT_CLAMP_OBSERVATION_GUARD
    );
    assert_eq!(
        family_member
            .provenance
            .as_ref()
            .map(|source| source.identity.as_str()),
        Some("test fixture")
    );
    assert_eq!(family_member.observation_guard_provenance, None);
    let family_guard_policy = family_member.policy.observation_guard.unwrap();
    let family_guard_metadata = GUARDED_FAMILY_VARIANT_CLAMP_OBSERVATION_GUARD.unwrap();
    assert_eq!(family_guard_policy.code, family_guard_metadata.code);
    assert_eq!(
        family_guard_policy.behavior,
        ObservationGuardBehaviorDef::Clamp
    );

    assert_eq!(result.report.families.len(), 1);
    let family = &result.report.families[0];
    assert_eq!(family.name, "guarded_family");
    assert_eq!(family.provenance.identity, "test fixture");
    assert_eq!(family.observation_guard_provenance, None);
    assert_eq!(family.policy, family_member.policy);
    assert_eq!(family.completeness, FamilyCompleteness::Complete);
    assert_eq!(family.selector_universe.identity_count(), Some(1));
    assert_eq!(family.members.len(), 1);
    assert_eq!(family.members[0].status, MemberStatus::Emit);
    assert_eq!(
        family.members[0].table_name.as_deref(),
        Some(family_member.table_name.as_str())
    );
    assert_eq!(
        family.members[0].symbol.as_deref(),
        Some(family_member.symbol.as_str())
    );
    assert_eq!(family.member_count, 1);
    assert!(family.gaps.is_empty());
    assert!(result.report.gaps.is_empty());
    assert_eq!(result.report.totals.member_count, 2);
    assert_eq!(
        result.report.totals.knot_count,
        standalone.knot_count + family_member.knot_count
    );
    assert_eq!(
        result.report.totals.table_bytes,
        standalone.table_bytes + family_member.table_bytes
    );
}
