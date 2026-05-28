//! Lightweight RSS-based memory budget controller for the indexing pipeline.
//!
//! This is a self-contained copy scoped to cc-index to avoid a circular
//! extra dependency cycles.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// RSS-based memory budget controller.
pub struct MemoryBudget {
    total_budget: u64,
    current_rss: AtomicU64,
    over_budget: AtomicBool,
    pressure_warned: AtomicBool,
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
        }
    }

    /// Refresh RSS reading and update pressure state.
    pub fn refresh(&self) {
        let rss = Self::current_rss_bytes();
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
            // Use getrusage ru_maxrss (peak) as a conservative upper bound;
            // on macOS ru_maxrss is in bytes.
            let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
            let rc = unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) };
            if rc == 0 {
                let usage = unsafe { usage.assume_init() };
                return usage.ru_maxrss as u64;
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
