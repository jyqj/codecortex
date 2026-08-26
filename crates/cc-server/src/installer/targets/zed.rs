//! Zed installer target.

use std::path::{Path, PathBuf};

use cc_model::CcResult;

use crate::installer::helpers;
use crate::installer::InstallerTarget;

pub struct ZedTarget;

impl ZedTarget {
    fn settings_path(home: &Path) -> PathBuf {
        if cfg!(target_os = "macos") {
            home.join("Library/Application Support/Zed/settings.json")
        } else {
            home.join(".config/zed/settings.json")
        }
    }
}

impl InstallerTarget for ZedTarget {
    fn name(&self) -> &str {
        "Zed"
    }

    fn id(&self) -> &str {
        "zed"
    }

    fn detect(&self, home: &Path) -> bool {
        Self::settings_path(home).exists()
    }

    fn install(&self, home: &Path, binary_path: &Path, _force: bool) -> CcResult<Vec<String>> {
        let settings_path = Self::settings_path(home);
        helpers::upsert_json_key(
            &settings_path,
            "context_servers",
            "codecortex",
            &serde_json::json!({
                "command": binary_path.to_string_lossy(),
                "args": ["mcp"]
            }),
        )?;
        Ok(vec![])
    }

    fn uninstall(&self, home: &Path) -> CcResult<()> {
        helpers::remove_json_key(&Self::settings_path(home), "context_servers", "codecortex")
    }

    fn config_location(&self, home: &Path) -> PathBuf {
        Self::settings_path(home)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::installer::test_support as ts;

    #[test]
    fn detect_requires_existing_settings_file() {
        let home = ts::fake_home();
        assert!(!ZedTarget.detect(home.path()));
        let settings = ZedTarget.config_location(home.path());
        std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
        std::fs::write(&settings, "{}").unwrap();
        assert!(ZedTarget.detect(home.path()));
    }

    #[test]
    fn config_path_is_platform_specific() {
        let home = ts::fake_home();
        let expected = if cfg!(target_os = "macos") {
            home.path()
                .join("Library/Application Support/Zed/settings.json")
        } else {
            home.path().join(".config/zed/settings.json")
        };
        assert_eq!(ZedTarget.config_location(home.path()), expected);
    }

    #[test]
    fn install_writes_context_servers_entry() {
        let home = ts::fake_home();
        let binary = ts::temp_binary(home.path());
        ZedTarget.install(home.path(), &binary, false).unwrap();
        let root = ts::read_json(&ZedTarget.config_location(home.path()));
        assert_eq!(
            root["context_servers"]["codecortex"]["command"],
            binary.to_string_lossy().as_ref()
        );
        assert_eq!(root["context_servers"]["codecortex"]["args"][0], "mcp");
    }

    #[test]
    fn json_config_lifecycle() {
        ts::assert_json_config_lifecycle(&ZedTarget, "context_servers");
    }
}
