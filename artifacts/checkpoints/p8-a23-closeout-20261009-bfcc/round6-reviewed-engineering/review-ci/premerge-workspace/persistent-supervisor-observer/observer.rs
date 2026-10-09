use cc_eval::benchmark::p8_scale::{self, ScalePlan};
use std::path::Path;
use std::time::{Duration, Instant};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    assert_eq!(args.len(), 3, "observer requires original helper and a new output directory");
    let plan = ScalePlan { deadline_ms: 250, ..ScalePlan::default() };
    let start = Instant::now();
    let report = p8_scale::run_supervised(&plan, Path::new(&args[2]), Path::new(&args[1])).unwrap();
    let elapsed = start.elapsed();
    println!("{report}");
    eprintln!("observer_elapsed_ns={}", elapsed.as_nanos());
    assert!(
        elapsed < Duration::from_secs(2),
        "a descendant must not turn the 250ms deadline into a 20s join"
    );
    assert_eq!(report["exit_code"], 0, "{report}");
    assert_eq!(report["stderr_complete"], true);
    assert_eq!(report["release_certification"], "not_run");
}
