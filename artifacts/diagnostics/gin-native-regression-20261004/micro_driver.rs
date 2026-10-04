//! Own Go analogy fixture; real index/search/MCP/normalizer/unchanged native scorer.
use cc_eval::benchmark::{metrics, normalizer, schema::Query};
use cc_server::engine::CodeIndex;
use serde_json::json;
use std::path::Path;
fn main() {
 let args:Vec<String>=std::env::args().collect(); let root=Path::new(&args[1]); let out=Path::new(&args[2]);
 std::fs::create_dir_all(root).unwrap(); std::fs::create_dir_all(out).unwrap();
 let original="package grove\n\n// Cedar stores the garden state.\ntype Cedar struct {\n    Ready bool\n}\n\n// Pulse changes the ready state.\nfunc (c *Cedar) Pulse() {\n    c.Ready = true\n}\n\n// Feeder accepts a seed.\ntype Feeder interface {\n    Feed(seed int) error\n}\n\n// SeedLabel names a seed.\ntype SeedLabel string\n";
 let text=if args.get(3).is_some_and(|v|v=="inversion") {original.replace("    Ready bool", "    // Pulse API changes the ready state with the garden storage.\n    Ready bool")} else if args.get(3).is_some_and(|v|v=="receiver") {original.replace("    Ready bool", "    Ready bool\n    State bool")} else {original.into()};
 std::fs::write(root.join("micro.go"),text).unwrap();
 std::fs::write(root.join(".codecortex.json"),r#"{"auto_index":{"enabled":false},"indexing":{"db_read_pool_size":1}}"#).unwrap();
 let mut index=CodeIndex::new(Some(root)).unwrap(); index.build_index(true).unwrap();
 let backend=cc_eval::runner::CodeIndexBackend::open_existing(root).unwrap();
 let mut result=json!({});
 for query in ["Cedar Pulse", "Pulse", "Feeder", "SeedLabel", "Which Cedar API changes the ready state with Pulse?"] {
  let engine=serde_json::to_value(index.search().search_in_context(query,10,None).unwrap()).unwrap();
  let mcp=backend.call_tool("search",&json!({"query":query,"top_k":10,"mode":"hybrid"})).unwrap();
  let (name,kind)=match query {"Feeder"=>("Feeder","interface"),"SeedLabel"=>("SeedLabel","type_alias"),_=>("Pulse","method")};
  let gold:Query=serde_json::from_value(json!({"id":"own-go-analogy","category":"exact-symbol","difficulty":1,"language":"Go","split":"synthetic","query_family":"own-go-analogy","query":query,"path_prefix":null,"no_answer":false,"expected_files":[],"answers":[{"id":"declaration","primary":true,"grade":3,"alternatives":[{"path":"micro.go","symbol":{"name":name,"qname":null,"kind":kind},"span":null}]}]})).unwrap();
  for (api,payload) in [("engine",engine),("mcp",mcp)] {
   let (mut hits,status)=normalizer::mcp(&payload).unwrap();
   for h in &mut hits {normalizer::verify_source(h,root).unwrap();assert_eq!(h.evidence_valid,Some(true));}
   let score=metrics::native(&hits,&gold);
   let mut wrong=gold.clone();wrong.answers[0].alternatives[0].symbol.as_mut().unwrap().name="Unrelated".into();assert_eq!(metrics::native(&hits,&wrong).recall10,Some(0.0));
   let mut qname=gold.clone();qname.answers[0].alternatives[0].symbol.as_mut().unwrap().qname=Some(if name=="Pulse" {"Cedar.Pulse"} else {name}.into());
   result[query][api]=json!({"raw":payload,"normalized":hits,"status":status,"scores":score,"qname_required_scores":metrics::native(&hits,&qname)});
  }
 }
 std::fs::write(out.join("results.json"),serde_json::to_vec_pretty(&result).unwrap()).unwrap();
 println!("real index/engine/MCP/normalizer/native completed; all evidence valid, unrelated name rejected");
}
