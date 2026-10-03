#!/usr/bin/env python3
"""Source-read candidate authoring. Never invokes retrieval or edits shared files."""
from pathlib import Path
import hashlib,json,sys
ROOT=Path(__file__).resolve().parents[1]
SHA='7ef98448f8b38099ab1ded55e458538ad47a51e7'
S=[]
def add(cat,key,cluster,query,answer,ev,edges=None,absence=None):
    S.append(dict(category=cat,key=key,cluster=cluster,query=query,answer=answer,evidence=ev,edges=edges or [],absence=absence))
# Evidence tuples: upstream path, inclusive line range, source symbol.
E='lib/express.js'; A='lib/application.js'; Q='lib/request.js'; R='lib/response.js'; U='lib/utils.js'; V='lib/view.js'
add('exact/API','entry-export','application-creation','Which public entry file forwards the package export to the application factory module?',"index.js assigns module.exports to require('./lib/express').",[('index.js',9,11,'module.exports')])
add('exact/API','header-alias','request-headers','Which request API aliases share header lookup and how are Referer and Referrer treated?','req.get and req.header share header; names are lowercased, and either spelling returns headers.referrer first, then headers.referer.',[(Q,63,85,'header')])
add('exact/API','status-contract','response-status','What constraints does res.status enforce before setting statusCode?','It requires an integer from 100 through 999; otherwise TypeError or RangeError respectively. It returns the response.',[(R,65,78,'status')])
add('exact/API','view-engine-registration','view-engine','How does app.engine register a callback for an extension without a leading dot?','It validates the callback is a function, prefixes a dot when needed, and stores it in this.engines[extension].',[(A,294,310,'engine')])
add('behavior','response-transfer-length','send-transfer','When res.send has a body and a Transfer-Encoding header, does it add Content-Length?','No. After determining byte length, it adds Content-Length only when Transfer-Encoding is absent.',[(R,168,198,'send')])
add('behavior','jsonp-sanitization','jsonp','How does res.jsonp handle an array callback parameter and unsafe callback characters?','It selects the first array element, accepts a nonempty string, removes characters outside brackets, word characters, dollar and dot, and emits a guarded JavaScript call.',[(R,264,308,'jsonp')])
add('behavior','render-callback-deferral','view-render','How is a synchronously invoked template-engine callback normalized by View.render?','The callback arguments and receiver are copied, then delivered through process.nextTick; callbacks invoked after the engine returns are passed through directly.',[(V,133,158,'render')])
add('behavior','cookie-expiration-clear','cookies','How does res.clearCookie prevent an options.maxAge from preserving the cookie?','It overrides expires with new Date(1), deletes maxAge, and calls cookie with an empty value.',[(R,714,722,'clearCookie')])
add('architecture/facets','application-prototype-wiring','application-creation','How are application behavior and per-application request/response prototypes assembled?','createApplication mixes EventEmitter and application methods into a callable app, creates app.request and app.response from the exported prototypes, assigns their app properties, and initializes the app.',[(E,36,56,'createApplication'),(A,59,83,'init')],['createApplication -> app.init'])
add('architecture/facets','lazy-router','router-lifecycle','Where does the application create its router, and which application settings determine its construction?','app.init installs a router getter; the first access constructs the external Router with caseSensitive and strict flags read from settings. It reuses that router thereafter.',[(A,59,83,'init')])
add('architecture/facets','mounted-inheritance','mount-lifecycle','Which prototype and settings inheritance operations run when an application is mounted?','onmount optionally removes default trust settings for parent inheritance, then sets request, response, engines and settings prototypes to the parent objects.',[(A,90,142,'defaultConfiguration'),(A,190,243,'use')],['use emits mount -> onmount'])
add('crossfile-chain','query-parser-pipeline','query-parser','Trace the query parser setting from app.set to the request query getter.','app.set compiles the setting into query parser fn using compileQueryParser; the query getter reads it and invokes it on parseurl query text, or returns a null-prototype empty object when disabled.',[(A,351,383,'set'),(U,162,183,'compileQueryParser'),(Q,230,243,'query')],['app.set -> compileQueryParser','query -> app.get(query parser fn)','query -> compiled parser'])
add('crossfile-chain','proxy-protocol-pipeline','proxy-trust','Trace how a numeric trust proxy setting affects the protocol getter.','app.set installs compileTrust(val); numeric trust allows hop indices less than val. protocol tests the socket address at hop 0 before accepting the first trimmed X-Forwarded-Proto value.',[(A,351,383,'set'),(U,194,212,'compileTrust'),(Q,297,314,'protocol')],['app.set -> compileTrust','protocol -> app.get(trust proxy fn)','protocol -> trust(socket.remoteAddress,0)'])
add('crossfile-chain','etag-generation-pipeline','etag','Trace weak ETag configuration to response body ETag generation.','app.set compiles weak into wetag; the generator converts non-Buffers to Buffer and calls etag with weak options. res.send calls etag fn when no ETag is already set and a body length exists.',[(A,351,383,'set'),(U,130,151,'compileETag'),(U,51,51,'wetag'),(U,249,257,'createETagGenerator'),(R,168,206,'send')],['app.set -> compileETag','compileETag -> wetag','wetag -> createETagGenerator','send -> etag fn'])
add('crossfile-chain','render-resolution-pipeline','view-resolution','Trace res.render to the source that searches for a view file and an index fallback.','res.render calls app.render, which constructs the configured View. View calls lookup, lookup iterates roots and calls resolve; resolve checks dir/file first, then dir/basename(file,ext)/index+ext.',[(R,896,920,'render'),(A,522,570,'render'),(V,52,95,'View'),(V,104,121,'lookup'),(V,169,186,'resolve')],['res.render -> app.render','app.render -> new View','View -> lookup','lookup -> resolve'])
add('config/error','invalid-etag-setting','etag','Where is an unrecognized ETag setting rejected and what error is raised?','compileETag throws TypeError with unknown value for etag function followed by the value.',[(U,130,151,'compileETag')])
add('config/error','relative-sendfile-error','file-transfer','What precondition rejects a relative path passed to res.sendFile?','If options.root is absent and path is not absolute, sendFile throws TypeError: path must be absolute or specify root to res.sendFile.',[(R,375,402,'sendFile')])
add('hardnegative/noanswer','no-local-router-matcher','router-boundary','Within admitted index.js and lib/*.js only, where is the Router dependency\'s path-to-regexp matching algorithm implemented?','No implementation exists in this scope. Express imports Router and delegates; external router package code is excluded.',[(E,15,21,'Router import'),(A,26,26,'Router import'),(A,256,258,'route')],absence={'absent':'path matching algorithm implementation','boundary':'external router module','tokens':['path-to-regexp','function match','class Layer']})
add('hardnegative/noanswer','no-local-session-store','session-boundary','Within admitted index.js and lib/*.js only, where does Express implement a persistent session store with get/set/destroy methods?','No built-in session store exists in the admitted core. Middleware is registered through app.use; cookie serialization is not session persistence.',[(A,190,243,'use'),(R,747,783,'cookie')],absence={'absent':'persistent session store implementation','boundary':'user-supplied middleware','tokens':['SessionStore','sessionStore','Store.prototype.destroy']})
add('hardnegative/noanswer','no-local-static-filesystem','static-boundary','Within admitted index.js and lib/*.js only, where is the filesystem traversal implementation of express.static defined?','No local implementation exists: express.static directly exports serve-static. res.sendFile is a different response API and delegates streaming to send.',[(E,77,81,'static export'),(R,404,421,'sendFile')],absence={'absent':'serve-static filesystem traversal implementation','boundary':'external serve-static package','tokens':['function serveStatic','exports.static = function']})

def digest(data):return hashlib.sha256(data).hexdigest()
def write_json(path,data):path.write_text(json.dumps(data,ensure_ascii=False,indent=2)+'\n')
def evidence(e):
    path,lo,hi,symbol=e
    raw=(ROOT/'source'/path).read_bytes(); lines=raw.splitlines(keepends=True)
    assert 1<=lo<=hi<=len(lines),(path,lo,hi)
    start=sum(map(len,lines[:lo-1])); end=sum(map(len,lines[:hi])); chunk=raw[start:end]
    assert chunk.strip()
    return {'source_sha':SHA,'path':path,'symbol':symbol,'line_start':lo,'line_end':hi,'byte_start':start,'byte_end':end,'sha256':digest(chunk),'text':chunk.decode()}

def build(count):
    assert len({s['key'] for s in S})==len(S)
    chosen=S[:count]; rows=[]
    for s in chosen:
        family='express-v19-'+s['key']; fh=digest(family.encode()); cluster='express-v19-related-'+s['cluster']; ch=digest(cluster.encode())
        split='holdout' if int(ch[:16],16)%4==0 else 'dev'
        ev=[evidence(e) for e in s['evidence']]; no=s['absence'] is not None
        annotations={'status':'candidate_pending_independent_review','author_role':'B/express','source_sha':SHA,'family_sha256':fh,'related_family_cluster':cluster,'split_hash_sha256':ch,'split_status':'local_candidate_hash_75_25_not_global_freeze','answer_rationale':s['answer'],'source_evidence':ev,'chain_edges':[{'from_evidence':i,'to_evidence':i+1,'relation':edge} for i,edge in enumerate(s['edges'])], 'reviewer':None,'retrieval_inspected':False}
        if s['edges']:
            endpoint_pairs=CHAIN_ENDPOINTS[s['key']]
            assert len(endpoint_pairs)==len(s['edges'])
            annotations['chain_edges']=[]
            for relation,(src,dst) in zip(s['edges'],endpoint_pairs):
                annotations['chain_edges'].append({'relation':relation,'from':{'evidence_index':src,'path':ev[src]['path'],'symbol':ev[src]['symbol']},'to':{'evidence_index':dst,'path':ev[dst]['path'],'symbol':ev[dst]['symbol']},'supporting_evidence':sorted(set([src,dst]))})
        if no:
            files=sorted(p.relative_to(ROOT/'source').as_posix() for p in (ROOT/'source').rglob('*.js'))
            checks=[]
            for token in s['absence']['tokens']:
                hits=[]
                for path in files:
                    for i,line in enumerate((ROOT/'source'/path).read_text().splitlines(),1):
                        if token in line:hits.append({'path':path,'line':i,'text':line})
                checks.append({'literal':token,'hits':hits})
            annotations['absence_proof']={**s['absence'],'scope_files':files,'method':'author read all admitted source; positive delegation/boundary evidence plus literal corroboration; absence not inferred from rank','checks':checks,'scope_digest_sha256':digest(json.dumps([(p,digest((ROOT/'source'/p).read_bytes())) for p in files],separators=(',',':')).encode())}
        rows.append({'id':family,'category':s['category'],'difficulty':3 if s['category'] in ('crossfile-chain','hardnegative/noanswer') else 2,'language':'JavaScript','split':split,'query_family':family,'query':s['query'],'path_prefix':None,'no_answer':no,'expected_files':[],'answers':[] if no else [{'id':'facet-'+str(i),'primary':True,'grade':2,'alternatives':[{'path':e['path'],'symbol':{'name':e['symbol'],'qname':None,'kind':None},'span':{'start':e['byte_start'],'end':e['byte_end']}}]} for i,e in enumerate(ev)],'annotations':annotations})
    name=f'block-{count:03d}'; out=ROOT/'blocks'/name; out.mkdir(exist_ok=True)
    (out/'questions.jsonl').write_text(''.join(json.dumps(r,ensure_ascii=False,separators=(',',':'))+'\n' for r in rows))
    files=sorted(p.relative_to(ROOT/'source').as_posix() for p in (ROOT/'source').rglob('*.js'))
    write_json(out/'suite.json',{'schema_version':1,'name':f'V19 Express {count} candidate families; not reviewed','source':{'root':'../../source','commit':None,'digest':'AUTHORING_REQUIRES_EXPLICIT_FREEZE','files':files},'queries':'questions.jsonl','queries_digest':'AUTHORING_REQUIRES_EXPLICIT_FREEZE','scoring':'codecortex-native-v1','repetitions':1,'warmup':0,'seed':20261003,'timeout_ms':30000,'top_k':10,'engine_config':{'auto_index':{'enabled':False}}})
    from collections import Counter
    write_json(out/'inventory.json',{'candidate_families':len(rows),'independently_reviewed_families':0,'by_category':dict(Counter(r['category'] for r in rows)),'by_split':dict(Counter(r['split'] for r in rows)),'local_split_rule':'sha256(related cluster), first 64 bits modulo 4: 0 holdout, otherwise dev; probabilistic 75/25 target','family_set_sha256':digest('\n'.join(sorted(r['query_family'] for r in rows)).encode()),'questions_sha256':digest((out/'questions.jsonl').read_bytes()),'global_freeze':False,'retrieval_run':False})
    print(name,len(rows))
CHAIN_ENDPOINTS={
 'application-prototype-wiring':[(0,1)], 'mounted-inheritance':[(1,0)],
 'query-parser-pipeline':[(0,1),(2,0),(2,1)],
 'proxy-protocol-pipeline':[(0,1),(2,0),(2,1)],
 'etag-generation-pipeline':[(0,1),(1,2),(2,3),(4,3)],
 'render-resolution-pipeline':[(0,1),(1,2),(2,3),(3,4)],
 'app-call-to-dispatch':[(0,1),(1,1)],
 'default-subdomain-offset':[(0,1),(1,0)],
 'json-settings-serialization':[(0,1),(1,2),(1,3)],
 'jsonp-config-query':[(0,1),(1,2),(2,2)],
 'freshness-response-feedback':[(0,1),(1,0),(0,0)],
 'stale-fresh-inversion':[(0,1),(1,2)],
 'protocol-secure-derivation':[(0,1),(1,3),(2,3)],
 'host-hostname-subdomains':[(0,1),(2,3),(3,4),(4,1)],
 'format-normalization':[(0,1),(0,2),(0,2)],
 'string-send-charset':[(0,1),(0,2)],
 'render-default-response-flow':[(0,1),(1,2),(2,3),(3,0)],
 'render-response-locals':[(0,1),(1,2),(2,2)],
 'verb-route-delegation':[(0,1),(1,2),(2,2)],
 'file-transfer-etag-switch':[(0,1),(1,2),(2,2)],
 'proxy-client-ip':[(0,1),(2,0),(2,2)],
 'default-error-dispatch':[(0,1),(1,2),(2,1)]
}
from additional import extend
extend(add,E,A,Q,R,U,V)
RELATED_CLUSTER_CANONICAL={
 'proxy-ips':'proxy-trust','proxy-host':'proxy-trust','hostname':'proxy-trust','proxy-ip':'proxy-trust','subdomains':'proxy-trust',
 'view-engine':'view-system','view-engine-cache':'view-system','view-render':'view-system','view-resolution':'view-system','view-lookup':'view-system','view-cache':'view-system','view-render-errors':'view-system','render-locals':'view-system','render-default-callback':'view-system',
 'json-stringify':'json-body','send-body-dispatch':'json-body',
 'response-mime':'mime-normalization'
}
for spec in S:spec['cluster']=RELATED_CLUSTER_CANONICAL.get(spec['cluster'],spec['cluster'])
if __name__=='__main__':build(int(sys.argv[1]) if len(sys.argv)>1 else len(S))
