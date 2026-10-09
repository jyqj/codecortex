"""Finite controls for the actual inline JSON observer, never native execution."""
import ast
import copy
import hashlib
import json
from pathlib import Path
import re
import tempfile

import yaml

ROOT = Path(__file__).resolve().parent
WORKFLOW = ROOT / '.github/workflows/p8-oracle-serialization-equal-lifetime-cost.yml'
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
node = next(n for n in ast.parse(blocks[2]).body
            if isinstance(n, ast.FunctionDef) and n.name == 'observe_cost_output')
namespace = {'json': json, 'Path': Path, 're': re, 'hashlib': hashlib}
exec(compile(ast.Module(body=[node], type_ignores=[]), str(WORKFLOW), 'exec'), namespace)
results = []
with tempfile.TemporaryDirectory(prefix='oracle-cost-observer-controls-') as directory:
    out, source, target = [Path(directory) / name for name in ('out', 'source', 'target')]
    for path in (out, source, target):
        path.mkdir()
    executable = target / 'probe'
    executable.write_bytes(b'Synthetic identity only; this file is never executed.\n')
    artifact = {
        'reason': 'compiler-artifact',
        'target': {'name': 'cc_eval', 'kind': ['lib'], 'src_path': str(source / 'crates/cc-eval/src/lib.rs')},
        'profile': {'test': True, 'opt_level': '3', 'debug_assertions': False},
        'executable': str(executable), 'package_id': 'path+file://' + str(source) + '/crates/cc-eval#1.0.0',
        'manifest_path': str(source / 'crates/cc-eval/Cargo.toml'), 'features': ['default'], 'fresh': False,
    }
    records = [dict(schema='p8-oracle-serialization-equal-lifetime-cost-v1', case=case, round=i, rows=256,
                    first='original' if i % 2 == 0 else 'borrowed', original_ns='100', borrowed_ns='101',
                    original_digest='a' * 64, borrowed_digest='a' * 64, reference_digest='a' * 64, canonical_bytes=1024,
                    timing_threshold=None, original_row_projection_source_blob='4c7fdb879e039af164b3b10ea1491d538c69ac68',
                    timing_scope='same SQLite SELECT traversal plus row serialization and output retention; verification, hash and output drop excluded',
                    output_lifetime='one immutable shared reference; each side output verified, hashed and dropped before next side timing',
                    order_limit='alternating order retained; allocator and cache history not reset or claimed eliminated',
                    comparison_protocol='independent equal-lifetime observation; not a rerun or correction of the v1 retained-pair probe')
               for case in ('multiple_text', 'short_fields', 'blob', 'empty_text', 'null') for i in range(20)]
    for case in ('complete_prefix', 'compiler_warning_quotes_schema', 'zero_tests', 'missing_slot',
                 'duplicate_slot', 'digest_change', 'wrong_producer', 'failed_process', 'wrong_default_feature',
                 'wrong_reference', 'retained_pair_lifetime', 'timing_threshold', 'wrong_protocol'):
        values, event = copy.deepcopy(records), copy.deepcopy(artifact)
        if case == 'zero_tests': values = []
        if case == 'missing_slot': values.pop()
        if case == 'duplicate_slot': values[-1] = copy.deepcopy(values[0])
        if case == 'digest_change': values[33]['borrowed_digest'] = 'b' * 64
        if case == 'wrong_producer': event['manifest_path'] = str(source / 'wrong/Cargo.toml')
        if case == 'wrong_default_feature': event['features'] = []
        if case == 'wrong_reference': values[0]['reference_digest'] = 'b' * 64
        if case == 'retained_pair_lifetime': values[0]['output_lifetime'] = 'both outputs retained'
        if case == 'timing_threshold': values[0]['timing_threshold'] = 1.0
        if case == 'wrong_protocol': values[0]['comparison_protocol'] = 'v1 rerun'
        lines = [json.dumps(event), json.dumps({'reason': 'build-finished', 'success': True})]
        if case == 'compiler_warning_quotes_schema':
            lines.append(json.dumps({'reason': 'compiler-message', 'message': {
                'message': 'quoted p8-oracle-serialization-equal-lifetime-cost-v1 in a compiler diagnostic'}}))
        lines += [('test benchmark::oracle::projection_tests::borrowed_serialization_equal_lifetime_release_cost_probe ... '
                   if i == 0 else '') + json.dumps(value) for i, value in enumerate(values)]
        count = '0' if case == 'zero_tests' else '1'
        lines.append('test result: ok. ' + count + ' passed; 0 failed; 0 ignored; 23 filtered out; finished in 0.01s')
        (out / 'probe.stdout').write_text('\n'.join(lines) + '\n')
        (out / 'execution.json').write_text(json.dumps({'exit_code': 1 if case == 'failed_process' else 0,
                                                       'launch_error': None}))
        try:
            namespace['observe_cost_output'](out, source, target)
            accepted, error = True, None
        except Exception as exc:
            accepted, error = False, type(exc).__name__
        assert accepted == (case in ('complete_prefix', 'compiler_warning_quotes_schema')), case
        results.append({'case': case, 'accepted': accepted, 'error_type': error, 'expected_result_observed': True})
data = WORKFLOW.read_bytes()
receipt = {'scope': 'Actual inline observer AST on synthetic Cargo/probe text. No Rust, Actions, native workload, or scale replay.',
           'workflow': {'path': str(WORKFLOW), 'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest(),
                        'git_blob': hashlib.sha1(b'blob ' + str(len(data)).encode() + b'\0' + data).hexdigest()},
           'control_script_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
           'yaml_parse': True, 'python_AST_blocks': len(blocks), 'controls': results}
target = ROOT / 'workflow-static-and-output-controls-v2.json'
assert not target.exists()
target.write_text(json.dumps(receipt, indent=2) + '\n')
print(json.dumps(receipt, indent=2))
