//! Additive metadata identity. Reads never initialize or repair a missing id.
//! Every published replacement DB gets a fresh incarnation, even at equal epochs.
use crate::{index_db::ReadOps, sql_util::db_err};
use cc_model::{generation::ReadGeneration, CcError, CcResult};
use rusqlite::Connection;

pub(crate) const INCARNATION: &str = "index_incarnation";

fn parse_id(value: &str) -> CcResult<[u8; 16]> {
    if value.len() != 32
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(CcError::Database(
            "invalid persisted index incarnation".into(),
        ));
    }
    let mut id = [0u8; 16];
    for (i, byte) in id.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[i * 2..i * 2 + 2], 16)
            .map_err(|_| CcError::Database("invalid persisted index incarnation".into()))?;
    }
    if id == [0; 16] {
        return Err(CcError::Database("zero persisted index incarnation".into()));
    }
    Ok(id)
}

/// Writable schema-open seam only. INSERT OR IGNORE converges across openers;
/// malformed existing values fail instead of silently acquiring a new identity.
pub(crate) fn ensure(conn: &Connection) -> CcResult<()> {
    conn.execute(
        "INSERT OR IGNORE INTO metadata(key,value) VALUES(?1,lower(hex(randomblob(16))))",
        [INCARNATION],
    )
    .map_err(db_err)?;
    read_on(conn).map(|_| ())
}

/// Called on the completed staging DB before publication, not on the live DB.
pub(crate) fn renew(conn: &Connection) -> CcResult<()> {
    conn.execute(
        "INSERT INTO metadata(key,value) VALUES(?1,lower(hex(randomblob(16)))) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        [INCARNATION],
    ).map_err(db_err)?;
    read_on(conn).map(|_| ())
}

pub(crate) fn read_on(conn: &Connection) -> CcResult<ReadGeneration> {
    let mut stmt = conn.prepare_cached(
        "SELECT key,value FROM metadata WHERE key IN ('index_incarnation','index_epoch','evidence_epoch','semantic_epoch')",
    ).map_err(db_err)?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .map_err(db_err)?;
    let (mut incarnation, mut index_epoch, mut evidence_epoch, mut semantic_epoch) =
        (None, 0, 0, None);
    for row in rows {
        let (key, value) = row.map_err(db_err)?;
        if key == INCARNATION {
            incarnation = Some(parse_id(&value)?);
        } else {
            let epoch = value
                .parse::<u64>()
                .map_err(|_| CcError::Database(format!("invalid persisted {key}")))?;
            match key.as_str() {
                "index_epoch" => index_epoch = epoch,
                "evidence_epoch" => evidence_epoch = epoch,
                "semantic_epoch" => semantic_epoch = Some(epoch),
                _ => unreachable!(),
            }
        }
    }
    Ok(ReadGeneration {
        incarnation: incarnation
            .ok_or_else(|| CcError::Database("missing persisted index incarnation".into()))?,
        index_epoch,
        evidence_epoch,
        semantic_epoch,
    })
}
impl ReadOps<'_> {
    /// One SQL statement captures all persisted dependencies on one connection.
    pub fn read_generation(&self) -> CcResult<ReadGeneration> {
        let conn = self.0.read_conn()?;
        read_on(&conn)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index_db::IndexDb;
    #[test]
    fn persisted_identity_survives_reopen_and_differs_at_equal_epochs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("index.sqlite3");
        let db = IndexDb::open(&path).unwrap().0;
        let first = db.reads().read_generation().unwrap();
        let other = IndexDb::open(&dir.path().join("other.sqlite3")).unwrap().0;
        let second = other.reads().read_generation().unwrap();
        assert_eq!(first.index_epoch, second.index_epoch);
        assert_ne!(first.incarnation, second.incarnation);
        drop(db);
        let reopened = IndexDb::open(&path).unwrap().0;
        assert_eq!(first, reopened.reads().read_generation().unwrap());
        assert_eq!(first.semantic_epoch, None);
    }
    #[test]
    fn identity_and_epoch_corruption_never_become_zero_generation() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE metadata(key TEXT PRIMARY KEY,value TEXT);")
            .unwrap();
        assert!(read_on(&conn).is_err());
        ensure(&conn).unwrap();
        let before = read_on(&conn).unwrap();
        ensure(&conn).unwrap();
        assert_eq!(before, read_on(&conn).unwrap());
        conn.execute("INSERT INTO metadata VALUES('index_epoch','broken')", [])
            .unwrap();
        assert!(read_on(&conn).is_err());
        conn.execute("DELETE FROM metadata WHERE key='index_epoch'", [])
            .unwrap();
        conn.execute(
            "UPDATE metadata SET value='broken' WHERE key=?1",
            [INCARNATION],
        )
        .unwrap();
        assert!(ensure(&conn).is_err());
    }
    #[test]
    fn staged_rebuild_gets_fresh_persistent_identity() {
        let dir = tempfile::tempdir().unwrap();
        let db = IndexDb::open(&dir.path().join("index.sqlite3")).unwrap().0;
        let before = db.reads().read_generation().unwrap();
        db.rebuild_with_temp_db(|_| Ok(())).unwrap();
        let after = db.reads().read_generation().unwrap();
        assert_ne!(before.incarnation, after.incarnation);
        assert!(after.index_epoch > before.index_epoch);
        assert!(after.evidence_epoch > before.evidence_epoch);
    }
}
