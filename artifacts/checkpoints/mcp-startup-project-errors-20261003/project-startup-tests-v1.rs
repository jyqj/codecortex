//! Each gate-policy fixture gets a fresh process singleton and owned writable paths.
use super::*;
use crate::mcp::{run_mcp_server, CodeCortexMcpServer};
use cc_model::{config::ProjectConfig, CcError};
use cc_semantic::admission::GateLimits;
use std::time::Duration;

#[test]
fn isolated_startup_regressions() {
    let root = tempfile::tempdir_in("/workspace").unwrap();
    for case in [
        "busy_startup_and_retry",
        "conflicting_startup_and_path_switch",
    ] {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                &format!("project_session::startup_tests::{case}"),
                "--ignored",
                "--nocapture",
            ])
            .env(cc_semantic::cache::CACHE_ROOT_ENV, root.path().join(case))
            .output()
            .unwrap();
        println!("{}", String::from_utf8_lossy(&output.stdout));
        assert!(
            output.status.success(),
            "{case}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

fn project(root: &Path, name: &str, caps: Option<(u32, u32)>) -> PathBuf {
    let path = root.join(name);
    std::fs::create_dir(&path).unwrap();
    write_config(&path, caps);
    std::fs::write(path.join("lib.rs"), "pub fn answer() -> u32 { 42 }\n").unwrap();
    path
}
fn write_config(path: &Path, caps: Option<(u32, u32)>) {
    let mut config = ProjectConfig::default();
    // Enabled background features make rejection ordering consequential.
    config.auto_index.enabled = true;
    if let Some((global, per_project)) = caps {
        config.semantic.enabled = true;
        config.semantic.model_id = "synthetic-startup".into();
        config.semantic.dimensions = Some(2);
        config.semantic.max_input_tokens = Some(8192);
        config.semantic.max_batch_items = Some(16);
        config.semantic.endpoint = "https://semantic.invalid/v1".into();
        config.semantic.max_concurrent = global;
        config.semantic.max_concurrent_per_project = per_project;
    }
    std::fs::write(
        path.join(".codecortex.json"),
        serde_json::to_vec(&config).unwrap(),
    )
    .unwrap();
}
async fn assert_unpublished(session: &ProjectSession, path: &Path, original: &SharedCodeIndex) {
    let path = normalize_path(path);
    assert!(!session.project_cache.lock().await.contains(&path));
    assert!(!session.live_projects.lock().await.contains_key(&path));
    assert!(Arc::ptr_eq(original, &session.active_index().await));
    assert!(!session.watcher_ready());
    assert!(!session.auto_indexing.load(Ordering::SeqCst));
}
async fn reject_at_every_boundary(path: &Path) {
    let runtime = tokio::runtime::Handle::current();
    let tasks_before = runtime.metrics().num_alive_tasks();
    assert!(matches!(
        CodeIndex::new(Some(path)),
        Err(CcError::Config(_))
    ));
    assert!(matches!(
        ProjectSession::new(Some(path)),
        Err(CcError::Config(_))
    ));
    assert!(matches!(
        CodeCortexMcpServer::new(Some(path)),
        Err(CcError::Config(_))
    ));
    // Actual stdio entry point must return before transport/initial tasks/idle/watchdog.
    // On the old path it falls through to stdio (or hangs); neither is Config.
    let result = tokio::time::timeout(
        Duration::from_secs(2),
        run_mcp_server(Some(path.to_path_buf())),
    )
    .await
    .expect("rejected project must not enter MCP transport");
    assert!(matches!(result, Err(CcError::Config(_))));
    tokio::task::yield_now().await;
    assert_eq!(
        runtime.metrics().num_alive_tasks(),
        tasks_before,
        "startup refusal must publish no background Tokio task"
    );
}

#[tokio::test]
#[ignore = "isolated_startup_regressions starts a fresh process"]
async fn busy_startup_and_retry() {
    let root = tempfile::tempdir_in("/workspace").unwrap();
    let rejected = project(root.path(), "busy", Some((4, 2)));
    let gate = crate::service_factory::semantic_provider_gate();
    let permit = gate
        .try_acquire_permit("old-default", Duration::ZERO)
        .unwrap();
    reject_at_every_boundary(&rejected).await;
    let session = ProjectSession::new(None).unwrap();
    let original = session.active_index().await;
    assert!(original.read().unwrap().project_path.is_none());
    assert!(CodeCortexMcpServer::new(None).is_ok());
    for _ in 0..2 {
        assert!(matches!(
            session.set_active_project(rejected.clone()).await,
            Err(CcError::Config(_))
        ));
        assert!(matches!(
            session.index_for_project_path(rejected.to_str()).await,
            Err(CcError::Config(_))
        ));
        assert_unpublished(&session, &rejected, &original).await;
    }
    assert_eq!(gate.snapshot().in_flight, 1);
    assert_eq!(gate.snapshot().max_concurrent, usize::MAX);
    drop(permit);
    let retry = session
        .index_for_project_path(rejected.to_str())
        .await
        .unwrap();
    assert_eq!(
        retry.read().unwrap().project_path.as_deref(),
        Some(rejected.as_path())
    );
    assert!(retry.read().unwrap().semantic_subsystem().is_some());
    assert!(!Arc::ptr_eq(&retry, &original));
    let active = session.set_active_project(rejected.clone()).await.unwrap();
    assert!(Arc::ptr_eq(&retry, &active));
    assert_eq!(gate.snapshot().max_concurrent, 4);
    assert_eq!(gate.snapshot().in_flight, 0);
    session.shutdown().await;
    println!("BUSY: CodeIndex/session/server/run_mcp_server=Config; None valid; cache/live absent; active unchanged; old permit preserved/released; retry initializes real project");
}

#[tokio::test]
#[ignore = "isolated_startup_regressions starts a fresh process"]
async fn conflicting_startup_and_path_switch() {
    let root = tempfile::tempdir_in("/workspace").unwrap();
    let a = project(root.path(), "a", Some((4, 2)));
    let b = project(root.path(), "b", Some((5, 2)));
    crate::service_factory::init_semantic_provider_gate(GateLimits::validated(4, Some(2)).unwrap())
        .unwrap();
    let session = ProjectSession::new(Some(&a)).unwrap();
    let original = session.active_index().await;
    assert!(original.read().unwrap().semantic_subsystem().is_some());
    reject_at_every_boundary(&b).await;
    assert!(matches!(
        session.set_active_project(b.clone()).await,
        Err(CcError::Config(_))
    ));
    assert!(matches!(
        session.index_for_project_path(b.to_str()).await,
        Err(CcError::Config(_))
    ));
    assert_unpublished(&session, &b, &original).await;
    write_config(&b, Some((4, 2)));
    let switched = session.set_active_project(b.clone()).await.unwrap();
    assert!(switched.read().unwrap().semantic_subsystem().is_some());
    assert_eq!(
        switched.read().unwrap().project_path.as_deref(),
        Some(b.as_path())
    );
    assert!(!Arc::ptr_eq(&original, &switched));
    assert!(Arc::ptr_eq(
        &original,
        &session.set_active_project(a.clone()).await.unwrap()
    ));
    session.shutdown().await;
    assert_eq!(session.evict_idle_now().await, 2);
    assert!(original.read().unwrap().is_closed());
    assert!(switched.read().unwrap().is_closed());
    let reopened = session.index_for_project_path(b.to_str()).await.unwrap();
    assert!(Arc::ptr_eq(&reopened, &switched));
    assert!(!reopened.read().unwrap().is_closed());
    assert!(reopened.read().unwrap().semantic_subsystem().is_some());
    session.reopen_active_index_if_closed().await.unwrap();
    assert!(!original.read().unwrap().is_closed());
    let plain = project(root.path(), "plain", None);
    assert!(ProjectSession::new(Some(&plain)).is_ok());
    println!("CONFLICT: all startup boundaries=Config; failed switch preserves active/cache/live; corrected config retries; A/B identity and idle reopen work; plain project valid");
}
