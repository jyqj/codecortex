from pathlib import Path
import json,re,itertools
r=Path('/workspace/owner-lexical-validation');dest=Path('/workspace/codecortex/artifacts/reviews/query-owner-lexical-spans-correction-20261004');review=Path('/workspace/owner-lexical-review-source');rows=[]
def add(family,body,comparative):rows.append(dict(family=family,query='Which Beacon API '+body+' with Commit?',comparative=comparative))
wraps=[('',''),('(',')'),('[',']'),('{','}'),('"','"'),("'","'"),('`','`'),('“','”'),('‘','’')]
for (a,b),case,ws,(op,cl),nest,extent in itertools.product([('rather','than'),('instead','of')],['lower','upper','mixed'],[' ','\t','\n','\u2003','\u00a0'],wraps,['','round','mixed'],['pair','clause']):
 words=[a,b] if case=='lower' else [a.upper(),b.upper()] if case=='upper' else [a.title(),b.upper()]
 body=op+words[0]+ws+words[1]+(' the Sink' if extent=='clause' else '')+cl
 if nest=='round':body='(('+body+'))'
 if nest=='mixed':body='[{('+body+')} ]'
 add('grouping_product',body,True)
decorations=['{}','`{}`','"{}"',"'{}'",'({})','[{}]','{{{}}}','${}','@{}','#{}','{}()','{}[]','{}::','mod.{}','{}.member','\\{}','{}_suffix','{}X']
for (a,b),left,right,ws,nest in itertools.product([('rather','than'),('instead','of')],decorations,decorations,[' ','\t','\u2003'],['','round']):
 if left==right=='{}':continue
 body=left.format(a)+ws+right.format(b)
 if nest=='round':body='(('+body+'))'
 add('decorations_product',body,False)
for (a,b),gap in itertools.product([('rather','than'),('instead','of')],['','.', '::','/','-','\u200b','; ',': ', ',, ', ' () ', ' ( ) ', ' ` ` ']):add('delimiters',a+gap+b,False)
for a,b in [('rather','than'),('instead','of')]:
 for ws in [' ','\t','\u2003']:add('delimiters',a+','+ws+b,True)
 for template in ['({}]','"{}\'','({}','{})','(({})','call({})','call[{}]','call(({}))','${{{}}}','@({})','({})()','`{}`()','$"{}"','({})suffix','({}).member','"{}"suffix','`{}`.member']:
  add('delimiters',template.format(a+' '+b+' Sink'),False)
for a,b in [('rather','than'),('instead','of')]:
 for op,cl in wraps:
  add('grouping_product',op+a+' '+b+" Sink's"+cl,True)
# Preserve exact reviewer contracts as independent rows, not derived expectations.
probes=json.loads((review/'probes.json').read_text())
for p in probes:rows.append(dict(family='independent',query=p['query'],comparative=p['contract']=='comparative'))
(dest/'lexical-matrix.json').write_text(json.dumps(rows,ensure_ascii=False,indent=2)+'\n')
(r/'lexical-matrix.json').write_bytes((dest/'lexical-matrix.json').read_bytes())
base=(review/'driver.rs').read_text();get=lambda s:[json.loads(x) for x in re.findall(r'^\s*\(("(?:[^"\\]|\\.)*"),',s,re.M)];existing=get(base);extra=[]
# All grouping types x both phrases x pair/clause plus nested and code/delimiter samples.
for (a,b),(op,cl),extent in itertools.product([('rather','than'),('instead','of')],wraps,['pair','clause']):
 body=op+a+' '+b+(' the Sink' if extent=='clause' else '')+cl
 for nested in [body,'[('+body+')]']:
  q='Which Beacon API '+nested+' accepts state?'
  if q not in existing and q not in extra:extra.append(q)
for row in rows:
 if row['family']=='delimiters' and row['query'] not in existing and row['query'] not in extra:extra.append(row['query'])
extra_text=''.join('        ('+json.dumps(q,ensure_ascii=False)+', None),\n' for q in extra)
s=base.replace('    ];\n    let mut result',extra_text+'    ];\n    let mut result');(r/'lexical_driver.rs').write_text(s);(dest/'lexical_driver.rs').write_text(s)
# Explicit baseline/candidate contracts for API selected subset; all original85 tuples unchanged.
contracts={p['query']:p['contract']=='comparative' for p in probes}
for (a,b),(op,cl),extent in itertools.product([('rather','than'),('instead','of')],wraps,['pair','clause']):
 body=op+a+' '+b+(' the Sink' if extent=='clause' else '')+cl
 for nested in [body,'[('+body+')]']:contracts['Which Beacon API '+nested+' accepts state?']=True
for row in rows:
 if row['family']=='delimiters':contracts[row['query']]=row['comparative']
(r/'control-contract.json').write_text(json.dumps({'queries':get(s),'explicit':contracts,'total_queries':len(get(s)),'unit_rows':len(rows),'unit_families':{f:sum(x['family']==f for x in rows) for f in {x['family'] for x in rows}}},ensure_ascii=False,indent=2)+'\n')
(dest/'control-contract.json').write_bytes((r/'control-contract.json').read_bytes())
print('Unit matrix',len(rows),'API driver',len(get(s)))
