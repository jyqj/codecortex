//! Self-authored real-filesystem fixtures only. Never executes captured Python.
#![cfg(target_os = "linux")]
use cc_index::project_model::python_inventory::*;
use cc_model::{declaration_identity::*, module_inputs::PythonRootEvidence};
use cc_parsers::python_identity::*;
use std::{collections::BTreeMap, path::Path};
const CONFIG: &[u8] = b"# raw config\n[tool.setuptools.package-dir]\n\"\"='./src'\n[tool.setuptools.packages.find]\nwhere=['src/./']\n";
const SOURCE: &[u8] = b"\xef\xbb\xbf\r\n# trivia\r\nclass Outer:\r\n    def pulse(self): return 1\r\ndef factory():\r\n    class Local: pass\r\nif ready:\r\n    def echo(): pass\r\nelse:\r\n    def echo(): pass\r\n";
fn fixture() -> (tempfile::TempDir, BTreeMap<String, Vec<u8>>) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".git")).unwrap();
    let files: BTreeMap<String, Vec<u8>> = BTreeMap::from([
        ("pyproject.toml".into(), CONFIG.to_vec()),
        (
            "src/harbor/__init__.py".into(),
            b"raise RuntimeError('never execute')\ndef harbor(): pass\n".to_vec(),
        ),
        ("src/harbor/tide.py".into(), SOURCE.to_vec()),
        (".gitignore".into(), b"__init__.py\n".to_vec()),
        ("note".into(), b"unrelated scope content".to_vec()),
    ]);
    for (p, bytes) in &files {
        write(dir.path(), p, bytes);
    }
    (dir, files)
}
fn write(root: &Path, path: &str, bytes: &[u8]) {
    std::fs::create_dir_all(root.join(path).parent().unwrap()).unwrap();
    std::fs::write(root.join(path), bytes).unwrap();
}
fn policies() -> AdmissionPolicies {
    AdmissionPolicies {
        capture: CaptureLimits {
            entries: 64,
            files: 32,
            total_bytes: 8192,
            file_bytes: 4096,
            depth: 8,
            path_bytes: 256,
            total_path_bytes: 4096,
            output_count: 32,
            output_bytes: 65536,
        },
        ast: PythonIdentityLimits {
            source_bytes: 4096,
            visited_nodes: 2000,
            tree_depth: 64,
            declarations: 32,
            output_segments: 128,
            output_text_bytes: 8192,
            parse_timeout_micros: 1_000_000,
        },
        declarations: DeclarationLimits {
            max_files: 32,
            max_total_bytes: 8192,
            max_file_bytes: 4096,
            max_roots: 1,
            max_evidence: 16,
            max_ancestry_depth: 16,
            max_identifier_bytes: 64,
        },
    }
}
fn capture(root: &Path, p: AdmissionPolicies) -> Result<CapturedDeclarations, CaptureRefusal> {
    capture_python_declarations(
        root,
        CaptureRequest {
            owner: "authorized-fixture-owner",
            scope: AuthorizedScope::EntireProject,
            exclusions: &[],
        },
        p,
    )
}
fn derived(v: &IdentityOutcome) -> &BoundDeclaration {
    match v {
        IdentityOutcome::Derived(d) => d,
        other => panic!("{other:?}"),
    }
}
#[test]
fn actual_inventory_raw_directives_and_same_source_admission() {
    let (dir, files) = fixture();
    let c = capture(dir.path(), policies()).unwrap();
    assert_eq!(c.inventory(), &files);
    assert_eq!(c.owner(), "authorized-fixture-owner");
    assert!(matches!(c.scope(), AuthorizedScope::EntireProject));
    let e = &c.provenance().roots["src"];
    assert_eq!(
        e,
        &vec![
            PythonRootEvidence::Explicit {
                directive: "/tool/setuptools/package-dir/".into(),
                value: "./src".into()
            },
            PythonRootEvidence::Explicit {
                directive: "/tool/setuptools/packages/find/where/0".into(),
                value: "src/./".into()
            }
        ]
    );
    assert_eq!(c.config().digest, Some(content_digest(CONFIG)));
    let values = &c.outcomes()["src/harbor/tide.py"];
    assert_eq!(values.len(), 6);
    let d = derived(&values[1]);
    assert_eq!(d.address().owner, "authorized-fixture-owner");
    assert_eq!(d.address().module, ["harbor", "tide"]);
    assert_eq!(
        d.address().lexical,
        [
            ("Outer".into(), ScopeKind::Class),
            ("pulse".into(), ScopeKind::Function)
        ]
    );
    assert_eq!(d.binding().source_digest, content_digest(SOURCE));
    assert_eq!(
        d.binding().root.directive,
        serde_json::to_string(e).unwrap()
    );
    assert!(d
        .binding()
        .package_files
        .contains_key("src/harbor/__init__.py"));
    for segment in &d.binding().ancestry {
        assert_eq!(
            &SOURCE[segment.name_span.start..segment.name_span.end],
            segment.name.as_bytes()
        );
    }
    assert_eq!(
        values[3],
        IdentityOutcome::Unavailable(IdentityReason::LocalDeclaration)
    );
    assert_eq!(derived(&values[4]).address(), derived(&values[5]).address());
    assert_ne!(
        derived(&values[4]).fingerprint().unwrap(),
        derived(&values[5]).fingerprint().unwrap()
    );
}
#[test]
fn ignores_do_not_hide_markers_and_exclusions_or_partial_scopes_are_refused() {
    let (dir, _) = fixture();
    assert!(capture(dir.path(), policies())
        .unwrap()
        .inventory()
        .contains_key("src/harbor/__init__.py"));
    for scope in [
        AuthorizedScope::Subtree("src/harbor".into()),
        AuthorizedScope::Subtree("src".into()),
    ] {
        assert!(matches!(
            capture_python_declarations(
                dir.path(),
                CaptureRequest {
                    owner: "owner",
                    scope,
                    exclusions: &[]
                },
                policies()
            ),
            Err(CaptureRefusal::UnauthorizedScope)
        ));
    }
    let exclusions = ["src/harbor/__init__.py".into()];
    assert!(matches!(
        capture_python_declarations(
            dir.path(),
            CaptureRequest {
                owner: "owner",
                scope: AuthorizedScope::EntireProject,
                exclusions: &exclusions
            },
            policies()
        ),
        Err(CaptureRefusal::UnauthorizedScope)
    ));
    std::fs::remove_file(dir.path().join("src/harbor/__init__.py")).unwrap();
    let c = capture(dir.path(), policies()).unwrap();
    assert_eq!(
        c.outcomes()["src/harbor/tide.py"][0],
        IdentityOutcome::Unavailable(IdentityReason::NamespaceAncestry)
    );
}
#[test]
fn full_scope_content_marker_absence_and_collision_are_bound() {
    let (dir, _) = fixture();
    let c = capture(dir.path(), policies()).unwrap();
    let before = derived(&c.outcomes()["src/harbor/tide.py"][0])
        .fingerprint()
        .unwrap();
    for path in ["note", "pyproject.toml", "src/harbor/__init__.py"] {
        let bytes = std::fs::read(dir.path().join(path)).unwrap();
        let mut changed = bytes.clone();
        changed.extend_from_slice(b"\n# changed\n");
        write(dir.path(), path, &changed);
        let next = capture(dir.path(), policies()).unwrap();
        assert_ne!(
            before,
            derived(&next.outcomes()["src/harbor/tide.py"][0])
                .fingerprint()
                .unwrap()
        );
        write(dir.path(), path, &bytes);
    }
    write(dir.path(), "src/harbor.py", b"pass\n");
    let next = capture(dir.path(), policies()).unwrap();
    assert_eq!(
        next.outcomes()["src/harbor/tide.py"][0],
        IdentityOutcome::Unavailable(IdentityReason::ModulePackageCollision)
    );
}
#[test]
fn defaults_partial_duplicate_toml_and_unsupported_collection_policies_refuse() {
    let (dir, _) = fixture();
    for config in [
        "[project]\nname='fixture'\n",                              // defaults
        "[tool.setuptools.package-dir]\nfoo='src'\n\"\"='src'\n",   // partial
        "[tool.setuptools.package-dir]\n\"\"='src'\n\"\"='src'\n",  // duplicate key
        "[tool.setuptools.packages.find]\nwhere=['src','other']\n", // multiple roots
        "[tool.setuptools.packages.find]\nwhere=['src']\nexclude=['harbor']\n",
        "[tool.setuptools.packages.find]\nwhere=['src']\nnamespaces=false\n",
        "[tool.poetry]\npackages=[{include='harbor',from='src'}]\n",
        "[tool.setuptools.package-dir]\n\"\"='../src'\n",
        "[tool.setuptools.package-dir]\n\"\"='absent'\n",
    ] {
        write(dir.path(), "pyproject.toml", config.as_bytes());
        assert!(
            matches!(
                capture(dir.path(), policies()),
                Err(CaptureRefusal::Configuration)
            ),
            "{config}"
        );
    }
    write(dir.path(), "pyproject.toml", CONFIG);
    write(dir.path(), "src/pyproject.toml", CONFIG);
    assert!(matches!(
        capture(dir.path(), policies()),
        Err(CaptureRefusal::Configuration)
    ));
    std::fs::remove_file(dir.path().join("src/pyproject.toml")).unwrap();
    std::fs::rename(
        dir.path().join("pyproject.toml"),
        dir.path().join("renamed.toml"),
    )
    .unwrap();
    assert!(matches!(
        capture(dir.path(), policies()),
        Err(CaptureRefusal::Configuration)
    ));
}
#[test]
fn canonical_project_root_alias_is_explicitly_accepted_below_root_aliases_refused() {
    use std::os::unix::fs::symlink;
    let (dir, _) = fixture();
    let outer = tempfile::tempdir().unwrap();
    let alias = outer.path().join("alias");
    symlink(dir.path(), &alias).unwrap();
    let a = capture(dir.path(), policies()).unwrap();
    let b = capture(&alias, policies()).unwrap();
    assert_eq!(a.outcomes(), b.outcomes());
    symlink("harbor", dir.path().join("src/link")).unwrap();
    assert!(matches!(
        capture(dir.path(), policies()),
        Err(CaptureRefusal::SymlinkOrNonregular)
    ));
    std::fs::remove_file(dir.path().join("src/link")).unwrap();
    symlink("tide.py", dir.path().join("src/harbor/link.py")).unwrap();
    assert!(matches!(
        capture(dir.path(), policies()),
        Err(CaptureRefusal::SymlinkOrNonregular)
    ));
}
#[test]
fn native_non_utf8_portable_and_hardlink_aliases_are_rejected() {
    use std::os::unix::ffi::OsStrExt;
    let (dir, _) = fixture();
    for name in [
        b"bad\xff".as_slice(),
        b"a\\b",
        b"a:b",
        b"trailing.",
        b"CON.py",
        "caf\u{e9}".as_bytes(),
    ] {
        let path = dir.path().join(std::ffi::OsStr::from_bytes(name));
        std::fs::write(&path, b"").unwrap();
        assert!(matches!(
            capture(dir.path(), policies()),
            Err(CaptureRefusal::Path)
        ));
        std::fs::remove_file(path).unwrap();
    }
    write(dir.path(), "NOTE", b"");
    assert!(matches!(
        capture(dir.path(), policies()),
        Err(CaptureRefusal::Alias)
    ));
    std::fs::remove_file(dir.path().join("NOTE")).unwrap();
    // Reject even a hardlink whose other spelling is outside the authorized root.
    let outside = tempfile::tempdir().unwrap();
    std::fs::hard_link(dir.path().join("note"), outside.path().join("alias")).unwrap();
    assert!(matches!(
        capture(dir.path(), policies()),
        Err(CaptureRefusal::Alias)
    ));
}
#[test]
fn fifo_is_rejected_without_blocking_and_syntax_failure_has_no_partial_result() {
    use std::ffi::CString;
    let (dir, _) = fixture();
    let path = CString::new(dir.path().join("fifo").as_os_str().as_encoded_bytes()).unwrap();
    // SAFETY: self-authored temporary fixture, valid C string, mode only.
    assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
    assert!(matches!(
        capture(dir.path(), policies()),
        Err(CaptureRefusal::SymlinkOrNonregular)
    ));
    std::fs::remove_file(dir.path().join("fifo")).unwrap();
    write(
        dir.path(),
        "src/harbor/tide.py",
        b"def ok(): pass\ndef broken(: pass\n",
    );
    assert!(matches!(
        capture(dir.path(), policies()),
        Err(CaptureRefusal::Ast {
            reason: PythonIdentityReason::SyntaxError,
            ..
        })
    ));
}
#[test]
fn exact_and_one_over_inventory_and_aggregate_output_bounds() {
    let (dir, files) = fixture();
    let c = capture(dir.path(), policies()).unwrap();
    let output_bytes = serde_json::to_vec(c.outcomes()).unwrap().len();
    let mut exact = policies();
    exact.capture = CaptureLimits {
        entries: 9,
        files: files.len(),
        total_bytes: files.values().map(Vec::len).sum(),
        file_bytes: files.values().map(Vec::len).max().unwrap(),
        depth: 3,
        path_bytes: files.keys().map(String::len).max().unwrap(),
        total_path_bytes: files.keys().map(String::len).sum::<usize>()
            + ".git".len()
            + "src".len()
            + "src/harbor".len(),
        output_count: c.outcomes().values().map(Vec::len).sum(),
        output_bytes,
    };
    capture(dir.path(), exact).unwrap();
    for resource in [
        "entries",
        "files",
        "total_bytes",
        "file_bytes",
        "depth",
        "path_bytes",
        "total_path_bytes",
        "output_count",
        "output_bytes",
    ] {
        let mut one_over = exact;
        match resource {
            "entries" => one_over.capture.entries -= 1,
            "files" => one_over.capture.files -= 1,
            "total_bytes" => one_over.capture.total_bytes -= 1,
            "file_bytes" => one_over.capture.file_bytes -= 1,
            "depth" => one_over.capture.depth -= 1,
            "path_bytes" => one_over.capture.path_bytes -= 1,
            "total_path_bytes" => one_over.capture.total_path_bytes -= 1,
            "output_count" => one_over.capture.output_count -= 1,
            "output_bytes" => one_over.capture.output_bytes -= 1,
            _ => unreachable!(),
        }
        assert!(
            matches!(capture(dir.path(), one_over), Err(CaptureRefusal::Budget(r)) if r == resource),
            "{resource}"
        );
    }
    exact.declarations.max_files = files.len() - 1;
    assert!(matches!(
        capture(dir.path(), exact),
        Err(CaptureRefusal::Model(DeclarationError::Resource(_)))
    ));
    let mut ast = policies();
    ast.ast.source_bytes = SOURCE.len() - 1;
    assert!(matches!(
        capture(dir.path(), ast),
        Err(CaptureRefusal::Ast {
            reason: PythonIdentityReason::SourceLimit,
            ..
        })
    ));
}

#[test]
fn explicit_project_root_and_root_initializer_are_supported_without_defaults() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "pyproject.toml",
        b"[tool.setuptools.package-dir]\n\"\"='.'\n",
    );
    write(dir.path(), "__init__.py", b"def root(): pass\n");
    write(dir.path(), "mod.py", b"\n# leading trivia\ndef f(): pass\n");
    let c = capture(dir.path(), policies()).unwrap();
    assert_eq!(
        c.outcomes()["__init__.py"][0],
        IdentityOutcome::Unavailable(IdentityReason::RootInitializer)
    );
    assert_eq!(
        derived(&c.outcomes()["mod.py"][0]).address().module,
        ["mod"]
    );
}
