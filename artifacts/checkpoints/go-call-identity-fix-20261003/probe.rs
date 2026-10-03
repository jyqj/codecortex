use cc_db::index_db::IndexDb;
use cc_index::Indexer;
use cc_model::{config::IndexingConfig, Language};
use cc_parsers::{go::GoParser, traits::FileParser};
use std::{collections::HashMap, path::Path, sync::Arc};
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let source = std::fs::read_to_string(&args[1]).unwrap();
    let name = Path::new(&args[1]).file_name().unwrap().to_str().unwrap();
    let out = GoParser::new().parse(name, &source, Language::Go).unwrap();
    let mut groups: HashMap<&str, usize> = HashMap::new();
    for call in &out.call_edges {
        *groups.entry(&call.edge_id).or_default() += 1;
    }
    let duplicates = groups.values().filter(|&&n| n > 1).count();
    let repeat = GoParser::new().parse(name, &source, Language::Go).unwrap();
    assert_eq!(
        serde_json::to_value(&out).unwrap(),
        serde_json::to_value(&repeat).unwrap()
    );
    let spans_exact = out.call_edges.iter().all(|c| {
        let line = source.lines().nth(c.line as usize - 1).unwrap();
        &line.as_bytes()[c.start_col as usize..c.end_col as usize] == c.callee_symbol.as_bytes()
    });
    let root = Path::new(&args[2]);
    std::fs::create_dir(root).unwrap();
    std::fs::write(root.join(name), source).unwrap();
    std::fs::create_dir_all(root.join(".codecortex")).unwrap();
    let (db, _) = IndexDb::open(&root.join(".codecortex/index.sqlite3")).unwrap();
    let config = IndexingConfig::default();
    let indexer = Indexer::new(Arc::new(db), root, &config);
    let result = indexer.build_index(root, true);
    let (ok, error) = match result {
        Ok(_) => (true, None),
        Err(e) => (false, Some(e.to_string())),
    };
    println!(
        "{}",
        serde_json::json!({"path":name,"calls":out.call_edges.len(),"refs":out.symbol_refs.len(),"duplicate_id_groups":duplicates,"all_spans_exact_callee_tokens":spans_exact,"parse_deterministic":true,"index_ok":ok,"index_error":error})
    );
    if args[3] == "fixed" {
        assert_eq!(duplicates, 0);
        assert!(spans_exact);
        assert!(ok);
    } else {
        assert!(duplicates > 0);
        assert!(!ok);
        let message = error.unwrap();
        assert!(
            message.contains("conflicting duplicate call site")
                || message.contains("conflicting duplicate symbol_ref site")
        );
    }
}
