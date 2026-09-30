//! Bounded Go manifest lexer/projection. Never invokes go, downloads or user code.
use cc_model::{go_project::*, module_inputs::ConfigDocument, project_model::*, CcError, CcResult};
use std::collections::BTreeMap;
fn tokens(line: &str) -> Result<Vec<String>, String> {
    let mut out = vec![];
    let mut it = line.char_indices().peekable();
    while let Some((start, c)) = it.next() {
        if c.is_whitespace() {
            continue;
        }
        if c == '/' && it.peek().is_some_and(|(_, c)| *c == '/') {
            break;
        }
        if matches!(c, '(' | ')') {
            out.push(c.to_string());
            continue;
        }
        if matches!(c, '"' | '`') {
            let mut end = None;
            let mut escape = false;
            for (i, ch) in it.by_ref() {
                if c == '"' && ch == '\\' && !escape {
                    escape = true;
                    continue;
                }
                if ch == c && !escape {
                    end = Some(i + ch.len_utf8());
                    break;
                }
                escape = false;
            }
            let end = end.ok_or("go_unterminated_string")?;
            let raw = &line[start..end];
            out.push(if c == '"' {
                serde_json::from_str(raw).map_err(|_| "go_string_escape_unsupported")?
            } else {
                raw[1..raw.len() - 1].into()
            });
        } else {
            let mut end = start + c.len_utf8();
            while let Some(&(i, ch)) = it.peek() {
                if ch.is_whitespace() || matches!(ch, '(' | ')') {
                    break;
                }
                end = i + ch.len_utf8();
                it.next();
            }
            out.push(line[start..end].into());
        }
        if out.len() > 32 {
            return Err("go_directive_token_limit".into());
        }
    }
    Ok(out)
}
pub(super) fn parse(bytes: &[u8], work: bool) -> CcResult<ConfigDocument> {
    let text =
        std::str::from_utf8(bytes).map_err(|_| CcError::Config("go_config_not_utf8".into()))?;
    let mut c = GoConfig::default();
    let mut block: Option<String> = None;
    for (line_number, line) in text.lines().enumerate() {
        if line_number > 16384 {
            return Err(CcError::Config("go_config_line_limit".into()));
        }
        let mut ts = match tokens(line) {
            Ok(t) => t,
            Err(e) => {
                c.diagnostics.push(e);
                continue;
            }
        };
        if ts.is_empty() {
            continue;
        }
        if ts == [")"] {
            if block.take().is_none() {
                c.diagnostics.push("go_unmatched_block_end".into());
            }
            continue;
        }
        let directive = if let Some(b) = &block {
            b.clone()
        } else {
            ts.remove(0)
        };
        if ts == ["("] {
            if block.is_some() || !matches!(directive.as_str(), "require" | "replace" | "use") {
                c.diagnostics.push("go_invalid_block".into());
            } else {
                block = Some(directive);
            }
            continue;
        }
        let ok = match directive.as_str() {
            "module" if !work && ts.len() == 1 => {
                if c.module.replace(ts[0].clone()).is_some() {
                    c.diagnostics.push("go_duplicate_module".into());
                }
                true
            }
            "go" | "toolchain" if ts.len() == 1 => true,
            "require" if !work && ts.len() == 2 => {
                if c.requires.insert(ts[0].clone(), ts[1].clone()).is_some() {
                    c.diagnostics.push("go_duplicate_require".into());
                }
                true
            }
            "use" if work && ts.len() == 1 => {
                c.uses.push(ts[0].clone());
                true
            }
            "replace" => {
                if let Some(i) = ts.iter().position(|s| s == "=>") {
                    let left = &ts[..i];
                    let right = &ts[i + 1..];
                    if (1..=2).contains(&left.len()) && (1..=2).contains(&right.len()) {
                        let v = GoReplace {
                            module: left[0].clone(),
                            version: left.get(1).cloned(),
                            target: right[0].clone(),
                            target_version: right.get(1).cloned(),
                        };
                        if c.replaces
                            .iter()
                            .any(|r| r.module == v.module && r.version == v.version)
                        {
                            c.diagnostics.push("go_duplicate_replace".into());
                        }
                        c.replaces.push(v);
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            _ => false,
        };
        if !ok {
            c.diagnostics
                .push(format!("go_directive_unsupported:{directive}"));
        }
        if c.requires.len() + c.uses.len() + c.replaces.len() > 4096 {
            return Err(CcError::Config("go_config_entry_limit".into()));
        }
    }
    if block.is_some() {
        c.diagnostics.push("go_unclosed_block".into());
    }
    if !work && c.module.as_deref().is_none_or(|s| s.is_empty()) {
        c.diagnostics.push("go_missing_module".into());
    }
    c.diagnostics.sort();
    c.diagnostics.dedup();
    Ok(ConfigDocument::Go(c))
}
pub(super) fn build(
    documents: &BTreeMap<String, ConfigDocument>,
    sources: &BTreeMap<String, GoSourceInput>,
) -> GoProject {
    let mut out = GoProject::default();
    for (p, d) in documents
        .iter()
        .filter(|(p, _)| matches!(p.rsplit('/').next(), Some("go.mod" | "go.work")))
    {
        let c = match d {
            ConfigDocument::Go(c) => c.clone(),
            _ => GoConfig {
                diagnostics: vec!["invalid_go_config".into()],
                ..Default::default()
            },
        };
        out.configs.insert(p.clone(), c);
    }
    for (p, s) in sources {
        out.sources.insert(p.clone(), s.facts.clone());
        let leaf = p.rsplit('/').next().unwrap_or(p);
        if !leaf.starts_with(['.', '_']) && !p.split('/').any(|s| s == "testdata") {
            out.directories
                .entry(parent(p).into())
                .or_default()
                .push(p.clone());
        }
    }
    out
}
