//! 真实 MCP dispatch seam 的行为回归测试。
//!
//! eval 的 `CodeIndexBackend` 通过 in-process duplex 上的 rmcp JSON-RPC client
//! 调用与 stdio wire path 同源的 `CodeCortexMcpServer`（见 `cc_eval::mcp_wire`）。
//! 这里固化 seam 的错误语义：schema 非法参数 / 枚举校验失败 / 未知工具都必须
//! 以错误浮出，而不是被手写 dispatch 的 `unwrap_or(default)` 静默吞掉。

use cc_eval::runner::CodeIndexBackend;
use serde_json::json;

fn fixture_backend() -> (tempfile::TempDir, CodeIndexBackend) {
    let crate_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let fixtures_dir = crate_dir.join("fixtures").join("sample-project");
    let tmp = tempfile::tempdir().expect("create tempdir");
    for entry in std::fs::read_dir(&fixtures_dir).expect("read fixtures") {
        let entry = entry.expect("read fixture entry");
        if entry.path().is_file() {
            std::fs::copy(entry.path(), tmp.path().join(entry.file_name()))
                .expect("copy fixture file");
        }
    }
    let backend = CodeIndexBackend::new(tmp.path()).expect("backend should build fixture index");
    (tmp, backend)
}

#[test]
fn seam_rejects_schema_invalid_params() {
    let (_tmp, backend) = fixture_backend();

    // 枚举校验（sanitize）：非法 mode 必须报 invalid params，而不是回退 hybrid。
    let err = backend
        .call_tool("search", &json!({"query": "x", "mode": "bogus_mode"}))
        .expect_err("invalid enum value must be rejected by sanitize()");
    assert!(
        err.contains("invalid mode"),
        "expected sanitize() enum error, got: {}",
        err
    );

    // 类型错误：top_k 传字符串必须在 schema 反序列化阶段报错。
    let err = backend
        .call_tool("search", &json!({"query": "x", "top_k": "five"}))
        .expect_err("type-mismatched param must fail schema deserialization");
    assert!(
        err.contains("-32602") || err.to_lowercase().contains("invalid"),
        "expected schema deserialization error, got: {}",
        err
    );

    // 缺失必填参数：search 没有 query 必须报错，而不是用空串兜底。
    let err = backend
        .call_tool("search", &json!({"mode": "symbol"}))
        .expect_err("missing required param must fail schema deserialization");
    assert!(
        err.contains("query"),
        "expected missing-field error mentioning `query`, got: {}",
        err
    );
}

#[test]
fn seam_rejects_unknown_tool() {
    let (_tmp, backend) = fixture_backend();
    let err = backend
        .call_tool("definitely_not_a_tool", &json!({}))
        .expect_err("unknown tool must surface a router error");
    assert!(
        err.contains("tool not found") || err.contains("-32602"),
        "expected rmcp router 'tool not found' error, got: {}",
        err
    );
}

#[test]
fn seam_result_is_unwrapped_handler_json() {
    let (_tmp, backend) = fixture_backend();
    // search(mode=symbol) 的 handler 返回根数组；经过 envelope 解包后 corpus
    // 断言看到的应当就是这个数组（与真实 MCP client 看到的 result 一致）。
    let out = backend
        .call_tool(
            "search",
            &json!({"query": "formatName", "mode": "symbol", "top_k": 5}),
        )
        .expect("symbol search should succeed");
    let arr = out.as_array().expect("symbol search returns a root array");
    assert!(
        arr.iter().any(|item| item
            .get("name")
            .and_then(|v| v.as_str())
            .is_some_and(|n| n == "formatName")),
        "expected formatName hit in: {}",
        out
    );
}
