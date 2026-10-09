#!/usr/bin/env python3
"""Package exact reviewed originals in fixed segments after a large-blob transport error."""
from pathlib import Path
import datetime
import hashlib
import json
import shutil

ROOT = Path('/workspace/scratch/28fef0db5e01')
PREVIOUS = ROOT/'integration-validation/round13-runtime-delivery'
OWN = ROOT/'integration-validation/round13-runtime-delivery-v2'
PREFIX = 'artifacts/checkpoints/p8-independent-acceptance-20261009-28fe/round12-M6-and-F5-originals'
OUT = OWN/'delivery'/PREFIX
OUT.mkdir(parents=True, exist_ok=False)
old = json.loads((PREVIOUS/'delivery-receipt.json').read_text())
assert len(old['files']) == 90 and old['total_bytes'] == 95282009
CHUNK = 8*1024*1024


def identity(path):
    h,g = hashlib.sha256(),hashlib.sha1()
    size = path.stat().st_size
    g.update(b'blob '+str(size).encode()+b'\0')
    with path.open('rb') as stream:
        for data in iter(lambda:stream.read(1024*1024),b''):
            h.update(data);g.update(data)
    return {'bytes':size,'sha256':h.hexdigest(),'git_blob':g.hexdigest()}


logical=[]
for entry in old['files']:
    source=Path(entry['local_path']);before=identity(source)
    assert all(before[k]==entry[k] for k in before)
    relative=entry['path'].removeprefix(PREFIX+'/')
    assert relative != entry['path']
    record={'path':relative,'mode':'100644',**before}
    if entry['bytes'] > 20*1024*1024:
        assert relative.endswith(('.zip','.tar.xz'))
        folder='archive-parts/'+('F5-original-zip' if relative.endswith('.zip') else 'M6-completed-original-tar-xz')
        (OUT/folder).mkdir(parents=True)
        parts=[]
        with source.open('rb') as stream:
            i=0
            while data:=stream.read(CHUNK):
                name=folder+f'/part-{i:03d}.bin';p=OUT/name
                with p.open('xb') as out:out.write(data)
                parts.append({'path':name,'ordinal':i,**identity(p)})
                i+=1
        assert sum(x['bytes'] for x in parts)==entry['bytes']
        h=hashlib.sha256()
        for part in parts:
            with (OUT/part['path']).open('rb') as stream:
                for data in iter(lambda:stream.read(1024*1024),b''):h.update(data)
        assert h.hexdigest()==entry['sha256']
        record['segments']=parts
    else:
        target=OUT/'payload'/relative;target.parent.mkdir(parents=True,exist_ok=True)
        shutil.copy2(source,target)
        assert identity(target)==before
        record['payload']='payload/'+relative
    assert identity(source)==before
    logical.append(record)

manifest={'schema':'p8-original-runtime-segment-transport-v1','logical_file_count':90,'logical_bytes':95282009,
          'fixed_maximum_segment_bytes':CHUNK,'logical_v1_delivery_receipt_sha256':'2e2722b20088d52e24e3839ca1b0488c8eccbd73fe53950ea33c90f44d43c93e',
          'v1_independent_review_sha256':'5deb7fb09bdb75970901da012363332356a8054bd5f2ac0576c9b82854ef58e9',
          'reason':'Direct 25488176-byte blob publication returned a transport send error before any ref update. Fixed segmentation changes transport only; no original is recompressed or edited.',
          'files':logical}
(OUT/'logical-package-manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
shutil.copy2(ROOT/'integration-validation/restore_round13_runtime_originals.py',OUT/'restore_originals.py')
shutil.copy2(ROOT/'runtime-review/round13-runtime-delivery-review/independent-delivery-review.json',OUT/'logical-v1-independent-review.json')
shutil.copy2(PREVIOUS/'delivery-receipt.json',OUT/'logical-v1-delivery-receipt.json')
shutil.copy2(ROOT/'runtime-review/m6-local-managed/reproducible-M6-runtime-cache-cleanup.json',OUT/'later-reproducible-runtime-cache-cleanup.json')
(OUT/'README.md').write_text('''# M6 / F5 原始运行证据：固定分段运输

此容器可以逐字恢复已经独立审查的 **90 个文件、95,282,009 字节**。原执行结论和失败历史均在恢复后的 `README.md` 与原报告中。原任务仍 **192 总数 / 163 done / 29 剩余；正式完成 0/10**。

一次直接写入 25,488,176 字节大 blob 的请求返回传输错误，尚未创建提交或更新分支。这里将该 M6 tar.xz 和 51,012,345 字节 F5 官方原 ZIP 按固定最大 8 MiB 分段。没有重压、改字节、删成员、重跑、修改原 receipt 或将失败改成功。每段以及完整原文件都有精确 size、SHA-256 和 Git blob 身份。

其余 88 个逻辑文件逐字保存在 `payload/`；`archive-parts/` 保存两个大原件的全部连续分段。`logical-package-manifest.json` 给出完整 90 文件映射和每段顺序。`payload/` 单独不是完整逻辑包，不能跳过分段去宣称原 ZIP 或 tar 缺失、验收失败或成功。

## 恢复并核验原文件

选择一个不存在的新输出目录，其父目录须已存在：

```sh
python3 restore_originals.py --output /tmp/codecortex-runtime-originals
```

恢复程序只复制/拼接普通文件，不打开 SQLite、不解 ZIP/tar、不执行包内代码或原生测量。输出目录必须独占新建，每一个恢复文件都核对原 size、SHA-256 和 Git blob，并核完整 90 文件集合；失败保留非成功收据。恢复收据写入输出目录旁边的 `.restore-receipt.json` 文件，避免混入原包。恢复后的两个大包与原包逐字完全相同，随后可以按原 `package-index.json` 和各自原 manifest 复查。

独立 v1 交付审查的 270 项检查只认证原件运输和范围，不是新产品测试。新分段方案另有独立传输审查。F5 小时通过只支持已核验的 M6 不变成功路径；本地 M6 未知小时、四 mixed 的资源覆盖 false、原规模 150/1500 缺口和所有硬依赖仍保留。完整状态参见恢复后 README，未改变任何 TODO。
''')

entries=[]
for p in sorted(OUT.rglob('*')):
    if p.is_file():entries.append({'path':p.relative_to(OUT).as_posix(),'mode':'100644',**identity(p)})
(OUT/'package-index.json').write_text(json.dumps({'schema_version':1,'scope':'all physical delivery files except this index','files':entries},indent=2)+'\n')
entries=[]
for p in sorted(OUT.rglob('*')):
    if p.is_file():entries.append({'path':PREFIX+'/'+p.relative_to(OUT).as_posix(),'local_path':str(p),'mode':'100644',**identity(p)})
receipt={'schema_version':2,'prepared_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),
         'status':'prepared_for_independent_segment_transport_review_not_published','parent_sha':old['parent_sha'],'base_tree_sha':old['base_tree_sha'],
         'file_count':len(entries),'total_bytes':sum(x['bytes'] for x in entries),'files':entries,
         'logical_file_count':90,'logical_bytes':95282009,'segmented_originals':sum('segments'in x for x in logical),
         'original_todos_closed':0,'remaining_todos':29}
(OWN/'delivery-receipt.json').write_text(json.dumps(receipt,indent=2)+'\n')
print(json.dumps({'physical_files':len(entries),'physical_bytes':receipt['total_bytes'],'segments':sum(len(x.get('segments',[])) for x in logical),
                  'receipt_sha256':identity(OWN/'delivery-receipt.json')['sha256']}))
