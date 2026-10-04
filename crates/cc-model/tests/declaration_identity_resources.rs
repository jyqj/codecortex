//! Fresh, self-authored pure assertions; no parser, capture or production admission.
use cc_model::{declaration_identity::*, source::ByteSpan};
use std::collections::BTreeMap;

fn fixture() -> (
    BTreeMap<String, Vec<u8>>,
    Vec<ConfiguredRoot>,
    DeclarationInput,
) {
    let source = b"class Bell: pass\n";
    let files = BTreeMap::from([
        ("config".into(), b"src".to_vec()),
        ("src/a/__init__.py".into(), vec![]),
        ("src/a/b/__init__.py".into(), vec![]),
        ("src/a/b/chime.py".into(), source.to_vec()),
    ]);
    let roots = vec![ConfiguredRoot {
        directory: "src".into(),
        config_path: "config".into(),
        config_digest: content_digest(b"src"),
        directive: "root".into(),
    }];
    let input = DeclarationInput {
        file_path: "src/a/b/chime.py".into(),
        source_digest: content_digest(source),
        ancestry: vec![DeclarationSegment {
            name: "Bell".into(),
            kind: ScopeKind::Class,
            span: ByteSpan {
                start: 0,
                end: source.len(),
            },
            name_span: ByteSpan { start: 6, end: 10 },
        }],
    };
    (files, roots, input)
}
fn limits() -> DeclarationLimits {
    DeclarationLimits {
        max_files: 4,
        max_total_bytes: 20,
        max_file_bytes: 17,
        max_roots: 1,
        max_evidence: 2,
        max_ancestry_depth: 1,
        max_identifier_bytes: 5,
    }
}
fn refusal<T>(result: DeclarationResult<T>, resource: ResourceKind, limit: usize, actual: usize) {
    match result {
        Err(DeclarationError::Resource(got)) => assert_eq!(
            got,
            ResourceRefusal::LimitExceeded {
                resource,
                limit,
                actual
            }
        ),
        _ => panic!("expected typed resource refusal"),
    }
}
fn derived(outcome: IdentityOutcome) -> Box<BoundDeclaration> {
    match outcome {
        IdentityOutcome::Derived(d) => d,
        _ => panic!("expected derived"),
    }
}
#[test]
fn exact_admission_and_resolution_boundaries_preserve_fingerprints() {
    let (files, roots, input) = fixture();
    assert_eq!(files.values().map(Vec::len).sum::<usize>(), 20);
    let borrowed =
        DeclarationSnapshot::with_limits("bell".into(), &files, roots.clone(), limits()).unwrap();
    let owned =
        DeclarationSnapshot::with_limits("bell".into(), files.clone(), roots.clone(), limits())
            .unwrap();
    let legacy = DeclarationSnapshot::new("bell".into(), files.clone(), roots).unwrap();
    let a = derived(borrowed.resolve_with_limits(&input).unwrap());
    assert_eq!(a, derived(owned.resolve_with_limits(&input).unwrap()));
    assert_eq!(a, derived(legacy.resolve(&input).unwrap()));
    assert_eq!(
        a.fingerprint().unwrap(),
        derived(legacy.resolve(&input).unwrap())
            .fingerprint()
            .unwrap()
    );
    assert!(borrowed.is_current_with_limits(&a).unwrap());
    assert_eq!(borrowed.limits(), limits());
}
#[test]
fn each_inventory_budget_refuses_one_over() {
    let (files, roots, _) = fixture();
    for (kind, cap, actual) in [
        (ResourceKind::FileCount, 3, 4),
        (ResourceKind::TotalBytes, 19, 20),
        (ResourceKind::FileBytes, 16, 17),
        (ResourceKind::Roots, 0, 1),
    ] {
        let mut l = limits();
        match kind {
            ResourceKind::FileCount => l.max_files = cap,
            ResourceKind::TotalBytes => l.max_total_bytes = cap,
            ResourceKind::FileBytes => l.max_file_bytes = cap,
            ResourceKind::Roots => l.max_roots = cap,
            _ => unreachable!(),
        }
        refusal(
            DeclarationSnapshot::with_limits("bell".into(), &files, roots.clone(), l),
            kind,
            cap,
            actual,
        );
    }
}
#[test]
fn size_preflight_checks_overflow_without_allocating_bytes() {
    let l = DeclarationLimits {
        max_files: 2,
        max_total_bytes: usize::MAX,
        max_file_bytes: usize::MAX,
        ..limits()
    };
    assert_eq!(
        l.check_inventory_sizes(1, [usize::MAX]).unwrap(),
        usize::MAX
    );
    assert!(matches!(
        l.check_inventory_sizes(2, [usize::MAX, 1]),
        Err(DeclarationError::Resource(
            ResourceRefusal::ArithmeticOverflow {
                resource: ResourceKind::TotalBytes
            }
        ))
    ));
    refusal(
        l.check_inventory_sizes(3, std::iter::from_fn(|| panic!("must not traverse"))),
        ResourceKind::FileCount,
        2,
        3,
    );
    assert!(matches!(
        l.check_inventory_sizes(2, [1]),
        Err(DeclarationError::Model(_))
    ));
    refusal(
        l.check_inventory_sizes(1, [1, 1]),
        ResourceKind::FileCount,
        1,
        2,
    );
}
#[test]
fn evidence_depth_and_identifier_one_over_are_typed() {
    let (files, roots, mut input) = fixture();
    let mut l = limits();
    l.max_evidence = 1;
    let snapshot =
        DeclarationSnapshot::with_limits("bell".into(), &files, roots.clone(), l).unwrap();
    refusal(
        snapshot.resolve_with_limits(&input),
        ResourceKind::Evidence,
        1,
        2,
    );
    let snapshot =
        DeclarationSnapshot::with_limits("bell".into(), &files, roots, limits()).unwrap();
    input.ancestry.push(input.ancestry[0].clone());
    // Oversized ancestry wins even over a malformed path and names.
    input.file_path = "../bad".into();
    input.ancestry[0].name = "much_too_long".into();
    refusal(
        snapshot.resolve_with_limits(&input),
        ResourceKind::AncestryDepth,
        1,
        2,
    );
    input = fixture().2;
    input.ancestry[0].name = "ABCDEF".into();
    refusal(
        snapshot.resolve_with_limits(&input),
        ResourceKind::IdentifierBytes,
        5,
        6,
    );
    input = fixture().2;
    input.file_path = "src/a/b/longer.py".into();
    refusal(
        snapshot.resolve_with_limits(&input),
        ResourceKind::IdentifierBytes,
        5,
        6,
    );
}
#[test]
fn multibyte_identifier_budget_counts_bytes_before_ascii_policy() {
    let (files, roots, mut input) = fixture();
    input.ancestry[0].name = "钟钟".into(); // two characters, six bytes
    let mut l = limits();
    l.max_identifier_bytes = 5;
    let snapshot =
        DeclarationSnapshot::with_limits("bell".into(), &files, roots.clone(), l).unwrap();
    refusal(
        snapshot.resolve_with_limits(&input),
        ResourceKind::IdentifierBytes,
        5,
        6,
    );
    l.max_identifier_bytes = 6;
    let snapshot = DeclarationSnapshot::with_limits("bell".into(), &files, roots, l).unwrap();
    assert_eq!(
        snapshot.resolve_with_limits(&input).unwrap(),
        IdentityOutcome::Unavailable(IdentityReason::UnsupportedIdentifier)
    );
}
#[test]
fn deep_assertions_are_iterative_and_bounded_before_traversal() {
    let (files, roots, mut input) = fixture();
    // Equal nested Class ranges remain an adapter assertion, as in the pure model.
    input.ancestry = vec![input.ancestry[0].clone(); 1024];
    let l = DeclarationLimits {
        max_ancestry_depth: 1024,
        ..limits()
    };
    let snapshot =
        DeclarationSnapshot::with_limits("bell".into(), &files, roots.clone(), l).unwrap();
    let bound = derived(snapshot.resolve_with_limits(&input).unwrap());
    assert_eq!(bound.binding().ancestry.len(), 1024);
    input.ancestry.push(input.ancestry[0].clone());
    refusal(
        snapshot.resolve_with_limits(&input),
        ResourceKind::AncestryDepth,
        1024,
        1025,
    );
    let small = DeclarationSnapshot::with_limits("bell".into(), &files, roots, limits()).unwrap();
    refusal(
        small.is_current_with_limits(&bound),
        ResourceKind::AncestryDepth,
        1,
        1024,
    );
}
#[test]
fn partial_failures_leave_borrowed_inventory_and_success_deterministic() {
    let (mut files, roots, input) = fixture();
    let before = files.clone();
    let mut bad_roots = roots.clone();
    bad_roots[0].config_digest = content_digest(b"bad");
    assert!(matches!(
        DeclarationSnapshot::with_limits("bell".into(), &files, bad_roots, limits()),
        Err(DeclarationError::Model(_))
    ));
    assert_eq!(files, before);
    let a = {
        let snapshot =
            DeclarationSnapshot::with_limits("bell".into(), &files, roots.clone(), limits())
                .unwrap();
        let a = derived(snapshot.resolve_with_limits(&input).unwrap());
        let mut bad = input.clone();
        bad.ancestry[0].name_span.end = usize::MAX;
        assert_eq!(
            snapshot.resolve_with_limits(&bad).unwrap(),
            IdentityOutcome::Unavailable(IdentityReason::InvalidDeclaration)
        );
        assert_eq!(a, derived(snapshot.resolve_with_limits(&input).unwrap()));
        a
    };
    files.insert("extra".into(), vec![]);
    let l = DeclarationLimits {
        max_files: 5,
        ..limits()
    };
    let changed = DeclarationSnapshot::with_limits("bell".into(), &files, roots, l).unwrap();
    assert!(!changed.is_current_with_limits(&a).unwrap());
    assert_eq!(
        a.address(),
        derived(changed.resolve_with_limits(&input).unwrap()).address()
    );
}
#[test]
fn zero_budgets_and_fixed_metadata_boundaries() {
    let empty = BTreeMap::<String, Vec<u8>>::new();
    let zero = DeclarationLimits {
        max_files: 0,
        max_total_bytes: 0,
        max_file_bytes: 0,
        max_roots: 0,
        max_evidence: 0,
        max_ancestry_depth: 0,
        max_identifier_bytes: 0,
    };
    assert_eq!(zero.check_inventory_sizes(0, []).unwrap(), 0);
    assert!(DeclarationSnapshot::with_limits("o".repeat(4096), &empty, vec![], zero).is_ok());
    refusal(
        DeclarationSnapshot::with_limits("o".repeat(4097), &empty, vec![], zero),
        ResourceKind::MetadataBytes,
        4096,
        4097,
    );
    let (files, roots, mut input) = fixture();
    let snapshot =
        DeclarationSnapshot::with_limits("bell".into(), &files, roots, limits()).unwrap();
    input.file_path = "a".repeat(4097);
    refusal(
        snapshot.resolve_with_limits(&input),
        ResourceKind::MetadataBytes,
        4096,
        4097,
    );
    let mut excessive = files.clone();
    excessive.insert("a".repeat(4097), vec![]);
    let l = DeclarationLimits {
        max_files: 5,
        ..limits()
    };
    refusal(
        DeclarationSnapshot::with_limits("bell".into(), &excessive, vec![], l),
        ResourceKind::MetadataBytes,
        4096,
        4097,
    );
}

#[test]
fn compatibility_constructor_has_a_real_finite_count_policy() {
    let files: BTreeMap<String, Vec<u8>> = (0..=DeclarationLimits::PROTOTYPE.max_files)
        .map(|n| (format!("file_{n}"), vec![]))
        .collect();
    let error = match DeclarationSnapshot::new("bell".into(), files, vec![]) {
        Err(error) => error,
        Ok(_) => panic!("compatibility admission must remain finite"),
    };
    assert!(
        matches!(error, cc_model::CcError::InvalidParams(ref message)
        if message.contains("FileCount") && message.contains("4097"))
    );
}
