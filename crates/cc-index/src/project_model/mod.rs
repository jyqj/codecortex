//! Discover bounded project inputs once, then hand immutable data to resolution.
mod config_cache;
mod go;
mod go_capture;
mod jsonc;
pub(crate) mod package;
mod python;
pub(crate) mod rust;
mod rust_capture;
mod source_capture;
mod typescript;
use crate::{
    indexer::{BuildScope, ScanDiffResult},
    scanner::WalkManifest,
};
use cc_db::index_db::IndexDb;
use cc_model::{project_model::*, resolution::*, CcError, CcResult, ParseOutcome};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

/// Conservative watcher admission for JSON configuration changes. It does not
/// admit those files as indexed source and never reads their contents here.
pub fn potential_config_path(path: &str) -> bool {
    cc_model::repo_path::is_canonical_file(path)
        && (path.ends_with(".json")
            || path.ends_with(".jsonc")
            || matches!(
                path.rsplit('/').next(),
                Some("Cargo.toml" | "pyproject.toml" | "go.mod" | "go.work")
            ))
        && !path.split('/').any(|s| {
            matches!(
                s,
                ".git"
                    | "target"
                    | "node_modules"
                    | "vendor"
                    | "dist"
                    | "build"
                    | "__pycache__"
                    | ".mypy_cache"
                    | ".pytest_cache"
                    | ".tox"
                    | ".venv"
                    | "venv"
                    | ".eggs"
                    | ".idea"
                    | ".vscode"
                    | ".cache"
            ) || s.starts_with(".codecortex")
        })
}
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredInputs {
    digest: String,
    inputs: ProjectInputs,
}
pub fn read_inputs(db: &IndexDb) -> CcResult<ProjectInputs> {
    match db.reads().get_metadata(PROJECT_INPUT_KEY)? {
        Some(text) => {
            if text.len() > 16 * 1024 * 1024 + 256 {
                return Err(CcError::Config("project input snapshot too large".into()));
            }
            let stored: StoredInputs = serde_json::from_str(&text).map_err(|_| {
                CcError::Config("corrupt project input snapshot; full rebuild required".into())
            })?;
            if stored.inputs.digest()? != stored.digest {
                return Err(CcError::Config(
                    "project input snapshot digest mismatch; full rebuild required".into(),
                ));
            }
            Ok(stored.inputs)
        }
        None => Ok(ProjectInputs::default()),
    }
}
/// Borrowed input view shared by the full-snapshot strategies. It carries no
/// mutable service or filesystem resolver into the snapshot writer.
#[derive(Clone, Copy)]
pub(crate) struct BuildInputView<'a> {
    pub walk_manifest: Option<&'a WalkManifest>,
    pub project_model: Option<&'a CapturedProject>,
}

pub struct CapturedProject {
    legacy_config_inputs_unchanged: bool,
    model: ProjectModel,
    inputs: ProjectInputs,
    report: ProjectModelReport,
}
impl CapturedProject {
    pub(crate) fn legacy_config_inputs_unchanged(&self) -> bool {
        self.legacy_config_inputs_unchanged
    }
    pub fn model(&self) -> &ProjectModel {
        &self.model
    }
    pub fn report(&self) -> &ProjectModelReport {
        &self.report
    }
    pub fn inputs(&self) -> &ProjectInputs {
        &self.inputs
    }
    pub fn verify(&self, root: &Path) -> CcResult<()> {
        config_cache::verify(root, &self.inputs)
    }
    /// Acknowledgement follows successful facts + durable-frontier publication.
    /// A crash before acknowledgement repeats invalidation, never loses it.
    pub(crate) fn acknowledge(&self, db: &IndexDb) -> CcResult<()> {
        let value = serde_json::to_string(&StoredInputs {
            digest: self.inputs.digest()?,
            inputs: self.inputs.clone(),
        })?;
        if db.reads().get_metadata(PROJECT_INPUT_KEY)?.as_deref() != Some(&value) {
            db.writes().set_metadata(PROJECT_INPUT_KEY, &value)?;
        }
        Ok(())
    }
    pub(crate) fn capture(
        indexer: &crate::Indexer,
        scan: &ScanDiffResult,
        scope: Option<&BuildScope>,
        full: bool,
    ) -> CcResult<Self> {
        let old = if full {
            ProjectInputs::default()
        } else {
            read_inputs(&indexer.db)?
        };
        discover_inner(
            indexer.scanner.project_path(),
            scan.scanned_paths.iter().cloned().collect(),
            scan.walk_manifest.as_deref(),
            scope,
            &old,
            Some(scan),
        )
    }
    pub(crate) fn apply_imports(&self, file: &str, outcome: &mut ParseOutcome) {
        let mut blocked = BTreeSet::new();
        let mut type_only = BTreeSet::new();
        for import in &mut outcome.imports {
            let r = crate::module_resolution::resolve_import(&self.model, file, import);
            import.resolved_path = r.resolved_path;
            if matches!(
                import.context.syntax,
                cc_model::module_inputs::ImportSyntax::TypeOnly
                    | cc_model::module_inputs::ImportSyntax::TypeOnlyRequire
            ) {
                if let Some(local) = import.alias.as_ref().or(import.imported_name.as_ref()) {
                    type_only.insert(local.clone());
                }
            }
            if (is_jsts(file)
                || file.ends_with(".go")
                || file.ends_with(".py")
                || (file.ends_with(".rs") && !self.model.rust().crates.is_empty()))
                && r.status != ModuleStatus::Resolved
            {
                if let Some(local) = import.alias.as_ref().or(import.imported_name.as_ref()) {
                    blocked.insert(local.clone());
                }
            }
        }
        let blocked_name = |name: &str| blocked.contains(name.split('.').next().unwrap_or(name));
        for call in &mut outcome.call_edges {
            if (blocked_name(&call.callee_symbol)
                || (file.ends_with(".go")
                    && call.receiver_expr.as_deref().is_some_and(blocked_name))
                || type_only.contains(
                    call.callee_symbol
                        .split('.')
                        .next()
                        .unwrap_or(&call.callee_symbol),
                ))
                && call.resolution_strategy != PARSER_UNSUPPORTED_BINDING
            {
                call.target_symbol_id = None;
                call.callee_symbol_uid = None;
                call.target_file_path = None;
                call.resolution_kind = cc_model::ResolutionKind::Unresolved;
                call.resolution_strategy = MODULE_BLOCKED_BINDING.into();
                call.resolution_confidence = 0.0;
            }
        }
        for sref in &mut outcome.symbol_refs {
            if blocked_name(sref.ref_name.as_deref().unwrap_or(&sref.symbol_name))
                && sref.resolution_strategy != PARSER_UNSUPPORTED_BINDING
            {
                sref.target_symbol_id = None;
                sref.target_symbol_uid = None;
                sref.target_file_path = None;
                sref.resolution_kind = cc_model::ResolutionKind::Unresolved;
                sref.resolution_strategy = MODULE_BLOCKED_BINDING.into();
                sref.resolution_confidence = 0.0;
            }
        }
    }
    pub(crate) fn seal_modules(&self, file: &str, outcome: &mut ParseOutcome) {
        let mut seen = BTreeSet::new();
        for import in &outcome.imports {
            if !seen.insert((
                &import.import_string,
                crate::module_resolution::request_key(import),
            )) {
                continue;
            }
            let r = crate::module_resolution::resolve_import(&self.model, file, import);
            for config in &r.config_dependencies {
                outcome
                    .resolution
                    .dependency(DependencyKind::ModuleConfig, config.clone());
            }
            for path in &r.probes {
                outcome
                    .resolution
                    .dependency(DependencyKind::MissingPath, path.clone());
            }
            if let Some(package) = &r.resolved_package {
                for path in &package.files {
                    outcome
                        .resolution
                        .dependency(DependencyKind::TargetSurface, path.clone());
                }
            }
            outcome.resolution.record_module(r);
        }
        // Proven local Go imports record every current manifest and missing ancestor
        // boundary. Keep the global fallback for unknown/external/budget-limited cases.
        if file.ends_with(".go")
            && outcome.resolution.complete
            && outcome.public_surface.conditions.is_empty()
            && outcome
                .resolution
                .modules
                .iter()
                .all(|m| m.status == ModuleStatus::Resolved && m.resolved_package.is_some())
        {
            outcome.resolution.dependencies.remove(
                &cc_model::resolution::ResolutionDependency::new(DependencyKind::ModuleConfig, "*"),
            );
        }
        outcome.resolution.normalize();
    }
}
/// `manifest` is the existing shared walk. Scoped builds reuse committed root
/// discovery and add explicit config events; source inventory comes from the
/// already reconciled scan, not from another repository walk.
pub fn discover(
    root: &Path,
    files: BTreeSet<String>,
    manifest: Option<&WalkManifest>,
    scope: Option<&BuildScope>,
    old: &ProjectInputs,
) -> CcResult<CapturedProject> {
    discover_inner(root, files, manifest, scope, old, None)
}
fn discover_inner(
    root: &Path,
    files: BTreeSet<String>,
    manifest: Option<&WalkManifest>,
    scope: Option<&BuildScope>,
    old: &ProjectInputs,
    scan: Option<&ScanDiffResult>,
) -> CcResult<CapturedProject> {
    old.validate()?;
    let catalog = FileCatalog::new(files)?;
    let mut dirs = BTreeSet::new();
    for file in catalog.files().iter().filter(|p| is_jsts(p)) {
        let mut p = parent(file);
        loop {
            dirs.insert(p.to_string());
            if p.is_empty() {
                break;
            }
            p = parent(p);
        }
    }
    let mut roots = if let Some(walk) = manifest {
        walk.files
            .iter()
            .filter(|f| {
                is_config_root(&f.rel_path)
                    && potential_config_path(&f.rel_path)
                    && dirs.contains(parent(&f.rel_path))
            })
            .map(|f| f.rel_path.clone())
            .collect()
    } else {
        old.roots.clone()
    };
    if let Some(scope) = scope {
        for p in scope.changed.iter().chain(&scope.removed) {
            if is_config_root(p) && potential_config_path(p) && dirs.contains(parent(p)) {
                roots.insert(p.clone());
            }
        }
    }
    // Roots that no longer own admitted JS/TS files do not retain stale inputs.
    roots.retain(|p| dirs.contains(parent(p)));
    let mut packages = if manifest.is_none() {
        old.package_roots.clone()
    } else {
        BTreeMap::new()
    };
    if let Some(walk) = manifest {
        for file in &walk.files {
            if file
                .rel_path
                .split('/')
                .any(|p| matches!(p, "node_modules" | "target" | ".git"))
            {
                continue;
            }
            let kind = match file.rel_path.rsplit('/').next() {
                Some("package.json") => "javascript",
                Some("Cargo.toml") => "rust",
                Some("go.mod" | "go.work") => "go",
                Some("pyproject.toml") => "python",
                _ => continue,
            };
            packages.insert(file.rel_path.clone(), kind.into());
        }
    }
    // Named config roots are semantic inputs even when gitignored as source.
    // Probe only ancestor directories of admitted JS/TS files; no second walk.
    let mut root_probes = 0usize;
    if manifest.is_some() {
        for dir in &dirs {
            for name in ["tsconfig.json", "jsconfig.json"] {
                let path = if dir.is_empty() {
                    name.into()
                } else {
                    format!("{dir}/{name}")
                };
                if potential_config_path(&path) && !roots.contains(&path) {
                    root_probes += 1;
                    if root.join(&path).symlink_metadata().is_ok() {
                        roots.insert(path);
                    }
                }
            }
        }
    }
    // A scoped source addition may enter a directory never owned by this
    // model. Probe only event ancestors, not every import or repository file.
    if manifest.is_none() {
        if let Some(scope) = scope {
            let mut probed = BTreeSet::new();
            for file in scope
                .changed
                .iter()
                .filter(|p| is_jsts(p) && catalog.contains(p))
            {
                let mut dir = parent(file);
                loop {
                    if probed.insert(dir.to_owned()) {
                        for name in ["tsconfig.json", "jsconfig.json"] {
                            let p = if dir.is_empty() {
                                name.into()
                            } else {
                                format!("{dir}/{name}")
                            };
                            root_probes += 1;
                            if root.join(&p).symlink_metadata().is_ok() {
                                roots.insert(p);
                            }
                        }
                    }
                    if dir.is_empty() {
                        break;
                    }
                    dir = parent(dir);
                }
            }
        }
    }
    let mut loader = config_cache::Loader::new(root, &old.configs);
    let configs = typescript::build(&roots, &mut loader)?;
    roots.retain(|p| configs.contains_key(p));
    // Configuration inventory is shared with the scanner; ancestor probes also
    // admit explicitly ignored manifests without a second filesystem walk.
    let mut manifest_dirs = BTreeSet::new();
    for file in catalog
        .files()
        .iter()
        .filter(|p| is_jsts(p) || p.ends_with(".rs") || p.ends_with(".py") || p.ends_with(".go"))
    {
        if manifest.is_none() && scope.is_some_and(|s| !s.changed.contains(file)) {
            continue;
        }
        let mut dir = parent(file);
        loop {
            manifest_dirs.insert(dir.to_owned());
            if dir.is_empty() {
                break;
            }
            dir = parent(dir);
        }
    }
    if let Some(s) = scope {
        for p in s.changed.iter().chain(&s.removed) {
            if matches!(
                p.rsplit('/').next(),
                Some("package.json" | "Cargo.toml" | "pyproject.toml" | "go.mod" | "go.work")
            ) {
                packages.insert(p.clone(), "module_input".into());
            }
        }
    }
    for dir in manifest_dirs {
        for name in [
            "package.json",
            "Cargo.toml",
            "pyproject.toml",
            "go.mod",
            "go.work",
        ] {
            let p = if dir.is_empty() {
                name.into()
            } else {
                format!("{dir}/{name}")
            };
            if !packages.contains_key(&p) && potential_config_path(&p) {
                root_probes += 1;
                if root.join(&p).symlink_metadata().is_ok() {
                    packages.insert(p, "module_input".into());
                }
            }
        }
    }
    let mut documents = BTreeMap::new();
    for path in packages.keys().filter(|p| potential_config_path(p)) {
        let input = loader.load(path)?;
        if input.error.as_deref() == Some("missing_config") {
            continue;
        }
        documents.insert(
            path.clone(),
            input.parsed.unwrap_or_else(|| {
                cc_model::module_inputs::ConfigDocument::Invalid(
                    input.error.unwrap_or_else(|| "invalid_manifest".into()),
                )
            }),
        );
    }
    packages.retain(|p, _| documents.contains_key(p));
    let reads = loader.reads;
    let hits = loader.hits;
    let mut needed: BTreeSet<_> = configs
        .values()
        .flat_map(|c| c.dependencies.iter().cloned())
        .collect();
    needed.extend(documents.keys().cloned());
    loader.inputs.retain(|p, _| needed.contains(p));
    let (rust_sources, rust_reads, rust_hits) =
        rust_capture::capture(root, &catalog, &old.rust_sources, scan)?;
    let (go_sources, go_reads, go_hits) =
        go_capture::capture(root, &catalog, &old.go_sources, scan)?;
    let go_model = go::build(&documents, &go_sources);
    let (rust_model, aliases) = rust::build(&documents, &catalog, &rust_sources)?;
    let python_model = python::build(&documents);
    let mut groups = BTreeMap::new();
    for (path, doc) in &documents {
        if let cc_model::module_inputs::ConfigDocument::Package(p) = doc {
            if !p.workspaces.is_empty() {
                let base = parent(path);
                let members = documents
                    .keys()
                    .filter(|candidate| candidate.ends_with("/package.json"))
                    .filter(|candidate| {
                        let dir = parent(candidate);
                        let rel = if base.is_empty() {
                            Some(dir)
                        } else {
                            dir.strip_prefix(&format!("{base}/"))
                        };
                        rel.is_some_and(|r| {
                            p.workspaces
                                .iter()
                                .any(|pat| package::member_matches(pat, r))
                        })
                    })
                    .cloned()
                    .collect();
                groups.insert(path.clone(), members);
            }
        }
    }
    let inputs = ProjectInputs {
        rust_sources,
        go_sources,
        version: PROJECT_MODEL_VERSION,
        roots,
        configs: loader.inputs,
        package_roots: packages.clone(),
    };
    let mut changed = BTreeSet::new();
    for path in old.configs.keys().chain(inputs.configs.keys()) {
        if old.configs.get(path) != inputs.configs.get(path) {
            changed.insert(path.clone());
        }
    }
    for path in old.rust_sources.keys().chain(inputs.rust_sources.keys()) {
        if old.rust_sources.get(path).map(|s| &s.facts)
            != inputs.rust_sources.get(path).map(|s| &s.facts)
        {
            changed.insert(path.clone());
        }
    }
    for path in old.go_sources.keys().chain(inputs.go_sources.keys()) {
        if old.go_sources.get(path).map(|s| &s.facts)
            != inputs.go_sources.get(path).map(|s| &s.facts)
        {
            changed.insert(path.clone());
        }
    }
    for path in old.roots.symmetric_difference(&inputs.roots) {
        changed.insert(path.clone());
    }
    let report = ProjectModelReport {
        go_source_reads: go_reads,
        go_fact_cache_hits: go_hits,
        rust_source_reads: rust_reads,
        rust_fact_cache_hits: rust_hits,
        version: PROJECT_MODEL_VERSION,
        input_digest: inputs.digest()?,
        catalog_files: catalog.files().len(),
        config_roots: configs.len(),
        config_inputs: inputs.configs.len(),
        config_reads: reads,
        config_root_probes: root_probes,
        config_parse_cache_hits: hits,
        inventory_source: if manifest.is_some() {
            "shared_walk"
        } else {
            "scoped_catalog"
        }
        .into(),
        changed_configs: changed.into_iter().collect(),
        modes: configs
            .values()
            .map(|c| c.module_resolution.clone())
            .collect(),
        diagnostics: configs
            .values()
            .flat_map(|c| c.diagnostics.iter().cloned())
            .chain(documents.iter().flat_map(|(p, d)| {
                let reasons = match d {
                    cc_model::module_inputs::ConfigDocument::Package(v) => v.diagnostics.clone(),
                    cc_model::module_inputs::ConfigDocument::Go(v) => v.diagnostics.clone(),
                    cc_model::module_inputs::ConfigDocument::Invalid(e) => vec![e.clone()],
                    _ => vec![],
                };
                reasons.into_iter().map(|reason| ConfigDiagnostic {
                    path: p.clone(),
                    reason,
                })
            }))
            .chain(rust_model.crates.iter().flat_map(|(p, c)| {
                c.diagnostics
                    .iter()
                    .cloned()
                    .chain(
                        c.blocked_dependencies
                            .iter()
                            .map(|(name, why)| format!("{name}:{why}")),
                    )
                    .map(|reason| ConfigDiagnostic {
                        path: p.clone(),
                        reason,
                    })
            }))
            .chain(
                rust_model
                    .diagnostics
                    .iter()
                    .cloned()
                    .map(|reason| ConfigDiagnostic {
                        path: "Cargo.toml".into(),
                        reason,
                    }),
            )
            .chain(python_model.diagnostics.iter().flat_map(|(p, es)| {
                es.iter().cloned().map(move |reason| ConfigDiagnostic {
                    path: if p.is_empty() {
                        "pyproject.toml".into()
                    } else {
                        format!("{p}/pyproject.toml")
                    },
                    reason,
                })
            }))
            .take(4096)
            .collect(),
    };
    let legacy_config_inputs_unchanged = scope
        .is_some_and(|scope| !scope.changed.is_empty() || !scope.removed.is_empty())
        && old.configs.keys().eq(inputs.configs.keys())
        && scope.is_some_and(|scope| {
            scope
                .changed
                .iter()
                .chain(&scope.removed)
                .all(|p| inputs.configs.contains_key(p))
        });
    let model = ProjectModel::new(catalog, configs, packages, aliases)
        .with_module_inputs(documents, rust_model, python_model)
        .with_workspaces(groups)
        .with_go(go_model);
    Ok(CapturedProject {
        legacy_config_inputs_unchanged,
        model,
        inputs,
        report,
    })
}
