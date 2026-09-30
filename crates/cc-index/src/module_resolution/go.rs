//! Local Go module/workspace resolution over immutable inputs. No MVS/network guessing.
use cc_model::{go_project::*, project_model::*, ImportRecord};
use std::collections::{BTreeMap, BTreeSet};
pub(crate) fn path(base: &str, tail: &str) -> String {
    if base.is_empty() {
        tail.into()
    } else {
        format!("{base}/{tail}")
    }
}
fn under(file: &str, dir: &str) -> bool {
    dir.is_empty() || file.starts_with(&format!("{dir}/"))
}
fn prefix(spec: &str, module: &str) -> bool {
    spec == module
        || spec
            .strip_prefix(module)
            .is_some_and(|s| s.starts_with('/'))
}
fn issue(
    mut r: ModuleResolution,
    status: ModuleStatus,
    why: impl Into<String>,
) -> ModuleResolution {
    r.status = status;
    r.reason = Some(why.into());
    r
}
fn nearest<'a>(model: &'a GoProject, file: &str, name: &str) -> Option<(&'a String, &'a GoConfig)> {
    let mut directory = parent(file);
    loop {
        if let Some(pair) = model.configs.get_key_value(&path(directory, name)) {
            return Some(pair);
        }
        if directory.is_empty() {
            return None;
        }
        directory = parent(directory);
    }
}
pub(super) fn resolve(model: &ProjectModel, file: &str, imp: &ImportRecord) -> ModuleResolution {
    let mut r = super::empty(imp, "go_local_modules");
    let g = model.go();
    let spec = imp.import_string.as_str();
    if spec.is_empty()
        || spec.len() > 4096
        || spec.split('/').any(|s| {
            s.is_empty() || matches!(s, "." | "..") || s.contains(['\\', '\0', ':', '%', ' ', '\t'])
        })
    {
        return issue(r, ModuleStatus::Unsupported, "invalid_go_import_path");
    }
    // Missing ancestor configs are dependencies too: creating go.work/go.mod must invalidate.
    let mut dir = parent(file);
    loop {
        r.config_dependencies.insert(path(dir, "go.mod"));
        r.config_dependencies.insert(path(dir, "go.work"));
        if dir.is_empty() {
            break;
        }
        dir = parent(dir);
    }
    let Some((owner, config)) = nearest(g, file, "go.mod") else {
        return issue(r, ModuleStatus::Unknown, "go_module_owner_not_captured");
    };
    if !config.diagnostics.is_empty() {
        return issue(r, ModuleStatus::Unknown, config.diagnostics.join(";"));
    }
    let mut main = BTreeMap::from([(owner.clone(), config)]);
    let work = nearest(g, file, "go.work");
    if let Some((work_path, w)) = work {
        if !w.diagnostics.is_empty() {
            return issue(r, ModuleStatus::Unknown, w.diagnostics.join(";"));
        }
        main.clear();
        for rel in &w.uses {
            let Some(d) = join_relative(parent(work_path), rel) else {
                return issue(
                    r,
                    ModuleStatus::Unsupported,
                    "go_workspace_use_outside_project",
                );
            };
            let p = path(&d, "go.mod");
            r.config_dependencies.insert(p.clone());
            let Some(c) = g.configs.get(&p) else {
                return issue(
                    r,
                    ModuleStatus::Unresolved,
                    "go_workspace_member_manifest_missing",
                );
            };
            if !c.diagnostics.is_empty() {
                return issue(r, ModuleStatus::Unknown, "go_workspace_member_invalid");
            }
            main.insert(p, c);
        }
        if !main.contains_key(owner) {
            return issue(
                r,
                ModuleStatus::Unknown,
                "go_importer_outside_workspace_members",
            );
        }
    }
    r.config_dependencies.extend(main.keys().cloned());
    let mut roots = BTreeSet::new();
    let mut main_prefixes = BTreeSet::new();
    for (p, c) in &main {
        if let Some(m) = &c.module {
            if prefix(spec, m) {
                main_prefixes.insert(m.clone());
                roots.insert((m.clone(), parent(p).to_owned()));
            }
        }
    }
    // Include required module boundaries, so an unindexed nested module is not resolved in its parent.
    let mut requirements: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for c in main.values() {
        for (m, v) in &c.requires {
            if prefix(spec, m) {
                requirements.entry(m.clone()).or_default().insert(v.clone());
            }
        }
    }
    for (module, versions) in &requirements {
        if main_prefixes.contains(module) {
            continue;
        }
        if versions.len() != 1 {
            return issue(
                r,
                ModuleStatus::Unknown,
                "go_version_selection_requires_external_oracle",
            );
        }
        let mut replacements: Vec<(&str, &GoReplace)> = vec![];
        if let Some((wp, w)) = work {
            replacements.extend(
                w.replaces
                    .iter()
                    .filter(|v| &v.module == module)
                    .map(|v| (parent(wp), v)),
            );
        }
        if replacements.is_empty() {
            for (p, c) in &main {
                replacements.extend(
                    c.replaces
                        .iter()
                        .filter(|v| &v.module == module)
                        .map(|v| (parent(p), v)),
                );
            }
        }
        if replacements.iter().any(|(_, v)| v.version.is_some()) {
            return issue(
                r,
                ModuleStatus::Unknown,
                "go_versioned_replace_requires_mvs",
            );
        }
        let mut targets = BTreeSet::new();
        for (origin, v) in replacements {
            if v.target_version.is_some() {
                targets.insert(("external".to_owned(), v.target.clone()));
            } else {
                if !v.target.starts_with("./")
                    && !v.target.starts_with("../")
                    && v.target != "."
                    && v.target != ".."
                {
                    return issue(
                        r,
                        ModuleStatus::Unsupported,
                        "go_replace_nonlocal_target_without_version",
                    );
                }
                let Some(d) = join_relative(origin, &v.target) else {
                    return issue(r, ModuleStatus::Unsupported, "go_replace_outside_project");
                };
                targets.insert(("local".to_owned(), d));
            }
        }
        if targets.len() > 1 {
            r.candidates = targets
                .into_iter()
                .map(|(kind, p)| format!("{kind}:{p}"))
                .collect();
            return issue(
                r,
                ModuleStatus::Ambiguous,
                "go_conflicting_main_module_replaces",
            );
        }
        if let Some((kind, d)) = targets.into_iter().next() {
            if kind == "local" {
                let m = path(&d, "go.mod");
                r.config_dependencies.insert(m.clone());
                let Some(c) = g.configs.get(&m) else {
                    return issue(
                        r,
                        ModuleStatus::Unresolved,
                        "go_replace_manifest_not_admitted",
                    );
                };
                if !c.diagnostics.is_empty() {
                    return issue(r, ModuleStatus::Unknown, "invalid_go_replace_manifest");
                }
                roots.insert((module.clone(), d));
            } else {
                return issue(r, ModuleStatus::External, "go_remote_replace_not_indexed");
            }
        } else {
            return issue(
                r,
                ModuleStatus::External,
                "go_required_dependency_not_indexed",
            );
        }
    }
    if roots.is_empty() {
        return issue(
            r,
            ModuleStatus::External,
            "go_standard_or_remote_package_not_indexed",
        );
    }
    // Distinct visible module roots containing the same package are ambiguous; no map-order winner.
    let mut found = vec![];
    let mut missing_local = false;
    for (module, root) in roots {
        let suffix = spec
            .strip_prefix(&module)
            .unwrap_or("")
            .trim_start_matches('/');
        let directory = if suffix.is_empty() {
            root.clone()
        } else {
            path(&root, suffix)
        };
        let mut ancestor = directory.as_str();
        loop {
            r.config_dependencies.insert(path(ancestor, "go.mod"));
            if ancestor.is_empty() || ancestor == root {
                break;
            }
            ancestor = parent(ancestor);
        }
        let boundary = nearest(g, &path(&directory, "probe.go"), "go.mod");
        if boundary.is_some_and(|(p, _)| parent(p) != root) {
            missing_local = true;
            continue;
        }
        if let Some(internal) = directory
            .split('/')
            .enumerate()
            .filter(|(_, s)| *s == "internal")
            .map(|(i, _)| directory.split('/').take(i).collect::<Vec<_>>().join("/"))
            .last()
        {
            if !under(file, &internal) {
                return issue(
                    r,
                    ModuleStatus::Unsupported,
                    "go_internal_package_not_visible",
                );
            }
        }
        let members = g
            .directories
            .get(&directory)
            .into_iter()
            .flatten()
            .filter(|p| !p.ends_with("_test.go"))
            .take(4097)
            .cloned()
            .collect::<Vec<_>>();
        if members.len() > 4096 {
            r.candidates = members.into_iter().take(64).collect();
            return issue(
                r,
                ModuleStatus::Unknown,
                "go_package_member_budget_exceeded",
            );
        }
        if members.is_empty() {
            missing_local = true;
            continue;
        }
        r.probes.extend(members.iter().cloned());
        r.config_dependencies.extend(members.iter().cloned());
        let names: BTreeSet<_> = members
            .iter()
            .filter_map(|p| g.sources.get(p).and_then(|s| s.package.clone()))
            .collect();
        if names.is_empty() {
            return issue(r, ModuleStatus::Unknown, "go_package_identity_missing");
        }
        if names.len() != 1 {
            r.candidates = names.into_iter().collect();
            return issue(
                r,
                ModuleStatus::Ambiguous,
                "go_conflicting_package_declarations",
            );
        }
        let mut unknown = false;
        for p in &members {
            let f = &g.sources[p];
            unknown |= f.unknown || !f.conditions.is_empty();
            r.conditions
                .extend(f.conditions.iter().map(|c| format!("{p}:{c}")));
        }
        if unknown {
            r.candidates = members;
            return issue(r, ModuleStatus::Unknown, "go_build_profile_not_selected");
        }
        let name = names.into_iter().next().unwrap();
        if name == "main" {
            return issue(r, ModuleStatus::Unsupported, "go_program_is_not_importable");
        }
        found.push(ResolvedPackage {
            directory,
            name,
            files: members,
        });
    }
    if found.len() > 1 {
        r.candidates = found.iter().map(|p| p.directory.clone()).collect();
        return issue(r, ModuleStatus::Ambiguous, "go_ambiguous_package_roots");
    }
    if let Some(p) = found.pop() {
        r.resolved_package = Some(p);
        r.status = ModuleStatus::Resolved;
        r.conditions
            .push("portable_unconditional_files;tests_excluded;no_host_profile".into());
        return r;
    }
    issue(
        r,
        ModuleStatus::Unresolved,
        if missing_local {
            "go_local_package_missing"
        } else {
            "go_package_missing"
        },
    )
}
