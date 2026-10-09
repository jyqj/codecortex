#!/usr/bin/env python3
"""Run only the reviewed original lifecycle evidence checker and retained statistics."""
from pathlib import Path
import hashlib
import json
import os
import subprocess
import sys
import time

OWNED = Path('/Users/jin/Desktop/codecortex-rust/artifacts/checkpoints/p8-round19-evidence-intake-20261009/gates-lifecycle275e-author')
IMAGE = 'python@sha256:3d361d7fea344d55ac7a0f51ed7faa99213808ccc96fc9b9170adb95b6570b96'
IMAGE_ID = 'sha256:3d361d7fea344d55ac7a0f51ed7faa99213808ccc96fc9b9170adb95b6570b96'
SOURCE = '275e8799d4947d297329073eaa3ca675d3fd0777'
TREE = '5859c18f6ead4f8eff5abc3be05d80dfc2fc5b46'
CHECKER_SHA = '89648292044faca2aa386f52f0e21d9b826a2a2504d2b70a58a6ae020fa1087f'
ZIP_SHA = 'e0315c535a51ed7629308463296e6714c63e14731e14d7019be9b266524c7fd1'

def need(ok, message):
    if not ok:
        raise ValueError(message)

def sha(path):
    h=hashlib.sha256()
    with Path(path).open('rb') as stream:
        for block in iter(lambda:stream.read(1024*1024),b''):
            h.update(block)
    return h.hexdigest()

def save(path, value):
    with path.open('x') as stream:
        json.dump(value,stream,indent=2,sort_keys=True)
        stream.write('\n')

def main():
    need(not sys.flags.optimize, 'optimized Python forbidden')
    source=OWNED/'source';base=OWNED/'lifecycle';files=base/'files';execution=base/'execution-01'
    need(OWNED.resolve(strict=True)==OWNED and source.resolve(strict=True)==source
         and base.resolve(strict=True)==base and files.resolve(strict=True)==files
         and not execution.exists(), 'owned prepared paths and new execution')
    prep=json.loads((base/'preparation-receipt.json').read_bytes())
    need(prep['status']=='prepared_for_independent_Linux_receiver_execution'
         and prep['source']==SOURCE and prep['tree']==TREE and prep['artifact']==11597589468,
         'prepared exact original lifecycle artifact')
    need(sha(base/'independent_review.py')==CHECKER_SHA and sha(base/'original-artifact.zip')==ZIP_SHA,
         'unchanged checker and original ZIP')
    inventory=json.loads((base/'zip-members.json').read_bytes())
    def original_inventory():
        need({p.relative_to(files).as_posix() for p in files.rglob('*') if p.is_file()}==set(inventory),
             'all original files and no extras')
        for path,row in inventory.items():
            p=files/path
            need(p.is_file() and not p.is_symlink() and p.stat().st_size==row['bytes']
                 and sha(p)==row['sha256'], 'unchanged original file '+path)
    original_inventory()
    sys.dont_write_bytecode=True
    sys.path.insert(0,str(source/'scripts'))
    need(sha(source/'scripts/p7_build_identity.py')=='5003305105464936edf8f444fc0b93cd14a5d5f074dc650f16de2259e86f1cc5',
         'fixed source snapshot helper')
    from p7_build_identity import source_snapshot
    before=source_snapshot(source)
    need(before['source_commit']==SOURCE and before['source_tree']==TREE
         and before['input_count']==len(before['inputs'])==1089
         and before['manifest_sha256']=='593cf443fc914998dd2cb5a30c3caf180eb8d0d7a398a86e4624e3df2ca65e83',
         'fixed complete source')
    objects_lines=(source/'.git/objects/info/alternates').read_text().splitlines()
    need(len(objects_lines)==1,'one existing object store')
    objects=Path(objects_lines[0]);need(objects.is_absolute() and objects.resolve(strict=True)==objects
                                     and objects.name=='objects','read-only Git object store')
    execution.mkdir()
    inspected=subprocess.run(['/usr/local/bin/docker','image','inspect','--platform','linux/amd64',IMAGE],
                             stdout=subprocess.PIPE,stderr=subprocess.PIPE,timeout=15)
    (execution/'image-inspect.json').write_bytes(inspected.stdout)
    (execution/'image-inspect.stderr').write_bytes(inspected.stderr)
    need(inspected.returncode==0,'existing image required without pull')
    image=json.loads(inspected.stdout)
    need(len(image)==1 and image[0]['Id']==IMAGE_ID and image[0]['Architecture']=='amd64'
         and image[0]['Os']=='linux','exact already available image')
    mount=lambda src,dst,ro=True:['--mount','type=bind,src='+str(src)+',dst='+dst+(',readonly' if ro else '')]
    argv=['/usr/local/bin/docker','run','--rm','--pull=never','--network=none','--read-only',
          '--platform=linux/amd64','--user',str(os.getuid())+':'+str(os.getgid()),
          '--cap-drop=ALL','--security-opt=no-new-privileges','--pids-limit=128','--memory=2g','--cpus=1',
          '--env=PYTHONDONTWRITEBYTECODE=1','--env=GIT_OPTIONAL_LOCKS=0',
          '--cidfile',str(execution/'container-id'),
          '--tmpfs','/home/runner/work/_temp/p8-lifecycle/measurement/fixtures:rw,nosuid,nodev,size=16777216,mode=1777']
    argv += mount(execution,'/review/lifecycle',False)
    argv += mount(files,'/review/lifecycle/files')
    argv += mount(base/'original-artifact.zip','/review/lifecycle/original-artifact.zip')
    argv += mount(base/'expected-artifact.json','/review/lifecycle/expected-artifact.json')
    argv += mount(base/'independent_review.py','/review/lifecycle/independent_review.py')
    argv += mount(source,'/review/source')
    argv += mount(objects,str(objects))
    argv += [IMAGE,'python3','-B','/review/lifecycle/independent_review.py']
    record={'status':'failed_or_partial','scope':'offline original evidence only; no product workload or Cargo',
            'source':SOURCE,'tree':TREE,'artifact':11597589468,'argv':argv,'checker_sha256':CHECKER_SHA,
            'started_utc':time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime())}
    save(execution/'command.json',record)
    try:
        with (execution/'stdout.log').open('xb') as stdout,(execution/'stderr.log').open('xb') as stderr:
            cp=subprocess.run(argv,stdin=subprocess.DEVNULL,stdout=stdout,stderr=stderr,timeout=420)
        record['exit_code']=cp.returncode
        need(cp.returncode==0,'original lifecycle checker/replay failed')
        actual=json.loads((execution/'independent-review-01/inspection.json').read_bytes())
        need(actual['verdict']=='accepted_scoped_lifecycle_observation' and actual['source']['source_commit']==SOURCE
             and actual['workflow_run']==37890757049 and actual['artifact_id']==11597589468
             and actual['replay']['exit_code']==actual['sealed_verify']['exit_code']==actual['final_seal']['exit_code']==0,
             'actual original lifecycle result')
        original_inventory()
        need(source_snapshot(source)==before and sha(base/'independent_review.py')==CHECKER_SHA
             and sha(base/'original-artifact.zip')==ZIP_SHA,'all original source/checker/ZIP unchanged')
        record.update(status='passed',inspection_sha256=sha(execution/'independent-review-01/inspection.json'),
                      complete_original_files_unchanged=True,source_before_after_equal=True)
    except BaseException as error:
        record.update(status='failed_or_partial',exception_type=type(error).__name__,error=str(error))
        cid_path=execution/'container-id'
        if cid_path.exists():
            cid=cid_path.read_text().strip()
            if len(cid)==64 and all(c in '0123456789abcdef' for c in cid):
                stopped=subprocess.run(['/usr/local/bin/docker','rm','--force',cid],stdout=subprocess.PIPE,
                                       stderr=subprocess.PIPE,timeout=15)
                record['owned_container_cleanup']={'id':cid,'exit_code':stopped.returncode,
                                                   'stdout':stopped.stdout.decode(),'stderr':stopped.stderr.decode()}
        raise
    finally:
        record['finished_utc']=time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime())
        record['stdout_sha256']=sha(execution/'stdout.log') if (execution/'stdout.log').exists() else None
        record['stderr_sha256']=sha(execution/'stderr.log') if (execution/'stderr.log').exists() else None
        save(execution/'execution.json',record)
    print(json.dumps({'status':record['status'],'artifact':11597589468,
                      'inspection_sha256':record['inspection_sha256'],'TODO_closed':0,'TODO_remaining':29}))

if __name__=='__main__':
    main()
