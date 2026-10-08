use cc_eval::benchmark::{
    adapters::{mcp_stdio::McpStdio, Backend},
    manifest, normalizer,
    readiness::State,
    schema::SearchInput,
};
use serde_json::json;
use std::{path::PathBuf, time::Duration};
#[cfg(target_os = "linux")]
#[path = "support/p7_procfs.rs"]
mod p7_procfs;
fn project() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("a.py"),
        "def renew_session():\n    return 1\n",
    )
    .unwrap();
    std::fs::write(
        d.path().join(".codecortex.json"),
        "{\"auto_index\":{\"enabled\":false}}",
    )
    .unwrap();
    d
}
#[tokio::test]
#[ignore = "requires CODECORTEX_BENCH_BINARY pointing at an explicitly built product"]
async fn real_stdio_index_search_scope_and_tool_error() {
    let binary = PathBuf::from(
        std::env::var("CODECORTEX_BENCH_BINARY").expect("explicit product binary required"),
    );
    let d = project();
    let files = manifest::inventory(d.path(), &["a.py".into()]).unwrap();
    let mut client = McpStdio::spawn(&binary, d.path(), Duration::from_secs(15))
        .await
        .unwrap();
    assert!(client.pid().is_some());
    client.prepare(&files).await.unwrap();
    assert_eq!(client.readiness(&files).await.unwrap().state, State::Ready);
    let v = client
        .search(&SearchInput {
            query: "renew_session".into(),
            top_k: 10,
            path_prefix: None,
        })
        .await
        .unwrap();
    let (h, _) = normalizer::mcp(&v).unwrap();
    assert!(h.iter().any(|h| h.path == "a.py"));
    let v = client
        .search(&SearchInput {
            query: "renew_session".into(),
            top_k: 10,
            path_prefix: Some("no-such-dir/".into()),
        })
        .await
        .unwrap();
    assert!(normalizer::mcp(&v).unwrap().0.is_empty());
    assert!(client.call("search", json!({"qurey":"bad"})).await.is_err());
    client.close().await.unwrap();
    assert!(client
        .search(&SearchInput {
            query: "x".into(),
            top_k: 1,
            path_prefix: None
        })
        .await
        .is_err());
}
#[cfg(unix)]
#[tokio::test]
async fn exited_child_is_protocol_error_not_empty_search() {
    let d = project();
    assert!(McpStdio::spawn(
        std::path::Path::new("/usr/bin/true"),
        d.path(),
        Duration::from_millis(500)
    )
    .await
    .is_err());
}

mod p7_offline {
    use super::*;
    use rmcp::{
        model::{CallToolRequestParams, ErrorCode},
        service::{RunningService, ServiceError},
        RoleClient, ServiceExt,
    };
    use serde_json::Value;
    use std::{collections::BTreeSet, path::Path, process::Stdio};

    type Client = RunningService<RoleClient, ()>;
    fn verify_build_receipt(binary: &Path, package: &str, receipt: &Value) -> Result<(), String> {
        use sha2::{Digest, Sha256};
        let reject = || Err("product build receipt does not match binary/package/features".into());
        let expected_features = match package {
            "default" => json!([]),
            "semantic" => json!(["semantic"]),
            _ => return reject(),
        };
        let artifact = &receipt["cargo_artifact"];
        let mut expected_command = vec![
            "cargo",
            "build",
            "-p",
            "cc-server",
            "--bin",
            "codecortex",
            "--no-default-features",
            "--locked",
            "--message-format=json-render-diagnostics",
        ];
        if package == "semantic" {
            expected_command.extend(["--features", "semantic"]);
        }
        let actual_command = &receipt["build_command"];
        if actual_command != &json!(expected_command) {
            expected_command.push("--offline");
            if actual_command != &json!(expected_command) {
                return reject();
            }
        }
        if receipt["schema_version"] != 1
            || receipt["build_exit_code"] != 0
            || receipt["package_kind"] != package
            || artifact["reason"] != "compiler-artifact"
            || artifact["target"]["name"] != "codecortex"
            || artifact["target"]["kind"] != json!(["bin"])
            || artifact["features"] != expected_features
            || artifact["executable"].as_str().is_none_or(str::is_empty)
            || !artifact["package_id"].as_str().is_some_and(|id| {
                id.rsplit_once('#').is_some_and(|(source, version)| {
                    source.ends_with("/cc-server") && !version.is_empty()
                })
            })
        {
            return reject();
        }
        let path = receipt["binary_path"]
            .as_str()
            .ok_or_else(|| "missing receipt binary path".to_string())?;
        if Path::new(path).canonicalize().map_err(|e| e.to_string())?
            != binary.canonicalize().map_err(|e| e.to_string())?
        {
            return reject();
        }
        let digest = format!(
            "{:x}",
            Sha256::digest(std::fs::read(binary).map_err(|e| e.to_string())?)
        );
        if receipt["binary_sha256"] != digest {
            return reject();
        }
        Ok(())
    }

    #[test]
    fn build_receipt_rejects_identity_feature_and_binary_mismatch() {
        use sha2::{Digest, Sha256};
        let dir = tempfile::tempdir().unwrap();
        let binary = dir.path().join("codecortex");
        std::fs::write(&binary, b"synthetic receipt-validator bytes, not a product").unwrap();
        let digest = format!("{:x}", Sha256::digest(std::fs::read(&binary).unwrap()));
        let receipt = json!({"schema_version":1,"build_exit_code":0,"package_kind":"default","build_command":["cargo","build","-p","cc-server","--bin","codecortex","--no-default-features","--locked","--message-format=json-render-diagnostics"],"binary_path":binary,"binary_sha256":digest,"cargo_artifact":{"reason":"compiler-artifact","package_id":"path+file:///synthetic/cc-server#cc-server@1.0.0","target":{"name":"codecortex","kind":["bin"]},"features":[],"executable":"/synthetic/target/codecortex"}});
        verify_build_receipt(&binary, "default", &receipt).unwrap();
        assert!(verify_build_receipt(&binary, "semantic", &receipt).is_err());
        assert!(verify_build_receipt(&binary, "default", &json!({})).is_err());
        for (pointer, replacement) in [
            ("/package_kind", json!("semantic")),
            ("/cargo_artifact/features", json!(["semantic"])),
            (
                "/cargo_artifact/package_id",
                json!("path+file:///synthetic/other#other@1.0.0"),
            ),
            ("/cargo_artifact/target/kind", json!(["lib"])),
            ("/cargo_artifact/target/name", json!("other")),
            ("/build_exit_code", json!(101)),
            (
                "/build_command",
                json!(["cargo", "build", "--features", "semantic"]),
            ),
            ("/binary_sha256", json!("wrong")),
        ] {
            let mut bad = receipt.clone();
            *bad.pointer_mut(pointer).unwrap() = replacement;
            assert!(
                verify_build_receipt(&binary, "default", &bad).is_err(),
                "accepted mismatch: {pointer}"
            );
        }
        let other = dir.path().join("other");
        std::fs::write(&other, b"synthetic other binary").unwrap();
        assert!(verify_build_receipt(&other, "default", &receipt).is_err());
        std::fs::write(&binary, b"mutated after receipt").unwrap();
        assert!(verify_build_receipt(&binary, "default", &receipt).is_err());
    }
    const SOURCE: &str = "pub fn parse_field(raw: &str) -> i32 {\n    raw.trim().parse().unwrap_or(0)\n}\n\npub fn validate_payload(raw: &str) -> bool {\n    parse_field(raw) > 0\n}\n\npub fn renew_session(raw: &str) -> bool {\n    validate_payload(raw)\n}\n";
    const TOOLS: [&str; 14] = [
        "status",
        "index",
        "search",
        "context",
        "node",
        "explore",
        "trace",
        "relations",
        "impact",
        "architecture",
        "files",
        "graph_query",
        "ingest_traces",
        "adr",
    ];

    fn file_digest(path: &Path) -> String {
        use sha2::{Digest, Sha256};
        format!("{:x}", Sha256::digest(std::fs::read(path).unwrap()))
    }

    enum SpawnPolicy {
        Direct,
        KillOnNetwork {
            launcher: PathBuf,
            digest: String,
            probes: Value,
        },
    }

    impl SpawnPolicy {
        fn verified_network_guard() -> Self {
            assert!(
                cfg!(target_os = "linux"),
                "isolated offline acceptance requires Linux seccomp"
            );
            let launcher = PathBuf::from(
                std::env::var("P7_017_NETWORK_GUARD")
                    .expect("explicit product network launcher required"),
            )
            .canonicalize()
            .unwrap();
            let digest = file_digest(&launcher);
            let evidence = tempfile::tempdir().unwrap();
            let output = std::process::Command::new("python3")
                .arg(&launcher)
                .arg("--verify-kill-policy")
                .arg(evidence.path().join("probes"))
                .env_clear()
                .env("PATH", std::env::var_os("PATH").unwrap())
                .stdin(Stdio::null())
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "network kill probes failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let probes: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(probes["status"], "passed");
            assert_eq!(probes["wrapper_sha256"], digest);
            for family in ["ipv4", "ipv6"] {
                assert_eq!(probes["probes"][family]["reached_socket"], true);
                assert_eq!(probes["probes"][family]["exit_code"], -libc::SIGSYS);
            }
            Self::KillOnNetwork {
                launcher,
                digest,
                probes,
            }
        }

        fn proof(&self) -> Value {
            match self {
                Self::Direct => Value::Null,
                Self::KillOnNetwork { probes, .. } => probes.clone(),
            }
        }
    }

    struct ProductChild {
        process: tokio::process::Child,
        guard_receipt: Value,
    }

    async fn spawn(binary: &Path, root: &Path, policy: &SpawnPolicy) -> (Client, ProductChild) {
        let evidence = tempfile::tempdir().unwrap();
        let receipt_path = evidence.path().join("pre-exec.json");
        let mut command = match policy {
            SpawnPolicy::Direct => tokio::process::Command::new(binary),
            SpawnPolicy::KillOnNetwork {
                launcher, digest, ..
            } => {
                assert_eq!(&file_digest(launcher), digest, "network launcher changed");
                let mut command = tokio::process::Command::new("python3");
                command
                    .arg(launcher)
                    .args(["--action", "kill", "--receipt"])
                    .arg(&receipt_path)
                    .arg("--")
                    .arg(binary);
                command
            }
        };
        // Exact original child boundary: no keys or developer overrides.
        // The launcher execs the separately receipt-verified product; it is
        // never substituted for that product in binary identity checks.
        command
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap())
            .env("HOME", root)
            .env("XDG_CONFIG_HOME", root.join(".config"))
            .env("XDG_CACHE_HOME", root.join(".cache"))
            .env("CODECORTEX_PPID_POLL_MS", "0")
            .arg("mcp")
            .arg("--project-path")
            .arg(root)
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let mut process = command.spawn().unwrap();
        let transport = (
            process.stdout.take().unwrap(),
            process.stdin.take().unwrap(),
        );
        let client = tokio::time::timeout(Duration::from_secs(20), ().serve(transport))
            .await
            .unwrap()
            .unwrap();
        let guard_receipt = match policy {
            SpawnPolicy::Direct => Value::Null,
            SpawnPolicy::KillOnNetwork { digest, .. } => {
                let receipt: Value =
                    serde_json::from_slice(&std::fs::read(receipt_path).unwrap()).unwrap();
                assert_eq!(receipt["action"], "kill");
                assert_eq!(receipt["result"], "filter_loaded_before_exec");
                assert_eq!(receipt["wrapper_sha256"], *digest);
                assert_eq!(receipt["exec_sha256"], file_digest(binary));
                assert_eq!(
                    receipt["exec_command"],
                    json!([binary, "mcp", "--project-path", root])
                );
                assert_eq!(receipt["socket_fds_before_exec"], json!([]));
                receipt
            }
        };
        (
            client,
            ProductChild {
                process,
                guard_receipt,
            },
        )
    }

    async fn close(client: Client, mut child: ProductChild) -> Value {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        tokio::time::timeout_at(deadline, client.cancel())
            .await
            .unwrap()
            .unwrap();
        let status = tokio::time::timeout_at(deadline, child.process.wait())
            .await
            .unwrap()
            .unwrap();
        assert!(
            status.success(),
            "product must exit normally, including after the last response: {status}"
        );
        json!({"exit_code":status.code(), "success":true})
    }

    async fn raw(
        client: &Client,
        tool: &str,
        args: Value,
    ) -> Result<rmcp::model::CallToolResult, ServiceError> {
        tokio::time::timeout(
            Duration::from_secs(20),
            client.call_tool(
                CallToolRequestParams::new(tool.to_owned())
                    .with_arguments(args.as_object().unwrap().clone()),
            ),
        )
        .await
        .expect("stdio tool deadline")
    }

    async fn call(
        client: &Client,
        tool: &str,
        args: Value,
        called: &mut BTreeSet<String>,
    ) -> Value {
        let result = raw(client, tool, args).await.unwrap();
        assert_ne!(result.is_error, Some(true), "{tool}: {result:?}");
        called.insert(tool.into());
        result
            .structured_content
            .expect("complete structured MCP result")["result"]
            .clone()
    }

    fn files(root: &Path) -> Vec<String> {
        fn walk(root: &Path, path: &Path, out: &mut Vec<String>) {
            for entry in std::fs::read_dir(path).unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                if entry.file_type().unwrap().is_dir() {
                    walk(root, &path, out);
                } else {
                    out.push(
                        path.strip_prefix(root)
                            .unwrap()
                            .to_string_lossy()
                            .replace('\\', "/"),
                    );
                }
            }
        }
        let mut out = Vec::new();
        walk(root, root, &mut out);
        out.sort();
        assert!(!root.join(".cache/codecortex/semantic").exists());
        assert!(!root.join("Library/Caches/codecortex/semantic").exists());
        assert!(out
            .iter()
            .all(|path| !path.contains("semantic-cache.sqlite")
                && !path.split('/').any(|part| part.starts_with("namespace-"))));
        out
    }

    fn network_status(child: &ProductChild) -> Value {
        let pid = child
            .process
            .id()
            .expect("actual live product subprocess PID");
        #[cfg(target_os = "linux")]
        {
            let rows =
                super::p7_procfs::child_security(Path::new("/proc"), std::process::id(), pid)
                    .expect(
                    "positive direct-child procfs identity required; unavailable is not isolated",
                );
            if !child.guard_receipt.is_null() {
                for field in ["NoNewPrivs", "Seccomp", "Seccomp_filters", "NSpid"] {
                    assert_eq!(json!(rows[field]), child.guard_receipt["after"][field]);
                }
                assert_eq!(json!(rows["ProcPid"]), child.guard_receipt["after"]["Pid"]);
            }
            json!(rows)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = pid;
            json!({"status":"not_observed_non_linux"})
        }
    }

    #[tokio::test]
    #[ignore = "explicit default/semantic product binary; run with --include-ignored, optionally under verified per-process network denial"]
    async fn real_stdio_default_disabled_semantic_contract() {
        let binary = PathBuf::from(
            std::env::var("CODECORTEX_BENCH_BINARY")
                .expect("explicit built product binary required"),
        )
        .canonicalize()
        .unwrap();
        let package = std::env::var("P7_017_PACKAGE_KIND")
            .expect("default or semantic build receipt required");
        assert!(matches!(package.as_str(), "default" | "semantic"));
        let receipt_path = std::env::var("P7_017_BUILD_RECEIPT")
            .expect("explicit Cargo product build receipt required");
        let receipt: Value =
            serde_json::from_slice(&std::fs::read(&receipt_path).unwrap()).unwrap();
        verify_build_receipt(&binary, &package, &receipt).expect("verified product build identity");
        let restricted =
            std::env::var("P7_017_NETWORK_POLICY").as_deref() == Ok("process_seccomp_deny_network");
        if restricted {
            for address in ["127.0.0.1:0", "[::1]:0"] {
                assert_eq!(
                    std::net::TcpListener::bind(address)
                        .unwrap_err()
                        .raw_os_error(),
                    Some(1),
                    "actual IPv4/IPv6 socket probe must fail before claiming network denial"
                );
            }
        }
        let policy = if restricted {
            SpawnPolicy::verified_network_guard()
        } else {
            SpawnPolicy::Direct
        };
        let expected: BTreeSet<String> = TOOLS.iter().map(|name| (*name).into()).collect();
        let mut observations = Vec::new();
        for explicitly_disabled in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path();
            std::fs::create_dir(root.join("src")).unwrap();
            std::fs::write(root.join("src/lib.rs"), SOURCE).unwrap();
            std::fs::write(
                root.join("Cargo.toml"),
                "[package]\nname=\"offline-probe\"\nversion=\"0.0.0\"\nedition=\"2021\"\n",
            )
            .unwrap();
            let mut config = json!({"auto_index":{"enabled":false}});
            if explicitly_disabled {
                config["semantic"] = json!({"enabled":false,"endpoint":"https://provider.invalid/v1","api_key_ref":"env:P7_017_ABSENT_KEY","model_id":"must-not-contact"});
            }
            std::fs::write(
                root.join(".codecortex.json"),
                serde_json::to_vec(&config).unwrap(),
            )
            .unwrap();
            let (client, child) = spawn(&binary, root, &policy).await;
            let child_network = network_status(&child);
            let child_guard = child.guard_receipt.clone();
            if restricted {
                assert_eq!(child_network["NoNewPrivs"], "1");
                assert_eq!(child_network["Seccomp"], "2");
                assert!(
                    child_network["Seccomp_filters"]
                        .as_str()
                        .unwrap()
                        .parse::<usize>()
                        .unwrap()
                        >= 2
                );
            }
            let listed = tokio::time::timeout(Duration::from_secs(20), client.list_tools(None))
                .await
                .unwrap()
                .unwrap();
            assert!(listed.next_cursor.is_none());
            let names: BTreeSet<String> = listed
                .tools
                .iter()
                .map(|tool| tool.name.to_string())
                .collect();
            assert_eq!(names, expected);
            let mut called = BTreeSet::new();
            let index = call(
                &client,
                "index",
                json!({"path":root,"full":true}),
                &mut called,
            )
            .await;
            assert!(index["symbols_total"].as_u64().unwrap() >= 3);
            assert!(index["parse_errors"].as_array().unwrap().is_empty());
            let caps = call(
                &client,
                "status",
                json!({"aspect":"capabilities"}),
                &mut called,
            )
            .await;
            assert_eq!(caps["retrieval"]["semantic_state"], "not_configured");
            assert_eq!(caps["retrieval"]["dense_state"], "disabled");
            assert_eq!(caps["retrieval"]["local_state"], "available");
            let search = call(
                &client,
                "search",
                json!({"query":"renew_session","mode":"hybrid"}),
                &mut called,
            )
            .await;
            let (mut hits, _) = normalizer::mcp(&search).unwrap();
            assert!(hits.iter().any(|hit| hit.path == "src/lib.rs"));
            for hit in &mut hits {
                normalizer::verify_source(hit, root).unwrap();
                assert_eq!(hit.evidence_valid, Some(true));
            }
            let symbols = call(
                &client,
                "search",
                json!({"query":"renew_session","mode":"symbol"}),
                &mut called,
            )
            .await;
            assert!(symbols
                .as_array()
                .unwrap()
                .iter()
                .any(|symbol| symbol["name"] == "renew_session"));
            let context = call(
                &client,
                "context",
                json!({"task":"renew_session","retrieval_strategy":"auto"}),
                &mut called,
            )
            .await;
            assert!(!context["machine_pack"]["hits"]
                .as_array()
                .unwrap()
                .is_empty());
            assert_eq!(
                context["evidence_summary"]["retrieval"]["policy"]["effective"],
                "local"
            );
            let node = call(
                &client,
                "node",
                json!({"symbol":"renew_session","include":"source"}),
                &mut called,
            )
            .await;
            assert!(node["source"]
                .as_str()
                .unwrap()
                .contains("validate_payload"));
            let explore = call(
                &client,
                "explore",
                json!({"symbols":["renew_session","validate_payload"],"include_source":true}),
                &mut called,
            )
            .await;
            assert!(explore.to_string().contains("parse_field"));
            let trace = call(
                &client,
                "trace",
                json!({"from":"renew_session","to":"parse_field","source_mode":"body"}),
                &mut called,
            )
            .await;
            assert!(trace["path_count"].as_u64().unwrap() >= 1);
            let relations = call(
                &client,
                "relations",
                json!({"symbol":"validate_payload","kind":"callers"}),
                &mut called,
            )
            .await;
            assert!(relations.to_string().contains("renew_session"));
            let impact = call(&client, "impact", json!({"scope":"dead_code"}), &mut called).await;
            assert!(impact["dead_code"].is_array());
            let architecture = call(
                &client,
                "architecture",
                json!({"aspect":"overview"}),
                &mut called,
            )
            .await;
            assert!(architecture["languages"].is_array() || architecture["languages"].is_object());
            let region = call(
                &client,
                "files",
                json!({"action":"region","path":"src/lib.rs","start_line":1,"end_line":3}),
                &mut called,
            )
            .await;
            assert!(region["content"].as_str().unwrap().contains("parse_field"));
            let graph = call(
                &client,
                "graph_query",
                json!({"query":"MATCH (f:Function) RETURN f.name LIMIT 20"}),
                &mut called,
            )
            .await;
            assert!(graph["row_count"].as_u64().unwrap() >= 3);
            let ingest = call(&client, "ingest_traces", json!({"traces":[]}), &mut called).await;
            assert_eq!(ingest["accepted"], 0);
            let stored = call(&client, "adr", json!({"action":"store","adr_id":"P7-017-probe","title":"Local fixture only","status":"accepted","decision":"Remain offline"}), &mut called).await;
            assert_eq!(stored["stored"], "P7-017-probe");
            let adrs = call(&client, "adr", json!({"action":"list"}), &mut called).await;
            assert!(adrs["adrs"]
                .as_array()
                .unwrap()
                .iter()
                .any(|adr| adr["adr_id"] == "P7-017-probe"));
            assert_eq!(
                called, expected,
                "all 14 legacy tools must execute, not only list"
            );
            let typo = raw(&client, "search", json!({"qurey":"bad"}))
                .await
                .unwrap();
            assert_eq!(typo.is_error, Some(true));
            assert!(serde_json::to_string(&typo.content)
                .unwrap()
                .contains("unknown field"));
            for (tool, args, code) in [
                (
                    "search",
                    json!({"query":"renew_session","mode":"invalid"}),
                    ErrorCode::INVALID_PARAMS,
                ),
                (
                    "search",
                    json!({"query":"renew_session","retrieval_strategy":"semantic"}),
                    ErrorCode::INTERNAL_ERROR,
                ),
                (
                    "context",
                    json!({"task":"renew_session","retrieval_strategy":"semantic"}),
                    ErrorCode::INTERNAL_ERROR,
                ),
            ] {
                let error = raw(&client, tool, args).await.unwrap_err();
                assert!(
                    matches!(&error, ServiceError::McpError(data) if data.code == code),
                    "wrong {tool} error contract: {error}"
                );
            }
            let missing = raw(&client, "node", json!({"symbol":"no_such_fixture_symbol"}))
                .await
                .unwrap_err();
            assert!(matches!(&missing, ServiceError::McpError(data)
                if data.code == ErrorCode::INTERNAL_ERROR && data.message.contains("symbol not found")));
            let before_close = files(root);
            let child_exit = close(client, child).await;
            let (reopened, reopened_child) = spawn(&binary, root, &policy).await;
            let after = call(&reopened, "status", json!({"aspect":"index"}), &mut called).await;
            assert!(after["indexed_files"].as_u64().unwrap() >= 1);
            let reopened_search = call(
                &reopened,
                "search",
                json!({"query":"renew_session","mode":"hybrid"}),
                &mut called,
            )
            .await;
            assert!(normalizer::mcp(&reopened_search)
                .unwrap()
                .0
                .iter()
                .any(|hit| hit.path == "src/lib.rs"));
            let reopened_adrs = call(&reopened, "adr", json!({"action":"list"}), &mut called).await;
            assert!(reopened_adrs["adrs"]
                .as_array()
                .unwrap()
                .iter()
                .any(|adr| adr["adr_id"] == "P7-017-probe"));
            let removed = call(
                &reopened,
                "adr",
                json!({"action":"delete","adr_id":"P7-017-probe"}),
                &mut called,
            )
            .await;
            assert!(removed["deleted"].as_bool().unwrap());
            let reopened_network = network_status(&reopened_child);
            let reopened_guard = reopened_child.guard_receipt.clone();
            let reopened_exit = close(reopened, reopened_child).await;
            let final_files = files(root);
            // A main-process exit cannot account for a child whose SIGSYS is
            // swallowed. Only the separate full strace tree verifier may
            // publish zero attempts after every descendant/thread has exited.
            observations.push(json!({"package":package,"explicitly_disabled":explicitly_disabled,"child_environment":"env_clear; fixture HOME/XDG paths; PATH only; no key","network_scope":if restricted {"verified_process_seccomp_guard; full_descendant_trace_required"} else {"not_isolated_not_claimed"},"network_socket_attempts":Value::Null,"network_attempt_basis":Value::Null,"child_security":child_network,"reopened_child_security":reopened_network,"child_network_guard":child_guard,"reopened_network_guard":reopened_guard,"child_exit":child_exit,"reopened_exit":reopened_exit,"tools_listed":names,"tools_executed":called,"local_source_verified":true,"error_contracts_verified":true,"reopen_and_persisted_adr_verified":true,"files_before_close":before_close,"files_after_reopen_close":final_files,"semantic_cache_absent":true}));
        }
        verify_build_receipt(&binary, &package, &receipt)
            .expect("product identity unchanged after matrix");
        if let Ok(output) = std::env::var("CODECORTEX_BENCH_OBSERVATIONS") {
            std::fs::create_dir_all(&output).unwrap();
            std::fs::write(
                Path::new(&output).join(format!("p7-017-{package}.json")),
                serde_json::to_vec_pretty(
                    &json!({"binary":binary,"build_receipt":receipt,"network_positive_controls":policy.proof(),"cases":observations}),
                )
                .unwrap(),
            )
            .unwrap();
        }
    }
}

#[cfg(feature = "eval-http")]
mod http {
    use super::*;
    use cc_eval::benchmark::adapters::oce_http::OceHttp;
    use serde_json::Value;
    use sha2::{Digest, Sha256};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };
    async fn stub(
        responses: Vec<(&'static str, u16, Value)>,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let job = tokio::spawn(async move {
            for (path, status, value) in responses {
                let (mut s, _) = listener.accept().await.unwrap();
                let mut data = Vec::new();
                let mut buf = [0u8; 4096];
                loop {
                    let n = s.read(&mut buf).await.unwrap();
                    assert!(n > 0);
                    data.extend_from_slice(&buf[..n]);
                    assert!(data.len() < 2_000_000);
                    if let Some(pos) = data.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&data[..pos]);
                        assert!(head.starts_with(&format!("POST {path} ")));
                        let size = head
                            .lines()
                            .find_map(|l| {
                                l.to_lowercase()
                                    .strip_prefix("content-length:")
                                    .and_then(|v| v.trim().parse::<usize>().ok())
                            })
                            .unwrap_or(0);
                        if data.len() >= pos + 4 + size {
                            let request: Value =
                                serde_json::from_slice(&data[pos + 4..pos + 4 + size]).unwrap();
                            assert!(request.get("expected_files").is_none());
                            assert!(request.get("answers").is_none());
                            break;
                        }
                    }
                }
                let body = serde_json::to_vec(&value).unwrap();
                let header=format!("HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len());
                s.write_all(header.as_bytes()).await.unwrap();
                s.write_all(&body).await.unwrap();
                s.shutdown().await.unwrap();
            }
        });
        (format!("http://{address}"), job)
    }
    fn blob() -> String {
        let mut h = Sha256::new();
        h.update(b"a.py");
        h.update(b"def renew_session():\n    return 1\n");
        format!("{:x}", h.finalize())
    }
    #[tokio::test]
    async fn public_http_upload_ready_search_contract() {
        let d = project();
        let name = blob();
        let (endpoint, job) = stub(vec![
            ("/batch-upload", 200, json!({"blob_names":[name]})),
            (
                "/agents/blob-status",
                200,
                json!({"nonindexed_blob_names":[],"unknown_blob_names":[]}),
            ),
            (
                "/agents/codebase-retrieval",
                200,
                json!({"formatted_retrieval":"Path: a.py\nLine 1: def renew_session():"}),
            ),
        ])
        .await;
        let files = manifest::inventory(d.path(), &["a.py".into()]).unwrap();
        let mut client = OceHttp::new(
            &endpoint,
            "test-credential".into(),
            d.path().into(),
            Duration::from_secs(2),
            false,
        )
        .unwrap();
        client.prepare(&files).await.unwrap();
        let v = client
            .search(&SearchInput {
                query: "renew_session".into(),
                top_k: 10,
                path_prefix: None,
            })
            .await
            .unwrap();
        assert_eq!(normalizer::oce(&v).unwrap().0[0].path, "a.py");
        client.close().await.unwrap();
        job.await.unwrap();
    }
    #[tokio::test]
    async fn unknown_or_failed_inputs_do_not_become_ready() {
        for field in ["unknown_blob_names", "failed_blob_names"] {
            let d = project();
            let name = blob();
            let mut state = json!({"nonindexed_blob_names":[],"unknown_blob_names":[]});
            state[field] = json!([name]);
            let (endpoint, job) = stub(vec![
                ("/batch-upload", 200, json!({"blob_names":[name]})),
                ("/agents/blob-status", 200, state),
            ])
            .await;
            let files = manifest::inventory(d.path(), &["a.py".into()]).unwrap();
            let mut client = OceHttp::new(
                &endpoint,
                "test-credential".into(),
                d.path().into(),
                Duration::from_secs(2),
                false,
            )
            .unwrap();
            assert!(client.prepare(&files).await.is_err());
            job.await.unwrap();
        }
    }
    #[tokio::test]
    async fn errors_do_not_log_credentials_or_response_body() {
        let d = project();
        let (endpoint, job) = stub(vec![(
            "/agents/codebase-retrieval",
            500,
            json!({"secret":"do-not-log-this"}),
        )])
        .await;
        let mut client = OceHttp::new(
            &endpoint,
            "sensitive-token".into(),
            d.path().into(),
            Duration::from_secs(2),
            false,
        )
        .unwrap();
        let e = client
            .search(&SearchInput {
                query: "x".into(),
                top_k: 1,
                path_prefix: None,
            })
            .await
            .unwrap_err()
            .to_string();
        assert!(!e.contains("sensitive-token"));
        assert!(!e.contains("do-not-log-this"));
        job.await.unwrap();
    }
    #[test]
    fn external_transfer_and_endpoint_validation() {
        let d = project();
        assert!(OceHttp::new(
            "https://example.invalid",
            "x".into(),
            d.path().into(),
            Duration::from_secs(1),
            false
        )
        .is_err());
        assert!(OceHttp::new(
            "https://user:password@example.invalid",
            "x".into(),
            d.path().into(),
            Duration::from_secs(1),
            true
        )
        .is_err());
        assert!(OceHttp::new(
            "http://example.invalid",
            "x".into(),
            d.path().into(),
            Duration::from_secs(1),
            true
        )
        .is_err());
    }
    #[tokio::test]
    async fn timeout_is_distinct_from_empty_results() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let job = tokio::spawn(async move {
            let (_s, _) = listener.accept().await.unwrap();
            tokio::time::sleep(Duration::from_millis(200)).await;
        });
        let d = project();
        let mut client = OceHttp::new(
            &endpoint,
            "x".into(),
            d.path().into(),
            Duration::from_millis(30),
            false,
        )
        .unwrap();
        let e = client
            .search(&SearchInput {
                query: "x".into(),
                top_k: 1,
                path_prefix: None,
            })
            .await
            .unwrap_err();
        assert!(matches!(e, cc_eval::benchmark::BenchError::Timeout(_)));
        job.await.unwrap();
    }
}
