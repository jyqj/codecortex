//! Recompute a report from immutable raw observations, without running the server.
use cc_eval::quality;
use std::{error::Error, fs, io::BufReader};
fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 3 { return Err("usage: quality_replay RAW_JSONL REPORT_JSON".into()); }
    let (header, samples) = quality::read_raw(BufReader::new(fs::File::open(&args[1])?))?;
    let result = quality::report(&header, &samples)?;
    fs::write(&args[2], serde_json::to_vec_pretty(&result)?)?;
    if result["passed"] != true { return Err("quality regression gate failed".into()); }
    Ok(())
}
