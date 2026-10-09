//! Bounded JSON stderr for the opt-in runtime diagnostic target only.
//! Ordinary application logs retain their existing text formatter.
use std::collections::BTreeMap;
use std::fmt::{self, Write as _};
use std::io::Write as _;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::{Map, Value};
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

const SCHEMA: &str = "p8_runtime_diagnostics_v1";
const MAX_LINE_BYTES: usize = 4096;
const MAX_REQUEST_EVENTS: u64 = 256;
// Transport bound, not a query deadline, admission limit or performance SLA.
// An overflow invalidates diagnostics explicitly; it never changes RPC results.
const MAX_STREAM_BYTES: u64 = 256 * 1024 * 1024;
// Reserve a full bounded line each for stream-invalid and session-end.
const STREAM_TERMINAL_RESERVE: u64 = 2 * MAX_LINE_BYTES as u64;
const TARGET: &str = cc_db::runtime_diagnostics::TARGET;

#[derive(Default)]
struct Fields {
    values: BTreeMap<String, Value>,
    invalid: bool,
}
struct BoundedString(String);
impl fmt::Write for BoundedString {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        if self.0.len() + value.len() > MAX_LINE_BYTES {
            return Err(fmt::Error);
        }
        self.0.push_str(value);
        Ok(())
    }
}
impl Visit for Fields {
    fn record_i64(&mut self, field: &Field, value: i64) {
        self.values.insert(field.name().into(), value.into());
    }
    fn record_u64(&mut self, field: &Field, value: u64) {
        self.values.insert(field.name().into(), value.into());
    }
    fn record_bool(&mut self, field: &Field, value: bool) {
        self.values.insert(field.name().into(), value.into());
    }
    fn record_str(&mut self, field: &Field, value: &str) {
        if value.len() > MAX_LINE_BYTES {
            self.invalid = true;
        } else {
            self.values.insert(field.name().into(), value.into());
        }
    }
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        let mut text = BoundedString(String::new());
        if write!(&mut text, "{value:?}").is_err() {
            self.invalid = true;
        } else {
            self.record_str(field, &text.0);
        }
    }
}
struct Correlation {
    value: Value,
    events: AtomicU64,
}
#[derive(Default)]
struct Output {
    events: u64,
    written_bytes: u64,
    suppressed_events: u64,
    invalid: bool,
    overflowed: bool,
}
type DiagnosticWriter = dyn Fn(&[u8]) -> std::io::Result<()> + Send + Sync;
struct DiagnosticLayer {
    max_stream_bytes: u64,
    output: Arc<Mutex<Output>>,
    writer: Arc<DiagnosticWriter>,
}
impl DiagnosticLayer {
    fn stderr() -> Self {
        Self::with_writer(|bytes| std::io::stderr().lock().write_all(bytes))
    }
    fn with_writer(writer: impl Fn(&[u8]) -> std::io::Result<()> + Send + Sync + 'static) -> Self {
        Self {
            max_stream_bytes: MAX_STREAM_BYTES,
            output: Arc::new(Mutex::new(Output::default())),
            writer: Arc::new(writer),
        }
    }
    fn emit(&self, fields: Fields, correlation: Option<Value>) {
        let mut output = self
            .output
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        output.events = output.events.saturating_add(1);
        let sequence = output.events;
        let terminal = fields.values.get("event").and_then(Value::as_str) == Some("session_end");
        if output.overflowed && !terminal {
            output.suppressed_events = output.suppressed_events.saturating_add(1);
            return;
        }
        let mut invalid_encoding = fields.invalid;
        let mut values: Map<String, Value> = fields.values.into_iter().collect();
        if let Some(value) = values.remove("waits_json") {
            let parsed = value
                .as_str()
                .and_then(|raw| serde_json::from_str::<Value>(raw).ok());
            match parsed {
                Some(value) => {
                    values.insert("waits".into(), value);
                }
                None => invalid_encoding = true,
            }
        }
        output.invalid |= invalid_encoding
            || values.get("event").and_then(Value::as_str) == Some("wait_incomplete")
            || (values.get("event").and_then(Value::as_str) == Some("worker_waits")
                && values.get("complete") == Some(&Value::Bool(false)));
        values.insert("schema".into(), SCHEMA.into());
        values.insert("process_id".into(), std::process::id().into());
        values.insert("event_seq".into(), sequence.into());
        values.insert("correlation".into(), correlation.unwrap_or(Value::Null));
        if matches!(
            values.get("event").and_then(Value::as_str),
            Some("worker_waits" | "unscoped_wait" | "wait_incomplete")
        ) {
            values.insert("sqlite_busy_ns".into(), Value::Null);
        }
        if values.get("event").and_then(Value::as_str) == Some("wait_incomplete") {
            values.insert("result_ok".into(), Value::Null);
        }
        if terminal {
            values.insert(
                "complete".into(),
                (!output.invalid && !output.overflowed).into(),
            );
            values.insert("suppressed_events".into(), output.suppressed_events.into());
        }
        let mut bytes = serde_json::to_vec(&values).expect("fixed diagnostic values serialize");
        let too_large = bytes.len() + 1 > MAX_LINE_BYTES;
        let over_budget = !terminal
            && output.written_bytes.saturating_add(bytes.len() as u64 + 1)
                > self
                    .max_stream_bytes
                    .saturating_sub(STREAM_TERMINAL_RESERVE);
        if invalid_encoding || too_large || over_budget {
            output.invalid = true;
            output.overflowed |= over_budget;
            bytes = serde_json::to_vec(&serde_json::json!({
                "schema": SCHEMA, "process_id": std::process::id(), "event_seq": sequence,
                "event": "diagnostic_invalid",
                "reason": if over_budget { "stream_budget_exceeded" } else { "event_encoding_or_size" },
                "complete": false,
            })).expect("fixed diagnostic error serializes");
        }
        bytes.push(b'\n');
        // This guard also covers malformed terminal input and protects the
        // declared total even if a future change accidentally spends reserve.
        if output.written_bytes.saturating_add(bytes.len() as u64) > self.max_stream_bytes {
            output.invalid = true;
            output.overflowed = true;
            output.suppressed_events = output.suppressed_events.saturating_add(1);
            return;
        }
        output.written_bytes = output.written_bytes.saturating_add(bytes.len() as u64);
        if (self.writer)(&bytes).is_err() {
            output.invalid = true;
        }
    }
}
impl<S> Layer<S> for DiagnosticLayer
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    fn on_new_span(
        &self,
        attributes: &tracing::span::Attributes<'_>,
        id: &tracing::Id,
        context: Context<'_, S>,
    ) {
        if attributes.metadata().target() != TARGET
            || !matches!(attributes.metadata().name(), "mcp_call" | "local_operation")
        {
            return;
        }
        let mut fields = Fields::default();
        attributes.record(&mut fields);
        let correlation = fields
            .values
            .get("correlation_json")
            .and_then(Value::as_str)
            .and_then(|raw| serde_json::from_str::<Value>(raw).ok());
        if fields.invalid {
            self.emit(
                Fields {
                    invalid: true,
                    ..Fields::default()
                },
                None,
            );
            return;
        }
        if let (Some(span), Some(value)) = (context.span(id), correlation) {
            span.extensions_mut().insert(Correlation {
                value,
                events: AtomicU64::new(0),
            });
        } else {
            self.emit(
                Fields {
                    invalid: true,
                    ..Fields::default()
                },
                None,
            );
        }
    }
    fn on_event(&self, event: &Event<'_>, context: Context<'_, S>) {
        if event.metadata().target() != TARGET {
            return;
        }
        let mut fields = Fields::default();
        event.record(&mut fields);
        let correlation = context.event_scope(event).and_then(|mut scope| {
            scope.find_map(|span| {
                span.extensions().get::<Correlation>().map(|entry| {
                    (
                        entry.value.clone(),
                        entry.events.fetch_add(1, Ordering::Relaxed) + 1,
                    )
                })
            })
        });
        let correlation = match correlation {
            Some((value, sequence)) => {
                fields
                    .values
                    .insert("request_event_seq".into(), sequence.into());
                if sequence > MAX_REQUEST_EVENTS {
                    let mut output = self.output.lock().unwrap_or_else(|p| p.into_inner());
                    output.invalid = true;
                    if sequence > MAX_REQUEST_EVENTS + 1 {
                        output.suppressed_events = output.suppressed_events.saturating_add(1);
                        return;
                    }
                    drop(output);
                    fields.values.clear();
                    fields
                        .values
                        .insert("event".into(), "diagnostic_invalid".into());
                    fields
                        .values
                        .insert("reason".into(), "request_event_budget_exceeded".into());
                    fields.values.insert("complete".into(), false.into());
                    fields
                        .values
                        .insert("request_event_seq".into(), sequence.into());
                }
                Some(value)
            }
            None => None,
        };
        self.emit(fields, correlation);
    }
}

/// Protocol success may still carry a tool application error; keep them distinct.
pub(crate) fn mcp_result_outcome(
    result: &Result<rmcp::model::CallToolResult, rmcp::ErrorData>,
) -> &'static str {
    match result {
        Ok(result) if result.is_error == Some(true) => "tool_error",
        Ok(_) => "returned",
        Err(_) => "protocol_error",
    }
}

/// A normal process shutdown closes this diagnostic stream. Forced exit/crash
/// lacks this terminal event and must be treated as incomplete by a consumer.
pub struct Session;
impl Drop for Session {
    fn drop(&mut self) {
        tracing::debug!(target: "codecortex_runtime_diagnostics", event = "session_end");
    }
}

/// Called only after the executable's exact opt-in check. No RUST_LOG parsing,
/// target expansion, query data or alternate output file is introduced.
pub fn install() -> Session {
    tracing_subscriber::registry()
        .with(EnvFilter::new("info,codecortex_runtime_diagnostics=debug"))
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr)
                .with_ansi(false)
                .with_filter(tracing_subscriber::filter::filter_fn(|metadata| {
                    metadata.target() != TARGET
                })),
        )
        .with(DiagnosticLayer::stderr())
        .init();
    tracing::debug!(
        target: "codecortex_runtime_diagnostics",
        event = "session_start",
        clock = "process_monotonic_relative_ns",
        max_request_events = MAX_REQUEST_EVENTS,
        max_line_bytes = MAX_LINE_BYTES as u64,
        max_stream_bytes = MAX_STREAM_BYTES,
    );
    Session
}

#[cfg(test)]
mod tests {
    use super::*;
    use cc_db::runtime_diagnostics::{request_span, BlockingTask};
    use cc_model::{query::QueryControl, CcError};
    use cc_search::execution::ExecutionPool;
    use std::time::Duration;
    use tracing::instrument::WithSubscriber;
    use tracing::Instrument;

    // Each control runs itself in an isolated test process: no global env or
    // subscriber mutation races with the repository's other parallel tests.
    fn isolated(name: &str, opt_in: bool, body: impl FnOnce()) {
        if std::env::var("P8_DIAGNOSTIC_CONTROL").as_deref() == Ok(name) {
            println!("\nP8_DIAGNOSTIC_CONTROL_BEGIN:{name}");
            body();
            println!("\nP8_DIAGNOSTIC_CONTROL_OK:{name}");
            return;
        }
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                &format!("runtime_diagnostics::tests::{name}"),
                "--nocapture",
            ])
            .env("P8_DIAGNOSTIC_CONTROL", name);
        if opt_in {
            command.env("CODECORTEX_RUNTIME_DIAGNOSTICS", "1");
        } else {
            command.env_remove("CODECORTEX_RUNTIME_DIAGNOSTICS");
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success()
                && child_output_complete(name, &String::from_utf8_lossy(&output.stdout)),
            "child control failed or did not run exactly one method: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn child_output_complete(name: &str, stdout: &str) -> bool {
        let begin = format!("P8_DIAGNOSTIC_CONTROL_BEGIN:{name}");
        let end = format!("P8_DIAGNOSTIC_CONTROL_OK:{name}");
        stdout.lines().filter(|line| *line == begin).count() == 1
            && stdout.lines().filter(|line| *line == end).count() == 1
            && stdout
                .matches("test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured;")
                .count()
                == 1
    }

    type Lines = Arc<Mutex<Vec<Vec<u8>>>>;
    fn subscriber() -> (tracing::Dispatch, Lines) {
        let lines = Arc::new(Mutex::new(Vec::new()));
        let copy = lines.clone();
        let layer = DiagnosticLayer::with_writer(move |bytes| {
            copy.lock().unwrap().push(bytes.to_vec());
            Ok(())
        });
        let subscriber = tracing_subscriber::registry()
            .with(EnvFilter::new("codecortex_runtime_diagnostics=debug"))
            .with(layer);
        (tracing::Dispatch::new(subscriber), lines)
    }
    fn records(lines: &Lines) -> Vec<Value> {
        lines
            .lock()
            .unwrap()
            .iter()
            .map(|line| {
                assert!(line.len() <= MAX_LINE_BYTES);
                assert!(!line.contains(&0x1b));
                serde_json::from_slice(line).unwrap()
            })
            .collect()
    }
    fn terminal(records: &[Value], phase: &str, outcome: &str) -> bool {
        records.iter().any(|row| {
            row["phase"] == phase && row["event"] == "finish" && row["outcome"] == outcome
        })
    }
    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap()
    }

    #[test]
    fn default_path_emits_no_diagnostics() {
        isolated("default_path_emits_no_diagnostics", false, || {
            let (dispatch, lines) = subscriber();
            tracing::dispatcher::with_default(&dispatch, || {
                let span = request_span(Some(7), None);
                assert!(span.is_disabled());
                let result = BlockingTask::queued("test_queue", "test_service").run(|| Ok(17));
                assert_eq!(result.unwrap(), 17);
            });
            assert!(records(&lines).is_empty());
        });
    }

    #[test]
    fn typed_request_ids_are_distinct_and_payloads_are_not_logged() {
        isolated(
            "typed_request_ids_are_distinct_and_payloads_are_not_logged",
            true,
            || {
                let (dispatch, lines) = subscriber();
                tracing::dispatcher::with_default(&dispatch, || {
                    for span in [
                        request_span(Some(7), None),
                        request_span(None, Some("7")),
                        request_span(None, Some("quoted\\\"\n雪")),
                        request_span(None, Some(&"x".repeat(200))),
                    ] {
                        span.in_scope(|| {
                            let result = BlockingTask::queued("test_queue", "test_service")
                                .run(|| Ok("SECRET_QUERY_PARAMETER"));
                            assert_eq!(result.unwrap(), "SECRET_QUERY_PARAMETER");
                        });
                    }
                });
                let rows = records(&lines);
                assert!(!serde_json::to_string(&rows)
                    .unwrap()
                    .contains("SECRET_QUERY_PARAMETER"));
                let starts: Vec<_> = rows
                    .iter()
                    .filter(|row| row["phase"] == "test_service" && row["event"] == "start")
                    .collect();
                assert_eq!(starts.len(), 4);
                assert_eq!(starts[0]["correlation"]["request_id"], 7);
                assert_eq!(starts[1]["correlation"]["request_id"], "7");
                assert_ne!(
                    starts[0]["correlation"]["request_seq"],
                    starts[1]["correlation"]["request_seq"]
                );
                assert_eq!(starts[2]["correlation"]["request_id"], "quoted\\\"\n雪");
                assert_eq!(starts[3]["correlation"]["request_id_kind"], "string_hash");
                assert!(starts[3]["correlation"]["request_id"].is_null());
                assert_eq!(starts[3]["correlation"]["request_id_bytes"], 200);
                assert_eq!(
                    starts[3]["correlation"]["request_id_blake3"]
                        .as_str()
                        .unwrap()
                        .len(),
                    64
                );
            },
        );
    }

    #[test]
    fn cancelled_caller_does_not_finish_a_running_worker_or_release_its_permit() {
        isolated(
            "cancelled_caller_does_not_finish_a_running_worker_or_release_its_permit",
            true,
            || {
                let (dispatch, lines) = subscriber();
                tracing::dispatcher::with_default(&dispatch, || {
                    runtime().block_on(async {
                        let pool = ExecutionPool::new(1, 0, 1).unwrap();
                        let control = QueryControl::new(Duration::from_secs(10)).unwrap();
                        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
                        let (release_tx, release_rx) = std::sync::mpsc::channel();
                        let copy = pool.clone();
                        let supplied = control.clone();
                        let task = tokio::spawn(
                            async move {
                                copy.run_cpu(supplied, move || {
                                    started_tx.send(()).unwrap();
                                    release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                                    Ok(())
                                })
                                .await
                            }
                            .instrument(request_span(Some(71), None))
                            .with_current_subscriber(),
                        );
                        started_rx.await.unwrap();
                        control.cancel();
                        assert!(matches!(task.await.unwrap(), Err(CcError::QueryCancelled)));
                        assert_eq!(pool.stats().cpu_in_flight, 1);
                        let before = records(&lines);
                        assert!(terminal(&before, "cpu_caller", "cancelled"));
                        assert!(!before
                            .iter()
                            .any(|r| r["phase"] == "cpu_service" && r["event"] == "finish"));
                        assert!(matches!(
                            pool.run_cpu(
                                QueryControl::new(Duration::from_secs(1)).unwrap(),
                                || Ok(())
                            )
                            .await,
                            Err(CcError::QueryBusy)
                        ));
                        release_tx.send(()).unwrap();
                        tokio::time::timeout(Duration::from_secs(3), async {
                            loop {
                                if pool.stats().cpu_admitted == 0
                                    && terminal(&records(&lines), "cpu_service", "cancelled")
                                {
                                    break;
                                }
                                tokio::task::yield_now().await;
                            }
                        })
                        .await
                        .unwrap();
                        let rows = records(&lines);
                        let finish = rows
                            .iter()
                            .find(|r| r["phase"] == "cpu_service" && r["event"] == "finish")
                            .unwrap();
                        assert_eq!(finish["correlation"]["request_id"], 71);
                        let waits = rows.iter().find(|r| r["event"] == "worker_waits").unwrap();
                        assert_eq!(waits["worker_phase_id"], finish["phase_id"]);
                        assert!(waits["sqlite_busy_ns"].is_null());
                    })
                });
            },
        );
    }

    #[test]
    fn blocked_finish_writer_keeps_cpu_admission_and_running_permit() {
        isolated(
            "blocked_finish_writer_keeps_cpu_admission_and_running_permit",
            true,
            || {
                let lines = Arc::new(Mutex::new(Vec::<Vec<u8>>::new()));
                let copy = lines.clone();
                let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
                let entered = Mutex::new(Some(entered_tx));
                let (release_tx, release_rx) = std::sync::mpsc::channel();
                let release = Mutex::new(release_rx);
                let layer = DiagnosticLayer::with_writer(move |bytes| {
                    let row: Value = serde_json::from_slice(bytes).unwrap();
                    if row["event"] == "finish" && row["phase"] == "cpu_service" {
                        entered.lock().unwrap().take().unwrap().send(()).unwrap();
                        release
                            .lock()
                            .unwrap()
                            .recv_timeout(Duration::from_secs(5))
                            .unwrap();
                    }
                    copy.lock().unwrap().push(bytes.to_vec());
                    Ok(())
                });
                let subscriber = tracing_subscriber::registry()
                    .with(EnvFilter::new("codecortex_runtime_diagnostics=debug"))
                    .with(layer);
                let dispatch = tracing::Dispatch::new(subscriber);
                tracing::dispatcher::with_default(&dispatch, || {
                    runtime().block_on(async {
                        let pool = ExecutionPool::new(1, 0, 1).unwrap();
                        let copy = pool.clone();
                        let task = tokio::spawn(
                            async move {
                                copy.run_cpu(
                                    QueryControl::new(Duration::from_secs(10)).unwrap(),
                                    || Ok(17),
                                )
                                .await
                            }
                            .instrument(request_span(Some(801), None))
                            .with_current_subscriber(),
                        );
                        tokio::time::timeout(Duration::from_secs(3), entered_rx)
                            .await
                            .unwrap()
                            .unwrap();
                        // The original work callback has returned, but the actual
                        // blocking closure is still inside its diagnostic tail.
                        // Pure stats reads avoid logging recursively into the held writer.
                        assert_eq!(pool.stats().cpu_admitted, 1);
                        assert_eq!(pool.stats().cpu_in_flight, 1);
                        release_tx.send(()).unwrap();
                        assert_eq!(task.await.unwrap().unwrap(), 17);
                        assert_eq!(pool.stats().cpu_admitted, 0);
                        assert_eq!(pool.stats().cpu_in_flight, 0);
                    })
                });
                assert!(terminal(&records(&lines), "cpu_service", "ok"));
            },
        );
    }

    #[test]
    fn queued_cancel_does_not_enter_worker_service() {
        isolated("queued_cancel_does_not_enter_worker_service", true, || {
            let (dispatch, lines) = subscriber();
            tracing::dispatcher::with_default(&dispatch, || {
                runtime().block_on(async {
                    let pool = ExecutionPool::new(1, 1, 1).unwrap();
                    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
                    let (release_tx, release_rx) = std::sync::mpsc::channel();
                    let copy = pool.clone();
                    let first = tokio::spawn(
                        async move {
                            copy.run_cpu(
                                QueryControl::new(Duration::from_secs(10)).unwrap(),
                                move || {
                                    started_tx.send(()).unwrap();
                                    release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                                    Ok(())
                                },
                            )
                            .await
                        }
                        .instrument(request_span(Some(1), None))
                        .with_current_subscriber(),
                    );
                    started_rx.await.unwrap();
                    let queued_control = QueryControl::new(Duration::from_secs(10)).unwrap();
                    let supplied = queued_control.clone();
                    let copy = pool.clone();
                    let second = tokio::spawn(
                        async move {
                            copy.run_cpu::<()>(supplied, || panic!("cancelled queued work ran"))
                                .await
                        }
                        .instrument(request_span(Some(2), None))
                        .with_current_subscriber(),
                    );
                    tokio::time::timeout(Duration::from_secs(3), async {
                        while pool.stats().cpu_admitted != 2 {
                            tokio::task::yield_now().await;
                        }
                    })
                    .await
                    .unwrap();
                    queued_control.cancel();
                    assert!(matches!(
                        second.await.unwrap(),
                        Err(CcError::QueryCancelled)
                    ));
                    assert_eq!(pool.stats().cpu_admitted, 1);
                    release_tx.send(()).unwrap();
                    first.await.unwrap().unwrap();
                    let rows: Vec<_> = records(&lines)
                        .into_iter()
                        .filter(|row| row["correlation"]["request_id"] == 2)
                        .collect();
                    assert!(terminal(&rows, "cpu_permit_wait", "cancelled"));
                    assert!(!rows.iter().any(|row| row["phase"] == "cpu_service"));
                })
            });
        });
    }

    #[test]
    fn async_deadline_and_error_outcomes_preserve_parent_and_capacity() {
        isolated(
            "async_deadline_and_error_outcomes_preserve_parent_and_capacity",
            true,
            || {
                let (dispatch, lines) = subscriber();
                tracing::dispatcher::with_default(&dispatch, || {
                    runtime().block_on(async {
                        let pool = ExecutionPool::new(1, 1, 1).unwrap();
                        let parent = QueryControl::new(Duration::from_secs(5)).unwrap();
                        let child = parent.child(Duration::from_millis(10));
                        let result: cc_model::CcResult<()> =
                            pool.run_async(&child, std::future::pending()).await;
                        assert!(matches!(result, Err(CcError::QueryTimedOut)));
                        parent.check().unwrap();
                        assert_eq!(pool.stats().async_admitted, 0);
                        let failed: cc_model::CcResult<()> = pool
                            .run_async(&parent, async {
                                Err(CcError::Search("SECRET_ERROR_TEXT".into()))
                            })
                            .await;
                        assert!(matches!(failed, Err(CcError::Search(_))));
                        assert_eq!(pool.stats().async_in_flight, 0);
                    })
                });
                let rows = records(&lines);
                assert!(terminal(&rows, "async_service", "timed_out"));
                assert!(terminal(&rows, "async_service", "error"));
                assert!(!serde_json::to_string(&rows)
                    .unwrap()
                    .contains("SECRET_ERROR_TEXT"));
            },
        );
    }

    #[test]
    fn database_waits_are_bounded_aggregates_with_unobserved_busy_null() {
        isolated(
            "database_waits_are_bounded_aggregates_with_unobserved_busy_null",
            true,
            || {
                let temp = tempfile::tempdir().unwrap();
                let db = cc_db::index_db::IndexDb::open(&temp.path().join("diagnostic.sqlite3"))
                    .unwrap()
                    .0;
                let (dispatch, lines) = subscriber();
                tracing::dispatcher::with_default(&dispatch, || {
                    BlockingTask::queued("test_queue", "test_service")
                        .run(|| {
                            for _ in 0..20 {
                                db.reads().get_metadata("test_key")?;
                            }
                            Ok(())
                        })
                        .unwrap();
                });
                let rows = records(&lines);
                let waits: Vec<_> = rows
                    .iter()
                    .filter(|row| row["event"] == "worker_waits")
                    .collect();
                assert_eq!(waits.len(), 1);
                assert!(!rows.iter().any(|row| row["event"] == "unscoped_wait"));
                let metrics = waits[0]["waits"].as_array().unwrap();
                assert_eq!(metrics.len(), 7);
                let pooled = metrics
                    .iter()
                    .find(|m| m["wait"] == "db_read_pool")
                    .unwrap();
                assert_eq!(pooled["count"], 20);
                assert!(pooled["sum_ns"].as_u64().unwrap() >= pooled["max_ns"].as_u64().unwrap());
                let unused = metrics
                    .iter()
                    .find(|m| m["wait"] == "db_write_mutex")
                    .unwrap();
                assert_eq!(unused["count"], 0);
                assert!(unused["sum_ns"].is_null());
                assert!(unused["max_ns"].is_null());
                assert!(waits[0]["sqlite_busy_ns"].is_null());
                assert_eq!(waits[0]["complete"], true);
            },
        );
    }

    #[test]
    fn nested_origin_is_nearest_and_caught_wait_unwind_is_incomplete() {
        isolated(
            "nested_origin_is_nearest_and_caught_wait_unwind_is_incomplete",
            true,
            || {
                let (dispatch, lines) = subscriber();
                tracing::dispatcher::with_default(&dispatch, || {
                    request_span(Some(1), None).in_scope(|| {
                        cc_db::runtime_diagnostics::local_span(3, "fixture", 7, 1).in_scope(|| {
                            BlockingTask::queued("test_queue", "test_service")
                                .run(|| {
                                    let caught = std::panic::catch_unwind(|| {
                                        let _wait = cc_db::runtime_diagnostics::Wait::start(
                                            cc_db::runtime_diagnostics::WaitKind::DbWrite,
                                        );
                                        panic!(
                                            "acquisition panicked before a terminal observation"
                                        );
                                    });
                                    assert!(caught.is_err());
                                    Ok(())
                                })
                                .unwrap();
                        });
                    });
                });
                let rows = records(&lines);
                assert!(rows
                    .iter()
                    .all(|row| row["correlation"]["kind"] == "local_operation"));
                let waits = rows
                    .iter()
                    .find(|row| row["event"] == "worker_waits")
                    .unwrap();
                assert_eq!(waits["complete"], false);
                let metric = waits["waits"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|m| m["wait"] == "db_write_mutex")
                    .unwrap();
                assert_eq!(metric["count"], 0);
                assert_eq!(metric["incomplete"], 1);
                assert!(metric["sum_ns"].is_null());
                assert!(terminal(&rows, "test_service", "ok"));
            },
        );
    }

    #[test]
    fn child_output_rejects_zero_filtered_and_missing_completion() {
        let name = "fixture";
        assert!(!child_output_complete(
            name,
            "running 0 tests\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured;\n"
        ));
        assert!(!child_output_complete(name,
            "P8_DIAGNOSTIC_CONTROL_BEGIN:fixture\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured;\n"));
        assert!(child_output_complete(name,
            "P8_DIAGNOSTIC_CONTROL_BEGIN:fixture\nP8_DIAGNOSTIC_CONTROL_OK:fixture\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured;\n"));
    }

    #[test]
    fn request_budget_emits_one_invalid_and_retains_incomplete_session() {
        isolated(
            "request_budget_emits_one_invalid_and_retains_incomplete_session",
            true,
            || {
                let (dispatch, lines) = subscriber();
                tracing::dispatcher::with_default(&dispatch, || {
                    request_span(Some(99), None).in_scope(|| {
                        for _ in 0..130 {
                            cc_db::runtime_diagnostics::Phase::start("bounded_test").finish("ok");
                        }
                    });
                    tracing::debug!(target: "codecortex_runtime_diagnostics", event = "session_end");
                });
                let rows = records(&lines);
                assert_eq!(rows.len(), MAX_REQUEST_EVENTS as usize + 2);
                let invalid: Vec<_> = rows
                    .iter()
                    .filter(|row| row["event"] == "diagnostic_invalid")
                    .collect();
                assert_eq!(invalid.len(), 1);
                assert_eq!(invalid[0]["request_event_seq"], MAX_REQUEST_EVENTS + 1);
                assert_eq!(invalid[0]["reason"], "request_event_budget_exceeded");
                assert_eq!(rows.last().unwrap()["complete"], false);
                assert_eq!(rows.last().unwrap()["suppressed_events"], 3);
            },
        );
    }

    #[test]
    fn mcp_application_error_is_not_reported_as_success() {
        use rmcp::model::CallToolResult;
        assert_eq!(
            mcp_result_outcome(&Ok(CallToolResult::success(vec![]))),
            "returned"
        );
        assert_eq!(
            mcp_result_outcome(&Ok(CallToolResult::error(vec![]))),
            "tool_error"
        );
        assert_eq!(
            mcp_result_outcome(&Err(rmcp::ErrorData::internal_error(
                "private error remains only in the original result",
                None
            ))),
            "protocol_error"
        );
    }

    #[test]
    fn real_writer_stays_within_small_total_cap_and_closes_once() {
        let lines = Arc::new(Mutex::new(Vec::<Vec<u8>>::new()));
        let copy = lines.clone();
        let mut layer = DiagnosticLayer::with_writer(move |bytes| {
            copy.lock().unwrap().push(bytes.to_vec());
            Ok(())
        });
        // Same production emit path, with a small test-only instance budget.
        layer.max_stream_bytes = 3 * MAX_LINE_BYTES as u64;
        for _ in 0..100 {
            let mut event = Fields::default();
            event.values.insert("event".into(), "fixture".into());
            layer.emit(event, None);
        }
        let mut end = Fields::default();
        end.values.insert("event".into(), "session_end".into());
        layer.emit(end, None);
        let rows = records(&lines);
        assert_eq!(
            rows.iter()
                .filter(|r| r["event"] == "diagnostic_invalid")
                .count(),
            1
        );
        assert_eq!(
            rows.iter().filter(|r| r["event"] == "session_end").count(),
            1
        );
        assert_eq!(rows.last().unwrap()["complete"], false);
        let writer_bytes: u64 = lines
            .lock()
            .unwrap()
            .iter()
            .map(|line| line.len() as u64)
            .sum();
        assert_eq!(writer_bytes, layer.output.lock().unwrap().written_bytes);
        assert!(writer_bytes <= layer.max_stream_bytes);
    }

    #[test]
    fn output_bounds_emit_explicit_invalid_and_incomplete_terminal() {
        let lines = Arc::new(Mutex::new(Vec::<Vec<u8>>::new()));
        let copy = lines.clone();
        let layer = DiagnosticLayer::with_writer(move |bytes| {
            copy.lock().unwrap().push(bytes.to_vec());
            Ok(())
        });
        let oversized = Fields {
            invalid: true,
            ..Fields::default()
        };
        layer.emit(oversized, None);
        layer.output.lock().unwrap().written_bytes = MAX_STREAM_BYTES - STREAM_TERMINAL_RESERVE;
        let mut normal = Fields::default();
        normal.values.insert("event".into(), "fixture".into());
        layer.emit(normal, None);
        let mut suppressed = Fields::default();
        suppressed.values.insert("event".into(), "fixture".into());
        layer.emit(suppressed, None);
        let mut end = Fields::default();
        end.values.insert("event".into(), "session_end".into());
        layer.emit(end, None);
        let rows = records(&lines);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0]["event"], "diagnostic_invalid");
        assert_eq!(rows[1]["reason"], "stream_budget_exceeded");
        assert_eq!(rows[2]["event"], "session_end");
        assert_eq!(rows[2]["complete"], false);
        assert_eq!(rows[2]["suppressed_events"], 1);
        let actual_tail_bytes: usize = lines.lock().unwrap().iter().skip(1).map(Vec::len).sum();
        assert!(actual_tail_bytes <= 2 * MAX_LINE_BYTES);
        assert_eq!(
            layer.output.lock().unwrap().written_bytes,
            (MAX_STREAM_BYTES - STREAM_TERMINAL_RESERVE) + actual_tail_bytes as u64
        );
        assert!(layer.output.lock().unwrap().written_bytes <= MAX_STREAM_BYTES);
        // When an invalid/oversized field and stream exhaustion coincide,
        // emit only the one stream-invalid marker, never two extra lines.
        let both_lines = Arc::new(Mutex::new(Vec::<Vec<u8>>::new()));
        let copy = both_lines.clone();
        let both = DiagnosticLayer::with_writer(move |bytes| {
            copy.lock().unwrap().push(bytes.to_vec());
            Ok(())
        });
        both.output.lock().unwrap().written_bytes = MAX_STREAM_BYTES - STREAM_TERMINAL_RESERVE;
        both.emit(
            Fields {
                invalid: true,
                ..Fields::default()
            },
            None,
        );
        let mut end = Fields::default();
        end.values.insert("event".into(), "session_end".into());
        both.emit(end, None);
        let rows = records(&both_lines);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["reason"], "stream_budget_exceeded");
        assert_eq!(rows[1]["complete"], false);
        assert!(both.output.lock().unwrap().written_bytes <= MAX_STREAM_BYTES);
    }
}
