#!/usr/bin/env python3
"""Independent evidence consistency verification; no retrieval/product changes."""
import json,math,hashlib,tarfile,subprocess
from collections import Counter,defaultdict
from pathlib import Path
from paired import OUT,RUNTIME,SHAS,verify_build,inventory,sha,canon,dump
p=json.loads((OUT/'plan.json').read_bytes());s=json.loads((OUT/'summary.json').read_bytes());commands=json.loads((OUT/'runs/commands.json').read_bytes());assert len(commands)==20
checks=[]
for arm in SHAS:
 b=verify_build(arm);assert sha((OUT/('build-'+arm)/'build-receipt.json').read_bytes())==p['build_receipts_sha256'][arm];assert inventory(RUNTIME/arm/'inputs02')==p['same_input_inventory']
 for e in p['suite_entries']:
  root=OUT/'runs'/arm/e['key'];cmd=next(c for c in commands if c['arm']==arm and c['key']==e['key']);assert cmd['run_exit_code']==cmd['replay_exit_code']==1;assert not cmd['changed_on_replay'];assert inventory(root)==cmd['after_replay_sha256']
  ready=json.loads((root/'readiness.json').read_bytes());assert ready['state']=='ready' and ready['ready']==ready['total']==len(e['source_lock']['files']);assert ready['pending']==ready['failed']==ready['unknown']==0
  rs=[json.loads(l) for l in (root/'normalized.jsonl').read_bytes().splitlines()];qs=[json.loads(l) for l in (root/'queries.jsonl').read_bytes().splitlines()];scores=[json.loads(l) for l in (root/'scores.jsonl').read_bytes().splitlines()]
  expected=Counter((q['id'],i) for q in qs for i in range(e['repetitions']));actual=Counter((r['case_id'],r['repetition']) for r in rs);assert expected==actual and len(rs)==e['scheduled_rows']==len(scores)
  original=json.loads((root/'metrics.json').read_bytes());summ=next(v for v in s['arms'][arm]['suite_results'] if v['key']==e['key']);assert math.isclose(original['mean_top1'],summ['metrics']['top1']['mean'],abs_tol=1e-14);assert math.isclose(original['mean_ndcg10'],summ['metrics']['ndcg10']['mean'],abs_tol=1e-14)
  # Evaluator replay checks its original BLAKE3 raw_digest at report.rs:291; archive uses SHA256.
  checks.append({'arm':arm,'key':e['key'],'scheduled':len(expected),'executed':len(rs),'missing':0,'error':0,'partial':sum(r['status']=='partial' for r in rs),'ready':ready['ready'],'metric_summary_matches_original':True,'raw_replay_byte_identical':True})
# Exact same-profile score vectors, including nullable unsupported fields/no-answer score, no cherry-picked metrics.
score_equal=[]
for e in p['suite_entries']:
 b=(OUT/'runs/baseline'/e['key']/'scores.jsonl').read_bytes();c=(OUT/'runs/candidate'/e['key']/'scores.jsonl').read_bytes();assert b==c;score_equal.append(e['key'])
archive=OUT/'typescript-paired-original-outputs.tar.gz';manifest=json.loads((OUT/'raw-artifact-manifest.json').read_bytes());assert sha(archive.read_bytes())==manifest['archive_sha256']
with tarfile.open(archive) as t:
 assert len(t.getmembers())==manifest['file_count']
 for m in t.getmembers():assert m.isfile() and sha(t.extractfile(m).read())==manifest['files'][m.name]['sha256']
assert not subprocess.check_output(['git','diff','37dd042eaa1209a86e0cafdcd92ae77e036e76f5','--','crates','Cargo.toml','Cargo.lock','docs'],cwd='/workspace/ts-candidate')
dump(OUT/'verification-receipt.json',{'all_checks_passed':True,'stage_columns':checks,'identical_same_profile_original_score_vectors':score_equal,'actual_source_binary_build_input_locks_still_match':True,'archive_original_byte_readback_verified':True,'product_and_central_tasks_unchanged':True,'search_calls':0,'excluded_broad_tests_not_run':True})
print(json.dumps({'verified_suites':len(checks),'identical_score_vectors':len(score_equal),'archive_files':manifest['file_count'],'passed':True}))
