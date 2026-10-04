//! Independent resource review; synthetic assertions only, no capture or parser.
#[path = "support/declaration_resource_baseline.rs"]
mod old;
use cc_model::{declaration_identity::*, source::ByteSpan};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    collections::BTreeMap,
};

struct Counting;
thread_local! { static ALLOCS: Cell<Option<usize>> = const { Cell::new(None) }; }
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _ = ALLOCS.try_with(|c| {
            if let Some(n) = c.get() {
                c.set(Some(n + 1));
            }
        });
        System.alloc(layout)
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout)
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let _ = ALLOCS.try_with(|c| {
            if let Some(n) = c.get() {
                c.set(Some(n + 1));
            }
        });
        System.realloc(ptr, layout, size)
    }
}
#[global_allocator]
static ALLOCATOR: Counting = Counting;
fn measured<T>(f: impl FnOnce() -> T) -> (T, usize) {
    ALLOCS.with(|c| c.set(Some(0)));
    let result = f();
    let count = ALLOCS.with(|c| c.replace(None).unwrap());
    (result, count)
}
fn policy() -> DeclarationLimits {
    DeclarationLimits {
        max_files: 16,
        max_total_bytes: 4096,
        max_file_bytes: 1024,
        max_roots: 4,
        max_evidence: 4,
        max_ancestry_depth: 4,
        max_identifier_bytes: 16,
    }
}
fn fixture(
    path: &str,
    directory: &str,
) -> (
    BTreeMap<String, Vec<u8>>,
    Vec<ConfiguredRoot>,
    DeclarationInput,
) {
    let bytes = b"class Outer:\n    def call(): pass\n";
    let mut files = BTreeMap::from([
        ("cfg".into(), b"root".to_vec()),
        (path.into(), bytes.to_vec()),
    ]);
    files
        .entry(format!("{directory}pkg/__init__.py"))
        .or_insert_with(|| b"raise Exception()".to_vec());
    files
        .entry(format!("{directory}pkg/sub/__init__.py"))
        .or_default();
    let root = ConfiguredRoot {
        directory: directory.trim_end_matches('/').into(),
        config_path: "cfg".into(),
        config_digest: content_digest(b"root"),
        directive: "root".into(),
    };
    let input = DeclarationInput {
        file_path: path.into(),
        source_digest: content_digest(bytes),
        ancestry: vec![
            DeclarationSegment {
                name: "Outer".into(),
                kind: ScopeKind::Class,
                span: ByteSpan {
                    start: 0,
                    end: bytes.len(),
                },
                name_span: ByteSpan { start: 6, end: 11 },
            },
            DeclarationSegment {
                name: "call".into(),
                kind: ScopeKind::Function,
                span: ByteSpan {
                    start: 17,
                    end: bytes.len(),
                },
                name_span: ByteSpan { start: 21, end: 25 },
            },
        ],
    };
    (files, vec![root], input)
}
fn bound(outcome: IdentityOutcome) -> Box<BoundDeclaration> {
    match outcome {
        IdentityOutcome::Derived(b) => b,
        other => panic!("{other:?}"),
    }
}
fn refusal<T>(result: DeclarationResult<T>, kind: ResourceKind, limit: usize, actual: usize) {
    assert!(
        matches!(result, Err(DeclarationError::Resource(ResourceRefusal::LimitExceeded {
        resource, limit: l, actual: a })) if resource == kind && l == limit && a == actual)
    );
}

#[test]
fn frozen_baseline_full_wire_and_fingerprint_match_under_distinct_policies() {
    for (path, directory) in [
        ("plain.py", ""),
        ("pkg/unit.py", ""),
        ("src/pkg/sub/unit.py", "src/"),
        ("src/pkg/sub/__init__.py", "src/"),
    ] {
        let (files, roots, mut input) = fixture(path, directory);
        let old_roots = roots
            .iter()
            .map(|r| old::ConfiguredRoot {
                directory: r.directory.clone(),
                config_path: r.config_path.clone(),
                config_digest: r.config_digest.clone(),
                directive: r.directive.clone(),
            })
            .collect();
        let baseline =
            old::DeclarationSnapshot::new("review".into(), files.clone(), old_roots).unwrap();
        let mut normalized_root = roots.clone();
        normalized_root[0].directory = format!("./{}//", normalized_root[0].directory);
        if directory.is_empty() {
            normalized_root[0].directory = "./".into();
        }
        input.file_path = format!("./{}", input.file_path.replace('/', "\\"));
        let old_input = old::DeclarationInput {
            file_path: input.file_path.clone(),
            source_digest: input.source_digest.clone(),
            ancestry: input
                .ancestry
                .iter()
                .map(|s| old::DeclarationSegment {
                    name: s.name.clone(),
                    kind: match s.kind {
                        ScopeKind::Class => old::ScopeKind::Class,
                        ScopeKind::Function => old::ScopeKind::Function,
                    },
                    span: s.span,
                    name_span: s.name_span,
                })
                .collect(),
        };
        let old::IdentityOutcome::Derived(expected) = baseline.resolve(&old_input).unwrap() else {
            panic!("baseline unavailable")
        };
        assert!(baseline.is_current(&expected).unwrap());
        for limits in [policy(), DeclarationLimits::PROTOTYPE] {
            let borrowed = DeclarationSnapshot::with_limits(
                "review".into(),
                &files,
                normalized_root.clone(),
                limits,
            )
            .unwrap();
            let actual = bound(borrowed.resolve_with_limits(&input).unwrap());
            assert_eq!(
                serde_json::to_value(actual.address()).unwrap(),
                serde_json::to_value(expected.address()).unwrap()
            );
            assert_eq!(
                serde_json::to_value(&actual).unwrap(),
                serde_json::to_value(&expected).unwrap()
            );
            assert_eq!(
                actual.fingerprint().unwrap(),
                expected.fingerprint().unwrap()
            );
            let owned = DeclarationSnapshot::with_limits(
                "review".into(),
                files.clone(),
                roots.clone(),
                limits,
            )
            .unwrap();
            assert_eq!(actual, bound(owned.resolve_with_limits(&input).unwrap()));
            let compatible =
                DeclarationSnapshot::new("review".into(), files.clone(), roots.clone()).unwrap();
            assert_eq!(actual, bound(compatible.resolve(&input).unwrap()));
        }
    }
}

#[test]
fn allocation_free_resource_checks_precede_ancestry_clone_and_path_work() {
    let (files, roots, mut input) = fixture("pkg/unit.py", "");
    input.ancestry = vec![input.ancestry[0].clone(); 128];
    let big = DeclarationSnapshot::with_limits(
        "review".into(),
        &files,
        roots.clone(),
        DeclarationLimits {
            max_ancestry_depth: 128,
            ..policy()
        },
    )
    .unwrap();
    let declaration = bound(big.resolve_with_limits(&input).unwrap());
    let small =
        DeclarationSnapshot::with_limits("review".into(), &files, roots.clone(), policy()).unwrap();
    let (result, allocations) = measured(|| small.is_current_with_limits(&declaration));
    assert_eq!(allocations, 0);
    refusal(result, ResourceKind::AncestryDepth, 4, 128);
    input.file_path = "../".repeat(2000);
    let (result, allocations) = measured(|| small.resolve_with_limits(&input));
    assert_eq!(allocations, 0);
    refusal(result, ResourceKind::AncestryDepth, 4, 128);
    let owner = "review".to_owned();
    let (result, allocations) = measured(|| {
        DeclarationSnapshot::with_limits(
            owner,
            &files,
            roots,
            DeclarationLimits {
                max_roots: 0,
                max_file_bytes: 0,
                ..policy()
            },
        )
    });
    assert_eq!(allocations, 0);
    refusal(result, ResourceKind::Roots, 0, 1);
    let mut oversized_roots = fixture("pkg/unit.py", "").1;
    oversized_roots[0].directory = format!("{}.", "./".repeat(2048));
    let owner = "review".to_owned();
    let (result, allocations) =
        measured(|| DeclarationSnapshot::with_limits(owner, &files, oversized_roots, policy()));
    assert_eq!(allocations, 0);
    refusal(result, ResourceKind::MetadataBytes, 4096, 4097);
    input.ancestry.truncate(1);
    let (result, allocations) = measured(|| small.resolve_with_limits(&input));
    assert_eq!(allocations, 0);
    refusal(result, ResourceKind::MetadataBytes, 4096, 6000);
}

#[test]
fn every_raw_metadata_field_has_4096_byte_ceiling_before_normalization() {
    let (files, roots, input) = fixture("pkg/unit.py", "");
    for n in [4096, 4097] {
        let mut f = files.clone();
        f.insert("k".repeat(n), vec![]);
        let result = DeclarationSnapshot::with_limits("o".repeat(n), &f, roots.clone(), policy());
        if n == 4096 {
            assert!(result.is_ok());
        } else {
            refusal(result, ResourceKind::MetadataBytes, 4096, n);
        }
        let result = DeclarationSnapshot::with_limits("review".into(), &f, roots.clone(), policy());
        if n == 4096 {
            assert!(result.is_ok());
        } else {
            refusal(result, ResourceKind::MetadataBytes, 4096, n);
        }
        for field in 0..4 {
            let mut f = files.clone();
            let mut r = roots.clone();
            match field {
                0 => {
                    r[0].directory = format!(
                        "{}{}",
                        "./".repeat(n / 2),
                        if n % 2 == 1 { "/" } else { "" }
                    )
                }
                1 => {
                    r[0].config_path = "c".repeat(n);
                    f.insert(r[0].config_path.clone(), b"root".to_vec());
                }
                2 => r[0].directive = "d".repeat(n),
                _ => r[0].config_digest = "e".repeat(n),
            }
            let result = DeclarationSnapshot::with_limits("review".into(), &f, r, policy());
            if n == 4097 {
                refusal(result, ResourceKind::MetadataBytes, 4096, n);
            } else if field == 3 {
                assert!(matches!(result, Err(DeclarationError::Model(_))));
            } else {
                assert!(result.is_ok());
            }
        }
        let snapshot =
            DeclarationSnapshot::with_limits("review".into(), &files, roots.clone(), policy())
                .unwrap();
        let mut i = input.clone();
        i.file_path = format!(
            "{}{}",
            "./".repeat(n / 2),
            if n % 2 == 1 { "/" } else { "" }
        );
        let result = snapshot.resolve_with_limits(&i);
        if n == 4097 {
            refusal(result, ResourceKind::MetadataBytes, 4096, n);
        } else {
            assert!(result.is_ok());
        }
        i = input.clone();
        i.source_digest = "e".repeat(n);
        let result = snapshot.resolve_with_limits(&i);
        if n == 4097 {
            refusal(result, ResourceKind::MetadataBytes, 4096, n);
        } else {
            assert_eq!(
                result.unwrap(),
                IdentityOutcome::Unavailable(IdentityReason::StaleSource)
            );
        }
    }
}

#[test]
fn logical_bytes_ignore_capacity_and_preflight_overflow_and_order_are_typed() {
    let mut extra_capacity = Vec::with_capacity(8192);
    extra_capacity.extend_from_slice(b"x");
    let files = BTreeMap::from([
        ("empty".into(), Vec::with_capacity(8192)),
        ("one".into(), extra_capacity),
    ]);
    let l = DeclarationLimits {
        max_files: 2,
        max_file_bytes: 1,
        max_total_bytes: 1,
        ..policy()
    };
    assert!(DeclarationSnapshot::with_limits("review".into(), &files, vec![], l).is_ok());
    refusal(
        l.check_inventory_sizes(2, [0, 2]),
        ResourceKind::FileBytes,
        1,
        2,
    );
    refusal(
        DeclarationLimits {
            max_total_bytes: 0,
            ..l
        }
        .check_inventory_sizes(2, [0, 1]),
        ResourceKind::TotalBytes,
        0,
        1,
    );
    refusal(
        l.check_inventory_sizes(3, std::iter::from_fn(|| panic!("not traversed"))),
        ResourceKind::FileCount,
        2,
        3,
    );
    let l = DeclarationLimits {
        max_file_bytes: usize::MAX,
        max_total_bytes: usize::MAX,
        ..l
    };
    assert_eq!(
        l.check_inventory_sizes(2, [usize::MAX, 0]).unwrap(),
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
    assert!(matches!(
        l.check_inventory_sizes(2, [0]),
        Err(DeclarationError::Model(_))
    ));
    refusal(
        l.check_inventory_sizes(1, [0, 0]),
        ResourceKind::FileCount,
        1,
        2,
    );
}

#[test]
fn package_and_module_utf8_limits_precede_missing_stale_and_unsupported_reasons() {
    let (files, roots, mut input) = fixture("pkg/unit.py", "");
    let snapshot = DeclarationSnapshot::with_limits(
        "review".into(),
        &files,
        roots.clone(),
        DeclarationLimits {
            max_identifier_bytes: 5,
            max_evidence: 1,
            ..policy()
        },
    )
    .unwrap();
    for path in ["ééé/unit.py", "pkg/ééé.py", "pkg/abcdef.txt"] {
        input.file_path = path.into();
        refusal(
            snapshot.resolve_with_limits(&input),
            ResourceKind::IdentifierBytes,
            5,
            if path.ends_with("txt") { 10 } else { 6 },
        );
    }
    input.file_path = "pkg/sub/absent.py".into();
    input.source_digest = "stale".into();
    refusal(
        snapshot.resolve_with_limits(&input),
        ResourceKind::Evidence,
        1,
        2,
    );
    let snapshot = DeclarationSnapshot::with_limits(
        "review".into(),
        &files,
        roots,
        DeclarationLimits {
            max_identifier_bytes: 6,
            ..policy()
        },
    )
    .unwrap();
    input.file_path = "ééé/unit.py".into();
    assert_eq!(
        snapshot.resolve_with_limits(&input).unwrap(),
        IdentityOutcome::Unavailable(IdentityReason::MissingSource)
    );
}

#[test]
fn stale_config_marker_owner_and_unrelated_inventory_invalidate_complete_binding() {
    let (files, roots, input) = fixture("pkg/unit.py", "");
    let snapshot =
        DeclarationSnapshot::with_limits("review".into(), &files, roots.clone(), policy()).unwrap();
    let original = bound(snapshot.resolve_with_limits(&input).unwrap());
    for change in 0..6 {
        let mut f = files.clone();
        let mut r = roots.clone();
        let mut owner = "review".to_owned();
        match change {
            0 => {
                f.insert("unrelated".into(), vec![]);
            }
            1 => {
                f.insert("pkg/sub/__init__.py".into(), b"changed".to_vec());
            }
            2 => {
                f.insert("cfg".into(), b"changed".to_vec());
                r[0].config_digest = content_digest(b"changed");
            }
            3 => {
                r[0].directive = "different".into();
            }
            4 => {
                owner = "other".into();
            }
            _ => {
                f.insert("pkg/__init__.py".into(), b"changed".to_vec());
            }
        }
        let changed = DeclarationSnapshot::with_limits(owner, &f, r, policy()).unwrap();
        assert!(!changed.is_current_with_limits(&original).unwrap());
        assert_ne!(
            original.fingerprint().unwrap(),
            bound(changed.resolve_with_limits(&input).unwrap())
                .fingerprint()
                .unwrap()
        );
    }
    let mut stale = input;
    stale.source_digest = content_digest(b"other");
    assert_eq!(
        snapshot.resolve_with_limits(&stale).unwrap(),
        IdentityOutcome::Unavailable(IdentityReason::StaleSource)
    );
    let mut bad_roots = roots;
    bad_roots[0].config_digest = content_digest(b"other");
    assert!(matches!(
        DeclarationSnapshot::with_limits("review".into(), &files, bad_roots, policy()),
        Err(DeclarationError::Model(_))
    ));
    assert!(snapshot.is_current_with_limits(&original).unwrap());
}

#[test]
fn independent_exact_and_one_over_matrix_and_prototype_constants() {
    let (files, roots, input) = fixture("pkg/unit.py", "");
    let total = files.values().map(Vec::len).sum::<usize>();
    let largest = files.values().map(Vec::len).max().unwrap();
    let exact = DeclarationLimits {
        max_files: files.len(),
        max_total_bytes: total,
        max_file_bytes: largest,
        max_roots: 1,
        max_evidence: 1,
        max_ancestry_depth: 2,
        max_identifier_bytes: 5,
    };
    let snapshot =
        DeclarationSnapshot::with_limits("review".into(), &files, roots.clone(), exact).unwrap();
    let expected = bound(snapshot.resolve_with_limits(&input).unwrap());
    assert!(snapshot.is_current_with_limits(&expected).unwrap());
    for (resource, actual) in [
        (ResourceKind::FileCount, files.len()),
        (ResourceKind::TotalBytes, total),
        (ResourceKind::FileBytes, largest),
        (ResourceKind::Roots, 1),
        (ResourceKind::Evidence, 1),
        (ResourceKind::AncestryDepth, 2),
        (ResourceKind::IdentifierBytes, 5),
    ] {
        let mut limits = exact;
        match resource {
            ResourceKind::FileCount => limits.max_files -= 1,
            ResourceKind::TotalBytes => limits.max_total_bytes -= 1,
            ResourceKind::FileBytes => limits.max_file_bytes -= 1,
            ResourceKind::Roots => limits.max_roots -= 1,
            ResourceKind::Evidence => limits.max_evidence -= 1,
            ResourceKind::AncestryDepth => limits.max_ancestry_depth -= 1,
            ResourceKind::IdentifierBytes => limits.max_identifier_bytes -= 1,
            _ => unreachable!(),
        }
        let result =
            DeclarationSnapshot::with_limits("review".into(), &files, roots.clone(), limits);
        match resource {
            ResourceKind::Evidence
            | ResourceKind::AncestryDepth
            | ResourceKind::IdentifierBytes => {
                refusal(
                    result.unwrap().resolve_with_limits(&input),
                    resource,
                    actual - 1,
                    actual,
                );
            }
            _ => refusal(result, resource, actual - 1, actual),
        }
    }
    assert_eq!(
        DeclarationLimits::PROTOTYPE,
        DeclarationLimits {
            max_files: 4096,
            max_total_bytes: 64 * 1024 * 1024,
            max_file_bytes: 8 * 1024 * 1024,
            max_roots: 64,
            max_evidence: 256,
            max_ancestry_depth: 256,
            max_identifier_bytes: 4096
        }
    );
}
