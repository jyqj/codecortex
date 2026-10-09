import subprocess, pathlib
repo=pathlib.Path('/workspace/codecortex')
out=repo/'artifacts/checkpoints/independent-go-call-fix-20261003'
for label,sha in [('fixed','99a2c0426054ddbed7edf42de43593a780a76a49'),('base','ace2bc7983be2955831c9384e44d1bdd0749c909')]:
    root=pathlib.Path('/tmp/independent-go-call-fix-20261003')/label
    paths=subprocess.check_output(['git','ls-tree','-r','--name-only',sha],cwd=repo,text=True).splitlines()
    for p in paths:
        if p in ['Cargo.toml','Cargo.lock'] or (p.startswith('crates/') and ('/src/' in p or p.endswith('/Cargo.toml'))):
            dst=root/p;dst.parent.mkdir(parents=True,exist_ok=True)
            dst.write_bytes(subprocess.check_output(['git','show',sha+':'+p],cwd=repo))
    dst=root/'crates/cc-index/tests/independent_go_call_fix.rs';dst.parent.mkdir(parents=True,exist_ok=True)
    dst.write_bytes((out/'independent_go_call_fix.rs').read_bytes())
for name in ['context.go','LICENSE']:
    (out/name).write_bytes(subprocess.check_output(['git','show','99a2c0426054ddbed7edf42de43593a780a76a49:artifacts/checkpoints/go-call-identity-fix-20261003/public-source/'+name],cwd=repo))
