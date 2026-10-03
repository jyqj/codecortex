"""Read-only diagnosis of frozen PR67 dev raw; never reads public-v19 gold."""
import pathlib,json,hashlib,collections,subprocess,re
repo=pathlib.Path('/workspace/codecortex');frozen=repo/'artifacts/checkpoints/cloud-p7-v19-current-local-20261003';run=frozen/'run';out=pathlib.Path('/tmp/p7-baseline-diagnosis-78b0');out.mkdir(exist_ok=False)
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
for name,digest in json.load(open(frozen/'artifact-manifest.json')).items():assert sha(frozen/name)==digest,name
partition=collections.Counter();classes=collections.Counter();records=[];quality=[];s11=[];hits_verified=0;material=[]
for suite in sorted(run.glob('p*')):
 if not suite.is_dir() or not (suite/'manifest.json').exists():continue
 m=json.load(open(suite/'manifest.json'));origin=repo/'crates/cc-eval/benchmarks/manifests'/(suite.name+'.json');source=(origin.parent/m['suite']['source']['root']).resolve();files=set(m['suite']['source']['files']);queries={q['id']:q for q in map(json.loads,open(suite/'queries.jsonl'))};rows=list(map(json.loads,open(suite/'normalized.jsonl')));scores=list(map(json.loads,open(suite/'scores.jsonl')))
 assert len(rows)==len(scores)
 for row,score in zip(rows,scores):
  q=queries[row['case_id']];raw=json.load(open(suite/row['raw_path']));e=raw['evidence_summary'];ret=e['retrieval'];lanes=ret.get('lanes',ret.get('lane_receipts',[]));packing=e['packing']['partial'];bad=[{'lane':l['lane_id'],'state':l['status'],'reason':l.get('truncation_reason'),'candidate_count':l['candidate_count'],'coverage':l['coverage']} for l in lanes if l['status'] in ['partial','timeout','unavailable','error','cancelled']]
  assert e['source_freshness']['partial'] is False
  for h in row['hits']:
   assert h['path'] in files
   data=(source/h['path']).read_bytes();a,b=h['span']['start'],h['span']['end'];assert 0<=a<b<=len(data)
   assert data[a:b].decode('utf8')==h['text'],(suite.name,row['case_id'],h['path'])
   assert h['start_line']==data[:a].count(b'\n')+1
   assert h['end_line']==data[:b-1].count(b'\n')+1
   assert h['evidence_valid'] is True;hits_verified+=1
  if q['id']=='R09':
   assert not q['no_answer'];assert all(q['query'].encode() not in (source/a['path']).read_bytes() for g in q['answers'] for a in g['alternatives']);classification='invalid_material';material.append({'id':q['id'],'repetition':row['repetition'],'source_sha256':sha(source/q['answers'][0]['alternatives'][0]['path']),'identifier_count':0,'gold_snapshot_sha256':sha(suite/'queries.jsonl'),'status_retained':row['status'],'native_top1_retained':score['top1']})
  elif row['status']=='partial':
   assert packing or bad;classification='permitted_partial_quality_inconclusive';partition[('packing_and_lane' if packing and bad else 'packing_only' if packing else 'lane_only')]+=1
  elif q['no_answer']:
   assert row['status'] in ['no_match','success'];classification='complete_no_answer_observation' if not row['hits'] else 'complete_false_positive_observation'
  elif score['top1']==0:
   classification='complete_positive_recall_miss_observation'
   quality.append({'suite':suite.name,'id':q['id'],'repetition':row['repetition'],'query':q['query'],'status':row['status'],'hits':len(row['hits']),'gold_paths':[a['path'] for g in q['answers'] for a in g['alternatives']],'interpretation':'Valid authored dev expectation; actual local recall miss. Not relabelled as invalid gold. No independent hard-correctness implementation violation confirmed; natural-language semantic equivalence cannot be guaranteed by this current Local setup.'})
  else:classification='complete_positive_primary_observation'
  classes[classification]+=1
  record={'suite':suite.name,'case_id':q['id'],'repetition':row['repetition'],'status':row['status'],'classification':classification,'packing_partial':packing,'lane_incompleteness':bad,'retained_native_top1':score['top1'],'retained_native_ndcg10':score['ndcg10'],'raw_sha256':sha(suite/row['raw_path']),'raw_path':str((suite/row['raw_path']).relative_to(repo)),'source_text_span_valid':True}
  records.append(record)
  if q['id']=='S11':
   assert q['no_answer'] and not row['hits'] and row['status']=='no_match' and not packing and not bad
   assert raw['machine_pack']['hits']==[] and raw['spans']==[]
   assert all(q['query'].encode() not in (source/f).read_bytes() for f in files)
   # Positive fixture retrieval witnesses index availability, not mere emptiness.
   positives=[r for r,s in zip(rows,scores) if r['case_id']=='S01' and s['top1']==1.0]
   assert len(positives)==3;s11.append({**record,'absent_in_all_locked_files':True,'same_fixture_positive_S01_rows':3,'no_truncation_or_freshness_fault':True,'scope':'one exact absent-token fixture; not universal no-answer calibration'})
assert len(records)==164 and hits_verified==279
assert dict(partition)=={'packing_and_lane':33,'packing_only':21,'lane_only':9}
assert len(material)==3 and len(s11)==3
for label,rows in [('rows',records),('complete-positive-misses',quality),('S11-complete-absence',s11),('R09-invalid-material',material)]:
 (out/(label+'.json')).write_text(json.dumps(rows,ensure_ascii=False,indent=2)+'\n')
summary={'fixed_PR67_sha':'78b0ce52cb2ff4c53c53893b9f7dd469269806b8','executed_local_baseline_source':'6f3cee1f2d23b43af5271bcaeaea67b931fd4fb7','raw_rows':164,'classification_counts':dict(classes),'permitted_partial_partition':dict(partition),'actual_source_hit_byte_and_line_audits':hits_verified,'invalid_source_bytes_or_spans':0,'complete_positive_recall_miss_question_ids':sorted({x['id'] for x in quality}),'complete_positive_recall_miss_questions':len({(x['suite'],x['id']) for x in quality}),'partial_top1_zero_question_ids':sorted({r['case_id'] for r in records if r['status']=='partial' and r['retained_native_top1']==0}),'confirmed_production_hard_correctness_counterexamples':0,'conclusion':'Two gate failures arise from permitted incomplete packing/retrieval coverage and correctly remain failed/inconclusive. Seven valid complete positive NoMatch dev questions are real Local recall gaps; not invalid gold and not quality pass. R09 is separate invalid material; S11 actual complete absence with positive indexing witness. No minimal production repair is justified by these observed rows alone.','production_or_scorer_or_gold_changed':False,'new_public_v19_holdout_or_gold_read':False,'future_migration_versions_invented':False}
(out/'summary.json').write_text(json.dumps(summary,ensure_ascii=False,indent=2)+'\n');print(json.dumps(summary,ensure_ascii=False))
