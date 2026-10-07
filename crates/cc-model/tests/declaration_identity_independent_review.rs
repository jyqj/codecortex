//! Independent, self-authored fixtures. No parser/index/evaluator or public corpus.
use cc_model::{declaration_identity::*, source::ByteSpan, StableId, SymbolRecord};
use std::collections::BTreeMap;

const CONFIG: &[u8] = b"collection-root = 'code'\n";
const SOURCE: &[u8] = b"class Tower:\n    class Room:\n        def ring(self): pass\n";
const PATH: &str = "code/harbor/bells.py";
fn root() -> ConfiguredRoot {
    ConfiguredRoot {
        directory: "code".into(),
        config_path: "capture.cfg".into(),
        config_digest: content_digest(CONFIG),
        directive: "collection-root".into(),
    }
}
fn files() -> BTreeMap<String, Vec<u8>> {
    BTreeMap::from([
        ("capture.cfg".into(), CONFIG.to_vec()),
        (
            "code/harbor/__init__.py".into(),
            b"raise RuntimeError('cannot import')\n".to_vec(),
        ),
        (PATH.into(), SOURCE.to_vec()),
    ])
}
fn snap(f: BTreeMap<String, Vec<u8>>, roots: Vec<ConfiguredRoot>) -> DeclarationSnapshot {
    DeclarationSnapshot::new("independent-review".into(), f, roots).unwrap()
}
fn seg(
    name: &str,
    kind: ScopeKind,
    start: usize,
    end: usize,
    name_start: usize,
) -> DeclarationSegment {
    DeclarationSegment {
        name: name.into(),
        kind,
        span: ByteSpan { start, end },
        name_span: ByteSpan {
            start: name_start,
            end: name_start + name.len(),
        },
    }
}
fn input() -> DeclarationInput {
    DeclarationInput {
        file_path: PATH.into(),
        source_digest: content_digest(SOURCE),
        ancestry: vec![
            seg("Tower", ScopeKind::Class, 0, SOURCE.len(), 6),
            seg("Room", ScopeKind::Class, 17, SOURCE.len(), 23),
            seg("ring", ScopeKind::Function, 37, SOURCE.len(), 41),
        ],
    }
}
fn derive(s: &DeclarationSnapshot, i: &DeclarationInput) -> BoundDeclaration {
    match s.resolve(i).unwrap() {
        IdentityOutcome::Derived(d) => *d,
        o => panic!("{o:?}"),
    }
}
fn reject(s: &DeclarationSnapshot, i: &DeclarationInput, r: IdentityReason) {
    assert_eq!(s.resolve(i).unwrap(), IdentityOutcome::Unavailable(r));
}

#[test]
fn nested_classes_and_runtime_failure_are_legitimate_logical_addresses() {
    let s = snap(files(), vec![root()]);
    let d = derive(&s, &input());
    assert_eq!(d.address().module, ["harbor", "bells"]);
    assert_eq!(d.address().lexical.len(), 3);
    assert!(s.is_current(&d).unwrap()); // No Python execution; package deliberately raises.
    let mut i = input();
    i.ancestry[0].kind = ScopeKind::Function;
    reject(&s, &i, IdentityReason::LocalDeclaration);
    i = input();
    i.ancestry[1].kind = ScopeKind::Function;
    reject(&s, &i, IdentityReason::LocalDeclaration);
}

#[test]
fn exhaustive_small_and_extreme_range_mutations_do_not_panic() {
    let s = snap(files(), vec![root()]);
    for span in [
        ByteSpan { start: 0, end: 0 },
        ByteSpan { start: 8, end: 2 },
        ByteSpan {
            start: 0,
            end: usize::MAX,
        },
        ByteSpan {
            start: usize::MAX,
            end: usize::MAX,
        },
        ByteSpan {
            start: usize::MAX,
            end: 0,
        },
    ] {
        for index in 0..3 {
            let mut i = input();
            i.ancestry[index].span = span;
            reject(&s, &i, IdentityReason::InvalidDeclaration);
            i = input();
            i.ancestry[index].name_span = span;
            reject(&s, &i, IdentityReason::InvalidDeclaration);
        }
    }
    for start in 0..=SOURCE.len() + 1 {
        for end in 0..=SOURCE.len() + 1 {
            let mut i = input();
            i.ancestry[2].name_span = ByteSpan { start, end };
            if start == 41 && end == 45 {
                derive(&s, &i);
            } else {
                reject(&s, &i, IdentityReason::InvalidDeclaration);
            }
        }
    }
    let mut i = input();
    i.ancestry[1].span.end = 40;
    reject(&s, &i, IdentityReason::InvalidDeclaration); // Child escapes parent.
    i = input();
    i.ancestry[2].span.start = 42;
    reject(&s, &i, IdentityReason::InvalidDeclaration); // Name escapes declaration.
}

#[test]
fn semantic_assertions_are_not_proven_by_matching_bytes() {
    let mut f = files();
    f.insert(PATH.into(), b"# Tower\n".to_vec());
    f.insert("capture.cfg".into(), b"# no directive here\n".to_vec());
    let mut r = root();
    r.config_digest = content_digest(b"# no directive here\n");
    let s = snap(f, vec![r]);
    let mut i = DeclarationInput {
        file_path: PATH.into(),
        source_digest: content_digest(b"# Tower\n"),
        ancestry: vec![seg("Tower", ScopeKind::Class, 0, 8, 2)],
    };
    let class = derive(&s, &i); // Deliberately false AST/config assertions, documented trust boundary.
    i.ancestry[0].kind = ScopeKind::Function;
    assert_ne!(class.address(), derive(&s, &i).address());
    i.ancestry = vec![i.ancestry[0].clone(); 2];
    reject(&s, &i, IdentityReason::LocalDeclaration);
    i.ancestry
        .iter_mut()
        .for_each(|a| a.kind = ScopeKind::Class);
    assert_eq!(derive(&s, &i).address().lexical.len(), 2); // Equal/repeated spans need adapter authenticity.
}

#[test]
fn same_name_conditional_occurrences_have_distinct_binding() {
    let bytes = b"if enabled:\n    def chime(): pass\nelse:\n    def chime(): pass\n";
    let mut f = files();
    f.insert(PATH.into(), bytes.to_vec());
    let s = snap(f, vec![root()]);
    let starts: Vec<_> = bytes
        .windows(5)
        .enumerate()
        .filter(|(_, w)| *w == b"chime")
        .map(|(n, _)| n)
        .collect();
    assert_eq!(starts.len(), 2);
    let declarations: Vec<_> = starts
        .iter()
        .map(|&n| {
            derive(
                &s,
                &DeclarationInput {
                    file_path: PATH.into(),
                    source_digest: content_digest(bytes),
                    ancestry: vec![seg("chime", ScopeKind::Function, n - 4, n + 13, n)],
                },
            )
        })
        .collect();
    assert_eq!(declarations[0].address(), declarations[1].address());
    assert_ne!(
        declarations[0].fingerprint().unwrap(),
        declarations[1].fingerprint().unwrap()
    );
    assert_eq!(
        declarations[0].fingerprint().unwrap(),
        declarations[0].fingerprint().unwrap()
    );
}

#[test]
fn fingerprints_are_order_independent_and_portable_inputs_normalize() {
    let f = files();
    let reverse = f
        .iter()
        .rev()
        .map(|(p, b)| (p.clone(), b.clone()))
        .collect();
    let a = derive(&snap(f, vec![root()]), &input());
    let mut r = root();
    r.directory = "./code//".into();
    let s = snap(reverse, vec![r]);
    let mut i = input();
    i.file_path = "./code\\harbor//bells.py".into();
    let b = derive(&s, &i);
    assert_eq!(a, b);
    assert_eq!(a.fingerprint().unwrap(), b.fingerprint().unwrap());
    // Independent reproduction of domain-separated wire hash, not a stable UID.
    let wire = serde_json::to_vec(&("source-bound-declaration-v1", &a)).unwrap();
    assert_eq!(
        a.fingerprint().unwrap(),
        blake3::hash(&wire).to_hex().to_string()
    );
}

#[test]
fn absent_default_duplicate_disjoint_roots_and_collection_semantics() {
    let f = files();
    let i = input();
    reject(
        &snap(f.clone(), vec![]),
        &i,
        IdentityReason::NoConfiguredRoot,
    );
    let mut other = root();
    other.directory = "elsewhere".into();
    for roots in [
        vec![root(), root()],
        vec![root(), other.clone()],
        vec![other, root()],
    ] {
        reject(&snap(f.clone(), roots), &i, IdentityReason::MultipleRoots);
    }
    let mut r = root();
    r.directory = "code/harbor".into();
    assert_eq!(
        derive(&snap(f.clone(), vec![r]), &i).address().module,
        ["bells"]
    );
    let mut r = root();
    r.directory = "cod".into();
    reject(&snap(f, vec![r]), &i, IdentityReason::OutsideRoot);
}

#[test]
fn missing_marker_collision_and_incomplete_inventory_assertion() {
    let original = derive(&snap(files(), vec![root()]), &input());
    for competitor in ["code/harbor.py", "code/harbor/bells/__init__.py"] {
        let mut f = files();
        f.insert(competitor.into(), vec![]);
        let s = snap(f, vec![root()]);
        reject(&s, &input(), IdentityReason::ModulePackageCollision);
        assert!(!s.is_current(&original).unwrap());
    }
    let mut f = files();
    f.remove("code/harbor/__init__.py");
    reject(
        &snap(f, vec![root()]),
        &input(),
        IdentityReason::NamespaceAncestry,
    );
    // The constructor cannot detect an omitted competitor: completeness is a capture assertion.
    assert!(snap(files(), vec![root()]).is_current(&original).unwrap());
}

#[test]
fn config_source_owner_and_inventory_changes_invalidate() {
    let d = derive(&snap(files(), vec![root()]), &input());
    for variant in 0..7 {
        let mut f = files();
        let mut r = root();
        match variant {
            0 => {
                f.insert("unrelated.bin".into(), vec![0, 255]);
            }
            1 => {
                let b = f.remove("capture.cfg").unwrap();
                f.insert("renamed.cfg".into(), b);
                r.config_path = "renamed.cfg".into();
            }
            2 => {
                f.insert("capture.cfg".into(), b"changed".to_vec());
                r.config_digest = content_digest(b"changed");
            }
            3 => {
                r.directive = "different-setting".into();
            }
            4 => {
                f.insert("code/harbor/__init__.py".into(), vec![]);
            }
            5 => {
                let b = f.remove(PATH).unwrap();
                f.insert("code/harbor/moved.py".into(), b);
            }
            _ => {
                f.get_mut(PATH).unwrap().push(b'\n');
            }
        }
        assert!(
            !snap(f, vec![r]).is_current(&d).unwrap(),
            "variant {variant}"
        );
    }
    let other = DeclarationSnapshot::new("other-owner".into(), files(), vec![root()]).unwrap();
    assert!(!other.is_current(&d).unwrap());
    let mut i = input();
    i.source_digest = "A".repeat(64);
    reject(
        &snap(files(), vec![root()]),
        &i,
        IdentityReason::StaleSource,
    );
}

#[test]
fn paths_and_constructor_limits_reject_bad_evidence() {
    let s = snap(files(), vec![root()]);
    for path in [
        "/x.py",
        "\\x.py",
        "//host/x.py",
        "C:x.py",
        "x/../y.py",
        "x\\..\\y.py",
        "x\n.py",
        "x\0.py",
    ] {
        let mut i = input();
        i.file_path = path.into();
        assert!(s.resolve(&i).is_err(), "{path:?}");
    }
    for path in ["", "./x", "x//y", "x/", "x\\y", "/x", "x/../y", "x\0y"] {
        let mut f = files();
        f.insert(path.into(), vec![]);
        assert!(DeclarationSnapshot::new("review".into(), f, vec![root()]).is_err());
    }
    for owner in [String::new(), "x".repeat(4097), "bad\nowner".into()] {
        assert!(DeclarationSnapshot::new(owner, files(), vec![root()]).is_err());
    }
    let mut f = files();
    f.insert("x".repeat(4097), vec![]);
    assert!(DeclarationSnapshot::new("review".into(), f, vec![root()]).is_err());
    for directive in [String::new(), "x".repeat(4097), "bad\0directive".into()] {
        let mut r = root();
        r.directive = directive;
        assert!(DeclarationSnapshot::new("review".into(), files(), vec![r]).is_err());
    }
    let mut r = root();
    r.config_path = "./capture.cfg".into();
    assert!(DeclarationSnapshot::new("review".into(), files(), vec![r]).is_err());
    let mut r = root();
    r.config_digest = "wrong".into();
    assert!(DeclarationSnapshot::new("review".into(), files(), vec![r]).is_err());
}

#[test]
fn bounded_resource_controls_and_duplicate_map_semantics() {
    let mut f = files();
    f.insert("payload.bin".into(), vec![0x5a; 1024 * 1024]);
    for n in 0..256 {
        f.insert(format!("inventory/{n}.bin"), vec![n as u8; 64]);
    }
    let s = snap(f.clone(), vec![root()]);
    let d = derive(&s, &input());
    assert!(s.is_current(&d).unwrap());
    f.get_mut("payload.bin").unwrap()[0] ^= 1;
    assert!(!snap(f, vec![root()]).is_current(&d).unwrap());
    // BTreeMap overwrites duplicate keys before the constructor sees them.
    let mut f = files();
    assert!(f.insert(PATH.into(), SOURCE.to_vec()).is_some());
    assert_eq!(
        derive(&snap(f, vec![root()]), &input()).address(),
        d.address()
    );
    let mut i = input();
    let class = i.ancestry[0].clone();
    i.ancestry = vec![class; 256];
    assert_eq!(derive(&s, &i).address().lexical.len(), 256); // No ancestry quota; bounded probe only.
}

#[test]
fn legacy_wire_and_uid_have_independent_expected_values() {
    let legacy = serde_json::json!({"symbol_id":"legacy", "file_path":PATH, "name":"ring", "kind":"method",
        "container":"Tower.Room", "start_line":3, "end_line":3, "start_col":8, "end_col":28,
        "parser_tier":"tree_sitter", "parser_confidence":1.0, "qname":"Tower.Room.ring", "is_default_export":false});
    let record: SymbolRecord = serde_json::from_value(legacy.clone()).unwrap();
    let before = serde_json::to_value(&record).unwrap();
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"sym:code/harbor/bells.py\0Tower.Room.ring\0method");
    let expected = format!("uid:{}", &hasher.finalize().to_hex()[..24]);
    assert_eq!(
        StableId::symbol_uid(PATH, "Tower.Room.ring", "method", None),
        expected
    );
    derive(&snap(files(), vec![root()]), &input());
    assert_eq!(serde_json::to_value(record).unwrap(), before);
    for (key, value) in legacy.as_object().unwrap() {
        assert_eq!(&before[key], value);
    }
    assert!(before.get("declaration_identity").is_none());
}
