"""Actual inline observer AST on bounded synthetic outputs; never Rust/native."""
import ast
import copy
import hashlib
import json
from pathlib import Path
import re
import tempfile
import tomllib
import yaml

ROOT = Path(__file__).resolve().parent
WORKFLOW = ROOT / '.github/workflows/p8-rebuild-chunk-cost.yml'
document = yaml.safe_load(WORKFLOW.read_text())
blocks = []
for step in document['jobs']['observe']['steps']:
    value = step.get('run', '')
    if value.startswith("python3 -B - <<'PY'\n"):
        body = value.split('\n', 1)[1].rsplit('\nPY', 1)[0]
        ast.parse(body)
        blocks.append(body)
assert len(blocks) == 3
assert 'runner.' not in json.dumps(document['env'])
assert document['jobs']['observe']['runs-on'] == 'ubuntu-24.04'
assert document['env']['P8_COST_TEST'] == 'index_db_write_batch::rebuild_chunk_statement_tests::rebuild_chunk_statements_release_cost_probe'
assert "github.event.label.name == 'p8-rebuild-chunk-cost-run'" in document['jobs']['observe']['if']
cargo = ROOT.parent / 'C8-resume-diagnostic/candidate-M7F/guard-view/crates/cc-db/Cargo.toml'
features = tomllib.loads(cargo.read_text())['features']
assert features == {'p8-db-lock-observation': []}
node = next(n for n in ast.parse(blocks[2]).body if isinstance(n, ast.FunctionDef) and n.name == 'observe_cost_output')
namespace = {'json': json, 'Path': Path, 're': re, 'hashlib': hashlib}
exec(compile(ast.Module(body=[node], type_ignores=[]), str(WORKFLOW), 'exec'), namespace)
results = []
with tempfile.TemporaryDirectory(prefix='chunk-cost-observer-controls-') as directory:
    out, source, target = [Path(directory) / name for name in ('out', 'source', 'target')]
    for path in (out, source, target): path.mkdir()
    executable = target / 'probe'
    executable.write_bytes(b'Synthetic identity only; never executed.\n')
    artifact = dict(reason='compiler-artifact',
        target=dict(name='cc_db', kind=['lib'], src_path=str(source / 'crates/cc-db/src/lib.rs')),
        profile=dict(test=True, opt_level='3', debug_assertions=False), executable=str(executable),
        package_id='path+file://' + str(source) + '/crates/cc-db#1.0.0',
        manifest_path=str(source / 'crates/cc-db/Cargo.toml'), features=[], fresh=False)
    records = [dict(schema='p8-rebuild-chunk-statements-cost-v1', case=case, round=i, chunks=count,
                    first='original' if i % 2 == 0 else 'retained', original_ns='100', retained_ns='101',
                    original_typed_rows_debug_blake3='a'*64, retained_typed_rows_debug_blake3='a'*64,
                    original_base_rows=count, original_fts_rows=count, retained_base_rows=count, retained_fts_rows=count,
                    timed_scope='chunk-loop including compression selection and both inserts; excludes begin/readback/rollback',
                    performance_threshold=None)
               for case,count in (('single_plain',1),('short_plain_6',6),('mixed_32',32),('compressed_64',64)) for i in range(20)]
    cases = ('complete_prefix','compiler_warning_quotes_schema','zero_tests','missing_slot','duplicate_slot',
        'digest_change','digest_type','time_integer','time_fraction','time_negative','round_boolean','rows_boolean',
        'wrong_chunks','base_count','fts_count','missing_row','wrong_producer','failed_process',
        'wrong_feature','performance_threshold','wrong_scope','round_outside_population')
    for case in cases:
        values,event=copy.deepcopy(records),copy.deepcopy(artifact)
        if case=='zero_tests': values=[]
        if case=='missing_slot': values.pop()
        if case=='duplicate_slot': values[-1]=copy.deepcopy(values[0])
        if case=='digest_change':values[33]['retained_typed_rows_debug_blake3']='b'*64
        if case=='digest_type':values[33]['original_typed_rows_debug_blake3']=123
        if case=='time_integer':values[33]['retained_ns']=101
        if case=='time_fraction':values[33]['retained_ns']='1.5'
        if case=='time_negative':values[33]['original_ns']='-1'
        if case=='round_boolean':values[0]['round']=False
        if case=='rows_boolean':values[0]['original_fts_rows']=True
        if case=='wrong_chunks':values[33]['chunks']=32
        if case=='base_count':values[33]['original_base_rows']=5
        if case=='fts_count':values[33]['retained_fts_rows']=5
        if case=='missing_row':values[33].pop('retained_base_rows')
        if case=='wrong_producer':event['manifest_path']=str(source/'wrong/Cargo.toml')
        if case=='wrong_feature':event['features']=['default']
        if case=='performance_threshold':values[0]['performance_threshold']=1
        if case=='wrong_scope':values[0]['timed_scope']='after rollback'
        if case=='round_outside_population':values[0]['round']=20
        lines=[json.dumps(event),json.dumps({'reason':'build-finished','success':True})]
        if case=='compiler_warning_quotes_schema':
            lines.append(json.dumps({'reason':'compiler-message','message':{'message':'quoted p8-rebuild-chunk-statements-cost-v1'}}))
        lines += [('test index_db_write_batch::rebuild_chunk_statement_tests::rebuild_chunk_statements_release_cost_probe ... ' if i==0 else '')+json.dumps(value) for i,value in enumerate(values)]
        lines.append('test result: ok. '+('0' if case=='zero_tests' else '1')+' passed; 0 failed; 0 ignored; 99 filtered out; finished in 0.01s')
        (out/'probe.stdout').write_text('\n'.join(lines)+'\n')
        (out/'execution.json').write_text(json.dumps({'exit_code':1 if case=='failed_process' else 0,'launch_error':None}))
        try:
            namespace['observe_cost_output'](out,source,target);accepted,error=True,None
        except Exception as exc:accepted,error=False,type(exc).__name__
        assert accepted == (case in ('complete_prefix','compiler_warning_quotes_schema')),case
        results.append(dict(case=case,accepted=accepted,error_type=error,expected_result_observed=True))
b=WORKFLOW.read_bytes()
receipt=dict(scope='Actual inline observer AST on synthetic Cargo/probe text. No Rust, Actions, native workload or scale replay.',
    workflow=dict(path=str(WORKFLOW),bytes=len(b),sha256=hashlib.sha256(b).hexdigest(),git_blob=hashlib.sha1(b'blob '+str(len(b)).encode()+b'\0'+b).hexdigest()),
    control_script_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
    cc_db_features=dict(path=str(cargo),sha256=hashlib.sha256(cargo.read_bytes()).hexdigest(),actual=features),
    yaml_parse=True,python_AST_blocks=len(blocks),controls=results)
p=ROOT/'workflow-v1-static-and-output-controls.json';assert not p.exists();p.write_text(json.dumps(receipt,indent=2)+'\n');print(json.dumps(receipt,indent=2))
