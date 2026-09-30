//! Independent module answers and explicit unsupported cases. No user program runs.
use cc_index::{
    module_resolution,
    project_model::{discover, CapturedProject},
    Scanner,
};
use cc_model::{module_inputs::*, project_model::*, ImportRecord};
use std::path::Path;
fn put(root: &Path, path: &str, text: &str) {
    let p = root.join(path);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}
fn capture(root: &Path) -> CapturedProject {
    let (s, m) = Scanner::new(root, &Default::default()).scan_with_manifest();
    discover(
        root,
        s.into_iter().map(|f| f.rel_path).collect(),
        Some(&m),
        None,
        &Default::default(),
    )
    .unwrap()
}
fn request(c: &CapturedProject, file: &str, spec: &str, syntax: ImportSyntax) -> ModuleResolution {
    module_resolution::resolve_import(
        c.model(),
        file,
        &ImportRecord {
            file_path: file.into(),
            import_string: spec.into(),
            context: ImportContext {
                syntax,
                ..Default::default()
            },
            ..Default::default()
        },
    )
}
fn pkg(exports: &str) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    let r = d.path();
    put(r, "package.json", r#"{"workspaces":["packages/*"]}"#);
    put(
        r,
        "tsconfig.json",
        r#"{"compilerOptions":{"moduleResolution":"bundler"}}"#,
    );
    put(r, "use.ts", "export const value=1;\n");
    for p in ["first.ts", "second.ts", "types.d.ts"] {
        put(r, &format!("packages/lib/{p}"), "export const value=1;\n");
    }
    put(
        r,
        "packages/lib/package.json",
        &format!(r#"{{"name":"demo","exports":{exports}}}"#),
    );
    d
}
#[test]
fn package_conditions_follow_source_order_not_a_fixed_precedence() {
    for (exports, expect) in [
        (
            r#"{".":{"default":"./first.ts","types":"./types.d.ts"}}"#,
            "first.ts",
        ),
        (
            r#"{".":{"types":"./types.d.ts","default":"./first.ts"}}"#,
            "types.d.ts",
        ),
    ] {
        let d = pkg(exports);
        let c = capture(d.path());
        assert_eq!(
            request(&c, "use.ts", "demo", ImportSyntax::Static).resolved_path,
            Some(format!("packages/lib/{expect}"))
        );
    }
}
#[test]
fn import_and_require_are_independent_conditions() {
    let d = pkg(r#"{".":{"import":"./first.ts","require":"./second.ts"}}"#);
    let c = capture(d.path());
    for (syntax, p) in [
        (ImportSyntax::Static, "first.ts"),
        (ImportSyntax::Require, "second.ts"),
    ] {
        assert_eq!(
            request(&c, "use.ts", "demo", syntax).resolved_path,
            Some(format!("packages/lib/{p}"))
        );
    }
}
#[test]
fn package_patterns_choose_specific_subpath_and_null_does_not_fallback() {
    let d = pkg(r#"{"./secret":null,"./*":"./first.ts","./long/*":"./second.ts"}"#);
    let c = capture(d.path());
    assert_eq!(
        request(&c, "use.ts", "demo/long/name", ImportSyntax::Static)
            .resolved_path
            .as_deref(),
        Some("packages/lib/second.ts")
    );
    let blocked = request(&c, "use.ts", "demo/secret", ImportSyntax::Static);
    assert_eq!(blocked.status, ModuleStatus::Unresolved);
    assert!(blocked.resolved_path.is_none());
}
#[test]
fn arrays_do_not_retry_an_absent_file_after_a_valid_target() {
    let d = pkg(r#"{".":["./missing.ts","./first.ts"]}"#);
    let c = capture(d.path());
    let r = request(&c, "use.ts", "demo", ImportSyntax::Static);
    assert!(r.resolved_path.is_none());
    assert!(r.probes.iter().all(|p| !p.ends_with("first.ts")));
}
#[test]
fn package_targets_reject_escape_encoded_segments_and_node_modules() {
    for target in [
        "../outside.ts",
        "./../outside.ts",
        "./%2e%2e/outside.ts",
        "./node_modules/secret.ts",
        "/outside.ts",
    ] {
        let d = pkg(&format!(r#"{{".":"{target}"}}"#));
        let c = capture(d.path());
        assert_eq!(
            request(&c, "use.ts", "demo", ImportSyntax::Static).status,
            ModuleStatus::Unsupported,
            "{target}"
        );
    }
}
#[test]
fn duplicate_package_names_are_not_disambiguated_by_map_order() {
    let d = pkg(r#""./first.ts""#);
    put(
        d.path(),
        "packages/other/package.json",
        r#"{"name":"demo","exports":"./main.ts"}"#,
    );
    put(d.path(), "packages/other/main.ts", "export const x=1;\n");
    let c = capture(d.path());
    let r = request(&c, "use.ts", "demo", ImportSyntax::Static);
    assert_eq!(r.status, ModuleStatus::Ambiguous);
    assert_eq!(r.candidates.len(), 2);
    assert_eq!(
        r.reason.as_deref(),
        Some("ambiguous_workspace_package_name")
    );
}
#[test]
fn unrelated_package_is_not_a_workspace_dependency() {
    let d = pkg(r#""./first.ts""#);
    put(d.path(), "package.json", "{}");
    let c = capture(d.path());
    assert!(request(&c, "use.ts", "demo", ImportSyntax::Static)
        .resolved_path
        .is_none());
}
#[test]
fn package_imports_redirect_locally_and_cycles_are_explicit() {
    let d = pkg(r#""./first.ts""#);
    put(
        d.path(),
        "package.json",
        r##"{"workspaces":["packages/*"],"imports":{"#api":"demo","#a":"#b","#b":"#a"}}"##,
    );
    let c = capture(d.path());
    assert_eq!(
        request(&c, "use.ts", "#api", ImportSyntax::Static)
            .resolved_path
            .as_deref(),
        Some("packages/lib/first.ts")
    );
    assert_eq!(
        request(&c, "use.ts", "#a", ImportSyntax::Static)
            .reason
            .as_deref(),
        Some("package_imports_cycle")
    );
}
#[test]
fn node_mode_uses_package_type_and_does_not_add_esm_extensions() {
    let d = pkg(r#"{".":{"import":"./first.ts","require":"./second.ts"}}"#);
    put(
        d.path(),
        "tsconfig.json",
        r#"{"compilerOptions":{"moduleResolution":"node16"}}"#,
    );
    put(
        d.path(),
        "package.json",
        r#"{"type":"module","workspaces":["packages/*"]}"#,
    );
    put(d.path(), "local.ts", "export const x=1;\n");
    let c = capture(d.path());
    assert_eq!(
        request(&c, "use.ts", "demo", ImportSyntax::Static)
            .resolved_path
            .as_deref(),
        Some("packages/lib/first.ts")
    );
    assert_eq!(
        request(&c, "use.ts", "./local", ImportSyntax::Static)
            .reason
            .as_deref(),
        Some("node_esm_relative_extension_required")
    );
    assert_eq!(
        request(&c, "use.ts", "./local.js", ImportSyntax::Static)
            .resolved_path
            .as_deref(),
        Some("local.ts")
    );
    assert_eq!(
        request(&c, "use.ts", "./local", ImportSyntax::Require)
            .resolved_path
            .as_deref(),
        Some("local.ts")
    );
}
#[test]
fn invalid_duplicate_package_keys_are_not_sorted_and_accepted() {
    let d = pkg(r#"{".":"./first.ts",".":"./second.ts"}"#);
    let c = capture(d.path());
    assert!(request(&c, "use.ts", "demo", ImportSyntax::Static)
        .resolved_path
        .is_none());
}
#[test]
fn rust_declared_paths_cfg_and_super_have_separate_outcomes() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path();
    put(r,"Cargo.toml","[package]\nname=\"test\"\nversion=\"0.1.0\"\n[features]\ndefault=[\"enabled\"]\nenabled=[]\n");
    put(r,"src/lib.rs","#[path=\"custom.rs\"] pub mod api;\n#[cfg(feature=\"enabled\")]pub mod active;\n#[cfg(target_os=\"linux\")]pub mod platform;\n");
    for f in ["custom", "active", "platform"] {
        put(r, &format!("src/{f}.rs"), "pub fn ping(){}\n");
    }
    let c = capture(r);
    assert_eq!(
        request(
            &c,
            "src/lib.rs",
            "crate::api::ping",
            ImportSyntax::Unspecified
        )
        .resolved_path
        .as_deref(),
        Some("src/custom.rs")
    );
    assert_eq!(
        request(
            &c,
            "src/lib.rs",
            "crate::active::ping",
            ImportSyntax::Unspecified
        )
        .resolved_path
        .as_deref(),
        Some("src/active.rs")
    );
    assert_eq!(
        request(
            &c,
            "src/lib.rs",
            "crate::platform::ping",
            ImportSyntax::Unspecified
        )
        .status,
        ModuleStatus::Unsupported
    );
    assert_eq!(
        request(
            &c,
            "src/custom.rs",
            "super::active::ping",
            ImportSyntax::Unspecified
        )
        .resolved_path
        .as_deref(),
        Some("src/active.rs")
    );
}
#[test]
fn cargo_optional_and_development_dependencies_do_not_become_unconditional() {
    for line in [
        "[dependencies]\nother={path=\"other\",optional=true}",
        "[dev-dependencies]\nother={path=\"other\"}",
    ] {
        let d = tempfile::tempdir().unwrap();
        let root = d.path();
        put(root,"Cargo.toml",&format!("[workspace]\nmembers=[\"other\"]\n[package]\nname=\"sample\"\nversion=\"0.1.0\"\n{line}\n"));
        put(root, "src/lib.rs", "pub fn run(){}\n");
        put(
            root,
            "other/Cargo.toml",
            "[package]\nname=\"other\"\nversion=\"0.1.0\"\n",
        );
        put(root, "other/src/lib.rs", "pub fn ping(){}\n");
        let c = capture(root);
        assert_eq!(
            request(&c, "src/lib.rs", "other::ping", ImportSyntax::Unspecified).status,
            ModuleStatus::Unsupported
        );
    }
}
#[test]
fn rust_duplicate_module_files_and_complex_cfg_fail_closed() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path();
    put(
        r,
        "Cargo.toml",
        "[package]\nname=\"test\"\nversion=\"0.1.0\"\n",
    );
    put(
        r,
        "src/lib.rs",
        "pub mod api;\n#[cfg_attr(feature=\"x\",path=\"elsewhere.rs\")]pub mod other;\n",
    );
    for p in ["src/api.rs", "src/api/mod.rs", "src/other.rs"] {
        put(r, p, "pub fn ping(){}\n");
    }
    let c = capture(r);
    assert_eq!(
        request(
            &c,
            "src/lib.rs",
            "crate::api::ping",
            ImportSyntax::Unspecified
        )
        .reason
        .as_deref(),
        Some("rust_module_file_ambiguous")
    );
    assert_eq!(
        request(
            &c,
            "src/lib.rs",
            "crate::other::ping",
            ImportSyntax::Unspecified
        )
        .status,
        ModuleStatus::Unsupported
    );
}
#[test]
fn python_explicit_src_root_relative_parent_and_namespace() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path();
    put(
        r,
        "pyproject.toml",
        "[tool.setuptools.packages.find]\nwhere=[\"source\"]\n",
    );
    put(r, "source/pkg/api.py", "def ping():\n    return 1\n");
    put(r, "source/pkg/sub/use.py", "from ..api import ping\n");
    let c = capture(r);
    assert_eq!(
        request(
            &c,
            "source/pkg/sub/use.py",
            "..api",
            ImportSyntax::Unspecified
        )
        .resolved_path
        .as_deref(),
        Some("source/pkg/api.py")
    );
    assert_eq!(
        request(
            &c,
            "source/pkg/sub/use.py",
            "pkg.api",
            ImportSyntax::Unspecified
        )
        .resolved_path
        .as_deref(),
        Some("source/pkg/api.py")
    );
    assert_eq!(
        request(
            &c,
            "source/pkg/sub/use.py",
            "pkg",
            ImportSyntax::Unspecified
        )
        .reason
        .as_deref(),
        Some("namespace_package_has_no_single_source_file")
    );
    assert!(request(
        &c,
        "source/pkg/sub/use.py",
        "...api",
        ImportSyntax::Unspecified
    )
    .resolved_path
    .is_none());
}
#[test]
fn namespace_portion_does_not_override_a_regular_package_in_later_root() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path();
    put(
        r,
        "pyproject.toml",
        "[tool.setuptools.packages.find]\nwhere=[\"one\",\"two\"]\n",
    );
    put(r, "one/ns/api.py", "def ping(): pass\n");
    put(r, "two/ns/__init__.py", "value=1\n");
    put(r, "use.py", "import ns\n");
    let c = capture(r);
    assert_eq!(
        request(&c, "use.py", "ns", ImportSyntax::Unspecified)
            .resolved_path
            .as_deref(),
        Some("two/ns/__init__.py")
    );
}
#[test]
fn nested_workspace_resolution_uses_the_nearest_declared_workspace() {
    let d = pkg(r#""./first.ts""#);
    let r = d.path();
    put(r, "nested/package.json", r#"{"workspaces":["libs/*"]}"#);
    put(
        r,
        "nested/libs/lib/package.json",
        r#"{"name":"demo","exports":"./local.ts"}"#,
    );
    put(r, "nested/libs/lib/local.ts", "export const x=1;\n");
    put(r, "nested/use.ts", "export const x=1;\n");
    let c = capture(r);
    assert_eq!(
        request(&c, "nested/use.ts", "demo", ImportSyntax::Static)
            .resolved_path
            .as_deref(),
        Some("nested/libs/lib/local.ts")
    );
}
#[test]
fn relative_directory_uses_package_entry_before_index_in_cjs_and_bundler() {
    let d = pkg(r#""./first.ts""#);
    put(d.path(), "dir/package.json", r#"{"main":"./custom.js"}"#);
    put(d.path(), "dir/custom.ts", "export const x=1;\n");
    put(d.path(), "dir/index.ts", "export const x=2;\n");
    let c = capture(d.path());
    assert_eq!(
        request(&c, "use.ts", "./dir", ImportSyntax::Static)
            .resolved_path
            .as_deref(),
        Some("dir/custom.ts")
    );
}
#[test]
fn format_extensions_and_dynamic_import_override_cjs_package_type() {
    let d = pkg(r#"{".":{"import":"./first.ts","require":"./second.ts"}}"#);
    put(
        d.path(),
        "tsconfig.json",
        r#"{"compilerOptions":{"moduleResolution":"nodenext"}}"#,
    );
    put(d.path(), "entry.mts", "export const x=1;\n");
    put(d.path(), "entry.cts", "export const x=1;\n");
    let c = capture(d.path());
    assert_eq!(
        request(&c, "entry.mts", "demo", ImportSyntax::Static)
            .resolved_path
            .as_deref(),
        Some("packages/lib/first.ts")
    );
    assert_eq!(
        request(&c, "entry.cts", "demo", ImportSyntax::Static)
            .resolved_path
            .as_deref(),
        Some("packages/lib/second.ts")
    );
    assert_eq!(
        request(&c, "entry.cts", "demo", ImportSyntax::Dynamic)
            .resolved_path
            .as_deref(),
        Some("packages/lib/first.ts")
    );
}
#[test]
fn module_resolution_survives_removal_of_captured_input_files() {
    let d = pkg(r#""./first.ts""#);
    let c = capture(d.path());
    std::fs::remove_file(d.path().join("packages/lib/package.json")).unwrap();
    std::fs::remove_file(d.path().join("packages/lib/first.ts")).unwrap();
    assert_eq!(
        request(&c, "use.ts", "demo", ImportSyntax::Static)
            .resolved_path
            .as_deref(),
        Some("packages/lib/first.ts")
    );
    assert!(c.verify(d.path()).is_err());
}
