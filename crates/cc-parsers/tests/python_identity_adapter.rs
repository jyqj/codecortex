//! Self-authored AST witnesses; no public corpus/query fixtures.
use cc_model::{declaration_identity::*, source::ByteSpan};
use cc_parsers::python_identity::*;
use std::collections::BTreeMap;

const PATH: &str = "src/harbor/bells.py";
const CONFIG: &[u8] = b"[tool.setuptools.package-dir]\n\"\" = \"src\"\n";
const SOURCE: &[u8] = include_bytes!("fixtures/python_identity/witness.py");
fn parse(source: &[u8]) -> Vec<DeclarationInput> {
    match declaration_inputs(PATH, source, PythonIdentityLimits::default()).unwrap() {
        PythonIdentityOutcome::Inputs(inputs) => inputs,
        other => panic!("unexpected {other:?}"),
    }
}
fn snapshot(source: &[u8]) -> DeclarationSnapshot {
    DeclarationSnapshot::new(
        "self-authored-fixture".into(),
        BTreeMap::from([
            ("pyproject.toml".into(), CONFIG.to_vec()),
            (
                "src/harbor/__init__.py".into(),
                b"raise RuntimeError('not importable')\n".to_vec(),
            ),
            (PATH.into(), source.to_vec()),
        ]),
        vec![ConfiguredRoot {
            directory: "src".into(),
            config_path: "pyproject.toml".into(),
            config_digest: content_digest(CONFIG),
            directive: "tool.setuptools.package-dir.empty".into(),
        }],
    )
    .unwrap()
}
fn seg(
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
fn bound(s: &DeclarationSnapshot, input: &DeclarationInput) -> BoundDeclaration {
    match s.resolve(input).unwrap() {
        IdentityOutcome::Derived(value) => *value,
        other => panic!("unexpected {other:?}"),
    }
}
#[test]
fn original_ast_matches_pre_authored_byte_witnesses_and_model() {
    use ScopeKind::*;
    let tower = seg("Tower", Class, 0, 200, 19, 24);
    let room = seg("Room", Class, 30, 200, 46, 50);
    let ring = seg("ring", Function, 60, 200, 84, 88);
    let local = seg("local", Function, 108, 175, 112, 117);
    let hidden = seg("Hidden", Class, 137, 175, 143, 149);
    let expected = vec![
        vec![tower.clone()],
        vec![tower.clone(), room.clone()],
        vec![tower.clone(), room.clone(), ring.clone()],
        vec![tower.clone(), room.clone(), ring.clone(), local.clone()],
        vec![tower, room, ring, local, hidden],
        vec![seg("repeat", Function, 218, 244, 222, 228)],
        vec![seg("repeat", Function, 255, 281, 259, 265)],
    ]
    .into_iter()
    .map(|ancestry| DeclarationInput {
        file_path: PATH.into(),
        source_digest: content_digest(SOURCE),
        ancestry,
    })
    .collect::<Vec<_>>();
    let actual = parse(SOURCE);
    assert_eq!(actual, expected);
    let s = snapshot(SOURCE);
    for (i, (actual, witness)) in actual.iter().zip(&expected).enumerate() {
        assert_eq!(s.resolve(actual).unwrap(), s.resolve(witness).unwrap());
        if i == 3 || i == 4 {
            assert_eq!(
                s.resolve(actual).unwrap(),
                IdentityOutcome::Unavailable(IdentityReason::LocalDeclaration)
            );
        } else {
            let d = bound(&s, actual);
            assert_eq!(d.address().module, ["harbor", "bells"]);
            assert!(s.is_current(&d).unwrap());
        }
    }
    let stacked = b"@one\n@two(1)\ndef f():\n    pass\n";
    assert_eq!(
        parse(stacked)[0].ancestry,
        [seg("f", Function, 0, 30, 17, 18)]
    );
    assert_eq!(parse(stacked).len(), 1);
    bound(&snapshot(stacked), &parse(stacked)[0]);
    let a = bound(&s, &actual[5]);
    let b = bound(&s, &actual[6]);
    assert_eq!(a.address(), b.address());
    assert_ne!(a.fingerprint().unwrap(), b.fingerprint().unwrap());
}
#[test]
fn utf8_crlf_and_identifier_policy_preserve_original_bytes() {
    let source = "# 雪\r\n@deco\r\nasync def ping():\r\n    pass\r\n".as_bytes();
    let inputs = parse(source);
    assert_eq!(
        inputs[0].ancestry,
        [seg("ping", ScopeKind::Function, 7, 41, 24, 28)]
    );
    assert_eq!(inputs[0].source_digest, content_digest(source));
    let d = bound(&snapshot(source), &inputs[0]);
    assert_eq!(d.binding().source_digest, content_digest(source));
    let unicode = "class 雪:\n    pass\n".as_bytes();
    let inputs = parse(unicode);
    assert_eq!(inputs[0].ancestry[0].name, "雪");
    assert_eq!(
        inputs[0].ancestry[0].name_span,
        ByteSpan { start: 6, end: 9 }
    );
    assert_eq!(
        snapshot(unicode).resolve(&inputs[0]).unwrap(),
        IdentityOutcome::Unavailable(IdentityReason::UnsupportedIdentifier)
    );
    for name in ["match", "case", "type"] {
        let source = format!("def {name}():\n    pass\n");
        assert_eq!(parse(source.as_bytes())[0].ancestry[0].name, name);
    }
}
#[test]
fn malformed_recovery_never_publishes_partial_or_guessed_ancestry() {
    for source in [
        "def good():\n    pass\ndef bad(\n",
        "class :\n    def child():\n        pass\n",
        "def wrong.field():\n    pass\n",
        "def f[()():\n    pass\n",
        "class C(broken=):\n    pass\n",
        "@\ndef f():\n    pass\n",
    ] {
        assert_eq!(
            declaration_inputs(PATH, source.as_bytes(), PythonIdentityLimits::default()).unwrap(),
            PythonIdentityOutcome::Unavailable(PythonIdentityReason::SyntaxError),
            "{source}"
        );
    }
    for source in [
        b"def missing():\n".as_slice(),
        b"class Empty:\n",
        b"def if():\n    pass\n",
        b"class True:\n    pass\n",
        b"def comment_only():\n    # no statement\n",
    ] {
        assert_eq!(
            declaration_inputs(PATH, source, PythonIdentityLimits::default()).unwrap(),
            PythonIdentityOutcome::Unavailable(PythonIdentityReason::UnsupportedAst)
        );
    }
    assert_eq!(
        declaration_inputs(PATH, &[0xff], PythonIdentityLimits::default()).unwrap(),
        PythonIdentityOutcome::Unavailable(PythonIdentityReason::InvalidUtf8)
    );
}
#[test]
fn lexical_containers_do_not_create_scopes_or_alias_declarations() {
    let source = b"class C:\n    if enabled:\n        def f():\n            pass\n    try:\n        class D:\n            pass\n    except Exception:\n        pass\nfor item in items:\n    def f():\n        pass\nwith resource:\n    async def f():\n        pass\nAlias = C\ntext = 'def phantom(): pass'\n# class Phantom: pass\n";
    let inputs = parse(source);
    let names: Vec<Vec<&str>> = inputs
        .iter()
        .map(|i| i.ancestry.iter().map(|s| s.name.as_str()).collect())
        .collect();
    assert_eq!(
        names,
        [
            vec!["C"],
            vec!["C", "f"],
            vec!["C", "D"],
            vec!["f"],
            vec!["f"]
        ]
    );
    for input in &inputs {
        bound(&snapshot(source), input);
    }
    assert!(parse(b"alias = lambda: 1\n# def none(): pass\n").is_empty());
    let local_source = b"def outer():\n    class C:\n        def method(self):\n            pass\n";
    let local_inputs = parse(local_source);
    assert_eq!(
        local_inputs[2]
            .ancestry
            .iter()
            .map(|s| (s.name.as_str(), s.kind))
            .collect::<Vec<_>>(),
        [
            ("outer", ScopeKind::Function),
            ("C", ScopeKind::Class),
            ("method", ScopeKind::Function)
        ]
    );
    for input in &local_inputs[1..] {
        assert_eq!(
            snapshot(local_source).resolve(input).unwrap(),
            IdentityOutcome::Unavailable(IdentityReason::LocalDeclaration)
        );
    }
}
#[test]
fn all_independent_budgets_fail_closed_and_exact_output_budget_succeeds() {
    let default = PythonIdentityLimits::default();
    for (limits, reason) in [
        (
            PythonIdentityLimits {
                source_bytes: SOURCE.len() - 1,
                ..default
            },
            PythonIdentityReason::SourceLimit,
        ),
        (
            PythonIdentityLimits {
                visited_nodes: 1,
                ..default
            },
            PythonIdentityReason::WorkLimit,
        ),
        (
            PythonIdentityLimits {
                tree_depth: 1,
                ..default
            },
            PythonIdentityReason::DepthLimit,
        ),
        (
            PythonIdentityLimits {
                declarations: 6,
                ..default
            },
            PythonIdentityReason::OutputLimit,
        ),
        (
            PythonIdentityLimits {
                output_segments: 16,
                ..default
            },
            PythonIdentityReason::OutputLimit,
        ),
    ] {
        assert_eq!(
            declaration_inputs(PATH, SOURCE, limits).unwrap(),
            PythonIdentityOutcome::Unavailable(reason)
        );
    }
    let exact_text: usize = parse(SOURCE)
        .iter()
        .map(|i| {
            i.file_path.len()
                + i.source_digest.len()
                + i.ancestry.iter().map(|s| s.name.len()).sum::<usize>()
        })
        .sum();
    assert_eq!(
        declaration_inputs(
            PATH,
            SOURCE,
            PythonIdentityLimits {
                output_text_bytes: exact_text - 1,
                ..default
            }
        )
        .unwrap(),
        PythonIdentityOutcome::Unavailable(PythonIdentityReason::OutputLimit)
    );
    let limits = PythonIdentityLimits {
        output_text_bytes: exact_text,
        source_bytes: SOURCE.len(),
        declarations: 7,
        output_segments: 17,
        ..default
    };
    assert_eq!(
        declaration_inputs(PATH, SOURCE, limits).unwrap(),
        PythonIdentityOutcome::Inputs(parse(SOURCE))
    );
    assert!(declaration_inputs(
        PATH,
        SOURCE,
        PythonIdentityLimits {
            parse_timeout_micros: 0,
            ..default
        }
    )
    .is_err());
    assert!(declaration_inputs(
        PATH,
        SOURCE,
        PythonIdentityLimits {
            output_segments: 0,
            ..default
        }
    )
    .is_err());
}
#[test]
fn model_detects_staleness_and_forged_name_field() {
    let inputs = parse(SOURCE);
    let mut forged = inputs[0].clone();
    forged.ancestry[0].name_span = ByteSpan { start: 0, end: 5 };
    assert_eq!(
        snapshot(SOURCE).resolve(&forged).unwrap(),
        IdentityOutcome::Unavailable(IdentityReason::InvalidDeclaration)
    );
    let mut changed = SOURCE.to_vec();
    changed.push(b'\n');
    assert_eq!(
        snapshot(&changed).resolve(&inputs[0]).unwrap(),
        IdentityOutcome::Unavailable(IdentityReason::StaleSource)
    );
}
