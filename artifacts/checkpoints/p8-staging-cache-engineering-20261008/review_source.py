#!/usr/bin/env python3
"""Root's independent source/diff review; no Cargo or product execution."""
from pathlib import Path
import hashlib
import importlib.util
import json
import subprocess

HERE = Path(__file__).resolve().parent
REPO = Path('/workspace/scratch/2eaa00d0f93a/p8-staging-reviewed-exact-source')
BASE = '260f596582f2d82b8d7c707b61a6b8b6a43b069f'
HEAD = '41236c3eb6e43d8c6f0fc489d5192cf14483069c'
PATHS = ['crates/cc-db/src/resolution_dependency_store.rs',
         'crates/cc-db/src/symbol_identity_store.rs',
         'crates/cc-db/tests/p2b_resolution_store.rs',
         'crates/cc-index/tests/qname_identity_transaction.rs']

def git(*args):
    return subprocess.check_output(['git', '-C', str(REPO), *args])

def sha(b):
    return hashlib.sha256(b).hexdigest()

def main():
    assert git('rev-parse', 'HEAD').decode().strip() == HEAD
    assert set(git('diff', '--name-only', BASE, HEAD).decode().splitlines()) == set(PATHS)
    changes = []
    for path in PATHS:
        old = git('show', BASE + ':' + path)
        new = git('show', HEAD + ':' + path)
        assert (REPO / path).read_bytes() == new
        changes.append({'path': path, 'before_sha256': sha(old), 'after_sha256': sha(new), 'bytes': len(new),
                        'before_blob': git('rev-parse', BASE + ':' + path).decode().strip(),
                        'after_blob': git('rev-parse', HEAD + ':' + path).decode().strip()})

    old = git('show', BASE + ':' + PATHS[0]).decode()
    new = git('show', HEAD + ':' + PATHS[0]).decode()
    expected = old.replace('index_db::{FileWriteUnit, ReadOps}', 'index_db::{FileWriteUnit, IndexDb, ReadOps}')
    expected = expected.replace('    conn.execute(\n        "DELETE FROM resolution_dependencies WHERE file_path=?1",\n        [&file.rel_path],\n    )\n    .map_err(db_err)?;', '    IndexDb::execute_cached(\n        conn,\n        "DELETE FROM resolution_dependencies WHERE file_path=?1",\n        [&file.rel_path],\n    )?;')
    before = 'conn.execute("INSERT OR REPLACE INTO resolution_manifests(file_path,version,payload,digest) VALUES(?1,?2,?3,?4)",rusqlite::params![file.rel_path,m.version,payload,blake3::hash(payload.as_bytes()).to_hex().to_string()]).map_err(db_err)?;'
    after = before.replace('conn.execute(', 'IndexDb::execute_cached(conn,').replace(').map_err(db_err)?;', ')?;')
    assert expected.count(before) == 1
    expected = expected.replace(before, after)
    assert expected == new

    old = git('show', BASE + ':' + PATHS[1]).decode()
    new = git('show', HEAD + ':' + PATHS[1]).decode()
    expected = old.replace('use crate::{index_db::FileWriteUnit, sql_util::db_err};', 'use crate::{\n    index_db::{FileWriteUnit, IndexDb},\n    sql_util::db_err,\n};')
    before = 'conn.execute("INSERT INTO chunk_symbol_identity(chunk_id,file_path,doc_key,doc_version,symbol_id,format_version,record_json) VALUES(?1,?2,?3,?4,?5,?6,?7)",\n            rusqlite::params![identity.chunk_id,identity.file_path,identity.document.doc_key,identity.document.doc_version,identity.symbol_id,identity.format_version,serde_json::to_string(identity)?]).map_err(db_err)?;'
    after = before.replace('conn.execute(', 'IndexDb::execute_cached(conn,').replace(').map_err(db_err)?;', ')?;')
    assert expected.count(before) == 1
    expected = expected.replace(before, after)
    assert expected == new

    old = git('show', BASE + ':' + PATHS[2]).decode()
    new = git('show', HEAD + ':' + PATHS[2]).decode()
    anchor = '    assert_eq!(db.reads().list_file_paths().unwrap(), vec!["use.py"]);\n'
    start = new.index(anchor) + len(anchor)
    end = new.index('    db.writes().remove_files_batch(&["use.py".into()]).unwrap();', start)
    assert new[:start] + new[end:] == old
    extension = new[start:end]
    assert 'DependencyKind::NameBucket, "missing"' in extension
    assert 'DependencyKind::MissingPath, "api.py"' in extension
    assert '.is_empty()' in extension and 'replacement.py' in extension and 'drop(db);' in extension

    old = git('show', BASE + ':' + PATHS[3]).decode()
    new = git('show', HEAD + ':' + PATHS[3]).decode()
    start = new.index('\n\n    // The same writer must recover after rollback')
    end = new.index('\n}\n\n#[test]\nfn only_the_real_sql_surviving_declaration', start)
    assert new[:start] + new[end:] == old

    spec = importlib.util.spec_from_file_location('original_build_identity', REPO / 'scripts/p7_build_identity.py')
    identity = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(identity)
    snapshot = identity.source_snapshot(REPO)
    assert snapshot['source_commit'] == HEAD and snapshot['input_count'] == 1087
    (HERE / 'reviewed-production-source-snapshot.json').write_bytes(identity.json_bytes(snapshot))
    helper_path = 'crates/cc-db/src/index_db_rebuild.rs'
    assert git('show', BASE + ':' + helper_path) == git('show', HEAD + ':' + helper_path)
    report = {
        'decision': 'accepted_for_limited_engineering_validation_not_runtime_or_task_acceptance',
        'reviewer': 'root; source author scale_engineering',
        'base': BASE, 'reviewed_local_source_only_commit': HEAD,
        'source_input_count': snapshot['input_count'], 'source_manifest_sha256': snapshot['manifest_sha256'],
        'four_changed_paths': changes,
        'static_checks': {'exact_three_call_transform': True, 'all_original_test_code_preserved': True,
            'complete_existing_source_snapshot_matches_Git': True, 'shared_execute_cached_helper_unchanged': True},
        'findings': [
            'The same SQLite connection, literal SQL, parameters and operation order remain; both old execute and the existing helper map SQLite errors through db_err.',
            'The helper drops its temporary cached statement on return, before the existing survivor/readback proof; no connection, transaction, data-cache or generation semantics were changed.',
            'The complete symbol source/span/document checks, actual SQL survivor check and post-insert load_on validation remain byte-identical.',
            'The first test draft could miss stale reverse dependencies because final deletion masked them. The reviewed follow-up now checks both old reverse keys are absent after replacement/reopen and before final deletion.',
            'The two original test functions are extended; all prior statements and the duplicate-symbol survivor test remain unchanged.',
            'Both full staging and ordinary incremental writes use these helpers, so actual transaction/rollback/reopen and original whole parity regressions remain necessary.',
            'Measured staging time includes other work. This patch does not establish how much time was SQL compilation and cannot claim a speedup or deadline improvement.',
        ],
        'Cargo_executed': False, 'product_executed': False, 'performance_measured': False,
        'actual_publication_bridge_required': True, 'TODO_closed': 0, 'TODO_remaining': 29,
    }
    out = HERE / 'root-source-review.json'
    out.write_text(json.dumps(report, sort_keys=True, indent=2) + '\n')
    print(json.dumps({'decision': report['decision'], 'source_manifest_sha256': snapshot['manifest_sha256'], 'review_sha256': sha(out.read_bytes())}))

if __name__ == '__main__':
    main()
