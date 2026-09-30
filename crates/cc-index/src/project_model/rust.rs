//! Cargo/member/module discovery from admitted inventory and captured syntax.
use cc_model::{module_inputs::*, project_model::*, CcError, CcResult};
use std::collections::{BTreeMap, BTreeSet, HashMap};
fn joined(root: &str, p: &str) -> Option<String> {
    join_relative(root, p)
}
fn string<'a>(v: &'a serde_json::Value, path: &str) -> Option<&'a str> {
    v.pointer(path).and_then(|v| v.as_str())
}
fn entries(v: &serde_json::Value, path: &str) -> Vec<String> {
    v.pointer(path)
        .and_then(|v| v.as_array())
        .map_or(vec![], |a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
}
fn features(v: &serde_json::Value) -> BTreeSet<String> {
    let mut todo = entries(v, "/features/default");
    let mut seen = BTreeSet::new();
    let mut out = BTreeSet::new();
    while let Some(f) = todo.pop() {
        if seen.len() >= 4096 {
            break;
        }
        if !seen.insert(f.clone()) {
            continue;
        }
        if f.contains(['/', ':']) {
            out.insert(format!("unresolved_feature:{f}"));
            continue;
        }
        out.insert(format!("feature={f}"));
        if let Some(a) = v
            .get("features")
            .and_then(|v| v.get(&f))
            .and_then(|v| v.as_array())
        {
            todo.extend(a.iter().filter_map(|v| v.as_str().map(str::to_owned)));
        }
    }
    out.insert("profile=cargo_default_features".into());
    out
}
/// Three-valued cfg evaluation. Unknown targets/macros are never assumed false.
pub(crate) fn cfg(raw: &str, selected: &BTreeSet<String>, depth: usize) -> Option<bool> {
    if depth > 32 {
        return None;
    }
    let raw = raw.trim();
    if let Some((key, value)) = raw.split_once('=') {
        if key.trim() == "feature" {
            let value = serde_json::from_str::<String>(value.trim()).ok()?;
            return Some(selected.contains(&format!("feature={value}")));
        }
    }
    for op in ["all", "any", "not"] {
        if let Some(args) = raw
            .strip_prefix(op)
            .and_then(|s| s.trim().strip_prefix('('))
            .and_then(|s| s.strip_suffix(')'))
        {
            let mut parts = Vec::new();
            let (mut start, mut nesting, mut quote, mut escape) = (0, 0, false, false);
            for (i, c) in args.char_indices() {
                if quote {
                    if escape {
                        escape = false;
                    } else if c == '\\' {
                        escape = true;
                    } else if c == '"' {
                        quote = false;
                    }
                } else {
                    match c {
                        '"' => quote = true,
                        '(' => nesting += 1,
                        ')' => {
                            if nesting == 0 {
                                return None;
                            }
                            nesting -= 1;
                        }
                        ',' if nesting == 0 => {
                            if !args[start..i].trim().is_empty() {
                                parts.push(&args[start..i]);
                            }
                            start = i + 1;
                        }
                        _ => {}
                    }
                }
            }
            if quote || nesting != 0 {
                return None;
            }
            if !args[start..].trim().is_empty() {
                parts.push(&args[start..]);
            }
            let vals = parts
                .iter()
                .map(|p| cfg(p, selected, depth + 1))
                .collect::<Vec<_>>();
            return match op {
                "not" if vals.len() == 1 => vals[0].map(|v| !v),
                "all" => {
                    if vals.contains(&Some(false)) {
                        Some(false)
                    } else if vals.iter().any(Option::is_none) {
                        None
                    } else {
                        Some(true)
                    }
                }
                "any" => {
                    if vals.contains(&Some(true)) {
                        Some(true)
                    } else if vals.iter().any(Option::is_none) {
                        None
                    } else {
                        Some(false)
                    }
                }
                _ => None,
            };
        }
    }
    None
}
fn blocked(conditions: &[String], selected: &BTreeSet<String>) -> Option<String> {
    for c in conditions {
        match cfg(c, selected, 0) {
            Some(true) => {}
            Some(false) => return Some("rust_cfg_disabled".into()),
            None => return Some(format!("rust_cfg_unresolved:{c}")),
        }
    }
    None
}
pub(super) fn build(
    documents: &BTreeMap<String, ConfigDocument>,
    files: &FileCatalog,
    sources: &BTreeMap<String, RustSourceInput>,
) -> CcResult<(RustProject, HashMap<String, String>)> {
    let mut model = RustProject::default();
    for (manifest, doc) in documents.iter().filter(|(p, _)| p.ends_with("Cargo.toml")) {
        let ConfigDocument::Toml(v) = doc else {
            model.diagnostics.push(format!("invalid_cargo:{manifest}"));
            continue;
        };
        let Some(name) = string(v, "/package/name") else {
            continue;
        };
        let root = parent(manifest);
        let explicit = string(v, "/lib/path");
        let entry = if let Some(p) = explicit {
            joined(root, p)
        } else {
            ["src/lib.rs", "src/main.rs"]
                .iter()
                .filter_map(|p| joined(root, p))
                .find(|p| files.contains(p))
        };
        let Some(entry) = entry.filter(|p| files.contains(p)) else {
            model
                .diagnostics
                .push(format!("missing_crate_entry:{manifest}"));
            continue;
        };
        let mut c = RustCrate {
            manifest: manifest.clone(),
            entry,
            name: string(v, "/lib/name").unwrap_or(name).replace('-', "_"),
            conditions: features(v),
            ..Default::default()
        };
        if explicit.is_some_and(|p| join_relative(root, p).is_none()) {
            c.diagnostics.push("crate_entry_outside_project".into());
        }
        if v.get("target").is_some() {
            c.diagnostics
                .push("target_specific_dependencies_not_selected".into());
        }
        if let Some(deps) = v.get("dev-dependencies").and_then(|v| v.as_object()) {
            for name in deps.keys() {
                if v.get("dependencies").and_then(|d| d.get(name)).is_none() {
                    c.blocked_dependencies.insert(
                        name.replace('-', "_"),
                        "dev_dependency_profile_not_selected".into(),
                    );
                }
            }
        }
        if let Some(targets) = v.get("target").and_then(|v| v.as_object()) {
            for target in targets.values() {
                if let Some(deps) = target.get("dependencies").and_then(|v| v.as_object()) {
                    for name in deps.keys() {
                        c.blocked_dependencies.insert(
                            name.replace('-', "_"),
                            "target_dependency_conditions_not_selected".into(),
                        );
                    }
                }
            }
        }
        for field in ["dependencies"] {
            if let Some(deps) = v.get(field).and_then(|v| v.as_object()) {
                for (alias, raw) in deps {
                    let workspace = documents
                        .iter()
                        .filter(|(p, _)| {
                            manifest.starts_with(&format!("{}/", parent(p))) || parent(p).is_empty()
                        })
                        .filter_map(|(p, d)| {
                            if let ConfigDocument::Toml(v) = d {
                                v.pointer("/workspace/dependencies")
                                    .and_then(|v| v.get(alias))
                                    .map(|dep| (parent(p), dep))
                            } else {
                                None
                            }
                        })
                        .max_by_key(|(p, _)| p.len());
                    let (origin, dep) =
                        if raw.get("workspace").and_then(|v| v.as_bool()) == Some(true) {
                            workspace.unwrap_or((root, raw))
                        } else {
                            (root, raw)
                        };
                    if dep.get("features").is_some() || dep.get("default-features").is_some() {
                        c.blocked_dependencies.insert(
                            alias.replace('-', "_"),
                            "dependency_feature_profile_not_selected".into(),
                        );
                    }
                    if dep.get("optional").and_then(|v| v.as_bool()) == Some(true) {
                        c.blocked_dependencies.insert(
                            alias.replace('-', "_"),
                            "optional_dependency_not_selected".into(),
                        );
                        c.diagnostics
                            .push(format!("optional_dependency_not_selected:{alias}"));
                        continue;
                    }
                    if let Some(path) = dep.get("path").and_then(|v| v.as_str()) {
                        if let Some(dir) = join_relative(origin, path) {
                            let m = if dir.is_empty() {
                                "Cargo.toml".into()
                            } else {
                                format!("{dir}/Cargo.toml")
                            };
                            c.dependencies.insert(alias.replace('-', "_"), m);
                        } else {
                            c.diagnostics
                                .push(format!("dependency_outside_project:{alias}"));
                            c.blocked_dependencies.insert(
                                alias.replace('-', "_"),
                                "dependency_outside_project".into(),
                            );
                        }
                    } else if dep.is_string()
                        || dep.get("version").is_some()
                        || dep.get("git").is_some()
                        || dep.get("registry").is_some()
                    {
                        c.external_dependencies.insert(alias.replace('-', "_"));
                    } else {
                        c.blocked_dependencies.insert(
                            alias.replace('-', "_"),
                            "dependency_declaration_not_understood".into(),
                        );
                    }
                }
            }
        }
        model.crates.insert(manifest.clone(), c);
    }
    if model.crates.is_empty() && !documents.keys().any(|p| p.ends_with("Cargo.toml")) {
        // A root lib.rs/main.rs with declared modules is a static crate-root convention,
        // not a claim about Cargo target selection or caller-supplied rustc flags.
        for entry in ["lib.rs", "main.rs"] {
            if files.contains(entry) {
                model.crates.insert(
                    entry.into(),
                    RustCrate {
                        manifest: entry.into(),
                        entry: entry.into(),
                        name: entry.trim_end_matches(".rs").into(),
                        conditions: BTreeSet::from(["profile=standalone_declared_root".into()]),
                        ..Default::default()
                    },
                );
            }
        }
    }
    // Compatibility workspace aliases are derived from captured member manifests.
    let mut candidates: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (workspace, doc) in documents.iter().filter(|(p, _)| p.ends_with("Cargo.toml")) {
        let ConfigDocument::Toml(v) = doc else {
            continue;
        };
        let members = entries(v, "/workspace/members");
        let excludes = entries(v, "/workspace/exclude");
        let root = parent(workspace);
        for (manifest, c) in &model.crates {
            let dir = parent(manifest);
            let relative = if root.is_empty() {
                Some(dir)
            } else {
                dir.strip_prefix(&format!("{root}/"))
            };
            if let Some(rel) = relative {
                if members
                    .iter()
                    .any(|p| super::package::member_matches(p, rel))
                    && !excludes
                        .iter()
                        .any(|p| super::package::member_matches(p, rel))
                {
                    candidates
                        .entry(c.name.clone())
                        .or_default()
                        .insert(c.entry.clone());
                }
            }
        }
    }
    let aliases = candidates
        .into_iter()
        .filter_map(|(k, v)| {
            if v.len() == 1 {
                Some((k, v.into_iter().next().unwrap()))
            } else {
                model
                    .diagnostics
                    .push(format!("ambiguous_workspace_alias:{k}"));
                None
            }
        })
        .collect();
    for c in model.crates.values() {
        let condition = sources
            .get(&c.entry)
            .and_then(|s| blocked(&s.facts.inner_conditions, &c.conditions));
        let root = RustModuleLocation {
            crate_manifest: c.manifest.clone(),
            logical_path: vec![],
            file: c.entry.clone(),
            directory: parent(&c.entry).into(),
            blocked: condition,
        };
        let mut todo = vec![(root, Vec::<String>::new(), 0usize)];
        let mut expanded = BTreeSet::new();
        while let Some((location, syntax_parent, depth)) = todo.pop() {
            if depth > 64 || model.locations.len() > 100000 {
                return Err(CcError::Config("rust_module_graph_limit".into()));
            }
            if !expanded.insert((location.file.clone(), location.logical_path.clone())) {
                continue;
            }
            model.locations.push(location.clone());
            if location.blocked.is_some() {
                continue;
            }
            let Some(input) = sources.get(&location.file) else {
                continue;
            };
            if input.facts.syntax_error {
                continue;
            }
            for d in input
                .facts
                .modules
                .iter()
                .filter(|d| d.parent == syntax_parent)
            {
                let mut logical = location.logical_path.clone();
                logical.push(d.name.clone());
                let mut next = RustModuleLocation {
                    crate_manifest: c.manifest.clone(),
                    logical_path: logical,
                    file: location.file.clone(),
                    directory: location.directory.clone(),
                    blocked: d
                        .unsupported
                        .clone()
                        .or_else(|| blocked(&d.conditions, &c.conditions)),
                };
                if d.inline {
                    next.directory =
                        join_relative(&location.directory, d.path.as_deref().unwrap_or(&d.name))
                            .unwrap_or_default();
                    if next.directory.is_empty() {
                        next.blocked = Some("rust_inline_path_outside_project".into());
                    }
                    let mut syntax = syntax_parent.clone();
                    syntax.push(d.name.clone());
                    todo.push((next, syntax, depth + 1));
                } else {
                    let base = if let Some(p) = &d.path {
                        let anchor = if syntax_parent.is_empty() {
                            parent(&location.file)
                        } else {
                            &location.directory
                        };
                        join_relative(anchor, p)
                    } else {
                        join_relative(&location.directory, &d.name)
                    };
                    let Some(base) = base else {
                        next.blocked = Some("rust_module_path_outside_project".into());
                        model.locations.push(next);
                        continue;
                    };
                    let probes = if d.path.is_some() {
                        vec![base]
                    } else {
                        vec![format!("{base}.rs"), format!("{base}/mod.rs")]
                    };
                    let found = probes
                        .iter()
                        .filter(|p| files.contains(p))
                        .cloned()
                        .collect::<Vec<_>>();
                    if found.len() != 1 {
                        next.blocked = Some(
                            if found.is_empty() {
                                "rust_module_file_missing"
                            } else {
                                "rust_module_file_ambiguous"
                            }
                            .into(),
                        );
                        next.file = probes[0].clone();
                        model.locations.push(next);
                        continue;
                    }
                    next.file = found[0].clone();
                    let leaf = next.file.rsplit('/').next().unwrap_or("");
                    next.directory = if leaf == "mod.rs" {
                        parent(&next.file).into()
                    } else {
                        next.file.strip_suffix(".rs").unwrap_or(&next.file).into()
                    };
                    if let Some(s) = sources.get(&next.file) {
                        next.blocked = next
                            .blocked
                            .or_else(|| blocked(&s.facts.inner_conditions, &c.conditions));
                    }
                    // Re-entering one physical source in its ancestor chain is conservatively bounded.
                    if next.file == location.file {
                        next.blocked = Some("rust_module_source_cycle".into());
                    }
                    todo.push((next, vec![], depth + 1));
                }
            }
        }
    }
    model.locations.sort_by(|a, b| {
        (&a.crate_manifest, &a.logical_path, &a.file).cmp(&(
            &b.crate_manifest,
            &b.logical_path,
            &b.file,
        ))
    });
    model.index_locations();
    Ok((model, aliases))
}
