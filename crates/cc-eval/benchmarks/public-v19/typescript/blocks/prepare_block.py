#!/usr/bin/env python3
"""Protocol v1 authoring, private-preparation commitments, source-only validation.
.custody-blocked is explicitly shared-workspace preparation, NOT restricted custody.
"""
import hashlib,json,pathlib,re,sys,tarfile,subprocess
R=pathlib.Path(__file__).resolve().parents[1];sys.path.insert(0,str(R/'.validation-deps'))
import blake3,jsonschema
SHA='ed4807212c28c90777c1d7ef2bf8e47af5d08519';PREFIX='packages/typescript/src/'
CAT={'exact_api':'symbol_location','behavior':'semantic_feature','architecture_facets':'architecture_understanding','crossfile_chain':'call_chain','config_error':'error_handling','hardnegative_noanswer':'semantic_feature'}
def sha(b):return hashlib.sha256(b).hexdigest()
def dump(p,o):p.parent.mkdir(parents=True,exist_ok=True);p.write_text(json.dumps(o,indent=2,ensure_ascii=False)+'\n')
def split(f):return 'holdout' if int.from_bytes(hashlib.sha256(b'codecortex-public-v19-split-v1\n'+f.encode()).digest()[:8],'big')<2**62 else 'dev'
def evidence(ref):
 file,sym=ref.split('|');p=PREFIX+file;b=(R/'source'/p).read_bytes();lines=b.splitlines(keepends=True);owner=None
 if '.' in sym:
  owner,name=sym.split('.',1);i=next(i for i,l in enumerate(lines) if re.match(rb'^(?:export )?class '+owner.encode()+rb'\b',l));limit=next(j for j in range(i+1,len(lines)) if lines[j].strip()==b'}' and not lines[j].startswith(b' '));pat=re.compile(rb'^    (?:(?:private|protected|public|async|override|get|set) )*'+name.encode()+rb'(?:\b|\()');i=next(j for j in range(i+1,limit) if pat.search(lines[j]));indent=b'    '
 else:
  name=sym;pat=re.compile(rb'^(?:export )?(?:function\*?|class|interface|const) '+name.encode()+rb'\b');i=next(i for i,l in enumerate(lines) if pat.search(l) and not l.rstrip().endswith(b';'));indent=b''
 j=next(j for j in range(i+1,len(lines)) if re.match(rb'^'+indent+rb'}(?:;)?\s*$',lines[j]));start=sum(map(len,lines[:i]));end=sum(map(len,lines[:j+1]));a={'path':p,'symbol':{'name':name,'qname':None,'kind':None},'span':{'start':start,'end':end}}
 return a,{'source_sha':SHA,'path':p,'symbol':name,'owner':owner,'start_line':i+1,'end_line':j+1,'start_byte':start,'end_byte':end,'span_sha256':sha(b[start:end]),'file_sha256':sha(b)}
def v19(q,evidence,edges,domain,intent):
 return {'protocol_version':1,'protocol_commit':'03abe24950f1fa89de3cce0a4e1d44d01e9aa0d6','repo_id':'typescript','source_sha':SHA,'global_family':q['query_family'],'intent':intent,'query_language':'en','hard_scope':{'path_prefix':q['path_prefix']},'facets':[{'id':g['id'],'group_id':g['id'],'required':True} for g in q['answers']],'graph_constraints':edges,'mutation_profile':'none','review_status':'holdout_custody_blocked' if q['split']=='holdout' else 'pending','author_id':'typescript-independent-author-cloud','coverage_domain':domain,'gold_evidence':evidence,'answer_rationale':intent,'ranking_inspected':False,'independent_review_signed':False,'custody_status':'holdout_custody_blocked' if q['split']=='holdout' else 'public_dev_candidate','family_hash_sha256':sha(q['query_family'].encode()),'split_hash_sha256':sha(b'codecortex-public-v19-split-v1\n'+q['query_family'].encode())}
def from_spec(s):
 family=s['family'];ee=[];ans=[]
 for n,ref in enumerate(s.get('targets',[])):
  a,e=evidence(ref);ee.append(e);ans.append({'id':'facet-'+str(n+1),'primary':n==0,'grade':3 if n==0 else 2,'alternatives':[a]})
 edges=[]
 for u,v,token,relation in s.get('links',[]):
  e=ee[u];b=(R/'source'/e['path']).read_bytes();t=token.encode();loc=b.find(t,e['start_byte'],e['end_byte']);assert loc>=0,(family,token)
  edges.append({'from_group':f'facet-{u+1}','to_group':f'facet-{v+1}','from_symbol':ee[u]['symbol'],'to_symbol':ee[v]['symbol'],'path':e['path'],'start_byte':loc,'end_byte':loc+len(t),'token':token,'relation':relation,'source_sha':SHA})
 no='scope' in s;p=PREFIX+s['scope'] if no else None
 q={'id':family+'.en01','query_family':family,'query':s['query'],'category':CAT[s['domain']],'language':'typescript','difficulty':3 if s['domain'] in ['crossfile_chain','architecture_facets','hardnegative_noanswer'] else 2,'split':split(family),'path_prefix':p,'no_answer':no,'expected_files':[],'answers':ans,'annotations':{'v19':{}}}
 a=v19(q,ee,edges,s['domain'],s['rationale']);q['annotations']['v19']=a
 if no:
  b=(R/'source'/p).read_bytes();assert all(t.encode().lower() not in b.lower() for t in s['absent_tokens']),family
  a['absence_evidence']={'source_sha':SHA,'scope_files':[p],'file_sha256':sha(b),'checked_lines':[1,len(b.splitlines())],'checked_bytes':[0,len(b)],'absent_tokens':s['absent_tokens'],'source_read_reason':s['rationale'],'basis':'complete scoped file source read plus token corroboration; never retrieval rank'}
 return q

def suite(directory,rows,profile,queryname):
 paths=sorted({e['path'] for q in rows for e in q['annotations']['v19']['gold_evidence']}|{p for q in rows if q['no_answer'] for p in q['annotations']['v19']['absence_evidence']['scope_files']});files=[]
 for p in paths:
  b=(R/'source'/p).read_bytes();files.append({'path':p,'bytes':len(b),'digest':blake3.blake3(b).hexdigest()})
 qp=directory/queryname;qp.parent.mkdir(parents=True,exist_ok=True);qp.write_text(''.join(json.dumps(q,ensure_ascii=False,separators=(',',':'))+'\n' for q in rows));root=pathlib.Path(__import__('os').path.relpath(R/'source',directory)).as_posix();o={'schema_version':1,'name':'V19 TypeScript '+directory.name+' '+profile+' candidate; review/custody pending','source':{'root':root,'commit':None,'digest':blake3.blake3(json.dumps(files,separators=(',',':')).encode()).hexdigest(),'files':paths},'queries':queryname,'queries_digest':blake3.blake3(qp.read_bytes()).hexdigest(),'scoring':profile,'repetitions':3,'warmup':0,'seed':20261003,'timeout_ms':30000,'top_k':10,'engine_config':{'auto_index':{'enabled':False}}};sp=directory/('suite.'+('native' if profile=='codecortex-native-v1' else 'compat')+'.candidate.json');dump(sp,o)
 return qp,sp,files

def validate(rows):
 schema=json.loads((R/'protocol-reference/evaluator-query.schema.json').read_text())
 for q in rows:
  jsonschema.validate(q,schema);a=q['annotations']['v19'];assert a['source_sha']==SHA and q['split']==split(q['query_family'])
  if q['no_answer']:
   assert not q['answers'];e=a['absence_evidence'];p=q['path_prefix'];b=(R/'source'/p).read_bytes();assert sha(b)==e['file_sha256'] and e['scope_files']==[p];assert e['checked_bytes']==[0,len(b)] and e['checked_lines']==[1,len(b.splitlines())];assert all(t.encode().lower() not in b.lower() for t in e['absent_tokens'])
  else:
   for g,e in zip(q['answers'],a['gold_evidence'],strict=True):
    alt=g['alternatives'][0];b=(R/'source'/e['path']).read_bytes();start,end=e['start_byte'],e['end_byte'];assert alt['path']==e['path'] and alt['symbol']['name']==e['symbol'];assert alt['span']=={'start':start,'end':end};assert sha(b)==e['file_sha256'] and sha(b[start:end])==e['span_sha256'];b[start:end].decode();assert b[:start].count(b'\n')+1==e['start_line'] and b[:end].count(b'\n')==e['end_line'];assert g['grade']==(3 if g['primary'] else 2)
   for edge in a['graph_constraints']:
    n=int(edge['from_group'].split('-')[1])-1;e=a['gold_evidence'][n];assert e['start_byte']<=edge['start_byte']<edge['end_byte']<=e['end_byte'];assert (R/'source'/edge['path']).read_bytes()[edge['start_byte']:edge['end_byte']]==edge['token'].encode()
   if a['coverage_domain']=='crossfile_chain':assert len({e['path'] for e in a['gold_evidence']})>=2 and a['graph_constraints']

def author(block):
 D=R/'blocks'/block;private=R/'.custody-blocked'/block;spec=json.loads((D/'spec.dev.json').read_text())+json.loads((private/'spec.holdout.json').read_text());spec.sort(key=lambda s:s['family']);rows=[from_spec(s) for s in spec];assert len(rows)==20;validate(rows);emit(block,rows)
def emit(block,rows):
 D=R/'blocks'/block;private=R/'.custody-blocked'/block;dev=[q for q in rows if q['split']=='dev'];hold=[q for q in rows if q['split']=='holdout'];receipts=[];allfiles={}
 with tarfile.open('/tmp/typescript-v19.tar.gz') as t:
  for q in rows:
   paths={e['path'] for e in q['annotations']['v19']['gold_evidence']}
   if q['no_answer']:paths.update(q['annotations']['v19']['absence_evidence']['scope_files'])
   for p in paths:
    b=(R/'source'/p).read_bytes();assert b==t.extractfile('TypeScript-'+SHA+'/'+p).read();assert len(b)<=1000000 and b'\0' not in b;b.decode();allfiles[p]={'path':p,'bytes':len(b),'sha256':sha(b),'blake3':blake3.blake3(b).hexdigest(),'inclusion_reason':'hand-written public source obligation; generated/dependency imports not included'}
 licenses=json.loads((R/'provenance.json').read_text())['license_artifacts']
 for l in licenses:assert sha((R/l['path']).read_bytes())==l['sha256']
 dump(D/'source-manifest.json',{'upstream_sha':SHA,'source_kind':'explicit partial snapshot; commit null; Git cleanliness not claimed','source_url':'https://github.com/microsoft/TypeScript/tree/'+SHA,'base_sha':'bc8e22bd6b4f85774e7b84a3e6a30da8dc4a3b29','files':[allfiles[p] for p in sorted(allfiles)],'licenses':licenses,'submodules':'not admitted, partial snapshot','dirty_state':'not an upstream Git checkout','excluded':['**/*.generated.ts','**/vendor/**','src/enums/** (generated)','bundled declarations, dependency trees, binaries, tests, all unlisted files']})
 binary=R/'.build/debug/cc-eval'
 for kind,rr,directory in [('dev',dev,D),('holdout',hold,private)]:
  if not rr:continue
  qp,sp,files=suite(directory,rr,'codecortex-native-v1',f'queries.native.{kind}.jsonl');proc=subprocess.run([str(binary),'validate','--suite',str(sp)],capture_output=True);assert proc.returncode==0,(block,sha(proc.stderr));(directory/'native-validate.log').write_bytes(proc.stdout+proc.stderr);receipts.append({'partition':kind,'rows':len(rr),'family_count':len(rr),'file_sha256':sha(qp.read_bytes()),'suite_sha256':sha(sp.read_bytes()),'source_blake3':json.loads(sp.read_text())['source']['digest'],'queries_blake3':blake3.blake3(qp.read_bytes()).hexdigest(),'native_validation_exit_code':proc.returncode,'diagnostics_sha256':sha(proc.stdout+proc.stderr),'location_access_control_verified':False if kind=='holdout' else None})
 compat=[]
 for q in dev:
  if q['no_answer']:continue
  c=json.loads(json.dumps(q));c['expected_files']=list(dict.fromkeys(a['path'] for g in sorted(q['answers'],key=lambda g:not g['primary']) for a in g['alternatives']));c['answers']=[];c['annotations']['v19']['facets']=[];c['annotations']['v19']['graph_constraints']=[];c['annotations']['v19']['projection']='file-only compat; no native span/facet/chain claim';compat.append(c)
 cq,cs,_=suite(D,compat,'oce-compat-v1','queries.compat.dev.jsonl');proc=subprocess.run([str(binary),'validate','--suite',str(cs)],capture_output=True);assert proc.returncode==0;(D/'compat-validate.log').write_bytes(proc.stdout+proc.stderr)
 check=subprocess.run([sys.executable,str(R/'protocol-reference/check.py'),'--shard',f'typescript:native={D}/queries.native.dev.jsonl','--shard',f'typescript:compat={D}/queries.compat.dev.jsonl','--output',str(D/'protocol-check.json')],env={**__import__('os').environ,'PYTHONPATH':str(R/'.validation-deps')},capture_output=True);assert check.returncode==0,(block,check.stdout.decode());(D/'protocol-check.log').write_bytes(check.stdout)
 legacy=block=='01-migration';dump(D/'corpus-receipt.json',{'protocol_commit':'03abe24950f1fa89de3cce0a4e1d44d01e9aa0d6','protocol_files':json.loads((R/'protocol-reference/lock.json').read_text())['files'],'repo_id':'typescript','source_sha':SHA,'author_id':'typescript-independent-author-cloud','reviewer_id':None,'candidate_families':20,'accepted_families':0,'public_dev_families':len(dev),'holdout_candidate_families':len(hold),'holdout_custody_blocked_families':len(hold),'public_history_contaminated_holdout_families':len(hold) if legacy else 0,'confirmatory_holdout_eligible_families':0,'complete_reviewed_blocks':0,'families_global_components_candidate':20,'serial_range':[min(q['query_family'] for q in rows),max(q['query_family'] for q in rows)],'native_partitions':receipts,'compat_dev_rows':len(compat),'compat_noanswer_excluded_families':sum(q['no_answer'] for q in dev),'compat_file_sha256':sha(cq.read_bytes()),'coverage':{d:sum(q['annotations']['v19']['coverage_domain']==d for q in rows) for d in sorted({q['annotations']['v19']['coverage_domain'] for q in rows})},'source_manifest_sha256':sha((D/'source-manifest.json').read_bytes()),'validator_binary_sha256':sha(binary.read_bytes()),'format_checker':'passed; not semantic/source independent review','holdout_custody':'blocked: .custody-blocked is shared workspace, not restricted storage; bodies omitted from new public commits','private_preparation_visibility':'author/shared workspace exposure; independent custodian access audit required; no untouched-holdout claim','ranking_or_parameter_inspection':False,'global_split_frozen':False,'draft_pr_creation':'paused after Forbidden, no retry'})
 dump(D/'review-receipt.json',{'review_status':'pending','independent_reviewer':None,'author_id':'typescript-independent-author-cloud','author_self_signed':False,'accepted_families':0,'candidate_families':20,'holdout_review_status':'holdout_custody_blocked','semantic_dedup':'source obligations authored distinct; independent global equivalence review pending','quarantine_policy':'preserve all records, quarantine disagreement/exposed confirmatory holdout before freeze'})
 print(json.dumps({'block':block,'candidate_families':20,'public_dev':len(dev),'holdout_custody_blocked':len(hold),'native_hashes':[r['file_sha256'] for r in receipts],'coverage':json.loads((D/'corpus-receipt.json').read_text())['coverage'],'validated':True}))
if __name__=='__main__':author(sys.argv[1])
