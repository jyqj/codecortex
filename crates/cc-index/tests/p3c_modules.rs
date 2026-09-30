//! Independent static module truth, including negative and package-set results.
use cc_index::{module_resolution, project_model::discover, Scanner};
use cc_model::project_model::*;
use std::{collections::BTreeSet, path::Path};
fn put(root: &Path, p: &str, s: &str) {
    let p = root.join(p);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, s).unwrap();
}
fn capture(root: &Path) -> cc_index::project_model::CapturedProject {
    let (files, walk) = Scanner::new(root, &Default::default()).scan_with_manifest();
    discover(
        root,
        files.into_iter().map(|f| f.rel_path).collect(),
        Some(&walk),
        None,
        &Default::default(),
    )
    .unwrap()
}
fn fixture() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    put(d.path(),"app/go.mod","module example.com/app\ngo 1.22\nrequire example.com/api v1.0.0\nreplace example.com/api => ../one\n");
    put(
        d.path(),
        "app/use.go",
        "package app\nimport svc \"example.com/api\"\nfunc Run() int {return svc.Ping()}\n",
    );
    for dir in ["one", "two"] {
        put(
            d.path(),
            &format!("{dir}/go.mod"),
            "module example.com/api\ngo 1.22\n",
        );
        put(
            d.path(),
            &format!("{dir}/a.go"),
            "package service\nfunc Ping() int{return 1}\n",
        );
        put(
            d.path(),
            &format!("{dir}/b.go"),
            "package service\nfunc Pong() int{return 2}\n",
        );
    }
    d
}
#[test]
fn python_uncaptured_environment_differs_from_missing_local_submodule() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "use.py", "import remote\n");
    put(d.path(), "pkg/__init__.py", "");
    put(d.path(), "pkg/use.py", "from .missing import ping\n");
    let c = capture(d.path());
    assert_eq!(
        module_resolution::resolve(c.model(), "use.py", "remote").status,
        ModuleStatus::Unknown
    );
    assert_eq!(
        module_resolution::resolve(c.model(), "use.py", "pkg.missing").status,
        ModuleStatus::Unresolved
    );
    assert_eq!(
        module_resolution::resolve(c.model(), "pkg/use.py", ".missing").status,
        ModuleStatus::Unresolved
    );
}
#[test]
fn shared_authored_fixtures_match_module_targets() {
    let cases: serde_json::Value = serde_json::from_str(include_str!(
        "../../cc-eval/benchmarks/modules/p3c-fixtures.json"
    ))
    .unwrap();
    for case in cases["cases"].as_array().unwrap() {
        let d = tempfile::tempdir().unwrap();
        for (path, text) in case["files"].as_object().unwrap() {
            put(d.path(), path, text.as_str().unwrap());
        }
        let c = capture(d.path());
        let r = module_resolution::resolve(
            c.model(),
            case["file"].as_str().unwrap(),
            case["specifier"].as_str().unwrap(),
        );
        assert_eq!(r.status, ModuleStatus::Resolved, "{}: {:?}", case["id"], r);
        let files = r.resolved_package.map_or_else(
            || r.resolved_path.into_iter().collect::<Vec<_>>(),
            |p| p.files,
        );
        assert_eq!(
            serde_json::json!(files),
            case["expected_files"],
            "{}",
            case["id"]
        );
    }
}
#[test]
fn declared_local_ts_dependency_missing_is_not_external() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "use.ts", "import {x} from 'local';\n");
    put(
        d.path(),
        "package.json",
        r#"{"dependencies":{"local":"file:./missing"}}"#,
    );
    let c = capture(d.path());
    let local = module_resolution::resolve(c.model(), "use.ts", "local");
    let remote = module_resolution::resolve(c.model(), "use.ts", "remote");
    assert_eq!(local.status, ModuleStatus::Unresolved);
    assert!(local.config_dependencies.contains("missing/package.json"));
    assert_eq!(remote.status, ModuleStatus::External);
}
#[test]
fn declared_rust_registry_dependency_is_external_not_local_alias() {
    let d = tempfile::tempdir().unwrap();
    put(
        d.path(),
        "Cargo.toml",
        "[package]\nname='app'\nversion='0.1.0'\n[dependencies]\nserde='1'\n",
    );
    put(d.path(), "src/lib.rs", "use serde::Serialize;\n");
    let r = module_resolution::resolve(capture(d.path()).model(), "src/lib.rs", "serde::Serialize");
    assert_eq!(r.status, ModuleStatus::External);
    assert!(r.resolved_path.is_none());
}

#[test]
fn package_set_never_uses_a_representative_file() {
    let d = fixture();
    let c = capture(d.path());
    let r = module_resolution::resolve(c.model(), "app/use.go", "example.com/api");
    assert_eq!(r.status, ModuleStatus::Resolved);
    assert!(r.resolved_path.is_none());
    let p = r.resolved_package.unwrap();
    assert_eq!(p.name, "service");
    assert_eq!(p.files, vec!["one/a.go", "one/b.go"]);
}
#[test]
fn work_override_use_blocks_quotes_and_comments() {
    let d = fixture();
    put(
        d.path(),
        "go.work",
        "go 1.22\nuse (\n \"./app\" // owner\n)\nreplace (\n example.com/api => ./two\n)\n",
    );
    let c = capture(d.path());
    let r = module_resolution::resolve(c.model(), "app/use.go", "example.com/api");
    assert_eq!(
        r.resolved_package.unwrap().files,
        vec!["two/a.go", "two/b.go"]
    );
    assert!(r.config_dependencies.contains("go.work"));
}
#[test]
fn external_missing_and_unknown_are_distinct() {
    let d = fixture();
    let c = capture(d.path());
    assert_eq!(
        module_resolution::resolve(c.model(), "app/use.go", "net/http").status,
        ModuleStatus::External
    );
    assert_eq!(
        module_resolution::resolve(c.model(), "app/use.go", "example.com/api/missing").status,
        ModuleStatus::Unresolved
    );
    put(
        d.path(),
        "one/a_linux.go",
        "package service\nfunc Platform(){}\n",
    );
    let c = capture(d.path());
    let r = module_resolution::resolve(c.model(), "app/use.go", "example.com/api");
    assert_eq!(r.status, ModuleStatus::Unknown);
    assert!(!r.conditions.is_empty());
    assert!(r.resolved_package.is_none());
}
#[test]
fn test_files_are_not_production_and_conflicting_names_are_ambiguous() {
    let d = fixture();
    put(
        d.path(),
        "one/a_test.go",
        "package service_test\nfunc Extra(){}\n",
    );
    let c = capture(d.path());
    assert_eq!(
        module_resolution::resolve(c.model(), "app/use.go", "example.com/api")
            .resolved_package
            .unwrap()
            .files
            .len(),
        2
    );
    put(
        d.path(),
        "one/conflict.go",
        "package incompatible\nfunc Other(){}\n",
    );
    let r = module_resolution::resolve(capture(d.path()).model(), "app/use.go", "example.com/api");
    assert_eq!(r.status, ModuleStatus::Ambiguous);
    assert!(r.resolved_path.is_none());
}
#[test]
fn malformed_and_outside_configs_never_select_a_target() {
    for text in ["module example.com/app\nreplace (\n", "module example.com/app\nrequire example.com/api v1.0.0\nreplace example.com/api => ../../outside\n"]{let d=fixture();put(d.path(),"app/go.mod",text);let r=module_resolution::resolve(capture(d.path()).model(),"app/use.go","example.com/api");assert!(matches!(r.status,ModuleStatus::Unknown|ModuleStatus::Unsupported));assert!(r.resolved_package.is_none());}
}
#[test]
fn duplicate_workspace_modules_are_ambiguous() {
    let d = fixture();
    put(
        d.path(),
        "go.work",
        "go 1.22\nuse (\n ./app\n ./one\n ./two\n)\n",
    );
    let r = module_resolution::resolve(capture(d.path()).model(), "app/use.go", "example.com/api");
    assert_eq!(r.status, ModuleStatus::Ambiguous);
    assert!(r.candidates.len() >= 2);
}
#[test]
fn capture_remains_usable_without_disk_sources() {
    let d = fixture();
    let c = capture(d.path());
    std::fs::remove_dir_all(d.path().join("one")).unwrap();
    for _ in 0..1000 {
        let r = module_resolution::resolve(c.model(), "app/use.go", "example.com/api");
        assert_eq!(r.resolved_package.as_ref().unwrap().files.len(), 2);
        assert!(r.valid_target_shape());
    }
}
#[test]
fn cgo_and_build_directives_remain_unknown() {
    for source in [
        "//go:build custom\n\npackage service\n",
        "package service\nimport \"C\"\n",
    ] {
        let d = fixture();
        put(d.path(), "one/conditional.go", source);
        assert_eq!(
            module_resolution::resolve(capture(d.path()).model(), "app/use.go", "example.com/api")
                .status,
            ModuleStatus::Unknown
        );
    }
}
#[test]
fn structured_result_rejects_contradictory_or_invalid_package_shapes() {
    let d = fixture();
    let mut r =
        module_resolution::resolve(capture(d.path()).model(), "app/use.go", "example.com/api");
    assert!(r.valid_target_shape());
    r.resolved_path = Some("one/a.go".into());
    assert!(!r.valid_target_shape());
    r.resolved_path = None;
    r.resolved_package
        .as_mut()
        .unwrap()
        .files
        .push("../outside.go".into());
    assert!(!r.valid_target_shape());
}
#[test]
fn scope_capture_retains_manifest_inputs_without_source_admission() {
    let d = fixture();
    let cfg = cc_model::config::IndexingConfig {
        ignore: vec!["**/*.mod".into()],
        ..Default::default()
    };
    let (files, walk) = Scanner::new(d.path(), &cfg).scan_with_manifest();
    let c = discover(
        d.path(),
        files
            .into_iter()
            .map(|f| f.rel_path)
            .collect::<BTreeSet<_>>(),
        Some(&walk),
        None,
        &Default::default(),
    )
    .unwrap();
    assert_eq!(
        module_resolution::resolve(c.model(), "app/use.go", "example.com/api").status,
        ModuleStatus::Resolved
    );
}
