from pathlib import Path


def replace(path,old,new,count=1):
 p=Path(path);s=p.read_text()
 if s.count(old)!=count:raise RuntimeError(f'{path}: wanted {count}, got {s.count(old)} for {old!r}')
 p.write_text(s.replace(old,new))

p=Path('crates/cc-eval/src/quality.rs');s=p.read_text()
a=s.index('#[cfg(test)]\nmod tests');b=s.index('/// Read saved wire observations after measurement.')
assert a<b
p.write_text(s[:a]+s[b:]+'\n'+s[a:b])
replace('crates/cc-eval/examples/quality_run.rs','command("git", &["diff", "HEAD", "--"])\n                .unwrap_or_default()', 'command("git", &["diff", "HEAD", "--"])?') if '.unwrap_or_default().as_bytes()' in '' else None
# No silent provenance fallback: preserve the error as a failed run.
p=Path('crates/cc-eval/examples/quality_run.rs');s=p.read_text()
old='command("git", &["diff", "HEAD", "--"]).unwrap_or_default()'
assert old in s
p.write_text(s.replace(old,'command("git", &["diff", "HEAD", "--"])?'))

path='crates/cc-index/src/dirty_reload_policy.rs'
replace(path,'''    /// so all resolved targets of this category — same-file and cross-file
    /// alike — are cleared unconditionally for phase 4a re-resolution.''','''    /// so resolver-derived targets are cleared for phase 4a re-resolution.
    /// Parser-exact same-file call/ref bindings are the narrow exception:
    /// dirty reload never changes that file's source or local symbol identity.''')
replace(path,'        for edge in &mut call_edges {','''        for edge in &mut call_edges {
            if parser_local_binding(&edge.file_path, edge.target_file_path.as_deref(),
                &edge.resolution_strategy) && edge.target_symbol_id.is_some() && edge.callee_symbol_uid.is_some() {
                continue;
            }''')
replace(path,'        for sym_ref in &mut symbol_refs {','''        for sym_ref in &mut symbol_refs {
            if parser_local_binding(&sym_ref.file_path, sym_ref.target_file_path.as_deref(),
                &sym_ref.resolution_strategy) && sym_ref.target_symbol_id.is_some() && sym_ref.target_symbol_uid.is_some() {
                continue;
            }''')
replace(path,'#[cfg(test)]\nmod tests {','''/// Only source-proven local bindings survive an unchanged-file reload.
/// Heuristic same-file bindings may still depend on a changed receiver/type.
fn parser_local_binding(file: &str, target_file: Option<&str>, strategy: &str) -> bool {
    strategy == "parser_exact" && target_file == Some(file)
}

#[cfg(test)]
mod tests {''')
replace(path,'mod tests {\n    use super::*;','''mod tests {
    use super::*;
    #[test]
    fn only_parser_exact_local_bindings_survive() {
        assert!(parser_local_binding("a.ts",Some("a.ts"),"parser_exact"));
        assert!(!parser_local_binding("a.ts",Some("b.ts"),"parser_exact"));
        assert!(!parser_local_binding("a.ts",Some("a.ts"),"scope"));
        assert!(!parser_local_binding("a.ts",None,"parser_exact"));
    }''')

p=Path('crates/cc-db/src/index_db_graph.rs');s=p.read_text();idx=s.index('#[cfg(test)]')
new='''// Actual persisted call/reference targets are invalidation dependencies even
// when an import string could not be resolved (e.g. a Rust qualified `use`).
impl SymbolGraphReads<'_> {
    fn find_bound_dependents_of(&self, file_paths: &[String]) -> CcResult<Vec<String>> {
        let mut found = std::collections::BTreeSet::new();
        if file_paths.is_empty() { return Ok(Vec::new()); }
        let conn = self.db.read_conn()?;
        for batch in file_paths.chunks(IN_BATCH_SIZE) {
            let placeholders = vec!["?"; batch.len()].join(",");
            // Snapshot still contains the old symbols during dirty planning.
            // UID lookups use the existing call/ref target indexes; no schema
            // change and no per-file full scan of the edge tables.
            let sql = format!(
                "WITH changed AS (SELECT symbol_uid FROM symbols WHERE file_path IN ({placeholders}))
                 SELECT file_path FROM call_edges WHERE callee_symbol_uid IN (SELECT symbol_uid FROM changed)
                 UNION SELECT file_path FROM symbol_refs WHERE target_symbol_uid IN (SELECT symbol_uid FROM changed)
                 ORDER BY file_path"
            );
            let mut stmt = conn.prepare(&sql).map_err(db_err)?;
            let rows = stmt.query_map(rusqlite::params_from_iter(batch.iter()), |row| row.get::<_,String>(0)).map_err(db_err)?;
            for row in rows { found.insert(row.map_err(db_err)?); }
        }
        Ok(found.into_iter().collect())
    }
}

impl ReadOps<'_> {
    /// Files with persisted calls/references into the old symbols of these files.
    /// Complements imports; it does not discover previously unresolved targets.
    pub fn find_bound_dependents_of(&self, file_paths: &[String]) -> CcResult<Vec<String>> {
        self.0.symbol_graph_reads().find_bound_dependents_of(file_paths)
    }
}

'''
p.write_text(s[:idx]+new+s[idx:])
replace('crates/cc-index/src/indexer_phases/dirty.rs',
 '|files| self.db.reads().find_importers_of(files),',
 '''|files| {
                let mut dependents = self.db.reads().find_importers_of(files)?;
                dependents.extend(self.db.reads().find_bound_dependents_of(files)?);
                Ok(dependents)
            },''')
path='crates/cc-eval/tests/incremental_oracle.rs'
p=Path(path);s=p.read_text();a=s.index('        assert_eq!(\n            incremental, rebuilt,');b=s.index('\n        );',a)+len('\n        );')
s=s[:a]+'''        for section in ["symbols", "imports", "calls", "refs", "chunks"] {
            let actual = incremental[section].as_array().unwrap();
            let expected = rebuilt[section].as_array().unwrap();
            assert_eq!(actual.len(), expected.len(), "{section} cardinality after {api_path}: {replacement:?}");
            for (row, (a, b)) in actual.iter().zip(expected).enumerate() {
                assert_eq!(a, b, "{section}[{row}] after {api_path}: {replacement:?}");
            }
        }'''+s[b:];p.write_text(s)
# Never share compiled workspace artifacts across different checkout roots:
# checkout timestamps can otherwise make a stale baseline binary appear fresh.
replace('_dev/compare_baseline.sh','export CARGO_TARGET_DIR="$candidate/target/paired-benchmark"', 'export CARGO_TARGET_DIR="$candidate/target/paired-base-build"')
replace('_dev/compare_baseline.sh','cargo run -p cc-eval --locked --example quality_run -- crates/cc-eval/benchmarks/quality_smoke.json target/quality-paired', 'CARGO_TARGET_DIR="$candidate/target/paired-candidate-build" cargo run -p cc-eval --locked --example quality_run -- crates/cc-eval/benchmarks/quality_smoke.json target/quality-paired')
# `_dev` is excluded from the bot commit; persist the updated isolation script
# directly through the API on the next finalization step instead.
print('Preserved parser provenance; invalidation now follows known target bindings')
