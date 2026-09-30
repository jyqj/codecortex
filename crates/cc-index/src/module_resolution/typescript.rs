//! Local TS paths/baseUrl subset. No package/exports or Node format guesswork.
use cc_model::project_model::*;
use std::collections::BTreeSet;
pub(super) fn candidates(base: &str, json: bool) -> Vec<String> {
    for (ext, replacements) in [
        (".mjs", vec![".mts", ".d.mts", ".mjs"]),
        (".cjs", vec![".cts", ".d.cts", ".cjs"]),
        (".jsx", vec![".tsx", ".d.ts", ".jsx"]),
        (".js", vec![".ts", ".tsx", ".d.ts", ".js", ".jsx"]),
    ] {
        if let Some(stem) = base.strip_suffix(ext) {
            return replacements.iter().map(|e| format!("{stem}{e}")).collect();
        }
    }
    if [".ts", ".tsx", ".mts", ".cts"]
        .iter()
        .any(|e| base.ends_with(e))
        || (json && base.ends_with(".json"))
    {
        return vec![base.into()];
    }
    // Non-code assets are not invented as local source modules.
    if base.rsplit('/').next().is_some_and(|n| n.contains('.')) {
        return vec![];
    }
    let exts = [".ts", ".tsx", ".d.ts", ".js", ".jsx"];
    exts.iter()
        .map(|e| format!("{base}{e}"))
        .chain(exts.iter().map(|e| format!("{base}/index{e}")))
        .collect()
}
pub fn resolve(
    files: &FileCatalog,
    c: &TypeScriptConfig,
    spec: &str,
    from: &str,
) -> ModuleResolution {
    let mut r = ModuleResolution {
        resolved_package: None,
        candidates: vec![],
        request_key: String::new(),
        import_string: spec.into(),
        resolved_path: None,
        status: ModuleStatus::Unresolved,
        strategy: "project_local".into(),
        module_resolution: c.module_resolution.clone(),
        conditions: c.conditions.clone(),
        config_dependencies: c.dependencies.clone(),
        probes: vec![],
        reason: None,
    };
    if !c.diagnostics.is_empty() {
        r.status = ModuleStatus::Unsupported;
        r.reason = Some(
            c.diagnostics
                .iter()
                .take(8)
                .map(|d| d.reason.as_str())
                .collect::<Vec<_>>()
                .join(";"),
        );
        return r;
    }
    if spec.is_empty() || spec.len() > 4096 || spec.contains(['\\', '\0']) || spec.starts_with('/')
    {
        r.status = ModuleStatus::Unsupported;
        r.reason = Some("invalid_module_specifier".into());
        return r;
    }
    let mut bases = Vec::new();
    if spec.starts_with('.') {
        if let Some(p) = join_relative(parent(from), spec) {
            bases.push(p);
        } else {
            r.status = ModuleStatus::Unsupported;
            r.reason = Some("module_path_outside_project".into());
            return r;
        }
    } else {
        if let Some(paths) = &c.paths {
            // Equal-prefix overlapping patterns need source-order semantics.
            // The normalized map intentionally does not invent that ordering.
            if !paths.patterns.contains_key(spec) {
                let mut priorities = std::collections::BTreeMap::<usize, usize>::new();
                for pattern in paths.patterns.keys() {
                    if let Some((pre, suf)) = pattern.split_once('*') {
                        if spec.len() >= pre.len() + suf.len()
                            && spec.starts_with(pre)
                            && spec.ends_with(suf)
                        {
                            *priorities.entry(pre.len()).or_default() += 1;
                        }
                    }
                }
                if priorities
                    .last_key_value()
                    .is_some_and(|(_, count)| *count > 1)
                {
                    r.status = ModuleStatus::Unsupported;
                    r.reason = Some("equal_priority_paths_patterns_unsupported".into());
                    return r;
                }
            }
            let matched = paths
                .patterns
                .get_key_value(spec)
                .map(|(k, v)| (k, v, String::new()))
                .or_else(|| {
                    paths
                        .patterns
                        .iter()
                        .filter_map(|(pattern, values)| {
                            let (pre, suf) = pattern.split_once('*')?;
                            if spec.len() < pre.len() + suf.len()
                                || !spec.starts_with(pre)
                                || !spec.ends_with(suf)
                            {
                                return None;
                            }
                            Some((
                                pattern,
                                values,
                                spec[pre.len()..spec.len() - suf.len()].to_owned(),
                            ))
                        })
                        .max_by(|(a, _, _), (b, _, _)| {
                            a.find('*').cmp(&b.find('*')).then_with(|| b.cmp(a))
                        })
                });
            if let Some((_, targets, capture)) = matched {
                r.strategy = "ts_paths".into();
                let directory = c
                    .base_url
                    .as_deref()
                    .unwrap_or_else(|| parent(&paths.defined_in));
                for target in targets {
                    if let Some(base) = join_relative(directory, &target.replace('*', &capture)) {
                        bases.push(base);
                    } else {
                        r.status = ModuleStatus::Unsupported;
                        r.reason = Some("paths_target_outside_project".into());
                        return r;
                    }
                }
            }
        }
        if let Some(base) = &c.base_url {
            if let Some(p) = join_relative(base, spec) {
                bases.push(p);
                if r.strategy == "project_local" {
                    r.strategy = "ts_base_url".into();
                }
            }
        }
    }
    let mut seen = BTreeSet::new();
    for base in bases {
        for path in candidates(&base, c.resolve_json_module) {
            if seen.insert(path.clone()) {
                r.probes.push(path);
            }
        }
    }
    r.resolved_path = r.probes.iter().find(|p| files.contains(p)).cloned();
    if r.resolved_path.is_some() {
        r.status = ModuleStatus::Resolved;
    } else {
        if r.probes.is_empty() {
            r.status = ModuleStatus::Unsupported;
        }
        r.reason = Some(
            if r.probes.is_empty() {
                "external_package_or_asset_not_supported_in_p3a"
            } else {
                "no_admitted_local_candidate"
            }
            .into(),
        );
    }
    r
}
