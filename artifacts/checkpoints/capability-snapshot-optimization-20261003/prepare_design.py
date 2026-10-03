from pathlib import Path
import difflib, hashlib, json, subprocess
root=Path(__file__).resolve().parents[3]
out=Path(__file__).resolve().parent
base='6db4d396e5d994388ada1e97c3d28947ffdb9c81'
module='''//! Proposed as-of database observation; NOT a live-path/latest readiness fence.
//! First SELECT pins one deferred read transaction. All projected database
//! fields share that SQLite snapshot and its strict ReadGeneration.
use crate::{index_db::ReadOps, sql_util::db_err};
use cc_model::{freshness::ResolutionFreshness, generation::ReadGeneration, CcResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityReadSnapshot {
    pub generation: ReadGeneration,
    pub indexed_files: u64,
    pub indexed_symbols: u64,
    pub resolution_freshness: ResolutionFreshness,
    pub semantic: Option<CapabilitySemanticSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilitySemanticSnapshot {
    pub active_space: Option<String>,
    pub pending: u64,
    pub failed: u64,
    pub eligible: u64,
    pub published: u64,
}

impl ReadOps<'_> {
    /// Only an observation of this leased DB incarnation. Ordinary publications
    /// may commit before return. Caller must retain the existing latest fence
    /// and independently verify live database identity before projecting ready.
    /// No nested checkout; include_semantic=false avoids semantic counting.
    pub fn capability_snapshot_as_of(
        &self,
        include_semantic: bool,
    ) -> CcResult<CapabilityReadSnapshot> {
        let conn = self.0.read_conn()?;
        let tx = conn.unchecked_transaction().map_err(db_err)?;
        let generation = crate::read_generation::read_on(&tx)?;
        let (indexed_files, indexed_symbols) = tx.query_row(
            "SELECT (SELECT count(*) FROM files), (SELECT count(*) FROM symbols)",
            [],
            |r| Ok((r.get::<_, u64>(0)?, r.get::<_, u64>(1)?)),
        ).map_err(db_err)?;
        let resolution_freshness = crate::freshness_store::resolution_freshness_on(
            &tx, generation.index_epoch,
        )?;
        let semantic = if include_semantic {
            let active_space = crate::semantic_outbox::active_space_on(&tx)?;
            let (pending, failed, eligible, published) = match active_space.as_deref() {
                None => (0, 0, 0, 0),
                Some(space) => tx.query_row(
                    "SELECT
                     (SELECT count(*) FROM semantic_outbox WHERE space_id=?1 AND state IN ('pending','claimed')),
                     (SELECT count(*) FROM semantic_outbox WHERE space_id=?1 AND state='failed'),
                     (SELECT count(*) FROM document_manifest WHERE encoding_key IS NOT NULL),
                     (SELECT count(*) FROM semantic_manifest WHERE space_id=?1)",
                    [space],
                    |r| Ok((r.get::<_, u64>(0)?, r.get::<_, u64>(1)?,
                            r.get::<_, u64>(2)?, r.get::<_, u64>(3)?)),
                ).map_err(db_err)?,
            };
            Some(CapabilitySemanticSnapshot { active_space, pending, failed, eligible, published })
        } else {
            None
        };
        tx.commit().map_err(db_err)?;
        Ok(CapabilityReadSnapshot {
            generation, indexed_files, indexed_symbols, resolution_freshness, semantic,
        })
    }
}
'''
files={}
lib=root/'crates/cc-db/src/lib.rs'
files[str(lib.relative_to(root))]=(lib.read_text(),lib.read_text().replace('mod community_carry;','pub mod capability_read;\nmod community_carry;',1))
fresh=root/'crates/cc-db/src/freshness_store.rs'
a=fresh.read_text()
start=a.index('        let mut result = ResolutionFreshness::ready(epoch);')
end=a.index('        tx.commit().map_err(db_err)?;',start)
body=a[start:end].replace('tx.query_row(', 'conn.query_row(')
body='\n'.join(line[4:] if line.startswith('    ') else line for line in body.splitlines())+'\n'
b=a[:start]+'        let result = resolution_freshness_on(&tx, epoch)?;\n'+a[end:]
b+='\n/// Same-connection summary; caller owns the read transaction and epoch.\npub(crate) fn resolution_freshness_on(\n    conn: &Connection,\n    epoch: u64,\n) -> CcResult<ResolutionFreshness> {\n'+body+'    Ok(result)\n}\n'
files[str(fresh.relative_to(root))]=(a,b)
files['crates/cc-db/src/capability_read.rs']=('',module)
patch=''
for name,(a,b) in files.items():
    patch+=''.join(difflib.unified_diff(a.splitlines(True),b.splitlines(True),fromfile='a/'+name if a else '/dev/null',tofile='b/'+name))
(out/'PROPOSED-DB-ONLY.patch').write_text(patch)
paths=['crates/cc-server/src/capability_status.rs','crates/cc-server/tests/p7_status_snapshot_independent_review.rs','crates/cc-server/tests/support/p7_status_review/checks.rs','crates/cc-server/tests/support/p7_status_review/candidate.rs','crates/cc-server/tests/support/p7_status_review/legacy.rs','crates/cc-db/src/semantic_coverage.rs','crates/cc-db/src/index_db.rs','crates/cc-db/src/index_db_rebuild.rs','docs/adr/0003-semantic-persistence-single-db-boundary.md','artifacts/checkpoints/cloud-p7-status-generation-fix-20261003/README.md']
evidence=[]
for name in paths:
    actual=(root/name).read_bytes()
    frozen=subprocess.check_output(['git','show',f'{base}:{name}'],cwd=root)
    assert actual==frozen,name
    evidence.append({'path':name,'sha256':hashlib.sha256(actual).hexdigest(),'base_byte_identical':True})
checks=(root/paths[2]).read_text()
assert 'assert_eq!(attempts, 2);' in checks
assert 'assert_eq!(status["retrieval"]["generation"], json!(after));' in checks
assert 'assert_eq!(attempts, 3);' in checks
assert 'assert!(status["retrieval"]["generation"].is_null());' in checks
(out/'source-evidence.json').write_text(json.dumps({'base':base,'production_base':'9ebdb155c64e094b3d774be41dbe7ab8c4e222c1','diagnostic':'de4981c0727c82cb02096c705f6428bcfe4649a6','status':'design_contract_blocker_no_production_changes','files':evidence,'proposal_sha256':hashlib.sha256(patch.encode()).hexdigest(),'runtime_tests':'not_run_design_only','1k_5k_fake_status_AB':'not_run_no_accepted_candidate','100k':'not_run_separate_task','real_provider_calls':0,'heldout_read':False,'fault_injection':False},ensure_ascii=False,indent=2)+'\n')
print('Frozen source and independent oracle bytes verified; DB-only patch generated, not applied.')
