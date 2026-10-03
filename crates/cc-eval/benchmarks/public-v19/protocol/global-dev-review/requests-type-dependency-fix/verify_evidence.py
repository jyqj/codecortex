"""Offline hash, result and legacy-raw verification; no indexing/network."""
import pathlib,json,hashlib,tarfile,re,gzip
b=pathlib.Path(__file__).resolve().parent
m=json.loads((b/'artifact-manifest.json').read_text())
assert all(hashlib.sha256((b/p).read_bytes()).hexdigest()==h for p,h in m['file_sha256'].items())
r=json.loads((b/'evidence/index/index-receipt.json').read_text());assert r['source_sha']=='ee1988521e2125f86d2ff0aff8559dccc2a417b0' and r['search_calls']==r['provider_calls']==0 and len(r['tests'])==5
for t in r['tests']:
 assert t['status']=='pass' and t['files_parsed']==t['input_files']
 assert t['db_audit']['empty_keys']==t['db_audit']['punctuation_type_records']==t['db_audit']['duplicate_sites']==0
 assert t['db_audit']['manifest_count']==t['input_files']
assert r['tests'][-1]['input_files']==20
raw_logs=json.loads((b/'evidence/raw-test-log-sha256.json').read_text())
for name,h in raw_logs.items():assert hashlib.sha256(gzip.decompress((b/'evidence'/(name+'.gz')).read_bytes())).hexdigest()==h
s=gzip.decompress((b/'evidence/package-tests.log.gz').read_bytes()).decode();matches=re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored',s)
assert sum(int(t[0]) for t in matches)==563 and sum(int(t[1]) for t in matches)==0 and sum(int(t[2]) for t in matches)==3
assert 'Finished' in (b/'evidence/clippy.log').read_text() and not (b/'evidence/fmt.log').read_text()
with tarfile.open(b/'pr99-diagnosis.tar.gz') as t:
 prefix='crates/cc-eval/benchmarks/public-v19/protocol/global-dev-review/public-index-failure-diagnosis/'
 old=json.loads(t.extractfile(prefix+'artifact-manifest.json').read())
 for p,h in old['file_sha256'].items():assert hashlib.sha256(t.extractfile(prefix+p).read()).hexdigest()==h
assert len(old['file_sha256'])==18
print(json.dumps({'artifact_hashes':len(m['file_sha256']),'old_pr99_hashes':18,'index_tests_passed':5,'package_tests_passed':563,'existing_ignored':3,'new_calls':0}))
