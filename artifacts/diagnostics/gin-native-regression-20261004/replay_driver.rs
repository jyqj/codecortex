//! Read-only replay of stored raw through actual normalizer and unchanged scorer.
use cc_eval::benchmark::{metrics,normalizer,schema::{Query,Row,ScoreProfile}};
use std::{collections::BTreeMap,path::Path};
fn main(){let a:Vec<String>=std::env::args().collect();let root=Path::new(&a[1]);let mut rows=0;let mut hits=0;
 for arm in ["baseline","candidate"] {for profile in ["native","compat"] {let p=root.join(arm).join("runs").join(profile);
 let qs:BTreeMap<String,Query>=std::fs::read_to_string(p.join("queries.jsonl")).unwrap().lines().map(|l|{let q:Query=serde_json::from_str(l).unwrap();(q.id.clone(),q)}).collect();
 let rs=std::fs::read_to_string(p.join("normalized.jsonl")).unwrap();let ss=std::fs::read_to_string(p.join("scores.jsonl")).unwrap();
 for (line,expected) in rs.lines().zip(ss.lines()) {let row:Row=serde_json::from_str(line).unwrap();let score:metrics::Scores=serde_json::from_str(expected).unwrap();let actual=metrics::score(&row,&qs[&row.case_id],if profile=="native" {ScoreProfile::Native}else{ScoreProfile::OceCompat}).unwrap();assert_eq!(actual,score);
 let raw=serde_json::from_slice(&std::fs::read(p.join(&row.raw_path)).unwrap()).unwrap();let (mut normalized,status)=normalizer::mcp(&raw).unwrap();assert_eq!(status,row.status);assert_eq!(normalized.len(),row.hits.len());
 for (h,stored) in normalized.iter_mut().zip(&row.hits){h.span=stored.span.clone();h.evidence_valid=stored.evidence_valid;assert_eq!(serde_json::to_value(h).unwrap(),serde_json::to_value(stored).unwrap());hits+=1;}
 rows+=1;}}}
 let micro:serde_json::Value=serde_json::from_slice(&std::fs::read(&a[2]).unwrap()).unwrap();
 for name in ["Feeder","SeedLabel"] {let hs:Vec<cc_eval::benchmark::schema::Hit>=serde_json::from_value(micro[name]["mcp"]["normalized"].clone()).unwrap();
 let q:Query=serde_json::from_value(serde_json::json!({"id":"synthetic-stale-kind","category":"exact-symbol","difficulty":1,"language":"Go","split":"synthetic","query_family":"synthetic-stale-kind","query":name,"path_prefix":null,"no_answer":false,"expected_files":[],"answers":[{"id":"one","primary":true,"grade":3,"alternatives":[{"path":"micro.go","symbol":{"name":name,"qname":null,"kind":"function"},"span":null}]}]})).unwrap();assert_eq!(metrics::native(&hs,&q).recall10,Some(0.0));}
 println!("{{\"rows\":{rows},\"hits\":{hits},\"synthetic_stale_function_kind_rejected\":true,\"all_scores_exact\":true,\"all_raw_normalizer_fields_exact\":true,\"source_verification\":\"preserved archived spans and validity; no fresh full Gin indexing\"}}");}
