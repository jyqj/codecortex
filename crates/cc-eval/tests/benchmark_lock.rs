use cc_eval::benchmark::{importer_oce, manifest, report, schema::*, validation};
use serde_json::json;
use std::path::{Path, PathBuf};
fn setup() -> (tempfile::TempDir, PathBuf) {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir(d.path().join("src")).unwrap();
    std::fs::write(d.path().join("src/a.py"), "def renew():\n    return 1\n").unwrap();
    let q = json!({"id":"q","category":"semantic_feature","difficulty":2,"language":"python","split":"dev","query_family":"renew","query":"renew","path_prefix":null,"no_answer":false,"expected_files":[],"answers":[{"id":"a","primary":true,"grade":2,"alternatives":[{"path":"a.py","symbol":null,"span":null}]}]});
    std::fs::write(d.path().join("queries.jsonl"), format!("{q}\n")).unwrap();
    let p = d.path().join("suite.json");
    report::json(&p,&json!({"schema_version":1,"name":"fixture","source":{"root":"src","commit":null,"digest":"","files":["a.py"]},"queries":"queries.jsonl","queries_digest":"","scoring":"codecortex-native-v1","repetitions":1,"warmup":0,"seed":7,"timeout_ms":1000,"top_k":10,"engine_config":{"auto_index":{"enabled":false}}})).unwrap();
    let s = manifest::freeze(&p).unwrap();
    report::json(&p, &s).unwrap();
    (d, p)
}
#[test]
fn snapshot_lock_and_copy_keep_original_unchanged() {
    let (d, p) = setup();
    let l = manifest::load(&p).unwrap();
    let copy = manifest::materialize(&l).unwrap();
    assert!(copy.path().join("a.py").exists());
    assert!(!d.path().join("src/.codecortex").exists());
    assert!(!copy.path().join("queries.jsonl").exists());
}
#[test]
fn same_filename_changed_content_rejected() {
    let (d, p) = setup();
    std::fs::write(d.path().join("src/a.py"), "def renew():\n    return 2\n").unwrap();
    assert!(manifest::load(&p).is_err());
}
#[test]
fn changed_question_rejected() {
    let (d, p) = setup();
    std::fs::write(d.path().join("queries.jsonl"), "{}\n").unwrap();
    assert!(manifest::load(&p).is_err());
}
#[test]
fn changed_source_after_validation_rejected() {
    let (d, p) = setup();
    let l = manifest::load(&p).unwrap();
    std::fs::write(d.path().join("src/a.py"), "changed").unwrap();
    assert!(manifest::materialize(&l).is_err());
}
#[test]
fn unknown_schema_and_fields_rejected() {
    let (_, p) = setup();
    assert!(!p.exists());
    assert!(serde_json::from_value::<Query>(json!({"unexpected":true})).is_err());
    let (d, p) = setup();
    let mut v: serde_json::Value = manifest::json_file(&p).unwrap();
    v["schema_version"] = json!(99);
    report::json(&p, &v).unwrap();
    assert!(manifest::load(&p).is_err());
    drop(d);
}
#[test]
fn duplicate_admitted_file_rejected() {
    let (d, _) = setup();
    assert!(manifest::inventory(&d.path().join("src"), &["a.py".into(), "a.py".into()]).is_err());
}
#[cfg(unix)]
#[test]
fn symlink_source_is_not_admitted() {
    let (d, _) = setup();
    std::os::unix::fs::symlink("a.py", d.path().join("src/b.py")).unwrap();
    assert!(manifest::source_bytes(&d.path().join("src"), "b.py").is_err());
}
#[test]
fn lfs_pointer_is_not_source() {
    let (d, _) = setup();
    std::fs::write(
        d.path().join("src/a.py"),
        "version https://git-lfs.github.com/spec/v1\n",
    )
    .unwrap();
    assert!(manifest::source_bytes(&d.path().join("src"), "a.py").is_err());
}
fn git(root: &Path, args: &[&str]) -> String {
    let o = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8(o.stdout).unwrap().trim().to_string()
}
#[test]
fn git_lock_rejects_dirty_even_with_same_head() {
    let (d, _) = setup();
    git(d.path(), &["init", "-q"]);
    git(d.path(), &["add", "."]);
    git(
        d.path(),
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
            "fixture",
        ],
    );
    let head = git(d.path(), &["rev-parse", "HEAD"]);
    assert!(manifest::git_lock(d.path(), Some(&head)).is_ok());
    std::fs::write(d.path().join("src/a.py"), "changed").unwrap();
    assert!(manifest::git_lock(d.path(), Some(&head)).is_err());
    assert!(manifest::git_lock(d.path(), None).is_err());
}
#[test]
fn external_import_count_alias_and_annotation_contract() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("external.jsonl");
    std::fs::write(&p,"\u{feff}{\"query_id\":\"x\",\"category\":\"symbol_location\",\"difficulty\":1,\"query\":\"renew\",\"expected_files\":[\"a.py\"],\"must_contain\":[\"not-scored\"]}\n").unwrap();
    let m = d.path().join("metadata.json");
    report::json(&m,&json!({"schema_version":1,"benchmark":"external.jsonl","questions":1,"repository":{"name":"sample","remote":"https://example.invalid/sample","commit":"a".repeat(40),"describe":null}})).unwrap();
    let (q, _) = importer_oce::import(&p, &m).unwrap();
    assert!(q[0]
        .annotations
        .contains_key("must_contain_annotation_only"));
    validation::queries(&q, ScoreProfile::OceCompat).unwrap();
    let mut v: serde_json::Value = manifest::json_file(&m).unwrap();
    v["questions"] = json!(200);
    report::json(&m, &v).unwrap();
    assert!(importer_oce::import(&p, &m).is_err());
}
