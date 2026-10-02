//! Dense-recall scope declaration (P7-011 dense 范围守卫; 接线待办 12 消纳).
//!
//! Boundary authority: P6-012's coverage 口径 (`crates/cc-db/src/semantic_coverage.rs`:
//! eligible / published / uncovered 差集 + per-document `uncovered` list) and
//! the P6-018 ruling "绝不把缺向量当完整空结果", extended to the recall layer
//! by this module: a dense recall that ran against a *partial* publication
//! must never hand its caller a `Complete` receipt.
//!
//! ## Why the plain exact-scan `Complete` claim is not enough
//!
//! The exact backend (P6-010) completes its scan over the *visible set* — the
//! active space's `semantic_manifest` rows. Completion over the visible set
//! says nothing about the *eligible* set: while documents are still awaiting
//! their embed (backfill, re-encode, budget exhaustion), a recall result —
//! including an empty one — is based on partial knowledge. Declaring it
//! `Complete` would masquerade missing vectors as a finished answer.
//!
//! ## The declaration (all reads on the caller's connection)
//!
//! The recall already holds one read connection for the candidate scan; the
//! declaration is computed on that same connection so scan and scope claim
//! are one consistent read:
//!
//! 1. **Space fence.** Coverage is defined against the single ACTIVE space.
//!    A recall resolving a different space digest has no honest coverage
//!    claim at all — and per P6-017 a non-active space's vectors must never
//!    be returned — so the outcome is `Unavailable`
//!    (`semantic_space_not_active`), never `Complete + 0`.
//! 2. **Uncovered intersection.** The P6-012 uncovered *list* (per-document
//!    差集 projection) is paged in keyset order and intersected with the
//!    request's hard scope: only an uncovered document that the scope admits
//!    makes the recall partially covered. This keeps `LaneCoverage` scoped to
//!    the lane's declared hard-scope query (cc-model's documented semantics)
//!    and avoids the over-strict "守卫过严把合法命中打成 partial" failure the
//!    P7 brief warns about.
//! 3. **Verdict.** No in-scope uncovered document → the existing
//!    `Complete / LaneCoverage::complete` receipt (byte-identical to the
//!    pre-guard behavior). Otherwise `Partial` with
//!    `semantic_coverage_uncovered`: the candidates stay usable (Partial is
//!    fusable), but the receipt names the gap instead of hiding it.
//!
//! ## Unknown-language rule (same convention as the scan)
//!
//! The uncovered list carries no language. A scope with a `languages`
//! constraint cannot admit a document of unknown language, and the exact
//! scan's own admission rule (`vector/exact.rs::row_passes_scope`) excludes
//! exactly that case — so the intersection mirrors the scan: language-
//! constrained scopes deny unknown-language uncovered rows, unconstrained
//! scopes evaluate the path dimensions only.
//!
//! ## Bounded honesty of the uncovered paging
//!
//! Paging is keyset-ordered with a fixed page budget
//! ([`UNCOVERED_SCAN_MAX_PAGES`] × [`UNCOVERED_SCAN_PAGE_ROWS`] ≈ 16k
//! documents). Hitting the budget yields the conservative verdict (Partial):
//! an unseen remainder *might* contain an in-scope uncovered document, and
//! the guard never resolves that doubt in favor of `Complete`. The page
//! budget is a DoS bound, not a correctness knob.

use cc_db::index_db::IndexDb;
use cc_db::semantic_coverage::uncovered_on;
use cc_db::semantic_outbox::active_space_on;
use cc_model::retrieval::{HardScope, LaneCoverage, LaneStatus};
use cc_model::{CcResult, Language};

/// `truncation_reason` of a partial-coverage recall receipt.
pub const PARTIAL_COVERAGE_REASON: &str = "semantic_coverage_uncovered";
/// `truncation_reason` when the recall's space is not the active space.
pub const SPACE_NOT_ACTIVE_REASON: &str = "semantic_space_not_active";
/// Uncovered-list page size (the P6-012 keyset scan's own granularity).
pub const UNCOVERED_SCAN_PAGE_ROWS: usize = 256;
/// Page budget before the declaration turns conservative (see module docs).
pub const UNCOVERED_SCAN_MAX_PAGES: usize = 64;

/// The honest scope verdict of one dense recall, consumed verbatim into the
/// lane outcome (`status` / `coverage` / `truncation_reason`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DenseScopeDeclaration {
    pub status: LaneStatus,
    pub coverage: LaneCoverage,
    pub truncation_reason: Option<String>,
}

/// Declare the recall scope. `space_digest` is the digest the recall scanned,
/// `scope` the request's hard scope, `candidates` the result count. The reads
/// run on their own short read-connection checkout (the engine convention:
/// never hold one checkout across another), strictly after the candidate
/// scan released its own; the lane adapter's generation re-verification
/// covers the interleave.
pub fn declare(
    db: &IndexDb,
    space_digest: &str,
    scope: &HardScope,
    candidates: usize,
) -> CcResult<DenseScopeDeclaration> {
    declare_with(
        db,
        space_digest,
        scope,
        candidates,
        UNCOVERED_SCAN_MAX_PAGES,
    )
}

/// Injectable page budget (tests exercise the conservative cap without
/// seeding 16k rows).
pub fn declare_with(
    db: &IndexDb,
    space_digest: &str,
    scope: &HardScope,
    candidates: usize,
    max_pages: usize,
) -> CcResult<DenseScopeDeclaration> {
    let conn = db.read_conn()?;
    // 1. Space fence: coverage exists only against the active space.
    if active_space_on(&conn)?.as_deref() != Some(space_digest) {
        return Ok(DenseScopeDeclaration {
            status: LaneStatus::Unavailable,
            coverage: LaneCoverage::not_run(),
            truncation_reason: Some(SPACE_NOT_ACTIVE_REASON.into()),
        });
    }
    // 2. Uncovered-list intersection with the request's hard scope.
    let mut uncovered_in_scope = false;
    let mut budget_exhausted = false;
    let mut cursor = String::new();
    for pages_used in 0..max_pages {
        let page = uncovered_on(&conn, &cursor, UNCOVERED_SCAN_PAGE_ROWS)?;
        if page.is_empty() {
            break;
        }
        cursor = page.last().unwrap().doc_key.clone();
        if page
            .iter()
            .any(|row| uncovered_passes_scope(scope, &row.file_path))
        {
            uncovered_in_scope = true;
            break;
        }
        if page.len() < UNCOVERED_SCAN_PAGE_ROWS {
            break; // list exhausted honestly
        }
        budget_exhausted = pages_used + 1 == max_pages;
    }
    // 3. Verdict. An exhausted page budget means a remainder might contain an
    //    in-scope uncovered document; the doubt never resolves to Complete.
    if uncovered_in_scope || budget_exhausted {
        Ok(DenseScopeDeclaration {
            status: LaneStatus::Partial,
            coverage: LaneCoverage::partial(None, candidates),
            truncation_reason: Some(PARTIAL_COVERAGE_REASON.into()),
        })
    } else {
        Ok(DenseScopeDeclaration {
            status: LaneStatus::Complete,
            coverage: LaneCoverage::complete(None, candidates),
            truncation_reason: None,
        })
    }
}

/// Scope admission for one uncovered row (module docs: unknown-language rule
/// mirrors `vector/exact.rs::row_passes_scope`).
fn uncovered_passes_scope(scope: &HardScope, file_path: &str) -> bool {
    match scope.languages.as_ref() {
        Some(_) => false,
        None => scope.passes(file_path, Language::Unknown),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cc_db::index_db::IndexDb;
    use rusqlite::Connection;

    fn db() -> (tempfile::TempDir, IndexDb) {
        let dir = tempfile::tempdir().unwrap();
        let (db, _) = IndexDb::open(&dir.path().join("index.sqlite3")).unwrap();
        (dir, db)
    }

    fn seed_eligible(conn: &Connection, doc_key: &str, file_path: &str) {
        conn.execute_batch(&format!(
            "INSERT INTO files(file_path,language,content_hash,mtime,size,indexed_at) \
               VALUES('{file_path}','rust','hash',1.0,1,'2026-01-01');
             INSERT INTO chunks(chunk_id,file_path,language,chunk_index,start_line,end_line,text) \
               VALUES('c-{doc_key}','{file_path}','rust',0,1,2,'body');
             INSERT INTO document_manifest(doc_key,doc_version,file_path,chunk_id,encoding_key,\
               reference_json,record_json) \
               VALUES('{doc_key}','v1','{file_path}','c-{doc_key}','enc','{{}}','{{}}');"
        ))
        .unwrap();
    }

    fn activate(conn: &Connection, space_id: &str) {
        conn.execute(
            "INSERT INTO semantic_spaces(space_id,spec_json,state) VALUES(?1,'{}','active')",
            [space_id],
        )
        .unwrap();
    }

    fn publish(conn: &Connection, doc_key: &str, space_id: &str) {
        conn.execute(
            "INSERT INTO semantic_manifest(doc_key,doc_version,file_path,encoding_key,\
             input_digest,space_id,artifact_ref,published_at,published_incarnation) \
             SELECT doc_key,doc_version,file_path,encoding_key,'in',?2,'art','2026-01-01','inc' \
             FROM document_manifest WHERE doc_key=?1",
            rusqlite::params![doc_key, space_id],
        )
        .unwrap();
    }

    #[test]
    fn full_coverage_declares_complete_like_the_pre_guard_receipt() {
        let (_dir, db) = db();
        let conn = rusqlite::Connection::open(db.admin().db_path()).unwrap();
        activate(&conn, "sp");
        seed_eligible(&conn, "d1", "src/d1.rs");
        publish(&conn, "d1", "sp");
        let declaration = declare(&db, "sp", &HardScope::default(), 1).unwrap();
        assert_eq!(
            declaration,
            DenseScopeDeclaration {
                status: LaneStatus::Complete,
                coverage: LaneCoverage::complete(None, 1),
                truncation_reason: None,
            }
        );
    }

    #[test]
    fn uncovered_in_scope_declares_partial_and_names_the_gap() {
        let (_dir, db) = db();
        let conn = rusqlite::Connection::open(db.admin().db_path()).unwrap();
        activate(&conn, "sp");
        seed_eligible(&conn, "d1", "src/d1.rs");
        seed_eligible(&conn, "d2", "src/d2.rs");
        publish(&conn, "d1", "sp");
        let declaration = declare(&db, "sp", &HardScope::default(), 1).unwrap();
        assert_eq!(declaration.status, LaneStatus::Partial);
        assert_eq!(
            declaration.truncation_reason.as_deref(),
            Some(PARTIAL_COVERAGE_REASON)
        );
        assert!(!declaration.coverage.complete);
    }

    #[test]
    fn uncovered_outside_the_hard_scope_stays_complete() {
        // LaneCoverage is scoped to the declared hard-scope query: d2 is
        // uncovered repo-wide but outside the requested file set, so the
        // recall's own domain is fully published (守卫不过严).
        let (_dir, db) = db();
        let conn = rusqlite::Connection::open(db.admin().db_path()).unwrap();
        activate(&conn, "sp");
        seed_eligible(&conn, "d1", "src/d1.rs");
        seed_eligible(&conn, "d2", "src/d2.rs");
        publish(&conn, "d1", "sp");
        let scope = HardScope {
            file_paths: Some(vec!["src/d1.rs".into()]),
            ..Default::default()
        };
        let declaration = declare(&db, "sp", &scope, 1).unwrap();
        assert_eq!(declaration.status, LaneStatus::Complete);
        assert!(declaration.coverage.complete);
        assert!(declaration.truncation_reason.is_none());
    }

    #[test]
    fn language_constrained_scope_denies_unknown_language_uncovered_rows() {
        // The uncovered list carries no language; a languages-constrained
        // scope cannot admit it (mirrors the scan's own admission rule).
        let (_dir, db) = db();
        let conn = rusqlite::Connection::open(db.admin().db_path()).unwrap();
        activate(&conn, "sp");
        seed_eligible(&conn, "d1", "src/d1.rs");
        seed_eligible(&conn, "d2", "src/d2.rs");
        publish(&conn, "d1", "sp");
        let scope = HardScope {
            languages: Some(vec![Language::Rust]),
            ..Default::default()
        };
        assert_eq!(
            declare(&db, "sp", &scope, 0).unwrap().status,
            LaneStatus::Complete
        );
        assert_eq!(
            declare(&db, "sp", &HardScope::default(), 0)
                .unwrap()
                .status,
            LaneStatus::Partial,
            "the same uncovered document without a language constraint is in scope"
        );
    }

    #[test]
    fn exhausted_page_budget_resolves_conservatively_to_partial() {
        let (_dir, db) = db();
        let conn = rusqlite::Connection::open(db.admin().db_path()).unwrap();
        activate(&conn, "sp");
        // Three pages' worth of uncovered documents outside the probed file
        // set; the budget runs out before the list is exhausted.
        for index in 0..(UNCOVERED_SCAN_PAGE_ROWS * 3) {
            seed_eligible(&conn, &format!("d{index:06}"), &format!("src/d{index:06}.rs"));
        }
        seed_eligible(&conn, "zz-published", "src/zz.rs");
        publish(&conn, "zz-published", "sp");
        let scope = HardScope {
            file_paths: Some(vec!["src/zz.rs".into()]),
            ..Default::default()
        };
        let declaration = declare_with(&db, "sp", &scope, 0, 2).unwrap();
        assert_eq!(declaration.status, LaneStatus::Partial);
        assert_eq!(
            declaration.truncation_reason.as_deref(),
            Some(PARTIAL_COVERAGE_REASON)
        );
        // With the default budget the same database is honestly complete.
        assert_eq!(declare(&db, "sp", &scope, 0).unwrap().status, LaneStatus::Complete);
    }

    #[test]
    fn non_active_space_is_unavailable_never_complete_zero() {
        let (_dir, db) = db();
        let conn = rusqlite::Connection::open(db.admin().db_path()).unwrap();
        activate(&conn, "sp-active");
        seed_eligible(&conn, "d1", "src/d1.rs");
        publish(&conn, "d1", "sp-active");
        let declaration = declare(&db, "sp-other", &HardScope::default(), 0).unwrap();
        assert_eq!(declaration.status, LaneStatus::Unavailable);
        assert_eq!(
            declaration.truncation_reason.as_deref(),
            Some(SPACE_NOT_ACTIVE_REASON)
        );
        assert_eq!(declaration.coverage, LaneCoverage::not_run());
    }

    #[test]
    fn no_active_space_at_all_is_unavailable() {
        let (_dir, db) = db();
        let declaration = declare(&db, "sp", &HardScope::default(), 0).unwrap();
        assert_eq!(declaration.status, LaneStatus::Unavailable);
    }

    #[test]
    fn the_declaration_validates_as_a_lane_outcome() {
        // Both fusable verdicts must satisfy LaneOutcome::validate's
        // status/coverage/reason coupling (empty receipts).
        let (_dir, db) = db();
        let conn = rusqlite::Connection::open(db.admin().db_path()).unwrap();
        activate(&conn, "sp");
        seed_eligible(&conn, "d1", "src/d1.rs");
        seed_eligible(&conn, "d2", "src/d2.rs");
        publish(&conn, "d1", "sp");
        let partial = declare(&db, "sp", &HardScope::default(), 0).unwrap();
        let narrowed = HardScope {
            file_paths: Some(vec!["src/d1.rs".into()]),
            ..Default::default()
        };
        let complete = declare(&db, "sp", &narrowed, 0).unwrap();
        for declaration in [partial, complete] {
            let outcome = cc_model::retrieval::LaneOutcome {
                schema_version: cc_model::retrieval::LANE_OUTCOME_SCHEMA_VERSION,
                lane_id: "semantic".into(),
                weight: 1.0,
                status: declaration.status,
                elapsed_us: 1,
                candidate_count: 0,
                coverage: declaration.coverage,
                truncation_reason: declaration.truncation_reason,
                candidates: Vec::new(),
            };
            outcome.validate().expect("empty receipt validates");
        }
    }
}
