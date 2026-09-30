//! Public-query freshness at the dispatch seam, independent of result caches.
use super::SharedCodeIndex;
use cc_model::{freshness::ResolutionFreshness, CcError, CcResult};
use serde_json::Value;

pub fn capture(runtime: &SharedCodeIndex, tool: &str) -> CcResult<Option<ResolutionFreshness>> {
    if !matches!(
        tool,
        "status"
            | "search"
            | "context"
            | "explore"
            | "trace"
            | "graph_query"
            | "node"
            | "relations"
            | "impact"
            | "architecture"
    ) {
        return Ok(None);
    }
    let rt = super::lock_index(runtime)?;
    rt.index_db()
        .map(|db| db.reads().resolution_freshness())
        .transpose()
}

pub fn attach(
    runtime: &SharedCodeIndex,
    tool: &str,
    before: Option<ResolutionFreshness>,
    value: Value,
) -> CcResult<Value> {
    let Some(after) = capture(runtime, tool)? else {
        return Ok(value);
    };
    attach_observed(before, after, value)
}

/// Shared schema projection for locked graph handlers and Arc-owned async queries.
/// The latter read both observations from the captured database, never whichever
/// project happens to be active after awaiting an optional port.
pub(crate) fn attach_observed(
    before: Option<ResolutionFreshness>,
    mut after: ResolutionFreshness,
    mut value: Value,
) -> CcResult<Value> {
    match before {
        Some(before) if before.index_epoch == after.index_epoch => {}
        _ => {
            after.complete = false;
            after.status = "changed_during_query".into();
            after.reason = Some("index_generation_changed_during_query".into());
            after.retry = Some("retry the query against a stable index generation".into());
        }
    }
    if let Some(object) = value.as_object_mut() {
        object.insert("resolution_freshness".into(), serde_json::to_value(after)?);
    } else if !after.complete {
        // Do not silently change legacy array schemas, or silently serve old
        // relationship facts when that response has no place for a warning.
        return Err(CcError::Other(format!(
            "resolution incomplete: {}; {}",
            after.reason.as_deref().unwrap_or("pending"),
            after.retry.as_deref().unwrap_or("retry index")
        )));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn generation_change_is_never_reported_ready() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.rs"), "pub fn a() {}\n").unwrap();
        let index = std::sync::Arc::new(std::sync::RwLock::new(
            crate::engine::CodeIndex::new(Some(d.path())).unwrap(),
        ));
        index.write().unwrap().build_index(true).unwrap();
        let before = capture(&index, "graph_query").unwrap();
        std::fs::write(d.path().join("b.rs"), "pub fn b() {}\n").unwrap();
        index.write().unwrap().build_index(false).unwrap();
        let result = attach(
            &index,
            "graph_query",
            before,
            serde_json::json!({"results":[]}),
        )
        .unwrap();
        assert_eq!(result["resolution_freshness"]["complete"], false);
        assert_eq!(
            result["resolution_freshness"]["status"],
            "changed_during_query"
        );
    }
}
