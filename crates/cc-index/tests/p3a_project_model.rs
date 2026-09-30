//! Independent configuration and immutable module-resolution contracts.
use cc_index::{
    module_resolution,
    project_model::{discover, CapturedProject},
    Scanner,
};
use cc_model::{config::IndexingConfig, project_model::*};
use std::{collections::BTreeSet, path::Path};
fn put(root: &Path, path: &str, text: &str) {
    let p = root.join(path);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}
fn capture(root: &Path, old: &ProjectInputs) -> CapturedProject {
    let s = Scanner::new(root, &IndexingConfig::default());
    let (files, m) = s.scan_with_manifest();
    discover(
        root,
        files.into_iter().map(|f| f.rel_path).collect(),
        Some(&m),
        None,
        old,
    )
    .unwrap()
}
fn module(c: &CapturedProject, file: &str, spec: &str) -> ModuleResolution {
    module_resolution::resolve(c.model(), file, spec)
}
#[test]
fn configuration_is_loaded_once_and_resolution_is_disk_free() {
    let d = tempfile::tempdir().unwrap();
    put(
        d.path(),
        "tsconfig.json",
        r#"{"compilerOptions":{"moduleResolution":"bundler","baseUrl":".","paths":{"@api":["src/api"]}}}"#,
    );
    put(d.path(), "src/api.ts", "export function f(){}\n");
    put(d.path(), "use.ts", "import {f} from '@api'; f();\n");
    let c = capture(d.path(), &Default::default());
    assert_eq!(c.report().config_reads, 1);
    let expected = module(&c, "use.ts", "@api");
    assert_eq!(expected.resolved_path.as_deref(), Some("src/api.ts"));
    std::fs::remove_file(d.path().join("src/api.ts")).unwrap();
    std::fs::remove_file(d.path().join("tsconfig.json")).unwrap();
    for _ in 0..1000 {
        assert_eq!(module(&c, "use.ts", "@api"), expected);
    }
    assert!(
        c.verify(d.path()).is_err(),
        "publishing stale config must fail"
    );
}
#[test]
fn cache_uses_content_even_when_mtime_and_size_match() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "use.ts", "import {f} from '@api';\n");
    for p in ["a.ts", "b.ts"] {
        put(d.path(), p, "export function f(){}\n");
    }
    let config =
        |file: &str| format!(r#"{{"compilerOptions":{{"paths":{{"@api":["./{file}"]}}}}}}"#);
    put(d.path(), "tsconfig.json", &config("a.ts"));
    let a = capture(d.path(), &Default::default());
    let again = capture(d.path(), a.inputs());
    assert_eq!(again.report().config_parse_cache_hits, 1);
    let p = d.path().join("tsconfig.json");
    let time = p.metadata().unwrap().modified().unwrap();
    let size = p.metadata().unwrap().len();
    put(d.path(), "tsconfig.json", &config("b.ts"));
    std::fs::OpenOptions::new()
        .write(true)
        .open(&p)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(time))
        .unwrap();
    assert_eq!(p.metadata().unwrap().len(), size);
    let b = capture(d.path(), a.inputs());
    assert_eq!(b.report().config_parse_cache_hits, 0);
    assert_eq!(
        module(&b, "use.ts", "@api").resolved_path.as_deref(),
        Some("b.ts")
    );
    assert_ne!(a.report().input_digest, b.report().input_digest);
}
#[test]
fn extends_array_and_option_origins_follow_override_order() {
    let d = tempfile::tempdir().unwrap();
    put(
        d.path(),
        "config/a.json",
        r#"{"compilerOptions":{"moduleResolution":"bundler","baseUrl":"../one","paths":{"@api":["api"],"@old":["old"]}}}"#,
    );
    put(
        d.path(),
        "config/b.json",
        r#"{"compilerOptions":{"paths":{"@api":["../two/api"]}}}"#,
    );
    put(
        d.path(),
        "pkg/tsconfig.json",
        r#"{"extends":["../config/a","../config/b"]}"#,
    );
    put(d.path(), "pkg/use.ts", "import {f} from '@api';\n");
    put(d.path(), "two/api.ts", "export function f(){}\n");
    let c = capture(d.path(), &Default::default());
    let config = c.model().nearest_config("pkg/use.ts").unwrap();
    assert_eq!(config.base_url.as_deref(), Some("one"));
    assert!(!config.paths.as_ref().unwrap().patterns.contains_key("@old"));
    assert_eq!(
        module(&c, "pkg/use.ts", "@api").resolved_path.as_deref(),
        Some("two/api.ts")
    );
    assert_eq!(config.dependencies.len(), 3);
}
#[test]
fn cycles_escape_missing_and_unsupported_modes_are_not_empty_success() {
    for (source, base, reason) in [
        (
            r#"{"extends":"./base"}"#,
            Some(r#"{"extends":"./tsconfig"}"#),
            "extends_cycle",
        ),
        (
            r#"{"extends":"../outside"}"#,
            None,
            "extends_outside_project",
        ),
        (r#"{"extends":"./base"}"#, None, "missing_config"),
        (
            r#"{"extends":"@tsconfig/node"}"#,
            None,
            "package_or_absolute_extends_unsupported",
        ),
        (
            r#"{"compilerOptions":{"moduleResolution":"classic"}}"#,
            None,
            "module_resolution_classic_unsupported_in_p3a",
        ),
        (
            r#"{"compilerOptions":{"baseUrl":"../../escape"}}"#,
            None,
            "invalid_or_outside_base_url",
        ),
    ] {
        let d = tempfile::tempdir().unwrap();
        put(d.path(), "use.ts", "import {f} from './api'; f();\n");
        put(d.path(), "api.ts", "export function f(){}\n");
        put(d.path(), "tsconfig.json", source);
        if let Some(base) = base {
            put(d.path(), "base.json", base);
        }
        let c = capture(d.path(), &Default::default());
        let r = module(&c, "use.ts", "./api");
        assert_eq!(r.status, ModuleStatus::Unsupported, "{r:?}");
        assert!(r.reason.unwrap().contains(reason));
        assert!(r.resolved_path.is_none());
    }
}
#[test]
fn exact_pattern_precedes_wildcard_and_longer_prefix_wins() {
    let d = tempfile::tempdir().unwrap();
    put(
        d.path(),
        "tsconfig.json",
        r#"{"compilerOptions":{"moduleResolution":"bundler","paths":{"@*": ["./wide/*"],"@lib/*": ["./specific/*"],"@lib/api":["./exact"]}}}"#,
    );
    for p in [
        "wide/lib/api.ts",
        "specific/api.ts",
        "exact.ts",
        "specific/other.ts",
        "use.ts",
    ] {
        put(d.path(), p, "export function f(){}\n");
    }
    let c = capture(d.path(), &Default::default());
    assert_eq!(
        module(&c, "use.ts", "@lib/api").resolved_path.as_deref(),
        Some("exact.ts")
    );
    assert_eq!(
        module(&c, "use.ts", "@lib/other").resolved_path.as_deref(),
        Some("specific/other.ts")
    );
}
#[test]
fn supported_extension_substitution_and_probe_order_are_explicit() {
    let d = tempfile::tempdir().unwrap();
    put(
        d.path(),
        "tsconfig.json",
        r#"{"compilerOptions":{"moduleResolution":"bundler"}}"#,
    );
    for p in [
        "use.ts",
        "api.js",
        "api.ts",
        "esm.mts",
        "common.cts",
        "folder/index.ts",
    ] {
        put(d.path(), p, "export function f(){}\n");
    }
    let c = capture(d.path(), &Default::default());
    for (spec, target) in [
        ("./api.js", "api.ts"),
        ("./esm.mjs", "esm.mts"),
        ("./common.cjs", "common.cts"),
        ("./folder", "folder/index.ts"),
    ] {
        assert_eq!(
            module(&c, "use.ts", spec).resolved_path.as_deref(),
            Some(target)
        );
    }
    assert_eq!(module(&c, "use.ts", "./api.js").probes[0], "api.ts");
}
#[test]
fn a_missing_base_is_observed_then_recovers() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "use.ts", "import {f} from '@api';\n");
    put(d.path(), "a.ts", "export function f(){}\n");
    put(d.path(), "tsconfig.json", r#"{"extends":"./base"}"#);
    let a = capture(d.path(), &Default::default());
    assert_eq!(
        module(&a, "use.ts", "@api").status,
        ModuleStatus::Unsupported
    );
    put(
        d.path(),
        "base.json",
        r#"{"compilerOptions":{"paths":{"@api":["./a"]}}}"#,
    );
    let b = capture(d.path(), a.inputs());
    assert_eq!(
        module(&b, "use.ts", "@api").resolved_path.as_deref(),
        Some("a.ts")
    );
    assert!(b
        .report()
        .changed_configs
        .contains(&"base.json".to_string()));
}
#[test]
fn nested_invalid_config_masks_parent_and_tsconfig_precedes_jsconfig() {
    let d = tempfile::tempdir().unwrap();
    put(
        d.path(),
        "tsconfig.json",
        r#"{"compilerOptions":{"paths":{"@api":["./a"]}}}"#,
    );
    put(d.path(), "pkg/jsconfig.json", "{}");
    put(d.path(), "pkg/tsconfig.json", "{ broken");
    put(d.path(), "pkg/use.ts", "import {f} from '@api';\n");
    put(d.path(), "a.ts", "export function f(){}\n");
    let c = capture(d.path(), &Default::default());
    assert_eq!(
        c.model().nearest_config("pkg/use.ts").unwrap().path,
        "pkg/tsconfig.json"
    );
    assert_eq!(
        module(&c, "pkg/use.ts", "@api").status,
        ModuleStatus::Unsupported
    );
}
#[cfg(unix)]
#[test]
fn symlinked_config_is_explicitly_unsupported() {
    let d = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    put(outside.path(), "base.json", "{}");
    put(d.path(), "use.ts", "export const x=1;\n");
    put(d.path(), "tsconfig.json", r#"{"extends":"./base"}"#);
    std::os::unix::fs::symlink(outside.path().join("base.json"), d.path().join("base.json"))
        .unwrap();
    let c = capture(d.path(), &Default::default());
    assert!(c
        .report()
        .diagnostics
        .iter()
        .any(|d| d.reason == "symlink_config_unsupported"));
}
#[test]
fn repeated_inheritance_deduplicates_diagnostics_instead_of_expanding_paths() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "use.ts", "export const x=1;\n");
    for level in 0..6 {
        let name = if level == 0 {
            "tsconfig.json".into()
        } else {
            format!("c{level}.json")
        };
        let next = if level == 5 {
            "./missing".into()
        } else {
            format!("./c{}", level + 1)
        };
        put(
            d.path(),
            &name,
            &serde_json::json!({"extends":vec![next;16]}).to_string(),
        );
    }
    let c = capture(d.path(), &Default::default());
    assert_eq!(c.report().diagnostics.len(), 1);
    assert_eq!(c.report().diagnostics[0].reason, "missing_config");
    assert_eq!(c.report().config_reads, 6);
}
#[test]
fn inheritance_depth_and_per_file_byte_limits_are_reported() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "use.ts", "export const x=1;\n");
    put(d.path(), "tsconfig.json", r#"{"extends":"./c0"}"#);
    for n in 0..35 {
        put(
            d.path(),
            &format!("c{n}.json"),
            &serde_json::json!({"extends":format!("./c{}",n+1)}).to_string(),
        );
    }
    let c = capture(d.path(), &Default::default());
    assert!(c
        .report()
        .diagnostics
        .iter()
        .any(|d| d.reason == "extends_depth_limit"));
    put(d.path(), "tsconfig.json", &" ".repeat(1024 * 1024 + 1));
    let c = capture(d.path(), &Default::default());
    assert!(c
        .report()
        .diagnostics
        .iter()
        .any(|d| d.reason == "config_size_limit"));
}
#[test]
fn many_shared_effective_configs_are_bounded_independently_of_raw_bytes() {
    let d = tempfile::tempdir().unwrap();
    let patterns: std::collections::BTreeMap<_, _> = (0..128)
        .map(|n| (format!("@p{n}"), vec![format!("./{}", "x".repeat(1024))]))
        .collect();
    put(
        d.path(),
        "base.json",
        &serde_json::json!({"compilerOptions":{"paths":patterns}}).to_string(),
    );
    for n in 0..140 {
        put(d.path(), &format!("p{n}/use.ts"), "export const x=1;\n");
        put(
            d.path(),
            &format!("p{n}/tsconfig.json"),
            r#"{"extends":"../base"}"#,
        );
    }
    let scanner = Scanner::new(d.path(), &Default::default());
    let (files, m) = scanner.scan_with_manifest();
    let r = discover(
        d.path(),
        files.into_iter().map(|f| f.rel_path).collect(),
        Some(&m),
        None,
        &Default::default(),
    );
    assert!(r
        .err()
        .unwrap()
        .to_string()
        .contains("effective_project_config_bytes_limit"));
}
#[test]
fn gitignored_config_is_still_a_semantic_input_not_an_admitted_source_file() {
    let d = tempfile::tempdir().unwrap();
    std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(d.path())
        .status()
        .unwrap();
    put(d.path(), ".gitignore", "tsconfig.json\n");
    put(
        d.path(),
        "tsconfig.json",
        r#"{"compilerOptions":{"paths":{"@api":["./api"]}}}"#,
    );
    put(d.path(), "api.ts", "export const x=1;\n");
    put(d.path(), "use.ts", "import {x} from '@api';\n");
    let scanner = Scanner::new(d.path(), &Default::default());
    let (files, m) = scanner.scan_with_manifest();
    assert!(!files.iter().any(|f| f.rel_path == "tsconfig.json"));
    let c = discover(
        d.path(),
        files.into_iter().map(|f| f.rel_path).collect(),
        Some(&m),
        None,
        &Default::default(),
    )
    .unwrap();
    assert_eq!(
        module(&c, "use.ts", "@api").resolved_path.as_deref(),
        Some("api.ts")
    );
    assert_eq!(c.report().config_roots, 1);
}

#[test]
fn hidden_config_dependencies_and_generated_exclusions_share_watcher_policy() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "use.ts", "import {x} from '@api';\n");
    put(d.path(), "api.ts", "export const x=1;\n");
    put(d.path(), "tsconfig.json", r#"{"extends":"./.config/base"}"#);
    put(
        d.path(),
        ".config/base.json",
        r#"{"compilerOptions":{"paths":{"@api":["../api"]}}}"#,
    );
    let c = capture(d.path(), &Default::default());
    assert_eq!(
        module(&c, "use.ts", "@api").resolved_path.as_deref(),
        Some("api.ts")
    );
    assert!(cc_index::project_model::potential_config_path(
        ".config/base.json"
    ));
    for dir in ["dist", "build", ".cache", ".venv", "node_modules"] {
        put(
            d.path(),
            "tsconfig.json",
            &serde_json::json!({"extends":format!("./{dir}/base")}).to_string(),
        );
        put(d.path(), &format!("{dir}/base.json"), "{}");
        let c = capture(d.path(), &Default::default());
        assert_eq!(
            module(&c, "use.ts", "@api").status,
            ModuleStatus::Unsupported
        );
        assert!(c
            .report()
            .diagnostics
            .iter()
            .any(|d| d.reason == "unsupported_config_path"));
    }
}

#[test]
fn equal_priority_overlapping_paths_do_not_pick_a_map_order_winner() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "use.ts", "export const x=1;\n");
    put(d.path(), "a.ts", "export const x=1;\n");
    put(d.path(), "b.ts", "export const x=1;\n");
    put(
        d.path(),
        "tsconfig.json",
        r#"{"compilerOptions":{"paths":{"@lib/*":["./a"],"@lib/*bar":["./b"]}}}"#,
    );
    let c = capture(d.path(), &Default::default());
    let r = module(&c, "use.ts", "@lib/foobar");
    assert_eq!(r.status, ModuleStatus::Unsupported);
    assert_eq!(
        r.reason.as_deref(),
        Some("equal_priority_paths_patterns_unsupported")
    );
}

#[test]
fn file_catalog_and_paths_reject_noncanonical_or_escaping_inputs() {
    assert!(FileCatalog::new(BTreeSet::from(["../escape.ts".into()])).is_err());
    for p in [
        "/tmp/file",
        "../../escape",
        "C:/file",
        "a\\b",
        ".codecortex/cache",
    ] {
        assert!(join_relative("", p).is_none(), "{p}");
    }
    assert_eq!(
        join_relative("pkg", "../src/api").as_deref(),
        Some("src/api")
    );
}
