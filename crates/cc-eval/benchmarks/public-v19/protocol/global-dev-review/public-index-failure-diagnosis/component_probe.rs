use cc_model::{Language,resolution::{resolution_name_keys,ResolutionManifest,DependencyKind,ResolutionRecord,ResolutionOutcome}};
use cc_parsers::ParserRegistry;
use std::collections::BTreeMap;
fn main(){
 let a:Vec<_>=std::env::args().collect();
 let src=std::fs::read_to_string(&a[1]).unwrap();
 let lang=if a[2]=="python"{Language::Python}else{Language::Go};
 let o=ParserRegistry::new().parse(&a[3],&src,lang).unwrap();
 let mut ids:BTreeMap<String,Vec<String>>=BTreeMap::new();
 let mut m=ResolutionManifest::new();
 for e in &o.call_edges {
  ids.entry(e.edge_id.clone()).or_default().push(format!("callee={:?} receiver={:?} line={} col={} end_col={}",e.callee_symbol,e.receiver_expr,e.line,e.start_col,e.end_col));
  m.record(ResolutionRecord{site_kind:"call".into(),site_id:e.edge_id.clone(),query:e.callee_symbol.clone(),outcome:ResolutionOutcome::Unresolved{reason:"diagnostic_probe".into()}});
 }
 let duplicates=ids.values().filter(|es|es.len()>1).count();
 for (id,es) in ids {if es.len()>1 {println!("DUPLICATE {id} {es:?}");}}
 m.normalize();
 if duplicates>0 {assert!(m.validate().unwrap_err().to_string().contains("conflicting duplicate call site"));}
 println!("COUNTS calls={} refs={} duplicate_ids={}",o.call_edges.len(),o.symbol_refs.len(),duplicates);
 // Exercise the actual shared model contract, not a copied implementation.
 assert!(resolution_name_keys("...").contains(""));
 let mut invalid=ResolutionManifest::new();
 for key in resolution_name_keys("..."){invalid.dependency(DependencyKind::NameBucket,key);}
 assert!(invalid.validate().unwrap_err().to_string().contains("invalid dependency key"));
 assert!(!resolution_name_keys("pkg.Type").contains(""));
 println!("MODEL ellipsis_empty_leaf_reproduced=1 invalid_dependency_reproduced=1 valid_qualified_name_control=1");
 for s in o.symbols {if s.return_type.as_deref().is_some_and(|t|t.contains("...")){println!("TYPE ellipsis_return_annotation_count=1 line={}",s.start_line);}}
}
