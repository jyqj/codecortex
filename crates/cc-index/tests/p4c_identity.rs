use cc_index::documents::{delta, render};
use cc_model::{identity::DocumentRecord, source::SourceSnapshot, Language};
use cc_parsers::ParserRegistry;
fn docs(path: &str, text: &str) -> Vec<DocumentRecord> {
    let out = ParserRegistry::new()
        .parse(path, text, Language::Python)
        .unwrap();
    delta::prepare(&SourceSnapshot::new(text.as_bytes()), &out, &[])
        .unwrap()
        .records
}
#[test]
fn content_keys_and_versions_are_not_legacy_ordinals() {
    let text = "def before():\n    return 0\n\ndef sample():\n    return 1\n";
    let a = docs("a.py", text);
    let b = docs("a.py", &format!("# Header\n\n{text}"));
    assert!(!a.is_empty() && !b.is_empty());
    let old = a
        .iter()
        .find(|r| text[r.source.span.start..r.source.span.end].contains("def sample"))
        .unwrap();
    // An unchanged exact slice can preserve its key across a position change.
    let new = b
        .iter()
        .find(|r| r.source.slice_digest == old.source.slice_digest)
        .expect("unchanged second function slice retained");
    assert_eq!(old.reference.doc_key, new.reference.doc_key);
    assert_ne!(old.reference.doc_version, new.reference.doc_version);
    let renamed = docs("other.py", "def sample():\n    return 1\n");
    assert!(renamed
        .iter()
        .all(|n| a.iter().all(|o| n.reference.doc_key != o.reference.doc_key)));
    let d = delta::compare(
        &a.iter().map(|r| r.reference.clone()).collect::<Vec<_>>(),
        &a,
    );
    assert!(d.upsert.is_empty() && d.removed.is_empty());
    assert_eq!(d.unchanged, a.len());
    assert_eq!(d.reusable_inputs, a.len());
}
#[test]
fn duplicate_content_keeps_provenance_and_mutated_input_is_rejected() {
    let text = "def sample():\n    return 1\n";
    let out = ParserRegistry::new()
        .parse("a.py", text, Language::Python)
        .unwrap();
    let source = SourceSnapshot::new(text.as_bytes());
    let chunk = &out.chunks[0];
    let input = render::render(&source, chunk, render::RenderOptions::default()).unwrap();
    let policy = out.chunk_policy.as_deref().unwrap();
    let a = DocumentRecord::new(
        chunk,
        policy,
        0,
        "authored-shared-input-test",
        Ok(input.clone()),
    )
    .unwrap();
    let mut other = chunk.clone();
    other.file_path = "b.py".into();
    other.chunk_id = "chunk:b.py:0".into();
    let b =
        DocumentRecord::new(&other, policy, 0, "authored-shared-input-test", Ok(input)).unwrap();
    assert_ne!(a.reference.doc_key, b.reference.doc_key);
    assert_eq!(a.reference.encoding_key, b.reference.encoding_key);
    let c = DocumentRecord::new(
        chunk,
        policy,
        1,
        "authored-shared-input-test",
        Ok(a.input.clone().unwrap()),
    )
    .unwrap();
    assert_ne!(a.reference.doc_key, c.reference.doc_key);
    let mut corrupt = a.clone();
    corrupt.input.as_mut().unwrap().text.push('X');
    assert!(corrupt.validate(&chunk.text).is_err());
    let changed = DocumentRecord::new(
        chunk,
        policy,
        0,
        "different-encoding-spec",
        Ok(a.input.unwrap()),
    )
    .unwrap();
    assert_ne!(a.reference.doc_version, changed.reference.doc_version);
    assert_ne!(a.reference.encoding_key, changed.reference.encoding_key);
}
