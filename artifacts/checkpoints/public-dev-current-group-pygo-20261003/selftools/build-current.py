#!/usr/bin/env python3
"""Locked build and exact compiler/binary receipt at the fixed PR131 source."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
from datetime import datetime,timezone

SOURCE='88f2cf099c8b81f3acef485fd5ac9b01c63ce790'
sha=lambda b:hashlib.sha256(b).hexdigest()


def build(worktree,output,binaries,target):
    head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=worktree,text=True).strip()
    if head!=SOURCE or subprocess.check_output(['git','diff','HEAD','--','crates','Cargo.toml','Cargo.lock'],cwd=worktree):raise ValueError('SOURCE_NOT_FIXED_CLEAN')
    output.mkdir(parents=True,exist_ok=False);binaries.mkdir(parents=True,exist_ok=False)
    env=os.environ.copy()
    toolchain=Path('/workspace/.rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin')
    env['RUSTC']=str(toolchain/'rustc')
    env['CARGO_HOME']='/workspace/.cargo'
    env['RUSTUP_HOME']='/workspace/.rustup'
    env.update({'CARGO_TARGET_DIR':str(target),'CARGO_INCREMENTAL':'0','CARGO_PROFILE_DEV_DEBUG':'0'})
    command=[str(toolchain/'cargo'),'build','-p','cc-eval','--bin','cc-eval','-p','cc-server','--bin','codecortex','--locked','--message-format=json']
    # Protect existing shared-target executable paths while reusing dependency cache.
    old={}
    for name in ['cc-eval','codecortex']:
        path=target/'debug'/name
        if path.exists():
            backup=binaries/('previous-'+name);shutil.copyfile(path,backup);shutil.copymode(path,backup);old[path]=backup
    started=datetime.now(timezone.utc).isoformat()
    proc=subprocess.run(command,cwd=worktree,env=env,capture_output=True)
    (output/'cargo-build.jsonl').write_bytes(proc.stdout);(output/'cargo-build.stderr').write_bytes(proc.stderr)
    artifacts=[json.loads(line) for line in proc.stdout.splitlines() if line.startswith(b'{')]
    selected={};receipts={}
    try:
        for name in ['cc-eval','codecortex']:
            candidates=[a for a in artifacts if a.get('reason')=='compiler-artifact' and a.get('target',{}).get('name')==name and a.get('executable')]
            if proc.returncode or len(candidates)!=1:raise ValueError('ACTUAL_COMPILER_ARTIFACT_MISSING')
            a=candidates[0]
            if 'semantic' in a['features'] or 'eval-http' in a['features']:raise ValueError('NETWORK_FEATURE_ENABLED')
            executable=Path(a['executable']);dest=binaries/name;shutil.copyfile(executable,dest);shutil.copymode(executable,dest)
            receipts[name]={'compiler_artifact':a,'copied_binary':str(dest),'binary_sha256':sha(dest.read_bytes()),'features':a['features']}
            selected[name]=str(dest)
    finally:
        for path,backup in old.items():shutil.copyfile(backup,path);shutil.copymode(backup,path)
    names=subprocess.check_output(['git','ls-files','crates','Cargo.toml','Cargo.lock'],cwd=worktree,text=True).splitlines()
    inventory={n:sha((worktree/n).read_bytes()) for n in sorted(names)}
    if len(inventory)!=708:raise ValueError('SOURCE_INVENTORY_COUNT')
    receipt={'schema_version':1,'source_sha':head,'source_worktree':str(worktree),'sparse_source_only_checkout':False,
        'source_file_sha256':inventory,'source_inventory_sha256':sha(json.dumps(inventory,sort_keys=True,separators=(',',':')).encode()),
        'build_command':command,'build_exit_code':proc.returncode,'started_utc':started,'finished_utc':datetime.now(timezone.utc).isoformat(),
        'build_environment':{k:env[k] for k in ['CARGO_TARGET_DIR','CARGO_INCREMENTAL','CARGO_PROFILE_DEV_DEBUG']},
        'rustc':subprocess.check_output([str(toolchain/'rustc'),'--version'],env=env,text=True).strip(),'cargo':subprocess.check_output([str(toolchain/'cargo'),'--version'],env=env,text=True).strip(),
        'artifacts':receipts,'default_features_only':True,'live_provider_calls':0}
    (output/'build-receipt.json').write_text(json.dumps(receipt,indent=2)+'\n')
    print(json.dumps({'build_exit_code':proc.returncode,'source_sha':head,'receipt_sha256':sha((output/'build-receipt.json').read_bytes()),'binaries':selected}))


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--worktree',type=Path,required=True);p.add_argument('--output',type=Path,required=True)
    p.add_argument('--binaries',type=Path,required=True);p.add_argument('--target',type=Path,required=True);a=p.parse_args()
    build(a.worktree.resolve(),a.output.resolve(),a.binaries.resolve(),a.target.resolve())
