from pathlib import Path
import gzip,hashlib,io,json,re,stat,tarfile
OLD=Path('/workspace/scratch/a217aaae3bde/checkpoint-round19-prep/pr-publication-history-payload')
OUT=Path('/workspace/scratch/a217aaae3bde/checkpoint-round19-prep/pr-publication-history-payload-v2')
def sha(b):return hashlib.sha256(b).hexdigest()
def blob(b):return hashlib.sha1(b'blob '+str(len(b)).encode()+b'\0'+b).hexdigest()
def rec(p):
 b=p.read_bytes();return {'path':str(p),'bytes':len(b),'sha256':sha(b),'git_blob_sha1':blob(b)}
m=json.loads((OLD/'payload-manifest.json').read_text());assert sha((OLD/'payload-manifest.json').read_bytes())=='cb001ef602e9536e08ce9f0c3038b6dbc39595662e92d94639fccf15e80406f8'
excluded='round17-pr-triage/pr161-files-page1-1791516844972.json';e=next(x for x in m['members'] if x['member']==excluded);raw=Path(e['source_path']).read_bytes()
assert sha(raw)==e['sha256']=='8b7e28bca93a7cfec7774346b20e464ad33187df52c2082304ce24ee4f4dfc4f'
assert len(re.findall(rb'file_[0-9a-f]{24,}',raw))==5
correction={'schema':'PR-history-v1-publication-selection-correction-v2','v1_archive':m['archive'],'v1_manifest':rec(OLD/'payload-manifest.json'),'v1_published_by_this_task':False,'v1_review_status':'blocked_by_independent_peer_before_publication','reason':'Initial URL-only scan omitted non-URL transfer reference identifiers embedded in an original PR diff API capture. Peer full-member scan found exactly one affected capture and five actual file identifiers.','excluded_original':e,'match_types':['download_file_reference with file identifier'],'match_count':5,'private_identifier_values_included':False,'preservation':'Original capture and v1 complete package remain unchanged locally. The v2 public package excludes the entire original capture; no redacted derivative is mislabeled as an original. Its complete PR filename inventory and Git-tree/source review captures remain present.','v2_scope':'102 original selection members remain (including replacement README scope document); adds this correction and a value-free peer count inventory. No native CI result or original task state changed.','formal_task_completion':False,'counts':{'done':163,'remaining':29}}
(OUT/'publication-selection-correction.json').write_text(json.dumps(correction,ensure_ascii=False,indent=2)+'\n')
old_readme=(OLD/'README.md').read_text()
readme='''# PR and publication history: revision 2\n\nThis public selection excludes exactly one original API capture because it embeds five private transport file identifiers in a PR diff. See publication-selection-correction.json for the complete source/member path, bytes, SHA256 and Git blob, reason, and original v1 archive identity. No identifier values appear in this exclusion record. The original capture and rejected v1 package remain unchanged locally; this is an explicit whole-file exclusion, not a redacted file represented as original. Do not claim that all 103 v1 captures were publicly delivered.\n\nAll retained v1 original source bytes and modes are unchanged. The README is replaced with this corrected selection explanation; the complete CI logs, full PR filename inventories, exact Git tree/source captures, partial-publication history and actual-merge proof remain included. The two added correction/inventory documents contain only value-free match types/counts and original hashes.\n\n'''+old_readme.replace('No private transfer reference or signed download URL is included.','This revision was scanned for actual file identifiers, library/transfer URIs and signed URLs after explicit exclusion; none remain in its members.')
(OUT/'README.md').write_text(readme)
sources={x['member']:Path(x['source_path']) for x in m['members'] if x['member'] not in [excluded,'README.md']}
for x in m['members']:
 if x['member'] not in [excluded,'README.md']:
  b=Path(x['source_path']).read_bytes();assert len(b)==x['bytes'] and sha(b)==x['sha256'] and blob(b)==x['git_blob_sha1'] and stat.S_IMODE(Path(x['source_path']).stat().st_mode)==x['source_mode']
sources['README.md']=OUT/'README.md';sources['publication-selection-correction.json']=OUT/'publication-selection-correction.json'
sources['peer-private-reference-count-inventory.json']=Path('/dev/shm/a217aaae3bde/scale-combined-E-review/round19-history-private-reference-inventory.json')
entries=[]
for n,p in sorted(sources.items()):
 b=p.read_bytes();s=b.decode();assert not re.search(r'file_[0-9a-f]{24,}',s,re.I),n
 assert not re.search(r'(?:library|download-ref|private-transfer)://',s,re.I),n
 for u in re.findall(r'https?://[^\s"\'<>\\]+',s):assert not re.search(r'[?&](?:sig|signature|X-Amz-Signature|token|se)=',u,re.I),n
 mode=stat.S_IMODE(p.stat().st_mode);assert mode in (0o644,0o755)
 entries.append({'member':n,'source_path':str(p),'source_mode':mode,'archive_mode':mode,'bytes':len(b),'sha256':sha(b),'git_blob_sha1':blob(b)})
def make():
 o=io.BytesIO()
 with gzip.GzipFile(fileobj=o,mode='wb',filename='',mtime=0,compresslevel=9) as gz:
  with tarfile.open(fileobj=gz,mode='w',format=tarfile.USTAR_FORMAT) as t:
   for x in entries:
    b=Path(x['source_path']).read_bytes();assert sha(b)==x['sha256'];i=tarfile.TarInfo(x['member']);i.size=len(b);i.mode=x['archive_mode'];i.uid=i.gid=i.mtime=0;i.uname=i.gname='';t.addfile(i,io.BytesIO(b))
 return o.getvalue()
b=make();assert make()==b and len(b)<=4194304;archive=OUT/'pr-publication-history-evidence-v2.tar.gz';archive.write_bytes(b)
with tarfile.open(archive,'r:gz') as t:
 members=t.getmembers();assert [x.name for x in members]==[x['member'] for x in entries]
 for i,x in zip(members,entries):
  raw=t.extractfile(i).read();assert raw==Path(x['source_path']).read_bytes() and sha(raw)==x['sha256'] and blob(raw)==x['git_blob_sha1'];assert i.isfile() and not i.pax_headers and i.mode==x['source_mode'];assert (i.uid,i.gid,i.mtime,i.uname,i.gname)==(0,0,0,'','')
assert excluded not in sources and len(entries)==104
assert sha(Path(e['source_path']).read_bytes())==e['sha256'] and sha(Path(m['archive']['path']).read_bytes())==m['archive']['sha256']
result={**{k:v for k,v in m.items() if k not in ['archive','member_count','uncompressed_bytes','members','schema']},'schema':'round19-pr-publication-history-audit-package-v2','archive':rec(archive),'member_count':len(entries),'uncompressed_bytes':sum(x['bytes'] for x in entries),'members':entries,'excluded_original_captures':[e],'exclusion_reason':'Five actual non-URL private transfer file identifiers; entire original API capture excluded, original remains preserved unchanged; no value printed.','private_identifier_values_included':False,'actual_file_identifier_scan_matches':0,'library_or_private_transfer_URI_scan_matches':0,'signed_URL_scan_matches':0,'v1_failure_preserved':rec(OUT/'publication-selection-correction.json'),'retained_v1_member_bytes_and_modes_unchanged_except_explicit_README_replacement':True,'original_v1_archive_and_excluded_source_unchanged':True}
(OUT/'payload-manifest.json').write_text(json.dumps(result,ensure_ascii=False,indent=2)+'\n');print(json.dumps({'archive':rec(archive),'manifest':rec(OUT/'payload-manifest.json'),'members':len(entries),'raw_bytes':result['uncompressed_bytes']}))
