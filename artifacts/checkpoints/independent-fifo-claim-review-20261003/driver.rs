use cc_db::{index_db::IndexDb,index_migrate::SchemaStatus,semantic_outbox::*,semantic_publish::LifecycleFence};
use rusqlite::{Connection,params};
use std::{path::Path,sync::{Arc,Barrier}};
fn insert(c:&Connection,id:i64,state:&str,sp:&str,at:f64,doc:&str,updated:&str){c.execute("INSERT INTO semantic_outbox(task_id,doc_key,doc_version,input_digest,space_id,op,state,available_at,created_at,updated_at) VALUES(?1,?2,'version','digest',?3,'embed',?4,?5,'created',?6)",params![id,doc,sp,state,at,updated]).unwrap();}
fn seed(p:&Path){
 let (db,s)=IndexDb::open(p).unwrap();assert_eq!(s,SchemaStatus::Initialized);drop(db);
 let c=Connection::open(p).unwrap();c.execute_batch("PRAGMA foreign_keys=ON;
 INSERT INTO files(file_path,language,content_hash,mtime,size,indexed_at,chunk_policy,document_spec) VALUES('unchanged.rs','rust','unchanged-content-hash',17,42,'old','policy','spec');
 INSERT INTO chunks(chunk_id,file_path,language,chunk_index,start_line,end_line,text) VALUES('chunk','unchanged.rs','rust',0,1,2,'fn unchanged() {}');
 INSERT INTO chunks_fts(rowid,chunk_id,file_path,text) SELECT rowid,chunk_id,file_path,text FROM chunks;
 INSERT INTO document_manifest VALUES('document','version','unchanged.rs','chunk','encoding','{}','{}');
 INSERT INTO semantic_manifest VALUES('document','version','unchanged.rs','encoding','digest','active','artifact','old',(SELECT value FROM metadata WHERE key='index_incarnation'));
 INSERT INTO semantic_spaces VALUES('active','{}','active','old');
 INSERT OR REPLACE INTO metadata VALUES('index_epoch','17'),('evidence_epoch','23'),('semantic_epoch','31');").unwrap();
 insert(&c,1,"pending","active",0.,"oldest","old");insert(&c,2,"pending","active",999999999999.,"future","old");insert(&c,3,"done","other",0.,"done","old");
 assert_eq!(c.query_row("PRAGMA integrity_check",[],|r|r.get::<_,String>(0)).unwrap(),"ok");
 println!("old production library generated v24 fixture, SQLite {}",rusqlite::version());
}
fn open_only(p:&Path){for _ in 0..2{let (db,s)=IndexDb::open(p).unwrap();assert_eq!(s,SchemaStatus::UpToDate);println!("generation {:?}",db.reads().read_generation().unwrap());drop(db);}println!("open/reopen passed");}
fn finite(){
 let c=Connection::open_in_memory().unwrap();cc_db::index_migrate::migrate_index_db(&c).unwrap();
 let choices:Vec<_>=["pending","claimed","done","failed","superseded"].iter().flat_map(|s|["active","other"].into_iter().flat_map(move|sp|[99.,100.,101.].into_iter().map(move|at|(*s,sp,at)))).collect();
 let mut cases=0;
 for a in &choices{for b in &choices{for d in &choices{
  c.execute("DELETE FROM semantic_outbox",[]).unwrap();let rows=[a,b,d];
  for (i,r) in rows.iter().enumerate(){insert(&c,(i+1)as i64,r.0,r.1,r.2,&format!("doc{i}"),"old");}
  let expected=rows.iter().position(|r|r.0=="pending"&&r.1=="active"&&r.2<=100.).map(|i|(i+1)as i64);
  let t=claim_next_on(&c,"active","independent",100.,7.).unwrap();assert_eq!(t.as_ref().map(|t|t.task_id),expected);
  if let Some(t)=t{assert_eq!(t.lease_expires_at,107.);assert_eq!(t.token.len(),32);assert!(t.token.bytes().all(|b|b.is_ascii_hexdigit()&&!b.is_ascii_uppercase()));assert_eq!(t.doc_version,"version");assert_eq!(t.input_digest,"digest");assert_eq!(t.op,OutboxOp::Embed);
   assert_eq!(c.query_row("SELECT attempt_count,claim_owner FROM semantic_outbox WHERE task_id=?1",[t.task_id],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?))).unwrap(),(1,"independent".into()));}
  for (i,r) in rows.iter().enumerate(){let got:(String,i64)=c.query_row("SELECT state,attempt_count FROM semantic_outbox WHERE task_id=?1",[(i+1)as i64],|r|Ok((r.get(0)?,r.get(1)?))).unwrap();if Some((i+1)as i64)!=expected{assert_eq!(got,(r.0.into(),0));}}
  cases+=1;
 }}}
 println!("independent finite oracle: {cases} three-row cases passed");
 c.execute("DELETE FROM semantic_outbox",[]).unwrap();insert(&c,1,"pending","active",0.,"retry","old");insert(&c,2,"pending","active",0.,"next","old");
 let one=claim_next_on(&c,"active","w",100.,10.).unwrap().unwrap();assert!(retry_on(&c,1,&one.token,"backoff",100.,5.,4).unwrap());
 let next=claim_next_on(&c,"active","w",104.,10.).unwrap().unwrap();assert_eq!(next.task_id,2);assert!(ack_done_on(&c,2,&next.token,104.).unwrap());assert!(claim_next_on(&c,"active","w",104.,10.).unwrap().is_none());
 let again=claim_next_on(&c,"active","w",105.,10.).unwrap().unwrap();assert_eq!(again.task_id,1);assert_ne!(again.token,one.token);
 assert!(!renew_lease_on(&c,1,&one.token,105.,10.).unwrap());assert!(!ack_done_on(&c,1,&one.token,105.).unwrap());assert!(!retry_on(&c,1,&one.token,"stale",105.,0.,4).unwrap());
 assert_eq!(c.query_row("SELECT attempt_count FROM semantic_outbox WHERE task_id=1",[],|r|r.get::<_,i64>(0)).unwrap(),2);
 assert_eq!(reclaim_expired_on(&c,116.).unwrap(),1);let third=claim_next_on(&c,"active","w",116.,10.).unwrap().unwrap();assert_ne!(third.token,again.token);assert_eq!(third.task_id,1);
 assert!(!ack_done_on(&c,1,&again.token,116.).unwrap());assert!(ack_done_on(&c,1,&third.token,116.).unwrap());println!("retry/backoff/reclaim/stale token trace passed");
 // MAX includes terminal rows of same doc, but excludes rows of other spaces.
 c.execute("DELETE FROM semantic_outbox",[]).unwrap();insert(&c,1,"pending","active",0.,"hot","10");insert(&c,2,"pending","active",0.,"cool","20");insert(&c,3,"done","active",0.,"hot","90");insert(&c,4,"done","other",0.,"cool","99");
 let rr=claim_next_fair_on(&c,"active","w",100.,10.,ClaimFairness::DocRoundRobin).unwrap().unwrap();assert_eq!(rr.task_id,2);println!("production DocRoundRobin terminal MAX/space isolation passed");
}
fn facade(p:&Path){
 let (db,_)=IndexDb::open(p).unwrap();let c=Connection::open(p).unwrap();insert(&c,1,"pending","other",0.,"other","old");insert(&c,2,"pending","active",0.,"only","old");
 assert!(db.claim_semantic("unconfigured",10.).unwrap().is_none());c.execute("INSERT INTO semantic_spaces VALUES('active','{}','active','old')",[]).unwrap();
 let generation=db.reads().read_generation().unwrap();let fence=LifecycleFence::default();fence.close();
 for _ in 0..3{assert!(db.claim_semantic_with_lifecycle("cancelled",10.,ClaimFairness::Fifo,Some(&fence)).unwrap().is_none());}
 assert_eq!(c.query_row("SELECT sum(attempt_count) FROM semantic_outbox",[],|r|r.get::<_,i64>(0)).unwrap(),0);
 drop(db);drop(c);let a=IndexDb::open(p).unwrap().0;let b=IndexDb::open(p).unwrap().0;let barrier=Arc::new(Barrier::new(2));
 let workers:Vec<_>=[a,b].into_iter().map(|db|{let barrier=barrier.clone();std::thread::spawn(move||{barrier.wait();db.claim_semantic("race",60.).unwrap()})}).collect();
 let results:Vec<_>=workers.into_iter().map(|w|w.join().unwrap()).collect();assert_eq!(results.iter().filter(|r|r.is_some()).count(),1);let t=results.into_iter().flatten().next().unwrap();assert_eq!(t.task_id,2);assert_eq!(t.token.len(),32);
 let db=IndexDb::open(p).unwrap().0;assert_eq!(db.reads().read_generation().unwrap(),generation);let c=Connection::open(p).unwrap();assert_eq!(c.query_row("SELECT sum(attempt_count) FROM semantic_outbox",[],|r|r.get::<_,i64>(0)).unwrap(),1);assert_eq!(c.query_row("SELECT state FROM semantic_outbox WHERE task_id=1",[],|r|r.get::<_,String>(0)).unwrap(),"pending");println!("two actual IndexDb connections IMMEDIATE race/active space/closed lifecycle/epochs passed");
}
fn paths(p:&Path){let(db,s)=IndexDb::open(p).unwrap();println!("path status {s:?}");let c=Connection::open(p).unwrap();let count:i64=c.query_row("SELECT count(*) FROM sqlite_schema WHERE name IN ('semantic_outbox_ready','semantic_outbox_fifo_pending')",[],|r|r.get(0)).unwrap();assert_eq!(count,2);drop(c);assert!(db.claim_semantic("empty",10.).unwrap().is_none());
 let c=Connection::open(p).unwrap();assert!(claim_next_on(&c,"active","fresh-or-reset",100.,10.).unwrap().is_none());drop(c);
 db.admin().rebuild_with_temp_db(|_|Ok(())).unwrap();let c=Connection::open(p).unwrap();assert_eq!(c.query_row("SELECT count(*) FROM sqlite_schema WHERE name IN ('semantic_outbox_ready','semantic_outbox_fifo_pending')",[],|r|r.get::<_,i64>(0)).unwrap(),2);insert(&c,1,"pending","active",0.,"rebuilt","old");assert_eq!(claim_next_on(&c,"active","rebuilt",100.,10.).unwrap().unwrap().task_id,1);drop(c);
 println!("fresh/mismatch and normal temp rebuild index availability passed");}
fn cancel(p:&Path){use cc_semantic::queue::{drain_pending_with_lifecycle,WorkerLimits,TaskExit};
 let db=IndexDb::open(p).unwrap().0;let c=Connection::open(p).unwrap();c.execute("INSERT INTO semantic_spaces VALUES('active','{}','active','old')",[]).unwrap();for i in 1..=3{insert(&c,i,"pending","active",0.,&format!("cancel{i}"),"old");}
 let generation=db.reads().read_generation().unwrap();let fence=LifecycleFence::default();let limits=WorkerLimits::validated(3,60.,0.,3).unwrap();let mut calls=0;
 let r=drain_pending_with_lifecycle(&db,"cancel",&limits,Some(&fence),&mut |_|{calls+=1;fence.close();Ok(TaskExit::Cancelled{started:false})}).unwrap();assert_eq!(r.claimed,1);assert_eq!(calls,1);
 let r=drain_pending_with_lifecycle(&db,"closed",&limits,Some(&fence),&mut |_|panic!("closed handler")).unwrap();assert_eq!(r.claimed,0);
 let mut s=c.prepare("SELECT task_id,state,attempt_count,lease_token FROM semantic_outbox ORDER BY task_id").unwrap();let rows:Vec<_>=s.query_map([],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?,r.get::<_,i64>(2)?,r.get::<_,Option<String>>(3)?))).unwrap().map(Result::unwrap).collect();assert_eq!(rows,vec![(1,"pending".into(),0,None),(2,"pending".into(),0,None),(3,"pending".into(),0,None)]);assert_eq!(db.reads().read_generation().unwrap(),generation);
 println!("production drain cancellation during first unstarted task: handback and no second claim; closed drain no claim; epochs unchanged");}
fn main(){let args:Vec<_>=std::env::args().collect();let p=Path::new(args.get(2).map(String::as_str).unwrap_or("unused"));println!("SQLite {}",rusqlite::version());match args[1].as_str(){"seed"=>seed(p),"open"=>open_only(p),"finite"=>finite(),"facade"=>facade(p),"paths"=>paths(p),"cancel"=>cancel(p),"reset"=>{let(db,s)=IndexDb::open(p).unwrap();assert_eq!(s,SchemaStatus::Initialized);let c=Connection::open(p).unwrap();assert_eq!(c.query_row("SELECT count(*) FROM files",[],|r|r.get::<_,i64>(0)).unwrap(),0);assert!(claim_next_on(&c,"active","reset",100.,10.).unwrap().is_none());println!("normal mismatch reset {:?}",db.reads().read_generation().unwrap());},"direct"=>{let db=IndexDb::open(p).unwrap().0;let r=db.admin().rebuild_with_direct_writer(|_|Ok(()));println!("direct result {r:?}");if r.is_err(){std::process::exit(2);}},_=>panic!("mode")}}
