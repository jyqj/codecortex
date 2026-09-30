//! Resolve explicitly declared Rust modules, preserving unknown cfg and macros.
use cc_model::{project_model::*, ImportRecord};
pub(super) fn resolve(model: &ProjectModel, file: &str, imp: &ImportRecord) -> ModuleResolution {
    let mut r = super::empty(imp, "rust_declared_modules");
    let raw = imp.import_string.trim();
    let raw = raw.split_once(" as ").map_or(raw, |(p, _)| p).trim();
    if raw.is_empty() || raw.contains(['{', '}', '*', '<', '>', '!']) {
        r.status = ModuleStatus::Unsupported;
        r.reason = Some("rust_group_macro_or_generic_import_unsupported".into());
        return r;
    }
    let mut parts = raw.split("::").map(str::trim).collect::<Vec<_>>();
    if parts
        .iter()
        .any(|p| p.is_empty() || !p.chars().all(|c| c.is_alphanumeric() || c == '_'))
    {
        r.status = ModuleStatus::Unsupported;
        r.reason = Some("rust_import_syntax_unsupported".into());
        return r;
    }
    let file_locations = model.rust().in_file(file).collect::<Vec<_>>();
    let mut bases = std::collections::BTreeMap::<&str, usize>::new();
    for l in &file_locations {
        bases
            .entry(&l.crate_manifest)
            .and_modify(|n| *n = (*n).min(l.logical_path.len()))
            .or_insert(l.logical_path.len());
    }
    let owners = file_locations
        .iter()
        .copied()
        .filter(|l| {
            let base = bases[&l.crate_manifest.as_str()];
            l.logical_path.len() == base + imp.context.lexical_module.len()
                && l.logical_path[base..] == imp.context.lexical_module
        })
        .collect::<Vec<_>>();
    if owners.len() != 1 {
        r.status = if owners.is_empty() {
            ModuleStatus::Unknown
        } else {
            ModuleStatus::Ambiguous
        };
        r.candidates = owners
            .iter()
            .map(|l| format!("{}:{}", l.crate_manifest, l.logical_path.join("::")))
            .collect();
        r.reason = Some("rust_module_owner_missing_or_ambiguous".into());
        return r;
    }
    let owner = owners[0];
    let mut manifest = owner.crate_manifest.clone();
    let Some(mut krate) = model.rust().crates.get(&manifest) else {
        return r;
    };
    r.config_dependencies.insert(manifest.clone());
    r.conditions.extend(krate.conditions.iter().cloned());
    r.conditions.extend(imp.context.conditions.iter().cloned());
    if let Some(e) = &owner.blocked {
        r.status = ModuleStatus::Unsupported;
        r.reason = Some(e.clone());
        return r;
    }
    for condition in &imp.context.conditions {
        if crate::project_model::rust::cfg(condition, &krate.conditions, 0) != Some(true) {
            r.status = ModuleStatus::Unsupported;
            r.reason = Some(format!("rust_import_cfg_not_enabled:{condition}"));
            return r;
        }
    }
    let first = parts.remove(0);
    let mut logical = match first {
        "crate" => vec![],
        "self" => owner.logical_path.clone(),
        "super" => {
            let mut p = owner.logical_path.clone();
            if p.pop().is_none() {
                r.reason = Some("rust_super_above_crate".into());
                return r;
            }
            p
        }
        dependency => {
            if let Some(reason) = krate.blocked_dependencies.get(dependency) {
                r.status = ModuleStatus::Unsupported;
                r.reason = Some(reason.clone());
                return r;
            }
            if krate.external_dependencies.contains(dependency) {
                r.status = ModuleStatus::External;
                r.reason = Some("rust_declared_external_dependency_not_indexed".into());
                return r;
            }
            if let Some(m) = krate.dependencies.get(dependency) {
                manifest = m.clone();
                let Some(c) = model.rust().crates.get(&manifest) else {
                    r.reason = Some("rust_dependency_not_admitted".into());
                    return r;
                };
                krate = c;
                vec![]
            } else if dependency == krate.name {
                vec![]
            } else if let Some(entry) = model.rust_aliases().get(dependency) {
                // Retain the old workspace convenience as an explicitly heuristic policy.
                let owners = model
                    .rust()
                    .by_entry
                    .get(entry)
                    .map_or(&[][..], Vec::as_slice);
                let [owner] = owners else {
                    r.status = if owners.len() > 1 {
                        ModuleStatus::Ambiguous
                    } else {
                        ModuleStatus::Unknown
                    };
                    r.candidates = owners.to_vec();
                    r.reason = Some("rust_alias_entry_owner_missing_or_ambiguous".into());
                    return r;
                };
                let Some(c) = model.rust().crates.get(owner) else {
                    return r;
                };
                manifest = owner.clone();
                krate = c;
                r.strategy = "rust_workspace_alias_heuristic".into();
                vec![]
            } else {
                parts.insert(0, dependency);
                owner.logical_path.clone()
            }
        }
    };
    while parts.first() == Some(&"super") {
        parts.remove(0);
        if logical.pop().is_none() {
            r.reason = Some("rust_super_above_crate".into());
            return r;
        }
    }
    r.config_dependencies.insert(manifest.clone());
    r.conditions.extend(krate.conditions.iter().cloned());
    r.conditions.sort();
    r.conditions.dedup();
    let mut location = model.rust().at(&manifest, &logical).next();
    let mut consumed = 0;
    for segment in &parts {
        let mut next = logical.clone();
        next.push((*segment).into());
        let matched = model.rust().at(&manifest, &next).collect::<Vec<_>>();
        if matched.len() > 1 {
            r.status = ModuleStatus::Ambiguous;
            r.candidates = matched.iter().map(|l| l.file.clone()).collect();
            r.reason = Some("rust_duplicate_module_declaration".into());
            return r;
        }
        let Some(found) = matched.first() else {
            break;
        };
        location = Some(*found);
        logical = next;
        consumed += 1;
    }
    let Some(location) = location else {
        r.reason = Some("rust_declared_module_missing".into());
        return r;
    };
    r.probes.push(location.file.clone());
    if let Some(e) = &location.blocked {
        r.status = if e == "rust_module_file_missing" {
            ModuleStatus::Unresolved
        } else {
            ModuleStatus::Unsupported
        };
        r.reason = Some(e.clone());
        return r;
    }
    if parts.len().saturating_sub(consumed) > 1 {
        r.status = ModuleStatus::Unsupported;
        r.reason = Some("rust_reexport_or_associated_path_not_resolved".into());
        return r;
    }
    if model.files().contains(&location.file) {
        r.resolved_path = Some(location.file.clone());
        r.status = ModuleStatus::Resolved;
        r.reason = None;
    } else {
        r.reason = Some("rust_module_source_not_admitted".into());
    }
    r
}
