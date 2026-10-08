"""Bounded driver control: synthetic stdio executables, no product certification."""
from contextlib import ExitStack
import argparse
import json
from pathlib import Path
import sys
from unittest import mock

ROOT = Path('/workspace/scratch/28fef0db5e01/codecortex')
sys.path.insert(0, str(ROOT / 'scripts'))
import p8_runtime as runtime


def setup(directory, oracle_exit=1):
    build = directory / 'build'
    build.mkdir(parents=True)
    def executable(name, body):
        path = build / name
        path.write_text('#!' + sys.executable + '\n' + body)
        path.chmod(0o755)
        return path
    product = executable('codecortex', """import json, os, sys
for line in sys.stdin:
    message = json.loads(line)
    if 'id' not in message: continue
    method = message['method']
    if method == 'initialize': result = {}
    elif method == 'tools/list': result = {'tools': [{}] * 14}
    else:
        name = message['params']['name']
        if name == 'index': value = {'parse_errors': [], 'resolution_freshness': {'complete': True}}
        elif name == 'search': value = [{'name': 'p8_runtime_stable_signal', 'file_path': 'stable.py'}]
        else: value = {'diagnostics': {'process_resources': {'pid': os.getpid(), 'resident_bytes': 10000000}}}
        result = {'structuredContent': {'result': value}}
    print(json.dumps({'jsonrpc': '2.0', 'id': message['id'], 'result': result}), flush=True)
""")
    oracle = executable('p8-oracle', r"""import argparse, json
from pathlib import Path
p=argparse.ArgumentParser()
for flag in ('left', 'right', 'output'): p.add_argument('--' + flag)
a=p.parse_args()
code = ORACLE_EXIT
Path(a.output).write_text(json.dumps({'comparison': {'equal': code == 0}, 'exit_code': code}) + '\n')
raise SystemExit(code)
""".replace('ORACLE_EXIT', str(oracle_exit)))
    statistics = executable('p8-runtime-statistics', r"""import argparse, hashlib, json
from pathlib import Path
p=argparse.ArgumentParser()
for flag in ('plan', 'raw', 'output'): p.add_argument('--' + flag)
a=p.parse_args()
def sha(path): return hashlib.sha256(Path(path).read_bytes()).hexdigest()
rows=[json.loads(line) for line in Path(a.raw).read_text().splitlines() if json.loads(line)['kind'] == 'operation']
code = int(any(row['status'] != 'success' for row in rows))
Path(a.output).write_text(json.dumps({'plan_sha256': sha(a.plan), 'raw_sha256': sha(a.raw), 'replay_binary_sha256': sha(__file__), 'exit_code': code, 'recorded_samples': len(rows), 'observation_status': 'synthetic_control'}, sort_keys=True) + '\n')
raise SystemExit(code)
""")
    for name in ('source-before.json', 'source-after.json', 'product-build.jsonl', 'seal.json'):
        (build / name).write_text('{}\n')
    (build / 'product-build.stderr').write_text('synthetic control; no Cargo compilation\n')
    for relative in runtime.OBSERVER_FILES:
        destination = build / 'observer-source' / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_bytes((ROOT / relative).read_bytes())
    receipt = build / 'build-receipt.json'
    receipt.write_text(json.dumps({'source_before': {'synthetic_control': True}, 'oracle_sha256': runtime.digest(oracle), 'statistics_path': str(statistics)}))
    identity = {'binary_path': str(product), 'binary_sha256': runtime.digest(product), 'build_identity': {'synthetic_control': True}}
    args = argparse.Namespace(binary=product, oracle=oracle, statistics=statistics, build_receipt=receipt, output=directory/'run', profile='mixed', concurrency=1, operations=60, files=8, interval_ms=1)
    return args, identity


def execute(directory, oracle_exit=1):
    args, identity = setup(directory, oracle_exit)
    with mock.patch.object(runtime, 'verify_build', return_value=identity), mock.patch.object(runtime, 'verify_receipt', return_value=identity['build_identity']):
        report = runtime.run(args)
    return args, report

if __name__ == '__main__':
    directory = Path(sys.argv[1])
    args, report = execute(directory, int(sys.argv[2]) if len(sys.argv)>2 else 1)
    rows = [json.loads(line) for line in (args.output/'raw.jsonl').read_text().splitlines()]
    print(json.dumps({'report': report, 'operation_rows': sum(row['kind']=='operation' for row in rows), 'statistics_present': (args.output/'statistics.json').exists(), 'oracle': json.loads((args.output/'parity.json').read_text())}, indent=2))
