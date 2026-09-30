//! Python's statically declared module bindings and forwarding routes.
//! __all__ controls star visibility, not whether an explicit binding exists.
use super::{add_entry, field, is_comment, literal, named, text};
use cc_model::{
    public_surface::{PublicSurface, SurfaceForward, VisibilityDomain},
    ImportRecord,
};
use std::collections::BTreeSet;
use tree_sitter::{Node, Tree};

pub(crate) fn extract(
    tree: &Tree,
    source: &[u8],
    path: &str,
    imports: &mut [ImportRecord],
) -> PublicSurface {
    let mut out = PublicSurface::new("python", path, "python-declared-v2");
    if tree.root_node().has_error() {
        out.mark_unknown("syntax_error");
    }
    let mut all: Option<BTreeSet<String>> = None;
    visit(tree.root_node(), source, "", &mut all, 0, &mut out);
    for e in &mut out.entries {
        if !e.qualified_name.contains('.') {
            e.visibility = if all
                .as_ref()
                .map(|a| a.contains(&e.exported_name))
                .unwrap_or(!e.exported_name.starts_with('_'))
            {
                VisibilityDomain::Exported
            } else {
                VisibilityDomain::Module
            };
        }
    }
    for f in &mut out.forwards {
        f.visibility = if all
            .as_ref()
            .map(|a| a.contains(&f.exported_name))
            .unwrap_or(!f.exported_name.starts_with('_'))
        {
            VisibilityDomain::Exported
        } else {
            VisibilityDomain::Module
        };
    }
    let mut bindings = BTreeSet::new();
    for e in &out.entries {
        if !bindings.insert(e.qualified_name.clone()) {
            out.reasons
                .push("python_repeated_binding_order_not_modeled".into());
        }
    }
    // __all__ is a sequence of existing bindings, not a declaration that can
    // manufacture missing names. Star resolution, when needed, stays Unknown.
    if all
        .as_ref()
        .is_some_and(|names| names.iter().any(|name| !bindings.contains(name)))
    {
        out.mark_unknown("python_all_binding_not_resolved");
    }
    super::mark_forwards(&out, imports);
    out.normalize();
    out
}
fn dynamic_value(node: Node<'_>) -> bool {
    let mut stack = vec![node];
    let mut count = 0;
    while let Some(n) = stack.pop() {
        count += 1;
        if count > 100_000 {
            return true;
        }
        if matches!(
            n.kind(),
            "call"
                | "lambda"
                | "list_comprehension"
                | "dictionary_comprehension"
                | "set_comprehension"
                | "generator_expression"
                | "assignment"
        ) {
            return true;
        }
        stack.extend(named(n));
    }
    false
}
fn instance_mutation(node: Node<'_>) -> bool {
    let mut stack = vec![node];
    let mut count = 0;
    while let Some(n) = stack.pop() {
        count += 1;
        if count > 100_000 {
            return true;
        }
        if matches!(n.kind(), "assignment" | "augmented_assignment")
            && n.child_by_field_name("left")
                .is_some_and(|l| l.kind() == "attribute")
        {
            return true;
        }
        stack.extend(named(n));
    }
    false
}
/// A yield changes the outer callable kind, unlike ordinary implementation
/// edits. Nested callable/class scopes cannot make the enclosing function a generator.
fn generator_body(body: Node<'_>, out: &mut PublicSurface) -> bool {
    let mut stack = vec![body];
    let mut visited = 0usize;
    while let Some(node) = stack.pop() {
        visited += 1;
        if visited > 100_000 {
            out.mark_unknown("surface_syntax_budget_exceeded");
            return false;
        }
        if matches!(
            node.kind(),
            "function_definition" | "decorated_definition" | "class_definition" | "lambda"
        ) {
            continue;
        }
        if matches!(node.kind(), "yield" | "yield_expression") {
            return true;
        }
        stack.extend(named(node));
    }
    false
}

fn visit(
    node: Node<'_>,
    source: &[u8],
    owner: &str,
    all: &mut Option<BTreeSet<String>>,
    depth: usize,
    out: &mut PublicSurface,
) {
    if depth > 128 {
        out.mark_unknown("surface_nesting_limit");
        return;
    }
    for item in named(node) {
        if is_comment(item) {
            continue;
        }
        let mut declaration = item;
        if item.kind() == "decorated_definition" {
            out.mark_unknown("python_decorator_not_evaluated");
            if let Some(d) = item.child_by_field_name("definition") {
                declaration = d;
            }
        }
        let name = field(declaration, "name", source);
        let qualified = if owner.is_empty() {
            name.to_string()
        } else {
            format!("{owner}.{name}")
        };
        match declaration.kind() {
            "function_definition" => {
                add_entry(
                    out,
                    item,
                    source,
                    &qualified,
                    name,
                    VisibilityDomain::Module,
                    false,
                );
                if declaration
                    .child_by_field_name("body")
                    .is_some_and(|body| generator_body(body, out))
                {
                    out.entries
                        .last_mut()
                        .unwrap()
                        .conditions
                        .push("callable_kind:generator".into());
                }
                if declaration
                    .child_by_field_name("parameters")
                    .is_some_and(dynamic_value)
                    || declaration
                        .child_by_field_name("return_type")
                        .is_some_and(dynamic_value)
                {
                    out.mark_unknown("python_signature_evaluation_not_modeled");
                }
                if name == "__getattr__" {
                    out.mark_unknown("dynamic_module_attributes");
                }
                if !owner.is_empty()
                    && declaration
                        .child_by_field_name("body")
                        .is_some_and(instance_mutation)
                {
                    out.mark_unknown("python_instance_shape_not_modeled");
                }
            }
            "class_definition" => {
                add_entry(
                    out,
                    declaration,
                    source,
                    &qualified,
                    name,
                    VisibilityDomain::Module,
                    true,
                );
                if declaration
                    .child_by_field_name("superclasses")
                    .is_some_and(dynamic_value)
                {
                    out.mark_unknown("python_class_base_evaluation_not_modeled");
                }
                if let Some(body) = declaration.child_by_field_name("body") {
                    visit(body, source, &qualified, &mut None, depth + 1, out);
                }
            }
            "import_from_statement" => {
                let module = field(item, "module_name", source);
                let names: Vec<_> = named(item)
                    .into_iter()
                    .filter(|n| {
                        Some(n.id()) != item.child_by_field_name("module_name").map(|x| x.id())
                    })
                    .collect();
                for n in names {
                    let (imported, alias) = if n.kind() == "aliased_import" {
                        (field(n, "name", source), field(n, "alias", source))
                    } else {
                        (text(n, source), text(n, source))
                    };
                    let exported = if alias.is_empty() { imported } else { alias };
                    if n.kind() == "wildcard_import" {
                        out.mark_unknown("python_star_surface_requires_resolution");
                    }
                    out.forwards.push(SurfaceForward {
                        source: module.into(),
                        imported_name: imported.into(),
                        exported_name: exported.into(),
                        visibility: VisibilityDomain::Module,
                        type_only: false,
                    });
                    let q = if owner.is_empty() {
                        exported.into()
                    } else {
                        format!("{owner}.{exported}")
                    };
                    add_entry(
                        out,
                        item,
                        source,
                        &q,
                        exported,
                        VisibilityDomain::Module,
                        false,
                    );
                }
            }
            "import_statement" => {
                for n in named(item) {
                    let raw = if n.kind() == "aliased_import" {
                        field(n, "name", source)
                    } else {
                        text(n, source)
                    };
                    let alias = if n.kind() == "aliased_import" {
                        field(n, "alias", source)
                    } else {
                        raw.split('.').next().unwrap_or(raw)
                    };
                    let q = if owner.is_empty() {
                        alias.into()
                    } else {
                        format!("{owner}.{alias}")
                    };
                    add_entry(
                        out,
                        item,
                        source,
                        &q,
                        alias,
                        VisibilityDomain::Module,
                        false,
                    );
                    out.forwards.push(SurfaceForward {
                        source: raw.into(),
                        imported_name: "*".into(),
                        exported_name: alias.into(),
                        visibility: VisibilityDomain::Module,
                        type_only: false,
                    });
                }
            }
            "expression_statement" => {
                for expression in named(item) {
                    if expression.kind() == "string" {
                        continue;
                    } // module/class docstring
                    if expression.kind() != "assignment" {
                        out.mark_unknown("python_dynamic_module_statement");
                        continue;
                    }
                    let Some(left) = expression.child_by_field_name("left") else {
                        out.mark_unknown("python_unknown_binding");
                        continue;
                    };
                    let binding = text(left, source);
                    if left.kind() != "identifier" {
                        out.mark_unknown("python_destructuring_or_monkey_patch");
                        continue;
                    }
                    let q = if owner.is_empty() {
                        binding.into()
                    } else {
                        format!("{owner}.{binding}")
                    };
                    add_entry(
                        out,
                        expression,
                        source,
                        &q,
                        binding,
                        VisibilityDomain::Module,
                        false,
                    );
                    if expression
                        .child_by_field_name("right")
                        .is_some_and(dynamic_value)
                    {
                        out.mark_unknown("python_binding_evaluation_not_modeled");
                    }
                    if binding == "__all__" && owner.is_empty() {
                        if all.is_some() {
                            out.mark_unknown("python_multiple_all_assignments");
                        }
                        let values = expression.child_by_field_name("right").and_then(|right| {
                            if !matches!(right.kind(), "list" | "tuple") {
                                return None;
                            }
                            named(right)
                                .into_iter()
                                .filter(|n| !is_comment(*n))
                                .map(|n| literal(n, source))
                                .collect::<Option<BTreeSet<_>>>()
                        });
                        if values.is_none() {
                            out.mark_unknown("python_dynamic_all");
                        }
                        *all = values;
                    }
                }
            }
            "pass_statement" => {}
            _ => {
                out.mark_unknown("python_conditional_or_dynamic_bindings");
            }
        }
    }
}
