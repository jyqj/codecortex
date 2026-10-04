use super::*;
use std::{fs, path::Path};
fn public_capture(
    root: &Path,
) -> std::result::Result<
    cc_index::project_model::python_inventory::CapturedDeclarations,
    cc_index::project_model::python_inventory::CaptureRefusal,
> {
    use cc_index::project_model::python_inventory as api;
    api::capture_python_declarations(
        root,
        api::CaptureRequest {
            owner: "delta-owner",
            scope: api::AuthorizedScope::EntireProject,
            exclusions: &[],
        },
        api::AdmissionPolicies {
            capture: api::CaptureLimits {
                entries: 32,
                files: 16,
                total_bytes: 8192,
                file_bytes: 4096,
                depth: 8,
                path_bytes: 256,
                total_path_bytes: 4096,
                output_count: 16,
                output_bytes: 65536,
            },
            ast: PythonIdentityLimits {
                source_bytes: 4096,
                visited_nodes: 4096,
                tree_depth: 64,
                declarations: 16,
                output_segments: 64,
                output_text_bytes: 4096,
                parse_timeout_micros: 1_000_000,
            },
            declarations: DeclarationLimits {
                max_files: 16,
                max_total_bytes: 8192,
                max_file_bytes: 4096,
                max_roots: 1,
                max_evidence: 16,
                max_ancestry_depth: 16,
                max_identifier_bytes: 128,
            },
        },
    )
}
fn fixture(config: &str) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("lib/reef")).unwrap();
    fs::create_dir_all(root.path().join("other")).unwrap();
    fs::write(root.path().join("pyproject.toml"), config).unwrap();
    fs::write(root.path().join("lib/reef/__init__.py"), b"pass\n").unwrap();
    fs::write(root.path().join("lib/reef/tide.py"), b"def delta(): pass\n").unwrap();
    root
}
#[test]
fn review_delta_public_rejects_each_incomplete_or_conflicting_find() {
    let too_many = format!(
        "[tool.setuptools.packages.find]\nwhere=[{}]\n",
        vec!["'lib'"; 33].join(",")
    );
    let mut cases = vec![
        "[tool.setuptools.packages.find]\n".to_string(),
        "[tool.setuptools.packages.find]\nwhere=[]\n".to_string(),
        "[tool.setuptools.packages.find]\nwhere='lib'\n".to_string(),
        "[tool.setuptools.packages.find]\nwhere={}\n".to_string(),
        "[tool.setuptools.packages.find]\nwhere=['lib',1]\n".to_string(),
        "[tool.setuptools.packages.find]\nwhere=['lib','']\n".to_string(),
        "[tool.setuptools.packages.find]\nwhere=['lib','other']\n".to_string(),
        "[tool.setuptools.packages.find]\nwhere=['lib']\nwhere=['lib']\n".to_string(),
        "[tool.setuptools.packages.find]\nwhere=['lib']\nexclude=['reef']\n".to_string(),
        "[tool.setuptools.packages]\nfind=false\n".to_string(),
        "[tool.setuptools.packages]\nfind=['lib']\n".to_string(),
        "[tool.setuptools.packages.find]\nwhere=['../lib']\n".to_string(),
        "[tool.setuptools.packages.find]\nwhere=['absent']\n".to_string(),
    ];
    cases.push(too_many);
    for case in cases {
        for prefix in ["", "[tool.setuptools.package-dir]\n\"\"='lib'\n"] {
            let config = format!("{prefix}{case}");
            let root = fixture(&config);
            let result = public_capture(root.path());
            assert!(
                matches!(
                    result,
                    Err(cc_index::project_model::python_inventory::CaptureRefusal::Configuration)
                ),
                "{config}: {result:?}"
            );
            println!("public rejected {config:?}");
        }
    }
}
#[test]
fn review_delta_public_valid_standalone_and_agreeing_evidence() {
    use cc_model::module_inputs::PythonRootEvidence;
    for (config,expected) in [
        ("[tool.setuptools.package-dir]\n\"\"='./lib'\n",vec![("/tool/setuptools/package-dir/","./lib")]),
        ("[tool.setuptools.packages.find]\nwhere=['lib/./']\n",vec![("/tool/setuptools/packages/find/where/0","lib/./")]),
        ("[tool.setuptools.package-dir]\n\"\"='./lib'\n[tool.setuptools.packages.find]\nwhere=['lib/./','lib']\n",vec![("/tool/setuptools/package-dir/","./lib"),("/tool/setuptools/packages/find/where/0","lib/./"),("/tool/setuptools/packages/find/where/1","lib")]),
        ("[tool.setuptools.packages.find]\nwhere=['lib','lib']\n",vec![("/tool/setuptools/packages/find/where/0","lib"),("/tool/setuptools/packages/find/where/1","lib")]),
    ] {
        let root=fixture(config);let capture=public_capture(root.path()).unwrap();assert_eq!(capture.inventory()["pyproject.toml"],config.as_bytes());let evidence=&capture.provenance().roots["lib"];
        assert_eq!(evidence.iter().map(|item|match item{PythonRootEvidence::Explicit{directive,value}=>(directive.as_str(),value.as_str()),other=>panic!("{other:?}")}).collect::<Vec<_>>(),expected);
        let IdentityOutcome::Derived(declaration)=&capture.outcomes()["lib/reef/tide.py"][0] else{panic!("expected derived")};
        assert_eq!(declaration.binding().root.directive,serde_json::to_string(evidence).unwrap());assert_eq!(declaration.binding().root.config_digest,content_digest(config.as_bytes()));println!("public accepted {config:?}; complete evidence {expected:?}");
    }
}
