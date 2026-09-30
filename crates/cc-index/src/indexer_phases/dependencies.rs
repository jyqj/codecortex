//! Derive invalidation events from changed-file evidence. This feeds the same
//! bounded dirty closure; it is not a second planner or a persistent frontier.
use crate::indexer::{FileAction, Indexer};
use cc_db::index_db::FileWriteUnit;
use cc_model::{package_surface::PackageKey, resolution::*, CcResult};
use std::collections::{BTreeMap, BTreeSet, HashMap};
impl Indexer {
    pub(crate) fn resolution_dependency_events(
        &self,
        actions: &HashMap<String, FileAction>,
        units: &[FileWriteUnit],
        removed: &[String],
        surface_changes: &[String],
    ) -> CcResult<BTreeSet<ResolutionDependency>> {
        let paths: Vec<_> = units
            .iter()
            .map(|u| u.rel_path.clone())
            .chain(removed.iter().cloned())
            .collect();
        let mut events = BTreeSet::new();
        for p in surface_changes {
            events.insert(ResolutionDependency::new(
                DependencyKind::TargetSurface,
                p.clone(),
            ));
        }
        let old = self.db.reads().resolution_symbols_in_files(&paths)?;
        let new: Vec<_> = units
            .iter()
            .flat_map(|u| u.outcome.symbols.iter().map(ResolutionSymbol::from))
            .collect();
        let buckets = |symbols: Vec<ResolutionSymbol>| {
            let mut map: BTreeMap<String, BTreeSet<ResolutionSymbol>> = BTreeMap::new();
            for symbol in symbols {
                let mut keys = resolution_name_keys(&symbol.name);
                if let Some(qname) = &symbol.qname {
                    keys.extend(resolution_name_keys(qname));
                }
                for key in keys {
                    map.entry(key).or_default().insert(symbol.clone());
                }
            }
            map
        };
        let old = buckets(old);
        let new = buckets(new);
        for key in old.keys().chain(new.keys()) {
            if old.get(key) != new.get(key) {
                events.insert(ResolutionDependency::new(
                    DependencyKind::NameBucket,
                    key.clone(),
                ));
            }
        }
        if !events.is_empty() {
            events.insert(ResolutionDependency::new(
                DependencyKind::SymbolInventory,
                "*",
            ));
        }
        let inventory: Vec<_> = actions
            .iter()
            .filter(|(_, a)| matches!(a, FileAction::Add))
            .map(|(p, _)| p.clone())
            .chain(removed.iter().cloned())
            .collect();
        if !inventory.is_empty() {
            events.insert(ResolutionDependency::new(
                DependencyKind::FileInventory,
                "*",
            ));
        }
        for path in inventory {
            events.insert(ResolutionDependency::new(DependencyKind::MissingPath, path));
        }
        let old_surfaces = self.db.reads().public_surfaces(&paths)?;
        let changed: BTreeSet<_> = surface_changes.iter().collect();
        for p in &paths {
            if changed.contains(p) || removed.contains(p) {
                if let Some(key) = old_surfaces.get(p).and_then(PackageKey::from_surface) {
                    events.insert(ResolutionDependency::new(
                        DependencyKind::PackageFiles,
                        key.storage_key(),
                    ));
                }
            }
            let leaf = p.rsplit('/').next().unwrap_or(p);
            if matches!(
                leaf,
                "Cargo.toml"
                    | "Cargo.lock"
                    | "go.mod"
                    | "go.work"
                    | "go.sum"
                    | "package.json"
                    | "tsconfig.json"
                    | "jsconfig.json"
                    | "pyproject.toml"
            ) {
                events.insert(ResolutionDependency::new(DependencyKind::ModuleConfig, "*"));
            }
        }
        for u in units {
            if changed.contains(&u.rel_path) {
                if let Some(key) = PackageKey::from_surface(&u.outcome.public_surface) {
                    events.insert(ResolutionDependency::new(
                        DependencyKind::PackageFiles,
                        key.storage_key(),
                    ));
                }
            }
        }
        Ok(events)
    }
}
