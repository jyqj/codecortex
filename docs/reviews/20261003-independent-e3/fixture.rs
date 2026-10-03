//! Fresh synthetic review fixture; only compiled in an isolated source copy.
use std::{sync::{Arc, Mutex, Condvar, atomic::{AtomicBool, Ordering}}, time::{Duration, Instant}, io::{Read, Write}, net::{TcpListener,TcpStream}};
use crate::{engine::CodeIndex, semantic_wiring::{self, SemanticSubsystem}, semantic_runtime::{self,SemanticRuntime}, service_factory::QueryServices};
use cc_model::config::ProjectConfig;
use cc_semantic::{ports::{DocumentInput,EmbeddingProvider,ProviderError,QueryInput}, cache::CacheRead, publish::{Publisher,PublishVerdict}, queue::{self,WorkerLimits}};
use cc_db::index_db::IndexDb;

struct Project { _dir: tempfile::TempDir, db: Arc<IndexDb>, sub: Arc<SemanticSubsystem>, services: Arc<QueryServices>, config: ProjectConfig }
impl Project {
 fn new(width:u32, endpoint:&str, count:usize)->Self {
  let dir=tempfile::tempdir().unwrap();
  for i in 0..count { std::fs::write(dir.path().join(format!("unit_{i}.rs")),format!("pub fn independently_reviewed_{i}() -> usize {{ {i} }}\n")).unwrap(); }
  let mut index=CodeIndex::new(Some(dir.path())).unwrap(); index.build_index(true).unwrap();
  let db=index.index_db().unwrap().clone();
  let mut config=ProjectConfig::default(); let c=&mut config.semantic;
  c.enabled=true; c.model_id="independent-e3-loopback".into(); c.dimensions=Some(2); c.max_input_tokens=Some(8192); c.max_batch_items=Some(1); c.endpoint=endpoint.into(); c.network_opt_in=true; c.allow_http=true; c.api_key_ref=Some("env:CC_INDEPENDENT_E3_SYNTHETIC_KEY".into());
  c.max_concurrent=if width>=2 {4} else {0}; c.max_concurrent_per_project=width; c.acquire_timeout_ms=10000;
  let root=dir.path().join("real-cache");
  let sub=Arc::new(semantic_wiring::assemble_with(&dir.path().to_string_lossy(),&config,db.clone(),|name| if name==cc_semantic::cache::CACHE_ROOT_ENV {Some(root.to_string_lossy().into())} else {None},false).unwrap().unwrap());
  let id=sub.space.digest().unwrap(); db.register_semantic_space(id.as_str(), &serde_json::to_string(&sub.space).unwrap()).unwrap();
  let desired=cc_db::document_store::semantic_worker_desired_page(&db,"",256).unwrap().desired;
  assert_eq!(desired.len(),count,"one real indexed input per source file");
  db.enqueue_semantic_backfill_plan(id.as_str(),&desired).unwrap(); db.switch_semantic_active_space(id.as_str(),"independent-review").unwrap();
  Self{_dir:dir,db,sub,services:Arc::new(QueryServices::default()),config}
 }
 fn scalar(&self,sql:&str)->usize {self.db.read_conn().unwrap().query_row(sql,[],|r|r.get::<_,i64>(0)).unwrap() as usize}
 fn runtime(&self)->Arc<SemanticRuntime>{semantic_runtime::from_config(self.db.clone(),self.sub.clone(),self.services.clone(),&self.config.semantic).unwrap().unwrap()}
 fn verify(&self,n:usize) {
  let conn=self.db.read_conn().unwrap(); let mut q=conn.prepare("SELECT s.input_digest,s.artifact_ref,json_extract(d.record_json,'$.input.text') FROM semantic_manifest s JOIN document_manifest d USING(doc_key)").unwrap();
  let rows=q.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?))).unwrap(); let mut found=0;
  for row in rows { let (input,reference,text)=row.unwrap(); let digest=cc_semantic::types::InputDigest::of_input(text.as_bytes()).unwrap(); assert_eq!(digest.as_str(),input); match self.sub.cache.get(&self.sub.space,&digest,&self.sub.doc_spec).unwrap(){CacheRead::Hit(v)=>{assert_eq!(v.artifact_ref.as_str(),reference);assert_eq!(v.data,vec![1.0,0.0]);},other=>panic!("real cache not hit: {other:?}")};found+=1; }
  assert_eq!(found,n); assert_eq!(self.scalar("SELECT count(*) FROM semantic_outbox WHERE state='done'"),n);
 }
}
struct Loopback { endpoint:String, arrivals:std::sync::mpsc::Receiver<()>, release:Arc<(Mutex<bool>,Condvar)>, stop:Arc<AtomicBool>, join:Option<std::thread::JoinHandle<()>> }
impl Loopback {
 fn new()->Self {
  let listener=TcpListener::bind("127.0.0.1:0").unwrap(); let endpoint=format!("http://{}",listener.local_addr().unwrap()); listener.set_nonblocking(true).unwrap();
  let (tx,arrivals)=std::sync::mpsc::channel(); let release=Arc::new((Mutex::new(false),Condvar::new())); let stop=Arc::new(AtomicBool::new(false)); let r=release.clone();let s=stop.clone();
  let join=std::thread::spawn(move|| {let mut handles=vec![];while !s.load(Ordering::Acquire){match listener.accept(){Ok((stream,_))=>{let r=r.clone();let tx=tx.clone();handles.push(std::thread::spawn(move||serve(stream,r,tx)));},Err(e) if e.kind()==std::io::ErrorKind::WouldBlock=>std::thread::sleep(Duration::from_millis(2)),Err(e)=>panic!("{e}")}} for h in handles{h.join().unwrap();}});
  Self{endpoint,arrivals,release,stop,join:Some(join)}
 }
 fn receive(&self,n:usize){for _ in 0..n {self.arrivals.recv_timeout(Duration::from_secs(10)).expect("held real HTTP request");}}
 fn no_extra(&self){assert!(matches!(self.arrivals.recv_timeout(Duration::from_millis(300)),Err(std::sync::mpsc::RecvTimeoutError::Timeout)),"extra HTTP request arrived before any held response was released");}
 fn release(&self){*self.release.0.lock().unwrap()=true;self.release.1.notify_all();}
}
impl Drop for Loopback{fn drop(&mut self){self.release();self.stop.store(true,Ordering::Release);if let Some(j)=self.join.take(){j.join().unwrap();}}}
fn serve(mut stream:TcpStream,release:Arc<(Mutex<bool>,Condvar)>,tx:std::sync::mpsc::Sender<()>) {
 stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();let mut bytes=Vec::new();let mut buf=[0;4096];let body_start;
 loop {let n=stream.read(&mut buf).unwrap();assert!(n>0);bytes.extend_from_slice(&buf[..n]);if let Some(i)=bytes.windows(4).position(|x|x==b"\r\n\r\n"){body_start=i+4;break;}}
 let headers=String::from_utf8_lossy(&bytes[..body_start]);let len=headers.lines().find_map(|l|l.to_lowercase().strip_prefix("content-length:").map(|v|v.trim().parse::<usize>().unwrap())).unwrap();
 while bytes.len()<body_start+len {let n=stream.read(&mut buf).unwrap();assert!(n>0);bytes.extend_from_slice(&buf[..n]);}
 let request:serde_json::Value=serde_json::from_slice(&bytes[body_start..body_start+len]).unwrap();assert_eq!(request["input"].as_array().unwrap().len(),1);
 tx.send(()).unwrap();let mut open=release.0.lock().unwrap();while !*open{open=release.1.wait(open).unwrap();}drop(open);
 let body=r#"{"model":"independent-e3-loopback","data":[{"index":0,"embedding":[1.0,0.0]}],"usage":{"prompt_tokens":1,"total_tokens":1}}"#;
 let response=format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body);let _=stream.write_all(response.as_bytes());
}
fn until(mut pred:impl FnMut()->bool){let start=Instant::now();while !pred(){assert!(start.elapsed()<Duration::from_secs(20),"bounded wait expired");std::thread::sleep(Duration::from_millis(5));}}

#[tokio::test(flavor="multi_thread",worker_threads=2)]
async fn independent_e3_real_runtime_http_cache_close_and_projects(){
 std::env::set_var("CC_INDEPENDENT_E3_SYNTHETIC_KEY","synthetic-local-only");
 let server=Loopback::new();let p=Project::new(2,&server.endpoint,24);let runtime=p.runtime();assert!(runtime.schedule());server.receive(2);
 until(||p.scalar("SELECT count(*) FROM semantic_outbox WHERE state='claimed'")==4);server.no_extra();assert_eq!(p.services.query_pins(),1);
 server.release();until(||p.services.query_pins()==0);p.verify(24);
 let requests=2+server.arrivals.try_iter().count();assert_eq!(requests,24);
 // Re-enqueue against a real durable cache; a new runtime must publish without another HTTP request.
 let seed=rusqlite::Connection::open(p.db.admin().db_path()).unwrap();seed.execute("DELETE FROM semantic_manifest",[]).unwrap();seed.execute("DELETE FROM semantic_outbox",[]).unwrap();drop(seed);
 let reopened=p.runtime();assert!(reopened.schedule());until(||p.services.query_pins()==0);p.verify(24);server.no_extra();
 println!("runtime width=4; held HTTP <=2; claim round budget=16; published/cache verified=24; reopen HTTP=0");
 for width in [0,1] {let s=Loopback::new();let p=Project::new(width,&s.endpoint,5);let r=p.runtime();assert!(r.schedule());s.receive(1);s.no_extra();assert_eq!(p.scalar("SELECT count(*) FROM semantic_outbox WHERE state='claimed'"),1);s.release();until(||p.services.query_pins()==0);p.verify(5);println!("default local width {width}: local=1 held HTTP=1 verified=5");}
 // Two distinct namespaces share the actual production 4/2 gate and two runtime capacity pins.
 let s=Loopback::new();let a=Project::new(2,&s.endpoint,12);let b=Project::new(2,&s.endpoint,12);assert_ne!(a.sub.namespace,b.sub.namespace);let ar=a.runtime();let br=b.runtime();assert!(ar.schedule());assert!(br.schedule());s.receive(4);
 until(||a.scalar("SELECT count(*) FROM semantic_outbox WHERE state='claimed'")==4 && b.scalar("SELECT count(*) FROM semantic_outbox WHERE state='claimed'")==4);s.no_extra();assert_eq!(a.services.query_pins()+b.services.query_pins(),2);
 println!("two projects held HTTP=4, local claims=8, runtime pins=2; each input owned per worker; no prefetch");s.release();until(||a.services.query_pins()+b.services.query_pins()==0);a.verify(12);b.verify(12);
 let s=Loopback::new();let p=Project::new(2,&s.endpoint,8);let r=p.runtime();assert!(r.schedule());s.receive(2);until(||p.scalar("SELECT count(*) FROM semantic_outbox WHERE state='claimed'")==4);r.close();assert_eq!(p.services.query_pins(),1);std::thread::sleep(Duration::from_millis(100));assert_eq!(p.services.query_pins(),1);assert_eq!(p.scalar("SELECT count(*) FROM semantic_manifest"),0);s.release();until(||p.services.query_pins()==0);assert_eq!(p.scalar("SELECT count(*) FROM semantic_manifest"),0);assert!(!r.schedule());println!("close/cancel pin retained until physical I/O/worker join; late publication=0");
}

struct LocalHold {space:cc_semantic::types::VectorSpace, state:Arc<(Mutex<(usize,usize,usize,bool)>,Condvar)>, fail_first:AtomicBool}
impl EmbeddingProvider for LocalHold {
 fn space(&self)->&cc_semantic::types::VectorSpace{&self.space}
 fn embed_documents(&self,batch:&[DocumentInput])->Result<Vec<Vec<f32>>,ProviderError>{
  assert_eq!(batch.len(),1);batch[0].verify().unwrap();
  let mut state=self.state.0.lock().unwrap();state.0+=1;state.1=state.1.max(state.0);state.2+=batch[0].bytes.len();self.state.1.notify_all();while !state.3 {state=self.state.1.wait(state).unwrap();}state.0-=1;drop(state);
  if self.fail_first.swap(false,Ordering::AcqRel){return Err(ProviderError::Timeout);}
  Ok(vec![vec![1.0,0.0]])
 }
 fn embed_queries(&self,_:&[QueryInput])->Result<Vec<Vec<f32>>,ProviderError>{unreachable!()}
}
#[test]
fn independent_e3_queue_width_claim16_retry_and_real_publication(){
 for (width,budget,expected) in [(2,16,4),(2,2,2),(0,16,1),(1,16,1)] {
  let p=Project::new(2,"http://127.0.0.1:1",24);let state=Arc::new((Mutex::new((0,0,0,false)),Condvar::new()));
  let provider=LocalHold{space:p.sub.space.clone(),state:state.clone(),fail_first:AtomicBool::new(true)};
  std::thread::scope(|scope|{
   let job=scope.spawn(||{
    let limits=WorkerLimits::validated(budget,60.0,30.0,3).unwrap();let incarnation=p.db.reads().read_generation().unwrap().incarnation;
    queue::drain_pending_parallel_with_lifecycle(&p.db,"independent-owned",&limits,None,width,&|guard|{
     let resolve=|task:&cc_db::semantic_outbox::ClaimedTask| cc_db::document_store::semantic_worker_input(&p.db,&task.doc_key,&task.doc_version,&task.input_digest).unwrap().map(|s|DocumentInput::from_bytes(s.as_bytes())).transpose();
     let publisher=Publisher::new(&p.db,&p.sub.cache,&p.sub.space,&p.sub.doc_spec,incarnation)?;
     queue::EmbedHandler::new(publisher,&provider,&resolve).handle(guard)
    }).unwrap()
   });
   until(||state.0.lock().unwrap().0==expected);std::thread::sleep(Duration::from_millis(100));assert_eq!(state.0.lock().unwrap().1,expected);assert_eq!(p.scalar("SELECT count(*) FROM semantic_outbox WHERE state='claimed'"),expected);
   {state.0.lock().unwrap().3=true;state.1.notify_all();}
   let report=job.join().unwrap();assert_eq!(report.claimed,budget);assert_eq!(report.retried,1);assert_eq!(report.completed,budget-1);assert_eq!(p.scalar("SELECT count(*) FROM semantic_outbox WHERE state='pending'"),24-budget+1);p.verify(budget-1);
   println!("width config={width} budget={budget} local_peak={expected} claimed={} retry={} successful={} input_bytes_processed={}",report.claimed,report.retried,report.completed,state.0.lock().unwrap().2);
  });
 }
}
#[test]
fn independent_e3_real_cache_fences_token_state_contract(){
 let now=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
 // Expiry alone does not revoke token authority; actual reclaim does.
 let p=Project::new(2,"http://127.0.0.1:1",3);let incarnation=p.db.reads().read_generation().unwrap().incarnation;
 let publish=Publisher::new(&p.db,&p.sub.cache,&p.sub.space,&p.sub.doc_spec,incarnation).unwrap();
 let expired=p.db.claim_semantic("old-but-unreclaimed",60.0).unwrap().unwrap();
 let sql=rusqlite::Connection::open(p.db.admin().db_path()).unwrap();sql.execute("UPDATE semantic_outbox SET lease_expires_at=0 WHERE task_id=?",[expired.task_id]).unwrap();drop(sql);
 assert!(matches!(publish.publish_embedding(&expired,&[1.0,0.0],now).unwrap(),PublishVerdict::Published{..}));
 let old=p.db.claim_semantic("old-reclaimed",60.0).unwrap().unwrap();let sql=rusqlite::Connection::open(p.db.admin().db_path()).unwrap();sql.execute("UPDATE semantic_outbox SET lease_expires_at=0 WHERE task_id=?",[old.task_id]).unwrap();drop(sql);
 assert_eq!(p.db.reclaim_expired_semantic().unwrap(),1);let fresh=p.db.claim_semantic("new-token",60.0).unwrap().unwrap();assert_eq!(old.task_id,fresh.task_id);assert_ne!(old.token,fresh.token);
 assert!(matches!(publish.publish_embedding(&old,&[1.0,0.0],now).unwrap(),PublishVerdict::Rejected(cc_db::semantic_publish::PublishRejection::LeaseLost)));
 assert!(matches!(publish.publish_embedding(&fresh,&[1.0,0.0],now).unwrap(),PublishVerdict::Published{..}));p.verify(2);
 println!("real cache/Pub: expired unreclaimed token accepted; reclaimed old token refused; fresh token accepted");
 let source=Project::new(2,"http://127.0.0.1:1",1);let task=source.db.claim_semantic("source-fence",60.0).unwrap().unwrap();let sql=rusqlite::Connection::open(source.db.admin().db_path()).unwrap();sql.execute("UPDATE document_manifest SET doc_version=? WHERE doc_key=?",rusqlite::params!["changed-after-claim",&task.doc_key]).unwrap();drop(sql);
 let pub_source=Publisher::new(&source.db,&source.sub.cache,&source.sub.space,&source.sub.doc_spec,source.db.reads().read_generation().unwrap().incarnation).unwrap();assert!(matches!(pub_source.publish_embedding(&task,&[1.0,0.0],now).unwrap(),PublishVerdict::Rejected(_)));assert_eq!(source.scalar("SELECT count(*) FROM semantic_manifest"),0);
 let space=Project::new(2,"http://127.0.0.1:1",1);let task=space.db.claim_semantic("space-fence",60.0).unwrap().unwrap();let other=cc_semantic::types::VectorSpace::new("other-independent-space",2).unwrap();let digest=other.digest().unwrap();space.db.register_semantic_space(digest.as_str(),&serde_json::to_string(&other).unwrap()).unwrap();space.db.switch_semantic_active_space(digest.as_str(),"independent-space-change").unwrap();
 let pub_space=Publisher::new(&space.db,&space.sub.cache,&space.sub.space,&space.sub.doc_spec,space.db.reads().read_generation().unwrap().incarnation).unwrap();assert!(matches!(pub_space.publish_embedding(&task,&[1.0,0.0],now).unwrap(),PublishVerdict::Rejected(_)));assert_eq!(space.scalar("SELECT count(*) FROM semantic_manifest"),0);
 println!("real artifact-before-manifest source version and active space changes refused");
}

struct TimedSynthetic {space:cc_semantic::types::VectorSpace,active:std::sync::atomic::AtomicUsize,peak:std::sync::atomic::AtomicUsize}
impl EmbeddingProvider for TimedSynthetic {
 fn space(&self)->&cc_semantic::types::VectorSpace{&self.space}
 fn embed_documents(&self,batch:&[DocumentInput])->Result<Vec<Vec<f32>>,ProviderError>{let n=self.active.fetch_add(1,Ordering::AcqRel)+1;self.peak.fetch_max(n,Ordering::AcqRel);std::thread::sleep(Duration::from_millis(20));self.active.fetch_sub(1,Ordering::AcqRel);Ok(batch.iter().map(|_|vec![1.0,0.0]).collect())}
 fn embed_queries(&self,_:&[QueryInput])->Result<Vec<Vec<f32>>,ProviderError>{unreachable!()}
}
#[test]
fn independent_e3_synthetic_perf_only(){
 for rep in 0..3 {
  let p=Project::new(2,"http://127.0.0.1:1",64);let provider=TimedSynthetic{space:p.sub.space.clone(),active:0.into(),peak:0.into()};let start=Instant::now();
  let outcome=semantic_wiring::drain_worker_batch_parallel(&p.db,&p.sub,&p.services,&provider,&|task|cc_db::document_store::semantic_worker_input(&p.db,&task.doc_key,&task.doc_version,&task.input_digest)?.map(|s|DocumentInput::from_bytes(s.as_bytes())).transpose(),semantic_wiring::WorkerDrainOptions{owner:"independent-perf",max_batch:64,now_unix:std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64,lifecycle:None}).unwrap();
  let elapsed=start.elapsed();assert_eq!(outcome.batch.claimed,64);assert_eq!(outcome.batch.completed,64);p.verify(64);
  println!("synthetic rep={rep} delay_ms=20 documents=64 elapsed_us={} local_peak={} real_cache_verified=64",elapsed.as_micros(),provider.peak.load(Ordering::Acquire));
 }
}
