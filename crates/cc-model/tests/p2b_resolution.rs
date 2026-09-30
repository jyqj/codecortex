use cc_model::resolution::*;
#[test]
fn normalizing_between_records_cannot_reset_the_byte_budget() {
    let mut m = ResolutionManifest::new();
    for i in 0..8 {
        m.record(ResolutionRecord {
            site_kind: "call".into(),
            site_id: format!("call:{i}"),
            query: "x".repeat(1024 * 1024),
            outcome: ResolutionOutcome::Unresolved {
                reason: "miss".into(),
            },
        });
        m.normalize();
    }
    assert!(
        m.omitted_records > 0,
        "normalization must not grant a fresh 4MiB allowance"
    );
    assert!(!m.complete);
}
#[test]
fn duplicate_candidates_are_not_two_independent_targets() {
    let target = ResolutionTarget {
        file_path: "a.py".into(),
        symbol_id: "id".into(),
        symbol_uid: Some("uid".into()),
        qname: Some("run".into()),
        kind: "function".into(),
    };
    let mut m = ResolutionManifest::new();
    m.record(ResolutionRecord {
        site_kind: "call".into(),
        site_id: "call:1".into(),
        query: "run".into(),
        outcome: ResolutionOutcome::Ambiguous {
            candidates: vec![target.clone(), target],
            reason: "tie".into(),
            candidate_count_lower_bound: 2,
            truncated: false,
        },
    });
    assert!(m.validate().is_err());
}
#[test]
fn ambiguity_roundtrips_without_a_selected_target() {
    let mut m = ResolutionManifest::new();
    let target = |p: &str| ResolutionTarget {
        file_path: p.into(),
        symbol_id: p.into(),
        symbol_uid: Some(format!("uid:{p}")),
        qname: Some("same".into()),
        kind: "function".into(),
    };
    m.record(ResolutionRecord {
        site_kind: "call".into(),
        site_id: "call:1".into(),
        query: "same".into(),
        outcome: ResolutionOutcome::Ambiguous {
            candidates: vec![target("a.py"), target("b.py")],
            reason: "tie".into(),
            candidate_count_lower_bound: 2,
            truncated: false,
        },
    });
    m.normalize();
    m.validate().unwrap();
    let copy: ResolutionManifest =
        serde_json::from_str(&serde_json::to_string(&m).unwrap()).unwrap();
    assert_eq!(m, copy);
    assert!(matches!(
        copy.records[0].outcome,
        ResolutionOutcome::Ambiguous { .. }
    ));
}
#[test]
fn budget_overflow_preserves_conservative_invalidation() {
    let mut m = ResolutionManifest::new();
    for i in 0..MAX_RESOLUTION_DEPENDENCIES + 10 {
        m.dependency(DependencyKind::NameBucket, format!("name{i}"));
    }
    assert!(!m.complete);
    assert!(m.dependencies.contains(&ResolutionDependency::new(
        DependencyKind::SymbolInventory,
        "*"
    )));
    m.normalize();
    m.validate().unwrap();
    for i in 0..MAX_RESOLUTION_RECORDS + 10 {
        m.record(ResolutionRecord {
            site_kind: "call".into(),
            site_id: format!("call:{i}"),
            query: "missing".into(),
            outcome: ResolutionOutcome::Unresolved {
                reason: "miss".into(),
            },
        });
    }
    m.normalize();
    m.validate().unwrap();
    assert!(m.omitted_records > 0);
    assert!(!m.complete);
}
#[test]
fn invalid_version_and_fake_unique_ambiguity_fail() {
    let mut m = ResolutionManifest::new();
    m.version = 999;
    assert!(m.validate().is_err());
    let mut m = ResolutionManifest::new();
    m.record(ResolutionRecord {
        site_kind: "call".into(),
        site_id: "a".into(),
        query: "x".into(),
        outcome: ResolutionOutcome::Ambiguous {
            candidates: vec![],
            reason: "tie".into(),
            candidate_count_lower_bound: 0,
            truncated: false,
        },
    });
    assert!(m.validate().is_err());
}
