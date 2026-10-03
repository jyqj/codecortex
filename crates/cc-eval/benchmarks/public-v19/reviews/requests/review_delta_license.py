#!/usr/bin/env python3
"""Independent fixed official-archive checks; never read query or holdout bodies."""
import ast,hashlib,io,json,pathlib,re,subprocess,tarfile,urllib.request
HERE=pathlib.Path(__file__).resolve().parent;ROOT=HERE.parents[5];P=ROOT/'crates/cc-eval/benchmarks/public-v19/requests';BASE='e49f9ada5826b4206d2128f3bfb8d31603ff42fa'
sha=lambda b:hashlib.sha256(b).hexdigest();inputs={}
def read(rel):
 p=P/rel;b=p.read_bytes();assert b==subprocess.check_output(['git','show',BASE+':'+str(p.relative_to(ROOT))],cwd=ROOT);inputs[rel]=sha(b);return b
checks=[]
for repo,commit,items,expected in [('pallets/werkzeug','d902d2c05a2cb3d7d1f5414fe9c678273f5ffa05',{'LICENSE':'LICENSE','AUTHORS':'AUTHORS','werkzeug/http.py':'werkzeug/http.py'},'5771eba4567faa089aaae9a17be947759e3fa1ed1fa1d244f7c1efbb96c1abe3'),('psf/requests','9966017a4976339c76b834eab8a10b4dfba474f1',{'LICENSE':'requests-introduction-LICENSE','AUTHORS':'requests-introduction-AUTHORS','NOTICE':'requests-introduction-NOTICE','requests/utils.py':'requests-introduction-utils.py'},None)]:
 url=f'https://codeload.github.com/{repo}/tar.gz/{commit}'
 with urllib.request.urlopen(url,timeout=40) as response:data=response.read()
 if expected:assert sha(data)==expected
 archive=tarfile.open(fileobj=io.BytesIO(data),mode='r:gz');members={m.name.split('/',1)[1]:m for m in archive.getmembers() if '/' in m.name and m.isfile()};retained=[]
 for origin,local in items.items():
  b=archive.extractfile(members[origin]).read();assert b==read('license/werkzeug-0.6.2/'+local);retained.append({'artifact_id':local,'sha256':sha(b),'bytes':len(b),'official_byte_match':True})
 checks.append({'repository':repo,'commit':commit,'url':url,'archive_sha256':sha(data),'retained':retained})
class Normalize(ast.NodeTransformer):
 def visit_FunctionDef(self,n):
  n=self.generic_visit(n);n.returns=None;n.type_comment=None
  for arg in n.args.posonlyargs+n.args.args+n.args.kwonlyargs:arg.annotation=None
  if n.args.vararg:n.args.vararg.annotation=None
  if n.args.kwarg:n.args.kwarg.annotation=None
  if n.body and isinstance(n.body[0],ast.Expr) and isinstance(n.body[0].value,ast.Constant) and isinstance(n.body[0].value.value,str):n.body=n.body[1:]
  return n
 def visit_AnnAssign(self,n):
  assert n.value is not None
  return ast.Assign(targets=[n.target],value=n.value)
# Parse complete top-level helper definitions independently. Unrelated historic
# Python 2 exception syntax elsewhere in the module is not normalized or executed.
def helper_node(raw,symbol):
 text=raw.decode();start=re.search(r'^def '+re.escape(symbol)+r'\(',text,re.M).start();nextdef=re.search(r'^def ',text[start+4:],re.M);end=start+4+nextdef.start() if nextdef else len(text)
 tree=ast.parse(text[start:end]);assert len(tree.body)==1 and isinstance(tree.body[0],ast.FunctionDef) and tree.body[0].name==symbol;return tree.body[0]
norm=[]
for symbol in ['parse_list_header','parse_dict_header','unquote_header_value']:
 hashes=[]
 for path in ['license/werkzeug-0.6.2/werkzeug/http.py','license/werkzeug-0.6.2/requests-introduction-utils.py','source/src/requests/utils.py']:
  hashes.append(sha(ast.dump(Normalize().visit(helper_node(read(path),symbol)),include_attributes=False).encode()))
 assert len(set(hashes))==1;norm.append({'helper_id':symbol,'normalized_ast_sha256':hashes[0],'three_way_equal':True})
license=read('license/werkzeug-0.6.2/LICENSE').decode();assert all(x in license for x in ['Copyright (c) 2010','Redistribution and use in source and binary forms','Redistributions of source code must retain','Redistributions in binary form must reproduce','names of the contributors may not be used to endorse','THIS SOFTWARE IS PROVIDED'])
notice=read('license/THIRD-PARTY-NOTICES.md').decode();assert 'werkzeug-0.6.2/LICENSE' in notice and 'werkzeug-0.6.2/AUTHORS' in notice
raw=read('source/src/requests/utils.py');assert raw.count(b'From mitsuhiko/werkzeug (used with permission).')==3
old=json.loads((HERE/'block04-review.json').read_bytes());assert sha(raw)==old['read_allowlist_sha256']['source/src/requests/utils.py']
report={'author_sha':BASE,'official_reads':checks,'normalized_helper_lineage':norm,'normalization':'remove docstrings and type annotations; AnnAssign to Assign; no algorithm normalization','independent_license_evidence':'accept_with_full_BSD_notice_retention','source_suite_admission':False,'private_permission_inferred':False,'exact_historical_copy_release_claimed':False,'gold_admission_for_helpers':False,'holdout_bodies_read':False,'read_allowlist_sha256':inputs}
(HERE/'delta-license-review.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({'official_archive_matches':len(checks),'helper_lineage_matches':len(norm),'license_evidence':report['independent_license_evidence']}))
