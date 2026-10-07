//! Independent review of a65c655 against the exact 37dd042 Python adapter.
//! The legacy support file is verbatim `git show 37dd042:.../python.rs`.
#[path = "../src/project_model/python.rs"]
mod current;
#[path = "provenance_review_support/legacy.rs"]
mod legacy;
use cc_model::{module_inputs::*, project_model::*};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

fn inputs(docs: &BTreeMap<String, ConfigDocument>) -> BTreeMap<String, ConfigInput> {
    docs.iter()
        .map(|(p, d)| {
            (
                p.clone(),
                ConfigInput {
                    digest: Some("a".repeat(64)),
                    parsed: Some(d.clone()),
                    error: None,
                },
            )
        })
        .collect()
}
fn compare(
    docs: &BTreeMap<String, ConfigDocument>,
    captured: &BTreeMap<String, ConfigInput>,
) -> PythonProject {
    let old = legacy::build(docs);
    let new = current::build(docs, captured);
    assert_eq!(old.roots, new.roots, "{docs:?}");
    assert_eq!(old.diagnostics, new.diagnostics, "{docs:?}");
    let files = FileCatalog::new(
        [
            "use.py",
            "pkg.py",
            "src/pkg/__init__.py",
            "src/pkg/f.py",
            "other/pkg/f.py",
            "nested/use.py",
            "nested/src/pkg/f.py",
            "nested/src/pkg/__init__.py",
            "nested/other/pkg.py",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect(),
    )
    .unwrap();
    let model = |p| {
        ProjectModel::new(
            files.clone(),
            Default::default(),
            Default::default(),
            Default::default(),
        )
        .with_module_inputs(Default::default(), Default::default(), p)
    };
    let before = model(old);
    let after = model(new.clone());
    for file in [
        "use.py",
        "src/pkg/f.py",
        "nested/use.py",
        "nested/src/pkg/f.py",
    ] {
        for spec in [
            "pkg",
            "pkg.f",
            "pkg.absent",
            "external",
            ".f",
            "..pkg",
            "...f",
            ".",
            "",
            "a/b",
        ] {
            assert_eq!(
                cc_index::module_resolution::resolve(&before, file, spec),
                cc_index::module_resolution::resolve(&after, file, spec),
                "{file} {spec} {docs:?}"
            );
        }
    }
    new
}

#[test]
fn independent_review_generated_shape_and_root_matrix() {
    let shapes = [
        Value::Null,
        json!(false),
        json!(7),
        json!("src"),
        json!([]),
        json!({}),
        json!(["src"]),
        json!({"bad":"src"}),
    ];
    let pointers = [
        "/tool",
        "/tool/setuptools",
        "/tool/poetry",
        "/tool/setuptools/package-dir",
        "/tool/setuptools/packages",
        "/tool/setuptools/packages/find",
        "/tool/setuptools/packages/find/where",
        "/tool/poetry/packages",
    ];
    let mut cases = vec![
        json!({}),
        json!({"tool":{"setuptools":{"package-dir":{"":"src"}},"poetry":{"packages":[{"include":"pkg","from":"other"}]}}}),
    ];
    for pointer in pointers {
        for shape in &shapes {
            let mut node = shape.clone();
            for part in pointer
                .split('/')
                .skip(1)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
            {
                node = json!({part: node});
            }
            cases.push(node);
        }
    }
    for root in [
        "",
        ".",
        "./src",
        "src/./",
        "src/../other",
        "../outside",
        "/abs",
        "C:\\src",
        "a//b",
        "a\\b",
        "é",
        "a/../../b",
    ] {
        cases.push(json!({"tool":{"setuptools":{"package-dir":{"":root}}}}));
        cases.push(
            json!({"tool":{"setuptools":{"packages":{"find":{"where":[root,"src",1,"src/./"]}}}}}),
        );
    }
    for size in [0, 1, 31, 32, 33, 40] {
        cases.push(json!({"tool":{"setuptools":{"packages":{"find":{"where":vec!["src";size]}}}}}));
        cases.push(
            json!({"tool":{"poetry":{"packages":vec![json!({"include":"p","from":"src"});size]}}}),
        );
    }
    for item in [
        json!(null),
        json!(1),
        json!("pkg"),
        json!({}),
        json!({"include":"pkg"}),
        json!({"from":"src"}),
        json!({"include":1,"from":"src"}),
        json!({"include":"","from":"src"}),
        json!({"include":"pkg","from":1}),
    ] {
        cases.push(json!({"tool":{"poetry":{"packages":[{"include":"ok","from":"other"},item]}}}));
    }
    assert_eq!(cases.len(), 111);
    for doc in cases {
        for path in ["pyproject.toml", "nested/pyproject.toml"] {
            let docs = [(path.into(), ConfigDocument::Toml(doc.clone()))].into();
            let new = compare(&docs, &inputs(&docs));
            let scope = parent(path);
            assert_eq!(
                new.roots[scope].iter().collect::<BTreeSet<_>>(),
                new.provenance[scope].roots.keys().collect()
            );
            if new.provenance[scope].state == PythonRootState::Explicit {
                assert!(new.provenance[scope].limitations.is_empty());
                assert!(
                    new.provenance[scope]
                        .config
                        .as_ref()
                        .unwrap()
                        .captured_document
                );
            }
        }
    }
    compare(&BTreeMap::new(), &BTreeMap::new());
    for doc in [
        ConfigDocument::Invalid("invalid_toml".into()),
        ConfigDocument::TypeScript(Default::default()),
    ] {
        let docs = [("pyproject.toml".into(), doc)].into();
        assert_eq!(
            compare(&docs, &inputs(&docs)).provenance[""].state,
            PythonRootState::InvalidConfig
        );
    }
}

#[test]
fn independent_review_supporting_evidence_scopes_and_stale_inputs() {
    let doc = ConfigDocument::Toml(
        json!({"tool":{"setuptools":{"package-dir":{"":"./src"},"packages":{"find":{"where":["src","src/./","other"]}}},"poetry":{"packages":[{"include":"p","from":"src"}]}}}),
    );
    let docs = [
        ("pyproject.toml".into(), doc.clone()),
        ("nested/pyproject.toml".into(), doc.clone()),
    ]
    .into();
    let good = inputs(&docs);
    let p = compare(&docs, &good);
    for (scope, root) in [("", "src"), ("nested", "nested/src")] {
        assert_eq!(p.provenance[scope].roots[root].len(), 4);
        for ev in &p.provenance[scope].roots[root] {
            let PythonRootEvidence::Explicit { directive, value } = ev else {
                panic!()
            };
            let ConfigDocument::Toml(v) = &doc else {
                panic!()
            };
            assert_eq!(v.pointer(directive).unwrap().as_str(), Some(value.as_str()));
        }
    }
    for variant in 0..6 {
        let mut bad = good.clone();
        let i = bad.get_mut("pyproject.toml").unwrap();
        match variant {
            0 => i.digest = None,
            1 => i.digest = Some("x".repeat(64)),
            2 => i.digest = Some("a".repeat(63)),
            3 => i.parsed = None,
            4 => i.parsed = Some(ConfigDocument::Toml(json!({}))),
            _ => i.error = Some("read_failed".into()),
        }
        let p = compare(&docs, &bad);
        assert_eq!(
            p.provenance[""].state,
            PythonRootState::PartialOrUnsupported
        );
        assert!(!p.provenance[""].config.as_ref().unwrap().captured_document);
    }
    let docs = [
        ("nested/apyproject.toml".into(), doc.clone()),
        (
            "nested/pyproject.toml".into(),
            ConfigDocument::Toml(json!({})),
        ),
    ]
    .into();
    let p = compare(&docs, &inputs(&docs));
    assert_eq!(p.roots["nested"], ["nested", "nested/src"]);
    assert_eq!(
        p.provenance["nested"].state,
        PythonRootState::PartialOrUnsupported
    );
    assert!(p.provenance["nested"]
        .limitations
        .contains(&"multiple_config_documents_same_scope".into()));
    let docs = [("nested/apyproject.toml".into(), doc)].into();
    assert!(compare(&docs, &inputs(&docs)).provenance["nested"]
        .limitations
        .contains(&"nonstandard_pyproject_name".into()));
}

#[test]
fn independent_review_original_loader_bytes_refresh_and_wire_boundaries() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("nested/src")).unwrap();
    let a = "# first\n[tool.setuptools.packages.find]\nwhere = ['src','./src','src/./']\n";
    let b = "[tool.poetry]\npackages=[{include='p',from='src'}]\n";
    std::fs::write(dir.path().join("pyproject.toml"), a).unwrap();
    std::fs::write(dir.path().join("nested/pyproject.toml"), b).unwrap();
    let files = BTreeSet::from(["nested/src/main.py".into()]);
    let first = cc_index::project_model::discover(
        dir.path(),
        files.clone(),
        None,
        None,
        &ProjectInputs::default(),
    )
    .unwrap();
    for (path, text) in [("pyproject.toml", a), ("nested/pyproject.toml", b)] {
        let binding = first.model().python().provenance[parent(path)]
            .config
            .as_ref()
            .unwrap();
        assert_eq!(binding.path, path);
        assert_eq!(
            binding.digest,
            Some(blake3::hash(text.as_bytes()).to_hex().to_string())
        );
        assert_eq!(binding.digest, first.inputs().configs[path].digest);
    }
    let docs = first
        .inputs()
        .configs
        .iter()
        .filter_map(|(p, i)| i.parsed.clone().map(|d| (p.clone(), d)))
        .collect();
    assert_eq!(
        compare(&docs, &first.inputs().configs),
        *first.model().python()
    );
    let second =
        cc_index::project_model::discover(dir.path(), files.clone(), None, None, first.inputs())
            .unwrap();
    assert!(second.report().config_parse_cache_hits >= 2);
    assert_eq!(
        first.inputs().digest().unwrap(),
        second.inputs().digest().unwrap()
    );
    assert_eq!(PROJECT_MODEL_VERSION, 3);
    assert!(!first.inputs().payload().unwrap().contains("provenance"));
    let mut stripped = first.model().python().clone();
    stripped.provenance.clear();
    assert_ne!(
        serde_json::to_vec(&stripped).unwrap(),
        serde_json::to_vec(first.model().python()).unwrap()
    );
    std::fs::write(
        dir.path().join("pyproject.toml"),
        format!("{a}# bytes only\n"),
    )
    .unwrap();
    assert!(second.verify(dir.path()).is_err());
    let third =
        cc_index::project_model::discover(dir.path(), files, None, None, first.inputs()).unwrap();
    assert_eq!(first.model().python().roots, third.model().python().roots);
    assert_ne!(
        first.inputs().digest().unwrap(),
        third.inputs().digest().unwrap()
    );
    assert_ne!(
        first.model().python().provenance,
        third.model().python().provenance
    );
    for wire in [
        r#"{"roots":{"": ["src"]},"diagnostics":{}}"#,
        r#"{"roots":{"": ["src"]},"diagnostics":{},"provenance":{"":{}}}"#,
    ] {
        let p: PythonProject = serde_json::from_str(wire).unwrap();
        assert!(p.provenance.is_empty() || p.provenance[""].state == PythonRootState::Unknown);
    }
}

#[test]
fn independent_review_real_toml_matrix() {
    let texts = [
        "",
        "[project]\nname='p'\n",
        "[tool.setuptools.package-dir]\n\"\"='.'\n",
        "[tool.setuptools.package-dir]\n\"\"=''\n",
        "[tool.setuptools.package-dir]\n\"\"='./src'\n",
        "[tool.setuptools.package-dir]\n\"\"='../outside'\n",
        "[tool.setuptools.package-dir]\n\"\"='src'\np='other'\n",
        "[tool.setuptools.package-dir]\n\"\"=7\n",
        "[tool.setuptools.packages.find]\nwhere=['src','src/./','other']\n",
        "[tool.setuptools.packages.find]\nwhere=['src',7]\n",
        "[tool.setuptools.packages.find]\nwhere='src'\n",
        "[tool.setuptools]\npackage-dir='src'\n",
        "[tool]\nsetuptools=1\n",
        "tool='bad'\n",
        "[tool.poetry]\npackages=[{include='p',from='src'},{include='q',from='other'}]\n",
        "[tool.poetry]\npackages=[{include='p'},{from='src'}]\n",
        "[tool.poetry]\npackages=[{include='',from='src'},7]\n",
        "[tool.poetry]\npackages='bad'\n",
        "[tool.setuptools]\npackages=['p']\n",
        "[broken",
        "[tool.setuptools.package-dir]\n\"\"='src'\n\"\"='other'\n",
    ];
    for text in texts {
        for scope in ["", "nested"] {
            let dir = tempfile::tempdir().unwrap();
            let path = if scope.is_empty() {
                "pyproject.toml".to_owned()
            } else {
                format!("{scope}/pyproject.toml")
            };
            std::fs::create_dir_all(dir.path().join(scope)).unwrap();
            std::fs::write(dir.path().join(&path), text).unwrap();
            let source = if scope.is_empty() {
                "use.py".into()
            } else {
                format!("{scope}/use.py")
            };
            let captured = cc_index::project_model::discover(
                dir.path(),
                BTreeSet::from([source]),
                None,
                None,
                &ProjectInputs::default(),
            )
            .unwrap();
            let docs = captured
                .inputs()
                .configs
                .iter()
                .map(|(p, i)| {
                    (
                        p.clone(),
                        i.parsed
                            .clone()
                            .unwrap_or_else(|| ConfigDocument::Invalid(i.error.clone().unwrap())),
                    )
                })
                .collect();
            let p = compare(&docs, &captured.inputs().configs);
            assert_eq!(&p, captured.model().python(), "{text}");
            assert_eq!(
                p.provenance[scope].config.as_ref().unwrap().digest,
                Some(blake3::hash(text.as_bytes()).to_hex().to_string())
            );
        }
    }
}
