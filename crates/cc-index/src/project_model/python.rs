//! Explicit static source-root policy; no interpreter, import hooks, or user code.
use cc_model::{module_inputs::*, project_model::*};
use std::collections::BTreeMap;
pub(super) fn build(documents: &BTreeMap<String, ConfigDocument>) -> PythonProject {
    let mut p = PythonProject::default();
    p.roots
        .insert(String::new(), vec![String::new(), "src".into()]);
    for (path, doc) in documents
        .iter()
        .filter(|(p, _)| p.ends_with("pyproject.toml"))
    {
        let root = parent(path);
        let mut roots = Vec::new();
        let mut errors = Vec::new();
        match doc {
            ConfigDocument::Toml(v) => {
                if let Some(d) = v
                    .pointer("/tool/setuptools/package-dir")
                    .and_then(|v| v.as_object())
                {
                    if d.keys().any(|k| !k.is_empty()) {
                        errors.push("named_package_dir_mapping_unsupported".into());
                    }
                    if let Some(value) = d.get("") {
                        if let Some(s) = value.as_str() {
                            roots.push(s.to_owned());
                        } else {
                            errors.push("invalid_python_package_dir".into());
                        }
                    }
                }
                if let Some(where_) = v.pointer("/tool/setuptools/packages/find/where") {
                    if let Some(a) = where_.as_array().filter(|a| a.len() <= 32) {
                        for x in a {
                            if let Some(s) = x.as_str() {
                                roots.push(s.into());
                            } else {
                                errors.push("invalid_python_where".into());
                            }
                        }
                    } else {
                        errors.push("invalid_python_where".into());
                    }
                }
                if let Some(items) = v
                    .pointer("/tool/poetry/packages")
                    .and_then(|v| v.as_array())
                {
                    for item in items.iter().take(32) {
                        if let Some(s) = item.get("from").and_then(|v| v.as_str()) {
                            roots.push(s.into());
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
        if roots.is_empty() {
            roots = vec![".".into(), "src".into()];
        }
        let mut normalized = Vec::new();
        for r in roots {
            if let Some(r) = join_relative(root, &r) {
                if !normalized.contains(&r) {
                    normalized.push(r);
                }
            } else {
                errors.push("python_root_outside_project".into());
            }
        }
        p.roots.insert(root.into(), normalized);
        if !errors.is_empty() {
            p.diagnostics.insert(root.into(), errors);
        }
    }
    p
}
