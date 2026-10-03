#!/usr/bin/env python3
"""Package synthetic evidence only; runtime and generated repo remain local."""
import gzip,hashlib,json,pathlib,shutil,sys
sys.dont_write_bytecode=True
OUT=pathlib.Path(__file__).resolve().parent;CASE=OUT/'live/n100000'
def read(p):
    if p.exists():return json.loads(p.read_text())
    with gzip.open(str(p)+'.gz','rt') as f:return json.load(f)
def write(p,v):p.write_text(json.dumps(v,ensure_ascii=False,indent=2)+'\n')
s=read(OUT/'summary.json');a=read(OUT/'rpc-http-analysis.json');resources=read(OUT/'resource-analysis.json');build=read(OUT/'build-receipt.json')
checks={c['name']:c['result'] for c in s['checks']};cold=checks.get('cold_index',{});boundary=s.get('deadline_300s_db',{});tail=s.get('cleanup_tail_db',{})
failure_brief=[{k:r.get(k) for k in ['phase','type','error']} for r in s['failures']]
root_peak=max((d['root_rss_max_bytes'] or 0 for d in resources['phases'].values()),default=0)
def totals(p):
    n=logical=allocated=0
    for f in p.rglob('*'):
        if f.is_file():
            t=f.stat();n+=1;logical+=t.st_size;allocated+=t.st_blocks*512
    return {'files':n,'logical_bytes':logical,'allocated_bytes':allocated}
write(OUT/'retained-local-data.json',{'generated_repo':totals(CASE/'repo'),'semantic_cache':totals(CASE/'cache'),'build_runtime':totals(OUT/'runtime'),'note':'Retained locally; not committed. Synthetic input corpus is completely specified by source(i,0), with all 100k file hashes committed.'})
ready=checks.get('ready_manifest_integrity_fk');product=s['status']
lines=[f'# Independent status v2 fixed 100k release observation', '',f'Product outcome: **{product}**. One formal 100k sample; full V20 and semantic quality are not claimed. Independent correctness review is separate.', '',f'- Production: `11af963c33cfa68cc9497e355464c1d6d058adac`; final source/build: `29b03a0fec6bfde92aac9c41dce996ebb2d08cc7`. No production code difference between these commits.',f'- Fresh target release build: {build["wall_seconds"]:.3f}s, exit {build["build_exit_code"]}, opt-level 3, no debug assertions; semantic + semantic-http.',f'- Binary SHA256: `{s["binary_sha256"]}`.',f'- Cold index: {cold.get("wall_seconds",0):.6f}s; 100000 scanned/parsed/added, no skipped/parse errors.',f'- Status observations: {a["status_observations"]}; status errors: {a["status_error_count"]}. Status latency p50/p95/max (ms): {a["rpc_timings"].get("status")}.',f'- Root sampled RSS maximum: {root_peak} bytes ({root_peak/1024**2:.2f} MiB). Children files unavailable: full tree RSS **unknown**, never zero-filled.',f'- Normal process exits: {s.get("product_exits",[])}.','']
if ready:lines += [f'Ready observed at {ready["drain_wall_seconds"]:.6f}s with complete manifests, integrity/FK checks; bounded repeated/distinct query stability and reopen checks are recorded in summary.','']
else:lines += [f'No accepted ready observation within 300s. Failure: {failure_brief}.',f'Boundary read-only snapshot began at {boundary.get("snapshot_elapsed_drain_seconds")}s; counts: {boundary.get("counts")}. This is an actual timestamped near-boundary observation, not asserted to be an exact 300.000000s count.',f'Cleanup-tail inspection completed at {tail.get("elapsed_drain_seconds")}s: counts {tail.get("counts")}; integrity {tail.get("integrity")}, FK errors {tail.get("foreign_key_errors")}. Tail is outside the deadline and does not pass readiness.',f'Not run: {s["not_run"]}. Failure cleanup normal EOF is separate from the gated post-ready normal-exit test.','']
lines += ['Fixed protocol: 100000 source files, source(i,value=0), 128 dimensions, fake loopback HTTP, batch16, semantic concurrency4/per-project2, parse4, 200ms poll gap, 300s readiness, original RPC/overall/resource limits. Raw RPC, HTTP and 20ms resource logs are retained compressed. V2 observations explicitly consume point-in-time generation/identity/service scopes, not response-time latest or query authorization.','',f'HTTP counts split by monotonic deadline: {a["http_counts"]}. HTTP returned inputs do not imply committed manifests. Configured max_batch_items is 16; every actual request contained 1 input in this run.','', 'Original PR101 failure at `a8452be903e1a88c935e267f5d76879ce7ee2716` remains failed: cold 21.638s, not ready within 300s, cleanup semantic_manifest 29696. Neither author 1k/5k in-process AB nor this single observation certifies MCP quality/full V20 or proves relative speed across environments.', '', 'Real provider calls: 0. No heldout, private source transfer, GC/WAL fault or kill experiment. Dummy bearer is synthetic and loopback-only. HOME unchanged. Default rustup read-only path error and gh CLI Forbidden are recorded; rejected action was not retried. GitHub App identity lookup succeeded as jyqj (noUsername=false).', '', 'Protocol and evidence:', '', '- `protocol.json`, `protocol-diff.patch`, `baseline-run.py`, `run.py`: frozen scope and derivation.', '- `build-receipt.json`, compressed Cargo/build logs: build identity.', '- `preregistration.json`, `summary.json.gz`, `environment.json`: run outcome and timing.', '- `live/n100000/*.jsonl.gz`: input manifest, original RPC/HTTP/resource observations.', '- `live/n100000/deadline-300s-db.json`, `cleanup-tail-db.json`: separately timed counts.', '- `resource-analysis.json`, `rpc-http-analysis.json`, `verification.json`, `analyze.py`: replayable evidence checks. These verify evidence retention/identity, not independent product correctness.', '- `retained-local-data.json`: local runtime/corpus/database retention. Generated corpus, database/cache and build tree are excluded from Git.','']
(OUT/'README.md').write_text('\n'.join(lines))
(OUT/'pr-body.md').write_text('\n'.join(lines[:lines.index('Protocol and evidence:')]))
for p in list(OUT.glob('*.json'))+list(OUT.glob('*.jsonl'))+list(CASE.glob('*.json'))+list((CASE/'reopen').glob('*.json')):
    if p.suffix=='.jsonl' or p.stat().st_size>100000:
        with p.open('rb') as src,gzip.open(str(p)+'.gz','wb') as dst:shutil.copyfileobj(src,dst)
        p.unlink()
files=[p for p in OUT.iterdir() if p.is_file() and p.name not in ['curated-manifest.json','SHA256SUMS']]
files += [p for p in CASE.rglob('*') if p.is_file() and not any(x in ['repo','cache'] for x in p.relative_to(CASE).parts)]
files=sorted(set(files));manifest=[{'path':str(p.relative_to(OUT)),'bytes':p.stat().st_size,'sha256':hashlib.sha256(p.read_bytes()).hexdigest()} for p in files]
write(OUT/'curated-manifest.json',manifest)
(OUT/'SHA256SUMS').write_text(''.join(r['sha256']+'  '+r['path']+'\n' for r in manifest))
print(json.dumps({'curated_files':len(manifest),'curated_bytes':sum(r['bytes'] for r in manifest),'product_outcome':product,'root_peak_bytes':root_peak}))
