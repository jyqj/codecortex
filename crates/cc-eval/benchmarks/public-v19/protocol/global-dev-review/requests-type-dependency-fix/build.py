#!/usr/bin/env python3
"""Offline build and exact compiler/binary receipt at the fixed Requests repair source."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
from datetime import datetime,timezone

SOURCE='ee1988521e2125f86d2ff0aff8559dccc2a417b0'
sha=lambda b:hashlib.sha256(b).hexdigest()


def build(worktree,output,binaries,target):
    head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=worktree,text=True).strip()
    if head!=SOURCE or subprocess.check_output(['git','status','--porcelain'],cwd=worktree):raise ValueError('SOURCE_NOT_FIXED_CLEAN')
    output.mkdir(parents=True,exist_ok=False);binaries.mkdir(parents=True,exist_ok=False)
    env={k:v for k,v in os.environ.items() if k in ['PATH','RUSTUP_HOME','CARGO_HOME','LANG','LC_ALL','TMPDIR']}
    env.update({'CARGO_TARGET_DIR':str(target),'CARGO_INCREMENTAL':'0','CARGO_PROFILE_DEV_DEBUG':'0'})
    command=['cargo','build','-p','cc-server','--bin','codecortex','--locked','--offline','--message-format=json']
    # Protect existing shared-target executable paths while reusing dependency cache.
    old={}
    for name in ['codecortex']:
        path=target/'debug'/name
        if path.exists():
            backup=binaries/('previous-'+name);shutil.copyfile(path,backup);shutil.copymode(path,backup);old[path]=backup
    started=datetime.now(timezone.utc).isoformat()
    proc=subprocess.run(command,cwd=worktree,env=env,capture_output=True)
    (output/'cargo-build.jsonl').write_bytes(proc.stdout);(output/'cargo-build.stderr').write_bytes(proc.stderr)
    artifacts=[json.loads(line) for line in proc.stdout.splitlines() if line.startswith(b'{')]
    selected={};receipts={}
    try:
        for name in ['codecortex']:
            candidates=[a for a in artifacts if a.get('reason')=='compiler-artifact' and a.get('target',{}).get('name')==name and a.get('executable')]
            if proc.returncode or len(candidates)!=1:raise ValueError('ACTUAL_COMPILER_ARTIFACT_MISSING')
            a=candidates[0]
            if 'semantic' in a['features'] or 'eval-http' in a['features']:raise ValueError('NETWORK_FEATURE_ENABLED')
            executable=Path(a['executable']);dest=binaries/name;shutil.copyfile(executable,dest);shutil.copymode(executable,dest)
            receipts[name]={'compiler_artifact':a,'copied_binary':str(dest),'binary_sha256':sha(dest.read_bytes()),'features':a['features']}
            selected[name]=str(dest)
    finally:
        for path,backup in old.items():shutil.copyfile(backup,path);shutil.copymode(backup,path)
    inventory={p.relative_to(worktree).as_posix():sha(p.read_bytes()) for p in sorted(worktree.rglob('*')) if p.is_file() and p.name!='.git'}
    receipt={'schema_version':1,'source_sha':head,'source_worktree':str(worktree),'sparse_source_only_checkout':True,
        'source_file_sha256':inventory,'source_inventory_sha256':sha(json.dumps(inventory,sort_keys=True,separators=(',',':')).encode()),
        'build_command':command,'build_exit_code':proc.returncode,'started_utc':started,'finished_utc':datetime.now(timezone.utc).isoformat(),
        'build_environment':{k:env[k] for k in ['CARGO_TARGET_DIR','CARGO_INCREMENTAL','CARGO_PROFILE_DEV_DEBUG']},
        'rustc':subprocess.check_output(['rustc','--version'],env=env,text=True).strip(),'cargo':subprocess.check_output(['cargo','--version'],env=env,text=True).strip(),
        'artifacts':receipts,'default_features_only':True,'live_provider_calls':0}
    (output/'build-receipt.json').write_text(json.dumps(receipt,indent=2)+'\n')
    print(json.dumps({'build_exit_code':proc.returncode,'source_sha':head,'receipt_sha256':sha((output/'build-receipt.json').read_bytes()),'binaries':selected}))


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--worktree',type=Path,required=True);p.add_argument('--output',type=Path,required=True)
    p.add_argument('--binaries',type=Path,required=True);p.add_argument('--target',type=Path,required=True);a=p.parse_args()
    build(a.worktree.resolve(),a.output.resolve(),a.binaries.resolve(),a.target.resolve())
