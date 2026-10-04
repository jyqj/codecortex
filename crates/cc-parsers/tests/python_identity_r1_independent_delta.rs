//! Positive delta for fixed source 7d49beb; historical R1 remains in its own target.
use cc_model::{declaration_identity::*, source::ByteSpan};
use cc_parsers::python_identity::*;
use std::collections::BTreeMap;
const PATH: &str = "src/review/probe.py";
const NESTED: &[u8] = include_bytes!("fixtures/python_identity_independent/nested.py");
const PREFIXES: &[&[u8]] = &[
    b"\n\n",
    b"  # lead\r\n\r\n",
    b"\xef\xbb\xbf",
    b"\xef\xbb\xbf\r\n  # extra \xe9\x9b\xaa\r\n\r\n",
];
fn tree(source: &[u8]) -> tree_sitter::Tree {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_python::LANGUAGE.into())
        .unwrap();
    parser.parse(source, None).unwrap()
}
fn inputs(source: &[u8]) -> Vec<DeclarationInput> {
    match declaration_inputs(PATH, source, PythonIdentityLimits::default()).unwrap() {
        PythonIdentityOutcome::Inputs(v) => v,
        other => panic!("{source:?}: {other:?}"),
    }
}
fn snapshot(source: &[u8]) -> DeclarationSnapshot {
    let config = b"[tool.setuptools.package-dir]\n\"\" = \"src\"\n";
    DeclarationSnapshot::new(
        "independent-r1-delta".into(),
        BTreeMap::from([
            (PATH.into(), source.to_vec()),
            ("pyproject.toml".into(), config.to_vec()),
            (
                "src/review/__init__.py".into(),
                b"raise RuntimeError('never execute')\n".to_vec(),
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
fn unavailable(source: &[u8], limits: PythonIdentityLimits, reason: PythonIdentityReason) {
    assert_eq!(
        declaration_inputs(PATH, source, limits).unwrap(),
        PythonIdentityOutcome::Unavailable(reason)
    );
}
#[test]
fn every_original_r1_counterexample_now_succeeds_at_absolute_spans() {
    for (source, root_start, decl_start, decl_end, name_start) in [
        (b"\n\ndef f(): pass\n".as_slice(), 2, 2, 15, 6),
        (
            b"  # lead\r\n\r\ndef f(): pass\r\n  ".as_slice(),
            2,
            12,
            25,
            16,
        ),
        (b"\xef\xbb\xbfdef f(): pass\r\n".as_slice(), 3, 3, 16, 7),
    ] {
        let t = tree(source);
        assert!(!t.root_node().has_error());
        assert_eq!(t.root_node().start_byte(), root_start);
        let expected = DeclarationInput {
            file_path: PATH.into(),
            source_digest: content_digest(source),
            ancestry: vec![DeclarationSegment {
                name: "f".into(),
                kind: ScopeKind::Function,
                span: ByteSpan {
                    start: decl_start,
                    end: decl_end,
                },
                name_span: ByteSpan {
                    start: name_start,
                    end: name_start + 1,
                },
            }],
        };
        let actual = inputs(source);
        assert_eq!(actual, vec![expected.clone()]);
        let s = snapshot(source);
        assert_eq!(
            s.resolve(&actual[0]).unwrap(),
            s.resolve(&expected).unwrap()
        );
        assert!(matches!(
            s.resolve(&actual[0]).unwrap(),
            IdentityOutcome::Derived(_)
        ));
    }
    for source in [
        b" \t\r\n\r\n".as_slice(),
        b"\xef\xbb\xbf",
        b"".as_slice(),
        b"# only\r\n",
        b"# comment\n \t\n",
        b"\n\n# leading only\r\n \t\r\n",
    ] {
        assert!(!tree(source).root_node().has_error());
        assert!(inputs(source).is_empty());
    }
    for source in [
        b"def f(): pass".as_slice(),
        b"def f(): pass\n  \t\n",
        b"# lead\r\ndef f(): pass\r\n",
    ] {
        assert_eq!(inputs(source).len(), 1);
    }
}
#[test]
fn all_nested_ancestries_shift_by_prefix_bytes_and_bind_full_digest() {
    // Baseline exact fixed byte witnesses are checked in the original independent target.
    let baseline = inputs(NESTED);
    assert_eq!(baseline.len(), 7);
    for prefix in PREFIXES {
        let source = [*prefix, NESTED, b" \t\r\n"].concat();
        let mut expected = baseline.clone();
        for input in &mut expected {
            input.source_digest = content_digest(&source);
            for segment in &mut input.ancestry {
                segment.span.start += prefix.len();
                segment.span.end += prefix.len();
                segment.name_span.start += prefix.len();
                segment.name_span.end += prefix.len();
            }
        }
        let actual = inputs(&source);
        assert_eq!(actual, expected);
        let s = snapshot(&source);
        for (index, (input, witness)) in actual.iter().zip(&expected).enumerate() {
            assert_eq!(s.resolve(input).unwrap(), s.resolve(witness).unwrap());
            if (2..=4).contains(&index) {
                assert_eq!(
                    s.resolve(input).unwrap(),
                    IdentityOutcome::Unavailable(IdentityReason::LocalDeclaration)
                );
            } else if let IdentityOutcome::Derived(d) = s.resolve(input).unwrap() {
                assert_eq!(d.address().module, ["review", "probe"]);
                assert!(s.is_current(&d).unwrap());
            } else {
                panic!();
            }
        }
        assert_ne!(actual[0].source_digest, baseline[0].source_digest);
        let changed = [source.as_slice(), b"\n"].concat();
        assert_eq!(
            snapshot(&changed).resolve(&actual[0]).unwrap(),
            IdentityOutcome::Unavailable(IdentityReason::StaleSource)
        );
    }
}
#[test]
fn accepted_prefixes_do_not_hide_whole_file_error_missing_or_unsupported_shape() {
    for prefix in PREFIXES {
        for bad in [
            b"def broken(\n".as_slice(),
            b"class C(A, meta:\n    def child(self): pass\n",
            b"@\ndef bad(): pass\n",
        ] {
            let source = [*prefix, b"def good(): pass\n", bad].concat();
            let t = tree(&source);
            assert!(t.root_node().has_error());
            if bad.starts_with(b"class C") {
                assert!(t.root_node().to_sexp().contains("MISSING"));
            }
            unavailable(
                &source,
                PythonIdentityLimits::default(),
                PythonIdentityReason::SyntaxError,
            );
        }
        for bad in [
            b"def empty():\n".as_slice(),
            b"class Empty:\n",
            b"def comment():\n    # only\n",
            b"def if(): pass\n",
            b"class True: pass\n",
        ] {
            let source = [*prefix, b"def good(): pass\n", bad].concat();
            unavailable(
                &source,
                PythonIdentityLimits::default(),
                PythonIdentityReason::UnsupportedAst,
            );
        }
        let source = [*prefix, b"def good(): pass\n", &[0xff]].concat();
        unavailable(
            &source,
            PythonIdentityLimits::default(),
            PythonIdentityReason::InvalidUtf8,
        );
    }
}
fn metrics(node: tree_sitter::Node<'_>, depth: usize) -> (usize, usize) {
    let (mut count, mut deepest) = (1, depth);
    for child in node.children(&mut node.walk()) {
        let (n, d) = metrics(child, depth + 1);
        count += n;
        deepest = deepest.max(d);
    }
    (count, deepest)
}
#[test]
fn full_source_and_post_tree_output_work_depth_limits_remain_exact() {
    let default = PythonIdentityLimits::default();
    for prefix in PREFIXES {
        let source = [*prefix, NESTED, b" \t\r\n"].concat();
        let expected = inputs(&source);
        let t = tree(&source);
        let (nodes, depth) = metrics(t.root_node(), 1);
        let segments = expected.iter().map(|i| i.ancestry.len()).sum::<usize>();
        let text = expected
            .iter()
            .map(|i| {
                i.file_path.len()
                    + i.source_digest.len()
                    + i.ancestry.iter().map(|s| s.name.len()).sum::<usize>()
            })
            .sum::<usize>();
        let exact = PythonIdentityLimits {
            source_bytes: source.len(),
            visited_nodes: nodes,
            tree_depth: depth,
            declarations: expected.len(),
            output_segments: segments,
            output_text_bytes: text,
            ..default
        };
        assert_eq!(
            declaration_inputs(PATH, &source, exact).unwrap(),
            PythonIdentityOutcome::Inputs(expected)
        );
        for (limits, reason) in [
            (
                PythonIdentityLimits {
                    source_bytes: source.len() - 1,
                    ..exact
                },
                PythonIdentityReason::SourceLimit,
            ),
            (
                PythonIdentityLimits {
                    visited_nodes: nodes - 1,
                    ..exact
                },
                PythonIdentityReason::WorkLimit,
            ),
            (
                PythonIdentityLimits {
                    tree_depth: depth - 1,
                    ..exact
                },
                PythonIdentityReason::DepthLimit,
            ),
            (
                PythonIdentityLimits {
                    declarations: 6,
                    ..exact
                },
                PythonIdentityReason::OutputLimit,
            ),
            (
                PythonIdentityLimits {
                    output_segments: segments - 1,
                    ..exact
                },
                PythonIdentityReason::OutputLimit,
            ),
            (
                PythonIdentityLimits {
                    output_text_bytes: text - 1,
                    ..exact
                },
                PythonIdentityReason::OutputLimit,
            ),
        ] {
            unavailable(&source, limits, reason);
        }
    }
    for source in [b" \t\r\n\r\n".as_slice(), b"\xef\xbb\xbf"] {
        unavailable(
            source,
            PythonIdentityLimits {
                source_bytes: source.len() - 1,
                ..default
            },
            PythonIdentityReason::SourceLimit,
        );
        assert_eq!(
            declaration_inputs(
                PATH,
                source,
                PythonIdentityLimits {
                    source_bytes: source.len(),
                    visited_nodes: 1,
                    tree_depth: 1,
                    ..default
                }
            )
            .unwrap(),
            PythonIdentityOutcome::Inputs(vec![])
        );
    }
}
