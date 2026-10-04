//! Self-authored fixtures exercise the real shared capture/parser, not prototype data.
use super::{config_cache::Loader, python::build};
use cc_model::{module_inputs::*, project_model::*};
use std::collections::{BTreeMap, BTreeSet};

fn capture(path: &str, text: &str) -> (PythonProject, BTreeMap<String, ConfigInput>) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(parent(path))).unwrap();
    std::fs::write(dir.path().join(path), text).unwrap();
    let old = BTreeMap::new();
    let mut loader = Loader::new(dir.path(), &old);
    let input = loader.load(path).unwrap();
    let doc = input
        .parsed
        .clone()
        .unwrap_or_else(|| ConfigDocument::Invalid(input.error.clone().unwrap()));
    let documents = [(path.into(), doc)].into();
    let project = build(&documents, &loader.inputs);
    assert_eq!(loader.reads, 1);
    assert_eq!(loader.hits, 0);
    (project, loader.inputs)
}
fn evidence(project: &PythonProject, scope: &str) -> PythonRootProvenance {
    project.provenance[scope].clone()
}
fn explicit(project: &PythonProject, scope: &str, root: &str) -> Vec<(String, String)> {
    evidence(project, scope).roots[root]
        .iter()
        .map(|e| match e {
            PythonRootEvidence::Explicit { directive, value } => (directive.clone(), value.clone()),
            _ => panic!("inferred evidence"),
        })
        .collect()
}

#[test]
fn python_provenance_defaults_are_never_explicit() {
    let no_config = build(&BTreeMap::new(), &BTreeMap::new());
    assert_eq!(no_config.roots[""], ["", "src"]);
    assert_eq!(evidence(&no_config, "").state, PythonRootState::NoConfig);
    assert!(evidence(&no_config, "").config.is_none());
    let (default, _) = capture("pyproject.toml", "[project]\nname='sample'\n");
    assert_eq!(default.roots, no_config.roots);
    assert_eq!(
        evidence(&default, "").state,
        PythonRootState::InferredDefaults
    );
    assert!(evidence(&default, "").config.unwrap().captured_document);
    for root in evidence(&default, "").roots.values() {
        assert_eq!(root, &[PythonRootEvidence::InferredDefault]);
    }
    let old: PythonProject =
        serde_json::from_str(r#"{"roots":{"": ["", "src"]},"diagnostics":{}}"#).unwrap();
    assert!(old.provenance.is_empty());
    assert_eq!(
        PythonRootProvenance::default().state,
        PythonRootState::Unknown
    );
}

#[test]
fn python_provenance_root_and_src_bind_exact_original_bytes_and_directive() {
    for value in [".", "./src"] {
        let text = format!("# original bytes\n[tool.setuptools.package-dir]\n\"\" = '{value}'\n");
        let (p, inputs) = capture("pyproject.toml", &text);
        let root = if value == "." { "" } else { "src" };
        assert_eq!(p.roots[""], [root]);
        assert_eq!(evidence(&p, "").state, PythonRootState::Explicit);
        assert_eq!(
            explicit(&p, "", root),
            [("/tool/setuptools/package-dir/".into(), value.into())]
        );
        let binding = evidence(&p, "").config.unwrap();
        assert_eq!(binding.path, "pyproject.toml");
        assert_eq!(
            binding.digest,
            Some(blake3::hash(text.as_bytes()).to_hex().to_string())
        );
        assert_eq!(binding.digest, inputs["pyproject.toml"].digest);
        assert!(binding.captured_document);
    }
}

#[test]
fn python_provenance_nested_poetry_duplicate_and_multiple_roots() {
    let text = r#"
[tool.setuptools.package-dir]
"" = "./src"
[tool.setuptools.packages.find]
where = ["src", "src/./", "other"]
[tool.poetry]
packages = [{ include = "p", from = "src" }, { include = "q", from = "other" }]
"#;
    let (p, _) = capture("nested/pyproject.toml", text);
    assert_eq!(p.roots["nested"], ["nested/src", "nested/other"]);
    assert_eq!(evidence(&p, "nested").state, PythonRootState::Explicit);
    assert_eq!(
        explicit(&p, "nested", "nested/src"),
        [
            ("/tool/setuptools/package-dir/".into(), "./src".into()),
            (
                "/tool/setuptools/packages/find/where/0".into(),
                "src".into()
            ),
            (
                "/tool/setuptools/packages/find/where/1".into(),
                "src/./".into()
            ),
            ("/tool/poetry/packages/0/from".into(), "src".into()),
        ]
    );
    assert_eq!(explicit(&p, "nested", "nested/other").len(), 2);
    assert_eq!(
        evidence(&p, "nested").config.unwrap().path,
        "nested/pyproject.toml"
    );
    // The nested config leaves the repository fallback in place.
    assert_eq!(evidence(&p, "").state, PythonRootState::NoConfig);
}

#[test]
fn python_provenance_invalid_partial_named_outside_and_unsupported_are_tainted() {
    let (invalid, _) = capture("pyproject.toml", "[not valid");
    assert_eq!(invalid.roots[""], ["", "src"]);
    assert_eq!(evidence(&invalid, "").state, PythonRootState::InvalidConfig);
    assert!(!evidence(&invalid, "").config.unwrap().captured_document);
    for text in [
        "[tool.setuptools.package-dir]\n\"\"='src'\np='elsewhere'\n",
        "[tool.setuptools.packages.find]\nwhere=['src', 1]\n",
        "[tool.setuptools.packages.find]\nwhere=['src', '../outside']\n",
        "[tool.poetry]\npackages=[{from='src'}, {include='implicit'}]\n",
        "[tool.setuptools.package-dir]\n\"\"='src'\n[tool.poetry]\npackages='bad'\n",
    ] {
        let (p, _) = capture("pyproject.toml", text);
        assert_eq!(p.roots[""], ["src"]);
        assert_eq!(
            evidence(&p, "").state,
            PythonRootState::PartialOrUnsupported
        );
        assert!(!evidence(&p, "").limitations.is_empty());
        assert!(!explicit(&p, "", "src").is_empty());
    }
    let (outside, _) = capture(
        "pyproject.toml",
        "[tool.setuptools.package-dir]\n\"\"='../outside'\n",
    );
    assert!(outside.roots[""].is_empty()); // legacy does not fall back after rejected join
    assert_eq!(outside.diagnostics[""], ["python_root_outside_project"]);
    assert_eq!(
        evidence(&outside, "").state,
        PythonRootState::PartialOrUnsupported
    );
}

#[test]
fn python_provenance_over_32_preserves_legacy_limits_and_marks_partial() {
    let where_items = vec!["'src'"; 33].join(",");
    let (where_p, _) = capture(
        "pyproject.toml",
        &format!("[tool.setuptools.packages.find]\nwhere=[{where_items}]\n"),
    );
    assert_eq!(where_p.roots[""], ["", "src"]);
    assert_eq!(where_p.diagnostics[""], ["invalid_python_where"]);
    assert!(evidence(&where_p, "")
        .limitations
        .contains(&"python_where_limit_over_32".into()));
    let poetry_items = vec!["{from='src'}"; 33].join(",");
    let (poetry_p, _) = capture(
        "pyproject.toml",
        &format!("[tool.poetry]\npackages=[{poetry_items}]\n"),
    );
    assert_eq!(poetry_p.roots[""], ["src"]);
    assert_eq!(poetry_p.diagnostics[""], ["python_root_limit"]);
    assert_eq!(explicit(&poetry_p, "", "src").len(), 32);
    assert_eq!(
        evidence(&poetry_p, "").state,
        PythonRootState::PartialOrUnsupported
    );
}

#[test]
fn python_provenance_missing_stale_or_error_capture_cannot_assert_bound_roots() {
    let (p, inputs) = capture(
        "pyproject.toml",
        "[tool.setuptools.package-dir]\n\"\"='src'\n",
    );
    let documents = [(
        "pyproject.toml".into(),
        inputs["pyproject.toml"].parsed.clone().unwrap(),
    )]
    .into();
    let mut error_inputs = inputs.clone();
    error_inputs.get_mut("pyproject.toml").unwrap().error = Some("incomplete_capture".into());
    let mut stale_inputs = inputs.clone();
    stale_inputs.get_mut("pyproject.toml").unwrap().parsed = None;
    for bad in [&BTreeMap::new(), &error_inputs, &stale_inputs] {
        let rebuilt = build(&documents, bad);
        assert_eq!(rebuilt.roots, p.roots);
        assert_eq!(rebuilt.diagnostics, p.diagnostics);
        let prov = evidence(&rebuilt, "");
        assert_eq!(prov.state, PythonRootState::PartialOrUnsupported);
        assert!(!prov.config.unwrap().captured_document);
    }
}

#[test]
fn python_provenance_rename_and_comment_change_invalidate_binding_not_roots() {
    let text = "[tool.setuptools.package-dir]\n\"\"='src'\n";
    let (p, inputs) = capture("a/pyproject.toml", text);
    let (renamed, _) = capture("b/pyproject.toml", text);
    let (changed, changed_inputs) =
        capture("a/pyproject.toml", &format!("{text}# comment changed\n"));
    assert_eq!(p.roots, changed.roots);
    assert_ne!(p.provenance, changed.provenance);
    assert_eq!(
        evidence(&p, "a").config.as_ref().unwrap().digest,
        evidence(&renamed, "b").config.as_ref().unwrap().digest
    );
    assert_ne!(
        evidence(&p, "a").config.as_ref().unwrap().path,
        evidence(&renamed, "b").config.as_ref().unwrap().path
    );
    let snapshot = ProjectInputs {
        configs: inputs,
        ..Default::default()
    };
    let changed_snapshot = ProjectInputs {
        configs: changed_inputs,
        ..Default::default()
    };
    assert_ne!(
        snapshot.digest().unwrap(),
        changed_snapshot.digest().unwrap()
    );
}

#[test]
fn python_provenance_real_discovery_reuses_parse_and_verifies_config_change() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("pyproject.toml"),
        "[tool.setuptools.package-dir]\n\"\"='src'\n",
    )
    .unwrap();
    std::fs::create_dir(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src/main.py"), "pass\n").unwrap();
    let files = BTreeSet::from(["src/main.py".into()]);
    let first = super::discover(
        dir.path(),
        files.clone(),
        None,
        None,
        &ProjectInputs::default(),
    )
    .unwrap();
    let second = super::discover(dir.path(), files, None, None, first.inputs()).unwrap();
    assert_eq!(first.report().config_reads, 1);
    assert_eq!(second.report().config_parse_cache_hits, 1);
    assert_eq!(first.model().python(), second.model().python());
    second.verify(dir.path()).unwrap();
    std::fs::write(dir.path().join("pyproject.toml"), "# changed\n").unwrap();
    assert!(second.verify(dir.path()).is_err());
}

#[cfg(unix)]
#[test]
fn python_provenance_native_symlink_and_hardlink_limits() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let text = "[tool.setuptools.package-dir]\n\"\"='src'\n";
    std::fs::write(dir.path().join("original.toml"), text).unwrap();
    symlink("original.toml", dir.path().join("pyproject.toml")).unwrap();
    let old = BTreeMap::new();
    let mut loader = Loader::new(dir.path(), &old);
    let denied = loader.load("pyproject.toml").unwrap();
    assert_eq!(denied.error.as_deref(), Some("symlink_config_unsupported"));
    assert!(denied.digest.is_none());
    let p = build(
        &[(
            "pyproject.toml".into(),
            ConfigDocument::Invalid(denied.error.unwrap()),
        )]
        .into(),
        &loader.inputs,
    );
    assert_eq!(evidence(&p, "").state, PythonRootState::InvalidConfig);
    std::fs::create_dir(dir.path().join("real")).unwrap();
    std::fs::write(dir.path().join("real/pyproject.toml"), text).unwrap();
    symlink("real", dir.path().join("alias")).unwrap();
    assert_eq!(
        loader
            .load("alias/pyproject.toml")
            .unwrap()
            .error
            .as_deref(),
        Some("symlink_config_unsupported")
    );
    assert_eq!(
        loader
            .load("real/../real/pyproject.toml")
            .unwrap()
            .error
            .as_deref(),
        Some("unsupported_config_path")
    );
    std::fs::create_dir(dir.path().join("other")).unwrap();
    std::fs::hard_link(
        dir.path().join("real/pyproject.toml"),
        dir.path().join("other/pyproject.toml"),
    )
    .unwrap();
    let real = loader.load("real/pyproject.toml").unwrap();
    let other = loader.load("other/pyproject.toml").unwrap();
    assert_eq!(real.digest, other.digest); // no inode/alias uniqueness proof
    assert!(real.parsed.is_some() && other.parsed.is_some());
    let root_alias = tempfile::tempdir().unwrap();
    symlink(dir.path(), root_alias.path().join("root")).unwrap();
    let alias_path = root_alias.path().join("root");
    let mut alias_loader = Loader::new(&alias_path, &old);
    assert_eq!(
        alias_loader.load("real/pyproject.toml").unwrap().digest,
        real.digest
    );
    // Source-root directories are not inspected by the config adapter.
    symlink("../other", dir.path().join("real/src")).unwrap();
    let doc = real.parsed.clone().unwrap();
    let p = build(
        &[("real/pyproject.toml".into(), doc)].into(),
        &loader.inputs,
    );
    assert!(evidence(&p, "real").config.unwrap().captured_document);
}

#[test]
fn python_provenance_duplicate_toml_keys_are_invalid_not_explicit() {
    let (p, _) = capture(
        "pyproject.toml",
        "[tool.setuptools.package-dir]\n\"\"='src'\n\"\"='other'\n",
    );
    assert_eq!(evidence(&p, "").state, PythonRootState::InvalidConfig);
    assert_eq!(p.roots[""], ["", "src"]);
}

#[test]
fn python_provenance_capture_size_utf8_and_missing_limits() {
    let dir = tempfile::tempdir().unwrap();
    let old = BTreeMap::new();
    let mut loader = Loader::new(dir.path(), &old);
    let missing = loader.load("pyproject.toml").unwrap();
    assert_eq!(missing.error.as_deref(), Some("missing_config"));
    assert!(missing.digest.is_none());
    std::fs::write(
        dir.path().join("pyproject.toml"),
        vec![b' '; super::config_cache::MAX_CONFIG_BYTES + 1],
    )
    .unwrap();
    let mut loader = Loader::new(dir.path(), &old);
    let oversized = loader.load("pyproject.toml").unwrap();
    assert_eq!(oversized.error.as_deref(), Some("config_size_limit"));
    assert!(oversized.digest.is_none());
    std::fs::write(dir.path().join("pyproject.toml"), [0xff]).unwrap();
    let mut loader = Loader::new(dir.path(), &old);
    let invalid = loader.load("pyproject.toml").unwrap();
    assert!(invalid.parsed.is_none());
    assert!(invalid.error.unwrap().contains("config_not_utf8"));
    assert_eq!(
        invalid.digest,
        Some(blake3::hash(&[0xff]).to_hex().to_string())
    );
}

#[test]
fn python_provenance_legacy_resolution_ignores_additive_evidence() {
    use crate::module_resolution::resolve;
    let (p, _) = capture(
        "pyproject.toml",
        "[tool.setuptools.packages.find]\nwhere=['src', 'other']\n",
    );
    let files = FileCatalog::new(
        [
            "use.py",
            "src/pkg/__init__.py",
            "src/pkg/f.py",
            "other/pkg/__init__.py",
            "other/pkg/f.py",
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
    let mut legacy = p.clone();
    legacy.provenance.clear();
    for (file, import) in [
        ("use.py", "pkg.f"),
        ("src/pkg/f.py", ".missing"),
        ("use.py", "external"),
    ] {
        assert_eq!(
            resolve(&model(p.clone()), file, import),
            resolve(&model(legacy.clone()), file, import)
        );
    }
    assert_eq!(
        resolve(&model(p), "use.py", "pkg.f")
            .resolved_path
            .as_deref(),
        Some("src/pkg/f.py")
    );
}
