//! Cursor installer target.

use std::path::{Path, PathBuf};

use cc_model::CcResult;

use crate::installer::helpers;
use crate::installer::InstallerTarget;

pub struct CursorTarget;

impl CursorTarget {
    fn mcp_path(home: &Path) -> PathBuf {
        home.join(".cursor/mcp.json")
    }
}

impl InstallerTarget for CursorTarget {
    fn name(&self) -> &str {
        "Cursor"
    }
    fn id(&self) -> &str {
        "cursor"
    }

    fn detect(&self, home: &Path) -> bool {
        home.join(".cursor").exists()
            || home.join("Library/Application Support/Cursor").exists()
            || home.join(".config/Cursor").exists()
    }

    fn install(&self, home: &Path, binary_path: &Path, _force: bool) -> CcResult<Vec<String>> {
        let path = Self::mcp_path(home);
        helpers::upsert_json_key(
            &path,
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
    fn detect_matches_any_cursor_dir_variant() {
        let home = ts::fake_home();
        assert!(!CursorTarget.detect(home.path()));
        for dir in [
            ".cursor",
            "Library/Application Support/Cursor",
            ".config/Cursor",
        ] {
            let home = ts::fake_home();
            std::fs::create_dir_all(home.path().join(dir)).unwrap();
            assert!(CursorTarget.detect(home.path()), "not detected via {}", dir);
        }
    }

    #[test]
    fn config_lives_in_dot_cursor_mcp_json() {
        let home = ts::fake_home();
        assert_eq!(
            CursorTarget.config_location(home.path()),
            home.path().join(".cursor/mcp.json")
        );
    }

    #[test]
    fn install_writes_command_and_args() {
        let home = ts::fake_home();
        let binary = ts::temp_binary(home.path());
        CursorTarget.install(home.path(), &binary, false).unwrap();
        let root = ts::read_json(&home.path().join(".cursor/mcp.json"));
        assert_eq!(
            root["mcpServers"]["codecortex"]["command"],
            binary.to_string_lossy().as_ref()
        );
        assert_eq!(root["mcpServers"]["codecortex"]["args"][0], "mcp");
    }

    #[test]
    fn json_config_lifecycle() {
        ts::assert_json_config_lifecycle(&CursorTarget, "mcpServers");
    }
}
