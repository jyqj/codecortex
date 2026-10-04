//! Fresh actual filesystem capture -> AST -> model fixtures; no Python execution.
#![cfg(target_os = "linux")]
use cc_index::project_model::python_inventory::*;
use cc_model::{declaration_identity::*, source::ByteSpan};
use cc_parsers::python_identity::*;
use std::path::Path;

const CONFIG: &[u8] = b"# constellation config\n[tool.setuptools.package-dir]\n\"\"='./lib'\n[tool.setuptools.packages.find]\nwhere=['lib/./','lib']\n";
const SOURCE: &[u8] = b"\xef\xbb\xbf\r\n# sky\r\nclass Observatory:\r\n    def glow(self): return 1\r\nif phase:\r\n    def signal(): return 2\r\nelse:\r\n    def signal(): return 3\r\n";
const FILE: &str = "lib/sky/chart.py";

fn write(root: &Path, path: &str, bytes: &[u8]) {
    let dest = root.join(path);
    std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
    std::fs::write(dest, bytes).unwrap();
}
fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (path, bytes) in [
        ("pyproject.toml", CONFIG),
        (FILE, SOURCE),
        ("lib/sky/__init__.py", b"raise RuntimeError('unexecuted')\n"),
        (".git/HEAD", b"ref: refs/heads/fixture\n"),
        (".cache/raw", b"\xff\x00opaque"),
        (".gitignore", b"__init__.py\n.cache/\n"),
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
fn capture(root: &Path, p: AdmissionPolicies) -> Result<CapturedDeclarations, CaptureRefusal> {
    capture_python_declarations(
        root,
        CaptureRequest {
            owner: "constellation-owner",
            scope: AuthorizedScope::EntireProject,
            exclusions: &[],
        },
        p,
    )
}
fn derived(outcome: &IdentityOutcome) -> &BoundDeclaration {
    match outcome {
        IdentityOutcome::Derived(d) => d,
        other => panic!("{other:?}"),
    }
}

#[test]
fn fresh_capture_matches_actual_ast_and_borrowed_model_with_literal_names() {
    let dir = fixture();
    let c = capture(dir.path(), policies()).unwrap();
    assert_eq!(c.inventory().len(), 6);
    assert_eq!(c.inventory()[".cache/raw"], b"\xff\x00opaque");
    assert_eq!(c.inventory()[".git/HEAD"], b"ref: refs/heads/fixture\n");
    assert_eq!(c.inventory()[FILE], SOURCE);
    assert_eq!(c.inventory()["pyproject.toml"], CONFIG);
    let evidence = &c.provenance().roots["lib"];
    assert_eq!(evidence.len(), 3);
    let root = derived(&c.outcomes()[FILE][0]).binding().root.clone();
    assert_eq!(root.config_digest, content_digest(CONFIG));
    assert_eq!(root.directive, serde_json::to_string(evidence).unwrap());
    let snapshot = DeclarationSnapshot::with_limits(
        c.owner().into(),
        c.inventory(),
        vec![root],
        policies().declarations,
    )
    .unwrap();
    let PythonIdentityOutcome::Inputs(inputs) =
        declaration_inputs(FILE, &c.inventory()[FILE], policies().ast).unwrap()
    else {
        panic!("AST refusal")
    };
    assert_eq!(inputs.len(), 4);
    for (i, (name, start, end)) in [
        ("Observatory", 18, 29),
        ("glow", 40, 44),
        ("signal", 81, 87),
        ("signal", 116, 122),
    ]
    .into_iter()
    .enumerate()
    {
        let segment = inputs[i].ancestry.last().unwrap();
        assert_eq!(segment.name, name);
        assert_eq!(segment.name_span, ByteSpan { start, end });
        assert_eq!(&SOURCE[start..end], name.as_bytes());
        assert_eq!(inputs[i].source_digest, content_digest(SOURCE));
        assert_eq!(
            snapshot.resolve_with_limits(&inputs[i]).unwrap(),
            c.outcomes()[FILE][i]
        );
        let d = derived(&c.outcomes()[FILE][i]);
        assert_eq!(d.address().module, ["sky", "chart"]);
        assert_eq!(d.address().owner, "constellation-owner");
        assert!(snapshot.is_current_with_limits(d).unwrap());
    }
    assert_eq!(
        derived(&c.outcomes()[FILE][2]).address(),
        derived(&c.outcomes()[FILE][3]).address()
    );
    assert_ne!(
        derived(&c.outcomes()[FILE][2]).fingerprint().unwrap(),
        derived(&c.outcomes()[FILE][3]).fingerprint().unwrap()
    );
}

#[test]
fn whole_scope_recapture_preserves_old_bytes_and_changes_marker_and_git_binding() {
    let dir = fixture();
    let old = capture(dir.path(), policies()).unwrap();
    let old_fingerprint = derived(&old.outcomes()[FILE][0]).fingerprint().unwrap();
    write(dir.path(), ".git/HEAD", b"ref: refs/heads/changed\n");
    let changed = capture(dir.path(), policies()).unwrap();
    assert_ne!(
        old_fingerprint,
        derived(&changed.outcomes()[FILE][0]).fingerprint().unwrap()
    );
    assert_eq!(old.inventory()[".git/HEAD"], b"ref: refs/heads/fixture\n");
    std::fs::remove_file(dir.path().join("lib/sky/__init__.py")).unwrap();
    let absent = capture(dir.path(), policies()).unwrap();
    assert!(absent.outcomes()[FILE]
        .iter()
        .all(|v| *v == IdentityOutcome::Unavailable(IdentityReason::NamespaceAncestry)));
    write(dir.path(), "lib/sky/__init__.py", b"pass\n");
    write(dir.path(), "lib/sky.py", b"pass\n");
    let collision = capture(dir.path(), policies()).unwrap();
    assert!(collision.outcomes()[FILE]
        .iter()
        .all(|v| *v == IdentityOutcome::Unavailable(IdentityReason::ModulePackageCollision)));
    assert_eq!(old.inventory()[FILE], SOURCE);
    assert_eq!(
        old_fingerprint,
        derived(&old.outcomes()[FILE][0]).fingerprint().unwrap()
    );
}

#[test]
fn combined_capture_ast_model_and_aggregate_fail_closed() {
    let dir = fixture();
    let c = capture(dir.path(), policies()).unwrap();
    let bytes = serde_json::to_vec(c.outcomes()).unwrap().len();
    let mut p = policies();
    p.capture.output_bytes = bytes;
    assert!(capture(dir.path(), p).is_ok());
    p.capture.output_bytes = bytes - 1;
    assert!(matches!(
        capture(dir.path(), p),
        Err(CaptureRefusal::Budget("output_bytes"))
    ));
    p = policies();
    p.ast.declarations = 3;
    assert!(matches!(
        capture(dir.path(), p),
        Err(CaptureRefusal::Ast { .. })
    ));
    p = policies();
    p.declarations.max_identifier_bytes = 10;
    assert!(matches!(
        capture(dir.path(), p),
        Err(CaptureRefusal::Model(_))
    ));
    write(
        dir.path(),
        "pyproject.toml",
        b"[tool.setuptools.package-dir]\n\"\"='lib'\n[tool.setuptools.packages.find]\n",
    );
    assert!(matches!(
        capture(dir.path(), policies()),
        Err(CaptureRefusal::Configuration)
    ));
    assert_eq!(c.config().digest, Some(content_digest(CONFIG)));
    assert_eq!(c.inventory()[FILE], SOURCE);
}
