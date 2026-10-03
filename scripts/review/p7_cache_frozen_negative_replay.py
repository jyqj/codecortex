#!/usr/bin/env python3
"""Replays just the two unchanged PR48 resource_worker negative cells.
Requires independently built/snapshotted before/fixed test binaries; no matrix changes.
"""
import argparse, hashlib, json, os, pathlib, subprocess, tempfile, time
parser=argparse.ArgumentParser()
parser.add_argument('--before',required=True)
parser.add_argument('--fixed',required=True)
parser.add_argument('--output',required=True)
args=parser.parse_args()
page=os.sysconf('SC_PAGE_SIZE')
def rss(pid):
    try:
        return int(pathlib.Path(f'/proc/{pid}/statm').read_text().split()[1])*page
    except (FileNotFoundError,ProcessLookupError,IndexError):
        return None
cells=[]
for revision,binary in [('before',pathlib.Path(args.before).resolve()),('fixed',pathlib.Path(args.fixed).resolve())]:
    binary_hash=hashlib.sha256(binary.read_bytes()).hexdigest()
    for suffix in ['.bin','.meta.json']:
        with tempfile.TemporaryDirectory(prefix='independent-frozen-cache-') as temp:
            root=pathlib.Path(temp)
            env=os.environ.copy()
            env.update(V16_RESOURCE_ROOT=temp,V16_RESOURCE_N='1',V16_RESOURCE_C='1',V16_RESOURCE_CORRUPT=suffix)
            with (root/'child.log').open('w') as log:
                child=subprocess.Popen([str(binary),'--exact','resource_worker','--nocapture'],env=env,stdout=log,stderr=subprocess.STDOUT)
            deadline=time.monotonic()+30
            samples=[]
            begun=None
            gaps=[]
            try:
                while True:
                    if time.monotonic()>deadline:
                        raise AssertionError('frozen negative exceeded independent 30-second child bound')
                    if (root/'ready').exists():
                        sample=rss(child.pid)
                        if sample is not None:
                            now=time.monotonic()
                            if samples:gaps.append(now-samples[-1][0])
                            samples.append((now,sample))
                            if begun is None:
                                begun=now
                                (root/'go').write_bytes(b'go')
                    status=child.poll()
                    if status is not None:break
                    time.sleep(.005)
                assert status==0,(root/'child.log').read_text()
                result=json.loads((root/'result.json').read_text())
                assert result['n']==result['concurrency']==1
                assert result['dimension']==128 and result['k']==10 and result['batch_rows']==1 and result['repeats']==2
                assert result['workers'][0]['rows_scanned']==2 and result['workers'][0]['result_count']==0
                assert result['cache_files_before']==result['cache_files_after']
                assert result['cache_disk_bytes_before']==result['cache_disk_bytes_after']
                delta=result['rust_live_peak_delta_bytes']
                if revision=='before':assert delta>=64*1024*1024,delta
                else:assert delta<64*1024,delta
                result['independent_replay']={'revision':revision,'binary_sha256':binary_hash,'exit_code':status,
                    'rss_baseline_bytes':samples[0][1],'rss_sampled_peak_bytes':max(v for _,v in samples),
                    'rss_sampled_delta_bytes':max(v for _,v in samples)-samples[0][1],
                    'rss_samples':len(samples),'rss_samples_after_baseline':len(samples)-1,
                    'rss_max_gap_ms':max(gaps,default=0)*1000,'rss_nominal_interval_ms':5,
                    'rss_method':'/proc/<child>/statm resident pages * sysconf page size',
                    'rss_is_continuous_peak':False,'original_worker_source_modified':False}
                cells.append(result)
            finally:
                if child.poll() is None:child.kill();child.wait()
output=pathlib.Path(args.output)
output.write_text(json.dumps({'before_source':'671352d175a552b2448a8c9618f94bef94d9cad6',
    'fixed_source':'db9841ec13f50e5932f469200019d04d7f3b5610','cells':cells},indent=2)+'\n')
for cell in cells:
    print(cell['independent_replay']['revision'],cell['corrupt_suffix'],cell['rust_live_peak_delta_bytes'],cell['independent_replay'])
