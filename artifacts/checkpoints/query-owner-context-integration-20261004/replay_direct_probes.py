"""Replay only the accepted 166 direct probes; no API/production process targets."""
from pathlib import Path
import argparse
import hashlib
import json
import subprocess
import tarfile

ROOT = Path(__file__).resolve().parents[3]
EVIDENCE = Path(__file__).resolve().parent
ARCHIVE = ROOT / 'artifacts/reviews/query-owner-closing-qualifiers-independent-20261004/evidence.tar.gz'
ARCHIVE_SHA = '2fb0201a379ef450078a4b0b7164308b7d95852d596dab1b14153998179c275b'

def sha(p):
    return hashlib.sha256(p.read_bytes()).hexdigest()

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--owned-root', type=Path, required=True)
parser.add_argument('--target-dir', type=Path, required=True)
parser.add_argument('--rustc', type=Path, required=True)
args = parser.parse_args()
r = args.owned_root.resolve()
r.mkdir(parents=True, exist_ok=True)
assert sha(ARCHIVE) == ARCHIVE_SHA
with tarfile.open(ARCHIVE) as archive:
    for name in ['probe-delivery.rs', 'probes.json', 'delivery-probes.json', 'previous-probes.json']:
        member = archive.getmember(name)
        assert member.isfile()
        (r / name).write_bytes(archive.extractfile(member).read())
s = (r / 'probe-delivery.rs').read_text()
s = s.replace('/workspace/codecortex/crates/', str(ROOT / 'crates') + '/')
s = s.replace('/workspace/independent-closing-review/probes.json', str(r / 'probes.json'))
(r / 'probe-integration.rs').write_text(s)
deps = args.target_dir.resolve() / 'debug/deps'
libs = {}
for name in ['cc_model', 'cc_db', 'serde_json']:
    candidates = list(deps.glob('lib' + name + '-*.rlib'))
    if name == 'serde_json':
        candidates = [p for p in candidates if 'float_roundtrip' in json.loads(
            (args.target_dir / 'debug/.fingerprint' / p.stem[3:] / 'lib-serde_json.json').read_text())['features']]
    assert len(candidates) == 1, (name, candidates)
    libs[name] = candidates[0]
cmd = [str(args.rustc), '--edition', '2021', str(r / 'probe-integration.rs'),
       '-L', 'dependency=' + str(deps), '-o', str(r / 'probe-integration')]
for name, p in libs.items():
    cmd += ['--extern', name + '=' + str(p)]
with (EVIDENCE / 'independent-probes-link.log').open('w') as f:
    result = subprocess.run(cmd, stdout=f, stderr=subprocess.STDOUT)
receipt = {'argv': cmd, 'exit': result.returncode,
           'libraries': {n: {'path': str(p), 'sha256': sha(p)} for n, p in libs.items()}}
assert result.returncode == 0
with (EVIDENCE / 'independent-probes.json').open('w') as f:
    result = subprocess.run([str(r / 'probe-integration')], stdout=f)
receipt['run_exit'] = result.returncode
receipt['binary_sha256'] = sha(r / 'probe-integration')
assert result.returncode == 0
fresh = json.loads((EVIDENCE / 'independent-probes.json').read_text())
archived = json.loads((r / 'delivery-probes.json').read_text())
previous = {x['query']: x for x in json.loads((r / 'previous-probes.json').read_text())}
inputs = json.loads((r / 'probes.json').read_text())
assert fresh == archived and len(fresh) == len(inputs) == 166
for p, x in zip(inputs, fresh):
    assert p['query'] == x['query']
    y = previous[p['query']]
    assert x['target'] == y['target']
    assert [(f['target_owner'], f['target_member']) for f in x['flags']] == [(f['target_owner'], f['target_member']) for f in y['flags']]
    assert all(f['other'] for f in x['flags'])
    assert all(f['owner'] for f in x['flags'] if f['kind'] in ['method', 'function', 'unknown'])
    assert len({f['owner'] for f in x['flags'][:6]}) == 1
    assert x['flags'][0]['owner'] == (p['comparative'] or p['explicit_bypass'])
    if p['explicit_bypass']:
        assert x == y
receipt.update(status='passed', direct_probes=len(fresh),
               explicit_bypass_cases=sum(p['explicit_bypass'] for p in inputs),
               comparison_scope='fresh integration production-module probes vs pinned contract and archived accepted outputs; previous outputs historical, not rebuilt',
               input_sha256=sha(r / 'probes.json'), wrapper_sha256=sha(r / 'probe-integration.rs'),
               accepted_archive_sha256=ARCHIVE_SHA)
(EVIDENCE / 'independent-probes-receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')
(EVIDENCE / 'independent-probe-wrapper.rs').write_text(s)
(EVIDENCE / 'independent-probe-inputs.json').write_bytes((r / 'probes.json').read_bytes())
print(json.dumps({k: receipt[k] for k in ['status', 'direct_probes', 'explicit_bypass_cases', 'exit', 'run_exit']}))
