//! Gemini CLI installer target.

use std::path::{Path, PathBuf};

use cc_model::CcResult;

use crate::installer::helpers;
use crate::installer::InstallerTarget;

pub struct GeminiCliTarget;

impl InstallerTarget for GeminiCliTarget {
    fn name(&self) -> &str {
        "Gemini CLI"
    }

    fn id(&self) -> &str {
        "gemini_cli"
    }

    fn detect(&self, home: &Path) -> bool {
        home.join(".gemini").exists()
    }

    fn install(&self, home: &Path, binary_path: &Path, _force: bool) -> CcResult<Vec<String>> {
        let settings_path = home.join(".gemini/settings.json");
        helpers::upsert_json_key(
            &settings_path,
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
        let settings_path = home.join(".gemini/settings.json");
        helpers::remove_json_key(&settings_path, "mcpServers", "codecortex")
    }

    fn config_location(&self, home: &Path) -> PathBuf {
        home.join(".gemini/settings.json")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::installer::test_support as ts;

    #[test]
    fn detect_requires_gemini_dir() {
        let home = ts::fake_home();
        assert!(!GeminiCliTarget.detect(home.path()));
        std::fs::create_dir_all(home.path().join(".gemini")).unwrap();
        assert!(GeminiCliTarget.detect(home.path()));
    }

    #[test]
    fn install_writes_settings_json_under_gemini_dir() {
        let home = ts::fake_home();
        let binary = ts::temp_binary(home.path());
        GeminiCliTarget
            .install(home.path(), &binary, false)
            .unwrap();
        let root = ts::read_json(&home.path().join(".gemini/settings.json"));
        assert_eq!(
            root["mcpServers"]["codecortex"]["command"],
            binary.to_string_lossy().as_ref()
        );
        assert_eq!(root["mcpServers"]["codecortex"]["args"][0], "mcp");
    }

    #[test]
    fn json_config_lifecycle() {
        ts::assert_json_config_lifecycle(&GeminiCliTarget, "mcpServers");
    }
}
