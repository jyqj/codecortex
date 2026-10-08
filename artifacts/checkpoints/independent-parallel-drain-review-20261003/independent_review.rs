use super::*;
use cc_model::config::ProjectConfig;
use cc_semantic::{admission::ProviderGate, providers::fake::{FakeProvider,FakeProviderConfig}};
use std::sync::atomic::AtomicUsize;
struct NormalFailure { space: cc_semantic::types::VectorSpace, calls: AtomicUsize }
impl EmbeddingProvider for NormalFailure {
 fn space(&self)->&cc_semantic::types::VectorSpace { &self.space }
 fn embed_documents(&self,_:&[DocumentInput])->Result<Vec<Vec<f32>>,ProviderError>{self.calls.fetch_add(1,Ordering::SeqCst);Err(ProviderError::ServerError)}
 fn embed_queries(&self,_:&[QueryInput])->Result<Vec<Vec<f32>>,ProviderError>{Err(ProviderError::ServerError)}
}
fn fixture(n:usize,width:u32)->(tempfile::TempDir,Arc<SemanticRuntime>,Arc<NormalFailure>,Arc<ProviderGate>) {
 let dir=tempfile::tempdir_in("/tmp").unwrap();
 let mut c=ProjectConfig::default();c.auto_index.enabled=false;c.semantic.enabled=true;
 c.semantic.model_id="fake/independent-review".into();c.semantic.dimensions=Some(2);
 c.semantic.max_input_tokens=Some(8192);c.semantic.max_batch_items=Some(16);
 c.semantic.max_concurrent=if width>=4 {8}else{4};c.semantic.max_concurrent_per_project=width;
 c.semantic.endpoint="https://semantic.invalid/v1".into();
 std::fs::write(dir.path().join(".codecortex.json"),serde_json::to_vec(&c).unwrap()).unwrap();
 for i in 0..n {std::fs::write(dir.path().join(format!("document_{i}.rs")),format!("pub fn independent_{i}() -> u64 {{ {i} }}\n")).unwrap();}
 let mut index=crate::engine::CodeIndex::new(Some(dir.path())).unwrap();
 let subsystem=index.semantic_subsystem().unwrap();
 index.install_semantic_provider(Arc::new(FakeProvider::new(FakeProviderConfig::new(subsystem.space.clone())))).unwrap();
 let old=index.semantic_runtime().unwrap();old.close();
 crate::handlers::core::build_index(Arc::new(std::sync::RwLock::new(index)),true).unwrap();
 let inner=Arc::new(NormalFailure{space:subsystem.space.clone(),calls:AtomicUsize::new(0)});
 let gate=Arc::new(ProviderGate::validated(c.semantic.max_concurrent as usize,if width==0 {None}else{Some(width as usize)}).unwrap());
 let admitted=Arc::new(AdmittedProvider{inner:inner.clone(),gate:Some(gate.clone()),namespace:"independent".into(),wait:std::time::Duration::from_millis(100),cancellation:tokio_util::sync::CancellationToken::new(),control:None});
 let w=SemanticRuntime::new(old.db.clone(),subsystem,old.services.clone(),admitted).unwrap();
 w.authorize_configured_space_transition();(dir,w,inner,gate)
}
fn counts(w:&SemanticRuntime)->(i64,i64,i64) {
 w.db.read_conn().unwrap().query_row("SELECT COUNT(*), SUM(CASE WHEN state='pending' AND available_at <= unixepoch('now','subsec') THEN 1 ELSE 0 END), SUM(CASE WHEN attempt_count=0 THEN 1 ELSE 0 END) FROM semantic_outbox",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap()
}
#[test]
fn independent_review_retry_round_strands_ready_rows() {
 let (_dir,w,p,gate)=fixture(12,2);
 let more=w.run_round_with(w.resolve_provider().unwrap().as_ref()).unwrap();
 let (total,ready,unstarted)=counts(&w);
 println!("INDEPENDENT_RETRY_ROUND more={more} rows={total} ready={ready} unstarted={unstarted} provider_calls={} cursor_none={} gate_inflight={}",p.calls.load(Ordering::SeqCst),w.backfill_cursor.lock().unwrap().is_none(),gate.snapshot().in_flight);
 assert!(!more);assert!(ready>0);assert!(unstarted>0);assert!(w.backfill_cursor.lock().unwrap().is_none());assert_eq!(gate.snapshot().in_flight,0);
}
#[tokio::test(flavor="multi_thread",worker_threads=2)]
async fn independent_review_actual_schedule_reaches_idle_with_ready_rows() {
 let (_dir,w,p,gate)=tokio::task::spawn_blocking(||fixture(12,2)).await.unwrap();
 assert!(w.schedule());
 tokio::time::timeout(std::time::Duration::from_secs(10),async {while w.running.load(Ordering::Acquire)||w.services.query_pins()!=0 {tokio::task::yield_now().await;}}).await.unwrap();
 let (total,ready,unstarted)=counts(&w);
 println!("INDEPENDENT_RETRY_SCHEDULE rows={total} ready={ready} unstarted={unstarted} provider_calls={} running={} pins={} requested={} gate_inflight={}",p.calls.load(Ordering::SeqCst),w.running.load(Ordering::Acquire),w.services.query_pins(),w.requested.load(Ordering::Acquire),gate.snapshot().in_flight);
 assert!(ready>0);assert!(unstarted>0);assert!(!w.requested.load(Ordering::Acquire));assert_eq!(gate.snapshot().in_flight,0);
}
#[derive(Default)]
struct Latch { state: Mutex<(usize,bool)>, cv: std::sync::Condvar }
impl Latch {
 fn enter(&self) {let mut s=self.state.lock().unwrap();s.0+=1;self.cv.notify_all();while !s.1 {let (v,t)=self.cv.wait_timeout(s,std::time::Duration::from_secs(5)).unwrap();s=v;assert!(!t.timed_out(),"finite provider latch timeout");}}
 fn wait(&self,n:usize) {let mut s=self.state.lock().unwrap();while s.0<n {let (v,t)=self.cv.wait_timeout(s,std::time::Duration::from_secs(5)).unwrap();s=v;assert!(!t.timed_out(),"expected provider width not reached");}}
 fn release(&self){self.state.lock().unwrap().1=true;self.cv.notify_all();}
}
struct Held { fake:FakeProvider, latch:Arc<Latch>,active:AtomicUsize,peak:AtomicUsize,calls:AtomicUsize }
impl EmbeddingProvider for Held {
 fn space(&self)->&cc_semantic::types::VectorSpace{self.fake.space()}
 fn embed_documents(&self,b:&[DocumentInput])->Result<Vec<Vec<f32>>,ProviderError>{let a=self.active.fetch_add(1,Ordering::SeqCst)+1;self.peak.fetch_max(a,Ordering::SeqCst);self.calls.fetch_add(1,Ordering::SeqCst);self.latch.enter();let r=self.fake.embed_documents(b);self.active.fetch_sub(1,Ordering::SeqCst);r}
 fn embed_queries(&self,b:&[QueryInput])->Result<Vec<Vec<f32>>,ProviderError>{self.fake.embed_queries(b)}
}
fn held_worker(n:usize,width:u32)->(tempfile::TempDir,Arc<SemanticRuntime>,Arc<Held>,Arc<ProviderGate>){
 let (dir,old,_,gate)=fixture(n,width);old.close();
 let inner=Arc::new(Held{fake:FakeProvider::new(FakeProviderConfig::new(old.subsystem.space.clone())),latch:Arc::new(Latch::default()),active:AtomicUsize::new(0),peak:AtomicUsize::new(0),calls:AtomicUsize::new(0)});
 let w=SemanticRuntime::new_with_factory(old.db.clone(),old.subsystem.clone(),old.services.clone(),{let inner=inner.clone();let gate=gate.clone();move |cancellation|Ok(Arc::new(AdmittedProvider{inner:inner.clone(),gate:Some(gate.clone()),namespace:"independent".into(),wait:std::time::Duration::from_millis(100),cancellation,control:None}) as Arc<dyn EmbeddingProvider>)}).unwrap();
 w.authorize_configured_space_transition();(dir,w,inner,gate)
}
async fn idle(w:&Arc<SemanticRuntime>){tokio::time::timeout(std::time::Duration::from_secs(10),async{while w.running.load(Ordering::Acquire)||w.services.query_pins()!=0{tokio::task::yield_now().await;}}).await.unwrap();}
#[tokio::test(flavor="multi_thread",worker_threads=2)]
async fn independent_review_close_preserves_physical_ownership(){
 let (_dir,w,p,gate)=tokio::task::spawn_blocking(||held_worker(20,2)).await.unwrap();assert!(w.schedule());
 let l=p.latch.clone();tokio::task::spawn_blocking(move||l.wait(2)).await.unwrap();
 assert_eq!(gate.snapshot().in_flight,2);assert_eq!(w.services.query_pins(),1);
 // Real DB read and write both complete while both provider calls are blocked.
 let db=w.db.clone();tokio::task::spawn_blocking(move||{db.reads().read_generation().unwrap();db.enqueue_semantic_rebuild_plan(&[]).unwrap();}).await.unwrap();
 w.close();assert!(!w.schedule());assert!(w.running.load(Ordering::Acquire));assert_eq!(w.services.query_pins(),1);assert_eq!(gate.snapshot().in_flight,2);assert_eq!(p.active.load(Ordering::SeqCst),2);
 p.latch.release();idle(&w).await;assert_eq!(gate.snapshot().in_flight,0);assert_eq!(p.active.load(Ordering::SeqCst),0);assert_eq!(p.calls.load(Ordering::SeqCst),2);
 assert_eq!(w.db.reads().semantic_coverage().unwrap().coverage.published,0);
 let conn=w.db.read_conn().unwrap();let claimed:i64=conn.query_row("SELECT COUNT(*) FROM semantic_outbox WHERE state='claimed'",[],|r|r.get(0)).unwrap();assert_eq!(claimed,0);
 println!("INDEPENDENT_CLOSE peak=2 pins_while_physical_calls=1 gate_after_join=0 claimed_after_join=0 published=0 db_read_write_while_calls=ok");
}
#[tokio::test(flavor="multi_thread",worker_threads=2)]
async fn independent_review_requested_race_and_successful_continuation(){
 let (_dir,w,p,gate)=tokio::task::spawn_blocking(||held_worker(40,2)).await.unwrap();assert!(w.schedule());let l=p.latch.clone();tokio::task::spawn_blocking(move||l.wait(2)).await.unwrap();
 assert!(!w.schedule());assert!(w.requested.load(Ordering::Acquire));p.latch.release();idle(&w).await;
 let c=w.db.reads().semantic_coverage().unwrap().coverage;assert_eq!(c.published,40);assert_eq!(p.calls.load(Ordering::SeqCst),40);assert_eq!(p.peak.load(Ordering::SeqCst),2);assert_eq!(gate.snapshot().in_flight,0);
 println!("INDEPENDENT_SUCCESS published=40 calls=40 peak=2 coalesced_busy_request=retained pins_after_join=0");
}
#[tokio::test(flavor="multi_thread",worker_threads=2)]
async fn independent_review_normal_space_switch_fences_publication(){
 let (_dir,w,p,gate)=tokio::task::spawn_blocking(||held_worker(12,2)).await.unwrap();assert!(w.schedule());let l=p.latch.clone();tokio::task::spawn_blocking(move||l.wait(2)).await.unwrap();
 let db=w.db.clone();tokio::task::spawn_blocking(move||{let space=cc_semantic::types::VectorSpace::new("fake/independent-second-space",2).unwrap();let spec=cc_semantic::spec::DocumentEncodingSpec::new(space.clone(),None,8192,cc_model::chunk_policy::TOKEN_ESTIMATOR).unwrap();cc_semantic::space_switch::register_backfill_space(&db,&spec).unwrap();cc_semantic::space_switch::activate_space(&db,&space,"normal independent transition").unwrap();}).await.unwrap();
 p.latch.release();idle(&w).await;assert_eq!(gate.snapshot().in_flight,0);assert_eq!(p.calls.load(Ordering::SeqCst),2);
 let conn=w.db.read_conn().unwrap();let publications:i64=conn.query_row("SELECT COUNT(*) FROM semantic_manifest",[],|r|r.get(0)).unwrap();assert_eq!(publications,0);
 println!("INDEPENDENT_SPACE_SWITCH provider_calls=2 published_old_vectors=0 gate_after_join=0");
}
#[test]
fn independent_review_width_and_budget_with_real_publish(){
 for width in [0,1,2,4] {
 let (_dir,w,_,gate)=held_worker(40,width);let p=Arc::new(Held{fake:FakeProvider::new(FakeProviderConfig::new(w.subsystem.space.clone())),latch:Arc::new(Latch::default()),active:AtomicUsize::new(0),peak:AtomicUsize::new(0),calls:AtomicUsize::new(0)});
 let admitted=AdmittedProvider{inner:p.clone(),gate:Some(gate.clone()),namespace:"bounded".into(),wait:std::time::Duration::from_millis(100),cancellation:tokio_util::sync::CancellationToken::new(),control:None};
 std::thread::scope(|s|{let job=s.spawn(||w.run_round_with(&admitted));p.latch.wait(if width>=2{2}else{1});p.latch.release();assert!(job.join().unwrap().unwrap());});
 let (done,distinct_tokens,attempts):(i64,i64,i64)=w.db.read_conn().unwrap().query_row("SELECT COUNT(*), COUNT(DISTINCT lease_token), SUM(attempt_count) FROM semantic_outbox WHERE state='done'",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();assert_eq!((done,distinct_tokens,attempts),(16,16,16));
 assert_eq!(p.calls.load(Ordering::SeqCst),16);assert_eq!(w.db.reads().semantic_coverage().unwrap().coverage.published,16);assert!(p.peak.load(Ordering::SeqCst)<=if width>=2{2}else{1});assert_eq!(gate.snapshot().in_flight,0);
 println!("INDEPENDENT_WIDTH configured={width} calls=16 published=16 peak={} effective_global={} effective_project={:?}",p.peak.load(Ordering::SeqCst),gate.snapshot().max_concurrent,gate.snapshot().max_concurrent_per_project);
 }
}
#[test]
fn independent_review_multi_project_gate(){
 let space=cc_semantic::types::VectorSpace::new("fake/independent-gate",2).unwrap();let gate=Arc::new(ProviderGate::validated(4,Some(2)).unwrap());let p=Arc::new(Held{fake:FakeProvider::new(FakeProviderConfig::new(space)),latch:Arc::new(Latch::default()),active:AtomicUsize::new(0),peak:AtomicUsize::new(0),calls:AtomicUsize::new(0)});
 let wrap=|namespace:&str|AdmittedProvider{inner:p.clone(),gate:Some(gate.clone()),namespace:namespace.into(),wait:std::time::Duration::from_millis(10),cancellation:tokio_util::sync::CancellationToken::new(),control:None};
 let input=DocumentInput::from_bytes(b"ordinary gate probe").unwrap();
 std::thread::scope(|s|{let jobs:Vec<_>=["a","a","b","b"].into_iter().map(|name|{let provider=wrap(name);let input=input.clone();s.spawn(move||provider.embed_documents(&[input]))}).collect();p.latch.wait(4);let snapshot=gate.snapshot();assert_eq!(snapshot.in_flight,4);assert_eq!(snapshot.per_project_in_flight["a"],2);assert_eq!(snapshot.per_project_in_flight["b"],2);assert_eq!(wrap("c").embed_documents(std::slice::from_ref(&input)),Err(ProviderError::Timeout));p.latch.release();for j in jobs{j.join().unwrap().unwrap();}});
 let a1=gate.try_acquire_permit("a",std::time::Duration::from_millis(10)).unwrap();let a2=gate.try_acquire_permit("a",std::time::Duration::from_millis(10)).unwrap();assert_eq!(wrap("a").embed_documents(&[input]),Err(ProviderError::Timeout));drop((a1,a2));assert_eq!(gate.snapshot().in_flight,0);assert_eq!(p.calls.load(Ordering::SeqCst),4);println!("INDEPENDENT_GATE peak=4 project_a=2 project_b=2 overlimit_inner_calls=0 final_permits=0");
}
fn prepared(n:usize)->(tempfile::TempDir,Arc<SemanticRuntime>){let (d,w,_,_)=fixture(n,2);w.run_round_with(w.resolve_provider().unwrap().as_ref()).unwrap();let c=rusqlite::Connection::open(w.db.admin().db_path()).unwrap();c.execute("UPDATE semantic_outbox SET available_at=0, attempt_count=0 WHERE state='pending'",[]).unwrap();(d,w)}
#[test]
fn independent_review_unstarted_cancellation_refunds_and_token_renewal_is_fenced(){
 let (_dir,w)=prepared(3);let limits=cc_semantic::queue::WorkerLimits::validated(16,60.0,30.0,3).unwrap();
 let observed=Mutex::new(Vec::new());
 let r=cc_semantic::queue::drain_pending_parallel_with_lifecycle(&w.db,"independent-unstarted",&limits,None,2,&|g|{assert!(g.renew()?);assert!(!w.db.renew_semantic_lease(g.task().task_id,"other-token",60.0)?);observed.lock().unwrap().push((g.task().task_id,g.task().token.clone()));Ok(cc_semantic::queue::TaskExit::Cancelled{started:false})}).unwrap();
 let seen=observed.lock().unwrap();assert!(r.claimed>=1&&r.claimed<=2);let tokens:std::collections::HashSet<_>=seen.iter().map(|r|&r.1).collect();assert_eq!(tokens.len(),seen.len());
 let c=w.db.read_conn().unwrap();let (charged,claimed):(i64,i64)=c.query_row("SELECT SUM(attempt_count), SUM(CASE WHEN state='claimed' THEN 1 ELSE 0 END) FROM semantic_outbox",[],|r|Ok((r.get(0)?,r.get(1)?))).unwrap();assert_eq!(charged,0);assert_eq!(claimed,0);
 for (id,token) in seen.iter(){assert!(!w.db.renew_semantic_lease(*id,token,60.0).unwrap());}
 println!("INDEPENDENT_UNSTARTED claims={} observed_handler_tokens={} charges=0 claimed=0 stale_renew=false",r.claimed,tokens.len());
}
#[test]
fn independent_review_unhandled_error_joins_companion_before_return(){
 let (_dir,w)=prepared(4);let limits=cc_semantic::queue::WorkerLimits::validated(16,60.0,30.0,3).unwrap();let gate=Arc::new(ProviderGate::validated(4,Some(2)).unwrap());
 let p=Arc::new(Held{fake:FakeProvider::new(FakeProviderConfig::new(w.subsystem.space.clone())),latch:Arc::new(Latch::default()),active:AtomicUsize::new(0),peak:AtomicUsize::new(0),calls:AtomicUsize::new(0)});
 let admitted=AdmittedProvider{inner:p.clone(),gate:Some(gate.clone()),namespace:"join".into(),wait:std::time::Duration::from_millis(100),cancellation:tokio_util::sync::CancellationToken::new(),control:None};let ordinal=AtomicUsize::new(0);let (tx,rx)=std::sync::mpsc::channel();
 std::thread::scope(|s|{let j=s.spawn(||cc_semantic::queue::drain_pending_parallel_with_lifecycle(&w.db,"independent-error",&limits,None,2,&|g|{if ordinal.fetch_add(1,Ordering::SeqCst)==0{p.latch.wait(1);tx.send(()).unwrap();Err(cc_model::CcError::InvalidParams("ordinary handler refuses one task".into()))}else{admitted.embed_documents(&[DocumentInput::from_bytes(b"physical companion").unwrap()]).unwrap();w.db.hand_back_cancelled_semantic_task(g.task().task_id,&g.task().token,true)?;Ok(cc_semantic::queue::TaskExit::Disposed)}}));rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();assert!(!j.is_finished());assert_eq!(gate.snapshot().in_flight,1);p.latch.release();assert!(j.join().unwrap().is_err());});assert_eq!(gate.snapshot().in_flight,0);assert_eq!(p.active.load(Ordering::SeqCst),0);let c=w.db.read_conn().unwrap();let claimed:i64=c.query_row("SELECT COUNT(*) FROM semantic_outbox WHERE state='claimed'",[],|r|r.get(0)).unwrap();assert_eq!(claimed,0);println!("INDEPENDENT_ERR returned_after_physical_join=true permits=0 claimed=0");
}
#[test]
fn independent_review_serial_api_continues_after_retry_but_width_zero_runtime_stops(){
 let (_dir,w,p,_)=fixture(12,0);let more=w.run_round_with(w.resolve_provider().unwrap().as_ref()).unwrap();assert!(!more);assert_eq!(p.calls.load(Ordering::SeqCst),1);assert_eq!(counts(&w).1,11);
 let provider=w.resolve_provider().unwrap();let outcome=semantic_wiring::drain_worker_batch(&w.db,&w.subsystem,&w.services,provider.as_ref(),&|t|cc_db::document_store::semantic_worker_input(&w.db,&t.doc_key,&t.doc_version,&t.input_digest)?.map(|text|DocumentInput::from_bytes(text.as_bytes())).transpose(),semantic_wiring::WorkerDrainOptions{owner:"independent-serial",max_batch:16,now_unix:0,lifecycle:Some(&w.lifecycle)}).unwrap();
 assert_eq!(outcome.batch.claimed,11);assert_eq!(outcome.batch.retried,11);println!("INDEPENDENT_SERIAL_DIFF width0_runtime_calls=1 more=false ready=11 original_serial_api_claimed=11 retried=11");
}

#[test]
fn independent_review_disabled_network_defaults(){let dir=tempfile::tempdir_in("/tmp").unwrap();let db=Arc::new(IndexDb::open(&dir.path().join("index.db")).unwrap().0);let config=ProjectConfig::default();assert!(!config.semantic.enabled);assert!(!config.semantic.network_opt_in);let assembled=semantic_wiring::assemble_with("independent-disabled",&config,db,|_|panic!("disabled config queried environment"),false).unwrap();assert!(assembled.is_none());println!("INDEPENDENT_DEFAULT enabled=false network_opt_in=false env_lookups=0 provider_assembly=none");}
