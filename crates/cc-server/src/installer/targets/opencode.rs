//! opencode installer target.

use std::path::{Path, PathBuf};

use cc_model::CcResult;

use crate::installer::helpers;
use crate::installer::InstallerTarget;

pub struct OpenCodeTarget;

impl OpenCodeTarget {
    fn config_path(home: &Path) -> PathBuf {
        home.join(".config/opencode/opencode.json")
    }
}

impl InstallerTarget for OpenCodeTarget {
    fn name(&self) -> &str {
        "opencode"
    }
    fn id(&self) -> &str {
        "opencode"
    }

    fn detect(&self, home: &Path) -> bool {
        home.join(".config/opencode").exists() || home.join(".opencode").exists()
    }

    fn install(&self, home: &Path, binary_path: &Path, _force: bool) -> CcResult<Vec<String>> {
        let path = Self::config_path(home);
        helpers::upsert_json_key(
            &path,
            "mcp",
            "codecortex",
            &serde_json::json!({
                "type": "local",
                "command": [binary_path.to_string_lossy(), "mcp"]
            }),
        )?;
        Ok(vec![])
    }

    fn uninstall(&self, home: &Path) -> CcResult<()> {
        helpers::remove_json_key(&Self::config_path(home), "mcp", "codecortex")
    }

    fn config_location(&self, home: &Path) -> PathBuf {
        Self::config_path(home)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::installer::test_support as ts;

    #[test]
    fn detect_matches_either_opencode_dir() {
        let home = ts::fake_home();
        assert!(!OpenCodeTarget.detect(home.path()));
        for dir in [".config/opencode", ".opencode"] {
            let home = ts::fake_home();
            std::fs::create_dir_all(home.path().join(dir)).unwrap();
            assert!(
                OpenCodeTarget.detect(home.path()),
                "not detected via {}",
                dir
            );
        }
    }

    #[test]
    fn install_writes_local_command_entry_under_mcp_section() {
        let home = ts::fake_home();
        let binary = ts::temp_binary(home.path());
        OpenCodeTarget.install(home.path(), &binary, false).unwrap();
        let root = ts::read_json(&home.path().join(".config/opencode/opencode.json"));
        let entry = &root["mcp"]["codecortex"];
        assert_eq!(entry["type"], "local");
        assert_eq!(entry["command"][0], binary.to_string_lossy().as_ref());
        assert_eq!(entry["command"][1], "mcp");
    }

    #[test]
    fn json_config_lifecycle() {
        ts::assert_json_config_lifecycle(&OpenCodeTarget, "mcp");
    }
}
