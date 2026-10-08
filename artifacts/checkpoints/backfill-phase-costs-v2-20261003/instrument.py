import pathlib,subprocess
O=pathlib.Path(__file__).resolve().parent;R=O/'source'
def edit(p,a,b):
 f=R/p;s=f.read_text();assert a in s,(p,a);f.write_text(s.replace(a,b,1))
(R/'crates/cc-db/src/phase_cost.rs').write_text('''//! Temporary diagnostic timer. No production schema or semantics change.
use std::{collections::BTreeMap, sync::{Mutex,OnceLock}, time::Instant};
static DATA: OnceLock<Mutex<BTreeMap<&'static str,Vec<u64>>>> = OnceLock::new();
pub struct Span(&'static str,Instant);
pub fn span(name: &'static str)->Span { Span(name,Instant::now()) }
impl Drop for Span { fn drop(&mut self) { let ns=self.1.elapsed().as_nanos() as u64; DATA.get_or_init(Default::default).lock().unwrap().entry(self.0).or_default().push(ns); } }
pub fn measure<T>(name: &'static str, f:impl FnOnce()->T)->T { let _span=span(name); f() }
pub fn flush() { let data=std::mem::take(&mut *DATA.get_or_init(Default::default).lock().unwrap()); eprintln!("PHASE_COST {}",serde_json::to_string(&data).unwrap()); }
''')
edit('crates/cc-db/src/lib.rs','pub mod semantic_outbox;','pub mod semantic_outbox;\npub mod phase_cost;')
edit('crates/cc-semantic/src/queue.rs','db.reclaim_expired_semantic()?;','cc_db::phase_cost::measure("reclaim", || db.reclaim_expired_semantic())?;')
edit('crates/cc-semantic/src/queue.rs','let Some(task) = db.claim_semantic_with_lifecycle(','let Some(task) = cc_db::phase_cost::measure("claim", || db.claim_semantic_with_lifecycle(')
edit('crates/cc-semantic/src/queue.rs','            lifecycle,\n        )?\n        else {','            lifecycle,\n        ))?\n        else {')
edit('crates/cc-semantic/src/queue.rs','if !guard.renew()? {','if !cc_db::phase_cost::measure("renew", || guard.renew())? {')
edit('crates/cc-semantic/src/queue.rs','let Some(input) = (self.resolve_input)(task)? else {','let Some(input) = cc_db::phase_cost::measure("input_resolve", || (self.resolve_input)(task))? else {')
edit('crates/cc-semantic/src/queue.rs','match self.provider.embed_documents(&batch) {','match cc_db::phase_cost::measure("provider_including_cache_lookup", || self.provider.embed_documents(&batch)) {')
edit('crates/cc-semantic/src/publish.rs','let input = InputDigest::new(task.input_digest.clone());','let _publish_span = cc_db::phase_cost::span("publish_total_inclusive");\n        let input = InputDigest::new(task.input_digest.clone());')
edit('crates/cc-semantic/src/publish.rs','match self\n                .cache\n                .put(self.space, &input, self.doc_spec, vector, now_unix)','match cc_db::phase_cost::measure("cache_durable_put", || self.cache.put(self.space, &input, self.doc_spec, vector, now_unix))')
edit('crates/cc-semantic/src/publish.rs','match self.cache.get(self.space, &input, self.doc_spec)? {','match cc_db::phase_cost::measure("cache_readback", || self.cache.get(self.space, &input, self.doc_spec))? {')
edit('crates/cc-semantic/src/publish.rs','let Some(outcome) = self\n            .db\n            .publish_semantic_with_lifecycle(&request, self.lifecycle)?','let Some(outcome) = cc_db::phase_cost::measure("publish_cas", || self.db.publish_semantic_with_lifecycle(&request, self.lifecycle))?')
edit('crates/cc-server/src/semantic_runtime.rs','fn run_round_with(&self, provider: &dyn EmbeddingProvider) -> CcResult<bool> {','fn run_round_with(&self, provider: &dyn EmbeddingProvider) -> CcResult<bool> {\n        let round_span = cc_db::phase_cost::span("round_total_inclusive");\n        let reconcile_span = cc_db::phase_cost::span("round_reconcile");')
edit('crates/cc-server/src/semantic_runtime.rs','        let now = std::time::SystemTime::now()','        drop(reconcile_span);\n        let now = std::time::SystemTime::now()')
edit('crates/cc-server/src/semantic_runtime.rs','        Ok(!self.closed.load(Ordering::Acquire) && (backfilling || outcome.batch.claimed == 16))','        let more = !self.closed.load(Ordering::Acquire) && (backfilling || outcome.batch.claimed == 16);\n        drop(round_span);\n        cc_db::phase_cost::flush();\n        Ok(more)')
# FencedProvider cache lookup is nested in provider span; no logging per task.
edit('crates/cc-server/src/semantic_runtime.rs','        let mut cached = Vec::with_capacity(batch.len());','        let cache_span = cc_db::phase_cost::span("provider_cache_lookup_nested");\n        let mut cached = Vec::with_capacity(batch.len());')
edit('crates/cc-server/src/semantic_runtime.rs','        let result = if !batch.is_empty() && cached.len() == batch.len() {','        drop(cache_span);\n        let result = if !batch.is_empty() && cached.len() == batch.len() {')
lines=(R/'crates/cc-db/src/phase_cost.rs').read_text().splitlines()
patch=subprocess.check_output(['git','diff'],cwd=R,text=True)
patch+='diff --git a/crates/cc-db/src/phase_cost.rs b/crates/cc-db/src/phase_cost.rs\nnew file mode 100644\n--- /dev/null\n+++ b/crates/cc-db/src/phase_cost.rs\n@@ -0,0 +1,%d @@\n'%len(lines)+''.join('+'+x+'\n' for x in lines)
(O/'instrumentation.patch').write_text(patch)
