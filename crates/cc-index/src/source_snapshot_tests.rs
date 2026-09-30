//! Parse/scan coherence, independent of full/incremental agreement.
use crate::{
    indexer::{FileAction, PendingFile},
    Indexer, ScannedFile,
};
use std::sync::Arc;
#[test]
fn parser_never_pairs_reread_bytes_with_the_old_scan_digest() {
    let root = tempfile::tempdir().unwrap();
    let storage = tempfile::tempdir().unwrap();
    let original = "export function value() { return 1; }\r\n";
    let changed = "export function value() { return 2; }\r\n";
    std::fs::write(root.path().join("a.ts"), changed).unwrap();
    let db = Arc::new(
        cc_db::index_db::IndexDb::open(&storage.path().join("index.db"))
            .unwrap()
            .0,
    );
    let index = Indexer::new(db, root.path(), &Default::default());
    let pending = PendingFile {
        scanned: ScannedFile {
            rel_path: "a.ts".into(),
            abs_path: root.path().join("a.ts"),
            language: cc_model::Language::TypeScript,
            size: original.len() as u64,
            mtime: 0.0,
        },
        content_hash: blake3::hash(original.as_bytes()).to_hex().to_string(),
        action: FileAction::Add,
        content: None,
    };
    let result = index
        .phase_parse(root.path(), vec![pending.clone()])
        .unwrap();
    assert!(result.write_units.is_empty());
    assert_eq!(result.parse_errors.len(), 1);
    assert!(result.parse_errors[0].contains("source changed between scan and parse"));
    // A retained snapshot is coherent in its own right even when disk has advanced.
    let mut retained = pending;
    retained.content = Some(Arc::from(original));
    let result = index.phase_parse(root.path(), vec![retained]).unwrap();
    assert!(result.parse_errors.is_empty());
    assert_eq!(result.write_units.len(), 1);
    let unit = &result.write_units[0];
    assert_eq!(
        unit.outcome
            .chunks
            .iter()
            .map(|c| c.text.as_str())
            .collect::<String>(),
        original
    );
    assert_eq!(
        unit.outcome
            .source_structure
            .as_ref()
            .unwrap()
            .source
            .content_digest,
        unit.content_hash
    );
}
