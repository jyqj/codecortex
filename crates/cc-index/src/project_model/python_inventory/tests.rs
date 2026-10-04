//! Deterministic drift witnesses run between byte capture and admission/verify.
use super::*;
fn policies() -> AdmissionPolicies {
    AdmissionPolicies {
        capture: CaptureLimits {
            entries: 32,
            files: 16,
            total_bytes: 4096,
            file_bytes: 2048,
            depth: 8,
            path_bytes: 256,
            total_path_bytes: 4096,
            output_count: 32,
            output_bytes: 65536,
        },
        ast: PythonIdentityLimits {
            parse_timeout_micros: 1_000_000,
            ..Default::default()
        },
        declarations: DeclarationLimits {
            max_files: 16,
            max_total_bytes: 4096,
            max_file_bytes: 2048,
            max_roots: 1,
            max_evidence: 16,
            max_ancestry_depth: 16,
            max_identifier_bytes: 64,
        },
    }
}
fn fixture(root: &Path) {
    std::fs::create_dir_all(root.join("src/pkg")).unwrap();
    std::fs::write(
        root.join("pyproject.toml"),
        b"[tool.setuptools.package-dir]\n\"\"='src'\n",
    )
    .unwrap();
    std::fs::write(root.join("src/pkg/__init__.py"), b"pass\n").unwrap();
    std::fs::write(root.join("src/pkg/mod.py"), b"def f(): pass\n").unwrap();
}
#[test]
fn python_inventory_observed_midcapture_mutations_refuse() {
    for mutation in 0..8 {
        let outer = tempfile::tempdir().unwrap();
        let root = outer.path().join("root");
        fixture(&root);
        let r = capture_inner(
            &root,
            CaptureRequest {
                owner: "owner",
                scope: AuthorizedScope::EntireProject,
                exclusions: &[],
            },
            policies(),
            || match mutation {
                0 => std::fs::write(
                    root.join("pyproject.toml"),
                    b"# changed\n[tool.setuptools.package-dir]\n\"\"='src'\n",
                )
                .unwrap(),
                1 => std::fs::write(root.join("src/pkg/mod.py"), b"def g(): pass\n").unwrap(),
                2 => std::fs::remove_file(root.join("src/pkg/__init__.py")).unwrap(),
                3 => std::fs::write(root.join("src/pkg.py"), b"pass\n").unwrap(),
                4 => std::fs::rename(root.join("src/pkg/mod.py"), root.join("src/pkg/new.py"))
                    .unwrap(),
                5 => {
                    std::fs::rename(&root, outer.path().join("held")).unwrap();
                    fixture(&root);
                }
                6 => {
                    std::fs::remove_file(root.join("src/pkg/mod.py")).unwrap();
                    std::os::unix::fs::symlink("__init__.py", root.join("src/pkg/mod.py")).unwrap();
                }
                7 => std::fs::rename(root.join("pyproject.toml"), root.join("moved.toml")).unwrap(),
                _ => unreachable!(),
            },
        );
        assert!(
            matches!(
                r,
                Err(CaptureRefusal::Drift | CaptureRefusal::SymlinkOrNonregular)
            ),
            "mutation {mutation}: {r:?}"
        );
    }
}
