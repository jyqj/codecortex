from pathlib import Path
import collections,datetime,gzip,hashlib,json
root=Path.cwd();review=root/'review-runtime';prefix='artifacts/benchmarks/p8-bfccb8494ba0'
aid=11594089439;rawbase=root/'raw'/str(aid);ex=rawbase/'extracted'
def read(p):return json.loads(p.read_text())
def sha(p):
 h=hashlib.sha256()
 with p.open('rb') as f:
  for chunk in iter(lambda:f.read(1048576),b''):h.update(chunk)
 return h.hexdigest()
def ref(p):
 return {'path':prefix+'/'+str(p.relative_to(root)),'mac_project_relative_path':prefix+'/'+str(p.relative_to(root)),'bytes':p.stat().st_size,'sha256':sha(p)}
terminal=read(review/'round5-p8-007-010-terminal-evidence.json')
native=terminal['new_soak_terminal_assessment']
columns=['source_line','byte_offset','byte_length','line_sha256_including_original_newline','event_or_kind','original_request_or_operation_id','clock_field','original_clock_ns']
indices=[]
for relative,key in [('p8-runtime/raw.jsonl','raw'),('p8-runtime/product/rpc.jsonl','actual_stdio'),('p8-runtime/full-product/rpc.jsonl','full_control_stdio')]:
 src=ex/relative;expected=native[key];basename=relative.replace('/','-')+'.index.jsonl.gz';dest=review/('soak-'+str(aid)+'-'+basename)
 assert not dest.exists()
 source_digest=hashlib.sha256();index_digest=hashlib.sha256();uncompressed_index_bytes=0;offset=0;rows=0;kinds=collections.Counter();operation_ids=[];request_ids=[];response_ids=[]
 header={'schema_version':1,'document_kind':'complete_original_jsonl_byte_index','source':expected,'columns':columns,'original_payloads_retained_in':'Official artifact ZIP and Mac central raw; this index is a derived locator, not original payload or a replacement workload/collector input.','clock_scope':'Original raw driver-relative clocks and original stdio transport clocks remain separate; no UTC conversion or cross-clock relabeling.'}
 with dest.open('xb') as file_obj:
  with gzip.GzipFile(fileobj=file_obj,mode='wb',filename='',mtime=0,compresslevel=9) as gz:
   def write_line(obj):
    nonlocal_placeholder=None
    return (json.dumps(obj,ensure_ascii=False,separators=(',',':'))+'\n').encode('utf-8')
   hline=write_line(header);gz.write(hline);index_digest.update(hline);uncompressed_index_bytes+=len(hline)
   with src.open('rb') as f:
    for rownum,line in enumerate(f,1):
     obj=json.loads(line);kind=obj.get('event',obj.get('kind'));kinds[kind]+=1
     payload=obj.get('payload')
     ident=obj.get('request_id',obj.get('id'))
     if isinstance(payload,dict):ident=payload.get('id',ident)
     if kind=='stdout_wire':
      wire=json.loads(obj['text']);ident=wire.get('id')
      assert len(obj['text'].encode('utf-8'))==obj['wire_bytes']
      assert hashlib.sha256(obj['text'].encode('utf-8')).hexdigest()==obj['wire_sha256']
     if kind=='operation':operation_ids.append(ident)
     if kind=='request':request_ids.append(ident)
     if kind=='response':response_ids.append(ident)
     clock_field=next((k for k in ['time_ns','at_ns','finished_ns'] if k in obj),None)
     row=[rownum,offset,len(line),hashlib.sha256(line).hexdigest(),kind,ident,clock_field,obj.get(clock_field) if clock_field else None]
     b=write_line(row);gz.write(b);index_digest.update(b);uncompressed_index_bytes+=len(b)
     source_digest.update(line);offset+=len(line);rows+=1
 assert offset==expected['bytes']==src.stat().st_size and source_digest.hexdigest()==expected['sha256']
 if operation_ids:assert operation_ids==list(range(3601))
 if request_ids:
  assert len(set(request_ids))==len(request_ids)
  assert set(request_ids)==set(response_ids)
 if relative=='p8-runtime/raw.jsonl':
  assert rows==7200
  assert dict(kinds)=={'initial_build':1,'resources':3594,'operation':3601,'endpoint_status':1,'full_control':1,'endpoint_public':1,'oracle_process':1}
 if relative=='p8-runtime/product/rpc.jsonl':
  assert rows==57603 and kinds['request']==kinds['response']==kinds['stdout_wire']==kinds['rpc_send']==14400
 if relative=='p8-runtime/full-product/rpc.jsonl':
  assert rows==19 and kinds['request']==kinds['response']==kinds['stdout_wire']==kinds['rpc_send']==4
 info={'original':expected,'index':ref(dest),'index_compression':'gzip with filename empty and mtime 0; exact complete JSONL index can be losslessly decompressed','decompressed_index_sha256':index_digest.hexdigest(),'decompressed_index_bytes':uncompressed_index_bytes,'source_rows':rows,'source_bytes_fully_accounted':offset,'kinds':dict(kinds),'original_rpc_requests':len(request_ids),'original_rpc_responses':len(response_ids),'original_operation_ids':{'count':len(operation_ids),'first':operation_ids[0] if operation_ids else None,'last':operation_ids[-1] if operation_ids else None},'complete':True}
 indices.append(info)
 # Verify the completed lossless index archive against its own decoded bytes, not a partial stdout rendering.
 d=hashlib.sha256();size=0
 with gzip.open(dest,'rb') as f:
  for part in iter(lambda:f.read(1048576),b''):d.update(part);size+=len(part)
 assert d.hexdigest()==info['decompressed_index_sha256'] and size==uncompressed_index_bytes
indexdoc={'schema_version':1,'artifact_id':aid,'source_sha':native['source_sha'],'created_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'derived_index_scope':'All original physical JSONL rows and all original bytes indexed without filtering, response reuse, reordering or omission. Per-line offsets, lengths and SHA256 bind the original newline too. Complete original payloads remain external; native validators still require those payloads.','indices':indices}
indexpath=review/('soak-'+str(aid)+'-raw-stdio-indices.json');assert not indexpath.exists();indexpath.write_text(json.dumps(indexdoc,ensure_ascii=False,indent=2,sort_keys=True)+'\n')
readme=review/('soak-'+str(aid)+'-archive-README.md');assert not readme.exists()
readme.write_text('''# 原 a23 soak 最小归档选择

这是原 artifact 11594089439 的小证据选择，不是完整原 sealed artifact。完整原 ZIP、70,117,809-byte raw.jsonl、322,493,166-byte product/rpc.jsonl、原 ELF 与原数据库仍在 GitHub 原 artifact 和 Mac central raw/11594089439 原件中；它们没有被改写、截短或用索引替代。官方定位及每个选择文件的 SHA256 / bytes 见同目录 archive-selection.json；官方 ZIP SHA256 为 d6debfa5e4a51bb59902cdb69a35ae7626a1e20b09bcaa5ab6cfda9ee3237086。

小文件完整保存 plan/report/parity/statistics/replay/execution receipts、build receipts/source maps/seals、原 observer scripts、原 full-control stdio、两个 process receipts、产品 stderr、原 GitHub 已解码日志以及两个独审脚本/结果。重复的 runtime/build-evidence 原文件通过逐一精确 SHA256 与 p8-build 原件对应，不额外复制；旧 mixed/lifecycle/backfill v2 审查引用既有文件，不覆盖或复制。

三个 gzip JSONL 索引覆盖原 raw 7,200 行、product stdio 57,603 行和 full-control stdio 19 行。首行是 schema 与原文件路径/bytes/SHA256，后续每行依次为原物理行号、byte offset、byte length、包含原换行的 SHA256、event/kind、原 request/operation id、原 clock 字段名及值。gzip 是无损索引封装，mtime=0；其压缩及解压摘要均在 raw-stdio-indices.json。索引只提供完整定位与覆盖证明，没有保留其中原载荷，不能单独重放 collector 或验收。full-control 的原 stdio 文件较小，也完整收录。

复核完整原验收时必须先取原官方 ZIP 并验证上述 digest，再解压到原 central raw/11594089439/extracted 结构，准备 exact-a23 source，才能运行 runtime-independent-review-v2.py 及 soak-stdio-binding-review.py。不能对本小选择目录运行原 seal CLI 后期待通过；也不能删除被引用的大文件后继续声称全量原验收可从此选择单独重放。

本轮只追加 Round 5 终态 evidence 和 soak 新审查结果，Round 4 pending JSON / Markdown 仍为原字节。新 P、PR #161 新 head 及任何其它 source 都不继承此 a23 执行信用。
''')
selected={}
def add(p,role):
 key=str(p.relative_to(root))
 if key in selected:return
 selected[key]={'source':ref(p),'action':'archive_full_original_file' if p.is_relative_to(rawbase) else 'archive_full_review_file','role':role}
for name in ['github-metadata.json','transport-receipt.json','zip-members.json']:
 add(rawbase/name,'Original artifact transport identity and exact ZIP member inventory')
for name in ['plan.json','report.json','parity.json','seal.json','statistics.json','statistics-replay.json','statistics-execution.json','statistics.json.stdout','statistics.json.stderr','statistics-replay.json.stdout','statistics-replay.json.stderr']:
 add(ex/'p8-runtime'/name,'Complete original small runtime/plan/raw-derived report/statistics/seal data; not a selected sample')
for sub,name,role in [
 ('product','process.json','Original owned product process receipt'),
 ('product','product-stderr.log','Complete original product stderr including actual catalog compaction witnesses'),
 ('full-product','process.json','Original fresh-full owned process receipt'),
 ('full-product','product-stderr.log','Complete original fresh-full stderr'),
 ('full-product','rpc.jsonl','Complete original small fresh-full stdio payloads')]:
 add(ex/'p8-runtime'/sub/name,role)
for p in sorted((ex/'p8-build').rglob('*')):
 if p.is_file() and p.name not in ['codecortex','p8-oracle','p8-runtime-statistics']:
  add(p,'Complete original build/observer/source/receipt evidence; exact original bytes')
review_names=[
 'runtime-independent-review-v2.py','soak-stdio-binding-review.py',
 'runtime-11594089439-review-v2.json','soak-11594089439-stdio-binding-review.json',
 'github-job-113631481157-soak.log','soak-11594089439-github-terminal.json',
 'round5-p8-007-010-terminal-evidence.json','round5-p8-007-010-terminal-review.md',
 'prepare-round5-terminal-assessment.py','prepare-soak-minimal-archive.py',
 'soak-11594089439-raw-stdio-indices.json','soak-11594089439-archive-README.md']
for name in review_names:add(review/name,'Exact new soak reviewer/script/index/terminal assessment; shared v2 script may be reused if same SHA already archived')
for info in indices:
 add(root/Path(info['index']['path']).relative_to(prefix),'Complete derived byte index, losslessly gzip packed; not a replacement for original source payload')
excluded=[]
for p in [rawbase/'original.zip',ex/'p8-runtime/raw.jsonl',ex/'p8-runtime/product/rpc.jsonl']+[ex/'p8-build'/n for n in ['codecortex','p8-oracle','p8-runtime-statistics']]:
 excluded.append({'source':ref(p),'action':'retain_official_locator_and_mac_original_do_not_copy_into_git','reason':'Original large ZIP/raw payload or native binary remains complete in original artifact; fetch exact original artifact to replay original acceptance'})
duplicates=[]
for p in sorted((ex/'p8-runtime/build-evidence').rglob('*')):
 if not p.is_file():continue
 q=ex/'p8-build'/p.relative_to(ex/'p8-runtime/build-evidence')
 assert q.is_file() and sha(p)==sha(q) and p.stat().st_size==q.stat().st_size
 duplicates.append({'source':ref(p),'action':'do_not_duplicate_identical_original_build_evidence','identical_retained_canonical':ref(q)})
archive={
 'schema_version':1,'document_kind':'minimal_new_soak_archive_selection',
 'artifact_id':aid,'source_sha':native['source_sha'],'run_id':37871838957,'job_id':113631481157,
 'generated_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),
 'selection_file_project_path':prefix+'/review-runtime/soak-11594089439-archive-selection.json',
 'source_root_absolute':str(root),'source_path_rule':'Entries use project-relative paths and preserve complete actual source file bytes. Do not truncate JSONL or overwrite earlier v2 review files.',
 'official_artifact':{k:native[k] for k in ['artifact_id','artifact_url','archive_download_url','archive_bytes','archive_sha256','artifact_path']},
 'complete_original_archive_retained':True,'selection_is_complete_original_sealed_artifact':False,
 'full_raw_and_stdio_required_for_original_replay':True,
 'selected_files':list(selected.values()),'selected_file_count':len(selected),'selected_bytes':sum(x['source']['bytes'] for x in selected.values()),
 'files':[{'path':str(Path(x['source']['path']).relative_to(prefix)),'bytes':x['source']['bytes'],'sha256':x['source']['sha256']} for x in selected.values()],
 'file_count':len(selected),'total_bytes':sum(x['source']['bytes'] for x in selected.values()),
 'consumer_contract':'files paths are relative to the Mac parent cwd artifacts/benchmarks/p8-bfccb8494ba0; list only full files to copy; selection itself is not included. root archives selection separately.',
 'external_original_large_inputs':excluded,
 'identical_original_build_evidence_not_duplicated':duplicates,
 'unselected_native_fixture_and_git_objects':{'scope':'p8-runtime/project and p8-runtime/fresh-full remain complete only in official artifact / central raw; full member list and all original seal hashes retained. Original identity and 15-table parity were fully reviewed from those originals.','zip_member_inventory':ref(rawbase/'zip-members.json'),'runtime_seal':ref(ex/'p8-runtime/seal.json')},
 'existing_review_references_do_not_recopy':terminal['component_review_references'][:6],
 'existing_pending_references_keep_unchanged':[terminal['prior_pending_snapshot'],terminal['prior_pending_markdown']],
 'shared_script_reuse_allowed_only_if_exact_bytes_match':[ref(review/'runtime-independent-review-v2.py')],
 'index_manifest':ref(indexpath),
 'status_and_execution_boundaries':{'task_status_changes':False,'new_primary_runs':False,'a23_only':True,'candidate_P_execution_credit':False,'partial_archive_not_raw_acceptance_input':True}
}
selection=review/('soak-'+str(aid)+'-archive-selection.json');assert not selection.exists();selection.write_text(json.dumps(archive,ensure_ascii=False,indent=2,sort_keys=True)+'\n')
print(json.dumps({'selection':ref(selection),'selected_file_count':archive['selected_file_count'],'selected_bytes':archive['selected_bytes'],'index_manifest':ref(indexpath),'indices':[{'index':x['index'],'source_rows':x['source_rows'],'source_bytes':x['source_bytes_fully_accounted']} for x in indices],'duplicates_not_repeated':len(duplicates),'all_original_big_files_retained':True,'not_complete_original_sealed_layout':True},ensure_ascii=False))
