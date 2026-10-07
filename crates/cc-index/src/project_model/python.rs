//! Explicit static source-root policy; no interpreter, import hooks, or user code.
use cc_model::{module_inputs::*, project_model::*};
use std::collections::BTreeMap;
pub(super) fn build(
    documents: &BTreeMap<String, ConfigDocument>,
    inputs: &BTreeMap<String, ConfigInput>,
) -> PythonProject {
    let mut p = PythonProject::default();
    p.roots
        .insert(String::new(), vec![String::new(), "src".into()]);
    p.provenance.insert(
        String::new(),
        PythonRootProvenance {
            version: PYTHON_ROOT_PROVENANCE_VERSION,
            state: PythonRootState::NoConfig,
            roots: [
                (String::new(), vec![PythonRootEvidence::InferredDefault]),
                ("src".into(), vec![PythonRootEvidence::InferredDefault]),
            ]
            .into(),
            ..Default::default()
        },
    );
    for (path, doc) in documents
        .iter()
        .filter(|(p, _)| p.ends_with("pyproject.toml"))
    {
        let root = parent(path);
        let mut roots = Vec::new();
        let mut errors = Vec::new();
        let input = inputs.get(path);
        let bound = input.is_some_and(|i| {
            i.error.is_none()
                && i.parsed.as_ref() == Some(doc)
                && i.digest
                    .as_ref()
                    .is_some_and(|d| d.len() == 64 && d.bytes().all(|b| b.is_ascii_hexdigit()))
        });
        let mut provenance = PythonRootProvenance {
            version: PYTHON_ROOT_PROVENANCE_VERSION,
            config: Some(PythonConfigBinding {
                path: path.clone(),
                digest: input.and_then(|i| i.digest.clone()),
                captured_document: bound,
            }),
            ..Default::default()
        };
        if path.rsplit('/').next() != Some("pyproject.toml") {
            provenance
                .limitations
                .push("nonstandard_pyproject_name".into());
        }
        if documents
            .keys()
            .filter(|p| p.ends_with("pyproject.toml") && parent(p) == root)
            .count()
            > 1
        {
            provenance
                .limitations
                .push("multiple_config_documents_same_scope".into());
        }
        if !bound {
            provenance.limitations.push("unbound_config_capture".into());
        }
        if let Some(error) = input.and_then(|i| i.error.as_ref()) {
            provenance.limitations.push(error.clone());
        }
        match doc {
            ConfigDocument::Toml(v) => {
                // Validate supported directive shapes separately: legacy roots and
                // diagnostics below retain their exact behavior.
                for (pointer, shape) in [
                    ("/tool", "object"),
                    ("/tool/setuptools", "object"),
                    ("/tool/poetry", "object"),
                    ("/tool/setuptools/package-dir", "object"),
                    ("/tool/setuptools/packages", "object"),
                    ("/tool/setuptools/packages/find", "object"),
                    ("/tool/poetry/packages", "array"),
                ] {
                    if v.pointer(pointer).is_some_and(|x| match shape {
                        "object" => !x.is_object(),
                        _ => !x.is_array(),
                    }) {
                        provenance
                            .limitations
                            .push(format!("unsupported_directive_shape:{pointer}"));
                    }
                }
                if let Some(d) = v
                    .pointer("/tool/setuptools/package-dir")
                    .and_then(|v| v.as_object())
                {
                    if d.keys().any(|k| !k.is_empty()) {
                        errors.push("named_package_dir_mapping_unsupported".into());
                    }
                    if let Some(value) = d.get("") {
                        if let Some(s) = value.as_str() {
                            roots.push((
                                s.to_owned(),
                                PythonRootEvidence::Explicit {
                                    directive: "/tool/setuptools/package-dir/".into(),
                                    value: s.into(),
                                },
                            ));
                        } else {
                            errors.push("invalid_python_package_dir".into());
                        }
                    }
                }
                if let Some(where_) = v.pointer("/tool/setuptools/packages/find/where") {
                    if let Some(a) = where_.as_array().filter(|a| a.len() <= 32) {
                        for (index, x) in a.iter().enumerate() {
                            if let Some(s) = x.as_str() {
                                roots.push((
                                    s.into(),
                                    PythonRootEvidence::Explicit {
                                        directive: format!(
                                            "/tool/setuptools/packages/find/where/{index}"
                                        ),
                                        value: s.into(),
                                    },
                                ));
                            } else {
                                errors.push("invalid_python_where".into());
                            }
                        }
                    } else {
                        if where_.as_array().is_some_and(|a| a.len() > 32) {
                            provenance
                                .limitations
                                .push("python_where_limit_over_32".into());
                        }
                        errors.push("invalid_python_where".into());
                    }
                }
                if let Some(items) = v
                    .pointer("/tool/poetry/packages")
                    .and_then(|v| v.as_array())
                {
                    for (index, item) in items.iter().take(32).enumerate() {
                        if !item.is_object()
                            || item
                                .get("include")
                                .is_none_or(|v| v.as_str().is_none_or(|s| s.is_empty()))
                        {
                            provenance
                                .limitations
                                .push(format!("unsupported_poetry_package_include:{index}"));
                        }
                        if let Some(s) = item.get("from").and_then(|v| v.as_str()) {
                            roots.push((
                                s.into(),
                                PythonRootEvidence::Explicit {
                                    directive: format!("/tool/poetry/packages/{index}/from"),
                                    value: s.into(),
                                },
                            ));
                        } else {
                            provenance
                                .limitations
                                .push(format!("unsupported_poetry_package_from:{index}"));
                        }
                    }
                    if items.len() > 32 {
                        errors.push("python_root_limit".into());
                    }
                }
            }
            ConfigDocument::Invalid(e) => errors.push(e.clone()),
            _ => errors.push("invalid_pyproject_document".into()),
        }
        let inferred = roots.is_empty();
        if inferred {
            roots = vec![
                (".".into(), PythonRootEvidence::InferredDefault),
                ("src".into(), PythonRootEvidence::InferredDefault),
            ];
        }
        let mut normalized = Vec::new();
        for (r, evidence) in roots {
            if let Some(r) = join_relative(root, &r) {
                provenance
                    .roots
                    .entry(r.clone())
                    .or_default()
                    .push(evidence);
                if !normalized.contains(&r) {
                    normalized.push(r);
                }
            } else {
                errors.push("python_root_outside_project".into());
            }
        }
        provenance.limitations.extend(errors.iter().cloned());
        provenance.state = if !matches!(doc, ConfigDocument::Toml(_)) {
            PythonRootState::InvalidConfig
        } else if !provenance.limitations.is_empty() {
            PythonRootState::PartialOrUnsupported
        } else if inferred {
            PythonRootState::InferredDefaults
        } else {
            PythonRootState::Explicit
        };
        p.provenance.insert(root.into(), provenance);
        p.roots.insert(root.into(), normalized);
        if !errors.is_empty() {
            p.diagnostics.insert(root.into(), errors);
        }
    }
    p
}
