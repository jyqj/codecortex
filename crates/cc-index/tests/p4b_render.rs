use cc_index::documents::render::{render, RenderOptions};
use cc_model::{source::SourceSnapshot, Language};
use cc_parsers::ParserRegistry;
#[test]
fn embedding_projection_is_distinct_bounded_deterministic_and_verifies_original() {
    let source = "/** 原文 */\r\nexport function marker(x: number) { return x; }\r\n";
    let snapshot = SourceSnapshot::new(source.as_bytes());
    let mut chunks = ParserRegistry::new()
        .parse("src/a.ts", source, Language::TypeScript)
        .unwrap()
        .chunks;
    let chunk = chunks
        .iter_mut()
        .find(|c| c.symbol_name.as_deref() == Some("marker"))
        .unwrap();
    let original = chunk.text.clone();
    let first = render(&snapshot, chunk, RenderOptions::default()).unwrap();
    let second = render(&snapshot, chunk, RenderOptions::default()).unwrap();
    assert_eq!(first.text, second.text);
    assert_eq!(first.input_hash, second.input_hash);
    assert_eq!(
        &first.text[first.source_range.start..first.source_range.end],
        original
    );
    assert_eq!(chunk.text, original);
    assert!(first.text.contains("src/a.ts"));
    assert!(first.text.contains("typescript"));
    assert!(first.text.len() <= 32768);
    assert_eq!(
        first.input_hash,
        blake3::hash(first.text.as_bytes()).to_hex().as_str()
    );
    let without = render(
        &snapshot,
        chunk,
        RenderOptions {
            include_signature: false,
            ..Default::default()
        },
    )
    .unwrap();
    assert_ne!(first.input_hash, without.input_hash);
    chunk.file_path = "renamed.ts".into();
    let renamed = render(&snapshot, chunk, RenderOptions::default()).unwrap();
    assert_ne!(first.input_hash, renamed.input_hash);
    assert_ne!(first.render_key, renamed.render_key);
    assert_eq!(chunk.text, original);
    let other = SourceSnapshot::new(b"different source");
    assert!(render(&other, chunk, RenderOptions::default()).is_err());
    chunk.text.push('!');
    assert!(render(&snapshot, chunk, RenderOptions::default()).is_err());
}
#[test]
fn metadata_truncation_is_explicit_but_source_is_never_truncated() {
    let text = "export function marker() { return 1; }\n";
    let snapshot = SourceSnapshot::new(text.as_bytes());
    let mut chunk = ParserRegistry::new()
        .parse("a.ts", text, Language::TypeScript)
        .unwrap()
        .chunks
        .remove(0);
    chunk.breadcrumb = "甲😀\n".repeat(2000);
    let r = render(
        &snapshot,
        &chunk,
        RenderOptions {
            metadata_bytes: 256,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(r.metadata_truncated);
    assert_eq!(
        &r.text[r.source_range.start..r.source_range.end],
        chunk.text
    );
    let raw = format!("export const marker = '{}';\n", "x".repeat(400));
    let snapshot = SourceSnapshot::new(raw.as_bytes());
    let chunk = ParserRegistry::new()
        .parse("a.ts", &raw, Language::TypeScript)
        .unwrap()
        .chunks
        .remove(0);
    assert!(render(
        &snapshot,
        &chunk,
        RenderOptions {
            max_bytes: 256,
            ..Default::default()
        }
    )
    .is_err());
    assert!(render(
        &snapshot,
        &chunk,
        RenderOptions {
            max_estimated_tokens: 32,
            ..Default::default()
        }
    )
    .is_err());
}
