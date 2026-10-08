"""Materialize exact read-only git inputs inside this checkpoint only."""
import hashlib, json, pathlib, subprocess, sys, re
ROOT = pathlib.Path(__file__).resolve().parent
REPO = ROOT.parents[2]
REFS = {'base': 'ee4c4fc0b41e298bf38b8269310053fa4b355c61',
        'candidate': '098ebd9c08031e0b652e7b021d8abbc9e8b19c3d'}
variant = sys.argv[1]
ref = REFS['base' if variant == 'base' else 'candidate']
def git(*args):
    return subprocess.check_output(['git', '-C', str(REPO), *args])
dest = ROOT / 'local/source'
files = git('ls-tree', '-r', '--name-only', ref).decode().splitlines()
hashes = {}
for f in files:
    if not (f in ['Cargo.toml', 'Cargo.lock'] or
            any(f.startswith('crates/'+c+'/') and (f.endswith('Cargo.toml') or '/src/' in f) for c in ['cc-db', 'cc-model'])):
        continue
    data = git('show', ref+':'+f)
    hashes[f] = hashlib.sha256(data).hexdigest()
    p = dest/f
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_bytes(data)
workspace = (dest/'Cargo.toml').read_text()
workspace = re.sub(r'members = \[.*?\]', 'members = ["crates/cc-db", "crates/cc-model"]', workspace, count=1, flags=re.S)
(dest/'Cargo.toml').write_text(workspace)
p = dest/'crates/cc-db/src/direct_writer.rs'
if variant == 'mutant-no-triggers':
    data = p.read_text()
    needle = '        // Read names and SQL from SQLite rather than parsing CREATE INDEX.'
    assert data.count(needle) == 1
    # A narrow, real mutation: schema executes correctly but all triggers are removed.
    data = data.replace(needle, '''        let trigger_names: Vec<String> = conn.prepare("SELECT name FROM sqlite_schema WHERE type='trigger'").unwrap()
            .query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
        for name in trigger_names {
            conn.execute_batch(&format!("DROP TRIGGER \\\"{}\\\"", name.replace('"', "\\\"\\\""))).unwrap();
        }
''' + needle)
    p.write_text(data)
elif variant == 'mutant-split':
    data = p.read_text()
    start = data.index('fn split_sql_statements(sql: &str) -> Vec<&str> {')
    end = data.index('\n#[cfg(test)]', start)
    data = data[:start] + '''fn split_sql_statements(sql: &str) -> Vec<&str> {
    sql.split(';').map(str::trim).filter(|s| !s.is_empty()).collect()
}
''' + data[end:]
    p.write_text(data)
elif variant == 'mutant-no-nul':
    data = p.read_text()
    a = data.index('        // SQLite\'s C API stops at NUL;')
    b = data.index('        let conn = Connection::open(path)', a)
    p.write_text(data[:a]+data[b:])
elif variant not in REFS:
    raise ValueError(variant)
record = dict(variant=variant, commit=ref, original_sha256=hashes,
              built_writer_sha256=hashlib.sha256(p.read_bytes()).hexdigest(),
              workspace_adaptation='members limited to cc-db and cc-model; source files otherwise exact except named mutants')
(ROOT/(variant+'-source.json')).write_text(json.dumps(record, indent=2)+'\n')
print(json.dumps({'variant': variant, 'commit': ref, 'files': len(hashes), 'writer_sha256': record['built_writer_sha256']}))
