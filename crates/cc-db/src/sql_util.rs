//! Shared SQL string-building helpers for batched IN(...) queries and
//! LIKE-pattern escaping.

/// Standard chunk size for batched SQL IN(...) queries.
pub const IN_BATCH_SIZE: usize = 200;

/// Build a 1-based "?1,?2,..." placeholder list for SQL IN(...) clauses.
pub fn sql_in_placeholders(len: usize) -> String {
    (1..=len)
        .map(|idx| format!("?{}", idx))
        .collect::<Vec<_>>()
        .join(",")
}

/// Escape `\`, `%`, and `_` in `text` so it can be embedded in a SQL LIKE
/// pattern as a literal. The corresponding LIKE must carry `ESCAPE '\'`.
pub fn escape_like(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for ch in text.chars() {
        if matches!(ch, '\\' | '%' | '_') {
            escaped.push('\\');
        }
        escaped.push(ch);
    }
    escaped
}

/// Map any `Display` error (rusqlite / r2d2 / io / lock-poison) into a
/// `CcError::Database` carrying its string form. The single source of truth
/// for DB error mapping across cc-db — every `.map_err(db_err)` site used to
/// spell out `|e| CcError::Database(e.to_string())`. Errors that need
/// contextual detail (e.g. `read_conn`'s `"pool get: {e}"`) keep their own
/// closure; only the bare `to_string()` form routes here.
pub fn db_err(err: impl std::fmt::Display) -> cc_model::CcError {
    cc_model::CcError::Database(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::escape_like;

    #[test]
    fn escape_like_escapes_wildcards_and_backslash() {
        assert_eq!(escape_like("read_conn"), "read\\_conn");
        assert_eq!(escape_like("100%"), "100\\%");
        assert_eq!(escape_like("a\\b"), "a\\\\b");
        assert_eq!(escape_like("plain"), "plain");
        assert_eq!(escape_like(""), "");
    }
}
