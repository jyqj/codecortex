#!/usr/bin/env python3
"""Independently source-reviewed dev task obligations. Does not edit original gold."""
import json,hashlib,datetime,sys
from pathlib import Path
R=Path(__file__).resolve().parents[4];O=Path(__file__).parent
SUITES={'source':R/'artifacts/benchmarks/p0-g0-20260927/frozen-inputs/p0-codecortex-subset/suite.json','smoke':R/'artifacts/benchmarks/p0-g0-20260927/frozen-inputs/p0-smoke/suite.json','exact':R/'crates/cc-eval/benchmarks/manifests/p1b-exact.json','intents':R/'crates/cc-eval/benchmarks/manifests/p1c-intents.json'}
SOURCE={
'R01':['pub fn symbol_uid('],
'R02':['h.update(file_path.as_bytes());','h.update(qname.as_bytes());','h.update(kind.as_bytes());','h.update(Self::normalize_signature(sig).as_bytes());'],
'R03':['pub fn chunk_with_symbols('],
'R04':['if nlines as u32 <= self.line_budget {','let end = (off + budget).min(nlines);','let t = lines[s0 + off..s0 + end].join("\\n");'],
'R05':['pub const CURRENT_SCHEMA_VERSION: u32 = 7;'],
'R06':['if stored == CURRENT_SCHEMA_VERSION {','if stored != 0 {','return Ok(SchemaStatus::Mismatch { stored });'],
'R07':['pub(crate) fn compute_dirty_closure<'],
'R08':['let importers = importers_of(&frontier)?;','let flipped = surface_changed_of(&candidates, &changed_so_far)?;','changed_so_far.extend(flipped.iter().cloned());','frontier = next_frontier;'],
'R09':['fn compute_fingerprint_for_unit('],
'R10':['.filter(|path| !targets_cache.contains_key(path.as_str()))','let mut fetched = self.db.reads().reexport_targets_for_files(&missing)?;','targets_cache.insert(path.to_string(), targets);'],
'R11':['pub fn preselect_files('],
'R12':['RankDecaySource::WorkingSet => ctx.boost_paths,','RankDecaySource::Recent => ctx.recent_paths,','RankDecaySource::Pinned => ctx.pinned_paths,','ranking.preselect_working_set_floor,','ranking.preselect_recent_floor,','ranking.preselect_pinned_floor,'],
'R13':['pub(crate) fn chunk_scope(&self) -> cc_db::ChunkScope {','self.filters.chunk_scope()'],
'R14':['normalize_request_from_dsl(&mut request, &dsl);','let filters = MaterializedFilters::from_request(&request);','languages: request.languages.clone(),','file_paths: request.file_paths.clone(),'],
}
SMOKE={
'S01':['def renew_session('],
'S02':['if now - issued_at >= lifetime:','return {"token": token + ":renewed", "issued_at": now}','return {"token": token, "issued_at": issued_at}'],
'S03':['def retry_connection('],
'S04':['def retry_connection(connect, attempts=3):','for _ in range(attempts):','raise TimeoutError("connection retry budget exhausted")'],
'S05':['def renew_subscription('],
'S06':['def renew_subscription(account, months):','return {"account": account, "months": months, "invoice_required": True}'],
'S07':['def validate_amount('],
'S08':['if amount <= 0:','raise ValueError("amount must be positive")'],
'S09':['export function normalizeRoute('],
'S10':['export function validateAmount('],
}
EXACT={'E01':['def decodeFrame():'],'E02':['def encodeFrame():'],'E03':['def decodeFrame():','def encodeFrame():'],'E04':['def describe_format():'],'E05':['def probeBoundary():',"return 'allowed'"],'E06':['def probeBoundary():',"return 'neighboring directory, not the requested scope'"],'E07':['def probeBoundary():'],'E08':['def describe_format():']}
INTENTS={}
for id in ['I01','I02','I03','I04']: INTENTS[id]=['DEFAULT_TIMEOUT = 30','def load_settings(overrides):','return {"timeout": overrides.get("timeout", DEFAULT_TIMEOUT)}']
for id in ['I05','I06']: INTENTS[id]=['def decode_frame(raw):']
for id in ['I07','I08']: INTENTS[id]=['def decode_frame(raw):','tag, payload = raw.split(b":", 1)','return tag.decode("ascii"), payload']
for id in ['I09','I10','I11','I12']: INTENTS[id]=['def process_packet(raw):','tag, payload = decode_frame(raw)','return {"kind": tag, "body": payload}']
for id in ['I13','I14','I15','I16']: INTENTS[id]=['def safe_decode(raw):','return decode_frame(raw)','except ValueError:','return None']
MARKERS={'source':SOURCE,'smoke':SMOKE,'exact':EXACT,'intents':INTENTS}
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
rows=[];locks=[]
for dataset,suitep in SUITES.items():
 suite=json.load(suitep.open());root=(suitep.parent/suite['source']['root']).resolve();qp=(suitep.parent/suite['queries']).resolve();queries=[json.loads(x) for x in qp.read_text().splitlines()]
 files=[{'path':p,'sha256':sha(root/p)} for p in suite['source']['files']]
 locks.append({'dataset':dataset,'suite_path':str(suitep.relative_to(R)),'suite_sha256':sha(suitep),'query_path':str(qp.relative_to(R)),'query_sha256':sha(qp),'source_root':str(root.relative_to(R)),'files':files})
 for q in queries:
  req={'dataset':dataset,'id':q['id'],'query':q['query'],'path_prefix':q.get('path_prefix'),'no_answer':q['no_answer'],'required_source_evidence':[],'graph_object_required':False,'inventory_coverage_not_task_evidence':True}
  if q['no_answer']:
   req['absence_basis']='All locked fixture sources independently read; no declaration, implementation or callsite of the requested invented symbol/task scheduler exists. Mere empty Partial is not validated absence.'
   req['absence_required_status']='NoMatch with all required lookup/scope evidence complete; zero source hits; no timeout/error/partial absence claim.'
   req['scope_files_reviewed']=files
  else:
   path=q['answers'][0]['alternatives'][0]['path'];raw=(root/path).read_bytes()
   for marker in MARKERS[dataset][q['id']]:
    m=marker.encode();start=raw.find(m);assert start>=0,(dataset,q['id'],marker)
    req['required_source_evidence'].append({'path':path,'byte_span':{'start':start,'end':start+len(m)},'required_text':marker,'source_sha256':sha(root/path),'rule':'complete span union of independently byte-valid source hits; refs/path metadata do not satisfy'})
   req['reason']='Definition/location evidence for identifier/path lookup; actual behavior statements for semantic/how/caller queries. Minimum core task evidence, not whole candidate inventory or whole-file return.'
   if dataset=='intents' and q['id'] in ['I09','I10','I11','I12']:
    req['source_relation']={'caller':'dispatcher.py:process_packet','callee':'decoder.py:decode_frame','kind':'CALLS','direction':'caller_to_callee','source_callsite_required':True,'note':'source callsite satisfies question; any emitted graph fact must independently agree, graph metadata alone is not source span evidence'}
  rows.append(req)
result={'schema_version':1,'reviewer':'acceptance_auditor','observed_at_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'profile':'independent_reviewed_development_task_obligations_v2','source_basis':'Source and original query read without new candidate retrieval results; original gold untouched. Historical questions already public, not heldout.','locked_inputs':locks,'tasks':rows,'known_scope_limitations':['Minimum task evidence is separate from truthful lane inventory Partial and packing omissions.','No question in these 51 explicitly requests a machine graph object; graph correctness certification requires independent graph constraint fixtures and emitted-fact validation.','Derived obligations do not replace fixed original ranking/no-answer scorers or their retained failures.','No threshold/marker may be removed after candidate failure without independent source-spec justification.']}
p=O/(sys.argv[1] if len(sys.argv)>1 else 'task-obligations-v1.json');assert not p.exists();p.write_text(json.dumps(result,ensure_ascii=False,indent=2)+'\n');print('locked independent obligations',len(rows),'source facets',sum(len(r['required_source_evidence']) for r in rows))
