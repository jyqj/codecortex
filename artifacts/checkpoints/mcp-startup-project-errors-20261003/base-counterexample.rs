use super::*;
use cc_model::{config::ProjectConfig, CcError};
use std::time::Duration;
#[tokio::test]
async fn busy_initialization_publishes_empty_project_identity() {
    let dir = tempfile::tempdir_in("/workspace").unwrap();
    let mut config = ProjectConfig::default();
    config.semantic.enabled = true;
    config.semantic.model_id = "synthetic-startup-counterexample".into();
    config.semantic.dimensions = Some(2);
    config.semantic.max_input_tokens = Some(8192);
    config.semantic.max_batch_items = Some(16);
    config.semantic.endpoint = "https://semantic.invalid/v1".into();
    config.semantic.max_concurrent = 4;
    config.semantic.max_concurrent_per_project = 2;
    std::fs::write(dir.path().join(".codecortex.json"), serde_json::to_vec(&config).unwrap()).unwrap();
    let gate = crate::service_factory::semantic_provider_gate();
    let permit = gate.try_acquire_permit("old", Duration::ZERO).unwrap();
    assert!(matches!(CodeIndex::new(Some(dir.path())), Err(CcError::Config(_))));
    let session = ProjectSession::new(Some(dir.path()));
    let active = session.active_index().await;
    assert!(active.read().unwrap().project_path.is_none());
    assert!(active.read().unwrap().semantic_subsystem().is_none());
    let path = normalize_path(dir.path());
    assert!(session.project_cache.lock().await.contains(&path));
    assert!(session.live_projects.lock().await.contains_key(&path));
    let routed = session.index_for_project_path(dir.path().to_str()).await.unwrap();
    assert!(Arc::ptr_eq(&active, &routed));
    // Even the actual stdio startup boundary fails to return the Config error.
    let outcome = tokio::time::timeout(Duration::from_millis(300), crate::mcp::run_mcp_server(Some(path))).await;
    assert!(!matches!(outcome, Ok(Err(CcError::Config(_)))));
    println!("BASE_COUNTEREXAMPLE: CodeIndex=Config; session=Self(empty); requested cache/live identity published; route=Ok(same empty Arc); run_mcp_server did not propagate Config: {}", match outcome { Err(_) => "entered transport and timed out", Ok(Err(_)) => "returned transport error", Ok(Ok(())) => "returned Ok" });
    assert_eq!(gate.snapshot().in_flight, 1);
    drop(permit);
    assert_eq!(gate.snapshot().in_flight, 0);
    session.shutdown().await;
}
