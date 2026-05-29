//! Lightweight RSS-based memory budget controller for the indexing pipeline.
//!
//! This is a self-contained copy scoped to cc-index to avoid a circular
//! extra dependency cycles.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Pluggable source for reading the process's current resident set size.
///
/// The default uses the platform-specific `current_rss_bytes()` reader; tests
/// inject a controllable closure to simulate RSS rising above budget and then
/// falling back below it.
type RssSource = Box<dyn Fn() -> u64 + Send + Sync>;

/// RSS-based memory budget controller.
pub struct MemoryBudget {
    total_budget: u64,
    current_rss: AtomicU64,
    over_budget: AtomicBool,
    pressure_warned: AtomicBool,
    rss_source: RssSource,
}

impl MemoryBudget {
    /// Create a new budget as a fraction of total physical RAM.
    pub fn new(fraction: f64) -> Self {
        let total_ram = Self::total_physical_ram();
        let budget = (total_ram as f64 * fraction.clamp(0.1, 0.95)) as u64;
        tracing::info!(
            total_ram_mb = total_ram / 1_048_576,
            budget_mb = budget / 1_048_576,
            fraction,
            "memory budget initialized"
        );
        Self {
            total_budget: budget,
            current_rss: AtomicU64::new(0),
            over_budget: AtomicBool::new(false),
            pressure_warned: AtomicBool::new(false),
            rss_source: Box::new(Self::current_rss_bytes),
        }
    }

    /// Construct a budget with an explicit byte budget and an injectable RSS
    /// source. Intended for tests that need to drive the pressure state
    /// deterministically without touching the real process RSS.
    #[cfg(test)]
    pub fn with_rss_source<F>(total_budget: u64, rss_source: F) -> Self
    where
        F: Fn() -> u64 + Send + Sync + 'static,
    {
        Self {
            total_budget,
            current_rss: AtomicU64::new(0),
            over_budget: AtomicBool::new(false),
            pressure_warned: AtomicBool::new(false),
            rss_source: Box::new(rss_source),
        }
    }

    /// Refresh RSS reading and update pressure state.
    pub fn refresh(&self) {
        let rss = (self.rss_source)();
        self.current_rss.store(rss, Ordering::Relaxed);
        let over = rss > self.total_budget;
        let was_over = self.over_budget.swap(over, Ordering::Relaxed);

        if over && !was_over {
            self.pressure_warned.store(true, Ordering::Relaxed);
            tracing::warn!(
                rss_mb = rss / 1_048_576,
                budget_mb = self.total_budget / 1_048_576,
                "memory pressure: RSS exceeds budget"
            );
        } else if !over && was_over && self.pressure_warned.swap(false, Ordering::Relaxed) {
            tracing::info!(
                rss_mb = rss / 1_048_576,
                budget_mb = self.total_budget / 1_048_576,
                "memory pressure resolved"
            );
        }
    }

    /// Check if currently over budget.
    pub fn is_over_budget(&self) -> bool {
        self.over_budget.load(Ordering::Relaxed)
    }

    /// Get last observed RSS in bytes.
    pub fn current_rss(&self) -> u64 {
        self.current_rss.load(Ordering::Relaxed)
    }

    /// Calculate suggested batch size for streaming parse.
    pub fn suggested_batch_size(&self, total_files: usize, default_batch: usize) -> usize {
        if total_files <= default_batch {
            return total_files;
        }
        if self.is_over_budget() {
            (default_batch / 2).max(20)
        } else {
            default_batch
        }
    }

    // ── platform-specific helpers ──────────────────────────────────────────

    fn current_rss_bytes() -> u64 {
        #[cfg(target_os = "linux")]
        {
            if let Ok(statm) = std::fs::read_to_string("/proc/self/statm") {
                if let Some(pages_str) = statm.split_whitespace().nth(1) {
                    if let Ok(pages) = pages_str.parse::<u64>() {
                        let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
                        if page_size > 0 {
                            return pages.saturating_mul(page_size as u64);
                        }
                    }
                }
            }
            return 0;
        }

        #[cfg(target_os = "macos")]
        {
            // Read the *current* resident size via the mach task_info API.
            // getrusage().ru_maxrss is the peak RSS (monotonically non-decreasing),
            // which made the "pressure resolved" branch unreachable and broke the
            // adaptive batch sizing. MACH_TASK_BASIC_INFO.resident_size reflects
            // live memory and can fall back down after pressure is released.
            let mut info = std::mem::MaybeUninit::<libc::mach_task_basic_info>::uninit();
            let mut count: libc::mach_msg_type_number_t = libc::MACH_TASK_BASIC_INFO_COUNT;
            // mach_task_self_ / mach_task_self() are deprecated in libc in favor
            // of the mach2 crate, but reading the static directly avoids pulling
            // in an extra dependency for a single port handle.
            #[allow(deprecated)]
            let task = unsafe { libc::mach_task_self_ };
            let rc = unsafe {
                libc::task_info(
                    task,
                    libc::MACH_TASK_BASIC_INFO,
                    info.as_mut_ptr() as libc::task_info_t,
                    &mut count,
                )
            };
            if rc == libc::KERN_SUCCESS {
                let info = unsafe { info.assume_init() };
                return info.resident_size;
            }
            0
        }

        #[cfg(target_os = "windows")]
        {
            // Use GetProcessMemoryInfo via PowerShell to read WorkingSetSize.
            // This avoids adding windows-sys / winapi as a build dependency.
            match std::process::Command::new("powershell")
                .args([
                    "-NoProfile",
                    "-Command",
                    "(Get-Process -Id $PID).WorkingSet64",
                ])
                .output()
            {
                Ok(output) if output.status.success() => {
                    let text = String::from_utf8_lossy(&output.stdout);
                    if let Ok(bytes) = text.trim().parse::<u64>() {
                        return bytes;
                    }
                }
                _ => {}
            }
            tracing::warn!("Windows RSS detection failed; memory budget may be inaccurate");
            0
        }

        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        {
            0
        }
    }

    fn total_physical_ram() -> u64 {
        #[cfg(target_os = "macos")]
        {
            let mut memsize: u64 = 0;
            let mut size = std::mem::size_of::<u64>();
            let name = b"hw.memsize\0";
            unsafe {
                libc::sysctlbyname(
                    name.as_ptr() as *const libc::c_char,
                    &mut memsize as *mut u64 as *mut libc::c_void,
                    &mut size,
                    std::ptr::null_mut(),
                    0,
                );
            }
            if memsize > 0 {
                return memsize;
            }
            8 * 1024 * 1024 * 1024
        }

        #[cfg(target_os = "linux")]
        {
            if let Ok(content) = std::fs::read_to_string("/proc/meminfo") {
                for line in content.lines() {
                    if line.starts_with("MemTotal:") {
                        if let Some(kb_str) = line.split_whitespace().nth(1) {
                            if let Ok(kb) = kb_str.parse::<u64>() {
                                return kb * 1024;
                            }
                        }
                    }
                }
            }
            8 * 1024 * 1024 * 1024
        }

        #[cfg(target_os = "windows")]
        {
            // Query total physical RAM via PowerShell (avoids winapi dependency).
            match std::process::Command::new("powershell")
                .args([
                    "-NoProfile",
                    "-Command",
                    "(Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory",
                ])
                .output()
            {
                Ok(output) if output.status.success() => {
                    let text = String::from_utf8_lossy(&output.stdout);
                    if let Ok(bytes) = text.trim().parse::<u64>() {
                        if bytes > 0 {
                            return bytes;
                        }
                    }
                }
                _ => {}
            }
            tracing::warn!("Windows total RAM detection failed; using 8 GiB fallback");
            8 * 1024 * 1024 * 1024
        }

        #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
        {
            8 * 1024 * 1024 * 1024
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64;
    use std::sync::Arc;

    /// Simulate RSS rising above budget and then falling back below it.
    /// With a current-RSS reader (not peak), the pressure state must be able
    /// to recover from over-budget back to under-budget, and the suggested
    /// batch size must recover alongside it.
    #[test]
    fn pressure_recovers_when_rss_drops_back() {
        let budget = 1_000u64;
        let rss = Arc::new(AtomicU64::new(0));
        let rss_reader = {
            let rss = Arc::clone(&rss);
            move || rss.load(Ordering::Relaxed)
        };
        let mb = MemoryBudget::with_rss_source(budget, rss_reader);

        // Below budget initially.
        rss.store(500, Ordering::Relaxed);
        mb.refresh();
        assert!(!mb.is_over_budget(), "500 < 1000 should be under budget");
        assert_eq!(
            mb.suggested_batch_size(1000, 200),
            200,
            "under budget keeps the default batch size"
        );

        // Rise above budget.
        rss.store(2_000, Ordering::Relaxed);
        mb.refresh();
        assert!(mb.is_over_budget(), "2000 > 1000 should be over budget");
        assert_eq!(
            mb.suggested_batch_size(1000, 200),
            100,
            "over budget halves the batch size"
        );

        // Drop back below budget — this is the case the peak-RSS bug broke.
        rss.store(400, Ordering::Relaxed);
        mb.refresh();
        assert!(
            !mb.is_over_budget(),
            "RSS dropped back below budget, pressure must resolve"
        );
        assert_eq!(
            mb.suggested_batch_size(1000, 200),
            200,
            "batch size must recover after pressure resolves"
        );
    }

    /// The real platform reader must report a plausible non-zero current RSS
    /// for the running test process (guards against a broken syscall path).
    #[test]
    fn real_current_rss_is_nonzero() {
        let rss = MemoryBudget::current_rss_bytes();
        // A running Rust test process always has a resident set; 0 means the
        // platform syscall path is broken.
        assert!(rss > 0, "current RSS should be > 0, got {}", rss);
    }
}
