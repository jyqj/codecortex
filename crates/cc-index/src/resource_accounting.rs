//! Kernel accounting of the calling process, shared by product diagnostics and
//! evaluation. SELF and /proc/self do not interpret a namespace PID as a host PID.
use serde::Serialize;

/// ru_maxrss is bytes on macOS and KiB on Linux. Missing units stay unavailable.
pub fn high_water_bytes(platform: &str, value: u64) -> Option<u64> {
    match platform {
        "macos" => Some(value),
        "linux" => value.checked_mul(1024),
        _ => None,
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ProcessIo {
    pub read_bytes: u64,
    pub write_bytes: u64,
    pub cancelled_write_bytes: u64,
    pub rchar: u64,
    pub wchar: u64,
    pub read_syscalls: u64,
    pub write_syscalls: u64,
    pub method: &'static str,
}

/// All threads in this process. Current RSS and lifetime RSS high-water are
/// separate fields. IO counts are cumulative; rchar/wchar include cached IO.
/// This is never a process-tree or external-service measurement.
#[derive(Debug, Clone, Serialize)]
pub struct CurrentProcessUsage {
    pub pid: u32,
    pub user_cpu_ns: u64,
    pub system_cpu_ns: u64,
    pub peak_resident_bytes: Option<u64>,
    pub resident_bytes: Option<u64>,
    pub io: Option<ProcessIo>,
    pub method: &'static str,
    pub resident_method: &'static str,
    pub io_unavailable_reason: Option<&'static str>,
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn timeval_ns(seconds: impl TryInto<u64>, microseconds: impl TryInto<u64>) -> Option<u64> {
    let seconds = seconds.try_into().ok()?;
    let microseconds = microseconds.try_into().ok()?;
    if microseconds >= 1_000_000 {
        return None;
    }
    seconds
        .checked_mul(1_000_000_000)?
        .checked_add(microseconds.checked_mul(1000)?)
}

#[cfg(any(target_os = "linux", test))]
fn parse_io(text: &str) -> Option<ProcessIo> {
    let mut fields = std::collections::BTreeMap::new();
    for line in text.lines() {
        let (key, value) = line.split_once(':')?;
        if fields
            .insert(key, value.trim().parse::<u64>().ok()?)
            .is_some()
        {
            return None;
        }
    }
    Some(ProcessIo {
        read_bytes: *fields.get("read_bytes")?,
        write_bytes: *fields.get("write_bytes")?,
        cancelled_write_bytes: *fields.get("cancelled_write_bytes")?,
        rchar: *fields.get("rchar")?,
        wchar: *fields.get("wchar")?,
        read_syscalls: *fields.get("syscr")?,
        write_syscalls: *fields.get("syscw")?,
        method: "Linux /proc/self/io; process lifetime counters; storage bytes and logical characters separate",
    })
}

fn current_io() -> Option<ProcessIo> {
    #[cfg(target_os = "linux")]
    {
        use std::io::Read;
        let mut bytes = Vec::new();
        std::fs::File::open("/proc/self/io")
            .ok()?
            .take(32769)
            .read_to_end(&mut bytes)
            .ok()?;
        if bytes.len() > 32768 {
            return None;
        }
        parse_io(std::str::from_utf8(&bytes).ok()?)
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

pub fn current_process_usage() -> Option<CurrentProcessUsage> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
        // SAFETY: on success the kernel initializes the writable C-layout
        // rusage. SELF refers directly to the caller and all its threads.
        if unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) } != 0 {
            return None;
        }
        // SAFETY: initialization was checked above.
        let usage = unsafe { usage.assume_init() };
        let io = current_io();
        let io_unavailable_reason = io.is_none().then_some(if cfg!(target_os = "linux") {
            "proc_self_io_unavailable_or_invalid"
        } else {
            "process_io_not_implemented_on_this_platform"
        });
        Some(CurrentProcessUsage {
            pid: std::process::id(),
            user_cpu_ns: timeval_ns(usage.ru_utime.tv_sec, usage.ru_utime.tv_usec)?,
            system_cpu_ns: timeval_ns(usage.ru_stime.tv_sec, usage.ru_stime.tv_usec)?,
            peak_resident_bytes: u64::try_from(usage.ru_maxrss)
                .ok()
                .and_then(|value| high_water_bytes(std::env::consts::OS, value)),
            resident_bytes: crate::process_rss_bytes_opt(),
            io,
            method: "getrusage(RUSAGE_SELF); all caller threads; lifetime RSS high-water",
            resident_method: "native current-process RSS; /proc/self/statm on Linux or Mach task_info on macOS; no numeric PID lookup",
            io_unavailable_reason,
        })
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_units_and_invalid_io_do_not_become_zero() {
        assert_eq!(high_water_bytes("linux", 42), Some(43008));
        assert_eq!(high_water_bytes("macos", 42), Some(42));
        assert_eq!(high_water_bytes("other", 42), None);
        assert_eq!(high_water_bytes("linux", u64::MAX), None);
        let text = "rchar: 1\nwchar: 2\nsyscr: 3\nsyscw: 4\nread_bytes: 0\nwrite_bytes: 6\ncancelled_write_bytes: 0\n";
        assert_eq!(parse_io(text).unwrap().read_bytes, 0);
        for invalid in [
            String::new(),
            text.replace("wchar: 2\n", ""),
            text.replace("rchar: 1", "rchar: -1"),
            format!("{text}rchar: 1\n"),
        ] {
            assert!(parse_io(&invalid).is_none());
        }
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn self_identity_cpu_io_and_nullable_current_rss_are_distinct() {
        assert_eq!(timeval_ns(-1, 0), None);
        assert_eq!(timeval_ns(0, -1), None);
        assert_eq!(timeval_ns(0, 1_000_000), None);
        assert_eq!(timeval_ns(i64::MAX, 0), None);
        assert_eq!(timeval_ns(1, 42), Some(1_000_042_000));
        let before = current_process_usage().expect("native SELF accounting");
        let started = std::time::Instant::now();
        while started.elapsed() < std::time::Duration::from_millis(30) {
            std::hint::black_box((0_u64..1000).sum::<u64>());
        }
        let after = current_process_usage().unwrap();
        assert_eq!(after.pid, std::process::id());
        assert_eq!(before.pid, after.pid);
        assert!(
            after.user_cpu_ns + after.system_cpu_ns > before.user_cpu_ns + before.system_cpu_ns
        );
        assert!(after.peak_resident_bytes.is_some_and(|bytes| bytes > 0));
        assert!(after.resident_bytes.is_none_or(|bytes| bytes > 0));
        if let (Some(before), Some(after)) = (before.io, after.io) {
            assert!(after.rchar >= before.rchar);
            assert!(after.read_syscalls >= before.read_syscalls);
        }
    }
}
