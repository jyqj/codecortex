"""Scoped execution only; all-target lint/build compile without running suites."""
import json, os, pathlib, subprocess, time
root=pathlib.Path(__file__).resolve().parent
repo=pathlib.Path.cwd()
env=os.environ.copy(); env.update(CARGO_HOME='/workspace/.cargo',RUSTUP_HOME='/workspace/.rustup',PATH='/workspace/.cargo/bin:'+env['PATH'])
cargo=['cargo','+1.95.0']
checks=[
 ('model', cargo+['test','--locked','-p','cc-model','--test','declaration_identity_v1','--test','declaration_identity_independent_review','--test','declaration_identity_resources','--test','declaration_resource_independent','--test','provenance_compatibility']),
 ('parser', cargo+['test','--locked','-p','cc-parsers','--test','python_identity_adapter','--test','python_identity_trivia_r1','--test','python_identity_independent','--test','python_identity_r1_independent_delta']),
 ('index', cargo+['test','--locked','-p','cc-index','--test','python_identity_resource_integration','--test','python_provenance_independent_review','--test','python_declaration_identity']),
 ('provenance',cargo+['test','--locked','-p','cc-index','--lib','python_provenance']),
 ('resolver',cargo+['test','--locked','-p','cc-index','--test','p3c_modules','python_uncaptured_environment_differs_from_missing_local_submodule']),
 ('query',cargo+['test','--locked','-p','cc-search','--lib','query_target']),
 ('fmt',cargo+['fmt','--all','--','--check']),
 ('lint',cargo+['clippy','--locked','--workspace','--all-targets','--','-D','warnings']),
 ('build',cargo+['build','--locked','--workspace','--all-targets']),
 ('api-probes',['python3','crates/cc-model/tests/support/resource_api_probes/check.py','--rustc','/workspace/.rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin/rustc','--rlib','target/debug/libcc_model.rlib']),
 ('architecture',['python3','scripts/check_source_architecture.py']),
 ('diff',['git','diff','--check']),
]
receipts=[]
for name, argv in checks:
 start=time.time()
 with (root/(name+'.log')).open('w') as log:
  r=subprocess.run(argv,env=env,stdout=log,stderr=subprocess.STDOUT)
 receipts.append(dict(name=name,argv=argv,exit_code=r.returncode,seconds=round(time.time()-start,3),log=str((root/(name+'.log')).relative_to(repo))))
 (root/'validation.json').write_text(json.dumps(receipts,indent=2)+'\n')
 print(name, r.returncode, flush=True)
 if r.returncode: raise SystemExit(r.returncode)
