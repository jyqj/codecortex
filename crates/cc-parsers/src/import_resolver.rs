//! Import path resolution — maps import strings to file paths.
//!
//! Supports Node.js (./relative, bare module) and Python (dotted) resolution.

use std::path::Path;

/// Attempt to resolve an import string to a file path relative to the project root.
pub fn resolve_import(project_root: &Path, from_file: &str, import_str: &str) -> Option<String> {
    // Python dotted imports: "from foo.bar import baz" → foo/bar.py or foo/bar/__init__.py
    if !import_str.starts_with('.')
        && !import_str.starts_with('/')
        && !import_str.contains('/')
        && from_file.ends_with(".py")
    {
        return resolve_python_import(project_root, import_str);
    }

    // JS/TS relative imports: "./foo", "../bar"
    if import_str.starts_with('.') {
        return resolve_js_relative(project_root, from_file, import_str);
    }

    // Bare module import (node_modules) — not resolvable to a project file
    None
}

fn resolve_python_import(project_root: &Path, import_str: &str) -> Option<String> {
    // "import foo.bar.baz" or "from foo.bar import baz"
    // Try: extract module path from import statement text
    let module_path = if import_str.starts_with("from ") {
        import_str
            .strip_prefix("from ")?
            .split_whitespace()
            .next()?
    } else if import_str.starts_with("import ") {
        import_str
            .strip_prefix("import ")?
            .split_whitespace()
            .next()?
    } else {
        import_str
    };

    let parts: Vec<&str> = module_path.split('.').collect();
    let rel_path = parts.join("/");

    // Try: module.py
    let candidate = project_root.join(format!("{}.py", rel_path));
    if candidate.exists() {
        return Some(format!("{}.py", rel_path));
    }

    // Try: module/__init__.py
    let candidate = project_root.join(&rel_path).join("__init__.py");
    if candidate.exists() {
        return Some(format!("{}/__init__.py", rel_path));
    }

    None
}

fn resolve_js_relative(project_root: &Path, from_file: &str, import_str: &str) -> Option<String> {
    let from_dir = Path::new(from_file).parent()?;
    let target = from_dir.join(import_str);

    // Normalize path (resolve .. and .)
    let mut normalized = Vec::new();
    for component in target.components() {
        match component {
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            std::path::Component::CurDir => {}
            c => normalized.push(c.as_os_str().to_string_lossy().to_string()),
        }
    }
    let base = normalized.join("/");

    // Try extensions: .ts, .tsx, .js, .jsx, /index.ts, /index.js
    let extensions = [".ts", ".tsx", ".js", ".jsx", ".mjs"];
    for ext in &extensions {
        let candidate = format!("{}{}", base, ext);
        if project_root.join(&candidate).exists() {
            return Some(candidate);
        }
    }

    // Try index files
    for ext in &extensions {
        let candidate = format!("{}/index{}", base, ext);
        if project_root.join(&candidate).exists() {
            return Some(candidate);
        }
    }

    // Exact match (already has extension)
    if project_root.join(&base).exists() && project_root.join(&base).is_file() {
        return Some(base);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn resolve_python_module() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("foo")).unwrap();
        std::fs::write(root.join("foo/bar.py"), "").unwrap();

        let result = resolve_import(root, "main.py", "from foo.bar import baz");
        assert_eq!(result, Some("foo/bar.py".to_string()));
    }

    #[test]
    fn resolve_js_relative() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("src/utils")).unwrap();
        std::fs::write(root.join("src/utils/helper.ts"), "").unwrap();

        let result = resolve_import(root, "src/main.ts", "./utils/helper");
        assert_eq!(result, Some("src/utils/helper.ts".to_string()));
    }
}
