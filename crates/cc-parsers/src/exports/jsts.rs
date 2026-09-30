//! ES/type exports and a bounded, explicitly static CommonJS subset.
//! Binding resolution here is file-local; project/module resolution stays in
//! cc-index. Dynamic assignments, escaped specifiers and missing local bindings
//! produce Unknown rather than an apparently complete empty export set.
use super::{add_entry, field, is_comment, is_function, literal, named, text};
use cc_model::{
    public_surface::{PublicSurface, SurfaceForward, VisibilityDomain},
    ImportRecord, Language,
};
use std::collections::{BTreeMap, BTreeSet};
use tree_sitter::{Node, Tree};

#[derive(Clone)]
enum Binding<'a> {
    Declaration(Node<'a>),
    Forward {
        source: String,
        name: String,
        type_only: bool,
    },
}
type Bindings<'a> = BTreeMap<String, Binding<'a>>;

pub(crate) fn extract(
    tree: &Tree,
    source: &[u8],
    path: &str,
    language: Language,
    imports: &mut [ImportRecord],
) -> PublicSurface {
    let mut out = PublicSurface::new(language.as_str(), path, "jsts-declared-v3");
    if tree.root_node().has_error() {
        out.mark_unknown("syntax_error");
    }
    let nodes = named(tree.root_node());
    let mut bindings = Bindings::new();
    let mut module = path.ends_with(".mjs") || path.ends_with(".mts");
    for &node in &nodes {
        match node.kind() {
            "import_statement" => {
                module = true;
                collect_import(node, source, &mut bindings, &mut out);
            }
            "export_statement" => {
                module = true;
                if let Some(d) = node.child_by_field_name("declaration") {
                    collect_declarations(d, source, &mut bindings, &mut out);
                }
            }
            _ => {
                collect_declarations(node, source, &mut bindings, &mut out);
            }
        }
    }
    let shadowed = ["module", "exports", "require"]
        .iter()
        .any(|name| bindings.contains_key(*name));
    let mut cjs_names = BTreeSet::new();
    let mut module_replaced = false;
    for node in nodes {
        if is_comment(node) || node.kind() == "import_statement" {
            continue;
        }
        if node.kind() == "ambient_declaration" {
            out.mark_unknown("jsts_ambient_augmentation_not_modeled");
            continue;
        }
        if node.kind() == "export_statement" {
            export(node, source, &bindings, &mut out);
            continue;
        }
        let expr = if node.kind() == "expression_statement" {
            node.named_child(0)
        } else {
            None
        };
        if let Some(expr) = expr {
            if expr.kind() == "assignment_expression" {
                let left = expr.child_by_field_name("left");
                let target = left.and_then(|n| member_parts(n, source));
                let is_object = target
                    .as_ref()
                    .is_some_and(|p| p.as_slice() == ["module", "exports"]);
                let key = target.as_ref().and_then(|p| match p.as_slice() {
                    [a, b] if a == "exports" => Some(b.clone()),
                    [a, b, c] if a == "module" && b == "exports" => Some(c.clone()),
                    _ => None,
                });
                if is_object || key.is_some() {
                    module = true;
                    // `exports` aliases the original object, not subsequent
                    // replacements of module.exports. Do not merge these objects.
                    if module_replaced
                        && target
                            .as_ref()
                            .is_some_and(|p| p.first().is_some_and(|v| v == "exports"))
                    {
                        out.mark_unknown("commonjs_detached_exports_alias");
                        continue;
                    }
                    if shadowed {
                        out.mark_unknown("commonjs_bindings_shadowed");
                        continue;
                    }
                    let Some(value) = expr.child_by_field_name("right") else {
                        out.mark_unknown("commonjs_missing_value");
                        continue;
                    };
                    if is_object {
                        module_replaced = true;
                        if !cjs_names.is_empty() {
                            out.mark_unknown("commonjs_object_reassigned");
                        }
                        if value.kind() == "object" {
                            for property in named(value) {
                                if is_comment(property) {
                                    continue;
                                }
                                let pair = match property.kind() {
                                    "pair" => property
                                        .child_by_field_name("key")
                                        .and_then(|key| simple_name(key, source))
                                        .zip(property.child_by_field_name("value")),
                                    "shorthand_property_identifier" => {
                                        Some((text(property, source).into(), property))
                                    }
                                    "method_definition" => property
                                        .child_by_field_name("name")
                                        .and_then(|n| simple_name(n, source))
                                        .map(|n| (n, property)),
                                    _ => None,
                                };
                                if let Some((key, value)) = pair {
                                    commonjs_value(
                                        &key,
                                        value,
                                        source,
                                        &bindings,
                                        &mut cjs_names,
                                        &mut out,
                                    );
                                } else {
                                    out.mark_unknown("commonjs_dynamic_object_member");
                                }
                            }
                        } else if let Some(module_source) = require_source(value, source) {
                            out.forwards.push(SurfaceForward {
                                source: module_source,
                                imported_name: "*".into(),
                                exported_name: "*".into(),
                                visibility: VisibilityDomain::Exported,
                                type_only: false,
                            });
                            out.mark_unknown("commonjs_star_requires_resolution");
                        } else {
                            commonjs_value(
                                "default",
                                value,
                                source,
                                &bindings,
                                &mut cjs_names,
                                &mut out,
                            );
                        }
                    } else if let Some(key) = key {
                        commonjs_value(&key, value, source, &bindings, &mut cjs_names, &mut out);
                    }
                    continue;
                }
            }
        }
        // Includes nested/conditional writes, Object.assign/defineProperty and
        // runtime mutations inside functions; static root assignments above are
        // the only forms this extractor certifies.
        if contains_export_reference(node, source) {
            out.mark_unknown("dynamic_commonjs_or_export_mutation");
        }
        if matches!(
            node.kind(),
            "expression_statement"
                | "if_statement"
                | "for_statement"
                | "for_in_statement"
                | "while_statement"
                | "try_statement"
                | "switch_statement"
        ) && !expr.is_some_and(|e| matches!(e.kind(), "string" | "number"))
        {
            out.mark_unknown("jsts_module_runtime_effects");
        }
    }
    include_local_dependencies(source, &bindings, &mut out);
    if !module {
        out.mark_unknown("script_global_scope_or_module_mode_unknown");
    }
    super::mark_forwards(&out, imports);
    out.normalize();
    out
}

fn simple_name(node: Node<'_>, source: &[u8]) -> Option<String> {
    match node.kind() {
        "identifier"
        | "type_identifier"
        | "property_identifier"
        | "shorthand_property_identifier"
        | "shorthand_property_identifier_pattern" => Some(text(node, source).into()),
        "string" => literal(node, source),
        _ => None,
    }
}
fn word(node: Node<'_>, keyword: &str) -> bool {
    let mut cursor = node.walk();
    let found = node.children(&mut cursor).any(|n| n.kind() == keyword);
    found
}
fn collect_declarations<'a>(
    node: Node<'a>,
    source: &[u8],
    bindings: &mut Bindings<'a>,
    out: &mut PublicSurface,
) {
    if matches!(node.kind(), "lexical_declaration" | "variable_declaration") {
        for declarator in named(node)
            .into_iter()
            .filter(|n| n.kind() == "variable_declarator")
        {
            let Some(name) = declarator.child_by_field_name("name") else {
                continue;
            };
            if let Some(module_source) = declarator
                .child_by_field_name("value")
                .and_then(|n| require_source(n, source))
            {
                if let Some(local) = simple_name(name, source) {
                    bind(
                        bindings,
                        local,
                        Binding::Forward {
                            source: module_source,
                            name: "*".into(),
                            type_only: false,
                        },
                        out,
                    );
                } else if name.kind() == "object_pattern" {
                    for p in named(name) {
                        let pair = if p.kind() == "pair_pattern" {
                            p.child_by_field_name("key")
                                .and_then(|n| simple_name(n, source))
                                .zip(
                                    p.child_by_field_name("value")
                                        .and_then(|n| simple_name(n, source)),
                                )
                        } else {
                            simple_name(p, source).map(|s| (s.clone(), s))
                        };
                        if let Some((imported, local)) = pair {
                            bind(
                                bindings,
                                local,
                                Binding::Forward {
                                    source: module_source.clone(),
                                    name: imported,
                                    type_only: false,
                                },
                                out,
                            );
                        }
                    }
                }
            } else if let Some(name) = simple_name(name, source) {
                bind(bindings, name, Binding::Declaration(declarator), out);
            }
        }
    } else if matches!(
        node.kind(),
        "function_declaration"
            | "generator_function_declaration"
            | "class_declaration"
            | "abstract_class_declaration"
            | "type_alias_declaration"
            | "interface_declaration"
            | "enum_declaration"
            | "internal_module"
            | "function_signature"
    ) {
        let name = field(node, "name", source);
        if !name.is_empty() {
            bind(bindings, name.into(), Binding::Declaration(node), out);
        }
    }
}
fn collect_import<'a>(
    node: Node<'a>,
    source: &[u8],
    bindings: &mut Bindings<'a>,
    out: &mut PublicSurface,
) {
    let Some(module) = node
        .child_by_field_name("source")
        .and_then(|n| literal(n, source))
    else {
        out.mark_unknown("import_specifier_not_static");
        return;
    };
    let type_only = word(node, "type");
    for clause in named(node)
        .into_iter()
        .filter(|n| n.kind() == "import_clause")
    {
        for child in named(clause) {
            match child.kind() {
                "identifier" => {
                    bind(
                        bindings,
                        text(child, source).into(),
                        Binding::Forward {
                            source: module.clone(),
                            name: "default".into(),
                            type_only,
                        },
                        out,
                    );
                }
                "namespace_import" => {
                    if let Some(name) = child.named_child(0).and_then(|n| simple_name(n, source)) {
                        bind(
                            bindings,
                            name,
                            Binding::Forward {
                                source: module.clone(),
                                name: "*".into(),
                                type_only,
                            },
                            out,
                        );
                    }
                }
                "named_imports" => {
                    for spec in named(child)
                        .into_iter()
                        .filter(|n| n.kind() == "import_specifier")
                    {
                        let imported = spec
                            .child_by_field_name("name")
                            .and_then(|n| simple_name(n, source));
                        if let Some(imported) = imported {
                            let alias = alias_or_name(spec, &imported, source, out);
                            bind(
                                bindings,
                                alias,
                                Binding::Forward {
                                    source: module.clone(),
                                    name: imported,
                                    type_only: type_only || word(spec, "type"),
                                },
                                out,
                            );
                        } else {
                            out.mark_unknown("escaped_import_binding");
                        }
                    }
                }
                _ => {
                    out.mark_unknown("import_binding_not_modeled");
                }
            }
        }
    }
}
/// Ordinary imports and public-surface forwarding share the AST binding
/// extractor. A statement is not a symbol name; each binding gets a record.
pub(crate) fn ordinary_import_records(
    node: Node<'_>,
    source: &[u8],
    path: &str,
) -> Vec<ImportRecord> {
    let mut bindings = Bindings::new();
    let mut diagnostics = PublicSurface::new("jsts", path, "import-bindings-v1");
    collect_import(node, source, &mut bindings, &mut diagnostics);
    let mut result = Vec::new();
    for (local, binding) in bindings {
        if let Binding::Forward {
            source,
            name,
            type_only,
        } = binding
        {
            result.push(ImportRecord {
                context: cc_model::module_inputs::ImportContext {
                    syntax: if type_only {
                        cc_model::module_inputs::ImportSyntax::TypeOnly
                    } else {
                        cc_model::module_inputs::ImportSyntax::Static
                    },
                    ..Default::default()
                },
                file_path: path.into(),
                import_string: source,
                resolved_path: None,
                is_namespace: name == "*",
                is_default: name == "default",
                is_reexport: false,
                alias: (local != name).then_some(local),
                imported_name: Some(name),
            });
        }
    }
    if result.is_empty() {
        if let Some(module) = node
            .child_by_field_name("source")
            .and_then(|n| literal(n, source))
        {
            result.push(ImportRecord {
                context: cc_model::module_inputs::ImportContext {
                    syntax: cc_model::module_inputs::ImportSyntax::Static,
                    ..Default::default()
                },
                file_path: path.into(),
                import_string: module,
                resolved_path: None,
                imported_name: None,
                alias: None,
                is_namespace: false,
                is_default: false,
                is_reexport: false,
            });
        }
    }
    result
}

fn emit_binding(
    name: &str,
    exported: &str,
    type_only: bool,
    node: Node<'_>,
    source: &[u8],
    bindings: &Bindings<'_>,
    out: &mut PublicSurface,
) {
    match bindings.get(name) {
        Some(Binding::Forward {
            source,
            name,
            type_only: import_type,
        }) => {
            out.forwards.push(SurfaceForward {
                source: source.clone(),
                imported_name: name.clone(),
                exported_name: exported.into(),
                visibility: VisibilityDomain::Exported,
                type_only: type_only || *import_type,
            });
        }
        Some(Binding::Declaration(declaration)) => {
            emit_declaration(*declaration, name, exported, type_only, source, out)
        }
        None => {
            add_entry(
                out,
                node,
                source,
                name,
                exported,
                VisibilityDomain::Exported,
                false,
            );
            out.mark_unknown("exported_local_binding_not_resolved");
        }
    }
}
fn emit_declaration(
    node: Node<'_>,
    name: &str,
    exported: &str,
    type_only: bool,
    source: &[u8],
    out: &mut PublicSurface,
) {
    add_entry(
        out,
        node,
        source,
        name,
        exported,
        VisibilityDomain::Exported,
        false,
    );
    // The declarator node omits its parent declaration keyword. Const/let/var
    // are part of the interface (including literal-type inference), not trivia.
    if node.kind() == "variable_declarator" {
        let qualifier = node
            .parent()
            .and_then(|p| ["const", "let", "var"].into_iter().find(|k| word(p, k)));
        if let Some(kind) = qualifier {
            out.entries.last_mut().unwrap().signature.insert(
                0,
                cc_model::public_surface::SurfaceToken {
                    kind: "binding_qualifier".into(),
                    text: kind.into(),
                },
            );
        } else {
            out.mark_unknown("jsts_binding_qualifier_not_modeled");
        }
    }
    if type_only {
        out.entries
            .last_mut()
            .unwrap()
            .kind
            .insert_str(0, "type_export:");
    }
    // An omitted return annotation can depend on executable code. Do not call
    // a syntactically identical header proof that TypeScript's inferred API is
    // unchanged. Known declarations with explicit return types still skip bodies.
    let mut stack = vec![node];
    let mut visited = 0;
    while let Some(n) = stack.pop() {
        visited += 1;
        if visited > 100_000 {
            out.mark_unknown("surface_syntax_budget_exceeded");
            break;
        }
        if is_function(n) {
            if n.child_by_field_name("return_type").is_none() {
                out.mark_unknown("jsts_inferred_return_type_not_evaluated");
            }
            // Defaults, decorators and annotations are not executable bodies.
            // They can depend on evaluation even with an explicit return type.
            let body = n.child_by_field_name("body").map(|b| b.id());
            stack.extend(named(n).into_iter().filter(|c| Some(c.id()) != body));
            continue;
        }
        if n.kind() == "computed_property_name" {
            out.mark_unknown("jsts_computed_property_name_not_evaluated");
        }
        if matches!(
            n.kind(),
            "decorator" | "class_static_block" | "call_expression" | "new_expression"
        ) {
            out.mark_unknown("jsts_dynamic_declaration_shape");
        }
        stack.extend(named(n));
    }
}
fn export(node: Node<'_>, source: &[u8], bindings: &Bindings<'_>, out: &mut PublicSurface) {
    let default = word(node, "default");
    let type_only = word(node, "type");
    if let Some(module_node) = node.child_by_field_name("source") {
        let Some(module) = literal(module_node, source) else {
            out.mark_unknown("export_specifier_not_static");
            return;
        };
        if let Some(clause) = named(node)
            .into_iter()
            .find(|n| n.kind() == "export_clause")
        {
            for spec in named(clause)
                .into_iter()
                .filter(|n| n.kind() == "export_specifier")
            {
                if let Some(name) = spec
                    .child_by_field_name("name")
                    .and_then(|n| simple_name(n, source))
                {
                    let alias = alias_or_name(spec, &name, source, out);
                    out.forwards.push(SurfaceForward {
                        source: module.clone(),
                        imported_name: name,
                        exported_name: alias,
                        visibility: VisibilityDomain::Exported,
                        type_only: type_only || word(spec, "type"),
                    });
                } else {
                    out.mark_unknown("escaped_export_binding");
                }
            }
        } else {
            let namespace = match named(node)
                .into_iter()
                .find(|n| n.kind() == "namespace_export")
            {
                Some(ns) => match ns.named_child(0).and_then(|n| simple_name(n, source)) {
                    Some(name) => name,
                    None => {
                        out.mark_unknown("escaped_namespace_export");
                        String::new()
                    }
                },
                None => "*".into(),
            };
            out.forwards.push(SurfaceForward {
                source: module,
                imported_name: "*".into(),
                exported_name: namespace,
                visibility: VisibilityDomain::Exported,
                type_only,
            });
        }
    } else if let Some(declaration) = node.child_by_field_name("declaration") {
        if matches!(
            declaration.kind(),
            "lexical_declaration" | "variable_declaration"
        ) {
            for d in named(declaration)
                .into_iter()
                .filter(|n| n.kind() == "variable_declarator")
            {
                if let Some(name) = d
                    .child_by_field_name("name")
                    .and_then(|n| simple_name(n, source))
                {
                    emit_declaration(d, &name, &name, type_only, source, out);
                } else {
                    out.mark_unknown("exported_destructuring_not_modeled");
                }
            }
        } else {
            let name = field(declaration, "name", source);
            emit_declaration(
                declaration,
                name,
                if default { "default" } else { name },
                type_only,
                source,
                out,
            );
        }
    } else if let Some(clause) = named(node)
        .into_iter()
        .find(|n| n.kind() == "export_clause")
    {
        for spec in named(clause)
            .into_iter()
            .filter(|n| n.kind() == "export_specifier")
        {
            if let Some(name) = spec
                .child_by_field_name("name")
                .and_then(|n| simple_name(n, source))
            {
                let alias = alias_or_name(spec, &name, source, out);
                emit_binding(
                    &name,
                    &alias,
                    type_only || word(spec, "type"),
                    spec,
                    source,
                    bindings,
                    out,
                );
            } else {
                out.mark_unknown("escaped_export_binding");
            }
        }
    } else if let Some(value) = node.child_by_field_name("value") {
        if value.kind() == "identifier" {
            emit_binding(
                text(value, source),
                "default",
                false,
                value,
                source,
                bindings,
                out,
            );
        } else {
            emit_declaration(value, "<default>", "default", false, source, out);
        }
    } else {
        out.mark_unknown("export_form_not_modeled");
    }
}
fn require_source(node: Node<'_>, source: &[u8]) -> Option<String> {
    if node.kind() != "call_expression" || field(node, "function", source) != "require" {
        return None;
    }
    let args = node.child_by_field_name("arguments")?;
    let args = named(args);
    if args.len() != 1 {
        return None;
    }
    literal(args[0], source)
}
fn member_parts(mut node: Node<'_>, source: &[u8]) -> Option<Vec<String>> {
    let mut parts = Vec::new();
    for _ in 0..8 {
        if node.kind() == "identifier" {
            parts.push(text(node, source).into());
            parts.reverse();
            return Some(parts);
        }
        let property = match node.kind() {
            "member_expression" => simple_name(node.child_by_field_name("property")?, source),
            "subscript_expression" => literal(node.child_by_field_name("index")?, source),
            _ => return None,
        }?;
        parts.push(property);
        node = node.child_by_field_name("object")?;
    }
    None
}
fn bind<'a>(
    bindings: &mut Bindings<'a>,
    name: String,
    value: Binding<'a>,
    out: &mut PublicSurface,
) {
    if bindings.insert(name, value).is_some() {
        out.mark_unknown("duplicate_or_overloaded_binding");
    }
}
fn alias_or_name(node: Node<'_>, name: &str, source: &[u8], out: &mut PublicSurface) -> String {
    match node.child_by_field_name("alias") {
        None => name.into(),
        Some(n) => match simple_name(n, source) {
            Some(alias) => alias,
            None => {
                out.mark_unknown("escaped_export_alias");
                String::new()
            }
        },
    }
}
/// Reachable local declaration dependencies are interface evidence, not new
/// exports. A bounded worklist avoids recursive upstream hashing and cycles.
fn include_local_dependencies(source: &[u8], bindings: &Bindings<'_>, out: &mut PublicSurface) {
    let mut visited: BTreeSet<String> = out
        .entries
        .iter()
        .map(|e| e.qualified_name.clone())
        .collect();
    let mut cursor = 0;
    while cursor < out.entries.len() {
        if visited.len() > 1024 {
            out.mark_unknown("local_surface_dependency_limit");
            break;
        }
        let dependencies: Vec<_> = out.entries[cursor]
            .signature
            .iter()
            .filter(|t| matches!(t.kind.as_str(), "identifier" | "type_identifier"))
            .map(|t| t.text.clone())
            .collect();
        cursor += 1;
        for name in dependencies {
            if visited.contains(&name) {
                continue;
            }
            let Some(binding) = bindings.get(&name) else {
                continue;
            };
            visited.insert(name.clone());
            match binding {
                Binding::Declaration(node) => {
                    emit_declaration(*node, &name, "", false, source, out);
                    let e = out.entries.last_mut().unwrap();
                    e.visibility = VisibilityDomain::Module;
                    e.kind.insert_str(0, "local_dependency:");
                }
                Binding::Forward { .. } => {
                    out.mark_unknown("imported_interface_dependency_requires_resolution");
                }
            }
        }
    }
}
fn commonjs_value(
    key: &str,
    value: Node<'_>,
    source: &[u8],
    bindings: &Bindings<'_>,
    seen: &mut BTreeSet<String>,
    out: &mut PublicSurface,
) {
    if !seen.insert(key.into()) {
        out.mark_unknown("commonjs_duplicate_assignment");
    }
    if matches!(value.kind(), "identifier" | "shorthand_property_identifier") {
        emit_binding(
            text(value, source),
            key,
            false,
            value,
            source,
            bindings,
            out,
        );
    } else {
        emit_declaration(value, key, key, false, source, out);
    }
}
fn contains_export_reference(node: Node<'_>, source: &[u8]) -> bool {
    let mut stack = vec![node];
    let mut count = 0;
    while let Some(n) = stack.pop() {
        count += 1;
        if count > 100_000 {
            return true;
        }
        if n.kind() == "identifier" && text(n, source) == "exports" {
            return true;
        }
        if matches!(n.kind(), "member_expression" | "subscript_expression")
            && member_parts(n, source)
                .is_some_and(|p| p.starts_with(&["module".into(), "exports".into()]))
        {
            return true;
        }
        stack.extend(named(n));
    }
    false
}
