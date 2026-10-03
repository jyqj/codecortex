//! Dense-hit manifest fence at evidence assembly (P7-011 hydrate 守卫).
//!
//! Boundary authority: the P6-011 publish CAS (`crates/cc-db/src/semantic_publish.rs`,
//! fences 3-5: doc version / input digest / active space) and P7-011's brief
//! ("最终二次检验 manifest/source"; hydrate 二次校验复用 [`crate::evidence`]
//! SourceVerifier 的磁盘与证明校验，本模块补齐其 manifest 半边).
//!
//! ## What the existing hydrate stage already proves
//!
//! [`crate::evidence_hydrator::EvidenceHydrator`] re-checks every final hit
//! against the *document* manifest (identity/span/proof, `path_current` disk
//! digest) and skips stale files. What it cannot see is the **vector basis**:
//! a hit that entered through the dense lane was ranked by a published
//! embedding, and that publication can move or vanish while every
//! document-side fact is untouched (revoke, space switch, supersede published
//! into the manifest before this query's receipts were assembled). Trusting
//! such a hit presents stale-vector evidence as current — the recall-layer
//! twin of "绝不把缺向量当完整空结果".
//!
//! ## The guard (P6-011 fence 口径, read side)
//!
//! For every hit the dense lane annotated (`semantic@{rank}` reason, the
//! sanctioned lane-annotation convention of `plan.rs`), the CURRENT
//! publication row of the hit's document must still hold:
//!
//! 1. a `semantic_manifest` row exists (revoked → the ranking basis is gone),
//! 2. the row's `space_id` is the single ACTIVE space (P6-017: a foreign
//!    space's vector is never returned, so it never proves a hit either),
//! 3. the row's `doc_version` equals the hit's document version,
//! 4. the row's `encoding_key` equals the hit's document encoding key — the
//!    embedded-input digest handle the publish CAS copied from the document
//!    manifest (publish write: `semantic_manifest.encoding_key =
//!    document_manifest.encoding_key`).
//!
//! ## Stale disposition (skip + annotate, never fake)
//!
//! A violated fence means the hit's ranking basis is stale: the hit is
//! **skipped** — mirroring `path_current`'s skip semantics — and counted in
//! the hydrator diagnostics (`dense_manifest_fence`), never silently folded
//! into a smaller-but-complete result and never an error (the document-side
//! checks above it keep their hard-fail contract for identity corruption).

use cc_db::index_db::IndexDb;
use cc_model::{identity::DocumentRef, CcResult};
use std::collections::HashMap;

/// The one lane id whose receipts this guard fences.
pub const DENSE_LANE_ID: &str = "semantic";

/// Diagnostics key added to [`crate::evidence_hydrator::EvidenceHydrator::diagnostics`].
pub const FENCE_DIAGNOSTICS_KEY: &str = "dense_manifest_fence";

/// True when the hit was annotated by the dense lane (`{lane_id}@{rank}`,
/// the lane-driven annotation convention; a bare lane id is accepted
/// defensively but never produced today).
pub fn is_dense_hit(hit: &cc_model::search::SearchHit) -> bool {
    hit.reasons.iter().any(|reason| {
        reason.as_str() == DENSE_LANE_ID || reason.starts_with(&format!("{DENSE_LANE_ID}@"))
    })
}

/// Memoized per-document fence verdicts over one hydrator lifetime. Verdicts
/// memoize because every chunk of one document shares the publication row;
/// fence reads are pure SELECTs (no clock), so a memoized verdict is as
/// current as the hydrator's own generation fence requires.
pub struct DenseFenceGuard<'a> {
    db: &'a IndexDb,
    verdicts: HashMap<String, bool>,
    checked: usize,
    skipped: usize,
}

impl<'a> DenseFenceGuard<'a> {
    pub fn new(db: &'a IndexDb) -> Self {
        Self {
            db,
            verdicts: HashMap::new(),
            checked: 0,
            skipped: 0,
        }
    }

    /// P6-011 fence, read side: does the document's current publication still
    /// back this hit (exists, active space, same version, same encoding key)?
    /// DB errors propagate (strict reads, fail-stop) — only fence *verdicts*
    /// skip.
    pub fn manifest_current(&mut self, document: &DocumentRef) -> CcResult<bool> {
        if let Some(current) = self.verdicts.get(&document.doc_key) {
            return Ok(*current);
        }
        let current = self.check(document)?;
        self.checked += 1;
        if !current {
            self.skipped += 1;
        }
        self.verdicts.insert(document.doc_key.clone(), current);
        Ok(current)
    }

    fn check(&self, document: &DocumentRef) -> CcResult<bool> {
        let conn = self.db.read_conn()?;
        let Some(active) = cc_db::semantic_outbox::active_space_on(&conn)? else {
            return Ok(false);
        };
        let Some(row) = cc_db::semantic_manifest_reads::SemanticManifestReads::on(&conn)
            .published_row(&document.doc_key)?
        else {
            return Ok(false);
        };
        // The publish CAS copies document_manifest.encoding_key into the
        // publication row (fence 4), and the consuming DocumentRef carries the
        // same handle — so this is the manifest/input-digest identity check.
        Ok(row.space_id == active
            && row.doc_version == document.doc_version
            && Some(row.encoding_key.as_str()) == document.encoding_key.as_deref())
    }

    /// Additive diagnostics block; `checked == 0` (no dense hit in the
    /// result) reports an untouched guard.
    pub fn diagnostics(&self) -> serde_json::Value {
        serde_json::json!({
            "checked": self.checked,
            "skipped": self.skipped,
            "skip_disposition": "stale_publication_skipped",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine_test_support::{insert_chunk_file, scoped_test_engine};
    use cc_model::query::QueryControl;
    use cc_model::retrieval::HardScope;
    use cc_model::search::{SearchHit, SearchRequest};
    use std::sync::Arc;
    use std::time::Duration;

    /// One real indexed document (files → chunks → document_manifest, written
    /// through the production document path) plus its source bytes on disk,
    /// and one genuine lexical hit carrying full document/source metadata.
    fn seeded_hit() -> (tempfile::TempDir, Arc<IndexDb>, SearchHit, DocumentRef) {
        let (engine, tmp) = scoped_test_engine();
        let text = "fn needle_guard() { dense_fence_probe() }\n";
        insert_chunk_file(&engine, "src/d1.rs", cc_model::Language::Rust, text);
        // The file must exist on disk with exactly the indexed bytes for
        // `path_current` to admit the hit.
        let disk = tmp.path().join("src");
        std::fs::create_dir_all(&disk).unwrap();
        std::fs::write(disk.join("d1.rs"), text).unwrap();

        let hits = engine
            .search(&SearchRequest {
                query: "needle".into(),
                top_k: 5,
                include_grep: false,
                ..Default::default()
            })
            .unwrap()
            .to_vec();
        let mut hit = hits
            .into_iter()
            .find(|hit| hit.file_path == "src/d1.rs")
            .expect("seeded document must be searchable");
        let reference: DocumentRef = serde_json::from_value(hit.metadata["document"].clone())
            .expect("hit carries a document reference");
        assert!(
            reference.encoding_key.is_some(),
            "fixture must be an embeddable document"
        );
        // Annotate as a dense-lane hit (the lane-annotation convention).
        hit.reasons.push("semantic@1".into());
        (tmp, engine.db.clone(), hit, reference)
    }

    /// Activate one space and publish the fixture document into it, using the
    /// hit's own reference for the fenced identity fields (what the publish
    /// CAS copies from the document manifest).
    fn publish(conn: &rusqlite::Connection, space: &str, reference: &DocumentRef) {
        conn.execute(
            "INSERT INTO semantic_spaces(space_id,spec_json,state) VALUES(?1,'{}','active')",
            [space],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO semantic_manifest(doc_key,doc_version,file_path,encoding_key,\
             input_digest,space_id,artifact_ref,published_at,published_incarnation) \
             VALUES(?1,?2,'src/d1.rs',?3,'in',?4,'art','2026-01-01','inc')",
            rusqlite::params![
                reference.doc_key,
                reference.doc_version,
                reference.encoding_key,
                space
            ],
        )
        .unwrap();
    }

    fn hydrate(
        db: &IndexDb,
        root: &std::path::Path,
        hit: &SearchHit,
    ) -> (Vec<SearchHit>, serde_json::Value) {
        let generation = db.reads().read_generation().unwrap();
        let mut hydrator = crate::evidence_hydrator::EvidenceHydrator::new(
            db,
            root,
            HardScope::default(),
            generation,
            QueryControl::new(Duration::from_secs(10)).unwrap(),
        )
        .unwrap();
        let hits = hydrator.hydrate(std::slice::from_ref(hit)).unwrap();
        let diagnostics = hydrator.diagnostics();
        (hits, diagnostics)
    }

    #[test]
    fn current_publication_keeps_the_dense_hit_and_reports_a_clean_fence() {
        let (tmp, db, hit, reference) = seeded_hit();
        publish(&crate::test_seed::seed_conn(&db), "sp", &reference);
        let (hits, diagnostics) = hydrate(&db, tmp.path(), &hit);
        assert_eq!(hits.len(), 1, "a current publication must not skip");
        assert_eq!(diagnostics[FENCE_DIAGNOSTICS_KEY]["checked"], 1);
        assert_eq!(diagnostics[FENCE_DIAGNOSTICS_KEY]["skipped"], 0);
    }

    #[test]
    fn stale_published_version_skips_the_dense_hit_without_faking_completeness() {
        let (tmp, db, hit, reference) = seeded_hit();
        let conn = crate::test_seed::seed_conn(&db);
        publish(&conn, "sp", &reference);
        // The publication moved on (a newer embed was published for the same
        // document family): the hit's version is no longer the published one.
        let moved = "b".repeat(64);
        assert_ne!(moved, reference.doc_version, "fixture sanity");
        conn.execute(
            "UPDATE semantic_manifest SET doc_version=?1 WHERE doc_key=?2",
            rusqlite::params![moved, reference.doc_key],
        )
        .unwrap();
        let (hits, diagnostics) = hydrate(&db, tmp.path(), &hit);
        assert!(hits.is_empty(), "stale publication must skip, not pass");
        assert_eq!(diagnostics[FENCE_DIAGNOSTICS_KEY]["skipped"], 1);
    }

    #[test]
    fn revoked_publication_skips_the_dense_hit() {
        let (tmp, db, hit, reference) = seeded_hit();
        let conn = crate::test_seed::seed_conn(&db);
        publish(&conn, "sp", &reference);
        conn.execute(
            "DELETE FROM semantic_manifest WHERE doc_key=?1",
            [&reference.doc_key],
        )
        .unwrap();
        let (hits, diagnostics) = hydrate(&db, tmp.path(), &hit);
        assert!(hits.is_empty(), "revoked ranking basis must skip");
        assert_eq!(diagnostics[FENCE_DIAGNOSTICS_KEY]["skipped"], 1);
    }

    #[test]
    fn foreign_space_publication_skips_the_dense_hit() {
        let (tmp, db, hit, reference) = seeded_hit();
        let conn = crate::test_seed::seed_conn(&db);
        publish(&conn, "sp", &reference);
        // Space switch left the row in a non-active space (P6-017: a foreign
        // space's vector never proves anything).
        conn.execute(
            "UPDATE semantic_manifest SET space_id='sp-old' WHERE doc_key=?1",
            [&reference.doc_key],
        )
        .unwrap();
        let (hits, _diagnostics) = hydrate(&db, tmp.path(), &hit);
        assert!(hits.is_empty(), "foreign-space publication must skip");
    }

    #[test]
    fn fence_applies_only_to_dense_annotated_hits() {
        let (tmp, db, mut hit, reference) = seeded_hit();
        let conn = crate::test_seed::seed_conn(&db);
        publish(&conn, "sp", &reference);
        conn.execute(
            "DELETE FROM semantic_manifest WHERE doc_key=?1",
            [&reference.doc_key],
        )
        .unwrap();
        hit.reasons.retain(|reason| !reason.starts_with("semantic"));
        let (hits, diagnostics) = hydrate(&db, tmp.path(), &hit);
        assert_eq!(
            hits.len(),
            1,
            "lexical-provenance hits have no vector basis to fence"
        );
        assert_eq!(diagnostics[FENCE_DIAGNOSTICS_KEY]["checked"], 0);
    }

    #[test]
    fn unconfigured_semantic_skips_dense_hits_without_error() {
        // No active space at all: nothing can back a dense hit.
        let (tmp, db, hit, _reference) = seeded_hit();
        let (hits, diagnostics) = hydrate(&db, tmp.path(), &hit);
        assert!(hits.is_empty());
        assert_eq!(diagnostics[FENCE_DIAGNOSTICS_KEY]["skipped"], 1);
    }

    #[test]
    fn is_dense_hit_matches_the_lane_annotation_convention() {
        let mut hit = minimal_hit();
        hit.reasons = vec!["lexical@1".into(), "semantic@2".into()];
        assert!(is_dense_hit(&hit));
        hit.reasons = vec!["exact-target".into()];
        assert!(!is_dense_hit(&hit));
        hit.reasons = vec!["semantic".into()];
        assert!(is_dense_hit(&hit), "bare lane id is accepted defensively");
        hit.reasons = vec!["semantics@1".into()];
        assert!(!is_dense_hit(&hit), "no prefix confusion across lane ids");
    }

    fn minimal_hit() -> SearchHit {
        SearchHit {
            chunk_id: "c".into(),
            file_path: "a.rs".into(),
            language: cc_model::Language::Rust,
            start_line: 1,
            end_line: 1,
            breadcrumb: "b".into(),
            symbol_name: None,
            symbol_kind: None,
            text: "t".into(),
            fused_score: 0.0,
            lexical_score: 0.0,
            grep_score: 0.0,
            graph_score: 0.0,
            rerank_score: 0.0,
            reasons: Vec::new(),
            score_trace: Vec::new(),
            source: String::new(),
            lane: None,
            metadata: serde_json::json!({}),
        }
    }
}
