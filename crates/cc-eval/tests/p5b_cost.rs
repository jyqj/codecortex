//! Local release execution/admission observations. Not throughput/tail certification.
use cc_model::{query::QueryControl, CcError};
use cc_search::execution::ExecutionPool;
use serde_json::json;
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "explicit release P5-B bounded-execution cost observations"]
#[allow(clippy::assertions_on_constants)]
async fn release_bounded_admission_and_capacity_recovery() {
    assert!(!cfg!(debug_assertions), "release required");
    let mut rounds = Vec::new();
    for round in 0..3 {
        let pool = ExecutionPool::new(4, 32, 8).unwrap();
        let running = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let barrier = Arc::new(tokio::sync::Barrier::new(65));
        let gate = Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
        let mut jobs = Vec::new();
        let start = Instant::now();
        for _ in 0..64 {
            let pool = pool.clone();
            let running = running.clone();
            let peak = peak.clone();
            let barrier = barrier.clone();
            let gate = gate.clone();
            jobs.push(tokio::spawn(async move {
                barrier.wait().await;
                pool.run_cpu(
                    QueryControl::new(Duration::from_secs(5)).unwrap(),
                    move || {
                        let now = running.fetch_add(1, Ordering::SeqCst) + 1;
                        peak.fetch_max(now, Ordering::SeqCst);
                        let (lock, cv) = &*gate;
                        let released = lock.lock().unwrap();
                        let _released = cv
                            .wait_timeout_while(released, Duration::from_secs(2), |v| !*v)
                            .unwrap();
                        running.fetch_sub(1, Ordering::SeqCst);
                        Ok(())
                    },
                )
                .await
            }));
        }
        barrier.wait().await;
        tokio::time::timeout(Duration::from_secs(2), async {
            while pool.stats().cpu_admitted != 36 || pool.stats().rejected != 28 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let full = pool.stats();
        assert!(full.cpu_in_flight <= 4);
        assert_eq!(full.cpu_admitted, 36);
        {
            let (lock, cv) = &*gate;
            *lock.lock().unwrap() = true;
            cv.notify_all();
        }
        let mut accepted = 0;
        let mut rejected = 0;
        for job in jobs {
            match job.await.unwrap() {
                Ok(()) => accepted += 1,
                Err(CcError::QueryBusy) => rejected += 1,
                other => panic!("unexpected result {other:?}"),
            }
        }
        assert_eq!(accepted, 36);
        assert_eq!(rejected, 28);
        assert!(peak.load(Ordering::SeqCst) <= 4);
        assert_eq!(pool.stats().cpu_admitted, 0);
        pool.run_cpu(
            QueryControl::new(Duration::from_secs(1)).unwrap(),
            || Ok(()),
        )
        .await
        .unwrap();
        rounds.push(json!({"round":round,"requests":64,"accepted":accepted,"rejected":rejected,"peak_workers":peak.load(Ordering::SeqCst),"observed_saturation":full,"settled":pool.stats(),"elapsed_us":start.elapsed().as_micros(),"native_rss_bytes":cc_index::process_rss_bytes_opt()}));
    }
    let result = json!({"status":"passed","scope":"three local release barrier-synchronized bounded-admission rounds; not open-loop throughput, latency percentile, instant peak RSS or 100k certification","rounds":rounds});
    if let Ok(out) = std::env::var("CODECORTEX_BENCH_OBSERVATIONS") {
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(
            std::path::Path::new(&out).join("p5b-execution-cost.json"),
            serde_json::to_vec_pretty(&result).unwrap(),
        )
        .unwrap();
    }
}
