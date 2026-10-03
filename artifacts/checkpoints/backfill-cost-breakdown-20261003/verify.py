#!/usr/bin/env python3
"""Evidence math/identity checks; no production, workspace tests or heldout."""
import hashlib, json, pathlib, subprocess
OUT=pathlib.Path(__file__).resolve().parent
ROOT=OUT.parents[2]
a=json.loads((OUT/'analysis.json').read_text())
manifest=json.loads((OUT/'inputs-manifest.json').read_text())
for key,expected in manifest.items():
    data=subprocess.check_output(['git','show',key],cwd=ROOT)
    assert len(data)==expected['bytes'] and hashlib.sha256(data).hexdigest()==expected['sha256'],key
assert a['http']['requests']==29696 and a['http']['observed_peak_inflight']==1
assert a['measurement_window']['returned']+a['cleanup_tail']['returned']==29696
assert a['status']['requests']==sum(a['status']['state_counts'].values())==293
assert sum(w['returned'] for w in a['windows_30s'])==a['measurement_window']['returned']
assert abs(sum(v['wall_s'] for v in a['resource_by_poll_exposure'].values())-a['measurement_window']['sample_wall_s'])<1e-6
assert a['binary_receipt']['source_sha']==a['source_sha']
assert a['binary_receipt']['binary_sha256']=='1dc3dbff22f9fda053c7e7eac6d11d27f0a7271cec28b902057e632be21b9b27'
s=json.loads((OUT/'sql-probe.json').read_text())
assert {r['n'] for r in s['results']}=={1000,5000}
assert all(r['vm_steps_one_query']==9*r['pending']+17 for r in s['results'])
changes=subprocess.check_output(['git','diff','--name-only','a8452be903e1a88c935e267f5d76879ce7ee2716'],cwd=ROOT,text=True).splitlines()
assert all(p.startswith('artifacts/checkpoints/backfill-cost-breakdown-20261003/') for p in changes),changes
v={'status':'passed_evidence_identity_and_math_checks','product_outcome':'original_100k_failed_unchanged','product_executions':0,'real_provider_requests':0,'new_scope':'artifacts/checkpoints/backfill-cost-breakdown-20261003/','inputs_verified':len(manifest),'sql_probe_scales':[1000,5000]}
(OUT/'verification.json').write_text(json.dumps(v,indent=2)+'\n')
print(json.dumps(v))
