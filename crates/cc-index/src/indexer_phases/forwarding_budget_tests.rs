//! Actual forwarding-phase boundaries. Synthetic surfaces are control inputs;
//! the large direct-import case also uses the real parser and symbol resolver.
use super::*;
use crate::project_model::{discover, CapturedProject};
use cc_db::index_db::IndexDb;
use cc_model::public_surface::{PublicSurface, SurfaceForward, SurfaceKnowledge, VisibilityDomain};
use cc_model::{ImportRecord, Language};
use std::collections::BTreeSet;
use std::sync::Arc;
use tempfile::TempDir;

struct Fixture {
    root: TempDir,
    _storage: TempDir,
    indexer: Indexer,
}

impl Fixture {
    fn new() -> Self {
        let root = TempDir::new().unwrap();
        let storage = TempDir::new().unwrap();
        let db = Arc::new(
            IndexDb::open(&storage.path().join("index.sqlite3"))
                .unwrap()
                .0,
        );
        let indexer = Indexer::new(db, root.path(), &Default::default());
        Self {
            root,
            _storage: storage,
            indexer,
        }
    }

    fn capture(&self, paths: BTreeSet<String>) -> CapturedProject {
        discover(self.root.path(), paths, None, None, &Default::default()).unwrap()
    }

    fn check(&self, units: &[FileWriteUnit], paths: BTreeSet<String>) -> CcResult<()> {
        self.indexer
            .install_forwarding(&mut SymbolCatalog::new(), units, &self.capture(paths))
    }
}

fn unit(path: &str, surface: PublicSurface) -> FileWriteUnit {
    FileWriteUnit {
        rel_path: path.into(),
        language: Language::TypeScript,
        content_hash: "synthetic-forwarding-budget-control".into(),
        mtime: 0.0,
        size: 1,
        outcome: ParseOutcome {
            public_surface: surface,
            ..Default::default()
        },
    }
}

fn leaf(path: &str) -> FileWriteUnit {
    unit(
        path,
        PublicSurface::new("typescript", path, "budget-control-v1"),
    )
}

fn forwarding(path: &str, target: &str, edge_count: usize) -> FileWriteUnit {
    let mut surface = PublicSurface::new("typescript", path, "budget-control-v1");
    surface.forwards = (0..edge_count)
        .map(|n| SurfaceForward {
            source: format!("./{target}"),
            imported_name: "value".into(),
            exported_name: format!("value{n}"),
            visibility: VisibilityDomain::Exported,
            type_only: false,
        })
        .collect();
    surface.normalize();
    unit(path, surface)
}

fn importer(paths: &[String]) -> FileWriteUnit {
    let mut caller = leaf("caller.ts");
    caller.outcome.imports = paths
        .iter()
        .map(|path| ImportRecord {
            file_path: "caller.ts".into(),
            import_string: format!("./{path}"),
            resolved_path: Some(path.clone()),
            ..Default::default()
        })
        .collect();
    caller
}

fn inventory(paths: &[String]) -> BTreeSet<String> {
    paths.iter().cloned().chain(["caller.ts".into()]).collect()
}

fn assert_file_budget(result: CcResult<()>) {
    assert_eq!(
        result.unwrap_err().to_string(),
        "config error: reexport_file_budget_exceeded"
    );
}

#[test]
fn more_than_4096_known_leaf_roots_keep_all_native_direct_bindings() {
    let fixture = Fixture::new();
    let parser = cc_parsers::ParserRegistry::new();
    let count = 4_097;
    let paths: BTreeSet<_> = (0..count).map(|n| format!("file{n}.ts")).collect();
    let captured = fixture.capture(paths);
    let mut units = Vec::new();
    for n in 0..count {
        let next = (n + 1) % count;
        let path = format!("file{n}.ts");
        let source = format!(
            "import {{own{next}}} from './file{next}';\nexport function own{n}(x:number):number{{return x;}}\nexport function call{n}():number{{return own{next}(1);}}\n"
        );
        let mut outcome = parser.parse(&path, &source, Language::TypeScript).unwrap();
        assert_eq!(outcome.public_surface.knowledge, SurfaceKnowledge::Known);
        assert!(outcome.public_surface.forwards.is_empty());
        captured.apply_imports(&path, &mut outcome);
        assert_eq!(outcome.imports.len(), 1);
        assert_eq!(
            outcome.imports[0].resolved_path,
            Some(format!("file{next}.ts"))
        );
        let mut parsed = leaf(&path);
        parsed.outcome = outcome;
        units.push(parsed);
    }
    let mut resolution = fixture
        .indexer
        .build_resolution_catalog(true, &units, &[])
        .unwrap();
    fixture
        .indexer
        .install_forwarding(&mut resolution.catalog, &units, &captured)
        .unwrap();
    Indexer::resolve_call_edges(
        &resolution.catalog,
        &mut units,
        &resolution.resolution_contexts,
    );
    let mut bindings = 0;
    for (n, parsed) in units.iter().enumerate() {
        let next = (n + 1) % count;
        let edges = &parsed.outcome.call_edges;
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].callee_symbol, format!("own{next}"));
        assert_eq!(edges[0].target_file_path, Some(format!("file{next}.ts")));
        assert!(edges[0].target_symbol_id.is_some());
        assert!(edges[0].callee_symbol_uid.is_some());
        bindings += 1;
    }
    assert_eq!(bindings, 4_097);
}

#[test]
fn actual_forwarding_roots_above_4096_still_fail() {
    let fixture = Fixture::new();
    let paths: Vec<_> = (0..4_097).map(|n| format!("barrel{n}.ts")).collect();
    let mut units: Vec<_> = paths
        .iter()
        .map(|path| forwarding(path, "leaf.ts", 1))
        .collect();
    units.push(importer(&paths));
    units.push(leaf("leaf.ts"));
    let mut files = inventory(&paths);
    files.insert("leaf.ts".into());
    assert_file_budget(fixture.check(&units, files));
}

#[test]
fn current_unknown_empty_surfaces_above_4096_still_fail() {
    let fixture = Fixture::new();
    let paths: Vec<_> = (0..4_097).map(|n| format!("unknown{n}.ts")).collect();
    let mut units: Vec<_> = paths
        .iter()
        .map(|path| {
            let mut value = leaf(path);
            value
                .outcome
                .public_surface
                .mark_unknown("control-no-proof-of-empty");
            value
        })
        .collect();
    units.push(importer(&paths));
    assert_file_budget(fixture.check(&units, inventory(&paths)));
}

#[test]
fn persisted_known_empty_surfaces_keep_the_original_load_budget() {
    let fixture = Fixture::new();
    let paths: Vec<_> = (0..4_097).map(|n| format!("stored{n}.ts")).collect();
    let stored: Vec<_> = paths.iter().map(|path| leaf(path)).collect();
    fixture
        .indexer
        .db
        .writes()
        .replace_files_batch(&stored)
        .unwrap();
    assert_eq!(
        fixture
            .indexer
            .db
            .reads()
            .public_surfaces(&paths)
            .unwrap()
            .len(),
        paths.len()
    );
    fixture
        .check(&[importer(&paths[..4_096])], inventory(&paths))
        .unwrap();
    assert_file_budget(fixture.check(&[importer(&paths)], inventory(&paths)));
}

#[test]
fn missing_persisted_surfaces_above_4096_still_fail_before_loading() {
    let fixture = Fixture::new();
    let paths: Vec<_> = (0..4_097).map(|n| format!("missing{n}.ts")).collect();
    assert_file_budget(fixture.check(&[importer(&paths)], inventory(&paths)));
}

#[test]
fn current_leaf_does_not_add_depth_but_33_forwarding_layers_still_fail() {
    for layers in [32, 33] {
        let fixture = Fixture::new();
        let paths: Vec<_> = (0..layers).map(|n| format!("layer{n}.ts")).collect();
        let stored: Vec<_> = paths
            .iter()
            .enumerate()
            .map(|(n, path)| {
                forwarding(
                    path,
                    paths.get(n + 1).map(String::as_str).unwrap_or("leaf.ts"),
                    1,
                )
            })
            .collect();
        fixture
            .indexer
            .db
            .writes()
            .replace_files_batch(&stored)
            .unwrap();
        // The leaf is also an initial direct-import root. It was visited in
        // round one before the fast path; pruning it must not add a 33rd hop.
        let units = [
            importer(&[paths[0].clone(), "leaf.ts".into()]),
            leaf("leaf.ts"),
        ];
        let mut files = inventory(&paths);
        files.insert("leaf.ts".into());
        let result = fixture.check(&units, files);
        if layers == 32 {
            result.unwrap();
        } else {
            assert_eq!(
                result.unwrap_err().to_string(),
                "config error: reexport_depth_budget_exceeded"
            );
        }
    }
}

#[test]
fn actual_forwarding_edges_above_65536_still_fail() {
    let fixture = Fixture::new();
    let roots = ["barrel.ts".to_string()];
    let units = [
        importer(&roots),
        forwarding("barrel.ts", "leaf.ts", 65_537),
        leaf("leaf.ts"),
    ];
    let mut files = inventory(&roots);
    files.insert("leaf.ts".into());
    assert_eq!(
        fixture.check(&units, files).unwrap_err().to_string(),
        "config error: reexport_edge_budget_exceeded"
    );
}
