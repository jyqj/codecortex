//! Broad fixed-seed real-parser mutations; truth assertions are hand-authored,
//! not generated from the actual graph. The global-name fixtures test the stated
//! heuristic contract, not compiler-valid cross-module programs.
use cc_eval::benchmark::{
    mutation_case::{self, FactAssertion, MutationCase, Stage},
    mutations::Mutation,
};
use serde_json::{json, Value};
use std::{collections::BTreeMap, path::Path};

fn assertion(id: &str, file: &str, callee: &str, target: Option<&str>) -> FactAssertion {
    FactAssertion {
        id: id.into(),
        table: "call_edges".into(),
        matches: [
            ("file_path".into(), json!(file)),
            ("callee_symbol".into(), json!(callee)),
            ("target_file_path".into(), json!(target)),
        ]
        .into(),
        count: 1,
    }
}
fn write(path: &str, text: &str) -> Mutation {
    Mutation::Write {
        path: path.into(),
        content: text.into(),
    }
}
fn stage(mutation: Mutation, assertions: Vec<FactAssertion>) -> Stage {
    Stage {
        mutation,
        reopen: false,
        settle: true,
        assertions,
    }
}
fn run(case: &MutationCase) {
    let temp = tempfile::tempdir().unwrap();
    let out = std::env::var("CODECORTEX_BENCH_OBSERVATIONS")
        .map(|p| Path::new(&p).join(&case.name))
        .unwrap_or_else(|_| temp.path().join("run"));
    let result = mutation_case::run(case, &out, 32).unwrap();
    assert_eq!(
        result["passed"],
        true,
        "{} {:?}; raw {}",
        case.name,
        result["failure_signature"],
        out.display()
    );
}
fn language_case(ext: &str, seed: u32) -> MutationCase {
    let api = format!("api.{ext}");
    let user = format!("use.{ext}");
    let moved = format!("moved.{ext}");
    let (original, body, signature, consumer) = match ext {
        "py" => (
            "def ping(x):\n    return x\n".into(),
            format!("def ping(x):\n    return x+{seed}\n"),
            "def ping(x,y=0):\n    return x+y\n".into(),
            "def run(x):\n    return ping(x)\n",
        ),
        "ts" => (
            "export function ping(x:number):number{return x;}\n".into(),
            format!("export function ping(x:number):number{{return x+{seed};}}\n"),
            "export function ping(x:number,y=0):number{return x+y;}\n".into(),
            "export function run(x:number){return ping(x);}\n",
        ),
        "rs" => (
            "pub fn ping(x:i32)->i32{x}\n".into(),
            format!("pub fn ping(x:i32)->i32{{x+{seed}}}\n"),
            "pub fn ping(x:i32,y:i32)->i32{x+y}\n".into(),
            "pub fn run(x:i32)->i32{ping(x)}\n",
        ),
        "go" => (
            "package p\nfunc ping(x int) int{return x}\n".into(),
            format!("package p\nfunc ping(x int) int{{return x+{seed}}}\n"),
            "package p\nfunc ping(x int,y int) int{return x+y}\n".into(),
            "package p\nfunc run(x int) int{return ping(x)}\n",
        ),
        _ => unreachable!(),
    };
    let original: String = original;
    let signature: String = signature;
    let mut stages = vec![
        stage(
            write(&api, &body),
            vec![assertion("body", &user, "ping", Some(&api))],
        ),
        stage(
            write(&api, &signature),
            vec![assertion("signature", &user, "ping", Some(&api))],
        ),
        stage(
            Mutation::Rename {
                from: api.clone(),
                to: moved.clone(),
            },
            vec![assertion("rename", &user, "ping", Some(&moved))],
        ),
        stage(
            write(&api, &signature),
            vec![assertion("collision", &user, "ping", None)],
        ),
        stage(
            Mutation::Delete { path: api.clone() },
            vec![assertion("unique_again", &user, "ping", Some(&moved))],
        ),
        stage(
            Mutation::Rename {
                from: moved.clone(),
                to: api.clone(),
            },
            vec![assertion("rename_back", &user, "ping", Some(&api))],
        ),
        stage(
            Mutation::Delete { path: api.clone() },
            vec![assertion("missing", &user, "ping", None)],
        ),
        stage(
            write(&api, &signature),
            vec![assertion("restore", &user, "ping", Some(&api))],
        ),
        stage(
            write(&user, consumer),
            vec![assertion("noop", &user, "ping", Some(&api))],
        ),
    ];
    for (i, s) in stages.iter_mut().enumerate() {
        s.reopen = (i + seed as usize).is_multiple_of(3);
    }
    MutationCase {
        schema_version: 1,
        name: format!("p2d-{ext}-{seed}"),
        initial: [(api, original), (user, consumer.into())].into(),
        stages,
        dirty_budget: 1,
        max_resume_builds: 32,
    }
}
#[test]
fn python_changes_have_independent_truth() {
    for seed in [1, 17, 91] {
        run(&language_case("py", seed));
    }
}
#[test]
fn typescript_changes_have_independent_truth() {
    for seed in [1, 17, 91] {
        run(&language_case("ts", seed));
    }
}
#[test]
fn rust_changes_have_independent_truth() {
    for seed in [1, 17, 91] {
        run(&language_case("rs", seed));
    }
}
#[test]
fn go_changes_have_independent_truth() {
    for seed in [1, 17, 91] {
        run(&language_case("go", seed));
    }
}

#[test]
fn pending_changes_rebase_and_shrinker_cannot_certify_incomplete() {
    let mut initial = BTreeMap::new();
    for p in ["a.py", "b.py", "c.py", "d.py"] {
        initial.insert(p.into(), "def run(x):\n    return ping(x)\n".into());
    }
    let mut first = stage(write("api.py", "def ping(x):\n    return x\n"), vec![]);
    first.settle = false;
    let mut second = stage(
        write("api.py", "def ping(x,y=0):\n    return x+y\n"),
        vec![],
    );
    second.settle = false;
    second.reopen = true;
    let third = stage(
        Mutation::Rename {
            from: "api.py".into(),
            to: "moved.py".into(),
        },
        ["a.py", "b.py", "c.py", "d.py"]
            .iter()
            .map(|p| assertion(p, p, "ping", Some("moved.py")))
            .collect(),
    );
    let case = MutationCase {
        schema_version: 1,
        name: "p2d-interleaved-debt".into(),
        initial,
        stages: vec![first, second, third],
        dirty_budget: 1,
        max_resume_builds: 32,
    };
    run(&case);
    let result = mutation_case::evaluate(&case).unwrap();
    assert_eq!(
        result["checkpoints"][0]["status"],
        "incomplete_not_certified"
    );
    assert_eq!(
        result["checkpoints"][1]["status"],
        "incomplete_not_certified"
    );
}
#[test]
fn bounded_shrink_preserves_exact_failure_and_replays_standalone() {
    // Deliberately wrong expectation tests the reducer, not a product bug.
    let mut case = language_case("py", 1);
    case.name = "p2d-reducer-selftest".into();
    for stage in &mut case.stages {
        stage.assertions.clear();
    }
    case.stages
        .last_mut()
        .unwrap()
        .assertions
        .push(FactAssertion {
            id: "deliberate_false_truth".into(),
            table: "call_edges".into(),
            matches: [("file_path".into(), json!("use.py"))].into(),
            count: 777,
        });
    let out = tempfile::tempdir().unwrap();
    let result = mutation_case::run(&case, &out.path().join("run"), 32).unwrap();
    assert_eq!(result["passed"], false);
    assert_eq!(result["shrink"]["reproduced"], true);
    let minimal: MutationCase =
        serde_json::from_slice(&std::fs::read(out.path().join("run/minimal-case.json")).unwrap())
            .unwrap();
    assert!(minimal.stages.len() < case.stages.len());
    assert_eq!(
        mutation_case::evaluate(&minimal).unwrap()["failure_signature"],
        result["failure_signature"]
    );
    assert!(mutation_case::run(&case, &out.path().join("run"), 32).is_err());
    let (_, receipt) = mutation_case::shrink(&case, "truth:deliberate_false_truth", 0).unwrap();
    assert_eq!(receipt["attempts"], 0);
    assert_eq!(receipt["stage_deletion_minimal"], false);
    assert!(mutation_case::shrink(&case, "another_failure", 1).is_err());
}
#[test]
fn unknown_surface_is_not_reported_as_a_proven_empty_interface() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("api.py"), "__all__ = generate_exports()\n").unwrap();
    let mut index = cc_server::engine::CodeIndex::new(Some(d.path())).unwrap();
    let report = index.build_index(true).unwrap();
    assert!(report.public_surface_coverage.unknown > 0);
    let raw = cc_eval::benchmark::oracle::canonical(d.path()).unwrap();
    let surface = &raw["public_surfaces"][0];
    assert!(surface["fingerprint"].is_null());
    let payload: Value = serde_json::from_str(surface["payload"].as_str().unwrap()).unwrap();
    assert_eq!(payload["knowledge"], "unknown");
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires explicit product binary; actual public MCP process"]
async fn p2d_public_mcp_multilanguage_rename_truth_and_no_fake_jsts_calls() {
    use cc_eval::benchmark::adapters::{mcp_stdio::McpStdio, Backend};
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join(".codecortex.json"),r#"{"auto_index":{"enabled":false},"indexing":{"dirty_propagation_max_files":1,"db_read_pool_size":1}}"#).unwrap();
    let cases = [
        (
            "py",
            "def py_marker(x):\n    return x\n",
            "def run_py(x):\n    return py_marker(x)\n",
            "py_marker",
        ),
        (
            "ts",
            "export function ts_marker(x:number){return x;}\n",
            "export function run_ts(x:number){return ts_marker(x);}\n",
            "ts_marker",
        ),
        (
            "rs",
            "pub fn rs_marker(x:i32)->i32{x}\n",
            "pub fn run_rs(x:i32)->i32{rs_marker(x)}\n",
            "rs_marker",
        ),
        (
            "go",
            "package p\nfunc go_marker(x int) int{return x}\n",
            "package p\nfunc run_go(x int) int{return go_marker(x)}\n",
            "go_marker",
        ),
    ];
    for (ext, api, consumer, _) in cases {
        std::fs::write(d.path().join(format!("api.{ext}")), api).unwrap();
        std::fs::write(d.path().join(format!("use.{ext}")), consumer).unwrap();
    }
    std::fs::write(
        d.path().join("fake.ts"),
        "function fake(){ // fake()\n const text='fake()'; return text; }\n",
    )
    .unwrap();
    let binary = std::path::PathBuf::from(std::env::var("CODECORTEX_BENCH_BINARY").unwrap());
    let mut m = McpStdio::spawn(&binary, d.path(), std::time::Duration::from_secs(30))
        .await
        .unwrap();
    m.call("index", json!({"path":d.path(),"full":true}))
        .await
        .unwrap();
    let mut observations = Vec::new();
    for (ext, _, _, name) in cases {
        std::fs::rename(
            d.path().join(format!("api.{ext}")),
            d.path().join(format!("moved.{ext}")),
        )
        .unwrap();
        m.close().await.unwrap();
        m = McpStdio::spawn(&binary, d.path(), std::time::Duration::from_secs(30))
            .await
            .unwrap();
        let mut report = Value::Null;
        for tick in 0..12 {
            report = m
                .call("index", json!({"path":d.path(),"full":false}))
                .await
                .unwrap();
            if report["resolution_freshness"]["complete"] == true {
                break;
            }
            assert!(tick < 11, "MCP debt did not settle");
        }
        let query=format!("MATCH (a:Symbol)-[:CALLS]->(b:Symbol) WHERE a.file_path = 'use.{ext}' AND b.name = '{name}' RETURN b.file_path AS path LIMIT 20");
        let graph = m.call("graph_query", json!({"query":query})).await.unwrap();
        assert_eq!(
            graph["results"],
            json!([{"path":format!("moved.{ext}")}]),
            "{graph}"
        );
        observations.push(json!({"language":ext,"index":report,"graph":graph}));
    }
    let graph=m.call("graph_query",json!({"query":"MATCH (a:Symbol)-[:CALLS]->(b:Symbol) WHERE a.file_path = 'fake.ts' RETURN b.name AS target LIMIT 20"})).await.unwrap();
    assert_eq!(graph["results"], json!([]));
    let out = std::env::var("CODECORTEX_BENCH_OBSERVATIONS").unwrap();
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(
        Path::new(&out).join("p2d-public-mcp.json"),
        serde_json::to_vec_pretty(&observations).unwrap(),
    )
    .unwrap();
    m.close().await.unwrap();
}

#[test]
fn mutation_inputs_fail_closed() {
    let mut typo = language_case("py", 1);
    typo.stages[0].assertions[0].table = "dispatch_sites".into();
    typo.stages[0].assertions[0].matches = [("nonexistent_column".into(), json!(0))].into();
    typo.stages[0].assertions[0].count = 0;
    assert!(
        mutation_case::evaluate(&typo).is_err(),
        "zero rows cannot hide a typo"
    );
    let mut case = language_case("py", 1);
    case.initial.insert("../escape.py".into(), "".into());
    assert!(case.validate().is_err());
    let mut case = language_case("py", 1);
    case.stages.last_mut().unwrap().settle = false;
    assert!(case.validate().is_err());
    let mut case = language_case("py", 1);
    case.stages[0].assertions[0].table = "not_a_table".into();
    assert!(case.validate().is_err());
    let mut v = serde_json::to_value(language_case("py", 1)).unwrap();
    v["unknown"] = Value::Bool(true);
    assert!(serde_json::from_value::<MutationCase>(v).is_err());
}
