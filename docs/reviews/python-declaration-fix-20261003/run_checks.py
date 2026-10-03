"""Bounded locked checks; archives the original omission failure without rewriting it."""
import hashlib, json, os, subprocess
from pathlib import Path
repo = Path(__file__).resolve().parents[3]
root = Path(__file__).resolve().parent
owned = Path('/workspace/scratch/python-kind')
rust = owned / 'rust/bin'
env = dict(os.environ, PATH=str(rust)+os.pathsep+os.environ['PATH'], CARGO_HOME=str(owned/'cargo'), CARGO_TARGET_DIR=str(owned/'target'), RUSTC=str(rust/'rustc'), RUSTDOC=str(rust/'rustdoc'), CODECORTEX_CACHE_DIR=str(owned/'cache'), CARGO_BUILD_JOBS='4')
commands = [
    ('parsers', ['test','--locked','-p','cc-parsers','--lib','--tests']),
    ('index', ['test','--locked','-p','cc-index','--test','independent_qname_parser_public','--test','python_declaration_identity','--test','qname_identity_proof','--test','qname_owner_diagnostic']),
    ('public', ['test','--locked','-p','cc-server','--test','python_declaration_public','--test','qname_identity_lifecycle','--test','qname_public_diagnostic']),
    ('search', ['test','--locked','-p','cc-search','--lib']),
    ('fmt', ['fmt','--all','--','--check']),
    ('clippy', ['clippy','--locked','-p','cc-parsers','-p','cc-index','-p','cc-server','--all-targets','--','-D','warnings']),
]
logs = root/'checks'
logs.mkdir(exist_ok=True)
receipt = {'source_sha':subprocess.check_output(['git','rev-parse','HEAD'],cwd=repo,text=True).strip(), 'toolchain':subprocess.check_output([str(rust/'rustc'),'--version'],text=True).strip(), 'commands':[]}
for label,args in commands:
    with (logs/f'{label}.log').open('w') as f:
        r = subprocess.run([str(rust/'cargo'),*args],cwd=repo,env=env,stdout=f,stderr=subprocess.STDOUT)
    receipt['commands'].append({'label':label,'argv':[str(rust/'cargo'),*args],'exit_code':r.returncode,'log_sha256':hashlib.sha256((logs/f'{label}.log').read_bytes()).hexdigest()})
    (root/'checks.json').write_text(json.dumps(receipt,indent=2)+'\n')
    if r.returncode: raise SystemExit(r.returncode)
original = repo/'crates/cc-server/tests/independent_qname_public_boundary.rs'
assert not original.exists()
try:
    original.write_bytes((root/'original-tests'/original.name).read_bytes())
    with (logs/'original-public-red-replay.log').open('w') as f:
        r = subprocess.run([str(rust/'cargo'),'test','--locked','-p','cc-server','--test','independent_qname_public_boundary'],cwd=repo,env=env,stdout=f,stderr=subprocess.STDOUT)
    log = (logs/'original-public-red-replay.log').read_text()
    assert r.returncode == 101 and 'unproved decorated class' in log and 'split_method_local_function_and_replaced_same_name_have_exact_sql_survivors ... ok' in log
    receipt['original_public_red_replay'] = {'exit_code':r.returncode,'status':'original absence assertion fails on correctly proved Class qname; not counted as PASS','log_sha256':hashlib.sha256(log.encode()).hexdigest()}
finally:
    original.unlink()
(root/'checks.json').write_text(json.dumps(receipt,indent=2)+'\n')
print(json.dumps(receipt,indent=2))
