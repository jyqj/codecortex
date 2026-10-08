"""Bounded evidence runner; no production edits, no default rustup proxy."""
import datetime, hashlib, json, os, pathlib, shutil, subprocess, sys
ROOT=pathlib.Path(__file__).resolve().parent
TOOL=pathlib.Path('/workspace/.rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin')
variant=sys.argv[1]
env=os.environ.copy()
env.update(CARGO_HOME='/workspace/.cargo', RUSTC=str(TOOL/'rustc'), RUSTDOC=str(TOOL/'rustdoc'),
           CARGO_TARGET_DIR=str(ROOT/'local/target'), CARGO_BUILD_JOBS='2')
receipts=ROOT/'commands.jsonl'
def run(label,argv):
    log=ROOT/(label+'.log')
    # Keep every failed attempt rather than overwrite it on a replay.
    if log.exists():
        index=1
        while (ROOT/(label+f'-attempt{index}.log')).exists(): index+=1
        log=ROOT/(label+f'-attempt{index}.log')
    begin=datetime.datetime.now(datetime.timezone.utc).isoformat()
    with log.open('w') as f:
        f.write(json.dumps(argv)+'\n'); f.flush()
        r=subprocess.run(argv,cwd=ROOT,env=env,stdout=f,stderr=subprocess.STDOUT)
    record=dict(time=begin,label=label,argv=argv,exit_code=r.returncode,log=log.name,
                log_sha256=hashlib.sha256(log.read_bytes()).hexdigest(),
                driver_sha256=hashlib.sha256((ROOT/'driver/src/main.rs').read_bytes()).hexdigest(),
                writer_sha256=hashlib.sha256((ROOT/'local/source/crates/cc-db/src/direct_writer.rs').read_bytes()).hexdigest())
    if (ROOT/'local/target/debug/independent-direct-review').exists():
        record['binary_sha256']=hashlib.sha256((ROOT/'local/target/debug/independent-direct-review').read_bytes()).hexdigest()
    with receipts.open('a') as f: f.write(json.dumps(record)+'\n')
    print(json.dumps({k:record[k] for k in ['label','exit_code','log']}),flush=True)
    return r.returncode
if run(variant+'-prepare',[sys.executable,str(ROOT/'prepare.py'),variant]): sys.exit(1)
lock=ROOT/'driver/Cargo.lock'
if not lock.exists(): shutil.copyfile(ROOT/'local/source/Cargo.lock',lock)
if run(variant+'-build',[str(TOOL/'cargo'),'build','--locked','--manifest-path',str(ROOT/'driver/Cargo.toml')]):sys.exit(1)
binary=ROOT/'local/target/debug/independent-direct-review'
(ROOT/'local/binaries').mkdir(exist_ok=True)
shutil.copyfile(binary, ROOT/'local/binaries'/variant)
cases={'base':['canonical'],'candidate':['canonical','helpers','synthetic','invalid','nul','failures'],
       'mutant-no-triggers':['canonical'],'mutant-split':['helpers'],'mutant-no-nul':['nul']}[variant]
for case in cases:
    rc=run(variant+'-'+case,[str(binary),case,str(ROOT/'local/dbs'/variant/(case+'-'+str(sum(1 for _ in receipts.open()))))])
    # Each run uses its own new ordinary synthetic database directory.
    if variant=='candidate' and rc:sys.exit(rc)
