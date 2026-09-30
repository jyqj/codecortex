//! Independent negative contracts for G3; never use the daily index.
use cc_index::{
    module_resolution,
    project_model::{discover, CapturedProject},
    Scanner,
};
use cc_model::{config::IndexingConfig, project_model::*};
use std::path::Path;
fn put(root: &Path, path: &str, text: &str) {
    let p = root.join(path);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}
fn capture(root: &Path) -> CapturedProject {
    let scanner = Scanner::new(root, &IndexingConfig::default());
    let (files, walk) = scanner.scan_with_manifest();
    discover(
        root,
        files.into_iter().map(|f| f.rel_path).collect(),
        Some(&walk),
        None,
        &ProjectInputs::default(),
    )
    .unwrap()
}
#[test]
fn foreign_language_import_must_not_resolve_using_javascript_suffixes() {
    let root = tempfile::tempdir().unwrap();
    put(root.path(), "src/Main.java", "class Main {}\n");
    put(root.path(), "src/helper.ts", "export const marker=1;\n");
    let captured = capture(root.path());
    let result = module_resolution::resolve(captured.model(), "src/Main.java", "./helper");
    assert_eq!(
        result.status,
        ModuleStatus::Unsupported,
        "a JS path guess is not Java module resolution: {result:?}"
    );
    assert!(result.resolved_path.is_none());
}
#[cfg(unix)]
#[test]
#[ignore = "child-process harness exercised by the bounded FIFO parent tests"]
fn special_config_child() {
    let Ok(root) = std::env::var("CC_P3D_SPECIAL_CONFIG_ROOT") else {
        return;
    };
    if std::env::var_os("CC_P3D_OWN_CONFIG").is_some() {
        let _ = cc_model::config::load_project_config(Path::new(&root));
    } else {
        let c = capture(Path::new(&root));
        assert!(
            c.report()
                .diagnostics
                .iter()
                .any(|d| d.reason == "config_not_regular"),
            "{:?}",
            c.report()
        );
    }
}
#[cfg(unix)]
#[test]
fn fifo_configuration_is_rejected_without_waiting_for_a_writer() {
    use std::{
        os::unix::ffi::OsStrExt,
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    let root = tempfile::tempdir().unwrap();
    put(root.path(), "use.ts", "export const value=1;\n");
    let path =
        std::ffi::CString::new(root.path().join("tsconfig.json").as_os_str().as_bytes()).unwrap();
    // SAFETY: path is NUL-terminated and only creates a private test FIFO.
    assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "special_config_child",
            "--ignored",
            "--nocapture",
        ])
        .env("CC_P3D_SPECIAL_CONFIG_ROOT", root.path())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(
                status.success(),
                "special input was not explicitly rejected"
            );
            break;
        }
        if Instant::now() > deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("configuration open blocked on FIFO instead of rejecting a non-regular input");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
#[cfg(unix)]
#[test]
fn application_configuration_fifo_is_also_bounded() {
    use std::{
        os::unix::ffi::OsStrExt,
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    let root = tempfile::tempdir().unwrap();
    let path = std::ffi::CString::new(root.path().join(".codecortex.json").as_os_str().as_bytes())
        .unwrap();
    // SAFETY: private test directory and valid NUL-terminated path.
    assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "special_config_child", "--ignored"])
        .env("CC_P3D_SPECIAL_CONFIG_ROOT", root.path())
        .env("CC_P3D_OWN_CONFIG", "1")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        if Instant::now() > deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("application config blocked on FIFO");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
#[cfg(unix)]
#[test]
fn directory_symlink_and_final_symlink_never_load_external_configuration() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    put(
        outside.path(),
        "base.json",
        r#"{"compilerOptions":{"baseUrl":".","paths":{"marker":["safe"]}}}"#,
    );
    put(root.path(), "safe.ts", "export const marker=1;\n");
    std::os::unix::fs::symlink(outside.path(), root.path().join("linked")).unwrap();
    put(
        root.path(),
        "tsconfig.json",
        r#"{"extends":"./linked/base.json"}"#,
    );
    let c = capture(root.path());
    assert!(c
        .report()
        .diagnostics
        .iter()
        .any(|d| d.reason == "symlink_config_unsupported"));
    assert_eq!(
        module_resolution::resolve(c.model(), "safe.ts", "marker").status,
        ModuleStatus::Unsupported
    );
}
#[test]
fn configuration_directory_is_not_a_regular_file() {
    let root = tempfile::tempdir().unwrap();
    put(root.path(), "use.ts", "export const value=1;\n");
    std::fs::create_dir(root.path().join("tsconfig.json")).unwrap();
    assert!(capture(root.path())
        .report()
        .diagnostics
        .iter()
        .any(|d| d.reason == "config_not_regular"));
}
#[test]
fn sfc_script_imports_use_the_same_captured_typescript_rules() {
    for suffix in ["vue", "svelte"] {
        let root = tempfile::tempdir().unwrap();
        let file = format!("src/Component.{suffix}");
        put(
            root.path(),
            &file,
            "<script>import { helper } from './helper'; helper();</script>",
        );
        put(root.path(), "src/helper.ts", "export function helper(){}\n");
        let c = capture(root.path());
        assert_eq!(
            module_resolution::resolve(c.model(), &file, "./helper")
                .resolved_path
                .as_deref(),
            Some("src/helper.ts")
        );
    }
}
#[test]
fn excessive_go_package_members_are_unknown_not_an_invalid_resolved_set() {
    use cc_model::go_project::{GoConfig, GoProject, GoSourceFacts};
    let mut go = GoProject::default();
    go.configs.insert(
        "go.mod".into(),
        GoConfig {
            module: Some("example.com/app".into()),
            ..Default::default()
        },
    );
    let members = (0..4097)
        .map(|n| format!("api/f{n:05}.go"))
        .collect::<Vec<_>>();
    for member in &members {
        go.sources.insert(
            member.clone(),
            GoSourceFacts {
                package: Some("api".into()),
                ..Default::default()
            },
        );
    }
    go.directories.insert("api".into(), members.clone());
    let files = FileCatalog::new(members.into_iter().chain(["use.go".into()]).collect()).unwrap();
    let model = ProjectModel::new(
        files,
        Default::default(),
        Default::default(),
        Default::default(),
    )
    .with_go(go);
    let result = module_resolution::resolve(&model, "use.go", "example.com/app/api");
    assert_eq!(result.status, ModuleStatus::Unknown);
    assert!(result.resolved_package.is_none());
    assert_eq!(
        result.reason.as_deref(),
        Some("go_package_member_budget_exceeded")
    );
    assert_eq!(
        result.candidates.len(),
        64,
        "a bounded witness must be emitted, not an unreachable preview branch"
    );
    assert!(result.valid_target_shape());
}
#[test]
fn rejected_import_previews_do_not_collapse_distinct_request_identities() {
    let model = ProjectModel::new(
        FileCatalog::new(Default::default()).unwrap(),
        Default::default(),
        Default::default(),
        Default::default(),
    );
    let a = module_resolution::resolve(&model, "use.ts", &format!("{}a", "x".repeat(8192)));
    let b = module_resolution::resolve(&model, "use.ts", &format!("{}b", "x".repeat(8192)));
    assert_eq!(a.status, ModuleStatus::Unsupported);
    assert_eq!(a.import_string, b.import_string);
    assert_ne!(a.request_key, b.request_key);
    assert!(a.reason.unwrap().contains("bounded_preview"));
}
#[test]
fn rust_alias_entry_index_preserves_ambiguity_and_rebuilds_after_removal() {
    use cc_model::module_inputs::{RustCrate, RustProject};
    let mut project = RustProject::default();
    for owner in ["one/Cargo.toml", "two/Cargo.toml"] {
        project.crates.insert(
            owner.into(),
            RustCrate {
                entry: "shared/lib.rs".into(),
                ..Default::default()
            },
        );
    }
    project.index_locations();
    assert_eq!(project.by_entry["shared/lib.rs"].len(), 2);
    project.crates.remove("two/Cargo.toml");
    project.index_locations();
    assert_eq!(project.by_entry["shared/lib.rs"], ["one/Cargo.toml"]);
}
#[test]
fn ordinary_extends_filename_suffix_is_not_a_package_manifest_identity() {
    for name in ["base-package.json", "subpackage.json"] {
        let root = tempfile::tempdir().unwrap();
        put(root.path(), "use.ts", "export const value=1;\n");
        put(root.path(), "target.ts", "export const marker=1;\n");
        put(
            root.path(),
            "tsconfig.json",
            &serde_json::json!({"extends":format!("./{name}")}).to_string(),
        );
        put(
            root.path(),
            name,
            r#"{"compilerOptions":{"baseUrl":".","paths":{"marker":["target.ts"]}}}"#,
        );
        let c = capture(root.path());
        let result = module_resolution::resolve(c.model(), "use.ts", "marker");
        assert_eq!(result.status, ModuleStatus::Resolved, "{name}: {result:?}");
        assert_eq!(result.resolved_path.as_deref(), Some("target.ts"));
        assert!(c.report().diagnostics.is_empty(), "{:?}", c.report());
    }
}
#[test]
fn absolute_and_url_extends_never_fetch_or_execute() {
    for extends in [
        "https://example.invalid/config.json",
        "file:///tmp/config.json",
        "/tmp/config.json",
        "../../outside.json",
    ] {
        let root = tempfile::tempdir().unwrap();
        put(root.path(), "use.ts", "export const value=1;\n");
        put(
            root.path(),
            "tsconfig.json",
            &serde_json::json!({"extends":extends}).to_string(),
        );
        let c = capture(root.path());
        assert_eq!(
            module_resolution::resolve(c.model(), "use.ts", "./other").status,
            ModuleStatus::Unsupported
        );
        assert_eq!(c.report().config_reads, 1);
    }
}
