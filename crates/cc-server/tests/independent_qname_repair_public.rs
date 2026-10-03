//! Stronger new acceptance; historical absent-qname assertion stays untouched.
use cc_model::source::{ChunkSource, SourceSnapshot};
use cc_server::engine::CodeIndex;
#[test]
fn decorated_classes_have_exact_surviving_public_class_and_method_proofs() {
    for text in ["@decorate\nclass Outer:\n    @decorate\n    class Inner:\n        def pulse(self): return 1\n", "# 中文\r\n@first(maker())\r\n@second\r\nclass Outer:\r\n    if ready:\r\n        @third\r\n        class Inner:\r\n            @staticmethod\r\n            def pulse(): return 'β'\r\n"] {
        let root=tempfile::tempdir().unwrap();
        std::fs::write(root.path().join(".codecortex.json"),r#"{"auto_index":{"enabled":false},"indexing":{"db_read_pool_size":1}}"#).unwrap();
        std::fs::write(root.path().join("micro.py"),text).unwrap();
        let mut index=CodeIndex::new(Some(root.path())).unwrap();index.build_index(true).unwrap();
        let conn=rusqlite::Connection::open(index.index_db().unwrap().admin().db_path()).unwrap();
        let source=SourceSnapshot::new(text.as_bytes());
        for (name,kind,qname) in [("Outer","class","Outer"),("Inner","class","Outer.Inner"),("pulse","method","Outer.Inner.pulse")] {
            let output=index.search().search_in_context(name,20,None).unwrap();
            let hits:Vec<_>=output.machine_pack["hits"].as_array().unwrap().iter().filter(|h|h["symbol_name"]==name).collect();
            assert!(!hits.is_empty(),"independent chunk {name}");
            println!("{name}: {} real chunks",hits.len());
            for h in hits {assert_eq!(h["symbol_kind"],kind);assert_eq!(h["metadata"]["qname"],qname);
            let proof:ChunkSource=serde_json::from_value(h["metadata"]["source_evidence"].clone()).unwrap();
            assert_eq!(proof.source,*source.identity());assert_eq!(source.slice(proof.span).unwrap(),h["text"].as_str().unwrap());
            let owner=proof.owner.unwrap();let start=source.point(owner.start).unwrap();let end=source.point(owner.end).unwrap();
            let count:i64=conn.query_row("SELECT count(*) FROM symbols WHERE file_path='micro.py' AND name=?1 AND kind=?2 AND qname=?3 AND start_line=?4 AND start_col=?5 AND end_line=?6 AND end_col=?7",rusqlite::params![name,kind,qname,start.0 as u32,start.1 as u32,end.0 as u32,end.1 as u32],|r|r.get(0)).unwrap();assert_eq!(count,1);
            let record:String=conn.query_row("SELECT record_json FROM chunk_symbol_identity WHERE chunk_id=?1",[h["chunk_id"].as_str().unwrap()],|r|r.get(0)).unwrap();
            let identity:serde_json::Value=serde_json::from_str(&record).unwrap();assert_eq!(identity["qname"],qname);assert_eq!(identity["kind"],kind);
            assert_eq!(identity["owner"],serde_json::to_value(owner).unwrap());
            }
        }
    }
}
