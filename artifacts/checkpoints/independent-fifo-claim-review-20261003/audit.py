from pathlib import Path
import json,hashlib,subprocess,sqlite3,re
p=Path(__file__).resolve().parent
def save(n,d):(p/n).write_text(json.dumps(d,indent=2,ensure_ascii=False)+'\n')
files=['crates/cc-db/src/semantic_outbox.rs','crates/cc-db/src/index_migrate.rs','crates/cc-db/src/sql/index_v1.sql']
shas=['5ffbadcf48e26523b2eb46beda0d187a2e2e29cd','c73128b500c2a19daa8a293caa52afb711b03516','807f4710495a38bc6631549da2ab9623c83684b8','52a50730b58e2b59351449fc80f65771b24c26a8']
def blob(s,f):return subprocess.check_output(['git','show',s+':'+f])
hashes={s:{f:hashlib.sha256(blob(s,f)).hexdigest() for f in files} for s in shas}
assert hashes[shas[1]]==hashes[shas[2]]==hashes[shas[3]]
save('production-byte-identity.json',{'hashes':hashes,'production_final_lintfix_identical':True,'lintfix_remote_head':shas[3],'reviewed_commit':shas[2]})
old=(p/'local/old/crates/cc-db/src/semantic_outbox.rs').read_text().split('pub fn claim_next_fair_on')[1]
new=(p/'local/candidate/crates/cc-db/src/semantic_outbox.rs').read_text().split('pub fn claim_next_fair_on')[1]
oldliteral=re.search(r'let sql = format!\(\s*"(UPDATE semantic_outbox.*?lease_expires_at)"',old,re.S).group(1)
newliteral=re.search(r'ClaimFairness::DocRoundRobin => format!\(\s*"(UPDATE semantic_outbox.*?lease_expires_at)"',new,re.S).group(1)
assert oldliteral==newliteral
unchanged=['crates/cc-db/src/semantic_queue.rs','crates/cc-db/src/direct_writer.rs','crates/cc-db/src/index_db.rs','crates/cc-db/src/index_db_rebuild.rs','crates/cc-semantic/src/queue.rs','crates/cc-server/src/semantic_runtime.rs']
assert all(blob(shas[0],f)==blob(shas[2],f) for f in unchanged)
save('source-audit.json',{'DocRoundRobin_format_literal_byte_equal':True,'unchanged_lifecycle_open_rebuild_sources':unchanged,'direct_rebuild_reason':'split_sql_statements splits trigger body at semicolon; standalone INSERT using new.rowid enters extract_table_statements. Both fixed old and candidate fail before deferred-index creation.'})
reset={}
for v in [21,22,23]:
    c=sqlite3.connect(p/'local/run-2'/f'mismatch-v{v}.sqlite3');meta=dict(c.execute('SELECT * FROM metadata'))
    assert meta['index_epoch']=='18' and meta['evidence_epoch']=='24' and 'semantic_epoch' not in meta
    reset[str(v)]={'user_version':c.execute('PRAGMA user_version').fetchone()[0],'metadata':meta,'files':c.execute('SELECT count(*) FROM files').fetchone()[0],'indexes':list(c.execute("PRAGMA index_list('semantic_outbox')"))};c.close()
save('legacy-reset-summary.json',{'fixtures':'PR110 binary generated actual old v24 DDL + synthetic user_version 21/22/23; no historical legacy executable claim','observations':reset})
for n in ['old','candidate']:
    d=p/'local'/('driver-'+n)
    (p/(n+'-driver-Cargo.toml')).write_bytes((d/'Cargo.toml').read_bytes());(p/(n+'-driver-Cargo.lock')).write_bytes((d/'Cargo.lock').read_bytes())
prov=json.loads((p/'provenance.json').read_text())
prov['toolchain']=subprocess.check_output(['/workspace/.rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin/rustc','--version'],text=True).strip()
prov['binaries']={n:hashlib.sha256((p/'local/target/debug'/('independent-fifo-'+n)).read_bytes()).hexdigest() for n in ['old','candidate']}
prov['sqlite_production']='3.53.2'
prov['features']='cc-db and cc-semantic default (no optional features); rusqlite bundled,vtab,functions; debug unoptimized'
prov['not_run']=['full codecortex CLI/server binary','semantic-http/HTTP provider','historical v21/v22/v23 binaries or DDL','100k or performance AB','GC/WAL/crash/fault','real user databases','RO permission retry','author tests as independent evidence','workspace lint/test suite']
save('provenance.json',prov)
(p/'mutant-correction.log').write_text('Supplementary Python mutant harness initially failed with incorrect binding count after removing ?5. review-run-v2.log retains failure. Bind count now derives from exact maximum numbered parameter; six mutants killed across 324 two-row cases. Production Rust checks not repeated.\n')
print('source identity, RR literal, legacy reset and binary provenance verified')
