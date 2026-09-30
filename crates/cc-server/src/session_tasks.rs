//! Session-owned recurring tasks. No strong self-reference from task to owner.
use std::{
    future::Future,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::task::JoinHandle;
#[derive(Default)]
struct Slots {
    stopped: bool,
    watcher: Option<(JoinHandle<()>, Arc<AtomicBool>)>,
    idle: Option<JoinHandle<()>>,
}
#[derive(Default)]
pub(crate) struct SessionTasks(Mutex<Slots>);
impl SessionTasks {
    pub(crate) fn replace_watcher<F>(&self, make: impl FnOnce(Arc<AtomicBool>) -> F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let mut slots = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if slots.stopped {
            return;
        }
        if let Some((old, _)) = slots.watcher.take() {
            old.abort();
        }
        let ready = Arc::new(AtomicBool::new(false));
        slots.watcher = Some((tokio::spawn(make(ready.clone())), ready));
    }
    pub(crate) fn start_idle(&self, work: impl Future<Output = ()> + Send + 'static) {
        let mut slots = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if slots.stopped || slots.idle.as_ref().is_some_and(|h| !h.is_finished()) {
            return;
        }
        slots.idle = Some(tokio::spawn(work));
    }
    pub(crate) fn watcher_ready(&self) -> bool {
        let slots = self.0.lock().unwrap_or_else(|e| e.into_inner());
        slots
            .watcher
            .as_ref()
            .is_some_and(|(h, r)| !h.is_finished() && r.load(Ordering::Acquire))
    }
    pub(crate) async fn shutdown(&self) {
        let handles = {
            let mut slots = self.0.lock().unwrap_or_else(|e| e.into_inner());
            slots.stopped = true;
            let mut handles = Vec::new();
            if let Some((h, _)) = slots.watcher.take() {
                handles.push(h);
            }
            if let Some(h) = slots.idle.take() {
                handles.push(h);
            }
            handles
        };
        for h in &handles {
            h.abort();
        }
        let _ = tokio::time::timeout(Duration::from_secs(2), async {
            for h in handles {
                let _ = h.await;
            }
        })
        .await;
    }
}
impl Drop for SessionTasks {
    fn drop(&mut self) {
        let slots = self.0.get_mut().unwrap_or_else(|e| e.into_inner());
        if let Some((h, _)) = slots.watcher.take() {
            h.abort();
        }
        if let Some(h) = slots.idle.take() {
            h.abort();
        }
    }
}

/// Move into a blocking build: cancellation of its awaiting task must neither
/// leak the flag nor release it before the real worker returns.
pub(crate) struct AutoIndexPermit(pub Arc<AtomicBool>);
impl Drop for AutoIndexPermit {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}
