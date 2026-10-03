"""Read-only verification of fixed/legacy real-API execution evidence."""
import hashlib
import json
import subprocess
from pathlib import Path

root = Path(__file__).resolve().parent
repo = root.parents[2]
r = json.loads((root / 'receipt.json').read_text())
for item in r['files']:
    p = repo / item['path']
    assert hashlib.sha256(p.read_bytes()).hexdigest() == item['sha256'], str(p)
assert r['checks']['strict_scoped_clippy'] == 'passed'
source = 'crates/cc-db/src/index_db_query.rs'
for sha, expected in r['source_sha256_by_commit'].items():
    assert hashlib.sha256(subprocess.check_output(['git', 'show', sha + ':' + source], cwd=repo)).hexdigest() == expected
assert subprocess.check_output(['git', 'show', r['production_source'] + ':' + source], cwd=repo) == (repo / source).read_bytes()
fixed = json.loads((root / 'fixed-results.json').read_text())
legacy = json.loads((root / 'legacy-results.json').read_text())
assert len(legacy) == 1 and legacy[0]['error'] == 'too many SQL variables'
assert legacy[0]['excluded_count'] == 50002
large = [v for v in fixed if v['case'] == 'fixed-large-API']
assert {v['excluded_count'] for v in large} == {50003, 200003}
assert all(v['returned_rows'] == 40 for v in large)
matrix = next(v for v in fixed if v['case'] == 'literal-matrix')
assert matrix['oracle_cases'] == 27 and matrix['NULL_path_rejected'] and matrix['sql_injection_no_mutation']
big = next(v for v in fixed if v['case'] == '50k-file-database')
assert big['files'] == 50000 and big['symbols'] == 100000 and big['baseline_absent']
assert next(v for v in fixed if v['case'] == 'embedded-NUL')['exact_set_semantics']
resource = json.loads((root / 'resource.json').read_text())
assert resource['exit_code'] == 0 and resource['max_rss_kib_linux'] > 0
assert r['status'] == 'bounded_support_not_full_V20'
print(json.dumps({'status':'passed','legacy_actual_overflow':True,'fixed_50k_file_database':True,'large_exclusion_max_count':200003,'peak_RSS_KiB':resource['max_rss_kib_linux'],'full_V20':False}))
