#!/usr/bin/env python3
"""Source-only authoring; no retrieval, ranking or provider calls."""
import hashlib,json,pathlib,re,sys
R=pathlib.Path(__file__).resolve().parent
sys.path.insert(0,str(R/'.validation-deps'))
import blake3,jsonschema
SHA='ed4807212c28c90777c1d7ef2bf8e47af5d08519'
P='packages/typescript/src/api/'
def sha(b):return hashlib.sha256(b).hexdigest()
def dump(p,x):p.write_text(json.dumps(x,indent=2,ensure_ascii=False)+'\n')
def ev(file,symbol):
 p=P+file;b=(R/'source'/p).read_bytes(); lines=b.splitlines(keepends=True)
 pat=re.compile(rb'^(?:export )?(?:function|class|interface|const) '+symbol.encode()+rb'\b')
 i=next(i for i,l in enumerate(lines) if pat.search(l))
 j=i
 if not (lines[i].rstrip().endswith(b';') or lines[i].rstrip().endswith(b'}')):
  j=next(j for j in range(i+1,len(lines)) if re.match(rb'^}(?:;)?\s*$',lines[j]))
 a=sum(map(len,lines[:i]));z=sum(map(len,lines[:j+1])); s=b[a:z]
 return {'path':p,'symbol':{'name':symbol,'qname':None,'kind':None},'span':{'start':a,'end':z}}, {'source_sha':SHA,'path':p,'symbol':symbol,'start_line':i+1,'end_line':j+1,'start_byte':a,'end_byte':z,'span_sha256':sha(s),'file_sha256':sha(b)}
# Each tuple names a separate source obligation. Related variants must reuse this family.
S=[
('spawn-discriminant','exact_api','Locate isSpawnOptions, the ClientOptions discriminant helper.', [('options.ts','isSpawnOptions')], 'The absence of the pipe property identifies spawn options.'),
('executable-fallback','exact_api','Locate resolveExePath, which selects the executable override or bundled executable.', [('options.ts','resolveExePath')], 'tsserverPath uses nullish fallback to getExePath().'),
('windows-volume','exact_api','Locate splitVolumePath, returning the Windows volume, remainder and recognition flag.', [('path.ts','splitVolumePath')], 'A letter plus colon yields a lowercased volume; otherwise the input is the remainder.'),
('symbol-owner-validation','exact_api','Locate validateSymbolOwner, which validates file and snapshot ownership shapes.', [('proto.ts','validateSymbolOwner')], 'File owners require file and exclude snapshot/project; snapshot owners require snapshot/project and exclude file.'),
('absolute-combination','behavior','Where does combining paths replace the previous prefix when a later path is rooted, while leaving relative segments unsimplified?', [('path.ts','combinePaths')], 'Rooted paths replace the accumulator; relative paths append with a separator.'),
('wtf8-lone-surrogates','behavior','Where does text encoding preserve unpaired UTF-16 surrogate code units instead of always replacing them with U+FFFD?', [('node/wtf8.ts','encodeWtf8')], 'The manual branch encodes lone surrogate code units in three bytes, while paired surrogates use four bytes.'),
('diagnostic-message-tree','behavior','Where are nested diagnostic message chains recursively flattened with indentation and host newlines?', [('diagnosticFormatter.ts','flattenDiagnosticMessage')], 'Children recurse with indentLevel + 1; nonzero levels add newline and two spaces per level.'),
('deprecated-project-shim','behavior','Where is deprecated openProject folded ahead of openProjects and omitted from the resulting wire object?', [('proto.ts','toCreateSnapshotRequest')], 'Destructuring removes openProject; resolveFileName(openProject) is prepended only when defined.'),
('filesystem-modes','architecture_facets','Identify both public factories distinguishing a full request filesystem from a layering filesystem.', [('fs.ts','createFileSystem'),('fs.ts','createFileSystemLayer')], 'The two factories pass full and layer respectively to createRequestFileSystem.'),
('callback-contract-table','architecture_facets','Identify the filesystem sentinel definitions and the per-operation sentinel compatibility table.', [('fs.ts','serverFS'),('fsCallbacks.ts','fileSystemCallbackTable')], 'Unique-symbol sentinels represent server behavior; each callback lists allowed sentinels.'),
('diagnostic-rendering-modes','architecture_facets','Identify the plain diagnostic renderer and the renderer adding ANSI colors, context and related information.', [('diagnosticFormatter.ts','formatDiagnostics'),('diagnosticFormatter.ts','formatDiagnosticsWithColorAndContext')], 'The plain and contextual entry points have distinct output construction.'),
('request-file-uri-normalization','crossfile_chain','Trace request filesystem entry identifiers through URI-to-file resolution and path normalization before insertion.', [('fs.ts','createRequestFileSystem'),('proto.ts','resolveFileName'),('path.ts','documentURIToFileName'),('path.ts','normalizePath')], 'createRequestFileSystem calls resolveFileName then normalizePath; URI identifiers call documentURIToFileName.'),
('diagnostic-relative-path','crossfile_chain','Trace the plain diagnostic file location through the host-aware relative-name helper to path conversion.', [('diagnosticFormatter.ts','formatDiagnostics'),('diagnosticFormatter.ts','relativeFileName'),('path.ts','convertToRelativePath')], 'formatDiagnostics calls relativeFileName, which supplies host directory and canonicalizer to convertToRelativePath.'),
('library-filesystem-mount','crossfile_chain','Trace the default-library filesystem factory into path normalization and the full request filesystem builder.', [('fs.ts','createFileSystemWithLib'),('path.ts','normalizePath'),('fs.ts','createRequestFileSystem')], 'Default library candidates are normalized and mounted via host symlinks before full filesystem creation.'),
('identifier-to-document-uri','crossfile_chain','Trace a string DocumentIdentifier to its document URI across the protocol and path modules.', [('proto.ts','resolveDocumentURI'),('path.ts','fileNameToDocumentURI')], 'String identifiers call fileNameToDocumentURI; URI objects return their uri directly.'),
('callback-config-error','config_error','Where are non-function, unsupported filesystem callback values rejected while supported non-useOS sentinels become CLI arguments?', [('fsCallbacks.ts','configureFileSystemCallbacks')], 'Functions are registered, supported sentinels become name:sentinel arguments except useOS, and other values throw TypeError.'),
('rooted-validation-error','config_error','Where does rootedPathFromNormalized reject a path failing validation, and what conditions does its validator inspect?', [('path.ts','rootedPathFromNormalized'),('path.ts','tryRootedPathFromNormalized')], 'Failure throws Path is not rooted and normalized; validator checks roots, backslashes, relative segments, URL suffixes and separators.'),
]
N=[
('transport-retry-policy','options.ts','Within packages/typescript/src/api/options.ts only, locate a configurable transport retry or backoff policy.',['retry','backoff'],'The complete option interfaces and process-argument builder contain no retry/backoff policy.'),
('mkdir-callback','fsCallbacks.ts','Within packages/typescript/src/api/fsCallbacks.ts only, locate registration or encoding of a mkdir filesystem callback.',['mkdir'],'The complete callback table enumerates readFile, fileExists, directoryExists, getAccessibleEntries, realpath, stat, writeFile and removeFile; it has no mkdir operation.'),
('wtf8-file-persistence','node/wtf8.ts','Within packages/typescript/src/api/node/wtf8.ts only, locate code that persists encoded bytes to a disk file.',['writeFile','node:fs','createWriteStream'],'The complete module operates on strings, byte arrays and TextDecoder; it imports node:buffer and has no filesystem persistence implementation.'),
]
rows=[]; evidence=[]
for slug,category,query,targets,rationale in S:
 family='ts-v19:'+slug; fh=sha(family.encode());ans=[];ee=[]
 for n,(file,symbol) in enumerate(targets):
  a,e=ev(file,symbol);ans.append({'id':'facet-'+str(n+1),'primary':True,'grade':2,'alternatives':[a]});ee.append(e)
 edges=[]
 if category=='crossfile_chain':
  links={'request-file-uri-normalization':[(0,1),(0,3),(1,2)],'diagnostic-relative-path':[(0,1),(1,2)],'library-filesystem-mount':[(0,1),(0,2)],'identifier-to-document-uri':[(0,1)]}[slug]
  for a,b in links:
   src=ee[a]; raw=(R/'source'/src['path']).read_bytes(); token=ee[b]['symbol'].encode()+b'('; loc=raw.find(token,src['start_byte'],src['end_byte']);assert loc>=0
   edges.append({'from_facet':a+1,'to_facet':b+1,'relation':'calls','path':src['path'],'start_byte':loc,'end_byte':loc+len(token),'source_sha':SHA})
 rows.append({'id':family,'category':category,'difficulty':3 if category in ['crossfile_chain','architecture_facets'] else 2,'language':'typescript','split':'holdout' if int(fh[:8],16)%4==0 else 'dev','query_family':family,'query':query,'path_prefix':None,'no_answer':False,'expected_files':[],'answers':ans,'annotations':{'status':'candidate_pending_independent_review','family_hash_sha256':fh,'source_sha':SHA,'gold_evidence':ee,'answer_rationale':rationale,'chain_edges':edges}})
for slug,file,query,tokens,note in N:
 family='ts-v19:'+slug;fh=sha(family.encode());p=P+file;b=(R/'source'/p).read_bytes();assert all(t.encode().lower() not in b.lower() for t in tokens)
 rows.append({'id':family,'category':'hardnegative_noanswer','difficulty':3,'language':'typescript','split':'holdout' if int(fh[:8],16)%4==0 else 'dev','query_family':family,'query':query,'path_prefix':p,'no_answer':True,'expected_files':[],'answers':[],'annotations':{'status':'candidate_pending_independent_review','family_hash_sha256':fh,'source_sha':SHA,'absence_evidence':{'scope_files':[p],'file_sha256':sha(b),'checked_lines':[1,len(b.splitlines())],'checked_bytes':[0,len(b)],'absent_tokens':tokens,'source_read_reason':note,'basis':'complete scoped source read and token check, never retrieval rank'}}})
# Related source obligations share a split cluster; allocation uses only stable hashes.
clusters={
 'client-process-options':['spawn-discriminant','executable-fallback','transport-retry-policy'],
 'wtf8':['wtf8-lone-surrogates','wtf8-file-persistence'],
 'diagnostics':['diagnostic-message-tree','diagnostic-rendering-modes','diagnostic-relative-path'],
 'request-filesystem-identifiers':['filesystem-modes','request-file-uri-normalization','library-filesystem-mount','identifier-to-document-uri'],
 'filesystem-callbacks':['callback-contract-table','callback-config-error','mkdir-callback'],
}
for q in rows:
 slug=q['query_family'].removeprefix('ts-v19:')
 q['annotations']['split_cluster']=next((g for g,members in clusters.items() if slug in members),slug)
# Preregistered candidate block seed selected solely for 15/5 count, never results.
salt='v19-typescript-candidate-v1:3'
def split_hash(q):return sha((salt+':'+q['annotations']['split_cluster']).encode())
for q in rows:
 h=split_hash(q);q['split']='holdout' if int(h[:8],16)%4==0 else 'dev'
 q['annotations']['split_hash_sha256']=h
 q['annotations']['split_salt']=salt
(R/'questions').mkdir(exist_ok=True)
qp=R/'questions/candidates.jsonl';qp.write_text(''.join(json.dumps(q,ensure_ascii=False,separators=(',',':'))+'\n' for q in rows))
files=[]
for p in sorted((R/'source').rglob('*.ts')):
 b=p.read_bytes();assert len(b)<=1000000 and b'\0' not in b
 files.append({'path':p.relative_to(R/'source').as_posix(),'bytes':len(b),'digest':blake3.blake3(b).hexdigest()})
source_digest=blake3.blake3(json.dumps(files,separators=(',',':')).encode()).hexdigest()
suite={'schema_version':1,'name':'V19 TypeScript candidate block 01; not reviewed or global split frozen','source':{'root':'source','commit':None,'digest':source_digest,'files':[f['path'] for f in files]},'queries':'questions/candidates.jsonl','queries_digest':blake3.blake3(qp.read_bytes()).hexdigest(),'scoring':'codecortex-native-v1','repetitions':1,'warmup':0,'seed':27,'timeout_ms':30000,'top_k':10,'engine_config':{'auto_index':{'enabled':False}}}
dump(R/'suite.candidate.json',suite)
repo=R.parents[4]
schema=json.loads((repo/'crates/cc-eval/benchmarks/schema/query.schema.json').read_text())
for q in rows:jsonschema.validate(q,schema)
dump(R/'provenance.json',{'repository':'microsoft/TypeScript','upstream_sha':SHA,'base_sha':'bc8e22bd6b4f85774e7b84a3e6a30da8dc4a3b29','source_kind':'byte-identical admitted public snapshot; suite commit null because partial snapshot is not a Git checkout','source_url':f'https://github.com/microsoft/TypeScript/tree/{SHA}','files':[{**f,'sha256':sha((R/'source'/f['path']).read_bytes())} for f in files],'license_sha256':sha((R/'license/LICENSE.txt').read_bytes()),'independent_review':None,'excluded':['packages/typescript/vendor/**','**/*.generated.ts','packages/typescript/src/enums/** (generated content)','tsc/internal/bundled/libs/**','tests/**','binaries, dependencies and all unlisted files'],'third_party_retention':'NOTICE.txt retained in full; path.ts references vscode-uri at edfdccd976efaf4bb8fdeca87e97c47257721729; its MIT license retained separately','license_artifacts':[{'path':str(p.relative_to(R)), 'sha256':sha(p.read_bytes())} for p in sorted((R/'license').iterdir())], 'vscode_uri_license_url':'https://raw.githubusercontent.com/microsoft/vscode-uri/edfdccd976efaf4bb8fdeca87e97c47257721729/LICENSE.md', 'archive_sha256':sha(pathlib.Path('/tmp/typescript-v19.tar.gz').read_bytes())})
dump(R/'validation.json',{'candidate_families':len(rows),'categories':{c:sum(q['category']==c for q in rows) for c in sorted({q['category'] for q in rows})},'splits':{s:sum(q['split']==s for q in rows) for s in ['dev','holdout']},'questions_sha256':sha(qp.read_bytes()),'schema_validation':'20 query rows passed JSON Schema','gold_span_validation':'byte-exact symbol evidence and chain call token checks passed','license_lock_valid':sha((R/'license/LICENSE.txt').read_bytes())=='a7d00bfd54525bc694b6e32f64c7ebcf5e6b7ae3657be5cc12767bce74654a47','retrieval_run':False,'global_split_frozen':False,'review_status':'candidate; independent author review required'})
print(json.dumps(json.loads((R/'validation.json').read_text())))
