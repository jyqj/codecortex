#!/usr/bin/env python3
"""Prepare exact original275e lifecycle ZIP; never execute a workload or checker."""
import hashlib
import json
from pathlib import Path, PurePosixPath
import shutil
import stat
import subprocess
import sys
import zipfile

OWNED = Path('/Users/jin/Desktop/codecortex-rust/artifacts/checkpoints/p8-round19-evidence-intake-20261009/gates-lifecycle275e-author')
SOURCE = '275e8799d4947d297329073eaa3ca675d3fd0777'
TREE = '5859c18f6ead4f8eff5abc3be05d80dfc2fc5b46'
MANIFEST = '593cf443fc914998dd2cb5a30c3caf180eb8d0d7a398a86e4624e3df2ca65e83'
RUN, JOB, ARTIFACT = 37890757049, 113690916508, 11597589468
ZIP_BYTES = 49286914
ZIP_SHA = 'e0315c535a51ed7629308463296e6714c63e14731e14d7019be9b266524c7fd1'
CHECKER_BLOB = 'e532254975bab588f99d45a8cacb1944c0c6e463'
CHECKER_SHA = '89648292044faca2aa386f52f0e21d9b826a2a2504d2b70a58a6ae020fa1087f'

def need(ok, message):
    if not ok:
        raise ValueError(message)

def sha(path):
    h = hashlib.sha256()
    with Path(path).open('rb') as stream:
        for block in iter(lambda: stream.read(1024*1024), b''):
            h.update(block)
    return h.hexdigest()

def save(path, value):
    with path.open('x') as stream:
        json.dump(value, stream, indent=2, sort_keys=True)
        stream.write('\n')

def gh(path):
    return subprocess.check_output(['/opt/homebrew/bin/gh','api','repos/jyqj/codecortex/'+path],
                                   stderr=subprocess.PIPE, timeout=180)

def main():
    need(not sys.flags.optimize, 'optimized Python forbidden')
    need(OWNED.is_dir() and not OWNED.is_symlink(), 'explicit owned root')
    source, out = OWNED/'source', OWNED/'lifecycle'
    need(source.is_dir() and not source.is_symlink() and not out.exists(), 'fixed prepared source and new output')
    need(sha(source/'scripts/p7_build_identity.py') == '5003305105464936edf8f444fc0b93cd14a5d5f074dc650f16de2259e86f1cc5',
         'exact source snapshot helper')
    sys.dont_write_bytecode=True
    sys.path.insert(0,str(source/'scripts'))
    from p7_build_identity import source_snapshot
    before=source_snapshot(source)
    need(before['source_commit']==SOURCE and before['source_tree']==TREE
         and before['input_count']==len(before['inputs'])==1089 and before['manifest_sha256']==MANIFEST,
         'complete fixed source')
    need(shutil.disk_usage(OWNED).free >= ZIP_BYTES+64*1024**2+512*1024**2, 'free space before download')
    out.mkdir()
    result={'status':'preparation_failed_or_partial','scope':'original ZIP transport and materialization only; receiver not executed',
            'source':SOURCE,'tree':TREE,'run':RUN,'job':JOB,'artifact':ARTIFACT,'product_or_Cargo_executed':False}
    try:
        save(out/'source-before.json',before)
        for filename,path in [('run.json',f'actions/runs/{RUN}'),
                              ('jobs.json',f'actions/runs/{RUN}/jobs?per_page=100&filter=all'),
                              ('artifacts.json',f'actions/runs/{RUN}/artifacts?per_page=100'),
                              (f'job-{JOB}.log',f'actions/jobs/{JOB}/logs')]:
            with (out/filename).open('xb') as stream:stream.write(gh(path))
        run=json.loads((out/'run.json').read_bytes());jobs=json.loads((out/'jobs.json').read_bytes())
        artifacts=json.loads((out/'artifacts.json').read_bytes())
        need(run['id']==RUN and run['head_sha']==SOURCE and run['run_attempt']==1
             and run['status']=='completed' and run['conclusion']=='success','original run')
        need(jobs['total_count']==len(jobs['jobs'])==1 and jobs['jobs'][0]['id']==JOB
             and jobs['jobs'][0]['run_id']==RUN and jobs['jobs'][0]['conclusion']=='success','original job')
        need(artifacts['total_count']==len(artifacts['artifacts'])==1,'complete artifact population')
        a=artifacts['artifacts'][0]
        need(a['id']==ARTIFACT and a['size_in_bytes']==ZIP_BYTES and a['digest']=='sha256:'+ZIP_SHA
             and a['workflow_run']['id']==RUN and a['workflow_run']['head_sha']==SOURCE and not a['expired'],
             'fixed artifact metadata')
        log=(out/f'job-{JOB}.log').read_text()
        import re
        need('git checkout --progress --force '+SOURCE in log
             and re.search(r'git log -1 --format=%H\n[^\n]*'+SOURCE,log)
             and 'Artifact ID '+str(ARTIFACT) in log,'actual checkout and upload')
        archive=out/'original-artifact.zip'
        with archive.open('xb') as stream:
            cp=subprocess.run(['/opt/homebrew/bin/gh','api',
                               f'repos/jyqj/codecortex/actions/artifacts/{ARTIFACT}/zip'],
                              stdout=stream,stderr=subprocess.PIPE,timeout=180)
        result['download_exit_code']=cp.returncode
        need(cp.returncode==0,'download failed; original partial retained')
        need(archive.stat().st_size==ZIP_BYTES and sha(archive)==ZIP_SHA,'complete original ZIP')
        inventory={}
        with zipfile.ZipFile(archive) as z:
            for member in z.infolist():
                name=member.filename;p=PurePosixPath(name)
                need(name and not member.is_dir() and not p.is_absolute() and p.as_posix()==name
                     and '..' not in p.parts and '\\' not in name and '\0' not in name
                     and name not in inventory and not member.flag_bits&1
                     and stat.S_IFMT(member.external_attr>>16) in (0,stat.S_IFREG),'safe original ZIP member')
                h=hashlib.sha256();size=0
                with z.open(member) as stream:
                    for chunk in iter(lambda:stream.read(1024*1024),b''):
                        h.update(chunk);size+=len(chunk)
                need(size==member.file_size,'complete streamed original member including CRC')
                inventory[name]={'bytes':size,'sha256':h.hexdigest(),'crc32':f'{member.CRC:08x}',
                                 'original_mode':member.external_attr>>16}
            expanded=sum(row['bytes'] for row in inventory.values())
            need(shutil.disk_usage(out).free >= expanded+64*1024**2+512*1024**2,'actual expanded bytes plus fixed reserve')
            files=out/'files';files.mkdir()
            for name,row in inventory.items():
                target=files/name;target.parent.mkdir(parents=True,exist_ok=True)
                with z.open(name) as original,target.open('xb') as destination:
                    shutil.copyfileobj(original,destination,1024*1024)
                need(target.stat().st_size==row['bytes'] and sha(target)==row['sha256'],'exact original extraction')
                target.chmod(stat.S_IMODE(row['original_mode']) or 0o644)
        need({p.relative_to(files).as_posix() for p in files.rglob('*') if p.is_file()}==set(inventory),
             'complete materialized original inventory')
        save(out/'zip-members.json',inventory)
        save(out/'expected-artifact.json',{'source':SOURCE,'tree':TREE,'run_id':RUN,'run_attempt':1,
                                          'job_id':JOB,'artifact_id':ARTIFACT,'bytes':ZIP_BYTES,'sha256':ZIP_SHA})
        import base64
        packet=json.loads(gh('git/blobs/'+CHECKER_BLOB));body=base64.b64decode(packet['content'])
        need(packet['sha']==CHECKER_BLOB and packet['encoding']=='base64' and len(body)==packet['size']
             and hashlib.sha256(body).hexdigest()==CHECKER_SHA
             and hashlib.sha1(('blob '+str(len(body))+'\0').encode()+body).hexdigest()==CHECKER_BLOB,
             'exact reviewed checker')
        with (out/'independent_review.py').open('xb') as stream:stream.write(body)
        after=source_snapshot(source);need(after==before,'source stable during reception')
        save(out/'source-after.json',after)
        result.update(status='prepared_for_independent_Linux_receiver_execution',zip_sha256=ZIP_SHA,
                      original_member_count=len(inventory),original_member_bytes=expanded,
                      actual_expansion_CRC_SHA256_verified=True,checker_blob=CHECKER_BLOB,
                      checker_sha256=CHECKER_SHA,source_manifest_sha256=MANIFEST)
    except BaseException as error:
        result.update(status='preparation_failed_or_partial',exception_type=type(error).__name__,error=str(error))
        raise
    finally:
        save(out/'preparation-receipt.json',result)
    print(json.dumps(result,sort_keys=True))

if __name__=='__main__':
    main()
