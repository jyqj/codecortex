//! Real files, the production scanner/parser/SQLite writer, and actual FTS.
//! This contains no benchmark question, repository-specific allowlist, or gold.
use cc_db::{index_db::IndexDb, index_db_retrieval::ChunkScope};
use cc_index::{BuildScope, Indexer, Scanner};
use cc_model::config::IndexingConfig;
use std::{collections::BTreeSet, path::Path, sync::Arc};

fn write(root: &Path, path: &str, bytes: &[u8]) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

fn opted(text: bool, hidden: bool) -> IndexingConfig {
    // JSON makes these tests runnable against the old source too: the old
    // config ignores the new keys, and the required source coverage then fails.
    serde_json::from_value(serde_json::json!({
        "include_text_files": text,
        "include_hidden_files": hidden
    }))
    .unwrap()
}

fn paths(scanner: &Scanner, requested: Option<&[String]>) -> BTreeSet<String> {
    let files = match requested {
        Some(paths) => scanner.scan_paths(paths),
        None => scanner.scan(),
    };
    files.into_iter().map(|f| f.rel_path).collect()
}

fn strings(values: &[&str]) -> BTreeSet<String> {
    values.iter().map(|v| (*v).into()).collect()
}

#[test]
fn opt_in_full_directory_and_single_event_scans_share_admission() {
    let root = tempfile::tempdir().unwrap();
    let admitted = [
        "src/lib.rs",
        "README.md",
        "guide.rst",
        "templates/page.html",
        "settings.cfg",
        "LICENSE",
        ".github/workflows/ci.yml",
        ".hidden/a/b/c/d/e/notes.txt",
    ];
    let excluded = [
        ".env",
        "nested/.env.production",
        ".codecortex.json",
        ".codecortex/index.txt",
        ".git/objects/cache.txt",
        ".hg/state.txt",
        ".svn/state.txt",
        ".ssh/key.txt",
        ".config/settings.txt",
        ".cache/cache.txt",
        "node_modules/dependency.txt",
        "target/build.txt",
        "build/compiled.txt",
        "dist/bundle.txt",
        "__pycache__/compiled.txt",
    ];
    for path in admitted.iter().chain(excluded.iter()) {
        write(root.path(), path, b"sourceadmissiontoken\n");
    }
    let mut config = opted(true, true);
    // Even overriding user ignores cannot admit generated/system/secret state
    // through the explicit broad source option.
    config.ignore.clear();
    let scanner = Scanner::new(root.path(), &config);
    let expected = strings(&admitted);
    assert_eq!(paths(&scanner, None), expected);
    let requests: Vec<String> = admitted
        .iter()
        .chain(excluded.iter())
        .map(|v| (*v).into())
        .collect();
    assert_eq!(paths(&scanner, Some(&requests)), expected);
    let mut individual = BTreeSet::new();
    for requested in &requests {
        individual.extend(paths(&scanner, Some(std::slice::from_ref(requested))));
    }
    assert_eq!(individual, expected);
    let subtrees = vec![
        "src".into(),
        "templates".into(),
        ".github".into(),
        ".hidden".into(),
    ];
    assert_eq!(
        paths(&scanner, Some(&subtrees)),
        strings(&[
            "src/lib.rs",
            "templates/page.html",
            ".github/workflows/ci.yml",
            ".hidden/a/b/c/d/e/notes.txt"
        ])
    );
    // Expanded source traversal must not silently expand the config walk's
    // original hidden-depth consumer contract.
    let (_, manifest) = scanner.scan_with_manifest();
    assert!(!manifest
        .files
        .iter()
        .any(|f| f.rel_path == ".hidden/a/b/c/d/e/notes.txt"));
}

#[test]
fn defaults_and_the_two_opt_ins_remain_independent() {
    let root = tempfile::tempdir().unwrap();
    for p in [
        "src/lib.rs",
        "notes.rst",
        ".hidden/secret.rs",
        ".hidden/notes.rst",
    ] {
        write(root.path(), p, b"marker\n");
    }
    assert_eq!(
        paths(&Scanner::new(root.path(), &IndexingConfig::default()), None),
        strings(&["src/lib.rs"])
    );
    assert_eq!(
        paths(&Scanner::new(root.path(), &opted(true, false)), None),
        strings(&["src/lib.rs", "notes.rst"])
    );
    assert_eq!(
        paths(&Scanner::new(root.path(), &opted(false, true)), None),
        strings(&["src/lib.rs", ".hidden/secret.rs"])
    );
    assert_eq!(
        paths(&Scanner::new(root.path(), &opted(true, true)), None),
        strings(&[
            "src/lib.rs",
            "notes.rst",
            ".hidden/secret.rs",
            ".hidden/notes.rst"
        ])
    );
}

#[test]
fn explicit_text_rejects_binary_invalid_utf8_and_over_limit_on_every_entrypoint() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "exact.rst", b"12345678");
    write(root.path(), "large.rst", b"123456789");
    write(root.path(), "nul.rst", b"abc\0def");
    write(root.path(), "nonutf.rst", &[0xff, 0xfe]);
    write(root.path(), "empty.unknown", b"");
    let mut config = opted(true, true);
    config.max_file_bytes = 8;
    let scanner = Scanner::new(root.path(), &config);
    let requested: Vec<String> = [
        "exact.rst",
        "large.rst",
        "nul.rst",
        "nonutf.rst",
        "empty.unknown",
    ]
    .iter()
    .map(|s| (*s).into())
    .collect();
    let expected = strings(&["exact.rst", "empty.unknown"]);
    assert_eq!(paths(&scanner, None), expected);
    assert_eq!(paths(&scanner, Some(&requested)), expected);
}

#[cfg(unix)]
#[test]
fn explicit_text_cannot_read_an_environment_file_through_a_symlink_alias() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), ".env", b"privatefixturevalue\n");
    std::os::unix::fs::symlink(".env", root.path().join("alias.txt")).unwrap();
    let scanner = Scanner::new(root.path(), &opted(true, true));
    assert!(paths(&scanner, None).is_empty());
    assert!(paths(&scanner, Some(&["alias.txt".into()])).is_empty());
}

fn hits(db: &IndexDb, query: &str) -> BTreeSet<String> {
    db.retrieval()
        .fts_chunk_candidates(query, &ChunkScope::default(), 20)
        .unwrap()
        .into_iter()
        .map(|(_, path, _)| path)
        .collect()
}

fn scoped(index: &Indexer, root: &Path, changed: &[&str], removed: &[&str]) {
    let scope = BuildScope {
        changed: changed.iter().map(|p| (*p).into()).collect(),
        removed: removed.iter().map(|p| (*p).into()).collect(),
    };
    let prepared = index
        .prepare_build_scoped(root, false, None, Some(&scope))
        .unwrap();
    let report = index.commit_build(root, false, None, prepared).unwrap();
    assert!(report.parse_errors.is_empty(), "{:?}", report.parse_errors);
}

#[test]
fn real_generic_chunks_fts_updates_hidden_moves_and_deletes_stay_coherent() {
    let root = tempfile::tempdir().unwrap();
    let storage = tempfile::tempdir().unwrap();
    write(root.path(), "guide.rst", b"fallbackalpha\n");
    write(
        root.path(),
        "templates/page.html",
        b"<p>templategamma</p>\n",
    );
    write(root.path(), ".hidden/notes.txt", b"hiddenbeta\n");
    let db = Arc::new(IndexDb::open(&storage.path().join("index.db")).unwrap().0);
    let index = Indexer::new(db.clone(), root.path(), &opted(true, true));
    let report = index.build_index(root.path(), true).unwrap();
    assert!(report.parse_errors.is_empty());
    for (token, path) in [
        ("fallbackalpha", "guide.rst"),
        ("templategamma", "templates/page.html"),
        ("hiddenbeta", ".hidden/notes.txt"),
    ] {
        assert_eq!(hits(&db, token), strings(&[path]));
        assert_eq!(
            db.reads().file_summary(path).unwrap()["parser_tier"],
            "generic"
        );
    }

    write(root.path(), "guide.rst", b"fallbackdelta\n");
    scoped(&index, root.path(), &["guide.rst"], &[]);
    assert!(hits(&db, "fallbackalpha").is_empty());
    assert_eq!(hits(&db, "hiddenbeta"), strings(&[".hidden/notes.txt"]));
    std::fs::remove_file(root.path().join(".hidden/notes.txt")).unwrap();
    scoped(&index, root.path(), &[], &[".hidden/notes.txt"]);
    assert!(hits(&db, "hiddenbeta").is_empty());
    assert_eq!(hits(&db, "fallbackdelta"), strings(&["guide.rst"]));
    assert_eq!(
        hits(&db, "templategamma"),
        strings(&["templates/page.html"])
    );
    assert!(!db.reads().file_is_indexed(".hidden/notes.txt").unwrap());

    std::fs::rename(
        root.path().join("guide.rst"),
        root.path().join(".hidden/moved.rst"),
    )
    .unwrap();
    scoped(&index, root.path(), &[".hidden/moved.rst"], &["guide.rst"]);
    assert_eq!(hits(&db, "fallbackdelta"), strings(&[".hidden/moved.rst"]));
    assert!(!db.reads().file_is_indexed("guide.rst").unwrap());

    // Becoming non-text is a removal from the admitted input, never a stale hit.
    write(root.path(), ".hidden/moved.rst", b"changed\0binary");
    scoped(&index, root.path(), &[".hidden/moved.rst"], &[]);
    assert!(hits(&db, "fallbackdelta").is_empty());
    assert!(!db.reads().file_is_indexed(".hidden/moved.rst").unwrap());
    let before = db.reads().list_file_paths().unwrap();
    index.build_index(root.path(), true).unwrap();
    assert_eq!(db.reads().list_file_paths().unwrap(), before);
    assert_eq!(
        hits(&db, "templategamma"),
        strings(&["templates/page.html"])
    );
}
