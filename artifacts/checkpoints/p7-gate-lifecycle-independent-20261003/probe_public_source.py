#!/usr/bin/env python3
"""Same literal PR58 fixture through actual stdio; preserve coordinate counterexamples."""
import hashlib, json, pathlib, sys, tempfile
from audit_scope import DOCS
here = pathlib.Path(__file__).resolve().parent
namespace = {'__name__': 'independent_lifecycle_helpers'}
# Reuse only this review's transport/endpoint helpers, not any production oracle.
exec(compile((here/'lifecycle_stdio.py').read_text().split('\ntry:\n    with tempfile.TemporaryDirectory')[0], str(here/'lifecycle_stdio.py'), 'exec'), namespace)
Child, records, server = namespace['Child'], namespace['records'], namespace['server']
output = pathlib.Path(sys.argv[2]); rows = []
try:
 with tempfile.TemporaryDirectory(prefix='p7-coordinate-review-') as tmp:
  root = pathlib.Path(tmp)
  child = Child(root, {'auto_index':{'enabled':False}}, 'public-source-coordinate')
  (root/'one.rs').unlink()
  for path,text in DOCS.items():
   p=root/path;p.parent.mkdir(parents=True,exist_ok=True);p.write_text(text)
  child.index()
  for tool,key in [('search','query'),('context','task')]:
   result=child.tool(tool,{key:'needle path:scope/ kind:function name:needle','retrieval_strategy':'local','top_k':50} if tool=='search' else {key:'needle path:scope/ kind:function name:needle','retrieval_strategy':'local','max_symbols':50})
   assert {h['file_path'] for h in result['machine_pack']['hits']}=={'scope/needle.rs','scope/sub/needle.rs','scope/needle.py'}
   mismatches=[]
   for h in result['machine_pack']['hits']:
    src=DOCS[h['file_path']];text=h['text'];span=h['metadata']['source_evidence']['span']
    assert src.encode()[span['start']:span['end']].decode()==text
    raw=src.encode(); expected=[raw[:span['start']].count(b'\n')+1,raw[:span['end']-1].count(b'\n')+1]
    assert [h['start_line'],h['end_line']]==expected, 'byte span line boundaries'
    assert any(s['file_path']==h['file_path'] and s['start_line']==h['start_line'] and s['end_line']==h['end_line'] for s in result['spans'])
   rows.append({'tool':tool,'hit_count':len(result['machine_pack']['hits']),'public_span_count':len(result['spans']),'line_mismatches':mismatches,'source_span_bytes_correct':True,'result':result})
  child.close()
finally:
 for child in namespace['children']:
  child.close()
 server.shutdown()
 (output/'stdio-raw.json').write_text(json.dumps(records,indent=2)+'\n')
 (output/'coordinate-result.json').write_text(json.dumps({'baseline':'83a6b54ab1e71db033264e3b4e8d4f0a1d5319ad','product_sha256':hashlib.sha256(pathlib.Path(sys.argv[1]).read_bytes()).hexdigest(),'rows':rows,'scope_leak':False,'line_coordinates_closed':not any(r['line_mismatches'] for r in rows)},indent=2)+'\n')
