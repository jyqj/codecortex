"""Finite independent correctness review; all writes confined to this directory.
Run from repository root: python3 artifacts/checkpoints/independent-fifo-claim-review-20261003/review.py
Requires prepared fixed git archives and built custom drivers (see README).
"""
from pathlib import Path
import hashlib,itertools,json,re,sqlite3,subprocess
P=Path(__file__).resolve().parent
RESULTS=[]
def save(name,data):
    (P/name).write_text(json.dumps(data,indent=2,ensure_ascii=False)+'\n')
def run(label,variant,mode,path=None):
    args=[str(P/'local/target/debug'/('independent-fifo-'+variant)),mode]
    if path: args.append(str(path))
    r=subprocess.run(args,text=True,capture_output=True)
    (P/(label+'.log')).write_text('argv='+json.dumps(args)+'\nexit='+str(r.returncode)+'\n'+r.stdout+r.stderr)
    RESULTS.append({'label':label,'exit':r.returncode})
    save('results.json',RESULTS)
    assert r.returncode==0,label
def snapshot(path):
    c=sqlite3.connect(path)
    def values(row):
        return [({'blob_hex':v.hex()} if isinstance(v,bytes) else v) for v in row]
    names=[r[0] for r in c.execute("SELECT name FROM sqlite_schema WHERE type='table' ORDER BY name")]
    tables={n:sorted([values(r) for r in c.execute('SELECT * FROM "'+n.replace('"','""')+'"')],key=lambda r:json.dumps(r,sort_keys=True)) for n in names}
    out={'tables':tables,'user_version':c.execute('PRAGMA user_version').fetchone()[0],
         'schema':list(c.execute('SELECT type,name,tbl_name,sql FROM sqlite_schema ORDER BY type,name')),
         'integrity':list(c.execute('PRAGMA integrity_check')),'foreign_keys':list(c.execute('PRAGMA foreign_key_check'))}
    c.close();return out
def copy_db(source,dest):
    a=sqlite3.connect(source);b=sqlite3.connect(dest);a.backup(b);b.close();a.close()
def logical_old_open():
    old=P/'local/true-old-v24.sqlite3'
    if old.exists():
        # Retain successful fixed-old generation/open evidence; resume only.
        before=json.loads((P/'old-v24-before.json').read_text())
        after=json.loads((P/'old-v24-after.json').read_text())
    else:
        run('true-old-v24-generation','old','seed',old)
        before=snapshot(old);save('old-v24-before.json',before)
        run('candidate-true-old-v24-open-reopen','candidate','open',old)
        after=snapshot(old);save('old-v24-after.json',after)
    assert before['user_version']==24
    assert not any(r[1]=='semantic_outbox_fifo_pending' for r in before['schema'])
    assert before['tables']==after['tables'] and after['user_version']==24
    added=[r for r in after['schema'] if r not in before['schema']]
    removed=[r for r in before['schema'] if r not in after['schema']]
    assert removed==[] and len(added)==1 and added[0][1]=='semantic_outbox_fifo_pending'
    assert [list(r) for r in after['integrity']]==[['ok']] and after['foreign_keys']==[]
    save('old-v24-comparison.json',{'logical_tables_equal':True,'table_count':len(before['tables']),
      'all_metadata_and_files_chunks_document_semantic_manifests_equal':True,'user_version_equal':True,
      'sqlite_schema_added':added,'sqlite_schema_removed':removed,
      'note':'File bytes, rootpages, schema_version, WAL frames excluded as physical SQLite changes. Source content is a persisted unchanged-hash/text fixture; no real reparse/provider run.'})
    # Actual legacy-version fixture DDL is the fixed PR110 DDL, explicitly synthetic
    # version tags. This exercises normal destructive mismatch handling, not a
    # historical v21/22/23 product executable or an additive migration.
    for v in [21,22,23]:
        path=P/'local/run-2'/f'mismatch-v{v}.sqlite3'
        run(f'mismatch-v{v}-generate','old','seed',path)
        c=sqlite3.connect(path);c.execute(f'PRAGMA user_version={v}');c.commit();c.close()
        run(f'mismatch-v{v}-reset','candidate','reset',path)
        after=snapshot(path)
        assert after['user_version']==24 and after['tables']['files']==[] and after['tables']['document_manifest']==[] and after['tables']['semantic_manifest']==[]
    run('fresh-temp-direct-paths','candidate','paths',P/'local/fresh.sqlite3')
def mutants():
    src=(P/'local/candidate/crates/cc-db/src/semantic_outbox.rs').read_text()
    literal=re.search(r'ClaimFairness::Fifo => "(UPDATE semantic_outbox.*?lease_expires_at)"\s*\.to_owned',src,re.S).group(1)
    sql=re.sub(r'\\\n\s*','',literal)
    (P/'extracted-production-fifo.sql').write_text(sql+'\n')
    # SQLite Python is a supplementary mutant harness, NOT production verification.
    c=sqlite3.connect(':memory:')
    c.executescript((P/'local/old/crates/cc-db/src/sql/index_v1.sql').read_text())
    c.execute("CREATE INDEX semantic_outbox_fifo_pending ON semantic_outbox(space_id,task_id,available_at) WHERE state='pending'")
    samples=[]
    choices=list(itertools.product(['pending','done','claimed'],['active','other'],[99.,100.,101.]))
    variants={'reverse_fifo':sql.replace('ORDER BY task_id ASC','ORDER BY task_id DESC'),
      'strict_readiness':sql.replace('available_at<=?1','available_at<?1'),
      'ready_time_order':sql.replace('ORDER BY task_id ASC','ORDER BY available_at ASC, task_id ASC'),
      'unfenced_space':sql.replace(' AND space_id=?5','').replace('space_id=?5 AND ',''),
      'include_terminal':sql.replace('INDEXED BY semantic_outbox_fifo_pending ','').replace("WHERE space_id=?5 AND state='pending' AND available_at<=?1","WHERE space_id=?5 AND available_at<=?1"),
      'guard_false':sql.replace('CASE WHEN EXISTS','CASE WHEN NOT EXISTS')}
    killed={k:None for k in variants}
    for rows in itertools.product(choices,repeat=2):
        expected=next((i+1 for i,r in enumerate(rows) if r[0]=='pending' and r[1]=='active' and r[2]<=100.),None)
        for name,query in [('production',sql)]+[(k,v) for k,v in variants.items() if killed[k] is None]:
            c.execute('DELETE FROM semantic_outbox')
            for i,(state,sp,at) in enumerate(rows):
                c.execute("INSERT INTO semantic_outbox(task_id,doc_key,doc_version,input_digest,space_id,op,state,available_at,created_at,updated_at) VALUES(?,?,'v','d',?,'embed',?,?,'old','old')",(i+1,f'd{i}',sp,state,at))
            n=max(map(int,re.findall(r'\?(\d+)',query)))
            r=c.execute(query,(100.,10.,'oracle','updated','active')[:n]).fetchone();got=r[0] if r else None
            if name=='production':assert got==expected
            elif got!=expected:killed[name]={'rows':rows,'expected':expected,'got':got}
    assert all(killed.values()),killed
    save('mutants.json',{'python_sqlite':sqlite3.sqlite_version,'actual_sql_sha256':hashlib.sha256(sql.encode()).hexdigest(),'two_row_cases':len(choices)**2,'killed':killed,'scope':'synthetic SQL mutation sensitivity only; production claims tested by Rust bundled SQLite separately'})
def main():
    (P/'local/run-2').mkdir(exist_ok=True)
    logical_old_open()
    for variant in ['old','candidate']:
        run(variant+'-finite',variant,'finite')
        run(variant+'-facade',variant,'facade',P/'local'/(variant+'-race.sqlite3'))
        run(variant+'-cancel',variant,'cancel',P/'local'/(variant+'-cancel.sqlite3'))
    observed=[]
    for variant in ['old','candidate']:
        args=[str(P/'local/target/debug'/('independent-fifo-'+variant)),'direct',str(P/'local'/(variant+'-direct.sqlite3'))]
        r=subprocess.run(args,text=True,capture_output=True)
        (P/(variant+'-direct.log')).write_text('argv='+json.dumps(args)+'\nexit='+str(r.returncode)+'\n'+r.stdout+r.stderr)
        observed.append({'variant':variant,'exit':r.returncode,'output':r.stdout,'stderr':r.stderr})
    assert all(r['exit']==2 and 'no such column: new.rowid' in r['output'] for r in observed)
    save('direct-rebuild-failure-comparison.json',{'observations':observed,'classification':'pre-existing shared production direct-writer schema splitter failure, not introduced by FIFO diff; this path is FAILED, not a passing index-availability check'})
    mutants();save('results.json',RESULTS);print('bounded checks completed; direct rebuild shared baseline failure retained; no performance acceptance implied')
if __name__=='__main__':main()
