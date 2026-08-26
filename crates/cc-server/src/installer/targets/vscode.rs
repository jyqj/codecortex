//! VS Code installer target.

use std::path::{Path, PathBuf};

use cc_model::CcResult;

use crate::installer::helpers;
use crate::installer::InstallerTarget;

pub struct VsCodeTarget;

impl VsCodeTarget {
    fn mcp_path(home: &Path) -> PathBuf {
        if cfg!(target_os = "macos") {
            home.join("Library/Application Support/Code/User/mcp.json")
        } else {
            home.join(".config/Code/User/mcp.json")
        }
    }
}

impl InstallerTarget for VsCodeTarget {
    fn name(&self) -> &str {
        "VS Code"
    }

    fn id(&self) -> &str {
        "vscode"
    }

    fn detect(&self, home: &Path) -> bool {
        let mcp_path = Self::mcp_path(home);
        mcp_path.parent().map(|p| p.exists()).unwrap_or(false)
    }

    fn install(&self, home: &Path, binary_path: &Path, _force: bool) -> CcResult<Vec<String>> {
        let mcp_path = Self::mcp_path(home);
        helpers::upsert_json_key(
            &mcp_path,
            "mcpServers",
            "codecortex",
            &serde_json::json!({
                "command": binary_path.to_string_lossy(),
                "args": ["mcp"]
            }),
        )?;
        Ok(vec![])
    }

    fn uninstall(&self, home: &Path) -> CcResult<()> {
        helpers::remove_json_key(&Self::mcp_path(home), "mcpServers", "codecortex")
    }

    fn config_location(&self, home: &Path) -> PathBuf {
        Self::mcp_path(home)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::installer::test_support as ts;

    #[test]
    fn detect_requires_user_config_dir() {
        let home = ts::fake_home();
        assert!(!VsCodeTarget.detect(home.path()));
        let user_dir = VsCodeTarget.config_location(home.path());
        std::fs::create_dir_all(user_dir.parent().unwrap()).unwrap();
        assert!(VsCodeTarget.detect(home.path()));
    }

    #[test]
    fn config_path_is_platform_specific() {
        let home = ts::fake_home();
        let expected = if cfg!(target_os = "macos") {
            home.path()
                .join("Library/Application Support/Code/User/mcp.json")
        } else {
            home.path().join(".config/Code/User/mcp.json")
        };
        assert_eq!(VsCodeTarget.config_location(home.path()), expected);
    }

    #[test]
    fn install_writes_command_and_args() {
        let home = ts::fake_home();
        let binary = ts::temp_binary(home.path());
        VsCodeTarget.install(home.path(), &binary, false).unwrap();
        let root = ts::read_json(&VsCodeTarget.config_location(home.path()));
        assert_eq!(
            root["mcpServers"]["codecortex"]["command"],
            binary.to_string_lossy().as_ref()
        );
        assert_eq!(root["mcpServers"]["codecortex"]["args"][0], "mcp");
    }

    #[test]
    fn json_config_lifecycle() {
        ts::assert_json_config_lifecycle(&VsCodeTarget, "mcpServers");
    }
}
