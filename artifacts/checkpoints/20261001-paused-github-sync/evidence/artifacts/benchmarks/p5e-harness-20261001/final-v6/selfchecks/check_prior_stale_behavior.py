"""Read-only actual old raw contradiction, not a fake/new runtime witness."""
import json,pathlib,hashlib
old=pathlib.Path('artifacts/benchmarks/p5e-formal-runs-20261001-v1');checks=[]
for bits in ['100','110','111']:
 receipt=json.load(open(old/f'factorial-build/receipts/cell_{bits}.json'))
 for path in sorted((old/f'original51-factorial/exact--cell_{bits}/raw').glob('*.json')):
  raw=json.load(open(path));ret=raw.get('evidence_summary',{}).get('retrieval',{});lanes=ret.get('lane_receipts',ret.get('lanes',[]));matches=[l for l in lanes if l['lane_id']=='exact_symbol']
  if not matches:continue
  on=bits[1]=='1';lane=matches[0];passed=lane['status']=='complete' and lane['coverage']['complete'] is True and lane['candidate_count']>0 if on else lane['status']=='disabled' and lane['candidate_count']==0
  checks.append({'cell':'cell_'+bits,'registered_exact_on':on,'raw_path':str(path),'raw_sha256':hashlib.sha256(path.read_bytes()).hexdigest(),'binary_sha256':receipt['binary_sha256'],'actual_exact_lane':lane,'would_pass_exact_control_binding':passed})
assert checks and all(not c['would_pass_exact_control_binding'] for c in checks if c['registered_exact_on'])
p=pathlib.Path(__file__).with_name('PRIOR-STALE-BEHAVIOR-REJECTION.json');assert not p.exists();p.write_text(json.dumps({'status':'actual_old_stale110111rejected_by_required_on_complete_nonzero_domain','checks':checks,'limits':['no compilednewwitness claimed;actualnew3factorfixture stillmustexecute','oldcounterfactual48trusted0 preserved','doesnotreplacefull111policy/body/proof equivalence or freshsourcebuild']},indent=2)+'\n');print('old actual110/111 disabled => requiredon witness reject')
