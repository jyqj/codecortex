"""Verify the independent acceptance deliverables without rerunning product."""
import gzip
import hashlib
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
receipt = json.loads((HERE / 'fresh-build-receipt.json').read_text())
archive = json.loads((HERE / 'archive-review.json').read_text())
summary = json.loads((HERE / 'live-summary.json').read_text())
assert receipt['binary_sha256'] == archive['new_binary_sha256'] == summary['binary_sha256']
assert receipt['source_sha'] == summary['source_sha'] == '574f7598662334c63e020da136c87f4f7281554d'
assert receipt['fresh_source_equals_production_c70']
assert receipt['compiler_artifact']['profile']['opt_level'] == '3'
assert sorted(receipt['compiler_artifact']['features']) == ['semantic', 'semantic-http']
for row in json.loads((HERE / 'live-capture-manifest.json').read_text()):
    data = (HERE / row['path']).read_bytes()
    assert hashlib.sha256(data).hexdigest() == row['capture_sha256']
    raw = gzip.decompress(data) if row['path'].endswith('.gz') else data
    assert len(raw) == row['original_bytes']
    assert hashlib.sha256(raw).hexdigest() == row['original_sha256']
for count in (1000, 50000):
    case = HERE / 'live' / f'n{count}'
    report = json.loads((case / 'independent-db-readback.json').read_text())
    assert report['files'] == count and report['testfiles'] == report['test_edges'] == report['FK_errors'] == 0
    assert report['integrity'] == 'ok'
    inputs = json.loads(gzip.decompress((case / 'source-inputs.json.gz').read_bytes()))
    assert len(inputs) == count
    cold = json.loads(gzip.decompress((case / 'cold-result.json.gz').read_bytes()))
    assert cold['result']['files_parsed'] == cold['result']['files_added'] == cold['result']['files_scanned'] == count
    assert cold['result']['files_skipped'] == 0 and not cold['result']['parse_errors']
    assert json.loads((case / 'final-status.json').read_text())['semantic_state'] == 'ready'
    assert json.loads((case / 'final-db.json').read_text())['counts']['semantic_manifest'] == count
    run = next(row for row in summary['scales'] if row['files'] == count)
    assert run['exit_code'] == 0
assert 'test result: ok. 10 passed; 0 failed' in (HERE / 'old-release-isolated.log').read_text()
assert 'test result: ok. 11 passed; 0 failed' in (HERE / 'new-release-final.log').read_text()
assert 'test result: ok. 175 passed; 0 failed; 1 ignored' in (HERE / 'cc-db-lib.log').read_text()
assert (HERE / 'fmt.log').read_text() == ''
assert 'Finished' in (HERE / 'independent-clippy.log').read_text()
print(json.dumps({'status': 'passed_independent_scoped_acceptance', 'production_source_sha': receipt['production_source_sha'],
                  'binary_sha256': receipt['binary_sha256'], 'live_scales': summary['scales'], 'full_V20': False}))
