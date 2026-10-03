//! Baseline diagnostic for the proposed source-bound symbol association.
//! These candidates are observations, not persisted identity authority.
use cc_model::{source::SourceSnapshot, Language, SymbolRecord};
use cc_parsers::ParserRegistry;

fn exact_candidates<'a>(
    source: &SourceSnapshot<'_>,
    owner: cc_model::source::ByteSpan,
    symbols: &'a [SymbolRecord],
    file: &str,
) -> Vec<&'a SymbolRecord> {
    let start = source.point(owner.start).unwrap();
    let end = source.point(owner.end).unwrap();
    symbols
        .iter()
        .filter(|s| {
            s.file_path == file
                && (s.start_line as usize, s.start_col as usize) == start
                && (s.end_line as usize, s.end_col as usize) == end
        })
        .collect()
}

#[test]
fn python_exact_owner_coordinates_distinguish_classes_and_decorated_declarations() {
    let text = "# 中文\r\nclass Alpha:\r\n    def needle(self):\r\n        return 'α'\r\nclass Beta:\r\n    @staticmethod\r\n    def needle():\r\n        return 'β'\r\n";
    let source = SourceSnapshot::new(text.as_bytes());
    let parsed = ParserRegistry::new()
        .parse("a.py", text, Language::Python)
        .unwrap();
    let structure = parsed.source_structure.as_ref().unwrap();
    assert!(structure.complete && structure.validate(&source));
    let mut names = Vec::new();
    for chunk in &parsed.chunks {
        let proof = chunk.source.as_ref().unwrap();
        assert_eq!(proof.source, *source.identity());
        assert_eq!(source.slice(proof.span).unwrap(), chunk.text);
        if chunk.symbol_name.as_deref() != Some("needle") {
            continue;
        }
        let candidates = exact_candidates(&source, proof.owner.unwrap(), &parsed.symbols, "a.py");
        assert_eq!(
            candidates.len(),
            1,
            "owner={:?}; candidates={candidates:?}",
            proof.owner
        );
        names.push(candidates[0].qname.as_deref().unwrap());
    }
    names.sort();
    assert_eq!(names, ["Alpha.needle", "Beta.needle"]);
}

#[test]
fn go_long_method_fragments_have_exact_method_owner_not_neighbor_or_type() {
    let mut text = String::from("package sample\n// 中文\ntype Alpha struct {}\ntype Beta struct {}\nfunc (a Alpha) needle() int {\n    value := 0\n");
    for i in 0..240 {
        text.push_str(&format!("    value += {i}\n"));
    }
    text.push_str("    return value\n}\nfunc (b Beta) needle() int { return 2 }\n");
    let source = SourceSnapshot::new(text.as_bytes());
    let parsed = ParserRegistry::new()
        .parse("a.go", &text, Language::Go)
        .unwrap();
    let structure = parsed.source_structure.as_ref().unwrap();
    assert!(structure.complete && structure.validate(&source));
    let mut alpha = 0;
    let mut beta = 0;
    for chunk in &parsed.chunks {
        let proof = chunk.source.as_ref().unwrap();
        assert_eq!(source.slice(proof.span).unwrap(), chunk.text);
        if chunk.symbol_name.as_deref() != Some("needle") {
            continue;
        }
        let candidates = exact_candidates(&source, proof.owner.unwrap(), &parsed.symbols, "a.go");
        assert_eq!(candidates.len(), 1);
        match candidates[0].qname.as_deref().unwrap() {
            "Alpha.needle" => alpha += 1,
            "Beta.needle" => beta += 1,
            other => panic!("wrong owner: {other}"),
        }
    }
    assert!(alpha > 1, "long method must really split");
    assert_eq!(beta, 1);
}

#[test]
fn python_nested_symbol_preserves_lexical_function_scope() {
    let text = "def outer():\n    def inner():\n        return '中文'\n    return inner()\n";
    let parsed = ParserRegistry::new()
        .parse("nested.py", text, Language::Python)
        .unwrap();
    let inner = parsed.symbols.iter().find(|s| s.name == "inner").unwrap();
    assert_eq!(inner.qname.as_deref(), Some("outer.inner"));
    assert_eq!(inner.container.as_deref(), Some("outer"));
    assert_eq!(inner.kind, cc_model::SymbolKind::Function);
    assert_eq!(inner.receiver_type, None);
}
