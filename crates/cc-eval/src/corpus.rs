use crate::types::EvalCase;
use std::path::Path;

pub fn load_corpus(dir: &Path) -> Result<Vec<EvalCase>, String> {
    let mut cases = Vec::new();
    if !dir.exists() {
        return Err(format!("corpus directory not found: {}", dir.display()));
    }
    for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.extension().map_or(false, |e| e == "toml") {
            let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
            let case: EvalCase =
                toml::from_str(&content).map_err(|e| format!("{}: {}", path.display(), e))?;
            cases.push(case);
        }
    }
    cases.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(cases)
}
