#!/usr/bin/env python3
"""Offline actual Cargo compiler-artifact receipt; no corpus or ranking access."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

ROOT=Path(__file__).resolve().parents[6]
p=argparse.ArgumentParser(description=__doc__);p.add_argument('--output',type=Path,required=True);args=p.parse_args()
command=['cargo','build','-p','cc-eval','--bin','cc-eval','--locked','--offline','--message-format=json']
proc=subprocess.run(command,cwd=ROOT,capture_output=True,text=True)
if proc.returncode:raise SystemExit(proc.returncode)
artifacts=[json.loads(l) for l in proc.stdout.splitlines()]
artifacts=[m for m in artifacts if m.get('reason')=='compiler-artifact' and m.get('target',{}).get('name')=='cc-eval' and m.get('executable')]
if len(artifacts)!=1:raise SystemExit('expected one actual evaluator compiler artifact')
a=artifacts[0];binary=Path(a['executable']);receipt={'build_exit_code':0,'build_command':command,'compiler_artifact':a,'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'source_sha':subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip()};args.output.write_text(json.dumps(receipt,indent=2)+'\n');print(json.dumps({'receipt_sha256':hashlib.sha256(args.output.read_bytes()).hexdigest(),'binary':str(binary)}))
