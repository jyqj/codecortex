"""Publish only this owned synthetic run's safe evidence, never source/cache/DB/target."""
import gzip
import hashlib
import json
from pathlib import Path
import shutil
BASE = Path(__file__).resolve().parent
WORK = Path('/workspace/qname-100k-measurements')
def digest(p):
    h=hashlib.sha256()
    with p.open('rb') as f:
        for b in iter(lambda:f.read(1048576),b''): h.update(b)
    return h.hexdigest()
def retain(src,dst):
    dst.parent.mkdir(parents=True,exist_ok=True)
    if (src.suffix == '.jsonl' or src.name in ('build-stderr.log','logs-sha256.json')
            or (src.suffix == '.json' and src.stat().st_size > 1048576)):
        dst=Path(str(dst)+'.gz')
        with src.open('rb') as f,gzip.open(dst,'wb') as z:shutil.copyfileobj(f,z)
    else:shutil.copyfile(src,dst)
    return dict(source=str(src),source_sha256=digest(src),source_bytes=src.stat().st_size,archive=str(dst.relative_to(BASE)),archive_sha256=digest(dst),archive_bytes=dst.stat().st_size)
def rows(path):
    opener=gzip.open if path.suffix=='.gz' else open
    with opener(path,'rt') as f:
        for line in f:yield json.loads(line)
def aggregate(run):
    result={}
    for path in run.rglob('composition.jsonl'):
        phases={}
        for r in rows(path):
            phase=r['phase'];m=r['cgroup'];v=m['memory_current_bytes']['value']
            d=phases.setdefault(phase,dict(samples=0,cgroup_min_bytes=None,cgroup_max_bytes=None,product_root_rss_max_bytes=None))
            d['samples']+=1
            if v is not None:
                d['cgroup_min_bytes']=v if d['cgroup_min_bytes'] is None else min(v,d['cgroup_min_bytes'])
                if d['cgroup_max_bytes'] is None or v>d['cgroup_max_bytes']:
                    d['cgroup_max_bytes']=v;d['composition_at_cgroup_max']=m['composition_bytes'];d['cgroup_max_sample_ns']=r['time_ns']
            for p in r.get('processes',[]):
                if p['role']=='product_root' and p['stat']['value']:
                    rss=p['stat']['value']['rss_bytes'];d['product_root_rss_max_bytes']=max(d['product_root_rss_max_bytes'] or 0,rss)
            d['group_path']=m['group_path'];d['group_inode']=m['group_inode'];d['memory_max']=m['memory_max'];d['last_memory_events']=m['memory_events'];d['members_last']=m['members']
        result[str(path.relative_to(run))]=phases
    resources={}
    for path in run.rglob('resources.jsonl.gz'):
        d=dict(samples=0,root_rss_max_bytes=None,root_vmhwm_max_bytes=None,sampled_tree_rss_max_bytes=None,tree_coverage_error_samples=0,phases={})
        for r in rows(path):
            d['samples']+=1;p=r.get('product') or {};tree=r.get('product_tree') or {}
            for field,key in [('rss_bytes','root_rss_max_bytes'),('vmhwm_bytes','root_vmhwm_max_bytes')]:
                if p.get(field) is not None:d[key]=max(d[key] or 0,p[field])
            if tree.get('sampled_sum_rss_bytes') is not None:d['sampled_tree_rss_max_bytes']=max(d['sampled_tree_rss_max_bytes'] or 0,tree['sampled_sum_rss_bytes'])
            if tree.get('coverage_errors'):d['tree_coverage_error_samples']+=1
            phase=r['phase'];v=d['phases'].setdefault(phase,dict(first=r,last=r,samples=0));v['last']=r;v['samples']+=1
        resources[str(path.relative_to(run))]=d
    return dict(cgroup_phases=result,product_resources=resources,independence='unknown',rss_is_not_cgroup=True,non_atomic=True,composition_counters_not_summed=True,sampled_tree_shared_pages_may_repeat=True)
def main():
    retained=[]
    for attempt in sorted(WORK.glob('build-attempt-*')):
        for p in sorted(attempt.rglob('*')):
            if p.is_file() and not {'target','cargo','tmp'} & set(p.relative_to(attempt).parts):retained.append(retain(p,BASE/'evidence'/attempt.name/p.relative_to(attempt)))
    run=WORK/'current-once'
    if run.exists():
        for p in sorted(run.rglob('*')):
            if p.is_file() and not {'repo','cache'} & set(p.relative_to(run).parts):retained.append(retain(p,BASE/'evidence/current-once'/p.relative_to(run)))
        (BASE/'resource-analysis.json').write_text(json.dumps(aggregate(run),indent=2)+'\n')
    (BASE/'publication-manifest.json').write_text(json.dumps(dict(retained=retained,excludes=['synthetic source corpus','repo .git','DB/WAL/SHM','cache','cargo cache','target','binary','old evidence','private diagnostics'],private_userdata_export=False),indent=2)+'\n')
if __name__=='__main__':main()
