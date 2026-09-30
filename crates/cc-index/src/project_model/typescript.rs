//! TS configuration inheritance; option paths retain their defining origin.
use super::config_cache::Loader;
use cc_model::{project_model::*, CcResult};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone, Default)]
struct Effective {
    options: BTreeMap<String, (String, std::sync::Arc<serde_json::Value>)>,
    dependencies: BTreeSet<String>,
    diagnostics: BTreeSet<ConfigDiagnostic>,
}
impl Effective {
    fn issue(&mut self, path: &str, reason: &str) {
        self.diagnostics.insert(ConfigDiagnostic {
            path: path.into(),
            reason: reason.into(),
        });
    }
}
fn load(
    path: &str,
    loader: &mut Loader<'_>,
    stack: &mut Vec<String>,
    memo: &mut BTreeMap<String, Effective>,
) -> CcResult<Effective> {
    if stack.iter().any(|p| p == path) {
        let mut e = Effective::default();
        e.dependencies.insert(path.into());
        e.issue(path, "extends_cycle");
        return Ok(e);
    }
    if stack.len() >= 32 {
        let mut e = Effective::default();
        e.dependencies.insert(path.into());
        e.issue(path, "extends_depth_limit");
        return Ok(e);
    }
    if let Some(e) = memo.get(path) {
        return Ok(e.clone());
    }
    let input = loader.load(path)?;
    let mut e = Effective::default();
    e.dependencies.insert(path.into());
    let Some(raw) = input
        .parsed
        .and_then(cc_model::module_inputs::ConfigDocument::typescript)
    else {
        e.issue(path, input.error.as_deref().unwrap_or("invalid_config"));
        memo.insert(path.into(), e.clone());
        return Ok(e);
    };
    stack.push(path.into());
    for spec in &raw.extends {
        if !spec.starts_with('.') {
            e.issue(path, "package_or_absolute_extends_unsupported");
            continue;
        }
        let Some(mut base) = join_relative(parent(path), spec) else {
            e.issue(path, "extends_outside_project");
            continue;
        };
        if !(base.ends_with(".json") || base.ends_with(".jsonc")) {
            base.push_str(".json");
        }
        let inherited = load(&base, loader, stack, memo)?;
        e.options.extend(inherited.options);
        e.dependencies.extend(inherited.dependencies);
        e.diagnostics.extend(inherited.diagnostics);
    }
    stack.pop();
    for (key, value) in raw.options {
        e.options
            .insert(key, (path.into(), std::sync::Arc::new(value)));
    }
    memo.insert(path.into(), e.clone());
    Ok(e)
}
pub(super) fn build(
    roots: &BTreeSet<String>,
    loader: &mut Loader<'_>,
) -> CcResult<BTreeMap<String, TypeScriptConfig>> {
    let mut memo = BTreeMap::new();
    let mut configs = BTreeMap::new();
    let mut effective_bytes = 0usize;
    for path in roots {
        let raw = loader.load(path)?;
        if raw.error.as_deref() == Some("missing_config") {
            continue;
        }
        let e = load(path, loader, &mut Vec::new(), &mut memo)?;
        let config = finish(path, e);
        effective_bytes = effective_bytes.saturating_add(serde_json::to_vec(&config)?.len());
        if effective_bytes > 16 * 1024 * 1024 {
            return Err(cc_model::CcError::Config(
                "effective_project_config_bytes_limit".into(),
            ));
        }
        configs.insert(path.clone(), config);
    }
    Ok(configs)
}
fn finish(path: &str, e: Effective) -> TypeScriptConfig {
    let mut c = TypeScriptConfig {
        path: path.into(),
        dependencies: e.dependencies,
        diagnostics: e.diagnostics.into_iter().collect(),
        ..Default::default()
    };
    let mut issues = Vec::new();
    if let Some((origin, v)) = e.options.get("baseUrl") {
        c.base_url = v.as_str().and_then(|s| join_relative(parent(origin), s));
        if c.base_url.is_none() {
            issues.push("invalid_or_outside_base_url".to_owned());
        }
    }
    if let Some((origin, v)) = e.options.get("paths") {
        let mut patterns = BTreeMap::new();
        if let Some(obj) = v.as_object().filter(|o| o.len() <= 256) {
            for (pattern, values) in obj {
                let targets = values
                    .as_array()
                    .filter(|a| !a.is_empty() && a.len() <= 32)
                    .and_then(|a| {
                        a.iter()
                            .map(|v| {
                                v.as_str()
                                    .filter(|s| {
                                        !s.is_empty()
                                            && s.len() <= 4096
                                            && s.matches('*').count() <= 1
                                    })
                                    .map(str::to_owned)
                            })
                            .collect::<Option<Vec<_>>>()
                    });
                if pattern.is_empty()
                    || pattern.len() > 4096
                    || pattern.matches('*').count() > 1
                    || targets.is_none()
                {
                    issues.push("invalid_paths_pattern_or_targets".into());
                    continue;
                }
                let targets = targets.unwrap();
                if !pattern.contains('*') && targets.iter().any(|s| s.contains('*')) {
                    issues.push("wildcard_target_without_pattern".into());
                    continue;
                }
                patterns.insert(pattern.clone(), targets);
            }
        } else {
            issues.push("invalid_paths_object_or_limit".into());
        }
        c.paths = Some(AnchoredPaths {
            defined_in: origin.clone(),
            patterns,
        });
    }
    c.module_resolution = if let Some((_, v)) = e.options.get("moduleResolution") {
        v.as_str().unwrap_or("invalid").to_lowercase()
    } else {
        match e
            .options
            .get("module")
            .and_then(|(_, v)| v.as_str())
            .map(str::to_lowercase)
            .as_deref()
        {
            Some("commonjs") => "node10",
            Some("preserve") => "bundler",
            Some("node16" | "node18" | "node20") => "node16",
            Some("nodenext") => "nodenext",
            _ => "local_compat",
        }
        .into()
    };
    if c.module_resolution == "node" {
        c.module_resolution = "node10".into();
    }
    if !matches!(
        c.module_resolution.as_str(),
        "node10" | "bundler" | "local_compat" | "node16" | "nodenext"
    ) {
        issues.push(format!(
            "module_resolution_{}_unsupported_in_p3a",
            c.module_resolution
        ));
    }
    c.conditions = vec!["types".into(), "local_files".into()];
    if let Some((_, v)) = e.options.get("customConditions") {
        match v.as_array().filter(|a| a.len() <= 64).and_then(|a| {
            a.iter()
                .map(|v| {
                    v.as_str()
                        .filter(|s| !s.is_empty() && s.len() <= 128)
                        .map(str::to_owned)
                })
                .collect::<Option<Vec<_>>>()
        }) {
            Some(values) => c.conditions.extend(values),
            None => issues.push("invalid_custom_conditions".into()),
        }
    }
    c.conditions.sort();
    c.conditions.dedup();
    if let Some((_, v)) = e.options.get("resolveJsonModule") {
        match v.as_bool() {
            Some(b) => c.resolve_json_module = b,
            None => issues.push("invalid_resolve_json_module".into()),
        }
    }
    for name in [
        "rootDirs",
        "moduleSuffixes",
        "allowArbitraryExtensions",
        "noResolve",
    ] {
        if let Some((_, value)) = e.options.get(name) {
            if value.as_ref() != &serde_json::Value::Bool(false) {
                issues.push(format!("{name}_unsupported_in_p3a"));
            }
        }
    }
    for reason in issues {
        c.diagnostics.push(ConfigDiagnostic {
            path: path.into(),
            reason,
        });
    }
    c.diagnostics
        .sort_by(|a, b| (&a.path, &a.reason).cmp(&(&b.path, &b.reason)));
    c.diagnostics.dedup();
    c
}
