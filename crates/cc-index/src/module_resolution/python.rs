//! File-backed Python packages and relative imports from an immutable catalog.
use cc_model::{project_model::*, ImportRecord};
fn under(file: &str, root: &str) -> bool {
    root.is_empty() || file.starts_with(&format!("{root}/"))
}
fn join(root: &str, tail: &str) -> String {
    if root.is_empty() {
        tail.into()
    } else if tail.is_empty() {
        root.into()
    } else {
        format!("{root}/{tail}")
    }
}
fn valid_module(s: &str) -> bool {
    s.split('.')
        .all(|p| !p.is_empty() && p.chars().all(|c| c.is_alphanumeric() || c == '_'))
}
fn directory(files: &FileCatalog, path: &str) -> bool {
    let prefix = format!("{path}/");
    files
        .files()
        .range(prefix.clone()..)
        .next()
        .is_some_and(|p| p.starts_with(&prefix))
}
enum Located {
    File(String),
    Namespace(Vec<String>),
    Missing(&'static str, bool),
}
fn locate(
    files: &FileCatalog,
    mut paths: Vec<String>,
    parts: &[&str],
    r: &mut ModuleResolution,
) -> Located {
    if parts.is_empty() {
        for path in &paths {
            let init = join(path, "__init__.py");
            r.probes.push(init.clone());
            if files.contains(&init) {
                return Located::File(init);
            }
        }
        return Located::Namespace(paths);
    }
    let mut closed = false;
    for (index, part) in parts.iter().enumerate() {
        let last = index + 1 == parts.len();
        let mut namespaces = Vec::new();
        let mut regular = None;
        for root in paths {
            let base = join(&root, part);
            let init = join(&base, "__init__.py");
            let module = format!("{base}.py");
            r.probes.extend([init.clone(), module.clone()]);
            if files.contains(&init) {
                regular = Some((base, init, true));
                break;
            }
            if files.contains(&module) {
                regular = Some((base, module, false));
                break;
            }
            if directory(files, &base) {
                namespaces.push(base);
            }
        }
        if let Some((base, file, is_package)) = regular {
            if last {
                return Located::File(file);
            }
            if !is_package {
                return Located::Missing("python_parent_is_not_package", true);
            }
            paths = vec![base];
            closed = true;
        } else {
            if namespaces.is_empty() {
                return Located::Missing("python_module_not_in_admitted_roots", closed);
            }
            if last {
                return Located::Namespace(namespaces);
            }
            paths = namespaces;
        }
    }
    Located::Missing("python_module_not_in_admitted_roots", closed)
}
pub(super) fn resolve(model: &ProjectModel, file: &str, imp: &ImportRecord) -> ModuleResolution {
    let mut r = super::empty(imp, "python_static_roots");
    r.conditions = vec!["no_runtime_import_hooks".into()];
    let (project, roots) = {
        let mut directory = parent(file);
        loop {
            if let Some((key, roots)) = model.python().roots.get_key_value(directory) {
                break (key.as_str(), roots.clone());
            }
            if directory.is_empty() {
                break ("", vec![String::new(), "src".into()]);
            }
            directory = parent(directory);
        }
    };
    if let Some(e) = model.python().diagnostics.get(project) {
        r.status = ModuleStatus::Unsupported;
        r.reason = Some(e.join(";"));
        return r;
    }
    let manifest = join(project, "pyproject.toml");
    if model.manifests().contains_key(&manifest) {
        r.config_dependencies.insert(manifest);
    }
    let spec = &imp.import_string;
    let dots = spec.bytes().take_while(|&b| b == b'.').count();
    let tail = &spec[dots..];
    if (!tail.is_empty() && !valid_module(tail))
        || spec.is_empty()
        || spec.len() > 4096
        || (!spec.starts_with('.') && !valid_module(spec))
    {
        r.status = ModuleStatus::Unsupported;
        r.reason = Some("invalid_python_module".into());
        return r;
    }
    let search = if dots > 0 {
        let Some(source_root) = roots
            .iter()
            .filter(|root| under(file, root))
            .max_by_key(|s| s.len())
        else {
            r.reason = Some("python_source_root_unknown".into());
            return r;
        };
        let mut package = parent(file).to_owned();
        if package == *source_root {
            r.reason = Some("relative_import_without_package".into());
            return r;
        }
        for _ in 1..dots {
            package = parent(&package).into();
            if package == *source_root || !under(&join(&package, "x.py"), source_root) {
                r.reason = Some("relative_import_beyond_top_level".into());
                return r;
            }
        }
        vec![package]
    } else {
        roots
    };
    let parts = if tail.is_empty() {
        vec![]
    } else {
        tail.split('.').collect::<Vec<_>>()
    };
    match locate(model.files(), search, &parts, &mut r) {
        Located::File(path) => {
            r.resolved_path = Some(path);
            r.status = ModuleStatus::Resolved;
        }
        Located::Missing(reason, closed) => {
            r.reason = Some(reason.into());
            if dots == 0 && !closed {
                r.status = ModuleStatus::Unknown;
                r.reason = Some("python_external_environment_not_captured".into());
            }
        }
        Located::Namespace(paths) => {
            if let Some(name) = imp
                .imported_name
                .as_deref()
                .filter(|s| valid_module(s) && *s != "*")
            {
                if let Located::File(path) = locate(
                    model.files(),
                    paths,
                    &name.split('.').collect::<Vec<_>>(),
                    &mut r,
                ) {
                    r.resolved_path = Some(path);
                    r.status = ModuleStatus::Resolved;
                    r.strategy = "python_namespace_submodule".into();
                    return r;
                }
            }
            r.status = ModuleStatus::Unknown;
            r.reason = Some("namespace_package_has_no_single_source_file".into());
        }
    }
    r
}
