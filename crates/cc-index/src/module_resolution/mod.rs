//! Pure admitted-catalog resolution. Discovery and filesystem access live elsewhere.
mod go;
mod package;
mod python;
mod rust;
pub mod typescript;
use cc_model::{module_inputs::*, project_model::*, ImportRecord};
pub fn request_key(imp: &ImportRecord) -> String {
    serde_json::to_string(&(&imp.context, &imp.imported_name)).expect("finite import request")
}
pub(super) fn empty(imp: &ImportRecord, mode: &str) -> ModuleResolution {
    ModuleResolution {
        resolved_package: None,
        candidates: vec![],
        request_key: request_key(imp),
        import_string: imp.import_string.clone(),
        resolved_path: None,
        status: ModuleStatus::Unresolved,
        strategy: mode.into(),
        module_resolution: mode.into(),
        conditions: vec![],
        config_dependencies: Default::default(),
        probes: vec![],
        reason: None,
    }
}
pub fn resolve(model: &ProjectModel, file: &str, spec: &str) -> ModuleResolution {
    let imp = ImportRecord {
        file_path: file.into(),
        import_string: spec.into(),
        context: ImportContext {
            syntax: ImportSyntax::Static,
            ..Default::default()
        },
        ..Default::default()
    };
    resolve_import(model, file, &imp)
}
pub fn resolve_import(model: &ProjectModel, file: &str, imp: &ImportRecord) -> ModuleResolution {
    // Reject unbounded syntax before any language-specific expansion or lookup.
    if !cc_model::repo_path::is_canonical_file(file)
        || file.split('/').count() > 256
        || imp.import_string.len() > 4096
        || imp.import_string.is_empty()
        || imp.import_string.chars().any(char::is_control)
        || imp.context.lexical_module.len() > 256
        || imp.context.conditions.len() > 64
        || imp
            .context
            .lexical_module
            .iter()
            .chain(&imp.context.conditions)
            .any(|s| s.len() > 4096 || s.chars().any(char::is_control))
        || imp
            .context
            .lexical_module
            .iter()
            .chain(&imp.context.conditions)
            .fold(0usize, |n, s| n.saturating_add(s.len()))
            > 4096
        || imp
            .imported_name
            .as_ref()
            .is_some_and(|s| s.len() > 4096 || s.chars().any(char::is_control))
    {
        let mut r = empty(
            &ImportRecord {
                import_string: imp.import_string.chars().take(1024).collect(),
                ..Default::default()
            },
            "invalid_request",
        );
        r.status = ModuleStatus::Unsupported;
        let mut digest = blake3::Hasher::new();
        let mut field = |s: &str| {
            digest.update(&(s.len() as u64).to_le_bytes());
            digest.update(s.as_bytes());
        };
        field("rejected-module-request-v1");
        field(file);
        field(&imp.import_string);
        field(&serde_json::to_string(&imp.context.syntax).expect("finite syntax"));
        field(if imp.imported_name.is_some() {
            "some"
        } else {
            "none"
        });
        if let Some(name) = &imp.imported_name {
            field(name);
        }
        for list in [&imp.context.lexical_module, &imp.context.conditions] {
            field(&list.len().to_string());
            for s in list {
                field(s);
            }
        }
        r.request_key = format!("rejected:{}", digest.finalize().to_hex());
        r.reason = Some("module_request_bounds_exceeded;import_string_is_a_bounded_preview".into());
        return r;
    }
    if file.ends_with(".go") {
        return go::resolve(model, file, imp);
    }
    if file.ends_with(".py") {
        return python::resolve(model, file, imp);
    }
    if file.ends_with(".rs") && !model.rust().crates.is_empty() {
        return rust::resolve(model, file, imp);
    }
    if is_jsts(file) {
        let default = TypeScriptConfig {
            module_resolution: "local_compat".into(),
            ..Default::default()
        };
        let c = model.nearest_config(file).unwrap_or(&default);
        let mut r = typescript::resolve(model.files(), c, &imp.import_string, file);
        r.request_key = request_key(imp);
        if !c.diagnostics.is_empty() {
            return r;
        }
        r.conditions = match package::conditions(model, file, c, imp) {
            Ok(v) => v,
            Err(e) => {
                r.resolved_path = None;
                r.status = ModuleStatus::Unsupported;
                r.reason = Some(e);
                return r;
            }
        };
        if let Some((owner, _)) = package::nearest_package(model, file) {
            r.config_dependencies.insert(owner.into());
        }
        if matches!(c.module_resolution.as_str(), "node16" | "nodenext")
            && r.conditions.iter().any(|c| c == "import")
            && imp.import_string.starts_with('.')
        {
            let leaf = imp.import_string.rsplit('/').next().unwrap_or("");
            if !leaf.contains('.') || imp.import_string.ends_with('/') {
                r.resolved_path = None;
                r.status = ModuleStatus::Unresolved;
                r.reason = Some("node_esm_relative_extension_required".into());
                return r;
            }
        }
        if imp.import_string.starts_with('.') {
            return package::directory(model, file, imp, r);
        }
        if !imp.import_string.starts_with('.')
            && !imp.import_string.starts_with('/')
            && r.resolved_path.is_none()
            && r.reason.as_deref() != Some("equal_priority_paths_patterns_unsupported")
        {
            r.status = ModuleStatus::Unresolved;
            r.reason = None;
            return package::resolve(model, file, imp, c, r);
        }
        return r;
    }
    let mut r = empty(imp, "unsupported_language_module_rules");
    r.status = ModuleStatus::Unsupported;
    r.reason = Some("language_module_rules_not_modeled".into());
    r
}
