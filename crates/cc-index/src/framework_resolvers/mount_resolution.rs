//! Shared cross-file mount-prefix resolution.
//!
//! Most framework resolvers propagate route prefixes the same way:
//! 1. collect mount edges (`app.use("/api", router)`-style),
//! 2. locate the file that defines the mounted router via the symbol catalog
//!    and prepend the prefix to that file's http route edges,
//! 3. bind `handler_symbol_uid` for route edges that still lack one.
//!
//! [`resolve_mounts`] is that flow; [`MountSpec`] declares the per-framework
//! differences (mount edge kinds, prefix joining, target-file lookup).

use cc_model::edge::RouteEdgeRecord;
use cc_model::parse::ParseOutcome;

use crate::resolver::SymbolCatalog;

/// One mount site collected from a mount-kind route edge.
pub(crate) struct MountPoint {
    pub prefix: String,
    pub handler_name: String,
    /// Original handler expression; falls back to `handler_name`.
    pub handler_expr: String,
    /// The route_kind that marked this edge as a mount.
    pub mount_kind: String,
    pub mount_file: String,
}

/// How a mount prefix is joined onto a sub-route path. A bare `"/"`
/// sub-route always collapses to the prefix itself.
pub(crate) enum PrefixJoin {
    /// `"{prefix}{path}"` (Express-style: "/api" + "/users" → "/api/users").
    Plain,
    /// Strip the sub-route's leading slash before appending (Django-style:
    /// "/api/" + "/users/" → "/api/users/").
    StripSubLeadingSlash,
    /// Trim the prefix's trailing slash, strip the sub-route's leading slash,
    /// join with "/" (Actix-style: "/api" + "/users" → "/api/users").
    SlashNormalized,
}

/// Target-file lookup: given a mount, find the file whose routes receive the
/// prefix. The outcomes slice carries all file paths for lookups that need
/// the file list (e.g. Laravel `routes/api.php` suffix matching).
pub(crate) type TargetLookupFn<'a> =
    &'a dyn Fn(&SymbolCatalog, &[(String, ParseOutcome)], &MountPoint) -> Option<String>;

/// Strategy for resolving a mount's target file.
pub(crate) enum TargetLookup<'a> {
    /// Catalog lookup by `handler_name`, requiring a different file.
    Default,
    /// Default lookup first; the fallback runs only on a miss.
    DefaultWithFallback(TargetLookupFn<'a>),
    /// Full replacement of the default lookup.
    Custom(TargetLookupFn<'a>),
}

/// Per-framework parameters of the shared mount-resolution flow.
pub(crate) struct MountSpec<'a> {
    /// route_kind values that mark a mount edge.
    pub mount_kinds: &'a [&'a str],
    /// Skip mounts whose prefix is `"/"` (empty prefixes are always skipped).
    pub skip_root_prefix: bool,
    /// Restrict every step to edges of this framework; `None` = all edges.
    pub framework: Option<&'static str>,
    pub join: PrefixJoin,
    pub lookup: TargetLookup<'a>,
}

/// Catalog lookup by `handler_name`, requiring the hit to live in a
/// different file than the mount itself.
pub(crate) fn default_target_lookup(catalog: &SymbolCatalog, mount: &MountPoint) -> Option<String> {
    match catalog.lookup_symbol(&mount.handler_name, &mount.mount_file) {
        Some((_, file)) if file != mount.mount_file => Some(file),
        _ => None,
    }
}

fn resolve_target_file(
    catalog: &SymbolCatalog,
    outcomes: &[(String, ParseOutcome)],
    mount: &MountPoint,
    lookup: &TargetLookup<'_>,
) -> Option<String> {
    match lookup {
        TargetLookup::Default => default_target_lookup(catalog, mount),
        TargetLookup::DefaultWithFallback(fallback) => {
            default_target_lookup(catalog, mount).or_else(|| fallback(catalog, outcomes, mount))
        }
        TargetLookup::Custom(custom) => custom(catalog, outcomes, mount),
    }
}

fn join_prefix(prefix: &str, route_path: &str, join: &PrefixJoin) -> String {
    if route_path == "/" {
        return prefix.to_string();
    }
    match join {
        PrefixJoin::Plain => format!("{}{}", prefix, route_path),
        PrefixJoin::StripSubLeadingSlash => {
            let sub = route_path.strip_prefix('/').unwrap_or(route_path);
            format!("{}{}", prefix, sub)
        }
        PrefixJoin::SlashNormalized => {
            let sub = route_path.strip_prefix('/').unwrap_or(route_path);
            format!("{}/{}", prefix.trim_end_matches('/'), sub)
        }
    }
}

/// The shared three-step flow: collect mounts → prepend prefixes to http
/// route edges in the target files → bind missing `handler_symbol_uid`s.
pub(crate) fn resolve_mounts(
    catalog: &SymbolCatalog,
    outcomes: &mut [(String, ParseOutcome)],
    spec: &MountSpec<'_>,
) {
    let framework_matches = |edge: &RouteEdgeRecord| {
        spec.framework
            .is_none_or(|fw| edge.framework.as_deref() == Some(fw))
    };

    // Step 1: collect mount points.
    let mut mounts: Vec<MountPoint> = Vec::new();
    for (file_path, outcome) in outcomes.iter() {
        for edge in &outcome.route_edges {
            if !framework_matches(edge) {
                continue;
            }
            let kind = edge.route_kind.as_deref().unwrap_or("");
            if !spec.mount_kinds.contains(&kind) {
                continue;
            }
            if let Some(ref handler) = edge.handler_name {
                let prefix = &edge.route_path;
                if prefix.is_empty() || (spec.skip_root_prefix && prefix == "/") {
                    continue;
                }
                mounts.push(MountPoint {
                    prefix: prefix.clone(),
                    handler_name: handler.clone(),
                    handler_expr: edge.handler_expr.clone().unwrap_or_else(|| handler.clone()),
                    mount_kind: kind.to_string(),
                    mount_file: file_path.clone(),
                });
            }
        }
    }

    // Step 2: for each mount, find the target file and prepend the prefix.
    for mount in &mounts {
        let target_file = match resolve_target_file(catalog, &*outcomes, mount, &spec.lookup) {
            Some(file) => file,
            None => continue,
        };

        for (file_path, outcome) in outcomes.iter_mut() {
            if *file_path != target_file {
                continue;
            }
            for edge in &mut outcome.route_edges {
                if !framework_matches(edge) {
                    continue;
                }
                if edge.route_kind.as_deref() != Some("http") {
                    continue;
                }
                // Only prepend once (avoid double-prefixing on repeated runs)
                if !edge.route_path.starts_with(&mount.prefix) {
                    edge.route_path = join_prefix(&mount.prefix, &edge.route_path, &spec.join);
                }
            }
        }
    }

    // Step 3: resolve handler_symbol_uid for route edges that lack one.
    for (file_path, outcome) in outcomes.iter_mut() {
        let fp = file_path.clone();
        for edge in &mut outcome.route_edges {
            if !framework_matches(edge) {
                continue;
            }
            if edge.handler_symbol_uid.is_some() {
                continue;
            }
            if let Some(ref handler_name) = edge.handler_name {
                if let Some((uid, _)) = catalog.lookup_symbol(handler_name, &fp) {
                    if !uid.is_empty() {
                        edge.handler_symbol_uid = Some(uid);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{make_route_edge, RouteEdgeSpec};
    use super::*;
    use cc_model::symbol::{SymbolKind, SymbolRecord};
    use cc_model::ParserTier;

    fn make_symbol(name: &str, file: &str, uid: Option<&str>) -> SymbolRecord {
        SymbolRecord {
            symbol_id: format!("sym#{}#{}", file, name),
            file_path: file.to_string(),
            name: name.to_string(),
            kind: SymbolKind::Variable,
            container: None,
            start_line: 1,
            end_line: 1,
            start_col: 0,
            end_col: 0,
            signature: None,
            doc: None,
            parser_tier: ParserTier::TreeSitter,
            parser_confidence: 0.9,
            qname: Some(name.to_string()),
            parent_symbol_id: None,
            scope_id: None,
            export_name: Some(name.to_string()),
            is_default_export: false,
            symbol_uid: uid.map(String::from),
            framework_role: None,
            receiver_type: None,
            param_types: None,
            return_type: None,
            param_count: None,
            base_types: None,
            implements: None,
        }
    }

    fn make_edge(
        file: &str,
        route_path: &str,
        handler: &str,
        route_kind: &'static str,
    ) -> RouteEdgeRecord {
        make_route_edge(
            file,
            1,
            0,
            RouteEdgeSpec {
                route_path: route_path.to_string(),
                handler_name: Some(handler.to_string()),
                method: None,
                framework: "testfw",
                route_kind,
                confidence: 0.9,
                parser_tier: ParserTier::TreeSitter,
            },
        )
    }

    fn outcome_with(edges: Vec<RouteEdgeRecord>) -> ParseOutcome {
        ParseOutcome {
            route_edges: edges,
            ..Default::default()
        }
    }

    fn default_spec() -> MountSpec<'static> {
        MountSpec {
            mount_kinds: &["test_mount"],
            skip_root_prefix: true,
            framework: None,
            join: PrefixJoin::Plain,
            lookup: TargetLookup::Default,
        }
    }

    /// Catalog with `apiRouter` defined in `src/routes.py`, plus standard
    /// mount (`src/app.py`) and sub-router (`src/routes.py`) outcomes.
    fn mount_fixture(sub_route_path: &str) -> (SymbolCatalog, Vec<(String, ParseOutcome)>) {
        let mut catalog = SymbolCatalog::new();
        catalog.add_symbols(&[make_symbol(
            "apiRouter",
            "src/routes.py",
            Some("uid:apiRouter"),
        )]);
        let outcomes = vec![
            (
                "src/app.py".to_string(),
                outcome_with(vec![make_edge(
                    "src/app.py",
                    "/api",
                    "apiRouter",
                    "test_mount",
                )]),
            ),
            (
                "src/routes.py".to_string(),
                outcome_with(vec![make_edge(
                    "src/routes.py",
                    sub_route_path,
                    "getUsers",
                    "http",
                )]),
            ),
        ];
        (catalog, outcomes)
    }

    #[test]
    fn root_sub_route_takes_bare_prefix() {
        let (catalog, mut outcomes) = mount_fixture("/");
        resolve_mounts(&catalog, &mut outcomes, &default_spec());
        assert_eq!(outcomes[1].1.route_edges[0].route_path, "/api");
    }

    #[test]
    fn plain_join_prepends_prefix() {
        let (catalog, mut outcomes) = mount_fixture("/users");
        resolve_mounts(&catalog, &mut outcomes, &default_spec());
        assert_eq!(outcomes[1].1.route_edges[0].route_path, "/api/users");
    }

    #[test]
    fn prepend_is_idempotent_via_starts_with_guard() {
        let (catalog, mut outcomes) = mount_fixture("/users");
        resolve_mounts(&catalog, &mut outcomes, &default_spec());
        resolve_mounts(&catalog, &mut outcomes, &default_spec());
        assert_eq!(
            outcomes[1].1.route_edges[0].route_path, "/api/users",
            "second run must not double-prefix"
        );
    }

    #[test]
    fn unknown_router_skips_prefixing() {
        let (catalog, mut outcomes) = mount_fixture("/users");
        outcomes[0].1.route_edges[0].handler_name = Some("ghostRouter".to_string());
        resolve_mounts(&catalog, &mut outcomes, &default_spec());
        assert_eq!(
            outcomes[1].1.route_edges[0].route_path, "/users",
            "mount whose router is not in the catalog must be a no-op"
        );
    }

    #[test]
    fn non_http_edges_skip_prefixing() {
        let (catalog, mut outcomes) = mount_fixture("/users");
        outcomes[1].1.route_edges[0].route_kind = Some("middleware".to_string());
        resolve_mounts(&catalog, &mut outcomes, &default_spec());
        assert_eq!(outcomes[1].1.route_edges[0].route_path, "/users");
    }

    #[test]
    fn root_prefix_mount_is_skipped_when_configured() {
        let (catalog, mut outcomes) = mount_fixture("users");
        outcomes[0].1.route_edges[0].route_path = "/".to_string();
        resolve_mounts(&catalog, &mut outcomes, &default_spec());
        assert_eq!(outcomes[1].1.route_edges[0].route_path, "users");
    }

    #[test]
    fn binds_handler_uid_from_catalog() {
        let (mut catalog, mut outcomes) = mount_fixture("/users");
        catalog.add_symbols(&[make_symbol(
            "getUsers",
            "src/routes.py",
            Some("uid:getUsers"),
        )]);
        resolve_mounts(&catalog, &mut outcomes, &default_spec());
        assert_eq!(
            outcomes[1].1.route_edges[0].handler_symbol_uid.as_deref(),
            Some("uid:getUsers")
        );
    }

    #[test]
    fn missing_uid_leaves_handler_unbound() {
        let (mut catalog, mut outcomes) = mount_fixture("/users");
        // Symbol exists but carries no UID: the empty-uid guard must hold.
        catalog.add_symbols(&[make_symbol("getUsers", "src/routes.py", None)]);
        resolve_mounts(&catalog, &mut outcomes, &default_spec());
        assert_eq!(outcomes[1].1.route_edges[0].handler_symbol_uid, None);
    }

    #[test]
    fn framework_filter_skips_other_frameworks() {
        let (catalog, mut outcomes) = mount_fixture("/users");
        let spec = MountSpec {
            framework: Some("otherfw"),
            ..default_spec()
        };
        resolve_mounts(&catalog, &mut outcomes, &spec);
        assert_eq!(
            outcomes[1].1.route_edges[0].route_path, "/users",
            "edges of a different framework must be untouched"
        );
    }

    #[test]
    fn fallback_lookup_used_when_default_misses() {
        let (catalog, mut outcomes) = mount_fixture("/users");
        outcomes[0].1.route_edges[0].handler_name = Some("ghostRouter".to_string());
        let fallback = |_: &SymbolCatalog,
                        _: &[(String, ParseOutcome)],
                        _: &MountPoint|
         -> Option<String> { Some("src/routes.py".to_string()) };
        let spec = MountSpec {
            lookup: TargetLookup::DefaultWithFallback(&fallback),
            ..default_spec()
        };
        resolve_mounts(&catalog, &mut outcomes, &spec);
        assert_eq!(outcomes[1].1.route_edges[0].route_path, "/api/users");
    }

    #[test]
    fn join_strip_sub_leading_slash() {
        assert_eq!(
            join_prefix("/api/", "/users/", &PrefixJoin::StripSubLeadingSlash),
            "/api/users/"
        );
        assert_eq!(
            join_prefix("/api/", "/", &PrefixJoin::StripSubLeadingSlash),
            "/api/"
        );
    }

    #[test]
    fn join_slash_normalized() {
        assert_eq!(
            join_prefix("/api", "/users", &PrefixJoin::SlashNormalized),
            "/api/users"
        );
        assert_eq!(
            join_prefix("/api/", "users", &PrefixJoin::SlashNormalized),
            "/api/users"
        );
        assert_eq!(
            join_prefix("/api", "/", &PrefixJoin::SlashNormalized),
            "/api"
        );
    }
}
