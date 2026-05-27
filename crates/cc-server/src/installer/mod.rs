//! Installer — multi-agent configuration injection and hook setup.
//!
//! Supports: Claude Code, Codex CLI, Cursor, Gemini CLI, opencode, Zed, VS Code.

mod helpers;
pub mod registry;
pub mod targets;

use std::path::{Path, PathBuf};

/// Trait implemented by each agent installer target.
#[allow(dead_code)]
pub trait InstallerTarget: Send + Sync {
    /// Human-readable name (e.g. "Claude Code").
    fn name(&self) -> &str;
    /// Machine identifier (e.g. "claude_code").
    fn id(&self) -> &str;
    /// Returns `true` if the agent appears to be installed on this system.
    fn detect(&self, home: &Path) -> bool;
    /// Install CodeCortex MCP configuration. Returns a list of hook descriptions.
    fn install(&self, home: &Path, binary_path: &Path, force: bool) -> Result<Vec<String>, String>;
    /// Remove CodeCortex configuration for this agent.
    fn uninstall(&self, home: &Path) -> Result<(), String>;
    /// Path to the primary config file this target writes to.
    fn config_location(&self, home: &Path) -> PathBuf;
}

/// Result of an install operation.
#[derive(Debug, Default)]
pub struct InstallReport {
    pub agents_configured: Vec<String>,
    pub hooks_installed: Vec<String>,
    pub errors: Vec<String>,
}

/// Install CodeCortex MCP configuration for all detected agents.
pub fn install_all(binary_path: &Path, force: bool) -> InstallReport {
    let home = dirs::home_dir().unwrap_or_default();
    let reg = registry::InstallerRegistry::default_registry();
    let mut report = InstallReport::default();

    for target in reg.targets() {
        if !force && !target.detect(&home) {
            continue;
        }
        match target.install(&home, binary_path, force) {
            Ok(hooks) => {
                report.agents_configured.push(target.name().to_string());
                report.hooks_installed.extend(hooks);
            }
            Err(e) => {
                report.errors.push(format!("{}: {}", target.name(), e));
            }
        }
    }

    report
}

/// Remove CodeCortex MCP configuration from all detected agents.
pub fn uninstall_all() -> InstallReport {
    let home = dirs::home_dir().unwrap_or_default();
    let reg = registry::InstallerRegistry::default_registry();
    let mut report = InstallReport::default();

    for target in reg.targets() {
        if !target.detect(&home) {
            continue;
        }
        match target.uninstall(&home) {
            Ok(()) => {
                report.agents_configured.push(target.name().to_string());
            }
            Err(e) => {
                report.errors.push(format!("{}: {}", target.name(), e));
            }
        }
    }

    report
}
