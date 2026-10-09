import pathlib,sqlite3,time,json,statistics,re
O=pathlib.Path(__file__).resolve().parent
src=(O/'source/crates/cc-db/src/sql/index_v1.sql').read_text();schema=src[src.index('CREATE TABLE IF NOT EXISTS semantic_outbox ('):src.index('-- active space')]
claim="""UPDATE semantic_outbox SET state='claimed',lease_token=lower(hex(randomblob(16))),lease_expires_at=?1+?2,claim_owner=?3,attempt_count=attempt_count+1,updated_at=?4 WHERE task_id=(SELECT task_id FROM semantic_outbox WHERE space_id=?5 AND state='pending' AND available_at<=?1 ORDER BY task_id LIMIT 1) RETURNING task_id,lease_token,doc_key,doc_version,input_digest,op,lease_expires_at"""
params=(1000.,60.,'synthetic','1970-01-01T00:16:40Z','active')
results=[]
for n in [1000,5000,10000]:
 for mix in ['all_pending','mixed']:
  for drained in [0,.5,.9]:
   for variant in ['existing','candidate_fifo']:
    p=O/f'sql-{n}-{mix}-{int(drained*100)}-{variant}.sqlite3';c=sqlite3.connect(p);c.executescript(schema)
    rows=[]
    for i in range(n):
     state='pending';space='active';avail=900.;expiry=None
     if mix=='mixed':
      x=i%10
      if x<2:avail=2000.
      elif x<4:space='other'
      elif x==4:state='claimed';expiry=2000.
      elif x==5:state='done'
     if i<int(n*drained) and state=='pending' and space=='active' and avail<=1000:state='done'
     rows.append((f'doc{i}',f'v{i}',f'in{i}',space,state,avail,expiry))
    c.executemany("INSERT INTO semantic_outbox(doc_key,doc_version,input_digest,space_id,op,state,available_at,lease_expires_at,created_at,updated_at) VALUES(?,?,?,?,'embed',?,?,?,'created','updated')",rows);c.commit()
    if variant=='candidate_fifo':c.execute("CREATE INDEX diagnostic_fifo_pending ON semantic_outbox(space_id,task_id,available_at) WHERE state='pending'");c.commit()
    dist=dict(c.execute('SELECT state,count(*) FROM semantic_outbox GROUP BY state'))
    categories={'ready_active':c.execute("SELECT count(*) FROM semantic_outbox WHERE state='pending' AND space_id='active' AND available_at<=1000").fetchone()[0],'future_active':c.execute("SELECT count(*) FROM semantic_outbox WHERE state='pending' AND space_id='active' AND available_at>1000").fetchone()[0],'pending_other_space':c.execute("SELECT count(*) FROM semantic_outbox WHERE state='pending' AND space_id!='active'").fetchone()[0]}
    plan=[list(x) for x in c.execute('EXPLAIN QUERY PLAN '+claim,params)];vm=[list(x) for x in c.execute('EXPLAIN '+claim,params)]
    timing=[];ids=[]
    for k in range(10):
     c.execute('BEGIN');t=time.perf_counter_ns();res=c.execute(claim,params).fetchall();timing.append((time.perf_counter_ns()-t)/1e6);ids.append(res[0][0] if res else None);c.rollback()
    steps=[0]
    def callback():steps[0]+=1;return 0
    c.execute('BEGIN');c.set_progress_handler(callback,1);res=c.execute(claim,params).fetchall();c.set_progress_handler(None,0);c.rollback()
    # Exact semantics check against independent FIFO predicate.
    expected=c.execute("SELECT min(task_id) FROM semantic_outbox WHERE state='pending' AND space_id='active' AND available_at<=1000").fetchone()[0];assert set(ids)=={expected}
    results.append({'n':n,'mix':mix,'drained_fraction':drained,'variant':variant,'state_distribution':dist,'categories':categories,'selected_task':expected,'plan':plan,'sort_opcodes':[x for x in vm if 'Sort' in x[1] or 'Ephemeral' in x[1]],'vm_steps':steps[0],'timing_ms_median':statistics.median(timing),'timing_ms_samples':timing,'note':'Full production FIFO UPDATE RETURNING with original indexes; independent synthetic database. Timings exclude commit because transaction rolls back; VM steps measured separately; not whole pipeline.'})
    c.close()
(O/'sql-probe.json').write_text(json.dumps({'sqlite_version':sqlite3.sqlite_version,'claim_sql':claim,'candidate_index':"CREATE INDEX diagnostic_fifo_pending ON semantic_outbox(space_id,task_id,available_at) WHERE state='pending'",'results':results},indent=2)+'\n')
for r in results:
 if r['drained_fraction']==0:print(r['n'],r['mix'],r['variant'],r['categories'],r['vm_steps'],round(r['timing_ms_median'],4),r['plan'])
