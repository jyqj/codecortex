pub mod diag {
 use std::{cell::RefCell, time::Instant};
 thread_local! { static STATS: RefCell<[(u128,u64);30]> = const { RefCell::new([(0,0);30]) }; }
 pub fn record(id:usize,t:Instant) { let ns=t.elapsed().as_nanos(); STATS.with(|s| {let mut s=s.borrow_mut();s[id].0+=ns;s[id].1+=1;}); }
 pub fn measure<T>(id:usize,f:impl FnOnce()->T)->T {let t=Instant::now();let r=f();record(id,t);r}
 pub fn reset() {STATS.with(|s|*s.borrow_mut()=[(0,0);30]);}
 pub fn take()->Vec<(String,u128,u64)> {
  let names=["validation_space_digest","encode_f32","checksum_meta_construct","object_path","create_dir_all","meta_json","reference","unused","bin_atomic_total","meta_atomic_total","bin_temp_path","bin_file_create","bin_write","bin_file_sync","bin_file_close","bin_rename","bin_dir_open","bin_dir_sync","bin_dir_close","unused","meta_temp_path","meta_file_create","meta_write","meta_file_sync","meta_file_close","meta_rename","meta_dir_open","meta_dir_sync","meta_dir_close","unused"];
  STATS.with(|s|s.borrow().iter().enumerate().filter(|(_,(_,n))|*n>0).map(|(i,(ns,n))|(names[i].into(),*ns,*n)).collect())
 }
}

fn main() {
 let mut rows=Vec::new();
 for _ in 0..5 {
  diag::reset();let start=std::time::Instant::now();
  for _ in 0..10000 {diag::measure(0, || std::hint::black_box(()));}
  let elapsed=start.elapsed().as_nanos();assert_eq!(diag::take()[0].2,10000);rows.push(elapsed);
 }
 println!("{:?}",rows);
}
