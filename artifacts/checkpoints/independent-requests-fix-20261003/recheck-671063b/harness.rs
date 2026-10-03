mod atoms;
use cc_db::index_db::IndexDb;
use cc_index::Indexer;
use cc_model::{config::IndexingConfig, resolution::*};
use serde_json::{json, Value};
use std::{collections::BTreeSet, path::Path, sync::Arc};

fn index(root: &Path, db_path: &Path) -> Value {
    assert!(!db_path.exists(), "fresh DB required");
    let (db, _) = IndexDb::open(db_path).unwrap();
    let db = Arc::new(db);
    let config = IndexingConfig::default();
    let root = root.canonicalize().unwrap();
    let root = root.as_path();
    match Indexer::new(db.clone(), root, &config).build_index(root, true) {
        Err(e) => json!({"error": e.to_string()}),
        Ok(report) => {
            let conn = rusqlite::Connection::open(db_path).unwrap();
            let mut stmt = conn.prepare("SELECT file_path,payload FROM resolution_manifests ORDER BY file_path").unwrap();
            let rows = stmt.query_map([], |r| Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?))).unwrap();
            let mut manifests = Vec::new();
            for row in rows {
                let (path, payload) = row.unwrap();
                let m: ResolutionManifest = serde_json::from_str(&payload).unwrap();
                m.validate().unwrap();
                manifests.push(json!({"path":path,"dependencies":m.dependencies}));
            }
            let total: i64 = conn.query_row("SELECT count(*) FROM resolution_dependencies", [], |r|r.get(0)).unwrap();
            let empty: i64 = conn.query_row("SELECT count(*) FROM resolution_dependencies WHERE key=''", [], |r|r.get(0)).unwrap();
            assert_eq!(empty, 0);
            json!({"report":report,"dependency_count":total,"empty_dependencies":empty,"validated_manifests":manifests})
        }
    }
}
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let fixed = args[1] != "old";
    let repaired = args[1] == "repaired";
    let output = Path::new(&args[2]);
    std::fs::create_dir_all(output).unwrap();
    if args.len() == 4 && args[3] == "legal-identifiers" {
        let mut cases = Vec::new();
        for (name, file, text, expected) in [
            ("python-underscore", "sample.py", "class _: pass\ndef consume(item: _):\n    pass\n", "_"),
            ("python-double-underscore", "sample.py", "class __: pass\ndef consume(item: __):\n    pass\n", "__"),
            ("typescript-dollar", "sample.ts", "class $ {}\nfunction consume(item: $): void {}\n", "$"),
            ("python-unicode-other-id", "sample.py", "class ℘: pass\ndef consume(item: ℘):\n    pass\n", "℘"),
            ("python-unicode-estimated", "sample.py", "class ℮: pass\ndef consume(item: ℮):\n    pass\n", "℮"),
            ("python-unicode-continuation", "sample.py", "class a·b: pass\ndef consume(item: a·b):\n    pass\n", "a·b"),
            ("python-combining-continuation", "sample.py", "class A\u{0301}: pass\ndef consume(item: A\u{0301}):\n    pass\n", "A\u{0301}"),
        ] {
            let dir = output.join(name);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(file), text).unwrap();
            let result = index(&dir, &output.join(format!("{name}.sqlite")));
            assert!(result.get("error").is_none(), "{result}");
            assert_eq!(result["report"]["files_parsed"],1);
            assert!(result["report"]["parse_errors"].as_array().unwrap().is_empty());
            let conn = rusqlite::Connection::open(output.join(format!("{name}.sqlite"))).unwrap();
            let edges: i64 = conn.query_row("SELECT count(*) FROM semantic_edges WHERE relation_kind='uses_type' AND target_symbol=?1", [expected], |r|r.get(0)).unwrap();
            let deps = result["validated_manifests"][0]["dependencies"].as_array().unwrap();
            let name_dep = deps.iter().any(|d| d["kind"] == "name_bucket" && d["key"] == expected);
            cases.push(json!({"name":name,"type_name":expected,"atoms":atoms::type_atoms(expected),"uses_type_edges":edges,"name_dependency":name_dep,"result":result}));
        }
        std::fs::write(output.join("legal-identifiers.json"),serde_json::to_vec_pretty(&cases).unwrap()).unwrap();
        return;
    }
    if args.len() == 4 {
        let result = index(Path::new(&args[3]), &output.join("index.sqlite"));
        std::fs::write(output.join("full-index.json"), serde_json::to_vec_pretty(&result).unwrap()).unwrap();
        assert!(result.get("error").is_none(), "{result}");
        return;
    }
    let atom_controls: [(&str, &[&str]); 6] = [
        ("Mapping[pkg.Account, Sequence[domain::Receipt]] | café.Étiquette", &["Mapping","pkg.Account","Sequence","domain::Receipt","café.Étiquette"]),
        ("tuple[订单, ...]", &["tuple","订单"]),
        ("Envelope<\u{2003}Alpha,\tBeta\nGamma>", &["Envelope","Alpha","Beta","Gamma"]),
        ("&_Internal | *Pointer", &["_Internal","*Pointer"]),
        ("List[int] | None", &["List"]),
        ("tuple[tuple[Node, ...], ...]", &["tuple","tuple","Node"]),
    ];
    let mut controls = Vec::new();
    for (raw, expected) in atom_controls {
        let actual = atoms::type_atoms(raw);
        let passed = actual == expected;
        if fixed { assert!(passed, "{raw}: {actual:?}"); }
        controls.push(json!({"raw":raw,"actual":actual,"expected":expected,"passed":passed}));
    }
    let punctuation: Vec<_> = ["...", "::", "???", "---", "***", "&&", "!!"].into_iter().map(|raw| {
        let actual = atoms::type_atoms(raw);
        if fixed { assert!(actual.is_empty(), "{raw}"); }
        json!({"raw":raw,"actual":actual})
    }).collect();
    let boundary_inputs = ["_", "__", "$", "$$", "℘", "℮", "a·b", "A\u{0301}", "pkg::℘", "ns.$", "module.__"];
    let mut boundaries = Vec::new();
    for raw in boundary_inputs {
        let actual = atoms::type_atoms(raw);
        if repaired { assert_eq!(actual, [raw], "legal identifier {raw}"); }
        boundaries.push(json!({"raw":raw,"actual":actual}));
    }
    for raw in ["…", "⚙"] {
        let actual = atoms::type_atoms(raw);
        if repaired { assert_eq!(actual, [raw], "unknown spelling retained conservatively"); }
        boundaries.push(json!({"raw":raw,"actual":actual}));
    }
    for raw in ["_ | __ | $ | ℘ | ℮", "Pair<_, pkg::℘> | ns.$"] {
        let actual = atoms::type_atoms(raw);
        if repaired {
            assert_eq!(actual, if raw.starts_with("Pair") {vec!["Pair", "_", "pkg::℘", "ns.$"]} else {vec!["_", "__", "$", "℘", "℮"]});
        }
        boundaries.push(json!({"raw":raw,"actual":actual}));
    }
    let mut keys = Vec::new();
    for raw in ["pkg::Account", "café.Étiquette", "订单.状态", "...", "pkg.\t", "::", ""] {
        let actual = resolution_name_keys(raw);
        if fixed { assert!(!actual.contains("")); }
        keys.push(json!({"raw":raw,"keys":actual}));
    }
    for (raw, expected) in [("pkg::Account", vec!["pkg::account","account"]),("café.Étiquette", vec!["café.étiquette","étiquette"]),("订单.状态",vec!["订单.状态","状态"])] {
        assert_eq!(resolution_name_keys(raw), expected.into_iter().map(String::from).collect::<BTreeSet<_>>());
    }
    // Both construction and deserialized/normalized payloads must reject empty keys.
    let mut invalid = ResolutionManifest::new();
    invalid.dependency(DependencyKind::NameBucket, "");
    invalid.normalize();
    let direct_error = invalid.validate().unwrap_err().to_string();
    assert!(direct_error.contains("invalid dependency key"));
    let restored: ResolutionManifest = serde_json::from_slice(&serde_json::to_vec(&invalid).unwrap()).unwrap();
    assert!(restored.validate().is_err());
    let mut results = Vec::new();
    let fixtures = [
        ("old-negative", "class Packet: pass\nclass Fault(Exception):\n    def __init__(self, retry_history: tuple[int, ...] | None = None):\n        pass\n"),
        ("nonvariadic", "class Packet: pass\nclass Fault(Exception):\n    def __init__(self, retry_history: tuple[int] | None = None):\n        pass\n"),
        ("named-variadic", "class Packet: pass\nclass Fault(Exception):\n    def __init__(self, packets: tuple[Packet, ...] | None = None):\n        pass\n"),
        ("unicode-union", "class 订单: pass\nclass Receipt: pass\nclass Owner:\n    def __init__(self, values: list[订单] | Receipt):\n        pass\n"),
        ("qualified", "import domain\nclass Owner:\n    def __init__(self, values: dict[str, domain.Invoice]):\n        pass\n"),
    ];
    for (name, text) in fixtures {
        let dir = output.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("sample.py"), text).unwrap();
        let result = index(&dir, &output.join(format!("{name}.sqlite")));
        if fixed || (name != "named-variadic" && name != "old-negative") {
            assert!(result.get("error").is_none(), "{name}: {result}");
            assert_eq!(result["report"]["files_parsed"], 1);
            assert!(result["report"]["parse_errors"].as_array().unwrap().is_empty());
        } else {
            assert!(result.to_string().contains("invalid dependency key"), "{name}: {result}");
            if result.get("error").is_none() { assert_eq!(result["dependency_count"], 0); }
        }
        if result.get("error").is_none() && !result["validated_manifests"].as_array().unwrap().is_empty() {
            let deps = result["validated_manifests"][0]["dependencies"].as_array().unwrap();
            for key in match name {"named-variadic" => vec!["packet"], "unicode-union" => vec!["订单","receipt"], "qualified" => vec!["domain.invoice","invoice"], _=>vec![]} {
                assert!(deps.iter().any(|d| d["kind"] == "name_bucket" && d["key"] == key), "{name} missing {key}: {deps:?}");
            }
        }
        results.push(json!({"name":name,"result":result}));
    }
    std::fs::write(output.join("results.json"),serde_json::to_vec_pretty(&json!({"fixed":fixed,"atom_controls":controls,"identifier_boundaries":boundaries,"punctuation":punctuation,"name_keys":keys,"strict_validator_error":direct_error,"fixtures":results})).unwrap()).unwrap();
    println!("independent controls completed, fixed={fixed}");
}
