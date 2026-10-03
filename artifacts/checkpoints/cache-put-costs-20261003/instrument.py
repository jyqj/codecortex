from pathlib import Path
import difflib
p=Path(__file__).resolve().parent
original=(p/'cache.original.rs').read_text()
s=original
begin=s.index('    pub fn put(');end=s.index('    /// Discardable semantics:',begin)
put=s[begin:end]
def replace(a,b):
 global put
 assert put.count(a)==1,(a,put.count(a));put=put.replace(a,b)
replace('        space.validate()?;', '        let t = std::time::Instant::now();\n        space.validate()?;')
replace('        let payload: Vec<u8>', '        diag::record(0, t);\n        let t = std::time::Instant::now();\n        let payload: Vec<u8>')
replace('        let checksum = bytes_hash(&payload);', '        diag::record(1, t);\n        let t = std::time::Instant::now();\n        let checksum = bytes_hash(&payload);')
replace('        let dir = self.object_dir(&space_digest, input, spec);','        diag::record(2, t);\n        let t = std::time::Instant::now();\n        let dir = self.object_dir(&space_digest, input, spec);\n        diag::record(3, t);\n        let t = std::time::Instant::now();')
replace('        std::fs::create_dir_all(&dir)?;', '        std::fs::create_dir_all(&dir)?;\n        diag::record(4, t);\n        let t = std::time::Instant::now();')
replace('        atomic_write(\n            &dir.join(format!("{}.meta.json", spec.as_str())),\n            &serde_json::to_vec(&meta)?,\n        )?;', '        diag::record(8, t);\n        let t = std::time::Instant::now();\n        let meta_payload = serde_json::to_vec(&meta)?;\n        diag::record(5, t);\n        let t = std::time::Instant::now();\n        atomic_write(\n            &dir.join(format!("{}.meta.json", spec.as_str())),\n            &meta_payload,\n        )?;\n        diag::record(9, t);')
replace('        Ok(self.reference(&space_digest, input, spec, &checksum))','        let t = std::time::Instant::now();\n        let reference = self.reference(&space_digest, input, spec, &checksum);\n        diag::record(6, t);\n        Ok(reference)')
s=s[:begin]+put+s[end:]
begin=s.index('fn atomic_write(');end=s.index('\nstatic TEMP_SEQ:',begin)
a=s[begin:end]
a=a.replace('    let parent = target','    let base = if target.extension().and_then(|s| s.to_str()) == Some("bin") { 10 } else { 20 };\n    let t = std::time::Instant::now();\n    let parent = target')
a=a.replace('    let write = ||','    diag::record(base, t);\n    let write = ||')
a=a.replace('let mut file = std::fs::File::create(&temp)?;', 'let mut file = diag::measure(base+1, || std::fs::File::create(&temp))?;')
a=a.replace('file.write_all(payload)?;', 'diag::measure(base+2, || file.write_all(payload))?;')
a=a.replace('file.sync_all()?;', 'diag::measure(base+3, || file.sync_all())?;')
a=a.replace('drop(file);','diag::measure(base+4, || drop(file));')
a=a.replace('std::fs::rename(&temp, target)','diag::measure(base+5, || std::fs::rename(&temp, target))')
a=a.replace('std::fs::File::open(parent)', 'diag::measure(base+6, || std::fs::File::open(parent))')
a=a.replace('let _ = dir.sync_all();','let _ = diag::measure(base+7, || dir.sync_all());\n        diag::measure(base+8, || drop(dir));')
s=s[:begin]+a+s[end:]
s += r'''
/// Diagnostic-only source-copy counters; no production change.
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
'''
(p/'source/crates/cc-semantic/src/cache.rs').write_text(s)
(p/'instrumentation.patch').write_text(''.join(difflib.unified_diff(original.splitlines(True),s.splitlines(True),fromfile='fixed-source/crates/cc-semantic/src/cache.rs',tofile='instrumented-copy/crates/cc-semantic/src/cache.rs')))
