#!/usr/bin/env python3
"""Independent source/owner/byte audit. Never imports candidate/review/product code.
Output: aggregate counts and hashes only. Source and question content stays in memory.
"""
import ast, copy, hashlib, json, subprocess
from collections import Counter
from pathlib import Path
HERE=Path(__file__).resolve().parent
ROOT=HERE.parents[2]
CANDIDATE='97c478cdb05d2bb85f852ff42af5452b00ba6e3a'
REVIEW='6c1416109003bcff0c1911307a4af5bd48870517'
ADMISSION='5385f5a7a2a875c6d5cbd049bdde039bf71bbf32'
CP='artifacts/checkpoints/public-dev-declaration-kind-candidate-20261003/'
RP='artifacts/checkpoints/public-dev-kind-review-20261003/'
P='crates/cc-eval/benchmarks/public-v19/'
def require(ok,label):
    if not ok: raise ValueError(label)
def sha(raw): return hashlib.sha256(raw).hexdigest()
def canon(v): return json.dumps(v,sort_keys=True,ensure_ascii=False,separators=(',',':')).encode()
def blob(e): return subprocess.check_output(['git','show',e],cwd=ROOT)
def frozen(p): return blob(CANDIDATE+':'+CP+p)
def jblob(e): return json.loads(blob(e))
def key(r): return tuple(r[k] for k in ('repo','query_ordinal','group_ordinal','alternative_ordinal'))
def unique(rows):
    out={}
    for r in rows:
        k=key(r);require(k not in out,'duplicate');out[k]=r
    return out

def ranges(raw):
    """Independent recursive JSON lexical walker with duplicate-key rejection."""
    text=raw.decode();dec=json.JSONDecoder();out={}
    def ws(i):
        while i<len(text) and text[i].isspace(): i+=1
        return i
    def walk(i,path):
        i=ws(i);start=i
        if text[i]=='{':
            i=ws(i+1);seen=set()
            while text[i]!='}':
                k,i=dec.raw_decode(text,i);require(k not in seen,'duplicate JSON key');seen.add(k)
                i=ws(i);require(text[i]==':','colon');i=ws(walk(i+1,path+(k,)))
                if text[i]=='}':break
                require(text[i]==',','comma');i=ws(i+1)
            i+=1
        elif text[i]=='[':
            i=ws(i+1);n=0
            while text[i]!=']':
                i=ws(walk(i,path+(n,)));n+=1
                if text[i]==']':break
                require(text[i]==',','comma');i=ws(i+1)
            i+=1
        else: _,i=dec.raw_decode(text,i)
        out[path]=(len(text[:start].encode()),len(text[:i].encode()));return i
    require(ws(walk(0,()))==len(text),'trailing JSON');return out

def python_decls(raw):
    tree=ast.parse(raw);parents={child:node for node in ast.walk(tree) for child in ast.iter_child_nodes(node)}
    lines=raw.splitlines(keepends=True);offset=[0]
    for line in lines:offset.append(offset[-1]+len(line))
    out=[]
    for n in ast.walk(tree):
        if not isinstance(n,(ast.ClassDef,ast.FunctionDef,ast.AsyncFunctionDef)):continue
        chain=[];p=parents.get(n)
        while p is not None:
            if isinstance(p,(ast.ClassDef,ast.FunctionDef,ast.AsyncFunctionDef)):chain.append(p)
            p=parents.get(p)
        kind='class' if isinstance(n,ast.ClassDef) else 'method' if chain and isinstance(chain[0],ast.ClassDef) else 'function'
        out.append(dict(name=n.name,kind=kind,scope='.'.join([x.name for x in reversed(chain)]+[n.name]),
                        start=offset[n.lineno-1]+n.col_offset,end=offset[n.end_lineno-1]+n.end_col_offset))
    return out

def go_decls(raw):return json.loads(subprocess.check_output([str(HERE/'.scratch/declarations')],input=json.dumps(raw.decode()).encode()))
def projection(q):
    out=[]
    for g in sorted(q['answers'],key=lambda g:not g['primary']):
        for a in g['alternatives']:
            if a['path'] not in out:out.append(a['path'])
    return out

def validate_changes(changes,expected):
    actual=unique([c['binding'] for c in changes]);require(set(actual)==set(expected),'subset/extra')
    for c in changes:
        k=key(c['binding']);require(c['binding']==expected[k],'forged owner/binding')

def verify_overlay(original, token):
    expected=copy.deepcopy(original);expected['symbol']['kind']='method'
    require(json.loads(token)==expected,'overlay non-kind fields')

def main():
    manifest_raw=frozen('change-manifest.json');artifact_raw=frozen('artifact-manifest.json')
    require(sha(manifest_raw)=='f70f9b2ea562cb7fe8dca44f341e24671af0f84a072ff878e80f10fb67189507','change manifest external pin')
    require(sha(artifact_raw)=='1a0b9ed8a02eb770672d1f0baa7ef330451d865a6d174ea8eab4c8dc823603e9','artifact external pin')
    artifact=json.loads(artifact_raw)
    for p,r in artifact['files'].items():
        raw=frozen(p);require(len(raw)==r['bytes'] and sha(raw)==r['sha256'],'candidate artifact pin')
    inputs=json.loads(frozen('input-manifest.json'))
    for entry,digest in inputs.items():require(sha(blob(entry))==digest,'original input pin')
    m=json.loads(manifest_raw);require(m['status']=='not_admitted' and m['scoring_permitted'] is False,'status')
    proposal=unique(jblob(REVIEW+':'+RP+'item-review.json'))
    validate_changes(m['changes'],proposal)
    changes=unique([dict(c['binding'],change=c) for c in m['changes']])
    replacements=unique(json.loads(frozen('candidate-gold.json'))['replacements'])
    require(set(changes)==set(replacements),'overlay item set')
    admission=jblob(ADMISSION+':'+P+'protocol/global-dev-review/typescript-extension/admission.json')
    require(sha(blob(ADMISSION+':'+P+'protocol/global-dev-review/typescript-extension/admission.json'))==m['previous_admission_sha256'],'admission pin')
    totals=Counter();byrepo={};source_entries=set();semantic_keys=set();derived_pins={}
    for repo in ('requests','gin'):
        receipt=admission['repo_results'][repo];base=receipt['author_sha']+':'+P+repo+'/'
        suite=jblob(receipt['native_dev_suite_entry']);entry=base+suite['queries'];raw=blob(entry)
        compat_suite=jblob(receipt['compat_dev_suite_entry']);compat={q['query_family']:q for q in map(json.loads,blob(base+compat_suite['queries']).splitlines())}
        gold_entry=base+('gold/dev.json' if repo=='requests' else 'gold/evidence.json');gold_raw=blob(gold_entry);gold_ranges=ranges(gold_raw)
        gold={g.get('family_id',g.get('family')):(g,gold_raw[slice(*gold_ranges[(i,)])]) for i,g in enumerate(json.loads(gold_raw))}
        if repo=='requests':
            owner_entries=[receipt['review_sha']+':'+P+'reviews/requests/'+f for f in ('dev20-review.json','block02-review.json','block03-review.json','block04-review.json','delta-dev-review.json')]
        else:owner_entries=[receipt['source_review_sha']+':'+P+'reviews/gin/dev-067-review.json',receipt['review_sha']+':'+P+'reviews/gin/repair-v2/dev-004-repair-review.json']
        owners=[(e,r) for e in owner_entries for r in jblob(e)['rows']]
        counts=Counter();cache={};new_lines=[];file_offset=0
        for qi,line in enumerate(raw.splitlines(keepends=True)):
            q=json.loads(line);lex=ranges(line);g,g_token=gold[q['query_family']];family=q['query_family'];fh=sha(family.encode())
            owned=[(e,r) for e,r in owners if r.get('family_id')==family or r.get('family_id_sha256')==fh]
            require(owned and owned[-1][1]['decision']=='accept','source owner accept')
            last=owned[-1][1]
            if repo=='requests':require(last['question_sha256']==sha(canon(q)) and last['gold_record_sha256']==sha(canon(g)),'owner original raw association')
            else:require(last.get('new_native_row_sha256',last.get('row_sha256'))==sha(line),'owner row pin');require(g==dict(family=family,**q['annotations']['v19']),'Gin source map')
            evidence=g['evidence'] if repo=='requests' else q['annotations']['v19']['gold_evidence'];patches=[];restored=copy.deepcopy(q)
            counts['rows']+=1
            if q['answers']:require(projection(q)==compat[family]['expected_files'],'historical compat projection');counts['compat_projections']+=1
            for gi,group in enumerate(q['answers']):
                for ai,a in enumerate(group['alternatives']):
                    counts['alternatives']+=1;k=(repo,qi,gi,ai);path=a['path'];se=base+'source/'+path;source_entries.add(se)
                    if se not in cache:
                        body=blob(se);require(sha(body)==admission['inputs_sha256'][se],'locked source pin');cache[se]=(body,python_decls(body) if repo=='requests' else go_decls(body))
                    body,defs=cache[se];s,e=a['span']['start'],a['span']['end'];require(0<=s<e<=len(body),'span');body[:s].decode();body[s:e].decode()
                    witnesses=[w for w in evidence if w['path']==path and (w.get('start_byte',w.get('span',{}).get('start')),w.get('end_byte',w.get('span',{}).get('end')))==(s,e)]
                    require(len(witnesses)==1,'unique gold source witness');w=witnesses[0]
                    require(sha(body[s:e])==w.get('span_sha256',w.get('sha256')),'source fragment pin');require(w['symbol'].split('.')[-1]==a['symbol']['name'],'source symbol name')
                    # Independently bind all declaration kinds, including class controls.
                    match=[d for d in defs if d['name']==a['symbol']['name'] and d['start']==s and d['end']<=e and not body[d['end']:e].strip()]
                    require(len(match)==1,'unique syntax declaration');d=match[0]
                    if repo=='requests':require(w['symbol']==d['scope'] and a['symbol']['qname']=='requests.'+Path(path).stem+'.'+d['scope'],'lexical owner/qname')
                    else:
                        if d['owner']:require(w['symbol']==d['owner']+'.'+d['name'],'receiver owner')
                        else:require(w['symbol'].split('.')[-1]==d['name'],'free/type declaration name')
                    needed=a['symbol']['kind']=='function' and d['kind']=='method'
                    require((k in changes)==needed,'complete independently derived delta set')
                    counts['syntax:'+d['kind']]+=1
                    if not needed:continue
                    semantic_keys.add(k);counts['changes']+=1;b=changes[k];c=b['change'];p=c['delta'];altpath=('answers',gi,'alternatives',ai);kindpath=altpath+('symbol','kind');bs,be=lex[kindpath]
                    expected_bindings=dict(source_entry=se,source_sha256=sha(body),sourcecoords=a['span'],source_fragment_sha256=sha(body[s:e]),declarationcoords={'start':d['start'],'end':d['end']},source_map_entry=gold_entry,source_map_record_sha256=sha(canon(w)),owner_reviews=[dict(entry=oe,row_sha256=sha(canon(orow)),decision=orow['decision']) for oe,orow in owned])
                    for field,value in expected_bindings.items():require(b[field]==value,'independent owner/source binding')
                    require(c['source_binding_before']==expected_bindings==c['source_binding_after'],'unchanged source binding')
                    for field,value in dict(original_query_line_raw_sha256=sha(line),original_query_text_json_token_sha256=sha(line[slice(*lex[('query',)])]),original_answers_raw_sha256=sha(line[slice(*lex[('answers',)])]),original_alternative_raw_sha256=sha(line[slice(*lex[altpath])]),original_gold_record_raw_sha256=sha(g_token),original_qname_sha256=sha(canon(a['symbol'].get('qname'))),query_id_sha256=sha(q['id'].encode()),family_id_sha256=fh,primary=group['primary'],required_facet_links=sum(f.get('group_id')==group['id'] for f in q['annotations']['v19']['facets'])).items():require(b[field]==value,'original gold token binding')
                    require(c['before_gold_source_record_raw_sha256']==sha(g_token)==c['after_gold_source_record_raw_sha256'],'source map raw bytes')
                    require(p['pointer']==f'/answers/{gi}/alternatives/{ai}/symbol/kind' and (p['line_byte_start'],p['line_byte_end'])==(bs,be) and (p['file_byte_start'],p['file_byte_end'])==(file_offset+bs,file_offset+be),'patch coordinates')
                    require(line[bs:be]==b'"function"' and p['before_raw_token']=='"function"' and p['after_raw_token']=='"method"','patch tokens')
                    require(p['before_raw_token_sha256']==sha(b'"function"') and p['after_raw_token_sha256']==sha(b'"method"'),'token pins')
                    require(c['candidate_kind']=='method' and c['overlay_applied_in_candidate'] is True,'overlay kind');patches.append((bs,be));restored['answers'][gi]['alternatives'][ai]['symbol']['kind']='method'
                    r=replacements[k];verify_overlay(a,r['alternative_json_token'])
                    altbytes=line[slice(*lex[altpath])];relative=bs-lex[altpath][0];revised_alt=altbytes[:relative]+b'"method"'+altbytes[relative+10:]
                    require(r['alternative_json_token'].encode()==revised_alt and sha(revised_alt)==r['after_alternative_raw_sha256']==p['after_alternative_raw_sha256'],'overlay exact bytes')
                    require(r['before_alternative_raw_sha256']==sha(altbytes) and r['query_id_sha256']==sha(q['id'].encode()),'overlay pins')
            revised=line
            for bs,be in sorted(patches,reverse=True):revised=revised[:bs]+b'"method"'+revised[be:]
            require(json.loads(revised)==restored,'all non-kind fields invariant')
            undo=revised
            for n,(bs,be) in reversed(list(enumerate(sorted(patches)))):
                shifted=bs-2*n;require(undo[shifted:shifted+8]==b'"method"','reverse token');undo=undo[:shifted]+b'"function"'+undo[shifted+8:]
            require(undo==line,'non-kind byte invariant');require(projection(json.loads(revised))==projection(q),'compat projection invariant')
            if patches:
                row=next(r for r in m['rows'] if r['repo']==repo and r['query_ordinal']==qi)
                revised_ranges=ranges(revised)
                require(row['before_line_raw_sha256']==sha(line) and row['after_line_raw_sha256']==sha(revised),'row manifest pins')
                require(row['before_answers_raw_sha256']==sha(line[slice(*lex[('answers',)])]) and row['after_answers_raw_sha256']==sha(revised[slice(*revised_ranges[('answers',)])]),'row answers pins')
                require(row['changes']==len(patches),'row delta count')
            new_lines.append(revised);file_offset+=len(line)
        derived=b''.join(new_lines);f=m['files'][repo];require(sha(raw)==f['before_raw_sha256'] and sha(derived)==f['after_raw_sha256'] and len(raw)==f['before_bytes'] and len(derived)==f['after_bytes'],'derived file pins')
        byrepo[repo]=dict(counts);totals.update(counts);derived_pins[repo]=sha(derived)
    require(semantic_keys==set(proposal)==set(replacements),'complete 166 semantic set')
    legal=json.loads(frozen('license-retention-manifest.json'))
    for path,r in legal.items():require(frozen(path)==blob(r['entry']) and sha(frozen(path))==r['sha256'],'retained license/NOTICE bytes')
    require(totals['alternatives']==268 and totals['changes']==166 and len(source_entries)==41 and len(legal)==9,'scope counts')
    report=dict(source_semantic_verdict='PASS',admission='not_decided; root only',candidate_commit=CANDIDATE,review_commit=REVIEW,original_admission_commit=ADMISSION,change_manifest_sha256=sha(manifest_raw),artifact_manifest_sha256=sha(artifact_raw),candidate_gold_sha256=sha(frozen('candidate-gold.json')),counts_by_repo=byrepo,source_files=41,retained_legal_files=9,derived_native_sha256=derived_pins,new_independent_samples=0,new_scores=0,scope='Only Requests/Python and Gin/Go declaration-kind v2 candidate; Express/JS and TypeScript retain historical taxonomy.',historical_kind_errors_claimed=0,non_kind_byte_changes=0,compat_projection_changes=0)
    (HERE/'result.json').write_text(json.dumps(report,sort_keys=True,indent=2)+'\n');print(json.dumps(report,sort_keys=True))
if __name__=='__main__':main()
