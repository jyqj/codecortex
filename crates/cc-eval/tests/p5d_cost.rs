//! Release mechanism observations, not a throughput/tail certification.
use cc_server::{project_session::ProjectSession, query_handle::QueryHandle};
use serde_json::json;
use std::{sync::Arc, time::Instant};
#[tokio::test]
#[ignore = "explicit release P5-D idle/pin lifecycle cost"]
#[allow(clippy::assertions_on_constants)]
async fn release_idle_sweep_preserves_pins_and_recovers_after_release() {
    assert!(!cfg!(debug_assertions));
    let mut samples = Vec::new();
    for pin_count in [0, 4, 8, 16] {
        for repetition in 0..3 {
            let root = tempfile::tempdir().unwrap();
            let session = ProjectSession::new(None).unwrap();
            let mut runtimes = Vec::new();
            let mut weak_dbs = Vec::new();
            for n in 0..16 {
                let p = root.path().join(format!("p{n}"));
                std::fs::create_dir(&p).unwrap();
                std::fs::write(
                    p.join(".codecortex.json"),
                    r#"{"auto_index":{"enabled":false},"indexing":{"db_read_pool_size":1}}"#,
                )
                .unwrap();
                let rt = session
                    .index_for_project_path(Some(p.to_str().unwrap()))
                    .await
                    .unwrap();
                weak_dbs.push(Arc::downgrade(rt.read().unwrap().index_db().unwrap()));
                runtimes.push(rt);
            }
            let pins: Vec<_> = runtimes
                .iter()
                .take(pin_count)
                .map(|r| QueryHandle::capture(r).unwrap())
                .collect();
            let start = Instant::now();
            let first = session.evict_idle_now().await;
            let first_us = start.elapsed().as_micros();
            assert_eq!(first, 16 - pin_count);
            for rt in runtimes.iter().take(pin_count) {
                assert!(!rt.read().unwrap().is_closed());
            }
            drop(pins);
            let start = Instant::now();
            let second = session.evict_idle_now().await;
            let released_us = start.elapsed().as_micros();
            assert_eq!(second, pin_count);
            assert!(weak_dbs.iter().all(|w| w.upgrade().is_none()));
            session.shutdown().await;
            samples.push(json!({"repetition":repetition,"cached_projects":16,"pinned_views":pin_count,"first_closed":first,"first_sweep_us":first_us,"released_closed":second,"released_sweep_us":released_us,"all_db_resources_reclaimed":true}));
        }
    }
    if let Ok(out) = std::env::var("CODECORTEX_BENCH_OBSERVATIONS") {
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(std::path::Path::new(&out).join("p5d-idle-cost.json"),serde_json::to_vec_pretty(&json!({"status":"passed","samples":samples,"scope":"12 paired release sweeps of 16 cached projects with 0/4/8/16 pins; every sample retained; no network, parser throughput or latency-percentile certification"})).unwrap()).unwrap();
    }
}
