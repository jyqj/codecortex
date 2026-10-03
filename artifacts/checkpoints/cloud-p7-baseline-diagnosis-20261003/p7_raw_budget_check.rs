fn main() {
    for name in std::env::args().skip(1) {
        let bytes=std::fs::read(&name).unwrap();
        let v:serde_json::Value=serde_json::from_slice(&bytes).unwrap();
        let actual=serde_json::to_vec(&v).unwrap().len();
        let packing=&v["evidence_summary"]["packing"];
        let receipt=packing["used_bytes"].as_u64().unwrap();
        let limit=packing["limit_bytes"].as_u64().unwrap();
        let tokens=v["token_estimate"].as_u64().unwrap();
        assert_eq!(actual as u64,receipt,"{name}");
        assert!(actual as u64<=limit,"{name}");
        assert_eq!(tokens,(actual as u64).div_ceil(4),"{name}");
        println!("{}",serde_json::json!({"path":name,"actual_compact_bytes":actual,"used_bytes":receipt,"limit_bytes":limit,"token_estimate":tokens}));
    }
}
