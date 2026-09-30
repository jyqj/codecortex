//! Independent source fidelity and structure regressions; no captured-output gold.
use cc_model::{chunk_policy::ChunkPolicy, Language, ParserTier};
use cc_parsers::{chunker::Chunker, ParserRegistry};
#[test]
fn consecutive_file_documentation_stays_one_evidence_unit() {
    let doc="//! Turns incoming requests into a normalized plan.\r\n//!\r\n//! Retains all filters and records execution metadata.\r\n";
    let text = format!("{doc}\r\nconst LIMIT: usize = 1;\r\n");
    let out = ParserRegistry::new()
        .parse("lib.rs", &text, Language::Rust)
        .unwrap();
    assert_source(&text, &out.chunks, 80);
    let hits: Vec<_> = out
        .chunks
        .iter()
        .filter(|c| c.text.contains("//!"))
        .collect();
    assert_eq!(
        hits.len(),
        1,
        "adjacent paragraphs are one file-level doc block"
    );
    assert!(hits[0].text.contains(doc));
    assert!(!hits[0].text.contains("const LIMIT"));
}
#[test]
fn original_crlf_unicode_and_final_newline_are_not_reconstructed() {
    let text = "const 名字 = '值';\r\nconst n = 2;\r\n";
    let chunks = Chunker::from_policy(ChunkPolicy {
        lines: 1,
        ..Default::default()
    })
    .unwrap()
    .chunk_by_lines(
        "src/a.ts",
        text,
        Language::TypeScript,
        ParserTier::Generic,
        0.3,
    );
    assert_eq!(
        chunks.iter().map(|c| c.text.as_str()).collect::<String>(),
        text
    );
    assert_eq!(chunks[0].text, "const 名字 = '值';\r\n");
    assert_eq!(chunks[1].text, "const n = 2;\r\n");
}
#[test]
fn long_python_docstring_and_decorator_stay_with_the_signature() {
    let mut code=String::from("@record\ndef execute():\n    \"\"\"Execute this request without changing its identity.\"\"\"\n");
    for _ in 0..100 {
        code.push_str("    consume()\n");
    }
    let out = ParserRegistry::new()
        .parse("a.py", &code, Language::Python)
        .unwrap();
    assert_source(&code, &out.chunks, 80);
    assert!(
        out.chunks
            .iter()
            .any(|c| c.text.contains("@record\ndef execute():")
                && c.text.contains("Execute this request")),
        "definition documentation should not become an unrelated body fragment"
    );
}
#[test]
fn arrow_signature_ends_at_the_body_not_at_end_of_implementation() {
    let mut code = String::from("export const execute = (x) => {\n");
    for _ in 0..90 {
        code.push_str("  consume(x);\n");
    }
    code.push_str("};\n");
    let out = ParserRegistry::new()
        .parse("a.ts", &code, Language::TypeScript)
        .unwrap();
    let owner = out
        .source_structure
        .as_ref()
        .unwrap()
        .boundaries
        .iter()
        .find(|b| b.name.as_deref() == Some("execute"))
        .unwrap();
    let signature = owner.signature.unwrap();
    assert_eq!(
        &code[signature.start..signature.end],
        "export const execute = (x) => "
    );
    assert_source(&code, &out.chunks, 80);
}
#[test]
fn malformed_tree_coordinates_and_opaque_input_do_not_panic_or_fabricate_source() {
    use cc_model::source::*;
    let text = "fn marker() {}\n";
    let source = SourceSnapshot::new(text.as_bytes());
    let mut structure = SourceStructure::fallback(&source, "deliberately invalid test input");
    structure.boundaries.push(SyntaxBoundary {
        span: source.whole(),
        parent: Some(0),
        kind: BoundaryKind::Symbol,
        name: Some("fabricated".into()),
        symbol_kind: None,
        signature: None,
        leading_comment: None,
        documentation: None,
    });
    let chunks = Chunker::default().from_structure(
        "a.rs",
        &source,
        Language::Rust,
        &structure,
        ParserTier::Generic,
        0.2,
    );
    assert_source(text, &chunks, 80);
    assert!(chunks.iter().all(|c| c.symbol_name.is_none()));
    let opaque = SourceSnapshot::new(&[0xff]);
    assert!(Chunker::default()
        .from_structure(
            "a.rs",
            &opaque,
            Language::Rust,
            &structure,
            ParserTier::Generic,
            0.2
        )
        .is_empty());
}

fn assert_source(text: &str, chunks: &[cc_model::ChunkRecord], budget: u32) {
    let source = cc_model::source::SourceSnapshot::new(text.as_bytes());
    let mut end = 0;
    for c in chunks {
        let e = c.source.as_ref().expect("original source evidence");
        assert_eq!(e.span.start, end, "non-overlapping complete byte partition");
        assert_eq!(source.slice(e.span).unwrap(), c.text);
        assert!(e.validate(&c.text));
        assert_eq!(source.lines(e.span).unwrap(), (c.start_line, c.end_line));
        assert!(c.end_line - c.start_line < budget.max(1));
        assert!(c.text.len() <= 16 * 1024);
        end = e.span.end;
    }
    assert_eq!(end, text.len());
}
#[test]
fn line_fallback_rejects_zero_budget_and_is_total_for_multibyte_and_very_long_lines() {
    assert!(Chunker::from_policy(ChunkPolicy {
        lines: 0,
        ..Default::default()
    })
    .is_err());
    let chunker = Chunker::from_policy(ChunkPolicy {
        lines: 1,
        ..Default::default()
    })
    .unwrap();
    for text in [
        String::new(),
        "\n\r\n  \r".into(),
        "甲".repeat(12000),
        "x\r\n".repeat(100),
    ] {
        let chunks =
            chunker.chunk_by_lines("a.txt", &text, Language::Python, ParserTier::Generic, 0.3);
        assert_source(&text, &chunks, 1);
    }
}
#[test]
fn same_line_symbols_and_multiline_literals_preserve_a_disjoint_union() {
    let code =
        "export function a(){ return '甲'; } export function b(){ return `one\ntwo\nthree`; }\r\n";
    let p = ParserRegistry::new()
        .parse("a.ts", code, Language::TypeScript)
        .unwrap();
    assert_source(code, &p.chunks, 80);
    assert!(p.source_structure.as_ref().unwrap().complete);
}
#[test]
fn long_function_uses_statement_spans_and_retains_signature_coordinates() {
    let mut code = String::from("pub fn work() {\n");
    for n in 0..40 {
        code.push_str(&format!(
            "    if ready({n}) {{\n        consume({n});\n    }}\n"
        ));
    }
    code.push_str("}\n");
    let out = ParserRegistry::new()
        .parse("lib.rs", &code, Language::Rust)
        .unwrap();
    assert_source(&code, &out.chunks, 80);
    for n in 0..40 {
        let expected = format!("if ready({n}) {{\n        consume({n});\n    }}");
        assert!(
            out.chunks.iter().any(|c| c.text.contains(&expected)),
            "statement must not be cut: {n}"
        );
    }
    for chunk in &out.chunks {
        if chunk.symbol_name.as_deref() == Some("work") {
            let signature = chunk.source.as_ref().unwrap().signature.unwrap();
            assert_eq!(&code[signature.start..signature.end], "pub fn work() ");
        }
    }
}
#[test]
fn detached_configuration_comment_is_not_attached_to_next_definition() {
    let code =
        "/** Configuration notes, not function docs. */\n\nexport function run() { return 1; }\n";
    let out = ParserRegistry::new()
        .parse("a.ts", code, Language::TypeScript)
        .unwrap();
    let chunk = out
        .chunks
        .iter()
        .find(|c| c.symbol_name.as_deref() == Some("run"))
        .unwrap();
    assert!(!chunk.text.contains("Configuration notes"));
    assert_source(code, &out.chunks, 80);
}
#[test]
fn all_tree_languages_export_owned_structure_without_changing_source() {
    let registry = ParserRegistry::new();
    for (path, language, code) in [
        (
            "a.py",
            Language::Python,
            "# 说明\r\ndef run():\r\n    return 1\r\n",
        ),
        ("a.rs", Language::Rust, "/// Note\nfn run() {}\n"),
        ("a.go", Language::Go, "package app\nfunc Run() {}\n"),
        ("A.java", Language::Java, "class A { void run() {} }\n"),
        ("a.c", Language::C, "int run(void) { return 1; }\n"),
        ("a.cpp", Language::Cpp, "class A { void run() {} };\n"),
    ] {
        let out = registry.parse(path, code, language).unwrap();
        let structure = out.source_structure.unwrap();
        assert_eq!(structure.capability, "existing_tree_sitter_ast");
        assert!(structure.visited_nodes > 0);
        assert!(
            structure.validate(&cc_model::source::SourceSnapshot::new(code.as_bytes())),
            "{path}"
        );
        let roundtrip: cc_model::source::SourceStructure =
            serde_json::from_str(&serde_json::to_string(&structure).unwrap()).unwrap();
        assert_eq!(roundtrip, structure);
        assert_source(code, &out.chunks, 80);
    }
}
#[test]
fn sfc_two_scripts_keep_original_offsets_after_non_ascii_template() {
    let code="<template>你好</template>\r\n<script>function a(){return 1;}</script>\r\n<p>x</p>\r\n<script>function b(){return a();}</script>\r\n";
    let out = ParserRegistry::new()
        .parse("View.vue", code, Language::Vue)
        .unwrap();
    assert_source(code, &out.chunks, 80);
    let a = out.symbols.iter().find(|s| s.name == "a").unwrap();
    let b = out.symbols.iter().find(|s| s.name == "b").unwrap();
    assert_eq!((a.start_line, b.start_line), (2, 4));
    assert_eq!((a.start_col, b.start_col), (8, 8));
}

#[test]
fn small_containers_preserve_member_and_parent_retrieval_identities() {
    for (path, lang, text, container, method) in [
        (
            "logger.ts",
            Language::TypeScript,
            "class Logger {\n  log(msg) { console.log(msg); }\n}\n",
            "Logger",
            "log",
        ),
        (
            "account.cpp",
            Language::Cpp,
            "class Account {\npublic:\n  int withdraw(int amount) { return amount; }\n};\n",
            "Account",
            "withdraw",
        ),
    ] {
        let out = ParserRegistry::new().parse(path, text, lang).unwrap();
        assert_source(text, &out.chunks, 80);
        assert!(
            out.chunks
                .iter()
                .any(|c| c.symbol_name.as_deref() == Some(container)),
            "parent identity: {path}"
        );
        assert!(
            out.chunks
                .iter()
                .any(|c| c.symbol_name.as_deref() == Some(method)),
            "member identity: {path}"
        );
    }
}

#[test]
fn oversized_class_splits_on_methods_not_arbitrary_windows() {
    let mut text = String::from("export class Service {\n");
    for n in 0..5 {
        text.push_str(&format!(
            "  /** Documentation for method {n}. */\n  method{n}() {{\n"
        ));
        for _ in 0..19 {
            text.push_str("    doWork();\n");
        }
        text.push_str("  }\n");
    }
    text.push_str("}\n");
    let out = ParserRegistry::new()
        .parse("service.ts", &text, Language::TypeScript)
        .unwrap();
    for n in 0..5 {
        let name = format!("method{n}");
        let chunk = out
            .chunks
            .iter()
            .find(|c| c.symbol_name.as_deref() == Some(name.as_str()))
            .expect("method is a structural chunk, not a parent-labelled line window");
        assert!(chunk
            .text
            .contains(&format!("Documentation for method {n}")));
        assert!(chunk.text.contains(&format!("{name}()")));
        assert!(!chunk.text.contains(&format!("method{}()", (n + 1) % 5)));
        assert!(chunk.breadcrumb.contains("Service"));
    }
}
#[test]
fn sfc_chunk_text_is_component_source_not_padded_script() {
    let text = "<template>你好</template>\r\n<script lang=\"ts\">\r\nexport function answer() { return 42; }\r\n</script>\r\n";
    let out = ParserRegistry::new()
        .parse("View.vue", text, Language::Vue)
        .unwrap();
    assert_eq!(
        out.chunks
            .iter()
            .map(|c| c.text.as_str())
            .collect::<String>(),
        text
    );
}
