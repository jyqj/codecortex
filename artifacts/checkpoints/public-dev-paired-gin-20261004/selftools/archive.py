#!/usr/bin/env python3
"""Archive only admitted public Gin outputs; independently extract, replay, reanalyze."""
import hashlib, json, subprocess, tarfile, shutil
from pathlib import Path
import analyze

BASE=analyze.BASE
TMP=Path('/tmp/public-dev-paired-gin-20261004')
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
def inventory(path):
    return {p.relative_to(path).as_posix():dict(sha256=sha(p),bytes=p.stat().st_size) for p in sorted(path.rglob('*')) if p.is_file()}
def main():
    staged=TMP/'archive-stage'; staged.mkdir(exist_ok=False)
    audit=[]
    for arm in ['baseline','candidate']:
        for mode in ['native','compat']:
            run=BASE/arm/'runs'/mode; manifest=json.loads((run/'manifest.json').read_bytes())
            admitted={f['path'] for f in manifest['input']['files']}
            rows=analyze.rows(run/'normalized.jsonl'); qs=analyze.rows(run/'queries.jsonl')
            assert all(q['split']=='dev' for q in qs)
            assert all(h['path'] in admitted for r in rows for h in r['hits'])
            assert not any(h['evidence_valid'] is not True for r in rows for h in r['hits'])
            assert manifest['input']['source_digest']=='e16e23913e3eb98a04d8a933488e5fff61d74ce4af3465ca73fd4dc882337e94'
            raw_names={r['raw_path'] for r in rows}; assert raw_names=={p.relative_to(run).as_posix() for p in (run/'raw').glob('*.json')}
            inventory_run=inventory(run)
            assert all(Path(n).suffix in ['.json','.jsonl','.md'] for n in inventory_run)
            assert not any(any(x in Path(n).parts for x in ['source','target','.cache','.codecortex']) for n in inventory_run)
            dest=staged/arm/'runs'/mode; shutil.copytree(run,dest)
            audit.append(dict(arm=arm,mode=mode,dev_queries=len(qs),raw_outputs=len(raw_names),
                proof_hits=sum(len(r['hits']) for r in rows),invalid_or_unverified_hits=0,
                source_paths_within_fixed_admitted_manifest=True,source_digest=manifest['input']['source_digest'],
                archived_full_source_or_binary_or_db=False))
    legal=json.loads((analyze.ROOT/'artifacts/checkpoints/public-dev-declaration-kind-candidate-20261003/license-retention-manifest.json').read_bytes())
    notices={}
    for name,pin in legal.items():
        if '/gin/' not in name:continue
        raw=subprocess.check_output(['git','show',pin['entry']],cwd=analyze.ROOT)
        assert hashlib.sha256(raw).hexdigest()==pin['sha256']
        dest=staged/name;dest.parent.mkdir(parents=True,exist_ok=True);dest.write_bytes(raw); notices[name]=pin
    files=inventory(staged)
    archive=BASE/'public-development-paired-raw.tar.gz'; assert not archive.exists()
    with tarfile.open(archive,'w:gz') as t:
        for n in files:t.add(staged/n,arcname=n,recursive=False)
    readback=TMP/'archive-readback';readback.mkdir(exist_ok=False)
    with tarfile.open(archive,'r:gz') as t:
        for m in t.getmembers():assert m.isfile() and not Path(m.name).is_absolute() and '..' not in Path(m.name).parts
        t.extractall(readback,filter='data')
    assert inventory(readback)==files
    replays=[]
    for arm in ['baseline','candidate']:
        build=json.loads((BASE/arm/'build/build-receipt.json').read_bytes())
        for mode in ['native','compat']:
            command=[build['artifacts']['cc-eval']['path'],'replay','--run',str(readback/arm/'runs'/mode)]
            p=subprocess.run(command,capture_output=True,cwd=TMP/arm)
            expected=json.loads((BASE/arm/'logs'/(mode+'-integrity.json')).read_bytes())['replay']['exit_code']
            assert p.returncode==expected
            prefix=BASE/arm/'logs'/(mode+'-archive-replay')
            prefix.with_suffix('.stdout').write_bytes(p.stdout);prefix.with_suffix('.stderr').write_bytes(p.stderr)
            replays.append(dict(arm=arm,mode=mode,command=command,exit_code=p.returncode,stdout_sha256=hashlib.sha256(p.stdout).hexdigest(),stderr_sha256=hashlib.sha256(p.stderr).hexdigest()))
    assert inventory(readback)==files
    repeated=TMP/'analysis-readback';analyze.analyze(readback,repeated)
    assert inventory(repeated)==inventory(BASE/'analysis')
    analyze.save(BASE/'raw-artifact-manifest.json',dict(scope='admitted public Gin DEV raw original outputs and verified excerpts only',archive_sha256=sha(archive),files=files))
    analyze.save(BASE/'safety-license-audit.json',dict(audits=audit,retained_notices=notices,
        secret_scope='Only fixed admitted MIT Gin public source result excerpts and dev query snapshots; no credentials/environment dump/private corpus/full source export',
        future_holdout_reads=0,extra_source_export=0))
    analyze.save(BASE/'readback-receipt.json',dict(archive_sha256=sha(archive),file_count=len(files),bytes=sum(v['bytes'] for v in files.values()),
        extracted_replay_byte_identical=True,analysis_byte_identical=True,replay_records=replays,new_retrieval_calls=0,analysis_inventory=inventory(repeated)))
    # Retain unpacked originals locally, publish one complete archive without duplicated raw trees.
    for arm in ['baseline','candidate']:shutil.move(str(BASE/arm/'runs'),str(TMP/arm/'original-runs'))
    print(json.dumps({'archive_sha256':sha(archive),'files':len(files),'byte_identical':True}))
if __name__=='__main__':main()
