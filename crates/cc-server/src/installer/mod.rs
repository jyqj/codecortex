//! Installer — multi-agent configuration injection and hook setup.
//!
//! Supports: Claude Code, Codex CLI, Cursor, Gemini CLI, opencode, Zed, VS Code.

mod helpers;
pub mod registry;
pub mod targets;

use std::path::{Path, PathBuf};

use cc_model::CcResult;

/// Trait implemented by each agent installer target.
pub trait InstallerTarget: Send + Sync {
    /// Human-readable name (e.g. "Claude Code").
    fn name(&self) -> &str;
    /// Machine identifier (e.g. "claude_code").
    fn id(&self) -> &str;
    /// Returns `true` if the agent appears to be installed on this system.
    fn detect(&self, home: &Path) -> bool;
    /// Install CodeCortex MCP configuration. Returns a list of hook descriptions.
    fn install(&self, home: &Path, binary_path: &Path, force: bool) -> CcResult<Vec<String>>;
    /// Remove CodeCortex configuration for this agent.
    fn uninstall(&self, home: &Path) -> CcResult<()>;
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
    install_all_in(&home, binary_path, force)
}

fn install_all_in(home: &Path, binary_path: &Path, force: bool) -> InstallReport {
    let reg = registry::InstallerRegistry::default_registry();
    let mut report = InstallReport::default();

    for target in reg.targets() {
        if !force && !target.detect(home) {
            continue;
        }
        tracing::debug!(
            agent_id = target.id(),
            config_location = %target.config_location(home).display(),
            "installer: applying target"
        );
        match target.install(home, binary_path, force) {
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
    uninstall_all_in(&home)
}

fn uninstall_all_in(home: &Path) -> InstallReport {
    let reg = registry::InstallerRegistry::default_registry();
    let mut report = InstallReport::default();

    for target in reg.targets() {
        if !target.detect(home) {
            continue;
        }
        tracing::debug!(
            agent_id = target.id(),
            config_location = %target.config_location(home).display(),
            "installer: removing target"
        );
        match target.uninstall(home) {
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

#[cfg(test)]
pub(crate) mod test_support {
    use std::path::{Path, PathBuf};

    use super::InstallerTarget;

    pub(crate) fn fake_home() -> tempfile::TempDir {
        tempfile::TempDir::new().unwrap()
    }

    /// Binary path handed to installs; never has to exist.
    pub(crate) fn temp_binary(home: &Path) -> PathBuf {
        home.join("bin/codecortex")
    }

    pub(crate) fn read_json(path: &Path) -> serde_json::Value {
        let content = std::fs::read_to_string(path).unwrap();
        serde_json::from_str(&content).unwrap()
    }

    pub(crate) fn write_json(path: &Path, value: &serde_json::Value) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, serde_json::to_string_pretty(value).unwrap()).unwrap();
    }

    /// Exercise the standard lifecycle of a JSON-config target whose MCP
    /// entries live under top-level `section`:
    /// (a) fresh install creates the config with a codecortex entry,
    /// (b) install merges into an existing config without clobbering it,
    /// (c) uninstall removes only the codecortex entry,
    /// (d) uninstall without a config present is a no-op.
    pub(crate) fn assert_json_config_lifecycle(target: &dyn InstallerTarget, section: &str) {
        // (a) fresh install
        let home = fake_home();
        let binary = temp_binary(home.path());
        target.install(home.path(), &binary, false).unwrap();
        let config = target.config_location(home.path());
        assert!(config.exists(), "{}: config not created", target.id());
        let root = read_json(&config);
        let entry = &root[section]["codecortex"];
        assert!(
            entry.is_object(),
            "{}: missing codecortex entry under {:?}",
            target.id(),
            section
        );
        assert!(
            serde_json::to_string(entry)
                .unwrap()
                .contains(binary.to_string_lossy().as_ref()),
            "{}: entry does not reference the binary path",
            target.id()
        );

        // Re-running install must succeed and keep the entry.
        target.install(home.path(), &binary, false).unwrap();
        assert!(read_json(&config)[section]["codecortex"].is_object());

        // (b) merge with a pre-existing user config
        let home = fake_home();
        let binary = temp_binary(home.path());
        let config = target.config_location(home.path());
        write_json(
            &config,
            &serde_json::json!({
                section: { "other-server": { "command": "other-bin" } },
                "user_setting": true,
            }),
        );
        target.install(home.path(), &binary, false).unwrap();
        let root = read_json(&config);
        assert_eq!(root[section]["other-server"]["command"], "other-bin");
        assert_eq!(root["user_setting"], true);
        assert!(root[section]["codecortex"].is_object());

        // (c) uninstall removes our entry, leaves the rest untouched
        target.uninstall(home.path()).unwrap();
        let root = read_json(&config);
        assert!(root[section].get("codecortex").is_none());
        assert_eq!(root[section]["other-server"]["command"], "other-bin");
        assert_eq!(root["user_setting"], true);

        // (d) uninstall on a home without any config is a no-op
        let home = fake_home();
        target.uninstall(home.path()).unwrap();
        assert!(!target.config_location(home.path()).exists());
    }
}

#[cfg(test)]
mod tests {
    use super::test_support as ts;
    use super::*;

    #[test]
    fn install_all_skips_undetected_targets() {
        let home = ts::fake_home();
        let binary = ts::temp_binary(home.path());
        let report = install_all_in(home.path(), &binary, false);
        assert!(report.agents_configured.is_empty());
        assert!(report.errors.is_empty());
    }

    #[test]
    fn force_install_configures_every_registered_target() {
        let home = ts::fake_home();
        let binary = ts::temp_binary(home.path());
        let report = install_all_in(home.path(), &binary, true);
        let reg = registry::InstallerRegistry::default_registry();
        assert_eq!(report.errors, Vec::<String>::new());
        assert_eq!(report.agents_configured.len(), reg.targets().len());
        for target in reg.targets() {
            assert!(
                target.config_location(home.path()).exists(),
                "{}: config missing after force install",
                target.id()
            );
        }
    }

    #[test]
    fn install_error_is_reported_under_target_name() {
        let home = ts::fake_home();
        let binary = ts::temp_binary(home.path());
        std::fs::create_dir_all(home.path().join(".cursor")).unwrap();
        std::fs::write(home.path().join(".cursor/mcp.json"), "{ not json").unwrap();
        let report = install_all_in(home.path(), &binary, false);
        assert!(report.agents_configured.is_empty());
        assert_eq!(report.errors.len(), 1);
        assert!(
            report.errors[0].starts_with("Cursor: "),
            "{}",
            report.errors[0]
        );
        assert!(
            report.errors[0].contains("mcp.json"),
            "{}",
            report.errors[0]
        );
    }

    #[test]
    fn uninstall_all_clears_codecortex_from_every_config() {
        let home = ts::fake_home();
        let binary = ts::temp_binary(home.path());
        install_all_in(home.path(), &binary, true);

        let report = uninstall_all_in(home.path());
        assert_eq!(report.errors, Vec::<String>::new());

        let reg = registry::InstallerRegistry::default_registry();
        for target in reg.targets() {
            let config = target.config_location(home.path());
            if config.exists() {
                let content = std::fs::read_to_string(&config).unwrap();
                assert!(
                    !content.contains("codecortex"),
                    "{}: codecortex entry survived uninstall",
                    target.id()
                );
            }
        }
    }
}
