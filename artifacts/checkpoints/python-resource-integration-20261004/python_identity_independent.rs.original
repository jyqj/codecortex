//! Independent byte witnesses and characterization of review defect R1.
use cc_model::{declaration_identity::*, source::ByteSpan, CcError};
use cc_parsers::python_identity::*;
use std::collections::BTreeMap;
const PATH: &str = "src/review/probe.py";
const SOURCE: &[u8] = include_bytes!("fixtures/python_identity_independent/nested.py");
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
fn unavailable(source: &[u8], limits: PythonIdentityLimits, reason: PythonIdentityReason) {
    assert_eq!(
        declaration_inputs(PATH, source, limits).unwrap(),
        PythonIdentityOutcome::Unavailable(reason)
    );
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
#[test]
fn independent_nested_exact_bytes_and_pure_model() {
    use ScopeKind::*;
    assert_eq!(
        content_digest(SOURCE),
        "08fc7ab660a3744baff4ffd57b0ff87b3c3810632e61054babcdb3f09161f520"
    );
    let outer = segment("Outer", Class, 7, 376, 27, 32);
    let method = segment("method", Function, 39, 260, 60, 66);
    let inner = segment("inner", Function, 106, 239, 130, 135);
    let hidden = segment("Hidden", Class, 156, 239, 162, 168);
    let leaf = segment("leaf", Function, 191, 239, 223, 227);
    let expected: Vec<_> = [
        vec![outer.clone()],
        vec![outer.clone(), method.clone()],
        vec![outer.clone(), method.clone(), inner.clone()],
        vec![outer.clone(), method.clone(), inner.clone(), hidden.clone()],
        vec![outer.clone(), method, inner, hidden, leaf],
        vec![outer.clone(), segment("same", Function, 284, 316, 304, 308)],
        vec![outer, segment("same", Function, 337, 376, 364, 368)],
    ]
    .into_iter()
    .map(|ancestry| DeclarationInput {
        file_path: PATH.into(),
        source_digest: content_digest(SOURCE),
        ancestry,
    })
    .collect();
    assert_eq!(inputs(SOURCE), expected);
    // This is an explicit configuration fixture assertion, not a production capture.
    let config = b"[tool.setuptools.package-dir]\n\"\" = \"src\"\n";
    let snapshot = DeclarationSnapshot::new(
        "independent-review".into(),
        BTreeMap::from([
            (PATH.into(), SOURCE.to_vec()),
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
    .unwrap();
    let actual = inputs(SOURCE);
    for (index, (got, witness)) in actual.iter().zip(&expected).enumerate() {
        assert_eq!(
            snapshot.resolve(got).unwrap(),
            snapshot.resolve(witness).unwrap()
        );
        if (2..=4).contains(&index) {
            assert_eq!(
                snapshot.resolve(got).unwrap(),
                IdentityOutcome::Unavailable(IdentityReason::LocalDeclaration)
            );
        } else if let IdentityOutcome::Derived(d) = snapshot.resolve(got).unwrap() {
            assert_eq!(d.address().module, ["review", "probe"]);
            assert!(snapshot.is_current(&d).unwrap());
        } else {
            panic!("valid witness unavailable");
        }
    }
    let derived = |i| match snapshot.resolve(&actual[i]).unwrap() {
        IdentityOutcome::Derived(d) => d,
        _ => panic!(),
    };
    assert_eq!(derived(5).address(), derived(6).address());
    assert_ne!(
        derived(5).fingerprint().unwrap(),
        derived(6).fingerprint().unwrap()
    );
}
#[test]
fn r1_valid_leading_trivia_and_bom_are_rejected_by_root_coverage_check() {
    // Characterization of a defect, not the desired contract: these recovery-free
    // modules have declarations but fail only because the module starts after trivia.
    for (source, start) in [
        (b"\n\ndef f(): pass\n".as_slice(), 2),
        (b"  # lead\r\n\r\ndef f(): pass\r\n  ".as_slice(), 2),
        (b"\xef\xbb\xbfdef f(): pass\r\n".as_slice(), 3),
        (b" \t\r\n\r\n".as_slice(), 6),
        (b"\xef\xbb\xbf".as_slice(), 3),
    ] {
        let t = tree(source);
        let root = t.root_node();
        assert_eq!(root.kind(), "module");
        assert!(!root.has_error());
        assert_eq!(root.start_byte(), start);
        assert_eq!(root.end_byte(), source.len());
        unavailable(
            source,
            PythonIdentityLimits::default(),
            PythonIdentityReason::UnsupportedAst,
        );
    }
    for source in [b"".as_slice(), b"# only\r\n", b"# comment\n \t\n"] {
        assert!(inputs(source).is_empty());
    }
    for source in [
        b"def f(): pass".as_slice(),
        b"def f(): pass\n  \t\n",
        b"# lead\r\ndef f(): pass\r\n",
    ] {
        let t = tree(source);
        assert_eq!(t.root_node().start_byte(), 0);
        assert_eq!(t.root_node().end_byte(), source.len());
        let v = inputs(source);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].source_digest, content_digest(source));
    }
}
#[test]
fn anonymous_nodes_count_for_work_and_depth_at_exact_boundaries() {
    let t = tree(SOURCE);
    let mut cursor = t.walk();
    let (mut count, mut depth, mut max_depth, mut anonymous) = (0, 1, 1, 0);
    loop {
        count += 1;
        max_depth = max_depth.max(depth);
        if !cursor.node().is_named() {
            anonymous += 1;
        }
        if cursor.goto_first_child() {
            depth += 1;
            continue;
        }
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                break;
            }
            depth -= 1;
        }
        if depth == 1 && cursor.node() == t.root_node() {
            break;
        }
    }
    assert!(anonymous > 20);
    let default = PythonIdentityLimits::default();
    let exact = PythonIdentityLimits {
        visited_nodes: count,
        tree_depth: max_depth,
        ..default
    };
    assert_eq!(
        declaration_inputs(PATH, SOURCE, exact).unwrap(),
        PythonIdentityOutcome::Inputs(inputs(SOURCE))
    );
    unavailable(
        SOURCE,
        PythonIdentityLimits {
            visited_nodes: count - 1,
            ..default
        },
        PythonIdentityReason::WorkLimit,
    );
    unavailable(
        SOURCE,
        PythonIdentityLimits {
            tree_depth: max_depth - 1,
            ..default
        },
        PythonIdentityReason::DepthLimit,
    );
    // Post-tree syntax rejection precedes traversal limits, not parser allocation.
    unavailable(
        b"def broken(\n",
        PythonIdentityLimits {
            visited_nodes: 1,
            tree_depth: 1,
            ..default
        },
        PythonIdentityReason::SyntaxError,
    );
}
#[test]
fn malformed_missing_empty_and_hard_keyword_shapes_fail_whole_file() {
    let default = PythonIdentityLimits::default();
    for bad in [
        "def f(: pass\n",
        "@\nclass C: pass\n",
        "class C(broken=): pass\n",
        "class : pass\n",
    ] {
        let source = format!("def good(): pass\n{bad}");
        let t = tree(source.as_bytes());
        assert!(t.root_node().has_error());
        unavailable(
            source.as_bytes(),
            default,
            PythonIdentityReason::SyntaxError,
        );
    }
    for bad in [
        "def empty():\n",
        "class Empty:\n",
        "def comment():\n    # only\n",
        "def if(): pass\n",
        "class True: pass\n",
    ] {
        let source = format!("def good(): pass\n{bad}");
        unavailable(
            source.as_bytes(),
            default,
            PythonIdentityReason::UnsupportedAst,
        );
    }
    for name in ["match", "case", "type"] {
        let source = format!("def {name}(): pass\n");
        assert_eq!(inputs(source.as_bytes())[0].ancestry[0].name, name);
    }
    for name in [
        "and", "async", "await", "None", "return", "yield", "while", "False",
    ] {
        let source = format!("def good(): pass\ndef {name}(): pass\n");
        assert!(matches!(
            declaration_inputs(PATH, source.as_bytes(), default).unwrap(),
            PythonIdentityOutcome::Unavailable(
                PythonIdentityReason::SyntaxError | PythonIdentityReason::UnsupportedAst
            )
        ));
    }
    assert!(
        inputs(b"text = 'class Phantom: pass'\n# def ghost(): pass\nalias = lambda: 1\n")
            .is_empty()
    );
}
#[test]
fn independent_output_text_source_and_timeout_phases() {
    let default = PythonIdentityLimits::default();
    let v = inputs(SOURCE);
    let segments = v.iter().map(|i| i.ancestry.len()).sum::<usize>();
    let text = v
        .iter()
        .map(|i| {
            i.file_path.len()
                + i.source_digest.len()
                + i.ancestry.iter().map(|s| s.name.len()).sum::<usize>()
        })
        .sum::<usize>();
    let exact = PythonIdentityLimits {
        source_bytes: SOURCE.len(),
        declarations: v.len(),
        output_segments: segments,
        output_text_bytes: text,
        ..default
    };
    assert_eq!(
        declaration_inputs(PATH, SOURCE, exact).unwrap(),
        PythonIdentityOutcome::Inputs(v)
    );
    for limits in [
        PythonIdentityLimits {
            declarations: exact.declarations - 1,
            ..exact
        },
        PythonIdentityLimits {
            output_segments: segments - 1,
            ..exact
        },
        PythonIdentityLimits {
            output_text_bytes: text - 1,
            ..exact
        },
    ] {
        unavailable(SOURCE, limits, PythonIdentityReason::OutputLimit);
    }
    unavailable(
        &[0xff, 0xff],
        PythonIdentityLimits {
            source_bytes: 1,
            ..default
        },
        PythonIdentityReason::SourceLimit,
    );
    unavailable(&[0xff], default, PythonIdentityReason::InvalidUtf8);
    let large = "value = [1, 2, 3, 4]\n".repeat(20_000);
    assert!(matches!(
        declaration_inputs(
            PATH,
            large.as_bytes(),
            PythonIdentityLimits {
                parse_timeout_micros: 1,
                ..default
            }
        ),
        Err(CcError::Parse { .. })
    ));
    assert!(matches!(
        declaration_inputs(
            PATH,
            SOURCE,
            PythonIdentityLimits {
                parse_timeout_micros: 0,
                ..default
            }
        ),
        Err(CcError::InvalidParams(_))
    ));
    assert!(matches!(
        declaration_inputs(&"p".repeat(4097), SOURCE, default),
        Err(CcError::InvalidParams(_))
    ));
}

#[test]
fn actual_missing_anonymous_token_rejects_even_valid_sibling_and_child() {
    let source = b"def good(): pass\nclass C(A, meta:\n    def f(self): pass\n";
    let t = tree(source);
    assert!(t.root_node().has_error());
    assert!(t.root_node().to_sexp().contains("MISSING"));
    let mut cursor = t.walk();
    let mut missing = None;
    loop {
        let node = cursor.node();
        if node.is_missing() {
            missing = Some((node.kind().to_owned(), node.is_named()));
            break;
        }
        if cursor.goto_first_child() {
            continue;
        }
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                break;
            }
        }
        if cursor.node() == t.root_node() {
            break;
        }
    }
    assert_eq!(missing, Some((")".into(), false)));
    unavailable(
        source,
        PythonIdentityLimits::default(),
        PythonIdentityReason::SyntaxError,
    );
}
