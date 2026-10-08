//! Fail-closed validation for benchmark input, before any indexing or network I/O.
use super::{invalid, schema::*, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};
pub fn relative_path(path: &str) -> Result<()> {
    if path.is_empty()
        || path.contains('\\')
        || path.contains(':')
        || path.chars().any(char::is_control)
        || Path::new(path)
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
        || path
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == "..")
    {
        return Err(invalid(format!("non-canonical relative path: {path:?}")));
    }
    Ok(())
}
pub fn queries(rows: &[Query], profile: ScoreProfile) -> Result<()> {
    if rows.is_empty() {
        return Err(invalid("empty corpus"));
    }
    let mut ids = BTreeSet::new();
    let mut families = BTreeMap::new();
    for q in rows {
        if q.id.is_empty() || !ids.insert(&q.id) {
            return Err(invalid("empty/duplicate query id"));
        }
        if !(1..=3).contains(&q.difficulty)
            || q.query.trim().is_empty()
            || q.query.len() > 4096
            || q.query_family.is_empty()
            || q.category.is_empty()
            || q.language.is_empty()
            || !["dev", "holdout", "quarantine"].contains(&q.split.as_str())
        {
            return Err(invalid(format!("invalid query fields: {}", q.id)));
        }
        if let Some(old) = families.insert(&q.query_family, &q.split) {
            if old != &q.split {
                return Err(invalid("query family crosses splits"));
            }
        }
        if let Some(p) = &q.path_prefix {
            relative_path(p.trim_end_matches('/'))?;
        }
        if q.no_answer {
            if profile == ScoreProfile::OceCompat
                || !q.answers.is_empty()
                || !q.expected_files.is_empty()
            {
                return Err(invalid("no-answer must have empty native gold"));
            }
            continue;
        }
        if profile == ScoreProfile::OceCompat {
            if q.expected_files.is_empty() || !q.answers.is_empty() {
                return Err(invalid("compat requires expected_files only"));
            }
            let mut unique = BTreeSet::new();
            for p in &q.expected_files {
                relative_path(p)?;
                super::metrics::path_matches("probe", p)?;
                if !unique.insert(p) {
                    return Err(invalid("duplicate expected path"));
                }
            }
        } else {
            if q.answers.is_empty()
                || !q.expected_files.is_empty()
                || !q.answers.iter().any(|a| a.primary)
            {
                return Err(invalid("native requires primary answer groups"));
            }
            let mut groups = BTreeSet::new();
            for g in &q.answers {
                if g.id.is_empty()
                    || !groups.insert(&g.id)
                    || g.alternatives.is_empty()
                    || !(1..=3).contains(&g.grade)
                {
                    return Err(invalid("invalid answer group"));
                }
                for a in &g.alternatives {
                    relative_path(&a.path)?;
                    if a.path.contains(['*', '?', '[']) {
                        return Err(invalid("native needs explicit alternatives"));
                    }
                    if a.span.as_ref().is_some_and(|s| s.start >= s.end) {
                        return Err(invalid("invalid byte span"));
                    }
                    if a.symbol.as_ref().is_some_and(|s| s.name.is_empty()) {
                        return Err(invalid("empty symbol"));
                    }
                }
            }
        }
    }
    Ok(())
}
