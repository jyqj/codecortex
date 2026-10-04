//! R1 positive regression v1; preserves independent historical characterization.
use cc_model::{declaration_identity::*, source::ByteSpan};
use cc_parsers::python_identity::*;
use std::collections::BTreeMap;

const PATH: &str = "src/r1/probe.py";
const NESTED: &[u8] = b"\xef\xbb\xbf\r\n  # lead\r\n\r\n@one\r\nclass C:\r\n    @two\r\n    async def f(self):\r\n        pass\r\n  \t\r\n";
fn inputs(source: &[u8]) -> Vec<DeclarationInput> {
    match declaration_inputs(PATH, source, PythonIdentityLimits::default()).unwrap() {
        PythonIdentityOutcome::Inputs(inputs) => inputs,
        other => panic!("{source:?}: {other:?}"),
    }
}
fn segment(
    name: &str,
    kind: ScopeKind,
    start: usize,
    end: usize,
    ns: usize,
    ne: usize,
) -> DeclarationSegment {
    DeclarationSegment {
        name: name.into(),
        kind,
        span: ByteSpan { start, end },
        name_span: ByteSpan { start: ns, end: ne },
    }
}
fn tree(source: &[u8]) -> tree_sitter::Tree {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_python::LANGUAGE.into())
        .unwrap();
    parser.parse(source, None).unwrap()
}
fn snapshot(source: &[u8]) -> DeclarationSnapshot {
    let config = b"[tool.setuptools.package-dir]\n\"\" = \"src\"\n";
    DeclarationSnapshot::new(
        "r1-positive-byte-witnesses-v1".into(),
        BTreeMap::from([
            (PATH.into(), source.to_vec()),
            ("pyproject.toml".into(), config.to_vec()),
            (
                "src/r1/__init__.py".into(),
                b"raise RuntimeError('not imported')\n".to_vec(),
            ),
        ]),
        vec![ConfiguredRoot {
            directory: "src".into(),
            config_path: "pyproject.toml".into(),
            config_digest: content_digest(config),
            directive: "tool.setuptools.package-dir.empty".into(),
        }],
    )
    .unwrap()
}
#[test]
fn r1_characterized_leading_trivia_now_has_exact_absolute_byte_identity() {
    for (source, root_start, start, end, ns, ne) in [
        (b"\n\ndef f(): pass\n".as_slice(), 2, 2, 15, 6, 7),
        (
            b"  # lead\r\n\r\ndef f(): pass\r\n  ".as_slice(),
            2,
            12,
            25,
            16,
            17,
        ),
        (b"\xef\xbb\xbfdef f(): pass\r\n".as_slice(), 3, 3, 16, 7, 8),
    ] {
        let t = tree(source);
        assert_eq!(t.root_node().kind(), "module");
        assert!(!t.root_node().has_error());
        assert_eq!(t.root_node().start_byte(), root_start);
        assert_eq!(t.root_node().end_byte(), source.len());
        let expected = DeclarationInput {
            file_path: PATH.into(),
            source_digest: content_digest(source),
            ancestry: vec![segment("f", ScopeKind::Function, start, end, ns, ne)],
        };
        let actual = inputs(source);
        assert_eq!(actual.as_slice(), std::slice::from_ref(&expected));
        let s = snapshot(source);
        assert_eq!(
            s.resolve(&actual[0]).unwrap(),
            s.resolve(&expected).unwrap()
        );
        let IdentityOutcome::Derived(bound) = s.resolve(&actual[0]).unwrap() else {
            panic!("expected derived")
        };
        assert_eq!(bound.binding().source_digest, content_digest(source));
        assert_eq!(bound.address().module, ["r1", "probe"]);
        assert!(s.is_current(&bound).unwrap());
    }
}
#[test]
fn blank_comment_and_bom_only_modules_are_empty_successes() {
    for source in [
        b"".as_slice(),
        b"\n\n",
        b" \t\r\n\r\n",
        b"# only\r\n",
        b"  # only\r\n  \t\r\n",
        b"\xef\xbb\xbf",
        b"\xef\xbb\xbf\r\n  # only\r\n\r\n",
    ] {
        let t = tree(source);
        assert_eq!(t.root_node().kind(), "module");
        assert!(!t.root_node().has_error());
        assert!(inputs(source).is_empty());
    }
    for source in [b" \t\r\n\r\n".as_slice(), b"\xef\xbb\xbf"] {
        assert_eq!(tree(source).root_node().start_byte(), source.len());
    }
}
#[test]
fn nested_decorators_after_bom_comment_crlf_keep_exact_witnesses_and_digest() {
    assert_eq!(NESTED.len(), 86);
    let class = segment("C", ScopeKind::Class, 17, 79, 29, 30);
    let function = segment("f", ScopeKind::Function, 37, 79, 57, 58);
    let expected: Vec<_> = [vec![class.clone()], vec![class, function]]
        .into_iter()
        .map(|ancestry| DeclarationInput {
            file_path: PATH.into(),
            source_digest: content_digest(NESTED),
            ancestry,
        })
        .collect();
    let actual = inputs(NESTED);
    assert_eq!(actual, expected);
    let s = snapshot(NESTED);
    for (actual, expected) in actual.iter().zip(&expected) {
        assert_eq!(s.resolve(actual).unwrap(), s.resolve(expected).unwrap());
        assert!(matches!(
            s.resolve(actual).unwrap(),
            IdentityOutcome::Derived(_)
        ));
    }
    // Prefix/trailing bytes are digest evidence even though outside declaration spans.
    let mut changed = NESTED.to_vec();
    changed[7] = b'!';
    assert_eq!(
        snapshot(&changed).resolve(&actual[0]).unwrap(),
        IdentityOutcome::Unavailable(IdentityReason::StaleSource)
    );
    let mut changed = NESTED.to_vec();
    changed.push(b'\n');
    assert_eq!(
        snapshot(&changed).resolve(&actual[0]).unwrap(),
        IdentityOutcome::Unavailable(IdentityReason::StaleSource)
    );
}
#[test]
fn prefixes_never_hide_malformed_siblings_or_anonymous_missing_tokens() {
    for prefix in [b"\n\n".as_slice(), b"  # lead\r\n\r\n", b"\xef\xbb\xbf"] {
        for bad in [
            b"def broken(\n".as_slice(),
            b"class C(A, meta:\n    def child(self): pass\n",
        ] {
            let source = [prefix, b"def good(): pass\n", bad].concat();
            assert_eq!(
                declaration_inputs(PATH, &source, PythonIdentityLimits::default()).unwrap(),
                PythonIdentityOutcome::Unavailable(PythonIdentityReason::SyntaxError)
            );
        }
        for bad in [b"def empty():\n".as_slice(), b"def if(): pass\n"] {
            let source = [prefix, b"def good(): pass\n", bad].concat();
            assert_eq!(
                declaration_inputs(PATH, &source, PythonIdentityLimits::default()).unwrap(),
                PythonIdentityOutcome::Unavailable(PythonIdentityReason::UnsupportedAst)
            );
        }
    }
}
#[test]
fn original_trivia_counts_toward_source_budget_and_all_other_budgets_remain() {
    let default = PythonIdentityLimits::default();
    let actual = inputs(NESTED);
    let output_text = actual
        .iter()
        .map(|i| {
            i.file_path.len()
                + i.source_digest.len()
                + i.ancestry.iter().map(|s| s.name.len()).sum::<usize>()
        })
        .sum();
    let exact = PythonIdentityLimits {
        source_bytes: NESTED.len(),
        declarations: 2,
        output_segments: 3,
        output_text_bytes: output_text,
        ..default
    };
    assert_eq!(
        declaration_inputs(PATH, NESTED, exact).unwrap(),
        PythonIdentityOutcome::Inputs(actual)
    );
    for (limits, reason) in [
        (
            PythonIdentityLimits {
                source_bytes: NESTED.len() - 1,
                ..exact
            },
            PythonIdentityReason::SourceLimit,
        ),
        (
            PythonIdentityLimits {
                declarations: 1,
                ..exact
            },
            PythonIdentityReason::OutputLimit,
        ),
        (
            PythonIdentityLimits {
                output_segments: 2,
                ..exact
            },
            PythonIdentityReason::OutputLimit,
        ),
        (
            PythonIdentityLimits {
                output_text_bytes: output_text - 1,
                ..exact
            },
            PythonIdentityReason::OutputLimit,
        ),
        (
            PythonIdentityLimits {
                visited_nodes: 1,
                ..exact
            },
            PythonIdentityReason::WorkLimit,
        ),
        (
            PythonIdentityLimits {
                tree_depth: 1,
                ..exact
            },
            PythonIdentityReason::DepthLimit,
        ),
    ] {
        assert_eq!(
            declaration_inputs(PATH, NESTED, limits).unwrap(),
            PythonIdentityOutcome::Unavailable(reason)
        );
    }
    assert_eq!(
        declaration_inputs(
            PATH,
            b" \t\r\n\r\n",
            PythonIdentityLimits {
                source_bytes: 5,
                ..default
            }
        )
        .unwrap(),
        PythonIdentityOutcome::Unavailable(PythonIdentityReason::SourceLimit)
    );
}
