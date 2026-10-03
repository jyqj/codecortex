use cc_semantic::{cache::{ArtifactCache, CacheRead}, spec::{VectorSpace, DocumentEncodingSpec}, types::InputDigest};
use std::time::Instant;
fn main() {
 let args: Vec<String> = std::env::args().collect();
 let root = std::path::PathBuf::from(&args[1]);
 let n: usize = args[2].parse().unwrap();
 assert!(n <= 1000);
 let space = VectorSpace::new("synthetic/cache-cost",128).unwrap();
 let spec = DocumentEncodingSpec::new(space.clone(),None,512,"synthetic").unwrap().digest().unwrap();
 let inputs: Vec<_> = (0..n).map(|i| InputDigest::of_input(format!("bounded-cache-cost-{i}").as_bytes()).unwrap()).collect();
 let vector: Vec<f32> = (0..128).map(|i| (i+1) as f32 /128.0).collect();
 let cache = ArtifactCache::open(&root,"cost-probe".to_string()).unwrap();
 for pass in ["new", "rewrite"] {
  #[cfg(feature="diag-probe")] cc_semantic::cache::diag::reset();
  let start=Instant::now();
  let refs: Vec<_> = inputs.iter().map(|input| cache.put(&space,input,&spec,&vector,1791028800).unwrap()).collect();
  let put_ns=start.elapsed().as_nanos();
  #[cfg(feature="diag-probe")] let stages=cc_semantic::cache::diag::take();
  #[cfg(not(feature="diag-probe"))] let stages: Vec<(String,u128,u64)>=vec![];
  let start=Instant::now();
  for (input,reference) in inputs.iter().zip(refs.iter()) {
   match cache.get(&space,input,&spec).unwrap() {
    CacheRead::Hit(v) => {assert_eq!(&v.artifact_ref,reference);assert_eq!(v.dimension,128);assert_eq!(v.data,vector);},
    other => panic!("not verified hit: {other:?}"),
   }
  }
  println!("{}",serde_json::json!({"pass":pass,"n":n,"dims":128,"instrumented":cfg!(feature="diag-probe"),"put_ns":put_ns,"get_ns":start.elapsed().as_nanos(),"verified_hits":n,"stages":stages,"root":root}));
 }
}
