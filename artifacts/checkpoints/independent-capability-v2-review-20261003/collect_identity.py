from pathlib import Path
import hashlib,json,subprocess,re,platform
R=Path(__file__).resolve().parent;T=Path('/workspace/.rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin')
def digest(p):return hashlib.sha256(p.read_bytes()).hexdigest()
entries=[]
for name in ['independent-db','independent-default','independent-http','query-fence']:
 p=R/(name+'.log');s=p.read_text();paths=re.findall(r'Running .*?\((/[^)]+)\)',s)
 for x in paths:
  b=Path(x);entry={'run':name,'binary':str(b.relative_to(R)),'sha256':digest(b),'bytes':b.stat().st_size,'test_result':re.findall(r'test result: (.*)',s)}
  for f in (R/'target/debug/.fingerprint').glob('*'+b.name.rsplit('-',1)[-1]+'/*.json'):
   entry.setdefault('cargo_fingerprints',[]).append({'path':str(f.relative_to(R)),'contents':json.loads(f.read_text())})
  entries.append(entry)
p=R/'target/debug/codecortex'
for f in (R/'target/debug/.fingerprint').glob('cc-server-*/bin-codecortex.json'):
 meta=json.loads(f.read_text())
 entries.append({'run':'actual-stdio-query-fence','binary':str(p.relative_to(R)),'sha256':digest(p),'bytes':p.stat().st_size,'cargo_fingerprint':meta,'source_scope':'unmodified final production checkout; semantic-http'})
sqlite=next(Path('/workspace/.cargo/registry/src').glob('*/libsqlite3-sys-0.38.1/sqlite3/sqlite3.c'))
contents=sqlite.read_text();version=re.search(r'#define SQLITE_VERSION\s+"([^"]+)"',contents).group(1)
identity={'platform':platform.platform(),'rustc':subprocess.check_output([str(T/'rustc'),'-Vv']).decode(),'cargo':subprocess.check_output([str(T/'cargo'),'-Vv']).decode(),'rustc_binary_sha256':digest(T/'rustc'),'cargo_binary_sha256':digest(T/'cargo'),'original_lock_sha256':digest(R/'source/Cargo.lock'),'harness_lock_sha256':digest(R/'harness/Cargo.lock'),'sqlite_version':version,'sqlite_source_sha256':digest(sqlite),'sqlite_features':'rusqlite 0.40 bundled,vtab,functions; libsqlite3-sys 0.38.1','binaries':entries,'retention':'local target binaries retained; binaries themselves not committed; hashes tied to run logs and features','temporary_databases':'synthetic; TMPDIR=review directory; DELETE rename standalone; no GC/WAL fault/kill'}
(R/'binary-identity.json').write_text(json.dumps(identity,indent=2)+'\n')
print('recorded',len(entries),'binaries; sqlite',version)
