//! Authored temporary filesystem fixtures. No serialized declaration is admitted.
#![cfg(target_os = "linux")]
use cc_index::project_model::python_inventory::*;
use cc_model::declaration_identity::{DeclarationLimits, IdentityOutcome, IdentityReason};
use cc_parsers::python_identity::{PythonIdentityLimits, PythonIdentityReason};
use std::path::Path;

const OWNER: &str = "review owner \\\"星\"";
const CONFIG: &[u8] = b"# original bytes\n[tool.setuptools.package-dir]\n\"\"='./src'\n[tool.setuptools.packages.find]\nwhere=['src/./']\n";
const SOURCE: &[u8] = b"\xef\xbb\xbf\r\nclass Harbor:\r\n    def tide(self): return 1\r\ndef factory():\r\n    class Local: pass\r\n";
const FILE: &str = "src/ocean/wave.py";

fn write(root: &Path, path: &str, bytes: &[u8]) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}
fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (path, bytes) in [
        ("pyproject.toml", CONFIG),
        (FILE, SOURCE),
        (
            "src/ocean/__init__.py",
            b"raise RuntimeError('unexecuted')\n",
        ),
        (".gitignore", b"__init__.py\n.cache/\n"),
        (".cache/opaque", b"\xff\x00note"),
    ] {
        write(dir.path(), path, bytes);
    }
    std::fs::create_dir(dir.path().join("empty")).unwrap();
    dir
}
fn policies() -> AdmissionPolicies {
    AdmissionPolicies {
        capture: CaptureLimits {
            entries: 32,
            files: 16,
            total_bytes: 4096,
            file_bytes: 2048,
            depth: 8,
            path_bytes: 128,
            total_path_bytes: 2048,
            output_count: 16,
            output_bytes: 32768,
        },
        ast: PythonIdentityLimits {
            source_bytes: 2048,
            visited_nodes: 512,
            tree_depth: 32,
            declarations: 16,
            output_segments: 32,
            output_text_bytes: 4096,
            parse_timeout_micros: 1_000_000,
        },
        declarations: DeclarationLimits {
            max_files: 16,
            max_total_bytes: 4096,
            max_file_bytes: 2048,
            max_roots: 1,
            max_evidence: 8,
            max_ancestry_depth: 8,
            max_identifier_bytes: 32,
        },
    }
}
fn request() -> CaptureRequest<'static> {
    CaptureRequest {
        owner: OWNER,
        scope: AuthorizedScope::EntireProject,
        exclusions: &[],
    }
}
fn capture(root: &Path) -> CapturedDeclarations {
    capture_python_declarations(root, request(), policies()).unwrap()
}
fn wire(receipt: &CaptureReceipt) -> Vec<u8> {
    serde_json::to_vec(receipt).unwrap()
}
fn revalidate(
    root: &Path,
    receipt: &CaptureReceipt,
) -> Result<CapturedDeclarations, RevalidationRefusal> {
    revalidate_python_declarations(root, request(), policies(), &wire(receipt))
}

#[test]
fn round_trip_rederives_original_bytes_config_provenance_and_all_outcomes() {
    let dir = fixture();
    let old = capture(dir.path());
    let receipt = old.receipt().unwrap();
    assert_eq!(receipt.version, 1);
    assert_eq!(receipt.derivation_version, 1);
    assert_eq!(receipt.owner, OWNER);
    assert_eq!(receipt.scope, "entire_project");
    let saved = wire(&receipt);
    // The receipt does not contain source bytes or serialized declaration objects.
    let object: serde_json::Value = serde_json::from_slice(&saved).unwrap();
    assert_eq!(object.as_object().unwrap().len(), 7);
    assert!(object.get("outcomes").is_none());
    let fresh = revalidate_python_declarations(dir.path(), request(), policies(), &saved).unwrap();
    assert_eq!(fresh.inventory(), old.inventory());
    assert_eq!(fresh.inventory()[FILE], SOURCE);
    assert_eq!(fresh.config(), old.config());
    assert_eq!(fresh.provenance(), old.provenance());
    assert_eq!(fresh.outcomes(), old.outcomes());
    assert_eq!(fresh.receipt().unwrap(), receipt);
    let values = &fresh.outcomes()[FILE];
    assert_eq!(values.len(), 4);
    assert_eq!(
        values[3],
        IdentityOutcome::Unavailable(IdentityReason::LocalDeclaration)
    );
    let IdentityOutcome::Derived(method) = &values[1] else {
        panic!("expected rederived method")
    };
    assert_eq!(method.address().owner, OWNER);
    assert_eq!(method.address().module, ["ocean", "wave"]);
    assert_eq!(method.address().lexical[0].0, "Harbor");
    assert_eq!(method.address().lexical[1].0, "tide");
}

#[test]
fn configuration_raw_bytes_and_normalized_directives_remain_bound() {
    let dir = fixture();
    let old = capture(dir.path());
    let receipt = old.receipt().unwrap();
    for config in [
        b"# a different comment\n[tool.setuptools.package-dir]\n\"\"='./src'\n[tool.setuptools.packages.find]\nwhere=['src/./']\n".as_slice(),
        b"[tool.setuptools.package-dir]\n\"\"='src'\n[tool.setuptools.packages.find]\nwhere=['./src','src']\n",
    ] {
        write(dir.path(), "pyproject.toml", config);
        assert!(matches!(
            revalidate(dir.path(), &receipt),
            Err(RevalidationRefusal::Mismatch(ReceiptMismatch::Configuration))
        ));
    }
    assert_eq!(old.inventory()["pyproject.toml"], CONFIG);
    write(dir.path(), "pyproject.toml", CONFIG);
    revalidate(dir.path(), &receipt).unwrap();
}

#[test]
fn full_inventory_binds_sources_markers_ignored_files_additions_and_renames() {
    let dir = fixture();
    let old = capture(dir.path());
    let receipt = old.receipt().unwrap();
    for (path, bytes) in [
        (FILE, b"def changed(): return 2\n".as_slice()),
        ("src/ocean/__init__.py", b"pass\n"),
        (".cache/opaque", b"opaque change\xff"),
    ] {
        write(dir.path(), path, bytes);
        assert!(
            matches!(
                revalidate(dir.path(), &receipt),
                Err(RevalidationRefusal::Mismatch(ReceiptMismatch::Inventory))
            ),
            "{path}"
        );
        write(dir.path(), path, &old.inventory()[path]);
    }
    std::fs::remove_file(dir.path().join("src/ocean/__init__.py")).unwrap();
    assert!(capture(dir.path()).outcomes()[FILE][..3]
        .iter()
        .all(|value| *value == IdentityOutcome::Unavailable(IdentityReason::NamespaceAncestry)));
    assert!(matches!(
        revalidate(dir.path(), &receipt),
        Err(RevalidationRefusal::Mismatch(ReceiptMismatch::Inventory))
    ));
    write(
        dir.path(),
        "src/ocean/__init__.py",
        &old.inventory()["src/ocean/__init__.py"],
    );
    write(dir.path(), "src/ocean.py", b"pass\n");
    assert!(capture(dir.path()).outcomes()[FILE][..3].iter().all(
        |value| *value == IdentityOutcome::Unavailable(IdentityReason::ModulePackageCollision)
    ));
    assert!(matches!(
        revalidate(dir.path(), &receipt),
        Err(RevalidationRefusal::Mismatch(ReceiptMismatch::Inventory))
    ));
    std::fs::remove_file(dir.path().join("src/ocean.py")).unwrap();
    std::fs::rename(
        dir.path().join(FILE),
        dir.path().join("src/ocean/renamed.py"),
    )
    .unwrap();
    assert!(matches!(
        revalidate(dir.path(), &receipt),
        Err(RevalidationRefusal::Mismatch(ReceiptMismatch::Inventory))
    ));
    assert_eq!(old.inventory()[FILE], SOURCE);
}

#[test]
fn each_receipt_digest_is_checked_against_fresh_derivation() {
    let dir = fixture();
    let receipt = capture(dir.path()).receipt().unwrap();
    for component in [
        ReceiptMismatch::Configuration,
        ReceiptMismatch::Inventory,
        ReceiptMismatch::Outcomes,
    ] {
        let mut forged = receipt.clone();
        let field = match component {
            ReceiptMismatch::Configuration => &mut forged.configuration_digest,
            ReceiptMismatch::Inventory => &mut forged.inventory_digest,
            ReceiptMismatch::Outcomes => &mut forged.outcomes_digest,
        };
        // Still well-formed hex, so this cannot pass only a wire-shape check.
        field.replace_range(0..1, if field.starts_with('0') { "1" } else { "0" });
        assert!(matches!(
            revalidate(dir.path(), &forged),
            Err(RevalidationRefusal::Mismatch(actual)) if actual == component
        ));
    }
}

#[test]
fn version_owner_and_scope_mismatch_refuse_before_filesystem_capture() {
    let dir = fixture();
    let receipt = capture(dir.path()).receipt().unwrap();
    let missing_root = dir.path().join("no-such-project");
    for (wire_version, derivation_version) in [(0, 1), (2, 1), (1, 0), (1, 2)] {
        let mut changed = receipt.clone();
        changed.version = wire_version;
        changed.derivation_version = derivation_version;
        assert!(matches!(
            revalidate(&missing_root, &changed),
            Err(RevalidationRefusal::UnsupportedVersion)
        ));
    }
    let mut changed = receipt.clone();
    changed.owner = "another owner".into();
    assert!(matches!(
        revalidate(&missing_root, &changed),
        Err(RevalidationRefusal::OwnerMismatch)
    ));
    changed = receipt.clone();
    changed.scope = "subtree:src".into();
    assert!(matches!(
        revalidate(&missing_root, &changed),
        Err(RevalidationRefusal::ScopeMismatch)
    ));
    // A correct receipt still needs a readable project; it is not an offline cache.
    assert!(matches!(
        revalidate(&missing_root, &receipt),
        Err(RevalidationRefusal::Capture(CaptureRefusal::Io))
    ));
}

#[test]
fn missing_duplicate_unknown_and_malformed_fields_cannot_assert_outputs() {
    let dir = fixture();
    let receipt = capture(dir.path()).receipt().unwrap();
    let missing_root = dir.path().join("absent");
    let value = serde_json::to_value(&receipt).unwrap();
    let valid = String::from_utf8(wire(&receipt)).unwrap();
    let mut cases = vec![
        "{}".to_owned(),
        "[]".to_owned(),
        "null".to_owned(),
        "{".to_owned(),
        format!("{valid} {{}}"),
        format!("{{\"version\":1,{}", &valid[1..]),
    ];
    for key in value.as_object().unwrap().keys() {
        let mut missing = value.clone();
        missing.as_object_mut().unwrap().remove(key);
        cases.push(missing.to_string());
    }
    let mut injected = value.clone();
    injected["outcomes"] = serde_json::json!({FILE: [{"status": "derived", "value": {}}]});
    cases.push(injected.to_string());
    for bad_digest in ["", "abc", &"G".repeat(64), &"0".repeat(65)] {
        let mut changed = value.clone();
        changed["outcomes_digest"] = bad_digest.into();
        cases.push(changed.to_string());
    }
    for (key, invalid) in [
        ("version", serde_json::json!("1")),
        ("owner", serde_json::json!([OWNER])),
        ("inventory_digest", serde_json::Value::Null),
    ] {
        let mut changed = value.clone();
        changed[key] = invalid;
        cases.push(changed.to_string());
    }
    for json in cases {
        assert!(
            matches!(
                revalidate_python_declarations(
                    &missing_root,
                    request(),
                    policies(),
                    json.as_bytes()
                ),
                Err(RevalidationRefusal::MalformedReceipt)
            ),
            "{json}"
        );
    }
}

#[test]
fn receipt_byte_ceiling_accepts_exact_size_and_refuses_one_over_before_io() {
    let dir = fixture();
    let receipt = capture(dir.path()).receipt().unwrap();
    let mut bytes = wire(&receipt);
    bytes.resize(CAPTURE_RECEIPT_MAX_BYTES, b' ');
    revalidate_python_declarations(dir.path(), request(), policies(), &bytes).unwrap();
    bytes.push(b' ');
    assert!(matches!(
        revalidate_python_declarations(&dir.path().join("absent"), request(), policies(), &bytes),
        Err(RevalidationRefusal::ReceiptBytes { actual, limit })
            if actual == CAPTURE_RECEIPT_MAX_BYTES + 1 && limit == CAPTURE_RECEIPT_MAX_BYTES
    ));
}

#[test]
fn caller_authorization_and_each_current_budget_are_reapplied() {
    let dir = fixture();
    let saved = wire(&capture(dir.path()).receipt().unwrap());
    let exclusions = [".cache".to_owned()];
    for denied in [
        CaptureRequest {
            scope: AuthorizedScope::Subtree("src".into()),
            ..request()
        },
        CaptureRequest {
            exclusions: &exclusions,
            ..request()
        },
    ] {
        assert!(matches!(
            revalidate_python_declarations(dir.path(), denied, policies(), &saved),
            Err(RevalidationRefusal::Capture(
                CaptureRefusal::UnauthorizedScope
            ))
        ));
    }
    let mut capture_limit = policies();
    capture_limit.capture.files = 1;
    assert!(matches!(
        revalidate_python_declarations(dir.path(), request(), capture_limit, &saved),
        Err(RevalidationRefusal::Capture(CaptureRefusal::Budget(
            "files"
        )))
    ));
    let mut ast_limit = policies();
    ast_limit.ast.declarations = 1;
    assert!(matches!(
        revalidate_python_declarations(dir.path(), request(), ast_limit, &saved),
        Err(RevalidationRefusal::Capture(CaptureRefusal::Ast {
            reason: PythonIdentityReason::OutputLimit,
            ..
        }))
    ));
    let mut model_limit = policies();
    model_limit.declarations.max_identifier_bytes = 3;
    assert!(matches!(
        revalidate_python_declarations(dir.path(), request(), model_limit, &saved),
        Err(RevalidationRefusal::Capture(CaptureRefusal::Model(_)))
    ));
}

#[test]
fn valid_old_receipt_never_bypasses_config_ast_or_native_refusal() {
    let dir = fixture();
    let receipt = capture(dir.path()).receipt().unwrap();
    write(dir.path(), FILE, b"def valid(): pass\ndef broken(: pass\n");
    assert!(matches!(
        revalidate(dir.path(), &receipt),
        Err(RevalidationRefusal::Capture(CaptureRefusal::Ast {
            reason: PythonIdentityReason::SyntaxError,
            ..
        }))
    ));
    write(dir.path(), FILE, SOURCE);
    write(
        dir.path(),
        "pyproject.toml",
        b"[tool.setuptools.package-dir]\n\"\"='src'\n[tool.setuptools.packages.find]\n",
    );
    assert!(matches!(
        revalidate(dir.path(), &receipt),
        Err(RevalidationRefusal::Capture(CaptureRefusal::Configuration))
    ));
    write(dir.path(), "pyproject.toml", CONFIG);
    std::os::unix::fs::symlink("opaque", dir.path().join(".cache/alias")).unwrap();
    assert!(matches!(
        revalidate(dir.path(), &receipt),
        Err(RevalidationRefusal::Capture(
            CaptureRefusal::SymlinkOrNonregular
        ))
    ));
}

#[test]
fn zero_declarations_still_bind_complete_inventory_and_configuration() {
    let dir = fixture();
    write(dir.path(), FILE, b"# no declarations\n");
    let old = capture(dir.path());
    assert!(old.outcomes().values().all(Vec::is_empty));
    let receipt = old.receipt().unwrap();
    revalidate(dir.path(), &receipt).unwrap();
    write(dir.path(), ".cache/opaque", b"different unrelated bytes");
    assert!(matches!(
        revalidate(dir.path(), &receipt),
        Err(RevalidationRefusal::Mismatch(ReceiptMismatch::Inventory))
    ));
}

#[test]
fn receipt_is_content_identity_under_explicit_owner_and_allows_root_alias() {
    let first = fixture();
    let second = fixture();
    let old = capture(first.path());
    let receipt = old.receipt().unwrap();
    // Native inode/root continuity is not serialized. The caller explicitly
    // authorizes the second root, which must independently pass complete capture.
    let next = revalidate(second.path(), &receipt).unwrap();
    assert_eq!(next.outcomes(), old.outcomes());
    let outside = tempfile::tempdir().unwrap();
    let alias = outside.path().join("project");
    std::os::unix::fs::symlink(first.path(), &alias).unwrap();
    revalidate(&alias, &receipt).unwrap();
    // Receipt creation remains independent of subsequent disk availability.
    first.close().unwrap();
    assert_eq!(old.receipt().unwrap(), receipt);
    assert!(matches!(
        revalidate(&alias, &receipt),
        Err(RevalidationRefusal::Capture(CaptureRefusal::Io))
    ));
}
