from pathlib import Path
import json,itertools,re
r=Path('/workspace/owner-qualifier-validation');d=Path('/workspace/codecortex/artifacts/reviews/query-owner-closing-qualifiers-correction-20261004');review=r/'reviewer-inputs';rows=json.loads((review/'probes.json').read_text());wraps=[('(',')'),('[',']'),('{','}'),('"','"'),("'","'"),('`','`'),('“','”'),('‘','’')]
code=['::Commit','::','.Commit','.0','->Commit','->','?.Commit','?.[0]','?.()']
prose=['.',':',':Commit','?.',' .Commit',' ::Commit','\u2003::Commit',': :Commit','- >Commit','? .Commit']
for (a,b),(op,cl),extent,depth,suffix in itertools.product([('rather','than'),('instead','of')],wraps,['pair','clause'],['single','outer','inner'],code+prose):
 body=op+a+' '+b+(' the Sink' if extent=='clause' else '')+cl
 if depth=='outer':body='[('+body+')]'+suffix
 elif depth=='inner':body='[('+body+suffix+')]'
 else:body+=suffix
 rows.append(dict(query='Which Beacon API calls '+body+' with Commit?',comparative=suffix in prose,family='closing_qualifier_product'))
for a,b in [('rather','than'),('instead','of')]:
 for suffix in code:rows.append(dict(query=f'Which Beacon API calls {a} {b}{suffix} with Commit?',comparative=False,family='word_qualifier_product'))
# A qualifier on a grouped target is not an ancestor of the comparison words.
for a,b in [('rather','than'),('instead','of')]:
 for suffix in ['::Commit','.Commit','->Commit','?.Commit']:
  rows.append(dict(query=f'Which Beacon API ({a} {b} (Sink){suffix}) accepts state?',comparative=True,family='qualified_target_prose'))
unique={x['query']:x for x in rows};rows=list(unique.values());(d/'qualifier-matrix.json').write_text(json.dumps(rows,ensure_ascii=False,indent=2)+'\n');(r/'qualifier-matrix.json').write_bytes((d/'qualifier-matrix.json').read_bytes())
s=(review/'driver.rs').read_text();get=lambda s:[json.loads(x) for x in re.findall(r'^\s*\(("(?:[^"\\]|\\.)*"),',s,re.M)];existing=get(s)
selected=[x for x in rows if x['family'] in ['word_qualifier_product','qualified_target_prose'] or x['family']=='closing_qualifier_product' and x['query'].endswith(' with Commit?') and any(k in x['query'] for k in ['::Commit','->Commit','?.Commit']) and ' the Sink' in x['query']]
extra=[x for x in selected if x['query'] not in existing];s=s.replace('    ];\n    let mut result',''.join('        ('+json.dumps(x['query'],ensure_ascii=False)+', None),\n' for x in extra)+'    ];\n    let mut result');(r/'qualifier_driver.rs').write_text(s);(d/'qualifier_driver.rs').write_text(s)
contracts=json.loads((review/'controls.json').read_text());contracts['explicit'].update({x['query']:x['comparative'] for x in rows});contracts['queries']=get(s);contracts['total_queries']=len(get(s));contracts['qualifier_cases']=[x['query'] for x in rows if x['family']=='attached_code_suffix' and '::Commit' in x['query']];contracts['unit_rows']=len(rows);(r/'control-contract.json').write_text(json.dumps(contracts,ensure_ascii=False,indent=2)+'\n');(d/'control-contract.json').write_bytes((r/'control-contract.json').read_bytes());print(len(rows),'bounded rows;',len(get(s)),'driver queries;',len(contracts['qualifier_cases']),'exact reviewer qualifiers')
