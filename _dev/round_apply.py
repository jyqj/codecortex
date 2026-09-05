from pathlib import Path
import json

def replace(path,old,new,count=1):
 p=Path(path);s=p.read_text()
 if s.count(old)!=count:raise RuntimeError(f'{path}: unexpected count {s.count(old)} for {old!r}')
 p.write_text(s.replace(old,new))
replace('crates/cc-search/src/lanes.rs','scan.matches.sort_by(|a, b| b.0.cmp(&a.0));','scan.matches.sort_by_key(|a| std::cmp::Reverse(a.0));')
replace('crates/cc-search/src/lanes.rs','seen_out.as_deref_mut()','seen_out')
replace('crates/cc-search/src/lanes.rs','mut seen_out: Option','seen_out: Option')
replace('crates/cc-eval/src/runner.rs','Assertion::expected_symbols(', 'expected(',2)
replace('crates/cc-eval/src/runner.rs','mod rank_contract_tests {\n    use super::*;','''mod rank_contract_tests {
    use super::*;
    fn expected(value: &str) -> Assertion {
        serde_json::from_value(serde_json::json!({"kind":"expected_symbols","value":value})).unwrap()
    }''')
p=Path('crates/cc-eval/src/lib.rs');s=p.read_text();assert 'pub mod quality;' not in s;p.write_text(s+'\npub mod quality;\n')
files={
 'src/a.ts':'export function transact() {\n  return "opaque_needle_z917";\n}\n',
 'src/b.ts':'export function distractor() { return 0; }\n',
 'src/entry.ts':"import { helper } from './helper';\nexport function entry() { return helper(1); }\n",
 'src/helper.ts':'export function helper(value: number) {\n  return value + 917;\n}\n',
 'src/launch.ts':"import { executeHuge } from './huge';\nexport function launch() { return executeHuge(); }\n",
 'src/huge.ts':'export function executeHuge() {\n'+''.join(f'  const local{i} = {i};\n' for i in range(1,106))+'  return "tail_evidence_8391";\n}\n',
}
pyfiles={'billing.py':'def lookup(key):\n    return "billing-ledger"\n','users.py':'def lookup(key):\n    return "user-directory"\n','policy.py':'# 幂等校验\ndef deduplicate(event):\n    return event\n'}
def label(id,path,line,anchor=None,symbol=None):
 d=dict(id=id,file_path=path,start_line=line,end_line=line,grade=3)
 if anchor:d['anchor']=anchor
 if symbol:d['symbol']=symbol
 return d

def task(id,repo,query,labels,category,params=None,groups=None):
 p=dict(query=query,mode='hybrid',top_k=5);p.update(params or {})
 return dict(id=id,repo=repo,category=category,tool='search',params=p,result_pointer='' if p['mode']=='symbol' else '/machine_pack/hits',labels=labels,required_groups=groups or [[l['id']] for l in labels],no_answer=not labels,**({'min_recall_at_5':1.0} if labels else {}))
tasks=[
 task('preselect-rescue','ts','opaque_needle_z917',[label('transaction','src/a.ts',2,'opaque_needle_z917')],'preselection',dict(boost_files=['src/b.ts'],file_preselect_limit=1)),
 task('hard-scope-negative','ts','opaque_needle_z917',[],'negative',dict(path_prefix='src/b.ts',file_preselect_limit=1)),
 task('graph-rescue','ts','entry',[label('entry','src/entry.ts',2,'return helper(1)'),label('helper','src/helper.ts',2,'return value + 917')],'graph',dict(boost_files=['src/entry.ts'],file_preselect_limit=1)),
 task('split-symbol-graph','ts','launch',[label('tail','src/huge.ts',107,'tail_evidence_8391')],'split-symbol',dict(boost_files=['src/launch.ts'],file_preselect_limit=1)),
 task('long-body-literal','ts','tail_evidence_8391',[label('tail','src/huge.ts',107,'tail_evidence_8391')],'literal'),
 task('absent-implementation','ts','cacheEvictionPolicy',[],'negative'),
 task('same-named-symbols','py','lookup',[label('billing','billing.py',1,symbol='lookup'),label('user','users.py',1,symbol='lookup')],'identity',dict(mode='symbol',exact=True)),
 task('body-evidence','py','billing ledger',[label('billing','billing.py',2,'billing-ledger')],'literal'),
 task('chinese-comment','py','幂等校验',[label('policy','policy.py',1,'幂等校验')],'unicode'),
 task('absent-scope','py','lookup',[],'negative',dict(path_prefix='missing/')),
]
manifest=dict(schema_version=1,dataset_id='code-index-regression-v1',purpose='regression',repositories=[dict(id='ts',revision='authored-fixture-v1',files=files),dict(id='py',revision='authored-fixture-v1',files=pyfiles)],tasks=tasks)
p=Path('crates/cc-eval/benchmarks/quality_smoke.json');p.parent.mkdir(parents=True,exist_ok=True);p.write_text(json.dumps(manifest,ensure_ascii=False,indent=2)+'\n')
assert files['src/huge.ts'].splitlines()[106]=='  return "tail_evidence_8391";'
Path('crates/cc-eval/benchmarks/no_graph_retrieval.json').write_text('{"search":{"graph_weight":0.0}}\n')
print('Registered independent quality scorer, 10-task regression manifest and incremental oracle')
