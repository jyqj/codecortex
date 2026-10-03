use rusqlite::{Connection, params, StatementStatus};
use serde_json::json;
use std::time::Instant;
fn main() {
    let output=std::env::args().nth(1).unwrap();
    let root=std::path::Path::new(&output);
    let candidate=std::fs::read_to_string(root.join("candidate.sql")).unwrap();
    let original="UPDATE semantic_outbox SET state='claimed',lease_token=lower(hex(randomblob(16))),lease_expires_at=?1+?2,claim_owner=?3,attempt_count=attempt_count+1,updated_at=?4 WHERE task_id=(SELECT task_id FROM semantic_outbox WHERE space_id=?5 AND state='pending' AND available_at<=?1 ORDER BY task_id ASC LIMIT 1) RETURNING task_id,lease_token,doc_key,doc_version,input_digest,op,lease_expires_at";
    let mut results=Vec::new();
    for n in [1000,5000,10000] { for scenario in ["all_ready","all_future","future_prefix","other_space_ready","other_space_only","mixed"] { for new in [false,true] {
        let c=Connection::open_in_memory().unwrap();
        cc_db::index_migrate::migrate_index_db(&c).unwrap();
        if !new { c.execute_batch("DROP INDEX semantic_outbox_fifo_pending").unwrap(); }
        c.execute_batch("BEGIN").unwrap();
        let insert_start=Instant::now();
        for i in 0..n {
            let mut state="pending";let mut space="active";let mut available=900.;
            match scenario {
                "all_future"=>available=2000.,
                "future_prefix" if i<n/2=>available=2000.,
                "other_space_ready" if i<n-1=>{space="other";available=800.;},
                "other_space_only"=>space="other",
                "mixed"=> match i%10 {0|1=>available=2000.,2|3=>space="other",4=>state="claimed",5=>state="done",6=>state="superseded",_=>{}},
                _=>{}
            }
            c.execute("INSERT INTO semantic_outbox(doc_key,doc_version,input_digest,space_id,op,state,available_at,created_at,updated_at) VALUES(?1,'v','in',?2,'embed',?3,?4,'created','updated')",params![format!("doc{i}"),space,state,available]).unwrap();
        }
        c.execute_batch("COMMIT").unwrap();let insert_ms=insert_start.elapsed().as_secs_f64()*1000.;
        let sql=if new {candidate.as_str()} else {original};
        let mut plan_stmt=c.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
        let plan:Vec<String>=plan_stmt.query_map(params![1000.,60.,"owner","1970-01-01T00:16:40+00:00","active"],|r|r.get(3)).unwrap().map(Result::unwrap).collect();drop(plan_stmt);
        let expected:Option<i64>=c.query_row("SELECT min(task_id) FROM semantic_outbox WHERE space_id='active' AND state='pending' AND available_at<=1000",[],|r|r.get(0)).unwrap();
        let mut timings=Vec::new();
        for _ in 0..10 {
            c.execute_batch("BEGIN").unwrap();let t=Instant::now();
            let selected=if new {cc_db::semantic_outbox::claim_next_on(&c,"active","owner",1000.,60.).unwrap().map(|x|x.task_id)} else {
                let mut s=c.prepare_cached(sql).unwrap();let mut rows=s.query(params![1000.,60.,"owner","1970-01-01T00:16:40+00:00","active"]).unwrap();rows.next().unwrap().map(|r|r.get::<_,i64>(0).unwrap())
            };timings.push(t.elapsed().as_secs_f64()*1000.);assert_eq!(selected,expected);c.execute_batch("ROLLBACK").unwrap();
        }
        c.execute_batch("BEGIN").unwrap();let mut s=c.prepare(sql).unwrap();
        { let mut rows=s.query(params![1000.,60.,"owner","1970-01-01T00:16:40+00:00","active"]).unwrap();while rows.next().unwrap().is_some() {} }
        let vm=s.get_status(StatementStatus::VmStep);let sorts=s.get_status(StatementStatus::Sort);drop(s);c.execute_batch("ROLLBACK").unwrap();
        let pages:i64=c.pragma_query_value(None,"page_count",|r|r.get(0)).unwrap();
        if new {assert!(plan.iter().any(|p|p.contains("semantic_outbox_fifo_pending")));assert_eq!(sorts,0);}
        results.push(json!({"n":n,"scenario":scenario,"candidate":new,"selected":expected,"plan":plan,"vm_steps":vm,"sorts":sorts,"rollback_claim_ms":timings,"insert_commit_ms":insert_ms,"page_count":pages}));
    }}}
    std::fs::write(root.join("sql-results.json"),serde_json::to_string_pretty(&json!({"sqlite":rusqlite::version(),"original":original,"candidate":candidate,"results":results})).unwrap()).unwrap();
}
