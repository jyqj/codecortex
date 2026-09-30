//! Ordered package target selection against captured workspace manifests only.
use cc_model::{module_inputs::*, project_model::*, ImportRecord};
use std::collections::BTreeSet;

pub(super) fn nearest_package<'a>(
    model: &'a ProjectModel,
    file: &str,
) -> Option<(&'a str, &'a ConfigDocument)> {
    let mut dir = parent(file);
    loop {
        let p = if dir.is_empty() {
            "package.json".into()
        } else {
            format!("{dir}/package.json")
        };
        if let Some((path, doc)) = model.manifests().get_key_value(&p) {
            return Some((path, doc));
        }
        if dir.is_empty() {
            return None;
        }
        dir = parent(dir);
    }
}
pub(super) fn conditions(
    model: &ProjectModel,
    file: &str,
    c: &TypeScriptConfig,
    imp: &ImportRecord,
) -> Result<Vec<String>, String> {
    let is_node = matches!(c.module_resolution.as_str(), "node16" | "nodenext");
    let kind = if matches!(
        imp.context.syntax,
        ImportSyntax::Require | ImportSyntax::TypeOnlyRequire
    ) {
        "require"
    } else if imp.context.syntax == ImportSyntax::Dynamic {
        "import"
    } else if is_node {
        if file.ends_with(".mts") || file.ends_with(".mjs") {
            "import"
        } else if file.ends_with(".cts") || file.ends_with(".cjs") {
            "require"
        } else {
            match nearest_package(model, file) {
                Some((_, ConfigDocument::Invalid(e))) => return Err(e.clone()),
                Some((_, ConfigDocument::Package(p))) if !p.diagnostics.is_empty() => {
                    return Err(p.diagnostics.join(";"))
                }
                Some((_, ConfigDocument::Package(p)))
                    if p.package_type.as_deref() == Some("module") =>
                {
                    "import"
                }
                _ => "require",
            }
        }
    } else {
        "import"
    };
    let mut out = c
        .conditions
        .iter()
        .filter(|x| !matches!(x.as_str(), "local_files" | "import" | "require"))
        .cloned()
        .collect::<Vec<_>>();
    out.push("types".into());
    out.push(kind.into());
    if is_node || c.module_resolution == "node10" {
        out.push("node".into());
    }
    out.sort();
    out.dedup();
    Ok(out)
}
#[derive(Debug, Clone)]
enum Choice {
    Missing,
    Blocked,
    Path(String),
    Invalid(String),
}
fn valid_target(s: &str, imports: bool) -> bool {
    !s.is_empty()
        && s.len() <= 4096
        && !s.contains(['\\', '\0', '%', ':', '?'])
        && !s.starts_with('/')
        && (imports || s.starts_with("./"))
        && s.strip_prefix("./")
            .unwrap_or(s)
            .split('/')
            .all(|x| !matches!(x, "." | ".." | "node_modules") && !x.is_empty())
}
fn choose(
    t: &PackageTarget,
    conditions: &[String],
    star: Option<&str>,
    imports: bool,
    depth: usize,
) -> Choice {
    if depth > 32 {
        return Choice::Invalid("package_target_depth_limit".into());
    }
    match t {
        PackageTarget::Absent => Choice::Missing,
        PackageTarget::Null => Choice::Blocked,
        PackageTarget::Invalid => Choice::Invalid("invalid_package_target".into()),
        PackageTarget::Path(s) => {
            let s = if let Some(cap) = star {
                s.replace('*', cap)
            } else {
                s.clone()
            };
            if !valid_target(&s, imports) || s.contains('*') {
                Choice::Invalid("invalid_or_escaping_package_target".into())
            } else {
                Choice::Path(s)
            }
        }
        PackageTarget::Array(a) => {
            let mut last = Choice::Blocked;
            for item in a {
                match choose(item, conditions, star, imports, depth + 1) {
                    Choice::Path(s) => return Choice::Path(s),
                    Choice::Missing => {}
                    c => last = c,
                }
            }
            last
        }
        PackageTarget::Object(o) => {
            if o.iter()
                .any(|(k, _)| k.starts_with('.') || k.parse::<u64>().is_ok())
            {
                return Choice::Invalid("invalid_condition_keys".into());
            }
            for (k, v) in o {
                if k == "default" || conditions.contains(k) {
                    let picked = choose(v, conditions, star, imports, depth + 1);
                    if !matches!(picked, Choice::Missing) {
                        return picked;
                    }
                }
            }
            Choice::Missing
        }
    }
}
fn mapped(target: &PackageTarget, key: &str, conditions: &[String], imports: bool) -> Choice {
    if let PackageTarget::Object(o) = target {
        let is_map = imports || o.iter().any(|(k, _)| k.starts_with('.'));
        if is_map {
            if o.iter().any(|(k, _)| {
                if imports {
                    !k.starts_with('#') || k == "#" || k.starts_with("#/")
                } else {
                    !k.starts_with('.')
                }
            }) {
                return Choice::Invalid("mixed_or_invalid_package_map_keys".into());
            }
            if let Some((_, v)) = o.iter().find(|(k, _)| k == key) {
                return choose(v, conditions, None, imports, 0);
            }
            let mut patterns = o
                .iter()
                .filter_map(|(k, v)| {
                    let (pre, suf) = k.split_once('*')?;
                    if suf.contains('*')
                        || key.len() < pre.len() + suf.len()
                        || !key.starts_with(pre)
                        || !key.ends_with(suf)
                    {
                        return None;
                    }
                    Some((
                        pre.len(),
                        k.len(),
                        v,
                        &key[pre.len()..key.len() - suf.len()],
                    ))
                })
                .collect::<Vec<_>>();
            patterns.sort_by_key(|b| std::cmp::Reverse((b.0, b.1)));
            return patterns.first().map_or(Choice::Missing, |(_, _, v, cap)| {
                choose(v, conditions, Some(cap), imports, 0)
            });
        }
    }
    if !imports && key == "." {
        choose(target, conditions, None, false, 0)
    } else {
        Choice::Missing
    }
}
fn split_package(spec: &str) -> Option<(&str, String)> {
    if spec.starts_with('@') {
        let first = spec.find('/')?;
        if first == 1 {
            return None;
        }
        let end = spec[first + 1..]
            .find('/')
            .map_or(spec.len(), |n| first + 1 + n);
        if end == first + 1 {
            return None;
        }
        Some((
            &spec[..end],
            if end == spec.len() {
                ".".into()
            } else {
                format!(".{}", &spec[end..])
            },
        ))
    } else {
        let end = spec.find('/').unwrap_or(spec.len());
        if end == 0 {
            return None;
        }
        Some((
            &spec[..end],
            if end == spec.len() {
                ".".into()
            } else {
                format!(".{}", &spec[end..])
            },
        ))
    }
}
fn select_package(
    model: &ProjectModel,
    file: &str,
    name: &str,
    r: &mut ModuleResolution,
) -> Result<Option<String>, String> {
    if let Some((owner, ConfigDocument::Package(p))) = nearest_package(model, file) {
        if p.name.as_deref() == Some(name) && !matches!(p.exports, PackageTarget::Absent) {
            return Ok(Some(owner.into()));
        }
        if let Some(spec) = p.dependencies.get(name) {
            for prefix in ["file:", "link:"] {
                if let Some(rel) = spec.strip_prefix(prefix) {
                    let dir = join_relative(parent(owner), rel)
                        .ok_or("package_dependency_outside_project")?;
                    let path = if dir.is_empty() {
                        "package.json".into()
                    } else {
                        format!("{dir}/package.json")
                    };
                    r.config_dependencies.insert(path.clone());
                    // A declared local link missing its manifest is missing, not external.
                    return Ok(Some(path));
                }
            }
        }
    }
    let nearest = model.nearest_workspace(file);
    let candidates = model
        .packages_named(name)
        .iter()
        .filter(|candidate| nearest.is_some_and(|(_, members)| members.contains(*candidate)))
        .cloned()
        .collect::<BTreeSet<_>>();
    if candidates.len() > 1 {
        r.candidates = candidates.into_iter().collect();
        r.status = ModuleStatus::Ambiguous;
        return Err("ambiguous_workspace_package_name".into());
    }
    Ok(candidates.into_iter().next())
}
fn apply_path(
    model: &ProjectModel,
    manifest: &str,
    path: &str,
    r: &mut ModuleResolution,
    exact_target: bool,
) {
    let Some(base) = join_relative(parent(manifest), path) else {
        r.status = ModuleStatus::Unsupported;
        r.reason = Some("package_path_outside_project".into());
        return;
    };
    let probes = super::typescript::candidates(&base, false);
    // Exports/imports target paths are exact runtime paths. No directory/index fallback.
    if exact_target && !base.rsplit('/').next().is_some_and(|n| n.contains('.')) {
        r.status = ModuleStatus::Unsupported;
        r.reason = Some("extensionless_package_target_unsupported".into());
        return;
    }
    r.probes.extend(probes);
    r.resolved_path = r.probes.iter().find(|p| model.files().contains(p)).cloned();
    r.status = if r.resolved_path.is_some() {
        ModuleStatus::Resolved
    } else {
        ModuleStatus::Unresolved
    };
    r.reason = if r.resolved_path.is_some() {
        None
    } else {
        Some("package_target_not_admitted".into())
    };
}
pub(super) fn directory(
    model: &ProjectModel,
    file: &str,
    imp: &ImportRecord,
    mut r: ModuleResolution,
) -> ModuleResolution {
    let Some(base) = join_relative(parent(file), &imp.import_string) else {
        return r;
    };
    let prefix = format!("{base}/");
    if r.resolved_path
        .as_deref()
        .is_some_and(|p| !p.starts_with(&prefix))
    {
        return r;
    }
    let manifest = format!("{base}/package.json");
    let Some(doc) = model.manifests().get(&manifest) else {
        return r;
    };
    r.config_dependencies.insert(manifest.clone());
    let ConfigDocument::Package(p) = doc else {
        r.resolved_path = None;
        r.status = ModuleStatus::Unsupported;
        r.reason = Some("invalid_directory_package".into());
        return r;
    };
    if !p.diagnostics.is_empty() {
        r.resolved_path = None;
        r.status = ModuleStatus::Unsupported;
        r.reason = Some(p.diagnostics.join(";"));
        return r;
    }
    let fallback = std::mem::take(&mut r.probes);
    r.resolved_path = None;
    for entry in p.types.iter().chain(&p.main) {
        let entry = if entry.starts_with("./") {
            entry.clone()
        } else {
            format!("./{entry}")
        };
        if !valid_target(&entry, false) {
            r.status = ModuleStatus::Unsupported;
            r.reason = Some("directory_entry_outside_package".into());
            return r;
        }
        apply_path(model, &manifest, &entry, &mut r, false);
        if r.resolved_path.is_some() {
            r.strategy = "directory_package_entry".into();
            r.probes.extend(fallback);
            return r;
        }
    }
    r.probes.extend(fallback);
    r.resolved_path = r.probes.iter().find(|p| model.files().contains(p)).cloned();
    r.status = if r.resolved_path.is_some() {
        ModuleStatus::Resolved
    } else {
        ModuleStatus::Unresolved
    };
    r.reason = if r.resolved_path.is_some() {
        None
    } else {
        Some("directory_module_not_admitted".into())
    };
    r
}
pub(super) fn resolve(
    model: &ProjectModel,
    file: &str,
    imp: &ImportRecord,
    c: &TypeScriptConfig,
    mut r: ModuleResolution,
) -> ModuleResolution {
    let mut spec = imp.import_string.clone();
    let mut seen = BTreeSet::new();
    for _ in 0..16 {
        if !seen.insert(spec.clone()) {
            r.status = ModuleStatus::Unsupported;
            r.reason = Some("package_imports_cycle".into());
            return r;
        }
        if spec.starts_with('#') {
            let Some((owner, doc)) = nearest_package(model, file) else {
                r.reason = Some("package_imports_without_owner".into());
                return r;
            };
            r.config_dependencies.insert(owner.into());
            let ConfigDocument::Package(p) = doc else {
                r.status = ModuleStatus::Unsupported;
                r.reason = Some("invalid_owning_package".into());
                return r;
            };
            if c.module_resolution == "node10" {
                r.status = ModuleStatus::Unsupported;
                r.reason = Some("package_imports_require_modern_mode".into());
                return r;
            }
            match mapped(&p.imports, &spec, &r.conditions, true) {
                Choice::Path(s) if s.starts_with("./") => {
                    r.strategy = "package_imports".into();
                    apply_path(model, owner, &s, &mut r, true);
                    return r;
                }
                Choice::Path(s) => {
                    spec = s;
                    continue;
                }
                Choice::Invalid(e) => {
                    r.status = ModuleStatus::Unsupported;
                    r.reason = Some(e);
                    return r;
                }
                _ => {
                    r.status = ModuleStatus::Unresolved;
                    r.reason = Some("package_imports_blocked_or_not_defined".into());
                    return r;
                }
            }
        }
        let Some((name, subpath)) = split_package(&spec) else {
            r.status = ModuleStatus::Unsupported;
            r.reason = Some("invalid_package_name".into());
            return r;
        };
        let manifest = match select_package(model, file, name, &mut r) {
            Ok(Some(p)) => p,
            Ok(None) => {
                r.status = ModuleStatus::External;
                r.reason = Some("external_or_unlinked_package".into());
                return r;
            }
            Err(e) => {
                if r.status != ModuleStatus::Ambiguous {
                    r.status = ModuleStatus::Unsupported;
                }
                r.reason = Some(e);
                return r;
            }
        };
        r.config_dependencies.insert(manifest.clone());
        if !model.manifests().contains_key(&manifest) {
            r.status = ModuleStatus::Unresolved;
            r.reason = Some("local_package_manifest_missing".into());
            return r;
        }
        let Some(ConfigDocument::Package(p)) = model.manifests().get(&manifest) else {
            r.status = ModuleStatus::Unknown;
            r.reason = Some("invalid_package_manifest".into());
            return r;
        };
        if !p.diagnostics.is_empty() {
            r.status = ModuleStatus::Unsupported;
            r.reason = Some(p.diagnostics.join(";"));
            return r;
        }
        if !matches!(p.exports, PackageTarget::Absent) && c.module_resolution != "node10" {
            r.strategy = "workspace_package_exports".into();
            match mapped(&p.exports, &subpath, &r.conditions, false) {
                Choice::Path(s) => apply_path(model, &manifest, &s, &mut r, true),
                Choice::Invalid(e) => {
                    r.status = ModuleStatus::Unsupported;
                    r.reason = Some(e)
                }
                _ => {
                    r.status = ModuleStatus::Unresolved;
                    r.reason = Some("package_subpath_not_exported".into())
                }
            }
            return r;
        }
        r.strategy = "workspace_package_legacy_entry".into();
        if subpath != "." {
            apply_path(model, &manifest, &subpath, &mut r, false);
            return r;
        }
        for entry in p
            .types
            .iter()
            .chain(p.main.iter())
            .map(String::as_str)
            .chain(std::iter::once("./index"))
        {
            let entry = if entry.starts_with("./") {
                entry.into()
            } else {
                format!("./{entry}")
            };
            if !valid_target(&entry, false) {
                r.status = ModuleStatus::Unsupported;
                r.reason = Some("invalid_package_entry".into());
                return r;
            }
            apply_path(model, &manifest, &entry, &mut r, false);
            if r.resolved_path.is_some() {
                return r;
            }
        }
        return r;
    }
    r.status = ModuleStatus::Unsupported;
    r.reason = Some("package_redirect_depth_limit".into());
    r
}
