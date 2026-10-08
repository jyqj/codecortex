from pathlib import Path, PurePosixPath
import datetime
import hashlib
import json
import os
import stat
import subprocess
import time

ROOT = Path('/workspace/scratch/28fef0db5e01/codecortex')
OUT = Path(__file__).resolve().parent
EXPECTED = '2e2b1e50775fc3df133c7fe72da1f9dc9d6bb55a'
ORIGINAL = ROOT.parent / 'integration-validation/round8-pins5-source-proof'
def digest(raw): return hashlib.sha256(raw).hexdigest()
def utc(): return datetime.datetime.now(datetime.timezone.utc).isoformat()
def git(*args, data=None):
    return subprocess.check_output(['git', *args], cwd=ROOT, input=data)
def identity():
    original = json.loads((ORIGINAL / 'receipt.json').read_text())
    result = dict(head=git('rev-parse', 'HEAD').decode().strip(),
                  tree=git('rev-parse', 'HEAD^{tree}').decode().strip(),
                  index_entries_sha256=digest(git('ls-files', '--stage', '-z')),
                  inputs={p: digest((ROOT / p).read_bytes()) for p in original['inputs_before']})
    if result['head'] != EXPECTED or result['tree'] != original['tree_before'] or result['inputs'] != original['inputs_before']:
        raise RuntimeError('Actual fixed PINS5 source changed')
    return result

if (OUT / 'materialization-receipt.json').exists() or (OUT / 'historical-recheck.log').exists():
    raise RuntimeError('Preserve previous observations; no overwriting')
receipt = dict(schema_version=1, kind='materialize_missing_tracked_historical_evidence_from_actual_PINS5',
               started_at_utc=utc(), source_before=identity(),
               original_failed_gate=dict(receipt_sha256=digest((ORIGINAL/'receipt.json').read_bytes()),
                                         log_sha256=digest((ORIGINAL/'02.log').read_bytes()),
                                         exit_code=1,
                                         error="missing artifacts/diagnostics in the sparse working tree"),
               missing_files=[], unchanged_existing_files=0,
               scope='Exact original tracked evidence materialization only; no code, guard, task, manifest or existing file changes.')
a = json.loads((ROOT/'docs/checkpoints/2026-10-03-packing-integration/source-manifest.json').read_text())
b = json.loads((ROOT/'docs/checkpoints/2026-10-03-fixed-e3-integration/imported-identity.json').read_text())
rows = {}
for row in a['evidence_files'] + b['files']:
    p = row['path']
    if p in rows and rows[p]['sha256'] != row['sha256']:
        raise RuntimeError('Conflicting historical hashes')
    rows[p] = row
paths = sorted(rows)
tree = {}
for line in git('ls-tree', '-r', '-z', EXPECTED, '--', *paths).split(b'\0'):
    if not line: continue
    meta, name = line.split(b'\t',1)
    mode, typ, sha = meta.decode().split()
    tree[name.decode()] = dict(mode=mode, type=typ, blob=sha)
if set(tree) != set(paths):
    raise RuntimeError('Historical paths not all tracked in actual PINS5')
body = git('cat-file','--batch',data=''.join(tree[p]['blob']+'\n' for p in paths).encode())
offset=0
validated=[]
for p in paths:
    entry=tree[p]
    if entry['type']!='blob' or entry['mode'] not in ('100644','100755'):
        raise RuntimeError('Non-regular historical Git entry')
    end=body.index(b'\n',offset)
    blob,typ,size=body[offset:end].decode().split()
    size=int(size);offset=end+1
    raw=body[offset:offset+size];offset+=size
    if blob!=entry['blob'] or typ!='blob' or body[offset:offset+1]!=b'\n':
        raise RuntimeError('Invalid Git batch response')
    offset+=1
    if digest(raw)!=rows[p]['sha256'] or ('bytes' in rows[p] and size!=rows[p]['bytes']):
        raise RuntimeError('Actual tracked artifact differs from original manifest: '+p)
    parts=PurePosixPath(p).parts
    if not parts or p.startswith('/') or '..' in parts or p.startswith('artifacts/checkpoints/localwidth4-100k-paired-20261003'):
        raise RuntimeError('Invalid or excluded path')
    target=ROOT
    for part in parts[:-1]:
        target=target/part
        if target.exists() or target.is_symlink():
            if target.is_symlink() or not target.is_dir(): raise RuntimeError('Non-directory parent')
    target=ROOT/p
    if target.exists() or target.is_symlink():
        if target.is_symlink() or not target.is_file() or target.read_bytes()!=raw:
            raise RuntimeError('Existing original file differs; refusing overwrite: '+p)
        receipt['unchanged_existing_files']+=1
    else:
        validated.append((p,entry,raw))
if offset!=len(body): raise RuntimeError('Extra Git batch response')
for p,entry,raw in validated:
    target=ROOT/p
    target.parent.mkdir(parents=True,exist_ok=True)
    with target.open('xb') as f: f.write(raw)
    target.chmod(0o755 if entry['mode']=='100755' else 0o644)
    if digest(target.read_bytes())!=rows[p]['sha256']: raise RuntimeError('Materialization readback failed')
    receipt['missing_files'].append(dict(path=p,git_blob=entry['blob'],git_mode=entry['mode'],bytes=len(raw),sha256=digest(raw)))
receipt.update(status='passed', required_unique_files=len(paths),materialized_files=len(validated),
               materialized_bytes=sum(len(v[2]) for v in validated),source_after=identity(),finished_at_utc=utc())
if receipt['source_before']!=receipt['source_after']: raise RuntimeError('Source or index entries changed')
(OUT/'materialization-receipt.json').write_text(json.dumps(receipt,indent=2)+'\n')
print(json.dumps({k:receipt[k] for k in ('status','required_unique_files','materialized_files','materialized_bytes','unchanged_existing_files')}),flush=True)
argv=['python3','-B','scripts/verify_historical_integrations_v2.py']
run=dict(schema_version=1,kind='original_failed_historical_gate_after_exact_evidence_materialization',
         argv=argv,cwd=str(ROOT),started_at_utc=utc(),source_before=identity(),status='running',
         original_failed_receipt_sha256=receipt['original_failed_gate']['receipt_sha256'],
         original_failed_log_sha256=receipt['original_failed_gate']['log_sha256'],
         materialization_receipt_sha256=digest((OUT/'materialization-receipt.json').read_bytes()),
         scope='Only the original failed command is re-executed; original 181-test and complete v15 proof successes are retained at identical source inputs. All original failure records remain unchanged.')
(OUT/'recheck-receipt.json').write_text(json.dumps(run,indent=2)+'\n')
started=time.monotonic()
with (OUT/'historical-recheck.log').open('xb') as log:
    result=subprocess.run(argv,cwd=ROOT,env=dict(os.environ,PYTHONDONTWRITEBYTECODE='1'),stdout=log,stderr=subprocess.STDOUT)
raw=(OUT/'historical-recheck.log').read_bytes()
run.update(exit_code=result.returncode,elapsed_seconds=time.monotonic()-started,
           finished_at_utc=utc(),source_after=identity(),log_bytes=len(raw),log_sha256=digest(raw),
           status='passed' if result.returncode==0 else 'failed')
if run['source_before']!=run['source_after']: raise RuntimeError('Source changed during recheck')
(OUT/'recheck-receipt.json').write_text(json.dumps(run,indent=2)+'\n')
print(json.dumps({k:run[k] for k in ('status','exit_code','elapsed_seconds','log_sha256')}),flush=True)
raise SystemExit(result.returncode)

