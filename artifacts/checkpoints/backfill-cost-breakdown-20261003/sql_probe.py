#!/usr/bin/env python3
"""Finite 1k/5k in-memory SQL diagnostic. Not a product/provider/backfill run.

Use exact outbox DDL and claim candidate query at fixed source. Same source(i,0)
generator binds synthetic doc digests. No alternate index, writes or hooks in core.
"""
import hashlib, json, pathlib, platform, re, sqlite3, statistics, subprocess, time
OUT=pathlib.Path(__file__).resolve().parent
ROOT=OUT.parents[2]
SHA='574f7598662334c63e020da136c87f4f7281554d'
def read(path):
    return subprocess.check_output(['git','show',SHA+':'+path],cwd=ROOT).decode()
driver=read('scripts/p7_release_resource_preparation.py')
generator_line=next(x for x in driver.splitlines() if x.startswith('def source('))
scope={};exec(generator_line,scope);source=scope['source']
schema=read('crates/cc-db/src/sql/index_v1.sql')
ddl=schema[schema.index('CREATE TABLE IF NOT EXISTS semantic_outbox ('):schema.index('-- active space 指针')]
sql='SELECT task_id FROM semantic_outbox WHERE space_id=?1 AND state=\'pending\' AND available_at<=?2 ORDER BY task_id ASC LIMIT 1'
results=[]
for n in [1000,5000]:
    db=sqlite3.connect(':memory:');db.executescript(ddl)
    aggregate=hashlib.sha256()
    rows=[]
    for i in range(n):
        b=source(i,value=0).encode();aggregate.update(b)
        # Fixture keys are synthetic surrogate hashes, not product DocKey/DocVersion.
        digest=hashlib.sha256(b).hexdigest()
        rows.append((digest,digest,digest,'synthetic','embed','pending',1.0,'0','0'))
    db.executemany('INSERT INTO semantic_outbox(doc_key,doc_version,input_digest,space_id,op,state,available_at,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?)',rows);db.commit()
    for done in [0,n//4,3*n//4,9*n//10]:
        db.execute("UPDATE semantic_outbox SET state=CASE WHEN task_id<=? THEN 'done' ELSE 'pending' END",(done,));db.commit()
        plan=db.execute('EXPLAIN QUERY PLAN '+sql,('synthetic',2.0)).fetchall()
        vm=db.execute('EXPLAIN '+sql,('synthetic',2.0)).fetchall()
        timings=[]
        for _ in range(128):
            t=time.perf_counter_ns();v=db.execute(sql,('synthetic',2.0)).fetchone();timings.append((time.perf_counter_ns()-t)/1e6)
            assert v[0]==done+1
        steps=[0]
        def tick(): steps[0]+=1;return 0
        db.set_progress_handler(tick,1)
        assert db.execute(sql,('synthetic',2.0)).fetchone()[0]==done+1
        db.set_progress_handler(None,0)
        results.append({'n':n,'done':done,'pending':n-done,'plan':plan,'vm_opcodes':vm,'vm_steps_one_query':steps[0],'repetitions':128,'latency_ms_mean':statistics.mean(timings),'latency_ms_min':min(timings),'latency_ms_max':max(timings),'source_bytes_aggregate_sha256':aggregate.hexdigest()})
    db.close()
report={'scope':'one finite diagnostic, 1k/5k generated inputs; in-memory candidate SELECT only; no HTTP/provider/product/cache/transactions or concurrency test', 'source_sha':SHA,'generator_sha256':hashlib.sha256(driver.encode()).hexdigest(),'generator_line':generator_line,'schema_segment_sha256':hashlib.sha256(ddl.encode()).hexdigest(),'sql':sql,'python':platform.python_version(),'sqlite_runtime':sqlite3.sqlite_version,'differences':['Python sqlite runtime may differ from Cargo bundled SQLite; inspect dependency identity separately.', 'In-memory warm SQL timing excludes disk sync, locks, claim UPDATE/RETURNING and publish. Does not estimate 100k absolute latency.', 'Fixture has one active space, identical available_at=1.0, no retries. Done prefix is a modeled FIFO progression.', 'Hashes bind generator bytes only; these are surrogate keys, not product encoding/DocKey.'], 'results':results}
(OUT/'sql-probe.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps({'sqlite_runtime':report['sqlite_runtime'],'results':[{k:v for k,v in r.items() if k not in ['vm_opcodes']} for r in results]},indent=2))
