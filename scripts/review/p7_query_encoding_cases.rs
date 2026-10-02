//! Independently authored assertions. Only the candidate's explicit publication
//! hook is reused; factory, barriers, expected state and transports are ours.
use super::*;
use cc_semantic::providers::fake::{FakeProvider, FakeProviderConfig};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Mutex,
};
use std::time::Duration;

fn control() -> QueryControl {
    QueryControl::new(Duration::from_secs(5)).unwrap()
}
fn fixture() -> (Arc<FakeProvider>, QueryEncodingInputs) {
    let provider = Arc::new(FakeProvider::new(FakeProviderConfig::new(
        VectorSpace::new("independent/synthetic", 3).unwrap(),
    )));
    let inputs = QueryEncodingInputs {
        cache: Arc::new(QueryVectorCache::new(4, 4096)),
        namespace: cc_semantic::cache::namespace_key("independent/synthetic").unwrap(),
        spec: QueryEncodingSpec::new(
            provider.space().clone(),
            None,
            1024,
            cc_model::chunk_policy::TOKEN_ESTIMATOR,
        )
        .unwrap(),
        budget: InputBudget::validated(1, 1024, 1024).unwrap(),
        lifecycle: Arc::default(),
    };
    (provider, inputs)
}
struct Release(Option<std::sync::mpsc::Sender<()>>);
impl Release {
    fn now(&mut self) {
        if let Some(s) = self.0.take() {
            let _ = s.send(());
        }
    }
}
impl Drop for Release {
    fn drop(&mut self) {
        self.now();
    }
}
async fn free(capacity: &Semaphore) {
    tokio::time::timeout(Duration::from_secs(3), async {
        while capacity.available_permits() == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn publication_boundaries_independently_preserve_cache_and_physical_capacity() {
    for fault in 0..4 {
        let (p, inputs) = fixture();
        let cache = inputs.cache.clone();
        let lifecycle = inputs.lifecycle.clone();
        let capacity = Arc::new(Semaphore::new(1));
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let mut service = QueryEncodingService::build(
            inputs,
            CapacitySource::Direct(capacity.clone()),
            Arc::new(move |_| {
                count.fetch_add(1, Ordering::SeqCst);
                Ok(p.clone())
            }),
            None,
        )
        .unwrap();
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let entered = Mutex::new(Some(entered_tx));
        let (tx, rx) = std::sync::mpsc::channel();
        let rx = Mutex::new(rx);
        let mut release = Release(Some(tx));
        Arc::get_mut(&mut service).unwrap().before_publish = Some(Arc::new(move || {
            entered.lock().unwrap().take().unwrap().send(()).unwrap();
            rx.lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(3))
                .unwrap();
        }));
        let shared = control();
        let request_control = if fault == 2 {
            shared.child(Duration::from_millis(150))
        } else {
            shared.clone()
        };
        let request = tokio::spawn({
            let service = service.clone();
            async move {
                service
                    .encode(b"independent publication".to_vec(), request_control)
                    .await
            }
        });
        tokio::time::timeout(Duration::from_secs(2), entered_rx)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(cache.is_empty());
        assert_eq!(capacity.available_permits(), 0);
        match fault {
            0 => {
                request.abort();
                assert!(request.await.unwrap_err().is_cancelled());
                shared.check().unwrap();
            }
            1 => {
                shared.cancel();
                assert!(matches!(
                    request.await.unwrap(),
                    Err(CcError::QueryCancelled)
                ));
            }
            2 => {
                assert!(matches!(
                    request.await.unwrap(),
                    Err(CcError::QueryTimedOut)
                ));
                shared.check().unwrap();
            }
            3 => {
                lifecycle.close();
                release.now();
                assert!(matches!(
                    request.await.unwrap(),
                    Err(CcError::QueryCancelled)
                ));
            }
            _ => unreachable!(),
        }
        if fault != 3 {
            assert_eq!(
                capacity.available_permits(),
                0,
                "waiting future exit must not release physical slot"
            );
        }
        release.now();
        free(&capacity).await;
        assert!(cache.is_empty(), "late publication after fault {fault}");
    }
}

struct HeldProvider {
    space: VectorSpace,
    entered: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    release: Mutex<std::sync::mpsc::Receiver<()>>,
}
impl EmbeddingProvider for HeldProvider {
    fn space(&self) -> &VectorSpace {
        &self.space
    }
    fn embed_documents(&self, _: &[DocumentInput]) -> Result<Vec<Vec<f32>>, ProviderError> {
        panic!("query-only fixture")
    }
    fn embed_queries(&self, batch: &[QueryInput]) -> Result<Vec<Vec<f32>>, ProviderError> {
        assert_eq!(batch.len(), 1);
        self.entered
            .lock()
            .unwrap()
            .take()
            .unwrap()
            .send(())
            .unwrap();
        self.release
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(3))
            .unwrap();
        Ok(vec![vec![1.0, 0.0, 0.0]])
    }
}

#[tokio::test]
async fn root_drop_holds_real_query_pin_and_child_token_until_blocking_exit() {
    let (p, inputs) = fixture();
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let (tx, rx) = std::sync::mpsc::channel();
    let held = Arc::new(HeldProvider {
        space: p.space().clone(),
        entered: Mutex::new(Some(entered_tx)),
        release: Mutex::new(rx),
    });
    let mut release = Release(Some(tx));
    let captured = Arc::new(Mutex::new(None));
    let save = captured.clone();
    let context = Arc::new(QueryEncodingContext {
        factory: Arc::new(move |_, token| {
            *save.lock().unwrap() = Some(token);
            Ok(held.clone())
        }),
        cache: inputs.cache.clone(),
        namespace: inputs.namespace,
        spec: inputs.spec,
        input_budget: inputs.budget,
        lifecycle: inputs.lifecycle,
        cancellation: tokio_util::sync::CancellationToken::new(),
        capacity: Arc::new(Semaphore::new(1)),
    });
    let services = crate::service_factory::QueryServices::default();
    let shared = control();
    let request = tokio::spawn({
        let context = context.clone();
        let pin = services.pin();
        let control = shared.clone();
        async move {
            ensure_query_vector(context, "independent held factory".into(), control, pin).await
        }
    });
    tokio::time::timeout(Duration::from_secs(2), entered_rx)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(services.query_pins(), 1);
    request.abort();
    request.await.unwrap_err();
    assert_eq!(services.query_pins(), 1);
    assert_eq!(context.capacity.available_permits(), 0);
    assert!(captured.lock().unwrap().as_ref().unwrap().is_cancelled());
    assert!(!context.cancellation.is_cancelled());
    shared.check().unwrap();
    release.now();
    free(&context.capacity).await;
    tokio::time::timeout(Duration::from_secs(2), async {
        while services.query_pins() != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(context.cache.is_empty());
}

#[tokio::test]
async fn cache_hit_avoids_factory_and_provider_even_when_capacity_full() {
    let (p, inputs) = fixture();
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let provider = p.clone();
    let capacity = Arc::new(Semaphore::new(1));
    let service = QueryEncodingService::build(
        inputs,
        CapacitySource::Direct(capacity.clone()),
        Arc::new(move |_| {
            count.fetch_add(1, Ordering::SeqCst);
            Ok(provider.clone())
        }),
        None,
    )
    .unwrap();
    assert_eq!(
        service
            .encode(b"independent cache".to_vec(), control())
            .await
            .unwrap(),
        QueryEncodingOutcome::Ready { cache_hit: false }
    );
    let _occupied = capacity.acquire().await.unwrap();
    assert_eq!(
        service
            .encode(b"independent cache".to_vec(), control())
            .await
            .unwrap(),
        QueryEncodingOutcome::Ready { cache_hit: true }
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(p.call_count(), 1);
}

struct CaptureTransport {
    timeouts: Mutex<Vec<Duration>>,
    token: tokio_util::sync::CancellationToken,
    cancel_during: bool,
}
impl EmbeddingHttpTransport for CaptureTransport {
    fn post_json(&self, request: HttpRequest) -> Result<HttpResponse, TransportError> {
        self.timeouts.lock().unwrap().push(request.timeout);
        if self.cancel_during {
            self.token.cancel();
        }
        Ok(HttpResponse {
            status: 200,
            headers: vec![],
            body: b"{}".to_vec(),
        })
    }
}
fn http(timeout: Duration) -> HttpRequest {
    HttpRequest {
        url: "http://127.0.0.1:9/unused".into(),
        headers: vec![],
        body: vec![],
        timeout,
    }
}
#[test]
fn transport_clamps_deadline_and_checks_child_before_and_after_io() {
    let parent = tokio_util::sync::CancellationToken::new();
    let child = parent.child_token();
    let raw = Arc::new(CaptureTransport {
        timeouts: Mutex::default(),
        token: child.clone(),
        cancel_during: false,
    });
    let c = QueryControl::new(Duration::from_millis(200)).unwrap();
    let wrapped = query_deadline_transport(raw.clone(), c, child.clone());
    wrapped.post_json(http(Duration::from_secs(30))).unwrap();
    wrapped.post_json(http(Duration::from_millis(10))).unwrap();
    let times = raw.timeouts.lock().unwrap();
    assert!(times[0] <= Duration::from_millis(200));
    assert!(!times[0].is_zero());
    assert_eq!(times[1], Duration::from_millis(10));
    drop(times);
    child.cancel();
    assert!(matches!(
        wrapped.post_json(http(Duration::from_secs(1))),
        Err(TransportError::Cancelled)
    ));
    assert_eq!(raw.timeouts.lock().unwrap().len(), 2);
    assert!(!parent.is_cancelled());
    let child2 = parent.child_token();
    let raw2 = Arc::new(CaptureTransport {
        timeouts: Mutex::default(),
        token: child2.clone(),
        cancel_during: true,
    });
    assert!(matches!(
        query_deadline_transport(raw2.clone(), control(), child2)
            .post_json(http(Duration::from_secs(1))),
        Err(TransportError::Cancelled)
    ));
    assert_eq!(raw2.timeouts.lock().unwrap().len(), 1);
    assert!(!parent.is_cancelled());
}

#[test]
fn expired_deadline_and_parent_retirement_skip_raw_transport() {
    let parent = tokio_util::sync::CancellationToken::new();
    let raw = Arc::new(CaptureTransport {
        timeouts: Mutex::default(),
        token: parent.child_token(),
        cancel_during: false,
    });
    let expired = QueryControl::new(Duration::ZERO).unwrap();
    assert!(matches!(
        query_deadline_transport(raw.clone(), expired, parent.child_token())
            .post_json(http(Duration::from_secs(30))),
        Err(TransportError::Timeout)
    ));
    let wrapped = query_deadline_transport(raw.clone(), control(), parent.child_token());
    parent.cancel();
    assert!(matches!(
        wrapped.post_json(http(Duration::from_secs(30))),
        Err(TransportError::Cancelled)
    ));
    assert!(raw.timeouts.lock().unwrap().is_empty());
}
