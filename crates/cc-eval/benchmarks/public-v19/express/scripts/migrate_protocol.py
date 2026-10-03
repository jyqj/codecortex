#!/usr/bin/env python3
"""Versioned pre-ranking migration. Never persists or prints holdout bodies.
Historical public records remain exposed; hashes do not provide custody.
"""
import copy,hashlib,json,subprocess
from pathlib import Path
P=Path(__file__).resolve().parents[1]
OLD_COMMIT='2cf6495f5115f4694d219ced5a09abda3f0c8be1'
PREFIX=b'codecortex-public-v19-split-v1\n'
def sha(b):return hashlib.sha256(b).hexdigest()
def serial(i):return f'v19.express.f{i:04d}'
def write(p,x):p.write_text(json.dumps(x,ensure_ascii=False,indent=2)+'\n')
def jsonl(rows):return ''.join(json.dumps(r,ensure_ascii=False,separators=(',',':'))+'\n' for r in rows).encode()
raw=subprocess.check_output(['git','show',OLD_COMMIT+':crates/cc-eval/benchmarks/public-v19/express/blocks/block-100/questions.jsonl'])
old=[json.loads(s) for s in raw.splitlines()]; assert len(old)==100
families={r['query_family']:serial(i) for i,r in enumerate(old,1)}
# Candidate local positive/negative counterparts; global reviewer must adjudicate.
pairs=[('location-encoding','redirect-back-trap'),('node-server-listen','no-http2-listener')]
components=[]; canonical={f:f for f in families.values()}
for keys in pairs:
 members=sorted(families['express-v19-'+k] for k in keys);c=min(members)
 components.append({'global_family':c,'members':members})
 for m in members:canonical[m]=c
write(P/'relations.json',{'protocol_version':1,'status':'candidate_local_associations_pending_designated_global_reviewer','components':components})
cat={'exact/API':'api_usage','behavior':'semantic_feature','architecture/facets':'architecture_understanding','crossfile-chain':'call_chain','config/error':'error_handling','hardnegative/noanswer':'component_location'}
rows=[]; aliases=[]
for i,r in enumerate(old,1):
 q=copy.deepcopy(r); f=serial(i); component=canonical[f];split='holdout' if int.from_bytes(hashlib.sha256(PREFIX+component.encode()).digest()[:8],'big')<2**62 else 'dev';a=q['annotations']
 facets=[]
 for j,g in enumerate(q['answers']):
  g['primary']=(j==0);g['grade']=3 if j==0 else 2;facets.append({'id':'required-'+str(j),'group_id':g['id'],'required':True})
 a['status']='candidate_pending_independent_review';a['family_sha256']=sha(f.encode());a['split_status']='protocol_v1_candidate_not_global_freeze'
 a['v19']={'protocol_version':1,'repo_id':'express','source_sha':a['source_sha'],'global_family':component,'intent':q['query'],'query_language':'en','hard_scope':{'path_prefix':q['path_prefix']},'facets':facets,'graph_constraints':a['chain_edges'] if not q['no_answer'] else [],'mutation_profile':'none','review_status':'pending' if split=='dev' else 'holdout_custody_blocked','author_id':'B/express','reviewer_id':None,'original_category':q['category'],'related_candidate_cluster':a['related_family_cluster'],'primary_designation':'first source obligation primary grade3; other distinct required evidence grade2, not substitute primaries'}
 q['annotations']={'v19':{**a['v19'],'author_provenance':{k:v for k,v in a.items() if k!='v19'}}}
 q.update(id=f+'.en01',query_family=f,split=split,category=cat[q['category']])
 if r['query_family']=='express-v19-production-view-cache-default':q['category']='configuration_lookup'
 aliases.append({'original_draft_serial':i,'old_family':r['query_family'],'new_family':f,'global_family':component,'old_split':r['split'],'new_candidate_split':split,'old_row_sha256':sha(jsonl([r])),'new_candidate_row_sha256':sha(jsonl([q])),'reason':'monotonic original draft order; fixed protocol threshold, no ranking observed','public_exposure_commit':OLD_COMMIT,'custody_status':'exposed_quarantined_for_confirmatory_holdout' if split=='holdout' else 'public_dev_pending_review'})
 rows.append(q)
write(P/'provenance/id-split-migration.json',{'protocol_commit':'03abe24950f1fa89de3cce0a4e1d44d01e9aa0d6','original_public_commit':OLD_COMMIT,'original_file_sha256':sha(raw),'ordering':'immutable original question line/draft order; no split search or key retry','aliases':aliases})
from collections import Counter
source=json.loads((P/'provenance/source-lock.json').read_text())
write(P/'source-manifest.json',{'status':'verified_public_snapshot','upstream_sha':source['upstream_lock']['source_sha'],'original_git_checkout_verified_before_copy':True,'snapshot_commit':None,'snapshot_git_cleanliness_claimed':False,'files':source['files'],'license_sha256':source['license_sha256'],'source_lock_receipt':'provenance/source-lock.json','excluded_paths':source['exclusions']['paths'],'exclusion_reason':source['exclusions']['reasons'],'submodules':[],'binary_generated_vendored_inclusions':[]})
block_receipts={}
for n,label in [(20,'first-020'),(100,'full-100')]:
 selected=rows[:n]; dev=[q for q in selected if q['split']=='dev']; hold=[q for q in selected if q['split']=='holdout']; compat=[]
 for q in dev:
  if q['no_answer']:continue
  c=copy.deepcopy(q);c['expected_files']=list(dict.fromkeys(a['path'] for g in sorted(c['answers'],key=lambda g:not g['primary']) for a in g['alternatives']));c['answers']=[];c['annotations']['v19']['facets']=[];c['annotations']['v19']['graph_constraints']=[];compat.append(c)
 out=P/'intake'/label;out.mkdir(parents=True,exist_ok=True)
 (out/'queries.native.dev.jsonl').write_bytes(jsonl(dev));(out/'queries.compat.dev.jsonl').write_bytes(jsonl(compat))
 for profile in ['native','compat']:
  filename=f'queries.{profile}.dev.jsonl';write(out/f'suite.{profile}.dev.json',{'schema_version':1,'name':f'V19 Express {label} protocol-v1 public dev candidates','source':{'root':'../../source','commit':None,'digest':'REQUIRES_EXPLICIT_AUTHOR_FREEZE','files':sorted(f['path'] for f in source['files'])},'queries':filename,'queries_digest':'REQUIRES_EXPLICIT_AUTHOR_FREEZE','scoring':'codecortex-native-v1' if profile=='native' else 'oce-compat-v1','repetitions':3,'warmup':0,'seed':20261003,'timeout_ms':30000,'top_k':10,'engine_config':{'auto_index':{'enabled':False}}})
 block_receipts[label]={'draft_families':n,'native_dev_families':len(dev),'compat_dev_families':len(compat),'compat_noanswer_excluded':len(dev)-len(compat),'would_be_holdout_families':len(hold),'would_be_holdout_candidate_bytes_sha256':sha(jsonl(hold)),'holdout_materialized_file':None,'holdout_custody_status':'holdout_custody_blocked','holdout_contaminated_quarantined_components':len({q['annotations']['v19']['global_family'] for q in hold}),'accepted_holdout':0,'independently_reviewed':0,'source_category_counts':dict(Counter(q['annotations']['v19']['original_category'] for q in selected)),'native_dev_sha256':sha(jsonl(dev)),'compat_dev_sha256':sha(jsonl(compat)),'split_changed_rows':sum(a['old_split']!=a['new_candidate_split'] for a in aliases[:n])}
write(P/'corpus-receipt.json',{'protocol_version':1,'protocol_commit':'03abe24950f1fa89de3cce0a4e1d44d01e9aa0d6','status':'candidate_intake_holdout_custody_blocked','source_sha':source['upstream_lock']['source_sha'],'license_sha256':source['license_sha256'],'source_manifest_sha256':sha((P/'source-manifest.json').read_bytes()),'author_id':'B/express','reviewer_id':None,'draft_families':100,'candidate_global_components':len(set(canonical.values())),'independently_reviewed':0,'accepted_families':0,'accepted_holdout':0,'quarantined_confirmatory_holdout_families':sum(q['split']=='holdout' for q in rows),'original_public_exposure_all_draft_families':100,'body_exposure_cannot_be_repaired_by_deletion':True,'restricted_custodian':None,'blocks':block_receipts,'family_set_sha256':sha('\n'.join(sorted(families.values())).encode()),'component_set_sha256':sha('\n'.join(sorted(set(canonical.values()))).encode()),'native_scoring_limits':['required facet and graph metrics not implemented','native grade and primary migrated to protocol, no retrieval inspected'],'ranking_observed':False,'global_associations_frozen':False})
write(P/'review-receipt.json',{'author_id':'B/express','reviewer_id':None,'review_status':'pending','independently_reviewed':0,'accepted':0,'holdout_custody_blocked':True,'protocol_commit':'03abe24950f1fa89de3cce0a4e1d44d01e9aa0d6','source_sha':source['upstream_lock']['source_sha'],'required_checks':['semantic family/component adjudication','source gold correctness and alternatives','bounded absence proofs','source/license/span locks','restricted custody and exposure audit']})
print(json.dumps({'draft_families':100,'candidate_components':len(set(canonical.values())),'blocks':{k:{x:v[x] for x in ['native_dev_families','compat_dev_families','would_be_holdout_families','accepted_holdout']} for k,v in block_receipts.items()},'status':'holdout_custody_blocked'}))
