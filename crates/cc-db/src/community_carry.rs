//! Preserve derived membership across replacement of the SAME symbol identity.
//! Community gate inputs may be unchanged while a file/dirty writer replaces
//! symbol rows. Dropping membership in that case cannot be repaired by a skipped
//! postprocess pass. Changed graph inputs still recompute and overwrite it.
use crate::sql_util::{db_err, sql_in_placeholders, IN_BATCH_SIZE};
use cc_model::CcResult;
use rusqlite::Connection;

pub(crate) struct CommunityCarry(Vec<(String, String, String, String, i64)>);
impl CommunityCarry {
    pub(crate) fn capture(conn: &Connection, paths: &[&str]) -> CcResult<Self> {
        let mut records = Vec::new();
        for batch in paths.chunks(IN_BATCH_SIZE) {
            let sql=format!("SELECT file_path,symbol_uid,name,kind,community_id FROM symbols WHERE file_path IN ({}) AND symbol_uid IS NOT NULL AND community_id IS NOT NULL",sql_in_placeholders(batch.len()));
            let mut stmt = conn.prepare_cached(&sql).map_err(db_err)?;
            let rows = stmt
                .query_map(rusqlite::params_from_iter(batch.iter()), |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
                })
                .map_err(db_err)?;
            for row in rows {
                records.push(row.map_err(db_err)?);
            }
        }
        Ok(Self(records))
    }
    pub(crate) fn restore(self, conn: &Connection) -> CcResult<()> {
        if self.0.is_empty() {
            return Ok(());
        }
        let mut stmt=conn.prepare_cached("UPDATE symbols SET community_id=?5 WHERE file_path=?1 AND symbol_uid=?2 AND name=?3 AND kind=?4 AND community_id IS NULL").map_err(db_err)?;
        for (path, uid, name, kind, community) in self.0 {
            stmt.execute(rusqlite::params![path, uid, name, kind, community])
                .map_err(db_err)?;
        }
        Ok(())
    }
}
