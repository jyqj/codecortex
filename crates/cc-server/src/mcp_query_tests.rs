//! Real MCP transport cancellation with an explicitly injected fake port.
use super::*;
use cc_model::{
    query::QueryControl,
    retrieval::LaneOutcome,
    semantic::{SemanticRecall, SemanticRequest},
    CcResult,
};
use rmcp::ServiceExt;
use std::{
    future::Future,
    pin::Pin,
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};
use tokio::sync::Notify;

struct DropProof(Arc<AtomicUsize>);
impl Drop for DropProof {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
struct PendingRecall {
    started: Notify,
    dropped: Arc<AtomicUsize>,
}
impl SemanticRecall for PendingRecall {
    fn recall(
        &self,
        _request: SemanticRequest,
        _control: QueryControl,
    ) -> Pin<Box<dyn Future<Output = CcResult<LaneOutcome>> + Send + '_>> {
        Box::pin(async move {
            let _drop = DropProof(self.dropped.clone());
            self.started.notify_one();
            std::future::pending().await
        })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mcp_cancel_notification_stops_search_and_context_optional_recall() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(".codecortex.json"),r#"{"auto_index":{"enabled":false},"indexing":{"db_read_pool_size":1},"query":{"strategy":"auto","deadline_ms":10000,"semantic_timeout_ms":8000}}"#).unwrap();
    std::fs::write(dir.path().join("a.py"), "def needle():\n    return 7\n").unwrap();
    let server = CodeCortexMcpServer::new(Some(dir.path())).unwrap();
    let runtime = server.project_session.active_index().await;
    runtime.write().unwrap().build_index(true).unwrap();
    let fake = Arc::new(PendingRecall {
        started: Notify::new(),
        dropped: Arc::new(AtomicUsize::new(0)),
    });
    runtime
        .read()
        .unwrap()
        .set_semantic_recall(Some(fake.clone()));
    let (client_io, server_io) = tokio::io::duplex(65536);
    let serving = tokio::spawn(async move {
        let running = server.serve(server_io).await.unwrap();
        running.waiting().await.unwrap();
    });
    let client = ().serve(client_io).await.unwrap();
    let tools = client.list_tools(None).await.unwrap();
    assert_eq!(tools.tools.len(), 14);
    for (n, tool) in ["search", "context"].iter().enumerate() {
        let arguments = if *tool == "search" {
            serde_json::json!({"query":"needle","top_k":1})
        } else {
            serde_json::json!({"task":"needle","include_source":false})
        };
        let request: ClientRequest = serde_json::from_value(
            serde_json::json!({"method":"tools/call","params":{"name":tool,"arguments":arguments}}),
        )
        .unwrap();
        let pending = client
            .send_request_with_option(request, Default::default())
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(5), fake.started.notified())
            .await
            .unwrap();
        let status = tokio::time::timeout(
            Duration::from_secs(3),
            client.call_tool(
                CallToolRequestParams::new("status").with_arguments(
                    serde_json::json!({"aspect":"index"})
                        .as_object()
                        .unwrap()
                        .clone(),
                ),
            ),
        )
        .await
        .unwrap()
        .unwrap();
        assert_ne!(status.is_error, Some(true));
        pending
            .cancel(Some("P5-B protocol cancellation regression".into()))
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            while fake.dropped.load(Ordering::SeqCst) < n + 1 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }
    runtime.read().unwrap().set_semantic_recall(None);
    let response = client
        .call_tool(
            CallToolRequestParams::new("search").with_arguments(
                serde_json::json!({"query":"needle","top_k":1})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .unwrap();
    assert_ne!(response.is_error, Some(true));
    client.cancel().await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), serving)
        .await
        .unwrap()
        .unwrap();
}
