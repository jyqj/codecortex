//! Self-authored bounded integration. Real config capture and real AST; the source
//! inventory is an explicit complete fixture assertion, not a production capture API.
use cc_index::project_model::{discover, CapturedProject};
use cc_model::{
    declaration_identity::*,
    module_inputs::{PythonRootEvidence, PythonRootState},
    project_model::ProjectInputs,
    source::ByteSpan,
    Language, StableId,
};
use cc_parsers::{python_identity::*, ParserRegistry};
use std::collections::{BTreeMap, BTreeSet};

const PATH: &str = "src/harbor/tide.py";
const CONFIG: &[u8] = b"# original config\n[tool.setuptools.package-dir]\n\"\"='./src'\n";
const SOURCE: &[u8] = b"\xef\xbb\xbf\r\n# exact original bytes\r\nclass Outer:\r\n    class Inner:\r\n        def pulse(self): return 1\r\ndef factory():\r\n    class Local:\r\n        def hidden(self): return 2\r\n    return Local\r\nif ready:\r\n    def echo(): return 3\r\nelse:\r\n    def echo(): return 4\r\n";
fn inventory() -> BTreeMap<String, Vec<u8>> {
    BTreeMap::from([
        ("pyproject.toml".into(), CONFIG.to_vec()),
        (
            "src/harbor/__init__.py".into(),
            b"raise RuntimeError('never executed')\ndef harbor(): pass\n".to_vec(),
        ),
        (PATH.into(), SOURCE.to_vec()),
    ])
}
fn model_limits(files: &BTreeMap<String, Vec<u8>>) -> DeclarationLimits {
    DeclarationLimits {
        max_files: files.len(),
        max_total_bytes: files.values().map(Vec::len).sum(),
        max_file_bytes: files.values().map(Vec::len).max().unwrap(),
        max_roots: 1,
        max_evidence: 1,
        max_ancestry_depth: 3,
        max_identifier_bytes: 7,
    }
}
fn ast_limits() -> PythonIdentityLimits {
    PythonIdentityLimits {
        source_bytes: 1024,
        visited_nodes: 1000,
        tree_depth: 32,
        declarations: 16,
        output_segments: 32,
        output_text_bytes: 4096,
        parse_timeout_micros: 1_000_000,
    }
}
fn inputs(path: &str, source: &[u8]) -> Vec<DeclarationInput> {
    match declaration_inputs(path, source, ast_limits()).unwrap() {
        PythonIdentityOutcome::Inputs(v) => v,
        other => panic!("unexpected AST refusal: {other:?}"),
    }
}
fn capture(dir: &std::path::Path, files: &BTreeMap<String, Vec<u8>>) -> CapturedProject {
    for (p, bytes) in files {
        let path = dir.join(p);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }
    discover(
        dir,
        files
            .keys()
            .filter(|p| p.ends_with(".py"))
            .cloned()
            .collect::<BTreeSet<_>>(),
        None,
        None,
        &ProjectInputs::default(),
    )
    .unwrap()
}
// Test-only bridge restricted to this supported, single directive fixture. This
// does not authorize a general conversion based on Explicit/captured_document.
fn fixture_root(captured: &CapturedProject, files: &BTreeMap<String, Vec<u8>>) -> ConfiguredRoot {
    let provenance = &captured.model().python().provenance[""];
    assert_eq!(provenance.state, PythonRootState::Explicit);
    assert!(provenance.limitations.is_empty());
    assert_eq!(provenance.roots.len(), 1);
    assert_eq!(
        provenance.roots["src"],
        [PythonRootEvidence::Explicit {
            directive: "/tool/setuptools/package-dir/".into(),
            value: "./src".into(),
        }]
    );
    let binding = provenance.config.as_ref().unwrap();
    assert!(binding.captured_document);
    assert_eq!(binding.path, "pyproject.toml");
    assert_eq!(
        binding.digest.as_deref(),
        Some(content_digest(&files["pyproject.toml"]).as_str())
    );
    ConfiguredRoot {
        directory: "src".into(),
        config_path: binding.path.clone(),
        config_digest: binding.digest.clone().unwrap(),
        directive: "/tool/setuptools/package-dir/".into(),
    }
}
fn derived(outcome: IdentityOutcome) -> Box<BoundDeclaration> {
    match outcome {
        IdentityOutcome::Derived(d) => d,
        other => panic!("{other:?}"),
    }
}
fn refusal<T>(r: DeclarationResult<T>, resource: ResourceKind) {
    assert!(
        matches!(r, Err(DeclarationError::Resource(ResourceRefusal::LimitExceeded { resource: got, .. })) if got == resource)
    );
}
#[test]
fn actual_config_and_ast_bind_regular_package_nested_and_duplicate_occurrences() {
    let files = inventory();
    let dir = tempfile::tempdir().unwrap();
    let captured = capture(dir.path(), &files);
    captured.verify(dir.path()).unwrap();
    let root = fixture_root(&captured, &files);
    let reused = discover(
        dir.path(),
        BTreeSet::from([PATH.into(), "src/harbor/__init__.py".into()]),
        None,
        None,
        captured.inputs(),
    )
    .unwrap();
    assert_eq!(reused.report().config_parse_cache_hits, 1);
    assert_eq!(fixture_root(&reused, &files), root);
    let snapshot: DeclarationSnapshot<&BTreeMap<String, Vec<u8>>> =
        DeclarationSnapshot::with_limits(
            "fixture-owner".into(),
            &files,
            vec![root.clone()],
            model_limits(&files),
        )
        .unwrap();
    let v = inputs(PATH, &files[PATH]);
    assert_eq!(v.len(), 8);
    assert_eq!(
        v.iter()
            .map(|v| v.ancestry.last().unwrap().name.as_str())
            .collect::<Vec<_>>(),
        ["Outer", "Inner", "pulse", "factory", "Local", "hidden", "echo", "echo"]
    );
    for (i, input) in v.iter().enumerate() {
        assert_eq!(input.source_digest, content_digest(SOURCE));
        for segment in &input.ancestry {
            assert_eq!(
                &SOURCE[segment.name_span.start..segment.name_span.end],
                segment.name.as_bytes()
            );
        }
        let outcome = snapshot.resolve_with_limits(input).unwrap();
        if [4, 5].contains(&i) {
            assert_eq!(
                outcome,
                IdentityOutcome::Unavailable(IdentityReason::LocalDeclaration)
            );
        } else {
            let d = derived(outcome);
            assert_eq!(d.address().module, ["harbor", "tide"]);
            assert_eq!(d.binding().root, root);
            assert_eq!(
                d.binding().package_files["src/harbor/__init__.py"],
                content_digest(&files["src/harbor/__init__.py"])
            );
            assert!(snapshot.is_current_with_limits(&d).unwrap());
        }
    }
    assert_eq!(
        v[5].ancestry.iter().map(|s| s.kind).collect::<Vec<_>>(),
        [ScopeKind::Function, ScopeKind::Class, ScopeKind::Function]
    );
    let a = derived(snapshot.resolve_with_limits(&v[6]).unwrap());
    let b = derived(snapshot.resolve_with_limits(&v[7]).unwrap());
    assert_eq!(a.address(), b.address());
    assert_ne!(a.fingerprint().unwrap(), b.fingerprint().unwrap());
    let nested = derived(snapshot.resolve_with_limits(&v[2]).unwrap());
    assert_eq!(
        nested.address().lexical,
        [
            ("Outer".into(), ScopeKind::Class),
            ("Inner".into(), ScopeKind::Class),
            ("pulse".into(), ScopeKind::Function)
        ]
    );
    // Existing lexical extraction/UID remains a distinct contract.
    let out = ParserRegistry::new()
        .parse(PATH, std::str::from_utf8(SOURCE).unwrap(), Language::Python)
        .unwrap();
    let symbol = out
        .symbols
        .iter()
        .find(|s| s.qname.as_deref() == Some("Outer.Inner.pulse"))
        .unwrap();
    assert_eq!(
        symbol.symbol_uid,
        Some(StableId::symbol_uid(
            PATH,
            "Outer.Inner.pulse",
            symbol.kind.as_str(),
            Some("(self)")
        ))
    );
}
#[test]
fn initializer_and_root_initializer_keep_module_semantics() {
    let mut files = inventory();
    files.insert(
        "src/__init__.py".into(),
        b"def root_only(): pass\n".to_vec(),
    );
    let dir = tempfile::tempdir().unwrap();
    let c = capture(dir.path(), &files);
    let mut limits = model_limits(&files);
    limits.max_identifier_bytes = 9;
    let s = DeclarationSnapshot::with_limits(
        "fixture-owner".into(),
        &files,
        vec![fixture_root(&c, &files)],
        limits,
    )
    .unwrap();
    let v = inputs("src/harbor/__init__.py", &files["src/harbor/__init__.py"]);
    let d = derived(s.resolve_with_limits(&v[0]).unwrap());
    assert_eq!(d.address().module, ["harbor"]);
    assert_eq!(
        d.address().lexical,
        [("harbor".into(), ScopeKind::Function)]
    );
    let v = inputs("src/__init__.py", &files["src/__init__.py"]);
    assert_eq!(
        s.resolve_with_limits(&v[0]).unwrap(),
        IdentityOutcome::Unavailable(IdentityReason::RootInitializer)
    );
}
#[test]
fn leading_trivia_bom_and_crlf_keep_fixed_absolute_bytes_through_model() {
    for (source, offset) in [
        (b"\n\ndef f(): pass\n".as_slice(), 2),
        (b"  # lead\r\n\r\ndef f(): pass\r\n  ".as_slice(), 12),
        (b"\xef\xbb\xbfdef f(): pass\r\n".as_slice(), 3),
    ] {
        let mut files = inventory();
        files.insert(PATH.into(), source.to_vec());
        let dir = tempfile::tempdir().unwrap();
        let c = capture(dir.path(), &files);
        let s = DeclarationSnapshot::with_limits(
            "fixture-owner".into(),
            &files,
            vec![fixture_root(&c, &files)],
            model_limits(&files),
        )
        .unwrap();
        let v = inputs(PATH, source);
        assert_eq!(v.len(), 1);
        assert_eq!(
            v[0].ancestry[0].span,
            ByteSpan {
                start: offset,
                end: offset + 13
            }
        );
        assert_eq!(
            v[0].ancestry[0].name_span,
            ByteSpan {
                start: offset + 4,
                end: offset + 5
            }
        );
        let d = derived(s.resolve_with_limits(&v[0]).unwrap());
        assert_eq!(d.binding().source_digest, content_digest(source));
        assert_eq!(d.binding().ancestry, v[0].ancestry);
    }
}
#[test]
fn config_marker_source_and_whole_inventory_changes_invalidate_bindings() {
    let files = inventory();
    let dir = tempfile::tempdir().unwrap();
    let c = capture(dir.path(), &files);
    let root = fixture_root(&c, &files);
    let policy = model_limits(&files);
    let s = DeclarationSnapshot::with_limits(
        "fixture-owner".into(),
        &files,
        vec![root.clone()],
        policy,
    )
    .unwrap();
    let v = inputs(PATH, SOURCE);
    let original = derived(s.resolve_with_limits(&v[0]).unwrap());
    for path in ["pyproject.toml", "src/harbor/__init__.py", PATH] {
        let mut changed = files.clone();
        changed
            .get_mut(path)
            .unwrap()
            .extend_from_slice(b"# changed\n");
        let mut updated = root.clone();
        updated.config_digest = content_digest(&changed["pyproject.toml"]);
        if path == "pyproject.toml" {
            assert!(matches!(
                DeclarationSnapshot::with_limits(
                    "fixture-owner".into(),
                    &changed,
                    vec![root.clone()],
                    model_limits(&changed)
                ),
                Err(DeclarationError::Model(_))
            ));
            std::fs::write(dir.path().join(path), &changed[path]).unwrap();
            assert!(c.verify(dir.path()).is_err());
            let recaptured = discover(
                dir.path(),
                BTreeSet::from([PATH.into()]),
                None,
                None,
                c.inputs(),
            )
            .unwrap();
            assert_eq!(fixture_root(&recaptured, &changed), updated);
        }
        let next = DeclarationSnapshot::with_limits(
            "fixture-owner".into(),
            &changed,
            vec![updated],
            model_limits(&changed),
        )
        .unwrap();
        assert!(!next.is_current_with_limits(&original).unwrap());
        if path == PATH {
            assert_eq!(
                next.resolve_with_limits(&v[0]).unwrap(),
                IdentityOutcome::Unavailable(IdentityReason::StaleSource)
            );
        }
    }
    let mut missing = files.clone();
    missing.remove("src/harbor/__init__.py");
    let next = DeclarationSnapshot::with_limits(
        "fixture-owner".into(),
        &missing,
        vec![root.clone()],
        model_limits(&missing),
    )
    .unwrap();
    assert_eq!(
        next.resolve_with_limits(&v[0]).unwrap(),
        IdentityOutcome::Unavailable(IdentityReason::NamespaceAncestry)
    );
    let mut collision = files.clone();
    collision.insert("src/harbor.py".into(), b"pass\n".to_vec());
    let next = DeclarationSnapshot::with_limits(
        "fixture-owner".into(),
        &collision,
        vec![root.clone()],
        model_limits(&collision),
    )
    .unwrap();
    assert_eq!(
        next.resolve_with_limits(&v[0]).unwrap(),
        IdentityOutcome::Unavailable(IdentityReason::ModulePackageCollision)
    );
    let mut unrelated = files.clone();
    unrelated.insert("empty".into(), vec![]);
    let next = DeclarationSnapshot::with_limits(
        "fixture-owner".into(),
        &unrelated,
        vec![root],
        model_limits(&unrelated),
    )
    .unwrap();
    assert!(!next.is_current_with_limits(&original).unwrap());
    assert!(s.is_current_with_limits(&original).unwrap());
}
#[test]
fn adapter_and_model_refusals_are_independent_and_leave_no_partial_binding() {
    let files = inventory();
    let dir = tempfile::tempdir().unwrap();
    let c = capture(dir.path(), &files);
    let root = fixture_root(&c, &files);
    let v = inputs(PATH, SOURCE);
    let exact = model_limits(&files);
    for (resource, policy) in [
        (
            ResourceKind::FileCount,
            DeclarationLimits {
                max_files: exact.max_files - 1,
                ..exact
            },
        ),
        (
            ResourceKind::TotalBytes,
            DeclarationLimits {
                max_total_bytes: exact.max_total_bytes - 1,
                ..exact
            },
        ),
        (
            ResourceKind::FileBytes,
            DeclarationLimits {
                max_file_bytes: exact.max_file_bytes - 1,
                ..exact
            },
        ),
        (
            ResourceKind::Roots,
            DeclarationLimits {
                max_roots: 0,
                ..exact
            },
        ),
    ] {
        refusal(
            DeclarationSnapshot::with_limits(
                "fixture-owner".into(),
                &files,
                vec![root.clone()],
                policy,
            ),
            resource,
        );
    }
    for (resource, policy, input) in [
        (
            ResourceKind::Evidence,
            DeclarationLimits {
                max_evidence: 0,
                ..exact
            },
            &v[0],
        ),
        (
            ResourceKind::AncestryDepth,
            DeclarationLimits {
                max_ancestry_depth: 2,
                ..exact
            },
            &v[2],
        ),
        (
            ResourceKind::IdentifierBytes,
            DeclarationLimits {
                max_identifier_bytes: 6,
                ..exact
            },
            &v[3],
        ),
    ] {
        let s = DeclarationSnapshot::with_limits(
            "fixture-owner".into(),
            &files,
            vec![root.clone()],
            policy,
        )
        .unwrap();
        refusal(s.resolve_with_limits(input), resource);
    }
    for (limits, reason) in [
        (
            PythonIdentityLimits {
                source_bytes: SOURCE.len() - 1,
                ..ast_limits()
            },
            PythonIdentityReason::SourceLimit,
        ),
        (
            PythonIdentityLimits {
                visited_nodes: 1,
                ..ast_limits()
            },
            PythonIdentityReason::WorkLimit,
        ),
        (
            PythonIdentityLimits {
                tree_depth: 1,
                ..ast_limits()
            },
            PythonIdentityReason::DepthLimit,
        ),
        (
            PythonIdentityLimits {
                declarations: 1,
                ..ast_limits()
            },
            PythonIdentityReason::OutputLimit,
        ),
        (
            PythonIdentityLimits {
                output_segments: 1,
                ..ast_limits()
            },
            PythonIdentityReason::OutputLimit,
        ),
        (
            PythonIdentityLimits {
                output_text_bytes: 1,
                ..ast_limits()
            },
            PythonIdentityReason::OutputLimit,
        ),
    ] {
        assert_eq!(
            declaration_inputs(PATH, SOURCE, limits).unwrap(),
            PythonIdentityOutcome::Unavailable(reason)
        );
    }
    assert_eq!(
        declaration_inputs(PATH, b"def ok(): pass\ndef broken(: pass\n", ast_limits()).unwrap(),
        PythonIdentityOutcome::Unavailable(PythonIdentityReason::SyntaxError)
    );
    let s = DeclarationSnapshot::with_limits("fixture-owner".into(), &files, vec![root], exact)
        .unwrap();
    assert!(s
        .is_current_with_limits(&derived(s.resolve_with_limits(&v[0]).unwrap()))
        .unwrap());
    assert_eq!(files[PATH], SOURCE);
}
#[test]
fn config_only_explicit_capture_does_not_prove_source_inventory() {
    let dir = tempfile::tempdir().unwrap();
    let files = inventory();
    let complete = capture(dir.path(), &files);
    let config_only = discover(
        dir.path(),
        BTreeSet::from([PATH.into()]),
        None,
        None,
        &ProjectInputs::default(),
    )
    .unwrap();
    assert_eq!(complete.model().python(), config_only.model().python());
    assert_eq!(complete.inputs().configs, config_only.inputs().configs);
    assert!(config_only.inputs().rust_sources.is_empty());
    // The real catalog was supplied by us; it omits a disk marker without tainting
    // config provenance. Do not promote this capture to DeclarationSnapshot.
    assert!(!config_only
        .model()
        .files()
        .files()
        .contains("src/harbor/__init__.py"));
    std::fs::write(
        dir.path().join("pyproject.toml"),
        b"[project]\nname='harbor'\n",
    )
    .unwrap();
    let inferred = discover(
        dir.path(),
        BTreeSet::from([PATH.into()]),
        None,
        None,
        &ProjectInputs::default(),
    )
    .unwrap();
    assert_eq!(
        inferred.model().python().provenance[""].state,
        PythonRootState::InferredDefaults
    );
}
