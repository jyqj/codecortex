"""Seal only this session's completed synthetic product runs."""
import gzip
import hashlib
import json
from pathlib import Path
import shutil
import sqlite3

HERE = Path(__file__).resolve().parent
LIVE = Path('/tmp/empty-test-review-live-product')
summary = json.loads((LIVE / 'summary.json').read_text())
assert [r['files'] for r in summary['scales']] == [1000, 50000]
assert all(r['exit_code'] == 0 and r['ready_manifests'] == r['files'] for r in summary['scales'])
target = HERE / 'live'
target.mkdir()
captures = []
for count in (1000, 50000):
    source = LIVE / f'n{count}'
    dest = target / f'n{count}'
    dest.mkdir()
    files = ['cold-result.json', 'final-db.json', 'final-status.json',
             'rpc.jsonl', 'resources.jsonl', 'http.jsonl', 'product-stderr.log', 'model.log']
    for name in files:
        original = source / name
        data = original.read_bytes()
        path = dest / (name + '.gz' if name.endswith('.jsonl') or name == 'cold-result.json' else name)
        if path.suffix == '.gz':
            path.write_bytes(gzip.compress(data, mtime=0))
        else:
            path.write_bytes(data)
        captures.append({'path': str(path.relative_to(HERE)), 'original': str(original),
                         'original_bytes': len(data), 'original_sha256': hashlib.sha256(data).hexdigest(),
                         'capture_sha256': hashlib.sha256(path.read_bytes()).hexdigest(), 'live_prefix': False})
    shutil.copyfile(source / 'repo/.codecortex.json', dest / 'config.json')
    sources = sorted((source / 'repo/src').glob('*.rs'))
    assert len(sources) == count
    manifest = [{'path': p.name, 'bytes': p.stat().st_size, 'sha256': hashlib.sha256(p.read_bytes()).hexdigest()}
                for p in sources]
    (dest / 'source-inputs.json.gz').write_bytes(gzip.compress(json.dumps(manifest).encode(), mtime=0))
    db_path = source / 'repo/.codecortex/index.sqlite3'
    with sqlite3.connect(f'file:{db_path}?mode=ro', uri=True) as db:
        report = {'files': db.execute('SELECT COUNT(*) FROM files').fetchone()[0],
                  'testfiles': db.execute('SELECT COUNT(*) FROM files WHERE is_test_file != 0').fetchone()[0],
                  'test_edges': db.execute('SELECT COUNT(*) FROM test_edges').fetchone()[0],
                  'integrity': db.execute('PRAGMA integrity_check').fetchone()[0],
                  'FK_errors': len(db.execute('PRAGMA foreign_key_check').fetchall()),
                  'epochs': dict(db.execute("SELECT key,value FROM metadata WHERE key LIKE '%epoch%'"))}
    assert report['files'] == count and report['testfiles'] == report['test_edges'] == report['FK_errors'] == 0
    assert report['integrity'] == 'ok'
    (dest / 'independent-db-readback.json').write_text(json.dumps(report, indent=2) + '\n')
shutil.copyfile(LIVE / 'summary.json', HERE / 'live-summary.json')
(HERE / 'live-capture-manifest.json').write_text(json.dumps(captures, indent=2) + '\n')
print(json.dumps({'status': 'captured_completed_live_runs', 'files': len(captures), 'scales': [1000, 50000]}))
