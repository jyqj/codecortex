//! Additive P5-D wire policies: old modes remain strict and offline.
use cc_server::tools::{parse_retrieval_strategy, ContextParams, SearchParams};
use serde_json::{json, Value};

fn examples() -> Vec<Value> {
    let text = include_str!("../../../docs/MCP_TOOLS.md");
    let section = text
        .split("<!-- p5d-query-contract:start -->")
        .nth(1)
        .unwrap()
        .split("<!-- p5d-query-contract:end -->")
        .next()
        .unwrap();
    serde_json::from_str(
        section
            .split("```json\n")
            .nth(1)
            .unwrap()
            .split("```")
            .next()
            .unwrap(),
    )
    .unwrap()
}
#[test]
fn policy_parameters_are_optional_strict_and_documented() {
    for strategy in [None, Some("local"), Some("auto"), Some("semantic")] {
        let mut search: SearchParams =
            serde_json::from_value(json!({"query":"needle","retrieval_strategy":strategy}))
                .unwrap();
        search.sanitize().unwrap();
        let mut context: ContextParams =
            serde_json::from_value(json!({"task":"needle","retrieval_strategy":strategy})).unwrap();
        context.sanitize().unwrap();
        assert_eq!(
            parse_retrieval_strategy(strategy).unwrap(),
            parse_retrieval_strategy(search.retrieval_strategy.as_deref()).unwrap()
        );
    }
    for strategy in ["dense", "AUTO", "", "local "] {
        let mut p: SearchParams =
            serde_json::from_value(json!({"query":"needle","retrieval_strategy":strategy}))
                .unwrap();
        assert!(p.sanitize().is_err());
        let mut p: ContextParams =
            serde_json::from_value(json!({"task":"needle","retrieval_strategy":strategy})).unwrap();
        assert!(p.sanitize().is_err());
    }
    for strategy in ["auto", "semantic"] {
        let mut p: SearchParams = serde_json::from_value(
            json!({"query":"needle","mode":"symbol","retrieval_strategy":strategy}),
        )
        .unwrap();
        assert!(p.sanitize().is_err());
    }
    let mut p: SearchParams = serde_json::from_value(
        json!({"query":"needle","mode":"symbol","retrieval_strategy":"local"}),
    )
    .unwrap();
    p.sanitize().unwrap();
    for row in examples() {
        match row["tool"].as_str().unwrap() {
            "search" => {
                let mut p: SearchParams = serde_json::from_value(row["args"].clone()).unwrap();
                p.sanitize().unwrap();
            }
            "context" => {
                let mut p: ContextParams = serde_json::from_value(row["args"].clone()).unwrap();
                p.sanitize().unwrap();
            }
            other => panic!("unexpected documented tool {other}"),
        }
    }
}
#[tokio::test]
#[ignore = "explicit CODECORTEX_BENCH_BINARY; actual public policy and capabilities contract"]
async fn real_stdio_policy_override_and_capability_state() {
    use cc_eval::benchmark::adapters::{mcp_stdio::McpStdio, Backend};
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(".codecortex.json"),
        r#"{"auto_index":{"enabled":false},"query":{"strategy":"local"}}"#,
    )
    .unwrap();
    std::fs::write(dir.path().join("a.py"), "def needle():\n    return 7\n").unwrap();
    let binary = std::path::PathBuf::from(std::env::var("CODECORTEX_BENCH_BINARY").unwrap());
    let mut client = McpStdio::spawn(&binary, dir.path(), std::time::Duration::from_secs(20))
        .await
        .unwrap();
    client
        .call("index", json!({"path":dir.path(),"full":true}))
        .await
        .unwrap();
    let caps = client
        .call("status", json!({"aspect":"capabilities"}))
        .await
        .unwrap();
    assert_eq!(caps["retrieval"]["semantic_state"], "not_configured");
    assert_eq!(caps["retrieval"]["dense_state"], "disabled");
    assert_eq!(caps["retrieval"]["query_coverage"]["state"], "not_measured");
    let mut observations = Vec::new();
    for row in examples() {
        let tool = row["tool"].as_str().unwrap();
        let value = client.call(tool, row["args"].clone()).await.unwrap();
        if row["args"]["mode"] == "symbol" {
            assert!(value.is_array());
        } else {
            let policy = &value["evidence_summary"]["retrieval"]["policy"];
            assert_eq!(policy["effective"], "local");
            assert!(!value["machine_pack"]["hits"].as_array().unwrap().is_empty());
            if row["args"]["retrieval_strategy"] == "auto" {
                assert_eq!(policy["requested"], "auto");
                assert_eq!(policy["semantic_state"], "not_configured");
            }
            assert!(serde_json::to_vec(&value).unwrap().len() <= 16000);
        }
        observations.push(json!({"request":row,"passed":true}));
    }
    for tool in ["search", "context"] {
        let mut args = if tool == "search" {
            json!({"query":"needle"})
        } else {
            json!({"task":"needle"})
        };
        args["retrieval_strategy"] = json!("semantic");
        let error = client.call(tool, args).await.unwrap_err();
        assert!(error.to_string().to_lowercase().contains("semantic"));
    }
    for args in [
        json!({"query":"needle","retrieval_strategy":"unknown"}),
        json!({"query":"needle","mode":"symbol","retrieval_strategy":"auto"}),
    ] {
        assert!(client.call("search", args).await.is_err());
    }
    client.close().await.unwrap();
    if let Ok(out) = std::env::var("CODECORTEX_BENCH_OBSERVATIONS") {
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(std::path::Path::new(&out).join("p5d-public-policy.json"),serde_json::to_vec_pretty(&json!({"capabilities":caps,"calls":observations,"semantic_unavailable_tested":true,"legacy_modes_preserved":true})).unwrap()).unwrap();
    }
}
