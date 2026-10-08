use rusqlite::{Connection,params,StatementStatus};
use serde_json::json;
use std::time::Instant;
fn main(){
 let out=std::env::args().nth(1).unwrap();let root=std::path::Path::new(&out);
 if std::env::args().nth(2).as_deref()==Some("guard") { guard_probe(root);return; }
 let src=include_str!("../src/sql/index_v1.sql");let a=src.find("CREATE TABLE IF NOT EXISTS semantic_outbox (").unwrap();let b=src[a..].find("-- active space").unwrap()+a;let schema=&src[a..b];
 let claim="UPDATE semantic_outbox SET state='claimed',lease_token=lower(hex(randomblob(16))),lease_expires_at=?1+?2,claim_owner=?3,attempt_count=attempt_count+1,updated_at=?4 WHERE task_id=(SELECT task_id FROM semantic_outbox WHERE space_id=?5 AND state='pending' AND available_at<=?1 ORDER BY task_id LIMIT 1) RETURNING task_id,lease_token,doc_key,doc_version,input_digest,op,lease_expires_at";
 let mut results=Vec::new();
 for n in [1000,5000,10000]{for scenario in ["all_pending","mixed","future_prefix","no_ready"]{for candidate in [false,true]{
  let path=root.join(format!("rust-sql-{n}-{scenario}-{candidate}.sqlite3"));let c=Connection::open(path).unwrap();c.execute_batch(schema).unwrap();c.execute_batch("BEGIN").unwrap();
  for i in 0..n {let mut state="pending";let mut space="active";let mut avail=900.;let mut expiry:Option<f64>=None;
   if scenario=="mixed" {match i%10 {0|1=>avail=2000.,2|3=>space="other",4=>{state="claimed";expiry=Some(2000.)},5=>state="done",_=>{}}}
   if scenario=="future_prefix" && i<n/2 || scenario=="no_ready" {avail=2000.;}
   c.execute("INSERT INTO semantic_outbox(doc_key,doc_version,input_digest,space_id,op,state,available_at,lease_expires_at,created_at,updated_at) VALUES(?1,'v','digest',?2,'embed',?3,?4,?5,'created','updated')",params![format!("doc{i}"),space,state,avail,expiry]).unwrap();
  }c.execute_batch("COMMIT").unwrap();
  if candidate {c.execute_batch("CREATE INDEX diagnostic_fifo_pending ON semantic_outbox(space_id,task_id,available_at) WHERE state='pending'").unwrap();}
  let mut p=c.prepare(&format!("EXPLAIN QUERY PLAN {claim}")).unwrap();let plan:Vec<String>=p.query_map(params![1000.,60.,"synthetic","1970-01-01T00:16:40Z","active"],|r|r.get(3)).unwrap().map(Result::unwrap).collect();drop(p);
  let expected:Option<i64>=c.query_row("SELECT min(task_id) FROM semantic_outbox WHERE state='pending' AND space_id='active' AND available_at<=1000",[],|r|r.get(0)).unwrap();
  let mut timings=Vec::new();let mut selected=None;
  for _ in 0..10 {c.execute_batch("BEGIN").unwrap();let t=Instant::now();let task=cc_db::semantic_outbox::claim_next_fair_on(&c,"active","synthetic",1000.,60.,cc_db::semantic_outbox::ClaimFairness::Fifo).unwrap();timings.push(t.elapsed().as_secs_f64()*1000.);selected=task.map(|x|x.task_id);assert_eq!(selected,expected);c.execute_batch("ROLLBACK").unwrap();}
  c.execute_batch("BEGIN").unwrap();let mut s=c.prepare(claim).unwrap();{let mut rows=s.query(params![1000.,60.,"synthetic","1970-01-01T00:16:40Z","active"]).unwrap();while rows.next().unwrap().is_some(){}}
  let steps=s.get_status(StatementStatus::VmStep);let sorts=s.get_status(StatementStatus::Sort);drop(s);c.execute_batch("ROLLBACK").unwrap();
  let dist:Vec<(String,i64)>=c.prepare("SELECT state,count(*) FROM semantic_outbox GROUP BY state").unwrap().query_map([],|r|Ok((r.get(0)?,r.get(1)?))).unwrap().map(Result::unwrap).collect();
  let count=|predicate:&str| c.query_row(&format!("SELECT count(*) FROM semantic_outbox WHERE {predicate}"),[],|r|r.get::<_,i64>(0)).unwrap();
  results.push(json!({"n":n,"scenario":scenario,"candidate":candidate,"plan":plan,"vm_steps":steps,"sort_count":sorts,"selected":selected,"actual_production_function_ms":timings,"states":dist,"ready_active":count("state='pending' AND space_id='active' AND available_at<=1000"),"future_active":count("state='pending' AND space_id='active' AND available_at>1000"),"pending_other":count("state='pending' AND space_id<>'active'")}));
 }}}
 std::fs::write(root.join("rust-sql-probe.json"),serde_json::to_string_pretty(&json!({"sqlite_version":rusqlite::version(),"results":results,"note":"Production-linked rusqlite; real claim_next_fair_on; rollback timings exclude commit, not pipeline. VmStep/Sort separate identical full CAS. No schema migration."})).unwrap()).unwrap();
}

fn guard_probe(root:&std::path::Path){
 let sql="UPDATE semantic_outbox SET state='claimed',lease_token=lower(hex(randomblob(16))),lease_expires_at=?1+?2,claim_owner=?3,attempt_count=attempt_count+1,updated_at=?4 WHERE task_id=CASE WHEN EXISTS(SELECT 1 FROM semantic_outbox INDEXED BY semantic_outbox_ready WHERE state='pending' AND available_at<=?1 AND space_id=?5) THEN (SELECT task_id FROM semantic_outbox WHERE space_id=?5 AND state='pending' AND available_at<=?1 ORDER BY task_id LIMIT 1) ELSE NULL END RETURNING task_id,lease_token,doc_key,doc_version,input_digest,op,lease_expires_at";
 let mut results=Vec::new();
 for n in [1000,5000,10000] { for scenario in ["all_pending","mixed","future_prefix","no_ready"] {
 let c=Connection::open(root.join(format!("rust-sql-{n}-{scenario}-true.sqlite3"))).unwrap();
 let expected:Option<i64>=c.query_row("SELECT min(task_id) FROM semantic_outbox WHERE state='pending' AND space_id='active' AND available_at<=1000",[],|r|r.get(0)).unwrap();
 let mut p=c.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();let plan:Vec<String>=p.query_map(params![1000.,60.,"synthetic","1970-01-01T00:16:40Z","active"],|r|r.get(3)).unwrap().map(Result::unwrap).collect();drop(p);
 c.execute_batch("BEGIN").unwrap();let mut s=c.prepare(sql).unwrap();let mut selected=None;{let mut rows=s.query(params![1000.,60.,"synthetic","1970-01-01T00:16:40Z","active"]).unwrap();if let Some(row)=rows.next().unwrap(){ selected=Some(row.get::<_,i64>(0).unwrap()); }while rows.next().unwrap().is_some(){}}assert_eq!(selected,expected);
 let steps=s.get_status(StatementStatus::VmStep);let sorts=s.get_status(StatementStatus::Sort);drop(s);c.execute_batch("ROLLBACK").unwrap();results.push(json!({"n":n,"scenario":scenario,"selected":selected,"plan":plan,"vm_steps":steps,"sort_count":sorts}));
 }}
 std::fs::write(root.join("guarded-sql-probe.json"),serde_json::to_string_pretty(&json!({"sqlite_version":rusqlite::version(),"sql":sql,"results":results,"note":"One-statement CASE EXISTS short circuit plus candidate index, same FIFO result and CAS; only independent synthetic DBs, no production schema change."})).unwrap()).unwrap();
}
