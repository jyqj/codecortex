#!/usr/bin/env python3
"""Verify every retained raw byte, offline replay all outcomes, and repeat analysis."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile

import analyze
import baseline


def verify(output):
    here=Path(__file__).resolve().parent;build=baseline.verify_build(here/'build/build-receipt.json')
    expected=json.loads((here/'raw-artifact-manifest.json').read_bytes())
    errors=[];replays=[]
    env={k:v for k,v in os.environ.items() if k in ['PATH','RUSTUP_HOME','CARGO_HOME','LANG','LC_ALL','TMPDIR','TZ']}
    env['CODECORTEX_BENCH_PROCESS_PROBE']='0'
    with tempfile.TemporaryDirectory(prefix='v19-development-readback-') as td:
        root=Path(td)
        with tarfile.open(here/'public-development-raw.tar.gz') as archive:archive.extractall(root,filter='data')
        actual={p.relative_to(root).as_posix():{'bytes':p.stat().st_size,'sha256':baseline.sha(p.read_bytes())} for p in root.rglob('*') if p.is_file()}
        if actual!=expected:errors.append('RAW_ARCHIVE_CONTENT_DRIFT')
        license_expected=json.loads((here/'license-retention-manifest.json').read_bytes())
        license_root=root/'licenses';license_root.mkdir()
        with tarfile.open(here/'retained-licenses.tar.gz') as archive:archive.extractall(license_root,filter='data')
        if any(baseline.sha((license_root/'retained-licenses'/name).read_bytes())!=r['sha256'] for name,r in license_expected.items()):errors.append('LICENSE_RETENTION_DRIFT')
        for phase in ['pilot','full']:
            for command in json.loads((root/phase/'commands.json').read_bytes()):
                run=root/phase/command['key'];before={p.relative_to(run).as_posix():baseline.sha(p.read_bytes()) for p in run.rglob('*') if p.is_file()}
                proc=subprocess.run([build['artifacts']['cc-eval']['copied_binary'],'replay','--run',str(run)],env=env,capture_output=True)
                after={p.relative_to(run).as_posix():baseline.sha(p.read_bytes()) for p in run.rglob('*') if p.is_file()}
                unchanged=before==after;exit_match=proc.returncode==command['run_exit_code']
                replays.append({'phase':phase,'key':command['key'],'exit_code':proc.returncode,'matches_retained_run_exit':exit_match,'files_unchanged':unchanged,
                    'stdout_sha256':baseline.sha(proc.stdout),'stderr_sha256':baseline.sha(proc.stderr)})
                if not unchanged or not exit_match:errors.append('OFFLINE_REPLAY_DRIFT')
        analysis=analyze.analyze(here/'preregistration-receipt.json',root/'full',root/'analysis')
        analysis_same=(root/'analysis/summary.json').read_bytes()==(here/'analysis/summary.json').read_bytes()
        if not analysis_same:errors.append('ANALYSIS_READBACK_DRIFT')
    receipt={'schema_version':1,'status':'readback_verified' if not errors else 'readback_failed','errors':errors,
        'raw_archive_sha256':baseline.sha((here/'public-development-raw.tar.gz').read_bytes()),'raw_files_verified':len(actual),
        'retained_license_files_verified':len(license_expected),'retained_license_archive_sha256':baseline.sha((here/'retained-licenses.tar.gz').read_bytes()),
        'actual_source_sha':build['source_sha'],'actual_binary_sha256':{n:a['binary_sha256'] for n,a in build['artifacts'].items()},
        'offline_replays':replays,'analysis_byte_identical':analysis_same,'measurement_integrity':analysis['measurement_integrity'],
        'measurement_errors_preserved':analysis['errors'],'retrieval_calls_on_readback':0,'live_provider_calls':0,
        'distinction':'integrity replay success preserves the invalid/incomplete baseline; it does not turn gate failures into quality passes'}
    output.write_text(json.dumps(receipt,indent=2)+'\n')
    print(json.dumps({'status':receipt['status'],'errors':errors,'raw_files_verified':len(actual),'offline_replays':len(replays),
        'receipt_sha256':baseline.sha(output.read_bytes()),'measurement_integrity':analysis['measurement_integrity']}))


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--output',type=Path,required=True);a=p.parse_args();verify(a.output)
