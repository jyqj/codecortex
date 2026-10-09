//! One-shot diagnostic fixture for the existing supervised-child contract.
//! No indexing, scale workload, changed deadline or acceptance retry.
use cc_eval::benchmark::p8_scale::{run_supervised, ScalePlan};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let root = PathBuf::from(args.next().ok_or("expected one fresh output directory")?);
    if args.next().is_some() {
        return Err("expected exactly one fresh output directory".into());
    }
    std::fs::create_dir(&root)?;
    let helper = root.join("stderr-holder.sh");
    std::fs::write(
        &helper,
        concat!(
            "#!/bin/sh\n",
            "printf '%s\\n' 'probe:started' >&2\n",
            "while [ \"$#\" -gt 0 ]; do\n",
            " case \"$1\" in --output) shift; out=\"$1\";; esac\n",
            " shift\n",
            "done\n",
            "printf '%s\\n' 'probe:parsed' >&2\n",
            "printf '%s\\n' 'probe:before_fork' >&2\n",
            "sleep 20 &\n",
            "printf 'probe:after_fork descendant=%s\\n' \"$!\" >&2\n",
            "printf '{\"passed\":true}' > \"$out/worker-summary.json\"\n",
            "printf '%s\\n' 'probe:summary_written' >&2\n",
        ),
    )?;
    std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o700))?;
    let plan = ScalePlan {
        deadline_ms: 250,
        ..ScalePlan::default()
    };
    let started = std::time::Instant::now();
    let report = run_supervised(&plan, &root.join("run"), &helper)?;
    println!("{report}");
    eprintln!("probe_outer_elapsed_ms={}", started.elapsed().as_millis());
    std::process::exit(report["exit_code"].as_i64().unwrap_or(2) as i32);
}
