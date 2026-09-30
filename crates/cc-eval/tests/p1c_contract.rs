//! Entire advertised MCP surface: valid calls, strict unknown keys and legacy modes.
use cc_server::tools::*;
use serde_json::json;
#[test]
fn all_fourteen_parameter_types_deny_unknown_and_keep_default_requests() {
    macro_rules! check {
        ($ty:ty,$v:expr) => {{
            let valid = $v;
            let mut p: $ty = serde_json::from_value(valid.clone()).unwrap();
            p.sanitize().unwrap();
            for unknown in ["unexpected_field", "hard_scope", "dense_model"] {
                let mut bad = valid.clone();
                bad[unknown] = json!(true);
                assert!(serde_json::from_value::<$ty>(bad).is_err(), stringify!($ty));
            }
        }};
    }
    check!(StatusParams, json!({}));
    check!(IndexParams, json!({"path":"."}));
    check!(SearchParams, json!({"query":"needle"}));
    check!(ContextParams, json!({"task":"needle"}));
    check!(NodeParams, json!({"symbol":"needle"}));
    check!(ExploreParams, json!({"symbols":["needle"]}));
    check!(TraceParams, json!({"from":"a","to":"b"}));
    check!(RelationsParams, json!({"symbol":"needle"}));
    check!(ImpactParams, json!({"scope":"dead_code"}));
    check!(ArchitectureParams, json!({}));
    check!(FilesParams, json!({}));
    check!(
        GraphQueryParams,
        json!({"query":"MATCH (s:Symbol) RETURN s.name LIMIT 1"})
    );
    check!(IngestTracesParams, json!({"traces":[]}));
    check!(AdrParams, json!({}));
}
#[test]
fn search_modes_optional_nulls_and_utf8_sanitize_are_stable() {
    for mode in ["hybrid", "symbol"] {
        let mut p:SearchParams=serde_json::from_value(json!({"query":"中文needle","mode":mode,"path_prefix":null,"boost_files":[],"top_k":0,"file_preselect_limit":0})).unwrap();
        p.sanitize().unwrap();
        assert_eq!(p.top_k, 1);
        assert_eq!(p.file_preselect_limit, Some(1));
        assert!(p.path_prefix.is_none());
    }
    for mode in ["dense", "semantic", "HYBRID", ""] {
        let mut p = SearchParams {
            query: "needle".into(),
            mode: mode.into(),
            ..Default::default()
        };
        assert!(p.sanitize().is_err());
    }
    let mut p = SearchParams {
        query: "中文".repeat(3000),
        top_k: usize::MAX,
        ..Default::default()
    };
    p.sanitize().unwrap();
    assert!(p.query.len() <= 4096);
    assert_eq!(p.top_k, 200);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires CODECORTEX_BENCH_BINARY; full real stdio contract matrix"]
async fn p1c_fourteen_tools_valid_unknown_schema_and_modes() {
    use rmcp::{
        model::CallToolRequestParams,
        transport::{ConfigureCommandExt, TokioChildProcess},
        ServiceExt,
    };
    use std::{process::Stdio, time::Duration};
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir(d.path().join("src")).unwrap();
    std::fs::write(
        d.path().join(".codecortex.json"),
        r#"{"auto_index":{"enabled":false}}"#,
    )
    .unwrap();
    std::fs::write(d.path().join("src/main.py"),"def parse_field(raw):\n    return int(raw)\n\ndef entrypoint(raw):\n    return parse_field(raw)\n").unwrap();
    let bin = std::env::var("CODECORTEX_BENCH_BINARY").unwrap();
    let transport = TokioChildProcess::new(tokio::process::Command::new(bin).configure(|cmd| {
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("CODECORTEX_") {
                cmd.env_remove(key);
            }
        }
        cmd.arg("mcp")
            .arg("--project-path")
            .arg(d.path())
            .current_dir(d.path())
            .env("HOME", d.path())
            .env("XDG_CONFIG_HOME", d.path().join(".config"))
            .env("XDG_CACHE_HOME", d.path().join(".cache"))
            .env("CODECORTEX_PPID_POLL_MS", "0")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
    }))
    .unwrap();
    let c = tokio::time::timeout(Duration::from_secs(20), ().serve(transport))
        .await
        .unwrap()
        .unwrap();
    let tools = c.list_all_tools().await.unwrap();
    let cases = vec![
        ("index", json!({"path":d.path(),"full":true})),
        ("status", json!({})),
        ("search", json!({"query":"parse_field"})),
        ("context", json!({"task":"parse_field"})),
        ("node", json!({"symbol":"parse_field","include":"source"})),
        ("explore", json!({"symbols":["parse_field"]})),
        ("trace", json!({"from":"entrypoint","to":"parse_field"})),
        ("relations", json!({"symbol":"parse_field"})),
        ("impact", json!({"scope":"dead_code"})),
        ("architecture", json!({})),
        ("files", json!({"action":"list"})),
        (
            "graph_query",
            json!({"query":"MATCH (s:Symbol) RETURN s.name LIMIT 3"}),
        ),
        ("ingest_traces", json!({"traces":[]})),
        ("adr", json!({"action":"list"})),
    ];
    let actual: std::collections::BTreeSet<_> = tools.iter().map(|t| t.name.as_ref()).collect();
    assert_eq!(actual, cases.iter().map(|(name, _)| *name).collect());
    let mut receipts = Vec::new();
    for (name, args) in &cases {
        let tool = tools.iter().find(|t| t.name == *name).unwrap();
        assert_eq!(
            tool.input_schema.get("additionalProperties"),
            Some(&json!(false)),
            "schema {name}"
        );
        let request = CallToolRequestParams::new(name.to_string())
            .with_arguments(args.as_object().unwrap().clone());
        let result = tokio::time::timeout(Duration::from_secs(20), c.call_tool(request))
            .await
            .unwrap()
            .unwrap();
        assert_ne!(result.is_error, Some(true), "valid {name}: {result:?}");
        assert!(
            result
                .structured_content
                .as_ref()
                .is_some_and(|v| v.get("result").is_some()),
            "{name}"
        );
        let mut bad = args.clone();
        bad["unexpected_field"] = json!(true);
        let err = tokio::time::timeout(
            Duration::from_secs(20),
            c.call_tool(
                CallToolRequestParams::new(name.to_string())
                    .with_arguments(bad.as_object().unwrap().clone()),
            ),
        )
        .await
        .unwrap();
        let rejected = err.expect("existing SDK reports deserialize rejection as a tool result");
        assert_eq!(
            rejected.is_error,
            Some(true),
            "unknown {name}: {rejected:?}"
        );
        assert!(rejected.structured_content.is_none());
        let rejection = serde_json::to_value(&rejected).unwrap();
        assert!(rejection.to_string().contains("unknown field"));
        receipts.push(json!({"tool":name,"valid":true,"unknown_error":"tool_is_error_deserialize","input_schema":tool.input_schema}));
    }
    for (mode, is_array) in [("hybrid", false), ("symbol", true)] {
        let r = c
            .call_tool(
                CallToolRequestParams::new("search").with_arguments(
                    json!({"query":"parse_field","mode":mode,"path_prefix":"src","top_k":1})
                        .as_object()
                        .unwrap()
                        .clone(),
                ),
            )
            .await
            .unwrap();
        let value = &r.structured_content.as_ref().unwrap()["result"];
        assert_eq!(value.is_array(), is_array);
    }
    let bad = c
        .call_tool(
            CallToolRequestParams::new("search").with_arguments(
                json!({"query":"parse_field","mode":"semantic"})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await;
    assert!(matches!(bad,Err(rmcp::ServiceError::McpError(e)) if e.code.0==-32602));
    if let Ok(path) = std::env::var("CODECORTEX_CONTRACT_RECEIPT") {
        std::fs::write(path, serde_json::to_vec_pretty(&receipts).unwrap()).unwrap();
    }
    println!("P1C_MATRIX tools=14 valid=14 unknown=tool_is_error_x14 invalid_mode=-32602 modes=hybrid,symbol");
    c.cancel().await.unwrap();
}
