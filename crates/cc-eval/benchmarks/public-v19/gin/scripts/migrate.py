"""Protocol metadata migration of legacy drafts, without ranking or gold fact edits.

Input is an explicitly supplied legacy author bundle. Holdout output is NOT private:
use only an independently authorized custodian destination for custody. This task's
/tmp preservation copy has no access boundary and is marked blocked/contaminated.
"""
import argparse,json,pathlib,hashlib,collections,copy
PREFIX=b'codecortex-public-v19-split-v1\n'
PROTOCOL='03abe24950f1fa89de3cce0a4e1d44d01e9aa0d6'
def sha(b): return hashlib.sha256(b).hexdigest()
def raw(rows): return ''.join(json.dumps(q,ensure_ascii=False,separators=(',',':'))+'\n' for q in rows).encode()
def partition(key): return 'holdout' if int.from_bytes(hashlib.sha256(PREFIX+key.encode()).digest()[:8],'big')<2**62 else 'dev'
def main():
 parser=argparse.ArgumentParser(); parser.add_argument('--legacy',type=pathlib.Path,required=True); parser.add_argument('--blocked-preservation',type=pathlib.Path,required=True); parser.add_argument('--protocol',type=pathlib.Path,required=True); args=parser.parse_args()
 base=pathlib.Path(__file__).resolve().parents[1]; oldgold=json.loads((args.legacy/'gold/evidence.json').read_text()); oldrows={q['query_family']:q for s in ['dev','holdout'] for q in map(json.loads,(args.legacy/'questions'/f'{s}.jsonl').read_text().splitlines())}
 assert len(oldgold)==100 and len(oldrows)==100
 aliases=[]; native=[]; blocked=[]; would_split=collections.Counter(); buckets=collections.Counter(); provenance=[]
 # Original declaration order is gold file order; aliases were not selected to fit a split quota.
 for serial,g in enumerate(oldgold,1):
  old=oldrows[g['family']]; f=f'v19.gin.f{serial:04d}'; q=copy.deepcopy(old); oldann=q['annotations']; new_split=partition(f); would_split[new_split]+=1; buckets[q['category']]+=1
  category={'exact/API':'api_usage','behavior':'semantic_feature','architecture/facets':'architecture_understanding','crossfile':'call_chain','config/error':'error_handling','hardnegative/noanswer':'component_location'}[q['category']]
  for i,group in enumerate(q['answers']):
   group['primary']=i==0; group['grade']=3 if i==0 else 2
  v19=dict(protocol_version=1,repo_id='gin',source_sha=oldann['source_sha'],global_family=f,intent=q['query'],query_language='en',hard_scope={'path_prefix':q['path_prefix']},facets=[dict(id=gr['id'],group_id=gr['id'],required=True) for gr in q['answers']],graph_constraints=[dict(from_group=f"facet-{e['from_facet']}",to_group=f"facet-{e['to_facet']}",relation=e['relation'],direction='forward',required=True,evidence=[oldann['gold_evidence'][e['from_facet']-1],oldann['gold_evidence'][e['to_facet']-1]]) for e in oldann['chain_edges']],mutation_profile='none',review_status='pending',author_id='gin-author-cloud',source_gold_status='candidate_not_independently_reviewed',author_target_bucket=q['category'],gold_evidence=oldann['gold_evidence'],literal_answer=oldann['literal_answer'])
  if q['no_answer']: v19['absence_review']=oldann['absence_review']; assert not v19['facets'] and not v19['graph_constraints']
  q.update(id=f+'.en01',query_family=f,category=category,split=new_split,annotations={'v19':v19})
  alias=dict(serial=serial,legacy_family_sha256=sha(old['query_family'].encode()),legacy_query_sha256=sha(raw([old])),legacy_gold_sha256=sha(json.dumps(g,ensure_ascii=False,separators=(',',':')).encode()),new_family=f,new_id=q['id'],old_candidate_split=old['split'],protocol_intended_split=new_split,reason='opaque serial in original draft order; preregistered SHA256 threshold; no ranking observed')
  if new_split=='holdout':
   q['split']='quarantine'; v19['review_status']='quarantine'; v19['custody_status']='holdout_custody_blocked'; v19['quarantine_reason']='body drafted in shared workspace; preprotocol public Git exposure for existing records; no independent access boundary'
   alias['intake_status']='contaminated_quarantined_holdout_custody_blocked'; blocked.append(q)
  else: native.append(q); alias['intake_status']='pending_independent_review_dev'
  aliases.append(alias)
  provenance.append(dict(family=f,evidence_sha256=sha(json.dumps(oldann['gold_evidence'],ensure_ascii=False,separators=(',',':')).encode()),literal_answer_sha256=sha(oldann['literal_answer'].encode()),legacy_family_sha256=alias['legacy_family_sha256']))
 compat=[]
 for q in native:
  if q['no_answer']: continue
  c=copy.deepcopy(q); c['expected_files']=list(dict.fromkeys(a['path'] for gr in sorted(c['answers'],key=lambda gr:not gr['primary']) for a in gr['alternatives'])); c['answers']=[]; compat.append(c)
 for profile,rows in [('native',native),('compat',compat)]: (base/f'queries.{profile}.dev.jsonl').write_bytes(raw(rows))
 (base/'gold/evidence.json').write_text(json.dumps([dict(family=q['query_family'],**q['annotations']['v19']) for q in native],ensure_ascii=False,indent=2)+'\n')
 (base/'provenance/id-migration.json').write_text(json.dumps(dict(protocol_commit=PROTOCOL,method='original draft order; no salt, quota rebalancing or ranking',aliases=aliases,source_gold_semantics='literal answers and source evidence unchanged; group primary/grade assignments aligned to protocol (first primary3, required secondary2)',old_gold_file_sha256=sha((args.legacy/'gold/evidence.json').read_bytes()),old_question_files_sha256={s:sha((args.legacy/'questions'/f'{s}.jsonl').read_bytes()) for s in ['dev','holdout']}),indent=2)+'\n')
 (base/'provenance/gold-fact-commitments.json').write_text(json.dumps(provenance,indent=2)+'\n')
 args.blocked_preservation.mkdir(parents=True,exist_ok=True)
 excluded_native=raw(blocked); excluded_gold=json.dumps([dict(family=q['query_family'],**q['annotations']['v19']) for q in blocked],ensure_ascii=False,indent=2).encode()+b'\n'
 (args.blocked_preservation/'queries.native.quarantine.jsonl').write_bytes(excluded_native); (args.blocked_preservation/'gold.quarantine.json').write_bytes(excluded_gold)
 protocol_hashes={p:sha((args.protocol/p).read_bytes()) for p in ['AUTHOR-START.md','PREREGISTRATION.md','check.py','evaluator-query.schema.json','source-locks.json']}
 receipt=dict(protocol_version=1,protocol_commit=PROTOCOL,protocol_sha256=protocol_hashes,repo_id='gin',source_sha=oldgold[0]['source_sha'],author_id='gin-author-cloud',reviewer_id=None,review_status='pending',custody_status='holdout_custody_blocked',draft_family_ids=100,global_components_proposed=100,independence_status='source-distinct author draft intents; global equivalence review pending; singleton components provisional',accepted_families=0,independently_reviewed_families=0,intended_split_counts=dict(would_split),published_native_dev_rows=len(native),published_compat_dev_rows=len(compat),published_native_noanswer=sum(q['no_answer'] for q in native),native_noanswer_drafts=sum(q['no_answer'] for q in oldrows.values()),compat_noanswer_excluded_drafts=sum(q['no_answer'] for q in oldrows.values()),quarantine_custody_blocked_families=len(blocked),eligible_confirmatory_holdout_families=0,author_target_bucket_counts=dict(buckets),query_file_sha256={'native_dev':sha(raw(native)),'compat_dev':sha(raw(compat))},blocked_preservation_commitments={'native_quarantine_sha256':sha(excluded_native),'gold_quarantine_sha256':sha(excluded_gold),'access_boundary':False,'storage_status':'shared_cloud_tmp_preservation_only_not_restricted_custody'},legacy_public_exposure=dict(commits=['f0079845ea57b93fd491990d75547242b7a13b5f','d128679c234e57a2a2ef777d49ec62c8024d0f46','776157c519dccd22d3a89e26cf778cb3af3541f6','367f1b632abe3d78817cadcf364251f768b21e7a'],public_draft_families=80,history_remains_exposed=True),family_set_sha256=sha('\n'.join(a['new_family'] for a in aliases).encode()),retrieval_run=False)
 (base/'corpus-receipt.json').write_text(json.dumps(receipt,indent=2)+'\n')
 inv=json.loads((base/'provenance/inventory.json').read_text()); source=json.loads((base/'provenance/source-lock.json').read_text())
 (base/'source-manifest.json').write_text(json.dumps(dict(repository=source['repository'],source_sha=source['source_sha'],representation='MIT-noticed snapshot independently byte-checked against clean upstream checkout',upstream_git_clean=True,snapshot_git_clean_verified=False,upstream_submodules='',license_sha256=source['license_files'][0]['sha256'],files=inv),indent=2)+'\n')
 (base/'review-receipt.json').write_text(json.dumps(dict(status='pending',author_id='gin-author-cloud',reviewer_id=None,accepted_families=0,reviewed_families=0,quarantined_custody_blocked=len(blocked),protocol_commit=PROTOCOL,self_review_signed=False),indent=2)+'\n')
 for profile in ['native','compat']:
  oldsuite=json.loads((base/'suite-dev.json').read_text()); oldsuite.update(name='gin-v19-candidate-'+profile+'-dev',queries=f'queries.{profile}.dev.jsonl',queries_digest='',scoring='codecortex-native-v1' if profile=='native' else 'oce-compat-v1',repetitions=3,warmup=0,seed=20261003)
  (base/f'suite-{profile}-dev.json').write_text(json.dumps(oldsuite,indent=2)+'\n')
 print(json.dumps(dict(draft_families=100,public_dev=len(native),blocked_holdout=len(blocked),receipt_sha256=sha((base/'corpus-receipt.json').read_bytes()))))
if __name__=='__main__': main()
