#!/usr/bin/env python3
"""Immutable authorization-denial evidence. Positive tests await real wiring."""
import hashlib, json, os, re, subprocess
from pathlib import Path
HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]
BASE = '00e8c6d667198b1c0a1eb2b4a8fd69ca3fd71c4b'
SOURCE = '10303e7'
def sha(p): return hashlib.sha256(p.read_bytes()).hexdigest()
def write(p, value): p.write_text(json.dumps(value, indent=2, sort_keys=True) + '\n')
source = subprocess.check_output(['git', 'rev-parse', SOURCE], cwd=REPO, text=True).strip()
assert (REPO / 'crates/cc-server/tests/p7_v18_parameter_contract.rs').read_bytes() == subprocess.check_output(['git', 'show', source + ':crates/cc-server/tests/p7_v18_parameter_contract.rs'], cwd=REPO), 'Replay requires the frozen source test file, not later appended tests'
root = HERE / 'negative-matrix'
root.mkdir()
env = dict(os.environ, CARGO_HOME='/workspace/.cargo', RUSTUP_HOME='/workspace/.rustup', PATH='/workspace/.cargo/bin:' + os.environ['PATH'], CARGO_BUILD_JOBS='5', CARGO_INCREMENTAL='0')
results = []
negative = ['config_bool_is_default_false_and_rejects_non_boolean_values', 'each_missing_gate_keeps_cold_requests_off_the_network', 'mcp_input_cannot_enable_or_bypass_configuration_authority']
for profile, features in [('default',None), ('semantic','semantic'), ('semantic-http','semantic-http')]:
    profile_root = root / profile
    profile_root.mkdir()
    for n in range(1, 6):
        filters = negative if features == 'semantic-http' else ['']
        for name in filters:
            run = profile_root / f'{n:02}' / (name or 'all-negative')
            run.mkdir(parents=True)
            raw = run / 'raw'
            command = ['cargo','test','-p','cc-server','--test','p7_v18_parameter_contract','query_network_contract' + ('::'+name if name else ''),'--locked','--offline']
            if features: command += ['--features',features]
            command += ['--','--nocapture']
            with (run / 'test.log').open('wb') as out:
                result = subprocess.run(command,cwd=REPO,env=dict(env,P7_V18_EVIDENCE_DIR=str(raw)),stdout=out,stderr=subprocess.STDOUT)
            log = (run / 'test.log').read_text()
            count = 1 if name else 3
            if result.returncode or f'{count} passed; 0 failed; 0 ignored' not in log:
                raise RuntimeError(f'{profile}/{n}/{name} failed; raw retained')
            records = [json.loads(p.read_text()) for p in sorted(raw.glob('*.json'))]
            probes = [r['data'] for r in records if r['kind'] == 'query_network_probe']
            assert all(p['query_http_calls'] == 0 and p['only_loopback'] and not p['real_credentials'] for p in probes)
            runners = re.findall(r'Running tests/p7_v18_parameter_contract\.rs \(([^)]+)\)',log)
            assert len(runners)==1
            receipt = {'baseline_sha':BASE,'source_sha':source,'profile':profile,'repetition':n,'filter':name or 'all-negative','command':command,'exit_code':0,'passed':count,'failed':0,'ignored':0,'query_http_calls':0,'document_http_calls':sum(p['document_http_calls'] for p in probes),'product_binary_sha256':sha(REPO/'target/debug/codecortex'),'test_binary_sha256':sha(REPO/runners[0]),'source_file_sha256':sha(REPO/'crates/cc-server/tests/p7_v18_parameter_contract.rs'),'raw_record_count':len(records),'artifact_sha256':{str(p.relative_to(run)):sha(p) for p in sorted(run.rglob('*')) if p.is_file()}}
            write(run/'receipt.json',receipt)
            results.append(receipt)
    print(profile, '5 repetitions passed',flush=True)
write(root/'summary.json',{'baseline_sha':BASE,'source_sha':source,'test_runs':len(results),'test_executions':sum(r['passed'] for r in results),'failed':0,'ignored':0,'query_http_calls':0,'runs':results,'positive_network_tests':'not_run: actual product producer wiring pending','formal_P7_014':'pending independent review','real_provider_calls':0,'D1_D2':'unchanged'})
