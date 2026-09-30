//! Only documented public output is normalized. Never infer a symbol from its title.
use super::{schema::*, validation, BenchError, Result};
use serde_json::Value;
use std::path::Path;
fn text(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(Value::as_str).map(str::to_owned)
}
/// Older binaries omit lane receipts. New receipts must satisfy the versioned
/// contract: malformed/unknown states are protocol failures, never no-match.
fn lanes_partial(payload: &Value) -> Result<bool> {
    use cc_model::retrieval::{LaneOutcome, LaneStatus};
    if let Some(raw) = payload.pointer("/evidence_summary/retrieval/lane_receipts") {
        if payload
            .pointer("/evidence_summary/retrieval/lanes")
            .is_some()
        {
            return Err(BenchError::Protocol(
                "both full and projected lane receipts".into(),
            ));
        }
        let receipts: Vec<cc_model::lane_receipt::LaneReceipt> =
            serde_json::from_value(raw.clone()).map_err(|error| {
                BenchError::Protocol(format!("invalid projected lane receipts: {error}"))
            })?;
        let mut ids = std::collections::HashSet::new();
        let mut partial = false;
        for receipt in receipts {
            receipt.validate().map_err(|error| {
                BenchError::Protocol(format!("invalid projected lane receipt: {error}"))
            })?;
            if !ids.insert(receipt.lane_id) {
                return Err(BenchError::Protocol(
                    "duplicate projected lane receipt".into(),
                ));
            }
            partial |= matches!(
                receipt.status,
                LaneStatus::Partial
                    | LaneStatus::Timeout
                    | LaneStatus::Unavailable
                    | LaneStatus::Error
                    | LaneStatus::Cancelled
            );
        }
        return Ok(partial);
    }
    let Some(raw) = payload.pointer("/evidence_summary/retrieval/lanes") else {
        return Ok(false);
    };
    let lanes: Vec<LaneOutcome> = serde_json::from_value(raw.clone())
        .map_err(|error| BenchError::Protocol(format!("invalid lane receipts: {error}")))?;
    let mut ids = std::collections::HashSet::new();
    let mut partial = false;
    for lane in lanes {
        lane.validate()
            .map_err(|error| BenchError::Protocol(format!("invalid lane receipt: {error}")))?;
        if !ids.insert(lane.lane_id) {
            return Err(BenchError::Protocol("duplicate lane receipt".into()));
        }
        partial |= matches!(
            lane.status,
            LaneStatus::Partial
                | LaneStatus::Timeout
                | LaneStatus::Unavailable
                | LaneStatus::Error
                | LaneStatus::Cancelled
        );
    }
    Ok(partial)
}

pub fn mcp(payload: &Value) -> Result<(Vec<Hit>, ResultStatus)> {
    if payload.get("_truncated").and_then(Value::as_bool) == Some(true) {
        return Ok((vec![], ResultStatus::Partial));
    }
    let machine = payload.pointer("/machine_pack/hits");
    let indexed = machine.is_some();
    let (nodes, symbols) = if let Some(a) = payload.as_array() {
        (a, true)
    } else if let Some(v) = machine {
        (
            v.as_array()
                .ok_or_else(|| BenchError::Protocol("machine_pack.hits is not an array".into()))?,
            false,
        )
    } else {
        (
            payload
                .get("nodes")
                .and_then(Value::as_array)
                .ok_or_else(|| BenchError::Protocol("search response has no nodes array".into()))?,
            false,
        )
    };
    let mut hits = Vec::new();
    for n in nodes {
        if !n.is_object() {
            return Err(BenchError::Protocol("non-object search node".into()));
        }
        if !symbols && !indexed {
            let kind = n
                .get("node_type")
                .and_then(Value::as_str)
                .ok_or_else(|| BenchError::Protocol("missing node_type".into()))?;
            if kind != "SEARCH_HIT" {
                continue;
            }
        }
        let path =
            text(n, "file_path").ok_or_else(|| BenchError::Protocol("missing hit path".into()))?;
        validation::relative_path(&path)?;
        let meta = n.get("metadata").unwrap_or(&Value::Null);
        let number = |key: &str| -> Result<Option<u32>> {
            match n.get(key) {
                Some(Value::Null) | None => Ok(None),
                Some(v) => v
                    .as_u64()
                    .and_then(|x| u32::try_from(x).ok())
                    .map(Some)
                    .ok_or_else(|| BenchError::Protocol(format!("invalid {key}"))),
            }
        };
        hits.push(Hit {
            source_evidence: n
                .get("source_evidence")
                .or_else(|| meta.get("source_evidence"))
                .filter(|v| !v.is_null())
                .cloned(),
            path,
            symbol_name: if symbols {
                text(n, "name")
            } else {
                text(n, "symbol_name").or_else(|| text(meta, "symbol_name"))
            },
            qname: text(n, "qname").or_else(|| text(meta, "qname")),
            kind: text(n, "kind")
                .or_else(|| text(n, "symbol_kind"))
                .or_else(|| text(meta, "symbol_kind")),
            start_line: number("start_line")?,
            end_line: number("end_line")?,
            span: None,
            text: text(n, "text"),
            evidence_valid: None,
        });
    }
    let lane_partial = lanes_partial(payload)?;
    let grep = payload.pointer("/evidence_summary/retrieval/grep/status");
    let grep_partial = match grep.and_then(Value::as_str) {
        None if grep.is_none() || grep == Some(&Value::Null) => false,
        Some("complete" | "limited" | "disabled") => false,
        Some("partial") => true,
        _ => return Err(BenchError::Protocol("unknown grep coverage status".into())),
    };
    let graph_partial = [
        "/evidence_summary/graph_explain",
        "/evidence_summary/graph_enrichment/graph_explain",
    ]
    .iter()
    .any(|root| {
        payload
            .pointer(&format!("{root}/truncated"))
            .and_then(Value::as_bool)
            == Some(true)
            || payload
                .pointer(&format!("{root}/read_errors"))
                .and_then(Value::as_array)
                .is_some_and(|a| !a.is_empty())
    }) || grep_partial
        || lane_partial
        || payload
            .pointer("/evidence_summary/source_freshness/partial")
            .and_then(Value::as_bool)
            == Some(true);
    let packing_partial = match payload.pointer("/evidence_summary/packing") {
        None => false,
        Some(packing) => {
            if packing.get("spec").and_then(Value::as_str)
                != Some(cc_model::context::CONTEXT_PACKING_SPEC)
            {
                return Err(BenchError::Protocol("invalid context packing spec".into()));
            }
            packing
                .get("partial")
                .and_then(Value::as_bool)
                .ok_or_else(|| BenchError::Protocol("invalid context packing status".into()))?
        }
    };
    let status = if graph_partial || packing_partial {
        ResultStatus::Partial
    } else if hits.is_empty() {
        ResultStatus::NoMatch
    } else {
        ResultStatus::Success
    };
    Ok((hits, status))
}
pub fn oce(payload: &Value) -> Result<(Vec<Hit>, ResultStatus)> {
    let formatted = payload
        .get("formatted_retrieval")
        .and_then(Value::as_str)
        .ok_or_else(|| BenchError::Protocol("missing formatted_retrieval".into()))?;
    let mut seen = std::collections::BTreeSet::new();
    let mut hits = Vec::new();
    for line in formatted.lines() {
        if let Some(p) = line.strip_prefix("Path: ") {
            let p = p.trim();
            validation::relative_path(p)?;
            if seen.insert(p.to_string()) {
                hits.push(Hit::path_only(p.to_string()));
            }
        }
    }
    if !formatted.trim().is_empty() && hits.is_empty() {
        return Err(BenchError::Protocol(
            "nonempty formatted response contains no Path headers; not counted as no-match".into(),
        ));
    }
    let status = if hits.is_empty() {
        ResultStatus::NoMatch
    } else {
        ResultStatus::Success
    };
    Ok((hits, status))
}
pub fn verify_source(hit: &mut Hit, root: &Path) -> Result<()> {
    let bytes = match super::manifest::source_bytes(root, &hit.path) {
        Ok(b) => b,
        Err(_) => {
            hit.evidence_valid = Some(false);
            return Ok(());
        }
    };
    let Some(ref text) = hit.text else {
        if hit.source_evidence.is_some() {
            hit.evidence_valid = Some(false);
        }
        return Ok(());
    };
    if let Some(raw) = &hit.source_evidence {
        // New byte proof is authoritative. Corrupt/mismatching proof may never
        // fall through to the legacy line-normalization compatibility path.
        hit.evidence_valid = Some(false);
        hit.span = None;
        let Ok(proof) = serde_json::from_value::<cc_model::source::ChunkSource>(raw.clone()) else {
            return Ok(());
        };
        if !proof.validate(text) || proof.source.byte_len != bytes.len() {
            return Ok(());
        }
        let Ok(source) = std::str::from_utf8(&bytes) else {
            return Ok(());
        };
        let Some(slice) = source.get(proof.span.start..proof.span.end) else {
            return Ok(());
        };
        if slice != text || blake3::hash(&bytes).to_hex().as_str() != proof.source.content_digest {
            return Ok(());
        }
        // Independent reconstruction from raw input, not the product's coordinate helper.
        let mut digest = blake3::Hasher::new();
        digest.update(b"codecortex-source-v1\0");
        digest.update(b"utf8\0");
        digest.update(&(bytes.len() as u64).to_le_bytes());
        digest.update(&bytes);
        if digest.finalize().to_hex().as_str() != proof.source.snapshot_id {
            return Ok(());
        }
        let first = bytes[..proof.span.start]
            .iter()
            .filter(|b| **b == b'\n')
            .count()
            + 1;
        let last = bytes[..proof.span.end - 1]
            .iter()
            .filter(|b| **b == b'\n')
            .count()
            + 1;
        if hit.start_line.map(u64::from) != Some(first as u64)
            || hit.end_line.map(u64::from) != Some(last as u64)
        {
            return Ok(());
        }
        hit.span = Some(ByteSpan {
            start: proof.span.start as u64,
            end: proof.span.end as u64,
        });
        hit.evidence_valid = Some(true);
        return Ok(());
    }
    let (Some(start), Some(end)) = (hit.start_line, hit.end_line) else {
        hit.evidence_valid = Some(false);
        return Ok(());
    };
    let source =
        std::str::from_utf8(&bytes).map_err(|_| BenchError::Protocol("source encoding".into()))?;
    let lines: Vec<&str> = source.split_inclusive('\n').collect();
    if start == 0 || end < start || end as usize > lines.len() {
        hit.evidence_valid = Some(false);
        return Ok(());
    }
    let a = lines[..start as usize - 1]
        .iter()
        .map(|s| s.len())
        .sum::<usize>();
    let b = lines[..end as usize].iter().map(|s| s.len()).sum::<usize>();
    let original = &source[a..b];
    let normalized = original.lines().collect::<Vec<_>>().join("\n");
    hit.evidence_valid = Some(normalized == *text);
    hit.span = Some(ByteSpan {
        start: a as u64,
        end: (b - original.len() + original.trim_end_matches(['\r', '\n']).len()) as u64,
    });
    Ok(())
}
