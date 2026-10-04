//! Self-authored model fixtures only; no production indexing or evaluation.
use cc_model::{declaration_identity::*, source::ByteSpan, StableId, SymbolRecord};
use std::collections::BTreeMap;

const CONFIG: &[u8] = b"[tool.setuptools.package-dir]\n\"\" = \"src\"\n";
const SOURCE: &[u8] = b"class Beacon:\n    def pulse(self):\n        pass\n";

fn files() -> BTreeMap<String, Vec<u8>> {
    BTreeMap::from([
        ("pyproject.toml".into(), CONFIG.to_vec()),
        ("src/beacon/__init__.py".into(), b"# package\n".to_vec()),
        ("src/beacon/signals.py".into(), SOURCE.to_vec()),
    ])
}
fn root() -> ConfiguredRoot {
    ConfiguredRoot {
        directory: "src".into(),
        config_path: "pyproject.toml".into(),
        config_digest: content_digest(CONFIG),
        directive: "tool.setuptools.package-dir.empty".into(),
    }
}
fn snapshot(files: BTreeMap<String, Vec<u8>>, roots: Vec<ConfiguredRoot>) -> DeclarationSnapshot {
    DeclarationSnapshot::new("fixture-repository".into(), files, roots).unwrap()
}
fn segment(bytes: &[u8], name: &str, kind: ScopeKind, start: usize) -> DeclarationSegment {
    let name_start = bytes
        .windows(name.len())
        .position(|w| w == name.as_bytes())
        .unwrap();
    DeclarationSegment {
        name: name.into(),
        kind,
        span: ByteSpan {
            start,
            end: bytes.len(),
        },
        name_span: ByteSpan {
            start: name_start,
            end: name_start + name.len(),
        },
    }
}
fn input() -> DeclarationInput {
    DeclarationInput {
        file_path: "src/beacon/signals.py".into(),
        source_digest: content_digest(SOURCE),
        ancestry: vec![
            segment(SOURCE, "Beacon", ScopeKind::Class, 0),
            segment(SOURCE, "pulse", ScopeKind::Function, 14),
        ],
    }
}
fn derived(s: &DeclarationSnapshot, i: &DeclarationInput) -> BoundDeclaration {
    match s.resolve(i).unwrap() {
        IdentityOutcome::Derived(d) => *d,
        other => panic!("expected derived, got {other:?}"),
    }
}
fn reason(s: &DeclarationSnapshot, i: &DeclarationInput, expected: IdentityReason) {
    assert_eq!(
        s.resolve(i).unwrap(),
        IdentityOutcome::Unavailable(expected)
    );
}

#[test]
fn regular_package_method_has_typed_source_bound_address() {
    let d = derived(&snapshot(files(), vec![root()]), &input());
    assert_eq!(d.address().schema, IdentitySchema::PythonDeclarationV1);
    assert_eq!(d.address().module, ["beacon", "signals"]);
    assert_eq!(
        d.address().lexical,
        [
            ("Beacon".into(), ScopeKind::Class),
            ("pulse".into(), ScopeKind::Function)
        ]
    );
    assert_eq!(d.binding().source_digest, content_digest(SOURCE));
    assert_eq!(
        d.binding().package_files["src/beacon/__init__.py"],
        content_digest(b"# package\n")
    );
    assert_eq!(
        serde_json::to_value(&d).unwrap()["address"]["schema"],
        "python_declaration_v1"
    );
}

#[test]
fn initializer_denotes_package_and_root_initializer_is_unavailable() {
    let mut f = files();
    f.insert("src/beacon/__init__.py".into(), SOURCE.to_vec());
    let mut i = input();
    i.file_path = "src/beacon/__init__.py".into();
    assert_eq!(
        derived(&snapshot(f.clone(), vec![root()]), &i)
            .address()
            .module,
        ["beacon"]
    );
    f.insert("src/__init__.py".into(), SOURCE.to_vec());
    i.file_path = "src/__init__.py".into();
    reason(
        &snapshot(f, vec![root()]),
        &i,
        IdentityReason::RootInitializer,
    );
}

#[test]
fn top_level_module_empty_root_and_nested_regular_packages() {
    let mut f = files();
    f.insert("standalone.py".into(), SOURCE.to_vec());
    let mut r = root();
    r.directory = ".".into();
    let mut i = input();
    i.file_path = "standalone.py".into();
    assert_eq!(
        derived(&snapshot(f.clone(), vec![r]), &i).address().module,
        ["standalone"]
    );
    f.insert("src/beacon/sub/__init__.py".into(), b"".to_vec());
    f.insert("src/beacon/sub/signals.py".into(), SOURCE.to_vec());
    i.file_path = "src/beacon/sub/signals.py".into();
    assert_eq!(
        derived(&snapshot(f, vec![root()]), &i).address().module,
        ["beacon", "sub", "signals"]
    );
}

#[test]
fn roots_never_use_defaults_order_or_longest_prefix() {
    let f = files();
    let i = input();
    reason(
        &snapshot(f.clone(), vec![]),
        &i,
        IdentityReason::NoConfiguredRoot,
    );
    let mut second = root();
    second.directory = "src/beacon".into();
    for roots in [
        vec![root(), second.clone()],
        vec![second, root()],
        vec![root(), root()],
    ] {
        reason(
            &snapshot(f.clone(), roots),
            &i,
            IdentityReason::MultipleRoots,
        );
    }
    let mut r = root();
    r.directory = "sr".into();
    reason(&snapshot(f, vec![r]), &i, IdentityReason::OutsideRoot);
}

#[test]
fn config_evidence_must_be_bound_to_captured_bytes() {
    let mut r = root();
    r.config_digest = content_digest(b"other config");
    assert!(DeclarationSnapshot::new("fixture".into(), files(), vec![r]).is_err());
    let mut r = root();
    r.config_path = "absent.toml".into();
    assert!(DeclarationSnapshot::new("fixture".into(), files(), vec![r]).is_err());
    let mut r = root();
    r.directive.clear();
    assert!(DeclarationSnapshot::new("fixture".into(), files(), vec![r]).is_err());
    let mut r = root();
    r.directory = "../src".into();
    assert!(DeclarationSnapshot::new("fixture".into(), files(), vec![r]).is_err());
}

#[test]
fn namespace_and_file_package_collisions_are_explicit() {
    let mut f = files();
    f.remove("src/beacon/__init__.py");
    reason(
        &snapshot(f, vec![root()]),
        &input(),
        IdentityReason::NamespaceAncestry,
    );
    for competitor in ["src/beacon.py", "src/beacon/signals/__init__.py"] {
        let mut f = files();
        f.insert(competitor.into(), b"".to_vec());
        reason(
            &snapshot(f, vec![root()]),
            &input(),
            IdentityReason::ModulePackageCollision,
        );
    }
}

#[test]
fn dotted_invalid_and_non_python_filenames_are_not_guessed() {
    for path in [
        "src/beacon/a.b.py",
        "src/beacon/1signal.py",
        "src/beacon/with.py",
        "src/beacon/signals.pyi",
        "src/beacon/signals.rs",
    ] {
        let mut f = files();
        f.insert(path.into(), SOURCE.to_vec());
        let mut i = input();
        i.file_path = path.into();
        reason(
            &snapshot(f, vec![root()]),
            &i,
            IdentityReason::UnsupportedFile,
        );
    }
    let mut f = files();
    f.insert("src/测/__init__.py".into(), b"".to_vec());
    f.insert("src/测/signals.py".into(), SOURCE.to_vec());
    let mut i = input();
    i.file_path = "src/测/signals.py".into();
    reason(
        &snapshot(f, vec![root()]),
        &i,
        IdentityReason::UnsupportedIdentifier,
    );
}

#[test]
fn normalization_and_inventory_aliases_obey_repository_path_contract() {
    let s = snapshot(files(), vec![root()]);
    let canonical = derived(&s, &input());
    let mut i = input();
    i.file_path = "./src\\beacon//signals.py".into();
    assert_eq!(derived(&s, &i), canonical);
    let mut r = root();
    r.directory = "./src//".into();
    assert_eq!(derived(&snapshot(files(), vec![r]), &input()), canonical);
    for path in [
        "/src/beacon/signals.py",
        "C:src/signals.py",
        "../src/beacon/signals.py",
        "src/../src/beacon/signals.py",
        "src\0x.py",
    ] {
        i.file_path = path.into();
        assert!(s.resolve(&i).is_err());
    }
    i.file_path = "SRC/beacon/signals.py".into();
    reason(&s, &i, IdentityReason::OutsideRoot);
    let mut f = files();
    f.insert("src\\beacon\\signals.py".into(), SOURCE.to_vec());
    assert!(DeclarationSnapshot::new("fixture".into(), f, vec![root()]).is_err());
}

#[test]
fn source_and_parser_witnesses_are_validated() {
    let s = snapshot(files(), vec![root()]);
    let mut i = input();
    i.source_digest = content_digest(b"stale");
    reason(&s, &i, IdentityReason::StaleSource);
    i = input();
    i.file_path = "src/beacon/absent.py".into();
    reason(&s, &i, IdentityReason::MissingSource);
    i = input();
    i.ancestry.clear();
    reason(&s, &i, IdentityReason::InvalidDeclaration);
    i = input();
    i.ancestry[1].span.end = SOURCE.len() + 1;
    reason(&s, &i, IdentityReason::InvalidDeclaration);
    i = input();
    i.ancestry[1].name = "other".into();
    reason(&s, &i, IdentityReason::InvalidDeclaration);
    i = input();
    i.ancestry[1].name_span.start = usize::MAX;
    reason(&s, &i, IdentityReason::InvalidDeclaration);
    i = input();
    i.ancestry[1].span = ByteSpan { start: 20, end: 2 };
    reason(&s, &i, IdentityReason::InvalidDeclaration);
    i = input();
    i.ancestry[1].name = "a.b".into();
    reason(&s, &i, IdentityReason::UnsupportedIdentifier);
}

#[test]
fn nested_local_function_and_local_class_are_not_runtime_names() {
    const LOCAL: &[u8] =
        b"def outer():\n    def inner():\n        pass\n    class Local:\n        pass\n";
    let mut f = files();
    f.insert("src/beacon/local.py".into(), LOCAL.to_vec());
    let s = snapshot(f, vec![root()]);
    for (name, kind, start) in [
        ("inner", ScopeKind::Function, 13),
        ("Local", ScopeKind::Class, 42),
    ] {
        let i = DeclarationInput {
            file_path: "src/beacon/local.py".into(),
            source_digest: content_digest(LOCAL),
            ancestry: vec![
                segment(LOCAL, "outer", ScopeKind::Function, 0),
                segment(LOCAL, name, kind, start),
            ],
        };
        reason(&s, &i, IdentityReason::LocalDeclaration);
    }
}

#[test]
fn relative_imports_and_reexports_do_not_mint_alias_addresses() {
    let original = derived(&snapshot(files(), vec![root()]), &input());
    let mut f = files();
    f.insert(
        "src/beacon/__init__.py".into(),
        b"from .signals import Beacon as Alias\n".to_vec(),
    );
    f.insert(
        "src/beacon/consumer.py".into(),
        b"from . import Alias\n".to_vec(),
    );
    let changed = derived(&snapshot(f, vec![root()]), &input());
    assert_eq!(original.address(), changed.address());
    assert_ne!(
        original.fingerprint().unwrap(),
        changed.fingerprint().unwrap()
    );
    assert_eq!(changed.address().module, ["beacon", "signals"]);
}

#[test]
fn rename_boundary_config_and_source_mutations_invalidate_binding() {
    let original = derived(&snapshot(files(), vec![root()]), &input());
    assert_eq!(
        original,
        derived(&snapshot(files(), vec![root()]), &input())
    );
    let mut f = files();
    f.insert("src/beacon/__init__.py".into(), b"# changed\n".to_vec());
    let changed = derived(&snapshot(f, vec![root()]), &input());
    assert_eq!(original.address(), changed.address());
    assert_ne!(
        original.fingerprint().unwrap(),
        changed.fingerprint().unwrap()
    );
    let mut f = files();
    f.insert("pyproject.toml".into(), b"# new root evidence\n".to_vec());
    let mut r = root();
    r.config_digest = content_digest(b"# new root evidence\n");
    assert_ne!(
        original.fingerprint().unwrap(),
        derived(&snapshot(f, vec![r]), &input())
            .fingerprint()
            .unwrap()
    );
    let mut f = files();
    let moved = f.remove("src/beacon/signals.py").unwrap();
    f.insert("src/beacon/renamed.py".into(), moved);
    let s = snapshot(f, vec![root()]);
    reason(&s, &input(), IdentityReason::MissingSource);
    let mut i = input();
    i.file_path = "src/beacon/renamed.py".into();
    assert_ne!(original.address(), derived(&s, &i).address());
    let mut f = files();
    let mut source = SOURCE.to_vec();
    source.extend_from_slice(b"# drift\n");
    f.insert("src/beacon/signals.py".into(), source.clone());
    let s = snapshot(f, vec![root()]);
    reason(&s, &input(), IdentityReason::StaleSource);
    let mut i = input();
    i.source_digest = content_digest(&source);
    let changed = derived(&s, &i);
    assert_eq!(original.address(), changed.address());
    assert_ne!(
        original.fingerprint().unwrap(),
        changed.fingerprint().unwrap()
    );
}

#[test]
fn owner_and_occurrence_are_not_conflated_with_logical_address() {
    const DUP: &[u8] = b"def pulse(): pass\ndef pulse(): pass\n";
    let mut f = files();
    f.insert("src/beacon/duplicate.py".into(), DUP.to_vec());
    let s = snapshot(f.clone(), vec![root()]);
    let mut i = DeclarationInput {
        file_path: "src/beacon/duplicate.py".into(),
        source_digest: content_digest(DUP),
        ancestry: vec![DeclarationSegment {
            name: "pulse".into(),
            kind: ScopeKind::Function,
            span: ByteSpan { start: 0, end: 17 },
            name_span: ByteSpan { start: 4, end: 9 },
        }],
    };
    let a = derived(&s, &i);
    i.ancestry[0].span = ByteSpan { start: 18, end: 35 };
    i.ancestry[0].name_span = ByteSpan { start: 22, end: 27 };
    let b = derived(&s, &i);
    assert_eq!(a.address(), b.address());
    assert_ne!(a.fingerprint().unwrap(), b.fingerprint().unwrap());
    let other_owner = DeclarationSnapshot::new("other-repository".into(), f, vec![root()]).unwrap();
    assert_ne!(b.address(), derived(&other_owner, &i).address());
}

#[test]
fn lexical_qname_uid_and_legacy_symbol_wire_remain_unchanged() {
    let wire = serde_json::json!({"symbol_id":"s", "file_path":"src/beacon/signals.py", "name":"pulse",
        "kind":"method", "container":"Beacon", "start_line":2,"end_line":3,"start_col":4,"end_col":12,
        "parser_tier":"tree_sitter", "parser_confidence":1.0, "qname":"Beacon.pulse", "is_default_export":false});
    let legacy: SymbolRecord = serde_json::from_value(wire).unwrap();
    let before = serde_json::to_value(&legacy).unwrap();
    let uid = StableId::symbol_uid(
        &legacy.file_path,
        legacy.qname.as_deref().unwrap(),
        "method",
        None,
    );
    derived(&snapshot(files(), vec![root()]), &input());
    assert_eq!(before, serde_json::to_value(&legacy).unwrap());
    assert_eq!(legacy.qname.as_deref(), Some("Beacon.pulse"));
    assert_eq!(
        uid,
        StableId::symbol_uid(&legacy.file_path, "Beacon.pulse", "method", None)
    );
    assert_ne!(
        uid,
        StableId::symbol_uid(
            &legacy.file_path,
            "beacon.signals.Beacon.pulse",
            "method",
            None
        )
    );
    assert!(before.get("declaration_identity").is_none());
}

#[test]
fn current_guard_rejects_root_package_and_inventory_mutations() {
    let s = snapshot(files(), vec![root()]);
    let original = derived(&s, &input());
    assert!(s.is_current(&original).unwrap());
    let mut f = files();
    f.remove("src/beacon/__init__.py");
    assert!(!snapshot(f, vec![root()]).is_current(&original).unwrap());
    let mut f = files();
    f.insert("src/beacon.py".into(), b"# competitor".to_vec());
    assert!(!snapshot(f, vec![root()]).is_current(&original).unwrap());
    let mut f = files();
    f.insert(
        "unrelated.txt".into(),
        b"# conservative invalidation".to_vec(),
    );
    assert!(!snapshot(f, vec![root()]).is_current(&original).unwrap());
    let mut r = root();
    r.directive = "different explicit policy".into();
    assert!(!snapshot(files(), vec![r]).is_current(&original).unwrap());
    let mut f = files();
    let marker = f.remove("src/beacon/__init__.py").unwrap();
    let source = f.remove("src/beacon/signals.py").unwrap();
    f.insert("src/lantern/__init__.py".into(), marker.clone());
    f.insert("src/lantern/signals.py".into(), source.clone());
    let renamed = snapshot(f, vec![root()]);
    assert!(!renamed.is_current(&original).unwrap());
    let mut i = input();
    i.file_path = "src/lantern/signals.py".into();
    assert_eq!(
        derived(&renamed, &i).address().module,
        ["lantern", "signals"]
    );
    let mut f = files();
    f.remove("src/beacon/__init__.py");
    f.remove("src/beacon/signals.py");
    f.insert("lib/beacon/__init__.py".into(), marker);
    f.insert("lib/beacon/signals.py".into(), source);
    let new_config = b"[tool.setuptools.package-dir]\n\"\" = \"lib\"\n";
    f.insert("pyproject.toml".into(), new_config.to_vec());
    let mut r = root();
    r.directory = "lib".into();
    r.config_digest = content_digest(new_config);
    let moved = snapshot(f, vec![r]);
    assert!(!moved.is_current(&original).unwrap());
    i.file_path = "lib/beacon/signals.py".into();
    let rebound = derived(&moved, &i);
    assert_eq!(rebound.address().module, original.address().module);
    assert_ne!(rebound.address(), original.address());
    assert!(moved.is_current(&rebound).unwrap());
}
