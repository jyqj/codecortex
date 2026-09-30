//! Independent source partition checks, not output-captured gold.
use cc_model::Language;
use cc_model::{chunk_policy::ChunkPolicy, source::*};
use cc_parsers::ParserRegistry;
fn exact(text: &str, result: &cc_model::ParseOutcome, policy: ChunkPolicy) {
    let mut pos = 0;
    for c in &result.chunks {
        let p = c.source.as_ref().unwrap();
        assert!(p.validate(&c.text));
        assert_eq!(p.span.start, pos);
        assert_eq!(&text[p.span.start..p.span.end], c.text);
        assert!(c.text.len() <= policy.bytes as usize);
        assert!(c.text.chars().count() <= policy.chars as usize);
        assert!(c.token_estimate <= policy.estimated_tokens);
        assert!(c.end_line - c.start_line < policy.lines);
        if p.span.end < text.len() {
            assert!(
                !(text.as_bytes()[p.span.end - 1] == b'\r' && text.as_bytes()[p.span.end] == b'\n')
            );
        }
        pos = p.span.end;
    }
    assert_eq!(pos, text.len());
}
#[test]
fn every_budget_bounds_head_gaps_symbols_tail_and_unicode_scalar_boundaries() {
    let cases = [
        (
            "a.ts",
            Language::TypeScript,
            format!(
                "/** doc */\r\nexport function marker() {{\r\n{}return '甲😀';\r\n}}\r\n{}",
                "  call();\r\n".repeat(20),
                "// comment\r\n".repeat(20)
            ),
        ),
        (
            "a.vue",
            Language::Vue,
            format!(
                "<template>{}</template>\r\n<script>function marker(){{return 1;}}</script>\r\n",
                "甲😀".repeat(30)
            ),
        ),
        (
            "a.yaml",
            Language::Yaml,
            format!("value: {}\r\n{}", "甲😀".repeat(100), "\r\n".repeat(10)),
        ),
        (
            "a.cs",
            Language::CSharp,
            "class Marker { void Run() {} }\r\n".into(),
        ),
    ];
    for bytes in [4, 5, 7, 31, 64, 4096] {
        for chars in [2, 3, 9, 500] {
            let p = ChunkPolicy {
                lines: 3,
                bytes,
                chars,
                estimated_tokens: 17,
                ..Default::default()
            };
            for (path, lang, text) in &cases {
                let r = ParserRegistry::with_chunk_policy(p)
                    .parse(path, text, *lang)
                    .unwrap();
                exact(text, &r, p);
            }
        }
    }
}
#[test]
fn nested_control_scopes_and_distinct_sibling_symbols_are_merge_barriers() {
    let source=format!("export function first() {{\n{}if (flag) {{\n  branch();\n}} else {{\n  alternative();\n}}\nreturn 1;\n}}\nexport function second() {{ return 2; }}\n","  consume();\n".repeat(90));
    let result = ParserRegistry::new()
        .parse("a.ts", &source, Language::TypeScript)
        .unwrap();
    exact(&source, &result, ChunkPolicy::default());
    assert!(result
        .chunks
        .iter()
        .any(|c| c.symbol_name.as_deref() == Some("first")));
    assert!(result
        .chunks
        .iter()
        .any(|c| c.symbol_name.as_deref() == Some("second")));
    assert!(!result
        .chunks
        .iter()
        .any(|c| c.text.contains("second()") && c.text.contains("return 1")));
}
#[test]
fn duplicate_contained_and_crossing_spans_never_duplicate_or_omit_bytes() {
    let text = "a = 1;\nb = 2;\nc = 3;\n";
    let snapshot = SourceSnapshot::new(text.as_bytes());
    let mut s = SourceStructure::fallback(&snapshot, "authored_overlap");
    for (start, end) in [(0, 14), (0, 14), (7, 21), (0, 7)] {
        s.boundaries.push(SyntaxBoundary {
            span: ByteSpan { start, end },
            parent: None,
            kind: BoundaryKind::Statement,
            name: None,
            symbol_kind: None,
            signature: None,
            leading_comment: None,
            documentation: None,
        });
    }
    let chunks = cc_parsers::chunker::Chunker::from_policy(ChunkPolicy {
        lines: 2,
        ..Default::default()
    })
    .unwrap()
    .from_structure(
        "a.ts",
        &snapshot,
        Language::TypeScript,
        &s,
        cc_model::ParserTier::Generic,
        0.3,
    );
    exact(
        text,
        &cc_model::ParseOutcome {
            chunks,
            ..Default::default()
        },
        ChunkPolicy {
            lines: 2,
            ..Default::default()
        },
    );
}
#[test]
fn malformed_syntax_is_explicit_partial_while_empty_source_is_empty() {
    let p = ChunkPolicy {
        bytes: 32,
        ..Default::default()
    };
    let registry = ParserRegistry::with_chunk_policy(p);
    let text = "export function broken( {\n// unfinished\n";
    let r = registry.parse("a.ts", text, Language::TypeScript).unwrap();
    exact(text, &r, p);
    assert!(!r.source_structure.unwrap().complete);
    assert!(registry
        .parse("a.ts", "", Language::TypeScript)
        .unwrap()
        .chunks
        .is_empty());
    assert!(
        ParserRegistry::with_chunk_policy(ChunkPolicy { lines: 0, ..p })
            .parse("a.ts", "", Language::TypeScript)
            .is_err()
    );
}

#[test]
fn independent_control_flow_units_are_not_coalesced_together() {
    for branch in [
        "if (flag) { firstBranch(); }\nif (other) { secondBranch(); }",
        "if (flag) firstBranch();\nif (other) secondBranch();",
    ] {
        let text = format!(
            "function evaluate() {{\n{}{}\n}}\n",
            "consume();\n".repeat(90),
            branch
        );
        let result = ParserRegistry::new()
            .parse("a.ts", &text, Language::TypeScript)
            .unwrap();
        exact(&text, &result, ChunkPolicy::default());
        assert!(
            !result
                .chunks
                .iter()
                .any(|c| c.text.contains("firstBranch") && c.text.contains("secondBranch")),
            "distinct control-flow units are semantic merge barriers"
        );
    }
}
#[test]
fn adjacent_small_statements_are_not_individual_search_candidates() {
    let source = format!(
        "export function calculate() {{\n{}return 1;\n}}\n",
        "  consume();\n".repeat(150)
    );
    let result = ParserRegistry::new()
        .parse("a.ts", &source, Language::TypeScript)
        .unwrap();
    assert_eq!(
        result
            .chunks
            .iter()
            .map(|c| c.text.as_str())
            .collect::<String>(),
        source
    );
    assert!(
        result.chunks.len() < 25,
        "same-domain tiny fragments should coalesce: {}",
        result.chunks.len()
    );
}
