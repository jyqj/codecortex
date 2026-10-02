//! Public-query freshness at the dispatch seam, independent of result caches.
use super::SharedCodeIndex;
use cc_model::{freshness::ResolutionFreshness, CcError, CcResult};
use serde_json::Value;

/// Extract the fence-accepted read generation embedded in a finished search
/// envelope (`evidence_summary/source_freshness/generation`, written by the
/// hydrator from the outer fence's accepted generation). `None` when the
/// response shape carries no accepted generation; annotation then stays
/// strictly conservative.
pub(crate) fn accepted_generation_of(
    value: &Value,
) -> Option<cc_model::generation::ReadGeneration> {
    value
        .pointer("/evidence_summary/source_freshness/generation")
        .and_then(|generation| serde_json::from_value(generation.clone()).ok())
}

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
    let accepted = accepted_generation_of(&value);
    attach_observed(before, after, accepted, value)
}

/// Shared schema projection for locked graph handlers and Arc-owned async queries.
/// The latter read both observations from the captured database, never whichever
/// project happens to be active after awaiting an optional port.
///
/// `accepted` is the query's own fence-accepted read generation, when the
/// response carries one. A dispatch-window generation change whose post-change
/// epoch equals the accepted epoch is a fresh single-generation result: the
/// change is recorded as an explicit observation instead of flagging the fresh
/// result incomplete. Every other change path keeps the strict
/// `changed_during_query` annotation — true staleness is never washed clean.
pub(crate) fn attach_observed(
    before: Option<ResolutionFreshness>,
    mut after: ResolutionFreshness,
    accepted: Option<cc_model::generation::ReadGeneration>,
    mut value: Value,
) -> CcResult<Value> {
    // A dispatch-window generation change whose post-change index epoch
    // equals the query's fence-accepted epoch means the result itself is
    // fresh and single-generation self-consistent (the fence recovered onto
    // the new generation). Record the change as an explicit observation
    // instead of flagging the fresh result incomplete. Comparison uses the
    // index epoch, the same granularity as the dispatch-window comparison
    // below. Every other change path keeps the strict annotation — true
    // staleness is never washed clean.
    let refined_from = match (&before, accepted) {
        (Some(before), Some(accepted))
            if before.index_epoch != after.index_epoch
                && accepted.index_epoch == after.index_epoch =>
        {
            Some(before.index_epoch)
        }
        _ => None,
    };
    if refined_from.is_none()
        && matches!(&before, Some(before) if before.index_epoch != after.index_epoch)
    {
        after.complete = false;
        after.status = "changed_during_query".into();
        after.reason = Some("index_generation_changed_during_query".into());
        after.retry = Some("retry the query against a stable index generation".into());
    }
    if let Some(object) = value.as_object_mut() {
        let after_epoch = after.index_epoch;
        let mut freshness = serde_json::to_value(after)?;
        if let (Some(from_index_epoch), Some(freshness_object)) =
            (refined_from, freshness.as_object_mut())
        {
            freshness_object.insert(
                "dispatch_observed_generation_change".into(),
                serde_json::json!({
                    "from_index_epoch": from_index_epoch,
                    "to_index_epoch": after_epoch,
                }),
            );
        }
        object.insert("resolution_freshness".into(), freshness);
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

    fn freshness_at(index_epoch: u64) -> ResolutionFreshness {
        ResolutionFreshness::ready(index_epoch)
    }

    fn generation_at(index_epoch: u64) -> cc_model::generation::ReadGeneration {
        cc_model::generation::ReadGeneration {
            incarnation: [7; 16],
            index_epoch,
            evidence_epoch: 0,
            semantic_epoch: None,
        }
    }

    // P2: a dispatch-window generation change whose post-change epoch equals
    // the fence-accepted generation is a fresh, self-consistent result. It
    // must stay complete and carry the explicit observation instead of the
    // incomplete changed_during_query flag.
    #[test]
    fn accepted_generation_matching_current_is_not_flagged_incomplete() {
        let value = attach_observed(
            Some(freshness_at(1)),
            freshness_at(2),
            Some(generation_at(2)),
            serde_json::json!({"hits": []}),
        )
        .unwrap();
        assert_eq!(value["resolution_freshness"]["complete"], true);
        assert_eq!(value["resolution_freshness"]["status"], "ready");
        assert_eq!(
            value["resolution_freshness"]["dispatch_observed_generation_change"]
                ["from_index_epoch"],
            1
        );
        assert_eq!(
            value["resolution_freshness"]["dispatch_observed_generation_change"]
                ["to_index_epoch"],
            2
        );
    }

    // P2 guard: when the accepted generation is older than the current one,
    // the result is genuinely stale — the strict annotation must survive.
    #[test]
    fn true_stale_accepted_generation_still_marks_changed_during_query() {
        let value = attach_observed(
            Some(freshness_at(1)),
            freshness_at(2),
            Some(generation_at(1)),
            serde_json::json!({"hits": []}),
        )
        .unwrap();
        assert_eq!(value["resolution_freshness"]["complete"], false);
        assert_eq!(
            value["resolution_freshness"]["status"],
            "changed_during_query"
        );
        assert!(value["resolution_freshness"]["dispatch_observed_generation_change"]
            .is_null());
    }

    // P2 guard: without an accepted generation the annotation must stay
    // strictly conservative — no path may wash a change into complete=true.
    #[test]
    fn missing_accepted_generation_keeps_strict_annotation() {
        let value = attach_observed(
            Some(freshness_at(1)),
            freshness_at(2),
            None,
            serde_json::json!({"hits": []}),
        )
        .unwrap();
        assert_eq!(value["resolution_freshness"]["complete"], false);
        assert_eq!(
            value["resolution_freshness"]["status"],
            "changed_during_query"
        );
    }

    #[test]
    fn accepted_generation_of_reads_envelope_pointer() {
        let generation = generation_at(9);
        let value = serde_json::json!({
            "evidence_summary": {
                "source_freshness": {
                    "generation": serde_json::to_value(generation).unwrap(),
                },
            },
        });
        assert_eq!(accepted_generation_of(&value), Some(generation));
        assert_eq!(accepted_generation_of(&serde_json::json!({"hits": []})), None);
    }
}
