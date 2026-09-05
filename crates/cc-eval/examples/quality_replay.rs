//! Recompute a report from immutable raw observations, without running the server.
use std::{error::Error, fs, io::{BufRead,BufReader}};
use cc_eval::quality::{self,Header,Sample};
use serde_json::Value;
fn main() -> Result<(),Box<dyn Error>> {
    let args:Vec<_>=std::env::args().collect();
    if args.len()!=3 { return Err("usage: quality_replay RAW_JSONL REPORT_JSON".into()); }
    let mut header:Option<Header>=None;
    let mut samples:Vec<Sample>=Vec::new();
    for line in BufReader::new(fs::File::open(&args[1])?).lines() {
        let row:Value=serde_json::from_str(&line?)?;
        match row["kind"].as_str() {
            Some("header") if header.is_none() && samples.is_empty()=>header=Some(serde_json::from_value(row["data"].clone())?),
            Some("sample") if header.is_some()=>samples.push(serde_json::from_value(row["data"].clone())?),
            Some("index" | "warmup") if header.is_some()=>{},
            _=>return Err("unexpected or duplicate raw record".into()),
        }
    }
    let result=quality::report(&header.ok_or("missing header")?,&samples)?;
    fs::write(&args[2],serde_json::to_vec_pretty(&result)?)?;
    if result["passed"]!=true {return Err("quality regression gate failed".into());}
    Ok(())
}
