//! Installer registry — collects all known targets.

use crate::installer::targets;
use crate::installer::InstallerTarget;

pub struct InstallerRegistry {
    targets: Vec<Box<dyn InstallerTarget>>,
}

impl InstallerRegistry {
    /// Create a registry populated with all built-in targets.
    pub fn default_registry() -> Self {
        let targets: Vec<Box<dyn InstallerTarget>> = vec![
            Box::new(targets::claude_code::ClaudeCodeTarget),
            Box::new(targets::codex_cli::CodexCliTarget),
            Box::new(targets::cursor::CursorTarget),
            Box::new(targets::gemini_cli::GeminiCliTarget),
            Box::new(targets::opencode::OpenCodeTarget),
            Box::new(targets::vscode::VsCodeTarget),
            Box::new(targets::zed::ZedTarget),
        ];
        Self { targets }
    }

    /// Iterate over registered targets.
    pub fn targets(&self) -> &[Box<dyn InstallerTarget>] {
        &self.targets
    }
}
