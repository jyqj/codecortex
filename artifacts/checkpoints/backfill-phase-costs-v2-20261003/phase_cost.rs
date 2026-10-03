//! Temporary diagnostic timer. No production schema or semantics change.
use std::{collections::BTreeMap, sync::{Mutex,OnceLock}, time::Instant};
static DATA: OnceLock<Mutex<BTreeMap<&'static str,Vec<u64>>>> = OnceLock::new();
pub struct Span(&'static str,Instant);
pub fn span(name: &'static str)->Span { Span(name,Instant::now()) }
impl Drop for Span { fn drop(&mut self) { let ns=self.1.elapsed().as_nanos() as u64; DATA.get_or_init(Default::default).lock().unwrap().entry(self.0).or_default().push(ns); } }
pub fn measure<T>(name: &'static str, f:impl FnOnce()->T)->T { let _span=span(name); f() }
pub fn flush() { let data=std::mem::take(&mut *DATA.get_or_init(Default::default).lock().unwrap()); eprintln!("PHASE_COST {}",serde_json::to_string(&data).unwrap()); }
